# rd-inspection

Read-only inspection of release packages, ported from
`Packages/ReleaseDockKit/Sources/ReleaseDockCore/Inspection/`. Depends on `rd-support`
(hashing, `Crc32`, path safety) and `rd-model` (`PackageInspection`, `PackageEntry`,
`InspectionIssue`, `ValidationResult`, …).

## Swift file → Rust module

| Swift | Rust |
|---|---|
| `ZIPArchive.swift` (`ZIPError`, `ZIPEntry`, `ZIPArchive`, `SHA256Stream`) | `src/zip_archive.rs` (`ZipError`, `ZipEntry`, `ZipArchive`; SHA-256 streaming uses `sha2` directly) |
| `ZIPInspectionService.swift` (`ZIPInspecting`, `PackageInspectionError`, `ZIPInspectionService`) | `src/zip_inspection.rs` (`ZipInspecting`, `ZipInspectionService`); `PackageInspectionError` in `src/error.rs` |
| `FolderInspectionService.swift` (`FolderInspecting`, `FolderInspectionService`) | `src/folder_inspection.rs` |
| `FileClassifier.swift` (`GlobPattern`, `FileClassification`, `FileClassifier`) | `src/file_classifier.rs` (includes a port of Darwin `fnmatch(3)`) |
| `PackagePathEvaluator.swift` (`InspectionOptions`, `PackagePathEvaluator`, `Violation.issueKind`) | `src/path_evaluator.rs` (`issue_kind(&Violation)`) |
| `AssetInspector.swift` (`AssetFile`, `AssetsReport`, `AssetInspector`, `AssetRules`) | `src/asset_inspector.rs` (`asset_rules::check`) |
| — | `src/error.rs`: crate `Error` (ZIP, package, path, not-a-directory, I/O) |
| — | `src/text.rs`: Foundation `CharacterSet.whitespaces` / `.newlines` and `standardizedFileURL` |

Dependencies: `flate2` (raw DEFLATE decompression only; the ZIP container is parsed by our
own reader) and `imagesize` (image header dimensions instead of ImageIO).

## Tests

| Swift test | Rust test |
|---|---|
| `InspectionTests.swift` `ZIPInspectionTests` (15 tests) | `tests/inspection.rs`, same names in snake_case |
| `InspectionTests.swift` `FolderInspectionTests` (4 tests) | `tests/inspection.rs` (`symlinks_are_blocked` is `#[cfg(unix)]`) |
| `InspectionTests.swift` `ProductDetectorTests.testVersionNeverComesFromFileName` | `tests/inspection.rs` `version_never_comes_from_file_name` (adapted, see below) |
| `BuildTests.swift` `ExportIgnoreTests` (2 tests) | `tests/inspection.rs` `parses_export_ignore`, `gitattributes_apply_only_without_distignore` |
| `AssetRulesTests.swift` (4 of 5 tests) | `tests/asset_rules.rs` |
| `PerformanceTests.swift` `testLargeFolderAndZIPInspection` | `tests/performance.rs` (opt-in: `RELEASEDOCK_PERF_TESTS=1`) |

Test helpers from `Support/TestSupport.swift` are in `tests/common/mod.rs`: `TempDir`,
`fixtures::plugin_main_file` / `plugin_readme` / `Plugin` (`makePlugin`), `TestZipBuilder`
(malicious stored ZIPs), and two replacements for macOS tools: `zip_folder` (writes deflate
ZIPs with Info-ZIP style extra fields instead of running `/usr/bin/zip -r -y`) and
`write_png` (instead of CoreGraphics/ImageIO).

Extra tests not in Swift: `zip_archive` unit tests (valid ZIP64 record and extra field,
deflate bomb that understates its size, CRC mismatch, overlapping entries, truncated
central directory), `fnmatch` and pattern parsing unit tests, base64 of binary names, and
an `AssetsReport` JSON round trip.

### Not ported

- `PathSafetyTests` (in `InspectionTests.swift`): already ported in `rd-support`.
- `ProductDetectorTests` other than the ZIP test: belong to `rd-versioning`.
- `testVersionNeverComesFromFileName`, final assertion: Swift runs `ProductDetector` on the
  extracted folder; that type lives in `rd-versioning`, so the Rust test checks the
  extracted main file's `Version:` header directly. Re-add the detector assertion once a
  crate depends on both.
- `AssetRulesTests.testAssetWarningsDoNotBlockAndCanBeDisabled`: tests `ValidationService`
  (validation crate, not ported yet).
- `PerformanceTests`, the `ReleaseStagingService.buildCandidate` step: release crate.
- Tests in `ValidationTests`, `ThemeChecksTests`, `ReadmeRulesTests`, `WordPressOrgTests`,
  `CodeCheckTests` and `BuildTests` that call `FolderInspectionService` only as setup for
  validation, build or pipeline behaviour; they belong to those crates.

## Behaviour differences from Swift

- **Folder walk order** is depth-first pre-order with children sorted by name, so issue
  order, `excludedPaths` order, the entry-limit cut-off and which path a case-duplicate
  message names are deterministic. Swift follows `FileManager` enumeration order. The
  sorted `includedEntries` and the tree digest are identical.
- **Hidden assets**: `.skipsHiddenFiles` also honours the macOS `UF_HIDDEN` flag; Rust
  skips names starting with `.` only.
- **Non-UTF-8 file names in folders** (possible on Linux) are reported as
  `unsafeFilename` with "Path is not valid UTF-8" and the lossy path; macOS cannot have them.
- **Invalid deflate data** yields `Corrupt ZIP archive: invalid deflate data for <name>`;
  Swift surfaces the Compression framework's own error. Size and CRC checks are identical.
- **Error text from the OS** ("File cannot be read: …", I/O errors) uses Rust's
  `io::Error` text instead of Cocoa's `localizedDescription`.
- **Size totals** use saturating addition; Swift would trap on `UInt64` overflow from
  absurd ZIP64 sizes (this only makes Rust more robust; the total then exceeds every limit).
- **Extraction** creates files with `create_new` (`O_CREAT|O_EXCL`), which never follows a
  symlink at the final component, so `O_NOFOLLOW` (needs `libc`) is not passed separately.
  The file is not `fsync`ed, as in Swift; close errors are not reported.
- **String comparison**: Swift `String` equality and `hasPrefix`/`hasSuffix` use Unicode
  canonical equivalence; Rust compares bytes. Only matters for names in different Unicode
  normalization forms (all built-in name lists are ASCII).
- **fnmatch** works on Unicode characters; Darwin's result for non-ASCII names depends on the
  process locale (in the C locale `?` matches a single byte). `[[:class:]]` character
  classes are not implemented.
- **Regex `$`** in asset name rules matches only at the end of the name; `NSRegularExpression`
  also matches before a final newline (file names with a trailing newline).
- **blueprint.json** is parsed with `serde_json` (a UTF-8 BOM is accepted, top-level
  scalars are "not valid JSON", as with `JSONSerialization` without `.fragmentsAllowed`);
  UTF-16/UTF-32 encoded files, which `JSONSerialization` would detect, are "not valid JSON".
- **Image sizes** come from `imagesize` (header parsing, content-detected format like
  ImageIO). Raw pixel dimensions are reported in both; exotic formats ImageIO decodes but
  `imagesize` does not leave width/height empty.
- `AssetsReport`/`AssetFile` are defined here (they are `Codable` in Swift and stored in
  release records); if `ReleaseRecord` moves to `rd-model`, these types may need to move too.
