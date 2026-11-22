import Foundation

/// What a package root says about itself. The version always comes from file
/// headers inside the package, never from a ZIP or folder name.
public struct DetectedProduct: Codable, Hashable, Sendable {
    public var type: ProductType
    /// `Plugin Name:` or `Theme Name:`.
    public var name: String?
    public var version: String?
    public var textDomain: String?
    /// Main plugin file or `style.css`, relative to the package root.
    public var mainFile: String?
    public var headers: [String: String]
    public var readme: WordPressReadme?
    public var readmeFile: String?
    /// Problems encountered while detecting (ambiguous main file, unreadable headers…).
    public var problems: [String]

    public init(
        type: ProductType,
        name: String? = nil,
        version: String? = nil,
        textDomain: String? = nil,
        mainFile: String? = nil,
        headers: [String: String] = [:],
        readme: WordPressReadme? = nil,
        readmeFile: String? = nil,
        problems: [String] = []
    ) {
        self.type = type
        self.name = name
        self.version = version
        self.textDomain = textDomain
        self.mainFile = mainFile
        self.headers = headers
        self.readme = readme
        self.readmeFile = readmeFile
        self.problems = problems
    }

    /// `Template:` header present → child theme.
    public var parentTheme: String? { type == .theme ? headers["Template"] : nil }
}

public enum ProductDetectionError: Error, LocalizedError, Equatable, Sendable {
    case notADirectory(String)
    case noProductFound
    case ambiguousType

    public var errorDescription: String? {
        switch self {
        case .notADirectory(let path): "Not a directory: \(path)"
        case .noProductFound:
            "No WordPress plugin (PHP file with a \"Plugin Name:\" header) or theme (style.css with a \"Theme Name:\" header) was found at the package root."
        case .ambiguousType:
            "The package contains both a plugin header and a theme style.css header."
        }
    }
}

public struct ProductDetector: Sendable {
    public init() {}

    /// Detects a plugin or theme at `root`. Pass `expectedType` to resolve packages that
    /// look like both, and `mainPluginFile` to pin the plugin main file.
    public func detect(at root: URL, expectedType: ProductType? = nil, mainPluginFile: String? = nil, slug: String? = nil) throws -> DetectedProduct {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isDir), isDir.boolValue else {
            throw ProductDetectionError.notADirectory(root.path)
        }
        let plugin = try detectPlugin(at: root, mainPluginFile: mainPluginFile, slug: slug)
        let theme = try detectTheme(at: root)

        switch (plugin, theme, expectedType) {
        case (let p?, nil, _): return p
        case (nil, let t?, _): return t
        case (let p?, _?, .plugin?): return p
        case (_?, let t?, .theme?): return t
        case (_?, _?, nil): throw ProductDetectionError.ambiguousType
        case (nil, nil, _): throw ProductDetectionError.noProductFound
        }
    }

    // MARK: Plugin

    func detectPlugin(at root: URL, mainPluginFile: String?, slug: String?) throws -> DetectedProduct? {
        let fm = FileManager.default
        // WordPress scans PHP files directly inside the plugin directory for headers.
        let entries = try fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey], options: [])
        let phpFiles = entries.filter { url in
            let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            return url.pathExtension.lowercased() == "php" && values?.isRegularFile == true && values?.isSymbolicLink != true
        }.sorted { $0.lastPathComponent < $1.lastPathComponent }

        var candidates: [(file: String, headers: [String: String])] = []
        for file in phpFiles {
            let headers = try WordPressHeaders.read(file, headers: WordPressHeaders.pluginHeaders)
            if headers["Plugin Name"] != nil {
                candidates.append((file.lastPathComponent, headers))
            }
        }
        guard !candidates.isEmpty else { return nil }

        var problems: [String] = []
        let chosen: (file: String, headers: [String: String])
        if let mainPluginFile {
            guard let match = candidates.first(where: { $0.file == mainPluginFile }) else {
                problems.append("Configured main plugin file \"\(mainPluginFile)\" has no \"Plugin Name:\" header.")
                return DetectedProduct(type: .plugin, problems: problems)
            }
            chosen = match
        } else if candidates.count == 1 {
            chosen = candidates[0]
        } else if let slug, let match = candidates.first(where: { $0.file == "\(slug).php" }) {
            chosen = match
            problems.append("Multiple files declare \"Plugin Name:\" (\(candidates.map(\.file).joined(separator: ", "))); using \(match.file).")
        } else {
            problems.append("Multiple files declare \"Plugin Name:\" (\(candidates.map(\.file).joined(separator: ", "))). Set the main plugin file in the product settings.")
            return DetectedProduct(type: .plugin, problems: problems)
        }

        var readme: WordPressReadme?
        var readmeFile: String?
        if let url = Self.findReadme(in: root) {
            readme = try ReadmeParser.parse(fileAt: url)
            readmeFile = url.lastPathComponent
        }
        return DetectedProduct(
            type: .plugin,
            name: chosen.headers["Plugin Name"],
            version: chosen.headers["Version"],
            textDomain: chosen.headers["Text Domain"],
            mainFile: chosen.file,
            headers: chosen.headers,
            readme: readme,
            readmeFile: readmeFile,
            problems: problems
        )
    }

    // MARK: Theme

    func detectTheme(at root: URL) throws -> DetectedProduct? {
        let style = root.appendingPathComponent("style.css")
        let values = try? style.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values?.isRegularFile == true, values?.isSymbolicLink != true else { return nil }
        let headers = try WordPressHeaders.read(style, headers: WordPressHeaders.themeHeaders)
        guard headers["Theme Name"] != nil else { return nil }
        var readme: WordPressReadme?
        var readmeFile: String?
        if let url = Self.findReadme(in: root) {
            readme = try ReadmeParser.parse(fileAt: url)
            readmeFile = url.lastPathComponent
        }
        return DetectedProduct(
            type: .theme,
            name: headers["Theme Name"],
            version: headers["Version"],
            textDomain: headers["Text Domain"],
            mainFile: "style.css",
            headers: headers,
            readme: readme,
            readmeFile: readmeFile
        )
    }

    /// `readme.txt` (preferred) or `readme.md`, matched case-insensitively at the root.
    static func findReadme(in root: URL) -> URL? {
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: root.path) else { return nil }
        for wanted in ["readme.txt", "readme.md"] {
            if let name = names.first(where: { $0.lowercased() == wanted }) {
                let url = root.appendingPathComponent(name)
                let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                if values?.isRegularFile == true, values?.isSymbolicLink != true { return url }
            }
        }
        return nil
    }
}
