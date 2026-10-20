<script lang="ts" module>
  export interface AdjustSlider {
    key: string;
    label: string;
    min: number;
    max: number;
    step: number;
    default: number;
  }
  export interface AdjustSelect {
    key: string;
    label: string;
    choices: [number, string][];
    default: number;
  }
  /** One adjustment: its controls and how they map to the core's `(kind, a, b)`. */
  export interface AdjustSpec {
    /** Settings key for the last used values. */
    id: string;
    title: string;
    kind: number;
    sliders?: AdjustSlider[];
    select?: AdjustSelect;
    /** Core parameters from the control values. */
    params: (v: Record<string, number>) => number[];
  }
</script>

<script lang="ts">
  // Colour adjustment or effect with a live preview on the canvas (Exposure, Saturation,
  // Desaturate, the Effects menu).
  // The core shows the result on the active layer without recording it; OK makes it one
  // undo step, Cancel restores the layer. The last values are remembered.
  import { untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import Slider from "../Slider.svelte";
  import type { Doc } from "../../lib/documents.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";
  import { FilterPreview, whileBusy } from "../../lib/native";
  import { notify } from "../../lib/notify.svelte";
  import { errorText } from "../../lib/documents.svelte";

  let { close, doc, spec }: { doc: Doc; spec: AdjustSpec } & DialogProps<boolean> = $props();

  const s = untrack(() => spec);
  const defaults: Record<string, number> = Object.fromEntries([
    ...(s.sliders ?? []).map((o) => [o.key, o.default] as const),
    ...(s.select ? [[s.select.key, s.select.default] as const] : []),
  ]);
  let values = $state<Record<string, number>>({
    ...defaults,
    ...loadSetting<Record<string, number>>(`adjust-${s.id}`, {}),
  });
  let preview = $state(loadSetting("adjust-preview", true));
  let timer: ReturnType<typeof setTimeout> | undefined;
  // Large layers are filtered by the backend on all cores (the UI stays responsive).
  const filter = untrack(() => new FilterPreview(doc));
  const params = () => s.params(values);

  function show() {
    // Large images take a while per preview: wait until the slider rests for a moment, then
    // show the latest values (one preview, not one per mouse move).
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (preview) filter.show(s.kind, params()).catch(failed);
      else {
        doc.core.filter_cancel();
        doc.sync();
      }
    }, 150);
  }

  $effect(() => {
    void JSON.stringify(values);
    void preview;
    show();
  });

  function failed(e: unknown) {
    notify(errorText(e), "error");
  }

  async function ok() {
    clearTimeout(timer);
    try {
      await whileBusy(`Applying ${s.title}…`, () => filter.show(s.kind, params()));
    } catch (e) {
      failed(e);
      return;
    }
    filter.end();
    doc.core.filter_commit(s.title);
    doc.sync();
    saveSetting(`adjust-${s.id}`, $state.snapshot(values));
    saveSetting("adjust-preview", preview);
    close(true);
  }

  function cancel() {
    clearTimeout(timer);
    filter.end();
    doc.core.filter_cancel();
    doc.sync();
    close();
  }
</script>

<Dialog title={s.title} width={320} preview onsubmit={ok} oncancel={cancel}>
  {#if s.select}
    <label class="row">
      <span class="label">{s.select.label}</span>
      <select bind:value={values[s.select.key]}>
        {#each s.select.choices as [v, label] (v)}<option value={v}>{label}</option>{/each}
      </select>
    </label>
  {/if}
  {#each s.sliders ?? [] as o (o.key)}
    <Slider label={o.label} bind:value={values[o.key]!} min={o.min} max={o.max} step={o.step} />
  {/each}
  <label class="check">
    <input type="checkbox" bind:checked={preview} />
    <span>Preview</span>
  </label>
  {#snippet buttons()}
    <button type="button" onclick={() => (values = { ...defaults })}> Reset </button>
    <span class="grow"></span>
    <button type="button" onclick={cancel}>Cancel</button>
    <button type="submit" class="primary">OK</button>
  {/snippet}
</Dialog>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .label {
    color: var(--fg-dim);
  }
  select {
    height: 24px;
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
