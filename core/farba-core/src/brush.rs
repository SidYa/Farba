//! Round brush: dab stamping, stroke interpolation and dirty-tile tracking.
//!
//! A stroke works like GIMP's paint core in "constant" mode: dabs accumulate into a
//! per-stroke coverage buffer that approaches the brush opacity but never exceeds it, and
//! the layer pixels are recomputed from the pixels the stroke started with. So a stroke
//! never builds up past its opacity where it overlaps itself, and painting the same spot
//! again within one stroke changes nothing.
//!
//! The dab shape comes from a lookup table indexed by squared distance (no square root or
//! falloff curve per pixel), and only pixels whose coverage grew are recomputed.
//!
//! The retouching tools use the same strokes. Clone, Heal and Dodge / Burn are "constant"
//! too: only the colour a pixel moves towards changes (the source pixel; the source pixel
//! plus a smooth correction that matches the surroundings; the original pixel lightened or
//! darkened). Smudge and Blur / Sharpen read the pixels as they are, so their dabs change
//! the surface directly ("incremental", repeated dabs add up, as in GIMP).

pub use crate::color::Rgba8;
use crate::selection::Mask;
use crate::surface::{Surface, TILE_SIZE};

const T: usize = TILE_SIZE as usize;
/// Coverage of 1.0 in the stroke buffer.
const ONE: u32 = 65535;
const LUT_SIZE: usize = 4096;

/// Painting is limited to the selection: the mask and the layer's position in the image.
pub type Clip<'a> = Option<(&'a Mask, i32, i32)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintMode {
    /// Paint `color` over the pixels ("normal" mode).
    Paint,
    /// Lower the alpha (the Eraser).
    Erase,
    /// Paint the pixels of a source (see [`CloneSource`]).
    Clone,
    /// Clone, corrected to blend into the surroundings (GIMP's Heal).
    Heal,
    /// Lighten (`true`) or darken (`false`) a tonal range; `Brush::rate` is the exposure.
    DodgeBurn(bool, Range),
    /// Smear the pixels along the stroke; `rate` = how long the smear lasts, `flow` = how
    /// much of `color` it picks up.
    Smudge,
    /// Blur (`false`) or sharpen (`true`); `rate` = strength.
    Convolve(bool),
}

/// The tonal range Dodge / Burn works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Shadows,
    Midtones,
    Highlights,
}

impl PaintMode {
    /// Smudge and Blur / Sharpen change the current pixels dab by dab.
    fn incremental(self) -> bool {
        matches!(self, PaintMode::Smudge | PaintMode::Convolve(_))
    }
}

/// Where Clone and Heal take their pixels from: a snapshot of a layer (or the merged
/// image) taken when the stroke starts.
#[derive(Clone)]
pub struct CloneSource {
    pub surface: Surface,
    /// Position of `surface` in the image.
    pub origin: (i32, i32),
    /// Image offset from a painted pixel to its source pixel.
    pub delta: (i32, i32),
    /// GIMP's "Fixed" alignment: every dab takes the pixels around this image point.
    pub fixed: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Brush {
    /// Diameter in pixels.
    pub size: f32,
    /// 0 = soft (falloff over the whole radius), 1 = hard edge (1 px antialiasing only).
    pub hardness: f32,
    /// 0..=1: the most a stroke can cover.
    pub opacity: f32,
    /// Distance between dabs as a fraction of the diameter.
    pub spacing: f32,
    pub color: Rgba8,
    /// Hard pixels without antialiasing (Pencil, Eraser "Hard edge").
    pub aliased: bool,
    pub mode: PaintMode,
    /// Keep the existing alpha and only change colour (the layer's "lock alpha").
    pub lock_alpha: bool,
    /// Strength of the retouching tools, 0..=1 (Smudge rate, Blur / Sharpen rate,
    /// Dodge / Burn exposure).
    pub rate: f32,
    /// Smudge: how much of `color` the smear picks up, 0..=1.
    pub flow: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 20.0,
            hardness: 0.5,
            opacity: 1.0,
            spacing: 0.1,
            color: Rgba8::new(0, 0, 0, 255),
            aliased: false,
            mode: PaintMode::Paint,
            lock_alpha: false,
            rate: 0.5,
            flow: 0.0,
        }
    }
}

impl Brush {
    fn radius(&self) -> f32 {
        (self.size * 0.5).max(0.5)
    }

    /// Dab coverage (0..=ONE) by squared distance from the centre, `LUT_SIZE` steps over
    /// `0..radius²`.
    fn lut(&self) -> Vec<u32> {
        let r = self.radius();
        let hard_r = (r * self.hardness).min(r - 1.0).max(0.0);
        let falloff = r - hard_r;
        (0..LUT_SIZE)
            .map(|i| {
                let d = ((i as f32 + 0.5) / LUT_SIZE as f32 * r * r).sqrt();
                let cov = if self.aliased || d <= hard_r {
                    1.0
                } else {
                    let t = 1.0 - (d - hard_r) / falloff;
                    t * t * (3.0 - 2.0 * t)
                };
                (cov * ONE as f32 + 0.5) as u32
            })
            .collect()
    }
}

/// Set of touched tiles, in first-touched order.
#[derive(Default)]
pub struct DirtyTiles {
    marked: Vec<bool>,
    list: Vec<u32>,
}

impl DirtyTiles {
    fn mark(&mut self, index: usize, tile_count: usize) {
        if self.marked.len() != tile_count {
            self.marked = vec![false; tile_count];
        }
        if !self.marked[index] {
            self.marked[index] = true;
            self.list.push(index as u32);
        }
    }

    /// Returns the touched tile indices and clears the set.
    pub fn take(&mut self) -> Vec<u32> {
        for &i in &self.list {
            self.marked[i as usize] = false;
        }
        std::mem::take(&mut self.list)
    }
}

/// One continuous stroke (pointer down .. pointer up) on one surface.
pub struct Stroke {
    pub brush: Brush,
    lut: Vec<u32>,
    /// The surface as it was when the stroke started.
    orig: Surface,
    /// Stroke coverage per tile (0..=ONE).
    cov: Vec<Option<Box<[u16; T * T]>>>,
    last: Option<(f32, f32)>,
    /// Distance already travelled since the last dab.
    carry: f32,
    /// Clone / Heal: the source pixels; `layer_origin` = the painted layer's position.
    source: Option<CloneSource>,
    layer_origin: (i32, i32),
    /// Smudge: the colour being smeared (premultiplied RGBA per brush pixel).
    smear: Option<Vec<[f32; 4]>>,
}

impl Stroke {
    /// A stroke on `surface` (its current pixels are the stroke's starting point).
    pub fn new(brush: Brush, surface: &Surface) -> Self {
        Self {
            brush,
            lut: brush.lut(),
            orig: surface.clone(),
            cov: vec![None; (surface.tiles_x() * surface.tiles_y()) as usize],
            last: None,
            carry: 0.0,
            source: None,
            layer_origin: (0, 0),
            smear: None,
        }
    }

    /// A Clone / Heal stroke on a layer at `layer_origin` (image coordinates).
    pub fn with_source(
        brush: Brush,
        surface: &Surface,
        source: CloneSource,
        layer_origin: (i32, i32),
    ) -> Self {
        Self {
            source: Some(source),
            layer_origin,
            ..Self::new(brush, surface)
        }
    }

    /// The last point the stroke reached.
    pub fn last(&self) -> Option<(f32, f32)> {
        self.last
    }

    /// Extends the stroke to `(x, y)`, stamping dabs along the way.
    pub fn line_to(
        &mut self,
        surface: &mut Surface,
        dirty: &mut DirtyTiles,
        clip: Clip,
        x: f32,
        y: f32,
    ) {
        let Some((lx, ly)) = self.last else {
            self.dab(surface, dirty, clip, x, y);
            self.last = Some((x, y));
            return;
        };
        let step = (self.brush.size * self.brush.spacing).max(0.5);
        let (dx, dy) = (x - lx, y - ly);
        let dist = (dx * dx + dy * dy).sqrt();
        let mut t = step - self.carry;
        while t <= dist {
            let k = t / dist;
            self.dab(surface, dirty, clip, lx + dx * k, ly + dy * k);
            t += step;
        }
        self.carry = dist - (t - step);
        self.last = Some((x, y));
    }

    /// Stamps one dab centred at `(cx, cy)` (pixel centres are at `.5`).
    fn dab(&mut self, surface: &mut Surface, dirty: &mut DirtyTiles, clip: Clip, cx: f32, cy: f32) {
        if self.brush.mode.incremental() {
            self.dab_incremental(surface, dirty, clip, cx, cy);
            return;
        }
        let b = self.brush;
        let r = b.radius();
        // Aliased dabs cover whole pixels: align the dab to the pixel grid so a 1 px pencil
        // always hits exactly one pixel.
        let (cx, cy) = if b.aliased {
            ((cx - r).round() + r, (cy - r).round() + r)
        } else {
            (cx, cy)
        };
        let x0 = ((cx - r).floor().max(0.0)) as u32;
        let y0 = ((cy - r).floor().max(0.0)) as u32;
        let x1 = ((cx + r).ceil().min(surface.width() as f32)) as u32;
        let y1 = ((cy + r).ceil().min(surface.height() as f32)) as u32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let r2 = r * r;
        // Aliased: a pixel is in when its centre is (slightly generous, so a 1 px dab on a
        // pixel centre counts).
        let limit = if b.aliased { r2 + 1e-3 } else { r2 };
        let scale = LUT_SIZE as f32 / limit;
        let op = (b.opacity.clamp(0.0, 1.0) * ONE as f32 + 0.5) as u32;
        if op == 0 || (b.mode == PaintMode::Erase && b.lock_alpha) {
            return;
        }
        let tile_count = (surface.tiles_x() * surface.tiles_y()) as usize;

        // Clone / Heal: where this dab's pixels come from (layer coordinates + delta =
        // source image point), and Heal's correction over the dab's box.
        let delta = self.source.as_ref().map(|s| match s.fixed {
            Some((fx, fy)) => (
                (fx - (cx + self.layer_origin.0 as f32)).round() as i32,
                (fy - (cy + self.layer_origin.1 as f32)).round() as i32,
            ),
            None => s.delta,
        });
        let heal = if b.mode == PaintMode::Heal {
            let d = delta.unwrap_or((0, 0));
            Some(self.heal_correction(surface, (x0, y0, x1, y1), (cx, cy, r), d))
        } else {
            None
        };
        let (source, lo) = (self.source.as_ref(), self.layer_origin);

        for ty in y0 / TILE_SIZE..=(y1 - 1) / TILE_SIZE {
            for tx in x0 / TILE_SIZE..=(x1 - 1) / TILE_SIZE {
                let (tx0, ty0, tw, th) = surface.tile_rect(tx, ty);
                let px0 = x0.max(tx0);
                let px1 = x1.min(tx0 + tw);
                let py0 = y0.max(ty0);
                let py1 = y1.min(ty0 + th);
                let index = surface.tile_index(tx, ty);
                let orig = self.orig.tile(tx, ty);
                // Erasing transparent pixels changes nothing.
                if b.mode == PaintMode::Erase && !self.orig.is_allocated(tx, ty) {
                    continue;
                }
                let cov = self.cov[index].get_or_insert_with(|| Box::new([0; T * T]));
                let mut tile: Option<&mut [u8]> = None;
                for py in py0..py1 {
                    let fy = py as f32 + 0.5 - cy;
                    let fy2 = fy * fy;
                    if fy2 >= limit {
                        continue;
                    }
                    let row = (py - ty0) as usize * T;
                    for px in px0..px1 {
                        let fx = px as f32 + 0.5 - cx;
                        let d2 = fx * fx + fy2;
                        if d2 >= limit {
                            continue;
                        }
                        let mut m = self.lut[((d2 * scale) as usize).min(LUT_SIZE - 1)];
                        if let Some((mask, ox, oy)) = clip {
                            let s = mask.get(px as i64 + ox as i64, py as i64 + oy as i64) as u32;
                            m = m * s / 255;
                        }
                        let i = row + (px - tx0) as usize;
                        let c = cov[i] as u32;
                        if m == 0 || c >= op {
                            continue;
                        }
                        // Grow towards the opacity: c += (op - c) * m.
                        let n = c + ((op - c) * m + ONE / 2) / ONE;
                        if n == c {
                            continue;
                        }
                        cov[i] = n as u16;
                        if tile.is_none() {
                            tile = Some(&mut surface.tile_mut(tx, ty)[..]);
                        }
                        let t = tile.as_mut().unwrap();
                        let p = i * 4;
                        let src = [orig[p], orig[p + 1], orig[p + 2], orig[p + 3]];
                        let target = match b.mode {
                            PaintMode::Clone | PaintMode::Heal => {
                                let (dx, dy) = delta.unwrap_or((0, 0));
                                let Some(mut s) = source_px(source, lo, px, py, dx, dy) else {
                                    continue;
                                };
                                if let Some((hx0, hy0, hw, corr)) = &heal {
                                    let k = ((py - hy0) as usize * hw + (px - hx0) as usize) * 4;
                                    for c in 0..4 {
                                        s[c] = (s[c] as f32 + corr[k + c]).round().clamp(0.0, 255.0)
                                            as u8;
                                    }
                                }
                                s
                            }
                            PaintMode::DodgeBurn(dodge, range) => tonal(src, dodge, range, b.rate),
                            _ => b.color.to_array(),
                        };
                        let out = apply(&b, src, n as f32 / ONE as f32, target);
                        t[p..p + 4].copy_from_slice(&out);
                    }
                }
                if tile.is_some() {
                    dirty.mark(index, tile_count);
                }
            }
        }
    }
}

/// The result of moving the original pixel `p` `a` (0..=1) of the way to `target` (the
/// brush colour, a source pixel, or `p` lightened / darkened).
#[inline]
fn apply(b: &Brush, p: [u8; 4], a: f32, target: [u8; 4]) -> [u8; 4] {
    match b.mode {
        PaintMode::Erase => {
            let na = (p[3] as f32 * (1.0 - a) + 0.5) as u8;
            [p[0], p[1], p[2], na]
        }
        PaintMode::DodgeBurn(..) => {
            let mut out = p;
            for c in 0..3 {
                out[c] = (p[c] as f32 + (target[c] as f32 - p[c] as f32) * a + 0.5) as u8;
            }
            out
        }
        _ => {
            let src = [target[0] as f32, target[1] as f32, target[2] as f32];
            let a = a * target[3] as f32 / 255.0;
            if b.lock_alpha {
                if p[3] == 0 {
                    return p;
                }
                let mut out = p;
                for c in 0..3 {
                    out[c] = (p[c] as f32 + (src[c] - p[c] as f32) * a + 0.5) as u8;
                }
                return out;
            }
            let da = p[3] as f32 / 255.0;
            let out_a = a + da * (1.0 - a);
            if out_a <= 0.0 {
                return p;
            }
            let wd = da * (1.0 - a);
            let mut out = [0u8; 4];
            for c in 0..3 {
                out[c] = ((src[c] * a + p[c] as f32 * wd) / out_a + 0.5) as u8;
            }
            out[3] = (out_a * 255.0 + 0.5) as u8;
            out
        }
    }
}

/// Dodge / Burn of one pixel (sRGB channels; alpha kept).
fn tonal(p: [u8; 4], dodge: bool, range: Range, exposure: f32) -> [u8; 4] {
    let e = exposure.clamp(0.0, 1.0);
    let f = |v: f32| -> f32 {
        match (dodge, range) {
            (true, Range::Highlights) => v * (1.0 + e),
            (true, Range::Midtones) => v.powf(1.0 / (1.0 + e)),
            (true, Range::Shadows) => e + v * (1.0 - e),
            (false, Range::Highlights) => v * (1.0 - e),
            (false, Range::Midtones) => v.powf(1.0 + e),
            (false, Range::Shadows) => {
                if e >= 1.0 {
                    0.0
                } else {
                    (v - e) / (1.0 - e)
                }
            }
        }
    };
    let mut out = p;
    for c in 0..3 {
        out[c] = (f(p[c] as f32 / 255.0).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
    out
}

fn premul(p: &[u8]) -> [f32; 4] {
    let a = p[3] as f32 / 255.0;
    [
        p[0] as f32 / 255.0 * a,
        p[1] as f32 / 255.0 * a,
        p[2] as f32 / 255.0 * a,
        a,
    ]
}

fn unpremul(p: [f32; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    let c = |v: f32| ((v / a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [c(p[0]), c(p[1]), c(p[2]), (a * 255.0 + 0.5) as u8]
}

/// The source pixel for layer pixel `(px, py)` of a layer at `lo` (Clone / Heal), or `None`
/// outside the source.
fn source_px(
    source: Option<&CloneSource>,
    lo: (i32, i32),
    px: u32,
    py: u32,
    dx: i32,
    dy: i32,
) -> Option<[u8; 4]> {
    let s = source?;
    let x = px as i64 + lo.0 as i64 + dx as i64 - s.origin.0 as i64;
    let y = py as i64 + lo.1 as i64 + dy as i64 - s.origin.1 as i64;
    s.surface.pixel(x, y)
}

impl Stroke {
    /// Heal: the correction added to the source pixels over the dab box `(x0, y0, x1, y1)`
    /// (layer coordinates): the difference between the layer and the source, kept as it is
    /// outside the brush circle and smoothly interpolated inside it (a membrane: Laplace's
    /// equation, solved on a coarse grid). Returns `(x0, y0, width, RGBA corrections)`.
    fn heal_correction(
        &self,
        surface: &Surface,
        (x0, y0, x1, y1): (u32, u32, u32, u32),
        (cx, cy, r): (f32, f32, f32),
        (dx, dy): (i32, i32),
    ) -> (u32, u32, usize, Vec<f32>) {
        // One pixel more around the box, so the border is outside the circle.
        let bx0 = x0.saturating_sub(1);
        let by0 = y0.saturating_sub(1);
        let bx1 = (x1 + 1).min(surface.width());
        let by1 = (y1 + 1).min(surface.height());
        let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
        let dest = surface.read_rect(bx0 as i64, by0 as i64, w as u32, h as u32);
        let mut diff = vec![0f32; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let s = source_px(
                    self.source.as_ref(),
                    self.layer_origin,
                    bx0 + x as u32,
                    by0 + y as u32,
                    dx,
                    dy,
                )
                .unwrap_or([dest[i], dest[i + 1], dest[i + 2], dest[i + 3]]);
                for c in 0..4 {
                    diff[i + c] = dest[i + c] as f32 - s[c] as f32;
                }
            }
        }
        // Coarse grid (at most ~40 cells across).
        let k = (w.max(h)).div_ceil(40).max(1);
        let (gw, gh) = (w.div_ceil(k), h.div_ceil(k));
        let mut grid = vec![[0f32; 4]; gw * gh];
        let mut fixed = vec![false; gw * gh];
        for gy in 0..gh {
            for gx in 0..gw {
                let (mut sum, mut n) = ([0f32; 4], 0f32);
                for y in gy * k..((gy + 1) * k).min(h) {
                    for x in gx * k..((gx + 1) * k).min(w) {
                        let i = (y * w + x) * 4;
                        for c in 0..4 {
                            sum[c] += diff[i + c];
                        }
                        n += 1.0;
                    }
                }
                let g = gy * gw + gx;
                grid[g] = sum.map(|v| v / n.max(1.0));
                // The cell centre in layer coordinates: outside the circle = boundary.
                let ccx = bx0 as f32 + (gx * k) as f32 + k as f32 * 0.5;
                let ccy = by0 as f32 + (gy * k) as f32 + k as f32 * 0.5;
                let d = ((ccx - cx).powi(2) + (ccy - cy).powi(2)).sqrt();
                fixed[g] =
                    d > r - k as f32 * 0.5 || gx == 0 || gy == 0 || gx == gw - 1 || gy == gh - 1;
            }
        }
        // Interior: start from the mean of the boundary, then relax (SOR).
        let (mut sum, mut n) = ([0f32; 4], 0f32);
        for (g, f) in grid.iter().zip(&fixed) {
            if *f {
                for c in 0..4 {
                    sum[c] += g[c];
                }
                n += 1.0;
            }
        }
        let mean = sum.map(|v| v / n.max(1.0));
        for (g, f) in grid.iter_mut().zip(&fixed) {
            if !*f {
                *g = mean;
            }
        }
        for _ in 0..(gw.max(gh) * 3).max(20) {
            for gy in 1..gh.saturating_sub(1) {
                for gx in 1..gw.saturating_sub(1) {
                    let g = gy * gw + gx;
                    if fixed[g] {
                        continue;
                    }
                    let (l, rt, up, dn) = (grid[g - 1], grid[g + 1], grid[g - gw], grid[g + gw]);
                    for (c, v) in grid[g].iter_mut().enumerate() {
                        let avg = (l[c] + rt[c] + up[c] + dn[c]) * 0.25;
                        *v += 1.7 * (avg - *v);
                    }
                }
            }
        }
        // Back to pixels (bilinear between cell centres), only for the dab box.
        let (ow, oh) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut out = vec![0f32; ow * oh * 4];
        let at = |gx: isize, gy: isize| {
            grid[gy.clamp(0, gh as isize - 1) as usize * gw + gx.clamp(0, gw as isize - 1) as usize]
        };
        for y in 0..oh {
            for x in 0..ow {
                let lx = (x0 - bx0) as f32 + x as f32 + 0.5;
                let ly = (y0 - by0) as f32 + y as f32 + 0.5;
                let fx = lx / k as f32 - 0.5;
                let fy = ly / k as f32 - 0.5;
                let (ix, iy) = (fx.floor() as isize, fy.floor() as isize);
                let (tx, ty) = (fx - ix as f32, fy - iy as f32);
                let (a, b2, c2, d) = (
                    at(ix, iy),
                    at(ix + 1, iy),
                    at(ix, iy + 1),
                    at(ix + 1, iy + 1),
                );
                let o = (y * ow + x) * 4;
                for ch in 0..4 {
                    let top = a[ch] + (b2[ch] - a[ch]) * tx;
                    let bot = c2[ch] + (d[ch] - c2[ch]) * tx;
                    out[o + ch] = top + (bot - top) * ty;
                }
            }
        }
        (x0, y0, ow, out)
    }

    /// Smudge and Blur / Sharpen: one dab that reads and changes the current pixels.
    fn dab_incremental(
        &mut self,
        surface: &mut Surface,
        dirty: &mut DirtyTiles,
        clip: Clip,
        cx: f32,
        cy: f32,
    ) {
        let b = self.brush;
        let r = b.radius();
        // The brush box (unclamped, so the smear stays attached to the brush).
        let ox = (cx - r).floor() as i64;
        let oy = (cy - r).floor() as i64;
        // A fixed size, so the smear buffer lines up from dab to dab.
        let n = (2.0 * r).ceil() as usize + 1;
        let m = n;
        // Current pixels with a 1 px margin (Blur / Sharpen look at neighbours).
        let cur = surface.read_rect(ox - 1, oy - 1, n as u32 + 2, m as u32 + 2);
        let cw = n + 2;
        let pm = |x: usize, y: usize| premul(&cur[(y * cw + x) * 4..(y * cw + x) * 4 + 4]);
        let r2 = r * r;
        let scale = LUT_SIZE as f32 / r2;
        let op = b.opacity.clamp(0.0, 1.0);
        let rate = b.rate.clamp(0.0, 1.0);
        if b.mode == PaintMode::Smudge && self.smear.is_none() {
            // The smear starts as the pixels under the first dab.
            self.smear = Some((0..n * m).map(|i| pm(i % n + 1, i / n + 1)).collect());
        }
        let fg = {
            let c = b.color;
            premul(&[c.r, c.g, c.b, c.a])
        };
        let mut out: Vec<Option<[f32; 4]>> = vec![None; n * m];
        for y in 0..m {
            for x in 0..n {
                let (px, py) = (ox + x as i64, oy + y as i64);
                if px < 0 || py < 0 || px >= surface.width() as i64 || py >= surface.height() as i64
                {
                    continue;
                }
                let fx = px as f32 + 0.5 - cx;
                let fy = py as f32 + 0.5 - cy;
                let d2 = fx * fx + fy * fy;
                if d2 >= r2 {
                    continue;
                }
                let mut a =
                    self.lut[((d2 * scale) as usize).min(LUT_SIZE - 1)] as f32 / ONE as f32 * op;
                if let Some((mask, mx, my)) = clip {
                    a *= mask.get(px + mx as i64, py + my as i64) as f32 / 255.0;
                }
                if a <= 0.0 {
                    continue;
                }
                let c = pm(x + 1, y + 1);
                let target = match b.mode {
                    PaintMode::Smudge => {
                        let s = &mut self.smear.as_mut().unwrap()[y * n + x];
                        // Picks up some of the colour (GIMP's flow).
                        for k in 0..4 {
                            s[k] += (fg[k] - s[k]) * b.flow.clamp(0.0, 1.0);
                        }
                        *s
                    }
                    PaintMode::Convolve(sharpen) => {
                        // 3 x 3 Gaussian of the neighbours.
                        let mut bl = [0f32; 4];
                        for (dy, wy) in [(0usize, 1.0f32), (1, 2.0), (2, 1.0)] {
                            for (dx, wx) in [(0usize, 1.0f32), (1, 2.0), (2, 1.0)] {
                                let q = pm(x + dx, y + dy);
                                for k in 0..4 {
                                    bl[k] += q[k] * wx * wy / 16.0;
                                }
                            }
                        }
                        if sharpen {
                            let mut s = [0f32; 4];
                            for k in 0..4 {
                                s[k] = c[k] + (c[k] - bl[k]);
                            }
                            s
                        } else {
                            bl
                        }
                    }
                    _ => c,
                };
                let amount = if b.mode == PaintMode::Smudge {
                    a
                } else {
                    a * rate
                };
                let mut o = [0f32; 4];
                for k in 0..4 {
                    o[k] = c[k] + (target[k] - c[k]) * amount;
                }
                o[3] = o[3].clamp(0.0, 1.0);
                for k in 0..3 {
                    o[k] = o[k].clamp(0.0, o[3]);
                }
                if b.lock_alpha {
                    // Keep the alpha: rescale the colour to the old coverage.
                    let s = if o[3] > 0.0 { c[3] / o[3] } else { 0.0 };
                    for v in &mut o[..3] {
                        *v *= s;
                    }
                    o[3] = c[3];
                }
                out[y * n + x] = Some(o);
            }
        }
        // The smear continues with what is under the brush now (GIMP's rate).
        if b.mode == PaintMode::Smudge
            && let Some(smear) = self.smear.as_mut()
        {
            for (i, s) in smear.iter_mut().enumerate() {
                let now = out[i].unwrap_or_else(|| pm(i % n + 1, i / n + 1));
                for k in 0..4 {
                    s[k] = s[k] * rate + now[k] * (1.0 - rate);
                }
            }
        }
        let tile_count = (surface.tiles_x() * surface.tiles_y()) as usize;
        for y in 0..m {
            for x in 0..n {
                let Some(o) = out[y * n + x] else { continue };
                let (px, py) = ((ox + x as i64) as u32, (oy + y as i64) as u32);
                let (tx, ty) = (px / TILE_SIZE, py / TILE_SIZE);
                let index = surface.tile_index(tx, ty);
                let i = ((py % TILE_SIZE) as usize * T + (px % TILE_SIZE) as usize) * 4;
                let v = unpremul(o);
                let tile = surface.tile_mut(tx, ty);
                if tile[i..i + 4] != v {
                    tile[i..i + 4].copy_from_slice(&v);
                    dirty.mark(index, tile_count);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(s: &Surface, x: i64, y: i64) -> [u8; 4] {
        s.pixel(x, y).unwrap()
    }

    fn stroke(s: &mut Surface, brush: Brush, points: &[(f32, f32)]) -> Vec<u32> {
        let mut dirty = DirtyTiles::default();
        let mut st = Stroke::new(brush, s);
        for &(x, y) in points {
            st.line_to(s, &mut dirty, None, x, y);
        }
        dirty.take()
    }

    #[test]
    fn hard_dab_fills_centre_and_marks_tiles() {
        let mut s = Surface::new(200, 200);
        let brush = Brush {
            size: 10.0,
            hardness: 1.0,
            ..Brush::default()
        };
        let tiles = stroke(&mut s, brush, &[(64.0, 64.0)]); // on a tile corner
        assert_eq!(px(&s, 64, 64), [0, 0, 0, 255]);
        assert_eq!(tiles.len(), 4);
    }

    #[test]
    fn stroke_spacing_is_continuous_across_segments() {
        let mut s = Surface::new(300, 50);
        let brush = Brush {
            size: 4.0,
            hardness: 1.0,
            spacing: 0.25,
            ..Brush::default()
        };
        let pts: Vec<(f32, f32)> = (10..=290).step_by(7).map(|x| (x as f32, 25.0)).collect();
        stroke(&mut s, brush, &pts);
        assert!((12..288).all(|x| px(&s, x, 25)[3] == 255));
    }

    #[test]
    fn opacity_does_not_build_up_within_a_stroke() {
        let mut s = Surface::new(100, 20);
        let brush = Brush {
            size: 10.0,
            hardness: 1.0,
            opacity: 0.5,
            ..Brush::default()
        };
        // Back and forth over the same line.
        stroke(
            &mut s,
            brush,
            &[(10.0, 10.0), (90.0, 10.0), (10.0, 10.0), (90.0, 10.0)],
        );
        assert_eq!(px(&s, 50, 10), [0, 0, 0, 128]);
        // A second stroke adds on top.
        stroke(&mut s, brush, &[(10.0, 10.0), (90.0, 10.0)]);
        assert_eq!(px(&s, 50, 10), [0, 0, 0, 192]);
    }

    #[test]
    fn soft_brush_fades_out() {
        let mut s = Surface::new(100, 100);
        let brush = Brush {
            size: 40.0,
            hardness: 0.0,
            ..Brush::default()
        };
        stroke(&mut s, brush, &[(50.0, 50.0)]);
        let centre = px(&s, 50, 50)[3];
        let mid = px(&s, 60, 50)[3];
        let edge = px(&s, 68, 50)[3];
        assert!(
            centre > 240 && mid > edge && edge > 0 && edge < 40,
            "{centre} {mid} {edge}"
        );
        assert_eq!(px(&s, 71, 50)[3], 0);
    }

    #[test]
    fn one_pixel_pencil_hits_one_pixel() {
        let mut s = Surface::new(10, 10);
        let brush = Brush {
            size: 1.0,
            aliased: true,
            ..Brush::default()
        };
        stroke(&mut s, brush, &[(3.2, 4.9)]);
        let painted: Vec<(i64, i64)> = (0..10)
            .flat_map(|y| (0..10).map(move |x| (x, y)))
            .filter(|&(x, y)| px(&s, x, y)[3] != 0)
            .collect();
        assert_eq!(painted, vec![(3, 4)]);
    }

    #[test]
    fn eraser_lowers_alpha_and_respects_lock_alpha() {
        let mut s = Surface::new(50, 50);
        s.fill([255, 0, 0, 255]);
        let eraser = Brush {
            size: 10.0,
            hardness: 1.0,
            mode: PaintMode::Erase,
            ..Brush::default()
        };
        stroke(&mut s, eraser, &[(25.0, 25.0)]);
        assert_eq!(px(&s, 25, 25), [255, 0, 0, 0]);
        let mut locked = Surface::new(50, 50);
        locked.fill([255, 0, 0, 255]);
        stroke(
            &mut locked,
            Brush {
                lock_alpha: true,
                ..eraser
            },
            &[(25.0, 25.0)],
        );
        assert_eq!(px(&locked, 25, 25), [255, 0, 0, 255]);
    }

    fn split(w: u32, h: u32) -> Surface {
        // Left half red, right half blue, opaque.
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < w / 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        Surface::from_rgba(w, h, &rgba)
    }

    #[test]
    fn clone_copies_the_source() {
        let mut s = split(100, 20);
        let src = CloneSource {
            surface: s.clone(),
            origin: (0, 0),
            delta: (-60, 0),
            fixed: None,
        };
        let brush = Brush {
            size: 6.0,
            hardness: 1.0,
            mode: PaintMode::Clone,
            ..Brush::default()
        };
        let mut st = Stroke::with_source(brush, &s, src, (0, 0));
        let mut dirty = DirtyTiles::default();
        st.line_to(&mut s, &mut dirty, None, 80.0, 10.0);
        // The blue side gets the red pixels from 60 px to the left.
        assert_eq!(px(&s, 80, 10), [255, 0, 0, 255]);
        assert_eq!(px(&s, 95, 10), [0, 0, 255, 255]);
    }

    #[test]
    fn heal_matches_the_surroundings() {
        // Grey 100 everywhere, a darker source patch at the left: healing with it keeps
        // the destination's tone (the correction lifts the patch).
        let mut rgba = vec![100u8; 120 * 60 * 4];
        for p in rgba.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        for y in 0..60 {
            for x in 0..40 {
                let i = (y * 120 + x) * 4;
                rgba[i..i + 3].copy_from_slice(&[40, 40, 40]);
            }
        }
        let mut s = Surface::from_rgba(120, 60, &rgba);
        let src = CloneSource {
            surface: s.clone(),
            origin: (0, 0),
            delta: (-70, 0),
            fixed: None,
        };
        let brush = Brush {
            size: 20.0,
            hardness: 1.0,
            mode: PaintMode::Heal,
            ..Brush::default()
        };
        let mut st = Stroke::with_source(brush, &s, src, (0, 0));
        st.line_to(&mut s, &mut DirtyTiles::default(), None, 90.0, 30.0);
        let v = px(&s, 90, 30)[0] as i32;
        assert!((v - 100).abs() <= 6, "{v}");
    }

    #[test]
    fn dodge_lightens_and_burn_darkens() {
        for (dodge, check) in [(true, true), (false, false)] {
            let mut s = Surface::new(40, 40);
            s.fill([100, 100, 100, 255]);
            let brush = Brush {
                size: 10.0,
                hardness: 1.0,
                rate: 0.5,
                mode: PaintMode::DodgeBurn(dodge, Range::Midtones),
                ..Brush::default()
            };
            stroke(&mut s, brush, &[(20.0, 20.0)]);
            let v = px(&s, 20, 20);
            assert_eq!(v[3], 255);
            assert_eq!(v[0] > 100, check, "{v:?}");
        }
    }

    #[test]
    fn smudge_drags_colour_and_blur_softens_edges() {
        let mut s = split(100, 20);
        let brush = Brush {
            size: 8.0,
            hardness: 1.0,
            rate: 0.9,
            mode: PaintMode::Smudge,
            ..Brush::default()
        };
        stroke(&mut s, brush, &[(40.0, 10.0), (70.0, 10.0)]);
        // Red was dragged into the blue half.
        assert!(px(&s, 60, 10)[0] > 60, "{:?}", px(&s, 60, 10));
        let mut b = split(100, 20);
        let blur = Brush {
            size: 10.0,
            hardness: 1.0,
            rate: 1.0,
            mode: PaintMode::Convolve(false),
            ..Brush::default()
        };
        stroke(&mut b, blur, &[(50.0, 10.0)]);
        let edge = px(&b, 50, 10);
        assert!(edge[0] > 0 && edge[2] > 0, "{edge:?}");
    }

    #[test]
    fn dab_outside_surface_is_ignored() {
        let mut s = Surface::new(10, 10);
        assert!(stroke(&mut s, Brush::default(), &[(-100.0, -100.0)]).is_empty());
    }
}
