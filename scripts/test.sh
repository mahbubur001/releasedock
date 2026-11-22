#!/bin/bash
# Runs the core test suite (unit + local-SVN integration tests) and builds the app.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/Packages/ReleaseDockKit"
swift test --parallel
cd "$ROOT"
command -v xcodegen >/dev/null || { echo "xcodegen is required: brew install xcodegen" >&2; exit 1; }
xcodegen generate --quiet
xcodebuild -project ReleaseDock.xcodeproj -scheme ReleaseDock -configuration Debug \
  -derivedDataPath build/DerivedData build | grep -E "error:|warning:|BUILD" || true
