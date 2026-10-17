# Changelog

User-visible changes, by release. The app and the engine (CLI and daemon) are released together with `v*` tags, and the
VSCode extension separately with `vscode-v*` tags, so each has its own section. Changes not released yet go under
`Unreleased`; on release, that heading becomes `### [x.y.z] - YYYY-MM-DD`, followed by a short summary paragraph that
becomes the GitHub Release notes.

## App and engine

### [Unreleased]

#### Changed

- Repositories are chosen on a repository page that opens as a tab (⌘T, ⌘N, ⌘O, ⌘⇧R), instead of a separate window.
  Categories on the left (All, Favorites, This Mac and each SSH host with their folders to scan and other folders,
  groups) and
  each repository once on the right, with its branch, status, last commit and when it was last opened. The toolbar has
  a Repositories button for the page, and the repository name next to it opens the same list to switch the current tab.

#### Added

- Folders to scan on SSH hosts: the engine on the host finds the repositories (`amonhen find-repositories`).

#### Fixed

- The repository list no longer jumps back to the top while scrolling (each row's branch and change count used to
  rebuild the whole list as it arrived).
- Line numbers, file headers and stage checkboxes in diffs no longer come out garbled after the window moves
  between screens of different scale (1x and 2x).
- Building History no longer spins up threads for every few commits. On the Linux engines (musl) it was 5 times
  slower than it should be; on git/git it now takes about 0.1 s instead of 0.6 s there, and about 20% less elsewhere.

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

### [Unreleased]

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
