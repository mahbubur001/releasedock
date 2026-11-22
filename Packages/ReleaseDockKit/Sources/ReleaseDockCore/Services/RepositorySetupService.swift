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

        let info: SVNInfo
        do {
            info = try await svn.info(url.absoluteString)
        } catch let error as SVNError where error.kind == .notFound {
            throw RepositorySetupError.repositoryNotFound(url.absoluteString)
        }
        let entries = try await svn.list(url.absoluteString, recursive: false)
        let directories = Set(entries.filter { $0.kind == .dir }.map(\.name))
        let hasPluginLayout = directories.contains("trunk") && directories.contains("tags")
        let hasThemeLayout = directories.contains { ThemeReleaseStrategy.isVersionDirectory($0) }

        let layoutType: ProductType? = hasPluginLayout ? .plugin : (hasThemeLayout ? .theme : nil)
        let type = Self.inferredType(host: url.host) ?? layoutType ?? expectedType
        var description = RepositoryDescription(url: url, slug: url.lastPathComponent, type: type, revision: info.lastChangedRevision ?? info.revision)
        guard let type else {
            if entries.isEmpty {
                description.notes.append("The repository is empty; choose Plugin or Theme.")
                return description
            }
            throw RepositorySetupError.unknownLayout(url.absoluteString)
        }

        let probe = Product(name: description.slug, slug: description.slug, type: type, svnRepositoryURL: url, workingCopyPath: "/")
        let snapshot = try await strategies.strategy(for: type).inspectRemote(product: probe, candidateVersion: nil, svn: svn)
        description.currentVersion = snapshot.currentVersion
        description.versions = snapshot.existingVersions
        description.notes = snapshot.structureProblems + snapshot.notes
        description.productName = try await remoteProductName(url: url, type: type, slug: description.slug, currentVersion: snapshot.currentVersion)
        return description
    }

    private func remoteProductName(url: URL, type: ProductType, slug: String, currentVersion: String?) async throws -> String? {
        let base = url.absoluteString
        switch type {
        case .plugin:
            if let data = try await WorkingCopyOperations.catIfExists("\(base)/trunk/\(slug).php", svn: svn),
               let name = WordPressHeaders.parse(String(decoding: data.prefix(WordPressHeaders.readLimit), as: UTF8.self), headers: ["Plugin Name"])["Plugin Name"] {
                return name
            }
            for readme in ["readme.txt", "readme.md"] {
                if let data = try await WorkingCopyOperations.catIfExists("\(base)/trunk/\(readme)", svn: svn) {
                    return ReadmeParser.parse(String(decoding: data, as: UTF8.self)).name
                }
            }
            return nil
        case .theme:
            guard let currentVersion,
                  let data = try await WorkingCopyOperations.catIfExists("\(base)/\(currentVersion)/style.css", svn: svn) else { return nil }
            return WordPressHeaders.parse(String(decoding: data.prefix(WordPressHeaders.readLimit), as: UTF8.self), headers: ["Theme Name"])["Theme Name"]
        }
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

    /// Describes a user-selected checkout. Server details are used when reachable;
    /// otherwise the local copy is read.
    public func describeWorkingCopy(at path: URL) async throws -> WorkingCopyDescription {
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

        var repository: RepositoryDescription
        do {
            repository = try await describeRemote(url)
        } catch let error as SVNError where error.kind == .network || error.kind == .certificate {
            repository = describeLocally(url: url, path: path)
            repository.notes.append("The server could not be reached (\(error.localizedDescription)); details come from the local copy.")
        }
        let changes = try await WorkingCopyOperations.localChanges(at: path, svn: svn)
        return WorkingCopyDescription(path: path, repository: repository, revision: info.revision, localChanges: changes)
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
        let versions = type == .plugin
            ? ((try? fm.contentsOfDirectory(atPath: path.appendingPathComponent("tags").path)) ?? [])
            : names.filter(ThemeReleaseStrategy.isVersionDirectory)
        let current = VersionService().highest(of: versions)
        var name: String?
        let productRoot = type == .plugin ? path.appendingPathComponent("trunk") : current.map { path.appendingPathComponent($0) }
        if let productRoot, let detected = try? detector.detect(at: productRoot, expectedType: type, slug: url.lastPathComponent) {
            name = detected.name
        }
        return RepositoryDescription(url: url, slug: url.lastPathComponent, type: type, productName: name, currentVersion: current, versions: versions)
    }
}
