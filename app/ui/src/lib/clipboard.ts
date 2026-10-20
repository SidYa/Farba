// Copy / cut / paste through the Windows clipboard (the backend uses `arboard`). Pixels go
// through shared memory in both directions. Copy and cut take the selected part of the
// active layer (its bounding box, alpha scaled by the selection), or the whole layer
// without a selection, as in GIMP.
import { documents, errorText, type Doc } from "./documents.svelte";
import { invokeShared, sendShared } from "./ipc/shared";
import { notify } from "./notify.svelte";
import { wasmBytes } from "./wasm/core";

export async function copyLayer(doc: Doc, cut: boolean) {
  const l = doc.activeLayer;
  if (!l) return;
  try {
    const r = doc.core.copy_pixels(l.id);
    if (!r) {
      notify("The selection does not touch the active layer", "error");
      return;
    }
    const [ptr, , , width, height] = r as unknown as number[];
    const len = width! * height! * 4;
    try {
      await sendShared(len, (dst) => dst.set(wasmBytes(ptr!, len)), "clipboard_set_image", {
        width,
        height,
      });
    } finally {
      doc.core.free_pixels();
    }
  } catch (e) {
    notify(`Could not copy: ${errorText(e)}`, "error");
    return;
  }
  if (cut) doc.run((c) => c.clear_layer(l.id, "Cut"));
}

async function readClipboard() {
  const r = await invokeShared<{ width: number; height: number }>("clipboard_get_image", {});
  return { ...r.header, bytes: new Uint8Array(r.buffer), release: r.release };
}

/** Pastes the clipboard image as a new layer centred on the selection, or on the image. */
export async function paste(doc: Doc) {
  try {
    const img = await readClipboard();
    try {
      const [bx, by, bw, bh] = doc.selection ?? [0, 0, doc.width, doc.height];
      const x = bx + Math.round((bw - img.width) / 2);
      const y = by + Math.round((bh - img.height) / 2);
      doc.run((c) =>
        c.add_layer_rgba("Paste", "Pasted Layer", img.width, img.height, img.bytes, x, y),
      );
    } finally {
      img.release();
    }
  } catch (e) {
    notify(errorText(e), "error");
  }
}

export async function pasteAsNewImage() {
  try {
    const img = await readClipboard();
    try {
      await documents.createFrom("Clipboard", img.width, img.height, img.bytes);
    } finally {
      img.release();
    }
  } catch (e) {
    notify(errorText(e), "error");
  }
}
