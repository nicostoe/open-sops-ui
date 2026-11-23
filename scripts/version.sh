#!/usr/bin/env bash
# Works out the version of this build from the Git tag and makes it available
# to the build:
#
#   OPEN_SOPS_UI_VERSION   the full version, compiled into the app. It's what
#                          the app shows and compares when it looks for updates.
#   src-tauri/tauri.version.conf.json
#                          package metadata that formats can take without
#                          breaking (pass it to `tauri build --config`),
#                          including the plain x.y.z for every package.
#
# Tag v1.2.0          -> 1.2.0   (or RELEASE_TAG=v1.2.0, for a release started
#                                 from the Actions tab, before the tag exists)
# Tag v1.2.0-beta.4   -> 1.2.0-beta.4
# Any other CI build  -> <version being worked on>-dev.<run number>, from the
#                        tags (scripts/next-release.sh dev)
# Local, outside CI   -> the version in tauri.conf.json, which only serves
#                        local builds: releases take theirs from the tag
#
# The packages themselves keep the plain x.y.z: .rpm refuses a "-" in the
# version and .deb would sort a release below its own betas. Where a format has
# a separate field, it gets the pre-release: the MSI's fourth number (a label
# only; Windows Installer compares the first three) and the rpm release, which
# makes dnf order 1.2.0-0.beta.4 < 1.2.0-0.beta.5 < 1.2.0-1.
#
# Usage: scripts/version.sh   (in GitHub Actions it also writes $GITHUB_ENV)
set -euo pipefail

cd "$(dirname "$0")/.."
base="$(sed -n 's/^  "version": "\([0-9]*\.[0-9]*\.[0-9]*\)",$/\1/p' src-tauri/tauri.conf.json)"
[ -n "$base" ] || { echo "version.sh: no x.y.z version in src-tauri/tauri.conf.json" >&2; exit 1; }

ref="${GITHUB_REF:-}"
if [ -n "${RELEASE_TAG:-}" ]; then ref="refs/tags/$RELEASE_TAG"; fi
if [[ "$ref" == refs/tags/v* ]]; then
  version="${ref#refs/tags/v}"
elif [ -n "${GITHUB_RUN_NUMBER:-}" ]; then
  version="$(scripts/next-release.sh dev)-dev.$GITHUB_RUN_NUMBER"
else
  version="$base"
fi

semver='^([0-9]+)\.([0-9]+)\.([0-9]+)(-([0-9A-Za-z.-]+))?$'
[[ "$version" =~ $semver ]] || { echo "version.sh: '$version' isn't a version like 1.2.0 or 1.2.0-beta.4" >&2; exit 1; }
core="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[3]}"
pre="${BASH_REMATCH[5]}"

if [ -n "$pre" ]; then
  number="$(grep -oE '[0-9]+$' <<<"$pre" || echo 0)"
  msi="$core.$((number % 65536))"
  rpm_release="0.${pre//-/.}"
else
  msi="$core"
  rpm_release="1"
fi

cat > src-tauri/tauri.version.conf.json <<JSON
{
  "version": "$core",
  "bundle": {
    "windows": { "wix": { "version": "$msi" } },
    "linux": { "rpm": { "release": "$rpm_release" } }
  }
}
JSON

echo "version.sh: $version (MSI $msi, rpm release $rpm_release)"
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "OPEN_SOPS_UI_VERSION=$version" >> "$GITHUB_ENV"
else
  echo "export OPEN_SOPS_UI_VERSION=$version"
fi
