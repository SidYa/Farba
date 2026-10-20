//! PDF export (portable, `pdf-writer`), with the behaviour of GIMP's PDF export as in the
//! owner's Pinta-plus: every layer is its own image object (lossless, Flate) drawn with its
//! opacity and cropped to its non-transparent pixels; optional "layers as pages", reverse
//! order, background fill, single-colour layers as rectangles, and no smoothing when a viewer
//! zooms in. One image pixel is one layer pixel at any resolution.
//!
//! PDF import is in `pdf_read.rs` (Windows.Data.Pdf).

use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, TextStr};
use serde::Deserialize;

use crate::ora::{parallel, write_atomically};
use crate::stack::Stack;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfOptions {
    /// Pixels per inch: the page is `pixels * 72 / resolution` points.
    pub resolution: f64,
    /// Draw a layer whose visible pixels all have one colour as a filled rectangle.
    pub vectorize: bool,
    /// Leave out hidden layers and layers with zero opacity (otherwise they are exported
    /// and shown, as in GIMP).
    pub ignore_hidden: bool,
    /// Fill every page with this colour first.
    pub background: Option<[u8; 3]>,
    pub layers_as_pages: bool,
    /// With `layers_as_pages`: top layer first.
    pub reverse_order: bool,
    /// Ask viewers not to smooth the pixels when they zoom in (`/Interpolate false`).
    pub no_interpolation: bool,
}

/// What one layer becomes in the file.
enum Drawn {
    Nothing,
    /// A filled rectangle `(x, y, w, h)` in image pixels with a straight RGBA colour.
    Rect([i64; 4], [u8; 4]),
    /// An image placed at `(x, y)` in image pixels: zlib-compressed RGB and alpha (`None`
    /// when opaque).
    Image {
        rect: [i64; 4],
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
}

/// The smallest rectangle `(x0, y0, x1, y1)` (exclusive) of pixels with alpha > 0 inside
/// `(cx0, cy0, cx1, cy1)` (layer coordinates), or `None`.
fn content_bounds(rgba: &[u8], w: usize, clip: [usize; 4]) -> Option<[usize; 4]> {
    let [cx0, cy0, cx1, cy1] = clip;
    let (mut x0, mut y0, mut x1, mut y1) = (cx1, cy1, cx0, cy0);
    for y in cy0..cy1 {
        let row = &rgba[(y * w + cx0) * 4..(y * w + cx1) * 4];
        let Some(first) = row.as_chunks::<4>().0.iter().position(|p| p[3] != 0) else {
            continue;
        };
        let last = row
            .as_chunks::<4>()
            .0
            .iter()
            .rposition(|p| p[3] != 0)
            .unwrap_or(first);
        x0 = x0.min(cx0 + first);
        x1 = x1.max(cx0 + last + 1);
        y0 = y0.min(y);
        y1 = y + 1;
    }
    (x0 < x1).then_some([x0, y0, x1, y1])
}

fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).map_err(|e| e.to_string())?;
    e.finish().map_err(|e| e.to_string())
}

/// Crops, checks for a single colour and compresses one layer.
fn prepare(stack: &Stack, i: usize, rgba: &[u8], vectorize: bool) -> Result<Drawn, String> {
    let l = &stack.layers[i];
    let (w, h) = (l.width as i64, l.height as i64);
    // The part of the layer inside the image.
    let clip = [
        (-l.x as i64).clamp(0, w),
        (-l.y as i64).clamp(0, h),
        (stack.width as i64 - l.x as i64).clamp(0, w),
        (stack.height as i64 - l.y as i64).clamp(0, h),
    ];
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return Ok(Drawn::Nothing);
    }
    let Some([x0, y0, x1, y1]) = content_bounds(rgba, w as usize, clip.map(|v| v as usize)) else {
        return Ok(Drawn::Nothing);
    };
    let rect = [
        l.x as i64 + x0 as i64,
        l.y as i64 + y0 as i64,
        (x1 - x0) as i64,
        (y1 - y0) as i64,
    ];
    let row = |y: usize| &rgba[(y * w as usize + x0) * 4..(y * w as usize + x1) * 4];
    if vectorize {
        let first: [u8; 4] = row(y0)[..4].try_into().expect("4 bytes");
        if (y0..y1).all(|y| row(y).as_chunks::<4>().0.iter().all(|p| *p == first)) {
            return Ok(Drawn::Rect(rect, first));
        }
    }
    let n = (x1 - x0) * (y1 - y0);
    let mut rgb = Vec::with_capacity(n * 3);
    let mut alpha = Vec::with_capacity(n);
    for y in y0..y1 {
        for p in row(y).as_chunks::<4>().0 {
            rgb.extend_from_slice(&p[..3]);
            alpha.push(p[3]);
        }
    }
    let opaque = alpha.iter().all(|&a| a == 255);
    Ok(Drawn::Image {
        rect,
        rgb: zlib(&rgb)?,
        alpha: if opaque { None } else { Some(zlib(&alpha)?) },
    })
}

/// The layers (indices, bottom to top) on each page: all on one, or one per layer with
/// "Layers as pages" (GIMP; bottom layer first unless reversed), text layers joining the
/// page below them.
fn pages(stack: &Stack, o: &PdfOptions) -> Vec<Vec<usize>> {
    let exported: Vec<usize> = (0..stack.layers.len())
        .filter(|&i| {
            let l = &stack.layers[i];
            !o.ignore_hidden || (l.visible && l.opacity > 0.0)
        })
        .collect();
    if !o.layers_as_pages {
        return vec![exported];
    }
    // A text layer is drawn on the page of the layer below it (owner request: text is an
    // overlay, not a page of its own); only a text layer with nothing below starts a page.
    let mut pages: Vec<Vec<usize>> = Vec::new();
    for i in exported {
        match pages.last_mut() {
            Some(page) if stack.layers[i].text.is_some() => page.push(i),
            _ => pages.push(vec![i]),
        }
    }
    if o.reverse_order {
        pages.reverse();
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    pages
}

/// Writes the layers of `stack` (with its pixel buffer, see [`crate::stack`]) as a PDF.
pub fn write(path: &Path, stack: &Stack, data: &[u8], o: &PdfOptions) -> Result<(), String> {
    if !(o.resolution.is_finite() && o.resolution > 0.0) {
        return Err("Invalid resolution".into());
    }
    let (pixels, _) = stack.split(data)?;
    let pages = pages(stack, o);
    let used: Vec<usize> = {
        let mut v: Vec<usize> = pages.iter().flatten().copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let drawn = parallel(&used, |&i| prepare(stack, i, pixels[i].rgba, o.vectorize))?;
    let drawn_of = |i: usize| &drawn[used.binary_search(&i).expect("prepared")];

    let s = 72.0 / o.resolution;
    let (pw, ph) = (stack.width as f64 * s, stack.height as f64 * s);
    // Image pixels -> PDF points (origin at the bottom left).
    let place = |r: [i64; 4]| {
        [
            (r[2] as f64 * s) as f32,
            0.0,
            0.0,
            (r[3] as f64 * s) as f32,
            (r[0] as f64 * s) as f32,
            ((stack.height as i64 - r[1] - r[3]) as f64 * s) as f32,
        ]
    };

    let mut pdf = Pdf::new();
    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let catalog = alloc();
    let tree = alloc();
    let info = alloc();
    let outline = (o.layers_as_pages && pages.iter().any(|p| !p.is_empty())).then(&mut alloc);

    // Image objects, shared by the pages that show them.
    let mut images = vec![None; drawn.len()];
    for (k, d) in drawn.iter().enumerate() {
        if let Drawn::Image { rect, rgb, alpha } = d {
            let id = alloc();
            let mask = alpha.as_ref().map(|_| alloc());
            let mut img = pdf.image_xobject(id, rgb);
            img.filter(Filter::FlateDecode);
            img.width(rect[2] as i32);
            img.height(rect[3] as i32);
            img.color_space().device_rgb();
            img.bits_per_component(8);
            img.interpolate(!o.no_interpolation);
            if let Some(m) = mask {
                img.s_mask(m);
            }
            img.finish();
            if let (Some(m), Some(a)) = (mask, alpha) {
                let mut sm = pdf.image_xobject(m, a);
                sm.filter(Filter::FlateDecode);
                sm.width(rect[2] as i32);
                sm.height(rect[3] as i32);
                sm.color_space().device_gray();
                sm.bits_per_component(8);
                sm.interpolate(!o.no_interpolation);
                sm.finish();
            }
            images[k] = Some(id);
        }
    }

    let mut page_ids = Vec::new();
    let mut bookmarks = Vec::new();
    for layers in &pages {
        let page_id = alloc();
        let content_id = alloc();
        page_ids.push(page_id);
        let mut content = Content::new();
        if let Some([r, g, b]) = o.background {
            content
                .set_fill_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
                .rect(0.0, 0.0, pw as f32, ph as f32)
                .fill_nonzero();
        }
        let mut xobjects = Vec::new();
        let mut states = Vec::new();
        for &i in layers {
            let opacity = stack.layers[i].opacity.clamp(0.0, 1.0);
            let k = used.binary_search(&i).expect("prepared");
            match drawn_of(i) {
                Drawn::Nothing => {}
                Drawn::Rect(r, c) => {
                    let gs = format!("G{k}");
                    states.push((gs.clone(), opacity * c[3] as f32 / 255.0));
                    let m = place(*r);
                    content
                        .save_state()
                        .set_parameters(Name(gs.as_bytes()))
                        .set_fill_rgb(
                            c[0] as f32 / 255.0,
                            c[1] as f32 / 255.0,
                            c[2] as f32 / 255.0,
                        )
                        .rect(m[4], m[5], m[0], m[3])
                        .fill_nonzero()
                        .restore_state();
                }
                Drawn::Image { rect, .. } => {
                    let name = format!("I{k}");
                    let gs = format!("G{k}");
                    xobjects.push((name.clone(), images[k].expect("image written")));
                    states.push((gs.clone(), opacity));
                    content
                        .save_state()
                        .set_parameters(Name(gs.as_bytes()))
                        .transform(place(*rect))
                        .x_object(Name(name.as_bytes()))
                        .restore_state();
                }
            }
        }
        pdf.stream(content_id, &content.finish());
        let mut page = pdf.page(page_id);
        page.parent(tree)
            .media_box(Rect::new(0.0, 0.0, pw as f32, ph as f32))
            .contents(content_id);
        let mut res = page.resources();
        {
            let mut xo = res.x_objects();
            for (name, id) in &xobjects {
                xo.pair(Name(name.as_bytes()), *id);
            }
        }
        {
            let mut ext = res.ext_g_states();
            for (name, alpha) in &states {
                ext.insert(Name(name.as_bytes()))
                    .start::<pdf_writer::writers::ExtGraphicsState>()
                    .non_stroking_alpha(*alpha)
                    .stroking_alpha(*alpha);
            }
        }
        res.finish();
        page.finish();
        // A page per layer: a bookmark per page, named after its (bottom) layer.
        if o.layers_as_pages
            && let Some(&first) = layers.first()
        {
            bookmarks.push((page_id, stack.layers[first].name.clone()));
        }
    }

    pdf.pages(tree)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    let mut cat = pdf.catalog(catalog);
    cat.pages(tree);
    if let Some(root) = outline {
        cat.outlines(root);
    }
    cat.finish();
    pdf.document_info(info).creator(TextStr("Farba"));

    if let Some(root) = outline {
        let items: Vec<Ref> = bookmarks.iter().map(|_| alloc()).collect();
        let mut ol = pdf.outline(root);
        if let (Some(first), Some(last)) = (items.first(), items.last()) {
            ol.first(*first).last(*last);
        }
        ol.count(items.len() as i32);
        ol.finish();
        for (j, ((page, name), id)) in bookmarks.iter().zip(&items).enumerate() {
            let mut item = pdf.outline_item(*id);
            item.title(TextStr(name)).parent(root);
            if j > 0 {
                item.prev(items[j - 1]);
            }
            if let Some(n) = items.get(j + 1) {
                item.next(*n);
            }
            item.dest().page(*page).xyz(0.0, ph as f32, None);
        }
    }

    let bytes = pdf.finish();
    write_atomically(path, |w| w.write_all(&bytes).map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack::StackLayer;

    fn layer(name: &str, x: i32, y: i32, w: u32, h: u32) -> StackLayer {
        StackLayer {
            name: name.into(),
            x,
            y,
            width: w,
            height: h,
            opacity: 1.0,
            visible: true,
            lock_alpha: false,
            lock_position: false,
            text: None,
            name_from_text: false,
            source: None,
        }
    }

    fn options() -> PdfOptions {
        PdfOptions {
            resolution: 72.0,
            vectorize: true,
            ignore_hidden: true,
            background: None,
            layers_as_pages: false,
            reverse_order: false,
            no_interpolation: true,
        }
    }

    /// 40 x 30 image: a red layer (20 x 10 at 5, 5, partly outside on the right of a wider
    /// layer), a noisy layer, a hidden layer.
    fn sample() -> (Stack, Vec<u8>) {
        let stack = Stack {
            width: 40,
            height: 30,
            active: 0,
            layers: vec![
                layer("Red", 5, 5, 20, 10),
                layer("Noise", 30, 20, 20, 20),
                {
                    let mut l = layer("Hidden", 0, 0, 40, 30);
                    l.visible = false;
                    l
                },
            ],
            selection: false,
            notes: Vec::new(),
        };
        let mut data = [255u8, 0, 0, 255].repeat(200);
        data.extend((0..400u32).flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255]));
        data.extend([0u8, 0, 255, 255].repeat(1200));
        (stack, data)
    }

    #[test]
    fn page_assignment_follows_gimp() {
        let (stack, _) = sample();
        let mut o = options();
        assert_eq!(pages(&stack, &o), vec![vec![0, 1]]);
        o.ignore_hidden = false;
        o.layers_as_pages = true;
        o.reverse_order = true;
        assert_eq!(pages(&stack, &o), vec![vec![2], vec![1], vec![0]]);
        // Text layers go on the page of the layer below them.
        let mut s = stack.clone();
        s.layers[1].text = Some("{}".into());
        assert_eq!(pages(&s, &o), vec![vec![2], vec![0, 1]]);
        s.layers[0].text = Some("{}".into());
        o.reverse_order = false;
        assert_eq!(pages(&s, &o), vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn content_bounds_and_single_colour() {
        let (stack, data) = sample();
        let (px, _) = stack.split(&data).unwrap();
        match prepare(&stack, 0, px[0].rgba, true).unwrap() {
            Drawn::Rect(r, c) => assert_eq!((r, c), ([5, 5, 20, 10], [255, 0, 0, 255])),
            _ => panic!("expected a rectangle"),
        }
        // Clipped to the image: 10 x 10 of the 20 x 20 layer.
        match prepare(&stack, 1, px[1].rgba, true).unwrap() {
            Drawn::Image { rect, alpha, .. } => {
                assert_eq!(rect, [30, 20, 10, 10]);
                assert!(alpha.is_none());
            }
            _ => panic!("expected an image"),
        }
    }

    /// Elsewhere the reader is PDFium: run with `FARBA_PDFIUM=<path to libpdfium.so>`.
    #[test]
    fn round_trip_through_windows() {
        use crate::pdf_read::PdfReader;
        #[cfg(not(windows))]
        if std::env::var_os("FARBA_PDFIUM").is_none() {
            return;
        }
        let (stack, data) = sample();
        let dir = std::env::temp_dir().join(format!("farba-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pdf");
        let mut o = options();
        o.resolution = 144.0;
        write(&path, &stack, &data, &o).unwrap();
        let r = PdfReader::open(&path, None).unwrap();
        assert_eq!(r.page_count(), 1);
        let (w, h) = r.page_size(0).unwrap();
        assert!(
            (w - 20.0).abs() < 0.01 && (h - 15.0).abs() < 0.01,
            "{w} x {h}"
        );
        // Rendered back at the export resolution: the same pixels.
        let px = r.render(0, 40, 30, false).unwrap();
        let at = |x: usize, y: usize| &px[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert_eq!(at(10, 10), [255, 0, 0, 255]);
        assert_eq!(at(1, 1)[3], 0, "transparent background");
        // Windows.Data.Pdf smooths images, so compare the centres of 8x magnified pixels
        // (the file itself holds the exact pixels).
        let big = r.render(0, 320, 240, false).unwrap();
        let noise = &data[800..];
        for (x, y) in [(30, 20), (35, 25), (39, 29)] {
            let (bx, by) = (x * 8 + 4, y * 8 + 4);
            let got = &big[(by * 320 + bx) * 4..(by * 320 + bx) * 4 + 4];
            let s = ((y - 20) * 20 + (x - 30)) * 4;
            let d: Vec<i32> = (0..4)
                .map(|c| got[c] as i32 - noise[s + c] as i32)
                .collect();
            assert!(d.iter().all(|v| v.abs() <= 2), "pixel {x},{y}: {d:?}");
        }
        let white = r.render(0, 40, 30, true).unwrap();
        assert_eq!(white[4..8], [255, 255, 255, 255]);

        o.layers_as_pages = true;
        o.ignore_hidden = false;
        o.background = Some([0, 255, 0]);
        write(&path, &stack, &data, &o).unwrap();
        let r = PdfReader::open(&path, None).unwrap();
        assert_eq!(r.page_count(), 3);
        let px = r.render(2, 40, 30, false).unwrap();
        assert_eq!(px[..4], [0, 0, 255, 255]);
        let px = r.render(0, 40, 30, false).unwrap();
        assert_eq!(px[..4], [0, 255, 0, 255]);
        drop(r);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
