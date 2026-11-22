#if DEBUG
import AppKit

/// Debug-only verification of hover cursors: RELEASEDOCK_UI_CURSOR_TEST=/path/results.txt
/// with RELEASEDOCK_UI_CURSOR_POINTS="name:x,y;name:x,y" (window coordinates, origin
/// bottom-left). For each point the real pointer is warped there, mouse-moved events are
/// delivered through the normal event path, and the resulting current cursor is recorded.
@MainActor
enum CursorSelfTest {
    static func scheduleIfRequested() {
        let env = ProcessInfo.processInfo.environment
        guard let output = env["RELEASEDOCK_UI_CURSOR_TEST"], let spec = env["RELEASEDOCK_UI_CURSOR_POINTS"] else { return }
        let explicitPoints: [(String, NSPoint)] = spec == "auto" ? [] : spec.split(separator: ";").compactMap { item in
            let parts = item.split(separator: ":")
            guard parts.count == 2 else { return nil }
            let xy = parts[1].split(separator: ",").compactMap { Double($0) }
            guard xy.count == 2 else { return nil }
            return (String(parts[0]), NSPoint(x: xy[0], y: xy[1]))
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
            Task { @MainActor in
                var lines: [String] = []
                guard let window = NSApplication.shared.windows.first(where: { $0.isVisible && $0.contentView != nil }) else {
                    try? "no window".write(toFile: output, atomically: true, encoding: .utf8)
                    return
                }
                window.makeKeyAndOrderFront(nil)
                window.acceptsMouseMovedEvents = true
                let points = spec == "auto" ? automaticTargets(in: window) : explicitPoints
                for (name, point) in points {
                    // Approach from the left in small steps so enter/move tracking fires.
                    for step in stride(from: -24.0, through: 0, by: 4) {
                        let p = NSPoint(x: point.x + step, y: point.y)
                        let screen = window.convertPoint(toScreen: p)
                        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
                        CGWarpMouseCursorPosition(CGPoint(x: screen.x, y: primaryHeight - screen.y))
                        if let event = NSEvent.mouseEvent(with: .mouseMoved, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0) {
                            NSApplication.shared.sendEvent(event)
                        }
                        try? await Task.sleep(for: .milliseconds(40))
                    }
                    try? await Task.sleep(for: .milliseconds(250))
                    let kind = describe(NSCursor.current)
                    lines.append("\(name) \(Int(point.x)),\(Int(point.y)) -> \(kind)")
                }
                try? lines.joined(separator: "\n").write(toFile: output, atomically: true, encoding: .utf8)
            }
        }
    }

    /// Every enabled AppKit button in the window frame (title bar and toolbar included) plus
    /// every SwiftUI view using `pointingHandCursor`, as window coordinates. Also adds the
    /// standard window buttons, which must keep the arrow.
    static func automaticTargets(in window: NSWindow) -> [(String, NSPoint)] {
        var targets: [(String, NSPoint)] = []
        let standard = [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton].compactMap { window.standardWindowButton($0) }
        func walk(_ view: NSView) {
            if let button = view as? NSButton, button.isEnabled, !button.isHidden, button.window != nil, button.bounds.width > 4 {
                let frame = button.convert(button.bounds, to: nil)
                let kind = standard.contains { $0 === button } ? "window-button(expect arrow)" : "appkit"
                let title = button.title.isEmpty ? (button.toolTip ?? button.accessibilityLabel() ?? String(describing: type(of: button))) : button.title
                targets.append(("\(kind):\(title.replacingOccurrences(of: " ", with: "_"))", NSPoint(x: frame.midX, y: frame.midY)))
            }
            view.subviews.forEach(walk)
        }
        if let frameView = window.contentView?.superview { walk(frameView) }
        let height = window.contentView?.bounds.height ?? 0
        for frame in CursorTargetRegistry.shared.frames.values.sorted(by: { $0.minY < $1.minY }) {
            // SwiftUI global coordinates are top-left based within the content view.
            targets.append(("swiftui-row \(Int(frame.width))x\(Int(frame.height))", NSPoint(x: frame.midX, y: height - frame.midY)))
        }
        // Empty areas checked last: the hand must not stick after leaving targets.
        targets.append(("empty-sidebar(expect arrow)", NSPoint(x: 113, y: 300)))
        targets.append(("empty-content(expect arrow)", NSPoint(x: (window.contentView?.bounds.width ?? 800) - 40, y: 40)))
        return targets
    }

    /// Centers of every enabled accessibility button/link in the window, in window coordinates.
    static func accessibilityButtons(in window: NSWindow) -> [(String, NSPoint)] {
        var found: [(String, NSPoint)] = []
        var visited = 0
        func walk(_ element: Any) {
            visited += 1
            guard visited < 5000, let element = element as? NSAccessibilityProtocol else { return }
            let role = element.accessibilityRole()
            if role == .button || role == .link || role == .popUpButton {
                let frame = element.accessibilityFrame()
                let enabled = (element as? NSAccessibilityElementProtocol).map { _ in true } ?? true
                if frame.width > 0, frame.height > 0, enabled {
                    let screenCenter = NSPoint(x: frame.midX, y: frame.midY)
                    let local = window.convertPoint(fromScreen: screenCenter)
                    let content = window.contentView?.bounds ?? .zero
                    if content.contains(local) {
                        let label = element.accessibilityLabel() ?? element.accessibilityTitle() ?? "?"
                        found.append(("\(role!.rawValue.replacingOccurrences(of: "AX", with: "")):\(label.replacingOccurrences(of: " ", with: "_"))", local))
                    }
                }
            }
            for child in element.accessibilityChildren() ?? [] { walk(child) }
        }
        walk(window)
        return found
    }

    /// Identifies a cursor by its image and hot spot, since AppKit may hand out copies.
    static func describe(_ cursor: NSCursor) -> String {
        let known: [(String, NSCursor)] = [("pointingHand", .pointingHand), ("arrow", .arrow), ("iBeam", .iBeam), ("openHand", .openHand), ("resizeLeftRight", .resizeLeftRight)]
        for (name, candidate) in known where candidate == cursor { return name }
        let data = cursor.image.tiffRepresentation
        for (name, candidate) in known where candidate.hotSpot == cursor.hotSpot && candidate.image.tiffRepresentation == data {
            return name + " (copy)"
        }
        return "other(hotSpot=\(cursor.hotSpot), size=\(cursor.image.size))"
    }
}
#endif
