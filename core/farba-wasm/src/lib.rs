//! WASM bindings over `farba-core`. Keep this layer thin: conversion of arguments only.

use farba_core::brush::{Brush, PaintMode, Range};
use farba_core::color::Rgba8;
use farba_core::composite::premultiply_tile;
use farba_core::document::{DocError, Document, LoadedLayer, ResizeLayers, SelChanges, SourceSpec};
use farba_core::fill::{Gradient, GradientShape, Repeat};
use farba_core::filters::Filter;
use farba_core::layer::LayerProps;
use farba_core::projective::{Matrix, Rendered};
use farba_core::selection::{self, ColorMatch, Criterion, Mask, SelectMode};
use farba_core::surface::{Surface, TILE_BYTES, TILE_SIZE, Tile};
use farba_core::transform::Interpolation;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn core_version() -> String {
    farba_core::VERSION.to_string()
}

#[wasm_bindgen]
pub fn tile_size() -> u32 {
    TILE_SIZE
}

#[wasm_bindgen]
pub fn tile_bytes() -> usize {
    TILE_BYTES
}

/// The transform moving points `from` to `to` (flat `[x0, y0, x1, y1, ...]`, 1 to 4 points):
/// a row-major 3x3 matrix, or `undefined` when the points are degenerate.
#[wasm_bindgen]
pub fn matrix_from_points(from: &[f64], to: &[f64]) -> Option<Vec<f64>> {
    let pts = |v: &[f64]| -> Vec<(f64, f64)> {
        v.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect()
    };
    Matrix::from_points(&pts(from), &pts(to)).map(|m| m.0.to_vec())
}

/// Grows the WASM heap to at least `mb` megabytes ahead of time (touching every page), so
/// the first image open does not pay for heap growth. Call when the app is idle.
#[wasm_bindgen]
pub fn warm_up_heap(mb: usize) {
    let len = mb << 20;
    let mut v: Vec<u8> = Vec::with_capacity(len);
    let ptr = v.as_mut_ptr();
    for offset in (0..len).step_by(4096) {
        // SAFETY: `offset < len <= capacity`; volatile so the writes are not optimised out.
        unsafe { ptr.add(offset).write_volatile(0) };
    }
    drop(v);
}

/// An adjustment or effect from its kind and parameters (missing parameters are 0).
fn filter(kind: u8, p: &[f32]) -> Filter {
    Filter::from_params(kind, p)
}

/// 0xRRGGBBAA -> colour.
fn unpack(c: u32) -> Rgba8 {
    let [r, g, b, a] = c.to_be_bytes();
    Rgba8::new(r, g, b, a)
}

fn js_err(e: DocError) -> JsError {
    JsError::new(&e.0)
}

fn check_len(width: u32, height: u32, rgba: &[u8]) -> Result<(), JsError> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(JsError::new(
            "pixel buffer size does not match the image size",
        ));
    }
    Ok(())
}

fn set(obj: &Object, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), &value.into());
}

/// Builds a document from the layers of a layered file (`lib/layered.ts`): add the layers
/// bottom to top, then `build`.
#[wasm_bindgen]
pub struct DocBuilder {
    width: u32,
    height: u32,
    layers: Vec<LoadedLayer>,
    selection: Option<Mask>,
}

#[wasm_bindgen]
impl DocBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> DocBuilder {
        DocBuilder {
            width,
            height,
            layers: Vec::new(),
            selection: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_layer(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        x: i32,
        y: i32,
        opacity: f32,
        visible: bool,
        lock_alpha: bool,
        lock_position: bool,
        text: Option<String>,
        name_from_text: bool,
    ) -> Result<(), JsError> {
        check_len(width, height, rgba)?;
        self.layers.push(LoadedLayer {
            name: name.to_string(),
            surface: Surface::from_rgba(width, height, rgba),
            x,
            y,
            opacity,
            visible,
            lock_alpha,
            lock_position,
            text,
            name_from_text,
            source: None,
        });
        Ok(())
    }

    /// Changes the image size (when it is known only after the layers, e.g. PDF pages).
    pub fn set_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// The transform source of the last added layer (original pixels + 3x3 matrix).
    pub fn set_source(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
        matrix: &[f64],
    ) -> Result<(), JsError> {
        check_len(width, height, rgba)?;
        let (Some(l), Ok(m)) = (self.layers.last_mut(), <[f64; 9]>::try_from(matrix)) else {
            return Err(JsError::new("invalid transform source"));
        };
        l.source = Some((Surface::from_rgba(width, height, rgba), Matrix(m)));
        Ok(())
    }

    /// The selection: one coverage byte per image pixel.
    pub fn set_selection(&mut self, mask: &[u8]) -> Result<(), JsError> {
        if mask.len() != self.width as usize * self.height as usize {
            return Err(JsError::new("selection size does not match the image size"));
        }
        self.selection = Some(Mask::from_dense(self.width, self.height, mask));
        Ok(())
    }

    /// The document (`active` = index of the active layer, bottom = 0).
    pub fn build(self, active: usize) -> Result<Doc, JsError> {
        Document::from_layers(self.width, self.height, self.layers, active, self.selection)
            .map(Doc::wrap)
            .map_err(js_err)
    }
}

/// Marks a whole-layer change in [`Doc::take_changes`].
const FULL: u32 = u32::MAX;

/// An open image: the layer stack, undo history and change log (see `farba_core::document`).
#[wasm_bindgen]
pub struct Doc {
    inner: Document,
    /// Premultiplied copy of one tile for GPU upload.
    scratch: Box<Tile>,
    /// Pixels handed to JS by pointer (flattened image, layer copy).
    out: Vec<u8>,
}

impl Doc {
    fn wrap(inner: Document) -> Doc {
        Doc {
            inner,
            scratch: Box::new([0; TILE_BYTES]),
            out: Vec::new(),
        }
    }
}

#[wasm_bindgen]
impl Doc {
    /// A new image with one "Background" layer filled with the given colour.
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Doc {
        Doc::wrap(Document::new(width, height, Rgba8::new(r, g, b, a)))
    }

    /// An image whose single layer `name` holds straight RGBA8 pixels.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8], name: &str) -> Result<Doc, JsError> {
        check_len(width, height, rgba)?;
        Ok(Doc::wrap(Document::with_layer(
            Surface::from_rgba(width, height, rgba),
            name,
        )))
    }

    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.inner.width()
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.inner.height()
    }

    /// Layers bottom to top: `{ id, name, x, y, width, height, tilesX, opacity, visible,
    /// lockAlpha, lockPosition, isText }`.
    pub fn layers(&self) -> Array {
        self.inner
            .layers()
            .iter()
            .map(|l| {
                let o = Object::new();
                set(&o, "id", l.id);
                set(&o, "name", l.name.as_str());
                set(&o, "x", l.x);
                set(&o, "y", l.y);
                set(&o, "width", l.width());
                set(&o, "height", l.height());
                set(&o, "tilesX", l.surface.tiles_x());
                set(&o, "opacity", l.opacity);
                set(&o, "visible", l.visible);
                set(&o, "lockAlpha", l.lock_alpha);
                set(&o, "lockPosition", l.lock_position);
                set(&o, "isText", l.text.is_some());
                JsValue::from(o)
            })
            .collect()
    }

    #[wasm_bindgen(getter)]
    pub fn active(&self) -> u32 {
        self.inner.active()
    }

    pub fn set_active(&mut self, id: u32) -> Result<(), JsError> {
        self.inner.set_active(id).map_err(js_err)
    }

    // ---- layer operations ----

    pub fn add_layer(&mut self, r: u8, g: u8, b: u8, a: u8) -> Result<u32, JsError> {
        self.inner.add_layer(Rgba8::new(r, g, b, a)).map_err(js_err)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_layer_rgba(
        &mut self,
        label: &str,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
        x: i32,
        y: i32,
    ) -> Result<u32, JsError> {
        check_len(width, height, rgba)?;
        let surface = Surface::from_rgba(width, height, rgba);
        self.inner
            .add_layer_from(label, name, surface, x, y)
            .map_err(js_err)
    }

    pub fn delete_layers(&mut self, ids: &[u32]) -> Result<(), JsError> {
        self.inner.delete_layers(ids).map_err(js_err)
    }

    pub fn duplicate_layer(&mut self, id: u32) -> Result<u32, JsError> {
        self.inner.duplicate_layer(id).map_err(js_err)
    }

    /// Moves `ids` so the lowest lands at index `to` of the list without them (0 = bottom).
    pub fn move_layers(&mut self, ids: &[u32], to: usize) -> Result<(), JsError> {
        self.inner.move_layers(ids, to).map_err(js_err)
    }

    pub fn raise_layer(&mut self, id: u32) -> Result<(), JsError> {
        self.inner.raise_layer(id).map_err(js_err)
    }

    pub fn lower_layer(&mut self, id: u32) -> Result<(), JsError> {
        self.inner.lower_layer(id).map_err(js_err)
    }

    pub fn merge_down(&mut self, id: u32) -> Result<u32, JsError> {
        self.inner.merge_down(id).map_err(js_err)
    }

    pub fn merge_layers(&mut self, ids: &[u32]) -> Result<u32, JsError> {
        self.inner.merge_layers(ids).map_err(js_err)
    }

    pub fn merge_visible(&mut self) -> Result<u32, JsError> {
        self.inner.merge_visible().map_err(js_err)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_layer_props(
        &mut self,
        id: u32,
        name: String,
        opacity: f32,
        visible: bool,
        lock_alpha: bool,
        lock_position: bool,
        x: i32,
        y: i32,
        label: &str,
    ) -> Result<(), JsError> {
        let props = LayerProps {
            name,
            opacity,
            visible,
            lock_alpha,
            lock_position,
            x,
            y,
        };
        self.inner.set_layer_props(id, props, label).map_err(js_err)
    }

    pub fn clear_layer(&mut self, id: u32, label: &str) -> Result<(), JsError> {
        self.inner.clear_layer(id, label).map_err(js_err)
    }

    // ---- painting ----

    // Colours are packed as 0xRRGGBBAA (straight alpha).

    /// Starts a stroke on the active layer: Paintbrush / Pencil (`aliased`) or Eraser
    /// (`erase`). `spacing` is a fraction of the size.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_stroke(
        &mut self,
        size: f32,
        hardness: f32,
        opacity: f32,
        spacing: f32,
        color: u32,
        aliased: bool,
        erase: bool,
        label: &str,
    ) {
        let brush = Brush {
            size,
            hardness,
            opacity,
            spacing,
            color: unpack(color),
            aliased,
            mode: if erase {
                PaintMode::Erase
            } else {
                PaintMode::Paint
            },
            lock_alpha: false,
            ..Brush::default()
        };
        self.inner.begin_stroke(brush, label);
    }

    /// A retouching stroke. `kind`: 0 Clone, 1 Heal, 2 Smudge, 3 Blur, 4 Sharpen, 5 Dodge,
    /// 6 Burn; `range` (Dodge / Burn): 0 shadows, 1 midtones, 2 highlights; `rate`, `flow`
    /// 0..=1. Clone / Heal: `source` = the source layer id, or -1 for the merged image;
    /// `(dx, dy)` = image offset to the source, or with `fixed` the fixed source point.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_retouch(
        &mut self,
        kind: u8,
        size: f32,
        hardness: f32,
        opacity: f32,
        spacing: f32,
        rate: f32,
        flow: f32,
        range: u8,
        color: u32,
        source: i32,
        dx: f32,
        dy: f32,
        fixed: bool,
        label: &str,
    ) {
        let range = match range {
            0 => Range::Shadows,
            2 => Range::Highlights,
            _ => Range::Midtones,
        };
        let mode = match kind {
            0 => PaintMode::Clone,
            1 => PaintMode::Heal,
            2 => PaintMode::Smudge,
            3 => PaintMode::Convolve(false),
            4 => PaintMode::Convolve(true),
            5 => PaintMode::DodgeBurn(true, range),
            _ => PaintMode::DodgeBurn(false, range),
        };
        let brush = Brush {
            size,
            hardness,
            opacity,
            spacing,
            color: unpack(color),
            aliased: false,
            mode,
            lock_alpha: false,
            rate,
            flow,
        };
        let spec = (kind <= 1).then(|| SourceSpec {
            layer: (source >= 0).then_some(source as u32),
            delta: if fixed {
                (0, 0)
            } else {
                (dx.round() as i32, dy.round() as i32)
            },
            fixed: fixed.then_some((dx, dy)),
        });
        self.inner.begin_stroke_from(brush, label, spec);
    }

    /// Bucket Fill at image point `(x, y)`: `similar` fills similar colours (Fuzzy Select's
    /// parameters), otherwise the whole selection.
    #[allow(clippy::too_many_arguments)]
    pub fn bucket_fill(
        &mut self,
        x: f64,
        y: f64,
        color: u32,
        opacity: f32,
        similar: bool,
        threshold: f32,
        criterion: u8,
        fill_transparent: bool,
        merged: bool,
        antialias: bool,
    ) -> Result<(), JsError> {
        let m = ColorMatch {
            threshold,
            criterion: Criterion::from_index(criterion),
            select_transparent: fill_transparent,
            antialias,
        };
        self.inner
            .bucket_fill(
                x.floor() as i64,
                y.floor() as i64,
                unpack(color),
                opacity,
                similar.then_some((&m, merged)),
            )
            .map_err(js_err)
    }

    /// Gradient from `(x0, y0)` to `(x1, y1)` (image coordinates). `shape`: 0 linear,
    /// 1 radial; `repeat`: 0 none, 1 sawtooth, 2 triangular.
    #[allow(clippy::too_many_arguments)]
    pub fn gradient(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        from: u32,
        to: u32,
        shape: u8,
        repeat: u8,
        reverse: bool,
        opacity: f32,
    ) -> Result<(), JsError> {
        let g = Gradient {
            start: (x0, y0),
            end: (x1, y1),
            shape: if shape == 1 {
                GradientShape::Radial
            } else {
                GradientShape::Linear
            },
            repeat: match repeat {
                1 => Repeat::Sawtooth,
                2 => Repeat::Triangular,
                _ => Repeat::None,
            },
            reverse,
            from: unpack(from),
            to: unpack(to),
        };
        self.inner.gradient(g, opacity).map_err(js_err)
    }

    pub fn stroke_to(&mut self, x: f32, y: f32) {
        self.inner.stroke_to(x, y);
    }

    pub fn end_stroke(&mut self) {
        self.inner.end_stroke();
    }

    // ---- selection ----
    // `mode`: 0 replace, 1 add, 2 subtract, 3 intersect. `feather`: radius in pixels, 0 = off.

    #[allow(clippy::too_many_arguments)]
    pub fn select_rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        radius: f64,
        mode: u8,
        antialias: bool,
        feather: f32,
    ) -> Result<(), JsError> {
        let (iw, ih) = (self.inner.width(), self.inner.height());
        let r = selection::rectangle(x, y, w, h, radius, antialias, iw, ih);
        let r = selection::feather(r, feather);
        self.inner
            .select_region("Rectangle Select", &r, SelectMode::from_index(mode))
            .map_err(js_err)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn select_ellipse(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        mode: u8,
        antialias: bool,
        feather: f32,
    ) -> Result<(), JsError> {
        let (iw, ih) = (self.inner.width(), self.inner.height());
        let r = selection::feather(selection::ellipse(x, y, w, h, antialias, iw, ih), feather);
        self.inner
            .select_region("Ellipse Select", &r, SelectMode::from_index(mode))
            .map_err(js_err)
    }

    /// Free Select: `points` = `[x0, y0, x1, y1, ...]` in image coordinates.
    pub fn select_polygon(
        &mut self,
        points: &[f64],
        mode: u8,
        antialias: bool,
        feather: f32,
    ) -> Result<(), JsError> {
        let pts: Vec<(f64, f64)> = points
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (p[0], p[1]))
            .collect();
        let (iw, ih) = (self.inner.width(), self.inner.height());
        let r = selection::feather(selection::polygon(&pts, antialias, iw, ih), feather);
        self.inner
            .select_region("Free Select", &r, SelectMode::from_index(mode))
            .map_err(js_err)
    }

    /// Fuzzy Select (`contiguous`) or Select by Color. `criterion`: 0 composite, 1 red,
    /// 2 green, 3 blue, 4 alpha, 5 hue, 6 saturation, 7 value.
    #[allow(clippy::too_many_arguments)]
    pub fn select_color(
        &mut self,
        x: f64,
        y: f64,
        threshold: f32,
        criterion: u8,
        select_transparent: bool,
        merged: bool,
        contiguous: bool,
        mode: u8,
        antialias: bool,
        feather: f32,
    ) -> Result<(), JsError> {
        let m = ColorMatch {
            threshold,
            criterion: Criterion::from_index(criterion),
            select_transparent,
            antialias,
        };
        let label = if contiguous {
            "Fuzzy Select"
        } else {
            "Select by Color"
        };
        self.inner
            .select_color(
                label,
                x.floor() as i64,
                y.floor() as i64,
                &m,
                merged,
                contiguous,
                SelectMode::from_index(mode),
                feather,
            )
            .map_err(js_err)
    }

    pub fn select_all(&mut self) -> Result<(), JsError> {
        self.inner.select_all().map_err(js_err)
    }

    pub fn select_none(&mut self) -> Result<(), JsError> {
        self.inner.select_none().map_err(js_err)
    }

    pub fn invert_selection(&mut self) -> Result<(), JsError> {
        self.inner.invert_selection().map_err(js_err)
    }

    pub fn crop_to_selection(&mut self) -> Result<(), JsError> {
        self.inner.crop_to_selection().map_err(js_err)
    }

    /// `[x, y, width, height]` of the selection, or `undefined` without one.
    pub fn selection_bounds(&mut self) -> Option<Vec<u32>> {
        self.inner
            .selection_bounds()
            .map(|(x, y, w, h)| vec![x, y, w, h])
    }

    /// Selection tiles changed since the last call: `[]` none, `[0xFFFFFFFF]` all (also
    /// when the selection appeared or disappeared), otherwise tile indices.
    pub fn take_selection_changes(&mut self) -> Vec<u32> {
        match self.inner.take_selection_changes() {
            SelChanges::None => Vec::new(),
            SelChanges::All => vec![FULL],
            SelChanges::Tiles(t) => t.into_iter().collect(),
        }
    }

    /// Address of a selection tile (`TILE_SIZE` x `TILE_SIZE` bytes, coverage 0..255), or 0
    /// without a selection. Valid until the next call into the document.
    pub fn selection_tile_ptr(&self, tx: u32, ty: u32) -> usize {
        match self.inner.selection() {
            Some(m) => m.tile(tx, ty).as_ptr() as usize,
            None => 0,
        }
    }

    /// What Copy takes from a layer (see `Document::copy_pixels`): copies the pixels into
    /// the internal buffer and returns `[address, x, y, width, height]`, or `undefined`
    /// when the selection does not touch the layer. Call `free_pixels` when done.
    pub fn copy_pixels(&mut self, id: u32) -> Result<Option<Vec<f64>>, JsError> {
        let Some((x, y, w, h, rgba)) = self.inner.copy_pixels(id).map_err(js_err)? else {
            return Ok(None);
        };
        self.out = rgba;
        Ok(Some(vec![
            self.out.as_ptr() as usize as f64,
            x as f64,
            y as f64,
            w as f64,
            h as f64,
        ]))
    }

    // ---- moving and geometry ----

    pub fn move_layers_by(&mut self, ids: &[u32], dx: i32, dy: i32) -> Result<(), JsError> {
        self.inner.move_layers_by(ids, dx, dy).map_err(js_err)
    }

    pub fn move_selection_by(&mut self, dx: i32, dy: i32) -> Result<(), JsError> {
        self.inner.move_selection_by(dx, dy).map_err(js_err)
    }

    /// The Move tool's "Pick a layer" at an image point, or `undefined`.
    pub fn pick_layer(&self, x: f64, y: f64) -> Option<u32> {
        self.inner.pick_layer(x.floor() as i64, y.floor() as i64)
    }

    /// `interp`: 0 none, 1 linear, 2 cubic, 3 Lanczos.
    pub fn scale_image(&mut self, width: u32, height: u32, interp: u8) -> Result<(), JsError> {
        self.inner
            .scale_image(width, height, Interpolation::from_index(interp))
            .map_err(js_err)
    }

    /// `resize`: 0 no layers, 1 image-sized layers, 2 all layers.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        ox: i32,
        oy: i32,
        resize: u8,
    ) -> Result<(), JsError> {
        let resize = match resize {
            1 => ResizeLayers::ImageSized,
            2 => ResizeLayers::All,
            _ => ResizeLayers::None,
        };
        self.inner
            .resize_canvas(width, height, ox, oy, resize)
            .map_err(js_err)
    }

    pub fn flip_image(&mut self, horizontal: bool) -> Result<(), JsError> {
        self.inner.flip_image(horizontal).map_err(js_err)
    }

    /// Quarter turns clockwise.
    pub fn rotate_image(&mut self, turns: u32) -> Result<(), JsError> {
        self.inner.rotate_image(turns).map_err(js_err)
    }

    pub fn layer_to_image_size(&mut self, id: u32) -> Result<(), JsError> {
        self.inner.layer_to_image_size(id).map_err(js_err)
    }

    pub fn flip_layer(&mut self, id: u32, horizontal: bool) -> Result<(), JsError> {
        self.inner.flip_layer(id, horizontal).map_err(js_err)
    }

    // ---- text layers ----

    /// Starts editing a text layer (`id`) or a new one at `(x, y)`; returns its id.
    pub fn text_begin(&mut self, id: Option<u32>, x: i32, y: i32) -> Result<u32, JsError> {
        self.inner.text_begin(id, x, y).map_err(js_err)
    }

    /// Puts newly rendered text (straight RGBA8, `width` x `height` at `(x, y)`) on the layer
    /// being edited, with its description `spec` and the name derived from it.
    #[allow(clippy::too_many_arguments)]
    pub fn text_update(
        &mut self,
        spec: String,
        width: u32,
        height: u32,
        rgba: &[u8],
        x: i32,
        y: i32,
        name: &str,
    ) -> Result<(), JsError> {
        check_len(width, height, rgba)?;
        let surface = Surface::from_rgba(width, height, rgba);
        self.inner
            .text_update(spec, surface, x, y, name)
            .map_err(js_err)
    }

    pub fn text_commit(&mut self, label: &str) {
        self.inner.text_commit(label);
    }

    pub fn text_cancel(&mut self) {
        self.inner.text_cancel();
    }

    /// The layer whose text is being edited, or `undefined`.
    #[wasm_bindgen(getter)]
    pub fn text_editing(&self) -> Option<u32> {
        self.inner.text_editing()
    }

    /// A text layer's description (JSON), or `undefined` for other layers.
    pub fn layer_text(&self, id: u32) -> Option<String> {
        self.inner.layer_text(id).map(str::to_string)
    }

    pub fn discard_text(&mut self, id: u32) -> Result<(), JsError> {
        self.inner.discard_text(id).map_err(js_err)
    }

    // ---- transform tools ----

    /// Starts (or continues) a transform: `[x, y, width, height]` of what is transformed.
    pub fn transform_begin(&mut self) -> Result<Vec<i32>, JsError> {
        let (x, y, w, h) = self.inner.transform_begin().map_err(js_err)?;
        Ok(vec![x, y, w as i32, h as i32])
    }

    /// `true`: the selected part was lifted (the layer stays visible without it); `false`:
    /// the whole layer is transformed (hide it in the preview); `undefined`: no transform.
    pub fn transform_lifted(&self) -> Option<bool> {
        self.inner.transform_lifted()
    }

    /// Address of a tile of the pixels being transformed, premultiplied (like `tile_ptr`),
    /// or 0 without a transform.
    pub fn transform_tile_ptr(&mut self, tx: u32, ty: u32) -> usize {
        let Some(s) = self.inner.transform_float() else {
            return 0;
        };
        let tile = s.tile(tx, ty);
        if premultiply_tile(tile, &mut self.scratch[..]) {
            self.scratch.as_ptr() as usize
        } else {
            tile.as_ptr() as usize
        }
    }

    /// Applies the 3x3 row-major matrix `m` (image -> image) to the transform in progress.
    pub fn transform_commit(&mut self, m: &[f64], interp: u8, label: &str) -> Result<(), JsError> {
        let m: [f64; 9] = m
            .try_into()
            .map_err(|_| JsError::new("a transform needs 9 numbers"))?;
        self.inner
            .transform_commit(Matrix(m), Interpolation::from_index(interp), label)
            .map_err(js_err)
    }

    pub fn transform_cancel(&mut self) {
        self.inner.transform_cancel();
    }

    /// Multiplies a layer's alpha by `mask` (`width * height` bytes) as one undo step.
    pub fn multiply_alpha(
        &mut self,
        id: u32,
        width: u32,
        height: u32,
        mask: &[u8],
        label: &str,
    ) -> Result<(), JsError> {
        self.inner
            .multiply_alpha(id, width, height, mask, label)
            .map_err(js_err)
    }

    // ---- adjustments ----
    // `kind` and `params`: 0 invert; 1 exposure [stops, black level]; 2 saturation [scale];
    // 3 desaturate [mode: 0 luminance, 1 luma, 2 lightness, 3 average, 4 value]. Effects:
    // 10 black & white [red, green, blue %, contrast, sepia]; 11 enhance colours [auto,
    // vibrance]; 12 retouch [radius, smoothness, amount]; 13 reduce noise [radius,
    // strength]; 14 sharpen [radius, amount %, threshold]; 15 gaussian blur [sigma];
    // 16 vignette [amount, size, softness].

    pub fn apply_filter(&mut self, label: &str, kind: u8, params: &[f32]) -> Result<(), JsError> {
        self.inner
            .apply_filter(label, filter(kind, params))
            .map_err(js_err)
    }

    pub fn filter_preview(&mut self, kind: u8, params: &[f32]) {
        self.inner.filter_preview(filter(kind, params));
    }

    // ---- heavy work done by the native backend (see app/ui/src/lib/native.ts) ----------

    /// Starts a filter session (unless one is running) and returns its serial.
    pub fn filter_begin(&mut self) -> u32 {
        self.inner.filter_begin()
    }

    /// The pixels a filter session starts from: `[ptr, width, height, x, y]` (with `copy`,
    /// straight RGBA at `ptr` until `free_pixels`; otherwise `ptr` is 0).
    pub fn filter_original(&mut self, copy: bool) -> Option<Vec<f64>> {
        let (s, (x, y)) = self.inner.filter_original()?;
        let ptr = if copy {
            self.out = s.to_rgba();
            self.out.as_ptr() as usize
        } else {
            0
        };
        Some(vec![
            ptr as f64,
            s.width() as f64,
            s.height() as f64,
            x as f64,
            y as f64,
        ])
    }

    /// Shows filtered pixels of session `serial`; false when that session has ended.
    pub fn filter_show(&mut self, serial: u32, width: u32, height: u32, rgba: &[u8]) -> bool {
        if rgba.len() != width as usize * height as usize * 4 {
            return false;
        }
        self.inner
            .filter_show(serial, Surface::from_rgba(width, height, rgba))
    }

    /// Scale Image with layers scaled elsewhere: `sizes` = width, height per layer (bottom
    /// to top), `data` = their straight RGBA one after another; `selection` = the scaled
    /// dense selection.
    pub fn scale_image_with(
        &mut self,
        width: u32,
        height: u32,
        sizes: &[u32],
        data: &[u8],
        selection: Option<Vec<u8>>,
    ) -> Result<(), JsError> {
        let mut layers = Vec::new();
        let mut off = 0;
        for s in sizes.as_chunks::<2>().0 {
            let len = s[0] as usize * s[1] as usize * 4;
            let px = data
                .get(off..off + len)
                .ok_or_else(|| JsError::new("scaled layers are missing"))?;
            layers.push(Surface::from_rgba(s[0], s[1], px));
            off += len;
        }
        self.inner
            .scale_image_with(width, height, layers, selection)
            .map_err(js_err)
    }

    /// What applying the transform `m` renders: `[ptr, width, height, matrix (9),
    /// interpolation, has mask]`. With `copy`, at `ptr` (until `free_pixels`) are the source's straight
    /// RGBA pixels, followed by the image-sized selection when there is a mask (rendered
    /// through `m` itself, linear).
    pub fn transform_job(
        &mut self,
        m: &[f64],
        interp: u8,
        copy: bool,
    ) -> Result<Option<Vec<f64>>, JsError> {
        let m = matrix(m)?;
        let Some(job) = self
            .inner
            .transform_job(&m, Interpolation::from_index(interp))
        else {
            return Ok(None);
        };
        if copy {
            self.out = job.source.to_rgba();
            if let Some(mask) = &job.mask {
                self.out.extend_from_slice(&mask.to_dense());
            }
        }
        let mut info = vec![
            self.out.as_ptr() as usize as f64,
            job.source.width() as f64,
            job.source.height() as f64,
        ];
        info.extend_from_slice(&job.matrix.0);
        info.push(job.interp as u8 as f64);
        info.push(job.mask.is_some() as u8 as f64);
        Ok(Some(info))
    }

    /// Finishes a transform with pixels rendered elsewhere: `rect` = `[x, y, w, h]` of
    /// `data` (absent: too large), `mask_rect` / `mask` the rendered selection.
    pub fn transform_commit_with(
        &mut self,
        m: &[f64],
        label: &str,
        rect: Option<Vec<i32>>,
        data: Vec<u8>,
        mask_rect: Option<Vec<i32>>,
        mask: Vec<u8>,
    ) -> Result<(), JsError> {
        let m = matrix(m)?;
        let rendered = |r: Option<Vec<i32>>, data: Vec<u8>, ch: usize| {
            let r = r.filter(|r| r.len() == 4 && r[2] > 0 && r[3] > 0)?;
            (data.len() == r[2] as usize * r[3] as usize * ch).then(|| Rendered {
                x: r[0],
                y: r[1],
                w: r[2] as u32,
                h: r[3] as u32,
                data,
            })
        };
        let r = rendered(rect, data, 4);
        let mr = rendered(mask_rect, mask, 1);
        self.inner
            .transform_commit_with(m, label, r, mr)
            .map_err(js_err)
    }

    pub fn filter_commit(&mut self, label: &str) {
        self.inner.filter_commit(label);
    }

    pub fn filter_cancel(&mut self) {
        self.inner.filter_cancel();
    }

    // ---- history ----

    pub fn undo(&mut self) -> bool {
        self.inner.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.inner.redo()
    }

    pub fn undo_label(&self) -> Option<String> {
        self.inner.undo_label().map(str::to_string)
    }

    pub fn redo_label(&self) -> Option<String> {
        self.inner.redo_label().map(str::to_string)
    }

    #[wasm_bindgen(getter)]
    pub fn dirty(&self) -> bool {
        self.inner.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.inner.mark_saved();
    }

    pub fn set_undo_limits(&mut self, megabytes: usize, steps: usize) {
        self.inner.history().set_limits(megabytes << 20, steps);
    }

    // ---- pixels for the GPU and the backend ----

    /// Changed tiles since the last call, flattened as `[layer, count, index...]` per layer;
    /// `count == 0xFFFFFFFF` (no indices follow) means "re-upload the whole layer".
    pub fn take_changes(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        for c in self.inner.take_changes() {
            out.push(c.layer);
            match c.tiles {
                None => out.push(FULL),
                Some(tiles) => {
                    out.push(tiles.len() as u32);
                    out.extend(tiles);
                }
            }
        }
        out
    }

    /// Address of a layer tile's **premultiplied** pixels (`tile_bytes()` long, stride
    /// `tile_size() * 4`) for texture upload, or 0 if the layer does not exist. Valid until
    /// the next call into the document.
    pub fn tile_ptr(&mut self, layer: u32, tx: u32, ty: u32) -> usize {
        let Some(l) = self.inner.layer(layer) else {
            return 0;
        };
        let tile = l.surface.tile(tx, ty);
        if premultiply_tile(tile, &mut self.scratch[..]) {
            self.scratch.as_ptr() as usize
        } else {
            tile.as_ptr() as usize
        }
    }

    /// Colour at image pixel `(x, y)` as `[r, g, b, a]` for the Color Picker (see
    /// `Document::sample`), or `undefined` outside the image / active layer.
    pub fn sample(&self, x: f64, y: f64, radius: u32, merged: bool) -> Option<Vec<u8>> {
        self.inner
            .sample(x.floor() as i64, y.floor() as i64, radius, merged)
            .map(|c| c.to_array().to_vec())
    }

    /// `w` x `h` straight RGBA8 thumbnail of a layer.
    pub fn thumbnail(&self, id: u32, w: u32, h: u32) -> Result<Vec<u8>, JsError> {
        self.inner.thumbnail(id, w, h).map_err(js_err)
    }

    /// Flattens the visible layers into an internal buffer (`width * height * 4` straight
    /// RGBA8 bytes) and returns its address. Call `free_pixels` when done.
    pub fn flatten(&mut self) -> usize {
        self.out = self.inner.composite().to_rgba();
        self.out.as_ptr() as usize
    }

    /// Copies a layer's pixels (its own size) into the internal buffer and returns its
    /// address. Call `free_pixels` when done.
    pub fn layer_pixels(&mut self, id: u32) -> Result<usize, JsError> {
        let Some(l) = self.inner.layer(id) else {
            return Err(JsError::new("The layer no longer exists"));
        };
        self.out = l.surface.to_rgba();
        Ok(self.out.as_ptr() as usize)
    }

    /// Ends a stroke / preview / text or transform session in progress (before saving).
    pub fn settle(&mut self) {
        self.inner.settle();
    }

    /// What a layered file keeps besides the pixels and the pad's attributes:
    /// `{ text?, nameFromText, source?: { width, height, matrix } }`.
    pub fn layer_extra(&self, id: u32) -> Result<Object, JsError> {
        let Some(l) = self.inner.layer(id) else {
            return Err(JsError::new("The layer no longer exists"));
        };
        let o = Object::new();
        if let Some(t) = &l.text {
            set(&o, "text", t.spec.as_str());
        }
        set(&o, "nameFromText", l.name_from_text);
        if let Some(s) = &l.source {
            let src = Object::new();
            set(&src, "width", s.original.width());
            set(&src, "height", s.original.height());
            let m: Array = s.matrix.0.iter().map(|&v| JsValue::from(v)).collect();
            set(&src, "matrix", m);
            set(&o, "source", src);
        }
        Ok(o)
    }

    /// Copies a layer's transform source pixels into the internal buffer and returns its
    /// address (0 if it has none). Call `free_pixels` when done.
    pub fn source_pixels(&mut self, id: u32) -> usize {
        match self.inner.layer(id).and_then(|l| l.source.as_ref()) {
            Some(s) => {
                self.out = s.original.to_rgba();
                self.out.as_ptr() as usize
            }
            None => 0,
        }
    }

    /// Copies the selection (one byte per image pixel) into the internal buffer and returns
    /// its address (0 without a selection). Call `free_pixels` when done.
    pub fn selection_pixels(&mut self) -> usize {
        match self.inner.selection() {
            Some(m) => {
                self.out = m.to_dense();
                self.out.as_ptr() as usize
            }
            None => 0,
        }
    }

    pub fn free_pixels(&mut self) {
        self.out = Vec::new();
    }
}

fn matrix(m: &[f64]) -> Result<Matrix, JsError> {
    let m: [f64; 9] = m
        .try_into()
        .map_err(|_| JsError::new("a transform needs 9 numbers"))?;
    Ok(Matrix(m))
}
