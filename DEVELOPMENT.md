# Development

> **Continuing the work?** Start with [docs/HANDOFF.md](docs/HANDOFF.md): current state, setup for macOS, Windows and Linux, and the exact next steps. The cross-platform rewrite lives in `rust/` (core) and `desktop/` (Tauri app).

## Prerequisites

- macOS 14+, Xcode 16 or newer (developed with Xcode 27.2 / Swift 6.4)
- `brew install subversion xcodegen`
- Optional: `php` (enables the differential `version_compare` test)

## Layout

```
App/                      SwiftUI app (views, view models, Sparkle/notifications glue)
  Sources/  Resources/    Info.plist and entitlements are generated from project.yml
Packages/ReleaseDockKit/  ReleaseDockCore library + tests (all release logic)
Config/                   xcconfig files (Local.xcconfig is git-ignored)
scripts/                  test.sh, release.sh, generate-icon.swift, ExportOptions.plist
docs/research/            sourced research notes
project.yml               XcodeGen spec — the .xcodeproj is generated
```

## Build and run

```bash
xcodegen generate
open ReleaseDock.xcodeproj         # scheme ReleaseDock
# or
xcodebuild -project ReleaseDock.xcodeproj -scheme ReleaseDock -derivedDataPath build/DerivedData build
```

Debug builds are ad-hoc signed. Release builds expect a Developer ID identity (see RELEASE_WORKFLOW.md).

Core library only:

```bash
cd Packages/ReleaseDockKit
swift build
swift test --parallel
```

## Configuration

`Config/Base.xcconfig` holds shared values; copy `Config/Local.xcconfig.example` to `Config/Local.xcconfig` for:

| Setting | Purpose |
|---|---|
| `DEVELOPMENT_TEAM` | Developer ID team for Release builds |
| `SPARKLE_FEED_URL` | HTTPS appcast URL (`https:/$()/host/path` — `//` starts a comment in xcconfig) |
| `SPARKLE_PUBLIC_ED_KEY` | Sparkle EdDSA public key |
| `RELEASEDOCK_GITHUB_REPOSITORY` | Default `owner/name` for update notifications |

Runtime environment variables:

| Variable | Effect |
|---|---|
| `RELEASEDOCK_DATA_DIR` | Absolute path used instead of `~/Library/Application Support/ReleaseDock/ReleaseDockData` |
| `RELEASEDOCK_UI_TEST_AUTOPREPARE=1` | Debug builds only: run preparation when a package is opened |

## Conventions

- Swift 6 language mode with complete concurrency checking; services are `Sendable`, the pipeline is an actor, view models are `@MainActor @Observable`.
- Release logic belongs in `ReleaseDockCore`, never in views.
- New behaviour gets a protocol seam if it touches the filesystem, network or processes, so it can be faked in tests.
- External processes only through `ProcessRunner` with argument arrays. Never build shell strings.
- Never log or persist credentials. Use `privacy: .public` only for non-sensitive values.
- Database changes: append a `Migration` with the next version; never edit an existing one. Make new `Codable` fields optional or decode them with `decodeIfPresent` defaults.
- WordPress.org behaviour must cite a source in `docs/research/`; if undocumented, say so in RELEASE_WORKFLOW.md.

## Regenerating the icon

```bash
swift scripts/generate-icon.swift "$PWD"
```
