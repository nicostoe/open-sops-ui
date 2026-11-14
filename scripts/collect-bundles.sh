#!/usr/bin/env bash
# Copies the packages Tauri built for one target into an output folder, with
# spaces removed from the names and an optional suffix before the extension.
#
# Usage: scripts/collect-bundles.sh <rust-target-triple> <out-dir> [suffix]
set -euo pipefail

target="${1:?usage: collect-bundles.sh <target> <out-dir> [suffix]}"
out="${2:?usage: collect-bundles.sh <target> <out-dir> [suffix]}"
suffix="${3:-}"
src="target/$target/release/bundle"

mkdir -p "$out"
count=0
for file in "$src"/deb/*.deb "$src"/rpm/*.rpm "$src"/appimage/*.AppImage \
            "$src"/dmg/*.dmg "$src"/msi/*.msi "$src"/nsis/*.exe; do
  [ -f "$file" ] || continue
  base="$(basename "$file")"
  stem="${base%.*}"
  cp "$file" "$out/${stem// /-}${suffix}.${base##*.}"
  count=$((count + 1))
done
if [ "$count" -eq 0 ]; then
  echo "collect-bundles: no packages found in $src" >&2; exit 1
fi
echo "collect-bundles: copied $count package(s) to $out"
