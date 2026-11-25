# Architecture

## Overview

```
ReleaseDock.app (SwiftUI, App/)                 ReleaseDockKit (Swift package, Packages/ReleaseDockKit)
┌───────────────────────────────┐               ┌──────────────────────────────────────────────┐
│ Views  ─► ViewModels ─► AppModel ─────────────►│ ReleasePipelineService (actor)                │
│ (Dashboard, Products, Wizard, │  composition  │   ├─ ZIPInspectionService / FolderInspection  │
│  History; Settings, Update)   │     root      │   ├─ ProductDetector + VersionService          │
│                               │               │   ├─ ValidationService (pure)                 │
│ Platform/                     │               │   ├─ ReleaseStagingService                    │
│  SparkleUpdateService ──────► Sparkle 2       │   ├─ Plugin/ThemeReleaseStrategy              │
│  UserNotificationService      │               │   └─ SVNService ─► ProcessRunner ─► svn CLI   │
│  AppUpdateChecker ────────────────────────────►│ GitHubService (URLSession)                    │
└───────────────────────────────┘               │ KeychainService (Security.framework)          │
                                                │ Store (SQLite, migrations, event log)         │
                                                │ GitService, DiagnosticsService, ProductService│
                                                └──────────────────────────────────────────────┘
```

All release logic lives in `ReleaseDockCore`, which has no SwiftUI dependency and is fully testable with `swift test`. The app target only wires services together (`AppModel`) and renders state. Every service is a protocol (`SVNServicing`, `ZIPInspecting`, `ValidationServicing`, `KeychainServicing`, `GitHubServicing`, `NotificationServicing`, `ProductRepository`, …) injected through `PipelineDependencies`, so tests substitute fakes (e.g. `FaultInjectingSVN`, `InMemoryKeychain`, `StubURLProtocol`).

## Modules (`Sources/ReleaseDockCore`)

| Folder | Responsibility |
|---|---|
| `Support/` | `ProcessRunner` (no shell, timeout, cancellation, stdin), `PathSafety`, hashing/CRC-32, `AppPaths`, `Log` + `SecretRedactor`, XML DOM |
| `Models/` | `Product`, `ReleaseRecord`, `ReleaseState`, validation and inspection models, `RepositorySnapshot` |
| `Inspection/` | Own ZIP reader (central directory, ZIP64, stored/deflate via Compression.framework), ZIP and folder inspection, `FileClassifier` + glob patterns |
| `Versioning/` | `PHPVersion` (`version_compare()` port), `VersionService`, `WordPressHeaders` (`get_file_data()`), `ReadmeParser`, `ProductDetector` |
| `SVN/` | `SVNService` over the svn CLI (`--xml` parsing, error-code classification, `@` peg escaping, conservative commit outcome) |
| `Validation/` | `ValidationService`: a pure function of `ValidationContext` |
| `Release/` | State machine, strategies, staging, manifest, `AutomationGate`, `ReleasePipelineService` |
| `Persistence/` | `SQLiteDatabase` wrapper, `Migrator` (user_version, backup before migrating), `Store` |
| `Keychain/`, `Git/`, `GitHub/`, `Services/` | Credentials, read-only Git inspection, release checks, product CRUD/validation, diagnostics |

## Release state machine

```
Detected → Inspected → Validated → Staged → Previewed → Approved → Publishing → Verifying → Completed
    │          │           │          │         │  ▲  │       │   │         │  ▲       │
    └──────────┴───────────┴──────────┴─────────┤  └──┘       │   │         │  │       └─► RecoveryRequired
                       Failed / Cancelled ◄─────┘ (re-approval│   │         │  │                 │
                                                  after change)   │         │  └── commit found ─┤
                                       Publishing ─► Previewed (credentials rejected; nothing committed)
                                       Publishing ─► Failed   (rejected before commit: hook, out of date…)
                                       Publishing ─► RecoveryRequired (outcome uncertain)
                       RecoveryRequired ─► Verifying | Failed | Approved (only with explicit user approval
                                                                  after proving the commit is absent)
```

- Transitions are validated (`ReleaseStateMachine`) and every transition is persisted, with a note, *before* the next side effect. The `release_events` table is an append-only log.
- A dry run ends `Previewed → Completed` and can never be approved.
- `Publishing` cannot be cancelled. After an app restart, releases in `Publishing`/`Verifying` become `RecoveryRequired`; earlier states become `Cancelled` and their working-copy changes are reverted (with a backup).

## Pipeline steps

1. **Inspect** – ZIP: inspect the central directory without writing, then extract only included entries into `Staging/<id>/extracted` (exclusive creates, no symlinks, re-validated paths, CRC + size enforced). Folder: walk read-only and hash every included file.
2. **Detect** – `ProductDetector` reads headers from the extracted copy or the folder.
3. **Snapshot** – the strategy reads the remote (structure, released version, existing tags/version dirs); the working copy is checked for URL mismatch and local changes, then (only if clean) checked out sparsely or updated; `svn status --show-updates` gives freshness. Failures become part of the snapshot rather than exceptions, so validation reports them precisely.
4. **Validate** – pure `ValidationService`. Blocking results end the release in `Failed`.
5. **Stage** – copy included files into `Staging/<id>/candidate`, verifying each SHA-256 against inspection (detects changes between inspect and stage); re-detect on the candidate; the strategy mirrors it into the working copy, schedules adds/deletes from `svn status --no-ignore`, and creates the release copy.
6. **Preview** – changes from `svn status`, text diff, commit message (contains `ReleaseDock-Release-ID: <uuid>`), and a fingerprint over every changed path, its content hash, the candidate digest and the message. Validation re-runs with the release-diff check.
7. **Approve** – binds the approval to the fingerprint. Automatic approval goes through `AutomationGate`.
8. **Publish** – re-fingerprint (changed → back to `Previewed`), `status -u` (remote changed → fail and restore), tag absent (or already ours → recover), credentials resolved, `revisionBefore` recorded, `Publishing` persisted, one `svn commit`.
9. **Verify** – tag/version directory exists, file list equals the candidate, exported files' SHA-256 match, trunk Stable tag (plugins). Then the working copy is updated, the product's current version and last release are recorded, the manifest finalized, and a notification sent.

## Plugin vs theme strategies

| | `PluginReleaseStrategy` | `ThemeReleaseStrategy` |
|---|---|---|
| Layout | `/<slug>/{trunk,tags,assets}` | `/<slug>/<version>/` only |
| Working copy | `--depth immediates`, `trunk` infinity, `tags` immediates, `assets` infinity when syncing | `--depth immediates`, previous version dir infinity |
| Apply | mirror candidate → `trunk`, optional assets mirror + `svn:mime-type`, `svn cp trunk tags/<v>` | `svn cp <previous> <v>`, mirror candidate → `<v>` |
| Allowed changes | `trunk/`, `tags/<v>/`, `assets/` | `<v>/` |
| Specific rules | Stable tag must equal version; readme rules; assets don't belong in trunk | numeric-only version; style.css importer headers; screenshot, readme.txt, index.php or templates/index.html; theme-forbidden files; first version must go through the upload form |

## Persistence and data layout

```
~/Library/Application Support/ReleaseDock/ReleaseDockData/   (0700; override: RELEASEDOCK_DATA_DIR)
├── Products/<type>-<slug>/svn   default SVN working copies
├── Staging/<release-id>/        extracted/, candidate/, verification scratch (removed after release)
├── Releases/<slug>/<v>-<id>/    manifest.json
├── Backups/                     DB copies before migrations, working-copy diffs before reverts/commits
├── Logs/                        diagnostic exports
└── Database/releasedock.sqlite  (0600, WAL, synchronous=FULL)
```

Tables: `products`, `releases` (JSON payload + indexed columns), `release_events` (append-only), `settings`. Schema changes are added as new `Migration`s; a database written by a newer app version is refused instead of being modified.

## Concurrency

`ReleasePipelineService` is an actor; a per-product lock rejects concurrent operations on the same working copy (actor reentrancy across `await` would otherwise allow it). `ProcessRunner` reads stdout/stderr concurrently, observes exit via `terminationHandler`, and terminates (SIGTERM, then SIGKILL) on timeout or task cancellation. The app's view models are `@MainActor @Observable`.

## Self-update

- `AppUpdateChecker` queries GitHub (`/releases/latest`, or `/releases` for beta) on launch and on demand, caches the result with its ETag in settings, respects rate limits, and notifies once per new version.
- `SparkleUpdateService` wraps `SPUStandardUpdaterController`. It is started only if Info.plist contains an HTTPS `SUFeedURL` and an `SUPublicEDKey`; otherwise in-app installation is disabled and the reason is shown. Beta builds are filtered with Sparkle channels (`allowedChannels(for:)`).
