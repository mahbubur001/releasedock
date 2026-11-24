import ReleaseDockCore
import SwiftUI

/// Previews and applies a version bump in a development folder. Nothing is written until
/// the user clicks Apply, and applying refuses if a file changed after the preview.
struct VersionBumpSheet: View {
    @Environment(\.dismiss) private var dismiss
    let product: Product
    let folder: URL
    var onApplied: (String) -> Void = { _ in }

    @State private var current: String?
    @State private var newVersion = ""
    @State private var notes = ""
    @State private var plan: VersionBumpPlan?
    @State private var error: String?
    @State private var applied = false

    private let service = VersionBumpService()

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Bump Version").font(.title2.bold())
            Text(folder.path).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)

            if applied, let plan {
                Label("Updated \(plan.files.count) file(s) to \(plan.newVersion).", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                Text("Commit these changes to Git before releasing, so the release matches a commit.")
                    .font(.callout).foregroundStyle(.secondary)
            } else {
                form
            }

            HStack {
                if let error { Label(error, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red).font(.callout).lineLimit(3) }
                Spacer()
                if applied {
                    Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
                } else {
                    Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                    Button("Apply") { apply() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(plan == nil)
                }
            }
        }
        .padding(20)
        .frame(width: 620)
        .onAppear(perform: load)
    }

    @ViewBuilder private var form: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text("Current").foregroundStyle(.secondary)
            Text(current ?? "—").font(.body.monospacedDigit())
            Image(systemName: "arrow.right").foregroundStyle(.secondary)
            TextField("New version", text: $newVersion)
                .frame(width: 120)
                .onChange(of: newVersion) { refresh() }
            if let current {
                ForEach(VersionBumpService.Part.allCases.reversed(), id: \.self) { part in
                    if let suggestion = VersionBumpService.suggest(part, from: current) {
                        Button(part.rawValue.capitalized) { newVersion = suggestion }
                            .help(suggestion)
                    }
                }
            }
        }
        VStack(alignment: .leading, spacing: 4) {
            Text("Changelog notes (optional, one per line)").font(.callout).foregroundStyle(.secondary)
            TextEditor(text: $notes)
                .font(.body.monospaced())
                .frame(height: 70)
                .scrollContentBackground(.hidden)
                .padding(4)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color.secondary.opacity(0.08)))
                .onChange(of: notes) { refresh() }
        }
        if let plan {
            GroupBox("Changes") {
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(plan.changes) { change in
                            VStack(alignment: .leading, spacing: 2) {
                                Text("\(change.file)\(change.line > 0 ? ":\(change.line)" : "") · \(change.description)")
                                    .font(.caption).foregroundStyle(.secondary)
                                if !change.oldText.isEmpty {
                                    Text("− " + change.oldText.trimmingCharacters(in: .whitespaces)).foregroundStyle(.red)
                                }
                                Text("+ " + change.newText.trimmingCharacters(in: .whitespaces)).foregroundStyle(.green)
                            }
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220)
            }
            ForEach(plan.notes, id: \.self) { Label($0, systemImage: "info.circle").font(.callout).foregroundStyle(.secondary) }
        }
    }

    private func load() {
        current = try? ProductDetector().detect(at: folder, expectedType: product.type, mainPluginFile: product.mainPluginFile, slug: product.slug).version
        if let current, newVersion.isEmpty { newVersion = VersionBumpService.suggest(.patch, from: current) ?? "" }
        refresh()
    }

    private func refresh() {
        guard !newVersion.isEmpty else { plan = nil; error = nil; return }
        do {
            plan = try service.plan(folder: folder, product: product, newVersion: newVersion, changelogNotes: notes)
            error = nil
        } catch {
            plan = nil
            self.error = error.localizedDescription
        }
    }

    private func apply() {
        guard let plan else { return }
        do {
            try service.apply(plan, folder: folder)
            applied = true
            onApplied(plan.newVersion)
        } catch {
            self.error = error.localizedDescription
            refresh()
        }
    }
}

/// Folder a bump sheet edits; `Identifiable` for `.sheet(item:)`.
struct VersionBumpTarget: Identifiable {
    let folder: URL
    var id: String { folder.path }
}
