import AppKit
import ObjectiveC

/// Shows the pointing-hand cursor over every enabled button in the app.
///
/// Most SwiftUI buttons on macOS are hosted AppKit controls (`SwiftUIAppKitButton`, an
/// `NSButton` subclass that inherits `NSButton.resetCursorRects`). AppKit controls ignore
/// SwiftUI's `pointerStyle`/hover modifiers, so the cursor rectangle is added at the
/// AppKit level: `NSButton.resetCursorRects` and `setEnabled:` are exchanged once, at
/// launch, for versions that add a pointing-hand rect while the button is enabled.
/// Buttons drawn by SwiftUI itself are covered by the `Button` wrapper's `pointerStyle`.
@MainActor
enum PointingHandCursor {
    private static var installed = false

    static func install() {
        guard !installed else { return }
        installed = true
        exchange(#selector(NSView.resetCursorRects), with: #selector(NSButton.releaseDock_resetCursorRects))
        exchange(#selector(setter: NSControl.isEnabled), with: #selector(NSButton.releaseDock_setEnabled(_:)))
    }

    /// Exchanges implementations on `NSButton` only. If `NSButton` inherits the original
    /// (e.g. `setEnabled:` from `NSControl`), the replacement is added to `NSButton` first so
    /// superclasses and sibling classes are untouched.
    private static func exchange(_ original: Selector, with replacement: Selector) {
        let cls: AnyClass = NSButton.self
        guard let originalMethod = class_getInstanceMethod(cls, original),
              let replacementMethod = class_getInstanceMethod(cls, replacement) else { return }
        if class_addMethod(cls, original, method_getImplementation(replacementMethod), method_getTypeEncoding(replacementMethod)) {
            class_replaceMethod(cls, replacement, method_getImplementation(originalMethod), method_getTypeEncoding(originalMethod))
        } else {
            method_exchangeImplementations(originalMethod, replacementMethod)
        }
    }
}

extension NSButton {
    @objc fileprivate func releaseDock_resetCursorRects() {
        releaseDock_resetCursorRects() // original implementation after the exchange
        if isEnabled, !isHidden, !bounds.isEmpty, !isStandardWindowButton {
            addCursorRect(bounds, cursor: .pointingHand)
        }
    }

    /// Close/minimise/zoom keep the system arrow.
    private var isStandardWindowButton: Bool {
        guard let window else { return false }
        return [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton, .toolbarButton, .documentIconButton]
            .contains { window.standardWindowButton($0) === self }
    }

    @objc fileprivate func releaseDock_setEnabled(_ enabled: Bool) {
        let changed = enabled != isEnabled
        releaseDock_setEnabled(enabled) // original implementation after the exchange
        if changed { window?.invalidateCursorRects(for: self) }
    }
}
