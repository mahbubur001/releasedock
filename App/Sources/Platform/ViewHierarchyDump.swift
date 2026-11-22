#if DEBUG
import AppKit

/// Debug-only diagnostics: RELEASEDOCK_UI_DUMP_VIEWS=/path writes the AppKit view tree of
/// every window (class, frame in window coordinates, enabled state) a few seconds after launch.
@MainActor
enum ViewHierarchyDump {
    static func scheduleIfRequested() {
        guard let path = ProcessInfo.processInfo.environment["RELEASEDOCK_UI_DUMP_VIEWS"] else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
            var lines: [String] = []
            for window in NSApplication.shared.windows {
                lines.append("WINDOW \(type(of: window)) \(window.frame) visible=\(window.isVisible)")
                guard let content = window.contentView else { continue }
                func walk(_ view: NSView, depth: Int) {
                    let frame = view.convert(view.bounds, to: nil)
                    var line = String(repeating: " ", count: depth) + String(describing: type(of: view)) + " \(Int(frame.minX)),\(Int(frame.minY)) \(Int(frame.width))x\(Int(frame.height))"
                    if let control = view as? NSControl { line += " enabled=\(control.isEnabled)" }
                    lines.append(line)
                    for sub in view.subviews { walk(sub, depth: depth + 1) }
                }
                walk(content, depth: 1)
            }
            try? lines.joined(separator: "\n").write(toFile: path, atomically: true, encoding: .utf8)
        }
    }
}
#endif
