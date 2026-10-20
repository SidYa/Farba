// Text tool (GIMP behaviour): a click creates a text box that grows with the text, a drag
// creates a fixed box; a click on a text layer continues editing it. Typing goes through a
// hidden textarea (so keyboard layouts and IME work); the text is laid out and drawn by the
// backend and shown on the layer at once. Everything typed until the editing ends is one
// undo step. Escape (or a click outside, another tool, another image) ends editing.
//
// Selection and caret follow the layout the backend returns (character boundaries per
// line). The floating style editor (TextStyleEditor.svelte) styles the selected fragment, or
// the text typed next when nothing is selected.
import type { Doc } from "../documents.svelte";
import { notify } from "../notify.svelte";
import { palette } from "../palette.svelte";
import { tools } from "../tools.svelte";
import {
  applyStyle,
  nameFromText,
  parseSpec,
  replaceText,
  styleAt,
  styleOfRange,
  type Justify,
  type SpanStyle,
  type TextSpec,
} from "../text/model";
import { renderText, type LineLayout } from "../text/render";

type Pt = [number, number];

export interface TextEdit {
  doc: Doc;
  layer: number;
  isNew: boolean;
  spec: TextSpec;
  /** Box position in image pixels. */
  x: number;
  y: number;
  /** Size of the last render (the box). */
  width: number;
  height: number;
  lines: LineLayout[];
  caret: number;
  anchor: number;
  /** Style for the next typed text (set by the style editor without a selection). */
  pending: SpanStyle | undefined;
}

const o = <T extends string | number | boolean>(k: string) => tools.option<T>("text", k);

/** The layer-wide part of a spec from the tool options. */
function optionsSpec(): Partial<TextSpec> {
  return {
    font: o<string>("font"),
    size: o<number>("fontSize"),
    justify: o<Justify>("justify"),
    indent: o<number>("indent"),
    lineSpacing: o<number>("lineSpacing"),
    letterSpacing: o<number>("letterSpacing"),
    antialias: o<boolean>("antialias"),
  };
}

function newSpec(): TextSpec {
  const c = palette.primary;
  return {
    text: "",
    font: "Segoe UI",
    size: 62,
    color: [c.r, c.g, c.b, Math.round(c.a * 255)],
    bold: false,
    italic: false,
    justify: "left",
    indent: 0,
    lineSpacing: 0,
    letterSpacing: 0,
    boxSize: null,
    antialias: true,
    spans: [],
    ...optionsSpec(),
  };
}

function lineOf(lines: LineLayout[], idx: number): number {
  const i = lines.findIndex((l) => idx >= l.start && idx <= l.end);
  if (i >= 0) return i;
  return idx <= 0 ? 0 : lines.length - 1;
}

function caretX(line: LineLayout, idx: number): number {
  let x = line.carets[0]?.[1] ?? 0;
  for (const [i, cx] of line.carets) {
    if (i > idx) break;
    x = cx;
  }
  return x;
}

function nearestIndex(line: LineLayout, x: number): number {
  let best = line.carets[0]?.[0] ?? line.start;
  let d = Infinity;
  for (const [i, cx] of line.carets) {
    const dd = Math.abs(cx - x);
    if (dd < d) [best, d] = [i, dd];
  }
  return best;
}

/** Word boundary search (Ctrl+arrows, Ctrl+Backspace). */
function wordEdge(text: string, from: number, dir: 1 | -1): number {
  let i = from;
  const isWord = (c: string | undefined) => !!c && /[\p{L}\p{N}_]/u.test(c);
  if (dir < 0) {
    while (i > 0 && !isWord(text[i - 1])) i--;
    while (i > 0 && isWord(text[i - 1])) i--;
  } else {
    while (i < text.length && !isWord(text[i])) i++;
    while (i < text.length && isWord(text[i])) i++;
  }
  return i;
}

class TextTool {
  edit = $state.raw<TextEdit | undefined>();
  /** The box being dragged for a new fixed-size text (image pixels). */
  private boxDrag: { start: Pt; end: Pt } | undefined;
  private selecting = false;
  private input: HTMLTextAreaElement | undefined;
  private rendering = false;
  private dirty = false;
  private finishWhenDone = false;
  /** Called after every render (the viewport redraws). */
  onchange: () => void = () => {};

  get busy(): boolean {
    return this.selecting || !!this.boxDrag;
  }

  attach(el: HTMLTextAreaElement) {
    this.input = el;
  }

  focus() {
    this.input?.focus({ preventScroll: true });
  }

  get selection(): [number, number] {
    const e = this.edit;
    if (!e) return [0, 0];
    return [Math.min(e.caret, e.anchor), Math.max(e.caret, e.anchor)];
  }

  /** The style shown in the style editor: of the selection, else of the caret. */
  get currentStyle(): SpanStyle {
    const e = this.edit;
    if (!e) return {};
    const [a, b] = this.selection;
    if (a === b && e.pending) return { ...styleAt(e.spec, a), ...e.pending };
    return styleOfRange(e.spec, a, b);
  }

  // ---- starting and ending ----

  private begin(doc: Doc, layer: number | undefined, at: Pt) {
    let id: number;
    try {
      id = doc.core.text_begin(layer, Math.round(at[0]), Math.round(at[1]));
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), "error");
      return;
    }
    const l = doc.core.layers().find((x: { id: number }) => x.id === id) as
      { x: number; y: number; width: number; height: number } | undefined;
    const spec = layer === undefined ? newSpec() : parseSpec(doc.core.layer_text(id), newSpec());
    if (layer !== undefined) this.showOptions(spec);
    this.edit = {
      doc,
      layer: id,
      isNew: layer === undefined,
      spec,
      x: l?.x ?? Math.round(at[0]),
      y: l?.y ?? Math.round(at[1]),
      width: l?.width ?? 1,
      height: l?.height ?? 1,
      lines: [],
      caret: spec.text.length,
      anchor: spec.text.length,
      pending: undefined,
    };
    doc.sync();
    this.render();
    this.focus();
  }

  /** Ends editing: records the text as one undo step (an empty new text is dropped). */
  finish() {
    const e = this.edit;
    if (!e) return;
    if (this.rendering || this.dirty) {
      // Wait for the last change to reach the layer.
      this.finishWhenDone = true;
      return;
    }
    this.finishWhenDone = false;
    this.edit = undefined;
    this.boxDrag = undefined;
    this.selecting = false;
    if (e.doc.core.text_editing !== e.layer) return;
    if (e.isNew && !e.spec.text) e.doc.core.text_cancel();
    else e.doc.core.text_commit(e.isNew ? "Add Text Layer" : "Edit Text");
    e.doc.sync();
    this.onchange();
  }

  /** The core ended the editing (undo, another change): forget it. */
  check() {
    const e = this.edit;
    if (e && e.doc.core.text_editing !== e.layer && !this.rendering) {
      this.edit = undefined;
      this.boxDrag = undefined;
      this.selecting = false;
    }
  }

  // ---- rendering ----

  private set(patch: Partial<TextEdit>, rerender = true) {
    if (!this.edit) return;
    this.edit = { ...this.edit, ...patch };
    if (rerender) this.render();
    else this.onchange();
  }

  private async render() {
    if (this.rendering) {
      this.dirty = true;
      return;
    }
    this.rendering = true;
    try {
      do {
        this.dirty = false;
        const e = this.edit;
        if (!e) break;
        const spec = e.spec;
        const r = await renderText(spec);
        try {
          if (this.edit?.layer === e.layer && e.doc.core.text_editing === e.layer) {
            e.doc.core.text_update(
              JSON.stringify(spec),
              r.width,
              r.height,
              r.bytes,
              this.edit.x,
              this.edit.y,
              nameFromText(spec.text),
            );
            this.edit = { ...this.edit, width: r.width, height: r.height, lines: r.lines };
            e.doc.sync();
          }
        } finally {
          r.release();
        }
      } while (this.dirty);
    } catch (err) {
      notify(`Text: ${err instanceof Error ? err.message : String(err)}`, "error");
    } finally {
      this.rendering = false;
      this.onchange();
      if (this.finishWhenDone) this.finish();
    }
  }

  // ---- pointer ----

  private inside(p: Pt): boolean {
    const e = this.edit;
    if (!e) return false;
    const m = 4;
    return (
      p[0] >= e.x - m && p[1] >= e.y - m && p[0] <= e.x + e.width + m && p[1] <= e.y + e.height + m
    );
  }

  private hit(p: Pt): number {
    const e = this.edit!;
    const [lx, ly] = [p[0] - e.x, p[1] - e.y];
    if (!e.lines.length) return e.caret;
    let line = e.lines.find((l) => ly >= l.top && ly < l.top + l.height);
    if (!line) line = ly < 0 ? e.lines[0]! : e.lines[e.lines.length - 1]!;
    return nearestIndex(line, lx);
  }

  down(doc: Doc, e: PointerEvent, p: Pt) {
    const ed = this.edit;
    if (ed && ed.doc === doc && this.inside(p)) {
      const i = this.hit(p);
      this.set({ caret: i, anchor: e.shiftKey ? ed.anchor : i, pending: undefined }, false);
      this.selecting = true;
      this.focus();
      return;
    }
    if (ed) this.finish();
    // A click on a text layer edits it (the topmost visible one under the pointer).
    const target = [...doc.layers]
      .reverse()
      .find(
        (l) =>
          l.visible &&
          l.isText &&
          p[0] >= l.x &&
          p[1] >= l.y &&
          p[0] < l.x + l.width &&
          p[1] < l.y + l.height,
      );
    if (target) {
      doc.select(target.id);
      this.begin(doc, target.id, p);
      const i = this.edit ? this.hit(p) : 0;
      this.set({ caret: i, anchor: i }, false);
      return;
    }
    this.boxDrag = { start: p, end: p };
  }

  move(e: PointerEvent, p: Pt) {
    if (this.selecting && this.edit && e.buttons & 1) {
      this.set({ caret: this.hit(p) }, false);
    } else if (this.boxDrag) {
      this.boxDrag.end = p;
      this.onchange();
    }
  }

  up(doc: Doc) {
    this.selecting = false;
    const b = this.boxDrag;
    if (!b) {
      this.focus();
      return;
    }
    this.boxDrag = undefined;
    const [x0, y0] = [Math.min(b.start[0], b.end[0]), Math.min(b.start[1], b.end[1])];
    const [w, h] = [Math.abs(b.end[0] - b.start[0]), Math.abs(b.end[1] - b.start[1])];
    const fixed = w * doc.view.zoom > 6 && h * doc.view.zoom > 6;
    this.begin(doc, undefined, fixed ? [x0, y0] : b.start);
    if (fixed && this.edit)
      this.set({
        spec: { ...this.edit.spec, boxSize: [Math.round(w), Math.round(h)] },
      });
    tools.setOption("text", "box", fixed ? "fixed" : "dynamic");
  }

  // ---- editing ----

  private replace(insert: string) {
    const e = this.edit;
    if (!e) return;
    const [a, b] = this.selection;
    const style = e.pending ? { ...styleAt(e.spec, a), ...e.pending } : styleAt(e.spec, a);
    const spec = replaceText(e.spec, a, b, insert, style);
    const at = a + insert.length;
    this.set({ spec, caret: at, anchor: at });
  }

  /** Text typed or pasted (from the hidden textarea). */
  insert(text: string) {
    if (!text) return;
    this.replace(text.replace(/\r\n?/g, "\n"));
  }

  selectedText(): string {
    const e = this.edit;
    if (!e) return "";
    const [a, b] = this.selection;
    return e.spec.text.slice(a, b);
  }

  deleteSelection() {
    this.replace("");
  }

  /** Keys while editing; returns true when used. */
  key(ev: KeyboardEvent): boolean {
    const e = this.edit;
    if (!e) return false;
    const text = e.spec.text;
    const [a, b] = this.selection;
    const ctrl = ev.ctrlKey;
    const move = (to: number) => {
      const c = Math.max(0, Math.min(text.length, to));
      this.set({ caret: c, anchor: ev.shiftKey ? e.anchor : c, pending: undefined }, false);
    };
    const line = () => e.lines[lineOf(e.lines, e.caret)];
    switch (ev.key) {
      case "Escape":
        this.finish();
        return true;
      case "ArrowLeft":
        move(!ev.shiftKey && a !== b ? a : ctrl ? wordEdge(text, e.caret, -1) : e.caret - 1);
        return true;
      case "ArrowRight":
        move(!ev.shiftKey && a !== b ? b : ctrl ? wordEdge(text, e.caret, 1) : e.caret + 1);
        return true;
      case "ArrowUp":
      case "ArrowDown": {
        const li = lineOf(e.lines, e.caret);
        const target = e.lines[li + (ev.key === "ArrowUp" ? -1 : 1)];
        const cur = e.lines[li];
        if (!target || !cur) move(ev.key === "ArrowUp" ? 0 : text.length);
        else move(nearestIndex(target, caretX(cur, e.caret)));
        return true;
      }
      case "Home":
        move(ctrl ? 0 : (line()?.start ?? 0));
        return true;
      case "End":
        move(ctrl ? text.length : (line()?.end ?? text.length));
        return true;
      case "Backspace":
        if (a === b) this.set({ anchor: ctrl ? wordEdge(text, a, -1) : Math.max(0, a - 1) }, false);
        this.replace("");
        return true;
      case "Delete":
        if (a === b)
          this.set({ anchor: ctrl ? wordEdge(text, a, 1) : Math.min(text.length, a + 1) }, false);
        this.replace("");
        return true;
      case "Enter":
        this.replace("\n");
        return true;
      case "Tab":
        this.replace("\t");
        return true;
    }
    if (ctrl && !ev.altKey) {
      switch (ev.code) {
        case "KeyA":
          this.set({ anchor: 0, caret: text.length }, false);
          return true;
        case "KeyB":
          this.toggle("bold");
          return true;
        case "KeyI":
          this.toggle("italic");
          return true;
        case "KeyU":
          this.toggle("underline");
          return true;
        case "KeyZ":
        case "KeyY": {
          // Undo / redo work on whole edits: end this one first.
          const doc = e.doc;
          this.finish();
          if (ev.code === "KeyZ" && !ev.shiftKey) doc.undo();
          else doc.redo();
          return true;
        }
      }
    }
    return false;
  }

  private toggle(k: "bold" | "italic" | "underline" | "strike") {
    const s = this.currentStyle;
    const e = this.edit;
    const on = !(s[k] ?? (k === "bold" ? e?.spec.bold : k === "italic" ? e?.spec.italic : false));
    this.style({ [k]: on });
  }

  /** Styles the selection, or the text typed next when nothing is selected. */
  style(patch: SpanStyle) {
    const e = this.edit;
    if (!e) return;
    const [a, b] = this.selection;
    if (a === b) this.set({ pending: { ...e.pending, ...patch } }, false);
    else this.set({ spec: applyStyle(e.spec, a, b, patch) });
  }

  /** Removes the fragment styles of the selection (back to the layer's style). */
  clearStyle() {
    const e = this.edit;
    if (!e) return;
    const [a, b] = this.selection;
    if (a === b) {
      this.set({ pending: undefined }, false);
      return;
    }
    const keys = [
      "font",
      "size",
      "color",
      "bold",
      "italic",
      "underline",
      "strike",
      "baseline",
      "kerning",
    ] as const;
    this.set({
      spec: applyStyle(e.spec, a, b, Object.fromEntries(keys.map((k) => [k, undefined]))),
    });
  }

  /** The tool options changed: they apply to the whole text being edited (GIMP). */
  optionsChanged() {
    const e = this.edit;
    if (!e) return;
    const fixed = o<string>("box") === "fixed";
    const boxSize: [number, number] | null = fixed ? (e.spec.boxSize ?? [e.width, e.height]) : null;
    const spec = { ...e.spec, ...optionsSpec(), boxSize };
    if (JSON.stringify(spec) !== JSON.stringify(e.spec)) this.set({ spec });
  }

  /** Shows a text layer's settings in the tool options (GIMP does the same). */
  private showOptions(spec: TextSpec) {
    const set = (k: string, v: string | number | boolean) => {
      if (tools.option("text", k) !== v) tools.setOption("text", k, v);
    };
    set("font", spec.font);
    set("fontSize", spec.size);
    set("justify", spec.justify);
    set("indent", spec.indent);
    set("lineSpacing", spec.lineSpacing);
    set("letterSpacing", spec.letterSpacing);
    set("antialias", spec.antialias);
    set("box", spec.boxSize ? "fixed" : "dynamic");
  }

  /** Moves the text box to image point `(x, y)` (its top-left corner). */
  moveBox(x: number, y: number) {
    const e = this.edit;
    if (!e) return;
    const [nx, ny] = [Math.round(x), Math.round(y)];
    if (nx !== e.x || ny !== e.y) this.set({ x: nx, y: ny });
  }

  /** Switches the box between dynamic and fixed (keeping the current size). */
  setFixedBox(fixed: boolean) {
    const e = this.edit;
    if (!e) return;
    this.set({ spec: { ...e.spec, boxSize: fixed ? [e.width, e.height] : null } });
  }

  // ---- drawing ----

  /** Screen position (device pixels) of the caret's top, for the hidden textarea / IME. */
  caretScreen(): Pt | undefined {
    const e = this.edit;
    if (!e) return undefined;
    const line = e.lines[lineOf(e.lines, e.caret)];
    if (!line) return undefined;
    const v = e.doc.view;
    return [
      (e.x + caretX(line, e.caret)) * v.zoom + v.x,
      (e.y + line.top + line.height) * v.zoom + v.y,
    ];
  }

  drawOverlay(ctx: CanvasRenderingContext2D, doc: Doc | undefined, blinkOn: boolean) {
    if (!doc) return;
    const v = doc.view;
    const dpr = devicePixelRatio;
    const sx = (x: number) => x * v.zoom + v.x;
    const sy = (y: number) => y * v.zoom + v.y;
    const frame = (x: number, y: number, w: number, h: number) => {
      ctx.beginPath();
      ctx.rect(Math.round(sx(x)) + 0.5, Math.round(sy(y)) + 0.5, w * v.zoom, h * v.zoom);
      ctx.lineWidth = dpr;
      ctx.setLineDash([]);
      ctx.strokeStyle = "#fff";
      ctx.stroke();
      ctx.setLineDash([4 * dpr, 4 * dpr]);
      ctx.strokeStyle = "#000";
      ctx.stroke();
      ctx.setLineDash([]);
    };
    if (this.boxDrag) {
      const b = this.boxDrag;
      frame(
        Math.min(b.start[0], b.end[0]),
        Math.min(b.start[1], b.end[1]),
        Math.abs(b.end[0] - b.start[0]),
        Math.abs(b.end[1] - b.start[1]),
      );
    }
    const e = this.edit;
    if (!e || e.doc !== doc) return;
    frame(e.x, e.y, e.width, e.height);
    const [a, b] = this.selection;
    if (a !== b) {
      ctx.fillStyle = "rgba(80, 140, 255, 0.35)";
      for (const l of e.lines) {
        const s = Math.max(a, l.start);
        const t = Math.min(b, l.end);
        if (s > t || (s === t && !(l.start === l.end && a <= l.start && b > l.start))) continue;
        const x0 = caretX(l, s);
        const x1 = s === t ? x0 + l.height * 0.3 : caretX(l, t);
        ctx.fillRect(sx(e.x + x0), sy(e.y + l.top), (x1 - x0) * v.zoom, l.height * v.zoom);
      }
    } else if (blinkOn) {
      const l = e.lines[lineOf(e.lines, e.caret)];
      if (l) {
        const x = Math.round(sx(e.x + caretX(l, e.caret)));
        ctx.fillStyle = "#000";
        ctx.fillRect(x - dpr, sy(e.y + l.top), 3 * dpr, l.height * v.zoom);
        ctx.fillStyle = "#fff";
        ctx.fillRect(x, sy(e.y + l.top), dpr, l.height * v.zoom);
      }
    }
  }
}

export const textTool = new TextTool();
