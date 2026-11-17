# TUI design notes

[日本語](ja/tui.md)

A front end that runs in a terminal: `amonhen tui`. It is for the places the apps don't reach (an SSH session, a machine without a desktop, a terminal pane beside an editor) and it keeps the same aim as the apps: nothing on screen waits for git.

Status: stages T1 and T2 are built (History, the selected commit's details and diff, scrolling, `--snapshot`, `--measure-scroll`), and most of T3 (the Working Copy list and diff, staging by file, hunk and line, discard, the message field, commit and amend, change notifications) and most of T4 (the sidebar with Branches, Remotes and Tags and its filter, History search, ancestor emphasis, the key help) and the first operations of T5 (fetch, pull, push, checkout, new branch, undo). The rest is design.

## Overview

- Rust, with ratatui (the cell buffer and diffing of frames; `ratatui-core` and `ratatui-crossterm` only, since every screen is drawn by hand and the widgets crate isn't needed) and crossterm (input, colors, the alternate screen; Unix and Windows).
- Part of the engine: `amonhen tui [<path> | ssh://host/path | daemon:<path>]`. No second executable, so a host that has the engine has the TUI.
- It reaches the core only through the front API (`amonhen-front`, [front.md](front.md)), as the CLI does: in-process for a local path, the shared daemon for `daemon:` and `ssh://`. History uses the same persistent cache as the apps.
- Keyboard first. The mouse is an addition (wheel, click to select, drag the dividers).
- Screens, names and wording follow [ux.md](ux.md). Where a terminal can't do what the apps do, the TUI has its own form, and [frontends.md](frontends.md) gets a TUI column that says so.

Not planned:

- Avatars, window backdrops and themes with images, the GitHub and AI features, and the settings pages (the apps and `amonhen run` change settings; the TUI reads them).
- Drag and drop. Operations the apps offer by dragging (merge, rebase, reorder) are commands on the selection.
- Terminals without Unicode box drawing or with fewer than 256 colors.

## Where it lives

| Piece | Place | Notes |
| --- | --- | --- |
| The TUI | `crates/tui` (`amonhen-tui`, a library) | Takes an `Arc<dyn Front>`; knows nothing about how the repository was opened. Depends on `amonhen-front`, `amonhen-layout`, `amonhen-model`, `amonhen-projection`, ratatui and crossterm (`amonhen-commands` from T4) |
| The entry | `crates/cli`, the `tui` subcommand | Opens the repository as every other subcommand does (`open_repository`) and hands the front to `amonhen_tui::run` |
| The switch | The `tui` feature of `amonhen-cli`, on by default | Cross-target checks that build with `--no-default-features` leave it out, like `syntax` |
| The graph in cells | `amonhen-projection::terminal` | The lane-to-column packing and the box-drawing grid now in `crates/cli/src/graph.rs` move here, so `amonhen log` and the TUI draw the same graph. The CLI keeps turning the grid into ANSI text; the TUI writes it into ratatui's buffer |

The core gets no terminal code, and `amonhen-front` gets no new commands for the first stages (what the TUI needs is already there for the VSCode extension and the CLI).

## Structure

```
 crossterm events ─┐
 front.subscribe ──┼─▶ one channel ─▶ main thread: update the state ─▶ draw if dirty ─▶ terminal
 worker replies ───┘                         │
                                             └─▶ workers: front.call (blocking), cancelled when stale
```

- One main thread owns the state and the terminal. It blocks on one channel and wakes for input, a reply from a worker, a change notification, or a timer (only while something animates or a search reports progress). No frame is drawn when nothing changed.
- `Front::call` blocks, so every call runs on a worker thread and its reply comes back through the channel. Each kind of request (History rows, a diff, status, a search) carries a generation; a reply older than the current one is dropped, and a running search is cancelled through `call_until`.
- The state is plain data (which view, the selection, the scroll position, the caches below). Drawing is a function of the state, as ratatui expects. There is no retained widget tree.
- A frame is written inside a synchronized update (`CSI ? 2026`), so a terminal that supports it shows the frame whole. ratatui sends only the cells that changed.

### History

- `history` gives a numbered History and its row count; the number stays valid while the user scrolls, even if a refresh arrives.
- Rows are fetched by range with `historyRows` (nodes, edges, IDs, times, subject and author). The TUI keeps a window of rows around the visible range (a few screens each way) and asks for the next window before the edge is reached, so a scroll step draws from memory. Rows not there yet (after a jump to the far end) stay blank until the reply arrives; one request is in flight at a time, and the next is decided when it returns.
- One terminal line per commit, always. A row is found from a line by subtraction, and scrolling by a page never has to measure rows. Curves that `amonhen log` puts on a connector line between commits are drawn on a commit's own line: a curve leaving a commit turns on that commit's line, a curve arriving at a parent turns on the parent's line, and a curve straight to the next row turns on the parent's line for a first parent (a branch returning to where it forked) and on the child's for a merge. A line crossing other lanes shows as `┼`.
- The graph column takes what it needs up to a share of the width (a third by default); lanes that only pass through are packed into one thin column, as `amonhen log` does.
- Ref labels come from `refs`, joined to rows by commit ID. The selected commit's ancestors are emphasized with `historyLineage` (dimming the rest), as in the apps. `historyLineage` answers for the whole History (a bit per row: 185 KB on linux), where the apps compute only as far as they draw; one request is in flight at a time, and what this costs over SSH is not measured.
- Search uses the History search syntax with `historySearch`, showing matches as they come (`historySearchPartial`). As built: `/` takes the query on the bottom line, the search runs on its own worker and is cancelled when another starts or Esc clears it, matching rows get a colored subject, and the bottom line shows which match is selected and how many there are. Matches are shown only when the search ends (no partial results yet), and a reload of History searches again.

### Diffs

- `commitDiff` and `workingDiff` return line records and a text table; the TUI keeps the reply and draws the visible lines from it without building a string per line.
- Changed words use the `highlights` section. Syntax colors are asked for after the diff is shown (`diffSyntax`) and painted when they arrive. Positions in both are UTF-16 units within the line, as the window gives them to every front end.
- The details load on their own worker, apart from History's rows, so a large commit's diff doesn't hold up scrolling. One request is in flight; when it returns and the selection has moved on, the reply is dropped and the selected commit is asked for.
- Long lines are cut at the pane's edge and the diff pans sideways (`h` `l`); tabs advance to multiples of 8, as `git diff` shows them in a terminal.
- Colors follow the terminal, not the theme chosen in the apps (that theme is the apps' setting, not the core's, and a terminal already has a palette its user picked). Syntax kinds and the `+` `-` signs use the terminal's 16 colors. The backgrounds of added and removed lines and of changed words are fixed 256-color tints, a dark set or a light set chosen from `COLORFGBG` (dark when the terminal doesn't say).
- Unified view only at first. Side by side needs about 160 columns and comes later.

### Changes

`front.subscribe` delivers the change bits (refs, index, working tree, config). Bits arriving close together are merged, then only what the bits name is reloaded (`status` for index and working tree; `refs` and `history` for refs). The History on screen keeps its number until the new one is ready, then the selection is carried over by commit ID (`historyRowOf`). A refs change reloads History in place and selects the same commit again (`historyRowOf`); the rows are still blank until they arrive. After the TUI's own staging it reloads the status without waiting for the notification.

## Screen

```
 amonhen-standalone  main ↑2 ↓0                                   ⟳ Fetching origin…
┌ Workspace ─┬ History ───────────────────────────────┬ a1b2c3d4 ──────────────────────┐
│ Changes  5 │ ● ─╮  (HEAD → main) Fix the graph  Ann │ Fix the graph                  │
│ History    │ │  ●  Add a test               Bob  2d │ Ann <ann@example.com>  2d ago  │
│ Stashes  2 │ ●─╯  Merge branch 'topic'      Ann  3d │                                │
│ Branches   │ ●    Bump the version          Ann  5d │ M src/graph.rs         +12 -3  │
│  ★ main    │ ┆                                      │ A tests/graph.rs       +40     │
│    topic   │                                        ├────────────────────────────────┤
│ Remotes    │                                        │ @@ -10,7 +10,9 @@ fn layout()   │
│ Tags       │                                        │ -    let lane = 0;             │
│            │                                        │ +    let lane = trunk();       │
└────────────┴────────────────────────────────────────┴────────────────────────────────┘
 Rebase 3/7 · 2 conflicts   c continue  s skip  a abort                    ? help  : commands
```

- Three columns when the terminal is wide enough (about 140 columns and up): the sidebar, the main view, the details. Narrower, the details open in place of the main view (Enter to go in, Esc to come back); narrower still (under about 80), the sidebar opens over the main view on demand. As built: the sidebar (26 columns) shows from 100 columns; the main view and the details sit side by side when 116 columns are left for them (about 142 in all), the main view taking two fifths (60 columns at least). Under 100 columns `0` opens the sidebar in place of the main view.
- The sidebar has the same sections as the apps (Workspace rows that switch the main view, then Branches, Remotes, Tags, Worktrees), with the filter field.
- The top line is the repository, the current branch with ahead and behind, and what is running. The bottom line is the state of a merge, rebase or cherry-pick with its actions, and otherwise hints for the focused pane.
- Working Copy is the apps' single list with a check per file (`[x]` staged, `[-]` partly, `[ ]` not; Space stages or unstages), the diff on the right with staging by hunk and by line (`stageRows`, `discardRows`), and the message field under the list.
- Sheets (confirmations, Branch, Merge, Rebase, Push…) are boxes over the view with the same fields and wording as the apps. Destructive ones ask first and say whether Undo can bring it back.

## Keys

- Moving: arrows and `j` `k`, `g` `G`, PageUp and PageDown, `Ctrl-d` `Ctrl-u`; `Tab` and `Shift-Tab` between panes; `1`…`5` for the Workspace rows.
- In the details: `[` `]` to the previous and next file, `h` `l` (and the left and right arrows) to pan the diff, Esc back to History.
- In the Working Copy diff a line is selected (`j` `k`; `[` `]` to the hunk headers): Space stages or unstages it, `d` discards it; on a hunk header both act on the whole hunk. In the list, `d` discards the file's changes. Discarding asks first on the bottom line (`y` to go on, any other key to cancel) and says that Undo brings the change back; this stands in for the sheets until T5.
- The message field sits under the Working Copy list; its heading shows the length of the summary line. `c` goes into it, Esc or Tab comes back, and while typing every plain key is text. `Ctrl-s` commits (also from the list; Alt-Enter in the field too, where the terminal sends it), since a terminal can't tell Ctrl-Enter from Enter. `a` in the list turns amend on and off; turning it on fills an empty field with the last commit's message. A commit that can't be made says why on the bottom line (no message, nothing staged), as the apps' Commit button does in its tooltip.
- `0` goes to the sidebar (also `h` or the left arrow from a list, and Tab goes round the panes); Enter opens the selected row: a Workspace row switches the main view, a branch, remote branch or tag selects its commit in History. `1` and `2` switch to Changes and History from anywhere. When the sidebar is not showing, the top line lists the main views with their keys and the number of changed files.
- `/` search (History, or the filter in the sidebar), `n` `N` next and previous match.
- `:` opens the command list: every operation in the shared table (`amonhen-commands`) that the TUI has, filtered as you type, with its key beside it. `?` lists the keys of the focused pane.
- Operations have single keys where the apps have toolbar buttons (`f` fetch, `p` pull, `P` push, `b` branch, `m` merge, `r` rebase, `s` stash, `u` undo) and go through a sheet when the apps do.
- The command table's shortcuts are written for windowed apps (`Primary+Shift+B`) and mostly can't be typed in a terminal, so the table gets a column for the TUI's key, as it has one for GTK.
- The kitty keyboard protocol is turned on when the terminal answers for it (for keys a plain terminal can't tell apart); nothing depends on it.

## What a terminal makes different

- Passwords and passphrases: git and ssh run without the terminal (the TUI owns it), so the TUI sets `AMONHEN_ASKPASS` to a small script that runs `amonhen tui --askpass`. That process connects to a socket the running TUI listens on (its path is written into the script, since the daemon passes on `AMONHEN_ASKPASS` but not other `AMONHEN_*` variables), the TUI asks in a sheet, and the answer goes back. This also works when the shared daemon runs git, because the daemon uses the front end's `AMONHEN_ASKPASS`.
- The commit message: a multi-line field in the TUI for the usual case; `Ctrl-e` leaves the alternate screen, runs git's editor (`GIT_EDITOR`, `core.editor`, `VISUAL`, `EDITOR`) on the message and comes back.
- The mouse: capturing it stops the terminal's own text selection (most terminals still select with Shift held). On by default, with a key to turn it off and a setting.
- Conflicts: the TUI lists them, shows each file's conflicts (`conflict`) and offers the per-conflict and per-file choices the apps have, as far as the core gives them as commands (to check in T6); the three-pane editor is not planned. `e` opens the file in the editor.
- The before and after drawings of the sheets (rebase, merge, reset) are drawn with the same graph cells, without animation. Later stage.
- Suspend (`Ctrl-z`) and a resized terminal restore and redraw the screen. A panic restores the terminal before printing.

## Speed

The targets of [design.md](design.md) apply up to the point the TUI controls: the bytes handed to the terminal. What the terminal emulator does after that is outside the TUI's reach and is not measured.

| Item | Target |
| --- | --- |
| From a key event to the frame's bytes flushed, History scrolling by a line | Within 8.3 ms, with room: under 1 ms on a local repository at 200 × 60 |
| The same over `ssh://` and `daemon:` | The same, when the rows are in the window; never blocked on a reply |
| Bytes per scroll step | Measured and kept from growing (it is what an SSH session carries) |
| Launch to the first screen of History | A few ms more than `amonhen log` (same persistent cache) |

- `amonhen tui --measure-scroll` scrolls History through the real event path with the output going to a counter, and prints the time and the bytes per frame (the apps' flag of the same name).
- `amonhen tui --snapshot <file>` draws one frame into ratatui's test backend and writes it as text, with `--size <cols>x<rows>` and, to reach another screen first, `--keys <keys>` (one key per character; a newline is Enter, a tab is Tab, other control characters are Ctrl with the letter). Tests compare such frames; CI needs no terminal.
- criterion benches for building a frame and packing the graph into cells are not written yet.

### Measurement log

T1, a release build on an Apple Silicon Mac, the linux repository (1,485,021 commits), `--measure-scroll` (600 frames, one wheel notch of 3 rows every 8.3 ms):

| Size | Frame time (median / 95% / max) | Bytes per frame (mean / max) | Frames with rows missing |
| --- | --- | --- | --- |
| 200 × 60 | 1.48 ms / 3.49 ms / 12.3 ms | 25,974 / 52,601 | 0 |
| 120 × 40 | 0.81 ms / 2.18 ms / 13.2 ms | 12,828 / 23,813 | 0 |

- Within 8.3 ms except for the slowest frames, but the 1 ms target at 200 × 60 is not met, and the slowest frames (12 to 13 ms) are over. Where the time goes has not been profiled.
- A scroll step repaints almost every cell, because every row moves. Sending the terminal a scroll of the History region and only the rows that come into view would cut both the bytes and the time. ratatui's `Terminal` keeps the previous frame to itself, so this needs our own frame output over `ratatui-core`'s buffers (T1b).
- `--snapshot` of the first screen takes 0.34 s on linux with the cache warm (the whole process, most of it opening the repository), not yet compared with `amonhen log`.

T2, the same Mac, this repository (3,438 commits), with the details beside History at 200 × 60:

| Size | Frame time (median / 95% / max) | Bytes per frame (mean / max) | Frames with rows missing |
| --- | --- | --- | --- |
| 200 × 60 | 1.62 ms / 2.93 ms / 19.3 ms | 5,786 / 8,743 | 0 |
| 100 × 40 | 0.90 ms / 1.38 ms / 17.5 ms | 4,741 / 7,978 | 0 |

- Not comparable with the T1 rows (a different repository, and at 200 × 60 History is now 80 columns wide). The slowest frames are still over 8.3 ms and still unprofiled. Scrolling the diff itself is not measured yet.

## Distribution

- The TUI ships inside the engine for every target the engine is built for. crossterm talks to the Windows console directly, so the Windows engines get it too.
- It adds about 0.36 MB to the engine (40.28 MB to 40.64 MB for the darwin-aarch64 release build, 0.9%), so every engine carries it, the ones copied to SSH hosts included.
- No terminfo and no C library are needed, so the static musl engines stay static.

## Implementation order

- [x] T1. `crates/tui`, the event loop, `amonhen tui`, History (graph, labels, subject, author, date), scrolling with the row window, `amonhen-projection::terminal` shared with `amonhen log`, `--snapshot` and `--measure-scroll`. The speed, the one-line graph and the engine's size measured on the linux repository (see the measurement log)
- [ ] T1b. Our own frame output with terminal scroll regions, so a scroll step sends only the rows that come into view; profile the slowest frames; criterion benches
- [x] T2. Commit details and the diff (file list, unified diff, changed words, syntax colors), the narrow layouts
- [ ] T3. Working Copy: the list, staging by file, hunk and line, discard, the message field and the editor, commit and amend; change notifications. Built so far: the list with a check per file, the diff with a mark on staged lines, staging and unstaging with Space (a file in the list; a line, or a hunk on its header, in the diff), discard with `d` after a confirmation, the message field under the list with commit and amend, reloading on change notifications. Left: opening the message in git's editor (`Ctrl-e`), selecting a range of lines
- [ ] T4. The sidebar (Branches, Remotes, Tags, the filter), History by ref, search, ancestor emphasis, the command list and help. Built so far: the sidebar with the Workspace rows, Branches (the current one marked, ahead and behind), Remotes and Tags; opening a ref selects its commit in History; the selection is kept by commit across reloads; search with `/` and `n` `N`; the sidebar's filter (`/` there); rows outside the selected commit's ancestry dimmed; `?` lists the focused pane's keys. Left: partial search results, lighting the graph's lines along the ancestry (only whole rows are dimmed), the command list (`:`), Worktrees and Stashes in the sidebar
- [ ] T5. Built so far: `f` fetch, `p` pull, `P` push to the upstream (asked first), `u` undo (asked first, naming what is undone), `b` a new branch at the selected commit or ref, `c` in the sidebar to check out a branch (a remote branch gets a tracking branch); one operation runs at a time on its own worker and the top line names it; ahead and behind show beside the branch. Confirmations and the branch name are asked on the bottom line, not in sheets yet. Left: the sheets with the apps' options (pull modes, force push, publishing a branch without an upstream), merge, rebase, stash, reset, askpass (an operation that needs a password or passphrase is untested and may fail or wait), and the rest of this item. Operations with their sheets (fetch, pull, push, branch, checkout, merge, rebase, stash, reset, undo), askpass, what is running; the key and the setting that turn mouse capture off, suspend (`Ctrl-z`), the kitty keyboard protocol
- [ ] T6. Conflicts, Stashes, Reflog, Worktrees, History's edit mode as commands, the before and after drawings
- [ ] T7. The TUI column of frontends.md, usage.md, the CHANGELOG entry

## Decisions

Settled on 2026-11-17:

1. The TUI is part of the engine, `amonhen tui`, not a separate program.
2. `amonhen` with no arguments never starts the TUI; it keeps opening the app.
3. Mouse capture is on by default, with a key and a setting to turn it off.
4. The commit message is a field in the TUI, with git's editor on a key.

Open:

5. How far the first release goes: through T3 (read and commit), or through T5 (the everyday operations).
