// App preferences of Main menu > Settings that do not belong to a feature module
// (transparency: canvasprefs, rulers: guides, toolbox grouping: tools). Each one is applied
// at once and remembered.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { loadSetting, saveSetting } from "./settings";
import { theme } from "./theme.svelte";

export type Accent = "blue" | "teal" | "green" | "orange" | "pink" | "purple";

/** Accent colours per theme ([dark, light]); "blue" is the theme's own (app.css). */
export const ACCENTS: Record<Accent, [string, string]> = {
  blue: ["#4b9be8", "#1f6fd1"],
  teal: ["#2bb3a3", "#00897b"],
  green: ["#5cb860", "#2e7d32"],
  orange: ["#f0913a", "#d0600a"],
  pink: ["#e8607a", "#c2185b"],
  purple: ["#a77be8", "#6a3fc8"],
};

export const UI_SCALES = [0.9, 1, 1.1, 1.25, 1.5];

/** How a newly opened image is zoomed: fit (at most 100%, GIMP), fit (may enlarge), 100%. */
export type OpenZoom = "fit" | "fill" | "actual";
/** Single-layer images are proposed in this format by Save As (layered ones: .ora). */
export type FlatFormat = "png" | "jpg" | "webp";

export const UNDO_DEFAULT = { megabytes: 512, steps: 200 };

class Prefs {
  accent = $state<Accent>(loadSetting("accent", "blue"));
  uiScale = $state<number>(loadSetting("ui-scale", 1));
  /** Colour around the image, `#rrggbb`, or null for the theme's. */
  canvasBack = $state<string | null>(loadSetting<string | null>("canvas-back", null));
  openZoom = $state<OpenZoom>(loadSetting("open-zoom", "fit"));
  /** The plain mouse wheel zooms (Ctrl+wheel scrolls) instead of scrolling. */
  wheelZooms = $state<boolean>(loadSetting("wheel-zooms", false));
  brushOutline = $state<boolean>(loadSetting("brush-outline", true));
  jpegQuality = $state<number>(loadSetting("jpeg-quality", 100));
  /** Ask for the JPEG / HEIC / AVIF quality on the first save of an image. */
  askQuality = $state<boolean>(loadSetting("ask-quality", true));
  flatFormat = $state<FlatFormat>(loadSetting("flat-format", "png"));
  reopenTabs = $state<boolean>(loadSetting("reopen-tabs", false));
  askDeleteLayer = $state<boolean>(loadSetting("ask-delete-layer", true));
  /** Close All asks even when nothing is unsaved. */
  askCloseAll = $state<boolean>(loadSetting("ask-close-all", true));
  undo = $state<{ megabytes: number; steps: number }>(loadSetting("undo-limits", UNDO_DEFAULT));

  set<K extends keyof PrefValues>(key: K, value: PrefValues[K]) {
    (this as PrefValues)[key] = value;
    saveSetting(SETTING_KEYS[key], value);
  }

  /** Accent and canvas colours as CSS variables on <html>; call from an $effect too. */
  applyColors() {
    const root = document.documentElement.style;
    const dark = theme.effective === "dark";
    if (this.accent === "blue") {
      root.removeProperty("--accent");
      root.removeProperty("--accent-soft");
    } else {
      const c = ACCENTS[this.accent][dark ? 0 : 1];
      const n = parseInt(c.slice(1), 16);
      root.setProperty("--accent", c);
      root.setProperty(
        "--accent-soft",
        `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${dark ? 0.28 : 0.2})`,
      );
    }
    if (this.canvasBack) root.setProperty("--bg-canvas", this.canvasBack);
    else root.removeProperty("--bg-canvas");
  }

  /** Interface size: the webview zoom, so the canvas still draws at device pixels. */
  applyScale() {
    void getCurrentWebview()
      .setZoom(this.uiScale)
      .catch(() => {});
  }
}

type PrefValues = Omit<Prefs, "set" | "applyColors" | "applyScale">;

const SETTING_KEYS: Record<keyof PrefValues, string> = {
  accent: "accent",
  uiScale: "ui-scale",
  canvasBack: "canvas-back",
  openZoom: "open-zoom",
  wheelZooms: "wheel-zooms",
  brushOutline: "brush-outline",
  jpegQuality: "jpeg-quality",
  askQuality: "ask-quality",
  flatFormat: "flat-format",
  reopenTabs: "reopen-tabs",
  askDeleteLayer: "ask-delete-layer",
  askCloseAll: "ask-close-all",
  undo: "undo-limits",
};

export const prefs = new Prefs();
