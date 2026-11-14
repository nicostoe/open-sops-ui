#!/usr/bin/env bash
# Runs the scripted UI test against an app binary and stores the screenshots.
# On Linux without a display it starts a virtual one.
#
# Usage: scripts/ui-test.sh <app-binary> <screenshot-folder>
set -euo pipefail

app="${1:?usage: ui-test.sh <app-binary> <screenshot-folder>}"
out="${2:?usage: ui-test.sh <app-binary> <screenshot-folder>}"

if [ "$(uname -s)" = "Linux" ] && [ -z "${DISPLAY:-}" ]; then
  exec xvfb-run -a -s "-screen 0 1280x800x24" node ui-tests/selftest.mjs "$app" "$out" --require-screenshots
fi
exec node ui-tests/selftest.mjs "$app" "$out" --require-screenshots
