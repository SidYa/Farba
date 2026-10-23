<script lang="ts">
  // Modal dialog frame: dimmed backdrop, rounded panel with a title, content and a button
  // row. Enter submits (the form's default button), Escape or a click on the backdrop
  // cancels. Focus moves into the dialog when it opens. `preview`: no dimming and the panel
  // sits at the top right, so a live preview on the canvas stays visible (adjustments).
  import type { Snippet } from "svelte";

  let {
    title,
    width = 360,
    onsubmit,
    oncancel,
    children,
    buttons,
    preview = false,
  }: {
    title: string;
    width?: number;
    onsubmit: () => void;
    oncancel: () => void;
    children: Snippet;
    buttons: Snippet;
    preview?: boolean;
  } = $props();

  let panel: HTMLElement;

  $effect(() => {
    const first = panel.querySelector<HTMLElement>(
      "[data-autofocus], input, select, button.primary",
    );
    first?.focus();
    if (first instanceof HTMLInputElement && first.type !== "checkbox") first.select();
    const onKey = (e: KeyboardEvent) => {
      // Stacked dialogs (About > Licenses): only the top one (last in the DOM) cancels.
      const open = document.querySelectorAll('[role="dialog"]');
      if (open[open.length - 1] !== panel) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        oncancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
</script>

<div
  class="backdrop"
  class:preview
  role="presentation"
  onpointerdown={(e) => {
    if (e.target === e.currentTarget && !preview) oncancel();
  }}
>
  <div
    class="panel"
    bind:this={panel}
    style:width="{width}px"
    role="dialog"
    aria-modal="true"
    aria-label={title}
  >
    <form
      onsubmit={(e) => {
        e.preventDefault();
        onsubmit();
      }}
    >
      <header>{title}</header>
      <div class="body">{@render children()}</div>
      <footer>{@render buttons()}</footer>
    </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 500;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--backdrop);
  }
  .backdrop.preview {
    align-items: flex-start;
    justify-content: flex-end;
    padding: 96px 300px 0 0;
    background: transparent;
  }
  .panel {
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    overflow: auto;
    background: var(--bg-popup);
    border: 1px solid var(--border-soft);
    border-radius: 12px;
    box-shadow: var(--shadow);
    animation: pop 0.12s ease-out;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  header {
    padding: 12px 16px 4px;
    font-size: 14px;
    font-weight: 600;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 8px 16px 12px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 16px 14px;
  }
  footer :global(button) {
    min-width: 84px;
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  footer :global(button:hover) {
    background: var(--hover);
  }
  footer :global(button.primary) {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }
  footer :global(button.primary:hover) {
    filter: brightness(1.08);
  }
  footer :global(button.danger) {
    background: var(--danger);
    border-color: var(--danger);
    color: var(--accent-fg);
  }
  footer :global(.grow) {
    flex: 1;
  }
</style>
