//! AVIF: opened with `avif-decode` (rav1d, the Rust port of dav1d; BSD) and saved with
//! `ravif` (rav1e; BSD). Windows can open AVIF too, but its decoder drops the alpha and it
//! has no AV1 encoder. Both keep alpha. Building rav1d / rav1e needs NASM on PATH.

use std::io::Write;
use std::path::Path;

use avif_decode::Image;

use crate::ora::write_atomically;
use crate::stack::MAX_SIDE;

fn to8(v: u16) -> u8 {
    ((v as u32 * 255 + 32767) / 65535) as u8
}

/// Decodes an AVIF file to straight RGBA8.
pub fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = avif_decode::Decoder::from_avif(&data)
        .and_then(|d| d.to_image())
        .map_err(|e| e.to_string())?;
    let (w, h) = match &img {
        Image::Rgb8(i) => (i.width(), i.height()),
        Image::Rgb16(i) => (i.width(), i.height()),
        Image::Rgba8(i) => (i.width(), i.height()),
        Image::Rgba16(i) => (i.width(), i.height()),
        Image::Gray8(i) => (i.width(), i.height()),
        Image::Gray16(i) => (i.width(), i.height()),
    };
    if w == 0 || h == 0 || w > MAX_SIDE as usize || h > MAX_SIDE as usize {
        return Err(format!("Unsupported image size {w} x {h}"));
    }
    let mut out = Vec::with_capacity(w * h * 4);
    match &img {
        Image::Rgb8(i) => i
            .rows()
            .flatten()
            .for_each(|p| out.extend([p.r, p.g, p.b, 255])),
        Image::Rgba8(i) => i
            .rows()
            .flatten()
            .for_each(|p| out.extend([p.r, p.g, p.b, p.a])),
        Image::Gray8(i) => i.rows().flatten().for_each(|p| {
            let v = p.value();
            out.extend([v, v, v, 255])
        }),
        Image::Rgb16(i) => i
            .rows()
            .flatten()
            .for_each(|p| out.extend([to8(p.r), to8(p.g), to8(p.b), 255])),
        Image::Rgba16(i) => i
            .rows()
            .flatten()
            .for_each(|p| out.extend([to8(p.r), to8(p.g), to8(p.b), to8(p.a)])),
        Image::Gray16(i) => i.rows().flatten().for_each(|p| {
            let v = to8(p.value());
            out.extend([v, v, v, 255])
        }),
    }
    Ok((w as u32, h as u32, out))
}

/// Encodes straight RGBA8 as AVIF with `quality` 0..=100.
pub fn encode(
    path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<(), String> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("pixel buffer size does not match the image size".into());
    }
    let px: Vec<ravif::RGBA8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
        .collect();
    let img = ravif::Img::new(&px[..], width as usize, height as usize);
    let q = quality.clamp(1, 100) as f32;
    let out = ravif::Encoder::new()
        .with_quality(q)
        .with_alpha_quality(q)
        .with_speed(6)
        .encode_rgba(img)
        .map_err(|e| e.to_string())?;
    write_atomically(path, |w| {
        w.write_all(&out.avif_file).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_alpha() {
        let rgba: Vec<u8> = (0..64 * 48u32)
            .flat_map(|i| {
                [
                    (i % 64 * 4) as u8,
                    (i / 64 * 5) as u8,
                    128,
                    if i % 64 < 32 { 0 } else { 255 },
                ]
            })
            .collect();
        let dir = std::env::temp_dir().join(format!("farba-avif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.avif");
        encode(&path, &rgba, 64, 48, 90).unwrap();
        let (w, h, px) = decode(&path).unwrap();
        assert_eq!((w, h), (64, 48));
        let i = (24 * 64 + 48) * 4;
        let close = px[i..i + 4]
            .iter()
            .zip(&rgba[i..i + 4])
            .all(|(a, b)| (*a as i32 - *b as i32).abs() < 12);
        assert!(close, "{:?}", &px[i..i + 4]);
        assert!(px[(24 * 64 + 8) * 4 + 3] < 10);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
