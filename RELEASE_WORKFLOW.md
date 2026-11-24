# Release Workflow

Part 1 describes how ReleaseDock publishes WordPress.org plugins and themes. Part 2 describes how ReleaseDock itself is built, signed, notarized and shipped with Sparkle. Every WordPress.org rule below is sourced in [docs/research/WORDPRESS_SVN_RESEARCH.md](docs/research/WORDPRESS_SVN_RESEARCH.md); anything undocumented is listed as an assumption.

---

## Part 1 — Publishing WordPress.org plugins and themes

### Plugins

Repository: `https://plugins.svn.wordpress.org/<slug>` with `trunk/`, `tags/`, `assets/`. WordPress.org reads `Stable tag` from `trunk/readme.txt` and serves `tags/<stable tag>`; if the tag is missing or the value is empty it silently serves trunk (or refuses, with Release Confirmation enabled).

ReleaseDock's sequence, in one atomic commit (the same shape as `10up/action-wordpress-plugin-deploy`):

1. Sparse checkout (`--depth immediates`; `trunk` infinity; `tags` immediates; `assets` infinity only when assets sync is configured), or `svn update` if the working copy is clean.
2. Mirror the clean candidate into `trunk/` (deleting files that no longer exist), `svn add`/`svn delete` from `svn status --no-ignore` (so svn's global-ignores can't silently drop files such as `*.a`).
3. Optionally mirror the configured assets folder into `assets/` and set `svn:mime-type` for png/jpeg/gif/svg.
4. `svn copy trunk tags/<version>` (working-copy copy, carries the scheduled changes).
5. Preview, approval, `svn commit` of the working copy with message `Release <name> <version>` + `ReleaseDock-Release-ID: <uuid>`.
6. Verify `tags/<version>` exists and matches the candidate file-for-file (SHA-256 after export), and `trunk/readme.txt` Stable tag equals the version.

Checks specific to plugins: main file with `Plugin Name:` at the package root (the importer only scans the top level), `Version` header, readme present, `Stable tag` equal to the version (not `trunk`), `Tested up to` / `Requires PHP` format, plugin-file headers overriding readme values, ≤ 5 tags, changelog entry (optional), assets (`banner-*`, `icon-*`, `screenshot-*`) not shipped inside trunk.

**Release Confirmation** (opt-in per plugin on WordPress.org): every committer gets a “Pending release” email and must confirm at <https://wordpress.org/plugins/developers/releases/>. There is no documented way to detect whether it is enabled, so ReleaseDock always shows this guidance after a plugin release. Don't modify a confirmed tag — that requires re-confirmation.

### Themes

Repository: `https://themes.svn.wordpress.org/<slug>/<version>/` — no trunk or tags. ReleaseDock:

1. Checks out `--depth immediates` and fills only the previous (highest) version directory.
2. `svn copy <previous> <version>` (keeps history, as the Themes handbook instructs), mirrors the candidate into it, schedules adds/deletes.
3. Commits exactly one new version directory (the importer re-imports every changed version dir in a commit).
4. Verifies the new directory on the server.

WordPress.org imports the commit afterwards and runs Theme Check; failures are emailed (“Theme Import Failure…”) and that version does not go live, while the directory stays in SVN and cannot be overwritten. ReleaseDock therefore validates strictly before committing: version `^\d+(\.\d+)*$`, not all zeros, higher than the current version; `style.css` with `Theme Name`, `Description`, `Version`, `Tags` (importer-required) and the review headers; `screenshot.(png|jpg|jpeg|gif)`; `readme.txt`; `index.php` or `templates/index.html` (+ `theme.json`); no hidden files, `.sql`, `.sh`, `.dat`, `.wie`, `favicon.ico` or non-whitelisted `.xml`.

The first version of a theme must go through <https://wordpress.org/themes/upload/> and review; ReleaseDock blocks a theme repository that has no version directory.

### Release modes

| Mode | Behaviour |
|---|---|
| Dry Run | Inspect → … → Preview, then restore the working copy. Never approvable. |
| Assisted (default) | Stops at the preview; publishing requires a confirmation dialog. |
| Automatic | Publishes without a dialog only if the product has automation explicitly enabled **and** `AutomationGate` finds no blocking reason: valid final version, newer than current, identity fully confirmed, all required checks passed and none skipped, working copy clean and current, tag absent, credentials available, preview unchanged, no unresolved error. Otherwise the user is asked. |

### Authentication

1. If ReleaseDock has a password in the Keychain for `<host>` + username, svn runs with `--username U --password-from-stdin --no-auth-cache --non-interactive`; the password goes through stdin.
2. Otherwise, if `svn auth` lists cached credentials for the host's realm, svn uses them.
3. Otherwise publishing asks for the WordPress.org username (case-sensitive) and the **SVN password** from Profile → Account & Security → SVN credentials.

WordPress.org reads are anonymous, so credentials can only be proven valid by a commit. An authentication failure is rejected before anything is committed; ReleaseDock returns the release to *Previewed*, asks for credentials, and requires approval again.

### Failure handling and recovery

| Situation | Outcome |
|---|---|
| Validation fails (same/older/invalid version, existing tag, product mismatch, missing metadata, dirty working copy, remote changes, no credentials, network down…) | `Failed` with the report; nothing touched in SVN. |
| Working copy has local changes | Blocked. Never updated or modified automatically. *Revert Working Copy* backs up `svn diff`, the status list and unversioned files to `Backups/` first. |
| Preview changes after approval | Back to `Previewed`; approve again. |
| Repository changed after approval | `Failed`; working copy restored (backup kept). Prepare again. |
| Commit rejected (hook, out of date, conflict, forbidden) | `Failed`; working copy restored. |
| Credentials rejected | `Previewed`; enter credentials, approve again. |
| Commit timed out / interrupted / network error / unrecognised error | `RecoveryRequired`. ReleaseDock immediately searches `svn log` of the product path for the release ID. Found → verify and complete (never re-commit). Not found and tag absent → *verified not committed*; the user may publish again (explicit approval only) or abandon. Tag present without the ID → `Failed` (someone else created it). |
| App quit/crash during a release | On launch: pre-publish releases → `Cancelled` with the working copy restored; `Publishing`/`Verifying` → `RecoveryRequired`. |

### Assumptions (undocumented by WordPress.org)

- A single commit containing trunk changes and the new tag is acceptable (it is what the de-facto reference deploy action does; SVN commits are atomic).
- Plugin and theme pre-commit hook rules, commit-size limits and tag-count limits are undocumented; hook rejections are surfaced verbatim.
- WordPress.org has a release “cool-down” constant that is currently 0; if it is raised, updates will reach users later than the commit.

---

## Part 2 — Shipping ReleaseDock itself

### Current channel: GitHub releases without a Developer ID

Until a Developer ID certificate is available, builds are signed ad hoc and published as GitHub
releases of this repository, **mahbubur001/releasedock**:

```bash
scripts/publish-github-release.sh 0.1.1 2 --notes "What changed"
```

The script runs the tests, builds with `scripts/build-adhoc.sh`, signs the ZIP with Sparkle's
EdDSA key from the login Keychain, tags the source `vX.Y.Z`, creates the GitHub release with the
ZIP attached, then regenerates `updates/appcast.xml` (served from `raw.githubusercontent.com`) and
pushes it to `main`. Before tagging, it verifies the signature with the `SUPublicEDKey` built into the
app and stops if it does not match, so an update signed with the wrong key is never published.
Run it from an up-to-date `main`. The build number
must increase every release. Installed copies show the update in **GitHub Updates** and install
it with Sparkle after verifying the signature. Because these builds are not notarized, the very
first download needs **System Settings → Privacy & Security → Open Anyway**; in-app updates do not.

#### From GitHub Actions (no Xcode needed locally)

*Actions → Release → Run workflow* on `main` runs the same script on a free GitHub-hosted macOS
runner. It is manual only (no push, pull-request or scheduled runs). Enter the version, the build
number, the notes and whether it is a beta. One-time setup: store the Sparkle private key as the
repository secret `SPARKLE_PRIVATE_KEY` (`generate_keys -x <file>`, `gh secret set
SPARKLE_PRIVATE_KEY < <file>`, then delete the file). The script passes it to `sign_update` on
stdin (`--ed-key-file -`), never on the command line.

Back up the Sparkle private key (`generate_keys -x <file>`, then store it in a password manager):
losing it means existing installs can no longer verify new updates.

### Notarized distribution (Developer ID)

### One-time setup

1. **Developer ID**: an Apple Developer account with a *Developer ID Application* certificate in the login keychain. Put your team ID in `Config/Local.xcconfig` (`DEVELOPMENT_TEAM`).
2. **Notarization credentials**: `xcrun notarytool store-credentials releasedock-notary --apple-id <id> --team-id <team> --password <app-specific-password>`.
3. **Sparkle EdDSA keys**: build the project once so SwiftPM fetches Sparkle, then run `…/artifacts/sparkle/Sparkle/bin/generate_keys`. The private key is stored in your login keychain (back it up: `generate_keys -x private-key-file`). Put the printed public key in `Config/Local.xcconfig` as `SPARKLE_PUBLIC_ED_KEY`. **Never lose or change both the key and the signing certificate in the same release** — Sparkle accepts an update if either the EdDSA signature or a matching Developer ID signature verifies.
4. **Feed hosting**: choose an HTTPS location for `appcast.xml` and the archives (for example GitHub Pages `/updates/`). Set `SPARKLE_FEED_URL` (write `https:/$()/host/path` in xcconfig) and `RELEASEDOCK_GITHUB_REPOSITORY` (`owner/name`, used for update notifications).

Builds without `SPARKLE_FEED_URL` and `SPARKLE_PUBLIC_ED_KEY` keep in-app installation disabled; ReleaseDock never starts Sparkle without signature verification configured.

### Each release

```bash
DOWNLOAD_URL_PREFIX=https://<host>/updates/ scripts/release.sh 1.2.0 42          # stable
DOWNLOAD_URL_PREFIX=https://<host>/updates/ scripts/release.sh 1.3.0-beta1 43 --beta
```

`scripts/release.sh`:

1. Runs the test suite.
2. `xcodebuild archive` (Release, hardened runtime, `--timestamp`), then `-exportArchive` with `method = developer-id`, which re-signs Sparkle's `Autoupdate`, `Updater.app` and XPC services. (Never use `codesign --deep` to sign.)
3. Verifies the signature and refuses if `get-task-allow` is present.
4. Notarizes with `notarytool submit --wait`, staples the app, checks Gatekeeper with `spctl`.
5. Zips with `ditto -c -k --keepParent`, prints the SHA-256.
6. Copies the archive into `dist/updates/` and runs Sparkle's `generate_appcast`, which signs it with the EdDSA key from the keychain and writes `appcast.xml` (`--channel beta` for beta builds).

Then upload `dist/updates/` to the feed location and create the GitHub release `v<version>` with the ZIP attached (this drives the in-app “new version available” notification). Mark beta releases as pre-releases on GitHub.

Update CHANGELOG.md and bump `MARKETING_VERSION`/`CURRENT_PROJECT_VERSION` (the script passes them on the command line).
