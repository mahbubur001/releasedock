import Foundation

/// WordPress.org plugin releases:
///
///     <slug>/
///     ├── assets/   banners, icons, screenshots (optional sync)
///     ├── tags/     one directory per released version
///     └── trunk/    latest code; trunk/readme.txt "Stable tag" selects the served tag
///
/// The release is one atomic commit containing the trunk update and `svn cp trunk
/// tags/<version>` (the approach used by 10up/action-wordpress-plugin-deploy).
public struct PluginReleaseStrategy: ReleaseStrategy {
    public init() {}

    public var productType: ProductType { .plugin }

    public func releasePath(version: String) -> String { "tags/\(version)" }

    public func allowedChangePrefixes(version: String) -> [String] {
        ["trunk", "tags/\(version)", "assets"]
    }

    /// The pattern the WordPress.org importer expects; anything else only warns there.
    static let importerVersionPattern = #"^\d+(?:\.\d+)*(?:-(?:rc|beta|alpha)(?:\.?\d+)?)?$"#

    public func validateVersionFormat(_ version: String) -> [ValidationResult] {
        if version.range(of: Self.importerVersionPattern, options: [.regularExpression, .caseInsensitive]) == nil {
            return [.warn(.versionValidity, "WordPress.org expects plugin versions like 1.2.3 or 1.2.3-beta1; \"\(version)\" will raise an importer warning")]
        }
        return []
    }

    public func validateMetadata(detected: DetectedProduct, product: Product) -> [ValidationResult] {
        var results: [ValidationResult] = []
        for problem in detected.problems {
            results.append(.fail(.metadataValidity, problem, fix: "Set the main plugin file in the product settings."))
        }
        guard let mainFile = detected.mainFile else { return results }
        if detected.name == nil {
            results.append(.fail(.metadataValidity, "Missing \"Plugin Name:\" header", file: mainFile))
        }
        if detected.headers["License"] == nil {
            results.append(.warn(.metadataValidity, "Main plugin file has no License header", file: mainFile))
        }

        guard let readme = detected.readme, let readmeFile = detected.readmeFile else {
            results.append(.fail(.metadataValidity, "readme.txt is missing", fix: "Add a readme.txt with a Stable tag header."))
            return results
        }
        if readmeFile.lowercased() == "readme.md" {
            results.append(.warn(.metadataValidity, "Using readme.md; WordPress.org prefers readme.txt", file: readmeFile))
        }
        if readme.name == nil {
            results.append(.fail(.metadataValidity, "readme has no \"=== Plugin Name ===\" title line", file: readmeFile))
        }

        // Stable tag: WordPress.org reads it from trunk/readme.txt; a missing or wrong value
        // silently serves trunk (or blocks with Release Confirmation).
        let version = detected.version ?? ""
        switch readme.stableTag {
        case nil, "":
            results.append(.fail(.metadataValidity, "readme Stable tag is missing", file: readmeFile, fix: "Add \"Stable tag: \(version)\"."))
        case "trunk"?:
            results.append(.fail(.metadataValidity, "Stable tag is \"trunk\", which WordPress.org discourages and prohibits for new plugins", file: readmeFile, fix: "Set \"Stable tag: \(version)\"."))
        case let tag?:
            if tag == version {
                results.append(.pass(.metadataValidity, "Stable tag \(tag) matches the plugin version"))
            } else {
                results.append(.fail(.metadataValidity, "Stable tag \(tag) does not match the plugin version \(version)", file: readmeFile, fix: "Set \"Stable tag: \(version)\"."))
            }
            if let raw = readme.rawStableTag, raw != tag {
                results.append(.warn(.metadataValidity, "Stable tag \"\(raw)\" is read by WordPress.org as \"\(tag)\"", file: readmeFile))
            }
        }

        if let tested = readme.testedUpTo {
            if tested.range(of: #"^\d+\.\d(\.\d+)?$"#, options: .regularExpression) == nil {
                results.append(.warn(.testedUpTo, "Tested up to \"\(tested)\" is not a WordPress version number like 6.9", file: readmeFile))
            } else {
                results.append(.pass(.testedUpTo, "Tested up to \(tested)"))
            }
        } else {
            results.append(.warn(.testedUpTo, "readme has no \"Tested up to\" header", file: readmeFile))
        }
        if let requiresPHP = readme.requiresPHP, requiresPHP.range(of: #"^\d+(\.\d+){1,2}$"#, options: .regularExpression) == nil {
            results.append(.warn(.metadataValidity, "Requires PHP \"\(requiresPHP)\" is not a version number and will be ignored", file: readmeFile))
        }
        // The main plugin file overrides these readme headers on WordPress.org.
        for (header, readmeValue) in [("Requires at least", readme.requiresAtLeast), ("Requires PHP", readme.requiresPHP)] {
            if let pluginValue = detected.headers[header], let readmeValue, pluginValue != readmeValue {
                results.append(.warn(.metadataValidity, "\(header) is \(pluginValue) in \(mainFile) but \(readmeValue) in the readme; the plugin file wins", file: mainFile))
            }
        }
        if readme.license == nil {
            results.append(.warn(.metadataValidity, "readme has no License header", file: readmeFile))
        }
        if readme.contributors.isEmpty {
            results.append(.warn(.metadataValidity, "readme has no Contributors", file: readmeFile))
        }
        results += ReadmeRules.check(readme, file: readmeFile, version: detected.version)
        results += ReadmeRules.checkRequiresPlugins(detected.headers["Requires Plugins"], slug: product.slug, file: mainFile)
        let tags = readme.tags.filter { !["plugin", "wordpress"].contains($0.lowercased()) }
        if tags.count > 5 {
            results.append(.warn(.metadataValidity, "readme has \(tags.count) tags; WordPress.org keeps only the first 5", file: readmeFile))
        }
        if product.validation.requireChangelogEntry {
            if readme.changelogMentions(version: version) {
                results.append(.pass(.changelogEntry, "Changelog has an entry for \(version)"))
            } else {
                results.append(.warn(.changelogEntry, "Changelog has no entry for \(version)", file: readmeFile, fix: "Add a \"= \(version) =\" entry under == Changelog ==."))
            }
        }
        return results
    }

    public func validateRequiredFiles(inspection: PackageInspection, detected: DetectedProduct) -> [ValidationResult] {
        let rootFiles = Set(inspection.includedFiles.map(\.path).filter { !$0.contains("/") })
        var results: [ValidationResult] = []
        if let main = detected.mainFile, rootFiles.contains(main) {
            results.append(.pass(.requiredFiles, "Main plugin file \(main) is at the package root"))
        } else {
            results.append(.fail(.requiredFiles, "Main plugin file must be at the package root"))
        }
        if let readme = detected.readmeFile, rootFiles.contains(readme) {
            results.append(.pass(.requiredFiles, "\(readme) present"))
        } else {
            results.append(.fail(.requiredFiles, "readme.txt must be at the package root"))
        }
        return results
    }

    public func validateForbiddenFiles(inspection: PackageInspection) -> [ValidationResult] {
        // Screenshots, banners and icons belong in /assets, not in trunk.
        inspection.includedFiles
            .filter { entry in
                let name = entry.path.lowercased()
                return !name.contains("/") && (name.hasPrefix("screenshot-") || name.hasPrefix("banner-") || name.hasPrefix("icon-"))
                    && [".png", ".jpg", ".jpeg", ".gif", ".svg"].contains(where: name.hasSuffix)
            }
            .map { .warn(.forbiddenFiles, "Directory assets belong in SVN /assets, not in the plugin", file: $0.path, fix: "Move it to the product's assets folder.") }
    }

    // MARK: Repository

    public func inspectRemote(product: Product, candidateVersion: String?, svn: SVNServicing) async throws -> RepositorySnapshot {
        var snapshot = RepositorySnapshot()
        let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
        // Independent requests run concurrently: each WordPress.org round trip takes seconds.
        async let infoRequest = svn.info(base)
        async let topRequest = svn.list(base, recursive: false)
        let info: SVNInfo
        do {
            info = try await infoRequest
        } catch let error as SVNError where error.kind == .notFound {
            snapshot.repositoryExists = false
            return snapshot
        }
        snapshot.repositoryExists = true
        snapshot.remoteRevision = info.lastChangedRevision ?? info.revision

        let names = Set(try await topRequest.filter { $0.kind == .dir }.map(\.name))
        for required in ["trunk", "tags"] where !names.contains(required) {
            snapshot.structureProblems.append("Repository has no /\(required) directory; this does not look like a WordPress.org plugin repository")
        }
        guard snapshot.structureProblems.isEmpty else { return snapshot }

        async let tagsRequest = svn.list("\(base)/tags", recursive: false)
        async let trunkRequest = svn.list("\(base)/trunk", recursive: false)
        let tags = try await tagsRequest.filter { $0.kind == .dir }.map(\.name)
        let trunkFiles = try await trunkRequest.filter { $0.kind == .file }.map(\.name)
        snapshot.existingVersions = tags

        // Released version = trunk readme Stable tag when that tag exists; otherwise the
        // highest tag (what the importer falls back to when trunk is empty). WordPress.org
        // matches the readme name case-insensitively, preferring readme.txt.
        var stable: String?
        if let readme = Self.readmeName(in: trunkFiles),
           let data = try await WorkingCopyOperations.catIfExists("\(base)/trunk/\(readme)", svn: svn) {
            let parsed = ReadmeParser.parse(String(decoding: data, as: UTF8.self))
            stable = parsed.stableTag
            snapshot.readmeTitle = parsed.name
        }
        if let stable, !stable.isEmpty, stable != "trunk", tags.contains(stable) {
            snapshot.currentVersion = stable
            snapshot.currentVersionSource = "trunk readme Stable tag"
        } else if let highest = VersionService().highest(of: tags) {
            snapshot.currentVersion = highest
            snapshot.currentVersionSource = "highest tag"
            if let stable, !stable.isEmpty {
                snapshot.notes.append("Current Stable tag \"\(stable)\" does not point to an existing tag; WordPress.org is serving trunk.")
            }
        }
        if let candidateVersion {
            snapshot.releasePathExists = tags.contains(candidateVersion)
        }
        return snapshot
    }

    /// `readme.txt` (preferred) or `readme.md`, matched case-insensitively like WordPress.org.
    static func readmeName(in files: [String]) -> String? {
        for wanted in ["readme.txt", "readme.md"] {
            if let name = files.first(where: { $0.lowercased() == wanted }) { return name }
        }
        return nil
    }

    public func prepareWorkingCopy(product: Product, currentVersion: String?, svn: SVNServicing) async throws {
        let wc = product.workingCopyURL
        let fm = FileManager.default
        if !fm.fileExists(atPath: wc.appendingPathComponent(".svn").path) {
            try fm.createDirectory(at: wc, withIntermediateDirectories: true)
            // Sparse checkout: never download every tag.
            try await svn.checkout(product.svnRepositoryURL.absoluteString, to: wc, depth: .immediates)
            try await svn.update(wc.appendingPathComponent("trunk"), setDepth: .infinity)
            try await svn.update(wc.appendingPathComponent("tags"), setDepth: .immediates)
            if product.assetsPath != nil, fm.fileExists(atPath: wc.appendingPathComponent("assets").path) {
                try await svn.update(wc.appendingPathComponent("assets"), setDepth: .infinity)
            }
        } else {
            try await svn.update(wc, setDepth: nil)
            if product.assetsPath != nil, fm.fileExists(atPath: wc.appendingPathComponent("assets").path) {
                try await svn.update(wc.appendingPathComponent("assets"), setDepth: .infinity)
            }
        }
    }

    public func apply(candidate: StagedCandidate, version: String, currentVersion: String?, product: Product, svn: SVNServicing) async throws {
        let wc = product.workingCopyURL
        let tag = wc.appendingPathComponent("tags").appendingPathComponent(version)
        if FileManager.default.fileExists(atPath: tag.path) {
            throw ReleaseError.releasePathExists("tags/\(version)")
        }
        try await WorkingCopyOperations.mirror(candidate: candidate, into: wc.appendingPathComponent("trunk"), svn: svn)

        if let assetsPath = product.assetsPath {
            try await syncAssets(from: URL(fileURLWithPath: assetsPath, isDirectory: true), into: wc.appendingPathComponent("assets"), svn: svn)
        }
        // Copy trunk (with its scheduled changes) to the new tag; committed atomically together.
        try await svn.copy(wc.appendingPathComponent("trunk"), to: tag)
    }

    static let assetMimeTypes = ["png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif", "svg": "image/svg+xml"]

    func syncAssets(from source: URL, into target: URL, svn: SVNServicing) async throws {
        let inspection = try FolderInspectionService().inspect(folderAt: source, options: InspectionOptions())
        let blocking = inspection.blockingIssues
        guard blocking.isEmpty else { throw PackageInspectionError.blockingIssues(blocking) }
        let candidate = StagedCandidate(rootPath: source.path, entries: inspection.includedEntries, treeDigest: inspection.sha256)
        try await WorkingCopyOperations.mirror(candidate: candidate, into: target, svn: svn)
        // Set MIME types explicitly so images display instead of downloading.
        for (ext, mime) in Self.assetMimeTypes {
            let files = inspection.includedFiles.filter { ($0.path as NSString).pathExtension.lowercased() == ext }
                .map { target.appendingPathComponent($0.path) }
            try await svn.setProperty("svn:mime-type", value: mime, paths: files)
        }
    }

    public func verify(candidate: StagedCandidate, version: String, product: Product, svn: SVNServicing, scratch: URL) async throws -> VerificationReport {
        let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
        let tagURL = "\(base)/tags/\(version)"
        guard try await svn.exists(tagURL) else {
            return VerificationReport(committedRevision: nil, tagExists: false, tagURL: tagURL, notes: ["The tag does not exist on the server."])
        }
        let info = try await svn.info(tagURL)
        let tree = try await WorkingCopyOperations.verifyRemoteTree(url: tagURL, candidate: candidate, svn: svn, scratch: scratch)
        var stableMatches: Bool?
        var notes: [String] = []
        let trunkFiles = try await svn.list("\(base)/trunk", recursive: false).filter { $0.kind == .file }.map(\.name)
        if let readme = Self.readmeName(in: trunkFiles),
           let data = try await WorkingCopyOperations.catIfExists("\(base)/trunk/\(readme)", svn: svn) {
            let stable = ReadmeParser.parse(String(decoding: data, as: UTF8.self)).stableTag
            stableMatches = stable == version
            if stableMatches == false { notes.append("trunk readme Stable tag is \(stable ?? "missing"), not \(version).") }
        }
        if !tree.mismatched.isEmpty {
            notes.append("Checksum differences can come from svn:eol-style or keyword properties applied by the repository.")
        }
        return VerificationReport(
            committedRevision: info.lastChangedRevision,
            tagExists: true,
            tagURL: tagURL,
            missingFiles: tree.missing,
            unexpectedFiles: tree.unexpected,
            checksumMismatches: tree.mismatched,
            stableTagMatches: stableMatches,
            notes: notes
        )
    }

    // MARK: Readme updates

    public func applyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws {
        let wc = product.workingCopyURL
        let tag = wc.appendingPathComponent("tags").appendingPathComponent(version)
        guard FileManager.default.fileExists(atPath: tag.path) else {
            throw ReleaseError.invalidState("tags/\(version) is not in the working copy.")
        }
        // Tags are checked out empty (sparse); the released one is needed in full to compare.
        try await svn.update(tag, setDepth: .infinity)
        let differences = try WorkingCopyOperations.differences(candidate: candidate, releasedTree: tag, ignoring: [readmeFile])
        guard differences.isEmpty else { throw ReleaseError.readmeUpdateChangesCode(tag: "tags/\(version)", paths: differences) }

        let readme = candidate.rootURL.appendingPathComponent(readmeFile)
        try await WorkingCopyOperations.replaceFile(at: wc.appendingPathComponent("trunk").appendingPathComponent(readmeFile), with: readme, svn: svn)
        try await WorkingCopyOperations.replaceFile(at: tag.appendingPathComponent(readmeFile), with: readme, svn: svn)
        if let assetsPath = product.assetsPath {
            try await syncAssets(from: URL(fileURLWithPath: assetsPath, isDirectory: true), into: wc.appendingPathComponent("assets"), svn: svn)
        }
    }

    public func verifyReadmeUpdate(candidate: StagedCandidate, readmeFile: String, version: String, product: Product, svn: SVNServicing) async throws -> VerificationReport {
        let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
        let tagURL = "\(base)/tags/\(version)"
        guard try await svn.exists(tagURL) else {
            return VerificationReport(committedRevision: nil, tagExists: false, tagURL: tagURL, notes: ["The tag does not exist on the server."])
        }
        let expected = candidate.entries.first { $0.path == readmeFile }?.sha256
        var missing: [String] = []
        var mismatched: [String] = []
        var revision: Int?
        var stableMatches: Bool?
        for path in ["trunk/\(readmeFile)", "tags/\(version)/\(readmeFile)"] {
            guard let data = try await WorkingCopyOperations.catIfExists("\(base)/\(path)", svn: svn) else {
                missing.append(path)
                continue
            }
            if Hashing.sha256(data) != expected { mismatched.append(path) }
            if path.hasPrefix("trunk/") {
                stableMatches = ReadmeParser.parse(String(decoding: data, as: UTF8.self)).stableTag == version
            }
            revision = max(revision ?? 0, try await svn.info("\(base)/\(path)").lastChangedRevision ?? 0)
        }
        var notes: [String] = []
        if stableMatches == false { notes.append("trunk readme Stable tag is not \(version).") }
        if !mismatched.isEmpty {
            notes.append("Checksum differences can come from svn:eol-style or keyword properties applied by the repository.")
        }
        return VerificationReport(
            committedRevision: revision,
            tagExists: true,
            tagURL: tagURL,
            missingFiles: missing,
            checksumMismatches: mismatched,
            stableTagMatches: stableMatches,
            notes: notes
        )
    }

    public func postReadmeUpdateGuidance(product: Product, version: String) -> [String] {
        [
            "WordPress.org refreshes the plugin page within minutes. Banner, icon and screenshot changes can take up to 6 hours to appear because of caching.",
            "If Release Confirmation is enabled for \(product.name), changing tags/\(version) may need confirming again at https://wordpress.org/plugins/developers/releases/.",
        ]
    }

    public func postReleaseGuidance(product: Product, version: String) -> [String] {
        [
            "WordPress.org holds new releases for a security review period (currently about 6 hours) before offering the update to sites. ReleaseDock checks the directory and notifies you when \(version) is live.",
            "If Release Confirmation is enabled for \(product.name), every committer receives a \"Pending release\" email. Confirm version \(version) at https://wordpress.org/plugins/developers/releases/ — users stay on the previous version until it is confirmed.",
            "Do not modify tags/\(version) after it is confirmed; changes require confirming again.",
        ]
    }
}

extension String {
    var trimmingTrailingSlash: String { hasSuffix("/") ? String(dropLast()) : self }
}
