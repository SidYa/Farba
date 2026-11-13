// Crop tool. Choosing it puts a frame with eight handles on the canvas (around the
// selection, else around the whole image). Dragging a handle moves that side or corner,
// dragging inside moves the frame, dragging outside draws a new one; the part that would be
// cut away is dimmed at once, and dragging the handles back out brings it back. Nothing
// changes in the image until Enter or a double click crops it (one undo step); switching
// to another tool or image crops too (like the transform tools). Escape puts the frame back
// around the whole image.
import { documents, errorText, type Doc } from "../documents.svelte";
import { snap } from "../guides.svelte";
import { notify } from "../notify.svelte";
import { transformStatus } from "./transformtools.svelte";

export const CROP_TOOL = "crop";

/** Device pixels within which a handle is hit. */
const HIT = 10;

type Pt = [number, number];
/** x0, y0, x1, y1 in image pixels (integers, x0 < x1, y0 < y1). */
type Rect = [number, number, number, number];

interface Session {
  doc: Doc;
  /** The image size the frame was made for (an undo or resize starts a new frame). */
  w: number;
  h: number;
  rect: Rect;
}

type Drag =
  | { kind: "edge"; ex: -1 | 0 | 1; ey: -1 | 0 | 1; r0: Rect; p0: Pt }
  | { kind: "move"; r0: Rect; p0: Pt }
  | { kind: "new"; r0: Rect; p0: Pt };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

class CropTool {
  session: Session | undefined;
  private drag: Drag | undefined;

  get busy(): boolean {
    return !!this.drag;
  }

  /** The frame of `doc`, made when missing or when the image size changed. */
  frame(doc: Doc): Session {
    const s = this.session;
    if (s && s.doc === doc && s.w === doc.width && s.h === doc.height) return s;
    const sel = doc.selection;
    const rect: Rect = sel
      ? [sel[0], sel[1], sel[0] + sel[2], sel[1] + sel[3]]
      : [0, 0, doc.width, doc.height];
    this.session = { doc, w: doc.width, h: doc.height, rect: fitRect(rect, doc.width, doc.height) };
    this.drag = undefined;
    this.status();
    return this.session;
  }

  /** The frame differs from the whole image (applying would crop). */
  private changed(s: Session): boolean {
    const [x0, y0, x1, y1] = s.rect;
    return x0 !== 0 || y0 !== 0 || x1 !== s.w || y1 !== s.h;
  }

  /** Crops `doc` to the frame (one undo step), keeping the kept part where it was on screen. */
  apply() {
    const s = this.session;
    this.forget();
    if (!s || !documents.list.includes(s.doc) || !this.changed(s)) return;
    if (s.w !== s.doc.width || s.h !== s.doc.height) return;
    const [x0, y0, x1, y1] = s.rect;
    try {
      s.doc.run((c) => c.crop(x0, y0, x1 - x0, y1 - y0));
    } catch (e) {
      notify(errorText(e), "error");
      return;
    }
    const v = s.doc.view;
    s.doc.view = { ...v, x: Math.round(v.x + x0 * v.zoom), y: Math.round(v.y + y0 * v.zoom) };
  }

  /** Drops the frame without cropping. */
  forget() {
    this.session = undefined;
    this.drag = undefined;
    transformStatus.text = "";
  }

  /** The handle under image point `p`: [ex, ey] (-1 left / top, 1 right / bottom). */
  private handleAt(s: Session, p: Pt): [-1 | 0 | 1, -1 | 0 | 1] | undefined {
    const zoom = s.doc.view.zoom;
    const [x0, y0, x1, y1] = s.rect;
    const xs = [x0, (x0 + x1) / 2, x1];
    const ys = [y0, (y0 + y1) / 2, y1];
    const reach = (HIT * devicePixelRatio) / zoom;
    let best: [-1 | 0 | 1, -1 | 0 | 1] | undefined;
    let dist = Infinity;
    for (let j = 0; j < 3; j++)
      for (let i = 0; i < 3; i++) {
        if (i === 1 && j === 1) continue;
        const d = Math.hypot(xs[i]! - p[0], ys[j]! - p[1]);
        if (d <= reach && d < dist) {
          dist = d;
          best = [(i - 1) as -1 | 0 | 1, (j - 1) as -1 | 0 | 1];
        }
      }
    if (best) return best;
    // Anywhere along a side moves that side.
    const inY = p[1] >= y0 && p[1] <= y1;
    const inX = p[0] >= x0 && p[0] <= x1;
    if (inY && Math.abs(p[0] - x0) <= reach) return [-1, 0];
    if (inY && Math.abs(p[0] - x1) <= reach) return [1, 0];
    if (inX && Math.abs(p[1] - y0) <= reach) return [0, -1];
    if (inX && Math.abs(p[1] - y1) <= reach) return [0, 1];
    return undefined;
  }

  /** CSS cursor over image point `p`. */
  cursor(doc: Doc, p: Pt): string {
    if (this.drag?.kind === "move") return "move";
    const s = this.frame(doc);
    const h = this.drag?.kind === "edge" ? [this.drag.ex, this.drag.ey] : this.handleAt(s, p);
    if (h) {
      const [ex, ey] = h;
      if (ex && ey) return ex === ey ? "nwse-resize" : "nesw-resize";
      return ex ? "ew-resize" : "ns-resize";
    }
    const [x0, y0, x1, y1] = s.rect;
    return p[0] > x0 && p[0] < x1 && p[1] > y0 && p[1] < y1 ? "move" : "crosshair";
  }

  /** Left button down at image point `p`. */
  down(doc: Doc, p: Pt) {
    const s = this.frame(doc);
    const h = this.handleAt(s, p);
    const [x0, y0, x1, y1] = s.rect;
    const q = snap(doc, ...p);
    if (h) this.drag = { kind: "edge", ex: h[0], ey: h[1], r0: [...s.rect], p0: q };
    else if (p[0] > x0 && p[0] < x1 && p[1] > y0 && p[1] < y1)
      this.drag = { kind: "move", r0: [...s.rect], p0: p };
    else this.drag = { kind: "new", r0: [...s.rect], p0: q };
  }

  move(e: PointerEvent, p: Pt) {
    const s = this.session;
    const d = this.drag;
    if (!s || !d) return;
    const { w, h } = s;
    const q = snap(s.doc, ...p);
    switch (d.kind) {
      case "edge": {
        let [x0, y0, x1, y1] = d.r0;
        const dx = q[0] - d.p0[0];
        const dy = q[1] - d.p0[1];
        if (d.ex === -1) x0 = clamp(Math.round(x0 + dx), 0, x1 - 1);
        if (d.ex === 1) x1 = clamp(Math.round(x1 + dx), x0 + 1, w);
        if (d.ey === -1) y0 = clamp(Math.round(y0 + dy), 0, y1 - 1);
        if (d.ey === 1) y1 = clamp(Math.round(y1 + dy), y0 + 1, h);
        if (e.shiftKey && d.ex && d.ey) [x0, y0, x1, y1] = keepRatio(d, [x0, y0, x1, y1], w, h);
        s.rect = [x0, y0, x1, y1];
        break;
      }
      case "move": {
        const [x0, y0, x1, y1] = d.r0;
        const dx = clamp(Math.round(p[0] - d.p0[0]), -x0, w - x1);
        const dy = clamp(Math.round(p[1] - d.p0[1]), -y0, h - y1);
        s.rect = [x0 + dx, y0 + dy, x1 + dx, y1 + dy];
        break;
      }
      case "new": {
        const r = fitRect(
          [
            Math.round(Math.min(d.p0[0], q[0])),
            Math.round(Math.min(d.p0[1], q[1])),
            Math.round(Math.max(d.p0[0], q[0])),
            Math.round(Math.max(d.p0[1], q[1])),
          ],
          w,
          h,
        );
        // A click without a drag keeps the frame.
        s.rect = r[2] - r[0] >= 1 && r[3] - r[1] >= 1 ? r : d.r0;
        break;
      }
    }
    this.status();
  }

  up() {
    const s = this.session;
    const d = this.drag;
    this.drag = undefined;
    // A frame drawn too small to see (a click outside) gives the old one back.
    if (s && d?.kind === "new") {
      const [x0, y0, x1, y1] = s.rect;
      const px = 3 / s.doc.view.zoom;
      if (x1 - x0 < px && y1 - y0 < px) s.rect = d.r0;
      this.status();
    }
  }

  /** Enter crops, Escape puts the frame back around the whole image. */
  key(e: KeyboardEvent): boolean {
    const s = this.session;
    if (!s || e.type !== "keydown") return false;
    if (e.key === "Enter") this.apply();
    else if (e.key === "Escape" && this.changed(s)) {
      this.drag = undefined;
      s.rect = [0, 0, s.w, s.h];
      this.status();
    } else return false;
    return true;
  }

  dblclick() {
    this.apply();
  }

  private status() {
    const s = this.session;
    if (!s) return;
    const [x0, y0, x1, y1] = s.rect;
    transformStatus.text = `Crop ${x1 - x0} × ${y1 - y0} px at ${x0}, ${y0}${
      this.changed(s) ? " (Enter: crop, Escape: reset)" : ""
    }`;
  }

  /** The dimmed part that is cut away, the frame, its thirds and handles (device pixels).
   *  `shade`: the canvas background colour. */
  drawOverlay(ctx: CanvasRenderingContext2D, doc: Doc | undefined, shade: string) {
    if (!doc) return;
    const s = this.frame(doc);
    const v = doc.view;
    const dpr = devicePixelRatio;
    const sx = (x: number) => Math.round(x * v.zoom + v.x);
    const sy = (y: number) => Math.round(y * v.zoom + v.y);
    const [ix0, iy0, ix1, iy1] = [sx(0), sy(0), sx(s.w), sy(s.h)];
    const [x0, y0, x1, y1] = [sx(s.rect[0]), sy(s.rect[1]), sx(s.rect[2]), sy(s.rect[3])];
    ctx.save();
    // What is cut away: the image outside the frame, almost hidden.
    ctx.fillStyle = shade;
    ctx.globalAlpha = 0.78;
    ctx.beginPath();
    ctx.rect(ix0, iy0, ix1 - ix0, iy1 - iy0);
    ctx.rect(x0, y0, x1 - x0, y1 - y0);
    ctx.fill("evenodd");
    ctx.globalAlpha = 1;
    // Thirds while dragging.
    if (this.drag) {
      ctx.beginPath();
      for (const f of [1 / 3, 2 / 3]) {
        const x = Math.round(x0 + (x1 - x0) * f) + 0.5;
        const y = Math.round(y0 + (y1 - y0) * f) + 0.5;
        ctx.moveTo(x, y0);
        ctx.lineTo(x, y1);
        ctx.moveTo(x0, y);
        ctx.lineTo(x1, y);
      }
      ctx.lineWidth = dpr;
      ctx.strokeStyle = "rgba(255,255,255,0.45)";
      ctx.stroke();
    }
    // The frame: a dashed black-and-white line.
    ctx.beginPath();
    ctx.rect(x0 + 0.5, y0 + 0.5, x1 - x0 - 1, y1 - y0 - 1);
    ctx.lineWidth = dpr;
    ctx.strokeStyle = "#fff";
    ctx.stroke();
    ctx.setLineDash([4 * dpr, 4 * dpr]);
    ctx.strokeStyle = "#000";
    ctx.stroke();
    ctx.setLineDash([]);
    // Handles: corners and side middles.
    const r = 4 * dpr;
    ctx.fillStyle = "#fff";
    ctx.strokeStyle = "#000";
    for (const px of [x0, (x0 + x1) / 2, x1])
      for (const py of [y0, (y0 + y1) / 2, y1]) {
        if (px === (x0 + x1) / 2 && py === (y0 + y1) / 2) continue;
        ctx.fillRect(px - r, py - r, 2 * r, 2 * r);
        ctx.strokeRect(px - r, py - r, 2 * r, 2 * r);
      }
    ctx.restore();
  }
}

/** `r` limited to the image, at least 1 pixel. */
function fitRect(r: Rect, w: number, h: number): Rect {
  const x0 = clamp(r[0], 0, w - 1);
  const y0 = clamp(r[1], 0, h - 1);
  return [x0, y0, clamp(r[2], x0 + 1, w), clamp(r[3], y0 + 1, h)];
}

/** Shift on a corner: the frame keeps the shape it had when the drag began. */
function keepRatio(d: Extract<Drag, { kind: "edge" }>, r: Rect, w: number, h: number): Rect {
  const [ox0, oy0, ox1, oy1] = d.r0;
  const ratio = (ox1 - ox0) / (oy1 - oy0);
  let [x0, y0, x1, y1] = r;
  let cw = x1 - x0;
  let ch = y1 - y0;
  if (cw / ch > ratio) cw = Math.round(ch * ratio);
  else ch = Math.round(cw / ratio);
  // The opposite corner stays.
  if (d.ex === -1) x0 = x1 - cw;
  else x1 = x0 + cw;
  if (d.ey === -1) y0 = y1 - ch;
  else y1 = y0 + ch;
  return fitRect([x0, y0, x1, y1], w, h);
}

export const cropTool = new CropTool();
