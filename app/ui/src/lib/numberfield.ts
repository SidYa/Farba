// Number fields: `<input use:numberField={{ value, oninput, ... }} />`.
//
// A plain `<input type="number" value={x}>` whose handler changes `x` (or a field derived
// from it) gets its text rewritten while the user types: typing "10" into Scale Image's
// percent field became "0.98" after the "1" and then "0.980". Here the text belongs to the
// user while they edit; it is reformatted from the value only when the editing ends
// (Enter, focus leaves, Escape) or when the value changes while the field is not edited.
// Commas are accepted as the decimal separator; ArrowUp / ArrowDown step (Shift: x10).

export interface NumberFieldOptions {
  value: number;
  min?: number;
  max?: number;
  /** Arrow key step (default 1). */
  step?: number;
  /** Decimals shown (default: those of `step`). */
  decimals?: number;
  /** Every valid number while typing, not clamped (live fields). */
  oninput?: (v: number) => void;
  /** The committed value, clamped to min / max (Enter, focus leaves, arrow keys). */
  onchange?: (v: number) => void;
}

function decimalsOf(step: number): number {
  const s = String(step);
  const dot = s.indexOf(".");
  return dot < 0 ? 0 : s.length - dot - 1;
}

/** The number in `text`, or NaN for an empty or partial one ("", "-", "1e"). */
export function parseNumber(text: string): number {
  const s = text.trim().replace(",", ".");
  return /^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(s) ? Number(s) : NaN;
}

export function numberField(el: HTMLInputElement, options: NumberFieldOptions) {
  let o = options;
  let editing = false;
  el.type = "text";
  el.inputMode = "decimal";
  el.autocomplete = "off";
  el.spellcheck = false;

  const clamp = (v: number) => Math.min(o.max ?? Infinity, Math.max(o.min ?? -Infinity, v));
  const show = () => {
    const f = 10 ** (o.decimals ?? decimalsOf(o.step ?? 1));
    el.value = Number.isFinite(o.value) ? String(Math.round(o.value * f) / f) : "";
  };
  const emit = (v: number) => {
    o.oninput?.(v);
    o.onchange?.(v);
  };
  const commit = () => {
    if (!editing) return;
    editing = false;
    const v = parseNumber(el.value);
    if (Number.isFinite(v)) emit(clamp(v));
    show();
  };

  const onInput = () => {
    editing = true;
    const v = parseNumber(el.value);
    if (Number.isFinite(v)) o.oninput?.(v);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter") commit();
    else if (e.key === "Escape") {
      editing = false;
      show();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const typed = parseNumber(el.value);
      const step = (o.step ?? 1) * (e.shiftKey ? 10 : 1) * (e.key === "ArrowUp" ? 1 : -1);
      editing = false;
      emit(clamp((Number.isFinite(typed) ? typed : o.value) + step));
      show();
    }
  };

  show();
  el.addEventListener("input", onInput);
  el.addEventListener("keydown", onKey);
  el.addEventListener("blur", commit);
  return {
    update(n: NumberFieldOptions) {
      o = n;
      if (!editing) show();
    },
    destroy() {
      // A popover closed by a click outside goes away without a blur.
      commit();
      el.removeEventListener("input", onInput);
      el.removeEventListener("keydown", onKey);
      el.removeEventListener("blur", commit);
    },
  };
}
