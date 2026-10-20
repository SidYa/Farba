//! A document: the layer stack, the active layer, the undo history, and a log of changed
//! tiles that the frontend uses to keep its GPU textures in sync.
//!
//! Every change goes through [`Document::edit`] (or a brush stroke), which records an undo
//! step and works out which tiles changed by comparing tile identities with the previous
//! state, so individual operations do not track dirty tiles themselves.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::brush::{Brush, CloneSource, DirtyTiles, Stroke};
use crate::color::{Rgba8, blend_over};
use crate::composite::{composite_over, thumbnail};
use crate::fill::Gradient;
use crate::filters::{self, Filter};
use crate::history::{History, State};
use crate::layer::{Layer, LayerProps, TextData, TransformSource};
use crate::projective::{self, Matrix, Rendered};
use crate::selection::{self, ColorMatch, Mask, Region, SelectMode, apply_mask};
use crate::surface::Surface;
use crate::transform::{self, Interpolation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocError(pub String);

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DocError {}

pub type Result<T> = std::result::Result<T, DocError>;

fn err<T>(msg: &str) -> Result<T> {
    Err(DocError(msg.to_string()))
}

/// `(x, y, width, height, rgba)` of copied pixels in image coordinates.
pub type CopiedPixels = (i32, i32, u32, u32, Vec<u8>);

/// Tiles of one layer that must be re-uploaded: `None` = the whole layer (new or resized).
#[derive(Debug, PartialEq, Eq)]
pub struct LayerChange {
    pub layer: u32,
    pub tiles: Option<Vec<u32>>,
}

/// A transform tool at work: the pixels being transformed ("float") and where they were.
/// Text being edited: the layer and the document before the first change (the edit is one
/// undo step).
struct TextSession {
    layer: u32,
    before: State,
}

struct TransformSession {
    layer: u32,
    before: State,
    float: Surface,
    pos: (i32, i32),
    /// The selection at the start: only the selected part moves, and the selection with it.
    mask: Option<Mask>,
}

/// What a transform renders (see [`Document::transform_job`]).
pub struct TransformJob {
    pub source: Surface,
    pub matrix: Matrix,
    pub interp: Interpolation,
    /// The selection at the start, rendered through the user's matrix when present.
    pub mask: Option<Mask>,
}

struct TransformPlan {
    index: usize,
    orig: Layer,
    source: Surface,
    matrix: Matrix,
}

struct FilterSession {
    layer: u32,
    before: State,
    /// Tells results computed for an earlier session apart (see `filter_show`).
    serial: u32,
}

struct ActiveStroke {
    stroke: Stroke,
    layer: u32,
    label: String,
    before: State,
    dirty: DirtyTiles,
    painted: bool,
}

pub struct Document {
    width: u32,
    height: u32,
    /// Bottom to top.
    layers: Vec<Layer>,
    active: u32,
    next_id: u32,
    history: History,
    changes: BTreeMap<u32, Option<BTreeSet<u32>>>,
    stroke: Option<ActiveStroke>,
    /// An adjustment being previewed on the active layer (see `filter_preview`).
    filter: Option<FilterSession>,
    /// The serial of the last filter session started.
    filter_serial: u32,
    transform: Option<TransformSession>,
    text_edit: Option<TextSession>,
    /// `None` = nothing selected (operations apply to everything).
    selection: Option<Mask>,
    /// Selection tiles changed since the last `take_selection_changes`.
    sel_changes: SelChanges,
    /// Cached `selection_bounds` (outer `None` = not computed yet).
    sel_bounds: Option<Option<(u32, u32, u32, u32)>>,
}

/// A layer of a document opened from a layered file (see [`Document::from_layers`]).
pub struct LoadedLayer {
    pub name: String,
    pub surface: Surface,
    pub x: i32,
    pub y: i32,
    pub opacity: f32,
    pub visible: bool,
    pub lock_alpha: bool,
    pub lock_position: bool,
    /// A text layer's description (rendered to exactly `surface`).
    pub text: Option<String>,
    pub name_from_text: bool,
    /// What a transformed layer was rendered from: original pixels and the matrix that maps
    /// them to image coordinates (rendered to exactly `surface` at `(x, y)`).
    pub source: Option<(Surface, Matrix)>,
}

/// Where Clone / Heal strokes take their pixels from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceSpec {
    /// The source layer, or `None` for the visible layers merged ("Sample merged").
    pub layer: Option<u32>,
    /// Image offset from a painted pixel to its source pixel.
    pub delta: (i32, i32),
    /// "Fixed" alignment: always the pixels around this image point.
    pub fixed: Option<(f32, f32)>,
}

/// Which layers Canvas Size resizes to the new canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeLayers {
    None,
    /// Layers that covered exactly the old image.
    ImageSized,
    All,
}

/// Whether two surfaces hold the same pixels (same size and shared tiles).
/// The size of a `layer`-sized layer after scaling an `image` to `to` (Scale Image).
pub fn scaled_layer_size(layer: (u32, u32), image: (u32, u32), to: (u32, u32)) -> (u32, u32) {
    let sx = to.0 as f64 / image.0 as f64;
    let sy = to.1 as f64 / image.1 as f64;
    (
        ((layer.0 as f64 * sx).round() as u32).max(1),
        ((layer.1 as f64 * sy).round() as u32).max(1),
    )
}

fn same_pixels(a: &Surface, b: &Surface) -> bool {
    (a.width(), a.height()) == (b.width(), b.height()) && a.diff_tiles(b).is_empty()
}

/// Mirrors a surface in place.
fn flip_surface(s: &mut Surface, horizontal: bool) {
    let (w, h) = (s.width(), s.height());
    let out = transform::flip(&s.to_rgba(), w as usize, h as usize, 4, horizontal);
    *s = Surface::from_rgba(w, h, &out);
}

/// Selection tiles that the frontend must re-upload.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum SelChanges {
    #[default]
    None,
    Tiles(BTreeSet<u32>),
    /// The whole mask (new, removed or resized).
    All,
}

impl Document {
    /// A new image with one "Background" layer filled with `fill`.
    pub fn new(width: u32, height: u32, fill: Rgba8) -> Self {
        let mut surface = Surface::new(width, height);
        surface.fill(fill.to_array());
        Self::with_layer(surface, "Background")
    }

    /// A document whose single layer is `surface` (an opened file, a pasted image).
    pub fn with_layer(surface: Surface, name: &str) -> Self {
        let mut doc = Self {
            width: surface.width(),
            height: surface.height(),
            layers: Vec::new(),
            active: 1,
            next_id: 2,
            history: History::default(),
            changes: BTreeMap::new(),
            stroke: None,
            filter: None,
            filter_serial: 0,
            transform: None,
            text_edit: None,
            selection: None,
            sel_changes: SelChanges::None,
            sel_bounds: None,
        };
        doc.layers.push(Layer::new(1, name, surface));
        doc
    }

    /// A document from saved layers (bottom to top), with no undo history and nothing
    /// unsaved. `active` is an index into `layers`; `selection` must be image-sized.
    pub fn from_layers(
        width: u32,
        height: u32,
        layers: Vec<LoadedLayer>,
        active: usize,
        selection: Option<Mask>,
    ) -> Result<Self> {
        if width == 0 || height == 0 || layers.is_empty() {
            return err("The image has no layers");
        }
        if selection
            .as_ref()
            .is_some_and(|m| (m.width(), m.height()) != (width, height))
        {
            return err("The selection does not match the image size");
        }
        let mut doc = Self::with_layer(Surface::new(1, 1), "");
        doc.width = width;
        doc.height = height;
        doc.layers = layers
            .into_iter()
            .zip(1..)
            .map(|(l, id)| {
                let mut layer = Layer::new(id, l.name, l.surface);
                layer.x = l.x;
                layer.y = l.y;
                layer.opacity = l.opacity.clamp(0.0, 1.0);
                layer.visible = l.visible;
                layer.lock_alpha = l.lock_alpha;
                layer.lock_position = l.lock_position;
                layer.text = l.text.map(|spec| {
                    Arc::new(TextData {
                        spec,
                        rendered: layer.surface.clone(),
                    })
                });
                layer.name_from_text = layer.text.is_some() && l.name_from_text;
                layer.source = l.source.map(|(original, matrix)| {
                    Arc::new(TransformSource {
                        original,
                        matrix,
                        rendered: layer.surface.clone(),
                        x: layer.x,
                        y: layer.y,
                    })
                });
                layer
            })
            .collect();
        doc.next_id = doc.layers.len() as u32 + 1;
        doc.active = active.min(doc.layers.len() - 1) as u32 + 1;
        doc.selection = selection.filter(|m| !m.is_empty());
        Ok(doc)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Bottom to top.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, id: u32) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    fn index_of(&self, id: u32) -> Result<usize> {
        match self.layers.iter().position(|l| l.id == id) {
            Some(i) => Ok(i),
            None => err("The layer no longer exists"),
        }
    }

    pub fn active(&self) -> u32 {
        self.active
    }

    /// Selects the active layer (not an undo step).
    pub fn set_active(&mut self, id: u32) -> Result<()> {
        self.index_of(id)?;
        self.active = id;
        Ok(())
    }

    // ---- history ------------------------------------------------------------------------

    fn state(&self) -> State {
        State {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            selection: self.selection.clone(),
        }
    }

    fn restore(&mut self, s: State) {
        let old = State {
            width: self.width,
            height: self.height,
            layers: std::mem::replace(&mut self.layers, s.layers),
            active: self.active,
            selection: std::mem::replace(&mut self.selection, s.selection),
        };
        self.width = s.width;
        self.height = s.height;
        self.active = s.active;
        self.record_diff(&old);
    }

    /// Records the layer and selection tiles that differ between `old` and now.
    fn record_diff(&mut self, old: &State) {
        let sel = match (&old.selection, &self.selection) {
            (None, None) => SelChanges::None,
            (Some(a), Some(b)) if (a.width(), a.height()) == (b.width(), b.height()) => {
                let tiles = b.diff_tiles(a);
                if tiles.is_empty() {
                    SelChanges::None
                } else {
                    SelChanges::Tiles(tiles.into_iter().collect())
                }
            }
            _ => SelChanges::All,
        };
        if sel != SelChanges::None {
            self.sel_bounds = None;
            self.sel_changes = match (std::mem::take(&mut self.sel_changes), sel) {
                (SelChanges::All, _) | (_, SelChanges::All) => SelChanges::All,
                (SelChanges::Tiles(mut a), SelChanges::Tiles(b)) => {
                    a.extend(b);
                    SelChanges::Tiles(a)
                }
                (_, s) => s,
            };
        }
        let old = &old.layers;
        let mut found = Vec::new();
        for l in &self.layers {
            match old.iter().find(|o| o.id == l.id) {
                Some(o) if (o.width(), o.height()) == (l.width(), l.height()) => {
                    let tiles = l.surface.diff_tiles(&o.surface);
                    if !tiles.is_empty() {
                        found.push((l.id, Some(tiles)));
                    }
                }
                _ => found.push((l.id, None)),
            }
        }
        for (id, tiles) in found {
            self.mark_changed(id, tiles);
        }
    }

    fn mark_changed(&mut self, id: u32, tiles: Option<Vec<u32>>) {
        match tiles {
            None => {
                self.changes.insert(id, None);
            }
            Some(tiles) => {
                if let Some(set) = self.changes.entry(id).or_insert(Some(BTreeSet::new())) {
                    set.extend(tiles);
                }
            }
        }
    }

    /// Tiles changed since the last call, per layer (layers that no longer exist are
    /// skipped; the frontend drops their textures when it syncs the layer list).
    pub fn take_changes(&mut self) -> Vec<LayerChange> {
        std::mem::take(&mut self.changes)
            .into_iter()
            .filter(|(id, _)| self.layers.iter().any(|l| l.id == *id))
            .map(|(layer, tiles)| LayerChange {
                layer,
                tiles: tiles.map(|t| t.into_iter().collect()),
            })
            .collect()
    }

    /// Runs one undoable change. If `f` fails, the document is restored and nothing is
    /// recorded.
    fn edit<R>(&mut self, label: &str, f: impl FnOnce(&mut Self) -> Result<R>) -> Result<R> {
        self.settle();
        let before = self.state();
        match f(self) {
            Ok(r) => {
                self.drop_stale_sources();
                self.record_diff(&before);
                self.history
                    .push(label, before, &self.layers, self.selection.as_ref());
                Ok(r)
            }
            Err(e) => {
                self.restore(before);
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        self.settle();
        match self.history.undo(self.state()) {
            Some(s) => {
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        self.settle();
        match self.history.redo(self.state()) {
            Some(s) => {
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    pub fn history(&mut self) -> &mut History {
        &mut self.history
    }

    // ---- layer operations -----------------------------------------------------------------

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn name_taken(&self, name: &str) -> bool {
        self.layers.iter().any(|l| l.name == name)
    }

    /// "Layer 2", "Layer 3", ... (the first number not in use).
    fn numbered_name(&self, base: &str) -> String {
        (self.layers.len() + 1..)
            .map(|n| format!("{base} {n}"))
            .find(|n| !self.name_taken(n))
            .unwrap_or_default()
    }

    /// `name`, or `name #1`, `name #2`, ... if it is taken (GIMP's scheme).
    fn unique_name(&self, name: &str) -> String {
        if !self.name_taken(name) {
            return name.to_string();
        }
        (1..)
            .map(|n| format!("{name} #{n}"))
            .find(|n| !self.name_taken(n))
            .unwrap_or_default()
    }

    fn active_index(&self) -> usize {
        self.index_of(self.active).unwrap_or(self.layers.len() - 1)
    }

    /// Adds an image-sized layer filled with `fill` above the active one and makes it active.
    pub fn add_layer(&mut self, fill: Rgba8) -> Result<u32> {
        self.edit("New Layer", |d| {
            let id = d.alloc_id();
            let mut surface = Surface::new(d.width, d.height);
            surface.fill(fill.to_array());
            let layer = Layer::new(id, d.numbered_name("Layer"), surface);
            let at = d.active_index() + 1;
            d.layers.insert(at, layer);
            d.active = id;
            Ok(id)
        })
    }

    /// Adds `surface` as a new layer at `(x, y)` above the active one (paste, import).
    pub fn add_layer_from(
        &mut self,
        label: &str,
        name: &str,
        surface: Surface,
        x: i32,
        y: i32,
    ) -> Result<u32> {
        self.edit(label, |d| {
            let id = d.alloc_id();
            let mut layer = Layer::new(id, d.unique_name(name), surface);
            layer.x = x;
            layer.y = y;
            let at = d.active_index() + 1;
            d.layers.insert(at, layer);
            d.active = id;
            Ok(id)
        })
    }

    pub fn delete_layers(&mut self, ids: &[u32]) -> Result<()> {
        let mut indices = ids
            .iter()
            .map(|&id| self.index_of(id))
            .collect::<Result<Vec<_>>>()?;
        indices.sort_unstable();
        indices.dedup();
        if indices.is_empty() {
            return Ok(());
        }
        if indices.len() >= self.layers.len() {
            return err("An image must keep at least one layer");
        }
        let label = if indices.len() == 1 {
            "Delete Layer"
        } else {
            "Delete Layers"
        };
        self.edit(label, |d| {
            let lowest = indices[0];
            for &i in indices.iter().rev() {
                d.layers.remove(i);
            }
            if d.index_of(d.active).is_err() {
                // The layer that was below the deleted ones (or the new bottom layer).
                d.active = d.layers[lowest.saturating_sub(1).min(d.layers.len() - 1)].id;
            }
            Ok(())
        })
    }

    /// Duplicates a layer above itself ("Name copy") and makes the copy active.
    pub fn duplicate_layer(&mut self, id: u32) -> Result<u32> {
        let i = self.index_of(id)?;
        self.edit("Duplicate Layer", |d| {
            let mut copy = d.layers[i].clone();
            copy.id = d.alloc_id();
            copy.name = d.unique_name(&format!("{} copy", copy.name));
            copy.name_from_text = false;
            let new_id = copy.id;
            d.layers.insert(i + 1, copy);
            d.active = new_id;
            Ok(new_id)
        })
    }

    /// Moves `ids` (keeping their relative order) so the lowest of them lands at index `to`
    /// of the list without them (0 = bottom).
    pub fn move_layers(&mut self, ids: &[u32], to: usize) -> Result<()> {
        for &id in ids {
            self.index_of(id)?;
        }
        let (moved, mut rest): (Vec<Layer>, Vec<Layer>) = self
            .layers
            .iter()
            .cloned()
            .partition(|l| ids.contains(&l.id));
        let to = to.min(rest.len());
        let order: Vec<u32> = rest[..to]
            .iter()
            .chain(&moved)
            .chain(&rest[to..])
            .map(|l| l.id)
            .collect();
        if order.iter().eq(self.layers.iter().map(|l| &l.id)) {
            return Ok(());
        }
        let label = if ids.len() == 1 {
            "Reorder Layer"
        } else {
            "Reorder Layers"
        };
        self.edit(label, |d| {
            rest.splice(to..to, moved);
            d.layers = rest;
            Ok(())
        })
    }

    pub fn raise_layer(&mut self, id: u32) -> Result<()> {
        let i = self.index_of(id)?;
        if i + 1 < self.layers.len() {
            self.move_layers(&[id], i + 1)?;
        }
        Ok(())
    }

    pub fn lower_layer(&mut self, id: u32) -> Result<()> {
        let i = self.index_of(id)?;
        if i > 0 {
            self.move_layers(&[id], i - 1)?;
        }
        Ok(())
    }

    /// Merges the layers at `indices` (ascending) into one layer covering all of them. The
    /// result keeps the bottom layer's id, name and locks, takes the place of the top one,
    /// and becomes active.
    fn merge_indices(&mut self, label: &str, indices: Vec<usize>) -> Result<u32> {
        if indices.len() < 2 {
            return err("There is nothing to merge");
        }
        self.edit(label, |d| {
            let parts: Vec<&Layer> = indices.iter().map(|&i| &d.layers[i]).collect();
            let (mut x0, mut y0, mut x1, mut y1) = parts[0].bounds();
            for l in &parts[1..] {
                let b = l.bounds();
                x0 = x0.min(b.0);
                y0 = y0.min(b.1);
                x1 = x1.max(b.2);
                y1 = y1.max(b.3);
            }
            let mut surface = Surface::new((x1 - x0) as u32, (y1 - y0) as u32);
            for l in &parts {
                composite_over(&mut surface, (x0, y0), &l.surface, (l.x, l.y), l.opacity);
            }
            let bottom = parts[0];
            let mut merged = Layer::new(bottom.id, bottom.name.clone(), surface);
            merged.x = x0;
            merged.y = y0;
            merged.lock_alpha = bottom.lock_alpha;
            merged.lock_position = bottom.lock_position;
            let top = *indices.last().unwrap();
            let at = top + 1 - indices.len();
            for &i in indices.iter().rev() {
                d.layers.remove(i);
            }
            d.active = merged.id;
            d.layers.insert(at, merged);
            Ok(d.active)
        })
    }

    /// Merges a visible layer into the first visible layer below it.
    pub fn merge_down(&mut self, id: u32) -> Result<u32> {
        let i = self.index_of(id)?;
        if !self.layers[i].visible {
            return err("The layer is hidden");
        }
        let Some(below) = (0..i).rev().find(|&j| self.layers[j].visible) else {
            return err("There is no visible layer below");
        };
        self.merge_indices("Merge Down", vec![below, i])
    }

    /// Merges the visible layers among `ids` (hidden ones are left as they are).
    pub fn merge_layers(&mut self, ids: &[u32]) -> Result<u32> {
        let mut indices = ids
            .iter()
            .map(|&id| self.index_of(id))
            .collect::<Result<Vec<_>>>()?;
        indices.retain(|&i| self.layers[i].visible);
        indices.sort_unstable();
        indices.dedup();
        self.merge_indices("Merge Layers", indices)
    }

    pub fn merge_visible(&mut self) -> Result<u32> {
        let indices = (0..self.layers.len())
            .filter(|&i| self.layers[i].visible)
            .collect();
        self.merge_indices("Merge Visible Layers", indices)
    }

    /// Changes layer attributes as one undo step named `label` (no step if nothing changed).
    pub fn set_layer_props(&mut self, id: u32, props: LayerProps, label: &str) -> Result<()> {
        let i = self.index_of(id)?;
        let mut props = props;
        props.opacity = props.opacity.clamp(0.0, 1.0);
        if self.layers[i].props() == props {
            return Ok(());
        }
        self.edit(label, |d| {
            let l = &mut d.layers[i];
            if l.name != props.name {
                l.name_from_text = false;
            }
            l.name = props.name;
            l.opacity = props.opacity;
            l.visible = props.visible;
            l.lock_alpha = props.lock_alpha;
            l.lock_position = props.lock_position;
            l.x = props.x;
            l.y = props.y;
            Ok(())
        })
    }

    /// Clears the selected part of a layer to transparency, or the whole layer without a
    /// selection ("Clear", "Cut").
    pub fn clear_layer(&mut self, id: u32, label: &str) -> Result<()> {
        let i = self.index_of(id)?;
        self.edit(label, |d| {
            let l = &mut d.layers[i];
            match &d.selection {
                Some(m) => apply_mask(&mut l.surface, (l.x, l.y), m, false),
                None => l.surface.fill([0; 4]),
            }
            Ok(())
        })
    }

    /// The pixels Copy takes from a layer: the selected part (bounding box of the selection
    /// within the layer, alpha scaled by the selection) or the whole layer. Returns
    /// `(x, y, width, height, rgba)` in image coordinates, or `None` when the selection
    /// does not touch the layer.
    pub fn copy_pixels(&self, id: u32) -> Result<Option<CopiedPixels>> {
        let l = &self.layers[self.index_of(id)?];
        let Some(mask) = &self.selection else {
            return Ok(Some((l.x, l.y, l.width(), l.height(), l.surface.to_rgba())));
        };
        let Some((bx, by, bw, bh)) = mask.bounds() else {
            return Ok(None);
        };
        let (lx0, ly0, lx1, ly1) = l.bounds();
        let x0 = (bx as i32).max(lx0);
        let y0 = (by as i32).max(ly0);
        let x1 = (bx as i32 + bw as i32).min(lx1);
        let y1 = (by as i32 + bh as i32).min(ly1);
        if x0 >= x1 || y0 >= y1 {
            return Ok(None);
        }
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let rgba = l
            .surface
            .read_rect((x0 - l.x) as i64, (y0 - l.y) as i64, w, h);
        let mut s = Surface::from_rgba(w, h, &rgba);
        apply_mask(&mut s, (x0, y0), mask, true);
        Ok(Some((x0, y0, w, h, s.to_rgba())))
    }

    // ---- selection ------------------------------------------------------------------------

    pub fn selection(&self) -> Option<&Mask> {
        self.selection.as_ref()
    }

    /// Bounding box `(x, y, w, h)` of the selection, or `None` without one.
    pub fn selection_bounds(&mut self) -> Option<(u32, u32, u32, u32)> {
        if self.sel_bounds.is_none() {
            self.sel_bounds = Some(self.selection.as_ref().and_then(Mask::bounds));
        }
        self.sel_bounds.unwrap()
    }

    /// Selection tiles changed since the last call.
    pub fn take_selection_changes(&mut self) -> SelChanges {
        std::mem::take(&mut self.sel_changes)
    }

    /// Replaces the selection as one undo step (none if nothing changes). An empty mask
    /// becomes "no selection".
    fn set_selection(&mut self, label: &str, mask: Option<Mask>) -> Result<()> {
        let mask = mask.filter(|m| !m.is_empty());
        let same = match (&self.selection, &mask) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                (a.width(), a.height()) == (b.width(), b.height()) && b.diff_tiles(a).is_empty()
            }
            _ => false,
        };
        if same {
            return Ok(());
        }
        self.edit(label, |d| {
            d.selection = mask;
            Ok(())
        })
    }

    /// Combines a shape with the selection (the selection tools).
    pub fn select_region(&mut self, label: &str, region: &Region, mode: SelectMode) -> Result<()> {
        let mut mask = match (&self.selection, mode) {
            (Some(m), m2) if m2 != SelectMode::Replace => m.clone(),
            _ => Mask::new(self.width, self.height),
        };
        mask.combine(region, mode);
        self.set_selection(label, Some(mask))
    }

    pub fn select_all(&mut self) -> Result<()> {
        self.set_selection("Select All", Some(Mask::full(self.width, self.height)))
    }

    pub fn select_none(&mut self) -> Result<()> {
        self.set_selection("Select None", None)
    }

    pub fn invert_selection(&mut self) -> Result<()> {
        let mask = match &self.selection {
            Some(m) => {
                let mut m = m.clone();
                m.invert();
                m
            }
            None => Mask::full(self.width, self.height),
        };
        self.set_selection("Invert Selection", Some(mask))
    }

    /// Fuzzy Select (`contiguous`) or Select by Color at image point `(x, y)`, sampling the
    /// active layer or the visible layers composited (`merged`). Nothing happens when the
    /// point is outside the sampled pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn select_color(
        &mut self,
        label: &str,
        x: i64,
        y: i64,
        m: &ColorMatch,
        merged: bool,
        contiguous: bool,
        mode: SelectMode,
        feather: f32,
    ) -> Result<()> {
        let (ox, oy, w, h, rgba) = if merged {
            let (w, h) = (self.width, self.height);
            (0, 0, w, h, self.composite().to_rgba())
        } else {
            let l = &self.layers[self.active_index()];
            (l.x, l.y, l.width(), l.height(), l.surface.to_rgba())
        };
        let (sx, sy) = (x - ox as i64, y - oy as i64);
        if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
            return Ok(());
        }
        let data = selection::color_region(
            &rgba,
            w as usize,
            h as usize,
            sx as usize,
            sy as usize,
            m,
            contiguous,
        );
        let region = selection::feather(selection::region_from(ox, oy, w, h, data), feather);
        self.select_region(label, &region, mode)
    }

    /// Crops the image to the selection's bounding box (Pinta's Crop to Selection): every
    /// layer becomes the new image size, pixels outside the selection become transparent,
    /// and the selection is removed.
    pub fn crop_to_selection(&mut self) -> Result<()> {
        let Some((x, y, w, h)) = self.selection_bounds() else {
            return err("There is no selection");
        };
        self.edit("Crop to Selection", |d| {
            let mask = d.selection.take().unwrap();
            for l in &mut d.layers {
                let rgba = l
                    .surface
                    .read_rect(x as i64 - l.x as i64, y as i64 - l.y as i64, w, h);
                let mut s = Surface::from_rgba(w, h, &rgba);
                apply_mask(&mut s, (x as i32, y as i32), &mask, true);
                l.surface = s;
                l.x = 0;
                l.y = 0;
            }
            d.width = w;
            d.height = h;
            Ok(())
        })
    }

    // ---- painting -------------------------------------------------------------------------

    /// Starts a brush stroke on the active layer; the stroke is one undo step.
    pub fn begin_stroke(&mut self, brush: Brush, label: &str) {
        self.begin_stroke_from(brush, label, None);
    }

    /// Starts a stroke that takes its pixels from a source (Clone / Heal): a snapshot of the
    /// source layer, or of the merged image, as it is now.
    pub fn begin_stroke_from(&mut self, brush: Brush, label: &str, source: Option<SourceSpec>) {
        self.settle();
        let source = source.map(|s| {
            let (surface, origin) = match s.layer.and_then(|id| self.layer(id)) {
                Some(l) => (l.surface.clone(), (l.x, l.y)),
                None => (self.composite(), (0, 0)),
            };
            CloneSource {
                surface,
                origin,
                delta: s.delta,
                fixed: s.fixed,
            }
        });
        let layer = &self.layers[self.active_index()];
        let brush = Brush {
            lock_alpha: layer.lock_alpha,
            ..brush
        };
        let stroke = match source {
            Some(src) => Stroke::with_source(brush, &layer.surface, src, (layer.x, layer.y)),
            None => Stroke::new(brush, &layer.surface),
        };
        self.stroke = Some(ActiveStroke {
            stroke,
            layer: layer.id,
            label: label.to_string(),
            before: self.state(),
            dirty: DirtyTiles::default(),
            painted: false,
        });
    }

    /// Extends the current stroke to image point `(x, y)`.
    pub fn stroke_to(&mut self, x: f32, y: f32) {
        let Some(st) = &mut self.stroke else {
            return;
        };
        let Some(layer) = self.layers.iter_mut().find(|l| l.id == st.layer) else {
            return;
        };
        let clip = self.selection.as_ref().map(|m| (m, layer.x, layer.y));
        st.stroke.line_to(
            &mut layer.surface,
            &mut st.dirty,
            clip,
            x - layer.x as f32,
            y - layer.y as f32,
        );
        let tiles = st.dirty.take();
        if !tiles.is_empty() {
            st.painted = true;
            let id = st.layer;
            self.mark_changed(id, Some(tiles));
        }
    }

    pub fn end_stroke(&mut self) {
        if let Some(st) = self.stroke.take()
            && st.painted
        {
            self.drop_stale_sources();
            self.history
                .push(st.label, st.before, &self.layers, self.selection.as_ref());
        }
    }

    /// Bucket Fill on the active layer: `similar` = fill the region of colours similar to
    /// the one at image point `(x, y)` (sampled from the layer or, `merged`, from the
    /// visible layers); `None` = fill the whole selection (the whole layer without one).
    /// Always limited to the selection. Nothing happens outside the sampled pixels.
    pub fn bucket_fill(
        &mut self,
        x: i64,
        y: i64,
        color: Rgba8,
        opacity: f32,
        similar: Option<(&ColorMatch, bool)>,
    ) -> Result<()> {
        let i = self.active_index();
        let (lx, ly) = (self.layers[i].x, self.layers[i].y);
        // Colour region in its own coordinates (origin, size, coverage).
        let region = match similar {
            None => None,
            Some((m, merged)) => {
                let (ox, oy, w, h, rgba) = if merged {
                    (0, 0, self.width, self.height, self.composite().to_rgba())
                } else {
                    let l = &self.layers[i];
                    (lx, ly, l.width(), l.height(), l.surface.to_rgba())
                };
                let (sx, sy) = (x - ox as i64, y - oy as i64);
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                    return Ok(());
                }
                let cov = selection::color_region(
                    &rgba,
                    w as usize,
                    h as usize,
                    sx as usize,
                    sy as usize,
                    m,
                    true,
                );
                Some((ox, oy, w, h, cov))
            }
        };
        self.edit("Bucket Fill", |d| {
            let sel = d.selection.as_ref();
            let l = &mut d.layers[i];
            let coverage = |px: u32, py: u32| -> u8 {
                let (ix, iy) = (px as i64 + lx as i64, py as i64 + ly as i64);
                let s = sel.map_or(255, |m| m.get(ix, iy)) as u32;
                let r = match &region {
                    None => 255,
                    Some((ox, oy, w, h, cov)) => {
                        let (rx, ry) = (ix - *ox as i64, iy - *oy as i64);
                        if rx < 0 || ry < 0 || rx >= *w as i64 || ry >= *h as i64 {
                            0
                        } else {
                            cov[ry as usize * *w as usize + rx as usize] as u32
                        }
                    }
                };
                (s * r / 255) as u8
            };
            let c = color.to_array();
            crate::fill::paint_over(&mut l.surface, opacity, l.lock_alpha, coverage, |_, _| c);
            Ok(())
        })
    }

    /// Gradient over the active layer (limited to the selection) from image point `start`
    /// to `end`. Nothing happens when the two points coincide.
    pub fn gradient(&mut self, gradient: Gradient, opacity: f32) -> Result<()> {
        if gradient.start == gradient.end {
            return Ok(());
        }
        let i = self.active_index();
        self.edit("Gradient", |d| {
            let sel = d.selection.as_ref();
            let l = &mut d.layers[i];
            let (lx, ly) = (l.x, l.y);
            let coverage = |px: u32, py: u32| -> u8 {
                sel.map_or(255, |m| m.get(px as i64 + lx as i64, py as i64 + ly as i64))
            };
            let color = |px: u32, py: u32| {
                gradient.color_at(
                    (px as i64 + lx as i64) as f32 + 0.5,
                    (py as i64 + ly as i64) as f32 + 0.5,
                )
            };
            crate::fill::paint_over(&mut l.surface, opacity, l.lock_alpha, coverage, color);
            Ok(())
        })
    }

    /// Finishes a stroke and drops an adjustment preview before any other change (and
    /// before saving).
    pub fn settle(&mut self) {
        self.end_stroke();
        self.filter_cancel();
        self.transform_cancel();
        self.text_commit("Text");
    }

    /// Forgets transform sources of layers that were edited since (they hold the original
    /// pixels, which would otherwise stay in memory for nothing).
    fn drop_stale_sources(&mut self) {
        for l in &mut self.layers {
            if l.source
                .as_ref()
                .is_some_and(|s| !same_pixels(&l.surface, &s.rendered))
            {
                l.source = None;
            }
            if l.text
                .as_ref()
                .is_some_and(|t| !same_pixels(&l.surface, &t.rendered))
            {
                l.text = None;
                l.name_from_text = false;
            }
        }
    }

    // ---- text layers ----------------------------------------------------------------------

    /// Starts editing text: an existing text layer (`Some(id)`), or a new empty text layer at
    /// `(x, y)` above the active one. Returns the layer id. Changes made with `text_update`
    /// become one undo step at `text_commit`; `text_cancel` undoes them (and removes a new
    /// layer). Editing the same layer again just continues.
    pub fn text_begin(&mut self, id: Option<u32>, x: i32, y: i32) -> Result<u32> {
        if let (Some(s), Some(id)) = (&self.text_edit, id)
            && s.layer == id
        {
            return Ok(id);
        }
        self.settle();
        let before = self.state();
        let id = match id {
            Some(id) => {
                let i = self.index_of(id)?;
                if self.layers[i].text.is_none() {
                    return err("The layer is not a text layer");
                }
                self.active = id;
                id
            }
            None => {
                let id = self.alloc_id();
                let mut layer = Layer::new(id, self.unique_name("Text"), Surface::new(1, 1));
                layer.x = x;
                layer.y = y;
                layer.name_from_text = true;
                layer.text = Some(Arc::new(TextData {
                    spec: String::new(),
                    rendered: layer.surface.clone(),
                }));
                let at = self.active_index() + 1;
                self.layers.insert(at, layer);
                self.active = id;
                self.mark_changed(id, None);
                id
            }
        };
        self.text_edit = Some(TextSession { layer: id, before });
        Ok(id)
    }

    /// The layer being edited, if any.
    pub fn text_editing(&self) -> Option<u32> {
        self.text_edit.as_ref().map(|s| s.layer)
    }

    /// Shows newly rendered text on the layer being edited (not an undo step by itself):
    /// `spec` is the text description, `rgba` its `w` x `h` pixels placed at `(x, y)`, `name`
    /// the name derived from the text (used while the name follows the text).
    pub fn text_update(
        &mut self,
        spec: String,
        surface: Surface,
        x: i32,
        y: i32,
        name: &str,
    ) -> Result<()> {
        let Some(s) = &self.text_edit else {
            return err("No text is being edited");
        };
        let i = self.index_of(s.layer)?;
        let l = &mut self.layers[i];
        let old = std::mem::replace(&mut l.surface, surface);
        l.x = x;
        l.y = y;
        if l.name_from_text && !name.is_empty() {
            l.name = name.to_string();
        }
        l.text = Some(Arc::new(TextData {
            spec,
            rendered: l.surface.clone(),
        }));
        let id = l.id;
        let tiles = (old.width(), old.height()) == (l.width(), l.height());
        let changed = if tiles {
            Some(l.surface.diff_tiles(&old))
        } else {
            None
        };
        self.mark_changed(id, changed);
        Ok(())
    }

    /// Ends editing; records the changes as one undo step named `label` (none if nothing
    /// changed).
    pub fn text_commit(&mut self, label: &str) {
        let Some(s) = self.text_edit.take() else {
            return;
        };
        let changed = match (
            self.layers.iter().find(|l| l.id == s.layer),
            s.before.layers.iter().find(|l| l.id == s.layer),
        ) {
            (Some(now), Some(was)) => {
                !same_pixels(&now.surface, &was.surface)
                    || (now.x, now.y, &now.name) != (was.x, was.y, &was.name)
                    || now.text.as_ref().map(|t| &t.spec) != was.text.as_ref().map(|t| &t.spec)
            }
            (Some(_), None) => true,
            _ => false,
        };
        if changed {
            self.history
                .push(label, s.before, &self.layers, self.selection.as_ref());
        }
    }

    /// Ends editing and undoes its changes (a new text layer disappears).
    pub fn text_cancel(&mut self) {
        let Some(s) = self.text_edit.take() else {
            return;
        };
        let current = self.state();
        self.layers = s.before.layers;
        self.active = s.before.active;
        self.record_diff(&current);
    }

    /// The text description of a text layer (for editing), or `None`.
    pub fn layer_text(&self, id: u32) -> Option<&str> {
        self.layer(id)?.text.as_ref().map(|t| t.spec.as_str())
    }

    /// Turns a text layer into a plain layer (GIMP's "Discard Text Information").
    pub fn discard_text(&mut self, id: u32) -> Result<()> {
        let i = self.index_of(id)?;
        if self.layers[i].text.is_none() {
            return Ok(());
        }
        self.edit("Discard Text Information", |d| {
            d.layers[i].text = None;
            d.layers[i].name_from_text = false;
            Ok(())
        })
    }

    // ---- transform tools ------------------------------------------------------------------

    /// Starts transforming the active layer, or its selected part (which is lifted off the
    /// layer while the transform is shown). Returns the rectangle `(x, y, w, h)` being
    /// transformed in image coordinates. Calling it again returns the same rectangle.
    pub fn transform_begin(&mut self) -> Result<(i32, i32, u32, u32)> {
        if let Some(s) = &self.transform {
            return Ok((s.pos.0, s.pos.1, s.float.width(), s.float.height()));
        }
        self.end_stroke();
        self.filter_cancel();
        let i = self.active_index();
        if self.layers[i].lock_position {
            return err("The layer's position and size are locked");
        }
        let before = self.state();
        let l = &self.layers[i];
        let (float, pos) = match &self.selection {
            None => (l.surface.clone(), (l.x, l.y)),
            Some(_) => {
                let Some((x, y, w, h, rgba)) = self.copy_pixels(l.id)? else {
                    return err("The selection does not touch the active layer");
                };
                (Surface::from_rgba(w, h, &rgba), (x, y))
            }
        };
        let mask = self.selection.clone();
        if let Some(m) = &mask {
            // Show the layer without the lifted part.
            let l = &mut self.layers[i];
            let old = l.surface.clone();
            apply_mask(&mut l.surface, (l.x, l.y), m, false);
            let tiles = l.surface.diff_tiles(&old);
            let id = l.id;
            if !tiles.is_empty() {
                self.mark_changed(id, Some(tiles));
            }
        }
        let rect = (pos.0, pos.1, float.width(), float.height());
        self.transform = Some(TransformSession {
            layer: self.layers[i].id,
            before,
            float,
            pos,
            mask,
        });
        Ok(rect)
    }

    /// The pixels being transformed (for the preview), if a transform is in progress.
    pub fn transform_float(&self) -> Option<&Surface> {
        self.transform.as_ref().map(|s| &s.float)
    }

    /// True while a transform is in progress and the layer is shown without its selected
    /// part (otherwise the frontend hides the whole layer during the preview).
    pub fn transform_lifted(&self) -> Option<bool> {
        self.transform.as_ref().map(|s| s.mask.is_some())
    }

    /// Applies `m` (image coordinates -> image coordinates) to what `transform_begin`
    /// lifted, as one undo step named `label`. A whole layer is re-rendered from its
    /// original pixels when it was transformed before (no quality loss from repeated
    /// transforms). Grid-exact transforms (flips, quarter turns) copy pixels exactly.
    pub fn transform_commit(
        &mut self,
        m: Matrix,
        interp: Interpolation,
        label: &str,
    ) -> Result<()> {
        let Some(job) = self.transform_job(&m, interp) else {
            self.transform = None;
            return Ok(());
        };
        let (sw, sh) = (job.source.width() as usize, job.source.height() as usize);
        let rendered =
            projective::render(&job.source.to_rgba(), sw, sh, 4, &job.matrix, job.interp);
        let mask = job.mask.as_ref().and_then(|mask| {
            let (w, h) = (self.width as usize, self.height as usize);
            projective::render(&mask.to_dense(), w, h, 1, &m, Interpolation::Linear)
        });
        self.transform_commit_with(m, label, rendered, mask)
    }

    /// What `transform_commit` renders for `m`: the source pixels, the matrix and the
    /// interpolation, and the selection to render through `m` (linear, one channel) when a
    /// selected part moves. `None` when no transform is in progress.
    pub fn transform_job(&self, m: &Matrix, interp: Interpolation) -> Option<TransformJob> {
        let plan = self.transform_plan(m)?;
        let interp = if plan.matrix.is_grid_exact() {
            Interpolation::None
        } else {
            interp
        };
        Some(TransformJob {
            source: plan.source,
            matrix: plan.matrix,
            interp,
            mask: self.transform.as_ref()?.mask.clone(),
        })
    }

    fn transform_plan(&self, m: &Matrix) -> Option<TransformPlan> {
        let s = self.transform.as_ref()?;
        let index = self.layers.iter().position(|l| l.id == s.layer)?;
        let orig = s.before.layers.iter().find(|l| l.id == s.layer)?.clone();
        let (source, matrix) = match &s.mask {
            None => {
                // Start from the original pixels when the layer is an unedited transform.
                let (src, base) = match &orig.source {
                    Some(src) if same_pixels(&orig.surface, &src.rendered) => (
                        src.original.clone(),
                        Matrix::translate((orig.x - src.x) as f64, (orig.y - src.y) as f64)
                            .mul(&src.matrix),
                    ),
                    _ => (
                        orig.surface.clone(),
                        Matrix::translate(orig.x as f64, orig.y as f64),
                    ),
                };
                (src, m.mul(&base))
            }
            Some(_) => (
                s.float.clone(),
                m.mul(&Matrix::translate(s.pos.0 as f64, s.pos.1 as f64)),
            ),
        };
        Some(TransformPlan {
            index,
            orig,
            source,
            matrix,
        })
    }

    /// Finishes `transform_commit` with the pixels rendered for its
    /// [`transform_job`](Self::transform_job) (`None`: too large or degenerate) and the
    /// rendered selection (`None`: nothing left selected).
    pub fn transform_commit_with(
        &mut self,
        m: Matrix,
        label: &str,
        rendered: Option<Rendered>,
        mask_rendered: Option<Rendered>,
    ) -> Result<()> {
        let Some(plan) = self.transform_plan(&m) else {
            self.transform = None;
            return Ok(());
        };
        let Some(s) = self.transform.take() else {
            return Ok(());
        };
        let TransformPlan {
            index: i,
            orig,
            source,
            matrix: mat,
        } = plan;
        let current = self.state();
        let mut layer = orig.clone();
        let mut selection = self.selection.clone();
        let Some(r) = rendered else {
            self.restore_layer(i, &orig, &current);
            return err("The transformed result is too large or invalid");
        };
        match &s.mask {
            None => {
                layer.surface = Surface::from_rgba(r.w, r.h, &r.data);
                layer.x = r.x;
                layer.y = r.y;
                layer.source = Some(Arc::new(TransformSource {
                    original: source,
                    matrix: mat,
                    rendered: layer.surface.clone(),
                    x: r.x,
                    y: r.y,
                }));
            }
            Some(mask) => {
                // The layer without the lifted part, grown to hold the result.
                let mut base = orig.surface.clone();
                apply_mask(&mut base, (orig.x, orig.y), mask, false);
                let (lx0, ly0, lx1, ly1) = orig.bounds();
                let x0 = lx0.min(r.x);
                let y0 = ly0.min(r.y);
                let x1 = lx1.max(r.x + r.w as i32);
                let y1 = ly1.max(r.y + r.h as i32);
                let mut out = Surface::new((x1 - x0) as u32, (y1 - y0) as u32);
                composite_over(&mut out, (x0, y0), &base, (orig.x, orig.y), 1.0);
                let moved = Surface::from_rgba(r.w, r.h, &r.data);
                composite_over(&mut out, (x0, y0), &moved, (r.x, r.y), 1.0);
                layer.surface = out;
                layer.x = x0;
                layer.y = y0;
                layer.source = None;
                // The selection follows the pixels.
                let (w, h) = (self.width as usize, self.height as usize);
                selection = mask_rendered.and_then(|mr| {
                    let mut dense = vec![0u8; w * h];
                    for y in 0..mr.h as i32 {
                        let iy = mr.y + y;
                        if iy < 0 || iy >= h as i32 {
                            continue;
                        }
                        for x in 0..mr.w as i32 {
                            let ix = mr.x + x;
                            if ix >= 0 && ix < w as i32 {
                                dense[iy as usize * w + ix as usize] =
                                    mr.data[(y * mr.w as i32 + x) as usize];
                            }
                        }
                    }
                    Some(Mask::from_dense(self.width, self.height, &dense))
                        .filter(|m| !m.is_empty())
                });
            }
        }
        self.layers[i] = layer;
        self.selection = selection;
        self.drop_stale_sources();
        self.record_diff(&current);
        self.history
            .push(label, s.before, &self.layers, self.selection.as_ref());
        Ok(())
    }

    /// Puts a layer back as it was before a transform (and reports the tiles).
    fn restore_layer(&mut self, i: usize, orig: &Layer, current: &State) {
        self.layers[i] = orig.clone();
        self.record_diff(current);
    }

    /// Ends a transform without changing anything.
    pub fn transform_cancel(&mut self) {
        let Some(s) = self.transform.take() else {
            return;
        };
        if s.mask.is_none() {
            return;
        }
        let (Some(i), Some(orig)) = (
            self.layers.iter().position(|l| l.id == s.layer),
            s.before.layers.iter().find(|l| l.id == s.layer),
        ) else {
            return;
        };
        let current = self.state();
        self.layers[i] = orig.clone();
        self.record_diff(&current);
    }

    // ---- moving ---------------------------------------------------------------------------

    /// Moves layers by `(dx, dy)` as one undo step. Refused when one is position-locked.
    pub fn move_layers_by(&mut self, ids: &[u32], dx: i32, dy: i32) -> Result<()> {
        let indices = ids
            .iter()
            .map(|&id| self.index_of(id))
            .collect::<Result<Vec<_>>>()?;
        if indices.iter().any(|&i| self.layers[i].lock_position) {
            return err("The layer's position is locked");
        }
        if (dx, dy) == (0, 0) || indices.is_empty() {
            return Ok(());
        }
        let label = if indices.len() == 1 {
            "Move Layer"
        } else {
            "Move Layers"
        };
        self.edit(label, |d| {
            for &i in &indices {
                d.layers[i].x += dx;
                d.layers[i].y += dy;
            }
            Ok(())
        })
    }

    /// Moves the selection outline by `(dx, dy)` (the pixels stay).
    pub fn move_selection_by(&mut self, dx: i32, dy: i32) -> Result<()> {
        let Some(m) = &self.selection else {
            return Ok(());
        };
        if (dx, dy) == (0, 0) {
            return Ok(());
        }
        let (w, h) = (self.width as i64, self.height as i64);
        let src = m.to_dense();
        let mut out = vec![0u8; src.len()];
        for y in 0..h {
            let sy = y - dy as i64;
            if sy < 0 || sy >= h {
                continue;
            }
            for x in 0..w {
                let sx = x - dx as i64;
                if sx >= 0 && sx < w {
                    out[(y * w + x) as usize] = src[(sy * w + sx) as usize];
                }
            }
        }
        let mask = Mask::from_dense(self.width, self.height, &out);
        self.set_selection("Move Selection", Some(mask))
    }

    /// The topmost visible layer with a mostly opaque pixel at image point `(x, y)` (the Move
    /// tool's "Pick a layer").
    pub fn pick_layer(&self, x: i64, y: i64) -> Option<u32> {
        self.layers
            .iter()
            .rev()
            .filter(|l| l.visible)
            .find(|l| {
                l.surface
                    .pixel(x - l.x as i64, y - l.y as i64)
                    .is_some_and(|p| p[3] > 63)
            })
            .map(|l| l.id)
    }

    // ---- image and layer geometry ---------------------------------------------------------

    /// Scales the image and every layer (sizes and positions) to `width` x `height`.
    pub fn scale_image(&mut self, width: u32, height: u32, interp: Interpolation) -> Result<()> {
        if width == 0 || height == 0 {
            return err("The size must be at least 1 pixel");
        }
        if (width, height) == (self.width, self.height) {
            return Ok(());
        }
        let image = (self.width, self.height);
        let layers = self
            .layers
            .iter()
            .map(|l| {
                let (lw, lh) = (l.width(), l.height());
                let (nw, nh) = scaled_layer_size((lw, lh), image, (width, height));
                let out = transform::resample(
                    &l.surface.to_rgba(),
                    lw as usize,
                    lh as usize,
                    4,
                    nw as usize,
                    nh as usize,
                    interp,
                );
                Surface::from_rgba(nw, nh, &out)
            })
            .collect();
        let selection = self.selection.as_ref().map(|m| {
            let (w, h) = (self.width as usize, self.height as usize);
            let dense = m.to_dense();
            transform::resample(&dense, w, h, 1, width as usize, height as usize, interp)
        });
        self.scale_image_with(width, height, layers, selection)
    }

    /// Finishes `scale_image` with the layers (bottom to top, sized by
    /// [`scaled_layer_size`]) and the selection (dense, image-sized) scaled elsewhere.
    pub fn scale_image_with(
        &mut self,
        width: u32,
        height: u32,
        layers: Vec<Surface>,
        selection: Option<Vec<u8>>,
    ) -> Result<()> {
        let image = (self.width, self.height);
        let fits = layers.len() == self.layers.len()
            && self.layers.iter().zip(&layers).all(|(l, s)| {
                scaled_layer_size((l.width(), l.height()), image, (width, height))
                    == (s.width(), s.height())
            })
            && selection.is_some() == self.selection.is_some()
            && selection
                .as_ref()
                .is_none_or(|s| s.len() == width as usize * height as usize);
        if !fits {
            return err("The image changed while it was being scaled");
        }
        let sx = width as f64 / self.width as f64;
        let sy = height as f64 / self.height as f64;
        self.edit("Scale Image", |d| {
            for (l, s) in d.layers.iter_mut().zip(layers) {
                l.surface = s;
                l.x = (l.x as f64 * sx).round() as i32;
                l.y = (l.y as f64 * sy).round() as i32;
            }
            if let Some(dense) = selection {
                d.selection =
                    Some(Mask::from_dense(width, height, &dense)).filter(|m| !m.is_empty());
            }
            d.width = width;
            d.height = height;
            Ok(())
        })
    }

    /// Changes the canvas to `width` x `height`, moving the content by `(ox, oy)`.
    /// `resize`: which layers become the new canvas size (the rest keep their size).
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        ox: i32,
        oy: i32,
        resize: ResizeLayers,
    ) -> Result<()> {
        if width == 0 || height == 0 {
            return err("The size must be at least 1 pixel");
        }
        if (width, height, ox, oy) == (self.width, self.height, 0, 0) {
            return Ok(());
        }
        self.edit("Canvas Size", |d| {
            let (iw, ih) = (d.width, d.height);
            for l in &mut d.layers {
                let image_sized = (l.x, l.y, l.width(), l.height()) == (0, 0, iw, ih);
                l.x += ox;
                l.y += oy;
                let grow = match resize {
                    ResizeLayers::None => false,
                    ResizeLayers::ImageSized => image_sized,
                    ResizeLayers::All => true,
                };
                if grow {
                    let rgba = l
                        .surface
                        .read_rect(-(l.x as i64), -(l.y as i64), width, height);
                    l.surface = Surface::from_rgba(width, height, &rgba);
                    l.x = 0;
                    l.y = 0;
                }
            }
            if let Some(m) = &d.selection {
                let src = m.to_dense();
                let (w, h) = (width as i64, height as i64);
                let mut out = vec![0u8; (w * h) as usize];
                for y in 0..h {
                    let sy = y - oy as i64;
                    if sy < 0 || sy >= ih as i64 {
                        continue;
                    }
                    for x in 0..w {
                        let sx = x - ox as i64;
                        if sx >= 0 && sx < iw as i64 {
                            out[(y * w + x) as usize] = src[(sy * iw as i64 + sx) as usize];
                        }
                    }
                }
                d.selection = Some(Mask::from_dense(width, height, &out)).filter(|m| !m.is_empty());
            }
            d.width = width;
            d.height = height;
            Ok(())
        })
    }

    /// Mirrors the whole image (all layers, their positions and the selection).
    pub fn flip_image(&mut self, horizontal: bool) -> Result<()> {
        let label = if horizontal {
            "Flip Image Horizontally"
        } else {
            "Flip Image Vertically"
        };
        self.edit(label, |d| {
            let (iw, ih) = (d.width as i32, d.height as i32);
            for l in &mut d.layers {
                flip_surface(&mut l.surface, horizontal);
                if horizontal {
                    l.x = iw - (l.x + l.width() as i32);
                } else {
                    l.y = ih - (l.y + l.height() as i32);
                }
            }
            if let Some(m) = &d.selection {
                let (w, h) = (d.width as usize, d.height as usize);
                let out = transform::flip(&m.to_dense(), w, h, 1, horizontal);
                d.selection = Some(Mask::from_dense(d.width, d.height, &out));
            }
            Ok(())
        })
    }

    /// Rotates the whole image by `turns` quarter turns clockwise (1 = 90°, 2 = 180°,
    /// 3 = 90° counter-clockwise).
    pub fn rotate_image(&mut self, turns: u32) -> Result<()> {
        let turns = turns % 4;
        if turns == 0 {
            return Ok(());
        }
        let label = match turns {
            1 => "Rotate 90° Clockwise",
            2 => "Rotate 180°",
            _ => "Rotate 90° Counter-Clockwise",
        };
        self.edit(label, |d| {
            let (iw, ih) = (d.width as i32, d.height as i32);
            for l in &mut d.layers {
                let (lw, lh) = (l.width() as i32, l.height() as i32);
                let (x, y) = (l.x, l.y);
                (l.x, l.y) = match turns {
                    1 => (ih - (y + lh), x),
                    2 => (iw - (x + lw), ih - (y + lh)),
                    _ => (y, iw - (x + lw)),
                };
                let (out, nw, nh) =
                    transform::rotate(&l.surface.to_rgba(), lw as usize, lh as usize, 4, turns);
                l.surface = Surface::from_rgba(nw as u32, nh as u32, &out);
            }
            if let Some(m) = &d.selection {
                let (w, h) = (d.width as usize, d.height as usize);
                let (out, nw, nh) = transform::rotate(&m.to_dense(), w, h, 1, turns);
                d.selection = Some(Mask::from_dense(nw as u32, nh as u32, &out));
            }
            if turns != 2 {
                std::mem::swap(&mut d.width, &mut d.height);
            }
            Ok(())
        })
    }

    /// Makes a layer exactly the image size at (0, 0) (cropping or extending it).
    pub fn layer_to_image_size(&mut self, id: u32) -> Result<()> {
        let i = self.index_of(id)?;
        let l = &self.layers[i];
        if (l.x, l.y, l.width(), l.height()) == (0, 0, self.width, self.height) {
            return Ok(());
        }
        self.edit("Layer to Image Size", |d| {
            let (w, h) = (d.width, d.height);
            let l = &mut d.layers[i];
            let rgba = l.surface.read_rect(-(l.x as i64), -(l.y as i64), w, h);
            l.surface = Surface::from_rgba(w, h, &rgba);
            l.x = 0;
            l.y = 0;
            Ok(())
        })
    }

    /// Mirrors a layer's pixels in place.
    pub fn flip_layer(&mut self, id: u32, horizontal: bool) -> Result<()> {
        let i = self.index_of(id)?;
        let label = if horizontal {
            "Flip Layer Horizontally"
        } else {
            "Flip Layer Vertically"
        };
        self.edit(label, |d| {
            flip_surface(&mut d.layers[i].surface, horizontal);
            Ok(())
        })
    }

    /// Multiplies a layer's alpha by `mask` (one byte per layer pixel, 255 = keep) as one undo
    /// step (AI background removal). Refused when the layer size no longer matches.
    pub fn multiply_alpha(
        &mut self,
        id: u32,
        width: u32,
        height: u32,
        mask: &[u8],
        label: &str,
    ) -> Result<()> {
        let i = self.index_of(id)?;
        let l = &self.layers[i];
        if (l.width(), l.height()) != (width, height) || mask.len() != (width * height) as usize {
            return err("The layer changed size while the background was being removed");
        }
        let m = Mask::from_dense(width, height, mask);
        self.edit(label, |d| {
            apply_mask(&mut d.layers[i].surface, (0, 0), &m, true);
            Ok(())
        })
    }

    // ---- adjustments ----------------------------------------------------------------------

    /// Applies an adjustment to the active layer (limited to the selection) as one step.
    pub fn apply_filter(&mut self, label: &str, filter: Filter) -> Result<()> {
        let i = self.active_index();
        self.edit(label, |d| {
            let image = (d.width, d.height);
            let sel = d.selection.as_ref();
            let l = &mut d.layers[i];
            filters::apply(&mut l.surface, (l.x, l.y), image, sel, filter);
            Ok(())
        })
    }

    /// Shows an adjustment on the active layer without recording it (live preview). Each
    /// call starts again from the layer's pixels before the first preview. End with
    /// `filter_commit` or `filter_cancel`; any other change cancels it.
    pub fn filter_preview(&mut self, filter: Filter) {
        let serial = self.filter_begin();
        let Some((mut surface, pos)) = self.filter_original() else {
            return;
        };
        let image = (self.width, self.height);
        filters::apply(&mut surface, pos, image, self.selection.as_ref(), filter);
        self.filter_show(serial, surface);
    }

    /// Starts a filter session on the active layer unless one is running; returns its
    /// serial. A preview computed elsewhere (the native backend) starts from
    /// [`filter_original`](Self::filter_original) and is shown with
    /// [`filter_show`](Self::filter_show).
    pub fn filter_begin(&mut self) -> u32 {
        self.end_stroke();
        if self.filter.is_none() {
            self.filter_serial += 1;
            self.filter = Some(FilterSession {
                layer: self.active,
                before: self.state(),
                serial: self.filter_serial,
            });
        }
        self.filter.as_ref().map_or(0, |s| s.serial)
    }

    /// The pixels the filter session starts from and the layer's position.
    pub fn filter_original(&self) -> Option<(Surface, (i32, i32))> {
        let s = self.filter.as_ref()?;
        let l = self.layers.iter().find(|l| l.id == s.layer)?;
        let orig = s.before.layers.iter().find(|l| l.id == s.layer)?;
        Some((orig.surface.clone(), (l.x, l.y)))
    }

    /// Shows filtered pixels of session `serial` on its layer (not recorded). Ignored (false)
    /// when that session has ended or the layer changed size.
    pub fn filter_show(&mut self, serial: u32, mut surface: Surface) -> bool {
        let Some(s) = self.filter.as_ref().filter(|s| s.serial == serial) else {
            return false;
        };
        let Some(i) = self.layers.iter().position(|l| l.id == s.layer) else {
            self.filter = None;
            return false;
        };
        let Some(orig) = s.before.layers.iter().find(|l| l.id == s.layer) else {
            return false;
        };
        if (surface.width(), surface.height()) != (orig.width(), orig.height()) {
            return false;
        }
        surface.share_equal_tiles(&orig.surface);
        let old = std::mem::replace(&mut self.layers[i].surface, surface);
        let tiles = self.layers[i].surface.diff_tiles(&old);
        if !tiles.is_empty() {
            let id = self.layers[i].id;
            self.mark_changed(id, Some(tiles));
        }
        true
    }

    /// Records the previewed adjustment as one undo step.
    pub fn filter_commit(&mut self, label: &str) {
        if let Some(s) = self.filter.take() {
            let changed = self.layers.iter().zip(&s.before.layers).any(|(a, b)| {
                (a.width(), a.height()) == (b.width(), b.height())
                    && !a.surface.diff_tiles(&b.surface).is_empty()
            });
            if changed {
                self.drop_stale_sources();
                self.history
                    .push(label, s.before, &self.layers, self.selection.as_ref());
            }
        }
    }

    /// Drops the preview and restores the layer.
    pub fn filter_cancel(&mut self) {
        let Some(s) = self.filter.take() else {
            return;
        };
        let Some(i) = self.layers.iter().position(|l| l.id == s.layer) else {
            return;
        };
        if let Some(orig) = s.before.layers.iter().find(|l| l.id == s.layer) {
            let old = std::mem::replace(&mut self.layers[i].surface, orig.surface.clone());
            let tiles = self.layers[i].surface.diff_tiles(&old);
            if !tiles.is_empty() {
                self.mark_changed(s.layer, Some(tiles));
            }
        }
    }

    // ---- reading pixels -------------------------------------------------------------------

    /// The visible layers composited at one image pixel (transparent outside the image).
    fn composite_pixel(&self, x: i64, y: i64) -> [u8; 4] {
        let mut out = [0u8; 4];
        for l in self.layers.iter().filter(|l| l.visible) {
            if let Some(p) = l.surface.pixel(x - l.x as i64, y - l.y as i64) {
                blend_over(&mut out, &p, l.opacity);
            }
        }
        out
    }

    /// The colour at image pixel `(x, y)` for the Color Picker: of the active layer, or of
    /// the visible layers composited (`merged`). With `radius > 1` it is the average over a
    /// square of side `2 * radius - 1` (averaged premultiplied, so transparent pixels do
    /// not darken it). `None` when no sampled pixel is inside the image (and, unmerged,
    /// inside the active layer).
    pub fn sample(&self, x: i64, y: i64, radius: u32, merged: bool) -> Option<Rgba8> {
        let r = radius.max(1) as i64 - 1;
        let layer = &self.layers[self.active_index()];
        let (mut acc, mut n) = ([0f64; 4], 0u32);
        for py in y - r..=y + r {
            for px in x - r..=x + r {
                if px < 0 || py < 0 || px >= self.width as i64 || py >= self.height as i64 {
                    continue;
                }
                let p = if merged {
                    self.composite_pixel(px, py)
                } else {
                    match layer
                        .surface
                        .pixel(px - layer.x as i64, py - layer.y as i64)
                    {
                        Some(p) => p,
                        None => continue,
                    }
                };
                let a = p[3] as f64;
                for c in 0..3 {
                    acc[c] += p[c] as f64 * a;
                }
                acc[3] += a;
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        if acc[3] == 0.0 {
            return Some(Rgba8::TRANSPARENT);
        }
        let ch = |c: usize| (acc[c] / acc[3] + 0.5) as u8;
        Some(Rgba8::new(
            ch(0),
            ch(1),
            ch(2),
            (acc[3] / n as f64 + 0.5) as u8,
        ))
    }

    /// The visible layers flattened onto a transparent image-sized surface.
    pub fn composite(&self) -> Surface {
        let mut out = Surface::new(self.width, self.height);
        for l in self.layers.iter().filter(|l| l.visible) {
            composite_over(&mut out, (0, 0), &l.surface, (l.x, l.y), l.opacity);
        }
        out
    }

    /// A `w` x `h` straight RGBA8 thumbnail of a layer (its own extent).
    pub fn thumbnail(&self, id: u32, w: u32, h: u32) -> Result<Vec<u8>> {
        let i = self.index_of(id)?;
        Ok(thumbnail(&self.layers[i].surface, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const WHITE: Rgba8 = Rgba8::new(255, 255, 255, 255);

    fn ids(d: &Document) -> Vec<u32> {
        d.layers().iter().map(|l| l.id).collect()
    }

    fn pixel(s: &Surface, x: usize, y: usize) -> [u8; 4] {
        let rgba = s.to_rgba();
        let i = (y * s.width() as usize + x) * 4;
        rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn add_undo_redo_and_dirty_flag() {
        let mut d = Document::new(100, 80, WHITE);
        assert!(!d.is_dirty());
        let id = d.add_layer(Rgba8::TRANSPARENT).unwrap();
        assert_eq!(ids(&d), vec![1, id]);
        assert_eq!(d.active(), id);
        assert_eq!(d.layer(id).unwrap().name, "Layer 2");
        assert!(d.is_dirty());
        assert_eq!(d.undo_label(), Some("New Layer"));

        assert!(d.undo());
        assert_eq!(ids(&d), vec![1]);
        assert_eq!(d.active(), 1);
        assert!(!d.is_dirty());
        assert!(d.redo());
        assert_eq!(ids(&d), vec![1, id]);
        assert!(!d.redo());
    }

    #[test]
    fn stroke_is_one_step_and_reports_tiles() {
        let mut d = Document::new(200, 200, Rgba8::TRANSPARENT);
        d.take_changes();
        d.begin_stroke(Brush::default(), "Paintbrush");
        d.stroke_to(10.0, 10.0);
        d.stroke_to(150.0, 10.0);
        d.end_stroke();
        let changes = d.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].tiles, Some(vec![0, 1, 2]));
        assert_eq!(pixel(&d.composite(), 100, 10), [0, 0, 0, 255]);

        assert!(d.undo());
        assert_eq!(pixel(&d.composite(), 100, 10), [0, 0, 0, 0]);
        // Undo reports exactly the tiles that differ.
        assert_eq!(d.take_changes()[0].tiles, Some(vec![0, 1, 2]));
        assert!(d.redo());
        assert_eq!(pixel(&d.composite(), 100, 10), [0, 0, 0, 255]);
    }

    #[test]
    fn delete_keeps_one_layer_and_picks_the_layer_below() {
        let mut d = Document::new(10, 10, WHITE);
        let a = d.add_layer(RED).unwrap();
        let b = d.add_layer(RED).unwrap();
        assert!(d.delete_layers(&[1, a, b]).is_err());
        d.delete_layers(&[b]).unwrap();
        assert_eq!(d.active(), a);
        d.delete_layers(&[a]).unwrap();
        assert_eq!(d.active(), 1);
        d.undo();
        d.undo();
        assert_eq!(ids(&d), vec![1, a, b]);
    }

    #[test]
    fn reorder_and_raise_lower() {
        let mut d = Document::new(10, 10, WHITE);
        let a = d.add_layer(RED).unwrap();
        let b = d.add_layer(RED).unwrap();
        d.move_layers(&[b], 0).unwrap();
        assert_eq!(ids(&d), vec![b, 1, a]);
        d.raise_layer(b).unwrap();
        assert_eq!(ids(&d), vec![1, b, a]);
        d.lower_layer(a).unwrap();
        assert_eq!(ids(&d), vec![1, a, b]);
        // A no-op move records nothing.
        let label = d.undo_label().map(str::to_string);
        d.move_layers(&[b], 2).unwrap();
        assert_eq!(d.undo_label().map(str::to_string), label);
    }

    #[test]
    fn merge_down_composites_with_opacity_and_offset() {
        let mut d = Document::new(100, 100, WHITE);
        let top = d
            .add_layer_from(
                "Paste",
                "Pasted",
                {
                    let mut s = Surface::new(20, 20);
                    s.fill(RED.to_array());
                    s
                },
                90,
                90,
            )
            .unwrap();
        let mut p = d.layer(top).unwrap().props();
        p.opacity = 0.5;
        d.set_layer_props(top, p, "Layer Opacity").unwrap();
        let merged = d.merge_down(top).unwrap();
        assert_eq!(merged, 1);
        assert_eq!(ids(&d), vec![1]);
        let l = d.layer(1).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (0, 0, 110, 110));
        assert_eq!(pixel(&l.surface, 95, 95), [255, 128, 128, 255]);
        assert_eq!(pixel(&l.surface, 105, 105), [255, 0, 0, 128]);
        assert_eq!(pixel(&d.composite(), 50, 50), [255, 255, 255, 255]);
    }

    #[test]
    fn merge_visible_skips_hidden_layers() {
        let mut d = Document::new(10, 10, WHITE);
        let a = d.add_layer(RED).unwrap();
        let b = d.add_layer(RED).unwrap();
        let mut p = d.layer(a).unwrap().props();
        p.visible = false;
        d.set_layer_props(a, p, "Hide Layer").unwrap();
        d.merge_visible().unwrap();
        assert_eq!(ids(&d), vec![a, 1]);
        assert!(d.merge_down(b).is_err());
    }

    #[test]
    fn duplicate_names_follow_gimp() {
        let mut d = Document::new(10, 10, WHITE);
        let c1 = d.duplicate_layer(1).unwrap();
        let c2 = d.duplicate_layer(1).unwrap();
        assert_eq!(d.layer(c1).unwrap().name, "Background copy");
        assert_eq!(d.layer(c2).unwrap().name, "Background copy #1");
    }

    #[test]
    fn sample_single_average_merged_and_outside() {
        let mut d = Document::new(10, 10, WHITE);
        let top = d
            .add_layer_from(
                "Paste",
                "Red",
                {
                    let mut s = Surface::new(2, 2);
                    s.fill(RED.to_array());
                    s
                },
                4,
                4,
            )
            .unwrap();
        // Active layer only: inside it red, outside it nothing.
        assert_eq!(d.sample(4, 4, 1, false), Some(RED));
        assert_eq!(d.sample(0, 0, 1, false), None);
        // Merged: white where only the background is.
        assert_eq!(d.sample(0, 0, 1, true), Some(WHITE));
        // A 3x3 average around (4, 4) merged: 4 red + 5 white pixels.
        assert_eq!(
            d.sample(4, 4, 2, true),
            Some(Rgba8::new(255, 142, 142, 255))
        );
        // Unmerged average uses only pixels inside the layer.
        assert_eq!(d.sample(4, 4, 2, false), Some(RED));
        assert_eq!(d.sample(-5, -5, 1, true), None);
        let _ = top;
    }

    #[test]
    fn lock_alpha_paints_colour_only() {
        let mut d = Document::new(64, 64, Rgba8::TRANSPARENT);
        let mut p = d.layer(1).unwrap().props();
        p.lock_alpha = true;
        d.set_layer_props(1, p, "Lock Alpha").unwrap();
        d.begin_stroke(
            Brush {
                size: 30.0,
                hardness: 1.0,
                ..Brush::default()
            },
            "Paintbrush",
        );
        d.stroke_to(32.0, 32.0);
        d.end_stroke();
        assert_eq!(pixel(&d.composite(), 32, 32), [0, 0, 0, 0]);
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Region {
        selection::rectangle(x, y, w, h, 0.0, false, 1000, 1000)
    }

    #[test]
    fn selection_is_undoable_and_reports_changes() {
        let mut d = Document::new(200, 100, WHITE);
        d.select_region(
            "Rectangle Select",
            &rect(10.0, 10.0, 20.0, 30.0),
            SelectMode::Replace,
        )
        .unwrap();
        assert_eq!(d.selection_bounds(), Some((10, 10, 20, 30)));
        assert_eq!(d.take_selection_changes(), SelChanges::All);
        d.select_region(
            "Rectangle Select",
            &rect(100.0, 10.0, 20.0, 30.0),
            SelectMode::Add,
        )
        .unwrap();
        assert_eq!(d.selection_bounds(), Some((10, 10, 110, 30)));
        assert_eq!(
            d.take_selection_changes(),
            SelChanges::Tiles([1].into_iter().collect())
        );
        assert!(d.undo());
        assert_eq!(d.selection_bounds(), Some((10, 10, 20, 30)));
        d.select_none().unwrap();
        assert_eq!(d.selection_bounds(), None);
        // Select None without a selection records nothing.
        let label = d.undo_label().map(str::to_string);
        d.select_none().unwrap();
        assert_eq!(d.undo_label().map(str::to_string), label);
        d.invert_selection().unwrap();
        assert_eq!(d.selection_bounds(), Some((0, 0, 200, 100)));
    }

    #[test]
    fn clear_copy_and_paint_respect_the_selection() {
        let mut d = Document::new(100, 100, RED);
        d.select_region(
            "Rectangle Select",
            &rect(0.0, 0.0, 50.0, 100.0),
            SelectMode::Replace,
        )
        .unwrap();
        let (x, y, w, h, rgba) = d.copy_pixels(1).unwrap().unwrap();
        assert_eq!((x, y, w, h), (0, 0, 50, 100));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        d.clear_layer(1, "Clear").unwrap();
        assert_eq!(pixel(&d.composite(), 10, 10)[3], 0);
        assert_eq!(pixel(&d.composite(), 60, 10)[3], 255);
        d.undo();
        d.begin_stroke(
            Brush {
                size: 40.0,
                hardness: 1.0,
                color: Rgba8::new(0, 0, 255, 255),
                ..Brush::default()
            },
            "Paintbrush",
        );
        d.stroke_to(50.0, 50.0);
        d.end_stroke();
        assert_eq!(pixel(&d.composite(), 45, 50), [0, 0, 255, 255]);
        assert_eq!(pixel(&d.composite(), 55, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn crop_to_selection_resizes_and_clips() {
        let mut d = Document::new(100, 100, RED);
        let id = d
            .add_layer_from(
                "Paste",
                "Small",
                {
                    let mut s = Surface::new(10, 10);
                    s.fill(WHITE.to_array());
                    s
                },
                45,
                45,
            )
            .unwrap();
        let r = selection::ellipse(20.0, 30.0, 60.0, 40.0, true, 100, 100);
        d.select_region("Ellipse Select", &r, SelectMode::Replace)
            .unwrap();
        d.crop_to_selection().unwrap();
        assert_eq!((d.width(), d.height()), (60, 40));
        assert!(d.selection().is_none());
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (0, 0, 60, 40));
        let c = d.composite();
        assert_eq!(pixel(&c, 30, 20), [255, 255, 255, 255]);
        assert_eq!(pixel(&c, 0, 0)[3], 0); // outside the ellipse
        assert!(d.undo());
        assert_eq!((d.width(), d.height()), (100, 100));
        assert!(d.selection().is_some());
    }

    #[test]
    fn fuzzy_select_on_the_active_layer_uses_its_offset() {
        let mut d = Document::new(100, 100, WHITE);
        d.add_layer_from(
            "Paste",
            "Red",
            {
                let mut s = Surface::new(10, 10);
                s.fill(RED.to_array());
                s
            },
            20,
            20,
        )
        .unwrap();
        let m = ColorMatch {
            threshold: 15.0,
            criterion: selection::Criterion::Composite,
            select_transparent: true,
            antialias: false,
        };
        d.select_color(
            "Fuzzy Select",
            25,
            25,
            &m,
            false,
            true,
            SelectMode::Replace,
            0.0,
        )
        .unwrap();
        assert_eq!(d.selection_bounds(), Some((20, 20, 10, 10)));
        // Outside the active layer nothing happens.
        d.select_color(
            "Fuzzy Select",
            5,
            5,
            &m,
            false,
            true,
            SelectMode::Replace,
            0.0,
        )
        .unwrap();
        assert_eq!(d.selection_bounds(), Some((20, 20, 10, 10)));
        // Merged: the white background around the red square.
        d.select_color(
            "Fuzzy Select",
            5,
            5,
            &m,
            true,
            true,
            SelectMode::Replace,
            0.0,
        )
        .unwrap();
        assert_eq!(d.selection_bounds(), Some((0, 0, 100, 100)));
    }

    #[test]
    fn bucket_fill_similar_and_whole_selection() {
        let mut d = Document::new(100, 100, WHITE);
        d.select_region(
            "Rectangle Select",
            &rect(0.0, 0.0, 50.0, 100.0),
            SelectMode::Replace,
        )
        .unwrap();
        d.select_none().unwrap();
        // A red square on white; fill the white around it with blue.
        d.clear_layer(1, "Clear").unwrap();
        d.add_layer_from(
            "Paste",
            "Red",
            {
                let mut s = Surface::new(10, 10);
                s.fill(RED.to_array());
                s
            },
            40,
            40,
        )
        .unwrap();
        d.merge_visible().unwrap();
        let m = ColorMatch {
            threshold: 15.0,
            criterion: selection::Criterion::Composite,
            select_transparent: true,
            antialias: false,
        };
        let blue = Rgba8::new(0, 0, 255, 255);
        d.bucket_fill(5, 5, blue, 1.0, Some((&m, false))).unwrap();
        let c = d.composite();
        assert_eq!(pixel(&c, 5, 5)[3], 255);
        assert_eq!(pixel(&c, 45, 45), [255, 0, 0, 255]);
        // Whole selection: only the selected half.
        d.select_region(
            "Rectangle Select",
            &rect(0.0, 0.0, 50.0, 100.0),
            SelectMode::Replace,
        )
        .unwrap();
        d.bucket_fill(0, 0, WHITE, 1.0, None).unwrap();
        let c = d.composite();
        assert_eq!(pixel(&c, 45, 45), [255, 255, 255, 255]);
        assert_eq!(pixel(&c, 55, 55), [0, 0, 255, 255]);
        assert_eq!(d.undo_label(), Some("Bucket Fill"));
    }

    #[test]
    fn gradient_covers_the_layer() {
        use crate::fill::{GradientShape, Repeat};
        let mut d = Document::new(100, 10, WHITE);
        let g = Gradient {
            start: (0.0, 0.0),
            end: (100.0, 0.0),
            shape: GradientShape::Linear,
            repeat: Repeat::None,
            reverse: false,
            from: RED,
            to: Rgba8::new(0, 0, 255, 255),
        };
        d.gradient(g, 1.0).unwrap();
        let c = d.composite();
        assert_eq!(pixel(&c, 0, 5), [254, 0, 1, 255]);
        assert_eq!(pixel(&c, 99, 5), [1, 0, 254, 255]);
        assert_eq!(d.undo_label(), Some("Gradient"));
    }

    fn small(d: &mut Document, x: i32, y: i32) -> u32 {
        d.add_layer_from(
            "Paste",
            "Small",
            {
                let mut s = Surface::new(10, 20);
                s.fill(RED.to_array());
                s
            },
            x,
            y,
        )
        .unwrap()
    }

    #[test]
    fn move_layers_and_selection() {
        let mut d = Document::new(100, 100, WHITE);
        let id = small(&mut d, 5, 5);
        d.move_layers_by(&[id], 10, -3).unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y), (15, 2));
        assert_eq!(d.pick_layer(16, 3), Some(id));
        assert_eq!(d.pick_layer(50, 50), Some(1));
        let mut p = d.layer(id).unwrap().props();
        p.lock_position = true;
        d.set_layer_props(id, p, "Lock").unwrap();
        assert!(d.move_layers_by(&[id], 1, 1).is_err());
        d.select_region(
            "Rectangle Select",
            &rect(0.0, 0.0, 10.0, 10.0),
            SelectMode::Replace,
        )
        .unwrap();
        d.move_selection_by(20, 30).unwrap();
        assert_eq!(d.selection_bounds(), Some((20, 30, 10, 10)));
        assert_eq!(d.undo_label(), Some("Move Selection"));
    }

    #[test]
    fn rotate_flip_and_canvas() {
        let mut d = Document::new(100, 50, WHITE);
        let id = small(&mut d, 5, 7); // 10 x 20 at (5, 7)
        d.select_region(
            "Rectangle Select",
            &rect(0.0, 0.0, 10.0, 10.0),
            SelectMode::Replace,
        )
        .unwrap();
        d.rotate_image(1).unwrap();
        assert_eq!((d.width(), d.height()), (50, 100));
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (23, 5, 20, 10));
        assert_eq!(d.selection_bounds(), Some((40, 0, 10, 10)));
        d.rotate_image(3).unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (5, 7, 10, 20));
        d.flip_image(true).unwrap();
        assert_eq!(d.layer(id).unwrap().x, 85);
        assert_eq!(d.selection_bounds(), Some((90, 0, 10, 10)));
        d.resize_canvas(120, 60, 10, 5, ResizeLayers::ImageSized)
            .unwrap();
        let bg = d.layer(1).unwrap();
        assert_eq!((bg.x, bg.y, bg.width(), bg.height()), (0, 0, 120, 60));
        assert_eq!(pixel(&bg.surface, 5, 5)[3], 0);
        assert_eq!(pixel(&bg.surface, 15, 10)[3], 255);
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (95, 12, 10, 20));
        assert_eq!(d.selection_bounds(), Some((100, 5, 10, 10)));
        d.layer_to_image_size(id).unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (0, 0, 120, 60));
        assert_eq!(pixel(&l.surface, 96, 13), [255, 0, 0, 255]);
    }

    #[test]
    fn scale_image_scales_layers_and_positions() {
        let mut d = Document::new(100, 50, WHITE);
        let id = small(&mut d, 10, 10);
        d.scale_image(50, 25, Interpolation::Cubic).unwrap();
        assert_eq!((d.width(), d.height()), (50, 25));
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (5, 5, 5, 10));
        assert_eq!(pixel(&l.surface, 2, 5), [255, 0, 0, 255]);
        assert!(d.undo());
        assert_eq!((d.width(), d.height()), (100, 50));
    }

    #[test]
    fn filter_preview_commit_and_cancel() {
        let mut d = Document::new(10, 10, Rgba8::new(0, 0, 0, 255));
        d.take_changes();
        d.filter_preview(Filter::Invert);
        assert_eq!(pixel(&d.composite(), 1, 1), [255, 255, 255, 255]);
        assert!(!d.take_changes().is_empty());
        d.filter_preview(Filter::Desaturate {
            mode: crate::filters::DesaturateMode::Luminance,
        });
        assert_eq!(pixel(&d.composite(), 1, 1), [0, 0, 0, 255]);
        d.filter_cancel();
        assert_eq!(pixel(&d.composite(), 1, 1), [0, 0, 0, 255]);
        assert_eq!(d.undo_label(), None);
        d.filter_preview(Filter::Invert);
        d.filter_commit("Invert");
        assert_eq!(d.undo_label(), Some("Invert"));
        assert!(d.undo());
        assert_eq!(pixel(&d.composite(), 1, 1), [0, 0, 0, 255]);
        // Another change ends a preview by cancelling it.
        d.filter_preview(Filter::Invert);
        d.add_layer(Rgba8::TRANSPARENT).unwrap();
        assert_eq!(pixel(&d.composite(), 1, 1), [0, 0, 0, 255]);
    }

    /// A noisy layer (detail that blurs when resampled).
    fn noisy(d: &mut Document, w: u32, h: u32) -> u32 {
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = ((i.wrapping_mul(2654435761)) >> 24) as u8;
                [v, 255 - v, v / 2, 255]
            })
            .collect();
        d.add_layer_from("Paste", "Noise", Surface::from_rgba(w, h, &rgba), 10, 10)
            .unwrap()
    }

    #[test]
    fn repeated_transforms_start_from_the_original() {
        let mut d = Document::new(200, 200, WHITE);
        let id = noisy(&mut d, 40, 30);
        let original = d.layer(id).unwrap().surface.to_rgba();
        // Scale down to 25%, then back up 4x: rendered from the original, so 1:1 again.
        d.transform_begin().unwrap();
        let m = Matrix::translate(10.0, 10.0)
            .mul(&Matrix::scale(0.25, 0.25))
            .mul(&Matrix::translate(-10.0, -10.0));
        d.transform_commit(m, Interpolation::Cubic, "Scale")
            .unwrap();
        assert_eq!(d.layer(id).unwrap().width(), 10);
        let (x, y, _, _) = d.transform_begin().unwrap();
        let back = Matrix::translate(x as f64, y as f64)
            .mul(&Matrix::scale(4.0, 4.0))
            .mul(&Matrix::translate(-x as f64, -y as f64));
        d.transform_commit(back, Interpolation::Cubic, "Scale")
            .unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.width(), l.height()), (40, 30));
        assert_eq!(l.surface.to_rgba(), original);
        // Painting makes the layer plain pixels again.
        d.begin_stroke(Brush::default(), "Paintbrush");
        d.stroke_to(5.0, 5.0);
        d.end_stroke();
        assert!(d.layer(id).unwrap().source.is_none());
    }

    #[test]
    fn flips_and_quarter_turns_are_exact() {
        let mut d = Document::new(100, 100, WHITE);
        let id = noisy(&mut d, 20, 10);
        let original = d.layer(id).unwrap().surface.to_rgba();
        let (x, y, w, _h) = d.transform_begin().unwrap();
        let cx = x as f64 + w as f64 / 2.0;
        let flip = Matrix::translate(cx, 0.0)
            .mul(&Matrix::scale(-1.0, 1.0))
            .mul(&Matrix::translate(-cx, 0.0));
        d.transform_commit(flip, Interpolation::Cubic, "Flip")
            .unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y), (x, y));
        assert_eq!(pixel(&l.surface, 19, 0), {
            let p = &original[0..4];
            [p[0], p[1], p[2], p[3]]
        });
        d.transform_begin().unwrap();
        let r = Matrix::rotate_around(std::f64::consts::FRAC_PI_2, 50.0, 50.0);
        d.transform_commit(r, Interpolation::Cubic, "Rotate")
            .unwrap();
        let l = d.layer(id).unwrap();
        assert_eq!((l.width(), l.height()), (10, 20));
        assert!(l.surface.to_rgba().chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn transforming_a_selection_moves_pixels_and_outline() {
        let mut d = Document::new(100, 100, Rgba8::TRANSPARENT);
        d.clear_layer(1, "Clear").unwrap();
        let id = noisy(&mut d, 40, 40); // at (10, 10)
        d.select_region(
            "Rectangle Select",
            &rect(10.0, 10.0, 10.0, 10.0),
            SelectMode::Replace,
        )
        .unwrap();
        let rect_ = d.transform_begin().unwrap();
        assert_eq!(rect_, (10, 10, 10, 10));
        assert_eq!(d.transform_lifted(), Some(true));
        // While transforming, the lifted part is gone from the layer.
        assert_eq!(pixel(&d.layer(id).unwrap().surface, 2, 2)[3], 0);
        d.transform_commit(Matrix::translate(60.0, 60.0), Interpolation::Cubic, "Move")
            .unwrap();
        assert_eq!(d.selection_bounds(), Some((70, 70, 10, 10)));
        let l = d.layer(id).unwrap();
        assert_eq!((l.x, l.y, l.width(), l.height()), (10, 10, 70, 70));
        let c = d.composite();
        assert_eq!(pixel(&c, 12, 12)[3], 0);
        assert_eq!(pixel(&c, 72, 72)[3], 255);
        assert_eq!(pixel(&c, 30, 30)[3], 255);
        // Undo brings everything back; cancel restores the lifted part.
        assert!(d.undo());
        assert_eq!(d.selection_bounds(), Some((10, 10, 10, 10)));
        d.transform_begin().unwrap();
        d.transform_cancel();
        assert_eq!(pixel(&d.composite(), 12, 12)[3], 255);
    }

    #[test]
    fn results_computed_elsewhere_are_checked() {
        // A filter preview of an ended session is ignored.
        let mut d = Document::new(64, 64, RED);
        let serial = d.filter_begin();
        let (orig, _) = d.filter_original().unwrap();
        d.filter_cancel();
        assert!(!d.filter_show(serial, orig.clone()));
        let serial2 = d.filter_begin();
        assert_ne!(serial, serial2);
        let mut white = Surface::new(64, 64);
        white.fill(WHITE.to_array());
        assert!(d.filter_show(serial2, white));
        d.filter_commit("Test");
        assert_eq!(pixel(&d.layer(1).unwrap().surface, 5, 5), WHITE.to_array());
        // Scaled layers that do not match the document (it changed meanwhile) are refused.
        let wrong = vec![Surface::new(10, 10)];
        assert!(d.scale_image_with(32, 32, wrong, None).is_err());
        assert!(d.scale_image_with(32, 32, vec![red(32, 32)], None).is_ok());
        assert_eq!((d.width(), d.height()), (32, 32));
    }

    #[test]
    fn transform_job_and_commit_with_match_transform_commit() {
        let make = || {
            let mut d = Document::new(100, 100, Rgba8::TRANSPARENT);
            d.clear_layer(1, "Clear").unwrap();
            noisy(&mut d, 40, 40);
            d.select_region("Select", &rect(12.0, 12.0, 20.0, 15.0), SelectMode::Replace)
                .unwrap();
            d.transform_begin().unwrap();
            d
        };
        let m = Matrix::rotate_around(0.4, 30.0, 30.0);
        let mut a = make();
        a.transform_commit(m, Interpolation::Cubic, "Rotate")
            .unwrap();
        let mut b = make();
        let job = b.transform_job(&m, Interpolation::Cubic).unwrap();
        let (sw, sh) = (job.source.width() as usize, job.source.height() as usize);
        let r = projective::render(&job.source.to_rgba(), sw, sh, 4, &job.matrix, job.interp);
        let mask = job.mask.as_ref().unwrap();
        let mr = projective::render(&mask.to_dense(), 100, 100, 1, &m, Interpolation::Linear);
        b.transform_commit_with(m, "Rotate", r, mr).unwrap();
        assert_eq!(a.composite().to_rgba(), b.composite().to_rgba());
        assert_eq!(
            a.selection().map(|m| m.to_dense()),
            b.selection().map(|m| m.to_dense())
        );
    }

    fn red(w: u32, h: u32) -> Surface {
        let mut s = Surface::new(w, h);
        s.fill(RED.to_array());
        s
    }

    #[test]
    fn text_editing_is_one_step_and_names_follow_the_text() {
        let mut d = Document::new(100, 100, WHITE);
        let id = d.text_begin(None, 10, 20).unwrap();
        assert_eq!(d.layers().len(), 2);
        d.text_update("{\"text\":\"H\"}".into(), red(5, 8), 10, 20, "H")
            .unwrap();
        d.text_update("{\"text\":\"Hi\"}".into(), red(9, 8), 10, 20, "Hi")
            .unwrap();
        d.text_commit("Add Text Layer");
        let l = d.layer(id).unwrap();
        assert_eq!((l.name.as_str(), l.width()), ("Hi", 9));
        assert_eq!(d.layer_text(id), Some("{\"text\":\"Hi\"}"));
        assert_eq!(d.undo_label(), Some("Add Text Layer"));
        assert!(d.undo());
        assert_eq!(d.layers().len(), 1);
        assert!(d.redo());
        // Editing again continues from the layer; renaming stops the name following.
        d.text_begin(Some(id), 0, 0).unwrap();
        d.text_update("{\"text\":\"Hey\"}".into(), red(12, 8), 10, 20, "Hey")
            .unwrap();
        d.text_commit("Edit Text");
        assert_eq!(d.layer(id).unwrap().name, "Hey");
        let mut p = d.layer(id).unwrap().props();
        p.name = "Title".into();
        d.set_layer_props(id, p, "Rename Layer").unwrap();
        d.text_begin(Some(id), 0, 0).unwrap();
        d.text_update("{\"text\":\"Yo\"}".into(), red(8, 8), 10, 20, "Yo")
            .unwrap();
        d.text_commit("Edit Text");
        assert_eq!(d.layer(id).unwrap().name, "Title");
    }

    #[test]
    fn cancelled_new_text_disappears_and_painting_makes_it_plain() {
        let mut d = Document::new(100, 100, WHITE);
        d.text_begin(None, 0, 0).unwrap();
        d.text_cancel();
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.undo_label(), None);
        let id = d.text_begin(None, 0, 0).unwrap();
        d.text_update("{}".into(), red(30, 30), 0, 0, "A").unwrap();
        // Another edit ends the text editing first (as one step).
        d.add_layer(Rgba8::TRANSPARENT).unwrap();
        assert_eq!(d.text_editing(), None);
        d.set_active(id).unwrap();
        d.begin_stroke(Brush::default(), "Paintbrush");
        d.stroke_to(5.0, 5.0);
        d.end_stroke();
        assert!(d.layer(id).unwrap().text.is_none());
        assert!(d.undo());
        assert!(d.layer(id).unwrap().text.is_some());
    }

    #[test]
    fn from_layers_restores_text_and_source() {
        let layer = |name: &str, surface: Surface| LoadedLayer {
            name: name.into(),
            surface,
            x: 3,
            y: 4,
            opacity: 0.5,
            visible: false,
            lock_alpha: true,
            lock_position: false,
            text: None,
            name_from_text: false,
            source: None,
        };
        let mut text = layer("Hello", red(10, 6));
        text.text = Some("{}".into());
        text.name_from_text = true;
        let mut moved = layer("Moved", red(8, 8));
        moved.source = Some((red(4, 4), Matrix::scale(2.0, 2.0)));
        let sel = Mask::from_dense(20, 20, &[255; 400]);
        let d = Document::from_layers(
            20,
            20,
            vec![layer("Bottom", red(20, 20)), text, moved],
            1,
            Some(sel),
        )
        .unwrap();
        assert!(!d.is_dirty());
        assert_eq!(d.undo_label(), None);
        assert_eq!(d.active(), 2);
        assert_eq!(d.layer_text(2), Some("{}"));
        assert!(d.layer(2).unwrap().name_from_text);
        assert!(d.layer(3).unwrap().source.is_some());
        let b = d.layer(1).unwrap();
        assert_eq!(
            (b.x, b.y, b.opacity, b.visible, b.lock_alpha),
            (3, 4, 0.5, false, true)
        );
        assert!(d.selection().is_some());
        assert!(Document::from_layers(20, 20, Vec::new(), 0, None).is_err());
    }
}
