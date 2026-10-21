// All application actions with GIMP's default shortcuts (plus the owner's custom ones, see
// CLAUDE.md). Actions that are not implemented yet stay visible but disabled, so menus show
// the final structure.
import { getCurrentWindow } from "@tauri-apps/api/window";
import AboutDialog from "../components/dialogs/AboutDialog.svelte";
import AddonsManagerDialog from "../components/dialogs/AddonsManagerDialog.svelte";
import AdjustDialog, { type AdjustSpec } from "../components/dialogs/AdjustDialog.svelte";
import CanvasSizeDialog from "../components/dialogs/CanvasSizeDialog.svelte";
import LayerPropertiesDialog from "../components/dialogs/LayerPropertiesDialog.svelte";
import RotateZoomDialog from "../components/dialogs/RotateZoomDialog.svelte";
import ScaleImageDialog from "../components/dialogs/ScaleImageDialog.svelte";
import SettingsDialog from "../components/dialogs/SettingsDialog.svelte";
import { defineActions, type Action } from "./actions";
import { copyLayer, paste, pasteAsNewImage } from "./clipboard";
import { choose, openDialog } from "./dialogs.svelte";
import { documents, errorText, type Doc } from "./documents.svelte";
import * as native from "./native";
import {
  closeAll,
  closeDocument,
  importLayerDialog,
  newImage,
  openImages,
  saveDocument,
} from "./files";
import { notify } from "./notify.svelte";
import { palette } from "./palette.svelte";
import { prefs } from "./prefs.svelte";
import { guidePrefs } from "./guides.svelte";
import { confirmRasterize } from "./textguard";
import { theme } from "./theme.svelte";
import { TOOLS, tools } from "./tools.svelte";
import { fit, zoomActive, zoomStep } from "./view.svelte";

const hasDoc = () => !!documents.active;

/** An action on the active document (disabled without one). */
function docAction(
  id: string,
  label: string,
  run: (doc: Doc) => unknown,
  opts: { keys?: string[]; icon?: string; enabled?: (doc: Doc) => boolean } = {},
): Action {
  return {
    id,
    label,
    keys: opts.keys,
    icon: opts.icon,
    enabled: () => !!documents.active && (opts.enabled?.(documents.active) ?? true),
    run: () => documents.active && run(documents.active),
  };
}

function layerIndex(doc: Doc): number {
  return doc.layers.findIndex((l) => l.id === doc.active);
}

/** Deletes the selected layers, asking first unless "Don't ask again" was ticked. */
export async function deleteLayers(doc: Doc) {
  const ids = doc.selectedIds;
  if (ids.length >= doc.layers.length) {
    notify("An image must keep at least one layer", "error");
    return;
  }
  if (prefs.askDeleteLayer) {
    const what =
      ids.length === 1 ? `the layer “${doc.layer(ids[0]!)?.name}”` : `${ids.length} layers`;
    const r = await choose({
      title: ids.length === 1 ? "Delete Layer" : "Delete Layers",
      message: `Delete ${what}?`,
      checkbox: "Don't ask again",
      buttons: [
        { label: "Cancel", value: "cancel" },
        { label: "Delete", value: "delete", primary: true, danger: true },
      ],
    });
    if (r?.value !== "delete") return;
    if (r.checked) prefs.set("askDeleteLayer", false);
  }
  doc.run((c) => c.delete_layers(Uint32Array.from(ids)));
}

async function scaleImage(doc: Doc) {
  const r = await openDialog(ScaleImageDialog, { width: doc.width, height: doc.height });
  if (!r) return;
  // Large images are scaled by the backend on all cores; small ones by the WASM core.
  await native.whileBusy("Scaling the image…", async () => {
    try {
      if (await native.scaleImage(doc, r.width, r.height, r.interpolation)) return;
    } catch (e) {
      notify(errorText(e), "error");
      return;
    }
    doc.run((c) => c.scale_image(r.width, r.height, r.interpolation));
  });
}

async function canvasSize(doc: Doc) {
  const r = await openDialog(CanvasSizeDialog, { width: doc.width, height: doc.height });
  if (!r) return;
  doc.run((c) => c.resize_canvas(r.width, r.height, r.x, r.y, r.resize));
  // Keep the old content where it was on screen.
  const v = doc.view;
  doc.view = { ...v, x: Math.round(v.x - r.x * v.zoom), y: Math.round(v.y - r.y * v.zoom) };
}

// Adjustments (GIMP's ranges and defaults; see filters.rs for the maths).
const EXPOSURE: AdjustSpec = {
  id: "exposure",
  title: "Exposure",
  kind: 1,
  sliders: [
    { key: "black", label: "Black level", min: -0.1, max: 0.1, step: 0.001, default: 0 },
    { key: "exposure", label: "Exposure", min: -10, max: 10, step: 0.01, default: 0 },
  ],
  params: (v) => [v.exposure!, v.black!],
};
const SATURATION: AdjustSpec = {
  id: "saturation",
  title: "Saturation",
  kind: 2,
  sliders: [{ key: "scale", label: "Scale", min: 0, max: 10, step: 0.01, default: 1 }],
  params: (v) => [v.scale!, 0],
};
const DESATURATE: AdjustSpec = {
  id: "desaturate",
  title: "Desaturate",
  kind: 3,
  select: {
    key: "mode",
    label: "Mode",
    choices: [
      [0, "Luminance"],
      [1, "Luma"],
      [2, "Lightness (HSL)"],
      [3, "Average (HSI Intensity)"],
      [4, "Value (HSV)"],
    ],
    default: 0,
  },
  params: (v) => [v.mode!, 0],
};

// Effects (Effects menu; see effects.rs for the maths).
const slider = (key: string, label: string, min: number, max: number, def: number, step = 1) => ({
  key,
  label,
  min,
  max,
  step,
  default: def,
});
const EFFECTS: { id: string; icon?: string; spec: AdjustSpec }[] = [
  {
    id: "effects-black-white",
    icon: "filter_b_and_w",
    spec: {
      id: "black-white",
      title: "Black & White",
      kind: 10,
      sliders: [
        slider("red", "Reds", -100, 200, 30),
        slider("green", "Greens", -100, 200, 59),
        slider("blue", "Blues", -100, 200, 11),
        slider("contrast", "Contrast", -100, 100, 0),
        slider("sepia", "Sepia tone", 0, 100, 0),
      ],
      params: (v) => [v.red!, v.green!, v.blue!, v.contrast!, v.sepia!],
    },
  },
  {
    id: "effects-enhance-colors",
    icon: "palette",
    spec: {
      id: "enhance-colors",
      title: "Enhance Colors",
      kind: 11,
      sliders: [
        slider("auto", "Auto levels", 0, 100, 100),
        slider("vibrance", "Vibrance", 0, 100, 30),
      ],
      params: (v) => [v.auto!, v.vibrance!],
    },
  },
  {
    id: "effects-retouch",
    icon: "healing",
    spec: {
      id: "retouch",
      title: "Retouch (Smooth Skin)",
      kind: 12,
      sliders: [
        slider("radius", "Radius", 1, 50, 8),
        slider("smoothness", "Smoothness", 1, 100, 40),
        slider("amount", "Amount", 0, 100, 100),
      ],
      params: (v) => [v.radius!, v.smoothness!, v.amount!],
    },
  },
  {
    id: "effects-reduce-noise",
    icon: "grain",
    spec: {
      id: "reduce-noise",
      title: "Reduce Noise",
      kind: 13,
      sliders: [slider("radius", "Radius", 1, 10, 2), slider("strength", "Strength", 1, 100, 30)],
      params: (v) => [v.radius!, v.strength!],
    },
  },
  {
    id: "effects-sharpen",
    icon: "center_focus_strong",
    spec: {
      id: "sharpen",
      title: "Sharpen (Unsharp Mask)",
      kind: 14,
      sliders: [
        slider("radius", "Radius", 0.1, 20, 2, 0.1),
        slider("amount", "Amount", 0, 500, 100),
        slider("threshold", "Threshold", 0, 255, 0),
      ],
      params: (v) => [v.radius!, v.amount!, v.threshold!],
    },
  },
  {
    id: "effects-gaussian-blur",
    icon: "blur_on",
    spec: {
      id: "gaussian-blur",
      title: "Gaussian Blur",
      kind: 15,
      sliders: [slider("sigma", "Size", 0.5, 150, 5, 0.5)],
      params: (v) => [v.sigma!],
    },
  },
  {
    id: "effects-vignette",
    icon: "vignette",
    spec: {
      id: "vignette",
      title: "Vignette",
      kind: 16,
      sliders: [
        slider("amount", "Amount", -100, 100, -50),
        slider("size", "Size", 0, 100, 50),
        slider("softness", "Softness", 0, 100, 50),
      ],
      params: (v) => [v.amount!, v.size!, v.softness!],
    },
  },
];

/** Effects menu items (built-in effects; add-ons add theirs below). */
export const EFFECT_ACTIONS = EFFECTS.map((e) => e.id);

async function adjust(doc: Doc, spec: AdjustSpec) {
  if (await confirmRasterize(doc)) void openDialog(AdjustDialog, { doc, spec });
}

/** Wraps an action that changes the active layer's pixels: a text layer asks first. */
function pixels(run: (doc: Doc) => unknown): (doc: Doc) => Promise<unknown> {
  return async (doc) => ((await confirmRasterize(doc)) ? run(doc) : undefined);
}

/** Changes a slider option of the active tool by `delta` and shows the new value (keys,
 * Ctrl+Alt+wheel). */
export function adjustToolOption(key: string, delta: number) {
  const o = tools.def.options.find((x) => x.key === key);
  if (o?.type !== "slider") return;
  const v = Math.min(o.max, Math.max(o.min, tools.option<number>(tools.active, key) + delta));
  tools.setOption(tools.active, key, v);
  notify(`${tools.def.name} ${o.label.toLowerCase()}: ${v}${o.unit ? ` ${o.unit}` : ""}`);
}

/** Crops to the selection, keeping the kept part where it was on screen. */
function cropToSelection(doc: Doc) {
  const sel = doc.selection;
  if (!sel) return;
  doc.run((c) => c.crop_to_selection());
  if (doc.width === sel[2] && doc.height === sel[3]) {
    const v = doc.view;
    doc.view = { ...v, x: Math.round(v.x + sel[0] * v.zoom), y: Math.round(v.y + sel[1] * v.zoom) };
  }
}

export async function layerProperties(doc: Doc, id = doc.active) {
  const layer = doc.layer(id);
  if (!layer) return;
  const p = await openDialog(LayerPropertiesDialog, { doc, layer });
  if (p) doc.setProps(id, p, "Layer Properties");
}

export function registerCommands() {
  defineActions([
    // File
    { id: "file-new", label: "New…", icon: "note_add", keys: ["Ctrl+N"], run: newImage },
    { id: "file-open", label: "Open…", icon: "file_open", keys: ["Ctrl+O"], run: openImages },
    // Save writes PNG / JPEG / WebP (GIMP's Export), so GIMP's export keys do the same.
    docAction("file-save", "Save", (d) => saveDocument(d, false), {
      keys: ["Ctrl+S", "Ctrl+E"],
      icon: "save",
    }),
    docAction("file-save-as", "Save As…", (d) => saveDocument(d, true), {
      keys: ["Ctrl+Shift+S", "Ctrl+Shift+E"],
      icon: "save_as",
    }),
    docAction("file-close", "Close", (d) => closeDocument(d), { keys: ["Ctrl+W"] }),
    {
      id: "file-close-all",
      label: "Close All",
      icon: "cancel",
      keys: ["Ctrl+Shift+W"],
      enabled: hasDoc,
      run: closeAll,
    },
    {
      id: "app-settings",
      label: "Settings…",
      icon: "settings",
      run: () => openDialog(SettingsDialog, {}),
    },
    {
      id: "app-addons",
      label: "Addons Manager…",
      icon: "extension",
      run: () => openDialog(AddonsManagerDialog, {}),
    },
    {
      id: "app-about",
      label: "About Farba…",
      icon: "info",
      run: () => openDialog(AboutDialog, {}),
    },
    { id: "file-quit", label: "Quit", keys: ["Ctrl+Q"], run: () => getCurrentWindow().close() },

    // Edit
    docAction("edit-undo", "Undo", (d) => d.undo(), {
      keys: ["Ctrl+Z"],
      icon: "undo",
      enabled: (d) => !!d.undoLabel,
    }),
    docAction("edit-redo", "Redo", (d) => d.redo(), {
      keys: ["Ctrl+Y", "Ctrl+Shift+Z"],
      icon: "redo",
      enabled: (d) => !!d.redoLabel,
    }),
    docAction(
      "edit-cut",
      "Cut",
      pixels((d) => copyLayer(d, true)),
      {
        keys: ["Ctrl+X"],
        icon: "content_cut",
      },
    ),
    docAction("edit-copy", "Copy", (d) => copyLayer(d, false), {
      keys: ["Ctrl+C"],
      icon: "content_copy",
    }),
    {
      id: "edit-paste",
      label: "Paste",
      icon: "content_paste",
      keys: ["Ctrl+V"],
      // Without an open image, paste opens the clipboard as a new image.
      run: () => (documents.active ? paste(documents.active) : pasteAsNewImage()),
    },
    {
      id: "edit-paste-as-new",
      label: "Paste as New Image",
      keys: ["Ctrl+Shift+V"],
      run: pasteAsNewImage,
    },
    docAction(
      "edit-clear",
      "Clear",
      pixels((d) => d.run((c) => c.clear_layer(d.active, "Clear"))),
      {
        keys: ["Delete"],
      },
    ),

    // Select
    docAction("select-all", "Select All", (d) => d.run((c) => c.select_all()), {
      keys: ["Ctrl+A"],
      icon: "select",
    }),
    docAction("select-none", "Select None", (d) => d.run((c) => c.select_none()), {
      keys: ["Ctrl+Shift+A"],
      icon: "deselect",
      enabled: (d) => !!d.selection,
    }),
    docAction("select-invert", "Invert Selection", (d) => d.run((c) => c.invert_selection()), {
      keys: ["Ctrl+I"],
    }),
    docAction("image-crop-to-selection", "Crop to Selection", cropToSelection, {
      icon: "crop",
      enabled: (d) => !!d.selection,
    }),

    // View
    {
      id: "view-zoom-in",
      label: "Zoom In",
      icon: "zoom_in",
      keys: ["=", "KP_Add"],
      enabled: hasDoc,
      run: () => zoomStep(documents.active!, 1),
    },
    {
      id: "view-zoom-out",
      label: "Zoom Out",
      icon: "zoom_out",
      keys: ["-", "KP_Subtract"],
      enabled: hasDoc,
      run: () => zoomStep(documents.active!, -1),
    },
    {
      id: "view-zoom-fit",
      label: "Fit Image in Window",
      icon: "fit_screen",
      keys: ["Ctrl+Shift+J"],
      enabled: hasDoc,
      run: () => fit(documents.active!),
    },
    ...(
      [
        ["16-1", "16:1 (1600%)", 16, ["5"]],
        ["8-1", "8:1 (800%)", 8, ["4"]],
        ["4-1", "4:1 (400%)", 4, ["3"]],
        ["2-1", "2:1 (200%)", 2, ["2"]],
        ["1-1", "1:1 (100%)", 1, ["1", "KP_1"]],
        ["1-2", "1:2 (50%)", 1 / 2, ["Shift+2"]],
        ["1-4", "1:4 (25%)", 1 / 4, ["Shift+KP_3", "Shift+3"]],
        ["1-8", "1:8 (12.5%)", 1 / 8, ["Shift+KP_4", "Shift+4"]],
        ["1-16", "1:16 (6.25%)", 1 / 16, ["Shift+KP_5", "Shift+5"]],
      ] as const
    ).map(([id, label, zoom, keys]): Action => ({
      id: `view-zoom-${id}`,
      label,
      keys: [...keys],
      enabled: hasDoc,
      run: () => zoomActive(zoom),
    })),
    {
      id: "view-theme-system",
      label: "System",
      icon: "brightness_auto",
      run: () => theme.set("system"),
    },
    { id: "view-theme-light", label: "Light", icon: "light_mode", run: () => theme.set("light") },
    { id: "view-theme-dark", label: "Dark", icon: "dark_mode", run: () => theme.set("dark") },

    // Layers
    docAction("layer-new", "New Layer", (d) => d.run((c) => c.add_layer(0, 0, 0, 0)), {
      keys: ["Ctrl+Shift+N"],
      icon: "add",
    }),
    docAction(
      "layer-duplicate",
      "Duplicate Layer",
      (d) => d.run((c) => c.duplicate_layer(d.active)),
      {
        keys: ["Ctrl+Shift+D"],
        icon: "content_copy",
      },
    ),
    docAction("layer-delete", "Delete Layer", deleteLayers, {
      icon: "delete",
      enabled: (d) => d.selected.size < d.layers.length,
    }),
    docAction("layer-merge-down", "Merge Down", (d) => d.run((c) => c.merge_down(d.active)), {
      icon: "merge",
      enabled: (d) => layerIndex(d) > 0,
    }),
    docAction(
      "layer-merge-selected",
      "Merge Selected Layers",
      (d) => d.run((c) => c.merge_layers(Uint32Array.from(d.selectedIds))),
      { icon: "view_apps", enabled: (d) => d.selected.size > 1 },
    ),
    docAction(
      "layer-merge-visible",
      "Merge Visible Layers",
      (d) => d.run((c) => c.merge_visible()),
      {
        icon: "preview",
        keys: ["Ctrl+M"],
        enabled: (d) => d.layers.filter((l) => l.visible).length > 1,
      },
    ),
    docAction("layer-raise", "Raise Layer", (d) => d.run((c) => c.raise_layer(d.active)), {
      icon: "arrow_upward",
      enabled: (d) => layerIndex(d) < d.layers.length - 1,
    }),
    docAction("layer-lower", "Lower Layer", (d) => d.run((c) => c.lower_layer(d.active)), {
      icon: "arrow_downward",
      enabled: (d) => layerIndex(d) > 0,
    }),
    docAction("layer-properties", "Layer Properties…", (d) => layerProperties(d), {
      icon: "tune",
    }),
    docAction("layer-import", "Import from File…", importLayerDialog, { icon: "file_open" }),
    docAction("layer-select-all", "Select All Layers", (d) => d.selectAll()),
    docAction(
      "layer-flip-h",
      "Flip Horizontally",
      pixels((d) => d.run((c) => c.flip_layer(d.active, true))),
      {
        icon: "flip",
      },
    ),
    docAction(
      "layer-flip-v",
      "Flip Vertically",
      pixels((d) => d.run((c) => c.flip_layer(d.active, false))),
      { icon: "flip_vertical" },
    ),
    docAction(
      "layer-to-image-size",
      "Layer to Image Size",
      (d) => d.run((c) => c.layer_to_image_size(d.active)),
      { icon: "aspect_ratio" },
    ),
    docAction(
      "layer-rotate",
      "Rotate / Zoom…",
      pixels((d) => openDialog(RotateZoomDialog, { doc: d })),
      {
        icon: "rotate_right",
      },
    ),

    // Image
    docAction("image-scale", "Scale Image…", scaleImage, { icon: "photo_size_select_large" }),
    docAction("image-canvas-size", "Canvas Size…", canvasSize, { icon: "aspect_ratio" }),
    docAction("image-flip-h", "Flip Horizontally", (d) => d.run((c) => c.flip_image(true)), {
      icon: "flip",
    }),
    docAction("image-flip-v", "Flip Vertically", (d) => d.run((c) => c.flip_image(false)), {
      icon: "flip_vertical",
    }),
    docAction("image-rotate-90", "Rotate 90° Clockwise", (d) => d.run((c) => c.rotate_image(1)), {
      icon: "rotate_right",
    }),
    docAction(
      "image-rotate-270",
      "Rotate 90° Counter-clockwise",
      (d) => d.run((c) => c.rotate_image(3)),
      { icon: "rotate_left" },
    ),
    docAction("image-rotate-180", "Rotate 180°", (d) => d.run((c) => c.rotate_image(2)), {
      icon: "cached",
    }),
    docAction("image-guides-remove", "Remove All Guides", (d) => (d.guides = []), {
      enabled: (d) => guidePrefs.show && d.guides.length > 0,
    }),

    // Colours (adjustments on the active layer, limited to the selection)
    docAction("colors-exposure", "Exposure…", (d) => adjust(d, EXPOSURE), { icon: "exposure" }),
    docAction("colors-saturation", "Saturation…", (d) => adjust(d, SATURATION), {
      icon: "contrast",
    }),
    docAction("colors-desaturate", "Desaturate…", (d) => adjust(d, DESATURATE), {
      icon: "format_color_reset",
    }),
    // Effects (the Effects menu)
    ...EFFECTS.map((e) =>
      docAction(e.id, `${e.spec.title}…`, (d) => adjust(d, e.spec), { icon: e.icon }),
    ),
    docAction(
      "colors-invert",
      "Invert",
      pixels((d) => d.run((c) => c.apply_filter("Invert", 0, new Float32Array()))),
      {
        icon: "invert_colors",
      },
    ),

    // Colours
    { id: "colors-swap", label: "Swap Colours", keys: ["X"], run: () => palette.swapWithRecents() },
    { id: "colors-default", label: "Default Colours", keys: ["D"], run: () => palette.reset() },

    // Tool size / opacity (GIMP's tools-size-* / tools-opacity-* keys)
    ...(
      [
        ["tools-size-decrease", "Tool's Size: Decrease by 1", "size", -1, ["["]],
        ["tools-size-increase", "Tool's Size: Increase by 1", "size", 1, ["]"]],
        ["tools-size-decrease-skip", "Tool's Size: Decrease by 10", "size", -10, ["Shift+["]],
        ["tools-size-increase-skip", "Tool's Size: Increase by 10", "size", 10, ["Shift+]"]],
        ["tools-opacity-decrease", "Tool's Opacity: Decrease by 1", "opacity", -1, ["Shift+,"]],
        ["tools-opacity-increase", "Tool's Opacity: Increase by 1", "opacity", 1, ["Shift+."]],
        [
          "tools-opacity-decrease-skip",
          "Tool's Opacity: Decrease by 10",
          "opacity",
          -10,
          ["Ctrl+Shift+,"],
        ],
        [
          "tools-opacity-increase-skip",
          "Tool's Opacity: Increase by 10",
          "opacity",
          10,
          ["Ctrl+Shift+."],
        ],
      ] as const
    ).map(([id, label, key, delta, keys]): Action => ({
      id,
      label,
      keys: [...keys],
      enabled: () => tools.def.options.some((o) => o.key === key),
      run: () => adjustToolOption(key, delta),
    })),

    // Tools
    ...TOOLS.map((t): Action => ({
      id: `tool-${t.id}`,
      label: t.name,
      icon: t.icon,
      keys: t.keys,
      run: () => tools.select(t.id),
    })),
  ]);
}
