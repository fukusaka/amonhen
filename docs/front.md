# Front API (`amonhen-front`)

[日本語](ja/front.md)

The contract that exposes every function of the core in a language-independent form. Used by the VSCode extension (TypeScript) and the CLI. The core runs in the local shared daemon (`amonhen --serve`); FEs do not hold a core. Repositories on SSH hosts are opened by the daemon too (to FEs they look the same as local ones). The CLI can also open local repositories with an in-process core (`amonhen_front::InProcess`), passing the same commands.

The macOS app uses an in-process core through the C ABI (`crates/ffi-c`) (no copies, via shared memory and mmap: the fastest path).

## Connecting

1. Connect to the Unix domain socket (on Windows, the named pipe `\\.\pipe\amonhen-<hash of the user's SID>-<build fingerprint>`; Node can connect to it directly with `net.connect`). `amonhen --socket` prints its location (on macOS `$TMPDIR/amonhend-<first 8 characters of the build fingerprint>.sock`, elsewhere `$XDG_RUNTIME_DIR` or `/tmp/amonhen-<uid>`; it can be changed with `AMONHEN_DAEMON_DIR`). If it cannot connect, launch `amonhen --serve <socket>` detached from the parent and wait until it can connect.
2. First, send the location to open: a length (u32, LE) and the UTF-8 string `"\u0001" + location`, optionally followed by `"\u0000"` and the front-end's environment as `NAME=value` pairs separated by `"\u0000"`. The location is the absolute path of a local repository or `ssh://host/path`. The daemon runs git and ssh for this connection (opening an `ssh://` location included) with that environment instead of its own: it takes `PATH`, `AMONHEN_ASKPASS`, `SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `GIT_*` and the names listed in `AMONHEN_PASS_ENV` (`NAME1,NAME2`), but never those that point to a repository (such as `GIT_DIR`), `GIT_ASKPASS` or other `AMONHEN_*`, and ignores the rest (`amonhen_model::process::ClientEnv`).
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
| `history` | rebuild?, fold?, expanded? (array of IDs), tips? (array of IDs; only what is reachable from them) | history, generation, rows, head, headRow, folded | |
| `historyMatching` | history, rows | same as `history` (a History of only the matching rows) | |
| `historyRelease` | history | | |
| `historyRows` | history, and start and count or rows (array of rows) | count, edges | rowNumbers u32, nodes, ids, times i64, spans (start and length of the subject and author in `text`, u32 × 4), text (UTF-8), signatures u8, folded u32 (for a folded History), parentStarts u32 (number of rows + 1), parents u32 (parent rows), parentIds (parent IDs), edges, edgeIds u32 (only with start and count; edges overlapping that range) |
| `historyGraph` | history | rows, edges | nodes, edges, ids, folded (all rows) |
| `historySearch` | history, query (syntax of `crates/core/src/query.rs`) | count | rows u32 (ascending) |
| `historyDensity` | history, buckets | buckets | density (f32 × 2: frequency, activity, 0 to 1) |
| `historyLineage` | history, row | | rows (u64 bit set), edgeFrom u32 |
| `historyLanes` | history, start, end, metrics (full, thin, minFull, minThin, available) | width (of all lanes) | positions f32 |
| `historyRowOf` | history, id | row (null if absent) | |
| `historyOrder` | order (`date`, or `topo` to keep a branch's commits together under its merge; applies to every History this session loads from now on, and the current one is reordered without rereading git) | order | |
| `refs` | | headBranch, head, entries (kind, name, target, detail, ahead, behind, gone) | |
| `status` | | generation, branch, upstream, ahead, behind, entries (kind, path, origPath, index, worktree, staged, unstaged, submodule) | |
| `commitDiff` | id or stash (ID), or history, rows, pair? (forward, reverse, mergeBase) | files (path, oldPath, kind, added, removed, binary), changes, selection, rows | diff lines (below) |
| `workingDiff` | path, mode (unstaged, staged, combined) | diff, path, mode, binary, added, removed, rows, lineEndings (below) | diff lines |
| `conflict` | path | path, rows | diff lines |
| `stage`, `unstage` | paths | | |
| `stageRows` | diff, rows, stage? (for a combined diff; false to unstage) | | |
| `discardRows` | diff, rows (lines of an unstaged diff) | | |
| `commit` | message, amend? | | |
| `run` | command (a command of `crates/core/src/command.rs`: checkout, merge, rebase, fetch, push, stash, undo, blame, settings and more) | that command's reply (ok, error and others) | |
| `connection` | | sent, received, closed (the SSH host connection; null if local) | |

Diff line records: rows (diff line × number of rows), textLengths u32, text (UTF-8), highlights (row, start, end as u32 × 3; byte positions of changed words). `commitDiff` starts each file with a header line (kind 8, text is the path, old and new are the numbers of removed and added lines). The line numbers (0-based) of `workingDiff` are the numbers that the rows of `stageRows` and `discardRows` refer to.

`workingDiff`'s lineEndings (null for staged diffs, submodules and deleted files): index, worktree and checkout are each `none`, `lf`, `crlf` or `mixed` (what git stores, the working tree file, and what checkout would write); rule is the conversion in effect, written like `git check-attr` (`text=auto eol=crlf`, `-text`, `core.autocrlf=true`; empty if none or not notable); notable is true when the working tree and git differ in line endings or warning is true; warning is true when line endings are mixed, or the working tree has CRLF where checkout would write LF. Diffs compare what git stores on both sides, so these differences don't show as changed lines; front-ends show them beside the diff when notable.

## Version

When the shape of the API changes, bump `amonhen_front::VERSION` (check it with the version in the `hello` reply). The socket name includes the build fingerprint (`amonhen_remote::BUILD_ID`, a hash of the workspace's sources), so it never connects to a daemon of a different build.

When a front-end starts the shared daemon itself, it passes its language as `AMONHEN_LANG` (the daemon uses it for what it does on its own, such as connecting to SSH hosts).
