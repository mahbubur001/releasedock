import ReleaseDockCore
import SwiftUI

@main
struct ReleaseDockApp: App {
    @State private var bootstrap = AppModel.bootstrap()
    @State private var updater = SparkleUpdateService()

    init() {
        PointingHandCursor.install()
        #if DEBUG
        ViewHierarchyDump.scheduleIfRequested()
        CursorSelfTest.scheduleIfRequested()
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
    @State private var columnVisibility = NavigationSplitViewVisibility.all

    var body: some View {
        @Bindable var model = model
        NavigationSplitView(columnVisibility: $columnVisibility) {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 210, ideal: 230, max: 280)
                // Our own toggle (below) replaces the system one so it gets the app's hover cursor.
                .toolbar(removing: .sidebarToggle)
        } detail: {
            switch model.selection ?? .dashboard {
            case .dashboard: DashboardView()
            case .products: ProductsView()
            case .wizard: ReleaseWizardView()
            case .history: ReleaseHistoryView()
            case .updates: GitHubUpdatesView()
            case .settings: SettingsView()
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
        }
        .task { await model.start() }
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
