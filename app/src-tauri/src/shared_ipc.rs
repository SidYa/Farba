//! Large pixel buffers between the backend and the page where WebView2 SharedBuffer does not
//! exist (Linux: WebKitGTK). Same API as the Windows `shared.rs`, so the commands are the
//! same on every platform; only the transport differs: the bytes travel as raw Tauri IPC
//! bodies (no JSON), see `ipc/shared.ts`.
//!
//! - backend -> page: [`post`] keeps the buffer and its JSON header by the header's `id`;
//!   after the command returns, the page fetches them with `shared_header` + `shared_data`.
//! - page -> backend: the page uploads the bytes with `shared_put` (id in the `farba-buffer`
//!   request header) and then calls a command that reads them with [`take`].

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tauri::WebviewWindow;
use tauri::ipc::{InvokeBody, Request, Response};

struct Outgoing {
    header: String,
    data: Vec<u8>,
}

/// Buffers posted to the page and not fetched yet, by id.
static OUTGOING: LazyLock<Mutex<HashMap<u32, Outgoing>>> = LazyLock::new(Default::default);
/// Buffers uploaded by the page and not taken yet, by id.
static INCOMING: LazyLock<Mutex<HashMap<u32, Vec<u8>>>> = LazyLock::new(Default::default);

fn header_id(json: &str) -> Result<u32, String> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v["id"].as_u64())
        .map(|id| id as u32)
        .ok_or_else(|| "shared buffer header without an id".to_string())
}

/// Posts `bytes` to the page; `json` (with the page's `id`) is its header.
pub fn post(_window: &WebviewWindow, bytes: Vec<u8>, json: String) -> Result<(), String> {
    let id = header_id(&json)?;
    OUTGOING.lock().map_err(|e| e.to_string())?.insert(
        id,
        Outgoing {
            header: json,
            data: bytes,
        },
    );
    Ok(())
}

/// Lets `fill` write a buffer of `len` bytes, then posts it to the page with `json`.
pub fn post_filled(
    window: &WebviewWindow,
    len: usize,
    json: String,
    fill: impl FnOnce(&mut [u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut data = vec![0; len];
    fill(&mut data)?;
    post(window, data, json)
}

/// Not used here: the page uploads its bytes with `shared_put` instead of filling a lent
/// buffer.
pub fn lend(_window: &WebviewWindow, _id: u32, _len: usize, _json: String) -> Result<(), String> {
    Err("lend_buffer needs WebView2 shared buffers".into())
}

/// Takes the bytes the page uploaded as `id`.
pub fn take(_window: &WebviewWindow, id: u32) -> Result<Vec<u8>, String> {
    INCOMING
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id)
        .ok_or_else(|| format!("no shared buffer with id {id}"))
}

/// The JSON header of a posted buffer (fetched before its data).
#[tauri::command]
pub async fn shared_header(id: u32) -> Result<String, String> {
    OUTGOING
        .lock()
        .map_err(|e| e.to_string())?
        .get(&id)
        .map(|o| o.header.clone())
        .ok_or_else(|| format!("no shared buffer with id {id}"))
}

/// The bytes of a posted buffer, as a raw response (an `ArrayBuffer` in the page); this
/// releases the buffer here.
#[tauri::command]
pub async fn shared_data(id: u32) -> Result<Response, String> {
    OUTGOING
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id)
        .map(|o| Response::new(o.data))
        .ok_or_else(|| format!("no shared buffer with id {id}"))
}

/// Receives bytes from the page (a raw body); the id is in the `farba-buffer` header.
#[tauri::command]
pub async fn shared_put(request: Request<'_>) -> Result<(), String> {
    let id = request
        .headers()
        .get("farba-buffer")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or("shared_put without a farba-buffer header")?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("shared_put expects raw bytes".into());
    };
    INCOMING
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id, bytes.clone());
    Ok(())
}
