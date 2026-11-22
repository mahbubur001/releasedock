import Foundation
import SQLite3

public enum DatabaseError: Error, LocalizedError, Sendable {
    case open(String)
    case prepare(String, sql: String)
    case step(String)
    case migration(version: Int, String)
    case newerSchema(found: Int, supported: Int)

    public var errorDescription: String? {
        switch self {
        case .open(let m): "Could not open the database: \(m)"
        case .prepare(let m, let sql): "SQL error: \(m) in \(sql.prefix(80))"
        case .step(let m): "SQL execution error: \(m)"
        case .migration(let v, let m): "Database migration \(v) failed: \(m)"
        case .newerSchema(let found, let supported):
            "The database was created by a newer ReleaseDock (schema \(found), this version supports \(supported)). Update ReleaseDock."
        }
    }
}

public enum SQLValue: Sendable, Equatable {
    case null
    case integer(Int64)
    case real(Double)
    case text(String)
    case blob(Data)
}

/// A small, serialized SQLite wrapper. All access goes through one connection
/// guarded by a lock; WAL mode and `synchronous=FULL` favour durability of release
/// state over write throughput.
public final class SQLiteDatabase: @unchecked Sendable {
    private var handle: OpaquePointer?
    private let lock = NSRecursiveLock()
    public let url: URL?

    /// Opens (creating if needed) a database file, or an in-memory database when `url` is nil.
    public init(url: URL?) throws {
        self.url = url
        let path = url?.path ?? ":memory:"
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
        guard sqlite3_open_v2(path, &handle, flags, nil) == SQLITE_OK else {
            let message = handle.map { String(cString: sqlite3_errmsg($0)) } ?? "unknown"
            sqlite3_close(handle)
            throw DatabaseError.open(message)
        }
        if let url {
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            try execute("PRAGMA journal_mode=WAL")
        }
        try execute("PRAGMA foreign_keys=ON")
        try execute("PRAGMA synchronous=FULL")
        sqlite3_busy_timeout(handle, 5000)
    }

    deinit {
        sqlite3_close_v2(handle)
    }

    private var errorMessage: String { String(cString: sqlite3_errmsg(handle)) }

    public func execute(_ sql: String) throws {
        try lock.withLock {
            var error: UnsafeMutablePointer<CChar>?
            if sqlite3_exec(handle, sql, nil, nil, &error) != SQLITE_OK {
                let message = error.map { String(cString: $0) } ?? errorMessage
                sqlite3_free(error)
                throw DatabaseError.step(message)
            }
        }
    }

    public func run(_ sql: String, _ bindings: [SQLValue] = []) throws {
        try lock.withLock {
            let statement = try prepare(sql, bindings)
            defer { sqlite3_finalize(statement) }
            let rc = sqlite3_step(statement)
            guard rc == SQLITE_DONE || rc == SQLITE_ROW else { throw DatabaseError.step(errorMessage) }
        }
    }

    public func query(_ sql: String, _ bindings: [SQLValue] = []) throws -> [[SQLValue]] {
        try lock.withLock {
            let statement = try prepare(sql, bindings)
            defer { sqlite3_finalize(statement) }
            var rows: [[SQLValue]] = []
            while true {
                let rc = sqlite3_step(statement)
                if rc == SQLITE_DONE { break }
                guard rc == SQLITE_ROW else { throw DatabaseError.step(errorMessage) }
                let columns = sqlite3_column_count(statement)
                var row: [SQLValue] = []
                for i in 0..<columns {
                    switch sqlite3_column_type(statement, i) {
                    case SQLITE_INTEGER: row.append(.integer(sqlite3_column_int64(statement, i)))
                    case SQLITE_FLOAT: row.append(.real(sqlite3_column_double(statement, i)))
                    case SQLITE_TEXT: row.append(.text(String(cString: sqlite3_column_text(statement, i))))
                    case SQLITE_BLOB:
                        let count = Int(sqlite3_column_bytes(statement, i))
                        if let bytes = sqlite3_column_blob(statement, i), count > 0 {
                            row.append(.blob(Data(bytes: bytes, count: count)))
                        } else {
                            row.append(.blob(Data()))
                        }
                    default: row.append(.null)
                    }
                }
                rows.append(row)
            }
            return rows
        }
    }

    /// Runs `body` inside a transaction, rolling back on error.
    public func transaction<T>(_ body: () throws -> T) throws -> T {
        try lock.withLock {
            try execute("BEGIN IMMEDIATE")
            do {
                let value = try body()
                try execute("COMMIT")
                return value
            } catch {
                try? execute("ROLLBACK")
                throw error
            }
        }
    }

    public var userVersion: Int {
        get throws {
            guard case .integer(let v)? = try query("PRAGMA user_version").first?.first else { return 0 }
            return Int(v)
        }
    }

    private static let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

    private func prepare(_ sql: String, _ bindings: [SQLValue]) throws -> OpaquePointer? {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(handle, sql, -1, &statement, nil) == SQLITE_OK else {
            throw DatabaseError.prepare(errorMessage, sql: sql)
        }
        for (index, value) in bindings.enumerated() {
            let i = Int32(index + 1)
            let rc: Int32
            switch value {
            case .null: rc = sqlite3_bind_null(statement, i)
            case .integer(let v): rc = sqlite3_bind_int64(statement, i, v)
            case .real(let v): rc = sqlite3_bind_double(statement, i, v)
            case .text(let v): rc = sqlite3_bind_text(statement, i, v, -1, Self.transient)
            case .blob(let v):
                rc = v.withUnsafeBytes { sqlite3_bind_blob(statement, i, $0.baseAddress, Int32(v.count), Self.transient) }
            }
            guard rc == SQLITE_OK else {
                sqlite3_finalize(statement)
                throw DatabaseError.prepare(errorMessage, sql: sql)
            }
        }
        return statement
    }
}

extension SQLValue {
    public var stringValue: String? {
        switch self {
        case .text(let s): s
        case .blob(let d): String(data: d, encoding: .utf8)
        case .integer(let i): String(i)
        default: nil
        }
    }

    public var dataValue: Data? {
        switch self {
        case .blob(let d): d
        case .text(let s): Data(s.utf8)
        default: nil
        }
    }

    public var intValue: Int? {
        if case .integer(let i) = self { return Int(i) }
        return nil
    }
}
