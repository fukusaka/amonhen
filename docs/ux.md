# Rethinking the UI / UX

[日本語](ja/ux.md)

A review, built on "Screen layout" in `docs/design.md`, aimed at making everyday operations as easy as in SourceTree, Fork, Tower and JetBrains (the IDE's Git), or easier. The speed policy (never wait on git on the UI thread, stay within one frame) does not change.

## Principles

| Principle | Details |
| --- | --- |
| Do not give up speed | New screens also "compute and draw only what is visible". Heavy things (diffs across several commits, AI, GitHub) are added afterwards; whatever can be shown first is shown first |
| Follow macOS conventions | Do not break window tabs, Settings (⌘,), toolbar customization, complete menus, or the standard macOS shortcuts. Custom conventions only inside our own views, such as History and diffs |
| One operation completes in one place | Choose → check → run on the same screen. Separate windows and sheets only for operations that need confirmation |
| Everything can be undone | Writes go into the operation log (oplog) and can be reverted from the toolbar and Activity |
| Everything can be done from the keyboard | Every operation has a menu item, and shortcuts are looked up from the menu |
| External communication is explicit | AI and GitHub only when enabled in settings. Show what will be sent before sending it |

## Overall layout

```
┌ Tabs: amonhen-standalone │ linux │ vscode-amonhen │ + ─────────────────────────────────────────┐
│ [◧] main ↑2↓0 │ Fetch Pull Push │ Branch Merge Rebase │ Stash Pop │  ⟳ 2 ops  ↶  🔍____ │
├────────────┬──────────────────────────────────────────────────────────────────────────────┤
│ WORKSPACE  │ (one of Working Copy / History / Conflicts / Stashes / Reflog)               │
│  Changes 5 │                                                                              │
│  History   │                                                                              │
│  Conflicts │                                                                              │
│  Stashes 2 │                                                                              │
│  Reflog    │                                                                              │
│ BRANCHES   │                                                                              │
│ REMOTES    │                                                                              │
│ PULL REQS  │                                                                              │
│ TAGS       │                                                                              │
│ SUBMODULES │                                                                              │
│ WORKTREES  │                                                                              │
└────────────┴──────────────────────────────────────────────────────────────────────────────┘
 During a merge, rebase or cherry-pick, a bar under the toolbar: "Rebase 3/7 · 2 conflicts  [Continue] [Skip] [Abort]"
```

- Conflicts appears in the sidebar only when there are conflicts, with a count.
- A filter field at the top of the sidebar (Navigate > Filter Branches…, ⌥⌘F) narrows Branches, Remotes and Tags to
  the names containing every word typed; matches are listed flat with their full names. Esc clears it.
- Several branches or tags can be selected (⌘-click, ⇧-click). History then shows the commits reachable from any
  of them, and the context menu offers Copy Names and, for local branches, Favorites, Archive and Delete together (Delete asks
  first and lists the names), and for tags, the tag items below together.
- Unlike branches, tags have no remote copies in the repository (fetched tags join your own `refs/tags`), so which
  remote has which tag cannot be told from local refs. Each Fetch (automatic ones too) also reads every remote's tags
  (`git ls-remote --tags`) into `<common git dir>/amonhen/remote-tags.json`; pushing or deleting tags updates it at
  once. Tags are compared by the object their refs point to, so an annotated tag made again on the same commit is a
  different tag (Push would be refused). A tag row then shows a mark at its right when no remote has the tag (☁ with
  a slash) or when a remote has a different tag with the name (orange). Its tooltip lists every remote (has it, a
  different tag, doesn't have it, not checked yet), and clicking the mark opens a popover with the same, when each
  remote was read, and Push to / Replace on.
- A tag's context menu follows what is known: Push <tag> to <remote> for remotes that lack it, Replace <tag> on
  <remote>… (a forced push) where the remote has a different tag with the name, Delete…, which deletes your own copy only and can
  be undone (an annotated tag comes back with its message), and Delete <tag> from <remote>… for remotes that have it,
  which removes it for everyone and cannot be undone. A remote not read yet gets Push and Delete from, and the sheet
  says it was not checked. Deleting and replacing ask first, naming the tags.
- Workspace holds the rows that switch the main view. Stashes, after History (and Conflicts), appears only when there
  are stashes, with a count, and opens the Stashes view (section 5a); the stashes themselves are listed there, not in
  the sidebar. Reflog, always last, opens the Reflog view (section 5b).
- Pull Requests and Worktrees appear only when there is something to show.

## 1. Multiple repositories: tabs and separate windows

Reference: Fork (a tab per repository), Tower (window tabs).

- Use standard macOS window tabs with `NSWindow.tabbingMode = .preferred`. Dragging a tab makes it a separate window, and "Merge All Windows" in the Window menu works out of the box. No custom tab bar.
- ⌘T and the tab bar's "+": show the list of repositories and open the chosen one in a tab of the front window (replace this with Quick Open once it exists). ⌘N is a new window. Repositories opened with Open… or from the list also go into a tab of the front window. Switch tabs with ⌃Tab / ⌃⇧Tab and ⌘⇧] ⌘⇧[.
- **At launch**, reopen the windows and tabs that were open when the app quit, in the same order. A repository given as an argument or opened from the Finder or `amonhen` opens after them, in a tab of the front window. Windows closed before quitting are not reopened (the usual macOS behavior), so quitting with nothing open shows the repository page next time. Repositories whose folder is gone are left out.
- **Clicking the Dock icon** with no window showing brings back the minimized windows if there are any, and otherwise shows the repository page in a new window (the app keeps running after its last window closes; ⌘Q quits).
- **Repositories that need connecting** (on an SSH host, or through the daemon): opening one puts its tab in place at once (where the repository goes: a tab of the front window, a new window, or the tab it replaces), showing that it is connecting, with Cancel. When connected, the repository takes that tab's place, and comes to the front only if the tab was in front (a tab that finishes in the background does not take the focus). If connecting fails, the tab stays and shows the error with Try Again and Close Tab, instead of an alert. Restoring at launch, opening from the list and Reconnect all work this way, so the order of tabs and windows does not depend on which connects first, and a repository that could not connect is still reopened at the next launch until its tab is closed. A local repository that fails to open at launch also keeps its tab, with the error.
- Background tabs keep their memory down: keep only the History chunks near what is visible (using the existing mechanism), and keep watching files but delay recomputing status until the tab comes to the front. The tab title shows whether there are uncommitted changes (●) and ahead/behind.

## 2. Bookmarks (the repository list)

Extend today's "Repositories…" (a list in the order opened) into the equivalent of Fork's Repository Manager and SourceTree's bookmarks.

| Element | Details |
| --- | --- |
| Adding one by one | Choose a folder with "Add…", drop from the Finder, or add repositories automatically when opened (can be turned off in settings). SSH hosts (`ssh://`) go here too |
| Picking up from folders | Register several "folders to scan" (e.g. `~/prj`). Specify depth (default 3) and exclude patterns (`node_modules` and so on). Repositories found go into a group named after the folder. Folders are watched with FSEvents, and additions and removals are reflected. Repositories that disappear stay, greyed out, until removed by hand |
| Groups | Manual groups (nested like folders) and scan groups. Reorder and move by dragging |
| Status | Each row shows the current branch, ahead/behind and the number of changed files. Only `.git/HEAD` is read right away; for the rest, only visible rows run `git status --porcelain=v2 --branch --untracked-files=no` (takes no lock) in the background, 4 at a time. Untracked files are not counted |
| Extra information | Color labels, favorites (★), display name override, last opened time |
| Operations | Open (⏎ in a tab of the current window, ⌘⏎ in a new window), Show in Finder, Open in Terminal, Open in Editor, Fetch (all selected ones at once) |
| Quick Open | ⌘⇧O opens a panel (Spotlight-style) that fuzzy-searches all bookmarks |

The list appears in two places, with the same contents and operations (`RepositoryBrowser`):

- **The repository page**, a tab with no repository open (`RepositoryHome.swift`). ⌘T, ⌘N, ⌘O and ⌘⇧R show it, and so
  does launching or closing the last repository with nothing open. On the left are categories: All, Favorites, Places
  (This Mac and one entry per SSH host) and the manual groups (shown once one exists), each with a count. Under each place are its folders to
  scan (🔍; only these are searched, on that machine: SSH hosts are searched by the engine there with
  `amonhen find-repositories`) and the folders of the other repositories (opened or added by hand), as a tree of the
  folders that hold repositories (folders with a single child and no repository of their own are folded into one,
  like `~/prj/oss`); choosing a folder lists the repositories under it (folders under a place hold only repositories
  outside its 🔍 folders, so nothing is counted twice; a shortened name shows the full path as a tooltip). Things are added from the "+" under the left
  side or by right-clicking: Places adds a local repository, an SSH connection, a folder to scan on this Mac or one on an SSH host (any host, not only ones already listed), This Mac a local repository or a
  folder to scan, a host a repository or a folder to scan on that host, Groups a group (for an SSH host the dialog suggests
  hosts from `~/.ssh/config` and `~/.ssh/known_hosts`, and lists the folders on the host to pick from: double-click to go
  in, repositories in green; hosts that ask for a password can't be listed, so the path is typed); a 🔍 folder sets its depth,
  rescans or stops scanning. On the right,
  the repositories in the chosen category appear once each, in five sortable columns: name and path, branch, status
  ("changes n", ahead/behind ↑↓, ✓ when clean, explained in the column header's tooltip; "SSH" for remote
  ones, which are read only when opened), the last commit (when and its
  subject) and when it was last opened (the default order; — if never). Dragging rows onto Favorites or a group puts them there.
  Choosing a repository (double-click or ⏎) turns this tab into that repository (⌘⏎ for a new window). The right side
  is just the filter and the list; there are no buttons under it. A person takes in about five or six things at once, so each part of the page stays within that. The page has
  the same toolbar as a repository tab (the repository actions are disabled), so the tab bar does not move when
  switching between them. There is no separate window.
- **Repositories** at the left of the toolbar switches to the repository page (⌘⇧R).
- **The repository name next to it** (like GitHub Desktop). It opens the list in a popover with names
  and status only, favorites first and then by when they were last opened; choosing one switches the current tab to it.

Saved not in UserDefaults but in `~/Library/Application Support/Amon Hen/bookmarks.json` (structured, and editable by hand). Scan results are not saved; they are collected again in the background at launch.

### Settings per SSH host

An SSH host's settings are on Settings… > SSH Hosts (chapter 3): the hosts with + / −, and for each its connection, engine, signature verification, folders to scan and git settings for every repository there. The host's entry in Places opens that tab with the host selected (Settings… on its context menu), and so does the line under a repository's Settings list (`git 2.34 · build-1`). The folders to scan under a host in Places and those on the tab are one setting.

## 3. Settings (Settings…, ⌘,)

The standard macOS layout with a row of tabs with icons at the top (`NSTabViewController` with the `.toolbar` style, contents in SwiftUI `Form`), resizable, 960 points wide by default; the size is kept across tabs and launches, and a tab's content fills it (scrolling inside when it is taller). The toggles currently in the app menu move here.

Settings… holds what Amon Hen itself decides, and the git settings of each machine (the Git tab). git's settings for one repository are on that repository's page (chapter 4); the Git tab is the "all repositories" level of those rows, for this Mac and for each SSH host. Rules for every tab: it fits on one screen without scrolling; nothing folds away (no Details); help only where the question can't stand alone, a few words, the rest behind ⓘ; no editor is embedded (a template's text, a prompt open in a sheet); a program is chosen like the others, never typed.

| Tab | Items |
| --- | --- |
| General | Open the previous repositories at launch, add opened repositories to bookmarks, default for opening repositories (tab / window), fetch automatically (one interval for every open repository), choice of external editor (done: Automatic picks the first code editor installed, or the default text editor; or any application), terminal and diff tool |
| Git | The machines' git settings: the Repository Settings screen (chapter 4) opened at the "all repositories" level, so it has the same list of pages and the same rows, and writes `--global` on that machine. Pages: Author and Signing; Commits (the template choice, and under it the shared template library: the names with + / − and Edit… for the text in a sheet); Operations; Working Copy (Ignore everywhere, `core.excludesFile`, in place of the repository's exclude); Programs (git, gpg, ssh-keygen and gpgsm, each chosen the same way: Automatic, the first on the PATH, shown with where it comes from and its version; the ones found, as Homebrew 2.56.0 or Apple 2.54.0; Other… for a path. git is kept in the machine's Amon Hen settings, `~/.config/amonhen/settings.json`, which the CLI, the shared daemon and the VSCode extension there follow too, and a chosen git that isn't there is shown under the row and stops what runs git rather than another git running; an SSH host's Programs page writes the host's own file; the others go to `gpg.program`, `gpg.ssh.program` and `gpg.x509.program`); Environment Variables (this Mac's, for git and ssh: whether to use the login shell's, read as a terminal opens one, with when it was read, Read Again and whether the SSH agent is there; a table of the variables with their values, where those git and ssh always use are checked and locked, others can be checked to pass them too (`AMONHEN_PASS_ENV`), and those that can't be passed say why; under it the selected value in full, PATH-like ones place by place with the git in use. The value of a variable whose name looks secret is masked, in the table and below it until Show, and search looks only at its name: the name is split into words at anything but letters and digits, and a word that contains TOKEN, SECRET, PASSWORD, PASSWD, CREDENTIAL or PRIVATE, is AUTH or PASS, or ends in KEY (APIKEY too) makes it secret, except `SSH_AUTH_SOCK` and `AMONHEN_PASS_ENV`. A change applies at once, to the shared daemon too. Amon Hen's own setting, not git's); Keys and Values. Pages that only a repository has (Branches, Remotes, Hooks · LFS) are not listed. Each row's source reads git's default or all repositories, with ↩︎ and ⓘ as on a repository's page. Under the hood the level holds an empty repository of the app's own and ignores its local values |
| SSH Hosts | The SSH hosts Amon Hen knows, with + / − to add one (from `~/.ssh/config` and known_hosts, or typed) or forget one, and for the selected host its pages in a second list: Connection (the host, its state and Check Connection, the engine, signature verification, folders to scan) and the pages of Git for all repositories there (writing the host's `--global`). Three columns; see "SSH Hosts" below |
| Accounts | GitHub: CI status and avatars in History, the branches and commits to read, and reading avatar URLs once. Later, accounts for GitHub, GitLab and others (see below), tokens in the keychain |
| Appearance | Theme (system / light / dark), font and size for diffs and messages, History order, one-line display, row items, avatars and ancestor highlighting, graph colors |
| Diff & Merge | Default unified / split, ignore whitespace, context lines, wrapping, tab width, default layout of the merge editor (see below), diff3, resolving unambiguous conflicts when the merge editor opens (done) |
| Commit | Subject length guide (50 / 72), add Sign-off, Conventional Commits completion, push after commit. Not shown until it has rows; the message template library is on Git > Commits |
| AI | The AI to use (none / Apple Intelligence on-device / Claude Code / Codex; by default the on-device model where it is available, otherwise none), the chosen CLI's location, version and login state (with install guidance if not found), model, target language for translation, per-repository opt-out. Per-purpose prompts under the advanced section (chapter 9) |
| Performance | Automatic commit-graph writing, fsmonitor, opening through the shared daemon, location, size and clearing of the persistent cache, handling of background tabs |

There is no Advanced tab: its items are in Performance (the daemon) and Git (the shell's environment variables) and SSH Hosts (signature verification, per host). Two rows of the first Git tab go rather than move: the pull default (the Pull sheet always preselects Follow git's settings; how to pull is git's setting on the repository's Operations page, for this repository or for all) and the user-wide `rebase.updateRefs` (the same row on Operations, set for all repositories).

### SSH Hosts

One place for everything about an SSH host. A host is a machine, so what is decided here applies to every repository on it; nothing of this is on a repository's page (chapter 4). Three columns: the hosts, the selected host's pages, and the selected page, so that each page holds as much as a page of Settings… > Git and nothing is cramped:

```
Settings… > SSH Hosts                          (960 wide: 150 / 170 / 640)

┌───────────┬──────────────┬──────────────────────────────────────────────┐
│ build-1   │ Connection   │ Connection                                   │
│ dev-2     │ Author and   │   Host     build-1   from ~/.ssh/config      │
│ old-box · │   Signing    │   State    ● connected                       │
│   unreach.│ Commits      │            amonhen 0.2.7 (sent) · git 2.34   │
│           │ Operations   │            [Check Connection]                │
│           │ Working Copy │                                              │
│           │ Programs     │ Engine                                       │
│           │ Keys and     │   Use      ( Automatic                    ⌄) │
│           │   Values     │   Send one when none fits            (●)     │
│           │              │   In use   ~/.local/share/amonhen/engine/…   │
│           │              │ Signature verification                       │
│           │              │   Verify with  ( This Mac’s keys          ⌄) │
│           │              │                                              │
│           │              │ Folders to scan                              │
│           │              │   ~/src    3 levels                          │
│ [+] [−]   │ git 2.34 ·   │   ~/work   3 levels                 [+] [−]  │
│           │   build-1    │                                              │
└───────────┴──────────────┴──────────────────────────────────────────────┘
```

- **The hosts** (left). Every host Amon Hen knows: those in Places on the Repositories screen (a repository or a folder to scan there) and those a repository was opened on. + adds a host, choosing from `~/.ssh/config` and known_hosts or typing a name; − forgets one, which also removes its folders to scan and the bookmarks of its repositories, after a confirmation. A host that cannot be reached stays in the list, dimmed, with "unreachable" after its name.
- **The pages** (middle). Connection first, then the same pages as Settings… > Git: Author and Signing, Commits, Operations, Working Copy, Programs, Environment Variables, Keys and Values. Under the list, the git of this host (`git 2.34 · build-1`), as under a repository's Settings list.
- **Connection.** The host as ssh knows it, whether Amon Hen can reach it, and what it found there (the engine and the git). Check Connection tries again and reports what failed (ssh, the engine, git). Engine: which `amonhen` to run there (Automatic, the one on the host's PATH if it is this build, else one sent earlier, else send one; the one on the host's PATH only; the one Amon Hen sends; or a path on the host), whether to send one when none fits, and the one in use with how it was prepared. `AMONHEN_REMOTE_ENGINE` and `AMONHEN_PUSH` in the environment still decide over these, and the page says so when they are set. A change reconnects the tab; repositories already open on the host keep their engine until they are opened again. Signature verification: with this Mac's keys (the default; this replaces the one switch for every host that Settings… had) or with the host's git and keys. Folders to scan: the same list as under the host in Places; the two are one setting.
- **The git pages.** The pages of Settings… > Git, opened for this host: the same rows, reading and writing the host's `--global`; a line at the top says so ("Applies to every repository on build-1 (its ~/.gitconfig). git in a terminal there follows these too."). The user's name, email, signing and key, with the keys collected on the host; the template file; pull, fetch, rebase and merge; line endings, the executable bit and the global excludes; the programs on the host; Keys and Values. This needs the engine to run on the host without a repository, so that Settings… reaches the host as soon as it opens, with no repository of the host open; until then the pages say which repository they are reading through.
- **Programs on a host.** git is chosen the same way as on this Mac (Automatic, the ones found on the host with their versions, Other… for a file chosen on the host, see Paths on a host); it is written to the host's Amon Hen settings (`~/.config/amonhen/settings.json` there), which the host's engine, CLI and VSCode extension follow, and applies from the next operation without reconnecting. A chosen git that isn't there is shown under the row, and on the Connection page with Use the git on the PATH. A git chosen for the host by an earlier build, which the app kept, is moved to the host's file when Settings > SSH Hosts connects, unless the file already has one. gpg, ssh-keygen and gpgsm write the host's `gpg.*.program`.
- **Paths on a host.** Wherever a path on the host is asked for (Other… for a program or the engine, + for a folder to scan, Choose… when adding a repository over SSH), a sheet shows the host's folders in columns, like the Finder's column view, from `/` to the current folder (the current value, else the home folder), with git repositories marked in green. The path field above it takes a typed path (`~` is the host's home), hidden files show with a switch (⇧⌘.), and what can't be chosen (a file when a folder is asked for, a file that isn't executable when a program is) is dimmed. The engine on the host reads the folders (`listDirectory`), so the sheet connects to the host first when nothing is connected yet; if it can't connect, a typed path can still be chosen. Adding a folder to scan from Places also lets the host be chosen in the sheet.
- **Environment Variables on a host.** The engine there is started by a command over ssh, which runs in a shell that is not a login shell, so the PATH and `GIT_*` set in `~/.profile` and the like may be missing, as they are for an app opened from the Finder on this Mac. Use the login shell's environment variables (per host, off by default) starts the engine with the environment of the host's login shell (tcsh, csh and fish included, reading the files their login shells read), and looks for the amonhen on the PATH in that PATH too; what the profile prints cannot break the connection. Below it, the engine's environment read from the host, which git there gets as it is (no column to choose what to pass), with the selected value in full and PATH place by place, marking the git in use; values are masked as on this Mac's page. Changing the switch reconnects.
- **From elsewhere.** The host's entry in Places (Settings… on its context menu) and the line under a repository's Settings list (`git 2.34 · build-1`) open this tab with the host selected; they are shortcuts, not other screens. This Mac's entry in Places has Settings… too, which opens Settings… > Git.

## 4. Repository settings (Settings in the sidebar's Workspace, ⌘;)

A page for one repository, the last row of the sidebar's Workspace (Repository > Repository Settings, ⌘;, opens it). It holds only what belongs to this repository and is a setting: git's own settings for the repository, and the few things Amon Hen keeps per repository. Nothing on it reaches other repositories, and nothing on it is really a file of the repository or an attribute of the repository list; those live where they belong (4.7).

The first version (a sheet with tabs, U11) showed git's configuration as git stores it. The second (the first page) kept too much of that shape: rows under a "More" fold, a chevron that had to be hit exactly, settings that only a terminal honors, and attributes of the list. This design starts from the person using it and from what each row does.

### 4.1 Principles

1. **Ask in terms of what happens.** A row is a question in plain words ("When pulling, how are the changes brought in?") and its answers are plain words ("Merge", "Rebase", "Fast-forward"). git's key and raw value are shown only on request (ⓘ) and in All settings.
2. **Always show what happens now, as one value.** The control shows the value in effect. When nothing is set anywhere, it shows git's default behavior, not "Not set". Its source is written next to it (git's default, all repositories, this repository).
3. **Changing and reverting are two different things.** Picking a value writes it for this repository. ↩︎, shown only while this repository overrides, removes the override. There is no "Inherited" entry in a menu.
4. **One choice per decision.** When two git keys make one decision (sign or not and in which format; the pull method), the row is one choice and writes both keys.
5. **Choose, don't type.** When the answers can be listed (signing keys, templates, git executables, remotes), the row lists them; typing is the "Other…" entry, not the row.
6. **Only what Amon Hen follows.** A git setting that Amon Hen's own operations ignore (because they pass an explicit flag) is not on the page; it stays in All settings. Where Amon Hen's behavior differs from a terminal's with the same value, the help says so.
7. **Shared things are shared.** What applies to every repository on a machine (the user's name, email and signing key, the git executable, the gpg programs, the global excludes) is set for that machine: Settings… > Git for this Mac, Settings… > SSH Hosts for a host; what applies to the app (auto-fetch) is in Settings… > General, and the template library is on Settings… > Git > Commits. This page shows "all repositories" as the source when such a value is in effect, and writes only this repository's own values.
8. **No "More".** A row is either worth a place on the page or it is not. Nothing is folded away.
9. **What is not a setting does not look like one.** The git in use, the default branch, hooks, LFS and the worktrees are information with actions, on About this repository. Files of the repository (`.gitignore`) are edited as files, in Working Copy.
10. **Help is readable.** One sentence under each question, in body size and secondary color, set in from the question. Nothing in caption size.
11. **One language.** Every label is in the UI language, including page names. Keys, values and file names stay as git spells them.

### 4.2 The shape of a row

A choice, with nothing set in this repository (the user-wide value, or git's default, is in effect):

```
When pulling, how are the changes brought in?        ( Merge            ⌄ )   git's default  ⓘ
    Merge records a merge commit when both sides have new commits. Rebase replays
    your commits on top of the fetched ones. Fast-forward refuses when they have diverged.
```

The same row after choosing Rebase here:

```
When pulling, how are the changes brought in?        ( Rebase           ⌄ ) ↩︎  this repository  ⓘ
    Merge records a merge commit when both sides have new commits. …
```

ⓘ opens a popover with where the value is set; no row expands:

```
  pull.rebase = true
  this repository (.git/config)
  Without the override here: Merge (all repositories, ~/.gitconfig)
```

A yes/no setting is a switch with the same source:

```
Stash changes before rebasing                                     (  ●)   this repository  ↩︎  ⓘ
    Uncommitted changes are stashed before a rebase or a pull with rebase and restored
    after it. History edits always stash.
```

A choice whose answers are collected (the signing key; the list depends on the format chosen above it):

```
Key                               ( 0F8A 1B2C … Ada Lovelace <ada@example.com>  ⌄ )   all repositories  ⓘ
                                    ├ 0F8A 1B2C … Ada Lovelace <ada@example.com>
                                    ├ 7D3E 9A10 … Ada Lovelace (work) <ada@work.example>
                                    └ Other…
```

A row the git in use cannot honor (here, stacked branches below git 2.38):

```
Move stacked branches along when rebasing              ( Leave them   ⌄ )   git's default  ⓘ
    ⚠ Needs git 2.38 or later. This repository's git is 2.34 (build-1).
```

The parts, left to right: the question with its help under it; the control showing the value in effect; ↩︎ while this repository overrides; the source; ⓘ.

### 4.3 The source: where the value comes from

| Source | Meaning | In git |
| --- | --- | --- |
| git's default | Nothing is set anywhere; git's own default behavior is shown | — |
| all repositories | The user running git set it for every repository (Settings… > Git for this Mac, Settings… > SSH Hosts for a host) | `--global` (`~/.gitconfig`) |
| this repository | Set here; ↩︎ removes it | `--local` (`.git/config`) |
| this worktree | Set for the current worktree only | `--worktree` |
| system-wide | Set for every user of the machine (read-only here) | `--system` |

For a repository on an SSH host, "all repositories" means those of the user on that host, and the source says so ("all repositories on build-1"). Its tooltip shows the file. These words are the only names for git's scopes in the app: git config's Source column uses the same ones, and "global", "local" and "system" appear only in that column's tooltips.

### 4.4 Writing and reverting

- Choosing a value writes it to this repository at once (`git config --local`), with no confirmation. ↩︎ is the way back, and the source changes as soon as the write lands.
- Every change on these pages can be undone and redone with Edit > Undo (⌘Z) and Redo (⇧⌘Z), any number of steps while the page is shown (Settings… > Git too). Undo puts back the previous value (↩︎ instead takes the value of the level above). It is a record of its own, apart from the repository's Undo (⌥⌘Z, the core's operation log): neither undoes the other.
- A failed write shows its error under the row, in red, and the control goes back to the previous value.
- Nothing on this page writes `--global`. A value for every repository on a machine is set in Settings… > Git (this Mac) or Settings… > SSH Hosts (that host), and both say that git in a terminal follows those settings too. Here the source only says when such a value is in effect, and git config's Levels view shows the all-repositories column read-only. (Set for All Repositories on a row, and editing that column here, were tried and dropped: they put the same thing in a second place, and a context menu on a settings row is not something anyone looks for.)

### 4.5 Pages

A list on the left and the chosen page on the right, like System Settings. The list has no groups: the pages people change often come first, the two they look at now and then last, and under the list one small line says which git runs this repository (`git 2.54.0 · This Mac`, or `git 2.34 · build-1`); clicking it opens Settings… > Git with that machine selected (chapter 3). Pages are named by their contents; no page is called "about" or "all", because every page here is a setting of this repository.

| Page | Rows |
| --- | --- |
| Author and signing | **Name**, **Email** (`user.name`, `user.email`; typed, since they are not a list). **Signing**: one choice, No signing / OpenPGP / SSH / X.509 (writes `commit.gpgsign` and `gpg.format` together). **Key**: a list collected for the chosen format (OpenPGP: `gpg --list-secret-keys`, the keys for the email first; SSH: `~/.ssh/*.pub` and `ssh-add -L`; X.509: `gpgsm --list-secret-keys`), plus Other… for a value not in the list; collected by the core, so on the SSH host for a repository there (`user.signingkey`). Name, Email, Signing and Key have the same rows for all repositories in Settings… > Git, where the gpg / ssh-keygen / gpgsm programs are also found and can be overridden. |
| Commits | **Message template**: None (start empty) / a shared template from the library on Settings… > Git > Commits / This repository's own / A file… (git's `commit.template`, with a file chooser). Under the row: a shared template is shown read-only with Edit Shared Template… (opens Settings… > Git > Commits) and Edit for This Repository (copies it as the repository's own and edits it in place); the repository's own is edited there (Save); a file is previewed. The repository's own lives in `<git dir>/amonhen/commit-template` with `commit.template` pointing at it, so git in a terminal uses it too; a shared one is Amon Hen's (`amonhen.commitTemplate`). The commit box takes the template as written; there are no placeholders. Nothing else: `commit.cleanup` stays in All settings. |
| Branches | **Default branch**: the one detected (`origin/HEAD`, then `main`, `master`), or a local branch chosen here for repositories without one. Amon Hen's own key, `amonhen.defaultBranch` in `.git/config`, so the CLI and the extension see it too; it decides "merged" and the trunk in Branches. The source reads "detected" while nothing is chosen. |
| Operations | **Pull**: When pulling, how are the changes brought in? (Merge / Rebase / Fast-forward; `pull.rebase`, and `pull.ff=only` with `pull.rebase=false` for the last). **Fetch**: Remove remote-tracking branches deleted on the remote (`fetch.prune`; Amon Hen's Fetch passes `--prune` only when it is on, as `git fetch` does, so the switch needs no explanation). **Rebase**: Stash changes before rebasing (`rebase.autoStash`; History edits always stash); Move stacked branches along when rebasing or editing History (`rebase.updateRefs`; disabled below git 2.38 with the version found). **Merge**: How conflicts are written in files (Both sides / Both sides and the base / Both sides and the base, shortened; `merge.conflictStyle`; the help says the merge editor works with all three, and that with the base it resolves one-sided and word-level conflicts by itself, so zdiff3 is what Amon Hen recommends). No Push rows: Amon Hen's Push sends the branch to the target chosen in its sheet with an explicit refspec and `--set-upstream`, so `push.default` and `push.autoSetupRemote` only affect a terminal (All settings); Publish starts from `branch.<name>.pushRemote` and `remote.pushDefault`, as git does. |
| Working Copy | **Line endings** (`core.autocrlf`: Keep as committed / CRLF in the working tree, LF in commits / LF in commits). **Track changes to the executable bit** (`core.fileMode`). **Ignore in this repository only**: an editor for `.git/info/exclude` that fills the window's height. Not `status.showUntrackedFiles` (Amon Hen's status always lists every untracked file) and not `.gitignore`, which is a file of the repository (4.7). |
| Remotes | A table of the remotes (name, fetch URL, push URL when different, whether it is on GitHub or similar). Add, Edit and Remove open one sheet: name, fetch URL, push URL (empty means the same), what the URL points to (GitHub, GitLab, a path, an SSH host) and a connection check (`git ls-remote`) before saving. Prune per remote. The same actions are in the sidebar: Add Remote… on the Remotes group's context menu; Rename, Edit URL…, Remove and Prune on a remote's. Repository > Add Remote… opens the same sheet, and removing a remote asks the same question wherever it starts. |
| Hooks · LFS | **Hooks**: a table with each hook's name, when git runs it (before a commit, before a push, …), and an Enabled switch (the executable bit); the selected hook's script is shown under the table with syntax colors (the language from its `#!` line, the diff's colors), with Edit in <editor> (the external editor from Settings > General) and a menu of the other installed editors, and the same on the row's context menu with Reveal in Finder; Create Hook… picks one of the hooks git knows that isn't there yet and puts a `#!/bin/sh` skeleton in place; where they live when `core.hooksPath` is set. **LFS**: whether git-lfs is installed (with how to install it if not), the tracked patterns from `.gitattributes` with Track Pattern… (`git lfs track`) and Remove, and how many files are in LFS. Named by its contents: no single word covers both, and the worktrees are already listed under Working Copy in the sidebar. |
| Keys and Values | git's settings as keys and values, for what the other pages don't ask about. A table like Xcode's Build Settings: Key, Value in effect, then one column per source (system-wide, all repositories, this repository, this worktree) with the value set there. The value in effect is bold. Rows are grouped by the key's dots as Xcode groups build settings: a section (`core`, `remote`, …) and, under it, a subsection (`remote.origin`), each a bold header with a disclosure triangle, every group open at first; a key reads as its last part under its group, and the full key is in its tooltip. Keys are spelled as git documents them (`core.fileMode`). The column that can be written (this repository; all repositories in Settings… > Git; the value in effect in Combined) shows plain text with a field outline on hover (and for the one selected row), and one click on it edits; emptying it removes the key there; the other columns are read-only. "+" adds a row for a key and its value (left without both, the row goes away); "−" and ⌫ remove the selected keys. Every change made from the table (edit, add, remove) can be undone and redone with ⌘Z / ⇧⌘Z, any number of steps while the page is open; the last removal also shows in a line under the table with Restore, which is one ⌘Z. Search by key or value, sorting by any column, ⌘C copies the selected rows as `key = value`. Above the table, Xcode's two switches with Xcode's words, not translated, remembered app-wide, Customized and Levels at first: the rows (Basic: the keys the pages here ask about, set or not; Customized: the keys set at this level; All: every variable git knows, from `git help --config-for-completion`, a blank value and "git's default" for one set nowhere, without the `remote.<name>.…` forms) and the columns (Combined: the value in effect and its source; Levels: a column per source). |

### 4.6 Older git and SSH hosts

- A row whose setting the git in use does not understand is shown disabled, with the version needed and the version found (and the host). It is not hidden, so the user learns why the behavior is missing.
- On an SSH host, every row reads and writes the host's git, and lists (signing keys, remotes' reachability) are collected there. The source reads "all repositories on <host>" for `--global` values there.

### 4.7 What is not on this page, and where it is

| Was here | Now | Why |
| --- | --- | --- |
| Display name, color label | The Repositories screen (rename and Color Label on a repository's context menu) | Attributes of the list, already there |
| Fetch automatically | Settings… > General, one interval for all open repositories | Shared |
| gpg program and the like | Settings… > Git (the machine), found by Amon Hen, overridable | Shared |
| Message cleanup (`commit.cleanup`) | All settings | Not a decision anyone needs to make from here |
| `push.default`, `push.autoSetupRemote`, `status.showUntrackedFiles` | All settings | Amon Hen's own operations pass explicit flags, so they only affect a terminal |
| `.gitignore` | Working Copy, opened as a file; Add to .gitignore on an ignored file's context menu | A file of the repository, shared through commits |
| Global excludes (`core.excludesFile`) | Settings… > Git (the machine) | Shared |
| The git version | The line under the list, which opens Settings… > Git for that machine | It belongs to the machine (this Mac, or the SSH host), not to the repository; it is the same for every repository there |
| Worktrees | The sidebar, under Working Copy | Already listed there |

### 4.8 Stages

Each stage is its own pull request after the page's framework (the list, the rows, the source and ↩︎, Pull and Fetch following git's settings):

1. Author and signing as one choice, and the key collected by the core.
2. The template library in Settings… (on Git > Commits after U21), and the Commits page choosing from it or making the repository's own.
3. Operations as one page; Working Copy with the exclude editor; `.gitignore` out of the page, with Add to .gitignore in Working Copy.
4. Remotes: the sheet with the connection check, and the sidebar's context menus.
5. Hooks · LFS as tables with actions.
6. Keys and Values, git config as the Xcode-style table.
7. The SSH hosts' settings: Settings… > SSH Hosts (chapter 3, U22), after the engine can run on a host without a repository. This Mac's settings are done (Settings… > Git).

## 5. Toolbar

Reference: Fork (the Fetch, Pull, Push, Stash, Pop row), Tower (buttons with a menu per operation).

| Group | Item | Click | ▾ menu |
| --- | --- | --- | --- |
| Location | Branch (in the title position) | Branch switcher panel (fuzzy search, recent branches on top) | — (rename and upstream are in the Repository menu) |
| Sync | Fetch | Fetch the default remote | All remotes, with prune, tags too |
| | Pull | Confirmation sheet: the commits to bring in and the local-only ones, how to integrate (merge / rebase / ff-only, the default from settings) with what that does (a merge commit, rewritten IDs, ff-only fails), a warning about uncommitted changes | — |
| | Push | Confirmation window (modal, movable and resizable): where it goes, warnings worth a look (straight to the default branch, fixup!/WIP commits, files that may hold secrets, binaries added), the commits to send on the left and, on the right, the same message, files and diff as the detail column for the selected commit or for all changes together (read-only). ↩ pushes. If upstream has commits we lack, suggest Pull (Force Push… only from there, listing the commits upstream would lose); if there is no upstream, the button is Publish instead (see below) | — |
| Integrate | Merge | Merge the selected commit or branch into the current branch (choose no-ff or squash in the confirmation sheet) | List of recent branches |
| | Rebase | Rebase the current branch onto the selection | interactive (enters History's edit mode), onto |
| Stash | Stash | Stash (with a message field; ⌥-click stashes immediately) | Include untracked, staged only, selected files only |
| | Pop | Confirm, naming the stash and its message, then pop the latest | — (apply or pop a chosen one in Stashes) |
| Status | Activity | Number of running operations and a spinner. Click to open Activity | — |
| | Refresh | Reload status, refs and History (⌘R) | Reload the whole working tree (without the fsmonitor and status caches), discard the persistent cache and rebuild History |
| | Undo | Confirm, naming the operation, then revert it | — (the operation log in Activity) |
| Search | Search field (`NSSearchToolbarItem`) | ⌘F searches History, ⌘⇧F with a scope | Switch between subject and body / author / ID / path / contents (pickaxe) |

- When the current branch has no upstream (or its remote branch is gone), Push reads Publish (in the toolbar, the Repository menu and Commit and Publish) and asks for the remote, starting from `branch.<name>.pushRemote`, `remote.pushDefault`, the only remote, then origin, and the name there; what it sends becomes the upstream. If that remote branch already exists, it says so and makes it the upstream instead, asking to Pull first when it has commits the branch lacks.
- A local branch that is not checked out is synced from its context menu, as of the last Fetch: Fast-Forward when it is only behind its upstream (moved without a checkout, recorded for Undo; not for one checked out in another worktree, which offers to open that worktree), Push when it is only ahead, and Publish when it has no upstream. A branch that has diverged is checked out to merge or rebase. Fetch never fast-forwards branches by itself.
- Keep Refresh. Changes are detected automatically, but this makes sure things line up when detection misses, such as on network file systems, SSH hosts, or when other tools rewrite `.git` directly. While reloading after a click, the button spins.
- Set `allowsUserCustomization = true` so that Branch, Merge, Rebase, Cherry-pick, Terminal, Finder, Open on GitHub and so on can be added and removed.
- Show ahead/behind both as badges on Push and Pull (macOS 26 and later) and next to the branch name.
- When unavailable, disable the item and give the reason in the tooltip ("No upstream", "Resolving conflicts").
- Deleting a branch and dropping a stash ask first (both can be undone). A branch with commits that are in
  neither the current branch nor its upstream lists them in the sheet and needs Delete Anyway (git's `-D`).
- No ▾ menus on Pull, Push and Branch: one click on an icon must never send or rewrite anything. Operations that leave the machine or move branches (Pull, Push, Pop, Undo) open a sheet that says what will happen; ↩ runs it, so the keyboard costs one extra key. Fetch and Refresh run at once (they change nothing of ours). The menu shortcuts (⌃⌘L, ⌃⌘P) open the same sheets; Commit and Push does not ask again.
- One name in one place: while the toolbar shows the search field, History hides its own (the match count, previous,
  next and "matching only" stay in History); while it shows Branch, the window subtitle leaves the branch out.
- Network git that prints nothing for 120 seconds (usually a credential prompt nobody sees) is stopped with an error saying so. It can also be cancelled from Activity.

## 5a. Stashes

A stash is a commit kept in `refs/stash` and its reflog: the working tree, with the HEAD it was made on (its base) and
the index as parents, plus the untracked files with `-u`. The list is shared by all worktrees; applying goes to the
worktree being edited.

- Selecting Stashes in the sidebar (View > Stashes ⌘4) opens this view: the stashes on the left, newest first, and the
  selected one's contents on the right, in the same column as History's detail (its base is the first parent link).
- Each row shows `stash@{n}` and the message (for a stash made without one, WIP and the subject of its base), then
  the branch it was made on (dimmed when it is not the current branch), where its base stands against the edited
  worktree's HEAD (at HEAD, N behind HEAD, or, in orange, not in HEAD's history, where applying is likelier to
  conflict), when, whether it holds untracked files, and the base's short ID.
- Apply, Pop, Drop… and Branch… sit above the list and in each row's context menu. Apply and Pop restore what was
  staged as staged when they can (`--index`). Drop asks first and can be undone. Branch… is `git stash branch`: it
  creates a branch at the base, switches to it and applies the stash there, so it does not conflict however far HEAD
  has moved. With two or more worktrees, the bar says which worktree it applies to (✎).
- After the last stash is popped or dropped, the view switches to History.

## 5b. Reflog

The reflog of the edited worktree's HEAD: every place HEAD has been, newest first, including commits that a reset,
rebase or amend left behind and that no branch reaches any more. It is where those are found and brought back.

- Selecting Reflog in the sidebar (View > Reflog ⌘5) opens this view: the entries on the left and the selected
  entry's commit on the right, in the same column as History's detail.
- Each row shows `HEAD@{n}` and what was done (git's message: `commit: …`, `checkout: moving from a to b`,
  `reset: moving to …`), then, in orange, "not on any branch" when no ref reaches the commit, when, the short ID and
  the commit's subject.
- New Branch… and Checkout (detached) sit above the list. Each row's context menu also has New Tag, Reset ▸ (section
  5c), Show in History (for commits History has), Copy Commit ID and Copy Name. With two or more worktrees, the bar says whose HEAD it is
  (✎).
- It reloads when HEAD moves and when the edited worktree changes.

## 5c. Reset

"Reset <branch> to <commit> ▸" in the context menu of a History row and of a Reflog row moves the edited worktree's
current branch (HEAD when detached) to that commit, like `git reset`. It asks first, naming the branch and the commit.

- Soft keeps the changes of the commits taken off staged; Mixed keeps them in the working tree, unstaged; Hard makes
  the index and the working tree match the commit.
- Hard keeps uncommitted changes (untracked files are left alone, as git does) as a stash first ("Before reset --hard
  to …"), so nothing is lost.
- All three can be undone (⌥⌘Z): Soft and Mixed with the same kind of reset, Hard with `--keep`. Commits taken off
  stay reachable from Reflog. Not during a merge, rebase, cherry-pick or revert.

## 6. Working Copy

Reference: Tower and JetBrains (a single checkbox list, commit input on top).

### Left column (from the top)

```
┌ Commit ─────────────────────────────────┐
│ [Subject                      ] 42/50   │
│ ┌ Body (grows with the text) ────────┐ │
│ └────────────────────────────────────┘ │
│ ☐ Amend  ☐ Sign-off   [Commit ▾] ⌘↩   │
├ Changes ──────────── [≡ List|🌲 Tree] ┤
│ 🔍 Filter                              │
│ ☐ Changes (4 / 2 included)             │
│   ☑ M  src/lib.rs            +12 −3    │
│   ▣ M  src/main.rs           +5  −1    │  ← only some lines included
│   ☐ M  README.md             +1  −1    │
│   ☐ ?  notes.txt                       │
└────────────────────────────────────────┘
```

### A single checkbox list

Drop the two lists, Staged and Unstaged, and put all changed files in one list, where each row's checkbox shows "whether it goes into the next commit". The checkbox is the index (stage) state itself; the FE keeps no selection state of its own (a `git add` in the terminal shows up as is).

| State | Meaning | On click |
| --- | --- | --- |
| ☑ | All working tree changes are staged | Unstage the whole file |
| ▣ | Only partly staged (lines or hunks included) | Stage everything |
| ☐ | Not staged | Stage everything |

- The header row (Changes) and directory rows also have three-state checkboxes that toggle everything below them.
- The header shows "total / included". Commit cannot be clicked when nothing is included (except with Amend).
- New files (?), deletions and renames are treated the same way. Conflicted files show ⚠ instead of a checkbox, and clicking it opens the merge editor.

- Put the commit input on top (as in JetBrains and GitHub Desktop, show "what to write" first). The input can be collapsed (to just the one subject line).
- Choose from three ways to show the list: List (paths in one column), Tree (a directory tree, collapsing levels with a single child; done), Flat (file name, with the parent directory in faint text). Sort by path / status / extension.
- A filter field (fuzzy search on paths). Helps when hundreds of files change.
- Each row shows +/− line counts and a status icon (M / A / D / R / ? / U), and a Discard button on hover. Space toggles the check of the selected files (several allowed), ⌫ discards (with confirmation).

### Making commits easier to write

| Feature | Details |
| --- | --- |
| Character count | Show how much of the subject is left, with a caution color at 50 and a warning color at 72. A guide line at column 72 for the body |
| Draft | Saved automatically per repository. Not lost until committed. Starts from `commit.template` if there is one |
| Past messages | ⌥↑ / ⌥↓ in the subject field recalls recent messages. ▾ for a list |
| Completion | `Co-authored-by:` (from past authors), `#123` (GitHub issues), Conventional Commits types (`feat:` and so on), the issue number in the branch name |
| Commit ▾ | Commit / Commit and Push / Amend / Amend (keep the message) / Fixup! the selected commit |
| Amend | When turned on, fill in the previous message and show "the difference from HEAD" above the diff. Warn if already pushed |
| hooks | Show the progress of pre-commit and other output in a sheet; on failure, keep the output and do not clear the message |
| AI | "Suggest a message" (from the staged diff). See below |
| Reason when disabled | When Commit cannot be clicked, show the reason next to the button ("No staged files") |

### Fine-grained staging in the diff on the right

Reference: GitHub Desktop's checks in the line gutter, JetBrains' per-line include and exclude.

Since the list is a single one, the diff is a single one too. The right side shows the diff from HEAD → working tree, and a check in the gutter on each changed line shows "whether it goes into the next commit" (whether it is in the index).

```
 ☑ ▾ @@ fn parse @@                         [Discard hunk]
     12   let a = 1;
 ☑ − 13   let b = 2;
 ☑ + 13   let b = 3;
 ☐ + 14   dbg!(b);          ← line only in the working tree
```

- The core overlays the two diffs HEAD → index and index → working tree, and returns each changed line of HEAD → working tree marked with "staged or not" (`working_diff_combined`, `STAGED` in the line's `flags`). Files whose index cannot be expressed as "HEAD with part of the changes applied" (for example, a change only in the index that was reverted in the working tree) are shown per file with the old Unstaged / Staged switch.
- Toggling rebuilds and writes "which changes applied to HEAD make up the index" (`set_rows_staged`). Including everything is the same as `git add`, and excluding everything the same as `git reset` (for new and deleted files).
- The check on the hunk header has three states; clicking it includes or excludes the whole hunk.
- Clicking a line's check toggles that line. Drag in the gutter to select a range, ⌘-click to add to the selection, and Space toggles them all at once.
- Clicking the boundary of a block of changes (consecutive − and ＋) selects just that block.
- Keys: Space toggles the check of the selected lines (or the current hunk if none), `D` discards, `J` / `K` go to the next / previous hunk, `⌥J` / `⌥K` to the next / previous file.
- After toggling, stay in the same position and move the cursor to the next hunk (update the display optimistically and reconcile when status settles).
- Display options (the bar above the diff): unified / split, ignore whitespace, context lines (3 / 10 / all), wrapping.
- Discard keeps the contents in the oplog and can be undone (the current working tree contents are written to a blob).

## 7. History's right column

### Reading the whole message

- Stop collapsing at 16 lines; show the whole message by default. The boundaries between the top (details), middle (changed files) and bottom (diff) of the right column can be dragged, and their positions are remembered.
- A "⤢" at the top right of the details pane: click it to expand only the details to fill the right column (click again to restore).
- The body can be selected, copied and searched (when the right column has focus, ⌘F searches inside it).

### A diff view like a GitHub PR

In addition to today's "choose a file to show only its diff", offer a view that "stacks all files vertically".

```
┌ Changed files (tree) ┬ Diff (all files stacked) ──────────────────────────────┐
│ ▾ src                │ ▾ src/lib.rs                       +12 −3  ☐ Viewed  ⋯ │ ← headers stick to the top
│    lib.rs   +12 −3   │   @@ fn main @@                                        │
│    main.rs  +1  −0   │   …                                                    │
│ ▾ docs               │ ▾ src/main.rs                      +1 −0   ☐ Viewed  ⋯ │
│    design.md +40     │   …                                                    │
│ 3 files +53 −3       │ ▸ Cargo.lock (collapsed: generated file)  +210 −180    │
└──────────────────────┴────────────────────────────────────────────────────────┘
```

- Clicking a file in the tree on the left jumps to it. The tree's selection follows scrolling.
- Each file can be collapsed. Lockfiles, `linguist-generated`, binaries and anything over 1000 lines start collapsed.
- Marking a file "Viewed" collapses it and puts a ✓ in the tree. Remembered per commit (for reviews that span several commits).
- The header's ⋯: blame, file history, open the contents as of this commit, open in the editor.
- How it works: the core takes the diffs of all files once and holds them (`StackedDiff`; taken in parallel when there are many files), and builds a view that joins "a file header row (`DiffRow::FILE`) + each file's rows" into one. The strings keep sharing the file contents. Collapsing and expanding only rebuild the view. The Metal machinery of the diff view works as is. On linux, the time from choosing a commit to the diff appearing is the same as with one file at a time (p50 15–16 ms).
- The single-file view (today's) stays, chosen with a switch in the header.

### When several commits are selected

| Selection | What is shown |
| --- | --- |
| A contiguous range (⇧-click, on a single first-parent line) | The diff from the parent of the oldest to the newest (the same as the squashed result). On top, "5 commits · 2 authors · 3 days · +120 −40" and the list of included commits (click one to show it alone) |
| Two (⌘-click) | The diff between the two (A..B). ⇄ swaps the direction, and "from merge-base" (A...B) can also be chosen |
| Three or more, scattered | Group each commit's diff by file (under the file header, a subheading says which commit the change is from) |

- Right-click while selected: cherry-pick (keeping the order), revert, squash (when contiguous), copy the IDs together, save as a patch.
- When the ends of the range span a merge, treat it as a first-parent range and say so.
- How it works: the core's `Selection::of` follows the first-parent chain in the History graph, and `selection_diff` builds the diffs of all files with the backend's `changes_between` (the tree diff between two commits). When scattered, each commit's diff is laid out under a "path — short ID" header, ordered by path and then oldest first.

## 8. Resolving conflicts (the embedded merge editor)

### Flow

1. When a merge, rebase, cherry-pick or stash pop produces conflicts, show a bar under the toolbar and select Conflicts in the sidebar.
2. The Conflicts screen is "left: the list of conflicted files (unresolved / resolved)" and "right: the merge editor". Not a separate window; it can only be popped out with ⤢.
3. When everything is resolved, Continue on the bar is highlighted. For a rebase, Continue moves on to the next commit, and if there are new conflicts it goes straight on to open the next file.
4. Abort from the bar at any time (it stays in the oplog, so Undo can also revert it).

### Editor layout (matching JetBrains)

```
┌ Ours (main) ───────────┬ Result ──────────────────┬ Theirs (feature) ─────┐
│                        │                          │                       │
│  Change A   ≫ ✕ ───────┼──▶                       │                       │
│                        │      ◀───────────────────┼─ ✕ ≪   Change B       │
│  Conflict   ≫ ✕ ═══════╪══ ⚠ Conflict ════════════╪═ ✕ ≪   Conflict       │
└────────────────────────┴──────────────────────────┴───────────────────────┘
 ✨ Magic Merge   ← → next / previous change   Show Base   Accept Ours / Accept Theirs   4 changes · 1 conflict   [Save & Mark Resolved]
```

- Connect the three columns with a band (curve) per change, with colors for the kind: green for additions, blue for modifications, grey for deletions, red for conflicts.
- Each change has "≫ / ≪ take" and "✕ discard". For a conflict, taking both puts them one after the other (⌥ chooses which comes first).
- Result can be edited directly (`NSTextView`, with the OS's standard Japanese input).
- Base is hidden by default; "Show Base" adds a fourth column (or overlays the difference from Base on each of the Ours and Theirs columns).
- Drop today's "three panes on top, Result below" layout and use only three columns. Vertical scrolling of the three columns is kept in sync, aligning the changes (differences in line counts are absorbed by the bands).

### Magic Merge (clean up automatically)

The equivalent of JetBrains' "Apply non-conflicting changes" and "Resolve simple conflicts". Pressing ✨ applies the following in order; what was applied stays, with its band in a fainter color (each can be reverted one at a time).

1. Take all non-conflicting changes (places only one side changed).
2. Where both sides made the same change, keep one.
3. Conflicts that differ only in whitespace or line endings go to the side set in settings (by default, Theirs' contents with Ours' whitespace).
4. Conflicts that changed different words within the same line get a word-level 3-way merge (word boundaries are the same as the diff view's word diff).
5. Where one side only added lines (such as adding imports) and the ordering conflicts, put both (when the language's grammar is known, only when tree-sitter can tell that it is an ordering of imports or uses).

- Only 1 and 2 are applied automatically when a file is opened (can be turned off in settings). The number of remaining conflicts is shown in the list and the tab.
- All computation is in the core (added to `mergeSides`, `resolveText`, `conflictRegions`). Word-level merging and the checks are pinned down with tests.
- Right-click in the list: "Resolve this file with Ours / Theirs", "Open in external merge tool".
- Conflicts left after Magic Merge can get a resolution suggested by AI (chapter 9). "AI で解消" (Resolve with AI) sits next to ✨ Magic Merge on the bar below the editor.

## 9. AI (translation, explanation, message suggestions, conflict resolution)

| Where | Button | What it does |
| --- | --- | --- |
| History details | 翻訳 (Translate) | Translates the subject and body into the language from settings, shown collapsible under the original |
| History details | 解説 (Explain) | Summarizes "what changed and why" and "places likely affected" from the commit's diff and message. With several selected, the whole range |
| Working Copy | 提案 (Suggest) | Suggests a message from the staged diff (matching how the repository has written them before) |
| A diff hunk | 解説 (Explain) | Just that hunk |
| Merge editor | 解消 (Resolve) | Builds a resolution for one conflict, all remaining conflicts in the file, or all files (see "Conflict resolution" below) |
| Merge editor | 解説 (Explain) | For one conflict, explains the intent of each of Ours and Theirs (from the messages and diffs of each side's commits) |

### Conflict resolution

- Magic Merge goes first; AI gets only the remaining conflicts. Do not hand AI what can be decided mechanically (faster, and no errors creep in).
- What is sent: the conflict's Base, Ours and Theirs with surrounding lines, the file's path and language, and the subjects and bodies of the commits involved in the conflict on both sides (for a merge, each side of `merge-base..`; for a rebase, the commit being applied).
- The response format is fixed (resolved text per conflict and a short reason, as JSON). A response with the wrong shape or the wrong number of conflicts is discarded and reported as an error.
- The result goes into Result as a "suggestion", shown with a purple band and a callout with the reason. Each conflict has "採用" (Accept), "却下" (Reject) and "編集して採用" (Edit and accept). Nothing is marked resolved until accepted. Accepting all at once is per file, with confirmation.
- Accepted suggestions stay in the oplog and can be reverted with Undo.
- If a suggestion still contains conflict markers (`<<<<<<<` and so on), it cannot be accepted. When the language is known, tree-sitter checks for broken syntax and warns if it is broken.
- The whole file is sent only when the conflicts make up most of it. Otherwise, only the surroundings of the conflicts.

- The backend is a local CLI: either Claude Code (`claude -p --output-format stream-json --verbose --include-partial-messages --tools "" --no-session-persistence`, started in an empty temporary directory so it does not read the repository's CLAUDE.md) or Codex (`codex exec --json --sandbox read-only --skip-git-repo-check -`), chosen explicitly in Settings > AI. Whatever is found is never used on its own; the AI buttons do not appear until one is chosen. Not split per purpose; one for everything.
- Both are started as child processes; the prompt and diff are passed on standard input, and the streamed JSON is read. The app does not manage keys; that is left to the CLI's login.
- Apple Intelligence (on-device, macOS 26 with Apple Intelligence on) uses the Foundation Models framework directly: nothing leaves the Mac, so there is no per-repository question, but it reads only about 4,000 tokens, so the longest variable (usually the diff) is cut to fit. Good for translations and short explanations; Claude Code or Codex for large commits and resolving conflicts.
- The model is remembered per CLI, chosen from a pop-up (Claude Code: Haiku, Sonnet, Opus, Fable; Codex: the models listed in `~/.codex/models_cache.json`; "Other…" for any name), and passed with `--model` (`-m` for Codex). Claude Code defaults to the fast `claude-haiku-5-5`; `claude-sonnet-5-5` can be chosen for explanations only.
- Started with tools disabled (no available tools for Claude Code, a read-only sandbox for Codex), because explanation and translation never need to write files or run commands. Check the exact flag names against `--help` of the local version at implementation time.
- Calls are confined to the `Assistant` trait in a small crate outside the core (`amonhen-assist`), with one implementation per CLI. Output parsing is kept small and separate per CLI, and checks the version (the shape of Codex's `exec` events changes easily). Process spawning is injected from `Platform` (even for repositories on an SSH host, the CLI runs locally).
- When the chosen CLI is not found or not logged in, the buttons are disabled with the reason shown.

### Prompt settings (Settings > AI > Advanced)

The prompt can be edited per purpose (translation, commit explanation, hunk explanation, message suggestion, conflict resolution, conflict explanation).

- Each purpose has a built-in default prompt, and only edited ones are saved. There is "既定に戻す" (Restore Default) and a view of the difference from the default.
- A prompt is a template with substitution variables. The available variables are listed per purpose, and unknown variables are an error (e.g. `{{diff}}`, `{{message}}`, `{{files}}`, `{{language}}`, `{{base}}`, `{{ours}}`, `{{theirs}}`, `{{ours_commits}}`, `{{theirs_commits}}`, `{{path}}`, `{{recent_messages}}`).
- The response format (the JSON shape) is decided by the app and cannot be changed by editing the prompt. It is appended to the end of the prompt automatically.
- A "試す" (Try) button: runs the prompt being edited on the current selection in the current repository and shows what is sent next to the result (without the cache).
- Saved in `~/Library/Application Support/Amon Hen/prompts/<purpose>.md` (editable by hand, portable with dotfiles). If the repository has `.amonhen/prompts/<purpose>.md`, that takes precedence (to match how a team or repository writes things). Which one is in use is shown in Settings and in the button's tooltip. Since repository prompts also come along with clones, show their contents for confirmation the first time they are used and whenever they change (the permission is remembered by repository and prompt hash).
- The cache key also includes a hash of the prompt contents (edits trigger a refetch).
- Before sending, show what will be sent (diff size, number of files), and allow "このリポジトリでは使わない" (Don't Use in This Repository) per repository. Disabled by default.
- Results are shown as they stream, and stored in the persistent cache keyed by commit ID + kind + language + CLI + model + prompt hash (shown instantly the second time).
- Large diffs are trimmed to the list of files and line counts + the diffs of the visible files.

## 10. Activity (internal operations, history, performance)

Window > Activity (⌥⌘A). Also opens from the toolbar spinner. One for the whole app, not per repository (repositories can be filtered on the left).

| Tab | Details |
| --- | --- |
| Operations | Running and finished operations. The git command line, start time, duration, exit code, standard output and error. Running ones show progress (bytes received for fetch and clone) and can be cancelled. Failed ones are red, and the bar notification jumps here |
| History | The operation log (oplog). Operation name, HEAD before and after, changed refs. Choose a row and "ここまで戻す" (Undo to Here) |
| Performance | Histograms and latest values of input-to-display time (History scrolling and selection, showing diffs), dropped frames, time per core Intent, memory of History chunks, persistent cache hit rate. Shows the same values as `FrameProfile` and os_signpost |
| Events | FSEvents notifications, status recomputation, traffic with the daemon (over SSH) |

- The core records operations and measurements in a ring buffer (with a size limit), and the FE reads it only while Activity is open (no drawing cost when closed).
- Right-click to export as JSON (for bug reports and comparing measurements).

## 11. Menus

Make every operation available from the menus. Shortcuts do not follow other clients; they follow our own consistent rules.

| Modifier | Operations assigned |
| --- | --- |
| ⌘ + letter | macOS standard (⌘N, ⌘T, ⌘O, ⌘W, ⌘F, ⌘Z, ⌘,), plus switching views (⌘1–3), Refresh (⌘R), Commit (⌘↩) |
| ⌃⌘ + letter | Syncing with remotes (Fetch, Pull, Push) |
| ⌥⌘ + letter | Local writes (Stash, Pop, Branch, Tag, Merge, Rebase) |
| ⌘⇧ + letter | Opening and finding (Quick Open, Find in History, Go to Ref) |
| No modifier | Only when a list or diff has focus (Space, `D`, `J`, `K` and so on) |

- Use the initial letter (F for Fetch, P for Push). When initials collide, the more frequently used one gets it.
- macOS's "Keyboard > Keyboard Shortcuts > App Shortcuts" can change shortcuts by menu name, so the app has no screen for editing them.
- The table below is a proposal following the rules.

| Menu | Items |
| --- | --- |
| Amon Hen | About, Settings… ⌘,, Services, Hide, Quit |
| File | New Window ⌘N, New Tab ⌘T, Open… ⌘O, Quick Open… ⌘⇧O, Open Recent ▸, Clone… ⌃⌘C, Add Remote Repository…, Init…, Close Tab ⌘W, Close Window ⌘⇧W |
| Edit | Standard (Undo and Redo undo text edits), Find ⌘F, Find in History ⌘⇧F, Find Next / Previous ⌘G / ⌘⇧G |
| View | Working Copy ⌘1, History ⌘2, Conflicts ⌘3, Stashes ⌘4, Reflog ⌘5, Refresh ⌘R, Show Sidebar ⌃⌘S, Diff: Unified / Split ⌥⌘D, Ignore Whitespace, Wrap Lines, Font Size ⌘+ / ⌘−, History display (one line, History Items ▸ to choose branches and tags, author, date, commit ID, signature and CI status, History Order ▸ Date Order / Topological Order (both also from the button at the right of History's bar and the header's context menu, whose menu also has One Line per Commit, Fold Merged Branches and Highlight Ancestors, and from Settings > Appearance), avatars, ancestor highlighting), Commit Details: Compact / Standard / Detailed |
| Navigate | Go to HEAD ⌘⇧H, Go to Ref… ⌘⇧J, Go to Parent / Child ⌥⌘↓ / ⌥⌘↑, Next / Previous Hunk, Next / Previous File, Next Conflict |
| Repository | Fetch ⌃⌘F, Pull ⌃⌘L, Push ⌃⌘P, Stash ⌥⌘S, Pop ⌥⌘⇧S, New Branch… ⌥⌘B, New Tag… ⌥⌘T, Merge… ⌥⌘M, Rebase… ⌥⌘R, Interactive Rebase…, Continue / Skip / Abort, Undo (operation name) ⌥⌘Z, Repository Settings ⌘;, Open in Terminal, Reveal in Finder, Open in Editor, Open on GitHub |
| Commit | Commit ⌘↩, Commit and Push ⌘⇧↩, Amend ⌥⌘↩, Include All, Exclude All, Toggle Selection (Space), Discard…, Cherry-pick, Revert, Reword…, Create Patch… |
| Window | Standard (minimize, tabs, merge all windows), Repositories ⌘⇧R, Activity ⌥⌘A |
| Help | List of shortcuts, open the log location |

- Context menus show the same operations under the same names as the menu bar (one name to remember).
- Unavailable items are disabled, not removed (so their place can be remembered).
- Undo for git operations lives in Repository > Undo (⌥⌘Z), separate from Edit's ⌘Z (undoing text). So that ⌘Z never unexpectedly reverts a git operation.

## 12. Integration with GitHub and others

| Feature | Details |
| --- | --- |
| Accounts | Left to the local `gh` (GitHub CLI). The app never touches tokens; it spawns `gh` to read and write (log in with `gh auth login`). Without gh, the integration features are not shown. GitLab and Gitea later |
| Remote detection | Detect the GitHub repository from the URL of origin and others (the same as today's `#123` links) |
| Pull Requests | A list in the sidebar (mine, review requests, all). Status (draft, review, CI). Check out, open in the browser |
| Creating a PR | "Create Pull Request…" from Repository, from a local branch's context menu, and from the branch's status popover once the branch is pushed and has no open pull request (the popover shows the open one instead). A branch not pushed yet, or ahead of its upstream, gets the Push confirmation window first; create the pull request after its CI has run. Title and body from the branch's last commit (an AI summary can also be chosen), choice of base, draft |
| CI status | Show ✓ / ✗ / ● on branch heads and each commit in History (only the visible range, fetched conditionally with ETag and stored in the persistent cache) |
| Avatars | GitHub avatars if there is a GitHub account, otherwise today's Gravatar |
| Clone | Choose from the list of your own repositories and clone |

- Communication runs gh in the background and never makes the UI thread wait (currently inside the macOS FE; when GitLab and others are added, move it to a crate outside the core in a form swappable via `HostingProvider`). When something cannot be read, it goes to Activity's Events and does not affect other operations.

## 13. Language

- The app is written in English and localized; Japanese is the first translation
  (`apps/macos/Localization/ja.lproj/Localizable.strings`, used through `L()` in `Localization.swift`). The language
  follows the system's preferred languages.
- git's operations and the app's screens keep their English names in every language, because those are the words
  people search for and see in git's own output: Working Copy, History, Activity, Repositories, Branches, Remotes,
  Tags, Stashes, Pull Requests, Summary, Description, Diff, Blame, Commit, Amend, Push, Pull, Fetch, Merge, Rebase,
  Stash, Pop, Cherry-pick, Revert, Reset, Squash, Fixup, Reword, Stage, Unstage, Discard, Clone, Undo, Continue,
  Skip, Abort, Force Push, Fast-Forward, HEAD. A button or menu item that is only such a phrase ("New Branch…",
  "Stage Lines") is not translated.
- Everything else is translated: standard macOS menus and items (with Apple's wording), explanations, confirmations,
  placeholders and tool tips. In Japanese prose ブランチ, タグ, リモート, 上流, コミット, 作業ツリー and コンフリクト are
  used as nouns.
- The core (Rust) is localized the same way (`tr!` in `amonhen_model::i18n`, Japanese in
  `crates/model/src/i18n/ja.strings`). The language comes from the side that shows the text: the app passes its UI
  language over FFI, the CLI reads `LC_ALL` / `LC_MESSAGES` / `LANG`, and the shared daemon and SSH hosts answer
  each connection in the language its hello named (the daemon's own work uses `AMONHEN_LANG` from whoever started it).
  git's own messages stay English (it runs with `LC_ALL=C`).
- The VSCode extension's UI is not localized yet.

## 14. Worktrees

Tools such as Claude Code create worktrees all the time (for example under `.claude/worktrees/`). Opening each one as
another repository is tedious, so a repository's worktrees live in its tab. The design follows git's own model.

What git says, and what the app keeps:

- Worktrees are peers. Each is a working tree with its own HEAD and index; the main one only holds `.git`. The app
  lists them like `git worktree list`: the main one first, all the same otherwise.
- Branches, tags, remotes, objects, the stash list and config are shared by all worktrees. HEAD, the index, the
  working tree files and an operation in progress (rebase, merge…) belong to one worktree.
- git commands act on the worktree they run in. The app keeps one **worktree it edits** (git's "the worktree you are
  in"), and only commands that touch HEAD, the index or the working tree go there: checkout, commit, amend, stage,
  discard, merge, rebase, pull, reset, cherry-pick, revert, stash (push and pop) and Undo. Repository-wide commands
  (fetch, creating, renaming or deleting branches and tags, remotes) work the same from any worktree.
- A branch can be checked out in only one worktree. The app never forces it (`--ignore-other-worktrees`): choosing a
  branch that another worktree has checked out moves the edited worktree there instead.
- "The current branch" is the edited worktree's branch: bold with an accent icon in Branches, the filled label in
  History, the toolbar's Branch. Three marks tell worktrees apart everywhere: ✎ the edited worktree, ⌂ the main
  worktree (the one holding `.git`), ⑂ a linked worktree. In Branches, the current branch gets ✎, the main worktree's
  branch ⌂, and a branch checked out in a linked worktree ⑂ with a green icon (its worktree in the tooltip); History
  labels each worktree's HEAD with the same mark, and a linked worktree's branch label is green; the branch switcher
  says "in the worktree …". Worktrees on a detached HEAD have no branch, so they show only under Worktrees and in
  History.
- Worktrees are created and removed with git (`git worktree add`, `remove`, `prune`; listed with
  `git worktree list --porcelain`), never by editing `.git/worktrees`. Removing one with changes asks, then uses
  `--force`; locked worktrees are shown as locked and never pruned; a worktree whose folder is gone is shown as
  prunable.
- The other worktrees are not opened just to count their changes: the opened repository's engine runs `git status`
  in each of them (eight at a time) and answers for all of them in one request (`worktreeChanges`; over SSH one
  connection serves every worktree). The engine remembers the counts and counts again only the worktrees that changed:
  it watches every worktree's HEAD and index under `<git dir>/worktrees` (a few files, so staging, commits and
  checkouts show anywhere within 10 seconds), and counts the worktrees with changes again every 30 seconds. It does
  not watch the worktrees' files: Linux's inotify uses one watch per folder (about 1 KB of kernel memory each), and a
  large tree like minix has 17,000 folders, so watching 59 would reach the limit of about a million, and even a few
  cost tens of megabytes for numbers that may lag a little. The window asks every 10 seconds while in front, which
  runs no git when nothing changed, and counts everything again every 5 minutes, to see a clean worktree whose files
  were edited without staging. Only the worktree being
  edited and the two edited before it stay open.
- The Undo log is per worktree (`<git dir>/amonhen/undo.json`, and a linked worktree has its own git dir), like
  HEAD; undoing something that moved a shared branch is seen from every worktree.

Viewing mixes them, editing names one:

- **View (all worktrees together).** Under Working Copy in the sidebar, every worktree is listed with its number of changes (✎ the edited one); picking
  one edits it and shows its changes. The main worktree comes first, then the rest by name (w2 before w10); worktrees with no changes are
  folded into "No changes N" (closed at first) so the ones being worked on stand out; worktrees whose folders share a name show
  enough parent folders to tell them apart (`aslr/src`, `ssp/src`); the main worktree, the edited one, a missing or a locked one always shows. History labels each worktree's HEAD commit (green, before the branch labels: ✎ for the edited one, ⌂ for the
  main worktree, ⑂ for linked ones, with the number of changed files), so where every worktree is and how much it has changed shows on one
  screen. Working Copy itself shows only the edited worktree's changes (an earlier version listed every worktree's changes there; with many worktrees that was hard to scan, so the list moved to the sidebar).
- **Edit (one worktree, always shown).** The toolbar shows the edited worktree (✎ name) next to its branch. Picking a
  worktree in the sidebar or from ✎ makes it the edited one. Confirmation sheets name the worktree ("Pull into
  gui-gtk4").
- **Elsewhere.** The repository page and lists show worktrees under their repository, not as repositories of their
  own (also when a scan finds them). Opening a worktree's folder (the CLI, Finder) opens its repository's tab with that
  worktree edited. The CLI works on the worktree it runs in, as git does; `amonhen worktree` mirrors `git worktree`
  (list, add, remove, prune).
- New Worktree… (from a branch or commit, folder chosen), Remove Worktree, Prune, Reveal in Finder, Open in Terminal.

## Implementation order

Starting with what has the biggest felt effect and fewest dependencies. At each step, update the checkboxes, as with "Implementation order" in `docs/design.md`.

- [x] U1. Complete menus and tidy shortcuts, the Settings window (move today's toggles there). Menus, shortcuts following the rules, Settings, File > Clone… (⌃⌘C; with gh, choose from your own GitHub repositories; once cloned, add to bookmarks and open in a tab) are done
- [x] U2. Reorganize the toolbar (buttons with menus, Merge, Rebase, Pop, the Activity spinner, search field, customization). Pull ▾, Push ▾, Merge, Rebase, Stash, Pop, Activity, Refresh, Undo, branch (click for the switcher panel ⌘⇧B: fuzzy search, recently switched on top, remote branches create a tracking branch, a new branch if nothing matches; ▾ for New, Rename, Set Upstream), the History search field (⌘⇧F, ▾ for author:, path:, contents:), customization are done. Later the ▾ menus on Pull, Push and Branch were replaced by confirmation sheets (section 5)
- [x] U3. Rearrange Working Copy's left column (commit input on top, a single checkbox list, List / Tree / Flat, filter) and strengthen the commit input. List / Tree / Flat, per-row +/− (the core's `workingStats`), "変更を捨てる" (Discard Changes) on hover, per-repository drafts, starting from `commit.template`, recent messages with ⌥↑ / ⌥↓ and the 🕘 list, Conventional Commits types, Co-authored-by (recent authors) and Signed-off-by from ⊕, Commit ▾ (Commit and Push ⌘⇧↩, Amend), the column-72 guide line in the body, typing `#` lists open issues and PRs (gh, remembered for 5 minutes; when typed with an IME, after the text is committed), Fixup Commit For and Squash Commit For in Commit ▾ (choosing from recent commits fills in a fixup! or squash! subject), and the hooks output sheet (shown even on success if the hooks wrote anything; readable in a monospaced font even when they stop) are also done
- [x] U4. A single HEAD → working tree diff with per-line checks (the core overlays two diffs), line selection in the gutter, and key operations. Line and hunk checks, Space, and the Unstaged / Staged switch for files that cannot be overlaid are done. J / K for the next / previous hunk, ⌥J / ⌥K for the next / previous file, and selecting the next hunk after including or excluding are also done
- [x] U5. Show History details in full, and drag the boundaries. The body is shown in full without collapsing, boundary positions are remembered, ⤢ expands only the details. ⌘F while the details pane has focus (search inside the pane) is also done
- [x] U6. A PR-style diff in History (all files stacked vertically, Viewed). File headers (collapse and expand, line counts, Viewed), headers that stick to the top, linking list selection and scrolling, lockfiles and files over 1000 lines collapsed at first, one file at a time beyond 200 files, switching from the View menu and Settings are done. Remembering Viewed until the next launch (up to 500 commits), right-click on headers (collapse, Viewed, Blame, File History, copy path, Finder, default app), and showing the changed files as a tree to the left of the diff (switch with View > Changed Files as Tree and Settings; folders with a single child are joined) are also done
- [x] U7. Multiple selection in History (range, two, scattered). Select with ⇧- and ⌘-click; the core decides how to present it (`Selection`) and it is shown as a diff with all files laid out. The upper pane shows the list of selected commits and how the diff is taken. Switching how the diff of two is taken (A..B, swap direction, from merge-base A...B; via links in the upper pane) and right-click on a multiple selection (cherry-pick oldest first, revert newest first, squash a contiguous range (the message is edited from the messages laid out oldest first), copy IDs) are also done
- [x] U8. Embedding the merge editor, the three-column layout and bands, Magic Merge (the core's word-level merge). Embedded on the right of Working Copy (⧉ to pop out into a separate window), the three columns Current, Result and Incoming (Base can be toggled), bands connecting conflicts and ≫, ≪, scrolling aligned at conflict positions, Magic Merge rules 1–5 (including detecting import ordering), the window bar (operation in progress, number of conflicts, Continue / Skip / Abort), Conflicts in the sidebar (a count badge; click to open the first file), applying only the unmistakable rules automatically when opened (can be turned off in settings), and undoing Magic Merge one conflict at a time with ⌘Z are done
- [x] U9. Window tabs, and restoring the open tabs. Opened repositories go into a tab of the front window, only ⌘N makes a new window. The tab bar's "+" and ⌘T, tab operations in the Window menu, recording and restoring the tab order per window, background tabs waiting to reload the working tree until they come to the front, ● and ↑↓ in tab names, and background tabs keeping only the History chunks near what is visible (putting linux in the background after reading about 6,000 rows: 397 → 356 MB) are done
- [x] U10. Bookmarks (groups, folder scanning, status display, Quick Open). Headings for Favorites, Recent, Bookmarks, manual groups and scanned folders, right-click (favorite, name, group, Finder, terminal), scanning (depth, excludes, rescanned each time the list opens), row status, Quick Open (⌘⇧O, ⌘T, ⌘N, fuzzy search), watching scanned folders with FSEvents (rescan when repositories appear or disappear; changes inside found repositories are not watched), reordering by drag (bookmarks, groups, favorites), color labels, bulk Fetch of the selection or everything under a heading (4 at a time, failures in red on the row), and automatically registering opened repositories (except under scanned folders; can be turned off in Settings) are also done
- [x] U11. Repository settings (git settings and their origin, Remotes, Ignore). A page in the sidebar's Workspace (Repository > Repository Settings, ⌘;; it was a sheet until step 21 of the design) with Identity, Branches, Remotes, Ignore and All Settings. Each item with the value in effect and where it is set, overriding and clearing in this repository (local), adding, renaming, URL, removing and pruning remotes, editing `.gitignore` and `.git/info/exclude`, General (display name, color label, auto-fetch while open; app-only settings stored in the bookmark), Hooks and LFS (hooks in .git/hooks or core.hooksPath and whether they are executable, whether git lfs is installed, LFS patterns in .gitattributes), and examples and counts of matching files for each Ignore line (using the unsaved contents, asking git ls-files -i) are also done
- [x] U12. Activity (the core's ring buffer, Operations, History, Performance, Events). Window > Activity (⌥⌘A) and Activity in the toolbar (spins during operations). Operations has git invocations and core commands (repository, contents, time, success or failure, error, filtering, JSON export), History has the operation log and "ここまで戻す" (Undo to Here), Performance has p50 / p95 / max of History input → display, selection → diff and Working Copy reloads, Events has file change notifications. Records of git invoked by the daemon on an SSH host (read from the daemon every 2 seconds), progress of fetch, pull, push and clone (picking up `--progress` lines as they stream) and cancellation (stopping the whole process group; on SSH hosts, asking the daemon), and reporting failed operations on the window bar (fatal: and error: lines, details, Activity, close; also when cancelled) are also done
- [x] U13. GitHub integration (accounts, listing and creating PRs, CI status). Left to gh. Detect the GitHub repository from the origin URL; Pull Requests in the sidebar shows open PRs (CI ✓, ✗, ●, draft, review state; clicking the CI mark opens the same checks popover as in History); right-click to check out, open in the browser, copy the URL; selecting one shows that branch's history. Repository > Create Pull Request… (from the current commit's message, base is the default branch) and Open on GitHub, per-commit CI status in History (the last 100 commits of the current branch and the default branch read in one GraphQL query, ✓, ✗ or a clock to the right of the signature mark; clicking the mark opens a popover that lists the commit's checks with their result and duration, each linking to its log, and a button to the checks page; re-runs are collapsed to one row per check, and a check that ended cancelled shows as cancelled rather than failed), GitHub avatars (the author's avatarUrl from the same query, preferred over Gravatar), and Clone from your own repositories (U1) are also done
- [x] U14. AI (translation, explanation and message suggestions through the Claude Code / Codex CLI, conflict resolution, per-purpose prompt settings). Choose the CLI (default: not used), model and language in Settings > AI, and edit per-purpose prompts as files or restore the default. Translation and explanation in History details (the combined diff for a multiple selection), ✨ suggestions in Working Copy ("メッセージに入れる" (Insert into Message)), "AI で解消" (Resolve with AI) in the merge editor (received as JSON, discarded if markers remain; "Result に入れる" (Insert into Result) replaces, ⌘Z reverts) and "AI で解説" (Explain with AI), results shown as they stream and cached. The first-time confirmation per repository, confirmation of repository prompts, explanation from right-clicking a diff hunk (purpose "hunk explanation", in History and Working Copy), and "試す" (Try) for prompts in Settings (sends the current prompt with a sample diff and message) are also done. Codex is not installed on this Mac, so its output parsing has not been verified
- [x] U15. Worktrees in the core and the CLI: listing (`git worktree list --porcelain`), add, remove, prune, over SSH
  too; `amonhen worktree` (`list --changes` adds each worktree's number of changed files).
- [x] U16. Viewing worktrees: the sidebar's Worktrees with branches and change counts, kept up to date as worktrees
  come and go; worktrees grouped under their repository on the repository page; opening a worktree's folder opens its
  repository's tab.
- [x] U17. Editing one worktree: the edited worktree (✎ in the toolbar), commands that touch HEAD, the index or the
  working tree sent there, the worktree named in confirmation sheets, branches checked out elsewhere moving the edited
  worktree, New Worktree… / Remove Worktree / Prune.
- [x] U18. Worktrees under Working Copy in the sidebar with their change counts (first built as every worktree's changes inside Working Copy, then moved to the sidebar).
- [x] U19. History's labels at each worktree's HEAD (name and number of changed files; ✎ for the edited one), kept up
  to date when a worktree commits or changes files.
- [x] U20. Repository settings redone from the person using it (chapter 4). Done: the page in the sidebar's Workspace with its list of pages (Author and Signing, Commits, Operations, Branches, Working Copy, Remotes, Hooks · LFS, git config) and the git line under it, the rows with the value in effect, its source, ↩︎ and ⓘ, the default branch override (`amonhen.defaultBranch`), Pull and Fetch following git's settings, rows that only a terminal honors taken off the page, and Keys and Values, git config as the Xcode-style table (one row per key, a column per source, the local column editable, the Basic / Customized / All and Combined / Levels switches), and signing as one choice (No signing / OpenPGP / SSH / X.509) with the key chosen from those the core collects, and the template library in Settings… (on Git > Commits after U21), chosen on the Commits page (`amonhen.commitTemplate`) next to the repository's own template (`.git/amonhen/commit-template`, pointed to by `commit.template`, edited under the row) and a file, and Add to .gitignore on an untracked file's context menu in Working Copy, and the remotes sheet (name, fetch and push URLs, what the URL points to, a connection check) with the same actions on the sidebar's Remotes, and Hooks · LFS as tables with actions (an Enabled switch per hook, the selected hook's script under the table with Edit in <editor>, Create Hook…; Track Pattern… and Remove for LFS, with the count of files in LFS). This Mac's settings for all repositories are Settings… > Git (U21). What stage 7 still had, the SSH hosts' settings, is U22; Set for All Repositories on a row and editing the all-repositories column from a repository's page were dropped (4.4).
- [x] U21. The Settings window in the shape of chapter 3: resizable, 960 points wide by default, every tab on one screen; the tabs General, Git (the machines), Accounts, Appearance, Diff & Merge, AI and Performance, no Advanced tab (Commit comes when it has rows). The message template library moves from Git to Commit, auto-fetch and bookmarking to General, GitHub to Accounts, the merge editor's auto-resolve to Diff & Merge, the daemon and the shell environment to Performance; the pull default and the user-wide `rebase.updateRefs` rows go (chapter 3). Done, with the Git tab as the Repository Settings screen at the all-repositories level (Author and Signing, Commits with the template library, Operations, Working Copy with Ignore everywhere, Programs, Keys and Values with its all-repositories column editable); the SSH hosts are U22.
- [x] U22. Settings… > SSH Hosts (chapter 3), three columns: the hosts with + / −; the selected host's pages, Connection (state and Check Connection, the engine, signature verification, which moves here from the one switch for every host, and the folders to scan, one setting with Places) and the pages of Settings… > Git for all repositories there (against the host's `--global`); the selected page. Needs the engine to run on a host without a repository (`amonhen --stdio` with no path, serving config, version, programs and keys) and the shared daemon to connect with `ssh://host` alone. The host's choices (git, the engine, sending one) go to the core when it connects, directly or through the shared daemon, which keeps a separate connection per choice.

## Decisions

| Question | Decision |
| --- | --- |
| Working Copy list | A single checkbox list (shows the index state as is; ▣ when partial) |
| Default History diff | All files stacked vertically. Huge commits (over 200 files or so) automatically switch to the single-file view (going ahead with the proposal and revisiting after use) |
| Merge editor layout | Only the three columns Ours, Result, Theirs |
| AI backend | A local CLI. Claude Code or Codex, chosen explicitly in settings (default "none", not split per purpose) |
| Shortcuts | Decided by our own rules (not matching other clients) |
| Refresh | Stays in the toolbar (⌘R) |
