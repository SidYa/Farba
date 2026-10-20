<script lang="ts">
  // "Choose Colors": a port of Pinta's ColorPickerDialog (MIT) in the app's style.
  // - Left: primary and secondary colour (click to choose which one is edited) and a swap
  //   button (X while the dialog is active).
  // - Middle: hue/saturation wheel (optionally shaded by value) or saturation/value square.
  // - Right: hex, H/S/V, R/G/B and alpha sliders.
  // - Bottom: recent colours (2 x 11) and the default palette (2 x 17).
  // Like Pinta's live palette mode it does not block the app: clicking outside makes it
  // translucent and applies changed colours to the palette (adding them to recents), so you
  // can paint and come back. OK applies and closes; Cancel just closes; Reset restores the
  // colours it opened with. It can be moved by its title.
  import { untrack } from "svelte";
  import Icon from "../Icon.svelte";
  import ColorSlider from "./ColorSlider.svelte";
  import ColorSurface from "./ColorSurface.svelte";
  import { cssRgb, hsvToRgb, parseHex, rgbToHsv, withHsv, type Hsv } from "../../lib/color";
  import { closeColorDialog } from "../../lib/colordialog.svelte";
  import {
    DEFAULT_PALETTE,
    palette,
    sameColor,
    toCss,
    toHex,
    type Rgba,
  } from "../../lib/palette.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";

  type Which = "primary" | "secondary";

  let { primary: startPrimary }: { primary: boolean } = $props();

  const RECENT_COLS = 11;
  const PALETTE_ROWS = 2;

  const original = untrack(() => ({ primary: palette.primary, secondary: palette.secondary }));
  let colors = $state.raw<Record<Which, Rgba>>({ ...original });
  let target = $state<Which>(untrack(() => (startPrimary ? "primary" : "secondary")));
  let hsv = $state<Hsv>(untrack(() => rgbToHsv(colors[target])));
  let current = $derived(colors[target]);

  let mode = $state<"wheel" | "square">(loadSetting("color-surface", "wheel"));
  let showValue = $state(loadSetting("color-show-value", true));
  let active = $state(true);
  let hexFocused = false;
  let hexText = $state(untrack(() => toHex(current).slice(1)));

  let panel: HTMLElement;
  let pos = $state<{ x: number; y: number } | undefined>();

  $effect(() => {
    if (!hexFocused) hexText = toHex(current).slice(1);
  });

  function setColor(c: Rgba) {
    colors = { ...colors, [target]: c };
    hsv = rgbToHsv(c, hsv);
  }

  function setHsv(patch: Partial<Hsv>) {
    hsv = { ...hsv, ...patch };
    colors = { ...colors, [target]: withHsv(current, hsv) };
  }

  function select(which: Which) {
    target = which;
    hsv = rgbToHsv(colors[which]);
  }

  function swap() {
    colors = { primary: colors.secondary, secondary: colors.primary };
    hsv = rgbToHsv(colors[target]);
  }

  /** Puts changed colours into the palette (which adds them to recents). */
  function apply() {
    for (const which of ["primary", "secondary"] as const)
      if (!sameColor(colors[which], palette[which]))
        palette.setColor(which === "primary", colors[which]);
  }

  function ok() {
    apply();
    closeColorDialog();
  }

  function reset() {
    colors = { ...original };
    hsv = rgbToHsv(colors[target]);
  }

  // Palette changes made elsewhere while the dialog is open (the Color Picker tool, the X
  // key while it is inactive) show up here.
  $effect(() => {
    const p = palette.primary;
    const s = palette.secondary;
    untrack(() => {
      if (sameColor(p, colors.primary) && sameColor(s, colors.secondary)) return;
      colors = { primary: p, secondary: s };
      hsv = rgbToHsv(colors[target], hsv);
    });
  });

  $effect(() => {
    // Centre on first show.
    const r = panel.getBoundingClientRect();
    pos ??= {
      x: Math.round((window.innerWidth - r.width) / 2),
      y: Math.round((window.innerHeight - r.height) / 2),
    };
  });

  $effect(() => {
    const onDown = (e: PointerEvent) => {
      if (panel.contains(e.target as Node)) {
        active = true;
      } else if (active) {
        // Losing focus applies the colours, like Pinta's live palette.
        active = false;
        apply();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (!active) return;
      const inText = e.target instanceof HTMLInputElement && e.target.type === "text";
      if (e.key === "Escape") {
        e.preventDefault();
        closeColorDialog();
      } else if (e.key === "Enter") {
        e.preventDefault();
        ok();
      } else if (e.code === "KeyX" && !inText && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        swap();
      }
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  });

  function startMove(e: PointerEvent) {
    if (e.button !== 0 || (e.target as Element).closest("button") || !pos) return;
    const start = { x: e.clientX - pos.x, y: e.clientY - pos.y };
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const r = panel.getBoundingClientRect();
      pos = {
        x: Math.min(window.innerWidth - 40, Math.max(40 - r.width, ev.clientX - start.x)),
        y: Math.min(window.innerHeight - 30, Math.max(0, ev.clientY - start.y)),
      };
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }

  function setMode(m: "wheel" | "square") {
    mode = m;
    saveSetting("color-surface", m);
  }

  // Gradients for the sliders, showing what each component does to the current colour.
  const rgb = (h: Hsv) => cssRgb(...hsvToRgb(h));
  let gHue = $derived(
    `linear-gradient(to right, ${[0, 60, 120, 180, 240, 300, 360].map((h) => rgb({ ...hsv, h: h % 360 })).join(", ")})`,
  );
  let gSat = $derived(
    `linear-gradient(to right, ${rgb({ ...hsv, s: 0 })}, ${rgb({ ...hsv, s: 1 })})`,
  );
  let gVal = $derived(
    `linear-gradient(to right, ${rgb({ ...hsv, v: 0 })}, ${rgb({ ...hsv, v: 1 })})`,
  );
  let gR = $derived(
    `linear-gradient(to right, ${cssRgb(0, current.g, current.b)}, ${cssRgb(255, current.g, current.b)})`,
  );
  let gG = $derived(
    `linear-gradient(to right, ${cssRgb(current.r, 0, current.b)}, ${cssRgb(current.r, 255, current.b)})`,
  );
  let gB = $derived(
    `linear-gradient(to right, ${cssRgb(current.r, current.g, 0)}, ${cssRgb(current.r, current.g, 255)})`,
  );
  let gA = $derived(
    `linear-gradient(to right, ${cssRgb(current.r, current.g, current.b, 0)}, ${cssRgb(current.r, current.g, current.b, 1)})`,
  );
</script>

<div
  class="panel"
  class:inactive={!active}
  bind:this={panel}
  style:left="{pos?.x ?? 0}px"
  style:top="{pos?.y ?? 0}px"
  style:visibility={pos ? "visible" : "hidden"}
  role="dialog"
  aria-label="Choose Colors"
>
  <header role="presentation" onpointerdown={startMove}>Choose Colors</header>

  <div class="top">
    <div class="displays">
      <button class="swap" title="Swap primary and secondary colour (X)" onclick={swap}>
        <Icon name="swap_vert" size={18} />
      </button>
      {#each ["primary", "secondary"] as const as which (which)}
        <button
          class="display"
          class:selected={target === which}
          class:alpha={colors[which].a < 1}
          style:--c={toCss(colors[which])}
          title={which === "primary" ? "Primary colour" : "Secondary colour"}
          aria-pressed={target === which}
          onclick={() => select(which)}
        ></button>
      {/each}
    </div>

    <div class="surface-box">
      <div class="modes" role="radiogroup" aria-label="Picker">
        <button
          class:on={mode === "wheel"}
          role="radio"
          aria-checked={mode === "wheel"}
          onclick={() => setMode("wheel")}
        >
          Hue &amp; Sat
        </button>
        <button
          class:on={mode === "square"}
          role="radio"
          aria-checked={mode === "square"}
          onclick={() => setMode("square")}
        >
          Sat &amp; Value
        </button>
      </div>
      <ColorSurface {mode} {hsv} {showValue} onpick={setHsv} />
      {#if mode === "wheel"}
        <label
          class="check"
          title="If enabled, the Value component is applied to the colour wheel."
        >
          <input
            type="checkbox"
            bind:checked={showValue}
            onchange={() => saveSetting("color-show-value", showValue)}
          />
          Show Value
        </label>
      {/if}
    </div>

    <div class="sliders">
      <div class="hex">
        <span class="label">Hex</span>
        <input
          type="text"
          spellcheck="false"
          maxlength="9"
          bind:value={hexText}
          onfocus={() => (hexFocused = true)}
          onblur={() => {
            hexFocused = false;
            hexText = toHex(current).slice(1);
          }}
          oninput={() => {
            const c = parseHex(hexText);
            if (c) setColor(c);
          }}
        />
      </div>
      <ColorSlider
        label="Hue"
        value={hsv.h}
        max={360}
        gradient={gHue}
        onchange={(h) => setHsv({ h: h % 360 })}
      />
      <ColorSlider
        label="Sat"
        value={hsv.s * 100}
        max={100}
        gradient={gSat}
        onchange={(s) => setHsv({ s: s / 100 })}
      />
      <ColorSlider
        label="Value"
        value={hsv.v * 100}
        max={100}
        gradient={gVal}
        onchange={(v) => setHsv({ v: v / 100 })}
      />
      <div class="sep"></div>
      <ColorSlider
        label="Red"
        value={current.r}
        max={255}
        gradient={gR}
        onchange={(r) => setColor({ ...current, r })}
      />
      <ColorSlider
        label="Green"
        value={current.g}
        max={255}
        gradient={gG}
        onchange={(g) => setColor({ ...current, g })}
      />
      <ColorSlider
        label="Blue"
        value={current.b}
        max={255}
        gradient={gB}
        onchange={(b) => setColor({ ...current, b })}
      />
      <div class="sep"></div>
      <ColorSlider
        label="Alpha"
        value={current.a * 255}
        max={255}
        gradient={gA}
        checker
        onchange={(a) => setColor({ ...current, a: a / 255 })}
      />
    </div>
  </div>

  <div class="swatches">
    <div class="grid" style:grid-template-columns="repeat({RECENT_COLS}, 19px)">
      {#each palette.recent as c, i (i)}
        <button
          class="swatch"
          class:alpha={c.a < 1}
          style:--c={toCss(c)}
          title={toHex(c)}
          aria-label={toHex(c)}
          onclick={() => setColor(c)}
        ></button>
      {/each}
    </div>
    <div
      class="grid"
      style:grid-template-rows="repeat({PALETTE_ROWS}, 19px)"
      style:grid-auto-flow="column"
    >
      {#each DEFAULT_PALETTE as c, i (i)}
        <button
          class="swatch"
          style:--c={toCss(c)}
          title={toHex(c)}
          aria-label={toHex(c)}
          onclick={() => setColor({ ...c })}
        ></button>
      {/each}
    </div>
  </div>

  <footer>
    <button onclick={reset} title="Restore the colours the dialog opened with">Reset</button>
    <span class="grow"></span>
    <button onclick={closeColorDialog}>Cancel</button>
    <button class="primary" onclick={ok}>OK</button>
  </footer>
</div>

<style>
  .panel {
    position: fixed;
    z-index: 400;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 0 12px 12px;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 12px;
    box-shadow: var(--shadow);
    transition: opacity 0.15s;
  }
  .panel.inactive {
    opacity: 0.85;
  }
  header {
    margin: 0 -12px;
    padding: 10px 16px 6px;
    font-size: 14px;
    font-weight: 600;
    cursor: move;
  }
  .top {
    display: flex;
    gap: 6px;
  }
  .displays {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .swap {
    display: inline-flex;
    padding: 4px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
  }
  .swap:hover {
    background: var(--hover);
  }
  .display {
    width: 50px;
    height: 50px;
    padding: 0;
    border: 3px solid var(--color-marker-outer);
    border-radius: 2px;
    outline-offset: 2px;
    background: linear-gradient(var(--c), var(--c));
  }
  .display.alpha,
  .swatch.alpha {
    background:
      linear-gradient(var(--c), var(--c)),
      repeating-conic-gradient(var(--check-a) 0 25%, var(--check-b) 0 50%) 0 0 / 8px 8px;
  }
  .display.selected {
    outline: 2px solid var(--accent);
  }
  .surface-box {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .modes {
    display: flex;
    gap: 2px;
  }
  .modes button {
    height: 24px;
    padding: 0 12px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: transparent;
  }
  .modes button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-dim);
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
  .sliders {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
  }
  .hex {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .hex .label {
    width: 44px;
    color: var(--fg-dim);
  }
  .hex input {
    width: 100px;
    height: 22px;
    padding: 0 6px;
    font-family: var(--font-mono);
  }
  .sep {
    height: 1px;
    background: var(--border-soft);
  }
  .swatches {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .grid {
    display: grid;
    grid-auto-rows: 19px;
    grid-auto-columns: 19px;
  }
  .swatch {
    width: 19px;
    height: 19px;
    padding: 0;
    border: 1px solid var(--swatch-border);
    background: linear-gradient(var(--c), var(--c));
  }
  footer {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }
  footer button {
    min-width: 84px;
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  footer button:hover {
    background: var(--hover);
  }
  footer button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }
  .grow {
    flex: 1;
  }
</style>
