// Files dragged from Explorer. Where they are dropped decides what happens (owner decision,
// see CLAUDE.md "Drag and drop"):
// - on the canvas of an open image: added to it as new layers;
// - on the image tabs bar (or the canvas when no image is open): opened as new tabs.
// Drop zones are elements with `data-drop-zone="canvas" | "tabs"`; the zone under the
// pointer is highlighted while dragging.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { documents } from "./documents.svelte";
import { importLayers, openPaths } from "./files";

export type DropZone = "canvas" | "tabs";

/** The zone under the pointer while files are dragged over the window. */
export const dragState = $state<{ zone: DropZone | undefined }>({ zone: undefined });

function zoneAt(pos: { x: number; y: number }): DropZone | undefined {
  // Tauri reports physical pixels.
  const el = document.elementFromPoint(pos.x / devicePixelRatio, pos.y / devicePixelRatio);
  const zone = el?.closest<HTMLElement>("[data-drop-zone]")?.dataset.dropZone;
  return zone === "canvas" || zone === "tabs" ? zone : undefined;
}

/** Listens for file drops on the window; returns the unsubscribe function. */
export function initDragDrop(): () => void {
  const un = getCurrentWebview().onDragDropEvent(async (ev) => {
    const p = ev.payload;
    if (p.type === "leave") {
      dragState.zone = undefined;
      return;
    }
    const zone = zoneAt(p.position);
    if (p.type !== "drop") {
      dragState.zone = zone;
      return;
    }
    dragState.zone = undefined;
    const doc = documents.active;
    if (zone === "canvas" && doc) await importLayers(doc, p.paths);
    else if (zone) await openPaths(p.paths);
  });
  return () => void un.then((f) => f());
}
