import ReleaseDockCore
import SwiftUI

/// Overview: what needs you now, what is going live, and every product at a glance.
struct DashboardView: View {
    @Environment(AppModel.self) private var model

    private var needsAttention: [ReleaseRecord] {
        model.releases.filter { $0.state == .recoveryRequired }
    }

    private var waiting: [ReleaseRecord] { model.releasesAwaitingApproval }

    private var goingLive: [ReleaseRecord] {
        model.releases.filter { $0.awaitsDirectory() }
    }

    private var recent: [ReleaseRecord] {
        Array(model.releases.filter { $0.state.isTerminal || $0.state == .recoveryRequired }.prefix(5))
    }

    private var releasedThisWeek: Int {
        let weekAgo = Date().addingTimeInterval(-7 * 86_400)
        return model.releases.filter { $0.state == .completed && $0.mode != .dryRun && ($0.completedAt ?? .distantPast) > weekAgo }.count
    }

    private var headline: String {
        if !needsAttention.isEmpty { return "Needs attention." }
        if !waiting.isEmpty { return "Ready to publish." }
        return model.products.isEmpty ? "Welcome to ReleaseDock." : "Good to ship."
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 8) {
                    PageTitle(title: headline)
                    HStack(spacing: 18) {
                        summary(color: .green, "\(releasedThisWeek) released this week")
                        summary(color: .blue, "\(waiting.count) waiting for approval")
                        summary(color: needsAttention.isEmpty ? .secondary : .orange, "\(needsAttention.count) need attention")
                    }
                }
                banners
                if model.products.isEmpty {
                    emptyState
                } else {
                    DropArea(title: "Drop a ZIP or folder to start a release", subtitle: "ReleaseDock detects the product and version from the package headers.", compact: true) { url in
                        model.startRelease(from: url)
                    } actions: {
                        Button("Choose Folder…") { if let url = PackagePicker.choose(zip: false) { model.startRelease(from: url) } }
                        Button("Choose ZIP…") { if let url = PackagePicker.choose(zip: true) { model.startRelease(from: url) } }
                            .buttonStyle(.borderedProminent)
                    }
                    .padding(.top, 8)
                    if !waiting.isEmpty || !needsAttention.isEmpty || !recent.isEmpty || !goingLive.isEmpty {
                        SectionHeading("Waiting for you")
                        HStack(alignment: .top, spacing: 12) {
                            VStack(spacing: 12) {
                                ForEach(needsAttention + waiting) { release in ResumeCard(release: release) }
                                if needsAttention.isEmpty && waiting.isEmpty {
                                    Label("Nothing waits for approval.", systemImage: "checkmark.circle")
                                        .foregroundStyle(.secondary)
                                        .card()
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .top)
                            ActivityCard(goingLive: goingLive, recent: recent)
                                .frame(maxWidth: .infinity, alignment: .top)
                        }
                    }
                    SectionHeading("Products") {
                        Button("Show All") { model.selection = .products }.buttonStyle(.link)
                    }
                    LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 12) {
                        ForEach(model.products) { product in ProductCard(product: product) }
                    }
                }
            }
            .padding(.horizontal, 28)
            .padding(.vertical, 20)
            .frame(maxWidth: 980)
            .frame(maxWidth: .infinity)
        }
        .navigationTitle("Overview")
        .toolbar {
            ToolbarItem {
                Button { model.wizard.reset(); model.selection = .wizard } label: { Label("New Release", systemImage: "plus") }
                    .disabled(model.products.isEmpty)
                    .help("New Release (⌘N)")
            }
        }
    }

    private func summary(color: Color, _ text: String) -> some View {
        HStack(spacing: 6) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(text)
        }
        .foregroundStyle(.secondary)
    }

    @ViewBuilder private var banners: some View {
        if model.svnInstallation == nil, model.svnProblem != nil {
            ProblemCard(title: "Subversion isn't installed", message: "ReleaseDock uses the svn command to talk to WordPress.org. Install it with Homebrew, then check again.") {
                Button("Copy “brew install subversion”") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString("brew install subversion", forType: .string)
                }
                Button("Check Again") { Task { await model.detectSVN() } }.buttonStyle(.borderedProminent)
            }
        } else if let problem = model.svnProblem {
            ProblemCard(title: "Subversion needs an update", message: problem) {
                Button("Check Again") { Task { await model.detectSVN() } }.buttonStyle(.borderedProminent)
            }
        }
    }

    private var emptyState: some View {
        ContentUnavailableView {
            Label("Add your first product", systemImage: "shippingbox")
        } description: {
            Text("Register a WordPress.org plugin or theme to start releasing.")
        } actions: {
            Button("Add Product…") {
                model.selection = .products
                model.showAddProduct = true
            }
            .buttonStyle(.borderedProminent)
        }
        .card(padding: 30)
    }
}

/// A release that waits for a decision, with the way forward.
private struct ResumeCard: View {
    @Environment(AppModel.self) private var model
    let release: ReleaseRecord

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                ProductGlyph(type: release.productType, productID: release.productID)
                VStack(alignment: .leading, spacing: 1) {
                    Text("\(release.productName) \(release.version ?? "")").font(.headline)
                    Text(detail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                StatusPill(text: release.state == .recoveryRequired ? "Recovery required" : "Ready to approve", color: release.state == .recoveryRequired ? .orange : .blue)
            }
            HStack {
                if let report = release.validation {
                    Text("\(report.results.filter { $0.status == .passed }.count) checks passed · \(report.warnings.count) warning(s)")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button(release.state == .recoveryRequired ? "Inspect…" : "Review & Publish") { model.open(release) }
                    .buttonStyle(.borderedProminent)
            }
        }
        .card()
    }

    private var detail: String {
        let when = release.updatedAt.formatted(.relative(presentation: .named))
        if let preview = release.preview { return "\(release.kind == .readmeUpdate ? "Readme update" : "Prepared") \(when) · \(preview.changes.count) SVN change(s)" }
        return "Updated \(when)"
    }
}

/// Going-live releases first, then recent outcomes.
private struct ActivityCard: View {
    @Environment(AppModel.self) private var model
    let goingLive: [ReleaseRecord]
    let recent: [ReleaseRecord]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(goingLive) { release in
                row(release, symbol: "hourglass", color: .orange, title: "\(release.productName) \(release.version ?? "") is going live",
                    detail: "WordPress.org review period" + (release.directoryStatus.map { " · checked \($0.checkedAt.formatted(.relative(presentation: .named)))" } ?? ""))
            }
            ForEach(recent.filter { r in !goingLive.contains { $0.id == r.id } }) { release in
                row(release, symbol: symbol(release), color: release.state.tint, title: title(release), detail: detail(release))
            }
            if goingLive.isEmpty && recent.isEmpty {
                Text("No releases yet.").foregroundStyle(.secondary).padding(14)
            }
        }
        .padding(.vertical, 6)
        .card(padding: 0)
    }

    private func row(_ release: ReleaseRecord, symbol: String, color: Color, title: String, detail: String) -> some View {
        Button { model.open(release) } label: {
            HStack(alignment: .top, spacing: 12) {
                TintedSymbol(systemImage: symbol, color: color, size: 24, circle: true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.callout.weight(.semibold)).foregroundStyle(.primary)
                    Text(detail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func symbol(_ release: ReleaseRecord) -> String {
        switch release.state {
        case .completed: "checkmark"
        case .failed: "xmark"
        case .cancelled: "minus"
        default: "exclamationmark"
        }
    }

    private func title(_ release: ReleaseRecord) -> String {
        let name = "\(release.productName) \(release.version ?? "")"
        switch release.state {
        case .completed where release.mode == .dryRun: return "\(name) dry run passed"
        case .completed where release.kind == .readmeUpdate: return "\(name) readme updated"
        case .completed: return "\(name) published"
        case .failed: return "\(name) failed"
        case .cancelled: return "\(name) cancelled"
        default: return "\(name) needs recovery"
        }
    }

    private func detail(_ release: ReleaseRecord) -> String {
        var parts: [String] = []
        if let revision = release.committedRevision { parts.append("r\(revision)") }
        if release.directoryStatus?.state == .live { parts.append("live") }
        if release.state == .cancelled { parts.append("stopped before commit") }
        if release.state == .failed, let first = release.validation?.blockingResults.first { parts.append(first.name) }
        parts.append(release.updatedAt.formatted(.relative(presentation: .named)))
        return parts.joined(separator: " · ")
    }
}

/// A product at a glance with its released version and the way to a new release.
private struct ProductCard: View {
    @Environment(AppModel.self) private var model
    let product: Product

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                ProductGlyph(type: product.type, productID: product.id)
                VStack(alignment: .leading, spacing: 1) {
                    Text(product.name).font(.headline)
                    Text("\(product.type.displayName) · \(product.slug)").font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                if product.currentVersion != nil { StatusPill(text: "Live", color: .green) }
            }
            HStack(alignment: .bottom) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(product.currentVersion ?? "—")
                        .font(.system(size: 28, weight: .bold).monospacedDigit())
                        .tracking(-0.6)
                    Text(lastRelease).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button("New Release…") {
                    model.wizard.reset()
                    model.wizardProductID = product.id
                    model.selection = .wizard
                }
            }
        }
        .card()
    }

    private var lastRelease: String {
        guard let last = product.lastSuccessfulRelease else { return "No release from ReleaseDock yet · \(product.releaseMode.displayName)" }
        return "Released \(last.date.formatted(.relative(presentation: .named)))\(last.revision.map { " · r\($0)" } ?? "") · \(product.releaseMode.displayName)"
    }
}
