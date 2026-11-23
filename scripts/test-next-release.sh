#!/usr/bin/env bash
# Checks scripts/next-release.sh against throwaway tags in a temporary clone:
# betas count up (numerically, past gaps) at the current commit, stable
# promotes the commit of the latest beta, and both refuse once that version is
# released.
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
sed -i.bak 's/^  "version": "[0-9.]*",$/  "version": "999.1.0",/' src-tauri/tauri.conf.json
git commit -q -am "Version 999.1.0 for the test"

expect() {
  local got
  got="$(scripts/next-release.sh "$1" 2>&1)" || got="refused: $got"
  case "$got" in
    $2) ;;
    *) echo "next release test failed: $1 gave '$got', expected '$2'" >&2; exit 1 ;;
  esac
}

first="$(git rev-parse HEAD)"
expect beta "v999.1.0-beta.1 $first"
expect stable "refused: *Release a beta first*"
git tag v999.1.0-beta.1
git tag v999.1.0-beta.9
git commit -q --allow-empty -m "The commit of the latest beta"
tried="$(git rev-parse HEAD)"
git tag -a v999.1.0-beta.10 -m v999.1.0-beta.10
git tag v999.0.0-beta.40   # another version's betas don't count
git tag v999.1.0-rc.1      # nor other suffixes
git commit -q --allow-empty -m "Work after the latest beta"
now="$(git rev-parse HEAD)"
expect beta "v999.1.0-beta.11 $now"
expect stable "v999.1.0 $tried"
git tag -a v999.1.0 -m v999.1.0 "$tried"
expect beta "refused: *already released*"
expect stable "refused: *already released*"
expect nightly "refused: usage:*"

echo "next release test passed"
