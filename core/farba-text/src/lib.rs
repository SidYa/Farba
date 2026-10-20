//! Text layers: layout with system fonts and rasterisation to straight RGBA8 (native only).
//!
//! The frontend owns the editable text model and sends it as a [`TextSpec`] (JSON). This
//! crate lays it out with `cosmic-text` (shaping, font fallback, wrapping, alignment) and
//! draws it with `swash`, and returns the pixels plus the positions of every character
//! boundary, so the frontend can draw the caret and the selection and hit-test clicks.
//!
//! Text offsets are UTF-16 code units (JavaScript string indices) everywhere in the API.

use std::ops::Range;

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, LetterSpacing, Metrics, Shaping, Style,
    SwashCache, TextDecoration, UnderlineStyle, Weight, Wrap,
};
use serde::{Deserialize, Serialize};

/// Default line height as a multiple of the font size (before GIMP's extra line spacing).
const LINE_HEIGHT: f32 = 1.2;

/// Style of a fragment; `None` = the layer's value.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SpanStyle {
    pub font: Option<String>,
    pub size: Option<f32>,
    /// Straight RGBA.
    pub color: Option<[u8; 4]>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Raise (positive) or lower the fragment, in pixels.
    pub baseline: Option<f32>,
    /// Extra space after every character of the fragment, in pixels (GIMP's kerning).
    pub kerning: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub style: SpanStyle,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
    Fill,
}

/// The whole text layer (GIMP's text tool options plus per-fragment styles).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TextSpec {
    pub text: String,
    pub font: String,
    /// Pixels.
    pub size: f32,
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub justify: Justify,
    /// First-line indent of every paragraph, pixels.
    pub indent: f32,
    /// Extra space between lines, pixels (GIMP's line spacing).
    pub line_spacing: f32,
    /// Extra space between characters, pixels (GIMP's letter spacing).
    pub letter_spacing: f32,
    /// Fixed box size (wraps and clips); `None` = dynamic (grows with the text).
    pub box_size: Option<[f32; 2]>,
    pub antialias: bool,
    pub spans: Vec<Span>,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            font: "Segoe UI".into(),
            size: 62.0,
            color: [0, 0, 0, 255],
            bold: false,
            italic: false,
            justify: Justify::Left,
            indent: 0.0,
            line_spacing: 0.0,
            letter_spacing: 0.0,
            box_size: None,
            antialias: true,
            spans: Vec::new(),
        }
    }
}

/// One laid-out line (a wrapped paragraph gives several).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineLayout {
    /// Text range of the line (UTF-16).
    pub start: usize,
    pub end: usize,
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
    /// Character boundaries `[index, x]`, sorted by index (UTF-16 index, pixels).
    pub carets: Vec<(usize, f32)>,
}

pub struct Rendered {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA8, `width * height * 4`.
    pub rgba: Vec<u8>,
    pub lines: Vec<LineLayout>,
}

/// UTF-16 <-> byte offset conversion for one string.
struct Offsets {
    /// UTF-16 offset of every byte boundary (len + 1 entries; inner bytes of a char repeat).
    utf16_at_byte: Vec<usize>,
}

impl Offsets {
    fn new(s: &str) -> Self {
        let mut v = vec![0; s.len() + 1];
        let mut u = 0;
        for (b, c) in s.char_indices() {
            for k in 0..c.len_utf8() {
                v[b + k] = u;
            }
            u += c.len_utf16();
        }
        v[s.len()] = u;
        Self { utf16_at_byte: v }
    }

    fn utf16(&self, byte: usize) -> usize {
        self.utf16_at_byte[byte.min(self.utf16_at_byte.len() - 1)]
    }

    /// Byte offset of a UTF-16 offset (rounded down to a char boundary).
    fn byte(&self, utf16: usize) -> usize {
        self.utf16_at_byte
            .partition_point(|&u| u < utf16)
            .min(self.utf16_at_byte.len() - 1)
    }
}

/// The layout engine; loading the system fonts takes a moment, so keep one around.
pub struct TextEngine {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// Loads the system fonts.
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// Font family names, sorted, without duplicates.
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .fonts
            .db()
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// The installed family of a font named in a file (a PostScript name such as
    /// `Arial-BoldMT`, or a family name): `(family, bold, italic)`.
    pub fn resolve_font(&self, name: &str) -> Option<(String, bool, bool)> {
        let faces = || self.fonts.db().faces();
        let face = faces()
            .find(|f| f.post_script_name.eq_ignore_ascii_case(name))
            .or_else(|| {
                faces().find(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
            })?;
        let family = face.families.first()?.0.clone();
        let bold = face.weight.0 >= 600;
        let italic = face.style != cosmic_text::fontdb::Style::Normal;
        Some((family, bold, italic))
    }

    /// Lays out and draws a text layer.
    pub fn render(&mut self, spec: &TextSpec) -> Rendered {
        let text = &spec.text;
        let offsets = Offsets::new(text);
        let size = spec.size.max(1.0);
        let metrics = Metrics::new(size, size * LINE_HEIGHT + spec.line_spacing);
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        let (box_w, box_h) = match spec.box_size {
            Some([w, h]) => (Some(w.max(1.0)), Some(h.max(1.0))),
            None => (None, None),
        };
        buffer.set_wrap(if box_w.is_some() {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        });
        buffer.set_size(box_w, None);

        // Style runs: split the text at every span boundary; each piece gets its style.
        let base = SpanStyle {
            font: Some(spec.font.clone()),
            size: Some(size),
            color: Some(spec.color),
            bold: Some(spec.bold),
            italic: Some(spec.italic),
            underline: Some(false),
            strike: Some(false),
            baseline: Some(0.0),
            kerning: Some(0.0),
        };
        let pieces = style_pieces(text, &offsets, &spec.spans);
        // Baseline shifts per piece, looked up through the glyphs' metadata.
        let mut shifts = Vec::with_capacity(pieces.len());
        let owned: Vec<(Range<usize>, SpanStyle)> = pieces
            .into_iter()
            .map(|(r, st)| (r, merge(&base, &st)))
            .collect();
        let rich = owned.iter().enumerate().map(|(i, (r, st))| {
            let fsize = st.size.unwrap_or(size).max(1.0);
            let mut attrs = Attrs::new()
                .family(Family::Name(st.font.as_deref().unwrap_or(&spec.font)))
                .weight(if st.bold == Some(true) {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                })
                .style(if st.italic == Some(true) {
                    Style::Italic
                } else {
                    Style::Normal
                })
                .metrics(Metrics::new(fsize, fsize * LINE_HEIGHT + spec.line_spacing))
                .metadata(i);
            if let Some([r, g, b, a]) = st.color {
                attrs = attrs.color(Color::rgba(r, g, b, a));
            }
            let spacing = spec.letter_spacing + st.kerning.unwrap_or(0.0);
            if spacing != 0.0 {
                attrs.letter_spacing_opt = Some(LetterSpacing(spacing / fsize));
            }
            let mut deco = TextDecoration::new();
            if st.underline == Some(true) {
                deco.underline = UnderlineStyle::Single;
            }
            deco.strikethrough = st.strike == Some(true);
            attrs.text_decoration = deco;
            (&text[r.clone()], attrs)
        });
        let align = match spec.justify {
            Justify::Left => Align::Left,
            Justify::Center => Align::Center,
            Justify::Right => Align::Right,
            Justify::Fill => Align::Justified,
        };
        let default_attrs = Attrs::new().family(Family::Name(&spec.font));
        buffer.set_rich_text(rich, &default_attrs, Shaping::Advanced, Some(align));
        for (_, st) in &owned {
            shifts.push(st.baseline.unwrap_or(0.0));
        }
        buffer.shape_until_scroll(&mut self.fonts, false);

        // Paragraph start offsets (UTF-16), to make line-relative offsets global.
        let mut para_starts = vec![0usize];
        for (b, c) in text.char_indices() {
            if c == '\n' {
                para_starts.push(offsets.utf16(b + 1));
            }
        }
        let para_bytes: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(b, _)| b + 1))
            .collect();

        // The size: the box, or what the text needs (at least one line, so an empty
        // layer still shows where the caret is).
        let mut lines = Vec::new();
        let mut text_w: f32 = 0.0;
        let mut text_h: f32 = 0.0;
        for run in buffer.layout_runs() {
            text_w = text_w.max(run.line_w);
            text_h = text_h.max(run.line_top + run.line_height);
            let pb = para_bytes.get(run.line_i).copied().unwrap_or(0);
            let off16 = |b: usize| offsets.utf16(pb + b);
            let mut carets: Vec<(usize, f32)> = Vec::new();
            let indent = if run.glyphs.first().is_none_or(|g| g.start == 0) {
                spec.indent
            } else {
                0.0
            };
            for g in run.glyphs {
                carets.push((off16(g.start), g.x + indent));
                carets.push((off16(g.end), g.x + g.w + indent));
            }
            carets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            carets.dedup_by(|a, b| a.0 == b.0);
            let (start, end) = match (carets.first(), carets.last()) {
                (Some(a), Some(b)) => (a.0, b.0),
                _ => {
                    // An empty line: the caret sits at the alignment position.
                    let s = para_starts.get(run.line_i).copied().unwrap_or(0);
                    let w = box_w.unwrap_or(0.0);
                    let x = match spec.justify {
                        Justify::Center => w / 2.0,
                        Justify::Right => w,
                        _ => spec.indent,
                    };
                    carets.push((s, x));
                    (s, s)
                }
            };
            lines.push(LineLayout {
                start,
                end,
                top: run.line_top,
                height: run.line_height,
                baseline: run.line_y,
                carets,
            });
        }
        if lines.is_empty() {
            lines.push(LineLayout {
                start: 0,
                end: 0,
                top: 0.0,
                height: metrics.line_height,
                baseline: size,
                carets: vec![(0, spec.indent)],
            });
            text_h = metrics.line_height;
        }
        let width = box_w.unwrap_or(text_w + spec.indent).ceil().max(1.0) as u32;
        let height = box_h.unwrap_or(text_h).ceil().max(1.0) as u32;
        let mut rgba = vec![0u8; width as usize * height as usize * 4];

        // Draw: glyphs (shifted by their fragment's baseline), then decorations.
        let antialias = spec.antialias;
        let mut put = |x: i32, y: i32, c: Color| {
            if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                return;
            }
            let [r, g, b, a] = c.as_rgba();
            let a = if antialias {
                a
            } else if a >= 128 {
                255
            } else {
                0
            };
            if a == 0 {
                return;
            }
            let i = (y as usize * width as usize + x as usize) * 4;
            over(&mut rgba[i..i + 4], [r, g, b, a]);
        };
        let default_color = Color::rgba(spec.color[0], spec.color[1], spec.color[2], spec.color[3]);
        for run in buffer.layout_runs() {
            let indent = if run.glyphs.first().is_none_or(|g| g.start == 0) {
                spec.indent
            } else {
                0.0
            };
            for g in run.glyphs {
                let shift = shifts.get(g.metadata).copied().unwrap_or(0.0);
                let p = g.physical((indent, run.line_y - shift), 1.0);
                let color = g.color_opt.unwrap_or(default_color);
                let base_a = color.a() as u32;
                self.cache
                    .with_pixels(&mut self.fonts, p.cache_key, color, |x, y, c| {
                        // Masks come without the colour's own alpha.
                        let [r, gg, b, a] = c.as_rgba();
                        let a = (a as u32 * base_a / 255) as u8;
                        put(p.x + x, p.y + y, Color::rgba(r, gg, b, a));
                    });
            }
            let mut deco = Decorations {
                put: &mut put,
                dx: indent as i32,
            };
            cosmic_text::render_decoration(&mut deco, &run, default_color);
        }
        Rendered {
            width,
            height,
            rgba,
            lines,
        }
    }
}

/// Draws decoration rectangles through the pixel callback.
struct Decorations<'a, F: FnMut(i32, i32, Color)> {
    put: &'a mut F,
    dx: i32,
}

impl<F: FnMut(i32, i32, Color)> cosmic_text::Renderer for Decorations<'_, F> {
    fn rectangle(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        for yy in y..y + h as i32 {
            for xx in x..x + w as i32 {
                (self.put)(xx + self.dx, yy, color);
            }
        }
    }

    fn glyph(&mut self, _: cosmic_text::PhysicalGlyph, _: Color) {}
}

/// Straight-alpha "over".
fn over(dst: &mut [u8], src: [u8; 4]) {
    let sa = src[3] as f32 / 255.0;
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return;
    }
    for c in 0..3 {
        dst[c] = ((src[c] as f32 * sa + dst[c] as f32 * da * (1.0 - sa)) / oa + 0.5) as u8;
    }
    dst[3] = (oa * 255.0 + 0.5) as u8;
}

/// `over` style values replace `base` ones.
fn merge(base: &SpanStyle, over: &SpanStyle) -> SpanStyle {
    SpanStyle {
        font: over.font.clone().or_else(|| base.font.clone()),
        size: over.size.or(base.size),
        color: over.color.or(base.color),
        bold: over.bold.or(base.bold),
        italic: over.italic.or(base.italic),
        underline: over.underline.or(base.underline),
        strike: over.strike.or(base.strike),
        baseline: over.baseline.or(base.baseline),
        kerning: over.kerning.or(base.kerning),
    }
}

/// Splits the text (byte ranges) at every span boundary; each piece carries the merged style
/// of the spans covering it (later spans win). Covers the whole text.
fn style_pieces(text: &str, offsets: &Offsets, spans: &[Span]) -> Vec<(Range<usize>, SpanStyle)> {
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for s in spans {
        cuts.push(offsets.byte(s.start));
        cuts.push(offsets.byte(s.end));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let (ua, ub) = (offsets.utf16(a), offsets.utf16(b));
        let mut style = SpanStyle::default();
        for s in spans
            .iter()
            .filter(|s| s.start <= ua && s.end >= ub && s.start < s.end)
        {
            style = merge(&style, &s.style);
        }
        out.push((a..b, style));
    }
    if out.is_empty() {
        out.push((0..0, SpanStyle::default()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> TextEngine {
        TextEngine::new()
    }

    fn spec(text: &str) -> TextSpec {
        TextSpec {
            text: text.into(),
            font: "Arial".into(),
            size: 40.0,
            ..TextSpec::default()
        }
    }

    fn ink(r: &Rendered) -> usize {
        r.rgba.chunks(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn offsets_round_trip() {
        let o = Offsets::new("aї😀b");
        // a = 1 byte / 1 unit, ї = 2 / 1, 😀 = 4 / 2.
        assert_eq!(o.utf16(0), 0);
        assert_eq!(o.utf16(1), 1);
        assert_eq!(o.utf16(3), 2);
        assert_eq!(o.utf16(7), 4);
        assert_eq!(o.byte(2), 3);
        assert_eq!(o.byte(4), 7);
    }

    #[test]
    fn pieces_follow_spans() {
        let text = "Hello world";
        let o = Offsets::new(text);
        let spans = vec![Span {
            start: 6,
            end: 11,
            style: SpanStyle {
                bold: Some(true),
                ..SpanStyle::default()
            },
        }];
        let p = style_pieces(text, &o, &spans);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].0, 0..6);
        assert_eq!(p[1].1.bold, Some(true));
    }

    #[test]
    fn renders_text_with_caret_positions() {
        let mut e = engine();
        if e.families().is_empty() {
            return; // no system fonts (e.g. a bare CI machine)
        }
        let r = e.render(&spec("Hi\nthere"));
        assert!(r.width > 40 && r.height > 80, "{}x{}", r.width, r.height);
        assert!(ink(&r) > 100);
        assert_eq!(r.lines.len(), 2);
        assert_eq!(r.lines[1].start, 3);
        assert_eq!(r.lines[1].end, 8);
        let xs: Vec<f32> = r.lines[1].carets.iter().map(|c| c.1).collect();
        assert!(xs.windows(2).all(|w| w[0] <= w[1]));
        // Bigger text is wider.
        let big = e.render(&TextSpec {
            size: 80.0,
            ..spec("Hi\nthere")
        });
        assert!(big.width > r.width);
    }

    #[test]
    fn empty_text_has_one_line_and_a_caret() {
        let mut e = engine();
        let r = e.render(&spec(""));
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].carets, vec![(0, 0.0)]);
        assert!(r.height >= 40);
    }

    #[test]
    fn fixed_box_wraps_and_keeps_its_size() {
        let mut e = engine();
        if e.families().is_empty() {
            return;
        }
        let s = TextSpec {
            box_size: Some([120.0, 300.0]),
            ..spec("one two three four five")
        };
        let r = e.render(&s);
        assert_eq!((r.width, r.height), (120, 300));
        assert!(r.lines.len() >= 3);
    }

    #[test]
    fn spec_json_round_trip() {
        let json = r#"{"text":"ab","font":"Arial","size":20,"color":[1,2,3,255],
            "spans":[{"start":0,"end":1,"bold":true,"baseline":4}],"boxSize":[10,20]}"#;
        let s: TextSpec = serde_json::from_str(json).unwrap();
        assert_eq!(s.spans[0].style.bold, Some(true));
        assert_eq!(s.spans[0].style.baseline, Some(4.0));
        assert_eq!(s.box_size, Some([10.0, 20.0]));
        assert!(s.antialias);
    }
}
