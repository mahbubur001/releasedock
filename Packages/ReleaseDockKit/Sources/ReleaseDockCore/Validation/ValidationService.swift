import Foundation

/// Everything validation looks at. Gathering (filesystem, SVN, Git) happens before;
/// validation itself is a pure function of this context, which keeps it testable.
public struct ValidationContext: Sendable {
    public var product: Product
    public var inspection: PackageInspection
    public var detected: DetectedProduct?
    public var detectionError: String?
    public var repository: RepositorySnapshot?
    public var git: GitRepositoryState?
    /// Present once the release has been staged and previewed.
    public var preview: ReleasePreview?
    public var kind: ReleaseKind
    /// Present once the staged candidate has been linted / checked.
    public var codeChecks: CodeCheckReport?

    public init(product: Product, inspection: PackageInspection, detected: DetectedProduct?, detectionError: String? = nil, repository: RepositorySnapshot?, git: GitRepositoryState? = nil, preview: ReleasePreview? = nil, kind: ReleaseKind = .version, codeChecks: CodeCheckReport? = nil) {
        self.product = product
        self.inspection = inspection
        self.detected = detected
        self.detectionError = detectionError
        self.repository = repository
        self.git = git
        self.preview = preview
        self.kind = kind
        self.codeChecks = codeChecks
    }
}

public protocol ValidationServicing: Sendable {
    func validate(_ context: ValidationContext) -> ValidationReport
}

public struct ValidationService: ValidationServicing {
    private let versions: VersionServicing
    private let strategies: StrategyProvider

    public init(versions: VersionServicing = VersionService(), strategies: StrategyProvider = StrategyProvider()) {
        self.versions = versions
        self.strategies = strategies
    }

    public func validate(_ context: ValidationContext) -> ValidationReport {
        let strategy = strategies.strategy(for: context.product.type)
        var results: [ValidationResult] = []
        results += sourceIntegrity(context)
        results += packageStructure(context)
        results += forbiddenFiles(context, strategy: strategy)
        results += identity(context)
        results += version(context, strategy: strategy)
        if let detected = context.detected {
            results += strategy.validateMetadata(detected: detected, product: context.product)
            results += strategy.validateRequiredFiles(inspection: context.inspection, detected: detected)
        } else {
            results.append(.fail(.metadataValidity, "Product metadata could not be read", fix: context.detectionError))
            results.append(.skip(.requiredFiles, "Skipped because the product was not detected"))
        }
        results += repository(context)
        results += releaseDiff(context, strategy: strategy)
        results += optional(context)
        let freshness = testedUpToFreshness(context)
        if !freshness.isEmpty {
            // The freshness result supersedes the strategy's format-only pass.
            results.removeAll { $0.check == .testedUpTo && $0.status == .passed }
        }
        results += freshness
        results += codeChecks(context)

        let disabled = context.product.validation.disabledChecks
        let filtered = results.filter { $0.check.isRequired || !disabled.contains($0.check) }
        return ValidationReport(results: filtered, blockOnWarnings: context.product.validation.blockOnWarnings)
    }

    // MARK: Source

    func sourceIntegrity(_ c: ValidationContext) -> [ValidationResult] {
        let corrupt = c.inspection.issues.filter { $0.kind == .corruptArchive }
        if !corrupt.isEmpty {
            return corrupt.map { .fail(.sourceIntegrity, $0.message, fix: "Rebuild the ZIP package and try again.") }
        }
        let label = c.inspection.sourceKind == .zip ? "ZIP SHA-256" : "Folder digest"
        if c.inspection.includedFiles.isEmpty {
            return [.fail(.sourceIntegrity, "The package contains no publishable files")]
        }
        return [.pass(.sourceIntegrity, "\(label) \(c.inspection.sha256.prefix(16))…, \(c.inspection.includedFiles.count) files")]
    }

    static let structuralKinds: Set<InspectionIssueKind> = [
        .pathTraversal, .absolutePath, .unsafeFilename, .symbolicLink, .duplicatePath, .nestedRoot,
        .encryptedEntry, .unsupportedCompression, .archiveTooLarge, .suspiciousCompressionRatio, .specialFile, .multipleRoots,
    ]

    func packageStructure(_ c: ValidationContext) -> [ValidationResult] {
        let issues = c.inspection.issues.filter { Self.structuralKinds.contains($0.kind) }
        if issues.isEmpty { return [.pass(.packageStructure, "Package structure is safe")] }
        return issues.map { issue in
            issue.severity == .error
                ? .fail(.packageStructure, issue.message, file: issue.path, fix: structureFix(issue.kind))
                : .warn(.packageStructure, issue.message, file: issue.path)
        }
    }

    private func structureFix(_ kind: InspectionIssueKind) -> String {
        switch kind {
        case .symbolicLink: "Replace the link with the real file, or exclude it."
        case .nestedRoot: "Rebuild the ZIP so the plugin or theme files sit directly inside one top-level folder."
        case .duplicatePath: "Remove the duplicate or case-conflicting file."
        case .pathTraversal, .absolutePath: "The archive is unsafe. Rebuild it with a standard ZIP tool."
        default: "Rebuild the package without this entry."
        }
    }

    func forbiddenFiles(_ c: ValidationContext, strategy: ReleaseStrategy) -> [ValidationResult] {
        var results: [ValidationResult] = []
        for issue in c.inspection.issues where issue.kind == .forbiddenFile || issue.kind == .possibleSecret {
            guard !issue.excluded else {
                results.append(.warn(.forbiddenFiles, issue.message, file: issue.path))
                continue
            }
            results.append(issue.severity == .error
                ? .fail(.forbiddenFiles, issue.message, file: issue.path, fix: "Remove the file from the package or add an exclusion pattern.")
                : .warn(.forbiddenFiles, issue.message, file: issue.path))
        }
        results += strategy.validateForbiddenFiles(inspection: c.inspection)
        if results.isEmpty {
            let excluded = c.inspection.issues.filter { $0.excluded }
            let devCount = excluded.filter { $0.kind == .developmentArtifact || $0.kind == .versionControlMetadata }.count
            var message = "No forbidden files in the release candidate"
            if !excluded.isEmpty {
                message += "; \(excluded.count) path(s) excluded" + (devCount > 0 ? " (\(devCount) development or VCS artifacts)" : "")
            }
            results.append(.pass(.forbiddenFiles, message))
        }
        return results
    }

    // MARK: Identity

    static func normalizeName(_ s: String) -> String {
        s.lowercased()
            .replacingOccurrences(of: "&amp;", with: "&")
            .components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
    }

    func identity(_ c: ValidationContext) -> [ValidationResult] {
        guard let detected = c.detected else {
            return [.fail(.productIdentity, "No \(c.product.type.rawValue) was detected in the package", fix: c.detectionError)]
        }
        var results: [ValidationResult] = []
        if detected.type != c.product.type {
            results.append(.fail(.productIdentity, "Package is a \(detected.type.rawValue), but \(c.product.name) is registered as a \(c.product.type.rawValue)"))
        }
        if let name = detected.name {
            if Self.normalizeName(name) == Self.normalizeName(c.product.expectedHeaderName) {
                results.append(.pass(.productIdentity, "\(detected.type == .plugin ? "Plugin" : "Theme") Name \"\(name)\" matches"))
            } else {
                results.append(.fail(
                    .productIdentity,
                    "Package declares \"\(name)\", expected \"\(c.product.expectedHeaderName)\"",
                    file: detected.mainFile,
                    fix: "Make sure this package belongs to \(c.product.name). If the product was renamed, update its expected header name."
                ))
            }
        }
        if let domain = detected.textDomain, domain != c.product.slug {
            results.append(.warn(.productIdentity, "Text Domain \"\(domain)\" differs from the slug \"\(c.product.slug)\"; WordPress.org translations use the slug", file: detected.mainFile))
        }
        if let root = c.inspection.archiveRootName, root != c.product.slug {
            results.append(.warn(.productIdentity, "ZIP top-level folder \"\(root)\" differs from the slug \"\(c.product.slug)\""))
        }
        return results
    }

    // MARK: Version

    func version(_ c: ValidationContext, strategy: ReleaseStrategy) -> [ValidationResult] {
        guard let version = c.detected?.version else {
            return [
                .fail(.versionValidity, "No Version header found", file: c.detected?.mainFile, fix: "Add a \"Version:\" header."),
                .skip(.versionIncrease, "No version to compare"),
                .skip(.existingTag, "No version to check"),
            ]
        }
        var results: [ValidationResult] = []
        switch versions.validate(version) {
        case .invalid(let reason):
            results.append(.fail(.versionValidity, "Version \"\(version)\" requires review: \(reason)", file: c.detected?.mainFile))
        case .valid(let isPrerelease):
            results.append(.pass(.versionValidity, "Version \(version)"))
            if isPrerelease {
                results.append(.warn(.versionValidity, "\(version) is a pre-release; publishing it makes it the stable version for every site", fix: "Publish pre-releases outside WordPress.org, or confirm this is intended."))
            }
        }
        results += strategy.validateVersionFormat(version)
        if c.kind == .readmeUpdate { return results + releasedVersion(c, version: version, strategy: strategy) }

        if let repo = c.repository, repo.connectivityError == nil {
            switch versions.compare(candidate: version, current: repo.currentVersion) {
            case .newer:
                results.append(.pass(.versionIncrease, "\(version) is newer than \(repo.currentVersion ?? "-")"))
            case .noCurrentRelease:
                results.append(.pass(.versionIncrease, "No previous release found; \(version) will be the first"))
            case .same:
                results.append(.fail(.versionIncrease, "Version \(version) is already the current release", fix: "Increase the version number."))
            case .older:
                results.append(.fail(.versionIncrease, "Version \(version) is older than the current release \(repo.currentVersion ?? "")", fix: "Increase the version number."))
            case .requiresReview(let reason):
                results.append(.fail(.versionIncrease, "Version comparison requires review: \(reason)"))
            }
            switch repo.releasePathExists {
            case true?:
                results.append(.fail(.existingTag, "\(strategy.releasePath(version: version)) already exists in SVN", fix: "Releases are immutable. Publish a higher version."))
            case false?:
                results.append(.pass(.existingTag, "\(strategy.releasePath(version: version)) does not exist yet"))
            case nil:
                results.append(.skip(.existingTag, "Could not determine whether the release already exists"))
            }
        } else {
            results.append(.skip(.versionIncrease, "Repository unavailable; version could not be compared"))
            results.append(.skip(.existingTag, "Repository unavailable"))
        }
        return results
    }

    /// A readme update edits the released version in place: the package must carry exactly
    /// that version, and its tag must exist.
    func releasedVersion(_ c: ValidationContext, version: String, strategy: ReleaseStrategy) -> [ValidationResult] {
        let releasePath = strategy.releasePath(version: version)
        guard c.product.type == .plugin else {
            return [.fail(.releasedVersion, "Readme updates without a new version are only available for plugins", fix: "Publish a new theme version instead.")]
        }
        guard let repo = c.repository, repo.connectivityError == nil else {
            return [.skip(.releasedVersion, "Repository unavailable; the released version could not be checked")]
        }
        guard let current = repo.currentVersion else {
            return [.fail(.releasedVersion, "\(c.product.name) has no released version to update", fix: "Publish a version first.")]
        }
        var results: [ValidationResult] = []
        if PHPVersion.compare(version, current) == 0 {
            results.append(.pass(.releasedVersion, "Updates the readme of the released version \(current)"))
        } else {
            results.append(.fail(.releasedVersion, "The package is version \(version), but the released version is \(current)", fix: "Use the package of \(current), or publish \(version) as a new version."))
        }
        switch repo.releasePathExists {
        case true?: results.append(.pass(.existingTag, "\(releasePath) exists and will be updated"))
        case false?: results.append(.fail(.existingTag, "\(releasePath) does not exist in SVN", fix: "Publish \(version) as a new version."))
        case nil: results.append(.skip(.existingTag, "Could not determine whether \(releasePath) exists"))
        }
        return results
    }

    // MARK: Repository

    func repository(_ c: ValidationContext) -> [ValidationResult] {
        guard let repo = c.repository else {
            return [ValidationCheckID.svnConnectivity, .workingCopyClean, .remoteFreshness, .authentication].map { .skip($0, "Repository not checked") }
        }
        var results: [ValidationResult] = []
        if let error = repo.connectivityError {
            results.append(.fail(.svnConnectivity, error, fix: "Check the network connection and the SVN repository URL."))
        } else if repo.repositoryExists == false {
            results.append(.fail(.svnConnectivity, "Repository \(c.product.svnRepositoryURL.absoluteString) does not exist", fix: "Check the slug and repository URL."))
        } else {
            results.append(.pass(.svnConnectivity, "Connected (svn \(repo.svnVersion ?? "?"), r\(repo.remoteRevision.map(String.init) ?? "?"))"))
        }
        for problem in repo.structureProblems {
            results.append(.fail(.svnConnectivity, problem))
        }

        if let mismatch = repo.workingCopyURLMismatch {
            results.append(.fail(.workingCopyClean, mismatch, fix: "Choose an empty folder or a checkout of this product's repository."))
        } else if !repo.workingCopyChanges.isEmpty {
            let shown = repo.workingCopyChanges.prefix(5).joined(separator: ", ")
            results.append(.fail(.workingCopyClean, "Working copy has \(repo.workingCopyChanges.count) local change(s): \(shown)", fix: "Commit or revert them. ReleaseDock can back them up and revert."))
        } else {
            results.append(.pass(.workingCopyClean, repo.workingCopyExists ? "Working copy is clean" : "Working copy will be checked out"))
        }

        if !repo.outOfDatePaths.isEmpty {
            results.append(.fail(.remoteFreshness, "\(repo.outOfDatePaths.count) path(s) changed on the server since the last update", fix: "Update the working copy and prepare the release again."))
        } else if repo.connectivityError == nil {
            results.append(.pass(.remoteFreshness, "Working copy is up to date\(repo.workingCopyRevision.map { " (r\($0))" } ?? "")"))
        } else {
            results.append(.skip(.remoteFreshness, "Repository unavailable"))
        }

        switch repo.authentication {
        case .keychain:
            results.append(.pass(.authentication, "SVN credentials stored in the Keychain"))
        case .cachedBySVN:
            results.append(.pass(.authentication, "svn has cached credentials for this repository"))
        case .none:
            if c.product.releaseMode == .dryRun {
                results.append(.warn(.authentication, "No SVN credentials are available; a real release will need them"))
            } else {
                results.append(.fail(.authentication, "No SVN credentials are available", fix: "Add your WordPress.org username and SVN password (Profile → Account & Security → SVN credentials)."))
            }
        case .unknown:
            results.append(.skip(.authentication, "Authentication availability unknown"))
        }
        return results
    }

    func releaseDiff(_ c: ValidationContext, strategy: ReleaseStrategy) -> [ValidationResult] {
        guard let preview = c.preview else { return [.skip(.releaseDiff, "Computed after staging")] }
        guard let version = c.detected?.version else { return [.fail(.releaseDiff, "No version")] }
        var results: [ValidationResult] = []
        if preview.changes.isEmpty {
            results.append(.fail(.releaseDiff, c.kind == .readmeUpdate ? "The readme and assets already match the server; there is nothing to update" : "The release produces no SVN changes"))
        }
        if c.kind == .readmeUpdate {
            let releasePath = strategy.releasePath(version: version)
            let readme = c.detected?.readmeFile ?? "readme.txt"
            let allowed = ["trunk/\(readme)", "\(releasePath)/\(readme)"]
            let unexpected = preview.changes.filter { change in
                !(allowed.contains(change.path) || change.path == "assets" || change.path.hasPrefix("assets/"))
            }
            for change in unexpected.prefix(10) {
                results.append(.fail(.releaseDiff, "A readme update may only change the readme and /assets", file: change.path))
            }
            if results.isEmpty {
                results.append(.pass(.releaseDiff, "\(preview.changes.count) change(s) to the readme and assets of \(releasePath)"))
            }
            return results
        }
        let releasePath = strategy.releasePath(version: version)
        if !preview.changes.contains(where: { $0.path == releasePath && $0.kind == .added }) {
            results.append(.fail(.releaseDiff, "The preview does not create \(releasePath)"))
        }
        let allowed = strategy.allowedChangePrefixes(version: version)
        let unexpected = preview.changes.filter { change in !allowed.contains { change.path == $0 || change.path.hasPrefix($0 + "/") } }
        for change in unexpected.prefix(10) {
            results.append(.fail(.releaseDiff, "Unexpected change outside the release paths", file: change.path))
        }
        if results.isEmpty {
            results.append(.pass(.releaseDiff, "\(preview.added) added, \(preview.modified) modified, \(preview.deleted) deleted; creates \(releasePath)"))
        }
        return results
    }

    // MARK: Tested up to

    /// Compares "Tested up to" with the current WordPress branch (Plugin Check's
    /// `outdated_tested_upto_header`) and, for plugins, with the main file's own
    /// "Tested up to" header (`mismatched_tested_up_to_header`). Format problems are
    /// reported by the strategies; values that are not version numbers are ignored here.
    func testedUpToFreshness(_ c: ValidationContext) -> [ValidationResult] {
        guard let detected = c.detected else { return [] }
        var results: [ValidationResult] = []
        let readmeValue = detected.readme?.testedUpTo
        let tested: String?
        let file: String?
        switch detected.type {
        case .plugin:
            tested = readmeValue
            file = detected.readmeFile
            if let header = detected.headers["Tested up to"], let readmeValue,
               let order = WordPressBranch.compare(header, readmeValue), order != 0 {
                results.append(.warn(.testedUpTo, "Tested up to is \(header) in \(detected.mainFile ?? "the plugin file") but \(readmeValue) in the readme", file: detected.mainFile, fix: "Use the same value in both files, or remove it from the plugin header."))
            }
        case .theme:
            tested = detected.headers["Tested up to"] ?? readmeValue
            file = detected.mainFile
        }
        guard let tested, let latest = c.repository?.latestWordPressVersion,
              let latestBranch = WordPressBranch.of(latest),
              let order = WordPressBranch.compare(tested, latest)
        else { return results }
        if order < 0 {
            results.append(.warn(
                .testedUpTo,
                "Tested up to \(tested) is behind the current WordPress release \(latest); WordPress.org's Plugin Check reports it as outdated",
                file: file,
                fix: "Test with WordPress \(latestBranch), then set \"Tested up to: \(latestBranch)\"."
            ))
        } else if results.isEmpty {
            results.append(.pass(.testedUpTo, "Tested up to \(tested) covers the current WordPress release \(latest)"))
        }
        return results
    }

    // MARK: Code checks

    static let listedFindings = 15

    func codeChecks(_ c: ValidationContext) -> [ValidationResult] {
        guard let report = c.codeChecks else { return [] }
        var results: [ValidationResult] = []
        func location(_ f: CodeCheckFinding) -> String { f.line.map { "Line \($0): " } ?? "" }

        if let reason = report.phpLintSkipped {
            results.append(.skip(.phpSyntax, reason))
        } else {
            let lint = report.findings(.phpLint)
            let errors = lint.filter { $0.severity == .error }
            for finding in errors.prefix(Self.listedFindings) {
                results.append(.fail(.phpSyntax, location(finding) + finding.message, file: finding.file, fix: "Fix the syntax error. WordPress cannot load a file that does not parse."))
            }
            if errors.count > Self.listedFindings {
                results.append(.fail(.phpSyntax, "\(errors.count - Self.listedFindings) more syntax error(s)"))
            }
            for finding in lint.filter({ $0.severity != .error }).prefix(Self.listedFindings) {
                results.append(.warn(.phpSyntax, location(finding) + finding.message, file: finding.file))
            }
            if lint.isEmpty {
                results.append(.pass(.phpSyntax, "\(report.lintedFiles) PHP file(s) parse with PHP \(report.phpVersion ?? "?")"))
            }
        }

        if let reason = report.pluginCheckSkipped {
            results.append(.skip(.pluginCheck, reason))
        } else if report.pluginCheckRan {
            let findings = report.findings(.pluginCheck)
            if findings.isEmpty {
                results.append(.pass(.pluginCheck, "Plugin Check found no issues"))
            } else {
                let errors = findings.filter { $0.severity == .error }.count
                results.append(.warn(.pluginCheck, "Plugin Check: \(errors) error(s), \(findings.count - errors) warning(s). WordPress.org runs these checks on every release.", fix: "Review the findings; fix real issues before publishing."))
                for finding in findings.sorted(by: { $0.severity > $1.severity }).prefix(Self.listedFindings) {
                    let code = finding.code.map { "[\($0)] " } ?? ""
                    results.append(.warn(.pluginCheck, location(finding) + code + finding.message, file: finding.file))
                }
            }
        }
        return results
    }

    // MARK: Optional

    func optional(_ c: ValidationContext) -> [ValidationResult] {
        var results: [ValidationResult] = []
        let settings = c.product.validation
        let limit = UInt64(settings.packageSizeWarningMB) << 20
        if c.inspection.totalUncompressedSize > limit {
            results.append(.warn(.packageSize, "Package is \(c.inspection.totalUncompressedSize >> 20) MiB (warning threshold \(settings.packageSizeWarningMB) MiB)"))
        } else {
            results.append(.pass(.packageSize, "Package size \(ByteCountFormatter.string(fromByteCount: Int64(c.inspection.totalUncompressedSize), countStyle: .file))"))
        }

        let portability = c.inspection.issues.filter { $0.kind == .portability || $0.kind == .zeroByteFile }
        if portability.isEmpty {
            results.append(.pass(.portableFilenames, "File names are portable"))
        } else {
            results += portability.map { .warn(.portableFilenames, $0.message, file: $0.path) }
        }

        if let git = c.git, git.isRepository {
            if git.isClean {
                results.append(.pass(.gitWorkingTree, "Git tree clean at \(git.headCommit?.prefix(10) ?? "?")"))
            } else if settings.requireCleanGitTree {
                results.append(.fail(.gitWorkingTree, "Development folder has \(git.uncommittedChanges.count) uncommitted change(s)", fix: "Commit or stash changes before releasing."))
            } else {
                results.append(.warn(.gitWorkingTree, "Development folder has \(git.uncommittedChanges.count) uncommitted change(s)"))
            }
        } else if settings.requireCleanGitTree, c.inspection.sourceKind == .folder {
            results.append(.fail(.gitWorkingTree, "A clean Git tree is required but the folder is not a Git repository"))
        }
        return results
    }
}
