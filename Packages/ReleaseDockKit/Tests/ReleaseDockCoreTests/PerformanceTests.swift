import XCTest
@testable import ReleaseDockCore

/// Opt-in scale checks: RELEASEDOCK_PERF_TESTS=1 swift test --filter PerformanceTests
final class PerformanceTests: XCTestCase {
    func testLargeFolderAndZIPInspection() throws {
        guard ProcessInfo.processInfo.environment["RELEASEDOCK_PERF_TESTS"] == "1" else { throw XCTSkip("Set RELEASEDOCK_PERF_TESTS=1") }
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.0.0")
        for i in 0..<5000 {
            dir.write("sample-plugin/vendor/pkg\(i % 50)/src/File\(i).php", "<?php\n// file \(i)\n" + String(repeating: "x", count: 2000))
        }
        let clock = ContinuousClock()
        var start = clock.now
        let folder = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        print("folder inspection: \(folder.includedFiles.count) files in \(clock.now - start)")
        XCTAssertEqual(folder.includedFiles.count, 5004)

        let zip = dir.path("big.zip")
        try Fixtures.zip(folder: root, to: zip)
        start = clock.now
        let inspection = try ZIPInspectionService().inspect(zipAt: zip, options: InspectionOptions())
        print("zip inspection: \(inspection.includedFiles.count) files in \(clock.now - start)")
        start = clock.now
        let extracted = try ZIPInspectionService().extract(zipAt: zip, inspection: inspection, to: dir.path("out"))
        print("zip extraction: \(extracted.count) entries in \(clock.now - start)")
        start = clock.now
        let staged = try ReleaseStagingService(paths: AppPaths(root: dir.path("data"))).buildCandidate(from: root, entries: folder.includedEntries, releaseID: UUID())
        print("candidate build: \(staged.files.count) files in \(clock.now - start)")
    }
}
