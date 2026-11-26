# rd-svn

Subversion support for ReleaseDock: a shell-free process runner, a small XML tree for `svn --xml`
output, the SVN models and service, and the per-product command transcript.

## Swift file → Rust module

| Swift (`Packages/ReleaseDockKit/Sources/ReleaseDockCore/`) | Rust (`src/`) |
|---|---|
| `Support/ProcessRunner.swift` | `process_runner.rs` (`ProcessRequest`, `ProcessResult`, `ProcessRunnerError`, `ProcessRunning`, `ProcessRunner`, `with_cancellation`) |
| `Support/XMLTree.swift` | `xml_tree.rs` (`XmlElement`, parsed with `roxmltree`) |
| `SVN/SVNModels.swift` | `models.rs` (`SvnInstallation`, `SvnCredentials`, `SvnInfo`, `SvnStatusEntry`, `SvnErrorKind::classify`, `SvnError`, …) |
| `SVN/SVNService.swift` | `service.rs` (`SvnServicing` trait, `SvnService`) |
| `SVN/SVNTranscript.swift` | `transcript.rs` (`TranscriptScope`, `TranscriptEntry`, `SvnTranscribing`, `SvnTranscript`); `iso8601.rs` for the `.iso8601` date format |

| Swift tests (`Tests/ReleaseDockCoreTests/`) | Rust (`tests/`) |
|---|---|
| `CoreTests.swift`: `ProcessRunnerTests`, `SVNParsingTests`, `SVNTimeoutTests`, `RecordingRunner` | `core.rs`, `support/mod.rs` |
| `TranscriptTests.swift` | `transcript.rs` |
| `LiveRepositoryTests.swift` (SVN-level part) | `live_repository.rs` (`#[ignore]`, needs the same env vars) |
| `Support/TestSupport.swift`: `TempDir`, `Fixtures` (plugin/theme), `LocalSVNRepository` | `support/mod.rs` |
| — (Rust-only integration tests on a `file://` repository) | `local_repository.rs` |

Tests that need `svn`/`svnadmin` skip themselves (print `skipped: …` and pass) when either is missing.
Unix-only process tests (`/bin/echo`, `/bin/cat`, `/bin/sleep`, `/bin/sh`) are `#[cfg(unix)]`.

## Differences from Swift

- **Async API.** `SvnServicing` and `ProcessRunning` are `async_trait` traits (object-safe, so
  `Arc<dyn SvnServicing>` works like `any SVNServicing`). Every method returns `Result<_, SvnError>`.
  `SvnError` has two extra variants for what Swift rethrows untyped: `Process(ProcessRunnerError)`
  (launch failures, timeouts, cancellation) and `Io` (temporary message/targets files).
- **Cancellation.** Swift task cancellation becomes a `CancellationToken` set for a task with
  `with_cancellation(token, future)`; cancelling it terminates the child and returns
  `ProcessRunnerError::Cancelled(partial)`, exactly like the Swift `onCancel`. Dropping the future
  also stops the child (`kill_on_drop`, an immediate SIGKILL), but then there is no partial result.
- **Termination.** SIGTERM, then SIGKILL after `kill_grace_period` (via `nix` on Unix). On Windows
  both steps are `TerminateProcess`.
- **Transcript scope.** `@TaskLocal SVNTranscript.scope` becomes a `tokio::task_local!`:
  `SvnTranscript::with_scope(scope, future)` / `SvnTranscript::scope()`. Like Swift task-locals it
  is not inherited by `tokio::spawn`ed tasks (Swift's `async let` children do inherit it; use
  `tokio::join!` inside the scope instead of spawning).
- **Transcript file format** is unchanged: `<PRODUCT-UUID>.jsonl`, one JSON object per line, keys
  `id, date, productID, releaseID?, operation, command, exitCode?, duration, output, failure?`,
  dates as ISO 8601 strings without fractional seconds (`2026-11-26T09:30:15Z`, what
  `JSONEncoder.dateEncodingStrategy = .iso8601` writes), uppercase UUIDs. Tests read a line written in
  the Swift format (including `\/` escapes) and check the Rust output.
- **svn discovery.** Unix candidates are the Swift list (`/opt/homebrew/bin`, `/usr/local/bin`,
  `/opt/local/bin`, `/usr/bin` — the last covers Linux). Windows candidates are the TortoiseSVN,
  SlikSVN and CollabNet install folders. On every platform `svn`/`svn.exe` on the parent process's
  `PATH` is tried last (`which` crate); Swift stops at the fixed list.
- **Child environment.** Same minimal environment (`PATH`, `HOME`, `LANG`, `LC_ALL`). On Windows,
  `PATH`, `PATHEXT`, `SystemRoot`, `SystemDrive`, `windir`, `TEMP`, `TMP`, `USERPROFILE`, `APPDATA`,
  `LOCALAPPDATA` and `ComSpec` are passed through instead of the Unix `PATH`, because Windows
  programs cannot start without them and svn keeps its config under `%APPDATA%`.
- **"svn is not installed" message.** Verbatim on macOS; Linux and Windows get their own install hint
  instead of Homebrew.
- **`supports_password_from_stdin`** compares dotted numeric versions instead of calling PHP
  `version_compare` (that lives in `rd-versioning`, which this crate does not depend on). For
  `svn --version --quiet` output (`1.14.5`) the result is the same.
- **`ByteCountFormatter`** (transcript size summaries such as `(2 KB of output)`) is reimplemented
  for `.file` style and English output; the tests check it against values printed by Foundation.
- **XML.** `roxmltree` puts CDATA sections in the element text; Swift's `XMLParser` delegate ignored
  them. `svn --xml` never emits CDATA.
- **Duration text.** `Timed out after 0.301 seconds: …` and `timed out after … seconds` use the same
  formatting as Swift's `Duration.description`.
- **Logging** uses the `log` crate (target `svn`) in place of `os.Logger`.
- `ProcessRequest` and `SvnCredentials` have hand-written `Debug` impls that never print stdin or the
  password.

## Not ported

| Swift test | Reason |
|---|---|
| `TranscriptTests.swift`: `PipelineIntegrationTests.testReleaseRecordsItsSVNCommandsPerOperation` | Needs the release pipeline (`ReleasePipelineService`), not ported yet. `local_repository.rs::transcript_records_real_commands_in_scope` covers the SVN side. |
| `RepositorySetupTests.swift` (all 6 tests) | Tests `RepositorySetupService` (Services/), not SVN-level. Port with that service; it can reuse `support::LocalSvnRepository`. |
| `LiveRepositoryTests.swift`: full `testDescribeRealRepositoryAndCheckout` | Goes through `RepositorySetupService.describeRemote` / `describeWorkingCopyLocally`. The svn calls it makes are in `live_repository.rs::describe_real_repository_and_checkout` (ignored). |
| `LiveRepositoryTests.swift`: `LiveDryRunTests.testDryRunAgainstWordPressOrg` | Needs `ProductDetector`, `Store` and the pipeline. Its first step (export trunk) is `live_repository.rs::export_trunk_from_word_press_org` (ignored). |
| `LiveRepositoryTests.swift`: `LiveWordPressOrgAPITests.testRealEndpoints` | WordPress.org HTTP API, not SVN; belongs to the WordPress.org crate. |
| `CoreTests.swift`: state machine, store, manifest, product service and keychain tests | Not SVN; they belong to later crates. |
| `TestSupport.swift`: `FaultInjectingSVN`, `RecordingNotifications`, `StubGit`, `TestZIPBuilder` | Used only by pipeline, notification, git and ZIP tests; port with those. `FaultInjectingSVN` can wrap `Arc<dyn SvnServicing>` directly. |
