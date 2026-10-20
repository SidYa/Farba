//! PDF commands: an import session (open with a password, page sizes, thumbnails, pages
//! rendered into shared memory) and export. The work is in farba-io.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use farba_io::pdf_read::{OpenError, PdfReader};

use crate::{blocking, shared};

/// Open import sessions. A document is used by one call at a time.
fn sessions() -> &'static Mutex<HashMap<u32, Arc<Mutex<PdfReader>>>> {
    static S: std::sync::OnceLock<Mutex<HashMap<u32, Arc<Mutex<PdfReader>>>>> =
        std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

fn get_session(id: u32) -> Result<Arc<Mutex<PdfReader>>, String> {
    sessions()
        .lock()
        .map_err(|e| e.to_string())?
        .get(&id)
        .cloned()
        .ok_or_else(|| "The PDF is no longer open".to_string())
}

#[derive(serde::Serialize)]
pub struct PdfInfo {
    session: u32,
    /// Page sizes in points.
    pages: Vec<[f64; 2]>,
}

/// Opens a PDF. Fails with the text `password-required` or `wrong-password` for a
/// protected file without the right password.
#[tauri::command]
pub async fn pdf_open(path: String, password: Option<String>) -> Result<PdfInfo, String> {
    blocking(move || {
        let reader =
            PdfReader::open(Path::new(&path), password.as_deref()).map_err(|e| match e {
                OpenError::NeedPassword => "password-required".to_string(),
                OpenError::WrongPassword => "wrong-password".to_string(),
                OpenError::Other(e) => e,
            })?;
        let pages = (0..reader.page_count())
            .map(|i| reader.page_size(i).map(|(w, h)| [w, h]))
            .collect::<Result<Vec<_>, _>>()?;
        if pages.is_empty() {
            return Err("The PDF has no pages".into());
        }
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        sessions()
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id, Arc::new(Mutex::new(reader)));
        Ok(PdfInfo { session: id, pages })
    })
    .await
}

/// A page as a PNG of `width` x `height` (the import dialog's thumbnails).
#[tauri::command]
pub async fn pdf_thumbnail(
    session: u32,
    page: u32,
    width: u32,
    height: u32,
    white: bool,
) -> Result<tauri::ipc::Response, String> {
    blocking(move || {
        let s = get_session(session)?;
        let r = s.lock().map_err(|e| e.to_string())?;
        r.render_png(page, width, height, white)
            .map(tauri::ipc::Response::new)
    })
    .await
}

/// Renders a page to `width` x `height` straight RGBA8 pixels, posted as a shared buffer with
/// the header `{ id, width, height }`.
#[tauri::command]
pub async fn pdf_render(
    window: tauri::WebviewWindow,
    session: u32,
    page: u32,
    width: u32,
    height: u32,
    white: bool,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let s = get_session(session)?;
        let rgba = s
            .lock()
            .map_err(|e| e.to_string())?
            .render(page, width, height, white)?;
        let header = serde_json::json!({ "id": id, "width": width, "height": height });
        shared::post(&window, rgba, header.to_string())
    })
    .await
}

#[tauri::command]
pub fn pdf_close(session: u32) {
    if let Ok(mut s) = sessions().lock() {
        s.remove(&session);
    }
}

/// Saves a layered image (farba-io `Stack` + its pixels from a lent buffer) as a PDF.
#[tauri::command]
pub async fn save_pdf(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    path: String,
    stack: farba_io::Stack,
    options: farba_io::pdf::PdfOptions,
) -> Result<(), String> {
    blocking(move || {
        let data = shared::take(&window, buffer_id)?;
        farba_io::pdf::write(Path::new(&path), &stack, &data, &options)
    })
    .await
}
