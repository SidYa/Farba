<script lang="ts">
  import { iconSvg } from "../lib/icons";

  let { name, size = 18 }: { name: string; size?: number } = $props();
</script>

<span class="icon" style:--size="{size}px" aria-hidden="true">{@html iconSvg(name)}</span>

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
    fill: currentColor;
  }
</style>
