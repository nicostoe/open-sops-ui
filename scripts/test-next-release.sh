#!/usr/bin/env bash
# Checks scripts/next-release.sh against throwaway tags in a temporary clone:
# the first beta after a stable release follows the chosen bump, later betas
# count up (numerically, past gaps), a bigger bump moves on to a new version,
# stable promotes the commit of the latest beta and needs one, and dev builds
# get the version being worked on.
#
# Usage: scripts/test-next-release.sh
set -euo pipefail

cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git clone -q --no-hardlinks . "$work/repo"
cp scripts/next-release.sh "$work/repo/scripts/next-release.sh"
cd "$work/repo"
git tag -l | xargs -r git tag -d >/dev/null
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
commit() { git commit -q --allow-empty -m "$1"; git rev-parse HEAD; }

expect() {
  local want="$1"; shift
  local got
  got="$(scripts/next-release.sh "$@" 2>&1)" || got="refused: $got"
  case "$got" in
    $want) ;;
    *) echo "next release test failed: '$*' gave '$got', expected '$want'" >&2; exit 1 ;;
  esac
}

# No tags at all: the version in tauri.conf.json, for dev builds only.
conf="$(sed -n 's/^  "version": "\([0-9.]*\)",$/\1/p' src-tauri/tauri.conf.json)"
expect "$conf" dev
expect "refused: *Release a beta first*" stable

git tag v998.9.9                     # an older release
git tag -a v999.1.0 -m v999.1.0      # the latest release
git tag v999.0.5-beta.3              # betas of older versions don't count
head="$(commit "Work after the release")"
expect "999.1.1" dev
expect "refused: *newer than v999.1.0*" stable
expect "v999.1.1-beta.1 $head" beta
expect "v999.1.1-beta.1 $head" beta patch
expect "v999.2.0-beta.1 $head" beta minor
expect "v1000.0.0-beta.1 $head" beta major
expect "refused: *patch, minor or major*" beta huge
expect "refused: usage:*" nightly

git tag v999.1.1-beta.1
git tag v999.1.1-beta.9
tried="$(commit "The commit of the latest beta")"
git tag -a v999.1.1-beta.10 -m v999.1.1-beta.10
git tag v999.1.1-rc.1                # other suffixes don't count
head="$(commit "Work after the latest beta")"
expect "999.1.1" dev
expect "v999.1.1-beta.11 $head" beta
expect "v999.1.1-beta.11 $head" beta patch
expect "v999.1.1 $tried" stable

# A feature after all: minor moves the betas on to 999.2.0.
git tag v999.2.0-beta.1
expect "v999.2.0-beta.2 $head" beta patch
expect "v999.2.0-beta.2 $head" beta minor
expect "999.2.0" dev
expect "v999.2.0 $head" stable

git tag v999.2.0 "$head"
expect "999.2.1" dev
expect "refused: *newer than v999.2.0*" stable
expect "v999.2.1-beta.1 $head" beta

echo "next release test passed"
