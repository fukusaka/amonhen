# Changelog

User-visible changes, by release. The app and the engine (CLI and daemon) are released together with `v*` tags, and the
VSCode extension separately with `vscode-v*` tags, so each has its own section. Changes not released yet go under
`Unreleased`; on release, that heading becomes `### [x.y.z] - YYYY-MM-DD`, followed by a short summary paragraph that
becomes the GitHub Release notes.

## App and engine

### Unreleased

#### Added

- Tags can be pushed to a remote, deleted (undoably), replaced on a remote and deleted from a remote from their
  context menu in the sidebar, one at a time or several selected together.
- Fetch also reads which remote has which tag, and a tag row is marked when no remote has it or a remote has a
  different tag with the name; its tooltip and popover list every remote, and the menu offers only what applies.
- File History can list merge commits too (Show Merge Commits in the list's context menu, or Settings > Appearance >
  File History); they stay out by default.
- Blame colors the code by syntax, as diffs do.
- View > Show Tab Bar (⇧⌘T) shows the tab bar of a window with one tab, so its tab can be dragged into another
  window, and Window > Move Tab to Window ▸ moves the front tab into the window chosen (for File History and Blame,
  the repository's window comes first).
- File History marks where the file was renamed with a heading for each name, and its commits have a context menu
  (Show in History, Blame at the commit, copying the ID or path), with Return and ⌘C.
- The Pull sheet, the Merge… and Rebase… sheets and a branch's popover show which files would conflict before you
  merge, pull or rebase (git 2.38 or newer; for rebase it is an estimate). `amonhen merge-check` does the same.
- Settings > Git and each repository's Settings choose whether a rebase or a History edit moves the branches stacked
  under the current one along (git's `rebase.updateRefs`; git 2.38 or newer).
- Pull can follow git's own settings (`pull.rebase`, `pull.ff`), as `git pull` in a terminal does: "Follow git's
  settings" in Settings > Git, now the default, and in the Pull sheet, which says what those settings choose.

#### Changed

- The app, the shared daemon and an SSH host's engine work together only when they are the same build (a hash of the
  sources), not when a hand-raised protocol number matches; `amonhen --version`, `amonhen daemon status` and Activity
  show it.
- A development build looks for the engines to send to SSH hosts and to run as the shared daemon only inside its own
  checkout, and the shared daemon is not started from an engine of another build.
- Opening an SSH host uses an engine of the same build already placed there (by another Mac, the VSCode extension or
  `amonhen install`) without sending one, and still opens when the engine here is of another build.
- The History cache is kept per format, so builds that run side by side (and share the shared daemon's cache folder)
  don't rewrite each other's file; the first launch of this version builds History once more.
- The shared daemon runs git and ssh for each app, CLI or VSCode window with that one's environment (`PATH`, the SSH
  agent, `GIT_*`, the app's password dialog), not with that of whichever started the daemon first.
- `AMONHEN_PASS_ENV=NAME1,NAME2` adds environment variables that git and ssh get, through the shared daemon too; the app
  no longer takes `HOMEBREW_*` from the login shell.
- Settings > Advanced > Terminal Environment shows the git and SSH agent in use and the variables passed, lets you leave
  out or add names, and applies a change at once, to the shared daemon too.
- Repository Settings is a page at the end of the sidebar's Workspace instead of a sheet (⌘; still opens it), redone
  from the person using it: rows ask in plain words with a sentence of help under each, show the value in effect with
  where it comes from (git's default, your settings, this repository) and ↩︎ to stop overriding, and ⓘ for the key and
  file. Its pages are Author and Signing, Commits, Operations (pull, fetch, rebase, merge), Branches (the default
  branch, which Branches measures the others against: detected, or chosen for repositories without one), Working Copy
  (line endings, the executable bit, ignoring in this repository only), Remotes, Hooks · LFS and git config, with the
  git that runs the repository shown under the list. Settings that only a terminal honors stay in git config.
- Signing in Repository Settings is one choice, No signing / OpenPGP / SSH / X.509, and the key is chosen from the
  ones found (gpg's secret keys with the ones for your email first, `~/.ssh/*.pub` and ssh-agent's keys, gpgsm's
  certificates; on an SSH host, those on the host), with Other… for a key that is not listed.
- In Working Copy, an untracked file's context menu offers Add to .gitignore for the file and for its extension. The
  exclude editor in Repository Settings > Working Copy, and the editor of the repository's own commit template in
  Repository Settings > Commits, fill the window's height.
- Working Copy's commit message box can be made taller: a divider between the commit input and the file list is
  dragged, and its position is remembered; resizing the window changes the file list, not the message box.
- Settings > Git keeps a library of commit message templates shared by every repository. Repository Settings >
  Commits chooses a shared one (kept as `amonhen.commitTemplate` in the repository's git config; shown under the row
  with Edit in Settings > Git… and Copy to This Repository), the repository's own (`.git/amonhen/commit-template`,
  which `commit.template` points to so git in a terminal uses it too; edited under the row), or a file with a file
  chooser.
- git config in Repository Settings is a table like Xcode's Build Settings: one row per key, grouped by the key's dots, with the value in effect
  and a column per source (system-wide, all repositories, this repository, this worktree), the source in effect in
  bold. Double-click a row to set the key in this repository (empty removes it), + adds a key, − and ⌫ remove the
  selected keys, ⌘Z / ⇧⌘Z undo and redo every change made from the table, ⌘C copies the selected rows as `key = value`; search matches keys and values, and every column sorts. Above it, Xcode's switches: Basic
  (the keys the settings pages ask about, set or not), Customized (keys set in this repository, the default) or All (every variable
  git knows, blank when set nowhere), and Combined (the value in effect and its source) or Levels (a column per source).
- Fetch automatically is one interval for all open repositories, in Settings > Git, instead of a setting per
  repository.
- `.gitignore` is no longer edited in Repository Settings (it is a file of the repository); `.git/info/exclude` is,
  under Working Copy.
- Fetch removes remote-tracking branches deleted on the remote only when `fetch.prune` is on, as `git fetch` does;
  it used to always remove them.

- File History and Blame open in a tab next to the repository's tab instead of a separate window, with the same
  toolbar, so switching tabs keeps the toolbar and the size (drag the tab out for a window).
- File History opens at once with the newest 100 commits and adds the rest as they are read.
- File History is much faster in large repositories (a file in the linux kernel repository: 86 s → 1.6 s): it
  follows renames itself instead of using `git log --follow`, so history simplification and commit-graph's changed-path
  Bloom filters apply.
- With git's `rebase.updateRefs` set to true (git 2.38 or later), editing History moves the branches stacked under the
  current one along, as a rebase does. Undo puts back the branches a rebase or a History edit moved.
- Undo restores the branches that are not checked out in one transaction, so it changes all of them or none.
- Amon Hen needs git 2.23 or later (also on SSH hosts), as documented now; when a git command fails with an older
  git, the error says so.
- With Settings > Performance > Write commit-graph automatically, a repository that already has a commit-graph gets
  the new commits added to it (and their changed-path Bloom filters) when it is opened and after each fetch or pull.

#### Fixed

- Opening an SSH host and git over SSH no longer fail with "Permission denied (publickey)" after the Mac restarts:
  the app kept the SSH agent from an earlier login, and now checks the agent each time it starts ssh or git.
- When an SSH host refuses the public key and no SSH agent can be reached, the error says so, and Settings > Advanced >
  Terminal Environment shows whether the SSH agent is there.
- Blame of uncommitted contents and File History read the worktree chosen in Working Copy, not the repository's main
  checkout.
- Writing commit-graph in the background also works with git older than 2.27, without the changed-path Bloom filters.
- Undoing a branch rename works for the checked-out branch, and keeps the branch's settings (upstream, description,
  Archive and Favorites).
- Commit details, worktrees, repository settings and the signature's trust level work with git older than 2.36.
- Folders in the sidebar (`/` folders of branches and tags, and the remotes under Remotes) stay open when a fetch, a
  push or another change to the refs rebuilds it, instead of folding up each time.

### [0.2.6] - 2026-10-23

History and Reflog rows can reset the current branch to a commit (Soft, Mixed or Hard, all undoable), and a Reflog
view in the sidebar's Workspace brings back commits left behind by a reset, rebase or amend. Repositories on SSH hosts
or through the daemon get their tab at once and are restored at launch, and Windows gets a one-line installer for the
command line tool.

#### Added

- Reflog in the sidebar's Workspace opens a view of where HEAD has been, marking commits no branch reaches any more
  (left behind by a reset, rebase or amend), with New Branch… and Checkout to bring them back (View > Reflog ⌘5).
- Reset ▸ Soft, Mixed or Hard in the context menu of History and Reflog rows moves the current branch to that commit,
  after asking; Hard keeps uncommitted changes as a stash first, and all three can be undone.
- `scripts/install-engine.ps1` installs or updates the command line tool on Windows in one line (with the attestation
  checked when the GitHub CLI is logged in).

#### Changed

- A repository on an SSH host or opened through the daemon gets its tab in place at once, showing that it is
  connecting; if connecting fails, the tab stays with the error and Try Again instead of an alert, replacing the
  separate connecting window.

#### Fixed

- Clicking the Dock icon with no window open shows the repository page (or brings back minimized windows) instead of
  doing nothing.
- Repositories on SSH hosts or opened through the daemon are restored at launch in their windows and tab order, and
  one that could not connect is still reopened next time.
- Working Copy and Workspace in the sidebar stay folded when you fold them, instead of opening again whenever the
  repository changes outside the app.

### [0.2.5] - 2026-10-22

Stashes moved into the sidebar's Workspace and open as a view of their own, showing where each stash's base stands
against HEAD, with Branch… to make a branch from one. Bookmarks are no longer lost when more than one copy of the app
runs, and branches are nested correctly when the app is opened from a linked worktree.

#### Added

- Stashes shows where each stash's base stands against HEAD (in orange when it is not in HEAD's history), and
  Branch… creates a branch from a stash (`git stash branch`).

#### Changed

- Stashes moved into the sidebar's Workspace as one row that opens a Stashes view: the stashes with their branch, base
  and date on the left, the selected one's changes on the right, and Apply, Pop and Drop (View > Stashes ⌘4).
- The main worktree always stays at the top of the sidebar's worktrees, instead of being folded into "No changes" when
  it has none.

#### Fixed

- The repository page no longer stalls for a moment whenever a scan finishes: when only the counts change, the left
  side is redrawn instead of rebuilt.
- Bookmarks, groups and folders to scan are no longer lost when more than one copy of the app runs (the stable
  release, the snapshot, a development build): each change now rereads the file and writes on top of it.
- Opened from a linked worktree (`git worktree add`), the sidebar still nests a branch under the base it was created
  on when the base has moved on since: where a branch was created is now read through the git backend, which finds
  the reflog in the shared git dir. The remote protocol is now version 23 (the stash bases took it to 22).

### [0.2.4] - 2026-10-22

The sidebar nests branches under the one they are built on more reliably: a branch forked from the default branch no
longer lands under an unrelated pushed branch, and pushed branches are stacked too, so a stack of pull requests shows
as a stack.

#### Changed

- A pushed branch built on another one is nested under it in the sidebar too (↳), so a stack of pull requests shows
  as a stack. Which of two branches is the base is read from where each was created (its reflog) when the base has
  moved on since, and only a branch's own line (first parents) counts, so an integration branch is not nested under
  what it merged.

#### Fixed

- A branch forked from the default branch is no longer nested under an unrelated pushed branch in the sidebar (nor
  "Built on" it in the status popover). A pushed branch counts as the base only where its own commits, not the default
  branch's, are in the history, and the walk stops at the first base it reaches.

### [0.2.3] - 2026-10-21

History rows sit right next to the graph, and what a row shows and the commit order (date or topological) can be
chosen. Push shows what it sends before sending, and a pull request is created from the branch once it is pushed.
HTTPS pushes no longer hang in Git Credential Manager, and the launch crash of 0.2.2 is fixed.

#### Added

- A filter field at the top of the sidebar (⌥⌘F) narrows branches, remotes and tags by name.
- Several branches or tags can be selected in the sidebar: History shows the commits reachable from any of them, and
  the context menu copies their names, or adds them to Favorites, archives or deletes them together (Delete asks first).
- Kotlin (`.kt`, `.kts`) is highlighted in diffs.
- The Push confirmation is a window of its own (movable, resizable) that shows what it sends: warnings worth a look (straight to the default branch, fixup!/WIP commits,
  files that may hold secrets, binaries added), the commits on the left, and on the right the message, files and
  diff of the selected commit or of all changes together, as in the detail column.
- Clicking a commit's CI mark in History, or a pull request's in the sidebar, opens a popover listing its checks
  (result, name, duration), each linking to its log, with a button to the checks page on GitHub. Re-runs are
  collapsed to one row per check: a success hides the cancelled runs before it, and a check that ended cancelled
  shows as cancelled, not failed.
- View > History Items chooses what each row shows besides the subject: branches and tags, author, date, commit ID,
  signature and CI status. With none of author, date and ID, rows are one line.
- View > History Order switches History between date order and topological order, which keeps a branch's commits
  together under the merge that brought them in. The CLI has `amonhen log --topo-order`.
- Both are also in Settings > Appearance and in the menu of the new button at the right of History's bar (and the
  header's context menu), which also has One Line per Commit, Fold Merged Branches and Highlight Ancestors.
- Create Pull Request… is also in a local branch's context menu and in the branch's status popover (once the branch
  is pushed; an open pull request is shown and opened instead), and can start from a branch other than the current
  one. A branch not pushed yet gets the Push confirmation window instead of a plain alert.

#### Changed

- Highlighting picks the language by the shebang when the file name says nothing (`#!/usr/bin/env python3`, `#!/bin/sh`
  and so on), treats a `.h` that looks like C++ as C++, and knows more file names (Cargo.lock, Podfile, BUILD, …).
- Deleting a branch asks first. A branch with commits that are in neither the current branch nor its upstream lists
  them and offers Delete Anyway (git refused and nothing happened before). Dropping a stash asks too.
- In History, each row's message starts right after that row's graph (its dot and the lines passing it) instead
  of at a fixed 120 pt, and the graph column is only as wide as the visible lanes need.

#### Fixed

- Push, Pull and Fetch over HTTPS no longer hang in Git Credential Manager when run from the app. git inherited the
  app's blocked SIGCHLD; git and ssh started by the engine now begin with an empty signal mask.
- The app no longer crashes at every launch after the first (the shell environment's background read ran on the
  main actor's behalf). In 0.2.2, turn off "Take the login shell's environment at launch" to work around it.

### [0.2.2] - 2026-10-20

Fetch, Pull and Push work from the app as they do in a terminal: the app takes the login shell's environment at
launch, so Homebrew's git, credential helpers and SSH agents are found, and a dialog asks for a password or passphrase
when git needs one. Failures say what failed, why, and what to do next, and the bar for a rebase, merge, cherry-pick or
revert says what is going on and where it stopped. The sidebar's branches are arranged for getting around: Favorites,
the current branch with the branches it is built on, branches nested under the one they build on, marks for merged and
gone branches with an explaining popover, and Inactive and Archived folded away. History follows branches that other
programs rebase or commit to instead of going empty, the Pull sheet remembers the chosen way, and Working Copy shows a
file's line endings when they matter. The remote protocol is now version 21.

#### Added

- Working Copy shows a file's line endings beside its diff when they matter: `CRLF → LF` when git converts them
  (`core.autocrlf`, the `text` and `eol` attributes), and a warning when they are mixed or CRLF where checkout would
  write LF, which the next commit or checkout may change for the whole file. The tooltip names the conversion.
  `amonhen diff` prints the same under the file's header, and `--json` has `lineEndings`. The remote protocol is now
  version 21.
- `scripts/install-app.sh --user` installs the app into your own Applications folder (`~/Applications`); `--system`
  picks `/Applications`, and `--dir <folder>` any folder. Without any of them, an update replaces the app where it is
  already installed.
- Branches in the sidebar are arranged for getting around: Favorites (★, in the order you choose by dragging) first,
  then the current branch together with the branches it is built on and the default branch, then the active branches
  newest first with `/` names in folders, and at the end Inactive (no commit for 90 days) and Archived, both folded.
  A branch built on top of another one is nested under it with a ↳ mark, and a branch already merged into the default
  branch shows a merge mark at the right, next to the mark for a gone upstream; clicking those marks opens a popover
  that explains them, shows the branch's upstream, base and last commit, and offers Delete Branch (only when merged)
  and Archive. Add to Favorites and Archive are in a branch's menu; both are kept in
  git's config (`branch.<name>.amonhenPinned`, `branch.<name>.amonhenArchived`), so they follow a rename and go away
  with the branch. `amonhen run '{"op":"branchGroups"}'` gives the same facts (`merged`, `parent`, `pinned`,
  `archived`, `mine`, the tip's `time`).
- Selecting a branch that has diverged from its upstream shows both in History, so the commits ahead and behind are
  in the list.
- `amonhen run` answers a failed command with `errorKind` (`worktreeInUse`, `auth`, `hostKey`, `network`, `rejected`,
  `diverged`, `localChanges`, `locked` or `noUpstream`) and `errorPath` when git's message tells what went wrong, and
  `{"op":"sequence"}` has `info` with the branch, where it is going, the step and the commit it stopped at.

#### Changed

- The Pull sheet starts with the way you pulled last time (merge, rebase or fast-forward only), and the note under it
  always explains the chosen way, not only when your branch and upstream have diverged.
- A failed operation names what failed (Pull Failed, Couldn’t Switch…), says why in plain words instead of git's first
  line, and offers the next step: open the worktree that has the branch checked out, Open in Terminal for sign-in
  problems, Try Again, Pull before Push, Open Working Copy or Set Upstream. Details adds how to fix it.
- The bar shown during a rebase, merge, cherry-pick or revert is taller and tinted, and says what is going on
  ("Rebasing topic onto main · step 2 of 5"), where it stopped and why (conflicts, a commit to edit, a failed command)
  and what to do next.
- Checkout in the sidebar's menu, and Rebase when dropping onto a branch, open the worktree that has the branch
  checked out instead of failing.

#### Fixed

- Fetch, Pull and Push now work from the app as they do in a terminal. Launched from the Finder or the Dock, the app
  had launchd's bare environment, so Homebrew's git, credential helpers and SSH agents such as 1Password were not
  found; it now takes `PATH`, `SSH_AUTH_SOCK`, `GIT_*` and the like from your login shell at launch (cached, refreshed
  in the background; Settings > Advanced > Terminal Environment shows what was taken, reads it again, and turns it
  off; `AmonHen --print-environment` prints it). When git or ssh needs a password, a token or a key passphrase, a
  dialog asks for it instead of failing with "terminal prompts disabled".
- History no longer goes empty when another program (a terminal, an editor) rebases, amends or commits while it shows
  the current branch or a branch picked in the sidebar: it follows the branch to its new commit. Refreshing after such
  a change keeps the scroll position, loads the visible rows before swapping so the list does not blank out, cross-fades
  the two, and leaves the commit details alone when the selected commit is the same.
- A rebase, cherry-pick or revert finished by another program no longer stays shown as in progress (most often when a
  hook or autostash ran at the end).

### [0.2.1] - 2026-10-19

Worktrees live in their repository's tab, listed with their changes under Working Copy and edited one at a time, also
over SSH with many worktrees. Pull, Push, Pop and Undo show what will happen before running, repositories are chosen on
a page that opens as a tab, and the app and engine speak Japanese. New: a GTK4 app for Linux (early), Apple's
on-device model for AI, folders to scan on SSH hosts, and correct line endings in Windows diffs. The remote protocol is
now version 20; the app sends the matching engine to SSH hosts.

#### Changed

- Repositories are chosen on a repository page that opens as a tab (⌘T, ⌘N, ⌘O, ⌘⇧R), instead of a separate window.
  Categories on the left (All, Favorites, This Mac and each SSH host with their folders to scan and other folders,
  groups) and
  each repository once on the right, with its branch, status, last commit and when it was last opened. The toolbar has
  a Repositories button for the page, and the repository name next to it opens the same list to switch the current tab.
- Pull, Push, Pop and Undo no longer run on a single click. Each opens a sheet with what will happen: the commits to
  send or bring in, how Pull integrates them (merge, rebase or fast-forward only, chosen there) and what that does,
  uncommitted changes in the way, the stash or operation being undone. Push suggests Pull when upstream has commits
  you lack, and Force Push lists the commits upstream would lose. The ▾ menus on Pull, Push and Branch are gone.
- Discarding a whole file in Working Copy asks first.
- No more doubled names: History hides its own search field while the toolbar has one, and the window subtitle leaves
  out the branch while the toolbar shows it. The Summary field spans the commit box, with its length next to Commit.
- Sidebar headings show how many items they hold and open or close with a click, so a closed Remotes or Tags no longer
  looks empty.
- On SSH hosts, paths under the home folder are shown with `~`, so `~/prj` and `/home/me/prj` sit in one tree.
- The repository page: folders under a place no longer count repositories already under a folder to scan, the status
  column says "changes n" and explains itself, and empty Groups are hidden.

#### Added

- A GTK4 app for Linux (`amonhen-gtk-<version>-linux-<cpu>.tar.gz`, early): History with the graph, search and
  multi-select, commit details and diffs, Working Copy with line staging, the sidebar and its operations, the macOS
  app's menus in the header bar, the merge editor, Blame, File History, Activity and Settings, opening repositories
  locally, through the shared daemon or over SSH.
- The app is in English and Japanese, following the system language. git's operations and the app's screens keep
  their English names (Commit, Push, Working Copy, History, …) in Japanese too. English counts read "1 commit",
  not "1 commits".
- Messages from the core, the CLI and the daemon are in English and Japanese too, in the language of whoever shows
  them: the app's UI language, the CLI's `LC_ALL` / `LANG`, and for the shared daemon and SSH hosts the language each
  client names when it connects (the remote protocol is now version 20, so SSH hosts need the matching engine; the app sends it).
- Before sending an engine to an SSH host, the app checks that the bundled engine speaks its protocol, and says so
  instead of sending an old one that the connection would then refuse.
- AI can use Apple Intelligence's on-device model (macOS 26): nothing leaves the Mac, and it is the default where
  available. Claude Code and Codex models are chosen from a pop-up (Codex lists the models its CLI knows).
- Worktrees live in their repository's tab, the way git sees them. The sidebar lists them under Working Copy with their
  changes; picking one makes it the worktree you edit (✎ next to the repository name), and Working Copy,
  commits, checkout, Pull, Push, stash and Undo act there, while History and branches stay shared. A branch checked out
  in another worktree takes you to that worktree. New Worktree…, Remove Worktree and Prune Worktrees; confirmation
  sheets name the worktree. In the sidebar the main worktree comes first, then
  the rest by name; worktrees with no changes are folded into "No changes", and worktrees whose folders share a name,
  like `build/aslr/src` and `build/ssp/src`, show as `aslr/src` and `ssp/src`. History labels each
  worktree's HEAD with its name and number of changed files. The current branch is the edited worktree's; branches checked out in
  other worktrees are marked in Branches, History and the branch switcher. Opening a worktree's folder opens its repository's tab, and the repository page no longer
  lists worktrees as repositories of their own.
- Tags are grouped into folders by `/` like branches (`backup/…`, `archive/…`), and clicking a folder or a remote in
  the sidebar opens or closes it.
- Copy a branch or tag name from the sidebar's right-click menu, or from a commit's menu in History.
- `amonhen worktree` (list, add, remove, prune), the same as `git worktree`, through the core (so over SSH too);
  `list --changes` adds each worktree's number of changed files.
- Folders to scan on SSH hosts: the engine on the host finds the repositories (`amonhen find-repositories`). Add one
  from "+" or the Places menu for any host, or from a listed host's menu.
- Choosing an SSH folder: adding a folder to scan or an SSH connection suggests hosts from `~/.ssh/config` and
  `~/.ssh/known_hosts` and lists the folders on the host (double-click to go in, repositories in green), instead of
  only typing the path.

#### Fixed

- Opening an SSH repository with many worktrees no longer fails with "Session open refused by peer", and no longer
  keeps an engine and a connection per worktree: the opened repository counts every worktree's changes in one request
  (`worktreeChanges`), only the edited worktree and the two before it stay open, and past five
  engines per host each gets its own SSH connection (sshd allows only a few sessions per connection; short commands it
  refuses are retried on their own). The engine watches the worktrees' HEAD and index (not their files) and counts
  again only the ones that changed, plus those with changes every 30 seconds. With 59 worktrees on a Linux host this went from 59 engines to
  2; counting all of them takes under a second, and asking again when nothing changed runs no git.
- History no longer crashes when showing only the commits that match a search scrolled down past the matches.
- Fetch, Pull and Push no longer hang forever when git waits for credentials nobody can see: after 120 seconds with no
  output they stop and say so.
- The repository and branch names in the toolbar have a fixed width, so the buttons after them stay put when switching
  tabs.
- The repository list no longer jumps back to the top while scrolling (each row's branch and change count used to
  rebuild the whole list as it arrived).
- Line numbers, file headers and stage checkboxes in diffs no longer come out garbled after the window moves
  between screens of different scale (1x and 2x).
- Building History no longer spins up threads for every few commits. On the Linux engines (musl) it was 5 times
  slower than it should be; on git/git it now takes about 0.1 s instead of 0.6 s there, and about 20% less elsewhere.
- On Windows, the shared daemon no longer flashes a console window each time it runs git or ssh (for example on
  every branch selected in the VSCode extension).
- Working Copy diffs follow git's line ending conversion (`core.autocrlf`, `core.eol` and the `text` and `eol`
  attributes). On Windows, where git checks files out with CRLF, every changed file used to show as entirely rewritten,
  and staging lines put CRs into the index. Discarding lines changes only those lines, leaving the line endings of the
  rest of the file as they were.

### [0.2.0] - 2026-10-17

The first release signed with Developer ID and notarized. The app is reworked around a menu bar, tabs and bookmarks,
with History search and folding, a three-pane merge editor with Magic Merge and AI assistance. New: the `amonhen` CLI,
a shared daemon for the app, the CLI and the VSCode extension, and engines for Linux and Windows.

#### Added

- Menu bar with every operation, a Settings window and a reworked toolbar with a branch switcher and History search.
- Repositories open as window tabs, and the tab layout is restored. Bookmarks with groups, scanned folders, colors and
  Quick Open; repositories opened from outside go to the window used last.
- Working Copy: commit message first, changes with checkboxes, staged lines marked in one diff, a flat file layout,
  line counts, drafts, recalled messages and trailers, `J` / `K` to move between hunks and files.
- History:
  - Fold merged branches, and show commit density beside the scroll bar.
  - Search from an in-memory index of subjects and authors; step through matches, or show only matching commits.
  - Compare two selected commits (also in reverse or from their merge base), act on several commits at once, and
    squash a contiguous range into one.
  - Commit details: all files in one diff with sticky headers and Viewed marks, a file tree, find in the details.
  - GitHub CI state at the right end of each row, and GitHub avatars (both can be turned off).
  - Rebuild History.
- Three-pane merge editor with Magic Merge (safe rules, joining added imports, one conflict at a time) and a banner for
  a merge or rebase in progress.
- Clone from a URL or from your GitHub repositories. List pull requests and create them through the GitHub CLI.
- AI (Claude Code or Codex): translate, explain a hunk, suggest commit messages and resolve conflicts, with prompts you
  can try on sample input.
- Repository settings: identity, branches, remotes, ignore files (with the files each pattern matches), general, hooks
  and LFS.
- Pull can merge, rebase or fast-forward only. Git progress is shown, network operations can be cancelled, and
  failures are reported in a banner.
- Activity window: operations, the undo log, timings, events, the shared daemon's state and the engine used on SSH
  hosts.
- App icon, with a separate one for snapshot builds.
- `amonhen` CLI: open in the app, a graph log, status, diff, show, stage, discard, commit, refs, watch, clone, run,
  undo and Magic Merge for files (`merge-file`, usable as git's mergetool). Everything takes `--json`.
- Shared daemon (`amonhen --serve`): the app, the CLI and the VSCode extension share one History, search index, undo
  log and SSH connection per repository. Unix domain sockets, and named pipes on Windows.
- Engines for macOS arm64, Linux x86_64 / aarch64 (static musl) and Windows x86_64 / aarch64 on each release.
- `amonhen install` places the engine in one location per machine (`~/.local/share/amonhen`, Windows:
  `%LOCALAPPDATA%\amonhen`) and links `~/.local/bin/amonhen`. On SSH hosts the engine is provisioned the way VS Code
  Server is.
- Install scripts that need only `curl`: `scripts/install-app.sh`, `scripts/install-engine.sh` and
  `scripts/install-vscode.sh`. Running them again updates in place, and `install-app.sh` tells you when the app is
  still running the old version.
- The app, the engine archives and the `.vsix` include the licenses and a list of the third-party licenses built into
  them.
- Releases are signed with Developer ID and notarized (snapshots are signed only), and every file carries a build
  provenance attestation.

#### Changed

- The CLI and the daemon are one executable, `amonhen`.
- The engine reports the version it was released as, so `amonhen install` can tell snapshots from releases.
- Licensed under MIT OR Apache-2.0.
- The docs are in English, with Japanese versions in `README.ja.md` and `docs/ja/`.

#### Fixed

- Text and graphs are redrawn when a window moves between screens of different scale.
- On Windows, cancelling stops git together with its children.

### [0.1.0] - 2026-10-16

First release.

- History drawn with Metal: graph layout computed incrementally, a persistent cache so it shows right after launch
  even on very large repositories, ancestor highlight of the selected commit, fisheye and lane animation.
- Commit details in compact, standard or detailed style: signatures checked in batches with a trust indicator, author
  and committer with avatars, merged tags, push state, related commits, rewording in place.
- Sidebar with branches, remotes, tags, stashes and submodules, updated through FSEvents.
- Diffs drawn with Metal and colored by syntax (tree-sitter), with text selection, copy and search.
- Working Copy: file, hunk and line-level staging and commits. Status uses the untracked cache and fsmonitor.
- Checkout, merge, rebase, push and fetch, and commit editing (reword, squash, fixup, drop, reorder, split) with drag
  and drop. Every operation can be undone.
- Conflict resolution per conflict and per file in a three-pane merge editor.
- Repositories on SSH hosts through a daemon on the host.

## VSCode extension

### Unreleased

#### Changed

- The shared daemon runs the extension's git and ssh with the extension's environment (`PATH`, the SSH agent,
  `GIT_*`), even when the app started the daemon.

### [0.2.3] - 2026-10-21

The bundled engines are the 0.2.3 ones and still speak remote protocol 21, so the extension shares one daemon with the
0.2.3 app and CLI. Nothing changed in the extension itself.

### [0.2.2] - 2026-10-20

The bundled engines are the 0.2.2 ones and speak remote protocol 21, like the 0.2.2 app and CLI, so all three share
one daemon again. The Working Copy diff shows a file's line endings when they matter.

#### Added

- The Working Copy diff shows the file's line endings when they matter: `CRLF → LF` when git converts them, and a
  warning when they are mixed or CRLF where checkout would write LF. The tooltip names the conversion.

### [0.2.1] - 2026-10-19

Fixes for Windows: no more console windows flashing, and Working Copy diffs of files checked out with CRLF show only
the changed lines. The bundled engines speak remote protocol 20 like the 0.2.1 app and CLI, so they share one daemon
again. The engine's messages follow VSCode's display language.

#### Changed

- Messages from the engine (errors and the like) are in VSCode's display language: Japanese or English.

#### Fixed

- On Windows, git and the engine no longer flash a console window when the extension runs them.
- Working Copy diffs and line staging follow git's line ending conversion (`core.autocrlf`, `core.eol` and the `text`
  and `eol` attributes), with or without the bundled engine. On Windows, changed files no longer show as entirely
  rewritten, and staging lines no longer puts CRs into the index. Discarding lines changes only those lines.

### [0.2.0] - 2026-10-17

The first stable release, versioned with the app. History, diffs and operations run on the bundled Amon Hen engine
through the daemon shared with the app and the CLI. Adds discarding changes, stashes, creating branches and tags,
cherry-pick and revert.

#### Added

- In the Working Copy, discard unstaged changes by file, hunk or line (Discard). Discarded content can be restored with
  Undo.
- Create branches and tags (New Branch / New Tag) and rename branches, from branches and tags in the sidebar and from
  right-clicking a commit in the History.
- Save, apply, pop and drop stashes, and show their contents (including untracked files). Dropping can be undone.
- Cherry-pick / Revert by right-clicking a commit in the History.
- Update submodules and open them in another window.
- The Amon Hen engine (shared with the app; bundled for macOS arm64, Linux x86_64 / aarch64 and Windows x86_64 /
  aarch64) runs History (graph, commit details and diffs), Working Copy diffs, line-level staging and discarding, and
  operations such as commit, branches and tags, merge, rebase, cherry-pick, revert, stash, fetch and push through the
  shared daemon. It works without any setup: the engine is placed in `~/.local/share/amonhen` and links
  `~/.local/bin/amonhen` (the CLI). The app, the CLI and the extension share the same daemon, History, search index and
  undo log. It can be turned off with `amonhen.daemon.enabled`, and on an OS and CPU without a matching engine the
  extension runs on its git execution layer.

#### Changed

- The nav (entries for the Working Copy and History, merges and rebases in progress, branches and the like) moved to
  the sidebar, and the main view shows only the Working Copy or History, wide. Fetch / Pull / Push / Undo became
  buttons in the sidebar title.
- Branches are grouped into folders by `/`.

#### Removed

- The changed-file list in the sidebar and line-level staging in the standard diff editor opened from it
  (`ctrl+alt+s` / `ctrl+alt+u`). Staging is done in the Working Copy of the main view.
- The one-line commit field in the sidebar. Commits are made in the Working Copy of the main view.

#### Fixed

- The undo log is read again every time. Operations done in the app or the CLI were missed, and their records could be
  erased.

### [0.0.2] - 2026-10-14

#### Added

- The main view: one Webview in the editor area with the nav on the left (Working Copy / History / branches and the
  like) and the screen on the right. Opened from the Amon Hen icon in the activity bar.
- Working Copy: the list of changed files, per-hunk diffs (with word-level highlighting), staging and unstaging by line,
  hunk and file, a multi-line commit field and amend.
- History: the commit graph and the details of the selected commit (message, author, parents, changed files and their
  diffs). History loads 200 entries at a time, and diffs from the files that come into view.
- Reword / Squash / Fixup / Edit / Split / Drop / reorder commits by right-clicking in the History (reordering also by
  dragging).
- Branches / Remotes / Tags / Stashes / Submodules in the nav. Selecting a branch shows its history in the History, and
  double-clicking switches to it (for a remote branch, creates a branch that tracks it). Right-click for Checkout /
  Merge / Rebase / Delete.
- Merge / Rebase / Push / Force Push by dragging and dropping branches. If the drop target is not checked out and a
  fast-forward is enough, it moves forward without a checkout.
- Fetch / Pull / Push / Undo in the nav toolbar.
- Undo the last operation (commit, Merge, Rebase, fast-forward, Pull, deleting a branch, editing commits). The record
  survives restarting VSCode.
- When a merge or rebase has stopped, its progress is shown, with Continue / Skip / Abort.
- Open conflicted files in the standard 3-way merge editor. Resolve per file with Accept Current / Accept Incoming;
  when marking as resolved, you are asked to confirm if markers remain.
- A Changes view in the sidebar. Open changed files in the standard diff editor and stage / unstage the selected lines
  (`ctrl+alt+s` / `ctrl+alt+u`).
- A script that installs the stable release or the snapshot from GitHub Releases.
