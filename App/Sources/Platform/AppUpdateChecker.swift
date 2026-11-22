import Foundation
import Observation
import ReleaseDockCore

/// Checks GitHub for a newer ReleaseDock release, caches the answer, and notifies
/// once per new version. Installing updates is Sparkle's job (signed and verified);
/// this only tells the user an update exists.
@MainActor
@Observable
final class AppUpdateChecker {
    private let github: GitHubServicing
    private let store: Store
    private let keychain: KeychainServicing
    private let notifications: NotificationServicing

    private(set) var cache: ReleaseCheckCache?
    private(set) var isChecking = false
    private(set) var lastError: String?

    static let cacheKey = "github.releaseCache"
    static let notifiedKey = "github.notifiedVersion"
    /// Automatic checks reuse a cached answer younger than this, sparing the 60/hour anonymous limit.
    static let minimumInterval: TimeInterval = 3600

    init(github: GitHubServicing, store: Store, keychain: KeychainServicing, notifications: NotificationServicing) {
        self.github = github
        self.store = store
        self.keychain = keychain
        self.notifications = notifications
        cache = try? store.setting(ReleaseCheckCache.self, forKey: Self.cacheKey)
    }

    var latestRelease: GitHubRelease? { cache?.release }

    var isUpdateAvailable: Bool {
        guard let version = latestRelease?.version else { return false }
        return PHPVersion.compare(version, AppModel.appVersion) > 0
    }

    func check(settings: AppSettings, manual: Bool) async {
        guard let repository = GitHubRepository(string: settings.githubRepository) else {
            lastError = settings.githubRepository.isEmpty ? nil : "“\(settings.githubRepository)” is not a GitHub repository (use owner/name)."
            return
        }
        if !manual, let cache, cache.repository == repository.fullName, cache.channel == settings.updateChannel,
           Date().timeIntervalSince(cache.checkedAt) < Self.minimumInterval {
            return
        }
        isChecking = true
        defer { isChecking = false }
        do {
            let token = try keychain.password(for: .githubToken)
            let result = try await github.latestRelease(in: repository, channel: settings.updateChannel, token: token, cache: cache)
            cache = result
            lastError = nil
            try? store.setSetting(result, forKey: Self.cacheKey)
            await notifyIfNew()
        } catch {
            lastError = error.localizedDescription
            Log.github.error("Update check failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    private func notifyIfNew() async {
        guard isUpdateAvailable, let release = latestRelease else { return }
        let notified = try? store.setting(String.self, forKey: Self.notifiedKey)
        guard notified != release.version else { return }
        try? store.setSetting(release.version, forKey: Self.notifiedKey)
        await notifications.notify(
            title: "ReleaseDock \(release.version) is available",
            body: "You are running \(AppModel.appVersion). Open ReleaseDock → GitHub Updates to install.",
            identifier: "app-update-\(release.version)"
        )
    }
}
