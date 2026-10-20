//! GIMP documents (`.xcf`), opened as layers. Written from the published XCF format
//! description (developer.gimp.org, "XCF file format"); no GIMP code.
//!
//! Kept: every layer with its name, offsets, opacity, visibility, lock alpha / position and
//! layer mask (multiplied into the alpha when applied), RGB / grayscale / indexed images in
//! any precision (8 / 16 / 32-bit integer, half / float / double, linear or gamma),
//! uncompressed, RLE or zlib tiles, XCF versions 0 to the current one (64-bit offsets from
//! version 11). Layer groups are flattened (opacity multiplied, hidden groups hide their
//! layers). Text layers become editable Farba text where possible (the `gimp-text-layer`
//! parasite), keeping GIMP's pixels. Not kept (noted): layer modes other than Normal,
//! layer effects.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

use farba_text::{Justify, Span, SpanStyle, TextSpec};

use crate::psd_text::FontResolver;
use crate::stack::{MAX_BYTES, MAX_SIDE, Stack, StackLayer};

type R<T> = Result<T, String>;
const DAMAGED: &str = "The file is truncated or damaged";

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
    /// Version 11+: 8-byte offsets.
    wide: bool,
}

impl<'a> Cur<'a> {
    fn at(d: &'a [u8], p: usize, wide: bool) -> Self {
        Self { d, p, wide }
    }
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.d.len())
            .ok_or(DAMAGED)?;
        let s = &self.d[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn ptr(&mut self) -> R<usize> {
        let v = if self.wide {
            u64::from_be_bytes(self.take(8)?.try_into().unwrap())
        } else {
            self.u32()? as u64
        };
        usize::try_from(v).map_err(|_| DAMAGED.to_string())
    }
    fn string(&mut self) -> R<String> {
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        let b = self.take(n)?;
        Ok(String::from_utf8_lossy(&b[..n - 1]).to_string())
    }
    /// The property list: `(type, payload)` until PROP_END.
    fn props(&mut self) -> R<Vec<(u32, &'a [u8])>> {
        let mut out = Vec::new();
        loop {
            let ty = self.u32()?;
            let len = self.u32()? as usize;
            if ty == 0 {
                return Ok(out);
            }
            // The colour map's length was wrong in old files: compute it.
            let len = if ty == 1 {
                let save = self.p;
                let n = self.u32()? as usize;
                self.p = save;
                4 + 3 * n
            } else {
                len
            };
            out.push((ty, self.take(len)?));
        }
    }
}

fn be32(b: &[u8], i: usize) -> u32 {
    b.get(i..i + 4)
        .map_or(0, |s| u32::from_be_bytes(s.try_into().unwrap()))
}

/// How the components of a pixel are stored.
#[derive(Clone, Copy)]
enum Comp {
    U8,
    U16,
    U32,
    Half,
    F32,
    F64,
}

impl Comp {
    fn bytes(self) -> usize {
        match self {
            Comp::U8 => 1,
            Comp::U16 | Comp::Half => 2,
            Comp::U32 | Comp::F32 => 4,
            Comp::F64 => 8,
        }
    }
    fn read(self, b: &[u8]) -> f32 {
        match self {
            Comp::U8 => b[0] as f32 / 255.0,
            Comp::U16 => u16::from_be_bytes([b[0], b[1]]) as f32 / 65535.0,
            Comp::U32 => u32::from_be_bytes(b[..4].try_into().unwrap()) as f32 / u32::MAX as f32,
            Comp::Half => half(u16::from_be_bytes([b[0], b[1]])),
            Comp::F32 => f32::from_be_bytes(b[..4].try_into().unwrap()),
            Comp::F64 => f64::from_be_bytes(b[..8].try_into().unwrap()) as f32,
        }
    }
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The component type and whether colours are linear, from the precision field.
fn precision(version: u32, p: u32) -> R<(Comp, bool)> {
    Ok(match version {
        0..=3 => (Comp::U8, false),
        4 => match p {
            0 => (Comp::U8, false),
            1 => (Comp::U16, false),
            2 => (Comp::U32, true),
            3 => (Comp::Half, true),
            4 => (Comp::F32, true),
            _ => return Err(format!("Unknown precision {p}")),
        },
        5 | 6 => match p {
            100 => (Comp::U8, true),
            150 => (Comp::U8, false),
            200 => (Comp::U16, true),
            250 => (Comp::U16, false),
            300 => (Comp::U32, true),
            350 => (Comp::U32, false),
            400 => (Comp::Half, true),
            450 => (Comp::Half, false),
            500 => (Comp::F32, true),
            550 => (Comp::F32, false),
            _ => return Err(format!("Unknown precision {p}")),
        },
        _ => match p {
            100 => (Comp::U8, true),
            150 => (Comp::U8, false),
            200 => (Comp::U16, true),
            250 => (Comp::U16, false),
            300 => (Comp::U32, true),
            350 => (Comp::U32, false),
            500 => (Comp::Half, true),
            550 => (Comp::Half, false),
            600 => (Comp::F32, true),
            650 => (Comp::F32, false),
            700 => (Comp::F64, true),
            750 => (Comp::F64, false),
            _ => return Err(format!("Unknown precision {p}")),
        },
    })
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

/// Decodes one RLE tile (every byte plane separately) into interleaved pixels.
fn unrle(src: &[u8], out: &mut [u8], bpp: usize) -> R<()> {
    let n = out.len() / bpp;
    let mut i = 0;
    for plane in 0..bpp {
        let mut o = 0;
        while o < n {
            let op = *src.get(i).ok_or(DAMAGED)?;
            i += 1;
            let (run, literal) = match op {
                0..=126 => (op as usize + 1, false),
                127 => {
                    let c = u16::from_be_bytes([
                        *src.get(i).ok_or(DAMAGED)?,
                        *src.get(i + 1).ok_or(DAMAGED)?,
                    ]);
                    i += 2;
                    (c as usize, false)
                }
                128 => {
                    let c = u16::from_be_bytes([
                        *src.get(i).ok_or(DAMAGED)?,
                        *src.get(i + 1).ok_or(DAMAGED)?,
                    ]);
                    i += 2;
                    (c as usize, true)
                }
                _ => (256 - op as usize, true),
            };
            let run = run.min(n - o);
            if literal {
                let s = src.get(i..i + run).ok_or(DAMAGED)?;
                for (k, &v) in s.iter().enumerate() {
                    out[(o + k) * bpp + plane] = v;
                }
                i += run;
            } else {
                let v = *src.get(i).ok_or(DAMAGED)?;
                i += 1;
                for k in 0..run {
                    out[(o + k) * bpp + plane] = v;
                }
            }
            o += run;
        }
    }
    Ok(())
}

struct Ctx<'a> {
    d: &'a [u8],
    wide: bool,
    compression: u8,
    comp: Comp,
    linear: bool,
    colormap: Vec<u8>,
}

/// Reads a hierarchy (first level) into raw interleaved pixels `w * h * bpp`.
fn hierarchy(c: &Ctx, ptr: usize) -> R<(usize, usize, usize, Vec<u8>)> {
    let mut h = Cur::at(c.d, ptr, c.wide);
    let w = h.u32()? as usize;
    let ht = h.u32()? as usize;
    let bpp = h.u32()? as usize;
    if w == 0 || ht == 0 || w > MAX_SIDE as usize || ht > MAX_SIDE as usize || bpp == 0 || bpp > 32
    {
        return Err(DAMAGED.into());
    }
    let level = h.ptr()?;
    let mut l = Cur::at(c.d, level, c.wide);
    let lw = l.u32()? as usize;
    let lh = l.u32()? as usize;
    if (lw, lh) != (w, ht) {
        return Err(DAMAGED.into());
    }
    let (tx, ty) = (w.div_ceil(64), ht.div_ceil(64));
    // A list that ends early (a 0 pointer) leaves the remaining tiles empty.
    let mut tiles = Vec::with_capacity(tx * ty);
    for _ in 0..tx * ty {
        let p = l.ptr()?;
        if p == 0 {
            break;
        }
        tiles.push(p);
    }
    let mut out = vec![0u8; w * ht * bpp];
    let mut tile = vec![0u8; 64 * 64 * bpp];
    for (k, &p) in tiles.iter().enumerate() {
        let (col, row) = (k % tx, k / tx);
        let tw = (w - col * 64).min(64);
        let th = (ht - row * 64).min(64);
        let size = tw * th * bpp;
        // Tiles are contiguous: one ends where the next starts (the last: the file end).
        let end = tiles
            .get(k + 1)
            .copied()
            .filter(|&e| e > p)
            .unwrap_or(c.d.len())
            .min(c.d.len());
        let src = c.d.get(p..end).ok_or(DAMAGED)?;
        let t = &mut tile[..size];
        match c.compression {
            0 => t.copy_from_slice(src.get(..size).ok_or(DAMAGED)?),
            1 => unrle(src, t, bpp)?,
            2 => {
                t.fill(0);
                let mut z = flate2::read::ZlibDecoder::new(src);
                let mut buf = Vec::with_capacity(size);
                let _ = z.by_ref().take(size as u64).read_to_end(&mut buf);
                t[..buf.len().min(size)].copy_from_slice(&buf[..buf.len().min(size)]);
            }
            n => return Err(format!("Unsupported tile compression {n}")),
        }
        for y in 0..th {
            let dst = ((row * 64 + y) * w + col * 64) * bpp;
            out[dst..dst + tw * bpp].copy_from_slice(&t[y * tw * bpp..(y + 1) * tw * bpp]);
        }
    }
    Ok((w, ht, bpp, out))
}

/// Raw pixels of a layer type to straight RGBA8.
fn to_rgba(c: &Ctx, ty: u32, raw: &[u8], n: usize, bpp: usize) -> Vec<u8> {
    let cb = c.comp.bytes();
    let mut out = vec![0u8; n * 4];
    let colour = |v: f32| {
        to8(if c.linear {
            linear_to_srgb(v.clamp(0.0, 1.0))
        } else {
            v
        })
    };
    for i in 0..n {
        let px = &raw[i * bpp..(i + 1) * bpp];
        let comp = |k: usize| c.comp.read(&px[k * cb..]);
        let o = &mut out[i * 4..i * 4 + 4];
        match ty {
            0 | 1 => {
                o[0] = colour(comp(0));
                o[1] = colour(comp(1));
                o[2] = colour(comp(2));
                o[3] = if ty == 1 { to8(comp(3)) } else { 255 };
            }
            2 | 3 => {
                let v = colour(comp(0));
                o[..3].fill(v);
                o[3] = if ty == 3 { to8(comp(1)) } else { 255 };
            }
            _ => {
                let idx = px[0] as usize;
                let m = &c.colormap;
                for (k, v) in o[..3].iter_mut().enumerate() {
                    *v = m.get(4 + idx * 3 + k).copied().unwrap_or(0);
                }
                o[3] = if ty == 5 { px[1] } else { 255 };
            }
        }
    }
    out
}

fn mode_name(m: u32) -> Option<&'static str> {
    Some(match m {
        0 | 28 | 61 => return None, // normal (legacy / current), pass through
        1 => "dissolve",
        2 | 29 => "behind",
        3 | 30 => "multiply",
        4 | 31 => "screen",
        5 | 23 => "overlay",
        6 | 32 => "difference",
        7 | 33 => "addition",
        8 | 34 => "subtract",
        9 | 35 => "darken only",
        10 | 36 => "lighten only",
        11 | 24 | 37 => "hue",
        12 | 25 | 38 => "saturation",
        13 | 26 | 39 => "color",
        14 | 27 | 40 => "value",
        15 | 41 => "divide",
        16 | 42 => "dodge",
        17 | 43 => "burn",
        18 | 44 => "hard light",
        19 | 45 => "soft light",
        20 | 46 => "grain extract",
        21 | 47 => "grain merge",
        22 | 57 => "color erase",
        _ => "other",
    })
}

struct Layer {
    name: String,
    w: usize,
    h: usize,
    ty: u32,
    x: i32,
    y: i32,
    opacity: f32,
    visible: bool,
    mode: u32,
    lock_alpha: bool,
    lock_position: bool,
    group: bool,
    path: Vec<u32>,
    apply_mask: bool,
    text: Option<String>,
    text_modified: bool,
    floating: bool,
    hierarchy: usize,
    mask: usize,
}

fn layer(c: &Ctx, ptr: usize) -> R<Layer> {
    let mut r = Cur::at(c.d, ptr, c.wide);
    let w = r.u32()? as usize;
    let h = r.u32()? as usize;
    let ty = r.u32()?;
    let name = r.string()?;
    let mut l = Layer {
        name,
        w,
        h,
        ty,
        x: 0,
        y: 0,
        opacity: 1.0,
        visible: true,
        mode: 28,
        lock_alpha: false,
        lock_position: false,
        group: false,
        path: Vec::new(),
        apply_mask: true,
        text: None,
        text_modified: false,
        floating: false,
        hierarchy: 0,
        mask: 0,
    };
    for (ty, p) in r.props()? {
        match ty {
            5 => l.floating = true,
            6 => l.opacity = be32(p, 0) as f32 / 255.0,
            33 if p.len() >= 4 => {
                l.opacity = f32::from_be_bytes(p[..4].try_into().unwrap()).clamp(0.0, 1.0)
            }
            7 => l.mode = be32(p, 0),
            8 => l.visible = be32(p, 0) != 0,
            10 => l.lock_alpha = be32(p, 0) != 0,
            11 => l.apply_mask = be32(p, 0) != 0,
            15 => {
                l.x = be32(p, 0) as i32;
                l.y = be32(p, 4) as i32;
            }
            21 => l.text = parasite(p, "gimp-text-layer"),
            26 => l.text_modified = be32(p, 0) & 2 != 0,
            29 => l.group = true,
            30 => {
                l.path = p
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_be_bytes(*b))
                    .collect()
            }
            32 => l.lock_position = be32(p, 0) != 0,
            _ => {}
        }
    }
    l.hierarchy = r.ptr()?;
    l.mask = r.ptr()?;
    Ok(l)
}

/// The payload of the parasite `name` in a PROP_PARASITES list, as text.
fn parasite(p: &[u8], name: &str) -> Option<String> {
    let mut c = Cur::at(p, 0, false);
    while c.p < p.len() {
        let n = c.string().ok()?;
        c.u32().ok()?;
        let len = c.u32().ok()? as usize;
        let data = c.take(len).ok()?;
        if n == name {
            return Some(
                String::from_utf8_lossy(data)
                    .trim_end_matches('\0')
                    .to_string(),
            );
        }
    }
    None
}

/// The layer mask (a channel) as 8-bit coverage, or `None`.
fn mask(c: &Ctx, ptr: usize) -> R<Option<Vec<u8>>> {
    if ptr == 0 {
        return Ok(None);
    }
    let mut r = Cur::at(c.d, ptr, c.wide);
    r.u32()?;
    r.u32()?;
    r.string()?;
    r.props()?;
    let h = r.ptr()?;
    let (w, ht, bpp, raw) = hierarchy(c, h)?;
    let cb = c.comp.bytes();
    Ok(Some(
        (0..w * ht)
            .map(|i| to8(c.comp.read(&raw[i * bpp..i * bpp + cb])))
            .collect(),
    ))
}

// ---- text layers -----------------------------------------------------------------------------

#[derive(Debug)]
enum Sx {
    List(Vec<Sx>),
    Atom(String),
    Str(String),
}

fn sexpr(s: &str) -> Vec<Sx> {
    fn parse(b: &[char], i: &mut usize, depth: u32) -> Vec<Sx> {
        let mut out = Vec::new();
        while *i < b.len() {
            match b[*i] {
                '(' if depth < 32 => {
                    *i += 1;
                    out.push(Sx::List(parse(b, i, depth + 1)));
                }
                ')' => {
                    *i += 1;
                    return out;
                }
                '"' => {
                    *i += 1;
                    let mut t = String::new();
                    while *i < b.len() && b[*i] != '"' {
                        if b[*i] == '\\' && *i + 1 < b.len() {
                            *i += 1;
                            t.push(match b[*i] {
                                'n' => '\n',
                                't' => '\t',
                                c => c,
                            });
                        } else {
                            t.push(b[*i]);
                        }
                        *i += 1;
                    }
                    *i += 1;
                    out.push(Sx::Str(t));
                }
                c if c.is_whitespace() => *i += 1,
                _ => {
                    let s = *i;
                    while *i < b.len() && !b[*i].is_whitespace() && b[*i] != '(' && b[*i] != ')' {
                        *i += 1;
                    }
                    out.push(Sx::Atom(b[s..*i].iter().collect()));
                }
            }
        }
        out
    }
    let chars: Vec<char> = s.chars().collect();
    parse(&chars, &mut 0, 0)
}

fn sx_text(v: &[Sx]) -> Option<&str> {
    v.iter().find_map(|x| match x {
        Sx::Str(s) | Sx::Atom(s) => Some(s.as_str()),
        _ => None,
    })
}

/// The attributes of a markup tag (`name="value"` pairs).
fn attrs(tag: &str) -> Vec<(&str, String)> {
    let mut out = Vec::new();
    let mut rest = tag.split_once(char::is_whitespace).map_or("", |(_, r)| r);
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let body = &after[1..];
        let end = body.find(q).unwrap_or(body.len());
        out.push((name, unescape(&body[..end])));
        rest = body.get(end + 1..).unwrap_or("");
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn hex_color(v: &str) -> Option<[u8; 4]> {
    let h = v.strip_prefix('#')?;
    let n = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some([n(0)?, n(2)?, n(4)?, 255]),
        8 => Some([n(0)?, n(2)?, n(4)?, n(6)?]),
        _ => None,
    }
}

/// The style a markup tag sets. Pango sizes are 1/1024 point; GIMP lays the text out at
/// the image resolution, so a point is `res / 72` pixels.
fn tag_style(tag: &str, res: f32, resolve: FontResolver) -> SpanStyle {
    let unit = res / 72.0 / 1024.0;
    let mut st = SpanStyle::default();
    match tag.split_whitespace().next().unwrap_or("") {
        "b" => st.bold = Some(true),
        "i" => st.italic = Some(true),
        "u" => st.underline = Some(true),
        "s" => st.strike = Some(true),
        _ => {}
    }
    for (k, v) in attrs(tag) {
        match k {
            "foreground" | "fgcolor" | "color" => st.color = hex_color(&v),
            "size" => {
                st.size = if let Some(pt) = v.strip_suffix("pt") {
                    pt.parse::<f32>().ok().map(|p| p * res / 72.0)
                } else if let Some(px) = v.strip_suffix("px") {
                    px.parse().ok()
                } else {
                    v.parse::<f32>().ok().map(|u| u * unit)
                }
            }
            "letter_spacing" => st.kerning = v.parse::<f32>().ok().map(|u| u * unit),
            "rise" => st.baseline = v.parse::<f32>().ok().map(|u| u * unit),
            "weight" => {
                st.bold =
                    Some(v == "bold" || v == "heavy" || v.parse::<u32>().is_ok_and(|w| w >= 600))
            }
            "style" => st.italic = Some(v == "italic" || v == "oblique"),
            "underline" => st.underline = Some(v != "none"),
            "strikethrough" => st.strike = Some(v == "true"),
            "font_family" | "face" | "font" | "font_desc" => {
                // "Family Bold 12": try the whole name, then without trailing words.
                let words: Vec<&str> = v.split_whitespace().collect();
                for n in (1..=words.len()).rev() {
                    if let Some((family, bold, italic)) = resolve(&words[..n].join(" ")) {
                        st.font = Some(family);
                        st.bold = st.bold.or(Some(bold));
                        st.italic = st.italic.or(Some(italic));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    st
}

fn merge(into: &mut SpanStyle, s: &SpanStyle) {
    macro_rules! m {
        ($($f:ident),*) => { $( if s.$f.is_some() { into.$f = s.$f.clone(); } )* };
    }
    m!(
        font, size, color, bold, italic, underline, strike, baseline, kerning
    );
}

/// GIMP's text markup: plain text and the style of every UTF-16 unit. The word joiners
/// (U+2060) GIMP puts between letters with their own spacing are dropped.
fn markup(m: &str, res: f32, resolve: FontResolver) -> (String, Vec<SpanStyle>) {
    let mut text = String::new();
    let mut styles: Vec<SpanStyle> = Vec::new();
    let mut open: Vec<(String, SpanStyle)> = Vec::new();
    let mut rest = m;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('<') {
            let end = r.find('>').unwrap_or(r.len());
            let tag = &r[..end];
            rest = r.get(end + 1..).unwrap_or("");
            if let Some(t) = tag.strip_prefix('/') {
                if let Some(i) = open.iter().rposition(|(n, _)| n == t.trim()) {
                    open.truncate(i);
                }
            } else if !tag.ends_with('/') {
                let name = tag.split_whitespace().next().unwrap_or("").to_string();
                open.push((name, tag_style(tag, res, resolve)));
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let mut current = SpanStyle::default();
            for (_, s) in &open {
                merge(&mut current, s);
            }
            for ch in unescape(&rest[..end]).chars().filter(|&c| c != '\u{2060}') {
                text.push(ch);
                for _ in 0..ch.len_utf16() {
                    styles.push(current.clone());
                }
            }
            rest = &rest[end..];
        }
    }
    (text, styles)
}

/// Moves what all characters share into the layer style and returns the rest as spans.
fn spans_from(spec: &mut TextSpec, styles: &[SpanStyle]) -> Vec<Span> {
    let mut styles = styles.to_vec();
    macro_rules! uniform {
        ($f:ident) => {
            styles
                .first()
                .and_then(|s| s.$f.clone())
                .filter(|v| styles.iter().all(|s| s.$f.as_ref() == Some(v)))
        };
    }
    if let Some(v) = uniform!(font) {
        spec.font = v;
        styles.iter_mut().for_each(|s| s.font = None);
    }
    if let Some(v) = uniform!(size) {
        spec.size = v;
        styles.iter_mut().for_each(|s| s.size = None);
    }
    if let Some(v) = uniform!(color) {
        spec.color = v;
        styles.iter_mut().for_each(|s| s.color = None);
    }
    if let Some(v) = uniform!(bold) {
        spec.bold = v;
        styles.iter_mut().for_each(|s| s.bold = None);
    }
    if let Some(v) = uniform!(italic) {
        spec.italic = v;
        styles.iter_mut().for_each(|s| s.italic = None);
    }
    if let Some(v) = uniform!(kerning) {
        spec.letter_spacing = v;
        styles.iter_mut().for_each(|s| s.kerning = None);
    }
    // The same rise everywhere only moves the whole text: the layer position has it.
    if uniform!(baseline).is_some() {
        styles.iter_mut().for_each(|s| s.baseline = None);
    }
    let mut spans: Vec<Span> = Vec::new();
    for (i, s) in styles.into_iter().enumerate() {
        if s == SpanStyle::default() {
            continue;
        }
        match spans.last_mut() {
            Some(last) if last.end == i && last.style == s => last.end = i + 1,
            _ => spans.push(Span {
                start: i,
                end: i + 1,
                style: s,
            }),
        }
    }
    spans
}

/// The Farba description of a GIMP text layer (the `gimp-text-layer` parasite).
fn text_spec(src: &str, res: f32, resolve: FontResolver) -> Option<String> {
    let items = sexpr(src);
    let mut spec = TextSpec::default();
    let mut text = None;
    let mut fixed = None;
    let (mut bw, mut bh) = (0.0f32, 0.0f32);
    let mut unit_ok = true;
    for it in &items {
        let Sx::List(v) = it else { continue };
        let (Some(Sx::Atom(key)), rest) = (v.first(), &v[1..]) else {
            continue;
        };
        let num = || sx_text(rest).and_then(|s| s.parse::<f32>().ok());
        match key.as_str() {
            "text" => text = sx_text(rest).map(|s| (s.to_string(), None)),
            "markup" => {
                text = sx_text(rest).map(|m| {
                    let (t, styles) = markup(m, res, resolve);
                    (t, Some(styles))
                })
            }
            "font" => {
                // Since GIMP 3: (font "name" ...) may nest a resource description.
                let name = sx_text(rest).map(str::to_string).or_else(|| {
                    rest.iter().find_map(|x| match x {
                        Sx::List(l) => sx_text(l).map(str::to_string),
                        _ => None,
                    })
                })?;
                match resolve(&name) {
                    Some((family, bold, italic)) => {
                        spec.font = family;
                        spec.bold = bold;
                        spec.italic = italic;
                    }
                    None => {
                        // "Sans-serif Bold Italic" style names.
                        let mut n = name.as_str();
                        for (suffix, flag) in [(" Italic", 1), (" Oblique", 1), (" Bold", 0)] {
                            if let Some(s) = n.strip_suffix(suffix) {
                                n = s;
                                if flag == 0 {
                                    spec.bold = true;
                                } else {
                                    spec.italic = true;
                                }
                            }
                        }
                        spec.font = resolve(n).map_or(n.to_string(), |r| r.0);
                    }
                }
            }
            "font-size" => spec.size = num()?,
            "font-size-unit" => unit_ok = sx_text(rest) == Some("pixels"),
            "antialias" => spec.antialias = sx_text(rest) != Some("no"),
            "indent" => spec.indent = num().unwrap_or(0.0),
            "line-spacing" => spec.line_spacing = num().unwrap_or(0.0),
            "letter-spacing" => spec.letter_spacing = num().unwrap_or(0.0),
            "justify" => {
                spec.justify = match sx_text(rest) {
                    Some("right") => Justify::Right,
                    Some("center") => Justify::Center,
                    Some("fill") => Justify::Fill,
                    _ => Justify::Left,
                }
            }
            "box-mode" => fixed = Some(sx_text(rest) == Some("fixed")),
            "box-width" => bw = num().unwrap_or(0.0),
            "box-height" => bh = num().unwrap_or(0.0),
            "color" => {
                // (color (color-rgb r g b)) or (color (color-rgba r g b a)), 0..1.
                if let Some(Sx::List(c)) = rest.first() {
                    let n: Vec<f32> = c[1..]
                        .iter()
                        .filter_map(|x| match x {
                            Sx::Atom(a) => a.parse().ok(),
                            _ => None,
                        })
                        .collect();
                    if n.len() >= 3 {
                        spec.color = [to8(n[0]), to8(n[1]), to8(n[2]), 255];
                    }
                }
            }
            _ => {}
        }
    }
    if !unit_ok {
        return None;
    }
    let (t, styles) = text?;
    spec.text = t;
    if let Some(styles) = styles {
        spec.spans = spans_from(&mut spec, &styles);
    }
    if fixed == Some(true) && bw > 0.0 && bh > 0.0 {
        spec.box_size = Some([bw, bh]);
    }
    serde_json::to_string(&spec).ok()
}

// ---- reading ---------------------------------------------------------------------------------

/// Reads a GIMP file: the layers as a [`Stack`] and its pixel buffer. `resolve` finds
/// installed fonts for text layers.
pub fn read(path: &Path, resolve: FontResolver) -> R<(Stack, Vec<u8>)> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    read_bytes(&data, resolve)
}

pub fn read_bytes(d: &[u8], resolve: FontResolver) -> R<(Stack, Vec<u8>)> {
    if !d.starts_with(b"gimp xcf ") || d.len() < 14 {
        return Err("This is not a GIMP file".into());
    }
    let tag = &d[9..13];
    let version = if tag == b"file" {
        0
    } else if tag[0] == b'v' {
        std::str::from_utf8(&tag[1..])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(DAMAGED)?
    } else {
        return Err(DAMAGED.into());
    };
    let wide = version >= 11;
    let mut c = Cur::at(d, 14, wide);
    let width = c.u32()?;
    let height = c.u32()?;
    let base = c.u32()?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE || base > 2 {
        return Err(format!("Unsupported image {width} x {height}"));
    }
    let prec = if version >= 4 { c.u32()? } else { 150 };
    let (comp, linear) = precision(version, prec)?;
    let mut compression = 0u8;
    let mut colormap = Vec::new();
    let mut res = 72.0f32;
    for (ty, p) in c.props()? {
        match ty {
            1 => colormap = p.to_vec(),
            17 => compression = p.first().copied().unwrap_or(0),
            19 if p.len() >= 4 => {
                let r = f32::from_be_bytes(p[..4].try_into().unwrap());
                if r.is_finite() && r > 1.0 {
                    res = r;
                }
            }
            _ => {}
        }
    }
    let mut layer_ptrs = Vec::new();
    loop {
        let p = c.ptr()?;
        if p == 0 {
            break;
        }
        layer_ptrs.push(p);
    }
    let ctx = Ctx {
        d,
        wide,
        compression,
        comp,
        linear,
        colormap,
    };

    let mut notes_modes = BTreeSet::new();
    let mut groups = false;
    let mut text_lost = false;
    // Group properties by item path (to apply to their layers).
    let mut group_props: HashMap<Vec<u32>, (f32, bool)> = HashMap::new();
    let mut out: Vec<(StackLayer, Vec<u8>)> = Vec::new();
    let mut total = 0usize;
    // Layers come top first; groups before their children.
    for &p in &layer_ptrs {
        let l = layer(&ctx, p)?;
        if l.floating {
            continue;
        }
        let (mut opacity, mut visible) = (l.opacity, l.visible);
        for k in 1..l.path.len() {
            if let Some(&(o, v)) = group_props.get(&l.path[..k]) {
                opacity *= o;
                visible &= v;
            }
        }
        if let Some(m) = mode_name(l.mode) {
            notes_modes.insert(m.to_string());
        }
        if l.group {
            groups = true;
            group_props.insert(l.path.clone(), (opacity, visible));
            continue;
        }
        if l.w == 0 || l.h == 0 {
            continue;
        }
        let (w, h, bpp, raw) = hierarchy(&ctx, l.hierarchy)?;
        if (w, h) != (l.w, l.h) {
            return Err(DAMAGED.into());
        }
        total += w * h * 4;
        if total > MAX_BYTES {
            return Err("The image is too large to open".into());
        }
        let mut rgba = to_rgba(&ctx, l.ty, &raw, w * h, bpp);
        if l.apply_mask
            && let Some(m) = mask(&ctx, l.mask)?
            && m.len() == w * h
        {
            for (px, &mv) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&m) {
                px[3] = ((px[3] as u32 * mv as u32 + 127) / 255) as u8;
            }
        }
        let text = match &l.text {
            Some(t) if !l.text_modified => {
                let s = text_spec(t, res, resolve);
                text_lost |= s.is_none();
                s
            }
            Some(_) => None,
            None => None,
        };
        out.push((
            StackLayer {
                name: l.name,
                x: l.x,
                y: l.y,
                width: w as u32,
                height: h as u32,
                opacity,
                visible,
                lock_alpha: l.lock_alpha,
                lock_position: l.lock_position,
                text,
                name_from_text: false,
                source: None,
            },
            rgba,
        ));
    }
    if out.is_empty() {
        return Err("The file has no layers".into());
    }
    out.reverse(); // bottom to top
    let mut notes = Vec::new();
    if groups {
        notes.push("Layer groups were merged into a single list of layers".to_string());
    }
    if !notes_modes.is_empty() {
        let m: Vec<String> = notes_modes.into_iter().collect();
        notes.push(format!(
            "Layer modes are not supported, shown as Normal: {}",
            m.join(", ")
        ));
    }
    if text_lost {
        notes.push("Some text layers could not be made editable and were opened as pixels".into());
    }
    let mut buf = Vec::with_capacity(total);
    let mut layers = Vec::with_capacity(out.len());
    for (l, px) in out {
        buf.extend_from_slice(&px);
        layers.push(l);
    }
    Ok((
        Stack {
            width,
            height,
            active: layers.len() - 1,
            layers,
            selection: false,
            notes,
        },
        buf,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_planes() {
        // Two pixels, two bytes each: plane 0 = run of 2 x 7, plane 1 = literal [1, 2].
        let mut out = [0u8; 4];
        unrle(&[1, 7, 254, 1, 2], &mut out, 2).unwrap();
        assert_eq!(out, [7, 1, 7, 2]);
    }

    #[test]
    fn markup_spans() {
        let m = "<markup><span foreground=\"#ff0000\"><span size=\"24576\">a<b>bc</b>&amp;<i>d</i>\u{2060}</span></span></markup>";
        let (t, styles) = markup(m, 300.0, &|_| None);
        assert_eq!(t, "abc&d");
        let mut spec = TextSpec::default();
        let s = spans_from(&mut spec, &styles);
        // Shared by all: colour and 24 pt at 300 ppi = 100 px.
        assert_eq!(spec.color, [255, 0, 0, 255]);
        assert!((spec.size - 100.0).abs() < 0.01);
        assert_eq!((s[0].start, s[0].end, s[0].style.bold), (1, 3, Some(true)));
        assert_eq!(
            (s[1].start, s[1].end, s[1].style.italic),
            (4, 5, Some(true))
        );
    }

    #[test]
    fn text_parasite() {
        let p = r#"(text "Hi\nthere") (font "Serif Bold") (font-size 40.000000) (font-size-unit pixels) (color (color-rgb 1.000000 0.000000 0.000000)) (justify center) (box-mode fixed) (box-width 200.000000) (box-height 100.000000)"#;
        let json = text_spec(p, 72.0, &|_| None).unwrap();
        let s: TextSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(s.text, "Hi\nthere");
        assert_eq!((s.font.as_str(), s.bold, s.size), ("Serif", true, 40.0));
        assert_eq!(s.color, [255, 0, 0, 255]);
        assert_eq!(s.justify, Justify::Center);
        assert_eq!(s.box_size, Some([200.0, 100.0]));
    }

    /// The samples: GIMP's own test file (2.6) and `$FARBA_XCF` files.
    #[test]
    #[ignore]
    fn xcf_samples() {
        for p in std::env::var("FARBA_XCF")
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
        {
            let (s, buf) = read(Path::new(p), &|_| None).unwrap();
            println!("{p}: {}x{} {:?}", s.width, s.height, s.notes);
            for l in &s.layers {
                println!(
                    "  {:?} {}x{} at {},{} op {} vis {} text {:?}",
                    l.name, l.width, l.height, l.x, l.y, l.opacity, l.visible, l.text
                );
            }
            assert_eq!(buf.len(), s.byte_len());
            let img = s.to_document(&buf).unwrap().composite().to_rgba();
            let out = std::env::temp_dir().join(format!(
                "{}.png",
                Path::new(p).file_stem().unwrap().to_string_lossy()
            ));
            image::save_buffer(
                &out,
                &img,
                s.width,
                s.height,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
            println!("  -> {}", out.display());
        }
    }
}
