import XCTest
@testable import ReleaseDockCore

func makeProduct(type: ProductType = .plugin, slug: String = "sample-plugin", url: String? = nil, workingCopy: String = "/tmp/wc", mode: ReleaseMode = .assisted, automation: Bool = false) -> Product {
    Product(
        name: type == .plugin ? "Sample Plugin" : "Sample Theme",
        slug: slug,
        type: type,
        svnRepositoryURL: URL(string: url ?? "https://\(type == .plugin ? "plugins" : "themes").svn.wordpress.org/\(slug)")!,
        workingCopyPath: workingCopy,
        releaseMode: mode,
        automationEnabled: automation
    )
}

final class ReleaseStateMachineTests: XCTestCase {
    func testHappyPath() throws {
        var record = ReleaseRecord(product: makeProduct(), source: .zip(path: "/x.zip"), mode: .assisted)
        for state: ReleaseState in [.inspected, .validated, .staged, .previewed, .approved, .publishing, .verifying, .completed] {
            try ReleaseStateMachine.transition(&record, to: state)
        }
        XCTAssertEqual(record.state, .completed)
        XCTAssertEqual(record.transitions.count, 9)
        XCTAssertEqual(record.transitions.last?.from, .verifying)
    }

    func testInvalidTransitions() {
        var record = ReleaseRecord(product: makeProduct(), source: .zip(path: "/x.zip"), mode: .assisted)
        XCTAssertThrowsError(try ReleaseStateMachine.transition(&record, to: .publishing))
        XCTAssertThrowsError(try ReleaseStateMachine.transition(&record, to: .completed))
        XCTAssertFalse(ReleaseStateMachine.canTransition(from: .publishing, to: .cancelled, mode: .assisted), "publishing cannot be cancelled")
        XCTAssertFalse(ReleaseStateMachine.canTransition(from: .completed, to: .failed, mode: .assisted))
        XCTAssertFalse(ReleaseStateMachine.canTransition(from: .verifying, to: .failed, mode: .assisted), "a committed release cannot be marked failed")
        XCTAssertFalse(ReleaseStateMachine.canTransition(from: .recoveryRequired, to: .publishing, mode: .assisted), "no blind retry")
    }

    func testDryRunNeverPublishes() throws {
        var record = ReleaseRecord(product: makeProduct(), source: .folder(path: "/x"), mode: .dryRun)
        for state: ReleaseState in [.inspected, .validated, .staged, .previewed] { try ReleaseStateMachine.transition(&record, to: state) }
        XCTAssertThrowsError(try ReleaseStateMachine.transition(&record, to: .approved)) { error in
            XCTAssertEqual(error as? ReleaseStateError, .dryRunCannotPublish)
        }
        try ReleaseStateMachine.transition(&record, to: .completed)
    }

    func testInterruptionMapping() {
        XCTAssertEqual(ReleaseStateMachine.stateAfterInterruption(.staged), .cancelled)
        XCTAssertEqual(ReleaseStateMachine.stateAfterInterruption(.approved), .cancelled)
        XCTAssertEqual(ReleaseStateMachine.stateAfterInterruption(.publishing), .recoveryRequired)
        XCTAssertEqual(ReleaseStateMachine.stateAfterInterruption(.verifying), .recoveryRequired)
        XCTAssertNil(ReleaseStateMachine.stateAfterInterruption(.completed))
        XCTAssertNil(ReleaseStateMachine.stateAfterInterruption(.recoveryRequired))
    }

    func testNotesAreRedacted() throws {
        var record = ReleaseRecord(product: makeProduct(), source: .zip(path: "/x.zip"), mode: .assisted)
        try ReleaseStateMachine.transition(&record, to: .failed, note: "svn --password hunter2")
        XCTAssertFalse(record.transitions.last!.note!.contains("hunter2"))
    }
}

final class StoreTests: XCTestCase {
    func testProductsReleasesAndEvents() throws {
        let store = try Store.inMemory()
        var product = makeProduct()
        try store.save(product)
        product.currentVersion = "1.0.0"
        try store.save(product)
        XCTAssertEqual(try store.allProducts().map(\.currentVersion), ["1.0.0"])

        var duplicate = makeProduct()
        duplicate.id = UUID()
        XCTAssertThrowsError(try store.save(duplicate))

        var record = ReleaseRecord(product: product, source: .zip(path: "/p.zip"), mode: .assisted)
        try store.save(record)
        try ReleaseStateMachine.transition(&record, to: .inspected, note: "ok")
        try store.save(record)
        try ReleaseStateMachine.transition(&record, to: .validated)
        try store.save(record)
        try store.save(record) // idempotent: no duplicate events
        XCTAssertEqual(try store.events(releaseID: record.id).map(\.to), [.detected, .inspected, .validated])
        XCTAssertEqual(try store.activeReleases().map(\.id), [record.id])
        XCTAssertEqual(try store.release(id: record.id)?.state, .validated)

        try store.setSetting(["a": 1], forKey: "k")
        XCTAssertEqual(try store.setting([String: Int].self, forKey: "k"), ["a": 1])

        try store.deleteProduct(id: product.id)
        XCTAssertNil(try store.release(id: record.id), "releases cascade with their product")
    }

    func testMigrationsBackupAndRefuseNewerSchema() throws {
        let dir = TempDir()
        let paths = AppPaths(root: dir.path("data"))
        _ = try Store.open(paths: paths)
        let db = try SQLiteDatabase(url: paths.databaseFile)
        XCTAssertEqual(try db.userVersion, Migrator.latestVersion)
        try db.execute("PRAGMA user_version = 999")
        XCTAssertThrowsError(try Store(database: db, backupDirectory: paths.backups)) { error in
            guard case DatabaseError.newerSchema = error else { return XCTFail("\(error)") }
        }
    }

    func testDatabaseFileIsPrivate() throws {
        let dir = TempDir()
        let paths = AppPaths(root: dir.path("data"))
        _ = try Store.open(paths: paths)
        let perms = try FileManager.default.attributesOfItem(atPath: paths.databaseFile.path)[.posixPermissions] as? Int
        XCTAssertEqual(perms, 0o600)
        let dirPerms = try FileManager.default.attributesOfItem(atPath: paths.root.path)[.posixPermissions] as? Int
        XCTAssertEqual(dirPerms, 0o700)
    }
}

final class ManifestTests: XCTestCase {
    func testManifestContainsReleaseDataAndNoSecrets() throws {
        var product = makeProduct()
        product.svnUsername = "alice"
        var record = ReleaseRecord(product: product, source: .zip(path: "/tmp/p.zip"), mode: .assisted)
        record.version = "1.2.0"
        record.sourceSHA256 = String(repeating: "a", count: 64)
        record.revisionBefore = 100
        record.committedRevision = 101
        record.validation = ValidationReport(results: [.pass(.versionValidity, "ok")])
        let candidate = StagedCandidate(rootPath: "/tmp/c", entries: [PackageEntry(path: "a.php", kind: .file, size: 3, sha256: "abc")], treeDigest: "digest")
        let manifest = ReleaseManifest.make(record: record, candidate: candidate, releasePath: "tags/1.2.0", gitCommit: "deadbeef", generator: "test")
        let json = String(decoding: try manifest.encoded(), as: UTF8.self)
        XCTAssertTrue(json.contains("\"svnRevisionBefore\" : 100"))
        XCTAssertTrue(json.contains("\"finalRevision\" : 101"))
        XCTAssertTrue(json.contains("tags/1.2.0"))
        XCTAssertTrue(json.contains("a.php"))
        for forbidden in ["password", "alice", "token"] {
            XCTAssertFalse(json.lowercased().contains(forbidden), "manifest must not contain \(forbidden)")
        }
    }
}

final class ProductServiceTests: XCTestCase {
    func testValidation() throws {
        let dir = TempDir()
        let service = ProductService(repository: try Store.inMemory(), paths: AppPaths(root: dir.path("data")))
        var product = makeProduct(workingCopy: dir.path("wc").path)
        XCTAssertFalse(service.validate(product).contains(where: \.isError), "\(service.validate(product))")

        product.svnRepositoryURL = URL(string: "https://plugins.svn.wordpress.org/other-slug")!
        XCTAssertTrue(service.validate(product).contains { $0.field == .svnURL && $0.isError }, "URL must match slug")
        product.svnRepositoryURL = URL(string: "https://user:pw@plugins.svn.wordpress.org/sample-plugin")!
        XCTAssertTrue(service.validate(product).contains { $0.field == .svnURL && $0.isError }, "no credentials in URL")
        product.svnRepositoryURL = URL(string: "http://plugins.svn.wordpress.org/sample-plugin")!
        XCTAssertTrue(service.validate(product).contains { $0.field == .svnURL && $0.isError })
        product.svnRepositoryURL = URL(string: "https://themes.svn.wordpress.org/sample-plugin")!
        XCTAssertTrue(service.validate(product).contains { $0.field == .svnURL && $0.isError }, "plugin on themes host")

        product = makeProduct(workingCopy: dir.path("dev/wc").path)
        product.localDevelopmentPath = dir.path("dev").path
        XCTAssertTrue(service.validate(product).contains { $0.field == .workingCopy && $0.isError }, "no nesting")
        product = makeProduct(slug: "Bad Slug", workingCopy: dir.path("wc").path)
        XCTAssertTrue(service.validate(product).contains { $0.field == .slug })
        XCTAssertThrowsError(try service.save(product))
    }

    func testSuggestionFromFolder() throws {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "3.0.0")
        let suggestion = try ProductService(repository: try Store.inMemory(), paths: AppPaths(root: dir.path("d"))).suggestion(forFolder: root)
        XCTAssertEqual(suggestion.type, .plugin)
        XCTAssertEqual(suggestion.slug, "sample-plugin")
        XCTAssertEqual(suggestion.version, "3.0.0")
    }
}

final class ProcessRunnerTests: XCTestCase {
    func testArgumentsAreNotInterpretedByAShell() async throws {
        let dir = TempDir()
        let marker = dir.path("pwned")
        let result = try await ProcessRunner().run(ProcessRequest(executableURL: URL(fileURLWithPath: "/bin/echo"), arguments: ["$(touch \(marker.path))", "; rm -rf /", "`id`"]))
        XCTAssertEqual(result.stdout, "$(touch \(marker.path)) ; rm -rf / `id`\n")
        XCTAssertFalse(FileManager.default.fileExists(atPath: marker.path))
    }

    func testStdinAndCapture() async throws {
        let result = try await ProcessRunner().run(ProcessRequest(executableURL: URL(fileURLWithPath: "/bin/cat"), arguments: [], standardInput: Data("secret\n".utf8)))
        XCTAssertEqual(result.stdout, "secret\n")
        XCTAssertEqual(result.exitCode, 0)
        XCTAssertGreaterThan(result.duration, .zero)
    }

    func testTimeout() async throws {
        do {
            _ = try await ProcessRunner(killGracePeriod: .seconds(1)).run(ProcessRequest(executableURL: URL(fileURLWithPath: "/bin/sleep"), arguments: ["10"], timeout: .milliseconds(300)))
            XCTFail("expected timeout")
        } catch ProcessRunnerError.timedOut(let partial) {
            XCTAssertLessThan(partial.duration, .seconds(5))
        }
    }

    func testCancellation() async throws {
        let task = Task {
            try await ProcessRunner().run(ProcessRequest(executableURL: URL(fileURLWithPath: "/bin/sleep"), arguments: ["10"]))
        }
        try await Task.sleep(for: .milliseconds(200))
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch ProcessRunnerError.cancelled {}
    }

    func testRedactedDisplay() {
        let request = ProcessRequest(executableURL: URL(fileURLWithPath: "/bin/x"), arguments: ["--password", "pw"], redactedArgumentIndices: [1])
        XCTAssertEqual(request.displayCommand, "/bin/x --password <redacted>")
    }
}

final class SVNParsingTests: XCTestCase {
    func testPegRevisionEscaping() {
        XCTAssertEqual(SVNService.target("/wc/trunk/logo@2x.png"), "/wc/trunk/logo@2x.png@")
        XCTAssertEqual(SVNService.target("/wc/trunk/a.php"), "/wc/trunk/a.php")
    }

    func testErrorClassification() {
        func kind(_ stderr: String) -> SVNErrorKind {
            SVNErrorKind.classify(codes: SVNService.errorCodes(in: stderr), stderr: stderr)
        }
        XCTAssertEqual(kind("svn: E170001: Authorization failed"), .authenticationFailed)
        XCTAssertEqual(kind("svn: E215004: No more credentials or we tried too many times."), .authenticationRequired)
        XCTAssertEqual(kind("svn: E170013: Unable to connect to a repository at URL\nsvn: E670008: nodename nor servname provided"), .network)
        XCTAssertEqual(kind("svn: E165001: Commit blocked by pre-commit hook (exit code 1) with output:"), .hookRejected)
        XCTAssertEqual(kind("svn: E155011: File '/x' is out of date"), .outOfDate)
        XCTAssertEqual(kind("svn: warning: W170000: URL 'x' non-existent in revision 5\nsvn: E200009: Could not display info"), .notFound)
        XCTAssertEqual(kind("svn: E230001: Server SSL certificate verification failed"), .certificate)
        XCTAssertEqual(kind("svn: E155004: Working copy locked"), .workingCopyLocked)
    }

    func testCommittedRevisionParsing() {
        XCTAssertEqual(SVNService.committedRevision(in: "Sending  a\nTransmitting file data .done\nCommitting transaction...\nCommitted revision 3816107.\n"), 3816107)
        XCTAssertNil(SVNService.committedRevision(in: "Sending a\n"))
    }

    func testStatusXMLParsing() throws {
        let xml = """
        <?xml version="1.0" encoding="UTF-8"?>
        <status><target path="/wc">
        <entry path="/wc/trunk/a.php"><wc-status item="modified" props="none" revision="5"></wc-status></entry>
        <entry path="/wc/tags/1.0"><wc-status item="added" props="none" copied="true"></wc-status></entry>
        <entry path="/wc/trunk/b.php"><wc-status item="normal" props="none" revision="5"></wc-status><repos-status item="modified" props="none"></repos-status></entry>
        </target></status>
        """
        let entries = SVNService.parseStatus(try XMLElement.parse(Data(xml.utf8)))
        XCTAssertEqual(entries.count, 3)
        XCTAssertTrue(entries[0].isLocalChange)
        XCTAssertTrue(entries[1].copied)
        XCTAssertTrue(entries[2].isOutOfDate)
        XCTAssertFalse(entries[2].isLocalChange)
    }
}

final class KeychainTests: XCTestCase {
    func testInMemoryKeychain() throws {
        let keychain = InMemoryKeychain()
        let item = KeychainItem.svn(host: "plugins.svn.wordpress.org", username: "alice")
        XCTAssertNil(try keychain.password(for: item))
        try keychain.setPassword("pw", for: item)
        XCTAssertEqual(try keychain.password(for: item), "pw")
        try keychain.deletePassword(for: item)
        XCTAssertNil(try keychain.password(for: item))
    }

    /// Exercises the real login keychain; opt in with RELEASEDOCK_KEYCHAIN_TESTS=1.
    func testRealKeychainRoundTrip() throws {
        guard ProcessInfo.processInfo.environment["RELEASEDOCK_KEYCHAIN_TESTS"] == "1" else { throw XCTSkip("Set RELEASEDOCK_KEYCHAIN_TESTS=1") }
        let keychain = KeychainService()
        let item = KeychainItem(service: "com.releasedock.tests.\(UUID().uuidString)", account: "tester")
        defer { try? keychain.deletePassword(for: item) }
        try keychain.setPassword("first", for: item)
        try keychain.setPassword("second", for: item)
        XCTAssertEqual(try keychain.password(for: item), "second")
    }
}
