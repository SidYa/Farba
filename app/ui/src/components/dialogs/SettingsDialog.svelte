<script lang="ts">
  // Main menu > Settings: app preferences, applied at once and remembered (Close only closes).
  // Sections in a list on the left (owner decision 2026-10-21): Appearance, Canvas, Files,
  // Behaviour, Toolbox; the last one shown is remembered. Add-ons have their settings in
  // the Addons Manager.
  import Dialog from "./Dialog.svelte";
  import Icon from "../Icon.svelte";
  import Slider from "../Slider.svelte";
  import Switch from "../Switch.svelte";
  import type { DialogProps } from "../../lib/dialogs.svelte";
  import { documents } from "../../lib/documents.svelte";
  import { tools } from "../../lib/tools.svelte";
  import { guidePrefs } from "../../lib/guides.svelte";
  import { numberField } from "../../lib/numberfield";
  import {
    ACCENTS,
    prefs,
    UI_SCALES,
    UNDO_DEFAULT,
    type Accent,
    type FlatFormat,
    type OpenZoom,
  } from "../../lib/prefs.svelte";
  import { loadSetting, saveSetting } from "../../lib/settings";
  import { cssColor, theme, type ThemeMode } from "../../lib/theme.svelte";
  import {
    canvasPrefs,
    type TransparencySize,
    type TransparencyStyle,
  } from "../../lib/canvasprefs.svelte";

  let { close }: DialogProps<boolean> = $props();

  const SECTIONS = [
    { id: "appearance", label: "Appearance", icon: "palette" },
    { id: "canvas", label: "Canvas", icon: "aspect_ratio" },
    { id: "files", label: "Files", icon: "save" },
    { id: "behaviour", label: "Behaviour", icon: "tune" },
    { id: "toolbox", label: "Toolbox", icon: "brush" },
  ] as const;
  type Section = (typeof SECTIONS)[number]["id"];

  let section = $state<Section>(loadSetting<Section>("settings-section", "appearance"));
  function show(s: Section) {
    section = s;
    saveSetting("settings-section", s);
  }

  const THEMES: [ThemeMode, string, string][] = [
    ["system", "System", "brightness_auto"],
    ["light", "Light", "light_mode"],
    ["dark", "Dark", "dark_mode"],
  ];

  /** A theme colour (CSS variable) as `#rrggbb` (the defaults of the colour buttons). */
  function themeHex(name: string): string {
    void theme.effective;
    void prefs.canvasBack;
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

  function setUndo(megabytes: number, steps: number) {
    prefs.set("undo", { megabytes, steps });
    for (const d of documents.list) d.applyUndoLimits();
  }
</script>

<Dialog title="Settings" width={640} onsubmit={() => close(true)} oncancel={() => close()}>
  <div class="layout">
    <nav aria-label="Sections">
      {#each SECTIONS as s (s.id)}
        <button
          type="button"
          class:on={section === s.id}
          aria-current={section === s.id}
          data-autofocus={section === s.id ? "" : undefined}
          onclick={() => show(s.id)}
        >
          <Icon name={s.icon} size={18} />
          {s.label}
        </button>
      {/each}
    </nav>

    <div class="page">
      {#if section === "appearance"}
        <div class="item">
          <div class="text"><div class="name">Theme</div></div>
          <div class="segmented" role="radiogroup" aria-label="Theme">
            {#each THEMES as [mode, label, icon] (mode)}
              <button
                type="button"
                role="radio"
                aria-checked={theme.mode === mode}
                class:on={theme.mode === mode}
                onclick={() => theme.set(mode)}
              >
                <Icon name={icon} size={16} />
                {label}
              </button>
            {/each}
          </div>
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Accent colour</div>
            <div class="hint">Selected buttons, switches, sliders and highlights.</div>
          </div>
          <div class="swatches" role="radiogroup" aria-label="Accent colour">
            {#each Object.entries(ACCENTS) as [id, [dark, light]] (id)}
              <button
                type="button"
                role="radio"
                class="swatch"
                class:on={prefs.accent === id}
                aria-checked={prefs.accent === id}
                aria-label={id}
                title={id[0]!.toUpperCase() + id.slice(1)}
                style:--c={theme.effective === "dark" ? dark : light}
                onclick={() => prefs.set("accent", id as Accent)}
              ></button>
            {/each}
          </div>
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Interface size</div>
            <div class="hint">Scales the whole window: text, icons and panels.</div>
          </div>
          <select
            value={prefs.uiScale}
            onchange={(e) => prefs.set("uiScale", Number(e.currentTarget.value))}
          >
            {#each UI_SCALES as s (s)}
              <option value={s}>{Math.round(s * 100)}%</option>
            {/each}
          </select>
        </div>
      {:else if section === "canvas"}
        <div class="item">
          <div class="text"><div class="name">Zoom when opening</div></div>
          <select
            value={prefs.openZoom}
            onchange={(e) => prefs.set("openZoom", e.currentTarget.value as OpenZoom)}
          >
            <option value="fit">Fit in window, at most 100%</option>
            <option value="fill">Fit in window</option>
            <option value="actual">100%</option>
          </select>
        </div>
        <div class="item">
          <div class="text"><div class="name">Mouse wheel</div></div>
          <select
            value={prefs.wheelZooms ? "zoom" : "scroll"}
            onchange={(e) => prefs.set("wheelZooms", e.currentTarget.value === "zoom")}
          >
            <option value="scroll">Scrolls (Ctrl+wheel zooms)</option>
            <option value="zoom">Zooms (Ctrl+wheel scrolls)</option>
          </select>
        </div>
        <div class="item">
          <div class="text"><div class="name">Background around the image</div></div>
          <div class="controls">
            <input
              class="color"
              type="color"
              title="Background around the image"
              value={prefs.canvasBack ?? themeHex("--bg-canvas")}
              oninput={(e) => prefs.set("canvasBack", e.currentTarget.value)}
            />
            <button
              type="button"
              class="small"
              disabled={!prefs.canvasBack}
              title="Use the theme's colour"
              onclick={() => prefs.set("canvasBack", null)}
            >
              Theme color
            </button>
          </div>
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Transparency</div>
            <div class="hint">How transparent pixels are shown.</div>
          </div>
          <div class="controls">
            <select
              value={canvasPrefs.transparency}
              onchange={(e) =>
                canvasPrefs.setTransparency(e.currentTarget.value as TransparencyStyle)}
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
            {:else}
              <input
                class="color"
                type="color"
                title="Colour shown for transparent pixels"
                value={canvasPrefs.solid}
                oninput={(e) => canvasPrefs.setSolid(e.currentTarget.value)}
              />
            {/if}
          </div>
        </div>
        {#if canvasPrefs.transparency === "dots"}
          <div class="item sub">
            <div class="text"><div class="name">Dot colors</div></div>
            <div class="controls">
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
            </div>
          </div>
        {/if}
        <div class="item">
          <div class="text">
            <div class="name">Brush outline</div>
            <div class="hint">A circle of the brush size follows the pointer.</div>
          </div>
          <Switch
            label="Brush outline"
            checked={prefs.brushOutline}
            onchange={(on) => prefs.set("brushOutline", on)}
          />
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Rulers and guides</div>
            <div class="hint">
              Drag from a ruler onto the image to add a guide; drag a guide to move it, back onto a
              ruler to remove it. Rectangle and Ellipse Select snap to guides.
            </div>
          </div>
          <Switch
            label="Rulers and guides"
            checked={guidePrefs.show}
            onchange={(on) => guidePrefs.setShow(on)}
          />
        </div>
      {:else if section === "files"}
        <div class="item">
          <div class="text">
            <div class="name">Save single-layer images as</div>
            <div class="hint">The format Save proposes; images with layers are saved as .ora.</div>
          </div>
          <select
            value={prefs.flatFormat}
            onchange={(e) => prefs.set("flatFormat", e.currentTarget.value as FlatFormat)}
          >
            <option value="png">PNG</option>
            <option value="jpg">JPEG</option>
            <option value="webp">WebP</option>
          </select>
        </div>
        <div class="item column">
          <Slider
            label="JPEG quality"
            min={1}
            max={100}
            bind:value={() => prefs.jpegQuality, (v) => prefs.set("jpegQuality", v)}
          />
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Ask for the quality</div>
            <div class="hint">
              On the first JPEG / HEIC / AVIF save of an image and with Save As. Off: the quality
              above (HEIC / AVIF: the last one chosen) is used.
            </div>
          </div>
          <Switch
            label="Ask for the quality"
            checked={prefs.askQuality}
            onchange={(on) => prefs.set("askQuality", on)}
          />
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Reopen the last tabs</div>
            <div class="hint">At start, open the files that were open when Farba was closed.</div>
          </div>
          <Switch
            label="Reopen the last tabs"
            checked={prefs.reopenTabs}
            onchange={(on) => prefs.set("reopenTabs", on)}
          />
        </div>
      {:else if section === "behaviour"}
        <div class="item">
          <div class="text"><div class="name">Ask before deleting layers</div></div>
          <Switch
            label="Ask before deleting layers"
            checked={prefs.askDeleteLayer}
            onchange={(on) => prefs.set("askDeleteLayer", on)}
          />
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Ask before closing all tabs</div>
            <div class="hint">Unsaved changes are always asked about.</div>
          </div>
          <Switch
            label="Ask before closing all tabs"
            checked={prefs.askCloseAll}
            onchange={(on) => prefs.set("askCloseAll", on)}
          />
        </div>
        <div class="item">
          <div class="text">
            <div class="name">Undo history</div>
            <div class="hint">
              Per image; the oldest steps are dropped beyond either limit. More memory keeps more
              steps of large images.
            </div>
          </div>
          <div class="controls">
            <input
              class="num"
              aria-label="Undo memory (MB)"
              use:numberField={{
                value: prefs.undo.megabytes,
                min: 64,
                max: 16384,
                step: 64,
                onchange: (v) => setUndo(Math.round(v), prefs.undo.steps),
              }}
            />
            <span class="unit">MB</span>
            <input
              class="num"
              aria-label="Undo steps"
              use:numberField={{
                value: prefs.undo.steps,
                min: 1,
                max: 1000,
                step: 10,
                onchange: (v) => setUndo(prefs.undo.megabytes, Math.round(v)),
              }}
            />
            <span class="unit">steps</span>
            <button
              type="button"
              class="small"
              title="512 MB, 200 steps"
              disabled={prefs.undo.megabytes === UNDO_DEFAULT.megabytes &&
                prefs.undo.steps === UNDO_DEFAULT.steps}
              onclick={() => setUndo(UNDO_DEFAULT.megabytes, UNDO_DEFAULT.steps)}
            >
              Reset
            </button>
          </div>
        </div>
      {:else}
        <div class="item">
          <div class="text">
            <div class="name">Group similar tools</div>
            <div class="hint">
              One button per group (hold it to pick another tool of the group). Off: every tool has
              its own button.
            </div>
          </div>
          <Switch
            label="Group similar tools"
            checked={tools.grouped}
            onchange={(on) => tools.setGrouped(on)}
          />
        </div>
      {/if}
    </div>
  </div>

  {#snippet buttons()}
    <button type="submit" class="primary">Close</button>
  {/snippet}
</Dialog>

<style>
  .layout {
    display: grid;
    grid-template-columns: 148px 1fr;
    gap: 16px;
    height: 380px;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: 12px;
    border-right: 1px solid var(--border-soft);
  }
  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--fg-dim);
    text-align: left;
    cursor: pointer;
  }
  nav button:hover {
    background: var(--hover);
    color: var(--fg);
  }
  nav button.on {
    background: var(--accent-soft);
    color: var(--fg);
    font-weight: 600;
  }
  .page {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    overflow-y: auto;
    padding-right: 4px;
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 32px;
    padding: 8px 12px;
    border: 1px solid var(--border-soft);
    border-radius: 8px;
    background: var(--bg-panel);
  }
  .item.sub {
    margin: -4px 0 0 16px;
  }
  .item.column {
    display: block;
  }
  .text {
    min-width: 0;
  }
  .name {
    color: var(--fg);
  }
  .hint {
    margin-top: 2px;
    color: var(--fg-faint);
    font-size: 11px;
  }
  .controls {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
  }
  select {
    flex: none;
    height: 24px;
  }
  .segmented {
    display: inline-flex;
    flex: none;
    border: 1px solid var(--border-soft);
    border-radius: 6px;
    overflow: hidden;
  }
  .segmented button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 10px;
    border: 0;
    background: var(--bg-input);
    color: var(--fg-dim);
    cursor: pointer;
  }
  .segmented button + button {
    border-left: 1px solid var(--border-soft);
  }
  .segmented button.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .swatches {
    display: inline-flex;
    flex: none;
    gap: 8px;
  }
  .swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--c);
    outline: 2px solid transparent;
    outline-offset: 2px;
    cursor: pointer;
  }
  .swatch.on {
    outline-color: var(--c);
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
  .num {
    width: 56px;
    height: 24px;
    padding: 0 6px;
    text-align: right;
  }
  .unit {
    color: var(--fg-faint);
  }
</style>
