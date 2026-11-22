# WordPress.org SVN Release Automation: Research Reference

Research date: 2026-11-22. Scope: automating WordPress.org plugin and theme releases from a macOS app through the `svn` CLI.

Conventions used here:
- **Confirmed facts** come from a primary source: a handbook page, a make.wordpress.org post, wordpress.org/meta source code (the GitHub mirror `WordPress/wordpress.org`, trunk, read 2026-11-22), WordPress core (`WordPress/wordpress-develop`, trunk), php-src, Subversion/serf source, or a live probe I ran. Each fact carries a short source tag such as [S3] (see Sources at the end).
- **Observed** means I checked it directly against the live servers on 2026-11-22.
- **Assumptions/unclear** covers anything not documented. Items with no primary source are marked **undocumented**.
- Server-side code changes often. The wordpress.org source quoted here is a snapshot. Handbook wording and code don't always agree, and where they differ both are given.

---

## 1. WordPress.org plugin SVN

### 1.1 Repository URL and layout

**Confirmed facts**
- Repo URL is `https://plugins.svn.wordpress.org/<slug>` [S1].
- Every plugin lives in **one shared repository**. `svn info --xml` on `https://plugins.svn.wordpress.org/hello-dolly/trunk` reports `<root>https://plugins.svn.wordpress.org</root>`, UUID `b8457f37-d9ea-0310-8a92-e5e31aec5664`, and HEAD revision about 3,816,106 (observed). Revision numbers are therefore global and grow because of other plugins' commits. Never run `svn log` against the repo root. Always target `<root>/<slug>/...`.
- Default directories are `/assets/`, `/tags/`, `/trunk/`. `/branches/` "is no longer created by default", is deprecated, and isn't used by WordPress.org [S1]. Older plugins such as hello-dolly still have `branches/` (observed).
- `trunk/` holds the latest code (it may be a beta). `tags/<version>/` holds releases. `assets/` holds banners, icons and screenshots [S1].
- The main plugin file must sit directly in `trunk/`, not in a subfolder: "Doing that will cause the zip generator to break" [S1][S6].
- svn externals "won't get added to the downloadable zip file" [S6].
- Tags "should look like version numbers", for example `2.0.1.3`. The FAQ says tags should contain only "numbers and periods". Semantic versioning is "strongly encouraged" [S1][S6].
- The importer scans only the **top level** of the exported stable directory for PHP files with headers: `find_plugin_headers($directory, $max_depth = -1)`. It prefers the first file with a non-empty `Name`, and throws `Could not find the plugin headers.` if none is found [S8].
- Read access is anonymous over HTTPS. `svn info`, `svn ls` and a plain HTTP GET all succeeded without credentials (observed).

**Assumptions/unclear**
- Server-side pre-commit hook rules for plugins (blocked file types, path restrictions, size caps) are **undocumented**. The hook source isn't in the public mirror.

### 1.2 How the Stable tag is resolved

**Confirmed facts (handbook)** [S2][S6]
- The directory reads `Stable tag` from **`/trunk/readme.txt`**, then uses the `readme.txt` inside `/tags/<stable>/` for the plugin page.
- Once a tag is the stable one, editing the trunk readme doesn't change the plugin page. The tag's own readme must also carry the correct Stable tag: "failing to do so may cause your plugin to not be updatable."
- The plugin **version** comes from the main PHP file's `Version:` header, not from the readme or the tag folder name.
- `Stable tag: trunk` "still works, but isn't supported or recommended". It is "discouraged and prohibited for new plugins". The FAQ asks authors not to use it because "that makes rollbacks more complicated."

**Confirmed facts (source: `plugin-directory/cli/class-import.php::export_and_parse_plugin`)** [S8]
1. `$stable_tag = 'trunk'` is the starting assumption.
2. If trunk has no files: if there are no tags either, it throws `Plugin has no files in trunk, nor tags.`. Otherwise stable becomes the highest tag by `version_compare()`.
3. If trunk has a readme matching `!^readme.(txt|md)$!i` (`readme.txt` preferred over `readme.md`), then `$stable_tag = $trunk_readme->stable_tag`.
4. If the stable value is non-empty and isn't `trunk`, the importer runs `svn info` on `tags/<stable>`. If that fails and the value starts with `0.`, it retries `tags/.<rest>`. It also requires `svn ls` of the tag to return files.
5. If the tag is missing or empty, **or the Stable tag value is empty or missing**, the importer falls back to trunk. A non-trunk value also records the warnings `stable_tag_invalid_trunk_fallback` and `stable_tag_invalid`.
6. If Release Confirmation is on and the resolved stable is trunk, it throws `Plugin cannot be released from trunk due to release confirmation being enabled.`
7. The `Version` header in the stable tree is validated:
   - Failing `/^\d+(?:\.\d+)*(?:-(?:rc|beta|alpha)(?:\.?\d+)?)?$/i` only produces the warning `version_header_unexpected_chars`.
   - A version that isn't path-safe is fatal (`invalid_version_header`).
   - A Version/tag mismatch is only a **warning** (`version_tag_mismatch`). The comparison is `version_compare(normalized_tag, normalized_version, '==')` with trailing `.0` segments stripped, so `1.0` matches `1.0.0`.
8. The readme parser's `sanitize_stable_tag()` trims quotes, strips a leading `tags/` or `/tags/`, removes characters outside `[a-z0-9_.-]`, and prefixes `0` to a leading `.` [S7].
9. `Requires at least` and `Requires PHP` come from the readme by default. They are **overridden by the main plugin file headers** `Requires at least` and `Requires PHP` when those match `!^[\d.]{3,}$!` [S8]. The handbook says these are read from the main PHP file "since WordPress 5.8" [S2].

**Implications for the tool**
- Before committing, check that `trunk/readme.txt` Stable tag equals the new version, `tags/<v>/readme.txt` Stable tag equals the new version, and the main file's `Version:` equals `<v>`.
- A missing tag or a typo in Stable tag **silently ships trunk** (or fails outright with Release Confirmation on). Warn loudly.

### 1.3 Recommended release steps

**Confirmed facts (handbook)** [S1]
- Check out with `svn co https://plugins.svn.wordpress.org/<slug> <dir>`, edit `trunk/`, then `svn add` new files and commit with `svn ci -m '...'`. A commit message is required.
- Tag by copying trunk: `svn cp trunk tags/2.0`, then commit with a message like `"tagging version 2.0"`. Then update `Stable Tag` in `trunk/readme.txt`.
- Edit in trunk and copy to a tag. Don't edit the tag directly. `svn copy` preserves history. Tags are "the only fully supported way" to make sure the correct version is served.
- Each push rebuilds zips for all versions. Updates can take "up to 6 hours" to appear. The FAQ says the directory "updates every few minutes" and asks authors to wait "at least 6 hours" before contacting the team [S1][S6].

**Single commit containing trunk changes and the new tag**
- The handbook doesn't say this is either allowed or forbidden (**undocumented as policy**). The handbook example uses separate commits.
- The de facto reference implementation (10up action, §1.10) does exactly this: one `svn commit` that includes the trunk changes, the `svn cp trunk tags/$VERSION`, and assets changes [S12].
- Subversion commits are atomic ("single atomic transaction"; all or nothing) [S22].
- The importer reads the post-commit state, so one commit that both bumps Stable tag in trunk and creates the tag should resolve correctly. That follows from the code in §1.2 but isn't stated in any doc.

### 1.4 Do-not-commit rules

**Confirmed facts**
- "Do not commit anything you aren't prepared to deploy to all users", explicitly including vendor files and `.gitignore` [S1].
- "Do not upload zip files. SVN expects individual files" [S1].
- Screenshots belong in `/assets/`, not `/trunk/` [S1].
- SVN "is a release repository, not a development one". Commit only deployable code: a stable release, beta or RC. "Frequent commits to a plugin should be avoided". Rapid trivial commits, including readme tweaks, can look like gaming "Recently Updated", with the exception of a readme-only "tested up to" bump (Guideline 14) [S5].
- "Plugin version numbers must be incremented for each new release." "The trunk readme.txt must always reflect the current version of the plugin" (Guideline 15) [S5].
- Guideline 4 requires human-readable source to be available, either in the plugin or linked from the readme [S5].
- Keep "as few as possible" old tags: "Just the last one or two for them is good" [S6].

**Assumptions/unclear**
- Hard limits on **commit size, file count, or number of tags** are **undocumented**. The importer code shows no tag-count cap [S8].
- The FAQ mentions a 10 MB limit for the **initial submission zip** upload [S6]. Whether any cap applies to SVN commits is **undocumented**.

### 1.5 Assets folder

**Confirmed facts** [S3][S8]
- Assets go in top-level `/assets/`, not `trunk/assets`.
- **Banners**: `banner-772x250.(jpg|png)` and the high-DPI `banner-1544x500.(jpg|png)`. The high-DPI file only supplements the normal one and can't be used alone. Max size is 4 MB.
- **Icons**: `icon-128x128.(png|jpg|gif)`, `icon-256x256.(png|jpg|gif)`, and `icon.svg`. An SVG icon needs a PNG fallback. Max size is 1 MB.
- **Screenshots**: `screenshot-N.(png|jpg)`, with lowercase names, numbered to match the `== Screenshots ==` list in the readme (one per line). Max size is 10 MB.
- **Localized and RTL variants**: `banner-772x250-rtl.png`, `banner-772x250-es_ES.png`, `screenshot-1-de.png`, and so on. A locale-specific file wins over `-rtl`.
- **Blueprints** (from source only): the cap is 100 KB per blueprint and at most 10 are used (`array_slice(..., 0, 10)`). Oversized or zero-byte assets are skipped silently by the importer [S8]. The handbook asset page doesn't cover blueprints.
- **MIME type**: images can download instead of displaying when served as `application/octet-stream`. Fix them with:
  ```
  svn propset svn:mime-type image/png *.png
  svn propset svn:mime-type image/jpeg *.jpg
  ```
  Alternatively, set `*.png = svn:mime-type=image/png` and the equivalent rules under auto-props in `~/.subversion/config`. Auto-props only affect newly added files [S3].
- Assets are served through a CDN and "cached heavily". Changes can take minutes, or up to 6 hours under load [S3].
- Handbook page last updated 2025-01-31 [S3].

**Tool recommendation:** set `svn:mime-type` explicitly per extension (png, jpeg, gif, svg+xml), as 10up does. Don't rely on the user's auto-props.

### 1.6 Release Confirmation

**Confirmed facts** [S4][S8][S9][S10]
- This is an opt-in feature. The handbook was last updated 2022-11-01; the feature was proposed in meta ticket #5352.
- The toggle is in the plugin's **Advanced View → Danger Zone**. Once enabled it can't be turned off from that screen; disabling requires emailing plugins@wordpress.org. This detail comes from the handbook via secondary summary. The fetched handbook text didn't show the toggle location, so treat it as likely but re-verify.
- Flow:
  1. Commit the tag as usual. You must still update `Stable tag` in `trunk/readme.txt`.
  2. WordPress.org detects the new tag and emails **all committers** (excluding bot and no-login accounts).
  3. A committer confirms on the **Release Management dashboard**, `https://wordpress.org/plugins/developers/releases/`. Every action there needs a tokenised link sent by email.
  4. After confirmation the release proceeds and "should update within a few minutes".
- A banner also appears on the plugin's directory page.
- Releases **must come from tags**. Trunk is refused.
- By default one committer's confirmation is enough. Multi-approval must be requested from the Plugins team. The source also supports "two-person" confirmation.
- Committers must receive the email directly. Shared inboxes and ticket systems don't qualify.
- After release, "the tagged version cannot be changed, including its readme". In source, modifying a confirmed tag (tag revision > release `source_revision`) sets `reset_confirmation`, so the modified tag must be re-approved from scratch [S8][S10].
- Email subject: `Pending release for <Plugin Name>`. The body says `<who> has committed a new version of <Plugin> - <version>. Action is required before the new version will be released.` It includes a confirmation link and the line "Once confirmed, the new version is released when the Stable tag in trunk/readme.txt points to it." [S9]
- An unconfirmed release blocks the import (`Plugin release {tag} not confirmed.`) [S8].
- Meta changeset 14274 (December 2024) added an intermediate state between "waiting for confirmation" and "confirmed" while the importer reprocesses.

**What the tool should tell the user after a successful commit (when RC is enabled)**
- "Committed r<N>. Release Confirmation is enabled for this plugin: every committer has been emailed. Open https://wordpress.org/plugins/developers/releases/ (or click the emailed link) to confirm version X. Until it's confirmed, users stay on the previous version."
- Also tell them not to modify `tags/X` after confirming. Any change requires re-confirmation, and the tag is otherwise treated as immutable.

**Assumptions/unclear**
- Whether the tool can detect that RC is enabled for a plugin is **undocumented**. No public API is documented. The dashboard requires login.
- Release cooldown: the source defines `RELEASE_COOL_DOWN_DELAY`, which delays serving a new version to the update API. Its default is currently **0 (disabled)**, with a code comment saying it "will be raised once the surrounding workflow is ready" [S11]. A future delay before users receive updates is plausible. It's **undocumented** in the handbook.

### 1.7 readme.txt header and parser rules

**Confirmed facts (handbook, last updated 2026-03-11)** [S2]
- Header fields: `=== Plugin Name ===`, then `Contributors`, `Donate link`, `Tags`, `Requires at least`, `Tested up to`, `Requires PHP`, `Stable tag`, `License`, `License URI`, then the short description.
- `Contributors` are comma-separated wp.org usernames, case-sensitive. Display names don't work.
- `Tags` takes 1 to 5 terms.
- `Tested up to` is a major version, numbers only.
- `Requires PHP` is optional, numbers only.
- `License` must be GPLv2-or-later compatible.
- The short description has a recommended maximum of 150 characters, no markup, and is truncated beyond that.
- Readmes larger than 10k "may cause errors". Move old changelog entries to `changelog.txt`.

**Confirmed facts (parser source: `plugin-directory/readme/class-parser.php`)** [S7]
- **Name line**: no regex. The parser takes the first non-blank line and trims it with `trim($line, "#= \t\0\x0B")`. If the result is literally `plugin name`, it uses the next line when that line is under 50 characters and isn't a header. Warning: `invalid_plugin_name_header`.
- **Header keys** are case-insensitive and split on the first `:`. A header line must not start with `#` or `=`. Parsing stops at the first non-header, non-blank line.
- Recognized keys and aliases:
  - `tested`, `tested up to`
  - `requires`, `requires at least`
  - `requires php`
  - `tags`
  - `contributors`
  - `donate link`
  - `stable tag`
  - `license`
  - `license uri`
- **Tags**: `plugin` and `wordpress` are removed first. More than 5 sets the `too_many_tags` warning; the extras are dropped.
- **Contributors**: a leading `@` is stripped, and each name is looked up by login, then by slug. Unknown names are dropped (`contributor_ignored`). No count limit.
- **Requires at least / Tested up to**:
  - Removes `WordPress`, `WP`, `or higher`, `and above`, `+`.
  - Drops anything after `-`.
  - Must match `^\d+\.\d(\.\d+)?$`.
  - Values above `WP_CORE_STABLE_BRANCH + 0.1` are ignored, with warnings `requires_header_ignored` and `tested_header_ignored`.
- **Requires PHP** must match `^\d+(\.\d+){1,2}$`, else it's ignored (`requires_php_header_ignored`).
- **Short description**: 150 characters (`trimmed_short_description`, `no_short_description_present`).
- **Sections** are trimmed by **word** count: 2500 by default, 5000 for `changelog` and `faq` (`trimmed_section_<name>`). The upgrade notice has no length cap in the parser.
- **License** warnings: `license_missing`, `invalid_license`, `unknown_license`.
- **readme.md**: supported **as a fallback**. The import code matches `readme.(txt|md)` case-insensitively and prefers `.txt` [S8]. The handbook doesn't mention readme.md support (undocumented in docs, confirmed in code).
- Validator UI: `https://wordpress.org/plugins/developers/readme-validator/`. A validator class exists in source (`readme/class-validator.php`).

### 1.8 Plugin main-file header parsing (WordPress core)

**Confirmed facts (core trunk source)** [S13][S14]
- `get_file_data($file, $default_headers, $context)` in `wp-includes/functions.php`:
  - Reads **only the first 8 KB**: `file_get_contents($file, false, null, 0, 8 * KB_IN_BYTES)`.
  - Converts `\r` to `\n`, which handles CR-only line endings.
  - For each header it uses:
    `preg_match('/^(?:[ \t]*<\?(?:php)?)?[ \t\/*#@]*' . preg_quote($name, '/') . ':(.*)$/mi', $data, $m) && $m[1]`
    That means multiline and **case-insensitive** matching. The line may start with an optional `<?php`, then any run of spaces, tabs, `/`, `*`, `#` or `@`, then `Header Name:`. The first match wins. An empty value counts as not found.
  - The value is cleaned by `_cleanup_header_comment()`, which is `trim(preg_replace('/\s*(?:\*\/|\?>).*/', '', $str))` and strips a trailing `*/` or `?>` and everything after it.
  - Extra headers come from the `extra_{$context}_headers` filter.
- `get_plugin_data()` headers:
  - `Plugin Name`, `Plugin URI`, `Version`, `Description`, `Author`, `Author URI`
  - `Text Domain`, `Domain Path`, `Network`
  - `Requires at least`, `Requires PHP`, `Update URI`, `Requires Plugins`
  - `Site Wide Only` (deprecated in favor of `Network`)
- `Network` is true only when the value is `true` (case-insensitive). `TextDomain` falls back to the plugin folder slug.
- `get_plugins()` scans `.php` files at the top level of `wp-content/plugins` and **one level deep** in each subdirectory. A file counts as a plugin only if `Name` (`Plugin Name:`) is non-empty.
- The WordPress.org importer scans only the top level of the release directory (§1.1).

**Implication:** to mirror core, read the first 8192 bytes, normalize `\r`, and apply the same regex per header, case-insensitively, first match.

### 1.9 Version number rules (plugin side)

- Tag dirs should be numbers and periods [S6].
- The importer's "expected" pattern is `^\d+(?:\.\d+)*(?:-(?:rc|beta|alpha)(?:\.?\d+)?)?$` (case-insensitive). Anything else only warns [S8].

### 1.10 Reference implementation: 10up/action-wordpress-plugin-deploy (`deploy.sh`, develop branch) [S12]

```
svn checkout --depth immediates "https://plugins.svn.wordpress.org/${SLUG}/" "$SVN_DIR"
cd "$SVN_DIR"
svn update --set-depth infinity assets
svn update --set-depth infinity trunk
svn update --set-depth immediates tags
# early exit if tags/$VERSION already exists (prints "already published")
rsync -rc <source or .distignore-filtered or BUILD_DIR>/ trunk/ --delete --delete-excluded
rsync -rc "$GITHUB_WORKSPACE/$ASSETS_DIR/" assets/ --delete
svn add . --force > /dev/null
svn status | grep '^!' | sed 's/! *//' | xargs -I% svn rm %@ > /dev/null
svn cp "trunk" "tags/$VERSION"
svn propset svn:mime-type "image/png"     assets/*.png || true   # also image/jpeg *.jpg,
svn propset svn:mime-type "image/jpeg"    assets/*.jpg || true   # image/gif *.gif,
svn propset svn:mime-type "image/gif"     assets/*.gif || true   # image/svg+xml *.svg
svn propset svn:mime-type "image/svg+xml" assets/*.svg || true
svn update
svn status
svn commit -m "Update to version $VERSION from GitHub" --no-auth-cache --non-interactive \
  --username "$SVN_USERNAME" --password "$SVN_PASSWORD"
```

Notes:
- The whole release is a **single commit** containing trunk, the new tag, and assets.
- The `%@` suffix on `svn rm` escapes filenames that contain `@`.
- It passes `--password` on argv. A native app should use `--password-from-stdin` instead (§4).
- It doesn't verify the readme Stable tag. It doesn't handle a pre-existing tag beyond skipping.

---

## 2. WordPress.org SVN authentication

**Confirmed facts**
- The username is your **WordPress.org username, not your email**. It is **case-sensitive** ("JaneDoe" must keep its capitals) and may contain spaces on older accounts. New usernames can't contain uppercase letters or spaces, and existing usernames can't be changed [S1][S15].
- **SVN password**: announced 2024-09-04 on the Make Plugins blog alongside mandatory 2FA, effective 2024-10-01 for plugin and theme authors [S16].
  - It's a separate, randomly generated password; you can't choose your own.
  - Generate it on your profile under **Account & Security → SVN credentials → Generate Password** [S15]. A third-party direct link is `https://profiles.wordpress.org/me/profile/edit/group/3/?screen=svn-password` (unverified).
  - It can be revoked without changing the account password.
  - 2FA isn't applied to SVN itself "due to technical limitations". SVN uses HTTP Basic auth, so the prompt labels can't be changed.
  - CI and deploy scripts must store the SVN password [S16].
- **Is the SVN password strictly required yet?** The Meta handbook "Subversion Access" page (Dion Hulse; published 2024-08-30, last modified **2026-08-25**) says account passwords are temporarily still accepted with SVN, and "We have not yet announced when we'll cease to accept the account password with SVN." [S15]. The plugin handbook says "You can set an SVN-specific password" [S1].
  **Tool guidance:** always ask the user for the *SVN password*, explain where to generate it, and don't assume the account password will keep working.
- **Realm**: an unauthenticated write request to `plugins.svn.wordpress.org` and to `themes.svn.wordpress.org` returns `401` with `WWW-Authenticate: Basic realm="Use your WordPress.org login"` (observed).
  - serf builds the client realmstring as `"<%s://%s:%d> %s"` [S23], so the svn realmstring is `<https://plugins.svn.wordpress.org:443> Use your WordPress.org login`. The theme host uses the same pattern. This is derived from source plus the observed header; not tested with a real login.
- **Read access is anonymous**: checkout, `info`, `ls` and `log` work without credentials (observed).

**Assumptions/unclear**
- Write authorization rules: plugin committers are managed in the plugin's Advanced view. Theme write access is **undocumented** (see §3).
- Rate limiting and lockout after repeated auth failures are **undocumented**.

---

## 3. WordPress.org theme SVN

### 3.1 URL and layout

**Confirmed facts**
- Repo URL is `https://themes.svn.wordpress.org/<slug>/`. Each version is a top-level directory `/<slug>/<version>/`. There is **no trunk, tags or branches** (observed: `twentytwentyfive/` lists `1.0/ ... 1.3/ ...`) [S17][S18].
- The import job only treats paths matching `^/<slug>/<version starting with a digit>(/...)?` as versions [S19].

### 3.2 Can authors commit new versions via SVN?

**Confirmed facts**
- Yes. The handbook "Updating Your Theme" (published 2022-01-06, last modified 2022-02-17) says themes already in the directory can be updated by zip upload or by SVN [S17]. Learn.wordpress.org (2024-07) agrees [S18]. Meta ticket #5899, "Allow theme updates via SVN", introduced this in 2021.
- The documented procedure [S17]:
  ```
  svn co https://themes.svn.wordpress.org/<slug>/
  svn cp 1.0.1 1.0.2          # copy previous version WITH history into new version dir
  # edit; bump Version in style.css; update readme.txt changelog
  find . -name ".DS_Store" -print -delete   # remove macOS cruft (and __MACOSX)
  svn commit -m "..."
  ```
- "It is important to name the new directory with the new version number." The new version must be higher than the current one. "Once you commit, the changes cannot be overwritten". Corrections require another, higher version directory [S17].
- A successful upload sends a confirmation email. The update "may take some time" to appear [S17][S18].

**Confirmed facts (server source; post-commit processing)** [S19][S20]
- Theme Check runs **after the commit**, not as a pre-commit hook.
  1. A cron watcher runs `svn log -v --xml` over new revisions of `themes.svn.wordpress.org` and queues an import per changed slug/version.
  2. `process_update_from_svn()` exports `https://themes.svn.wordpress.org/<slug>/<version>/` and runs the same `import()` path as zip uploads, with `expected_version = <dir name>`.
- Failures are **emailed** to the theme author with subject `Theme Import Failure: <Theme> [<rev>] <commit msg>`, including Theme Check HTML (BCC themes@wordpress.org). The commit stays in SVN, but that version doesn't go live.
- Checks during import:
  - `style.css` exists (shortest path wins).
  - A screenshot exists (`screenshot.(jpg|jpeg|png|gif)`).
  - `Theme Name`, `Description` and `Tags` are present.
  - **Version identity**:
    - The `style.css` `Version` must match `^\d+(\.\d+)*$` and must not be all zeros.
    - It must **exactly equal the SVN directory name** (`version_mismatch`).
    - It must be `version_compare(... , max_version, '>')` (`invalid_version`).
  - The committer must be the theme's post author (`cannot_upload_theme`).
  - The theme must not be suspended.
  - No unreviewable files and no non-portable filenames.
  - Theme Check must pass; it's blocking except for special or default themes.
  - If a Trac ticket is created and the theme already has a live version, the update is **auto-approved**. The ticket priority is "theme update" and the action is `new_no_review`.
- SVN-imported versions are set `live` directly (`version_status = 'live'`).
- Theme cooldown constant `WPORG_THEMES_RELEASE_COOL_DOWN_DELAY` currently defaults to **0**. The comment says it will be raised later. With a delay, versions would sit in `approved` before going live.
- A retry is scheduled once (after 1 hour) on an SVN export error.

**Assumptions/unclear**
- Server-side write authorization (who can commit to `themes.svn/<slug>`) is **undocumented**. The importer rejects commits by anyone other than the theme author.
- A pre-commit hook exists: the zip path surfaces `pre-commit-hook` errors. Its rules for author commits are **undocumented**. Ticket #5899 said SVN updates would "at least initially" have format restrictions. The ticket page returned 403 to my fetcher.
- Whether `svn cp <prev> <new>` is *required*, rather than just recommended, is **undocumented**. The docs instruct it, and a plain `svn add` of a new directory would presumably also import.
- Behavior when one commit touches several version dirs: the importer re-imports **every** changed version [S19]. Commit exactly one new version directory per commit.
- Whether themes in the initial review queue (not yet live) can be updated via SVN is **undocumented**. The docs say queued themes are updated by **zip** upload, and a version bump doesn't change queue position [S17].

### 3.3 Required theme files and headers

**Confirmed facts**
- `style.css` required headers (marked * on the handbook page, last updated 2021-03-01):
  - `Theme Name`, `Author`, `Description`, `Version`
  - `Requires at least`, `Tested up to`, `Requires PHP`
  - `License`, `License URI`, `Text Domain`
  Version format is `X.X` or `X.X.X`. `Tested up to` is a major version, number only [S21][S24].
- Review requirements (Themes Team "Required" page, last updated 2026-06-09) [S24]:
  - A valid `readme.txt` (Contributors with one username, Requires at least, Tested up to, Requires PHP, License, License URI).
  - Block themes need `templates/index.html` plus `theme.json`.
  - Screenshot no larger than 1200×900, 4:3 ratio, and it must not look like an advertisement.
  - No hidden files and no zip files.
  - Remove `.git`, `.svn`, `.hg`, `.bzr`.
  - Disallowed types: `thumbs.db`, `desktop.ini`, `.sql`, `.sh`, `favicon.ico`, `.dat`, `.wie`, and generic `.xml` (except `wpml-config.xml`, `loco.xml`, `phpcs.xml`).
  - Mixed DOS/UNIX line endings "can cause SVN problems".
  - Minified assets need their unminified sources.
- Core `WP_Theme` raises `theme_no_index` unless a classic theme has `index.php`, or a block theme has `templates/index.html` (or the legacy `block-templates/index.html`) [S25].
- The importer uses `get_file_data()` (§1.8) on `readme.txt` with headers: the "first readme.txt" by path length [S20].

---

## 4. Subversion CLI on macOS

**Confirmed facts**
- Apple stopped bundling `svn`. Xcode 10 removed IDE support. Xcode 11 and the macOS 10.15 release notes announced removal of the command-line tools; Apple Silicon CLT lacks `svn`. Install via Homebrew (`brew install subversion`). This comes from Apple release notes via secondary quotes. This machine has `/opt/homebrew/bin/svn` 1.14.5 (observed).
- **`--password-from-stdin`** was added in **Subversion 1.10.0** (CHANGES: "New '--password-from-stdin' option for 'svn' (r1819093)", released 2018-04-12/13) [S26].
  - It **requires `--non-interactive`**. Otherwise svn fails with `E205000: --password-from-stdin requires --non-interactive` [S27, svn.c].
  - It reads **one line** from stdin (`svn_cmdline__stdin_readline`).
  - Don't combine it with `-F -`, which also reads stdin. Write the log message to a temp file and use `-F <file>`, or use `-m`.
  - `--password ARG` is visible to other users (`ps`); the help text itself warns about this.
- **`--non-interactive`**: prompting is off by default when stdin isn't a TTY. Use it always, plus `--trust-server-cert-failures` only if needed (it requires `--non-interactive`). Since 1.10, `--non-interactive` implies `--accept recommended` for conflicts [S28].
- **`--no-auth-cache`**: don't store credentials.
- **`svn auth`** (new in **1.9**) lists and removes cached credentials [S29].
  - Usage: `svn auth [PATTERN...]` and `svn auth --remove PATTERN...`. Patterns are case-sensitive globs, and a credential must match all of them.
  - `--show-passwords` displays cached passwords, where the store allows it.
  - The output shows the realmstring, username, password-cache type and, for SSL, certificate details.
  - Use `svn auth 'https://plugins.svn.wordpress.org*'` to check whether a credential is cached, and `svn auth --remove '<https://plugins.svn.wordpress.org:443>*'` to clear it.
- **Password stores**: the `~/.subversion/config` `[auth]` setting `password-stores` lists valid values `gnome-keyring`, `kwallet`, `gpg-agent`, `keychain` (Mac OS X) and `windows-cryptoapi`. The default on keychain builds is `keychain`. An empty list disables all stores [S30].
  - The macOS keychain provider stores a **generic password** whose **service = realmstring** and **account = username** (`SecKeychainAddGenericPassword`). With `--non-interactive` it calls `SecKeychainSetUserInteractionAllowed(FALSE)` [S31], so a locked keychain fails instead of prompting.
  - `svn --version` on this Homebrew build lists only "Mac OS X Keychain" as the credential cache (observed).
- **Recommended approach for an app**: keep the SVN password in the app's own Keychain item. Run `svn ... --non-interactive --no-auth-cache --username U --password-from-stdin` and write the password plus `\n` to stdin. Credentials then never touch argv or `~/.subversion/auth`.
- **`--xml`** is supported by `info`, `status`, `log` and `list` (observed in `svn help`). `svn info --show-item <kind|url|relative-url|repos-root-url|repos-uuid|repos-size|revision|last-changed-revision|last-changed-date|last-changed-author|wc-root>` has existed since 1.9 [S29].
- **Exit codes**: `svn` returns `EXIT_SUCCESS` (0) or `EXIT_FAILURE` (1). There are no finer codes [S27].
  - Errors print to stderr as `svn: E<6-digit>: message`; warnings print as `svn: warning: W<code>: ...`.
  - Some warnings still produce exit 1. Observed: `svn info` on a missing tag URL prints `W170000 ... non-existent in revision N` and then `E200009: Could not display info for all targets because some targets don't exist`, exit 1.
- **HTTP timeout**: the default is 600 s (`DEFAULT_HTTP_TIMEOUT 600` in ra_serf). Configure it with `http-timeout` in `~/.subversion/servers` or `--config-option servers:global:http-timeout=N` [S32].

### 4.1 Error codes, computed from `svn_error_codes.h` (1.14.x) with APR and serf bases [S33][S34]

| Code | Symbol | Meaning / typical cause |
|---|---|---|
| E170001 | SVN_ERR_RA_NOT_AUTHORIZED | Authorization failed: wrong password or username, or no write permission |
| E215004 | SVN_ERR_AUTHN_FAILED | Authentication failed (e.g. "No more credentials or we tried too many times") |
| E215000 | SVN_ERR_AUTHN_CREDS_UNAVAILABLE | No credentials available (non-interactive, nothing cached) |
| E175013 | SVN_ERR_RA_DAV_FORBIDDEN | HTTP 403 (no commit access to that path) |
| E165001 | SVN_ERR_REPOS_HOOK_FAILURE | Server pre-commit hook rejected the commit; the message contains hook output |
| E160013 | SVN_ERR_FS_NOT_FOUND | Path not found in repository |
| E160020 / E175005 | FS_ALREADY_EXISTS / RA_DAV_ALREADY_EXISTS | Path already exists (e.g. tag created concurrently) |
| E160024 / E160028 / E160042 / E155011 | FS_CONFLICT / TXN_OUT_OF_DATE / FS_OUT_OF_DATE / WC_NOT_UP_TO_DATE | Out-of-date working copy; run `svn update` |
| E155004 | SVN_ERR_WC_LOCKED | Working copy locked |
| E155037 | SVN_ERR_WC_CLEANUP_REQUIRED | Previous operation interrupted; run `svn cleanup` |
| E155010 | SVN_ERR_WC_PATH_NOT_FOUND | WC path missing |
| E155015 | SVN_ERR_WC_FOUND_CONFLICT | Conflict blocks operation |
| E170013 | SVN_ERR_RA_CANNOT_CREATE_SESSION | Unable to connect (wraps DNS/TCP errors below) |
| E175002 | SVN_ERR_RA_DAV_REQUEST_FAILED | Generic HTTP request failure |
| E175012 | SVN_ERR_RA_DAV_CONN_TIMEOUT | Connection timed out |
| E120108 | SERF_ERROR_ABORTED_CONNECTION | Server closed the connection unexpectedly |
| E230001 | SVN_ERR_RA_SERF_SSL_CERT_UNTRUSTED | TLS certificate untrusted |
| E200015 | SVN_ERR_CANCELLED | Interrupted (SIGINT) |
| E200009 | SVN_ERR_ILLEGAL_TARGET | Some targets don't exist |
| E205000 | SVN_ERR_CL_ARG_PARSING_ERROR | Bad CLI arguments |
| E000060 / E000061 / E000054 / E000065 | APR passthrough of Darwin errno | ETIMEDOUT / ECONNREFUSED / ECONNRESET / EHOSTUNREACH |
| E670008 | APR_OS_START_EAIERR + EAI_NONAME | DNS failure ("nodename nor servname provided") |

The APR rows use Darwin errno values (`<sys/errno.h>`, `<netdb.h>`) plus APR bases (USERERR = 120000, EAIERR = 670000). They are derived, not taken from WordPress docs. Parse codes with the regex `svn: (E\d{6}):` and treat the **last** E-code line as the most specific.

**Assumptions/unclear**
- Which code WordPress.org returns for a wrong password: plain HTTP Basic 401 with serf typically yields `E170001` with "Authorization failed", sometimes preceded by `E215004`. Not tested with real credentials. Match on both.

### 4.2 Detecting whether a commit happened after a client-side timeout or crash

**Confirmed facts**
- Commits are atomic: all or nothing [S22]. The server may still finish a commit after the client loses the connection. svnbook doesn't describe how the WC recovers (undocumented).

**Recommended procedure (assumption-based, built on documented commands)**
1. Before committing, record `svn info --show-item revision URL/<slug>` and the full set of intended paths.
2. After a timeout or ambiguous failure:
   - Run `svn info --xml https://plugins.svn.wordpress.org/<slug>/tags/<v>`. If it exists, `<commit revision>`, `<author>` and `<date>` identify it.
   - Run `svn log --xml -v -l 1 https://plugins.svn.wordpress.org/<slug>` and compare the author, the msg (embed a unique release ID in the commit message) and the changed paths.
3. If found, run `svn cleanup` (if E155037 appears) and then `svn update` in the WC to bring it to the committed state. Don't recommit.
4. If not found, run `svn cleanup` and `svn status`, then retry the commit.
5. For themes, check `https://themes.svn.wordpress.org/<slug>/<v>` in the same way.

### 4.3 `svn cp` of a modified working-copy dir, then a single commit

**Confirmed facts**
- WC→WC `svn copy` means "copy and schedule for addition (with history)". It doesn't contact the repository. By default the copy source is `BASE` [S35]; this is from `svn help copy`.
- WC→URL "immediately commit[s] a copy of WC to URL". URL→URL is a server-side copy.

**Assumptions/unclear (behavior widely relied upon, e.g. by 10up, but not spelled out in svnbook)**
- `svn cp trunk tags/X` after modifying trunk copies the *working* state, local edits and scheduled adds included. One `svn commit` at the WC root then commits the trunk modifications and the new `tags/X` (copied-from `trunk@BASE` plus the same edits) atomically.
- Run `svn add`/`svn rm` (resolve `!` missing and `?` unversioned items) **before** `svn cp`, so the tag matches trunk.
- Use `svn update` first so trunk's BASE isn't mixed-revision or out of date. Out-of-date paths cause `E160024`/`E155011`.

---

## 5. PHP `version_compare()`, exact algorithm

Source: `php-src/ext/standard/versioning.c` (master, read 2026-11-22) [S36]. WordPress core uses it for update decisions, for example `version_compare($update->new_version, $plugin_data['Version'], '>')` in `wp_update_plugins()` for `Update URI` hosts [S37]. The wp.org importer uses it for tag fallback and Version/tag matching [S8]. The theme importer requires the new version to be greater than `max_version` [S20]. The api.wordpress.org update-check logic wasn't reviewed.

**Empty strings**: `""` == `""`, and an empty string is less than any non-empty string.

**Canonicalization** (`php_canonicalize_version`), skipped if the string starts with `#`:
1. The first character is copied verbatim.
2. For each later character `c`, with previous input char `lp` and last output char `lq`:
   - If `c` is `-`, `_` or `+`, emit `.` (unless `lq` is already `.`).
   - Else, on a digit↔non-digit transition (where `.` counts as neither), emit `.` (unless `lq` is `.`) and then `c`.
   - Else, if `c` isn't alphanumeric (including a literal `.`), emit `.` (unless `lq` is `.`). Repeated separators collapse.
   - Else emit `c`.
3. Newer php-src strips a trailing `.` (fix "trailing dot", php-src commit 60b3fa3c76, April 2026; present in the PHP-8.4 and PHP-8.5 branch heads, absent in PHP-8.3).

Examples: `1.0-rc1` → `1.0.rc.1`; `1.0RC1` → `1.0.RC.1`; `2.0beta` → `2.0.beta`; `1.0_p2` → `1.0.p.2`.

**Comparison** (`php_version_compare`): split both strings on `.` and walk the parts pairwise.
- **Both numeric**: compare numerically.
  - Older PHP (≤ 8.3 and earlier patch releases): `strtol`, so very long numbers overflow.
  - Current branches: leading zeros are stripped, then the numbers compare by digit length and then lexically. This allows arbitrary-length numbers (commits de5c604302 and 362a212584, Nov 2026).
  - So `01` == `1`.
- **Both non-numeric**: compare by special-form order.
- **Mixed**: the number is treated as the special form `#`. So `#N#` vs the string, which means a number beats `dev`/`alpha`/`beta`/`RC` and loses to `pl`/`p`.
- If one side has parts left over:
  - If the next leftover part is numeric, the longer side is greater (`1.0` < `1.0.0`).
  - Otherwise compare `#N#` against the leftover (`1.0` > `1.0-rc1`, `1.0` < `1.0pl1`, `1.0` > `1.0-foo`).

**Special-form order.** The match is a **prefix match** (`strncmp(part, name, strlen(name))`), checked in table order:
```
dev (0) < alpha = a (1) < beta = b (2) < RC = rc (3) < # (4) < pl = p (5)
```
- Any part matching none of these gets **-1**, lower than `dev`.
- Because of the prefix match, `bogus` counts as beta, `abc` as alpha, `patch` as pl and `devel` as dev.
- Case matters except for the explicit `RC`/`rc` pair: `Rc` and `Beta` match nothing and get -1.

**Operators**: `<`/`lt`, `<=`/`le`, `>`/`gt`, `>=`/`ge`, `==`/`=`/`eq`, `!=`/`<>`/`ne`. Anything else throws ValueError (PHP 8). Current master also rejects NUL bytes in arguments (8ba59c8b5f).

**Worked examples**
```
1.0 < 1.0.0
1.0.0 > 1.0-rc1
1.0-dev < 1.0-alpha < 1.0-beta < 1.0-RC1 < 1.0 < 1.0pl1
1.10 > 1.9
1.0.0-beta.2 > 1.0.0-beta.1
1.0-Beta1 < 1.0-dev      # 'Beta' unrecognised => -1
```

**Implementation note:** port the algorithm verbatim, including prefix matching and unknown-string = -1, rather than using semver. Test it against a table generated with real PHP.

---

## Sources

- [S1] Using Subversion, Plugin Handbook (last modified 2025-03-26): https://developer.wordpress.org/plugins/wordpress-org/how-to-use-subversion/
- [S2] How Your Plugin Readme Works (last updated 2026-03-11): https://developer.wordpress.org/plugins/wordpress-org/how-your-readme-txt-works/
- [S3] Plugin Assets (last updated 2025-01-31): https://developer.wordpress.org/plugins/wordpress-org/plugin-assets/
- [S4] Release Confirmation Emails (last updated 2022-11-01): https://developer.wordpress.org/plugins/wordpress-org/release-confirmation-emails/ ; dashboard: https://wordpress.org/plugins/developers/releases/ ; tickets https://meta.trac.wordpress.org/ticket/5352 , https://meta.trac.wordpress.org/changeset/14274
- [S5] Detailed Plugin Guidelines (last modified 2026-03-11): https://developer.wordpress.org/plugins/wordpress-org/detailed-plugin-guidelines/
- [S6] Plugin Developer FAQ (last updated 2026-11-10): https://developer.wordpress.org/plugins/wordpress-org/plugin-developer-faq/
- [S7] Readme parser source: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/plugin-directory/readme/class-parser.php
- [S8] Plugin importer source: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/plugin-directory/cli/class-import.php
- [S9] Release confirmation email source: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/plugin-directory/email/class-release-confirmation.php
- [S10] Immutable-tag test: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/plugin-directory/tests/Release_Confirmation_Immutable_Tag_Test.php
- [S11] Plugin release cooldown constant: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/plugin-directory/plugin-directory.php
- [S12] 10up deploy script: https://github.com/10up/action-wordpress-plugin-deploy/blob/develop/deploy.sh
- [S13] Core `get_file_data()` and `_cleanup_header_comment()`: https://github.com/WordPress/wordpress-develop/blob/trunk/src/wp-includes/functions.php
- [S14] Core `get_plugin_data()` and `get_plugins()`: https://github.com/WordPress/wordpress-develop/blob/trunk/src/wp-admin/includes/plugin.php
- [S15] Meta handbook, Subversion Access (published 2024-08-30, last modified 2026-08-25): https://make.wordpress.org/meta/handbook/tutorials-guides/svn-access/
- [S16] Upcoming Security Changes for Plugin and Theme Authors (2024-09-04): https://make.wordpress.org/plugins/2024/09/04/upcoming-security-changes-for-plugin-and-theme-authors-on-wordpress-org/
- [S17] Updating Your Theme, Theme Handbook (last modified 2022-02-17): https://developer.wordpress.org/themes/releasing-your-theme/updating-your-theme/
- [S18] Learn: Updating your theme (modified 2024-07-31): https://learn.wordpress.org/lesson/updating-your-theme/
- [S19] Theme SVN import job: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/theme-directory/jobs/class-svn-import.php
- [S20] Theme upload/import class: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/theme-directory/class-wporg-themes-upload.php ; cooldown: https://github.com/WordPress/wordpress.org/blob/trunk/wordpress.org/public_html/wp-content/plugins/theme-directory/theme-directory.php
- [S21] Main Stylesheet (style.css) (last updated 2021-03-01): https://developer.wordpress.org/themes/basics/main-stylesheet-style-css/
- [S22] SVN Book, Subversion in Action (atomic commits): https://svnbook.red-bean.com/en/1.7/svn.basic.in-action.html
- [S23] serf realm construction (`serf__construct_realm`, `"<%s://%s:%d> %s"`): https://github.com/apache/serf/blob/trunk/auth/auth.c
- [S24] Themes Team, Required review requirements (last updated 2026-06-09): https://make.wordpress.org/themes/handbook/review/required/
- [S25] Core `WP_Theme` (`theme_no_index`): https://github.com/WordPress/wordpress-develop/blob/trunk/src/wp-includes/class-wp-theme.php
- [S26] Subversion 1.10.0 CHANGES (`--password-from-stdin`, r1819093): https://svn.apache.org/repos/asf/subversion/tags/1.10.0/CHANGES ; release history: https://subversion.apache.org/docs/release-notes/release-history.html
- [S27] svn client main (`--password-from-stdin requires --non-interactive`, exit codes): https://github.com/apache/subversion/blob/1.14.x/subversion/svn/svn.c
- [S28] Subversion 1.10 release notes: https://subversion.apache.org/docs/release-notes/1.10.html
- [S29] Subversion 1.9 release notes (`svn auth`, `info --show-item`): https://subversion.apache.org/docs/release-notes/1.9.html#svn-auth
- [S30] Default config template (`password-stores`): https://github.com/apache/subversion/blob/1.14.x/subversion/libsvn_subr/config_file.c
- [S31] macOS keychain provider: https://github.com/apache/subversion/blob/1.14.x/subversion/libsvn_subr/macos_keychain.c
- [S32] ra_serf default timeout: https://github.com/apache/subversion/blob/1.14.x/subversion/libsvn_ra_serf/serf.c
- [S33] Subversion error codes: https://github.com/apache/subversion/blob/1.14.x/subversion/include/svn_error_codes.h
- [S34] APR error bases: https://github.com/apache/apr/blob/1.7.x/include/apr_errno.h ; serf error codes: https://github.com/apache/serf/blob/trunk/serf.h
- [S35] SVN Book, svn copy: https://svnbook.red-bean.com/en/1.7/svn.ref.svn.c.copy.html
- [S36] php-src versioning.c: https://github.com/php/php-src/blob/master/ext/standard/versioning.c (PHP-8.3 branch for older numeric behavior: https://github.com/php/php-src/blob/PHP-8.3/ext/standard/versioning.c)
- [S37] Core `wp_update_plugins()`: https://github.com/WordPress/wordpress-develop/blob/trunk/src/wp-includes/update.php
- Secondary (context only): WP Tavern on Release Confirmation, https://wptavern.com/plugin-authors-can-opt-into-email-confirmation-for-wordpress-plugin-updates ; Apple Xcode/Catalina SVN removal summarized via https://developer.apple.com/forums/thread/651031 ; meta ticket #5899 (403 to fetcher): https://meta.trac.wordpress.org/ticket/5899
