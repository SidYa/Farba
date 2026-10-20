//! Native file formats of Farba (the Tauri backend only; not compiled to WASM).

pub mod avif;
pub mod ora;
pub mod pdf;
#[cfg(windows)]
pub mod pdf_read;
#[cfg(not(windows))]
#[path = "pdf_read_pdfium.rs"]
pub mod pdf_read;
pub mod psd;
pub mod psd_text;
pub mod raster;
pub mod stack;
pub mod svg;
#[cfg(windows)]
pub mod wic;
pub mod xcf;

pub use stack::{Stack, StackLayer, StackSource};
