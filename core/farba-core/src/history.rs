//! Undo/redo history.
//!
//! Each step stores the whole document state from before the change. That is cheap:
//! surfaces share their tiles through `Arc`, so a snapshot costs a pointer per tile and only
//! the tiles that were written afterwards exist twice (copy-on-write). Non-pixel changes
//! (order, names, flags) cost almost nothing. Memory is estimated per step as the tile data
//! it does not share with its neighbour state, and the oldest steps are dropped above a cap.

use crate::layer::Layer;
use crate::selection::Mask;
use crate::surface::TILE_BYTES;

/// Default cap on the tile memory held only by the history.
pub const DEFAULT_LIMIT_BYTES: usize = 512 << 20;
/// Default cap on the number of undo steps.
pub const DEFAULT_MAX_STEPS: usize = 200;

/// Everything undo restores.
#[derive(Clone)]
pub struct State {
    pub width: u32,
    pub height: u32,
    /// Bottom to top.
    pub layers: Vec<Layer>,
    /// Id of the active layer.
    pub active: u32,
    /// `None` = nothing selected (operations apply to everything).
    pub selection: Option<Mask>,
}

struct Entry {
    label: String,
    state: State,
    bytes: usize,
}

pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    used: usize,
    limit: usize,
    max_steps: usize,
    /// `undo.len()` when the document was last saved; `None` when that state is gone.
    saved: Option<usize>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            used: 0,
            limit: DEFAULT_LIMIT_BYTES,
            max_steps: DEFAULT_MAX_STEPS,
            saved: Some(0),
        }
    }
}

/// Memory held by `state` that the other state (`layers`, `selection`) does not share (an
/// estimate).
fn cost(state: &State, other: &[Layer], other_sel: Option<&Mask>) -> usize {
    let sel = state
        .selection
        .as_ref()
        .map_or(0, |m| m.unshared_bytes(other_sel) + 64);
    sel + state
        .layers
        .iter()
        .map(|l| {
            let o = other.iter().find(|o| o.id == l.id);
            let tiles = (l.surface.tiles_x() * l.surface.tiles_y()) as usize;
            l.surface.unshared_bytes(o.map(|o| &o.surface)) + tiles * 8 + 128
        })
        .sum::<usize>()
}

impl History {
    /// Records a finished change: `before` is the state before it, `current` after it.
    pub fn push(
        &mut self,
        label: impl Into<String>,
        before: State,
        current: &[Layer],
        current_sel: Option<&Mask>,
    ) {
        for e in self.redo.drain(..) {
            self.used -= e.bytes;
        }
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            self.saved = None;
        }
        let bytes = cost(&before, current, current_sel);
        self.used += bytes;
        self.undo.push(Entry {
            label: label.into(),
            state: before,
            bytes,
        });
        self.trim();
    }

    /// Steps back: takes the current state, returns the one to restore.
    pub fn undo(&mut self, current: State) -> Option<State> {
        let e = self.undo.pop()?;
        self.used -= e.bytes;
        let bytes = cost(&current, &e.state.layers, e.state.selection.as_ref());
        self.used += bytes;
        self.redo.push(Entry {
            label: e.label,
            state: current,
            bytes,
        });
        Some(e.state)
    }

    /// Steps forward: takes the current state, returns the one to restore.
    pub fn redo(&mut self, current: State) -> Option<State> {
        let e = self.redo.pop()?;
        self.used -= e.bytes;
        let bytes = cost(&current, &e.state.layers, e.state.selection.as_ref());
        self.used += bytes;
        self.undo.push(Entry {
            label: e.label,
            state: current,
            bytes,
        });
        Some(e.state)
    }

    fn trim(&mut self) {
        while self.undo.len() > 1 && (self.used > self.limit || self.undo.len() > self.max_steps) {
            let e = self.undo.remove(0);
            self.used -= e.bytes;
            self.saved = match self.saved {
                Some(0) | None => None,
                Some(n) => Some(n - 1),
            };
        }
    }

    pub fn set_limits(&mut self, limit_bytes: usize, max_steps: usize) {
        self.limit = limit_bytes.max(TILE_BYTES);
        self.max_steps = max_steps.max(1);
        self.trim();
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Estimated bytes held by the history.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
    }

    /// True when the document differs from the last saved (or opened) state.
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.undo.len())
    }
}
