<script lang="ts" module>
  export interface ScaleResult {
    width: number;
    height: number;
    /** 0 none, 1 linear, 2 cubic, 3 Lanczos (the core's order). */
    interpolation: number;
  }
</script>

<script lang="ts">
  // Image > Scale Image: new size in pixels or percent, linked by the chain (keep the
  // aspect ratio), and the interpolation (remembered; GIMP's default is Cubic).
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Icon from "../Icon.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";

  let {
    close,
    width: w0,
    height: h0,
  }: { width: number; height: number } & DialogProps<ScaleResult> = $props();

  const MAX = 32768;
  const ow = untrack(() => w0);
  const oh = untrack(() => h0);
  let width = $state(ow);
  let height = $state(oh);
  let linked = $state(true);
  let interpolation = $state(loadSetting("scale-interpolation", 2));
  let valid = $derived(width >= 1 && height >= 1 && width <= MAX && height <= MAX);

  function setWidth(v: number) {
    width = v;
    if (linked) height = Math.max(1, Math.round((v * oh) / ow));
  }
  function setHeight(v: number) {
    height = v;
    if (linked) width = Math.max(1, Math.round((v * ow) / oh));
  }
  const pct = (v: number, o: number) => Math.round((v / o) * 10000) / 100;

  function submit() {
    if (!valid) return;
    saveSetting("scale-interpolation", interpolation);
    close({ width: Math.round(width), height: Math.round(height), interpolation });
  }
</script>

<Dialog title="Scale Image" width={340} onsubmit={submit} oncancel={() => close()}>
  <div class="grid">
    <label for="si-w">Width</label>
    <input
      id="si-w"
      type="number"
      min="1"
      max={MAX}
      value={width}
      oninput={(e) => setWidth(e.currentTarget.valueAsNumber || 0)}
    />
    <span class="dim">px</span>
    <input
      type="number"
      min="0.01"
      step="any"
      value={pct(width, ow)}
      aria-label="Width in percent"
      oninput={(e) =>
        setWidth(Math.max(1, Math.round((ow * (e.currentTarget.valueAsNumber || 0)) / 100)))}
    />
    <span class="dim">%</span>
    <button
      type="button"
      class="chain"
      class:on={linked}
      title={linked ? "Keep the aspect ratio" : "Width and height are independent"}
      onclick={() => (linked = !linked)}
    >
      <Icon name={linked ? "link" : "link_off"} size={16} />
    </button>

    <label for="si-h">Height</label>
    <input
      id="si-h"
      type="number"
      min="1"
      max={MAX}
      value={height}
      oninput={(e) => setHeight(e.currentTarget.valueAsNumber || 0)}
    />
    <span class="dim">px</span>
    <input
      type="number"
      min="0.01"
      step="any"
      value={pct(height, oh)}
      aria-label="Height in percent"
      oninput={(e) =>
        setHeight(Math.max(1, Math.round((oh * (e.currentTarget.valueAsNumber || 0)) / 100)))}
    />
    <span class="dim">%</span>
    <span></span>

    <label for="si-i">Interpolation</label>
    <select id="si-i" class="wide" bind:value={interpolation}>
      <option value={0}>None</option>
      <option value={1}>Linear</option>
      <option value={2}>Cubic</option>
      <option value={3}>Lanczos (sharpest)</option>
    </select>
  </div>
  <div class="dim">Current size: {ow} × {oh} px</div>
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>Scale</button>
  {/snippet}
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 80px auto 70px auto auto;
    align-items: center;
    gap: 8px 6px;
  }
  label {
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
  .wide {
    grid-column: 2 / span 4;
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
