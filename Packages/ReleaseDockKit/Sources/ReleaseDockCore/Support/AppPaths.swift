import Foundation

/// Layout of the ReleaseDock data directory:
///
///     ReleaseDockData/
///     ├── Products/   default SVN working copies, one folder per product
///     ├── Staging/    per-release extraction and clean release candidates
///     ├── Releases/   release manifests and reports
///     ├── Backups/    database backups before migrations, working-copy diffs
///     ├── Logs/       exported diagnostics
///     └── Database/   SQLite database
public struct AppPaths: Sendable {
    public let root: URL

    public init(root: URL) {
        self.root = root
    }

    /// `~/Library/Application Support/ReleaseDock/ReleaseDockData`, or the absolute path in
    /// `RELEASEDOCK_DATA_DIR` (used for UI testing and isolated profiles).
    public static func defaultRoot(environment: [String: String] = ProcessInfo.processInfo.environment) throws -> URL {
        if let override = environment["RELEASEDOCK_DATA_DIR"], override.hasPrefix("/") {
            return URL(fileURLWithPath: override, isDirectory: true)
        }
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        return support.appendingPathComponent("ReleaseDock/ReleaseDockData", isDirectory: true)
    }

    public var products: URL { root.appendingPathComponent("Products", isDirectory: true) }
    public var staging: URL { root.appendingPathComponent("Staging", isDirectory: true) }
    public var releases: URL { root.appendingPathComponent("Releases", isDirectory: true) }
    public var backups: URL { root.appendingPathComponent("Backups", isDirectory: true) }
    public var logs: URL { root.appendingPathComponent("Logs", isDirectory: true) }
    public var database: URL { root.appendingPathComponent("Database", isDirectory: true) }
    public var databaseFile: URL { database.appendingPathComponent("releasedock.sqlite") }

    public func stagingDirectory(for releaseID: UUID) -> URL {
        staging.appendingPathComponent(releaseID.uuidString, isDirectory: true)
    }

    public func releaseDirectory(slug: String, version: String, releaseID: UUID) -> URL {
        releases
            .appendingPathComponent(slug, isDirectory: true)
            .appendingPathComponent("\(version)-\(releaseID.uuidString.prefix(8))", isDirectory: true)
    }

    public func defaultWorkingCopy(forSlug slug: String, type: ProductType) -> URL {
        products.appendingPathComponent("\(type.rawValue)-\(slug)", isDirectory: true)
            .appendingPathComponent("svn", isDirectory: true)
    }

    /// Creates every directory with owner-only permissions (0700).
    public func createAll() throws {
        let fm = FileManager.default
        for dir in [root, products, staging, releases, backups, logs, database] {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        }
    }
}
