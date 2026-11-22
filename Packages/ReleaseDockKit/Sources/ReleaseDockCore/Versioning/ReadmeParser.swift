import Foundation

/// Parsed WordPress `readme.txt`.
public struct WordPressReadme: Codable, Hashable, Sendable {
    public var name: String?
    /// Header fields keyed by their lowercased field name (e.g. `stable tag`).
    public var fields: [String: String]
    /// Section bodies keyed by lowercased section title (e.g. `changelog`).
    public var sections: [String: String]

    public init(name: String?, fields: [String: String], sections: [String: String]) {
        self.name = name
        self.fields = fields
        self.sections = sections
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
}

/// Parser for WordPress readme files following the WordPress.org plugin directory
/// parser (`plugin-directory/readme/class-parser.php`) where it matters for releases:
///
/// - The name is the first non-blank line trimmed of `#`, `=` and whitespace.
/// - Header lines follow; keys are case-insensitive and split on the first `:`.
///   Parsing stops at the first non-blank line that has no `:` or starts with `#`/`=`.
///   Aliases: `tested` → `tested up to`, `requires` → `requires at least`.
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
        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty { index += 1; continue }
            guard let colon = trimmed.firstIndex(of: ":"), !trimmed.hasPrefix("#"), !trimmed.hasPrefix("=") else { break }
            var key = trimmed[..<colon].trimmingCharacters(in: .whitespaces).lowercased()
            key = headerAliases[key] ?? key
            let value = trimmed[trimmed.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            if knownHeaders.contains(key), fields[key] == nil { fields[key] = value }
            index += 1
        }

        // Sections.
        var sections: [String: String] = [:]
        var currentSection: String?
        var body: [String] = []
        func flush() {
            if let currentSection, sections[currentSection] == nil {
                sections[currentSection] = body.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
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
        return WordPressReadme(name: name, fields: fields, sections: sections)
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
