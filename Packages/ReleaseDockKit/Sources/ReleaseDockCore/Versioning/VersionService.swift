import Foundation

public enum VersionValidity: Hashable, Sendable {
    case valid(isPrerelease: Bool)
    /// The string cannot be trusted as a release version and requires review.
    case invalid(reason: String)

    public var isValid: Bool {
        if case .valid = self { return true }
        return false
    }
}

public enum VersionComparison: Hashable, Sendable {
    /// Candidate is higher than the current release.
    case newer
    case same
    case older
    /// No current release is known (first release).
    case noCurrentRelease
    /// One of the versions is invalid; a human must review.
    case requiresReview(reason: String)

    public var allowsRelease: Bool {
        switch self {
        case .newer, .noCurrentRelease: true
        default: false
        }
    }
}

public protocol VersionServicing: Sendable {
    func validate(_ version: String) -> VersionValidity
    func compare(candidate: String, current: String?) -> VersionComparison
    func highest(of versions: [String]) -> String?
}

/// Version rules for WordPress releases. Ordering follows PHP `version_compare()`,
/// which is what WordPress uses to decide whether an update is available.
public struct VersionService: VersionServicing {
    public init() {}

    /// Words PHP's `version_compare()` orders as pre-release forms (case-sensitive,
    /// exactly as php-src matches them: "Beta" or "Rc" are *unknown* and sort below "dev").
    static let prereleaseWords: Set<String> = ["dev", "alpha", "a", "beta", "b", "RC", "rc"]
    /// Patch-level forms that PHP orders *above* a plain number.
    static let patchLevelWords: Set<String> = ["pl", "p"]

    public func validate(_ version: String) -> VersionValidity {
        if version.isEmpty { return .invalid(reason: "Version is empty") }
        if version != version.trimmingCharacters(in: .whitespacesAndNewlines) {
            return .invalid(reason: "Version has surrounding whitespace")
        }
        if version.count > 64 { return .invalid(reason: "Version is longer than 64 characters") }
        // Must start with a digit: PHP orders "v1.2" *below* "1.0", which would make a
        // leading "v" silently sort as a downgrade.
        guard let first = version.unicodeScalars.first, ("0"..."9").contains(first) else {
            return .invalid(reason: "Version must start with a digit (found \"\(version)\")")
        }
        let allowed = CharacterSet(charactersIn: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-+_")
        if version.unicodeScalars.contains(where: { !allowed.contains($0) }) {
            return .invalid(reason: "Version contains characters other than letters, digits, '.', '-', '+' or '_'")
        }
        if version.contains("..") || version.hasSuffix(".") {
            return .invalid(reason: "Version has an empty component")
        }
        let parts = PHPVersion.canonicalize(version).split(separator: ".").map(String.init)
        let words = parts.filter { $0.first.map { !$0.isNumber } ?? false }
        // php-src matches special forms by *prefix*, so "pre" sorts as "p" (a patch level,
        // newer than the plain release) and any unknown word sorts below "dev". Only exact
        // known forms are accepted; everything else needs human review.
        for word in words where !Self.prereleaseWords.contains(word) && !Self.patchLevelWords.contains(word) {
            return .invalid(reason: "Version identifier \"\(word)\" has no reliable order in PHP version_compare()")
        }
        return .valid(isPrerelease: words.contains { Self.prereleaseWords.contains($0) })
    }

    public func compare(candidate: String, current: String?) -> VersionComparison {
        if case .invalid(let reason) = validate(candidate) {
            return .requiresReview(reason: "Candidate version: \(reason)")
        }
        guard let current, !current.isEmpty else { return .noCurrentRelease }
        if current.lowercased() == "trunk" { return .noCurrentRelease }
        if case .invalid(let reason) = validate(current) {
            return .requiresReview(reason: "Current version \"\(current)\": \(reason)")
        }
        if candidate == current { return .same }
        switch PHPVersion.compare(candidate, current) {
        case 1: return .newer
        case -1: return .older
        default:
            // Different strings PHP considers equal (e.g. "1.0-beta" vs "1.0beta").
            return .same
        }
    }

    public func highest(of versions: [String]) -> String? {
        versions
            .filter { validate($0).isValid }
            .max { PHPVersion.compare($0, $1) < 0 }
    }
}
