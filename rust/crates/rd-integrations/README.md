# rd-integrations

The services that talk to external tools and web services: `git`, the product's build command,
`php -l` and WP-CLI Plugin Check, the WordPress.org API, GitHub releases, and the SVN repository setup
(describe a repository, sparse checkout, adopt an existing working copy).

Processes run through `rd-svn`'s shell-free `ProcessRunning` (arguments as a list, minimal
environment, timeout). HTTP uses `reqwest` 0.13 with rustls (aws-lc-rs provider, OS certificate
verifier via `rustls-platform-verifier`, system proxy settings); there is no OpenSSL in the tree.
The stored result types (`BuildResult`, `CodeCheckReport`/`Finding`/`Tool`, `GitRepositoryState`,
`GitHubReleaseResult`, `DirectoryListing`) live in `rd-records` and are re-exported, not duplicated.

## Swift file → Rust module

| Swift (`Packages/ReleaseDockKit/Sources/ReleaseDockCore/`) | Rust (`src/`) |
|---|---|
| `Git/GitService.swift` | `git.rs` (`GitServicing`, `GitService`); `discovery.rs` (`GIT_CANDIDATES`) |
| `Release/BuildService.swift` | `build.rs` (`Building`, `BuildService`, `BuildError` = `ReleaseError.buildFailed`) |
| `CodeChecks/CodeCheckService.swift` | `code_checks.rs` (`CodeCheckConfiguration`, `CodeCheckConfigurationStore`, `CodeChecking`, `CodeCheckService` with `parse_lint` / `parse_plugin_check`); `discovery.rs` (`PHP_CANDIDATES`, `WP_CANDIDATES`) |
| `WordPressOrg/WordPressOrgService.swift` | `wordpress_org.rs` (`WordPressOrgError`, `WordPressOrgServicing`, `WordPressOrgService` with `parse_listing` / `parse_latest_version`, `WordPressBranch`) |
| `GitHub/GitHubService.swift` | `github.rs` (`GitHubRepository`, `GitHubRelease`, `GitHubAsset` (= `GitHubRelease.Asset`), `RateLimitStatus`, `GitHubError`, `ReleaseChannel`, `ReleaseCheckCache`, `GitHubServicing`, `GitHubReleasing`, `CreateRelease`, `GitHubService`) |
| `GitHub/GitHubReleasePublisher.swift` | `github_release_publisher.rs` (`GitHubReleasePublisher`) |
| `Services/RepositorySetupService.swift` | `repository_setup.rs` (`RepositoryDescription`, `WorkingCopyDescription`, `RepositorySetupError`, `RepositorySetupService`) |
| `Release/PluginReleaseStrategy.swift`, `Release/ThemeReleaseStrategy.swift`: `inspectRemote`, `prepareWorkingCopy`, `isVersionDirectory`, `readmeName`; `Release/ReleaseStrategy.swift`: `WorkingCopyOperations.catIfExists` / `localChanges` / `changes` | `repository_layout.rs` (the parts `RepositorySetupService` needs; the release pipeline should call these rather than copy them) |
| `URL` helpers (`lastPathComponent`, `deletingLastPathComponent`, `trimmingTrailingSlash`, `.urlPathAllowed`) | `url_text.rs` (private) |

| Swift tests (`Tests/ReleaseDockCoreTests/`) | Rust (`tests/`) |
|---|---|
| `GitHubServiceTests.swift` (all 5) | `github_service.rs` |
| `GitHubReleaseTests.swift` `GitHubReleaseAPITests` (3), `GitHubReleasePublisherTests` (4), `StubGitHubReleaser` | `github_release.rs` |
| `WordPressOrgTests.swift` `WordPressOrgTests` (3) | `wordpress_org.rs` |
| `DirectoryStatusTests.swift` `testParsesDirectoryListing` | `wordpress_org.rs` (the other pure tests are in `rd-records`) |
| `LiveRepositoryTests.swift` `LiveWordPressOrgAPITests.testRealEndpoints` | `wordpress_org.rs::real_endpoints` (`#[ignore]`) |
| `CodeCheckTests.swift` `CodeCheckParsingTests` (2), `CodeCheckServiceTests` (3) | `code_checks.rs` |
| `BuildTests.swift` `BuildServiceTests` (3) | `build.rs` (`ExportIgnoreTests` are in `rd-inspection`) |
| `RepositorySetupTests.swift` (all 6) | `repository_setup.rs` |
| `LiveRepositoryTests.swift` `testDescribeRealRepositoryAndCheckout` | `repository_setup.rs::describe_real_repository_and_checkout` (`#[ignore]`, same env vars) |
| `Support/TestSupport.swift` `TempDir`, `Fixtures`, `LocalSVNRepository`; `CoreTests.swift` `makeProduct`; `StubURLProtocol` | `common/mod.rs` (`http::MockServer`, a small HTTP/1.1 server on 127.0.0.1) |
| — (Rust-only) | `git.rs` (real `git` in a temp repository), plus extra cases marked "Rust-only" in each file: request paths and queries for WordPress.org, Windows line endings in lint output, lint batching and the Plugin Check command line, missing tools, build timeout, symlinks in the copy, a Windows `cmd.exe` build, local changes, empty repositories |

Tests that need `php`, `git`, `svn`/`svnadmin` skip themselves (print `skipped: …` and pass) when the
tool is missing. Build tests use POSIX shell commands and are `#[cfg(unix)]`; Windows has its own test.
No test reaches the network except the two `#[ignore]`d live tests:

```sh
cargo test -p rd-integrations --test wordpress_org -- --ignored
RELEASEDOCK_LIVE_SLUG=tlp-team cargo test -p rd-integrations --test repository_setup -- --ignored
```

## Platform decisions

- **Discovery** (`discovery.rs`). macOS uses the Swift lists verbatim (`/usr/bin/git`, Homebrew, …).
  Linux checks `/usr/bin`, `/usr/local/bin` and Linuxbrew; Windows checks Git for Windows
  (`C:\Program Files\Git\cmd\git.exe`, …) and `C:\php`, `C:\Program Files\PHP`, `C:\tools\php` for PHP.
  On every platform the program on the parent's `PATH` is tried last (`which`, honouring `PATHEXT`, so
  WP-CLI's `wp.bat` is found on Windows). Swift stops at the fixed list.
- **Install hints.** "PHP is not installed (brew install php)" and "WP-CLI is not installed (brew install
  wp-cli)" are verbatim on macOS; Linux and Windows get their own hint (`sudo apt install php-cli`,
  `winget install PHP.PHP.8.4`, `https://wp-cli.org/#installing`).
- **Build shell.** macOS: `/bin/zsh -l -c <command>` as in Swift. Linux: `/bin/sh -l -c <command>`.
  Windows: `%ComSpec%` (cmd.exe), chosen over PowerShell because `&&` and the `npm`/`composer` `.cmd`
  shims work on every Windows version (Windows PowerShell 5.1 has no `&&`). Windows has no argument
  array and cmd.exe does not understand the `\"` escaping Rust uses, so the command travels in the
  `RELEASEDOCK_BUILD_COMMAND` environment variable and runs as `cmd /D /S /C %RELEASEDOCK_BUILD_COMMAND%`:
  the text reaches cmd unchanged, quotes included. Caveat: `%VAR%` references *inside* the command are
  not expanded a second time. `BuildService::with_shell` with a `powershell`/`pwsh` executable uses
  `-NoProfile -NonInteractive -Command <command>` instead.
- **Build copy.** macOS clones with `/bin/cp -c -R` (APFS `clonefile` per file; the workspace forbids
  `unsafe`, so `clonefile(2)` is not called directly) and falls back to a recursive copy. Linux and
  Windows use the recursive copy, which keeps symbolic links as links like `FileManager.copyItem`.
- **Paths from svn.** Working-copy root comparison resolves `.`/`..` lexically and is case-insensitive on
  Windows (svn reports `C:/…`); local-change paths are made relative with either separator.

## Differences from Swift

- **Async traits.** `GitServicing`, `Building`, `CodeChecking`, `WordPressOrgServicing`,
  `GitHubServicing` and `GitHubReleasing` are `async_trait` traits (usable as `Arc<dyn …>`). Concurrent
  `async let` requests become `tokio::join!` (which keeps the SVN transcript task-local scope).
- **Constructors.** Swift default arguments become builders: `GitService::new().with_executable(…)`,
  `BuildService::new().with_shell(…).with_timeout(…)`, `CodeCheckService::new().with_php(…)`.
  `WordPressOrgService::new()` / `GitHubService::new()` return `Result` (TLS initialisation can fail;
  no `expect` in library code); `with_base_url` points them at a test server. Base URLs end in `/`.
- **`create_release`** takes a `CreateRelease` struct instead of six parameters (clippy's argument limit).
- **Errors.** `GitService::state` returns `ProcessRunnerError` (what Swift rethrows). `BuildError` has
  `BuildFailed` (message identical to `ReleaseError.buildFailed`, including the last 800 characters),
  `Copy` and `Process`. `GitHubError` and `WordPressOrgError` gain `Decoding` (displayed as Foundation's
  "The data couldn’t be read because it isn’t in the correct format."), `Transport` (reqwest's message
  instead of `URLError`'s) and, for GitHub, `File` (unreadable asset). `RepositorySetupError` gains `Svn`
  and `Io`; its `PartialEq` compares those by message.
- **Rate-limit time.** `Date.formatted(time: .shortened)` is approximated as local `h:mm AM/PM`
  (en-US); Swift follows the user's locale.
- **URLs are strings.** `RepositoryDescription.url`, `GitHubRelease.html_url`, asset URLs and
  `RepositorySetupService::repository_url`'s result are `String` (as `Product.svn_repository_url` is).
  Parsing uses the `url` crate, which is more lenient than `URLComponents(string:)` (it percent-encodes
  spaces instead of rejecting them) and lowercases the scheme.
- **Upload URL query.** The asset name is percent-encoded except `A–Z a–z 0–9 - . _ ~`; `URLQueryItem`
  leaves `+` and a few other characters as they are. Same result for release ZIP names.
- **Directory order.** `describe_locally` sorts directory names; `contentsOfDirectory` order is unspecified.
- **Character classes.** `GitHubRepository` accepts `char::is_alphanumeric` (Unicode letters and
  numbers) where Swift uses `CharacterSet.alphanumerics` (which also includes combining marks). The lint
  pattern uses the regex crate with CRLF-aware anchors (`(?mR)`), matching ICU's line handling for `\r\n`.
- **Swift `Duration`** in the build timeout note is formatted with `rd-svn`'s `describe_duration`.
- **Update check** (`latest_release`, `ReleaseCheckCache`, `RateLimitStatus`, Retry-After handling) is
  ported in full because it was cheap and shares the client and error type with release publishing; the
  Tauri app does not need it (its updater replaces it) and may ignore it. `ReleaseCheckCache` was never
  persisted by the Swift app, so it has no JSON compatibility requirement.
- **Authorization header.** Release requests skip `Authorization` for an empty token (the publisher
  never sends one); Swift would send `Bearer `.

## Not ported

| Swift test | Reason |
|---|---|
| `GitHubReleaseTests.swift` `GitHubReleasePipelineTests.testCompletedReleaseCreatesGitHubReleaseWithTheExactPackage` | Needs `ReleasePipelineService`, `Store`, `ReleaseStagingService` (pipeline not ported). The stub's ZIP listing (`zipinfo`) is only used there. |
| `WordPressOrgTests.swift` `TestedUpToFreshnessTests` (7) | Tests `ValidationService` (validation crate). |
| `CodeCheckTests.swift` `CodeCheckValidationTests` (4) | Tests `ValidationService` rules for code-check reports (validation crate). |
| `CodeCheckTests.swift` `CodeCheckPipelineTests.testSyntaxErrorStopsBeforeTheWorkingCopyIsTouched`, `StubCodeChecker` | Pipeline. |
| `BuildTests.swift` `BuildPipelineTests` (2) | Pipeline. |
| `BuildTests.swift` `ExportIgnoreTests` (2) | Already ported in `rd-inspection`. |
| `DirectoryStatusTests.swift` `testPendingThenLiveNotifiesOnce`, `testRefreshAwaitingSkipsUntrackedAndFinishedReleases`, `testWithoutWordPressOrgServiceNothingHappens`, `testNotListedProduct`, `StubWordPressOrg` | Pipeline (`refreshDirectoryStatus`). |
| `LiveRepositoryTests.swift` `LiveDryRunTests.testDryRunAgainstWordPressOrg` | Pipeline. |
| `Support/TestSupport.swift` `StubGit` | Only used by pipeline tests; trivial to write against `GitServicing`. |

Test fixture change: `testBuildsOnACloneAndLeavesTheSourceUntouched` runs `rm -r src` instead of
`rm -rf src` (same effect on the fixture's plain files).
