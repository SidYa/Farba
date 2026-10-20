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

  /** True while a shape is being drawn (the viewport keeps redrawing the overlay). */
  get busy(): boolean {
    return !!this.drag || !!this.lasso;
  }

  cancel() {
    this.drag = undefined;
    this.lasso = undefined;
    selectionPreview.size = undefined;
  }

  /** Left button down with a selection tool. `ix, iy`: image point; `view` for distances. */
  down(doc: Doc, tool: string, e: PointerEvent, ix: number, iy: number) {
    if (tool === "rect-select" || tool === "ellipse-select") {
      const x = Math.round(ix);
      const y = Math.round(iy);
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
      if (w < 1 || h < 1) {
        if (d.mode === 0) d.doc.run((c) => c.select_none());
        return;
      }
      const aa = tools.option<boolean>(d.tool, "antialias");
      const feather = featherOf(d.tool);
      if (d.tool === "rect-select") {
        const radius = tools.option<number>(d.tool, "rounded");
        d.doc.run((c) => c.select_rect(x, y, w, h, radius, d.mode, aa, feather));
      } else {
        d.doc.run((c) => c.select_ellipse(x, y, w, h, d.mode, aa, feather));
      }
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
      else if (e.key === "Escape" && down) this.cancel();
      else return false;
      this.updatePreview();
      return true;
    }
    if (!this.lasso || e.type !== "keydown") return false;
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
    } else return;
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
  }
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

/** `[x, y, w, h]` of a rectangle / ellipse drag (image pixels). */
function rectOf(d: Drag): [number, number, number, number] {
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
