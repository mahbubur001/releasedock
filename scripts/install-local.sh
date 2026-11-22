#!/bin/bash
# Builds ReleaseDock (Release, ad hoc; see build-adhoc.sh) and installs it to /Applications.
# The previous copy is moved aside, never deleted.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$("$ROOT/scripts/build-adhoc.sh" "$@" | tail -1)"
if [ -e /Applications/ReleaseDock.app ]; then
  osascript -e 'tell application "ReleaseDock" to quit' >/dev/null 2>&1 || true
  PREVIOUS="$(mktemp -d /tmp/ReleaseDock-previous.XXXX)"
  mv /Applications/ReleaseDock.app "$PREVIOUS/"
  echo "Previous app moved to $PREVIOUS"
fi
ditto "$APP" /Applications/ReleaseDock.app
echo "Installed /Applications/ReleaseDock.app"
