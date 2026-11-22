#!/bin/bash
# Builds the UI test driver, launches ReleaseDock (Debug) with RELEASEDOCK_DATA_DIR (default: a
# new empty temp folder; pass a demo data folder with at least two products and releases for the
# list checks), runs the real-input checks, then quits that instance.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="${1:-$(mktemp -d /tmp/rd-uitest-data.XXXX)}"
BUILD="$(mktemp -d /tmp/rd-uitest-build.XXXX)"
cp "$ROOT/UITests/UIDriver.swift" "$BUILD/"
cp "$ROOT/UITests/main.swift" "$BUILD/main.swift"
swiftc -O -o "$BUILD/ui-tests" "$BUILD/UIDriver.swift" "$BUILD/main.swift"
EXE="$ROOT/build/DerivedData/Build/Products/Debug/ReleaseDock.app/Contents/MacOS/ReleaseDock"
env RELEASEDOCK_DATA_DIR="$DATA" "$EXE" >/dev/null 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
sleep_seconds() { perl -e "select(undef,undef,undef,$1)"; }
sleep_seconds 5
"$BUILD/ui-tests" "$PID"
