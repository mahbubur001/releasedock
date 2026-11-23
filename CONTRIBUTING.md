# Contributing to ReleaseDock

Thanks for helping. ReleaseDock publishes code to every site that installs a WordPress.org plugin or theme, so correctness and safety come before features. Please read this page before opening an issue or a pull request.

## Reporting bugs and requesting features

- **Bugs:** open an issue with the *Bug report* form. Include the ReleaseDock version (Dashboard → *Installed*, or GitHub Updates → *Installed version*), macOS version, `svn --version --quiet`, the release mode and the wizard step where it went wrong.
- **Features:** open an issue with the *Feature request* form and describe the release problem you want solved, not only the solution.
- **Diagnostics:** *Settings → Export Diagnostics* produces a redacted report. Read it before attaching it; it should contain no credentials, but it does contain product slugs and release history.
- **Never post** SVN passwords, GitHub tokens, private keys, `wp-config.php` contents or unreleased code you're not allowed to share.

### Security issues

Do **not** open a public issue. Report privately through [GitHub Security Advisories](https://github.com/mahbubur001/releasedock/security/advisories/new). See [SECURITY.md](SECURITY.md) for the threat model.

## Development setup

- macOS 14 or later and Xcode 16 or newer
- `brew install subversion xcodegen`
- Optional: `php`, which enables the differential `version_compare()` test

```bash
xcodegen generate                  # the .xcodeproj is generated from project.yml
open ReleaseDock.xcodeproj         # scheme ReleaseDock
```

Everything else (layout, configuration, environment variables) is in [DEVELOPMENT.md](DEVELOPMENT.md). The design is described in [ARCHITECTURE.md](ARCHITECTURE.md).

## Making a change

1. Fork the repository and create a branch from `main`.
2. Put release logic in `Packages/ReleaseDockKit` (`ReleaseDockCore`), never in the SwiftUI app. The app only wires services together and renders state.
3. Add or update tests for every behaviour change. Integration tests use throwaway local repositories (`svnadmin create`, `file://`); **no test may contact WordPress.org or use real credentials.**
4. Run the full suite and the app build:
   ```bash
   scripts/test.sh
   ```
5. Add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
6. Open a pull request that explains what changed, why, and how you tested it.

## Rules the code must keep

These are the guarantees users rely on. A pull request that weakens one won't be merged.

- **No shell, ever.** Processes are started with an executable URL and an argument array.
- **Credentials stay in the Keychain.** They are passed to `svn` through stdin only, and never appear in arguments, logs, the database, manifests or diagnostics.
- **The source is read-only.** A dirty SVN working copy is never changed without an explicit user action that backs it up first.
- **One commit, and never retried blindly.** An uncertain commit outcome goes to `RecoveryRequired` and is resolved from the SVN log.
- **Approval binds to the exact preview.** Any change after approval requires approving again.
- **WordPress parity.** Version comparison and header parsing must keep matching PHP `version_compare()` and WordPress `get_file_data()`.

## Style

- Swift 6 language mode with complete concurrency checking. Services are `Sendable`, the pipeline is an actor, and view models are `@MainActor @Observable`.
- Services are protocols injected through `PipelineDependencies`, so tests can substitute fakes.
- Keep user-facing text short and specific, and say what the user can do next.

## Releases

Maintainers publish releases. ReleaseDock is distributed through GitHub releases of this repository and updates itself in-app with Sparkle; it is not distributed through the Mac App Store. Releases are made with the manual *Release* workflow or `scripts/publish-github-release.sh`; there is no automatic CI, so run `scripts/test.sh` before opening a pull request. See [RELEASE_WORKFLOW.md](RELEASE_WORKFLOW.md).

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
