import Foundation

/// Reads WordPress file headers the way WordPress core's `get_file_data()` does:
/// only the first 8 KiB of the file are read, `\r` is normalized to `\n`, and each
/// header matches `/^(?:[ \t]*<\?(?:php)?)?[ \t\/*#@]*{Name}:(.*)$/mi`. The captured
/// value is cleaned with `_cleanup_header_comment()` (strip `*/` / `?>` and trailing text, trim).
public enum WordPressHeaders {
    public static let readLimit = 8 * 1024

    public static let pluginHeaders = [
        "Plugin Name", "Plugin URI", "Version", "Description", "Author", "Author URI",
        "Text Domain", "Domain Path", "Network", "Requires at least", "Requires PHP",
        "Update URI", "Requires Plugins", "License", "License URI",
    ]

    public static let themeHeaders = [
        "Theme Name", "Theme URI", "Description", "Author", "Author URI", "Version",
        "Template", "Status", "Tags", "Text Domain", "Domain Path", "Requires at least",
        "Tested up to", "Requires PHP", "License", "License URI", "Update URI",
    ]

    /// Reads at most 8 KiB of `url` and parses the given headers. Missing headers are absent from the result.
    public static func read(_ url: URL, headers: [String]) throws -> [String: String] {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: readLimit) ?? Data()
        return parse(String(decoding: data, as: UTF8.self), headers: headers)
    }

    public static func parse(_ contents: String, headers: [String]) -> [String: String] {
        let text = contents.replacingOccurrences(of: "\r", with: "\n")
        let range = NSRange(text.startIndex..., in: text)
        var result: [String: String] = [:]
        for header in headers {
            let pattern = #"^(?:[ \t]*<\?(?:php)?)?[ \t/*#@]*"# + NSRegularExpression.escapedPattern(for: header) + #":(.*)$"#
            guard let regex = try? NSRegularExpression(pattern: pattern, options: [.anchorsMatchLines, .caseInsensitive]),
                  let match = regex.firstMatch(in: text, range: range),
                  let valueRange = Range(match.range(at: 1), in: text)
            else { continue }
            let raw = String(text[valueRange])
            guard !raw.isEmpty else { continue }
            let cleaned = cleanupHeaderComment(raw)
            if !cleaned.isEmpty { result[header] = cleaned }
        }
        return result
    }

    /// `_cleanup_header_comment()`: `trim( preg_replace( '/\s*(?:\*\/|\?>).*/', '', $str ) )`.
    static func cleanupHeaderComment(_ value: String) -> String {
        var s = value
        if let regex = try? NSRegularExpression(pattern: #"\s*(?:\*/|\?>).*"#) {
            s = regex.stringByReplacingMatches(in: s, range: NSRange(s.startIndex..., in: s), withTemplate: "")
        }
        return s.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
