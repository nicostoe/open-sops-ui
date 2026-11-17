#!/usr/bin/env bash
# Prints the release notes for a version tag: the changes since the previous
# release, grouped by commit type (cliff.toml).
#
# A pre-release (v1.2.0-beta.1) lists the changes since the tag before it.
# A release (v1.2.0) lists everything since the last release, so the betas
# leading up to it are folded in.
#
# Usage: scripts/release-notes.sh [tag]
#   without a tag: the changes not released yet, as a preview.
# Needs git-cliff (cargo install git-cliff --locked) and the full git history.
set -euo pipefail

cd "$(dirname "$0")/.."
tag="${1:-}"
repo="https://github.com/nicostoe/open-sops-ui"

notes() { # <from or empty> <to> [extra git-cliff options]
  local from="$1" to="$2"; shift 2
  # With no earlier tag, everything since the first commit counts.
  [ -n "$from" ] || from="$(git rev-list --max-parents=0 "$to" | tail -n 1)"
  git cliff --config cliff.toml --strip all "$@" "$from..$to"
}

if [ -z "$tag" ]; then
  previous="$(git describe --tags --abbrev=0 HEAD 2>/dev/null || true)"
  notes "$previous" HEAD
  exit 0
fi

if [[ "$tag" == *-* ]]; then
  previous="$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)"
  notes "$previous" "$tag"
else
  # Pre-release tags are ignored, so their commits count toward this release.
  previous="$(git describe --tags --abbrev=0 --exclude '*-*' "$tag^" 2>/dev/null || true)"
  notes "$previous" "$tag" --ignore-tags '.*-.*'
fi

if [ -n "$previous" ]; then
  printf '\n**Full changelog:** %s/compare/%s...%s\n' "$repo" "$previous" "$tag"
fi
