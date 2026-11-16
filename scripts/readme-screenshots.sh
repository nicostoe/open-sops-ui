#!/usr/bin/env bash
# Regenerates the pictures in docs/screenshots/ from the UI test.
#
# Runs the test in a clean Ubuntu container, so the screenshots show neutral
# paths (/home/demo/...) and the same fonts every time, never anything from
# the machine that ran it.
#
# Usage: scripts/readme-screenshots.sh <path-to-the-with-sops-.deb> [folder-for-all-screenshots]
# Needs Docker. Build the package first:
#   scripts/fetch-sops.sh x86_64-unknown-linux-gnu
#   npm run build -- --bundles deb --config src-tauri/tauri.sops.conf.json
set -euo pipefail

deb="$(cd "$(dirname "${1:?usage: readme-screenshots.sh <with-sops .deb>}")" && pwd)/$(basename "$1")"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

docker run --rm -v "$root":/src:ro -v "$deb":/pkg/app.deb:ro -v "$out":/out -w /src ubuntu:24.04 bash -euc '
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -q >/dev/null
  apt-get install -y -q --no-install-recommends /pkg/app.deb xvfb xauth imagemagick nodejs git \
    fonts-inter fonts-jetbrains-mono >/dev/null
  export OPEN_SOPS_UI_SELFTEST_ROOT=/home/demo
  export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1
  scripts/ui-test.sh /usr/bin/open-sops-ui /out
  chmod -R a+rwX /out
'

mkdir -p "$root/docs/screenshots"
cp "$out/07-unsaved-changes.png" "$root/docs/screenshots/editor-light.png"
cp "$out/14-dark-changes.png" "$root/docs/screenshots/changes-dark.png"
if [ -n "${2:-}" ]; then mkdir -p "$2" && cp "$out"/*.png "$2"/; fi
echo "readme-screenshots: updated docs/screenshots/"
