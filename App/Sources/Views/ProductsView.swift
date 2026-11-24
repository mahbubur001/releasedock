import AppKit
import ReleaseDockCore
import SwiftUI

struct ProductsView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: UUID?
    @State private var editing: ProductEditorModel?
    @State private var confirmDelete: Product?

    @State private var search = ""

    private var filtered: [Product] {
        search.isEmpty ? model.products : model.products.filter {
            $0.name.localizedCaseInsensitiveContains(search) || $0.slug.localizedCaseInsensitiveContains(search)
        }
    }

    var body: some View {
        MasterDetailLayout {
            VStack(spacing: 0) {
                HStack(spacing: 8) {
                    SearchField(text: $search, prompt: "Filter")
                    Button { model.showAddProduct = true } label: { Image(systemName: "plus") }
                        .buttonStyle(.borderless)
                        .help("Add Product (⇧⌘N)")
                }
                .padding(12)
                if model.products.isEmpty {
                    ContentUnavailableView {
                        Label("No products", systemImage: "shippingbox")
                    } description: {
                        Text("Add a plugin or theme to get started.")
                    } actions: {
                        Button("Add Product") { model.showAddProduct = true }
                            .buttonStyle(.borderedProminent)
                    }
                    .frame(maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 4) {
                            ForEach(filtered) { product in
                                ProductRow(product: product, isSelected: product.id == selection) { selection = product.id }
                                    .contextMenu {
                                        SwiftUI.Button("Edit…") { editing = ProductEditorModel(model: model, product: product) }
                                        SwiftUI.Button("Delete…", role: .destructive) { confirmDelete = product }
                                    }
                            }
                        }
                        .padding(8)
                    }
                    .arrowKeySelection(filtered.map(\.id), selection: $selection)
                }
            }
        } detail: {
            if let product = model.product(selection) {
                ProductDetailView(product: product) {
                    editing = ProductEditorModel(model: model, product: product)
                } onDelete: {
                    confirmDelete = product
                }
            } else {
                ContentUnavailableView("Select a product", systemImage: "shippingbox", description: Text("Choose a product on the left."))
            }
        }
        .navigationTitle("Products")
        .sheet(item: $editing) { editor in
            ProductEditorView(editor: editor)
        }
        .confirmationDialog("Delete \(confirmDelete?.name ?? "")?", isPresented: .init(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } })) {
            SwiftUI.Button("Delete Product and History", role: .destructive) {
                if let product = confirmDelete {
                    do {
                        try? model.removeCredentials(for: product)
                        try model.productService.delete(product.id)
                        model.reload()
                    } catch { model.show(error) }
                }
            }
        } message: {
            Text("The product, its release history and its stored SVN password are removed. The SVN working copy and release manifests on disk are kept.")
        }
        .onAppear { if selection == nil { selection = model.products.first?.id } }
        .onChange(of: model.products.count) { selection = model.products.last { $0.updatedAt == model.products.map(\.updatedAt).max() }?.id ?? selection }
    }
}

private struct ProductRow: View {
    @Environment(AppModel.self) private var model
    let product: Product
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        SelectableRow(isSelected: isSelected, action: action) {
            HStack(spacing: 10) {
                ProductGlyph(type: product.type, size: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(product.name).font(.callout.weight(.semibold)).lineLimit(1)
                    Text("\(product.type.displayName) · \(product.slug)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                VStack(alignment: .trailing, spacing: 2) {
                    Text(product.currentVersion ?? "—").font(.callout.weight(.semibold).monospacedDigit())
                    if model.releases.contains(where: { $0.productID == product.id && $0.state == .recoveryRequired }) {
                        Text("Recovery").font(.caption2.weight(.semibold)).foregroundStyle(.orange)
                    }
                }
            }
        }
    }
}

/// One product: identity and actions, then grouped settings and recent releases.
struct ProductDetailView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let product: Product
    let onEdit: () -> Void
    let onDelete: () -> Void
    @State private var confirmRevert = false
    @State private var reverting = false
    @State private var bumpTarget: VersionBumpTarget?
    @State private var restoring = false

    private var hasWorkingCopy: Bool { FileManager.default.fileExists(atPath: product.workingCopyURL.appendingPathComponent(".svn").path) }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                header
                SectionHeading("Repository")
                GroupedRows {
                    GroupedRow(isFirst: true) {
                        label("SVN URL")
                        mono(product.svnRepositoryURL.absoluteString)
                        CopyButton(text: product.svnRepositoryURL.absoluteString)
                    }
                    GroupedRow {
                        label("Working copy")
                        mono((product.workingCopyPath as NSString).abbreviatingWithTildeInPath)
                        Button("Show in Finder") { NSWorkspace.shared.open(product.workingCopyURL) }
                            .fixedSize()
                            .disabled(!FileManager.default.fileExists(atPath: product.workingCopyPath))
                    }
                    GroupedRow {
                        label("Development folder")
                        if let dev = product.localDevelopmentURL {
                            mono((dev.path as NSString).abbreviatingWithTildeInPath)
                            Button("Bump Version…") { bumpTarget = VersionBumpTarget(folder: dev) }.fixedSize()
                        } else {
                            Text("Not set").foregroundStyle(.tertiary).frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    GroupedRow {
                        label("Credentials")
                        HStack(spacing: 6) {
                            Text(product.svnUsername ?? "No username")
                            if model.hasStoredPassword(for: product) {
                                Label("Saved in Keychain", systemImage: "key.fill").foregroundStyle(.green)
                            } else {
                                Text("· password not stored").foregroundStyle(.secondary)
                            }
                        }
                        .font(.callout)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                SectionHeading("Release")
                GroupedRows {
                    GroupedRow(isFirst: true) {
                        label("Mode")
                        value(product.releaseMode.displayName + (product.releaseMode == .automatic ? (product.automationEnabled ? " · allowed" : " · not allowed yet") : ""))
                    }
                    if let live = product.currentVersion {
                        GroupedRow {
                            label("Restore")
                            value("Problem in \(live)? Ship an older version's code again as a new version.")
                            Button("Restore Older Version…") { restoring = true }
                                .fixedSize()
                                .disabled(model.wizard.isRunning)
                        }
                    }
                    GroupedRow {
                        label("Build command")
                        if let build = product.effectiveBuildCommand { mono(build) } else { value("None — folders are released as they are", faded: true) }
                    }
                    if product.type == .plugin {
                        GroupedRow {
                            label("Assets folder")
                            if let assets = product.assetsPath { mono((assets as NSString).abbreviatingWithTildeInPath) } else { value("Not synced", faded: true) }
                        }
                    }
                    GroupedRow {
                        label("GitHub")
                        if let github = product.githubRepositoryURL {
                            Button {
                                openURL(github)
                            } label: {
                                Label(github.path.trimmingCharacters(in: CharacterSet(charactersIn: "/")), systemImage: "arrow.up.right.square")
                            }
                            .buttonStyle(InlineLinkButtonStyle())
                            .linkCursor()
                            Spacer()
                            if product.githubRelease?.enabled == true {
                                StatusPill(text: "Releases \(product.githubRelease?.tagPrefix ?? "")<version>", color: .green)
                            }
                        } else {
                            value("Not linked", faded: true)
                        }
                    }
                }
                history
                SectionHeading("Working Copy")
                GroupedRows {
                    GroupedRow(isFirst: true) {
                        value("Discard local changes in the working copy and match the server. Everything changed is backed up first.")
                        Button(reverting ? "Reverting…" : "Revert Working Copy…", role: .destructive) { confirmRevert = true }
                            .fixedSize()
                            .disabled(reverting || !hasWorkingCopy)
                    }
                }
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 20)
            .frame(maxWidth: 860, alignment: .leading)
        }
        .confirmationDialog("Revert all local changes in the working copy?", isPresented: $confirmRevert) {
            SwiftUI.Button("Back Up and Revert", role: .destructive) { revert() }
        } message: {
            Text("ReleaseDock first copies every changed, added and unversioned file plus the svn diff into the Backups folder, then reverts and updates the working copy. If any backup step fails, nothing is reverted.")
        }
        .sheet(item: $bumpTarget) { target in
            VersionBumpSheet(product: product, folder: target.folder)
        }
        .sheet(isPresented: $restoring) {
            RestoreVersionSheet(product: product)
        }
        #if DEBUG
        // UI verification: RELEASEDOCK_UI_TEST_RESTORE=1 opens the restore sheet; =auto also prepares it.
        .onAppear { if ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_RESTORE"] != nil, product.currentVersion != nil { restoring = true } }
        #endif
    }

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 14) {
                identity
                Spacer(minLength: 12)
                actions
            }
            VStack(alignment: .leading, spacing: 12) {
                identity
                HStack(spacing: 8) { actions }
            }
        }
    }

    private var identity: some View {
        HStack(spacing: 14) {
            ProductGlyph(type: product.type, size: 52)
            VStack(alignment: .leading, spacing: 4) {
                Text(product.name).font(.title2.weight(.bold)).lineLimit(1)
                HStack(spacing: 6) {
                    Text("\(product.type.displayName) · \(product.slug)").foregroundStyle(.secondary).lineLimit(1)
                    if let version = product.currentVersion { StatusPill(text: "Live \(version)", color: .green).fixedSize() }
                }
            }
        }
        .fixedSize()
    }

    @ViewBuilder private var actions: some View {
            Menu {
                SwiftUI.Button("Show Working Copy in Finder") { NSWorkspace.shared.open(product.workingCopyURL) }
                SwiftUI.Button("Revert Working Copy…") { confirmRevert = true }.disabled(!hasWorkingCopy)
                SwiftUI.Button("Restore Older Version…") { restoring = true }.disabled(product.currentVersion == nil || model.wizard.isRunning)
                Divider()
                SwiftUI.Button("Delete Product…", role: .destructive, action: onDelete)
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("More")
            Button("Edit…", action: onEdit).fixedSize()
            Button("New Release…") {
                model.wizard.reset()
                model.wizardProductID = product.id
                model.selection = .wizard
            }
            .buttonStyle(.borderedProminent)
            .fixedSize()
    }

    @ViewBuilder private var history: some View {
        let releases = model.releases(for: product.id)
        SectionHeading("History") {
            if !releases.isEmpty {
                Button("Show All") {
                    model.historySelection = releases.first?.id
                    model.selection = .history
                }
                .buttonStyle(.link)
            }
        }
        GroupedRows {
            if releases.isEmpty {
                GroupedRow(isFirst: true) { value("No releases yet.", faded: true) }
            }
            ForEach(Array(releases.prefix(6).enumerated()), id: \.element.id) { index, release in
                GroupedRow(isFirst: index == 0) {
                    Button { model.open(release) } label: {
                        HStack(spacing: 10) {
                            Text(release.version ?? "—").font(.callout.weight(.semibold).monospacedDigit())
                            if release.kind == .readmeUpdate { Text("readme").font(.caption).foregroundStyle(.secondary) }
                            Text(release.createdAt.formatted(date: .abbreviated, time: .shortened)).font(.callout).foregroundStyle(.secondary)
                            Spacer()
                            if let revision = release.committedRevision { Text("r\(revision)").font(.caption.monospaced()).foregroundStyle(.secondary) }
                            StatusPill(text: release.state.displayName, color: release.state.tint)
                            Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func label(_ text: String) -> some View {
        Text(text).lineLimit(1).frame(width: 130, alignment: .leading)
    }

    private func value(_ text: String, faded: Bool = false) -> some View {
        Text(text).font(.callout).foregroundStyle(faded ? .tertiary : .secondary).frame(maxWidth: .infinity, alignment: .leading)
    }

    private func mono(_ text: String) -> some View {
        Text(text)
            .font(.callout.monospaced())
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .lineLimit(1)
            .truncationMode(.middle)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func revert() {
        reverting = true
        Task {
            defer { reverting = false }
            do {
                if let backup = try await model.pipeline.revertWorkingCopy(productID: product.id) {
                    NSWorkspace.shared.activateFileViewerSelecting([backup])
                }
            } catch {
                model.show(error, title: "Could not revert the working copy")
            }
        }
    }
}

struct CopyButton: View {
    let text: String

    var body: some View {
        Button {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        } label: {
            Image(systemName: "doc.on.doc")
        }
        .buttonStyle(.borderless)
        .help("Copy")
    }
}
