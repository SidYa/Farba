// Loads the farba-wasm module once and exposes its linear memory for zero-copy tile reads.
import init, { type InitOutput } from "./pkg/farba_wasm";

export * from "./pkg/farba_wasm";

let ready: Promise<InitOutput> | undefined;
let output: InitOutput | undefined;

export function loadCore(): Promise<InitOutput> {
  ready ??= init().then((o) => (output = o));
  return ready;
}

/**
 * View of `len` bytes at `ptr` in WASM memory. Must be re-created after any call into
 * WASM that may allocate (memory growth detaches old views), so never keep it.
 */
export function wasmBytes(ptr: number, len: number): Uint8Array {
  if (!output) throw new Error("farba-wasm is not loaded");
  return new Uint8Array(output.memory.buffer, ptr, len);
}
