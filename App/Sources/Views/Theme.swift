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

/// The product's WordPress.org icon when its assets folder has one, otherwise a
/// gradient tile with the product type's symbol.
struct ProductGlyph: View {
    @Environment(AppModel.self) private var model
    let type: ProductType
    var size: CGFloat = 36
    var productID: UUID?

    var body: some View {
        if let icon = model.product(productID).flatMap(ProductIcon.image) {
            Image(nsImage: icon)
                .resizable()
                .interpolation(.high)
                .aspectRatio(contentMode: .fill)
                .frame(width: size, height: size)
                .clipShape(RoundedRectangle(cornerRadius: size * 0.25, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: size * 0.25, style: .continuous).strokeBorder(.separator.opacity(0.6), lineWidth: 0.5))
                .shadow(color: .black.opacity(0.12), radius: 1, y: 0.5)
                .accessibilityHidden(true)
        } else {
            fallback
        }
    }

    private var fallback: some View {
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

/// Finds and caches a product's WordPress.org directory icon, looking in the
/// local assets folder first, then the working copy's SVN `assets/` folder.
@MainActor
enum ProductIcon {
    private static let names = ["icon-256x256.png", "icon-128x128.png", "icon-256x256.jpg", "icon-128x128.jpg", "icon-256x256.gif", "icon-128x128.gif", "icon.svg"]
    private static let cache = NSCache<NSString, NSImage>()

    static func image(for product: Product) -> NSImage? {
        let folders = [product.assetsPath.map { URL(fileURLWithPath: $0, isDirectory: true) }, product.workingCopyURL.appendingPathComponent("assets", isDirectory: true)]
        for folder in folders.compactMap({ $0 }) {
            for name in names {
                let url = folder.appendingPathComponent(name)
                guard let modified = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate else { continue }
                // Keyed by modification date so a replaced icon shows up without a relaunch.
                let key = "\(url.path)|\(modified.timeIntervalSince1970)" as NSString
                if let cached = cache.object(forKey: key) { return cached }
                if let image = still(at: url) ?? NSImage(contentsOf: url) {
                    cache.setObject(image, forKey: key)
                    return image
                }
            }
        }
        return nil
    }

    /// For an animated GIF, the frame with the most detail: these icons often
    /// start and end on a flat background with the logo only mid-animation.
    private static func still(at url: URL) -> NSImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let count = CGImageSourceGetCount(source)
        guard count > 1 else { return nil }
        let samples = Set((0..<24).map { $0 * count / 24 })
        let best = samples.compactMap { CGImageSourceCreateImageAtIndex(source, $0, nil) }.max { detail($0) < detail($1) }
        return best.map { NSImage(cgImage: $0, size: NSSize(width: $0.width, height: $0.height)) }
    }

    /// Colour variance of a 16×16 thumbnail.
    private static func detail(_ image: CGImage) -> Double {
        let side = 16
        var pixels = [UInt8](repeating: 0, count: side * side * 4)
        guard let context = CGContext(data: &pixels, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return 0 }
        context.draw(image, in: CGRect(x: 0, y: 0, width: side, height: side))
        return (0..<3).reduce(0) { total, channel in
            let values = stride(from: channel, to: pixels.count, by: 4).map { Double(pixels[$0]) }
            let mean = values.reduce(0, +) / Double(values.count)
            return total + values.reduce(0) { $0 + ($1 - mean) * ($1 - mean) } / Double(values.count)
        }
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
