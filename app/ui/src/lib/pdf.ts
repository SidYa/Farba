// PDF import and export (behaviour of GIMP's PDF plug-ins as in the owner's Pinta-plus).
// Import: password prompt, page selector (PdfImportDialog), the chosen pages rendered by
// the backend (Windows.Data.Pdf) one at a time, as layers of one image (the size of the
// largest page, every page at the top left, top layer active) or as separate images.
// Export: every layer is its own object (core/farba-io/src/pdf.rs).
import { invoke } from "@tauri-apps/api/core";
import PasswordDialog from "../components/dialogs/PasswordDialog.svelte";
import PdfExportDialog, {
  PDF_EXPORT_DEFAULTS,
  type PdfExportSettings,
} from "../components/dialogs/PdfExportDialog.svelte";
import PdfImportDialog, {
  type PdfImportResult,
} from "../components/dialogs/PdfImportDialog.svelte";
import { openDialog } from "./dialogs.svelte";
import { baseName, documents, type Doc } from "./documents.svelte";
import { invokeShared } from "./ipc/shared";
import { sendStack } from "./layered";
import { palette } from "./palette.svelte";
import { loadSetting, saveSetting } from "./settings";
import { Doc as CoreDoc, DocBuilder, loadCore } from "./wasm/core";

interface PdfInfo {
  session: number;
  /** Page sizes in points. */
  pages: [number, number][];
}

export function isPdf(path: string): boolean {
  return /\.pdf$/i.test(path);
}

const stem = (name: string) => name.replace(/\.[^.]+$/, "");

/** Opens the PDF, asking for the password while it is wrong; undefined if cancelled. */
async function openSession(path: string): Promise<PdfInfo | undefined> {
  let password: string | undefined;
  for (;;) {
    try {
      return await invoke<PdfInfo>("pdf_open", { path, password });
    } catch (e) {
      const code = String(e);
      if (code !== "password-required" && code !== "wrong-password") throw e;
      password = await openDialog(PasswordDialog, {
        name: baseName(path),
        retry: code === "wrong-password",
      });
      if (password === undefined) return undefined;
    }
  }
}

/** Asks which pages, renders them and hands each to `use` (in the order to add them). */
async function withPages(
  path: string,
  layersOnly: boolean,
  use: (
    opts: PdfImportResult,
    page: number,
    img: { width: number; height: number; bytes: Uint8Array },
  ) => void,
): Promise<PdfImportResult | undefined> {
  const name = baseName(path);
  const info = await openSession(path);
  if (!info) return undefined;
  try {
    const opts = await openDialog(PdfImportDialog, {
      name,
      session: info.session,
      pages: info.pages,
      layersOnly,
    });
    if (!opts) return undefined;
    await loadCore();
    const order = opts.reverse ? [...opts.pages].reverse() : opts.pages;
    for (const [k, page] of order.entries()) {
      documents.busy = `Rendering page ${page + 1} (${k + 1} of ${order.length})…`;
      const [w, h] = info.pages[page]!;
      const r = await invokeShared<{ width: number; height: number }>("pdf_render", {
        session: info.session,
        page,
        width: Math.ceil((w * opts.resolution) / 72 - 1e-6),
        height: Math.ceil((h * opts.resolution) / 72 - 1e-6),
        white: opts.white,
      });
      try {
        use(opts, page, { ...r.header, bytes: new Uint8Array(r.buffer) });
      } finally {
        r.release();
      }
    }
    return opts;
  } finally {
    documents.busy = "";
    void invoke("pdf_close", { session: info.session });
  }
}

/** File > Open of a PDF. */
export async function openPdf(path: string) {
  const name = baseName(path);
  let builder: DocBuilder | undefined;
  let size = [0, 0];
  try {
    const opts = await withPages(path, false, (o, page, img) => {
      if (o.target === "images") {
        documents.adopt(
          `${stem(name)}-${page + 1}`,
          CoreDoc.from_rgba(img.width, img.height, img.bytes, `Page ${page + 1}`),
        );
        return;
      }
      // The builder needs the final size: grow it as pages come (pages sit at 0, 0).
      size = [Math.max(size[0]!, img.width), Math.max(size[1]!, img.height)];
      builder ??= new DocBuilder(1, 1);
      builder.add_layer(
        `Page ${page + 1}`,
        img.width,
        img.height,
        img.bytes,
        0,
        0,
        1,
        true,
        false,
        false,
        undefined,
        false,
      );
    });
    if (!opts || opts.target !== "layers" || !builder) return;
    const b = builder;
    builder = undefined;
    b.set_size(size[0]!, size[1]!);
    const doc = documents.adopt(name, b.build(opts.pages.length - 1), path);
    // Saving back proposes the same pages at the same size (Pinta-plus).
    doc.pdfDefaults = { resolution: opts.resolution, layersAsPages: opts.pages.length > 1 };
  } finally {
    builder?.free();
  }
}

/** Layers > Import from File (or a drop on the canvas) of a PDF: pages become layers. */
export async function importPdfLayers(doc: Doc, path: string) {
  await withPages(path, true, (_o, page, img) => {
    const x = Math.round((doc.width - img.width) / 2);
    const y = Math.round((doc.height - img.height) / 2);
    doc.run((c) =>
      c.add_layer_rgba("Import Layer", `Page ${page + 1}`, img.width, img.height, img.bytes, x, y),
    );
  });
}

/**
 * The export options: asked on the first PDF save of `doc` in this session (always with
 * Save As or a new file), then reused. Undefined if cancelled.
 */
export async function pdfOptions(
  doc: Doc,
  name: string,
  ask: boolean,
): Promise<PdfExportSettings | undefined> {
  if (!ask && doc.pdfExport) return doc.pdfExport;
  const initial = {
    ...PDF_EXPORT_DEFAULTS,
    ...loadSetting<Partial<PdfExportSettings>>("pdf-export", {}),
    ...doc.pdfDefaults,
    ...doc.pdfExport,
  };
  const o = await openDialog(PdfExportDialog, {
    name,
    width: doc.width,
    height: doc.height,
    layers: doc.layers.length,
    initial,
  });
  if (!o) return undefined;
  saveSetting("pdf-export", o);
  doc.pdfExport = o;
  return o;
}

export async function savePdf(doc: Doc, path: string, o: PdfExportSettings) {
  const bg = palette.secondary;
  await sendStack(doc, false, "save_pdf", {
    path,
    options: {
      resolution: o.resolution,
      vectorize: o.vectorize,
      ignoreHidden: o.ignoreHidden,
      background: o.fillBackground ? [bg.r, bg.g, bg.b] : null,
      layersAsPages: o.layersAsPages,
      reverseOrder: o.reverseOrder,
      noInterpolation: o.noInterpolation,
    },
  });
}
