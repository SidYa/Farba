// Add-ons (see app/src-tauri/src/addons.rs): optional features installed from
// `.fba` packages in the Addons Manager. Nothing of an add-on shows before it is
// installed; afterwards its actions appear where its manifest says (header bar, Effects
// menu), and the Addons Manager shows the settings it describes.
//
// An action of kind "alpha-mask" gets the active layer's pixels and returns one byte per
// pixel; the layer's alpha is multiplied by it as one undo step.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import AddonProgressDialog from "../components/dialogs/AddonProgressDialog.svelte";
import { defineActions } from "./actions";
import { choose, openDialog } from "./dialogs.svelte";
import { documents, errorText, type Doc } from "./documents.svelte";
import { invokeShared, sendShared } from "./ipc/shared";
import { notify } from "./notify.svelte";
import { loadSetting, saveSetting } from "./settings";
import { confirmRasterize } from "./textguard";
import { wasmBytes } from "./wasm/core";

export interface AddonAction {
  id: string;
  label: string;
  icon?: string;
  places: string[];
  kind: string;
  title?: string;
}

export interface Addon {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  license: string;
  actions: AddonAction[];
  /** "remove" / "update": finished at the next start. */
  pending: string | null;
}

/** A setting described by an add-on (rendered by the Addons Manager). */
export type AddonSetting =
  | {
      type: "select";
      key: string;
      label: string;
      value: string;
      choices: { value: string; label: string }[];
    }
  | { type: "switch"; key: string; label: string; value: boolean; on?: string; off?: string }
  | {
      type: "storage";
      label: string;
      bytes: number;
      folder: string;
      action: string;
      confirm?: string;
    }
  | { type: "note"; text: string };

class Addons {
  list = $state.raw<Addon[]>([]);
  /** An add-on action is running (its buttons are disabled, the progress dialog is open). */
  running = $state<{ title: string; text: string; fraction: number; addon: string } | undefined>();
  /** Add-on actions the user took off the header bar (Addons Manager); keyed by action id. */
  headerHidden = $state<Record<string, boolean>>(loadSetting("addon-header-hidden", {}));

  setOnHeader(actionId: string, on: boolean) {
    this.headerHidden = { ...this.headerHidden, [actionId]: !on };
    saveSetting("addon-header-hidden", this.headerHidden);
  }

  /** Usable add-ons (not being removed). */
  get active(): Addon[] {
    return this.list.filter((a) => a.pending !== "remove");
  }

  /** Action ids of the active add-ons for a place ("header", "effects"). */
  actionsFor(place: string): string[] {
    return this.active.flatMap((a) =>
      a.actions
        .filter((x) => x.places.includes(place))
        .map((x) => actionId(a, x))
        // The header bar shows an action unless the user turned it off.
        .filter((id) => place !== "header" || !this.headerHidden[id]),
    );
  }

  async refresh() {
    let list: Addon[] = [];
    try {
      list = await invoke<Addon[]>("addons_list");
    } catch {
      // No add-ons (or the folder cannot be read).
    }
    // Define the actions before the list changes: the header shows them at once.
    for (const a of list)
      defineActions(
        a.actions.map((x) => ({
          id: actionId(a, x),
          label: x.label,
          icon: x.icon,
          enabled: () => !!documents.active && !this.running,
          run: () => documents.active && runAction(documents.active, a, x),
        })),
      );
    this.list = list;
  }
}

export const addons = new Addons();

export function actionId(a: Addon, x: AddonAction): string {
  return `addon:${a.id}:${x.id}`;
}

/** The setting values the user chose for an add-on (the add-on supplies defaults). */
export function addonValues(id: string): Record<string, unknown> {
  return loadSetting<Record<string, unknown>>(`addon.${id}`, {});
}

export function setAddonValue(id: string, key: string, value: unknown) {
  saveSetting(`addon.${id}`, { ...addonValues(id), [key]: value });
}

export async function addonSettings(id: string): Promise<AddonSetting[]> {
  const r = await invoke<{ items: AddonSetting[] }>("addon_call", {
    id,
    request: { method: "settings", values: addonValues(id) },
  });
  return r.items ?? [];
}

export async function addonSettingsAction(id: string, action: string): Promise<string> {
  const r = await invoke<{ message?: string }>("addon_call", {
    id,
    request: { method: "settings_action", action, values: addonValues(id) },
  });
  return r.message ?? "";
}

export function cancelAddon() {
  if (addons.running) void invoke("addon_cancel", { id: addons.running.addon });
}

export function formatBytes(n: number): string {
  if (n >= 1 << 30) return `${(n / (1 << 30)).toFixed(1)} GB`;
  if (n >= 1 << 20) return `${Math.round(n / (1 << 20))} MB`;
  if (n >= 1 << 10) return `${Math.round(n / (1 << 10))} KB`;
  return `${n} B`;
}

/** Runs an add-on action on the active layer of `doc`. */
async function runAction(doc: Doc, addon: Addon, action: AddonAction) {
  if (addons.running || !(await confirmRasterize(doc))) return;
  const layer = doc.activeLayer;
  if (!layer) return;
  const { id, width, height } = layer;
  addons.running = {
    title: action.title ?? action.label,
    text: "Preparing…",
    fraction: -1,
    addon: addon.id,
  };
  void openDialog(AddonProgressDialog, {});
  const unlisten = await listen<{ addon: string; text?: string; fraction?: number }>(
    "addon-progress",
    (e) => {
      if (addons.running && e.payload.addon === addon.id)
        addons.running = {
          ...addons.running,
          text: e.payload.text ?? addons.running.text,
          fraction: e.payload.fraction ?? -1,
        };
    },
  );
  try {
    const len = width * height * 4;
    const ptr = doc.core.layer_pixels(id);
    try {
      await sendShared(len, (dst) => dst.set(wasmBytes(ptr, len)), "addon_put_pixels", {
        width,
        height,
      });
    } finally {
      doc.core.free_pixels();
    }
    const r = await invokeShared<{ reply: { status?: string } }>("addon_run", {
      addon: addon.id,
      request: { action: action.id, values: addonValues(addon.id) },
    });
    try {
      // The image or the layer may have been closed / deleted meanwhile.
      if (!documents.list.includes(doc) || !doc.layer(id)) return;
      if (action.kind === "alpha-mask") {
        const ok = doc.run((c) => {
          c.multiply_alpha(id, width, height, new Uint8Array(r.buffer), action.label);
          return true;
        });
        if (ok && r.header.reply?.status) notify(r.header.reply.status);
      }
    } finally {
      r.release();
    }
  } catch (e) {
    const msg = errorText(e);
    if (msg !== "cancelled") {
      addons.running = undefined;
      await choose({
        title: `${action.title ?? action.label} failed`,
        message: msg,
        buttons: [{ label: "OK", value: "ok", primary: true }],
      });
    }
  } finally {
    unlisten();
    addons.running = undefined;
  }
}
