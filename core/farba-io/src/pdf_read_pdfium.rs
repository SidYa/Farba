//! PDF import outside Windows through PDFium (Chromium's PDF engine), loaded at run time
//! from `libpdfium.so` shipped with the app (see [`set_library_dir`]). Same API as the
//! Windows `pdf_read.rs`: pages rendered to straight RGBA8, transparent where the page paints
//! nothing unless filled with white.
//!
//! A `PdfDocument` borrows the library and cannot move between threads, so the reader keeps
//! the path and password and opens the document for every call (one call at a time).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use pdfium_render::prelude::*;

use crate::stack::MAX_SIDE;

#[derive(Debug)]
pub enum OpenError {
    /// The file is protected and no password was given.
    NeedPassword,
    WrongPassword,
    Other(String),
}

static LIBRARY_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The folder that holds `libpdfium.so` (the app's resources). Set once at startup, before
/// the first PDF is opened.
pub fn set_library_dir(dir: PathBuf) {
    if let Ok(mut d) = LIBRARY_DIR.lock() {
        *d = Some(dir);
    }
}

/// The library, bound on first use: `$FARBA_PDFIUM` (a file), the resources folder, then
/// the system's libpdfium.
fn pdfium() -> Result<&'static Pdfium, String> {
    static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    PDFIUM
        .get_or_init(|| {
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Some(p) = std::env::var_os("FARBA_PDFIUM") {
                candidates.push(p.into());
            }
            if let Some(dir) = LIBRARY_DIR.lock().ok().and_then(|d| d.clone()) {
                candidates.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
            }
            let bindings = candidates
                .iter()
                .filter(|p| p.is_file())
                .find_map(|p| Pdfium::bind_to_library(p).ok())
                .map(Ok)
                .unwrap_or_else(Pdfium::bind_to_system_library)
                .map_err(|e| format!("The PDF library (libpdfium) could not be loaded: {e}"))?;
            Ok(Pdfium::new(bindings))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn other(e: PdfiumError) -> String {
    e.to_string()
}

pub struct PdfReader {
    path: PathBuf,
    password: Option<String>,
    /// Page sizes in points.
    sizes: Vec<(f64, f64)>,
}

impl PdfReader {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self, OpenError> {
        let pdfium = pdfium().map_err(OpenError::Other)?;
        let doc = match pdfium.load_pdf_from_file(path, password) {
            Ok(doc) => doc,
            Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
                return Err(if password.is_some() {
                    OpenError::WrongPassword
                } else {
                    OpenError::NeedPassword
                });
            }
            Err(e) => return Err(OpenError::Other(other(e))),
        };
        let sizes = doc
            .pages()
            .iter()
            .map(|p| (p.width().value as f64, p.height().value as f64))
            .collect();
        Ok(Self {
            path: path.to_path_buf(),
            password: password.map(str::to_string),
            sizes,
        })
    }

    pub fn page_count(&self) -> u32 {
        self.sizes.len() as u32
    }

    /// Page size in points (1/72 inch), rotation and crop box applied.
    pub fn page_size(&self, index: u32) -> Result<(f64, f64), String> {
        self.sizes
            .get(index as usize)
            .copied()
            .ok_or_else(|| format!("No page {}", index + 1))
    }

    /// The page rendered to `width` x `height` pixels as an encoded PNG.
    pub fn render_png(
        &self,
        index: u32,
        width: u32,
        height: u32,
        white: bool,
    ) -> Result<Vec<u8>, String> {
        let rgba = self.render(index, width, height, white)?;
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Adaptive)
            .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        Ok(png)
    }

    /// The page rendered to `width` x `height` straight RGBA8 pixels.
    pub fn render(
        &self,
        index: u32,
        width: u32,
        height: u32,
        white: bool,
    ) -> Result<Vec<u8>, String> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(format!("Unsupported page size {width} x {height}"));
        }
        let doc = pdfium()?
            .load_pdf_from_file(&self.path, self.password.as_deref())
            .map_err(other)?;
        let page = doc.pages().get(index as PdfPageIndex).map_err(other)?;
        let config = PdfRenderConfig::new()
            .set_target_size(width as Pixels, height as Pixels)
            .set_format(PdfBitmapFormat::BGRA)
            .set_clear_color(PdfColor::new(255, 255, 255, if white { 255 } else { 0 }));
        let bitmap = page.render_with_config(&config).map_err(other)?;
        if (bitmap.width() as u32, bitmap.height() as u32) != (width, height) {
            return Err("The page was rendered at a different size".into());
        }
        Ok(bitmap.as_rgba_bytes())
    }
}
