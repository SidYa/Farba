<script lang="ts">
  // Colors pad: primary/secondary swatches + swap/reset icons, the 3x7 recent-colours grid
  // and a clear button. Geometry and behaviour follow Pinta-plus exactly (CLAUDE.md).
  import Icon from "../Icon.svelte";
  import { openColorDialog } from "../../lib/colordialog.svelte";
  import { palette, RECENT_COLUMNS, toCss, toHex, type Rgba } from "../../lib/palette.svelte";
  import { theme } from "../../lib/theme.svelte";

  const W = 79;
  const H = 56;
  const PRIMARY = { x: 2, y: 2, w: 34, h: 34 };
  const SECONDARY = { x: 18, y: 18, w: 34, h: 34 };
  const SWAP = { x: 60, y: 3, w: 15, h: 15 };
  const RESET = { x: 60, y: 26, w: 15, h: 15 };
  type Rect = typeof PRIMARY;

  let canvas: HTMLCanvasElement;
  let hoverTitle = $state("");

  const inside = (r: Rect, x: number, y: number) =>
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;

  function hit(e: MouseEvent): "primary" | "secondary" | "swap" | "reset" | undefined {
    const b = canvas.getBoundingClientRect();
    const x = e.clientX - b.left;
    const y = e.clientY - b.top;
    if (inside(PRIMARY, x, y)) return "primary";
    if (inside(SECONDARY, x, y)) return "secondary";
    if (inside(SWAP, x, y)) return "swap";
    if (inside(RESET, x, y)) return "reset";
  }

  function css(name: string) {
    return getComputedStyle(canvas).getPropertyValue(name).trim();
  }

  function checker(ctx: CanvasRenderingContext2D, r: Rect) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(r.x, r.y, r.w, r.h);
    ctx.clip();
    for (let y = 0; y < r.h; y += 8)
      for (let x = 0; x < r.w; x += 8) {
        ctx.fillStyle = (x + y) % 16 === 0 ? css("--check-a") : css("--check-b");
        ctx.fillRect(r.x + x, r.y + y, 8, 8);
      }
    ctx.restore();
  }

  function swatch(ctx: CanvasRenderingContext2D, r: Rect, c: Rgba) {
    if (c.a < 1) checker(ctx, r);
    ctx.fillStyle = toCss(c);
    ctx.fillRect(r.x, r.y, r.w, r.h);
    ctx.lineWidth = 1;
    ctx.strokeStyle = "#ffffff";
    ctx.strokeRect(r.x + 1.5, r.y + 1.5, r.w - 3, r.h - 3);
    ctx.strokeStyle = "#000000";
    ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
  }

  function swapIcon(ctx: CanvasRenderingContext2D, r: Rect) {
    // Curved arrow from the top-left to the bottom-right with arrowheads at both ends.
    const a = 4;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(r.x + 2, r.y + 3);
    ctx.quadraticCurveTo(r.x + r.w - 3, r.y + 3, r.x + r.w - 3, r.y + r.h - 2);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(r.x + 2 + a, r.y + 3 - a);
    ctx.lineTo(r.x + 2, r.y + 3);
    ctx.lineTo(r.x + 2 + a, r.y + 3 + a);
    ctx.moveTo(r.x + r.w - 3 - a, r.y + r.h - 2 - a);
    ctx.lineTo(r.x + r.w - 3, r.y + r.h - 2);
    ctx.lineTo(r.x + r.w - 3 + a, r.y + r.h - 2 - a);
    ctx.stroke();
  }

  function resetIcon(ctx: CanvasRenderingContext2D, r: Rect) {
    const s = Math.round(r.w * 0.6);
    ctx.lineWidth = 1;
    ctx.fillRect(r.x + r.w - s, r.y + r.h - s, s, s);
    ctx.strokeRect(r.x + 0.5, r.y + 0.5, s - 1, s - 1);
  }

  function draw() {
    const dpr = devicePixelRatio;
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    swatch(ctx, SECONDARY, palette.secondary);
    swatch(ctx, PRIMARY, palette.primary);
    const fg = css("--fg");
    ctx.strokeStyle = fg;
    ctx.fillStyle = fg;
    swapIcon(ctx, SWAP);
    resetIcon(ctx, RESET);
  }

  $effect(() => {
    // Redraw when the colours or the theme change.
    void palette.primary;
    void palette.secondary;
    void theme.effective;
    // Next frame: the new data-theme is applied by App after this (child) effect runs.
    requestAnimationFrame(draw);
  });

  function onRelease(e: PointerEvent) {
    switch (hit(e)) {
      case "primary":
        openColorDialog(true);
        break;
      case "secondary":
        openColorDialog(false);
        break;
      case "swap":
        palette.swapQuiet();
        break;
      case "reset":
        palette.reset();
        break;
    }
  }

  function onMove(e: PointerEvent) {
    switch (hit(e)) {
      case "primary":
        hoverTitle = `Color: ${toHex(palette.primary)}\n\nClick to select primary color.`;
        break;
      case "secondary":
        hoverTitle = `Color: ${toHex(palette.secondary)}\n\nClick to select secondary color.`;
        break;
      case "swap":
        hoverTitle = "Click to switch between primary and secondary color. Shortcut key: X";
        break;
      case "reset":
        hoverTitle = "Click to reset primary and secondary color.";
        break;
      default:
        hoverTitle = "";
    }
  }
</script>

<div class="pad">
  <canvas
    bind:this={canvas}
    class="main"
    style:width="{W}px"
    style:height="{H}px"
    title={hoverTitle}
    onpointerup={onRelease}
    onpointermove={onMove}
  ></canvas>

  <div class="recent" style:grid-template-columns="repeat({RECENT_COLUMNS}, 14px)">
    {#each palette.recent as c, i (i)}
      <button
        class="swatch"
        class:alpha={c.a < 1}
        style:--c={toCss(c)}
        title={`Color: ${toHex(c)}\n\nLeft click to set primary color. Right click to set secondary color.`}
        onpointerup={(e) => {
          if (e.button === 0) palette.setColor(true, c, false);
          else if (e.button === 2) palette.setColor(false, c, false);
        }}
        oncontextmenu={(e) => e.preventDefault()}
        aria-label={toHex(c)}
      ></button>
    {/each}
  </div>

  <button class="clear" title="Clear recent colors" onclick={() => palette.clearRecent()}>
    <Icon name="clear_all" size={14} />
  </button>
</div>

<style>
  .pad {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 4px 4px;
  }
  .main {
    flex: none;
    margin-left: 4px;
    margin-top: 2px;
    align-self: flex-start;
  }
  .recent {
    flex: none;
    display: grid;
    grid-auto-rows: 14px;
    gap: 2px;
  }
  .swatch {
    width: 14px;
    height: 14px;
    padding: 0;
    border: 1px solid var(--swatch-border);
    background: linear-gradient(var(--c), var(--c));
  }
  .swatch.alpha {
    background:
      linear-gradient(var(--c), var(--c)),
      repeating-conic-gradient(var(--check-a) 0 25%, var(--check-b) 0 50%) 0 0 / 8px 8px;
  }
  .clear {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 2px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
  }
  .clear:hover {
    background: var(--hover);
  }
</style>
