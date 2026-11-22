import CryptoKit
import Foundation

public enum Hashing {
    /// Streams a file through SHA-256 without loading it fully into memory.
    public static func sha256(ofFile url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().hexString
    }

    public static func sha256(_ data: Data) -> String {
        SHA256.hash(data: data).hexString
    }

    public static func sha256(_ string: String) -> String {
        sha256(Data(string.utf8))
    }

    /// Deterministic digest of a file tree from (relative path, file digest) pairs.
    /// Used as the identity of a folder source and of a release preview.
    public static func treeDigest(_ entries: [(path: String, sha256: String)]) -> String {
        var hasher = SHA256()
        for entry in entries.sorted(by: { $0.path < $1.path }) {
            hasher.update(data: Data(entry.path.utf8))
            hasher.update(data: Data([0]))
            hasher.update(data: Data(entry.sha256.utf8))
            hasher.update(data: Data([0x0A]))
        }
        return hasher.finalize().hexString
    }
}

extension Digest {
    var hexString: String { map { String(format: "%02x", $0) }.joined() }
}

/// CRC-32 (IEEE 802.3), used to verify ZIP entry integrity.
public struct CRC32: Sendable {
    private static let table: [UInt32] = (0..<256).map { i -> UInt32 in
        var c = UInt32(i)
        for _ in 0..<8 { c = (c & 1) != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    public private(set) var value: UInt32 = 0xFFFF_FFFF

    public init() {}

    public mutating func update(_ data: Data) {
        var c = value
        data.withUnsafeBytes { (buffer: UnsafeRawBufferPointer) in
            for byte in buffer {
                c = CRC32.table[Int((c ^ UInt32(byte)) & 0xFF)] ^ (c >> 8)
            }
        }
        value = c
    }

    public var checksum: UInt32 { value ^ 0xFFFF_FFFF }

    public static func checksum(_ data: Data) -> UInt32 {
        var crc = CRC32()
        crc.update(data)
        return crc.checksum
    }
}
