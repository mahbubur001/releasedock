import XCTest
@testable import ReleaseDockCore

final class RepositorySetupTests: XCTestCase {
    var repo: LocalSVNRepository!
    var work: TempDir!
    var service: RepositorySetupService!

    override func setUp() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        repo = try LocalSVNRepository()
        work = TempDir()
        service = RepositorySetupService(svn: SVNService())
    }

    func testURLNormalization() throws {
        XCTAssertEqual(try RepositorySetupService.repositoryURL(from: "my-plugin", type: .plugin).absoluteString, "https://plugins.svn.wordpress.org/my-plugin")
        XCTAssertEqual(try RepositorySetupService.repositoryURL(from: "my-theme", type: .theme).absoluteString, "https://themes.svn.wordpress.org/my-theme")
        XCTAssertEqual(try RepositorySetupService.repositoryURL(from: "https://plugins.svn.wordpress.org/x/", type: .plugin).absoluteString, "https://plugins.svn.wordpress.org/x")
        XCTAssertThrowsError(try RepositorySetupService.repositoryURL(from: "http://plugins.svn.wordpress.org/x", type: .plugin))
        XCTAssertThrowsError(try RepositorySetupService.repositoryURL(from: "https://u:p@plugins.svn.wordpress.org/x", type: .plugin))
    }

    func testCheckoutFromServerDetectsEverything() async throws {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let url = URL(string: "\(repo.url)/sample-plugin")!
        let description = try await service.describeRemote(url)
        XCTAssertEqual(description.type, .plugin)
        XCTAssertEqual(description.slug, "sample-plugin")
        XCTAssertEqual(description.currentVersion, "1.0.0")
        XCTAssertEqual(description.productName, "Sample Plugin")

        let destination = work.path("checkout")
        try await service.checkout(description, type: .plugin, to: destination)
        XCTAssertTrue(FileManager.default.fileExists(atPath: destination.appendingPathComponent("trunk/sample-plugin.php").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: destination.appendingPathComponent("tags/1.0.0").path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: destination.appendingPathComponent("tags/1.0.0/readme.txt").path), "tags are checked out sparsely")

        let product = service.makeProduct(from: description, type: .plugin, workingCopy: destination)
        XCTAssertEqual(product.name, "Sample Plugin")
        XCTAssertEqual(product.expectedHeaderName, "Sample Plugin")
        XCTAssertEqual(product.currentVersion, "1.0.0")
        XCTAssertNil(product.mainPluginFile, "slug.php is found automatically")
        XCTAssertEqual(product.workingCopyPath, destination.path)
    }

    func testCheckoutRefusesNonEmptyFolder() async throws {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let description = try await service.describeRemote(URL(string: "\(repo.url)/sample-plugin")!)
        work.write("busy/file.txt", "x")
        await XCTAssertThrowsErrorAsync(try await self.service.checkout(description, type: .plugin, to: self.work.path("busy")))
    }

    func testRejectsTrunkAndMissingRepositories() async throws {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        do {
            _ = try await service.describeRemote(URL(string: "\(repo.url)/sample-plugin/trunk")!)
            XCTFail("trunk must be rejected")
        } catch let error as RepositorySetupError {
            guard case .notProductRoot(_, let suggestion) = error else { return XCTFail("\(error)") }
            XCTAssertTrue(suggestion.hasSuffix("/sample-plugin"))
        }
        do {
            _ = try await service.describeRemote(URL(string: "\(repo.url)/missing")!)
            XCTFail("missing repository")
        } catch {
            XCTAssertEqual(error as? RepositorySetupError, .repositoryNotFound("\(repo.url)/missing"))
        }
    }

    func testExistingWorkingCopyIsAdopted() async throws {
        try repo.createTheme(slug: "sample-theme", initialVersion: "1.2.0")
        let checkout = work.path("existing")
        try repo.run(["checkout", "\(repo.url)/sample-theme", checkout.path])
        let description = try await service.describeWorkingCopy(at: checkout)
        XCTAssertEqual(description.repository.type, .theme)
        XCTAssertEqual(description.repository.currentVersion, "1.2.0")
        XCTAssertEqual(description.repository.productName, "Sample Theme")
        XCTAssertEqual(description.localChanges, [])

        let product = service.makeProduct(from: description.repository, type: .theme, workingCopy: checkout)
        XCTAssertEqual(product.slug, "sample-theme")
        XCTAssertEqual(product.name, "Sample Theme")
        XCTAssertEqual(product.type, .theme)
    }

    func testExistingWorkingCopyMustBeTheRoot() async throws {
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let checkout = work.path("wc")
        try repo.run(["checkout", "\(repo.url)/sample-plugin", checkout.path])
        await XCTAssertThrowsErrorAsync(try await self.service.describeWorkingCopy(at: checkout.appendingPathComponent("trunk")))
        work.mkdir("plain")
        await XCTAssertThrowsErrorAsync(try await self.service.describeWorkingCopy(at: self.work.path("plain")))
    }
}
