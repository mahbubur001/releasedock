import XCTest
@testable import ReleaseDockCore

/// Opt-in, read-only checks against WordPress.org and a local checkout:
/// RELEASEDOCK_LIVE_SLUG=tlp-team RELEASEDOCK_LIVE_WC=~/svn/tlp-team swift test --filter LiveRepositoryTests
final class LiveRepositoryTests: XCTestCase {
    func testDescribeRealRepositoryAndCheckout() async throws {
        let env = ProcessInfo.processInfo.environment
        guard let slug = env["RELEASEDOCK_LIVE_SLUG"] else { throw XCTSkip("Set RELEASEDOCK_LIVE_SLUG") }
        let service = RepositorySetupService(svn: SVNService())
        let clock = ContinuousClock()

        var start = clock.now
        let remote = try await service.describeRemote(URL(string: "https://plugins.svn.wordpress.org/\(slug)")!)
        print("LIVE remote: \(clock.now - start) name=\(remote.productName ?? "nil") version=\(remote.currentVersion ?? "nil") tags=\(remote.versions.count) notes=\(remote.notes)")
        XCTAssertNotNil(remote.productName)
        XCTAssertNotNil(remote.currentVersion)

        if let wc = env["RELEASEDOCK_LIVE_WC"] {
            start = clock.now
            let local = try await service.describeWorkingCopyLocally(at: URL(fileURLWithPath: (wc as NSString).expandingTildeInPath))
            print("LIVE local: \(clock.now - start) name=\(local.repository.productName ?? "nil") version=\(local.repository.currentVersion ?? "nil") changes=\(local.localChanges.count)")
            XCTAssertLessThan(clock.now - start, .seconds(3))
        }
    }
}
