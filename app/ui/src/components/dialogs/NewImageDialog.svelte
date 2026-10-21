<script lang="ts" module>
  export type NewImageFill = "background" | "foreground" | "white" | "transparent";
  export interface NewImageResult {
    width: number;
    height: number;
    fill: NewImageFill;
  }
</script>

<script lang="ts">
  // File > New: image size and the fill of the background layer. The last values are
  // remembered.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { numberField } from "../../lib/numberfield";
  import { loadSetting, saveSetting } from "../../lib/settings";

  let {
    close,
    initial,
  }: { initial?: { width: number; height: number } } & DialogProps<NewImageResult> = $props();

  const MAX = 32768;
  const last = loadSetting<NewImageResult>("new-image", {
    width: 1920,
    height: 1080,
    fill: "background",
  });
  const start = untrack(() => initial) ?? last;
  let width = $state(start.width);
  let height = $state(start.height);
  let fill = $state<NewImageFill>(last.fill);
  let valid = $derived(width >= 1 && height >= 1 && width <= MAX && height <= MAX);

  function submit() {
    if (!valid) return;
    const result = { width: Math.round(width), height: Math.round(height), fill };
    saveSetting("new-image", result);
    close(result);
  }
</script>

<Dialog title="Create a New Image" width={320} onsubmit={submit} oncancel={() => close()}>
  <div class="grid">
    <label for="ni-w">Width</label>
    <span
      ><input
        id="ni-w"
        use:numberField={{
          value: width,
          min: 1,
          max: MAX,
          oninput: (v) => (width = Math.round(v)),
        }}
      /> px</span
    >
    <label for="ni-h">Height</label>
    <span
      ><input
        id="ni-h"
        use:numberField={{
          value: height,
          min: 1,
          max: MAX,
          oninput: (v) => (height = Math.round(v)),
        }}
      /> px</span
    >
    <label for="ni-fill">Fill with</label>
    <select id="ni-fill" bind:value={fill}>
      <option value="background">Background colour</option>
      <option value="foreground">Foreground colour</option>
      <option value="white">White</option>
      <option value="transparent">Transparency</option>
    </select>
  </div>
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>OK</button>
  {/snippet}
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 8px 12px;
  }
  label {
    color: var(--fg-dim);
  }
  span {
    color: var(--fg-faint);
  }
  input {
    width: 90px;
    height: 24px;
    padding: 0 6px;
    text-align: right;
  }
  select {
    height: 24px;
  }
</style>
