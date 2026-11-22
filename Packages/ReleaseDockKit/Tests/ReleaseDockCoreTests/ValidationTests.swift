import XCTest
@testable import ReleaseDockCore

final class ValidationServiceTests: XCTestCase {
    let validator = ValidationService()

    func context(version: String = "1.1.0", stableTag: String? = nil, current: String? = "1.0.0", tagExists: Bool = false, extra: [String: String] = [:], product: Product = makeProduct(), snapshot: ((inout RepositorySnapshot) -> Void)? = nil) throws -> (ValidationContext, TempDir) {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: version, stableTag: stableTag, extraFiles: extra)
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try? ProductDetector().detect(at: root, expectedType: product.type, slug: product.slug)
        var repo = RepositorySnapshot(svnVersion: "1.14.5", repositoryExists: true, currentVersion: current, existingVersions: current.map { [$0] } ?? [], releasePathExists: tagExists, workingCopyExists: true, workingCopyRevision: 10, remoteRevision: 10, authentication: .keychain)
        snapshot?(&repo)
        return (ValidationContext(product: product, inspection: inspection, detected: detected, repository: repo), dir)
    }

    func failures(_ report: ValidationReport, _ check: ValidationCheckID) -> [ValidationResult] {
        report.results.filter { $0.check == check && $0.status == .failed }
    }

    func testValidReleasePasses() throws {
        let (ctx, _dir) = try context()
        _ = _dir
        let report = validator.validate(ctx)
        XCTAssertFalse(report.isBlocking, "\(report.blockingResults)")
        for check in [ValidationCheckID.sourceIntegrity, .productIdentity, .versionValidity, .versionIncrease, .metadataValidity, .requiredFiles, .packageStructure, .svnConnectivity, .workingCopyClean, .existingTag, .remoteFreshness, .authentication] {
            XCTAssertTrue(report.results.contains { $0.check == check && $0.status == .passed }, "\(check) should pass")
        }
        XCTAssertTrue(report.results.allSatisfy { !$0.name.isEmpty && !$0.message.isEmpty })
    }

    func testSameVersionIsBlocked() throws {
        let (ctx, dir) = try context(version: "1.0.0", current: "1.0.0"); _ = dir
        XCTAssertFalse(failures(validator.validate(ctx), .versionIncrease).isEmpty)
    }

    func testOlderVersionIsBlocked() throws {
        let (ctx, dir) = try context(version: "0.9.0", current: "1.0.0"); _ = dir
        XCTAssertFalse(failures(validator.validate(ctx), .versionIncrease).isEmpty)
    }

    func testInvalidVersionRequiresReview() throws {
        let (ctx, dir) = try context(version: "1.1-hotfix"); _ = dir
        let report = validator.validate(ctx)
        XCTAssertTrue(failures(report, .versionValidity).first?.message.contains("requires review") == true)
        XCTAssertTrue(report.isBlocking)
    }

    func testExistingTagIsBlocked() throws {
        let (ctx, dir) = try context(tagExists: true); _ = dir
        XCTAssertFalse(failures(validator.validate(ctx), .existingTag).isEmpty)
    }

    func testStableTagMismatchIsBlocked() throws {
        let (ctx, dir) = try context(version: "1.1.0", stableTag: "1.0.0"); _ = dir
        let failed = failures(validator.validate(ctx), .metadataValidity)
        XCTAssertTrue(failed.contains { $0.message.contains("does not match") && $0.filePath == "readme.txt" && $0.suggestion != nil })
    }

    func testStableTagTrunkIsBlocked() throws {
        let (ctx, dir) = try context(version: "1.1.0", stableTag: "trunk"); _ = dir
        XCTAssertTrue(failures(validator.validate(ctx), .metadataValidity).contains { $0.message.contains("trunk") })
    }

    func testMissingMetadataIsBlocked() throws {
        let dir = TempDir()
        dir.write("p/sample-plugin.php", "<?php\n/* Plugin Name: Sample Plugin */\n")
        let inspection = try FolderInspectionService().inspect(folderAt: dir.path("p"), options: InspectionOptions())
        let detected = try ProductDetector().detect(at: dir.path("p"))
        let report = validator.validate(ValidationContext(product: makeProduct(), inspection: inspection, detected: detected, repository: RepositorySnapshot(repositoryExists: true, releasePathExists: false, authentication: .keychain)))
        XCTAssertFalse(failures(report, .versionValidity).isEmpty, "missing Version header")
        XCTAssertTrue(failures(report, .metadataValidity).contains { $0.message.contains("readme.txt is missing") })
    }

    func testProductMismatchIsBlocked() throws {
        var product = makeProduct()
        product.expectedHeaderName = "Completely Different"
        let (ctx, dir) = try context(product: product); _ = dir
        XCTAssertFalse(failures(validator.validate(ctx), .productIdentity).isEmpty)

        let (themeCtx, dir2) = try context(product: makeProduct(type: .theme, slug: "sample-plugin")); _ = dir2
        XCTAssertTrue(validator.validate(themeCtx).isBlocking, "a plugin package for a theme product must block")
    }

    func testDirtyWorkingCopyAndRemoteChangesAreBlocked() throws {
        let (dirty, d1) = try context { $0.workingCopyChanges = ["modified: trunk/x.php"] }; _ = d1
        XCTAssertFalse(failures(validator.validate(dirty), .workingCopyClean).isEmpty)
        let (stale, d2) = try context { $0.outOfDatePaths = ["/wc/trunk/readme.txt"] }; _ = d2
        XCTAssertFalse(failures(validator.validate(stale), .remoteFreshness).isEmpty)
    }

    func testNetworkFailureAndMissingAuthBlock() throws {
        let (offline, d1) = try context { $0 = RepositorySnapshot(connectivityError: "Unable to connect", authentication: .keychain) }; _ = d1
        let report = validator.validate(offline)
        XCTAssertFalse(failures(report, .svnConnectivity).isEmpty)
        XCTAssertTrue(report.blockingResults.contains { $0.check == .existingTag && $0.status == .skipped }, "skipped required checks block")
        let (noAuth, d2) = try context { $0.authentication = .none }; _ = d2
        XCTAssertFalse(failures(validator.validate(noAuth), .authentication).isEmpty)
    }

    func testForbiddenFilesBlock() throws {
        let (ctx, dir) = try context(extra: ["vendor/bundle.zip": "PK"]); _ = dir
        XCTAssertTrue(failures(validator.validate(ctx), .forbiddenFiles).contains { $0.filePath == "vendor/bundle.zip" })
    }

    func testWarningsBlockWhenConfigured() throws {
        var product = makeProduct()
        product.validation.blockOnWarnings = true
        let (ctx, dir) = try context(version: "1.1.0-beta1", product: product); _ = dir
        XCTAssertTrue(validator.validate(ctx).isBlocking, "pre-release warning blocks with blockOnWarnings")
    }

    func testOptionalChecksCanBeDisabledButRequiredCannot() throws {
        var product = makeProduct()
        product.validation.disabledChecks = [.packageSize, .versionIncrease]
        let (ctx, dir) = try context(version: "1.0.0", current: "1.0.0", product: product); _ = dir
        let report = validator.validate(ctx)
        XCTAssertFalse(report.results.contains { $0.check == .packageSize })
        XCTAssertFalse(failures(report, .versionIncrease).isEmpty)
    }

    func testReleaseDiffValidation() throws {
        var (ctx, dir) = try context(); _ = dir
        ctx.preview = ReleasePreview(changes: [PreviewChange(path: "trunk/a.php", kind: .modified, isDirectory: false)], diffExcerpt: "", diffTruncated: false, commitMessage: "", fingerprint: "x", workingCopyRevision: 1)
        XCTAssertTrue(failures(validator.validate(ctx), .releaseDiff).contains { $0.message.contains("does not create") })
        ctx.preview?.changes = [PreviewChange(path: "tags/1.1.0", kind: .added, isDirectory: true, isCopy: true), PreviewChange(path: "tags/0.5/x.php", kind: .modified, isDirectory: false)]
        XCTAssertTrue(failures(validator.validate(ctx), .releaseDiff).contains { $0.filePath == "tags/0.5/x.php" })
    }

    func testThemeRules() throws {
        let dir = TempDir()
        let root = Fixtures.makeTheme(in: dir, version: "1.1.0-beta")
        dir.write("sample-theme/data.sql", "x")
        let product = makeProduct(type: .theme, slug: "sample-theme")
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root)
        let report = validator.validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: RepositorySnapshot(repositoryExists: true, currentVersion: "1.0.0", releasePathExists: false, authentication: .keychain)))
        XCTAssertTrue(failures(report, .versionValidity).contains { $0.message.contains("only numbers") })
        XCTAssertTrue(failures(report, .forbiddenFiles).contains { $0.filePath == "data.sql" })

        try FileManager.default.removeItem(at: root.appendingPathComponent("screenshot.png"))
        let inspection2 = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let report2 = validator.validate(ValidationContext(product: product, inspection: inspection2, detected: detected, repository: nil))
        XCTAssertTrue(failures(report2, .requiredFiles).contains { $0.message.contains("screenshot") })
    }

    func testAutomationGate() throws {
        var product = makeProduct(mode: .automatic, automation: false)
        var record = ReleaseRecord(product: product, source: .folder(path: "/x"), mode: .automatic)
        var (ctx, dir) = try context(); _ = dir
        let preview = ReleasePreview(changes: [PreviewChange(path: "tags/1.1.0", kind: .added, isDirectory: true, isCopy: true)], diffExcerpt: "", diffTruncated: false, commitMessage: "", fingerprint: "fp", workingCopyRevision: 1)
        ctx.preview = preview
        record.validation = validator.validate(ctx)
        record.repository = ctx.repository
        record.preview = preview
        XCTAssertTrue(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "fp").contains { $0.contains("not explicitly enabled") })
        product.automationEnabled = true
        XCTAssertEqual(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "fp"), [])
        XCTAssertFalse(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "changed").isEmpty)
        record.errors = ["x"]
        XCTAssertFalse(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "fp").isEmpty)
    }
}
