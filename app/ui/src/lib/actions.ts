// Action registry + keyboard shortcuts.
//
// Every command (menu item, header button, shortcut) is an action with a stable id.
// Shortcuts are matched on physical keys (KeyboardEvent.code), so they work the same with
// the Ukrainian or any other keyboard layout. Notation follows GIMP: "Ctrl+Shift+S", "2",
// "Shift+KP_5", "[".
import { dialogs } from "./dialogs.svelte";

export interface Action {
  id: string;
  label: string;
  icon?: string;
  /** Shortcuts; the first one is shown in menus and tooltips. */
  keys?: string[];
  run: () => unknown;
  enabled?: () => boolean;
}

const actions = new Map<string, Action>();
const byKey = new Map<string, string[]>();

export function defineActions(list: Action[]) {
  for (const a of list) {
    actions.set(a.id, a);
    for (const k of a.keys ?? []) {
      const ids = byKey.get(k) ?? [];
      ids.push(a.id);
      byKey.set(k, ids);
    }
  }
}

/** Whether an action is registered (menus skip unknown ones instead of failing). */
export function hasAction(id: string): boolean {
  return actions.has(id);
}

export function getAction(id: string): Action {
  const a = actions.get(id);
  if (!a) throw new Error(`unknown action: ${id}`);
  return a;
}

export function isEnabled(id: string): boolean {
  return getAction(id).enabled?.() ?? true;
}

export function runAction(id: string) {
  const a = getAction(id);
  if (a.enabled?.() ?? true) void a.run();
}

/** "Ctrl+Shift+S" for display, or "" when the action has no shortcut. */
export function shortcutOf(id: string): string {
  return (actions.get(id)?.keys?.[0] ?? "").replace(/KP_/g, "Num ");
}

const CODE_NAMES: Record<string, string> = {
  Equal: "=",
  Minus: "-",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  NumpadAdd: "KP_Add",
  NumpadSubtract: "KP_Subtract",
  NumpadMultiply: "KP_Multiply",
  NumpadDivide: "KP_Divide",
  NumpadDecimal: "KP_Decimal",
  NumpadEnter: "Enter",
};

/** Normalised key of a keyboard event, e.g. "Ctrl+Shift+S", or undefined for bare modifiers. */
export function eventKey(e: KeyboardEvent): string | undefined {
  const c = e.code;
  let key: string;
  if (/^Key[A-Z]$/.test(c)) key = c.slice(3);
  else if (/^Digit\d$/.test(c)) key = c.slice(5);
  else if (/^Numpad\d$/.test(c)) key = "KP_" + c.slice(6);
  else if (c in CODE_NAMES) key = CODE_NAMES[c]!;
  else if (/^(Shift|Control|Alt|Meta)(Left|Right)$/.test(c) || c === "") return undefined;
  else key = c; // Delete, Backspace, Escape, Tab, Enter, Space, F1..F12, Arrow*, Home...
  return (e.ctrlKey ? "Ctrl+" : "") + (e.shiftKey ? "Shift+" : "") + (e.altKey ? "Alt+" : "") + key;
}

export function isTextInput(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  if (t.isContentEditable || t instanceof HTMLTextAreaElement) return true;
  return (
    t instanceof HTMLInputElement && !["checkbox", "radio", "range", "button"].includes(t.type)
  );
}

/**
 * Global keydown handler. When several actions share a key (e.g. tools), the first enabled
 * one after the most recently run one is used, so repeated presses cycle through them.
 */
let lastRun: string | undefined;
export function handleKeydown(e: KeyboardEvent) {
  if (e.defaultPrevented || dialogs.isOpen || isTextInput(e.target)) return;
  const key = eventKey(e);
  if (!key) return;
  const ids = byKey.get(key)?.filter((id) => isEnabled(id));
  if (!ids?.length) return;
  e.preventDefault();
  const i = lastRun ? ids.indexOf(lastRun) : -1;
  const id = ids[(i + 1) % ids.length]!;
  lastRun = id;
  runAction(id);
}
