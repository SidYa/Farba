// Menu model shared by the header-bar menus and context menus.

export type MenuItem =
  | { action: string; checked?: () => boolean }
  | { separator: true }
  | { label: string; items: MenuItem[] };

/** Which header menu is open (only one at a time; hovering another switches to it). */
export const menuState = $state<{ open: string | undefined }>({ open: undefined });
