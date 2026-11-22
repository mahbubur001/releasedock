import Foundation

public struct PipelineEvent: Sendable, Hashable {
    public var releaseID: UUID
    public var state: ReleaseState
    public var message: String
    public var at: Date
}

public struct PipelineDependencies: Sendable {
    public var products: ProductRepository
    public var releases: ReleaseRepository
    public var svn: SVNServicing
    public var git: GitServicing
    public var keychain: KeychainServicing
    public var zipInspector: ZIPInspecting
    public var folderInspector: FolderInspecting
    public var detector: ProductDetector
    public var validator: ValidationServicing
    public var staging: ReleaseStaging
    public var strategies: StrategyProvider
    public var notifications: NotificationServicing
    /// Recorded in manifests, e.g. "ReleaseDock 1.0.0 (42)".
    public var generator: String

    public init(
        products: ProductRepository,
        releases: ReleaseRepository,
        svn: SVNServicing,
        git: GitServicing,
        keychain: KeychainServicing,
        zipInspector: ZIPInspecting = ZIPInspectionService(),
        folderInspector: FolderInspecting = FolderInspectionService(),
        detector: ProductDetector = ProductDetector(),
        validator: ValidationServicing = ValidationService(),
        staging: ReleaseStaging,
        strategies: StrategyProvider = StrategyProvider(),
        notifications: NotificationServicing = NoopNotificationService(),
        generator: String = "ReleaseDock"
    ) {
        self.products = products
        self.releases = releases
        self.svn = svn
        self.git = git
        self.keychain = keychain
        self.zipInspector = zipInspector
        self.folderInspector = folderInspector
        self.detector = detector
        self.validator = validator
        self.staging = staging
        self.strategies = strategies
        self.notifications = notifications
        self.generator = generator
    }
}

/// Orchestrates a release through the state machine. Every transition is persisted
/// before the next side effect, so a crash leaves a record that says exactly how far
/// the release got. Once a commit may have reached the server, the outcome is only
/// ever established by inspecting the repository — never by retrying.
public actor ReleasePipelineService {
    private let deps: PipelineDependencies
    private var busyProducts: Set<UUID> = []
    private var observers: [UUID: AsyncStream<PipelineEvent>.Continuation] = [:]

    public init(dependencies: PipelineDependencies) {
        deps = dependencies
    }

    // MARK: Events

    public func events() -> AsyncStream<PipelineEvent> {
        let id = UUID()
        let (stream, continuation) = AsyncStream<PipelineEvent>.makeStream(bufferingPolicy: .bufferingNewest(100))
        observers[id] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeObserver(id) }
        }
        return stream
    }

    private func removeObserver(_ id: UUID) {
        observers[id] = nil
    }

    private func emit(_ record: ReleaseRecord, _ message: String) {
        let event = PipelineEvent(releaseID: record.id, state: record.state, message: message, at: Date())
        for observer in observers.values { observer.yield(event) }
        Log.release.info("[\(record.productSlug, privacy: .public) \(record.version ?? "?", privacy: .public)] \(record.state.rawValue, privacy: .public): \(message, privacy: .public)")
    }

    // MARK: Helpers

    private func load(_ id: UUID) throws -> ReleaseRecord {
        guard let record = try deps.releases.release(id: id) else { throw ReleaseError.releaseNotFound }
        return record
    }

    private func product(_ id: UUID) throws -> Product {
        guard let product = try deps.products.product(id: id) else { throw ReleaseError.productNotFound }
        return product
    }

    private func move(_ record: inout ReleaseRecord, to state: ReleaseState, _ note: String? = nil) throws {
        // Guard against a stale in-memory copy overwriting a state another operation persisted.
        if let stored = try deps.releases.release(id: record.id), stored.state != record.state {
            throw ReleaseError.invalidState("The release changed to \(stored.state.displayName) in the meantime.")
        }
        // A new approval or publish attempt invalidates any earlier "verified not committed" result.
        if state == .approved || state == .publishing { record.verifiedNotCommitted = false }
        try ReleaseStateMachine.transition(&record, to: state, note: note)
        try deps.releases.save(record)
        emit(record, note ?? state.displayName)
    }

    private func withProductLock<T: Sendable>(_ productID: UUID, _ body: () async throws -> T) async throws -> T {
        guard !busyProducts.contains(productID) else {
            throw ReleaseError.invalidState("Another operation is already running for this product.")
        }
        busyProducts.insert(productID)
        defer { busyProducts.remove(productID) }
        return try await body()
    }

    func credentials(for product: Product) throws -> SVNCredentials? {
        guard let username = product.svnUsername, !username.isEmpty, let host = product.svnRepositoryURL.host else { return nil }
        guard let password = try deps.keychain.password(for: .svn(host: host, username: username)) else { return nil }
        return SVNCredentials(username: username, password: password)
    }

    func authenticationAvailability(for product: Product) async -> AuthenticationAvailability {
        if (try? credentials(for: product)) != nil { return .keychain }
        if product.svnRepositoryURL.isFileURL { return .cachedBySVN } // local repositories need no credentials
        if (try? await deps.svn.hasCachedCredentials(forRepository: product.svnRepositoryURL)) == true { return .cachedBySVN }
        return .none
    }

    // MARK: Create

    /// Registers a new release attempt for a ZIP or folder source.
    public func createRelease(productID: UUID, source: ReleaseSource, mode: ReleaseMode? = nil) throws -> ReleaseRecord {
        let product = try product(productID)
        let record = ReleaseRecord(product: product, source: source, mode: mode ?? product.releaseMode)
        try deps.releases.save(record)
        emit(record, "Release detected from \(source.kind == .zip ? "ZIP" : "folder") \(URL(fileURLWithPath: source.path).lastPathComponent)")
        return record
    }

    // MARK: Prepare (Detected → Previewed)

    /// Inspects, validates, stages and previews the release. A dry run completes here.
    /// Validation failures end the release in `failed` with the report attached.
    public func prepare(_ id: UUID) async throws -> ReleaseRecord {
        var record = try load(id)
        guard record.state == .detected else { throw ReleaseError.invalidState("Release is \(record.state.displayName); preparation already ran.") }
        let product = try product(record.productID)
        return try await withProductLock(product.id) {
            do {
                return try await runPreparation(&record, product: product)
            } catch {
                try? await fail(&record, product: product, error: error)
                throw error
            }
        }
    }

    private func runPreparation(_ record: inout ReleaseRecord, product: Product) async throws -> ReleaseRecord {
        let strategy = deps.strategies.strategy(for: product.type)
        let options = InspectionOptions(excludePatterns: product.validation.excludePatterns)

        // 1. Inspect (never modifies the source).
        emit(record, "Inspecting package")
        let inspection: PackageInspection
        var packageRoot: URL?
        var entries: [PackageEntry] = []
        var gitState: GitRepositoryState?
        switch record.source {
        case .zip(let path):
            let url = URL(fileURLWithPath: path)
            inspection = try deps.zipInspector.inspect(zipAt: url, options: options)
            if inspection.blockingIssues.isEmpty {
                let extracted = try deps.staging.extract(zip: url, inspection: inspection, releaseID: record.id)
                packageRoot = extracted.root
                entries = extracted.entries
            }
        case .folder(let path):
            let url = URL(fileURLWithPath: path, isDirectory: true)
            inspection = try deps.folderInspector.inspect(folderAt: url, options: options)
            packageRoot = url
            entries = inspection.includedEntries
            if await deps.git.isAvailable() {
                let git = try? await deps.git.state(of: url)
                record.gitCommit = git?.headCommit
                gitState = git
            }
        }
        record.inspection = inspection
        record.sourceSHA256 = inspection.sha256
        record.stagingPath = deps.staging is ReleaseStagingService ? (try? deps.staging.workspace(for: record.id).path) : nil

        var detectionError: String?
        if let packageRoot {
            do {
                record.detected = try deps.detector.detect(at: packageRoot, expectedType: product.type, mainPluginFile: product.mainPluginFile, slug: product.slug)
                record.version = record.detected?.version
            } catch {
                detectionError = error.localizedDescription
            }
        } else {
            detectionError = "The package has blocking structural problems and was not extracted."
        }
        try move(&record, to: .inspected, "Inspected \(inspection.includedFiles.count) files; version \(record.version ?? "unknown")")

        // 2. Validate against the repository.
        emit(record, "Checking the SVN repository")
        let snapshot = await gatherSnapshot(product: product, version: record.version, strategy: strategy)
        record.repository = snapshot
        record.previousVersion = snapshot.currentVersion ?? product.currentVersion
        let context = ValidationContext(product: product, inspection: inspection, detected: record.detected, detectionError: detectionError, repository: snapshot, git: gitState)
        let report = deps.validator.validate(context)
        record.validation = report
        if report.isBlocking {
            try move(&record, to: .failed, "Validation failed: \(report.blockingResults.count) blocking result(s)")
            deps.staging.removeWorkspace(for: record.id)
            return record
        }
        try move(&record, to: .validated, "All required checks passed (\(report.warnings.count) warning(s))")

        // 3. Stage a clean candidate and apply it to the working copy.
        guard let packageRoot, let version = record.version else { throw ReleaseError.missingData("package root or version") }
        emit(record, "Preparing the clean release candidate")
        let candidate = try deps.staging.buildCandidate(from: packageRoot, entries: entries, releaseID: record.id)
        let recheck = try deps.detector.detect(at: candidate.rootURL, expectedType: product.type, mainPluginFile: product.mainPluginFile, slug: product.slug)
        guard recheck.version == version, recheck.name == record.detected?.name else {
            throw ReleaseError.candidateChanged("version or name differs in the staged copy")
        }
        record.candidate = candidate
        emit(record, "Applying the candidate to the SVN working copy")
        record.workingCopyModified = true
        try deps.releases.save(record)
        try await strategy.apply(candidate: candidate, version: version, currentVersion: snapshot.currentVersion, product: product, svn: deps.svn)
        try writeManifest(&record, strategy: strategy)
        try move(&record, to: .staged, "Staged \(candidate.files.count) files into \(strategy.releasePath(version: version))")

        // 4. Preview.
        let preview = try await buildPreview(record: record, product: product, strategy: strategy)
        record.preview = preview
        var withPreview = context
        withPreview.preview = preview
        let finalReport = deps.validator.validate(withPreview)
        record.validation = finalReport
        if finalReport.isBlocking {
            throw ReleaseError.validationBlocked(finalReport.blockingResults.count)
        }
        try move(&record, to: .previewed, "Preview: \(preview.added) added, \(preview.modified) modified, \(preview.deleted) deleted")

        if record.mode == .dryRun {
            try await resetWorkingCopy(product: product, backupName: nil)
            record.workingCopyModified = false
            try move(&record, to: .completed, "Dry run complete; nothing was committed")
            try writeManifest(&record, strategy: strategy)
            await deps.notifications.notify(title: "Dry run complete", body: "\(product.name) \(version) is ready to publish.", identifier: record.id.uuidString)
        }
        return record
    }

    /// Collects repository facts; failures become part of the snapshot instead of throwing,
    /// so validation can report them precisely.
    func gatherSnapshot(product: Product, version: String?, strategy: ReleaseStrategy) async -> RepositorySnapshot {
        var snapshot: RepositorySnapshot
        do {
            let installation = try await deps.svn.detectInstallation()
            snapshot = try await strategy.inspectRemote(product: product, candidateVersion: version, svn: deps.svn)
            snapshot.svnVersion = installation.version
        } catch {
            return RepositorySnapshot(connectivityError: error.localizedDescription, authentication: await authenticationAvailability(for: product))
        }
        snapshot.authentication = await authenticationAvailability(for: product)
        guard snapshot.repositoryExists == true, snapshot.structureProblems.isEmpty else { return snapshot }

        let wc = product.workingCopyURL
        do {
            snapshot.workingCopyExists = FileManager.default.fileExists(atPath: wc.appendingPathComponent(".svn").path)
            if FileManager.default.fileExists(atPath: wc.path) {
                snapshot.workingCopyURLMismatch = try await WorkingCopyOperations.workingCopyMismatch(at: wc, expected: product.svnRepositoryURL, svn: deps.svn)
            }
            if snapshot.workingCopyURLMismatch != nil { return snapshot }
            if snapshot.workingCopyExists {
                snapshot.workingCopyChanges = try await WorkingCopyOperations.localChanges(at: wc, svn: deps.svn)
                // Never update over local changes: that could merge into or conflict with them.
                if !snapshot.workingCopyChanges.isEmpty { return snapshot }
            }
            try await strategy.prepareWorkingCopy(product: product, currentVersion: snapshot.currentVersion, svn: deps.svn)
            snapshot.workingCopyExists = true
            let status = try await deps.svn.status(wc, showUpdates: true)
            snapshot.outOfDatePaths = status.filter(\.isOutOfDate).map(\.path)
            snapshot.workingCopyRevision = try await deps.svn.info(wc.path).revision
        } catch {
            snapshot.connectivityError = "Working copy: \(error.localizedDescription)"
        }
        return snapshot
    }

    func commitMessage(record: ReleaseRecord) -> String {
        "Release \(record.productName) \(record.version ?? "")\n\n\(record.commitMarker)\n"
    }

    /// Lists the working-copy changes and fingerprints them together with the content
    /// of every changed file, the candidate digest and the commit message.
    func buildPreview(record: ReleaseRecord, product: Product, strategy: ReleaseStrategy) async throws -> ReleasePreview {
        let wc = product.workingCopyURL
        let root = wc.standardizedFileURL.path
        let status = try await deps.svn.status(wc, showUpdates: false)
        let releasePath = strategy.releasePath(version: record.version ?? "")
        var changes: [PreviewChange] = []
        var fingerprintLines: [String] = []
        for entry in status where entry.isLocalChange || entry.item == .unversioned {
            let relative = entry.path.hasPrefix(root + "/") ? String(entry.path.dropFirst(root.count + 1)) : entry.path
            var isDir: ObjCBool = false
            let exists = FileManager.default.fileExists(atPath: entry.path, isDirectory: &isDir)
            let kind: ChangeKind = switch entry.item {
            case .added: .added
            case .deleted, .missing: .deleted
            case .replaced: .replaced
            case .modified: .modified
            case .unversioned: .added
            default: entry.props == .modified ? .propertyChanged : .modified
            }
            changes.append(PreviewChange(path: relative, kind: kind, isDirectory: isDir.boolValue, isCopy: entry.copied && relative == releasePath))
            var line = "\(entry.item.rawValue)|\(entry.props.rawValue)|\(entry.copied)|\(relative)"
            if exists, !isDir.boolValue { line += "|" + ((try? Hashing.sha256(ofFile: URL(fileURLWithPath: entry.path))) ?? "unreadable") }
            fingerprintLines.append(line)
        }
        var diff = (try? await deps.svn.diff(wc)) ?? ""
        let limit = 200_000
        let truncated = diff.count > limit
        if truncated { diff = String(diff.prefix(limit)) }
        let message = commitMessage(record: record)
        let fingerprint = Hashing.sha256(fingerprintLines.sorted().joined(separator: "\n") + "\n" + (record.candidate?.treeDigest ?? "") + "\n" + message)
        let revision = try? await deps.svn.info(wc.path).revision
        return ReleasePreview(changes: changes.sorted { $0.path < $1.path }, diffExcerpt: diff, diffTruncated: truncated, commitMessage: message, fingerprint: fingerprint, workingCopyRevision: revision)
    }

    // MARK: Approve

    /// Records approval of the current preview. Automatic approval passes only when
    /// every `AutomationGate` condition holds.
    public func approve(_ id: UUID, kind: ApprovalKind) async throws -> ReleaseRecord {
        let product = try product(try load(id).productID)
        return try await withProductLock(product.id) {
            var record = try load(id)
            guard record.mode != .dryRun else { throw ReleaseStateError.dryRunCannotPublish }
            switch record.state {
            case .previewed:
                break
            case .recoveryRequired where record.verifiedNotCommitted && kind == .user:
                break
            default:
                throw ReleaseError.invalidState("Only a previewed release can be approved (current state: \(record.state.displayName)).")
            }
            guard let preview = record.preview else { throw ReleaseError.missingData("preview") }
            if let report = record.validation, report.isBlocking { throw ReleaseError.validationBlocked(report.blockingResults.count) }

            let strategy = deps.strategies.strategy(for: product.type)
            let current = try await buildPreview(record: record, product: product, strategy: strategy).fingerprint
            if kind == .automatic {
                let reasons = AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: current)
                guard reasons.isEmpty else { throw ReleaseError.automationNotAuthorized(reasons) }
            }
            guard current == preview.fingerprint else { throw ReleaseError.previewChanged }
            record.approval = Approval(kind: kind, previewFingerprint: preview.fingerprint)
            try move(&record, to: .approved, kind == .automatic ? "Automatically authorized" : "Approved by user")
            return record
        }
    }

    /// Re-runs validation with a new preview so a changed working copy is never approved
    /// against a stale report.
    private func revalidate(_ record: inout ReleaseRecord, product: Product, preview: ReleasePreview) {
        guard let inspection = record.inspection else { return }
        let context = ValidationContext(product: product, inspection: inspection, detected: record.detected, repository: record.repository, preview: preview)
        record.validation = deps.validator.validate(context)
    }

    // MARK: Publish (Approved → Completed)

    public func publish(_ id: UUID) async throws -> ReleaseRecord {
        var record = try load(id)
        let product = try product(record.productID)
        guard record.state == .approved, let approval = record.approval else {
            throw ReleaseError.invalidState("The release must be approved before publishing.")
        }
        guard let version = record.version else { throw ReleaseError.missingData("version") }
        let strategy = deps.strategies.strategy(for: product.type)

        return try await withProductLock(product.id) {
            // Pre-flight: the approved preview must be exactly what is about to be committed.
            emit(record, "Re-checking the working copy and repository")
            let current = try await buildPreview(record: record, product: product, strategy: strategy)
            guard current.fingerprint == approval.previewFingerprint else {
                record.approval = nil
                record.preview = current
                revalidate(&record, product: product, preview: current)
                if let report = record.validation, report.isBlocking {
                    try await failAndReset(&record, product: product, reason: "The working copy changed after approval and no longer passes validation")
                    throw ReleaseError.validationBlocked(report.blockingResults.count)
                }
                try move(&record, to: .previewed, "Preview changed after approval; review and approve again")
                throw ReleaseError.previewChanged
            }
            let base = product.svnRepositoryURL.absoluteString.trimmingTrailingSlash
            let releaseURL = "\(base)/\(strategy.releasePath(version: version))"
            // Check for an earlier landed attempt first: once it landed, the working copy is
            // necessarily out of date, and that must lead to recovery, not failure.
            let releaseExists = try await deps.svn.exists(releaseURL)
            if !releaseExists {
                let status = try await deps.svn.status(product.workingCopyURL, showUpdates: true)
                let outOfDate = status.filter(\.isOutOfDate)
                if !outOfDate.isEmpty {
                    try await failAndReset(&record, product: product, reason: "Repository changed on the server (\(outOfDate.count) path(s)); prepare the release again")
                    throw ReleaseError.remoteChanged("\(outOfDate.count) path(s) are out of date")
                }
            }
            if releaseExists {
                // Idempotency: an earlier attempt of this very release may have landed.
                if try await findCommittedRevision(record: record, product: product) != nil {
                    try move(&record, to: .publishing, "Release already present on the server")
                    return try await recoverAfterUncertainCommit(&record, product: product, strategy: strategy)
                }
                try await failAndReset(&record, product: product, reason: "\(strategy.releasePath(version: version)) was created on the server by someone else")
                throw ReleaseError.releasePathExists(strategy.releasePath(version: version))
            }

            let credentials = try credentials(for: product)
            if credentials == nil {
                let availability = await authenticationAvailability(for: product)
                guard availability == .cachedBySVN else { throw ReleaseError.credentialsRequired }
            }
            record.revisionBefore = try await deps.svn.info(base).lastChangedRevision

            // Persist "publishing" before the commit so a crash is recoverable.
            try move(&record, to: .publishing, "Committing to \(base)")
            if let backup = try? await deps.svn.diff(product.workingCopyURL), !backup.isEmpty {
                _ = try? deps.staging.writeBackup(backup, name: "\(product.slug)-\(version)-\(record.id.uuidString.prefix(8))-committed.diff")
            }
            do {
                let revision = try await deps.svn.commit([product.workingCopyURL], message: current.commitMessage, credentials: credentials)
                record.committedRevision = revision
                try move(&record, to: .verifying, "Committed revision \(revision)")
            } catch SVNError.commitOutcomeUncertain(let detail) {
                record.errors.append("Commit outcome uncertain: \(detail)")
                try move(&record, to: .recoveryRequired, "Commit outcome uncertain: \(detail)")
                return try await recoverAfterUncertainCommit(&record, product: product, strategy: strategy)
            } catch let error as SVNError where error.kind == .authenticationFailed || error.kind == .authenticationRequired {
                // Rejected before anything was committed; the staged working copy is intact.
                record.approval = nil
                try move(&record, to: .previewed, "SVN rejected the credentials; nothing was committed")
                throw error
            } catch {
                try await failAndReset(&record, product: product, reason: "Commit rejected: \(error.localizedDescription)")
                throw error
            }
            return try await verifyAndComplete(&record, product: product, strategy: strategy)
        }
    }

    /// Convenience for automatic mode: approve automatically, then publish.
    public func publishAutomatically(_ id: UUID) async throws -> ReleaseRecord {
        _ = try await approve(id, kind: .automatic)
        return try await publish(id)
    }

    private func verifyAndComplete(_ record: inout ReleaseRecord, product: Product, strategy: ReleaseStrategy) async throws -> ReleaseRecord {
        guard let version = record.version, let candidate = record.candidate else { throw ReleaseError.missingData("candidate") }
        emit(record, "Verifying the release on the server")
        let report: VerificationReport
        do {
            let scratch = try deps.staging.workspace(for: record.id)
            report = try await strategy.verify(candidate: candidate, version: version, product: product, svn: deps.svn, scratch: scratch)
        } catch {
            // Leave the release recoverable ("Inspect Repository") rather than stuck in Verifying.
            record.errors.append("Verification failed: \(SecretRedactor.redact(error.localizedDescription))")
            if record.state == .verifying { try move(&record, to: .recoveryRequired, "Verification could not complete: \(error.localizedDescription)") }
            throw error
        }
        record.verification = report
        guard report.tagExists else {
            try move(&record, to: .recoveryRequired, "The commit was reported but \(report.tagURL) is missing")
            return record
        }
        if record.committedRevision == nil { record.committedRevision = report.committedRevision }

        // Bring the working copy to the committed state.
        do {
            try await deps.svn.update(product.workingCopyURL, setDepth: nil)
        } catch {
            record.verification?.notes.append("Working copy update after commit failed: \(error.localizedDescription)")
        }
        record.workingCopyModified = false
        record.guidance = strategy.postReleaseGuidance(product: product, version: version)
        let summary = report.isClean ? "Verified" : "Verified with \(report.missingFiles.count + report.unexpectedFiles.count + report.checksumMismatches.count) difference(s)"
        try move(&record, to: .completed, "\(summary); revision \(record.committedRevision.map(String.init) ?? "?")")

        var updated = (try? deps.products.product(id: product.id)) ?? product
        updated.currentVersion = version
        updated.lastSuccessfulRelease = ReleaseSummary(releaseID: record.id, version: version, revision: record.committedRevision, date: Date())
        updated.updatedAt = Date()
        try deps.products.save(updated)
        try writeManifest(&record, strategy: strategy)
        deps.staging.removeWorkspace(for: record.id)
        await deps.notifications.notify(
            title: "\(product.name) \(version) released",
            body: "Committed revision \(record.committedRevision.map(String.init) ?? "?")." + (report.isClean ? "" : " Verification found differences; review the report."),
            identifier: record.id.uuidString
        )
        return record
    }

    // MARK: Recovery

    /// Searches the product's recent history for this release's commit marker.
    func findCommittedRevision(record: ReleaseRecord, product: Product) async throws -> Int? {
        let log = try await deps.svn.log(product.svnRepositoryURL.absoluteString.trimmingTrailingSlash, limit: 30)
        return log.first { $0.message.contains(record.commitMarker) }?.revision
    }

    /// Establishes what happened to an uncertain commit by inspecting the repository.
    public func recover(_ id: UUID) async throws -> ReleaseRecord {
        var record = try load(id)
        guard record.state == .recoveryRequired else { throw ReleaseError.invalidState("Release does not require recovery.") }
        let product = try product(record.productID)
        let strategy = deps.strategies.strategy(for: product.type)
        return try await withProductLock(product.id) {
            try await recoverAfterUncertainCommit(&record, product: product, strategy: strategy)
        }
    }

    private func recoverAfterUncertainCommit(_ record: inout ReleaseRecord, product: Product, strategy: ReleaseStrategy) async throws -> ReleaseRecord {
        guard let version = record.version else { throw ReleaseError.missingData("version") }
        emit(record, "Inspecting the repository to determine the commit outcome")
        let wc = product.workingCopyURL
        _ = try? await deps.svn.cleanup(wc)

        if let revision = try await findCommittedRevision(record: record, product: product) {
            record.committedRevision = revision
            if record.state != .verifying {
                try move(&record, to: .verifying, "Found this release's commit in revision \(revision)")
            }
            // The server has the commit; discard the local scheduled copy and update to it.
            try await resetWorkingCopy(product: product, backupName: nil)
            return try await verifyAndComplete(&record, product: product, strategy: strategy)
        }

        let releaseURL = "\(product.svnRepositoryURL.absoluteString.trimmingTrailingSlash)/\(strategy.releasePath(version: version))"
        if try await deps.svn.exists(releaseURL) {
            record.errors.append("\(releaseURL) exists but was not created by this release")
            if record.state == .publishing { try move(&record, to: .recoveryRequired, "Release path exists without this release's marker") }
            try await failAndReset(&record, product: product, reason: "\(strategy.releasePath(version: version)) exists but was not created by this release")
            return record
        }

        record.verifiedNotCommitted = true
        if record.state == .publishing {
            try move(&record, to: .recoveryRequired, "Verified: the commit is not in the repository")
        } else {
            record.transitions.append(StateTransition(from: .recoveryRequired, to: .recoveryRequired, note: "Verified: the commit is not in the repository"))
            try deps.releases.save(record)
            emit(record, "Verified: the commit is not in the repository")
        }
        return record
    }

    // MARK: Cancel / abandon

    /// Cancels a release that has not started publishing, restoring the working copy.
    public func cancel(_ id: UUID) async throws -> ReleaseRecord {
        var record = try load(id)
        let product = try product(record.productID)
        guard !record.state.mayHaveCommitted, !record.state.isTerminal else {
            throw ReleaseError.invalidState("A release cannot be cancelled once publishing started; use recovery.")
        }
        return try await withProductLock(product.id) {
            if record.workingCopyModified {
                try await resetWorkingCopy(product: product, backupName: "\(product.slug)-\(record.id.uuidString.prefix(8))-cancelled.diff")
                record.workingCopyModified = false
            }
            try move(&record, to: .cancelled, "Cancelled by user")
            deps.staging.removeWorkspace(for: record.id)
            return record
        }
    }

    /// Ends a recovery whose commit was verified absent.
    public func abandon(_ id: UUID) async throws -> ReleaseRecord {
        var record = try load(id)
        let product = try product(record.productID)
        guard record.state == .recoveryRequired, record.verifiedNotCommitted else {
            throw ReleaseError.invalidState("Only a release verified as not committed can be abandoned.")
        }
        return try await withProductLock(product.id) {
            try await failAndReset(&record, product: product, reason: "Abandoned after verifying nothing was committed")
            return record
        }
    }

    /// Marks interrupted releases after an app restart: anything that may have committed
    /// requires recovery; everything earlier is cancelled (nothing left the machine).
    public func reconcileInterruptedReleases() async throws -> [ReleaseRecord] {
        var changed: [ReleaseRecord] = []
        for var record in try deps.releases.activeReleases() {
            guard let target = ReleaseStateMachine.stateAfterInterruption(record.state) else { continue }
            if target == .cancelled, record.workingCopyModified, let product = try? product(record.productID) {
                _ = try? await resetWorkingCopy(product: product, backupName: "\(product.slug)-\(record.id.uuidString.prefix(8))-interrupted.diff")
                record.workingCopyModified = false
            }
            try move(&record, to: target, "Interrupted by an app restart")
            changed.append(record)
        }
        return changed
    }

    // MARK: Working copy maintenance

    /// Explicit user action for a dirty working copy: backs up the diff and unversioned
    /// files, then reverts. Returns the backup location.
    public func revertWorkingCopy(productID: UUID) async throws -> URL? {
        let product = try product(productID)
        return try await withProductLock(product.id) {
            try await resetWorkingCopy(product: product, backupName: "\(product.slug)-\(ISO8601DateFormatter().string(from: Date()).replacingOccurrences(of: ":", with: "-"))-revert.diff")
        }
    }

    /// Reverts all local changes under the working copy and updates it.
    ///
    /// With `backupName`, the status listing and diff are written to `Backups/` and every
    /// added, modified, replaced, conflicted, unversioned and ignored path is copied there
    /// first; if any copy fails nothing is reverted. Afterwards, files that were scheduled
    /// for addition are removed, and pre-existing unversioned/ignored files are removed
    /// only when they were backed up.
    @discardableResult
    private func resetWorkingCopy(product: Product, backupName: String?) async throws -> URL? {
        let wc = product.workingCopyURL
        let fm = FileManager.default
        guard fm.fileExists(atPath: wc.appendingPathComponent(".svn").path) else { return nil }
        let root = wc.path.hasSuffix("/") ? wc.path : wc.path + "/"
        let status = try await deps.svn.status(wc, showUpdates: false)
        let added = Set(status.filter { $0.item == .added || $0.item == .replaced }.map(\.path))
        let untracked = status.filter { $0.item == .unversioned || $0.item == .ignored }.map(\.path)

        var backupURL: URL?
        if let backupName {
            let diff = (try? await deps.svn.diff(wc)) ?? ""
            let listing = status.map { "\($0.item.rawValue) \($0.props.rawValue) \($0.path)" }.joined(separator: "\n")
            let url = try deps.staging.writeBackup("# svn status\n\(listing)\n\n# svn diff (text files only; full copies are in the folder next to this file)\n\(diff)", name: backupName)
            backupURL = url
            let folder = url.deletingPathExtension().appendingPathExtension("files")
            let preserve: Set<SVNItemStatus> = [.added, .modified, .replaced, .conflicted, .merged, .obstructed, .unversioned, .ignored]
            var copied: [String] = []
            for entry in status.filter({ preserve.contains($0.item) || $0.props == .modified }).sorted(by: { $0.path < $1.path }) {
                guard fm.fileExists(atPath: entry.path) else { continue }
                guard entry.path.hasPrefix(root) else { throw ReleaseError.invalidState("Unexpected path outside the working copy: \(entry.path)") }
                if copied.contains(where: { entry.path.hasPrefix($0 + "/") }) { continue }
                let destination = folder.appendingPathComponent(String(entry.path.dropFirst(root.count)))
                // No try?: a failed backup must stop the revert.
                try fm.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
                try fm.copyItem(atPath: entry.path, toPath: destination.path)
                copied.append(entry.path)
            }
        }

        try await deps.svn.revert(wc)
        let removable = added.union(backupName != nil ? Set(untracked) : [])
        for path in removable.sorted(by: { $0.count > $1.count }) {
            guard fm.fileExists(atPath: path), path.hasPrefix(root), PathSafety.isContained(URL(fileURLWithPath: path), in: wc) else { continue }
            try fm.removeItem(atPath: path)
        }
        _ = try? await deps.svn.update(wc, setDepth: nil)
        return backupURL
    }

    private func failAndReset(_ record: inout ReleaseRecord, product: Product, reason: String) async throws {
        if record.workingCopyModified || record.state.mayHaveCommitted {
            _ = try? await resetWorkingCopy(product: product, backupName: "\(product.slug)-\(record.id.uuidString.prefix(8))-failed.diff")
            record.workingCopyModified = false
        }
        record.errors.append(SecretRedactor.redact(reason))
        try move(&record, to: .failed, reason)
        deps.staging.removeWorkspace(for: record.id)
        await deps.notifications.notify(title: "Release failed: \(product.name)", body: reason, identifier: record.id.uuidString)
    }

    private func fail(_ record: inout ReleaseRecord, product: Product, error: Error) async throws {
        guard !record.state.isTerminal, ReleaseStateMachine.canTransition(from: record.state, to: .failed, mode: record.mode) else { return }
        try await failAndReset(&record, product: product, reason: error.localizedDescription)
    }

    private func writeManifest(_ record: inout ReleaseRecord, strategy: ReleaseStrategy) throws {
        guard let version = record.version else { return }
        let manifest = ReleaseManifest.make(record: record, candidate: record.candidate, releasePath: strategy.releasePath(version: version), gitCommit: record.gitCommit, generator: deps.generator)
        let url = try deps.staging.writeManifest(manifest, slug: record.productSlug, version: version, releaseID: record.id)
        if record.manifestPath != url.path {
            record.manifestPath = url.path
            try deps.releases.save(record)
        }
    }
}
