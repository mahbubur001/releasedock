# Real-input UI tests

Drives a Debug build of ReleaseDock with real mouse and keyboard events (CGEvent) and reads the
interface through the Accessibility API. Requires Accessibility permission for the app that
runs the script (System Settings → Privacy & Security → Device Control and Data Access).

```bash
UITests/run.sh            # launches an isolated demo instance and runs the checks
```

**Safety:** every click, move and key press first checks that ReleaseDock is the frontmost app
and owns the topmost window under the pointer; otherwise the run stops without sending input.
Still, don't use the Mac while the tests run (about 30 seconds).

Checks: sidebar navigation, ↑/↓ in Release History and Products, arrow cursor (macOS convention) over
sidebar items, list rows, detail buttons and the sidebar toggle, arrow over empty areas, and
hiding/showing the sidebar.
