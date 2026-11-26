# ReleaseDock desktop (Tauri)

The cross-platform ReleaseDock app for macOS, Windows and Linux: a Tauri 2 shell with a
React + TypeScript UI (`src/`) on top of the Rust core in `../rust` (see
`../docs/plans/2026-11-26-layout-v2-and-cross-platform.md`). The UI follows the approved
Layout v2 design (`../design-preview/layout-v2.html`, kept out of git).

```sh
npm install
npm run tauri dev          # uses the same data folder as the Swift app
RELEASEDOCK_DATA_DIR=/tmp/rd-data npm run tauri dev   # or an isolated copy
npx tsc --noEmit           # type-check the UI
```

The dev server runs on port 1430. `src-tauri/src/commands.rs` holds the commands the UI
calls; all release logic stays in the `rd-*` crates.
