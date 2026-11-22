#!/usr/bin/env bash
# Copies the packages Tauri built for one target into an output folder, with
# spaces removed from the names and an optional suffix before the extension.
#
# Tauri names most packages after the plain x.y.z from tauri.conf.json, so
# every pre-release of 1.2.0 would download as Open-SOPS-UI_1.2.0_amd64.AppImage.
# With OPEN_SOPS_UI_VERSION set (scripts/version.sh), the copies carry the full
# version instead: Open-SOPS-UI_1.2.0-beta.4_amd64.AppImage. The .rpm already
# has it in its release field (Open-SOPS-UI-1.2.0-0.beta.4.x86_64.rpm) and is
# left as it is. Only the file name changes; the package metadata doesn't.
#
# Usage: scripts/collect-bundles.sh <rust-target-triple> <out-dir> [suffix]
set -euo pipefail

target="${1:?usage: collect-bundles.sh <target> <out-dir> [suffix]}"
out="${2:?usage: collect-bundles.sh <target> <out-dir> [suffix]}"
suffix="${3:-}"
src="${BUNDLE_DIR:-target/$target/release/bundle}" # BUNDLE_DIR: for scripts/test-collect-bundles.sh
version="${OPEN_SOPS_UI_VERSION:-}"
core="${version%%-*}"

mkdir -p "$out"
count=0
for file in "$src"/deb/*.deb "$src"/rpm/*.rpm "$src"/appimage/*.AppImage \
            "$src"/dmg/*.dmg "$src"/msi/*.msi "$src"/nsis/*.exe; do
  [ -f "$file" ] || continue
  base="$(basename "$file")"
  stem="${base%.*}"
  if [ "$version" != "$core" ]; then
    stem="${stem/_"$core"_/_"$version"_}"
  fi
  cp "$file" "$out/${stem// /-}${suffix}.${base##*.}"
  count=$((count + 1))
done
if [ "$count" -eq 0 ]; then
  echo "collect-bundles: no packages found in $src" >&2; exit 1
fi
echo "collect-bundles: copied $count package(s) to $out"
