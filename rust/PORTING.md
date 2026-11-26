# Porting ReleaseDockCore (Swift) to Rust

The Rust workspace replaces `Packages/ReleaseDockKit` for the cross-platform Tauri app (see
`docs/plans/2026-11-26-layout-v2-and-cross-platform.md`). The Swift app keeps shipping on macOS
until the Tauri app reaches parity, so **behaviour must match the Swift code exactly**.

## Layout

One crate per area, in `rust/crates/`:

| Crate | Ports | Depends on |
|---|---|---|
| `rd-support` | `Support/` hashing, path safety, redaction, release notes, commit templates | — |
| `rd-model` | `Models/` product, validation, inspection (JSON-compatible with the Swift store) | — |
| `rd-versioning` | `Versioning/` PHP `version_compare`, headers, readme parser, product detector, version service, version bump | support, model |
| `rd-inspection` | `Inspection/` ZIP reader, ZIP/folder inspection, file classifier, path evaluator, asset inspector | support, model |
| `rd-svn` | `SVN/` + `Support/ProcessRunner` + `Support/XMLTree` | support, model |
| later | validation, store, keychain, GitHub, WordPress.org, code checks, build, git, release pipeline | … |

## Rules

1. **Port the tests first.** Each Swift test in `Packages/ReleaseDockKit/Tests` becomes a Rust test with
   the same intent (`testRejectsUnsafePaths` → `rejects_unsafe_paths`). Keep every assertion and every
   fixture value. A test may only be dropped if it tests Swift/Apple-only behaviour; list it in the crate's
   `README.md` under "Not ported" with the reason.
2. **Same behaviour, same messages.** User-facing strings (validation messages, suggestions, issue
   messages) are copied verbatim; the UI and existing release records depend on them.
3. **JSON compatibility.** Types that are stored (anything `Codable` saved in SQLite) use serde with
   `rename_all = "camelCase"`, explicit renames where Swift uses acronyms (`svnRepositoryURL`,
   `releaseID`, `packageSizeWarningMB`), `SwiftDate` for `Date` (seconds since 2001), `Id` for `UUID`
   (uppercase), and `#[serde(default, skip_serializing_if = "Option::is_none")]` for optionals.
   Swift enums with raw values serialize as their raw value. Add a round-trip test with a JSON sample.
4. **Errors:** `thiserror` enums per crate. No `unwrap`/`expect` in library code except for
   compile-time constants (regexes). No `unsafe` (the workspace forbids it).
5. **Cross-platform.** No macOS-only APIs. Paths use `std::path`; package paths inside ZIPs and SVN are
   always `/`-separated strings. Gate Unix-only tests (symlinks, permissions) with `#[cfg(unix)]`.
6. **External processes** (`svn`, `php`, `git`, `wp`) run without a shell, with arguments as a list,
   secrets only through stdin, and a timeout. Never log or store a password.
7. **Dependencies:** prefer well-known crates (`regex`, `serde`, `sha2`, `zip` is *not* used — the ZIP
   reader is our own security control, port it), add them to the crate's own `Cargo.toml` with a version.
   Ask before adding anything heavy.
8. **Style:** `cargo fmt` (max width 120), `cargo clippy --all-targets` with no warnings, doc comments
   ported from Swift doc comments. Keep files and functions organized like the Swift source so the two
   can be compared side by side.
9. **Done means:** `cargo test -p <crate>` passes, `cargo clippy -p <crate> --all-targets` is clean, and
   the crate `README.md` lists Swift file → Rust module and any tests not ported.

## Commands

```sh
cd rust
cargo test -p rd-support
cargo clippy -p rd-support --all-targets
cargo fmt
```

Parallel agents use their own target directory to avoid lock contention:
`CARGO_TARGET_DIR=target-<crate> cargo test -p <crate>`.
