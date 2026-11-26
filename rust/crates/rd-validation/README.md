# rd-validation

Port of `Packages/ReleaseDockKit/Sources/ReleaseDockCore/Validation/`, plus the pure helpers
`ValidationService` needs from Swift files that are not ported yet.

Validation is a pure function of a `ValidationContext`. It reuses `Product`, `PackageInspection`,
`ValidationResult` and `ValidationReport` (rd-model), `DetectedProduct`, `WordPressReadme` and
`VersionService` (rd-versioning), `AssetsReport` and `asset_rules` (rd-inspection), and
`RepositorySnapshot`, `GitRepositoryState`, `ReleasePreview`, `ReleaseKind` and `CodeCheckReport`
(rd-records). It defines no stored types, so it has no JSON round-trip tests.

## Swift file → Rust module

| Swift | Rust |
|---|---|
| `Validation/ValidationService.swift`: `ValidationContext` | `context` (`ValidationContext`, `ValidationContext::new` = the Swift initializer's defaults) |
| `Validation/ValidationService.swift`: `ValidationServicing`, `ValidationService` | `service` (`ValidationServicing`, `ValidationService`, `STRUCTURAL_KINDS`, `ValidationService::LISTED_FINDINGS`) |
| `Validation/ReadmeRules.swift` | `readme_rules` (`check`, `plain_text`, `check_requires_plugins`, limits as constants) |
| `Release/ReleaseStrategy.swift`: the pure part of `ReleaseStrategy`, `StrategyProvider` | `strategy` (`ReleaseStrategy`, `StrategyProvider`) |
| `Release/PluginReleaseStrategy.swift`: version format, metadata, required and forbidden files, release paths, `readmeName(in:)` | `plugin_strategy` (`PluginReleaseStrategy`, `IMPORTER_VERSION_PATTERN`) |
| `Release/ThemeReleaseStrategy.swift`: version format, metadata, required and forbidden files, release paths, `isVersionDirectory` | `theme_strategy` (`ThemeReleaseStrategy`, header and extension lists as constants) |
| `WordPressOrg/WordPressOrgService.swift`: `WordPressBranch` | `wordpress_branch` (`WordPressBranch`) |
| `Inspection/AssetInspector.swift`: `AssetRules` | already in rd-inspection (`asset_rules`); reused |
| (Foundation string semantics) | `text` (private: grapheme counts, `capitalized`, `pathExtension`, `lowercased`, `ByteCountFormatter`) |

The repository, working-copy, apply and verify methods of the Swift `ReleaseStrategy` protocol
need the SVN client. They belong to the release pipeline, which can add them as a second trait
implemented for `PluginReleaseStrategy` and `ThemeReleaseStrategy`.

## Tests

| Swift test | Rust test |
|---|---|
| `ValidationTests.swift`: all 17 tests of `ValidationServiceTests`, including `testAutomationGate` | `tests/validation.rs` |
| `ReadmeRulesTests.swift`: `testCleanReadmeHasNoWarnings`, `testShortDescriptionLimitCountsVisibleText`, `testShortDescriptionWithColonAfterBlankLineIsKept`, `testShortDescriptionWithColonDirectlyUnderHeadersIsLost`, `testMissingShortDescriptionAndDescriptionSection`, `testLongUpgradeNoticeWarnsForThemesOnly`, `testLargeReadmeAndLongChangelog`, `testRequiresPlugins`, `testRulesRunForPluginsDuringValidation` | `tests/readme_rules.rs` |
| `ThemeChecksTests.swift`: `testCorrectScreenshotAndNameHaveNoThemeWarnings`, `testScreenshotSizeAndRatio`, `testReservedNamesAndSpelling`, `testBundledResourcesNeedAResourcesSection` | `tests/theme_checks.rs` |
| `CodeCheckTests.swift`: all 4 tests of `CodeCheckValidationTests` | `tests/code_checks.rs` |
| `AssetRulesTests.swift`: `testAssetWarningsDoNotBlockAndCanBeDisabled` | `tests/asset_rules.rs` |
| `WordPressOrgTests.swift`: `testBranches` and all 7 tests of `TestedUpToFreshnessTests` | `tests/wordpress_org.rs` |
| `TestSupport.swift` (`TempDir`, `Fixtures`), `CoreTests.swift` (`makeProduct`), `ThemeChecksTests.writePNG` | `tests/common/mod.rs` |

`testAutomationGate` is now ported with the real validator. rd-records has an adapted copy that
uses a hand-written passing report; that copy can stay or be removed.

Tests added beyond the Swift suite:

- `tests/added_checks.rs` pins the exact results of checks that Swift tests only through the
  pipeline: released version and tag for readme updates, themes refused for readme updates, the
  readme-update and normal release diffs, Git tree and restored versions, repository messages,
  source integrity, structure and size, excluded-path counts, and the undetected-product path.
- `tests/code_checks.rs::many_syntax_errors_are_summarized`, and two extra tests in
  `tests/readme_rules.rs`: Markdown stripping, and grapheme counting of the short description.
- Unit tests for the `text` helpers, compared with values printed by Foundation. There are also
  unit tests for the theme spelling look-ahead and for `readme_name` / `is_version_directory`.

### Not ported (ported with the pipeline or their service)

- `ReadmeRulesTests.swift`: `testParsesShortDescriptionAndUpgradeNotice`,
  `testHeaderKeysAndValuesAreTrimmedLikeWordPressOrg`, and `ChangelogEntryTests`. These are
  parser-only and already in rd-versioning (`tests/versioning.rs`), together with the parser
  assertions of the two `testShortDescriptionWithColon…` tests.
- `ThemeChecksTests.swift`: `testDetectorReadsScreenshotSize`. Detector-only, already in
  rd-versioning (`tests/detector.rs`).
- `AssetRulesTests.swift`: the other tests cover `AssetInspector` and `AssetRules`, and are in
  rd-inspection (`tests/asset_rules.rs`).
- `CodeCheckTests.swift`: `CodeCheckParsingTests` and `CodeCheckServiceTests` go with the code
  check service. `CodeCheckPipelineTests.testSyntaxErrorStopsBeforeTheWorkingCopyIsTouched`
  goes with the pipeline.
- `WordPressOrgTests.swift`: `testParsesLatestVersionFromVersionCheck` and
  `testRejectsUnexpectedVersionCheckPayloads` go with the WordPress.org service.
- With the pipeline: `ReadmeUpdateTests` (`testRequiresTheReleasedVersion`,
  `testNothingToUpdateIsBlocked`, `testThemesCannotUseReadmeUpdates` and the rest),
  `RestoreReleaseTests`, and the `PipelineIntegrationTests` that assert on validation results
  (`testSameVersionAndOlderVersionFailValidation`, `testExistingTagBlocks`,
  `testDirtyWorkingCopyBlocksAndCanBeRevertedWithBackup`, `testNetworkFailureDuringValidation`,
  `testMissingCredentialsBlockPublishing`, `testIgnoredFilesInWorkingCopyBlockReleaseButJunkDoesNot`,
  `testThemeWithoutExistingVersionIsBlocked`, `testPreviewChangeAfterApprovalIsRevalidated`).
  `tests/added_checks.rs` covers their validation side.

## Differences from Swift

- **Look-ahead.** The theme spelling rule `\bWordpress\b|\bwordpress\b(?!\.)` has a look-ahead,
  which `regex` does not support. Rust finds `\bwordpress\b` and accepts the match only when the
  next character is not `.`. All other patterns are used unchanged. `NSString.range(of:options:
  .regularExpression)` anchors `$` only at the end of the input (checked against Foundation), and
  so does `regex`.
- **Characters.** Lengths and prefixes that users see use grapheme clusters, like Swift
  `count` / `prefix`: the short description, the upgrade notice, the 60-character header line,
  the digest prefix and the commit prefix. Word counts use `split_whitespace`; Swift
  `Character.isWhitespace` gives the same result for all text except a whitespace character
  followed by a combining mark. Theme-name words split on `!is_alphabetic && !is_numeric` per
  `char`. Swift's `Character.isLetter` would keep a letter and a following combining mark
  together.
- **Foundation helpers.** `capitalized` (`License Uri`, `Frequently Asked Questions`) and
  `pathExtension` (no extension for `.htaccess`) are reimplemented. Unit tests compare them with
  values printed by Foundation. `ByteCountFormatter` (`.file` style, English) is a copy of
  `rd_svn::transcript::byte_count_file_style`, so validation does not depend on the SVN client
  and tokio. It could move to rd-support, where both crates would use it.
- **Negative package size setting.** `UInt64(packageSizeWarningMB)` traps in Swift when the value
  is negative. Rust treats a negative value as 0 MB.
- **Injection.** `ValidationService::with(versions, strategies)` takes an
  `Arc<dyn VersionServicing>`, which replaces the Swift initializer's default arguments.
  `ValidationService::new()` uses `VersionService`.
- **Repository URL.** The "does not exist" message prints `Product.svn_repository_url` as stored.
  Swift prints `URL.absoluteString`, which is the same string for every URL the product service
  accepts.
