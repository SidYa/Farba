// Symbolic icons: Material Symbols Outlined, weight 500 (Apache-2.0), embedded by
// scripts/gen-icons.mjs (`pnpm icons`) and coloured with currentColor. Every icon comes from
// that one set, so they all have the same stroke weight (no hand-drawn ones; variants such
// as a vertical flip are derived there).

import { MATERIAL_ICONS } from "./icons.generated";

const icons: Record<string, string> = MATERIAL_ICONS;

export function iconSvg(name: string): string {
  const svg = icons[name];
  if (!svg) {
    console.warn(`unknown icon: ${name}`);
    return "";
  }
  return svg;
}
