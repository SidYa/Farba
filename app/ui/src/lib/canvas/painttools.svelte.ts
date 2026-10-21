// Painting tools on the canvas: Paintbrush, Pencil and Eraser (strokes), the retouching
// tools (Clone, Heal, Smudge, Blur / Sharpen, Dodge / Burn: strokes too), Bucket Fill
// (click) and Gradient (drag). The pixel work happens in the core; this turns pointer input
// into core calls and draws the brush outline / gradient line on the overlay canvas.
//
// GIMP behaviour: Shift+click with a brush tool draws a straight line from where the last
// stroke ended; Shift with Bucket Fill toggles "fill similar colours" / "fill whole
// selection"; Ctrl while dragging a gradient snaps its angle to 15° steps. Clone / Heal:
// Ctrl+click sets the source (the active layer, or the merged image with Sample merged);
// Blur / Sharpen and Dodge / Burn: Ctrl at the press uses the other mode. Paintbrush /
// Pencil: holding Alt picks colours instead (owner request 2026-10-14; GIMP uses Ctrl),
// with the Color Picker's options; the Viewport does the picking.
import type { Doc } from "../documents.svelte";
import { notify } from "../notify.svelte";
import { palette, type Rgba } from "../palette.svelte";
import { prefs } from "../prefs.svelte";
import { tools } from "../tools.svelte";

export const RETOUCH_TOOLS = ["clone", "heal", "smudge", "blur-sharpen", "dodge-burn"];
export const BRUSH_TOOLS = ["paintbrush", "pencil", "eraser", ...RETOUCH_TOOLS];
export const PAINT_TOOLS = [...BRUSH_TOOLS, "bucket-fill", "gradient"];
/** Tools where holding Alt turns the pointer into the Color Picker (all painting tools,
 *  owner request 2026-10-21; GIMP uses Ctrl). */
export const ALT_PICK_TOOLS = PAINT_TOOLS;

const CRITERIA = ["composite", "red", "green", "blue", "alpha", "hue", "saturation", "value"];

/** 0xRRGGBBAA for the core. */
function pack(c: Rgba): number {
  return ((c.r << 24) | (c.g << 16) | (c.b << 8) | Math.round(c.a * 255)) >>> 0;
}

interface GradientDrag {
  doc: Doc;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

class PaintTools {
  /** Document being painted on (pointer down .. up). */
  private painting: Doc | undefined;
  private gradient: GradientDrag | undefined;
  /** Where the last stroke ended (Shift+click continues from it). */
  private last: { doc: Doc; x: number; y: number } | undefined;
  /** Pointer in image coordinates (for the brush outline), undefined when outside. */
  private cursor: { x: number; y: number; shift: boolean } | undefined;
  /** Alt is held (painting tools pick colours: no brush outline). */
  alt = $state(false);
  /** Ctrl is held too (Ctrl+Alt+wheel sizes the brush: the outline stays). */
  ctrl = $state(false);
  /** Clone / Heal source (Ctrl+click): image point and the layer it was set on. */
  private source: { doc: Doc; x: number; y: number; layer: number } | undefined;
  /** "Aligned": the offset fixed by the first stroke after the source was set. */
  private aligned: { dx: number; dy: number } | undefined;
  /** The offset of the stroke in progress (for the source marker). */
  private strokeDelta: [number, number] | undefined;

  get busy(): boolean {
    return !!this.painting || !!this.gradient;
  }

  cancel() {
    this.gradient = undefined;
  }

  /** Left button down with a paint tool at image point `(ix, iy)`. */
  down(doc: Doc, tool: string, e: PointerEvent, ix: number, iy: number) {
    if (RETOUCH_TOOLS.includes(tool)) {
      this.retouch(doc, tool, e, ix, iy);
    } else if (BRUSH_TOOLS.includes(tool)) {
      const o = <T extends number | boolean>(k: string) => tools.option<T>(tool, k);
      const pencil = tool === "pencil";
      const eraser = tool === "eraser";
      const aliased = pencil || (eraser && o<boolean>("hardEdge"));
      doc.core.begin_stroke(
        o<number>("size"),
        aliased ? 1 : o<number>("hardness") / 100,
        o<number>("opacity") / 100,
        tool === "paintbrush" ? o<number>("spacing") / 100 : 0.1,
        pack(palette.primary),
        aliased,
        eraser,
        eraser ? "Eraser" : pencil ? "Pencil" : "Paintbrush",
      );
      this.painting = doc;
      if (e.shiftKey && this.last?.doc === doc) doc.core.stroke_to(this.last.x, this.last.y);
      this.strokeTo(ix, iy);
    } else if (tool === "bucket-fill") {
      const o = <T extends number | string | boolean>(k: string) => tools.option<T>(tool, k);
      const color = o<string>("fill") === "bg" ? palette.secondary : palette.primary;
      // Shift toggles the affected area (GIMP).
      const similar = (o<string>("area") === "similar") !== e.shiftKey;
      doc.run((c) =>
        c.bucket_fill(
          ix,
          iy,
          pack(color),
          o<number>("opacity") / 100,
          similar,
          o<number>("threshold"),
          Math.max(0, CRITERIA.indexOf(o<string>("criterion"))),
          o<boolean>("transparent"),
          o<boolean>("sampleMerged"),
          o<boolean>("antialias"),
        ),
      );
    } else if (tool === "gradient") {
      this.gradient = { doc, x0: ix, y0: iy, x1: ix, y1: iy };
    }
  }

  /** A Clone / Heal / Smudge / Blur / Sharpen / Dodge / Burn stroke (or Ctrl+click). */
  private retouch(doc: Doc, tool: string, e: PointerEvent, ix: number, iy: number) {
    const o = <T extends number | string | boolean>(k: string) => tools.option<T>(tool, k);
    const cloning = tool === "clone" || tool === "heal";
    let kind: number;
    let source = -1;
    let [dx, dy, fixed] = [0, 0, false];
    if (cloning) {
      if (e.ctrlKey) {
        this.source = { doc, x: ix, y: iy, layer: doc.active };
        this.aligned = undefined;
        notify("Source set");
        return;
      }
      const src = this.source?.doc === doc ? this.source : undefined;
      if (!src) {
        notify("Ctrl+click to set the source first");
        return;
      }
      kind = tool === "clone" ? 0 : 1;
      source = o<boolean>("sampleMerged") ? -1 : src.layer;
      switch (o<string>("alignment")) {
        case "aligned":
          this.aligned ??= { dx: src.x - ix, dy: src.y - iy };
          [dx, dy] = [this.aligned.dx, this.aligned.dy];
          break;
        case "registered":
          break;
        case "fixed":
          [dx, dy, fixed] = [src.x, src.y, true];
          break;
        default:
          [dx, dy] = [src.x - ix, src.y - iy];
      }
      this.strokeDelta = fixed ? undefined : [dx, dy];
    } else if (tool === "smudge") {
      kind = 2;
    } else if (tool === "blur-sharpen") {
      kind = (o<string>("type") === "sharpen") !== e.ctrlKey ? 4 : 3;
    } else {
      kind = (o<string>("type") === "burn") !== e.ctrlKey ? 6 : 5;
    }
    const rate = (tool === "dodge-burn" ? o<number>("exposure") : o<number>("rate")) / 100;
    const labels: Record<number, string> = {
      0: "Clone",
      1: "Heal",
      2: "Smudge",
      3: "Blur",
      4: "Sharpen",
      5: "Dodge",
      6: "Burn",
    };
    doc.core.begin_retouch(
      kind,
      o<number>("size"),
      o<number>("hardness") / 100,
      o<number>("opacity") / 100,
      0.1,
      rate,
      tool === "smudge" ? o<number>("flow") / 100 : 0,
      ["shadows", "midtones", "highlights"].indexOf(o<string>("range")),
      pack(palette.primary),
      source,
      dx,
      dy,
      fixed,
      labels[kind]!,
    );
    this.painting = doc;
    if (e.shiftKey && this.last?.doc === doc) doc.core.stroke_to(this.last.x, this.last.y);
    this.strokeTo(ix, iy);
  }

  private strokeTo(ix: number, iy: number) {
    const doc = this.painting!;
    doc.core.stroke_to(ix, iy);
    this.last = { doc, x: ix, y: iy };
  }

  /** Pointer moved to image point `(ix, iy)`; `events` are the coalesced points. */
  move(e: PointerEvent, points: [number, number][]) {
    const [ix, iy] = points[points.length - 1]!;
    this.cursor = { x: ix, y: iy, shift: e.shiftKey };
    this.ctrl = e.ctrlKey;
    // Also after Alt+Tab, whose key-up never reaches the page.
    this.alt = e.altKey;
    if (this.painting) {
      for (const [x, y] of points) this.strokeTo(x, y);
    } else if (this.gradient) {
      const g = this.gradient;
      let [x, y] = [ix, iy];
      if (e.ctrlKey) {
        const step = Math.PI / 12;
        const len = Math.hypot(x - g.x0, y - g.y0);
        const angle = Math.round(Math.atan2(y - g.y0, x - g.x0) / step) * step;
        x = g.x0 + Math.cos(angle) * len;
        y = g.y0 + Math.sin(angle) * len;
      }
      g.x1 = x;
      g.y1 = y;
    }
  }

  leave() {
    this.cursor = undefined;
  }

  up() {
    if (this.painting) {
      this.strokeDelta = undefined;
      this.painting.core.end_stroke();
      // Updates undo state and thumbnails.
      this.painting.sync();
      this.painting = undefined;
    } else if (this.gradient) {
      const g = this.gradient;
      this.gradient = undefined;
      if (g.x0 === g.x1 && g.y0 === g.y1) return;
      const o = <T extends number | string | boolean>(k: string) => tools.option<T>("gradient", k);
      const from = palette.primary;
      const to = o<string>("gradient") === "fg-transparent" ? { ...from, a: 0 } : palette.secondary;
      g.doc.run((c) =>
        c.gradient(
          g.x0,
          g.y0,
          g.x1,
          g.y1,
          pack(from),
          pack(to),
          o<string>("shape") === "radial" ? 1 : 0,
          ["none", "sawtooth", "triangular"].indexOf(o<string>("repeat")),
          o<boolean>("reverse"),
          o<number>("opacity") / 100,
        ),
      );
    }
  }

  /** Alt alone with a painting tool: the Color Picker, not the brush (no outline). */
  picksColor(tool: string): boolean {
    return this.alt && !this.ctrl && !this.painting && ALT_PICK_TOOLS.includes(tool);
  }

  /** The brush outline replaces the pointer (no crosshair inside the circle). */
  showsOutline(tool: string): boolean {
    return prefs.brushOutline && BRUSH_TOOLS.includes(tool) && !this.picksColor(tool);
  }

  /** Draws the brush outline, the Shift line or the gradient line (device pixels). */
  drawOverlay(ctx: CanvasRenderingContext2D, doc: Doc | undefined, tool: string) {
    if (!doc) return;
    const v = doc.view;
    const sx = (x: number) => x * v.zoom + v.x;
    const sy = (y: number) => y * v.zoom + v.y;
    const dpr = devicePixelRatio;
    if (this.gradient?.doc === doc) {
      const g = this.gradient;
      ctx.beginPath();
      ctx.moveTo(sx(g.x0), sy(g.y0));
      ctx.lineTo(sx(g.x1), sy(g.y1));
      stroke(ctx);
      for (const [x, y] of [
        [g.x0, g.y0],
        [g.x1, g.y1],
      ] as const) {
        ctx.beginPath();
        ctx.arc(sx(x), sy(y), 4 * dpr, 0, Math.PI * 2);
        stroke(ctx);
      }
      return;
    }
    const c = this.cursor;
    if (!c || !BRUSH_TOOLS.includes(tool)) return;
    if (this.picksColor(tool)) return;
    const r = (tools.option<number>(tool, "size") / 2) * v.zoom;
    // Clone / Heal: a cross where the source pixels come from.
    const src = this.source;
    if ((tool === "clone" || tool === "heal") && src?.doc === doc) {
      const alignment = tools.option<string>(tool, "alignment");
      let at: [number, number] = [src.x, src.y];
      if (this.strokeDelta) at = [c.x + this.strokeDelta[0], c.y + this.strokeDelta[1]];
      else if (alignment === "registered") at = [c.x, c.y];
      else if (alignment === "aligned" && this.aligned)
        at = [c.x + this.aligned.dx, c.y + this.aligned.dy];
      const [x, y] = [sx(at[0]), sy(at[1])];
      const k = 6 * dpr;
      ctx.beginPath();
      ctx.moveTo(x - k, y);
      ctx.lineTo(x + k, y);
      ctx.moveTo(x, y - k);
      ctx.lineTo(x, y + k);
      stroke(ctx, false);
    }
    ctx.beginPath();
    // Settings > Canvas > "Show the brush outline" (the Shift line is drawn anyway).
    if (prefs.brushOutline) ctx.arc(sx(c.x), sy(c.y), Math.max(r, 2 * dpr), 0, Math.PI * 2);
    if (c.shift && !this.painting && this.last?.doc === doc) {
      ctx.moveTo(sx(this.last.x), sy(this.last.y));
      ctx.lineTo(sx(c.x), sy(c.y));
    }
    stroke(ctx, false);
  }
}

/** A thin line visible on any background: dark outline with a light core (or dashed). */
function stroke(ctx: CanvasRenderingContext2D, dashed = true) {
  const dpr = devicePixelRatio;
  ctx.lineWidth = dpr;
  ctx.setLineDash([]);
  ctx.strokeStyle = dashed ? "#fff" : "rgba(0,0,0,0.6)";
  ctx.lineWidth = dashed ? dpr : 3 * dpr;
  ctx.stroke();
  ctx.lineWidth = dpr;
  if (dashed) ctx.setLineDash([4 * dpr, 4 * dpr]);
  ctx.strokeStyle = dashed ? "#000" : "rgba(255,255,255,0.9)";
  ctx.stroke();
}

export const paintTools = new PaintTools();
