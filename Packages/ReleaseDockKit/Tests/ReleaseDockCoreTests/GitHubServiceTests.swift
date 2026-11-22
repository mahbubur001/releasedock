import XCTest
@testable import ReleaseDockCore

final class StubURLProtocol: URLProtocol {
    nonisolated(unsafe) static var handler: ((URLRequest) -> (HTTPURLResponse, Data))?
    nonisolated(unsafe) static var lastRequest: URLRequest?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lastRequest = request
        guard let handler = Self.handler else { return }
        let (response, data) = handler(request)
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: data)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

final class GitHubServiceTests: XCTestCase {
    var service: GitHubService!
    let repo = GitHubRepository(owner: "acme", name: "releasedock")

    override func setUp() {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubURLProtocol.self]
        service = GitHubService(session: URLSession(configuration: config))
    }

    static func releaseJSON(tag: String, prerelease: Bool = false, draft: Bool = false) -> String {
        """
        {"id": 1, "tag_name": "\(tag)", "name": "ReleaseDock \(tag)", "body": "Notes", "draft": \(draft), "prerelease": \(prerelease),
         "html_url": "https://github.com/acme/releasedock/releases/tag/\(tag)", "published_at": "2026-11-01T10:00:00Z",
         "assets": [{"id": 9, "name": "ReleaseDock.zip", "size": 10, "url": "https://api.github.com/repos/acme/releasedock/releases/assets/9",
                     "browser_download_url": "https://github.com/acme/releasedock/releases/download/\(tag)/ReleaseDock.zip", "content_type": "application/zip"}]}
        """
    }

    func response(_ status: Int, headers: [String: String] = [:]) -> HTTPURLResponse {
        HTTPURLResponse(url: URL(string: "https://api.github.com")!, statusCode: status, httpVersion: nil, headerFields: headers)!
    }

    func testLatestStableReleaseAndHeaders() async throws {
        StubURLProtocol.handler = { _ in (self.response(200, headers: ["ETag": "\"abc\"", "x-ratelimit-remaining": "59", "x-ratelimit-limit": "60"]), Data(Self.releaseJSON(tag: "v1.4.0").utf8)) }
        let result = try await service.latestRelease(in: repo, channel: .stable, token: nil, cache: nil)
        XCTAssertEqual(result.release?.version, "1.4.0")
        XCTAssertEqual(result.etag, "\"abc\"")
        XCTAssertEqual(result.rateLimit?.remaining, 59)
        let request = try XCTUnwrap(StubURLProtocol.lastRequest)
        XCTAssertEqual(request.url?.path, "/repos/acme/releasedock/releases/latest")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Accept"), "application/vnd.github+json")
        XCTAssertNotNil(request.value(forHTTPHeaderField: "X-GitHub-Api-Version"))
        XCTAssertNil(request.value(forHTTPHeaderField: "Authorization"), "public checks are unauthenticated")
    }

    func testConditionalRequestUsesCache() async throws {
        StubURLProtocol.handler = { _ in (self.response(200, headers: ["ETag": "\"abc\""]), Data(Self.releaseJSON(tag: "1.4.0").utf8)) }
        let first = try await service.latestRelease(in: repo, channel: .stable, token: nil, cache: nil)
        StubURLProtocol.handler = { request in
            XCTAssertEqual(request.value(forHTTPHeaderField: "If-None-Match"), "\"abc\"")
            return (self.response(304), Data())
        }
        let second = try await service.latestRelease(in: repo, channel: .stable, token: "github_pat_x", cache: first)
        XCTAssertEqual(second.release?.version, "1.4.0")
        XCTAssertEqual(StubURLProtocol.lastRequest?.value(forHTTPHeaderField: "Authorization"), "Bearer github_pat_x")
    }

    func testBetaChannelPicksHighestNonDraft() async throws {
        let body = "[\(Self.releaseJSON(tag: "1.5.0-beta1", prerelease: true)), \(Self.releaseJSON(tag: "1.4.0")), \(Self.releaseJSON(tag: "2.0.0", draft: true))]"
        StubURLProtocol.handler = { _ in (self.response(200), Data(body.utf8)) }
        let result = try await service.latestRelease(in: repo, channel: .beta, token: nil, cache: nil)
        XCTAssertEqual(result.release?.version, "1.5.0-beta1")
    }

    func testRateLimitAndErrors() async {
        StubURLProtocol.handler = { _ in (self.response(403, headers: ["x-ratelimit-remaining": "0", "x-ratelimit-reset": "1900000000"]), Data()) }
        do {
            _ = try await service.latestRelease(in: repo, channel: .stable, token: nil, cache: nil)
            XCTFail()
        } catch let error as GitHubError {
            XCTAssertEqual(error, .rateLimited(resetAt: Date(timeIntervalSince1970: 1_900_000_000)))
        } catch { XCTFail("\(error)") }

        StubURLProtocol.handler = { _ in (self.response(404), Data()) }
        do {
            _ = try await service.latestRelease(in: repo, channel: .stable, token: nil, cache: nil)
            XCTFail()
        } catch { XCTAssertEqual(error as? GitHubError, .notFound) }

        StubURLProtocol.handler = { _ in (self.response(401), Data()) }
        do {
            _ = try await service.latestRelease(in: repo, channel: .stable, token: "bad", cache: nil)
            XCTFail()
        } catch { XCTAssertEqual(error as? GitHubError, .unauthorized) }
    }

    func testRepositoryParsing() {
        XCTAssertEqual(GitHubRepository(string: "https://github.com/acme/release-dock.git")?.fullName, "acme/release-dock")
        XCTAssertEqual(GitHubRepository(string: "git@github.com:acme/x.git")?.fullName, "acme/x")
        XCTAssertEqual(GitHubRepository(string: "acme/x")?.fullName, "acme/x")
        XCTAssertNil(GitHubRepository(string: "https://github.com/acme"))
        XCTAssertNil(GitHubRepository(string: "acme/../x"))
        XCTAssertNil(GitHubRepository(string: "acme/x?y=1"))
    }
}
