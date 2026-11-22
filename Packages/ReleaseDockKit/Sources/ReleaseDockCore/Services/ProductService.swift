import Foundation

public struct ProductValidationIssue: Hashable, Sendable, Identifiable {
    public enum Field: String, Sendable {
        case name, slug, headerName, svnURL, workingCopy, developmentPath, assetsPath, githubURL, mainPluginFile, svnUsername, automation
    }

    public var id: String { "\(field.rawValue):\(message)" }
    public var field: Field
    public var message: String
    public var isError: Bool

    public init(field: Field, message: String, isError: Bool) {
        self.field = field
        self.message = message
        self.isError = isError
    }
}

/// What a development folder says about itself, used to pre-fill product registration.
public struct ProductSuggestion: Hashable, Sendable {
    public var type: ProductType
    public var name: String
    public var slug: String
    public var version: String?
    public var mainFile: String?
}

public protocol ProductServicing: Sendable {
    func allProducts() throws -> [Product]
    func validate(_ product: Product) -> [ProductValidationIssue]
    func save(_ product: Product) throws
    func delete(_ productID: UUID) throws
    func suggestion(forFolder url: URL) throws -> ProductSuggestion
}

public enum ProductServiceError: Error, LocalizedError, Sendable {
    case invalid([ProductValidationIssue])

    public var errorDescription: String? {
        switch self {
        case .invalid(let issues): issues.filter(\.isError).map(\.message).joined(separator: "\n")
        }
    }
}

public struct ProductService: ProductServicing {
    private let repository: ProductRepository
    private let paths: AppPaths
    private let detector: ProductDetector

    public init(repository: ProductRepository, paths: AppPaths, detector: ProductDetector = ProductDetector()) {
        self.repository = repository
        self.paths = paths
        self.detector = detector
    }

    public func allProducts() throws -> [Product] { try repository.allProducts() }

    public func validate(_ product: Product) -> [ProductValidationIssue] {
        var issues: [ProductValidationIssue] = []
        func error(_ field: ProductValidationIssue.Field, _ message: String) { issues.append(.init(field: field, message: message, isError: true)) }
        func warning(_ field: ProductValidationIssue.Field, _ message: String) { issues.append(.init(field: field, message: message, isError: false)) }

        if product.name.trimmingCharacters(in: .whitespaces).isEmpty { error(.name, "Name is required.") }
        if !PathSafety.isValidSlug(product.slug) { error(.slug, "Slug must use lowercase letters, digits and hyphens.") }
        if product.expectedHeaderName.trimmingCharacters(in: .whitespaces).isEmpty {
            error(.headerName, "Expected \(product.type == .plugin ? "Plugin" : "Theme") Name is required for identity checks.")
        }

        // SVN URL
        let url = product.svnRepositoryURL
        switch url.scheme?.lowercased() {
        case "https"?:
            if url.user != nil || url.password != nil { error(.svnURL, "The SVN URL must not contain credentials.") }
            if let host = url.host?.lowercased(), host.hasSuffix(".svn.wordpress.org") {
                let expectedHost = product.type == .plugin ? "plugins.svn.wordpress.org" : "themes.svn.wordpress.org"
                if host != expectedHost { error(.svnURL, "\(product.type.displayName)s are hosted on \(expectedHost).") }
                let path = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
                if path != product.slug { error(.svnURL, "The WordPress.org URL must be https://\(expectedHost)/\(product.slug).") }
            }
        case "file"?:
            warning(.svnURL, "Local file:// repositories are for testing only.")
        default:
            error(.svnURL, "The SVN URL must use https.")
        }

        // Paths
        let home = NSHomeDirectory()
        let wc = product.workingCopyPath
        if wc.isEmpty || !wc.hasPrefix("/") {
            error(.workingCopy, "The working copy path must be absolute.")
        } else {
            let wcURL = URL(fileURLWithPath: wc).standardizedFileURL
            if ["/", home, home + "/"].contains(wcURL.path) { error(.workingCopy, "Choose a dedicated folder for the SVN working copy.") }
            if PathSafety.isContained(wcURL, in: paths.staging) { error(.workingCopy, "The working copy cannot live inside the staging area.") }
            if let dev = product.localDevelopmentPath {
                let devURL = URL(fileURLWithPath: dev).standardizedFileURL
                if PathSafety.isContained(wcURL, in: devURL) || PathSafety.isContained(devURL, in: wcURL) {
                    error(.workingCopy, "The SVN working copy and the development folder must not contain each other.")
                }
            }
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: wcURL.path, isDirectory: &isDir), !isDir.boolValue {
                error(.workingCopy, "The working copy path is a file.")
            }
        }
        if let dev = product.localDevelopmentPath, !dev.isEmpty {
            var isDir: ObjCBool = false
            if !FileManager.default.fileExists(atPath: dev, isDirectory: &isDir) || !isDir.boolValue {
                warning(.developmentPath, "The development folder does not exist.")
            }
        }
        if let assets = product.assetsPath, !assets.isEmpty {
            if product.type == .theme { error(.assetsPath, "Assets sync applies to plugins only.") }
            var isDir: ObjCBool = false
            if !FileManager.default.fileExists(atPath: assets, isDirectory: &isDir) || !isDir.boolValue {
                error(.assetsPath, "The assets folder does not exist.")
            }
        }
        if let main = product.mainPluginFile, !main.isEmpty {
            if main.contains("/") || !main.lowercased().hasSuffix(".php") { error(.mainPluginFile, "The main plugin file must be a .php file name at the plugin root.") }
        }
        if let github = product.githubRepositoryURL, GitHubRepository(string: github.absoluteString) == nil {
            error(.githubURL, "Enter a GitHub repository like https://github.com/owner/repo.")
        }
        if let username = product.svnUsername, username != username.trimmingCharacters(in: .whitespaces) {
            error(.svnUsername, "The SVN username has leading or trailing spaces.")
        }
        if product.releaseMode == .automatic && !product.automationEnabled {
            warning(.automation, "Automatic Release mode has no effect until automation is explicitly enabled.")
        }
        return issues
    }

    public func save(_ product: Product) throws {
        let issues = validate(product)
        guard !issues.contains(where: \.isError) else { throw ProductServiceError.invalid(issues) }
        var updated = product
        updated.updatedAt = Date()
        try repository.save(updated)
    }

    public func delete(_ productID: UUID) throws {
        try repository.deleteProduct(id: productID)
    }

    public func suggestion(forFolder url: URL) throws -> ProductSuggestion {
        let folderSlug = url.lastPathComponent.lowercased()
        let detected = try detector.detect(at: url, slug: folderSlug)
        let slug = PathSafety.isValidSlug(detected.textDomain ?? "") ? detected.textDomain! : folderSlug
        return ProductSuggestion(type: detected.type, name: detected.name ?? url.lastPathComponent, slug: slug, version: detected.version, mainFile: detected.mainFile)
    }

    /// Default working-copy location for a new product.
    public func defaultWorkingCopyPath(slug: String, type: ProductType) -> String {
        paths.defaultWorkingCopy(forSlug: slug, type: type).path
    }
}
