#!/bin/bash
# Builds a self-contained end-to-end setup for the desktop app, without touching real data:
#   <dir>/repo            local svn repository (file://) with Sample Plugin 1.0.0 released
#   <dir>/plugin-1.1.0    the next version, ready to release
#   <dir>/data            a ReleaseDock data folder whose only product points at the repo
# Prints the product id. Needs svn, svnadmin and cargo.
#
#   desktop/scripts/make-e2e.sh /tmp/rd-e2e
#   cd desktop && tail -f /dev/null | RELEASEDOCK_DATA_DIR=/tmp/rd-e2e/data \
#     RELEASEDOCK_UI_TEST_RELEASE="<id>|/tmp/rd-e2e/plugin-1.1.0|assisted" RELEASEDOCK_UI_TEST_PUBLISH=1 npm run tauri dev
set -euo pipefail
E=${1:?target folder}
[ -e "$E" ] && { echo "$E already exists; choose a new folder" >&2; exit 1; }
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$E/plugin-1.0.0"
cat > "$E/plugin-1.0.0/sample-plugin.php" <<'PHP'
<?php
/**
 * Plugin Name: Sample Plugin
 * Version: 1.0.0
 * Requires PHP: 7.4
 * License: GPLv2 or later
 * Text Domain: sample-plugin
 */
PHP
cat > "$E/plugin-1.0.0/readme.txt" <<'TXT'
=== Sample Plugin ===
Contributors: alice
Tags: sample
Requires at least: 6.0
Tested up to: 7.1
Stable tag: 1.0.0
Requires PHP: 7.4
License: GPLv2 or later

A sample plugin for testing ReleaseDock.

== Description ==

Sample.

== Changelog ==

= 1.0.0 =
* First release.
TXT
svnadmin create "$E/repo"
mkdir -p "$E/seed/trunk" "$E/seed/tags/1.0.0" "$E/seed/assets"
cp "$E/plugin-1.0.0/"* "$E/seed/trunk/"
cp "$E/plugin-1.0.0/"* "$E/seed/tags/1.0.0/"
svn import -q -m "Initial" "$E/seed" "file://$E/repo"
cp -R "$E/plugin-1.0.0" "$E/plugin-1.1.0"
perl -pi -e 's/Version: 1.0.0/Version: 1.1.0/' "$E/plugin-1.1.0/sample-plugin.php"
perl -0pi -e 's/Stable tag: 1.0.0/Stable tag: 1.1.0/; s/== Changelog ==\n/== Changelog ==\n\n= 1.1.0 =\n* Faster bookings.\n/' "$E/plugin-1.1.0/readme.txt"
cd "$ROOT/rust" && cargo run -q -p rd-store --example seed_product -- "$E/data" "file://$E/repo" "$E/wc" "$E/plugin-1.1.0"
