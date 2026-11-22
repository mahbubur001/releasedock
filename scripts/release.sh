#!/bin/bash
# Builds, signs, notarizes and packages a ReleaseDock release, then signs the archive for
# Sparkle and regenerates the appcast.
#
# Requirements (see RELEASE_WORKFLOW.md):
#   - Config/Local.xcconfig with DEVELOPMENT_TEAM, SPARKLE_PUBLIC_ED_KEY, SPARKLE_FEED_URL
#   - "Developer ID Application" certificate in the login keychain
#   - notarytool credentials: xcrun notarytool store-credentials releasedock-notary ...
#   - Sparkle EdDSA private key in the login keychain (Sparkle's generate_keys)
#
# Usage: scripts/release.sh <version> <build-number> [--beta]
set -euo pipefail

VERSION="${1:?version required, e.g. 1.2.0}"
BUILD="${2:?build number required, e.g. 42}"
CHANNEL_ARGS=()
[[ "${3:-}" == "--beta" ]] && CHANNEL_ARGS=(--channel beta)
NOTARY_PROFILE="${NOTARY_PROFILE:-releasedock-notary}"
# All archives listed in the appcast must share this prefix (e.g. a GitHub Pages /updates/ folder).
DOWNLOAD_URL_PREFIX="${DOWNLOAD_URL_PREFIX:?set DOWNLOAD_URL_PREFIX, e.g. https://your-org.github.io/releasedock/updates/}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist/$VERSION"
ARCHIVE="$DIST/ReleaseDock.xcarchive"
EXPORT="$DIST/export"
ZIP="$DIST/ReleaseDock-$VERSION.zip"
UPDATES="$ROOT/dist/updates"

[[ -f "$ROOT/Config/Local.xcconfig" ]] || { echo "Config/Local.xcconfig missing" >&2; exit 1; }
grep -q '^SPARKLE_PUBLIC_ED_KEY *= *[A-Za-z0-9+/=]\{20,\}' "$ROOT/Config/Local.xcconfig" \
  || { echo "SPARKLE_PUBLIC_ED_KEY is not set in Config/Local.xcconfig" >&2; exit 1; }

rm -rf "$DIST" && mkdir -p "$DIST" "$UPDATES"
cd "$ROOT"
xcodegen generate --quiet

echo "==> Testing"
(cd Packages/ReleaseDockKit && swift test --parallel)

echo "==> Archiving $VERSION ($BUILD)"
xcodebuild -project ReleaseDock.xcodeproj -scheme ReleaseDock -configuration Release \
  -archivePath "$ARCHIVE" MARKETING_VERSION="$VERSION" CURRENT_PROJECT_VERSION="$BUILD" archive

echo "==> Exporting with Developer ID (re-signs Sparkle's helpers)"
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportPath "$EXPORT" \
  -exportOptionsPlist "$ROOT/scripts/ExportOptions.plist"
APP="$EXPORT/ReleaseDock.app"

echo "==> Verifying signature"
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -d --entitlements :- "$APP" | grep -q "get-task-allow" && { echo "get-task-allow present; refusing to notarize" >&2; exit 1; }

echo "==> Notarizing"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$DIST/notarize.zip"
xcrun notarytool submit "$DIST/notarize.zip" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$APP"
spctl --assess --type execute --verbose "$APP"

echo "==> Packaging"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
shasum -a 256 "$ZIP" | tee "$ZIP.sha256"

echo "==> Sparkle signing and appcast"
SPARKLE_BIN="$(find "$ROOT/build" "$HOME/Library/Developer/Xcode/DerivedData" -path '*artifacts/sparkle/Sparkle/bin' -type d 2>/dev/null | head -1)"
[[ -n "$SPARKLE_BIN" ]] || { echo "Sparkle tools not found; build the project once in Xcode" >&2; exit 1; }
cp "$ZIP" "$UPDATES/"
"$SPARKLE_BIN/generate_appcast" "${CHANNEL_ARGS[@]}" \
  --download-url-prefix "$DOWNLOAD_URL_PREFIX" \
  "$UPDATES"

echo
echo "Done. Next:"
echo "  1. Upload $UPDATES (archives + appcast.xml) to $DOWNLOAD_URL_PREFIX"
echo "  2. Create GitHub release v$VERSION with $ZIP attached (drives update notifications)"
