#!/usr/bin/env bash
# Installs the app's package inside a Linux distribution's container and runs
# the self-check and the scripted UI test there. Catches wrong package
# dependencies, missing libraries and web view differences between distros.
#
# Usage (as root, inside the container, from the repository root):
#   scripts/distro-test.sh <folder-with-packages> <output-folder>
# Needs node on PATH. Picks the -with-sops .deb or .rpm by package manager.
set -euo pipefail

packages="${1:?usage: distro-test.sh <folder-with-packages> <output-folder>}"
out="${2:?usage: distro-test.sh <folder-with-packages> <output-folder>}"

if command -v apt-get >/dev/null 2>&1; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -q
  apt-get install -y -q --no-install-recommends "$(ls "$packages"/*with-sops.deb | head -n 1)" \
    xvfb xauth imagemagick ca-certificates fonts-dejavu-core
elif command -v dnf >/dev/null 2>&1; then
  dnf install -y -q "$(ls "$packages"/*with-sops.rpm | head -n 1)" \
    xorg-x11-server-Xvfb xorg-x11-xauth ImageMagick dejavu-sans-fonts
else
  echo "distro-test: no supported package manager found" >&2; exit 1
fi

. /etc/os-release
echo "distro-test: $PRETTY_NAME"
sops-env --self-check /tmp/self-check.json
grep -q '"source": "bundled"' /tmp/self-check.json

# Containers have no user session: no sandbox helper and no GPU.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1
scripts/ui-test.sh "$(command -v sops-env)" "$out"
