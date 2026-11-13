// Tool registry: the MVP tools in the owner's GIMP toolbox order, their GIMP default
// shortcuts, their options (rendered generically by the tool options popover) and the
// toolbox groups (similar tools share one button; hold it to pick another).
import { loadSetting, saveSetting } from "./settings";

export type OptionValue = number | string | boolean;

export type ToolOption =
  | {
      key: string;
      type: "slider";
      label: string;
      min: number;
      max: number;
      step?: number;
      unit?: string;
      default: number;
    }
  | { key: string; type: "select"; label: string; choices: [string, string][]; default: string }
  | { key: string; type: "check"; label: string; default: boolean }
  // Icon buttons, one per mode: [value, tooltip, icon] (the selection combine modes when
  // `choices` is left out).
  | {
      key: string;
      type: "modes";
      label: string;
      default: string;
      choices?: [string, string, string][];
    }
  | { key: string; type: "font"; label: string; default: string }; // installed font families

/** An option shown only while another option of the tool has a given value. */
export type ToolOptionDef = ToolOption & { onlyIf?: [string, OptionValue] };

export interface ToolDef {
  id: string;
  name: string;
  icon: string;
  keys: string[];
  help: string;
  options: ToolOptionDef[];
}

const selectionCommon: ToolOptionDef[] = [
  { key: "mode", type: "modes", label: "Mode", default: "replace" },
  { key: "antialias", type: "check", label: "Antialiasing", default: true },
  { key: "feather", type: "check", label: "Feather edges", default: false },
  {
    key: "featherRadius",
    type: "slider",
    label: "Radius",
    min: 0,
    max: 100,
    default: 10,
    unit: "px",
  },
];

const thresholdOptions: ToolOptionDef[] = [
  { key: "transparent", type: "check", label: "Select transparent areas", default: true },
  { key: "sampleMerged", type: "check", label: "Sample merged", default: false },
  { key: "threshold", type: "slider", label: "Threshold", min: 0, max: 255, default: 15 },
  {
    key: "criterion",
    type: "select",
    label: "Select by",
    choices: [
      ["composite", "Composite"],
      ["red", "Red"],
      ["green", "Green"],
      ["blue", "Blue"],
      ["alpha", "Alpha"],
      ["hue", "HSV Hue"],
      ["saturation", "HSV Saturation"],
      ["value", "HSV Value"],
    ],
    default: "composite",
  },
];

const interpolation: ToolOption = {
  key: "interpolation",
  type: "select",
  label: "Interpolation",
  choices: [
    ["none", "None"],
    ["linear", "Linear"],
    ["cubic", "Cubic"],
    ["lanczos", "Lanczos"],
  ],
  default: "cubic",
};

const brushSize: ToolOption = {
  key: "size",
  type: "slider",
  label: "Size",
  min: 1,
  max: 1000,
  default: 20,
  unit: "px",
};
const opacity: ToolOption = {
  key: "opacity",
  type: "slider",
  label: "Opacity",
  min: 0,
  max: 100,
  default: 100,
  unit: "%",
};
const hardness: ToolOption = {
  key: "hardness",
  type: "slider",
  label: "Hardness",
  min: 0,
  max: 100,
  default: 50,
};

/** Clone / Heal: where the source pixels are taken relative to the stroke (GIMP). */
const cloneAlignment: ToolOption = {
  key: "alignment",
  type: "select",
  label: "Alignment",
  choices: [
    ["none", "None"],
    ["aligned", "Aligned"],
    ["registered", "Registered"],
    ["fixed", "Fixed"],
  ],
  default: "none",
};
const sampleMergedSource: ToolOption = {
  key: "sampleMerged",
  type: "check",
  label: "Sample merged",
  default: false,
};

export const TOOLS: ToolDef[] = [
  {
    id: "move",
    name: "Move",
    icon: "tool_move",
    keys: ["M"],
    help: "Move layers and selections (arrows: 1 px, Shift+arrows: more)",
    options: [
      {
        key: "target",
        type: "select",
        label: "Move",
        choices: [
          ["layer", "Layer"],
          ["selection", "Selection"],
        ],
        default: "layer",
      },
      { key: "pickLayer", type: "check", label: "Pick a layer (Shift toggles)", default: true },
    ],
  },
  {
    id: "rect-select",
    name: "Rectangle Select",
    icon: "tool_rect_select",
    keys: ["R"],
    help: "Select a rectangular region",
    options: [
      ...selectionCommon,
      {
        key: "rounded",
        type: "slider",
        label: "Rounded corners",
        min: 0,
        max: 100,
        default: 0,
        unit: "px",
      },
    ],
  },
  {
    id: "ellipse-select",
    name: "Ellipse Select",
    icon: "tool_ellipse_select",
    keys: ["E"],
    help: "Select an elliptical region",
    options: selectionCommon,
  },
  {
    id: "free-select",
    name: "Free Select",
    icon: "tool_free_select",
    keys: ["F"],
    help: "Select a hand-drawn region with free and polygonal segments",
    options: selectionCommon,
  },
  {
    id: "fuzzy-select",
    name: "Fuzzy Select",
    icon: "tool_fuzzy_select",
    keys: ["U"],
    help: "Select a contiguous region on the basis of colour",
    options: [...selectionCommon, ...thresholdOptions],
  },
  {
    id: "by-color-select",
    name: "Select by Color",
    icon: "tool_color_select",
    keys: ["Shift+O"],
    help: "Select regions with similar colours",
    options: [...selectionCommon, ...thresholdOptions],
  },
  {
    id: "rotate",
    name: "Rotate",
    icon: "tool_rotate",
    keys: ["Shift+R"],
    help: "Rotate the layer or selection (drag the centre to move it; Ctrl: 15° steps). Enter applies",
    options: [interpolation],
  },
  {
    id: "scale",
    name: "Scale",
    icon: "tool_scale",
    keys: ["Shift+S"],
    help: "Scale the layer or selection, keeping the aspect ratio (Shift: free; Ctrl: from the centre). Enter applies",
    options: [
      interpolation,
      // On by default, Shift scales freely (owner request 2026-11-11). A new key, so the
      // "off" saved by earlier versions does not stay.
      { key: "keepRatio", type: "check", label: "Keep aspect ratio", default: true },
    ],
  },
  {
    id: "flip",
    name: "Flip",
    icon: "tool_flip",
    keys: ["Shift+F"],
    help: "Reverse the layer or selection horizontally or vertically (Ctrl toggles)",
    options: [
      {
        key: "direction",
        type: "select",
        label: "Direction",
        choices: [
          ["horizontal", "Horizontal"],
          ["vertical", "Vertical"],
        ],
        default: "horizontal",
      },
    ],
  },
  {
    id: "transform-3d",
    name: "3D Transform",
    icon: "tool_transform_3d",
    keys: ["Shift+W"],
    help: "Rotate the layer or selection in 3D (drag; Shift: one axis; Ctrl: around Z). Enter applies",
    options: [
      interpolation,
      {
        key: "fov",
        type: "slider",
        label: "Angle of view",
        min: 1,
        max: 170,
        default: 53,
        unit: "°",
      },
    ],
  },
  {
    id: "handle-transform",
    name: "Handle Transform",
    icon: "tool_handle_transform",
    keys: ["Shift+L"],
    help: "Deform with up to 4 handles: click adds one, drag moves it (Shift: move only the handle; Ctrl+click: remove). Enter applies",
    options: [interpolation],
  },
  {
    id: "bucket-fill",
    name: "Bucket Fill",
    icon: "tool_bucket",
    keys: ["Shift+B"],
    help: "Fill selected area with a colour (Shift: toggle the affected area; Alt: pick a colour)",
    options: [
      {
        key: "fill",
        type: "select",
        label: "Fill",
        choices: [
          ["fg", "FG colour"],
          ["bg", "BG colour"],
        ],
        default: "fg",
      },
      {
        key: "area",
        type: "select",
        label: "Affected area",
        choices: [
          ["similar", "Fill similar colours"],
          ["selection", "Fill whole selection"],
        ],
        default: "similar",
      },
      opacity,
      { key: "transparent", type: "check", label: "Fill transparent areas", default: true },
      ...thresholdOptions.slice(1),
      { key: "antialias", type: "check", label: "Antialiasing", default: true },
    ],
  },
  {
    id: "gradient",
    name: "Gradient",
    icon: "tool_gradient",
    keys: ["G"],
    help: "Fill with a colour gradient (Ctrl while dragging: snap the angle; Alt: pick a colour)",
    options: [
      {
        key: "gradient",
        type: "select",
        label: "Gradient",
        choices: [
          ["fg-bg", "FG to BG"],
          ["fg-transparent", "FG to transparent"],
        ],
        default: "fg-bg",
      },
      {
        key: "shape",
        type: "select",
        label: "Shape",
        choices: [
          ["linear", "Linear"],
          ["radial", "Radial"],
        ],
        default: "linear",
      },
      {
        key: "repeat",
        type: "select",
        label: "Repeat",
        choices: [
          ["none", "None"],
          ["sawtooth", "Sawtooth"],
          ["triangular", "Triangular"],
        ],
        default: "none",
      },
      { key: "reverse", type: "check", label: "Reverse", default: false },
      opacity,
    ],
  },
  {
    id: "paintbrush",
    name: "Paintbrush",
    icon: "tool_brush",
    keys: ["P"],
    help: "Paint strokes: smooth (Brush mode) or hard edged pixels (Pencil mode). Shift+click: straight line; Alt: pick a colour",
    options: [
      {
        key: "mode",
        type: "modes",
        label: "Mode",
        default: "brush",
        choices: [
          ["brush", "Brush: smooth strokes", "tool_brush"],
          ["pencil", "Pencil: hard edged pixels (N)", "tool_pencil"],
        ],
      },
      brushSize,
      { ...hardness, onlyIf: ["mode", "brush"] },
      opacity,
      {
        key: "spacing",
        type: "slider",
        label: "Spacing",
        min: 1,
        max: 200,
        default: 10,
        unit: "%",
        onlyIf: ["mode", "brush"],
      },
    ],
  },
  {
    id: "eraser",
    name: "Eraser",
    icon: "tool_eraser",
    keys: ["Shift+E"],
    help: "Erase to transparency (Shift+click: straight line; Alt: pick a colour)",
    options: [
      brushSize,
      hardness,
      opacity,
      { key: "hardEdge", type: "check", label: "Hard edge", default: false },
    ],
  },
  {
    id: "clone",
    name: "Clone",
    icon: "tool_clone",
    keys: ["C"],
    help: "Paint with pixels from another place (Ctrl+click: set the source; Alt: pick a colour)",
    options: [brushSize, hardness, opacity, cloneAlignment, sampleMergedSource],
  },
  {
    id: "heal",
    name: "Heal",
    icon: "tool_heal",
    keys: ["H"],
    help: "Remove spots: clone that blends into the surroundings (Ctrl+click: set the source; Alt: pick a colour)",
    options: [brushSize, hardness, opacity, cloneAlignment, sampleMergedSource],
  },
  {
    id: "smudge",
    name: "Smudge",
    icon: "tool_smudge",
    keys: ["S"],
    help: "Smear the pixels along the stroke (Alt: pick a colour)",
    options: [
      brushSize,
      hardness,
      opacity,
      { key: "rate", type: "slider", label: "Rate", min: 0, max: 100, default: 50 },
      { key: "flow", type: "slider", label: "Flow (FG colour)", min: 0, max: 100, default: 0 },
    ],
  },
  {
    id: "blur-sharpen",
    name: "Blur / Sharpen",
    icon: "tool_blur",
    keys: ["Shift+U"],
    help: "Blur or sharpen with a brush (Ctrl: the other mode; Alt: pick a colour)",
    options: [
      brushSize,
      hardness,
      opacity,
      {
        key: "type",
        type: "select",
        label: "Type",
        choices: [
          ["blur", "Blur"],
          ["sharpen", "Sharpen"],
        ],
        default: "blur",
      },
      { key: "rate", type: "slider", label: "Rate", min: 0, max: 100, default: 50 },
    ],
  },
  {
    id: "dodge-burn",
    name: "Dodge / Burn",
    icon: "tool_dodge",
    keys: ["Shift+D"],
    help: "Lighten (dodge) or darken (burn) with a brush (Ctrl: the other mode; Alt: pick a colour)",
    options: [
      brushSize,
      hardness,
      opacity,
      {
        key: "type",
        type: "select",
        label: "Type",
        choices: [
          ["dodge", "Dodge"],
          ["burn", "Burn"],
        ],
        default: "dodge",
      },
      {
        key: "range",
        type: "select",
        label: "Range",
        choices: [
          ["shadows", "Shadows"],
          ["midtones", "Midtones"],
          ["highlights", "Highlights"],
        ],
        default: "midtones",
      },
      { key: "exposure", type: "slider", label: "Exposure", min: 0, max: 100, default: 50 },
    ],
  },
  {
    id: "text",
    name: "Text",
    icon: "tool_text",
    keys: ["T"],
    help: "Click to type (drag: a fixed box), click a text layer to edit it. Escape ends",
    options: [
      { key: "font", type: "font", label: "Font", default: "Segoe UI" },
      {
        key: "fontSize",
        type: "slider",
        label: "Size",
        min: 1,
        max: 1000,
        default: 62,
        unit: "px",
      },
      { key: "antialias", type: "check", label: "Antialiasing", default: true },
      {
        key: "justify",
        type: "select",
        label: "Justify",
        choices: [
          ["left", "Left"],
          ["center", "Center"],
          ["right", "Right"],
          ["fill", "Filled"],
        ],
        default: "left",
      },
      {
        key: "indent",
        type: "slider",
        label: "Indent",
        min: -500,
        max: 500,
        default: 0,
        unit: "px",
      },
      {
        key: "lineSpacing",
        type: "slider",
        label: "Line spacing",
        min: -200,
        max: 500,
        default: 0,
        unit: "px",
      },
      {
        key: "letterSpacing",
        type: "slider",
        label: "Letter spacing",
        min: -50,
        max: 200,
        default: 0,
        unit: "px",
      },
      {
        key: "box",
        type: "select",
        label: "Box",
        choices: [
          ["dynamic", "Dynamic"],
          ["fixed", "Fixed"],
        ],
        default: "dynamic",
      },
    ],
  },
  {
    id: "color-picker",
    name: "Color Picker",
    icon: "tool_picker",
    keys: ["O"],
    help: "Set colours from image pixels as seen (left click: FG, right click: BG)",
    options: [
      // Picks what is seen by default (all visible layers); "sampleLayer" replaced the old
      // "sampleMerged" (off by default) so the saved value does not keep the old behaviour.
      { key: "sampleLayer", type: "check", label: "Active layer only", default: false },
      { key: "average", type: "check", label: "Sample average", default: false },
      { key: "radius", type: "slider", label: "Radius", min: 1, max: 300, default: 3, unit: "px" },
    ],
  },
];

/**
 * Toolbox sections, top to bottom: divider-separated blocks of buttons. Tools in one group
 * share a button that shows the last chosen one; holding the button opens the others
 * (GIMP 2.10 / Photoshop tool groups). The drawing tools form one block (owner request
 * 2026-11-11): Paintbrush, Bucket Fill, Gradient, Eraser, Color Picker, Text.
 */
export const TOOL_SECTIONS: string[][][] = [
  [["move"]],
  [["rect-select", "ellipse-select", "free-select"]],
  [["fuzzy-select", "by-color-select"]],
  [["rotate", "scale", "flip", "transform-3d", "handle-transform"]],
  [["paintbrush"], ["bucket-fill", "gradient"], ["eraser"], ["color-picker"], ["text"]],
  [["clone", "heal"]],
  [["smudge", "blur-sharpen", "dodge-burn"]],
];

export const TOOL_GROUPS: string[][] = TOOL_SECTIONS.flat();

/**
 * Brush tools share one size (owner decision 2026-10-14): changing it on one (slider, wheel,
 * `[` `]`) changes it on all of them. Their other options stay per tool.
 */
export const SHARED_SIZE_TOOLS = [
  "paintbrush",
  "eraser",
  "clone",
  "heal",
  "smudge",
  "blur-sharpen",
  "dodge-burn",
];

export function toolDef(id: string): ToolDef {
  return TOOLS.find((t) => t.id === id) ?? TOOLS[0]!;
}

/** A saved tool id that no longer exists (e.g. the removed Crop tool) falls back. */
function validTool(id: string): string {
  return TOOLS.some((t) => t.id === id) ? id : "paintbrush";
}

/** Whether an option is shown with the tool's current values (`onlyIf`). */
export function optionShown(o: ToolOptionDef, values: Record<string, OptionValue>): boolean {
  return !o.onlyIf || values[o.onlyIf[0]] === o.onlyIf[1];
}

function defaults(): Record<string, Record<string, OptionValue>> {
  const out: Record<string, Record<string, OptionValue>> = {};
  for (const t of TOOLS) out[t.id] = Object.fromEntries(t.options.map((o) => [o.key, o.default]));
  return out;
}

class Tools {
  active = $state<string>(validTool(loadSetting("active-tool", "paintbrush")));
  /** The Paintbrush icon follows its mode (the Pencil is a mode of it now). */
  icon(id: string): string {
    if (id === "paintbrush" && this.values.paintbrush?.mode === "pencil") return "tool_pencil";
    return toolDef(id).icon;
  }
  values = $state(shareSize(mergeSaved(defaults(), loadSetting("tool-options", {})), this.active));
  /** The tool each group button shows, keyed by the group's first tool id. */
  shownInGroup = $state<Record<string, string>>(loadSetting("tool-groups", {}));
  /** Similar tools share one button (Settings > Toolbox); off = every tool has its own. */
  grouped = $state<boolean>(loadSetting("toolbox-grouped", false));

  setGrouped(on: boolean) {
    this.grouped = on;
    saveSetting("toolbox-grouped", on);
  }

  get def(): ToolDef {
    return toolDef(this.active);
  }

  /** The tool a group button shows: the active one if it is in the group, else the last
   *  chosen one. */
  shown(group: string[]): string {
    if (group.includes(this.active)) return this.active;
    const saved = this.shownInGroup[group[0]!];
    return saved && group.includes(saved) ? saved : group[0]!;
  }

  select(id: string) {
    this.active = id;
    saveSetting("active-tool", id);
    const group = TOOL_GROUPS.find((g) => g.includes(id));
    if (group && group.length > 1) {
      this.shownInGroup[group[0]!] = id;
      saveSetting("tool-groups", $state.snapshot(this.shownInGroup));
    }
  }

  option<T extends OptionValue>(tool: string, key: string): T {
    return this.values[tool]![key] as T;
  }

  setOption(tool: string, key: string, value: OptionValue) {
    const shared = key === "size" && SHARED_SIZE_TOOLS.includes(tool);
    for (const t of shared ? SHARED_SIZE_TOOLS : [tool]) this.values[t]![key] = value;
    saveSetting("tool-options", $state.snapshot(this.values));
  }
}

function mergeSaved(
  base: Record<string, Record<string, OptionValue>>,
  saved: Record<string, Record<string, OptionValue>>,
) {
  for (const [tool, opts] of Object.entries(saved)) {
    if (!base[tool]) continue;
    for (const [k, v] of Object.entries(opts)) if (k in base[tool]!) base[tool]![k] = v;
  }
  return base;
}

/** Settings saved before the size was shared: every brush tool takes the size of the active
 *  one (or of the Paintbrush). */
function shareSize(values: Record<string, Record<string, OptionValue>>, active: string) {
  const from = SHARED_SIZE_TOOLS.includes(active) ? active : "paintbrush";
  for (const t of SHARED_SIZE_TOOLS) values[t]!.size = values[from]!.size!;
  return values;
}

export const tools = new Tools();
