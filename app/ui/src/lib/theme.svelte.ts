// Theme: System / Light / Dark. `system` follows Windows live via prefers-color-scheme.
// The effective theme is written to <html data-theme>, and the native window theme is kept
// in sync so the frame and WebView2 scrollbars match.
//
// Note: /theme-boot.js reads the same localStorage key raw (it stores a bare string, not
// JSON), so this module uses localStorage directly instead of lib/settings.ts.
import { getCurrentWindow } from "@tauri-apps/api/window";

export type ThemeMode = "system" | "light" | "dark";

const KEY = "farba.theme";
const media = window.matchMedia("(prefers-color-scheme: dark)");

function readMode(): ThemeMode {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

class Theme {
  mode = $state<ThemeMode>(readMode());
  systemDark = $state(media.matches);
  effective = $derived<"light" | "dark">(
    this.mode === "system" ? (this.systemDark ? "dark" : "light") : this.mode,
  );

  constructor() {
    media.addEventListener("change", (e) => (this.systemDark = e.matches));
  }

  set(mode: ThemeMode) {
    this.mode = mode;
    try {
      localStorage.setItem(KEY, mode);
    } catch {
      // not remembered
    }
  }

  /** Applies the theme to the document and the native window; call from an $effect. */
  apply() {
    document.documentElement.dataset.theme = this.effective;
    getCurrentWindow()
      .setTheme(this.mode === "system" ? null : this.mode)
      .catch(() => {});
  }
}

export const theme = new Theme();

/** Reads a CSS custom property of the current theme as an [r, g, b] triple in 0..1. */
let probe: CanvasRenderingContext2D | null | undefined;

export function cssColor(name: string): [number, number, number] {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  // Any CSS colour form (the release build minifies `#999999` to `#999`): let the browser
  // normalise it, then read `#rrggbb` (or `rgb[a](r, g, b, ...)`).
  probe ??= document.createElement("canvas").getContext("2d");
  let s = v;
  if (probe) {
    probe.fillStyle = "#000000";
    probe.fillStyle = v;
    s = String(probe.fillStyle);
  }
  const hex = /^#([0-9a-f]{6})$/i.exec(s);
  if (hex) {
    const n = parseInt(hex[1]!, 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  }
  const rgb = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/i.exec(s);
  if (rgb) return [+rgb[1]! / 255, +rgb[2]! / 255, +rgb[3]! / 255];
  return [0.5, 0.5, 0.5];
}
