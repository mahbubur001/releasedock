import AppKit
import ReleaseDockCore
import SwiftUI

/// The `svn` commands ReleaseDock ran, newest operation first: command, exit code,
/// duration and the end of the output. Passwords never appear (they go through stdin).
struct TranscriptList: View {
    let entries: [TranscriptEntry]

    var body: some View {
        if entries.isEmpty {
            Text("No svn commands recorded yet.")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(14)
                .background(.quinary.opacity(0.6), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        } else {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(entries.enumerated()), id: \.element.id) { index, entry in
                    if index == 0 || entries[index - 1].operation != entry.operation || entries[index - 1].releaseID != entry.releaseID {
                        HStack(spacing: 6) {
                            Text(entry.operation).font(.caption.weight(.semibold))
                            Text(entry.date.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary)
                        }
                        .padding(.horizontal, 12)
                        .padding(.top, index == 0 ? 10 : 14)
                        .padding(.bottom, 4)
                    }
                    TranscriptRow(entry: entry)
                }
            }
            .padding(.bottom, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.quinary.opacity(0.6), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }

    /// Plain text for the clipboard or a support request.
    static func text(_ entries: [TranscriptEntry]) -> String {
        entries.map { entry in
            var lines = ["[\(entry.date.formatted(.iso8601))] \(entry.operation)", "$ \(entry.command)", "  \(TranscriptRow.status(entry))"]
            if !entry.output.isEmpty { lines += entry.output.split(separator: "\n", omittingEmptySubsequences: false).map { "  " + $0 } }
            return lines.joined(separator: "\n")
        }
        .joined(separator: "\n\n")
    }
}

private struct TranscriptRow: View {
    let entry: TranscriptEntry
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("$").foregroundStyle(.tertiary)
                Text(entry.command)
                    .lineLimit(expanded ? nil : 1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                Spacer(minLength: 8)
                Text(Self.status(entry))
                    .font(.caption)
                    .foregroundStyle(entry.succeeded ? Color.green : Color.red)
                    .fixedSize()
            }
            .font(.system(.callout, design: .monospaced))
            if expanded, !entry.output.isEmpty || entry.failure != nil {
                Text([entry.output, entry.failure ?? ""].filter { !$0.isEmpty }.joined(separator: "\n"))
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .padding(.leading, 18)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 3)
        .contentShape(Rectangle())
        .onTapGesture { withAnimation(.easeOut(duration: 0.15)) { expanded.toggle() } }
        .help(expanded ? "Click to collapse" : "Click to show the output")
    }

    static func status(_ entry: TranscriptEntry) -> String {
        let duration = entry.duration < 1 ? String(format: "%.0f ms", entry.duration * 1000) : String(format: "%.1f s", entry.duration)
        if let exit = entry.exitCode, entry.failure == nil { return "exit \(exit) · \(duration)" }
        return "stopped · \(duration)"
    }
}

/// A product's transcript in a sheet, from the product page.
struct TranscriptSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let product: Product
    @State private var entries: [TranscriptEntry] = []

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Transcript").font(.title3.weight(.semibold))
                    Text("svn commands for \(product.name), newest first. Passwords are passed through stdin and never shown.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(20)
            ScrollView {
                TranscriptList(entries: entries)
                    .padding(.horizontal, 20)
                    .padding(.bottom, 20)
            }
            Divider()
            HStack {
                Button("Copy All") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(TranscriptList.text(entries), forType: .string)
                }
                .disabled(entries.isEmpty)
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 760, height: 560)
        .task { entries = model.transcriptEntries(productID: product.id) }
    }
}
