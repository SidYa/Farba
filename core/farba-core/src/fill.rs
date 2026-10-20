//! Bucket Fill and Gradient: painting a colour (or a colour per pixel) over a whole
//! surface with a per-pixel coverage (the selection, a colour region).

use crate::color::{Rgba8, blend_over};
use crate::surface::{Surface, TILE_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientShape {
    Linear,
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    None,
    Sawtooth,
    Triangular,
}

/// A two-colour gradient from `start` to `end` (surface coordinates).
#[derive(Clone, Copy, Debug)]
pub struct Gradient {
    pub start: (f32, f32),
    pub end: (f32, f32),
    pub shape: GradientShape,
    pub repeat: Repeat,
    pub reverse: bool,
    pub from: Rgba8,
    pub to: Rgba8,
}

impl Gradient {
    /// Position along the gradient (0 = `from`, 1 = `to`) of the point `(x, y)`.
    fn position(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (self.end.0 - self.start.0, self.end.1 - self.start.1);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let (px, py) = (x - self.start.0, y - self.start.1);
        let t = match self.shape {
            GradientShape::Linear => (px * dx + py * dy) / len2,
            GradientShape::Radial => ((px * px + py * py) / len2).sqrt(),
        };
        let t = match self.repeat {
            Repeat::None => t.clamp(0.0, 1.0),
            Repeat::Sawtooth => t.rem_euclid(1.0),
            Repeat::Triangular => {
                let f = t.rem_euclid(2.0);
                if f > 1.0 { 2.0 - f } else { f }
            }
        };
        if self.reverse { 1.0 - t } else { t }
    }

    /// The colour at `(x, y)`, interpolated in premultiplied form (so "to transparent"
    /// keeps the colour instead of fading through black).
    pub fn color_at(&self, x: f32, y: f32) -> [u8; 4] {
        let t = self.position(x, y);
        let (a0, a1) = (self.from.a as f32, self.to.a as f32);
        let a = a0 + (a1 - a0) * t;
        if a <= 0.0 {
            return [0; 4];
        }
        let ch = |c0: u8, c1: u8| {
            let v = (c0 as f32 * a0 + (c1 as f32 * a1 - c0 as f32 * a0) * t) / a;
            (v + 0.5).clamp(0.0, 255.0) as u8
        };
        [
            ch(self.from.r, self.to.r),
            ch(self.from.g, self.to.g),
            ch(self.from.b, self.to.b),
            (a + 0.5) as u8,
        ]
    }
}

/// Paints `color(x, y)` over every pixel of `surface` with alpha scaled by
/// `coverage(x, y) / 255 * opacity` (surface coordinates). With `lock_alpha` only the colour
/// of non-transparent pixels changes. Tiles that do not change stay shared.
pub fn paint_over(
    surface: &mut Surface,
    opacity: f32,
    lock_alpha: bool,
    coverage: impl Fn(u32, u32) -> u8,
    color: impl Fn(u32, u32) -> [u8; 4],
) {
    if opacity <= 0.0 {
        return;
    }
    let t = TILE_SIZE as usize;
    for ty in 0..surface.tiles_y() {
        for tx in 0..surface.tiles_x() {
            if lock_alpha && !surface.is_allocated(tx, ty) {
                continue;
            }
            let (x0, y0, w, h) = surface.tile_rect(tx, ty);
            let mut out: Option<Box<[u8]>> = None;
            let src = surface.tile(tx, ty);
            for y in 0..h {
                for x in 0..w {
                    let cov = coverage(x0 + x, y0 + y);
                    if cov == 0 {
                        continue;
                    }
                    let i = (y as usize * t + x as usize) * 4;
                    let p = [src[i], src[i + 1], src[i + 2], src[i + 3]];
                    if lock_alpha && p[3] == 0 {
                        continue;
                    }
                    let c = color(x0 + x, y0 + y);
                    let a = opacity * cov as f32 / 255.0;
                    let mut q = p;
                    if lock_alpha {
                        let a = a * c[3] as f32 / 255.0;
                        for k in 0..3 {
                            q[k] = (p[k] as f32 + (c[k] as f32 - p[k] as f32) * a + 0.5) as u8;
                        }
                    } else {
                        blend_over(&mut q, &c, a);
                    }
                    if q != p {
                        out.get_or_insert_with(|| src.to_vec().into_boxed_slice())[i..i + 4]
                            .copy_from_slice(&q);
                    }
                }
            }
            if let Some(out) = out {
                surface.tile_mut(tx, ty).copy_from_slice(&out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const BLUE: Rgba8 = Rgba8::new(0, 0, 255, 255);

    fn gradient(shape: GradientShape, repeat: Repeat) -> Gradient {
        Gradient {
            start: (0.0, 0.0),
            end: (100.0, 0.0),
            shape,
            repeat,
            reverse: false,
            from: RED,
            to: BLUE,
        }
    }

    #[test]
    fn linear_gradient_ends_and_middle() {
        let g = gradient(GradientShape::Linear, Repeat::None);
        assert_eq!(g.color_at(0.0, 7.0), [255, 0, 0, 255]);
        assert_eq!(g.color_at(50.0, 0.0), [128, 0, 128, 255]);
        assert_eq!(g.color_at(150.0, 0.0), [0, 0, 255, 255]);
        let g = gradient(GradientShape::Linear, Repeat::Triangular);
        assert_eq!(g.color_at(150.0, 0.0), [128, 0, 128, 255]);
        assert_eq!(g.color_at(200.0, 0.0), [255, 0, 0, 255]);
        let g = gradient(GradientShape::Radial, Repeat::None);
        assert_eq!(g.color_at(0.0, 50.0), [128, 0, 128, 255]);
    }

    #[test]
    fn to_transparent_keeps_the_colour() {
        let g = Gradient {
            to: Rgba8::TRANSPARENT,
            ..gradient(GradientShape::Linear, Repeat::None)
        };
        assert_eq!(g.color_at(50.0, 0.0), [255, 0, 0, 128]);
    }

    #[test]
    fn paint_over_uses_coverage_and_lock_alpha() {
        let mut s = Surface::new(10, 10);
        paint_over(
            &mut s,
            1.0,
            false,
            |x, _| if x < 5 { 255 } else { 0 },
            |_, _| [9, 9, 9, 255],
        );
        assert_eq!(s.pixel(2, 2), Some([9, 9, 9, 255]));
        assert_eq!(s.pixel(7, 2), Some([0, 0, 0, 0]));
        paint_over(&mut s, 0.5, true, |_, _| 255, |_, _| [109, 109, 109, 255]);
        assert_eq!(s.pixel(2, 2), Some([59, 59, 59, 255]));
        assert_eq!(s.pixel(7, 2), Some([0, 0, 0, 0]));
    }
}
