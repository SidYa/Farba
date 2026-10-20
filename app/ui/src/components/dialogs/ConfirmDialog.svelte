<script lang="ts">
  // A question with several buttons and an optional check box ("Don't ask again").
  import Dialog from "./Dialog.svelte";
  import type { ChoiceButton, DialogProps } from "../../lib/dialogs.svelte";

  let {
    title,
    message,
    buttons: choices,
    checkbox,
    close,
  }: {
    title: string;
    message: string;
    buttons: ChoiceButton[];
    checkbox?: string;
  } & DialogProps<{ value: string; checked: boolean }> = $props();

  let checked = $state(false);
  let primary = $derived(choices.find((b) => b.primary));
</script>

<Dialog
  {title}
  width={400}
  onsubmit={() => primary && close({ value: primary.value, checked })}
  oncancel={() => close()}
>
  <p class="message">{message}</p>
  {#if checkbox}
    <label class="check">
      <input type="checkbox" bind:checked />
      <span>{checkbox}</span>
    </label>
  {/if}
  {#snippet buttons()}
    {#each choices as b (b.value)}
      <button
        type={b.primary ? "submit" : "button"}
        class:primary={b.primary && !b.danger}
        class:danger={b.danger}
        onclick={b.primary ? undefined : () => close({ value: b.value, checked })}
      >
        {b.label}
      </button>
    {/each}
  {/snippet}
</Dialog>

<style>
  .message {
    margin: 0;
    white-space: pre-line;
    color: var(--fg);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg-dim);
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
</style>
