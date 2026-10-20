// Layered files (OpenRaster .ora, Farba's native format): the header and pixel buffer that
// travel between the page and the backend. Mirrors farba-io's `Stack` (core/farba-io/src/
// stack.rs), including the buffer layout:
//   for every layer, bottom to top: its straight RGBA8 pixels, then its transform source's
//   pixels if it has one; then the selection (one byte per image pixel) if `selection`.
import type { Doc } from "./documents.svelte";
import { sendShared } from "./ipc/shared";
import { Doc as CoreDoc, DocBuilder, wasmBytes } from "./wasm/core";

export interface StackSource {
  width: number;
  height: number;
  matrix: number[];
}

export interface StackLayer {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  opacity: number;
  visible: boolean;
  lockAlpha: boolean;
  lockPosition: boolean;
  text?: string | null;
  nameFromText: boolean;
  source?: StackSource | null;
}

export interface Stack {
  width: number;
  height: number;
  /** Index of the active layer (bottom = 0). */
  active: number;
  /** Bottom to top. */
  layers: StackLayer[];
  selection: boolean;
  /** What could not be kept when reading the file. */
  notes?: string[];
}

/** Builds a WASM document from a stack and its pixels (no undo history, nothing unsaved). */
export function buildDoc(stack: Stack, bytes: Uint8Array): CoreDoc {
  const b = new DocBuilder(stack.width, stack.height);
  let off = 0;
  const take = (n: number) => bytes.subarray(off, (off += n));
  try {
    for (const l of stack.layers) {
      b.add_layer(
        l.name,
        l.width,
        l.height,
        take(l.width * l.height * 4),
        l.x,
        l.y,
        l.opacity,
        l.visible,
        l.lockAlpha,
        l.lockPosition,
        l.text ?? undefined,
        l.nameFromText,
      );
      const s = l.source;
      if (s)
        b.set_source(s.width, s.height, take(s.width * s.height * 4), new Float64Array(s.matrix));
    }
    if (stack.selection) b.set_selection(take(stack.width * stack.height));
  } catch (e) {
    b.free();
    throw e;
  }
  return b.build(stack.active);
}

/** Saves `doc` with all its layers as OpenRaster. */
export function saveLayered(doc: Doc, path: string): Promise<void> {
  return sendStack(doc, true, "save_layered", { path });
}

/**
 * Sends `doc`'s layers to backend command `cmd` (as `stack` plus a lent buffer). Without
 * `full` only the layers' own pixels go (no transform sources, no selection).
 */
export async function sendStack(
  doc: Doc,
  full: boolean,
  cmd: string,
  args: Record<string, unknown>,
): Promise<void> {
  const core = doc.core;
  core.settle();
  doc.sync();
  const layers: StackLayer[] = doc.layers.map((l) => {
    const extra = core.layer_extra(l.id) as {
      text?: string;
      nameFromText: boolean;
      source?: StackSource;
    };
    return {
      name: l.name,
      x: l.x,
      y: l.y,
      width: l.width,
      height: l.height,
      opacity: l.opacity,
      visible: l.visible,
      lockAlpha: l.lockAlpha,
      lockPosition: l.lockPosition,
      text: extra.text ?? null,
      nameFromText: extra.nameFromText,
      source: full ? (extra.source ?? null) : null,
    };
  });
  const stack: Stack = {
    width: doc.width,
    height: doc.height,
    active: Math.max(
      0,
      doc.layers.findIndex((l) => l.id === doc.active),
    ),
    layers,
    selection: full && !!doc.selection,
  };
  let len = stack.selection ? stack.width * stack.height : 0;
  for (const l of layers) {
    len += l.width * l.height * 4;
    if (l.source) len += l.source.width * l.source.height * 4;
  }
  const fill = (dst: Uint8Array) => {
    let off = 0;
    const copy = (ptr: number, n: number) => {
      try {
        dst.set(wasmBytes(ptr, n), off);
        off += n;
      } finally {
        core.free_pixels();
      }
    };
    for (const [i, l] of doc.layers.entries()) {
      copy(core.layer_pixels(l.id), l.width * l.height * 4);
      const s = layers[i]!.source;
      if (s) copy(core.source_pixels(l.id), s.width * s.height * 4);
    }
    if (stack.selection) copy(core.selection_pixels(), stack.width * stack.height);
  };
  await sendShared(len, fill, cmd, { ...args, stack });
}
