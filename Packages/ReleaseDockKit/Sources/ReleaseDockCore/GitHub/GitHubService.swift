import Foundation

public struct GitHubRepository: Codable, Hashable, Sendable {
    public var owner: String
    public var name: String

    public init(owner: String, name: String) {
        self.owner = owner
        self.name = name
    }

    /// Parses `owner/name`, `https://github.com/owner/name(.git)` or `git@github.com:owner/name.git`.
    public init?(string: String) {
        var s = string.trimmingCharacters(in: .whitespacesAndNewlines)
        for prefix in ["https://github.com/", "http://github.com/", "git@github.com:", "github.com/"] where s.lowercased().hasPrefix(prefix) {
            s = String(s.dropFirst(prefix.count))
            break
        }
        if s.hasSuffix(".git") { s = String(s.dropLast(4)) }
        if s.hasSuffix("/") { s = String(s.dropLast()) }
        let parts = s.split(separator: "/").map(String.init)
        guard parts.count == 2 else { return nil }
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_."))
        guard parts.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." && $0.unicodeScalars.allSatisfy(allowed.contains) }) else { return nil }
        owner = parts[0]
        name = parts[1]
    }

    public var fullName: String { "\(owner)/\(name)" }
    public var htmlURL: URL { URL(string: "https://github.com/\(owner)/\(name)")! }
}

public struct GitHubRelease: Codable, Hashable, Sendable, Identifiable {
    public struct Asset: Codable, Hashable, Sendable {
        public var id: Int
        public var name: String
        public var size: Int
        public var url: URL
        public var browserDownloadURL: URL
        public var contentType: String?

        enum CodingKeys: String, CodingKey {
            case id, name, size, url
            case browserDownloadURL = "browser_download_url"
            case contentType = "content_type"
        }
    }

    public var id: Int
    public var tagName: String
    public var name: String?
    public var body: String?
    public var draft: Bool
    public var prerelease: Bool
    public var htmlURL: URL
    public var publishedAt: Date?
    public var assets: [Asset]
    /// `https://uploads.github.com/repos/o/r/releases/1/assets{?name,label}`
    public var uploadURL: String?

    enum CodingKeys: String, CodingKey {
        case id, name, body, draft, prerelease, assets
        case tagName = "tag_name"
        case htmlURL = "html_url"
        case publishedAt = "published_at"
        case uploadURL = "upload_url"
    }

    /// Version derived from the tag (`v1.2.3` → `1.2.3`).
    public var version: String {
        let t = tagName.trimmingCharacters(in: .whitespaces)
        if let first = t.first, first == "v" || first == "V", t.dropFirst().first?.isNumber == true { return String(t.dropFirst()) }
        return t
    }
}

public struct RateLimitStatus: Codable, Hashable, Sendable {
    public var limit: Int?
    public var remaining: Int?
    public var reset: Date?
}

public enum GitHubError: Error, LocalizedError, Equatable, Sendable {
    case noReleases
    case notFound
    case unauthorized
    case rateLimited(resetAt: Date?)
    case http(Int)
    case invalidResponse
    case forbidden
    case commitNotOnGitHub(String)
    case validationFailed(String)

    public var errorDescription: String? {
        switch self {
        case .noReleases: "The repository has no published releases."
        case .notFound: "No published release was found. The repository may have no releases yet, or it is private and needs a token with Contents: read permission."
        case .unauthorized: "The GitHub token was rejected."
        case .rateLimited(let reset):
            if let reset { "GitHub API rate limit reached. Try again after \(reset.formatted(date: .omitted, time: .shortened))." }
            else { "GitHub API rate limit reached." }
        case .http(let code): "GitHub returned HTTP \(code)."
        case .invalidResponse: "Unexpected response from GitHub."
        case .forbidden: "The GitHub token may not write to this repository. Use a fine-grained token with Contents: read and write."
        case .commitNotOnGitHub(let sha): "Commit \(sha.prefix(10)) is not on GitHub. Push it, then create the release again."
        case .validationFailed(let message): "GitHub rejected the request: \(message)"
        }
    }
}

public enum ReleaseChannel: String, Codable, CaseIterable, Sendable {
    case stable
    case beta
}

/// Cached result of the last release check, reused with ETag conditional requests.
public struct ReleaseCheckCache: Codable, Hashable, Sendable {
    public var repository: String
    public var channel: ReleaseChannel
    public var etag: String?
    public var release: GitHubRelease?
    public var checkedAt: Date
    public var rateLimit: RateLimitStatus?
}

public protocol GitHubServicing: Sendable {
    func latestRelease(in repository: GitHubRepository, channel: ReleaseChannel, token: String?, cache: ReleaseCheckCache?) async throws -> ReleaseCheckCache
}

/// GitHub REST client for update notifications. Public repositories need no token;
/// private repositories use a fine-grained token with *Contents: read* only.
public struct GitHubService: GitHubServicing {
    public static let apiVersion = "2022-11-28"
    private let session: URLSession
    private let baseURL: URL

    public init(session: URLSession = .shared, baseURL: URL = URL(string: "https://api.github.com")!) {
        self.session = session
        self.baseURL = baseURL
    }

    public func latestRelease(in repository: GitHubRepository, channel: ReleaseChannel, token: String?, cache: ReleaseCheckCache?) async throws -> ReleaseCheckCache {
        // /releases/latest excludes drafts and pre-releases; the beta channel lists releases instead.
        let path = channel == .stable
            ? "repos/\(repository.owner)/\(repository.name)/releases/latest"
            : "repos/\(repository.owner)/\(repository.name)/releases?per_page=20"
        guard let url = URL(string: path, relativeTo: baseURL) else { throw GitHubError.invalidResponse }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue(Self.apiVersion, forHTTPHeaderField: "X-GitHub-Api-Version")
        request.setValue("ReleaseDock", forHTTPHeaderField: "User-Agent")
        if let token, !token.isEmpty { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        let sameRequest = cache?.repository == repository.fullName && cache?.channel == channel
        if sameRequest, let etag = cache?.etag { request.setValue(etag, forHTTPHeaderField: "If-None-Match") }

        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw GitHubError.invalidResponse }
        let rate = Self.rateLimit(from: http)

        switch http.statusCode {
        case 200:
            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            let release: GitHubRelease?
            if channel == .stable {
                release = try decoder.decode(GitHubRelease.self, from: data)
            } else {
                let releases = try decoder.decode([GitHubRelease].self, from: data).filter { !$0.draft }
                release = releases.max { PHPVersion.compare($0.version, $1.version) < 0 }
            }
            return ReleaseCheckCache(repository: repository.fullName, channel: channel, etag: http.value(forHTTPHeaderField: "ETag"), release: release, checkedAt: Date(), rateLimit: rate)
        case 304:
            guard var cached = cache, sameRequest else { throw GitHubError.invalidResponse }
            cached.checkedAt = Date()
            cached.rateLimit = rate
            return cached
        case 401:
            throw GitHubError.unauthorized
        case 404:
            // GitHub answers 404 both for unknown/private repositories and for /releases/latest
            // when nothing is published, so the two cannot be told apart.
            throw GitHubError.notFound
        case 403, 429:
            if rate.remaining == 0 || http.value(forHTTPHeaderField: "Retry-After") != nil || http.statusCode == 429 {
                let retryAfter = http.value(forHTTPHeaderField: "Retry-After").flatMap(Double.init).map { Date().addingTimeInterval($0) }
                throw GitHubError.rateLimited(resetAt: retryAfter ?? rate.reset)
            }
            throw GitHubError.http(http.statusCode)
        default:
            throw GitHubError.http(http.statusCode)
        }
    }

    static func rateLimit(from response: HTTPURLResponse) -> RateLimitStatus {
        RateLimitStatus(
            limit: response.value(forHTTPHeaderField: "x-ratelimit-limit").flatMap(Int.init),
            remaining: response.value(forHTTPHeaderField: "x-ratelimit-remaining").flatMap(Int.init),
            reset: response.value(forHTTPHeaderField: "x-ratelimit-reset").flatMap(Double.init).map(Date.init(timeIntervalSince1970:))
        )
    }
}

/// Write access used to publish a product's GitHub release. Separate from update checks,
/// which only read.
public protocol GitHubReleasing: Sendable {
    func commitExists(_ sha: String, in repository: GitHubRepository, token: String) async throws -> Bool
    /// The release for `tag`, or nil when there is none.
    func release(tag: String, in repository: GitHubRepository, token: String) async throws -> GitHubRelease?
    func createRelease(in repository: GitHubRepository, tag: String, commit: String, name: String, body: String, draft: Bool, prerelease: Bool, token: String) async throws -> GitHubRelease
    func uploadAsset(_ file: URL, name: String, to release: GitHubRelease, token: String) async throws
}

extension GitHubService: GitHubReleasing {
    private func request(_ url: URL, method: String = "GET", token: String) -> URLRequest {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 60)
        request.httpMethod = method
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue(Self.apiVersion, forHTTPHeaderField: "X-GitHub-Api-Version")
        request.setValue("ReleaseDock", forHTTPHeaderField: "User-Agent")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        return request
    }

    private func send(_ request: URLRequest, body: Data? = nil) async throws -> (Data, HTTPURLResponse) {
        let (data, response): (Data, URLResponse)
        if let body {
            (data, response) = try await session.upload(for: request, from: body)
        } else {
            (data, response) = try await session.data(for: request)
        }
        guard let http = response as? HTTPURLResponse else { throw GitHubError.invalidResponse }
        switch http.statusCode {
        case 401: throw GitHubError.unauthorized
        case 403 where http.value(forHTTPHeaderField: "x-ratelimit-remaining") == "0":
            throw GitHubError.rateLimited(resetAt: Self.rateLimit(from: http).reset)
        case 403: throw GitHubError.forbidden
        default: return (data, http)
        }
    }

    private func url(_ path: String) throws -> URL {
        guard let url = URL(string: path, relativeTo: baseURL) else { throw GitHubError.invalidResponse }
        return url
    }

    private static func decoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }

    public func commitExists(_ sha: String, in repository: GitHubRepository, token: String) async throws -> Bool {
        let (_, http) = try await send(request(try url("repos/\(repository.fullName)/commits/\(sha)"), token: token))
        switch http.statusCode {
        case 200: return true
        case 404, 422: return false
        default: throw GitHubError.http(http.statusCode)
        }
    }

    public func release(tag: String, in repository: GitHubRepository, token: String) async throws -> GitHubRelease? {
        let encoded = tag.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? tag
        let (data, http) = try await send(request(try url("repos/\(repository.fullName)/releases/tags/\(encoded)"), token: token))
        switch http.statusCode {
        case 200: return try Self.decoder().decode(GitHubRelease.self, from: data)
        case 404: return nil
        default: throw GitHubError.http(http.statusCode)
        }
    }

    public func createRelease(in repository: GitHubRepository, tag: String, commit: String, name: String, body: String, draft: Bool, prerelease: Bool, token: String) async throws -> GitHubRelease {
        var request = request(try url("repos/\(repository.fullName)/releases"), method: "POST", token: token)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let payload: [String: Any] = [
            "tag_name": tag, "target_commitish": commit, "name": name, "body": body,
            "draft": draft, "prerelease": prerelease,
        ]
        request.httpBody = try JSONSerialization.data(withJSONObject: payload)
        let (data, http) = try await send(request)
        switch http.statusCode {
        case 201: return try Self.decoder().decode(GitHubRelease.self, from: data)
        case 404: throw GitHubError.notFound
        case 422: throw GitHubError.validationFailed(Self.errorMessage(data) ?? "invalid release")
        default: throw GitHubError.http(http.statusCode)
        }
    }

    public func uploadAsset(_ file: URL, name: String, to release: GitHubRelease, token: String) async throws {
        guard let template = release.uploadURL,
              let base = template.split(separator: "{").first,
              var components = URLComponents(string: String(base))
        else { throw GitHubError.invalidResponse }
        components.queryItems = [URLQueryItem(name: "name", value: name)]
        guard let uploadURL = components.url else { throw GitHubError.invalidResponse }
        var request = request(uploadURL, method: "POST", token: token)
        request.setValue("application/zip", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 600
        let (data, http) = try await send(request, body: try Data(contentsOf: file))
        switch http.statusCode {
        case 201: return
        case 422: throw GitHubError.validationFailed(Self.errorMessage(data) ?? "asset already exists")
        default: throw GitHubError.http(http.statusCode)
        }
    }

    /// `{"message": "Validation Failed", "errors": [{"code": "already_exists", ...}]}`
    static func errorMessage(_ data: Data) -> String? {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        let codes = (object["errors"] as? [[String: Any]])?.compactMap { $0["message"] as? String ?? $0["code"] as? String } ?? []
        let message = object["message"] as? String
        return ([message].compactMap { $0 } + codes).joined(separator: ": ")
    }
}
