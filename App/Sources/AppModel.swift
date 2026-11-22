import Foundation
import Observation
import ReleaseDockCore

/// User preferences stored in the database settings table.
struct AppSettings: Codable, Equatable {
    var githubRepository: String = ""
    var updateChannel: ReleaseChannel = .stable
    var checkGitHubOnLaunch: Bool = true
    var svnExecutablePath: String = ""
    var notificationsEnabled: Bool = true

    static let key = "app.settings"
}

enum SidebarItem: String, CaseIterable, Identifiable, Hashable {
    case dashboard, products, wizard, history, updates, settings

    var id: String { rawValue }

    var title: String {
        switch self {
        case .dashboard: "Dashboard"
        case .products: "Products"
        case .wizard: "Release Wizard"
        case .history: "Release History"
        case .updates: "GitHub Updates"
        case .settings: "Settings"
        }
    }

    var symbol: String {
        switch self {
        case .dashboard: "gauge.with.dots.needle.33percent"
        case .products: "shippingbox"
        case .wizard: "wand.and.stars"
        case .history: "clock.arrow.circlepath"
        case .updates: "arrow.down.app"
        case .settings: "gearshape"
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
    let github: GitHubServicing
    let notifications: NotificationServicing
    let diagnostics: DiagnosticsService
    let updateChecker: AppUpdateChecker

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
        svn = SVNService(executable: loadedSettings.svnExecutablePath.isEmpty ? nil : URL(fileURLWithPath: loadedSettings.svnExecutablePath))
        productService = ProductService(repository: store, paths: paths)
        repositorySetup = RepositorySetupService(svn: svn)
        notifications = UserNotificationService()
        github = GitHubService()
        diagnostics = DiagnosticsService(products: store, releases: store, paths: paths)
        pipeline = ReleasePipelineService(dependencies: PipelineDependencies(
            products: store,
            releases: store,
            svn: svn,
            git: GitService(),
            keychain: keychain,
            staging: ReleaseStagingService(paths: paths),
            notifications: notifications,
            generator: "ReleaseDock \(Self.appVersion) (\(Self.appBuild))"
        ))
        updateChecker = AppUpdateChecker(github: github, store: store, keychain: keychain, notifications: notifications)
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
        if settings.githubRepository.isEmpty, let bundled = Bundle.main.object(forInfoDictionaryKey: "RDGitHubRepository") as? String, !bundled.isEmpty {
            settings.githubRepository = bundled
        }
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
        if settings.checkGitHubOnLaunch { await updateChecker.check(settings: settings, manual: false) }
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
