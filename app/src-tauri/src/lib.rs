//! Tauri backend. Commands here are thin wrappers over the native crates.
//!
//! Large pixel buffers never go through `invoke`: see `shared.rs`.

use std::time::{Duration, Instant};

use tauri::Manager;

mod addons;
#[cfg(target_os = "linux")]
mod glibc_compat;
mod io;
mod ops;
mod pdf;
#[cfg(windows)]
mod shared;
// Elsewhere (Linux: WebKitGTK) there are no WebView2 shared buffers: the same API over raw
// IPC bodies.
#[cfg(not(windows))]
#[path = "shared_ipc.rs"]
mod shared;

#[tauri::command]
fn core_version() -> &'static str {
    farba_core::VERSION
}

/// Runs blocking work (decoding, encoding, shared-buffer calls that wait for the UI thread)
/// off the async runtime.
async fn blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Decodes an image file and sends its straight RGBA8 pixels as a WebView2 shared buffer
/// with the header `{ id, width, height, decodeMs }`. A layered file (OpenRaster, PSD, XCF) also has
/// `stack` (see farba-io `Stack`; the buffer then holds all its layers), unless `flatten`
/// asks for its composite image only (open as a layer). `size` renders a vector image (SVG)
/// at that size instead of its own.
#[tauri::command]
async fn open_image(
    window: tauri::WebviewWindow,
    path: String,
    id: u32,
    flatten: Option<bool>,
    size: Option<(u32, u32)>,
) -> Result<(), String> {
    blocking(move || {
        let t = Instant::now();
        let file = std::path::Path::new(&path);
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        // Layered files: OpenRaster, and Photoshop / GIMP documents (opened, saved as .ora).
        let layered = if farba_io::ora::is_ora(file) {
            Some(farba_io::ora::read(file))
        } else if ext == "psd" || ext == "psb" {
            let resolve = |name: &str| text_engine().lock().ok()?.resolve_font(name);
            Some(farba_io::psd::read(file, &resolve))
        } else if ext == "xcf" {
            let resolve = |name: &str| text_engine().lock().ok()?.resolve_font(name);
            Some(farba_io::xcf::read(file, &resolve))
        } else {
            None
        };
        if let Some(read) = layered {
            let (stack, data) = read.map_err(|e| format!("{path}: {e}"))?;
            let mut header = serde_json::json!({
                "id": id,
                "width": stack.width,
                "height": stack.height,
            });
            let data = if flatten == Some(true) {
                farba_io::ora::flatten(&stack, &data)?
            } else {
                header["stack"] = serde_json::to_value(&stack).map_err(|e| e.to_string())?;
                data
            };
            header["decodeMs"] = (t.elapsed().as_secs_f64() * 1000.0).into();
            return shared::post(&window, data, header.to_string());
        }
        // An installed add-on that opens this format comes first (the HEIC add-on reads
        // rotation and alpha, which the Windows codec does not).
        let decoded = match addons::open_file(file).transpose()? {
            Some((w, h, rgba)) => farba_io::raster::Decoded::from_rgba(w, h, rgba),
            None => farba_io::raster::open_sized(file, size)?,
        };
        let (width, height) = (decoded.width, decoded.height);
        // The pixels are converted (and turned upright) straight into the shared buffer.
        let len = decoded.rgba_len();
        let mut header = serde_json::json!({ "id": id, "width": width, "height": height });
        header["decodeMs"] = (t.elapsed().as_secs_f64() * 1000.0).into();
        shared::post_filled(&window, len, header.to_string(), |dst| {
            decoded.write_rgba(dst)
        })
    })
    .await
}

/// The size of an SVG document in CSS pixels (96 per inch), for the import dialog.
#[tauri::command]
async fn svg_size(path: String) -> Result<(f64, f64), String> {
    blocking(move || farba_io::svg::size(std::path::Path::new(&path))).await
}

/// Lends the page an empty shared buffer of `len` bytes to fill (page -> backend transfer);
/// a later command reads it by `id` (see `ipc/shared.ts` `sendShared`).
#[tauri::command]
async fn lend_buffer(window: tauri::WebviewWindow, id: u32, len: usize) -> Result<(), String> {
    blocking(move || {
        shared::lend(
            &window,
            id,
            len,
            serde_json::json!({ "id": id }).to_string(),
        )
    })
    .await
}

/// Saves straight RGBA8 pixels from a lent buffer as PNG / JPEG / WebP.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn save_image(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    path: String,
    width: u32,
    height: u32,
    format: io::Format,
    quality: u8,
    background: [u8; 3],
) -> Result<(), String> {
    blocking(move || {
        let rgba = shared::take(&window, buffer_id)?;
        io::save(
            std::path::Path::new(&path),
            &rgba,
            width,
            height,
            format,
            quality,
            background,
        )
    })
    .await
}

/// Saves a layered image (farba-io `Stack` + its pixels from a lent buffer) as OpenRaster.
#[tauri::command]
async fn save_layered(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    path: String,
    stack: farba_io::Stack,
) -> Result<(), String> {
    blocking(move || {
        let data = shared::take(&window, buffer_id)?;
        farba_io::ora::write(std::path::Path::new(&path), &stack, &data)
    })
    .await
}

#[tauri::command]
async fn clipboard_set_image(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    blocking(move || {
        let rgba = shared::take(&window, buffer_id)?;
        io::clipboard_set(rgba, width, height)
    })
    .await
}

/// Sends the clipboard image as a shared buffer with the header `{ id, width, height }`.
#[tauri::command]
async fn clipboard_get_image(window: tauri::WebviewWindow, id: u32) -> Result<(), String> {
    blocking(move || {
        let (width, height, rgba) = io::clipboard_get()?;
        let header = serde_json::json!({ "id": id, "width": width, "height": height });
        shared::post(&window, rgba, header.to_string())
    })
    .await
}

/// The text engine (system fonts), created on first use and shared by all text commands.
fn text_engine() -> &'static std::sync::Mutex<farba_text::TextEngine> {
    static ENGINE: std::sync::OnceLock<std::sync::Mutex<farba_text::TextEngine>> =
        std::sync::OnceLock::new();
    ENGINE.get_or_init(|| std::sync::Mutex::new(farba_text::TextEngine::new()))
}

/// Font family names of the installed fonts, sorted.
#[tauri::command]
async fn text_fonts() -> Result<Vec<String>, String> {
    blocking(|| Ok(text_engine().lock().map_err(|e| e.to_string())?.families())).await
}

/// Lays out and draws a text layer; posts the straight RGBA8 pixels as a shared buffer with
/// the header `{ id, width, height, lines }` (`lines`: caret positions, see farba-text).
#[tauri::command]
async fn render_text(
    window: tauri::WebviewWindow,
    spec: farba_text::TextSpec,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let r = text_engine()
            .lock()
            .map_err(|e| e.to_string())?
            .render(&spec);
        let header = serde_json::json!({
            "id": id,
            "width": r.width,
            "height": r.height,
            "lines": r.lines,
        });
        shared::post(&window, r.rgba, header.to_string())
    })
    .await
}

/// Opens the author's PayPal donation page (About dialog) in the default browser. The
/// address is fixed here, so the page cannot open arbitrary URLs.
#[tauri::command]
fn open_donate() -> Result<(), String> {
    // `explorer <url>` opened the Documents folder instead (the `&` / `?` in the address);
    // the URL protocol handler always hands it to the default browser.
    #[cfg(windows)]
    let mut command = std::process::Command::new("rundll32");
    #[cfg(windows)]
    command.args(["url.dll,FileProtocolHandler", DONATE_URL]);
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    command.arg(DONATE_URL);
    command.spawn().map(drop).map_err(|e| e.to_string())
}

const DONATE_URL: &str =
    "https://www.paypal.com/donate/?business=sergiisidko%40gmail.com&currency_code=USD";

/// Opens the Microsoft Store page of a Windows codec extension (a missing HEIF / HEVC / RAW
/// codec). Only these product ids are accepted.
#[tauri::command]
fn open_store(product: String) -> Result<(), String> {
    const PRODUCTS: [&str; 3] = [
        "9PMMSR1CGPWG", // HEIF Image Extensions (free)
        "9NMZLZ57R3T7", // HEVC Video Extensions (about $1, often preinstalled by the PC maker)
        "9NCTDW2W1BH8", // Raw Image Extension (free)
    ];
    if !PRODUCTS.contains(&product.as_str()) {
        return Err("Unknown product".into());
    }
    std::process::Command::new("explorer")
        .arg(format!("ms-windows-store://pdp/?ProductId={product}"))
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// When the process started (`run`), for the startup log.
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Startup measurement (development): with `FARBA_STARTUP_LOG=<file>` every stage the page
/// reports is appended to the file with the time since `run` started; with
/// `FARBA_STARTUP_EXIT=1` the app quits at the first frame (repeatable timings).
#[tauri::command]
fn startup_mark(app: tauri::AppHandle, stage: String) {
    mark(&stage);
    if stage == "first-frame" && std::env::var_os("FARBA_STARTUP_EXIT").is_some() {
        app.exit(0);
    }
}

fn mark(stage: &str) {
    let Some(path) = std::env::var_os("FARBA_STARTUP_LOG") else {
        return;
    };
    let ms = STARTED
        .get()
        .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let line = format!("{stage}: {ms:.0} ms (at {epoch})\n");
    use std::io::Write;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Files passed on the command line (`farba.exe a.png b.jpg`), opened at startup.
#[tauri::command]
fn startup_files() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| std::path::Path::new(a).is_file())
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    STARTED.get_or_init(Instant::now);
    let builder = tauri::Builder::default();
    // First: a second start (a double click on an .ora / .psd / .xcf while Farba runs)
    // hands its files to this instance, which opens them as tabs, and exits. Release only:
    // the dev build (same identifier) must start even while the installed app runs.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
        let files: Vec<String> = args
            .iter()
            .skip(1)
            .map(|a| std::path::Path::new(&cwd).join(a))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        let _ = tauri::Emitter::emit(app, "open-files", files);
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            mark("setup");
            // The window starts hidden and the page shows it after its first frame (no white
            // flash). Safety net in case the page fails to load.
            // Load the system fonts in the background so the first text renders at once.
            std::thread::spawn(|| drop(text_engine().lock()));
            // PDF import outside Windows: libpdfium.so ships among the resources.
            #[cfg(not(windows))]
            if let Ok(dir) = app.path().resource_dir() {
                farba_io::pdf_read::set_library_dir(dir);
            }
            addons::init(app.handle());
            mark("addons");
            let window = app.get_webview_window("main").expect("main window");
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                let _ = window.show();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            core_version,
            open_image,
            lend_buffer,
            save_image,
            save_layered,
            pdf::pdf_open,
            pdf::pdf_thumbnail,
            pdf::pdf_render,
            pdf::pdf_close,
            pdf::save_pdf,
            clipboard_set_image,
            clipboard_get_image,
            text_fonts,
            render_text,
            addons::addons_list,
            addons::addon_install,
            addons::addon_uninstall,
            addons::addon_call,
            addons::addon_put_pixels,
            addons::addon_run,
            addons::addon_cancel,
            addons::addon_open_folder,
            addons::addon_formats,
            ops::op_filter_source,
            ops::op_filter,
            ops::op_filter_end,
            ops::op_scale,
            ops::op_transform,
            svg_size,
            open_store,
            open_donate,
            startup_files,
            startup_mark,
            #[cfg(not(windows))]
            shared::shared_header,
            #[cfg(not(windows))]
            shared::shared_data,
            #[cfg(not(windows))]
            shared::shared_put
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
