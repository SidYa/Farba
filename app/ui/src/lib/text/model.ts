// The editable text model of a text layer (mirrors farba-text's TextSpec; the core stores it
// as JSON with the layer). Offsets are UTF-16 indices (JavaScript string indices). Spans
// carry per-fragment styles; they never overlap and are kept sorted and merged.

export interface SpanStyle {
  font?: string;
  size?: number;
  /** Straight RGBA, alpha 0..255. */
  color?: [number, number, number, number];
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  /** Raise (positive) or lower the fragment, pixels. */
  baseline?: number;
  /** Extra space after each character of the fragment, pixels (GIMP's kerning). */
  kerning?: number;
}

export interface Span extends SpanStyle {
  start: number;
  end: number;
}

export type Justify = "left" | "center" | "right" | "fill";

export interface TextSpec {
  text: string;
  font: string;
  size: number;
  color: [number, number, number, number];
  bold: boolean;
  italic: boolean;
  justify: Justify;
  indent: number;
  lineSpacing: number;
  letterSpacing: number;
  /** Fixed box [w, h]; null = dynamic (grows with the text). */
  boxSize: [number, number] | null;
  antialias: boolean;
  spans: Span[];
}

const STYLE_KEYS: (keyof SpanStyle)[] = [
  "font",
  "size",
  "color",
  "bold",
  "italic",
  "underline",
  "strike",
  "baseline",
  "kerning",
];

function styleOf(s: Span | SpanStyle): SpanStyle {
  const out: SpanStyle = {};
  for (const k of STYLE_KEYS) if (s[k] !== undefined) (out as Record<string, unknown>)[k] = s[k];
  return out;
}

function sameStyle(a: SpanStyle, b: SpanStyle): boolean {
  return JSON.stringify(styleOf(a)) === JSON.stringify(styleOf(b));
}

/** Sorts, drops empty / style-less spans and merges touching spans with the same style. */
function normalize(spans: Span[]): Span[] {
  const sorted = spans
    .filter((s) => s.end > s.start && Object.keys(styleOf(s)).length > 0)
    .sort((a, b) => a.start - b.start);
  const out: Span[] = [];
  for (const s of sorted) {
    const last = out[out.length - 1];
    if (last && last.end === s.start && sameStyle(last, s)) last.end = s.end;
    else out.push({ ...s });
  }
  return out;
}

/** The style of the character before `pos` (what typing there continues with). */
export function styleAt(spec: TextSpec, pos: number): SpanStyle {
  const at = Math.max(0, pos - 1);
  const s = spec.spans.find((x) => x.start <= at && x.end > at);
  return s ? styleOf(s) : {};
}

/** The style shared by the whole range (keys whose values differ are left out). */
export function styleOfRange(spec: TextSpec, start: number, end: number): SpanStyle {
  if (start >= end) return styleAt(spec, start);
  let common: SpanStyle | undefined;
  for (let i = start; i < end; i++) {
    const s = spec.spans.find((x) => x.start <= i && x.end > i);
    const st = s ? styleOf(s) : {};
    if (!common) common = st;
    else
      for (const k of STYLE_KEYS)
        if (JSON.stringify(common[k]) !== JSON.stringify(st[k])) delete common[k];
  }
  return common ?? {};
}

/**
 * Replaces `[start, end)` with `insert` (typed with `style`). Spans shift with the text; the
 * inserted text gets `style` exactly (its own spans).
 */
export function replaceText(
  spec: TextSpec,
  start: number,
  end: number,
  insert: string,
  style: SpanStyle,
): TextSpec {
  const removed = end - start;
  const delta = insert.length - removed;
  const spans: Span[] = [];
  for (const s of spec.spans) {
    // Parts before and after the replaced range survive; the after part shifts.
    if (s.start < start) spans.push({ ...s, end: Math.min(s.end, start) });
    if (s.end > end)
      spans.push({ ...s, start: Math.max(s.start, end) + delta, end: s.end + delta });
  }
  if (insert && Object.keys(styleOf(style)).length)
    spans.push({ ...styleOf(style), start, end: start + insert.length });
  return {
    ...spec,
    text: spec.text.slice(0, start) + insert + spec.text.slice(end),
    spans: normalize(spans),
  };
}

/**
 * Applies `patch` to the style of `[start, end)` (a key set to `undefined` removes it).
 */
export function applyStyle(spec: TextSpec, start: number, end: number, patch: SpanStyle): TextSpec {
  if (start >= end) return spec;
  // Split the range into pieces at every span boundary and restyle each.
  const cuts = new Set([start, end]);
  for (const s of spec.spans) {
    if (s.start > start && s.start < end) cuts.add(s.start);
    if (s.end > start && s.end < end) cuts.add(s.end);
  }
  const points = [...cuts].sort((a, b) => a - b);
  const spans: Span[] = [];
  for (const s of spec.spans) {
    if (s.start < start) spans.push({ ...s, end: Math.min(s.end, start) });
    if (s.end > end) spans.push({ ...s, start: Math.max(s.start, end) });
  }
  for (let i = 0; i + 1 < points.length; i++) {
    const [a, b] = [points[i]!, points[i + 1]!];
    const cur = spec.spans.find((s) => s.start <= a && s.end >= b);
    const merged: SpanStyle = { ...(cur ? styleOf(cur) : {}) };
    for (const k of Object.keys(patch) as (keyof SpanStyle)[]) {
      const v = patch[k];
      if (v === undefined) delete merged[k];
      else (merged as Record<string, unknown>)[k] = v;
    }
    spans.push({ ...merged, start: a, end: b });
  }
  return { ...spec, spans: normalize(spans) };
}

/** The layer name a text gives (GIMP: the text itself, shortened). */
export function nameFromText(text: string): string {
  const line = text.split("\n").find((l) => l.trim()) ?? "";
  const t = line.trim();
  return t.length > 30 ? `${t.slice(0, 30)}…` : t;
}

export function parseSpec(json: string | undefined, fallback: TextSpec): TextSpec {
  if (!json) return fallback;
  try {
    return { ...fallback, ...(JSON.parse(json) as Partial<TextSpec>) };
  } catch {
    return fallback;
  }
}
