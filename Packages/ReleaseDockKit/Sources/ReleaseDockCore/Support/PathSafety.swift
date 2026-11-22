import Foundation

/// Validation of untrusted relative paths (ZIP entry names, folder entries) and
/// safe resolution of those paths beneath a trusted root directory.
public enum PathSafety {
    public enum Violation: Error, Equatable, Sendable, CustomStringConvertible {
        case empty
        case absolutePath
        case parentTraversal
        case backslash
        case nulOrControlCharacter
        case invalidUTF8
        case componentTooLong(String)
        case pathTooLong
        case escapesRoot

        public var description: String {
            switch self {
            case .empty: "Empty path"
            case .absolutePath: "Absolute path"
            case .parentTraversal: "Path contains '..' traversal"
            case .backslash: "Path contains a backslash separator"
            case .nulOrControlCharacter: "Path contains control characters"
            case .invalidUTF8: "Path is not valid UTF-8"
            case .componentTooLong(let c): "Path component longer than 255 bytes: \(c.prefix(40))…"
            case .pathTooLong: "Path longer than 1024 bytes"
            case .escapesRoot: "Path resolves outside the destination directory"
            }
        }
    }

    /// Characters Windows forbids in file names. WordPress sites run on Windows too,
    /// so these are reported as portability warnings.
    public static let windowsReservedCharacters = CharacterSet(charactersIn: "<>:\"|?*")

    /// Validates an untrusted, `/`-separated relative path and returns its normalized
    /// components (empty and `.` components removed). Throws on anything unsafe.
    public static func validateRelativePath(_ raw: String) throws(Violation) -> [String] {
        if raw.isEmpty { throw .empty }
        if raw.utf8.count > 1024 { throw .pathTooLong }
        if raw.hasPrefix("/") || raw.hasPrefix("~") { throw .absolutePath }
        // Drive-letter or UNC style paths from Windows archivers.
        if raw.count >= 2, raw.dropFirst().first == ":" { throw .absolutePath }
        if raw.contains("\\") { throw .backslash }
        if raw.unicodeScalars.contains(where: { $0.value < 0x20 || $0.value == 0x7F }) {
            throw .nulOrControlCharacter
        }
        var components: [String] = []
        for component in raw.split(separator: "/", omittingEmptySubsequences: true) {
            let c = String(component)
            if c == "." { continue }
            if c == ".." { throw .parentTraversal }
            if c.utf8.count > 255 { throw .componentTooLong(c) }
            components.append(c)
        }
        if components.isEmpty { throw .empty }
        return components
    }

    /// Resolves validated components beneath `root`, verifying the standardized
    /// result is still contained by `root`.
    public static func resolve(_ components: [String], under root: URL) throws(Violation) -> URL {
        var url = root
        for c in components { url.appendPathComponent(c, isDirectory: false) }
        guard isContained(url, in: root) else { throw .escapesRoot }
        return url
    }

    /// True when `url` is `root` or lies beneath it after resolving every symbolic link
    /// in the existing part of both paths. Paths that cannot be resolved (for example a
    /// dangling symlink) are treated as not contained.
    public static func isContained(_ url: URL, in root: URL) -> Bool {
        guard let rootPath = canonicalPath(root), let candidate = canonicalPath(url) else { return false }
        if candidate == rootPath { return true }
        let prefix = rootPath.hasSuffix("/") ? rootPath : rootPath + "/"
        return candidate.hasPrefix(prefix)
    }

    /// `realpath(3)` of the deepest existing ancestor (found with `lstat`, so symlinks are
    /// seen as entries rather than followed), with the non-existing remainder appended.
    /// `URL.resolvingSymlinksInPath()` is not used because it leaves paths whose final
    /// component does not exist unresolved.
    static func canonicalPath(_ url: URL) -> String? {
        var current = url.standardizedFileURL.path
        var tail: [String] = []
        var info = stat()
        while lstat(current, &info) != 0 {
            guard current != "/" && !current.isEmpty else { return nil }
            tail.insert((current as NSString).lastPathComponent, at: 0)
            current = (current as NSString).deletingLastPathComponent
        }
        guard let resolved = realpath(current, nil) else { return nil }
        defer { free(resolved) }
        var path = String(cString: resolved)
        for component in tail {
            path = path == "/" ? "/" + component : path + "/" + component
        }
        return path
    }

    /// Validates a user-entered slug: lowercase letters, digits and hyphens only,
    /// matching WordPress.org directory slugs.
    public static func isValidSlug(_ slug: String) -> Bool {
        guard !slug.isEmpty, slug.count <= 200 else { return false }
        guard slug.first != "-", slug.last != "-" else { return false }
        return slug.unicodeScalars.allSatisfy { scalar in
            (scalar >= "a" && scalar <= "z") || (scalar >= "0" && scalar <= "9") || scalar == "-"
        }
    }

    /// Returns a portability problem for a single file-name component, if any.
    public static func portabilityWarning(forComponent name: String) -> String? {
        if name.rangeOfCharacter(from: windowsReservedCharacters) != nil {
            return "Contains characters that are invalid on Windows (< > : \" | ? *)"
        }
        if name != name.trimmingCharacters(in: .whitespaces) {
            return "Has leading or trailing whitespace"
        }
        if name.hasSuffix(".") {
            return "Ends with a period, which Windows strips"
        }
        let reserved: Set<String> = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"]
        let stem = name.split(separator: ".", maxSplits: 1).first.map(String.init)?.uppercased() ?? ""
        if reserved.contains(stem) {
            return "Uses a reserved Windows device name"
        }
        return nil
    }
}
