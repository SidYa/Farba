// Opens the "Choose Colors" dialog (components/color/ColorDialog.svelte), rendered by App.

export const colorDialog = $state<{ open: boolean; primary: boolean; serial: number }>({
  open: false,
  primary: true,
  serial: 0,
});

/** Opens the dialog with the primary or the secondary colour selected. */
export function openColorDialog(primary: boolean) {
  colorDialog.primary = primary;
  colorDialog.serial++;
  colorDialog.open = true;
}

export function closeColorDialog() {
  colorDialog.open = false;
}
