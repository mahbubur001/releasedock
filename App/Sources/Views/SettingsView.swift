import AppKit
import ReleaseDockCore
import SwiftUI

/// The Settings window (⌘, or the gear in the sidebar). Changes apply as they are made.
struct SettingsView: View {
    @State private var tab = Self.initialTab

    var body: some View {
        TabView(selection: $tab) {
            GeneralSettings().tabItem { Label("General", systemImage: "gearshape") }.tag(0)
            SubversionSettings().tabItem { Label("Subversion", systemImage: "arrow.triangle.branch") }.tag(1)
            CheckSettings().tabItem { Label("Checks", systemImage: "checkmark.shield") }.tag(2)
            AccountSettings().tabItem { Label("Accounts", systemImage: "key") }.tag(3)
            UpdateSettings().tabItem { Label("Updates", systemImage: "arrow.down.circle") }.tag(4)
            AboutSettings().tabItem { Label("About", systemImage: "info.circle") }.tag(5)
        }
        .frame(width: 580, height: 460)
        .background(WindowTitleHidden())
    }

    private static var initialTab: Int {
        #if DEBUG
        ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_SETTINGS"].flatMap(Int.init) ?? 0
        #else
        0
        #endif
    }
}

/// Writes one change to the saved settings.
@MainActor
private func update(_ model: AppModel, _ change: (inout AppSettings) -> Void) {
    var settings = model.settings
    change(&settings)
    guard settings != model.settings else { return }
    model.saveSettings(settings)
}

private struct Hint: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text((try? AttributedString(markdown: text)) ?? AttributedString(text))
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

private struct GeneralSettings: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Form {
            Section("Notifications") {
                Toggle(isOn: Binding(get: { model.settings.notificationsEnabled }, set: { value in update(model) { $0.notificationsEnabled = value } })) {
                    Text("Notify about releases and updates")
                    Text("Release finished, approval needed, a release is live, a new ReleaseDock version.")
                }
            }
            Section("Data") {
                LabeledContent("Data folder") {
                    Button((model.paths.root.path as NSString).abbreviatingWithTildeInPath) { NSWorkspace.shared.open(model.paths.root) }
                        .buttonStyle(InlineLinkButtonStyle())
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                LabeledContent {
                    Button("Export…") {
                        if let url = model.exportDiagnostics() {
                            NSWorkspace.shared.activateFileViewerSelecting([url])
                        }
                    }
                } label: {
                    Text("Diagnostics")
                    Text("Products, recent releases and logs. Credentials are never included.")
                }
            }
        }
        .formStyle(.grouped)
    }
}

private struct SubversionSettings: View {
    @Environment(AppModel.self) private var model
    @State private var path = ""
    @State private var problem: String?

    var body: some View {
        Form {
            Section("Subversion") {
                LabeledContent("Detected") {
                    if let installation = model.svnInstallation, model.svnProblem == nil {
                        Label("\(installation.version) at \(installation.executableURL.path)", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                            .textSelection(.enabled)
                    } else {
                        Label(model.svnProblem ?? "Not found", systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                    }
                }
                TextField("svn executable", text: $path, prompt: Text("Automatic (Homebrew, MacPorts, /usr/bin)"))
                    .onSubmit(savePath)
                if let problem { Text(problem).font(.caption).foregroundStyle(.red) }
                Picker("Commit timeout", selection: Binding(
                    get: { model.settings.commitTimeoutMinutes ?? AppSettings.defaultCommitTimeoutMinutes },
                    set: { value in update(model) { $0.commitTimeoutMinutes = value } }
                )) {
                    ForEach([15, 30, 60, 120], id: \.self) { Text("\($0) minutes").tag($0) }
                }
            }
            Section {
                Hint("Install Subversion with `brew install subversion` if it is missing. WordPress.org can take minutes to finish committing a large release; if a commit still times out, ReleaseDock checks the repository instead of retrying. Changes to the path or timeout take effect after restarting ReleaseDock.")
            }
        }
        .formStyle(.grouped)
        .onAppear { path = model.settings.svnExecutablePath }
        .onDisappear(perform: savePath)
    }

    private func savePath() {
        let trimmed = path.trimmingCharacters(in: .whitespaces)
        if !trimmed.isEmpty, !FileManager.default.isExecutableFile(atPath: trimmed) {
            problem = "\(trimmed) is not an executable file."
            return
        }
        problem = nil
        update(model) { $0.svnExecutablePath = trimmed }
    }
}

private struct CheckSettings: View {
    @Environment(AppModel.self) private var model
    @State private var problem: String?

    var body: some View {
        Form {
            Section("PHP lint") {
                LabeledContent("PHP") {
                    if let phpPath {
                        Label("Uses \(phpPath)", systemImage: "checkmark.circle.fill").foregroundStyle(.green).textSelection(.enabled)
                    } else {
                        Label("Not found. Install it with `brew install php`.", systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    }
                }
            }
            Section("Plugin Check") {
                LabeledContent("WordPress install") {
                    HStack {
                        Text(model.settings.pluginCheckWordPressPath.map { ($0 as NSString).abbreviatingWithTildeInPath } ?? "Off")
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if model.settings.pluginCheckWordPressPath != nil {
                            Button("Turn Off") { update(model) { $0.pluginCheckWordPressPath = nil } }
                        }
                        Button("Choose…", action: chooseWordPress)
                    }
                }
                if let problem { Text(problem).font(.caption).foregroundStyle(.red) }
            }
            Section {
                Hint("Plugin Check runs `wp plugin check` in a local WordPress install that has the Plugin Check plugin active (WP-CLI: `brew install wp-cli`). Findings are shown as warnings; WordPress.org runs the same checks on every release.")
            }
        }
        .formStyle(.grouped)
    }

    private var phpPath: String? { CodeCheckService.phpCandidates.first(where: FileManager.default.isExecutableFile(atPath:)) }

    private func chooseWordPress() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.prompt = "Use WordPress"
        panel.message = "Choose the folder that contains wp-config.php"
        guard panel.runModal() == .OK, let wordpress = panel.url?.path else { return }
        let fm = FileManager.default
        let hasConfig = fm.fileExists(atPath: (wordpress as NSString).appendingPathComponent("wp-config.php"))
            || fm.fileExists(atPath: ((wordpress as NSString).deletingLastPathComponent as NSString).appendingPathComponent("wp-config.php"))
        guard hasConfig, fm.fileExists(atPath: (wordpress as NSString).appendingPathComponent("wp-load.php")) else {
            problem = "\(wordpress) is not a WordPress install (no wp-load.php / wp-config.php)."
            return
        }
        problem = nil
        update(model) { $0.pluginCheckWordPressPath = wordpress }
    }
}

private struct AccountSettings: View {
    @Environment(AppModel.self) private var model
    @State private var token = ""
    @State private var stored = false
    @State private var problem: String?

    var body: some View {
        Form {
            Section("GitHub releases") {
                if stored {
                    LabeledContent("Release token") {
                        HStack {
                            Label("Stored in Keychain", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                            Button("Remove", role: .destructive) {
                                try? model.keychain.deletePassword(for: .githubReleaseToken)
                                stored = false
                            }
                        }
                    }
                } else {
                    HStack {
                        SecureField("Release token", text: $token, prompt: Text("Needed only for products that create GitHub releases"))
                            .onSubmit(save)
                        Button("Save", action: save).disabled(token.isEmpty)
                    }
                }
                if let problem { Text(problem).font(.caption).foregroundStyle(.red) }
            }
            Section {
                Hint("A fine-grained token limited to your plugin and theme repositories, with Contents: read and write. It is stored in the Keychain. WordPress.org SVN passwords are asked for on the first commit and kept in the Keychain too.")
            }
        }
        .formStyle(.grouped)
        .onAppear { stored = ((try? model.keychain.password(for: .githubReleaseToken)) ?? nil) != nil }
    }

    private func save() {
        guard !token.isEmpty else { return }
        do {
            try model.keychain.setPassword(token, for: .githubReleaseToken)
            token = ""
            stored = true
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }
}

private struct UpdateSettings: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @Environment(\.openURL) private var openURL
    @State private var automaticChecks = false
    @State private var automaticDownloads = false

    var body: some View {
        Form {
            Section {
                HStack(spacing: 14) {
                    AppIcon(size: 48)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(headline).font(.headline)
                        Text(detail).font(.callout).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if updater.available != nil {
                        Button("Update…") { updater.checkForUpdates() }
                            .buttonStyle(.borderedProminent)
                    } else {
                        Button("Check Now") { updater.checkForUpdates() }
                            .disabled(!updater.canCheckForUpdates)
                    }
                }
                .padding(.vertical, 4)
            }
            if updater.isConfigured {
                Section("Preferences") {
                    Toggle("Check for updates automatically", isOn: $automaticChecks)
                        .onChange(of: automaticChecks) { _, value in updater.automaticallyChecksForUpdates = value }
                    Toggle("Download updates in the background", isOn: $automaticDownloads)
                        .onChange(of: automaticDownloads) { _, value in updater.automaticallyDownloadsUpdates = value }
                        .disabled(!automaticChecks)
                    Picker("Channel", selection: Binding(get: { model.settings.updateChannel }, set: { value in
                        update(model) { $0.updateChannel = value }
                        updater.channel = value
                    })) {
                        Text("Stable").tag(ReleaseChannel.stable)
                        Text("Beta").tag(ReleaseChannel.beta)
                    }
                }
                Section("Source") {
                    LabeledContent {
                        if let feed = updater.feedURL {
                            Button {
                                openURL(feed)
                            } label: {
                                Label(feed.host() ?? feed.absoluteString, systemImage: "arrow.up.right.square")
                            }
                            .buttonStyle(InlineLinkButtonStyle())
                            .linkCursor()
                        }
                    } label: {
                        Text("Update feed")
                        Text("Signed appcast, read directly; no GitHub API calls or rate limits.")
                    }
                }
            } else {
                Section {
                    LabeledContent("In-app updates", value: updater.disabledReason ?? "Disabled")
                }
            }
            Section {
                Hint("Updates download over HTTPS and install only if their EdDSA signature matches the key built into this app.")
            }
        }
        .formStyle(.grouped)
        .onAppear {
            automaticChecks = updater.automaticallyChecksForUpdates
            automaticDownloads = updater.automaticallyDownloadsUpdates
        }
    }

    private var headline: String {
        if let update = updater.available { return "ReleaseDock \(update.version) is available" }
        return "ReleaseDock \(AppModel.appVersion)"
    }

    private var detail: String {
        var parts = [updater.available == nil ? "Version \(AppModel.appVersion) (\(AppModel.appBuild))" : "You have \(AppModel.appVersion)"]
        if let checked = updater.lastUpdateCheckDate { parts.append("checked \(checked.formatted(.relative(presentation: .named)))") }
        return parts.joined(separator: " · ")
    }
}

private struct AboutSettings: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(spacing: 0) {
            AppIcon(size: 88).padding(.top, 18)
            Text("ReleaseDock").font(.title.weight(.bold)).tracking(-0.3).padding(.top, 6)
            Text("Version \(AppModel.appVersion) (\(AppModel.appBuild))").foregroundStyle(.secondary).textSelection(.enabled)
            Text("Ship WordPress.org plugins and themes from a ZIP or folder: inspect, validate, preview and commit to SVN safely.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .frame(maxWidth: 400)
                .padding(.top, 10)
            if let repository {
                HStack(spacing: 8) {
                    Button("Changelog") { openURL(repository.appendingPathComponent("blob/main/CHANGELOG.md")) }
                    Button("GitHub") { openURL(repository) }
                    Button("Report an Issue") { openURL(repository.appendingPathComponent("issues/new")) }
                }
                .padding(.top, 14)
            }
            Form {
                LabeledContent("macOS", value: ProcessInfo.processInfo.operatingSystemVersionString)
                LabeledContent("Subversion", value: model.svnInstallation?.version ?? "Not found")
                LabeledContent("Updates by") {
                    Button("Sparkle") { openURL(URL(string: "https://sparkle-project.org")!) }
                        .buttonStyle(InlineLinkButtonStyle())
                        .linkCursor()
                }
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            .frame(height: 150)
            Text("© 2026 Mahbubur Rahman · MIT License").font(.caption).foregroundStyle(.tertiary)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity)
    }

    private var repository: URL? {
        guard let name = Bundle.main.object(forInfoDictionaryKey: "RDGitHubRepository") as? String, !name.isEmpty else { return nil }
        return URL(string: "https://github.com/\(name)")
    }
}

/// Hides the window title, so the Settings tabs sit beside the window buttons.
private struct WindowTitleHidden: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { TitleHidingView() }
    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class TitleHidingView: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            window?.titleVisibility = .hidden
        }
    }
}
