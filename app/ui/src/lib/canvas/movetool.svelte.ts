// Move tool (GIMP behaviour): drag moves a layer or the selection outline.
// - Layer, "Pick a layer" (default on): the topmost visible layer with a mostly opaque pixel
//   under the pointer becomes active and moves. Off: the selected layers move. Shift at the
//   press toggles between the two.
// - Selection: the selection outline moves (the pixels stay).
// - Arrow keys move by 1 px; Shift+arrow by 25 screen pixels (at least 1 image pixel).
// While dragging the move is only drawn (the viewport offsets the layers / the outline);
// it becomes one undo step on release.
import type { Doc } from "../documents.svelte";
import { notify } from "../notify.svelte";
import { tools } from "../tools.svelte";

export interface MovePreview {
  doc: Doc;
  /** Layers being moved; empty when the selection moves. */
  layers: ReadonlySet<number>;
  selection: boolean;
  dx: number;
  dy: number;
}

class MoveTool {
  /** The move in progress, read by the viewport to draw it. */
  preview: MovePreview | undefined;
  private start: [number, number] = [0, 0];

  get busy(): boolean {
    return !!this.preview;
  }

  cancel() {
    this.preview = undefined;
  }

  private targets(doc: Doc, e: { shiftKey: boolean }, ix: number, iy: number): number[] {
    const pick = tools.option<boolean>("move", "pickLayer") !== e.shiftKey;
    if (pick) {
      const id = doc.core.pick_layer(ix, iy);
      if (id === undefined) return [];
      doc.select(id);
      return [id];
    }
    return doc.selectedIds;
  }

  down(doc: Doc, e: PointerEvent, ix: number, iy: number) {
    const selection = tools.option<string>("move", "target") === "selection";
    let layers: number[] = [];
    if (selection) {
      if (!doc.selection) return;
    } else {
      layers = this.targets(doc, e, ix, iy);
      if (!layers.length) return;
      if (layers.some((id) => doc.layer(id)?.lockPosition)) {
        notify("The layer's position is locked", "error");
        return;
      }
    }
    this.start = [ix, iy];
    this.preview = { doc, layers: new Set(layers), selection, dx: 0, dy: 0 };
  }

  move(ix: number, iy: number) {
    if (!this.preview) return;
    this.preview.dx = Math.round(ix - this.start[0]);
    this.preview.dy = Math.round(iy - this.start[1]);
  }

  up() {
    const p = this.preview;
    this.preview = undefined;
    if (!p || (p.dx === 0 && p.dy === 0)) return;
    if (p.selection) p.doc.run((c) => c.move_selection_by(p.dx, p.dy));
    else p.doc.run((c) => c.move_layers_by(Uint32Array.from(p.layers), p.dx, p.dy));
  }

  /** Arrow keys with the Move tool; returns true when the key was used. */
  key(doc: Doc, e: KeyboardEvent): boolean {
    const dirs: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    const d = dirs[e.code];
    if (!d || e.ctrlKey || e.altKey || this.preview) return false;
    const step = e.shiftKey ? Math.max(1, Math.round(25 / doc.view.zoom)) : 1;
    const [dx, dy] = [d[0] * step, d[1] * step];
    if (tools.option<string>("move", "target") === "selection") {
      doc.run((c) => c.move_selection_by(dx, dy));
    } else {
      const ids = doc.selectedIds;
      if (ids.some((id) => doc.layer(id)?.lockPosition)) {
        notify("The layer's position is locked", "error");
      } else {
        doc.run((c) => c.move_layers_by(Uint32Array.from(ids), dx, dy));
      }
    }
    return true;
  }
}

export const moveTool = new MoveTool();
