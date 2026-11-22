import AppKit
import ApplicationServices
// usage: dump <pid>
let pid = pid_t(CommandLine.arguments[1])!
let app = AXUIElementCreateApplication(pid)
for e in UI.elements(in: app) where ["AXButton", "AXRow", "AXCell", "AXTextField", "AXPopUpButton", "AXRadioButton", "AXCheckBox", "AXStaticText"].contains(e.role) && e.frame.width > 0 {
    print(e.role, "|", e.label.prefix(60), "|", Int(e.frame.midX), Int(e.frame.midY), Int(e.frame.width), "x", Int(e.frame.height), e.selected.map { "selected=\($0)" } ?? "")
}
