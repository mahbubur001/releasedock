import Foundation
import ImageIO

/// One file in a plugin's directory-assets folder (synced to SVN `/assets`).
public struct AssetFile: Codable, Hashable, Sendable {
    public var path: String
    public var size: UInt64
    public var width: Int?
    public var height: Int?

    public init(path: String, size: UInt64, width: Int? = nil, height: Int? = nil) {
        self.path = path
        self.size = size
        self.width = width
        self.height = height
    }
}

public struct AssetsReport: Codable, Hashable, Sendable {
    public var files: [AssetFile]
    /// Why `blueprints/blueprint.json` cannot be used; nil when absent or valid.
    public var blueprintProblem: String?

    public init(files: [AssetFile], blueprintProblem: String? = nil) {
        self.files = files
        self.blueprintProblem = blueprintProblem
    }
}

/// Reads names, sizes and pixel dimensions of the assets folder. Hidden files are
/// skipped, as the sync excludes them.
public struct AssetInspector: Sendable {
    public init() {}

    public func inspect(folderAt root: URL) throws -> AssetsReport {
        let fm = FileManager.default
        let base = root.standardizedFileURL.path + "/"
        var files: [AssetFile] = []
        var blueprintProblem: String?
        guard let enumerator = fm.enumerator(at: root, includingPropertiesForKeys: [.isRegularFileKey, .fileSizeKey], options: [.skipsHiddenFiles]) else {
            return AssetsReport(files: [])
        }
        while let url = enumerator.nextObject() as? URL {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
            guard values.isRegularFile == true else { continue }
            let full = url.standardizedFileURL.path
            guard full.hasPrefix(base) else { continue }
            let path = String(full.dropFirst(base.count))
            var file = AssetFile(path: path, size: UInt64(values.fileSize ?? 0))
            if ["png", "jpg", "jpeg", "gif"].contains(url.pathExtension.lowercased()),
               let source = CGImageSourceCreateWithURL(url as CFURL, nil),
               let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] {
                file.width = properties[kCGImagePropertyPixelWidth] as? Int
                file.height = properties[kCGImagePropertyPixelHeight] as? Int
            }
            if path == "blueprints/blueprint.json" {
                blueprintProblem = Self.blueprintProblem(at: url)
            }
            files.append(file)
        }
        return AssetsReport(files: files.sorted { $0.path < $1.path }, blueprintProblem: blueprintProblem)
    }

    /// WordPress.org reads the blueprint with `json_decode`; it must be a JSON object.
    static func blueprintProblem(at url: URL) -> String? {
        guard let data = try? Data(contentsOf: url) else { return "blueprint.json cannot be read" }
        do {
            guard try JSONSerialization.jsonObject(with: data) is [String: Any] else {
                return "blueprint.json must contain a JSON object"
            }
            return nil
        } catch {
            return "blueprint.json is not valid JSON"
        }
    }
}

/// WordPress.org plugin directory asset rules
/// (developer.wordpress.org/plugins/wordpress-org/plugin-assets/).
enum AssetRules {
    static let bannerLimit: UInt64 = 4 << 20
    static let iconLimit: UInt64 = 1 << 20
    static let screenshotLimit: UInt64 = 10 << 20

    static let suffix = #"(?:-(?:rtl|[a-z]{2,3}(?:_[A-Z]{2})?))?"#
    static let bannerPattern = #"^banner-(772x250|1544x500)("# + suffix + #")\.(?:png|jpg)$"#
    static let iconPattern = #"^icon-(128x128|256x256)\.(?:png|jpg|gif)$"#
    static let screenshotPattern = #"^screenshot-[1-9][0-9]*"# + suffix + #"\.(?:png|jpg)$"#

    static func check(_ report: AssetsReport) -> [ValidationResult] {
        var results: [ValidationResult] = []
        let names = Set(report.files.map(\.path))
        for file in report.files {
            let name = file.path
            if name == "blueprints/blueprint.json" { continue }
            if let expected = match(bannerPattern, name) {
                results += checkImage(file, expected: expected.size, limit: bannerLimit, kind: "Banner")
                if expected.size == "1544x500", !names.contains(where: { $0.hasPrefix("banner-772x250\(expected.suffix).") }) {
                    results.append(.warn(.directoryAssets, "The 1544×500 banner is only used together with a 772×250 banner", file: name, fix: "Add banner-772x250\(expected.suffix).png or .jpg."))
                }
            } else if let expected = match(iconPattern, name) {
                results += checkImage(file, expected: expected.size, limit: iconLimit, kind: "Icon")
            } else if name == "icon.svg" {
                if file.size > iconLimit { results.append(.warn(.directoryAssets, "Icon is over 1 MB", file: name)) }
            } else if name.range(of: screenshotPattern, options: .regularExpression) != nil {
                if file.size > screenshotLimit { results.append(.warn(.directoryAssets, "Screenshot is over 10 MB, the WordPress.org limit", file: name)) }
            } else if name != name.lowercased(), name.lowercased().range(of: "^(banner|icon|screenshot)-", options: .regularExpression) != nil {
                results.append(.warn(.directoryAssets, "Asset names must be lowercase; WordPress.org ignores \(name)", file: name, fix: "Rename it to \(name.lowercased())."))
            } else {
                results.append(.warn(.directoryAssets, "WordPress.org does not use \(name)", file: name, fix: "Use banner-772x250, banner-1544x500, icon-128x128, icon-256x256, icon.svg or screenshot-N names (.png/.jpg)."))
            }
        }
        if names.contains("icon.svg"), !names.contains(where: { $0.range(of: iconPattern, options: .regularExpression) != nil }) {
            results.append(.warn(.directoryAssets, "icon.svg needs a PNG or JPG icon as a fallback", file: "icon.svg", fix: "Add icon-128x128.png (and icon-256x256.png)."))
        }
        if let problem = report.blueprintProblem {
            results.append(.warn(.directoryAssets, "\(problem); Live Preview will not work", file: "blueprints/blueprint.json"))
        }
        if results.isEmpty {
            let blueprint = names.contains("blueprints/blueprint.json") ? ", blueprint.json is valid" : ""
            results.append(.pass(.directoryAssets, "\(report.files.count) asset(s) follow WordPress.org names and sizes\(blueprint)"))
        }
        return results
    }

    private static func match(_ pattern: String, _ name: String) -> (size: String, suffix: String)? {
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let m = regex.firstMatch(in: name, range: NSRange(name.startIndex..., in: name)),
              let size = Range(m.range(at: 1), in: name)
        else { return nil }
        let suffix = m.numberOfRanges > 2 ? Range(m.range(at: 2), in: name).map { String(name[$0]) } ?? "" : ""
        return (String(name[size]), suffix)
    }

    private static func checkImage(_ file: AssetFile, expected: String, limit: UInt64, kind: String) -> [ValidationResult] {
        var results: [ValidationResult] = []
        let parts = expected.split(separator: "x").compactMap { Int($0) }
        if let width = file.width, let height = file.height, parts.count == 2, width != parts[0] || height != parts[1] {
            results.append(.warn(.directoryAssets, "\(kind) is \(width)×\(height) but its name says \(parts[0])×\(parts[1])", file: file.path, fix: "Export it at exactly \(parts[0])×\(parts[1]) pixels."))
        }
        if file.size > limit {
            results.append(.warn(.directoryAssets, "\(kind) is over \(limit >> 20) MB, the WordPress.org limit", file: file.path))
        }
        return results
    }
}
