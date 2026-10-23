# Usage

[日本語](ja/usage.md)

How to use the app (macOS), SSH hosts, the shared daemon and the CLI. For installation see [install.md](install.md),
and for the VSCode extension see [apps/vscode](../apps/vscode).

## App

- On launch, the repository list (File > Repositories…, ⇧⌘O) appears. Repositories you have opened are listed newest first; add one with Add Repository… and take one off with Remove (the folder is left untouched). File > Open… (⌘O) opens another repository in a separate window. Closing the last window returns to the list.
- Sidebar: Working Copy / History, branches (folders, ahead/behind upstream ↑↓, gone), remotes, tags, stashes, submodules. Right-click for checkout, merge, rebase, create, rename, delete, stash operations, and updating a submodule or opening it in a separate window. Dropping a branch onto another branch merges / rebases (no checkout if a fast-forward suffices); dropping it onto a remote pushes.
- Toolbar: Fetch, Pull, Push, Stash, Undo. Rewriting history, discarding changes, dropping stashes and the like can be reverted with Undo (the record is shared with the CLI and the VSCode extension).
- Working Copy: files are shown as a list of paths or a directory tree (switch above the list). Click diff lines to select them (a hunk header selects the whole hunk, Shift selects a range, ⌘ adds to the selection), then Stage Lines / Discard Lines. Space / Enter stages / unstages. Write a Summary and Description and commit with ⌘↩. For conflicts, choose Use Current / Use Incoming / Use Both per conflict, or Merge Editor… (the 3-pane merge editor).
- History: a list with a graph (subject and branch / tag labels, with author, date and ID below). Right-click to edit a commit (Edit Message, Squash, Fixup, Edit, Split, Drop), cherry-pick, revert, or create a branch or tag. Drag to reorder. At the top, switch between All Branches / Current Branch / a chosen ref. ⌘F searches commits (Enter / ⌘G for next, ⇧Enter / ⇧⌘G for previous; you can also show only the matching commits).
- Diffs: drag to select text and ⌘C, ⌘F to search. Syntax highlighting is applied in the background after display.
- History search syntax: words are searched in the subject, author and ID. Separate with spaces for AND, `or` for OR, and group with parentheses. `author:` `subject:` `commit:` `path:` `contents:"..."` (e.g. `path:fs/ (author:linus or author:greg)`).
- Right-click a file (in Working Copy, or a changed file in History) for Blame and File History (renames are followed too). They open in a tab next to the repository's tab, with the same toolbar (its buttons switch back to the repository's tab and act there); drag the tab out for a separate window. Opening the same one again replaces its tab with a fresh one. Double-click a line or commit to select it in History. File History opens at once with the newest 100 commits and adds the rest as they are read; when the file was renamed, a heading with the name it had then sits above each name's commits. Right-click a commit there for Show in History, Blame at that commit and copying the ID or path (Return shows it in History, ⌘C copies the ID).
- View > One Line per Commit makes History rows a single line. Repositories that were open when you quit are reopened on the next launch.

## Repositories on SSH hosts

Use Add Remote Repository… in the repository list and enter the host and path, or `AmonHen ssh://[user@]host[:port]/path/to/repo` (`/~/src/repo` for a path from home). You must be able to connect without a password, using a key or ssh-agent. On the SSH host, only git processing is done, by `amonhen --stdio`. If an `amonhen` with a matching protocol version is on the SSH host's `PATH`, it is used; otherwise the bundled engine (Linux x86_64 and aarch64, macOS arm64) is pushed to `~/.local/share/amonhen/engine/<content hash>/` (its contents are verified after pushing; `AMONHEN_PUSH=0` disables pushing, `AMONHEN_REMOTE_ENGINE` specifies the location).

## Shared daemon

With "Open repositories through the shared daemon" in Settings > Advanced, or Open via Daemon in the list's context menu, a repository is opened through the local shared daemon (`daemon:/path`, `daemon:ssh://host/path`). History, the search index and SSH connections are shared with the CLI and the VSCode extension. Check its state in the Daemon tab of Window > Activity or with `amonhen daemon status`.

## CLI

`amonhen --help` lists all commands. The main ones:

```sh
amonhen .                      # open in the app
amonhen log -n 20 fix          # history with a graph (search syntax works too)
amonhen status / diff / show   # Working Copy and commit contents (--json for the form FEs read)
amonhen stage --rows 3,5-9 f   # stage diff lines (see the numbers with amonhen diff --combined --rows)
amonhen undo [--list]          # undo operations (record shared with the app and the VSCode extension)
amonhen log -C ssh://host/path # SSH host (the shared daemon keeps the SSH connection alive)
amonhen daemon status          # state of the shared daemon
```
