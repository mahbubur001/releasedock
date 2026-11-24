import ReleaseDockCore
import SwiftUI

/// ReleaseDock's own updates: status first, then release notes, source and verification.
struct GitHubUpdatesView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @Environment(\.openURL) private var openURL

    private var checker: AppUpdateChecker { model.updateChecker }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                status
                if let release = checker.latestRelease, let body = release.body, !body.isEmpty {
                    SectionHeading(checker.isUpdateAvailable ? "What's new in \(release.version)" : "Release notes for \(release.version)")
                    Text((try? AttributedString(markdown: body, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(body))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .card()
                }
                SectionHeading("Source")
                GroupedRows {
                    GroupedRow(isFirst: true) {
                        label("GitHub repository")
                        if let repository = GitHubRepository(string: model.settings.githubRepository) {
                            Button {
                                openURL(repository.htmlURL)
                            } label: {
                                Label(repository.fullName, systemImage: "arrow.up.right.square")
                            }
                            .buttonStyle(InlineLinkButtonStyle())
                            .linkCursor()
                            Spacer()
                        } else {
                            value("Not configured")
                            Button("Configure…") { model.selection = .settings }
                        }
                    }
                    GroupedRow {
                        label("Channel")
                        value(model.settings.updateChannel == .stable ? "Stable" : "Beta")
                    }
                    GroupedRow {
                        label("Install updates")
                        value(updater.isConfigured ? "Sparkle 2 · EdDSA signature required" : (updater.disabledReason ?? "Disabled"))
                    }
                    if let rate = checker.cache?.rateLimit, let remaining = rate.remaining, let limit = rate.limit {
                        GroupedRow {
                            label("API rate limit")
                            value("\(remaining) of \(limit) requests left")
                        }
                    }
                }
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "lock.shield").foregroundStyle(Color.accentColor).font(.title3)
                    Text("Updates download over HTTPS and install only if the EdDSA signature matches the public key built into this app. Developer ID builds are also checked against the installed app's code signature.")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .card()
                .padding(.top, 4)
            }
            .padding(.horizontal, 28)
            .padding(.vertical, 20)
            .frame(maxWidth: 720, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .navigationTitle("Updates")
    }

    private var status: some View {
        HStack(spacing: 16) {
            Image(nsImage: NSApplication.shared.applicationIconImage)
                .resizable()
                .frame(width: 56, height: 56)
            VStack(alignment: .leading, spacing: 3) {
                Text(headline).font(.title3.weight(.semibold))
                Text(detail).foregroundStyle(.secondary)
                if let error = checker.lastError {
                    Label(error, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(.orange)
                }
            }
            Spacer(minLength: 12)
            if checker.isUpdateAvailable, let release = checker.latestRelease {
                Button("Open on GitHub") { openURL(release.htmlURL) }
                Button("Install Update…") { updater.checkForUpdates() }
                    .buttonStyle(.borderedProminent)
                    .disabled(!updater.canCheckForUpdates)
                    .help(updater.disabledReason ?? "Downloads, verifies the signature and installs the update with Sparkle.")
            } else {
                Button {
                    Task { await checker.check(settings: model.settings, manual: true) }
                } label: {
                    Label(checker.isChecking ? "Checking…" : "Check Now", systemImage: "arrow.clockwise")
                }
                .disabled(checker.isChecking || model.settings.githubRepository.isEmpty)
            }
        }
        .card(padding: 20)
    }

    private var headline: String {
        guard let release = checker.latestRelease else { return "ReleaseDock \(AppModel.appVersion)" }
        return checker.isUpdateAvailable ? "ReleaseDock \(release.version) is available" : "ReleaseDock is up to date"
    }

    private var detail: String {
        var parts = ["Version \(AppModel.appVersion) (\(AppModel.appBuild))"]
        if let checked = checker.cache?.checkedAt { parts.append("checked \(checked.formatted(.relative(presentation: .named)))") }
        if checker.isUpdateAvailable, !updater.isConfigured { parts.append("install from GitHub") }
        return parts.joined(separator: " · ")
    }

    private func label(_ text: String) -> some View {
        Text(text).frame(width: 150, alignment: .leading)
    }

    private func value(_ text: String) -> some View {
        Text(text).font(.callout).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
    }
}
