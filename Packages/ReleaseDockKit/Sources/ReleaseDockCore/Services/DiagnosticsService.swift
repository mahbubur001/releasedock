import Foundation
import OSLog

/// Builds a diagnostic bundle (JSON) for support: environment, products (without
/// credentials), recent releases with their state history, and recent ReleaseDock
/// log entries. Everything passes through `SecretRedactor`.
public struct DiagnosticsService: Sendable {
    private let products: ProductRepository
    private let releases: ReleaseRepository
    private let paths: AppPaths

    public init(products: ProductRepository, releases: ReleaseRepository, paths: AppPaths) {
        self.products = products
        self.releases = releases
        self.paths = paths
    }

    struct Bundle: Encodable {
        struct ProductInfo: Encodable {
            var name: String
            var slug: String
            var type: String
            var svnURL: String
            var mode: String
            var automationEnabled: Bool
            var currentVersion: String?
            var hasSVNUsername: Bool
        }
        struct ReleaseInfo: Encodable {
            var id: String
            var product: String
            var version: String?
            var state: String
            var mode: String
            var committedRevision: Int?
            var errors: [String]
            var transitions: [String]
            var blocking: [String]
        }
        var generatedAt: Date
        var app: String
        var os: String
        var svn: String?
        var products: [ProductInfo]
        var releases: [ReleaseInfo]
        var log: [String]
    }

    /// Writes the bundle into `Logs/` and returns its URL.
    public func export(appVersion: String, svnVersion: String?, now: Date = Date()) throws -> URL {
        let bundle = Bundle(
            generatedAt: now,
            app: appVersion,
            os: ProcessInfo.processInfo.operatingSystemVersionString,
            svn: svnVersion,
            products: try products.allProducts().map {
                .init(name: $0.name, slug: $0.slug, type: $0.type.rawValue, svnURL: SecretRedactor.redact($0.svnRepositoryURL.absoluteString), mode: $0.releaseMode.rawValue, automationEnabled: $0.automationEnabled, currentVersion: $0.currentVersion, hasSVNUsername: $0.svnUsername?.isEmpty == false)
            },
            releases: try releases.releases(productID: nil).prefix(50).map { r in
                .init(
                    id: r.id.uuidString, product: r.productSlug, version: r.version, state: r.state.rawValue, mode: r.mode.rawValue,
                    committedRevision: r.committedRevision, errors: r.errors.map(SecretRedactor.redact),
                    transitions: r.transitions.map { "\($0.at.formatted(.iso8601)) \($0.from?.rawValue ?? "-") → \($0.to.rawValue)\($0.note.map { ": " + SecretRedactor.redact($0) } ?? "")" },
                    blocking: r.validation?.blockingResults.map { "\($0.name): \($0.message)" } ?? []
                )
            },
            log: Self.recentLogEntries(since: now.addingTimeInterval(-24 * 3600))
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        try FileManager.default.createDirectory(at: paths.logs, withIntermediateDirectories: true)
        let stamp = ISO8601DateFormatter().string(from: now).replacingOccurrences(of: ":", with: "-")
        let url = paths.logs.appendingPathComponent("ReleaseDock-diagnostics-\(stamp).json")
        try encoder.encode(bundle).write(to: url, options: [.atomic])
        return url
    }

    /// Reads this process's ReleaseDock log entries. Values logged as private remain redacted.
    static func recentLogEntries(since date: Date) -> [String] {
        guard let store = try? OSLogStore(scope: .currentProcessIdentifier) else { return [] }
        let position = store.position(date: date)
        let predicate = NSPredicate(format: "subsystem == %@", Log.subsystem)
        guard let entries = try? store.getEntries(at: position, matching: predicate) else { return [] }
        return entries.compactMap { $0 as? OSLogEntryLog }.suffix(2000).map {
            "\($0.date.formatted(.iso8601)) [\($0.category)] \(SecretRedactor.redact($0.composedMessage))"
        }
    }
}
