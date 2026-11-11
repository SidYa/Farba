<script lang="ts">
  import { iconSvg, type IconSet } from "../lib/icons";
  import { prefs } from "../lib/prefs.svelte";

  // `set`: a fixed icon set (the Settings preview); otherwise the chosen one.
  let { name, size = 18, set }: { name: string; size?: number; set?: IconSet } = $props();
</script>

<span
  class="icon"
  class:plain={set === "plain"}
  class:accent={set === "accent"}
  style:--size="{size}px"
  aria-hidden="true">{@html iconSvg(name, set ?? prefs.iconSet)}</span
>

<style>
  .icon {
    display: inline-flex;
    width: var(--size);
    height: var(--size);
    flex: none;
  }
  .icon :global(svg) {
    width: 100%;
    height: 100%;
  }
  /* Material Symbols: filled shapes. */
  .icon :global(svg:not(.fi)) {
    fill: currentColor;
  }
  /* Farba's own icons (lib/icons.farba.ts): strokes; "a" / "af" are the accent details
     (--icon-accent is set with Settings > Appearance > Icons > "Accent"). */
  .icon.plain {
    --icon-accent: currentColor;
  }
  .icon.accent {
    --icon-accent: var(--accent);
  }
  .icon :global(.fi .a) {
    stroke: var(--icon-accent, currentColor);
  }
  .icon :global(.fi .f) {
    fill: currentColor;
    stroke: none;
  }
  .icon :global(.fi .af) {
    fill: var(--icon-accent, currentColor);
    stroke: none;
  }
</style>
