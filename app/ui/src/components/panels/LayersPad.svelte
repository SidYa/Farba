<script lang="ts">
  // Layers pad (Pinta-plus layout + GIMP's locks, see CLAUDE.md "Layers pad"): rows with
  // thumbnail, name, visibility, lock alpha and lock position; Ctrl/Shift multi-select;
  // drag to reorder; double-click the name to rename; right-click menu; bottom toolbar.
  // Delete and Ctrl+A act on layers while the pointer is over the pad.
  import ActionButton from "../ActionButton.svelte";
  import Icon from "../Icon.svelte";
  import ContextMenu from "../shell/ContextMenu.svelte";
  import type { MenuItem } from "../shell/menu.svelte";
  import LayerThumb from "./LayerThumb.svelte";
  import { deleteLayers, layerProperties } from "../../lib/commands";
  import { dialogs } from "../../lib/dialogs.svelte";
  import { documents, type Doc, type LayerInfo } from "../../lib/documents.svelte";
  import { eventKey, isTextInput, runAction } from "../../lib/actions";

  const DRAG_THRESHOLD = 6;

  let doc = $derived(documents.active);
  let rows = $derived(doc ? [...doc.layers].reverse() : []);

  let hover = false;
  let menu = $state<{ x: number; y: number; items: MenuItem[] } | undefined>();
  let editing = $state<number | undefined>();
  let editName = $state("");

  const rowEls = new Map<number, HTMLElement>();
  let drag:
    { id: number; y: number; active: boolean; moved: number[]; pendingSelect?: number } | undefined;
  let dragging = $state<number[]>([]);
  let drop = $state<{ id: number; above: boolean } | undefined>();

  const contextItems: MenuItem[] = [
    { action: "layer-new" },
    { action: "layer-duplicate" },
    { action: "layer-delete" },
    { separator: true },
    { action: "layer-merge-down" },
    { action: "layer-merge-selected" },
    { action: "layer-merge-visible" },
    { separator: true },
    { action: "layer-raise" },
    { action: "layer-lower" },
    { separator: true },
    { action: "layer-flip-h" },
    { action: "layer-flip-v" },
    { action: "layer-rotate" },
    { action: "layer-to-image-size" },
    { separator: true },
    { action: "layer-properties" },
  ];

  const toolbarMenu: MenuItem[] = [
    { action: "layer-import" },
    { separator: true },
    { action: "layer-flip-h" },
    { action: "layer-flip-v" },
    { action: "layer-rotate" },
    { action: "layer-to-image-size" },
    { separator: true },
    { action: "layer-properties" },
  ];

  function rowRef(node: HTMLElement, id: number) {
    rowEls.set(id, node);
    return { destroy: () => rowEls.delete(id) };
  }

  function onRowDown(e: PointerEvent, d: Doc, layer: LayerInfo) {
    if (editing !== undefined) commitRename(d);
    if (e.button === 2) {
      if (!d.selected.has(layer.id)) d.select(layer.id);
      return;
    }
    if (e.button !== 0) return;
    const mode = e.shiftKey ? "range" : e.ctrlKey ? "toggle" : "replace";
    // A plain click on one of several selected layers keeps the selection until release,
    // so the whole selection can be dragged.
    let pendingSelect: number | undefined;
    if (mode === "replace" && d.selected.has(layer.id) && d.selected.size > 1)
      pendingSelect = layer.id;
    else d.select(layer.id, mode);
    const moved = d.selected.has(layer.id) ? d.selectedIds : [layer.id];
    drag = { id: layer.id, y: e.clientY, active: false, moved, pendingSelect };
  }

  function onListMove(e: PointerEvent) {
    if (!drag) return;
    if (!drag.active) {
      if (Math.abs(e.clientY - drag.y) < DRAG_THRESHOLD) return;
      drag.active = true;
      dragging = drag.moved;
      // Captured only once dragging, so clicks and double-clicks on rows still work.
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    }
    drop = undefined;
    for (const [id, el] of rowEls) {
      const r = el.getBoundingClientRect();
      if (e.clientY >= r.top && e.clientY < r.bottom) {
        drop = { id, above: e.clientY < r.top + r.height / 2 };
        break;
      }
    }
  }

  function onListUp() {
    const d = doc;
    if (!drag || !d) return;
    if (drag.active && drop && !drag.moved.includes(drop.id)) {
      const target = drop;
      const moved = drag.moved;
      const rest = d.layers.filter((l) => !moved.includes(l.id));
      const i = rest.findIndex((l) => l.id === target.id);
      // "Above" in the pad is higher in the stack.
      d.run((c) => c.move_layers(Uint32Array.from(moved), target.above ? i + 1 : i));
    } else if (!drag.active && drag.pendingSelect !== undefined) {
      d.select(drag.pendingSelect);
    }
    drag = undefined;
    dragging = [];
    drop = undefined;
  }

  function startRename(layer: LayerInfo) {
    editing = layer.id;
    editName = layer.name;
  }

  function commitRename(d: Doc) {
    const id = editing;
    editing = undefined;
    const name = editName.trim();
    if (id !== undefined && name && name !== d.layer(id)?.name)
      d.setProps(id, { name }, "Rename Layer");
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  // Pointer-based shortcuts (not focus-based): while the pointer is over the pad, Delete
  // deletes the selected layers and Ctrl+A selects all layers.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!hover || !doc || dialogs.isOpen || editing !== undefined || isTextInput(e.target))
        return;
      const key = eventKey(e);
      if (key === "Delete") {
        e.preventDefault();
        void deleteLayers(doc);
      } else if (key === "Ctrl+A") {
        e.preventDefault();
        runAction("layer-select-all");
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
</script>

<section
  class="pad"
  aria-label="Layers"
  onpointerenter={() => (hover = true)}
  onpointerleave={() => (hover = false)}
>
  <header class="title">Layers</header>
  <div
    class="list"
    role="listbox"
    aria-label="Layers"
    aria-multiselectable="true"
    tabindex="-1"
    onpointermove={onListMove}
    onpointerup={onListUp}
    onpointercancel={onListUp}
    oncontextmenu={(e) => {
      e.preventDefault();
      if (doc) menu = { x: e.clientX, y: e.clientY, items: contextItems };
    }}
  >
    {#if doc}
      {@const d = doc}
      {#each rows as layer (layer.id)}
        <div
          class="row"
          class:selected={d.selected.has(layer.id)}
          class:active={d.active === layer.id}
          class:hidden={!layer.visible}
          class:dragging={dragging.includes(layer.id)}
          class:drop-above={drop?.id === layer.id && drop.above}
          class:drop-below={drop?.id === layer.id && !drop.above}
          role="option"
          tabindex="-1"
          aria-selected={d.selected.has(layer.id)}
          use:rowRef={layer.id}
          onpointerdown={(e) => onRowDown(e, d, layer)}
        >
          <button
            class="toggle"
            title={layer.visible ? "Hide layer" : "Show layer"}
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() =>
              d.setProps(
                layer.id,
                { visible: !layer.visible },
                layer.visible ? "Hide Layer" : "Show Layer",
              )}
          >
            <Icon name={layer.visible ? "visibility" : "visibility_off"} size={16} />
          </button>
          <div
            class="thumb"
            role="presentation"
            ondblclick={() => void layerProperties(d, layer.id)}
          >
            <LayerThumb doc={d} {layer} />
            {#if layer.isText}
              <span class="text-badge" title="Text layer"
                ><Icon name="text_fields" size={12} /></span
              >
            {/if}
          </div>
          {#if editing === layer.id}
            <input
              class="rename"
              type="text"
              bind:value={editName}
              use:focusSelect
              onpointerdown={(e) => e.stopPropagation()}
              onblur={() => commitRename(d)}
              onkeydown={(e) => {
                if (e.key === "Enter") commitRename(d);
                else if (e.key === "Escape") {
                  e.stopPropagation();
                  editing = undefined;
                }
              }}
            />
          {:else}
            <span
              class="name"
              title={layer.name}
              role="presentation"
              ondblclick={() => startRename(layer)}>{layer.name}</span
            >
          {/if}
          <button
            class="toggle"
            class:on={layer.lockAlpha}
            title="Lock alpha channel"
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() =>
              d.setProps(layer.id, { lockAlpha: !layer.lockAlpha }, "Lock Alpha Channel")}
          >
            <Icon name="contrast" size={14} />
          </button>
          <button
            class="toggle"
            class:on={layer.lockPosition}
            title="Lock position"
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() =>
              d.setProps(layer.id, { lockPosition: !layer.lockPosition }, "Lock Position")}
          >
            <Icon name={layer.lockPosition ? "lock" : "lock_open"} size={14} />
          </button>
        </div>
      {/each}
    {:else}
      <div class="empty">No image</div>
    {/if}
  </div>
  <footer class="toolbar">
    <ActionButton action="layer-new" size={16} />
    <ActionButton action="layer-delete" size={16} />
    <ActionButton action="layer-duplicate" size={16} />
    <ActionButton action="layer-merge-down" size={16} />
    <ActionButton action="layer-raise" size={16} />
    <ActionButton action="layer-lower" size={16} />
    <span class="spacer"></span>
    <button
      class="more"
      title="More layer actions"
      disabled={!doc}
      onclick={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        menu = { x: r.right, y: r.top - 4, items: toolbarMenu };
      }}
    >
      <Icon name="more_vert" size={16} />
    </button>
  </footer>
</section>

{#if menu}
  <ContextMenu items={menu.items} x={menu.x} y={menu.y} onclose={() => (menu = undefined)} />
{/if}

<style>
  .pad {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .title {
    padding: 6px 8px;
    color: var(--fg-dim);
    border-top: 1px solid var(--border);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    background: var(--bg-input);
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
    outline: none;
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 46px;
    padding: 0 4px;
    border-bottom: 1px solid var(--border-soft);
  }
  .row.selected {
    background: var(--accent-soft);
  }
  .row.active {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .row.hidden .name {
    color: var(--fg-faint);
  }
  .row.dragging {
    opacity: 0.4;
  }
  .row.drop-above::before,
  .row.drop-below::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 3px;
    background: var(--accent);
    z-index: 1;
  }
  .row.drop-above::before {
    top: -2px;
  }
  .row.drop-below::after {
    bottom: -2px;
  }
  .thumb {
    position: relative;
    flex: none;
  }
  /* A text layer: a small "T" on the thumbnail (GIMP shows a text icon). */
  .text-badge {
    position: absolute;
    right: 1px;
    bottom: 1px;
    display: inline-flex;
    padding: 1px;
    border-radius: 3px;
    background: var(--bg-popup);
    color: var(--fg);
    opacity: 0.9;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rename {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 4px;
  }
  .toggle,
  .more {
    display: inline-flex;
    padding: 2px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--fg-dim);
  }
  .toggle:hover,
  .more:hover:not(:disabled) {
    background: var(--hover);
  }
  .more {
    padding: 6px;
    color: var(--fg);
  }
  .more:disabled {
    color: var(--fg-faint);
  }
  .toggle.on {
    color: var(--accent);
  }
  .empty {
    padding: 12px;
    color: var(--fg-faint);
    text-align: center;
  }
  .toolbar {
    display: flex;
    align-items: center;
    padding: 2px 4px;
  }
  .spacer {
    flex: 1;
  }
</style>
