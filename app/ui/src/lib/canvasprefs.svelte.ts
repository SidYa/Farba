// How the canvas shows transparent pixels (Settings > Canvas): a checkerboard or dots in
// the theme's colours, or one solid colour of the user's choice. Remembered.
import { loadSetting, saveSetting } from "./settings";

export type TransparencyStyle = "checker" | "dots" | "solid";
export type TransparencySize = "minimal" | "medium" | "big";

/** Checker cell size in CSS pixels per size (dots are drawn smaller on the same scale). */
export const CHECK_SIZES: Record<TransparencySize, number> = { minimal: 5, medium: 8, big: 14 };

class CanvasPrefs {
  transparency = $state<TransparencyStyle>(loadSetting("transparency", "checker"));
  /** The solid colour, `#rrggbb`. */
  solid = $state<string>(loadSetting("transparency-color", "#ffffff"));
  size = $state<TransparencySize>(loadSetting("transparency-size", "medium"));
  /** Dots: background and dot colours (`#rrggbb`), or null for the theme's colours. */
  dotsBack = $state<string | null>(loadSetting<string | null>("transparency-dots-back", null));
  dotsFore = $state<string | null>(loadSetting<string | null>("transparency-dots-fore", null));

  setTransparency(style: TransparencyStyle) {
    this.transparency = style;
    saveSetting("transparency", style);
  }

  setSize(size: TransparencySize) {
    this.size = size;
    saveSetting("transparency-size", size);
  }

  setDots(back: string | null, fore: string | null) {
    this.dotsBack = back;
    this.dotsFore = fore;
    saveSetting("transparency-dots-back", back);
    saveSetting("transparency-dots-fore", fore);
  }

  setSolid(color: string) {
    this.solid = color;
    saveSetting("transparency-color", color);
  }
}

export const canvasPrefs = new CanvasPrefs();

/** `#rrggbb` -> 0..1 RGB. */
export function rgb01(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1, 7), 16) || 0;
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}
