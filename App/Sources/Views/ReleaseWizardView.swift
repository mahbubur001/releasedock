import AppKit
import ReleaseDockCore
import SwiftUI
import UniformTypeIdentifiers

struct ReleaseWizardView: View {
    @Environment(AppModel.self) private var model
    @State private var wizard: ReleaseWizardModel?

    var body: some View {
        Group {
            if let wizard {
                ReleaseWizardContent(wizard: wizard)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Release Wizard")
        .onAppear {
            if wizard == nil { wizard = ReleaseWizardModel(model: model) }
            if let requested = model.wizardProductID, wizard?.record == nil { wizard?.productID = requested }
            model.wizardProductID = nil
            takePendingSource()
        }
        .onChange(of: model.pendingSource) { takePendingSource() }
    }

    private func takePendingSource() {
        guard let url = model.pendingSource, let wizard else { return }
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

private struct ReleaseWizardContent: View {
    @Environment(AppModel.self) private var model
    @Bindable var wizard: ReleaseWizardModel
    @State private var confirmPublish = false
    @State private var dropTargeted = false
    @State private var bumpTarget: VersionBumpTarget?

    var body: some View {
        VStack(spacing: 0) {
            StepIndicator(current: wizard.currentStep, failed: wizard.record?.state == .failed || wizard.record?.state == .cancelled)
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if wizard.record == nil {
                        selection
                    } else if let record = wizard.record {
                        ReleaseReportView(record: record, product: wizard.product)
                    }
                }
                .padding(20)
            }
            Divider()
            actionBar.padding(12)
        }
        .sheet(item: $bumpTarget) { target in
            if let product = wizard.product {
                VersionBumpSheet(product: product, folder: target.folder) { _ in
                    // Re-select so the wizard reads the new version.
                    wizard.setSource(url: target.folder)
                }
            }
        }
        .sheet(isPresented: $wizard.needsCredentials) {
            if let product = wizard.product {
                CredentialsSheet(product: product)
            }
        }
        .confirmationDialog(publishQuestion, isPresented: $confirmPublish) {
            SwiftUI.Button("Publish") { Task { await wizard.approveAndPublish(automatic: false) } }
        } message: {
            if let preview = wizard.record?.preview {
                Text("This commits \(preview.changes.count) change(s) to \(wizard.record?.svnURL ?? "SVN"). Released versions cannot be overwritten.")
            }
        }
    }

    /// The folder a bump would edit: the chosen folder source, else the development folder.
    private var bumpableFolder: URL? {
        if case .folder(let path)? = wizard.source { return URL(fileURLWithPath: path, isDirectory: true) }
        return wizard.product?.localDevelopmentURL
    }

    private var publishQuestion: String {
        let name = "\(wizard.product?.name ?? "") \(wizard.record?.version ?? "")"
        return wizard.record?.kind == .readmeUpdate
            ? "Update the readme of \(name) on WordPress.org?"
            : "Publish \(name) to WordPress.org?"
    }

    // MARK: Selection

    @ViewBuilder private var selection: some View {
        if model.products.isEmpty {
            ContentUnavailableView {
                Label("No products", systemImage: "shippingbox")
            } description: {
                Text("Add a plugin or theme before releasing.")
            } actions: {
                Button("Add Product") {
                    model.selection = .products
                    model.showAddProduct = true
                }
            }
        } else {
            SectionCard(title: "1. Product", systemImage: "shippingbox") {
                Picker("Product", selection: $wizard.productID) {
                    ForEach(model.products) { product in
                        Text("\(product.name) (\(product.type.displayName.lowercased()), \(product.currentVersion ?? "no release"))").tag(Optional(product.id))
                    }
                }
                if wizard.product?.type == .plugin {
                    Picker("Publish", selection: $wizard.kind) {
                        ForEach(ReleaseKind.allCases) { kind in
                            Text(kind.displayName).tag(kind)
                        }
                    }
                    .pickerStyle(.segmented)
                    if wizard.kind == .readmeUpdate {
                        Label("Updates readme.txt\(wizard.product?.assetsPath == nil ? "" : " and /assets") of the released \(wizard.product?.currentVersion ?? "version") in trunk and its tag, e.g. to raise \"Tested up to\". Use the released package: any code change is refused.", systemImage: "info.circle")
                            .font(.caption).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Picker("Mode", selection: Binding(get: { wizard.effectiveMode }, set: { wizard.modeOverride = $0 })) {
                    ForEach(ReleaseMode.allCases) { mode in
                        Text(mode.displayName).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                if wizard.effectiveMode == .automatic, wizard.product?.automationEnabled != true {
                    Label("Automation is not enabled for this product; you will be asked to confirm.", systemImage: "info.circle")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            SectionCard(title: "2. Package", systemImage: "doc.zipper") {
                dropZone
                HStack {
                    Button("Choose ZIP…") { choose(zip: true) }
                    Button("Choose Folder…") { choose(zip: false) }
                    if let dev = wizard.product?.localDevelopmentURL {
                        Button("Use Development Folder") { wizard.setSource(url: dev) }
                    }
                    if let folder = bumpableFolder {
                        Button("Bump Version…") { bumpTarget = VersionBumpTarget(folder: folder) }
                            .help("Update the version in the header, Stable tag, constants and package.json of \(folder.lastPathComponent)")
                    }
                }
                Text("The source is inspected read-only. Only a cleaned copy is ever published.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var dropZone: some View {
        RoundedRectangle(cornerRadius: 10)
            .strokeBorder(style: StrokeStyle(lineWidth: 2, dash: [6]))
            .foregroundStyle(dropTargeted ? Color.accentColor : Color.secondary.opacity(0.5))
            .frame(height: 110)
            .overlay {
                VStack(spacing: 6) {
                    Image(systemName: wizard.source?.kind == .folder ? "folder.fill" : "doc.zipper").font(.title)
                    if let source = wizard.source {
                        Text(URL(fileURLWithPath: source.path).lastPathComponent).font(.headline)
                        Text(source.path).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    } else {
                        Text("Drop a ZIP package or an updated folder here").foregroundStyle(.secondary)
                    }
                }
                .padding()
            }
            .dropDestination(for: URL.self) { urls, _ in
                guard let url = urls.first else { return false }
                wizard.setSource(url: url)
                return true
            } isTargeted: { dropTargeted = $0 }
    }

    private func choose(zip: Bool) {
        let panel = NSOpenPanel()
        panel.canChooseFiles = zip
        panel.canChooseDirectories = !zip
        panel.allowsMultipleSelection = false
        if zip { panel.allowedContentTypes = [.zip] }
        panel.prompt = zip ? "Use Package" : "Use Folder"
        if panel.runModal() == .OK, let url = panel.url { wizard.setSource(url: url) }
    }

    // MARK: Actions

    private var actionBar: some View {
        HStack(spacing: 12) {
            if wizard.isRunning {
                ProgressView().controlSize(.small)
                Text(wizard.activity ?? "Working…").foregroundStyle(.secondary).lineLimit(1)
            } else if let error = wizard.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red).lineLimit(2).textSelection(.enabled)
            }
            Spacer()
            if let record = wizard.record {
                if !record.state.isTerminal, !record.state.mayHaveCommitted, !wizard.isRunning {
                    Button("Cancel Release", role: .destructive) { Task { await wizard.cancelRelease() } }
                }
                if record.state == .recoveryRequired, !wizard.isRunning {
                    Button("Inspect Repository") { Task { await wizard.recover() } }
                }
                if record.state.isTerminal, !wizard.isRunning {
                    Button("New Release") { wizard.reset() }
                }
                if wizard.canApprove {
                    if wizard.effectiveMode == .automatic, wizard.automationReasons.isEmpty {
                        Button("Publish Automatically") { Task { await wizard.approveAndPublish(automatic: true) } }
                            .buttonStyle(.borderedProminent)
                    } else {
                        Button("Approve & Publish…") { confirmPublish = true }
                            .buttonStyle(.borderedProminent)
                            .help(wizard.automationReasons.joined(separator: "\n"))
                    }
                }
            } else {
                Button(wizard.effectiveMode == .dryRun ? "Run Dry Run" : "Inspect & Prepare") {
                    Task { await wizard.prepare() }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!wizard.canStart)
                .keyboardShortcut(.defaultAction)
            }
        }
    }
}

struct StepIndicator: View {
    let current: ReleaseWizardModel.Step
    let failed: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            ForEach(ReleaseWizardModel.Step.allCases) { step in
                VStack(spacing: 4) {
                    ZStack {
                        Circle().fill(fill(step)).frame(width: 22, height: 22)
                        if step.rawValue < current.rawValue {
                            Image(systemName: "checkmark").font(.caption2.bold()).foregroundStyle(.white)
                        } else {
                            Text("\(step.rawValue + 1)").font(.caption2.bold()).foregroundStyle(step == current ? .white : .secondary)
                        }
                    }
                    Text(step.title)
                        .font(.caption2)
                        .lineLimit(1)
                        .fixedSize()
                        .foregroundStyle(step == current ? .primary : .secondary)
                }
                .frame(minWidth: 52)
                if step != ReleaseWizardModel.Step.allCases.last {
                    Rectangle()
                        .fill(step.rawValue < current.rawValue ? Color.green.opacity(0.6) : Color.secondary.opacity(0.3))
                        .frame(height: 2)
                        .frame(maxWidth: .infinity)
                        .padding(.top, 10)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Step \(current.rawValue + 1) of 9: \(current.title)")
    }

    private func fill(_ step: ReleaseWizardModel.Step) -> Color {
        if step.rawValue < current.rawValue { return .green }
        if step == current { return failed ? .red : .accentColor }
        return Color.secondary.opacity(0.2)
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
