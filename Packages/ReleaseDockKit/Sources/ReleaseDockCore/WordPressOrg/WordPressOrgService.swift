import Foundation

public enum WordPressOrgError: Error, LocalizedError, Equatable {
    case invalidResponse
    case http(Int)

    public var errorDescription: String? {
        switch self {
        case .invalidResponse: "WordPress.org returned an unexpected response."
        case .http(let status): "WordPress.org returned HTTP \(status)."
        }
    }
}

/// What the plugin or theme directory currently offers to sites.
public struct DirectoryListing: Codable, Hashable, Sendable {
    public var version: String?
    public var tested: String?
    public var lastUpdated: String?

    public init(version: String?, tested: String? = nil, lastUpdated: String? = nil) {
        self.version = version
        self.tested = tested
        self.lastUpdated = lastUpdated
    }
}

/// Read-only WordPress.org API lookups. Used for facts that only the directory knows,
/// such as the current WordPress release that "Tested up to" is measured against.
public protocol WordPressOrgServicing: Sendable {
    /// The latest stable WordPress release, e.g. `7.1.5`.
    func latestWordPressVersion() async throws -> String
    /// The directory's listing for `slug`, or nil when the directory has no such product
    /// (not approved yet, or closed).
    func directoryListing(slug: String, type: ProductType) async throws -> DirectoryListing?
}

public struct WordPressOrgService: WordPressOrgServicing {
    private let session: URLSession
    private let baseURL: URL

    public init(session: URLSession = .shared, baseURL: URL = URL(string: "https://api.wordpress.org")!) {
        self.session = session
        self.baseURL = baseURL
    }

    public func latestWordPressVersion() async throws -> String {
        guard let url = URL(string: "core/version-check/1.7/", relativeTo: baseURL) else { throw WordPressOrgError.invalidResponse }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
        request.setValue("ReleaseDock", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw WordPressOrgError.invalidResponse }
        guard http.statusCode == 200 else { throw WordPressOrgError.http(http.statusCode) }
        return try Self.parseLatestVersion(data)
    }

    public func directoryListing(slug: String, type: ProductType) async throws -> DirectoryListing? {
        let action = type == .plugin ? "plugins/info/1.2/?action=plugin_information" : "themes/info/1.2/?action=theme_information"
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789-_")
        guard let encoded = slug.lowercased().addingPercentEncoding(withAllowedCharacters: allowed),
              let url = URL(string: "\(action)&request%5Bslug%5D=\(encoded)&request%5Bfields%5D%5Bsections%5D=0&request%5Bfields%5D%5Bversions%5D=0", relativeTo: baseURL)
        else { throw WordPressOrgError.invalidResponse }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
        request.setValue("ReleaseDock", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw WordPressOrgError.invalidResponse }
        switch http.statusCode {
        case 200: return try Self.parseListing(data)
        case 404: return nil
        default: throw WordPressOrgError.http(http.statusCode)
        }
    }

    static func parseListing(_ data: Data) throws -> DirectoryListing {
        struct Payload: Decodable {
            var version: String?
            var tested: String?
            var last_updated: String?
            var error: String?
        }
        let payload = try JSONDecoder().decode(Payload.self, from: data)
        guard payload.error == nil else { throw WordPressOrgError.invalidResponse }
        return DirectoryListing(version: payload.version, tested: payload.tested, lastUpdated: payload.last_updated)
    }

    /// The first offer is the newest release; older branches follow as `autoupdate` offers.
    static func parseLatestVersion(_ data: Data) throws -> String {
        struct Payload: Decodable {
            struct Offer: Decodable { var current: String? }
            var offers: [Offer]
        }
        let payload = try JSONDecoder().decode(Payload.self, from: data)
        guard let version = payload.offers.compactMap(\.current).first,
              version.range(of: #"^\d+(\.\d+)+$"#, options: .regularExpression) != nil
        else { throw WordPressOrgError.invalidResponse }
        return version
    }
}

/// WordPress release branches. "Tested up to" is compared by branch (`7.1`), not by
/// point release, the way Plugin Check compares it.
public enum WordPressBranch {
    /// `7.1.5` → `7.1`, `7` → `7.0`. Nil for anything that is not a numeric version.
    public static func of(_ version: String) -> String? {
        let trimmed = version.trimmingCharacters(in: .whitespaces)
        guard trimmed.range(of: #"^\d+(\.\d+)*$"#, options: .regularExpression) != nil else { return nil }
        let parts = trimmed.split(separator: ".")
        return "\(parts[0]).\(parts.count > 1 ? parts[1] : "0")"
    }

    /// Negative when `a`'s branch is older than `b`'s, zero when equal, positive when newer.
    public static func compare(_ a: String, _ b: String) -> Int? {
        guard let a = of(a), let b = of(b) else { return nil }
        return PHPVersion.compare(a, b)
    }
}
