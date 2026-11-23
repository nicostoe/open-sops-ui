#!/usr/bin/env bash
# Prints the tag of the next release on a channel and the commit to release,
# from the version in src-tauri/tauri.conf.json and the tags that exist:
#
#   beta    v1.2.0-beta.<n+1>, after the highest v1.2.0-beta.<n> (beta.1
#           first), at the current commit
#   stable  v1.2.0, at the commit of the latest v1.2.0 beta: a stable release
#           promotes what was tried as a beta, not whatever main holds now.
#           Refused while there is no beta.
#
# Output: "<tag> <commit>".
#
# Refuses both once v1.2.0 is released: a beta of a released version would
# sort below it, and a version is released only once. Raise the version in
# tauri.conf.json first.
#
# Usage: scripts/next-release.sh beta|stable   (needs the tags: git fetch --tags)
set -euo pipefail

cd "$(dirname "$0")/.."
channel="${1:-}"
case "$channel" in
  beta | stable) ;;
  *) echo "usage: next-release.sh beta|stable" >&2; exit 2 ;;
esac

base="$(sed -n 's/^  "version": "\([0-9]*\.[0-9]*\.[0-9]*\)",$/\1/p' src-tauri/tauri.conf.json)"
[ -n "$base" ] || { echo "next-release.sh: no x.y.z version in src-tauri/tauri.conf.json" >&2; exit 1; }

if git rev-parse -q --verify "refs/tags/v$base" >/dev/null; then
  echo "next-release.sh: v$base is already released. Raise the version in src-tauri/tauri.conf.json, then release again." >&2
  exit 1
fi

last="$(git tag -l "v$base-beta.*" | sed -n "s/^v${base//./\\.}-beta\.\([0-9][0-9]*\)$/\1/p" | sort -n | tail -n 1)"

if [ "$channel" = stable ]; then
  if [ -z "$last" ]; then
    echo "next-release.sh: there is no v$base beta to promote. Release a beta first." >&2
    exit 1
  fi
  echo "v$base $(git rev-parse "v$base-beta.$last^{commit}")"
  exit 0
fi

echo "v$base-beta.$(( ${last:-0} + 1 )) $(git rev-parse HEAD)"
