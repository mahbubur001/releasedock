import Darwin
import Foundation

/// gitignore-style glob matching used for exclusion patterns and `.distignore`.
///
/// - A pattern without `/` matches any path component (`*.log`, `node_modules`).
/// - A pattern containing `/` is anchored to the package root (`/tests`, `src/*.ts`).
/// - A trailing `/` restricts the pattern to directories.
/// - Matching a directory excludes everything beneath it.
/// - `#` comments, blank lines and `!` negations are ignored (negation is not supported
///   and is reported by the caller if needed).
public struct GlobPattern: Hashable, Sendable {
    public let raw: String
    let pattern: String
    let anchored: Bool
    let directoryOnly: Bool

    public init?(_ raw: String) {
        var p = raw.trimmingCharacters(in: .whitespaces)
        guard !p.isEmpty, !p.hasPrefix("#"), !p.hasPrefix("!") else { return nil }
        self.raw = raw
        directoryOnly = p.hasSuffix("/")
        if directoryOnly { p.removeLast() }
        if p.hasPrefix("/") { p.removeFirst() }
        guard !p.isEmpty else { return nil }
        anchored = raw.trimmingCharacters(in: .whitespaces).dropLast(directoryOnly ? 1 : 0).contains("/")
        pattern = p
    }

    /// Whether `path` (relative, `/`-separated) is matched directly — not via an ancestor.
    public func matches(path: String, isDirectory: Bool) -> Bool {
        if directoryOnly && !isDirectory { return false }
        if anchored {
            return Self.fnmatch(pattern, path, pathname: true)
        }
        let name = path.split(separator: "/").last.map(String.init) ?? path
        return Self.fnmatch(pattern, name, pathname: false)
    }

    static func fnmatch(_ pattern: String, _ string: String, pathname: Bool) -> Bool {
        // `**/` is approximated by trying the pattern with and without the prefix.
        if pattern.hasPrefix("**/") {
            let rest = String(pattern.dropFirst(3))
            if fnmatch(rest, string, pathname: pathname) { return true }
            let components = string.split(separator: "/")
            for i in 1..<max(components.count, 1) {
                if fnmatch(rest, components[i...].joined(separator: "/"), pathname: pathname) { return true }
            }
            return false
        }
        let flags: Int32 = pathname ? FNM_PATHNAME : 0
        return Darwin.fnmatch(pattern, string, flags) == 0
    }

    public static func parseList(_ text: String) -> [GlobPattern] {
        text.components(separatedBy: .newlines).compactMap(GlobPattern.init)
    }
}

public struct FileClassification: Hashable, Sendable {
    public var kind: InspectionIssueKind
    public var severity: ValidationSeverity
    public var message: String
    /// Excluded from the release candidate automatically.
    public var exclude: Bool
}

/// Classifies package paths into junk, VCS metadata, development artifacts,
/// forbidden files and possible secrets.
public struct FileClassifier: Sendable {
    public var userPatterns: [GlobPattern]
    public var sourceKind: SourceKind

    public init(sourceKind: SourceKind, userPatterns: [GlobPattern] = []) {
        self.sourceKind = sourceKind
        self.userPatterns = userPatterns
    }

    static let junkNames: Set<String> = [".DS_Store", "__MACOSX", "Thumbs.db", "ehthumbs.db", "desktop.ini", ".Spotlight-V100", ".Trashes", ".fseventsd", ".TemporaryItems", "Icon\r"]
    static let vcsNames: Set<String> = [".git", ".svn", ".hg", ".bzr", "CVS", ".gitignore", ".gitattributes", ".gitmodules", ".gitkeep", ".svnignore", ".hgignore"]
    static let devNames: Set<String> = [
        "node_modules", "bower_components", ".github", ".gitlab", ".gitlab-ci.yml", ".circleci", ".travis.yml",
        ".wordpress-org", ".idea", ".vscode", ".fleet", ".editorconfig", ".distignore", ".husky",
        ".phpcs.xml", ".phpcs.xml.dist", "phpcs.xml", "phpcs.xml.dist", "phpunit.xml", "phpunit.xml.dist",
        ".phpunit.result.cache", ".phpunit.cache", "phpstan.neon", "phpstan.neon.dist", "psalm.xml",
        ".php-cs-fixer.php", ".php-cs-fixer.dist.php", ".php-cs-fixer.cache", ".php_cs", ".php_cs.cache",
        ".eslintignore", ".prettierignore", ".stylelintignore", ".browserslistrc", ".babelrc", ".nvmrc", ".npmrc",
        "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "Gruntfile.js", "gulpfile.js", "webpack.config.js",
        ".wp-env.json", ".wp-env.override.json", "docker-compose.yml", "docker-compose.yaml", "Dockerfile", ".dockerignore",
        ".sass-cache", ".phpstorm.meta.php", ".env.example", ".env.dist", ".yarn", ".pnp.cjs", ".pnp.loader.mjs",
    ]
    static let devPrefixes = [".eslintrc", ".prettierrc", ".stylelintrc", ".yarnrc"]
    static let devSuffixes = [".log", ".code-workspace", ".swp", ".swo", "~", ".orig", ".rej"]
    static let archiveSuffixes = [".zip", ".tar", ".tar.gz", ".tgz", ".gz", ".bz2", ".xz", ".rar", ".7z"]
    static let binarySuffixes = [".exe", ".dll", ".dmg", ".pkg", ".msi", ".so", ".dylib", ".jar"]
    static let secretNames: Set<String> = [".env", ".htpasswd", "auth.json", "wp-config.php", "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".npmrc.local"]
    static let secretSuffixes = [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".asc"]

    public func classify(path: String, isDirectory: Bool) -> FileClassification? {
        let name = path.split(separator: "/").last.map(String.init) ?? path
        let lower = name.lowercased()

        if Self.junkNames.contains(name) || name.hasPrefix("._") {
            return .init(kind: .junkFile, severity: .info, message: "Operating system metadata", exclude: true)
        }
        if Self.vcsNames.contains(name) {
            return .init(kind: .versionControlMetadata, severity: .warning, message: "Version control metadata", exclude: true)
        }
        if Self.secretNames.contains(lower) || (lower.hasPrefix(".env.") && !Self.devNames.contains(name))
            || Self.secretSuffixes.contains(where: { lower.hasSuffix($0) }) {
            // In a built ZIP a secret-looking file is a red flag and blocks the release; in a
            // development folder it is expected and simply excluded.
            return .init(
                kind: .possibleSecret,
                severity: sourceKind == .zip ? .error : .warning,
                message: "Possible secret or private key; it must never be published",
                exclude: sourceKind == .folder
            )
        }
        if Self.devNames.contains(name) || Self.devPrefixes.contains(where: { name.hasPrefix($0) })
            || (!isDirectory && Self.devSuffixes.contains(where: { lower.hasSuffix($0) })) {
            return .init(kind: .developmentArtifact, severity: .warning, message: "Development artifact", exclude: true)
        }
        for pattern in userPatterns where pattern.matches(path: path, isDirectory: isDirectory) {
            return .init(kind: .developmentArtifact, severity: .info, message: "Excluded by pattern \"\(pattern.raw)\"", exclude: true)
        }
        if !isDirectory, Self.archiveSuffixes.contains(where: { lower.hasSuffix($0) }) {
            return .init(kind: .forbiddenFile, severity: .error, message: "Archive files must not be committed to WordPress.org SVN", exclude: false)
        }
        if !isDirectory, Self.binarySuffixes.contains(where: { lower.hasSuffix($0) }) {
            return .init(kind: .forbiddenFile, severity: .error, message: "Executable or compiled binary files are not allowed in a release", exclude: false)
        }
        if !isDirectory, lower.hasSuffix(".phar") {
            return .init(kind: .forbiddenFile, severity: .warning, message: "PHAR archives are usually rejected by WordPress.org review", exclude: false)
        }
        return nil
    }
}
