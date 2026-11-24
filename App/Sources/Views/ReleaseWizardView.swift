import AppKit
import ReleaseDockCore
import SwiftUI
import UniformTypeIdentifiers

/// New Release in four stages: Package → Checks → Review → Publish.
struct ReleaseWizardView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        ReleaseWizardContent(wizard: model.wizard)
            .navigationTitle("New Release")
            .onAppear {
                let wizard = model.wizard!
                if let requested = model.wizardProductID, wizard.record == nil { wizard.productID = requested }
                model.wizardProductID = nil
                if wizard.productID == nil || wizard.product == nil { wizard.productID = model.products.first?.id }
                takePendingSource()
            }
            .onChange(of: model.pendingSource) { takePendingSource() }
    }

    private func takePendingSource() {
        guard let url = model.pendingSource, let wizard = model.wizard else { return }
        model.pendingSource = nil
        if wizard.record?.state.isTerminal == true { wizard.reset() }
        guard wizard.record == nil else {
            model.alert = .init(title: "A release is in progress", message: "Finish or cancel the current release before opening another package.")
            return
        }
        wizard.setSource(url: url)
        #if DEBUG
        // UI verification hook: prepare (never publish) as soon as a package is opened.
        if ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_AUTOPREPARE"] == "1" {
            Task {
                await wizard.prepare()
                // Screenshot fixtures only: publishing is allowed solely to local file:// test repositories.
                if ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_AUTOPUBLISH"] == "1",
                   wizard.product?.svnRepositoryURL.isFileURL == true {
                    await wizard.approveAndPublish(automatic: false)
                }
            }
        }
        #endif
    }
}

private enum WizardScreen {
    case select, running, review, done, failed, recovery
}

private struct ReleaseWizardContent: View {
    @Environment(AppModel.self) private var model
    @Bindable var wizard: ReleaseWizardModel
    @State private var confirmPublish = false
    @State private var bumpTarget: VersionBumpTarget?

    private var screen: WizardScreen {
        guard let record = wizard.record else { return wizard.isRunning ? .running : .select }
        if wizard.isRunning, !record.state.isTerminal, record.state != .previewed, record.state != .approved { return .running }
        switch record.state {
        case .previewed, .approved: return .review
        case .completed: return record.mode == .dryRun ? .review : .done
        case .failed, .cancelled: return .failed
        case .recoveryRequired: return .recovery
        case .publishing, .verifying: return .running
        default: return .running
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            PhaseIndicator(phase: phase, failed: screen == .failed)
                .padding(.horizontal, 28)
                .padding(.vertical, 14)
            Divider()
            ScrollView {
                Group {
                    switch screen {
                    case .select: selection
                    case .running: RunningView(wizard: wizard)
                    case .review: review
                    case .done: done
                    case .failed: failed
                    case .recovery: recovery
                    }
                }
                .padding(.horizontal, 28)
                .padding(.vertical, 22)
                .frame(maxWidth: 940)
                .frame(maxWidth: .infinity)
            }
            actionBar
        }
        .sheet(isPresented: $wizard.needsCredentials) {
            if let product = wizard.product { CredentialsSheet(product: product) }
        }
        .sheet(isPresented: $confirmPublish) {
            if let record = wizard.record {
                PublishConfirmation(record: record) {
                    confirmPublish = false
                    Task { await wizard.approveAndPublish(automatic: false) }
                }
            }
        }
        .sheet(item: $bumpTarget) { target in
            if let product = wizard.product {
                VersionBumpSheet(product: product, folder: target.folder) { _ in wizard.setSource(url: target.folder) }
            }
        }
    }

    /// 0 Package · 1 Checks · 2 Review · 3 Publish
    private var phase: Int {
        guard let record = wizard.record else { return 0 }
        switch record.state {
        case .detected, .inspected, .validated: return 1
        case .staged, .previewed, .approved: return 2
        case .publishing, .verifying, .recoveryRequired: return 3
        case .completed: return record.mode == .dryRun ? 2 : 4
        case .failed, .cancelled: return record.preview != nil ? 2 : 1
        }
    }

    // MARK: Package

    @ViewBuilder private var selection: some View {
        if model.products.isEmpty {
            ContentUnavailableView {
                Label("No products", systemImage: "shippingbox")
            } description: {
                Text("Add a plugin or theme before releasing.")
            } actions: {
                Button("Add Product…") {
                    model.selection = .products
                    model.showAddProduct = true
                }
                .buttonStyle(.borderedProminent)
            }
        } else {
            VStack(alignment: .leading, spacing: 10) {
                SectionHeading("Product")
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
                    ForEach(model.products) { product in
                        ChoiceCard(isSelected: wizard.productID == product.id) {
                            wizard.productID = product.id
                        } content: {
                            ProductGlyph(type: product.type)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(product.name).font(.headline)
                                Text("\(product.type.displayName) · \(product.currentVersion.map { "live \($0)" } ?? "not released yet")").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                if wizard.product?.type == .plugin {
                    SectionHeading("Publish")
                    HStack(spacing: 10) {
                        ModeCard(title: "New Version", symbol: "shippingbox", detail: "Update trunk and create tags/<version>.", isSelected: wizard.kind == .version) { wizard.kind = .version }
                        ModeCard(title: "Readme & Assets Update", symbol: "doc.text", detail: "Change readme.txt\(wizard.product?.assetsPath == nil ? "" : " and /assets") of the released \(wizard.product?.currentVersion ?? "version"), e.g. raise “Tested up to”. Code changes are refused.", isSelected: wizard.kind == .readmeUpdate) { wizard.kind = .readmeUpdate }
                    }
                }
                SectionHeading("Mode")
                HStack(spacing: 10) {
                    ModeCard(title: "Assisted", symbol: "checkmark.shield", detail: "You approve the exact SVN preview before anything is committed.", isSelected: wizard.effectiveMode == .assisted) { wizard.modeOverride = .assisted }
                    ModeCard(title: "Dry Run", symbol: "doc.text.magnifyingglass", detail: "Run every check and build the preview. Never commits.", isSelected: wizard.effectiveMode == .dryRun) { wizard.modeOverride = .dryRun }
                    ModeCard(title: "Automatic", symbol: "paperplane", detail: "Publishes without asking when every condition holds.", isSelected: wizard.effectiveMode == .automatic) { wizard.modeOverride = .automatic }
                }
                if wizard.effectiveMode == .automatic, wizard.product?.automationEnabled != true {
                    Label("Automatic publishing is not enabled for this product; you will be asked to confirm.", systemImage: "info.circle")
                        .font(.caption).foregroundStyle(.secondary)
                }
                SectionHeading("Package")
                if let source = wizard.source {
                    SourceCard(source: source, product: wizard.product, canBump: bumpableFolder != nil) {
                        if let folder = bumpableFolder { bumpTarget = VersionBumpTarget(folder: folder) }
                    } onChange: {
                        wizard.source = nil
                    }
                } else {
                    DropArea(title: "Drop a ZIP package or an updated folder", subtitle: "The source is read-only. Only a cleaned copy is ever published.") { url in
                        wizard.setSource(url: url)
                    } actions: {
                        Button("Choose Folder…") { if let url = PackagePicker.choose(zip: false) { wizard.setSource(url: url) } }
                        if let dev = wizard.product?.localDevelopmentURL {
                            Button("Use Development Folder") { wizard.setSource(url: dev) }
                        }
                        if let folder = wizard.product?.localDevelopmentURL {
                            Button("Bump Version…") { bumpTarget = VersionBumpTarget(folder: folder) }
                        }
                        Button("Choose ZIP…") { if let url = PackagePicker.choose(zip: true) { wizard.setSource(url: url) } }
                            .buttonStyle(.borderedProminent)
                    }
                }
            }
        }
    }

    /// The folder a bump would edit: the chosen folder source, else the development folder.
    private var bumpableFolder: URL? {
        if case .folder(let path)? = wizard.source { return URL(fileURLWithPath: path, isDirectory: true) }
        return wizard.product?.localDevelopmentURL
    }

    // MARK: Review

    @ViewBuilder private var review: some View {
        if let record = wizard.record {
            VStack(alignment: .leading, spacing: 12) {
                if record.mode == .dryRun, record.state == .completed {
                    Label("Dry run complete. Everything was checked and staged; nothing was committed and the working copy was restored.", systemImage: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                        .card(padding: 12)
                }
                ReleaseHeroCard(record: record)
                ReleaseSummaryTiles(record: record)
                WhatsNewCard(record: record)
                if let build = record.build { BuildCard(build: build) }
                if let report = record.validation { ChecksSection(report: report) }
                if let preview = record.preview { ChangesSection(preview: preview) }
                if let inspection = record.inspection { ExcludedSection(inspection: inspection) }
            }
        }
    }

    // MARK: Done

    @ViewBuilder private var done: some View {
        if let record = wizard.record {
            VStack(spacing: 18) {
                Image(systemName: "checkmark.seal.fill")
                    .font(.system(size: 40))
                    .foregroundStyle(.green)
                    .frame(width: 76, height: 76)
                    .background(.green.opacity(0.15), in: Circle())
                    .transition(.scale(scale: 0.85).combined(with: .opacity))
                VStack(spacing: 6) {
                    Text(doneTitle(record)).font(.system(size: 24, weight: .bold)).tracking(-0.4)
                    Text(record.isDirectoryTracked
                         ? "WordPress.org offers it to sites after its security review period. ReleaseDock checks every 10 minutes and notifies you."
                         : "Committed once, then verified against the server copy.")
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                ReleaseOutcomeSection(record: record, product: wizard.product)
                    .frame(maxWidth: 640)
            }
            .frame(maxWidth: .infinity)
            .padding(.top, 8)
        }
    }

    private func doneTitle(_ record: ReleaseRecord) -> String {
        let name = "\(record.productName) \(record.version ?? "")"
        if record.kind == .readmeUpdate { return "The \(name) readme is updated" }
        return record.directoryStatus?.state == .live ? "\(name) is live" : "\(name) is committed"
    }

    // MARK: Failed / recovery

    @ViewBuilder private var failed: some View {
        if let record = wizard.record {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    TintedSymbol(systemImage: record.state == .cancelled ? "xmark" : "exclamationmark.octagon", color: record.state == .cancelled ? .secondary : .red)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(record.state == .cancelled ? "Release cancelled" : "Release stopped").font(.headline)
                        Text(record.state == .cancelled ? "Nothing was committed and the working copy was restored." : "Nothing was committed. Fix the problems below and prepare again.")
                            .foregroundStyle(.secondary)
                        ForEach(record.errors, id: \.self) { Text($0).font(.callout.monospaced()).textSelection(.enabled) }
                    }
                }
                .card()
                if record.detected != nil { ReleaseHeroCard(record: record) }
                if let build = record.build { BuildCard(build: build) }
                if let report = record.validation { ChecksSection(report: report) }
                if let preview = record.preview { ChangesSection(preview: preview) }
            }
        }
    }

    @ViewBuilder private var recovery: some View {
        if let record = wizard.record {
            VStack(alignment: .leading, spacing: 8) {
                Label(record.verifiedNotCommitted ? "Verified: the commit is not in the repository" : "The commit outcome is uncertain", systemImage: "exclamationmark.arrow.triangle.2.circlepath")
                    .font(.headline)
                    .foregroundStyle(.orange)
                Text(record.verifiedNotCommitted
                     ? "Nothing reached WordPress.org. You can publish again after reviewing the preview."
                     : "The connection ended during the commit. ReleaseDock never retries blindly: inspect the repository to find out whether it landed.")
                    .foregroundStyle(.secondary)
                ForEach(record.errors, id: \.self) { Text($0).font(.caption.monospaced()).foregroundStyle(.secondary) }
            }
            .card()
        }
    }

    // MARK: Action bar

    private var actionBar: some View {
        ActionBar {
            if wizard.isRunning {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(wizard.activity ?? "Working…")
                }
            } else if let error = wizard.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red).textSelection(.enabled)
            } else {
                Label(note, systemImage: noteSymbol)
            }
        } actions: {
            actions
        }
    }

    private var note: String {
        switch screen {
        case .select: wizard.source == nil ? "Choose a package to continue." : "ReleaseDock inspects, compares and validates before showing a preview."
        case .running: "You can cancel at any time before the commit."
        case .review: wizard.record?.mode == .dryRun ? "Dry run: nothing will be committed." : "Approving publishes exactly this preview. If the working copy changes, you'll approve again."
        case .done: "Release recorded in History."
        case .failed: "Nothing was sent to WordPress.org."
        case .recovery: "ReleaseDock decides from the repository, never by retrying."
        }
    }

    private var noteSymbol: String {
        switch screen {
        case .review: "checkmark.shield"
        case .done: "checkmark"
        default: "info.circle"
        }
    }

    @ViewBuilder private var actions: some View {
        if let record = wizard.record {
            if !record.state.isTerminal, !record.state.mayHaveCommitted, !wizard.isRunning {
                Button("Cancel Release", role: .destructive) { Task { await wizard.cancelRelease() } }
            }
            if record.state == .recoveryRequired, !wizard.isRunning {
                Button("Inspect Repository") { Task { await wizard.recover() } }
                    .buttonStyle(.borderedProminent)
            }
            if record.state.isTerminal, !wizard.isRunning {
                if record.state == .completed, record.mode != .dryRun {
                    Button("View in History") { model.open(record) }
                }
                Button(record.state == .completed ? "New Release" : "Start Over") { wizard.reset() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            }
            if wizard.canApprove {
                if wizard.effectiveMode == .automatic, wizard.automationReasons.isEmpty {
                    Button("Publish Automatically") { Task { await wizard.approveAndPublish(automatic: true) } }
                        .buttonStyle(.borderedProminent)
                } else {
                    Button(record.kind == .readmeUpdate ? "Update Readme…" : "Publish \(record.version ?? "")…") { confirmPublish = true }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.return, modifiers: .command)
                        .help(wizard.automationReasons.joined(separator: "\n"))
                }
            }
        } else if !wizard.isRunning {
            Button(wizard.effectiveMode == .dryRun ? "Run Dry Run" : "Inspect & Prepare") { Task { await wizard.prepare() } }
                .buttonStyle(.borderedProminent)
                .disabled(!wizard.canStart)
                .keyboardShortcut(.defaultAction)
        }
    }
}

// MARK: - Pieces

/// Four stages, each covering several pipeline states.
private struct PhaseIndicator: View {
    let phase: Int
    let failed: Bool

    private let phases = [("Package", "Product & source"), ("Checks", "Inspect · compare · validate"), ("Review", "Stage · preview · approve"), ("Publish", "Commit · verify")]

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(phases.enumerated()), id: \.offset) { index, item in
                HStack(spacing: 10) {
                    ZStack {
                        Circle().fill(fill(index)).frame(width: 24, height: 24)
                        if index < phase {
                            Image(systemName: "checkmark").font(.caption.weight(.bold)).foregroundStyle(.white)
                        } else {
                            Text("\(index + 1)").font(.caption.weight(.semibold)).foregroundStyle(index == phase ? .white : .secondary)
                        }
                    }
                    VStack(alignment: .leading, spacing: 0) {
                        Text(item.0).font(.callout.weight(.semibold)).foregroundStyle(index <= phase ? .primary : .secondary)
                        Text(item.1).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    .fixedSize()
                }
                if index < phases.count - 1 {
                    Capsule()
                        .fill(index < phase ? Color.green : Color.primary.opacity(0.1))
                        .frame(height: 2)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 14)
                }
            }
        }
        .animation(.easeOut(duration: 0.3), value: phase)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Stage \(min(phase + 1, 4)) of 4: \(phases[min(phase, 3)].0)")
    }

    private func fill(_ index: Int) -> Color {
        if index < phase { return .green }
        if index == phase { return failed ? .red : .accentColor }
        return Color.primary.opacity(0.1)
    }
}

/// Live checklist of what the pipeline has done, from its events.
private struct RunningView: View {
    let wizard: ReleaseWizardModel

    var body: some View {
        VStack(spacing: 18) {
            VStack(spacing: 4) {
                Text(title).font(.title2.weight(.semibold))
                Text(isPublishing ? "Committing once, then verifying the server copy." : "Nothing is sent to WordPress.org until you approve.")
                    .foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    let isCurrent = index == steps.count - 1 && wizard.isRunning
                    HStack(spacing: 12) {
                        if isCurrent {
                            ProgressView().controlSize(.small).frame(width: 20, height: 20)
                        } else {
                            Image(systemName: "checkmark")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(.white)
                                .frame(width: 20, height: 20)
                                .background(.green, in: Circle())
                        }
                        Text(step).foregroundStyle(isCurrent ? .primary : .secondary)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 9)
                    .background(isCurrent ? AnyShapeStyle(.quinary) : AnyShapeStyle(.clear), in: RoundedRectangle(cornerRadius: 10))
                }
            }
            .frame(maxWidth: 520)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 24)
    }

    private var isPublishing: Bool {
        guard let state = wizard.record?.state else { return false }
        return state == .approved || state == .publishing || state == .verifying
    }

    private var title: String {
        let name = "\(wizard.product?.name ?? "") \(wizard.record?.version ?? "")".trimmingCharacters(in: .whitespaces)
        return isPublishing ? "Publishing \(name)" : "Preparing \(name)"
    }

    /// Unique event messages in order; the last one is in progress.
    private var steps: [String] {
        var seen: Set<String> = []
        let messages = wizard.log.map(\.message).filter { seen.insert($0).inserted }
        return messages.isEmpty ? [wizard.activity ?? "Starting"] : messages
    }
}

/// Selectable card for a product.
private struct ChoiceCard<Content: View>: View {
    let isSelected: Bool
    let action: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                content
                Spacer(minLength: 8)
                Image(systemName: isSelected ? "largecircle.fill.circle" : "circle")
                    .font(.title3)
                    .foregroundStyle(isSelected ? Color.accentColor : Color.secondary)
            }
            .padding(14)
            .background(.background, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(isSelected ? Color.accentColor : Color.primary.opacity(0.1), lineWidth: isSelected ? 2 : 0.5))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// Selectable option with a one-line explanation.
private struct ModeCard: View {
    let title: String
    let symbol: String
    let detail: String
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 3) {
                Label(title, systemImage: symbol).font(.callout.weight(.semibold))
                    .foregroundStyle(isSelected ? Color.accentColor : .primary)
                Text(detail).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(isSelected ? Color.accentColor.opacity(0.1) : Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(isSelected ? Color.accentColor : .clear, lineWidth: 1.5))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

/// The chosen ZIP or folder.
private struct SourceCard: View {
    let source: ReleaseSource
    let product: Product?
    let canBump: Bool
    let onBump: () -> Void
    let onChange: () -> Void

    var body: some View {
        HStack(spacing: 14) {
            TintedSymbol(systemImage: source.kind == .folder ? "folder.fill" : "doc.zipper", size: 44)
            VStack(alignment: .leading, spacing: 2) {
                Text(URL(fileURLWithPath: source.path).lastPathComponent).font(.headline)
                Text(detail).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
            Spacer(minLength: 8)
            if source.kind == .folder, canBump { Button("Bump Version…", action: onBump) }
            Button("Change…", action: onChange)
        }
        .card()
    }

    private var detail: String {
        var parts = [(source.path as NSString).abbreviatingWithTildeInPath]
        if source.kind == .folder, let product {
            if let version = try? ProductDetector().detect(at: URL(fileURLWithPath: source.path), expectedType: product.type, mainPluginFile: product.mainPluginFile, slug: product.slug).version {
                parts.append("version \(version)")
            }
            if let build = product.effectiveBuildCommand { parts.append("build: \(build)") }
        }
        return parts.joined(separator: " · ")
    }
}

/// The publish confirmation: what exactly happens, then Publish.
private struct PublishConfirmation: View {
    @Environment(\.dismiss) private var dismiss
    let record: ReleaseRecord
    let onPublish: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            ProductGlyph(type: record.productType, size: 52)
            Text(record.kind == .readmeUpdate ? "Update the readme of \(record.productName) \(record.version ?? "") on WordPress.org?" : "Publish \(record.productName) \(record.version ?? "") to WordPress.org?")
                .font(.headline)
            Text("This commits \(record.preview?.displayedChanges.count ?? 0) change(s) in a single revision. \(record.kind == .readmeUpdate ? "The released code is not touched." : "A released version can't be overwritten.")")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            GroupedRows {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    GroupedRow(isFirst: index == 0) {
                        Image(systemName: step.0).foregroundStyle(.secondary).frame(width: 18)
                        Text(step.1)
                    }
                }
            }
            HStack {
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .frame(maxWidth: .infinity)
                Button(record.kind == .readmeUpdate ? "Update" : "Publish", action: onPublish)
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .frame(maxWidth: .infinity)
            }
            .controlSize(.large)
        }
        .padding(22)
        .frame(width: 440)
    }

    private var steps: [(String, String)] {
        let version = record.version ?? ""
        if record.kind == .readmeUpdate {
            return [("doc.text", "Update the readme in trunk and tags/\(version)"), ("checkmark.shield", "Verify both readmes on the server")]
        }
        if record.productType == .theme {
            return [("folder.badge.plus", "Create /\(version) from the previous version"), ("checkmark.shield", "Verify the server copy with SHA-256")]
        }
        return [("arrow.left.arrow.right", "Update trunk (\(record.candidate?.files.count ?? 0) files)"), ("doc.on.doc", "Copy trunk to tags/\(version)"), ("checkmark.shield", "Verify the server copy with SHA-256")]
    }
}

struct CredentialsSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let product: Product
    @State private var username: String = ""
    @State private var password: String = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("SVN credentials for \(product.name)").font(.headline)
            Text("Enter your WordPress.org username (case-sensitive) and the SVN password generated under Profile → Account & Security → SVN credentials. The password is stored only in your macOS Keychain and passed to svn through standard input.")
                .font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            TextField("Username", text: $username)
            SecureField("SVN password", text: $password)
            if let error { Text(error).foregroundStyle(.red).font(.caption) }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Save to Keychain") {
                    do {
                        try model.storeCredentials(for: product, username: username, password: password)
                        password = ""
                        dismiss()
                    } catch {
                        self.error = error.localizedDescription
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(username.isEmpty || password.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 440)
        .onAppear { username = product.svnUsername ?? "" }
    }
}
