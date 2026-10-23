// Builds ui/public/licenses.json for Help > About > Licenses: every third-party component
// that ships inside Farba (or one of its add-ons), with its licence and the licence text
// found in its sources.
//
//   origin   projects Farba's code is ported from or modelled on
//   bundled  native libraries shipped next to the app or inside an add-on
//   asset    icons and other artwork
//   rust     crates compiled into the app, the WASM module and the add-ons for the build
//            target (CARGO_BUILD_TARGET, else this OS: Windows MSVC / Linux GNU)
//   js       npm packages bundled into the UI (production deps + the Svelte runtime)
//
// Run: pnpm licenses (also runs as part of `pnpm build`)
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(appDir, "..");
const core = join(repoRoot, "core");
const out = join(appDir, "ui", "public", "licenses.json");

const texts = {};
function addText(text) {
  const norm = text.replace(/\r\n/g, "\n").trim();
  if (!norm) return null;
  const id = createHash("sha1").update(norm).digest("hex").slice(0, 12);
  texts[id] = norm;
  return id;
}

const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;
function licenseTexts(dir) {
  if (!dir || !existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => {
      try {
        return addText(readFileSync(join(dir, f), "utf8"));
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

/** A licence file that may be missing (e.g. sources fetched only by `pnpm heif-libs`). */
function fileText(path) {
  return existsSync(path) ? [addText(readFileSync(path, "utf8"))] : [];
}

const packages = [];

// --- Rust ---------------------------------------------------------------
function rustCrates() {
  const home =
    process.env.CARGO_HOME ?? join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".cargo");
  const cargo = existsSync(join(home, "bin", "cargo.exe")) ? join(home, "bin", "cargo.exe") : "cargo";
  const triple =
    process.env.CARGO_BUILD_TARGET ??
    (process.platform === "win32" ? "x86_64-pc-windows-msvc" : "x86_64-unknown-linux-gnu");
  const raw = execFileSync(
    cargo,
    ["metadata", "--format-version", "1", "--filter-platform", triple],
    { cwd: core, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  const meta = JSON.parse(raw);
  const members = new Set(meta.workspace_members);
  // Walk the resolve graph from the workspace, normal deps only (build-time and dev-only
  // crates are not shipped).
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const seen = new Set();
  const stack = [...members];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
  }
  for (const p of meta.packages) {
    if (!seen.has(p.id) || members.has(p.id)) continue;
    packages.push({
      kind: "rust",
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? "see licence file" : "unknown"),
      repository: p.repository ?? p.homepage ?? `https://crates.io/crates/${p.name}`,
      texts: licenseTexts(dirname(p.manifest_path)),
    });
  }
}

// --- JavaScript ---------------------------------------------------------
function resolveDir(name, from) {
  let dir = from;
  while (true) {
    const cand = join(dir, "node_modules", name);
    if (existsSync(join(cand, "package.json"))) return cand;
    const up = dirname(dir);
    if (up === dir) return null;
    dir = up;
  }
}

function jsPackages() {
  const appPkg = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
  // Production deps + the Svelte runtime, which the compiler inlines into the bundle.
  const roots = [...Object.keys(appPkg.dependencies ?? {}), "svelte"];
  const seen = new Set();
  const visit = (name, from, depth) => {
    const dir = resolveDir(name, from);
    if (!dir) return;
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const key = `${pkg.name}@${pkg.version}`;
    if (seen.has(key)) return;
    seen.add(key);
    const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
    packages.push({
      kind: "js",
      name: pkg.name,
      version: pkg.version,
      license: typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? "unknown"),
      repository: (repo ?? pkg.homepage ?? `https://www.npmjs.com/package/${pkg.name}`)
        .replace(/^git\+/, "")
        .replace(/\.git$/, ""),
      texts: licenseTexts(dir),
    });
    // Svelte's own deps are compiler-only; the runtime has none.
    if (name === "svelte" || depth > 20) return;
    for (const dep of Object.keys(pkg.dependencies ?? {})) visit(dep, dir, depth + 1);
  };
  for (const r of roots) visit(r, appDir, 0);
}

// --- Origins, native libraries and artwork (not discoverable from manifests) ---
const PINTA_MIT = addText(`The MIT License

Copyright (c) 2010 Jonathan Pobst <monkey@jpobst.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.`);
const PAINT_NET = addText(`Paint.NET 3.36 is licensed under the MIT License (the same text as Pinta's).

Copyright (C) 2007 Rick Brewster, Chris Crosetto, Tom Jackson, Michael Kelsey,
Brandon Ortiz, Craig Taylor, Chris Trevino, and Luke Walker.
Portions Copyright (C) 2007 Microsoft Corporation.`);
const GPL3 = addText(readFileSync(join(repoRoot, "LICENSE"), "utf8"));
const DIRECTML = addText(`DirectML.dll is redistributed under the Microsoft DirectML licence (the
"MICROSOFT SOFTWARE LICENSE TERMS" of the Microsoft.AI.DirectML package):
https://www.nuget.org/packages/Microsoft.AI.DirectML

Farba's licence grants an additional permission (GPL-3.0 section 7) to convey the AI
add-on combined with DirectML; see THIRD-PARTY-NOTICES.md.`);
const heifSrc = join(core, "target", "heif", "src");
const symbolsDir = resolveDir("@material-symbols/svg-500", appDir);

const manual = [
  {
    kind: "origin",
    name: "Pinta",
    version: "",
    license: "MIT",
    repository: "https://github.com/PintaProject/Pinta",
    note: "The colour picker dialog, the default palette and the Colors pad are ported from Pinta.",
    texts: [PINTA_MIT],
  },
  {
    kind: "origin",
    name: "Paint.NET",
    version: "3.36",
    license: "MIT",
    repository: "https://www.getpaint.net",
    note: "Parts of Pinta, and so of the code Farba ports from it, derive from Paint.NET 3.36.",
    texts: [PAINT_NET],
  },
  {
    kind: "origin",
    name: "GIMP",
    version: "",
    license: "GPL-3.0-or-later",
    repository: "https://gitlab.gnome.org/GNOME/gimp",
    note: "GIMP's behaviour is the reference for most tools. No GIMP code is ported at the time of writing.",
    texts: [GPL3],
  },
  {
    kind: "bundled",
    name: "ONNX Runtime",
    version: "",
    license: "MIT",
    repository: "https://github.com/microsoft/onnxruntime",
    note: "Inference engine of the AI background removal add-on.",
    texts: [],
  },
  {
    kind: "bundled",
    name: "DirectML",
    version: "",
    license: "Microsoft DirectML licence",
    repository: "https://github.com/microsoft/DirectML",
    note: "GPU inference of the AI add-on on Windows (DirectML.dll).",
    texts: [DIRECTML],
  },
  {
    kind: "bundled",
    name: "libheif",
    version: "1.23.6",
    license: "LGPL-3.0",
    repository: "https://github.com/strukturag/libheif",
    note: "HEIF container decoding in the HEIC add-on (heif.dll, unmodified; replaceable with your own build).",
    texts: fileText(join(heifSrc, "libheif", "COPYING")),
  },
  {
    kind: "bundled",
    name: "libde265",
    version: "1.1.3",
    license: "LGPL-3.0",
    repository: "https://github.com/strukturag/libde265",
    note: "HEVC decoding in the HEIC add-on (libde265.dll, unmodified; replaceable with your own build).",
    texts: fileText(join(heifSrc, "libde265", "COPYING")),
  },
  {
    kind: "bundled",
    name: "PDFium",
    version: "chromium/7881",
    license: "BSD-3-Clause AND Apache-2.0",
    repository: "https://github.com/bblanchon/pdfium-binaries",
    note: "PDF import in the Linux build (libpdfium.so, built by pdfium-binaries, MIT).",
    texts: [],
  },
  {
    kind: "asset",
    name: "Material Symbols",
    version: "",
    license: "Apache-2.0",
    repository: "https://github.com/google/material-design-icons",
    note: "Every icon in the app (Material Symbols Outlined, weight 500).",
    texts: licenseTexts(symbolsDir),
  },
];

try {
  rustCrates();
} catch (e) {
  console.warn("[licenses] cargo metadata failed:", e.message.split("\n")[0]);
}
jsPackages();

const order = { origin: 0, bundled: 1, asset: 2, rust: 3, js: 4 };
const all = [...manual, ...packages].sort(
  (a, b) => order[a.kind] - order[b.kind] || a.name.localeCompare(b.name),
);
const missing = all.filter((p) => p.texts.length === 0).map((p) => `${p.name}@${p.version}`);
// No licence file in the sources: point at the canonical SPDX text.
for (const p of all) {
  if (p.texts.length) continue;
  const ids = String(p.license)
    .split(/\s+(?:OR|AND|WITH)\s+|[()/]/)
    .map((x) => x.trim())
    .filter(Boolean);
  const links = ids.map((id) => `${id}: https://spdx.org/licenses/${id}.html`).join("\n");
  p.texts = [addText(`Licensed under ${p.license}.\n\n${links}`)];
}
writeFileSync(out, JSON.stringify({ generated: new Date().toISOString(), packages: all, texts }));
const count = (kind) => packages.filter((p) => p.kind === kind).length;
console.log(
  `[licenses] ${all.length} components (${count("rust")} rust, ${count("js")} js), ${Object.keys(texts).length} distinct texts -> ${out}`,
);
if (missing.length)
  console.log(
    `[licenses] no licence file found for ${missing.length}: ${missing.slice(0, 12).join(", ")}${missing.length > 12 ? ", …" : ""}`,
  );
