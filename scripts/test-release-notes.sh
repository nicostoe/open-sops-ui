#!/usr/bin/env bash
# Checks scripts/release-notes.sh against throwaway tags in a temporary clone:
# an annotated tag's message becomes the introduction, a message that only
# repeats the tag name is left out, and internal commits stay out of the notes.
#
# Usage: scripts/test-release-notes.sh   (needs git-cliff and the full history)
set -euo pipefail

cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git clone -q --no-hardlinks . "$work/repo"
cp cliff.toml "$work/repo/cliff.toml"
cp scripts/release-notes.sh "$work/repo/scripts/release-notes.sh"
cd "$work/repo"
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid

fail() { echo "release notes test failed: $1" >&2; printf '%s\n' "$notes" >&2; exit 1; }

git commit -q --allow-empty -m "feat: a change for the test"
git commit -q --allow-empty -m "Internal work for the test"
git tag -a v999.0.0 -m v999.0.0 -m "Introduction for the test."
notes="$(scripts/release-notes.sh v999.0.0 2>/dev/null)"
[ "$(printf '%s\n' "$notes" | sed -n '/[^[:space:]]/{p;q}')" = "Introduction for the test." ] \
  || fail "the tag message should open the notes"
grep -qx -- '- A change for the test (\[.*' <<<"$notes" || fail "the feature is missing"
! grep -q 'Internal work' <<<"$notes" || fail "an internal commit was listed"

git commit -q --allow-empty -m "fix: another change for the test"
git tag -a v999.0.1 -m v999.0.1
notes="$(scripts/release-notes.sh v999.0.1 2>/dev/null)"
[ "$(printf '%s\n' "$notes" | sed -n '/[^[:space:]]/{p;q}')" = "### Bug fixes" ] \
  || fail "a message that only names the tag should be left out"

echo "release notes test passed"
