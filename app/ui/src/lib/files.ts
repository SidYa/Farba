// File operations with their dialogs: new, open, import as layer, save (OpenRaster / PNG /
// JPEG / WebP), close with the unsaved-changes prompt, quit.
//
// Save writes the format of the file name: OpenRaster (.ora, Farba's native format) keeps
// the layers; the others get a flat image (the visible layers composited; the document
// keeps its layers), like Pinta. Images with several layers or text default to .ora.
import { open, save } from "@tauri-apps/plugin-dialog";
import QualityDialog from "../components/dialogs/QualityDialog.svelte";
import NewImageDialog from "../components/dialogs/NewImageDialog.svelte";
import { choose, openDialog } from "./dialogs.svelte";
import { baseName, decodeFile, documents, errorText, type Doc } from "./documents.svelte";
import { sendShared } from "./ipc/shared";
import { saveLayered } from "./layered";
import { offerCodec, RAW_EXTENSIONS } from "./codecs";
import { importPdfLayers, isPdf, openPdf, pdfOptions, savePdf } from "./pdf";
import { askSvgSize, isSvg } from "./svg";
import { notify } from "./notify.svelte";
import { palette, type Rgba } from "./palette.svelte";
import { loadSetting, saveSetting } from "./settings";
import { wasmBytes } from "./wasm/core";

const OPEN_FILTERS = [
  {
    name: "Images",
    extensions: [
      "ora",
      "png",
      "jpg",
      "jpeg",
      "webp",
      "heic",
      "heif",
      "avif",
      "pdf",
      "psd",
      "psb",
      "xcf",
      "svg",
      "svgz",
      ...RAW_EXTENSIONS,
    ],
  },
  { name: "All files", extensions: ["*"] },
];

type Format = "ora" | "png" | "jpeg" | "webp" | "heic" | "avif" | "pdf";

const SAVE_FILTERS: { format: Format; name: string; extensions: string[] }[] = [
  { format: "ora", name: "OpenRaster image (layers)", extensions: ["ora"] },
  { format: "png", name: "PNG image", extensions: ["png"] },
  { format: "jpeg", name: "JPEG image", extensions: ["jpg", "jpeg"] },
  { format: "webp", name: "WebP image", extensions: ["webp"] },
  { format: "heic", name: "HEIC image", extensions: ["heic", "heif"] },
  { format: "avif", name: "AVIF image", extensions: ["avif"] },
  { format: "pdf", name: "PDF document", extensions: ["pdf"] },
];

const FORMATS: Record<string, Format> = {
  ora: "ora",
  png: "png",
  jpg: "jpeg",
  jpeg: "jpeg",
  webp: "webp",
  heic: "heic",
  heif: "heic",
  avif: "avif",
  pdf: "pdf",
};

function extOf(path: string): string {
  return /\.([^.\\/]+)$/.exec(path)?.[1]?.toLowerCase() ?? "";
}

/** Save dialog filters, the format of `ext` first (Windows preselects the first one). */
function saveFilters(ext: string) {
  const first = FORMATS[ext] ?? "png";
  return [...SAVE_FILTERS]
    .sort((a, b) => Number(b.format === first) - Number(a.format === first))
    .map(({ name, extensions }) => ({ name, extensions }));
}

function rgba8(c: Rgba): [number, number, number, number] {
  return [c.r, c.g, c.b, Math.round(c.a * 255)];
}

export async function newImage() {
  const r = await openDialog(NewImageDialog, {});
  if (!r) return;
  const fill: [number, number, number, number] =
    r.fill === "foreground"
      ? rgba8(palette.primary)
      : r.fill === "background"
        ? rgba8(palette.secondary)
        : r.fill === "white"
          ? [255, 255, 255, 255]
          : [0, 0, 0, 0];
  await documents.create(r.width, r.height, fill);
}

export async function openPaths(paths: string[]) {
  for (const path of paths) {
    try {
      if (isPdf(path)) await openPdf(path);
      else if (isSvg(path) && !documents.list.some((d) => d.path === path)) {
        const size = await askSvgSize(path);
        if (size) await documents.openPath(path, size);
      } else await documents.openPath(path);
    } catch (e) {
      if (await offerCodec(path, e)) continue;
      notify(`Could not open ${path}: ${errorText(e)}`, "error");
    }
  }
}

export async function openImages() {
  const picked = await open({ multiple: true, filters: OPEN_FILTERS });
  if (picked) await openPaths(picked);
}

/**
 * Adds image files as new layers of `doc` at their own size, centred (Layers > Import from
 * File, or files dropped on the canvas).
 */
export async function importLayers(doc: Doc, paths: string[]) {
  for (const path of paths) {
    if (isPdf(path)) {
      try {
        await importPdfLayers(doc, path);
      } catch (e) {
        notify(`Could not open ${path}: ${errorText(e)}`, "error");
      }
      continue;
    }
    let size: [number, number] | undefined;
    if (isSvg(path)) {
      try {
        size = await askSvgSize(path);
      } catch (e) {
        notify(`Could not open ${path}: ${errorText(e)}`, "error");
        continue;
      }
      if (!size) continue;
    }
    documents.busy = `Opening ${baseName(path)}…`;
    try {
      const img = await decodeFile(path, false, size);
      try {
        const x = Math.round((doc.width - img.width) / 2);
        const y = Math.round((doc.height - img.height) / 2);
        doc.run((c) =>
          c.add_layer_rgba("Import Layer", baseName(path), img.width, img.height, img.bytes, x, y),
        );
      } finally {
        img.release();
      }
    } catch (e) {
      if (!(await offerCodec(path, e))) notify(`Could not open ${path}: ${errorText(e)}`, "error");
    } finally {
      documents.busy = "";
    }
  }
}

export async function importLayerDialog(doc: Doc) {
  const picked = await open({ multiple: true, filters: OPEN_FILTERS });
  if (picked) await importLayers(doc, picked);
}

function stem(name: string): string {
  return name.replace(/\.[^.]+$/, "");
}

/**
 * Saves `doc`: to its file, or (when `ask` or never saved) to a file chosen in a dialog.
 * JPEG asks for the quality once per document (every time with Save As). Returns false if
 * cancelled or failed.
 */
export async function saveDocument(doc: Doc, ask: boolean): Promise<boolean> {
  let path = doc.path;
  // Files Farba only opens (PSD, XCF, RAW) are saved as another file: ask, proposing .ora
  // (or .png for a single plain layer) next to the original.
  const writable = path !== undefined && extOf(path) in FORMATS;
  if (ask || !writable) {
    const layered = doc.layers.length > 1 || doc.layers.some((l) => l.isText);
    const ext = layered ? "ora" : "png";
    const defaultPath =
      path === undefined ? `${stem(doc.name)}.${ext}` : writable ? path : `${stem(path)}.${ext}`;
    const picked = await save({
      title: "Save Image",
      defaultPath,
      filters: saveFilters(extOf(defaultPath)),
    });
    if (!picked) return false;
    path = picked;
  }
  if (path === undefined) return false;
  let ext = extOf(path);
  if (!(ext in FORMATS)) {
    path += ".png";
    ext = "png";
  }
  const format = FORMATS[ext]!;
  const name = baseName(path);

  let quality = 100;
  if (format === "jpeg" || format === "heic" || format === "avif") {
    let q = doc.quality[format];
    if (ask || path !== doc.path || q === undefined) {
      // JPEG starts at 100 for every image; HEIC / AVIF remember the last choice.
      const initial = q ?? (format === "jpeg" ? 100 : loadSetting(`${format}-quality`, 85));
      q = await openDialog(QualityDialog, {
        title: `Export as ${format.toUpperCase()}`,
        name,
        quality: initial,
      });
      if (q === undefined) return false;
      if (format !== "jpeg") saveSetting(`${format}-quality`, q);
      doc.quality[format] = q;
    }
    quality = q;
  }

  const pdf = format === "pdf" ? await pdfOptions(doc, name, ask || path !== doc.path) : undefined;
  if (format === "pdf" && !pdf) return false;

  documents.busy = `Saving ${name}…`;
  try {
    if (format === "ora") await saveLayered(doc, path);
    else if (pdf) await savePdf(doc, path, pdf);
    else await saveFlat(doc, path, format, quality);
    doc.path = path;
    doc.name = name;
    doc.core.mark_saved();
    doc.sync();
    notify(`Saved ${name}`);
    return true;
  } catch (e) {
    if (!(await offerCodec(path, e, true)))
      notify(`Could not save ${name}: ${errorText(e)}`, "error");
    return false;
  } finally {
    documents.busy = "";
  }
}

async function saveFlat(doc: Doc, path: string, format: Format, quality: number) {
  const { width, height } = doc;
  const len = width * height * 4;
  const ptr = doc.core.flatten();
  try {
    const bg = palette.secondary;
    await sendShared(len, (dst) => dst.set(wasmBytes(ptr, len)), "save_image", {
      path,
      width,
      height,
      format,
      quality,
      background: [bg.r, bg.g, bg.b],
    });
  } finally {
    doc.core.free_pixels();
  }
}

/** Closes a tab, asking to save unsaved changes first. Returns false if cancelled. */
export async function closeDocument(doc: Doc): Promise<boolean> {
  if (doc.dirty) {
    documents.active = doc;
    const r = await choose({
      title: "Unsaved changes",
      message: `Save the changes to “${doc.name}” before closing?`,
      buttons: [
        { label: "Don't Save", value: "discard" },
        { label: "Cancel", value: "cancel" },
        { label: "Save", value: "save", primary: true },
      ],
    });
    if (!r || r.value === "cancel") return false;
    if (r.value === "save" && !(await saveDocument(doc, false))) return false;
  }
  documents.close(doc);
  return true;
}

function unsavedCount(): number {
  return documents.list.filter((d) => d.dirty).length;
}

export async function closeAll() {
  const n = unsavedCount();
  const r = await choose({
    title: "Close All",
    message:
      n === 0
        ? "Close all tabs?"
        : `Close all tabs?\n${n === 1 ? "One image has" : `${n} images have`} unsaved changes that will be lost.`,
    buttons: [
      { label: "Cancel", value: "cancel" },
      { label: "Close All", value: "ok", primary: true, danger: n > 0 },
    ],
  });
  if (r?.value !== "ok") return;
  for (const d of [...documents.list]) documents.close(d);
}

/** Asks before quitting when there are unsaved changes. */
export async function confirmQuit(): Promise<boolean> {
  const n = unsavedCount();
  if (n === 0) return true;
  const r = await choose({
    title: "Quit Farba",
    message: `${n === 1 ? "One image has" : `${n} images have`} unsaved changes. Quit anyway?`,
    buttons: [
      { label: "Cancel", value: "cancel" },
      { label: "Quit", value: "quit", primary: true, danger: true },
    ],
  });
  return r?.value === "quit";
}
