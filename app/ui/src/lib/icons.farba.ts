// Farba's own icons (owner request 2026-11-11): hand-drawn for this app, one style for all
// of them. Outline icons on a 24 x 24 grid, 1.75 stroke, round caps and joins, drawn with
// currentColor. The part that is the point of the tool (the paint, the drop, the handle...)
// has the class "a" (stroke) or "af" (fill): with Settings > Appearance > "Accent colour
// in icons" it takes the accent colour (--icon-accent), otherwise it is drawn like the
// rest. "f" is a filled detail in the icon's own colour.
//
// Each entry is the inside of the <svg>; lib/icons.ts wraps it. Names used by commands and
// components are the ones the Material set had (which stays as the fallback for names not
// drawn here, e.g. an add-on's icon); the tools have names of their own.

const p = (d: string, cls?: string, extra = "") =>
  `<path d="${d}"${cls ? ` class="${cls}"` : ""}${extra}/>`;
const dot = (cx: number, cy: number, r: number, cls = "f") =>
  `<circle cx="${cx}" cy="${cy}" r="${r}" class="${cls}"/>`;
const ring = (cx: number, cy: number, r: number, cls?: string, extra = "") =>
  `<circle cx="${cx}" cy="${cy}" r="${r}"${cls ? ` class="${cls}"` : ""}${extra}/>`;
const rect = (x: number, y: number, w: number, h: number, rx = 1.5, cls?: string, extra = "") =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}"${cls ? ` class="${cls}"` : ""}${extra}/>`;

/** Marching-ants look for selection outlines. */
const DASH = ` stroke-dasharray="2.6 2.73"`;

const drop = "M12 3.5s6 6.3 6 10.5a6 6 0 0 1-12 0c0-4.2 6-10.5 6-10.5z";
const eye = "M2.5 12S6 6 12 6s9.5 6 9.5 6-3.5 6-9.5 6-9.5-6-9.5-6z";
const floppy =
  "M5.5 4h10.2L20 8.3v10.2a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 18.5v-13A1.5 1.5 0 0 1 5.5 4z";
const magnifier = ring(10.5, 10.5, 6.5) + p("M15.3 15.3l5.2 5.2");
const rotateRight = p("M20 12a8 8 0 1 1-2.34-5.66L20 8.7") + p("M20 4v4.7h-4.7");
const rotateLeft = p("M4 12a8 8 0 1 0 2.34-5.66L4 8.7") + p("M4 4v4.7h4.7");
const scale =
  rect(4, 12, 8, 8, 1.2, "a") +
  p("M4 8.5v-3A1.5 1.5 0 0 1 5.5 4h13A1.5 1.5 0 0 1 20 5.5v13a1.5 1.5 0 0 1-1.5 1.5h-3") +
  p("M12.5 11.5L17 7") +
  p("M13.3 7H17v3.7");
const tee = p("M5 7.5V5h14v2.5") + p("M12 5v14") + p("M9 19h6");
const bandage =
  `<g transform="rotate(-45 12 12)">` +
  rect(2.5, 8, 19, 8, 2.6) +
  p("M8.8 8v8M15.2 8v8") +
  dot(10.9, 10.9, 0.75, "af") +
  dot(13.1, 10.9, 0.75, "af") +
  dot(10.9, 13.1, 0.75, "af") +
  dot(13.1, 13.1, 0.75, "af") +
  `</g>`;
const brush =
  p("M20.6 3.4a1.7 1.7 0 0 0-2.4 0l-8.5 8.5 2.4 2.4 8.5-8.5a1.7 1.7 0 0 0 0-2.4z") +
  p(
    "M9.2 13c-1.9-.1-3.3 1.2-3.6 3-.2 1.4-.9 2.4-2.2 3 1.8 1 4.4.9 6-.3 1.5-1.1 2.1-2.9 1.4-4.2z",
    "af",
  );
const pencil =
  p("M4 20l1-4.5L16.5 4a2.47 2.47 0 0 1 3.5 3.5L8.5 19z") +
  p("M4 20l1-4.5L8.5 19z", "af") +
  p("M14.4 6.1l3.5 3.5");
const flipH =
  p("M12 3v18", "a", ` stroke-dasharray="2.4 2.4"`) + p("M9 6.5v11H3.5z") + p("M15 6.5v11h5.5z");
const flipV =
  p("M3 12h18", "a", ` stroke-dasharray="2.4 2.4"`) + p("M6.5 9h11V3.5z") + p("M6.5 15h11v5.5z");
const marquee = rect(4, 4, 16, 16, 1.5, undefined, DASH);

export const FARBA_ICONS: Record<string, string> = {
  // --- Tools -----------------------------------------------------------------------------
  tool_move:
    p("M12 3v18M3 12h18") +
    p("M9.2 5.8L12 3l2.8 2.8M9.2 18.2L12 21l2.8-2.8") +
    p("M5.8 9.2L3 12l2.8 2.8M18.2 9.2L21 12l-2.8 2.8"),
  tool_rect_select: rect(3.5, 5, 15.5, 13, 1.2, undefined, DASH) + rect(17, 16, 4.5, 4.5, 1, "af"),
  tool_ellipse_select:
    `<ellipse cx="11.5" cy="11.5" rx="8.5" ry="7.5"${DASH}/>` + rect(17, 16, 4.5, 4.5, 1, "af"),
  tool_free_select:
    `<ellipse cx="12" cy="9.5" rx="8.5" ry="5.8"/>` +
    p("M7.6 15.6c-.9 1.3-.4 2.4.6 3 1.3.8 1.4 1.9.5 3") +
    dot(7.8, 14.4, 1.9, "af"),
  tool_fuzzy_select:
    p("M4 20l8.6-8.6") +
    p("M12.3 11.7l2.6-2.6", undefined, ` stroke-width="3.4"`) +
    p("M18.5 2.8v4M16.5 4.8h4", "a") +
    p("M19.8 11.5v3M18.3 13h3", "a") +
    p("M9.5 3.5v3M8 5h3", "a"),
  tool_color_select: marquee + ring(12, 12, 3.6, "af"),
  tool_rotate: rotateRight + dot(12, 12, 1.7, "af"),
  tool_scale: scale,
  tool_flip: flipH,
  tool_transform_3d: p("M12 3l8 4.5v9L12 21l-8-4.5v-9z") + p("M4 7.5l8 4.5 8-4.5M12 12v9", "a"),
  tool_handle_transform:
    p("M6 6l12 2-2 10-10.5-1z") +
    rect(4.2, 4.2, 3.6, 3.6, 0.8, "af") +
    rect(16.2, 6.2, 3.6, 3.6, 0.8, "af") +
    rect(14.2, 16.2, 3.6, 3.6, 0.8, "af") +
    rect(3.7, 15.2, 3.6, 3.6, 0.8, "af"),
  tool_brush: brush,
  tool_pencil: pencil,
  tool_bucket:
    p("M9.7 4.2l8.6 8.6-6 6a2 2 0 0 1-2.8 0L3.7 13a2 2 0 0 1 0-2.8z") +
    p("M9.7 4.2L7.6 2.1") +
    p("M3.6 11.9h13.8") +
    p("M20 15.2s1.9 2.3 1.9 3.6a1.9 1.9 0 0 1-3.8 0c0-1.3 1.9-3.6 1.9-3.6z", "af"),
  tool_gradient:
    rect(3.5, 4.5, 17, 15, 1.5) +
    p("M4.4 5.4h4.1v13.2H4.4z", "af") +
    p("M11.5 7v10", "a", ` stroke-width="2"`) +
    p("M14.6 7v10", "a", ` stroke-width="1.3" stroke-dasharray="2.2 1.6"`) +
    p("M17.4 7.5v9", "a", ` stroke-width="1" stroke-dasharray="0.6 2.2"`),
  tool_eraser:
    p("M13.5 4l7 7-8.3 8.3a2 2 0 0 1-2.8 0L5.2 15.1a2 2 0 0 1 0-2.8z") +
    p("M9.3 8.2l7 7") +
    p("M14.5 20.5h6.5", "a"),
  tool_picker:
    p("M13.2 8.2l-8 8V19H8l8-8") +
    p("M7.4 14H13l-5 5H5.2v-2.8z", "af") +
    p("M11.3 6.3l6.4 6.4") +
    p("M14 8l2.6-2.6a2.47 2.47 0 0 1 3.5 3.5L17.5 11.5"),
  tool_text: tee,
  tool_clone:
    p("M10 13c.5-1.5.6-2.7.2-4.2a3 3 0 1 1 3.6 0c-.4 1.5-.3 2.7.2 4.2") +
    rect(5, 13, 14, 4, 1.2) +
    p("M6 20.5h12", "a"),
  tool_heal: bandage,
  tool_smudge:
    p("M13.5 13.5V5a2 2 0 0 0-4 0v9.5l-1.6-1.7a1.9 1.9 0 0 0-2.9 2.4l2.6 3.6") +
    p("M13.5 10.5a1.75 1.75 0 0 1 3.5 0V12a1.75 1.75 0 0 1 3.5 0v3.5c0 1.6-.5 2.7-1.4 3.7") +
    p("M4 21.2c2-1.6 3.8.7 6 0s3.2-1.5 5.5-.4 3.5.4 5-.6", "a"),
  tool_blur: p(drop) + p("M9 14.3a3 3 0 0 0 3 3", "a"),
  tool_dodge:
    ring(11, 12, 5.5) +
    p("M11 6.5a5.5 5.5 0 0 0 0 11z", "f") +
    p("M19.5 12h2M17.4 6.6l1.4-1.4M17.4 17.4l1.4 1.4M11 2.5v1.5M11 20v1.5", "a"),

  // Selection combine modes (tool options).
  mode_replace: rect(5, 5, 14, 14, 1.5),
  mode_add: p("M4 4h11v5h5v11H9v-5H4z") + p("M12 12.5v4M10 14.5h4", "a"),
  mode_subtract: p("M4 4h11v5H9v6H4z") + p("M15 9h5v11H9v-5", "a", ` stroke-dasharray="2.2 2.2"`),
  mode_intersect: p("M4 4h11v11H4z") + p("M9 9h11v11H9z") + rect(9.9, 9.9, 4.2, 4.2, 0.4, "af"),

  // --- Basics ----------------------------------------------------------------------------
  add: p("M12 5v14M5 12h14"),
  remove: p("M5 12h14"),
  close: p("M6 6l12 12M18 6L6 18"),
  close_small: p("M8 8l8 8M16 8l-8 8"),
  check: p("M5 12.5l4.5 4.5L19 7.5"),
  chevron_right: p("M9.5 6l6 6-6 6"),
  menu: p("M4 7h16M4 12h16M4 17h16"),
  more_vert: dot(12, 5.5, 1.5) + dot(12, 12, 1.5) + dot(12, 18.5, 1.5),
  more_horiz: dot(5.5, 12, 1.5) + dot(12, 12, 1.5) + dot(18.5, 12, 1.5),
  arrow_upward: p("M12 19V5M6 11l6-6 6 6"),
  arrow_downward: p("M12 5v14M6 13l6 6 6-6"),
  swap_vert: p("M8 4v15M4.5 7.5L8 4l3.5 3.5") + p("M16 20V5M12.5 16.5L16 20l3.5-3.5"),
  crop_square: rect(5, 5, 14, 14, 1.5),
  filter_none:
    rect(4.5, 8, 11.5, 11.5, 1.5) +
    p("M8 8V6a1.5 1.5 0 0 1 1.5-1.5H18A1.5 1.5 0 0 1 19.5 6v8.5A1.5 1.5 0 0 1 18 16h-2"),
  info: ring(12, 12, 8.5) + p("M12 11.2v5.3") + dot(12, 7.9, 1.15, "af"),
  cancel: ring(12, 12, 8.5) + p("M9 9l6 6M15 9l-6 6"),
  favorite: p(
    "M12 20S4 14.9 4 9.2A4.3 4.3 0 0 1 12 7a4.3 4.3 0 0 1 8 2.2C20 14.9 12 20 12 20z",
    "a",
  ),
  license: ring(12, 9, 5.5) + p("M8.6 13.4L7 21l5-2.5 5 2.5-1.6-7.6") + dot(12, 9, 1.6, "af"),
  settings:
    p(
      "M10.27 5.22L10.44 2.93L13.56 2.93L13.73 5.22L15.57 5.98L17.31 4.49L19.51 6.69L18.02 8.43L18.78 10.27L21.07 10.44L21.07 13.56L18.78 13.73L18.02 15.57L19.51 17.31L17.31 19.51L15.57 18.02L13.73 18.78L13.56 21.07L10.44 21.07L10.27 18.78L8.43 18.02L6.69 19.51L4.49 17.31L5.98 15.57L5.22 13.73L2.93 13.56L2.93 10.44L5.22 10.27L5.98 8.43L4.49 6.69L6.69 4.49L8.43 5.98z",
    ) + ring(12, 12, 3, "a"),
  extension: p(
    "M9.5 7V5.5a2 2 0 0 1 4 0V7H17a1.5 1.5 0 0 1 1.5 1.5V12H20a2 2 0 0 1 0 4h-1.5v3.5A1.5 1.5 0 0 1 17 21H5.5A1.5 1.5 0 0 1 4 19.5V8.5A1.5 1.5 0 0 1 5.5 7z",
  ),
  tune:
    p("M4 7h9M17 7h3M4 12h3M11 12h9M4 17h10M18 17h2") +
    ring(15, 7, 2, "a") +
    ring(9, 12, 2, "a") +
    ring(16, 17, 2, "a"),
  instant_mix:
    p("M6 4v4M6 12v8M12 4v9M12 17v3M18 4v2M18 10v10") +
    ring(6, 10, 2, "a") +
    ring(12, 15, 2, "a") +
    ring(18, 8, 2, "a"),
  link:
    p("M10 14a4 4 0 0 0 5.66 0l3-3A4 4 0 0 0 13 5.34l-1 1") +
    p("M14 10a4 4 0 0 0-5.66 0l-3 3A4 4 0 0 0 11 18.66l1-1"),
  link_off:
    p("M13.5 5.8l.5-.5A4 4 0 0 1 19.66 11l-1.6 1.6") +
    p("M10.5 18.2l-.5.5A4 4 0 0 1 4.34 13l1.6-1.6") +
    p("M8.5 3.5V6M3.5 8.5H6M15.5 20.5V18M20.5 15.5H18", "a"),
  lock: rect(5, 10.5, 14, 10, 1.5) + p("M8 10.5V8a4 4 0 0 1 8 0v2.5") + dot(12, 15.5, 1.4, "af"),
  lock_open:
    rect(5, 10.5, 14, 10, 1.5) + p("M8 10.5V8a4 4 0 0 1 7.7-1.5") + dot(12, 15.5, 1.4, "af"),
  visibility: p(eye) + ring(12, 12, 2.8, "a"),
  visibility_off: p(eye) + p("M4.5 4.5l15 15"),

  // --- File and edit ---------------------------------------------------------------------
  note_add:
    p("M13.5 3H7a1.5 1.5 0 0 0-1.5 1.5v15A1.5 1.5 0 0 0 7 21h10a1.5 1.5 0 0 0 1.5-1.5V8z") +
    p("M13.5 3v5h5") +
    p("M12 11.5v6M9 14.5h6", "a"),
  file_open:
    p("M5 19.5A1.5 1.5 0 0 1 3.5 18V6A1.5 1.5 0 0 1 5 4.5h4.3l2 2.5H18a1.5 1.5 0 0 1 1.5 1.5v1.6") +
    p(
      "M5 19.5h12.4a1.5 1.5 0 0 0 1.43-1.05l2.2-7a1 1 0 0 0-.95-1.3H7.4a1.5 1.5 0 0 0-1.43 1.05L3.6 18.4",
      "a",
    ),
  save: p(floppy) + p("M8 4v4.5h6.5V4") + p("M7.5 20v-6h9v6", "a"),
  save_as:
    p("M20 10.5V8.3L15.7 4H5.5A1.5 1.5 0 0 0 4 5.5v13A1.5 1.5 0 0 0 5.5 20H10") +
    p("M8 4v4.5h6.5V4") +
    p("M13 21l.7-3.2 5.4-5.4a1.77 1.77 0 0 1 2.5 2.5l-5.4 5.4z", "a"),
  undo: p("M8.5 5L4 9.5 8.5 14") + p("M4 9.5h10.5a5 5 0 0 1 0 10H11"),
  redo: p("M15.5 5L20 9.5 15.5 14") + p("M20 9.5H9.5a5 5 0 0 0 0 10H13"),
  content_cut:
    ring(6.5, 6.5, 2.6, "a") + ring(6.5, 17.5, 2.6, "a") + p("M8.7 8.2L20 18.5M8.7 15.8L20 5.5"),
  content_copy:
    p("M15.5 8V5.5A1.5 1.5 0 0 0 14 4H5.5A1.5 1.5 0 0 0 4 5.5V14a1.5 1.5 0 0 0 1.5 1.5H8") +
    rect(8.5, 8.5, 11.5, 11.5, 1.5, "a"),
  content_paste:
    p(
      "M9 5H6.5A1.5 1.5 0 0 0 5 6.5v13A1.5 1.5 0 0 0 6.5 21h11a1.5 1.5 0 0 0 1.5-1.5v-13A1.5 1.5 0 0 0 17.5 5H15",
    ) +
    rect(9, 3, 6, 4, 1) +
    p("M9 12h6M9 16h4", "a"),
  delete:
    p("M4.5 6.5h15") +
    p("M9.5 6.5V5A1.5 1.5 0 0 1 11 3.5h2A1.5 1.5 0 0 1 14.5 5v1.5") +
    p("M6.5 6.5l.9 12.6a1.5 1.5 0 0 0 1.5 1.4h6.2a1.5 1.5 0 0 0 1.5-1.4l.9-12.6") +
    p("M10 10.5v6M14 10.5v6"),
  clear_all: p("M4 7h12M4 12h15M4 17h8") + p("M16 15.5l4.5 4.5M20.5 15.5L16 20", "a"),

  // --- Selection, image, view ------------------------------------------------------------
  select: marquee + rect(8.5, 8.5, 7, 7, 1, "af"),
  deselect: marquee + p("M8 16l8-8", "a"),
  crop: p("M7 3v12.5A1.5 1.5 0 0 0 8.5 17H21") + p("M3 7h12.5A1.5 1.5 0 0 1 17 8.5V21"),
  zoom_in: magnifier + p("M10.5 7.8v5.4M7.8 10.5h5.4", "a"),
  zoom_out: magnifier + p("M7.8 10.5h5.4", "a"),
  fit_screen:
    p("M4 9V6.5A1.5 1.5 0 0 1 5.5 5H8M16 5h2.5A1.5 1.5 0 0 1 20 6.5V9") +
    p("M20 15v2.5a1.5 1.5 0 0 1-1.5 1.5H16M8 19H5.5A1.5 1.5 0 0 1 4 17.5V15") +
    rect(8, 9, 8, 6, 1, "a"),
  brightness_auto:
    rect(3, 4.5, 18, 12, 1.5) +
    p("M8.5 20h7M12 16.5V20") +
    ring(12, 10.5, 2.8) +
    p("M12 7.7a2.8 2.8 0 0 1 0 5.6z", "af"),
  light_mode:
    ring(12, 12, 4, "a") +
    p(
      "M12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8",
    ),
  dark_mode: p("M20 14.5A8.5 8.5 0 1 1 9.5 4 6.8 6.8 0 0 0 20 14.5z"),
  image:
    rect(3.5, 4.5, 17, 15, 1.5) +
    p("M4 16.5L8.5 12l4.5 4.5M13.5 15l2.5-2.5 4 4") +
    dot(15.3, 8.8, 1.6, "af"),
  aspect_ratio: rect(3, 5, 18, 14, 1.5) + p("M7 12V9h3M17 12v3h-3", "a"),
  photo_size_select_large: scale,
  open_in_full: scale,
  open_with:
    p("M12 3v18M3 12h18") +
    p("M9.2 5.8L12 3l2.8 2.8M9.2 18.2L12 21l2.8-2.8") +
    p("M5.8 9.2L3 12l2.8 2.8M18.2 9.2L21 12l-2.8 2.8"),
  flip: flipH,
  flip_vertical: flipV,
  rotate_right: rotateRight,
  rotate_left: rotateLeft,
  cached:
    p("M4.6 9a8 8 0 0 1 14.3-1.6L20 9") +
    p("M20 4.5V9h-4.5") +
    p("M19.4 15a8 8 0 0 1-14.3 1.6L4 15") +
    p("M4 19.5V15h4.5"),
  background_replace: marquee + ring(12, 9.6, 2.6, "a") + p("M7.2 17.5a4.8 4.8 0 0 1 9.6 0", "a"),

  // --- Layers ----------------------------------------------------------------------------
  merge: p("M12 3.5v10.5M7.5 9.5L12 14l4.5-4.5") + p("M4.5 19.5h15", "a"),
  view_apps:
    p("M12 3.5l8.5 4.5-8.5 4.5L3.5 8z", "a") + p("M3.5 12.5L12 17l8.5-4.5M3.5 16.5L12 21l8.5-4.5"),
  preview:
    p("M3 10S6.5 4.5 12 4.5 21 10 21 10s-3.5 5.5-9 5.5S3 10 3 10z") +
    ring(12, 10, 2.4, "a") +
    p("M5 20h14"),
  text_fields: tee,

  // --- Colours and effects ---------------------------------------------------------------
  exposure:
    rect(4, 4, 16, 16, 1.5) +
    p("M19.4 4.6L4.6 19.4") +
    p("M7 8.5h4") +
    p("M13.5 15.8h4M15.5 13.8v4", "a"),
  contrast: ring(12, 12, 8.5) + p("M12 3.5a8.5 8.5 0 0 1 0 17z", "af"),
  tonality: ring(12, 12, 8.5) + p("M12 3.5a8.5 8.5 0 0 1 0 17z", "af"),
  format_color_reset: p(drop) + p("M4.5 4.5l15 15", "a"),
  invert_colors: p(drop) + p("M12 3.5V20a6 6 0 0 0 6-6c0-4.2-6-10.5-6-10.5z", "af"),
  water_drop: p(drop) + p("M9 14.3a3 3 0 0 0 3 3", "a"),
  palette:
    p(
      "M12 3.5a8.5 8.5 0 1 0 0 17c1.1 0 1.9-.8 1.9-1.9 0-.5-.2-1-.5-1.3-.3-.4-.5-.8-.5-1.3 0-1.1.9-2 2-2h2.2c2.4 0 4.4-2 4.4-4.4 0-3.4-4.2-6.1-9.5-6.1z",
    ) +
    dot(7.4, 11.6, 1.15, "af") +
    dot(9.6, 7.8, 1.15, "af") +
    dot(14.4, 7.6, 1.15, "af") +
    dot(17.6, 10.8, 1.15, "af"),
  filter_b_and_w: rect(4, 4, 16, 16, 1.5) + p("M20 4v14.5a1.5 1.5 0 0 1-1.5 1.5H4z", "f"),
  healing: bandage,
  grain:
    dot(6, 6, 1.3) +
    dot(14, 6, 1.3, "af") +
    dot(10, 10, 1.3, "af") +
    dot(18, 10, 1.3) +
    dot(6, 14, 1.3, "af") +
    dot(14, 14, 1.3) +
    dot(10, 18, 1.3) +
    dot(18, 18, 1.3, "af"),
  center_focus_strong: p("M12 4l8.5 15.5h-17z") + p("M12 11l3.3 6H8.7z", "af"),
  blur_on:
    dot(12, 12, 2.6, "af") +
    ring(12, 12, 5.6, undefined, ` stroke-dasharray="1.6 2.3"`) +
    ring(12, 12, 8.8, undefined, ` stroke-dasharray="0.3 3.2"`),
  vignette: rect(3, 5, 18, 14, 2) + `<ellipse cx="12" cy="12" rx="6" ry="4.2" class="a"/>`,
  deblur:
    p("M10 4l1.7 4.3L16 10l-4.3 1.7L10 16l-1.7-4.3L4 10l4.3-1.7z") +
    p("M18 13.5l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9z", "a"),

  // --- Text style ------------------------------------------------------------------------
  format_bold: p(
    "M7.5 4.5h5.3a3.6 3.6 0 0 1 0 7.2H7.5zM7.5 11.7h6.2a3.9 3.9 0 0 1 0 7.8H7.5z",
    undefined,
    ` stroke-width="2.4"`,
  ),
  format_italic: p("M10 4.5h8M6 19.5h8M14.5 4.5l-5 15"),
  format_underlined: p("M7 4v7a5 5 0 0 0 10 0V4") + p("M5.5 20h13", "a"),
  strikethrough_s:
    p("M16.5 7.6c-.6-2-2.3-3.1-4.5-3.1-2.5 0-4.3 1.3-4.3 3.4 0 1.2.6 2 1.5 2.6") +
    p("M7.2 16c.5 2.2 2.4 3.5 4.9 3.5 2.6 0 4.6-1.4 4.6-3.6 0-.6-.1-1.1-.4-1.5") +
    p("M4 12h16", "a"),
  vertical_align_center:
    p("M4 4.5h16M4 19.5h16") + p("M12 8v8M9.8 10.2L12 8l2.2 2.2M9.8 13.8L12 16l2.2-2.2", "a"),
  format_letter_spacing:
    p("M4.5 4v16M19.5 4v16") + p("M8 12h8M10.2 9.8L8 12l2.2 2.2M13.8 9.8L16 12l-2.2 2.2", "a"),
  format_clear: p("M4 6.5V4.5h12v2M10 4.5V19M7.5 19h5") + p("M15.5 14.5l5 5M20.5 14.5l-5 5", "a"),
};
