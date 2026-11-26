# ReleaseDock

Publishes WordPress.org plugins and themes to SVN safely. Two apps share one data folder:
- `App/` + `Packages/ReleaseDockKit`: the shipping macOS app (SwiftUI).
- `rust/` + `desktop/`: the cross-platform replacement (Rust core + Tauri 2 + React/TypeScript), in progress.

**Start here:** `docs/HANDOFF.md`. It describes the current state, the exact point where work stopped, how to set up a machine, daily commands, and next steps. The plan is in `docs/plans/2026-11-26-layout-v2-and-cross-platform.md`, and the porting rules are in `rust/PORTING.md`.

Working rules:
- Commit directly on `main`. Commit or push only when asked.
- Release the Swift app only through the GitHub Actions `release.yml` workflow, with an increasing build number and a CHANGELOG section. Never publish from a local machine.
- UI changes start as an HTML preview in `design-preview/`, which the user approves before anything is built. The approved design is `design-preview/layout-v2.html`.
- Keep behaviour identical to the Swift app: messages are copied verbatim, and release safety rules never change.
- Rust: tests first, no `unwrap` in library code, no `unsafe`, fmt width 120, clippy clean. CI runs on macOS, Windows and Linux.
- `npm run tauri dev` uses port 1430 and needs stdin kept open when it runs without a terminal (`tail -f /dev/null | …`).
- On macOS, buttons and controls keep the arrow cursor; only links to web pages get the pointing hand.
