<script lang="ts">
  // The canvas area: WebGL2 view of the active document, pan/zoom input (GIMP bindings),
  // the "canvas" drop zone (lib/dragdrop.svelte.ts), the selection tools (shapes in progress
  // on a 2D overlay canvas, the outline of the selection drawn by the renderer), and
  // the paint tools (lib/canvas/painttools.svelte.ts) and the Color Picker.
  import { onMount, untrack } from "svelte";
  import TextStyleEditor from "./TextStyleEditor.svelte";
  import Ruler from "./Ruler.svelte";
  import {
    RULER,
    finishGuideDrag,
    guideAt,
    guideDrag,
    guidePos,
    guidePrefs,
    snap,
    visibleGuides,
  } from "../../lib/guides.svelte";
  import { textTool } from "../../lib/canvas/texttool.svelte";
  import {
    Renderer,
    type DrawLayer,
    type GpuImage,
    type Scene,
    type TileSource,
  } from "../../lib/canvas/renderer";
  import {
    TRANSFORM_TOOLS,
    transformTools,
    type Session,
  } from "../../lib/canvas/transformtools.svelte";
  import { isTextInput } from "../../lib/actions";
  import { confirmRasterize, rasterAllowed } from "../../lib/textguard";
  import { moveTool } from "../../lib/canvas/movetool.svelte";
  import { ALT_PICK_TOOLS, PAINT_TOOLS, paintTools } from "../../lib/canvas/painttools.svelte";
  import { SELECTION_TOOLS, selectTools } from "../../lib/canvas/selecttools.svelte";
  import { documents, onDocumentClosed, type Doc } from "../../lib/documents.svelte";
  import { dialogs } from "../../lib/dialogs.svelte";
  import { dragState } from "../../lib/dragdrop.svelte";
  import { tile_size } from "../../lib/wasm/core";
  import { palette, type Rgba } from "../../lib/palette.svelte";
  import { cssColor, theme } from "../../lib/theme.svelte";
  import { CHECK_SIZES, canvasPrefs, rgb01 } from "../../lib/canvasprefs.svelte";
  import { tools } from "../../lib/tools.svelte";
  import { openView, pointer, screenToImage, viewport, zoomAt } from "../../lib/view.svelte";
  import { prefs } from "../../lib/prefs.svelte";

  let { onfirstframe }: { onfirstframe?: () => void } = $props();

  let canvasEl: HTMLCanvasElement;
  /** 2D canvas over the WebGL one for tool previews (selection shapes, brush outline). */
  let overlayEl: HTMLCanvasElement;
  let overlay: CanvasRenderingContext2D;
  let renderer: Renderer;
  /** GPU images per document, per layer id. */
  const gpu = new Map<number, Map<number, GpuImage>>();
  /** Selection mask textures per document. */
  const masks = new Map<number, GpuImage>();
  let antsPhase = 0;
  let floatTex: { id: number; image: GpuImage } | undefined;
  let textInput: HTMLTextAreaElement;
  // Where the hidden text input sits (at the caret, for the IME window) and the style
  // editor (above the text box), in CSS pixels of the viewport.
  let caretCss = $derived.by(() => {
    void textTool.edit;
    void documents.active?.view;
    const p = textTool.caretScreen();
    return p ? [p[0] / devicePixelRatio, p[1] / devicePixelRatio] : [-100, -100];
  });
  let editorPos = $derived.by(() => {
    const e = textTool.edit;
    if (!e) return [0, 0];
    const v = e.doc.view;
    const dpr = devicePixelRatio;
    return [Math.max(4, (e.x * v.zoom + v.x) / dpr), Math.max(40, (e.y * v.zoom + v.y) / dpr - 8)];
  });
  let frameQueued = false;
  let spaceDown = false;

  let pan: { x: number; y: number; vx: number; vy: number } | undefined;
  /** Color Picker drag: which colour it sets and the last picked colour. */
  let picking: { doc: Doc; primary: boolean; color?: Rgba } | undefined;

  const FULL = 0xffffffff;

  function tileSource(doc: Doc, layer: { id: number; width: number; height: number }): TileSource {
    return {
      tilesX: Math.ceil(layer.width / tile_size()),
      tilesY: Math.ceil(layer.height / tile_size()),
      tilePtr: (tx, ty) => doc.core.tile_ptr(layer.id, tx, ty),
    };
  }

  /**
   * Brings the document's GPU images up to date: re-uploads the tiles the core reports as
   * changed, creates images for new or resized layers and drops those of deleted layers.
   */
  function syncGpu(doc: Doc): Map<number, GpuImage> {
    let images = gpu.get(doc.id);
    if (!images) gpu.set(doc.id, (images = new Map()));
    const changes = doc.core.take_changes();
    const full = new Set<number>();
    for (let i = 0; i < changes.length;) {
      const id = changes[i]!;
      const n = changes[i + 1]!;
      if (n === FULL) {
        full.add(id);
        i += 2;
        continue;
      }
      const img = images.get(id);
      const layer = doc.layer(id);
      if (img && layer && img.width === layer.width && img.height === layer.height)
        img.uploadTiles(tileSource(doc, layer), changes.subarray(i + 2, i + 2 + n));
      i += 2 + n;
    }
    for (const layer of doc.layers) {
      const img = images.get(layer.id);
      if (img && !full.has(layer.id) && img.width === layer.width && img.height === layer.height)
        continue;
      img?.dispose();
      const fresh = renderer.createImage(layer.width, layer.height);
      fresh.uploadAll(tileSource(doc, layer));
      images.set(layer.id, fresh);
    }
    for (const [id, img] of images) {
      if (!doc.layer(id)) {
        img.dispose();
        images.delete(id);
      }
    }
    return images;
  }

  /** Brings the document's selection texture up to date (dropped when nothing is selected). */
  function syncMask(doc: Doc): GpuImage | undefined {
    const changes = doc.core.take_selection_changes();
    let mask = masks.get(doc.id);
    if (!doc.core.selection_tile_ptr(0, 0)) {
      mask?.dispose();
      masks.delete(doc.id);
      return undefined;
    }
    const src: TileSource = {
      tilesX: Math.ceil(doc.core.width / tile_size()),
      tilesY: Math.ceil(doc.core.height / tile_size()),
      tilePtr: (tx, ty) => doc.core.selection_tile_ptr(tx, ty),
    };
    if (
      !mask ||
      mask.width !== doc.core.width ||
      mask.height !== doc.core.height ||
      changes[0] === FULL
    ) {
      mask?.dispose();
      mask = renderer.createImage(doc.core.width, doc.core.height, true);
      mask.uploadAll(src);
      masks.set(doc.id, mask);
    } else if (changes.length) {
      mask.uploadTiles(src, changes);
    }
    return mask;
  }

  /** Texture of the pixels a transform session lifted (made once per session). */
  function floatImage(doc: Doc, s: Session): GpuImage {
    if (floatTex?.id !== s.id) {
      floatTex?.image.dispose();
      const image = renderer.createImage(s.rect[2], s.rect[3]);
      image.uploadAll({
        tilesX: Math.ceil(s.rect[2] / tile_size()),
        tilesY: Math.ceil(s.rect[3] / tile_size()),
        tilePtr: (tx, ty) => doc.core.transform_tile_ptr(tx, ty),
      });
      floatTex = { id: s.id, image };
    }
    return floatTex.image;
  }

  function scene(doc: Doc): Scene {
    const images = syncGpu(doc);
    // A Move tool drag in progress is drawn by offsetting what moves.
    const mv = moveTool.preview?.doc === doc ? moveTool.preview : undefined;
    const off = (id: number) => (mv?.layers.has(id) ? [mv.dx, mv.dy] : [0, 0]);
    // A transform in progress: the lifted pixels are drawn through its matrix, above the
    // layer (which is shown without them) or instead of it (the whole layer moves).
    textTool.check();
    let ts = transformTools.session?.doc === doc ? transformTools.session : undefined;
    if (ts && doc.core.transform_lifted() === undefined) {
      // The core ended it (undo or another change).
      transformTools.forget();
      ts = undefined;
    }
    if (!ts && floatTex) {
      floatTex.image.dispose();
      floatTex = undefined;
    }
    const layers: DrawLayer[] = [];
    for (const l of doc.layers) {
      if (!l.visible) continue;
      const t = ts && l.id === ts.layer ? ts : undefined;
      if (!t || t.lifted)
        layers.push({
          image: images.get(l.id)!,
          x: l.x + off(l.id)[0]!,
          y: l.y + off(l.id)[1]!,
          opacity: l.opacity,
        });
      if (t)
        layers.push({
          image: floatImage(doc, t),
          x: 0,
          y: 0,
          opacity: l.opacity,
          h: transformTools.previewMatrix(t),
        });
    }
    return {
      width: doc.width,
      height: doc.height,
      layers,
      selection: ts ? undefined : syncMask(doc),
      selectionOffset: mv?.selection ? [mv.dx, mv.dy] : undefined,
      antsPhase,
    };
  }

  /** Guides as dashed lines across the canvas; the one being dragged in the active colour. */
  function drawGuides(doc: Doc) {
    const v = doc.view;
    const dragged = guideDrag.doc === doc ? guideDrag : undefined;
    const lines = visibleGuides(doc)
      .filter((_, i) => i !== dragged?.index)
      .map((g) => ({ ...g, active: false }));
    if (dragged?.pos !== undefined)
      lines.push({ horizontal: dragged.horizontal, pos: dragged.pos, active: true });
    if (!lines.length) return;
    const dpr = devicePixelRatio;
    const { width, height } = overlayEl;
    const css = (name: string) =>
      `rgb(${cssColor(name)
        .map((c) => Math.round(c * 255))
        .join(",")})`;
    overlay.save();
    overlay.lineWidth = dpr;
    for (const g of lines) {
      const at = Math.round(g.pos * v.zoom + (g.horizontal ? v.y : v.x)) + 0.5;
      overlay.beginPath();
      if (g.horizontal) {
        overlay.moveTo(0, at);
        overlay.lineTo(width, at);
      } else {
        overlay.moveTo(at, 0);
        overlay.lineTo(at, height);
      }
      // Dashes of the guide colour over a dark line, visible on any image.
      overlay.setLineDash([]);
      overlay.strokeStyle = css("--guide-back");
      overlay.stroke();
      overlay.setLineDash([4 * dpr, 4 * dpr]);
      overlay.strokeStyle = css(g.active ? "--guide-active" : "--guide");
      overlay.stroke();
    }
    overlay.restore();
  }

  function drawOverlay(doc: Doc | undefined) {
    overlay.clearRect(0, 0, overlayEl.width, overlayEl.height);
    if (doc) drawGuides(doc);
    selectTools.drawOverlay(overlay, doc);
    paintTools.drawOverlay(overlay, doc, tools.active);
    transformTools.drawOverlay(overlay, doc);
    if (tools.active === "text" || textTool.edit) textTool.drawOverlay(overlay, doc, blinkOn);
  }

  function drawNow() {
    if (!renderer) return;
    const doc = documents.active;
    if (doc?.needsFit) {
      openView(doc);
      doc.needsFit = false;
    }
    renderer.draw(doc && scene(doc), doc?.view ?? { zoom: 1, x: 0, y: 0 });
    drawOverlay(doc);
  }

  function requestDraw() {
    if (frameQueued || !renderer) return;
    frameQueued = true;
    requestAnimationFrame(() => {
      frameQueued = false;
      drawNow();
    });
  }

  // Redraw on document switch, document changes (layers, undo...), pan/zoom, guides.
  $effect(() => {
    void documents.active?.view;
    void documents.active?.revision;
    void documents.active?.guides;
    void guidePrefs.show;
    requestDraw();
  });

  // Text editing: the caret blinks, the tool options apply to the text being edited, and
  // the editing ends when the tool or the image changes.
  let blinkOn = $state(true);
  $effect(() => {
    if (!textTool.edit) return;
    blinkOn = true;
    const t = setInterval(() => {
      blinkOn = !blinkOn;
      requestDraw();
    }, 530);
    return () => clearInterval(t);
  });
  $effect(() => {
    void JSON.stringify(tools.values.text);
    untrack(() => textTool.optionsChanged());
  });
  $effect(() => {
    textTool.onchange = requestDraw;
    const e = textTool.edit;
    if (e && (tools.active !== "text" || documents.active !== e.doc))
      untrack(() => textTool.finish());
  });

  // Marching ants: animate while the active document has a selection.
  $effect(() => {
    if (!documents.active?.selection) return;
    const t = setInterval(() => {
      antsPhase = (antsPhase + 0.25) % 2;
      requestDraw();
    }, 100);
    return () => clearInterval(t);
  });

  // A shape in progress is dropped when the tool or the document changes.
  $effect(() => {
    void tools.active;
    void documents.active;
    selectTools.cancel();
    paintTools.cancel();
    moveTool.cancel();
    // A transform in progress is applied (GIMP-like: switching away keeps the result).
    if (
      transformTools.session &&
      (transformTools.session.doc !== documents.active ||
        !TRANSFORM_TOOLS.includes(tools.active) ||
        transformTools.session.tool !== tools.active)
    )
      transformTools.apply();
    requestDraw();
  });

  // Theme or transparency display change (Settings > Canvas).
  $effect(() => {
    void theme.effective;
    void prefs.canvasBack;
    const style = canvasPrefs.transparency;
    const solid = rgb01(canvasPrefs.solid);
    const size = CHECK_SIZES[canvasPrefs.size] ?? 8;
    const dotsBack = canvasPrefs.dotsBack;
    const dotsFore = canvasPrefs.dotsFore;
    // Read the colours on the next frame: App applies data-theme in its own effect, which
    // runs after this (child) effect, and the renderer is created in onMount.
    requestAnimationFrame(() => {
      if (!renderer) return;
      renderer.theme = {
        background: cssColor("--bg-canvas"),
        // Dots may have their own colours (Settings); otherwise the theme's.
        checkA: style === "dots" && dotsBack ? rgb01(dotsBack) : cssColor("--check-a"),
        checkB: style === "dots" && dotsFore ? rgb01(dotsFore) : cssColor("--check-b"),
        checkStyle: style === "dots" ? 1 : style === "solid" ? 2 : 0,
        checkSolid: solid,
        checkSize: size,
      };
      requestDraw();
    });
  });

  /** Image point for the selection tools: Rectangle / Ellipse Select snap to guides. */
  function selectPoint(doc: Doc, e: PointerEvent): [number, number] {
    const p = screenToImage(doc.view, ...devicePoint(e));
    return tools.active === "rect-select" || tools.active === "ellipse-select"
      ? snap(doc, ...p)
      : p;
  }

  /** Cursor over a guide: "row" (horizontal), "col" or "". */
  let guideHover = $state("");

  function devicePoint(e: { clientX: number; clientY: number }): [number, number] {
    const r = canvasEl.getBoundingClientRect();
    const dpr = devicePixelRatio;
    return [(e.clientX - r.left) * dpr, (e.clientY - r.top) * dpr];
  }

  function onPointerDown(e: PointerEvent) {
    const doc = documents.active;
    if (!doc) return;
    canvasEl.setPointerCapture(e.pointerId);
    if (e.button === 1 || (e.button === 0 && spaceDown)) {
      pan = { x: e.clientX, y: e.clientY, vx: doc.view.x, vy: doc.view.y };
      e.preventDefault();
      return;
    }
    const tool = tools.active;
    // Any tool drags the guide under the pointer (GIMP: only the Move tool, which the owner
    // missed); dragged back onto a ruler or off the canvas it is removed.
    const gi = e.button === 0 && !e.altKey ? guideAt(doc, doc.view, ...devicePoint(e)) : undefined;
    if (gi !== undefined) {
      const g = doc.guides[gi]!;
      guideDrag.doc = doc;
      guideDrag.horizontal = g.horizontal;
      guideDrag.pos = g.pos;
      guideDrag.index = gi;
      requestDraw();
      return;
    }
    if (e.altKey && ALT_PICK_TOOLS.includes(tool) && (e.button === 0 || e.button === 2)) {
      // Paintbrush / Pencil + Alt: the Color Picker (left: FG, right: BG).
      e.preventDefault();
      picking = { doc, primary: e.button === 0 };
      pickAt(e);
    } else if (e.button === 0 && tool === "text") {
      // Keep the keyboard on the hidden text input (a press would move the focus away).
      e.preventDefault();
      textTool.down(doc, e, screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    } else if (
      e.button === 0 &&
      (TRANSFORM_TOOLS.includes(tool) || PAINT_TOOLS.includes(tool)) &&
      !rasterAllowed(doc)
    ) {
      // A text layer: ask first (the next press paints).
      void confirmRasterize(doc);
    } else if (e.button === 0 && TRANSFORM_TOOLS.includes(tool)) {
      transformTools.down(doc, tool, e, screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    } else if (e.button === 0 && tool === "move") {
      moveTool.down(doc, e, ...screenToImage(doc.view, ...devicePoint(e)));
    } else if (e.button === 0 && PAINT_TOOLS.includes(tool)) {
      paintTools.down(doc, tool, e, ...screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    } else if (e.button === 0 && SELECTION_TOOLS.includes(tool)) {
      selectTools.down(doc, tool, e, ...selectPoint(doc, e));
      requestDraw();
    } else if (tool === "color-picker" && (e.button === 0 || e.button === 2)) {
      picking = { doc, primary: e.button === 0 };
      pickAt(e);
    }
  }

  /**
   * Color Picker: sets the primary (left button) or secondary (right button) colour while
   * dragging; the colour goes to recents only on release (CLAUDE.md "Colors pad").
   */
  function pickAt(e: PointerEvent) {
    if (!picking) return;
    const { doc, primary } = picking;
    const average = tools.option<boolean>("color-picker", "average");
    const radius = average ? tools.option<number>("color-picker", "radius") : 1;
    const merged = tools.option<boolean>("color-picker", "sampleMerged");
    const [ix, iy] = screenToImage(doc.view, ...devicePoint(e));
    const p = doc.core.sample(ix, iy, radius, merged);
    if (!p) return;
    const c = { r: p[0]!, g: p[1]!, b: p[2]!, a: p[3]! / 255 };
    picking.color = c;
    palette.setColor(primary, c, false);
  }

  function onPointerMove(e: PointerEvent) {
    const doc = documents.active;
    if (!doc) return;
    if (pan) {
      const dpr = devicePixelRatio;
      doc.view = {
        ...doc.view,
        x: Math.round(pan.vx + (e.clientX - pan.x) * dpr),
        y: Math.round(pan.vy + (e.clientY - pan.y) * dpr),
      };
      return;
    }
    if (guideDrag.doc === doc && guideDrag.index !== undefined) {
      // A guide moved with the Move tool; dragged off the canvas it is removed.
      const r = canvasEl.getBoundingClientRect();
      const inside =
        e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.bottom;
      guideDrag.pos = inside
        ? guidePos(doc.view, guideDrag.horizontal, ...devicePoint(e))
        : undefined;
      requestDraw();
      return;
    }
    const dragging =
      moveTool.busy ||
      paintTools.busy ||
      selectTools.busy ||
      transformTools.busy ||
      textTool.busy ||
      picking !== undefined;
    if (!dragging) {
      const gi = guideAt(doc, doc.view, ...devicePoint(e));
      guideHover = gi === undefined ? "" : doc.guides[gi]!.horizontal ? "row" : "col";
    } else guideHover = "";
    if (textTool.busy) {
      textTool.move(e, screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    }
    if (transformTools.busy) {
      transformTools.move(e, screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    }
    if (moveTool.busy) {
      moveTool.move(...screenToImage(doc.view, ...devicePoint(e)));
      requestDraw();
    }
    if (PAINT_TOOLS.includes(tools.active) || paintTools.busy) {
      const events = paintTools.busy ? (e.getCoalescedEvents?.() ?? [e]) : [e];
      if (!events.length) events.push(e);
      paintTools.move(
        e,
        events.map((ev) => screenToImage(doc.view, ...devicePoint(ev))),
      );
      requestDraw();
    }
    if (picking?.doc === doc) pickAt(e);
    if (selectTools.busy) {
      selectTools.move(e, ...selectPoint(doc, e));
      requestDraw();
    }
    const [ix, iy] = screenToImage(doc.view, ...devicePoint(e));
    pointer.x = Math.floor(ix);
    pointer.y = Math.floor(iy);
  }

  function onPointerUp() {
    pan = undefined;
    if (guideDrag.doc && guideDrag.index !== undefined) {
      finishGuideDrag();
      requestDraw();
    }
    if (selectTools.busy) {
      selectTools.up();
      requestDraw();
    }
    if (picking) {
      if (picking.color) palette.setColor(picking.primary, picking.color);
      picking = undefined;
    }
    if (transformTools.busy) transformTools.up();
    if (textTool.busy && documents.active) {
      textTool.up(documents.active);
      requestDraw();
    }
    if (moveTool.busy) {
      moveTool.up();
      requestDraw();
    }
    if (paintTools.busy) {
      paintTools.up();
      requestDraw();
    }
  }

  function onWheel(e: WheelEvent) {
    const doc = documents.active;
    if (!doc) return;
    e.preventDefault();
    const dpr = devicePixelRatio;
    // Ctrl+wheel zooms, or the plain wheel with Settings > Canvas > "Mouse wheel zooms".
    if (!e.shiftKey && e.ctrlKey !== prefs.wheelZooms) {
      const [sx, sy] = devicePoint(e);
      zoomAt(doc, doc.view.zoom * Math.pow(2, -e.deltaY / 300), sx, sy);
    } else {
      const dx = e.shiftKey ? e.deltaY : e.deltaX;
      const dy = e.shiftKey ? 0 : e.deltaY;
      doc.view = { ...doc.view, x: doc.view.x - dx * dpr, y: doc.view.y - dy * dpr };
    }
  }

  onMount(() => {
    renderer = new Renderer(canvasEl);
    textTool.attach(textInput);
    overlay = overlayEl.getContext("2d")!;
    const ro = new ResizeObserver(() => {
      const dpr = devicePixelRatio;
      const r = canvasEl.getBoundingClientRect();
      canvasEl.width = Math.max(1, Math.round(r.width * dpr));
      canvasEl.height = Math.max(1, Math.round(r.height * dpr));
      overlayEl.width = canvasEl.width;
      overlayEl.height = canvasEl.height;
      viewport.width = canvasEl.width;
      viewport.height = canvasEl.height;
      // Resizing clears the canvas: draw now (before this frame is shown), not on the next
      // frame, or the canvas flashes black while the side panel is dragged.
      drawNow();
    });
    ro.observe(canvasEl);

    const offClosed = onDocumentClosed((doc) => {
      for (const img of gpu.get(doc.id)?.values() ?? []) img.dispose();
      masks.get(doc.id)?.dispose();
      masks.delete(doc.id);
      gpu.delete(doc.id);
      requestDraw();
    });

    const keys = (e: KeyboardEvent) => {
      // Space pans (not while typing text).
      if (e.code === "Space" && !isTextInput(e.target)) spaceDown = e.type === "keydown";
      if (e.key === "Alt" && paintTools.alt !== (e.type === "keydown")) {
        paintTools.alt = e.type === "keydown";
        requestDraw();
      }
      // Alt alone would move the focus to the window menu.
      if (e.key === "Alt" && ALT_PICK_TOOLS.includes(tools.active)) e.preventDefault();
      if (dialogs.isOpen) return;
      // Arrow keys of the Move tool and keys for a selection shape in progress (Enter,
      // Escape, Backspace, Shift, Ctrl) do not reach the global shortcuts.
      const doc = documents.active;
      const moved =
        e.type === "keydown" &&
        doc &&
        tools.active === "move" &&
        !isTextInput(e.target) &&
        moveTool.key(doc, e);
      if (
        moved ||
        (selectTools.busy && selectTools.key(e)) ||
        (transformTools.session?.doc === doc && transformTools.key(e))
      ) {
        e.preventDefault();
        e.stopPropagation();
        requestDraw();
      }
    };
    window.addEventListener("keydown", keys, true);
    window.addEventListener("keyup", keys, true);

    requestAnimationFrame(() => onfirstframe?.());

    return () => {
      ro.disconnect();
      offClosed();
      window.removeEventListener("keydown", keys, true);
      window.removeEventListener("keyup", keys, true);
      for (const images of gpu.values()) for (const img of images.values()) img.dispose();
    };
  });
</script>

<div class="area" class:rulers={guidePrefs.show} style:--ruler="{RULER}px">
  {#if guidePrefs.show}
    <div class="corner"></div>
    <div class="ruler-top">
      <Ruler horizontal canvas={() => canvasEl} onchange={requestDraw} />
    </div>
    <div class="ruler-left">
      <Ruler horizontal={false} canvas={() => canvasEl} onchange={requestDraw} />
    </div>
  {/if}
  <div
    class="viewport"
    class:drop-target={dragState.zone === "canvas"}
    data-drop-zone="canvas"
    data-drop-label={documents.active ? "Add as new layers" : "Open as new tabs"}
  >
    <canvas
      bind:this={canvasEl}
      class:picker={tools.active === "color-picker" ||
        SELECTION_TOOLS.includes(tools.active) ||
        PAINT_TOOLS.includes(tools.active)}
      class:mover={tools.active === "move"}
      class:guide-row={guideHover === "row"}
      class:guide-col={guideHover === "col"}
      class:transform={TRANSFORM_TOOLS.includes(tools.active)}
      class:texting={tools.active === "text"}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      onpointerleave={() => {
        pointer.x = pointer.y = undefined;
        paintTools.leave();
        requestDraw();
      }}
      onwheel={onWheel}
      ondblclick={() => {
        selectTools.dblclick();
        if (TRANSFORM_TOOLS.includes(tools.active)) transformTools.dblclick();
        requestDraw();
      }}
      oncontextmenu={(e) => e.preventDefault()}
    ></canvas>
    <canvas class="overlay" bind:this={overlayEl}></canvas>
    <!-- Keyboard input for the Text tool (IME and layouts work); kept at the caret. -->
    <textarea
      class="text-input"
      bind:this={textInput}
      style:left="{caretCss[0]}px"
      style:top="{caretCss[1]}px"
      tabindex="-1"
      aria-label="Text"
      autocomplete="off"
      spellcheck="false"
      onkeydown={(e) => {
        if (e.isComposing) return;
        if (textTool.key(e)) {
          e.preventDefault();
          e.stopPropagation();
          requestDraw();
        }
      }}
      oninput={(e) => {
        if ((e as unknown as InputEvent).isComposing) return;
        textTool.insert(textInput.value);
        textInput.value = "";
      }}
      oncompositionend={() => {
        textTool.insert(textInput.value);
        textInput.value = "";
      }}
      oncopy={(e) => {
        e.preventDefault();
        e.clipboardData?.setData("text/plain", textTool.selectedText());
      }}
      oncut={(e) => {
        e.preventDefault();
        e.clipboardData?.setData("text/plain", textTool.selectedText());
        textTool.deleteSelection();
      }}
      onpaste={(e) => {
        e.preventDefault();
        textTool.insert(e.clipboardData?.getData("text/plain") ?? "");
      }}></textarea>
    {#if textTool.edit && textTool.edit.doc === documents.active}
      <TextStyleEditor left={editorPos[0]!} top={editorPos[1]!} />
    {/if}
    {#if !documents.active}
      <div class="empty">
        <div>Open an image (Ctrl+O) or drop files here</div>
        {#if documents.busy}<div class="busy">{documents.busy}</div>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .area {
    flex: 1;
    display: grid;
    grid-template: "view" 1fr / 1fr;
    min-height: 0;
    min-width: 0;
  }
  .area.rulers {
    grid-template:
      "corner top" var(--ruler)
      "left view" 1fr / var(--ruler) 1fr;
  }
  .corner {
    grid-area: corner;
    background: var(--bg-header);
    border-right: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .ruler-top {
    grid-area: top;
    min-width: 0;
  }
  .ruler-left {
    grid-area: left;
    min-height: 0;
  }
  .viewport {
    grid-area: view;
    position: relative;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  canvas.overlay {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .text-input {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    border: 0;
    opacity: 0;
    resize: none;
    pointer-events: none;
  }
  canvas.texting {
    cursor: text;
  }
  canvas.transform {
    cursor: crosshair;
  }
  canvas.mover {
    cursor: move;
  }
  canvas.picker {
    cursor: crosshair;
  }
  /* After the tool cursors: over a guide every tool can drag it. */
  canvas.guide-row {
    cursor: row-resize;
  }
  canvas.guide-col {
    cursor: col-resize;
  }
  canvas {
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    justify-content: center;
    color: var(--fg-faint);
    font-size: 14px;
    pointer-events: none;
  }
</style>
