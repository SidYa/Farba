<script lang="ts">
  // Header-bar menu button: an icon with the menu name as tooltip (as in Pinta), or a text
  // label. Menus are right-aligned under the button because they sit at the right edge of
  // the header bar.
  import Icon from "../Icon.svelte";
  import MenuList from "./MenuList.svelte";
  import { menuState, type MenuItem } from "./menu.svelte";

  let {
    id,
    label,
    title,
    icon,
    items,
  }: { id: string; label?: string; title?: string; icon?: string; items: MenuItem[] } = $props();

  let root: HTMLElement;
  let open = $derived(menuState.open === id);

  function close() {
    if (menuState.open === id) menuState.open = undefined;
  }

  $effect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (root.contains(e.target as Node)) return;
      // The click only closes the menu; it must not reach the canvas (no stray paint).
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", close);
    };
  });
</script>

<div class="wrap" bind:this={root}>
  <button
    class="menu-button"
    class:open
    class:icon-only={!label}
    title={title ?? label ?? ""}
    aria-label={title ?? label}
    onclick={() => (menuState.open = open ? undefined : id)}
    onpointerenter={() => {
      // Menubar behaviour: when another menu is open, hovering switches to this one.
      if (menuState.open && menuState.open !== id) menuState.open = id;
    }}
  >
    {#if icon}<Icon name={icon} />{/if}
    {#if label}<span>{label}</span>{/if}
  </button>
  {#if open}
    <div class="popup"><MenuList {items} onclose={close} /></div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  .menu-button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
  }
  .menu-button.icon-only {
    width: 30px;
    padding: 0;
    justify-content: center;
  }
  .menu-button:hover,
  .menu-button.open {
    background: var(--hover);
  }
  .popup {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 100;
  }
</style>
