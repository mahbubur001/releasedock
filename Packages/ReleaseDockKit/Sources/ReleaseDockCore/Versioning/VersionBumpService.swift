import Foundation

/// One line the bump changes.
public struct VersionBumpChange: Codable, Hashable, Sendable, Identifiable {
    public var id: String { "\(file):\(line)" }
    public var file: String
    /// 1-based line number, or 0 for an inserted changelog entry.
    public var line: Int
    public var description: String
    public var oldText: String
    public var newText: String
}

/// Everything a version bump will write, computed without touching the folder.
public struct VersionBumpPlan: Codable, Hashable, Sendable {
    public var currentVersion: String
    public var newVersion: String
    public var changes: [VersionBumpChange]
    /// Things the user should know (no changelog entry, version not newer…).
    public var notes: [String]
    /// SHA-256 of each file as planned; applying refuses if any changed meanwhile.
    public var fileHashes: [String: String]
    /// New contents per file.
    var contents: [String: String]

    public var files: [String] { changes.map(\.file).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } } }
}

public enum VersionBumpError: Error, LocalizedError, Equatable, Sendable {
    case noProduct(String)
    case invalidVersion(String)
    case notNewer(current: String, new: String)
    case nothingToChange
    case fileChanged(String)

    public var errorDescription: String? {
        switch self {
        case .noProduct(let reason): "No plugin or theme found in the folder: \(reason)"
        case .invalidVersion(let v): "\"\(v)\" is not a valid version number."
        case .notNewer(let current, let new): "\(new) is not newer than the current version \(current)."
        case .nothingToChange: "Nothing to change: no version references were found."
        case .fileChanged(let file): "\(file) changed after the preview. Review the bump again."
        }
    }
}

/// Updates the version everywhere it is written in a development folder, so the header,
/// `Stable tag`, version constants and `package.json` never drift apart. This is the only
/// place ReleaseDock writes to a development folder, and only after the user approves the
/// planned lines.
public struct VersionBumpService: Sendable {
    public enum Part: String, CaseIterable, Sendable { case major, minor, patch }

    public init() {}

    /// `2.3.9` → patch `2.3.10`, minor `2.4.0`, major `3.0.0`. Pre-release suffixes are dropped.
    public static func suggest(_ part: Part, from version: String) -> String? {
        let core = version.split(separator: "-").first.map(String.init) ?? version
        var numbers = core.split(separator: ".").compactMap { Int($0) }
        guard !numbers.isEmpty, numbers.count == core.split(separator: ".").count else { return nil }
        while numbers.count < 3 { numbers.append(0) }
        switch part {
        case .major: numbers = [numbers[0] + 1, 0, 0]
        case .minor: numbers = [numbers[0], numbers[1] + 1, 0]
        case .patch: numbers[2] += 1
        }
        return numbers.prefix(3).map(String.init).joined(separator: ".")
    }

    public func plan(folder: URL, product: Product, newVersion rawVersion: String, changelogNotes: String = "", today: Date = Date()) throws -> VersionBumpPlan {
        let newVersion = rawVersion.trimmingCharacters(in: .whitespaces)
        guard case .valid = VersionService().validate(newVersion) else { throw VersionBumpError.invalidVersion(newVersion) }
        let detected: DetectedProduct
        do {
            detected = try ProductDetector().detect(at: folder, expectedType: product.type, mainPluginFile: product.mainPluginFile, slug: product.slug)
        } catch {
            throw VersionBumpError.noProduct(error.localizedDescription)
        }
        guard let mainFile = detected.mainFile, let current = detected.version else {
            throw VersionBumpError.noProduct(detected.problems.first ?? "no Version header")
        }
        guard PHPVersion.compare(newVersion, current) > 0 else { throw VersionBumpError.notNewer(current: current, new: newVersion) }

        var editor = Editor(folder: folder)
        // 1. The header WordPress reads.
        try editor.replaceHeader(in: mainFile, header: "Version", old: current, new: newVersion)
        // 2. Stable tag (plugins), so the readme points at the new tag.
        if let readme = detected.readmeFile {
            if product.type == .plugin, detected.readme?.stableTag != nil {
                try editor.replaceReadmeField(in: readme, field: "Stable tag", new: newVersion)
            }
            // 3. Changelog entry, only from notes the user typed.
            let notes = changelogNotes.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            if !notes.isEmpty {
                try editor.insertChangelog(in: readme, version: newVersion, notes: notes, date: today)
            } else if detected.readme?.changelogMentions(version: newVersion) != true {
                editor.notes.append("readme.txt has no changelog entry for \(newVersion). Add notes here, or edit the readme yourself.")
            }
        }
        // 4. Version constants in PHP files that still carry the current version.
        for file in phpFiles(in: folder) {
            try editor.replaceConstants(in: file, old: current, new: newVersion)
        }
        // 5. package.json.
        if FileManager.default.fileExists(atPath: folder.appendingPathComponent("package.json").path) {
            try editor.replacePackageJSON(old: current, new: newVersion)
        }

        let changes = editor.changes
        guard !changes.isEmpty else { throw VersionBumpError.nothingToChange }
        return VersionBumpPlan(currentVersion: current, newVersion: newVersion, changes: changes, notes: editor.notes, fileHashes: editor.originalHashes, contents: editor.contents)
    }

    /// Writes the planned contents. Refuses when a file changed since planning.
    public func apply(_ plan: VersionBumpPlan, folder: URL) throws {
        for file in plan.files {
            let url = folder.appendingPathComponent(file)
            let current = try Hashing.sha256(ofFile: url)
            guard current == plan.fileHashes[file] else { throw VersionBumpError.fileChanged(file) }
        }
        for file in plan.files {
            guard let text = plan.contents[file] else { continue }
            try Data(text.utf8).write(to: folder.appendingPathComponent(file), options: [.atomic])
        }
    }

    /// PHP files that can hold version constants; dependency and build folders are skipped.
    func phpFiles(in folder: URL) -> [String] {
        let skip: Set<String> = ["vendor", "node_modules", ".git", "tests", "build", "dist"]
        let base = folder.standardizedFileURL.path + "/"
        guard let enumerator = FileManager.default.enumerator(at: folder, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsHiddenFiles]) else { return [] }
        var files: [String] = []
        while let url = enumerator.nextObject() as? URL {
            if skip.contains(url.lastPathComponent) { enumerator.skipDescendants(); continue }
            guard url.pathExtension.lowercased() == "php" else { continue }
            let full = url.standardizedFileURL.path
            if full.hasPrefix(base) { files.append(String(full.dropFirst(base.count))) }
        }
        return files.sorted()
    }
}

/// Accumulates per-file text edits, keeping each file's line endings.
private struct Editor {
    let folder: URL
    var contents: [String: String] = [:]
    var originalHashes: [String: String] = [:]
    var changes: [VersionBumpChange] = []
    var notes: [String] = []

    init(folder: URL) { self.folder = folder }

    mutating func text(_ file: String) throws -> String {
        if let text = contents[file] { return text }
        let url = folder.appendingPathComponent(file)
        let data = try Data(contentsOf: url)
        originalHashes[file] = Hashing.sha256(data)
        let text = String(decoding: data, as: UTF8.self)
        contents[file] = text
        return text
    }

    /// Splits keeping line terminators so writing back preserves CRLF files.
    static func lines(_ text: String) -> [String] {
        var lines: [String] = []
        var current = ""
        for char in text {
            current.append(char)
            if char == "\n" || char == "\r\n" { lines.append(current); current = "" }
        }
        if !current.isEmpty { lines.append(current) }
        return lines
    }

    mutating func edit(_ file: String, description: String, limit: Int = .max, transform: (String) -> String?) throws {
        var lines = Self.lines(try text(file))
        var edited = 0
        for index in lines.indices where edited < limit {
            guard let replaced = transform(lines[index]), replaced != lines[index] else { continue }
            changes.append(VersionBumpChange(file: file, line: index + 1, description: description, oldText: lines[index].trimmingCharacters(in: .newlines), newText: replaced.trimmingCharacters(in: .newlines)))
            lines[index] = replaced
            edited += 1
        }
        contents[file] = lines.joined()
    }

    static func replacing(_ line: String, pattern: String, with template: String) -> String? {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]),
              regex.firstMatch(in: line, range: NSRange(line.startIndex..., in: line)) != nil
        else { return nil }
        return regex.stringByReplacingMatches(in: line, range: NSRange(line.startIndex..., in: line), withTemplate: template)
    }

    /// The first `Version:` header line in the 8 KiB WordPress reads.
    mutating func replaceHeader(in file: String, header: String, old: String, new: String) throws {
        let escapedOld = NSRegularExpression.escapedPattern(for: old)
        let pattern = #"^((?:[ \t]*<\?(?:php)?)?[ \t/*#@]*"# + header + #":[ \t]*)"# + escapedOld + #"(?![0-9A-Za-z.])"#
        try edit(file, description: "\(header) header", limit: 1) { line in
            Self.replacing(line, pattern: pattern, with: "$1" + NSRegularExpression.escapedTemplate(for: new))
        }
    }

    mutating func replaceReadmeField(in file: String, field: String, new: String) throws {
        let pattern = #"^([ \t*-]*"# + field + #"[ \t]*:[ \t]*)\S.*?([ \t]*)$"#
        try edit(file, description: field, limit: 1) { line in
            let body = line.trimmingCharacters(in: .newlines)
            let ending = String(line.dropFirst(body.count))
            return Self.replacing(body, pattern: pattern, with: "$1" + NSRegularExpression.escapedTemplate(for: new) + "$2").map { $0 + ending }
        }
    }

    /// `define( 'ACME_VERSION', '2.3.0' )` and `const VERSION = '2.3.0';`
    mutating func replaceConstants(in file: String, old: String, new: String) throws {
        let escapedOld = NSRegularExpression.escapedPattern(for: old)
        let quoted = #"(['"])"# + escapedOld + #"\2"#
        let definePattern = #"(define\s*\(\s*['"][A-Za-z0-9_\\]*VERSION['"]\s*,\s*)"# + quoted
        let constPattern = #"((?:(?:public|protected|private|final)\s+)*const\s+[A-Za-z0-9_]*VERSION\s*=\s*)"# + quoted
        let template = "$1$2" + NSRegularExpression.escapedTemplate(for: new) + "$2"
        try edit(file, description: "Version constant") { line in
            Self.replacing(line, pattern: definePattern, with: template) ?? Self.replacing(line, pattern: constPattern, with: template)
        }
    }

    /// The top-level `"version"` of package.json (its first occurrence).
    mutating func replacePackageJSON(old: String, new: String) throws {
        let pattern = #"^(\s*"version"\s*:\s*")"# + NSRegularExpression.escapedPattern(for: old) + #"(")"#
        try edit("package.json", description: "package.json version", limit: 1) { line in
            Self.replacing(line, pattern: pattern, with: "$1" + NSRegularExpression.escapedTemplate(for: new) + "$2")
        }
    }

    /// Inserts `= version - date =` with the notes as bullets right under `== Changelog ==`.
    mutating func insertChangelog(in file: String, version: String, notes entryNotes: [String], date: Date) throws {
        var lines = Self.lines(try text(file))
        guard let heading = lines.firstIndex(where: { line in
            let t = line.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return t.hasPrefix("==") && t.contains("changelog")
        }) else {
            notes.append("readme.txt has no == Changelog == section; add the \(version) entry yourself.")
            return
        }
        let newline = lines[heading].hasSuffix("\r\n") ? "\r\n" : "\n"
        let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
        let day = String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
        let bullets = entryNotes.map { $0.hasPrefix("*") || $0.hasPrefix("-") ? $0 : "* \($0)" }
        let block = ["= \(version) - \(day) ="] + bullets + [""]
        let next = heading + 1
        if next < lines.count, lines[next].trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            lines.insert(contentsOf: block.map { $0 + newline }, at: next + 1)
        } else {
            lines.insert(contentsOf: ([""] + block).map { $0 + newline }, at: next)
        }
        contents[file] = lines.joined()
        changes.append(VersionBumpChange(file: file, line: 0, description: "Changelog entry", oldText: "", newText: block.dropLast().joined(separator: "\n")))
    }
}
