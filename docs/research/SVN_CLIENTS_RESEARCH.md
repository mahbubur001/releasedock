# Versions and SmartSVN: What ReleaseDock Can Learn

Research date: 2026-11-25. This document studies two Subversion clients for features and layout ideas ReleaseDock can adopt:
- **Versions³** (versionsapp.com), the Mac-native client.
- **SmartSVN** (syntevo), the cross-platform power-user client.

It describes both apps, compares them, then turns the findings into a prioritized ReleaseDock feature list and a set of layout ideas.

Conventions:
- **Sources.** Every claim carries a source tag: [V#] for Versions, [S#] for SmartSVN. Both lists are at the end.
- **Inferred.** A claim drawn from indirect evidence (absent strings, screenshots, docs that are silent) is marked **inferred**.
- **How Versions was studied.** I read the shipping Versions 3.0.5 (build 3012) app bundle without running it: its Info.plist, Help book, Timeline HTML/CSS, menu nib and UI strings [V6]. I also read the live site, release notes and archived help.
- **How SmartSVN was studied.** I used the official site, the docs.syntevo.com manual, the 14.1 manual source on GitHub, the changelogs and the syntevo blog.
- **Review age.** Most detailed user reviews of both apps date from 2008 to 2015, and recent third-party coverage is thin.

---

## 1. Summary

- **Versions shows how a Mac SVN client should look and feel. SmartSVN shows how much an SVN client can do.** Reviewers praise Versions' calm, Mail-like layout and its Timeline, but call it "a toy" for anything past checkout and commit: it has no merge, no built-in diff and a hidden branch/tag action [V3][V13][V22]. SmartSVN is described as "the least-worst tool" for serious work, but its dense, Windows-style, dockable-pane UI is called "fugly" and "old fashioned" [S61][S62].
- **ReleaseDock is a release tool, not a general client.** The useful ideas are the ones that make *publishing a WordPress.org version* faster, safer or easier to trust. Merging, locks, branches, changelists and blame are out of scope.
- **Strongest ideas to adopt:**
  - a sidebar with live local/remote badges per product [V1];
  - a "Refreshed N minutes ago" freshness line [V6];
  - a day-grouped Timeline/activity feed with unread state [V6][S29];
  - a Releases (tag) browser [S38];
  - a commit/release sheet that remembers messages and shows diffs inline [V2][S24];
  - a transcript of the actual `svn` commands [V8];
  - side-by-side image comparison for `/assets` [S9];
  - designed empty and error states [V6].
- **Already covered in ReleaseDock, often better than both apps:** in-app diff preview before commit, a single verified commit, crash and timeout recovery, Keychain-only credentials, a sparse WordPress.org checkout (`trunk` full, `tags` immediates), automatic `svn:mime-type` on assets, and working-copy update/revert/conflict resolution (0.2.2).

---

## 2. Versions³ (versionsapp.com)

### 2.1 Status

| Item | Finding |
|---|---|
| Owner history | Sofa and Pico (2008) → Sofa (2010) → Black Pixel (2011) → Hypergiant (2018) → Letter Opener GmbH (2021) → **Leitmotif GmbH** (2022), which is the Kaleidoscope team [V2][V9][V11][V12][V26] |
| Awards | Apple Design Award, 2009 [V9][V10] |
| Current version | **3.0.5 (build 3012), 2026-11-24**. 3.0 shipped 2026-10-08, with five releases in seven weeks; the product is under active development again [V2] |
| Requirements | macOS 14+ (15+ recommended), universal binary. Bundles its own Subversion 1.15 and ignores any installed `svn` [V1][V6] |
| Updates and telemetry | Sparkle (`updates.versionsapp.com/v3/prod/appcast`), PostHog analytics [V6] |
| Price | **$149** perpetual, 7-day demo, $59 for students, paid upgrade from v1/v2 [V1][V3][V4] |
| Not supported | **Merging between branches** ("Right now, there's no way to merge changes between branches") [V3], **a built-in diff viewer** (uses Kaleidoscope or another external tool) [V1][V6], Git [V3] |

### 2.2 Features

**Bookmarks (the sidebar)**
- Two bookmark kinds:
  - **Repository bookmark:** a URL plus credentials.
  - **Working-copy bookmark:** a local folder, nested automatically under its repository [V7].
- **Nested repository bookmarks** for sub-folders share one set of credentials, and each shows a Timeline scoped to its folder [V7].
- Organizing bookmarks:
  - folders with Group/Ungroup;
  - "Keep Bookmarks Sorted by Recent Activity" [V2][V6];
  - **coloured bookmarks** (2.2), added after seeing a customer's long list [V2].
- Hosted-service presets (Assembla, Beanstalk, Helix TeamHub) [V6].
- Issue-tracker URL pattern per bookmark [V7][V2].
- Repository bookmarks show **"server can't be reached" and "login failed"** states on the row [V2].
- Changing a repository URL also updates the working copies under it [V2].

**Checkout and working copies**
- Checkout from the repository browser, at any revision ("Browse Other Revision…" ⇧⌘O). The new working copy is bookmarked automatically [V1][V7].
- **Pristines on Demand** (SVN 1.15) can be chosen at checkout, explained in one sentence: "Uses about half the disk space, but reverting and comparing files require network access" [V6].
- Working copies are tracked when moved, and older working-copy formats can be upgraded [V2].

**File browser** (⌘2)
- One outline for both working copies and repositories [V6].
  - Columns: Name, local status, remote status, Base, Last, Date, Author.
  - Optional columns: lock, copy, size, properties.
- Filters:
  - an **All / Changed pill** in the root row (⇧⌘C);
  - "Show Updates from Repository" (⇧⌘U);
  - "Show Ignored" (⇧⌘I) [V6].
- Name filter in the toolbar search field (v3) [V2].
- Quick Look, Open With, Reveal in Finder, and **Open in Terminal** (iTerm supported). It offers to install the bundled `svn` into `/usr/local/bin` [V2][V6].
- **Inspector pane** (⌘I) and Get Info window: properties, lock, copied-from and revision details [V6].

**Status**
- Status kinds: Added, Deleted, Modified, Replaced, Ignored, Missing, Unversioned, Conflicted, Locked, property-modified, External, Incomplete [V6][V7].
- Folders with changes inside are marked [V7].
- The sidebar shows **change-count badges** for local (↑n) and remote (↓n) changes. A preference turns the background remote check off for speed [V6].
- File-system auto-refresh. The toolbar subtitle reads **"Refreshed 9 minutes ago" / "Refresh required"** [V6].

**Commit (v3 sheet)**
- Layout: a "Commit Message:" field with a **per-working-copy message history** menu, and a file table with an Include checkbox, a status icon and the path ("2 of 3 Changed Files") [V1][V6].
- The file context menu offers Open, Open With, **Compare Differences**, Show History and Show Authors. Several files can be compared at once [V2].
- Externals are committed too, one commit per repository [V2].
- The sheet **keeps the message and selection after a failure or Cancel**, and reopens at its last size [V2].
- ⌘Return commits [V2].
- Command titles follow the selection: "Commit All…", "Commit 3 Items…" [V6].
- No changelists and no hunk staging (**inferred**: no such strings or docs).

**Update and diff**
- Update (⌥⌘U), Update to Revision, **Show Remote Changes** (preview what an update brings) [V7][V6].
- Diff runs **only in external tools**: Kaleidoscope by default, plus FileMerge, BBEdit, Beyond Compare and others, or custom scripts [V6][V7].
- Three diff commands:
  - Local Changes (⇧⌘D), which works offline;
  - Remote Changes;
  - Compare Differences (⌘D), which picks any two revisions [V7].
- Failures are explained ("tells you why instead of doing nothing", 3.0.3) [V2].

**History**
- **Timeline** (⌘1): a repository-wide, day-grouped commit feed (see §2.3) [V6].
- **History window** per file or folder [V8][V6]:
  - entry count and revision range controls;
  - a changed-paths list;
  - "Show Source Of Copied Paths".
- Search across author, message, path, revision and date, backed by a **local log cache** that fetches only new revisions [V2].
- **Show Authors** (blame) shows the log message for each revision, with line numbers [V2].
- Issue keys (`#123`, JIRA, FogBugz) and URLs in messages become links [V2][V6].

**Branches, tags and conflicts**
- **No branch/tag UI.** You Option-drag trunk into `branches/` [V7]. Users find this strange: "isn't this a major menu item?" [V15].
- No switch and no merge [V3].
- v3 **Resolve Conflict dialog** [V2][V6]:
  - shows base, theirs and mine, each with its commit;
  - actions: Resolve with Base / Theirs / Mine / As Merged;
  - per-version actions: Open, Compare, Reveal.

**Housekeeping**
- Revert asks for confirmation with a count: "You cannot undo this" [V6].
- Revert to Revision, Cleanup, Cleanup Locks [V6].
- Locks: lock, unlock, lock with message, steal [V6].
- Ignore by name or **by extension** [V6].
- **Transcript:** a log of every SVN operation, per bookmark (⌥⌘0). Reviewers saw it as the trust mechanism: "The developer must also be able to trust the tool by being able to verify what the tool is doing" [V8].

**Authentication and integration**
- Protocols: http(s), svn, svn+ssh, file; plus SASL, proxies and Kerberos (3.0.5) [V1][V2].
- Passwords live in the **iCloud-synced Keychain**. Each bookmark has Reveal / Delete Password [V2][V6].
- Certificate-trust sheet [V2].
- **Finder Sync extension** (v3) [V2][V6]:
  - badges for modified, added, conflicted and unversioned;
  - a "Versions ▸" menu with Update, Commit, Add, Ignore, Revert.
- Notifications ("Commit Complete") [V2].

**Keyboard shortcuts**

| Group | Shortcuts [V6] |
|---|---|
| Views | Timeline ⌘1, File Browser ⌘2, Transcript ⌥⌘0, Inspector ⌘I, Get Info ⌥⌘I, Refresh ⌘R |
| Commit and update | Update ⌥⌘U, Commit ⌥⌘C, Add ⌥⌘A, Revert ⌥⌘R, Mark Resolved ⌥⌘S |
| Diff | Compare ⌘D, Local Changes ⇧⌘D, Remote Changes ⌥⇧⌘D |
| Bookmarks | New Repository Bookmark ⌥⌘N, New Working Copy Bookmark ⌥⇧⌘N |

### 2.3 Layout and visual design

**Window** (v3 hero screenshot [V1])
- A single window with a full-height sidebar in the macOS 26 style. The traffic lights sit in the sidebar.

**Sidebar**
- A "Bookmarks" tree with two kinds of row:
  - repository rows, with a **cloud icon** for hosted repositories and a database icon for others;
  - working-copy rows (a box icon) indented under their repository, e.g. "beanie (trunk)".
- Icons are **tinted with the bookmark colour** (yellow, green, red, orange, blue).
- **Count pills** at the right edge: "↑4", "1↓ ↑13".
- Bottom bar with "+⌄" (new) and "⋯⌄" (actions) menus.

**Toolbar**
- Over the sidebar: Update (cloud ↓), Commit (cloud ↑) and Checkout.
- Over the content, left to right:
  - a segmented **Timeline / File Browser** switch and Refresh;
  - a **title with a subtitle**: "beanie (trunk)" over "Refreshed 9 minutes ago";
  - on the right: Revert, Local Changes / Authors / Compare, Search and Inspector.
- The toolbar is customizable, with optional labels [V6][V2].

**Inline actions** (2.0+) [V5][V1]
- Each changed row carries a coloured capsule with the *next sensible action*:
  - purple **Add** for unversioned;
  - green **Commit** for added;
  - blue **Commit** for modified;
  - red **Commit** for deleted;
  - grey **Delete** for missing;
  - blue **Update** for remote changes.
- This turns the status list into a to-do list.

**Colours**
- The accent is green, `rgb(100,158,21)`.
- Status colours follow the system palette [V6]:
  - added: green `rgb(40,205,65)`
  - deleted: red `rgb(255,59,48)`
  - modified: blue `rgb(0,122,255)`
  - replaced: indigo `rgb(88,86,214)`
  - unversioned: purple

**Timeline** (WebKit template [V6])
- **Sticky, blurred day headers** that collapse (⌥-click collapses or expands all).
- Each commit hangs off a **vertical accent rail with a dot**:
  - a "**r123** by alice, 10:42" line;
  - "Hide/Show N Items" and **Compare All** buttons;
  - the message;
  - the changed paths as small rounded **chips**, each with a coloured A/D/M/R square. Clicking a chip shows its diff.
- URLs and ticket IDs become chips.
- The footer reads "Load More", or "End of Timeline" above a zig-zag "torn paper" edge.

**Empty and error states** [V6]
- A centred icon, a title and one explanatory sentence.
- The states: "No Commits Yet", "Working copy folder is missing", "Folder is not a working copy", "Connection Failed", "Subversion Error".

**Welcome screen** [V2][V6]
- Four large buttons: New Repository Bookmark, New Working Copy Bookmark, Create Local Repository, Documentation.
- The 2.0 notes called it a "calming welcome screen".

**Settings panes** [V6]
- Timeline (number of revisions, show changed files).
- File Browser (inline actions, refresh on change).
- Icon and text size.
- Bookmark badges.
- Compare tool.
- Subversion library.
- Networking.
- Terminal app.
- Finder extension.
- Command-line tool.

### 2.4 What users said

- **Praise**
  - "The workflow in Versions is oriented around the most common cases… does the important and obvious things really well" [V8].
  - "Versions is everything Mac OS X software should be" [V19].
  - "Better UI… more consistent buttons… Beautiful timeline" [V13].
  - "Really intuitive layout", "loads in no time" [V18].
- **Complaints**
  - "Versions is a toy — good for checking out and committing, not so good for branching, merging, rolling back" [V22].
  - "Can't even do merging" [V17].
  - "No built in diff" [V13].
  - "You can't quickly see the changes to a file when committing" [V21]; v3's sheet fixes this.
  - Removing a bookmark "prompts to remove the files… a bit dangerous" [V13].
  - No Finder integration before v3 [V18].
  - Long maintenance gaps (2014–2017, 2024–2026) [V2][V20].

---

## 3. SmartSVN (syntevo)

### 3.1 Status

| Item | Finding |
|---|---|
| Latest | **14.5.1 (2025-10-08)**, adding macOS 26 support. **14.6 preview** (2026-10-14) adds word wrap, refreshed colours and icons, macOS 27, and drops support below macOS 14 [S3][S5][S6][S7][S8] |
| Cadence | About one minor release a year, mostly OS support and diff polish. **Inferred:** in maintenance mode but alive [S9][S12] |
| Runtime | Java with a bundled JRE (Java 21) and the **SWT** native-widget toolkit (since 7.5, "to make SmartSVN look native") [S3][S72] |
| SVN engine | **Official SVN 1.14 libraries through JavaHL**, not SVNKit [S14][S15][S3] |
| Platforms | Windows, macOS (separate Apple Silicon and Intel builds, native since 14.3), Linux [S2][S9] |
| Price | Perpetual per-user license: **$99 / $149 / $198** for 1 / 2 / 3 years of updates, 31-day trial. Free for open source, students and some charities [S10][S1] |
| Editions | The free Foundation edition ended with 14.3 (2023); every commercial user now pays (**inferred**) [S9][S12] |

### 3.2 Features

**Projects and working copies**
- A **project** is a named set of working copies plus settings. The Project Manager groups projects [S53].
- **Check Out wizard** [S54]:
  - pages: URL, Location (a repository tree with Show Revision), Local Directory, Export-only.
  - It **warns when you would check out trunk, tags and branches together**.
- **Sparse working copies** [S60][S56]:
  - "Update More" pulls in subdirectories that aren't checked out yet; "Exclude from Working Copy" is the inverse.
  - Depth levels: only this directory, only file children, immediate children, fully recursive.
- **Project Settings**, with defaults for new projects [S48]:
  - encoding;
  - auto-props;
  - global ignores;
  - binary patterns and MIME;
  - EOL and keywords;
  - lock behaviour.

**Status views**
- About **19 directory states and 20 file states**, plus overlays [S20]:
  - switched;
  - tree conflict;
  - direct and indirect local changes;
  - direct and indirect remote changes.
- Text columns: Local State, Remote State, Copy From, Merge Info, MIME, EOL, Needs Lock [S21].
- Filtering [S20][S23]:
  - wildcard and regex name filters, with saved patterns;
  - state filters: unchanged, unversioned, ignored, change set, remote changed;
  - "Files From Subdirectories" as a flat list.
- An explicit **"2,166 files hidden"** counter [S1 image].
- Double-click depends on the state: compare with HEAD, open, Conflict Solver, or Show Changes [S20].
- **Remote State** shows what an update would change, per file. Repository-only files appear as "Remote" rows [S41].

**Commit wizard**
- A **step list that shows only the pages that apply**: Repositories, Detect Moves, Files, Locks [S24].
- Warns about forgotten files and switched entries [S24].
- Message help [S24][S9]:
  - **History** dropdown and "Get from Log";
  - file-name completion;
  - a template from `tsvn:logtemplate`;
  - spell check;
  - a line-length guide.
- A **Differences** toggle docks a side-by-side diff under the file list [S24]. Reviewers named it SmartSVN's best feature [S62].
- Suggests adds and removes. **Detect Moves** pairs missing and unversioned files by content similarity [S18][S57].
- **Change Sets** (changelists) appear as nodes in the directory tree [S39].
- Client-side pre-commit hook [S24].

**Log, Transactions and Revision Graph**
- **Log** window [S27]:
  - layout: a revisions table with a filter, a Revision Info pane, and the changed directories and files below;
  - "Load Merged Revisions";
  - "Stop for Copied Revisions";
  - **Rollback Revision** (reverse merge);
  - Change Commit Message;
  - export to XML, HTML or text.
- **Log Cache** [S55]:
  - a local cache, with a choice of whole repository, module, or none;
  - Rebuild and Delete.
- **Transactions**: "a direct view into a repository's log which is continuously updated" [S29][S30].
  - Layout: a repository node, then date groups ("Today (2 revisions)", "3 days ago (12 revisions)"), then revisions, which expand to their files.
  - **Unread revisions in blue**, with Mark All as Read.
  - Polls watched URLs at a set interval.
  - The project view marks revisions not yet in the working copy with a green arrow.
- **Revision Graph** [S28]:
  - coloured branch lines in a narrow gutter, with branch-name chips;
  - merge colouring and merge arrows;
  - option to show dead tags and branches.

**Diff and conflicts**
- **File Compare**: two editors joined by curved ribbons, inner-line highlighting, ignore whitespace and case [S32].
- **Changes view** (14.1+) [S9][S7]:
  - **image compare**;
  - unified or side-by-side;
  - syntax colouring (PHP, JS, CSS, Markdown, `.po`…);
  - compact mode that hides unchanged lines;
  - word wrap (14.6).
- **Conflict Solver** [S31]:
  - 3-way: mine / merged / theirs, with an optional base;
  - actions: Take Left, Take Right, Left + Merge, Merge + Right.
- **Annotate** colours lines by age, author or "changes since", and can step through revisions [S44].

**Tags and branches**
- A **tag/branch layout** per project (`trunk`, `branches/*`, `tags/*`) [S36].
- **Add Tag** [S37]:
  - shows a preview of the resulting URL;
  - takes the working copy with or without local changes, **"Only Pristine"**, or a repository revision;
  - **aborts if the tag exists** unless "Replace existing one" is ticked.
- **Copy Within Repository**: a URL-to-URL server-side copy that needs no working copy [S58].
- **Tag Browser** [S38]:
  - a tree of what was copied from what, built from the log cache;
  - columns Name, Revision, **Removed At**; removed tags show a red minus.
- **Merge** [S34][S35]:
  - all revisions (merge tracking), a range, or "Record only";
  - a **Merge Preview** of per-file results before merging.

**Repair, ignore and properties**
- **Clean Up**, **Vacuum** and **Fix** are visible commands [S45][S46][S9]:
  - Fix restores case-changed names, repairs nested roots and reverts added-missing folders;
  - **Fix Line-Endings** rewrites files to one EOL style.
- **Ignore As Pattern** detects common patterns [S51].
- **Edit Properties** with per-property revert [S60].
- MIME type detected on Add [S60].
- An externals editor with a "bump to HEAD" action [S42].

**Integration and credentials**
- Its **own password file** with an optional master password, and **no macOS Keychain** (**inferred**). Forgetting the master password means losing every saved password [S43][S47].
- On macOS, Finder integration is only three Services (Update, Commit, Open), with no badges [S52].
- Find Action (search every menu item), Preferences search, customizable toolbar, menus and shortcuts, dark mode [S9][S23].

### 3.3 Layout and visual design

**Project window** [S1][S19][S18]
- **Title:** "project – branch – SmartSVN".
- **Toolbar:** large labelled icons in three groups:
  - Update, Switch, Merge, Commit;
  - Add, Remove, Move, Revert, Delete, Fix;
  - Changes, Annotate, Log, Graph.
- A **Main / Review perspective switch** at the far right.
- **Panes:**
  - top-left: **Directories** tree. The root reads "svn-1.9.x (1.9.x, 1824509)". Arrow overlays mark local and incoming changes.
  - top-right: **Files** table with a hidden-count, a filter field and state toggles. Modified files get a red icon; remote-changed rows are blue/purple.
  - middle: **Changes**, a side-by-side diff with ribbons and a "+2~5" counter.
  - bottom-left: **Output**, one tree node per command with a ✓ and per-file children, plus context actions (Show Changes, Copy Revision…).
  - bottom-right: **Transactions** feed.
  - status bar: progress, selection, refresh state, an online/offline "jack" icon, notifications.
- **Dockable views**, and two editable **perspectives**: Main (overview) and **Review** (big diff for pre-commit review) [S19].

**Commit wizard** [S18]
- A left **Steps** list.
- A header sentence: "Review the entries and provide the commit message. Only the selected entries will be committed."
- A checkbox table, the message field, and **Differences** and **History** buttons.
- Back / Next / Commit / Cancel.

**Other windows** [S18][S28][S31][S33][S38]
- **Revision Graph:** a revisions table whose message column carries a thin graph gutter (red trunk line, green branch line) and green **branch-name chips**.
- **Conflict Solver:** three editors with pink conflict blocks and ribbons into the centre file.
- **Repository Browser:** a tree whose nodes show the last-changed revision in grey ("1.9.x (1824405)"); a table with Last Rev., Size, Changed At and Changed By.
- **Tag Browser:** branches on the left; a tags table on the right with **yellow tag icons**.

**Preferences** [S43][S48][S47]
- A searchable tree.
- Two tiers: global settings, and per-project settings with defaults.
- Stored as YAML; deleting `ui-state.yml` resets the layout.

### 3.4 What users said

- **Praise**
  - "SmartSVN had a much smarter recursive commit mode… a separate diff panel that requires only a single click" [S62].
  - "SmartSVN is the least-worst tool available… Versions is a toy" [S61].
  - Same interface on every OS [S63].
  - Tags and branches "as conveniently as if they were native Subversion features" [S74].
  - The conflict solver [S74].
- **Complaints**
  - "Pretty fugly" [S61]; "a more old fashioned UI with simple list like tables" [S62]; written in Java [S61].
  - Needs a separate window per repository (a discoverability issue: a project can hold several working copies) [S62].
  - The log is "just a table with all log messages" [S62].
  - Slow refresh on large working copies ("Local Refreshing… can never end", 30 s scans) [S64][S65][S66].
  - Price and licensing changes (MacUpdate value rating 2/5) [S61].
  - Overwhelming for beginners [S75c].
  - The master-password store [S43].

---

## 4. Side-by-side

| Area | Versions³ | SmartSVN | ReleaseDock today |
|---|---|---|---|
| Look and feel | Mac-native, sidebar, calm | Dense, multi-pane, Windows-style | Mac-native, sidebar, cards |
| Organizing many working copies | Bookmarks, folders, colours, activity sort | Projects and Project Manager | Products list (one working copy each) |
| Status at a glance | ↑/↓ count badges per bookmark; "Refreshed N ago" | Hidden-count; overlays; text state columns | Working copy card on the product page (0.2.2) |
| Incoming changes | Show Remote Changes; ↓ badge | Remote State; **Transactions feed with unread** | Freshness check during a release |
| Commit | Sheet, message history, include checkboxes, compare from context menu | Step wizard, **inline Differences**, Detect Moves, templates | Release wizard with full in-app diff, one commit, verification |
| Diff | External only | Built in, **image compare**, syntax colour | Built in (text) |
| History | **Timeline** (day-grouped feed, chips) | Log table, Transactions, Revision Graph | Release History (ReleaseDock's own releases) |
| Tags | Option-drag (hidden) | **Add Tag with an exists-guard, Tag Browser** | `svn cp trunk tags/<v>` in the release; exists check |
| Conflicts | 3-version dialog (v3) | 3-way Conflict Solver | Per-file mine/theirs sheet (0.2.2) |
| Trust and audit | **Transcript** of SVN operations | Output pane, one node per command | Release manifest + timeline; `svn` output not shown |
| Credentials | Keychain (iCloud), reveal/delete per bookmark | Own password file + master password | Keychain only, passed via stdin |
| System integration | Finder Sync badges and menu, Terminal, Quick Look | Services only on Mac | Open With → ReleaseDock |
| Repair | Cleanup | Clean Up, Vacuum, **Fix** | Recovery after an interrupted commit; Revert with backup |

---

## 5. Feature list for ReleaseDock

Prioritized for a WordPress.org release tool:
- **P1:** high value, fits the current design.
- **P2:** valuable next.
- **P3:** later or optional.

Effort scale: S = days, M = about a week, L = several weeks.

### P1: next releases

| # | Feature | Inspired by | Why it matters for WordPress.org | Effort |
|---|---|---|---|---|
| 1 | **Sidebar product rows with live badges**: per-product icon (already), "↑n local" / "↓n remote" pills, and a red state for unreachable or login-failed | [V1][V2][V6] | With 7–30 plugins, you see at a glance which ones have unreleased trunk work or someone else's commits | M |
| 2 | **"Refreshed N minutes ago" subtitle** on product and release screens, with **Refresh required** when stale | [V6] | Release decisions made on stale SVN state are dangerous; the user sees how fresh the state is | S |
| 3 | **Activity feed per product** (Transactions/Timeline): commits to `trunk`, `tags` and `assets` grouped by day, **unread in blue**, "Mark All Read", polled on an interval; optional notification for commits made outside ReleaseDock | [S29][S30][V6] | Shows commits by co-maintainers, the plugin review team or another machine before you release over them | M |
| 4 | **Releases (tag) browser**: every `tags/<version>` with date, revision, author and size; the current **Stable tag** highlighted; actions Compare with trunk, Compare with previous, Restore Older Version (exists), Open in Finder/Trac | [S38][V2] | WordPress.org history *is* the tag list. It also gives Restore Older Version a natural home | M |
| 5 | **SVN transcript**: every `svn` command ReleaseDock ran (arguments with secrets redacted, exit code, duration, last lines of output) per release and per product, copyable | [V8][S18][S19] | Reviewers called it the trust mechanism. It also speeds up support ("what exactly happened at 14:02?") | S |
| 6 | **Image compare for `/assets`**: old and new banners, icons and screenshots side by side, with pixel size and file size, in the review step | [S9] | Asset updates are visual; a wrong banner ships to the whole directory page | M |
| 7 | **Release message history and templates**: recent commit messages per product, and tokens such as `Release {version}`, `Tag {version}`, `Readme: Tested up to {wp}` | [V2][S24] | Consistent messages; fewer typos in the permanent public log | S |
| 8 | **Designed empty and error states**: "Working copy folder is missing", "Folder is not a working copy", "Can't reach plugins.svn.wordpress.org", "SVN password rejected", "No releases yet", each with one fix button | [V6] | Removes the blank-pane confusion; each state says what to do next | S |
| 9 | **Remember sheet state**: keep the typed message, selection and sheet size after Cancel or a failed commit | [V2] | Losing a written changelog or message after a timeout is the most annoying failure | S |

### P2: valuable next

| # | Feature | Inspired by | Why it matters | Effort |
|---|---|---|---|---|
| 10 | **Inline next-action capsules** on product and file rows ("Release 1.4.2", "Update working copy", "Resolve 2 conflicts", "Sync assets"), coloured by kind | [V5][V1] | Turns status into a to-do list; one click to the right flow | M |
| 11 | **Working copy file browser** per product: trunk/tags/assets tree with local and remote status columns, an All / Changed toggle, a name filter, an "N files hidden" count, Quick Look, Reveal, Open in Terminal | [V6][S20][S41] | Inspect exactly what's in trunk without leaving the app or opening Terminal | L |
| 12 | **Incoming-changes preview** ("Show Remote Changes") before Update, per file: Modified, Added, Removed | [V7][S41] | Know what an update will overwrite before running it | S |
| 13 | **Local log cache per plugin path**, with search across message, author, path, revision and date (never the whole shared repository) | [V2][S55] | `plugins.svn.wordpress.org` is one giant repository; cached, scoped logs make the feed and tag browser instant | M |
| 14 | **Review mode**: a large diff layout for the final check, with syntax colouring for PHP, JS, CSS, Markdown and `.po`, word wrap, and hide-unchanged-lines | [S19][S9][S7] | The review step is where mistakes are caught; a focused layout helps | M |
| 15 | **Build-output rename detection**: pair missing and unversioned files by similarity and record them as `svn mv` so history follows renamed files | [S57][S18] | Release builds often rename or move files; history stays readable on Trac | M |
| 16 | **Commands titled by scope**: "Update radius-booking", "Revert 3 Files…", "Release 1.0.25…" | [V6] | The scope is clear before a destructive or public action | S |
| 17 | **Safer destructive dialogs**: counts plus "You cannot undo this" on revert; removing a product asks "Keep Working Copy / Move to Trash", **default Keep** | [V6][V13] | Mirrors the praise and the criticism; protects local work | S |
| 18 | **Credential management per account**: show which WordPress.org SVN user each product uses; Reveal / Replace / Delete password; test login | [V2][V6] | WordPress.org uses separate SVN passwords that users rotate; today this is hidden | S |
| 19 | **Repair actions surfaced**: Clean Up and Vacuum (SVN 1.14+) on the product page, with one-line explanations; auto-offer after an interrupted command | [S45][S46][V6] | Locked or interrupted working copies otherwise fail with cryptic E155004 errors | S |

### P3: later or optional

| # | Feature | Inspired by | Notes |
|---|---|---|---|
| 20 | **Finder Sync extension**: badges on development and working-copy folders, and a "ReleaseDock ▸ Release This Folder / Show Product" menu | [V2][V6] | Nice, but needs an app extension and a sandboxed helper |
| 21 | **Coloured product groups / folders** in the sidebar, sorted by recent activity | [V2][V7] | Useful past about 15 products |
| 22 | **Blame ("Show Authors")** on a trunk file, with the commit message per line | [V2][S44] | For "which release introduced this line?" |
| 23 | **Pristines on Demand** option for new checkouts (needs SVN 1.15), explained in one sentence | [V6] | Halves working-copy disk use for large plugins |
| 24 | **Commit-time warnings for forgotten files** (unversioned files in trunk not covered by `.distignore`) | [S24] | Partly covered by inspection already |
| 25 | **Issue and URL linkification** in messages and changelogs (GitHub issues, WordPress.org support threads) | [V2][S49] | Small polish for the activity feed |
| 26 | **Find Action / command palette** | [S9] | Power users; low priority with a small command set |

### Deliberately not planned

- Branches, merge, switch, locks, changelists, externals editing, Revision Graph. WordPress.org doesn't use branches ("no longer created by default"), and none of these help publish a version.
- A custom password store. Keep the macOS Keychain only; SmartSVN's master-password file is a known pain [S43].
- Dockable multi-pane perspectives and a big labelled toolbar. These are the source of SmartSVN's "old fashioned" criticism [S61][S62].

---

## 6. Layout and design ideas

1. **Sidebar** (from Versions)
   - Keep one window with a full-height sidebar.
   - Product rows: WordPress.org icon, name, a small grey subtitle (slug or current version), and right-aligned count pills (↑ local, ↓ remote, ⚠ problem).
   - Optional user colour per product or group.
   - A bottom bar with **+** (Add Product) and the existing gear.
2. **Toolbar title + subtitle**
   - Product name over "Refreshed 3 minutes ago · r3822482".
   - Right side: Update, Release, Activity/Files switch, Search, Inspector.
   - Use macOS 26 grouped toolbar items.
3. **Two content modes per product**, like Versions' Timeline / File Browser switch
   - **Activity**: the day-grouped feed.
   - **Files**: the working-copy browser.
   - The existing product overview stays as the default.
4. **Timeline styling** (Versions)
   - Sticky blurred day headers.
   - A thin accent rail with a dot per commit.
   - An "r123 by alice · 10:42" line, then the message.
   - Changed paths as chips with A/M/D squares in system green/blue/red; clicking one opens its diff.
   - Release commits get a tag chip ("tags/1.0.24").
   - Footer: "Load More" or "End of history".
5. **Status colours:** use system colours for added, modified, deleted, replaced and unversioned [V6] together with text labels [S20], so status isn't conveyed by colour alone.
6. **Release sheet** (both apps)
   - A left step list that hides steps that don't apply [S24]. Review → Message → Publish is already close to this.
   - An inline diff under the file list [S24][S62].
   - A message history menu [V2].
   - A sheet that remembers its size and contents [V2].
7. **Output / receipt as a tree** (SmartSVN)
   - Each operation is a row with ✓ or ✗, its duration and a summary ("Committed revision 3822482 · 14 files").
   - It expands to the files and the raw transcript.
   - Context actions: Copy Revision, Open in Trac, Show Diff.
8. **Empty states** (Versions): a centred icon, a title, one sentence and one primary button, used consistently across Overview, Products, History, Activity and Files.
9. **Inspector pane** (⌘I) for the selected file or release: revision, author, date, size, MIME, properties, and the SHA-256 from verification.
10. **Keyboard shortcuts** modelled on Versions:

    | Action | Shortcut |
    |---|---|
    | Activity / Files | ⌘1 / ⌘2 (move the sidebar shortcuts to ⌃⌘1…) |
    | Refresh | ⌘R |
    | Update working copy | ⌥⌘U |
    | New Release | ⌘N (exists) |
    | Compare | ⌘D |
    | Inspector | ⌘I |
    | Transcript | ⌥⌘0 |

---

## 7. Pitfalls to avoid

- **Whole-repository log caching.** WordPress.org is one shared repository with millions of revisions. Always scope `svn log` to `/<slug>/…`, and use `--stop-on-copy` or limits ([S55], and ReleaseDock's own WORDPRESS_SVN_RESEARCH.md §1.1).
- **Full rescans of large working copies on every open** [S64][S65][S66]. Scope status to `trunk`, the latest tag and `assets`, and use FSEvents for local changes.
- **Hiding the important action.** Versions' Option-drag tagging was criticised [V15]. In ReleaseDock, tagging must remain the visible result of a release.
- **Deleting user files as a side effect** of removing a bookmark or product [V13].
- **External-only diff** [V13]. Keep the in-app diff; optionally add "Open in Kaleidoscope/FileMerge/BBEdit".
- **Feature sprawl.** SmartSVN's 14 menus and dense toolbar were criticised as overwhelming [S75c]. Add features only where they help release work.

---

## Sources

### Versions
- [V1] Versions home page (live, Nov 2026): https://www.versionsapp.com/. Screenshots at https://www.versionsapp.com/media/images30/MacBookAir.jpg, …/images30/New/CommitSheet.jpg, …/New/FinderExtension.jpg, …/New/Search.jpg
- [V2] Release notes, 1.0b2 (2008) to 3.0.5 (2026-11-24): https://www.versionsapp.com/releaseNotes
- [V3] Support / FAQ: https://www.versionsapp.com/support
- [V4] Buy and upgrade pages: https://www.versionsapp.com/buy, https://www.versionsapp.com/vs2-to-vs3
- [V5] Wayback, 2024 homepage (Versions², $119): https://web.archive.org/web/2024/https://www.versionsapp.com/. Screenshots at https://www.versionsapp.com/media/images20/New/InlineActions.jpg, …/images20/New/UI.jpg
- [V6] Versions 3.0.5 (3012) app bundle, inspected read-only: Info.plist, Help book, Welcome and Timeline HTML/CSS, MainMenu.nib, Preferences.nib, UI strings, Finder Sync extension. Source: https://updates.versionsapp.com/v3/prod/Versions-3.0.5-3012.zip
- [V7] Archived online help, 2008–2010, e.g. https://web.archive.org/web/2010/http://versionsapp.com/documentation/versions_wf_bookmarks.html (also _browse_wc, _icons, _updrev, _repos, _props, _commit, _timeline, _transcript, _comp_*, _preferences, versions_faq_branches, _global-ignore)
- [V8] Scott Stevenson, "Preview of Versions SVN Client", Theocacao (2008-05-31): http://theocacao.com/document.page/576
- [V9] Macworld, "Black Pixel acquires Sofa's Versions, Kaleidoscope" (2011): https://www.macworld.com/article/214856/black_pixel_acquires_sofas_versions_kaleidoscope.html
- [V10] Macworld, 2009 Apple Design Awards: https://www.macworld.com/article/198538/apple_design_awards-2.html
- [V11] PRWeb, Hypergiant acquires Black Pixel (2018): https://www.prweb.com/releases/hypergiants_space_age_solutions_division_acquires_black_pixel_to_continue_industry_domination_in_ai_powered_software/prweb15995498.htm
- [V12] Kaleidoscope blog, Leitmotif (2022): https://blog.kaleidoscope.app/2022/06/02/leitmotif/
- [V13] James Low, "Cornerstone vs. Versions vs. SmartSVN" (2012): https://jameslow.com/2012/03/14/cornerstone-vs-versions-vs-smartsvn/
- [V14] qreate, "Five Subversion Clients for Mac OSX" (2009): https://qreate.co.uk/2009/04/08/five-subversion-clients-for-mac-osx/
- [V15] MacRumors, "Versions SVN Client for Mac" (2008): https://forums.macrumors.com/threads/versions-svn-client-for-mac.506102/
- [V16] MacRumors, "Version and Cornerstone": https://forums.macrumors.com/threads/version-and-cornerstone.688013/
- [V17] MacRumors, Tortoise-like SVN thread (2011): https://forums.macrumors.com/threads/anything-similar-to-tortoise-svn-for-mac-that-will-work-with-mac-and-windows.1144121/
- [V18] Aral Balkan, "The best Mac SVN client": https://ar.al/940/
- [V19] PokerCopilot blog (2008): https://blog.pokercopilot.com/2008/11/versions-top-notch-subversion-client
- [V20] If Not Nil forum, "SVN clients" (2020): https://ifnotnil.com/t/svn-clients/551
- [V21] AlternativeTo, Versions reviews: https://alternativeto.net/software/versions/about/
- [V22] MacUpdate, SmartSVN reviews (2013 comment on Versions): https://smartsvn.macupdate.com/
- [V23] infotexture (2007): https://infotexture.net/2007/06/versions-new-mac-os-x-subversion-client/
- [V24] Colin Devroe (2008): https://cdevroe.com/2008/11/18/versions-the-subversion-client-for-mac-goes/
- [V25] CounsellingResource review (v1.0.6; search snippet only): https://counsellingresource.com/lib/versions/
- [V26] Michael Tsai, "Acquiring Kaleidoscope": https://mjtsai.com/blog/2021/10/28/acquiring-kaleidoscope/

### SmartSVN
- [S1] https://www.smartsvn.com/
- [S2] https://www.smartsvn.com/download/
- [S3] https://www.smartsvn.com/documents/smartsvn/changelog.txt
- [S4] http://blog.syntevo.com/smartsvn/2025/03/06/smartsvn-14-5-released.html
- [S5] http://blog.syntevo.com/smartsvn/2025/10/08/smartsvn-14-5-1.html
- [S6] https://blog.syntevo.com/smartsvn/2026/10/15/smartsvn-14-6-preview.html
- [S7] https://www.smartsvn.com/documents/smartsvn/changelog-14.6.txt
- [S8] https://www.smartsvn.com/preview/
- [S9] https://www.smartsvn.com/whats-new/
- [S10] https://www.smartsvn.com/purchase/
- [S11] http://blog.syntevo.com/smartsvn/2023/04/12/smartsvn-14-3-released.html
- [S12] http://blog.syntevo.com/smartsvn/2020/10/20/smartsvn-14-released.html
- [S14] https://svn.haxx.se/users/archive-2016-01/0075.shtml
- [S15] https://blog.syntevo.com/smartgit/2025/11/10/20-years-syntevo.html
- [S16] http://blog.syntevo.com/smartsvn/2012/09/18/smartsvn-sold-to-wandisco.html
- [S17] https://www.smartsvn.com/documents/smartsvn/changelog-14.txt
- [S18] https://www.smartsvn.com/features/
- [S19] https://docs.syntevo.com/SmartSVN/Latest/Manual/Project-Window
- [S20] https://docs.syntevo.com/SmartSVN/Latest/Manual/Directory-Tree-and-File-Table
- [S21] https://docs.syntevo.com/SmartSVN/Latest/Manual/File-Attributes
- [S22] https://docs.syntevo.com/SmartSVN/Latest/Manual/Refresh
- [S23] https://docs.syntevo.com/SmartSVN/Latest/Manual/Menus
- [S24] https://docs.syntevo.com/SmartSVN/Latest/Manual/Commit
- [S25] https://docs.syntevo.com/SmartSVN/Latest/Manual/Update
- [S26] https://docs.syntevo.com/SmartSVN/Latest/Manual/Switch
- [S27] https://docs.syntevo.com/SmartSVN/Latest/Manual/Log
- [S28] https://docs.syntevo.com/SmartSVN/Latest/Manual/Revision-Graph
- [S29] https://docs.syntevo.com/SmartSVN/Latest/Manual/Transactions-Frame
- [S30] https://docs.syntevo.com/SmartSVN/Latest/Manual/Project-Transactions
- [S31] https://docs.syntevo.com/SmartSVN/Latest/Manual/Conflict-Solver
- [S32] https://docs.syntevo.com/SmartSVN/Latest/Manual/File-Compare
- [S33] https://docs.syntevo.com/SmartSVN/Latest/Manual/Repository-Browser
- [S34] https://docs.syntevo.com/SmartSVN/Latest/Manual/Merge
- [S35] https://docs.syntevo.com/SmartSVN/Latest/Manual/Merge-Preview
- [S36] https://docs.syntevo.com/SmartSVN/Latest/Manual/Tags-and-Branches
- [S37] https://docs.syntevo.com/SmartSVN/Latest/Manual/Add-Tag
- [S38] https://docs.syntevo.com/SmartSVN/Latest/Manual/Tag-Browser
- [S39] https://docs.syntevo.com/SmartSVN/Latest/Manual/Change-Sets
- [S40] https://docs.syntevo.com/SmartSVN/Latest/Manual/Locks
- [S41] https://docs.syntevo.com/SmartSVN/Latest/Manual/Remote-State
- [S42] https://docs.syntevo.com/SmartSVN/Latest/Manual/Externals
- [S43] https://docs.syntevo.com/SmartSVN/Latest/Manual/Preferences
- [S44] https://docs.syntevo.com/SmartSVN/Latest/Manual/Annotate
- [S45] https://docs.syntevo.com/SmartSVN/Latest/Manual/Clean-Up
- [S46] https://docs.syntevo.com/SmartSVN/Latest/Manual/Fix
- [S47] https://docs.syntevo.com/SmartSVN/Latest/Manual/Installation-and-Files
- [S48] https://docs.syntevo.com/SmartSVN/Latest/Manual/Project-Settings
- [S49] https://docs.syntevo.com/SmartSVN/Latest/Manual/Bugtraq-Properties
- [S50] https://docs.syntevo.com/SmartSVN/Latest/Manual/JIRA
- [S51] https://docs.syntevo.com/SmartSVN/Latest/Manual/Ignore-Patterns
- [S52] https://docs.syntevo.com/SmartSVN/Latest/Manual/Shell-Integration
- [S53] https://docs.syntevo.com/SmartSVN/Latest/Manual/Project-Manager
- [S54] https://docs.syntevo.com/SmartSVN/Latest/Manual/Check-Out
- [S55] https://docs.syntevo.com/SmartSVN/Latest/Manual/Log-Cache
- [S56] https://github.com/syntevo/syntevo.github.io/blob/master/SmartSVN/14.1/Common-Features.md
- [S57] https://github.com/syntevo/syntevo.github.io/blob/master/SmartSVN/14.1/Detect-Moves.md
- [S58] https://docs.syntevo.com/SmartSVN/Latest/Manual/Copy-Within-Repository
- [S59] https://docs.syntevo.com/SmartSVN/Latest/Manual/Revision-Compare
- [S60] syntevo.github.io SmartSVN/14.1 manual pages (Mark-Resolved, Revert, Remove, Add, Update-More, Exclude-from-Working-Copy, Edit-Properties, Add-Branch): https://github.com/syntevo/syntevo.github.io/tree/master/SmartSVN/14.1
- [S61] MacUpdate reviews: https://smartsvn.macupdate.com/
- [S62] James Low, "Cornerstone vs. Versions vs. SmartSVN" (2012): https://jameslow.com/2012/03/14/cornerstone-vs-versions-vs-smartsvn/
- [S63] Hacker News: https://news.ycombinator.com/item?id=999691
- [S64] https://andreypanasyuk.wordpress.com/2013/10/25/downgrading-from-smartsvn-7-6/
- [S65] https://smartsvn7.rssing.com/chan-5859332/article65.html
- [S66] https://groups.google.com/d/topic/smartsvn/3sXmsXEtdx4
- [S67] https://ifnotnil.com/t/svn-clients/551
- [S72] http://blog.syntevo.com/page24/ (7.5 preview: SWT instead of Swing)
- [S73] https://www.qbssoftware.com/neur/product/smartsvn/
- [S74] https://www.softpedia.com/get/Programming/Other-Programming-Files/SmartSVN.shtml
- [S75c] https://macdownload.informer.com/smartsvn/
