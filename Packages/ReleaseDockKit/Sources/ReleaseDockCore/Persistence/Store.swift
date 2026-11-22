import Foundation

public protocol ProductRepository: Sendable {
    func allProducts() throws -> [Product]
    func product(id: UUID) throws -> Product?
    func save(_ product: Product) throws
    func deleteProduct(id: UUID) throws
}

public protocol ReleaseRepository: Sendable {
    /// Inserts or updates the release and appends any new state transitions to the event log.
    func save(_ release: ReleaseRecord) throws
    func release(id: UUID) throws -> ReleaseRecord?
    func releases(productID: UUID?) throws -> [ReleaseRecord]
    /// Releases that are not in a terminal state.
    func activeReleases() throws -> [ReleaseRecord]
}

public protocol SettingsRepository: Sendable {
    func setting<T: Decodable>(_ type: T.Type, forKey key: String) throws -> T?
    func setSetting<T: Encodable>(_ value: T, forKey key: String) throws
}

public enum StoreError: Error, LocalizedError, Sendable {
    case duplicateProduct(type: ProductType, slug: String)
    case corruptRecord(String)

    public var errorDescription: String? {
        switch self {
        case .duplicateProduct(let type, let slug): "A \(type.rawValue) with the slug \"\(slug)\" is already registered."
        case .corruptRecord(let id): "Stored record \(id) could not be decoded."
        }
    }
}

/// SQLite-backed persistence for products, releases (with an append-only state
/// event log) and settings. Records are stored as JSON with indexed key columns.
public final class Store: ProductRepository, ReleaseRepository, SettingsRepository, @unchecked Sendable {
    public let db: SQLiteDatabase
    private let encoder: JSONEncoder
    private let decoder = JSONDecoder()

    public init(database: SQLiteDatabase, backupDirectory: URL? = nil) throws {
        db = database
        encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try Migrator.migrate(database, backupDirectory: backupDirectory)
    }

    /// Opens the on-disk store in the standard data directory.
    public static func open(paths: AppPaths) throws -> Store {
        try paths.createAll()
        return try Store(database: SQLiteDatabase(url: paths.databaseFile), backupDirectory: paths.backups)
    }

    public static func inMemory() throws -> Store {
        try Store(database: SQLiteDatabase(url: nil))
    }

    // MARK: Products

    public func allProducts() throws -> [Product] {
        try db.query("SELECT id, data FROM products ORDER BY slug").map { try decode(Product.self, $0) }
    }

    public func product(id: UUID) throws -> Product? {
        try db.query("SELECT id, data FROM products WHERE id = ?", [.text(id.uuidString)]).first.map { try decode(Product.self, $0) }
    }

    public func save(_ product: Product) throws {
        let conflict = try db.query(
            "SELECT id FROM products WHERE type = ? AND slug = ? AND id <> ?",
            [.text(product.type.rawValue), .text(product.slug), .text(product.id.uuidString)]
        )
        guard conflict.isEmpty else { throw StoreError.duplicateProduct(type: product.type, slug: product.slug) }
        try db.run("""
            INSERT INTO products (id, type, slug, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET type = excluded.type, slug = excluded.slug, data = excluded.data, updated_at = excluded.updated_at
            """, [
                .text(product.id.uuidString), .text(product.type.rawValue), .text(product.slug),
                .blob(try encoder.encode(product)),
                .real(product.createdAt.timeIntervalSince1970), .real(product.updatedAt.timeIntervalSince1970),
            ])
    }

    public func deleteProduct(id: UUID) throws {
        try db.run("DELETE FROM products WHERE id = ?", [.text(id.uuidString)])
    }

    // MARK: Releases

    public func save(_ release: ReleaseRecord) throws {
        try db.transaction {
            try db.run("""
                INSERT INTO releases (id, product_id, version, state, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET version = excluded.version, state = excluded.state, data = excluded.data, updated_at = excluded.updated_at
                """, [
                    .text(release.id.uuidString), .text(release.productID.uuidString),
                    release.version.map(SQLValue.text) ?? .null, .text(release.state.rawValue),
                    .blob(try encoder.encode(release)),
                    .real(release.createdAt.timeIntervalSince1970), .real(release.updatedAt.timeIntervalSince1970),
                ])
            let stored = try db.query("SELECT COUNT(*) FROM release_events WHERE release_id = ?", [.text(release.id.uuidString)])
                .first?.first?.intValue ?? 0
            for transition in release.transitions.dropFirst(stored) {
                try db.run(
                    "INSERT INTO release_events (release_id, from_state, to_state, note, at) VALUES (?, ?, ?, ?, ?)",
                    [
                        .text(release.id.uuidString), transition.from.map { .text($0.rawValue) } ?? .null,
                        .text(transition.to.rawValue), transition.note.map(SQLValue.text) ?? .null,
                        .real(transition.at.timeIntervalSince1970),
                    ]
                )
            }
        }
    }

    public func release(id: UUID) throws -> ReleaseRecord? {
        try db.query("SELECT id, data FROM releases WHERE id = ?", [.text(id.uuidString)]).first.map { try decode(ReleaseRecord.self, $0) }
    }

    public func releases(productID: UUID?) throws -> [ReleaseRecord] {
        if let productID {
            return try db.query("SELECT id, data FROM releases WHERE product_id = ? ORDER BY created_at DESC", [.text(productID.uuidString)])
                .map { try decode(ReleaseRecord.self, $0) }
        }
        return try db.query("SELECT id, data FROM releases ORDER BY created_at DESC").map { try decode(ReleaseRecord.self, $0) }
    }

    public func activeReleases() throws -> [ReleaseRecord] {
        let terminal = [ReleaseState.completed, .failed, .cancelled].map { "'\($0.rawValue)'" }.joined(separator: ",")
        return try db.query("SELECT id, data FROM releases WHERE state NOT IN (\(terminal)) ORDER BY created_at DESC")
            .map { try decode(ReleaseRecord.self, $0) }
    }

    /// Raw event log for a release, for diagnostics.
    public func events(releaseID: UUID) throws -> [StateTransition] {
        try db.query("SELECT from_state, to_state, note, at FROM release_events WHERE release_id = ? ORDER BY seq", [.text(releaseID.uuidString)])
            .map { row in
                guard let to = row[1].stringValue.flatMap(ReleaseState.init(rawValue:)), case .real(let at) = row[3] else {
                    throw StoreError.corruptRecord(releaseID.uuidString)
                }
                return StateTransition(from: row[0].stringValue.flatMap(ReleaseState.init(rawValue:)), to: to, at: Date(timeIntervalSince1970: at), note: row[2].stringValue)
            }
    }

    // MARK: Settings

    public func setting<T: Decodable>(_ type: T.Type, forKey key: String) throws -> T? {
        guard let data = try db.query("SELECT value FROM settings WHERE key = ?", [.text(key)]).first?.first?.dataValue else { return nil }
        return try decoder.decode(T.self, from: data)
    }

    public func setSetting<T: Encodable>(_ value: T, forKey key: String) throws {
        try db.run(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [.text(key), .blob(try encoder.encode(value))]
        )
    }

    private func decode<T: Decodable>(_ type: T.Type, _ row: [SQLValue]) throws -> T {
        guard row.count == 2, let data = row[1].dataValue else { throw StoreError.corruptRecord(row.first?.stringValue ?? "?") }
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            Log.persistence.error("Failed to decode \(String(describing: T.self), privacy: .public): \(error.localizedDescription, privacy: .public)")
            throw StoreError.corruptRecord(row[0].stringValue ?? "?")
        }
    }
}
