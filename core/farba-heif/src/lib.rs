//! The HEIC add-on: opens HEIC / HEIF files with libheif + libde265 (LGPL DLLs shipped in
//! the add-on package next to this DLL and preloaded by the host; built by
//! `pnpm heif-libs`). libheif applies the image's rotation / mirroring and keeps alpha.
//!
//! Add-on C ABI version 1 (see `core/farba-ai/src/addon.rs` for the conventions). Methods:
//! - `{"method": "settings"}` -> `{"items": [...]}` (a note only);
//! - `{"method": "open", "path": "..."}` -> `{"width", "height"}` + straight RGBA8 data.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::OnceLock;

use libloading::Library;
use serde_json::{Value, json};

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

// ---- libheif (the subset used here) ----------------------------------------------------------

#[repr(C)]
struct HeifError {
    code: c_int,
    subcode: c_int,
    message: *const c_char,
}

const COLORSPACE_RGB: c_int = 1;
const CHROMA_INTERLEAVED_RGBA: c_int = 11;
const CHANNEL_INTERLEAVED: c_int = 10;
/// Largest side accepted (as Farba's).
const MAX_SIDE: usize = 32768;

type Ctx = c_void;
type Handle = c_void;
type Image = c_void;

struct Heif {
    _lib: Library,
    context_alloc: unsafe extern "C" fn() -> *mut Ctx,
    context_free: unsafe extern "C" fn(*mut Ctx),
    read_from_memory:
        unsafe extern "C" fn(*mut Ctx, *const c_void, usize, *const c_void) -> HeifError,
    primary_handle: unsafe extern "C" fn(*mut Ctx, *mut *mut Handle) -> HeifError,
    handle_release: unsafe extern "C" fn(*const Handle),
    decode_image: unsafe extern "C" fn(
        *const Handle,
        *mut *mut Image,
        c_int,
        c_int,
        *const c_void,
    ) -> HeifError,
    image_width: unsafe extern "C" fn(*const Image, c_int) -> c_int,
    image_height: unsafe extern "C" fn(*const Image, c_int) -> c_int,
    plane: unsafe extern "C" fn(*const Image, c_int, *mut usize) -> *const u8,
    image_release: unsafe extern "C" fn(*const Image),
}

// SAFETY: libheif's functions are thread-safe for separate contexts.
unsafe impl Send for Heif {}
unsafe impl Sync for Heif {}

static HEIF: OnceLock<Result<Heif, String>> = OnceLock::new();

fn heif() -> Result<&'static Heif, String> {
    HEIF.get_or_init(|| unsafe {
        // The host preloaded both DLLs from the add-on folder, so this finds that copy.
        let lib = Library::new("heif.dll").map_err(|e| format!("heif.dll: {e}"))?;
        macro_rules! sym {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes())
                    .map_err(|e| format!("{}: {e}", $name))?
            };
        }
        Ok(Heif {
            context_alloc: sym!("heif_context_alloc"),
            context_free: sym!("heif_context_free"),
            read_from_memory: sym!("heif_context_read_from_memory_without_copy"),
            primary_handle: sym!("heif_context_get_primary_image_handle"),
            handle_release: sym!("heif_image_handle_release"),
            decode_image: sym!("heif_decode_image"),
            image_width: sym!("heif_image_get_width"),
            image_height: sym!("heif_image_get_height"),
            plane: sym!("heif_image_get_plane_readonly2"),
            image_release: sym!("heif_image_release"),
            _lib: lib,
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn check(e: HeifError) -> Result<(), String> {
    if e.code == 0 {
        return Ok(());
    }
    let msg = if e.message.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(e.message) }
            .to_string_lossy()
            .into_owned()
    };
    Err(if msg.is_empty() {
        format!("libheif error {}.{}", e.code, e.subcode)
    } else {
        msg
    })
}

/// Decodes the primary image of a HEIF file (in memory) to straight RGBA8, with its
/// transformations applied.
pub fn decode(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let h = heif()?;
    unsafe {
        let ctx = (h.context_alloc)();
        if ctx.is_null() {
            return Err("libheif: out of memory".into());
        }
        let mut handle: *mut Handle = std::ptr::null_mut();
        let mut img: *mut Image = std::ptr::null_mut();
        let result = (|| {
            check((h.read_from_memory)(
                ctx,
                data.as_ptr().cast(),
                data.len(),
                std::ptr::null(),
            ))?;
            check((h.primary_handle)(ctx, &mut handle))?;
            check((h.decode_image)(
                handle,
                &mut img,
                COLORSPACE_RGB,
                CHROMA_INTERLEAVED_RGBA,
                std::ptr::null(),
            ))?;
            let w = (h.image_width)(img, CHANNEL_INTERLEAVED);
            let ht = (h.image_height)(img, CHANNEL_INTERLEAVED);
            if w <= 0 || ht <= 0 || w as usize > MAX_SIDE || ht as usize > MAX_SIDE {
                return Err(format!("Unsupported image size {w} x {ht}"));
            }
            let (w, ht) = (w as usize, ht as usize);
            let mut stride = 0usize;
            let p = (h.plane)(img, CHANNEL_INTERLEAVED, &mut stride);
            if p.is_null() || stride < w * 4 {
                return Err("libheif returned no pixels".into());
            }
            let mut out = Vec::with_capacity(w * ht * 4);
            for y in 0..ht {
                out.extend_from_slice(std::slice::from_raw_parts(p.add(y * stride), w * 4));
            }
            Ok((w as u32, ht as u32, out))
        })();
        if !img.is_null() {
            (h.image_release)(img);
        }
        if !handle.is_null() {
            (h.handle_release)(handle);
        }
        (h.context_free)(ctx);
        result
    }
}

// ---- add-on ABI ------------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn farba_addon_api() -> u32 {
    1
}

/// # Safety
/// `dir` must point to `len` bytes (the data folder; not used).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn farba_addon_init(_dir: *const u8, _len: usize) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn farba_addon_cancel() {}

/// # Safety
/// `buf` must have been returned by this DLL and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn farba_addon_free(buf: FarbaBuf) {
    if !buf.ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

fn call(req: &[u8]) -> Result<(String, Vec<u8>), String> {
    let r: Value = serde_json::from_slice(req).map_err(|e| e.to_string())?;
    match r["method"].as_str() {
        Some("settings") => Ok((
            json!({ "items": [{
                "type": "note",
                "text": "HEIC / HEIF files open with this add-on. Saving as HEIC uses the Windows HEVC extension when it is installed.",
            }] })
            .to_string(),
            Vec::new(),
        )),
        Some("open") => {
            let path = r["path"].as_str().ok_or("No path")?;
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let (w, h, rgba) = decode(&data)?;
            Ok((json!({ "width": w, "height": h }).to_string(), rgba))
        }
        _ => Err("Unknown method".into()),
    }
}

/// # Safety
/// `req` must point to `req_len` bytes; `out_json` and `out_data` must be valid for writes.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn farba_addon_call(
    req: *const u8,
    req_len: usize,
    _input: *const u8,
    _input_len: usize,
    _progress: Option<ProgressFn>,
    _ctx: *mut c_void,
    out_json: *mut FarbaBuf,
    out_data: *mut FarbaBuf,
) -> i32 {
    let req = unsafe { std::slice::from_raw_parts(req, req_len) };
    let (code, reply, data) = match call(req) {
        Ok((reply, data)) => (0, reply, data),
        Err(e) => (1, json!({ "error": e }).to_string(), Vec::new()),
    };
    unsafe {
        *out_json = FarbaBuf::from_vec(reply.into_bytes());
        *out_data = FarbaBuf::from_vec(data);
    }
    code
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Loads the DLLs built by `pnpm heif-libs` (as the host does from the add-on folder);
    /// `None` when they are not built.
    fn preload() -> Option<Vec<Library>> {
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/heif/dist/bin");
        ["libde265.dll", "heif.dll"]
            .iter()
            .map(|f| unsafe { Library::new(bin.join(f)) }.ok())
            .collect()
    }

    #[test]
    fn opens_a_heic_made_by_windows() {
        let Some(_libs) = preload() else {
            eprintln!("skipped: run `pnpm heif-libs` first");
            return;
        };
        let rgb: Vec<u8> = (0..64 * 48u32)
            .flat_map(|i| [(i % 64 * 4) as u8, (i / 64 * 5) as u8, 128])
            .collect();
        let dir = std::env::temp_dir().join(format!("farba-heif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.heic");
        if let Err(e) = farba_io::wic::encode_heic(&path, &rgb, 64, 48, 90) {
            eprintln!("skipped: no Windows HEVC encoder ({e})");
            return;
        }
        let req = json!({ "method": "open", "path": path.to_str().unwrap() }).to_string();
        let (reply, px) = call(req.as_bytes()).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(
            (reply["width"].as_u64(), reply["height"].as_u64()),
            (Some(64), Some(48))
        );
        let i = 24 * 64 + 48;
        let close = (0..3).all(|c| (px[i * 4 + c] as i32 - rgb[i * 3 + c] as i32).abs() < 12);
        assert!(
            close,
            "{:?} vs {:?}",
            &px[i * 4..i * 4 + 4],
            &rgb[i * 3..i * 3 + 3]
        );
        assert_eq!(px[i * 4 + 3], 255);
        assert!(call(br#"{"method":"open","path":"C:\nope.heic"}"#).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
