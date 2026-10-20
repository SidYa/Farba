//! The add-on interface of this DLL (C ABI, version 1), loaded by Farba's add-on host
//! (`app/src-tauri/src/addons.rs`). Requests and replies are JSON; pixels travel as raw
//! buffers. The host knows nothing about AI: it shows the settings this add-on describes,
//! and runs its actions on the active layer.
//!
//! Exports:
//! - `farba_addon_api() -> u32`: the interface version (1).
//! - `farba_addon_init(dir, len) -> i32`: the add-on's data folder (UTF-8), once.
//! - `farba_addon_call(req, req_len, input, input_len, progress, ctx, out_json, out_data)
//!   -> i32`: 0 = ok, 1 = error (`out_json` = `{"error": "..."}`), 2 = cancelled. Methods:
//!   - `{"method": "settings", "values": {...}}` -> `{"items": [...]}` (see `settings`)
//!   - `{"method": "settings_action", "action": "delete_models"}` -> `{"message": "..."}`
//!   - `{"method": "run", "action": "remove-background", "width", "height", "values"}`
//!     with straight RGBA8 input -> a `width * height` alpha mask in `out_data` and
//!     `{"status": "..."}`. Progress: `{"text": "...", "fraction": 0..1 or -1}`.
//! - `farba_addon_cancel()`: stops the run in progress (any thread).
//! - `farba_addon_free(buf)`: frees a buffer this DLL returned.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use crate::{Ai, MODELS, RunOptions, model, new_run};

#[repr(C)]
pub struct FarbaBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl FarbaBuf {
    fn from_vec(v: Vec<u8>) -> Self {
        let mut v = std::mem::ManuallyDrop::new(v);
        Self {
            ptr: v.as_mut_ptr(),
            len: v.len(),
            cap: v.capacity(),
        }
    }
}

pub type ProgressFn = extern "C" fn(ctx: *mut c_void, json: *const u8, len: usize);

static AI: OnceLock<Mutex<Ai>> = OnceLock::new();
static CANCEL: AtomicBool = AtomicBool::new(false);
static RUN: Mutex<Option<Arc<RunOptions>>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn farba_addon_api() -> u32 {
    1
}

/// # Safety
/// `dir` must point to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn farba_addon_init(dir: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(dir, len) };
    let Ok(dir) = std::str::from_utf8(bytes) else {
        return 1;
    };
    let models = PathBuf::from(dir).join("ai-models");
    // Models no longer offered take space for nothing.
    for f in crate::RETIRED_FILES {
        let _ = std::fs::remove_file(models.join(f));
    }
    let _ = AI.set(Mutex::new(Ai::new(models)));
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn farba_addon_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
    if let Ok(run) = RUN.lock()
        && let Some(r) = run.as_ref()
    {
        let _ = r.terminate();
    }
}

/// # Safety
/// `buf` must have been returned by this DLL and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn farba_addon_free(buf: FarbaBuf) {
    if !buf.ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

/// # Safety
/// `req` / `input` must point to `req_len` / `input_len` bytes (input may be null with 0);
/// `out_json` and `out_data` must be valid for writes.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn farba_addon_call(
    req: *const u8,
    req_len: usize,
    input: *const u8,
    input_len: usize,
    progress: Option<ProgressFn>,
    ctx: *mut c_void,
    out_json: *mut FarbaBuf,
    out_data: *mut FarbaBuf,
) -> i32 {
    let req = unsafe { std::slice::from_raw_parts(req, req_len) };
    let input: &[u8] = if input.is_null() || input_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(input, input_len) }
    };
    let report = |text: &str, fraction: f64| {
        if let Some(p) = progress {
            let j = json!({ "text": text, "fraction": fraction }).to_string();
            p(ctx, j.as_ptr(), j.len());
        }
    };
    let (code, reply, data) = match call(req, input, &report) {
        Ok((reply, data)) => (0, reply, data),
        Err(e) if e == crate::CANCELLED => (2, json!({ "error": e }).to_string(), vec![]),
        Err(e) => (1, json!({ "error": e }).to_string(), vec![]),
    };
    unsafe {
        *out_json = FarbaBuf::from_vec(reply.into_bytes());
        *out_data = FarbaBuf::from_vec(data);
    }
    code
}

fn ai() -> Result<std::sync::MutexGuard<'static, Ai>, String> {
    AI.get()
        .ok_or("The add-on was not initialised")?
        .lock()
        .map_err(|e| e.to_string())
}

fn call(req: &[u8], input: &[u8], report: &dyn Fn(&str, f64)) -> Result<(String, Vec<u8>), String> {
    let r: Value = serde_json::from_slice(req).map_err(|e| e.to_string())?;
    let values = &r["values"];
    let model_id = values["model"].as_str().unwrap_or(MODELS[0].id).to_string();
    let gpu = values["gpu"].as_bool().unwrap_or(true);
    match r["method"].as_str() {
        Some("settings") => Ok((settings(&model_id, gpu)?, vec![])),
        Some("settings_action") => match r["action"].as_str() {
            Some("delete_models") => {
                let n = ai()?.delete_downloaded()?;
                let msg = format!("Deleted {n} file{}", if n == 1 { "" } else { "s" });
                Ok((json!({ "message": msg }).to_string(), vec![]))
            }
            _ => Err("Unknown settings action".into()),
        },
        Some("run") => {
            let w = r["width"].as_u64().unwrap_or(0) as usize;
            let h = r["height"].as_u64().unwrap_or(0) as usize;
            if w == 0 || h == 0 || input.len() != w * h * 4 {
                return Err("The pixels do not match the size".into());
            }
            run(input, w, h, &model_id, gpu, report)
        }
        _ => Err("Unknown method".into()),
    }
}

/// The settings the host shows for this add-on (types: select, switch, storage, note).
fn settings(model_id: &str, gpu: bool) -> Result<String, String> {
    let ai = ai()?;
    let choices: Vec<Value> = MODELS
        .iter()
        .map(|m| {
            let size = if ai.is_downloaded(m) {
                "downloaded".to_string()
            } else {
                format!("{} MB", m.size / (1 << 20))
            };
            json!({ "value": m.id, "label": format!("{} · {} · {size}", m.name, m.license) })
        })
        .collect();
    Ok(json!({
        "items": [
            { "type": "select", "key": "model", "label": "Model",
              "value": model(model_id).id, "choices": choices },
            device_item(gpu),
            { "type": "storage", "label": "Downloaded models",
              "bytes": ai.downloaded_bytes(), "folder": ai.dir().display().to_string(),
              "action": "delete_models",
              "confirm": "Delete the downloaded models? They are downloaded again when needed." },
            { "type": "note", "text": "BiRefNet models (MIT) are downloaded on first use." },
        ]
    })
    .to_string())
}

/// The GPU switch (DirectML, Windows only); elsewhere a note that the CPU is used.
fn device_item(gpu: bool) -> Value {
    if cfg!(windows) {
        json!({ "type": "switch", "key": "gpu", "label": "Use the GPU (DirectML)",
                "value": gpu, "on": "GPU", "off": "CPU" })
    } else {
        json!({ "type": "note", "text": "Runs on the CPU (the GPU is used on Windows only)." })
    }
}

fn run(
    rgba: &[u8],
    w: usize,
    h: usize,
    model_id: &str,
    gpu: bool,
    report: &dyn Fn(&str, f64),
) -> Result<(String, Vec<u8>), String> {
    CANCEL.store(false, Ordering::SeqCst);
    let m = model(model_id);
    // Download without holding the session lock.
    let (downloaded, dir) = {
        let ai = ai()?;
        (ai.is_downloaded(m), ai.dir().to_path_buf())
    };
    let path = if downloaded {
        ai()?.model_path(m)
    } else {
        let text = "Downloading the AI model (this happens only once)…";
        report(text, 0.0);
        let mut last = -1i64;
        Ai::new(dir).ensure_downloaded(
            m,
            |done, total| {
                let f = total.map_or(-1.0, |t| done as f64 / t as f64);
                let pct = (f * 100.0) as i64;
                if pct != last {
                    last = pct;
                    report(text, f);
                }
            },
            &CANCEL,
        )?
    };
    report("Finding the foreground…", -1.0);
    let run = new_run()?;
    *RUN.lock().map_err(|e| e.to_string())? = Some(run.clone());
    let r = ai()?.mask(rgba, w, h, &path, gpu, &CANCEL, &run);
    *RUN.lock().map_err(|e| e.to_string())? = None;
    let (mask, provider) = r?;
    let status = format!("Background removed using {provider}");
    Ok((json!({ "status": status }).to_string(), mask))
}
