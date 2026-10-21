<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import Viewport from "./components/canvas/Viewport.svelte";
  import ImageTabs from "./components/canvas/ImageTabs.svelte";
  import RightPanel from "./components/panels/RightPanel.svelte";
  import HeaderBar from "./components/shell/HeaderBar.svelte";
  import StatusBar from "./components/shell/StatusBar.svelte";
  import Toolbox from "./components/shell/Toolbox.svelte";
  import ColorDialog from "./components/color/ColorDialog.svelte";
  import DialogHost from "./components/dialogs/DialogHost.svelte";
  import { colorDialog } from "./lib/colordialog.svelte";
  import { handleKeydown } from "./lib/actions";
  import { registerCommands } from "./lib/commands";
  import { initDragDrop } from "./lib/dragdrop.svelte";
  import { confirmQuit, openPaths, rememberTabs, reopenTabs } from "./lib/files";
  import { prefs } from "./lib/prefs.svelte";
  import { theme } from "./lib/theme.svelte";
  import { loadCore, warm_up_heap } from "./lib/wasm/core";
  import { addons } from "./lib/addons.svelte";
  import { documents } from "./lib/documents.svelte";

  registerCommands();

  $effect(() => theme.apply());
  $effect(() => prefs.applyColors());
  $effect(() => prefs.applyScale());
  // Settings > Files > "Reopen the last tabs": remember the open files.
  $effect(() => {
    if (prefs.reopenTabs) rememberTabs();
  });

  function firstFrame() {
    // The window is created hidden; show it once something is painted (no white flash).
    void getCurrentWindow()
      .show()
      .then(() => invoke("startup_mark", { stage: "first-frame" }));
    // Pre-grow the WASM heap while idle so the first image open does not pay for it.
    const idle = window.requestIdleCallback ?? ((f: () => void) => setTimeout(f, 200));
    idle(() => void loadCore().then(() => warm_up_heap(256)));
  }

  onMount(() => {
    void invoke("startup_mark", { stage: "mount" });
    void loadCore();
    void addons.refresh();
    void reopenTabs()
      .then(() => invoke<string[]>("startup_files"))
      .then(openPaths);
    // Files from another start of Farba (single instance: opened here as tabs).
    const unFiles = listen<string[]>("open-files", (e) => void openPaths(e.payload));

    // Closing the window (title-bar button, Alt+F4, Quit) asks about unsaved changes.
    const win = getCurrentWindow();
    let quitting = false;
    const un = win.onCloseRequested(async (e) => {
      e.preventDefault();
      if (quitting) return;
      quitting = true;
      try {
        if (await confirmQuit()) await win.destroy();
      } finally {
        quitting = false;
      }
    });
    const unDrop = initDragDrop();
    return () => {
      void un.then((f) => f());
      void unFiles.then((f) => f());
      unDrop();
    };
  });
</script>

<svelte:window
  onkeydown={handleKeydown}
  onkeydowncapture={(e) => {
    // A result is being computed (native.ts `whileBusy`): no edits meanwhile.
    if (documents.blocking) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  }}
/>

<div class="app">
  <HeaderBar />
  <div class="main">
    <Toolbox />
    <div class="center">
      <ImageTabs />
      <Viewport onfirstframe={firstFrame} />
    </div>
    <RightPanel />
  </div>
  <StatusBar />
</div>
{#if colorDialog.open}
  {#key colorDialog.serial}
    <ColorDialog primary={colorDialog.primary} />
  {/key}
{/if}
<DialogHost />
{#if documents.blocking}
  <div
    class="blocker"
    aria-hidden="true"
    onpointerdowncapture={(e) => e.preventDefault()}
    onwheel={(e) => e.preventDefault()}
  ></div>
{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .main {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .center {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .blocker {
    position: fixed;
    inset: 0;
    z-index: 10000;
    cursor: progress;
  }
</style>
