import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import ReleaseDockCore

final class ThemeChecksTests: XCTestCase {
    func writePNG(_ url: URL, _ width: Int, _ height: Int) {
        let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, context.makeImage()!, nil)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
    }

    func warnings(name: String = "Lumen", screenshot: (Int, Int)? = (1200, 900), extra: [String: String] = [:], description: String? = nil) throws -> [String] {
        let dir = TempDir()
        let root = Fixtures.makeTheme(in: dir, version: "1.1.0", name: name)
        if let screenshot { writePNG(root.appendingPathComponent("screenshot.png"), screenshot.0, screenshot.1) }
        if let description {
            dir.write("sample-theme/style.css", Fixtures.themeStyle(name: name, version: "1.1.0").replacingOccurrences(of: "A sample theme.", with: description))
        }
        for (path, contents) in extra { dir.write("sample-theme/\(path)", contents) }
        var product = makeProduct()
        product.type = .theme
        product.expectedHeaderName = name
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .theme)
        let report = ValidationService().validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: nil))
        return report.warnings.map(\.message)
    }

    func testCorrectScreenshotAndNameHaveNoThemeWarnings() throws {
        let found = try warnings()
        XCTAssertFalse(found.contains { $0.contains("screenshot") || $0.contains("Theme names") || $0.contains("Spell") || $0.contains("Resources") }, "\(found)")
    }

    func testScreenshotSizeAndRatio() throws {
        XCTAssertTrue(try warnings(screenshot: (1600, 1200)).contains("screenshot.png is 1600×1200; theme review allows at most 1200×900"))
        let wide = try warnings(screenshot: (1200, 600))
        XCTAssertTrue(wide.contains("screenshot.png is 1200×600, not 4:3 as theme review requires"), "\(wide)")
        XCTAssertFalse(wide.contains { $0.contains("at most") })
    }

    func testDetectorReadsScreenshotSize() throws {
        let dir = TempDir()
        let root = Fixtures.makeTheme(in: dir, version: "1.0.0")
        writePNG(root.appendingPathComponent("screenshot.png"), 880, 660)
        let detected = try ProductDetector().detect(at: root)
        XCTAssertEqual(detected.screenshotFile, "screenshot.png")
        XCTAssertEqual(detected.screenshotSize, PixelSize(width: 880, height: 660))
    }

    func testReservedNamesAndSpelling() throws {
        for name in ["Sample Theme", "WordPress Starter", "Twentyish"] {
            XCTAssertTrue(try warnings(name: name).contains { $0.contains("Theme names must not use") }, name)
        }
        XCTAssertFalse(try warnings(name: "Themely").contains { $0.contains("Theme names must not use") }, "only whole words")
        XCTAssertTrue(try warnings(description: "A clean Wordpress blog theme.").contains { $0.hasPrefix("Spell \"WordPress\"") })
        XCTAssertFalse(try warnings(description: "Works on wordpress.org and WordPress.").contains { $0.hasPrefix("Spell") })
    }

    func testBundledResourcesNeedAResourcesSection() throws {
        let missing = try warnings(extra: ["assets/images/hero.jpg": "img", "assets/fonts/inter.woff2": "font"])
        XCTAssertTrue(missing.contains { $0.contains("bundles 2 image or font file(s)") }, "\(missing)")
    }
}
