// Transform tools (GIMP's Rotate, Scale, Flip, 3D Transform, Handle Transform).
//
// The first press with a transform tool starts a session: the core lifts the active layer
// (or its selected part) and the viewport draws it through the session's matrix on the GPU
// (perspective-correct). Dragging changes the tool's parameters; Enter or a double click
// applies (one undo step, rendered in the core from the original pixels), Escape cancels.
// Switching to another tool or image applies too. Flip applies at once on a click.
import { errorText, type Doc } from "../documents.svelte";
import { transformCommit, whileBusy } from "../native";
import { notify } from "../notify.svelte";
import { tools } from "../tools.svelte";
import { matrix_from_points } from "../wasm/core";

export const TRANSFORM_TOOLS = ["rotate", "scale", "flip", "transform-3d", "handle-transform"];

const INTERP = ["none", "linear", "cubic", "lanczos"];
const LABELS: Record<string, string> = {
  rotate: "Rotate",
  scale: "Scale",
  flip: "Flip",
  "transform-3d": "3D Transform",
  "handle-transform": "Handle Transform",
};
/** Device pixels within which a handle is hit. */
const HIT = 10;

type M = number[];
type Pt = [number, number];

const identity = (): M => [1, 0, 0, 0, 1, 0, 0, 0, 1];
const translate = (x: number, y: number): M => [1, 0, x, 0, 1, y, 0, 0, 1];
function mul(a: M, b: M): M {
  const r = new Array<number>(9).fill(0);
  for (let i = 0; i < 3; i++)
    for (let j = 0; j < 3; j++)
      for (let k = 0; k < 3; k++) r[i * 3 + j]! += a[i * 3 + k]! * b[k * 3 + j]!;
  return r;
}
function apply(m: M, x: number, y: number): Pt | undefined {
  const w = m[6]! * x + m[7]! * y + m[8]!;
  if (w <= 1e-9) return undefined;
  return [(m[0]! * x + m[1]! * y + m[2]!) / w, (m[3]! * x + m[4]! * y + m[5]!) / w];
}
function invert(m: M): M | undefined {
  const [a, b, c, d, e, f, g, h, i] = m as [
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  const c00 = e * i - f * h;
  const c01 = f * g - d * i;
  const c02 = d * h - e * g;
  const det = a * c00 + b * c01 + c * c02;
  if (Math.abs(det) < 1e-12) return undefined;
  const k = 1 / det;
  return [
    c00 * k,
    (c * h - b * i) * k,
    (b * f - c * e) * k,
    c01 * k,
    (a * i - c * g) * k,
    (c * d - a * f) * k,
    c02 * k,
    (b * g - a * h) * k,
    (a * e - b * d) * k,
  ];
}
const deg = (r: number) => (r * 180) / Math.PI;
const rad = (d: number) => (d * Math.PI) / 180;

/** Status bar text while transforming. */
export const transformStatus = $state<{ text: string }>({ text: "" });

let nextId = 1;

export interface Session {
  id: number;
  doc: Doc;
  tool: string;
  /** The layer being transformed. */
  layer: number;
  /** Undo label instead of the tool name. */
  label?: string;
  /** What is transformed, image coordinates: x, y, w, h. */
  rect: [number, number, number, number];
  /** The selected part was lifted (the layer stays visible without it). */
  lifted: boolean;
  /** Current transform, image -> image (row-major). */
  m: M;
  // Rotate
  angle: number;
  pivot: Pt;
  // Scale: target rectangle x0, y0, x1, y1
  box: [number, number, number, number];
  // 3D: rotation around X, Y, Z (degrees)
  rot: [number, number, number];
  // Handle Transform: handle positions before / after
  from: Pt[];
  to: Pt[];
}

type Drag =
  | { kind: "pivot" }
  | { kind: "rotate"; a0: number; angle0: number }
  | { kind: "edge"; ex: -1 | 0 | 1; ey: -1 | 0 | 1; box0: [number, number, number, number]; p0: Pt }
  | { kind: "move-box"; box0: [number, number, number, number]; p0: Pt }
  | { kind: "3d"; p0: Pt; rot0: [number, number, number]; s0: Pt }
  | { kind: "handle"; i: number; shift: boolean };

class TransformTools {
  session: Session | undefined;
  private drag: Drag | undefined;
  /** A commit is scheduled (see `apply`). */
  private applying = false;

  get busy(): boolean {
    return !!this.drag;
  }

  /** Starts (or continues) a session; also used by the Rotate / Zoom Layer dialog. */
  start(doc: Doc, tool: string): Session | undefined {
    if (this.applying) return undefined;
    if (this.session && (this.session.doc !== doc || this.session.tool !== tool)) this.apply();
    if (this.session) return this.session;
    let r: Int32Array;
    try {
      r = doc.core.transform_begin();
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e), "error");
      return undefined;
    }
    const [x, y, w, h] = [r[0]!, r[1]!, r[2]!, r[3]!];
    this.session = {
      id: nextId++,
      doc,
      tool,
      layer: doc.active,
      rect: [x, y, w, h],
      lifted: !!doc.core.transform_lifted(),
      m: identity(),
      angle: 0,
      pivot: [x + w / 2, y + h / 2],
      box: [x, y, x + w, y + h],
      rot: [0, 0, 0],
      from: [],
      to: [],
    };
    doc.sync();
    return this.session;
  }

  /** Applies the transform in progress (one undo step). */
  apply() {
    const s = this.session;
    if (!s || this.applying) return;
    this.drag = undefined;
    // Flip has no interpolation option: it is exact unless the layer is re-rendered from
    // an earlier transform's original, which then uses Cubic.
    const interp =
      s.tool === "flip"
        ? 2
        : Math.max(0, INTERP.indexOf(tools.option<string>(s.tool, "interpolation")));
    // Large layers are rendered by the backend on all cores, small ones by the WASM core;
    // the preview stays on screen meanwhile.
    this.applying = true;
    const m = Array.from(s.m);
    const label = s.label ?? LABELS[s.tool]!;
    void whileBusy("Applying the transform…", async () => {
      try {
        if (!(await transformCommit(s.doc, m, interp, label)))
          s.doc.run((c) => c.transform_commit(Float64Array.from(m), interp, label));
      } catch (e) {
        notify(errorText(e), "error");
        s.doc.core.transform_cancel();
        s.doc.sync();
      } finally {
        this.applying = false;
        if (this.session === s) this.forget();
      }
    });
  }

  cancel() {
    const s = this.session;
    if (!s) return;
    this.forget();
    s.doc.core.transform_cancel();
    s.doc.sync();
  }

  /** Drops the session without touching the core (it ended there: undo, another edit). */
  forget() {
    this.session = undefined;
    this.drag = undefined;
    transformStatus.text = "";
  }

  /** Left button down at image point `p` (`zoom` for hit distances). */
  down(doc: Doc, tool: string, e: PointerEvent, p: Pt) {
    if (tool === "flip") {
      const s = this.start(doc, tool);
      if (!s) return;
      const horizontal = (tools.option<string>("flip", "direction") === "horizontal") !== e.ctrlKey;
      const [x, y, w, h] = s.rect;
      const cx = x + w / 2;
      const cy = y + h / 2;
      s.m = horizontal
        ? mul(translate(cx, 0), mul([-1, 0, 0, 0, 1, 0, 0, 0, 1], translate(-cx, 0)))
        : mul(translate(0, cy), mul([1, 0, 0, 0, -1, 0, 0, 0, 1], translate(0, -cy)));
      this.apply();
      return;
    }
    const s = this.start(doc, tool);
    if (!s) return;
    const zoom = doc.view.zoom;
    const near = (a: Pt) => Math.hypot(a[0] - p[0], a[1] - p[1]) * zoom <= HIT * devicePixelRatio;
    if (tool === "rotate") {
      if (near(s.pivot)) this.drag = { kind: "pivot" };
      else
        this.drag = {
          kind: "rotate",
          a0: Math.atan2(p[1] - s.pivot[1], p[0] - s.pivot[0]),
          angle0: s.angle,
        };
    } else if (tool === "scale") {
      const [x0, y0, x1, y1] = s.box;
      const xs = [x0, (x0 + x1) / 2, x1];
      const ys = [y0, (y0 + y1) / 2, y1];
      for (let j = 0; j < 3; j++)
        for (let i = 0; i < 3; i++) {
          if ((i === 1 && j === 1) || !near([xs[i]!, ys[j]!])) continue;
          this.drag = {
            kind: "edge",
            ex: (i - 1) as -1 | 0 | 1,
            ey: (j - 1) as -1 | 0 | 1,
            box0: [...s.box],
            p0: p,
          };
          return;
        }
      this.drag = { kind: "move-box", box0: [...s.box], p0: p };
    } else if (tool === "transform-3d") {
      this.drag = { kind: "3d", p0: p, rot0: [...s.rot], s0: [e.clientX, e.clientY] };
    } else if (tool === "handle-transform") {
      const i = s.to.findIndex((h) => near(h));
      if (i >= 0 && e.ctrlKey) {
        s.from.splice(i, 1);
        s.to.splice(i, 1);
        this.update(s);
        return;
      }
      if (i >= 0) {
        this.drag = { kind: "handle", i, shift: e.shiftKey };
        return;
      }
      if (s.to.length >= 4) return;
      // A new handle where the pointer is; its original position is found through the
      // current transform, so adding it changes nothing.
      const inv = invert(s.m);
      const src = inv && apply(inv, p[0], p[1]);
      if (!src) return;
      s.from.push(src);
      s.to.push([...p]);
      this.drag = { kind: "handle", i: s.to.length - 1, shift: false };
    }
    this.update(s);
  }

  move(e: PointerEvent, p: Pt) {
    const s = this.session;
    const d = this.drag;
    if (!s || !d) return;
    switch (d.kind) {
      case "pivot":
        s.pivot = p;
        break;
      case "rotate": {
        let a = d.angle0 + Math.atan2(p[1] - s.pivot[1], p[0] - s.pivot[0]) - d.a0;
        if (e.ctrlKey) a = rad(Math.round(deg(a) / 15) * 15);
        s.angle = a;
        break;
      }
      case "move-box": {
        const [dx, dy] = [p[0] - d.p0[0], p[1] - d.p0[1]];
        const b = d.box0;
        s.box = [b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy];
        break;
      }
      case "edge": {
        const keep = tools.option<boolean>("scale", "keepAspect") !== e.shiftKey;
        const centre = e.ctrlKey;
        const [x0, y0, x1, y1] = d.box0;
        const [cx, cy] = [(x0 + x1) / 2, (y0 + y1) / 2];
        const [dx, dy] = [p[0] - d.p0[0], p[1] - d.p0[1]];
        let [nx0, ny0, nx1, ny1] = [x0, y0, x1, y1];
        if (d.ex === 1) nx1 += dx;
        if (d.ex === -1) nx0 += dx;
        if (d.ey === 1) ny1 += dy;
        if (d.ey === -1) ny0 += dy;
        if (centre) {
          if (d.ex) [nx0, nx1] = d.ex === 1 ? [2 * cx - nx1, nx1] : [nx0, 2 * cx - nx0];
          if (d.ey) [ny0, ny1] = d.ey === 1 ? [2 * cy - ny1, ny1] : [ny0, 2 * cy - ny0];
        }
        if (keep) {
          const ratio = (x1 - x0) / (y1 - y0);
          let w = nx1 - nx0;
          let h = ny1 - ny0;
          if (d.ex && d.ey) {
            // Corner: follow the larger change.
            if (Math.abs(w / (x1 - x0)) > Math.abs(h / (y1 - y0))) h = w / ratio;
            else w = h * ratio;
          } else if (d.ex) h = w / ratio;
          else w = h * ratio;
          // Grow from the anchored side (or the centre).
          const ax = centre || !d.ex ? (nx0 + nx1) / 2 : d.ex === 1 ? nx0 : nx1;
          const ay = centre || !d.ey ? (ny0 + ny1) / 2 : d.ey === 1 ? ny0 : ny1;
          nx0 = centre || !d.ex ? ax - w / 2 : d.ex === 1 ? ax : ax - w;
          nx1 = nx0 + w;
          ny0 = centre || !d.ey ? ay - h / 2 : d.ey === 1 ? ay : ay - h;
          ny1 = ny0 + h;
        }
        s.box = [nx0, ny0, nx1, ny1];
        break;
      }
      case "3d": {
        // Half a degree per screen pixel; Shift keeps the main direction only.
        let dx = (e.clientX - d.s0[0]) * 0.5;
        let dy = (e.clientY - d.s0[1]) * 0.5;
        if (e.shiftKey) {
          if (Math.abs(dx) > Math.abs(dy)) dy = 0;
          else dx = 0;
        }
        s.rot = e.ctrlKey
          ? [d.rot0[0], d.rot0[1], d.rot0[2] + dx]
          : [d.rot0[0] - dy, d.rot0[1] + dx, d.rot0[2]];
        break;
      }
      case "handle":
        if (d.shift) {
          // Move only the handle: its source point follows through the current transform.
          const inv = invert(s.m);
          const src = inv && apply(inv, p[0], p[1]);
          if (src) {
            s.from[d.i] = src;
            s.to[d.i] = [...p];
          }
        } else s.to[d.i] = [...p];
        break;
    }
    this.update(s);
  }

  up() {
    this.drag = undefined;
  }

  /** Enter applies, Escape cancels. Returns true when the key was used. */
  key(e: KeyboardEvent): boolean {
    if (!this.session || e.type !== "keydown") return false;
    if (e.key === "Enter") this.apply();
    else if (e.key === "Escape") this.cancel();
    else return false;
    return true;
  }

  dblclick() {
    this.apply();
  }

  /** Recomputes the matrix and the status text from the tool parameters. */
  private update(s: Session) {
    const [x, y, w, h] = s.rect;
    switch (s.tool) {
      case "rotate": {
        const [px, py] = s.pivot;
        const c = Math.cos(s.angle);
        const n = Math.sin(s.angle);
        s.m = mul(translate(px, py), mul([c, -n, 0, n, c, 0, 0, 0, 1], translate(-px, -py)));
        let a = deg(s.angle) % 360;
        if (a > 180) a -= 360;
        if (a < -180) a += 360;
        transformStatus.text = `Rotate ${a.toFixed(1)}°, centre ${px.toFixed(0)}, ${py.toFixed(0)}`;
        break;
      }
      case "scale": {
        const [x0, y0, x1, y1] = s.box;
        const sx = (x1 - x0) / w;
        const sy = (y1 - y0) / h;
        s.m = mul(translate(x0, y0), mul([sx, 0, 0, 0, sy, 0, 0, 0, 1], translate(-x, -y)));
        transformStatus.text = `Scale ${Math.round(Math.abs(x1 - x0))} × ${Math.round(
          Math.abs(y1 - y0),
        )} px (${Math.round(Math.abs(sx) * 100)}% × ${Math.round(Math.abs(sy) * 100)}%)`;
        break;
      }
      case "transform-3d": {
        const [ax, ay, az] = s.rot.map(rad) as [number, number, number];
        const [cx, cy] = [x + w / 2, y + h / 2];
        const fov = rad(tools.option<number>("transform-3d", "fov"));
        const d = Math.max(w, h) / 2 / Math.tan(fov / 2);
        // R = Rz * Ry * Rx; only its first two columns matter for points on the plane.
        const rx: M = [1, 0, 0, 0, Math.cos(ax), -Math.sin(ax), 0, Math.sin(ax), Math.cos(ax)];
        const ry: M = [Math.cos(ay), 0, Math.sin(ay), 0, 1, 0, -Math.sin(ay), 0, Math.cos(ay)];
        const rz: M = [Math.cos(az), -Math.sin(az), 0, Math.sin(az), Math.cos(az), 0, 0, 0, 1];
        const r = mul(rz, mul(ry, rx));
        // Perspective from a camera at distance d in front of the plane's centre.
        const p: M = [d * r[0]!, d * r[1]!, 0, d * r[3]!, d * r[4]!, 0, r[6]!, r[7]!, d];
        s.m = mul(translate(cx, cy), mul(p, translate(-cx, -cy)));
        transformStatus.text = `3D: X ${s.rot[0].toFixed(0)}°, Y ${s.rot[1].toFixed(
          0,
        )}°, Z ${s.rot[2].toFixed(0)}°`;
        break;
      }
      case "handle-transform": {
        const flat = (pts: Pt[]) => Float64Array.from(pts.flat());
        const m = matrix_from_points(flat(s.from), flat(s.to));
        if (m) s.m = Array.from(m);
        transformStatus.text = `Handle Transform: ${s.to.length} of 4 handles`;
        break;
      }
    }
  }

  /** The matrix the preview draws the lifted pixels with (their pixels -> image). */
  previewMatrix(s: Session): M {
    return mul(s.m, translate(s.rect[0], s.rect[1]));
  }

  /** Frame, centre and handles on the overlay (device pixels). */
  drawOverlay(ctx: CanvasRenderingContext2D, doc: Doc | undefined) {
    const s = this.session;
    if (!s || s.doc !== doc) return;
    const v = doc.view;
    const scr = (p: Pt | undefined): Pt | undefined =>
      p && [p[0] * v.zoom + v.x, p[1] * v.zoom + v.y];
    const [x, y, w, h] = s.rect;
    const corners = (
      [
        [x, y],
        [x + w, y],
        [x + w, y + h],
        [x, y + h],
      ] as Pt[]
    ).map((c) => scr(apply(s.m, c[0], c[1])));
    const dpr = devicePixelRatio;
    if (corners.every((c) => c)) {
      ctx.beginPath();
      corners.forEach((c, i) => (i ? ctx.lineTo(...c!) : ctx.moveTo(...c!)));
      ctx.closePath();
      line(ctx);
    }
    const square = (p: Pt) => {
      const r = 4 * dpr;
      ctx.fillStyle = "#fff";
      ctx.strokeStyle = "#000";
      ctx.lineWidth = dpr;
      ctx.setLineDash([]);
      ctx.fillRect(p[0] - r, p[1] - r, 2 * r, 2 * r);
      ctx.strokeRect(p[0] - r, p[1] - r, 2 * r, 2 * r);
    };
    const circle = (p: Pt, r: number, fill: string) => {
      ctx.beginPath();
      ctx.arc(p[0], p[1], r * dpr, 0, Math.PI * 2);
      ctx.fillStyle = fill;
      ctx.fill();
      ctx.strokeStyle = "#000";
      ctx.lineWidth = dpr;
      ctx.setLineDash([]);
      ctx.stroke();
    };
    if (s.tool === "rotate") {
      const p = scr(s.pivot)!;
      circle(p, 6, "rgba(255,255,255,0.7)");
      ctx.beginPath();
      ctx.moveTo(p[0] - 10 * dpr, p[1]);
      ctx.lineTo(p[0] + 10 * dpr, p[1]);
      ctx.moveTo(p[0], p[1] - 10 * dpr);
      ctx.lineTo(p[0], p[1] + 10 * dpr);
      ctx.stroke();
    } else if (s.tool === "scale") {
      const [x0, y0, x1, y1] = s.box;
      for (const px of [x0, (x0 + x1) / 2, x1])
        for (const py of [y0, (y0 + y1) / 2, y1])
          if (px !== (x0 + x1) / 2 || py !== (y0 + y1) / 2) square(scr([px, py])!);
    } else if (s.tool === "handle-transform") {
      s.to.forEach((p) => circle(scr(p)!, 6, "rgba(255,255,255,0.85)"));
    } else if (s.tool === "transform-3d") {
      const c = scr(apply(s.m, x + w / 2, y + h / 2));
      if (c) circle(c, 3, "#fff");
    }
  }
}

/** A dashed black-and-white line. */
function line(ctx: CanvasRenderingContext2D) {
  const dpr = devicePixelRatio;
  ctx.lineWidth = dpr;
  ctx.setLineDash([]);
  ctx.strokeStyle = "#fff";
  ctx.stroke();
  ctx.setLineDash([4 * dpr, 4 * dpr]);
  ctx.strokeStyle = "#000";
  ctx.stroke();
}

export const transformTools = new TransformTools();
