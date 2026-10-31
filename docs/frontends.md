# Front-end features

[日本語](ja/frontends.md)

Which user-visible features each front end has: the macOS app (`apps/macos`), the GTK4 app (`apps/gtk4`), the VSCode extension (`apps/vscode`) and the CLI (`crates/cli`). The rows follow the chapters of [docs/ux.md](ux.md), which describes the features (mostly as the macOS app has them), plus History, the sidebar, Blame / File History, the status bar, confirmations and SSH. It was checked against the code, not the design.

Marks:

- ✓ done.
- △ partly there, or done differently; the Notes column says how.
- — not there.
- n/a doesn't apply to that front end (the CLI has no sidebar; VSCode has its own windows, settings and merge editor).

For the CLI, △ "via `amonhen run`" means the core's command exists and the CLI reaches it only as JSON (`amonhen run '{"op":…}'`), with no subcommand of its own.

The menu commands themselves are not listed one by one: every front end takes them from the shared table in `crates/commands`, and the GTK4 app's `PENDING` list in `apps/gtk4/src/appmenu.rs` names the ones it doesn't have yet (none at the moment).

This table is kept by hand. Update it in the same change that adds, removes or changes a feature in any front end.

## Overall layout

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Workspace rows switch the main view (Working Copy, History, Stashes, Reflog…) | ✓ | ✓ | △ | n/a | VSCode: Working Copy, History and Stashes only; no Reflog or Conflicts. |
| Conflicts row with a count while there are conflicts | ✓ | ✓ | △ | n/a | VSCode: A Conflicts group inside the Working Copy view. |
| Stashes row with a count only when there are stashes | ✓ | ✓ | △ | n/a | VSCode: The Stashes section is always shown. |
| Range Diff row while a range diff is open | ✓ | ✓ | — | n/a | GTK4: Closed with Close Range Diff, not ×. |
| Filter field over Branches, Remotes and Tags | ✓ | ✓ | — | n/a |  |
| Several branches or tags selected at once | ✓ | △ | — | n/a | GTK4: Bulk actions only; History does not follow the selection. |
| Pull Requests and Worktrees shown only when not empty | ✓ | ✓ | — | n/a | VSCode: Neither group exists. |

## Sidebar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Branches arranged (current, active, inactive, merged, archived, stacked) | ✓ | ✓ | △ | △ | VSCode: A folder tree split on `/` only.<br>CLI: Via `amonhen run` (`branchGroups`). |
| Favorite branches | ✓ | ✓ | — | — |  |
| Archived branches | ✓ | ✓ | — | — |  |
| Favorites reordered by dragging | ✓ | — | — | n/a |  |
| Branch menu: checkout, rename, delete (Delete Anyway when unmerged) | ✓ | ✓ | ✓ | △ | macOS: Set Upstream is only in the Repository menu.<br>GTK4: Set Upstream is only for the current branch.<br>CLI: Via `amonhen run`. |
| Sync a branch that is not checked out (Fast-Forward, Push, Publish) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Tags listed | ✓ | ✓ | ✓ | ✓ |  |
| Tag marks for remotes (missing, different) with a popover | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`tagRemotes`). |
| Tag signature marks | ✓ | — | — | ✓ | GTK4: Shown only in History's details.<br>CLI: `refs --verify`. |
| Tag menu: Push to, Replace on, Delete, Delete from a remote | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Remotes with their branches | ✓ | ✓ | ✓ | ✓ |  |
| Remote menu: Add, Rename, Edit URL, Remove, Prune | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Pull Requests group | ✓ | ✓ | — | — |  |
| Submodules group (Update, Open in New Window) | ✓ | ✓ | ✓ | △ | CLI: Listed by `refs`; Update via `amonhen run`. |
| Worktrees under Working Copy with change counts (✎ ⌂ ⑂) | ✓ | ✓ | — | △ | CLI: `worktree list --changes`. |

## 1. Tabs and windows

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repository tabs in a window | ✓ | △ | n/a | n/a | GTK4: Tabs are moved between windows with menu items (Move Tab to New Window, Merge All Windows), not by dragging. |
| Separate windows (New Window) | ✓ | ✓ | n/a | n/a |  |
| Open windows and tabs restored at launch | ✓ | ✓ | n/a | n/a |  |
| Connecting tab for SSH and daemon repositories (Cancel, Try Again) | ✓ | — | n/a | n/a | GTK4: A loading overlay and an error message only. |
| Title shows uncommitted ● and ahead/behind | ✓ | ✓ | n/a | n/a | GTK4: On the tab names. |
| App keeps running after its last window closes | ✓ | — | n/a | n/a | GTK4: Quits, as GNOME apps do. |

## 2. Bookmarks (the repository list)

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repository page with All, Favorites, Places and groups | ✓ | △ | n/a | n/a | GTK4: A separate Repositories window instead of a tab page. |
| Add a local or an SSH repository | ✓ | ✓ | n/a | n/a |  |
| Folders to scan on this machine (depth, watched) | △ | △ | n/a | n/a | macOS: Excludes are a fixed list.<br>GTK4: Excludes are a fixed list and folders are not watched. |
| Folders to scan on SSH hosts | ✓ | △ | n/a | △ | GTK4: Added in Repositories; not on the SSH Hosts page.<br>CLI: `find-repositories` does the scan for the apps. |
| Folder tree of the repositories under a place | ✓ | — | n/a | n/a |  |
| Manual groups; rows dragged onto Favorites or a group | △ | △ | n/a | n/a | macOS: Groups themselves can't be reordered by dragging.<br>GTK4: No dragging. |
| Status columns (branch, changes, ahead/behind, last commit, opened), sortable | ✓ | ✓ | n/a | n/a |  |
| Favorites, display name, color label | ✓ | ✓ | n/a | n/a |  |
| Open in a tab or window; Finder / Files, Terminal | △ | △ | n/a | n/a | macOS: No Open in Editor.<br>GTK4: No Open in Editor. |
| Fetch the selected repositories | ✓ | ✓ | n/a | n/a |  |
| Quick Open (fuzzy search) | ✓ | ✓ | n/a | n/a | GTK4: Opens in a new window. |
| Repository name in the toolbar switches the repository | ✓ | — | △ | n/a | VSCode: Select Repository picks among workspace repositories. |
| Clone (choosing from your own GitHub repositories) | ✓ | ✓ | n/a | △ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone <url>` only. |
| Init a new repository | — | — | n/a | — | macOS: Listed in docs/ux.md's File menu but not built. |

## 3. Settings

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| General: bookmark opened repositories | ✓ | ✓ | n/a | n/a |  |
| General: reopen at launch, tab or window by default | — | — | n/a | n/a | macOS: Always reopens; no switch.<br>GTK4: Always reopens; no switch. |
| General: fetch automatically | ✓ | ✓ | — | n/a |  |
| General: external editor | ✓ | — | n/a | n/a |  |
| General: terminal and diff tool | — | — | n/a | n/a |  |
| Git: machine-wide pages (Author and Signing, Commits, Operations, Working Copy, Keys and Values) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Git > Commits: shared message template library | ✓ | — | — | — |  |
| Git > Programs: git, gpg, ssh-keygen, gpgsm | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`programs`, `setMachineGit`). |
| Git > Environment Variables of this machine (pass, masked secrets) | ✓ | ✓ | — | n/a |  |
| Environment Variables: use the login shell's (Read Again, SSH agent) | ✓ | — | — | n/a | GTK4: Left out: Linux sessions already carry it. |
| SSH Hosts: hosts with + / −, unreachable dimmed | ✓ | ✓ | n/a | n/a | VSCode: Remote-SSH takes this role. |
| SSH Hosts > Connection: state, Check Connection, engine | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: signature verification | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts > Connection: folders to scan | ✓ | — | n/a | n/a |  |
| SSH Hosts: git pages for every repository on the host | ✓ | ✓ | n/a | n/a |  |
| SSH Hosts: the host's Environment Variables (login shell switch) | ✓ | ✓ | n/a | n/a |  |
| Path picker in columns for a host or this machine | ✓ | — | n/a | n/a | GTK4: Host paths are typed; local ones use the GTK file dialog. |
| Accounts: GitHub (CI status, avatars) | ✓ | ✓ | — | n/a |  |
| Appearance: History order, one line, row items, avatars | ✓ | ✓ | n/a | n/a |  |
| Appearance: theme, fonts, graph colors | — | — | n/a | n/a |  |
| Diff & Merge: resolve unambiguous conflicts on open | ✓ | ✓ | n/a | n/a |  |
| Diff & Merge: unified/split, whitespace, context, wrap, tab width | — | — | — | n/a |  |
| AI: backend, CLI location and version, model, language, per-repository permission | ✓ | △ | — | n/a | GTK4: No login state shown. |
| AI prompts per purpose (edit, Restore Default, Try) | ✓ | ✓ | — | n/a |  |
| Performance: commit-graph, shared daemon | ✓ | ✓ | △ | n/a | VSCode: `amonhen.daemon.enabled` and `amonhen.daemon.path` as VSCode settings. |
| Performance: fsmonitor | ✓ | — | — | n/a |  |
| Performance: persistent cache location, size, clearing | — | — | — | △ | CLI: `log --rebuild`, `daemon rebuild`. |
| Undo / Redo of settings changes | △ | △ | n/a | n/a | macOS: Git pages and repository settings only.<br>GTK4: Git pages and Keys and Values only. |

## 4. Repository settings

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Settings page (pages, git line, source / ↩︎ / ⓘ per row) | ✓ | ✓ | — | n/a |  |
| Author and Signing (signing choice, collected keys, mailmap) | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`signingKeys`, `configSet`). |
| Commits: message template (none, shared, own, file) | ✓ | △ | — | △ | GTK4: No shared templates.<br>CLI: Via `amonhen run`. |
| Branches: default branch override | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Operations: pull, fetch.prune, autostash, updateRefs, conflict style, rerere | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Working Copy: line endings, file mode, `.git/info/exclude` editor | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Remotes: table and sheet with a connection check | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Hooks: table, Enabled switch, script, Create Hook… | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| LFS: installed, patterns, Track Pattern…, file count | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Keys and Values (git config table, Basic / Customized / All, Combined / Levels) | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`configList`, `configSet`). |
| Rows disabled with the git version needed | ✓ | ✓ | — | n/a |  |

## 5. Toolbar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Branch switcher (fuzzy search, recent, tracking or new branch) | ✓ | △ | △ | △ | GTK4: A plain chooser without search or recents.<br>VSCode: Checkout from the sidebar and New Branch… only.<br>CLI: Via `amonhen run`. |
| Fetch sheet (all or one remote, prune shown) | ✓ | ✓ | △ | △ | VSCode: Fetches all with prune at once, without a sheet.<br>CLI: Via `amonhen run`. |
| Pull sheet (commits, merge / rebase / ff-only, upstream when missing) | ✓ | ✓ | △ | △ | VSCode: Merges the upstream without a sheet or choice.<br>CLI: Via `amonhen run`. |
| Push sheet (warnings, commits and diff, Force Push with lease) | ✓ | ✓ | △ | △ | VSCode: No sheet; Force Push asks separately.<br>CLI: Via `amonhen run`. |
| Publish (choose the remote and name) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Merge (no-ff / squash options) | △ | △ | △ | △ | macOS: No no-ff or squash choice.<br>GTK4: No no-ff or squash choice.<br>VSCode: Context menu or drag, no options.<br>CLI: Via `amonhen run`. |
| Octopus merge of several (order, expected conflicts) | ✓ | ✓ | ✓ | △ | CLI: `merge-check` checks; the merge via `amonhen run`. |
| Rebase sheet drawing before and after | ✓ | ✓ | △ | △ | VSCode: A dialog with the rewrite preview as text.<br>CLI: `rewrite-preview` prints it; the rebase via `amonhen run`. |
| Stash (message, include untracked) | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Stash: staged only, selected files only | — | — | — | — |  |
| Pop the latest stash (asks first) | ✓ | ✓ | — | △ | VSCode: Pop per stash row, without asking.<br>CLI: Via `amonhen run`. |
| Activity spinner and count | ✓ | ✓ | △ | n/a | VSCode: VSCode's progress notifications. |
| Refresh and Rebuild History | ✓ | ✓ | △ | △ | VSCode: Refresh only.<br>CLI: `log --rebuild`, `daemon rebuild`. |
| Undo the last operation (asks, names what it puts back) | ✓ | ✓ | △ | ✓ | VSCode: Undoes without asking.<br>CLI: `undo` (`--peek` to see it first). |
| History search with scopes (subject, author, ID, path, contents) | ✓ | ✓ | — | ✓ | GTK4: Scopes come from the query syntax.<br>CLI: `log <query>`. |
| Ahead/behind badges | ✓ | ✓ | △ | △ | VSCode: On the sidebar's branch rows.<br>CLI: In `status`. |
| Toolbar customization | ✓ | — | n/a | n/a |  |
| Network git stopped after 120 s of silence | ✓ | ✓ | — | ✓ | VSCode: Not in the extension's own git layer. |

## 5a. Stashes

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Stashes view (list and contents) | ✓ | ✓ | △ | △ | VSCode: Sidebar list; Show Stash opens VSCode's multi-file diff.<br>CLI: Listed by `refs`; `show stash@{n}`. |
| Stash row details (branch, base against HEAD, untracked) | ✓ | ✓ | △ | △ | VSCode: Name and subject only.<br>CLI: Via `amonhen run` (`stashBases`). |
| Apply, Pop, Drop… (undoable), Branch… | ✓ | ✓ | △ | △ | VSCode: No Branch… and no `--index`.<br>CLI: Via `amonhen run`. |

## 5b. Reflog

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Reflog view with "not on any branch" | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Reflog actions (New Branch, Checkout, New Tag, Reset, Show in History, Copy) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |

## 5c. Reset

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Reset to a commit (Soft / Mixed / Hard, Hard stashes first, undoable) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |

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
| One checkbox list (☑ ▣ ☐, header and folder checks) | ✓ | △ | △ | n/a | GTK4: Folder rows have no checkbox.<br>VSCode: Separate Staged and Changes groups. |
| List / Tree / Flat | ✓ | ✓ | — | n/a |  |
| Filter on paths | ✓ | ✓ | — | n/a |  |
| Per-row +/− counts and Discard on hover | ✓ | ✓ | △ | △ | VSCode: No line counts.<br>CLI: `diff --stat`. |
| Space toggles, ⌫ discards | ✓ | — | △ | n/a | VSCode: Space toggles; no ⌫. |
| Commit box on top with 50/72 counter and body guide | ✓ | △ | △ | n/a | GTK4: No counter or guide.<br>VSCode: No counter or guide. |
| Draft per repository, starting from the template | ✓ | ✓ | — | n/a |  |
| Recent messages (⌥↑ / ⌥↓, list) | ✓ | ✓ | — | n/a |  |
| Insert Conventional types, Co-authored-by, Signed-off-by | ✓ | ✓ | — | n/a |  |
| `#issue` completion | — | — | — | n/a |  |
| Commit and Push, Amend | ✓ | ✓ | △ | △ | VSCode: Amend only.<br>CLI: `commit -m … --amend`. |
| Fixup / Squash Commit For | ✓ | ✓ | — | n/a |  |
| Hooks output sheet | ✓ | — | — | n/a |  |
| Reason shown when Commit is disabled | — | — | △ | n/a | VSCode: A warning when committing is blocked. |
| Combined HEAD → working tree diff with line and hunk checks | ✓ | ✓ | △ | ✓ | VSCode: Line staging on separate unstaged and staged diffs.<br>CLI: `diff --combined`, `stage --rows`. |
| Range selection in the gutter, J/K and ⌥J/⌥K | ✓ | △ | △ | n/a | GTK4: No J/K keys.<br>VSCode: No J/K keys. |
| Unstaged / Staged switch for files that can't be overlaid | ✓ | ✓ | n/a | ✓ | VSCode: Always shown separately.<br>CLI: `diff --staged`. |
| Diff options (unified/split, whitespace, context, wrap) | — | — | — | n/a |  |
| Discard lines, hunks and files (undoable) | ✓ | ✓ | ✓ | ✓ | CLI: `discard --rows`. |
| Add to .gitignore | ✓ | ✓ | — | n/a |  |
| Conflicted files open the merge editor | ✓ | △ | △ | n/a | GTK4: Opens it in a separate window.<br>VSCode: Opens VSCode's merge editor. |

## History

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Graph with branch and tag labels | ✓ | ✓ | ✓ | ✓ | CLI: `log`. |
| Huge histories in chunks with the persistent cache | ✓ | ✓ | ✓ | ✓ |  |
| Search with match count, next / previous, matching only | ✓ | ✓ | — | △ | CLI: `log <query>` lists the matches. |
| Display: order, one line, row items, fold merged, highlight ancestors | ✓ | ✓ | △ | △ | VSCode: Only the scope (current, all, a ref).<br>CLI: `--topo-order`, `--fold`. |
| Avatars | ✓ | △ | — | n/a | GTK4: In the details only, kept in memory. |
| Signature marks on commits | ✓ | ✓ | — | △ | CLI: In `show`. |
| CI status per commit with a checks popover | ✓ | ✓ | — | — |  |
| Worktree HEAD labels (✎ ⌂ ⑂, change counts) | ✓ | ✓ | — | — |  |
| Go to HEAD, Parent, Child | ✓ | ✓ | △ | n/a | VSCode: Parent links in the details only. |
| Go to Ref | — | — | — | n/a |  |
| Several commits selected (range, two, scattered) | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>` for two. |
| History of several selected refs | ✓ | — | — | ✓ | CLI: `log --branch` repeated. |
| Commit menu: checkout, new branch or tag, cherry-pick, revert, copy ID | ✓ | ✓ | △ | △ | VSCode: No checkout of a commit.<br>CLI: Via `amonhen run`. |
| Edit mode (keep, reword, squash, fixup, drop, edit; drag to reorder) | ✓ | ✓ | △ | △ | VSCode: One commit at a time, no plan of several steps.<br>CLI: Via `amonhen run`. |
| Rewrite preview (stacked and pushed branches, expected conflicts) | ✓ | ✓ | △ | ✓ | VSCode: Through the daemon and for the current branch only.<br>CLI: `rewrite-preview`. |
| Shortcuts: Edit Message…, Combine with Older…, Drop…, Combine N…, drag a commit | ✓ | △ | △ | △ | GTK4: Commits can't be dragged in History.<br>VSCode: No Combine N Commits.<br>CLI: Via `amonhen run`. |
| Split a commit | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Merges kept when editing (`--rebase-merges`) | ✓ | ✓ | — | △ | VSCode: Stops at the first merge.<br>CLI: Via `amonhen run`. |

## 7. History's right column

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Full message, draggable panes, ⤢ to expand | ✓ | △ | △ | n/a | GTK4: No ⤢.<br>VSCode: No ⤢. |
| Find inside the details | ✓ | — | — | n/a |  |
| All files stacked (sticky headers, collapse, Viewed) | ✓ | △ | △ | ✓ | GTK4: Collapse only; no sticky headers or Viewed.<br>VSCode: Stacked, loaded as they scroll; no collapse or Viewed.<br>CLI: `show`. |
| Changed files as a tree beside the diff | ✓ | — | △ | n/a | VSCode: A flat list above the diffs. |
| File header menu (Blame, File History, copy path, open) | ✓ | △ | △ | n/a | GTK4: Blame, File History and AI only.<br>VSCode: Open in VSCode's diff editor only. |
| Switch to the single-file diff | ✓ | — | — | n/a |  |
| Diff of a selection (range squashed, A..B / A...B, scattered) | ✓ | ✓ | — | △ | CLI: `diff <rev> <rev>`. |
| Actions on a selection (cherry-pick, revert, squash, copy IDs) | ✓ | ✓ | — | △ | CLI: Via `amonhen run`. |
| Signature popover | ✓ | — | — | n/a | GTK4: A text row in the details. |
| Word highlights and syntax colors | ✓ | ✓ | △ | n/a | VSCode: Words only. |

## Blame / File History

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`blame`). |
| File History | ✓ | ✓ | — | △ | CLI: Via `amonhen run` (`fileHistory`). |
| Line history | — | — | — | — |  |

## 7a. Patches

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Save as Patch (per commit or mbox, cover letter, -v) | ✓ | ✓ | △ | ✓ | VSCode: No `-v`.<br>CLI: `format-patch`. |
| Apply Patch (am / apply, preview, 3-way, --index) | ✓ | ✓ | ✓ | ✓ | CLI: `am`, `apply` (`--check`). |
| Drop patch files on the window | ✓ | ✓ | △ | n/a | VSCode: From the Explorer's context menu. |
| Stopped `am` with Retry with 3-Way, Skip, Abort, Continue | ✓ | ✓ | ✓ | ✓ | VSCode: In a sidebar row.<br>CLI: `am --continue / --skip / --abort / --retry-3way`. |

## 8. Merge editor

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Three columns with bands (Current / Result / Incoming) | ✓ | △ | n/a | n/a | GTK4: In a separate window, not embedded.<br>VSCode: VSCode's own 3-way merge editor. |
| Show Base | ✓ | ✓ | n/a | n/a |  |
| Magic Merge (and the safe rules when a file opens) | ✓ | ✓ | — | ✓ | CLI: `merge-file` (also as git's mergetool). |
| Editable Result with undo per conflict | ✓ | ✓ | n/a | n/a |  |
| Resolve a file with Current / Incoming | ✓ | ✓ | ✓ | △ | CLI: Via `amonhen run`. |
| Open in an external merge tool | — | — | n/a | n/a |  |
| Pop out the editor into a window | ✓ | n/a | n/a | n/a | GTK4: Always its own window. |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | △ | VSCode: On the sidebar's in-progress row.<br>CLI: Via `amonhen run`. |

## 9. AI

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Claude Code and Codex | ✓ | ✓ | — | — |  |
| Apple Intelligence (on-device) | ✓ | n/a | n/a | n/a |  |
| Translate a commit message | ✓ | ✓ | — | — | GTK4: Shown inline under the message. |
| Explain a commit or a selection | ✓ | ✓ | — | — | GTK4: Shown inline. |
| Suggest a commit message | ✓ | ✓ | — | — |  |
| Explain a diff hunk | ✓ | ✓ | — | — |  |
| Resolve a conflict with AI | ✓ | △ | — | — | GTK4: One conflict at a time, shown as a band. |
| Resolve every file with AI | ✓ | — | — | — |  |
| Explain a conflict | ✓ | ✓ | — | — |  |
| First-time confirmation per repository and for repository prompts | ✓ | ✓ | — | — |  |
| Streamed and cached results | ✓ | ✓ | — | — |  |

## 10. Activity

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operations (git commands, output, progress, cancel) | ✓ | ✓ | △ | △ | VSCode: An Output channel log only.<br>CLI: Via `amonhen run` (`remoteActivity`, `cancelActivity`). |
| History (operation log, Undo to Here) | ✓ | ✓ | — | △ | CLI: `undo --list` shows the log. |
| Performance | ✓ | ✓ | — | n/a |  |
| Events | ✓ | ✓ | — | n/a |  |
| Export as JSON | ✓ | ✓ | — | n/a |  |
| Daemon tab (start, stop, connections) | ✓ | ✓ | — | △ | CLI: `daemon status / start / stop`. |

## 11. Menus

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Menus from the shared command table (`crates/commands`) | ✓ | ✓ | — | n/a | VSCode: Declared in package.json. |
| Every command reachable without a menu bar (≡) | ✓ | ✓ | △ | n/a | VSCode: Command Palette and context menus. |
| Keyboard shortcuts | ✓ | ✓ | — | n/a | VSCode: No keybindings contributed. |
| Context menus with the menu's names | ✓ | ✓ | △ | n/a | VSCode: Names differ from the apps. |
| Keyboard shortcuts window | — | ✓ | n/a | n/a | macOS: macOS's App Shortcuts change them instead. |

## Status bar

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Operation in progress with Continue / Skip / Abort | ✓ | ✓ | △ | n/a | VSCode: A sidebar row. |
| Failure with its cause and Details | ✓ | ✓ | △ | n/a | VSCode: An error notification only. |
| Lost connection with Reconnect | ✓ | ✓ | n/a | n/a |  |
| What a finished operation did, for a few seconds | ✓ | ✓ | △ | n/a | VSCode: A status bar message or a notification with Undo. |

## Confirmations and Undo

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| A sheet before remote, HEAD-moving and deleting operations | ✓ | ✓ | △ | n/a | VSCode: Deleting and rewriting only; not Fetch, Pull, Push, Merge or Checkout. |
| Undo of git operations (`undo.json`) | ✓ | ✓ | ✓ | ✓ |  |
| Disabled controls say why in their tooltip | ✓ | — | — | n/a |  |

## 12. GitHub

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| GitHub through `gh` | ✓ | ✓ | — | — |  |
| Pull Requests (CI, draft, review; check out, open, copy URL) | ✓ | ✓ | — | — |  |
| Create Pull Request (base, draft) | ✓ | △ | — | — | GTK4: The base is typed. |
| Branch status popover (CI, pull request) | ✓ | ✓ | — | — |  |
| Open on GitHub | ✓ | ✓ | — | — |  |
| `#123` links in messages | ✓ | — | — | — |  |
| GitHub avatars | ✓ | △ | — | — | GTK4: Kept in memory only. |

## 13. Language

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| English UI with a Japanese translation | ✓ | ✓ | — | ✓ | VSCode: Not localized; some messages are in Japanese. |

## 14. Worktrees

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Worktrees with change counts | ✓ | ✓ | — | ✓ | CLI: `worktree list --changes`. |
| The edited worktree (✎) and switching it | ✓ | ✓ | — | n/a | CLI: Works on the worktree it runs in. |
| New Worktree… (from a branch or commit) | ✓ | ✓ | — | ✓ |  |
| Remove Worktree (asks, `--force` with changes) | ✓ | △ | — | ✓ | GTK4: Hidden for the main worktree and the opened one. |
| Prune worktrees | ✓ | ✓ | — | ✓ |  |
| A worktree's folder opens its repository with it edited | ✓ | — | — | n/a | GTK4: Opens it as a repository of its own. |

## SSH hosts, shared daemon, askpass

| Feature | macOS | GTK4 | VSCode | CLI | Notes |
| --- | :-: | :-: | :-: | :-: | --- |
| Repositories on SSH hosts (engine over ssh) | ✓ | ✓ | n/a | ✓ | VSCode: Remote-SSH runs the extension on the host.<br>CLI: `-C ssh://host/path`. |
| Through the shared daemon | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`. |
| Engine sent to a host when none fits | ✓ | ✓ | △ | ✓ | VSCode: The bundled engine is used on the host the extension runs on. |
| Windows SSH hosts | ✓ | ✓ | ✓ | ✓ |  |
| askpass dialog for passwords and passphrases | ✓ | ✓ | — | n/a | VSCode: Prompts are turned off (`GIT_TERMINAL_PROMPT=0`).<br>CLI: The dialog belongs to the apps. |
| Signature verification (this machine's keys or the host's) | ✓ | ✓ | — | △ | CLI: Checked in `show` and `refs --verify`; no choice. |
