<script lang="ts">
  // Main menu > Addons Manager: the installed add-ons (name, version, author, licence,
  // description), their settings (described by each add-on, like the owner's Pinta-plus
  // Add-in Manager), Uninstall, and Install from file (a `.fba` package).
  import { invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import Dialog from "./Dialog.svelte";
  import Icon from "../Icon.svelte";
  import {
    addons,
    addonSettings,
    addonSettingsAction,
    actionId,
    addonValues,
    formatBytes,
    setAddonValue,
    type Addon,
    type AddonSetting,
  } from "../../lib/addons.svelte";
  import { confirmAction, type DialogProps } from "../../lib/dialogs.svelte";
  import { errorText } from "../../lib/documents.svelte";
  import { notify } from "../../lib/notify.svelte";

  let { close }: DialogProps<boolean> = $props();

  /** The add-on whose settings are open, and its settings. */
  let open_ = $state<string | undefined>();
  let items = $state<AddonSetting[]>([]);
  let busy = $state(false);
  let error = $state("");

  async function showSettings(a: Addon) {
    if (open_ === a.id) {
      open_ = undefined;
      return;
    }
    open_ = a.id;
    await loadSettings(a.id);
  }

  async function loadSettings(id: string) {
    error = "";
    try {
      items = await addonSettings(id);
    } catch (e) {
      items = [];
      error = errorText(e);
    }
  }

  async function set(id: string, key: string, value: unknown) {
    setAddonValue(id, key, value);
    await loadSettings(id);
  }

  async function storageAction(id: string, s: AddonSetting & { type: "storage" }) {
    if (s.confirm && !(await confirmAction(s.label, s.confirm, "Delete"))) return;
    try {
      const msg = await addonSettingsAction(id, s.action);
      if (msg) notify(msg);
    } catch (e) {
      notify(errorText(e), "error");
    }
    await loadSettings(id);
  }

  async function install() {
    const path = await open({
      title: "Install Add-on",
      multiple: false,
      // `.farba-addon` was the extension of the first packages.
      filters: [{ name: "Farba add-ons", extensions: ["fba", "farba-addon"] }],
    });
    if (typeof path !== "string") return;
    busy = true;
    try {
      const r = await invoke<{ manifest: Addon; restart: boolean }>("addon_install", { path });
      await addons.refresh();
      notify(
        r.restart
          ? `${r.manifest.name} ${r.manifest.version} is installed; restart Farba to use the new version`
          : `${r.manifest.name} ${r.manifest.version} is installed`,
      );
    } catch (e) {
      notify(`Could not install: ${errorText(e)}`, "error");
    } finally {
      busy = false;
    }
  }

  async function uninstall(a: Addon) {
    const ok = await confirmAction(
      "Uninstall Add-on",
      `Uninstall “${a.name}”? Its data (e.g. downloaded models) is deleted too.`,
      "Uninstall",
    );
    if (!ok) return;
    try {
      const restart = await invoke<boolean>("addon_uninstall", { id: a.id });
      if (open_ === a.id) open_ = undefined;
      await addons.refresh();
      notify(restart ? `${a.name} is removed when Farba restarts` : `${a.name} is uninstalled`);
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  void addons.refresh();
</script>

<Dialog title="Addons Manager" width={520} onsubmit={() => close(true)} oncancel={() => close()}>
  {#if addons.list.length === 0}
    <div class="empty">
      No add-ons are installed. Add-ons come as <code>.fba</code> files: use “Install from file…” below.
    </div>
  {/if}
  {#each addons.list as a (a.id)}
    <div class="addon">
      <div class="head">
        <Icon name="extension" size={20} />
        <div class="title">
          <div class="name">{a.name} <span class="ver">{a.version}</span></div>
          <div class="meta">
            {a.author}{a.author && a.license ? " · " : ""}{a.license}
            {#if a.pending === "remove"}<span class="pending">· removed at restart</span>{/if}
            {#if a.pending === "update"}<span class="pending">· updated at restart</span>{/if}
          </div>
        </div>
        {#if a.pending !== "remove"}
          <button type="button" class:on={open_ === a.id} onclick={() => showSettings(a)}>
            <Icon name="settings" size={16} /> Settings
          </button>
        {/if}
        <button type="button" onclick={() => uninstall(a)}>Uninstall</button>
      </div>
      {#if a.description}<div class="desc">{a.description}</div>{/if}

      {#if open_ === a.id}
        <div class="settings">
          {#if error}<div class="error">{error}</div>{/if}
          {#each a.actions.filter((x) => x.places.includes("header")) as x (x.id)}
            <label class="check">
              <input
                type="checkbox"
                checked={!addons.headerHidden[actionId(a, x)]}
                onchange={(e) => addons.setOnHeader(actionId(a, x), e.currentTarget.checked)}
              />
              <span>Show “{x.label}” on the top bar (otherwise only in the Effects menu)</span>
            </label>
          {/each}
          {#each items as s, i (i)}
            {#if s.type === "select"}
              <label class="row">
                <span class="label">{s.label}</span>
                <select value={s.value} onchange={(e) => set(a.id, s.key, e.currentTarget.value)}>
                  {#each s.choices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
                </select>
              </label>
            {:else if s.type === "switch"}
              <label class="row">
                <span class="label">{s.label}</span>
                <span class="switch">
                  <input
                    type="checkbox"
                    role="switch"
                    checked={s.value}
                    onchange={(e) => set(a.id, s.key, e.currentTarget.checked)}
                  />
                  {#if s.on || s.off}<span class="state">{s.value ? s.on : s.off}</span>{/if}
                </span>
              </label>
            {:else if s.type === "storage"}
              <div class="row">
                <span class="label">{s.label}</span>
                <span class="storage">
                  <span>{formatBytes(s.bytes)}</span>
                  <button
                    type="button"
                    class="small"
                    onclick={() => void invoke("addon_open_folder", { path: s.folder })}
                  >
                    Folder
                  </button>
                  <button
                    type="button"
                    class="small"
                    disabled={!s.bytes || !!addons.running}
                    onclick={() => storageAction(a.id, s)}
                  >
                    Delete
                  </button>
                </span>
              </div>
            {:else if s.type === "note"}
              <p class="note">{s.text}</p>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}

  {#snippet buttons()}
    <button type="button" class="grow-left" disabled={busy} onclick={install}>
      Install from file…
    </button>
    <span class="grow"></span>
    <button type="submit" class="primary">Close</button>
  {/snippet}
</Dialog>

<style>
  .empty {
    color: var(--fg-dim);
    line-height: 1.5;
  }
  .addon {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px;
    border: 1px solid var(--border-soft);
    border-radius: 10px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .title {
    flex: 1;
    min-width: 0;
  }
  .name {
    font-weight: 600;
  }
  .ver,
  .meta {
    color: var(--fg-dim);
    font-size: 11px;
    font-weight: normal;
  }
  .pending {
    color: var(--accent);
  }
  .desc {
    color: var(--fg-dim);
    line-height: 1.4;
  }
  .head button,
  .small {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  .head button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border-soft);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .label {
    color: var(--fg-dim);
  }
  select {
    height: 24px;
    max-width: 300px;
  }
  .storage {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .check input {
    margin: 0;
    accent-color: var(--accent);
  }
  .note {
    margin: 0;
    color: var(--fg-faint);
    font-size: 11px;
  }
  .error {
    color: var(--danger);
  }
  /* An on / off switch. Sizes are multiples of 4 px, so they stay whole device pixels at
     125 % / 150 % display scaling (odd sizes there gave soft, smeared edges). */
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .switch input {
    appearance: none;
    position: relative;
    box-sizing: border-box;
    width: 36px;
    height: 20px;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg-input);
    transition: background 0.15s;
    cursor: pointer;
  }
  .switch input::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--fg-dim);
    transition: left 0.15s;
  }
  .switch input:checked {
    border-color: var(--accent);
    background: var(--accent);
  }
  .switch input:checked::after {
    left: 19px;
    background: var(--accent-fg);
  }
  .switch input:focus-visible {
    outline: 2px solid var(--accent-soft);
    outline-offset: 1px;
  }
  .state {
    min-width: 28px;
    color: var(--fg-dim);
  }
</style>
