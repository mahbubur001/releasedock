import AppKit
import SwiftUI

// Cursor convention (macOS): buttons, switches, pop-ups and list rows keep the arrow.
// Only links to web pages show the pointing hand, via `linkCursor()`.

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
    /// Pointing-hand cursor for links to web pages. Not for buttons or rows.
    func linkCursor() -> some View {
        modifier(LinkCursorModifier())
    }
}

private struct LinkCursorModifier: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 15.0, *) {
            content.pointerStyle(.link)
        } else {
            content.onHover { inside in (inside ? NSCursor.pointingHand : NSCursor.arrow).set() }
        }
    }
}
