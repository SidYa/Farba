//! A raster layer: pixels plus the attributes shown in the Layers pad.

use std::sync::Arc;

use crate::projective::Matrix;
use crate::surface::Surface;

/// What a transformed layer was rendered from, so the next transform starts from the
/// original pixels again (no quality loss from repeated transforms). Valid only while the
/// layer still holds exactly `rendered` at `(x, y)` (any edit makes it stale).
pub struct TransformSource {
    pub original: Surface,
    /// Original pixels -> image coordinates.
    pub matrix: Matrix,
    pub rendered: Surface,
    pub x: i32,
    pub y: i32,
}

/// A text layer's editable text: the frontend's JSON description (see farba-text's `TextSpec`)
/// and the pixels it was rendered to. Valid only while the layer still holds exactly
/// `rendered` (painting or any other pixel edit makes it a plain layer).
pub struct TextData {
    pub spec: String,
    pub rendered: Surface,
}

#[derive(Clone)]
pub struct Layer {
    /// Stable identity within a document (survives reordering and undo).
    pub id: u32,
    pub name: String,
    pub surface: Surface,
    /// Position of the layer's top-left pixel in image coordinates.
    pub x: i32,
    pub y: i32,
    /// 0..=1.
    pub opacity: f32,
    pub visible: bool,
    /// Painting keeps the existing alpha (GIMP's "Lock alpha channel").
    pub lock_alpha: bool,
    /// The layer cannot be moved (GIMP's "Lock position and size").
    pub lock_position: bool,
    /// Set by the transform tools (see [`TransformSource`]).
    pub source: Option<Arc<TransformSource>>,
    /// Set for text layers.
    pub text: Option<Arc<TextData>>,
    /// The name follows the text until the layer is renamed (GIMP).
    pub name_from_text: bool,
}

impl Layer {
    pub fn new(id: u32, name: impl Into<String>, surface: Surface) -> Self {
        Self {
            id,
            name: name.into(),
            surface,
            x: 0,
            y: 0,
            opacity: 1.0,
            visible: true,
            lock_alpha: false,
            lock_position: false,
            source: None,
            text: None,
            name_from_text: false,
        }
    }

    pub fn width(&self) -> u32 {
        self.surface.width()
    }

    pub fn height(&self) -> u32 {
        self.surface.height()
    }

    /// Layer bounds `(x0, y0, x1, y1)` in image coordinates (exclusive end).
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        (
            self.x,
            self.y,
            self.x + self.width() as i32,
            self.y + self.height() as i32,
        )
    }

    pub fn props(&self) -> LayerProps {
        LayerProps {
            name: self.name.clone(),
            opacity: self.opacity,
            visible: self.visible,
            lock_alpha: self.lock_alpha,
            lock_position: self.lock_position,
            x: self.x,
            y: self.y,
        }
    }
}

/// Editable layer attributes (the Layer Properties dialog and the pad's toggles).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerProps {
    pub name: String,
    pub opacity: f32,
    pub visible: bool,
    pub lock_alpha: bool,
    pub lock_position: bool,
    pub x: i32,
    pub y: i32,
}
