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

/// Read-only WordPress.org API lookups. Used for facts that only the directory knows,
/// such as the current WordPress release that "Tested up to" is measured against.
public protocol WordPressOrgServicing: Sendable {
    /// The latest stable WordPress release, e.g. `7.1.5`.
    func latestWordPressVersion() async throws -> String
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
