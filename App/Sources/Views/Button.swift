import AppKit
import SwiftUI

/// App-wide replacement for `SwiftUI.Button` that adds `pointingHandCursor` to every
/// button. Declaring it in the app module shadows SwiftUI's `Button`. Menus,
/// `confirmationDialog`s and `commands` use `SwiftUI.Button` explicitly. Avoid AppKit-backed
/// styles such as `.link` (use `InlineLinkButtonStyle`): hover tracking overrides their
/// AppKit cursor rectangles.
struct Button<Label: View>: View {
    private let button: SwiftUI.Button<Label>
    @Environment(\.isEnabled) private var isEnabled

    init(action: @escaping @MainActor () -> Void, @ViewBuilder label: () -> Label) {
        button = SwiftUI.Button(action: action, label: label)
    }

    init(role: ButtonRole?, action: @escaping @MainActor () -> Void, @ViewBuilder label: () -> Label) {
        button = SwiftUI.Button(role: role, action: action, label: label)
    }

    var body: some View {
        button.pointingHandCursor(isEnabled)
    }
}

extension Button where Label == Text {
    init(_ titleKey: LocalizedStringKey, action: @escaping @MainActor () -> Void) {
        button = SwiftUI.Button(titleKey, action: action)
    }

    init<S: StringProtocol>(_ title: S, action: @escaping @MainActor () -> Void) {
        button = SwiftUI.Button(title, action: action)
    }

    init(_ titleKey: LocalizedStringKey, role: ButtonRole?, action: @escaping @MainActor () -> Void) {
        button = SwiftUI.Button(titleKey, role: role, action: action)
    }

    init<S: StringProtocol>(_ title: S, role: ButtonRole?, action: @escaping @MainActor () -> Void) {
        button = SwiftUI.Button(title, role: role, action: action)
    }
}

/// Link-looking button drawn by SwiftUI (`.link` is an AppKit control).
struct InlineLinkButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(Color.accentColor)
            .opacity(configuration.isPressed ? 0.6 : 1)
            .contentShape(Rectangle())
    }
}

extension View {
    /// Shows the pointing-hand cursor over this view (buttons and custom clickable rows).
    ///
    /// SwiftUI-drawn content can sit under hosting views that own cursor updates, so AppKit
    /// cursor rectangles are unreliable there. Instead the cursor is set from SwiftUI's own
    /// hover tracking on every pointer move (and via `pointerStyle` on macOS 15+).
    /// Do not apply it to AppKit-backed buttons: it overrides their cursor rectangles.
    /// AppKit-backed buttons are handled separately by `PointingHandCursor`.
    func pointingHandCursor(_ enabled: Bool = true) -> some View {
        modifier(PointingHandCursorModifier(enabled: enabled))
    }
}

private struct PointingHandCursorModifier: ViewModifier {
    let enabled: Bool
    @State private var id = UUID()

    func body(content: Content) -> some View {
        content
            .modifier(LinkPointerStyle(enabled: enabled))
            #if DEBUG
            .background { CursorTargetRecorder(enabled: enabled) }
            #endif
            .onContinuousHover { phase in
                switch phase {
                case .active: HoverCursor.enter(id, enabled: enabled)
                case .ended: HoverCursor.leave(id)
                }
            }
            .onChange(of: enabled) { _, enabled in HoverCursor.update(id, enabled: enabled) }
            // A view removed while hovered (e.g. replaced after a click) must not leave the hand behind.
            .onDisappear { HoverCursor.leave(id) }
    }
}

/// Hover "ended" for the previous view can arrive after "active" for the next one. Only the
/// view that most recently received hover may reset the cursor, so the late "ended" no
/// longer overwrites the hand that the next view just set.
@MainActor
enum HoverCursor {
    private static var owner: UUID?

    static func enter(_ id: UUID, enabled: Bool) {
        owner = id
        (enabled ? NSCursor.pointingHand : NSCursor.arrow).set()
    }

    static func update(_ id: UUID, enabled: Bool) {
        guard owner == id else { return }
        (enabled ? NSCursor.pointingHand : NSCursor.arrow).set()
    }

    static func leave(_ id: UUID) {
        guard owner == id else { return }
        owner = nil
        NSCursor.arrow.set()
    }
}

private struct LinkPointerStyle: ViewModifier {
    let enabled: Bool

    func body(content: Content) -> some View {
        if #available(macOS 15.0, *) {
            content.pointerStyle(enabled ? .link : nil)
        } else {
            content
        }
    }
}

#if DEBUG
/// Debug-only: records where hover-cursor targets are, so `CursorSelfTest` can visit them.
@MainActor
final class CursorTargetRegistry {
    static let shared = CursorTargetRegistry()
    private(set) var frames: [UUID: CGRect] = [:]
    func set(_ frame: CGRect?, for id: UUID) { frames[id] = frame }
}

private struct CursorTargetRecorder: View {
    let enabled: Bool
    @State private var id = UUID()

    var body: some View {
        GeometryReader { proxy in
            Color.clear
                .onAppear { CursorTargetRegistry.shared.set(enabled ? proxy.frame(in: .global) : nil, for: id) }
                .onChange(of: proxy.frame(in: .global)) { _, frame in CursorTargetRegistry.shared.set(enabled ? frame : nil, for: id) }
                .onDisappear { CursorTargetRegistry.shared.set(nil, for: id) }
        }
    }
}
#endif
