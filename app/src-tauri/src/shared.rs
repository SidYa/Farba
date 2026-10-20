//! Fast transfer of large pixel buffers between the backend and the webview through
//! WebView2 SharedBuffer (shared memory), in both directions.
//!
//! Tauri's `invoke` copies bodies through WebView2's resource streams: about 1 s per 90 MB
//! backend -> page and 2 s page -> backend. With a SharedBuffer the bytes are written once
//! into a mapping that both sides see; the page receives it as an `ArrayBuffer` in the
//! `chrome.webview` `sharedbufferreceived` event, together with a JSON header.
//!
//! - backend -> page: [`post`] fills a buffer and posts it.
//! - page -> backend: [`lend`] posts an empty buffer that stays registered here; the page
//!   writes into it and then calls a command that reads it with [`take`].
//!
//! WebView2 objects belong to the UI thread, so all COM calls run there via `with_webview`;
//! these functions block on that and must not be called from the UI thread itself.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;

use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE, ICoreWebView2_17, ICoreWebView2Environment12,
    ICoreWebView2SharedBuffer,
};
use windows::core::{HSTRING, Interface};

thread_local! {
    /// Buffers lent to the page, by id. Only touched on the UI thread.
    static LENT: RefCell<HashMap<u32, ICoreWebView2SharedBuffer>> = RefCell::new(HashMap::new());
    /// Buffers being filled by [`post_filled`] before they are posted, by id.
    static OUTGOING: RefCell<HashMap<u32, ICoreWebView2SharedBuffer>> = RefCell::new(HashMap::new());
}

/// Runs `f` on the UI thread with the webview and waits for its result.
fn on_ui<R: Send + 'static>(
    window: &WebviewWindow,
    f: impl FnOnce(tauri::webview::PlatformWebview) -> windows::core::Result<R> + Send + 'static,
) -> Result<R, String> {
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |webview| {
            let _ = tx.send(f(webview).map_err(|e| e.to_string()));
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

fn create_and_post(
    webview: &tauri::webview::PlatformWebview,
    len: usize,
    fill: impl FnOnce(&mut [u8]),
    json: &str,
) -> windows::core::Result<ICoreWebView2SharedBuffer> {
    unsafe {
        let env: ICoreWebView2Environment12 = webview.environment().cast()?;
        let buffer = env.CreateSharedBuffer(len as u64)?;
        let mut ptr = std::ptr::null_mut();
        buffer.Buffer(&mut ptr)?;
        fill(std::slice::from_raw_parts_mut(ptr, len));
        let core: ICoreWebView2_17 = webview.controller().CoreWebView2()?.cast()?;
        core.PostSharedBufferToScript(
            &buffer,
            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
            &HSTRING::from(json),
        )?;
        Ok(buffer)
    }
}

/// Posts `bytes` to the page; `json` is delivered as `event.additionalData`.
pub fn post(window: &WebviewWindow, bytes: Vec<u8>, json: String) -> Result<(), String> {
    post_filled(window, bytes.len(), json, |dst| {
        dst.copy_from_slice(&bytes);
        Ok(())
    })
}

/// Creates a shared buffer of `len` bytes, lets `fill` write it on the calling thread (a
/// decoder writes straight into shared memory, and the UI thread copies nothing), then posts
/// it to the page with `json`. If `fill` fails the buffer is closed and never posted.
pub fn post_filled(
    window: &WebviewWindow,
    len: usize,
    json: String,
    fill: impl FnOnce(&mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let ptr = on_ui(window, move |webview| unsafe {
        let env: ICoreWebView2Environment12 = webview.environment().cast()?;
        // WebView2 refuses empty buffers.
        let buffer = env.CreateSharedBuffer(len.max(1) as u64)?;
        let mut ptr = std::ptr::null_mut();
        buffer.Buffer(&mut ptr)?;
        OUTGOING.with_borrow_mut(|out| out.insert(id, buffer));
        Ok(ptr as usize)
    })?;
    // The mapping stays valid while the UI thread holds the buffer in OUTGOING.
    let filled = fill(unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, len) });
    let ok = filled.is_ok();
    on_ui(window, move |webview| unsafe {
        let Some(buffer) = OUTGOING.with_borrow_mut(|out| out.remove(&id)) else {
            return Ok(());
        };
        if !ok {
            return buffer.Close();
        }
        // No `Close()`: that would unmap the memory under the page's ArrayBuffer. Dropping
        // our COM reference is enough; the page releases its side with `releaseBuffer`.
        let core: ICoreWebView2_17 = webview.controller().CoreWebView2()?.cast()?;
        core.PostSharedBufferToScript(
            &buffer,
            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
            &HSTRING::from(json),
        )
    })?;
    filled
}

/// Posts an empty (zeroed) buffer of `len` bytes for the page to fill, registered as `id`.
pub fn lend(window: &WebviewWindow, id: u32, len: usize, json: String) -> Result<(), String> {
    on_ui(window, move |webview| {
        let buffer = create_and_post(&webview, len, |_| {}, &json)?;
        LENT.with_borrow_mut(|lent| lent.insert(id, buffer));
        Ok(())
    })
}

/// Copies out and releases a buffer lent with [`lend`].
pub fn take(window: &WebviewWindow, id: u32) -> Result<Vec<u8>, String> {
    // Only the lookup runs on the UI thread; the copy runs here. The buffer stays in LENT
    // (so mapped) until it is closed afterwards.
    let (ptr, len) = on_ui(window, move |_| unsafe {
        let Some(buffer) = LENT.with_borrow(|lent| lent.get(&id).cloned()) else {
            const E_INVALIDARG: windows::core::HRESULT =
                windows::core::HRESULT(0x8007_0057_u32 as i32);
            return Err(windows::core::Error::new(
                E_INVALIDARG,
                format!("no shared buffer with id {id}"),
            ));
        };
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u64;
        buffer.Buffer(&mut ptr)?;
        buffer.Size(&mut len)?;
        Ok((ptr as usize, len as usize))
    })?;
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) }.to_vec();
    on_ui(window, move |_| {
        match LENT.with_borrow_mut(|lent| lent.remove(&id)) {
            Some(buffer) => unsafe { buffer.Close() },
            None => Ok(()),
        }
    })?;
    Ok(bytes)
}
