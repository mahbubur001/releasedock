import XCTest
@testable import ReleaseDockCore

/// Readme & assets updates of the released version, against local file:// repositories.
final class ReadmeUpdateTests: XCTestCase {
    var repo: LocalSVNRepository!
    var work: TempDir!
    var store: Store!
    var svn: FaultInjectingSVN!
    var notifications: RecordingNotifications!
    var pipeline: ReleasePipelineService!

    override func setUp() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        repo = try LocalSVNRepository()
        work = TempDir()
        store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        svn = FaultInjectingSVN(base: SVNService(commandTimeout: .seconds(60)))
        notifications = RecordingNotifications()
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), notifications: notifications, generator: "tests"
        ))
    }

    func registerPlugin(mode: ReleaseMode = .assisted, assets: Bool = false) throws -> Product {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        var product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path,
            currentVersion: "1.0.0", releaseMode: mode
        )
        if assets {
            work.write("assets/banner-772x250.png", "PNG")
            product.assetsPath = work.path("assets").path
        }
        try store.save(product)
        return product
    }

    /// The released 1.0.0 package with "Tested up to" raised to 7.1, plus `extra` files.
    func source(version: String = "1.0.0", extra: [String: String] = [:]) throws -> ReleaseSource {
        let src = TempDir()
        var files = extra
        files["readme.txt"] = Fixtures.pluginReadme(stableTag: version).replacingOccurrences(of: "Tested up to: 6.8", with: "Tested up to: 7.1")
        let plugin = Fixtures.makePlugin(in: src, version: version, extraFiles: files)
        let zip = work.path("sample-plugin-\(UUID().uuidString.prefix(4)).zip")
        try Fixtures.zip(folder: plugin, to: zip)
        return .zip(path: zip.path)
    }

    func remoteReadme(_ path: String) throws -> String {
        try repo.run(["cat", "\(repo.url)/sample-plugin/\(path)/readme.txt"])
    }

    func testUpdatesReadmeInTrunkAndReleasedTagOnly() async throws {
        let product = try registerPlugin(assets: true)
        var record = try await pipeline.createRelease(productID: product.id, source: try source(), kind: .readmeUpdate)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? []) \(record.errors)")
        XCTAssertTrue(record.validation?.results.contains { $0.check == .releasedVersion && $0.status == .passed } == true)
        XCTAssertFalse(record.validation?.results.contains { $0.check == .versionIncrease } == true)
        let paths = Set(try XCTUnwrap(record.preview).changes.map(\.path))
        XCTAssertEqual(paths, ["trunk/readme.txt", "tags/1.0.0/readme.txt", "assets/banner-772x250.png"])
        XCTAssertTrue(record.preview!.commitMessage.hasPrefix("Update readme for Sample Plugin 1.0.0"))

        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.verification?.isClean, true, "\(String(describing: record.verification))")
        XCTAssertNotNil(record.committedRevision)

        XCTAssertTrue(try remoteReadme("trunk").contains("Tested up to: 7.1"))
        XCTAssertTrue(try remoteReadme("tags/1.0.0").contains("Tested up to: 7.1"))
        let tags = try repo.run(["list", "\(repo.url)/sample-plugin/tags"])
        XCTAssertEqual(tags.trimmingCharacters(in: .whitespacesAndNewlines), "1.0.0/", "no new tag")
        let updated = try XCTUnwrap(try store.product(id: product.id))
        XCTAssertEqual(updated.currentVersion, "1.0.0")
        XCTAssertNil(updated.lastSuccessfulRelease, "a readme update is not a version release")
        XCTAssertTrue(record.guidance.contains { $0.contains("6 hours") })
        XCTAssertTrue(notifications.titles.contains { $0.contains("readme updated") })
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testRefusesCodeChanges() async throws {
        let product = try registerPlugin()
        let record = try await pipeline.createRelease(productID: product.id, source: try source(extra: ["includes/new.php": "<?php\n"]), kind: .readmeUpdate)
        do {
            _ = try await pipeline.prepare(record.id)
            XCTFail("code changes must be refused")
        } catch ReleaseError.readmeUpdateChangesCode(let tag, let paths) {
            XCTAssertEqual(tag, "tags/1.0.0")
            XCTAssertEqual(paths, ["includes/new.php"])
        }
        XCTAssertEqual(try store.release(id: record.id)?.state, .failed)
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0, "working copy restored")
        XCTAssertEqual(svn.commitCount, 0)
    }

    func testRequiresTheReleasedVersion() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try source(version: "1.1.0"), kind: .readmeUpdate)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation?.blockingResults.contains { $0.check == .releasedVersion && $0.message.contains("1.1.0") } == true)
        XCTAssertTrue(record.validation?.blockingResults.contains { $0.check == .existingTag } == true, "tags/1.1.0 does not exist")
    }

    func testNothingToUpdateIsBlocked() async throws {
        let product = try registerPlugin()
        let src = TempDir()
        let plugin = Fixtures.makePlugin(in: src, version: "1.0.0")
        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: plugin.path), kind: .readmeUpdate)
        do {
            record = try await pipeline.prepare(record.id)
            XCTFail("an identical package must not produce an empty commit")
        } catch ReleaseError.validationBlocked {
            record = try XCTUnwrap(try store.release(id: record.id))
        }
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation?.blockingResults.contains { $0.check == .releaseDiff && $0.message.contains("nothing to update") } == true)
    }

    func testDryRunCommitsNothing() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try source(), mode: .dryRun, kind: .readmeUpdate)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(svn.commitCount, 0)
        XCTAssertFalse(try remoteReadme("tags/1.0.0").contains("Tested up to: 7.1"))
    }

    func testUncertainCommitIsRecoveredByMarker() async throws {
        let product = try registerPlugin()
        svn.commitFault = .uncertainAfterCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try source(), kind: .readmeUpdate)
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(svn.commitCount, 1, "never retried blindly")
        XCTAssertTrue(try store.events(releaseID: record.id).map(\.to).contains(.recoveryRequired))
    }

    func testUncertainCommitThatDidNotLandIsVerifiedAbsent() async throws {
        let product = try registerPlugin()
        svn.commitFault = .uncertainWithoutCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try source(), kind: .readmeUpdate)
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .recoveryRequired, "the existing tag must not be mistaken for someone else's release")
        XCTAssertTrue(record.verifiedNotCommitted)
        svn.commitFault = .none
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(svn.commitCount, 2)
    }

    func testThemesCannotUseReadmeUpdates() async throws {
        try repo.createTheme(slug: "sample-theme", initialVersion: "1.0.0")
        let theme = Product(
            name: "Sample Theme", slug: "sample-theme", type: .theme,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-theme")!,
            workingCopyPath: work.path("wc-theme").path, currentVersion: "1.0.0"
        )
        try store.save(theme)
        let src = TempDir()
        let root = Fixtures.makeTheme(in: src, version: "1.0.0")
        await XCTAssertThrowsErrorAsync(try await self.pipeline.createRelease(productID: theme.id, source: .folder(path: root.path), kind: .readmeUpdate))
    }

    func testAutomationGateRequiresTheReleasedTag() throws {
        var product = try registerPlugin(mode: .automatic)
        product.automationEnabled = true
        var record = ReleaseRecord(product: product, source: .folder(path: "/tmp/x"), mode: .automatic, kind: .readmeUpdate)
        record.validation = ValidationReport(results: [
            .pass(.versionValidity, "ok"), .pass(.releasedVersion, "ok"), .pass(.productIdentity, "ok"),
        ])
        record.preview = ReleasePreview(changes: [], diffExcerpt: "", diffTruncated: false, commitMessage: "", fingerprint: "f", workingCopyRevision: 1)
        record.repository = RepositorySnapshot(releasePathExists: true, authentication: .keychain)
        XCTAssertEqual(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "f"), [])
        record.repository?.releasePathExists = false
        XCTAssertTrue(AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: "f").contains { $0.contains("released tag") })
    }
}
