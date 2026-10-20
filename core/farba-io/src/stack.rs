//! A layered image as it travels between the page and the backend: a small JSON header
//! ([`Stack`]) plus one raw buffer with the pixels, in this order:
//!
//! - for every layer, bottom to top: its straight RGBA8 pixels (`width * height * 4`), then,
//!   if it has a transform source, the source's pixels (`width * height * 4`);
//! - the selection mask (`image width * height` bytes) if `selection` is set.
//!
//! `app/ui/src/lib/layered.ts` reads and writes the same layout.

use farba_core::document::{Document, LoadedLayer};
use farba_core::projective::Matrix;
use farba_core::selection::Mask;
use farba_core::surface::Surface;
use serde::{Deserialize, Serialize};

/// Largest image or layer side accepted from a file.
pub const MAX_SIDE: u32 = 32768;
/// Largest total of pixel data accepted from a file (WASM memory is 4 GB).
pub const MAX_BYTES: usize = 2 << 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    pub width: u32,
    pub height: u32,
    /// Index of the active layer in `layers`.
    pub active: usize,
    /// Bottom to top.
    pub layers: Vec<StackLayer>,
    /// The buffer ends with the selection mask.
    pub selection: bool,
    /// What could not be kept when reading a file (shown to the user).
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackLayer {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub visible: bool,
    pub lock_alpha: bool,
    pub lock_position: bool,
    /// A text layer's description (farba-text `TextSpec` JSON).
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub name_from_text: bool,
    #[serde(default)]
    pub source: Option<StackSource>,
}

/// The original pixels a transformed layer was rendered from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StackSource {
    pub width: u32,
    pub height: u32,
    /// Original pixels -> image coordinates, row-major 3x3.
    pub matrix: [f64; 9],
}

/// The pixels of one layer within the buffer.
pub struct LayerPixels<'a> {
    pub rgba: &'a [u8],
    pub source: Option<&'a [u8]>,
}

fn rgba_len(w: u32, h: u32) -> usize {
    w as usize * h as usize * 4
}

impl Stack {
    /// Length of the pixel buffer that goes with this header.
    pub fn byte_len(&self) -> usize {
        let layers: usize = self
            .layers
            .iter()
            .map(|l| {
                rgba_len(l.width, l.height)
                    + l.source.as_ref().map_or(0, |s| rgba_len(s.width, s.height))
            })
            .sum();
        let sel = if self.selection {
            self.width as usize * self.height as usize
        } else {
            0
        };
        layers + sel
    }

    /// Checks sizes and splits `data` into the layers' pixels and the selection.
    pub fn split<'a>(
        &self,
        data: &'a [u8],
    ) -> Result<(Vec<LayerPixels<'a>>, Option<&'a [u8]>), String> {
        let sides = std::iter::once((self.width, self.height))
            .chain(self.layers.iter().map(|l| (l.width, l.height)))
            .chain(
                self.layers
                    .iter()
                    .filter_map(|l| l.source.as_ref().map(|s| (s.width, s.height))),
            );
        for (w, h) in sides {
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return Err(format!("Unsupported image size {w} x {h}"));
            }
        }
        if self.layers.is_empty() {
            return Err("The image has no layers".into());
        }
        if data.len() != self.byte_len() {
            return Err("The pixel buffer does not match the layers".into());
        }
        let mut rest = data;
        let mut take = |n: usize| {
            let (a, b) = rest.split_at(n);
            rest = b;
            a
        };
        let layers = self
            .layers
            .iter()
            .map(|l| LayerPixels {
                rgba: take(rgba_len(l.width, l.height)),
                source: l.source.as_ref().map(|s| take(rgba_len(s.width, s.height))),
            })
            .collect();
        let sel = self
            .selection
            .then(|| take(self.width as usize * self.height as usize));
        Ok((layers, sel))
    }

    /// Builds a core document (to composite it, or in tests).
    pub fn to_document(&self, data: &[u8]) -> Result<Document, String> {
        let (pixels, sel) = self.split(data)?;
        let layers =
            self.layers
                .iter()
                .zip(pixels)
                .map(|(l, p)| LoadedLayer {
                    name: l.name.clone(),
                    surface: Surface::from_rgba(l.width, l.height, p.rgba),
                    x: l.x,
                    y: l.y,
                    opacity: l.opacity,
                    visible: l.visible,
                    lock_alpha: l.lock_alpha,
                    lock_position: l.lock_position,
                    text: l.text.clone(),
                    name_from_text: l.name_from_text,
                    source: l.source.as_ref().zip(p.source).map(|(s, px)| {
                        (Surface::from_rgba(s.width, s.height, px), Matrix(s.matrix))
                    }),
                })
                .collect();
        let sel = sel.map(|m| Mask::from_dense(self.width, self.height, m));
        Document::from_layers(self.width, self.height, layers, self.active, sel).map_err(|e| e.0)
    }
}
