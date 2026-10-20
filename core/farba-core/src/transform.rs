//! Whole-buffer geometry: flips, quarter turns and resampling (scaling) of dense pixel
//! buffers with any number of channels (4 = straight RGBA8, 1 = a selection mask).
//!
//! Resampling is separable and, unlike GIMP's Linear / Cubic, widens the filter when
//! downscaling (proper prefiltering), so a reduced image does not alias. RGBA is filtered in
//! premultiplied form so transparent pixels do not darken edges. Rows are resampled
//! horizontally once and kept in a small window, so memory stays near the output size.

/// Interpolation for scaling (GIMP's names where they match).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Nearest neighbour ("None"): hard pixels.
    None,
    Linear,
    /// Catmull-Rom cubic.
    Cubic,
    /// Lanczos-3: sharpest, slight ringing on hard edges.
    Lanczos,
}

impl Interpolation {
    pub fn from_index(i: u8) -> Self {
        match i {
            0 => Self::None,
            1 => Self::Linear,
            3 => Self::Lanczos,
            _ => Self::Cubic,
        }
    }

    /// Filter radius in source pixels at scale 1.
    fn support(self) -> f64 {
        match self {
            Self::None => 0.5,
            Self::Linear => 1.0,
            Self::Cubic => 2.0,
            Self::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        let x = x.abs();
        match self {
            Self::None => {
                if x < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Linear => (1.0 - x).max(0.0),
            Self::Cubic => {
                // Keys, a = -0.5 (Catmull-Rom).
                let a = -0.5;
                if x < 1.0 {
                    (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
                } else if x < 2.0 {
                    a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
                } else {
                    0.0
                }
            }
            Self::Lanczos => {
                if x < 1e-9 {
                    1.0
                } else if x < 3.0 {
                    let px = std::f64::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                } else {
                    0.0
                }
            }
        }
    }
}

/// Source indices and normalised weights for each output position along one axis.
struct Contributions {
    start: Vec<usize>,
    weights: Vec<Vec<f32>>,
}

fn contributions(src: usize, dst: usize, filter: Interpolation) -> Contributions {
    let scale = src as f64 / dst as f64;
    let mut start = Vec::with_capacity(dst);
    let mut weights = Vec::with_capacity(dst);
    if filter == Interpolation::None {
        for i in 0..dst {
            start.push((((i as f64 + 0.5) * scale) as usize).min(src - 1));
            weights.push(vec![1.0]);
        }
        return Contributions { start, weights };
    }
    // Downscaling widens the filter to cover all the source pixels of one output pixel.
    let stretch = scale.max(1.0);
    let support = filter.support() * stretch;
    for i in 0..dst {
        let centre = (i as f64 + 0.5) * scale - 0.5;
        let lo = (centre - support).floor() as isize;
        let hi = (centre + support).ceil() as isize;
        // Clamp to the edges: weights of outside taps go to the edge pixel.
        let first = lo.max(0) as usize;
        let last = (hi.min(src as isize - 1)) as usize;
        let mut w = vec![0f64; last - first + 1];
        for j in lo..=hi {
            let v = filter.weight((j as f64 - centre) / stretch);
            let k = (j.clamp(0, src as isize - 1) as usize) - first;
            w[k] += v;
        }
        let sum: f64 = w.iter().sum();
        let w: Vec<f32> = w
            .iter()
            .map(|v| (v / if sum.abs() < 1e-12 { 1.0 } else { sum }) as f32)
            .collect();
        start.push(first);
        weights.push(w);
    }
    Contributions { start, weights }
}

/// Scales a dense buffer of `ch` channels (1 or 4; 4 = straight RGBA) to `dw` x `dh`.
pub fn resample(
    src: &[u8],
    sw: usize,
    sh: usize,
    ch: usize,
    dw: usize,
    dh: usize,
    filter: Interpolation,
) -> Vec<u8> {
    assert!(ch == 1 || ch == 4);
    let mut out = vec![0u8; dw * dh * ch];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    let cx = contributions(sw, dw, filter);
    let cy = contributions(sh, dh, filter);
    let rgba = ch == 4;

    // Bands of output rows on all cores; each band keeps its own cache of horizontally
    // resampled source rows (premultiplied f32), so the result does not depend on the split.
    let band_rows = 64;
    crate::par::chunks_mut(&mut out, band_rows * dw * ch, |band, out| {
        let mut cache: std::collections::VecDeque<(usize, Vec<f32>)> = Default::default();
        let row_of = |y: usize, cache: &mut std::collections::VecDeque<(usize, Vec<f32>)>| {
            if let Some(i) = cache.iter().position(|(r, _)| *r == y) {
                return i;
            }
            let line = &src[y * sw * ch..(y + 1) * sw * ch];
            let mut h = vec![0f32; dw * ch];
            for x in 0..dw {
                let s = cx.start[x];
                let o = &mut h[x * ch..x * ch + ch];
                for (k, &w) in cx.weights[x].iter().enumerate() {
                    let p = &line[(s + k) * ch..(s + k) * ch + ch];
                    if rgba {
                        let a = p[3] as f32;
                        let wa = w * a / 255.0;
                        o[0] += p[0] as f32 * wa;
                        o[1] += p[1] as f32 * wa;
                        o[2] += p[2] as f32 * wa;
                        o[3] += w * a;
                    } else {
                        o[0] += w * p[0] as f32;
                    }
                }
            }
            cache.push_back((y, h));
            cache.len() - 1
        };

        let mut acc = vec![0f32; dw * ch];
        for (j, o) in out.chunks_exact_mut(dw * ch).enumerate() {
            let y = band * band_rows + j;
            let s = cy.start[y];
            // Rows above the window are no longer needed (windows only move down).
            while cache.front().is_some_and(|(r, _)| *r < s) {
                cache.pop_front();
            }
            acc.iter_mut().for_each(|v| *v = 0.0);
            for (k, &w) in cy.weights[y].iter().enumerate() {
                let i = row_of(s + k, &mut cache);
                for (a, &v) in acc.iter_mut().zip(&cache[i].1) {
                    *a += w * v;
                }
            }
            if rgba {
                for x in 0..dw {
                    let p = &acc[x * 4..x * 4 + 4];
                    let a = p[3].clamp(0.0, 255.0);
                    let q = &mut o[x * 4..x * 4 + 4];
                    if a < 0.5 {
                        q.copy_from_slice(&[0; 4]);
                        continue;
                    }
                    let inv = 255.0 / a;
                    for c in 0..3 {
                        q[c] = (p[c] * inv + 0.5).clamp(0.0, 255.0) as u8;
                    }
                    q[3] = (a + 0.5) as u8;
                }
            } else {
                for (q, &v) in o.iter_mut().zip(&acc) {
                    *q = (v + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
    });
    out
}

/// Mirrors a dense buffer left-right (`horizontal`) or top-bottom.
pub fn flip(src: &[u8], w: usize, h: usize, ch: usize, horizontal: bool) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    let row = w * ch;
    for y in 0..h {
        let s = &src[y * row..(y + 1) * row];
        if horizontal {
            let d = &mut out[y * row..(y + 1) * row];
            for x in 0..w {
                d[x * ch..x * ch + ch].copy_from_slice(&s[(w - 1 - x) * ch..(w - x) * ch]);
            }
        } else {
            out[(h - 1 - y) * row..(h - y) * row].copy_from_slice(s);
        }
    }
    out
}

/// Rotates a dense buffer by `turns` quarter turns clockwise. Returns the pixels and the
/// new width and height.
pub fn rotate(src: &[u8], w: usize, h: usize, ch: usize, turns: u32) -> (Vec<u8>, usize, usize) {
    match turns % 4 {
        0 => (src.to_vec(), w, h),
        2 => {
            let f = flip(src, w, h, ch, true);
            (flip(&f, w, h, ch, false), w, h)
        }
        t => {
            // Output is h x w. Clockwise: (x, y) -> (h - 1 - y, x).
            let mut out = vec![0u8; src.len()];
            for y in 0..h {
                for x in 0..w {
                    let (nx, ny) = if t == 1 {
                        (h - 1 - y, x)
                    } else {
                        (y, w - 1 - x)
                    };
                    let si = (y * w + x) * ch;
                    let di = (ny * h + nx) * ch;
                    out[di..di + ch].copy_from_slice(&src[si..si + ch]);
                }
            }
            (out, h, w)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flips_and_rotations() {
        // 3x2: 1 2 3 / 4 5 6
        let src = [1u8, 2, 3, 4, 5, 6];
        assert_eq!(flip(&src, 3, 2, 1, true), vec![3, 2, 1, 6, 5, 4]);
        assert_eq!(flip(&src, 3, 2, 1, false), vec![4, 5, 6, 1, 2, 3]);
        assert_eq!(rotate(&src, 3, 2, 1, 1), (vec![4, 1, 5, 2, 6, 3], 2, 3));
        assert_eq!(rotate(&src, 3, 2, 1, 3), (vec![3, 6, 2, 5, 1, 4], 2, 3));
        assert_eq!(rotate(&src, 3, 2, 1, 2).0, vec![6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn uniform_colour_stays_uniform() {
        let src: Vec<u8> = [10u8, 200, 30, 255].repeat(37 * 23);
        for f in [
            Interpolation::None,
            Interpolation::Linear,
            Interpolation::Cubic,
            Interpolation::Lanczos,
        ] {
            for (dw, dh) in [(11, 7), (100, 64), (37, 23)] {
                let out = resample(&src, 37, 23, 4, dw, dh, f);
                assert!(
                    out.chunks(4).all(|p| p == [10, 200, 30, 255]),
                    "{f:?} {dw}x{dh}"
                );
            }
        }
    }

    #[test]
    fn transparent_pixels_do_not_darken() {
        // Red next to transparent black: the scaled edge stays red, only alpha drops.
        let mut src = Vec::new();
        for x in 0..8 {
            src.extend_from_slice(if x < 4 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
        let out = resample(&src, 8, 1, 4, 3, 1, Interpolation::Linear);
        let mid = &out[4..8];
        assert_eq!(&mid[..3], &[255, 0, 0]);
        assert!(mid[3] > 60 && mid[3] < 200, "{mid:?}");
    }

    #[test]
    fn nearest_doubles_pixels() {
        let src = [0u8, 255];
        assert_eq!(
            resample(&src, 2, 1, 1, 4, 1, Interpolation::None),
            vec![0, 0, 255, 255]
        );
    }

    #[test]
    fn downscale_averages() {
        // A 1 px checker of 0 / 255 reduced 4x becomes mid grey, not aliasing.
        let src: Vec<u8> = (0..64 * 64)
            .map(|i| if (i % 64 + i / 64) % 2 == 0 { 0 } else { 255 })
            .collect();
        let out = resample(&src, 64, 64, 1, 16, 16, Interpolation::Lanczos);
        assert!(
            out.iter().all(|&v| (120..=135).contains(&v)),
            "{:?}",
            &out[..16]
        );
    }
}
