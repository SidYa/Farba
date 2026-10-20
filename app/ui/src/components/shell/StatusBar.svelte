<script lang="ts">
  import { documents } from "../../lib/documents.svelte";
  import { message } from "../../lib/notify.svelte";
  import { pointer } from "../../lib/view.svelte";
  import { selectionPreview } from "../../lib/canvas/selecttools.svelte";
  import { transformStatus } from "../../lib/canvas/transformtools.svelte";
  import { guideStatus } from "../../lib/guides.svelte";

  let doc = $derived(documents.active);
  let zoom = $derived(doc ? Math.round(doc.view.zoom * 1000) / 10 : undefined);
  /** The shape being drawn, else the selection's bounding box. */
  let selSize = $derived(selectionPreview.size ?? doc?.selection?.slice(2));
</script>

<footer class="status">
  <span class="msg" class:error={message.kind === "error"}
    >{documents.busy || guideStatus() || transformStatus.text || message.text}</span
  >
  {#if doc}
    <span class="cell">
      {#if pointer.x !== undefined}{pointer.x}, {pointer.y}{/if}
    </span>
    <span class="cell" title="Selection size">
      {#if selSize}Selection {selSize[0]} × {selSize[1]}{/if}
    </span>
    <span class="cell">{doc.width} × {doc.height} px</span>
    <span class="cell">{zoom}%</span>
  {/if}
</footer>

<style>
  .status {
    display: flex;
    align-items: center;
    height: var(--status-h);
    padding: 0 8px;
    gap: 16px;
    background: var(--bg-header);
    border-top: 1px solid var(--border);
    color: var(--fg-dim);
    font-size: 11px;
  }
  .msg {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msg.error {
    color: var(--danger);
  }
  .cell {
    min-width: 80px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
