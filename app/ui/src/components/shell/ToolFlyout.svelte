<script lang="ts">
  // The tools of one toolbox group, opened to the right of its button by holding it. A tool
  // is picked by releasing the held button over it, or by clicking it. Escape or a click
  // outside closes it; that click never reaches the canvas (clicks in the toolbox still
  // work).
  import Icon from "../Icon.svelte";
  import { shortcutOf } from "../../lib/actions";
  import { toolDef, tools } from "../../lib/tools.svelte";

  let { group, anchor, onclose }: { group: string[]; anchor: DOMRect; onclose: () => void } =
    $props();

  let el: HTMLElement;

  function pick(id: string) {
    tools.select(id);
    onclose();
  }

  $effect(() => {
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element;
      if (el.contains(target)) return;
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
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onclose);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onclose);
    };
  });
</script>

<div
  class="flyout"
  bind:this={el}
  style:left="{anchor.right + 6}px"
  style:top="{anchor.top - 5}px"
  role="menu"
  aria-label="Tools"
>
  {#each group as id (id)}
    {@const t = toolDef(id)}
    <button
      class="item"
      class:on={tools.active === id}
      role="menuitemradio"
      aria-checked={tools.active === id}
      onpointerup={() => pick(id)}
      onclick={() => pick(id)}
    >
      <Icon name={t.icon} size={18} />
      <span class="name">{t.name}</span>
      <span class="key">{shortcutOf(`tool-${id}`)}</span>
    </button>
  {/each}
</div>

<style>
  .flyout {
    position: fixed;
    z-index: 210;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 200px;
    padding: 4px;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 10px;
    box-shadow: var(--shadow);
    animation: pop 0.1s ease-out;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateX(-4px);
    }
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 30px;
    padding: 0 10px 0 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    text-align: left;
    white-space: nowrap;
  }
  .item:hover {
    background: var(--accent-soft);
  }
  .item.on {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .name {
    flex: 1;
  }
  .key {
    color: var(--fg-dim);
    padding-left: 16px;
  }
</style>
