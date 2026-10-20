<script lang="ts">
  // Export quality of the lossy formats: JPEG (default 100, as the owner exports from GIMP),
  // HEIC and AVIF (default 85, as in Pinta-plus). No metadata is saved.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Slider from "../Slider.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";

  let {
    close,
    title,
    name,
    quality: initial,
  }: { title: string; name: string; quality: number } & DialogProps<number> = $props();

  let quality = $state(untrack(() => initial));
</script>

<Dialog {title} width={320} onsubmit={() => close(quality)} oncancel={() => close()}>
  <div class="file">{name}</div>
  <Slider label="Quality" bind:value={quality} min={0} max={100} />
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary">Export</button>
  {/snippet}
</Dialog>

<style>
  .file {
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
