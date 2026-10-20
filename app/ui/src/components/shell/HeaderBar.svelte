<script lang="ts">
  // Custom title bar: action buttons in groups (left), draggable space, menus (right) and
  // window controls. Layout from Pinta-plus (see CLAUDE.md "Header bar").
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import ActionButton from "../ActionButton.svelte";
  import Icon from "../Icon.svelte";
  import MenuButton from "./MenuButton.svelte";
  import type { MenuItem } from "./menu.svelte";
  import { theme } from "../../lib/theme.svelte";
  import { addons } from "../../lib/addons.svelte";
  import { EFFECT_ACTIONS } from "../../lib/commands";

  const win = getCurrentWindow();
  let maximized = $state(true);

  onMount(() => {
    const update = async () => (maximized = await win.isMaximized());
    void update();
    const un = win.onResized(update);
    return () => void un.then((f) => f());
  });

  const BASE_GROUPS = [
    ["file-new", "file-open", "file-save", "file-save-as"],
    ["edit-undo", "edit-redo"],
    ["edit-cut", "edit-copy", "edit-paste"],
    ["image-crop-to-selection", "select-none"],
  ];
  // Installed add-ons add their header buttons as the last group (e.g. AI Remove
  // Background); without add-ons there is nothing.
  let groups = $derived.by(() => {
    const extra = addons.actionsFor("header");
    return extra.length ? [...BASE_GROUPS, extra] : BASE_GROUPS;
  });

  const viewMenu: MenuItem[] = [
    { action: "view-zoom-in" },
    { action: "view-zoom-out" },
    { action: "view-zoom-fit" },
    {
      label: "Zoom",
      items: [
        { action: "view-zoom-16-1" },
        { action: "view-zoom-8-1" },
        { action: "view-zoom-4-1" },
        { action: "view-zoom-2-1" },
        { action: "view-zoom-1-1" },
        { action: "view-zoom-1-2" },
        { action: "view-zoom-1-4" },
        { action: "view-zoom-1-8" },
        { action: "view-zoom-1-16" },
      ],
    },
    { separator: true },
    {
      label: "Theme",
      items: [
        { action: "view-theme-system", checked: () => theme.mode === "system" },
        { action: "view-theme-light", checked: () => theme.mode === "light" },
        { action: "view-theme-dark", checked: () => theme.mode === "dark" },
      ],
    },
  ];

  const imageMenu: MenuItem[] = [
    { action: "image-crop-to-selection" },
    { separator: true },
    { action: "image-scale" },
    { action: "image-canvas-size" },
    { separator: true },
    { action: "image-flip-h" },
    { action: "image-flip-v" },
    { action: "image-rotate-90" },
    { action: "image-rotate-270" },
    { action: "image-rotate-180" },
    { separator: true },
    { action: "image-guides-remove" },
  ];

  const colorsMenu: MenuItem[] = [
    { action: "colors-exposure" },
    { action: "colors-saturation" },
    { action: "colors-desaturate" },
    { action: "colors-invert" },
  ];

  // Built-in effects, then (below a separator) the effects of installed add-ons.
  let effectsMenu = $derived.by((): MenuItem[] => {
    const base: MenuItem[] = EFFECT_ACTIONS.map((action) => ({ action }));
    const ids = addons.actionsFor("effects");
    return ids.length ? [...base, { separator: true }, ...ids.map((action) => ({ action }))] : base;
  });

  const mainMenu: MenuItem[] = [
    { action: "file-new" },
    { action: "file-open" },
    { action: "file-save" },
    { action: "file-save-as" },
    { separator: true },
    {
      label: "Edit",
      items: [
        { action: "edit-undo" },
        { action: "edit-redo" },
        { separator: true },
        { action: "edit-cut" },
        { action: "edit-copy" },
        { action: "edit-paste" },
        { action: "edit-paste-as-new" },
        { action: "edit-clear" },
      ],
    },
    {
      label: "Layer",
      items: [
        { action: "layer-new" },
        { action: "layer-duplicate" },
        { action: "layer-delete" },
        { separator: true },
        { action: "layer-merge-down" },
        { action: "layer-merge-selected" },
        { action: "layer-merge-visible" },
        { separator: true },
        { action: "layer-raise" },
        { action: "layer-lower" },
        { separator: true },
        { action: "layer-flip-h" },
        { action: "layer-flip-v" },
        { action: "layer-to-image-size" },
        { separator: true },
        { action: "layer-import" },
        { action: "layer-properties" },
      ],
    },
    {
      label: "Select",
      items: [{ action: "select-all" }, { action: "select-none" }, { action: "select-invert" }],
    },
    { separator: true },
    { action: "app-settings" },
    { action: "app-addons" },
    { action: "app-about" },
    { separator: true },
    { action: "file-close" },
    { action: "file-close-all" },
    { action: "file-quit" },
  ];
</script>

<header class="header" data-tauri-drag-region>
  <div class="actions">
    {#each groups as group, i (i)}
      {#if i > 0}<span class="sep"><Icon name="more_vert" size={16} /></span>{/if}
      {#each group as action (action)}<ActionButton {action} />{/each}
    {/each}
  </div>

  <div class="title" data-tauri-drag-region>Farba</div>

  <nav class="menus">
    <MenuButton id="view" icon="visibility" title="View" items={viewMenu} />
    <MenuButton id="image" icon="image" title="Image" items={imageMenu} />
    <MenuButton id="colors" icon="instant_mix" title="Adjustments" items={colorsMenu} />
    <MenuButton id="effects" icon="deblur" title="Effects" items={effectsMenu} />
    <MenuButton id="main" icon="menu" title="Main Menu" items={mainMenu} />
  </nav>

  <div class="window-controls">
    <button title="Minimize" onclick={() => win.minimize()}><Icon name="remove" size={16} /></button
    >
    <button title={maximized ? "Restore" : "Maximize"} onclick={() => win.toggleMaximize()}>
      <Icon name={maximized ? "filter_none" : "crop_square"} size={14} />
    </button>
    <button class="close" title="Close" onclick={() => win.close()}>
      <Icon name="close" size={18} />
    </button>
  </div>
</header>

<style>
  .header {
    display: flex;
    align-items: center;
    height: var(--header-h);
    padding-left: 6px;
    background: var(--bg-header);
    border-bottom: 1px solid var(--border);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 1px;
  }
  .sep {
    display: inline-flex;
    margin: 0 2px;
    opacity: 0.35;
  }
  .title {
    flex: 1;
    align-self: stretch;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-dim);
  }
  .menus {
    display: flex;
    gap: 1px;
    margin-right: 6px;
  }
  .window-controls {
    display: flex;
    align-self: stretch;
  }
  .window-controls button {
    width: 46px;
    border: 0;
    background: transparent;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .window-controls button:hover {
    background: var(--hover);
  }
  .window-controls .close:hover {
    background: var(--close-hover);
    color: var(--close-hover-fg);
  }
</style>
