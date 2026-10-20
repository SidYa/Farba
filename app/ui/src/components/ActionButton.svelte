<script lang="ts">
  // Flat icon button bound to an action; tooltip shows the label and shortcut.
  import { getAction, isEnabled, runAction, shortcutOf } from "../lib/actions";
  import Icon from "./Icon.svelte";

  let { action, size = 18 }: { action: string; size?: number } = $props();

  let a = $derived(getAction(action));
  let shortcut = $derived(shortcutOf(action));
  // `enabled` reads reactive state (documents, selection...), so this re-evaluates.
  let enabled = $derived(isEnabled(action));
</script>

<button
  class="action"
  disabled={!enabled}
  title={shortcut ? `${a.label} (${shortcut})` : a.label}
  onclick={() => runAction(action)}
>
  <Icon name={a.icon ?? "more_vert"} {size} />
</button>

<style>
  .action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--fg);
  }
  .action:hover:not(:disabled) {
    background: var(--hover);
  }
  .action:active:not(:disabled) {
    background: var(--pressed);
  }
  .action:disabled {
    color: var(--fg-faint);
  }
</style>
