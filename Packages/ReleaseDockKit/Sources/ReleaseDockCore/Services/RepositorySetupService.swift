import Foundation

/// What ReleaseDock learned about an SVN repository for a product.
public struct RepositoryDescription: Hashable, Sendable {
    public var url: URL
    public var slug: String
    /// Inferred from the WordPress.org host or the repository layout; nil when unclear.
    public var type: ProductType?
    public var productName: String?
    public var currentVersion: String?
    public var versions: [String]
    public var revision: Int?
    public var notes: [String]

    public init(url: URL, slug: String, type: ProductType?, productName: String? = nil, currentVersion: String? = nil, versions: [String] = [], revision: Int? = nil, notes: [String] = []) {
        self.url = url
        self.slug = slug
        self.type = type
        self.productName = productName
        self.currentVersion = currentVersion
        self.versions = versions
        self.revision = revision
        self.notes = notes
    }
}

/// An existing checkout selected by the user.
public struct WorkingCopyDescription: Hashable, Sendable {
    public var path: URL
    public var repository: RepositoryDescription
    public var revision: Int
    public var localChanges: [String]
}

public enum RepositorySetupError: Error, LocalizedError, Equatable, Sendable {
    case invalidURL(String)
    case repositoryNotFound(String)
    case notProductRoot(found: String, suggestion: String)
    case notAWorkingCopy(String)
    case notWorkingCopyRoot(selected: String, root: String)
    case unknownLayout(String)
    case destinationNotEmpty(String)

    public var errorDescription: String? {
        switch self {
        case .invalidURL(let url): "“\(url)” is not a valid https SVN URL."
        case .repositoryNotFound(let url): "No repository exists at \(url). Check the slug and product type."
        case .notProductRoot(let found, let suggestion): "\(found) is inside the product repository. Use the product root instead: \(suggestion)"
        case .notAWorkingCopy(let path): "\(path) is not an SVN working copy."
        case .notWorkingCopyRoot(let selected, let root): "\(selected) is part of a larger working copy. Select its root: \(root)"
        case .unknownLayout(let url): "\(url) does not look like a WordPress.org plugin (trunk/tags) or theme (version folders) repository."
        case .destinationNotEmpty(let path): "\(path) already exists and is not empty. Choose an empty or new folder."
        }
    }
}

/// Connects a product to SVN: inspects a repository on the server and checks it out
/// sparsely, or adopts an existing working copy chosen by the user.
public struct RepositorySetupService: Sendable {
    private let svn: SVNServicing
    private let strategies: StrategyProvider
    private let detector: ProductDetector

    public init(svn: SVNServicing, strategies: StrategyProvider = StrategyProvider(), detector: ProductDetector = ProductDetector()) {
        self.svn = svn
        self.strategies = strategies
        self.detector = detector
    }

    // MARK: Remote

    /// Normalizes user input: a full URL, or a slug combined with the WordPress.org host for `type`.
    public static func repositoryURL(from input: String, type: ProductType) throws -> URL {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.contains("/"), PathSafety.isValidSlug(trimmed), let url = type.defaultRepositoryURL(slug: trimmed) {
            return url
        }
        guard var components = URLComponents(string: trimmed), let scheme = components.scheme?.lowercased(),
              ["https", "file"].contains(scheme), components.user == nil, components.password == nil else {
            throw RepositorySetupError.invalidURL(trimmed)
        }
        while components.path.hasSuffix("/") { components.path.removeLast() }
        guard let url = components.url, !url.lastPathComponent.isEmpty, url.lastPathComponent != "/" else {
            throw RepositorySetupError.invalidURL(trimmed)
        }
        return url
    }

    static func inferredType(host: String?) -> ProductType? {
        switch host?.lowercased() {
        case "plugins.svn.wordpress.org"?: .plugin
        case "themes.svn.wordpress.org"?: .theme
        default: nil
        }
    }

    /// Reads the repository on the server without changing anything locally.
    public func describeRemote(_ url: URL, expectedType: ProductType? = nil) async throws -> RepositoryDescription {
        let last = url.lastPathComponent
        let parent = url.deletingLastPathComponent()
        if last == "trunk" || last == "tags" || last == "assets" || parent.lastPathComponent == "tags" {
            let root = last == "trunk" || last == "tags" || last == "assets" ? parent : parent.deletingLastPathComponent()
            throw RepositorySetupError.notProductRoot(found: url.absoluteString, suggestion: root.absoluteString.trimmingTrailingSlash)
        }

        // On WordPress.org the host decides the type; elsewhere the layout does.
        let hostType = Self.inferredType(host: url.host)
        var type = hostType
        var revision: Int?
        if hostType == nil {
            async let infoRequest = svn.info(url.absoluteString)
            async let listRequest = svn.list(url.absoluteString, recursive: false)
            let info: SVNInfo
            do {
                info = try await infoRequest
            } catch let error as SVNError where error.kind == .notFound {
                throw RepositorySetupError.repositoryNotFound(url.absoluteString)
            }
            revision = info.lastChangedRevision ?? info.revision
            let entries = try await listRequest
            let directories = Set(entries.filter { $0.kind == .dir }.map(\.name))
            let layoutType: ProductType? = directories.contains("trunk") && directories.contains("tags")
                ? .plugin : (directories.contains { ThemeReleaseStrategy.isVersionDirectory($0) } ? .theme : nil)
            type = layoutType ?? expectedType
            if type == nil {
                if entries.isEmpty {
                    return RepositoryDescription(url: url, slug: url.lastPathComponent, type: nil, revision: revision, notes: ["The repository is empty; choose Plugin or Theme."])
                }
                throw RepositorySetupError.unknownLayout(url.absoluteString)
            }
        }
        guard let type else { throw RepositorySetupError.unknownLayout(url.absoluteString) }

        let slug = url.lastPathComponent
        let probe = Product(name: slug, slug: slug, type: type, svnRepositoryURL: url, workingCopyPath: "/")
        // The main plugin file is usually <slug>.php; read it alongside the repository scan.
        async let headerName = type == .plugin ? pluginHeaderName(url: url, slug: slug) : nil
        let snapshot = try await strategies.strategy(for: type).inspectRemote(product: probe, candidateVersion: nil, svn: svn)
        if snapshot.repositoryExists == false { throw RepositorySetupError.repositoryNotFound(url.absoluteString) }

        var description = RepositoryDescription(url: url, slug: slug, type: type, revision: snapshot.remoteRevision ?? revision)
        description.currentVersion = snapshot.currentVersion
        description.versions = snapshot.existingVersions
        description.notes = snapshot.structureProblems + snapshot.notes
        switch type {
        case .plugin: description.productName = try await headerName ?? snapshot.readmeTitle
        case .theme: description.productName = try await themeName(url: url, currentVersion: snapshot.currentVersion)
        }
        return description
    }

    private func pluginHeaderName(url: URL, slug: String) async throws -> String? {
        guard let data = try await WorkingCopyOperations.catIfExists("\(url.absoluteString)/trunk/\(slug).php", svn: svn) else { return nil }
        return WordPressHeaders.parse(String(decoding: data.prefix(WordPressHeaders.readLimit), as: UTF8.self), headers: ["Plugin Name"])["Plugin Name"]
    }

    private func themeName(url: URL, currentVersion: String?) async throws -> String? {
        guard let currentVersion,
              let data = try await WorkingCopyOperations.catIfExists("\(url.absoluteString)/\(currentVersion)/style.css", svn: svn) else { return nil }
        return WordPressHeaders.parse(String(decoding: data.prefix(WordPressHeaders.readLimit), as: UTF8.self), headers: ["Theme Name"])["Theme Name"]
    }

    /// Sparse checkout into an empty (or new) folder, laid out exactly as releases need it.
    public func checkout(_ description: RepositoryDescription, type: ProductType, to destination: URL) async throws {
        let fm = FileManager.default
        if fm.fileExists(atPath: destination.path) {
            let contents = try fm.contentsOfDirectory(atPath: destination.path).filter { $0 != ".DS_Store" }
            guard contents.isEmpty else { throw RepositorySetupError.destinationNotEmpty(destination.path) }
        }
        let product = Product(name: description.slug, slug: description.slug, type: type, svnRepositoryURL: description.url, workingCopyPath: destination.path)
        try await strategies.strategy(for: type).prepareWorkingCopy(product: product, currentVersion: description.currentVersion, svn: svn)
    }

    // MARK: Existing working copy

    /// Describes a user-selected checkout from local data only (fast, no network): the
    /// repository URL from `svn info`, layout, versions and headers from disk, local changes.
    public func describeWorkingCopyLocally(at path: URL) async throws -> WorkingCopyDescription {
        let info: SVNInfo
        do {
            info = try await svn.info(path.path)
        } catch let error as SVNError where error.kind == .notAWorkingCopy || error.kind == .notFound {
            throw RepositorySetupError.notAWorkingCopy(path.path)
        }
        if let root = info.workingCopyRoot, URL(fileURLWithPath: root).standardizedFileURL.path != path.standardizedFileURL.path {
            throw RepositorySetupError.notWorkingCopyRoot(selected: path.path, root: root)
        }
        guard let url = URL(string: info.url) else { throw RepositorySetupError.invalidURL(info.url) }
        let last = url.lastPathComponent
        if last == "trunk" || last == "tags" || last == "assets" {
            throw RepositorySetupError.notProductRoot(found: url.absoluteString, suggestion: url.deletingLastPathComponent().absoluteString.trimmingTrailingSlash)
        }
        var repository = describeLocally(url: url, path: path)
        repository.revision = info.revision
        let changes = try await WorkingCopyOperations.localChanges(at: path, svn: svn)
        return WorkingCopyDescription(path: path, repository: repository, revision: info.revision, localChanges: changes)
    }

    /// Local description refreshed with server details when the server is reachable.
    public func describeWorkingCopy(at path: URL) async throws -> WorkingCopyDescription {
        var description = try await describeWorkingCopyLocally(at: path)
        do {
            let remote = try await describeRemote(description.repository.url, expectedType: description.repository.type)
            description.repository = Self.merge(local: description.repository, remote: remote)
        } catch let error as SVNError where error.kind == .network || error.kind == .certificate {
            description.repository.notes.append("The server could not be reached (\(error.localizedDescription)); details come from the local copy.")
        }
        return description
    }

    /// Server data wins for versions; the local name is kept if the server could not supply one.
    public static func merge(local: RepositoryDescription, remote: RepositoryDescription) -> RepositoryDescription {
        var merged = remote
        merged.productName = remote.productName ?? local.productName
        merged.type = remote.type ?? local.type
        return merged
    }

    /// Builds a fully populated product from a repository and its local checkout. Identity
    /// comes from the checked-out headers (trunk for plugins, the latest version folder for
    /// themes), falling back to what the server reported.
    public func makeProduct(from description: RepositoryDescription, type: ProductType, workingCopy: URL) -> Product {
        let productRoot = type == .plugin
            ? workingCopy.appendingPathComponent("trunk")
            : description.currentVersion.map { workingCopy.appendingPathComponent($0) }
        let detected = productRoot.flatMap { try? detector.detect(at: $0, expectedType: type, slug: description.slug) }
        let name = detected?.name ?? description.productName ?? description.slug
        var product = Product(
            name: name,
            slug: description.slug,
            type: type,
            expectedHeaderName: name,
            svnRepositoryURL: description.url,
            workingCopyPath: workingCopy.path,
            currentVersion: description.currentVersion
        )
        if type == .plugin, let main = detected?.mainFile, main != "\(description.slug).php" {
            product.mainPluginFile = main
        }
        return product
    }

    private func describeLocally(url: URL, path: URL) -> RepositoryDescription {
        let fm = FileManager.default
        let names = (try? fm.contentsOfDirectory(atPath: path.path)) ?? []
        var type = Self.inferredType(host: url.host)
        if type == nil {
            type = names.contains("trunk") ? .plugin : (names.contains(where: ThemeReleaseStrategy.isVersionDirectory) ? .theme : nil)
        }
        let versions = (type == .plugin
            ? ((try? fm.contentsOfDirectory(atPath: path.appendingPathComponent("tags").path)) ?? [])
            : names.filter(ThemeReleaseStrategy.isVersionDirectory)).filter { !$0.hasPrefix(".") }
        let current = VersionService().highest(of: versions)
        var name: String?
        let productRoot = type == .plugin ? path.appendingPathComponent("trunk") : current.map { path.appendingPathComponent($0) }
        if let productRoot, let detected = try? detector.detect(at: productRoot, expectedType: type, slug: url.lastPathComponent) {
            name = detected.name
        }
        return RepositoryDescription(url: url, slug: url.lastPathComponent, type: type, productName: name, currentVersion: current, versions: versions)
    }
}
