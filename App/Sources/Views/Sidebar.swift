import AppKit
import ReleaseDockCore
import SwiftUI

/// Custom navigation sidebar: app header, grouped sections with hover and selection
/// states, badges, and the svn status footer.
struct Sidebar: View {
    @Environment(AppModel.self) private var model

    private let sections: [(title: String, items: [SidebarItem])] = [
        ("Releases", [.dashboard, .products, .wizard, .history]),
        ("App", [.updates, .settings]),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(sections, id: \.title) { section in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(section.title.uppercased())
                                .font(.caption2.weight(.semibold))
                                .foregroundStyle(.tertiary)
                                .padding(.horizontal, 10)
                                .padding(.bottom, 4)
                            ForEach(section.items) { item in
                                SidebarRow(item: item, isSelected: (model.selection ?? .dashboard) == item, badge: badge(for: item)) {
                                    model.selection = item
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
            }
            Divider().padding(.horizontal, 12)
            SVNStatusFooter()
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(nsImage: NSApplication.shared.applicationIconImage)
                .resizable()
                .frame(width: 34, height: 34)
            VStack(alignment: .leading, spacing: 1) {
                Text("ReleaseDock").font(.headline)
                Text("WordPress.org releases").font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.top, 4)
        .padding(.bottom, 16)
    }

    private func badge(for item: SidebarItem) -> (count: Int, color: Color)? {
        switch item {
        case .history:
            let count = model.releases.filter { $0.state == .recoveryRequired }.count
            return count > 0 ? (count, .orange) : nil
        case .updates:
            return model.updateChecker.isUpdateAvailable ? (1, .blue) : nil
        case .products:
            return model.products.isEmpty ? nil : (model.products.count, .secondary)
        default:
            return nil
        }
    }
}

private struct SidebarRow: View {
    let item: SidebarItem
    let isSelected: Bool
    let badge: (count: Int, color: Color)?
    let action: () -> Void
    @State private var isHovering = false

    var body: some View {
        Button { action() } label: {
            HStack(spacing: 10) {
                Image(systemName: item.symbol)
                    .font(.system(size: 14, weight: .medium))
                    .frame(width: 20)
                    .foregroundStyle(isSelected ? Color.white : Color.accentColor)
                Text(item.title)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                Spacer(minLength: 4)
                if let badge {
                    Text("\(badge.count)")
                        .font(.caption2.weight(.bold))
                        .monospacedDigit()
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .foregroundStyle(isSelected ? Color.accentColor : (badge.color == .secondary ? Color.secondary : .white))
                        .background(Capsule().fill(isSelected ? Color.white : (badge.color == .secondary ? Color.primary.opacity(0.1) : badge.color)))
                }
            }
            .foregroundStyle(isSelected ? Color.white : Color.primary)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(isSelected ? Color.accentColor : (isHovering ? Color.primary.opacity(0.08) : Color.clear))
            }
            .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .animation(.easeOut(duration: 0.12), value: isHovering)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}
