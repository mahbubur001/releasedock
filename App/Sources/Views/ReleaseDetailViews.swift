import AppKit
import ReleaseDockCore
import SwiftUI

// Sections that describe one release. New Release shows them while reviewing and after
// publishing; History shows the same sections for past releases.

extension ReleasePreview {
    /// Changes as a person reads them: a copied tag appears once, not once per file inside it.
    var displayedChanges: [PreviewChange] {
        let copies = changes.filter(\.isCopy).map(\.path)
        return changes.filter { change in !copies.contains { change.path.hasPrefix($0 + "/") } }
    }
}

/// Product, source and the version change (or the readme update) at a glance.
struct ReleaseHeroCard: View {
    let record: ReleaseRecord

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 16) {
                identity
                Spacer(minLength: 16)
                versions
            }
            VStack(alignment: .leading, spacing: 14) {
                identity
                versions
            }
        }
        .card(padding: 20)
    }

    private var identity: some View {
        HStack(spacing: 16) {
            ProductGlyph(type: record.productType, size: 52, productID: record.productID)
            VStack(alignment: .leading, spacing: 3) {
                Text(record.productName).font(.title2.weight(.semibold)).lineLimit(1)
                Text(subtitle).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
        }
    }

    @ViewBuilder private var versions: some View {
        if record.kind == .readmeUpdate {
            versionColumn(record.version ?? "—", caption: "Released version · code unchanged")
        } else {
            HStack(spacing: 14) {
                versionColumn(record.repository?.currentVersion ?? record.previousVersion ?? "—", caption: record.repository?.currentVersionSource ?? "Current", faded: true)
                Image(systemName: "arrow.right").font(.title3.weight(.semibold)).foregroundStyle(.green)
                versionColumn(record.version ?? "—", caption: record.restoresVersion.map { "Code of \($0)" } ?? (record.productType == .plugin ? "From plugin header" : "From style.css"))
            }
        }
    }

    private var subtitle: String {
        let source = record.restoresVersion.map { "Restores the \($0) code" } ?? URL(fileURLWithPath: record.source.path).lastPathComponent
        return [record.productType.displayName, record.kind == .readmeUpdate ? record.kind.displayName : record.mode.displayName, source]
            .joined(separator: " · ")
    }

    private func versionColumn(_ version: String, caption: String, faded: Bool = false) -> some View {
        VStack(spacing: 2) {
            Text(version)
                .font(.system(size: 28, weight: .bold).monospacedDigit())
                .tracking(-0.6)
                .foregroundStyle(faded ? .tertiary : .primary)
                .lineLimit(1)
                .fixedSize()
            Text(caption).font(.caption2).foregroundStyle(.secondary).lineLimit(1).fixedSize()
        }
    }
}

/// Checks, package and SVN change counts.
struct ReleaseSummaryTiles: View {
    let record: ReleaseRecord

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) { tiles }
            VStack(spacing: 8) { tiles }
        }
    }

    @ViewBuilder private var tiles: some View {
        Group {
            if let report = record.validation {
                let passed = report.results.filter { $0.status == .passed }
                let checks = Set(report.results.map(\.check))
                let failedChecks = Set(report.failures.map(\.check))
                tile("checkmark.shield", color: report.isBlocking ? .red : .green,
                     title: report.isBlocking ? "\(failedChecks.count) check(s) failed" : "\(checks.count - failedChecks.count) of \(checks.count) checks passed",
                     detail: report.warnings.isEmpty ? "\(passed.count) results passed" : "\(report.warnings.count) warning(s)")
            }
            if let inspection = record.inspection {
                let excluded = inspection.issues.filter(\.excluded).count
                tile("shippingbox", color: .blue,
                     title: "\(inspection.includedFiles.count) files · \(ByteCountFormatter.string(fromByteCount: Int64(inspection.totalUncompressedSize), countStyle: .file))",
                     detail: excluded == 0 ? "Nothing excluded" : "\(excluded) excluded automatically")
            }
            if let preview = record.preview {
                let changes = preview.displayedChanges
                let copies = changes.filter(\.isCopy)
                tile("arrow.left.arrow.right", color: .orange, title: "\(changes.count) SVN change(s)",
                     detail: copies.isEmpty ? "\(changes.filter { $0.kind == .modified }.count) modified" : "trunk + \(copies.map(\.path).joined(separator: ", "))")
            }
        }
    }

    private func tile(_ symbol: String, color: Color, title: String, detail: String) -> some View {
        HStack(spacing: 12) {
            TintedSymbol(systemImage: symbol, color: color)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.callout.weight(.semibold)).lineLimit(1).fixedSize()
                Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(1).fixedSize()
            }
            Spacer(minLength: 0)
        }
        .card(padding: 14)
    }
}

/// The changelog entry and upgrade notice for the released version.
struct WhatsNewCard: View {
    let record: ReleaseRecord

    var body: some View {
        if let version = record.version, let readme = record.detected?.readme, let changelog = readme.changelogEntry(for: version) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "text.badge.star").foregroundStyle(Color.accentColor).font(.title3)
                VStack(alignment: .leading, spacing: 6) {
                    Text("What's new in \(version)").font(.headline)
                    Text(changelog).font(.callout.monospaced()).foregroundStyle(.secondary).textSelection(.enabled)
                    if let notice = readme.upgradeNotice(for: version) {
                        Label(notice, systemImage: "exclamationmark.bubble").font(.callout)
                    }
                }
                Spacer(minLength: 0)
            }
            .card()
        }
    }
}

/// Problems first, with their fixes; passed checks folded away.
struct ChecksSection: View {
    let report: ValidationReport
    @State private var showPassed = false

    private var problems: [ValidationResult] {
        let rank: (ValidationStatus) -> Int = { [.failed: 0, .warning: 1, .skipped: 2][$0] ?? 3 }
        return report.results.filter { $0.status != .passed }.sorted { rank($0.status) < rank($1.status) }
    }

    private var passed: [ValidationResult] { report.results.filter { $0.status == .passed } }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Checks")
            GroupedRows {
                ForEach(Array(problems.enumerated()), id: \.element.id) { index, result in
                    GroupedRow(isFirst: index == 0) { ProblemRow(result: result) }
                }
                GroupedRow(isFirst: problems.isEmpty) {
                    Button {
                        withAnimation(.snappy(duration: 0.2)) { showPassed.toggle() }
                    } label: {
                        HStack {
                            Image(systemName: "chevron.right")
                                .font(.caption.weight(.bold))
                                .rotationEffect(.degrees(showPassed ? 90 : 0))
                            Text("\(passed.count) passed").fontWeight(.medium)
                            Spacer()
                            Text(report.isBlocking ? "\(report.blockingResults.count) blocking" : "All required checks passed")
                                .font(.caption).foregroundStyle(report.isBlocking ? .red : .secondary)
                        }
                        .foregroundStyle(.secondary)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                if showPassed {
                    LazyVGrid(columns: [GridItem(.flexible(), alignment: .leading), GridItem(.flexible(), alignment: .leading)], alignment: .leading, spacing: 6) {
                        ForEach(passed) { result in
                            Label {
                                Text(result.message).lineLimit(1).truncationMode(.tail).help(result.message)
                            } icon: {
                                Image(systemName: "checkmark").foregroundStyle(.green).font(.caption.weight(.bold))
                            }
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.bottom, 12)
                }
            }
        }
    }
}

private struct ProblemRow: View {
    let result: ValidationResult

    var body: some View {
        ValidationStatusIcon(status: result.status)
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(result.name).fontWeight(.medium)
                if !result.check.isRequired { StatusPill(text: "Optional") }
            }
            Text(result.message).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
            if let path = result.filePath {
                Text(path).font(.caption.monospaced()).foregroundStyle(.secondary)
            }
            if let suggestion = result.suggestion {
                Label(suggestion, systemImage: "wrench.and.screwdriver")
                    .font(.caption)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 6))
            }
        }
        Spacer(minLength: 0)
    }
}

/// SVN changes on the left, the selected file's diff on the right.
struct ChangesSection: View {
    let preview: ReleasePreview
    @State private var selection: String?

    private var sections: [String: String] { UnifiedDiff.sections(preview.diffExcerpt) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Changes to commit") {
                Text("Your approval covers exactly this preview").font(.caption).foregroundStyle(.secondary)
            }
            HStack(alignment: .top, spacing: 0) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 1) {
                        ForEach(preview.displayedChanges) { change in
                            ChangeRow(change: change, isSelected: change.path == current) { selection = change.path }
                        }
                    }
                    .padding(6)
                }
                .frame(width: 280)
                Divider()
                DiffView(text: current.flatMap { path in sections.first { $0.key.hasSuffix("/" + path) || $0.key == path }?.value }, change: preview.changes.first { $0.path == current })
            }
            .frame(height: 280)
            .card(padding: 0)
            if preview.diffTruncated {
                Text("The diff was shortened for display; the commit contains every change.").font(.caption).foregroundStyle(.secondary)
            }
            DisclosureGroup("Commit message") {
                Text(preview.commitMessage).font(.callout.monospaced()).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(.callout)
        }
    }

    private var current: String? {
        selection ?? preview.displayedChanges.first { !$0.isDirectory && !$0.isCopy }?.path ?? preview.displayedChanges.first?.path
    }
}

private struct ChangeRow: View {
    let change: PreviewChange
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Text(letter)
                    .font(.caption.monospaced().weight(.bold))
                    .foregroundStyle(color)
                    .frame(width: 18, height: 18)
                    .background(color.opacity(0.16), in: RoundedRectangle(cornerRadius: 4))
                Text(change.path + (change.isDirectory ? "/" : ""))
                    .font(.caption.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(isSelected ? Color.primary.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(change.path)
    }

    private var letter: String {
        if change.isCopy { return "C" }
        switch change.kind {
        case .added: return "A"
        case .deleted: return "D"
        case .modified, .replaced: return "M"
        case .propertyChanged: return "P"
        }
    }

    private var color: Color {
        switch change.kind {
        case .added: .green
        case .deleted: .red
        default: .orange
        }
    }
}

private struct DiffView: View {
    let text: String?
    let change: PreviewChange?

    var body: some View {
        ScrollView([.vertical, .horizontal]) {
            if let text {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(text.split(separator: "\n", omittingEmptySubsequences: false).enumerated()), id: \.offset) { _, line in
                        Text(line.isEmpty ? " " : String(line))
                            .font(.caption.monospaced())
                            .foregroundStyle(line.hasPrefix("@@") ? Color.accentColor : .primary)
                            .padding(.horizontal, 10)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(background(line))
                    }
                }
                .padding(.vertical, 8)
                .textSelection(.enabled)
            } else {
                Text(placeholder)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.primary.opacity(0.025))
    }

    private var placeholder: String {
        guard let change else { return "Select a change." }
        if change.isCopy { return "\(change.path) is copied from trunk in the same commit." }
        if change.isDirectory { return "Directory \(change.kind.rawValue)." }
        return "No text diff for this file (binary or unchanged content)."
    }

    private func background(_ line: Substring) -> Color {
        if line.hasPrefix("+") && !line.hasPrefix("+++") { return .green.opacity(0.13) }
        if line.hasPrefix("-") && !line.hasPrefix("---") { return .red.opacity(0.12) }
        return .clear
    }
}

/// Splits `svn diff` output into per-file sections keyed by the `Index:` path.
enum UnifiedDiff {
    static func sections(_ text: String) -> [String: String] {
        var result: [String: String] = [:]
        var current: String?
        var lines: [Substring] = []
        func flush() {
            if let current { result[current] = lines.drop { !$0.hasPrefix("@@") }.joined(separator: "\n") }
            lines = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.hasPrefix("Index: ") {
                flush()
                current = String(line.dropFirst("Index: ".count))
            } else {
                lines.append(line)
            }
        }
        flush()
        return result
    }
}

/// What the release left out, and why.
struct ExcludedSection: View {
    let inspection: PackageInspection

    var body: some View {
        let excluded = inspection.issues.filter(\.excluded)
        if !excluded.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Excluded from the release")
                GroupedRows {
                    ForEach(Array(excluded.prefix(8).enumerated()), id: \.offset) { index, issue in
                        GroupedRow(isFirst: index == 0) {
                            Text(issue.path ?? "—").font(.callout.monospaced()).lineLimit(1).truncationMode(.middle)
                            Spacer()
                            Text(issue.message).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    if excluded.count > 8 {
                        GroupedRow { Text("\(excluded.count - 8) more").font(.caption).foregroundStyle(.secondary) }
                    }
                }
            }
        }
    }
}

/// The product's build command result.
struct BuildCard: View {
    let build: BuildResult
    @State private var showOutput = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                TintedSymbol(systemImage: "hammer", color: build.exitCode == 0 ? .green : .red)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Built with \(build.command)").font(.callout.monospaced()).lineLimit(1).truncationMode(.tail)
                    Text("Exit code \(build.exitCode) in \(build.durationSeconds.formatted(.number.precision(.fractionLength(1)))) s").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if !build.outputTail.isEmpty {
                    Button(showOutput ? "Hide Output" : "Show Output") { showOutput.toggle() }
                }
            }
            if showOutput {
                ScrollView {
                    Text(build.outputTail).font(.caption.monospaced()).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 180)
            }
        }
        .card(padding: 14)
    }
}

/// After the commit: committed, verified, going live, live — plus GitHub and guidance.
struct ReleaseOutcomeSection: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let record: ReleaseRecord
    let product: Product?
    @State private var directory: DirectoryStatus?
    @State private var checking = false

    private var status: DirectoryStatus? { directory ?? record.directoryStatus }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupedRows {
                GroupedRow(isFirst: true) {
                    StepDot(state: record.committedRevision == nil ? .pending : .done)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(record.committedRevision.map { "Committed in r\($0)" } ?? "Not committed").fontWeight(.semibold)
                        if let url = record.verification?.tagURL {
                            Text(url).font(.caption.monospaced()).foregroundStyle(.secondary).textSelection(.enabled).lineLimit(1).truncationMode(.middle)
                        }
                    }
                    Spacer()
                    if let url = record.verification?.tagURL {
                        Button { NSPasteboard.general.clearContents(); NSPasteboard.general.setString(url, forType: .string) } label: { Image(systemName: "doc.on.doc") }
                            .buttonStyle(.borderless)
                            .help("Copy URL")
                    }
                }
                if let verification = record.verification {
                    GroupedRow {
                        StepDot(state: verification.isClean ? .done : .warning)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verification.isClean ? "Verified" : "Verified with differences").fontWeight(.semibold)
                            Text(verificationDetail(verification)).font(.caption).foregroundStyle(.secondary)
                            ForEach(verification.notes, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                        }
                        Spacer()
                    }
                }
                if record.isDirectoryTracked {
                    if status?.state == .live {
                        GroupedRow {
                            StepDot(state: .done)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Live on WordPress.org").fontWeight(.semibold)
                                Text("Sites are offered \(record.version ?? "")" + (status?.liveSince.map { " · since \($0.formatted(date: .abbreviated, time: .shortened))" } ?? "")).font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                        }
                    } else {
                        GroupedRow {
                            StepDot(state: .waiting)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(waitingTitle).fontWeight(.semibold)
                                Text(waitingDetail).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer()
                            if checking { ProgressView().controlSize(.small) } else {
                                Button("Check Now") { check() }
                            }
                        }
                        GroupedRow {
                            StepDot(state: .pending)
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Live on WordPress.org").fontWeight(.semibold)
                                Text("You'll get a notification").font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                        }
                    }
                }
            }
            if record.kind == .version, product?.githubRelease?.enabled == true || record.githubRelease != nil {
                GroupedRows {
                    GroupedRow(isFirst: true) { GitHubReleaseRow(record: record) }
                }
            }
            ForEach(record.guidance, id: \.self) { line in
                Label(line, systemImage: "info.circle")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if record.productType == .plugin, record.kind == .version, record.mode != .dryRun {
                Button {
                    openURL(URL(string: "https://wordpress.org/plugins/developers/releases/")!)
                } label: {
                    Label("Open Release Confirmation", systemImage: "arrow.up.right.square")
                }
                .buttonStyle(InlineLinkButtonStyle())
                .linkCursor()
            }
        }
    }

    private var waitingTitle: String {
        status?.state == .notListed ? "Not listed in the directory yet" : status?.state == .superseded ? "A newer version is live" : "Waiting to go live"
    }

    private var waitingDetail: String {
        var text = "WordPress.org offers new releases after a security review period (currently about 6 hours) and Release Confirmation, if enabled."
        if let status {
            if let listed = status.listedVersion { text = "The directory still offers \(listed). " + text }
            text += " Checked \(status.checkedAt.formatted(.relative(presentation: .named)))."
        }
        return text
    }

    private func verificationDetail(_ v: VerificationReport) -> String {
        if v.isClean { return "The server copy matches the release candidate (SHA-256)." }
        return "\(v.missingFiles.count) missing · \(v.unexpectedFiles.count) unexpected · \(v.checksumMismatches.count) checksum difference(s)"
    }

    private func check() {
        checking = true
        Task {
            defer { checking = false }
            if let updated = await model.refreshDirectoryStatus(record.id) { directory = updated.directoryStatus }
        }
    }
}

struct StepDot: View {
    enum State { case done, waiting, warning, pending }
    let state: State

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: 10, weight: .bold))
            .foregroundStyle(foreground)
            .frame(width: 22, height: 22)
            .background(background, in: Circle())
            .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 4 }
    }

    private var symbol: String {
        switch state {
        case .done: "checkmark"
        case .waiting: "hourglass"
        case .warning: "exclamationmark"
        case .pending: "circle.dotted"
        }
    }

    private var foreground: Color {
        switch state {
        case .done: .white
        case .waiting, .warning: .orange
        case .pending: .secondary
        }
    }

    private var background: Color {
        switch state {
        case .done: .green
        case .waiting, .warning: .orange.opacity(0.18)
        case .pending: .primary.opacity(0.08)
        }
    }
}

/// The product's GitHub release for this version, with a way to create or retry it.
struct GitHubReleaseRow: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL
    let record: ReleaseRecord
    @State private var latest: GitHubReleaseResult?
    @State private var working = false

    private var result: GitHubReleaseResult? { latest ?? record.githubRelease }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("GitHub").foregroundStyle(.secondary).frame(width: 130, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
                if let result, result.succeeded, let url = result.url {
                    Button {
                        openURL(url)
                    } label: {
                        Label("Release \(result.tag)\(result.assetName.map { " with \($0)" } ?? "")", systemImage: "arrow.up.right.square")
                    }
                    .buttonStyle(InlineLinkButtonStyle())
                    .linkCursor()
                } else if let error = result?.error {
                    Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).font(.callout)
                        .fixedSize(horizontal: false, vertical: true)
                } else {
                    Text("Not created yet").font(.callout).foregroundStyle(.secondary)
                }
                ForEach(result?.notes ?? [], id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
            }
            Spacer(minLength: 8)
            if working {
                ProgressView().controlSize(.small)
            } else if result?.succeeded != true {
                Button(result == nil ? "Create GitHub Release" : "Try Again") {
                    Task {
                        working = true
                        defer { working = false }
                        do {
                            latest = try await model.pipeline.publishGitHubRelease(record.id).githubRelease
                            model.reload()
                        } catch {
                            model.show(error, title: "Could not create the GitHub release")
                        }
                    }
                }
            }
        }
    }
}
