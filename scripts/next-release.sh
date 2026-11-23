#!/usr/bin/env bash
# Works out release versions from the tags alone; no file holds the version.
#
#   next-release.sh beta [patch|minor|major]
#       The next pre-release and the commit to release (the current one).
#       The bump says what the version after the latest stable release is:
#       after v1.1.0, patch is 1.1.1, minor 1.2.0, major 2.0.0. If betas of
#       that version (or a higher one) are already out, it's their next
#       number: v1.1.1-beta.1, then v1.1.1-beta.2, ... Default: patch.
#   next-release.sh stable
#       The stable release of the version in beta, at the commit of its latest
#       beta: stable promotes what was tried, not whatever is newest. Refused
#       while no beta is newer than the latest stable release.
#   next-release.sh dev
#       The version being worked on, for builds that aren't releases: the
#       version in beta, or else the next patch. Without any release tags
#       (a shallow clone), the version in src-tauri/tauri.conf.json.
#
# Output: "<tag> <commit>" for beta and stable, "<version>" for dev.
# Only plain vX.Y.Z tags count as stable and vX.Y.Z-beta.N as betas.
#
# Usage: scripts/next-release.sh beta|stable|dev [patch|minor|major]
#        (needs the tags: git fetch --tags)
set -euo pipefail

cd "$(dirname "$0")/.."
channel="${1:-}"
bump="${2:-patch}"
case "$channel" in beta | stable | dev) ;; *) echo "usage: next-release.sh beta|stable|dev [patch|minor|major]" >&2; exit 2 ;; esac
case "$bump" in patch | minor | major) ;; *) echo "next-release.sh: the bump is patch, minor or major, not '$bump'" >&2; exit 2 ;; esac

# Versions as "X Y Z", sorted, newest last.
by_version() { sort -t' ' -k1,1n -k2,2n -k3,3n; }
stables="$(git tag -l 'v*' | sed -n 's/^v\([0-9][0-9]*\)\.\([0-9][0-9]*\)\.\([0-9][0-9]*\)$/\1 \2 \3/p' | by_version)"
betas="$(git tag -l 'v*-beta.*' | sed -n 's/^v\([0-9][0-9]*\)\.\([0-9][0-9]*\)\.\([0-9][0-9]*\)-beta\.\([0-9][0-9]*\)$/\1 \2 \3 \4/p')"

newer() { # newer "X Y Z" "X Y Z": is the first one higher?
  [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | by_version | tail -n 1)" = "$1" ]
}
dots() { tr ' ' . <<<"$1"; }

latest="$(tail -n 1 <<<"$stables")"
# The highest version with betas and no stable release yet, if any.
in_beta="$(cut -d' ' -f1-3 <<<"$betas" | sed '/^$/d' | by_version | tail -n 1)"
if [ -n "$in_beta" ] && [ -n "$latest" ] && ! newer "$in_beta" "$latest"; then in_beta=""; fi

next_after() { # next_after "X Y Z" patch|minor|major
  local x y z
  read -r x y z <<<"$1"
  case "$2" in
    patch) echo "$x $y $((z + 1))" ;;
    minor) echo "$x $((y + 1)) 0" ;;
    major) echo "$((x + 1)) 0 0" ;;
  esac
}
last_beta() { # the highest beta number of "X Y Z"
  { grep "^$1 " <<<"$betas" || true; } | cut -d' ' -f4 | sort -n | tail -n 1
}

case "$channel" in
  dev)
    if [ -n "$in_beta" ]; then dots "$in_beta"
    elif [ -n "$latest" ]; then dots "$(next_after "$latest" patch)"
    else sed -n 's/^  "version": "\([0-9]*\.[0-9]*\.[0-9]*\)",$/\1/p' src-tauri/tauri.conf.json
    fi
    ;;
  beta)
    target="$(next_after "${latest:-0 0 0}" "$bump")"
    # Betas already go further: carry on with them.
    if [ -n "$in_beta" ] && ! newer "$target" "$in_beta"; then target="$in_beta"; fi
    n="$(last_beta "$target")"
    echo "v$(dots "$target")-beta.$(( ${n:-0} + 1 )) $(git rev-parse HEAD)"
    ;;
  stable)
    if [ -z "$in_beta" ]; then
      since="the last release"
      [ -z "$latest" ] || since="v$(dots "$latest")"
      echo "next-release.sh: no beta is newer than $since. Release a beta first." >&2
      exit 1
    fi
    tag="v$(dots "$in_beta")"
    echo "$tag $(git rev-parse "$tag-beta.$(last_beta "$in_beta")^{commit}")"
    ;;
esac
