# Implementation Plan

Status legend: ✅ done and verified · 🟡 done, verification limited · ⬜ not done

## Phase 0 — Research and architecture ✅

- Repository inspected (empty at start). Toolchain: Xcode 27.2, Swift 6.4, svn 1.14.5 (Homebrew), no signing identity.
- Research with sources: [WordPress SVN](docs/research/WORDPRESS_SVN_RESEARCH.md), [Sparkle, GitHub API, macOS security](docs/research/SPARKLE_GITHUB_SECURITY_RESEARCH.md).
- Deployment target: **macOS 14**. All APIs used (Observation, SwiftUI `Table`/`ContentUnavailableView`, `@Observable`) are available there; Sparkle 2.10 requires macOS 12+.
- Decisions: XcodeGen project + local Swift package for all logic; SQLite via the system library (no third-party DB); own ZIP reader (security control, no dependency); non-sandboxed Developer ID app; login keychain; PHP `version_compare` semantics.

### Technical assumptions

1. WordPress.org accepts trunk changes and the new tag in one commit (de-facto practice; not documented as policy).
2. Plugin/theme pre-commit hook rules and size limits are undocumented; ReleaseDock surfaces hook output.
3. Whether Release Confirmation is enabled cannot be detected; guidance is always shown.
4. Theme updates via SVN apply only to themes that already have a live version.
5. The SVN realm is `<https://plugins.svn.wordpress.org:443> Use your WordPress.org login` (derived from the observed `WWW-Authenticate` header and serf's realm format; not tested with a real login).
6. WordPress.org SVN reads are anonymous, so credentials are validated only at commit time.

## Phase 1 — Foundation and product management ✅

SwiftUI app shell (Dashboard, Products, Release Wizard, Release History, GitHub Updates, Settings), `AppModel` composition root, SQLite store with migrations, product CRUD with validation of slug, URLs (https, no credentials, WordPress.org host/slug match), paths (absolute, no nesting with the dev folder, not in staging), main file, GitHub URL; product detection from a development folder.

## Phase 2 — ZIP and folder inspection ✅

Own ZIP reader (ZIP64, stored/deflate, CRC, overlap and header cross-checks), inspection without extraction, safe extraction, folder inspection, file classification, `.distignore` and user patterns, SHA-256. Fixed during testing: containment check did not resolve symlinks for not-yet-existing paths.

## Phase 3 — Version detection and validation ✅

`version_compare()` port (differential-tested against PHP), version validity rules, `get_file_data()` header parsing (verified against PHP), WordPress.org readme parser rules, product detection, 19 validation checks (14 required), automation gate.

## Phase 4 — SVN integration ✅

`ProcessRunner` (fixed a hang: `waitUntilExit` on dispatch threads), installation detection, info/list/cat/checkout/update/status/diff/add/delete/copy/revert/cleanup/propset/commit/log/export, `svn auth` cache detection, `--password-from-stdin`, error-code classification, conservative commit-outcome classification, `@` peg escaping, `--no-ignore`.

## Phase 5 — Plugin and theme strategies ✅

`PluginReleaseStrategy` (sparse WC, trunk mirror, assets + MIME props, tag copy, verification incl. Stable tag) and `ThemeReleaseStrategy` (version-directory copy with history, strict Theme Check-oriented validation).

## Phase 6 — Pipeline and recovery ✅

Persisted state machine, staging with hash re-verification, preview fingerprint, approval binding, pre-commit re-checks, uncertain-commit recovery via release ID, restart reconciliation, cancel/abandon with working-copy restore and backups, manifests, notifications, diagnostics export. Fixed during testing: manifest path overwritten by a later save.

## Phase 7 — GitHub notifications and Sparkle 🟡

GitHub release checks (stable/beta, ETag cache, rate limits, private-repo token), notify-once, release notes UI; Sparkle 2.10 integration with channels, refusing to start without feed + EdDSA key. **Not verified end to end**: requires a signed build, an EdDSA key pair and a hosted appcast.

## Phase 8 — Testing, signing, notarization, distribution 🟡

177 tests (unit + real-svn integration) pass as of 2026-11-24; app builds without warnings; UI verified manually through *Previewed* and restart reconciliation against a local repository. `scripts/release.sh` implements archive → Developer ID export → notarize → staple → zip → Sparkle appcast. **Not executed**: no Developer ID certificate or notarization credentials were available.

## Remaining work

| Item | Priority | Notes |
|---|---|---|
| First real release to WordPress.org with a test plugin | High | Validate hook messages, realm/credential caching and Release Confirmation guidance against production; use a dry run first. |
| Developer ID signing + notarization run, Sparkle EdDSA keys, hosted appcast, end-to-end update | High | Follow RELEASE_WORKFLOW.md Part 2. |
| XCUITest target for the main flows | Medium | Needs Accessibility permission for the test runner; Debug-only launch hooks (TESTING.md) are used for screenshot and cursor verification meanwhile. |
| Large-package SVN performance | Low | Inspection/extraction/staging measured at 5,000 files: 0.45 s / 1.2 s / 2.3 s (`RELEASEDOCK_PERF_TESTS=1`). `svn status`/commit at that scale not measured. |
| Localisation | Low | English only. |
