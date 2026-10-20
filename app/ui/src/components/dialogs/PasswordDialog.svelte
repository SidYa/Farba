<script lang="ts">
  // Asks for the password of a protected file (GIMP: "Wrong password!" on a retry).
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";

  let {
    close,
    name,
    retry = false,
  }: { name: string; retry?: boolean } & DialogProps<string> = $props();

  let password = $state("");
</script>

<Dialog title="Password" width={340} onsubmit={() => close(password)} oncancel={() => close()}>
  <div class="msg">
    {retry ? "Wrong password! Please try again." : `“${name}” is protected by a password.`}
  </div>
  <input type="password" aria-label="Password" bind:value={password} data-autofocus />
  {#snippet buttons()}
    <button type="button" onclick={() => close()}>Cancel</button>
    <button type="submit" class="primary">Open</button>
  {/snippet}
</Dialog>

<style>
  .msg {
    color: var(--fg-dim);
    overflow-wrap: anywhere;
  }
  input {
    height: 26px;
    padding: 0 6px;
  }
</style>
