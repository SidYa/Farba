// Files that need a Windows codec extension (HEIC: HEIF + HEVC; camera RAW: Raw Image
// Extension). Farba ships no codecs for them; when one is missing the backend says
// `codec-missing:<kind>` and this offers the extension's Microsoft Store page instead of a
// cryptic error. Everything else in Farba works without them. An installed HEIC add-on
// (optional, never suggested here) opens HEIC first, so then only saving can miss a codec.
import { invoke } from "@tauri-apps/api/core";
import { choose } from "./dialogs.svelte";
import { baseName, errorText } from "./documents.svelte";

/** Camera RAW file extensions (must match farba-io `wic::RAW_EXTENSIONS`). */
export const RAW_EXTENSIONS = [
  "3fr",
  "ari",
  "arw",
  "bay",
  "cap",
  "cr2",
  "cr3",
  "crw",
  "dcr",
  "dcs",
  "dng",
  "drf",
  "eip",
  "erf",
  "fff",
  "iiq",
  "k25",
  "kdc",
  "mef",
  "mos",
  "mrw",
  "nef",
  "nrw",
  "orf",
  "ori",
  "pef",
  "ptx",
  "raf",
  "raw",
  "rw2",
  "rwl",
  "sr2",
  "srf",
  "srw",
  "x3f",
];

const STORE = {
  heif: "9PMMSR1CGPWG",
  hevc: "9NMZLZ57R3T7",
  raw: "9NCTDW2W1BH8",
};

/**
 * Explains a missing codec for `path` and offers the Store pages. Returns false when `e` is
 * another error (the caller reports it).
 */
export async function offerCodec(path: string, e: unknown, saving = false): Promise<boolean> {
  const m = /codec-missing:(heif|raw)/.exec(errorText(e));
  if (!m) return false;
  const heif = m[1] === "heif";
  const name = baseName(path);
  const r = await choose({
    title: "Windows codec needed",
    message: heif
      ? saving
        ? `Saving “${name}” as HEIC needs two Windows extensions from the Microsoft Store: “HEIF Image Extensions” (free) and “HEVC Video Extensions” (often preinstalled, otherwise about $1). Install them and save again, or save in another format.`
        : `“${name}” is a HEIC / HEIF image. Windows opens it with two extensions from the Microsoft Store: “HEIF Image Extensions” (free) and “HEVC Video Extensions” (often preinstalled, otherwise about $1). Install them, then open the file again.`
      : `“${name}” is a camera RAW image. Windows opens it with the free “Raw Image Extension” from the Microsoft Store. Install it, then open the file again.`,
    buttons: heif
      ? [
          { label: "Close", value: "close" },
          { label: "HEIF Extensions", value: STORE.heif },
          { label: "HEVC Extensions", value: STORE.hevc, primary: true },
        ]
      : [
          { label: "Close", value: "close" },
          { label: "Open Microsoft Store", value: STORE.raw, primary: true },
        ],
  });
  if (r && r.value !== "close") void invoke("open_store", { product: r.value });
  return true;
}
