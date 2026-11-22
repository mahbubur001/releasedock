#!/bin/bash
# Builds a Release (non-Debug) ReleaseDock signed ad hoc and installs it to /Applications.
# For personal use on this Mac only; distribution builds use scripts/release.sh (Developer ID).
#
# Ad-hoc signatures have no Team ID, and Hardened Runtime's library validation rejects
# frameworks without the app's Team ID, so a local ad-hoc build cannot run with Hardened
# Runtime (Xcode's Debug builds skip it for the same reason). The app and the embedded Sparkle
# parts are therefore re-signed ad hoc from the inside out (never with --deep), without the
# runtime flag and without get-task-allow. Notarized builds from scripts/release.sh keep
# Hardened Runtime and are signed with the publisher's Developer ID.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
xcodegen generate --quiet
xcodebuild -project ReleaseDock.xcodeproj -scheme ReleaseDock -configuration Release \
  -derivedDataPath build/DerivedData CODE_SIGN_IDENTITY=- OTHER_CODE_SIGN_FLAGS= DEVELOPMENT_TEAM= build \
  | grep -E "error:|BUILD" || true
APP="build/DerivedData/Build/Products/Release/ReleaseDock.app"
SPARKLE="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
sign() { codesign --force --sign - --preserve-metadata=entitlements "$@"; }
for item in "$SPARKLE/XPCServices/Installer.xpc" "$SPARKLE/XPCServices/Downloader.xpc" "$SPARKLE/Autoupdate" "$SPARKLE/Updater.app"; do
  [ -e "$item" ] && sign "$item"
done
sign "$APP/Contents/Frameworks/Sparkle.framework"
# App entitlements without the debugging-only get-task-allow.
ENT="$(mktemp)"
codesign -d --entitlements - --xml "$APP" > "$ENT" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Delete :com.apple.security.get-task-allow" "$ENT" 2>/dev/null || true
codesign --force --sign - --entitlements "$ENT" "$APP"
rm -f "$ENT"
codesign --verify --strict --verbose=1 "$APP"
# Replace the installed copy without deleting anything: the previous app is moved aside.
if [ -e /Applications/ReleaseDock.app ]; then
  osascript -e 'tell application "ReleaseDock" to quit' >/dev/null 2>&1 || true
  PREVIOUS="$(mktemp -d /tmp/ReleaseDock-previous.XXXX)"
  mv /Applications/ReleaseDock.app "$PREVIOUS/"
  echo "Previous app moved to $PREVIOUS"
fi
ditto "$APP" /Applications/ReleaseDock.app
echo "Installed /Applications/ReleaseDock.app"
