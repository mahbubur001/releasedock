# rd-pipeline

The release pipeline: the staging service, the SVN side of the plugin and theme strategies, and
`ReleasePipelineService`, which drives a release through the state machine. Everything it needs
already exists in the other crates and is reused, not copied: records, state machine, automation gate
and manifest (`rd-records`), validation and the pure strategy rules (`rd-validation`), the SVN client
and transcript (`rd-svn`), the store and data folder (`rd-store`), credentials (`rd-keychain`), build,
code checks, Git, GitHub and WordPress.org services plus the repository inspection and sparse checkout
(`rd-integrations::repository_layout`), and the commit-message template (`rd-support`).

## Swift file → Rust module

| Swift (`Packages/ReleaseDockKit/Sources/ReleaseDockCore/`) | Rust (`src/`) |
|---|---|
| `Release/ReleasePipelineService.swift` (`PipelineEvent`, `PipelineDependencies`, `ReleasePipelineService`, the `transcribed` extension) | `pipeline.rs` |
| `Release/ReleaseStrategy.swift`: the SVN methods of `ReleaseStrategy`, the readme-update defaults, `StrategyProvider` | `strategy.rs` (`RepositoryStrategy: rd_validation::ReleaseStrategy`, `StrategyProvider`) |
| `Release/ReleaseStrategy.swift`: `WorkingCopyOperations`, `FileMirror` | `working_copy.rs` (`mirror`, `schedule_adds_and_deletes`, `verify_remote_tree`, `working_copy_mismatch`, `differences`, `replace_file`, `file_mirror`; `cat_if_exists` / `local_changes` / `changes` are re-exported from `rd_integrations::repository_layout`) |
| `Release/PluginReleaseStrategy.swift`: `inspectRemote`, `prepareWorkingCopy`, `apply`, `syncAssets`, `assetMimeTypes`, `verify`, readme updates, guidance | `plugin_strategy.rs` (`impl RepositoryStrategy for rd_validation::PluginReleaseStrategy`, `sync_assets`, `ASSET_MIME_TYPES`) |
| `Release/ThemeReleaseStrategy.swift`: `inspectRemote`, `prepareWorkingCopy`, `apply`, `verify`, guidance | `theme_strategy.rs` (`impl RepositoryStrategy for rd_validation::ThemeReleaseStrategy`) |
| `Release/ReleaseStagingService.swift` (`ReleaseError`) | `error.rs` (`ReleaseError`; `rd_integrations::BuildError::BuildFailed` maps onto `ReleaseError::BuildFailed`) |
| `Release/ReleaseStagingService.swift` (`ReleaseStaging`, `ReleaseStagingService`; `StagedCandidate` is in `rd-records`) | `staging.rs` |
| `Services/AppServices.swift` (`NotificationServicing`, `NoopNotificationService`, `UpdateServicing`) | `app_services.rs` |
| `writeReleaseZip` (`/usr/bin/ditto`) | `release_zip.rs` (own deflate ZIP writer) |
| `String.trimmingTrailingSlash`, `URL` / `FileManager` helpers | `fs_util.rs` (private) |
| `Release/CommitMessageTemplate.swift` | already in `rd-support` (`commit_template`) |

The repository inspection (`inspectRemote`) and the sparse checkout (`prepareWorkingCopy`) of both
strategies call `rd_integrations::repository_layout::{inspect_remote, prepare_working_copy}`.

## Tests

| Swift test (`Tests/ReleaseDockCoreTests/`) | Rust test (`tests/`) |
|---|---|
| `PipelineIntegrationTests.swift` (all 33) | `pipeline_integration.rs`, same names in snake_case |
| `CommitTemplateTests.swift` `testTemplateBecomesTheSummaryAndKeepsTheMarker`, `testInvalidTemplateIsRefusedBeforeAnythingRuns` | `pipeline_integration.rs` |
| `TranscriptTests.swift` `testReleaseRecordsItsSVNCommandsPerOperation` | `pipeline_integration.rs` |
| `RestoreReleaseTests.swift` (all 4) | `restore_release.rs` |
| `ReadmeUpdateTests.swift` (the 8 pipeline tests) | `readme_update.rs` |
| `DirectoryStatusTests.swift` `testPendingThenLiveNotifiesOnce`, `testRefreshAwaitingSkipsUntrackedAndFinishedReleases`, `testWithoutWordPressOrgServiceNothingHappens`, `testNotListedProduct` | `directory_status.rs` (no svn needed) |
| `BuildTests.swift` `BuildPipelineTests` (2) | `build_pipeline.rs` (`folder_release_ships_the_built_copy` is `#[cfg(unix)]`: its command is POSIX shell) |
| `CodeCheckTests.swift` `CodeCheckPipelineTests.testSyntaxErrorStopsBeforeTheWorkingCopyIsTouched` | `code_check_pipeline.rs` |
| `GitHubReleaseTests.swift` `GitHubReleasePipelineTests.testCompletedReleaseCreatesGitHubReleaseWithTheExactPackage` | `github_release_pipeline.rs` (the stub lists the ZIP with `rd_inspection::ZipArchive` instead of `zipinfo`) |
| `DemoDataTests.swift` `testMakeDemoData` | `demo_data.rs` (opt-in: `RELEASEDOCK_MAKE_DEMO=/tmp/rd-demo`) |
| `LiveRepositoryTests.swift` `LiveDryRunTests.testDryRunAgainstWordPressOrg` | `live_repository.rs` (`#[ignore]`, same env vars) |
| `PerformanceTests.swift` (the `ReleaseStagingService.buildCandidate` step) | `staging.rs::large_candidate_build` (opt-in: `RELEASEDOCK_PERF_TESTS=1`) |
| `Support/TestSupport.swift` `TempDir`, `Fixtures`, `LocalSVNRepository`, `FaultInjectingSVN`, `RecordingNotifications`, `StubGit`; `StubWordPressOrg`, `StubCodeChecker`, `StubGitHubReleaser`; the `setUp` of `PipelineIntegrationTests` | `common/mod.rs` (`Harness`); `zip_folder` replaces `/usr/bin/zip` (from rd-inspection's tests) |

Tests that need `svn`/`svnadmin` print `skipped: svn/svnadmin not installed` and pass when either is
missing (Windows CI). They use local `file://` repositories only; nothing reaches the network except the
ignored live test:

```sh
RELEASEDOCK_LIVE_DRYRUN_SLUG=tlp-team RELEASEDOCK_LIVE_DRYRUN_VERSION=6.0.8 \
  cargo test -p rd-pipeline --test live_repository -- --ignored --nocapture
```

Rust-only tests: every `ReleaseError` message (`staging.rs`), the `BuildError` mapping, candidate hash
and symlink checks, backup location and mode, the release ZIP round trip (our ZIP reader, and `unzip -t`
where installed), `operations_are_send` (the futures run on a multi-threaded runtime),
`a_busy_product_refuses_a_second_operation`, `events_are_broadcast`, and unit tests for the DOS date
conversion and the path/URL helpers.

### Not ported

- `ReadmeUpdateTests.testAutomationGateRequiresTheReleasedTag`: pure `AutomationGate` test, already in
  rd-records (`tests/automation_gate.rs`).
- `DirectoryStatusTests` `testParsesDirectoryListing`, `testStates`, `testOnlyCommittedWordPressOrgVersionReleasesAreTracked`,
  `testAwaitingWindowAndFinalStates`: already in rd-integrations / rd-records.
- `CommitTemplateTests` `testRendersTokens`, `testRejectsEmptyLongAndUnknownTokens`: already in rd-support.
- `TranscriptTests` other than the pipeline test: already in rd-svn.
- The non-pipeline tests of `BuildTests`, `CodeCheckTests`, `GitHubReleaseTests`, `LiveRepositoryTests`: already in
  rd-integrations, rd-inspection, rd-svn.

## Behaviour differences from Swift

Safety-relevant behaviour (single commit, fingerprint approval, commit marker + log lookup, recovery on
an uncertain outcome, reconciliation after a restart, backup before revert, SHA-256 verification,
automation gate, dry run) is ported line by line. Where the mechanics differ:

- **Actor → struct.** `ReleasePipelineService` is `Send + Sync`. `withProductLock` is a mutex-guarded
  set of busy product ids with a drop guard: a second operation on a busy product fails at once with
  "Another operation is already running for this product.", as in Swift; the guard is also released when
  an operation's future is dropped. Swift's actor additionally runs every synchronous stretch atomically;
  Rust runs non-locked operations (`create_release`, `reconcile_interrupted_releases`,
  `refresh_directory_status`, `publish_github_release`, `working_copy_state`) truly in parallel. To keep
  the stale-state guard of `move` exact, `move_to` (check stored state → transition → save → emit) holds a
  mutex, so two transitions of one release can never both pass the check. As in Swift, the app should
  call `reconcile_interrupted_releases` at launch before anything else.
- **Events.** `events()` returns a `tokio::sync::broadcast::Receiver` (capacity 100). Swift's
  `.bufferingNewest(100)` drops the oldest silently; a lagging Rust receiver gets `RecvError::Lagged(n)`
  once and then continues with the newest events. Logging uses the `log` crate (target `release`).
- **Transcript.** Every operation Swift wraps in `transcribed` runs in `SvnTranscript::with_scope` with
  the same operation names; concurrent SVN requests use `tokio::join!`, which keeps the task-local scope.
- **Defaults.** Swift default arguments are explicit: `create_release(product_id, source, mode: Option,
  kind, commit_template: Option)`, `create_restore_release(…, mode: Option)`,
  `refresh_awaiting_directory_releases(now)`. `PipelineDependencies::new` takes the six required services
  and `with_*` builders replace the optional ones. `create_release` is synchronous (it has no await in
  Swift either). `credentials`, `authentication_availability`, `gather_snapshot`, `commit_message`,
  `build_preview` and `find_committed_revision` are `pub` (internal in Swift, used by tests).
- **`stagingPath`.** Swift records it only when `deps.staging is ReleaseStagingService`; Rust asks the
  staging (`ReleaseStaging::recorded_staging_path`, default `None`, implemented by `ReleaseStagingService`).
- **Strategies.** `RepositoryStrategy` is implemented for `rd_validation`'s strategy types. Their
  `inspect_remote` / `prepare_working_copy` call `rd_integrations::repository_layout`, which switches on
  `product.product_type` rather than on the strategy; through `StrategyProvider` (which picks the strategy by
  the product's type) this is the same. Errors are `ReleaseError` (svn errors wrapped in `ReleaseError::Svn`).
- **Asset MIME types** are set in a fixed order (png, jpg, jpeg, gif, svg); Swift iterates a dictionary.
- **Release ZIP.** Swift runs `/usr/bin/ditto -c -k --norsrc --noextattr --noqtn --keepParent` on a
  clone of the candidate. Rust writes the ZIP itself (`release_zip.rs`) straight from the candidate:
  `<slug>/` root, directory entries, deflate, UTF-8 names, Unix modes, UTC DOS times, no ZIP64 (a file or
  archive over 4 GB fails and, as in Swift, the release keeps no ZIP). The file list and contents are the
  same; the archive bytes are not identical to ditto's.
- **Backups.** Entries are copied with `rd_integrations::BuildService::clone_tree` (APFS clone on macOS,
  recursive copy elsewhere; symlinks stay links) instead of `FileManager.copyItem`. As with `copyItem`, an
  existing destination is never overwritten: the backup fails and nothing is reverted.
- **Paths from svn.** svn reports paths as given; on Windows the working-copy prefix is matched
  case-insensitively with either separator, and preview paths are always `/`-separated, so the preview
  fingerprint is the same on every platform. The fingerprint lines are sorted by UTF-8 bytes; Swift sorts
  `String`s, which gives the same order for ASCII and NFC-normalized names, so an approval made in the
  Swift app is honoured by the Rust app for the same working copy.
- **Diff excerpt** is truncated at 200 000 `char`s; Swift counts grapheme clusters. Only the displayed
  excerpt can differ; the fingerprint does not include the diff.
- **Error text.** `ReleaseError` messages are verbatim. Wrapped errors (svn, store, inspection, I/O…) show
  the Rust crates' messages; file-system errors are `"<path>: <io error>"` instead of Cocoa's text.
  `localizedDescription` in notes and errors is `to_string()`.
- **`UpdateServicing`** (a `@MainActor` class protocol with settable properties) is a plain trait with
  getters and setters on `&mut self`; `NotificationServicing` is an `async_trait`.
- Observed and kept as in Swift (not changed by the port): after a restart, `reconcile_interrupted_releases`
  and `fail_and_reset` clear `working_copy_modified` even when the reset itself failed (`try?`); the working
  copy then still holds the staged files until the user reverts it.
