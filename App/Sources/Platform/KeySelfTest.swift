#if DEBUG
import AppKit

/// Debug-only: RELEASEDOCK_UI_KEY_TEST=/path clicks at RELEASEDOCK_UI_KEY_CLICK="x,y" (window
/// coordinates) and then sends ↓, ↓, ↑ through the normal event path, recording the
/// Release History selection index after each key.
@MainActor
enum KeySelfTest {
    static func scheduleIfRequested(model: AppModel) {
        let env = ProcessInfo.processInfo.environment
        guard let output = env["RELEASEDOCK_UI_KEY_TEST"], let click = env["RELEASEDOCK_UI_KEY_CLICK"] else { return }
        let xy = click.split(separator: ",").compactMap { Double($0) }
        guard xy.count == 2 else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
            Task { @MainActor in
                guard let window = NSApplication.shared.windows.first(where: { $0.contentView != nil && $0.frame.width > 600 }) else {
                    try? "no window: \(NSApplication.shared.windows.map { "\(type(of: $0)) \($0.frame) visible=\($0.isVisible)" })".write(toFile: output, atomically: true, encoding: .utf8)
                    return
                }
                window.makeKeyAndOrderFront(nil)
                NSApplication.shared.activate()
                let point = NSPoint(x: xy[0], y: xy[1])
                var lines: [String] = []
                @MainActor func index() -> String {
                    let ids = model.releases.map(\.id)
                    return model.historySelection.flatMap { ids.firstIndex(of: $0) }.map(String.init) ?? "none"
                }
                for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                    if let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1) {
                        NSApplication.shared.sendEvent(event)
                    }
                }
                try? await Task.sleep(for: .milliseconds(400))
                let seen = KeyCounter()
                let probe = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in seen.count += 1; return event }
                lines.append("active=\(NSApplication.shared.isActive) key=\(window.isKeyWindow) keyWindow=\(NSApplication.shared.keyWindow.map { String(describing: type(of: $0)) } ?? "nil") firstResponder=\(window.firstResponder.map { String(describing: type(of: $0)) } ?? "nil")")
                lines.append("after click: \(index())")
                for (name, code, char) in [("down", UInt16(125), "\u{F701}"), ("down", 125, "\u{F701}"), ("up", 126, "\u{F700}")] {
                    _ = char
                    // Real keyboard events posted to this process go through the full event path.
                    for down in [true, false] {
                        CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(code), keyDown: down)?.postToPid(getpid())
                    }
                    try? await Task.sleep(for: .milliseconds(300))
                    lines.append("after \(name): \(index())")
                }
                lines.append("releases: \(model.releases.count) keyDowns seen by a monitor: \(seen.count)")
                if let probe { NSEvent.removeMonitor(probe) }
                try? lines.joined(separator: "\n").write(toFile: output, atomically: true, encoding: .utf8)
            }
        }
    }
}
@MainActor final class KeyCounter { var count = 0 }
#endif
