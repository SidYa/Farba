<script lang="ts">
  // Tool settings popover: opened by right-clicking a tool in the toolbox (the owner's
  // preference instead of GIMP's Tool Options dock / Pinta's options row). Rounded panel
  // next to the toolbox, closed by clicking outside or Escape.
  import Icon from "../Icon.svelte";
  import { shortcutOf } from "../../lib/actions";
  import { TOOLS, tools, type ToolOption } from "../../lib/tools.svelte";
  import FontPicker from "../FontPicker.svelte";
  import { numberField } from "../../lib/numberfield";

  let { toolId, anchor, onclose }: { toolId: string; anchor: DOMRect; onclose: () => void } =
    $props();

  const MODES: [string, string, string][] = [
    ["replace", "Replace the current selection", "select"],
    ["add", "Add to the current selection (Shift)", "add"],
    ["subtract", "Subtract from the current selection (Ctrl)", "remove"],
    ["intersect", "Intersect with the current selection (Shift+Ctrl)", "close_small"],
  ];

  let el: HTMLElement;
  let tool = $derived(TOOLS.find((t) => t.id === toolId)!);
  let top = $state(0);

  function value<T>(o: ToolOption): T {
    return tools.option<T & (number | string | boolean)>(toolId, o.key);
  }

  function clamp(o: ToolOption & { type: "slider" }, v: number) {
    return Math.min(o.max, Math.max(o.min, v));
  }

  function onWheelSlider(e: WheelEvent, o: ToolOption & { type: "slider" }) {
    e.preventDefault();
    const step = o.step ?? 1;
    tools.setOption(toolId, o.key, clamp(o, value<number>(o) + (e.deltaY < 0 ? step : -step)));
  }

  $effect(() => {
    // Align with the tool button, but keep the whole popover inside the window.
    const h = el.offsetHeight;
    top = Math.max(8, Math.min(anchor.top - 8, window.innerHeight - h - 8));
  });

  $effect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (el.contains(target)) return;
      // Clicks in the toolbox still work (switch / reopen); anywhere else the click only
      // closes the popover and must not reach the canvas.
      if (!target.closest?.(".toolbox")) {
        e.preventDefault();
        e.stopPropagation();
      }
      onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onclose();
      }
    };
    // Capture phase, registered after the opening right-click has finished.
    const t = setTimeout(() => window.addEventListener("pointerdown", onDown, true));
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onclose);
    return () => {
      clearTimeout(t);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onclose);
    };
  });
</script>

<div
  class="popover"
  bind:this={el}
  style:left="{anchor.right + 8}px"
  style:top="{top}px"
  role="dialog"
  aria-label="{tool.name} options"
>
  <header>
    <Icon name={tool.icon} size={20} />
    <span class="name">{tool.name}</span>
    <span class="key">{shortcutOf(`tool-${tool.id}`)}</span>
  </header>

  <div class="body">
    {#each tool.options as o (o.key)}
      {#if o.type === "slider"}
        <div class="slider" onwheel={(e) => onWheelSlider(e, o)}>
          <div class="row">
            <span class="label">{o.label}</span>
            <span class="value">
              <input
                use:numberField={{
                  value: value<number>(o),
                  min: o.min,
                  max: o.max,
                  step: o.step ?? 1,
                  onchange: (v) => tools.setOption(toolId, o.key, v),
                }}
              />
              {#if o.unit}<span class="unit">{o.unit}</span>{/if}
            </span>
          </div>
          <input
            type="range"
            min={o.min}
            max={o.max}
            step={o.step ?? 1}
            value={value<number>(o)}
            style:--p="{((value<number>(o) - o.min) / (o.max - o.min)) * 100}%"
            oninput={(e) => tools.setOption(toolId, o.key, e.currentTarget.valueAsNumber)}
          />
        </div>
      {:else if o.type === "select"}
        <label class="row">
          <span class="label">{o.label}</span>
          <select
            value={value<string>(o)}
            onchange={(e) => tools.setOption(toolId, o.key, e.currentTarget.value)}
          >
            {#each o.choices as [v, label] (v)}<option value={v}>{label}</option>{/each}
          </select>
        </label>
      {:else if o.type === "font"}
        <div class="row">
          <span class="label">{o.label}</span>
          <FontPicker
            value={value<string>(o)}
            width={160}
            title={o.label}
            onchange={(f) => tools.setOption(toolId, o.key, f)}
          />
        </div>
      {:else if o.type === "check"}
        <label class="check">
          <input
            type="checkbox"
            checked={value<boolean>(o)}
            onchange={(e) => tools.setOption(toolId, o.key, e.currentTarget.checked)}
          />
          <span>{o.label}</span>
        </label>
      {:else if o.type === "modes"}
        <div class="row">
          <span class="label">{o.label}</span>
          <div class="modes" role="radiogroup" aria-label={o.label}>
            {#each MODES as [mode, title, icon] (mode)}
              <button
                class:on={value<string>(o) === mode}
                {title}
                role="radio"
                aria-checked={value<string>(o) === mode}
                onclick={() => tools.setOption(toolId, o.key, mode)}
              >
                <Icon name={icon} size={16} />
              </button>
            {/each}
          </div>
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .popover {
    position: fixed;
    z-index: 200;
    width: 280px;
    max-height: calc(100vh - 16px);
    overflow-y: auto;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 12px;
    box-shadow: var(--shadow);
    animation: pop 0.12s ease-out;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateX(-6px) scale(0.98);
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border-soft);
  }
  .name {
    flex: 1;
    font-weight: 600;
  }
  .key {
    color: var(--fg-dim);
    font-size: 11px;
    padding: 1px 6px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }
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
  .slider input[type="range"] {
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
  .slider input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--fg);
    border: 2px solid var(--accent);
  }
  select {
    height: 24px;
    max-width: 160px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
  .modes {
    display: flex;
    gap: 2px;
  }
  .modes button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: transparent;
  }
  .modes button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
</style>
