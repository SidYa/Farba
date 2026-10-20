<script lang="ts" module>
  export interface PdfExportSettings {
    resolution: number;
    vectorize: boolean;
    ignoreHidden: boolean;
    fillBackground: boolean;
    layersAsPages: boolean;
    reverseOrder: boolean;
    noInterpolation: boolean;
  }

  export const PDF_EXPORT_DEFAULTS: PdfExportSettings = {
    resolution: 300,
    vectorize: true,
    ignoreHidden: true,
    fillBackground: true,
    layersAsPages: false,
    reverseOrder: false,
    noInterpolation: true,
  };
</script>

<script lang="ts">
  // Export Image as PDF (GIMP's options, as in Pinta-plus, plus the resolution and "Do not
  // smooth pixels"). Every layer is its own object in the file; nothing is lost.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";

  let {
    close,
    name,
    width,
    height,
    layers,
    initial,
  }: {
    name: string;
    width: number;
    height: number;
    layers: number;
    initial: PdfExportSettings;
  } & DialogProps<PdfExportSettings> = $props();

  const o = $state({ ...untrack(() => initial) });
  const many = untrack(() => layers) > 1;
  let valid = $derived(o.resolution >= 5 && o.resolution <= 65536);
  const mm = (px: number) => ((px * 25.4) / o.resolution).toFixed(1).replace(/\.0$/, "");

  function submit() {
    if (!valid) return;
    close({ ...o, layersAsPages: o.layersAsPages && many });
  }
</script>

<Dialog title="Export Image as PDF" width={400} onsubmit={submit} oncancel={() => close()}>
  <div class="file">{name}</div>
  <div class="row">
    <label for="pdf-res">Resolution</label>
    <input id="pdf-res" type="number" min="5" max="65536" step="any" bind:value={o.resolution} />
    <span class="dim">pixels/in</span>
  </div>
  <div class="dim">Page size: {mm(width)} × {mm(height)} mm</div>
  <label class="check">
    <input type="checkbox" bind:checked={o.vectorize} /> Convert bitmaps to vector graphics where possible
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={o.ignoreHidden} /> Omit hidden layers and layers with zero opacity
  </label>
  <label class="check" title="The background color is the secondary color of the palette">
    <input type="checkbox" bind:checked={o.fillBackground} /> Fill transparent areas with background color
  </label>
  <label
    class="check"
    title="Zooming in on the PDF shows sharp pixels, as in the editor. The image data is lossless either way."
  >
    <input type="checkbox" bind:checked={o.noInterpolation} /> Do not smooth pixels when zoomed in viewers
  </label>
  <label class="check" class:off={!many} title="Layers as pages (bottom layers first).">
    <input type="checkbox" disabled={!many} bind:checked={o.layersAsPages} /> Layers as pages
  </label>
  <label
    class="check indent"
    class:off={!many || !o.layersAsPages}
    title="Reverse the pages order (top layers first)."
  >
    <input type="checkbox" disabled={!many || !o.layersAsPages} bind:checked={o.reverseOrder} />
    Reverse order
  </label>
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>Export</button>
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
    color: var(--fg-faint);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row label {
    color: var(--fg-dim);
  }
  .row input {
    width: 90px;
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
  .indent {
    margin-left: 24px;
  }
  .off {
    opacity: 0.5;
  }
</style>
