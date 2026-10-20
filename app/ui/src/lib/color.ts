// Colour conversions for the colour picker dialog. RGB channels are 0..255, alpha 0..1
// (the palette's `Rgba`); HSV is hue 0..360, saturation and value 0..1.
import type { Rgba } from "./palette.svelte";

export interface Hsv {
  h: number;
  s: number;
  v: number;
}

export function hsvToRgb({ h, s, v }: Hsv): [number, number, number] {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return [Math.round(f(5) * 255), Math.round(f(3) * 255), Math.round(f(1) * 255)];
}

/**
 * HSV of a colour. Hue (and saturation) are undefined for greys (and black), so `keep`
 * supplies them: the hue does not jump to red when the saturation is dragged to 0.
 */
export function rgbToHsv(c: Rgba, keep?: Hsv): Hsv {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  const v = max;
  if (max === 0) return { h: keep?.h ?? 0, s: keep?.s ?? 0, v: 0 };
  const s = d / max;
  if (d === 0) return { h: keep?.h ?? 0, s: 0, v };
  let h: number;
  if (max === r) h = ((g - b) / d) % 6;
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  h *= 60;
  if (h < 0) h += 360;
  return { h, s, v };
}

export function withHsv(c: Rgba, hsv: Hsv): Rgba {
  const [r, g, b] = hsvToRgb(hsv);
  return { r, g, b, a: c.a };
}

/** Parses "#RGB", "RRGGBB", "#RRGGBBAA" (the "#" is optional); undefined if invalid. */
export function parseHex(text: string): Rgba | undefined {
  let t = text.trim().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/i.test(t)) t = [...t].map((ch) => ch + ch).join("");
  if (!/^([0-9a-f]{6}|[0-9a-f]{8})$/i.test(t)) return undefined;
  const n = (i: number) => parseInt(t.slice(i, i + 2), 16);
  return { r: n(0), g: n(2), b: n(4), a: t.length === 8 ? n(6) / 255 : 1 };
}

export function cssRgb(r: number, g: number, b: number, a = 1): string {
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}
