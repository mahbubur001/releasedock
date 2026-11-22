import XCTest
@testable import ReleaseDockCore

/// Opt-in, read-only checks against WordPress.org and a local checkout:
/// RELEASEDOCK_LIVE_SLUG=tlp-team RELEASEDOCK_LIVE_WC=~/svn/tlp-team swift test --filter LiveRepositoryTests
final class LiveRepositoryTests: XCTestCase {
    func testDescribeRealRepositoryAndCheckout() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let slug = env["RELEASEDOCK_LIVE_SLUG"] else { throw XCTSkip("Set RELEASEDOCK_LIVE_SLUG") }
        let service = RepositorySetupService(svn: SVNService())
        let clock = ContinuousClock()

        var start = clock.now
        let remote = try await service.describeRemote(URL(string: "https://plugins.svn.wordpress.org/\(slug)")!)
        print("LIVE remote: \(clock.now - start) name=\(remote.productName ?? "nil") version=\(remote.currentVersion ?? "nil") tags=\(remote.versions.count) notes=\(remote.notes)")
        XCTAssertNotNil(remote.productName)
        XCTAssertNotNil(remote.currentVersion)

        if let wc = env["RELEASEDOCK_LIVE_WC"] {
            start = clock.now
            let local = try await service.describeWorkingCopyLocally(at: URL(fileURLWithPath: (wc as NSString).expandingTildeInPath))
            print("LIVE local: \(clock.now - start) name=\(local.repository.productName ?? "nil") version=\(local.repository.currentVersion ?? "nil") changes=\(local.localChanges.count)")
            XCTAssertLessThan(clock.now - start, .seconds(3))
        }
    }
}

/// Opt-in, read-only dry run against a real WordPress.org plugin:
/// RELEASEDOCK_LIVE_DRYRUN_SLUG=tlp-team RELEASEDOCK_LIVE_DRYRUN_VERSION=6.0.8 swift test --filter LiveDryRunTests
/// Exports trunk into a temp folder, bumps the version there, and runs the full pipeline in
/// Dry Run mode with an isolated data folder and a fresh temporary checkout. Never commits.
final class LiveDryRunTests: XCTestCase {
    func testDryRunAgainstWordPressOrg() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let slug = env["RELEASEDOCK_LIVE_DRYRUN_SLUG"], let newVersion = env["RELEASEDOCK_LIVE_DRYRUN_VERSION"] else {
            throw XCTSkip("Set RELEASEDOCK_LIVE_DRYRUN_SLUG and RELEASEDOCK_LIVE_DRYRUN_VERSION")
        }
        let work = TempDir()
        let url = "https://plugins.svn.wordpress.org/\(slug)"
        let svn = SVNService(commandTimeout: .seconds(600))
        let clock = ContinuousClock()
        var start = clock.now

        // 1. Package: current trunk with the version bumped (temp copy only).
        let package = work.path("package/\(slug)")
        try await svn.export("\(url)/trunk", to: package)
        let detected = try ProductDetector().detect(at: package, expectedType: .plugin, slug: slug)
        let main = try XCTUnwrap(detected.mainFile)
        let current = try XCTUnwrap(detected.version)
        func replace(_ file: URL, _ pattern: String, _ template: String) throws {
            let text = try String(contentsOf: file, encoding: .utf8)
            let regex = try NSRegularExpression(pattern: pattern, options: [.anchorsMatchLines, .caseInsensitive])
            let updated = regex.stringByReplacingMatches(in: text, range: NSRange(text.startIndex..., in: text), withTemplate: template)
            try updated.write(to: file, atomically: true, encoding: .utf8)
        }
        try replace(package.appendingPathComponent(main), #"^([ \t/*#@]*Version:[ \t]*)"# + NSRegularExpression.escapedPattern(for: current), "$1\(newVersion)")
        if let readme = detected.readmeFile {
            try replace(package.appendingPathComponent(readme), #"^(Stable tag:[ \t]*).*$"#, "$1\(newVersion)")
        }
        print("LIVE package: \(slug) \(current) → \(newVersion) (\(clock.now - start))")

        // 2. Pipeline in Dry Run mode, isolated data folder and fresh checkout.
        let store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let product = Product(name: detected.name ?? slug, slug: slug, type: .plugin, svnRepositoryURL: URL(string: url)!,
                              workingCopyPath: work.path("wc").path, releaseMode: .dryRun)
        try store.save(product)
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), generator: "live-dry-run"))
        start = clock.now
        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: package.path), mode: .dryRun)
        record = try await pipeline.prepare(record.id)
        print("LIVE dry run: \(record.state.rawValue) in \(clock.now - start)")
        print("LIVE current version on SVN: \(record.repository?.currentVersion ?? "?") via \(record.repository?.currentVersionSource ?? "?")")
        for result in record.validation?.results ?? [] where result.status != .passed {
            print("LIVE check \(result.status.rawValue): \(result.name) — \(result.message)")
        }
        if let preview = record.preview {
            print("LIVE preview: \(preview.added) added, \(preview.modified) modified, \(preview.deleted) deleted; \(preview.changes.count) entries")
            for change in preview.changes.prefix(12) { print("LIVE   \(change.isCopy ? "copy" : change.kind.rawValue) \(change.path)") }
        }
        print("LIVE errors: \(record.errors)")
        XCTAssertEqual(record.state, .completed, "dry run should complete")
        XCTAssertNil(record.committedRevision, "a dry run never commits")
    }
}
