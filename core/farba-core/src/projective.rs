//! Projective transforms (3x3 homographies) and rendering a buffer through one.
//!
//! One matrix type covers every transform tool: rotate, scale and flip are affine, 3D
//! Transform and Handle Transform (4 handles) are projective. A matrix maps a column vector
//! `(x, y, 1)`; pixel `(i, j)` covers `[i, i + 1) x [j, j + 1)`, so its centre is `+ 0.5`.
//!
//! Rendering maps every output pixel back into the source. When the source is shrunk
//! locally (the inverse map spreads one output pixel over several source pixels), the
//! pixel is the average of several samples (up to 8 x 8), so rotated or reduced images do
//! not alias. Pixels outside the source are transparent, which gives antialiased edges.

use crate::transform::Interpolation;

/// Row-major 3x3 matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f64; 9]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);

    pub fn translate(x: f64, y: f64) -> Self {
        Matrix([1.0, 0.0, x, 0.0, 1.0, y, 0.0, 0.0, 1.0])
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Matrix([sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 1.0])
    }

    /// Rotation by `angle` radians (clockwise on screen, y down) around `(cx, cy)`.
    pub fn rotate_around(angle: f64, cx: f64, cy: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Matrix::translate(cx, cy)
            .mul(&Matrix([c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]))
            .mul(&Matrix::translate(-cx, -cy))
    }

    /// `self * other` (apply `other` first).
    pub fn mul(&self, o: &Matrix) -> Matrix {
        let a = &self.0;
        let b = &o.0;
        let mut r = [0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                r[i * 3 + j] = (0..3).map(|k| a[i * 3 + k] * b[k * 3 + j]).sum();
            }
        }
        Matrix(r)
    }

    pub fn invert(&self) -> Option<Matrix> {
        let m = &self.0;
        let c00 = m[4] * m[8] - m[5] * m[7];
        let c01 = m[5] * m[6] - m[3] * m[8];
        let c02 = m[3] * m[7] - m[4] * m[6];
        let det = m[0] * c00 + m[1] * c01 + m[2] * c02;
        if det.abs() < 1e-12 {
            return None;
        }
        let d = 1.0 / det;
        Some(Matrix([
            c00 * d,
            (m[2] * m[7] - m[1] * m[8]) * d,
            (m[1] * m[5] - m[2] * m[4]) * d,
            c01 * d,
            (m[0] * m[8] - m[2] * m[6]) * d,
            (m[2] * m[3] - m[0] * m[5]) * d,
            c02 * d,
            (m[1] * m[6] - m[0] * m[7]) * d,
            (m[0] * m[4] - m[1] * m[3]) * d,
        ]))
    }

    /// The homogeneous image `(x, y, w)` of a point.
    #[inline]
    pub fn apply_h(&self, x: f64, y: f64) -> (f64, f64, f64) {
        let m = &self.0;
        (
            m[0] * x + m[1] * y + m[2],
            m[3] * x + m[4] * y + m[5],
            m[6] * x + m[7] * y + m[8],
        )
    }

    /// The image of a point, or `None` behind the "camera" (w <= 0).
    #[inline]
    pub fn apply(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (px, py, w) = self.apply_h(x, y);
        (w > 1e-9).then(|| (px / w, py / w))
    }

    pub fn is_affine(&self) -> bool {
        let m = &self.0;
        m[6].abs() < 1e-12 && m[7].abs() < 1e-12 && (m[8] - 1.0).abs() < 1e-12
    }

    /// True when the matrix maps the pixel grid onto itself exactly (flips, quarter turns,
    /// integer moves), so nearest-neighbour sampling copies pixels without any change.
    pub fn is_grid_exact(&self) -> bool {
        let m = &self.0;
        let unit = |v: f64| [-1.0, 0.0, 1.0].iter().any(|u| (v - u).abs() < 1e-9);
        let int = |v: f64| (v - v.round()).abs() < 1e-9;
        self.is_affine()
            && unit(m[0])
            && unit(m[1])
            && unit(m[3])
            && unit(m[4])
            && (m[0] * m[4] - m[1] * m[3]).abs() > 0.5
            && int(m[2])
            && int(m[5])
    }

    /// The transform that moves points `from[i]` to `to[i]` (1 to 4 pairs): a move, a
    /// similarity (move, rotate, uniform scale), an affine or a projective map.
    pub fn from_points(from: &[(f64, f64)], to: &[(f64, f64)]) -> Option<Matrix> {
        match from.len().min(to.len()) {
            0 => Some(Matrix::IDENTITY),
            1 => Some(Matrix::translate(to[0].0 - from[0].0, to[0].1 - from[0].1)),
            2 => {
                // Similarity: treat points as complex numbers, z' = a z + b.
                let (fx, fy) = (from[1].0 - from[0].0, from[1].1 - from[0].1);
                let (tx, ty) = (to[1].0 - to[0].0, to[1].1 - to[0].1);
                let d = fx * fx + fy * fy;
                if d < 1e-12 {
                    return None;
                }
                let a = (tx * fx + ty * fy) / d;
                let b = (ty * fx - tx * fy) / d;
                let m = Matrix([a, -b, 0.0, b, a, 0.0, 0.0, 0.0, 1.0]);
                let (px, py, _) = m.apply_h(from[0].0, from[0].1);
                Some(Matrix::translate(to[0].0 - px, to[0].1 - py).mul(&m))
            }
            3 => {
                // Affine: solve for the 6 coefficients.
                let mut a = [[0.0; 7]; 6];
                for i in 0..3 {
                    let (x, y) = from[i];
                    a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, to[i].0];
                    a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, to[i].1];
                }
                let s = solve(&mut a.iter().map(|r| r.to_vec()).collect::<Vec<_>>())?;
                Some(Matrix([s[0], s[1], s[2], s[3], s[4], s[5], 0.0, 0.0, 1.0]))
            }
            _ => {
                // Projective: 8 unknowns (h22 = 1).
                let mut rows = Vec::with_capacity(8);
                for i in 0..4 {
                    let (x, y) = from[i];
                    let (u, v) = to[i];
                    rows.push(vec![x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u]);
                    rows.push(vec![0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v]);
                }
                let s = solve(&mut rows)?;
                Some(Matrix([
                    s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], 1.0,
                ]))
            }
        }
    }
}

/// Gaussian elimination with partial pivoting on an augmented matrix (n rows, n + 1 cols).
fn solve(a: &mut [Vec<f64>]) -> Option<Vec<f64>> {
    let n = a.len();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        for r in 0..n {
            if r != c {
                let f = a[r][c] / a[c][c];
                let pivot = a[c].clone();
                for (v, p) in a[r][c..=n].iter_mut().zip(&pivot[c..=n]) {
                    *v -= f * p;
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

/// The output of `render`: pixels and where they go.
pub struct Rendered {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

/// Largest side of a rendered result; bigger transforms are refused.
pub const MAX_SIDE: u32 = 32768;
/// Largest area of a rendered result (pixels).
pub const MAX_AREA: u64 = 400_000_000;

#[inline]
fn kernel(interp: Interpolation, x: f64) -> f64 {
    let x = x.abs();
    match interp {
        Interpolation::Cubic => {
            let a = -0.5;
            if x < 1.0 {
                (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
            } else if x < 2.0 {
                a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
            } else {
                0.0
            }
        }
        Interpolation::Lanczos => {
            if x < 1e-9 {
                1.0
            } else if x < 3.0 {
                let px = std::f64::consts::PI * x;
                3.0 * px.sin() * (px / 3.0).sin() / (px * px)
            } else {
                0.0
            }
        }
        _ => (1.0 - x).max(0.0),
    }
}

/// Lanczos-3 kernel sampled on [0, 3) in 1/1024 steps (sin is too slow per tap).
static LANCZOS: std::sync::LazyLock<Vec<f64>> = std::sync::LazyLock::new(|| {
    (0..3 * 1024 + 2)
        .map(|i| kernel(Interpolation::Lanczos, i as f64 / 1024.0))
        .collect()
});

/// Tap weights for a sample at fraction `t` (0..1) past the pixel at offset 0; taps are at
/// offsets `1 - r ..= r` (Linear r = 1, Cubic r = 2, Lanczos r = 3).
#[inline]
fn weights(interp: Interpolation, t: f64) -> [f64; 6] {
    match interp {
        Interpolation::Linear => [1.0 - t, t, 0.0, 0.0, 0.0, 0.0],
        Interpolation::Cubic => {
            // Catmull-Rom (Keys, a = -0.5) in closed form.
            let (t2, t3) = (t * t, t * t * t);
            [
                -0.5 * t3 + t2 - 0.5 * t,
                1.5 * t3 - 2.5 * t2 + 1.0,
                -1.5 * t3 + 2.0 * t2 + 0.5 * t,
                0.5 * t3 - 0.5 * t2,
                0.0,
                0.0,
            ]
        }
        _ => {
            let lut = &*LANCZOS;
            let at = |x: f64| {
                let f = x.abs() * 1024.0;
                let i = f as usize;
                let k = f - i as f64;
                lut[i] + (lut[i + 1] - lut[i]) * k
            };
            std::array::from_fn(|k| at(t - (k as f64 - 2.0)))
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Samples a dense buffer (`ch` = 4 straight RGBA, or 1) at continuous source coordinates
/// (pixel centres at `.5`), adding the premultiplied result to `acc`. Outside is transparent.
#[inline]
fn sample(
    src: &[u8],
    sw: usize,
    sh: usize,
    ch: usize,
    u: f64,
    v: f64,
    interp: Interpolation,
    acc: &mut [f64; 4],
) {
    let px = |x: isize, y: isize| -> Option<&[u8]> {
        (x >= 0 && y >= 0 && (x as usize) < sw && (y as usize) < sh).then(|| {
            let i = (y as usize * sw + x as usize) * ch;
            &src[i..i + ch]
        })
    };
    let add = |acc: &mut [f64; 4], p: &[u8], w: f64| {
        if ch == 4 {
            let a = p[3] as f64 * w;
            acc[0] += p[0] as f64 * a;
            acc[1] += p[1] as f64 * a;
            acc[2] += p[2] as f64 * a;
            acc[3] += a;
        } else {
            acc[0] += p[0] as f64 * w;
        }
    };
    if interp == Interpolation::None {
        if let Some(p) = px(u.floor() as isize, v.floor() as isize) {
            add(acc, p, 1.0);
        }
        return;
    }
    let r: isize = match interp {
        Interpolation::Linear => 1,
        Interpolation::Cubic => 2,
        _ => 3,
    };
    // Continuous coordinate of the centre of pixel i is i + 0.5.
    let (fu, fv) = (u - 0.5, v - 0.5);
    let (iu, iv) = (fu.floor() as isize, fv.floor() as isize);
    let wx = weights(interp, fu - iu as f64);
    let wy = weights(interp, fv - iv as f64);
    let n = (2 * r) as usize;
    let (left, top) = (iu - r + 1, iv - r + 1);
    if ch == 4 && left >= 0 && top >= 0 && left as usize + n <= sw && top as usize + n <= sh {
        // Fast path: the whole footprint is inside the source (no bounds checks per tap).
        let mut a = [0f64; 4];
        for (ky, &wyk) in wy.iter().enumerate().take(n) {
            let row = ((top as usize + ky) * sw + left as usize) * 4;
            let line = &src[row..row + n * 4];
            for (p, &wxk) in line.as_chunks::<4>().0.iter().zip(&wx[..n]) {
                let w = wxk * wyk * p[3] as f64;
                a[0] += p[0] as f64 * w;
                a[1] += p[1] as f64 * w;
                a[2] += p[2] as f64 * w;
                a[3] += w;
            }
        }
        for c in 0..4 {
            acc[c] += a[c];
        }
        return;
    }
    for (ky, &wyk) in wy.iter().enumerate().take(n) {
        if wyk == 0.0 {
            continue;
        }
        let y = iv + ky as isize - r + 1;
        for (kx, &wxk) in wx.iter().enumerate().take(n) {
            if let Some(p) = px(iu + kx as isize - r + 1, y) {
                add(acc, p, wxk * wyk);
            }
        }
    }
}

/// Renders a dense `sw` x `sh` buffer of `ch` channels through `m` (source pixels ->
/// output pixels). Returns the covering rectangle of the result, or `None` when the
/// transform is degenerate, goes behind the camera or is too large.
pub fn render(
    src: &[u8],
    sw: usize,
    sh: usize,
    ch: usize,
    m: &Matrix,
    interp: Interpolation,
) -> Option<Rendered> {
    let inv = m.invert()?;
    let corners = [
        (0.0, 0.0),
        (sw as f64, 0.0),
        (sw as f64, sh as f64),
        (0.0, sh as f64),
    ];
    let pts: Vec<(f64, f64)> = corners
        .iter()
        .map(|&(x, y)| m.apply(x, y))
        .collect::<Option<_>>()?;
    let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min).floor();
    let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min).floor();
    let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max).ceil();
    let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max).ceil();
    let (w, h) = ((x1 - x0) as u64, (y1 - y0) as u64);
    if w == 0 || h == 0 || w > MAX_SIDE as u64 || h > MAX_SIDE as u64 || w * h > MAX_AREA {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w * h * ch];
    let affine = inv.is_affine();
    // Samples per axis from how far one output pixel spreads in the source.
    let spread = |x: f64, y: f64| -> usize {
        if interp == Interpolation::None {
            return 1;
        }
        let (Some(a), Some(b), Some(c)) = (
            inv.apply(x, y),
            inv.apply(x + 1.0, y),
            inv.apply(x, y + 1.0),
        ) else {
            return 1;
        };
        let du = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let dv = ((c.0 - a.0).powi(2) + (c.1 - a.1).powi(2)).sqrt();
        // The small margin keeps a pure rotation (spread exactly 1) at one sample.
        ((du.max(dv) - 1e-3).ceil() as usize).clamp(1, 8)
    };
    let fixed_n = affine.then(|| spread(0.0, 0.0));
    let im = &inv.0;
    // Bands of output rows on all cores (every row is independent).
    let band_rows = 16;
    crate::par::chunks_mut(&mut out, band_rows * w * ch, |band, out| {
        for (j, out) in out.chunks_exact_mut(w * ch).enumerate() {
            let oy = band * band_rows + j;
            let cy = y0 + oy as f64;
            // Affine maps: the pixels whose centres land near the source form one run per row,
            // found directly (u and v are linear in x), so the rest of the row costs nothing.
            let (mut first, mut last) = (0usize, w);
            if let Some(n) = fixed_n {
                let margin = 3.0 + n as f64;
                let (mut lo, mut hi) = (0f64, w as f64);
                // u(x) = u0 + x * du for pixel centre x + 0.5.
                for (k, limit) in [(0usize, sw as f64), (3, sh as f64)] {
                    let d = im[k];
                    let base = im[k] * (x0 + 0.5) + im[k + 1] * (cy + 0.5) + im[k + 2];
                    let (a, b) = (-margin, limit + margin);
                    if d.abs() < 1e-12 {
                        if base < a || base > b {
                            hi = lo;
                        }
                    } else {
                        let (t0, t1) = ((a - base) / d, (b - base) / d);
                        lo = lo.max(t0.min(t1).floor());
                        hi = hi.min(t0.max(t1).ceil() + 1.0);
                    }
                }
                if hi <= lo {
                    continue;
                }
                first = lo.max(0.0) as usize;
                last = (hi.min(w as f64)) as usize;
            }
            for ox in first..last {
                let cx = x0 + ox as f64;
                // Quick reject: the pixel centre maps far outside the source. (Affine maps need no
                // division by w.)
                let (u, v) = if affine {
                    let (x, y) = (cx + 0.5, cy + 0.5);
                    (im[0] * x + im[1] * y + im[2], im[3] * x + im[4] * y + im[5])
                } else {
                    let Some(p) = inv.apply(cx + 0.5, cy + 0.5) else {
                        continue;
                    };
                    p
                };
                let n = fixed_n.unwrap_or_else(|| spread(cx, cy));
                let margin = 3.0 + n as f64;
                if u < -margin || v < -margin || u > sw as f64 + margin || v > sh as f64 + margin {
                    continue;
                }
                let mut acc = [0f64; 4];
                if n == 1 {
                    sample(src, sw, sh, ch, u, v, interp, &mut acc);
                } else {
                    for sy in 0..n {
                        for sx in 0..n {
                            let px = cx + (sx as f64 + 0.5) / n as f64;
                            let py = cy + (sy as f64 + 0.5) / n as f64;
                            if let Some((u, v)) = inv.apply(px, py) {
                                sample(src, sw, sh, ch, u, v, interp, &mut acc);
                            }
                        }
                    }
                    let k = 1.0 / (n * n) as f64;
                    acc.iter_mut().for_each(|a| *a *= k);
                }
                let o = ox * ch;
                if ch == 4 {
                    let a = acc[3].clamp(0.0, 255.0);
                    if a < 0.5 {
                        continue;
                    }
                    let inv_a = 1.0 / acc[3];
                    for c in 0..3 {
                        out[o + c] = (acc[c] * inv_a + 0.5).clamp(0.0, 255.0) as u8;
                    }
                    out[o + 3] = (a + 0.5) as u8;
                } else {
                    out[o] = (acc[0] + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
    });
    Some(Rendered {
        x: x0 as i32,
        y: y0 as i32,
        w: w as u32,
        h: h as u32,
        data: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn matrix_basics() {
        let r = Matrix::rotate_around(std::f64::consts::FRAC_PI_2, 10.0, 10.0);
        // Clockwise on screen: right of the centre goes below it.
        assert!(close(r.apply(20.0, 10.0).unwrap(), (10.0, 20.0)));
        let i = r.invert().unwrap();
        assert!(close(i.apply(10.0, 20.0).unwrap(), (20.0, 10.0)));
        assert!(Matrix::translate(3.0, -2.0).is_grid_exact());
        assert!(Matrix([-1.0, 0.0, 10.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).is_grid_exact());
        assert!(!r.mul(&Matrix::translate(0.5, 0.0)).is_grid_exact());
    }

    #[test]
    fn matrices_from_points() {
        let from = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let to = [(5.0, 5.0), (25.0, 0.0), (30.0, 30.0), (0.0, 20.0)];
        for n in 1..=4 {
            let m = Matrix::from_points(&from[..n], &to[..n]).unwrap();
            for i in 0..n {
                assert!(
                    close(m.apply(from[i].0, from[i].1).unwrap(), to[i]),
                    "{n} {i}"
                );
            }
        }
    }

    #[test]
    fn grid_exact_render_copies_pixels() {
        let src: Vec<u8> = (0..4 * 3 * 4).map(|i| (i * 7 % 256) as u8 | 0x80).collect();
        let flip = Matrix([-1.0, 0.0, 4.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        let r = render(&src, 4, 3, 4, &flip, Interpolation::None).unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (0, 0, 4, 3));
        assert_eq!(&r.data[0..4], &src[12..16]);
        let back = render(&r.data, 4, 3, 4, &flip, Interpolation::None).unwrap();
        assert_eq!(back.data, src);
    }

    #[test]
    fn identity_cubic_keeps_pixels() {
        let src: Vec<u8> = [200u8, 50, 10, 255].repeat(20 * 20);
        let r = render(&src, 20, 20, 4, &Matrix::IDENTITY, Interpolation::Cubic).unwrap();
        assert_eq!((r.w, r.h), (20, 20));
        assert!(r.data.chunks(4).all(|p| p == [200, 50, 10, 255]));
    }

    #[test]
    fn rotated_square_has_soft_edges_and_solid_middle() {
        let src: Vec<u8> = [0u8, 0, 255, 255].repeat(40 * 40);
        let m = Matrix::rotate_around(0.5, 20.0, 20.0);
        let r = render(&src, 40, 40, 4, &m, Interpolation::Linear).unwrap();
        let at = |x: i32, y: i32| {
            let i = (((y - r.y) * r.w as i32 + (x - r.x)) * 4) as usize;
            [r.data[i], r.data[i + 1], r.data[i + 2], r.data[i + 3]]
        };
        assert_eq!(at(20, 20), [0, 0, 255, 255]);
        // Some edge pixels are partly transparent but keep the colour.
        let partial: Vec<[u8; 4]> = r
            .data
            .chunks(4)
            .filter(|p| p[3] > 0 && p[3] < 255)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        assert!(!partial.is_empty());
        assert!(partial.iter().all(|p| p[2] == 255 && p[0] == 0));
    }

    #[test]
    fn downscale_by_transform_averages() {
        // 1 px checker scaled to 1/4: mid grey everywhere inside.
        let src: Vec<u8> = (0..64 * 64)
            .map(|i| if (i % 64 + i / 64) % 2 == 0 { 0 } else { 255 })
            .collect();
        let r = render(
            &src,
            64,
            64,
            1,
            &Matrix::scale(0.25, 0.25),
            Interpolation::Linear,
        )
        .unwrap();
        assert_eq!((r.w, r.h), (16, 16));
        let inner: Vec<u8> = (2..14)
            .flat_map(|y| (2..14).map(move |x| (x, y)))
            .map(|(x, y)| r.data[y * 16 + x])
            .collect();
        assert!(inner.iter().all(|&v| (110..=145).contains(&v)), "{inner:?}");
    }

    #[test]
    fn behind_the_camera_is_refused() {
        let m = Matrix([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.1, 0.0, 1.0]);
        assert!(render(&[0; 4 * 100], 10, 10, 4, &m, Interpolation::Linear).is_none());
    }
}
