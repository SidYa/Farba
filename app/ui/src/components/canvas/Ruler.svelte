<script lang="ts">
  // A ruler along the top (horizontal) or left (vertical) edge of the canvas (GIMP): image
  // pixel coordinates with ticks that follow zoom and pan, and a marker at the pointer.
  // Dragging from it onto the canvas adds a guide (lib/guides.svelte.ts).
  import { documents } from "../../lib/documents.svelte";
  import { finishGuideDrag, guideDrag, guidePos } from "../../lib/guides.svelte";
  import { cssColor, theme } from "../../lib/theme.svelte";
  import { pointer } from "../../lib/view.svelte";
  import { prefs } from "../../lib/prefs.svelte";

  let {
    horizontal,
    canvas,
    onchange,
  }: {
    /** Along the top edge (makes horizontal guides). */
    horizontal: boolean;
    /** The image canvas (positions are measured against it). */
    canvas: () => HTMLCanvasElement;
    onchange: () => void;
  } = $props();

  let el: HTMLCanvasElement;
  let size = $state({ w: 1, h: 1 });

  const rgb = (name: string) =>
    `rgb(${cssColor(name)
      .map((v) => Math.round(v * 255))
      .join(",")})`;

  /** Image pixels between labelled ticks: at least ~60 screen pixels apart. */
  function step(zoom: number): number {
    const min = (60 * devicePixelRatio) / zoom;
    for (let p = 1; ; p *= 10) for (const m of [1, 2, 5]) if (m * p >= min) return m * p;
  }

  function draw() {
    const doc = documents.active;
    const ctx = el?.getContext("2d");
    if (!ctx) return;
    const dpr = devicePixelRatio;
    const { w, h } = size;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = rgb("--bg-header");
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = rgb("--border");
    if (horizontal) ctx.fillRect(0, h - dpr, w, dpr);
    else ctx.fillRect(w - dpr, 0, dpr, h);
    if (!doc) return;
    const v = doc.view;
    const origin = horizontal ? v.x : v.y;
    const length = horizontal ? w : h;
    const thick = horizontal ? h : w;
    const major = step(v.zoom);
    const minor = major / (major % 5 === 0 || major === 1 ? 5 : 2);
    const first = Math.floor(-origin / v.zoom / minor) * minor;
    const last = (length - origin) / v.zoom;
    ctx.strokeStyle = rgb("--fg-faint");
    ctx.lineWidth = dpr;
    ctx.beginPath();
    for (let i = first; i <= last; i += minor) {
      const at = Math.round(origin + i * v.zoom) + 0.5;
      const isMajor = Math.abs(i / major - Math.round(i / major)) < 1e-6;
      const len = isMajor ? thick : thick * 0.3;
      if (horizontal) {
        ctx.moveTo(at, h - len);
        ctx.lineTo(at, h);
      } else {
        ctx.moveTo(w - len, at);
        ctx.lineTo(w, at);
      }
    }
    ctx.stroke();
    ctx.fillStyle = rgb("--fg-dim");
    ctx.font = `${9 * dpr}px system-ui, sans-serif`;
    ctx.textBaseline = "top";
    for (let i = Math.floor(first / major) * major; i <= last; i += major) {
      const at = Math.round(origin + i * v.zoom);
      const label = String(Math.round(i));
      if (horizontal) ctx.fillText(label, at + 3 * dpr, 2 * dpr);
      else {
        ctx.save();
        ctx.translate(2 * dpr, at + 3 * dpr);
        ctx.rotate(Math.PI / 2);
        ctx.fillText(label, 0, -9 * dpr);
        ctx.restore();
      }
    }
    // The pointer position.
    const p = horizontal ? pointer.x : pointer.y;
    if (p !== undefined) {
      const at = Math.round(origin + (p + 0.5) * v.zoom);
      ctx.fillStyle = rgb("--accent");
      if (horizontal) ctx.fillRect(at, 0, dpr, h);
      else ctx.fillRect(0, at, w, dpr);
    }
  }

  $effect(() => {
    void documents.active?.view;
    void pointer.x;
    void pointer.y;
    void theme.effective;
    void prefs.accent;
    void size;
    requestAnimationFrame(draw);
  });

  $effect(() => {
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      const dpr = devicePixelRatio;
      el.width = Math.max(1, Math.round(r.width * dpr));
      el.height = Math.max(1, Math.round(r.height * dpr));
      size = { w: el.width, h: el.height };
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  /** Updates the dragged guide from a pointer position (removed outside the canvas). */
  function track(e: PointerEvent) {
    const doc = guideDrag.doc;
    if (!doc) return;
    const r = canvas().getBoundingClientRect();
    const inside =
      e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.bottom;
    const dpr = devicePixelRatio;
    const sx = (e.clientX - r.left) * dpr;
    const sy = (e.clientY - r.top) * dpr;
    guideDrag.pos = inside ? guidePos(doc.view, horizontal, sx, sy) : undefined;
    onchange();
  }
</script>

<canvas
  bind:this={el}
  class="ruler"
  class:horizontal
  title="Drag onto the image to add a guide"
  onpointerdown={(e) => {
    const doc = documents.active;
    if (e.button !== 0 || !doc) return;
    el.setPointerCapture(e.pointerId);
    guideDrag.doc = doc;
    guideDrag.horizontal = horizontal;
    guideDrag.index = undefined;
    guideDrag.pos = undefined;
  }}
  onpointermove={track}
  onpointerup={(e) => {
    if (!guideDrag.doc) return;
    track(e);
    finishGuideDrag();
    onchange();
  }}
  onpointercancel={() => {
    guideDrag.pos = undefined;
    finishGuideDrag();
    onchange();
  }}
></canvas>

<style>
  .ruler {
    display: block;
    width: 100%;
    height: 100%;
    cursor: col-resize;
  }
  .ruler.horizontal {
    cursor: row-resize;
  }
</style>
