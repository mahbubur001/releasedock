import XCTest
@testable import ReleaseDockCore

final class TranscriptTests: XCTestCase {
    private var folder: URL!

    override func setUpWithError() throws {
        folder = FileManager.default.temporaryDirectory.appendingPathComponent("transcript-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: folder)
    }

    private func service(_ transcript: SVNTranscript) -> SVNService {
        SVNService(runner: RecordingRunner(), executable: URL(fileURLWithPath: "/usr/bin/true"), transcript: transcript)
    }

    func testRecordsScopedCommandsWithoutThePassword() async throws {
        let transcript = SVNTranscript(folder: folder)
        let svn = service(transcript)
        let product = UUID(), release = UUID()
        let wc = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("svn/my plugin")
        try await SVNTranscript.$scope.withValue(TranscriptScope(productID: product, releaseID: release, operation: "Publish")) {
            _ = try await svn.commit([wc], message: "Release 1.2.0", credentials: SVNCredentials(username: "alice", password: "s3cret-pass"))
        }

        let entries = transcript.entries(productID: product, releaseID: release, limit: 10)
        XCTAssertEqual(entries.count, 1)
        let entry = try XCTUnwrap(entries.first)
        XCTAssertEqual(entry.operation, "Publish")
        XCTAssertTrue(entry.succeeded)
        XCTAssertTrue(entry.command.hasPrefix("svn commit"), entry.command)
        XCTAssertTrue(entry.command.contains("\"~/svn/my plugin\""), "home folder shortened and quoted: \(entry.command)")
        XCTAssertTrue(entry.command.contains("--username alice --password-from-stdin"), entry.command)
        XCTAssertFalse(entry.command.contains("--non-interactive"), "global options are left out")
        XCTAssertEqual(entry.output, "Committed revision 42.")
        let file = try String(contentsOf: folder.appendingPathComponent("\(product.uuidString).jsonl"), encoding: .utf8)
        XCTAssertFalse(file.contains("s3cret-pass"), "the password never reaches the transcript")
    }

    func testUnscopedCommandsAreNotRecorded() async throws {
        let transcript = SVNTranscript(folder: folder)
        _ = try await service(transcript).commit([URL(fileURLWithPath: "/tmp/wc")], message: "x", credentials: nil)
        XCTAssertFalse(FileManager.default.fileExists(atPath: folder.path))
    }

    func testDataOutputIsSummarisedAndLongOutputTrimmed() {
        let scope = TranscriptScope(productID: UUID(), operation: "Prepare")
        let xml = ProcessResult(exitCode: 0, standardOutput: Data(String(repeating: "<entry/>", count: 200).utf8), standardError: Data(), duration: .zero, displayCommand: "")
        XCTAssertEqual(SVNTranscript.entry(scope: scope, arguments: ["status", "--xml", "/wc"], workingDirectory: nil, result: xml, failure: nil, duration: 0.2).output, "(2 KB of output)")

        let lines = (1...30).map { "A    trunk/file\($0).php" }.joined(separator: "\n") + "\n"
        let update = ProcessResult(exitCode: 1, standardOutput: Data(lines.utf8), standardError: Data("svn: E170013: Unable to connect\n".utf8), duration: .zero, displayCommand: "")
        let entry = SVNTranscript.entry(scope: scope, arguments: ["update", "/wc"], workingDirectory: nil, result: update, failure: nil, duration: 1)
        XCTAssertFalse(entry.succeeded)
        XCTAssertTrue(entry.output.hasPrefix("… 18 more lines\nA    trunk/file19.php"), entry.output)
        XCTAssertTrue(entry.output.hasSuffix("svn: E170013: Unable to connect"), entry.output)
    }

    func testKeepsTheNewestEntriesAndFiltersByRelease() {
        let transcript = SVNTranscript(folder: folder, maximumEntries: 10)
        let product = UUID(), release = UUID()
        for index in 0..<400 {
            let scope = TranscriptScope(productID: product, releaseID: index.isMultiple(of: 2) ? release : nil, operation: "Op \(index)")
            transcript.record(SVNTranscript.entry(scope: scope, arguments: ["info", String(repeating: "x", count: 900)], workingDirectory: nil, result: nil, failure: "stopped", duration: 0))
        }
        let all = transcript.entries(productID: product, releaseID: nil, limit: 1000)
        // The file is checked once it passes 256 KB (about 230 of these entries), then cut to the newest 10.
        XCTAssertLessThan(all.count, 400, "old entries were dropped")
        XCTAssertEqual(all.last?.operation, "Op 399")
        XCTAssertTrue(transcript.entries(productID: product, releaseID: release, limit: 1000).allSatisfy { $0.releaseID == release })
    }
}

extension PipelineIntegrationTests {
    func testReleaseRecordsItsSVNCommandsPerOperation() async throws {
        let product = try registerPlugin(assets: false)
        var record = try await pipeline.createRelease(productID: product.id, source: try zipSource(version: "1.1.0"))
        record = try await pipeline.prepare(record.id)
        record = try await pipeline.approve(record.id, kind: .user)
        record = try await pipeline.publish(record.id)
        XCTAssertEqual(record.state, .completed, "\(record.errors)")

        let entries = transcript.entries(productID: product.id, releaseID: record.id, limit: 1000)
        XCTAssertTrue(entries.contains { $0.operation == "Prepare" && $0.command.hasPrefix("svn status") })
        let commit = try XCTUnwrap(entries.first { $0.command.hasPrefix("svn commit") })
        XCTAssertEqual(commit.operation, "Publish")
        XCTAssertTrue(commit.succeeded)
        XCTAssertTrue(commit.output.contains("Committed revision"), commit.output)
        XCTAssertTrue(entries.allSatisfy { $0.releaseID == record.id })
    }
}
