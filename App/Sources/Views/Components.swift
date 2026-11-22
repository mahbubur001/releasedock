import ReleaseDockCore
import SwiftUI

struct StateBadge: View {
    let state: ReleaseState

    var body: some View {
        Text(state.displayName)
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }

    var color: Color {
        switch state {
        case .completed: .green
        case .failed: .red
        case .cancelled: .secondary
        case .recoveryRequired: .orange
        case .publishing, .verifying: .blue
        default: .accentColor
        }
    }
}

struct ProductTypeIcon: View {
    let type: ProductType

    var body: some View {
        Image(systemName: type == .plugin ? "puzzlepiece.extension.fill" : "paintpalette.fill")
            .foregroundStyle(type == .plugin ? Color.accentColor : Color.purple)
    }
}

struct ValidationStatusIcon: View {
    let status: ValidationStatus

    var body: some View {
        switch status {
        case .passed: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .warning: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        case .failed: Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
        case .skipped: Image(systemName: "minus.circle").foregroundStyle(.secondary)
        }
    }
}

/// Validation results grouped by check, failures first.
struct ValidationListView: View {
    let report: ValidationReport

    private var sorted: [ValidationResult] {
        let rank: (ValidationStatus) -> Int = { [.failed: 0, .warning: 1, .skipped: 2, .passed: 3][$0] ?? 4 }
        return report.results.sorted { rank($0.status) < rank($1.status) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(sorted) { result in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    ValidationStatusIcon(status: result.status)
                        .accessibilityLabel(result.status.rawValue)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            Text(result.name).font(.callout.weight(.medium))
                            if !result.check.isRequired {
                                Text("optional").font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                        Text(result.message).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                        if let path = result.filePath {
                            Text(path).font(.caption.monospaced()).foregroundStyle(.secondary)
                        }
                        if let suggestion = result.suggestion, result.status != .passed {
                            Label(suggestion, systemImage: "lightbulb").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 6)
                Divider()
            }
        }
    }
}

struct SectionCard<Content: View>: View {
    let title: String
    var systemImage: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let systemImage {
                Label(title, systemImage: systemImage).font(.headline)
            } else {
                Text(title).font(.headline)
            }
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
    }
}

struct KeyValueRow: View {
    let key: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(key).foregroundStyle(.secondary).frame(width: 130, alignment: .leading)
            Text(value)
                .font(monospaced ? .callout.monospaced() : .callout)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }
}

struct PreviewChangesView: View {
    let preview: ReleasePreview

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 16) {
                Label("\(preview.added) added", systemImage: "plus.circle").foregroundStyle(.green)
                Label("\(preview.modified) modified", systemImage: "pencil.circle").foregroundStyle(.blue)
                Label("\(preview.deleted) deleted", systemImage: "minus.circle").foregroundStyle(.red)
            }
            .font(.callout)
            Table(preview.changes) {
                TableColumn("Change") { change in
                    Text(change.isCopy ? "copy" : change.kind.rawValue)
                        .foregroundStyle(color(change.kind))
                }
                .width(90)
                TableColumn("Path") { change in
                    Text(change.path + (change.isDirectory ? "/" : "")).font(.callout.monospaced())
                }
            }
            .frame(minHeight: 180, maxHeight: 320)
            if !preview.diffExcerpt.isEmpty {
                DisclosureGroup("Text diff\(preview.diffTruncated ? " (truncated)" : "")") {
                    ScrollView {
                        Text(preview.diffExcerpt)
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(maxHeight: 300)
                }
            }
            DisclosureGroup("Commit message") {
                Text(preview.commitMessage).font(.callout.monospaced()).textSelection(.enabled)
            }
        }
    }

    private func color(_ kind: ChangeKind) -> Color {
        switch kind {
        case .added: .green
        case .deleted: .red
        case .modified, .replaced, .propertyChanged: .blue
        }
    }
}

/// Fixed-width list column beside a flexible detail pane. Avoids `HSplitView`/`VSplitView`,
/// which collapse SwiftUI tables and hide panes inside a `NavigationSplitView` detail.
struct MasterDetailLayout<Master: View, Detail: View>: View {
    var masterWidth: CGFloat = 300
    @ViewBuilder var master: Master
    @ViewBuilder var detail: Detail

    var body: some View {
        HStack(spacing: 0) {
            master
                .frame(width: masterWidth)
                .frame(maxHeight: .infinity, alignment: .top)
                .background(.background.secondary)
            Divider()
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// A full-width clickable row with hover and selection styling.
struct SelectableRow<Content: View>: View {
    let isSelected: Bool
    let action: () -> Void
    @ViewBuilder var content: Content
    @State private var isHovering = false

    var body: some View {
        Button { action() } label: {
            content
                .padding(.horizontal, 10)
                .padding(.vertical, 9)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(isSelected ? Color.accentColor.opacity(0.18) : (isHovering ? Color.primary.opacity(0.06) : Color.clear))
                }
                .overlay {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(isSelected ? Color.accentColor.opacity(0.6) : Color.clear, lineWidth: 1)
                }
                .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .animation(.easeOut(duration: 0.12), value: isHovering)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

struct SearchField: View {
    @Binding var text: String
    var prompt: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField(prompt, text: $text).textFieldStyle(.plain)
            if !text.isEmpty {
                Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .buttonStyle(.plain)
                                            .help("Clear")
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.primary.opacity(0.06)))
    }
}

struct StatTile: View {
    let title: String
    let value: String
    let systemImage: String
    var tint: Color = .accentColor

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: systemImage)
                .font(.title3.weight(.semibold))
                .foregroundStyle(tint)
                .frame(width: 38, height: 38)
                .background(tint.opacity(0.15), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            VStack(alignment: .leading, spacing: 1) {
                Text(value).font(.title2.weight(.semibold)).monospacedDigit()
                Text(title).font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .frame(maxWidth: .infinity)
        .background(.background.secondary, in: RoundedRectangle(cornerRadius: 10))
    }
}
