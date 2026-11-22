import AppKit
import ReleaseDockCore
import SwiftUI

@MainActor
@Observable
final class AddProductModel {
    enum Mode { case checkout, existing, manual }

    private let model: AppModel
    var mode: Mode? {
        didSet { if mode == .manual, manualEditor == nil { manualEditor = ProductEditorModel(model: model, product: nil) } }
    }
    private(set) var manualEditor: ProductEditorModel?

    // Check out from server
    var type: ProductType = .plugin
    var repositoryInput = ""
    var destination = ""
    private var destinationCustomized = false

    // Existing working copy
    var existing: WorkingCopyDescription?
    var chosenType: ProductType = .plugin

    // Shared
    var remote: RepositoryDescription?
    var username = ""
    var password = ""
    var isWorking = false
    var activity: String?
    var errorMessage: String?

    init(model: AppModel) {
        self.model = model
    }

    var resolvedType: ProductType? {
        switch mode {
        case .checkout: remote?.type ?? type
        case .existing: existing?.repository.type ?? chosenType
        default: nil
        }
    }

    private func run(_ message: String, _ body: () async throws -> Void) async {
        isWorking = true
        activity = message
        errorMessage = nil
        defer { isWorking = false; activity = nil }
        do { try await body() } catch { errorMessage = error.localizedDescription }
    }

    func findRepository() async {
        remote = nil
        await run("Reading the repository…") {
            let url = try RepositorySetupService.repositoryURL(from: repositoryInput, type: type)
            let description = try await model.repositorySetup.describeRemote(url, expectedType: type)
            remote = description
            if let detected = description.type { type = detected }
            if !destinationCustomized {
                destination = model.productService.defaultWorkingCopyPath(slug: description.slug, type: description.type ?? type)
            }
        }
    }

    func chooseDestination() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Check Out Here"
        panel.message = "Choose an empty folder for the SVN working copy."
        if panel.runModal() == .OK, let url = panel.url {
            destination = url.path
            destinationCustomized = true
        }
    }

    func chooseExisting() async {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = "Use Working Copy"
        panel.message = "Select the root of an SVN checkout (the folder that contains trunk/ and tags/, or a theme's version folders)."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        existing = nil
        await run("Reading the working copy…") {
            existing = try await model.repositorySetup.describeWorkingCopy(at: url)
            if let detected = existing?.repository.type { chosenType = detected }
        }
    }

    /// Checks out (if needed), detects everything from the checkout, stores credentials, saves.
    func add() async -> Bool {
        var added = false
        await run(mode == .checkout ? "Checking out…" : "Adding product…") {
            let description: RepositoryDescription
            let workingCopy: URL
            let productType: ProductType
            switch mode {
            case .checkout:
                guard let remote else { return }
                productType = remote.type ?? type
                description = remote
                workingCopy = URL(fileURLWithPath: destination, isDirectory: true)
                try await model.repositorySetup.checkout(remote, type: productType, to: workingCopy)
            case .existing:
                guard let existing else { return }
                productType = existing.repository.type ?? chosenType
                description = existing.repository
                workingCopy = existing.path
            default:
                return
            }
            activity = "Detecting product details…"
            var product = model.repositorySetup.makeProduct(from: description, type: productType, workingCopy: workingCopy)
            let user = username.trimmingCharacters(in: .whitespaces)
            if !user.isEmpty { product.svnUsername = user }
            try model.productService.save(product)
            if !user.isEmpty, !password.isEmpty {
                try model.storeCredentials(for: product, username: user, password: password)
            }
            password = ""
            model.reload()
            added = true
        }
        return added
    }
}

struct AddProductView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var add: AddProductModel?

    var body: some View {
        Group {
            if let add {
                if add.mode == .manual, let editor = add.manualEditor {
                    ProductEditorView(editor: editor)
                } else {
                    AddProductContent(add: add, dismiss: { dismiss() })
                }
            }
        }
        .onAppear { if add == nil { add = AddProductModel(model: model) } }
    }
}

private struct AddProductContent: View {
    @Bindable var add: AddProductModel
    let dismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                if add.mode != nil {
                    Button { add.mode = nil; add.errorMessage = nil } label: { Image(systemName: "chevron.left") }
                        .buttonStyle(.borderless)
                        .pointingHandCursor(!add.isWorking)
                        .disabled(add.isWorking)
                }
                Text(title).font(.title2.bold())
            }
            .padding(20)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    switch add.mode {
                    case nil: chooser
                    case .checkout: checkout
                    case .existing: existing
                    case .manual: EmptyView()
                    }
                }
                .padding(20)
            }
            Divider()
            footer.padding(14)
        }
        .frame(width: 620, height: 600)
    }

    private var title: String {
        switch add.mode {
        case nil: "Add Product"
        case .checkout: "Check Out from SVN Server"
        case .existing: "Use Existing Working Copy"
        case .manual: "Set Up Manually"
        }
    }

    // MARK: Chooser

    private var chooser: some View {
        VStack(spacing: 12) {
            Text("Connect a WordPress.org plugin or theme. ReleaseDock reads the name, type, slug and current version from SVN automatically.")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            choice("Check out from SVN server", detail: "Enter the slug or SVN URL. ReleaseDock creates a lightweight checkout (trunk and the latest version only).", symbol: "icloud.and.arrow.down", mode: .checkout)
            choice("Use an existing working copy", detail: "Select a folder you already checked out with svn.", symbol: "folder.badge.gearshape", mode: .existing)
            choice("Set up manually", detail: "Enter every setting yourself.", symbol: "slider.horizontal.3", mode: .manual)
        }
    }

    private func choice(_ title: String, detail: String, symbol: String, mode: AddProductModel.Mode) -> some View {
        Button { add.mode = mode } label: {
            HStack(spacing: 14) {
                Image(systemName: symbol).font(.title).frame(width: 40).foregroundStyle(Color.accentColor)
                VStack(alignment: .leading, spacing: 3) {
                    Text(title).font(.headline)
                    Text(detail).font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.leading)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(.tertiary)
            }
            .padding(14)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .pointingHandCursor()
    }

    // MARK: Checkout

    @ViewBuilder private var checkout: some View {
        SectionCard(title: "Repository", systemImage: "server.rack") {
            Picker("Type", selection: $add.type) {
                ForEach(ProductType.allCases) { Text($0.displayName).tag($0) }
            }
            .pickerStyle(.segmented)
            .disabled(add.isWorking)
            HStack {
                TextField("Slug or SVN URL", text: $add.repositoryInput, prompt: Text(add.type == .plugin ? "my-plugin or https://plugins.svn.wordpress.org/my-plugin" : "my-theme or https://themes.svn.wordpress.org/my-theme"))
                    .onSubmit { Task { await add.findRepository() } }
                Button("Find") { Task { await add.findRepository() } }
                    .disabled(add.repositoryInput.trimmingCharacters(in: .whitespaces).isEmpty || add.isWorking)
            }
        }
        if let remote = add.remote {
            detected(remote)
            SectionCard(title: "Working copy", systemImage: "folder") {
                HStack {
                    Text(add.destination).font(.callout.monospaced()).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button("Change…") { add.chooseDestination() }
                }
                Text("Only trunk, the tag list and (for themes) the latest version are downloaded.").font(.caption).foregroundStyle(.secondary)
            }
            credentials
        }
    }

    // MARK: Existing

    @ViewBuilder private var existing: some View {
        SectionCard(title: "Working copy", systemImage: "folder") {
            HStack {
                Text(add.existing?.path.path ?? "No folder selected").font(.callout.monospaced()).lineLimit(1).truncationMode(.middle)
                Spacer()
                Button(add.existing == nil ? "Choose Folder…" : "Change…") { Task { await add.chooseExisting() } }
                    .disabled(add.isWorking)
            }
        }
        if let description = add.existing {
            detected(description.repository, revision: description.revision)
            if description.repository.type == nil {
                Picker("Type", selection: $add.chosenType) {
                    ForEach(ProductType.allCases) { Text($0.displayName).tag($0) }
                }
                .pickerStyle(.segmented)
            }
            if !description.localChanges.isEmpty {
                Label("This working copy has \(description.localChanges.count) local change(s). Releases are blocked until they are committed or reverted.", systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .font(.callout)
            }
            credentials
        }
    }

    // MARK: Shared

    private func detected(_ description: RepositoryDescription, revision: Int? = nil) -> some View {
        SectionCard(title: "Detected", systemImage: "sparkle.magnifyingglass") {
            KeyValueRow(key: "Name", value: description.productName ?? "Read after checkout")
            KeyValueRow(key: "Type", value: (description.type ?? add.resolvedType)?.displayName ?? "Unknown")
            KeyValueRow(key: "Slug", value: description.slug, monospaced: true)
            KeyValueRow(key: "SVN URL", value: description.url.absoluteString, monospaced: true)
            KeyValueRow(key: "Current version", value: description.currentVersion ?? "None released yet")
            KeyValueRow(key: description.type == .theme ? "Versions" : "Tags", value: "\(description.versions.count)")
            if let revision { KeyValueRow(key: "Checked out at", value: "r\(revision)") }
            ForEach(description.notes, id: \.self) { Label($0, systemImage: "info.circle").font(.caption).foregroundStyle(.secondary) }
        }
    }

    private var credentials: some View {
        SectionCard(title: "SVN credentials (optional)", systemImage: "key") {
            TextField("WordPress.org username", text: $add.username, prompt: Text("Case-sensitive"))
            SecureField("SVN password", text: $add.password)
            Text("From your WordPress.org profile → Account & Security → SVN credentials. Stored only in the macOS Keychain; you can also add it later or rely on svn's cached login.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var footer: some View {
        HStack {
            if add.isWorking {
                ProgressView().controlSize(.small)
                Text(add.activity ?? "Working…").foregroundStyle(.secondary)
            } else if let error = add.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red).lineLimit(3).textSelection(.enabled)
            }
            Spacer()
            Button("Cancel", role: .cancel) { dismiss() }
                .keyboardShortcut(.cancelAction)
                .disabled(add.isWorking)
            if add.mode == .checkout || add.mode == .existing {
                Button(add.mode == .checkout ? "Check Out & Add" : "Add Product") {
                    Task { if await add.add() { dismiss() } }
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(add.isWorking || (add.mode == .checkout ? add.remote == nil || add.destination.isEmpty : add.existing == nil))
            }
        }
    }
}
