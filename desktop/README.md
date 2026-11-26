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

## End-to-end checks

`scripts/make-e2e.sh <new folder>` builds a local `file://` repository with Sample Plugin 1.0.0, the
1.1.0 folder, and a data folder whose only product points at it (via `cargo run -p rd-store
--example seed_product`). Debug builds read these environment variables on launch:

| Variable | Effect |
|---|---|
| `RELEASEDOCK_DATA_DIR` | Use this data folder instead of the shared one |
| `RELEASEDOCK_UI_TEST_RELEASE="<id>\|<source>\|<mode>"` | Open New Release and prepare that package |
| `RELEASEDOCK_UI_TEST_PUBLISH=1` | Also publish (only for `file://` repositories) |
| `RELEASEDOCK_UI_TEST_VIEW="<id>\|<tab>\|<0/1>"` | Open a product tab, with the transcript drawer if `1` |

See `../docs/HANDOFF.md` for the current state and next steps.
