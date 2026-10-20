// Text rendering through the backend (farba-text: system fonts, cosmic-text, swash). The
// pixels come back through shared memory; `lines` gives the caret positions.
import { invoke } from "@tauri-apps/api/core";
import { invokeShared } from "../ipc/shared";
import type { TextSpec } from "./model";

export interface LineLayout {
  start: number;
  end: number;
  top: number;
  height: number;
  baseline: number;
  /** `[index, x]` character boundaries, sorted by index. */
  carets: [number, number][];
}

export interface RenderedText {
  width: number;
  height: number;
  lines: LineLayout[];
  bytes: Uint8Array;
  release(): void;
}

export async function renderText(spec: TextSpec): Promise<RenderedText> {
  const r = await invokeShared<{ width: number; height: number; lines: LineLayout[] }>(
    "render_text",
    { spec },
  );
  return { ...r.header, bytes: new Uint8Array(r.buffer), release: r.release };
}

let fonts: Promise<string[]> | undefined;

/** Installed font families (loaded once). */
export function fontFamilies(): Promise<string[]> {
  fonts ??= invoke<string[]>("text_fonts").catch(() => []);
  return fonts;
}
