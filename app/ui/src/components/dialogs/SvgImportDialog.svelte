<script lang="ts">
  // GIMP's "Render Scalable Vector Graphics": the size to render an SVG at, as width /
  // height / resolution linked through the document size (in CSS pixels, 96 per inch; the
  // aspect ratio is kept). The resolution is remembered.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";

  let {
    close,
    name,
    size,
  }: { name: string; size: [number, number] } & DialogProps<[number, number]> = $props();

  const MAX = 32768;
  const [sw, sh] = untrack(() => size);
  let resolution = $state(loadSetting("svg-resolution", 96));
  const px = (v: number, r: number) => Math.max(1, Math.round((v * r) / 96));
  let width = $derived(px(sw, resolution));
  let height = $derived(px(sh, resolution));
  let valid = $derived(resolution > 0 && width <= MAX && height <= MAX);

  function submit() {
    if (!valid) return;
    saveSetting("svg-resolution", resolution);
    close([width, height]);
  }
</script>

<Dialog
  title="Render Scalable Vector Graphics"
  width={360}
  onsubmit={submit}
  oncancel={() => close()}
>
  <div class="file">{name}</div>
  <div class="grid">
    <label for="svg-w">Width</label>
    <input
      id="svg-w"
      type="number"
      min="1"
      max={MAX}
      value={width}
      onchange={(e) => (resolution = ((e.currentTarget.valueAsNumber || 1) * 96) / sw)}
    />
    <span class="dim">pixels</span>
    <label for="svg-h">Height</label>
    <input
      id="svg-h"
      type="number"
      min="1"
      max={MAX}
      value={height}
      onchange={(e) => (resolution = ((e.currentTarget.valueAsNumber || 1) * 96) / sh)}
    />
    <span class="dim">pixels</span>
    <label for="svg-res">Resolution</label>
    <input
      id="svg-res"
      type="number"
      min="1"
      max="65536"
      step="any"
      value={Math.round(resolution * 100) / 100}
      onchange={(e) => (resolution = Math.max(1, e.currentTarget.valueAsNumber || 96))}
    />
    <span class="dim">pixels/in</span>
  </div>
  <div class="dim">
    Document size: {Math.round(sw * 100) / 100} × {Math.round(sh * 100) / 100} px at 96 pixels/in
  </div>
  {#if !valid}<div class="error">The image would be larger than {MAX} pixels per side.</div>{/if}
  {#snippet buttons()}
    <button type="button" onclick={() => (resolution = 96)}>Reset</button>
    <span class="grow"></span>
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>OK</button>
  {/snippet}
</Dialog>

<style>
  .file {
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dim {
    color: var(--fg-dim);
  }
  .error {
    color: var(--danger);
  }
  .grid {
    display: grid;
    grid-template-columns: auto 90px auto;
    align-items: center;
    gap: 6px 8px;
    justify-content: start;
  }
  .grid label {
    color: var(--fg-dim);
  }
  .grid input {
    height: 24px;
    padding: 0 6px;
    text-align: right;
  }
</style>
