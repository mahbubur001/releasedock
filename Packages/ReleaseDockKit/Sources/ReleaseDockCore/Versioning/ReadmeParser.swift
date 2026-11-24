import Foundation

/// Parsed WordPress `readme.txt`.
public struct WordPressReadme: Codable, Hashable, Sendable {
    public var name: String?
    /// Header fields keyed by their lowercased field name (e.g. `stable tag`).
    public var fields: [String: String]
    /// Section bodies keyed by lowercased section title (e.g. `changelog`).
    public var sections: [String: String]
    /// Text between the header block and the first section, whitespace-collapsed.
    public var shortDescription: String?
    /// Size of the file in bytes.
    public var byteCount: Int?
    /// Lines in the header block with an unknown key. A short description containing
    /// ":" lands here, because WordPress.org reads it as a header too.
    public var unknownHeaderLines: [String]?

    public init(name: String?, fields: [String: String], sections: [String: String], shortDescription: String? = nil, byteCount: Int? = nil, unknownHeaderLines: [String]? = nil) {
        self.name = name
        self.fields = fields
        self.sections = sections
        self.shortDescription = shortDescription
        self.byteCount = byteCount
        self.unknownHeaderLines = unknownHeaderLines
    }

    /// Stable tag as WordPress.org sanitizes it.
    public var stableTag: String? { fields["stable tag"].map(ReadmeParser.sanitizeStableTag) }
    public var rawStableTag: String? { fields["stable tag"] }
    public var testedUpTo: String? { fields["tested up to"] }
    public var requiresAtLeast: String? { fields["requires at least"] }
    public var requiresPHP: String? { fields["requires php"] }
    public var contributors: [String] { list(fields["contributors"]) }
    public var tags: [String] { list(fields["tags"]) }
    public var license: String? { fields["license"] }

    private func list(_ value: String?) -> [String] {
        (value ?? "").split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    }

    /// Whether the changelog section has an entry heading that mentions `version`
    /// (`= 1.2.3 =`, `= 1.2.3 - 2026-01-01 =`, `### 1.2.3`, `**1.2.3**` …).
    public func changelogMentions(version: String) -> Bool {
        guard let changelog = sections["changelog"] else { return false }
        let escaped = NSRegularExpression.escapedPattern(for: version)
        // The version must not be immediately followed by another version character,
        // so `1.2` does not match a `1.2.3` heading.
        let pattern = #"(?m)^[ \t]*(?:=+|#+|\*+)?[ \t]*v?"# + escaped + #"(?![0-9A-Za-z.\-+])"#
        return changelog.range(of: pattern, options: .regularExpression) != nil
    }

    /// The `== Upgrade Notice ==` entry for `version`.
    public func upgradeNotice(for version: String) -> String? { entry(in: "upgrade notice", version: version) }

    /// The `== Changelog ==` entry for `version`.
    public func changelogEntry(for version: String) -> String? { entry(in: "changelog", version: version) }

    /// The text under the `= version =` (or `= version - date =`, `### version`) heading of
    /// a section, up to the next heading.
    public func entry(in sectionTitle: String, version: String) -> String? {
        guard let section = sections[sectionTitle] else { return nil }
        var collecting = false
        var lines: [String] = []
        for line in section.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            let isHeading = (trimmed.hasPrefix("=") && trimmed.hasSuffix("=") && trimmed.count > 2) || trimmed.hasPrefix("#")
            if isHeading {
                if collecting { break }
                var title = trimmed.trimmingCharacters(in: CharacterSet(charactersIn: "=#*[] \t"))
                if title.hasPrefix("v") { title.removeFirst() }
                // "1.2.3 - 2026-11-24", "1.2.3 (2026-11-24)": the version is the first word.
                let first = title.split(whereSeparator: { $0 == " " || $0 == "\t" || $0 == "(" }).first.map(String.init) ?? title
                collecting = first == version
                continue
            }
            if collecting { lines.append(line) }
        }
        let text = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }
}

/// Parser for WordPress readme files following the WordPress.org plugin directory
/// parser (`plugin-directory/readme/class-parser.php`) where it matters for releases:
///
/// - The name is the first non-blank line trimmed of `#`, `=` and whitespace.
/// - Header lines follow; keys are case-insensitive and split on the first `:`, with
///   ` \t*-` trimmed from keys and values. Parsing stops at the first non-blank line that
///   has no `:` or starts with `#`/`=`, and at an unknown key that follows a blank line
///   (that line starts the short description). Aliases: `tested` → `tested up to`,
///   `requires` → `requires at least`.
/// - Sections start with `== Title ==` or a Markdown `## Title`.
///
/// It does not reproduce every rule of the official readme validator; ReleaseDock checks
/// the release-relevant fields and points users to the validator for the rest.
public enum ReadmeParser {
    static let headerAliases = ["tested": "tested up to", "requires": "requires at least"]
    static let knownHeaders: Set<String> = [
        "tested up to", "requires at least", "requires php", "tags", "contributors",
        "donate link", "stable tag", "license", "license uri",
    ]

    public static func parse(_ contents: String) -> WordPressReadme {
        let text = contents
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .replacingOccurrences(of: "\u{FEFF}", with: "")
        let lines = text.components(separatedBy: "\n")
        var index = 0

        // Name line.
        var name: String?
        while index < lines.count {
            let line = lines[index]
            index += 1
            if line.trimmingCharacters(in: .whitespaces).isEmpty { continue }
            let trimmed = line.trimmingCharacters(in: CharacterSet(charactersIn: "#= \t\0\u{0B}"))
            name = trimmed.isEmpty ? nil : trimmed
            break
        }

        // Header block.
        var fields: [String: String] = [:]
        var unknownHeaderLines: [String] = []
        var lastLineWasBlank = false
        let keyTrim = CharacterSet(charactersIn: " \t*-\r\n")
        let valueTrim = CharacterSet(charactersIn: " \t*-\r\n<>")
        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard let colon = line.firstIndex(of: ":"), !line.hasPrefix("#"), !line.hasPrefix("=") else {
                if trimmed.isEmpty { lastLineWasBlank = true; index += 1; continue }
                break
            }
            var key = line[..<colon].trimmingCharacters(in: keyTrim).lowercased()
            key = headerAliases[key] ?? key
            let value = line[line.index(after: colon)...].trimmingCharacters(in: valueTrim)
            if knownHeaders.contains(key) {
                if fields[key] == nil { fields[key] = value }
            } else if lastLineWasBlank {
                break // The short description, as on WordPress.org.
            } else {
                unknownHeaderLines.append(trimmed)
            }
            lastLineWasBlank = false
            index += 1
        }

        // Sections. Text before the first section is the short description.
        var sections: [String: String] = [:]
        var currentSection: String?
        var body: [String] = []
        var intro: [String] = []
        func flush() {
            if let currentSection {
                if sections[currentSection] == nil {
                    sections[currentSection] = body.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
                }
            } else {
                intro = body
            }
            body = []
        }
        while index < lines.count {
            let line = lines[index]
            index += 1
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if let title = sectionTitle(trimmed) {
                flush()
                currentSection = title.lowercased()
                continue
            }
            body.append(line)
        }
        flush()
        let short = intro.joined(separator: " ")
            .components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
        return WordPressReadme(name: name, fields: fields, sections: sections, shortDescription: short.isEmpty ? nil : short, byteCount: contents.utf8.count, unknownHeaderLines: unknownHeaderLines.isEmpty ? nil : unknownHeaderLines)
    }

    public static func parse(fileAt url: URL) throws -> WordPressReadme {
        let data = try Data(contentsOf: url)
        return parse(String(decoding: data, as: UTF8.self))
    }

    /// `sanitize_stable_tag()`: trim quotes, strip a leading `tags/` or `/tags/`, drop
    /// characters outside `[A-Za-z0-9_.-]`, and prefix `0` to a leading `.`.
    public static func sanitizeStableTag(_ raw: String) -> String {
        var tag = raw.trimmingCharacters(in: CharacterSet(charactersIn: "\"' \t"))
        for prefix in ["/tags/", "tags/"] where tag.hasPrefix(prefix) {
            tag = String(tag.dropFirst(prefix.count))
            break
        }
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-")
        tag = String(String.UnicodeScalarView(tag.unicodeScalars.filter { allowed.contains($0) }))
        if tag.hasPrefix(".") { tag = "0" + tag }
        return tag
    }

    private static func sectionTitle(_ line: String) -> String? {
        if line.hasPrefix("=="), !line.hasPrefix("==="), line.hasSuffix("=="), line.count > 4 {
            let title = line.trimmingCharacters(in: CharacterSet(charactersIn: "= \t"))
            return title.isEmpty ? nil : title
        }
        if line.hasPrefix("## "), !line.hasPrefix("### ") {
            let title = line.trimmingCharacters(in: CharacterSet(charactersIn: "#= \t"))
            return title.isEmpty ? nil : title
        }
        return nil
    }
}
