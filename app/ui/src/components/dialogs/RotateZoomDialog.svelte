<script lang="ts">
  // Layers > Rotate / Zoom (Pinta's dialog): angle, zoom and pan of the active layer (or
  // its selected part) around its centre, previewed on the canvas through the transform
  // tools' session. OK applies it as one undo step (rendered from the original pixels, so
  // it loses no quality over earlier transforms).
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Slider from "../Slider.svelte";
  import { transformTools } from "../../lib/canvas/transformtools.svelte";
  import type { Doc } from "../../lib/documents.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";

  let { close, doc }: { doc: Doc } & DialogProps<boolean> = $props();

  const d = untrack(() => doc);
  const session = transformTools.start(d, "rotate");
  if (session) session.label = "Rotate / Zoom Layer";
  let angle = $state(0);
  let zoom = $state(100);
  let panX = $state(0);
  let panY = $state(0);

  $effect(() => {
    if (!session) return;
    const [x, y, w, h] = session.rect;
    const cx = x + w / 2;
    const cy = y + h / 2;
    const a = (angle * Math.PI) / 180;
    const k = zoom / 100;
    const c = Math.cos(a) * k;
    const s = Math.sin(a) * k;
    // Around the centre, then moved by the pan.
    session.m = [c, -s, cx + panX - c * cx + s * cy, s, c, cy + panY - s * cx - c * cy, 0, 0, 1];
    d.revision++;
  });

  // Nothing to transform (e.g. the layer is locked): close at once.
  $effect(() => {
    if (!session) close();
  });

  function ok() {
    transformTools.apply();
    close(true);
  }

  function cancel() {
    transformTools.cancel();
    close();
  }
</script>

<Dialog title="Rotate / Zoom Layer" width={320} preview onsubmit={ok} oncancel={cancel}>
  <Slider label="Angle" bind:value={angle} min={-180} max={180} step={0.1} unit="°" />
  <Slider label="Zoom" bind:value={zoom} min={1} max={1600} step={1} unit="%" />
  <Slider label="Pan X" bind:value={panX} min={-4000} max={4000} step={1} unit="px" />
  <Slider label="Pan Y" bind:value={panY} min={-4000} max={4000} step={1} unit="px" />
  {#snippet buttons()}
    <button
      type="button"
      onclick={() => {
        angle = 0;
        zoom = 100;
        panX = panY = 0;
      }}>Reset</button
    >
    <span class="grow"></span>
    <button type="button" onclick={cancel}>Cancel</button>
    <button type="submit" class="primary">OK</button>
  {/snippet}
</Dialog>
