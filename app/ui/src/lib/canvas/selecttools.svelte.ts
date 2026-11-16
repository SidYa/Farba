// Selection tools on the canvas: Rectangle / Ellipse (drag), Free Select (click for polygon
// vertices, drag for freehand segments), Fuzzy Select and Select by Color (click). The
// pixel work happens in the core; this keeps the in-progress shape and draws its preview
// on the overlay canvas.
//
// GIMP behaviour: the modifiers held when the button goes down pick the mode (Shift = add,
// Ctrl = subtract, Shift+Ctrl = intersect), otherwise the tool's Mode option. Pressing
// Shift / Ctrl *during* a rectangle or ellipse drag fixes the aspect ratio to 1:1 / grows
// the shape from its centre. A click without dragging in Replace mode removes the
// selection. Free Select closes on a click on its first point, a double click or Enter;
// Backspace removes the last segment and Escape cancels.
//
// Handles: the rectangle or ellipse drawn last keeps eight handles (GIMP's editable
// rectangle). Dragging a handle resizes that shape, dragging inside it moves it; the
// selection is made again from what it was before the shape, with the shape's own mode, so
// this works for a shape that was added to (or subtracted from...) a combined selection
// too: only the last shape is editable, the earlier ones are part of the selection. A
// press outside the shape, or with Shift / Ctrl held (a mode), starts a new shape. Enter or
// a click inside ends the editing; so does any other change of the image (the handles go).
import type { Doc } from "../documents.svelte";
import { tools } from "../tools.svelte";
import type { View } from "./renderer";

export const SELECTION_TOOLS = [
  "rect-select",
  "ellipse-select",
  "free-select",
  "fuzzy-select",
  "by-color-select",
];

const MODES = ["replace", "add", "subtract", "intersect"];
const CRITERIA = ["composite", "red", "green", "blue", "alpha", "hue", "saturation", "value"];
/** Device pixels within which a click closes the free selection on its first point. */
const CLOSE_DIST = 8;
/** Device pixels within which a handle of the last shape is hit. */
const HIT = 10;
/** Undo labels of the core's shape selections (the step a handle drag replaces). */
const LABELS: Record<string, string> = {
  "rect-select": "Rectangle Select",
  "ellipse-select": "Ellipse Select",
};

type Rect = [number, number, number, number];
/** A handle of the last shape: the side(s) it moves, or "move" (inside the shape). */
type Handle = { ex: -1 | 0 | 1; ey: -1 | 0 | 1 } | "move";

/** Size of the shape being drawn (shown in the status bar), or undefined. */
export const selectionPreview = $state<{ size?: [number, number] }>({});

interface Modifiers {
  shiftKey: boolean;
  ctrlKey: boolean;
}

function modeFor(tool: string, e: Modifiers): number {
  if (e.shiftKey && e.ctrlKey) return 3;
  if (e.shiftKey) return 1;
  if (e.ctrlKey) return 2;
  return Math.max(0, MODES.indexOf(tools.option<string>(tool, "mode")));
}

function featherOf(tool: string): number {
  return tools.option<boolean>(tool, "feather") ? tools.option<number>(tool, "featherRadius") : 0;
}

interface Drag {
  doc: Doc;
  tool: string;
  mode: number;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  /** Shift / Ctrl held, pressed after the drag started (aspect 1:1 / from centre). */
  square: boolean;
  center: boolean;
  /** A handle of the last shape is dragged: the handle and the shape `[x, y, w, h]` before. */
  edit?: { handle: Handle; base: Rect };
}

/** The shape drawn last, while its handles are shown. */
interface Pending {
  doc: Doc;
  tool: string;
  mode: number;
  /** `[x, y, w, h]`, image pixels. */
  rect: Rect;
  /** `doc.revision` right after the shape was selected: any other change ends the editing. */
  revision: number;
}

interface Lasso {
  doc: Doc;
  mode: number;
  /** Image coordinates, flat `[x0, y0, x1, y1, ...]`. */
  points: number[];
  dragging: boolean;
  cursor?: [number, number];
}

class SelectTools {
  private drag: Drag | undefined;
  private lasso: Lasso | undefined;
  private pending: Pending | undefined;

  /** True while a shape is being drawn (the viewport keeps redrawing the overlay). */
  get busy(): boolean {
    return !!this.drag || !!this.lasso;
  }

  /** Drops the shape in progress and the handles of the last one (the selection stays). */
  cancel() {
    this.drag = undefined;
    this.lasso = undefined;
    this.pending = undefined;
    selectionPreview.size = undefined;
  }

  /** The last shape of `doc`, while it can still be edited with `tool`. */
  private editable(doc: Doc | undefined, tool: string): Pending | undefined {
    const p = this.pending;
    if (!p) return undefined;
    // Something else changed the image since: the shape is final.
    if (p.revision !== p.doc.revision || p.doc.undoLabel !== LABELS[p.tool]) {
      this.pending = undefined;
      return undefined;
    }
    return p.doc === doc && p.tool === tool ? p : undefined;
  }

  /** The handle of the last shape at image point `(ix, iy)`. */
  private handleAt(p: Pending, view: View, ix: number, iy: number): Handle | undefined {
    const [x, y, w, h] = p.rect;
    const hit = HIT * devicePixelRatio;
    const xs = [x, x + w / 2, x + w];
    const ys = [y, y + h / 2, y + h];
    // Corners first; the handles in the middle of a side exist only on a side long enough.
    const order = [0, 2, 1];
    for (const j of order)
      for (const i of order) {
        if (i === 1 && j === 1) continue;
        if ((i === 1 && !midHandles(w, view)) || (j === 1 && !midHandles(h, view))) continue;
        if (Math.abs(ix - xs[i]!) * view.zoom <= hit && Math.abs(iy - ys[j]!) * view.zoom <= hit)
          return { ex: (i - 1) as -1 | 0 | 1, ey: (j - 1) as -1 | 0 | 1 };
      }
    return ix > x && ix < x + w && iy > y && iy < y + h ? "move" : undefined;
  }

  /** The cursor over the last shape's handles ("" elsewhere: the tool's own cursor). */
  cursor(doc: Doc, tool: string, e: Modifiers, ix: number, iy: number): string {
    let h: Handle | undefined;
    if (this.drag) h = this.drag.edit?.handle;
    else if (!e.shiftKey && !e.ctrlKey) {
      const p = this.editable(doc, tool);
      h = p && this.handleAt(p, doc.view, ix, iy);
    }
    if (!h) return "";
    if (h === "move") return "move";
    if (h.ex && h.ey) return h.ex === h.ey ? "nwse-resize" : "nesw-resize";
    return h.ex ? "ew-resize" : "ns-resize";
  }

  /** Left button down with a selection tool. `ix, iy`: image point; `view` for distances. */
  down(doc: Doc, tool: string, e: PointerEvent, ix: number, iy: number) {
    if (tool === "rect-select" || tool === "ellipse-select") {
      const x = Math.round(ix);
      const y = Math.round(iy);
      // A handle of the last shape (no mode key held: those start a new shape).
      const last = e.shiftKey || e.ctrlKey ? undefined : this.editable(doc, tool);
      const handle = last && this.handleAt(last, doc.view, ix, iy);
      if (last && handle) {
        this.drag = {
          doc,
          tool,
          mode: last.mode,
          x0: x,
          y0: y,
          x1: x,
          y1: y,
          square: false,
          center: false,
          edit: { handle, base: [...last.rect] },
        };
        this.updatePreview();
        return;
      }
      this.pending = undefined;
      this.drag = {
        doc,
        tool,
        mode: modeFor(tool, e),
        x0: x,
        y0: y,
        x1: x,
        y1: y,
        square: false,
        center: false,
      };
      this.updatePreview();
    } else if (tool === "free-select") {
      if (this.lasso && this.lasso.doc !== doc) this.lasso = undefined;
      if (!this.lasso) {
        this.lasso = { doc, mode: modeFor(tool, e), points: [], dragging: false };
      } else if (this.nearFirst(doc.view, ix, iy)) {
        this.commitLasso();
        return;
      }
      this.lasso.points.push(ix, iy);
      this.lasso.dragging = true;
      this.lasso.cursor = [ix, iy];
    } else if (tool === "fuzzy-select" || tool === "by-color-select") {
      const o = <T extends string | number | boolean>(k: string) => tools.option<T>(tool, k);
      doc.run((c) =>
        c.select_color(
          ix,
          iy,
          o<number>("threshold"),
          Math.max(0, CRITERIA.indexOf(o<string>("criterion"))),
          o<boolean>("transparent"),
          o<boolean>("sampleMerged"),
          tool === "fuzzy-select",
          modeFor(tool, e),
          o<boolean>("antialias"),
          featherOf(tool),
        ),
      );
    }
  }

  move(e: PointerEvent, ix: number, iy: number) {
    if (this.drag) {
      this.drag.x1 = Math.round(ix);
      this.drag.y1 = Math.round(iy);
      this.updatePreview();
    } else if (this.lasso) {
      const l = this.lasso;
      l.cursor = [ix, iy];
      if (l.dragging && e.buttons & 1) {
        const n = l.points.length;
        const z = l.doc.view.zoom;
        // A freehand point every 2 screen pixels.
        if (Math.hypot(ix - l.points[n - 2]!, iy - l.points[n - 1]!) * z >= 2)
          l.points.push(ix, iy);
      }
    }
  }

  up() {
    if (this.drag) {
      const d = this.drag;
      this.drag = undefined;
      selectionPreview.size = undefined;
      const [x, y, w, h] = rectOf(d);
      if (d.edit) {
        const b = d.edit.base;
        if (x === b[0] && y === b[1] && w === b[2] && h === b[3]) {
          // A click inside the shape ends its editing (GIMP).
          if (d.edit.handle === "move" && d.x0 === d.x1 && d.y0 === d.y1) this.pending = undefined;
          return;
        }
        // Squeezed to nothing, or the image changed meanwhile: the shape stays as it was.
        if (w < 1 || h < 1 || !this.editable(d.doc, d.tool)) return;
      } else if (w < 1 || h < 1) {
        if (d.mode === 0) d.doc.run((c) => c.select_none());
        return;
      }
      const aa = tools.option<boolean>(d.tool, "antialias");
      const feather = featherOf(d.tool);
      const radius = d.tool === "rect-select" ? tools.option<number>(d.tool, "rounded") : 0;
      const edit = !!d.edit;
      const ok = d.doc.run((c) => {
        if (edit) {
          // The shape again, from the selection before it: its undo step is replaced. The
          // step also holds the active layer of that time; the one of now stays.
          const active = c.active;
          c.undo();
          c.set_active(active);
        }
        if (d.tool === "rect-select") c.select_rect(x, y, w, h, radius, d.mode, aa, feather);
        else c.select_ellipse(x, y, w, h, d.mode, aa, feather);
        return true;
      });
      // The shape keeps its handles (not when nothing is left selected: all subtracted...).
      this.pending =
        ok && d.doc.selection
          ? { doc: d.doc, tool: d.tool, mode: d.mode, rect: [x, y, w, h], revision: d.doc.revision }
          : undefined;
    } else if (this.lasso) {
      this.lasso.dragging = false;
    }
  }

  /** Double click: closes the free selection. */
  dblclick() {
    if (this.lasso) this.commitLasso();
  }

  /**
   * Keys while a shape is in progress. Returns true when the key was used (the caller then
   * stops it from reaching the global shortcuts).
   */
  key(e: KeyboardEvent): boolean {
    if (this.drag) {
      const down = e.type === "keydown";
      if (e.key === "Shift") this.drag.square = down;
      else if (e.key === "Control") this.drag.center = down;
      else if (e.key === "Escape" && down) {
        // The shape being drawn is dropped; the one being edited stays as it was.
        this.drag = undefined;
        selectionPreview.size = undefined;
      } else return false;
      this.updatePreview();
      return true;
    }
    if (!this.lasso) {
      // Enter ends the editing of the last shape (its handles go, the selection stays).
      const p = this.pending;
      if (e.type !== "keydown" || e.key !== "Enter" || !p || !this.editable(p.doc, p.tool))
        return false;
      this.pending = undefined;
      return true;
    }
    if (e.type !== "keydown") return false;
    if (e.key === "Enter") this.commitLasso();
    else if (e.key === "Escape") this.cancel();
    else if (e.key === "Backspace") {
      this.lasso.points.length -= 2;
      if (!this.lasso.points.length) this.cancel();
    } else return false;
    return true;
  }

  private nearFirst(view: View, ix: number, iy: number): boolean {
    const p = this.lasso!.points;
    if (p.length < 6) return false;
    return Math.hypot(ix - p[0]!, iy - p[1]!) * view.zoom <= CLOSE_DIST;
  }

  private commitLasso() {
    const l = this.lasso!;
    this.lasso = undefined;
    if (l.points.length < 6) return;
    const aa = tools.option<boolean>("free-select", "antialias");
    const feather = featherOf("free-select");
    l.doc.run((c) => c.select_polygon(Float64Array.from(l.points), l.mode, aa, feather));
  }

  private updatePreview() {
    if (!this.drag) return;
    const [, , w, h] = rectOf(this.drag);
    selectionPreview.size = [w, h];
  }

  /** Draws the shape in progress on the overlay (device pixels). */
  drawOverlay(ctx: CanvasRenderingContext2D, doc: Doc | undefined) {
    if (!doc) return;
    const v = doc.view;
    const sx = (x: number) => x * v.zoom + v.x;
    const sy = (y: number) => y * v.zoom + v.y;
    ctx.beginPath();
    if (this.drag?.doc === doc) {
      const [x, y, w, h] = rectOf(this.drag);
      if (this.drag.tool === "ellipse-select") {
        ctx.ellipse(
          sx(x + w / 2),
          sy(y + h / 2),
          (w / 2) * v.zoom,
          (h / 2) * v.zoom,
          0,
          0,
          Math.PI * 2,
        );
      } else {
        ctx.rect(sx(x) + 0.5, sy(y) + 0.5, w * v.zoom, h * v.zoom);
      }
    } else if (this.lasso?.doc === doc) {
      const p = this.lasso.points;
      ctx.moveTo(sx(p[0]!), sy(p[1]!));
      for (let i = 2; i < p.length; i += 2) ctx.lineTo(sx(p[i]!), sy(p[i + 1]!));
      const c = this.lasso.cursor;
      if (c && !this.lasso.dragging) ctx.lineTo(sx(c[0]), sy(c[1]));
    } else {
      this.drawHandles(ctx, doc);
      return;
    }
    stroke(ctx);

    // The first point of a free selection: a circle, filled when a click would close it.
    if (this.lasso?.doc === doc && this.lasso.points.length >= 6) {
      const p = this.lasso.points;
      const c = this.lasso.cursor;
      const near = !!c && this.nearFirst(v, c[0], c[1]);
      ctx.beginPath();
      ctx.arc(sx(p[0]!), sy(p[1]!), CLOSE_DIST * devicePixelRatio * 0.6, 0, Math.PI * 2);
      ctx.fillStyle = near ? "rgba(255,255,255,0.8)" : "rgba(255,255,255,0.25)";
      ctx.fill();
      stroke(ctx);
    }
    this.drawHandles(ctx, doc);
  }

  /** The handles of the last shape (and the frame of an ellipse), device pixels. */
  private drawHandles(ctx: CanvasRenderingContext2D, doc: Doc) {
    const p = this.editable(doc, tools.active);
    if (!p) return;
    const v = doc.view;
    const dpr = devicePixelRatio;
    // While a handle is dragged the handles follow the shape being drawn.
    const [x, y, w, h] = this.drag?.edit ? rectOf(this.drag) : p.rect;
    const sx = (a: number) => Math.round(a * v.zoom + v.x);
    const sy = (a: number) => Math.round(a * v.zoom + v.y);
    if (p.tool === "ellipse-select") {
      // The box the handles sit on (a rectangle's is its own outline).
      ctx.beginPath();
      ctx.rect(sx(x) + 0.5, sy(y) + 0.5, sx(x + w) - sx(x), sy(y + h) - sy(y));
      ctx.lineWidth = dpr;
      ctx.setLineDash([2 * dpr, 3 * dpr]);
      ctx.strokeStyle = "rgba(0,0,0,0.6)";
      ctx.stroke();
      ctx.lineDashOffset = 2.5 * dpr;
      ctx.strokeStyle = "rgba(255,255,255,0.8)";
      ctx.stroke();
      ctx.lineDashOffset = 0;
    }
    const r = 4 * dpr;
    ctx.setLineDash([]);
    ctx.lineWidth = dpr;
    ctx.fillStyle = "#fff";
    ctx.strokeStyle = "#000";
    const xs = [x, x + w / 2, x + w];
    const ys = [y, y + h / 2, y + h];
    for (let j = 0; j < 3; j++)
      for (let i = 0; i < 3; i++) {
        if (i === 1 && j === 1) continue;
        if ((i === 1 && !midHandles(w, v)) || (j === 1 && !midHandles(h, v))) continue;
        const [cx, cy] = [sx(xs[i]!) + 0.5, sy(ys[j]!) + 0.5];
        ctx.fillRect(cx - r, cy - r, 2 * r, 2 * r);
        ctx.strokeRect(cx - r, cy - r, 2 * r, 2 * r);
      }
  }
}

/** Whether a side of this many image pixels is long enough on screen for a middle handle. */
function midHandles(side: number, view: View): boolean {
  return side * view.zoom >= 4 * HIT * devicePixelRatio;
}

/** A dashed black-and-white line, visible on any background. */
function stroke(ctx: CanvasRenderingContext2D) {
  const dpr = devicePixelRatio;
  ctx.lineWidth = dpr;
  ctx.setLineDash([]);
  ctx.strokeStyle = "#fff";
  ctx.stroke();
  ctx.setLineDash([4 * dpr, 4 * dpr]);
  ctx.strokeStyle = "#000";
  ctx.stroke();
}

/** `[x, y, w, h]` of the last shape while one of its handles is dragged. */
function editedRect(d: Drag): Rect {
  const { handle, base } = d.edit!;
  const dx = d.x1 - d.x0;
  const dy = d.y1 - d.y0;
  if (handle === "move") return [base[0] + dx, base[1] + dy, base[2], base[3]];
  let [l, t, r, b] = [base[0], base[1], base[0] + base[2], base[1] + base[3]];
  const { ex, ey } = handle;
  if (ex === -1) l += dx;
  if (ex === 1) r += dx;
  if (ey === -1) t += dy;
  if (ey === 1) b += dy;
  if (d.center) {
    // Ctrl: the opposite side mirrors the dragged one.
    if (ex === 1) l = base[0] - dx;
    if (ex === -1) r = base[0] + base[2] - dx;
    if (ey === 1) t = base[1] - dy;
    if (ey === -1) b = base[1] + base[3] - dy;
  }
  if (d.square && ex && ey) {
    // Shift on a corner: 1:1, around the centre (Ctrl) or from the opposite corner.
    const s = Math.max(Math.abs(r - l), Math.abs(b - t));
    if (d.center) {
      const cx = base[0] + base[2] / 2;
      const cy = base[1] + base[3] / 2;
      [l, r, t, b] = [cx - s / 2, cx + s / 2, cy - s / 2, cy + s / 2];
    } else {
      if (ex === 1) r = l + Math.sign(r - l || 1) * s;
      else l = r - Math.sign(r - l || 1) * s;
      if (ey === 1) b = t + Math.sign(b - t || 1) * s;
      else t = b - Math.sign(b - t || 1) * s;
    }
  }
  const x = Math.round(Math.min(l, r));
  const y = Math.round(Math.min(t, b));
  return [x, y, Math.round(Math.max(l, r)) - x, Math.round(Math.max(t, b)) - y];
}

/** `[x, y, w, h]` of a rectangle / ellipse drag (image pixels). */
function rectOf(d: Drag): Rect {
  if (d.edit) return editedRect(d);
  let dx = d.x1 - d.x0;
  let dy = d.y1 - d.y0;
  if (d.square) {
    const s = Math.max(Math.abs(dx), Math.abs(dy));
    dx = Math.sign(dx || 1) * s;
    dy = Math.sign(dy || 1) * s;
  }
  if (d.center)
    return [d.x0 - Math.abs(dx), d.y0 - Math.abs(dy), 2 * Math.abs(dx), 2 * Math.abs(dy)];
  return [Math.min(d.x0, d.x0 + dx), Math.min(d.y0, d.y0 + dy), Math.abs(dx), Math.abs(dy)];
}

export const selectTools = new SelectTools();
