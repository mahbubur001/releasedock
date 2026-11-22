import Compression
import Foundation

public enum ZIPError: Error, LocalizedError, Equatable, Sendable {
    case fileTooLarge(UInt64)
    case notAZIP
    case corrupt(String)
    case unsupported(String)
    case entryTooLarge(String)
    case checksumMismatch(String)
    case destinationExists(String)

    public var errorDescription: String? {
        switch self {
        case .fileTooLarge(let size): "ZIP file is too large (\(size) bytes)"
        case .notAZIP: "The file is not a ZIP archive"
        case .corrupt(let detail): "Corrupt ZIP archive: \(detail)"
        case .unsupported(let detail): "Unsupported ZIP feature: \(detail)"
        case .entryTooLarge(let name): "Entry exceeds its declared size: \(name)"
        case .checksumMismatch(let name): "CRC-32 mismatch for \(name)"
        case .destinationExists(let path): "Refusing to overwrite existing file: \(path)"
        }
    }
}

public struct ZIPEntry: Hashable, Sendable {
    public var rawName: Data
    /// Decoded name, or nil when the name is not valid UTF-8.
    public var name: String?
    public var flags: UInt16
    public var method: UInt16
    public var crc32: UInt32
    public var compressedSize: UInt64
    public var uncompressedSize: UInt64
    public var localHeaderOffset: UInt64
    public var versionMadeBy: UInt16
    public var externalAttributes: UInt32

    public var isEncrypted: Bool { flags & 0x0001 != 0 }

    /// Unix mode bits when the archive was created on a Unix host.
    public var unixMode: UInt32? {
        guard versionMadeBy >> 8 == 3 else { return nil }
        let mode = externalAttributes >> 16
        return mode == 0 ? nil : mode
    }

    public var isSymlink: Bool {
        guard let mode = unixMode else { return false }
        return mode & 0o170000 == 0o120000
    }

    public var isDirectory: Bool {
        if let name, name.hasSuffix("/") { return true }
        if let mode = unixMode { return mode & 0o170000 == 0o040000 }
        return externalAttributes & 0x10 != 0
    }

    /// FIFOs, devices, sockets — anything that is not a file, directory or symlink.
    public var isSpecialFile: Bool {
        guard let mode = unixMode else { return false }
        let type = mode & 0o170000
        return type != 0o100000 && type != 0o040000 && type != 0o120000
    }
}

/// Minimal, defensive ZIP reader. It parses the central directory itself (including
/// ZIP64), validates every structure against the file bounds, cross-checks local
/// headers, and supports only the *stored* and *deflate* methods. Nothing is written
/// to disk except through `extract(_:to:)`, which never overwrites or follows links.
public final class ZIPArchive: @unchecked Sendable {
    public static let maximumArchiveSize: UInt64 = 1 << 30 // 1 GiB

    public let url: URL
    public let entries: [ZIPEntry]
    private let data: Data

    public init(url: URL) throws {
        self.url = url
        let size = (try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.uint64Value ?? 0
        guard size <= Self.maximumArchiveSize else { throw ZIPError.fileTooLarge(size) }
        data = try Data(contentsOf: url, options: [.alwaysMapped])
        entries = try Self.readCentralDirectory(data)
    }

    // MARK: Reading primitives

    private static func u16(_ d: Data, _ offset: Int) throws -> UInt16 {
        guard offset >= 0, offset <= d.count - 2 else { throw ZIPError.corrupt("read past end of file") }
        let s = d.startIndex + offset
        return UInt16(d[s]) | UInt16(d[s + 1]) << 8
    }

    private static func u32(_ d: Data, _ offset: Int) throws -> UInt32 {
        guard offset >= 0, offset <= d.count - 4 else { throw ZIPError.corrupt("read past end of file") }
        let s = d.startIndex + offset
        return UInt32(d[s]) | UInt32(d[s + 1]) << 8 | UInt32(d[s + 2]) << 16 | UInt32(d[s + 3]) << 24
    }

    private static func u64(_ d: Data, _ offset: Int) throws -> UInt64 {
        UInt64(try u32(d, offset)) | UInt64(try u32(d, offset + 4)) << 32
    }

    /// Converts an untrusted offset or size. Nothing in a valid archive can exceed the
    /// archive size limit, and bounding values here keeps later additions overflow-free.
    private static func int(_ value: UInt64, _ what: String) throws -> Int {
        guard value <= maximumArchiveSize else { throw ZIPError.corrupt("\(what) out of range") }
        return Int(value)
    }

    // MARK: Central directory

    static func readCentralDirectory(_ d: Data) throws -> [ZIPEntry] {
        guard d.count >= 22 else { throw ZIPError.notAZIP }
        // End of central directory: search backwards over the maximum comment length.
        let searchStart = max(0, d.count - 22 - 0xFFFF)
        var eocd: Int?
        var i = d.count - 22
        while i >= searchStart {
            if try u32(d, i) == 0x0605_4B50 {
                // The comment length must reach exactly to the end of the file.
                let commentLength = Int(try u16(d, i + 20))
                if i + 22 + commentLength == d.count { eocd = i; break }
            }
            i -= 1
        }
        guard let eocd else { throw ZIPError.notAZIP }

        let diskNumber = try u16(d, eocd + 4)
        let cdDisk = try u16(d, eocd + 6)
        var totalEntries = UInt64(try u16(d, eocd + 10))
        var cdSize = UInt64(try u32(d, eocd + 12))
        var cdOffset = UInt64(try u32(d, eocd + 16))
        if diskNumber != 0 || cdDisk != 0 { throw ZIPError.unsupported("multi-disk archives") }

        // ZIP64 end of central directory.
        if totalEntries == 0xFFFF || cdSize == 0xFFFF_FFFF || cdOffset == 0xFFFF_FFFF {
            let locator = eocd - 20
            guard locator >= 0, try u32(d, locator) == 0x0706_4B50 else {
                throw ZIPError.corrupt("missing ZIP64 end of central directory locator")
            }
            let z64 = try int(try u64(d, locator + 8), "ZIP64 record offset")
            guard try u32(d, z64) == 0x0606_4B50 else { throw ZIPError.corrupt("invalid ZIP64 end of central directory") }
            totalEntries = try u64(d, z64 + 32)
            cdSize = try u64(d, z64 + 40)
            cdOffset = try u64(d, z64 + 48)
        }

        let cdStart = try int(cdOffset, "central directory offset")
        let cdLength = try int(cdSize, "central directory size")
        guard cdStart + cdLength <= eocd else { throw ZIPError.corrupt("central directory outside file bounds") }
        guard totalEntries <= 100_000 else { throw ZIPError.unsupported("more than 100000 entries") }

        var entries: [ZIPEntry] = []
        entries.reserveCapacity(Int(totalEntries))
        var p = cdStart
        for _ in 0..<totalEntries {
            guard try u32(d, p) == 0x0201_4B50 else { throw ZIPError.corrupt("invalid central directory header") }
            let versionMadeBy = try u16(d, p + 4)
            let flags = try u16(d, p + 8)
            let method = try u16(d, p + 10)
            let crc = try u32(d, p + 16)
            var compressed = UInt64(try u32(d, p + 20))
            var uncompressed = UInt64(try u32(d, p + 24))
            let nameLength = Int(try u16(d, p + 28))
            let extraLength = Int(try u16(d, p + 30))
            let commentLength = Int(try u16(d, p + 32))
            let diskStart = try u16(d, p + 34)
            let external = try u32(d, p + 38)
            var localOffset = UInt64(try u32(d, p + 42))
            let nameStart = p + 46
            let extraStart = nameStart + nameLength
            let next = extraStart + extraLength + commentLength
            guard next <= cdStart + cdLength else { throw ZIPError.corrupt("central directory entry overruns directory") }

            if diskStart != 0 && diskStart != 0xFFFF { throw ZIPError.unsupported("multi-disk archives") }
            let rawName = d.subdata(in: (d.startIndex + nameStart)..<(d.startIndex + extraStart))

            // ZIP64 extended information extra field.
            var e = extraStart
            while e + 4 <= extraStart + extraLength {
                let headerID = try u16(d, e)
                let size = Int(try u16(d, e + 2))
                guard e + 4 + size <= extraStart + extraLength else { throw ZIPError.corrupt("extra field overruns entry") }
                if headerID == 0x0001 {
                    var f = e + 4
                    if uncompressed == 0xFFFF_FFFF { uncompressed = try u64(d, f); f += 8 }
                    if compressed == 0xFFFF_FFFF { compressed = try u64(d, f); f += 8 }
                    if localOffset == 0xFFFF_FFFF { localOffset = try u64(d, f); f += 8 }
                    guard f <= e + 4 + size else { throw ZIPError.corrupt("truncated ZIP64 extra field") }
                }
                e += 4 + size
            }

            let utf8Flag = flags & 0x0800 != 0
            var name = String(data: rawName, encoding: .utf8)
            // Without the UTF-8 flag, names are CP437 by spec; ASCII is identical in both,
            // and macOS tools write UTF-8 without setting the flag. Non-UTF-8 stays nil.
            if name == nil, !utf8Flag, rawName.allSatisfy({ $0 < 0x80 }) {
                name = String(decoding: rawName, as: UTF8.self)
            }

            entries.append(ZIPEntry(
                rawName: rawName, name: name, flags: flags, method: method, crc32: crc,
                compressedSize: compressed, uncompressedSize: uncompressed, localHeaderOffset: localOffset,
                versionMadeBy: versionMadeBy, externalAttributes: external
            ))
            p = next
        }

        try validateLayout(d, entries: entries, centralDirectoryStart: cdStart)
        return entries
    }

    /// Ensures local headers exist, agree with the central directory, lie before the
    /// central directory and do not overlap (overlapping entries are a zip-bomb technique).
    private static func validateLayout(_ d: Data, entries: [ZIPEntry], centralDirectoryStart: Int) throws {
        var ranges: [(start: Int, end: Int)] = []
        for entry in entries {
            let (dataStart, localName) = try localHeader(d, entry: entry)
            if localName != entry.rawName {
                throw ZIPError.corrupt("local file name differs from central directory for \(entry.name ?? "<binary name>")")
            }
            let end = dataStart + (try int(entry.compressedSize, "compressed size"))
            guard end <= centralDirectoryStart else { throw ZIPError.corrupt("entry data overlaps central directory") }
            ranges.append((try int(entry.localHeaderOffset, "local header offset"), end))
        }
        ranges.sort { $0.start < $1.start }
        for (a, b) in zip(ranges, ranges.dropFirst()) where b.start < a.end {
            throw ZIPError.corrupt("overlapping entries")
        }
    }

    /// Returns the offset of the entry's data and the name stored in the local header.
    private static func localHeader(_ d: Data, entry: ZIPEntry) throws -> (dataStart: Int, name: Data) {
        let offset = try int(entry.localHeaderOffset, "local header offset")
        guard try u32(d, offset) == 0x0403_4B50 else { throw ZIPError.corrupt("invalid local file header") }
        let nameLength = Int(try u16(d, offset + 26))
        let extraLength = Int(try u16(d, offset + 28))
        let nameStart = offset + 30
        let dataStart = nameStart + nameLength + extraLength
        guard dataStart <= d.count else { throw ZIPError.corrupt("local header overruns file") }
        return (dataStart, d.subdata(in: (d.startIndex + nameStart)..<(d.startIndex + nameStart + nameLength)))
    }

    // MARK: Extraction

    /// Streams the entry's decompressed bytes to `sink`, enforcing the declared size and CRC-32.
    public func read(_ entry: ZIPEntry, sink: (Data) throws -> Void) throws {
        try withoutActuallyEscaping(sink) { sink in try read(entry, escapingSink: sink) }
    }

    private func read(_ entry: ZIPEntry, escapingSink sink: @escaping (Data) throws -> Void) throws {
        guard !entry.isEncrypted else { throw ZIPError.unsupported("encrypted entries") }
        let label = entry.name ?? "<binary name>"
        let (dataStart, _) = try Self.localHeader(data, entry: entry)
        let length = try Self.int(entry.compressedSize, "compressed size")
        let compressed = data.subdata(in: (data.startIndex + dataStart)..<(data.startIndex + dataStart + length))
        var crc = CRC32()
        var produced: UInt64 = 0

        func emit(_ chunk: Data) throws {
            produced += UInt64(chunk.count)
            guard produced <= entry.uncompressedSize else { throw ZIPError.entryTooLarge(label) }
            crc.update(chunk)
            try sink(chunk)
        }

        switch entry.method {
        case 0:
            guard entry.compressedSize == entry.uncompressedSize else { throw ZIPError.corrupt("stored entry size mismatch for \(label)") }
            var offset = 0
            while offset < compressed.count {
                let end = min(offset + (1 << 20), compressed.count)
                try emit(compressed.subdata(in: offset..<end))
                offset = end
            }
        case 8:
            // COMPRESSION_ZLIB is raw DEFLATE (RFC 1951), which is what ZIP stores.
            let filter = try OutputFilter(.decompress, using: .zlib) { chunk in
                if let chunk { try emit(chunk) }
            }
            var offset = 0
            while offset < compressed.count {
                let end = min(offset + (1 << 16), compressed.count)
                try filter.write(compressed.subdata(in: offset..<end))
                offset = end
            }
            try filter.finalize()
        default:
            throw ZIPError.unsupported("compression method \(entry.method) for \(label)")
        }

        guard produced == entry.uncompressedSize else { throw ZIPError.corrupt("size mismatch for \(label)") }
        guard crc.checksum == entry.crc32 else { throw ZIPError.checksumMismatch(label) }
    }

    /// Writes a regular-file entry to `destination`. The destination must not exist;
    /// the file is created exclusively (`O_CREAT|O_EXCL|O_NOFOLLOW`) with mode 0644.
    /// Returns the SHA-256 of the written content.
    @discardableResult
    public func extract(_ entry: ZIPEntry, to destination: URL) throws -> String {
        let fd = open(destination.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o644)
        guard fd >= 0 else {
            if errno == EEXIST { throw ZIPError.destinationExists(destination.path) }
            throw CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: destination.path])
        }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        var hasher = SHA256Stream()
        do {
            try read(entry) { chunk in
                hasher.update(chunk)
                try handle.write(contentsOf: chunk)
            }
            try handle.close()
        } catch {
            try? handle.close()
            try? FileManager.default.removeItem(at: destination)
            throw error
        }
        return hasher.finalize()
    }
}

import CryptoKit

struct SHA256Stream {
    private var hasher = SHA256()
    mutating func update(_ data: Data) { hasher.update(data: data) }
    func finalize() -> String { hasher.finalize().hexString }
}
