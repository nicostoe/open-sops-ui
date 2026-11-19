#!/usr/bin/env bash
# Installs the packages built for one target and checks, without opening a
# window, that the installed app starts and finds the sops binary it shipped.
#
# Usage: scripts/smoke-test.sh <rust-target-triple>
# Meant for CI: it installs the app on the machine it runs on.
set -euo pipefail

target="${1:?usage: smoke-test.sh <rust-target-triple>}"
src="target/$target/release/bundle"
report="$(mktemp)"

check() { # <label> <command...>
  local label="$1"; shift
  rm -f "$report"
  "$@" --self-check "$report" >/dev/null || { echo "smoke-test: $label: self-check failed" >&2; exit 1; }
  if ! grep -q '"source": "bundled"' "$report"; then
    echo "smoke-test: $label: the app didn't pick its bundled sops:" >&2
    cat "$report" >&2
    exit 1
  fi
  echo "smoke-test: $label: ok ($(grep '"version"' "$report" | tr -d ' ,'))"
}

first() { ls "$@" 2>/dev/null | head -n 1; }

case "$target" in
  *-linux-*)
    sudo dpkg -i "$(first "$src"/deb/*.deb)"
    check "deb" open-sops-ui
    appimage="$(first "$src"/appimage/*.AppImage)"
    chmod +x "$appimage"
    check "AppImage" "$appimage" --appimage-extract-and-run
    # A decoy named sops next to the AppImage, as in a Downloads folder, must
    # be ignored: the app has to keep using the sops inside its package.
    decoy_dir="$(mktemp -d)"
    cp "$appimage" "$decoy_dir/"
    printf '#!/bin/sh\necho "sops 9.9.9"\ntouch "%s/decoy-ran"\n' "$decoy_dir" > "$decoy_dir/sops"
    chmod +x "$decoy_dir/sops"
    check "AppImage next to a decoy sops" "$decoy_dir/$(basename "$appimage")" --appimage-extract-and-run
    if [ -e "$decoy_dir/decoy-ran" ] || grep -q '9.9.9' "$report"; then
      echo "smoke-test: the AppImage ran the sops lying next to it" >&2
      exit 1
    fi
    ;;
  *-apple-darwin)
    # The .dmg only wraps this bundle; running it in place avoids mounting
    # a disk image on the runner.
    check "app bundle" "$src/macos/Open SOPS UI.app/Contents/MacOS/open-sops-ui"
    ls "$src"/dmg/*.dmg >/dev/null
    ;;
  *-windows-*)
    setup="$(cygpath -w "$(first "$src"/nsis/*.exe)")"
    powershell -NoProfile -Command "Start-Process -Wait -FilePath '$setup' -ArgumentList '/S'"
    report="$(cygpath -w "$report")"
    check "setup.exe" "$LOCALAPPDATA/Open SOPS UI/open-sops-ui.exe"
    ;;
  *) echo "smoke-test: unsupported target: $target" >&2; exit 1 ;;
esac
