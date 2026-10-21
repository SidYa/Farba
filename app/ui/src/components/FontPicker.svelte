<script lang="ts">
  // Font family picker: a button showing the current font in its own face, and a list where
  // every family is drawn in its own face (a native <select> cannot style its options), with
  // a search field. Keyboard: type to filter, Up / Down, Enter picks, Escape closes.
  import { tick } from "svelte";
  import { fontFamilies } from "../lib/text/render";

  let {
    value,
    onchange,
    width = 150,
    title = "Font",
  }: { value: string; onchange: (font: string) => void; width?: number; title?: string } = $props();

  let fonts = $state<string[]>([]);
  void fontFamilies().then((f) => (fonts = f));

  let open = $state(false);
  let query = $state("");
  let hover = $state(0);
  let button: HTMLButtonElement;
  let list = $state<HTMLElement>();
  let search = $state<HTMLInputElement>();
  let pos = $state({ left: 0, top: 0, maxHeight: 300 });

  let shown = $derived(
    query ? fonts.filter((f) => f.toLowerCase().includes(query.toLowerCase())) : fonts,
  );

  /** CSS font-family value for a family name. */
  const face = (f: string) => `"${f.replace(/"/g, "")}", var(--font-ui, sans-serif)`;

  async function toggle() {
    if (open) {
      open = false;
      return;
    }
    const r = button.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 12;
    const above = r.top - 12;
    const maxHeight = Math.max(160, Math.min(420, Math.max(below, above)));
    pos = {
      left: Math.min(r.left, window.innerWidth - 280),
      top: below >= Math.min(maxHeight, 300) ? r.bottom + 4 : r.top - 4 - maxHeight,
      maxHeight,
    };
    query = "";
    hover = Math.max(0, fonts.indexOf(value));
    open = true;
    await tick();
    search?.focus();
    list?.querySelector(".on")?.scrollIntoView({ block: "center" });
  }

  function pick(f: string) {
    open = false;
    if (f !== value) onchange(f);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      hover = Math.max(0, Math.min(shown.length - 1, hover + (e.key === "ArrowDown" ? 1 : -1)));
      list?.children[hover]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      const f = shown[hover];
      if (f) pick(f);
    }
  }

  // A press outside closes the list (and only closes it).
  $effect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      const t = e.target as Node;
      if (list?.parentElement?.contains(t) || button.contains(t)) return;
      e.preventDefault();
      e.stopPropagation();
      open = false;
    };
    window.addEventListener("pointerdown", down, true);
    return () => window.removeEventListener("pointerdown", down, true);
  });
</script>

<button
  type="button"
  class="current"
  bind:this={button}
  style:width="{width}px"
  style:font-family={face(value)}
  {title}
  aria-haspopup="listbox"
  aria-expanded={open}
  onclick={toggle}
>
  <span class="name">{value}</span>
  <span class="arrow">▾</span>
</button>

{#if open}
  <div
    class="popup"
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    style:max-height="{pos.maxHeight}px"
    role="presentation"
    onkeydown={onKey}
  >
    <input
      class="search"
      type="text"
      placeholder="Search fonts"
      bind:this={search}
      bind:value={query}
      oninput={() => (hover = 0)}
    />
    <div class="list" role="listbox" aria-label={title} bind:this={list}>
      {#each shown as f, i (f)}
        <div
          class="item"
          class:on={f === value}
          class:hover={i === hover}
          role="option"
          aria-selected={f === value}
          tabindex="-1"
          style:font-family={face(f)}
          onpointerenter={() => (hover = i)}
          onclick={() => pick(f)}
          onkeydown={() => {}}
        >
          {f}
        </div>
      {:else}
        <div class="empty">No fonts match</div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .current {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
    color: var(--fg);
    font-size: 13px;
    text-align: left;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .arrow {
    flex: none;
    color: var(--fg-dim);
    font-family: var(--font-ui, sans-serif);
    font-size: 10px;
  }
  .popup {
    position: fixed;
    z-index: 600;
    display: flex;
    flex-direction: column;
    width: 240px;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 10px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .search {
    flex: none;
    height: 24px;
    margin: 4px;
    padding: 0 8px;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 4px 6px;
    /* An opaque scroller: tiles not yet drawn while scrolling show this colour. */
    background: var(--bg-popup);
    contain: paint;
  }
  .item {
    padding: 1px 8px;
    border-radius: var(--radius);
    font-size: 14px;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .item.hover {
    background: var(--hover);
  }
  .item.on {
    background: var(--accent-soft);
  }
  .empty {
    padding: 8px;
    color: var(--fg-faint);
  }
</style>
