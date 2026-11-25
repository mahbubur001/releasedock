import ReleaseDockCore
import SwiftUI

@main
struct ReleaseDockApp: App {
    @State private var bootstrap = AppModel.bootstrap()
    @State private var updater = SparkleUpdateService()

    init() {
        #if DEBUG
        ViewHierarchyDump.scheduleIfRequested()
        #endif
    }

    var body: some Scene {
        WindowGroup("ReleaseDock", id: "main") {
            switch bootstrap {
            case .success(let model):
                RootView()
                    .environment(model)
                    .environment(updater)
                    .frame(minWidth: 980, minHeight: 640)
            case .failure(let error):
                StartupErrorView(error: error)
            }
        }
        .defaultSize(width: 1240, height: 820)
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(after: .appInfo) {
                SwiftUI.Button("Check for Updates…") { updater.checkForUpdates() }
                    .disabled(!updater.canCheckForUpdates)
            }
            CommandGroup(after: .sidebar) {
                if case .success(let model) = bootstrap {
                    ForEach(Array(SidebarItem.allCases.enumerated()), id: \.element) { index, item in
                        SwiftUI.Button(item.title) { model.selection = item }
                            .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: .command)
                    }
                }
            }
            CommandGroup(replacing: .newItem) {
                if case .success(let model) = bootstrap {
                    SwiftUI.Button("New Release…") { model.selection = .wizard }
                        .keyboardShortcut("n", modifiers: [.command])
                    SwiftUI.Button("Add Product…") {
                        model.selection = .products
                        model.showAddProduct = true
                    }
                    .keyboardShortcut("n", modifiers: [.command, .shift])
                }
            }
        }

        Settings {
            if case .success(let model) = bootstrap {
                SettingsView()
                    .environment(model)
                    .environment(updater)
            }
        }
    }
}

struct StartupErrorView: View {
    let error: Error

    var body: some View {
        ContentUnavailableView {
            Label("ReleaseDock could not start", systemImage: "exclamationmark.triangle")
        } description: {
            Text(error.localizedDescription)
        } actions: {
            Button("Quit") { NSApplication.shared.terminate(nil) }
        }
        .frame(minWidth: 520, minHeight: 320)
    }
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(SparkleUpdateService.self) private var updater
    @Environment(\.openSettings) private var openSettings
    @State private var columnVisibility = NavigationSplitViewVisibility.all

    var body: some View {
        @Bindable var model = model
        NavigationSplitView(columnVisibility: $columnVisibility) {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 210, ideal: 230, max: 280)
                .toolbar(removing: .sidebarToggle)
        } detail: {
            switch model.selection ?? .dashboard {
            case .dashboard: DashboardView()
            case .products: ProductsView()
            case .wizard: ReleaseWizardView()
            case .history: ReleaseHistoryView()
            }
        }
        .toolbar {
            // Window-level, so it stays available while the sidebar is hidden.
            ToolbarItem(placement: .navigation) {
                Button {
                    withAnimation { columnVisibility = columnVisibility == .detailOnly ? .all : .detailOnly }
                } label: {
                    Label(columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar", systemImage: "sidebar.left")
                }
                .help(columnVisibility == .detailOnly ? "Show Sidebar" : "Hide Sidebar")
            }
            if updater.available != nil {
                ToolbarItem(placement: .primaryAction) { UpdateToolbarButton() }
            }
        }
        .task {
            updater.channel = model.settings.updateChannel
            updater.onUpdateFound = { [weak model] update in model?.notifyAboutUpdate(update.version) }
            await model.start()
            updater.checkInBackground()
            #if DEBUG
            // UI verification: RELEASEDOCK_UI_TEST_SETTINGS=<tab> opens Settings; RELEASEDOCK_UI_TEST_UPDATE=1 the update window.
            let environment = ProcessInfo.processInfo.environment
            if environment["RELEASEDOCK_UI_TEST_SETTINGS"] != nil { openSettings() }
            if environment["RELEASEDOCK_UI_TEST_UPDATE"] != nil {
                try? await Task.sleep(for: .seconds(6))
                updater.checkForUpdates()
            }
            #endif
        }
        .onOpenURL { url in
            guard url.isFileURL else { return }
            model.pendingSource = url
            model.selection = .wizard
        }
        .sheet(isPresented: $model.showAddProduct) {
            AddProductView()
        }
        .alert(item: $model.alert) { alert in
            Alert(title: Text(alert.title), message: Text(alert.message))
        }
    }

}

struct SVNStatusFooter: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 6) {
            if let installation = model.svnInstallation, model.svnProblem == nil {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                Text("svn \(installation.version)")
            } else {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text(model.svnProblem == nil ? "Checking svn…" : "svn unavailable")
                    .help(model.svnProblem ?? "")
            }
            Spacer()
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}

/// "Update found · 0.2.3" with an Update button, shown in the toolbar while a newer
/// version is waiting; opens the Software Update window.
struct UpdateToolbarButton: View {
    @Environment(SparkleUpdateService.self) private var updater

    var body: some View {
        if let update = updater.available {
            HStack(spacing: 8) {
                Circle()
                    .fill(.green)
                    .frame(width: 7, height: 7)
                    .background(Circle().fill(.green.opacity(0.25)).frame(width: 13, height: 13))
                Text(status(update))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                Button(isBusy ? "Show" : "Update") { updater.checkForUpdates() }
                    .buttonStyle(.borderedProminent)
                    .buttonBorderShape(.capsule)
                    .controlSize(.small)
            }
            .padding(.leading, 10)
            .fixedSize()
            .help("ReleaseDock \(update.version) is available. You have \(AppModel.appVersion).")
        }
    }

    private var isBusy: Bool {
        switch updater.phase {
        case .downloading, .extracting, .installing: true
        default: false
        }
    }

    private func status(_ update: AvailableUpdate) -> AttributedString {
        var text: AttributedString
        switch updater.phase {
        case .downloading(let received, let expected) where expected > 0:
            text = AttributedString("Downloading \(Int(Double(received) / Double(expected) * 100))%")
        case .extracting, .installing:
            text = AttributedString("Installing")
        case .readyToInstall:
            text = AttributedString("Ready to install · ")
            var version = AttributedString(update.version)
            version.font = .callout.weight(.semibold)
            version.foregroundColor = .primary
            text += version
        default:
            text = AttributedString("Update found · ")
            var version = AttributedString(update.version)
            version.font = .callout.weight(.semibold)
            version.foregroundColor = .primary
            text += version
        }
        return text
    }
}
