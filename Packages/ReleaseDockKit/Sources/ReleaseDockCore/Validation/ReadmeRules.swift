import Foundation

/// readme.txt rules shared by plugins and themes, from the WordPress.org directory's
/// readme parser (`plugin-directory/readme/class-parser.php`) and the theme handbook.
/// All are warnings: the directory still imports the readme, but cuts what is too long.
enum ReadmeRules {
    static let shortDescriptionLimit = 150
    /// Words per section before the parser cuts it (`maximum_field_lengths`).
    static let sectionWordLimit = 2500
    static let longSectionWordLimit = ["changelog": 5000, "frequently asked questions": 5000, "faq": 5000]
    /// Theme handbook limit for an upgrade notice.
    static let themeUpgradeNoticeLimit = 300
    static let sizeLimit = 10 * 1024

    static func check(_ readme: WordPressReadme, file: String?, version: String?, type: ProductType = .plugin) -> [ValidationResult] {
        var results: [ValidationResult] = []
        if let short = readme.shortDescription {
            let plain = plainText(short)
            if plain.count > shortDescriptionLimit {
                results.append(.warn(.metadataValidity, "The short description is \(plain.count) characters; WordPress.org cuts it at \(shortDescriptionLimit)", file: file, fix: "Shorten the line under the readme headers to \(shortDescriptionLimit) characters."))
            }
        } else if let line = readme.unknownHeaderLines?.last {
            results.append(.warn(.metadataValidity, "WordPress.org reads \"\(line.prefix(60))\" as a header because it contains \":\", so the readme has no short description", file: file, fix: "Leave a blank line after the headers and avoid \":\" in the short description."))
        } else {
            results.append(.warn(.metadataValidity, "readme has no short description below the headers", file: file, fix: "Add one plain sentence (up to \(shortDescriptionLimit) characters) after the header block."))
        }
        if readme.sections["description"] == nil {
            results.append(.warn(.metadataValidity, "readme has no == Description == section", file: file))
        }
        for (title, body) in readme.sections.sorted(by: { $0.key < $1.key }) {
            let limit = longSectionWordLimit[title] ?? sectionWordLimit
            let words = body.split(whereSeparator: \.isWhitespace).count
            if words > limit {
                results.append(.warn(.metadataValidity, "The \(title.capitalized) section has \(words) words; WordPress.org cuts it at \(limit)", file: file, fix: title == "changelog" ? "Move older entries to changelog.txt." : "Shorten the section."))
            }
        }
        if type == .theme, let version, let notice = readme.upgradeNotice(for: version), plainText(notice).count > themeUpgradeNoticeLimit {
            results.append(.warn(.metadataValidity, "The upgrade notice for \(version) is \(plainText(notice).count) characters; the theme handbook allows \(themeUpgradeNoticeLimit)", file: file))
        }
        if let size = readme.byteCount, size > sizeLimit {
            results.append(.warn(.metadataValidity, "readme is \(size / 1024) KB; WordPress.org recommends under 10 KB", file: file, fix: "Keep the latest changelog entries in the readme and move older ones to changelog.txt."))
        }
        return results
    }

    /// Strips Markdown links/emphasis so lengths count what readers see.
    static func plainText(_ s: String) -> String {
        var text = s.replacingOccurrences(of: #"\[([^\]]*)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
        text = text.replacingOccurrences(of: #"[*_`]"#, with: "", options: .regularExpression)
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// `Requires Plugins`: comma-separated WordPress.org slugs (lowercase letters, digits
    /// and hyphens). Paths like `dir/file.php` are not supported by core.
    static func checkRequiresPlugins(_ value: String?, slug: String, file: String?) -> [ValidationResult] {
        guard let value else { return [] }
        var results: [ValidationResult] = []
        let slugs = value.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        for dependency in slugs {
            if dependency.range(of: #"^[a-z0-9]+(-[a-z0-9]+)*$"#, options: .regularExpression) == nil {
                results.append(.warn(.metadataValidity, "Requires Plugins entry \"\(dependency)\" is not a WordPress.org slug; WordPress ignores it", file: file, fix: "Use the plugin's directory slug, e.g. woocommerce."))
            } else if dependency == slug {
                results.append(.warn(.metadataValidity, "Requires Plugins lists the plugin itself", file: file))
            }
        }
        return results
    }
}
