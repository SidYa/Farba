// Rulers and guides (GIMP): off by default, Settings > Canvas > "Show rulers and guides".
// Drag from a ruler onto the canvas to add a guide; drag a guide (with any tool) to move it,
// or back onto a ruler / off the canvas to remove it; Image > Remove All Guides. Rectangle / Ellipse Select
// snap to guides. Guides belong to the document's view (not undo steps, not saved).
import type { View } from "./canvas/renderer";
import type { Doc } from "./documents.svelte";
import { loadSetting, saveSetting } from "./settings";

export interface Guide {
  horizontal: boolean;
  /** Image row (horizontal) or column (vertical). */
  pos: number;
}

/** Ruler thickness in CSS pixels. */
export const RULER = 18;
/** How close (in screen pixels) the pointer snaps to a guide / grabs one (GIMP: 8). */
const SNAP = 8;

class GuidePrefs {
  show = $state<boolean>(loadSetting("show-rulers", false));

  setShow(on: boolean) {
    this.show = on;
    saveSetting("show-rulers", on);
  }
}

export const guidePrefs = new GuidePrefs();

/** A guide being dragged (from a ruler or with the Move tool); `pos` undefined = removed. */
export const guideDrag = $state<{
  doc?: Doc;
  horizontal: boolean;
  pos?: number;
  /** Index in `doc.guides` of the guide being moved (undefined: a new one). */
  index?: number;
}>({ horizontal: true });

/** Status bar text while a guide is dragged. */
export function guideStatus(): string {
  if (!guideDrag.doc) return "";
  const kind = guideDrag.horizontal ? "Horizontal guide" : "Vertical guide";
  return guideDrag.pos === undefined ? `${kind}: remove` : `${kind}: ${guideDrag.pos} px`;
}

/** The guides shown for `doc` (none when rulers and guides are off). */
export function visibleGuides(doc: Doc): Guide[] {
  return guidePrefs.show ? doc.guides : [];
}

/** Snaps image point (x, y) to the nearest guides within `SNAP` screen pixels. */
export function snap(doc: Doc, x: number, y: number): [number, number] {
  const guides = visibleGuides(doc);
  if (!guides.length) return [x, y];
  const reach = (SNAP * devicePixelRatio) / doc.view.zoom;
  const near = (v: number, horizontal: boolean) => {
    let best = v;
    let dist = reach;
    for (const g of guides)
      if (g.horizontal === horizontal && Math.abs(g.pos - v) <= dist) {
        dist = Math.abs(g.pos - v);
        best = g.pos;
      }
    return best;
  };
  return [near(x, false), near(y, true)];
}

/** The guide of `doc` near device point (sx, sy) (for the Move tool), as an index. */
export function guideAt(doc: Doc, view: View, sx: number, sy: number): number | undefined {
  const reach = (SNAP / 2) * devicePixelRatio;
  let best: number | undefined;
  let dist = reach;
  visibleGuides(doc).forEach((g, i) => {
    const d = g.horizontal
      ? Math.abs(g.pos * view.zoom + view.y - sy)
      : Math.abs(g.pos * view.zoom + view.x - sx);
    if (d <= dist) {
      dist = d;
      best = i;
    }
  });
  return best;
}

/** Image coordinate of a guide at device point (sx, sy). */
export function guidePos(view: View, horizontal: boolean, sx: number, sy: number): number {
  return Math.round(horizontal ? (sy - view.y) / view.zoom : (sx - view.x) / view.zoom);
}

/** Ends a guide drag: adds, moves or (pos undefined) removes the guide. */
export function finishGuideDrag() {
  const { doc, horizontal, pos, index } = guideDrag;
  guideDrag.doc = undefined;
  guideDrag.pos = undefined;
  guideDrag.index = undefined;
  if (!doc) return;
  const guides = doc.guides.filter((_, i) => i !== index);
  if (pos !== undefined) guides.push({ horizontal, pos });
  doc.guides = guides;
}
