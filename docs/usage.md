# Usage

[日本語](ja/usage.md)

How to use the app (macOS), SSH hosts, the shared daemon and the CLI. For installation see [install.md](install.md),
and for the VSCode extension see [apps/vscode](../apps/vscode).

## App

- On launch, the repository list (File > Repositories…, ⇧⌘O) appears. Repositories you have opened are listed newest first; add one with Add Repository… and take one off with Remove (the folder is left untouched). File > Open… (⌘O) opens another repository in a separate window. Closing the last window returns to the list.
- Sidebar: Working Copy / History, branches (folders, ahead/behind upstream ↑↓, gone), remotes, tags, stashes, submodules. Right-click for checkout, merge, rebase, syncing a branch that is not checked out with its upstream (Fast-Forward when it is only behind, Push when only ahead, Publish when it has none), create, rename, delete, stash operations, and updating a submodule or opening it in a separate window. Dropping a branch onto another branch merges / rebases (no checkout if a fast-forward suffices); dropping it onto a remote pushes.
- Toolbar: Fetch, Pull, Push, Stash, Undo. Rewriting history, discarding changes, dropping stashes and the like can be reverted with Undo (the record is shared with the CLI and the VSCode extension).
- Working Copy: files are shown as a list of paths or a directory tree (switch above the list). Click diff lines to select them (a hunk header selects the whole hunk, Shift selects a range, ⌘ adds to the selection), then Stage Lines / Discard Lines. Space / Enter stages / unstages. Write a Summary and Description and commit with ⌘↩. For conflicts, choose Use Current / Use Incoming / Use Both per conflict, or Merge Editor… (the 3-pane merge editor).
- History: a list with a graph (subject and branch / tag labels, with author, date and ID below). Right-click to edit a commit (Edit Message, Squash, Fixup, Edit, Split, Drop), cherry-pick, revert, save as patches (Save as Patch…; select several for a series), or create a branch or tag. Drag to reorder. At the top, switch between All Branches / Current Branch / a chosen ref. ⌘F searches commits (Enter / ⌘G for next, ⇧Enter / ⇧⌘G for previous; you can also show only the matching commits).
- Range Diff: right-click a local branch and choose Compare with Previous Version (or Repository > Compare with Previous Version) to compare it with its version before the last rebase, History edit or amend, as `git range-diff` does: = same, ! changed (with the diff of the two patches), < dropped, > added. Two commits selected in History or a Reflog row can be compared too.
- Patches: Repository > Apply Patch…, or dropping .patch / .diff / .mbox / .eml files on the window, shows what they change and whether they apply, then applies mail patches as commits (`git am`) or a diff to the working tree (`git apply`). When am stops at a patch, the bar under the toolbar says why, with Retry with 3-Way Merge, Continue, Skip and Abort. Both can be undone. The CLI has `amonhen format-patch`, `amonhen am` and `amonhen apply`.
- Diffs: drag to select text and ⌘C, ⌘F to search. Syntax highlighting is applied in the background after display.
- History search syntax: words are searched in the subject, author and ID. Separate with spaces for AND, `or` for OR, and group with parentheses. `author:` `subject:` `commit:` `path:` `contents:"..."` (e.g. `path:fs/ (author:linus or author:greg)`). An author matches by the name the mailmap gives or the one recorded in the commit.
- People are shown by the names and emails in the repository's mailmap (`.mailmap`, `mailmap.file`), as `git log` does; the details show the recorded name and email under them when they differ. git's `log.mailmap` turns this off; it is on Repository Settings > Author and Signing (and Settings > Git for all repositories), with the mailmap file. Changing either shows the names again without rebuilding History. Blame always uses the mailmap, as git does.
- Right-click a file (in Working Copy, or a changed file in History) for Blame and File History (renames are followed too). They open in a tab next to the repository's tab, with the same toolbar (its buttons switch back to the repository's tab and act there); drag the tab out for a separate window, and put it back with Window > Move Tab to Window ▸, which moves the front tab into the window chosen and lists the repository's window first (or drag its tab onto a tab bar; View > Show Tab Bar, ⇧⌘T, shows the tab bar of a window with one tab). Opening the same one again replaces its tab with a fresh one. Merge commits are left out unless Show Merge Commits is on (the list's context menu, or Settings > Appearance > File History). Double-click a line or commit to select it in History. File History opens at once with the newest 100 commits and adds the rest as they are read; when the file was renamed, a heading with the name it had then sits above each name's commits. Right-click a commit there for Show in History, Blame at that commit and copying the ID or path (Return shows it in History, ⌘C copies the ID).
- Large repositories: when cloning, Download chooses Everything, File contents when needed (a partial clone, `--filter=blob:none`) or Recent history only (a shallow clone of 1, 100, 1,000, 10,000 or any number of commits), and Check out only some folders makes a sparse checkout. A shallow clone's History ends with a band that fetches older commits. Repository Settings > Fetching and Folders shows what the repository has and changes it: fetch more history or all file contents, choose the folders to check out, and turn git maintenance on or off. History and Working Copy in the sidebar carry a mark for such a repository.
- View > One Line per Commit makes History rows a single line. Repositories that were open when you quit are reopened on the next launch.

## Repositories on SSH hosts

Use Add Remote Repository… in the repository list and enter the host and path, or `AmonHen ssh://[user@]host[:port]/path/to/repo` (`/~/src/repo` for a path from home). You must be able to connect without a password, using a key or ssh-agent. On the SSH host, only git processing is done, by `amonhen --stdio`. If an `amonhen` of the same build is on the SSH host's `PATH`, or one of the same build was placed there earlier, it is used; otherwise the bundled engine (Linux x86_64 and aarch64, macOS arm64, Windows x86_64 and arm64) is pushed to `~/.local/share/amonhen/engine/<content hash>/` (its contents are verified after pushing; `AMONHEN_PUSH=0` disables pushing, `AMONHEN_REMOTE_ENGINE` specifies the location).

An SSH host running Windows (its OpenSSH server) works too, whatever its default shell is (cmd.exe, PowerShell or pwsh): `ssh://host/C:/src/repo` for a path on a drive, `/~/src/repo` from home. The engine is sent by scp to `%LOCALAPPDATA%\amonhen\engine\<content hash>\` and started by the default shell. Windows PowerShell, which every Windows 10 and 11 has, reads what is on the host. The login shell switch on the host's Environment Variables page doesn't apply there.

Settings > SSH Hosts holds everything about an SSH host: the hosts Amon Hen knows (those in Places, plus any you add with +), and for the selected host its connection (state, Check Connection, which engine to run there and whether to send one when none fits, the engine in use, whether commit signatures are verified with this Mac's keys or the host's, and the folders to scan, the same list as in Places) and the pages of Settings > Git for every repository on that host (read from and written to the host's `--global`, through the engine started there without a repository). Its Programs page chooses the git that runs there, kept in the host's own Amon Hen settings (see below), and its Environment Variables page shows the engine's environment and can start the engine with the environment of the host's login shell. Settings… on a host in Places and the git line under a repository's Settings list open it; Settings… on This Mac opens Settings > Git.

## Which git runs

Amon Hen runs the git chosen in Settings > Git > Programs. The choice belongs to the machine and is kept in Amon Hen's settings there, `~/.config/amonhen/settings.json` (`$XDG_CONFIG_HOME/amonhen/` when set, `%APPDATA%\amonhen\` on Windows), as `{"git": "/opt/homebrew/bin/git"}`; the CLI, the shared daemon and the VSCode extension on that machine follow it, and on an SSH host the engine, the CLI and the VSCode extension follow the file on that host, which the host's Programs page in Settings > SSH Hosts writes. Without a choice, the first `git` on the PATH runs; the VSCode extension uses VSCode's `git.path` before the PATH. `AMONHEN_GIT` in the environment overrides the file for that process. If the chosen git isn't there, what runs git stops with an error saying so, instead of running another git.

## Shared daemon

With "Open repositories through the shared daemon" in Settings > Performance, or Open via Daemon in the list's context menu, a repository is opened through the local shared daemon (`daemon:/path`, `daemon:ssh://host/path`). History, the search index and SSH connections are shared with the CLI and the VSCode extension. Check its state in the Daemon tab of Window > Activity or with `amonhen daemon status`.

## CLI

`amonhen --help` lists all commands. The main ones:

```sh
amonhen .                      # open in the app
amonhen log -n 20 fix          # history with a graph (search syntax works too; --json adds recordedAuthor where the mailmap changed it)
amonhen status / diff / show   # Working Copy and commit contents (--json for the form FEs read)
amonhen stage --rows 3,5-9 f   # stage diff lines (see the numbers with amonhen diff --combined --rows)
amonhen undo [--list]          # undo operations (record shared with the app and the VSCode extension)
amonhen range-diff --previous  # compare the branch with its version before the last rebase or amend (git range-diff)
amonhen log -C ssh://host/path # SSH host (the shared daemon keeps the SSH connection alive)
amonhen daemon status          # state of the shared daemon
```
