import AppKit
import ReleaseDockCore
import SwiftUI

/// Full report for one release: inspection, version comparison, validation, preview,
/// result and history. Used by the wizard and by Release History.
struct ReleaseReportView: View {
    let record: ReleaseRecord
    let product: Product?
    /// Hidden where the surrounding view already shows the product and state.
    var showsHeader = true

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if showsHeader { header }
            if record.state == .completed {
                result
            }
            if record.state == .recoveryRequired {
                SectionCard(title: "Recovery required", systemImage: "exclamationmark.arrow.triangle.2.circlepath") {
                    Text(record.verifiedNotCommitted
                         ? "ReleaseDock inspected the repository and verified that the commit did not land. You can publish again (with confirmation) or abandon the release."
                         : "The commit's outcome is unknown. Choose “Inspect Repository” — ReleaseDock looks for this release's ID in the SVN log before doing anything else and never re-commits blindly.")
                    ForEach(record.errors, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                }
            }
            if let inspection = record.inspection {
                inspectionCard(inspection)
            }
            versionCard
            if let report = record.validation {
                SectionCard(title: "Validation", systemImage: "checklist") {
                    if report.isBlocking {
                        Label("\(report.blockingResults.count) blocking result(s)", systemImage: "xmark.octagon.fill").foregroundStyle(.red)
                    }
                    ValidationListView(report: report)
                }
            }
            if let preview = record.preview {
                SectionCard(title: "SVN changes", systemImage: "arrow.left.arrow.right") {
                    PreviewChangesView(preview: preview)
                }
            }
            SectionCard(title: "Timeline", systemImage: "list.bullet.indent") {
                ForEach(Array(record.transitions.enumerated()), id: \.offset) { _, transition in
                    HStack(alignment: .firstTextBaseline) {
                        Text(transition.at.formatted(date: .omitted, time: .standard)).font(.caption.monospacedDigit()).foregroundStyle(.secondary).frame(width: 80, alignment: .leading)
                        StateBadge(state: transition.to)
                        Text(transition.note ?? "").font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
            }
        }
    }

    private var header: some View {
        HStack(alignment: .center) {
            ProductTypeIcon(type: record.productType).font(.title)
            VStack(alignment: .leading) {
                Text("\(record.productName) \(record.version ?? "")").font(.title2.bold())
                Text("\(record.kind == .readmeUpdate ? "\(record.kind.displayName) · " : "")\(record.mode.displayName) · started \(record.createdAt.formatted(date: .abbreviated, time: .shortened))").foregroundStyle(.secondary)
            }
            Spacer()
            StateBadge(state: record.state)
        }
    }

    private var result: some View {
        SectionCard(title: record.mode == .dryRun ? "Dry run complete" : "Release report", systemImage: "checkmark.seal.fill") {
            if record.mode == .dryRun {
                Text("Everything validated and staged. Nothing was committed and the working copy was restored.")
            } else {
                KeyValueRow(key: "Committed revision", value: record.committedRevision.map { "r\($0)" } ?? "—")
                KeyValueRow(key: "Revision before", value: record.revisionBefore.map { "r\($0)" } ?? "—")
                if let verification = record.verification {
                    KeyValueRow(key: "Release URL", value: verification.tagURL, monospaced: true)
                    KeyValueRow(key: "Verification", value: verification.isClean ? "Files match the release candidate" : "Differences found")
                    ForEach(verification.missingFiles, id: \.self) { Label("Missing: \($0)", systemImage: "minus.circle").font(.caption) }
                    ForEach(verification.unexpectedFiles, id: \.self) { Label("Unexpected: \($0)", systemImage: "plus.circle").font(.caption) }
                    ForEach(verification.checksumMismatches, id: \.self) { Label("Checksum differs: \($0)", systemImage: "number").font(.caption) }
                    ForEach(verification.notes, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                }
                if !record.guidance.isEmpty {
                    Divider()
                    ForEach(record.guidance, id: \.self) { Label($0, systemImage: "info.circle").font(.callout) }
                }
            }
            if let manifest = record.manifestPath {
                Button("Show Manifest in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: manifest)])
                }
            }
        }
    }

    private func inspectionCard(_ inspection: PackageInspection) -> some View {
        SectionCard(title: "Package", systemImage: inspection.sourceKind == .zip ? "doc.zipper" : "folder") {
            KeyValueRow(key: "Source", value: inspection.sourcePath, monospaced: true)
            KeyValueRow(key: inspection.sourceKind == .zip ? "SHA-256" : "Tree digest", value: inspection.sha256, monospaced: true)
            KeyValueRow(key: "Files", value: "\(inspection.includedFiles.count) included, \(inspection.excludedPaths.count) excluded, \(ByteCountFormatter.string(fromByteCount: Int64(inspection.totalUncompressedSize), countStyle: .file))")
            if let detected = record.detected {
                KeyValueRow(key: "Detected", value: "\(detected.type.displayName) “\(detected.name ?? "?")” in \(detected.mainFile ?? "?")")
            }
            if let commit = record.gitCommit {
                KeyValueRow(key: "Git commit", value: commit, monospaced: true)
            }
            if !inspection.excludedPaths.isEmpty {
                DisclosureGroup("Excluded paths") {
                    ForEach(inspection.excludedPaths, id: \.self) { Text($0).font(.caption.monospaced()) }
                }
            }
            let notable = inspection.issues.filter { $0.severity != .info }
            if !notable.isEmpty {
                DisclosureGroup("Inspection findings (\(notable.count))") {
                    ForEach(notable) { issue in
                        Label("\(issue.path ?? "") — \(issue.message)", systemImage: issue.severity == .error ? "xmark.octagon" : "exclamationmark.triangle")
                            .font(.caption)
                            .foregroundStyle(issue.severity == .error ? .red : .orange)
                    }
                }
            }
        }
    }

    private var versionCard: some View {
        SectionCard(title: "Version", systemImage: "number") {
            HStack(spacing: 24) {
                VStack(alignment: .leading) {
                    Text("Current (SVN)").font(.caption).foregroundStyle(.secondary)
                    Text(record.repository?.currentVersion ?? record.previousVersion ?? "—").font(.title3.monospacedDigit())
                    if let source = record.repository?.currentVersionSource { Text(source).font(.caption2).foregroundStyle(.secondary) }
                }
                Image(systemName: "arrow.right").foregroundStyle(.secondary)
                VStack(alignment: .leading) {
                    Text("Candidate").font(.caption).foregroundStyle(.secondary)
                    Text(record.version ?? "—").font(.title3.monospacedDigit())
                    Text("from package headers").font(.caption2).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
            if let result = record.validation?.results.first(where: { $0.check == (record.kind == .readmeUpdate ? .releasedVersion : .versionIncrease) }) {
                Label(result.message, systemImage: result.status == .passed ? "checkmark.circle.fill" : "xmark.octagon.fill")
                    .foregroundStyle(result.status == .passed ? .green : .red)
                    .font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(record.repository?.notes ?? [], id: \.self) { Text($0).font(.caption).foregroundStyle(.orange) }
        }
    }
}
