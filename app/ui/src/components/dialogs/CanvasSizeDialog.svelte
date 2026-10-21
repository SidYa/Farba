<script lang="ts" module>
  export interface CanvasResult {
    width: number;
    height: number;
    /** Where the old image lands on the new canvas. */
    x: number;
    y: number;
    /** 0 no layers, 1 image-sized layers, 2 all layers (the core's order). */
    resize: number;
  }
</script>

<script lang="ts">
  // Image > Canvas Size: new size (optionally linked), where the image sits on it (Pinta's
  // 3x3 anchor or exact offsets, as GIMP), and which layers grow with the canvas (GIMP's
  // "Resize layers"; default: image-sized layers, so the background follows the canvas).
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Icon from "../Icon.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { numberField } from "../../lib/numberfield";
  import { loadSetting, saveSetting } from "../../lib/settings";

  let {
    close,
    width: w0,
    height: h0,
  }: { width: number; height: number } & DialogProps<CanvasResult> = $props();

  const MAX = 32768;
  const ow = untrack(() => w0);
  const oh = untrack(() => h0);
  let width = $state(ow);
  let height = $state(oh);
  let linked = $state(false);
  /** Anchor column / row 0..2 (centre = 1, 1), or undefined after editing the offsets. */
  let anchor = $state<[number, number] | undefined>([1, 1]);
  let x = $state(0);
  let y = $state(0);
  let resize = $state(loadSetting("canvas-resize-layers", 1));
  let valid = $derived(width >= 1 && height >= 1 && width <= MAX && height <= MAX);

  // The anchor keeps the offsets in step with the size.
  $effect(() => {
    if (!anchor) return;
    x = Math.round(((width - ow) * anchor[0]) / 2);
    y = Math.round(((height - oh) * anchor[1]) / 2);
  });

  function setWidth(v: number) {
    width = v;
    if (linked) height = Math.max(1, Math.round((v * oh) / ow));
  }
  function setHeight(v: number) {
    height = v;
    if (linked) width = Math.max(1, Math.round((v * ow) / oh));
  }

  function submit() {
    if (!valid) return;
    saveSetting("canvas-resize-layers", resize);
    close({ width: Math.round(width), height: Math.round(height), x, y, resize });
  }
</script>

<Dialog title="Canvas Size" width={340} onsubmit={submit} oncancel={() => close()}>
  <div class="grid">
    <label for="cs-w">Width</label>
    <input
      id="cs-w"
      use:numberField={{ value: width, min: 1, max: MAX, oninput: (v) => setWidth(Math.round(v)) }}
    />
    <span class="dim">px</span>
    <button
      type="button"
      class="chain"
      class:on={linked}
      title={linked ? "Keep the aspect ratio" : "Width and height are independent"}
      onclick={() => (linked = !linked)}
    >
      <Icon name={linked ? "link" : "link_off"} size={16} />
    </button>
    <label for="cs-h">Height</label>
    <input
      id="cs-h"
      use:numberField={{
        value: height,
        min: 1,
        max: MAX,
        oninput: (v) => setHeight(Math.round(v)),
      }}
    />
    <span class="dim">px</span>
    <span></span>
  </div>

  <div class="place">
    <div class="anchor" role="radiogroup" aria-label="Anchor">
      {#each [0, 1, 2] as row (row)}
        {#each [0, 1, 2] as col (col)}
          <button
            type="button"
            role="radio"
            aria-checked={anchor?.[0] === col && anchor?.[1] === row}
            class:on={anchor?.[0] === col && anchor?.[1] === row}
            title="Anchor"
            aria-label="Anchor column {col + 1}, row {row + 1}"
            onclick={() => (anchor = [col, row])}
          ></button>
        {/each}
      {/each}
    </div>
    <div class="grid offsets">
      <label for="cs-x">Offset X</label>
      <input
        id="cs-x"
        use:numberField={{
          value: x,
          oninput: (v) => {
            anchor = undefined;
            x = Math.round(v);
          },
        }}
      />
      <label for="cs-y">Offset Y</label>
      <input
        id="cs-y"
        use:numberField={{
          value: y,
          oninput: (v) => {
            anchor = undefined;
            y = Math.round(v);
          },
        }}
      />
    </div>
  </div>

  <label class="row">
    <span class="label">Resize layers</span>
    <select bind:value={resize}>
      <option value={0}>None</option>
      <option value={1}>Image-sized layers</option>
      <option value={2}>All layers</option>
    </select>
  </label>
  <div class="dim">Current size: {ow} × {oh} px</div>
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>Resize</button>
  {/snippet}
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 90px auto auto;
    align-items: center;
    gap: 8px 6px;
  }
  .offsets {
    grid-template-columns: auto 80px;
  }
  label,
  .label {
    color: var(--fg-dim);
    margin-right: 6px;
  }
  .dim {
    color: var(--fg-faint);
  }
  input {
    width: 100%;
    height: 24px;
    padding: 0 6px;
    text-align: right;
  }
  select {
    height: 24px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .place {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .anchor {
    display: grid;
    grid-template-columns: repeat(3, 22px);
    gap: 3px;
  }
  .anchor button {
    width: 22px;
    height: 22px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  .anchor button.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .chain {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: transparent;
  }
  .chain.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
</style>
