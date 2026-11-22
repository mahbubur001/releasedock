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
                    SearchField(text: $search, prompt: "Search products")
                    Button { model.showAddProduct = true } label: { Image(systemName: "plus") }
                        .help("Add Product")
                }
                .padding(12)
                Divider()
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
                ProductDetailView(product: product) { editing = ProductEditorModel(model: model, product: product) }
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
                ProductTypeIcon(type: product.type)
                    .font(.title3)
                    .frame(width: 32, height: 32)
                    .background((product.type == .plugin ? Color.accentColor : Color.purple).opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    Text(product.name).font(.callout.weight(.semibold)).lineLimit(1)
                    Text("\(product.type.displayName) · \(product.slug)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                VStack(alignment: .trailing, spacing: 2) {
                    Text(product.currentVersion ?? "—").font(.callout.monospacedDigit())
                    if model.releases.contains(where: { $0.productID == product.id && $0.state == .recoveryRequired }) {
                        Text("Recovery").font(.caption2.weight(.semibold)).foregroundStyle(.orange)
                    }
                }
            }
        }
    }
}

struct ProductDetailView: View {
    @Environment(AppModel.self) private var model
    let product: Product
    let onEdit: () -> Void
    @State private var confirmRevert = false
    @State private var reverting = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .top, spacing: 14) {
                    ProductTypeIcon(type: product.type)
                        .font(.title)
                        .frame(width: 52, height: 52)
                        .background((product.type == .plugin ? Color.accentColor : Color.purple).opacity(0.14), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(product.name).font(.title2.bold()).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                        Text("\(product.type.displayName) · \(product.slug) · \(product.currentVersion.map { "v\($0)" } ?? "no release")")
                            .foregroundStyle(.secondary)
                        HStack(spacing: 8) {
                            Button {
                                model.wizardProductID = product.id
                                model.selection = .wizard
                            } label: { Label("New Release", systemImage: "paperplane.fill") }
                            .buttonStyle(.borderedProminent)
                            Button { onEdit() } label: { Label("Edit", systemImage: "pencil") }
                        }
                        .padding(.top, 6)
                    }
                    Spacer(minLength: 0)
                }
                SectionCard(title: "Repository") {
                    KeyValueRow(key: "SVN URL", value: product.svnRepositoryURL.absoluteString, monospaced: true)
                    KeyValueRow(key: "Working copy", value: product.workingCopyPath, monospaced: true)
                    KeyValueRow(key: "Development folder", value: product.localDevelopmentPath ?? "—", monospaced: true)
                    KeyValueRow(key: "SVN username", value: product.svnUsername ?? "Not set")
                    KeyValueRow(key: "SVN password", value: model.hasStoredPassword(for: product) ? "Stored in Keychain" : "Not stored")
                    if let github = product.githubRepositoryURL { KeyValueRow(key: "GitHub", value: github.absoluteString) }
                    HStack {
                        Button("Show Working Copy in Finder") { NSWorkspace.shared.open(product.workingCopyURL) }
                            .disabled(!FileManager.default.fileExists(atPath: product.workingCopyPath))
                        Button(reverting ? "Reverting…" : "Revert Working Copy…", role: .destructive) { confirmRevert = true }
                            .disabled(reverting || !FileManager.default.fileExists(atPath: product.workingCopyURL.appendingPathComponent(".svn").path))
                    }
                }
                SectionCard(title: "Releases") {
                    KeyValueRow(key: "Current version", value: product.currentVersion ?? "Unknown")
                    KeyValueRow(key: "Last release", value: product.lastSuccessfulRelease.map { "\($0.version) (r\($0.revision.map(String.init) ?? "?")) on \($0.date.formatted(date: .abbreviated, time: .shortened))" } ?? "—")
                    KeyValueRow(key: "Release mode", value: product.releaseMode.displayName)
                    KeyValueRow(key: "Automation", value: product.isAutomaticPublishingAuthorized ? "Enabled" : "Off")
                }
                SectionCard(title: "History") {
                    let history = model.releases(for: product.id)
                    if history.isEmpty { Text("No releases yet.").foregroundStyle(.secondary) }
                    ForEach(history.prefix(10)) { release in
                        HStack {
                            Text(release.version ?? "—").monospacedDigit()
                            Text(release.createdAt.formatted(date: .abbreviated, time: .shortened)).foregroundStyle(.secondary)
                            Spacer()
                            if let r = release.committedRevision { Text("r\(r)").foregroundStyle(.secondary) }
                            StateBadge(state: release.state)
                        }
                    }
                }
            }
            .padding(20)
        }
        .confirmationDialog("Revert all local changes in the working copy?", isPresented: $confirmRevert) {
            SwiftUI.Button("Back Up and Revert", role: .destructive) { revert() }
        } message: {
            Text("ReleaseDock first copies every changed, added and unversioned file plus the svn diff into the Backups folder, then reverts and updates the working copy. If any backup step fails, nothing is reverted.")
        }
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
