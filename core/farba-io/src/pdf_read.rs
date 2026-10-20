//! PDF import through Windows.Data.Pdf (built into Windows, nothing to ship). Renders pages
//! to straight RGBA8; transparent where the page paints nothing unless filled with white.
//!
//! Windows.Data.Pdf has no page labels, document title or antialiasing switch (it always
//! antialiases), so those GIMP / Pinta-plus options are not offered.

use std::path::Path;

use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Storage::StorageFile;
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::UI::Color;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{HRESULT, HSTRING};

use crate::stack::MAX_SIDE;

/// HRESULT_FROM_WIN32(ERROR_WRONG_PASSWORD): the document is encrypted and the password is
/// missing or wrong.
const WRONG_PASSWORD: HRESULT = HRESULT(0x8007_052B_u32 as i32);

#[derive(Debug)]
pub enum OpenError {
    /// The file is protected and no password was given.
    NeedPassword,
    WrongPassword,
    Other(String),
}

/// WinRT calls need COM on the calling (worker) thread.
fn com() {
    // S_FALSE / RPC_E_CHANGED_MODE mean it is already initialised, which is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn other(e: windows::core::Error) -> String {
    e.message().to_string()
}

pub struct PdfReader {
    doc: PdfDocument,
}

impl PdfReader {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self, OpenError> {
        com();
        let path = std::path::absolute(path).map_err(|e| OpenError::Other(e.to_string()))?;
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
            .and_then(|op| op.join())
            .map_err(|e| OpenError::Other(other(e)))?;
        let op = match password {
            Some(p) => PdfDocument::LoadFromFileWithPasswordAsync(&file, &HSTRING::from(p)),
            None => PdfDocument::LoadFromFileAsync(&file),
        };
        match op.and_then(|op| op.join()) {
            Ok(doc) => Ok(Self { doc }),
            Err(e) if e.code() == WRONG_PASSWORD => Err(if password.is_some() {
                OpenError::WrongPassword
            } else {
                OpenError::NeedPassword
            }),
            Err(e) => Err(OpenError::Other(other(e))),
        }
    }

    pub fn page_count(&self) -> u32 {
        self.doc.PageCount().unwrap_or(0)
    }

    /// Page size in points (1/72 inch), rotation and crop box applied.
    pub fn page_size(&self, index: u32) -> Result<(f64, f64), String> {
        com();
        let page = self.doc.GetPage(index).map_err(other)?;
        let s = page.Size().map_err(other)?;
        // Windows gives device-independent pixels (1/96 inch).
        Ok((s.Width as f64 * 0.75, s.Height as f64 * 0.75))
    }

    /// The page rendered to `width` x `height` pixels as an encoded PNG (Windows' BMP
    /// encoder would be faster but drops the alpha).
    pub fn render_png(
        &self,
        index: u32,
        width: u32,
        height: u32,
        white: bool,
    ) -> Result<Vec<u8>, String> {
        com();
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(format!("Unsupported page size {width} x {height}"));
        }
        let page = self.doc.GetPage(index).map_err(other)?;
        let opts = PdfPageRenderOptions::new().map_err(other)?;
        opts.SetDestinationWidth(width).map_err(other)?;
        opts.SetDestinationHeight(height).map_err(other)?;
        opts.SetBackgroundColor(Color {
            A: if white { 255 } else { 0 },
            R: 255,
            G: 255,
            B: 255,
        })
        .map_err(other)?;
        let stream = InMemoryRandomAccessStream::new().map_err(other)?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts)
            .and_then(|op| op.join())
            .map_err(other)?;
        let size = stream.Size().map_err(other)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(other)?)
            .map_err(other)?;
        reader
            .LoadAsync(size)
            .and_then(|op| op.join())
            .map_err(other)?;
        let mut png = vec![0u8; size as usize];
        reader.ReadBytes(&mut png).map_err(other)?;
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
        let png = self.render_png(index, width, height, white)?;
        let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        if img.dimensions() != (width, height) {
            return Err("The page was rendered at a different size".into());
        }
        Ok(img.into_raw())
    }
}
