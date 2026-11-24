# Testing

```bash
scripts/test.sh                                   # everything + app build
cd Packages/ReleaseDockKit && swift test --parallel
swift test --filter PipelineIntegrationTests      # one suite
RELEASEDOCK_KEYCHAIN_TESTS=1 swift test --filter KeychainTests   # real login-keychain round trip
RELEASEDOCK_PERF_TESTS=1 swift test --filter PerformanceTests     # 5,000-file package timings
# Read-only checks against WordPress.org (never commit):
RELEASEDOCK_LIVE_SLUG=tlp-team RELEASEDOCK_LIVE_WC=~/svn/tlp-team swift test --filter LiveRepositoryTests
RELEASEDOCK_LIVE_DRYRUN_SLUG=tlp-team RELEASEDOCK_LIVE_DRYRUN_VERSION=6.0.8 swift test --filter LiveDryRunTests
```

Current result: **205 tests, 0 failures** (the keychain, performance and live WordPress.org tests are skipped unless opted in). Integration tests need `svn` and `svnadmin` (Homebrew `subversion`); they are skipped otherwise. Version tests compare against the real `php` binary when installed.

**No test uses WordPress.org or any real credentials.** Integration tests create throwaway repositories with `svnadmin create` and talk to them over `file://`.

## Unit tests

| Area | File | Highlights |
|---|---|---|
| Version comparison | `VersionTests.swift` | `version_compare()` table tests; **differential test of 1,024 version pairs against real PHP**; validity rules (leading `v`, unknown words, `pre`, capitalised `Beta`); same/older/newer/review |
| Headers & readme | `VersionTests.swift` | `get_file_data()` regex parity (verified against PHP), 8 KiB limit, CR line endings; readme aliases, BOM, Stable tag sanitisation, changelog matching |
| Path safety | `InspectionTests.swift` | traversal, absolute, drive letters, backslashes, control chars, long components; symlink-aware containment; slugs; secret redaction |
| ZIP inspection | `InspectionTests.swift` | real deflate archives; crafted archives for traversal, absolute paths, symlinks, duplicates/case conflicts, file-vs-directory, nested root, `__MACOSX`/`.DS_Store`/`.git`, forbidden and secret files, non-UTF-8/control names, encrypted entries, local/central name mismatch, invalid ZIP, size limits, archive changed after inspection |
| Folder inspection | `InspectionTests.swift` | read-only guarantee, exclusions, `.distignore`, user patterns, symlinks, zero-byte files, hashes |
| Product detection | `InspectionTests.swift` | plugin/theme detection, ambiguous main file, version never from file name |
| Validation | `ValidationTests.swift` | each required check; theme rules; warnings as blocking; optional checks; release-diff rules; automation gate |
| State machine | `CoreTests.swift` | happy path, invalid transitions, dry run never publishes, interruption mapping, note redaction |
| Persistence | `CoreTests.swift` | products/releases/events, idempotent event log, cascade, migrations, newer-schema refusal, file permissions |
| Manifest | `CoreTests.swift` | contents and absence of credentials |
| Process runner | `CoreTests.swift` | no shell interpretation, stdin, timeout, cancellation |
| SVN parsing | `CoreTests.swift` | peg escaping, error classification, committed revision, status XML |
| GitHub | `GitHubServiceTests.swift` | headers, ETag/304, beta channel, rate limit, 404/401, repository parsing |

## Integration tests (`PipelineIntegrationTests.swift`, real svn)

| Scenario | Expected |
|---|---|
| Assisted plugin release from ZIP (with assets, `logo@2x.png`, junk) | Completed; tag contents and MIME props correct; manifest, events, notification; WC clean; staging removed |
| Folder release with deleted/new files | Deletions propagate; source folder byte-identical before/after |
| Dry run | Completed, no commit, WC restored, cannot be approved |
| Same version / older version | Failed at validation |
| Invalid ZIP | Failed (source integrity) |
| Existing tag | Failed (existing tag) |
| Dirty working copy | Failed; local edit untouched; explicit revert backs up diff and unversioned files |
| Remote change after approval | Publishing refused, release failed, nothing committed |
| Preview tampered after approval | Back to Previewed, no commit |
| Authentication failure | Nothing committed, back to Previewed, retry succeeds |
| Hook rejection | Failed, WC restored |
| Commit timeout after the server committed | RecoveryRequired → found by release ID → Completed; exactly one commit |
| Commit timeout before reaching server | RecoveryRequired, verified not committed; automatic retry refused; explicit retry completes |
| Network failure during commit | Treated as uncertain; abandon restores WC |
| Network failure during validation | Failed (connectivity) |
| Duplicate release | Second attempt blocked; re-publishing a completed release refused |
| App restart during publishing (commit landed) | Reconciled to RecoveryRequired, recovered to Completed |
| App restart before publishing | Cancelled, WC restored |
| Automatic mode | Refused without explicit opt-in; publishes once enabled |
| First release into an empty plugin repository | Completed |
| Revert with binary and newly added files | Every file copied to `Backups/` before anything is deleted |
| Ignored files in the working copy | Block the release (OS junk excepted) |
| Working copy edited after approval (released tag modified) | Re-validated, fails release-diff, nothing committed |
| Retry after "verified not committed", then uncertain again with recovery offline | Flag cleared, abandon refused, recovery completes |
| Crafted ZIP64 offsets near `Int.max` | Rejected as corrupt, no crash |
| Add product: check out from server / adopt existing working copy (`RepositorySetupTests`) | Type, slug, name and version detected; sparse checkout; trunk URLs, non-roots, missing repos and non-empty folders rejected |
| File ↔ directory type changes between versions | Replaced with `svn delete` + add, published and verified |
| Theme release | `svn cp` from previous version (history verified), deletions applied, only `<version>/` changed |
| Theme without any released version | Blocked (initial upload must use the review form) |

Fault injection uses `FaultInjectingSVN`, which wraps the real `SVNService` and replaces only the commit (or `info`) behaviour.

## Manual UI verification

The app can be run against an isolated data directory:

```bash
RELEASEDOCK_DATA_DIR=/tmp/rd-ui/data RELEASEDOCK_UI_TEST_AUTOPREPARE=1 \
  build/DerivedData/Build/Products/Debug/ReleaseDock.app/Contents/MacOS/ReleaseDock &
open -a build/DerivedData/Build/Products/Debug/ReleaseDock.app package.zip
```

`RELEASEDOCK_UI_TEST_AUTOPREPARE` exists only in Debug builds and only runs preparation (never approval or publishing) when a package is opened. This was used to verify the Dashboard and the Wizard through *Previewed* against a local `file://` repository, and restart reconciliation (a release left in Previewed was cancelled and its working copy restored on relaunch).

## Debug-only verification hooks

Debug builds read these environment variables (none exist in Release builds):

| Variable | Effect |
|---|---|
| `RELEASEDOCK_DATA_DIR` | Use an isolated data folder (also honoured in Release) |
| `RELEASEDOCK_UI_TEST_SECTION` | Open a section (`dashboard` (Overview), `products`, `wizard` (New Release), `history`, `updates`, `settings`) |
| `RELEASEDOCK_UI_TEST_HISTORY` | Select the History entry at this index |
| `RELEASEDOCK_UI_TEST_RESTORE` | `1` opens Restore Older Version for the selected product; `auto` also prepares the suggested restore |
| `RELEASEDOCK_UI_TEST_OPEN` | Open a ZIP or folder in the wizard at launch |
| `RELEASEDOCK_UI_TEST_AUTOPREPARE=1` | Run preparation when a package is opened |
| `RELEASEDOCK_UI_TEST_AUTOPUBLISH=1` | Then approve and publish — refused unless the repository is a local `file://` URL |
| `RELEASEDOCK_UI_TEST_EXISTING_WC` | Open Add Product → Use existing working copy with a folder |
| `RELEASEDOCK_UI_DUMP_VIEWS` | Write the AppKit view hierarchy to a file |

Demo data for screenshots and manual checks: `RELEASEDOCK_MAKE_DEMO=/tmp/rd-demo swift test --filter DemoDataTests` (in `Packages/ReleaseDockKit`) builds local repositories, two products and a release history; launch the Debug app with `RELEASEDOCK_DATA_DIR=/tmp/rd-demo/data`.

## Live checks against WordPress.org

`LiveDryRunTests` exports a real plugin's trunk to a temporary folder, bumps the version there and
runs the whole pipeline in Dry Run mode with an isolated data folder and a fresh checkout. On
2026-11-22 against `tlp-team` it completed in 26 s: the current version was read from the
trunk `README.txt` Stable tag (6.0.7), every required check passed, and the preview contained
exactly `tags/6.0.8` (copy) plus the version changes in `README.txt` and `tlp-team.php`.

## Not covered by automated tests

- A real commit to WordPress.org (hook behaviour, Release Confirmation, credential prompts).
- Sparkle installation end to end (requires a signed, notarized build and a hosted appcast).
- SwiftUI views (no XCUITest target yet; see IMPLEMENTATION_PLAN.md).
