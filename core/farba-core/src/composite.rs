//! CPU compositing of surfaces: merging layers, flattening for export, thumbnails.
//! (On screen, layers are composited by the WebGL shader instead.)

use crate::color::blend_over;
use crate::surface::{Surface, TILE_SIZE};

const T: usize = TILE_SIZE as usize;

/// Composites `src` (placed at `src_pos` in image coordinates) over `dst` (placed at
/// `dst_pos`) in normal mode with `opacity`. Pixels outside `dst` are ignored.
pub fn composite_over(
    dst: &mut Surface,
    dst_pos: (i32, i32),
    src: &Surface,
    src_pos: (i32, i32),
    opacity: f32,
) {
    if opacity <= 0.0 {
        return;
    }
    // A src pixel (sx, sy) lands on dst pixel (sx + off_x, sy + off_y).
    let off_x = src_pos.0 as i64 - dst_pos.0 as i64;
    let off_y = src_pos.1 as i64 - dst_pos.1 as i64;
    let (dw, dh) = (dst.width() as i64, dst.height() as i64);
    let ts = TILE_SIZE as i64;
    for sty in 0..src.tiles_y() {
        for stx in 0..src.tiles_x() {
            if !src.is_allocated(stx, sty) {
                continue;
            }
            let (sx0, sy0, sw, sh) = src.tile_rect(stx, sty);
            let (sx0, sy0) = (sx0 as i64, sy0 as i64);
            let dx0 = (sx0 + off_x).max(0);
            let dy0 = (sy0 + off_y).max(0);
            let dx1 = (sx0 + sw as i64 + off_x).min(dw);
            let dy1 = (sy0 + sh as i64 + off_y).min(dh);
            if dx0 >= dx1 || dy0 >= dy1 {
                continue;
            }
            let s_tile = src.tile(stx, sty);
            for dty in dy0 / ts..=(dy1 - 1) / ts {
                for dtx in dx0 / ts..=(dx1 - 1) / ts {
                    let (tx0, ty0, tw, th) = dst.tile_rect(dtx as u32, dty as u32);
                    let (tx0, ty0) = (tx0 as i64, ty0 as i64);
                    let x0 = dx0.max(tx0);
                    let x1 = dx1.min(tx0 + tw as i64);
                    let y0 = dy0.max(ty0);
                    let y1 = dy1.min(ty0 + th as i64);
                    if x0 >= x1 || y0 >= y1 {
                        continue;
                    }
                    let d_tile = dst.tile_mut(dtx as u32, dty as u32);
                    let n = (x1 - x0) as usize * 4;
                    for y in y0..y1 {
                        let si = ((y - off_y - sy0) as usize * T + (x0 - off_x - sx0) as usize) * 4;
                        let di = ((y - ty0) as usize * T + (x0 - tx0) as usize) * 4;
                        let s_row = s_tile[si..si + n].as_chunks::<4>().0;
                        let d_row = d_tile[di..di + n].as_chunks_mut::<4>().0;
                        for (d, s) in d_row.iter_mut().zip(s_row) {
                            blend_over(d, s, opacity);
                        }
                    }
                }
            }
        }
    }
}

/// Downscaled straight RGBA8 copy of `src` (`w * h * 4` bytes), sampled on a 4x4 grid per
/// output pixel and averaged in premultiplied form. Cheap enough to redraw often.
pub fn thumbnail(src: &Surface, w: u32, h: u32) -> Vec<u8> {
    const N: u32 = 4;
    let mut out = vec![0u8; w as usize * h as usize * 4];
    if w == 0 || h == 0 || src.width() == 0 || src.height() == 0 {
        return out;
    }
    let sx = src.width() as f32 / w as f32;
    let sy = src.height() as f32 / h as f32;
    for oy in 0..h {
        for ox in 0..w {
            let mut acc = [0f32; 4];
            for j in 0..N {
                let py =
                    (((oy as f32 + (j as f32 + 0.5) / N as f32) * sy) as u32).min(src.height() - 1);
                for i in 0..N {
                    let px = (((ox as f32 + (i as f32 + 0.5) / N as f32) * sx) as u32)
                        .min(src.width() - 1);
                    let tile = src.tile(px / TILE_SIZE, py / TILE_SIZE);
                    let k = (((py % TILE_SIZE) * TILE_SIZE + px % TILE_SIZE) * 4) as usize;
                    let a = tile[k + 3] as f32;
                    acc[0] += tile[k] as f32 * a;
                    acc[1] += tile[k + 1] as f32 * a;
                    acc[2] += tile[k + 2] as f32 * a;
                    acc[3] += a;
                }
            }
            let o = ((oy * w + ox) * 4) as usize;
            if acc[3] > 0.0 {
                for c in 0..3 {
                    out[o + c] = (acc[c] / acc[3] + 0.5) as u8;
                }
                out[o + 3] = (acc[3] / (N * N) as f32 + 0.5) as u8;
            }
        }
    }
    out
}

/// Premultiplies a tile for display (`dst` = `src` with colour scaled by alpha). Returns
/// `false` without writing when the tile is fully opaque (it can then be used as is).
pub fn premultiply_tile(src: &[u8], dst: &mut [u8]) -> bool {
    let src = src.as_chunks::<4>().0;
    if src.iter().all(|p| p[3] == 255) {
        return false;
    }
    for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
        let a = s[3] as u32;
        // Rounded c * a / 255, exact for 8-bit values.
        let mul = |c: u8| {
            let t = c as u32 * a + 128;
            ((t + (t >> 8)) >> 8) as u8
        };
        *d = [mul(s[0]), mul(s[1]), mul(s[2]), s[3]];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_with_offsets_clips_to_destination() {
        let mut dst = Surface::new(100, 100);
        let mut src = Surface::new(70, 70);
        src.fill([255, 0, 0, 255]);
        composite_over(&mut dst, (0, 0), &src, (-10, 50), 1.0);
        let rgba = dst.to_rgba();
        let px = |x: usize, y: usize| &rgba[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
        assert_eq!(px(0, 50), &[255, 0, 0, 255]);
        assert_eq!(px(59, 99), &[255, 0, 0, 255]);
        assert_eq!(px(60, 50), &[0, 0, 0, 0]);
        assert_eq!(px(0, 49), &[0, 0, 0, 0]);
    }

    #[test]
    fn thumbnail_averages_premultiplied() {
        let mut s = Surface::new(8, 8);
        s.fill([200, 100, 50, 255]);
        let t = thumbnail(&s, 2, 2);
        assert_eq!(&t[0..4], &[200, 100, 50, 255]);
    }

    #[test]
    fn premultiply_rounds_and_skips_opaque() {
        let src = [255, 128, 0, 128, 10, 20, 30, 0];
        let mut dst = [0u8; 8];
        assert!(premultiply_tile(&src, &mut dst));
        assert_eq!(dst, [128, 64, 0, 128, 0, 0, 0, 0]);
        assert!(!premultiply_tile(&[1, 2, 3, 255], &mut [0; 4]));
    }
}
