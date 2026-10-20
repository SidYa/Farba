//! Effects (the Effects menu): Black & White, Enhance Colors, Retouch (skin smoothing),
//! Reduce Noise, Sharpen, Gaussian Blur and Vignette. Unlike the per-pixel adjustments in
//! `filters`, most of these look at neighbouring pixels, so they run on dense pixels in
//! horizontal strips (with enough rows above and below), which keeps memory near the strip
//! size even for very large layers. The result is blended with the original by the
//! selection's coverage; alpha is kept.
//!
//! Smoothing (Retouch, Reduce Noise) is the guided filter (He et al.) with the image as its
//! own guide: edge-preserving, and its cost does not depend on the radius.

use crate::selection::Mask;
use crate::surface::{Surface, TILE_SIZE};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Channel weights in percent (gray = (r*R + g*G + b*B) / 100), contrast -100..100,
    /// sepia toning 0..100.
    BlackWhite {
        red: f32,
        green: f32,
        blue: f32,
        contrast: f32,
        sepia: f32,
    },
    /// Auto levels strength 0..100 (per-channel stretch, removes a colour cast) and vibrance
    /// 0..100 (more saturation for dull colours).
    EnhanceColors { auto: f32, vibrance: f32 },
    /// Edge-preserving smoothing: radius px, smoothness 1..100, amount 0..100.
    Retouch {
        radius: f32,
        smoothness: f32,
        amount: f32,
    },
    /// Light edge-preserving smoothing: radius px, strength 1..100.
    ReduceNoise { radius: f32, strength: f32 },
    /// Unsharp mask: radius px, amount percent, threshold 0..255.
    Sharpen {
        radius: f32,
        amount: f32,
        threshold: f32,
    },
    /// Standard deviation in pixels.
    GaussianBlur { sigma: f32 },
    /// Amount -100..100 (negative darkens the edges), size 0..100, softness 0..100.
    Vignette {
        amount: f32,
        size: f32,
        softness: f32,
    },
}

/// Rows per strip (several tiles).
const STRIP: usize = 128;

impl Effect {
    /// Rows above / below a strip the effect needs.
    fn halo(&self) -> usize {
        match *self {
            Effect::Retouch { radius, .. } | Effect::ReduceNoise { radius, .. } => {
                2 * radius.max(1.0).round() as usize + 1
            }
            Effect::Sharpen { radius, .. } => blur_halo(radius.max(0.1) / 2.0),
            Effect::GaussianBlur { sigma } => blur_halo(sigma),
            _ => 0,
        }
    }
}

fn blur_halo(sigma: f32) -> usize {
    box_sizes(sigma).iter().map(|b| b / 2).sum::<usize>() + 1
}

/// Odd box widths whose three passes approximate a Gaussian of `sigma`.
fn box_sizes(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as usize;
    if wl.is_multiple_of(2) {
        wl = wl.saturating_sub(1);
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f32;
    let m = ((12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0))
        .round() as i32;
    std::array::from_fn(|i| if (i as i32) < m { wl } else { wu })
}

/// Box blur (radius `r`) of one f32 plane `w` x `h`, horizontally then vertically, with
/// edge clamping.
fn box_blur(src: &[f32], dst: &mut [f32], tmp: &mut [f32], w: usize, h: usize, r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        let out = &mut tmp[y * w..y * w + w];
        let at = |i: isize| row[i.clamp(0, w as isize - 1) as usize];
        let mut sum: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for (x, o) in out.iter_mut().enumerate() {
            *o = sum * norm;
            sum += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    }
    let mut sum = vec![0f32; w];
    let row = |y: isize| (y.clamp(0, h as isize - 1) as usize) * w;
    for k in -(r as isize)..=r as isize {
        let o = row(k);
        for x in 0..w {
            sum[x] += tmp[o + x];
        }
    }
    for y in 0..h {
        let d = y * w;
        for x in 0..w {
            dst[d + x] = sum[x] * norm;
        }
        let (add, sub) = (
            row(y as isize + r as isize + 1),
            row(y as isize - r as isize),
        );
        for x in 0..w {
            sum[x] += tmp[add + x] - tmp[sub + x];
        }
    }
}

/// Gaussian-like blur of one plane (three box blurs).
fn gauss(plane: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let mut a = vec![0f32; plane.len()];
    let mut t = vec![0f32; plane.len()];
    for b in box_sizes(sigma) {
        box_blur(plane, &mut a, &mut t, w, h, b / 2);
        plane.copy_from_slice(&a);
    }
}

/// Self-guided filter of one plane (values 0..1): edges stay, flat areas are smoothed.
fn guided(p: &mut [f32], w: usize, h: usize, r: usize, eps: f32) {
    let n = p.len();
    let mut mean = vec![0f32; n];
    let mut tmp = vec![0f32; n];
    let mut sq: Vec<f32> = p.iter().map(|v| v * v).collect();
    let mut mean_sq = vec![0f32; n];
    box_blur(p, &mut mean, &mut tmp, w, h, r);
    box_blur(&sq, &mut mean_sq, &mut tmp, w, h, r);
    // a = var / (var + eps), b = (1 - a) * mean; then average a and b over the window.
    for i in 0..n {
        let var = (mean_sq[i] - mean[i] * mean[i]).max(0.0);
        let a = var / (var + eps);
        sq[i] = a;
        mean_sq[i] = (1.0 - a) * mean[i];
    }
    box_blur(&sq, &mut mean, &mut tmp, w, h, r);
    box_blur(&mean_sq, &mut sq, &mut tmp, w, h, r);
    for i in 0..n {
        p[i] = mean[i] * p[i] + sq[i];
    }
}

/// Per-channel stretch tables for auto levels (0.1% / 99.9% of the opaque pixels).
fn auto_levels(surface: &Surface) -> [[u8; 256]; 3] {
    let mut hist = [[0u64; 256]; 3];
    let mut count = 0u64;
    for ty in 0..surface.tiles_y() {
        for tx in 0..surface.tiles_x() {
            if !surface.is_allocated(tx, ty) {
                continue;
            }
            let (_, _, tw, th) = surface.tile_rect(tx, ty);
            let t = surface.tile(tx, ty);
            for y in 0..th as usize {
                for x in 0..tw as usize {
                    let i = (y * TILE_SIZE as usize + x) * 4;
                    if t[i + 3] < 128 {
                        continue;
                    }
                    for c in 0..3 {
                        hist[c][t[i + c] as usize] += 1;
                    }
                    count += 1;
                }
            }
        }
    }
    std::array::from_fn(|c| {
        if count == 0 {
            return std::array::from_fn(|v| v as u8);
        }
        let cut = count / 1000;
        let mut acc = 0;
        let lo = (0..256)
            .find(|&v| {
                acc += hist[c][v];
                acc > cut
            })
            .unwrap_or(0);
        acc = 0;
        let hi = (0..256)
            .rev()
            .find(|&v| {
                acc += hist[c][v];
                acc > cut
            })
            .unwrap_or(255);
        let (lo, hi) = if hi > lo {
            (lo as f32, hi as f32)
        } else {
            (0.0, 255.0)
        };
        std::array::from_fn(|v| ((v as f32 - lo) / (hi - lo) * 255.0 + 0.5).clamp(0.0, 255.0) as u8)
    })
}

/// Applies `effect` to `surface` (at `pos` in an image of size `image`), limited to `mask`.
pub fn apply(
    surface: &mut Surface,
    pos: (i32, i32),
    image: (u32, u32),
    mask: Option<&Mask>,
    effect: Effect,
) {
    let (w, h) = (surface.width() as usize, surface.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let levels = matches!(effect, Effect::EnhanceColors { .. }).then(|| auto_levels(surface));
    let halo = effect.halo();
    // Every strip reads the original pixels and covers whole tile rows, so strips are
    // computed on all cores (at most 16 at once: each needs float planes of its rows) and
    // their changed tiles are copied in afterwards.
    const _: () = assert!(STRIP.is_multiple_of(TILE_SIZE as usize));
    let original = surface.clone();
    let strips = h.div_ceil(STRIP);
    let tiles = crate::par::map(strips, 16, |k| {
        let y0 = k * STRIP;
        let y1 = (y0 + STRIP).min(h);
        let top = y0.saturating_sub(halo);
        let bottom = (y1 + halo).min(h);
        let src = original.read_rect(0, top as i64, w as u32, (bottom - top) as u32);
        let out = strip(
            &src,
            w,
            bottom - top,
            y0 - top,
            y1 - y0,
            y0,
            pos,
            image,
            effect,
            levels.as_ref(),
        );
        let orig = &src[(y0 - top) * w * 4..(y1 - top) * w * 4];
        changed_tiles(&original, pos, mask, y0, w, orig, &out)
    });
    for (tx, ty, tile) in tiles.into_iter().flatten() {
        surface.tile_mut(tx, ty).copy_from_slice(&tile);
    }
}

/// Computes the effect for the rows `[skip, skip + rows)` of a strip of `sh` rows; `row0`
/// is the layer row of the first output row.
#[allow(clippy::too_many_arguments)]
fn strip(
    src: &[u8],
    w: usize,
    sh: usize,
    skip: usize,
    rows: usize,
    row0: usize,
    pos: (i32, i32),
    image: (u32, u32),
    effect: Effect,
    levels: Option<&[[u8; 256]; 3]>,
) -> Vec<u8> {
    let px = &src[skip * w * 4..(skip + rows) * w * 4];
    let mut out = px.to_vec();
    match effect {
        Effect::BlackWhite {
            red,
            green,
            blue,
            contrast,
            sepia,
        } => {
            let k = 1.0 + contrast / 100.0;
            let s = sepia / 100.0;
            for (o, p) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(px.as_chunks::<4>().0)
            {
                let g = (p[0] as f32 * red + p[1] as f32 * green + p[2] as f32 * blue) / 100.0;
                let g = ((g - 128.0) * k + 128.0).clamp(0.0, 255.0);
                let tone = [g * 1.07, g * 0.74, g * 0.43];
                for c in 0..3 {
                    o[c] = (g + (tone[c].min(255.0) - g) * s + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
        Effect::EnhanceColors { auto, vibrance } => {
            let (a, v) = (auto / 100.0, vibrance / 100.0);
            for (o, p) in out
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(px.as_chunks::<4>().0)
            {
                let mut c = [p[0] as f32, p[1] as f32, p[2] as f32];
                if let Some(l) = levels {
                    for k in 0..3 {
                        c[k] += (l[k][p[k] as usize] as f32 - c[k]) * a;
                    }
                }
                let max = c[0].max(c[1]).max(c[2]);
                let min = c[0].min(c[1]).min(c[2]);
                let avg = (c[0] + c[1] + c[2]) / 3.0;
                // Dull colours gain the most; saturated ones stay.
                let sat = (max - min) / 255.0;
                let boost = 1.0 + v * (1.0 - sat) * (1.0 - sat);
                for k in 0..3 {
                    o[k] = (avg + (c[k] - avg) * boost + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
        Effect::Retouch {
            radius,
            smoothness,
            amount,
        } => {
            let e = 0.01 + smoothness / 100.0 * 0.19;
            smooth(
                src,
                w,
                sh,
                skip,
                rows,
                &mut out,
                radius,
                e * e,
                amount / 100.0,
            );
        }
        Effect::ReduceNoise { radius, strength } => {
            let e = strength / 100.0 * 0.12;
            smooth(src, w, sh, skip, rows, &mut out, radius, e * e, 1.0);
        }
        Effect::GaussianBlur { sigma } => {
            let planes = blurred(src, w, sh, sigma);
            write_premultiplied(&planes, w, skip, rows, &mut out);
        }
        Effect::Sharpen {
            radius,
            amount,
            threshold,
        } => {
            let planes = blurred(src, w, sh, radius.max(0.1) / 2.0);
            let k = amount / 100.0;
            for i in 0..rows * w {
                let j = skip * w + i;
                let a = planes[3][j];
                for c in 0..3 {
                    let orig = px[i * 4 + c] as f32;
                    let blur = if a > 1e-3 {
                        planes[c][j] / a * 255.0
                    } else {
                        orig
                    };
                    let d = orig - blur;
                    if d.abs() >= threshold {
                        out[i * 4 + c] = (orig + d * k + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        Effect::Vignette {
            amount,
            size,
            softness,
        } => {
            let (iw, ih) = (image.0.max(1) as f32, image.1.max(1) as f32);
            let start = 0.2 + size / 100.0 * 1.0;
            let end = start + 0.05 + softness / 100.0 * 1.0;
            let a = amount / 100.0;
            for r in 0..rows {
                let iy = (pos.1 as f32 + (row0 + r) as f32 + 0.5 - ih / 2.0) / (ih / 2.0);
                for x in 0..w {
                    let ix = (pos.0 as f32 + x as f32 + 0.5 - iw / 2.0) / (iw / 2.0);
                    let d = (ix * ix + iy * iy).sqrt() / std::f32::consts::SQRT_2 * 1.4;
                    let t = ((d - start) / (end - start)).clamp(0.0, 1.0);
                    let f = t * t * (3.0 - 2.0 * t);
                    let i = (r * w + x) * 4;
                    for c in 0..3 {
                        let v = px[i + c] as f32;
                        let nv = if a < 0.0 {
                            v * (1.0 + a * f)
                        } else {
                            v + (255.0 - v) * a * f
                        };
                        out[i + c] = (nv + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
    out
}

/// The strip's colour planes, premultiplied (0..1) and blurred; plane 3 is alpha.
fn blurred(src: &[u8], w: usize, sh: usize, sigma: f32) -> [Vec<f32>; 4] {
    let n = w * sh;
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0f32; n]);
    for (i, p) in src.as_chunks::<4>().0.iter().enumerate() {
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            planes[c][i] = p[c] as f32 / 255.0 * a;
        }
        planes[3][i] = a;
    }
    for plane in &mut planes {
        gauss(plane, w, sh, sigma);
    }
    planes
}

/// Writes blurred premultiplied planes back as straight RGBA for the output rows.
fn write_premultiplied(planes: &[Vec<f32>; 4], w: usize, skip: usize, rows: usize, out: &mut [u8]) {
    for i in 0..rows * w {
        let j = skip * w + i;
        let a = planes[3][j];
        if a <= 1e-4 {
            out[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for c in 0..3 {
            out[i * 4 + c] = (planes[c][j] / a * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
        out[i * 4 + 3] = (a * 255.0 + 0.5) as u8;
    }
}

/// Guided-filter smoothing of the colour channels, blended in by `amount`.
#[allow(clippy::too_many_arguments)]
fn smooth(
    src: &[u8],
    w: usize,
    sh: usize,
    skip: usize,
    rows: usize,
    out: &mut [u8],
    radius: f32,
    eps: f32,
    amount: f32,
) {
    let r = radius.max(1.0).round() as usize;
    let mut plane = vec![0f32; w * sh];
    for c in 0..3 {
        for (i, p) in src.as_chunks::<4>().0.iter().enumerate() {
            plane[i] = p[c] as f32 / 255.0;
        }
        guided(&mut plane, w, sh, r, eps);
        for i in 0..rows * w {
            let orig = out[i * 4 + c] as f32;
            let v = plane[skip * w + i] * 255.0;
            out[i * 4 + c] = (orig + (v - orig) * amount + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}

/// The tiles changed by effect output rows (straight RGBA, `w` wide, starting at layer row
/// `y0`, whole tile rows), blended with the original rows by the selection; alpha of the effect is used
/// only by the blurs (they may spread pixels).
#[allow(clippy::too_many_arguments)]
fn changed_tiles(
    surface: &Surface,
    pos: (i32, i32),
    mask: Option<&Mask>,
    y0: usize,
    w: usize,
    orig: &[u8],
    out: &[u8],
) -> Vec<(u32, u32, Box<[u8]>)> {
    let rows = out.len() / (w * 4);
    let t = TILE_SIZE as usize;
    let mut changed = Vec::new();
    for ty in (y0 / t)..(y0 + rows).div_ceil(t) {
        for tx in 0..surface.tiles_x() as usize {
            let (x0, ty0, tw, th) = surface.tile_rect(tx as u32, ty as u32);
            let mut tile: Option<Box<[u8]>> = None;
            for yy in 0..th as usize {
                let ly = ty0 as usize + yy;
                if ly < y0 || ly >= y0 + rows {
                    continue;
                }
                for xx in 0..tw as usize {
                    let lx = x0 as usize + xx;
                    let i = ((ly - y0) * w + lx) * 4;
                    let p = &orig[i..i + 4];
                    let mut q = [out[i], out[i + 1], out[i + 2], out[i + 3]];
                    if let Some(m) = mask {
                        let k = m.get(pos.0 as i64 + lx as i64, pos.1 as i64 + ly as i64) as u32;
                        for c in 0..4 {
                            q[c] = ((p[c] as u32 * (255 - k) + q[c] as u32 * k + 127) / 255) as u8;
                        }
                    }
                    if q != [p[0], p[1], p[2], p[3]] {
                        let buf = tile.get_or_insert_with(|| {
                            surface
                                .tile(tx as u32, ty as u32)
                                .to_vec()
                                .into_boxed_slice()
                        });
                        let o = (yy * t + xx) * 4;
                        buf[o..o + 4].copy_from_slice(&q);
                    }
                }
            }
            if let Some(buf) = tile {
                changed.push((tx as u32, ty as u32, buf));
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Surface {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&f(x, y));
            }
        }
        Surface::from_rgba(w, h, &v)
    }

    fn px(s: &Surface, x: i64, y: i64) -> [u8; 4] {
        s.pixel(x, y).unwrap()
    }

    #[test]
    fn uniform_stays_uniform() {
        for e in [
            Effect::GaussianBlur { sigma: 6.0 },
            Effect::Sharpen {
                radius: 3.0,
                amount: 150.0,
                threshold: 0.0,
            },
            Effect::Retouch {
                radius: 8.0,
                smoothness: 50.0,
                amount: 100.0,
            },
            Effect::ReduceNoise {
                radius: 2.0,
                strength: 40.0,
            },
        ] {
            let mut s = surface(70, 300, |_, _| [100, 150, 200, 255]);
            apply(&mut s, (0, 0), (70, 300), None, e);
            for (x, y) in [(0, 0), (35, 255), (35, 256), (69, 299)] {
                let p = px(&s, x, y);
                assert!(
                    p.iter()
                        .zip([100, 150, 200, 255])
                        .all(|(a, b)| a.abs_diff(b) <= 1),
                    "{e:?} {p:?}"
                );
            }
        }
    }

    #[test]
    fn blur_has_no_seam_between_strips() {
        // A horizontal edge exactly at a strip boundary blurs symmetrically.
        let mut s = surface(10, 512, |_, y| {
            if y < 256 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        apply(
            &mut s,
            (0, 0),
            (10, 512),
            None,
            Effect::GaussianBlur { sigma: 4.0 },
        );
        let a = px(&s, 5, 253)[0] as i32;
        let b = px(&s, 5, 258)[0] as i32;
        assert!((a + b - 255).abs() <= 2, "{a} {b}");
        assert!(a > 0 && b < 255);
    }

    #[test]
    fn black_white_vignette_and_enhance() {
        let mut s = surface(4, 4, |_, _| [255, 0, 0, 255]);
        let bw = Effect::BlackWhite {
            red: 30.0,
            green: 59.0,
            blue: 11.0,
            contrast: 0.0,
            sepia: 0.0,
        };
        apply(&mut s, (0, 0), (4, 4), None, bw);
        assert_eq!(px(&s, 1, 1), [77, 77, 77, 255]);

        let mut s = surface(100, 100, |_, _| [200, 200, 200, 255]);
        let v = Effect::Vignette {
            amount: -100.0,
            size: 20.0,
            softness: 30.0,
        };
        apply(&mut s, (0, 0), (100, 100), None, v);
        assert_eq!(px(&s, 50, 50), [200, 200, 200, 255]);
        assert!(px(&s, 0, 0)[0] < 60);

        // A dull, low-contrast image gets stretched to the full range.
        let mut s = surface(10, 10, |x, _| {
            let v = 100 + (x as u8) * 5;
            [v, v, v, 255]
        });
        let e = Effect::EnhanceColors {
            auto: 100.0,
            vibrance: 0.0,
        };
        apply(&mut s, (0, 0), (10, 10), None, e);
        assert!(px(&s, 0, 0)[0] < 10 && px(&s, 9, 0)[0] > 245);
    }

    #[test]
    fn retouch_keeps_edges_and_smooths_noise() {
        // Left half dark, right half light, with fine noise everywhere.
        let noise = |x: u32, y: u32| ((x * 7 + y * 13) % 5) as u8 * 6;
        let mut s = surface(80, 40, |x, y| {
            let base = if x < 40 { 50 } else { 200 };
            let v = base + noise(x, y);
            [v, v, v, 255]
        });
        let e = Effect::Retouch {
            radius: 4.0,
            smoothness: 30.0,
            amount: 100.0,
        };
        apply(&mut s, (0, 0), (80, 40), None, e);
        // The edge stays sharp.
        assert!(px(&s, 37, 20)[0] < 90 && px(&s, 42, 20)[0] > 170);
        // The noise is mostly gone.
        let row: Vec<u8> = (5..30).map(|x| px(&s, x, 20)[0]).collect();
        let spread = row.iter().max().unwrap() - row.iter().min().unwrap();
        assert!(spread <= 8, "{row:?}");
    }
}
