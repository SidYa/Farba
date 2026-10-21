<script lang="ts">
  // An on / off switch (the Addons Manager's style). Sizes are multiples of 4 px, so they
  // stay whole device pixels at 125 % / 150 % display scaling.
  let {
    checked,
    label,
    onchange,
  }: { checked: boolean; label: string; onchange: (on: boolean) => void } = $props();
</script>

<input
  class="switch"
  type="checkbox"
  role="switch"
  aria-label={label}
  {checked}
  onchange={(e) => onchange(e.currentTarget.checked)}
/>

<style>
  .switch {
    appearance: none;
    position: relative;
    flex: none;
    box-sizing: border-box;
    width: 36px;
    height: 20px;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-input);
    transition: background 0.15s;
    cursor: pointer;
  }
  .switch::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--fg-dim);
    transition: left 0.15s;
  }
  .switch:checked {
    border-color: var(--accent);
    background: var(--accent);
  }
  .switch:checked::after {
    left: 19px;
    background: var(--accent-fg);
  }
  .switch:focus-visible {
    outline: 2px solid var(--accent-soft);
    outline-offset: 1px;
  }
</style>
