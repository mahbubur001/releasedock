import ReleaseDockCore
import SwiftUI

struct GitHubUpdatesView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @Environment(\.openURL) private var openURL

    private var checker: AppUpdateChecker { model.updateChecker }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                SectionCard(title: "ReleaseDock", systemImage: "app.badge") {
                    KeyValueRow(key: "Installed version", value: "\(AppModel.appVersion) (\(AppModel.appBuild))")
                    KeyValueRow(key: "Repository", value: model.settings.githubRepository.isEmpty ? "Not configured" : model.settings.githubRepository)
                    KeyValueRow(key: "Channel", value: model.settings.updateChannel == .stable ? "Stable" : "Beta")
                    if let cache = checker.cache {
                        KeyValueRow(key: "Last checked", value: cache.checkedAt.formatted(date: .abbreviated, time: .shortened))
                        if let remaining = cache.rateLimit?.remaining, let limit = cache.rateLimit?.limit {
                            KeyValueRow(key: "API rate limit", value: "\(remaining) of \(limit) requests left")
                        }
                    }
                    if let error = checker.lastError {
                        Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(.orange)
                    }
                    HStack {
                        Button(checker.isChecking ? "Checking…" : "Check GitHub Now") {
                            Task { await checker.check(settings: model.settings, manual: true) }
                        }
                        .disabled(checker.isChecking || model.settings.githubRepository.isEmpty)
                        if model.settings.githubRepository.isEmpty {
                            Button("Configure…") { model.selection = .settings }
                        }
                    }
                }

                if let release = checker.latestRelease {
                    SectionCard(title: checker.isUpdateAvailable ? "Update available: \(release.version)" : "Latest release: \(release.version)", systemImage: checker.isUpdateAvailable ? "arrow.down.circle.fill" : "checkmark.circle") {
                        if let published = release.publishedAt {
                            KeyValueRow(key: "Published", value: published.formatted(date: .abbreviated, time: .omitted))
                        }
                        if release.prerelease { Label("Pre-release", systemImage: "flask").foregroundStyle(.orange) }
                        if let body = release.body, !body.isEmpty {
                            Text("Release notes").font(.subheadline.bold())
                            Text((try? AttributedString(markdown: body, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(body))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        HStack {
                            if checker.isUpdateAvailable {
                                Button("Install Update…") { updater.checkForUpdates() }
                                    .buttonStyle(.borderedProminent)
                                    .disabled(!updater.canCheckForUpdates)
                                    .help(updater.disabledReason ?? "Downloads, verifies the signature and installs the update with Sparkle.")
                            }
                            Button("Open on GitHub") { openURL(release.htmlURL) }
                        }
                        if checker.isUpdateAvailable, !updater.isConfigured {
                            Text("In-app installation is unavailable: \(updater.disabledReason ?? "not configured"). Download the signed release from GitHub instead.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }

                SectionCard(title: "How updates are verified", systemImage: "lock.shield") {
                    Text("ReleaseDock checks the GitHub REST API for the newest release and notifies you. Installation uses Sparkle 2, which only installs an update whose EdDSA signature matches the key built into this app and whose Developer ID signature matches the installed app.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            .padding(20)
        }
        .navigationTitle("GitHub Updates")
    }
}
