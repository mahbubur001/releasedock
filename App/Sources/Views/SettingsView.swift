import AppKit
import ReleaseDockCore
import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @State private var draft = AppSettings()
    @State private var githubToken = ""
    @State private var tokenStored = false
    @State private var message: String?

    var body: some View {
        Form {
            Section("Subversion") {
                LabeledContent("Detected") {
                    Text(model.svnInstallation.map { "\($0.version) at \($0.executableURL.path)" } ?? (model.svnProblem ?? "Not found"))
                        .textSelection(.enabled)
                }
                TextField("svn executable", text: $draft.svnExecutablePath, prompt: Text("Automatic (Homebrew, MacPorts, /usr/bin)"))
                Text("Install Subversion with `brew install subversion` if it is missing. Changing the path takes effect after restarting ReleaseDock.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("App updates") {
                TextField("GitHub repository", text: $draft.githubRepository, prompt: Text("owner/name"))
                Picker("Channel", selection: $draft.updateChannel) {
                    Text("Stable").tag(ReleaseChannel.stable)
                    Text("Beta").tag(ReleaseChannel.beta)
                }
                Toggle("Check GitHub when ReleaseDock starts", isOn: $draft.checkGitHubOnLaunch)
                if updater.isConfigured {
                    Toggle("Let Sparkle check automatically", isOn: Binding(get: { updater.automaticallyChecksForUpdates }, set: { updater.automaticallyChecksForUpdates = $0 }))
                } else {
                    LabeledContent("In-app updates", value: updater.disabledReason ?? "Disabled")
                }
                SecureField("GitHub token (private repositories only)", text: $githubToken, prompt: Text(tokenStored ? "Stored in Keychain" : "Not needed for public repositories"))
                Text("Use a fine-grained token limited to the ReleaseDock repository with Contents: read-only permission.")
                    .font(.caption).foregroundStyle(.secondary)
                if tokenStored {
                    Button("Remove Token", role: .destructive) {
                        try? model.keychain.deletePassword(for: .githubToken)
                        tokenStored = false
                    }
                }
            }
            Section("Notifications") {
                Toggle("Notify about releases and updates", isOn: $draft.notificationsEnabled)
            }
            Section("Data") {
                LabeledContent("Data folder") {
                    Button(model.paths.root.path) { NSWorkspace.shared.open(model.paths.root) }
                        .buttonStyle(.link)
                }
                Button("Export Diagnostics…") {
                    if let url = model.exportDiagnostics() {
                        NSWorkspace.shared.activateFileViewerSelecting([url])
                    }
                }
                Text("Diagnostics include products, recent releases and ReleaseDock logs. Credentials are never included.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let message { Text(message).foregroundStyle(.secondary) }
        }
        .formStyle(.grouped)
        .navigationTitle("Settings")
        .toolbar {
            ToolbarItem {
                Button("Save") { save() }
                    .disabled(draft == model.settings && githubToken.isEmpty)
            }
        }
        .onAppear {
            draft = model.settings
            tokenStored = ((try? model.keychain.password(for: .githubToken)) ?? nil) != nil
        }
    }

    private func save() {
        if !draft.githubRepository.isEmpty, GitHubRepository(string: draft.githubRepository) == nil {
            message = "Enter the repository as owner/name."
            return
        }
        if !draft.svnExecutablePath.isEmpty, !FileManager.default.isExecutableFile(atPath: draft.svnExecutablePath) {
            message = "\(draft.svnExecutablePath) is not an executable file."
            return
        }
        if !githubToken.isEmpty {
            do {
                try model.keychain.setPassword(githubToken, for: .githubToken)
                tokenStored = true
                githubToken = ""
            } catch {
                message = error.localizedDescription
                return
            }
        }
        updater.channel = draft.updateChannel
        model.saveSettings(draft)
        message = "Saved."
    }
}
