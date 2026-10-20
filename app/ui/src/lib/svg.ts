// SVG import: asks for the size to render at (SvgImportDialog, GIMP's "Render Scalable
// Vector Graphics") before the backend renders it (farba-io svg.rs).
import { invoke } from "@tauri-apps/api/core";
import SvgImportDialog from "../components/dialogs/SvgImportDialog.svelte";
import { openDialog } from "./dialogs.svelte";
import { baseName } from "./documents.svelte";

export function isSvg(path: string): boolean {
  return /\.svgz?$/i.test(path);
}

/** The size to render `path` at, or undefined when the user cancelled. */
export async function askSvgSize(path: string): Promise<[number, number] | undefined> {
  const size = await invoke<[number, number]>("svg_size", { path });
  return openDialog(SvgImportDialog, { name: baseName(path), size });
}
