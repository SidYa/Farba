<script lang="ts">
  // Labelled slider with a number field and unit (the tool popover style); the mouse wheel
  // over it changes the value by one step.
  import { numberField } from "../lib/numberfield";

  let {
    label,
    value = $bindable(),
    min,
    max,
    step = 1,
    unit = "",
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    unit?: string;
  } = $props();

  const clamp = (v: number) => Math.min(max, Math.max(min, v));
</script>

<div
  class="slider"
  onwheel={(e) => {
    e.preventDefault();
    value = clamp(value + (e.deltaY < 0 ? step : -step));
  }}
>
  <div class="row">
    <span class="label">{label}</span>
    <span class="value">
      <input use:numberField={{ value, min, max, step, onchange: (v) => (value = v) }} />
      {#if unit}<span class="unit">{unit}</span>{/if}
    </span>
  </div>
  <input
    type="range"
    {min}
    {max}
    {step}
    bind:value
    style:--p="{((value - min) / (max - min)) * 100}%"
  />
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .label {
    color: var(--fg-dim);
  }
  .value {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .value input {
    width: 58px;
    height: 22px;
    padding: 0 4px;
    text-align: right;
  }
  .unit {
    color: var(--fg-faint);
    min-width: 14px;
  }
  input[type="range"] {
    width: 100%;
    margin: 4px 0 0;
    height: 6px;
    appearance: none;
    border-radius: 3px;
    background: linear-gradient(
      to right,
      var(--accent) 0 var(--p),
      var(--border-soft) var(--p) 100%
    );
  }
  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--fg);
    border: 2px solid var(--accent);
  }
</style>
