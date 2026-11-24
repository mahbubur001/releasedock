import ReleaseDockCore
import SwiftUI

struct ValidationStatusIcon: View {
    let status: ValidationStatus

    var body: some View {
        switch status {
        case .passed: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .warning: Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
        case .failed: Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
        case .skipped: Image(systemName: "minus.circle").foregroundStyle(.secondary)
        }
    }
}

struct SectionCard<Content: View>: View {
    let title: String
    var systemImage: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let systemImage {
                Label(title, systemImage: systemImage).font(.headline)
            } else {
                Text(title).font(.headline)
            }
            content
        }
        .card()
    }
}

struct KeyValueRow: View {
    let key: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(key).foregroundStyle(.secondary).frame(width: 130, alignment: .leading)
            Text(value)
                .font(monospaced ? .callout.monospaced() : .callout)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }
}

/// Fixed-width list column beside a flexible detail pane. Avoids `HSplitView`/`VSplitView`,
/// which collapse SwiftUI tables and hide panes inside a `NavigationSplitView` detail.
struct MasterDetailLayout<Master: View, Detail: View>: View {
    var masterWidth: CGFloat = 300
    @ViewBuilder var master: Master
    @ViewBuilder var detail: Detail

    var body: some View {
        HStack(spacing: 0) {
            master
                .frame(width: masterWidth)
                .frame(maxHeight: .infinity, alignment: .top)
                .background(.background.secondary)
            Divider()
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// A full-width clickable row with hover and selection styling.
struct SelectableRow<Content: View>: View {
    let isSelected: Bool
    let action: () -> Void
    @ViewBuilder var content: Content
    @State private var isHovering = false

    var body: some View {
        Button { action() } label: {
            content
                .padding(.horizontal, 10)
                .padding(.vertical, 9)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(isSelected ? Color.accentColor.opacity(0.18) : (isHovering ? Color.primary.opacity(0.06) : Color.clear))
                }
                .overlay {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(isSelected ? Color.accentColor.opacity(0.6) : Color.clear, lineWidth: 1)
                }
                .contentShape(RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .animation(.easeOut(duration: 0.12), value: isHovering)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

struct SearchField: View {
    @Binding var text: String
    var prompt: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField(prompt, text: $text).textFieldStyle(.plain)
            if !text.isEmpty {
                Button { text = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                    .buttonStyle(.plain)
                                            .help("Clear")
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 7).fill(Color.primary.opacity(0.06)))
    }
}

extension View {
    /// ↑/↓ move the selection through `ids` while this list is on screen, including while
    /// typing in its search box. Implemented with an AppKit key monitor because SwiftUI focus
    /// does not reliably land on a scrolling list of buttons.
    func arrowKeySelection<ID: Hashable & Sendable>(_ ids: [ID], selection: Binding<ID?>) -> some View {
        background(ArrowKeyMonitor(ids: ids.map(AnyHashable.init)) { newValue in
            if let id = newValue.base as? ID { selection.wrappedValue = id }
        } current: {
            selection.wrappedValue.map(AnyHashable.init)
        })
    }
}

private struct ArrowKeyMonitor: NSViewRepresentable {
    let ids: [AnyHashable]
    let select: (AnyHashable) -> Void
    let current: () -> AnyHashable?

    func makeNSView(context: Context) -> MonitorView { MonitorView() }

    func updateNSView(_ view: MonitorView, context: Context) {
        view.ids = ids
        view.select = select
        view.current = current
    }

    final class MonitorView: NSView {
        var ids: [AnyHashable] = []
        var select: (AnyHashable) -> Void = { _ in }
        var current: () -> AnyHashable? = { nil }
        private var monitor: Any?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor { NSEvent.removeMonitor(monitor) }
            monitor = nil
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self, let window = self.window, event.window === window else { return event }
                let offset: Int
                switch event.keyCode {
                case 125: offset = 1   // ↓
                case 126: offset = -1  // ↑
                default: return event
                }
                // Arrows typed in a single-line field (the list's search box) move through the list,
                // as in other searchable lists; multi-line text keeps its arrows.
                if let text = window.firstResponder as? NSTextView, !text.isFieldEditor { return event }
                if !event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty { return event }
                guard !self.ids.isEmpty else { return event }
                let index = self.current().flatMap { self.ids.firstIndex(of: $0) }
                let next = index.map { min(max($0 + offset, 0), self.ids.count - 1) } ?? 0
                self.select(self.ids[next])
                return nil
            }
        }
        // The monitor is removed in viewDidMoveToWindow when the view leaves its window.
    }
}
