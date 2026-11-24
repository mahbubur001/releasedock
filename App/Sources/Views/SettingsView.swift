import AppKit
import ReleaseDockCore
import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @State private var draft = AppSettings()
    @State private var githubToken = ""
    @State private var tokenStored = false
    @State private var releaseToken = ""
    @State private var releaseTokenStored = false
    @State private var message: String?

    var body: some View {
        Form {
            Section("Subversion") {
                LabeledContent("Detected") {
                    Text(model.svnInstallation.map { "\($0.version) at \($0.executableURL.path)" } ?? (model.svnProblem ?? "Not found"))
                        .textSelection(.enabled)
                }
                TextField("svn executable", text: $draft.svnExecutablePath, prompt: Text("Automatic (Homebrew, MacPorts, /usr/bin)"))
                Picker("Commit timeout", selection: Binding(get: { draft.commitTimeoutMinutes ?? AppSettings.defaultCommitTimeoutMinutes }, set: { draft.commitTimeoutMinutes = $0 })) {
                    ForEach([15, 30, 60, 120], id: \.self) { Text("\($0) minutes").tag($0) }
                }
                Text("Install Subversion with `brew install subversion` if it is missing. WordPress.org can take minutes to finish committing a large release; if a commit still times out, ReleaseDock checks the repository instead of retrying. Changes to the path or timeout take effect after restarting ReleaseDock.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Code checks") {
                LabeledContent("PHP lint") {
                    Text(phpPath.map { "Uses \($0)" } ?? "PHP not found. Install it with `brew install php`.")
                        .textSelection(.enabled)
                }
                HStack {
                    TextField("WordPress for Plugin Check", text: Binding(get: { draft.pluginCheckWordPressPath ?? "" }, set: { draft.pluginCheckWordPressPath = $0.isEmpty ? nil : $0 }), prompt: Text("Off"))
                    Button("Choose…") { chooseWordPress() }
                }
                Text("Plugin Check runs `wp plugin check` in a local WordPress install that has the Plugin Check plugin active (WP-CLI: `brew install wp-cli`). Findings are shown as warnings; WordPress.org runs the same checks on every release.")
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
            Section("GitHub releases") {
                SecureField("Release token", text: $releaseToken, prompt: Text(releaseTokenStored ? "Stored in Keychain" : "Needed only for products that create GitHub releases"))
                Text("A fine-grained token limited to your plugin and theme repositories, with Contents: read and write. It is stored in the Keychain.")
                    .font(.caption).foregroundStyle(.secondary)
                if releaseTokenStored {
                    Button("Remove Release Token", role: .destructive) {
                        try? model.keychain.deletePassword(for: .githubReleaseToken)
                        releaseTokenStored = false
                    }
                }
            }
            Section("Notifications") {
                Toggle("Notify about releases and updates", isOn: $draft.notificationsEnabled)
            }
            Section("Data") {
                LabeledContent("Data folder") {
                    Button(model.paths.root.path) { NSWorkspace.shared.open(model.paths.root) }
                        .buttonStyle(InlineLinkButtonStyle())
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
                    .disabled(draft == model.settings && githubToken.isEmpty && releaseToken.isEmpty)
            }
        }
        .onAppear {
            draft = model.settings
            tokenStored = ((try? model.keychain.password(for: .githubToken)) ?? nil) != nil
            releaseTokenStored = ((try? model.keychain.password(for: .githubReleaseToken)) ?? nil) != nil
        }
    }

    private var phpPath: String? { CodeCheckService.phpCandidates.first(where: FileManager.default.isExecutableFile(atPath:)) }

    private func chooseWordPress() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.prompt = "Use WordPress"
        panel.message = "Choose the folder that contains wp-config.php"
        if panel.runModal() == .OK, let url = panel.url { draft.pluginCheckWordPressPath = url.path }
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
        if let wordpress = draft.pluginCheckWordPressPath, !wordpress.isEmpty {
            let fm = FileManager.default
            let hasConfig = fm.fileExists(atPath: (wordpress as NSString).appendingPathComponent("wp-config.php"))
                || fm.fileExists(atPath: ((wordpress as NSString).deletingLastPathComponent as NSString).appendingPathComponent("wp-config.php"))
            guard hasConfig, fm.fileExists(atPath: (wordpress as NSString).appendingPathComponent("wp-load.php")) else {
                message = "\(wordpress) is not a WordPress install (no wp-load.php / wp-config.php)."
                return
            }
        }
        if !releaseToken.isEmpty {
            do {
                try model.keychain.setPassword(releaseToken, for: .githubReleaseToken)
                releaseTokenStored = true
                releaseToken = ""
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
