<script lang="ts" module>
  export interface PdfImportResult {
    /** 0-based, in document order. */
    pages: number[];
    target: "layers" | "images";
    /** Pixels per inch. */
    resolution: number;
    white: boolean;
    reverse: boolean;
  }
</script>

<script lang="ts">
  // Load PDF (GIMP's / Pinta-plus' page selector): page thumbnails (click selects one,
  // Ctrl+click toggles, Shift+click selects a run, double click opens), the range text
  // ("1,3,5-7,9-"), pages as layers or images, and the size as width / height / resolution
  // linked through the first page. The choices are remembered.
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy, untrack } from "svelte";
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { formatPageRange, parsePageRange } from "../../lib/pdfrange";
  import { numberField } from "../../lib/numberfield";
  import { loadSetting, saveSetting } from "../../lib/settings";

  let {
    close,
    name,
    session,
    pages: sizes,
    layersOnly = false,
  }: {
    name: string;
    session: number;
    /** Page sizes in points. */
    pages: [number, number][];
    /** Adding to an open image: pages always become layers. */
    layersOnly?: boolean;
  } & DialogProps<PdfImportResult> = $props();

  const THUMB = 120;
  const MAX = 32768;
  const count = untrack(() => sizes.length);
  const [pw, ph] = untrack(() => sizes[0]!);
  const saved = loadSetting("pdf-import", {
    target: "layers" as "layers" | "images",
    resolution: 300,
    white: true,
    reverse: false,
  });

  let selected = $state(new Set<number>(Array.from({ length: count }, (_, i) => i)));
  let anchor = 0;
  let target = $state(untrack(() => (layersOnly ? "layers" : saved.target)));
  let resolution = $state(saved.resolution);
  let white = $state(saved.white);
  let reverse = $state(saved.reverse);
  let range = $state(formatPageRange(untrack(() => selected)));
  let thumbs = $state<(string | undefined)[]>(Array(count).fill(undefined));

  const px = (pt: number, res: number) => Math.ceil((pt * res) / 72 - 1e-6);
  let width = $derived(px(pw, resolution));
  let height = $derived(px(ph, resolution));
  let valid = $derived(
    selected.size > 0 && resolution > 0 && width <= MAX && height <= MAX && width >= 1,
  );
  let countText = $derived(
    selected.size === 0
      ? "Nothing selected"
      : selected.size === 1
        ? "One page selected"
        : selected.size === count
          ? `All ${count} pages selected`
          : `Pages selected: ${selected.size}`,
  );

  function setSelected(s: Set<number>) {
    selected = s;
    range = formatPageRange(s);
  }

  function pick(i: number, e: MouseEvent) {
    if (e.shiftKey) {
      const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
      setSelected(new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k)));
      return;
    }
    anchor = i;
    if (e.ctrlKey) {
      const s = new Set(selected);
      if (s.has(i)) s.delete(i);
      else s.add(i);
      setSelected(s);
    } else setSelected(new Set([i]));
  }

  function applyRange() {
    setSelected(new Set(parsePageRange(range, count)));
  }

  // Thumbnails, one at a time in page order; restarted when the background changes.
  let generation = 0;
  let urls: string[] = [];
  async function loadThumbs(bg: boolean) {
    const gen = ++generation;
    for (let i = 0; i < count; i++) {
      const [w, h] = sizes[i]!;
      const s = THUMB / Math.max(w, h);
      let png: ArrayBuffer;
      try {
        png = await invoke<ArrayBuffer>("pdf_thumbnail", {
          session,
          page: i,
          width: Math.max(1, Math.round(w * s)),
          height: Math.max(1, Math.round(h * s)),
          white: bg,
        });
      } catch {
        continue;
      }
      if (gen !== generation) return;
      const url = URL.createObjectURL(new Blob([png], { type: "image/png" }));
      urls.push(url);
      thumbs[i] = url;
    }
  }
  $effect(() => {
    void loadThumbs(white);
  });
  onDestroy(() => {
    generation++;
    urls.forEach((u) => URL.revokeObjectURL(u));
  });

  function submit() {
    if (!valid) return;
    saveSetting("pdf-import", {
      target: layersOnly ? saved.target : target,
      resolution,
      white,
      reverse,
    });
    close({ pages: [...selected].sort((a, b) => a - b), target, resolution, white, reverse });
  }

  const mm = (pt: number) => ((pt * 25.4) / 72).toFixed(1).replace(/\.0$/, "");
</script>

<Dialog title="Load PDF Image" width={560} onsubmit={submit} oncancel={() => close()}>
  <div class="file">{name}</div>
  <div class="pages" role="listbox" aria-multiselectable="true" aria-label="Pages">
    {#each thumbs as url, i (i)}
      <button
        type="button"
        class="page"
        class:on={selected.has(i)}
        role="option"
        aria-selected={selected.has(i)}
        onclick={(e) => pick(i, e)}
        ondblclick={() => {
          setSelected(new Set([i]));
          submit();
        }}
      >
        <span class="thumb">
          {#if url}<img src={url} alt="" />{/if}
        </span>
        <span class="num">{i + 1}</span>
      </button>
    {/each}
  </div>
  <div class="dim">{countText}</div>
  <div class="row">
    {#if !layersOnly}
      <label for="pdf-target">Open pages as</label>
      <select id="pdf-target" bind:value={target}>
        <option value="layers">Layers</option>
        <option value="images">Images</option>
      </select>
    {/if}
    <label for="pdf-range">Select range</label>
    <input
      id="pdf-range"
      class="range"
      title="Pages and ranges separated by commas, for example 1,3,5-7"
      bind:value={range}
      onblur={applyRange}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          applyRange();
        }
      }}
    />
    <button
      type="button"
      class="plain"
      onclick={() => setSelected(new Set(Array.from({ length: count }, (_, i) => i)))}
      >Select All</button
    >
  </div>
  <div class="grid">
    <label for="pdf-w">Width</label>
    <input
      id="pdf-w"
      use:numberField={{
        value: width,
        min: 1,
        max: MAX,
        oninput: (v) => (resolution = (Math.max(1, v) * 72) / pw),
      }}
    />
    <span class="dim">pixels</span>
    <label for="pdf-h">Height</label>
    <input
      id="pdf-h"
      use:numberField={{
        value: height,
        min: 1,
        max: MAX,
        oninput: (v) => (resolution = (Math.max(1, v) * 72) / ph),
      }}
    />
    <span class="dim">pixels</span>
    <label for="pdf-res">Resolution</label>
    <input
      id="pdf-res"
      use:numberField={{
        value: resolution,
        min: 5,
        max: 65536,
        decimals: 2,
        oninput: (v) => (resolution = Math.max(5, v)),
      }}
    />
    <span class="dim">pixels/in</span>
  </div>
  <div class="dim">Source PDF page size: {mm(pw)} × {mm(ph)} mm</div>
  <label class="check"><input type="checkbox" bind:checked={reverse} /> Load in reverse order</label
  >
  <label class="check">
    <input type="checkbox" bind:checked={white} /> Fill transparent areas with white
  </label>
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary" disabled={!valid}>Open</button>
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
  .pages {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(124px, 1fr));
    gap: 6px;
    height: 300px;
    overflow-y: auto;
    padding: 6px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
    align-content: start;
  }
  .page {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
  }
  .page:hover {
    background: var(--hover);
  }
  .page.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .thumb {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 120px;
    height: 120px;
  }
  .thumb img {
    max-width: 120px;
    max-height: 120px;
    box-shadow: var(--shadow);
    background-image: repeating-conic-gradient(var(--check-a) 0 25%, var(--check-b) 0 50%);
    background-size: 10px 10px;
  }
  .num {
    color: var(--fg-dim);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row label,
  .grid label {
    color: var(--fg-dim);
  }
  .row select,
  .row input,
  .row button {
    height: 24px;
  }
  .range {
    flex: 1;
    min-width: 60px;
    padding: 0 6px;
  }
  .plain {
    padding: 0 10px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  .plain:hover {
    background: var(--hover);
  }
  .grid {
    display: grid;
    grid-template-columns: auto 90px auto;
    align-items: center;
    gap: 6px 8px;
    justify-content: start;
  }
  .grid input {
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
