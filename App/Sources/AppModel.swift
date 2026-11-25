import Foundation
import Observation
import ReleaseDockCore

/// User preferences stored in the database settings table.
struct AppSettings: Codable, Equatable {
    var updateChannel: ReleaseChannel = .stable
    var svnExecutablePath: String = ""
    var notificationsEnabled: Bool = true
    /// Local WordPress install with Plugin Check active; nil or empty turns Plugin Check off.
    var pluginCheckWordPressPath: String?
    /// Minutes a commit may take before its outcome is treated as uncertain; nil = 30.
    var commitTimeoutMinutes: Int?

    static let defaultCommitTimeoutMinutes = 30

    static let key = "app.settings"
}

enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case dashboard, products, wizard, history

    var id: String { rawValue }

    var title: String {
        switch self {
        case .dashboard: "Overview"
        case .products: "Products"
        case .wizard: "New Release"
        case .history: "History"
        }
    }

    var symbol: String {
        switch self {
        case .dashboard: "square.grid.2x2"
        case .products: "shippingbox"
        case .wizard: "paperplane"
        case .history: "clock.arrow.circlepath"
        }
    }
}

/// Composition root and shared app state. Release logic lives in ReleaseDockCore;
/// this type wires services together and exposes state to SwiftUI.
@MainActor
@Observable
final class AppModel {
    let paths: AppPaths
    let store: Store
    let keychain: KeychainServicing
    let svn: SVNService
    let productService: ProductService
    let repositorySetup: RepositorySetupService
    let pipeline: ReleasePipelineService
    let notifications: NotificationServicing
    let diagnostics: DiagnosticsService
    @ObservationIgnored private var directoryMonitor: Task<Void, Never>?
    @ObservationIgnored let codeCheckConfiguration = CodeCheckConfigurationStore()

    var products: [Product] = []
    var releases: [ReleaseRecord] = []
    var settings: AppSettings
    var svnInstallation: SVNInstallation?
    var svnProblem: String?
    var selection: SidebarItem? = .dashboard
    var wizardProductID: UUID?
    /// A ZIP or folder opened from Finder (Open With, Dock icon drop) for the wizard.
    var pendingSource: URL?
    var historySelection: UUID?
    /// The release wizard lives here so a prepared release survives switching sections.
    var wizard: ReleaseWizardModel!
    var showAddProduct = false
    var alert: AlertMessage?

    struct AlertMessage: Identifiable {
        let id = UUID()
        var title: String
        var message: String
    }

    static let appVersion: String = {
        let info = Bundle.main.infoDictionary
        return info?["CFBundleShortVersionString"] as? String ?? "0.0.0"
    }()

    static let appBuild: String = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0"

    init(paths: AppPaths, store: Store) {
        self.paths = paths
        self.store = store
        keychain = KeychainService()
        let loadedSettings = (try? store.setting(AppSettings.self, forKey: AppSettings.key)) ?? AppSettings()
        settings = loadedSettings
        codeCheckConfiguration.value = CodeCheckConfiguration(pluginCheckWordPressPath: loadedSettings.pluginCheckWordPressPath)
        let commitMinutes = loadedSettings.commitTimeoutMinutes ?? AppSettings.defaultCommitTimeoutMinutes
        svn = SVNService(
            executable: loadedSettings.svnExecutablePath.isEmpty ? nil : URL(fileURLWithPath: loadedSettings.svnExecutablePath),
            commitTimeout: .seconds(commitMinutes * 60),
            commitHTTPTimeoutSeconds: commitMinutes * 60
        )
        productService = ProductService(repository: store, paths: paths)
        repositorySetup = RepositorySetupService(svn: svn)
        notifications = UserNotificationService()
        diagnostics = DiagnosticsService(products: store, releases: store, paths: paths)
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store,
            releases: store,
            svn: svn,
            git: GitService(),
            keychain: keychain,
            staging: ReleaseStagingService(paths: paths),
            notifications: notifications,
            wordpressOrg: WordPressOrgService(),
            codeChecker: CodeCheckService(configuration: codeCheckConfiguration),
            builder: BuildService(),
            githubReleaser: GitHubService(),
            generator: "ReleaseDock \(Self.appVersion) (\(Self.appBuild))"
        ))
        #if DEBUG
        // UI verification: open a specific section, e.g. RELEASEDOCK_UI_TEST_SECTION=history.
        if let section = ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_SECTION"].flatMap(SidebarItem.init(rawValue:)) {
            selection = section
        }
        if ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_EXISTING_WC"] != nil { showAddProduct = true }
        if let package = ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_OPEN"] {
            pendingSource = URL(fileURLWithPath: package)
            selection = .wizard
        }
        #endif
        wizard = ReleaseWizardModel(model: self)
    }

    /// Releases that were prepared and wait for a decision.
    var releasesAwaitingApproval: [ReleaseRecord] {
        releases.filter { $0.state == .previewed || $0.state == .approved }
    }

    /// Opens an unfinished release in New Release, or its report in History.
    func open(_ release: ReleaseRecord) {
        if !release.state.isTerminal, release.state != .recoveryRequired {
            wizard.resume(release)
            selection = .wizard
        } else {
            historySelection = release.id
            selection = .history
        }
    }

    /// Starts a new release from a dropped or chosen package.
    func startRelease(from url: URL, productID: UUID? = nil) {
        if let productID { wizardProductID = productID }
        pendingSource = url
        selection = .wizard
    }

    /// Opens the store in the standard location, or returns the error to show.
    static func bootstrap() -> Result<AppModel, Error> {
        do {
            let paths = AppPaths(root: try AppPaths.defaultRoot())
            let store = try Store.open(paths: paths)
            return .success(AppModel(paths: paths, store: store))
        } catch {
            return .failure(error)
        }
    }

    func start() async {
        reload()
        #if DEBUG
        // UI verification: select a History entry by index, e.g. RELEASEDOCK_UI_TEST_HISTORY=2.
        if let index = ProcessInfo.processInfo.environment["RELEASEDOCK_UI_TEST_HISTORY"].flatMap(Int.init), releases.indices.contains(index) {
            historySelection = releases[index].id
        }
        #endif
        #if DEBUG
        KeySelfTest.scheduleIfRequested(model: self)
        #endif
        do {
            let reconciled = try await pipeline.reconcileInterruptedReleases()
            if reconciled.contains(where: { $0.state == .recoveryRequired }) {
                alert = AlertMessage(title: "A release needs recovery", message: "ReleaseDock was closed while a release was publishing. Open Release History and choose “Inspect Repository” to find out whether the commit reached WordPress.org.")
            }
        } catch {
            alert = AlertMessage(title: "Could not check interrupted releases", message: error.localizedDescription)
        }
        reload()
        await detectSVN()
        if settings.notificationsEnabled { _ = await notifications.requestAuthorization() }
        startDirectoryMonitor()
    }

    /// Posts one notification per new ReleaseDock version found by a background check.
    func notifyAboutUpdate(_ version: String) {
        let key = "update.notifiedVersion"
        guard settings.notificationsEnabled, (try? store.setting(String.self, forKey: key)) != version else { return }
        try? store.setSetting(version, forKey: key)
        Task {
            await notifications.notify(
                title: "ReleaseDock \(version) is available",
                body: "You are running \(Self.appVersion). Click Update in the ReleaseDock toolbar to install it.",
                identifier: "app-update-\(version)"
            )
        }
    }

    // MARK: WordPress.org directory

    /// Every 10 minutes, asks WordPress.org whether recent releases are offered to sites
    /// yet; the pipeline notifies when one goes live.
    func startDirectoryMonitor() {
        directoryMonitor?.cancel()
        directoryMonitor = Task { [weak self] in
            while !Task.isCancelled {
                guard let pipeline = self?.pipeline else { return }
                let refreshed = await pipeline.refreshAwaitingDirectoryReleases()
                if !refreshed.isEmpty { self?.reload() }
                try? await Task.sleep(for: .seconds(600))
            }
        }
    }

    /// Checks one release now. Returns the updated record, or nil after showing the error.
    @discardableResult
    func refreshDirectoryStatus(_ id: UUID) async -> ReleaseRecord? {
        do {
            let updated = try await pipeline.refreshDirectoryStatus(id)
            reload()
            return updated
        } catch {
            show(error, title: "Could not check WordPress.org")
            return nil
        }
    }

    func reload() {
        do {
            products = try store.allProducts()
            releases = try store.releases(productID: nil)
        } catch {
            alert = AlertMessage(title: "Could not load data", message: error.localizedDescription)
        }
    }

    func detectSVN() async {
        do {
            svnInstallation = try await svn.detectInstallation()
            svnProblem = svnInstallation?.supportsPasswordFromStdin == false
                ? "Subversion \(svnInstallation!.version) is too old. ReleaseDock needs 1.10 or newer to pass passwords securely."
                : nil
        } catch {
            svnInstallation = nil
            svnProblem = error.localizedDescription
        }
    }

    func saveSettings(_ newValue: AppSettings) {
        settings = newValue
        codeCheckConfiguration.value = CodeCheckConfiguration(pluginCheckWordPressPath: newValue.pluginCheckWordPressPath)
        do {
            try store.setSetting(newValue, forKey: AppSettings.key)
        } catch {
            alert = AlertMessage(title: "Could not save settings", message: error.localizedDescription)
        }
    }

    func product(_ id: UUID?) -> Product? {
        guard let id else { return nil }
        return products.first { $0.id == id }
    }

    func releases(for productID: UUID) -> [ReleaseRecord] {
        releases.filter { $0.productID == productID }
    }

    func show(_ error: Error, title: String = "Something went wrong") {
        alert = AlertMessage(title: title, message: error.localizedDescription)
    }

    // MARK: Credentials

    func hasStoredPassword(for product: Product) -> Bool {
        guard let username = product.svnUsername, let host = product.svnRepositoryURL.host else { return false }
        return ((try? keychain.password(for: .svn(host: host, username: username))) ?? nil) != nil
    }

    func storeCredentials(for product: Product, username: String, password: String) throws {
        guard let host = product.svnRepositoryURL.host else { throw ReleaseError.missingData("repository host") }
        var updated = product
        if let previous = product.svnUsername, previous != username {
            try? keychain.deletePassword(for: .svn(host: host, username: previous))
        }
        updated.svnUsername = username
        try keychain.setPassword(password, for: .svn(host: host, username: username))
        try productService.save(updated)
        reload()
    }

    func removeCredentials(for product: Product) throws {
        guard let username = product.svnUsername, let host = product.svnRepositoryURL.host else { return }
        try keychain.deletePassword(for: .svn(host: host, username: username))
        reload()
    }

    func exportDiagnostics() -> URL? {
        do {
            return try diagnostics.export(appVersion: "\(Self.appVersion) (\(Self.appBuild))", svnVersion: svnInstallation?.version)
        } catch {
            show(error, title: "Could not export diagnostics")
            return nil
        }
    }
}
