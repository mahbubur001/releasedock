import AppKit
import SwiftUI

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
    @State private var isHovering = false

    func body(content: Content) -> some View {
        content
            .modifier(LinkPointerStyle(enabled: enabled))
            #if DEBUG
            .background { CursorTargetRecorder(enabled: enabled) }
            #endif
            .onContinuousHover { phase in
                switch phase {
                case .active:
                    isHovering = true
                    (enabled ? NSCursor.pointingHand : NSCursor.arrow).set()
                case .ended:
                    guard isHovering else { return }
                    isHovering = false
                    NSCursor.arrow.set()
                }
            }
            .onChange(of: enabled) { _, enabled in
                if isHovering { (enabled ? NSCursor.pointingHand : NSCursor.arrow).set() }
            }
            // A view removed while hovered (e.g. replaced after a click) must not leave the hand behind.
            .onDisappear {
                if isHovering { NSCursor.arrow.set() }
            }
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
