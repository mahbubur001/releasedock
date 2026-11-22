import Combine
import Foundation
import Observation
import ReleaseDockCore
import Sparkle

/// Sparkle 2 integration. Sparkle verifies every update's EdDSA signature against
/// `SUPublicEDKey` (and the Developer ID code signature) before installing, and shows
/// its own download/extract/install progress and relaunch UI.
///
/// The updater is only started when both `SUFeedURL` and `SUPublicEDKey` are present
/// in Info.plist. Builds without them keep in-app updates disabled instead of falling
/// back to weaker verification.
@MainActor
@Observable
final class SparkleUpdateService: NSObject, UpdateServicing {
    private var controller: SPUStandardUpdaterController?
    private var cancellable: AnyCancellable?

    private(set) var canCheckForUpdates = false
    private(set) var disabledReason: String?
    var channel: ReleaseChannel = .stable {
        didSet { if oldValue != channel { controller?.updater.resetUpdateCycleAfterShortDelay() } }
    }

    var automaticallyChecksForUpdates: Bool {
        get { controller?.updater.automaticallyChecksForUpdates ?? false }
        set { controller?.updater.automaticallyChecksForUpdates = newValue }
    }

    var isConfigured: Bool { controller != nil }

    override init() {
        super.init()
        let info = Bundle.main.infoDictionary ?? [:]
        let feed = (info["SUFeedURL"] as? String)?.trimmingCharacters(in: .whitespaces) ?? ""
        let key = (info["SUPublicEDKey"] as? String)?.trimmingCharacters(in: .whitespaces) ?? ""
        guard let feedURL = URL(string: feed), feedURL.scheme == "https" else {
            disabledReason = "This build has no HTTPS update feed configured."
            return
        }
        guard !key.isEmpty else {
            disabledReason = "This build has no update signing key, so updates cannot be verified."
            return
        }
        let controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: self, userDriverDelegate: nil)
        self.controller = controller
        cancellable = controller.updater.publisher(for: \.canCheckForUpdates)
            .receive(on: RunLoop.main)
            .sink { [weak self] value in self?.canCheckForUpdates = value }
    }

    func checkForUpdates() {
        controller?.checkForUpdates(nil)
    }

    var lastUpdateCheckDate: Date? { controller?.updater.lastUpdateCheckDate }
}

extension SparkleUpdateService: SPUUpdaterDelegate {
    /// Items tagged `<sparkle:channel>beta</sparkle:channel>` are offered only on the beta channel.
    nonisolated func allowedChannels(for updater: SPUUpdater) -> Set<String> {
        MainActor.assumeIsolated { channel == .beta ? ["beta"] : [] }
    }
}
