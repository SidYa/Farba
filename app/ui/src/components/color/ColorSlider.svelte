<script lang="ts">
  // One colour component (Pinta's ColorPickerSlider): label, a gradient bar showing what
  // the component does to the current colour, and a number field. Drag or click the bar;
  // the mouse wheel steps by 1.
  import { numberField } from "../../lib/numberfield";

  let {
    label,
    value,
    max,
    gradient,
    checker = false,
    onchange,
  }: {
    label: string;
    value: number;
    max: number;
    /** CSS background of the bar (a linear-gradient). */
    gradient: string;
    /** Draw a checkerboard under the gradient (alpha). */
    checker?: boolean;
    onchange: (v: number) => void;
  } = $props();

  let bar: HTMLElement;
  const clamp = (v: number) => Math.min(max, Math.max(0, Math.round(v)));

  function fromPointer(e: PointerEvent) {
    const r = bar.getBoundingClientRect();
    onchange(clamp(((e.clientX - r.left) / r.width) * max));
  }
</script>

<div class="row">
  <span class="label">{label}</span>
  <div
    class="bar"
    class:checker
    bind:this={bar}
    role="slider"
    tabindex="-1"
    aria-label={label}
    aria-valuemin={0}
    aria-valuemax={max}
    aria-valuenow={Math.round(value)}
    onpointerdown={(e) => {
      if (e.button !== 0) return;
      bar.setPointerCapture(e.pointerId);
      fromPointer(e);
    }}
    onpointermove={(e) => {
      if (bar.hasPointerCapture(e.pointerId)) fromPointer(e);
    }}
    onwheel={(e) => {
      e.preventDefault();
      onchange(clamp(value + (e.deltaY < 0 ? 1 : -1)));
    }}
  >
    <div class="fill" style:background={gradient}></div>
    <div class="marker" style:left="{(value / max) * 100}%"></div>
  </div>
  <input use:numberField={{ value, min: 0, max, onchange: (v) => onchange(clamp(v)) }} />
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .label {
    width: 44px;
    color: var(--fg-dim);
  }
  .bar {
    position: relative;
    width: 200px;
    height: 16px;
    border-radius: 3px;
    outline: 1px solid var(--border-soft);
  }
  .bar.checker {
    background: repeating-conic-gradient(var(--check-a) 0 25%, var(--check-b) 0 50%) 0 0 / 8px 8px;
  }
  .fill {
    position: absolute;
    inset: 0;
    border-radius: 3px;
  }
  .marker {
    position: absolute;
    top: -3px;
    bottom: -3px;
    width: 4px;
    margin-left: -2px;
    border: 1px solid var(--color-marker-outer);
    border-radius: 2px;
    box-shadow: inset 0 0 0 1px var(--color-marker-inner);
    pointer-events: none;
  }
  input {
    width: 50px;
    height: 22px;
    padding: 0 4px;
    text-align: right;
  }
</style>
