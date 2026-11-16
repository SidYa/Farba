<script lang="ts">
  // Main menu > About Farba: logo, version, author and a PayPal donation button (owner
  // request 2026-10-14). The backend opens the fixed PayPal page in the default browser
  // (`open_donate`); the address is not shown (owner request). Licenses opens the
  // third-party notices on top of this dialog.
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import Dialog from "./Dialog.svelte";
  import Icon from "../Icon.svelte";
  import LicensesDialog from "./LicensesDialog.svelte";
  import logo from "../../../../src-tauri/icons/128x128@2x.png";
  import { openDialog, type DialogProps } from "../../lib/dialogs.svelte";

  let { close }: DialogProps<void> = $props();

  let version = $state("");
  void getVersion().then((v) => (version = v));
</script>

<Dialog title="About Farba" width={360} onsubmit={() => close()} oncancel={() => close()}>
  <div class="head">
    <img src={logo} alt="" width="64" height="64" />
    <div>
      <div class="name">Farba</div>
      {#if version}<div class="dim">Version {version}</div>{/if}
      <div class="dim">Fast, light and useful image editor</div>
      <div class="dim">© 2026 Sergii Sidko</div>
    </div>
  </div>
  <div class="donate">The author will be grateful for any donation.</div>
  {#snippet buttons()}
    <!-- PayPal on the left of the button row, Licenses and Close on the right (owner request). -->
    <button type="button" class="paypal" onclick={() => void invoke("open_donate")}>
      <Icon name="favorite" size={16} />
      PayPal
    </button>
    <button type="button" class="with-icon" onclick={() => void openDialog(LicensesDialog, {})}>
      <Icon name="license" size={16} />
      Licenses
    </button>
    <button type="submit" class="primary">Close</button>
  {/snippet}
</Dialog>

<style>
  .head {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .name {
    font-size: 18px;
    font-weight: 600;
  }
  .dim {
    color: var(--fg-dim);
  }
  .donate {
    padding-top: 10px;
    border-top: 1px solid var(--border-soft);
  }
  .paypal {
    margin-right: auto;
    display: inline-flex;
    gap: 6px;
    align-items: center;
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-fg);
    cursor: pointer;
    /* The heart in the text colour: its accent colour is the button's background. */
    --icon-accent: currentColor;
  }
  /* Stays the accent colour under the pointer (the other buttons turn grey, on which the
     light text would not read). */
  .paypal:hover {
    background: var(--accent);
    filter: brightness(1.1);
  }
  .with-icon {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
  }
</style>
