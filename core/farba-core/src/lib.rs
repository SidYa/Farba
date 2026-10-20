//! Pixel core of Farba.
//!
//! This crate is compiled both natively (Tauri backend) and to WASM (webview), so it must
//! not depend on Tauri, wasm-bindgen or anything OS-specific.

pub mod brush;
pub mod color;
pub mod composite;
pub mod document;
pub mod effects;
pub mod fill;
pub mod filters;
pub mod history;
pub mod layer;
pub mod par;
pub mod projective;
pub mod selection;
pub mod surface;
pub mod transform;

/// Version of the core crate, used to check that the WASM and native builds match.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
