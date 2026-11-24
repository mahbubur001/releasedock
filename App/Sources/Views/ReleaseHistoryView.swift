import ReleaseDockCore
import SwiftUI

struct ReleaseHistoryView: View {
    @Environment(AppModel.self) private var model
    @State private var productFilter: UUID?
    @State private var search = ""
    @State private var busy = false
    @State private var confirmRetry = false

    private var releases: [ReleaseRecord] {
        model.releases.filter { release in
            (productFilter == nil || release.productID == productFilter)
                && (search.isEmpty
                    || release.productName.localizedCaseInsensitiveContains(search)
                    || (release.version ?? "").localizedCaseInsensitiveContains(search)
                    || release.state.displayName.localizedCaseInsensitiveContains(search))
        }
    }

    private var selected: ReleaseRecord? {
        model.releases.first { $0.id == model.historySelection }
    }

    var body: some View {
        MasterDetailLayout {
            VStack(spacing: 0) {
                VStack(spacing: 8) {
                    Picker("Product", selection: $productFilter) {
                        Text("All products").tag(UUID?.none)
                        ForEach(model.products) { Text($0.name).tag(Optional($0.id)) }
                    }
                    .labelsHidden()
                    SearchField(text: $search, prompt: "Search version or state")
                }
                .padding(12)
                Divider()
                if releases.isEmpty {
                    ContentUnavailableView("No releases", systemImage: "clock.arrow.circlepath", description: Text(model.releases.isEmpty ? "Releases you prepare appear here." : "Nothing matches the filter."))
                        .frame(maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 4) {
                            ForEach(releases) { release in
                                ReleaseRow(release: release, isSelected: release.id == model.historySelection) {
                                    model.historySelection = release.id
                                }
                            }
                        }
                        .padding(8)
                    }
                    .arrowKeySelection(releases.map(\.id), selection: Binding(get: { model.historySelection }, set: { model.historySelection = $0 }))
                }
            }
        } detail: {
            if let record = selected {
                VStack(spacing: 0) {
                    actionBar(for: record)
                    Divider()
                    ScrollView {
                        ReleaseReportView(record: record, product: model.product(record.productID), showsHeader: false)
                            .padding(20)
                    }
                }
            } else {
                ContentUnavailableView("Select a release", systemImage: "clock.arrow.circlepath", description: Text("Choose a release on the left to see its report."))
            }
        }
        .navigationTitle("Release History")
        .toolbar {
            ToolbarItem {
                Button { model.reload() } label: { Label("Refresh", systemImage: "arrow.clockwise") }
                    .help("Reload release history")
            }
        }
        .onAppear(perform: selectFirstIfNeeded)
        .onChange(of: productFilter) { selectFirstIfNeeded() }
    }

    private func selectFirstIfNeeded() {
        if !releases.contains(where: { $0.id == model.historySelection }) {
            model.historySelection = releases.first?.id
        }
    }

    private func actionBar(for record: ReleaseRecord) -> some View {
        HStack(spacing: 10) {
            ProductTypeIcon(type: record.productType).font(.title3)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(record.productName) \(record.version ?? "")").font(.headline).lineLimit(1)
                Text(record.createdAt.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary)
            }
            StateBadge(state: record.state)
            Spacer()
            if busy { ProgressView().controlSize(.small) }
            if record.state == .recoveryRequired {
                Button("Inspect Repository") { perform { try await model.pipeline.recover(record.id) } }
                    .buttonStyle(.borderedProminent)
                if record.verifiedNotCommitted {
                    Button("Abandon", role: .destructive) { perform { try await model.pipeline.abandon(record.id) } }
                    Button("Publish Again…") { confirmRetry = true }
                }
            }
            if !record.state.isTerminal, !record.state.mayHaveCommitted {
                Button("Cancel Release", role: .destructive) { perform { try await model.pipeline.cancel(record.id) } }
            }
            if let manifest = record.manifestPath {
                Button { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: manifest)]) } label: {
                    Label("Manifest", systemImage: "doc.text.magnifyingglass")
                }
                .help("Show the release manifest in Finder")
            }
        }
        .disabled(busy)
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .confirmationDialog("Publish \(record.productName) \(record.version ?? "") again?", isPresented: $confirmRetry) {
            SwiftUI.Button("Publish") {
                perform {
                    _ = try await model.pipeline.approve(record.id, kind: .user)
                    return try await model.pipeline.publish(record.id)
                }
            }
        } message: {
            Text("The repository was inspected and the previous commit did not land.")
        }
    }

    private func perform(_ action: @escaping () async throws -> ReleaseRecord) {
        busy = true
        Task {
            defer { busy = false; model.reload() }
            do { _ = try await action() } catch { model.show(error) }
        }
    }
}

private struct ReleaseRow: View {
    let release: ReleaseRecord
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        SelectableRow(isSelected: isSelected, action: action) {
            HStack(alignment: .top, spacing: 10) {
                ProductTypeIcon(type: release.productType).font(.title3).padding(.top, 2)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(release.productName).font(.callout.weight(.semibold)).lineLimit(1)
                        Spacer(minLength: 4)
                        StateBadge(state: release.state)
                    }
                    HStack(spacing: 6) {
                        Text(release.version ?? "—").font(.callout.monospacedDigit())
                        if release.kind == .readmeUpdate {
                            Text("readme update").font(.caption).foregroundStyle(.secondary)
                        } else if let previous = release.previousVersion, release.version != nil {
                            Text("from \(previous)").font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 4)
                        if let revision = release.committedRevision {
                            Text("r\(revision)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                        }
                    }
                    Text("\(release.mode.displayName) · \(release.createdAt.formatted(.relative(presentation: .named)))")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}
