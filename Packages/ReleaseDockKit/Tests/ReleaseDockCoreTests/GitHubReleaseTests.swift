import XCTest
@testable import ReleaseDockCore

final class GitHubReleaseAPITests: XCTestCase {
    var service: GitHubService!
    let repo = GitHubRepository(owner: "acme", name: "acme-gallery")

    override func setUp() {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubURLProtocol.self]
        service = GitHubService(session: URLSession(configuration: config))
    }

    func response(_ status: Int, _ url: URL? = nil) -> HTTPURLResponse {
        HTTPURLResponse(url: url ?? URL(string: "https://api.github.com")!, statusCode: status, httpVersion: nil, headerFields: [:])!
    }

    static let releaseJSON = """
    {"id": 7, "tag_name": "v2.4.0", "name": "2.4.0", "body": "x", "draft": false, "prerelease": false,
     "html_url": "https://github.com/acme/acme-gallery/releases/tag/v2.4.0", "published_at": null, "assets": [],
     "upload_url": "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{?name,label}"}
    """

    func testCreateReleaseTargetsTheCommit() async throws {
        var body: [String: Any] = [:]
        StubURLProtocol.handler = { request in
            if let stream = request.httpBodyStream {
                stream.open()
                var data = Data()
                var buffer = [UInt8](repeating: 0, count: 4096)
                while stream.hasBytesAvailable { let n = stream.read(&buffer, maxLength: buffer.count); if n <= 0 { break }; data.append(buffer, count: n) }
                stream.close()
                body = (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
            }
            return (self.response(201), Data(Self.releaseJSON.utf8))
        }
        let release = try await service.createRelease(in: repo, tag: "v2.4.0", commit: "abc123", name: "2.4.0", body: "Notes", draft: false, prerelease: false, token: "t0k")
        XCTAssertEqual(release.uploadURL, "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{?name,label}")
        let request = try XCTUnwrap(StubURLProtocol.lastRequest)
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.url?.path, "/repos/acme/acme-gallery/releases")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer t0k")
        XCTAssertEqual(body["target_commitish"] as? String, "abc123")
        XCTAssertEqual(body["tag_name"] as? String, "v2.4.0")
    }

    func testErrorsAreSpecific() async throws {
        StubURLProtocol.handler = { _ in (self.response(422), Data(#"{"message":"Validation Failed","errors":[{"resource":"Release","code":"already_exists","field":"tag_name"}]}"#.utf8)) }
        do {
            _ = try await service.createRelease(in: repo, tag: "v1", commit: "a", name: "1", body: "", draft: false, prerelease: false, token: "t")
            XCTFail("expected 422")
        } catch GitHubError.validationFailed(let message) {
            XCTAssertEqual(message, "Validation Failed: already_exists")
        }
        StubURLProtocol.handler = { _ in (self.response(403), Data()) }
        do {
            _ = try await service.release(tag: "v1", in: repo, token: "t")
            XCTFail("expected 403")
        } catch {
            XCTAssertEqual(error as? GitHubError, .forbidden)
        }
        StubURLProtocol.handler = { _ in (self.response(404), Data()) }
        let missing = try await service.release(tag: "v1", in: repo, token: "t")
        XCTAssertNil(missing)
        StubURLProtocol.handler = { _ in (self.response(422), Data()) }
        let exists = try await service.commitExists("abc", in: repo, token: "t")
        XCTAssertFalse(exists)
    }

    func testUploadExpandsTheTemplate() async throws {
        let release = try JSONDecoder.iso.decode(GitHubRelease.self, from: Data(Self.releaseJSON.utf8))
        let dir = TempDir()
        let zip = dir.write("a.zip", "PK")
        StubURLProtocol.handler = { _ in (self.response(201), Data("{}".utf8)) }
        try await service.uploadAsset(zip, name: "acme-gallery-2.4.0.zip", to: release, token: "t")
        let request = try XCTUnwrap(StubURLProtocol.lastRequest)
        XCTAssertEqual(request.url?.absoluteString, "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets?name=acme-gallery-2.4.0.zip")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Content-Type"), "application/zip")
    }
}

extension JSONDecoder {
    static var iso: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }
}

/// Records calls and plays back configured answers.
final class StubGitHubReleaser: GitHubReleasing, @unchecked Sendable {
    private let lock = NSLock()
    var existing: GitHubRelease?
    var commitOnGitHub = true
    private(set) var created: [(tag: String, commit: String, body: String, prerelease: Bool)] = []
    private(set) var uploads: [(name: String, entries: [String])] = []

    static func release(tag: String, assets: [String] = []) -> GitHubRelease {
        let assetJSON = assets.enumerated().map { i, name in
            #"{"id": \#(i), "name": "\#(name)", "size": 1, "url": "https://api.github.com/a/\#(i)", "browser_download_url": "https://github.com/d/\#(name)"}"#
        }.joined(separator: ",")
        let json = """
        {"id": 7, "tag_name": "\(tag)", "name": null, "body": null, "draft": false, "prerelease": false,
         "html_url": "https://github.com/acme/acme-gallery/releases/tag/\(tag)", "published_at": null, "assets": [\(assetJSON)],
         "upload_url": "https://uploads.github.com/repos/acme/acme-gallery/releases/7/assets{?name,label}"}
        """
        return try! JSONDecoder.iso.decode(GitHubRelease.self, from: Data(json.utf8))
    }

    func commitExists(_ sha: String, in repository: GitHubRepository, token: String) async throws -> Bool { commitOnGitHub }
    func release(tag: String, in repository: GitHubRepository, token: String) async throws -> GitHubRelease? { lock.withLock { existing } }
    func createRelease(in repository: GitHubRepository, tag: String, commit: String, name: String, body: String, draft: Bool, prerelease: Bool, token: String) async throws -> GitHubRelease {
        lock.withLock { created.append((tag, commit, body, prerelease)) }
        return Self.release(tag: tag)
    }
    func uploadAsset(_ file: URL, name: String, to release: GitHubRelease, token: String) async throws {
        // Record the archive's entries to check its layout.
        let listing = (try? await ProcessRunner().run(ProcessRequest(executableURL: URL(fileURLWithPath: "/usr/bin/zipinfo"), arguments: ["-1", file.path])))?.stdout ?? ""
        lock.withLock { uploads.append((name, listing.split(separator: "\n").map(String.init))) }
    }
}

final class GitHubReleasePublisherTests: XCTestCase {
    func record(commit: String? = "abc1234567", version: String = "2.4.0") -> (ReleaseRecord, Product) {
        let product = Product(
            name: "Acme Gallery", slug: "acme-gallery", type: .plugin,
            svnRepositoryURL: URL(string: "https://plugins.svn.wordpress.org/acme-gallery/")!, workingCopyPath: "/tmp/wc",
            githubRepositoryURL: URL(string: "https://github.com/acme/acme-gallery"), githubRelease: GitHubReleaseSettings(enabled: true)
        )
        var record = ReleaseRecord(product: product, source: .folder(path: "/tmp/src"), mode: .assisted)
        record.version = version
        record.gitCommit = commit
        record.committedRevision = 42
        record.detected = DetectedProduct(type: .plugin, version: version, readme: ReadmeParser.parse("=== A ===\nStable tag: \(version)\n\nShort.\n\n== Changelog ==\n= \(version) =\n* New: Lightbox.\n"))
        return (record, product)
    }

    func testCreatesReleaseOnTheReleasedCommit() async throws {
        let stub = StubGitHubReleaser()
        let (record, product) = record()
        let result = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: nil, token: "t")
        XCTAssertTrue(result.succeeded, "\(result)")
        XCTAssertEqual(result.tag, "v2.4.0")
        XCTAssertEqual(stub.created.first?.commit, "abc1234567")
        XCTAssertEqual(stub.created.first?.body, "* New: Lightbox.\n\nPublished to WordPress.org in SVN revision r42 with ReleaseDock.")
        XCTAssertEqual(stub.created.first?.prerelease, false)
        XCTAssertTrue(result.notes.contains { $0.contains("ZIP was no longer available") })
    }

    func testPrereleaseFlag() async {
        let stub = StubGitHubReleaser()
        let (record, product) = record(version: "2.5.0-beta1")
        _ = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: nil, token: "t")
        XCTAssertEqual(stub.created.first?.prerelease, true)
    }

    func testReusesAnExistingReleaseAndAddsTheMissingAsset() async throws {
        let stub = StubGitHubReleaser()
        stub.existing = StubGitHubReleaser.release(tag: "v2.4.0")
        let dir = TempDir()
        let zip = dir.write("x.zip", "PK")
        let (record, product) = record()
        let result = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: zip, token: "t")
        XCTAssertTrue(result.succeeded)
        XCTAssertTrue(result.alreadyExisted)
        XCTAssertTrue(stub.created.isEmpty, "never a second release for the tag")
        XCTAssertEqual(stub.uploads.map(\.name), ["acme-gallery-2.4.0.zip"])

        stub.existing = StubGitHubReleaser.release(tag: "v2.4.0", assets: ["acme-gallery-2.4.0.zip"])
        _ = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: zip, token: "t")
        XCTAssertEqual(stub.uploads.count, 1, "an existing asset is not uploaded again")
    }

    func testExplainsWhatIsMissing() async {
        let stub = StubGitHubReleaser()
        var (record, product) = record()
        var result = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: nil, token: nil)
        XCTAssertTrue(result.error?.contains("token") == true)
        result = await GitHubReleasePublisher(github: stub).publish(record: self.record(commit: nil).0, product: product, zip: nil, token: "t")
        XCTAssertTrue(result.error?.contains("not a ZIP") == true)
        stub.commitOnGitHub = false
        result = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: nil, token: "t")
        XCTAssertEqual(result.error, GitHubError.commitNotOnGitHub("abc1234567").errorDescription)
        product.githubRepositoryURL = nil
        record.version = "2.4.0"
        result = await GitHubReleasePublisher(github: stub).publish(record: record, product: product, zip: nil, token: "t")
        XCTAssertTrue(result.error?.contains("GitHub repository") == true)
        XCTAssertTrue(stub.created.isEmpty)
    }
}

final class GitHubReleasePipelineTests: XCTestCase {
    func testCompletedReleaseCreatesGitHubReleaseWithTheExactPackage() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        let repo = try LocalSVNRepository()
        let work = TempDir()
        let store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let keychain = InMemoryKeychain()
        try keychain.setPassword("t0k", for: .githubReleaseToken)
        let stub = StubGitHubReleaser()
        let git = StubGit(state: GitRepositoryState(isRepository: true, headCommit: "abc1234567", branch: "main", exactTag: nil, uncommittedChanges: [], remoteURL: nil))
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: SVNService(commandTimeout: .seconds(60)), git: git, keychain: keychain,
            staging: ReleaseStagingService(paths: paths), githubReleaser: stub, generator: "tests"
        ))
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path, currentVersion: "1.0.0",
            githubRepositoryURL: URL(string: "https://github.com/acme/sample-plugin"), githubRelease: GitHubReleaseSettings(enabled: true)
        )
        try store.save(product)
        let source = Fixtures.makePlugin(in: work, at: "dev/sample-plugin", version: "1.1.0")

        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: source.path))
        record = try await pipeline.prepare(record.id)
        _ = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")
        XCTAssertEqual(record.githubRelease?.succeeded, true, "\(String(describing: record.githubRelease))")
        XCTAssertEqual(stub.created.first?.tag, "v1.1.0")
        let upload = try XCTUnwrap(stub.uploads.first)
        XCTAssertEqual(upload.name, "sample-plugin-1.1.0.zip")
        XCTAssertTrue(upload.entries.contains("sample-plugin/sample-plugin.php"), "\(upload.entries)")
        XCTAssertTrue(upload.entries.allSatisfy { $0.hasPrefix("sample-plugin/") })
        XCTAssertTrue(FileManager.default.fileExists(atPath: try XCTUnwrap(record.releaseZipPath)))
        XCTAssertEqual(try store.release(id: record.id)?.githubRelease?.tag, "v1.1.0", "persisted")
    }
}
