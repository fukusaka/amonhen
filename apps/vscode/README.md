# Amon Hen

[日本語](https://github.com/fukusaka/amonhen/blob/main/apps/vscode/README.ja.md)

A personal extension for working with Git comfortably in VSCode. It is not published on the Marketplace; the `.vsix` is distributed through this repository's GitHub Releases. See the "VSCode extension" section of the repository's [CHANGELOG.md](https://github.com/fukusaka/amonhen/blob/main/CHANGELOG.md) for the change history, and [Development](https://github.com/fukusaka/amonhen/blob/main/docs/development.md#vscode-extension) and the [design notes](https://github.com/fukusaka/amonhen/blob/main/docs/vscode.md) for working on it.

> [!WARNING]
> Work in progress. Features, file formats and commands may change without notice. It is provided as is, without
> warranty of any kind (see the license in the repository root); keep backups of repositories that matter to you.

## Features

Clicking the Amon Hen icon in the activity bar opens the nav in the sidebar and the main view in the editor area (the main view can also be opened with "Amon Hen: Open Amon Hen" in the command palette). Whatever you select in the nav is shown in the main view.

### Nav (sidebar)

| Item | Description |
| --- | --- |
| Title buttons | Fetch (fetch from all remotes), Pull (bring the upstream into the current branch), Push (to the upstream; when the current branch has none, the button is Publish Branch instead: choose the remote and the name there, and it becomes the upstream), Undo. Under `…`: New Branch, Stash Changes, Open Amon Hen, Refresh, Force Push |
| Working Copy | The number of changes. Selecting it shows the Working Copy in the main view |
| History | The current branch. Selecting it shows the History in the main view |
| Merge / rebase in progress | A row like "Rebase in progress (2/5)". Continue / Skip / Abort |
| Branches | `feature/x` is grouped into folders. ✓ on the current branch, the difference from the upstream (↑↓), and gone if the upstream has been deleted |
| Remotes / Tags / Stashes / Submodules | Fetched when expanded |

- Selecting a branch, remote branch or tag shows its history in the History of the main view.
- Right-clicking a branch or tag offers Checkout (for a remote branch, creates a local branch that tracks it and switches to it), for a local branch that is not checked out Fast-Forward to Upstream (only behind it, as of the last Fetch; not for one checked out in another worktree) or Push to Upstream (only ahead), Publish Branch for a local branch without an upstream, Merge into Current Branch, Rebase Current Branch onto This, Rename Branch, Delete Branch, New Branch (create one from there and switch to it), and New Tag (an annotated tag if you enter a message). Creation and deletion can be reverted with Undo.
- The `+` on the Stashes header stashes your working changes (you can choose whether to include untracked files). Selecting a stash opens its contents as a multi-file diff; right-click for Apply / Pop / Drop (revertible with Undo). Apply and Pop also restore what was staged.
- Right-clicking a submodule offers Update (initialize it and match it to the recorded commit) and opening it in another window.
- When multiple repositories are open, switch between them with a title button.

### Drag and drop

Dropping a branch, remote branch or tag from the nav onto another item in the nav shows the candidate operations. Every operation rewrites the drop target.

| What you drag | Drop target | Available operations |
| --- | --- | --- |
| Branch, remote branch, tag | Local branch | Merge (merge the dragged item into the drop target), Rebase (rebase the drop target onto the dragged item) |
| Local branch | Remote, remote branch | Push, Force Push (`--force-with-lease`) |

- When the drop target is not checked out and a fast-forward is enough, the branch is just moved forward without a checkout. Otherwise it is checked out before the operation runs.
- Pushing a branch with no upstream by dropping it sets the push destination as its upstream.

### Working Copy

The Conflicts / Staged / Changes files are on the left, the commit form below them, and the diff of the selected file on the right. The diff is laid out hunk by hunk, with changed parts highlighted word by word.

- Click diff lines to select them (Shift+click for a range), and Stage N Lines stages only the selected lines. For a file in Staged, the same operation unstages.
- You can also use Stage Hunk per hunk, Stage File, `+` / `−` in the list, and Stage All. In the list, select with the up and down keys and stage / unstage with Space or Enter.
- For files in Changes, you can discard changes with ↺ in the list, Discard File, Discard Hunk, and Discard N Lines. Untracked files are deleted. Discarded content can be restored with Undo.
- The commit form has a Summary and a Description (multiple lines). Turning on Amend fills in the message of the previous commit. You can also commit with `⌘Enter` / `Ctrl+Enter`.
- For a conflicted file, you can choose Open in Merge Editor (the standard 3-way merge editor), Accept Current, Accept Incoming, and Mark as Resolved. A file deleted on one side can also be resolved with Accept. When marking as resolved, you are asked to confirm if markers remain.

### History

The commit list and graph are on the left, and the details of the selected commit on the right. The divider can be dragged, and its width is remembered. At the top, switch between Current Branch / All Branches / the ref selected in the nav.

- Select in the list by clicking, or with the up and down keys and PageUp / PageDown. The history is loaded 200 entries at a time as you scroll.
- The details show the full message, branches and tags, author and date, commit ID, parent commits (click to move to that commit), the changed files, and the diff of each file. Diffs are loaded for the files that come into view. A merge commit shows the diff against its first parent.
- Each file's Open, Open All, and double-click / Enter in the list also open it in the standard diff editor.

Commits reachable from HEAD by first-parent (up to just before the first merge commit) can be edited by right-clicking. Each is done with a non-interactive rebase and can be reverted with Undo. Working changes are stashed and restored automatically.

| Menu | Description |
| --- | --- |
| Reword… | Rewrite the message. In the edit field, save with `⌘Enter` / `Ctrl+Enter` and cancel with `Esc` |
| Squash into Parent… | Combine into the parent commit. Edit the two messages joined together, then run it |
| Fixup into Parent | Combine into the parent commit, keeping the parent's message |
| Edit (Stop Here) | Stop at that commit. Fix and commit (amend), then Continue |
| Split… | Undo that commit, put its changes back in the Working Copy, and stop. Commit them in pieces, then Continue |
| Drop… | Remove it from the history |
| Move Up / Move Down | Move it one step to the newer / older side. You can also reorder by dragging rows |
| Copy Commit ID | Copy the commit ID |

On any commit, right-click for New Branch / New Tag (create from that commit) and Revert (create a commit that cancels it). A commit not on the current branch can be applied to the current branch with Cherry-pick. All of these can be reverted with Undo.

### Stopped merge / rebase

When stopped by a conflict or by Edit / Split, a row like "Rebase in progress (2/5)" appears in the nav. After resolving and staging, continue with Continue. In a rebase, Skip skips the current commit. Abort returns to the state before it started.

### Undo

Undo in the nav reverts the last operation. It covers commits (including amend), Merge, Rebase, fast-forward, Pull, creating and deleting branches and tags, Cherry-pick, Revert, discarding changes, dropping stashes, and editing commits in the History.

- Undoing a commit puts its changes back as staged.
- Undoing a Merge or Rebase returns the branch and the checked-out branch to their state before the operation, keeping your working changes.
- If a branch has moved after the operation, Undo is not done, so as not to break anything. Likewise, discarding changes is not undone if the files were modified after discarding.
- An operation stopped by Edit or Split becomes undoable once it finishes with Continue.
- The record is kept in `.git/amonhen/undo.json`, so you can still undo after restarting VSCode.

### Current limitations

- Only the range from HEAD up to the first merge commit can be edited in the History.
- Push cannot be undone.
- Push and fetch that prompt for authentication do not work (authentication must work through SSH keys or a credential helper).
- Line-level staging does not take git filters such as autocrlf into account.
- Diffs have no syntax highlighting.
- External changes take about 1 second to show up (because it waits for notifications from the built-in Git extension).

## Engine and settings

By default the extension runs History, diffs and operations through the Amon Hen engine (the same engine as the app and
the CLI), via the shared daemon. The `.vsix` bundles the engine for macOS arm64, Linux x86_64 / aarch64 and Windows
x86_64 / aarch64, and nothing needs to be set up: on first use it is placed in `~/.local/share/amonhen` (Windows:
`%LOCALAPPDATA%\amonhen`). The app, the CLI and the extension share the same daemon, History, search index and Undo
log. On a Remote-SSH host, the engine for that host is used there.

Without an engine for the host's OS and CPU, or with `amonhen.daemon.enabled` off, the extension runs on its own git
execution layer (the features above work the same, but without sharing with the app and CLI).

| Setting | Default | Description |
| --- | --- | --- |
| `amonhen.daemon.enabled` | `true` | Use the engine's shared daemon. Off: the git execution layer only |
| `amonhen.daemon.path` | `""` | An engine (`amonhen`) to use instead of the bundled one |

## Install / update

Download `amonhen-vscode-<version>.vsix` from [Releases](https://github.com/fukusaka/amonhen/releases) (stable:
`vscode-v<version>`; latest main: `snapshot`) and install it, then reload the window:

```sh
code --install-extension amonhen-vscode-<version>.vsix
```

For a Remote-SSH host, run it in the integrated terminal of the connected window.

The root `scripts/install-vscode.sh [stable|snapshot]` finds the latest one, downloads it and installs it in one step (only `curl` is
needed). Without a clone:

```sh
curl -fsSL https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-vscode.sh | sh -s -- stable
```
