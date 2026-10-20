//! The selection: an image-sized 8-bit coverage mask in 64x64 tiles (0 = not selected,
//! 255 = fully selected, values between come from antialiasing and feathering), plus the
//! shapes that build it (rectangle, ellipse, polygon, colour regions) and the operations
//! that apply it to pixels.
//!
//! Like `Surface`, tiles are shared through `Arc`, so undo snapshots are copy-on-write and
//! changed tiles are found by identity. Unallocated tiles are empty (0).

use std::sync::{Arc, LazyLock};

use crate::surface::{Surface, TILE_SIZE};

const T: usize = TILE_SIZE as usize;
pub const MASK_TILE_BYTES: usize = T * T;

pub type MaskTile = [u8; MASK_TILE_BYTES];

static EMPTY: MaskTile = [0; MASK_TILE_BYTES];
static FULL: LazyLock<Arc<MaskTile>> = LazyLock::new(|| Arc::new([255; MASK_TILE_BYTES]));

/// How a new shape combines with the current selection (GIMP's selection modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl SelectMode {
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Add,
            2 => Self::Subtract,
            3 => Self::Intersect,
            _ => Self::Replace,
        }
    }
}

/// A dense coverage buffer placed at `(x, y)` in image coordinates (it may extend past the
/// image; combining clips it).
#[derive(Clone, Debug)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

impl Region {
    fn empty() -> Self {
        Self {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            data: Vec::new(),
        }
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        let (rx, ry) = (x - self.x, y - self.y);
        if rx < 0 || ry < 0 || rx >= self.w as i32 || ry >= self.h as i32 {
            return 0;
        }
        self.data[ry as usize * self.w as usize + rx as usize]
    }
}

#[derive(Clone)]
pub struct Mask {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    tiles: Vec<Option<Arc<MaskTile>>>,
}

impl Mask {
    /// An empty mask.
    pub fn new(width: u32, height: u32) -> Self {
        let tiles_x = width.div_ceil(TILE_SIZE);
        let tiles_y = height.div_ceil(TILE_SIZE);
        Self {
            width,
            height,
            tiles_x,
            tiles_y,
            tiles: vec![None; (tiles_x * tiles_y) as usize],
        }
    }

    /// Everything selected; all tiles share one allocation.
    pub fn full(width: u32, height: u32) -> Self {
        let mut m = Self::new(width, height);
        m.tiles.fill(Some(FULL.clone()));
        m
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> u32 {
        self.tiles_y
    }

    fn index(&self, tx: u32, ty: u32) -> usize {
        (ty * self.tiles_x + tx) as usize
    }

    /// Pixel rectangle `(x, y, w, h)` covered by a tile, clipped to the mask.
    pub fn tile_rect(&self, tx: u32, ty: u32) -> (u32, u32, u32, u32) {
        let x = tx * TILE_SIZE;
        let y = ty * TILE_SIZE;
        (
            x,
            y,
            TILE_SIZE.min(self.width - x),
            TILE_SIZE.min(self.height - y),
        )
    }

    /// Tile values (`TILE_SIZE` x `TILE_SIZE`, row stride `TILE_SIZE`); values outside the
    /// mask in edge tiles are meaningless and must be ignored.
    pub fn tile(&self, tx: u32, ty: u32) -> &MaskTile {
        match &self.tiles[self.index(tx, ty)] {
            Some(t) => t,
            None => &EMPTY,
        }
    }

    fn tile_mut(&mut self, tx: u32, ty: u32) -> &mut MaskTile {
        let i = self.index(tx, ty);
        Arc::make_mut(self.tiles[i].get_or_insert_with(|| Arc::new([0; MASK_TILE_BYTES])))
    }

    /// Coverage at image pixel `(x, y)`; 0 outside the mask.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return 0;
        }
        let (x, y) = (x as u32, y as u32);
        match &self.tiles[self.index(x / TILE_SIZE, y / TILE_SIZE)] {
            Some(t) => t[((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize],
            None => 0,
        }
    }

    /// Whether every value of the tile inside the mask equals `v`.
    fn tile_is(&self, tx: u32, ty: u32, v: u8) -> bool {
        let Some(t) = &self.tiles[self.index(tx, ty)] else {
            return v == 0;
        };
        if Arc::ptr_eq(t, &FULL) {
            return v == 255;
        }
        let (_, _, w, h) = self.tile_rect(tx, ty);
        (0..h as usize).all(|r| t[r * T..r * T + w as usize].iter().all(|&p| p == v))
    }

    /// Drops tiles that are entirely empty (so they cost nothing and read as unselected).
    fn prune(&mut self) {
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                if self.tiles[self.index(tx, ty)].is_some() && self.tile_is(tx, ty, 0) {
                    let i = self.index(tx, ty);
                    self.tiles[i] = None;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        (0..self.tiles_y).all(|ty| (0..self.tiles_x).all(|tx| self.tile_is(tx, ty, 0)))
    }

    /// Bounding box `(x, y, w, h)` of the selected (non-zero) pixels, or `None` if empty.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let Some(t) = &self.tiles[self.index(tx, ty)] else {
                    continue;
                };
                let (bx, by, w, h) = self.tile_rect(tx, ty);
                if Arc::ptr_eq(t, &FULL) {
                    x0 = x0.min(bx);
                    y0 = y0.min(by);
                    x1 = x1.max(bx + w);
                    y1 = y1.max(by + h);
                    continue;
                }
                // Skip tiles that cannot extend the box found so far.
                if bx >= x0 && by >= y0 && bx + w <= x1 && by + h <= y1 {
                    continue;
                }
                for r in 0..h {
                    let row = &t[r as usize * T..r as usize * T + w as usize];
                    let Some(first) = row.iter().position(|&v| v != 0) else {
                        continue;
                    };
                    let last = row.iter().rposition(|&v| v != 0).unwrap();
                    x0 = x0.min(bx + first as u32);
                    x1 = x1.max(bx + last as u32 + 1);
                    y0 = y0.min(by + r);
                    y1 = y1.max(by + r + 1);
                }
            }
        }
        (x0 < x1).then(|| (x0, y0, x1 - x0, y1 - y0))
    }

    /// The mask as a dense `width * height` buffer.
    pub fn to_dense(&self) -> Vec<u8> {
        let w = self.width as usize;
        let mut out = vec![0u8; w * self.height as usize];
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let Some(t) = &self.tiles[self.index(tx, ty)] else {
                    continue;
                };
                let (x0, y0, tw, th) = self.tile_rect(tx, ty);
                for r in 0..th as usize {
                    let d = (y0 as usize + r) * w + x0 as usize;
                    out[d..d + tw as usize].copy_from_slice(&t[r * T..r * T + tw as usize]);
                }
            }
        }
        out
    }

    /// A mask from a dense `width * height` buffer (empty tiles are not allocated).
    pub fn from_dense(width: u32, height: u32, data: &[u8]) -> Self {
        let mut m = Self::new(width, height);
        let w = width as usize;
        for ty in 0..m.tiles_y {
            for tx in 0..m.tiles_x {
                let (x0, y0, tw, th) = m.tile_rect(tx, ty);
                let rows = (0..th as usize).map(|r| (y0 as usize + r) * w + x0 as usize);
                if rows
                    .clone()
                    .all(|d| data[d..d + tw as usize].iter().all(|&v| v == 0))
                {
                    continue;
                }
                let tile = m.tile_mut(tx, ty);
                for (r, d) in rows.enumerate() {
                    tile[r * T..r * T + tw as usize].copy_from_slice(&data[d..d + tw as usize]);
                }
            }
        }
        m
    }

    /// Inverts the selection (selected becomes unselected and vice versa).
    pub fn invert(&mut self) {
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let i = self.index(tx, ty);
                self.tiles[i] = match &self.tiles[i] {
                    None => Some(FULL.clone()),
                    Some(t) if Arc::ptr_eq(t, &FULL) => None,
                    Some(t) => {
                        let mut n: Box<MaskTile> = Box::new(**t);
                        n.iter_mut().for_each(|v| *v = 255 - *v);
                        Some(Arc::from(n))
                    }
                };
            }
        }
        self.prune();
    }

    /// Combines a shape with the selection.
    pub fn combine(&mut self, region: &Region, mode: SelectMode) {
        match mode {
            SelectMode::Replace => {
                self.tiles.fill(None);
                self.combine(region, SelectMode::Add);
            }
            SelectMode::Intersect => {
                let mut other = Mask::new(self.width, self.height);
                other.combine(region, SelectMode::Add);
                for i in 0..self.tiles.len() {
                    self.tiles[i] = match (&self.tiles[i], &other.tiles[i]) {
                        (None, _) | (_, None) => None,
                        (Some(a), Some(b)) if Arc::ptr_eq(b, &FULL) => Some(a.clone()),
                        (Some(a), Some(b)) if Arc::ptr_eq(a, &FULL) => Some(b.clone()),
                        (Some(a), Some(b)) => {
                            let mut n: Box<MaskTile> = Box::new(**a);
                            n.iter_mut()
                                .zip(b.iter())
                                .for_each(|(v, &o)| *v = (*v).min(o));
                            Some(Arc::from(n))
                        }
                    };
                }
                self.prune();
            }
            SelectMode::Add | SelectMode::Subtract => {
                let add = mode == SelectMode::Add;
                let x0 = region.x.max(0);
                let y0 = region.y.max(0);
                let x1 = (region.x + region.w as i32).min(self.width as i32);
                let y1 = (region.y + region.h as i32).min(self.height as i32);
                if x0 >= x1 || y0 >= y1 {
                    return;
                }
                let ts = TILE_SIZE as i32;
                for ty in y0 / ts..=(y1 - 1) / ts {
                    for tx in x0 / ts..=(x1 - 1) / ts {
                        let i = self.index(tx as u32, ty as u32);
                        if !add && self.tiles[i].is_none() {
                            continue;
                        }
                        let (bx, by) = (tx * ts, ty * ts);
                        let (cx0, cx1) = (x0.max(bx), x1.min(bx + ts));
                        let (cy0, cy1) = (y0.max(by), y1.min(by + ts));
                        // Leave the tile alone (and shared) when nothing would change.
                        let changes = (cy0..cy1).any(|y| {
                            (cx0..cx1).any(|x| {
                                let r = region.at(x, y);
                                let m = self.get(x as i64, y as i64);
                                if add { r > m } else { r > 0 && m > 255 - r }
                            })
                        });
                        if !changes {
                            continue;
                        }
                        let tile = self.tile_mut(tx as u32, ty as u32);
                        for y in cy0..cy1 {
                            for x in cx0..cx1 {
                                let r = region.at(x, y);
                                let v = &mut tile[((y - by) * ts + x - bx) as usize];
                                *v = if add { (*v).max(r) } else { (*v).min(255 - r) };
                            }
                        }
                    }
                }
                if !add {
                    self.prune();
                }
            }
        }
    }

    /// Indices of the tiles that differ from `other` by identity (same size required).
    pub fn diff_tiles(&self, other: &Mask) -> Vec<u32> {
        self.tiles
            .iter()
            .zip(&other.tiles)
            .enumerate()
            .filter(|(_, (a, b))| match (a, b) {
                (None, None) => false,
                (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                _ => true,
            })
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Tile bytes held by this mask that `other` does not share (undo memory estimate).
    pub fn unshared_bytes(&self, other: Option<&Mask>) -> usize {
        let count = match other {
            Some(o) if (o.width, o.height) == (self.width, self.height) => self
                .tiles
                .iter()
                .zip(&o.tiles)
                .filter(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
                    (Some(_), None) => true,
                    _ => false,
                })
                .count(),
            _ => self.tiles.iter().filter(|t| t.is_some()).count(),
        };
        count * MASK_TILE_BYTES
    }
}

// ---- shapes ---------------------------------------------------------------------------------

/// Sub-scanlines per pixel row for antialiased polygons.
const AA_ROWS: usize = 16;

/// Rasterises a closed polygon (even-odd rule) into a region clipped to `width` x `height`.
/// With `antialias`, coverage is exact horizontally and sampled on 16 sub-rows vertically;
/// without it a pixel is selected when its centre is inside.
pub fn polygon(points: &[(f64, f64)], antialias: bool, width: u32, height: u32) -> Region {
    if points.len() < 3 {
        return Region::empty();
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        minx = minx.min(x);
        miny = miny.min(y);
        maxx = maxx.max(x);
        maxy = maxy.max(y);
    }
    let x0 = (minx.floor() as i64).clamp(0, width as i64) as i32;
    let y0 = (miny.floor() as i64).clamp(0, height as i64) as i32;
    let x1 = (maxx.ceil() as i64).clamp(0, width as i64) as i32;
    let y1 = (maxy.ceil() as i64).clamp(0, height as i64) as i32;
    if x0 >= x1 || y0 >= y1 {
        return Region::empty();
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);

    // Edges (top y, bottom y, x at top, dx/dy), sorted by top y.
    let mut edges: Vec<(f64, f64, f64, f64)> = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let (ax, ay) = points[i];
        let (bx, by) = points[(i + 1) % points.len()];
        if ay == by {
            continue;
        }
        let (top, bot) = if ay < by {
            ((ax, ay), (bx, by))
        } else {
            ((bx, by), (ax, ay))
        };
        edges.push((top.1, bot.1, top.0, (bot.0 - top.0) / (bot.1 - top.1)));
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut data = vec![0u8; w * h];
    // Per-row coverage deltas (prefix sums give the coverage); one extra cell for spans
    // that end at the right edge.
    let mut acc = vec![0f32; w + 2];
    let mut xs: Vec<f64> = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    let (subs, weight) = if antialias {
        (AA_ROWS, 1.0 / AA_ROWS as f32)
    } else {
        (1, 1.0)
    };

    for row in 0..h {
        acc.iter_mut().for_each(|v| *v = 0.0);
        for s in 0..subs {
            let sy = if antialias {
                (y0 + row as i32) as f64 + (s as f64 + 0.5) / subs as f64
            } else {
                (y0 + row as i32) as f64 + 0.5
            };
            while next < edges.len() && edges[next].0 <= sy {
                active.push(next);
                next += 1;
            }
            active.retain(|&e| edges[e].1 > sy);
            xs.clear();
            for &e in &active {
                let (ty, _, tx, k) = edges[e];
                if ty <= sy {
                    xs.push(tx + (sy - ty) * k);
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.as_chunks::<2>().0 {
                let a = pair[0] - x0 as f64;
                let b = pair[1] - x0 as f64;
                if antialias {
                    add_span(&mut acc, a.max(0.0), b.min(w as f64), weight);
                } else {
                    // Pixels whose centre lies in [a, b).
                    let ia = ((a - 0.5).ceil().max(0.0) as usize).min(w);
                    let ib = ((b - 0.5).ceil().max(0.0) as usize).min(w);
                    if ia < ib {
                        acc[ia] += 1.0;
                        acc[ib] -= 1.0;
                    }
                }
            }
        }
        let mut cov = 0f32;
        let out = &mut data[row * w..row * w + w];
        for (x, v) in out.iter_mut().enumerate() {
            cov += acc[x];
            *v = (cov.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    Region {
        x: x0,
        y: y0,
        w: w as u32,
        h: h as u32,
        data,
    }
}

/// Adds coverage `weight` over `[a, b)` (pixel units) to the delta row, with fractional ends.
fn add_span(acc: &mut [f32], a: f64, b: f64, weight: f32) {
    if a >= b {
        return;
    }
    let (ia, ib) = (a.floor() as usize, b.floor() as usize);
    let (fa, fb) = ((a - ia as f64) as f32, (b - ib as f64) as f32);
    acc[ia] += weight * (1.0 - fa);
    acc[ia + 1] += weight * fa;
    acc[ib] -= weight * (1.0 - fb);
    acc[ib + 1] -= weight * fb;
}

/// Points of an ellipse inscribed in the rectangle, with segments short enough to look
/// smooth at any zoom.
fn ellipse_points(x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (x + rx, y + ry);
    let n = ((std::f64::consts::TAU * rx.max(ry)) / 2.0).clamp(16.0, 20000.0) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / n as f64 * std::f64::consts::TAU;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect()
}

/// A rectangle, optionally with rounded corners of `radius` pixels.
#[allow(clippy::too_many_arguments)]
pub fn rectangle(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    antialias: bool,
    iw: u32,
    ih: u32,
) -> Region {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.5 {
        return polygon(
            &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
            antialias,
            iw,
            ih,
        );
    }
    let n = ((std::f64::consts::FRAC_PI_2 * r) / 2.0).clamp(4.0, 2000.0) as usize;
    let mut pts = Vec::with_capacity(4 * (n + 1));
    // Corner centres, clockwise from the top-right, each with its start angle.
    let corners = [
        (x + w - r, y + r, -std::f64::consts::FRAC_PI_2),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, std::f64::consts::FRAC_PI_2),
        (x + r, y + r, std::f64::consts::PI),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=n {
            let t = start + std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
            pts.push((cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    polygon(&pts, antialias, iw, ih)
}

pub fn ellipse(x: f64, y: f64, w: f64, h: f64, antialias: bool, iw: u32, ih: u32) -> Region {
    if w <= 0.0 || h <= 0.0 {
        return Region::empty();
    }
    polygon(&ellipse_points(x, y, w, h), antialias, iw, ih)
}

/// Softens a region's edges with a Gaussian-like blur (three box blurs) that fades out at
/// about `radius` pixels (GIMP's "Feather edges").
pub fn feather(region: Region, radius: f32) -> Region {
    if radius < 0.5 || region.w == 0 {
        return region;
    }
    let sigma = radius / 3.33;
    let boxes = box_sizes(sigma);
    let pad = boxes.iter().map(|b| b / 2).sum::<usize>() + 1;
    let (w, h) = (region.w as usize + 2 * pad, region.h as usize + 2 * pad);
    let mut buf = vec![0f32; w * h];
    for y in 0..region.h as usize {
        for x in 0..region.w as usize {
            buf[(y + pad) * w + x + pad] = region.data[y * region.w as usize + x] as f32;
        }
    }
    let mut tmp = vec![0f32; w * h];
    for &b in &boxes {
        box_blur_h(&buf, &mut tmp, w, h, b / 2);
        box_blur_v(&tmp, &mut buf, w, h, b / 2);
    }
    Region {
        x: region.x - pad as i32,
        y: region.y - pad as i32,
        w: w as u32,
        h: h as u32,
        data: buf
            .iter()
            .map(|&v| (v + 0.5).clamp(0.0, 255.0) as u8)
            .collect(),
    }
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

fn box_blur_h(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        let out = &mut dst[y * w..y * w + w];
        let mut sum: f32 = row[..r.min(w)].iter().sum();
        for x in 0..w {
            if x + r < w {
                sum += row[x + r];
            }
            if x > r {
                sum -= row[x - r - 1];
            }
            out[x] = sum * norm;
        }
    }
}

fn box_blur_v(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut sum = vec![0f32; w];
    for y in 0..r.min(h) {
        for x in 0..w {
            sum[x] += src[y * w + x];
        }
    }
    for y in 0..h {
        if y + r < h {
            for x in 0..w {
                sum[x] += src[(y + r) * w + x];
            }
        }
        if y > r {
            for x in 0..w {
                sum[x] -= src[(y - r - 1) * w + x];
            }
        }
        for x in 0..w {
            dst[y * w + x] = sum[x] * norm;
        }
    }
}

// ---- colour regions (Fuzzy Select, Select by Color) ------------------------------------------

/// Which channel(s) colour similarity looks at (GIMP's "Select by").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    Composite,
    Red,
    Green,
    Blue,
    Alpha,
    Hue,
    Saturation,
    Value,
}

impl Criterion {
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Red,
            2 => Self::Green,
            3 => Self::Blue,
            4 => Self::Alpha,
            5 => Self::Hue,
            6 => Self::Saturation,
            7 => Self::Value,
            _ => Self::Composite,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ColorMatch {
    /// 0..=255: the largest difference that still counts as similar.
    pub threshold: f32,
    pub criterion: Criterion,
    /// Fully transparent pixels can be selected (otherwise they never are).
    pub select_transparent: bool,
    /// Pixels a little beyond the threshold get partial coverage.
    pub antialias: bool,
}

fn hsv(p: [u8; 4]) -> (f32, f32, f32) {
    let (r, g, b) = (
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

impl ColorMatch {
    /// Difference between the seed colour and `p` in 0..=255 units.
    fn difference(&self, seed: [u8; 4], p: [u8; 4]) -> f32 {
        let d = |c: usize| (seed[c] as f32 - p[c] as f32).abs();
        match self.criterion {
            // A transparent seed selects by alpha only: the colour of transparent pixels is
            // invisible and arbitrary.
            Criterion::Composite if self.select_transparent && seed[3] == 0 => d(3),
            Criterion::Composite => d(0).max(d(1)).max(d(2)).max(d(3)),
            Criterion::Red => d(0),
            Criterion::Green => d(1),
            Criterion::Blue => d(2),
            Criterion::Alpha => d(3),
            Criterion::Hue => {
                let dh = (hsv(seed).0 - hsv(p).0).abs();
                dh.min(1.0 - dh) * 2.0 * 255.0
            }
            Criterion::Saturation => (hsv(seed).1 - hsv(p).1).abs() * 255.0,
            Criterion::Value => (hsv(seed).2 - hsv(p).2).abs() * 255.0,
        }
    }

    /// Selection coverage (0..=255) of pixel `p` against the seed colour.
    pub fn coverage(&self, seed: [u8; 4], p: [u8; 4]) -> u8 {
        if !self.select_transparent && p[3] == 0 {
            return 0;
        }
        self.coverage_of(self.difference(seed, p))
    }

    /// The channels whose largest absolute difference is the criterion's difference, or
    /// `None` for the HSV criteria (see `difference`).
    fn channels(&self, seed: [u8; 4]) -> Option<&'static [usize]> {
        Some(match self.criterion {
            Criterion::Composite if self.select_transparent && seed[3] == 0 => &[3],
            Criterion::Composite => &[0, 1, 2, 3],
            Criterion::Red => &[0],
            Criterion::Green => &[1],
            Criterion::Blue => &[2],
            Criterion::Alpha => &[3],
            _ => return None,
        })
    }

    /// Coverage for a difference (0..=255 units).
    fn coverage_of(&self, diff: f32) -> u8 {
        if self.antialias && self.threshold > 0.0 {
            // Full up to the threshold, then a linear fade to 0 at 1.5x the threshold.
            let t = 1.5 - diff / self.threshold;
            if t <= 0.0 {
                0
            } else if t < 0.5 {
                (t * 2.0 * 255.0 + 0.5) as u8
            } else {
                255
            }
        } else if diff <= self.threshold {
            255
        } else {
            0
        }
    }
}

/// Pixels of a dense straight RGBA8 buffer similar to the colour at `(sx, sy)`: only those
/// connected to it (`contiguous`, Fuzzy Select) or all of them (Select by Color). Returns
/// one coverage byte per pixel.
pub fn color_region(
    rgba: &[u8],
    w: usize,
    h: usize,
    sx: usize,
    sy: usize,
    m: &ColorMatch,
    contiguous: bool,
) -> Vec<u8> {
    let pixels = &rgba.as_chunks::<4>().0[..w * h];
    let seed = pixels[sy * w + sx];
    // Coverage of every pixel, computed once; the flood fill then only compares bytes.
    let cov: Vec<u8> = match m.channels(seed) {
        // Integer differences: coverage from a lookup table.
        Some(ch) => {
            let lut: [u8; 256] = std::array::from_fn(|d| m.coverage_of(d as f32));
            // Channels not in the criterion compare against themselves (difference 0).
            let mut mask = [0u8; 4];
            for &c in ch {
                mask[c] = 255;
            }
            let skip_clear = !m.select_transparent;
            pixels
                .iter()
                .map(|p| {
                    let d = (seed[0].abs_diff(p[0]) & mask[0])
                        .max(seed[1].abs_diff(p[1]) & mask[1])
                        .max(seed[2].abs_diff(p[2]) & mask[2])
                        .max(seed[3].abs_diff(p[3]) & mask[3]);
                    if skip_clear && p[3] == 0 {
                        0
                    } else {
                        lut[d as usize]
                    }
                })
                .collect()
        }
        None => pixels.iter().map(|&p| m.coverage(seed, p)).collect(),
    };
    if !contiguous {
        return cov;
    }
    // Scanline flood fill through pixels with non-zero coverage. `out != 0` marks visited.
    let mut out = vec![0u8; w * h];
    let open = |out: &[u8], i: usize| out[i] == 0 && cov[i] != 0;
    let mut stack = vec![(sx, sy)];
    while let Some((x, y)) = stack.pop() {
        let row = y * w;
        if !open(&out, row + x) {
            continue;
        }
        let mut l = x;
        while l > 0 && open(&out, row + l - 1) {
            l -= 1;
        }
        let mut r = x;
        while r + 1 < w && open(&out, row + r + 1) {
            r += 1;
        }
        out[row + l..=row + r].copy_from_slice(&cov[row + l..=row + r]);
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= h {
                continue;
            }
            // Push one seed per run of unvisited candidates in the neighbouring row.
            let nrow = ny * w;
            let mut in_run = false;
            for nx in l..=r {
                let o = open(&out, nrow + nx);
                if o && !in_run {
                    stack.push((nx, ny));
                }
                in_run = o;
            }
        }
    }
    out
}

/// Builds a region from a coverage buffer placed at `(x, y)`.
pub fn region_from(x: i32, y: i32, w: u32, h: u32, data: Vec<u8>) -> Region {
    Region { x, y, w, h, data }
}

// ---- applying the mask to pixels ------------------------------------------------------------

/// Scales the alpha of `surface` (placed at `pos` in image coordinates) by the mask:
/// `keep` keeps the selected part (alpha * m), otherwise the selected part is erased
/// (alpha * (1 - m)). Tiles that do not change stay shared.
pub fn apply_mask(surface: &mut Surface, pos: (i32, i32), mask: &Mask, keep: bool) {
    let scale = |a: u8, m: u8| -> u8 {
        let k = if keep { m } else { 255 - m } as u32;
        let t = a as u32 * k + 128;
        ((t + (t >> 8)) >> 8) as u8
    };
    for ty in 0..surface.tiles_y() {
        for tx in 0..surface.tiles_x() {
            if !surface.is_allocated(tx, ty) {
                continue;
            }
            let (x0, y0, w, h) = surface.tile_rect(tx, ty);
            let at = |x: u32, y: u32| {
                mask.get(
                    pos.0 as i64 + (x0 + x) as i64,
                    pos.1 as i64 + (y0 + y) as i64,
                )
            };
            let tile = surface.tile(tx, ty);
            let changes = (0..h).any(|y| {
                (0..w).any(|x| {
                    let a = tile[((y * TILE_SIZE + x) * 4 + 3) as usize];
                    a != 0 && scale(a, at(x, y)) != a
                })
            });
            if !changes {
                continue;
            }
            let tile = surface.tile_mut(tx, ty);
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * TILE_SIZE + x) * 4 + 3) as usize;
                    tile[i] = scale(tile[i], at(x, y));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region_sum(r: &Region) -> u64 {
        r.data.iter().map(|&v| v as u64).sum()
    }

    #[test]
    fn rectangle_without_antialias_is_exact() {
        let r = rectangle(2.0, 3.0, 4.0, 5.0, 0.0, false, 20, 20);
        assert_eq!((r.x, r.y, r.w, r.h), (2, 3, 4, 5));
        assert!(r.data.iter().all(|&v| v == 255));
    }

    #[test]
    fn antialiased_polygon_has_partial_edges_and_correct_area() {
        // A half-pixel offset square: edges are 50% covered.
        let r = rectangle(1.5, 1.5, 4.0, 4.0, 0.0, true, 20, 20);
        assert_eq!(r.at(1, 3), 128);
        assert_eq!(r.at(3, 3), 255);
        // Total area 16 px.
        let area = region_sum(&r) as f64 / 255.0;
        assert!((area - 16.0).abs() < 0.1, "{area}");
    }

    #[test]
    fn ellipse_area_is_close_to_pi_r_squared() {
        let r = ellipse(0.0, 0.0, 100.0, 100.0, true, 200, 200);
        let area = region_sum(&r) as f64 / 255.0;
        assert!(
            (area - std::f64::consts::PI * 2500.0).abs() < 10.0,
            "{area}"
        );
    }

    #[test]
    fn modes_combine() {
        let mut m = Mask::new(100, 100);
        m.combine(
            &rectangle(0.0, 0.0, 50.0, 50.0, 0.0, false, 100, 100),
            SelectMode::Replace,
        );
        m.combine(
            &rectangle(25.0, 0.0, 50.0, 50.0, 0.0, false, 100, 100),
            SelectMode::Add,
        );
        assert_eq!(m.bounds(), Some((0, 0, 75, 50)));
        m.combine(
            &rectangle(0.0, 0.0, 10.0, 50.0, 0.0, false, 100, 100),
            SelectMode::Subtract,
        );
        assert_eq!(m.bounds(), Some((10, 0, 65, 50)));
        m.combine(
            &rectangle(60.0, 10.0, 40.0, 10.0, 0.0, false, 100, 100),
            SelectMode::Intersect,
        );
        assert_eq!(m.bounds(), Some((60, 10, 15, 10)));
        m.combine(
            &rectangle(0.0, 0.0, 100.0, 100.0, 0.0, false, 100, 100),
            SelectMode::Subtract,
        );
        assert!(m.is_empty());
        assert!(m.tiles.iter().all(Option::is_none));
    }

    #[test]
    fn invert_and_full() {
        let mut m = Mask::full(70, 70);
        assert_eq!(m.bounds(), Some((0, 0, 70, 70)));
        m.invert();
        assert!(m.is_empty());
        m.combine(
            &rectangle(0.0, 0.0, 10.0, 10.0, 0.0, false, 70, 70),
            SelectMode::Add,
        );
        m.invert();
        assert_eq!(m.get(5, 5), 0);
        assert_eq!(m.get(69, 69), 255);
        assert_eq!(m.bounds(), Some((0, 0, 70, 70)));
    }

    #[test]
    fn fuzzy_is_contiguous_and_by_color_is_not() {
        // 5x1: red, red, blue, red, red.
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let rgba: Vec<u8> = [red, red, blue, red, red].concat();
        let m = ColorMatch {
            threshold: 15.0,
            criterion: Criterion::Composite,
            select_transparent: true,
            antialias: false,
        };
        assert_eq!(
            color_region(&rgba, 5, 1, 0, 0, &m, true),
            vec![255, 255, 0, 0, 0]
        );
        assert_eq!(
            color_region(&rgba, 5, 1, 0, 0, &m, false),
            vec![255, 255, 0, 255, 255]
        );
    }

    #[test]
    fn fuzzy_flood_fills_around_obstacles() {
        // 4x3 with a wall in the middle column except the bottom row.
        let o = [0, 0, 0, 255];
        let x = [255, 255, 255, 255];
        let rows = [[o, x, o, o], [o, x, o, o], [o, o, o, o]];
        let rgba: Vec<u8> = rows.iter().flat_map(|r| r.concat()).collect();
        let m = ColorMatch {
            threshold: 0.0,
            criterion: Criterion::Composite,
            select_transparent: true,
            antialias: true,
        };
        let out = color_region(&rgba, 4, 3, 3, 0, &m, true);
        assert_eq!(
            out,
            vec![255, 0, 255, 255, 255, 0, 255, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn transparent_pixels_follow_the_option() {
        let m = ColorMatch {
            threshold: 15.0,
            criterion: Criterion::Composite,
            select_transparent: false,
            antialias: false,
        };
        assert_eq!(m.coverage([0, 0, 0, 0], [0, 0, 0, 0]), 0);
        let m = ColorMatch {
            select_transparent: true,
            ..m
        };
        // A transparent seed matches transparent pixels of any colour.
        assert_eq!(m.coverage([0, 0, 0, 0], [200, 9, 9, 0]), 255);
    }

    #[test]
    fn feather_softens_and_keeps_the_middle() {
        let r = rectangle(20.0, 20.0, 40.0, 40.0, 0.0, false, 100, 100);
        let f = feather(r, 10.0);
        assert_eq!(f.at(40, 40), 255);
        let edge = f.at(20, 40);
        assert!(edge > 90 && edge < 165, "{edge}");
        assert!(f.at(12, 40) < 20);
    }

    #[test]
    fn apply_mask_erases_and_keeps() {
        let mut s = Surface::new(10, 10);
        s.fill([9, 9, 9, 255]);
        let mut m = Mask::new(10, 10);
        m.combine(
            &rectangle(0.0, 0.0, 5.0, 10.0, 0.0, false, 10, 10),
            SelectMode::Replace,
        );
        let mut erased = s.clone();
        apply_mask(&mut erased, (0, 0), &m, false);
        assert_eq!(erased.pixel(2, 2).unwrap()[3], 0);
        assert_eq!(erased.pixel(7, 2).unwrap()[3], 255);
        apply_mask(&mut s, (0, 0), &m, true);
        assert_eq!(s.pixel(2, 2).unwrap()[3], 255);
        assert_eq!(s.pixel(7, 2).unwrap()[3], 0);
    }
}
