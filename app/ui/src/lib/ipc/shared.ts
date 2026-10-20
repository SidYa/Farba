// Receives large pixel buffers from the backend through WebView2 SharedBuffer (zero-copy;
// see src-tauri/src/shared.rs). The command is invoked with an `id`; the backend posts the
// buffer with a JSON header containing the same `id`.
// Without WebView2 (Linux: WebKitGTK) the same functions move the bytes as raw IPC bodies
// (src-tauri/src/shared_ipc.rs): the page fetches a posted buffer after the command and
// uploads its own bytes before it.
import { invoke } from "@tauri-apps/api/core";

interface SharedBufferEvent {
  getBuffer(): ArrayBuffer;
  additionalData: { id: number } & Record<string, unknown>;
}

interface WebView2 {
  addEventListener(type: "sharedbufferreceived", cb: (e: SharedBufferEvent) => void): void;
  releaseBuffer(buffer: ArrayBuffer): void;
}

declare global {
  interface Window {
    chrome?: { webview?: WebView2 };
  }
}

export interface SharedResult<H> {
  id: number;
  header: H;
  buffer: ArrayBuffer;
  /** Unmaps the shared memory. The buffer must not be used afterwards. */
  release(): void;
}

type Received = { buffer: ArrayBuffer; header: Record<string, unknown> };

const waiting = new Map<number, (r: Received) => void>();
let nextId = 1;
let listening = false;

/** True with WebView2 shared memory (Windows); the IPC fallback copies the bytes. */
export function sharedBuffersAvailable(): boolean {
  return !!window.chrome?.webview;
}

export async function invokeShared<H>(
  cmd: string,
  args: Record<string, unknown>,
): Promise<SharedResult<H>> {
  const webview = window.chrome?.webview;
  if (!webview) return invokeCopied<H>(cmd, args);
  if (!listening) {
    webview.addEventListener("sharedbufferreceived", (e) => {
      // getBuffer() is only valid during the event dispatch.
      const buffer = e.getBuffer();
      const resolve = waiting.get(e.additionalData.id);
      if (resolve) {
        waiting.delete(e.additionalData.id);
        resolve({ buffer, header: e.additionalData });
      } else {
        webview.releaseBuffer(buffer);
      }
    });
    listening = true;
  }
  const id = nextId++;
  const received = new Promise<Received>((resolve) => waiting.set(id, resolve));
  try {
    await invoke(cmd, { ...args, id });
  } catch (err) {
    waiting.delete(id);
    throw err;
  }
  const { buffer, header } = await received;
  return { id, header: header as H, buffer, release: () => webview.releaseBuffer(buffer) };
}

/** `invokeShared` without WebView2: the backend keeps the buffer until it is fetched. */
async function invokeCopied<H>(
  cmd: string,
  args: Record<string, unknown>,
): Promise<SharedResult<H>> {
  const id = nextId++;
  await invoke(cmd, { ...args, id });
  const header = JSON.parse(await invoke<string>("shared_header", { id })) as H;
  const buffer = await invoke<ArrayBuffer>("shared_data", { id });
  return { id, header, buffer, release: () => {} };
}

/** Puts `len` bytes written by `fill` into the backend; returns the id to read them by. */
async function upload(len: number, fill: (dst: Uint8Array) => void): Promise<number> {
  if (!window.chrome?.webview) {
    const id = nextId++;
    const bytes = new Uint8Array(len);
    fill(bytes);
    await invoke("shared_put", bytes, { headers: { "farba-buffer": String(id) } });
    return id;
  }
  const { id, buffer, release } = await invokeShared<unknown>("lend_buffer", { len });
  try {
    fill(new Uint8Array(buffer));
  } finally {
    release();
  }
  return id;
}

/**
 * Sends `len` bytes to the backend through shared memory: the backend lends an empty buffer
 * (`lend_buffer`), `fill` writes into it, then `cmd` runs with `bufferId` and reads it.
 */
export async function sendShared<R>(
  len: number,
  fill: (dst: Uint8Array) => void,
  cmd: string,
  args: Record<string, unknown>,
): Promise<R> {
  const id = await upload(len, fill);
  return invoke<R>(cmd, { ...args, bufferId: id });
}

/**
 * Sends `len` bytes like `sendShared`, to a command that answers with a shared buffer
 * (like `invokeShared`).
 */
export async function exchangeShared<H>(
  len: number,
  fill: (dst: Uint8Array) => void,
  cmd: string,
  args: Record<string, unknown>,
): Promise<SharedResult<H>> {
  const id = await upload(len, fill);
  return invokeShared<H>(cmd, { ...args, bufferId: id });
}
