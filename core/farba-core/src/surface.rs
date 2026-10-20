//! Tiled RGBA8 pixel storage.
//!
//! Pixels are straight (non-premultiplied) sRGB RGBA8. A surface is split into
//! `TILE_SIZE` x `TILE_SIZE` tiles; fully transparent tiles are not allocated. Tiles are
//! shared through `Arc` so undo snapshots are copy-on-write: cloning a surface is cheap and
//! only tiles that are written afterwards get copied.

use std::sync::Arc;

pub const TILE_SIZE: u32 = 64;
pub const TILE_BYTES: usize = (TILE_SIZE * TILE_SIZE * 4) as usize;

pub type Tile = [u8; TILE_BYTES];

static EMPTY_TILE: Tile = [0; TILE_BYTES];

#[derive(Clone)]
pub struct Surface {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    tiles: Vec<Option<Arc<Tile>>>,
}

impl Surface {
    /// Creates a fully transparent surface.
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

    /// Creates a surface from a tightly packed straight RGBA8 buffer.
    ///
    /// # Panics
    /// If `rgba.len() != width * height * 4`.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        assert_eq!(rgba.len(), (width as usize) * (height as usize) * 4);
        let mut surface = Self::new(width, height);
        let row_bytes = width as usize * 4;
        for ty in 0..surface.tiles_y {
            for tx in 0..surface.tiles_x {
                let (x0, y0, w, h) = surface.tile_rect(tx, ty);
                let mut tile: Box<Tile> = Box::new([0; TILE_BYTES]);
                let mut any_opaque = false;
                for row in 0..h {
                    let src = (y0 + row) as usize * row_bytes + x0 as usize * 4;
                    let src = &rgba[src..src + w as usize * 4];
                    let dst = row as usize * TILE_SIZE as usize * 4;
                    tile[dst..dst + src.len()].copy_from_slice(src);
                    any_opaque |= src.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
                }
                if any_opaque {
                    surface.tiles[(ty * surface.tiles_x + tx) as usize] = Some(Arc::from(tile));
                }
            }
        }
        surface
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

    pub fn tile_index(&self, tx: u32, ty: u32) -> usize {
        (ty * self.tiles_x + tx) as usize
    }

    /// Pixel rectangle `(x, y, w, h)` covered by a tile, clipped to the surface.
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

    /// Tile pixels (always `TILE_SIZE` x `TILE_SIZE`, row stride `TILE_SIZE * 4`); an
    /// unallocated tile reads as transparent.
    pub fn tile(&self, tx: u32, ty: u32) -> &Tile {
        match &self.tiles[self.tile_index(tx, ty)] {
            Some(t) => t,
            None => &EMPTY_TILE,
        }
    }

    /// Mutable tile pixels; allocates the tile, or copies it if it is shared.
    pub fn tile_mut(&mut self, tx: u32, ty: u32) -> &mut Tile {
        let index = self.tile_index(tx, ty);
        let slot = self.tiles[index].get_or_insert_with(|| Arc::new([0; TILE_BYTES]));
        Arc::make_mut(slot)
    }

    /// The pixel at `(x, y)`, or `None` outside the surface.
    pub fn pixel(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let (x, y) = (x as u32, y as u32);
        let tile = self.tile(x / TILE_SIZE, y / TILE_SIZE);
        let i = (((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4) as usize;
        Some([tile[i], tile[i + 1], tile[i + 2], tile[i + 3]])
    }

    /// Whether a tile holds pixel data (an unallocated tile is fully transparent).
    pub fn is_allocated(&self, tx: u32, ty: u32) -> bool {
        self.tiles[self.tile_index(tx, ty)].is_some()
    }

    /// Fills the whole surface with one straight RGBA8 colour. All tiles share one
    /// allocation until they are written.
    pub fn fill(&mut self, color: [u8; 4]) {
        if color[3] == 0 {
            self.tiles.fill(None);
            return;
        }
        let mut tile: Box<Tile> = Box::new([0; TILE_BYTES]);
        for p in tile.as_chunks_mut::<4>().0 {
            *p = color;
        }
        let tile: Arc<Tile> = Arc::from(tile);
        self.tiles.fill(Some(tile));
    }

    /// Indices of the tiles that differ from `other` by identity (not shared through the
    /// same `Arc`). Shared tiles are immutable, so a shared tile is unchanged. Both surfaces
    /// must have the same size.
    /// Shares `other`'s tiles wherever this surface (of the same size) has the same pixels,
    /// so pixels computed elsewhere (the native backend) only count, upload and use history
    /// memory for the tiles that really changed.
    pub fn share_equal_tiles(&mut self, other: &Surface) {
        if (self.width, self.height) != (other.width, other.height) {
            return;
        }
        for (a, b) in self.tiles.iter_mut().zip(&other.tiles) {
            if let (Some(x), Some(y)) = (&*a, b)
                && !Arc::ptr_eq(x, y)
                && x[..] == y[..]
            {
                *a = Some(y.clone());
            }
        }
    }

    pub fn diff_tiles(&self, other: &Surface) -> Vec<u32> {
        debug_assert_eq!((self.width, self.height), (other.width, other.height));
        self.tiles
            .iter()
            .zip(&other.tiles)
            .enumerate()
            .filter(|(_, (a, b))| !same_tile(a, b))
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// Bytes of tile data held by this surface that `other` does not share (all of it when
    /// `other` is `None` or has a different size). Used for the undo memory estimate.
    pub fn unshared_bytes(&self, other: Option<&Surface>) -> usize {
        let count = match other {
            Some(o) if (o.width, o.height) == (self.width, self.height) => self
                .tiles
                .iter()
                .zip(&o.tiles)
                .filter(|(a, b)| a.is_some() && !same_tile(a, b))
                .count(),
            _ => self.tiles.iter().filter(|t| t.is_some()).count(),
        };
        count * TILE_BYTES
    }

    /// Copies the rectangle `(x, y, w, h)` (surface coordinates, may reach outside the
    /// surface, which reads as transparent) into a tightly packed RGBA8 buffer.
    pub fn read_rect(&self, x: i64, y: i64, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0u8; w as usize * h as usize * 4];
        let ts = TILE_SIZE as i64;
        for row in 0..h as i64 {
            let sy = y + row;
            if sy < 0 || sy >= self.height as i64 {
                continue;
            }
            let mut sx = x.max(0);
            let end = (x + w as i64).min(self.width as i64);
            while sx < end {
                let tx = sx / ts;
                let run = (end.min((tx + 1) * ts) - sx) as usize;
                let tile = self.tile(tx as u32, (sy / ts) as u32);
                let si = (((sy % ts) * ts + sx % ts) * 4) as usize;
                let di = ((row * w as i64 + sx - x) * 4) as usize;
                out[di..di + run * 4].copy_from_slice(&tile[si..si + run * 4]);
                sx += run as i64;
            }
        }
        out
    }

    /// Copies the whole surface into a tightly packed RGBA8 buffer.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.width as usize * self.height as usize * 4];
        self.write_rgba(&mut out);
        out
    }

    /// Writes the pixels as dense straight RGBA into `out` (`width * height * 4` bytes).
    pub fn write_rgba(&self, out: &mut [u8]) {
        let row_bytes = self.width as usize * 4;
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let (x0, y0, w, h) = self.tile_rect(tx, ty);
                let tile = self.tile(tx, ty);
                for row in 0..h {
                    let src = row as usize * TILE_SIZE as usize * 4;
                    let dst = (y0 + row) as usize * row_bytes + x0 as usize * 4;
                    out[dst..dst + w as usize * 4]
                        .copy_from_slice(&tile[src..src + w as usize * 4]);
                }
            }
        }
    }
}

fn same_tile(a: &Option<Arc<Tile>>, b: &Option<Arc<Tile>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_round_trip_with_partial_tiles() {
        let (w, h) = (130, 70);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8 | 1).collect();
        let s = Surface::from_rgba(w, h, &rgba);
        assert_eq!((s.tiles_x(), s.tiles_y()), (3, 2));
        assert_eq!(s.tile_rect(2, 1), (128, 64, 2, 6));
        assert_eq!(s.to_rgba(), rgba);
    }

    #[test]
    fn transparent_tiles_are_not_allocated() {
        let s = Surface::from_rgba(64, 64, &[0; 64 * 64 * 4]);
        assert!(s.tiles[0].is_none());
    }

    #[test]
    fn clone_is_copy_on_write() {
        let mut a = Surface::new(64, 64);
        a.tile_mut(0, 0)[0] = 7;
        let b = a.clone();
        a.tile_mut(0, 0)[0] = 9;
        assert_eq!(b.tile(0, 0)[0], 7);
        assert_eq!(a.tile(0, 0)[0], 9);
    }

    #[test]
    fn diff_and_unshared_bytes_follow_identity() {
        let mut a = Surface::new(128, 64);
        a.fill([1, 2, 3, 255]);
        let b = a.clone();
        assert!(a.diff_tiles(&b).is_empty());
        assert_eq!(a.unshared_bytes(Some(&b)), 0);
        a.tile_mut(1, 0)[0] = 9;
        assert_eq!(a.diff_tiles(&b), vec![1]);
        assert_eq!(a.unshared_bytes(Some(&b)), TILE_BYTES);
        assert_eq!(a.unshared_bytes(None), 2 * TILE_BYTES);
    }
}
