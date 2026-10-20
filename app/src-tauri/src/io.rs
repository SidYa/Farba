//! Image file encoding (Save) and the clipboard. Pixels arrive as straight RGBA8.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Jpeg,
    Webp,
    /// Through the Windows codecs; opaque (flattened over the background colour).
    Heic,
    Avif,
}

/// Drops alpha when every pixel is opaque (a smaller PNG, as GIMP does for images without
/// transparency), otherwise keeps RGBA.
fn rgb_if_opaque(rgba: &[u8]) -> Option<Vec<u8>> {
    let px = rgba.as_chunks::<4>().0;
    if !px.iter().all(|p| p[3] == 255) {
        return None;
    }
    Some(px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect())
}

/// Composites over `bg` (JPEG has no alpha; GIMP flattens with the background colour).
fn flatten_rgb(rgba: &[u8], bg: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for p in rgba.as_chunks::<4>().0 {
        let a = p[3] as u32;
        for c in 0..3 {
            out.push(((p[c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255) as u8);
        }
    }
    out
}

fn encode(
    w: &mut impl Write,
    rgba: &[u8],
    width: u32,
    height: u32,
    format: Format,
    quality: u8,
    background: [u8; 3],
) -> image::ImageResult<()> {
    match format {
        Format::Png => {
            // Compression 9, no metadata (the owner's GIMP export settings).
            let enc = PngEncoder::new_with_quality(w, CompressionType::Best, FilterType::Adaptive);
            match rgb_if_opaque(rgba) {
                Some(rgb) => enc.write_image(&rgb, width, height, ExtendedColorType::Rgb8),
                None => enc.write_image(rgba, width, height, ExtendedColorType::Rgba8),
            }
        }
        Format::Jpeg => {
            let rgb = flatten_rgb(rgba, background);
            JpegEncoder::new_with_quality(w, quality.clamp(1, 100)).write_image(
                &rgb,
                width,
                height,
                ExtendedColorType::Rgb8,
            )
        }
        Format::Webp => {
            WebPEncoder::new_lossless(w).write_image(rgba, width, height, ExtendedColorType::Rgba8)
        }
        Format::Heic | Format::Avif => unreachable!("written by farba-io"),
    }
}

/// Writes the image to `path` through a temporary file, so a failed save never leaves a
/// half-written file in place of the old one.
pub fn save(
    path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
    format: Format,
    quality: u8,
    background: [u8; 3],
) -> Result<(), String> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("pixel buffer size does not match the image size".into());
    }
    match format {
        // HEIF through WIC cannot keep alpha: flatten like JPEG.
        #[cfg(windows)]
        Format::Heic => {
            let rgb = flatten_rgb(rgba, background);
            return farba_io::wic::encode_heic(path, &rgb, width, height, quality);
        }
        #[cfg(not(windows))]
        Format::Heic => return Err("Saving HEIC needs the Windows codecs".into()),
        Format::Avif => return farba_io::avif::encode(path, rgba, width, height, quality),
        _ => {}
    }
    let mut tmp = PathBuf::from(path);
    tmp.as_mut_os_string().push(".farba-tmp");
    let result = (|| -> Result<(), String> {
        let file = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut w = BufWriter::new(file);
        encode(&mut w, rgba, width, height, format, quality, background)
            .map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        drop(w);
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn clipboard_set(rgba: Vec<u8>, width: u32, height: u32) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: rgba.into(),
    })
    .map_err(|e| e.to_string())
}

/// The clipboard image as straight RGBA8 `(width, height, pixels)`.
pub fn clipboard_get() -> Result<(u32, u32, Vec<u8>), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let img = cb.get_image().map_err(|e| match e {
        arboard::Error::ContentNotAvailable => "The clipboard does not contain an image".into(),
        e => e.to_string(),
    })?;
    Ok((img.width as u32, img.height as u32, img.bytes.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_uses_background() {
        assert_eq!(
            flatten_rgb(&[255, 0, 0, 0, 255, 0, 0, 255], [10, 20, 30]),
            vec![10, 20, 30, 255, 0, 0]
        );
    }

    #[test]
    fn save_round_trip_png_and_jpeg() {
        let dir = std::env::temp_dir().join(format!("farba-io-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rgba: Vec<u8> = (0..16 * 8).flat_map(|i| [i as u8, 50, 200, 128]).collect();
        let png = dir.join("a.png");
        save(&png, &rgba, 16, 8, Format::Png, 100, [255; 3]).unwrap();
        let back = image::open(&png).unwrap().into_rgba8();
        assert_eq!(back.into_raw(), rgba);
        let jpg = dir.join("a.jpg");
        save(&jpg, &rgba, 16, 8, Format::Jpeg, 100, [255; 3]).unwrap();
        assert_eq!(image::open(&jpg).unwrap().width(), 16);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
