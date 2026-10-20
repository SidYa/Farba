//! Colour adjustments: Invert, Exposure, Saturation, Desaturate. Each maps one straight
//! RGBA8 pixel to another and keeps alpha; `apply` runs one over a surface, limited to the
//! selection (the result is blended with the original by the selection's coverage).
//!
//! Exposure and Saturation work in linear light (like GIMP's defaults); per-channel maps
//! go through 256-entry lookup tables.

use crate::effects::Effect;
use crate::selection::Mask;
use crate::surface::{Surface, TILE_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesaturateMode {
    /// Relative luminance in linear light (GIMP's default).
    Luminance,
    /// Rec. 709 weights on the sRGB values.
    Luma,
    /// HSL lightness: (max + min) / 2.
    Lightness,
    Average,
    /// HSV value: max.
    Value,
}

impl DesaturateMode {
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Luma,
            2 => Self::Lightness,
            3 => Self::Average,
            4 => Self::Value,
            _ => Self::Luminance,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    Invert,
    /// `exposure` in stops; `black` level 0..1 (linear light).
    Exposure {
        exposure: f32,
        black: f32,
    },
    /// 0 = grey, 1 = unchanged, up to 10.
    Saturation {
        scale: f32,
    },
    Desaturate {
        mode: DesaturateMode,
    },
    /// An effect that looks at neighbouring pixels (see `effects`).
    Effect(Effect),
}

impl Filter {
    /// The filter for the frontend's `kind` and parameters (the same mapping in the WASM
    /// and the native build): 0 invert, 1 exposure, 2 saturation, 3 desaturate, 10-16 the
    /// effects.
    pub fn from_params(kind: u8, p: &[f32]) -> Filter {
        let a = |i: usize| p.get(i).copied().unwrap_or(0.0);
        let effect = Filter::Effect;
        match kind {
            1 => Filter::Exposure {
                exposure: a(0),
                black: a(1),
            },
            2 => Filter::Saturation { scale: a(0) },
            3 => Filter::Desaturate {
                mode: DesaturateMode::from_index(a(0) as u8),
            },
            10 => effect(Effect::BlackWhite {
                red: a(0),
                green: a(1),
                blue: a(2),
                contrast: a(3),
                sepia: a(4),
            }),
            11 => effect(Effect::EnhanceColors {
                auto: a(0),
                vibrance: a(1),
            }),
            12 => effect(Effect::Retouch {
                radius: a(0),
                smoothness: a(1),
                amount: a(2),
            }),
            13 => effect(Effect::ReduceNoise {
                radius: a(0),
                strength: a(1),
            }),
            14 => effect(Effect::Sharpen {
                radius: a(0),
                amount: a(1),
                threshold: a(2),
            }),
            15 => effect(Effect::GaussianBlur { sigma: a(0) }),
            16 => effect(Effect::Vignette {
                amount: a(0),
                size: a(1),
                softness: a(2),
            }),
            _ => Filter::Invert,
        }
    }
}

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Linear-light value of each 8-bit sRGB value.
fn decode_lut() -> [f32; 256] {
    std::array::from_fn(|i| to_linear(i as f32 / 255.0))
}

/// 8-bit sRGB from linear light, through a 4096-step table.
struct Encoder([u8; 4097]);

impl Encoder {
    fn new() -> Self {
        Self(std::array::from_fn(|i| {
            (to_srgb(i as f32 / 4096.0) * 255.0 + 0.5) as u8
        }))
    }

    #[inline]
    fn encode(&self, v: f32) -> u8 {
        self.0[(v.clamp(0.0, 1.0) * 4096.0 + 0.5) as usize]
    }
}

const LUM: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// A prepared filter: per-pixel function with its tables.
struct Prepared {
    filter: Filter,
    /// Per-channel table (Invert, Exposure).
    map: Option<[u8; 256]>,
    dec: [f32; 256],
    enc: Encoder,
}

impl Prepared {
    fn new(filter: Filter) -> Self {
        let dec = decode_lut();
        let enc = Encoder::new();
        let map = match filter {
            Filter::Invert => Some(std::array::from_fn(|i| 255 - i as u8)),
            Filter::Exposure { exposure, black } => {
                // GIMP: out = (in - black) * gain, gain = 1 / (2^-exposure - black).
                let gain = 1.0 / (2f32.powf(-exposure) - black).max(1e-6);
                Some(std::array::from_fn(|i| enc.encode((dec[i] - black) * gain)))
            }
            _ => None,
        };
        Self {
            filter,
            map,
            dec,
            enc,
        }
    }

    #[inline]
    fn pixel(&self, p: [u8; 4]) -> [u8; 4] {
        if let Some(m) = &self.map {
            return [m[p[0] as usize], m[p[1] as usize], m[p[2] as usize], p[3]];
        }
        match self.filter {
            Filter::Saturation { scale } => {
                let l = [
                    self.dec[p[0] as usize],
                    self.dec[p[1] as usize],
                    self.dec[p[2] as usize],
                ];
                let y = l[0] * LUM[0] + l[1] * LUM[1] + l[2] * LUM[2];
                let e = |c: f32| self.enc.encode(y + (c - y) * scale);
                [e(l[0]), e(l[1]), e(l[2]), p[3]]
            }
            Filter::Desaturate { mode } => {
                let v = match mode {
                    DesaturateMode::Luminance => {
                        let y = self.dec[p[0] as usize] * LUM[0]
                            + self.dec[p[1] as usize] * LUM[1]
                            + self.dec[p[2] as usize] * LUM[2];
                        self.enc.encode(y)
                    }
                    DesaturateMode::Luma => {
                        let y = p[0] as f32 * LUM[0] + p[1] as f32 * LUM[1] + p[2] as f32 * LUM[2];
                        (y + 0.5) as u8
                    }
                    DesaturateMode::Lightness => {
                        let max = p[0].max(p[1]).max(p[2]) as u32;
                        let min = p[0].min(p[1]).min(p[2]) as u32;
                        (max + min).div_ceil(2) as u8
                    }
                    DesaturateMode::Average => {
                        ((p[0] as u32 + p[1] as u32 + p[2] as u32 + 1) / 3) as u8
                    }
                    DesaturateMode::Value => p[0].max(p[1]).max(p[2]),
                };
                [v, v, v, p[3]]
            }
            _ => p,
        }
    }
}

/// Applies `filter` to `surface` (placed at `pos` in image coordinates), limited to `mask`.
/// Transparent tiles are left alone; unchanged tiles stay shared.
pub fn apply(
    surface: &mut Surface,
    pos: (i32, i32),
    image: (u32, u32),
    mask: Option<&Mask>,
    filter: Filter,
) {
    if let Filter::Effect(e) = filter {
        crate::effects::apply(surface, pos, image, mask, e);
        return;
    }
    let f = Prepared::new(filter);
    let t = TILE_SIZE as usize;
    let tiles: Vec<(u32, u32)> = (0..surface.tiles_y())
        .flat_map(|ty| (0..surface.tiles_x()).map(move |tx| (tx, ty)))
        .filter(|&(tx, ty)| surface.is_allocated(tx, ty))
        .collect();
    // Tiles are independent: computed on all cores, then written back.
    let source = &*surface;
    let changed = crate::par::map(tiles.len(), usize::MAX, |k| {
        let (tx, ty) = tiles[k];
        let (x0, y0, w, h) = source.tile_rect(tx, ty);
        let src = source.tile(tx, ty);
        let mut out: Option<Box<[u8]>> = None;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * t + x) * 4;
                let p = [src[i], src[i + 1], src[i + 2], src[i + 3]];
                if p[3] == 0 {
                    continue;
                }
                let mut q = f.pixel(p);
                if let Some(m) = mask {
                    let k = m.get(
                        pos.0 as i64 + x0 as i64 + x as i64,
                        pos.1 as i64 + y0 as i64 + y as i64,
                    ) as u32;
                    if k == 0 {
                        continue;
                    }
                    for c in 0..3 {
                        q[c] = ((p[c] as u32 * (255 - k) + q[c] as u32 * k + 127) / 255) as u8;
                    }
                }
                if q != p {
                    out.get_or_insert_with(|| src.to_vec().into_boxed_slice())[i..i + 4]
                        .copy_from_slice(&q);
                }
            }
        }
        out
    });
    for (&(tx, ty), out) in tiles.iter().zip(changed) {
        if let Some(out) = out {
            surface.tile_mut(tx, ty).copy_from_slice(&out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(filter: Filter, p: [u8; 4]) -> [u8; 4] {
        Prepared::new(filter).pixel(p)
    }

    #[test]
    fn invert_and_identity_exposure() {
        assert_eq!(one(Filter::Invert, [0, 100, 255, 7]), [255, 155, 0, 7]);
        let p = [12, 128, 250, 255];
        assert_eq!(
            one(
                Filter::Exposure {
                    exposure: 0.0,
                    black: 0.0
                },
                p
            ),
            p
        );
        assert_eq!(one(Filter::Saturation { scale: 1.0 }, p), p);
    }

    #[test]
    fn exposure_one_stop_doubles_linear_light() {
        let q = one(
            Filter::Exposure {
                exposure: 1.0,
                black: 0.0,
            },
            [128, 0, 255, 255],
        );
        // sRGB 128 = 0.216 linear -> 0.432 -> sRGB 175.5.
        assert_eq!(q, [176, 0, 255, 255]);
    }

    #[test]
    fn desaturate_modes() {
        let p = [255, 0, 0, 255];
        assert_eq!(
            one(
                Filter::Desaturate {
                    mode: DesaturateMode::Luminance
                },
                p
            ),
            [127, 127, 127, 255]
        );
        assert_eq!(
            one(
                Filter::Desaturate {
                    mode: DesaturateMode::Luma
                },
                p
            ),
            [54, 54, 54, 255]
        );
        assert_eq!(
            one(
                Filter::Desaturate {
                    mode: DesaturateMode::Average
                },
                p
            ),
            [85, 85, 85, 255]
        );
        // Saturation 0 is the luminance grey.
        assert_eq!(
            one(Filter::Saturation { scale: 0.0 }, p),
            [127, 127, 127, 255]
        );
    }

    #[test]
    fn apply_respects_the_mask() {
        use crate::selection::{SelectMode, rectangle};
        let mut s = Surface::new(10, 10);
        s.fill([0, 0, 0, 255]);
        let mut m = Mask::new(10, 10);
        m.combine(
            &rectangle(0.0, 0.0, 5.0, 10.0, 0.0, false, 10, 10),
            SelectMode::Replace,
        );
        apply(&mut s, (0, 0), (10, 10), Some(&m), Filter::Invert);
        assert_eq!(s.pixel(2, 2), Some([255, 255, 255, 255]));
        assert_eq!(s.pixel(7, 2), Some([0, 0, 0, 255]));
    }
}
