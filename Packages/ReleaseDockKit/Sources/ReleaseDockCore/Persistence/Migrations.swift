import Foundation

/// Ordered schema migrations tracked with `PRAGMA user_version`. Each migration runs
/// in its own transaction; before migrating an existing database a copy is written
/// to `Backups/` so a failed upgrade can be rolled back by hand.
public struct Migration: Sendable {
    public let version: Int
    public let name: String
    public let apply: @Sendable (SQLiteDatabase) throws -> Void

    public init(version: Int, name: String, apply: @escaping @Sendable (SQLiteDatabase) throws -> Void) {
        self.version = version
        self.name = name
        self.apply = apply
    }
}

public enum Migrator {
    public static let migrations: [Migration] = [
        Migration(version: 1, name: "initial schema") { db in
            try db.execute("""
            CREATE TABLE products (
                id TEXT PRIMARY KEY NOT NULL,
                type TEXT NOT NULL,
                slug TEXT NOT NULL,
                data BLOB NOT NULL,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL
            );
            CREATE UNIQUE INDEX products_type_slug ON products(type, slug);

            CREATE TABLE releases (
                id TEXT PRIMARY KEY NOT NULL,
                product_id TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
                version TEXT,
                state TEXT NOT NULL,
                data BLOB NOT NULL,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL
            );
            CREATE INDEX releases_product ON releases(product_id, created_at);
            CREATE INDEX releases_state ON releases(state);

            CREATE TABLE release_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id TEXT NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
                from_state TEXT,
                to_state TEXT NOT NULL,
                note TEXT,
                at REAL NOT NULL
            );
            CREATE INDEX release_events_release ON release_events(release_id, seq);

            CREATE TABLE settings (
                key TEXT PRIMARY KEY NOT NULL,
                value BLOB NOT NULL
            );
            """)
        },
    ]

    public static var latestVersion: Int { migrations.map(\.version).max() ?? 0 }

    /// Brings the database to the latest schema. Refuses to open a database written by
    /// a newer app version rather than risk corrupting it.
    public static func migrate(_ db: SQLiteDatabase, backupDirectory: URL?, now: Date = Date()) throws {
        let current = try db.userVersion
        if current > latestVersion {
            throw DatabaseError.newerSchema(found: current, supported: latestVersion)
        }
        let pending = migrations.filter { $0.version > current }.sorted { $0.version < $1.version }
        guard !pending.isEmpty else { return }

        if current > 0, let source = db.url, let backupDirectory {
            try db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            let stamp = ISO8601DateFormatter().string(from: now).replacingOccurrences(of: ":", with: "-")
            let backup = backupDirectory.appendingPathComponent("releasedock-v\(current)-\(stamp).sqlite")
            try FileManager.default.createDirectory(at: backupDirectory, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: source, to: backup)
            Log.persistence.info("Backed up database before migration to \(backup.lastPathComponent, privacy: .public)")
        }

        for migration in pending {
            do {
                try db.transaction {
                    try migration.apply(db)
                    try db.execute("PRAGMA user_version = \(migration.version)")
                }
                Log.persistence.info("Applied migration \(migration.version) (\(migration.name, privacy: .public))")
            } catch {
                throw DatabaseError.migration(version: migration.version, error.localizedDescription)
            }
        }
    }
}
