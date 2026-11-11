// The Color Picker loupe (owner request 2026-11-11): a round lens centred on the pointer
// with the image pixels around it magnified, the pixel under the pointer framed in the
// middle. Its ring shows the colour under the pointer (upper half) next to the colour it
// would replace (lower half); it is also shown with Ctrl / Alt held in a painting tool. It
// samples what is seen: the visible layers composited, whatever layer is active. Drawn on
// the Viewport's overlay canvas (no animation: the owner found the pulsing outline too much).
import type { Doc } from "../documents.svelte";
import type { Rgba } from "../palette.svelte";

/** Lens radius and ring width, CSS pixels. */
const RADIUS = 50;
const RING = 7;
/** Pixels across the lens (odd: the picked one is in the middle). */
const CELLS = 13;

type Pixel = [number, number, number, number] | undefined;

/** The magnified pixels, kept while the pointer stays on the same image pixel. */
let cache: { key: string; pixels: Pixel[] } | undefined;

function pixels(doc: Doc, px: number, py: number): Pixel[] {
  const key = `${doc.id}:${doc.revision}:${px}:${py}`;
  if (cache?.key === key) return cache.pixels;
  const half = (CELLS - 1) / 2;
  const out: Pixel[] = [];
  for (let y = py - half; y <= py + half; y++) {
    for (let x = px - half; x <= px + half; x++) {
      const p = doc.core.sample(x, y, 1, true);
      out.push(p ? [p[0]!, p[1]!, p[2]!, p[3]!] : undefined);
    }
  }
  cache = { key, pixels: out };
  return out;
}

/** The colour under the pointer as seen (undefined outside the image). */
export function loupeColor(doc: Doc, ix: number, iy: number): Pixel {
  return pixels(doc, Math.floor(ix), Math.floor(iy))[(CELLS * CELLS - 1) / 2];
}

/** Straight RGBA over a checkerboard square of `light` / `dark` grey. */
function overChecker(p: [number, number, number, number], light: boolean): string {
  const a = p[3] / 255;
  const back = light ? 204 : 153;
  const mix = (c: number) => Math.round(c * a + back * (1 - a));
  return `rgb(${mix(p[0])},${mix(p[1])},${mix(p[2])})`;
}

function css(c: Rgba | Pixel): string {
  if (!c) return "rgba(0,0,0,0)";
  return Array.isArray(c)
    ? `rgba(${c[0]},${c[1]},${c[2]},${c[3] / 255})`
    : `rgba(${c.r},${c.g},${c.b},${c.a})`;
}

function hex(p: [number, number, number, number]): string {
  return (
    "#" +
    p
      .slice(0, 3)
      .map((v) => v.toString(16).padStart(2, "0"))
      .join("")
      .toUpperCase()
  );
}

/**
 * Draws the loupe at device point `(sx, sy)` over image point `(ix, iy)`. `current` is the
 * colour a pick would replace; `outside` is the canvas background (no image there).
 */
export function drawLoupe(
  ctx: CanvasRenderingContext2D,
  doc: Doc,
  sx: number,
  sy: number,
  ix: number,
  iy: number,
  current: Rgba,
  outside: string,
) {
  const dpr = devicePixelRatio;
  const r = RADIUS * dpr;
  const ring = RING * dpr;
  const px = Math.floor(ix);
  const py = Math.floor(iy);
  const grid = pixels(doc, px, py);
  const picked = grid[(CELLS * CELLS - 1) / 2];
  const cell = (2 * r) / CELLS;
  const left = sx - r;
  const top = sy - r;

  ctx.save();
  // Soft shadow under the lens.
  ctx.beginPath();
  ctx.arc(sx, sy, r + ring, 0, Math.PI * 2);
  ctx.shadowColor = "rgba(0,0,0,0.45)";
  ctx.shadowBlur = 10 * dpr;
  ctx.shadowOffsetY = 2 * dpr;
  ctx.fillStyle = "rgba(0,0,0,0.5)";
  ctx.fill();
  ctx.restore();

  // Magnified pixels, clipped to the lens.
  ctx.save();
  ctx.beginPath();
  ctx.arc(sx, sy, r, 0, Math.PI * 2);
  ctx.clip();
  ctx.fillStyle = outside;
  ctx.fillRect(left, top, 2 * r, 2 * r);
  for (let j = 0; j < CELLS; j++) {
    for (let i = 0; i < CELLS; i++) {
      const p = grid[j * CELLS + i];
      if (!p) continue;
      const x = left + i * cell;
      const y = top + j * cell;
      // Snapped edges (no hairline gaps between cells).
      const x0 = Math.round(x);
      const y0 = Math.round(y);
      const w = Math.round(x + cell) - x0;
      const h = Math.round(y + cell) - y0;
      if (p[3] === 255) {
        ctx.fillStyle = `rgb(${p[0]},${p[1]},${p[2]})`;
        ctx.fillRect(x0, y0, w, h);
      } else {
        // Transparency: a 2x2 checker inside the cell.
        const hw = Math.round(w / 2);
        const hh = Math.round(h / 2);
        ctx.fillStyle = overChecker(p, true);
        ctx.fillRect(x0, y0, hw, hh);
        ctx.fillRect(x0 + hw, y0 + hh, w - hw, h - hh);
        ctx.fillStyle = overChecker(p, false);
        ctx.fillRect(x0 + hw, y0, w - hw, hh);
        ctx.fillRect(x0, y0 + hh, hw, h - hh);
      }
    }
  }
  // Faint pixel grid.
  ctx.beginPath();
  for (let k = 1; k < CELLS; k++) {
    const x = Math.round(left + k * cell) + 0.5;
    const y = Math.round(top + k * cell) + 0.5;
    ctx.moveTo(x, top);
    ctx.lineTo(x, top + 2 * r);
    ctx.moveTo(left, y);
    ctx.lineTo(left + 2 * r, y);
  }
  ctx.lineWidth = 1;
  ctx.strokeStyle = "rgba(0,0,0,0.12)";
  ctx.stroke();
  // The picked pixel: black and white frame.
  const half = (CELLS - 1) / 2;
  const cx = Math.round(left + half * cell);
  const cy = Math.round(top + half * cell);
  const cw = Math.round(left + (half + 1) * cell) - cx;
  ctx.lineWidth = 2 * dpr;
  ctx.strokeStyle = "rgba(0,0,0,0.85)";
  ctx.strokeRect(cx - dpr, cy - dpr, cw + 2 * dpr, cw + 2 * dpr);
  ctx.lineWidth = dpr;
  ctx.strokeStyle = "#fff";
  ctx.strokeRect(cx + 0.5 * dpr, cy + 0.5 * dpr, cw - dpr, cw - dpr);
  ctx.restore();

  // Ring: the colour under the pointer (top) and the one it would replace (bottom).
  const mid = r + ring / 2;
  ctx.save();
  ctx.lineWidth = ring;
  ctx.beginPath();
  ctx.arc(sx, sy, mid, Math.PI, 0);
  ctx.strokeStyle = picked && picked[3] > 0 ? css(picked) : "rgba(128,128,128,0.6)";
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(sx, sy, mid, 0, Math.PI);
  ctx.strokeStyle = css(current);
  ctx.stroke();
  // Thin dark edges so the ring reads on any colour.
  ctx.lineWidth = dpr;
  ctx.strokeStyle = "rgba(0,0,0,0.55)";
  ctx.beginPath();
  ctx.arc(sx, sy, r, 0, Math.PI * 2);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(sx, sy, r + ring, 0, Math.PI * 2);
  ctx.stroke();

  // Hex value under the lens.
  if (picked) {
    const label = picked[3] === 0 ? "Transparent" : hex(picked);
    ctx.font = `600 ${11 * dpr}px system-ui, sans-serif`;
    const tw = ctx.measureText(label).width;
    const pad = 6 * dpr;
    const bh = 18 * dpr;
    const bw = tw + 2 * pad;
    const bx = sx - bw / 2;
    const by = sy + r + ring + 8 * dpr;
    ctx.beginPath();
    ctx.roundRect(bx, by, bw, bh, bh / 2);
    ctx.fillStyle = "rgba(20,20,20,0.85)";
    ctx.fill();
    ctx.fillStyle = "#fff";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(label, sx, by + bh / 2 + 0.5 * dpr);
  }
  ctx.restore();
}
