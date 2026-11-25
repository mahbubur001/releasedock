import Foundation

/// Release notes of one app update, read from the HTML in its appcast `<description>`.
/// The publish script writes `<h3>Added</h3><ul><li>…</li></ul>` per Keep a Changelog
/// group; older entries are a single `<p>`, which becomes one untitled section.
public struct ReleaseNotes: Equatable, Sendable {
    public struct Section: Equatable, Sendable {
        /// Heading as written (Added, Changed, Fixed…); nil for text outside any heading.
        public var title: String?
        /// Items as Markdown: `**bold**`, `` `code` `` and links survive from the HTML.
        public var items: [String]

        public init(title: String?, items: [String]) {
            self.title = title
            self.items = items
        }
    }

    public var sections: [Section]

    public init(sections: [Section]) {
        self.sections = sections
    }

    public var isEmpty: Bool { sections.allSatisfy { $0.items.isEmpty } }

    public init(html: String) {
        var sections: [Section] = []
        let pattern = try! NSRegularExpression(pattern: #"<(h[1-6]|li|p)\b[^>]*>(.*?)</\1\s*>"#, options: [.caseInsensitive, .dotMatchesLineSeparators])
        let text = html as NSString
        for match in pattern.matches(in: html, range: NSRange(location: 0, length: text.length)) {
            let tag = text.substring(with: match.range(at: 1)).lowercased()
            let content = Self.markdown(fromInlineHTML: text.substring(with: match.range(at: 2)))
            guard !content.isEmpty else { continue }
            if tag.hasPrefix("h") {
                sections.append(Section(title: content, items: []))
            } else if sections.isEmpty {
                sections.append(Section(title: nil, items: [content]))
            } else {
                sections[sections.count - 1].items.append(content)
            }
        }
        if sections.isEmpty {
            let plain = Self.markdown(fromInlineHTML: html)
            if !plain.isEmpty { sections = [Section(title: nil, items: [plain])] }
        }
        self.sections = sections.filter { !$0.items.isEmpty }
    }

    /// Inline HTML to Markdown: keeps bold, code and links, drops other tags, decodes entities.
    static func markdown(fromInlineHTML html: String) -> String {
        var s = html
        let replacements: [(String, String)] = [
            (#"<(b|strong)\b[^>]*>(.*?)</\1\s*>"#, "**$2**"),
            (#"<code\b[^>]*>(.*?)</code\s*>"#, "`$1`"),
            (#"<a\b[^>]*href="([^"]*)"[^>]*>(.*?)</a\s*>"#, "[$2]($1)"),
            (#"<br\s*/?>"#, " "),
            (#"<[^>]+>"#, ""),
        ]
        for (pattern, template) in replacements {
            s = s.replacingOccurrences(of: pattern, with: template, options: [.regularExpression, .caseInsensitive])
        }
        for (entity, character) in [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'"), ("&#x27;", "'"), ("&nbsp;", " "), ("&amp;", "&")] {
            s = s.replacingOccurrences(of: entity, with: character)
        }
        return s.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
