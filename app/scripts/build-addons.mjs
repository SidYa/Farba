// Builds the add-on packages (`pnpm addons`): core/target/addons/<file>.fba, a zip
// with addon.json, the add-on DLL (release build), the DLLs it preloads (from `libs`,
// default the release folder) and extra files (licences). The staging folders next to the
// packages show what went into each one. Package names start with "add-" only so they sort
// together on the releases page; the host does not care about the file name.
//
// One package serves Windows and Linux: an add-on whose addon.json has `platforms.linux`
// also carries that library. On Windows this script builds it in WSL first (the repository
// is copied to ~/farba there, see app/build-linux.sh --addons); without WSL the package is
// Windows-only (`platforms` dropped). On Linux the script only builds the Linux libraries
// into core/target/addons/linux, for the Windows run to pack.
import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const core = resolve(dirname(fileURLToPath(import.meta.url)), "../../core");
const heif = join(core, "target", "heif");
const linuxLibs = join(core, "target", "addons", "linux");
const onLinux = process.platform === "linux";
const ADDONS = [
  { crate: "farba-ai", file: "add-ai-backrem", dir: join(core, "farba-ai") },
  {
    crate: "farba-heif",
    file: "add-heic",
    dir: join(core, "farba-heif"),
    // Built once by `pnpm heif-libs`.
    libs: join(heif, "dist", "bin"),
    files: {
      "THIRD-PARTY.txt": join(core, "farba-heif", "THIRD-PARTY.txt"),
      "libheif-COPYING.txt": join(heif, "src", "libheif", "COPYING"),
      "libde265-COPYING.txt": join(heif, "src", "libde265", "COPYING"),
    },
  },
];
for (const a of ADDONS) a.manifest = JSON.parse(readFileSync(join(a.dir, "addon.json"), "utf8"));

function cargoBuild(crate, env = process.env) {
  execFileSync("cargo", ["build", "--release", "-p", crate], { cwd: core, stdio: "inherit", env });
}

if (onLinux) {
  rmSync(linuxLibs, { recursive: true, force: true });
  mkdirSync(linuxLibs, { recursive: true });
  for (const a of ADDONS) {
    const p = a.manifest.platforms?.linux;
    if (!p) continue;
    // ONNX Runtime must not link the system libstdc++ (an older distribution's may be too
    // old); farba-ai's build.rs links it statically instead.
    cargoBuild(a.crate, { ...process.env, ORT_CXX_STDLIB: "" });
    for (const f of [p.library, ...(p.preload ?? [])])
      copyFileSync(join(core, "target", "release", f), join(linuxLibs, f));
  }
  console.log(`${linuxLibs}: ${readdirSync(linuxLibs).join(", ")}`);
  process.exit(0);
}

for (const a of ADDONS)
  if (a.libs && !existsSync(a.libs)) {
    console.error(`${a.crate}: ${a.libs} is missing; run \`pnpm heif-libs\` first`);
    process.exit(1);
  }

/** Builds the Linux libraries in WSL into core/target/addons/linux; false without WSL. */
function buildLinuxLibs() {
  try {
    execFileSync("wsl", ["--status"], { stdio: "ignore" });
  } catch {
    return false;
  }
  // D:\GIT\Soft\Farba -> /mnt/d/GIT/Soft/Farba
  const wsl = (p) =>
    p.replace(/^([A-Za-z]):/, (_, d) => `/mnt/${d.toLowerCase()}`).replaceAll("\\", "/");
  const root = wsl(resolve(core, ".."));
  // A copy inside the Linux file system: /mnt is slow and node_modules there holds win32
  // binaries.
  const script = [
    `rsync -a --delete --exclude target --exclude node_modules --exclude .git`,
    `--exclude app/ui/src/lib/wasm/pkg --exclude app/ui/dist '${root}/' ~/farba/`,
    `&& bash ~/farba/app/build-linux.sh --addons`,
    `&& rm -rf '${wsl(linuxLibs)}' && mkdir -p '${wsl(linuxLibs)}'`,
    `&& cp ~/farba/core/target/addons/linux/* '${wsl(linuxLibs)}/'`,
  ].join(" ");
  execFileSync("wsl", ["bash", "-lc", script], { stdio: "inherit" });
  return true;
}

const withLinux = ADDONS.some((a) => a.manifest.platforms?.linux) && buildLinuxLibs();
if (!withLinux) console.warn("WSL is not available: the packages are built for Windows only.");

for (const a of ADDONS) {
  cargoBuild(a.crate);
  const manifest = a.manifest;
  const stage = join(core, "target", "addons", manifest.id);
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(stage, { recursive: true });
  const release = join(core, "target", "release");
  copyFileSync(join(release, manifest.library), join(stage, manifest.library));
  for (const f of manifest.preload ?? []) copyFileSync(join(a.libs ?? release, f), join(stage, f));
  const files = [manifest.library, ...(manifest.preload ?? [])];
  const linux = manifest.platforms?.linux;
  if (linux && withLinux) {
    for (const f of [linux.library, ...(linux.preload ?? [])]) {
      copyFileSync(join(linuxLibs, f), join(stage, f));
      files.push(f);
    }
  } else {
    delete manifest.platforms;
  }
  writeFileSync(join(stage, "addon.json"), JSON.stringify(manifest, null, 2) + "\n");
  // Every package carries Farba's licence (the add-on DLLs contain farba-core, GPL).
  const extra = Object.entries({
    "LICENSE.txt": join(core, "..", "LICENSE"),
    "THIRD-PARTY-NOTICES.md": join(core, "..", "THIRD-PARTY-NOTICES.md"),
    ...a.files,
  });
  for (const [name, from] of extra) copyFileSync(from, join(stage, basename(name)));
  files.unshift("addon.json");
  files.push(...extra.map(([name]) => name));
  const out = join(core, "target", "addons", `${a.file}.fba`);
  rmSync(out, { force: true });
  // Windows' own bsdtar writes zip archives (a GNU tar on PATH, e.g. Git's, cannot).
  const tar = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
  execFileSync(tar, ["--format=zip", "-cf", out, "-C", stage, ...files], { stdio: "inherit" });
  console.log(`${out}${linux && withLinux ? " (Windows + Linux)" : ""}`);
}
