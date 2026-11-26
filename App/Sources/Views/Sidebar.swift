import AppKit
import ReleaseDockCore
import SwiftUI

/// Native source-list sidebar: sections, badges, and the svn status and app identity
/// footer with the Settings button.
struct Sidebar: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        @Bindable var model = model
        List(selection: $model.selection) {
            Section("Releases") {
                row(.dashboard)
                row(.products)
                row(.wizard)
                row(.history)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) { footer }
    }

    private func row(_ item: SidebarItem) -> some View {
        Label(item.title, systemImage: item.symbol)
            .badge(badge(for: item))
            .tag(item)
    }

    private func badge(for item: SidebarItem) -> Int {
        switch item {
        case .products: model.products.count
        case .wizard: model.releasesAwaitingApproval.count
        case .history: model.releases.filter { $0.state == .recoveryRequired }.count
        default: 0
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 10) {
            SVNStatusFooter()
            Divider()
            HStack(spacing: 10) {
                Image(nsImage: NSApplication.shared.applicationIconImage)
                    .resizable()
                    .frame(width: 28, height: 28)
                VStack(alignment: .leading, spacing: 0) {
                    Text("ReleaseDock").font(.callout.weight(.semibold)).lineLimit(1)
                    Text("Version \(AppModel.appVersion)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                Button {
                    openSettings()
                } label: {
                    Image(systemName: "gearshape")
                        .font(.system(size: 15))
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
                .help("Settings… (⌘,)")
                .accessibilityLabel("Settings")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }
}
