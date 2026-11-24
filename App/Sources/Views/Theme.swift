import ReleaseDockCore
import SwiftUI

// Shared building blocks of the native macOS look: cards on the window background,
// System Settings-style grouped rows, tinted status pills and product glyphs.

extension View {
    /// A raised card: window background, hairline border and a soft shadow.
    func card(padding: CGFloat = 16) -> some View {
        self
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(.separator.opacity(0.6), lineWidth: 0.5))
            .shadow(color: .black.opacity(0.05), radius: 1.5, y: 1)
    }
}

/// Large page title with tight tracking, as in macOS 26 apps.
struct PageTitle: View {
    let title: String
    var subtitle: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.system(size: 26, weight: .bold)).tracking(-0.5)
            if let subtitle { Text(subtitle).foregroundStyle(.secondary) }
        }
    }
}

/// Section heading above a card or group.
struct SectionHeading<Accessory: View>: View {
    let title: String
    let accessory: Accessory

    init(_ title: String, @ViewBuilder accessory: () -> Accessory) {
        self.title = title
        self.accessory = accessory()
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title).font(.headline)
            Spacer()
            accessory
        }
        .padding(.top, 10)
    }
}

extension SectionHeading where Accessory == EmptyView {
    init(_ title: String) {
        self.title = title
        accessory = EmptyView()
    }
}

/// Rows on a quiet fill separated by inset hairlines (System Settings grouping).
struct GroupedRows<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .background(.quinary.opacity(0.6), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}

/// One row of a `GroupedRows`, with a hairline above unless it is the first.
struct GroupedRow<Content: View>: View {
    var isFirst = false
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            if !isFirst { Divider().padding(.leading, 14) }
            HStack(alignment: .firstTextBaseline, spacing: 12) { content }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct StatusPill: View {
    let text: String
    var color: Color = .secondary

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .foregroundStyle(color)
            .background(color.opacity(0.15), in: Capsule())
    }
}

extension ReleaseState {
    var tint: Color {
        switch self {
        case .completed: .green
        case .failed: .red
        case .cancelled: .secondary
        case .recoveryRequired: .orange
        case .previewed, .approved: .blue
        default: .accentColor
        }
    }
}

/// Gradient tile with the product type's symbol.
struct ProductGlyph: View {
    let type: ProductType
    var size: CGFloat = 36

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.25, style: .continuous)
            .fill(LinearGradient(colors: type == .plugin ? [Color(red: 0.29, green: 0.66, blue: 1), Color(red: 0.04, green: 0.42, blue: 0.88)] : [Color(red: 0.82, green: 0.49, blue: 0.96), Color(red: 0.56, green: 0.23, blue: 0.82)], startPoint: .top, endPoint: .bottom))
            .frame(width: size, height: size)
            .overlay {
                Image(systemName: type == .plugin ? "powerplug.fill" : "paintpalette.fill")
                    .font(.system(size: size * 0.48, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .shadow(color: .black.opacity(0.12), radius: 1, y: 0.5)
            .accessibilityHidden(true)
    }
}

/// Icon in a tinted rounded square, for tiles and timelines.
struct TintedSymbol: View {
    let systemImage: String
    var color: Color = .accentColor
    var size: CGFloat = 32
    var circle = false

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.46, weight: .semibold))
            .foregroundStyle(color)
            .frame(width: size, height: size)
            .background(color.opacity(0.15), in: circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)))
    }
}

/// Bottom bar on a material, with a note on the left and actions on the right.
struct ActionBar<Note: View, Actions: View>: View {
    @ViewBuilder var note: Note
    @ViewBuilder var actions: Actions

    var body: some View {
        HStack(spacing: 10) {
            note
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(2)
            Spacer(minLength: 12)
            actions
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(.bar)
        .overlay(alignment: .top) { Divider() }
    }
}

/// Dashed drop area for a ZIP or folder.
struct DropArea<Actions: View>: View {
    let title: String
    let subtitle: String
    var compact = false
    let onDrop: (URL) -> Void
    @ViewBuilder var actions: Actions
    @State private var targeted = false

    var body: some View {
        let layout = compact ? AnyLayout(HStackLayout(spacing: 16)) : AnyLayout(VStackLayout(spacing: 10))
        layout {
            TintedSymbol(systemImage: "doc.zipper", size: 44)
            VStack(alignment: compact ? .leading : .center, spacing: 2) {
                Text(title).font(.headline)
                Text(subtitle).foregroundStyle(.secondary).multilineTextAlignment(compact ? .leading : .center)
            }
            if compact { Spacer(minLength: 8) }
            HStack(spacing: 8) { actions }
        }
        .padding(compact ? 18 : 26)
        .frame(maxWidth: .infinity)
        .background(Color.accentColor.opacity(targeted ? 0.14 : 0.05), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(Color.accentColor.opacity(targeted ? 1 : 0.4), style: StrokeStyle(lineWidth: 1.5, dash: [6, 4]))
        )
        .animation(.easeOut(duration: 0.15), value: targeted)
        .dropDestination(for: URL.self) { urls, _ in
            guard let url = urls.first else { return false }
            onDrop(url)
            return true
        } isTargeted: { targeted = $0 }
    }
}

enum PackagePicker {
    /// NSOpenPanel for a ZIP (`zip: true`) or a folder.
    @MainActor
    static func choose(zip: Bool) -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = zip
        panel.canChooseDirectories = !zip
        panel.allowsMultipleSelection = false
        if zip { panel.allowedContentTypes = [.zip] }
        panel.prompt = zip ? "Use Package" : "Use Folder"
        return panel.runModal() == .OK ? panel.url : nil
    }
}
