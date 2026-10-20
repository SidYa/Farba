<script lang="ts">
  // The picker surface (Pinta's ColorPickerDialog): a hue/saturation wheel (optionally
  // shaded by the current value) or a saturation/value square for the current hue. Click
  // or drag to pick; a small square marks the current colour.
  import { cssRgb, hsvToRgb, type Hsv } from "../../lib/color";

  const RADIUS = 100;
  const PADDING = 10;
  const SIZE = (RADIUS + PADDING) * 2;

  let {
    mode,
    hsv,
    showValue,
    onpick,
  }: {
    mode: "wheel" | "square";
    hsv: Hsv;
    showValue: boolean;
    onpick: (patch: Partial<Hsv>) => void;
  } = $props();

  let canvas: HTMLCanvasElement;

  // Only what the image depends on: the value for the wheel (when shown), the hue for the
  // square. Moving the cursor does not redraw the surface.
  let key = $derived(mode === "wheel" ? (showValue ? hsv.v : 1) : hsv.h);

  $effect(() => {
    const k = key;
    const dpr = devicePixelRatio;
    const d = Math.round(RADIUS * 2 * dpr);
    canvas.width = d;
    canvas.height = d;
    const ctx = canvas.getContext("2d")!;
    const img = ctx.createImageData(d, d);
    const px = img.data;
    const r = d / 2;
    for (let y = 0; y < d; y++) {
      for (let x = 0; x < d; x++) {
        const i = (y * d + x) * 4;
        let rgb: [number, number, number];
        let a = 255;
        if (mode === "wheel") {
          const vx = x + 0.5 - r;
          const vy = y + 0.5 - r;
          const mag = Math.hypot(vx, vy);
          if (mag > r) continue;
          const h = ((Math.atan2(vy, -vx) + Math.PI) / (2 * Math.PI)) * 360;
          rgb = hsvToRgb({ h, s: Math.min(mag / r, 1), v: k });
          a = Math.round(Math.min(r - mag, 1) * 255);
        } else {
          rgb = hsvToRgb({ h: k, s: 1 - y / (d - 1), v: x / (d - 1) });
        }
        px[i] = rgb[0];
        px[i + 1] = rgb[1];
        px[i + 2] = rgb[2];
        px[i + 3] = a;
      }
    }
    ctx.putImageData(img, 0, 0);
  });

  let cursor = $derived.by(() => {
    if (mode === "wheel") {
      const rad = (hsv.h * Math.PI) / 180;
      return {
        x: RADIUS + Math.cos(rad) * hsv.s * RADIUS,
        y: RADIUS - Math.sin(rad) * hsv.s * RADIUS,
      };
    }
    return { x: hsv.v * (RADIUS * 2), y: (1 - hsv.s) * (RADIUS * 2) };
  });

  function pick(e: PointerEvent) {
    const b = canvas.getBoundingClientRect();
    const x = e.clientX - b.left;
    const y = e.clientY - b.top;
    if (mode === "wheel") {
      const vx = x - RADIUS;
      const vy = y - RADIUS;
      const h = ((Math.atan2(vy, -vx) + Math.PI) / (2 * Math.PI)) * 360;
      onpick({ h: h % 360, s: Math.min(Math.hypot(vx, vy) / RADIUS, 1) });
    } else {
      const size = RADIUS * 2;
      const cx = Math.min(size, Math.max(0, x));
      const cy = Math.min(size, Math.max(0, y));
      onpick({ s: 1 - cy / size, v: cx / size });
    }
  }

  let current = $derived(hsvToRgb(hsv));
</script>

<div
  class="surface"
  style:width="{SIZE}px"
  style:height="{SIZE}px"
  role="slider"
  tabindex="-1"
  aria-label={mode === "wheel" ? "Hue and saturation" : "Saturation and value"}
  aria-valuenow={Math.round(hsv.h)}
  onpointerdown={(e) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pick(e);
  }}
  onpointermove={(e) => {
    if ((e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) pick(e);
  }}
>
  <canvas
    bind:this={canvas}
    style:left="{PADDING}px"
    style:top="{PADDING}px"
    style:width="{RADIUS * 2}px"
    style:height="{RADIUS * 2}px"
  ></canvas>
  <div
    class="cursor"
    style:left="{PADDING + cursor.x}px"
    style:top="{PADDING + cursor.y}px"
    style:background={cssRgb(...current)}
  ></div>
</div>

<style>
  .surface {
    position: relative;
    flex: none;
    touch-action: none;
  }
  canvas {
    position: absolute;
  }
  .cursor {
    position: absolute;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border: 2px solid var(--color-marker-outer);
    box-shadow: inset 0 0 0 1px var(--color-marker-inner);
    pointer-events: none;
  }
</style>
