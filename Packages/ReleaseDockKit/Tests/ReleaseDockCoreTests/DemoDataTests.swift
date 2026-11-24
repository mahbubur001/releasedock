import XCTest
@testable import ReleaseDockCore

/// Builds a demo data folder for screenshots and manual UI checks:
/// RELEASEDOCK_MAKE_DEMO=/tmp/rd-demo swift test --filter DemoDataTests
/// then launch the Debug app with RELEASEDOCK_DATA_DIR=/tmp/rd-demo/data.
/// Everything lives in that folder: local file:// repositories, working copies and products.
final class DemoDataTests: XCTestCase {
    func testMakeDemoData() async throws {
        guard let out = ProcessInfo.processInfo.environment["RELEASEDOCK_MAKE_DEMO"] else { throw XCTSkip("Set RELEASEDOCK_MAKE_DEMO") }
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        let root = URL(fileURLWithPath: out, isDirectory: true)
        try? FileManager.default.removeItem(at: root)
        let fm = FileManager.default
        try fm.createDirectory(at: root, withIntermediateDirectories: true)

        let repo = try LocalSVNRepository(persistentAt: root.appendingPathComponent("svn"))
        try repo.createPlugin(slug: "acme-gallery", initialVersion: "2.3.0", name: "Acme Gallery")
        try repo.createTheme(slug: "acme-theme", initialVersion: "1.1.0", name: "Acme Theme")

        let paths = AppPaths(root: root.appendingPathComponent("data"))
        try paths.createAll()
        let store = try Store.open(paths: paths)
        let devFolder = root.appendingPathComponent("dev/acme-gallery")
        let gallery = Product(
            name: "Acme Gallery", slug: "acme-gallery", type: .plugin,
            localDevelopmentPath: devFolder.path,
            svnRepositoryURL: URL(string: "\(repo.url)/acme-gallery")!,
            workingCopyPath: root.appendingPathComponent("wc/acme-gallery").path, currentVersion: "2.3.0",
            githubRepositoryURL: URL(string: "https://github.com/acme/acme-gallery"), svnUsername: "acme"
        )
        let theme = Product(
            name: "Acme Theme", slug: "acme-theme", type: .theme,
            svnRepositoryURL: URL(string: "\(repo.url)/acme-theme")!,
            workingCopyPath: root.appendingPathComponent("wc/acme-theme").path, currentVersion: "1.1.0", svnUsername: "acme"
        )
        try store.save(gallery)
        try store.save(theme)

        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: FaultInjectingSVN(base: SVNService(commandTimeout: .seconds(120))), git: StubGit(),
            keychain: InMemoryKeychain(), staging: ReleaseStagingService(paths: paths), generator: "ReleaseDock demo"
        ))
        func source(_ name: String, version: String, theme: Bool = false, extra: [String: String] = [:]) throws -> ReleaseSource {
            let src = root.appendingPathComponent("packages/\(name)-\(version)")
            try? fm.removeItem(at: src)
            let tmp = TempDir()
            let built = theme
                ? Fixtures.makeTheme(in: tmp, at: name, version: version, name: "Acme Theme")
                : Fixtures.makePlugin(in: tmp, at: name, version: version, name: "Acme Gallery", extraFiles: extra)
            try fm.createDirectory(at: src.deletingLastPathComponent(), withIntermediateDirectories: true)
            try fm.copyItem(at: built, to: src)
            return .folder(path: src.path)
        }
        func publish(_ product: Product, _ src: ReleaseSource) async throws {
            var record = try await pipeline.createRelease(productID: product.id, source: src)
            record = try await pipeline.prepare(record.id)
            _ = try await pipeline.approve(record.id, kind: .user)
            record = try await pipeline.publish(record.id)
            XCTAssertEqual(record.state, .completed, "\(record.errors)")
        }

        // History: a theme release, a cancelled attempt and a published plugin version.
        try await publish(theme, try source("acme-theme", version: "1.2.0", theme: true))
        let cancelled = try await pipeline.createRelease(productID: gallery.id, source: try source("acme-gallery", version: "2.4.0"))
        _ = try await pipeline.prepare(cancelled.id)
        _ = try await pipeline.cancel(cancelled.id)
        try await publish(try XCTUnwrap(try store.product(id: gallery.id)), try source("acme-gallery", version: "2.4.0", extra: ["includes/class-lightbox.php": "<?php\nfinal class Lightbox {}\n"]))

        // Waiting for approval: 2.5.0 prepared and previewed.
        let waiting = try await pipeline.createRelease(productID: gallery.id, source: try source("acme-gallery", version: "2.5.0", extra: ["includes/class-lightbox.php": "<?php\nfinal class Lightbox {\n    public function register(): void {}\n}\n"]))
        let previewed = try await pipeline.prepare(waiting.id)
        XCTAssertEqual(previewed.state, .previewed, "\(previewed.validation?.blockingResults ?? [])")

        // The development folder, one version behind, for Bump Version.
        let dev = TempDir()
        let plugin = Fixtures.makePlugin(in: dev, at: "acme-gallery", version: "2.5.0", name: "Acme Gallery")
        try fm.createDirectory(at: devFolder.deletingLastPathComponent(), withIntermediateDirectories: true)
        try fm.copyItem(at: plugin, to: devFolder)
        print("DEMO DATA: RELEASEDOCK_DATA_DIR=\(paths.root.path)")
    }
}
