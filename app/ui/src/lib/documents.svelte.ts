// Open documents (image tabs). The pixels, layers and undo history live in WASM (`Doc` from
// farba-wasm, the source of truth); this is the UI-side mirror: name, file, view (zoom/pan),
// the layer list and the Layers pad selection. Call `sync()` after changing the WASM doc;
// `run()` does it for you.
import { invokeShared } from "./ipc/shared";
import { buildDoc, type Stack } from "./layered";
import type { PdfExportSettings } from "../components/dialogs/PdfExportDialog.svelte";
import { notify } from "./notify.svelte";
import { Doc as CoreDoc, loadCore } from "./wasm/core";
import { prefs } from "./prefs.svelte";
import type { View } from "./canvas/renderer";
import type { Guide } from "./guides.svelte";

export interface LayerInfo {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  tilesX: number;
  opacity: number;
  visible: boolean;
  lockAlpha: boolean;
  lockPosition: boolean;
  /** A text layer (its text can be edited with the Text tool). */
  isText: boolean;
}

export type LayerProps = Pick<
  LayerInfo,
  "name" | "opacity" | "visible" | "lockAlpha" | "lockPosition" | "x" | "y"
>;

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

let nextId = 1;

export class Doc {
  readonly id = nextId++;
  name = $state("");
  /** The file Save writes to (opened or last saved); undefined until saved. */
  path = $state<string | undefined>();
  /** Quality per lossy format chosen in this session (asked once, then reused by Save). */
  quality: Partial<Record<"jpeg" | "heic" | "avif", number>> = {};
  /** PDF export options confirmed in this session (asked once, then reused by Save). */
  pdfExport: PdfExportSettings | undefined;
  /** Options to propose for the first PDF save (from the PDF this image was opened from). */
  pdfDefaults: Partial<PdfExportSettings> | undefined;
  width = $state(0);
  height = $state(0);
  readonly core: CoreDoc;
  /** Raw: replaced as a whole on every pan/zoom. */
  view = $state.raw<View>({ zoom: 1, x: 0, y: 0 });
  /** True until the first fit-to-window (needs the viewport size). */
  needsFit = true;
  /** Bottom to top, as in the core. */
  layers = $state.raw<LayerInfo[]>([]);
  /** Id of the active layer (painting target). */
  active = $state(0);
  /** Layers selected in the pad (always contains the active one). */
  selected = $state.raw<ReadonlySet<number>>(new Set());
  undoLabel = $state<string | undefined>();
  redoLabel = $state<string | undefined>();
  /** Guides (lib/guides.svelte.ts): view state, not undo steps. */
  guides = $state.raw<Guide[]>([]);
  /** Bounding box `[x, y, width, height]` of the image selection, or undefined. */
  selection = $state.raw<[number, number, number, number] | undefined>();
  dirty = $state(false);
  /** Bumped by every sync: the viewport re-uploads changed tiles, the pad redraws thumbnails. */
  revision = $state(0);

  constructor(name: string, core: CoreDoc, path?: string) {
    this.name = name;
    this.path = path;
    this.core = core;
    this.applyUndoLimits();
    this.sync();
  }

  /** Settings > Behaviour > Undo history (the core's default: 512 MB / 200 steps). */
  applyUndoLimits() {
    this.core.set_undo_limits(prefs.undo.megabytes, prefs.undo.steps);
  }

  /** Re-reads everything from the WASM document. */
  sync() {
    const c = this.core;
    this.width = c.width;
    this.height = c.height;
    this.layers = c.layers() as LayerInfo[];
    this.active = c.active;
    const ids = new Set(this.layers.map((l) => l.id));
    const kept = [...this.selected].filter((id) => ids.has(id));
    this.selected = new Set(kept.includes(this.active) ? kept : [this.active]);
    this.undoLabel = c.undo_label();
    this.redoLabel = c.redo_label();
    this.dirty = c.dirty;
    const b = c.selection_bounds();
    this.selection = b ? [b[0]!, b[1]!, b[2]!, b[3]!] : undefined;
    this.revision++;
  }

  /** Runs a change on the WASM document, reports a failure in the status bar, then syncs. */
  run<T>(f: (core: CoreDoc) => T): T | undefined {
    try {
      return f(this.core);
    } catch (e) {
      notify(errorText(e), "error");
      return undefined;
    } finally {
      this.sync();
    }
  }

  layer(id: number): LayerInfo | undefined {
    return this.layers.find((l) => l.id === id);
  }

  get activeLayer(): LayerInfo | undefined {
    return this.layer(this.active);
  }

  /** Selected layer ids, bottom to top. */
  get selectedIds(): number[] {
    return this.layers.filter((l) => this.selected.has(l.id)).map((l) => l.id);
  }

  /** Pad selection: a plain click, Ctrl+click (toggle) or Shift+click (range). */
  select(id: number, mode: "replace" | "toggle" | "range" = "replace") {
    let active = id;
    let selected: Set<number>;
    if (mode === "toggle") {
      selected = new Set(this.selected);
      if (selected.has(id) && selected.size > 1) {
        selected.delete(id);
        if (id === this.active) active = this.selectedIds.filter((x) => x !== id).pop()!;
        else active = this.active;
      } else selected.add(id);
    } else if (mode === "range") {
      const a = this.layers.findIndex((l) => l.id === this.active);
      const b = this.layers.findIndex((l) => l.id === id);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      selected = new Set(this.layers.slice(lo, hi + 1).map((l) => l.id));
    } else selected = new Set([id]);
    this.core.set_active(active);
    this.active = active;
    this.selected = selected;
  }

  selectAll() {
    this.selected = new Set(this.layers.map((l) => l.id));
  }

  setProps(id: number, patch: Partial<LayerProps>, label: string) {
    const l = this.layer(id);
    if (!l) return;
    const p = { ...l, ...patch };
    this.run((c) =>
      c.set_layer_props(
        id,
        p.name,
        p.opacity,
        p.visible,
        p.lockAlpha,
        p.lockPosition,
        p.x,
        p.y,
        label,
      ),
    );
  }

  undo() {
    this.run((c) => c.undo());
  }

  redo() {
    this.run((c) => c.redo());
  }
}

/** "Untitled", "Untitled 2", ... */
function untitledName(list: Doc[]): string {
  for (let n = 1; ; n++) {
    const name = n === 1 ? "Untitled" : `Untitled ${n}`;
    if (!list.some((d) => d.name === name)) return name;
  }
}

/**
 * Straight RGBA8 pixels decoded by the backend; call `release` when copied. A layered file
 * (.ora) comes as its composite image, unless `layered`: then `stack` describes the layers
 * and `bytes` holds them all (see layered.ts). `size`: the size to render an SVG at.
 */
export async function decodeFile(path: string, layered = false, size?: [number, number]) {
  const r = await invokeShared<{ width: number; height: number; stack?: Stack }>("open_image", {
    path,
    flatten: !layered,
    size: size ?? null,
  });
  return { ...r.header, bytes: new Uint8Array(r.buffer), release: r.release };
}

class Documents {
  list = $state.raw<Doc[]>([]);
  active = $state.raw<Doc | undefined>();
  /** Status line text for long operations ("Opening …"), or "". */
  busy = $state("");
  /** While above 0, input is held back (`whileBusy` in native.ts): a result is on its way. */
  blocking = $state(0);

  private add(doc: Doc) {
    this.list = [...this.list, doc];
    this.active = doc;
  }

  /** Opens a file in a new tab (`size`: the size to render an SVG at). */
  async openPath(path: string, size?: [number, number]) {
    const name = baseName(path);
    const existing = this.list.find((d) => d.path === path);
    if (existing) {
      this.active = existing;
      return;
    }
    this.busy = `Opening ${name}…`;
    try {
      await loadCore();
      const img = await decodeFile(path, true, size);
      let core: CoreDoc;
      try {
        core = img.stack
          ? buildDoc(img.stack, img.bytes)
          : CoreDoc.from_rgba(img.width, img.height, img.bytes, name);
      } finally {
        img.release();
      }
      this.add(new Doc(name, core, path));
      const notes = img.stack?.notes ?? [];
      if (notes.length) notify(`${name}: ${notes.join("; ")}`);
    } finally {
      this.busy = "";
    }
  }

  /** A new image with one layer filled with `fill` (straight RGBA, alpha 0..255). */
  async create(width: number, height: number, fill: [number, number, number, number]) {
    await loadCore();
    this.add(new Doc(untitledName(this.list), new CoreDoc(width, height, ...fill)));
  }

  /** Adds a document built elsewhere (PDF pages) as a new tab. */
  adopt(name: string, core: CoreDoc, path?: string): Doc {
    const doc = new Doc(name, core, path);
    this.add(doc);
    return doc;
  }

  /** A new image from pixels (paste as new image). */
  async createFrom(name: string, width: number, height: number, rgba: Uint8Array) {
    await loadCore();
    this.add(new Doc(name, CoreDoc.from_rgba(width, height, rgba, name)));
  }

  /** Closes without asking (see files.ts `closeDocument` for the unsaved-changes prompt). */
  close(doc: Doc) {
    const i = this.list.indexOf(doc);
    if (i < 0) return;
    this.list = this.list.filter((d) => d !== doc);
    if (this.active === doc) this.active = this.list[Math.min(i, this.list.length - 1)];
    onClose.forEach((f) => f(doc));
    doc.core.free();
  }
}

const onClose = new Set<(doc: Doc) => void>();
/** Subscribes to document closing (e.g. to free GPU textures). */
export function onDocumentClosed(f: (doc: Doc) => void): () => void {
  onClose.add(f);
  return () => onClose.delete(f);
}

export const documents = new Documents();
