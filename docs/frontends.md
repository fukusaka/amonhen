# Front-end features

[日本語](ja/frontends.md)

Which user-visible features each front end has: the macOS app (`apps/macos`), the GTK4 app (`apps/gtk4`), the VSCode extension (`apps/vscode`) and the CLI (`crates/cli`). The rows follow the chapters of [docs/ux.md](ux.md), which describes the features (mostly as the macOS app has them), plus History, the sidebar, Blame / File History, the status bar, confirmations and SSH. It was checked against the code, not the design.

Marks:

- ✓ done, possibly in that front end's own form (a tooltip instead of a popover, an input box instead of a sheet); the Notes column says how.
- △ partly there; the Notes column says what's missing.
- — not there.
- n/a doesn't apply to that front end (the CLI has no sidebar), or is left to the platform on purpose (VSCode's own settings, keybindings, Output channel and merge editor); the Notes column says what does it instead.

For the CLI, △ "via `amonhen run`" means the core's command exists and the CLI reaches it only as JSON (`amonhen run '{"op":…}'`), with no subcommand of its own.

The menu commands themselves are not listed one by one: every front end takes them from the shared table in `crates/commands`, and the GTK4 app's `PENDING` list in `apps/gtk4/src/appmenu.rs` names the ones it doesn't have yet (none at the moment).

This table is kept by hand. Update it in the same change that adds, removes or changes a feature in any front end.

## Platform differences between the macOS and GTK4 apps

The GTK4 app aims to behave like the macOS app. These are the only intended differences; each follows the platform, mostly the [GNOME HIG](https://developer.gnome.org/hig/):

- The app quits when its last window closes.
- No import of the login shell's environment (Linux sessions already carry it).
- On Linux and the BSDs, fsmonitor uses Watchman when installed, with git 2.26 or later (git has no fsmonitor--daemon there).
- No Apple Intelligence; Claude Code and Codex only.
- Files (the desktop's file manager) instead of Finder; external editors are found by their desktop entries.
- The header bar shows icons only, with tooltips; toolbar customization has no icon/text choice.
- History search is GNOME's search bar: a toggle in the header bar and Ctrl+F reveal a bar above History.
- Destructive confirmations: Cancel comes first and takes Return and Esc, and the destructive button is red; Cancel isn't highlighted.
- Keyboard shortcuts that GNOME takes are moved or left out (see Keyboard shortcuts below).
- “This Computer”, the package manager and systemd/cron in place of “This Mac”, Homebrew and launchd.

## Overall layout

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Workspace rows switch the main view (Working Copy, History, Stashes, Reflog…) | ✓ | ✓ | △ | n/a | VSCode: Working Copy, History, Stashes and (with the engine) Reflog; no Conflicts. |
| Conflicts row with a count while there are conflicts | ✓ | ✓ | ✓ | n/a | VSCode: A Conflicts group inside the Working Copy view. |
| Stashes row with a count only when there are stashes | ✓ | ✓ | ✓ | n/a | VSCode: The Stashes section. |
| Range Diff row while a range diff is open | ✓ | ✓ | n/a | n/a | VSCode: A range diff opens as an editor tab. |
| Filter field over Branches, Remotes and Tags | ✓ | ✓ | ✓ | n/a | VSCode: The filter button on the sidebar's title asks for the words; the matches are listed flat. |
| Several branches or tags selected at once | ✓ | ✓ | ✓ | n/a | VSCode: Merge into Current Branch, Delete Branch, favorites and Archive act on all of them. |
| Pull Requests and Worktrees shown only when not empty | ✓ | ✓ | △ | n/a | VSCode: Worktrees only (GitHub is left out). |

## Sidebar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Branches arranged (current, active, inactive, merged, archived, stacked) | ✓ | ✓ | ✓ | △ | VSCode: With the engine; without it, a folder tree split on `/`.<br>CLI: Via `amonhen run` (`branchGroups`). |
| Favorite branches | ✓ | ✓ | ✓ | — | |
| Archived branches | ✓ | ✓ | ✓ | — | |
| Clean Up Branches (merged, merged by a pull request, upstream gone; one Undo) | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`branchCleanup`, `deleteBranches`); merged pull requests are read by the apps with gh. |
| Merged-by-pull-request mark on a branch (squash or rebase on GitHub) | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`branchGroups` with `pullRequests`); the apps read merged pull requests with gh. |
| Favorites reordered by dragging | ✓ | ✓ | ✓ | n/a | VSCode: Move Up and Move Down in Favorites on the menu (a drop on a branch offers Merge and Rebase). |
| Branch menu: checkout, rename, delete (Delete Anyway when unmerged) | ✓ | ✓ | ✓ | △ | macOS: Set Upstream is only in the Repository menu.<br>GTK4: Set Upstream is only in the Repository menu.<br>CLI: Via `amonhen run`. |
| Sync a branch that is not checked out (Fast-Forward, Push, Publish) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Tags listed | ✓ | ✓ | ✓ | ✓ |  |
| Tag marks for remotes (missing, different) with a popover | ✓ | ✓ | ✓ | △ | VSCode: In the row's description, each remote's state in the tooltip (no popover).<br>CLI: Via `amonhen run` (`tagRemotes`). |
| Tag signature marks | ✓ | ✓ | ✓ | ✓ | VSCode: The row's icon, explained in the tooltip.<br>CLI: `refs --verify`. |
| Tag menu: Push to, Replace on, Delete, Delete from a remote | ✓ | ✓ | ✓ | △ | VSCode: The remote is chosen in a list when there are several.<br>CLI: Via `amonhen run`. |
| Remotes with their branches | ✓ | ✓ | ✓ | ✓ |  |
| Remote menu: Add, Rename, Edit URL, Remove, Prune | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Pull Requests group | ✓ | ✓ | — | — |  |
| Submodules group (Update, Open in New Window) | ✓ | ✓ | ✓ | ✓ | CLI: `submodule`, `submodule update`. |
| Submodule marks (not checked out, another commit, changes inside, conflict) and menus: Update… (recursive, remote), Edit URL, Branch to Follow, Remove (optionally .git/modules), Add, Sync URLs | ✓ | ✓ | ✓ | ✓ | VSCode: Remove asks with two buttons: keep `.git/modules` or delete it too.<br>CLI: `submodule` (`add`, `remove`, `set-url`, `set-branch`, `sync`). |
| Submodules: move repository data into .git/modules, run a command in each | ✓ | ✓ | △ | ✓ | VSCode: Run in Each Submodule… says only whether it worked; the command's output isn't shown.<br>CLI: `submodule absorb`, `submodule foreach`. |
| Worktrees under Working Copy with change counts (✎ ⌂ ⑂) | ✓ | ✓ | ✓ | △ | VSCode: The marks are the rows' icons; a branch another worktree has checked out gets ⌂ or ⑂.<br>CLI: `worktree list --changes`. |
| Marks for a shallow or partial clone (History) and a sparse checkout (Working Copy) | ✓ | ✓ | △ | n/a | VSCode: Shallow and Partial above History (Partial fetches all contents when clicked); no sparse mark. |

## 1. Tabs and windows

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repository tabs in a window | ✓ | ✓ | n/a | n/a |  |
| Separate windows (New Window) | ✓ | ✓ | n/a | n/a |  |
| Open windows and tabs restored at launch | ✓ | ✓ | n/a | n/a |  |
| Connecting tab for SSH and daemon repositories (Cancel, Try Again) | ✓ | ✓ | n/a | n/a |  |
| Title shows uncommitted ● and ahead/behind | ✓ | ✓ | n/a | n/a | GTK4: On the tab names. |
| App keeps running after its last window closes | ✓ | — | n/a | n/a | GTK4: Quits, as GNOME apps do. |

## 2. Bookmarks (the repository list)

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repository page with All, Favorites, Places and groups | ✓ | ✓ | n/a | n/a |  |
| Add a local or an SSH repository | ✓ | ✓ | n/a | n/a |  |
| Folders to scan on this machine (depth, watched) | △ | △ | n/a | n/a | macOS: Excludes are a fixed list.<br>GTK4: Excludes are a fixed list. |
| Folders to scan on SSH hosts | ✓ | ✓ | n/a | △ | CLI: `find-repositories` does the scan for the apps. |
| Folder tree of the repositories under a place | ✓ | ✓ | n/a | n/a |  |
| Manual groups; rows dragged onto Favorites or a group | △ | △ | n/a | n/a | macOS: Groups themselves can't be reordered by dragging.<br>GTK4: Groups themselves can't be reordered by dragging. |
| Status columns (branch, changes, ahead/behind, last commit, opened), sortable | ✓ | ✓ | n/a | n/a |  |
| Favorites, display name, color label | ✓ | ✓ | n/a | n/a |  |
| Open in a tab or window; Finder / Files, Terminal | △ | △ | n/a | n/a | macOS: No Open in Editor.<br>GTK4: No Open in Editor. |
| Fetch the selected repositories | ✓ | ✓ | n/a | n/a |  |
| Quick Open (fuzzy search) | ✓ | ✓ | n/a | n/a |  |
| Repository name in the toolbar switches the repository | ✓ | ✓ | ✓ | n/a | VSCode: Select Repository picks among workspace repositories. |
| Clone (choosing from your own GitHub repositories) | ✓ | ✓ | n/a | △ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone <url>` only. |
| Clone: what to download (everything, contents when needed, recent history) and only some folders | ✓ | ✓ | n/a | ✓ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone --filter`, `--depth`, `--sparse`. |
| Subtrees group (shown when git has git subtree): Add, Pull, Push, Split, remembering the source and squash | ✓ | ✓ | ✓ | ✓ | VSCode: The values are asked in input boxes and the squash in a list, not a sheet.<br>CLI: `subtree`. |
| Clone: check out submodules too | ✓ | ✓ | n/a | ✓ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone --recurse-submodules`. |
| Init a new repository | — | — | n/a | — | macOS: Listed in docs/ux.md's File menu but not built. |

## 3. Settings

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| General: bookmark opened repositories | ✓ | ✓ | n/a | n/a |  |
| General: reopen at launch, tab or window by default | — | — | n/a | n/a | macOS: Always reopens; no switch.<br>GTK4: Always reopens; no switch. |
| General: fetch automatically | ✓ | ✓ | n/a | n/a | VSCode: VSCode's own `git.autofetch`. |
| General: external editor | ✓ | ✓ | n/a | n/a | GTK4: Code editors are found by their desktop entries, in the macOS order; the fallback is a text editor, never an office app; Other… takes a program. |
| General: terminal and diff tool | — | — | n/a | n/a |  |
| Git: machine-wide pages (Author and Signing, Commits, Operations, Working Copy, Keys and Values) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Git > Commits: shared message template library | ✓ | ✓ | — | — |  |
| Git > Programs: git, gpg, ssh-keygen, gpgsm | ✓ | ✓ | n/a | △ | VSCode: VSCode's `git.path` picks git (the extension follows it); gpg and ssh-keygen come from git's settings.<br>CLI: Via `amonhen run` (`programs`, `setMachineGit`). |
| Git > Environment Variables of this machine (pass, masked secrets) | ✓ | ✓ | n/a | n/a | VSCode: VSCode's own environment, which the extension passes to the daemon. |
| Environment Variables: use the login shell's (Read Again, SSH agent) | ✓ | — | n/a | n/a | GTK4: Left out: Linux sessions already carry it.<br>VSCode: VSCode's own environment, which the extension passes to the daemon. |
| SSH Hosts: hosts with + / −, unreachable dimmed | ✓ | ✓ | n/a | n/a | VSCode: Remote-SSH takes this role. |
| SSH Hosts > Connection: state, Check Connection, engine | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: signature verification | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: folders to scan | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: git pages for every repository on the host | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: the host's Environment Variables (login shell switch) | ✓ | ✓ | n/a | n/a |  |
| Path picker in columns for a host or this machine | ✓ | ✓ | n/a | n/a |  |
| Accounts: GitHub (CI status, avatars) | ✓ | ✓ | — | n/a |  |
| Appearance: History order, one line, row items, avatars | ✓ | ✓ | n/a | n/a |  |
| Appearance: theme, fonts, graph colors | — | — | n/a | n/a |  |
| Diff & Merge: resolve unambiguous conflicts on open | ✓ | ✓ | n/a | n/a |  |
| Diff & Merge: unified/split, whitespace, context, wrap, tab width | — | — | — | n/a |  |
| AI: backend, CLI location and version, model, language, per-repository permission | ✓ | ✓ | — | n/a | Neither app reads the CLI's login state; a missing CLI says to install it and sign in. |
| AI prompts per purpose (edit, Restore Default, Try) | ✓ | ✓ | — | n/a |  |
| Performance: commit-graph, shared daemon | ✓ | ✓ | △ | n/a | VSCode: `amonhen.daemon.enabled` and `amonhen.daemon.path` as VSCode settings. |
| Performance: fsmonitor | ✓ | ✓ | — | n/a | GTK4: On Linux and the BSDs, uses Watchman through the engine as git's fsmonitor hook (`amonhen fsmonitor-watchman`); disabled when Watchman isn't installed or git is older than 2.26 (hook protocol 2). |
| Performance: persistent cache location, size, clearing | — | — | — | △ | CLI: `log --rebuild`, `daemon rebuild`. |
| Undo / Redo of settings changes | △ | △ | n/a | n/a | macOS: Git pages and repository settings only.<br>GTK4: Git pages and repository settings only. |

## 4. Repository settings

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Settings page (pages, git line, source / ↩︎ / ⓘ per row) | ✓ | ✓ | ✓ | n/a | VSCode: Repository Settings in the main view (with the engine), this repository only. |
| Author and Signing (signing choice, collected keys, mailmap) | ✓ | ✓ | ✓ | △ | VSCode: Keys are collected for the current format only.<br>CLI: Via `amonhen run` (`signingKeys`, `configSet`). |
| Commits: message template (none, shared, own, file) | ✓ | ✓ | △ | △ | VSCode: None, this repository's own (edited there) or a file; no shared templates.<br>CLI: Via `amonhen run`. |
| Branches: default branch override | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Operations: pull, fetch.prune, autostash, updateRefs, conflict style, rerere | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| rerere: forget one resolution (merge editor), forget all (Repository Settings) | ✓ | ✓ | ✓ | △ | VSCode: Forget Resolution on a Working Copy conflict that rerere resolved (VSCode's merge editor is its own); Forget All… under Repository Settings > Operations.<br>CLI: Via `amonhen run` (`forgetResolution`, `forgetAllResolutions`). |
| Working Copy: line endings, file mode, `.git/info/exclude` editor | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Remotes: table and sheet with a connection check | ✓ | ✓ | ✓ | △ | VSCode: The table with a row of buttons (Edit URL…, Push URL…, Check, Prune, Rename…, Remove…) and Add…; the values are asked in input boxes, not a sheet.<br>CLI: Via `amonhen run`. |
| Hooks: table, Enabled switch, script, Create Hook… | ✓ | ✓ | ✓ | △ | VSCode: Open opens the script in VSCode's editor.<br>CLI: Via `amonhen run`. |
| LFS: installed, patterns, Track Pattern…, file count | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Keys and Values (git config table, Basic / Customized / All, Combined / Levels) | ✓ | ✓ | △ | △ | VSCode: Basic / Customized / All and search, with the value in effect and its source; values are edited in an input box; no Levels columns.<br>CLI: Via `amonhen run` (`configList`, `configSet`). |
| Rows disabled with the git version needed | ✓ | ✓ | ✓ | n/a | |
| Repository's form on the git line (shallow, partial, sparse, maintenance) | ✓ | ✓ | ✓ | △ | CLI: `status` notes shallow, partial and sparse repositories. |
| Fetching and Folders: fetch more of a shallow clone (a count, up to a date, all) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run` (`deepen`, `unshallow`). |
| Fetching and Folders: fetch all contents of a partial clone (git 2.36) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run` (`fetchAllContents`). |
| Fetching and Folders: folders of a sparse checkout picked from HEAD's tree (git 2.25) | ✓ | ✓ | ✓ | △ | VSCode: The folder is picked by walking HEAD's tree in a list.<br>CLI: Via `amonhen run` (`sparseCheckout`, `treeFolders`). |
| Fetching and Folders: git maintenance switch (git 2.31) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run` (`maintenance`). |

## 5. Toolbar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| New Orphan Branch (empty, or keeping the files) | ✓ | ✓ | ✓ | △ | VSCode: The Command Palette and the Branches group's right-click.<br>CLI: Via `amonhen run` (`createOrphanBranch`). |
| Branch switcher (fuzzy search, recent, tracking or new branch) | ✓ | ✓ | △ | △ | VSCode: Checkout from the sidebar and New Branch… only.<br>CLI: Via `amonhen run`. |
| Fetch sheet (all or one remote, prune shown) | ✓ | ✓ | △ | △ | VSCode: Fetches all with prune at once, without a sheet.<br>CLI: Via `amonhen run`. |
| Pull sheet (commits, merge / rebase / ff-only, upstream when missing) | ✓ | ✓ | △ | △ | VSCode: Merges the upstream without a sheet or choice.<br>CLI: Via `amonhen run`. |
| Push sheet (warnings, commits and diff, Force Push with lease) | ✓ | ✓ | △ | △ | VSCode: No sheet; Force Push asks separately.<br>CLI: Via `amonhen run`. |
| Publish (choose the remote and name) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Merge (no-ff / squash options) | △ | △ | △ | △ | macOS: No no-ff or squash choice.<br>GTK4: No no-ff or squash choice.<br>VSCode: Context menu or drag, no options.<br>CLI: Via `amonhen run`. |
| Octopus merge of several (order, expected conflicts) | ✓ | ✓ | ✓ | △ | CLI: `merge-check` checks; the merge via `amonhen run`. |
| Rebase sheet drawing before and after | ✓ | ✓ | △ | △ | VSCode: A dialog with the rewrite preview as text.<br>CLI: `rewrite-preview` prints it; the rebase via `amonhen run`. |
| Stash (message, include untracked) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Stash: only the staged changes, keep the index, selected files | ✓ | ✓ | ✓ | △ | VSCode: With the engine, the Stash list has the first two, and a Working Copy file's right-click has Stash… for that file.<br>CLI: Via `amonhen run` (`stashPush` with `staged`, `keepIndex`, `paths`). |
| Delete ignored files (listed first) | ✓ | ✓ | ✓ | △ | VSCode: The Command Palette and Working Copy's right-click; the list is a pick list, all chosen at first.<br>CLI: Via `amonhen run` (`ignoredFiles`, `deleteIgnored`). |
| Stash: staged only, selected files only | — | — | — | — |  |
| Pop the latest stash (asks first) | ✓ | ✓ | ✓ | △ | VSCode: Pop Latest Stash… on the Stashes header and in the Command Palette (a row's Pop doesn't ask).<br>CLI: Via `amonhen run`. |
| Activity spinner and count | ✓ | ✓ | ✓ | n/a | VSCode: VSCode's progress notifications. |
| Refresh and Rebuild History | ✓ | ✓ | △ | △ | VSCode: Refresh only.<br>CLI: `log --rebuild`, `daemon rebuild`. |
| Undo the last operation (asks, names what it puts back) | ✓ | ✓ | ✓ | ✓ | VSCode: The title's Undo and the Command Palette ask; a notification's Undo, pressed right after the operation, doesn't.<br>CLI: `undo` (`--peek` to see it first). |
| History search with scopes (subject, author, ID, path, contents) | ✓ | ✓ | — | ✓ | CLI: `log <query>`. |
| Ahead/behind badges | ✓ | ✓ | ✓ | △ | VSCode: On the sidebar's branch rows.<br>CLI: In `status`. |
| Toolbar customization | ✓ | ✓ | n/a | n/a | GTK4: The palette works like NSToolbar's and slides down under the header bar over the content like its sheet, without changing the window's size, with no Show (icon/text) choice since the header bar shows icons; the Repository menu and the main menu take the menu bar's place and stay put. |
| Network git stopped after 120 s of silence | ✓ | ✓ | — | ✓ | VSCode: Not in the extension's own git layer. |

## 5a. Stashes

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Stashes view (list and contents) | ✓ | ✓ | ✓ | △ | VSCode: Sidebar list; Show Stash opens VSCode's multi-file diff.<br>CLI: Listed by `refs`; `show stash@{n}`. |
| Stash row details (branch, base against HEAD, untracked) | ✓ | ✓ | ✓ | △ | VSCode: Untracked in the description, the branch and base in the tooltip (with the engine).<br>CLI: Via `amonhen run` (`stashBases`). |
| Apply, Pop, Drop… (undoable), Branch… | ✓ | ✓ | ✓ | △ | VSCode: Branch… with the engine; Apply and Pop restore the index (`--index`) when they can.<br>CLI: Via `amonhen run`. |

## 5b. Reflog

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Reflog view with "not on any branch" | ✓ | ✓ | ✓ | △ | VSCode: With the engine only.<br>CLI: Via `amonhen run`. |
| Reflog actions (New Branch, Checkout, New Tag, Reset, Show in History, Copy) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |

## 5c. Reset

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Reset to a commit (Soft / Mixed / Hard, Hard stashes first, undoable) | ✓ | ✓ | ✓ | △ | VSCode: With the engine only.<br>CLI: Via `amonhen run`. |

## 5d. Range Diff

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Range Diff view (= ! < >, Changes Only, diff of diffs) | ✓ | ✓ | △ | ✓ | VSCode: A read-only text document in git's form.<br>CLI: `range-diff`. |
| Compare with Previous Version | ✓ | ✓ | ✓ | ✓ | CLI: `range-diff --previous`. |
| Two History commits or a Reflog entry compared as two versions | ✓ | ✓ | △ | ✓ | VSCode: Range Diff… takes typed ranges instead.<br>CLI: Any ranges. |
| Editable ranges | ✓ | ✓ | △ | ✓ | VSCode: Only before opening. |

## 6. Working Copy

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| One checkbox list (☑ ▣ ☐, header and folder checks) | ✓ | ✓ | ✓ | n/a | VSCode: Separate Staged and Changes groups. |
| List / Tree / Flat | ✓ | ✓ | — | n/a |  |
| Filter on paths | ✓ | ✓ | — | n/a |  |
| Per-row +/− counts and Discard on hover | ✓ | ✓ | △ | △ | VSCode: No line counts.<br>CLI: `diff --stat`. |
| Space toggles, ⌫ discards | ✓ | ✓ | △ | n/a | VSCode: Space toggles; no ⌫. |
| Commit box on top with 50/72 counter and body guide | ✓ | ✓ | △ | n/a | VSCode: No counter or guide. |
| Draft per repository, starting from the template | ✓ | ✓ | — | n/a |  |
| Recent messages (⌥↑ / ⌥↓, list) | ✓ | ✓ | — | n/a |  |
| Insert Conventional types, Co-authored-by, Signed-off-by | ✓ | ✓ | — | n/a |  |
| `#issue` completion | ✓ | ✓ | — | n/a |  |
| Commit and Push, Amend | ✓ | ✓ | △ | △ | VSCode: Amend only.<br>CLI: `commit -m … --amend`. |
| Fixup / Squash Commit For | ✓ | ✓ | — | n/a |  |
| Hooks output sheet | ✓ | ✓ | — | n/a |  |
| Reason shown when Commit is disabled | — | — | △ | n/a | VSCode: A warning when committing is blocked. |
| Combined HEAD → working tree diff with line and hunk checks | ✓ | ✓ | △ | ✓ | VSCode: Line staging on separate unstaged and staged diffs.<br>CLI: `diff --combined`, `stage --rows`. |
| Range selection in the gutter, J/K and ⌥J/⌥K | ✓ | ✓ | △ | n/a | VSCode: No J/K keys. |
| Unstaged / Staged switch for files that can't be overlaid | ✓ | ✓ | n/a | ✓ | VSCode: Always shown separately.<br>CLI: `diff --staged`. |
| Diff options (unified/split, whitespace, context, wrap) | — | — | — | n/a |  |
| Discard lines, hunks and files (undoable) | ✓ | ✓ | ✓ | ✓ | CLI: `discard --rows`. |
| Add to .gitignore | ✓ | ✓ | — | n/a |  |
| Conflicted files open the merge editor | ✓ | ✓ | ✓ | n/a | VSCode: Opens VSCode's merge editor. |
| Note above the list when only some folders are checked out | ✓ | ✓ | — | △ | CLI: `status` notes a sparse checkout. |

## History

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Graph with branch and tag labels | ✓ | ✓ | ✓ | ✓ | CLI: `log`. |
| Huge histories in chunks with the persistent cache | ✓ | ✓ | ✓ | ✓ |  |
| Search with match count, next / previous, matching only | ✓ | ✓ | — | △ | CLI: `log <query>` lists the matches. |
| Display: order, one line, row items, fold merged, highlight ancestors | ✓ | ✓ | △ | △ | VSCode: Only the scope (current, all, a ref).<br>CLI: `--topo-order`, `--fold`. |
| Avatars | ✓ | ✓ | — | n/a |  |
| Signature marks on commits | ✓ | ✓ | ✓ | △ | VSCode: With the engine.<br>CLI: In `show`. |
| CI status per commit with a checks popover | ✓ | ✓ | — | — |  |
| Worktree HEAD labels (✎ ⌂ ⑂, change counts) | ✓ | ✓ | — | — |  |
| Go to HEAD, Parent, Child | ✓ | ✓ | ✓ | n/a | VSCode: The HEAD button above History, Go to Parent and Go to Child on a commit's menu, and parent and child links in the details (with the engine). |
| Go to Ref | — | — | — | n/a |  |
| Several commits selected (range, two, scattered) | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>` for two. |
| History of several selected refs | ✓ | ✓ | — | ✓ | CLI: `log --branch` repeated. |
| Commit menu: checkout, new branch or tag, cherry-pick, revert, copy ID | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Edit mode (keep, reword, squash, fixup, drop, edit; drag to reorder) | ✓ | ✓ | △ | △ | VSCode: One commit at a time, no plan of several steps.<br>CLI: Via `amonhen run`. |
| Rewrite preview (stacked and pushed branches, expected conflicts) | ✓ | ✓ | △ | ✓ | VSCode: Through the daemon and for the current branch only.<br>CLI: `rewrite-preview`. |
| Shortcuts: Edit Message…, Combine with Older…, Drop…, Combine N…, drag a commit | ✓ | ✓ | △ | △ | VSCode: No Combine N Commits.<br>CLI: Via `amonhen run`. |
| Split a commit | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Merges kept when editing (`--rebase-merges`) | ✓ | ✓ | — | △ | VSCode: Stops at the first merge.<br>CLI: Via `amonhen run`. |
| Shallow clone: band at the end to fetch more, boundary commits marked | ✓ | ✓ | ✓ | △ | VSCode: With the engine.<br>CLI: `log` notes where the history stops; fetching more via `amonhen run`. |

## 7. History's right column

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Full message, draggable panes, ⤢ to expand | ✓ | ✓ | △ | n/a | VSCode: No ⤢. |
| Find inside the details | ✓ | ✓ | — | n/a |  |
| All files stacked (sticky headers, collapse, Viewed) | ✓ | ✓ | △ | ✓ | VSCode: Stacked, loaded as they scroll; no collapse or Viewed.<br>CLI: `show`. |
| Changed files as a tree beside the diff | ✓ | ✓ | △ | n/a | VSCode: A flat list above the diffs. |
| File header menu (Blame, File History, copy path, open) | ✓ | ✓ | ✓ | n/a | VSCode: Blame, File History and Copy Path on the right-click; Open in VSCode's diff editor. |
| Switch to the single-file diff | ✓ | ✓ | — | n/a |  |
| Diff of a selection (range squashed, A..B / A...B, scattered) | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>`. |
| Actions on a selection (cherry-pick, revert, squash, copy IDs) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Signature popover | ✓ | ✓ | ✓ | n/a | VSCode: A Signature row in the details (status, explanation, signer and key), no popover. |
| Word highlights and syntax colors | ✓ | ✓ | △ | n/a | VSCode: Words only. |

## Blame / File History

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | △ | △ | VSCode: A page of the main view, without syntax colours.<br>CLI: Via `amonhen run` (`blame`). |
| Restore a file from a commit (this version, the version before; staged if chosen) | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine, from a file's right-click in History's details; Restore and Stage is the second button.<br>CLI: `restore --source <rev> [--staged]`. |
| File History | ✓ | ✓ | ✓ | △ | VSCode: A page of the main view, opened from the Explorer, an editor tab, the Command Palette or a file in the main view.<br>CLI: Via `amonhen run` (`fileHistory`). |
| Opened as a tab beside the repository tab, with the repository toolbar | ✓ | ✓ | n/a | n/a | GTK4: When the repository tab is in front in another window, the tab gets a title bar without the toolbar. |
| Line history | — | — | — | — |  |

## 7a. Patches

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Save as Patch (per commit or mbox, cover letter, -v) | ✓ | ✓ | △ | ✓ | VSCode: No `-v`.<br>CLI: `format-patch`. |
| Apply Patch (am / apply, preview, 3-way, --index) | ✓ | ✓ | ✓ | ✓ | CLI: `am`, `apply` (`--check`). |
| Drop patch files on the window | ✓ | ✓ | ✓ | n/a | VSCode: From the Explorer's context menu. |
| Export Archive (zip / tar.gz / tar, top folder, one folder) | ✓ | ✓ | — | ✓ | CLI: `archive`. |
| Save as Bundle (all, some refs, after a base) | ✓ | ✓ | — | ✓ | CLI: `bundle create`. |
| Clone from a bundle file (branch when no HEAD) | ✓ | ✓ | — | ✓ | CLI: `clone <file> [-b <branch>]`. |
| Fetch from Bundle (under a name, prerequisites checked) | ✓ | ✓ | — | ✓ | CLI: `bundle fetch`. |
| Stopped `am` with Retry with 3-Way, Skip, Abort, Continue | ✓ | ✓ | ✓ | ✓ | VSCode: In a sidebar row.<br>CLI: `am --continue / --skip / --abort / --retry-3way`. |

## 8. Merge editor

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Three columns with bands (Current / Result / Incoming) | ✓ | ✓ | n/a | n/a | VSCode: VSCode's own 3-way merge editor. |
| Show Base | ✓ | ✓ | n/a | n/a |  |
| Magic Merge (and the safe rules when a file opens) | ✓ | ✓ | n/a | ✓ | VSCode: Conflicts open VSCode's standard 3-way merge editor.<br>CLI: `merge-file` (also as git's mergetool). |
| Editable Result with undo per conflict | ✓ | ✓ | n/a | n/a |  |
| Resolve a file with Current / Incoming | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Open in an external merge tool | — | — | n/a | n/a |  |
| Pop out the editor into a window | ✓ | ✓ | n/a | n/a |  |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | △ | VSCode: On the sidebar's in-progress row.<br>CLI: Via `amonhen run`. |

## 9. AI

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Claude Code and Codex | ✓ | ✓ | — | — |  |
| Apple Intelligence (on-device) | ✓ | n/a | n/a | n/a |  |
| Translate a commit message | ✓ | ✓ | — | — |  |
| Explain a commit or a selection | ✓ | ✓ | — | — |  |
| Suggest a commit message | ✓ | ✓ | — | — |  |
| Explain a diff hunk | ✓ | ✓ | — | — |  |
| Resolve a conflict with AI | ✓ | ✓ | — | — |  |
| Resolve every file with AI | — | — | — | — | Designed in docs/ux.md (chapter 9), not built: the merge editor resolves one conflict at a time. |
| Explain a conflict | ✓ | ✓ | — | — |  |
| First-time confirmation per repository and for repository prompts | ✓ | ✓ | — | — |  |
| Streamed and cached results | ✓ | ✓ | — | — |  |

## 10. Activity

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operations (git commands, output, progress, cancel) | ✓ | ✓ | △ | △ | VSCode: An Output channel log only.<br>CLI: Via `amonhen run` (`remoteActivity`, `cancelActivity`). |
| History (operation log, Undo to Here) | ✓ | ✓ | △ | △ | VSCode: Undo History… lists the Undo log and undoes down to the chosen one; no other activity.<br>CLI: `undo --list` shows the log. |
| Performance | ✓ | ✓ | n/a | n/a | VSCode: VSCode's Output channel (the Amon Hen log). |
| Events | ✓ | ✓ | n/a | n/a | VSCode: VSCode's Output channel (the Amon Hen log). |
| Export as JSON | ✓ | ✓ | n/a | n/a | VSCode: VSCode's Output channel (the Amon Hen log). |
| Daemon tab (start, stop, connections) | ✓ | ✓ | — | △ | CLI: `daemon status / start / stop`. |

## 11. Menus

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Menus from the shared command table (`crates/commands`) | ✓ | ✓ | n/a | n/a | VSCode: VSCode's Command Palette and context menus. |
| Every command reachable without a menu bar (≡) | ✓ | ✓ | ✓ | n/a | GTK4: The ≡ menu holds the menu bar's File, Edit, View, Navigate, Commit and Window menus with macOS's items and order (Edit's Undo, Cut, Copy… act on the focused field), then Settings…, About and Quit.<br>VSCode: Command Palette and context menus. |
| Keyboard shortcuts | ✓ | ✓ | n/a | n/a | GTK4: Keys that Linux desktops or other commands take differ: Show Sidebar is F9 (GNOME's utility pane key); Filter Branches, Go to Parent / Child and New Tag have none (GNOME takes Ctrl+Alt+arrows and Ctrl+Alt+T, and the HIG gives Alt+Up another meaning); the merge editor's previous / next conflict is Alt+Up / Down.<br>VSCode: VSCode's keybindings; the extension contributes none, and any command can be bound. |
| Context menus with the menu's names | ✓ | ✓ | △ | n/a | VSCode: Names differ from the apps. |
| Keyboard shortcuts window | — | ✓ | n/a | n/a | macOS: macOS's App Shortcuts change them instead. |

## Status bar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operation in progress with Continue / Skip / Abort | ✓ | ✓ | ✓ | n/a | VSCode: A sidebar row. |
| Failure with its cause and Details | ✓ | ✓ | △ | n/a | VSCode: An error notification only. |
| Lost connection with Reconnect | ✓ | ✓ | n/a | n/a |  |
| What a finished operation did, for a few seconds | ✓ | ✓ | ✓ | n/a | VSCode: A status bar message or a notification with Undo. |

## Confirmations and Undo

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| A sheet before remote, HEAD-moving and deleting operations | ✓ | ✓ | △ | n/a | GTK4: Name prompts and every confirmation are the same sheet. Return presses the default button wherever the focus is (a drop-down or a radio too), as on macOS.<br>VSCode: Deleting, rewriting and Reset only; not Fetch, Pull, Push, Merge or Checkout. |
| Undo of git operations (`undo.json`) | ✓ | ✓ | ✓ | ✓ |  |
| Disabled controls say why in their tooltip | ✓ | ✓ | n/a | n/a | VSCode: Commands that can't be used aren't shown (when clauses). |

## 12. GitHub

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| GitHub through `gh` | ✓ | ✓ | — | — |  |
| Pull Requests (CI, draft, review; check out, open, copy URL) | ✓ | ✓ | — | — |  |
| Create Pull Request (base, draft) | ✓ | ✓ | — | — | macOS: The base is typed (the default branch at first).<br>GTK4: The base is typed (the default branch at first). |
| Branch status popover (CI, pull request) | ✓ | ✓ | — | — |  |
| Open on GitHub | ✓ | ✓ | — | — |  |
| `#123` links in messages | ✓ | ✓ | — | — |  |
| GitHub avatars | ✓ | ✓ | — | — |  |

## 13. Language

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| English UI with a Japanese translation | ✓ | ✓ | — | ✓ | VSCode: Not localized; some messages are in Japanese. |

## 14. Worktrees

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Worktrees with change counts | ✓ | ✓ | ✓ | ✓ | CLI: `worktree list --changes`. |
| The edited worktree (✎) and switching it | ✓ | ✓ | ✓ | n/a | VSCode: Switches the repository the extension works on; one outside the workspace isn't added to the Source Control view.<br>CLI: Works on the worktree it runs in. |
| New Worktree… (from a branch or commit) | ✓ | ✓ | ✓ | ✓ |  |
| Remove Worktree (asks, `--force` with changes) | ✓ | ✓ | ✓ | ✓ |  |
| Lock and unlock, move, locate a moved folder (repair) | ✓ | ✓ | ✓ | ✓ | VSCode: The worktree's right-click; the worktree being edited moves only after switching to another.<br>CLI: `worktree lock`, `unlock`, `move`, `repair`. |
| Prune worktrees | ✓ | ✓ | ✓ | ✓ |  |
| A worktree's folder opens its repository with it edited | ✓ | ✓ | ✓ | n/a | VSCode: Opening the folder works on that worktree, with the others listed. |

## SSH hosts, shared daemon, askpass

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repositories on SSH hosts (engine over ssh) | ✓ | ✓ | n/a | ✓ | VSCode: Remote-SSH runs the extension on the host.<br>CLI: `-C ssh://host/path`. |
| Through the shared daemon | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`. |
| Engine sent to a host when none fits | ✓ | ✓ | ✓ | ✓ | VSCode: The bundled engine is used on the host the extension runs on. |
| Windows SSH hosts | ✓ | ✓ | ✓ | ✓ |  |
| askpass dialog for passwords and passphrases | ✓ | ✓ | — | n/a | VSCode: Prompts are turned off (`GIT_TERMINAL_PROMPT=0`).<br>CLI: The dialog belongs to the apps. |
| Signature verification (this machine's keys or the host's) | ✓ | ✓ | — | △ | CLI: Checked in `show` and `refs --verify`; no choice. |
