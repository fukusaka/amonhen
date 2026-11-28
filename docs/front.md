# Front API (`amonhen-front`)

[日本語](ja/front.md)

The contract that exposes every function of the core in a language-independent form. Used by the VSCode extension (TypeScript) and the CLI. The core runs in the local shared daemon (`amonhen --serve`); FEs do not hold a core. Repositories on SSH hosts are opened by the daemon too (to FEs they look the same as local ones). The CLI can also open local repositories with an in-process core (`amonhen_front::InProcess`), passing the same commands.

The macOS app uses an in-process core through the C ABI (`crates/ffi-c`) (no copies, via shared memory and mmap: the fastest path).

## Connecting

1. Connect to the Unix domain socket (on Windows, the named pipe `\\.\pipe\amonhen-<hash of the user's SID>-<build fingerprint>`; Node can connect to it directly with `net.connect`). `amonhen --socket` prints its location (on macOS `$TMPDIR/amonhend-<first 8 characters of the build fingerprint>.sock`, elsewhere `$XDG_RUNTIME_DIR` or `/tmp/amonhen-<uid>`; it can be changed with `AMONHEN_DAEMON_DIR`). If it cannot connect, launch `amonhen --serve <socket>` detached from the parent and wait until it can connect.
2. First, send the location to open: a length (u32, LE) and the UTF-8 string `"\u0001" + location`, optionally followed by `"\u0000"` and the front-end's environment as `NAME=value` pairs separated by `"\u0000"`. The location is the absolute path of a local repository or `ssh://host/path`. The daemon runs git and ssh for this connection (opening an `ssh://` location included) with that environment instead of its own: it takes what git and ssh always use (`PATH`, `SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `GIT_*`, and `AMONHEN_ASKPASS`, `AMONHEN_GIT`, `AMONHEN_FALLBACK_GIT` and `AMONHEN_PASS_ENV`) and the names listed in `AMONHEN_PASS_ENV` (`NAME1,NAME2`), but never those that point to a repository (such as `GIT_DIR`), `GIT_ASKPASS` or other `AMONHEN_*`, and ignores the rest (`amonhen_model::process::ClientEnv`). When the front-end's environment changes, it can send it again over the same connection, from the next request on: the app through `Request::Environment`, and a front-end on this API with `environment` (the VSCode extension does when `git.path` changes).
3. After that, communicate in frames. Make the first request `{"op":"hello","language":"ja"}` (if the repository cannot be opened, its reply is a failure with the reason). `language` is the front-end's display language (`en`, `ja`, `ja-JP` …); the daemon writes this connection's error messages in it (English when it has no translation, or when it is left out).

## Frames

| Bytes | Contents |
|---|---|
| 0..4 | Length (u32, LE): from the kind to the end of the payload (9 + payload length) |
| 4 | Kind: 1 request, 2 success, 3 failure, 4 notification, 5 cancel (the top bit marks compression; not used by this API) |
| 5..13 | Number (u64, LE). A reply has the same number as its request. 0 for notifications |
| 13.. | Payload |

- Request payload: the command JSON (UTF-8).
- Success payload: JSON length (u32, LE), JSON, padding to an 8-byte boundary, records (below).
- Failure payload: the reason text (UTF-8).
- Notification payload: `{"changes": <bits>}`. 1 refs, 2 index, 4 working tree, 8 config.
- Cancel: send the number of a running request (such as a search). That request ends in failure.

Requests may be pipelined. Replies come back in the order they finish.

## Records

The success JSON's `data` holds `[start, length]` (in bytes, from the start of the records) for each section. Sections start on an 8-byte boundary. All numbers are LE. In TS, slice a section out of the payload into an `ArrayBuffer` (use `slice` to make it start at 0) and read it with typed arrays.

| Type | Size | Layout |
|---|---|---|
| Node `Node` | 12 | row u32, lane u16, color u16, flags u16 (1 merge, 2 HEAD, 4 stash, 8 uncommitted), unused u16 |
| Edge `Edge` | 24 | rowStart u32, rowEnd u32, laneStart u16, laneEnd u16, color u16, kind u16 (1 first-parent, 2 start of a merge), child u32, parent u32 |
| ID | 20 | SHA-1 bytes |
| Diff line | 16 | kind u8 (0 context, 1 added, 2 removed, 3 hunk, 4 ours, 5 theirs, 6 base, 7 marker, 8 file header), flags u8 (1 staged), unused u16, old u32, new u32, start in `text` u32 |

## Commands

History and diffs are held by number (`history`, `diff`). Even if History is refreshed while an FE is scrolling, the same number returns the same rows. Up to 16 of the newest are kept per FE; older ones become "no longer available" (read again).

| Command | Arguments | Reply (JSON) | Records |
|---|---|---|---|
| `hello` | language | version, workdir, gitDir | |
| `activity` | | records (Activity's operations where git runs: seq, kind, repo, label, started (ms), duration, ok, error, progress, cancellable, output), remote (true when they come from the SSH host; cancel with `cancelActivity`'s remote) | |
| `environment` | env (the front-end's environment, `NAME=value` pairs separated by `"\u0000"`, as at opening) | | |
| `history` | rebuild?, fold? (fold every merge), expanded? (array of IDs; merges left open under fold), collapsed? (array of IDs; without fold, fold only these merges), tips? (array of IDs; only what is reachable from them) | history, generation, rows, head, headRow, folded, shallow (a shallow clone's History), boundaryRows (rows of the commits at the shallow boundary, whose parents aren't fetched) | |
| `historyMatching` | history, rows | same as `history` (a History of only the matching rows) | |
| `historyRelease` | history | | |
| `historyRows` | history, and start and count or rows (array of rows), recorded? (true to list the rows whose author the mailmap changed) | count, edges, recorded (index into the rows, name and email as recorded; empty unless asked) | rowNumbers u32, nodes, ids, times i64, spans (start and length of the subject and author in `text`, u32 × 4; the author by the mailmap), text (UTF-8), signatures u8, folded u32 (for a folded History), parentStarts u32 (number of rows + 1), parents u32 (parent rows), parentIds (parent IDs), edges, edgeIds u32 (only with start and count; edges overlapping that range) |
| `historyGraph` | history | rows, edges | nodes, edges, ids, folded (all rows) |
| `historySearch` | history, query (syntax of `crates/core/src/query.rs`) | count | rows u32 (ascending) |
| `historySearchPartial` | history, query (of the `historySearch` running on this connection) | count, done (how much is searched, 0–1; stays 0 without a contents term) | rows u32 (ascending; the matches found so far, part of the final answer; empty unless that search is the latest one started) |
| `historyContentsToFetch` | history, query | count (contents a partial clone must fetch before searching `contents:`; 0 when nothing), more (over the limit; `historySearch` then fails) | |
| `historyDensity` | history, buckets | buckets | density (f32 × 2: frequency, activity, 0 to 1) |
| `historyLineage` | history, row | | rows (u64 bit set), edgeFrom u32 |
| `historyLanes` | history, start, end, metrics (full, thin, minFull, minThin, available) | width (of all lanes) | positions f32 |
| `historyRowOf` | history, id | row (null if absent) | |
| `historyOrder` | order (`date`, or `topo` to keep a branch's commits together under its merge; applies to every History this session loads from now on, and the current one is reordered without rereading git) | order | |
| `refs` | | headBranch, head, entries (kind, name, target, detail, ahead, behind, gone) | |
| `status` | | generation, branch, upstream, ahead, behind, entries (kind, path, origPath, index, worktree, staged, unstaged, submodule; content conflicts, UU and AA, also have markers, the number of conflict markers left in the working tree file, and rerere, whether git's rerere applied an earlier resolution) | |
| `commitDiff` | id or stash (ID), or history, rows, pair? (forward, reverse, mergeBase); whitespace? (exact, at-eol, change, all: what to ignore when comparing lines, as `git diff --ignore-space-at-eol`, `-b`, `-w`); contexts? (file number as a string → lines of context, or "all" for the whole file; the others keep 3) | files (path, oldPath, kind, added, removed, binary, generated: `linguist-generated` or else a lock file or minified file, which the apps fold at first), changes, selection, rows, view (for `diffSyntax`) | diff lines (below) |
| `workingDiff` | path, mode (unstaged, staged, combined), whitespace? (as `commitDiff`; not with combined, and a diff that ignores whitespace can't be used with `stageRows` or `discardRows`); context? (lines of context, or "all" for the whole file; 3 if omitted); open? (`[{gap, top, bottom}]`: show the unchanged stretches folded into gap lines, below; even an empty list folds them) | diff, path, mode, binary, added, removed, rows, lineEndings (below) | diff lines |
| `treeFile` | oid (40 hex digits), path, plain? (true: just the file, without marking the commit's changes; Working Copy's All Files uses it for files unchanged since HEAD) | path, changed, binary | diff lines: the file as it is at that commit (History's All Files); when the commit changed it, the commit's diff of it with every line around the changes (a header line first), so the changed lines are marked |
| `setSignatureCheck` | local (true to check commit signatures of a repository opened through the daemon on an SSH host with this machine's keys, false with the host's git and keys) | remote (whether it applies: a local repository is always checked locally) | — |
| `conflict` | path | path, rows | diff lines |
| `diffSyntax` | diff (of `workingDiff`) or view (of `commitDiff`) | spans | spans (row, start, end, kind as u32 × 4: the diff line, positions in that line's text as in highlights, and keyword 1, string 2, comment 3, number 4 and the rest of `SyntaxSpan`'s kinds); asked after showing the diff, since it can take a while |
| `textSyntax` | path (picks the language), text | spans | spans as above, with row 0 and UTF-16 positions from the start of the text |
| `stage`, `unstage` | paths | | |
| `stageRows` | diff, rows, stage? (for a combined diff; false to unstage) | | |
| `discardRows` | diff, rows (lines of an unstaged diff) | | |
| `commit` | message, amend? | output (what hooks wrote) | |
| `run` | command (a command of `crates/core/src/command.rs`: checkout, merge, rebase, fetch, push, stash, undo, blame, settings and more) | that command's reply (ok, error and others) | |
| `connection` | | sent, received, closed, engine (the SSH host connection, and the engine used there and how it was set up; null if local) | |

Diff line records: rows (diff line × number of rows), textLengths u32, text (UTF-8), highlights (row, start, end as u32 × 3; byte positions of changed words). `commitDiff` starts each file with a header line (kind 8, text is the path, old and new are the numbers of removed and added lines). The line numbers (0-based) of `workingDiff` are the numbers that the rows of `stageRows` and `discardRows` refer to.

With `open`, `workingDiff` has no `@@` header lines. Each unchanged stretch of one line or more (the top of the file, between hunks, the end of the file) becomes one gap line instead: kind `HUNK` (3), old is the number of lines hidden, new is the number of lines shown, and the text is the function name of the next hunk (empty if none, and for the gap at the end). When the file changes from its first line, the first hunk still gets a gap line with 0 hidden lines. Gaps are numbered 0, 1, … in the order of their gap lines, and `{gap, top, bottom}` shows `top` lines from the top of that gap (just after the previous change) and `bottom` lines from its bottom (just before the next change); the shown lines are unchanged lines, the top ones before the gap line and the bottom ones after it. When top and bottom add up to the gap's length or more, the whole gap shows (the gap line stays, with 0 hidden lines). `stageRows` and `discardRows` take the rows of a folded or whole-file diff too.

`workingDiff`'s lineEndings (null for staged diffs, submodules and deleted files): index, worktree and checkout are each `none`, `lf`, `crlf` or `mixed` (what git stores, the working tree file, and what checkout would write); rule is the conversion in effect, written like `git check-attr` (`text=auto eol=crlf`, `-text`, `core.autocrlf=true`; empty if none or not notable); notable is true when the working tree and git differ in line endings or warning is true; warning is true when line endings are mixed, or the working tree has CRLF where checkout would write LF. Diffs compare what git stores on both sides, so these differences don't show as changed lines; front-ends show them beside the diff when notable.

## The drawing helper (`amonhen-render`)

The engine has no drawing features (see "Repository layout" in [design.md](design.md)). The VSCode extension draws All Files' Mermaid and DOT diagrams, math and Android VectorDrawable pictures, and checks SVGs for mobile renderers, with `amonhen-render` (`crates/render`), a separate executable bundled next to the engine as `bin/amonhen-render-<os>-<arch>` (under Remote-SSH it runs on the SSH host, like the engine). The extension starts it for each file it shows, with no arguments and `AMONHEN_LANG` set to its language, writes requests to its standard input and reads replies from its standard output, then closes the input; the helper answers what is left and exits. `amonhen-render --version` prints `amonhen-render <version>`.

Every message is one line of JSON (UTF-8). Requests carry an `id` (any JSON value, returned as is) and an `op`, and may be sent all at once; they are answered in order:

| `op` | Fields | Reply |
|---|---|---|
| `diagram` | language (the fence's word: `mermaid`, `dot`, `graphviz` or `gv`), text, from? (the Markdown file's path from the repository root; DOT images are read relative to its folder), colors (dark, text, background, kinds: the screen's `#rrggbb` colors, kinds being syntax kinds such as `keyword`), scheme? (the apps' Diagrams > Colors key: `syntax`, `pele`, `mermaid`, `mermaid-default` and the other named themes, or `mermaid-import:<name>`; `mermaid` without it), importedTheme? (the mermaid-cli config file's text, for `mermaid-import:`), timeZone? (gantt dates: `""` for the helper's own time zone, `UTC` or an IANA name, following daylight saving; a name it can't read means its own time zone), utcOffset? (the older fixed offset in minutes, local time − UTC, when there is no timeZone; UTC without either) | svg (colored by the scheme with the screen's colors, or by the diagram's own Mermaid theme) and note? (it's an approximation), or error (why it can't be drawn), or neither (not a diagram it draws: show the code); themeError? (the imported theme can't be read: the syntax colors are used) |
| `math` | formulas (tex, display?: one Markdown file's formulas, in order, under the apps' per-file limits), color (`#rrggbb`), timeLimit? (seconds one file's formulas may take, 0 for no limit; 2 without it) | formulas: for each, svg, width, height, depth (in em of the text size × the formula size, 1.21 by default), or error, or neither (show the TeX) |
| `vectorDrawable` | path (from the repository root), data (the XML, base64), fallback? (the color for what can't be resolved), dark? (look up the dark appearance's resources such as `values-night` first) | svg (sized in dp), notes |
| `svgCheck` | data (the SVG, base64) | issues (targets, feature, message, count), previews (android, ios, flutter, reactNative: svg? (SVG text; missing when the renderer can't read it), title, caption, messages, error?, exact — Android and flutter_svg are their real importers ported, iOS and react-native-svg approximations, as in the apps) |

A reply is `{"id": …, "ok": true, …}` with the fields above, or `{"id": …, "ok": false, "message": …}` when the request itself fails (a line that isn't JSON gets `"id": null`).

The helper never reads the repository. When a diagram's image or a VectorDrawable's resources are needed, it writes a question, `{"ask": n, "read": "path"}` for a file or `{"ask": n, "list": "folder"}` for a folder (the root is `""`), with paths from the repository root at the same commit, and waits for the answer, `{"answer": n, "data": "<base64>"}` (`null` when there is no such file) or `{"answer": n, "entries": [{"name": …, "folder": true|false}]}`. Questions come one at a time while a request is drawn; the extension reads the files from git and the folders with the engine's `treeEntries`, answering images over 1 MB as missing.

## Version

When the shape of the API changes, bump `amonhen_front::VERSION` (check it with the version in the `hello` reply). The socket name includes the build fingerprint (`amonhen_remote::BUILD_ID`, a hash of the workspace's sources), so it never connects to a daemon of a different build.

When a front-end starts the shared daemon itself, it passes its language as `AMONHEN_LANG` (the daemon uses it for what it does on its own). Errors from an SSH host follow the language of the front-end that asked, even when several front-ends share the connection to that host.
