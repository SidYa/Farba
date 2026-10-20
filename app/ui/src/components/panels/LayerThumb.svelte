<script lang="ts">
  // Layer thumbnail (60 x 40 box, the layer fitted inside over a checkerboard), redrawn
  // whenever the document changes.
  import type { Doc, LayerInfo } from "../../lib/documents.svelte";

  const BOX_W = 60;
  const BOX_H = 40;

  let { doc, layer }: { doc: Doc; layer: LayerInfo } = $props();

  let canvas: HTMLCanvasElement;
  let size = $derived.by(() => {
    const s = Math.min(BOX_W / layer.width, BOX_H / layer.height);
    return {
      w: Math.max(1, Math.round(layer.width * s)),
      h: Math.max(1, Math.round(layer.height * s)),
    };
  });

  $effect(() => {
    void doc.revision;
    const { w, h } = size;
    let pixels: Uint8Array;
    try {
      pixels = doc.core.thumbnail(layer.id, w, h);
    } catch {
      return; // the layer was just removed
    }
    canvas.width = w;
    canvas.height = h;
    canvas
      .getContext("2d")!
      .putImageData(new ImageData(Uint8ClampedArray.from(pixels), w, h), 0, 0);
  });
</script>

<div class="box">
  <canvas bind:this={canvas} style:width="{size.w}px" style:height="{size.h}px"></canvas>
</div>

<style>
  .box {
    width: 62px;
    height: 42px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  canvas {
    display: block;
    outline: 1px solid var(--border-soft);
    background: repeating-conic-gradient(var(--check-a) 0 25%, var(--check-b) 0 50%) 0 0 / 8px 8px;
  }
</style>
