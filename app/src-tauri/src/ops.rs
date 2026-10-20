//! Heavy pixel work for the page, natively on all cores: adjustments and effects (with
//! their live previews), Scale Image and the transform tools. The page sends the pixels
//! through shared memory, the same farba-core code runs here (`farba_core::par` spreads it
//! over the cores) and the result goes back to the page, which records it in the document
//! (the WASM document stays the only source of truth).

use std::sync::Mutex;

use farba_core::document::scaled_layer_size;
use farba_core::filters::{self, Filter};
use farba_core::projective::{self, Matrix};
use farba_core::selection::Mask;
use farba_core::surface::Surface;
use farba_core::transform::{self, Interpolation};

use crate::{blocking, shared};

/// The pixels an adjustment dialog previews on, kept between previews so they cross IPC
/// once per dialog.
struct FilterSource {
    key: String,
    surface: Surface,
    pos: (i32, i32),
    image: (u32, u32),
    mask: Option<Mask>,
}

static FILTER: Mutex<Option<FilterSource>> = Mutex::new(None);

fn lock() -> Result<std::sync::MutexGuard<'static, Option<FilterSource>>, String> {
    FILTER.lock().map_err(|e| e.to_string())
}

/// Stores a layer's pixels (`width` x `height` straight RGBA at `pos`, then the image-sized
/// selection when `mask`) from a lent buffer as the source of filter session `key`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn op_filter_source(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    key: String,
    width: u32,
    height: u32,
    pos: (i32, i32),
    image: (u32, u32),
    mask: bool,
) -> Result<(), String> {
    blocking(move || {
        let data = shared::take(&window, buffer_id)?;
        let len = width as usize * height as usize * 4;
        let mask_len = image.0 as usize * image.1 as usize;
        if data.len() != len + if mask { mask_len } else { 0 } {
            return Err("filter source: wrong buffer size".into());
        }
        let source = FilterSource {
            key,
            surface: Surface::from_rgba(width, height, &data[..len]),
            pos,
            image,
            mask: mask.then(|| Mask::from_dense(image.0, image.1, &data[len..])),
        };
        *lock()? = Some(source);
        Ok(())
    })
    .await
}

/// Applies filter `kind` with `params` to the source of session `key` and posts the
/// layer's pixels with the header `{ id, width, height }`.
#[tauri::command]
pub async fn op_filter(
    window: tauri::WebviewWindow,
    key: String,
    kind: u8,
    params: Vec<f32>,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let surface = {
            let guard = lock()?;
            let s = guard
                .as_ref()
                .filter(|s| s.key == key)
                .ok_or("filter source missing")?;
            let mut surface = s.surface.clone();
            let filter = Filter::from_params(kind, &params);
            filters::apply(&mut surface, s.pos, s.image, s.mask.as_ref(), filter);
            surface
        };
        let (w, h) = (surface.width(), surface.height());
        let header = serde_json::json!({ "id": id, "width": w, "height": h });
        let len = w as usize * h as usize * 4;
        shared::post_filled(&window, len, header.to_string(), |dst| {
            surface.write_rgba(dst);
            Ok(())
        })
    })
    .await
}

/// Drops the stored filter source (the dialog closed).
#[tauri::command]
pub async fn op_filter_end() -> Result<(), String> {
    // Waits (off the UI thread) for a preview still being computed.
    blocking(|| {
        *lock()? = None;
        Ok(())
    })
    .await
}

/// Scale Image: the lent buffer holds every layer's straight RGBA (`sizes`, bottom to top)
/// and, with `mask`, the image-sized selection. Posts the scaled layers (sizes from
/// `scaled_layer_size`) and selection one after another with the header `{ id, sizes }`.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn op_scale(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    sizes: Vec<(u32, u32)>,
    image: (u32, u32),
    to: (u32, u32),
    interp: u8,
    mask: bool,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let data = shared::take(&window, buffer_id)?;
        let interp = Interpolation::from_index(interp);
        let mut out = Vec::new();
        let mut out_sizes = Vec::new();
        let mut off = 0;
        for &(w, h) in &sizes {
            let len = w as usize * h as usize * 4;
            let src = data.get(off..off + len).ok_or("scale: buffer too short")?;
            off += len;
            let (nw, nh) = scaled_layer_size((w, h), image, to);
            out.push(transform::resample(
                src,
                w as usize,
                h as usize,
                4,
                nw as usize,
                nh as usize,
                interp,
            ));
            out_sizes.push((nw, nh));
        }
        if mask {
            let len = image.0 as usize * image.1 as usize;
            let src = data.get(off..off + len).ok_or("scale: buffer too short")?;
            let (w, h) = (image.0 as usize, image.1 as usize);
            out.push(transform::resample(
                src,
                w,
                h,
                1,
                to.0 as usize,
                to.1 as usize,
                interp,
            ));
        }
        drop(data);
        let header = serde_json::json!({ "id": id, "sizes": out_sizes });
        let len = out.iter().map(Vec::len).sum();
        shared::post_filled(&window, len, header.to_string(), |dst| {
            let mut off = 0;
            for o in &out {
                dst[off..off + o.len()].copy_from_slice(o);
                off += o.len();
            }
            Ok(())
        })
    })
    .await
}

/// The selection part of a transform: the image size and the user's matrix.
#[derive(serde::Deserialize)]
pub struct TransformMask {
    width: u32,
    height: u32,
    matrix: [f64; 9],
}

/// A transform tool's render: the lent buffer holds the source (`width` x `height` straight
/// RGBA) and, with `mask`, the image-sized selection. Posts the rendered pixels and then
/// the rendered selection with the header `{ id, rect, maskRect }` (`[x, y, w, h]`, null
/// when the result is too large / empty).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn op_transform(
    window: tauri::WebviewWindow,
    buffer_id: u32,
    width: u32,
    height: u32,
    matrix: [f64; 9],
    interp: u8,
    mask: Option<TransformMask>,
    id: u32,
) -> Result<(), String> {
    blocking(move || {
        let data = shared::take(&window, buffer_id)?;
        let len = width as usize * height as usize * 4;
        let src = data.get(..len).ok_or("transform: buffer too short")?;
        let (w, h) = (width as usize, height as usize);
        let interp = Interpolation::from_index(interp);
        let r = projective::render(src, w, h, 4, &Matrix(matrix), interp);
        let mr = match &mask {
            Some(m) => {
                let n = m.width as usize * m.height as usize;
                let dense = data
                    .get(len..len + n)
                    .ok_or("transform: buffer too short")?;
                let (mw, mh) = (m.width as usize, m.height as usize);
                projective::render(dense, mw, mh, 1, &Matrix(m.matrix), Interpolation::Linear)
            }
            None => None,
        };
        drop(data);
        let rect = |r: &projective::Rendered| [r.x, r.y, r.w as i32, r.h as i32];
        let header = serde_json::json!({
            "id": id,
            "rect": r.as_ref().map(rect),
            "maskRect": mr.as_ref().map(rect),
        });
        let parts = [r.map(|r| r.data), mr.map(|r| r.data)];
        let len = parts.iter().flatten().map(Vec::len).sum();
        shared::post_filled(&window, len, header.to_string(), |dst| {
            let mut off = 0;
            for p in parts.iter().flatten() {
                dst[off..off + p.len()].copy_from_slice(p);
                off += p.len();
            }
            Ok(())
        })
    })
    .await
}
