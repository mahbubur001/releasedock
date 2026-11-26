import Foundation
import Observation
import ReleaseDockCore

@MainActor
@Observable
final class ReleaseWizardModel {
    enum Step: Int, CaseIterable, Identifiable {
        case select, inspect, compare, validate, stage, preview, approve, publish, verify

        var id: Int { rawValue }

        var title: String {
            switch self {
            case .select: "Select"
            case .inspect: "Inspect"
            case .compare: "Compare"
            case .validate: "Validate"
            case .stage: "Stage"
            case .preview: "Preview"
            case .approve: "Approve"
            case .publish: "Publish"
            case .verify: "Verify"
            }
        }
    }

    private let model: AppModel
    var productID: UUID?
    var source: ReleaseSource?
    var modeOverride: ReleaseMode?
    var kind: ReleaseKind = .version
    /// Commit summary with `CommitMessageTemplate` tokens; empty uses the default for the kind.
    var commitTemplate = ""
    private(set) var record: ReleaseRecord?
    private(set) var isRunning = false
    private(set) var activity: String?
    private(set) var log: [PipelineEvent] = []
    var errorMessage: String?
    var needsCredentials = false
    private var eventsTask: Task<Void, Never>?

    init(model: AppModel) {
        self.model = model
        productID = model.wizardProductID ?? model.products.first?.id
    }

    var product: Product? { model.product(productID) }
    var effectiveMode: ReleaseMode { modeOverride ?? product?.releaseMode ?? .assisted }
    /// Readme updates exist only for plugins; themes always publish a new version.
    var effectiveKind: ReleaseKind { product?.type == .plugin ? kind : .version }

    var currentStep: Step {
        guard let record else { return .select }
        switch record.state {
        case .detected: return .inspect
        case .inspected: return .compare
        case .validated: return .stage
        case .staged: return .preview
        case .previewed: return .approve
        case .approved, .publishing: return .publish
        case .verifying, .completed, .recoveryRequired: return .verify
        case .failed, .cancelled:
            if record.preview != nil { return .preview }
            if record.validation != nil { return .validate }
            return .inspect
        }
    }

    var canStart: Bool { product != nil && source != nil && !isRunning && record == nil && model.svnInstallation != nil && commitTemplateProblem == nil }

    var canApprove: Bool {
        guard let record, !isRunning, record.mode != .dryRun else { return false }
        return record.state == .previewed || record.state == .approved
    }

    var automationReasons: [String] {
        guard let record, let product else { return [] }
        return AutomationGate.blockingReasons(product: product, record: record, currentPreviewFingerprint: record.preview?.fingerprint)
    }

    func setSource(url: URL) {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) else { return }
        if isDir.boolValue {
            source = .folder(path: url.path)
        } else if url.pathExtension.lowercased() == "zip" {
            source = .zip(path: url.path)
        } else {
            errorMessage = "Choose a .zip package or a folder."
        }
    }

    /// Continues an unfinished release, e.g. one prepared before switching sections.
    func resume(_ release: ReleaseRecord) {
        guard record?.id != release.id else { return }
        record = release
        productID = release.productID
        source = release.source
        modeOverride = release.mode
        kind = release.kind
        log = []
        errorMessage = nil
        observeEvents()
    }

    func reset() {
        record = nil
        source = nil
        log = []
        activity = nil
        errorMessage = nil
        modeOverride = nil
        kind = .version
        commitTemplate = product?.commitMessageTemplate ?? ""
    }

    var commitTemplateProblem: String? {
        commitTemplate.trimmingCharacters(in: .whitespaces).isEmpty ? nil : CommitMessageTemplate.problem(commitTemplate)
    }

    /// Templates used before for the selected product, newest first.
    var recentCommitTemplates: [String] {
        guard let product else { return [] }
        var seen = Set<String>()
        return model.releases(for: product.id)
            .sorted { $0.createdAt > $1.createdAt }
            .compactMap(\.commitTemplate)
            .filter { seen.insert($0).inserted }
            .prefix(5)
            .map { $0 }
    }

    private func observeEvents() {
        eventsTask?.cancel()
        let pipeline = model.pipeline
        eventsTask = Task { [weak self] in
            let stream = await pipeline.events()
            for await event in stream {
                guard let self, event.releaseID == self.record?.id else { continue }
                self.log.append(event)
                self.activity = event.message
                if let updated = try? self.model.store.release(id: event.releaseID) { self.record = updated }
            }
        }
    }

    private func run(_ body: () async throws -> ReleaseRecord) async {
        isRunning = true
        errorMessage = nil
        defer {
            isRunning = false
            activity = nil
            model.reload()
        }
        do {
            record = try await body()
        } catch {
            if let id = record?.id, let latest = try? model.store.release(id: id) { record = latest }
            if case ReleaseError.credentialsRequired = error {
                needsCredentials = true
            } else if let svnError = error as? SVNError, svnError.kind == .authenticationFailed || svnError.kind == .authenticationRequired {
                needsCredentials = true
                errorMessage = "WordPress.org rejected the SVN credentials. Nothing was committed."
            } else {
                errorMessage = error.localizedDescription
            }
        }
    }

    /// Detect → Inspect → Validate → Stage → Preview (a dry run completes here).
    func prepare() async {
        guard let product, let source else { return }
        observeEvents()
        await run {
            let template = commitTemplate.trimmingCharacters(in: .whitespacesAndNewlines)
            let created = try await model.pipeline.createRelease(productID: product.id, source: source, mode: effectiveMode, kind: effectiveKind, commitTemplate: template.isEmpty ? nil : template)
            model.rememberCommitTemplate(template.isEmpty ? nil : template, for: product)
            record = created
            return try await model.pipeline.prepare(created.id)
        }
    }

    /// Exports an older released version, gives it `newVersion` and prepares it like any release.
    func prepareRestore(productID: UUID, restoring oldVersion: String, as newVersion: String, notes: String, mode: ReleaseMode) async {
        guard !isRunning else { return }
        reset()
        self.productID = productID
        modeOverride = mode
        observeEvents()
        await run {
            activity = "Exporting \(oldVersion) from the repository"
            let created = try await model.pipeline.createRestoreRelease(productID: productID, restoring: oldVersion, as: newVersion, changelogNotes: notes, mode: mode)
            record = created
            source = created.source
            return try await model.pipeline.prepare(created.id)
        }
    }

    func approveAndPublish(automatic: Bool) async {
        guard let id = record?.id else { return }
        await run {
            if record?.state != .approved {
                _ = try await model.pipeline.approve(id, kind: automatic ? .automatic : .user)
            }
            return try await model.pipeline.publish(id)
        }
        // Record the directory's starting point right away; the monitor follows up.
        // Silent: a failed check must not read like a failed release.
        if let record, record.isDirectoryTracked, let updated = try? await model.pipeline.refreshDirectoryStatus(record.id) {
            self.record = updated
            model.reload()
        }
    }

    func cancelRelease() async {
        guard let id = record?.id else { return }
        await run { try await model.pipeline.cancel(id) }
    }

    /// Whether the release can be left for Package with its product, package and mode kept.
    /// Not once publishing may have started, and not for restores, whose package lives in
    /// the release's own workspace.
    var canGoBack: Bool {
        guard let record, !isRunning, record.restoresVersion == nil, source != nil else { return false }
        if record.state.isTerminal { return record.state != .completed || record.mode == .dryRun }
        return !record.state.mayHaveCommitted
    }

    /// Returns to Package, cancelling an unfinished release first (which restores the
    /// working copy). Returns false when the cancel failed.
    @discardableResult
    func backToPackage() async -> Bool {
        guard canGoBack, let current = record else { return false }
        if !current.state.isTerminal {
            await cancelRelease()
            guard record?.state == .cancelled else { return false }
        }
        record = nil
        log = []
        activity = nil
        errorMessage = nil
        return true
    }

    /// Runs every check again on the same package, e.g. after cleaning the working copy.
    func checkAgain() async {
        guard await backToPackage() else { return }
        await prepare()
    }

    /// Backs up and reverts the working copy, then checks again. For a release that
    /// stopped on a dirty working copy.
    func revertWorkingCopyAndCheckAgain() async {
        guard let productID = record?.productID ?? product?.id, !isRunning else { return }
        isRunning = true
        activity = "Backing up and reverting the working copy"
        errorMessage = nil
        do {
            _ = try await model.pipeline.revertWorkingCopy(productID: productID)
            isRunning = false
            await checkAgain()
        } catch {
            isRunning = false
            activity = nil
            errorMessage = "Could not revert the working copy: \(error.localizedDescription)"
        }
    }

    func recover() async {
        guard let id = record?.id else { return }
        await run { try await model.pipeline.recover(id) }
    }
}
