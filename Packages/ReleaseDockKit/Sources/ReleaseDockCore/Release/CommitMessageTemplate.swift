import Foundation

/// The summary line of a release commit, written with tokens the pipeline fills in once
/// the version is known: `{name}`, `{slug}`, `{version}`, `{previous}`. The commit
/// marker that recovery searches for is always appended after it.
public enum CommitMessageTemplate {
    public static let tokens = ["{name}", "{slug}", "{version}", "{previous}"]
    public static let maximumLength = 300

    /// Suggestions in the Templates menu.
    public static let presets = [
        "Release {name} {version}",
        "Release {version}",
        "Tagging version {version}",
        "Update to {version}",
    ]

    public static let defaultVersion = "Release {name} {version}"
    public static let defaultReadmeUpdate = "Update readme for {name} {version}"

    /// Why the template can't be used, or nil.
    public static func problem(_ template: String) -> String? {
        let trimmed = template.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "Enter a commit message." }
        if trimmed.count > maximumLength { return "Keep the commit message under \(maximumLength) characters." }
        let unknown = trimmed.matches(of: /\{[a-z]+\}/).map { String($0.output) }.filter { !tokens.contains($0) }
        if let first = unknown.first { return "\(first) is not a token. Use \(tokens.joined(separator: ", "))." }
        return nil
    }

    public static func render(_ template: String, name: String, slug: String, version: String?, previous: String?) -> String {
        template
            .replacingOccurrences(of: "{name}", with: name)
            .replacingOccurrences(of: "{slug}", with: slug)
            .replacingOccurrences(of: "{version}", with: version ?? "")
            .replacingOccurrences(of: "{previous}", with: previous ?? "")
            .replacingOccurrences(of: #"[ \t]{2,}"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
