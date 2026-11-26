import XCTest
@testable import ReleaseDockCore

/// End-to-end release tests using the real svn CLI against local file:// repositories
/// laid out like WordPress.org. No network and no WordPress.org credentials are used.
final class PipelineIntegrationTests: XCTestCase {
    var repo: LocalSVNRepository!
    var work: TempDir!
    var store: Store!
    var svn: FaultInjectingSVN!
    var notifications: RecordingNotifications!
    var pipeline: ReleasePipelineService!
    var paths: AppPaths!
    var transcript: SVNTranscript!

    override func setUp() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        repo = try LocalSVNRepository()
        work = TempDir()
        store = try Store.inMemory()
        paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        transcript = SVNTranscript(folder: paths.transcripts)
        svn = FaultInjectingSVN(base: SVNService(commandTimeout: .seconds(60), transcript: transcript))
        notifications = RecordingNotifications()
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), notifications: notifications, generator: "tests"
        ))
    }

    func registerPlugin(mode: ReleaseMode = .assisted, automation: Bool = false, initialVersion: String? = "1.0.0", assets: Bool = false) throws -> Product {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: initialVersion)
        var product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path,
            currentVersion: initialVersion, releaseMode: mode, automationEnabled: automation
        )
        if assets {
            work.write("assets/banner-772x250.png", "PNG")
            work.write("assets/icon.svg", "<svg/>")
            product.assetsPath = work.path("assets").path
        }
        try store.save(product)
        return product
    }

    func zipSource(version: String, stableTag: String? = nil, extra: [String: String] = [:]) throws -> ReleaseSource {
        let src = TempDir()
        let plugin = Fixtures.makePlugin(in: src, version: version, stableTag: stableTag, extraFiles: extra)
        let zip = work.path("sample-plugin-\(UUID().uuidString.prefix(4)).zip")
        try Fixtures.zip(folder: plugin, to: zip)
        return .zip(path: zip.path)
    }

    func folderSource(version: String, extra: [String: String] = [:]) -> ReleaseSource {
        let root = Fixtures.makePlugin(in: work, at: "dev-\(UUID().uuidString.prefix(6))/sample-plugin", version: version, extraFiles: extra)
        return .folder(path: root.path)
    }

    // MARK: Plugin

    func testAssistedPluginReleaseFromZIP() async throws {
        let product = try registerPlugin(assets: true)
        let source = try zipSource(version: "1.1.0", extra: [".DS_Store": "x", "node_modules/a.js": "x", "logo@2x.png": "img"])
        var record = try await pipeline.createRelease(productID: product.id, source: source)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? []) \(record.errors)")
        XCTAssertEqual(record.previousVersion, "1.0.0")
        let preview = try XCTUnwrap(record.preview)
        XCTAssertTrue(preview.changes.contains { $0.path == "tags/1.1.0" && $0.isCopy })
        XCTAssertTrue(preview.changes.contains { $0.path == "trunk/logo@2x.png" })
        XCTAssertTrue(preview.changes.contains { $0.path == "assets/banner-772x250.png" })
        XCTAssertFalse(preview.changes.contains { $0.path.contains("node_modules") || $0.path.contains(".DS_Store") })
        XCTAssertTrue(preview.commitMessage.contains(record.commitMarker))

        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        let revision = try XCTUnwrap(record.committedRevision)
        XCTAssertEqual(record.verification?.isClean, true, "\(String(describing: record.verification))")

        // Remote state
        let tagFiles = try repo.run(["list", "-R", "\(repo.url)/sample-plugin/tags/1.1.0"])
        XCTAssertTrue(tagFiles.contains("logo@2x.png"))
        XCTAssertFalse(tagFiles.contains("node_modules"))
        let mime = try repo.run(["propget", "svn:mime-type", "\(repo.url)/sample-plugin/assets/banner-772x250.png"])
        XCTAssertEqual(mime.trimmingCharacters(in: .whitespacesAndNewlines), "image/png")
        let log = try repo.run(["log", "-l", "1", "\(repo.url)/sample-plugin"])
        XCTAssertTrue(log.contains("ReleaseDock-Release-ID: \(record.id.uuidString)"))
        XCTAssertTrue(log.contains("r\(revision)"))

        // Product, manifest, history
        let updated = try XCTUnwrap(try store.product(id: product.id))
        XCTAssertEqual(updated.currentVersion, "1.1.0")
        XCTAssertEqual(updated.lastSuccessfulRelease?.revision, revision)
        let manifest = try String(contentsOfFile: try XCTUnwrap(record.manifestPath), encoding: .utf8)
        XCTAssertTrue(manifest.contains("\"finalRevision\" : \(revision)"))
        XCTAssertTrue(manifest.contains("\"state\" : \"completed\""))
        XCTAssertEqual(try store.events(releaseID: record.id).map(\.to), [.detected, .inspected, .validated, .staged, .previewed, .approved, .publishing, .verifying, .completed])
        XCTAssertTrue(notifications.titles.contains { $0.contains("1.1.0 released") })
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0, "working copy is clean afterwards")
        XCTAssertFalse(FileManager.default.fileExists(atPath: paths.stagingDirectory(for: record.id).path), "staging cleaned up")
    }

    func testFolderReleaseRemovesDeletedFilesAndNeverTouchesSource() async throws {
        let product = try registerPlugin()
        let source = folderSource(version: "1.1.0", extra: [".git/HEAD": "x", "new-file.php": "<?php"])
        // Remove a file that exists in trunk.
        try FileManager.default.removeItem(atPath: source.path + "/includes/class-sample.php")
        let before = try FileManager.default.subpathsOfDirectory(atPath: source.path).sorted()
        var record = try await pipeline.createRelease(productID: product.id, source: source)
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? [])")
        XCTAssertTrue(record.preview!.changes.contains { $0.path == "trunk/includes/class-sample.php" && $0.kind == .deleted })
        XCTAssertTrue(record.preview!.changes.contains { $0.path == "trunk/new-file.php" && $0.kind == .added })
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed)
        XCTAssertEqual(try FileManager.default.subpathsOfDirectory(atPath: source.path).sorted(), before, "source folder untouched")
        let trunk = try repo.run(["list", "-R", "\(repo.url)/sample-plugin/trunk"])
        XCTAssertFalse(trunk.contains("class-sample.php"))
        XCTAssertFalse(trunk.contains(".git"))
    }

    func testDryRunCommitsNothing() async throws {
        let product = try registerPlugin(mode: .dryRun)
        let head = try await svn.info("\(repo.url)/sample-plugin").lastChangedRevision
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .completed)
        XCTAssertNil(record.committedRevision)
        let after = try await svn.info("\(repo.url)/sample-plugin").lastChangedRevision
        XCTAssertEqual(after, head)
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.approve(record.id, kind: .user))
    }

    func testSameVersionAndOlderVersionFailValidation() async throws {
        let product = try registerPlugin()
        for version in ["1.0.0", "0.9.0"] {
            var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: version))
            record = try await pipeline.prepare(record.id)
            XCTAssertEqual(record.state, .failed)
            XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .versionIncrease || $0.check == .existingTag })
        }
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testInvalidZIPFails() async throws {
        let product = try registerPlugin()
        let bad = work.write("bad.zip", "not a zip")
        var record = try await pipeline.createRelease(productID: product.id, source: .zip(path: bad.path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .sourceIntegrity })
    }

    func testExistingTagBlocks() async throws {
        let product = try registerPlugin()
        try repo.run(["copy", "-m", "manual tag", "\(repo.url)/sample-plugin/trunk", "\(repo.url)/sample-plugin/tags/1.1.0"])
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .existingTag })
    }

    func testDirtyWorkingCopyBlocksAndCanBeRevertedWithBackup() async throws {
        let product = try registerPlugin()
        var first = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        first = try await pipeline.prepare(first.id)
        first = try await pipeline.cancel(first.id)
        XCTAssertEqual(first.state, .cancelled)

        let readme = product.workingCopyURL.appendingPathComponent("trunk/readme.txt")
        try Data("local edit".utf8).write(to: readme)
        try Data("stray".utf8).write(to: product.workingCopyURL.appendingPathComponent("trunk/stray.txt"))
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .workingCopyClean })
        XCTAssertEqual(try String(contentsOf: readme, encoding: .utf8), "local edit", "a dirty working copy is never modified automatically")

        let backup = try await pipeline.revertWorkingCopy(productID: product.id)
        let backupText = try String(contentsOf: try XCTUnwrap(backup), encoding: .utf8)
        XCTAssertTrue(backupText.contains("local edit"))
        XCTAssertTrue(FileManager.default.fileExists(atPath: backup!.deletingPathExtension().appendingPathExtension("files").appendingPathComponent("trunk/stray.txt").path))
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testCancellingAnUntouchedStagedReleaseWritesNoBackup() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.cancel(record.id)
        XCTAssertEqual(record.state, .cancelled)
        XCTAssertEqual((try? FileManager.default.contentsOfDirectory(atPath: paths.backups.path)) ?? [], [])
        let state = try await pipeline.workingCopyState(productID: product.id)
        XCTAssertTrue(state.isClean, "\(state.changes)")

        // A hand edit after the preview is the user's work: cancelling backs it up.
        record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        try Data("hand edit".utf8).write(to: product.workingCopyURL.appendingPathComponent("trunk/includes/class-sample.php"))
        _ = try await pipeline.cancel(record.id)
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: paths.backups.path).contains { $0.hasSuffix("-cancelled.diff") })
    }

    /// Prepares a working copy whose trunk/readme.txt conflicts with a server change.
    private func makeConflict(_ product: Product) async throws -> URL {
        _ = try await pipeline.updateWorkingCopy(productID: product.id)
        let readme = product.workingCopyURL.appendingPathComponent("trunk/readme.txt")
        try Data("local edit\n".utf8).write(to: readme)
        let other = work.path("other-wc-\(UUID().uuidString.prefix(4))")
        try repo.run(["checkout", "\(repo.url)/sample-plugin/trunk", other.path])
        try Data("server edit\n".utf8).write(to: other.appendingPathComponent("readme.txt"))
        try repo.run(["commit", "-m", "external", other.path])
        return readme
    }

    func testUpdateWorkingCopyReportsConflictsAndServerResolutionBacksUp() async throws {
        let product = try registerPlugin()
        let readme = try await makeConflict(product)
        let state = try await pipeline.updateWorkingCopy(productID: product.id)
        XCTAssertEqual(state.conflicts.map(\.path), ["trunk/readme.txt"])

        let (resolved, backup) = try await pipeline.resolveConflicts(productID: product.id, paths: ["trunk/readme.txt"], keeping: .server)
        XCTAssertTrue(resolved.isClean, "\(resolved.changes)")
        XCTAssertEqual(try String(contentsOf: readme, encoding: .utf8), "server edit\n")
        let saved = try XCTUnwrap(backup).deletingPathExtension().appendingPathExtension("files").appendingPathComponent("trunk/readme.txt")
        XCTAssertTrue(try String(contentsOf: saved, encoding: .utf8).contains("local edit"))
    }

    func testResolvingConflictKeepingMineLeavesALocalChange() async throws {
        let product = try registerPlugin()
        let readme = try await makeConflict(product)
        _ = try await pipeline.updateWorkingCopy(productID: product.id)
        let (resolved, backup) = try await pipeline.resolveConflicts(productID: product.id, paths: ["trunk/readme.txt"], keeping: .mine)
        XCTAssertNil(backup)
        XCTAssertEqual(resolved.conflicts, [])
        XCTAssertEqual(resolved.changes.map(\.path), ["trunk/readme.txt"])
        XCTAssertEqual(try String(contentsOf: readme, encoding: .utf8), "local edit\n")
    }

    func testUpdateIsRefusedWhileAReleaseIsStaged() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.updateWorkingCopy(productID: product.id))
    }

    func testRemoteChangeAfterApprovalBlocksPublishing() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        // Someone commits to trunk in the meantime.
        let other = work.path("other-wc")
        try repo.run(["checkout", "\(repo.url)/sample-plugin/trunk", other.path])
        try Data("changed elsewhere".utf8).write(to: other.appendingPathComponent("readme.txt"))
        try repo.run(["commit", "-m", "external", other.path])

        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .failed)
        XCTAssertNil(try? repo.run(["info", "\(repo.url)/sample-plugin/tags/1.1.0"]), "nothing was published")
        XCTAssertEqual(svn.commitCount, 0)
    }

    func testPreviewChangeAfterApprovalRequiresReapproval() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        try Data("tampered".utf8).write(to: product.workingCopyURL.appendingPathComponent("trunk/includes/class-sample.php"))
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        XCTAssertEqual(try store.release(id: record.id)?.state, .previewed)
        XCTAssertEqual(svn.commitCount, 0)
    }

    func testAuthenticationFailureCommitsNothingAndAllowsRetry() async throws {
        let product = try registerPlugin()
        svn.commitFault = .authenticationFailure
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .previewed)
        XCTAssertNil(record.approval)

        svn.commitFault = .none
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed)
    }

    func testMissingCredentialsBlockPublishing() async throws {
        let product = try registerPlugin()
        var withRemoteURL = product
        // Simulate a remote https repository with no stored or cached credentials.
        svn.cachedCredentials = false
        withRemoteURL.svnUsername = nil
        try store.save(withRemoteURL)
        let auth = await pipeline.authenticationAvailability(for: Product(name: "x", slug: "x", type: .plugin, svnRepositoryURL: URL(string: "https://plugins.svn.wordpress.org/x")!, workingCopyPath: "/tmp/x"))
        XCTAssertEqual(auth, .none)
    }

    func testHookRejectionFailsAndRestoresWorkingCopy() async throws {
        let product = try registerPlugin()
        svn.commitFault = .hookRejection
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .failed)
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testCommitTimeoutAfterServerCommitIsRecoveredWithoutRecommitting() async throws {
        let product = try registerPlugin()
        svn.commitFault = .uncertainAfterCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(svn.commitCount, 1, "never retried blindly")
        XCTAssertNotNil(record.committedRevision)
        let states = try store.events(releaseID: record.id).map(\.to)
        XCTAssertTrue(states.contains(.recoveryRequired))
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testCommitTimeoutWithoutServerCommitRequiresExplicitRetry() async throws {
        let product = try registerPlugin()
        svn.commitFault = .uncertainWithoutCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .recoveryRequired)
        XCTAssertTrue(record.verifiedNotCommitted)
        XCTAssertEqual(svn.commitCount, 1)

        await XCTAssertThrowsErrorAsync(try await self.pipeline.approve(record.id, kind: .automatic), "automatic retry is never allowed")
        svn.commitFault = .none
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed)
        XCTAssertEqual(svn.commitCount, 2)
    }

    func testNetworkFailureDuringCommitIsTreatedAsUncertain() async throws {
        let product = try registerPlugin()
        svn.commitFault = .networkFailureBeforeCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .recoveryRequired)
        record = try await pipeline.abandon(record.id)
        XCTAssertEqual(record.state, .failed)
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testNetworkFailureDuringValidation() async throws {
        let product = try registerPlugin()
        svn.failInfoWithNetworkError = true
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .svnConnectivity })
    }

    func testDuplicateReleaseIsRejected() async throws {
        let product = try registerPlugin()
        var first = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        first = try await pipeline.prepare(first.id)
        _ = try await pipeline.approve(first.id, kind: .user)
        first = try await pipeline.publish(first.id)
        XCTAssertEqual(first.state, .completed)
        // Publishing the same version again.
        var second = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        second = try await pipeline.prepare(second.id)
        XCTAssertEqual(second.state, .failed)
        XCTAssertTrue(second.validation!.blockingResults.contains { $0.check == .existingTag })
        // Re-publishing a completed release is refused.
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(first.id))
    }

    func testAppRestartDuringRelease() async throws {
        let product = try registerPlugin()
        var staged = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        staged = try await pipeline.prepare(staged.id)
        XCTAssertTrue(staged.workingCopyModified)

        // Simulate a crash in the middle of publishing (state persisted, commit landed).
        var publishing = try XCTUnwrap(try store.release(id: staged.id))
        publishing.approval = Approval(kind: .user, previewFingerprint: publishing.preview!.fingerprint)
        try ReleaseStateMachine.transition(&publishing, to: .approved)
        try ReleaseStateMachine.transition(&publishing, to: .publishing)
        try store.save(publishing)
        _ = try await svn.commit([product.workingCopyURL], message: publishing.preview!.commitMessage, credentials: nil)

        // "Restart": a fresh pipeline reconciles interrupted releases.
        let restarted = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), generator: "tests"
        ))
        let reconciled = try await restarted.reconcileInterruptedReleases()
        XCTAssertEqual(reconciled.map(\.state), [.recoveryRequired])
        let recovered = try await restarted.recover(staged.id)
        XCTAssertEqual(recovered.state, .completed, "\(recovered.errors)")
        XCTAssertNotNil(recovered.committedRevision)
    }

    func testRestartBeforePublishingCancelsAndRestoresWorkingCopy() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        let pendingCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertGreaterThan(pendingCount, 0)
        let reconciled = try await pipeline.reconcileInterruptedReleases()
        XCTAssertEqual(reconciled.map(\.state), [.cancelled])
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
    }

    func testAutomaticReleaseRequiresExplicitOptIn() async throws {
        let product = try registerPlugin(mode: .automatic, automation: false)
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publishAutomatically(record.id))
        XCTAssertEqual(svn.commitCount, 0)

        var enabled = try XCTUnwrap(try store.product(id: product.id))
        enabled.automationEnabled = true
        try store.save(enabled)
        record = try await pipeline.publishAutomatically(record.id)
        XCTAssertEqual(record.state, .completed)
        XCTAssertEqual(record.approval?.kind, .automatic)
    }

    func testFirstReleaseIntoEmptyPluginRepository() async throws {
        let product = try registerPlugin(initialVersion: nil)
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.0.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? [])")
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed)
    }

    func testFileAndDirectoryTypeChanges() async throws {
        let product = try registerPlugin()
        let src = TempDir()
        let root = Fixtures.makePlugin(in: src, version: "1.1.0")
        // includes/ (a directory in 1.0.0) becomes a file; assets/js/app.js (a file) becomes a directory.
        try FileManager.default.removeItem(at: root.appendingPathComponent("includes"))
        try Data("<?php // now a file\n".utf8).write(to: root.appendingPathComponent("includes"))
        try FileManager.default.removeItem(at: root.appendingPathComponent("assets/js/app.js"))
        src.write("sample-plugin/assets/js/app.js/index.js", "console.log('dir');\n")

        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: root.path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.errors) \(record.validation?.blockingResults ?? [])")
        XCTAssertTrue(record.preview!.changes.contains { $0.path == "trunk/includes" && $0.kind == .replaced })
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.verification?.isClean, true, "\(String(describing: record.verification))")
        let listing = try repo.run(["list", "-R", "\(repo.url)/sample-plugin/tags/1.1.0"])
        XCTAssertTrue(listing.contains("assets/js/app.js/index.js"))
        XCTAssertTrue(listing.components(separatedBy: "\n").contains("includes"))
    }

    // MARK: Audit regressions

    func testRevertBacksUpBinaryAndAddedFilesBeforeDeleting() async throws {
        let product = try registerPlugin()
        // Prime the working copy with a successful dry-run style preparation, then cancel.
        var first = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        first = try await pipeline.prepare(first.id)
        _ = try await pipeline.cancel(first.id)

        let wc = product.workingCopyURL
        let binary = Data([0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0x00, 0x10])
        try binary.write(to: wc.appendingPathComponent("trunk/new-image.png"))
        try repo.run(["add", wc.appendingPathComponent("trunk/new-image.png").path])
        try binary.write(to: wc.appendingPathComponent("trunk/includes/class-sample.php"))

        let backup = try await pipeline.revertWorkingCopy(productID: product.id)
        let files = try XCTUnwrap(backup).deletingPathExtension().appendingPathExtension("files")
        XCTAssertEqual(try Data(contentsOf: files.appendingPathComponent("trunk/new-image.png")), binary, "added binary file backed up")
        XCTAssertEqual(try Data(contentsOf: files.appendingPathComponent("trunk/includes/class-sample.php")), binary, "modified file backed up")
        XCTAssertFalse(FileManager.default.fileExists(atPath: wc.appendingPathComponent("trunk/new-image.png").path))
        let remaining = try await WorkingCopyOperations.localChanges(at: wc, svn: svn)
        XCTAssertEqual(remaining.count, 0)
    }

    func testIgnoredFilesInWorkingCopyBlockReleaseButJunkDoesNot() async throws {
        let product = try registerPlugin()
        var warmup = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        warmup = try await pipeline.prepare(warmup.id)
        _ = try await pipeline.cancel(warmup.id)

        let wc = product.workingCopyURL
        try Data("junk".utf8).write(to: wc.appendingPathComponent("trunk/.DS_Store"))
        var junkOnly = try await WorkingCopyOperations.localChanges(at: wc, svn: svn)
        XCTAssertEqual(junkOnly, [], "OS junk does not make the working copy dirty")
        try Data("compiled".utf8).write(to: wc.appendingPathComponent("trunk/library.a")) // matches svn global-ignores
        junkOnly = try await WorkingCopyOperations.localChanges(at: wc, svn: svn)
        XCTAssertTrue(junkOnly.contains { $0.contains("library.a") })
    }

    func testPreviewChangeAfterApprovalKeepsTheGitCheckOfFolderReleases() async throws {
        var product = try registerPlugin()
        product.validation.requireCleanGitTree = true
        try store.save(product)
        let clean = GitRepositoryState(isRepository: true, headCommit: "abc123", branch: "main", exactTag: nil, uncommittedChanges: [], remoteURL: nil)
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(state: clean), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), notifications: notifications, generator: "tests"
        ))
        var record = try await pipeline.createRelease(productID: product.id, source: folderSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? [])")
        record = try await pipeline.approve(record.id, kind: .user)
        try Data("tampered".utf8).write(to: product.workingCopyURL.appendingPathComponent("trunk/includes/class-sample.php"))
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? [])")
        XCTAssertTrue(record.validation!.results.contains { $0.check == .gitWorkingTree && $0.status == .passed })
    }

    func testPreviewChangeAfterApprovalIsRevalidated() async throws {
        let product = try registerPlugin()
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        // An edit outside the allowed release paths: modify the already released tag.
        let oldTag = product.workingCopyURL.appendingPathComponent("tags/1.0.0")
        try repo.run(["update", "--set-depth", "infinity", oldTag.path])
        try Data("tampered".utf8).write(to: oldTag.appendingPathComponent("readme.txt"))
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .failed, "the changed preview fails release-diff validation")
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.check == .releaseDiff })
        XCTAssertEqual(svn.commitCount, 0)
    }

    func testVerifiedNotCommittedIsClearedOnRetry() async throws {
        let product = try registerPlugin()
        svn.commitFault = .uncertainWithoutCommit
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertTrue(record.verifiedNotCommitted)
        // Retry also ends uncertain, and the repository cannot be inspected this time.
        record = try await pipeline.approve(record.id, kind: .user)
        XCTAssertFalse(record.verifiedNotCommitted, "a new attempt invalidates the earlier verification")
        svn.commitFault = .uncertainAfterCommit
        svn.failLogWithNetworkError = true
        await XCTAssertThrowsErrorAsync(try await self.pipeline.publish(record.id))
        record = try XCTUnwrap(try store.release(id: record.id))
        XCTAssertEqual(record.state, .recoveryRequired)
        XCTAssertFalse(record.verifiedNotCommitted)
        await XCTAssertThrowsErrorAsync(try await self.pipeline.abandon(record.id), "cannot abandon an unverified outcome")
        svn.failLogWithNetworkError = false
        record = try await pipeline.recover(record.id)
        XCTAssertEqual(record.state, .completed)
    }

    // MARK: Theme

    func testThemeReleaseCopiesPreviousVersion() async throws {
        try repo.createTheme(slug: "sample-theme", initialVersion: "1.0.0")
        let product = Product(name: "Sample Theme", slug: "sample-theme", type: .theme, svnRepositoryURL: URL(string: "\(repo.url)/sample-theme")!, workingCopyPath: work.path("wc-theme").path)
        try store.save(product)
        let src = TempDir()
        let theme = Fixtures.makeTheme(in: src, version: "1.0.1")
        try FileManager.default.removeItem(at: theme.appendingPathComponent("functions.php"))
        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: theme.path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .previewed, "\(record.validation?.blockingResults ?? [])")
        XCTAssertTrue(record.preview!.changes.contains { $0.path == "1.0.1" && $0.isCopy })
        XCTAssertTrue(record.preview!.changes.allSatisfy { $0.path == "1.0.1" || $0.path.hasPrefix("1.0.1/") })
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.verification?.isClean, true)
        let listing = try repo.run(["list", "\(repo.url)/sample-theme/1.0.1"])
        XCTAssertFalse(listing.contains("functions.php"))
        let history = try repo.run(["log", "--stop-on-copy", "-v", "\(repo.url)/sample-theme/1.0.1"])
        XCTAssertTrue(history.contains("(from /sample-theme/1.0.0"), "copied with history")
        XCTAssertFalse(record.guidance.isEmpty)
    }

    func testThemeWithoutExistingVersionIsBlocked() async throws {
        try repo.createTheme(slug: "sample-theme", initialVersion: nil)
        let product = Product(name: "Sample Theme", slug: "sample-theme", type: .theme, svnRepositoryURL: URL(string: "\(repo.url)/sample-theme")!, workingCopyPath: work.path("wc-theme").path)
        try store.save(product)
        let src = TempDir()
        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: Fixtures.makeTheme(in: src, version: "1.0.0").path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertTrue(record.validation!.blockingResults.contains { $0.message.contains("upload") || $0.message.contains("review") })
    }
}

func XCTAssertThrowsErrorAsync<T>(_ expression: @autoclosure () async throws -> T, _ message: String = "", file: StaticString = #filePath, line: UInt = #line) async {
    do {
        _ = try await expression()
        XCTFail("Expected an error. \(message)", file: file, line: line)
    } catch {}
}
