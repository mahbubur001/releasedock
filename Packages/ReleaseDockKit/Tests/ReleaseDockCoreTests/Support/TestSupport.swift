import Foundation
import XCTest
@testable import ReleaseDockCore

/// Temporary directory removed at deinit.
final class TempDir {
    let url: URL

    init() {
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("ReleaseDockTests-\(UUID().uuidString)", isDirectory: true)
            .resolvingSymlinksInPath()
        try! FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: url)
    }

    func path(_ relative: String) -> URL { url.appendingPathComponent(relative) }

    @discardableResult
    func write(_ relative: String, _ contents: String) -> URL {
        let file = path(relative)
        try! FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try! Data(contents.utf8).write(to: file)
        return file
    }

    func mkdir(_ relative: String) {
        try! FileManager.default.createDirectory(at: path(relative), withIntermediateDirectories: true)
    }
}

enum Fixtures {
    static func pluginMainFile(name: String = "Sample Plugin", version: String, textDomain: String = "sample-plugin") -> String {
        """
        <?php
        /**
         * Plugin Name: \(name)
         * Description: A sample plugin.
         * Version: \(version)
         * Requires at least: 6.0
         * Requires PHP: 7.4
         * Author: Test Author
         * License: GPL-2.0-or-later
         * Text Domain: \(textDomain)
         */
        defined( 'ABSPATH' ) || exit;
        """
    }

    static func pluginReadme(name: String = "Sample Plugin", stableTag: String, version: String? = nil) -> String {
        """
        === \(name) ===
        Contributors: testauthor
        Tags: testing, release
        Requires at least: 6.0
        Tested up to: 6.8
        Requires PHP: 7.4
        Stable tag: \(stableTag)
        License: GPLv2 or later
        License URI: https://www.gnu.org/licenses/gpl-2.0.html

        A sample plugin.

        == Description ==

        Does sample things.

        == Changelog ==

        = \(version ?? stableTag) =
        * Release.

        = 0.9.0 =
        * Earlier.
        """
    }

    /// Writes a plugin into `dir/relative` and returns its root.
    @discardableResult
    static func makePlugin(in dir: TempDir, at relative: String = "sample-plugin", version: String, stableTag: String? = nil, name: String = "Sample Plugin", extraFiles: [String: String] = [:]) -> URL {
        dir.write("\(relative)/sample-plugin.php", pluginMainFile(name: name, version: version))
        dir.write("\(relative)/readme.txt", pluginReadme(name: name, stableTag: stableTag ?? version, version: version))
        dir.write("\(relative)/includes/class-sample.php", "<?php\nclass Sample {}\n")
        dir.write("\(relative)/assets/js/app.js", "console.log('v\(version)');\n")
        for (path, contents) in extraFiles { dir.write("\(relative)/\(path)", contents) }
        return dir.path(relative)
    }

    static func themeStyle(name: String = "Sample Theme", version: String) -> String {
        """
        /*
        Theme Name: \(name)
        Author: Test Author
        Description: A sample theme.
        Version: \(version)
        Requires at least: 6.0
        Tested up to: 6.8
        Requires PHP: 7.4
        License: GNU General Public License v2 or later
        License URI: http://www.gnu.org/licenses/gpl-2.0.html
        Text Domain: sample-theme
        Tags: blog, one-column
        */
        """
    }

    @discardableResult
    static func makeTheme(in dir: TempDir, at relative: String = "sample-theme", version: String, name: String = "Sample Theme") -> URL {
        dir.write("\(relative)/style.css", themeStyle(name: name, version: version))
        dir.write("\(relative)/index.php", "<?php get_header(); ?>\n")
        dir.write("\(relative)/functions.php", "<?php // v\(version)\n")
        dir.write("\(relative)/screenshot.png", "PNG-DATA")
        dir.write("\(relative)/readme.txt", """
        === \(name) ===
        Contributors: testauthor
        Requires at least: 6.0
        Tested up to: 6.8
        Requires PHP: 7.4
        License: GPLv2 or later
        License URI: http://www.gnu.org/licenses/gpl-2.0.html

        == Changelog ==

        = \(version) =
        * Release.
        """)
        return dir.path(relative)
    }

    /// Zips `folder` (including the folder itself as the archive root) with /usr/bin/zip.
    static func zip(folder: URL, to destination: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        process.currentDirectoryURL = folder.deletingLastPathComponent()
        process.arguments = ["-q", "-r", "-y", destination.path, folder.lastPathComponent]
        let done = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in done.signal() }
        try process.run()
        done.wait()
        XCTAssertEqual(process.terminationStatus, 0)
    }
}

/// Builds ZIP archives byte by byte (stored entries only) so tests can produce
/// malicious structures that normal tools refuse to create.
struct TestZIPBuilder {
    struct Entry {
        var name: String
        var data: Data
        var unixMode: UInt32 = 0o100644
        var flags: UInt16 = 0x0800
        var localName: String?
    }

    var entries: [Entry] = []

    mutating func add(_ name: String, _ contents: String, mode: UInt32 = 0o100644) {
        entries.append(Entry(name: name, data: Data(contents.utf8), unixMode: mode))
    }

    mutating func addDirectory(_ name: String) {
        entries.append(Entry(name: name.hasSuffix("/") ? name : name + "/", data: Data(), unixMode: 0o040755))
    }

    mutating func addSymlink(_ name: String, target: String) {
        entries.append(Entry(name: name, data: Data(target.utf8), unixMode: 0o120777))
    }

    func build() -> Data {
        var out = Data()
        var central = Data()
        func le16(_ v: UInt16) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
        func le32(_ v: UInt32) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
        for entry in entries {
            let offset = UInt32(out.count)
            let name = Data(entry.name.utf8)
            let localName = Data((entry.localName ?? entry.name).utf8)
            let crc = CRC32.checksum(entry.data)
            out += le32(0x0403_4B50) + le16(20) + le16(entry.flags) + le16(0) + le16(0) + le16(0)
            out += le32(crc) + le32(UInt32(entry.data.count)) + le32(UInt32(entry.data.count))
            out += le16(UInt16(localName.count)) + le16(0) + localName + entry.data
            central += le32(0x0201_4B50) + le16(0x031E) + le16(20) + le16(entry.flags) + le16(0) + le16(0) + le16(0)
            central += le32(crc) + le32(UInt32(entry.data.count)) + le32(UInt32(entry.data.count))
            central += le16(UInt16(name.count)) + le16(0) + le16(0) + le16(0) + le16(0)
            central += le32(entry.unixMode << 16) + le32(offset) + name
        }
        let cdOffset = UInt32(out.count)
        out += central
        out += le32(0x0605_4B50) + le16(0) + le16(0) + le16(UInt16(entries.count)) + le16(UInt16(entries.count))
        out += le32(UInt32(central.count)) + le32(cdOffset) + le16(0)
        return out
    }

    func write(to url: URL) throws {
        try build().write(to: url)
    }
}

/// A local Subversion repository laid out like WordPress.org, using svnadmin and file:// URLs.
final class LocalSVNRepository {
    let dir = TempDir()
    let root: URL
    let svn = URL(fileURLWithPath: SVNService.candidatePaths.first { FileManager.default.isExecutableFile(atPath: $0) } ?? "/opt/homebrew/bin/svn")

    static var isAvailable: Bool {
        SVNService.candidatePaths.contains { FileManager.default.isExecutableFile(atPath: $0) }
            && ["/opt/homebrew/bin/svnadmin", "/usr/local/bin/svnadmin", "/opt/local/bin/svnadmin", "/usr/bin/svnadmin"].contains { FileManager.default.isExecutableFile(atPath: $0) }
    }

    init() throws {
        root = dir.path("repo")
        try Self.create(root)
    }

    /// A repository at `root` that outlives the test (demo data).
    init(persistentAt root: URL) throws {
        self.root = root
        try FileManager.default.createDirectory(at: root.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Self.create(root)
    }

    private static func create(_ root: URL) throws {
        let admin = ["/opt/homebrew/bin/svnadmin", "/usr/local/bin/svnadmin", "/opt/local/bin/svnadmin", "/usr/bin/svnadmin"].first { FileManager.default.isExecutableFile(atPath: $0) }!
        try run(URL(fileURLWithPath: admin), ["create", root.path])
    }

    var url: String { "file://" + root.path }

    @discardableResult
    func run(_ args: [String], cwd: URL? = nil) throws -> String {
        try Self.run(svn, args + ["--non-interactive"], cwd: cwd)
    }

    @discardableResult
    static func run(_ exe: URL, _ args: [String], cwd: URL? = nil) throws -> String {
        let p = Process()
        p.executableURL = exe
        p.arguments = args
        if let cwd { p.currentDirectoryURL = cwd }
        p.environment = ProcessRequest.defaultEnvironment()
        let out = Pipe(), err = Pipe()
        p.standardOutput = out
        p.standardError = err
        let done = DispatchSemaphore(value: 0)
        p.terminationHandler = { _ in done.signal() }
        try p.run()
        var e = Data()
        let errReader = Thread { e = err.fileHandleForReading.readDataToEndOfFile() }
        errReader.start()
        let o = out.fileHandleForReading.readDataToEndOfFile()
        done.wait()
        while !errReader.isFinished { usleep(1000) }
        guard p.terminationStatus == 0 else {
            throw NSError(domain: "svn", code: Int(p.terminationStatus), userInfo: [NSLocalizedDescriptionKey: String(decoding: e, as: UTF8.self)])
        }
        return String(decoding: o, as: UTF8.self)
    }

    /// Creates `/<slug>/{trunk,tags,assets}` and optionally publishes an initial version.
    func createPlugin(slug: String, initialVersion: String?, name: String = "Sample Plugin") throws {
        try run(["mkdir", "--parents", "-m", "Create \(slug)", "\(url)/\(slug)/trunk", "\(url)/\(slug)/tags", "\(url)/\(slug)/assets"])
        guard let initialVersion else { return }
        let src = TempDir()
        let plugin = Fixtures.makePlugin(in: src, at: "p", version: initialVersion, name: name)
        let wc = dir.path("seed-\(UUID().uuidString)")
        try run(["checkout", "\(url)/\(slug)", wc.path])
        let trunk = wc.appendingPathComponent("trunk")
        for item in try FileManager.default.contentsOfDirectory(atPath: plugin.path) {
            try FileManager.default.copyItem(at: plugin.appendingPathComponent(item), to: trunk.appendingPathComponent(item))
        }
        try run(["add", "--force", trunk.path])
        try run(["copy", trunk.path, wc.appendingPathComponent("tags/\(initialVersion)").path])
        try run(["commit", "-m", "Initial \(initialVersion)", wc.path])
    }

    func createTheme(slug: String, initialVersion: String?, name: String = "Sample Theme") throws {
        try run(["mkdir", "--parents", "-m", "Create \(slug)", "\(url)/\(slug)"])
        guard let initialVersion else { return }
        let src = TempDir()
        let theme = Fixtures.makeTheme(in: src, at: "t", version: initialVersion, name: name)
        try run(["import", "-m", "Initial \(initialVersion)", theme.path, "\(url)/\(slug)/\(initialVersion)"])
    }
}

/// Wraps a real `SVNServicing` and injects failures into selected operations.
final class FaultInjectingSVN: SVNServicing, @unchecked Sendable {
    enum CommitFault {
        case none
        /// Commit really happens, then the client reports an uncertain outcome (e.g. timeout after the server committed).
        case uncertainAfterCommit
        /// The client times out before anything reaches the server.
        case uncertainWithoutCommit
        case authenticationFailure
        case networkFailureBeforeCommit
        case hookRejection
    }

    let base: SVNServicing
    var commitFault: CommitFault = .none
    var failInfoWithNetworkError = false
    var failLogWithNetworkError = false
    var cachedCredentials = true
    private(set) var commitCount = 0

    init(base: SVNServicing) {
        self.base = base
    }

    func detectInstallation() async throws -> SVNInstallation { try await base.detectInstallation() }
    func info(_ target: String) async throws -> SVNInfo {
        if failInfoWithNetworkError {
            throw SVNError.commandFailed(kind: .network, codes: ["E170013"], message: "Unable to connect", command: "svn info")
        }
        return try await base.info(target)
    }
    func exists(_ target: String) async throws -> Bool {
        if failInfoWithNetworkError { _ = try await info(target) }
        return try await base.exists(target)
    }
    func list(_ url: String, recursive: Bool) async throws -> [SVNListEntry] { try await base.list(url, recursive: recursive) }
    func cat(_ url: String) async throws -> Data { try await base.cat(url) }
    func checkout(_ url: String, to path: URL, depth: SVNDepth) async throws { try await base.checkout(url, to: path, depth: depth) }
    func update(_ path: URL, setDepth: SVNDepth?) async throws -> Int { try await base.update(path, setDepth: setDepth) }
    func status(_ path: URL, showUpdates: Bool) async throws -> [SVNStatusEntry] { try await base.status(path, showUpdates: showUpdates) }
    func diff(_ path: URL) async throws -> String { try await base.diff(path) }
    func add(_ paths: [URL]) async throws { try await base.add(paths) }
    func delete(_ paths: [URL]) async throws { try await base.delete(paths) }
    func copy(_ source: URL, to destination: URL) async throws { try await base.copy(source, to: destination) }
    func revert(_ path: URL) async throws { try await base.revert(path) }
    func cleanup(_ path: URL) async throws { try await base.cleanup(path) }
    func resolve(_ paths: [URL], accept: SVNConflictChoice) async throws { try await base.resolve(paths, accept: accept) }
    func setProperty(_ name: String, value: String, paths: [URL]) async throws { try await base.setProperty(name, value: value, paths: paths) }
    func log(_ url: String, limit: Int) async throws -> [SVNLogEntry] {
        if failLogWithNetworkError {
            throw SVNError.commandFailed(kind: .network, codes: ["E170013"], message: "Unable to connect", command: "svn log")
        }
        return try await base.log(url, limit: limit)
    }
    func export(_ url: String, to path: URL) async throws { try await base.export(url, to: path) }
    func hasCachedCredentials(forRepository url: URL) async throws -> Bool { cachedCredentials }

    func commit(_ paths: [URL], message: String, credentials: SVNCredentials?) async throws -> Int {
        commitCount += 1
        switch commitFault {
        case .none:
            return try await base.commit(paths, message: message, credentials: credentials)
        case .uncertainAfterCommit:
            _ = try await base.commit(paths, message: message, credentials: credentials)
            throw SVNError.commitOutcomeUncertain("simulated timeout after the server committed")
        case .uncertainWithoutCommit:
            throw SVNError.commitOutcomeUncertain("simulated timeout before reaching the server")
        case .authenticationFailure:
            throw SVNError.commandFailed(kind: .authenticationFailed, codes: ["E170001"], message: "Authorization failed", command: "svn commit")
        case .networkFailureBeforeCommit:
            throw SVNError.commitOutcomeUncertain("Unable to connect")
        case .hookRejection:
            throw SVNError.commandFailed(kind: .hookRejected, codes: ["E165001"], message: "Commit blocked by pre-commit hook", command: "svn commit")
        }
    }
}

final class RecordingNotifications: NotificationServicing, @unchecked Sendable {
    private let lock = NSLock()
    private var _titles: [String] = []
    var titles: [String] { lock.withLock { _titles } }
    func requestAuthorization() async -> Bool { true }
    func notify(title: String, body: String, identifier: String) async { lock.withLock { _titles.append(title) } }
}

struct StubGit: GitServicing {
    var state: GitRepositoryState = .notARepository
    func isAvailable() async -> Bool { true }
    func state(of directory: URL) async throws -> GitRepositoryState { state }
}
