# Changelog

All notable changes to ReleaseDock are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
