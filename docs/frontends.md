# Front-end features

[日本語](ja/frontends.md)

Which user-visible features each front end has: the macOS app (`apps/macos`), the GTK4 app (`apps/gtk4`), the Windows app (`apps/windows`, WinUI 3, not released yet), the VSCode extension (`apps/vscode`), the CLI (`crates/cli`) and the terminal front end (`amonhen tui`, `crates/tui`). The rows follow the chapters of [docs/ux.md](ux.md), which describes the features (mostly as the macOS app has them), plus History, the sidebar, Blame / File History, the status bar, confirmations and SSH. It was checked against the code, not the design.

Marks:

- ✓ done, possibly in that front end's own form (a tooltip instead of a popover, an input box instead of a sheet); the Notes column says how.
- △ partly there; the Notes column says what's missing. For the Windows app, a form that differs from the macOS app counts as △ unless Windows itself rules out the macOS form; the Notes column describes the Windows form.
- — not there.
- n/a doesn't apply to that front end (the CLI has no sidebar), or is left to the platform on purpose (VSCode's own settings, keybindings, Output channel and merge editor); the Notes column says what does it instead.

For the CLI, △ "via `amonhen run`" means the core's command exists and the CLI reaches it only as JSON (`amonhen run '{"op":…}'`), with no subcommand of its own.

The menu commands themselves are not listed one by one: every front end takes them from the shared table in `crates/commands`, and the GTK4 app's `PENDING` list in `apps/gtk4/src/appmenu.rs` names the ones it doesn't have yet (none at the moment).

This table is kept by hand. Update it in the same change that adds, removes or changes a feature in any front end.

## Platform differences between the macOS and GTK4 apps

The GTK4 app aims to behave like the macOS app. These are the only intended differences; each follows the platform, mostly the [GNOME HIG](https://developer.gnome.org/hig/):

- The app quits when its last window closes.
- No import of the login shell's environment (Linux sessions already carry it).
- On Linux and the BSDs, fsmonitor uses Watchman when installed, with git 2.26 or later (git has no fsmonitor--daemon on the BSDs, and on Linux only from git 2.55, which Amon Hen does not use yet).
- No Apple Intelligence; Claude Code and Codex only.
- Files (the desktop's file manager) instead of Finder; external editors are found by their desktop entries.
- The header bar shows icons only, with tooltips; toolbar customization has no icon/text choice.
- History search is GNOME's search bar: a toggle in the header bar and Ctrl+F reveal a bar above History, and typing in History opens it with what was typed (type-to-search; not with Space, arrows or Ctrl/Alt keys). With a Japanese input method on, the first key stays Latin, because the input method only works once the field has focus; use Ctrl+F first to type Japanese.
- A click outside an open popover (a CI popover, a menu) only closes it, as with any GTK popover; on macOS the same click also reaches what was clicked, so opening another popover takes one click there and two in GTK4.
- Destructive confirmations: Cancel comes first and takes Return and Esc, and the destructive button is red; Cancel isn't highlighted.
- Keyboard shortcuts that GNOME takes are moved or left out (see Keyboard shortcuts below).
- A window lists the keyboard shortcuts, as GNOME apps have; macOS shows them in its menus.
- “This Computer”, the package manager and systemd/cron in place of “This Mac”, Homebrew and launchd.

## Overall layout

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Workspace rows switch the main view (Working Copy, History, Stashes, Reflog…) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Working Copy and History; with the engine, Reflog and Repository Settings, and Range Diff while one is open; Conflicts while there are conflicts, opening the first in the merge editor. Stashes is a section of the sidebar.<br>TUI: Changes, History and Reflog (also `1` `2` `3`); stashes are listed in the sidebar. |
| Conflicts row with a count while there are conflicts | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: A Conflicts group inside the Working Copy view.<br>TUI: Under Workspace in the sidebar; it opens the Working Copy. |
| Stashes row with a count only when there are stashes | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: The Stashes section.<br>TUI: A Stashes section in the sidebar while there are stashes, its heading with the count. |
| Range Diff row while a range diff is open | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: Until another file's Blame or File History takes its place. |
| Filter field over Branches, Remotes and Tags | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: The filter button on the sidebar's title asks for the words; the matching branches, remote branches and tags are listed flat with their full names.<br>TUI: `/` in the sidebar. |
| Several branches or tags selected at once | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Copy Names, Merge into Current Branch, Delete Branch, favorites and Archive act on all of them, and with tags only, Push, Replace, Delete and Delete from a remote.<br>TUI: `v` marks refs in the sidebar; `m` merges them in one commit, `D` deletes the marked branches, and `*` `z` add them to Favorites or archive them. |
| Pull Requests and Worktrees shown only when not empty | ✓ | ✓ | ✓ | △ | n/a | △ | VSCode: Worktrees only (GitHub is left out).<br>Windows: Worktrees are listed under Working Copy.<br>TUI: Worktrees only, when there are two or more. |

## Sidebar

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Branches arranged (current, active, inactive, merged, archived, stacked) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine; without it, a folder tree split on `/`.<br>CLI: Via `amonhen run` (`branchGroups`).<br>TUI: Favorites (★), the current and default branches, active ones newest first, stacked ones under their base (↳), merged ones marked, Inactive and Archived folded (Enter opens them); no `/` folders. |
| Favorite branches | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: `*` on a branch in the sidebar. |
| Archived branches | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: `z` on a branch in the sidebar. |
| Clean Up Branches (merged, merged by a pull request, upstream gone; one Undo) | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: With the engine, a multi-select list with a heading per reason (Command Palette, the Branches heading's right-click). CLI: Via `amonhen run` (`branchCleanup`, `deleteBranches`); merged pull requests are read by the apps and VSCode with gh.<br>TUI: Clean Up Branches in the command list; `a` takes in branches whose upstream is gone. Branches merged by a pull request aren't looked for (no GitHub). |
| Merged-by-pull-request mark on a branch (squash or rebase on GitHub) | ✓ | ✓ | ✓ | ✓ | △ | — | VSCode: With the engine; the number is in the row's tooltip. CLI: Via `amonhen run` (`branchGroups` with `pullRequests`); the apps and VSCode read merged pull requests with gh. |
| Favorites reordered by dragging | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Move Up and Move Down in Favorites on the menu (a drop on a branch offers Merge and Rebase).<br>TUI: `<` `>` on a favorite. |
| Branch menu: checkout, rename, delete (Delete Anyway when unmerged) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | macOS: Set Upstream is only in the Repository menu.<br>GTK4: Set Upstream is only in the Repository menu.<br>VSCode: Rename Branch… in the Command Palette renames the current branch, and Undo gives the earlier name back. Deleting several asks once, naming how many commits are on no other branch for each, with Delete Anyway.<br>CLI: Via `amonhen run`.<br>TUI: `c` checks out, `n` renames and `D` deletes; with commits on no other branch, it names how many and draws them, then deletes anyway. |
| Sync a branch that is not checked out (Fast-Forward, Push, Publish) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Fast-Forward asks first: with the main view open, a sheet with the drawing before and after; otherwise a modal.<br>CLI: Via `amonhen run`.<br>TUI: `p` and `P` on the branch in the sidebar. |
| Tags listed | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Tag marks for remotes (missing, different) with a popover | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: In the row's description, each remote's state in the tooltip (no popover).<br>CLI: Via `amonhen run` (`tagRemotes`).<br>Windows: ⚠ (a different tag on a remote) or ∅ (on no remote) after the name; clicking it opens the popover.<br>TUI: ⚠ or ∅ at the end of the row; `i` puts each remote's state on the bottom line instead of a popover, and `t` pushes or replaces. |
| Tag signature marks | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The row's icon, explained in the tooltip.<br>CLI: `refs --verify`.<br>TUI: ✓ (verified, or yellow when the key isn't trusted), ! (expired or revoked), ✗ (bad) or ? (can't check) at the end of the row; `i` shows the signature in a box. |
| Tag menu: Push to, Replace on, Delete, Delete from a remote | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: The remote is chosen in a list when there are several; Push asks first, naming the tags.<br>CLI: Via `amonhen run`.<br>TUI: `D` deletes a tag here, `t` pushes or replaces it on a remote and `X` deletes it from one, the remote chosen by digit. |
| Remotes with their branches | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Remote menu: Add, Rename, Edit URL, Remove, Prune | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Rename…, Edit URL…, Edit Push URL… and Check Connection in place of the app's one Edit… sheet.<br>CLI: Via `amonhen run`.<br>TUI: Add Remote in the command list; on a remote branch in the sidebar `e` edits the name and URL, `X` prunes and `x` removes. |
| Pull Requests group | ✓ | ✓ | ✓ | — | — | — |  |
| Submodules group (Update, Open in New Window) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `submodule`, `submodule update`.<br>TUI: A Submodules section in the sidebar, where Enter reopens the TUI in the submodule; Update Submodules in the command list. |
| Submodule marks (not checked out, another commit, changes inside, conflict) and menus: Update… (recursive, remote), Edit URL, Branch to Follow, Remove (optionally .git/modules), Add, Sync URLs | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Remove asks with two buttons: keep `.git/modules` or delete it too.<br>CLI: `submodule` (`add`, `remove`, `set-url`, `set-branch`, `sync`).<br>TUI: Marks at the end of the row; Update Submodules and Add Submodule in the command list, and on a submodule's row `e` URL, `B` branch to follow, `x` remove (`d` deletes .git/modules too), `y` sync URLs. |
| Submodules: move repository data into .git/modules, run a command in each | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Run in Each Submodule…'s output goes to an Output channel (the app's Activity).<br>CLI: `submodule absorb`, `submodule foreach`.<br>TUI: `a` and `F` on a submodule's row; `!` shows the command's output. |
| Worktrees under Working Copy with change counts (✎ ⌂ ⑂) | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: The marks are the rows' icons; a branch another worktree has checked out shows only ⌂ or ⑂ (the worktree's name in the tooltip) and has Open Worktree in place of Checkout.<br>CLI: `worktree list --changes`.<br>TUI: In the sidebar (folder, branch and change count) when there are two or more, not under Working Copy. |
| Marks for a shallow or partial clone (History) and a sparse checkout (Working Copy) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Shallow and Partial above History (Partial fetches all contents when clicked); " · some folders" on the Working Copy row for a sparse checkout.<br>TUI: [shallow] and [partial] on the top line after the branch; a sparse checkout's note above the Working Copy list. |

## 1. Tabs and windows

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Repository tabs in a window | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: Tabs in the back keep their repository open; coming back redraws the page they were on.<br>TUI: No tabs or windows; Enter on another worktree and Quick Open reopen the TUI there. |
| Separate windows (New Window) | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: New Window opens one on the current repository showing Repositories; tabs are dragged between windows or out into a new one, and the Window menu lists the windows. |
| Open windows and tabs restored at launch | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| Connecting tab for SSH and daemon repositories (Cancel, Try Again) | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| Title shows uncommitted ● and ahead/behind | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: On the tab names.<br>Windows: On the window title and each tab's name.<br>TUI: The top line shows the branch with ahead and behind. |
| App keeps running after its last window closes | ✓ | — | — | n/a | n/a | n/a | GTK4: Quits, as GTK apps do by default.<br>Windows: Quits, as Windows apps do. |

## 2. Bookmarks (the repository list)

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Repository page with All, Favorites, Places and groups | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: Ctrl-o or Repositories in the command list, over the screen; Quick Open also lists them. |
| Add a local or an SSH repository | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: `a` on the Repositories page, a path on this machine or `ssh://host/path` typed. |
| Folders to scan on this machine (depth, watched) | △ | △ | △ | n/a | n/a | △ | macOS: Excludes are a fixed list.<br>GTK4: Excludes are a fixed list.<br>Windows: Excludes are a fixed list; the folders are watched while Repositories is open.<br>TUI: Excludes are a fixed list; on the Repositories page, `a` adds a folder, and on a 🔍 folder `d` sets the depth, `r` rescans and `x` stops; watched while the page is open. |
| Folders to scan on SSH hosts | ✓ | ✓ | ✓ | n/a | △ | ✓ | CLI: `find-repositories` does the scan for the apps.<br>TUI: Searched by the engine on the host, after asking to send it when needed. |
| Folder tree of the repositories under a place | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: Under each place on the Repositories page. |
| Manual groups; rows dragged onto Favorites or a group | △ | △ | ✓ | n/a | n/a | △ | macOS: Groups themselves can't be reordered by dragging.<br>GTK4: Groups themselves can't be reordered by dragging.<br>TUI: `g` types the group; no dragging. |
| Status columns (branch, changes, ahead/behind, last commit, opened), sortable and reorderable | ✓ | ✓ | ✓ | n/a | n/a | △ | TUI: `s` changes the sorted column; the columns can't be reordered. |
| Favorites, display name, color label | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: `*`, `n` and `c` on the Repositories page. |
| Open in a tab or window; Finder / Files, Terminal, Editor | ✓ | ✓ | ✓ | n/a | n/a | ✓ | GTK4: An app other than a known code editor or a program chosen with Other… opens a folder only if it declares inode/directory.<br>Windows: Explorer, Windows Terminal (or cmd) and the editor chosen in Settings.<br>TUI: Enter reopens the TUI there; `t` a shell in the folder (the terminal handed over), `e` git's editor, `o` the file manager. |
| Fetch the selected repositories | ✓ | ✓ | ✓ | n/a | n/a | ✓ | Windows: Rows are selected with Ctrl or Shift, and Fetch on a selected row's menu fetches them all.<br>TUI: `v` marks rows and `f` fetches them in the background; repositories on SSH hosts through the shared daemon, after asking to send the engine when needed. |
| Quick Open (fuzzy search) | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: Quick Open in the command list lists what the apps' does: the shared bookmarks (`bookmarks.json`, favorites first), recently opened repositories (`recent.json`) and those found under the local folders to scan (searched in the background; folders on SSH hosts aren't); Enter reopens the TUI there. |
| Repository name in the toolbar switches the repository | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: Select Repository picks among workspace repositories.<br>TUI: Quick Open instead. |
| Clone (choosing from your own GitHub repositories) | ✓ | ✓ | ✓ | n/a | △ | △ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone <url>` only.<br>Windows: The clone opens in a new tab.<br>TUI: A URL typed in Quick Open; no GitHub. |
| Clone: what to download (everything, contents when needed, recent history) and only some folders | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone --filter`, `--depth`, `--sparse`.<br>TUI: Asked after the folder. |
| Subtrees group (shown when git has git subtree): Add, Pull, Push, Split, remembering the source and squash | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The values are asked in input boxes and the squash in a list, not a sheet.<br>CLI: `subtree`.<br>TUI: Add Subtree in the command list; `p` `P` `t` on a subtree's row; the source is typed on the bottom line and the squash chosen there. |
| Clone: check out submodules too | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: VSCode's own Git: Clone.<br>CLI: `clone --recurse-submodules`.<br>TUI: Asked before cloning. |
| Init a new repository | — | — | — | n/a | — | — | macOS: Listed in docs/ux.md's File menu but not built. |

## 3. Settings

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| General: bookmark opened repositories | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: No settings pages: the apps and `amonhen run` change the settings, which the TUI reads; `settings.json` `tui.mouse` sets the mouse default. |
| General: reopen at launch, tab or window by default | — | — | — | n/a | n/a | n/a | macOS: Always reopens; no switch.<br>GTK4: Always reopens; no switch. |
| General: fetch automatically | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode's own `git.autofetch`.<br>Windows: Windows: A failure is added to the status bar. |
| General: external editor | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: Code editors are found by their desktop entries, in the macOS order; the fallback is a text editor, never an office app; Other… takes a program.<br>Windows: Known code editors are found in their install folders or by their launchers on PATH, in the macOS order; the fallback is Notepad, which can't open folders; Other… takes a program.<br>TUI: git's editor. |
| General: terminal and diff tool | — | — | — | n/a | n/a | n/a |  |
| Git: machine-wide pages (Author and Signing, Commits, Operations, Working Copy, Keys and Values) | ✓ | ✓ | ✓ | — | △ | n/a | CLI: Via `amonhen run`. |
| Git > Commits: shared message template library | ✓ | ✓ | ✓ | — | — | n/a |  |
| Git > Programs: git, gpg, ssh-keygen, gpgsm | ✓ | ✓ | ✓ | n/a | △ | n/a | VSCode: The extension runs the git chosen here for the machine, and VSCode's `git.path` only when none is chosen; gpg and ssh-keygen come from git's settings.<br>CLI: Via `amonhen run` (`programs`, `setMachineGit`).<br>Windows: In Settings > Git. |
| Git > Environment Variables of this machine (pass, masked secrets) | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode's own environment, which the extension passes to the daemon. |
| Environment Variables: use the login shell's (Read Again, SSH agent) | ✓ | — | — | n/a | n/a | n/a | GTK4: Left out: Linux sessions already carry it.<br>Windows: Left out: Windows gives apps the user's environment without a login shell.<br>VSCode: VSCode's own environment, which the extension passes to the daemon. |
| SSH Hosts: hosts with + / −, unreachable dimmed | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: Remote-SSH takes this role. |
| SSH Hosts > Connection: state, Check Connection, engine | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts > Connection: signature verification | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts > Connection: folders to scan | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts: git pages for every repository on the host | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| SSH Hosts: the host's Environment Variables (login shell switch) | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| Path picker in columns for a host or this machine | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: Repositories' Add Repositories and Add a Folder to Scan, and folders on SSH hosts; File > Open… and Clone keep the system dialog. |
| Accounts: GitHub (CI status, avatars) | ✓ | ✓ | ✓ | — | n/a | — |  |
| Appearance: History order, one line, row items, avatars | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: `o` and `z` change the order and folding for the run. |
| Appearance: fonts, graph colors | — | — | — | n/a | n/a | n/a |  |
| Theme: window background (transparency and blur, image, toolbar tint, per repository) | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: No blur (GNOME can blur behind a window only from GNOME 51 with GTK 4.24, through `ext-background-effect-v1`, which the app does not use yet); a see-through window needs a compositor.<br>Windows: The blur strength is fixed by Acrylic. |
| Theme: syntax and diff colors (built-in themes, VSCode themes, standard diff colors) | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: follows VSCode's own theme.<br>Windows: The app follows Windows' app mode (light or dark), with a theme for each.<br>TUI: Follows the terminal's colors. |
| Diff & Merge: resolve unambiguous conflicts on open | ✓ | ✓ | ✓ | n/a | n/a | n/a |  |
| Diff & Merge: unified/split, whitespace, context, wrap, tab width | — | — | — | — | n/a | n/a |  |
| AI: backend, CLI location and version, model, language, per-repository permission | ✓ | ✓ | — | — | n/a | — | Neither app reads the CLI's login state; a missing CLI says to install it and sign in. |
| AI prompts per purpose (edit, Restore Default, Try) | ✓ | ✓ | — | — | n/a | — |  |
| Performance: commit-graph, shared daemon | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: `amonhen.writeCommitGraph`, `amonhen.daemon.enabled` and `amonhen.daemon.path` as VSCode settings. |
| Performance: fsmonitor | ✓ | ✓ | ✓ | ✓ | n/a | n/a | GTK4: On Linux and the BSDs, uses Watchman through the engine as git's fsmonitor hook (`amonhen fsmonitor-watchman`); disabled when Watchman isn't installed or git is older than 2.26 (hook protocol 2).<br>VSCode: `amonhen.fsmonitor` as a VSCode setting, on by default. |
| Performance: persistent cache location, size, clearing | ✓ | ✓ | ✓ | — | △ | n/a | History and AI results, one row each with its size and Clear. CLI: `log --rebuild`, `daemon rebuild`.<br>Windows: History only (there is no AI). |
| Undo / Redo of settings changes | △ | △ | △ | n/a | n/a | n/a | macOS: Git pages and repository settings only.<br>GTK4: Git pages and repository settings only.<br>Windows: Git pages and repository settings only (Ctrl+Z / Ctrl+Shift+Z outside a text field). |

## 4. Repository settings

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Settings page (pages, git line, source / ↩︎ / ⓘ per row) | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: Repository Settings in the main view (with the engine), this repository only.<br>Windows: Pages listed on the left (General, Git, Appearance, Diff & Merge, Performance); the setting rows have the source, ↩︎ and ⓘ (each level's value as a tooltip).<br>TUI: No settings pages; the apps and `amonhen run` change them. |
| Author and Signing (signing choice, collected keys, mailmap) | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Keys are collected for the current format only.<br>CLI: Via `amonhen run` (`signingKeys`, `configSet`). |
| Commits: message template (none, shared, own, file) | ✓ | ✓ | ✓ | △ | △ | n/a | VSCode: None, this repository's own (edited there) or a file; no shared templates.<br>CLI: Via `amonhen run`. |
| Branches: default branch override | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: Via `amonhen run`. |
| Operations: pull, fetch.prune, autostash, updateRefs, conflict style, rerere | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: Via `amonhen run`. |
| rerere: forget one resolution (merge editor), forget all (Repository Settings) | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Forget Resolution on a Working Copy conflict that rerere resolved (VSCode's merge editor is its own); Forget All… under Repository Settings > Operations.<br>CLI: Via `amonhen run` (`forgetResolution`, `forgetAllResolutions`). |
| Working Copy: line endings, file mode, `.git/info/exclude` editor | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: Via `amonhen run`. |
| Remotes: table and sheet with a connection check | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: The table with a row of buttons (Edit URL…, Push URL…, Check, Prune, Rename…, Remove…) and Add…; the values are asked in input boxes, not a sheet.<br>CLI: Via `amonhen run`. |
| Hooks: table, Enabled switch, script, Create Hook… | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Open opens the script in VSCode's editor.<br>CLI: Via `amonhen run`.<br>Windows: Hooks can't be disabled on Windows (git runs them whatever their permissions); no Edit With |
| LFS: installed, patterns, Track Pattern…, file count | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: Via `amonhen run`. |
| Keys and Values (git config table, Basic / Customized / All, Combined / Levels) | ✓ | ✓ | ✓ | ✓ | △ | n/a | VSCode: Basic / Customized / All, Combined / Levels and search; values are edited in an input box.<br>CLI: Via `amonhen run` (`configList`, `configSet`). |
| Rows disabled with the git version needed | ✓ | ✓ | ✓ | ✓ | n/a | n/a | Windows: Fetching and Folders' rows (partial clone, sparse checkout, maintenance) |
| Repository's form on the git line (shallow, partial, sparse, maintenance) | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: `status` notes shallow, partial and sparse repositories. |
| Fetching and Folders: fetch more of a shallow clone (a count, up to a date, all) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run` (`deepen`, `unshallow`).<br>TUI: `+` in History. |
| Fetching and Folders: fetch all contents of a partial clone (git 2.36) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run` (`fetchAllContents`).<br>TUI: `+` in History, then `c`. |
| Fetching and Folders: folders of a sparse checkout picked from HEAD's tree (git 2.25) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: The folder is picked by walking HEAD's tree in a list.<br>CLI: Via `amonhen run` (`sparseCheckout`, `treeFolders`).<br>TUI: `W` in the Working Copy: the folders are typed, Tab completes them from HEAD's tree, empty checks out all. |
| Fetching and Folders: git maintenance switch (git 2.31) | ✓ | ✓ | ✓ | ✓ | △ | n/a | CLI: Via `amonhen run` (`maintenance`). |

## 5. Toolbar

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| New Orphan Branch (empty, or keeping the files) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: The Command Palette and the Branches group's right-click.<br>CLI: Via `amonhen run` (`createOrphanBranch`).<br>TUI: In the command list. |
| Branch switcher (fuzzy search, recent, tracking or new branch) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Switch Branch… (a QuickPick) in the sidebar's ⋯, the Branches heading and the command palette; no shortcut, as ⌘⇧B is VSCode's build task.<br>CLI: Via `amonhen run`.<br>TUI: `Ctrl-b` (recent first from HEAD's reflog, narrowed by typing, a tracking branch for a remote one, a new branch when none matches), or `c` on a branch in the sidebar. |
| Fetch sheet (all or one remote, prune shown) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: A modal with one remote, a QuickPick of all or one with several.<br>CLI: Via `amonhen run`.<br>TUI: `f` asks on the bottom line: `a` all remotes or a digit for one, with the prune setting named. |
| Pull sheet (commits, merge / rebase / ff-only, upstream when missing) | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: With the main view open, a sheet with each way to integrate and its drawing before and after, and the commits; otherwise a QuickPick. No conflict forecast.<br>CLI: Via `amonhen run`.<br>TUI: `p`; only when diverged, it names the files likely to conflict and offers merge or rebase beside git's setting; otherwise asks with the number of commits and up to three subjects when the upstream is ahead; with no upstream, the remote branch to pull from, made the upstream. Fast-forward only when only the upstream moved; the local-only commits are counted, not listed. |
| Push sheet (warnings, commits and diff, Force Push with lease) | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: With the main view open, a sheet with the drawing after the push (or force push), the warnings, the commits to send and the ones the upstream would lose, Pull… or Force Push with lease; otherwise a QuickPick with the same. All changes to send and each commit open their diff in the multi-file diff editor, and the sheet stays open. Force Push of a branch without an upstream publishes it, as Push does.<br>CLI: Via `amonhen run`.<br>Windows: A ContentDialog at 85% of the window, refitted when the window is resized, with the commits on the left and the selected one's diff on the right; it can't leave the window, be moved or be resized, as ContentDialog can't.<br>TUI: `P` asks with the number of commits and up to three subjects, and offers Force Push with lease and its drawing when the upstream is ahead; no diff. |
| Publish (choose the remote and name) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run`.<br>TUI: `P` on a branch without an upstream. |
| Merge (fast-forward only / no-ff / squash options) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the main view open, a sheet with each way to merge and its drawing; otherwise a list. The choice is the confirmation.<br>CLI: Via `amonhen run` (`mode`).<br>TUI: `m` in the sidebar. |
| Octopus merge of several (order, expected conflicts) | ✓ | ✓ | ✓ | ✓ | △ | △ | VSCode: In the order picked; Change the Order… picks them again one by one in merge order (no dragging in a QuickPick), and Put Last is offered when one before the last conflicts.<br>CLI: `merge-check` checks; the merge via `amonhen run`.<br>Windows: From History's selection: the order (dragged), expected conflicts per branch with Put Last, how to merge, one by one, the message, the before-and-after drawing, and Merge One by One when git refuses.<br>TUI: `m` on refs marked with `v` in the sidebar, in the order marked, naming how many commits each brings and its expected conflicts; `l` puts the first conflicting one last; no other reordering. |
| Rebase sheet drawing before and after | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the main view open, a sheet with the drawing; otherwise a dialog listing the commits now and after the rebase as text.<br>CLI: `rewrite-preview` prints it; the rebase via `amonhen run`.<br>TUI: `r` in the sidebar; the drawing lies over the screen. |
| Stash (message, include untracked) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run`.<br>TUI: `s`. |
| Stash: only the staged changes, keep the index, selected files | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: As in the app's sheet, Stash asks All changes or Only the staged changes, then Keep the staged changes and Include untracked files, which combine (the first two with the engine); a Working Copy file's right-click has Stash… for that file or the selected files, with the same choices.<br>CLI: Via `amonhen run` (`stashPush` with `staged`, `keepIndex`, `paths`).<br>TUI: `s` offers `k` keep the index, `s` only what is staged and, in the Working Copy, `f` only the selected file. |
| Delete ignored files (listed first) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: The Command Palette and the right-click on the sidebar's Working Copy row; the list is a pick list, all chosen at first.<br>CLI: Via `amonhen run` (`ignoredFiles`, `deleteIgnored`).<br>TUI: In the command list. |
| Pop the latest stash (asks first) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Pop Latest Stash… on the Stashes header and in the Command Palette; a row's Pop asks too.<br>CLI: Via `amonhen run`.<br>TUI: `S`, naming the stash and its message, as do `a` and `o` on a stash in the sidebar. |
| Activity spinner and count | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: VSCode's progress notifications.<br>TUI: The top line names the running operation. |
| Refresh and Rebuild History | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Rebuild History in the sidebar's ⋯ and the command palette, with the engine.<br>CLI: `log --rebuild`, `daemon rebuild`.<br>TUI: `Ctrl-r` refreshes and Rebuild History is in the command list; `Ctrl-l` redraws the screen without reloading. |
| Undo the last operation (asks, names what it puts back) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The title's Undo and the Command Palette ask; a notification's Undo, pressed right after the operation, doesn't.<br>CLI: `undo` (`--peek` to see it first).<br>TUI: `u`. |
| History search with scopes (subject, author, ID, path, contents) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; the ▾ before the field has the app's nine ways to search.<br>CLI: `log <query>`.<br>TUI: `/` in History. |
| Ahead/behind badges | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: On the sidebar's branch rows.<br>CLI: In `status`.<br>TUI: On the sidebar's branches and the top line. |
| Toolbar customization | ✓ | ✓ | △ | n/a | n/a | n/a | GTK4: The palette works like NSToolbar's and slides down under the header bar over the content like its sheet, without changing the window's size, with no Show (icon/text) choice since the header bar shows icons; the Repository menu and the main menu take the menu bar's place and stay put.<br>Windows: The palette slides down over the content like NSToolbar's sheet: drag items, spaces, flexible spaces or the default set onto the toolbar, drag along the toolbar to reorder, and drag back onto the palette to remove; Show picks icon and text, icon only or text only. A tile dropped elsewhere stays.<br>TUI: No toolbar; operations are keys. |
| Network git stopped after 120 s of silence | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The engine's, and the extension's own fetch and push without it.<br>Windows: The engine's.<br>TUI: The engine's. |

## 5a. Stashes

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Stashes view (list and contents) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Sidebar list; Show Stash opens VSCode's multi-file diff.<br>CLI: Listed by `refs`; `show stash@{n}`.<br>TUI: In the sidebar; Enter shows a stash's diff in the details. |
| Stash row details (branch, base against HEAD, untracked) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: In the description, as the app's row: the branch, the base against HEAD, the time, untracked files and the base's short ID (the base and untracked with the engine); a row's right-click has Copy Name.<br>CLI: Via `amonhen run` (`stashBases`).<br>TUI: On the bottom line when Enter shows the stash. |
| Apply, Pop, Drop… (undoable), Branch… | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Branch… with the engine; Apply and Pop restore the index (`--index`) when they can.<br>CLI: Via `amonhen run`.<br>TUI: `a`, `o`, `x` and `b` in the sidebar. |

## 5b. Reflog

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Reflog view with "not on any branch" | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine only.<br>CLI: Via `amonhen run`.<br>TUI: `3`; entries whose commit isn't in History are dimmed, and `L` on a branch shows its reflog. |
| Reflog actions (New Branch, Checkout, New Tag, Reset, Show in History, Copy) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run`.<br>TUI: `b` new branch, `c` checkout, `T` new tag, `R` reset, `H` Show in History and `Y` copy. |

## 5c. Reset

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Reset to a commit (Soft / Mixed / Hard, Hard stashes first, undoable) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine only.<br>CLI: Via `amonhen run`.<br>TUI: `R` in History or Reflog. |

## 5d. Range Diff

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Range Diff view (= ! < >, Changes Only, diff of diffs) | ✓ | ✓ | ✓ | ✓ | ✓ | △ | CLI: `range-diff`.<br>TUI: In the Blame and File History view, not as a page of its own; `c` for Changes Only. |
| Compare with Previous Version | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `range-diff --previous`.<br>TUI: In the command list. |
| Two History commits or a Reflog entry compared as two versions | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: Any ranges.<br>TUI: `=` with two commits selected in History, or on a Reflog entry (with the current HEAD). |
| Editable ranges | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: `r` in a range diff, typed as git range-diff takes them. |

## 6. Working Copy

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| One checkbox list (☑ ▣ ☐, header and folder checks) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Separate Staged and Changes groups.<br>TUI: `[x]` `[-]` `[ ]` per file; `+` stages every file (no folders). |
| List / Tree / File Name | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `v` in the list cycles them; a folder row stages what is under it, Enter folds it. |
| Filter on paths | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `/` in the list. |
| Per-row +/− counts and Discard on hover | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Counts need the engine.<br>CLI: `diff --stat`.<br>Windows: The ↶ stays on the last row the pointer was on.<br>TUI: The counts at the right of each row; ↶ on hover, or `d`, discards. |
| Space toggles, ⌫ discards | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: On every selected file (⌘- or Ctrl-click, Shift-click or Shift+↑↓ select several; the row buttons and Stash… act on them too).<br>TUI: Space and `d`. |
| Commit box on top with 50/72 counter and body guide | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: A description line over 72 is counted instead of a guide (the column is narrower than 72 characters).<br>TUI: Its heading counts the summary line (yellow past 50), and what goes past 50 in the summary or 72 in a body line is yellow. |
| Draft per repository, starting from the template | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: `commit.template` only, not the shared template library.<br>TUI: The draft is kept in `<git dir>/amonhen/commit-draft`; the template is `commit.template` (the repository's own one on SSH hosts too), not the shared library. |
| Recent messages (⌥↑ / ⌥↓, list) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: Alt-↑ / Alt-↓ in the message field, `R` in the list for the list. |
| Insert Conventional types, Co-authored-by, Signed-off-by | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `I` in the Working Copy. |
| `#issue` completion | ✓ | ✓ | ✓ | — | n/a | — |  |
| Commit and Push, Amend | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `commit -m … --amend`.<br>TUI: `Ctrl-p` commits and then asks to push; `a` turns amend on. |
| Fixup / Squash Commit For | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `F` in the Working Copy; Tab switches to squash!. |
| Hooks output sheet | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: The first line on the bottom line, and `!` shows all of it in a box. |
| Reason shown when Commit is disabled | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | In the button's tooltip.<br>TUI: On the bottom line. |
| Combined HEAD → working tree diff with line and hunk checks | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; without it, line staging on separate unstaged and staged diffs.<br>CLI: `diff --combined`, `stage --rows`. |
| Range selection in the gutter, J/K and ⌥J/⌥K | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: `v` starts a range; `[` `]` go to the hunk headers. |
| Unstaged / Staged switch for files that can't be overlaid | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: Always shown separately.<br>CLI: `diff --staged`.<br>TUI: The unstaged side first; `s` in the diff switches sides. |
| Diff options (unified/split, context, wrap) | △ | △ | △ | △ | n/a | △ | macOS, GTK4: Wrapping long lines only (Wrap Long Lines, for every diff, file contents and the merge editor); no side-by-side view.<br>Windows: The same.<br>VSCode: Wrapping long lines only (`amonhen.wrapLongLines`, Alt+Z or the ↩︎ above the diff; the merge editor is VSCode's own).<br>TUI: Wrapping long lines only (Alt-z or Wrap Long Lines in the command list, for this run); no side-by-side view. |
| Ignore whitespace (lines aren't staged or discarded meanwhile) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; the diff of the chosen side (Changes or Staged) meanwhile.<br>CLI: `diff -w`, `-b`, `--ignore-space-at-eol` (not with `--combined`).<br>TUI: `w` in the diff goes through them in turn; one side at a time meanwhile (`s` switches). |
| Discard lines, hunks and files (undoable) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `discard --rows`.<br>Windows: Click lines in the Working Copy diff to select them (Shift for a range, Ctrl to add or remove, a hunk header for its hunk), then right-click them for Discard Lines; right-clicking an unselected line or hunk header discards just that. |
| Add to .gitignore | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: With the engine.<br>TUI: `i` on an untracked file, then `p` for its path or `e` for its extension. |
| Conflicted files open the merge editor | ✓ | ✓ | ✓ | ✓ | n/a | n/a | macOS, GTK4: A file that isn't UTF-8 doesn't open in it; take one side.<br>VSCode: Opens VSCode's merge editor.<br>TUI: The file's diff shows the markers, and `o` `t` `b` `a` resolve the conflict under the line. |
| Note above the list when only some folders are checked out | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: `status` notes a sparse checkout. |

## History

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Graph with branch and tag labels | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Up to three labels with the rest as +N and long names cut in the middle, which list every name when clicked; the commit frequency and branch activity strip beside the list (with the engine), as in the app.<br>CLI: `log`.<br>TUI: One line per commit. |
| Huge histories in chunks with the persistent cache | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Search with match count, next / previous, matching only | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine.<br>CLI: `log <query>` lists the matches.<br>TUI: `/`, `n` `N`, `&` for matching only. |
| Display: order, one line, row items, fold merged, highlight ancestors | ✓ | ✓ | ✓ | ✓ | △ | △ | macOS, GTK4: Any merge folds on its own (a +N badge, its context menu, ←/→), kept per repository.<br>VSCode: Order, folding and highlighting with the engine. Any merge folds on its own (its context menu, ←/→), kept per repository and shared with the apps.<br>CLI: `--topo-order`, `--fold`.<br>Windows: In View.<br>TUI: `o` order, for the run only; `z` folds every merge and `Z` one, kept per repository and shared with the apps; `I` the row items, for the run only; always one line. |
| Avatars | ✓ | ✓ | ✓ | — | n/a | — |  |
| Signature marks on commits | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine.<br>CLI: In `show`.<br>TUI: The mark before the subject; no popover. |
| CI status per commit with a checks popover | ✓ | ✓ | ✓ | — | — | — |  |
| Worktree HEAD labels (✎ ⌂ ⑂, change counts) | ✓ | ✓ | ✓ | ✓ | — | ✓ | TUI: When there are two or more worktrees. |
| Go to HEAD, Parent, Child | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: The HEAD button above History, Go to Parent and Go to Child on a commit's menu, and parent and child links in the details (with the engine).<br>TUI: `@`, `]` and `[`. |
| Go to Ref | — | — | — | — | n/a | — |  |
| Several commits selected (range, two, scattered) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine.<br>CLI: `diff <rev> <rev>` for two.<br>TUI: `m`, Shift-↑↓, or Ctrl- or Alt-click. |
| History of several selected refs | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `log --branch` repeated.<br>Windows: Selecting a ref in the sidebar shows its History, and Ctrl+click adds or removes refs; View has All Branches and Current Branch.<br>TUI: `v` marks refs in the sidebar and History shows what they reach; Esc clears the marks. Opening a ref selects its commit. |
| Commit menu: checkout, new branch or tag, cherry-pick, revert, copy ID | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Copy Branch or Tag Name… for the refs on the commit; Cherry-pick on every commit, Split only with an older editable commit; Go to Parent / Child also ⌘⌥↓ / ⌘⌥↑ (Ctrl+Alt elsewhere). New Tag… also asks for a message, which makes an annotated tag (left empty, a lightweight one, the only kind the app makes).<br>CLI: Via `amonhen run`.<br>TUI: `c`, `b`, `T`, `y`, `v` and `Y` (copied to the terminal's clipboard with OSC 52). |
| Edit mode (pick, reword, squash, fixup, drop, edit; drag to reorder) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine; without it, one commit at a time. p, r, s, f, d and e choose a row's action, and the line under the rows is dragged to take in older commits or leave out unchanged ones; the edit mode is a list in place of History.<br>CLI: Via `amonhen run`.<br>TUI: `E`, over the screen; `J` `K` move a row. |
| Rewrite preview (stacked and pushed branches, expected conflicts) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: Through the daemon. Rebasing another branch gives the conflict estimate instead, as in the app.<br>CLI: `rewrite-preview`. |
| Shortcuts: Edit Message…, Squash into Previous…, Drop…, Combine N…, drag a commit | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine they open the edit mode; without it they rewrite at once, and there's no Combine N Commits. A commit dropped between rows moves and dropped on a row squashes into it (with the engine), in History and in the edit mode; a webview can't tell a drop outside it, so there's no dragging out to drop.<br>CLI: Via `amonhen run`.<br>TUI: `e` Edit Message, `F` fixup into the parent, `X` Drop and `E` on the selected commits Combine N; merge and rebase are `m` `r` in the sidebar; dragging a commit onto another asks to move or combine it. |
| Split a commit | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run`.<br>TUI: `\|` in History, asked first; it stops in the Working Copy. |
| Merges kept when editing (`--rebase-merges`) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine; without it, editing stops at the first merge.<br>CLI: Via `amonhen run`. |
| Shallow clone: band at the end to fetch more, boundary commits marked | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine.<br>CLI: `log` notes where the history stops; fetching more via `amonhen run`.<br>TUI: A line below the last row, ┄ before a boundary commit's subject; `+` in History fetches more (a count, up to a date, all). |

## 7. History's right column

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Full message, draggable panes, ⤢ to expand | ✓ | ✓ | ✓ | ✓ | n/a | △ | VSCode: URLs and commit IDs in the message are links, the trailers are listed apart, git notes follow (with the engine) and History's search terms are marked. The header's right-click copies the ID, the short ID, the subject, the message or a git show command, and has Edit Message… for an editable commit; the author's and committer's names search History. The bar's ⚙︎ View Options has Details Shown (Brief, Standard, Full, with Full's children, branches, tags, pushed state, merged and related commits), Fit Height Automatically, Show Only Commit Details (kept across commits) and Wrap Long Lines.<br>TUI: The dividers are dragged with the mouse, the widths kept per repository in `amonhen/tui.json`; no ⤢, and on a narrow terminal the details open in place of History. |
| Find inside the details | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: ⌘F searches the whole main view; diffs not loaded yet aren't searched.<br>TUI: `/` in the details, `n` `N` for the next and previous. |
| Changed files as the diff's own file headers (folded at first, sticky, one bar of controls) | ✓ | ✓ | ✓ | ✓ | ✓ | △ | CLI: `show`.<br>VSCode: The header's ⋯ opens its right-click menu; Open opens VSCode's diff editor.<br>TUI: The header of the file in view sticks at the top, and `o` or a click on a header folds the file; `[` `]` move between files. Generated and large files are folded at first; the changed files are also a list above the diff. |
| Changed files as a list or a tree whose folders fold | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | TUI: Above the diff; `t` switches to the tree, a click folds a folder (`z` all) or goes to a file's diff. |
| Widen a hunk's context, show a whole file | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: With the engine; the ⇕ on a hunk header widens the file by 20 lines, Show Whole File is on the file's ⋯.<br>TUI: `+` widens the file in view by 20 lines, `F` shows all of it. |
| Ignore whitespace (at line ends, changes in amount, all) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine.<br>CLI: `show -w`, `-b`, `--ignore-space-at-eol`.<br>TUI: `w` in the details goes through them in turn. |
| Compare images (side by side, swipe, onion skin, differing pixels) | ✓ | ✓ | ✓ | ✓ | n/a | — | VSCode: Images over 20 MB aren't compared. Zoom (−, %, +, Fit, ⌘-scroll) and the differing pixels' share as in the app; an SVG shows as a picture with Show as Text. |
| Light or dark checkerboard behind transparent pictures (default from the picture's colors) | ✓ | ✓ | ✓ | ✓ | n/a | — | |
| All Files: the tree at the commit and a file's contents with the commit's lines marked | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: With the engine. A list or a tree (the bar's List / Tree), changed folders open at first, a deleted file shows its version before, the path is followed by @ and the commit, Show This Change goes to its diff, and relative Markdown links open that file.<br>TUI: `a` in History, over the screen; no previews (the contents are shown as text). |
| All Files previews: zoomable images, SVG picture and mobile check, formatted Markdown, Quick Look for binaries | ✓ | ✓ | △ | △ | n/a | — | VSCode: Any file opens in VSCode's editor instead of Quick Look. The mobile check switches between the renderers, remembered, and the views of SVG, Markdown and VectorDrawables are remembered; Mermaid and DOT diagrams and math are drawn by the drawing helper bundled with the engine (`amonhen-render`; the engine itself draws nothing), the diagrams in the theme's syntax colors (a diagram's own theme comes first; there's no Colors setting), with gantt dates in the system time zone.<br>GTK4: GNOME's previewer (or the default app) instead of Quick Look; the SVG pictures are drawn by librsvg.<br>Windows: Zoomable images, SVG pictures with the mobile check, and formatted Markdown with its Mermaid and DOT diagrams (drawn with resvg, as Direct2D draws no SVG text); the renderer switch is in the path bar, math is drawn as on the other apps (inline formulas sit on the baseline), and binaries have no preview yet. Images in the commit, `<picture>`, `align`, image sizes (a width in percent is taken of the fixed text column, which doesn't follow the window's width) and `<details>` are drawn, and DOT image nodes show their pictures; an image inside a sentence stays in the line, `<kbd>` is a key cap, and `<sub>` and `<sup>` are lowered and raised. |
| Find in the formatted Markdown (key caps and table cells included) | ✓ | ✓ | ✓ | ✓ | n/a | — | Pictures and closed `<details>` aren't searched.<br>VSCode: ⌘F (Ctrl+F) once the Markdown has focus (click in it); elsewhere in the view ⌘F is VSCode's own find. |
| All Files: Android VectorDrawable pictures (colors from the commit's `res`, animated vectors as their still picture) | ✓ | ✓ | ✓ | ✓ | n/a | — | |
| File header menu (Blame, File History, copy path, open) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Expand / Collapse first, Open File at This Commit, and Show Changes Only after Show Whole File; the headers of selected commits get File History and Copy Path. Show Whole File, Blame, File History, Restore This Version… / the Version Before…, and Copy Path on the right-click and the ⋯ (all but Copy Path with the engine); Open in VSCode's diff editor. No Open File at This Commit.<br>TUI: `B`, `L`, `Y` (copy path) and `e` (open in git's editor) for the file in view. |
| Diff of a selection (range squashed, A..B / A...B, scattered) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: With the engine.<br>CLI: `diff <rev> <rev>`.<br>TUI: `m` and Shift-↑↓ (or Ctrl- or Alt-click) select; `x` swaps the direction and `.` diffs from the merge base. |
| Actions on a selection (cherry-pick, revert, squash, copy IDs) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: Copy Short IDs too; Save Patches is left out when a merge is selected, and Merge needs two commits other than HEAD. With the engine.<br>CLI: Via `amonhen run`.<br>TUI: `y` `v` on the marked commits, `E` opens edit mode with them combined, `Y` copies the IDs, and Save as Patch saves them. |
| Signature popover | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: A Signature row in the details (status, explanation, signer, format and key; the fingerprint in its tooltip), and Tag and Merged Tag rows with their tagger, subject and signature (with the engine); no popover.<br>TUI: The check in the details, no popover. |
| Word highlights and syntax colors | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Syntax colors with the engine, after the diff shows. |

## Blame / File History

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Blame | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: A page of the main view.<br>CLI: Via `amonhen run` (`blame`).<br>Windows: A line's menu shows its commit in History.<br>TUI: `B` on a Working Copy file or in a commit's details; `H` selects the line's commit in History. |
| Restore a file from a commit (this version, the version before; staged if chosen) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine, from a file's right-click in the details (History, Reflog, File History) and Restore This Version… on a File History row of the file's current name; Restore and Stage is the second button.<br>CLI: `restore --source <rev> [--staged]`.<br>TUI: `V` in File History: `y` this version, `b` the version before, `s` and `B` staged too. |
| File History | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: A page of the main view, opened from the Explorer, an editor tab, the Command Palette or a file in the main view. A row's right-click has the app's items (Copy Short ID too); Merge Commits is a checkbox in the bar, not a menu item.<br>CLI: Via `amonhen run` (`fileHistory`).<br>Windows: From a Working Copy file.<br>TUI: `L` likewise, and `O` asks for the path of an unchanged file. |
| Opened as a tab beside the repository tab, with the repository toolbar | ✓ | ✓ | ✓ | n/a | n/a | n/a | GTK4: When the repository tab is in front in another window, the tab gets a title bar without the toolbar.<br>TUI: Opens in place of the main view; Esc goes back. |
| Line history | — | — | — | — | — | — |  |

## 7a. Patches

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Save as Patch (per commit or mbox, cover letter, -v) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The form and the cover letter combine, and the last choice comes first.<br>CLI: `format-patch`.<br>TUI: The selected commit, or the marked commits oldest first, from the command list; a name ending in `.mbox` writes one file, `c` adds a cover letter and `v` asks for the version. |
| Apply Patch (am / apply, preview, 3-way, --index) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `am`, `apply` (`--check`).<br>TUI: From the command list; the preview is the subjects or files in the question, 3-way is offered when it doesn't apply, and `i` applies to the index too. |
| Drop patch files on the window | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: From the Explorer's context menu.<br>TUI: Apply Patch asks for the file's path. |
| Export Archive (zip / tar.gz / tar, top folder, one folder) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; a branch's or tag's right-click, a commit's in History, or the command palette for HEAD.<br>CLI: `archive`.<br>TUI: The selected commit, from the command list; the extension picks the format, then `y` a top folder, `t` none and `o` one folder (typed). |
| Save as Bundle (all, some refs, after a base) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; the selected branches' and tags' right-click, or the sidebar's ⋯ for the whole repository.<br>CLI: `bundle create`.<br>TUI: From the command list: everything, `b` the current branch only, or `a` the current branch after a base. |
| Clone from a bundle file (branch when no HEAD) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The sidebar's ⋯ and the welcome view; then open the clone, in a new window or in the workspace.<br>CLI: `clone <file> [-b <branch>]`.<br>TUI: A `.bundle` path typed in Quick Open; the branch picked with `1`–`9`. |
| Fetch from Bundle (under a name, prerequisites checked) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: With the engine; the sidebar's ⋯.<br>CLI: `bundle fetch`.<br>TUI: In the command list. |
| Stopped `am` with Retry with 3-Way, Skip, Abort, Continue | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: In a sidebar row.<br>CLI: `am --continue / --skip / --abort / --retry-3way`.<br>Windows: Windows: In the band above Working Copy.<br>TUI: `C` `K` `A`, and `R` for Retry with 3-Way. |

## 8. Merge editor

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Three columns with bands (Current / Result / Incoming) | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: VSCode's own 3-way merge editor.<br>Windows: Three columns; the side columns color each conflict (Current blue, Incoming green, Base gray, the selected one darker, resolved ones dimmed) with ≫ / ≪ to take that side, and all three scroll to the selected conflict.<br>TUI: The conflicted file's diff with its markers, resolved per conflict with `o` `t` `b` `a`. |
| Show Base | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: The base shows within the markers with `merge.conflictStyle` `diff3` or `zdiff3`. |
| Magic Merge (and the safe rules when a file opens) | ✓ | ✓ | ✓ | n/a | ✓ | △ | VSCode: Conflicts open VSCode's standard 3-way merge editor.<br>CLI: `merge-file` (also as git's mergetool).<br>TUI: `m` on a conflicted file; not run when a file opens, by decision: the TUI has no merge editor whose result is saved later, so it would rewrite the file just by opening it. |
| Editable Result with undo per conflict | ✓ | ✓ | ✓ | n/a | n/a | n/a | TUI: `e` opens the file in git's editor. |
| Resolve a file with Current / Incoming | ✓ | ✓ | ✓ | ✓ | △ | ✓ | CLI: Via `amonhen run`.<br>TUI: `o` `t` in the list. |
| Open in an external merge tool | — | — | — | n/a | n/a | — |  |
| Pop out the editor into a window | ✓ | ✓ | ✓ | n/a | n/a | n/a | Windows: A new window opens the same file; unsaved edits go with it. |
| Continue / Skip / Abort | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: On the sidebar's in-progress row; Skip and Abort don't ask first, as in the app.<br>CLI: Via `amonhen run`.<br>TUI: `C` `K` `A`. |

## 9. AI

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Claude Code and Codex | ✓ | ✓ | — | — | — | — |  |
| Apple Intelligence (on-device) | ✓ | n/a | — | n/a | n/a | — |  |
| Translate a commit message | ✓ | ✓ | — | — | — | — |  |
| Explain a commit or a selection | ✓ | ✓ | — | — | — | — |  |
| Suggest a commit message | ✓ | ✓ | — | — | — | — |  |
| Explain a diff hunk | ✓ | ✓ | — | — | — | — |  |
| Resolve a conflict with AI | ✓ | ✓ | — | — | — | — |  |
| Resolve every file with AI | — | — | — | — | — | — | Designed in docs/ux.md (chapter 9), not built: the merge editor resolves one conflict at a time. |
| Explain a conflict | ✓ | ✓ | — | — | — | — |  |
| First-time confirmation per repository and for repository prompts | ✓ | ✓ | — | — | — | — |  |
| Streamed and cached results | ✓ | ✓ | — | — | — | — |  |

## 10. Activity

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Operations (git commands, output, progress, cancel) | ✓ | ✓ | ✓ | △ | △ | ✓ | VSCode: An Output channel log only.<br>CLI: Via `amonhen run` (`remoteActivity`, `cancelActivity`).<br>TUI: `Tab` in Activity's operation log: the git commands and core commands where git runs (the daemon's through it), the selected one's output or error below, `x` cancels a running one, `r` reads again. |
| History (operation log, Undo to Here) | ✓ | ✓ | ✓ | △ | △ | ✓ | VSCode: Undo History… lists the Undo log and undoes down to the chosen one; no other activity.<br>CLI: `undo --list` shows the log.<br>TUI: Activity in the command list; Enter undoes back to the selected one. |
| Performance | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode's Output channel (the Amon Hen log).<br>Windows: Diff loads and History and status reloads.<br>TUI: History and status reloads and diff loads (count, p50, p95, max, last), after History Cache in Activity. |
| History cache writes (size, count, bytes, last write per repository) | ✓ | ✓ | ✓ | — | △ | ✓ | CLI: Via `amonhen run` (`historyCache`).<br>TUI: The open repository's, after Operations in Activity (`Tab`). |
| Events | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode's Output channel (the Amon Hen log).<br>TUI: After History Cache in Activity (`Tab`): the change notices and the reloads they cause. |
| Export as JSON | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode's Output channel (the Amon Hen log).<br>TUI: `s` in Activity's Operations, to a file on this machine. |
| Daemon tab (start, stop, connections) | ✓ | ✓ | ✓ | ✓ | △ | ✓ | VSCode: A page of the main view (Show Daemon in the sidebar's ⋯ and the command palette); the window's connections are to the local daemon only.<br>CLI: `daemon status / start / stop`.<br>TUI: After History Cache in Activity (`Tab`): `S` starts, `X` stops. |

## 11. Menus

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Menus from the shared command table (`crates/commands`) | ✓ | ✓ | ✓ | n/a | n/a | ✓ | VSCode: VSCode's Command Palette and context menus.<br>TUI: The command list (`:`) takes its names from it. |
| Every command reachable without a menu bar (≡) | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | GTK4: The ≡ menu holds the menu bar's File, Edit, View, Navigate, Commit and Window menus with macOS's items and order (Edit's Undo, Cut, Copy… act on the focused field), then Settings…, About and Quit.<br>Windows: The ≡ at the right of the title bar holds every menu of the menu bar as a submenu, with the same items and order, and Settings… (Ctrl+,) near the end of File as Windows apps place it; the window has no menu bar row.<br>VSCode: Command Palette and context menus; the menus' Commit, Commit and Push, Stage All, Rename Branch…, Set Upstream…, Rebase…, Go to HEAD and Find in History… are commands too, and Merge Branches… and Rebase… offer the commit selected in History.<br>TUI: `:` lists every operation the TUI has, with its key. |
| Keyboard shortcuts | ✓ | ✓ | ✓ | n/a | n/a | ✓ | GTK4: Keys that Linux desktops or other commands take differ: Show Sidebar is F9 (GNOME's utility pane key); Filter Branches, Go to Parent / Child and New Tag have none (GNOME takes Ctrl+Alt+arrows and Ubuntu takes Ctrl+Alt+T, and the HIG gives Alt+Up another meaning); the merge editor's previous / next conflict is Alt+Up / Down.<br>VSCode: VSCode's keybindings; the extension contributes only Alt+Z (Wrap Long Lines), and any of its commands can be bound.<br>TUI: Its own single keys, since the table's shortcuts can't be typed in a terminal. |
| Context menus with the menu's names | ✓ | ✓ | ✓ | ✓ | n/a | n/a | VSCode: The same words without the names the apps put in (Checkout… for Checkout main…, Delete… for Delete main…), since VSCode's menu items are fixed text.<br>Windows: History and its changed files, the sidebar, Working Copy, Stashes, File History and Blame.<br>TUI: Operations are keys on the selected row. |
| Keyboard shortcuts window | — | ✓ | — | n/a | n/a | ✓ | macOS: macOS's App Shortcuts change them instead.<br>TUI: `?` lists the focused pane's keys. |

## Status bar

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Operation in progress with Continue / Skip / Abort | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: A sidebar row; with the engine it says, as the app's band, what is being done where and the step, and why it stopped (the commit, conflicts, an exec, a stop to edit) with what to do next. No Skip for a revert, as in the app.<br>TUI: On the bottom line, with `C` `K` `A`. |
| Failure with its cause and Details | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: The notification says a known failure (sign-in, host key, network, rejected, diverged, local changes, a lock, no upstream, a worktree) in plain words with the app's next steps (Pull…, Open Working Copy, Set Upstream…, Open Worktree, Open in Terminal, Try Again), and otherwise git's fatal: and error: lines; Details shows the advice, the whole output and the git command in an output panel.<br>TUI: The cause on the bottom line, and `!` shows the git command and all of git's answer. |
| Lost connection with Reconnect | ✓ | ✓ | ✓ | n/a | n/a | ✓ | TUI: The bottom line; `R` opens the same location again. |
| What a finished operation did, for a few seconds | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: A status bar message or a notification with Undo.<br>TUI: On the bottom line. |

## Confirmations and Undo

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| A sheet before remote, HEAD-moving and deleting operations | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | GTK4: Name prompts and every confirmation are the same sheet. Return presses the default button wherever the focus is (a drop-down or a radio too), as on macOS.<br>VSCode: Modals, and QuickPicks for Fetch, Pull and Push; Switch Branch… switches at once, as the app's switcher does.<br>TUI: Asked on the bottom line; the before and after drawings lie over the screen. |
| Undo of git operations (`undo.json`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Disabled controls say why in their tooltip | ✓ | ✓ | ✓ | n/a | n/a | n/a | VSCode: Commands that can't be used aren't shown (when clauses). The sidebar title's Pull and Push can't be disabled with a tooltip, so they stay enabled (Push becomes Publish for a branch without an upstream), and pressing one with nothing to do says why in a message.<br>Windows: WinUI shows no tooltip on a disabled control, so disabled buttons and boxes carry theirs on a wrapper, and the toolbar's Push, which can't be wrapped, is drawn faded while it stays enabled (pressing it beeps); Commit says why.<br>TUI: A key that can't act says why on the bottom line. |

## 12. GitHub

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| GitHub through `gh` | ✓ | ✓ | ✓ | — | — | — |  |
| Pull Requests (CI, draft, review; check out, open, copy URL) | ✓ | ✓ | ✓ | — | — | — |  |
| Create Pull Request (base, draft) | ✓ | ✓ | ✓ | — | — | — | macOS: The base is typed (the default branch at first). From a fork, to its parent.<br>GTK4: The base is typed (the default branch at first). From a fork, to its parent.<br>Windows: The base is typed (the default branch at first). From a fork, to its parent. |
| Branch status popover (CI, pull request) | ✓ | ✓ | ✓ | — | — | — | Windows: Opens from the marks at the right edge of a branch's row (merged, gone, ahead/behind) and from the top of its right-click menu; a menu opened from the keyboard shows it as a dialog. |
| Open on GitHub | ✓ | ✓ | ✓ | — | — | — |  |
| `#123` links in messages | ✓ | ✓ | ✓ | — | — | — |  |
| GitHub avatars | ✓ | ✓ | ✓ | — | — | — |  |

## 13. Language

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| English UI with a Japanese translation | ✓ | ✓ | ✓ | — | ✓ | ✓ | VSCode: The extension's own text is English only; the engine's messages follow VSCode's display language. |

## 14. Worktrees

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Worktrees with change counts | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `worktree list --changes`.<br>TUI: In the sidebar when there are two or more. |
| The edited worktree (✎) and switching it | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Switches the repository the extension works on; one outside the workspace isn't added to the Source Control view.<br>CLI: Works on the worktree it runs in.<br>TUI: Enter on another worktree reopens the TUI there. |
| New Worktree… (from a branch or commit) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: `w` on a local branch, or in History with a new branch at the selected commit. |
| Remove Worktree (asks, `--force` with changes) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: `x` after asking, naming the changes it removes with `--force`. |
| Lock and unlock, move, locate a moved folder (repair) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The worktree's right-click; the worktree being edited moves only after switching to another.<br>CLI: `worktree lock`, `unlock`, `move`, `repair`.<br>TUI: `L` and `n` on a worktree in the sidebar. |
| Prune worktrees | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | TUI: In the command list. |
| A worktree's folder opens its repository with it edited | ✓ | ✓ | ✓ | ✓ | n/a | ✓ | VSCode: Opening the folder works on that worktree, with the others listed.<br>Windows: The folder opens in a tab with that worktree ✎ and the others listed. |

## SSH hosts, shared daemon, askpass

| Feature | macOS | GTK4 | Windows | VSCode | CLI | TUI | Notes |
| --- | :-: | :-: | :-: | :-: | :-: | :-: | --- |
| Repositories on SSH hosts (engine over ssh) | ✓ | ✓ | ✓ | n/a | ✓ | ✓ | VSCode: Remote-SSH runs the extension on the host.<br>CLI: `-C ssh://host/path`.<br>TUI: `amonhen tui ssh://host/path`. |
| Through the shared daemon | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | CLI: `-C daemon:<path>`.<br>TUI: `amonhen tui daemon:<path>`. |
| Engine sent to a host when none fits | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | VSCode: The bundled engine is used on the host the extension runs on.<br>Windows: Asks first, as in the other apps. |
| Windows SSH hosts | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| askpass dialog for passwords and passphrases | ✓ | ✓ | ✓ | ✓ | n/a | △ | VSCode: VSCode's input box, through a script the extension passes to git, ssh and the shared daemon.<br>CLI: The dialog belongs to the apps.<br>TUI: On the bottom line, on Unix only; on Windows it's left to git's credential helpers, ssh-agent and pinentry, and a failure says so. |
| Signature verification (this machine's keys or the host's) | ✓ | ✓ | ✓ | n/a | △ | ✓ | VSCode: The extension opens no `ssh://` repositories; under Remote-SSH it runs on the host, so git verifies there with the host's keys, as for any repository the extension opens.<br>CLI: Checked in `show` and `refs --verify`; no choice.<br>TUI: `S` in the details, per repository (kept in `amonhen/tui.json`); the signature line says where it was checked. |

## Terminal front end (TUI)

`amonhen tui` (`crates/tui`) has its column in the tables above; [tui.md](tui.md) has the details and what is left. Where a terminal can't do what the apps do, it has its own forms: operations are keys, and the command list (`:`) lists them by the menus' names in place of menus and context menus; confirmations, names and choices are asked on the bottom line in place of sheets, with the before and after drawings over the screen; and in place of tabs and windows, Enter on another worktree and Quick Open close the TUI and open it again there.
