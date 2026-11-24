import ReleaseDockCore
import SwiftUI

/// Every release attempt, newest first, with the same sections New Release shows.
struct ReleaseHistoryView: View {
    @Environment(AppModel.self) private var model
    @State private var productFilter: UUID?
    @State private var search = ""

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
                VStack(alignment: .leading, spacing: 10) {
                    SearchField(text: $search, prompt: "Version or state")
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            FilterChip(title: "All", isOn: productFilter == nil) { productFilter = nil }
                            ForEach(model.products) { product in
                                FilterChip(title: product.name, isOn: productFilter == product.id) { productFilter = product.id }
                            }
                        }
                    }
                }
                .padding(12)
                if releases.isEmpty {
                    ContentUnavailableView("No releases", systemImage: "clock.arrow.circlepath", description: Text(model.releases.isEmpty ? "Releases you prepare appear here." : "Nothing matches the filter."))
                        .frame(maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 2) {
                            ForEach(releases) { release in
                                HistoryRow(release: release, isSelected: release.id == model.historySelection) {
                                    model.historySelection = release.id
                                }
                            }
                        }
                        .padding(.horizontal, 8)
                        .padding(.bottom, 8)
                    }
                    .arrowKeySelection(releases.map(\.id), selection: Binding(get: { model.historySelection }, set: { model.historySelection = $0 }))
                }
            }
        } detail: {
            if let record = selected {
                ReleaseHistoryDetail(record: record)
                    .id(record.id)
            } else {
                ContentUnavailableView("Select a release", systemImage: "clock.arrow.circlepath", description: Text("Choose a release on the left to see what happened."))
            }
        }
        .navigationTitle("History")
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
}

private struct FilterChip: View {
    let title: String
    let isOn: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.caption.weight(.medium))
                .lineLimit(1)
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .foregroundStyle(isOn ? Color.white : Color.primary)
                .background(isOn ? Color.accentColor : Color.primary.opacity(0.07), in: Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}

private struct HistoryRow: View {
    let release: ReleaseRecord
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        SelectableRow(isSelected: isSelected, action: action) {
            HStack(spacing: 10) {
                ProductGlyph(type: release.productType, size: 26)
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(release.productName) \(release.version ?? "")").font(.callout.weight(.semibold)).lineLimit(1)
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                VStack(alignment: .trailing, spacing: 3) {
                    StatusPill(text: release.state.displayName, color: release.state.tint)
                    if let revision = release.committedRevision {
                        Text("r\(revision)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    private var subtitle: String {
        var what = release.kind == .readmeUpdate ? "Readme update"
            : release.mode == .dryRun ? "Dry run"
            : release.previousVersion.map { "from \($0)" } ?? "First release"
        if let restored = release.restoresVersion { what = "Restores \(restored)" + (release.mode == .dryRun ? " · dry run" : "") }
        return "\(what) · \(release.createdAt.formatted(.relative(presentation: .named)))"
    }
}

/// One release: actions, outcome, then the same review sections as New Release.
private struct ReleaseHistoryDetail: View {
    @Environment(AppModel.self) private var model
    let record: ReleaseRecord
    @State private var busy = false
    @State private var confirmRetry = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if record.state == .completed, record.mode != .dryRun {
                        ReleaseOutcomeSection(record: record, product: model.product(record.productID))
                    } else if record.state == .failed || record.state == .cancelled || record.state == .recoveryRequired {
                        statusCard
                    }
                    if record.detected != nil { ReleaseHeroCard(record: record) }
                    ReleaseSummaryTiles(record: record)
                    WhatsNewCard(record: record)
                    if let build = record.build { BuildCard(build: build) }
                    if let report = record.validation { ChecksSection(report: report) }
                    if let preview = record.preview { ChangesSection(preview: preview) }
                    if let inspection = record.inspection { ExcludedSection(inspection: inspection) }
                    packageSection
                    timeline
                }
                .padding(.horizontal, 24)
                .padding(.vertical, 20)
                .frame(maxWidth: 900, alignment: .leading)
            }
        }
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

    private var header: some View {
        HStack(spacing: 12) {
            ProductGlyph(type: record.productType, size: 36)
            VStack(alignment: .leading, spacing: 2) {
                Text("\(record.productName) \(record.version ?? "")").font(.title3.weight(.semibold)).lineLimit(1)
                HStack(spacing: 6) {
                    StatusPill(text: record.state.displayName, color: record.state.tint)
                    Text(([record.kind == .readmeUpdate ? record.kind.displayName : nil, record.mode.displayName, record.createdAt.formatted(date: .abbreviated, time: .shortened)].compactMap { $0 }).joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
            if busy { ProgressView().controlSize(.small) }
            if record.state == .recoveryRequired {
                if record.verifiedNotCommitted {
                    Button("Abandon", role: .destructive) { perform { try await model.pipeline.abandon(record.id) } }
                    Button("Publish Again…") { confirmRetry = true }
                }
                Button("Inspect Repository") { perform { try await model.pipeline.recover(record.id) } }
                    .buttonStyle(.borderedProminent)
            }
            if !record.state.isTerminal, record.state != .recoveryRequired {
                Button("Continue in New Release") { model.open(record) }
                    .buttonStyle(.borderedProminent)
            }
            if let manifest = record.manifestPath {
                Button { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: manifest)]) } label: {
                    Label("Show Manifest", systemImage: "doc.text.magnifyingglass")
                }
                .help("Show the release manifest in Finder")
            }
        }
        .disabled(busy)
        .padding(.horizontal, 24)
        .padding(.vertical, 14)
    }

    private var statusCard: some View {
        HStack(alignment: .top, spacing: 12) {
            TintedSymbol(systemImage: record.state == .recoveryRequired ? "exclamationmark.arrow.triangle.2.circlepath" : record.state == .cancelled ? "xmark" : "exclamationmark.octagon",
                         color: record.state.tint)
            VStack(alignment: .leading, spacing: 4) {
                Text(statusTitle).font(.headline)
                Text(statusDetail).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                ForEach(record.errors, id: \.self) { Text($0).font(.caption.monospaced()).foregroundStyle(.secondary).textSelection(.enabled) }
            }
            Spacer(minLength: 0)
        }
        .card()
    }

    private var statusTitle: String {
        switch record.state {
        case .cancelled: "Cancelled"
        case .recoveryRequired: record.verifiedNotCommitted ? "Verified: the commit is not in the repository" : "The commit outcome is uncertain"
        default: "Stopped"
        }
    }

    private var statusDetail: String {
        switch record.state {
        case .cancelled: "Stopped before commit. Nothing was sent to WordPress.org and the working copy was restored."
        case .recoveryRequired: record.verifiedNotCommitted
            ? "Nothing reached WordPress.org. Publish again after reviewing, or abandon the release."
            : "ReleaseDock never retries blindly. Inspect the repository to find out whether the commit landed."
        default: "Nothing was committed."
        }
    }

    @ViewBuilder private var packageSection: some View {
        if let inspection = record.inspection {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Package")
                GroupedRows {
                    GroupedRow(isFirst: true) { key("Source"); value(record.source.path, mono: true) }
                    GroupedRow { key(inspection.sourceKind == .zip ? "SHA-256" : "Tree digest"); value(inspection.sha256, mono: true) }
                    GroupedRow {
                        key("Files")
                        value("\(inspection.includedFiles.count) included, \(inspection.issues.filter(\.excluded).count) excluded, \(ByteCountFormatter.string(fromByteCount: Int64(inspection.totalUncompressedSize), countStyle: .file))")
                    }
                    if let detected = record.detected {
                        GroupedRow { key("Detected"); value("\(detected.type.displayName) “\(detected.name ?? "?")” in \(detected.mainFile ?? "?")") }
                    }
                    if let commit = record.gitCommit {
                        GroupedRow { key("Git commit"); value(commit, mono: true) }
                    }
                }
            }
        }
    }

    private var timeline: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Timeline")
            GroupedRows {
                ForEach(Array(record.transitions.enumerated()), id: \.offset) { index, transition in
                    GroupedRow(isFirst: index == 0) {
                        Text(transition.at.formatted(date: .omitted, time: .standard))
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)
                        StatusPill(text: transition.to.displayName, color: transition.to.tint)
                        Text(transition.note ?? "").font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                        Spacer(minLength: 0)
                    }
                }
            }
        }
    }

    private func key(_ text: String) -> some View {
        Text(text).frame(width: 110, alignment: .leading)
    }

    private func value(_ text: String, mono: Bool = false) -> some View {
        Text(text)
            .font(mono ? .callout.monospaced() : .callout)
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func perform(_ action: @escaping () async throws -> ReleaseRecord) {
        busy = true
        Task {
            defer { busy = false; model.reload() }
            do { _ = try await action() } catch { model.show(error) }
        }
    }
}
