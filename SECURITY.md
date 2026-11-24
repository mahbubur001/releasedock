# Security

ReleaseDock publishes code to every site that installs a plugin or theme, holds WordPress.org commit credentials, and updates itself. This document lists what it defends against and how.

## Reporting a vulnerability

Please report security issues privately to the maintainers (GitHub Security Advisories on the ReleaseDock repository) rather than in public issues.

## Threat model

| Asset | Threat | Mitigations |
|---|---|---|
| WordPress.org SVN password | Disclosure via argv, logs, files, crash reports | Stored only in the macOS Keychain (generic password, `kSecAttrAccessibleWhenUnlocked`, service `com.releasedock.svn.<host>`). Passed to svn with `--password-from-stdin --non-interactive --no-auth-cache`, never on the command line (visible in `ps`) and never cached by svn in `~/.subversion`. Not part of any model, manifest, database row, log line or diagnostic export. `SecretRedactor` scrubs `--password`, `Authorization` headers, GitHub tokens and `user:pass@` URLs from persisted notes, errors and diagnostics as defence in depth. |
| GitHub token (private update checks) | Disclosure, over-privilege | Optional; Keychain only; sent only to `api.github.com` as a Bearer token. Documented minimum: fine-grained token for one repository with *Contents: read*. |
| Release integrity | Publishing the wrong product/version, stale or tampered content | Identity check (type, header name, text domain, ZIP root); version from headers only; PHP-identical version comparison; candidate files re-hashed against inspection hashes; approval bound to a fingerprint of the exact working-copy changes and content; re-checked immediately before commit; post-commit verification of the server copy. |
| Duplicate or blind re-commit | Double release after an uncertain commit | Unique release ID in every commit message; uncertain outcomes go to `RecoveryRequired` and are resolved by searching the SVN log; no automatic retry; re-publishing requires proof the commit is absent plus explicit user approval. |
| Local machine | Malicious ZIP | Own ZIP parser with bounds checks on every structure; local/central header name cross-check; overlapping-entry detection; entry count, file size, total size and compression-ratio limits; encrypted entries, symlinks, devices/FIFOs, non-UTF-8 and control-character names, absolute/`..`/backslash paths and case-insensitive duplicates rejected; extraction only of re-validated paths into an empty private directory with `O_CREAT|O_EXCL|O_NOFOLLOW`, CRC-32 and size enforcement. Containment checks resolve symlinks with `lstat`+`realpath` on the deepest existing ancestor. |
| Local machine | Command injection | `Process` with `executableURL` and an argument array — no shell anywhere. Executables come from fixed paths (or an explicit settings path validated as executable). Environment is replaced with a minimal fixed one. svn targets containing `@` are peg-escaped; paths passed to svn are absolute and URLs come from validated product settings. Long path lists go through `--targets` files. |
| User data | Destroying local work | The development folder is only read. A dirty SVN working copy blocks the release and is never updated or modified automatically. Reverting is an explicit action that first writes `svn status` and `svn diff` and copies every added, modified, replaced, conflicted, unversioned and ignored file (binary files included) to `Backups/`; if any copy fails, nothing is reverted. Afterwards only files that were scheduled for addition, or that were backed up, are removed, and only inside the working copy. Ignored files other than OS junk count as local changes, so staging never overwrites them. The database is backed up before migrations; databases from newer versions are refused. |
| Development secrets in packages | `.env`, private keys, `wp-config.php`, `auth.json` published | Flagged as possible secrets: excluded automatically from folder sources (with a warning) and **blocking** in ZIP packages. Archives and binaries are blocking. |
| App updates | Malicious or tampered update | Sparkle 2 with EdDSA signatures (`SUPublicEDKey`) and Developer ID code-signature checks; HTTPS feed; `SUVerifyUpdateBeforeExtraction`. The updater does not start at all unless both feed URL and public key are configured. GitHub release data is only used to *notify*. |
| Data at rest | Other local users reading release data | Data directory `0700`, database `0600`, staging/manifests `0700`. |

## Logging

`OSLog` with subsystem `com.releasedock.app`. Product slugs, versions, states and svn subcommands are public; credentials are never passed to a logger. Diagnostics export (`Settings → Export Diagnostics`) contains products (without usernames — only whether one is set), recent releases with redacted notes and errors, and the last 24 h of ReleaseDock log entries, passed through `SecretRedactor`.

## Platform choices

- **Not sandboxed.** ReleaseDock must run `/opt/homebrew/bin/svn` and work in arbitrary folders. App Sandbox is required only for the Mac App Store; Developer ID distribution with Hardened Runtime and notarization is the intended channel.
- **Login keychain, not the data-protection keychain.** `kSecUseDataProtectionKeychain` needs a keychain-access-group entitlement backed by a provisioning profile; without one, calls fail with `errSecMissingEntitlement`. The login keychain's default ACL trusts only the creating app.
- **TLS.** svn certificate failures are reported (`E230001`), never bypassed (`--trust-server-cert-failures` is not used).

## Known limitations

- WordPress.org reads are anonymous, so SVN credentials are only proven valid by a commit (an auth failure there is safe and recoverable).
- Checksum verification can report differences if a repository applies `svn:eol-style`/keyword properties; these are reported as verification warnings, not hidden.
