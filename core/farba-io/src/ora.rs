//! OpenRaster (`.ora`), Farba's native layered format: a zip with `mimetype`, `stack.xml`
//! (the layer tree, top layer first), a PNG per layer, `mergedimage.png` and
//! `Thumbnails/thumbnail.png` (OpenRaster 0.0.6). GIMP, Krita and MyPaint open it.
//!
//! What OpenRaster cannot describe is kept in the same file under the `farba:` XML namespace
//! and the `farba/` folder, which other programs ignore: lock alpha / lock position, text
//! layers (`farba/text<N>.json`), the original pixels of transformed layers
//! (`farba/source<N>.png` + `farba:matrix`) and the selection (`farba/selection.png`).
//!
//! Reading other programs' files: layer groups are flattened into the stack (offsets added,
//! opacity multiplied, hidden groups hide their layers) and blend modes become Normal; both
//! are reported in [`Stack::notes`].

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use farba_core::composite::thumbnail;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::stack::{MAX_BYTES, MAX_SIDE, Stack, StackLayer, StackSource};

const MIMETYPE: &str = "image/openraster";
const NS: &str = "urn:x-farba:openraster:1";
const THUMB: u32 = 256;

// ---- writing --------------------------------------------------------------------------------

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

fn stack_xml(stack: &Stack) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!(
        "<image version=\"0.0.6\" w=\"{}\" h=\"{}\" xres=\"72\" yres=\"72\" xmlns:farba=\"{NS}\"{}>\n",
        stack.width,
        stack.height,
        if stack.selection {
            " farba:selection=\"farba/selection.png\""
        } else {
            ""
        }
    ));
    x.push_str("  <stack>\n");
    for (i, l) in stack.layers.iter().enumerate().rev() {
        x.push_str(&format!(
            "    <layer name=\"{}\" src=\"data/layer{i}.png\" x=\"{}\" y=\"{}\" opacity=\"{}\" visibility=\"{}\" composite-op=\"svg:src-over\"",
            xml_escape(&l.name),
            l.x,
            l.y,
            l.opacity,
            if l.visible { "visible" } else { "hidden" },
        ));
        if i == stack.active {
            x.push_str(" selected=\"true\"");
        }
        if l.lock_alpha {
            x.push_str(" farba:lock-alpha=\"true\"");
        }
        if l.lock_position {
            x.push_str(" farba:lock-position=\"true\"");
        }
        if l.text.is_some() {
            x.push_str(&format!(" farba:text=\"farba/text{i}.json\""));
            if l.name_from_text {
                x.push_str(" farba:name-from-text=\"true\"");
            }
        }
        if let Some(s) = &l.source {
            let m: Vec<String> = s.matrix.iter().map(f64::to_string).collect();
            x.push_str(&format!(
                " farba:source=\"farba/source{i}.png\" farba:matrix=\"{}\"",
                m.join(" ")
            ));
        }
        x.push_str("/>\n");
    }
    x.push_str("  </stack>\n</image>\n");
    x
}

/// A PNG to encode: path in the zip, pixels, size, grey (1 byte per pixel) or RGBA.
struct PngJob<'a> {
    name: String,
    data: &'a [u8],
    width: u32,
    height: u32,
    grey: bool,
}

fn encode_png(job: &PngJob) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive);
    let color = if job.grey {
        ExtendedColorType::L8
    } else {
        ExtendedColorType::Rgba8
    };
    enc.write_image(job.data, job.width, job.height, color)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Runs `f` over `items` on all cores, keeping the order of the results.
pub(crate) fn parallel<T: Sync, R: Send>(
    items: &[T],
    f: impl Fn(&T) -> Result<R, String> + Sync,
) -> Result<Vec<R>, String> {
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<Result<R, String>>> = (0..items.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        for w in workers {
            for (i, r) in w.join().expect("worker panicked") {
                results[i] = Some(r);
            }
        }
    });
    results
        .into_iter()
        .map(|r| r.expect("every item is done"))
        .collect()
}

/// Writes `stack` with its pixel buffer (see [`crate::stack`]) to `path`, through a
/// temporary file so a failed save never replaces the old file.
pub fn write(path: &Path, stack: &Stack, data: &[u8]) -> Result<(), String> {
    let (pixels, sel) = stack.split(data)?;
    let merged = stack.to_document(data)?.composite();
    let (tw, th) = thumb_size(stack.width, stack.height);
    let thumb = thumbnail(&merged, tw, th);
    let merged = merged.to_rgba();

    let mut jobs = Vec::new();
    for (i, (l, p)) in stack.layers.iter().zip(&pixels).enumerate() {
        jobs.push(PngJob {
            name: format!("data/layer{i}.png"),
            data: p.rgba,
            width: l.width,
            height: l.height,
            grey: false,
        });
        if let (Some(s), Some(px)) = (&l.source, p.source) {
            jobs.push(PngJob {
                name: format!("farba/source{i}.png"),
                data: px,
                width: s.width,
                height: s.height,
                grey: false,
            });
        }
    }
    if let Some(m) = sel {
        jobs.push(PngJob {
            name: "farba/selection.png".into(),
            data: m,
            width: stack.width,
            height: stack.height,
            grey: true,
        });
    }
    jobs.push(PngJob {
        name: "mergedimage.png".into(),
        data: &merged,
        width: stack.width,
        height: stack.height,
        grey: false,
    });
    jobs.push(PngJob {
        name: "Thumbnails/thumbnail.png".into(),
        data: &thumb,
        width: tw,
        height: th,
        grey: false,
    });
    let pngs = parallel(&jobs, encode_png)?;

    write_atomically(path, |w| {
        let mut zip = ZipWriter::new(w);
        let stored = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(true);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let err = |e: zip::result::ZipError| e.to_string();
        // The mimetype comes first and uncompressed, so the format can be sniffed.
        zip.start_file("mimetype", stored).map_err(err)?;
        zip.write_all(MIMETYPE.as_bytes())
            .map_err(|e| e.to_string())?;
        zip.start_file("stack.xml", deflated).map_err(err)?;
        zip.write_all(stack_xml(stack).as_bytes())
            .map_err(|e| e.to_string())?;
        for (i, l) in stack.layers.iter().enumerate() {
            if let Some(t) = &l.text {
                zip.start_file(format!("farba/text{i}.json"), deflated)
                    .map_err(err)?;
                zip.write_all(t.as_bytes()).map_err(|e| e.to_string())?;
            }
        }
        // PNG data is already compressed.
        for (job, png) in jobs.iter().zip(&pngs) {
            zip.start_file(job.name.as_str(), stored).map_err(err)?;
            zip.write_all(png).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(err)?;
        Ok(())
    })
}

fn thumb_size(w: u32, h: u32) -> (u32, u32) {
    let s = (THUMB as f64 / w.max(h) as f64).min(1.0);
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

/// Writes through `<path>.farba-tmp`, renamed over `path` when complete.
pub fn write_atomically(
    path: &Path,
    f: impl FnOnce(&mut BufWriter<File>) -> Result<(), String>,
) -> Result<(), String> {
    let mut tmp = PathBuf::from(path);
    tmp.as_mut_os_string().push(".farba-tmp");
    let result = (|| -> Result<(), String> {
        let file = File::create(&tmp).map_err(|e| e.to_string())?;
        let mut w = BufWriter::new(file);
        f(&mut w)?;
        w.flush().map_err(|e| e.to_string())?;
        drop(w);
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

// ---- reading --------------------------------------------------------------------------------

/// A layer found in `stack.xml`, with group attributes already applied.
struct Found {
    layer: StackLayer,
    src: String,
    selected: bool,
    text: Option<String>,
    source: Option<(String, [f64; 9])>,
}

#[derive(Default)]
struct Notes {
    groups: bool,
    modes: BTreeSet<String>,
    skipped: usize,
}

fn attr<'a>(n: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute(name)
}

fn farba_attr<'a>(n: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attribute((NS, name))
}

fn int(n: roxmltree::Node, name: &str) -> i32 {
    attr(n, name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(0, |v| v.round() as i32)
}

fn opacity(n: roxmltree::Node) -> f32 {
    attr(n, "opacity")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map_or(1.0, |v| v.clamp(0.0, 1.0))
}

fn visible(n: roxmltree::Node) -> bool {
    attr(n, "visibility") != Some("hidden")
}

fn mode_note(n: roxmltree::Node, notes: &mut Notes) {
    if let Some(op) = attr(n, "composite-op")
        && op != "svg:src-over"
    {
        notes
            .modes
            .insert(op.trim_start_matches("svg:").to_string());
    }
}

/// Walks a `<stack>` (children top to bottom), appending layers top first.
fn walk(
    stack: roxmltree::Node,
    offset: (i32, i32),
    opacity_mul: f32,
    shown: bool,
    out: &mut Vec<Found>,
    notes: &mut Notes,
) {
    for n in stack.children().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "stack" => {
                notes.groups = true;
                mode_note(n, notes);
                walk(
                    n,
                    (offset.0 + int(n, "x"), offset.1 + int(n, "y")),
                    opacity_mul * opacity(n),
                    shown && visible(n),
                    out,
                    notes,
                );
            }
            "layer" => {
                let Some(src) = attr(n, "src") else {
                    notes.skipped += 1;
                    continue;
                };
                mode_note(n, notes);
                let matrix = farba_attr(n, "matrix").and_then(|m| {
                    let v: Vec<f64> = m
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    <[f64; 9]>::try_from(v).ok()
                });
                out.push(Found {
                    layer: StackLayer {
                        name: attr(n, "name").unwrap_or_default().to_string(),
                        x: offset.0 + int(n, "x"),
                        y: offset.1 + int(n, "y"),
                        width: 0,
                        height: 0,
                        opacity: opacity_mul * opacity(n),
                        visible: shown && visible(n),
                        lock_alpha: farba_attr(n, "lock-alpha") == Some("true"),
                        lock_position: farba_attr(n, "lock-position") == Some("true"),
                        text: None,
                        name_from_text: farba_attr(n, "name-from-text") == Some("true"),
                        source: None,
                    },
                    src: src.to_string(),
                    selected: attr(n, "selected") == Some("true"),
                    text: farba_attr(n, "text").map(str::to_string),
                    source: farba_attr(n, "source").map(str::to_string).zip(matrix),
                });
            }
            // Text and vector elements of other programs.
            _ => notes.skipped += 1,
        }
    }
}

fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = zip
        .by_name(name.trim_start_matches('/'))
        .map_err(|_| format!("{name} is missing from the file"))?;
    if f.size() > MAX_BYTES as u64 {
        return Err(format!("{name} is too large"));
    }
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Decodes an image entry to straight RGBA8 (or one grey byte per pixel).
fn decode(bytes: &[u8], grey: bool) -> Result<(u32, u32, Vec<u8>), String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("Unsupported layer size {w} x {h}"));
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(if grey {
        (w, h, img.into_luma8().into_raw())
    } else {
        (w, h, img.into_rgba8().into_raw())
    })
}

/// True if `path` is an OpenRaster file (by its `mimetype` entry).
pub fn is_ora(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(mut zip) = ZipArchive::new(std::io::BufReader::new(file)) else {
        return false;
    };
    read_entry(&mut zip, "mimetype").is_ok_and(|m| m.trim_ascii() == MIMETYPE.as_bytes())
}

/// Reads an OpenRaster file: the header and its pixel buffer (see [`crate::stack`]).
pub fn read(path: &Path) -> Result<(Stack, Vec<u8>), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|_| "This is not an OpenRaster file".to_string())?;
    let xml = read_entry(&mut zip, "stack.xml")?;
    let xml = String::from_utf8(xml).map_err(|_| "stack.xml is not valid UTF-8".to_string())?;
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("stack.xml: {e}"))?;
    let image = doc.root_element();
    if image.tag_name().name() != "image" {
        return Err("stack.xml has no <image>".into());
    }
    let (width, height) = (int(image, "w"), int(image, "h"));
    if width <= 0 || height <= 0 || width > MAX_SIDE as i32 || height > MAX_SIDE as i32 {
        return Err(format!("Unsupported image size {width} x {height}"));
    }
    let (width, height) = (width as u32, height as u32);
    let Some(root) = image
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "stack")
    else {
        return Err("stack.xml has no <stack>".into());
    };
    let mut found = Vec::new();
    let mut notes = Notes::default();
    walk(root, (0, 0), 1.0, true, &mut found, &mut notes);
    if found.is_empty() {
        return Err("The file has no layers".into());
    }
    found.reverse(); // bottom to top

    // Compressed entries are read in order, then decoded on all cores.
    struct Job {
        bytes: Vec<u8>,
        grey: bool,
    }
    let mut jobs = Vec::new();
    for f in &found {
        jobs.push(Job {
            bytes: read_entry(&mut zip, &f.src)?,
            grey: false,
        });
        if let Some((src, _)) = &f.source {
            jobs.push(Job {
                bytes: read_entry(&mut zip, src).unwrap_or_default(),
                grey: false,
            });
        }
    }
    let sel_src = image.attribute((NS, "selection"));
    if let Some(src) = sel_src {
        jobs.push(Job {
            bytes: read_entry(&mut zip, src).unwrap_or_default(),
            grey: true,
        });
    }
    let texts: Vec<Option<String>> = found
        .iter()
        .map(|f| {
            f.text
                .as_ref()
                .and_then(|t| read_entry(&mut zip, t).ok())
                .and_then(|b| String::from_utf8(b).ok())
        })
        .collect();
    // A missing or broken optional entry (source, selection) is only dropped.
    let decoded = parallel(&jobs, |j| Ok(decode(&j.bytes, j.grey)))?;

    let mut decoded = decoded.into_iter();
    let mut layers = Vec::new();
    let mut data = Vec::new();
    for (f, text) in found.iter().zip(texts) {
        let (w, h, rgba) = decoded.next().expect("one result per job")?;
        let mut layer = f.layer.clone();
        layer.width = w;
        layer.height = h;
        layer.text = text;
        layer.name_from_text &= layer.text.is_some();
        let source = if f.source.is_some() {
            decoded.next().expect("one result per job").ok()
        } else {
            None
        };
        data_reserve(&data, rgba.len())?;
        data.extend_from_slice(&rgba);
        drop(rgba);
        if let (Some((_, matrix)), Some((sw, sh, px))) = (&f.source, source) {
            data_reserve(&data, px.len())?;
            data.extend_from_slice(&px);
            layer.source = Some(StackSource {
                width: sw,
                height: sh,
                matrix: *matrix,
            });
        }
        layers.push(layer);
    }
    let selection = match (sel_src, decoded.next()) {
        (Some(_), Some(Ok((w, h, m)))) if (w, h) == (width, height) => {
            data.extend_from_slice(&m);
            true
        }
        _ => false,
    };
    let active = found
        .iter()
        .rposition(|f| f.selected)
        .unwrap_or(found.len() - 1);

    let mut stack_notes = Vec::new();
    if notes.groups {
        stack_notes.push("Layer groups were merged into a single list of layers".to_string());
    }
    if !notes.modes.is_empty() {
        let modes: Vec<String> = notes.modes.into_iter().collect();
        stack_notes.push(format!(
            "Layer modes are not supported, shown as Normal: {}",
            modes.join(", ")
        ));
    }
    if notes.skipped > 0 {
        stack_notes.push(format!(
            "{} unsupported item(s) were skipped",
            notes.skipped
        ));
    }
    let stack = Stack {
        width,
        height,
        active,
        layers,
        selection,
        notes: stack_notes,
    };
    debug_assert_eq!(stack.byte_len(), data.len());
    Ok((stack, data))
}

fn data_reserve(data: &[u8], more: usize) -> Result<(), String> {
    if data.len() + more > MAX_BYTES {
        return Err("The image is too large to open".into());
    }
    Ok(())
}

/// The composite image of a stack (for "open as layer" of a layered file).
pub fn flatten(stack: &Stack, data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(stack.to_document(data)?.composite().to_rgba())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("farba-ora-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn layer(name: &str, w: u32, h: u32) -> StackLayer {
        StackLayer {
            name: name.into(),
            x: 0,
            y: 0,
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

    #[test]
    fn round_trip_keeps_everything() {
        let mut text = layer("Hi <&> \"there\"", 3, 2);
        text.x = -5;
        text.y = 7;
        text.opacity = 0.25;
        text.visible = false;
        text.lock_alpha = true;
        text.text = Some("{\"text\":\"Hi\"}".into());
        text.name_from_text = true;
        let mut moved = layer("Moved", 2, 2);
        moved.lock_position = true;
        moved.source = Some(StackSource {
            width: 1,
            height: 1,
            matrix: [2.0, 0.0, 0.1, 0.0, 2.0, 1.0 / 3.0, 0.0, 0.0, 1.0],
        });
        let stack = Stack {
            width: 4,
            height: 3,
            active: 1,
            layers: vec![layer("Bottom", 4, 3), text, moved],
            selection: true,
            notes: Vec::new(),
        };
        let data: Vec<u8> = (0..stack.byte_len())
            .map(|i| (i * 37 % 251) as u8)
            .collect();
        let path = temp("round.ora");
        write(&path, &stack, &data).unwrap();
        assert!(is_ora(&path));
        let (back, back_data) = read(&path).unwrap();
        assert_eq!(back, stack);
        assert_eq!(back_data, data);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn reads_groups_and_modes_of_other_programs() {
        let path = temp("other.ora");
        let png = |w: u32, h: u32, c: [u8; 4]| {
            encode_png(&PngJob {
                name: String::new(),
                data: &c.repeat((w * h) as usize),
                width: w,
                height: h,
                grey: false,
            })
            .unwrap()
        };
        let xml = r#"<image w="10" h="8"><stack>
            <stack x="2" y="1" opacity="0.5" visibility="hidden">
              <layer src="data/a.png" x="1" y="1" opacity="0.5" composite-op="svg:multiply" name="A"/>
            </stack>
            <text>hello</text>
            <layer src="data/b.png" name="B"/>
          </stack></image>"#;
        write_atomically(&path, |w| {
            let mut zip = ZipWriter::new(w);
            let o = SimpleFileOptions::default();
            zip.start_file("mimetype", o).unwrap();
            zip.write_all(MIMETYPE.as_bytes()).unwrap();
            zip.start_file("stack.xml", o).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
            zip.start_file("data/a.png", o).unwrap();
            zip.write_all(&png(2, 2, [255, 0, 0, 255])).unwrap();
            zip.start_file("data/b.png", o).unwrap();
            zip.write_all(&png(10, 8, [0, 0, 255, 255])).unwrap();
            zip.finish().unwrap();
            Ok(())
        })
        .unwrap();
        let (s, data) = read(&path).unwrap();
        assert_eq!(s.layers.len(), 2);
        let (b, a) = (&s.layers[0], &s.layers[1]);
        assert_eq!((b.name.as_str(), b.width, b.height), ("B", 10, 8));
        assert_eq!((a.name.as_str(), a.x, a.y, a.visible), ("A", 3, 2, false));
        assert!((a.opacity - 0.25).abs() < 1e-6);
        assert_eq!(s.active, 1);
        assert_eq!(s.notes.len(), 3);
        assert!(s.notes[1].contains("multiply"));
        assert_eq!(flatten(&s, &data).unwrap()[..4], [0, 0, 255, 255]);
        std::fs::remove_file(&path).unwrap();
    }
}
