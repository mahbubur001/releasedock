#!/bin/bash
# Builds a Release ReleaseDock signed ad hoc for use without a Developer ID certificate.
# Usage: scripts/build-adhoc.sh [version] [build]   → prints the .app path on the last line.
#
# Ad-hoc signatures have no Team ID, and Hardened Runtime's library validation rejects
# frameworks without the app's Team ID, so these builds are signed without Hardened Runtime
# (as Xcode's Debug builds are) and without get-task-allow. Embedded Sparkle parts are
# re-signed from the inside out, never with --deep. Notarized builds (scripts/release.sh)
# keep Hardened Runtime and use the publisher's Developer ID.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION_ARGS=()
[ -n "${1:-}" ] && VERSION_ARGS+=("MARKETING_VERSION=$1")
[ -n "${2:-}" ] && VERSION_ARGS+=("CURRENT_PROJECT_VERSION=$2")
xcodegen generate --quiet
xcodebuild -project ReleaseDock.xcodeproj -scheme ReleaseDock -configuration Release \
  -derivedDataPath build/DerivedData CODE_SIGN_IDENTITY=- OTHER_CODE_SIGN_FLAGS= DEVELOPMENT_TEAM= \
  ${VERSION_ARGS[@]+"${VERSION_ARGS[@]}"} build | grep -E "error:|BUILD" >&2 || true
APP="$ROOT/build/DerivedData/Build/Products/Release/ReleaseDock.app"
SPARKLE="$APP/Contents/Frameworks/Sparkle.framework/Versions/B"
sign() { codesign --force --sign - --preserve-metadata=entitlements "$@" >&2; }
for item in "$SPARKLE/XPCServices/Installer.xpc" "$SPARKLE/XPCServices/Downloader.xpc" "$SPARKLE/Autoupdate" "$SPARKLE/Updater.app"; do
  [ -e "$item" ] && sign "$item"
done
sign "$APP/Contents/Frameworks/Sparkle.framework"
ENT="$(mktemp)"
codesign -d --entitlements - --xml "$APP" > "$ENT" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Delete :com.apple.security.get-task-allow" "$ENT" 2>/dev/null || true
codesign --force --sign - --entitlements "$ENT" "$APP" >&2
rm -f "$ENT"
codesign --verify --strict "$APP" >&2
echo "$APP"
