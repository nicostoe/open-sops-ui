#!/usr/bin/env bash
# Runs a command and, if it fails, repeats the end of its output as a GitHub
# error annotation. Job logs can only be read when signed in; annotations are
# public, so a failed check explains itself on the pull request.
#
# Usage: scripts/ci-annotate.sh "<title>" <command...>
set -uo pipefail

title="${1:?usage: ci-annotate.sh <title> <command...>}"
shift
log="$(mktemp)"
if "$@" 2>&1 | tee "$log"; then
  exit 0
fi
message="$(grep -v -E '^\s*(Compiling|Downloaded|Downloading|Finished|Running)' "$log" | tail -n 30 \
  | sed 's/%/%25/g' | awk '{ printf "%s%%0A", $0 }')"
echo "::error title=${title}::${message}"
exit 1
