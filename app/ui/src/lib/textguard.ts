// Changing the pixels of a text layer turns it into a normal layer (its text can no longer
// be edited). As GIMP does, ask first; Undo of the change brings the text layer back.
import { confirmAction } from "./dialogs.svelte";
import type { Doc } from "./documents.svelte";

/** The text layer the user already agreed to change, per document. */
const agreed = new WeakMap<Doc, number>();

/** True when the active layer may be changed (not a text layer, or the user agreed). */
export function rasterAllowed(doc: Doc): boolean {
  const l = doc.activeLayer;
  return !l?.isText || agreed.get(doc) === l.id;
}

/** Asks before changing the pixels of a text layer; resolves true to go ahead. */
export async function confirmRasterize(doc: Doc): Promise<boolean> {
  if (rasterAllowed(doc)) return true;
  const l = doc.activeLayer!;
  const ok = await confirmAction(
    "Modify Text Layer",
    `“${l.name}” is a text layer. Changing its pixels makes it a normal layer, and its text ` +
      `can no longer be edited (Undo brings the text back).`,
    "Continue",
  );
  if (ok) agreed.set(doc, l.id);
  return ok;
}
