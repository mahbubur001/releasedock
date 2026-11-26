# Handoff: where the work stands and how to continue

Last updated: 2026-11-26 (end of day). Read this first on a new computer. It is written so that
work can resume at exactly this point, with the same files and the same design.

## 1. Where things stand

| Area | State |
|---|---|
| **Swift macOS app** (`App/`, `Packages/ReleaseDockKit`) | Shipping. Latest release **0.3.0 (build 9)**, 2026-11-26. Stays the production app on Mac until the Tauri app reaches parity. |
| **Rust core** (`rust/`) | **Done** (Phase 3). 11 crates, about 375 tests, CI green on macOS, Windows and Linux (`.github/workflows/rust.yml`). Reads and writes the Swift app's database and files unchanged. |
| **Tauri app** (`desktop/`) | **Phase 4, in progress.** Shell, Overview, product page, History and **New Release (prepare → review → publish → verify)** work on real data. The product tabs, transcript drawer and working-copy actions are written but **not yet verified in the app** (see §5). |
| **Design** | Layout v2 approved: `design-preview/layout-v2.html` (open it in a browser). Its CSS is ported into `desktop/src/styles/app.css`. |
| **Plan** | `docs/plans/2026-11-26-layout-v2-and-cross-platform.md`. Phases 1–3 are done; Phase 4 is in progress. |
| **Research** | `docs/research/SVN_CLIENTS_RESEARCH.md` (Versions and SmartSVN study, P1–P3 feature list). |

### Decisions already made
- **Cross-platform stack:** Tauri 2, a Rust core and a React + TypeScript UI.
- **Styling:** plain CSS taken from the approved preview (no Tailwind).
- **UI sources:** the HTML previews in `design-preview/` are the source for the UI. New screens get a preview first, which the user approves before it is built.
- **Branching:** work directly on `main`, no feature branches.
- **Data compatibility:** the Tauri app uses the same data folder and the same JSON, SQLite schema and keychain items as the Swift app.

### Decisions still open
- Crate names: keep `rd-*` or rename to `releasedock-*`. This is cosmetic.
- Code signing for Phase 5: an Apple Developer ID for notarization, and optionally a Windows code-signing certificate.

## 2. Set up a new computer

Clone the repository, then install the following.

**macOS** (the full setup, including the Swift app)
```sh
xcode-select --install                 # plus Xcode 26 or later from the App Store for the Swift app
brew install subversion xcodegen gh node php
curl https://sh.rustup.rs -sSf | sh    # Rust stable 1.85 or later
gh auth login                          # needed for releases and CI
```

**Windows**
- Rust: install with `rustup` and the MSVC toolchain.
- Node.js 22 or later.
- Subversion command-line tools: install TortoiseSVN with "command line client tools" selected, or SlikSVN.
- The WebView2 runtime. Windows 11 already includes it.

**Linux (Debian/Ubuntu)**
```sh
sudo apt install subversion build-essential pkg-config libdbus-1-dev libwebkit2gtk-4.1-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```
Then install Rust with `rustup`, and Node.js 22 or later.

### Check the setup
```sh
cd rust && cargo test --workspace          # about 375 tests; svn integration tests are skipped if svn is missing
cd ../desktop && npm install && npx tsc --noEmit
cd src-tauri && cargo check
```

## 3. Daily commands

| What | Command |
|---|---|
| Rust core tests | `cd rust && cargo test --workspace` |
| Lint, as CI runs it | `cd rust && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Run the Tauri app on your real data | `cd desktop && npm run tauri dev` |
| Run it on a copy of the data | `RELEASEDOCK_DATA_DIR=/path/to/copy npm run tauri dev` |
| Swift app (macOS) | `xcodegen generate && open ReleaseDock.xcodeproj`, or `xcodebuild -scheme ReleaseDock -configuration Debug -derivedDataPath build/DerivedData build` |
| Swift core tests | `cd Packages/ReleaseDockKit && swift test --parallel` |

Notes for `npm run tauri dev`:
- The dev server uses **port 1430**. Port 1420 belongs to another local project on the first computer.
- When it runs without a terminal (for example from a script or an agent), keep stdin open: `tail -f /dev/null | npm run tauri dev`. Otherwise Vite exits and the window stays blank.
- The development CSP is off (`devCsp: null` in `tauri.conf.json`), because Vite injects inline scripts. Production builds keep the strict CSP.

### End-to-end check without touching real data
```sh
desktop/scripts/make-e2e.sh /tmp/rd-e2e        # prints the product id; needs svn, svnadmin, cargo
cd desktop
# Prepare and publish Sample Plugin 1.1.0 to the local file:// repository automatically:
tail -f /dev/null | RELEASEDOCK_DATA_DIR=/tmp/rd-e2e/data \
  RELEASEDOCK_UI_TEST_RELEASE="<id>|/tmp/rd-e2e/plugin-1.1.0|assisted" RELEASEDOCK_UI_TEST_PUBLISH=1 npm run tauri dev
# Open a product tab, with the transcript drawer if the last field is 1:
tail -f /dev/null | RELEASEDOCK_DATA_DIR=/tmp/rd-e2e/data RELEASEDOCK_UI_TEST_VIEW="<id>|activity|1" npm run tauri dev
```
- The `RELEASEDOCK_UI_TEST_*` hooks exist only in debug builds.
- They can publish only to `file://` repositories.

## 4. Releasing the Swift app (until Phase 6)
1. Write the version's entry in `CHANGELOG.md` (`## [x.y.z] - date` with `### Added/Changed/Fixed`). The update window and the GitHub release show it.
2. Commit and push to `main`.
3. Run the release workflow:
   `gh workflow run release.yml -R mahbubur001/releasedock --ref main -f version=X.Y.Z -f build=N -f notes="…"`
   - The build number must increase. The last release was 0.3.0, build 9.
   - Sparkle signs with the `SPARKLE_PRIVATE_KEY` repository secret. Never publish from a local machine.
4. After the workflow pushes the feed commit, run `git pull`.

## 5. Exactly where work stopped

**Last finished step:** New Release in the Tauri app. In the commit "New Release in the Tauri app…", a sample plugin was prepared, reviewed, published to a local repository and verified.

**In progress (committed, not yet verified):**
- **Product tabs:** `desktop/src/screens/ProductTabs.tsx`, with backend `desktop/src-tauri/src/product_tabs.rs`.
  - **Activity:** svn log for the product only, grouped by day, with A/M/D chips. To support the chips, `rd-svn` now parses each path's action (`changed_path_actions`).
  - **Releases:** `tags/`, newest first by PHP `version_compare`, with the Stable tag from `trunk/readme.txt`.
  - **Files:** working-copy state, with Update, Revert (backed up first) and conflict resolution.
- **Transcript drawer:** `desktop/src/components/Transcript.tsx`.
- **Error boundary:** `desktop/src/components/ErrorBoundary.tsx`, so a crashing screen shows its error instead of a blank window.
- `WorkingCopyState` and `WorkingCopyChange` in `rd-records` are now `Serialize`.

**Open issue to debug first.** After these changes, launching the dev app with `RELEASEDOCK_UI_TEST_VIEW="<id>|activity|1"` showed a **blank window**. The same page renders in a normal browser, and Vite and the Rust build report no errors. The planned next step was interrupted. It was:
1. Forward UI errors to the app log. Add a `log_ui_error(message)` command that prints to stderr. In `main.tsx`, report `window.onerror` and `unhandledrejection`, and send one "UI started" report.
2. Relaunch with the hook and read the log. Either the page didn't load (no "UI started" in the log), or the error will be there.
3. Possible causes to check:
   - timing between the view hook and `setTab`;
   - a value missing from the transcript or activity data;
   - the window being shown before Vite was ready.

   Also check whether a plain launch (no hook) renders, as it did before these changes.

**Small known glitch:** the toolbar sometimes read "Refreshed 5 minutes ago" right after a refresh. The `ago()` helper is correct (checked in Node), so the cause is elsewhere and still unknown.

## 6. Next steps (Phase 4, in order)
1. Fix the blank-window issue (§5), then verify Activity, Releases, Files and Transcript with the e2e setup, and on a copy of real data (read-only svn log against WordPress.org is fine).
2. Settings window: tabs General, Subversion, Checks, Accounts, Updates, About, matching the Swift app's 0.2.3 Settings. Changes apply immediately. Settings are stored under key `app.settings` in the shared database.
3. Add / Edit Product: port of `ProductService` validation, already in `rd-store`.
4. Restore Older Version, Version Bump, and Readme & Assets Update. The pipeline supports all three.
5. App updates via `tauri-plugin-updater`, with a static `latest.json` on GitHub, and a toolbar "Update found" button like the Swift app.
6. Notifications, for "release is live" and "approval needed".
7. Then Phase 5: signed installers for macOS, Windows and Linux, and CI builds. Then Phase 6: migration (see the plan).

## 7. Conventions to keep
- **Behaviour parity with Swift:** user-facing messages are copied verbatim, and release safety rules never change. These are: one commit per release, approval bound to the preview fingerprint, release-ID marker plus log lookup instead of retries, backup before revert, and SHA-256 verification.
- **Rust:** port tests first; no `unwrap` in library code; no `unsafe`; `cargo fmt` with width 120; clippy with no warnings. See `rust/PORTING.md`.
- **macOS cursor:** the arrow on buttons and controls; the pointing hand only on links to web pages.
- **Design changes:** make an HTML preview in `design-preview/` first and get approval. Don't use an artifact or document tool for designs.
- **Commit messages:** an imperative summary, then a short body explaining why.
