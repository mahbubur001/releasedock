import XCTest
@testable import ReleaseDockCore

final class CodeCheckParsingTests: XCTestCase {
    func testParsesLintOutputOnceDespiteDuplicates() {
        let output = """
        PHP Parse error:  syntax error, unexpected end of file, expecting "," or ";" in includes/bad.php on line 3

        Parse error: syntax error, unexpected end of file, expecting "," or ";" in includes/bad.php on line 3
        Errors parsing includes/bad.php
        Deprecated: Using ${var} in strings is deprecated in old.php on line 7
        No syntax errors detected in ok.php
        """
        let findings = CodeCheckService.parseLint(output)
        XCTAssertEqual(findings.count, 2, "\(findings)")
        XCTAssertEqual(findings[0].file, "includes/bad.php")
        XCTAssertEqual(findings[0].line, 3)
        XCTAssertEqual(findings[0].severity, .error)
        XCTAssertTrue(findings[0].message.hasPrefix("Parse error: syntax error"))
        XCTAssertEqual(findings[1].severity, .warning)
        XCTAssertEqual(findings[1].file, "old.php")
    }

    func testParsesPluginCheckStrictJSON() throws {
        let output = """
        Deprecated: Case statements followed by a semicolon (;) are deprecated in phar:///wp/vendor/a.php on line 369
        Notice: [plugin-check] something odd
        [{"file":"sample-plugin.php","line":12,"type":"ERROR","code":"WordPress.Security.EscapeOutput.OutputNotEscaped","message":"All output should be run through an escaping function"},{"file":"readme.txt","line":0,"type":"WARNING","code":"outdated_tested_upto_header","message":"Tested up to: 6.8 < 7.1."}]
        """
        let findings = try XCTUnwrap(CodeCheckService.parsePluginCheck(output))
        XCTAssertEqual(findings.count, 2)
        XCTAssertEqual(findings[0].severity, .error)
        XCTAssertEqual(findings[0].code, "WordPress.Security.EscapeOutput.OutputNotEscaped")
        XCTAssertEqual(findings[1].severity, .warning)
        XCTAssertEqual(CodeCheckService.parsePluginCheck("[]"), [])
        XCTAssertNil(CodeCheckService.parsePluginCheck("Error: This does not seem to be a WordPress installation."))
    }
}

final class CodeCheckServiceTests: XCTestCase {
    func service(wordpressPath: String? = nil) throws -> CodeCheckService {
        guard CodeCheckService.phpCandidates.contains(where: FileManager.default.isExecutableFile(atPath:)) else {
            throw XCTSkip("php not installed")
        }
        return CodeCheckService(configuration: CodeCheckConfigurationStore(CodeCheckConfiguration(pluginCheckWordPressPath: wordpressPath)))
    }

    func testLintsWithRealPHP() async throws {
        let service = try service()
        let dir = TempDir()
        dir.write("p/ok.php", "<?php\necho 'ok';\n")
        dir.write("p/includes/bad.php", "<?php\necho 'a'\n")
        let report = await service.check(root: dir.path("p"), phpFiles: ["ok.php", "includes/bad.php"], product: makeProduct(), requiresPHP: "7.4")
        XCTAssertNil(report.phpLintSkipped)
        XCTAssertNotNil(report.phpVersion)
        XCTAssertEqual(report.lintedFiles, 2)
        let errors = report.findings(.phpLint).filter { $0.severity == .error }
        XCTAssertEqual(errors.map(\.file), ["includes/bad.php"], "\(report.findings)")
        XCTAssertEqual(errors.first?.line, 3)
        XCTAssertEqual(report.pluginCheckSkipped?.contains("Settings"), true, "Plugin Check is opt-in")
    }

    func testSkipsLintWhenLocalPHPIsOlderThanRequired() async throws {
        let service = try service()
        let dir = TempDir()
        dir.write("p/ok.php", "<?php\n")
        let report = await service.check(root: dir.path("p"), phpFiles: ["ok.php"], product: makeProduct(), requiresPHP: "99.0")
        XCTAssertTrue(report.phpLintSkipped?.contains("older than Requires PHP 99.0") == true)
        XCTAssertEqual(report.lintedFiles, 0)
    }

    func testPluginCheckIsNotRunForThemes() async throws {
        let service = try service(wordpressPath: "/tmp/wordpress")
        let dir = TempDir()
        dir.write("t/functions.php", "<?php\n")
        var theme = makeProduct()
        theme.type = .theme
        let report = await service.check(root: dir.path("t"), phpFiles: ["functions.php"], product: theme, requiresPHP: nil)
        XCTAssertFalse(report.pluginCheckRan)
        XCTAssertNil(report.pluginCheckSkipped)
    }
}

final class CodeCheckValidationTests: XCTestCase {
    func validate(_ report: CodeCheckReport, disabled: Set<ValidationCheckID> = []) throws -> ValidationReport {
        let dir = TempDir()
        let root = Fixtures.makePlugin(in: dir, version: "1.1.0")
        var product = makeProduct()
        product.validation.disabledChecks = disabled
        let inspection = try FolderInspectionService().inspect(folderAt: root, options: InspectionOptions())
        let detected = try ProductDetector().detect(at: root, expectedType: .plugin, slug: "sample-plugin")
        let repo = RepositorySnapshot(repositoryExists: true, currentVersion: "1.0.0", releasePathExists: false, authentication: .keychain)
        return ValidationService().validate(ValidationContext(product: product, inspection: inspection, detected: detected, repository: repo, codeChecks: report))
    }

    func results(_ report: ValidationReport, _ check: ValidationCheckID) -> [ValidationResult] {
        report.results.filter { $0.check == check }
    }

    func testSyntaxErrorBlocks() throws {
        let report = try validate(CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 3, pluginCheckSkipped: "Not set up", findings: [
            CodeCheckFinding(tool: .phpLint, severity: .error, file: "includes/bad.php", line: 3, message: "Parse error: syntax error"),
        ]))
        XCTAssertTrue(report.isBlocking)
        let failure = try XCTUnwrap(results(report, .phpSyntax).first)
        XCTAssertEqual(failure.status, .failed)
        XCTAssertEqual(failure.filePath, "includes/bad.php")
        XCTAssertEqual(failure.message, "Line 3: Parse error: syntax error")
        XCTAssertEqual(results(report, .pluginCheck).map(\.status), [.skipped])
    }

    func testDisabledSyntaxCheckDoesNotBlock() throws {
        let report = try validate(CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 3, findings: [
            CodeCheckFinding(tool: .phpLint, severity: .error, file: "a.php", line: 1, message: "Parse error"),
        ]), disabled: [.phpSyntax])
        XCTAssertFalse(report.isBlocking)
        XCTAssertTrue(results(report, .phpSyntax).isEmpty)
    }

    func testCleanLintPassesAndSkippedLintDoesNotBlock() throws {
        let clean = try validate(CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 4))
        XCTAssertEqual(results(clean, .phpSyntax).map(\.message), ["4 PHP file(s) parse with PHP 8.5.7"])
        let skipped = try validate(CodeCheckReport(phpLintSkipped: "PHP is not installed"))
        XCTAssertEqual(results(skipped, .phpSyntax).map(\.status), [.skipped])
        XCTAssertFalse(skipped.isBlocking, "an optional check that could not run never blocks")
    }

    func testPluginCheckFindingsWarnWithoutBlocking() throws {
        let findings = (1...20).map { i in
            CodeCheckFinding(tool: .pluginCheck, severity: i == 20 ? .error : .warning, file: "f\(i).php", line: i, code: "code_\(i)", message: "Finding \(i)")
        }
        let report = try validate(CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 1, pluginCheckRan: true, findings: findings))
        XCTAssertFalse(report.isBlocking)
        let pcp = results(report, .pluginCheck)
        XCTAssertEqual(pcp.first?.message, "Plugin Check: 1 error(s), 19 warning(s). WordPress.org runs these checks on every release.")
        XCTAssertEqual(pcp.count, 1 + ValidationService.listedFindings)
        XCTAssertEqual(pcp[1].message, "Line 20: [code_20] Finding 20", "errors are listed first")
        let none = try validate(CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 1, pluginCheckRan: true))
        XCTAssertEqual(results(none, .pluginCheck).map(\.status), [.passed])
    }
}

/// Stub so pipeline tests do not depend on a local PHP.
struct StubCodeChecker: CodeChecking {
    var report: CodeCheckReport
    func check(root: URL, phpFiles: [String], product: Product, requiresPHP: String?) async -> CodeCheckReport { report }
}

final class CodeCheckPipelineTests: XCTestCase {
    func testSyntaxErrorStopsBeforeTheWorkingCopyIsTouched() async throws {
        guard LocalSVNRepository.isAvailable else { throw XCTSkip("svn/svnadmin not installed") }
        let repo = try LocalSVNRepository()
        let work = TempDir()
        let store = try Store.inMemory()
        let paths = AppPaths(root: work.path("data"))
        try paths.createAll()
        let svn = SVNService(commandTimeout: .seconds(60))
        let failing = CodeCheckReport(phpVersion: "8.5.7", lintedFiles: 3, findings: [
            CodeCheckFinding(tool: .phpLint, severity: .error, file: "includes/class-sample.php", line: 2, message: "Parse error: syntax error"),
        ])
        let pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store, releases: store, svn: svn, git: StubGit(), keychain: InMemoryKeychain(),
            staging: ReleaseStagingService(paths: paths), codeChecker: StubCodeChecker(report: failing), generator: "tests"
        ))
        try repo.createPlugin(slug: "sample-plugin", initialVersion: "1.0.0")
        let product = Product(
            name: "Sample Plugin", slug: "sample-plugin", type: .plugin,
            svnRepositoryURL: URL(string: "\(repo.url)/sample-plugin")!,
            workingCopyPath: work.path("wc-plugin").path, currentVersion: "1.0.0"
        )
        try store.save(product)
        let src = TempDir()
        let plugin = Fixtures.makePlugin(in: src, version: "1.1.0")

        var record = try await pipeline.createRelease(productID: product.id, source: .folder(path: plugin.path))
        record = try await pipeline.prepare(record.id)
        XCTAssertEqual(record.state, .failed)
        XCTAssertFalse(record.workingCopyModified, "nothing was applied")
        XCTAssertEqual(record.codeChecks, failing)
        XCTAssertTrue(record.validation?.blockingResults.contains { $0.check == .phpSyntax } == true)
        let localChangeCount = try await WorkingCopyOperations.localChanges(at: product.workingCopyURL, svn: svn).count
        XCTAssertEqual(localChangeCount, 0)
        XCTAssertNil(record.preview)
    }
}
