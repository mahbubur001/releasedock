# rd-store

SQLite persistence shared with the Swift app, the data folder layout, product validation/CRUD and
the diagnostics bundle. The store opens the Swift app's `releasedock.sqlite` unchanged and writes
rows the Swift app reads back byte for byte.

## Swift file → Rust module

| Swift (`Packages/ReleaseDockKit/Sources/ReleaseDockCore/`) | Rust (`src/`) |
|---|---|
| `Persistence/SQLiteDatabase.swift` | `database.rs` (`DatabaseError`, `SqlValue` (= `SQLValue`), `SqliteDatabase`, `Executor`) |
| `Persistence/Migrations.swift` | `migrations.rs` (`Migration`, `Migrator`, `INITIAL_SCHEMA`) |
| `Persistence/Store.swift` | `store.rs` (traits `ProductRepository`, `ReleaseRepository`, `SettingsRepository`; `StoreError`; `Store`; `encode`) and `swift_json.rs` (Foundation's JSON text) |
| `Support/AppPaths.swift` | `app_paths.rs` (`AppPaths`, `AppPathsError`, `DATA_DIR_VARIABLE`) |
| `Services/ProductService.swift` | `product_service.rs` (`ProductField` (= `ProductValidationIssue.Field`), `ProductValidationIssue`, `ProductSuggestion`, `ProductServicing`, `ProductServiceError`, `ProductService`) |
| `Services/DiagnosticsService.swift` | `diagnostics.rs` (`DiagnosticsService`, `DiagnosticsError`, `LogSource`, `LogEntry`) |

| Swift tests (`Tests/ReleaseDockCoreTests/`) | Rust (`tests/`) |
|---|---|
| `CoreTests.swift` `StoreTests.testProductsReleasesAndEvents` | `store.rs` `products_releases_and_events` |
| `CoreTests.swift` `StoreTests.testMigrationsBackupAndRefuseNewerSchema` | `store.rs` `migrations_backup_and_refuse_newer_schema` |
| `CoreTests.swift` `StoreTests.testDatabaseFileIsPrivate` | `store.rs` `database_file_is_private` (Unix only) |
| `CoreTests.swift` `ProductServiceTests.testValidation` | `product_service.rs` `validation` |
| `CoreTests.swift` `ProductServiceTests.testSuggestionFromFolder` | `product_service.rs` `suggestion_from_folder` |
| `CoreTests.swift` `makeProduct`, `Support/TestSupport.swift` `TempDir`, `Fixtures.makePlugin` | `common/mod.rs` |

Added (no Swift equivalent): `store.rs` upgrade backup naming, rollback of a failed migration,
WAL/foreign keys/synchronous PRAGMAs, settings JSON, corrupt rows; `product_service.rs` every
validation message verbatim and in order, save/delete; `app_paths.rs` layout, override, platform
folders; `diagnostics.rs` bundle keys, redaction, file name; unit tests for the JSON writer, the URL
parser, the GitHub repository parser and `.whitespaces` trimming.

`swift_database.rs` builds a database with raw SQL exactly as the Swift app does (schema text
from Migrations.swift, `user_version = 1`, Swift-encoded JSON for a product and the three release
fixtures from `rd-records/tests/fixtures`, events, settings), opens it with the Rust store, reads
everything, writes it back (bytes must equal Swift's), writes changes, and reads again with a fresh
store. It also checks that the Rust migrator produces the same `sqlite_master` as the Swift SQL.

`real_database.rs` (ignored) runs the same checks against a copy of a real Swift database:

```sh
sqlite3 "$HOME/Library/Application Support/ReleaseDock/ReleaseDockData/Database/releasedock.sqlite" ".backup /tmp/rd-store-check.sqlite"
RD_STORE_REAL_DB=/tmp/rd-store-check.sqlite cargo test -p rd-store --test real_database -- --ignored --nocapture
```

It opens the given file read-only (`immutable=1`), works on a second copy, and prints only counts.

## Compatibility

- **Schema**: `INITIAL_SCHEMA` is the Swift SQL after Swift strips the multi-line literal's
  indentation, so even the `sqlite_master` text is identical. `PRAGMA user_version` tracks the schema;
  before upgrading an existing database the file is copied to
  `Backups/releasedock-v<old>-<ISO8601 with ':' → '-'>.sqlite` after `wal_checkpoint(TRUNCATE)`;
  each migration and its `user_version` run in one `BEGIN IMMEDIATE` transaction; a newer schema is
  refused with the Swift message.
- **PRAGMAs**: `journal_mode=WAL` (files only), `foreign_keys=ON`, `synchronous=FULL`, busy timeout
  5 s, file mode 0600 (Unix).
- **Key columns**: `created_at`, `updated_at` and `release_events.at` are seconds since **1970**
  (Swift writes `timeIntervalSince1970`), while dates inside the JSON are seconds since 2001
  (`SwiftDate`). IDs are uppercase UUIDs; `type` / `state` / `from_state` / `to_state` are the enum
  raw values.
- **JSON** (`swift_json.rs`): `JSONEncoder` with `.sortedKeys` sorts keys by UTF-8 bytes (the real
  database has `"Requires PHP"` before `"Requires at least"`), escapes `/` as `\/`, and writes a
  whole `Double` without a fraction. `encode` reproduces this, so rewriting an unchanged record gives
  Swift's bytes. The diagnostics bundle uses the pretty-printed form (`"key" : value`, slashes
  escaped).
- **Settings** are stored as JSON values (a `String` setting is `"0.2.3"` including the quotes).

## Differences from Swift

- `save(_ product:)` / `save(_ release:)` are `save_product` / `save_release`. `SettingsRepository`
  has raw `setting_data` / `set_setting_data` (object-safe) plus the generic `setting` /
  `set_setting`.
- `SqliteDatabase` guards the connection with a `Mutex`; instead of a recursive lock,
  `transaction` passes the body an `Executor` to run statements on. The transaction also rolls back
  if the body panics.
- `Migrator::migrate_with` takes the migration list (used by the tests); `migrate` uses
  `Migrator::MIGRATIONS`. Copying a backup reports `DatabaseError::Io`; Swift rethrows the
  `FileManager` error.
- `StoreError` adds `Database`, `Encoding`, `Decoding` and `Io` for errors Swift rethrows from
  SQLite, `JSONEncoder`, `JSONDecoder` and `FileManager`.
- `AppPaths::default_root()` reads the process environment, `default_root_with(&env)` takes it. The
  override must be an absolute path (`Path::is_absolute`; Swift checks for a leading `/`). The
  default is `dirs::data_dir()/ReleaseDock/ReleaseDockData`: `~/Library/Application Support/…` on
  macOS (the Swift folder), `%APPDATA%\ReleaseDock\ReleaseDockData` on Windows,
  `$XDG_DATA_HOME` or `~/.local/share/ReleaseDock/ReleaseDockData` on Linux. Folder modes (0700) and
  the database file mode (0600) apply on Unix only.
- `ProductService` takes an `Arc<dyn ProductRepository>`. The working copy path must be absolute
  per `Path::is_absolute` (on Windows, a drive or UNC path). `URL` handling is a small RFC 3986
  parser (scheme, user, password, host, percent-decoded path), since the product stores the URL as
  a string. `GitHubRepository(string:)` (GitHub crate, not ported yet) is reproduced as a private
  `is_github_repository`; `CharacterSet.alphanumerics` is approximated by `char::is_alphanumeric`.
- `DiagnosticsService`: Swift reads its log lines from `OSLogStore`; the Rust service takes an
  optional `LogSource` (no lines without one) and formats, redacts and keeps the last 2000 the same
  way. `os` is `std::env::consts::OS` and `ARCH` (e.g. `macos aarch64`) instead of
  `ProcessInfo.operatingSystemVersionString`.

## Not ported (with the pipeline)

These tests use `Store`/`AppPaths` only as the backing store of `ReleasePipelineService` or another
service that is not ported yet:

- `PipelineIntegrationTests` (all 33).
- `BuildTests` `BuildPipelineTests`: `testFolderReleaseShipsTheBuiltCopy`, `testBuildFailureFailsTheRelease`.
- `CodeCheckTests` `CodeCheckPipelineTests.testSyntaxErrorStopsBeforeTheWorkingCopyIsTouched`.
- `DirectoryStatusTests`: `testPendingThenLiveNotifiesOnce`, `testRefreshAwaitingSkipsUntrackedAndFinishedReleases`,
  `testWithoutWordPressOrgServiceNothingHappens`, `testNotListedProduct`.
- `GitHubReleaseTests` `GitHubReleasePipelineTests.testCompletedReleaseCreatesGitHubReleaseWithTheExactPackage`.
- `RestoreReleaseTests` (all 4).
- `ReadmeUpdateTests`: the 8 pipeline tests (`testAutomationGateRequiresTheReleasedTag` is in `rd-records`).
- `LiveRepositoryTests` `LiveDryRunTests.testDryRunAgainstWordPressOrg` (live network, pipeline).
- `DemoDataTests.testMakeDemoData` (`Store.open` + pipeline).
- `PerformanceTests.testLargeFolderAndZIPInspection`: the inspection part is in `rd-inspection`; the
  `ReleaseStagingService` part (uses `AppPaths`) goes with the staging service.
