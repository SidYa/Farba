<script lang="ts">
  // Main menu > Settings: app preferences, applied at once and remembered (Close only closes).
  // Sections: Canvas, Toolbox. (The theme is in the View menu; add-ons have their settings in the
  // Addons Manager.)
  import Dialog from "./Dialog.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { tools } from "../../lib/tools.svelte";
  import { guidePrefs } from "../../lib/guides.svelte";
  import { cssColor, theme } from "../../lib/theme.svelte";
  import {
    canvasPrefs,
    type TransparencySize,
    type TransparencyStyle,
  } from "../../lib/canvasprefs.svelte";

  let { close }: DialogProps<boolean> = $props();

  /** A theme colour (CSS variable) as `#rrggbb` (the dots' default colours). */
  function themeHex(name: string): string {
    void theme.effective;
    return (
      "#" +
      cssColor(name)
        .map((v) =>
          Math.round(v * 255)
            .toString(16)
            .padStart(2, "0"),
        )
        .join("")
    );
  }
</script>

<Dialog title="Settings" width={380} onsubmit={() => close(true)} oncancel={() => close()}>
  <section>
    <h3>Canvas</h3>
    <div class="row">
      <span class="label">Transparency</span>
      <span class="controls">
        <select
          value={canvasPrefs.transparency}
          onchange={(e) => canvasPrefs.setTransparency(e.currentTarget.value as TransparencyStyle)}
        >
          <option value="checker">Checkerboard</option>
          <option value="dots">Dots</option>
          <option value="solid">Solid color</option>
        </select>
        {#if canvasPrefs.transparency !== "solid"}
          <select
            title="Size of the checks / dots"
            value={canvasPrefs.size}
            onchange={(e) => canvasPrefs.setSize(e.currentTarget.value as TransparencySize)}
          >
            <option value="minimal">Minimal</option>
            <option value="medium">Medium</option>
            <option value="big">Big</option>
          </select>
        {/if}
        {#if canvasPrefs.transparency === "solid"}
          <input
            class="color"
            type="color"
            title="Colour shown for transparent pixels"
            value={canvasPrefs.solid}
            oninput={(e) => canvasPrefs.setSolid(e.currentTarget.value)}
          />
        {/if}
      </span>
    </div>
    {#if canvasPrefs.transparency === "dots"}
      <div class="row">
        <span class="label">Dot colors</span>
        <span class="controls">
          <input
            class="color"
            type="color"
            title="Background"
            value={canvasPrefs.dotsBack ?? themeHex("--check-a")}
            oninput={(e) =>
              canvasPrefs.setDots(
                e.currentTarget.value,
                canvasPrefs.dotsFore ?? themeHex("--check-b"),
              )}
          />
          <input
            class="color"
            type="color"
            title="Dots"
            value={canvasPrefs.dotsFore ?? themeHex("--check-b")}
            oninput={(e) =>
              canvasPrefs.setDots(
                canvasPrefs.dotsBack ?? themeHex("--check-a"),
                e.currentTarget.value,
              )}
          />
          <button
            type="button"
            class="small"
            disabled={!canvasPrefs.dotsBack && !canvasPrefs.dotsFore}
            title="Use the theme's colours"
            onclick={() => canvasPrefs.setDots(null, null)}
          >
            Theme colors
          </button>
        </span>
      </div>
    {/if}
    <label class="check">
      <input
        type="checkbox"
        checked={guidePrefs.show}
        onchange={(e) => guidePrefs.setShow(e.currentTarget.checked)}
      />
      <span>Show rulers and guides</span>
    </label>
    <p class="hint">
      Drag from a ruler onto the image to add a guide; drag it with the Move tool, off the image to
      remove it. Rectangle and Ellipse Select snap to guides.
    </p>
  </section>

  <section>
    <h3>Toolbox</h3>
    <label class="check">
      <input
        type="checkbox"
        checked={tools.grouped}
        onchange={(e) => tools.setGrouped(e.currentTarget.checked)}
      />
      <span>Group similar tools</span>
    </label>
    <p class="hint">
      One button per group (hold it to pick another tool of the group). Off: every tool has its own
      button.
    </p>
  </section>

  {#snippet buttons()}
    <button type="submit" class="primary">Close</button>
  {/snippet}
</Dialog>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-dim);
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
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .label {
    color: var(--fg-dim);
  }
  .controls {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  select {
    height: 24px;
  }
  .small {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: var(--bg-input);
  }
  .color {
    width: 32px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border-soft);
    border-radius: var(--radius);
    background: transparent;
  }
  .hint {
    margin: 0 0 0 22px;
    color: var(--fg-faint);
    font-size: 11px;
  }
</style>
