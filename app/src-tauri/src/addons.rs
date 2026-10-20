//! Add-ons: optional features shipped as `.fba` packages (a zip with `addon.json`,
//! a DLL and the DLLs it needs), installed by the Addons Manager into
//! `<app data>/addons/<id>/`; their data (e.g. downloaded models) lives in
//! `<app data>/addon-data/<id>/`. The app knows only this generic interface:
//!
//! - the manifest declares the add-on's actions (where they appear, what they return) and
//!   the file formats it opens (`formats`);
//! - the DLL exports the C ABI described in `core/farba-ai/src/addon.rs` (version 1):
//!   JSON requests (`settings`, `settings_action`, `run`, `open`), raw pixel input / output,
//!   progress callbacks and cancel. `open` (`{"method": "open", "path"}`) answers
//!   `{"width", "height"}` with straight RGBA8 pixels.
//!
//! A loaded DLL is never unloaded while the app runs (its runtime may have threads), so
//! updating or removing a loaded add-on finishes at the next start.

use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use libloading::Library;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::{blocking, shared};

/// The interface version this app speaks.
const API: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// "header" and / or "effects".
    #[serde(default)]
    pub places: Vec<String>,
    /// What `run` returns: "alpha-mask" (one byte per pixel, multiplies the layer alpha).
    pub kind: String,
    /// Title of the progress dialog.
    #[serde(default)]
    pub title: Option<String>,
}

/// A file format an add-on opens.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFormat {
    pub name: String,
    /// Lower case, without the dot.
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    /// The add-on DLL (file name inside the package).
    pub library: String,
    /// DLLs to load first from the add-on folder (e.g. `DirectML.dll`).
    #[serde(default)]
    pub preload: Vec<String>,
    /// The library for other systems, by `std::env::consts::OS` (`linux`), so one package
    /// serves them all; Windows uses `library` / `preload` (older Farba ignores this).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub platforms: HashMap<String, PlatformLibrary>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub formats: Vec<FileFormat>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlatformLibrary {
    pub library: String,
    #[serde(default)]
    pub preload: Vec<String>,
}

impl Manifest {
    /// The library to load on this system and the libraries to load before it.
    fn native(&self) -> (&str, &[String]) {
        #[cfg(not(windows))]
        if let Some(p) = self.platforms.get(std::env::consts::OS) {
            return (&p.library, &p.preload);
        }
        (&self.library, &self.preload)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    #[serde(flatten)]
    manifest: Manifest,
    /// Removed or replaced at the next start.
    pending: Option<&'static str>,
}

#[repr(C)]
struct Buf {
    ptr: *mut u8,
    len: usize,
    cap: usize,
}

type ProgressFn = extern "C" fn(ctx: *mut c_void, json: *const u8, len: usize);
type CallFn = unsafe extern "C" fn(
    *const u8,
    usize,
    *const u8,
    usize,
    Option<ProgressFn>,
    *mut c_void,
    *mut Buf,
    *mut Buf,
) -> i32;

/// A loaded add-on DLL (kept for the life of the app).
struct Loaded {
    lib: Library,
    _preloads: Vec<Library>,
}

// SAFETY: the add-on ABI is thread-safe by contract (calls come from blocking threads,
// `cancel` from any thread).
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static LOADED: Mutex<Option<HashMap<String, Arc<Loaded>>>> = Mutex::new(None);
/// Pixels sent by the page for the next run: (width, height, straight RGBA8).
static PIXELS: Mutex<Option<(u32, u32, Vec<u8>)>> = Mutex::new(None);

fn root() -> Result<&'static Path, String> {
    ROOT.get()
        .map(|p| p.as_path())
        .ok_or_else(|| "add-ons are not initialised".into())
}

fn addons_dir() -> Result<PathBuf, String> {
    Ok(root()?.join("addons"))
}

fn data_dir(id: &str) -> Result<PathBuf, String> {
    Ok(root()?.join("addon-data").join(id))
}

/// Finishes pending removals / updates (from the last session). Call once at startup.
pub fn init(app: &tauri::AppHandle) {
    let root = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = ROOT.set(root);
    finish_pending();
}

/// Finishes removals / updates left from the last session.
fn finish_pending() {
    let Ok(dir) = addons_dir() else { return };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".remove")
            && valid_id(id)
        {
            let _ = fs::remove_dir_all(dir.join(id));
            if let Ok(d) = data_dir(id) {
                let _ = fs::remove_dir_all(d);
            }
            let _ = fs::remove_file(&p);
        } else if let Some(id) = name.strip_suffix(".new")
            && valid_id(id)
            && p.is_dir()
        {
            let _ = fs::remove_dir_all(dir.join(id));
            let _ = fs::rename(&p, dir.join(id));
        }
    }
}

fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(dir.join("addon.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("addon.json: {e}"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_loaded(id: &str) -> bool {
    LOADED
        .lock()
        .ok()
        .and_then(|m| m.as_ref().map(|m| m.contains_key(id)))
        .unwrap_or(false)
}

/// Installed add-ons.
#[tauri::command]
pub fn addons_list() -> Result<Vec<Installed>, String> {
    let dir = addons_dir()?;
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if !p.is_dir() || name.ends_with(".new") {
            continue;
        }
        if let Ok(manifest) = read_manifest(&p) {
            let pending = if dir.join(format!("{name}.remove")).exists() {
                Some("remove")
            } else if dir.join(format!("{name}.new")).exists() {
                Some("update")
            } else {
                None
            };
            out.push(Installed { manifest, pending });
        }
    }
    out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    manifest: Manifest,
    /// The add-on was in use: the new version takes effect after a restart.
    restart: bool,
}

/// Installs (or updates) an add-on from a `.fba` package.
#[tauri::command]
pub async fn addon_install(path: String) -> Result<InstallResult, String> {
    blocking(move || install(&path)).await
}

fn install(path: &str) -> Result<InstallResult, String> {
    {
        let file = fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|_| "This is not a Farba add-on package")?;
        let manifest: Manifest = {
            // At the top level (tolerating a "./" prefix).
            let n = zip.len();
            let index = (0..n)
                .find(|&i| {
                    zip.by_index(i).is_ok_and(|f| {
                        f.name()
                            .is_ok_and(|n| n.trim_start_matches("./") == "addon.json")
                    })
                })
                .ok_or("The package has no addon.json")?;
            let mut f = zip.by_index(index).map_err(|e| e.to_string())?;
            let mut s = String::new();
            f.read_to_string(&mut s).map_err(|e| e.to_string())?;
            serde_json::from_str(&s).map_err(|e| format!("addon.json: {e}"))?
        };
        if !valid_id(&manifest.id) {
            return Err("The add-on id is invalid".into());
        }
        // A package without a library for this system (only the Windows DLL) would
        // install fine here but never load.
        #[cfg(not(windows))]
        if manifest.native().0.to_lowercase().ends_with(".dll") {
            return Err(format!(
                "This {} package has no {} version",
                manifest.name,
                std::env::consts::OS
            ));
        }
        let dir = addons_dir()?;
        let restart = is_loaded(&manifest.id);
        let target = dir.join(if restart {
            format!("{}.new", manifest.id)
        } else {
            manifest.id.clone()
        });
        let _ = fs::remove_dir_all(&target);
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        // Only plain files at the top level (no paths out of the folder).
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if f.is_dir() {
                continue;
            }
            let Some(name) = f
                .enclosed_name()
                .and_then(|p| p.file_name().map(|n| n.to_owned()))
            else {
                continue;
            };
            let mut out = fs::File::create(target.join(name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
        }
        let library = manifest.native().0;
        if !target.join(library).is_file() {
            let _ = fs::remove_dir_all(&target);
            return Err(format!("The package has no {library}"));
        }
        // A reinstall cancels a pending removal.
        let _ = fs::remove_file(dir.join(format!("{}.remove", manifest.id)));
        Ok(InstallResult { manifest, restart })
    }
}

/// Removes an add-on and its data. Returns true when a restart finishes it (in use).
#[tauri::command]
pub async fn addon_uninstall(id: String) -> Result<bool, String> {
    blocking(move || {
        if !valid_id(&id) {
            return Err("The add-on id is invalid".into());
        }
        let dir = addons_dir()?;
        if is_loaded(&id) {
            fs::write(dir.join(format!("{id}.remove")), b"").map_err(|e| e.to_string())?;
            let _ = fs::remove_dir_all(dir.join(format!("{id}.new")));
            return Ok(true);
        }
        let _ = fs::remove_dir_all(dir.join(&id));
        let _ = fs::remove_dir_all(dir.join(format!("{id}.new")));
        let _ = fs::remove_dir_all(data_dir(&id)?);
        Ok(false)
    })
    .await
}

/// The loaded add-on, loading it (and initialising it) on first use.
fn load(id: &str) -> Result<Arc<Loaded>, String> {
    let mut map = LOADED.lock().map_err(|e| e.to_string())?;
    let map = map.get_or_insert_with(HashMap::new);
    if let Some(l) = map.get(id) {
        return Ok(l.clone());
    }
    if !valid_id(id) {
        return Err("The add-on id is invalid".into());
    }
    let dir = addons_dir()?.join(id);
    let manifest = read_manifest(&dir)?;
    unsafe {
        // Load the add-on's own copies of its dependencies first, so its DLL uses them.
        let (library, preload) = manifest.native();
        let mut preloads = Vec::new();
        for p in preload {
            preloads.push(Library::new(dir.join(p)).map_err(|e| format!("{p}: {e}"))?);
        }
        let lib = Library::new(dir.join(library)).map_err(|e| format!("{library}: {e}"))?;
        let api: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib
            .get(b"farba_addon_api")
            .map_err(|_| "This is not a Farba add-on (no farba_addon_api)")?;
        if api() != API {
            return Err(format!(
                "{} needs a different version of Farba (add-on interface {}, Farba {API})",
                manifest.name,
                api()
            ));
        }
        let init: libloading::Symbol<unsafe extern "C" fn(*const u8, usize) -> i32> =
            lib.get(b"farba_addon_init").map_err(|e| e.to_string())?;
        let data = data_dir(id)?;
        fs::create_dir_all(&data).map_err(|e| e.to_string())?;
        let d = data.to_string_lossy().to_string();
        if init(d.as_ptr(), d.len()) != 0 {
            return Err(format!("{} could not start", manifest.name));
        }
        let loaded = Arc::new(Loaded {
            lib,
            _preloads: preloads,
        });
        map.insert(id.to_string(), loaded.clone());
        Ok(loaded)
    }
}

extern "C" fn on_progress(ctx: *mut c_void, json: *const u8, len: usize) {
    // SAFETY: `ctx` is the `&dyn Fn(&str)` passed by `call` for the duration of the call.
    let f = unsafe { &*(ctx as *const &dyn Fn(&str)) };
    let bytes = unsafe { std::slice::from_raw_parts(json, len) };
    if let Ok(s) = std::str::from_utf8(bytes) {
        f(s);
    }
}

/// One call into an add-on: returns the reply JSON and the output data, or the error
/// ("cancelled" when stopped).
fn call(
    id: &str,
    request: &Value,
    input: &[u8],
    progress: &dyn Fn(&str),
) -> Result<(Value, Vec<u8>), String> {
    let loaded = load(id)?;
    let req = request.to_string();
    let mut out_json = Buf {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
    let mut out_data = Buf {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
    unsafe {
        let call: libloading::Symbol<CallFn> = loaded
            .lib
            .get(b"farba_addon_call")
            .map_err(|e| e.to_string())?;
        let free: libloading::Symbol<unsafe extern "C" fn(Buf)> = loaded
            .lib
            .get(b"farba_addon_free")
            .map_err(|e| e.to_string())?;
        let ctx: &dyn Fn(&str) = progress;
        let code = call(
            req.as_ptr(),
            req.len(),
            if input.is_empty() {
                std::ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            Some(on_progress),
            &ctx as *const &dyn Fn(&str) as *mut c_void,
            &mut out_json,
            &mut out_data,
        );
        let copy = |b: &Buf| {
            if b.ptr.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(b.ptr, b.len).to_vec()
            }
        };
        let reply = copy(&out_json);
        let data = copy(&out_data);
        free(out_json);
        free(out_data);
        let reply: Value = serde_json::from_slice(&reply).unwrap_or(Value::Null);
        match code {
            0 => Ok((reply, data)),
            2 => Err("cancelled".into()),
            _ => Err(reply["error"]
                .as_str()
                .unwrap_or("The add-on failed")
                .to_string()),
        }
    }
}

/// A decoded image: width, height, straight RGBA8.
pub type Decoded = (u32, u32, Vec<u8>);

/// Opens `path` with an installed add-on that declares its extension (straight RGBA8), or
/// `None` when no add-on handles it.
pub fn open_file(path: &Path) -> Option<Result<Decoded, String>> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let addon = addons_list().ok()?.into_iter().find(|a| {
        a.pending != Some("remove")
            && a.manifest
                .formats
                .iter()
                .any(|f| f.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
    })?;
    let request = serde_json::json!({ "method": "open", "path": path.to_string_lossy() });
    Some(
        call(&addon.manifest.id, &request, &[], &|_| {}).and_then(|(reply, data)| {
            let w = reply["width"].as_u64().unwrap_or(0) as u32;
            let h = reply["height"].as_u64().unwrap_or(0) as u32;
            if w == 0 || h == 0 || data.len() != w as usize * h as usize * 4 {
                return Err(format!("{}: invalid image", addon.manifest.name));
            }
            Ok((w, h, data))
        }),
    )
}

/// File extensions that installed add-ons open (for the Open dialog).
#[tauri::command]
pub fn addon_formats() -> Vec<FileFormat> {
    addons_list()
        .map(|l| {
            l.into_iter()
                .filter(|a| a.pending != Some("remove"))
                .flat_map(|a| a.manifest.formats)
                .collect()
        })
        .unwrap_or_default()
}

/// A small JSON call (settings, settings actions).
#[tauri::command]
pub async fn addon_call(id: String, request: Value) -> Result<Value, String> {
    blocking(move || call(&id, &request, &[], &|_| {}).map(|(r, _)| r)).await
}

/// Stores the layer pixels for the next `addon_run` (from a lent buffer).
#[tauri::command]
pub async fn addon_put_pixels(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    blocking(move || {
        let rgba = shared::take(&window, buffer_id)?;
        if rgba.len() != width as usize * height as usize * 4 {
            return Err("pixel buffer size does not match".into());
        }
        *PIXELS.lock().map_err(|e| e.to_string())? = Some((width, height, rgba));
        Ok(())
    })
    .await
}

/// Runs an add-on action on the pixels sent with `addon_put_pixels`; posts the output as a
/// shared buffer with the header `{ id, reply }`. Progress goes out as `addon-progress`
/// events `{ addon, text, fraction }`.
#[tauri::command]
pub async fn addon_run(
    window: tauri::WebviewWindow,
    addon: String,
    request: Value,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let (w, h, rgba) = PIXELS
            .lock()
            .map_err(|e| e.to_string())?
            .take()
            .ok_or("No pixels were sent")?;
        let mut request = request;
        request["method"] = "run".into();
        request["width"] = w.into();
        request["height"] = h.into();
        let progress = |json: &str| {
            let mut v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
            v["addon"] = addon.clone().into();
            let _ = window.emit("addon-progress", v);
        };
        let (reply, data) = call(&addon, &request, &rgba, &progress)?;
        let header = serde_json::json!({ "id": id, "reply": reply });
        shared::post(&window, data, header.to_string())
    })
    .await
}

/// Stops the add-on's run in progress.
#[tauri::command]
pub fn addon_cancel(id: String) -> Result<(), String> {
    let Some(loaded) = LOADED
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .and_then(|m| m.get(&id).cloned())
    else {
        return Ok(());
    };
    unsafe {
        if let Ok(cancel) = loaded
            .lib
            .get::<unsafe extern "C" fn()>(b"farba_addon_cancel")
        {
            cancel();
        }
    }
    Ok(())
}

/// Opens a folder of an add-on (its data) in Explorer; only folders under the app data.
#[tauri::command]
pub fn addon_open_folder(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let root = root()?;
    if !p.starts_with(root) || p.components().any(|c| c == std::path::Component::ParentDir) {
        return Err("Not an add-on folder".into());
    }
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(not(windows))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&p)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    /// One package for every system: Windows loads `library`, Linux `platforms.linux`.
    #[test]
    fn picks_the_library_of_this_system() {
        let m: Manifest = serde_json::from_str(
            r#"{ "id": "x", "name": "X", "version": "1", "library": "x.dll",
                 "preload": ["dep.dll"],
                 "platforms": { "linux": { "library": "libx.so" } } }"#,
        )
        .unwrap();
        let (library, preload) = m.native();
        if cfg!(windows) {
            assert_eq!((library, preload), ("x.dll", &["dep.dll".to_string()][..]));
        } else {
            assert_eq!((library, preload), ("libx.so", &[][..]));
        }
        // Packages without `platforms` (older ones) still parse.
        let m: Manifest = serde_json::from_str(
            r#"{ "id": "x", "name": "X", "version": "1", "library": "x.dll" }"#,
        )
        .unwrap();
        assert!(m.platforms.is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Installs the built packages (`pnpm addons`) into a temporary app folder, loads them
    /// and calls them. Skipped when the packages have not been built. The packages hold
    /// Windows DLLs.
    #[test]
    fn install_load_and_call_the_built_addon() {
        let pkg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../core/target/addons/add-ai-backrem.fba");
        if !pkg.is_file() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("farba-addons-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let _ = ROOT.set(tmp.clone());
        let r = install(pkg.to_str().unwrap()).unwrap();
        assert_eq!(r.manifest.id, "ai-background-removal");
        assert!(!r.restart);
        assert_eq!(addons_list().unwrap().len(), 1);
        let (reply, _) = call(
            "ai-background-removal",
            &serde_json::json!({ "method": "settings", "values": {} }),
            &[],
            &|_| {},
        )
        .unwrap();
        let items = reply["items"].as_array().unwrap();
        assert_eq!(items[0]["key"], "model");
        assert_eq!(items[1]["type"], "switch");
        // Loaded now: an uninstall waits for the restart.
        assert!(is_loaded("ai-background-removal"));
        let err = call(
            "ai-background-removal",
            &serde_json::json!({"method":"nope"}),
            &[],
            &|_| {},
        );
        assert_eq!(err.unwrap_err(), "Unknown method");

        // The HEIC add-on (a format add-on): opens a HEIC made with the Windows codec.
        let pkg = pkg.with_file_name("add-heic.fba");
        if !pkg.is_file() {
            return;
        }
        assert_eq!(install(pkg.to_str().unwrap()).unwrap().manifest.id, "heic");
        assert!(
            addon_formats()
                .iter()
                .any(|f| f.extensions.contains(&"heic".into()))
        );
        let heic = tmp.join("t.heic");
        let rgb = [200u8, 100, 50].repeat(32 * 16);
        if farba_io::wic::encode_heic(&heic, &rgb, 32, 16, 90).is_err() {
            return; // no Windows HEVC encoder on this PC
        }
        let (w, h, px) = open_file(&heic).unwrap().unwrap();
        assert_eq!((w, h), (32, 16));
        assert!((px[0] as i32 - 200).abs() < 12 && px[3] == 255);
        assert!(open_file(&tmp.join("a.png")).is_none());
    }
}
