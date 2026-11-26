# rd-records

Every type the Swift app stores in a release record, plus the pure release logic (state machine,
automation gate, manifest, restore options). The database, validation and pipeline crates build on it.

## Swift file → Rust module

| Swift (`Packages/ReleaseDockKit/Sources/ReleaseDockCore/`) | Rust (`src/`) |
|---|---|
| `Models/Release.swift` | `release.rs` (`ReleaseState`, `ReleaseSource`, `StateTransition`, `ReleaseKind`, `ApprovalKind`, `Approval`, `ChangeKind`, `PreviewChange`, `ReleasePreview`, `VerificationReport`, `DirectoryStatus` + `DirectoryState` (= `DirectoryStatus.State`), `ReleaseRecord`, `RestoreOptions`) |
| `Models/RepositorySnapshot.swift` | `repository_snapshot.rs` (`AuthenticationAvailability`, `RepositorySnapshot`, `WorkingCopyChange`, `WorkingCopyState`, `ConflictResolution`) |
| `Release/ReleaseStateMachine.swift` | `state_machine.rs` (`ReleaseStateError`, `ReleaseStateMachine`) |
| `Release/AutomationGate.swift` | `automation_gate.rs` (`AutomationGate`) |
| `Release/ReleaseManifest.swift` | `manifest.rs` (`ReleaseManifest`, `ManifestFile` (= `ReleaseManifest.File`), `ManifestError`); `swift_json.rs` writes Foundation's pretty-printed layout; `iso8601.rs` for `.iso8601` dates |
| `CodeChecks/CodeCheckService.swift` (data types only) | `code_checks.rs` (`CodeCheckTool`, `CodeCheckFinding`, `CodeCheckReport`) |
| `Git/GitService.swift` (data type only) | `git.rs` (`GitRepositoryState`) |
| `GitHub/GitHubReleasePublisher.swift` (data type only) | `github.rs` (`GitHubReleaseResult`; `GitHubReleaseSettings` is re-exported from `rd-model`) |
| `Release/BuildService.swift` (data type only) | `build.rs` (`BuildResult`) |
| `Release/ReleaseStagingService.swift` (data type only) | `staging.rs` (`StagedCandidate`) |
| `WordPressOrg/WordPressOrgService.swift` (data type only) | `wordpress_org.rs` (`DirectoryListing`, used by `DirectoryStatus::state`) |

Types stored in a record that live elsewhere are re-exported, not duplicated: `PackageInspection`,
`ValidationReport`, `Id`, `SwiftDate`… (`rd-model`), `DetectedProduct`, `WordPressReadme`, `PixelSize`
(`rd-versioning`), `AssetsReport`, `AssetFile` (`rd-inspection`), `SvnItemStatus` (`rd-svn`).

| Swift tests (`Tests/ReleaseDockCoreTests/`) | Rust (`tests/`) |
|---|---|
| `CoreTests.swift` `ReleaseStateMachineTests` (all 5) | `state_machine.rs` |
| `CoreTests.swift` `ManifestTests.testManifestContainsReleaseDataAndNoSecrets` | `manifest.rs` |
| `ValidationTests.swift` `testAutomationGate` | `automation_gate.rs` (adapted, see below) |
| `ReadmeUpdateTests.swift` `testAutomationGateRequiresTheReleasedTag` | `automation_gate.rs` |
| `DirectoryStatusTests.swift` `testStates`, `testOnlyCommittedWordPressOrgVersionReleasesAreTracked`, `testAwaitingWindowAndFinalStates` | `directory_status.rs` |
| `RestoreReleaseTests.swift` `testOptionsListOlderVersionsAndSuggestAVersionAboveTheHighest` | `restore.rs` (adapted, see below) |
| `CoreTests.swift` `makeProduct` | `common/mod.rs` |
| — (JSON compatibility with real Swift records in `tests/fixtures/`) | `compatibility.rs` |

Adapted tests:

- `testAutomationGate` validates a fixture plugin with `ValidationService`, which is not ported yet. The
  Rust test writes out the all-passing report that validation produces for that context instead; every
  gate assertion is kept.
- `testOptionsListOlderVersions…` releases two versions to a local SVN repository and asks the pipeline
  for `RestoreOptions`; the Rust test builds `RestoreOptions` from the equivalent `RepositorySnapshot`.
  The pipeline-level test is also listed below.

## JSON compatibility

- `ReleaseSource` is encoded like Swift's synthesized `Codable` for an enum with labelled associated
  values: `{"zip":{"path":"/x.zip"}}` / `{"folder":{"path":"/dev/acme"}}` (serde's externally tagged
  struct variants).
- Acronym keys are renamed explicitly: `productID`, `sourceSHA256`, `svnURL`, `tagURL`,
  `workingCopyURLMismatch`, `latestWordPressVersion`, `remoteURL`, `cachedBySVN`, `releaseID`.
- Non-optional Swift fields are required when decoding, as in Swift (no custom `init(from:)` exists).
  `releaseKind` is optional; read it through `ReleaseRecord::kind()`.
- `GitHubReleaseResult.url` (Swift `URL`) is a plain string.
- `ReleaseManifest` is the one type written with `.iso8601` dates and `[.prettyPrinted, .sortedKeys,
  .withoutEscapingSlashes]`; `encoded()` reproduces Foundation's layout (`"key" : value`, empty
  arrays as `[\n\n  ]`, case-insensitive key order).
- `tests/compatibility.rs` decodes the three Swift fixtures, round-trips them, and compares the
  re-encoded JSON tree with the original key by key (numbers by value).

## Differences from Swift

- Default arguments become separate functions: `ReleaseRecord::new` (fresh id, kind `version`, now)
  and `ReleaseRecord::new_with` (all parameters); `ReleaseStateMachine::transition` (now) and
  `transition_at`; `awaits_directory(now, window)` with `DIRECTORY_WAIT_WINDOW` (72 h) as the Swift default.
- `URL(string: svnURL)?.host` is a small parser (`scheme://[userinfo@]host[:port]`); no `url` crate.
- `CodeCheckReport.findings(_ tool:)` is `findings_for(tool)`; `StagedCandidate.rootURL` is `root_dir()`;
  `GitRepositoryState.notARepository` is `not_a_repository()`.

## Not ported (ported in rd-pipeline or their service)

- `DirectoryStatusTests`: `testParsesDirectoryListing` (`WordPressOrgService.parseListing`, WordPress.org
  service crate); `testPendingThenLiveNotifiesOnce`, `testRefreshAwaitingSkipsUntrackedAndFinishedReleases`,
  `testWithoutWordPressOrgServiceNothingHappens`, `testNotListedProduct` (pipeline).
- `RestoreReleaseTests`: all four tests at pipeline level (`testOptionsListOlderVersions…` is also
  covered here in adapted form).
- `ReadmeUpdateTests`: every test except `testAutomationGateRequiresTheReleasedTag` (pipeline).
- `CommitTemplateTests`: `testTemplateBecomesTheSummaryAndKeepsTheMarker`,
  `testInvalidTemplateIsRefusedBeforeAnythingRuns` (pipeline; the template itself is in `rd-support`).
- `CodeCheckTests` (code check service and validation rules), `BuildTests` (build service, pipeline),
  `GitHubReleaseTests` / `GitHubServiceTests` (GitHub service, pipeline), `WordPressOrgTests`
  (WordPress.org service, validation).
- `CoreTests.swift` `StoreTests` (database crate), `ProductServiceTests` (product service).
- `PipelineIntegrationTests`, `TranscriptTests.testReleaseRecordsItsSVNCommandsPerOperation`,
  `DemoDataTests` (pipeline).
