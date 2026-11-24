import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import ReleaseDockCore

final class AssetRulesTests: XCTestCase {
    func writePNG(_ dir: TempDir, _ path: String, _ width: Int, _ height: Int) {
        let url = dir.path(path)
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, context.makeImage()!, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }

    func messages(_ dir: TempDir) throws -> [String] {
        AssetRules.check(try AssetInspector().inspect(folderAt: dir.url)).map(\.message)
    }

    func testValidAssetsPass() throws {
        let dir = TempDir()
        writePNG(dir, "banner-772x250.png", 772, 250)
        writePNG(dir, "banner-1544x500.png", 1544, 500)
        writePNG(dir, "banner-772x250-rtl.png", 772, 250)
        writePNG(dir, "icon-128x128.png", 128, 128)
        _ = dir.write("icon.svg", "<svg/>")
        writePNG(dir, "screenshot-1.png", 40, 30)
        _ = dir.write("screenshot-2-de_DE.jpg", "not decoded")
        _ = dir.write("blueprints/blueprint.json", #"{"steps":[{"step":"login"}]}"#)
        _ = dir.write(".DS_Store", "x")
        let report = try AssetInspector().inspect(folderAt: dir.url)
        XCTAssertEqual(report.files.first { $0.path == "banner-1544x500.png" }?.width, 1544)
        XCTAssertFalse(report.files.contains { $0.path == ".DS_Store" })
        XCTAssertEqual(try messages(dir), ["8 asset(s) follow WordPress.org names and sizes, blueprint.json is valid"])
    }

    func testWrongDimensionsAndUnknownNames() throws {
        let dir = TempDir()
        writePNG(dir, "banner-772x250.png", 800, 300)
        writePNG(dir, "Screenshot-1.png", 10, 10)
        writePNG(dir, "banner.png", 10, 10)
        _ = dir.write("icon-128x128.jpeg", "x")
        let found = try messages(dir)
        XCTAssertTrue(found.contains("Banner is 800×300 but its name says 772×250"), "\(found)")
        XCTAssertTrue(found.contains("Asset names must be lowercase; WordPress.org ignores Screenshot-1.png"))
        XCTAssertTrue(found.contains("WordPress.org does not use banner.png"))
        XCTAssertTrue(found.contains("WordPress.org does not use icon-128x128.jpeg"), "only .jpg is listed in the handbook")
    }

    func testRetinaBannerAloneAndSVGWithoutFallback() throws {
        let dir = TempDir()
        writePNG(dir, "banner-1544x500-es.png", 1544, 500)
        _ = dir.write("icon.svg", "<svg/>")
        let found = try messages(dir)
        XCTAssertEqual(found, [
            "The 1544×500 banner is only used together with a 772×250 banner",
            "icon.svg needs a PNG or JPG icon as a fallback",
        ])
    }

    func testOversizedIconAndInvalidBlueprint() throws {
        let dir = TempDir()
        _ = dir.write("icon-256x256.gif", String(repeating: "x", count: (1 << 20) + 1))
        _ = dir.write("blueprints/blueprint.json", "{ steps: [] }")
        let found = try messages(dir)
        XCTAssertTrue(found.contains("Icon is over 1 MB, the WordPress.org limit"), "\(found)")
        XCTAssertTrue(found.contains("blueprint.json is not valid JSON; Live Preview will not work"))
        let array = TempDir()
        _ = array.write("blueprints/blueprint.json", "[]")
        XCTAssertEqual(try messages(array), ["blueprint.json must contain a JSON object; Live Preview will not work"])
    }

    func testAssetWarningsDoNotBlockAndCanBeDisabled() throws {
        let assets = AssetsReport(files: [AssetFile(path: "banner.png", size: 1)])
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0")
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .plugin, slug: "sample-plugin")
        let repo = RepositorySnapshot(repositoryExists: true, currentVersion: "1.0.0", releasePathExists: false, authentication: .keychain)
        var product = makeProduct()
        var report = ValidationService().validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: repo, assets: assets))
        XCTAssertFalse(report.isBlocking)
        XCTAssertTrue(report.warnings.contains { $0.check == .directoryAssets })
        product.validation.disabledChecks = [.directoryAssets]
        report = ValidationService().validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: repo, assets: assets))
        XCTAssertFalse(report.results.contains { $0.check == .directoryAssets })
    }
}
