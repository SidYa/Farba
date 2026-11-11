// App preferences of Main menu > Settings that do not belong to a feature module
// (transparency: canvasprefs, rulers: guides, toolbox grouping: tools). Each one is applied
// at once and remembered.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { IconSet } from "./icons";
import { loadSetting, saveSetting } from "./settings";
import { theme } from "./theme.svelte";

export type Accent =
  | "blue"
  | "teal"
  | "green"
  | "orange"
  | "pink"
  | "purple"
  | "red"
  | "amber"
  | "lime"
  | "cyan"
  | "indigo"
  | "custom";

/** Accent colours per theme ([dark, light]); "blue" is the theme's own (app.css). The second
 *  row (owner request 2026-11-11) follows the hues of the Material / Tailwind palettes:
 *  about the 400 shade on the dark theme and the 700 shade on the light one. */
export const ACCENTS: Record<Exclude<Accent, "custom">, [string, string]> = {
  pink: ["#e8607a", "#c2185b"],
  orange: ["#f0913a", "#d0600a"],
  green: ["#5cb860", "#2e7d32"],
  teal: ["#2bb3a3", "#00897b"],
  blue: ["#4b9be8", "#1f6fd1"],
  purple: ["#a77be8", "#6a3fc8"],
  red: ["#ef5350", "#c62828"],
  amber: ["#f2b632", "#a86200"],
  lime: ["#a5c94b", "#5a8a12"],
  cyan: ["#37b9d8", "#00789e"],
  indigo: ["#7f8cf0", "#3f51b5"],
};

/** The Settings swatches, row by row; the last one opens a colour chooser. */
export const ACCENT_ROWS: Accent[][] = [
  ["pink", "orange", "green", "teal", "blue", "purple"],
  ["red", "amber", "lime", "cyan", "indigo", "custom"],
];

/** Backgrounds the accent must stand out from, per theme (app.css: the panels and popups). */
const ACCENT_BACKS = { dark: ["#303030", "#353535", "#2b2b2b"], light: ["#f6f6f6", "#ffffff"] };
/** Least contrast of the accent with those backgrounds (WCAG 2 for UI parts: 3:1). */
const ACCENT_CONTRAST = 3;

function rgbOf(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
}

function hexOf(c: number[]): string {
  return (
    "#" +
    c
      .map((v) =>
        Math.round(Math.min(255, Math.max(0, v)))
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")
  );
}

function luminance(c: number[]): number {
  const l = c.map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * l[0]! + 0.7152 * l[1]! + 0.0722 * l[2]!;
}

function contrast(a: number[], b: number[]): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/**
 * The accent as used on a theme: a colour too close to the theme's backgrounds (white on
 * the light theme, black on the dark one, a pale yellow on white...) is darkened or
 * lightened, keeping its hue, until it stands out by ACCENT_CONTRAST.
 */
export function accentFor(hex: string, dark: boolean): string {
  const c = rgbOf(hex);
  const backs = ACCENT_BACKS[dark ? "dark" : "light"].map(rgbOf);
  const ok = (v: number[]) => backs.every((b) => contrast(v, b) >= ACCENT_CONTRAST);
  if (ok(c)) return hex;
  // Mix towards white (dark theme) or black (light theme) in small steps.
  const to = dark ? 255 : 0;
  for (let t = 0.05; t <= 1; t += 0.05) {
    const v = c.map((x) => x + (to - x) * t);
    if (ok(v)) return hexOf(v);
  }
  return dark ? "#ffffff" : "#000000";
}

/** Text on the accent (selected buttons...): white, or near black on a light accent. */
function accentText(hex: string): string {
  const c = rgbOf(hex);
  return contrast(c, [255, 255, 255]) >= contrast(c, [26, 26, 26]) ? "#ffffff" : "#1a1a1a";
}

export const UI_SCALES = [0.9, 1, 1.1, 1.25, 1.5];

/** How a newly opened image is zoomed: fit (at most 100%, GIMP), fit (may enlarge), 100%. */
export type OpenZoom = "fit" | "fill" | "actual";
/** Single-layer images are proposed in this format by Save As (layered ones: .ora). */
export type FlatFormat = "png" | "jpg" | "webp";

export const UNDO_DEFAULT = { megabytes: 512, steps: 200 };

class Prefs {
  accent = $state<Accent>(loadSetting("accent", "blue"));
  /** The colour chosen for the "custom" accent, `#rrggbb` (adapted per theme, accentFor). */
  customAccent = $state<string>(loadSetting("accent-custom", "#e05a9c"));
  /** Icons: Farba's own with the accent colour, own in one colour, or Material Symbols. */
  iconSet = $state<IconSet>(loadSetting("icon-set", "accent"));
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

  /** The accent colour on the dark or light theme (a custom one adapted to it). */
  accentColor(dark: boolean): string {
    if (this.accent === "custom") return accentFor(this.customAccent, dark);
    const c = ACCENTS[this.accent] ?? ACCENTS.blue;
    return accentFor(c[dark ? 0 : 1], dark);
  }

  /** Accent and canvas colours as CSS variables on <html>; call from an $effect too. */
  applyColors() {
    const root = document.documentElement.style;
    const dark = theme.effective === "dark";
    if (this.accent === "blue") {
      root.removeProperty("--accent");
      root.removeProperty("--accent-soft");
      root.removeProperty("--accent-fg");
    } else {
      const c = this.accentColor(dark);
      const [r, g, b] = rgbOf(c);
      root.setProperty("--accent", c);
      root.setProperty("--accent-fg", accentText(c));
      root.setProperty("--accent-soft", `rgba(${r}, ${g}, ${b}, ${dark ? 0.28 : 0.2})`);
    }
    // The accent details of the icons (Icon.svelte); unset: drawn like the rest.
    if (this.iconSet === "accent") root.setProperty("--icon-accent", "var(--accent)");
    else root.removeProperty("--icon-accent");
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

type PrefValues = Omit<Prefs, "set" | "applyColors" | "applyScale" | "accentColor">;

const SETTING_KEYS: Record<keyof PrefValues, string> = {
  accent: "accent",
  customAccent: "accent-custom",
  iconSet: "icon-set",
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
