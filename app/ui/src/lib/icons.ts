// Symbolic icons, coloured with currentColor. Farba's own set (lib/icons.farba.ts, drawn for
// this app: outline, one stroke weight, with an optional accent-coloured detail) is used by
// default; Settings > Appearance > Icons can switch to the Material Symbols the app had
// before (Outlined, weight 500, Apache-2.0, embedded by scripts/gen-icons.mjs), which also
// serve names that are not drawn in the own set (e.g. the icon of an add-on).

import { FARBA_ICONS } from "./icons.farba";
import { MATERIAL_ICONS } from "./icons.generated";

/** "accent": own icons with the accent colour; "plain": own, one colour; "material". */
export type IconSet = "accent" | "plain" | "material";

const material: Record<string, string> = MATERIAL_ICONS;

/** Material names of the icons that exist only in the own set (the tools, the modes). */
const MATERIAL_NAMES: Record<string, string> = {
  tool_move: "open_with",
  tool_rect_select: "ink_selection",
  tool_ellipse_select: "lasso_select",
  tool_free_select: "alt_route",
  tool_fuzzy_select: "wand_shine",
  tool_color_select: "border_outer",
  tool_rotate: "rotate_right",
  tool_scale: "open_in_full",
  tool_flip: "flip",
  tool_transform_3d: "3d_rotation",
  tool_handle_transform: "transform",
  tool_brush: "brush",
  tool_pencil: "edit",
  tool_bucket: "format_color_fill",
  tool_gradient: "gradient",
  tool_eraser: "ink_eraser",
  tool_picker: "colorize",
  tool_text: "text_fields",
  tool_clone: "approval",
  tool_heal: "healing",
  tool_smudge: "pan_tool_alt",
  tool_blur: "water_drop",
  tool_dodge: "tonality",
  mode_replace: "select",
  mode_add: "add",
  mode_subtract: "remove",
  mode_intersect: "close_small",
};

const WRAP =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" class="fi" fill="none" ' +
  'stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">';

const wrapped = new Map<string, string>();

export function iconSvg(name: string, set: IconSet = "accent"): string {
  if (set !== "material") {
    let svg = wrapped.get(name);
    if (svg) return svg;
    const inner = FARBA_ICONS[name];
    if (inner) {
      svg = `${WRAP}${inner}</svg>`;
      wrapped.set(name, svg);
      return svg;
    }
  }
  const svg = material[MATERIAL_NAMES[name] ?? name];
  if (!svg) {
    console.warn(`unknown icon: ${name}`);
    return "";
  }
  return svg;
}
