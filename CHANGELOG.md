# Changelog

All notable changes to ReleaseDock are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Readme & Assets Update for plugins: update `readme.txt` (and `/assets`) of the released version in trunk and its tag without publishing a new version, e.g. to raise "Tested up to". The package must be the released version; any code difference from the released tag is refused. Dry run, preview, approval, single commit, verification and uncertain-commit recovery work as for version releases.
- "Tested up to" is compared with the current WordPress release from api.wordpress.org and warns when it is behind, as WordPress.org's Plugin Check does. A plugin header "Tested up to" that differs from the readme also warns. Without a connection, only the format is checked.
- After a release is committed, ReleaseDock follows the WordPress.org directory until it offers the new version (every 10 minutes for 3 days, or **Check Now** in the report) and notifies you when it is live. The report shows Waiting, Live, Not listed or a newer version, and the guidance mentions the security review period WordPress.org applies before distributing releases.
- Code checks on the staged release candidate, before the SVN working copy is touched: PHP syntax (`php -l`, blocks on parse errors) and, with a local WordPress install set in Settings, WordPress.org's Plugin Check (`wp plugin check`, reported as warnings). Both are optional checks that can be turned off per product; lint is skipped when the local PHP is older than the plugin's Requires PHP.
- Deeper readme checks, matching the WordPress.org directory parser: short description over 150 characters, missing short description (including one swallowed as a header because it contains ":" directly under the headers), missing Description section, sections longer than WordPress.org keeps (2,500 words; 5,000 for Changelog and FAQ), readmes over 10 KB, theme upgrade notices over 300 characters, and `Requires Plugins` entries that are not directory slugs.
- Directory assets check (plugins with an assets folder): names, lowercase, pixel dimensions matching the name, size limits (banners 4 MB, icons 1 MB, screenshots 10 MB), a 1544×500 banner without its 772×250 partner, `icon.svg` without a PNG/JPG fallback, and a `blueprints/blueprint.json` that Live Preview cannot read. All warnings; the check can be turned off per product.
- Theme review checks: screenshot larger than 1200×900 or not 4:3, theme names using "WordPress", "Theme" or "Twenty", "Wordpress" misspelled in public text, and bundled images or fonts without a readme `== Resources ==` section.
- The release report shows "What's new": the changelog entry and upgrade notice for the version being released.
- Build command per product (e.g. `composer install --no-dev && npm run build`). For folder releases it runs in a login shell on an APFS clone of the folder, and the built copy is what gets inspected and published; the development folder is never modified. The output appears in the report, and a failing build stops the release. ZIP releases are used as they are.
- Folder releases honour `export-ignore` in `.gitattributes` when there is no `.distignore`, like `git archive` and 10up's deploy action.

### Changed
- Commits use their own, longer SVN HTTP timeout (30 minutes by default, adjustable in Settings) while reads keep failing fast after 2 minutes. Large releases no longer hit E175012 while WordPress.org finishes "Committing transaction…".

### Fixed
- readme headers are read like WordPress.org: `*`/`-` around keys and `<>` around values are ignored, and an unknown "header" after a blank line starts the short description.

## [0.1.3] - 2026-11-23

### Changed
- The source repository is now public, and releases and the update feed are published from it (`mahbubur001/releasedock`). The separate `releasedock-releases` repository has been retired; a saved update repository pointing to it is switched over automatically.
- Updates are signed with a new Sparkle EdDSA key. 0.1.0–0.1.2 can't update to 0.1.3 in-app; download it once from GitHub, and later updates install in-app again.
- The sidebar toggle is ReleaseDock's own button and shows a pointing-hand cursor.

## [0.1.2] - 2026-11-22

### Fixed
- GitHub Updates explains update verification accurately (EdDSA signature; Developer ID check only for Developer ID builds).

## [0.1.1] - 2026-11-22

### Added
- Arrow keys move through the Products and Release History lists.

### Changed
- "Use existing working copy" shows the local details immediately and refreshes from the server in the background; repository reads run in parallel.

### Fixed
- Plugin readme files are found regardless of case (e.g. `README.txt`), as on WordPress.org.

## [0.1.0] - 2026-11-22

First public build.

### Added
- Native SwiftUI app: Dashboard, Products, Release Wizard (9 steps), Release History, GitHub Updates, Settings; open ZIPs/folders from Finder or by drag and drop.
- Product management for WordPress.org plugins and themes with validation of slugs, URLs and paths, and detection from a development folder.
- Secure ZIP and folder inspection with automatic exclusion of OS junk, VCS metadata and development artifacts, `.distignore` support and SHA-256.
- Version engine identical to PHP `version_compare()`; header parsing identical to WordPress `get_file_data()`; WordPress.org readme parsing.
- 19 validation checks with severity, file path and suggested fix; per-product configuration.
- Subversion integration via the CLI with secure credential passing, sparse working copies and conservative commit-outcome handling.
- Plugin (trunk + tag, assets) and theme (version directory) release strategies with server-side verification.
- Persisted release state machine with preview fingerprints, approval binding, crash recovery and manifests.
- Dry Run, Assisted and Automatic release modes with an explicit automation gate.
- Keychain storage for SVN passwords and an optional GitHub token.
- GitHub release notifications and Sparkle 2 self-updates (stable and beta channels).
- Structured logging and diagnostics export.
- Release script for Developer ID signing, notarization and appcast generation.
- App icon (rendered by `scripts/generate-icon.swift`) and a full README with screenshots.
- Products → Revert Working Copy… with full file backups.
- Add Product: check out from the SVN server or adopt an existing working copy; name, type, slug, header name, main file and current version are detected automatically.

- `scripts/install-local.sh`: Release build for personal use, installed to /Applications.
- File ↔ directory type changes between versions are handled automatically.

### Fixed
- Working-copy revert could lose binary or newly added files; every changed file is now backed up first and the revert aborts if a backup fails.
- A preview changed after approval could be re-approved against a stale validation report.
- A stale "verified not committed" flag could survive a later uncertain commit.
- Ignored files in the working copy were not treated as local changes.
- `approve` was not serialized with other operations on the same product; stale records can no longer overwrite newer states.
- Crafted ZIP64 offsets could crash inspection through integer overflow.
- Verification failures now lead to Recovery Required instead of leaving a release in Verifying.
