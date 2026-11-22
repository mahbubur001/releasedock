// Real-input UI test driver for ReleaseDock (requires Accessibility permission for the host).
// Launches a Debug build with an isolated data folder, finds elements through the
// Accessibility API, and drives it with real CGEvent mouse/keyboard input.
import AppKit
import ApplicationServices

struct Element {
    let ref: AXUIElement
    let role: String
    let label: String
    let frame: CGRect
    var selected: Bool? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(ref, kAXSelectedAttribute as CFString, &value) == .success else { return nil }
        return (value as? Bool)
    }
}

/// Thrown (via exit) when input would not reach the app under test.
enum Safety {
    /// The only process that may receive input. Set before any input is sent.
    nonisolated(unsafe) static var targetPID: pid_t = 0

    /// Stops the run, sending nothing, unless the app under test is frontmost and owns the
    /// topmost window at `point`. Protects other apps from stray real clicks and key presses.
    static func ensureInputReachesTarget(at point: CGPoint?) {
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier
        guard targetPID != 0, front == targetPID else {
            abortRun("the app under test is not frontmost (frontmost pid \(front.map(String.init) ?? "none"))")
        }
        guard let point else { return }
        let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        let topmost = windows.first { window in
            guard (window[kCGWindowLayer as String] as? Int) == 0,
                  let bounds = window[kCGWindowBounds as String] as? [String: CGFloat] else { return false }
            return CGRect(x: bounds["X"] ?? 0, y: bounds["Y"] ?? 0, width: bounds["Width"] ?? 0, height: bounds["Height"] ?? 0).contains(point)
        }
        guard let owner = topmost?[kCGWindowOwnerPID as String] as? Int, pid_t(owner) == targetPID else {
            abortRun("another window covers \(Int(point.x)),\(Int(point.y)) (\(topmost?[kCGWindowOwnerName as String] as? String ?? "none"))")
        }
    }

    static func abortRun(_ reason: String) -> Never {
        print("ABORTED — no input sent: \(reason). Leave the test window in front and don't use the mouse or keyboard while tests run.")
        exit(2)
    }
}

enum UI {
    static func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
        var value: CFTypeRef?
        return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
    }

    static func frame(_ element: AXUIElement) -> CGRect {
        var point = CGPoint.zero, size = CGSize.zero
        if let p = attribute(element, kAXPositionAttribute) { AXValueGetValue(p as! AXValue, .cgPoint, &point) }
        if let s = attribute(element, kAXSizeAttribute) { AXValueGetValue(s as! AXValue, .cgSize, &size) }
        return CGRect(origin: point, size: size)
    }

    static func label(_ element: AXUIElement) -> String {
        for key in [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute, kAXHelpAttribute] {
            if let text = attribute(element, key) as? String, !text.isEmpty { return text }
        }
        // SwiftUI buttons often expose their text through static-text children.
        let children = (attribute(element, kAXChildrenAttribute) as? [AXUIElement]) ?? []
        return children.compactMap { attribute($0, kAXValueAttribute) as? String }.joined(separator: " ")
    }

    static func elements(in root: AXUIElement, limit: Int = 4000) -> [Element] {
        var result: [Element] = []
        var stack = [root]
        while let element = stack.popLast(), result.count < limit {
            let role = attribute(element, kAXRoleAttribute) as? String ?? ""
            result.append(Element(ref: element, role: role, label: label(element), frame: frame(element)))
            stack.append(contentsOf: (attribute(element, kAXChildrenAttribute) as? [AXUIElement]) ?? [])
        }
        return result
    }

    static func move(to point: CGPoint) {
        Safety.ensureInputReachesTarget(at: point)
        CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
    }

    static func click(_ point: CGPoint) {
        move(to: point); usleep(120_000)
        Safety.ensureInputReachesTarget(at: point)
        CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
        usleep(60_000)
        CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)
        usleep(400_000)
    }

    static func key(_ code: CGKeyCode) {
        Safety.ensureInputReachesTarget(at: nil)
        CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)?.post(tap: .cghidEventTap)
        usleep(50_000)
        CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)?.post(tap: .cghidEventTap)
        usleep(350_000)
    }

    /// The system-wide cursor currently shown. Outside an app's own process `NSCursor.arrow`
    /// is an empty image, so the hand is matched by image and anything else with the arrow's
    /// top-left hot spot is reported as the arrow.
    static func cursorName() -> String {
        guard let current = NSCursor.currentSystem else { return "unknown" }
        let hand = NSCursor.pointingHand
        if hand.hotSpot == current.hotSpot, hand.image.tiffRepresentation == current.image.tiffRepresentation { return "pointingHand" }
        if current.hotSpot.x < 8, current.hotSpot.y < 8 { return "arrow" }
        return "other(hotSpot=\(current.hotSpot))"
    }
}
