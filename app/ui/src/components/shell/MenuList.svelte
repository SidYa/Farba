<script lang="ts">
  // A popup list of menu items with nested submenus (opened on hover).
  import { getAction, hasAction, isEnabled, runAction, shortcutOf } from "../../lib/actions";
  import Icon from "../Icon.svelte";
  import MenuList from "./MenuList.svelte";
  import type { MenuItem } from "./menu.svelte";

  let { items, onclose }: { items: MenuItem[]; onclose: () => void } = $props();

  let sub = $state<number | undefined>();
  // An unknown action (e.g. of an add-on not loaded yet) is left out rather than breaking
  // the whole menu.
  let shown = $derived(items.filter((it) => !("action" in it) || hasAction(it.action)));
</script>

<div class="menu" role="menu">
  {#each shown as item, i (i)}
    {#if "separator" in item}
      <div class="sep"></div>
    {:else if "items" in item}
      <div
        class="item"
        class:open={sub === i}
        role="menuitem"
        tabindex="-1"
        onpointerenter={() => (sub = i)}
      >
        <span class="icon"></span>
        <span class="label">{item.label}</span>
        <Icon name="chevron_right" size={16} />
        {#if sub === i}
          <div class="submenu"><MenuList items={item.items} {onclose} /></div>
        {/if}
      </div>
    {:else}
      {@const a = getAction(item.action)}
      {@const enabled = isEnabled(item.action)}
      <button
        class="item"
        role="menuitem"
        disabled={!enabled}
        onpointerenter={() => (sub = undefined)}
        onclick={() => {
          onclose();
          runAction(item.action);
        }}
      >
        <span class="icon">
          {#if item.checked}
            {#if item.checked()}<Icon name="check" size={16} />{/if}
          {:else if a.icon}
            <Icon name={a.icon} size={16} />
          {/if}
        </span>
        <span class="label">{a.label}</span>
        <span class="key">{shortcutOf(item.action)}</span>
      </button>
    {/if}
  {/each}
</div>

<style>
  .menu {
    min-width: 220px;
    padding: 4px;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 6px;
    box-shadow: var(--shadow);
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px 0 6px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    text-align: left;
    white-space: nowrap;
  }
  .item:hover:not(:disabled),
  .item.open {
    background: var(--accent-soft);
  }
  .item:disabled {
    color: var(--fg-faint);
  }
  .icon {
    display: inline-flex;
    width: 16px;
    flex: none;
  }
  .label {
    flex: 1;
  }
  .key {
    color: var(--fg-dim);
    padding-left: 24px;
  }
  .item:disabled .key {
    color: var(--fg-faint);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border-soft);
  }
  .submenu {
    position: absolute;
    top: -5px;
    right: 100%;
    z-index: 1;
  }
</style>
