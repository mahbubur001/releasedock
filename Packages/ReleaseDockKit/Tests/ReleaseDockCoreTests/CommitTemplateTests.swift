import XCTest
@testable import ReleaseDockCore

final class CommitTemplateTests: XCTestCase {
    func testRendersTokens() {
        XCTAssertEqual(
            CommitMessageTemplate.render("  Tagging {slug} {version} (was {previous}) ", name: "Sample", slug: "sample-plugin", version: "1.1.0", previous: "1.0.0"),
            "Tagging sample-plugin 1.1.0 (was 1.0.0)"
        )
        XCTAssertEqual(CommitMessageTemplate.render("Release {version}", name: "S", slug: "s", version: nil, previous: nil), "Release")
    }

    func testRejectsEmptyLongAndUnknownTokens() {
        XCTAssertNil(CommitMessageTemplate.problem("Release {name} {version}"))
        XCTAssertNotNil(CommitMessageTemplate.problem("   "))
        XCTAssertNotNil(CommitMessageTemplate.problem(String(repeating: "x", count: 301)))
        XCTAssertEqual(CommitMessageTemplate.problem("Release {versoin}"), "{versoin} is not a token. Use {name}, {slug}, {version}, {previous}.")
    }
}

extension PipelineIntegrationTests {
    func testTemplateBecomesTheSummaryAndKeepsTheMarker() async throws {
        let product = try registerPlugin(assets: false)
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"), commitTemplate: "Tagging version {version} (from {previous})")
        record = try await pipeline.prepare(record.id)
        let message = try XCTUnwrap(record.preview?.commitMessage)
        XCTAssertTrue(message.hasPrefix("Tagging version 1.1.0 (from 1.0.0)\n\n"), message)
        XCTAssertTrue(message.contains(record.commitMarker), "recovery still finds the commit")
    }

    func testInvalidTemplateIsRefusedBeforeAnythingRuns() async throws {
        let product = try registerPlugin(assets: false)
        do {
            _ = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"), commitTemplate: "Release {ver}")
            XCTFail("expected a refusal")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("{ver} is not a token"), error.localizedDescription)
        }
    }
}
