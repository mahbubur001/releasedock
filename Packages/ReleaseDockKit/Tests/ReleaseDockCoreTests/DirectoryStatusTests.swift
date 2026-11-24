import XCTest
@testable import ReleaseDockCore

final class StubWordPressOrg: WordPressOrgServicing, @unchecked Sendable {
    private let lock = NSLock()
    private var _listing: DirectoryListing?
    private var _requests = 0
    var listing: DirectoryListing? {
        get { lock.withLock { _listing } }
        set { lock.withLock { _listing = newValue } }
    }
    var requests: Int { lock.withLock { _requests } }

    init(listing: DirectoryListing?) { _listing = listing }

    func latestWordPressVersion() async throws -> String { "7.1.5" }
    func directoryListing(slug: String, type: ProductType) async throws -> DirectoryListing? {
        lock.withLock {
            _requests += 1
            return _listing
        }
    }
}

final class DirectoryStatusTests: XCTestCase {
    func testParsesDirectoryListing() throws {
        let json = #"{"slug":"akismet","version":"5.7.2","tested":"7.1.5","last_updated":"2026-08-18 11:42pm GMT"}"#
        let listing = try WordPressOrgService.parseListing(Data(json.utf8))
        XCTAssertEqual(listing, DirectoryListing(version: "5.7.2", tested: "7.1.5", lastUpdated: "2026-08-18 11:42pm GMT"))
        XCTAssertThrowsError(try WordPressOrgService.parseListing(Data(#"{"error":"Plugin not found."}"#.utf8)))
    }

    func testStates() {
        XCTAssertEqual(DirectoryStatus.state(released: "2.4.0", listing: DirectoryListing(version: "2.4.0")), .live)
        XCTAssertEqual(DirectoryStatus.state(released: "2.4.0", listing: DirectoryListing(version: "2.3.0")), .pending)
        XCTAssertEqual(DirectoryStatus.state(released: "2.4.0", listing: DirectoryListing(version: "2.5.0")), .superseded)
        XCTAssertEqual(DirectoryStatus.state(released: "2.4.0", listing: nil), .notListed)
    }

    let product = Product(name: "Acme Gallery", slug: "acme-gallery", type: .plugin, svnRepositoryURL: URL(string: "https://plugins.svn.wordpress.org/acme-gallery/")!, workingCopyPath: "/tmp/wc-acme")

    func record(url: String = "https://plugins.svn.wordpress.org/acme-gallery/", mode: ReleaseMode = .assisted, kind: ReleaseKind = .version, completedAt: Date = Date()) -> ReleaseRecord {
        var product = product
        product.svnRepositoryURL = URL(string: url)!
        var record = ReleaseRecord(product: product, source: .zip(path: "/tmp/acme.zip"), mode: mode, kind: kind, createdAt: completedAt)
        record.version = "2.4.0"
        record.state = .completed
        record.transitions.append(StateTransition(from: .verifying, to: .completed, at: completedAt))
        return record
    }

    func testOnlyCommittedWordPressOrgVersionReleasesAreTracked() {
        XCTAssertTrue(record().isDirectoryTracked)
        XCTAssertTrue(record(url: "https://themes.svn.wordpress.org/acme-theme/").isDirectoryTracked)
        XCTAssertFalse(record(url: "file:///tmp/repo/acme-gallery").isDirectoryTracked)
        XCTAssertFalse(record(mode: .dryRun).isDirectoryTracked)
        XCTAssertFalse(record(kind: .readmeUpdate).isDirectoryTracked)
        var failed = record()
        failed.state = .failed
        XCTAssertFalse(failed.isDirectoryTracked)
    }

    func testAwaitingWindowAndFinalStates() {
        let now = Date()
        XCTAssertTrue(record(completedAt: now.addingTimeInterval(-3600)).awaitsDirectory(now: now))
        XCTAssertFalse(record(completedAt: now.addingTimeInterval(-73 * 3600)).awaitsDirectory(now: now), "gives up after 72 hours")
        var live = record()
        live.directoryStatus = DirectoryStatus(state: .live, listedVersion: "2.4.0")
        XCTAssertFalse(live.awaitsDirectory(now: now))
        var pending = record()
        pending.directoryStatus = DirectoryStatus(state: .pending, listedVersion: "2.3.0")
        XCTAssertTrue(pending.awaitsDirectory(now: now))
    }

    func makePipeline(_ wordpressOrg: StubWordPressOrg?) throws -> (ReleasePipelineService, Store, RecordingNotifications, TempDir) {
        let work = TempDir()
        let store = try Store.inMemory()
        try store.save(product)
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let notifications = RecordingNotifications()
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: SVNService(), git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), notifications: notifications, wordpressOrg: wordpressOrg, generator: "tests"
        ))
        return (pipeline, store, notifications, work)
    }

    func testPendingThenLiveNotifiesOnce() async throws {
        let wordpressOrg = StubWordPressOrg(listing: DirectoryListing(version: "2.3.0"))
        let (pipeline, store, notifications, work) = try makePipeline(wordpressOrg); _ = work
        let release = record()
        try store.save(release)

        var updated = try await pipeline.refreshDirectoryStatus(release.id)
        XCTAssertEqual(updated.directoryStatus?.state, .pending)
        XCTAssertEqual(updated.directoryStatus?.listedVersion, "2.3.0")
        XCTAssertNil(updated.directoryStatus?.liveSince)
        XCTAssertEqual(updated.state, .completed, "the release state never changes")

        wordpressOrg.listing = DirectoryListing(version: "2.4.0")
        updated = try await pipeline.refreshDirectoryStatus(release.id)
        XCTAssertEqual(updated.directoryStatus?.state, .live)
        let liveSince = try XCTUnwrap(updated.directoryStatus?.liveSince)
        updated = try await pipeline.refreshDirectoryStatus(release.id)
        XCTAssertEqual(updated.directoryStatus?.liveSince, liveSince, "first time live is kept")
        XCTAssertEqual(notifications.titles.filter { $0.contains("is live") }, ["Acme Gallery 2.4.0 is live"])
        XCTAssertEqual(try store.release(id: release.id)?.directoryStatus?.state, .live, "persisted")
    }

    func testRefreshAwaitingSkipsUntrackedAndFinishedReleases() async throws {
        let wordpressOrg = StubWordPressOrg(listing: DirectoryListing(version: "2.3.0"))
        let (pipeline, store, _, work) = try makePipeline(wordpressOrg); _ = work
        let awaiting = record()
        var live = record()
        live.directoryStatus = DirectoryStatus(state: .live, listedVersion: "2.4.0")
        let local = record(url: "file:///tmp/repo/acme-gallery")
        let old = record(completedAt: Date().addingTimeInterval(-80 * 3600))
        for r in [awaiting, live, local, old] { try store.save(r) }

        let refreshed = await pipeline.refreshAwaitingDirectoryReleases()
        XCTAssertEqual(refreshed.map(\.id), [awaiting.id])
        XCTAssertEqual(wordpressOrg.requests, 1)
    }

    func testWithoutWordPressOrgServiceNothingHappens() async throws {
        let (pipeline, store, _, work) = try makePipeline(nil); _ = work
        let release = record()
        try store.save(release)
        let updated = try await pipeline.refreshDirectoryStatus(release.id)
        XCTAssertNil(updated.directoryStatus)
        let refreshed = await pipeline.refreshAwaitingDirectoryReleases()
        XCTAssertTrue(refreshed.isEmpty)
    }

    func testNotListedProduct() async throws {
        let (pipeline, store, notifications, work) = try makePipeline(StubWordPressOrg(listing: nil)); _ = work
        let release = record()
        try store.save(release)
        let updated = try await pipeline.refreshDirectoryStatus(release.id)
        XCTAssertEqual(updated.directoryStatus?.state, .notListed)
        XCTAssertTrue(notifications.titles.isEmpty)
    }
}
