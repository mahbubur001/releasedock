# ReleaseDock: Sparkle, GitHub REST API and macOS Security Research

Researched 2026-11-22 for a native SwiftUI macOS 14+ app (Xcode 27 / Swift 6), distributed with Developer ID, **not sandboxed**.
Primary sources: the Sparkle docs (from the raw source of sparkle-project.github.io), the Sparkle `2.x` source tree, docs.github.com (markdown article API), and developer.apple.com (documentation JSON). Context7 `/websites/sparkle-project` was used to cross-check the user-driver API.

"Confirmed facts" were read in a primary source on the research date. "Assumptions" were not verified verbatim, or they are inferences that need a check at implementation time.

---

## 1. Sparkle 2

### 1.1 Version and installation

**Confirmed facts**
- **Latest stable: Sparkle 2.10.1**, released 2026-10-25 and marked "Latest" on GitHub. The newest pre-release is **2.11.0-beta.1** (2026-11-17). The tags in order are 2.11.0-beta.1, 2.10.1, 2.10.0 (2026-09-13), 2.10.0-beta.1, 2.9.6 (2026-08-17).
- 2.10.0 raised Sparkle's minimum deployment target to **macOS 12.0** and dropped CocoaPods. A macOS 14+ app is fine.
- The SPM URL is **`https://github.com/sparkle-project/Sparkle`** (File > Add Packages…). Its `Package.swift` is a `binaryTarget` (an XCFramework zip from GitHub releases) with platform `.macOS(.v12)`.
- The `Package.swift` on the `2.x` branch currently points at `2.11.0-beta.1`. Pin to "Up to Next Major from 2.10.1"; pre-releases are only used if you name them explicitly.
- With SPM, the CLI tools (`generate_keys`, `sign_update`, `generate_appcast`) live in `…/SourcePackages/artifacts/sparkle/Sparkle/bin/`.
- The Swift headers are annotated `NS_SWIFT_UI_ACTOR` on `SPUUpdater`, `SPUStandardUpdaterController`, `SPUStandardUserDriver` and the `SPUUpdaterDelegate` protocol, so in Swift they are `@MainActor`.

### 1.2 SwiftUI integration: the official "Create an Updater in SwiftUI" sample (verbatim)

Source: https://sparkle-project.org/documentation/programmatic-setup/#create-an-updater-in-swiftui

```swift
import SwiftUI
import Sparkle

// This view model class publishes when new updates can be checked by the user
final class CheckForUpdatesViewModel: ObservableObject {
    @Published var canCheckForUpdates = false

    init(updater: SPUUpdater) {
        updater.publisher(for: \.canCheckForUpdates)
            .assign(to: &$canCheckForUpdates)
    }
}

// This is the view for the Check for Updates menu item
// Note this intermediate view is necessary for the disabled state on the menu item to work properly before Monterey.
// See https://stackoverflow.com/questions/68553092/menu-not-updating-swiftui-bug for more info
struct CheckForUpdatesView: View {
    @ObservedObject private var checkForUpdatesViewModel: CheckForUpdatesViewModel
    private let updater: SPUUpdater
    
    init(updater: SPUUpdater) {
        self.updater = updater
        
        // Create our view model for our CheckForUpdatesView
        self.checkForUpdatesViewModel = CheckForUpdatesViewModel(updater: updater)
    }
    
    var body: some View {
        Button("Check for Updates…", action: updater.checkForUpdates)
            .disabled(!checkForUpdatesViewModel.canCheckForUpdates)
    }
}

@main
struct MyApp: App {
    private let updaterController: SPUStandardUpdaterController
    
    init() {
        // If you want to start the updater manually, pass false to startingUpdater and call .startUpdater() later
        // This is where you can also pass an updater delegate if you need one
        updaterController = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil)
    }
    
    var body: some Scene {
        WindowGroup {
        }
        .commands {
            CommandGroup(after: .appInfo) {
                CheckForUpdatesView(updater: updaterController.updater)
            }
        }
    }
}
```

The official SwiftUI settings view, from preferences-ui, uses `updater.automaticallyChecksForUpdates` and `updater.automaticallyDownloadsUpdates`. Its rule is to set these **only when the user changes them**:

```swift
struct UpdaterSettingsView: View {
    private let updater: SPUUpdater
    @State private var automaticallyChecksForUpdates: Bool
    @State private var automaticallyDownloadsUpdates: Bool
    init(updater: SPUUpdater) {
        self.updater = updater
        self.automaticallyChecksForUpdates = updater.automaticallyChecksForUpdates
        self.automaticallyDownloadsUpdates = updater.automaticallyDownloadsUpdates
    }
    var body: some View {
        VStack {
            Toggle("Automatically check for updates", isOn: $automaticallyChecksForUpdates)
                .onChange(of: automaticallyChecksForUpdates) { newValue in
                    updater.automaticallyChecksForUpdates = newValue
                }
            Toggle("Automatically download updates", isOn: $automaticallyDownloadsUpdates)
                .disabled(!automaticallyChecksForUpdates)
                .onChange(of: automaticallyDownloadsUpdates) { newValue in
                    updater.automaticallyDownloadsUpdates = newValue
                }
        }.padding()
    }
}
// In App.body: Settings { UpdaterSettingsView(updater: updaterController.updater) }
```

**Confirmed facts (from the SPUUpdater / SPUStandardUpdaterController headers)**
- `canCheckForUpdates` is read-only and documented as **"KVO-compliant"**. It is suited to menu-item validation. It does not reflect `sessionInProgress`.
- `checkForUpdates()` "must be called on the main thread". `checkForUpdatesInBackground()` does nothing while a session is in progress.
- The init options are `initWithStartingUpdater:updaterDelegate:userDriverDelegate:` and `initWithUpdaterDelegate:userDriverDelegate:`. **The delegates are weakly referenced, so the app must keep them alive.** Calling `init()` is unavailable.
- `startUpdater(_:)` throws if the configuration is invalid, for example a missing or invalid key.
- Do not call `checkForUpdatesInBackground` manually except right after starting the updater, and only when `automaticallyChecksForUpdates == true`. After changing channels or the feed, call `resetUpdateCycleAfterShortDelay()` or `resetUpdateCycle()`.

**Assumptions**
- Under Swift 6 strict concurrency, `CheckForUpdatesViewModel` will likely need `@MainActor`, because `SPUUpdater` is main-actor-isolated. The sample predates Swift 6. On macOS 14+, an `@Observable` model, or KVO via `updater.publisher(for: \.canCheckForUpdates)`, is equivalent. The "pre-Monterey" intermediate-view workaround is harmless but no longer required. Not compiled here.

### 1.3 Info.plist keys

**Confirmed facts** (source: sparkle-project.org/documentation/customization)

| Key | Type / default | Notes |
|---|---|---|
| `SUFeedURL` | String | Appcast URL. Set it in Info.plist even if it is overridden later. |
| `SUPublicEDKey` | String | Base64 EdDSA public key printed by `generate_keys`. |
| `SUEnableAutomaticChecks` | Bool, unset by default | Unset: the user is asked for permission on the **second** launch. `YES`: auto-check without asking. `NO`: off, no prompt. The runtime counterpart is `automaticallyChecksForUpdates`. |
| `SUScheduledCheckInterval` | Number, default `86400` | Seconds; **minimum 1 hour**. |
| `SUAutomaticallyUpdate` | Bool, default NO | Silent background download and install. |
| `SUAllowsAutomaticUpdates`, `SUPromptUserOnFirstLaunch`, `SUShowReleaseNotes`, `SUScheduledImpatientCheckInterval` (2.9+) | optional | |
| `SUVerifyUpdateBeforeExtraction` | Bool, default NO, available since 2.7.3 | Verifies the EdDSA signature **before** extraction. EdDSA is required. Key rotation is still possible through a Developer ID-signed **DMG** only. Required for `.aar` archives and for signed feeds. |
| `SURequireSignedFeed` | Bool, default NO, 2.9+ | Appcast and release notes must be EdDSA-signed (`generate_appcast` signs them). Requires `SUVerifyUpdateBeforeExtraction`. |
| `SURequireFeedBundleIdentifier` | Bool, **2.11 beta only** | Needs `<sparkle:bundleIdentifier>` in the channel. Requires `SURequireSignedFeed`. Not in 2.10.1 stable. |
| `SUEnableInstallerLauncherService` | Bool, default NO | **Sandboxed apps only.** The docs say: "Do not enable this XPC Service if your application is not sandboxed." |
| `SUEnableDownloaderService`, `SUEnableInstallerConnectionService`, `SUEnableInstallerStatusService` | Bool | Sandboxed apps only. "Applications that are not sandboxed should not customize any of these settings." |

- Sparkle compares versions using **`CFBundleVersion`**, which must increment. Use `sparkle:shortVersionString` for the display version.

### 1.4 What happens if `SUPublicEDKey` is missing or invalid

**Confirmed facts** (from `Sparkle/SPUUpdater.m` on the `2.x` branch)
- If the key is **invalid** (present but cannot be decoded), `startUpdater` fails with `SUNoPublicDSAFoundError`: "The EdDSA public key is not valid for …".
- If the key is **missing**:
  - If the feed is not HTTPS **or** the app is not code-signed, the updater fails to start: "For security reasons, updates need to be signed with an EdDSA key for … ".
  - If `SUVerifyUpdateBeforeExtraction = YES`, the updater also fails to start.
  - Otherwise (HTTPS feed and a code-signed app), it **still works on Apple code signing alone**, but logs: "Serving updates without an EdDSA key and only using Apple Code Signing is deprecated and may be unsupported in a future release."
- The update policy (`SUUpdateValidator.m`) **rejects** an update that removes an EdDSA key that the old version had ("Sparkle only supports rotation, but not removal").
- **Decision:** always ship `SUPublicEDKey`.

### 1.5 Code-signature matching and Developer ID requirements for updates

**Confirmed facts** (`SUUpdateValidator.m` and the docs)
- For app-bundle updates, the update is accepted if **either** of these holds:
  - (a) the archive's EdDSA signature verifies against the **old** (installed) app's public key, **or**
  - (b) the new app's Apple code signature is valid **and matches** the installed app's signature (same signing identity / team).
- Allowing one of the two to fail is what makes key rotation possible. You can rotate the Developer ID certificate **or** the EdDSA key in a release, **not both**.
- Hard rejections:
  - an update that drops code signing when the host was signed;
  - an update whose code signature is present but corrupted (even when EdDSA passes);
  - an update that ships a new EdDSA key that does not match the archive signature.
- Sparkle's docs recommend notarizing and Developer ID signing. Apple requires notarization for all Developer ID software built after 2019-06-01 to run on 10.15+ without Gatekeeper blocking. Sparkle 2.6.4+ runs a Gatekeeper scan of signed updates on macOS 14.4+, so users do not see a "Verifying…" dialog on relaunch.
- Supported archive formats are dmg, zip, tar.*, aar (2.7+, needs `SUVerifyUpdateBeforeExtraction`) and pkg.
- Zip archives should be made with `ditto -c -k --sequesterRsrc --keepParent MyApp.app MyApp.zip`, which preserves symlinks.

**Assumptions**
- Sparkle itself does not refuse a non-notarized update. It verifies EdDSA plus the code-signature match. Notarization is a Gatekeeper requirement, so notarize and staple the `.app` before zipping or DMG-ing the update archive.

### 1.6 EdDSA keys and tools

**Confirmed facts**
- `./bin/generate_keys` creates the key pair and stores the private key in the **login Keychain**. It is a generic password with service `https://sparkle-project.org`, default account `ed25519`, labelled "Private key for signing Sparkle updates".
- `generate_keys` prints the `SUPublicEDKey` value.
  - `-p` prints the existing public key.
  - `-x file` exports the private key; `-f file` imports it.
  - `--account` selects a different account name.
- `./bin/sign_update <archive>` outputs `sparkle:edSignature="…" length="…"`.
  - `--ed-key-file -` reads the key from stdin (for CI: `echo "$KEY" | sign_update --ed-key-file - file`).
  - `-s` (key as an argument) is **deprecated and insecure** (since 2.2.2).
- `./bin/generate_appcast /path/to/updates_folder/` signs archives (reading the Keychain), writes the appcast (file name taken from `SUFeedURL`) and builds `.delta` files. `.html` or `.md` files with the same name as an archive become release notes. Useful options:
  - `--download-url-prefix <url>` (for example `https://github.com/OWNER/REPO/releases/download/vX.Y.Z/`)
  - `--release-notes-url-prefix`, `--embed-release-notes`, `--full-release-notes-url`, `--link`
  - `--channel <name>`
  - `--maximum-versions` (default 3), `--maximum-deltas` (default 5)
  - `--critical-update-version`, `--phased-rollout-interval`, `--minimum-update-version`, `--major-version`, `--informational-update-versions`
  - `--ed-key-file`, `--account`, `-o <output>`, `--auto-prune-update-files`
- Keep keys off the hosting server. If the keys are lost, rotation through Developer ID is still possible (see 1.5).

### 1.7 Channels (optional beta) and dynamic feed URL

**Confirmed facts**
- Appcast item: `<sparkle:channel>beta</sparkle:channel>`. Items without the element are on the default channel. Valid channel-name characters are letters, digits, `-`, `_` and `.`.
- Delegate (Swift name as shown in the docs):
  ```swift
  func allowedChannels(for updater: SPUUpdater) -> Set<String> {
      return Set(["beta"])
  }
  ```
  The default channel is always included and cannot be excluded. An empty set means default channel only.
- Channels are meant for branches that later merge back to the default channel (beta, then stable), not for permanently parallel products.
- Dynamic feed delegate (recommended over the deprecated `setFeedURL:`):
  ```swift
  func feedURLString(for updater: SPUUpdater) -> String? {
      return UserDefaults.standard.bool(forKey: "beta") ? BETA_FEED_URL_STRING : STABLE_FEED_URL_STRING
  }
  ```
  Returning `nil` means the Info.plist `SUFeedURL` is used.
- If `setFeedURL:` was ever used, call `clearFeedURLFromUserDefaults()` right after starting the updater (2.4+).
- After toggling beta, call `updater.resetUpdateCycleAfterShortDelay()`.
- `generate_appcast --channel beta` tags newly generated items.

**Assumptions / recommendation**
- Prefer **one appcast plus channels** (a "Receive beta updates" toggle that drives `allowedChannels(for:)`) over two feeds. The delegate object must be retained, for example as a stored property on the App or an `@MainActor` singleton, because Sparkle holds it weakly.

### 1.8 Download progress UI

**Confirmed facts**
- `SPUStandardUpdaterController` uses `SPUStandardUserDriver`. That driver implements the `SPUUserDriver` callbacks:
  - `showUserInitiatedUpdateCheck(cancellation:)` (indeterminate progress window)
  - `showDownloadInitiated(cancellation:)`
  - `showDownloadDidReceiveExpectedContentLength(_:)` / `showDownloadDidReceiveData(ofLength:)` (determinate progress bar)
  - `showDownloadDidStartExtractingUpdate()`, then extraction and install progress
- **No extra code is needed for a download progress UI.** A custom UI means implementing `SPUUserDriver` and using `SPUUpdater(hostBundle:applicationBundle:userDriver:delegate:)` instead of the standard controller.

### 1.9 Sandboxed vs non-sandboxed (ReleaseDock is non-sandboxed)

**Confirmed facts**
- Apple: App Sandbox is required for **Mac App Store** distribution. It is not a requirement for Developer ID distribution or notarization. Notarization requires the Hardened Runtime, a Developer ID certificate, a secure timestamp, and no `get-task-allow`.
- Sparkle: if the app is not sandboxed, skip the sandboxing guide. Do **not** set any `SUEnable*Service` key and do not add the `-spks`/`-spki` mach-lookup exceptions. Optionally strip `Sparkle.framework/Versions/B/XPCServices` with an install-only build phase that re-signs afterwards; the script is on the sandboxing page.
- **A non-sandboxed Developer ID app that launches `/opt/homebrew/bin/svn` and reads arbitrary folders is fully compatible with Sparkle and notarization.**
- New Xcode macOS app templates **enable App Sandbox by default**. Remove the capability, or set `com.apple.security.app-sandbox` to false or remove it.

**Assumptions**
- Non-sandboxed apps are still subject to TCC. Accessing ~/Desktop, ~/Documents, ~/Downloads, iCloud Drive, removable or network volumes, and other apps' containers (macOS 14+) triggers a consent prompt, which is attributed to ReleaseDock even when the `svn` child process does the access. Plan an `NSOpenPanel` folder pick (implicit consent) and handle permission errors gracefully. Full Disk Access is not needed for typical working copies.

### 1.10 Signing of Sparkle's embedded helpers with SPM

**Confirmed facts** (Sparkle sandboxing and setup docs)
- Sparkle ships `Autoupdate`, `Updater.app`, `Installer.xpc` and `Downloader.xpc`, ad-hoc signed with the Hardened Runtime.
- **Code Sign on Copy alone does not re-sign the XPC services and helpers. Xcode's Product > Archive, then Distribute App > Developer ID, does re-sign them.** It preserves the Hardened Runtime and strips `get-task-allow`. That flow is the recommended one, and in CI the equivalent is `xcodebuild archive` + `xcodebuild -exportArchive`.
- For a manual re-sign, sign each helper individually: `codesign -f -s "$ID" -o runtime …/Installer.xpc`, `…/Downloader.xpc --preserve-metadata=entitlements`, `…/Autoupdate`, `…/Updater.app`, then `Sparkle.framework`.
- **Never use `--deep`, and never put it in `OTHER_CODE_SIGN_FLAGS`.**
- For Debug builds with Library Validation, sign with an "Apple Development" certificate rather than ad-hoc, or Sparkle may fail to load.

### 1.11 Hosting the appcast on GitHub

**Confirmed facts**
- The feed must be HTTPS (ATS). Sparkle logs a warning for a non-HTTPS feed and refuses to start without an EdDSA key.
- GitHub release assets must each be under 2 GiB, with at most 1000 assets per release and no bandwidth limit. `https://github.com/OWNER/REPO/releases/latest/download/<asset>` always resolves to the asset in the latest release.
- **Immutable releases** (GitHub setting): once published, the assets and tag cannot be changed or deleted.

**Assumptions / recommendation**
- Host the appcast on **GitHub Pages** (for example `https://OWNER.github.io/releasedock/appcast.xml`), or at a stable raw URL on a branch. Host the archives as **release assets**.
- Generate the appcast with `--download-url-prefix https://github.com/OWNER/REPO/releases/download/vX.Y.Z/`. In practice that means one prefix per release, so many teams run `generate_appcast` per release and merge the result, or post-process the URLs.
- Do not use a mutable `appcast.xml` release asset combined with `releases/latest/download/appcast.xml`. It works, but it breaks with immutable releases and it hides beta-channel items in pre-releases (`/latest` excludes pre-releases).
- Release asset downloads redirect to a GitHub CDN host. `NSURLSession` (Sparkle's downloader) follows the redirect.
- For a **private** repo, Sparkle cannot authenticate to release assets. A Sparkle-updated app must use a public repo, or put the appcast and archives on a public host.

---

## 2. GitHub REST API (releases)

### Confirmed facts
- **`GET /repos/{owner}/{repo}/releases/latest`** returns "the most recent non-prerelease, non-draft release, sorted by the `created_at` attribute". `created_at` is the date of the commit used for the release, not the publish date. `make_latest` (`true`, `false` or `legacy`) on create or update can override which release is "latest", and drafts and pre-releases can never be latest. If there are no published full releases, the endpoint returns **404**.
- **`GET /repos/{owner}/{repo}/releases`** lists releases (`per_page` default 30, max 100; `page`). Draft releases appear only for users with push access. Use this list for a beta channel: filter on `prerelease == true`. Each release has `draft`, `prerelease`, `tag_name`, `published_at`, `assets[]` and `immutable` fields. Paginate using the `Link` header.
- Headers:
  - `Accept: application/vnd.github+json`
  - `X-GitHub-Api-Version: 2026-03-10`, the **current** version, released 2026-03-10.
  - `2022-11-28` remains supported until **2028-03-10**. Requests without the header default to `2022-11-28`. An unsupported version returns `410 Gone`.
  - `Authorization: Bearer <token>` when authenticated.
- Rate limits (primary):
  - Unauthenticated: **60 requests/hour per originating IP**.
  - PAT or user token: **5,000/hour per user**. GitHub Enterprise Cloud org apps: 15,000/hour.
- Rate-limit headers: `x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-used`, `x-ratelimit-reset` (UTC epoch seconds), `x-ratelimit-resource`. `GET /rate_limit` does not count against the primary limit. The headers are authoritative and can vary between regions, so do not depend on an exact remaining count.
- When a limit is exceeded, the API returns **403 or 429**:
  - **Primary limit** (`x-ratelimit-remaining: 0`): wait until `x-ratelimit-reset`.
  - **Secondary limit:** if `retry-after` is present, wait that many seconds. Otherwise, if `x-ratelimit-remaining` is 0, wait until reset. Otherwise wait at least 1 minute, then back off exponentially and give up after N retries.
  - Continuing to make requests while limited can get the integration banned.
  - Secondary limits include at most 100 concurrent requests and 900 points/min for REST (a GET costs 1 point). Make requests serially.
- **Conditional requests:** most endpoints return `etag`, and many return `last-modified`. Send `If-None-Match: <etag>` or `If-Modified-Since`. **A `304 Not Modified` does not count against the primary rate limit only if the request was made "while correctly authorized with an `Authorization` header".** Honour `x-poll-interval` if it is present.
- **Fine-grained PAT, private repo releases:** all `GET /repos/{owner}/{repo}/releases*` endpoints (list, latest, tags, by id, assets, asset by id) are under **Repository permissions > "Contents": read**. GitHub's own example token URL grants `contents:read` plus `metadata:read`; Metadata (read) is the baseline repository permission. Org owners may require approval of fine-grained PATs; until approved, the token reads public resources only. GitHub removes PATs unused for a year and recommends setting an expiration.
- **Asset download:** `GET /repos/{owner}/{repo}/releases/assets/{asset_id}` with `Accept: application/octet-stream` returns the binary. The API either **redirects (302)** or streams it (200), and clients must handle both. `browser_download_url` is meant for browsers. For a private repo, use the API asset URL with a token.

### Assumptions
- On the 302 redirect from an asset download, the target is a pre-signed CDN URL that must **not** receive the `Authorization` header. `URLSession` drops the `Authorization` header on cross-host redirects by default; verify this, or handle `willPerformHTTPRedirection` explicitly.
- Unauthenticated conditional requests that return 304 may still count against the 60/hour limit, based on the docs' wording. Cache the ETag either way and keep polling infrequent (for example at most every 15–60 minutes).
- A 404 on a private repo with a bad or insufficient token is indistinguishable from a missing repo. Surface "repo not found or token lacks Contents: read". The `X-Accepted-GitHub-Permissions` response header names the required permissions.

---

## 3. macOS security practices for ReleaseDock

### 3.1 Keychain (storing the GitHub PAT)

**Confirmed facts**
- Use the **SecItem** API (`SecItemAdd`, `SecItemCopyMatching`, `SecItemUpdate`, `SecItemDelete`) with `kSecClassGenericPassword`, `kSecAttrService` and `kSecAttrAccount`.
- `kSecUseDataProtectionKeychain = true` targets the iOS-style **data protection keychain** on macOS. Apple recommends it for all operations unless you need legacy file-based items, and it is required for `kSecAttrAccessible*` and `kSecAttrAccessGroup` semantics.
- **Entitlement requirement (TN3137):** "macOS builds the list of data protection keychain access groups available to your program from its code signing entitlements… **These entitlements must be authorized by a provisioning profile.**" The data protection keychain is only available in a user login context. The file-based keychain is "on the road to deprecation".
- `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`: readable only while unlocked, and the item does not migrate to a new device or restore.

**Assumptions / recommendation**
- For a Developer ID app, enable the **Keychain Sharing** capability (`keychain-access-groups`) with automatic signing. Xcode then creates a Developer ID provisioning profile that is embedded in the app. Without a profile or entitlement, data protection keychain calls are expected to fail with `errSecMissingEntitlement` (-34018).
- Alternative: if you skip the profile, fall back to the file-based login keychain by omitting `kSecUseDataProtectionKeychain`. It works for non-sandboxed Developer ID apps, but its ACLs are tied to the code signature and it may prompt after updates if the signing identity changes.
- Never log the token, never write it to UserDefaults, and zero transient `Data` copies where practical.

### 3.2 Hardened Runtime, signing and notarization

**Confirmed facts**
- Notarization requires:
  - a valid code signature on **all** executables;
  - a **Developer ID Application** certificate;
  - the **Hardened Runtime**;
  - a secure timestamp;
  - no `com.apple.security.get-task-allow`;
  - well-formed entitlements.
- Entitlements apply only to executables; frameworks inherit the host's entitlements.
- Commands:
  - Store credentials once: `xcrun notarytool store-credentials "notarytool-password" --apple-id … --team-id … --password <app-specific>`. An App Store Connect API key also works.
  - Submit: `xcrun notarytool submit App.zip --keychain-profile "notarytool-password" --wait`.
  - Fetch the log: `xcrun notarytool log <id> --keychain-profile … developer_log.json`.
  - Staple: `xcrun stapler staple "App.app"` (also works on `.dmg` and `.pkg`). **You cannot staple a zip.** Staple the .app, then re-zip it.
  - Zip for submission with `ditto -c -k --keepParent`.
- `altool` is no longer supported (since 2023-11-01). The notary service accepts UDIF dmg, signed flat pkg and zip.

**Assumptions / recommended pipeline**
`xcodebuild archive`, then `-exportArchive` (method `developer-id`), which signs the Sparkle helpers correctly. Then `ditto` zip, `notarytool submit --wait`, `stapler staple ReleaseDock.app`, re-zip or create a DMG (and notarize and staple the DMG too if you ship one), then `generate_appcast`, then upload. The Hardened Runtime does not block launching external binaries such as `/opt/homebrew/bin/svn`, so no runtime-exception entitlements are expected.

### 3.3 Process API safety

**Confirmed facts**
- Foundation `Process`: set `executableURL` and `arguments`, then call `run()` (throws). The strings in `arguments` "don't undergo shell expansion, so you don't need to do special quoting, and shell variables… aren't resolved". Passing arguments this way rather than through `/bin/sh -c` prevents shell injection.

**Assumptions / recommendation**
- Resolve `svn` to an absolute path (`/opt/homebrew/bin/svn` or `/usr/local/bin/svn`) and check `FileManager.isExecutableFile`.
- Pass `--non-interactive`, and put `--` before user-supplied paths or URLs.
- Set an explicit minimal `environment` (PATH, `LANG=en_US.UTF-8`, HOME) and a `currentDirectoryURL`.
- Read stdout and stderr through `Pipe`s concurrently to avoid pipe-buffer deadlocks. Use `terminationHandler` or async wrappers, and add timeouts with `terminate()`.
- Never pass passwords on the command line; argv is visible in `ps`.

### 3.4 OSLog privacy

**Confirmed facts**
- `Logger(subsystem:category:)`. Interpolated strings and objects are **redacted by default** (shown as `<private>` when not debugging). Mark values explicitly with `\(value, privacy: .public)`, `.private` or `.sensitive`.
- `.private(mask: .hash)` (and `auto(mask:)` / `sensitive(mask:)`) logs a hash so that entries can be correlated without revealing the value.
- The legacy `os_log` format equivalents are `%{public}@` and `%{private}@`.

**Recommendation:** tokens are never logged. Mark paths and repo URLs `.private(mask: .hash)`. Mark status codes, rate-limit numbers and version strings `.public`.

### 3.5 UserNotifications in a non-sandboxed app

**Confirmed facts**
- Call `try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge])`. The system prompts only the first time.
- Ask in context, not automatically at first launch. `.provisional` delivers quietly without a prompt.
- Always check `await center.notificationSettings()` (`authorizationStatus`, `alertSetting`) before scheduling, because users can change settings at any time.

**Assumptions**
- No sandbox entitlement is needed for local notifications in a non-sandboxed, Developer ID-signed `.app` bundle with a stable bundle identifier. Use a `UNUserNotificationCenterDelegate` set early (in `applicationDidFinishLaunching` through an `NSApplicationDelegateAdaptor`) so that banners show while the app is frontmost (`willPresent` returns `[.banner, .sound]`).
- Unsigned or ad-hoc Debug builds can behave inconsistently with notification authorization. Test with an Apple Development-signed build.

---

## Sources

Sparkle
- https://sparkle-project.org/documentation/ (raw: github.com/sparkle-project/sparkle-project.github.io/blob/master/documentation/index.md)
- https://sparkle-project.org/documentation/programmatic-setup/ ("Create an Updater in SwiftUI", "API Expectations")
- https://sparkle-project.org/documentation/preferences-ui/
- https://sparkle-project.org/documentation/customization/
- https://sparkle-project.org/documentation/publishing/ (channels, feedURLString, sign_update)
- https://sparkle-project.org/documentation/sandboxing/ (code signing helpers, removing XPC services)
- https://sparkle-project.org/documentation/security-and-reliability/
- https://github.com/sparkle-project/Sparkle/releases (2.10.1 latest stable, 2.11.0-beta.1 pre-release)
- https://github.com/sparkle-project/Sparkle/blob/2.x/Package.swift
- https://github.com/sparkle-project/Sparkle/blob/2.x/Sparkle/SPUUpdater.m, SPUUpdater.h, SPUStandardUpdaterController.h, SPUUpdaterDelegate.h, SUUpdateValidator.m
- https://github.com/sparkle-project/Sparkle/blob/2.x/generate_appcast/main.swift, generate_keys/main.swift, sign_update/main.swift
- Context7 `/websites/sparkle-project` (SPUUserDriver progress callbacks)

GitHub
- https://docs.github.com/en/rest/releases/releases
- https://docs.github.com/en/rest/releases/assets
- https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api
- https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api
- https://docs.github.com/en/rest/about-the-rest-api/api-versions
- https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens
- https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens
- https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases
- https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases
- https://docs.github.com/en/code-security/supply-chain-security/understanding-your-software-supply-chain/immutable-releases

Apple
- https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
- https://developer.apple.com/documentation/security/ksecusedataprotectionkeychain
- https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly
- https://developer.apple.com/documentation/security/storing-keys-in-the-keychain
- https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
- https://developer.apple.com/documentation/security/customizing-the-notarization-workflow
- https://developer.apple.com/documentation/security/hardened-runtime
- https://developer.apple.com/documentation/security/app-sandbox
- https://developer.apple.com/documentation/foundation/process/arguments
- https://developer.apple.com/documentation/os/logger
- https://developer.apple.com/documentation/os/oslogprivacy
- https://developer.apple.com/documentation/usernotifications/asking-permission-to-use-notifications
