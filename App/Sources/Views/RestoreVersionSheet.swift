import ReleaseDockCore
import SwiftUI

/// Picks an older released version to ship again under a new, higher version. The code is
/// exported from the repository and goes through the normal checks and review in New Release.
struct RestoreVersionSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let product: Product

    @State private var options: RestoreOptions?
    @State private var loadError: String?
    @State private var restoring = ""
    @State private var newVersion = ""
    @State private var notes = ""
    @State private var notesEdited = false
    @State private var dryRun = false
    @State private var started = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Restore an Older Version").font(.title2.bold())
            Text("Ships an older version's code again under a new, higher version, so sites on \(options?.currentVersion ?? "the current version") update to it like any other update. Released versions stay as they are.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            if let options {
                if options.olderVersions.isEmpty {
                    Label("There is no released version older than \(options.currentVersion ?? "the current one").", systemImage: "info.circle")
                        .foregroundStyle(.secondary)
                } else {
                    form(options)
                }
            } else if let loadError {
                Label(loadError, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red).font(.callout)
            } else {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Reading released versions…").foregroundStyle(.secondary)
                }
            }

            HStack {
                if let problem { Label(problem, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange).font(.callout).lineLimit(2) }
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Prepare Release") { start() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canStart)
            }
        }
        .padding(20)
        .frame(width: 560)
        .task { await load() }
        .onDisappear { model.drafts[draftKey] = started || !notesEdited ? nil : notes }
        #if DEBUG
        // UI verification: RELEASEDOCK_UI_TEST_RESTORE=auto prepares the suggested restore once possible.
        .onChange(of: canStart, initial: true) { _, ready in
            if ready, ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_RESTORE"] == "auto" { start() }
        }
        #endif
    }

    @ViewBuilder private func form(_ options: RestoreOptions) -> some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 10) {
            GridRow {
                Text("Restore code from").foregroundStyle(.secondary)
                Picker("Restore code from", selection: $restoring) {
                    ForEach(options.olderVersions, id: \.self) { Text($0).tag($0) }
                }
                .labelsHidden()
                .fixedSize()
            }
            GridRow {
                Text("Release it as").foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    TextField("New version", text: $newVersion).frame(width: 120)
                    if let highest = options.highestVersion {
                        Text("newer than \(highest)").font(.callout).foregroundStyle(.secondary)
                    }
                }
            }
            GridRow {
                Text("Mode").foregroundStyle(.secondary)
                Toggle("Dry run (prepare and check, commit nothing)", isOn: $dryRun)
            }
        }
        .onChange(of: restoring) { suggestNotes() }
        VStack(alignment: .leading, spacing: 4) {
            Text("Changelog entry (one line per bullet)").font(.callout).foregroundStyle(.secondary)
            TextEditor(text: Binding(get: { notes }, set: { notes = $0; notesEdited = true }))
                .font(.body.monospaced())
                .frame(height: 60)
                .scrollContentBackground(.hidden)
                .padding(4)
                .background(RoundedRectangle(cornerRadius: 6).fill(.quinary))
        }
        Text("Next, New Release exports \(restoring.isEmpty ? "the version" : restoring) from SVN, sets the version to \(newVersion.isEmpty ? "the new one" : newVersion) and runs every check. You review the changes before anything is committed.")
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var problem: String? {
        guard let options, !options.olderVersions.isEmpty, !newVersion.isEmpty else { return nil }
        guard VersionService().validate(newVersion).isValid else { return "\"\(newVersion)\" is not a valid version." }
        if let highest = options.highestVersion, PHPVersion.compare(newVersion, highest) <= 0 {
            return "\(newVersion) must be newer than \(highest), or sites will not update."
        }
        return nil
    }

    private var canStart: Bool {
        options?.olderVersions.isEmpty == false && !restoring.isEmpty && !newVersion.isEmpty && problem == nil
            && model.svnInstallation != nil && !model.wizard.isRunning
    }

    private func load() async {
        do {
            let loaded = try await model.pipeline.restoreOptions(productID: product.id)
            options = loaded
            restoring = loaded.olderVersions.first ?? ""
            newVersion = loaded.suggestedVersion ?? ""
            if let saved = model.drafts[draftKey] {
                notes = saved
                notesEdited = true
            }
            suggestNotes()
        } catch {
            loadError = error.localizedDescription
        }
    }

    private func suggestNotes() {
        guard !notesEdited, !restoring.isEmpty else { return }
        notes = "Restores the \(restoring) code" + (options?.currentVersion.map { " after a problem in \($0)." } ?? ".")
    }

    private var draftKey: String { "restore-notes-\(product.id)" }

    private func start() {
        started = true
        let (productID, old, new, entry, mode) = (product.id, restoring, newVersion, notes, dryRun ? ReleaseMode.dryRun : .assisted)
        model.wizardProductID = productID
        model.selection = .wizard
        dismiss()
        Task { await model.wizard.prepareRestore(productID: productID, restoring: old, as: new, notes: entry, mode: mode) }
    }
}
