//! Single-image files (PNG, JPEG, WebP, AVIF, HEIC / RAW through WIC) opened as straight
//! RGBA8, the right way up (EXIF orientation applied).
//!
//! Decoding is split in two so the caller can allocate the destination (a WebView2 shared
//! buffer) once the size is known: [`open`] decodes, [`Decoded::write_rgba`] converts and
//! orients straight into the destination on all cores, without an intermediate RGBA copy.

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::stack::MAX_SIDE;

/// A decoded image waiting to be written as RGBA8. `width` / `height` are the size after
/// orientation.
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pixels: Pixels,
}

enum Pixels {
    /// Packed 3 (RGB) or 4 (RGBA) channels in the stored orientation, plus the EXIF
    /// orientation (1-8) to apply.
    Packed {
        data: Vec<u8>,
        channels: usize,
        width: u32,
        height: u32,
        orientation: u8,
    },
    /// An upright WIC source, decoded by `CopyPixels` straight into the destination.
    #[cfg(windows)]
    Wic(crate::wic::Source),
}

impl Decoded {
    /// Straight RGBA8 pixels that are already upright.
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self::packed(data, 4, width, height, 1)
    }

    fn packed(data: Vec<u8>, channels: usize, width: u32, height: u32, orientation: u8) -> Self {
        let (w, h) = oriented_size(width, height, orientation);
        Decoded {
            width: w,
            height: h,
            pixels: Pixels::Packed {
                data,
                channels,
                width,
                height,
                orientation,
            },
        }
    }

    /// Straight RGBA8 pixels stored with EXIF `orientation` (1-8).
    #[cfg(windows)]
    pub(crate) fn rotated(data: Vec<u8>, width: u32, height: u32, orientation: u8) -> Self {
        Self::packed(data, 4, width, height, orientation)
    }

    #[cfg(windows)]
    pub(crate) fn wic(source: crate::wic::Source) -> Self {
        let (width, height) = source.size;
        Decoded {
            width,
            height,
            pixels: Pixels::Wic(source),
        }
    }

    /// The number of bytes [`write_rgba`](Self::write_rgba) fills.
    pub fn rgba_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// Writes the upright straight RGBA8 pixels into `dst` (`rgba_len()` bytes).
    pub fn write_rgba(self, dst: &mut [u8]) -> Result<(), String> {
        if dst.len() != self.rgba_len() {
            return Err("destination size does not match the image".into());
        }
        match self.pixels {
            Pixels::Packed {
                data,
                channels,
                width,
                height,
                orientation,
            } => {
                orient(&data, channels, width, height, orientation, dst);
                Ok(())
            }
            #[cfg(windows)]
            Pixels::Wic(source) => source.copy_to(dst),
        }
    }

    /// The upright straight RGBA8 pixels in a new buffer.
    pub fn into_rgba(self) -> Result<Vec<u8>, String> {
        if let Pixels::Packed {
            channels: 4,
            orientation: 1,
            ..
        } = self.pixels
        {
            // Without WIC (not Windows) `Packed` is the only variant.
            #[cfg_attr(not(windows), allow(irrefutable_let_patterns))]
            let Pixels::Packed { data, .. } = self.pixels else {
                unreachable!()
            };
            return Ok(data);
        }
        let mut out = vec![0; self.rgba_len()];
        self.write_rgba(&mut out)?;
        Ok(out)
    }
}

/// Camera RAW extensions (opened through Windows' Raw Image Extension).
#[cfg(windows)]
pub use crate::wic::RAW_EXTENSIONS;

/// Decodes a single-image file, picking the decoder by extension (HEIC / HEIF, AVIF, camera
/// RAW, SVG) or by content (PNG, JPEG, WebP).
pub fn open(path: &Path) -> Result<Decoded, String> {
    open_sized(path, None)
}

/// [`open`], with the size to render a vector image (SVG) at; `None` = its own size.
pub fn open_sized(path: &Path, size: Option<(u32, u32)>) -> Result<Decoded, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        #[cfg(windows)]
        "heic" | "heif" => crate::wic::open(path, crate::wic::Codec::Heif),
        #[cfg(windows)]
        e if RAW_EXTENSIONS.contains(&e) => crate::wic::open(path, crate::wic::Codec::Raw),
        "svg" | "svgz" => {
            let (w, h) = match size {
                Some(s) => s,
                None => crate::svg::default_size(path)?,
            };
            Ok(Decoded::from_rgba(w, h, crate::svg::render(path, w, h)?))
        }
        "avif" => {
            let (w, h, rgba) = crate::avif::decode(path)?;
            Ok(Decoded::from_rgba(w, h, rgba))
        }
        _ => open_image(path).map_err(|e| format!("{}: {e}", path.display())),
    }
}

/// PNG / JPEG / WebP through the `image` crate, with the EXIF orientation.
fn open_image(path: &Path) -> image::ImageResult<Decoded> {
    let mut decoder = ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation().map(|o| o.to_exif()).unwrap_or(1);
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(image::ImageError::Unsupported(
            image::error::UnsupportedError::from_format_and_kind(
                image::error::ImageFormatHint::Unknown,
                image::error::UnsupportedErrorKind::GenericFeature(format!("image size {w} x {h}")),
            ),
        ));
    }
    let (data, channels) = match DynamicImage::from_decoder(decoder)? {
        DynamicImage::ImageRgb8(i) => (i.into_raw(), 3),
        DynamicImage::ImageRgba8(i) => (i.into_raw(), 4),
        other => (other.into_rgba8().into_raw(), 4),
    };
    Ok(Decoded::packed(data, channels, w, h, orientation))
}

/// The size after applying EXIF `orientation` (5-8 swap the sides).
fn oriented_size(w: u32, h: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (h, w)
    } else {
        (w, h)
    }
}

/// Copies `src` (`channels` = 3 or 4, `w` x `h`) into `dst` as RGBA8 with EXIF `orientation`
/// applied, rows split over all cores.
fn orient(src: &[u8], channels: usize, w: u32, h: u32, orientation: u8, dst: &mut [u8]) {
    let (w, h) = (w as usize, h as usize);
    let (dw, _) = oriented_size(w as u32, h as u32, orientation);
    let row_len = dw as usize * 4;
    if row_len == 0 {
        return;
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let rows = dst.len() / row_len;
    let per = rows.div_ceil(threads).max(16);
    std::thread::scope(|s| {
        for (i, chunk) in dst.chunks_mut(per * row_len).enumerate() {
            s.spawn(move || {
                for (j, row) in chunk.chunks_exact_mut(row_len).enumerate() {
                    let y = i * per + j;
                    for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        // Source pixel of destination (x, y), from the EXIF definitions.
                        let (sx, sy) = match orientation {
                            2 => (w - 1 - x, y),
                            3 => (w - 1 - x, h - 1 - y),
                            4 => (x, h - 1 - y),
                            5 => (y, x),
                            6 => (y, h - 1 - x),
                            7 => (w - 1 - y, h - 1 - x),
                            8 => (w - 1 - y, x),
                            _ => (x, y),
                        };
                        let k = (sy * w + sx) * channels;
                        px[..3].copy_from_slice(&src[k..k + 3]);
                        px[3] = if channels == 4 { src[k + 3] } else { 255 };
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::metadata::Orientation;

    #[test]
    fn orientation_matches_the_image_crate() {
        let (w, h) = (5u32, 3u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [i as u8, 0, 7, 200]).collect();
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        for o in 1..=8u8 {
            let mut expected =
                DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, rgba.clone()).unwrap());
            expected.apply_orientation(Orientation::from_exif(o).unwrap());
            let expected = expected.into_rgba8();
            let d = Decoded::packed(rgba.clone(), 4, w, h, o);
            assert_eq!(
                (d.width, d.height),
                expected.dimensions(),
                "orientation {o}"
            );
            assert_eq!(
                d.into_rgba().unwrap(),
                expected.as_raw().clone(),
                "rgba {o}"
            );
            let mut want_rgb = expected.into_raw();
            want_rgb
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p[3] = 255);
            let d = Decoded::packed(rgb.clone(), 3, w, h, o);
            assert_eq!(d.into_rgba().unwrap(), want_rgb, "rgb {o}");
        }
    }

    /// A JPEG with an EXIF orientation 6 (rotate 90° clockwise) opens upright.
    #[test]
    fn jpeg_exif_orientation() {
        let path = jpeg_with_orientation(6);
        let d = open(&path).unwrap();
        assert_eq!((d.width, d.height), (16, 32));
        let px = d.into_rgba().unwrap();
        // The red left half of the stored image ends up at the top.
        assert!(px[0] > 200 && px[2] < 60, "top-left {:?}", &px[..4]);
        let last = px.len() - 4;
        assert!(
            px[last] < 60 && px[last + 2] > 200,
            "bottom-right {:?}",
            &px[last..]
        );
        #[cfg(windows)]
        {
            // The WIC path (used for HEIC / RAW) reads the same EXIF tag.
            let d = crate::wic::open(&path, crate::wic::Codec::Raw).unwrap();
            assert_eq!((d.width, d.height), (16, 32));
            let px = d.into_rgba().unwrap();
            assert!(px[0] > 200 && px[2] < 60);
        }
        std::fs::remove_file(&path).unwrap();
    }

    /// A 32 x 16 JPEG, red on the left half and blue on the right, with an EXIF APP1
    /// segment holding only the orientation tag.
    pub(crate) fn jpeg_with_orientation(orientation: u16) -> std::path::PathBuf {
        let rgb: Vec<u8> = (0..32 * 16)
            .flat_map(|i| {
                if i % 32 < 16 {
                    [255, 0, 0]
                } else {
                    [0, 0, 255]
                }
            })
            .collect();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(&rgb, 32, 16, image::ExtendedColorType::Rgb8)
            .unwrap();
        // TIFF header (little endian), one IFD entry: 0x0112 SHORT 1 = orientation.
        let mut tiff = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = jpeg[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        let path = std::env::temp_dir().join(format!(
            "farba-orient-{}-{orientation}.jpg",
            std::process::id()
        ));
        std::fs::write(&path, out).unwrap();
        path
    }
}
