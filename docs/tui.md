# TUI design notes

[日本語](ja/tui.md)

A front end that runs in a terminal: `amonhen tui`. It is for the places the apps don't reach (an SSH session, a machine without a desktop, a terminal pane beside an editor) and it keeps the same aim as the apps: nothing on screen waits for git.

Status: stages T1 and T2 are built (History, the selected commit's details and diff, scrolling, `--snapshot`, `--measure-scroll`), and T3 (the Working Copy list and diff, staging by file, hunk and line, discard, the message field, commit and amend, change notifications) and T4 (the sidebar with Branches, Remotes and Tags and its filter, History search, ancestor emphasis, the key help and the command list) and the everyday operations of T5 (fetch, pull, push, checkout, new branch, merge, rebase, stash, undo, and continuing or aborting what stopped) and part of T6 (resolving conflicts by side, Stashes). The rest is design.

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
- A frame is written inside a synchronized update (`CSI ? 2026`), so a terminal that supports it shows the frame whole. Only the cells that changed are sent.
- The frame output is our own (`canvas`), over `ratatui-core`'s cell buffers and ratatui's crossterm backend, in place of ratatui's `Terminal`, which clears, rebuilds and compares every cell on every frame. An event marks what it changed: everything, or one pane (the sidebar, the main view, or the details and diff) when it can only have changed that pane, which is the wheel over a pane, moving the selection in History, scrolling the details, moving the selected line in the Working Copy diff or panning it, and the replies that fill History rows, ancestors, a commit and syntax colors. Only the marked panes are cleared, rebuilt and compared; the top and bottom lines are redrawn with any of them, and the key help, which lies over the panes, redraws everything. Anything not listed marks everything, so a pane is never left stale by an event nobody classified. A pane that scrolled is scrolled in the terminal with a scrolling region, so only the lines that came into view are sent: always when it has the full width, and beside other panes when the terminal has left and right margins.

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
- Long lines are cut at the pane's edge and the diff pans sideways (`h` `l`; at the left edge `h` goes back to the list); tabs advance to multiples of 8, as `git diff` shows them in a terminal.
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

- Three columns when the terminal is wide enough (about 140 columns and up): the sidebar, the main view, the details. Narrower, the details open in place of the main view (Enter or Space in History to go in, Esc to come back); narrower still (under about 80), the sidebar opens over the main view on demand. As built: the sidebar (26 columns) shows from 100 columns; the main view and the details sit side by side when 116 columns are left for them (about 142 in all), the main view taking two fifths (60 columns at least). Under 100 columns `0` opens the sidebar in place of the main view.
- The sidebar has the same sections as the apps (Workspace rows that switch the main view, then Branches, Remotes, Tags, Worktrees), with the filter field.
- The top line is the repository, the current branch with ahead and behind, and what is running. The bottom line is the state of a merge, rebase or cherry-pick with its actions, and otherwise hints for the focused pane.
- Working Copy is the apps' single list with a check per file (`[x]` staged, `[-]` partly, `[ ]` not; Space stages or unstages), the diff on the right with staging by hunk and by line (`stageRows`, `discardRows`), and the message field under the list.
- Sheets (confirmations, Branch, Merge, Rebase, Push…) are boxes over the view with the same fields and wording as the apps. Destructive ones ask first and say whether Undo can bring it back.

## Keys

- Moving: arrows and `j` `k`, `g` `G`, PageUp and PageDown, `Ctrl-d` `Ctrl-u`; `Tab` and `Shift-Tab` between panes; `1`…`5` for the Workspace rows.
- In the details: `[` `]` to the previous and next file, `h` `l` (and the left and right arrows) to pan the diff, Esc back to History.
- In the Working Copy diff a line is selected (`j` `k`; `[` `]` to the hunk headers): Space stages or unstages it, `d` discards it; on a hunk header both act on the whole hunk. In the list, `d` discards the file's changes, and `+` stages every file with unstaged changes except the conflicted ones (the apps' Stage All). Discarding asks first on the bottom line (`y` to go on, any other key to cancel) and says that Undo brings the change back; this stands in for the sheets until T5.
- The message field sits under the Working Copy list; its heading shows the length of the summary line. `c` goes into it, Esc or Tab comes back, and while typing every plain key is text. `Ctrl-s` commits (also from the list; Alt-Enter in the field too, where the terminal sends it), since a terminal can't tell Ctrl-Enter from Enter. `a` in the list turns amend on and off; turning it on fills an empty field with the last commit's message. A commit that can't be made says why on the bottom line (no message, nothing staged), as the apps' Commit button does in its tooltip.
- `0` goes to the sidebar (also `h` or the left arrow from a list, and Tab goes round the panes); Enter or Space opens the selected row: a Workspace row switches the main view, a branch, remote branch or tag selects its commit in History. `1` and `2` switch to Changes and History from anywhere. When the sidebar is not showing, the top line lists the main views with their keys and the number of changed files.
- In History, `@` goes to HEAD, `]` to the selected commit's parent (the first parent) and `[` to its nearest child, as the apps' Go to HEAD, Go to Parent and Go to Child; `]` moves down and `[` up, as in the details. `o` switches History between date order and topological order (a branch's commits kept together under its merge), for this run of the TUI only: the apps keep the order in their settings, which the TUI doesn't write. `z` folds merged branches (the apps' Fold Merged Branches: the trunk followed through first parents, with the number of commits a merge brought in before its subject), and `Z` on such a merge opens or closes the side it brought in; this too lasts for the run of the TUI.
- `/` search (History, or the filter in the sidebar), `n` `N` next and previous match.
- `:` opens the command list: every operation in the shared table (`amonhen-commands`) that the TUI has, filtered as you type, with its key beside it. `?` lists the keys of the focused pane.
- Operations have single keys where the apps have toolbar buttons (`f` fetch, `p` pull, `P` push, `b` branch, `m` merge, `r` rebase, `s` stash, `u` undo) and go through a sheet when the apps do.
- The command table's shortcuts are written for windowed apps (`Primary+Shift+B`) and mostly can't be typed in a terminal, so the table gets a column for the TUI's key, as it has one for GTK.
- The kitty keyboard protocol is turned on when the terminal answers for it (for keys a plain terminal can't tell apart); nothing depends on it.

## What a terminal makes different

- Passwords and passphrases: git and ssh run without the terminal (the TUI owns it), so the TUI sets `AMONHEN_ASKPASS` to a small script that runs `amonhen tui --askpass`. That process connects to a socket the running TUI listens on (its path is written into the script, since the daemon passes on `AMONHEN_ASKPASS` but not other `AMONHEN_*` variables), the TUI asks in a sheet, and the answer goes back. This also works when the shared daemon runs git, because the daemon uses the front end's `AMONHEN_ASKPASS`.
- The commit message: a multi-line field in the TUI for the usual case; `Ctrl-e` leaves the alternate screen, runs git's editor (`GIT_EDITOR`, `core.editor`, `VISUAL`, `EDITOR`) on the message and comes back.
- The mouse: capturing it stops the terminal's own text selection (most terminals still select with Shift held). On by default, with a key to turn it off and a setting.
- Conflicts: the TUI lists them, shows each file's conflicts (`conflict`) and offers the per-conflict and per-file choices the apps have (the core gives them as `resolveConflict` and `takeSide`); the three-pane editor is not planned. `e` opens the file in the editor.
- The before and after drawings of the sheets (rebase, merge, reset) are drawn with the same graph cells, without animation. Later stage.
- Suspend (`Ctrl-z`) and a resized terminal restore and redraw the screen. `Ctrl-l` clears the terminal and writes the whole screen again, without reloading anything, for when something else wrote over it. A panic restores the terminal before printing.

## Speed

The targets of [design.md](design.md) apply up to the point the TUI controls: the bytes handed to the terminal. What the terminal emulator does after that is outside the TUI's reach and is not measured.

| Item | Target |
| --- | --- |
| From a key event to the frame's bytes flushed, History scrolling by a line | Within 8.3 ms, with room: under 1 ms on a local repository at 200 × 60 |
| The same over `ssh://` and `daemon:` | The same, when the rows are in the window; never blocked on a reply |
| Bytes per scroll step | Measured and kept from growing (it is what an SSH session carries) |
| Launch to the first screen of History | A few ms more than `amonhen log` (same persistent cache) |

- `amonhen tui --measure-scroll` scrolls History through the real event path with the output going to a counter, and prints the time and the bytes per frame (the apps' flag of the same name). `--measure-move` does the same pressing `j` (and `k` at the end) to move the selection, `--measure-diff` going into the selected commit's details and scrolling the diff with `j`, `--measure-changes` going into the selected file's diff in the Working Copy and moving the selected line, and `--measure-diff-pan` and `--measure-changes-pan` panning those two diffs sideways with `l`; `--keys` are pressed first, to select the commit.
- `amonhen tui --snapshot <file>` draws through the same frame output as the real screen (a frame after each key, redrawing only what that key marked) and writes the last frame as text. It fails if that screen differs from one drawn whole, so every screen test also checks that no pane was left stale. It takes `--size <cols>x<rows>` and, to reach another screen first, `--keys <keys>` (one key per character; a newline is Enter, a tab is Tab, other control characters are Ctrl with the letter). Tests compare such frames; CI needs no terminal.
- `--measure-scroll` also checks its last screen against one drawn whole, and prints how much CPU time the slowest frame used.
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

Where the time goes (the same Mac, this repository, 200 × 60 with the sidebar, History and the details; `--measure-scroll` now prints the parts):

- Building the cells takes a median of 1.1 to 1.2 ms and diffing and writing the frame 0.5 ms; handling the wheel event is a few µs. Of the build, History's 58 rows take about 0.6 ms (10 µs a row), the details 0.4 ms and the sidebar 0.16 ms, though only History changes on a scroll step. Packing the graph into columns is 6 to 8 µs of it.
- Keeping the sidebar's and the details' cells from the last frame and copying them when nothing in them changed was tried: the build fell to about 0.8 ms and the whole frame from 1.6 to 1.5 ms. That is too little for a second copy of the state to keep in step, so it was not kept.
- Within a History row, writing the labels, subject, author and date is nearly all of the time (about 0.45 ms a frame); building the graph line and its cells is under 0.05 ms. Writing ASCII strings straight into the cells instead of through `set_stringn` was tried and changed nothing measurable (1.5 to 1.8 ms a frame, the same spread as before), so it was not kept either. The measurement sleeps 8 ms between frames, so each frame starts with cold caches, as real scrolling does.
- The slowest frames (8 to 24 ms) are not one part: in some runs the time is in the build, in others in the diff and write, with no worker reply handled in that frame. That looks like the thread being descheduled, but it is not confirmed.
- So the 1 ms target still needs T1b: not rebuilding and not diffing what did not move.

T1b, the same Mac, this repository (3,541 commits), `--measure-scroll` with the build before and the build after run in turn, five times each (the machine was not idle, and one build's medians spread by 20% from run to run, so only a difference seen in every pair counts):

| Size | Before (median of a run) | After | Build / diff and write, before | After |
| --- | --- | --- | --- | --- |
| 200 × 60 (sidebar, History, details) | 1.45 to 1.74 ms | 0.85 to 1.00 ms | 0.93 to 1.08 / 0.43 to 0.57 ms | 0.56 to 0.63 / 0.30 to 0.35 ms |
| 120 × 40 (sidebar, History) | 0.61 to 0.82 ms | 0.43 to 0.63 ms | 0.40 to 0.54 / 0.19 to 0.27 ms | 0.29 to 0.40 / 0.15 to 0.22 ms |
| 80 × 60 (History alone) | 0.93 to 1.46 ms | 0.93 to 1.39 ms | 0.63 to 0.93 / 0.29 to 0.51 ms | 0.63 to 0.89 / 0.31 to 0.46 ms |

- With the panes that did not move left alone, a scroll step at 200 × 60 costs what History alone costs: about 0.9 ms a frame in these runs, and 0.70 to 0.80 ms in runs taken while the machine was quieter (1.29 ms before, then). That is under the 1 ms target, but without room; the 95% frame is 1.7 to 1.9 ms (2.8 to 3.2 ms before).
- Where History has the whole width nothing was skipped before either, and the time is the same. What is left is History itself: 58 rows rebuilt and compared on every step, since every row moves.
- The bytes per step are the same (about 5,000 at 200 × 60), since they were already only the cells that changed.
- The slowest frames are the thread being descheduled: a frame that took 6 to 33 ms used 1.0 to 1.2 ms of CPU time (`CLOCK_THREAD_CPUTIME_ID`), in three runs out of three. Nothing in the TUI is slow in those frames.
- Terminal scroll regions were not built at this point (they are now, for a pane that has the full width: see the last table of this log). A region scrolls whole lines (`DECSTBM`), so it fits only when History has the full width; beside the sidebar or the details it needs left and right margins (`DECSLRM`), which many terminals lack. It would cut the bytes of a step (what an SSH session carries), not the time: the graph's columns are packed from the visible rows, so the rows that stay can change and are rebuilt and compared anyway.
- Checked in a real terminal emulator: the TUI run in tmux at 200 × 60, with keys that move the History selection and scroll the details, ends on the same screen as `--snapshot` with the same keys.

History's rows kept from the last frame, the same Mac and repository, the build before and the build after run in turn, five times each (the machine not idle, as above). `--measure-move` presses `j` where `--measure-scroll` turns the wheel:

| Size | Scroll, before (median of a run) | After | Move, before | After |
| --- | --- | --- | --- | --- |
| 200 × 60 (sidebar, History, details) | 0.65 to 0.99 ms | 0.30 to 0.56 ms | 0.56 to 0.59 ms | 0.40 to 0.45 ms |
| 80 × 60 (History alone) | 0.32 to 0.36 ms | 0.16 to 0.35 ms | 0.32 to 0.45 ms | 0.16 to 0.25 ms |
| 120 × 40 (sidebar, History) | 0.29 to 0.84 ms | 0.24 to 0.60 ms | 0.52 to 0.67 ms | 0.34 to 0.61 ms |

- A scroll step rebuilt every visible row, though all but the rows coming into view only moved. History keeps the cells to the right of the graph (labels, subject, author, date) of the rows it built, for twice the visible rows, and copies them while the width, the graph's width and the content stay the same. The graph, the selection and the ancestor dimming are drawn every frame, as they change from frame to frame. The check against a full redraw (`--snapshot`, the measurements) rebuilds every row, so a stale row fails it.
- Building a frame fell in every pair (at 200 × 60 from 0.44 to 0.62 ms to 0.11 to 0.19 ms a scroll step). Diffing and writing did not, and at 120 × 40 it rose (0.10 to 0.31 ms to 0.14 to 0.40 ms), so part of what the build no longer pays shows up there; the whole frame was still faster in 29 of the 30 pairs and 3% slower in the other (at 120 × 40, scrolling).
- What is left of a scroll step is mostly comparing the cells and writing them (0.2 to 0.3 ms at 200 × 60), and the bytes are the same.
- Moving the selection with `j` costs about what a scroll step does. Redrawing only the two rows the selection touches was not built: the ancestor dimming arrives a moment later and changes rows all over the pane.
- The numbers of this table are lower than the T1b table's for the same build because the machine was quieter; only the pairs compare.

Scrolling a commit's diff a line at a time (`--measure-diff`, which goes into the details of the commit that `--keys` selected and presses `j`, `k` at the end), the same Mac and repository, a commit whose details are 943 lines, five runs each:

| Size | Frame time (median of a run) | 95% | Build / diff and write | Bytes per frame (mean / max) |
| --- | --- | --- | --- | --- |
| 200 × 60 (the details beside the sidebar and History) | 0.44 to 0.74 ms | 1.6 to 2.0 ms | 0.26 to 0.38 / 0.18 to 0.36 ms | 9,997 / 12,226 |
| 80 × 60 (the details alone) | 0.40 to 0.57 ms | 1.3 to 1.5 ms | 0.25 to 0.34 / 0.15 to 0.23 ms | 8,880 / 10,354 |
| 120 × 40 (the details alone, beside the sidebar) | 0.22 to 0.40 ms | 0.6 to 1.5 ms | 0.13 to 0.22 / 0.09 to 0.18 ms | 6,186 / 7,784 |

- Under the 1 ms target in the median at every size, with only the details pane rebuilt and compared. The slowest frames (2 to 36 ms) used 0.2 to 1.5 ms of CPU time, so they are the thread descheduled again.
- A step sends about twice the bytes of a History step (10,000 against 5,000 at 200 × 60): every line of the diff moves and its colors go with it. This is where a scroll region saves the most, for an SSH session.

Moving through the Working Copy diff (`--measure-changes`, which goes into the selected file's diff and presses `j`, `k` at the end; the diff scrolls once the selected line leaves the pane, which is all but the first screenful of steps), the same Mac, a copy of this repository with one file changed (a diff of 2,804 lines), the build before and the build after run in turn, five times each:

| Size | Before (median of a run) | After | Bytes per frame (mean / max) |
| --- | --- | --- | --- |
| 200 × 60 (sidebar, the list, the diff) | 0.66 to 1.52 ms | 0.84 to 1.18 ms | 10,343 / 15,016 |
| 80 × 60 (the diff alone) | 0.74 to 1.02 ms | 0.72 to 0.98 ms | 9,101 / 12,920 |
| 120 × 40 (sidebar, the diff) | 0.80 to 1.01 ms | 0.61 to 0.72 ms | 6,601 / 9,223 |

- Before, a move of the selected line redrew the whole screen; after, the moving keys (`j` `k`, the page keys, `g` `G`, `[` `]`) mark only the diff pane. That was faster in 4 of 5 pairs at 200 × 60 and in all 5 at 120 × 40; at 80 × 60 the diff is the whole screen and nothing changed.
- At 200 × 60 this is about the 1 ms target, not under it, and slower than a commit's diff: the Working Copy diff has the pane to itself beside the list (more cells), and every line carries two line numbers and the staging mark. Building it takes 0.41 to 0.60 ms and diffing and writing 0.40 to 0.56 ms. The 95% frame is 2.1 to 2.4 ms.
- What would cut it is what History got: keeping the lines that only moved.

The diff's lines kept from the last frame, in the Working Copy diff and in a commit's details (the same Mac, repository and diffs as the two tables above), the build before and the build after run in turn, five times each:

| Size | Working Copy diff, before (median of a run) | After | Commit's diff, before | After |
| --- | --- | --- | --- | --- |
| 200 × 60 | 0.76 to 0.87 ms | 0.45 to 0.62 ms | 0.73 to 1.08 ms | 0.44 to 0.66 ms |
| 80 × 60 | 0.63 to 0.75 ms | 0.37 to 0.50 ms | 0.66 to 1.09 ms | 0.37 to 0.63 ms |
| 120 × 40 | 0.63 to 0.72 ms | 0.36 to 0.43 ms | 0.59 to 0.74 ms | 0.32 to 0.47 ms |

- Both panes keep the cells of the diff lines they built, for twice the visible lines, and copy them while the diff (its content and syntax colors) and the sideways position stay the same. The mark of the selected line is drawn every frame. Faster in all 30 pairs.
- Building a frame of the Working Copy diff at 200 × 60 fell from 0.35 to 0.40 ms to 0.07 to 0.08 ms. Diffing and writing stayed at 0.38 to 0.50 ms, and is now most of the frame, as in History; the bytes are the same.
- The 95% frame of the Working Copy diff at 200 × 60 is 0.9 to 2.4 ms (1.8 to 3.6 ms before).

Panning a diff sideways, 8 cells a step (`--measure-diff-pan` and `--measure-changes-pan`, which press `l` and turn back with `h` after 20 steps), the same Mac, diffs of this document, whose lines run to 3,000 characters (a commit's, 119 lines, and an uncommitted one of 355 lines in a copy of the repository), five runs each:

| Size | Commit's diff, machine busy (median of a run) | Machine quieter | Bytes per frame (mean) |
| --- | --- | --- | --- |
| 200 × 60 | 0.97 to 1.17 ms | 0.33 to 0.53 ms | 8,477 |
| 80 × 60 | 0.32 to 0.88 ms | not run | 6,282 |
| 120 × 40 | 0.42 to 0.54 ms | 0.20 to 0.37 ms | 4,211 |

- A pan rebuilds every line, since the kept lines are for one sideways position. Building is about three fifths of the frame (0.20 to 0.31 ms of 0.33 to 0.53 ms at 200 × 60).
- The machine's load moves these numbers by a factor of three: the same build, minutes apart, took 0.97 to 1.17 ms and then 0.33 to 0.53 ms a step at 200 × 60. So the first column is at the 1 ms target and the second well under it, and neither is the number; only runs taken in turn compare.
- In the Working Copy diff a pan redrew the whole screen. `l` and `h` now mark only the diff pane. The build before and the build after run in turn, five times each: at 200 × 60 0.45 to 1.04 ms before and 0.33 to 0.72 ms after, faster in all 5 pairs; at 120 × 40 0.24 to 0.30 ms and 0.20 to 0.27 ms, faster in 4 of 5. Before that change, with the machine busy, it was 1.17 to 1.49 ms at 200 × 60, 0.69 to 0.93 ms at 80 × 60 and 0.52 to 0.65 ms at 120 × 40.

Terminal scroll regions, the same Mac, repository and diffs, the build before and the build after run in turn, five times each. Mean bytes a step, and the median frame time of a run:

| Size and step | Bytes, before | After | Time, before | After |
| --- | --- | --- | --- | --- |
| 80 × 60, History, wheel | 6,180 | 1,261 | 0.16 to 0.27 ms | 0.14 to 0.21 ms |
| 80 × 60, History, `j` | 5,695 | 1,122 | 0.15 to 0.35 ms | 0.14 to 0.52 ms |
| 80 × 60, a commit's diff | 8,880 | 248 | 0.26 to 0.51 ms | 0.28 to 0.41 ms |
| 80 × 60, the Working Copy diff | 9,093 | 242 | 0.13 to 0.29 ms | 0.10 to 0.30 ms |
| 80 × 24, History, wheel | 2,425 | 1,052 | 0.08 to 0.21 ms | 0.07 to 0.09 ms |
| 80 × 24, a commit's diff | 3,342 | 247 | 0.06 to 0.09 ms | 0.05 to 0.07 ms |
| 80 × 24, the Working Copy diff | 3,678 | 254 | 0.07 ms | 0.05 to 0.06 ms |

- When a pane that has the full width of the terminal scrolled, the frame output sets the scrolling region to its lines (`DECSTBM`), scrolls it (`SU`, `SD`) and resets the region, does the same to its own copy of the screen, and then writes the cells that still differ: the lines that came into view, and whatever else changed. A pane says how far it moved since it was last drawn; if that is wrong, the cells differ and are written, so the screen is right either way.
- A step of a diff falls to about 250 bytes, whatever the height: one new line and the position at the bottom. History falls less (to about 1,100 to 1,300 bytes), because the graph's columns are packed from the visible rows and the selection and the ancestor dimming move, so rows that stay still change.
- The time is a little lower where the bytes fell (the History wheel step in all 5 pairs at 80 × 60, the diffs at 80 × 24 in all 5); it was never what a region was for.
- Only a pane with the full width qualifies: under 100 columns, where there is no sidebar. At 200 × 60 the bytes and the time are the same as before (0.31 to 0.37 ms against 0.32 to 0.39 ms for `j` in History, six pairs). Scrolling a pane that has other panes beside it needs left and right margins (`DECSLRM`), which many terminals lack; see below.
- Checked against a small terminal model in the tests (the bytes written, replayed, give the same screen as the frame output's copy, through scrolls up and down, a scroll of more than the height, and a wrong count), and in tmux at 80 × 30: scrolling History and a commit's diff ends on the same screens as `--snapshot` with the same keys. Not checked in other terminal emulators.

Left and right margins, for a pane with others beside it:

- When it takes the terminal, the TUI asks whether the terminal knows mode 69 (`DECLRMM`, by `DECRQM`) and then sends the question every terminal answers (`DA1`); the mode's answer has to come before that one. It waits 300 ms at most, on Unix only (elsewhere it does not ask, and only full-width panes scroll in the terminal).
- If the terminal knows the mode, a pane that scrolled is scrolled there too: the mode on, the region's lines (`DECSTBM`) and its left and right margins (`DECSLRM`), the scroll, and everything reset, with the mode on only for that long because it changes what `CSI s` means.
- Checked against the terminal model in the tests (the pane's cells move, the cells beside it do not, the margins and the mode end reset), and in tmux, which answers that it does not know the mode: the question at startup leaves nothing on the screen or in the input, and nothing is scrolled with margins. **Not checked in any terminal that has margins**, and not measured: none was at hand. `Ctrl-l` redraws if a terminal gets it wrong.

Lighting the graph's lines along the ancestry, the same Mac, this repository (3,748 commits), 200 × 60, the build before and the build after run in turn, five times each (median and 95% frame time, the range over the five runs):

| | Before | After |
| --- | --- | --- |
| `--measure-scroll` | 467 to 657 µs / 1,272 to 1,461 µs | 431 to 686 µs / 1,352 to 1,613 µs |
| `--measure-move` | 399 to 428 µs / 724 to 984 µs | 388 to 428 µs / 680 to 952 µs |

The runs overlap, so no difference shows at this size of repository. The TUI now also keeps the engine's number for every line of History (4 bytes each), copied on the loader's thread for each selected commit; that was not measured in a large repository.

## Distribution

- The TUI ships inside the engine for every target the engine is built for. crossterm talks to the Windows console directly, so the Windows engines get it too.
- It adds about 0.77 MB to the engine (40.28 MB to 41.05 MB for the darwin-aarch64 release build, 1.9%; 0.36 MB when only History was built), so every engine carries it, the ones copied to SSH hosts included.
- No terminfo and no C library are needed, so the static musl engines stay static.

## Implementation order

- [x] T1. `crates/tui`, the event loop, `amonhen tui`, History (graph, labels, subject, author, date), scrolling with the row window, `amonhen-projection::terminal` shared with `amonhen log`, `--snapshot` and `--measure-scroll`. The speed, the one-line graph and the engine's size measured on the linux repository (see the measurement log)
- [ ] T1b. Our own frame output with terminal scroll regions, so a scroll step sends only the rows that come into view; profile the slowest frames; criterion benches. Built so far: our own frame output that rebuilds and compares only the panes an event changed (a scroll step at 200 × 60 from about 1.5 ms to about 0.9 ms); the slowest frames found to be the thread descheduled. History keeps the rows it built from frame to frame (a scroll step at 200 × 60 to about 0.3 to 0.5 ms), and so do the Working Copy diff and a commit's diff (about 0.45 to 0.65 ms). Where a pane has the full width, a scroll step scrolls a terminal region and sends only what came into view (a diff step from about 9,000 bytes to about 250). With left and right margins (`DECSLRM`, asked of the terminal at startup) the same goes for a pane with others beside it, which is built but not yet run in a terminal that has them. Left: running that in such a terminal and measuring it, criterion benches
- [x] T2. Commit details and the diff (file list, unified diff, changed words, syntax colors), the narrow layouts. A signed commit's details show the signature checked (`verifySignatures`) in the apps' words (Verified, Untrusted key, Unverified signer, Expired, Revoked key, Bad signature, No key, Can't verify) and the signer
- [x] T3. Working Copy: the list, staging by file, hunk and line, discard, the message field and the editor, commit and amend; change notifications. Built so far: the list with a check per file, the diff with a mark on staged lines, staging and unstaging with Space (a file in the list; a line, or a hunk on its header, in the diff), discard with `d` after a confirmation, the message field under the list with commit and amend, reloading on change notifications; `v` in the diff starts a range of lines that Space and `d` then act on; `Ctrl-e` in the message field opens the message in git's editor (`git var GIT_EDITOR`) and takes it back when the editor closes, dropping `#` lines. While the editor has the terminal, the input thread stops reading (it polls for input 50 ms at a time so that it can)
- [x] T4. The sidebar (Branches, Remotes, Tags, the filter), History by ref, search, ancestor emphasis, the command list and help. Built so far: the sidebar with the Workspace rows, Branches (the current one marked, ahead and behind), Remotes and Tags; opening a ref selects its commit in History; the selection is kept by commit across reloads; search with `/` and `n` `N`; the sidebar's filter (`/` there); rows outside the selected commit's ancestry dimmed, and in the graph only the lines leading to its ancestors and the ancestors' dots lit (each line from the row the engine gives in `historyLineage`'s `edgeFrom`, matched through `historyRows`' `edgeIds`); `?` lists the focused pane's keys; `:` lists the commands by the names the apps' menus use (from `amonhen-commands`), those that work where the focus is, narrowed by the words typed, each with its key, and Enter runs the selected one as if its key were pressed (Ctrl-n and Ctrl-p or the arrows move, Esc closes). While a search runs, History shows the matches found so far (asked of the engine every 200 ms with `historySearchPartial`) and the bottom line their number, with the share searched for a contents search; the first of them moves the selection, later ones do not.
- [ ] T5. Built so far: `f` fetch, `p` pull, `P` push to the upstream (asked first), `u` undo (asked first, naming what is undone), `b` a new branch at the selected commit or ref, `c` in the sidebar to check out a branch (a remote branch gets a tracking branch), `m` and `r` there to merge the selected ref into the current branch or rebase the current branch onto it (asked first; in place of the apps' before and after drawings the question says what the engine foresees: a fast-forward and by how many commits, or the merge commit's subject and the commits on each side, the files likely to conflict, branches that move along, and whether pushing will need Force Push), `s` to stash and `S` to pop the newest stash; while a merge, rebase, cherry-pick or revert is in progress the bottom line shows it with its step and the number of conflicted files, and `C` continues, `K` skips and `A` aborts (asked first); an operation that stops at conflicts switches to the Working Copy; one operation runs at a time on its own worker and the top line names it; ahead and behind show beside the branch. Confirmations and the branch name are asked on the bottom line, not in sheets yet. `P` on a branch without an upstream publishes it (to the only remote, or one chosen by number) and sets the upstream; when the upstream has commits the branch lacks, `P` also offers a force push with lease and `p` offers merge or rebase beside git's setting, after saying which files are likely to conflict. While `P` asks, `t` takes a destination typed as `<remote>/<branch>` (starting with the upstream) and pushes there, leaving the upstream as it is. In the sidebar `n` renames a branch and `D` deletes a branch or a tag after asking (a branch git calls unmerged is refused, with git's reason; there is no forced delete); `T` makes a tag at the selected commit or ref, and `U` on a remote branch makes it the current branch's upstream after asking (the apps' Set Upstream). In History `y` cherry-picks and `v` reverts the selected commit and `c` checks it out detached, each after asking; a cherry-pick or revert that stops at conflicts goes on with `C` `K` `A` like a merge. The key list scrolls with `j` `k` when it doesn't fit. `s` asks for a message (Enter alone for none) and then what to stash (tracked files, with untracked files, or keeping what is staged); `Ctrl-p` in the Working Copy commits and then asks to push. Left: the sheets themselves with the rest of the apps' options (fast-forward only), the kitty keyboard protocol and a setting for mouse capture. Also built: `R` in History or Reflog resets the current branch to the selected commit after a choice of soft, mixed or hard on the bottom line (the question first says what the engine foresees: how many commits it leaves and brings, and whether pushing will need Force Push); `M` turns mouse capture off and on (off, the terminal selects text); `Ctrl-z` gives the terminal back and stops the process, and takes the terminal again when it continues (the terminal hand-over was checked in a pseudo-terminal, where the stop itself does not happen without a job-control shell, so stopping and `fg` are not verified). Askpass is built for Unix as described under "What a terminal makes different": the socket and the script live in a private temporary folder removed at exit, the question shows on the bottom line (typed characters hidden for passwords and passphrases, Enter answers, Esc cancels), and several questions queue. It was checked by calling the script by hand against a running TUI; a real fetch or push that asks, the shared daemon and Windows are not tested. Operations with their sheets (fetch, pull, push, branch, checkout, merge, rebase, stash, reset, undo), askpass, what is running; the key and the setting that turn mouse capture off, suspend (`Ctrl-z`), the kitty keyboard protocol
- [ ] T6. Conflicts, Stashes, Reflog, Worktrees, History's edit mode as commands, the before and after drawings. Built so far: a conflicted file shows its content with the markers (`conflict`), the current and incoming sides tinted; in that diff `o` `t` `b` resolve the conflict under the selected line as current, incoming or both, and `a` as the base where the conflict has that side (git writes it with `merge.conflictStyle` `diff3` or `zdiff3`; without it the key says so), and `m` on a conflicted file (in the list or its diff) runs Magic Merge on the whole file, resolving what its rules can and leaving the rest with their markers (`magicMergeFile`, which writes the file; undone with `u`) (`resolveConflict`), and in the list `o` `t` take one side of the whole file after asking (`takeSide`); Stashes in the sidebar by message, with `a` apply, `o` pop and `x` drop (asked first); Reflog as a third main view (`3`, and a Workspace row): HEAD's entries newest first, those whose commit is not in History dimmed, the selected entry's commit in the details pane, `b` for a new branch there; `L` on a branch in the sidebar shows that branch's reflog in the same view (the bottom line names it), and `3` goes back to HEAD's. Enter on a stash shows its diff in the details pane until History is moved; Worktrees list in the sidebar when there is more than one (folder and branch, the current one marked; Enter only says where it is, and `x` removes the worktree after asking: git refuses one with changes left, and the one that is open is not offered; `w` on a local branch asks for a folder, starting with one beside the working tree, and adds a worktree with that branch checked out; Prune Worktrees in the command list clears the records of worktrees whose folders are gone; it is rarely needed, so it has no key, and so have New Orphan Branch, which asks for a name and then whether to start empty or keep the current files staged, and Clean Up Branches, which names the branches merged into the default branch and deletes them after asking, with `a` to take in those whose upstream is gone as well (the apps leave those unchecked; branches merged by a pull request are not looked for, since the TUI has no GitHub), and Add Remote, which asks for the name, starting with origin when there is no remote yet, and then the URL, and Update Submodules, which takes every submodule to its recorded commit, recursively, or to its branch's latest, as chosen, and Delete Ignored Files, which counts and names what matches .gitignore and deletes it after asking, saying that Undo can't bring it back, and Export Archive, which writes the files of History's selected commit to a file on this machine, starting with one in the folder the TUI was started from; the extension (.zip, .tar.gz, .tar) picks the format and the files go in a folder named after the file, as in the apps, and Save Repository as Bundle, which writes HEAD and every branch and tag to one file on this machine that git can clone or fetch from, and Fetch from Bundle, which reads a bundle file on this machine, refuses one that needs commits the repository lacks, and fetches its branches under a remote name it asks for, starting with the file's name, and Apply Patch, which reads a patch file on this machine, shows git's reason when it doesn't apply, and otherwise applies a mail-style patch as commits (git am, stopping at conflicts like a merge) or a plain diff to the working tree (git apply), after asking, Save as Patch, which writes History's selected commit as a patch under git's name into a folder on this machine, and Compare with Previous Version, which finds the current branch's version before it was last rewritten (in its reflog) and lists the pairs `git range-diff` gives, one line each as git writes them (`=` same, `!` changed, `<` removed, `>` added), in the Blame and File History view, the selected pair's commit in the details pane, and under a changed pair its diff of diffs, indented as git prints it, lines only in the previous version in red and only in the current one in green). On the bottom line `Ctrl-u` clears what is typed, as in a shell. `e` in the Working Copy list opens the selected file in git's editor (the terminal is handed over as for the message; not over `ssh://`, where the files are on another machine). Blame and File History open from the Working Copy list (`B`, `L`) for the selected file: Blame shows each line with the commit that last changed it (ID, author and age on the first line of a run), File History the commits that changed the file; the selected line's commit shows in the details pane and `H` selects it in History; in File History `V` restores the working tree's file to the selected version after asking (`restoreFiles`, the working tree only, undone with `u`), except a version from before the file was renamed, which it refuses. In a commit's details, `B` and `L` do the same for the file in view (the one reached with `[` `]`, or the first header on screen), Blame at that commit; Esc goes back to where it was opened. History's edits as keys on the selected commit: `e` opens its message in git's editor and rewrites it when the text changed, `F` fixes it up into its parent, `X` drops it (the last two asked first; all three undo). Left: the edit mode itself (several edits at once, reordering, squashing with a new message, stopping to change files), the before and after drawings, a file with no changes (there is no file tree), opening worktrees, the edit mode, the drawings
- [ ] T7. The TUI column of frontends.md, usage.md, the CHANGELOG entry. Done so far: usage.md, the CHANGELOG entry, and a section in frontends.md listing what the TUI has (the column waits until its features settle)

## Decisions

Settled on 2026-11-17:

1. The TUI is part of the engine, `amonhen tui`, not a separate program.
2. `amonhen` with no arguments never starts the TUI; it keeps opening the app.
3. Mouse capture is on by default, with a key and a setting to turn it off.
4. The commit message is a field in the TUI, with git's editor on a key.

Open:

5. How far the first release goes: through T3 (read and commit), or through T5 (the everyday operations).
