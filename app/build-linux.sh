#!/usr/bin/env bash
# Builds Farba for Linux (.deb for Debian / Ubuntu, .rpm for Fedora / openSUSE). Run from
# anywhere; on WSL build from a copy inside the Linux file system (e.g. ~/farba): /mnt/d is
# very slow, and node_modules from Windows holds win32 binaries.
#   build-linux.sh           the .deb and .rpm (core/target/release/bundle/{deb,rpm}) and the
#                            add-ons' Linux libraries
#   build-linux.sh --addons  only the add-ons' Linux libraries (core/target/addons/linux);
#                            `pnpm addons` on Windows runs this in WSL and packs them with
#                            the Windows DLLs into one package
set -euo pipefail

# The newest glibc / libstdc++ the binaries may need: Ubuntu 22.04 has glibc 2.35,
# Debian 12 / MX Linux 23 glibc 2.36 and GCC 12 (GLIBCXX_3.4.30). A binary built on a newer
# system can need more (see app/src-tauri/src/glibc_compat.rs).
glibc_max=2.35
glibcxx_max=3.4.30

addons_only=""
[ "${1:-}" = "--addons" ] && addons_only=1

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo "Rust is missing: curl https://sh.rustup.rs -sSf | sh"; exit 1; }
command -v pnpm >/dev/null || { echo "pnpm is missing: npm install -g pnpm@10.28.0"; exit 1; }

cd "$root/app"
bins=()
if [ -z "$addons_only" ]; then
  rustup target list --installed | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
  command -v wasm-pack >/dev/null || cargo install wasm-pack --locked

  # PDFium for PDF import (BSD-3 / Apache-2.0; prebuilt by bblanchon/pdfium-binaries, MIT).
  # The version matches the pdfium-render feature in core/farba-io/Cargo.toml.
  pdfium_version=7881
  pdfium="$root/core/target/pdfium"
  if [ "$(cat "$pdfium/VERSION.farba" 2>/dev/null)" != "$pdfium_version" ]; then
    rm -rf "$pdfium" && mkdir -p "$pdfium"
    curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/$pdfium_version/pdfium-linux-x64.tgz" \
      | tar xz -C "$pdfium"
    echo "$pdfium_version" > "$pdfium/VERSION.farba"
  fi

  pnpm install --frozen-lockfile
  pnpm tauri build
  bins+=("$root/core/target/release/farba")
fi
pnpm addons
bins+=("$root"/core/target/addons/linux/*.so)

# Symbols of $1 that need a newer $2 (GLIBC / GLIBCXX) than $3; weak ones are optional.
too_new() {
  objdump -T "$1" | grep -v ' w ' \
    | sed -nE "s/.*$2_([0-9.]+)\)?[[:space:]]+([^[:space:]]+)\$/\1 \2/p" \
    | while read -r ver name; do
        [ "$(printf '%s\n%s\n' "$3" "$ver" | sort -V | tail -1)" = "$3" ] || echo "$2_$ver $name"
      done
}
failed=""
for bin in "${bins[@]}"; do
  bad="$(too_new "$bin" GLIBC "$glibc_max"; too_new "$bin" GLIBCXX "$glibcxx_max")"
  [ -z "$bad" ] && continue
  echo "$(basename "$bin") needs a newer glibc / libstdc++ (it would not start on older systems):"
  echo "$bad"
  failed=1
done
if [ -n "$failed" ]; then
  echo "Wrap these functions (see app/src-tauri/src/glibc_compat.rs + build.rs)."
  rm -rf "$root"/core/target/release/bundle/deb/*.deb "$root"/core/target/release/bundle/rpm/*.rpm "$root/core/target/addons/linux"
  exit 1
fi

[ -z "$addons_only" ] && ls -lh "$root"/core/target/release/bundle/{deb/*.deb,rpm/*.rpm}
ls -lh "$root"/core/target/addons/linux/
