<script lang="ts">
  import ActionButton from "../ActionButton.svelte";
  import Icon from "../Icon.svelte";
  import { documents } from "../../lib/documents.svelte";
  import { dragState } from "../../lib/dragdrop.svelte";
  import { closeDocument } from "../../lib/files";

  let strip = $state<HTMLDivElement>();

  // The mouse wheel scrolls the strip horizontally, smoothly, up to either end. Wheel steps
  // add to the target, and every frame moves part of the way towards it.
  let target = 0;
  let pos = 0;
  let frame = 0;

  function onWheel(e: WheelEvent) {
    const el = e.currentTarget as HTMLDivElement;
    const max = el.scrollWidth - el.clientWidth;
    if (max <= 0) return;
    e.preventDefault();
    const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? el.clientWidth : 1;
    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (!frame) target = pos = el.scrollLeft;
    target = Math.min(max, Math.max(0, target + delta * unit));
    if (!frame) frame = requestAnimationFrame(step);
  }

  function step() {
    if (!strip) return void (frame = 0);
    pos += (target - pos) * 0.25;
    if (Math.abs(target - pos) < 0.5) pos = target;
    strip.scrollLeft = pos;
    frame = pos === target ? 0 : requestAnimationFrame(step);
  }

  // Keep the active tab visible (a new image opens at the end of the strip).
  $effect(() => {
    const doc = documents.active;
    const tab = doc && strip?.querySelector<HTMLElement>(`[data-doc="${doc.id}"]`);
    if (!tab || !strip) return;
    cancelAnimationFrame(frame);
    frame = 0;
    const end = strip.querySelector<HTMLElement>(".end")?.offsetWidth ?? 0;
    const left = tab.offsetLeft;
    const right = left + tab.offsetWidth + end;
    if (left < strip.scrollLeft) strip.scrollLeft = left;
    else if (right > strip.scrollLeft + strip.clientWidth)
      strip.scrollLeft = right - strip.clientWidth;
  });
</script>

{#if documents.list.length}
  <div
    class="tabs"
    class:drop-target={dragState.zone === "tabs"}
    role="tablist"
    data-drop-zone="tabs"
    data-drop-label="Open as new tabs"
    bind:this={strip}
    onwheel={onWheel}
  >
    {#each documents.list as doc (doc.id)}
      <div
        class="tab"
        data-doc={doc.id}
        class:active={documents.active === doc}
        role="tab"
        tabindex="0"
        aria-selected={documents.active === doc}
        title={doc.path ?? doc.name}
        onpointerdown={(e) => {
          if (e.button === 0) documents.active = doc;
        }}
        onauxclick={(e) => {
          if (e.button === 1) void closeDocument(doc);
        }}
        onkeydown={(e) => {
          if (e.key === "Enter") documents.active = doc;
        }}
      >
        <span class="name">{doc.name}</span>
        {#if doc.dirty}<span class="dirty" title="Unsaved changes">•</span>{/if}
        <span class="size">{doc.width}×{doc.height}</span>
        <button
          class="close"
          title="Close"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={() => void closeDocument(doc)}
        >
          <Icon name="close" size={14} />
        </button>
      </div>
    {/each}
    <span class="spacer"></span>
    <span class="end"><ActionButton action="file-close-all" size={16} /></span>
  </div>
{/if}

<style>
  .tabs {
    position: relative;
    display: flex;
    height: 30px;
    background: var(--bg-app);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    scrollbar-width: none;
    flex: none;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 260px;
    padding: 0 4px 0 10px;
    border-right: 1px solid var(--border);
    color: var(--fg-dim);
  }
  .tab.active {
    background: var(--bg-panel);
    color: var(--fg);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dirty {
    color: var(--accent);
    margin-left: -4px;
  }
  .size {
    color: var(--fg-faint);
    font-size: 11px;
  }
  .close {
    display: inline-flex;
    padding: 2px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: inherit;
  }
  .spacer {
    flex: 1;
  }
  .end {
    display: flex;
    align-items: center;
    padding: 0 4px;
    position: sticky;
    right: 0;
    background: var(--bg-app);
  }
  .close:hover {
    background: var(--hover);
  }
</style>
