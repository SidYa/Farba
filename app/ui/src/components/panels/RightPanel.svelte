<script lang="ts">
  // Right dock: Colors pad (fixed height) on top, Layers below taking the rest. The width is
  // resizable by dragging the left edge and remembered.
  import ColorsPad from "./ColorsPad.svelte";
  import LayersPad from "./LayersPad.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";

  // The Colors pad needs 238 px (swatches 79 + recents 110 + clear button 20, gaps and
  // padding): narrower would cut off the clear button.
  const MIN = 242;
  const MAX = 600;
  const clamp = (w: number) => Math.min(MAX, Math.max(MIN, w));

  let width = $state(clamp(loadSetting("panel-width", 265)));

  function startResize(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    const w0 = width;
    const move = (ev: PointerEvent) => (width = clamp(w0 - (ev.clientX - x0)));
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      saveSetting("panel-width", width);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
</script>

<aside class="panel" style:width="{width}px">
  <div
    class="resize"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize panel"
    onpointerdown={startResize}
  ></div>
  <ColorsPad />
  <LayersPad />
</aside>

<style>
  .panel {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: none;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
    min-height: 0;
  }
  .resize {
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: ew-resize;
    z-index: 2;
  }
</style>
