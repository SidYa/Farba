// Primary/secondary colours and the recent-colours list (the Pinta-plus model, see CLAUDE.md
// "Colors pad"): always exactly 21 entries, padded with a light grey placeholder.
import { loadSetting, saveSetting } from "./settings";

export interface Rgba {
  r: number; // 0..255
  g: number;
  b: number;
  a: number; // 0..1
}

export const RECENT_COUNT = 21;
export const RECENT_COLUMNS = 7;
export const PLACEHOLDER: Rgba = { r: 230, g: 230, b: 230, a: 1 };
const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 1 };
const WHITE: Rgba = { r: 255, g: 255, b: 255, a: 1 };

export function sameColor(x: Rgba, y: Rgba): boolean {
  return x.r === y.r && x.g === y.g && x.b === y.b && x.a === y.a;
}

export function toHex(c: Rgba): string {
  const h = (n: number) => n.toString(16).padStart(2, "0").toUpperCase();
  return `#${h(c.r)}${h(c.g)}${h(c.b)}${c.a < 1 ? h(Math.round(c.a * 255)) : ""}`;
}

export function toCss(c: Rgba): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;
}

/**
 * Pinta's default palette (34 colours, shown in the colour picker dialog in 2 rows, column
 * by column: each colour above its lighter variant).
 */
// prettier-ignore
export const DEFAULT_PALETTE: readonly Rgba[] = [
  "#ffffff", "#000000", "#a0a0a0", "#808080", "#404040", "#303030",
  "#ff0000", "#ff7f7f", "#ff6a00", "#ffb27f", "#ffd800", "#ffe97f",
  "#b6ff00", "#daff7f", "#4cff00", "#a5ff7f", "#00ff21", "#7fff8e",
  "#00ff90", "#7fffc5", "#00ffff", "#7fffff", "#0094ff", "#7fc9ff",
  "#0026ff", "#7f92ff", "#4800ff", "#a17fff", "#b200ff", "#d67fff",
  "#ff00dc", "#ff7fed", "#ff006e", "#ff7fb6",
].map((hex) => {
  const n = parseInt(hex.slice(1), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255, a: 1 };
});

function pad(list: Rgba[]): Rgba[] {
  const out = list.slice(0, RECENT_COUNT);
  while (out.length < RECENT_COUNT) out.push(PLACEHOLDER);
  return out;
}

// Raw state: colours are immutable values, and placeholders are compared by identity.
class Palette {
  primary = $state.raw<Rgba>(loadSetting("primary-color", BLACK));
  secondary = $state.raw<Rgba>(loadSetting("secondary-color", WHITE));
  recent = $state.raw<Rgba[]>(pad(loadSetting<Rgba[]>("recent-colors", [])));

  /** Sets a colour; `addToRecent` mirrors Pinta-plus (recents updated even if unchanged). */
  setColor(primary: boolean, color: Rgba, addToRecent = true) {
    if (primary) this.primary = color;
    else this.secondary = color;
    if (addToRecent) this.addRecent(color);
    this.save();
  }

  addRecent(c: Rgba) {
    const list = this.recent;
    if (list[0] && sameColor(list[0], c)) return;
    const i = list.findIndex((x) => sameColor(x, c));
    const next = i >= 0 ? [c, ...list.slice(0, i), ...list.slice(i + 1)] : [c, ...list];
    this.recent = pad(next);
  }

  clearRecent() {
    this.recent = pad([]);
    this.save();
  }

  /** The swap icon: swaps without touching recents. */
  swapQuiet() {
    [this.primary, this.secondary] = [this.secondary, this.primary];
    this.save();
  }

  /** The X key: swaps through the setters, so both colours go to recents. */
  swapWithRecents() {
    const p = this.primary;
    this.setColor(true, this.secondary);
    this.setColor(false, p);
  }

  /** Reset: black / white, both added to recents (white ends up first). */
  reset() {
    this.setColor(true, BLACK);
    this.setColor(false, WHITE);
  }

  private save() {
    saveSetting("primary-color", this.primary);
    saveSetting("secondary-color", this.secondary);
    saveSetting(
      "recent-colors",
      this.recent.filter((c) => c !== PLACEHOLDER),
    );
  }
}

export const palette = new Palette();
