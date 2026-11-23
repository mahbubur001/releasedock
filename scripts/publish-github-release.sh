#!/bin/bash
# Publishes a ReleaseDock build as a GitHub release of this repository so the app can update itself.
#
# Usage: scripts/publish-github-release.sh <version> <build-number> [--beta] [--notes "text"]
#   e.g. scripts/publish-github-release.sh 0.1.1 2 --notes "Faster repository reads."
#
# Steps: tests → ad-hoc Release build (scripts/build-adhoc.sh) → ZIP → Sparkle EdDSA signature
# (private key from the login Keychain, or from $SPARKLE_PRIVATE_KEY when set, as in the GitHub
# Actions release workflow) → source tag vX.Y.Z pushed → GitHub release with the ZIP in
# mahbubur001/releasedock → updates/appcast.xml regenerated, committed and pushed to main.
# The build number must increase with every release: Sparkle compares it.
# Builds 1–3 (0.1.0–0.1.2) were published from the retired releasedock-releases repository.
set -euo pipefail
VERSION="${1:?version required}"; BUILD="${2:?build number required}"; shift 2
CHANNEL=""; NOTES="See the ReleaseDock changelog for details."
while [ $# -gt 0 ]; do
  case "$1" in
    --beta) CHANNEL="beta"; shift ;;
    --notes) NOTES="$2"; shift 2 ;;
    *) echo "Unknown option $1" >&2; exit 1 ;;
  esac
done
RELEASES_REPO="mahbubur001/releasedock"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
FEED_DIR="$ROOT/updates"
SPARKLE_BIN="$ROOT/build/DerivedData/SourcePackages/artifacts/sparkle/Sparkle/bin"
TAG="v$VERSION"
ZIP="$DIST/ReleaseDock-$VERSION.zip"

cd "$ROOT"
[ -z "$(git status --porcelain)" ] || { echo "Commit your changes first (the release is tagged from HEAD)." >&2; exit 1; }
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || { echo "Release from main (the feed is served from main)." >&2; exit 1; }
git fetch -q origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main differs from origin/main; pull or push first." >&2; exit 1; }
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && { echo "Tag $TAG already exists" >&2; exit 1; }
gh release view "$TAG" --repo "$RELEASES_REPO" >/dev/null 2>&1 && { echo "$TAG already exists in $RELEASES_REPO" >&2; exit 1; }
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "Version must look like 1.2.3 or 1.2.3-beta1" >&2; exit 1; }
[[ "$BUILD" =~ ^[1-9][0-9]*$ ]] || { echo "Build number must be a positive integer" >&2; exit 1; }
LAST_BUILD=$(python3 -c 'import json,sys; print(max([3]+[int(r["build"]) for r in json.load(open(sys.argv[1]))]))' "$FEED_DIR/releases.json" 2>/dev/null || echo 3)
[ "$BUILD" -gt "$LAST_BUILD" ] || { echo "Build number must be greater than $LAST_BUILD (Sparkle compares it)" >&2; exit 1; }

echo "==> Tests"
TEST_LOG="$(mktemp)"
(cd Packages/ReleaseDockKit && swift test --parallel >"$TEST_LOG" 2>&1) || { tail -80 "$TEST_LOG" >&2; echo "Tests failed" >&2; exit 1; }

echo "==> Building $VERSION ($BUILD)"
APP="$("$ROOT/scripts/build-adhoc.sh" "$VERSION" "$BUILD" | tail -1)"
BUILT_VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")
BUILT_BUILD=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$APP/Contents/Info.plist")
FEED=$(/usr/libexec/PlistBuddy -c "Print :SUFeedURL" "$APP/Contents/Info.plist")
[ "$BUILT_VERSION" = "$VERSION" ] && [ "$BUILT_BUILD" = "$BUILD" ] || { echo "Built version mismatch ($BUILT_VERSION/$BUILT_BUILD)" >&2; exit 1; }
[ -n "$FEED" ] || { echo "SUFeedURL missing from the build" >&2; exit 1; }

echo "==> Packaging"
mkdir -p "$DIST"
[ ! -e "$ZIP" ] || mv "$ZIP" "$(mktemp -d /tmp/ReleaseDock-oldzip.XXXX)/"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
# sparkle:edSignature="..." length="..."; the key goes through stdin, never the command line.
if [ -n "${SPARKLE_PRIVATE_KEY:-}" ]; then
  SIGNATURE_ATTRS=$(printf '%s' "$SPARKLE_PRIVATE_KEY" | "$SPARKLE_BIN/sign_update" --ed-key-file - "$ZIP")
else
  SIGNATURE_ATTRS=$("$SPARKLE_BIN/sign_update" "$ZIP")
fi
echo "    $SIGNATURE_ATTRS"

echo "==> Tagging source $TAG"
git tag -a "$TAG" -m "ReleaseDock $VERSION ($BUILD)"
git push -q origin "$TAG"

echo "==> GitHub release $TAG"
PRERELEASE=(); [ -n "$CHANNEL" ] && PRERELEASE=(--prerelease)
gh release create "$TAG" "$ZIP" --repo "$RELEASES_REPO" --verify-tag --title "ReleaseDock $VERSION" --notes "$NOTES" ${PRERELEASE[@]+"${PRERELEASE[@]}"} >/dev/null

echo "==> Update feed"
mkdir -p "$FEED_DIR"
python3 - "$FEED_DIR" "$VERSION" "$BUILD" "$CHANNEL" "$NOTES" "$SIGNATURE_ATTRS" "$RELEASES_REPO" <<'PY'
import email.utils, html, json, os, re, sys, time
feed_dir, version, build, channel, notes, attrs, repo = sys.argv[1:]
signature = re.search(r'sparkle:edSignature="([^"]+)"', attrs).group(1)
length = re.search(r'length="(\d+)"', attrs).group(1)
path = os.path.join(feed_dir, "releases.json")
releases = json.load(open(path)) if os.path.exists(path) else []
releases = [r for r in releases if r["version"] != version]
releases.append({"version": version, "build": build, "channel": channel, "notes": notes,
                 "signature": signature, "length": length, "date": email.utils.formatdate(time.time(), usegmt=True)})
releases.sort(key=lambda r: int(r["build"]), reverse=True)
json.dump(releases, open(path, "w"), indent=2)
items = []
for r in releases:
    url = f"https://github.com/{repo}/releases/download/v{r['version']}/ReleaseDock-{r['version']}.zip"
    channel_tag = f"\n      <sparkle:channel>{r['channel']}</sparkle:channel>" if r["channel"] else ""
    items.append(f"""    <item>
      <title>Version {html.escape(r['version'])}</title>
      <pubDate>{r['date']}</pubDate>
      <sparkle:version>{r['build']}</sparkle:version>
      <sparkle:shortVersionString>{html.escape(r['version'])}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>14.0</sparkle:minimumSystemVersion>{channel_tag}
      <description><![CDATA[<p>{html.escape(r['notes'])}</p>]]></description>
      <enclosure url="{url}" length="{r['length']}" type="application/octet-stream" sparkle:edSignature="{r['signature']}"/>
    </item>""")
xml = f"""<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>ReleaseDock</title>
    <link>https://github.com/{repo}</link>
    <description>ReleaseDock updates</description>
    <language>en</language>
{chr(10).join(items)}
  </channel>
</rss>
"""
open(os.path.join(feed_dir, "appcast.xml"), "w").write(xml)
PY
git add updates/appcast.xml updates/releases.json
git commit -q -m "Update feed: ReleaseDock $VERSION ($BUILD)"
git push -q origin main

echo
echo "Published ReleaseDock $VERSION ($BUILD)"
echo "  Download: https://github.com/$RELEASES_REPO/releases/tag/$TAG"
echo "  Feed:     $FEED"
