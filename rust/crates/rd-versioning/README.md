# rd-versioning

Port of `Packages/ReleaseDockKit/Sources/ReleaseDockCore/Versioning/`.

## Swift file → Rust module

| Swift | Rust |
|---|---|
| `PHPVersionCompare.swift` (`PHPVersion`) | `php_version` (`compare`, `ordering`, `canonicalize`) |
| `WordPressHeaders.swift` | `headers` (`read`, `parse`, `PLUGIN_HEADERS`, `THEME_HEADERS`, `READ_LIMIT`) |
| `ReadmeParser.swift` (`WordPressReadme`, `ReadmeParser`) | `readme` (`WordPressReadme`, `parse`, `parse_file`, `sanitize_stable_tag`) |
| `ProductDetector.swift` (`PixelSize`, `DetectedProduct`, `ProductDetectionError`, `ProductDetector`) | `detector` |
| `VersionService.swift` | `version_service` |
| `VersionBumpService.swift` | `version_bump` |
| (Foundation/ICU text semantics) | `text` (private: whitespace sets, ICU line terminators, Swift `split`) |

Stored types (`DetectedProduct`, `PixelSize`, `WordPressReadme`, `VersionBumpPlan`,
`VersionBumpChange`) use serde with the Swift `Codable` key names; `tests/compatibility.rs`
round-trips JSON samples.

## Tests

| Swift test | Rust test |
|---|---|
| `VersionTests.swift`: PHPVersionCompareTests (3), VersionServiceTests (3), HeaderAndReadmeTests (5) | `tests/versioning.rs` |
| `VersionBumpTests.swift` (7) | `tests/version_bump.rs` |
| `InspectionTests.swift`: ProductDetectorTests (5) | `tests/detector.rs` |
| `ThemeChecksTests.swift`: `testDetectorReadsScreenshotSize` | `tests/detector.rs` |
| `ReadmeRulesTests.swift`: `testParsesShortDescriptionAndUpgradeNotice`, `testHeaderKeysAndValuesAreTrimmedLikeWordPressOrg`, parser parts of `testShortDescriptionWithColon…` (2) | `tests/versioning.rs` |
| `ReadmeRulesTests.swift`: ChangelogEntryTests `testExtractsEntriesInCommonHeadingStyles` | `tests/versioning.rs` |

`testMatchesInstalledPHP` is ported twice. `matches_recorded_php_results` checks all 1,024 pairs
against `tests/fixtures/php_version_compare.tsv`, which was recorded from PHP 8.5.7. It runs
without PHP installed. `matches_installed_php` runs the same pairs through a local `php`, and
it passes without checking anything when PHP is missing (Swift: `XCTSkip`).

Tests added beyond the Swift suite: exact validation and error messages, the ambiguous
plugin/theme case, the configured main file without a header, keeping file permissions on apply,
the JSON round trips, and unit tests for the `text` helpers.

### Not ported or only partly ported

- `testVersionNeverComesFromFileName`: the Swift test zips the plugin into `sample-plugin-9.9.9.zip`
  and extracts it with `ZIPInspectionService`, which belongs to rd-inspection. The Rust version
  keeps the intent: a folder named `sample-plugin-9.9.9` still reports the header version. The ZIP
  round trip should be tested in rd-inspection.
- `testShortDescriptionWithColonAfterBlankLineIsKept` and `testShortDescriptionWithColonDirectlyUnderHeadersIsLost`:
  only the parser assertions are ported. The `ReadmeRules.check` assertions belong to the
  validation crate. The second test checks `unknown_header_lines` instead, which is the parser
  output that rule reads.
- The other `ReadmeRulesTests`, and the `ProductDetector` calls in AssetRules, CodeCheck,
  Validation, WordPressOrg, GitHubRelease and LiveRepository tests, only use the detector to set
  up other subsystems. They move with those crates.

## Differences from Swift

- **Regex engine.** ICU look-ahead and back-references (`(?!…)`, `\2`) are emulated, because
  `regex` supports neither. The look-ahead becomes "followed by a character outside the class,
  or the end". The back-reference becomes "both quotes captured, and a candidate with different
  quotes is not a match". Multi-line `^`/`$` and `.` follow ICU's line terminators (`\n`, `\v`,
  `\f`, `\r`, U+0085, U+2028, U+2029, with `\r\n` as one terminator): the text is split on them,
  or `.` is written as an explicit class. Case-insensitive matching uses simple case folding, not
  ICU's full folding (for example `ß` against `ss`). The patterns here are ASCII, so this cannot
  change a result.
- **Characters vs `char`s.** Swift `count`, `hasPrefix` and `firstIndex(of:)` work on grapheme
  clusters, and Rust uses `char`s. Results differ only for text with combining marks right
  after `=`, `#`, `:` or `v`, and for the 64-character version limit. A version made of
  combining sequences can get the "longer than 64 characters" message instead of the
  "contains characters other than…" one; both say it is invalid.
- **Readme file choice.** `find_readme` sorts the directory names before picking a
  case-insensitive match. Swift takes directory order, which is unspecified. This only matters
  when both `readme.txt` and `README.txt` exist on a case-sensitive file system.
- **Errors.** I/O failures are `ProductDetectionError::Io` / `VersionBumpError::Io` with the path
  and the OS message. Swift shows Foundation's localized description there. All other messages
  are copied verbatim.
- **Overflow.** `VersionBumpService::suggest` returns `None` when a number would overflow `i64`.
  Swift traps.
- **Screenshot size.** `PixelSize::of_image_at` uses the `imagesize` crate instead of ImageIO.
  Both read the size from the file's own header.
- **Changelog date.** The `today` argument of `plan` is a `SwiftDate`. It is converted to the
  local calendar day with `chrono` (Swift: `Calendar.current`).
