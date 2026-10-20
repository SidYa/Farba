// Modal dialogs in the app's own style. `openDialog` mounts a dialog component (rendered by
// components/dialogs/DialogHost.svelte) and resolves with the value it closes with, or
// undefined when cancelled. While a dialog is open, global shortcuts are off.
import type { Component } from "svelte";
import ConfirmDialog from "../components/dialogs/ConfirmDialog.svelte";

/** Every dialog component gets `close`; calling it without a value means "cancel". */
export interface DialogProps<R> {
  close: (result?: R) => void;
}

interface OpenDialog {
  id: number;
  component: Component<any>;
  props: Record<string, unknown>;
  close: (result?: unknown) => void;
}

let nextId = 1;

class Dialogs {
  stack = $state.raw<OpenDialog[]>([]);

  get isOpen(): boolean {
    return this.stack.length > 0;
  }
}

export const dialogs = new Dialogs();

type DialogResult<P> = P extends DialogProps<infer R> ? R : never;

export function openDialog<P extends DialogProps<any>>(
  component: Component<P>,
  props: Omit<P, "close">,
): Promise<DialogResult<P> | undefined> {
  return new Promise((resolve) => {
    const d: OpenDialog = {
      id: nextId++,
      component,
      props,
      close: (result) => {
        dialogs.stack = dialogs.stack.filter((x) => x !== d);
        resolve(result as DialogResult<P> | undefined);
      },
    };
    dialogs.stack = [...dialogs.stack, d];
  });
}

export interface ChoiceButton {
  label: string;
  value: string;
  /** The default button (Enter). */
  primary?: boolean;
  danger?: boolean;
}

export interface ConfirmOptions {
  title: string;
  message: string;
  buttons: ChoiceButton[];
  /** Optional check box label, e.g. "Don't ask again". */
  checkbox?: string;
}

/** Asks a question; resolves with the chosen button value, or undefined (Escape/Cancel). */
export async function choose(
  opts: ConfirmOptions,
): Promise<{ value: string; checked: boolean } | undefined> {
  return openDialog(ConfirmDialog, opts);
}

/** A simple OK / Cancel question. */
export async function confirmAction(title: string, message: string, ok: string) {
  const r = await choose({
    title,
    message,
    buttons: [
      { label: "Cancel", value: "cancel" },
      { label: ok, value: "ok", primary: true },
    ],
  });
  return r?.value === "ok";
}
