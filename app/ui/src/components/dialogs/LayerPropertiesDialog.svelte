<script lang="ts">
  // Layer Properties: name, opacity, offsets and the flags. Opacity, visibility and the
  // offsets show on the canvas at once (the UI mirror only); OK applies them as one undo
  // step, Cancel puts everything back.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Slider from "../Slider.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import type { Doc, LayerInfo, LayerProps } from "../../lib/documents.svelte";

  let { close, doc, layer }: { doc: Doc; layer: LayerInfo } & DialogProps<LayerProps> = $props();

  // The dialog edits a copy of the layer's values at the time it opened.
  const l = untrack(() => layer);
  let name = $state(l.name);
  let opacity = $state(Math.round(l.opacity * 1000) / 10);
  let x = $state(l.x);
  let y = $state(l.y);
  let visible = $state(l.visible);
  let lockAlpha = $state(l.lockAlpha);
  let lockPosition = $state(l.lockPosition);

  // Live preview on the canvas.
  $effect(() => {
    const patch = { opacity: opacity / 100, visible, x: Math.round(x) || 0, y: Math.round(y) || 0 };
    const d = untrack(() => doc);
    d.layers = d.layers.map((it) => (it.id === l.id ? { ...it, ...patch } : it));
    d.revision++;
  });

  function cancel() {
    doc.sync();
    close();
  }

  function submit() {
    close({
      name: name.trim() || l.name,
      opacity: opacity / 100,
      x: Math.round(x) || 0,
      y: Math.round(y) || 0,
      visible,
      lockAlpha,
      lockPosition,
    });
  }
</script>

<Dialog title="Layer Properties" width={340} preview onsubmit={submit} oncancel={cancel}>
  <label class="row">
    <span class="label">Name</span>
    <input class="name" type="text" bind:value={name} data-autofocus />
  </label>
  <Slider label="Opacity" bind:value={opacity} min={0} max={100} step={0.1} unit="%" />
  <div class="row">
    <span class="label">Offset</span>
    <span class="offsets">
      X <input type="number" bind:value={x} />
      Y <input type="number" bind:value={y} />
    </span>
  </div>
  <label class="check"><input type="checkbox" bind:checked={visible} /> Visible</label>
  <label class="check"><input type="checkbox" bind:checked={lockAlpha} /> Lock alpha channel</label>
  <label class="check"><input type="checkbox" bind:checked={lockPosition} /> Lock position</label>
  {#snippet buttons()}
    <button type="button" onclick={cancel}>Cancel</button>
    <button type="submit" class="primary">OK</button>
  {/snippet}
</Dialog>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .label {
    color: var(--fg-dim);
    min-width: 48px;
  }
  .name {
    flex: 1;
    height: 24px;
    padding: 0 6px;
  }
  .offsets {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-faint);
  }
  .offsets input {
    width: 76px;
    height: 24px;
    padding: 0 6px;
    text-align: right;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
</style>
