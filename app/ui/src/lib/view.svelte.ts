// Zoom / pan of the active document's view. The viewport reports its size here so that
// actions (zoom presets, fit) can work without a reference to the component.
import type { View } from "./canvas/renderer";
import { documents, type Doc } from "./documents.svelte";

export const viewport = $state({ width: 1, height: 1 });

/** Pointer position in image pixels (or undefined when outside the canvas). */
export const pointer = $state<{ x?: number; y?: number }>({});

export const MIN_ZOOM = 1 / 256;
export const MAX_ZOOM = 256;

// prettier-ignore
const PRESETS = [
  1 / 256, 1 / 128, 1 / 64, 1 / 32, 1 / 16, 1 / 12, 1 / 8, 1 / 6, 1 / 4, 1 / 3, 1 / 2, 2 / 3,
  1, 1.5, 2, 3, 4, 5.5, 8, 11, 16, 23, 32, 45, 64, 90, 128, 180, 256,
];

/** Zooms `doc` to `zoom`, keeping the image point under screen point (sx, sy) fixed. */
export function zoomAt(doc: Doc, zoom: number, sx = viewport.width / 2, sy = viewport.height / 2) {
  const z = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
  const v = doc.view;
  const ix = (sx - v.x) / v.zoom;
  const iy = (sy - v.y) / v.zoom;
  doc.view = { zoom: z, x: Math.round(sx - ix * z), y: Math.round(sy - iy * z) };
}

export function zoomStep(doc: Doc, dir: 1 | -1, sx?: number, sy?: number) {
  const z = doc.view.zoom;
  const next =
    dir > 0
      ? PRESETS.find((p) => p > z * 1.001)
      : [...PRESETS].reverse().find((p) => p < z / 1.001);
  if (next) zoomAt(doc, next, sx, sy);
}

export function fit(doc: Doc) {
  const z = Math.min(viewport.width / doc.width, viewport.height / doc.height);
  // Never enlarge small images past 100% when fitting on open (GIMP does the same).
  const zoom = Math.min(z, 1);
  doc.view = {
    zoom,
    x: Math.round((viewport.width - doc.width * zoom) / 2),
    y: Math.round((viewport.height - doc.height * zoom) / 2),
  };
}

/** Zooms the active document to an exact ratio (view-zoom-x-y actions), centred. */
export function zoomActive(zoom: number) {
  if (documents.active) zoomAt(documents.active, zoom);
}

export function screenToImage(v: View, sx: number, sy: number): [number, number] {
  return [(sx - v.x) / v.zoom, (sy - v.y) / v.zoom];
}
