<script lang="ts">
  // GIMP's on-canvas text style editor: a small bar above the text box that styles the
  // selected fragment (or the text typed next when nothing is selected): font, size,
  // colour, bold / italic / underline / strikethrough, baseline offset, kerning, and
  // "clear style". After each change the keyboard goes back to the text.
  import Icon from "../Icon.svelte";
  import { textTool } from "../../lib/canvas/texttool.svelte";
  import FontPicker from "../FontPicker.svelte";
  import type { SpanStyle } from "../../lib/text/model";

  let { left, top }: { left: number; top: number } = $props();

  let edit = $derived(textTool.edit);
  let style = $derived.by(() => {
    void edit;
    return textTool.currentStyle;
  });
  let font = $derived(style.font ?? edit?.spec.font ?? "");
  let size = $derived(style.size ?? edit?.spec.size ?? 0);
  let color = $derived(style.color ?? edit?.spec.color ?? [0, 0, 0, 255]);
  let bold = $derived(style.bold ?? edit?.spec.bold ?? false);
  let italic = $derived(style.italic ?? edit?.spec.italic ?? false);

  const hex = (c: number[]) =>
    "#" +
    c
      .slice(0, 3)
      .map((v) => v.toString(16).padStart(2, "0"))
      .join("");

  function apply(patch: SpanStyle) {
    textTool.style(patch);
    textTool.focus();
  }

  // The Move button: drag it to move the text box.
  let drag: { sx: number; sy: number; x: number; y: number } | undefined;
  function startMove(e: PointerEvent) {
    const ed = textTool.edit;
    if (!ed || e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { sx: e.clientX, sy: e.clientY, x: ed.x, y: ed.y };
  }
  function moveTo(e: PointerEvent) {
    const ed = textTool.edit;
    if (!drag || !ed) return;
    const k = devicePixelRatio / ed.doc.view.zoom;
    textTool.moveBox(drag.x + (e.clientX - drag.sx) * k, drag.y + (e.clientY - drag.sy) * k);
  }
  function endMove() {
    drag = undefined;
    textTool.focus();
  }

  function number(e: Event): number | undefined {
    const v = (e.currentTarget as HTMLInputElement).valueAsNumber;
    return Number.isFinite(v) ? v : undefined;
  }
</script>

<div
  class="editor"
  style:left="{left}px"
  style:top="{top - 34}px"
  role="toolbar"
  tabindex="-1"
  aria-label="Text style"
  onpointerdown={(e) => e.stopPropagation()}
>
  <FontPicker value={font} title="Font of the selected text" onchange={(f) => apply({ font: f })} />
  <input
    class="num"
    type="number"
    min="1"
    max="1000"
    title="Size of the selected text (px)"
    value={Math.round(size * 10) / 10}
    onchange={(e) => {
      const v = number(e);
      if (v && v > 0) apply({ size: v });
    }}
  />
  <input
    class="color"
    type="color"
    title="Colour of the selected text"
    value={hex(color)}
    onchange={(e) => {
      const h = e.currentTarget.value;
      const c = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
      apply({ color: [c[0]!, c[1]!, c[2]!, 255] });
    }}
  />
  <span class="sep"></span>
  <button class:on={bold} title="Bold (Ctrl+B)" onclick={() => apply({ bold: !bold })}>
    <Icon name="format_bold" size={16} />
  </button>
  <button class:on={italic} title="Italic (Ctrl+I)" onclick={() => apply({ italic: !italic })}>
    <Icon name="format_italic" size={16} />
  </button>
  <button
    class:on={style.underline}
    title="Underline (Ctrl+U)"
    onclick={() => apply({ underline: !style.underline })}
  >
    <Icon name="format_underlined" size={16} />
  </button>
  <button
    class:on={style.strike}
    title="Strikethrough"
    onclick={() => apply({ strike: !style.strike })}
  >
    <Icon name="strikethrough_s" size={16} />
  </button>
  <span class="sep"></span>
  <label title="Baseline offset of the selected text (px)">
    <Icon name="vertical_align_center" size={16} />
    <input
      class="num"
      type="number"
      step="1"
      value={style.baseline ?? 0}
      onchange={(e) => apply({ baseline: number(e) || undefined })}
    />
  </label>
  <label title="Kerning of the selected text (px)">
    <Icon name="format_letter_spacing" size={16} />
    <input
      class="num"
      type="number"
      step="0.5"
      value={style.kerning ?? 0}
      onchange={(e) => apply({ kerning: number(e) || undefined })}
    />
  </label>
  <button
    title="Clear the style of the selected text"
    onclick={() => {
      textTool.clearStyle();
      textTool.focus();
    }}
  >
    <Icon name="format_clear" size={16} />
  </button>
  <span class="sep"></span>
  <button
    class="move"
    title="Move the text (drag)"
    onpointerdown={startMove}
    onpointermove={moveTo}
    onpointerup={endMove}
    onpointercancel={endMove}
  >
    <Icon name="open_with" size={16} />
  </button>
</div>

<style>
  .editor {
    position: absolute;
    box-sizing: border-box;
    height: 34px;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 4px 6px;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 10px;
    box-shadow: var(--shadow);
    white-space: nowrap;
  }
  .num {
    width: 52px;
    height: 24px;
    padding: 0 4px;
    text-align: right;
  }
  .color {
    width: 28px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: transparent;
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
  }
  .move {
    cursor: move;
  }
  button:hover {
    background: var(--hover);
  }
  button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--fg-dim);
  }
  .sep {
    width: 1px;
    height: 18px;
    margin: 0 3px;
    background: var(--border-soft);
  }
</style>
