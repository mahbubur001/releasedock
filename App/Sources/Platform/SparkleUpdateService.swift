import AppKit
import Combine
import Foundation
import Observation
import ReleaseDockCore
import Sparkle
import SwiftUI

/// One version's notes in the update window.
struct VersionNotes: Identifiable, Equatable {
    var version: String
    var date: Date?
    var notes: ReleaseNotes
    var id: String { version }
}

/// The newest update Sparkle found, with the notes of every version since the installed one.
struct AvailableUpdate: Equatable {
    var version: String
    var date: Date?
    var size: UInt64
    var history: [VersionNotes]
}

enum UpdatePhase: Equatable {
    case idle
    case checking
    /// Waiting for Install, Later or Skip.
    case found
    case downloading(received: UInt64, expected: UInt64)
    case extracting(Double)
    case readyToInstall
    case installing
    case upToDate
    case failed(String)
}

/// Sparkle 2 with ReleaseDock's own user interface. Sparkle reads the signed appcast
/// (`SUFeedURL`, served from raw.githubusercontent.com, so no GitHub API rate limit),
/// downloads the update, verifies its EdDSA signature against `SUPublicEDKey` and the
/// Developer ID code signature, and installs it. This type only presents each step:
/// background checks surface as a toolbar button; the update window shows the notes,
/// progress and the install choice.
///
/// The updater is only started when both `SUFeedURL` and `SUPublicEDKey` are present
/// in Info.plist. Builds without them keep in-app updates disabled instead of falling
/// back to weaker verification.
@MainActor
@Observable
final class SparkleUpdateService: NSObject, UpdateServicing {
    @ObservationIgnored private var updater: SPUUpdater?
    @ObservationIgnored private var cancellable: AnyCancellable?
    @ObservationIgnored private var appcastItems: [SUAppcastItem] = []
    @ObservationIgnored private var choiceReply: ((SPUUserUpdateChoice) -> Void)?
    @ObservationIgnored private var cancellation: (() -> Void)?
    @ObservationIgnored private var acknowledgement: (() -> Void)?
    /// Set when Sparkle downloaded an update by itself and will install it on quit.
    @ObservationIgnored private var installNow: (() -> Void)?
    @ObservationIgnored private var window: UpdateWindowController?
    /// The user chose Install, so the window comes back when the download is ready.
    @ObservationIgnored private var installRequested = false
    /// Called when a background check finds a version, e.g. to post a notification.
    @ObservationIgnored var onUpdateFound: ((AvailableUpdate) -> Void)?

    private(set) var canCheckForUpdates = false
    private(set) var disabledReason: String?
    private(set) var available: AvailableUpdate?
    private(set) var phase: UpdatePhase = .idle

    var channel: ReleaseChannel = .stable {
        didSet { if oldValue != channel { updater?.resetUpdateCycleAfterShortDelay() } }
    }

    var automaticallyChecksForUpdates: Bool {
        get { updater?.automaticallyChecksForUpdates ?? false }
        set { updater?.automaticallyChecksForUpdates = newValue }
    }

    var automaticallyDownloadsUpdates: Bool {
        get { updater?.automaticallyDownloadsUpdates ?? false }
        set { updater?.automaticallyDownloadsUpdates = newValue }
    }

    var isConfigured: Bool { updater != nil }
    var lastUpdateCheckDate: Date? { updater?.lastUpdateCheckDate }
    var feedURL: URL? { updater?.feedURL }

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
        let updater = SPUUpdater(hostBundle: .main, applicationBundle: .main, userDriver: self, delegate: self)
        do {
            try updater.start()
        } catch {
            disabledReason = "The updater could not start: \(error.localizedDescription)"
            Log.updates.error("Sparkle did not start: \(error.localizedDescription, privacy: .public)")
            return
        }
        self.updater = updater
        cancellable = updater.publisher(for: \.canCheckForUpdates)
            .receive(on: RunLoop.main)
            .sink { [weak self] value in self?.canCheckForUpdates = value }
    }

    /// Opens the update window: the update already found, or a new check.
    func checkForUpdates() {
        if choiceReply != nil || installNow != nil || window != nil {
            presentWindow()
        } else {
            updater?.checkForUpdates()
        }
    }

    /// Quiet check, e.g. at launch: a found update shows in the toolbar, nothing else.
    func checkInBackground() {
        guard let updater, updater.automaticallyChecksForUpdates, updater.canCheckForUpdates else { return }
        updater.checkForUpdatesInBackground()
    }

    // MARK: Choices from the update window

    func install() {
        installRequested = true
        if let installNow, choiceReply == nil {
            phase = .installing
            installNow()
        } else {
            reply(.install)
        }
    }

    func later() {
        if choiceReply == nil {
            closeWindow()
        } else {
            reply(.dismiss)
        }
    }

    func skip() {
        available = nil
        reply(.skip)
    }

    func cancel() {
        installRequested = false
        let cancellation = cancellation
        self.cancellation = nil
        cancellation?()
    }

    func acknowledge() {
        let acknowledgement = acknowledgement
        self.acknowledgement = nil
        acknowledgement?()
        closeWindow()
    }

    private func reply(_ choice: SPUUserUpdateChoice) {
        let reply = choiceReply
        choiceReply = nil
        reply?(choice)
    }

    /// The window's close button: Later for a pending choice, Install on Quit for a
    /// downloaded update; a running download or check carries on in the background.
    fileprivate func windowDidClose() {
        window = nil
        switch phase {
        case .found, .readyToInstall: later()
        case .upToDate, .failed: acknowledge()
        default: break
        }
    }

    private func presentWindow() {
        if window == nil { window = UpdateWindowController(updater: self) }
        window?.show()
    }

    private func closeWindow() {
        let window = window
        self.window = nil
        window?.close()
    }

    private func makeUpdate(_ item: SUAppcastItem) -> AvailableUpdate {
        let installed = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0"
        let comparator = SUStandardVersionComparator.default
        let newer = appcastItems
            .filter {
                comparator.compareVersion($0.versionString, toVersion: installed) == .orderedDescending
                    && comparator.compareVersion($0.versionString, toVersion: item.versionString) != .orderedDescending
                    && ($0.channel == nil || $0.channel == item.channel || channel == .beta)
            }
            .sorted { comparator.compareVersion($0.versionString, toVersion: $1.versionString) == .orderedDescending }
        let items = newer.contains { $0.versionString == item.versionString } ? newer : [item] + newer
        return AvailableUpdate(
            version: item.displayVersionString,
            date: item.date,
            size: item.contentLength,
            history: items.map { VersionNotes(version: $0.displayVersionString, date: $0.date, notes: ReleaseNotes(html: $0.itemDescription ?? "")) }
        )
    }
}

extension SparkleUpdateService: SPUUpdaterDelegate {
    /// Items tagged `<sparkle:channel>beta</sparkle:channel>` are offered only on the beta channel.
    nonisolated func allowedChannels(for updater: SPUUpdater) -> Set<String> {
        MainActor.assumeIsolated { channel == .beta ? ["beta"] : [] }
    }

    nonisolated func updater(_ updater: SPUUpdater, didFinishLoading appcast: SUAppcast) {
        MainActor.assumeIsolated { appcastItems = appcast.items }
    }

    nonisolated func updaterDidNotFindUpdate(_ updater: SPUUpdater) {
        MainActor.assumeIsolated { available = nil }
    }

    /// With automatic downloads on, Sparkle installs on quit without any UI; keep the
    /// update visible in the toolbar so it can also be installed right away.
    nonisolated func updater(_ updater: SPUUpdater, willInstallUpdateOnQuit item: SUAppcastItem, immediateInstallationBlock immediateInstallHandler: @escaping () -> Void) -> Bool {
        // Sparkle calls delegates on the main thread; the handler never leaves it.
        nonisolated(unsafe) let handler = immediateInstallHandler
        return MainActor.assumeIsolated {
            let update = makeUpdate(item)
            available = update
            installNow = handler
            phase = .readyToInstall
            onUpdateFound?(update)
            return true
        }
    }
}

extension SparkleUpdateService: SPUUserDriver {
    func show(_ request: SPUUpdatePermissionRequest, reply: @escaping (SUUpdatePermissionResponse) -> Void) {
        // Info.plist turns automatic checks on (SUEnableAutomaticChecks), so Sparkle should not ask.
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: true, sendSystemProfile: false))
    }

    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) {
        self.cancellation = cancellation
        phase = .checking
        presentWindow()
    }

    func showUpdateFound(with appcastItem: SUAppcastItem, state: SPUUserUpdateState, reply: @escaping (SPUUserUpdateChoice) -> Void) {
        cancellation = nil
        let update = makeUpdate(appcastItem)
        available = update
        choiceReply = reply
        phase = state.stage == .notDownloaded ? .found : .readyToInstall
        if state.userInitiated || window != nil {
            presentWindow()
        } else {
            onUpdateFound?(update)
        }
    }

    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}

    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: any Error) {}

    func showUpdateNotFoundWithError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        cancellation = nil
        available = nil
        self.acknowledgement = acknowledgement
        phase = .upToDate
        presentWindow()
    }

    func showUpdaterError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        cancellation = nil
        installRequested = false
        Log.updates.error("Update failed: \(error.localizedDescription, privacy: .public)")
        guard window != nil else {
            // A background check failed: nothing to show, try again on the next check.
            phase = .idle
            acknowledgement()
            return
        }
        self.acknowledgement = acknowledgement
        phase = .failed(error.localizedDescription)
    }

    func showDownloadInitiated(cancellation: @escaping () -> Void) {
        self.cancellation = cancellation
        phase = .downloading(received: 0, expected: available?.size ?? 0)
    }

    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {
        phase = .downloading(received: 0, expected: expectedContentLength)
    }

    func showDownloadDidReceiveData(ofLength length: UInt64) {
        guard case .downloading(let received, let expected) = phase else { return }
        phase = .downloading(received: received + length, expected: max(expected, received + length))
    }

    func showDownloadDidStartExtractingUpdate() {
        cancellation = nil
        phase = .extracting(0)
    }

    func showExtractionReceivedProgress(_ progress: Double) {
        phase = .extracting(progress)
    }

    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        choiceReply = reply
        phase = .readyToInstall
        if installRequested { presentWindow() }
    }

    func showInstallingUpdate(withApplicationTerminated applicationTerminated: Bool, retryTerminatingApplication: @escaping () -> Void) {
        phase = .installing
    }

    func showUpdateInstalledAndRelaunched(_ relaunched: Bool, acknowledgement: @escaping () -> Void) {
        available = nil
        acknowledgement()
    }

    func showUpdateInFocus() {
        presentWindow()
    }

    func dismissUpdateInstallation() {
        choiceReply = nil
        cancellation = nil
        installRequested = false
        if acknowledgement == nil {
            phase = installNow == nil ? .idle : .readyToInstall
            closeWindow()
        }
    }
}

/// The "Software Update" window, created on demand.
@MainActor
final class UpdateWindowController: NSObject, NSWindowDelegate {
    private let panel: NSWindow
    private weak var updater: SparkleUpdateService?

    init(updater: SparkleUpdateService) {
        self.updater = updater
        panel = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 560),
            styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        super.init()
        panel.title = "Software Update"
        panel.titlebarAppearsTransparent = true
        panel.isReleasedWhenClosed = false
        panel.contentView = NSHostingView(rootView: UpdateWindowView().environment(updater))
        panel.setContentSize(NSSize(width: 600, height: 560))
        panel.center()
        panel.delegate = self
    }

    func show() {
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
    }

    func close() {
        panel.delegate = nil
        panel.close()
    }

    func windowWillClose(_ notification: Notification) {
        updater?.windowDidClose()
    }
}
