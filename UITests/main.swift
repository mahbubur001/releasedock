import AppKit
import ApplicationServices
// usage: ui-tests <pid>   — drives the given ReleaseDock instance with real input.
let pid = pid_t(CommandLine.arguments[1])!
let app = AXUIElementCreateApplication(pid)
Safety.targetPID = pid
var failures = 0
func check(_ name: String, _ ok: Bool, _ detail: String = "") {
    print(ok ? "PASS" : "FAIL", name, detail.isEmpty ? "" : "— \(detail)")
    if !ok { failures += 1 }
}
func activate() {
    NSRunningApplication(processIdentifier: pid)?.activate()
    usleep(600_000)
}
func all() -> [Element] { UI.elements(in: app) }
func sidebar(_ title: String) -> Element? {
    all().first { $0.role == "AXButton" && ($0.label == title || $0.label.hasPrefix(title + ",")) && $0.frame.width > 150 && $0.frame.width < 260 }
}
func rows(prefix: String, minX: CGFloat) -> [Element] {
    all().filter { $0.role == "AXButton" && $0.label.hasPrefix(prefix) && $0.frame.minX > minX && $0.frame.height > 40 }
        .sorted { $0.frame.minY < $1.frame.minY }
}
func selectedIndex(_ list: [Element]) -> Int? {
    list.firstIndex { $0.selected == true }
}

activate()

// Arrow keys in both list columns.
for (section, prefix) in [("History", "Acme Gallery"), ("Products", "Acme")] {
    guard let item = sidebar(section) else { check("\(section): sidebar item found", false); continue }
    UI.click(CGPoint(x: item.frame.midX, y: item.frame.midY))
    usleep(800_000)
    check("\(section): sidebar click selects it", sidebar(section)?.selected == true)
    let sidebarRight = item.frame.maxX
    var list = rows(prefix: prefix, minX: sidebarRight)
    guard list.count >= 2 else { check("\(section): at least two rows", false, "found \(list.count)"); continue }
    UI.click(CGPoint(x: list[0].frame.midX, y: list[0].frame.midY))
    list = rows(prefix: prefix, minX: sidebarRight)
    check("\(section): click selects row 1", selectedIndex(list) == 0, "selected=\(String(describing: selectedIndex(list)))")
    UI.key(125) // ↓
    list = rows(prefix: prefix, minX: sidebarRight)
    check("\(section): ↓ selects row 2", selectedIndex(list) == 1, "selected=\(String(describing: selectedIndex(list)))")
    UI.key(125) // ↓ at the end stays
    list = rows(prefix: prefix, minX: sidebarRight)
    check("\(section): ↓ at the end stays on the last row", selectedIndex(list) == list.count - 1)
    UI.key(126) // ↑
    list = rows(prefix: prefix, minX: sidebarRight)
    check("\(section): ↑ selects row 1 again", selectedIndex(list) == 0, "selected=\(String(describing: selectedIndex(list)))")
}

// Cursor over real controls (real mouse moves): macOS keeps the arrow on buttons and rows.
func hover(_ element: Element?, _ name: String, expect: String) {
    guard let element else { check("cursor: \(name) found", false); return }
    let point = CGPoint(x: element.frame.midX, y: element.frame.midY)
    UI.move(to: CGPoint(x: point.x - 30, y: point.y)); usleep(150_000)
    UI.move(to: point); usleep(450_000)
    let cursor = UI.cursorName()
    check("cursor over \(name) is \(expect)", cursor == expect, "got \(cursor)")
}
hover(sidebar("Overview"), "sidebar item", expect: "arrow")
if let products = sidebar("Products") { UI.click(CGPoint(x: products.frame.midX, y: products.frame.midY)); usleep(800_000) }
let productRow = rows(prefix: "Acme", minX: sidebar("Products")?.frame.maxX ?? 0).first
hover(productRow, "product row", expect: "arrow")
hover(all().first { $0.role == "AXButton" && $0.label.contains("New Release") && $0.frame.minX > (sidebar("Products")?.frame.maxX ?? 0) }, "New Release button", expect: "arrow")
hover(all().first { $0.role == "AXButton" && $0.label == "Edit" }, "Edit button", expect: "arrow")
hover(all().first { $0.role == "AXButton" && $0.label == "Hide Sidebar" }, "toolbar sidebar toggle", expect: "arrow")

// Sidebar toggle hides and shows the sidebar, and stays reachable while hidden.
if let toggle = all().first(where: { $0.role == "AXButton" && $0.label == "Hide Sidebar" }) {
    UI.click(CGPoint(x: toggle.frame.midX, y: toggle.frame.midY)); usleep(900_000)
    check("sidebar hides", sidebar("Overview") == nil || (sidebar("Overview")?.frame.width ?? 0) == 0)
    if let show = all().first(where: { $0.role == "AXButton" && $0.label == "Show Sidebar" }) {
        UI.click(CGPoint(x: show.frame.midX, y: show.frame.midY)); usleep(900_000)
        check("sidebar shows again", sidebar("Overview") != nil)
    } else {
        check("Show Sidebar button available while hidden", false)
    }
} else {
    check("sidebar toggle found", false)
}
if let window = (UI.attribute(app, kAXWindowsAttribute) as? [AXUIElement])?.first {
    let frame = UI.frame(window)
    let empty = Element(ref: window, role: "area", label: "empty", frame: CGRect(x: frame.maxX - 80, y: frame.maxY - 80, width: 10, height: 10))
    hover(empty, "empty content area", expect: "arrow")
}

print(failures == 0 ? "ALL PASSED" : "\(failures) FAILURE(S)")
exit(failures == 0 ? 0 : 1)
