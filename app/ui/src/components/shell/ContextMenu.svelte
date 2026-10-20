<script lang="ts">
  // A right-click menu at the pointer, kept inside the window. A click outside only closes
  // it (it does not reach what is underneath), like the header menus.
  import MenuList from "./MenuList.svelte";
  import type { MenuItem } from "./menu.svelte";

  let { items, x, y, onclose }: { items: MenuItem[]; x: number; y: number; onclose: () => void } =
    $props();

  let el: HTMLElement;
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)),
    };
  });

  $effect(() => {
    const onDown = (e: PointerEvent) => {
      if (el.contains(e.target as Node)) return;
      e.preventDefault();
      e.stopPropagation();
      onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onclose();
      }
    };
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

<div class="context" bind:this={el} style:left="{pos.left}px" style:top="{pos.top}px">
  <MenuList {items} {onclose} />
</div>

<style>
  .context {
    position: fixed;
    z-index: 300;
  }
</style>
