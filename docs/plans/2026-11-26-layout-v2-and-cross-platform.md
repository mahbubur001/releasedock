# Plan: Layout v2, P1 features and a cross-platform ReleaseDock

Date: 2026-11-26. Based on [SVN_CLIENTS_RESEARCH.md](../research/SVN_CLIENTS_RESEARCH.md) §5–6.

Status legend: ⬜ not started · 🟡 in progress · ✅ done

---

## 1. Where we are

- **Shipping app:** ReleaseDock 0.2.3 for macOS (SwiftUI). The core is a Swift package of about 9,100 lines with about 4,000 lines of tests (219 tests). The UI is about 6,000 lines.
- **Research outcome:**
  - There are **nine P1 features** worth building (research §5).
  - There is a **clear layout direction**: the Versions-style sidebar and timeline, plus SmartSVN's depth where it helps releasing.
  - Out of scope: merge, branches, locks and changelists.
- **New requirement:** ReleaseDock should run on **macOS, Windows and Linux**.

**Answer to "do we have features and a new layout?" Yes.** The research gives a concrete P1 list and a layout model. Most P1 items are new screens (activity feed, releases browser, transcript, image compare), so they belong in the new layout rather than being bolted onto the current one.

---

## 2. Cross-platform recommendation

### Recommendation: Tauri 2, with a Rust core and a TypeScript + React UI

**Why it fits ReleaseDock**
- **The UI is HTML and CSS.** The team already works in web technologies (WordPress, PHP, JS), and the approved HTML previews in `design-preview/` turn directly into the real UI instead of being redrawn in SwiftUI.
- **Small, native-feeling apps.** Tauri uses the system webview: WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux. Bundles are about 10–20 MB, against 100 MB or more for Electron.
- **A signed auto-updater like Sparkle is built in.** `tauri-plugin-updater` verifies a signature against a public key embedded in the app and reads a static JSON manifest that can live on GitHub, as today's appcast does (Tauri docs, plugin/updater). One GitHub Actions matrix builds all three platforms.
- **The Rust core suits what ReleaseDock does.**
  - It runs `svn` as a child process with arguments and stdin and no shell (`std::process` / `tokio::process`).
  - It reads ZIP files safely (the `zip` crate, or a port of our own reader).
  - It uses SQLite (`rusqlite`) and SHA-256 (`sha2`).
  - It stores passwords in the OS credential store: the `keyring` crate covers macOS Keychain, Windows Credential Manager and Linux Secret Service.
- **Security model.** Tauri's capability system limits which commands the UI can call. All SVN, file and credential work stays in Rust, which is the same boundary the Swift app has between `ReleaseDockCore` and its views.

**Trade-offs to accept**
- **Webview differences.** Each platform draws with a different engine (WebKit, Chromium-based WebView2, WebKitGTK), so the UI must be tested on all three. WebKitGTK on Linux is the weakest.
- **The macOS app will feel slightly less native** than SwiftUI. Window chrome, menus and the traffic lights stay native, but controls are web-drawn.
- **The core must be ported from Swift to Rust.** That is about 9,100 lines plus the tests, which are the safety net and must be ported first.

### Alternatives considered

| Option | Verdict |
|---|---|
| **Electron** (TypeScript everywhere) | Most consistent rendering and a huge ecosystem, but 100 MB+ apps, high memory use, and Node in the privileged process. Second choice. |
| **Flutter** (Dart) | One renderer everywhere, but it doesn't use the web skills we have, and desktop polish (menus, accessibility, text) lags. |
| **.NET + Avalonia** (C#) | Good desktop toolkit, but a new language and ecosystem for the team. |
| **Qt** (C++ or Python) | Mature and native-ish, but heavy licensing and tooling, and slow UI iteration. |
| **Keep the Swift core, add a Tauri UI with Swift as a sidecar** | Reuses tested code, but Swift on Windows and Linux lacks the Keychain, CryptoKit and Sparkle equivalents. Two languages and two toolchains in one product is fragile. |
| **SwiftUI on Mac + separate native apps elsewhere** | Three UIs to maintain. Rejected. |

### Migration approach

1. **Keep shipping the SwiftUI app for macOS until the Tauri app reaches parity.** Only fixes and small P1 items go into it.
2. **Port the core to Rust test-first.** Translate the 219 tests, then make them pass, module by module (versioning → inspection → validation → SVN → pipeline → recovery).
3. **Build Layout v2 directly in Tauri** from the approved HTML preview.
4. **Beta on all three platforms.** Then the macOS Tauri build replaces the SwiftUI build through a final Sparkle update that installs the Tauri app. Data migrates from the same SQLite schema.

**Decision (2026-11-26):** Tauri 2 confirmed. Phases 3–6 below build on it.

---

## 3. Phases

### Phase 1: Layout v2 preview (HTML) ✅
Platform-neutral, so it is useful whichever stack is chosen.
- ✅ Sidebar:
  - product rows with the WordPress.org icon, name and version;
  - ↑ local / ↓ remote count pills;
  - a problem state (unreachable, login failed, conflicts);
  - groups;
  - bottom bar with **+** and the Settings gear.
- ✅ Toolbar title with a freshness subtitle ("Refreshed 3 minutes ago · r3822482"), and a **Refresh required** state.
- ✅ Per-product tabs: **Overview · Activity · Releases · Files**.
- ✅ **Activity**: a day-grouped timeline with a rail and dots, "r123 by user · time", the message, file chips (A/M/D) and release tag chips, unread highlighting, Mark All Read, and Load More / End of history.
- ✅ **Releases**: every tag with date, revision and author; the **Stable tag** highlighted; actions Compare with previous / with trunk and Restore.
- ✅ **Files**: the trunk/tags/assets tree with local and remote status, an All / Changed toggle, a name filter and an "N files hidden" count.
- ✅ **Transcript drawer**: the `svn` commands of the last operation, with exit codes and durations.
- ✅ **Release review**: a step list, an inline diff, and a **side-by-side asset image compare**.
- ✅ Designed empty and error states.
- ✅ Dark and light themes, with system colours for status.

**Exit:** preview approved on 2026-11-26 (`design-preview/layout-v2.html`, kept out of git because it holds copied plugin images).

### Phase 2: Small P1 items in the current macOS app ✅
Low effort, immediately useful, and specified once so they port easily:
- ✅ SVN transcript, recorded in the core per release and per product (P1 #5).
- ✅ Release message templates, with earlier ones per product (P1 #7).
- ✅ Remember typed text in the bump and restore sheets after Cancel or a failure (P1 #9).
- ✅ Designed error states with a fix for working copy and Subversion problems (P1 #8).
- ✅ Release 0.3.0.

### Phase 3: Rust core (Tauri) ✅
- ✅ Workspace `rust/` with 11 crates (`rd-support`, `rd-model`, `rd-versioning`, `rd-inspection`, `rd-svn`, `rd-keychain`, `rd-records`, `rd-store`, `rd-validation`, `rd-integrations`, `rd-pipeline`); Tauri app in `desktop/`.
- ✅ Port the tests first, then the modules (373 Rust tests):
  - `PHPVersionCompare`, WordPress headers, readme parser;
  - ZIP and folder inspection;
  - validation rules;
  - the SVN service (process runner, XML parsing, stdin credentials);
  - staging, the pipeline and its state machine, manifests, recovery;
  - SQLite store and migrations.
- ✅ Credentials through `keyring`; `svn` discovery per platform (Homebrew/MacPorts, SlikSVN/TortoiseSVN CLI/`svn.exe` on PATH, distro packages).
- ✅ CI: `cargo test` on macOS, Windows and Linux runners (`.github/workflows/rust.yml`).

### Phase 4: Tauri app with Layout v2 ⬜
- ⬜ Shell: sidebar, toolbar, Settings window, update window (Tauri updater with a static `latest.json` on GitHub).
- ⬜ Screens in order: Overview → Products → New Release → History → Activity → Releases → Files → Transcript → asset compare.
- ⬜ Activity and Releases backed by a **per-plugin-path log cache** (never the whole shared repository).
- ⬜ Accessibility: keyboard navigation, text status labels besides colour, and the macOS cursor convention.

### Phase 5: Packaging and release ⬜
- ⬜ GitHub Actions matrix builds `.app`/`.dmg` (macOS, universal), `.msi`/NSIS (Windows) and AppImage/`.deb` (Linux), each signed for the updater.
- ⬜ Code signing:
  - macOS: Developer ID + notarization, when an account is available;
  - Windows: Authenticode, optional at first (SmartScreen warns without it).
- ⬜ Updater manifest generated from the CHANGELOG section, as today.

### Phase 6: Migration and parity ⬜
- ⬜ Import the existing SQLite data (products, history, settings); Keychain items stay where they are.
- ⬜ Parity checklist against 0.3.x, then a final Sparkle update moves macOS users to the Tauri build.

### Later (P2/P3 from the research)
Inline next-action capsules, an incoming-changes preview, log search, a Review mode with syntax colouring, build-output rename detection, a credentials manager, repair actions, a Finder/Explorer integration, blame.

---

## 4. Rough timeline (one developer with Claude)

| Phase | Estimate |
|---|---|
| 1. Preview | 1 day |
| 2. Small P1 items in the macOS app | 3–5 days |
| 3. Rust core port | 3–5 weeks |
| 4. Tauri app with Layout v2 | 3–4 weeks |
| 5. Packaging on three platforms | 1 week |
| 6. Migration and parity | 1 week |

## 5. Risks

- **Port regressions in release logic.** Mitigation: port the tests first, and run old and new apps on the same dry runs before the switch.
- **svn on Windows.** Mitigation: detect it, guide the user (`winget install TortoiseSVN` with command-line tools, or SlikSVN), and set path handling and CRLF rules explicitly.
- **WebKitGTK rendering on Linux.** Mitigation: test early and use only standard CSS.
- **Credential stores differ.** Linux needs a Secret Service (GNOME Keyring or KWallet). Show a clear message when none is running.
