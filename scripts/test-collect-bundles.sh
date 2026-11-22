#!/usr/bin/env bash
# Checks the package names scripts/collect-bundles.sh produces, with the names
# Tauri gives its packages: a pre-release carries its full version, so files of
# different pre-releases don't share a name, and a release keeps plain x.y.z.
#
# Usage: scripts/test-collect-bundles.sh
set -euo pipefail

cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

bundle="$work/bundle"
mkdir -p "$bundle"/{deb,rpm,appimage,dmg,msi,nsis}
touch "$bundle/deb/Open SOPS UI_1.2.0_amd64.deb" \
      "$bundle/rpm/Open SOPS UI-1.2.0-0.beta.4.x86_64.rpm" \
      "$bundle/appimage/Open SOPS UI_1.2.0_amd64.AppImage" \
      "$bundle/dmg/Open SOPS UI_1.2.0_aarch64.dmg" \
      "$bundle/msi/Open SOPS UI_1.2.0_x64_en-US.msi" \
      "$bundle/nsis/Open SOPS UI_1.2.0_x64-setup.exe"

# Every check also runs as bash 4.2 would, which like macOS's bash 3.2 keeps
# quotes in a substitution's replacement.
collect() { # <version> <out> [suffix]
  rm -rf "$2"
  BASH_COMPAT="${COMPAT:-}" OPEN_SOPS_UI_VERSION="$1" BUNDLE_DIR="$bundle" scripts/collect-bundles.sh test "$2" "${3:-}" >/dev/null
  (cd "$2" && ls | LC_ALL=C sort)
}
expect() { # <what> <actual> <expected...>
  local what="$1" actual="$2"; shift 2
  if [ "$actual" != "$(printf '%s\n' "$@")" ]; then
    echo "collect-bundles test failed: $what${COMPAT:+ (as bash $COMPAT)}" >&2
    echo "got:" >&2; printf '%s\n' "$actual" >&2
    exit 1
  fi
}

for COMPAT in "" 42; do
expect "a pre-release carries its full version" "$(collect 1.2.0-beta.4 "$work/pre" -with-sops)" \
  "Open-SOPS-UI-1.2.0-0.beta.4.x86_64-with-sops.rpm" \
  "Open-SOPS-UI_1.2.0-beta.4_aarch64-with-sops.dmg" \
  "Open-SOPS-UI_1.2.0-beta.4_amd64-with-sops.AppImage" \
  "Open-SOPS-UI_1.2.0-beta.4_amd64-with-sops.deb" \
  "Open-SOPS-UI_1.2.0-beta.4_x64-setup-with-sops.exe" \
  "Open-SOPS-UI_1.2.0-beta.4_x64_en-US-with-sops.msi"

expect "a release keeps x.y.z" "$(collect 1.2.0 "$work/release")" \
  "Open-SOPS-UI-1.2.0-0.beta.4.x86_64.rpm" \
  "Open-SOPS-UI_1.2.0_aarch64.dmg" \
  "Open-SOPS-UI_1.2.0_amd64.AppImage" \
  "Open-SOPS-UI_1.2.0_amd64.deb" \
  "Open-SOPS-UI_1.2.0_x64-setup.exe" \
  "Open-SOPS-UI_1.2.0_x64_en-US.msi"

expect "without a version, names stay as Tauri made them" "$(collect "" "$work/plain")" \
  "Open-SOPS-UI-1.2.0-0.beta.4.x86_64.rpm" \
  "Open-SOPS-UI_1.2.0_aarch64.dmg" \
  "Open-SOPS-UI_1.2.0_amd64.AppImage" \
  "Open-SOPS-UI_1.2.0_amd64.deb" \
  "Open-SOPS-UI_1.2.0_x64-setup.exe" \
  "Open-SOPS-UI_1.2.0_x64_en-US.msi"

done

echo "collect-bundles test passed"
