//! Photoshop text layers (the `TySh` block) as Farba text (farba-text `TextSpec`), best
//! effort: the text, font (resolved to an installed family), size (times the layer's
//! transform scale), colour, bold / italic (also faux), underline, strikethrough, tracking,
//! justification and paragraph boxes, with per-fragment styles. The layer keeps the pixels
//! Photoshop rendered; only an edit re-renders it with Farba's engine. Rotated or skewed
//! text is not made editable.
//!
//! Formats from Adobe's file format description: the "descriptor" structure and the
//! EngineData text (a PostScript-like dictionary syntax).

use farba_text::{Justify, Span, SpanStyle, TextSpec};

/// Finds an installed font for a PostScript / family name: `(family, bold, italic)`.
pub type FontResolver<'a> = &'a dyn Fn(&str) -> Option<(String, bool, bool)>;

// ---- descriptors -----------------------------------------------------------------------------

/// A descriptor value: only what text layers need is kept (the rest is read past).
#[derive(Debug, Clone)]
enum Item {
    Text(String),
    Raw(Vec<u8>),
    Other,
}

struct Rd<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let e = self.p.checked_add(n)?;
        let s = self.d.get(self.p..e)?;
        self.p = e;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    fn unicode(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.take(n.checked_mul(2)?)?;
        let u: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Some(
            String::from_utf16_lossy(&u)
                .trim_end_matches('\0')
                .to_string(),
        )
    }
    /// A key or class id: length then string, or 0 then a 4-byte code.
    fn id(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.take(if n == 0 { 4 } else { n })?;
        Some(String::from_utf8_lossy(b).to_string())
    }
    fn descriptor(&mut self, depth: u32) -> Option<Vec<(String, Item)>> {
        if depth > 32 {
            return None;
        }
        self.unicode()?;
        self.id()?;
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(256));
        for _ in 0..n {
            let key = self.id()?;
            let v = self.item(depth)?;
            out.push((key, v));
        }
        Some(out)
    }
    fn item(&mut self, depth: u32) -> Option<Item> {
        let ty: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(match &ty {
            b"TEXT" => Item::Text(self.unicode()?),
            b"tdta" => {
                let n = self.u32()? as usize;
                Item::Raw(self.take(n)?.to_vec())
            }
            b"doub" => {
                self.f64()?;
                Item::Other
            }
            b"UntF" => {
                self.take(4)?;
                self.f64()?;
                Item::Other
            }
            b"UnFl" => {
                self.take(4)?;
                let n = self.u32()? as usize;
                for _ in 0..n {
                    self.f64()?;
                }
                Item::Other
            }
            b"long" => {
                self.u32()?;
                Item::Other
            }
            b"comp" => {
                self.take(8)?;
                Item::Other
            }
            b"bool" => {
                self.take(1)?;
                Item::Other
            }
            b"enum" => {
                self.id()?;
                Item::Text(self.id()?)
            }
            b"Objc" | b"GlbO" => {
                self.descriptor(depth + 1)?;
                Item::Other
            }
            b"VlLs" => {
                let n = self.u32()? as usize;
                for _ in 0..n {
                    self.item(depth + 1)?;
                }
                Item::Other
            }
            b"type" | b"GlbC" => {
                self.unicode()?;
                self.id()?;
                Item::Other
            }
            b"alis" | b"Pth " => {
                let n = self.u32()? as usize;
                self.take(n)?;
                Item::Other
            }
            b"obj " => {
                let n = self.u32()? as usize;
                for _ in 0..n {
                    let t: [u8; 4] = self.take(4)?.try_into().ok()?;
                    match &t {
                        b"prop" => {
                            self.unicode()?;
                            self.id()?;
                            self.id()?;
                        }
                        b"Clss" => {
                            self.unicode()?;
                            self.id()?;
                        }
                        b"Enmr" => {
                            self.unicode()?;
                            self.id()?;
                            self.id()?;
                            self.id()?;
                        }
                        b"rele" => {
                            self.unicode()?;
                            self.id()?;
                            self.u32()?;
                        }
                        b"Idnt" | b"indx" => {
                            self.u32()?;
                        }
                        b"name" => {
                            self.unicode()?;
                            self.id()?;
                            self.unicode()?;
                        }
                        _ => return None,
                    }
                }
                Item::Other
            }
            _ => return None,
        })
    }
}

fn get<'a>(o: &'a [(String, Item)], key: &str) -> Option<&'a Item> {
    o.iter()
        .find(|(k, _)| k.trim() == key.trim())
        .map(|(_, v)| v)
}

// ---- EngineData ------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Ed {
    Dict(Vec<(String, Ed)>),
    List(Vec<Ed>),
    Num(f64),
    Bool(bool),
    Str(String),
    Name(String),
}

impl Ed {
    fn get(&self, key: &str) -> Option<&Ed> {
        match self {
            Ed::Dict(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn path(&self, keys: &[&str]) -> Option<&Ed> {
        keys.iter().try_fold(self, |e, k| e.get(k))
    }
    fn num(&self) -> Option<f64> {
        match self {
            Ed::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn bool(&self) -> Option<bool> {
        match self {
            Ed::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn list(&self) -> &[Ed] {
        match self {
            Ed::List(v) => v,
            _ => &[],
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Ed::Str(s) => Some(s),
            _ => None,
        }
    }
}

struct EdParser<'a> {
    d: &'a [u8],
    p: usize,
}

impl EdParser<'_> {
    fn ws(&mut self) {
        while self.p < self.d.len() && matches!(self.d[self.p], b' ' | b'\t' | b'\r' | b'\n') {
            self.p += 1;
        }
    }
    fn value(&mut self, depth: u32) -> Option<Ed> {
        if depth > 64 {
            return None;
        }
        self.ws();
        let c = *self.d.get(self.p)?;
        match c {
            b'<' if self.d.get(self.p + 1) == Some(&b'<') => {
                self.p += 2;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.d.get(self.p..self.p + 2) == Some(b">>") {
                        self.p += 2;
                        return Some(Ed::Dict(v));
                    }
                    if *self.d.get(self.p)? != b'/' {
                        return None;
                    }
                    let Ed::Name(k) = self.value(depth + 1)? else {
                        return None;
                    };
                    let val = self.value(depth + 1)?;
                    v.push((k, val));
                }
            }
            b'[' => {
                self.p += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if *self.d.get(self.p)? == b']' {
                        self.p += 1;
                        return Some(Ed::List(v));
                    }
                    v.push(self.value(depth + 1)?);
                }
            }
            b'(' => {
                self.p += 1;
                let mut raw = Vec::new();
                while self.p < self.d.len() {
                    let b = self.d[self.p];
                    self.p += 1;
                    match b {
                        b'\\' => {
                            if let Some(&n) = self.d.get(self.p) {
                                raw.push(n);
                                self.p += 1;
                            }
                        }
                        b')' => break,
                        _ => raw.push(b),
                    }
                }
                Some(Ed::Str(decode_str(&raw)))
            }
            b'/' => {
                self.p += 1;
                let s = self.p;
                while self.p < self.d.len()
                    && !matches!(
                        self.d[self.p],
                        b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'[' | b']' | b'<' | b'>' | b'('
                    )
                {
                    self.p += 1;
                }
                Some(Ed::Name(
                    String::from_utf8_lossy(&self.d[s..self.p]).to_string(),
                ))
            }
            _ => {
                let s = self.p;
                while self.p < self.d.len()
                    && !matches!(
                        self.d[self.p],
                        b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'[' | b']' | b'<' | b'>' | b'('
                    )
                {
                    self.p += 1;
                }
                let w = std::str::from_utf8(&self.d[s..self.p]).ok()?;
                if self.p == s {
                    return None;
                }
                Some(match w {
                    "true" => Ed::Bool(true),
                    "false" => Ed::Bool(false),
                    _ => Ed::Num(w.parse().unwrap_or(0.0)),
                })
            }
        }
    }
}

/// EngineData strings are UTF-16BE with a byte order mark.
fn decode_str(raw: &[u8]) -> String {
    if raw.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = raw[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(raw).to_string()
    }
}

// ---- TySh -> TextSpec ------------------------------------------------------------------------

fn color(e: Option<&Ed>) -> Option<[u8; 4]> {
    let v = e?.get("Values")?.list();
    let n = |i: usize| v.get(i).and_then(Ed::num).unwrap_or(0.0);
    let c = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    (v.len() >= 4).then(|| [c(n(1)), c(n(2)), c(n(3)), c(n(0))])
}

/// A style run as a Farba style (absolute values; `scale` = the layer's transform scale).
fn style(s: &Ed, fonts: &[Ed], scale: f64, resolve: FontResolver) -> SpanStyle {
    let mut out = SpanStyle::default();
    if let Some(i) = s.get("Font").and_then(Ed::num)
        && let Some(name) = fonts
            .get(i as usize)
            .and_then(|f| f.get("Name"))
            .and_then(Ed::str)
    {
        match resolve(name) {
            Some((family, bold, italic)) => {
                out.font = Some(family);
                out.bold = Some(bold);
                out.italic = Some(italic);
            }
            None => out.font = Some(name.to_string()),
        }
    }
    if let Some(sz) = s.get("FontSize").and_then(Ed::num) {
        out.size = Some((sz * scale) as f32);
    }
    if s.get("FauxBold").and_then(Ed::bool) == Some(true) {
        out.bold = Some(true);
    }
    if s.get("FauxItalic").and_then(Ed::bool) == Some(true) {
        out.italic = Some(true);
    }
    if let Some(u) = s.get("Underline").and_then(Ed::bool) {
        out.underline = Some(u);
    }
    if let Some(st) = s.get("Strikethrough").and_then(Ed::bool) {
        out.strike = Some(st);
    }
    if let Some(c) = color(s.get("FillColor")) {
        out.color = Some(c);
    }
    if let (Some(t), Some(sz)) = (s.get("Tracking").and_then(Ed::num), out.size) {
        out.kerning = Some((t / 1000.0) as f32 * sz);
    }
    if let Some(b) = s.get("BaselineShift").and_then(Ed::num)
        && b != 0.0
    {
        out.baseline = Some((b * scale) as f32);
    }
    out
}

/// Builds the Farba text description of a `TySh` block, or `None` (not editable).
pub fn text_spec(tysh: &[u8], resolve: FontResolver) -> Option<String> {
    let mut r = Rd { d: tysh, p: 0 };
    r.take(2)?; // version
    let t: Vec<f64> = (0..6).map(|_| r.f64()).collect::<Option<_>>()?;
    let (xx, xy, yx, yy) = (t[0], t[1], t[2], t[3]);
    // Only unrotated, unskewed text with the same scale on both axes.
    if xy.abs() > 1e-6 || yx.abs() > 1e-6 || xx <= 0.0 || (xx - yy).abs() > 1e-3 * xx.abs() {
        return None;
    }
    let scale = yy;
    r.take(2)?; // text version
    r.u32()?; // descriptor version
    let d = r.descriptor(0)?;
    let txt = match get(&d, "Txt ") {
        Some(Item::Text(s)) => s.clone(),
        _ => return None,
    };
    let Some(Item::Raw(engine)) = get(&d, "EngineData") else {
        return None;
    };
    let ed = EdParser { d: engine, p: 0 }.value(0)?;

    let fonts = ed
        .path(&["ResourceDict", "FontSet"])
        .or_else(|| ed.path(&["DocumentResources", "FontSet"]))
        .map(Ed::list)
        .unwrap_or(&[]);
    let engine_dict = ed.get("EngineDict")?;
    // Photoshop's text ends with a carriage return; paragraphs are separated by CR.
    let text: String = txt
        .trim_end_matches(['\r', '\0'])
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let len16 = text.encode_utf16().count();

    // Style runs.
    let runs = engine_dict.get("StyleRun")?;
    let sheets = runs.get("RunArray").map(Ed::list).unwrap_or(&[]);
    let lengths = runs.get("RunLengthArray").map(Ed::list).unwrap_or(&[]);
    let mut styled: Vec<(usize, usize, SpanStyle)> = Vec::new();
    let mut pos = 0usize;
    for (sheet, len) in sheets.iter().zip(lengths) {
        let n = len.num().unwrap_or(0.0).max(0.0) as usize;
        let data = sheet
            .path(&["StyleSheet", "StyleSheetData"])
            .cloned()
            .unwrap_or(Ed::Dict(Vec::new()));
        let end = (pos + n).min(len16);
        if end > pos {
            styled.push((pos, end, style(&data, fonts, scale, resolve)));
        }
        pos += n;
    }
    // The layer-wide style: the first run (filled with defaults).
    let first = styled.first().map(|s| s.2.clone()).unwrap_or_default();
    let mut spec = TextSpec {
        text: text.clone(),
        ..TextSpec::default()
    };
    if let Some(f) = &first.font {
        spec.font = f.clone();
    }
    spec.size = first.size.unwrap_or(12.0 * scale as f32);
    spec.color = first.color.unwrap_or([0, 0, 0, 255]);
    spec.bold = first.bold.unwrap_or(false);
    spec.italic = first.italic.unwrap_or(false);
    spec.letter_spacing = first.kerning.unwrap_or(0.0);
    // Other runs become spans with what differs from the layer style.
    for (start, end, s) in styled.iter().skip(1) {
        let diff = SpanStyle {
            font: s.font.clone().filter(|f| *f != spec.font),
            size: s.size.filter(|v| (*v - spec.size).abs() > 0.01),
            color: s.color.filter(|c| *c != spec.color),
            bold: s.bold.filter(|b| *b != spec.bold),
            italic: s.italic.filter(|i| *i != spec.italic),
            underline: s.underline.filter(|u| *u),
            strike: s.strike.filter(|v| *v),
            baseline: s.baseline,
            kerning: s
                .kerning
                .filter(|k| (*k - spec.letter_spacing).abs() > 0.01),
        };
        if diff != SpanStyle::default() {
            spec.spans.push(Span {
                start: *start,
                end: *end,
                style: diff,
            });
        }
    }
    // Underline / strikethrough of the first run apply as a span (no layer-wide flag).
    if let Some((s, e, st)) = styled.first()
        && (st.underline == Some(true) || st.strike == Some(true))
    {
        spec.spans.insert(
            0,
            Span {
                start: *s,
                end: *e,
                style: SpanStyle {
                    underline: st.underline.filter(|u| *u),
                    strike: st.strike.filter(|v| *v),
                    ..SpanStyle::default()
                },
            },
        );
    }
    // Justification of the first paragraph.
    let just = engine_dict
        .path(&["ParagraphRun", "RunArray"])
        .and_then(|a| a.list().first().cloned())
        .and_then(|p| {
            p.path(&["ParagraphSheet", "Properties", "Justification"])
                .and_then(Ed::num)
        });
    spec.justify = match just.unwrap_or(0.0) as i32 {
        1 => Justify::Right,
        2 => Justify::Center,
        3..=6 => Justify::Fill,
        _ => Justify::Left,
    };
    // Paragraph (box) text: a fixed box.
    if let Some(Ed::List(b)) = engine_dict
        .path(&["Rendered", "Shapes", "Children"])
        .and_then(|c| {
            c.list()
                .first()
                .and_then(|s| s.path(&["Cookie", "Photoshop", "BoxBounds"]))
                .cloned()
        })
        && b.len() == 4
        && engine_dict
            .path(&["Rendered", "Shapes", "Children"])
            .and_then(|c| {
                c.list()
                    .first()
                    .and_then(|s| s.get("ShapeType"))
                    .and_then(Ed::num)
            })
            == Some(1.0)
    {
        let n = |i: usize| b[i].num().unwrap_or(0.0);
        spec.box_size = Some([
            ((n(2) - n(0)) * scale) as f32,
            ((n(3) - n(1)) * scale) as f32,
        ]);
    }
    serde_json::to_string(&spec).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_data_parses() {
        let src = b"<< /EngineDict << /Editor << /Text (\xFE\xFF\x00H\x00i\x00\r) >> /Flag true /N 12.5 /L [ 1 2 << /A /Name >> ] >> >>";
        let e = EdParser { d: src, p: 0 }.value(0).unwrap();
        assert_eq!(
            e.path(&["EngineDict", "Editor", "Text"]).and_then(Ed::str),
            Some("Hi\r")
        );
        assert_eq!(e.path(&["EngineDict", "N"]).and_then(Ed::num), Some(12.5));
        assert_eq!(
            e.path(&["EngineDict", "Flag"]).and_then(Ed::bool),
            Some(true)
        );
        assert_eq!(
            e.path(&["EngineDict", "L"]).map(|l| l.list().len()),
            Some(3)
        );
    }
}
