// Builds the open-source HEIF decoder DLLs for the HEIC add-on (`pnpm heif-libs`, once):
// libde265 (HEVC decoder) and libheif (HEIF container), both LGPL, as shared libraries with
// MSVC, decoding only (no encoders, no other codecs). Sources are cloned at fixed tags into
// core/target/heif/src; the DLLs end up in core/target/heif/dist/bin, where `pnpm addons`
// takes them. Needs git and the CMake that comes with the VS Build Tools.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const LIBDE265 = "v1.1.3";
const LIBHEIF = "v1.23.6";

const core = resolve(dirname(fileURLToPath(import.meta.url)), "../../core");
const root = join(core, "target", "heif");
const src = join(root, "src");
const build = join(root, "build");
const dist = join(root, "dist");

function findCmake() {
  const vs = ["C:\\Program Files (x86)\\Microsoft Visual Studio", "C:\\Program Files\\Microsoft Visual Studio"];
  for (const base of vs)
    for (const ver of ["18", "2026", "2022"])
      for (const ed of ["BuildTools", "Community", "Professional", "Enterprise"]) {
        const p = join(base, ver, ed, "Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe");
        if (existsSync(p)) return p;
      }
  return "cmake";
}

const cmake = findCmake();
const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: "inherit" });

function clone(name, tag) {
  const dir = join(src, name);
  if (!existsSync(dir))
    run("git", ["clone", "--depth", "1", "--branch", tag, `https://github.com/strukturag/${name}.git`, dir]);
  return dir;
}

function cmakeBuild(name, dir, options) {
  const b = join(build, name);
  mkdirSync(b, { recursive: true });
  run(cmake, [
    "-S", dir, "-B", b, "-A", "x64",
    `-DCMAKE_INSTALL_PREFIX=${dist}`,
    `-DCMAKE_PREFIX_PATH=${dist}`,
    "-DBUILD_SHARED_LIBS=ON",
    // The C++ runtime linked in: the DLLs must work on a PC without the VC++ redistributable.
    "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW",
    "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
    "-DBUILD_TESTING=OFF",
    ...options,
  ]);
  run(cmake, ["--build", b, "--config", "Release", "--parallel"]);
  run(cmake, ["--install", b, "--config", "Release"]);
}

rmSync(build, { recursive: true, force: true });
rmSync(dist, { recursive: true, force: true });
mkdirSync(src, { recursive: true });
cmakeBuild("libde265", clone("libde265", LIBDE265), [
  "-DENABLE_SDL=OFF",
  "-DENABLE_DECODER=OFF",
  "-DENABLE_ENCODER=OFF",
]);
cmakeBuild("libheif", clone("libheif", LIBHEIF), [
  "-DWITH_LIBDE265=ON",
  "-DWITH_X265=OFF",
  "-DWITH_KVAZAAR=OFF",
  "-DWITH_UVG266=OFF",
  "-DWITH_VVDEC=OFF",
  "-DWITH_VVENC=OFF",
  "-DWITH_AOM_DECODER=OFF",
  "-DWITH_AOM_ENCODER=OFF",
  "-DWITH_DAV1D=OFF",
  "-DWITH_RAV1E=OFF",
  "-DWITH_SvtEnc=OFF",
  "-DWITH_JPEG_DECODER=OFF",
  "-DWITH_JPEG_ENCODER=OFF",
  "-DWITH_OpenJPEG_DECODER=OFF",
  "-DWITH_OpenJPEG_ENCODER=OFF",
  "-DWITH_OPENJPH_DECODER=OFF",
  "-DWITH_OPENJPH_ENCODER=OFF",
  "-DWITH_FFMPEG_DECODER=OFF",
  "-DWITH_OpenH264_DECODER=OFF",
  "-DWITH_UNCOMPRESSED_CODEC=OFF",
  "-DWITH_HEADER_COMPRESSION=OFF",
  "-DWITH_LIBSHARPYUV=OFF",
  "-DWITH_EXAMPLES=OFF",
  "-DWITH_GDK_PIXBUF=OFF",
  "-DENABLE_PLUGIN_LOADING=OFF",
  "-DCMAKE_DISABLE_FIND_PACKAGE_Doxygen=ON",
]);
console.log(join(dist, "bin"));
