// Heavy pixel work in the native backend, on all cores and off the UI thread: adjustment
// and effect previews, Scale Image and the transform tools' render (src-tauri/src/ops.rs).
// The pixels travel through shared memory; the same farba-core code runs there, and the
// result is recorded in the WASM document, which stays the only source of truth. Small
// images (or a webview without shared buffers) keep using the WASM core directly.
import { invoke } from "@tauri-apps/api/core";
import { documents, type Doc } from "./documents.svelte";
import { exchangeShared, invokeShared, sendShared, sharedBuffersAvailable } from "./ipc/shared";
import { wasmBytes } from "./wasm/core";

/** Pixels (of the layers worked on) from which the backend is used. */
const NATIVE_MIN_PIXELS = 1_000_000;

function useNative(pixels: number): boolean {
  return sharedBuffersAvailable() && pixels >= NATIVE_MIN_PIXELS;
}

/**
 * Runs `work` with a status text while the rest of the app ignores input (the page keeps
 * painting; only edits are held back until the result is in).
 */
export async function whileBusy<T>(text: string, work: () => Promise<T>): Promise<T> {
  documents.busy = text;
  documents.blocking++;
  try {
    // Show the status first (one frame).
    await new Promise((f) => requestAnimationFrame(() => setTimeout(f)));
    return await work();
  } finally {
    documents.blocking--;
    documents.busy = "";
  }
}

/** The active layer's image-sized selection mask into `dst` at `off` (when there is one). */
function copySelection(doc: Doc, dst: Uint8Array, off: number) {
  const ptr = doc.core.selection_pixels();
  try {
    if (ptr) dst.set(wasmBytes(ptr, doc.width * doc.height), off);
  } finally {
    doc.core.free_pixels();
  }
}

/**
 * Live preview of an adjustment / effect dialog on the active layer. `show` calls are
 * queued; a call that is superseded by a later one is skipped, and the returned promise
 * resolves once the latest values are on the canvas.
 */
export class FilterPreview {
  private chain: Promise<void> = Promise.resolve();
  private latest = 0;
  /** Backend session key whose source pixels were sent, and what is shown. */
  private sent = "";
  private shown = "";
  private closed = false;

  constructor(private doc: Doc) {}

  show(kind: number, params: number[]): Promise<void> {
    const n = ++this.latest;
    const p = this.chain.then(() => (n === this.latest ? this.run(kind, params) : undefined));
    // A failed preview does not stop the next ones.
    this.chain = p.catch(() => undefined);
    return p;
  }

  /** The dialog closed: forget the backend's copy of the pixels. */
  end() {
    this.closed = true;
    if (this.sent) void invoke("op_filter_end");
    this.sent = this.shown = "";
  }

  private async run(kind: number, params: number[]) {
    if (this.closed) return;
    const doc = this.doc;
    const core = doc.core;
    const serial = core.filter_begin();
    const info = core.filter_original(false);
    if (!info) return;
    const [, w, h, x, y] = info as unknown as number[];
    const key = `${doc.id}:${serial}`;
    const what = JSON.stringify([key, kind, params]);
    if (what === this.shown) return;
    if (!useNative(w! * h!)) {
      core.filter_preview(kind, Float32Array.from(params));
      doc.sync();
      this.shown = what;
      return;
    }
    if (this.sent !== key) {
      const len = w! * h! * 4;
      const mask = !!doc.selection;
      await sendShared(
        len + (mask ? doc.width * doc.height : 0),
        (dst) => {
          const ptr = core.filter_original(true)![0]!;
          try {
            dst.set(wasmBytes(ptr, len));
          } finally {
            core.free_pixels();
          }
          if (mask) copySelection(doc, dst, len);
        },
        "op_filter_source",
        { key, width: w, height: h, pos: [x, y], image: [doc.width, doc.height], mask },
      );
      this.sent = key;
      if (this.closed) return this.end();
    }
    const r = await invokeShared<{ width: number; height: number }>("op_filter", {
      key,
      kind,
      params,
    });
    try {
      const { width, height } = r.header;
      if (core.filter_show(serial, width, height, new Uint8Array(r.buffer))) this.shown = what;
    } finally {
      r.release();
    }
    doc.sync();
  }
}

/** Scale Image through the backend; false when the WASM core should do it. */
export async function scaleImage(
  doc: Doc,
  width: number,
  height: number,
  interp: number,
): Promise<boolean> {
  const core = doc.core;
  core.settle();
  const layers = doc.layers.map((l) => ({ id: l.id, w: l.width, h: l.height }));
  const pixels = layers.reduce((n, l) => n + l.w * l.h, 0);
  if (!useNative(pixels)) return false;
  const mask = !!doc.selection;
  const len = pixels * 4 + (mask ? doc.width * doc.height : 0);
  const r = await exchangeShared<{ sizes: [number, number][] }>(
    len,
    (dst) => {
      let off = 0;
      for (const l of layers) {
        const ptr = core.layer_pixels(l.id);
        try {
          dst.set(wasmBytes(ptr, l.w * l.h * 4), off);
        } finally {
          core.free_pixels();
        }
        off += l.w * l.h * 4;
      }
      if (mask) copySelection(doc, dst, off);
    },
    "op_scale",
    {
      sizes: layers.map((l) => [l.w, l.h]),
      image: [doc.width, doc.height],
      to: [width, height],
      interp,
      mask,
    },
  );
  try {
    const sizes = r.header.sizes;
    const n = sizes.reduce((s, [w, h]) => s + w * h * 4, 0);
    const all = new Uint8Array(r.buffer);
    const selection = mask ? all.slice(n, n + width * height) : undefined;
    doc.run((c) =>
      c.scale_image_with(
        width,
        height,
        Uint32Array.from(sizes.flat()),
        all.subarray(0, n),
        selection,
      ),
    );
  } finally {
    r.release();
  }
  return true;
}

/**
 * Applies the transform in progress with matrix `m` through the backend; false when the
 * WASM core should do it (small layer, or no transform in progress).
 */
export async function transformCommit(
  doc: Doc,
  m: number[],
  interp: number,
  label: string,
): Promise<boolean> {
  const core = doc.core;
  const info = core.transform_job(Float64Array.from(m), interp, false);
  if (!info) return false;
  const [, sw, sh] = info as unknown as number[];
  if (!useNative(sw! * sh!)) return false;
  const matrix = Array.from(info.slice(3, 12));
  const jobInterp = info[12]!;
  const mask = info[13] === 1;
  const len = sw! * sh! * 4;
  const maskLen = mask ? doc.width * doc.height : 0;
  const r = await exchangeShared<{
    rect: number[] | null;
    maskRect: number[] | null;
  }>(
    len + maskLen,
    (dst) => {
      const job = core.transform_job(Float64Array.from(m), interp, true);
      try {
        if (job) dst.set(wasmBytes(job[0]!, len + maskLen));
      } finally {
        core.free_pixels();
      }
    },
    "op_transform",
    {
      width: sw,
      height: sh,
      matrix,
      interp: jobInterp,
      mask: mask ? { width: doc.width, height: doc.height, matrix: m } : null,
    },
  );
  try {
    const { rect, maskRect } = r.header;
    const all = new Uint8Array(r.buffer);
    const n = rect ? rect[2]! * rect[3]! * 4 : 0;
    const data = all.slice(0, n);
    const maskData = maskRect ? all.slice(n, n + maskRect[2]! * maskRect[3]!) : new Uint8Array();
    doc.run((c) =>
      c.transform_commit_with(
        Float64Array.from(m),
        label,
        rect ? Int32Array.from(rect) : undefined,
        data,
        maskRect ? Int32Array.from(maskRect) : undefined,
        maskData,
      ),
    );
  } finally {
    r.release();
  }
  return true;
}
