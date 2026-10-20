// Page ranges as typed in GIMP's PDF page selector: "1,3,5-7,9-". Pages are 1-based in the
// text and 0-based in the arrays. Invalid parts are ignored; "-4" starts at the first page
// and "9-" runs to the last one.

/** A leading integer of `s` (like C's `%i` without octal / hex), or undefined. */
function leadingInt(s: string): number | undefined {
  const m = /^[+-]?\d+/.exec(s);
  return m ? Number(m[0]) : undefined;
}

/** The pages described by `text`, sorted, without duplicates. */
export function parsePageRange(text: string, count: number): number[] {
  const pages = new Set<number>();
  for (const raw of text.split(",")) {
    const part = raw.trim();
    const dash = part.indexOf("-", 1);
    if (dash > 0 || part.startsWith("-")) {
      const at = part.startsWith("-") ? 0 : dash;
      const from = part.slice(0, at).trim();
      const to = part.slice(at + 1).trim();
      const a = from === "" ? 1 : leadingInt(from);
      const b = to === "" ? count : leadingInt(to);
      if (a === undefined || b === undefined || a < 1 || b < a || a > count) continue;
      for (let p = a; p <= Math.min(b, count); p++) pages.add(p - 1);
    } else {
      const p = leadingInt(part);
      if (p !== undefined && p >= 1 && p <= count) pages.add(p - 1);
    }
  }
  return [...pages].sort((x, y) => x - y);
}

/** Pages as a range text, runs merged: [0, 1, 2, 4] -> "1-3,5". */
export function formatPageRange(pages: Iterable<number>): string {
  const sorted = [...new Set(pages)].sort((a, b) => a - b);
  const parts: string[] = [];
  for (let i = 0; i < sorted.length;) {
    let j = i;
    while (j + 1 < sorted.length && sorted[j + 1] === sorted[j]! + 1) j++;
    parts.push(i === j ? `${sorted[i]! + 1}` : `${sorted[i]! + 1}-${sorted[j]! + 1}`);
    i = j + 1;
  }
  return parts.join(",");
}
