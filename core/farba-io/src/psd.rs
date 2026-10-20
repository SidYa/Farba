//! Photoshop documents (`.psd`, and the large-document `.psb`), opened as layers. Written
//! from Adobe's published file format description (no third-party code).
//!
//! Kept: every pixel layer with its name (Unicode), position, opacity (times fill opacity),
//! visibility, transparency / position locks, its layer mask (multiplied into the alpha)
//! and clipping (a clipped layer keeps only what lies over its base layer). Layer groups are
//! flattened into the list (opacity multiplied, hidden groups hide their layers). Converted
//! to 8-bit straight RGBA from 8 / 16 / 32-bit RGB, grayscale, CMYK, Lab, indexed, duotone
//! and bitmap. Not kept (reported in the notes): blend modes other than Normal, adjustment /
//! fill layers without pixels, layer effects and vector masks. Text layers keep their
//! rendered pixels and become editable Farba text where possible (`psd_text.rs`). A document
//! without layers opens from its merged image.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use crate::psd_text::{FontResolver, text_spec};
use crate::stack::{MAX_BYTES, MAX_SIDE, Stack, StackLayer};

/// Reads big-endian values from a byte slice.
struct Cursor<'a> {
    d: &'a [u8],
    p: usize,
    /// PSB: some lengths are 8 bytes.
    psb: bool,
}

type R<T> = Result<T, String>;

const TRUNCATED: &str = "The file is truncated or damaged";

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.d.len())
            .ok_or(TRUNCATED)?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> R<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> R<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> R<i32> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> R<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    /// A length that is 8 bytes in PSB files.
    fn len(&mut self) -> R<usize> {
        let v = if self.psb {
            self.u64()?
        } else {
            self.u32()? as u64
        };
        usize::try_from(v).map_err(|_| TRUNCATED.to_string())
    }
    fn skip(&mut self, n: usize) -> R<()> {
        self.take(n).map(drop)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bitmap,
    Gray,
    Indexed,
    Rgb,
    Cmyk,
    Multichannel,
    Duotone,
    Lab,
}

struct Header {
    channels: usize,
    width: u32,
    height: u32,
    depth: u16,
    mode: Mode,
    palette: Vec<u8>,
}

/// A layer record (before its pixels are read).
struct Record {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    channels: Vec<(i16, usize)>,
    blend: [u8; 4],
    opacity: u8,
    clipping: bool,
    hidden: bool,
    mask: Option<MaskInfo>,
    name: String,
    /// 0 normal layer, 1 / 2 open / closed group, 3 group end.
    section: u32,
    section_blend: Option<[u8; 4]>,
    fill_opacity: u8,
    lock_alpha: bool,
    lock_position: bool,
    adjustment: bool,
    text: bool,
    /// The `TySh` block of a text layer.
    tysh: Option<Vec<u8>>,
    effects: bool,
    vector_mask: bool,
}

#[derive(Clone, Copy)]
struct MaskInfo {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    default: u8,
    disabled: bool,
}

/// A decoded layer: position, size, straight RGBA8.
struct Pixels {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct Notes {
    modes: BTreeSet<String>,
    groups: bool,
    adjustments: usize,
    text: bool,
    effects: bool,
    vector_masks: bool,
}

fn blend_name(key: &[u8; 4]) -> Option<&'static str> {
    Some(match key {
        b"norm" | b"pass" => return None,
        b"diss" => "dissolve",
        b"dark" => "darken",
        b"mul " => "multiply",
        b"idiv" => "color burn",
        b"lbrn" => "linear burn",
        b"dkCl" => "darker color",
        b"lite" => "lighten",
        b"scrn" => "screen",
        b"div " => "color dodge",
        b"lddg" => "linear dodge",
        b"lgCl" => "lighter color",
        b"over" => "overlay",
        b"sLit" => "soft light",
        b"hLit" => "hard light",
        b"vLit" => "vivid light",
        b"lLit" => "linear light",
        b"pLit" => "pin light",
        b"hMix" => "hard mix",
        b"diff" => "difference",
        b"smud" => "exclusion",
        b"fsub" => "subtract",
        b"fdiv" => "divide",
        b"hue " => "hue",
        b"sat " => "saturation",
        b"colr" => "color",
        b"lum " => "luminosity",
        _ => "other",
    })
}

fn header(c: &mut Cursor) -> R<Header> {
    if c.take(4)? != b"8BPS" {
        return Err("This is not a Photoshop file".into());
    }
    let version = c.u16()?;
    c.psb = match version {
        1 => false,
        2 => true,
        _ => return Err(format!("Unsupported Photoshop file version {version}")),
    };
    c.skip(6)?;
    let channels = c.u16()? as usize;
    let height = c.u32()?;
    let width = c.u32()?;
    let depth = c.u16()?;
    let mode = match c.u16()? {
        0 => Mode::Bitmap,
        1 => Mode::Gray,
        2 => Mode::Indexed,
        3 => Mode::Rgb,
        4 => Mode::Cmyk,
        7 => Mode::Multichannel,
        8 => Mode::Duotone,
        9 => Mode::Lab,
        m => return Err(format!("Unsupported Photoshop colour mode {m}")),
    };
    if !matches!(depth, 1 | 8 | 16 | 32) {
        return Err(format!("Unsupported bit depth {depth}"));
    }
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("Unsupported image size {width} x {height}"));
    }
    let n = c.u32()? as usize;
    let palette = c.take(n)?.to_vec();
    Ok(Header {
        channels,
        width,
        height,
        depth,
        mode,
        palette,
    })
}

/// Additional layer information blocks (`8BIM` / `8B64` key length data) until `end`.
fn tagged_blocks<'a>(
    c: &mut Cursor<'a>,
    end: usize,
    mut f: impl FnMut(&[u8; 4], &'a [u8]),
) -> R<()> {
    while c.p + 12 <= end {
        let sig = c.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key: [u8; 4] = c.take(4)?.try_into().unwrap();
        // In PSB these keys have 8-byte lengths.
        let long = c.psb
            && matches!(
                &key,
                b"LMsk"
                    | b"Lr16"
                    | b"Lr32"
                    | b"Layr"
                    | b"Mt16"
                    | b"Mt32"
                    | b"Mtrn"
                    | b"Alph"
                    | b"FMsk"
                    | b"lnk2"
                    | b"FEid"
                    | b"FXid"
                    | b"PxSD"
            );
        let n = if long {
            c.u64()? as usize
        } else {
            c.u32()? as usize
        };
        let data = c.take(n.min(end.saturating_sub(c.p)))?;
        f(&key, data);
        // Lengths are padded to an even (some writers: 4-byte) boundary.
        while c.p < end && !c.p.is_multiple_of(2) {
            c.p += 1;
        }
        while c.p + 4 <= end && &c.d[c.p..c.p + 4] != b"8BIM" && &c.d[c.p..c.p + 4] != b"8B64" {
            if c.d[c.p] != 0 {
                break;
            }
            c.p += 1;
        }
    }
    c.p = end;
    Ok(())
}

fn record(c: &mut Cursor) -> R<Record> {
    let top = c.i32()?;
    let left = c.i32()?;
    let bottom = c.i32()?;
    let right = c.i32()?;
    let n = c.u16()? as usize;
    if n > 64 {
        return Err(TRUNCATED.into());
    }
    let mut channels = Vec::with_capacity(n);
    for _ in 0..n {
        let id = c.i16()?;
        let len = c.len()?;
        channels.push((id, len));
    }
    if c.take(4)? != b"8BIM" {
        return Err(TRUNCATED.into());
    }
    let blend: [u8; 4] = c.take(4)?.try_into().unwrap();
    let opacity = c.u8()?;
    let clipping = c.u8()? == 1;
    let flags = c.u8()?;
    c.skip(1)?;
    let extra = c.u32()? as usize;
    let end =
        c.p.checked_add(extra)
            .filter(|&e| e <= c.d.len())
            .ok_or(TRUNCATED)?;

    // Layer mask data.
    let mlen = c.u32()? as usize;
    let mstart = c.p;
    let mask = if mlen >= 18 {
        let (top, left, bottom, right) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
        let default = c.u8()?;
        let mflags = c.u8()?;
        Some(MaskInfo {
            top,
            left,
            bottom,
            right,
            default,
            disabled: mflags & 2 != 0,
        })
    } else {
        None
    };
    c.p = mstart + mlen;
    // Blending ranges.
    let blen = c.u32()? as usize;
    c.skip(blen)?;
    // Pascal name padded to 4 bytes.
    let nlen = c.u8()? as usize;
    let raw = c.take(nlen)?;
    let mut name: String = raw.iter().map(|&b| b as char).collect();
    let pad = (4 - (nlen + 1) % 4) % 4;
    c.skip(pad.min(end.saturating_sub(c.p)))?;

    let mut r = Record {
        top,
        left,
        bottom,
        right,
        channels,
        blend,
        opacity,
        clipping,
        hidden: flags & 2 != 0,
        mask,
        name: String::new(),
        section: 0,
        section_blend: None,
        fill_opacity: 255,
        lock_alpha: false,
        lock_position: false,
        adjustment: false,
        text: false,
        tysh: None,
        effects: false,
        vector_mask: false,
    };
    tagged_blocks(c, end, |key, data| match key {
        b"luni" if data.len() >= 4 => {
            let n = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
            let units: Vec<u16> = data[4..]
                .as_chunks::<2>()
                .0
                .iter()
                .take(n)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect();
            name = String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_string();
        }
        b"lsct" | b"lsdk" if data.len() >= 4 => {
            r.section = u32::from_be_bytes(data[..4].try_into().unwrap());
            if data.len() >= 12 && &data[4..8] == b"8BIM" {
                r.section_blend = Some(data[8..12].try_into().unwrap());
            }
        }
        b"iOpa" if !data.is_empty() => r.fill_opacity = data[0],
        b"lspf" if data.len() >= 4 => {
            let f = u32::from_be_bytes(data[..4].try_into().unwrap());
            let all = f & 0x8000_0000 != 0;
            r.lock_alpha = all || f & 1 != 0;
            r.lock_position = all || f & 4 != 0;
        }
        b"TySh" => {
            r.text = true;
            r.tysh = Some(data.to_vec());
        }
        b"tySh" => r.text = true,
        b"lfx2" | b"lrFX" | b"lmfx" => r.effects = true,
        b"vmsk" | b"vsms" => r.vector_mask = true,
        b"SoCo" | b"GdFl" | b"PtFl" | b"brit" | b"levl" | b"curv" | b"expA" | b"vibA" | b"hue "
        | b"hue2" | b"blnc" | b"blwh" | b"phfl" | b"mixr" | b"clrL" | b"nvrt" | b"post"
        | b"thrs" | b"grdm" | b"selc" => r.adjustment = true,
        _ => {}
    })?;
    r.name = name;
    Ok(r)
}

/// PackBits: the decoded bytes of one row (or as many as `out` holds).
fn unpack_bits(src: &[u8], out: &mut [u8]) {
    let (mut i, mut o) = (0, 0);
    while i < src.len() && o < out.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            let end = (i + len).min(src.len());
            let take = (end - i).min(out.len() - o);
            out[o..o + take].copy_from_slice(&src[i..i + take]);
            i = end;
            o += take;
        } else if n != -128 {
            let len = (1 - n as isize) as usize;
            let Some(&v) = src.get(i) else { break };
            i += 1;
            let take = len.min(out.len() - o);
            out[o..o + take].fill(v);
            o += take;
        }
    }
}

/// Undoes the prediction of "ZIP with prediction" for one channel of `w` x `h` samples.
fn unpredict(buf: &mut [u8], w: usize, h: usize, depth: u16) {
    match depth {
        8 => {
            for row in buf.chunks_exact_mut(w).take(h) {
                for x in 1..w {
                    row[x] = row[x].wrapping_add(row[x - 1]);
                }
            }
        }
        16 => {
            for row in buf.chunks_exact_mut(w * 2).take(h) {
                let mut prev = u16::from_be_bytes([row[0], row[1]]);
                for x in 1..w {
                    let v = u16::from_be_bytes([row[x * 2], row[x * 2 + 1]]).wrapping_add(prev);
                    row[x * 2..x * 2 + 2].copy_from_slice(&v.to_be_bytes());
                    prev = v;
                }
            }
        }
        32 => {
            // Bytes of a row are stored as planes (all first bytes, then all second ...),
            // delta-coded as one byte stream.
            let mut tmp = vec![0u8; w * 4];
            for row in buf.chunks_exact_mut(w * 4).take(h) {
                for x in 1..w * 4 {
                    row[x] = row[x].wrapping_add(row[x - 1]);
                }
                for x in 0..w {
                    for b in 0..4 {
                        tmp[x * 4 + b] = row[b * w + x];
                    }
                }
                row.copy_from_slice(&tmp);
            }
        }
        _ => {}
    }
}

/// Bytes per row of one channel.
fn row_bytes(w: usize, depth: u16) -> usize {
    match depth {
        1 => w.div_ceil(8),
        d => w * (d as usize / 8),
    }
}

/// Decodes one channel's compressed data (compression code + data) to raw samples.
fn channel(data: &[u8], compression: u16, w: usize, h: usize, depth: u16, psb: bool) -> R<Vec<u8>> {
    let rb = row_bytes(w, depth);
    let mut out = vec![0u8; rb * h];
    match compression {
        0 => {
            let n = out.len().min(data.len());
            out[..n].copy_from_slice(&data[..n]);
        }
        1 => {
            let cw = if psb { 4 } else { 2 };
            let counts = data.get(..h * cw).ok_or(TRUNCATED)?;
            let mut pos = h * cw;
            for (y, row) in out.chunks_exact_mut(rb).enumerate() {
                let n = if psb {
                    u32::from_be_bytes(counts[y * 4..y * 4 + 4].try_into().unwrap()) as usize
                } else {
                    u16::from_be_bytes([counts[y * 2], counts[y * 2 + 1]]) as usize
                };
                let end = (pos + n).min(data.len());
                unpack_bits(&data[pos..end], row);
                pos = end;
            }
        }
        2 | 3 => {
            let mut z = flate2::read::ZlibDecoder::new(data);
            let mut buf = Vec::with_capacity(out.len());
            let _ = z.read_to_end(&mut buf);
            let n = out.len().min(buf.len());
            out[..n].copy_from_slice(&buf[..n]);
            if compression == 3 {
                unpredict(&mut out, w, h, depth);
            }
        }
        c => return Err(format!("Unsupported compression {c}")),
    }
    Ok(out)
}

/// One sample of a channel as 0..=1.
fn sample(ch: &[u8], i: usize, depth: u16) -> f32 {
    match depth {
        8 => ch[i] as f32 / 255.0,
        16 => u16::from_be_bytes([ch[i * 2], ch[i * 2 + 1]]) as f32 / 65535.0,
        32 => {
            let v = f32::from_be_bytes(ch[i * 4..i * 4 + 4].try_into().unwrap());
            // 32-bit documents are linear light.
            linear_to_srgb(v.clamp(0.0, 1.0))
        }
        1 => {
            let bit = (ch[i / 8] >> (7 - i % 8)) & 1;
            if bit == 1 { 0.0 } else { 1.0 }
        }
        _ => 0.0,
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn to8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// CIE Lab (D50) to sRGB.
fn lab_to_rgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let f = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    let (x, y, z) = (0.9642 * f(fx), f(fy), 0.8251 * f(fz));
    // Bradford-adapted D50 XYZ to linear sRGB.
    let r = 3.1339 * x - 1.6169 * y - 0.4906 * z;
    let g = -0.9788 * x + 1.9161 * y + 0.0335 * z;
    let bl = 0.0720 * x - 0.2290 * y + 1.4057 * z;
    [r, g, bl].map(|v| linear_to_srgb(v.clamp(0.0, 1.0)))
}

/// Combines decoded colour channels (in mode order) and alpha into straight RGBA8.
fn to_rgba(h: &Header, colour: &[Vec<u8>], alpha: Option<&[u8]>, n: usize) -> Vec<u8> {
    let d = h.depth;
    let mut out = vec![0u8; n * 4];
    let get = |c: usize, i: usize| colour.get(c).map_or(0.0, |ch| sample(ch, i, d));
    for i in 0..n {
        let rgb: [f32; 3] = match h.mode {
            Mode::Rgb => [get(0, i), get(1, i), get(2, i)],
            Mode::Gray | Mode::Duotone | Mode::Bitmap | Mode::Multichannel => {
                let v = get(0, i);
                [v, v, v]
            }
            Mode::Cmyk => {
                // Stored inverted (1 = no ink).
                let k = get(3, i);
                [get(0, i) * k, get(1, i) * k, get(2, i) * k]
            }
            Mode::Lab => lab_to_rgb(
                get(0, i) * 100.0,
                get(1, i) * 255.0 - 128.0,
                get(2, i) * 255.0 - 128.0,
            ),
            Mode::Indexed => {
                let idx = colour.first().map_or(0, |c| c.get(i).copied().unwrap_or(0)) as usize;
                let p = &h.palette;
                let at = |k: usize| p.get(k * 256 + idx).copied().unwrap_or(0) as f32 / 255.0;
                [at(0), at(1), at(2)]
            }
        };
        let a = alpha.map_or(1.0, |a| sample(a, i, d));
        let o = &mut out[i * 4..i * 4 + 4];
        o[0] = to8(rgb[0]);
        o[1] = to8(rgb[1]);
        o[2] = to8(rgb[2]);
        o[3] = to8(a);
    }
    out
}

fn colour_channels(mode: Mode) -> usize {
    match mode {
        Mode::Rgb | Mode::Lab => 3,
        Mode::Cmyk => 4,
        _ => 1,
    }
}

/// Reads the layer records and their pixels from a layer info section.
fn layer_info(c: &mut Cursor, end: usize, h: &Header) -> R<Vec<(Record, Option<Pixels>)>> {
    if c.p + 2 > end {
        return Ok(Vec::new());
    }
    let count = c.i16()?.unsigned_abs() as usize;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(record(c)?);
    }
    let nc = colour_channels(h.mode);
    let mut out = Vec::with_capacity(count);
    let mut total = 0usize;
    for r in records {
        let w = (r.right - r.left).max(0) as usize;
        let ht = (r.bottom - r.top).max(0) as usize;
        let mut colour: Vec<Vec<u8>> = vec![Vec::new(); nc];
        let mut alpha = None;
        let mut mask = None;
        for &(id, len) in &r.channels {
            let data = c.take(len.min(end.saturating_sub(c.p)))?;
            if data.len() < 2 {
                continue;
            }
            let comp = u16::from_be_bytes([data[0], data[1]]);
            let (cw, ch) = match (id, &r.mask) {
                (-2, Some(m)) => (
                    (m.right - m.left).max(0) as usize,
                    (m.bottom - m.top).max(0) as usize,
                ),
                (-2 | -3, _) => continue,
                _ => (w, ht),
            };
            if cw == 0 || ch == 0 || cw > MAX_SIDE as usize || ch > MAX_SIDE as usize {
                continue;
            }
            let decoded = channel(&data[2..], comp, cw, ch, h.depth, c.psb)?;
            match id {
                -1 => alpha = Some(decoded),
                -2 => mask = Some(decoded),
                i if (0..nc as i16).contains(&i) => colour[i as usize] = decoded,
                _ => {}
            }
        }
        let pixels = if w > 0 && ht > 0 && r.section == 0 {
            if w > MAX_SIDE as usize || ht > MAX_SIDE as usize {
                return Err(format!("Unsupported layer size {w} x {ht}"));
            }
            total += w * ht * 4;
            if total > MAX_BYTES {
                return Err("The image is too large to open".into());
            }
            for ch in colour.iter_mut() {
                if ch.is_empty() {
                    *ch = vec![0; row_bytes(w, h.depth) * ht];
                }
            }
            let mut rgba = to_rgba(h, &colour, alpha.as_deref(), w * ht);
            if let (Some(m), Some(info)) = (&mask, &r.mask)
                && !info.disabled
            {
                apply_mask(&mut rgba, (r.left, r.top, w), m, info, h.depth);
            }
            Some(Pixels {
                x: r.left,
                y: r.top,
                w: w as u32,
                h: ht as u32,
                rgba,
            })
        } else {
            None
        };
        out.push((r, pixels));
    }
    Ok(out)
}

/// Multiplies the layer alpha by its mask (outside the mask: its default colour).
fn apply_mask(
    rgba: &mut [u8],
    (lx, ly, w): (i32, i32, usize),
    m: &[u8],
    info: &MaskInfo,
    depth: u16,
) {
    let mw = (info.right - info.left).max(0) as usize;
    let mh = (info.bottom - info.top).max(0) as usize;
    let rows = rgba.len() / 4 / w.max(1);
    for y in 0..rows {
        for x in 0..w {
            let (ix, iy) = (lx + x as i32 - info.left, ly + y as i32 - info.top);
            let v = if ix >= 0 && iy >= 0 && (ix as usize) < mw && (iy as usize) < mh {
                to8(sample(m, iy as usize * mw + ix as usize, depth))
            } else {
                info.default
            };
            let a = &mut rgba[(y * w + x) * 4 + 3];
            *a = ((*a as u32 * v as u32 + 127) / 255) as u8;
        }
    }
}

/// The merged image (the document without layers).
fn merged(c: &mut Cursor, h: &Header) -> R<Vec<u8>> {
    let (w, ht) = (h.width as usize, h.height as usize);
    let comp = c.u16()?;
    let n = h.channels;
    let rb = row_bytes(w, h.depth);
    let rest = &c.d[c.p..];
    let mut chans: Vec<Vec<u8>> = Vec::with_capacity(n);
    match comp {
        0 => {
            // A cut-off file keeps what is there (the rest stays empty).
            for k in 0..n {
                let mut ch = vec![0u8; rb * ht];
                let from = (k * rb * ht).min(rest.len());
                let src = &rest[from..((k + 1) * rb * ht).min(rest.len())];
                ch[..src.len()].copy_from_slice(src);
                chans.push(ch);
            }
        }
        1 => {
            // Row byte counts of every channel first, then the data.
            let cw = if c.psb { 4 } else { 2 };
            let counts = rest.get(..n * ht * cw).ok_or(TRUNCATED)?;
            let mut pos = n * ht * cw;
            for k in 0..n {
                let mut ch = vec![0u8; rb * ht];
                for (y, row) in ch.chunks_exact_mut(rb).enumerate() {
                    let j = (k * ht + y) * cw;
                    let len = if c.psb {
                        u32::from_be_bytes(counts[j..j + 4].try_into().unwrap()) as usize
                    } else {
                        u16::from_be_bytes([counts[j], counts[j + 1]]) as usize
                    };
                    let end = (pos + len).min(rest.len());
                    unpack_bits(&rest[pos..end], row);
                    pos = end;
                }
                chans.push(ch);
            }
        }
        2 | 3 => {
            let mut z = flate2::read::ZlibDecoder::new(rest);
            let mut buf = Vec::new();
            let _ = z.read_to_end(&mut buf);
            for k in 0..n {
                let mut ch = vec![0u8; rb * ht];
                let from = (k * rb * ht).min(buf.len());
                let src = &buf[from..((k + 1) * rb * ht).min(buf.len())];
                ch[..src.len()].copy_from_slice(src);
                if comp == 3 {
                    unpredict(&mut ch, w, ht, h.depth);
                }
                chans.push(ch);
            }
        }
        c => return Err(format!("Unsupported compression {c}")),
    }
    let nc = colour_channels(h.mode).min(chans.len());
    Ok(to_rgba(h, &chans[..nc], None, w * ht))
}

/// The merged image stored in the file (what Photoshop shows), for tests and as a fallback.
pub fn read_merged(data: &[u8]) -> R<(u32, u32, Vec<u8>)> {
    let mut c = Cursor {
        d: data,
        p: 0,
        psb: false,
    };
    let h = header(&mut c)?;
    let n = c.u32()? as usize;
    c.skip(n)?;
    let n = c.len()?;
    c.skip(n)?;
    Ok((h.width, h.height, merged(&mut c, &h)?))
}

/// Reads a Photoshop file: the layers as a [`Stack`] and its pixel buffer.
/// `resolve` finds installed fonts for text layers (see `psd_text`).
pub fn read(path: &Path, resolve: FontResolver) -> R<(Stack, Vec<u8>)> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    read_bytes(&data, resolve)
}

pub fn read_bytes(data: &[u8], resolve: FontResolver) -> R<(Stack, Vec<u8>)> {
    let mut c = Cursor {
        d: data,
        p: 0,
        psb: false,
    };
    let h = header(&mut c)?;
    // Image resources.
    let n = c.u32()? as usize;
    c.skip(n)?;
    // Layer and mask information.
    let lm_len = c.len()?;
    let lm_end =
        c.p.checked_add(lm_len)
            .filter(|&e| e <= data.len())
            .ok_or(TRUNCATED)?;
    let mut layers = Vec::new();
    if lm_len > 0 {
        let li_len = c.len()?;
        let li_end = c.p + li_len;
        if li_len > 0 {
            // Some writers store a too small section length: read up to the end of the file.
            layers = layer_info(&mut c, data.len(), &h)?;
        }
        c.p = li_end.min(lm_end);
        // Global layer mask info.
        if c.p + 4 <= lm_end {
            let g = c.u32()? as usize;
            c.p = (c.p + g).min(lm_end);
        }
        // 16 / 32-bit documents keep their layers in an additional block.
        if layers.is_empty() {
            let mut found = None;
            let start = c.p;
            tagged_blocks(&mut c, lm_end, |key, d| {
                if matches!(key, b"Lr16" | b"Lr32" | b"Layr") && found.is_none() {
                    found = Some(
                        d.as_ptr() as usize - data.as_ptr() as usize
                            ..d.as_ptr() as usize - data.as_ptr() as usize + d.len(),
                    );
                }
            })?;
            let _ = start;
            if let Some(range) = found {
                let mut sub = Cursor {
                    d: data,
                    p: range.start,
                    psb: c.psb,
                };
                layers = layer_info(&mut sub, range.end, &h)?;
            }
        }
    }
    c.p = lm_end;

    let mut notes = Notes::default();
    let flat = flatten(layers, &mut notes, resolve);
    let (stack_layers, buf) = if flat.is_empty() {
        let rgba = merged(&mut c, &h)?;
        (
            vec![plain_layer("Background", 0, 0, h.width, h.height)],
            rgba,
        )
    } else {
        let mut ls = Vec::new();
        let mut buf = Vec::new();
        for f in flat {
            let mut l = plain_layer(&f.name, f.px.x, f.px.y, f.px.w, f.px.h);
            l.opacity = f.opacity;
            l.visible = f.visible;
            l.lock_alpha = f.lock_alpha;
            l.lock_position = f.lock_position;
            l.text = f.text;
            buf.extend_from_slice(&f.px.rgba);
            ls.push(l);
        }
        (ls, buf)
    };
    let active = stack_layers.len() - 1;
    Ok((
        Stack {
            width: h.width,
            height: h.height,
            active,
            layers: stack_layers,
            selection: false,
            notes: notes.list(),
        },
        buf,
    ))
}

fn plain_layer(name: &str, x: i32, y: i32, w: u32, h: u32) -> StackLayer {
    StackLayer {
        name: name.to_string(),
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

impl Notes {
    fn list(self) -> Vec<String> {
        let mut out = Vec::new();
        if self.groups {
            out.push("Layer groups were merged into a single list of layers".to_string());
        }
        if !self.modes.is_empty() {
            let m: Vec<String> = self.modes.into_iter().collect();
            out.push(format!(
                "Layer modes are not supported, shown as Normal: {}",
                m.join(", ")
            ));
        }
        if self.adjustments > 0 {
            out.push(format!(
                "{} adjustment / fill layer(s) were skipped",
                self.adjustments
            ));
        }
        if self.effects {
            out.push("Layer effects (shadows, strokes...) are not shown".to_string());
        }
        if self.vector_masks {
            out.push("Vector masks are not applied".to_string());
        }
        if self.text {
            out.push(
                "Some text layers could not be made editable and were opened as pixels".to_string(),
            );
        }
        out
    }
}

/// A layer ready for the stack.
struct Flat {
    name: String,
    px: Pixels,
    opacity: f32,
    visible: bool,
    lock_alpha: bool,
    lock_position: bool,
    /// Farba text description (an editable text layer).
    text: Option<String>,
}

/// Applies clipping and groups (records are bottom to top) and returns the pixel layers.
fn flatten(
    records: Vec<(Record, Option<Pixels>)>,
    notes: &mut Notes,
    resolve: FontResolver,
) -> Vec<Flat> {
    let mut out: Vec<Flat> = Vec::new();
    // Each open group (seen from below): where it starts in `out`, and the clipping base
    // below it.
    let mut groups: Vec<(usize, Option<usize>)> = Vec::new();
    // The current clipping base (index in `out`), if any.
    let mut base: Option<usize> = None;
    for (r, px) in records {
        match r.section {
            3 => {
                groups.push((out.len(), base));
                base = None;
                continue;
            }
            1 | 2 => {
                notes.groups = true;
                let (start, below) = groups.pop().unwrap_or((0, None));
                let blend = r.section_blend.unwrap_or(r.blend);
                if let Some(m) = blend_name(&blend).or(blend_name(&r.blend)) {
                    notes.modes.insert(m.to_string());
                }
                // A group's fill opacity applies to its contents too.
                let o = (r.opacity as f32 / 255.0) * (r.fill_opacity as f32 / 255.0);
                for f in &mut out[start..] {
                    f.opacity *= o;
                    f.visible &= !r.hidden;
                }
                // A clipped group: all of it keeps only what lies over the base below it.
                if r.clipping
                    && let Some(b) = below
                {
                    let (lower, upper) = out.split_at_mut(start);
                    for f in upper {
                        clip(&mut f.px, &lower[b].px);
                        f.visible &= lower[b].visible;
                    }
                    base = below;
                } else {
                    base = None;
                }
                continue;
            }
            _ => {}
        }
        let text = r.tysh.as_deref().and_then(|t| text_spec(t, resolve));
        if r.text && px.is_some() && text.is_none() {
            notes.text = true;
        }
        notes.effects |= r.effects;
        notes.vector_masks |= r.vector_mask && px.is_some();
        let Some(mut px) = px else {
            if r.adjustment {
                notes.adjustments += 1;
            }
            if !r.clipping {
                base = None;
            }
            continue;
        };
        if let Some(m) = blend_name(&r.blend) {
            notes.modes.insert(m.to_string());
        }
        let mut visible = !r.hidden;
        if r.clipping {
            if let Some(b) = base.map(|b| &out[b]) {
                clip(&mut px, &b.px);
                visible &= b.visible;
            }
        } else {
            base = Some(out.len());
        }
        out.push(Flat {
            name: r.name,
            px,
            opacity: (r.opacity as f32 / 255.0) * (r.fill_opacity as f32 / 255.0),
            visible,
            lock_alpha: r.lock_alpha,
            lock_position: r.lock_position,
            text,
        });
    }
    out
}

/// Keeps only the part of `px` that lies over `base` (multiplies by the base alpha).
fn clip(px: &mut Pixels, base: &Pixels) {
    let w = px.w as usize;
    for y in 0..px.h as usize {
        for x in 0..w {
            let (bx, by) = (px.x + x as i32 - base.x, px.y + y as i32 - base.y);
            let ba = if bx >= 0 && by >= 0 && (bx as u32) < base.w && (by as u32) < base.h {
                base.rgba[(by as usize * base.w as usize + bx as usize) * 4 + 3]
            } else {
                0
            };
            let a = &mut px.rgba[(y * w + x) * 4 + 3];
            *a = ((*a as u32 * ba as u32 + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens every `.psd` / `.psb` under `$FARBA_SAMPLES` and compares the composite of the
    /// layers with the merged image Photoshop stored (mean difference per channel, 0..255).
    /// `cargo test -p farba-io psd_corpus -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn psd_corpus() {
        let Ok(root) = std::env::var("FARBA_SAMPLES") else {
            return;
        };
        let mut files = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(root)];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("psd") || x.eq_ignore_ascii_case("psb"))
                {
                    files.push(p);
                }
            }
        }
        files.sort();
        let (mut failed, mut differ) = (0, 0);
        for p in &files {
            let data = std::fs::read(p).unwrap();
            let name = p.file_name().unwrap().to_string_lossy();
            match read_bytes(&data, &|_| None) {
                Err(e) => {
                    failed += 1;
                    println!("FAIL {name}: {e}");
                }
                Ok((s, buf)) => {
                    let comp = s.to_document(&buf).map(|d| d.composite().to_rgba());
                    let merged = read_merged(&data);
                    let diff = match (&comp, &merged) {
                        (Ok(a), Ok((_, _, b))) if a.len() == b.len() => {
                            // Compare over white (the merged image has no transparency).
                            let mut sum = 0u64;
                            for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
                                for c in 0..3 {
                                    let v = (pa[c] as u32 * pa[3] as u32
                                        + 255 * (255 - pa[3] as u32))
                                        / 255;
                                    sum += (v as i64 - pb[c] as i64).unsigned_abs();
                                }
                            }
                            format!("{:.1}", sum as f64 / (a.len() / 4 * 3) as f64)
                        }
                        _ => "-".into(),
                    };
                    if diff != "-" && diff.parse::<f64>().unwrap() > 8.0 {
                        differ += 1;
                    }
                    println!(
                        "ok   {name}: {}x{} {} layers, diff {diff} {:?}",
                        s.width,
                        s.height,
                        s.layers.len(),
                        s.notes
                    );
                }
            }
        }
        println!(
            "{} files, {failed} failed, {differ} differ by > 8",
            files.len()
        );
    }
}
