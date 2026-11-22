import Foundation

/// Limits applied while inspecting untrusted packages.
public struct InspectionOptions: Sendable {
    public var excludePatterns: [String]
    /// Maximum total uncompressed size of a package.
    public var maximumTotalSize: UInt64
    /// Maximum size of a single file.
    public var maximumFileSize: UInt64
    /// Compression ratio above which a large entry is treated as a possible zip bomb.
    public var maximumCompressionRatio: Double
    public var maximumEntries: Int

    public init(
        excludePatterns: [String] = [],
        maximumTotalSize: UInt64 = 512 << 20,
        maximumFileSize: UInt64 = 256 << 20,
        maximumCompressionRatio: Double = 200,
        maximumEntries: Int = 50_000
    ) {
        self.excludePatterns = excludePatterns
        self.maximumTotalSize = maximumTotalSize
        self.maximumFileSize = maximumFileSize
        self.maximumCompressionRatio = maximumCompressionRatio
        self.maximumEntries = maximumEntries
    }
}

/// Applies `FileClassifier` to package-relative paths, propagating directory
/// exclusions to descendants and reporting each excluded directory only once.
struct PackagePathEvaluator {
    let classifier: FileClassifier
    private(set) var issues: [InspectionIssue] = []
    private(set) var excludedPaths: [String] = []
    private var excludedDirectories: Set<String> = []
    private var evaluatedDirectories: Set<String> = []

    init(classifier: FileClassifier) {
        self.classifier = classifier
    }

    /// Returns true when the path is included in the release candidate.
    mutating func include(path: String, isDirectory: Bool) -> Bool {
        let components = path.split(separator: "/").map(String.init)
        // Ancestors first: an excluded ancestor excludes this path silently.
        var prefix = ""
        for component in components.dropLast() {
            prefix = prefix.isEmpty ? component : prefix + "/" + component
            if excludedDirectories.contains(prefix) { return false }
            if !evaluatedDirectories.contains(prefix) {
                evaluatedDirectories.insert(prefix)
                if !evaluate(path: prefix, isDirectory: true) { return false }
            }
        }
        if isDirectory {
            if excludedDirectories.contains(path) { return false }
            if evaluatedDirectories.contains(path) { return true }
            evaluatedDirectories.insert(path)
        }
        return evaluate(path: path, isDirectory: isDirectory)
    }

    private mutating func evaluate(path: String, isDirectory: Bool) -> Bool {
        guard let classification = classifier.classify(path: path, isDirectory: isDirectory) else { return true }
        let displayPath = isDirectory ? path + "/" : path
        issues.append(InspectionIssue(
            kind: classification.kind,
            severity: classification.severity,
            path: displayPath,
            message: classification.message + (classification.exclude ? " — excluded from the release" : ""),
            excluded: classification.exclude
        ))
        if classification.exclude {
            excludedPaths.append(displayPath)
            if isDirectory { excludedDirectories.insert(path) }
            return false
        }
        return true
    }

    mutating func addIssue(_ issue: InspectionIssue) {
        issues.append(issue)
    }
}

extension PathSafety.Violation {
    var issueKind: InspectionIssueKind {
        switch self {
        case .parentTraversal, .escapesRoot: .pathTraversal
        case .absolutePath: .absolutePath
        default: .unsafeFilename
        }
    }
}
