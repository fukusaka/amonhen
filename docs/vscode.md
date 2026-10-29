# VSCode extension design notes

[日本語](ja/vscode.md)

## Overview

A personal extension for working with Git comfortably in VSCode. It lives in `apps/vscode` (paths below are relative to it) and
bundles the Amon Hen engine (the same as the app and the CLI). It is not published on the Marketplace; the `.vsix` is
distributed through GitHub Releases.

| Requirement | Category |
| --- | --- |
| Sidebar (Branches / Remotes / Tags / Stashes / Submodules) | Required |
| Merge / rebase / push by dragging and dropping branches | Required |
| Flexible commit editing (reword / squash / fixup / drop / reorder / split) | Required |
| Building commits finely, by hunk and by line, from the diff view | Required |
| Conflict resolution | Required |
| Rich diff display | Desired |
| Works over Remote-SSH | Desired |
| Fast even on large repositories | Desired |

VSCode's UI components offer little freedom, so instead of building a dedicated Git client's screens wholesale, the core operations are mapped onto VSCode's components. Currently the nav is in a sidebar TreeView, the contents in a Webview in the editor area (the main view), and conflicts are resolved in the standard 3-way merge editor (see "Bringing the UI into one Webview").

## Overall structure

With `extensionKind: ["workspace"]`, the extension host runs on the SSH host during a Remote-SSH connection, and git and
the engine run there too. That takes care of most of the over-SSH support.

```
Local (VSCode UI)                    Remote (extension host, the Remote-SSH host)
┌──────────────────────┐            ┌──────────────────────────────────────────────┐
│ TreeView (nav)       │            │ UI layer: providers, Webview communication   │
│ Webview (main view)  │──postMessage──▶      ▼                                   │
│ 3-way merge editor   │   (via SSH)  │ State management ◀── vscode.git API          │
└──────────────────────┘            │ (change events)                              │
                                    │        ▼                         ▼           │
                                    │ src/daemon/ (default)    git execution layer │
                                    │ front API client         (fallback)          │
                                    │        ▼                         ▼           │
                                    │ shared daemon            git CLI             │
                                    │ (amonhen --serve)                            │
                                    │        ▼                         ▼           │
                                    │ repository                                   │
                                    └──────────────────────────────────────────────┘
```

The Webview is rendered locally, and only its communication with the extension host goes over SSH. Large diffs and logs
are sent in chunks.

## Engine (shared daemon)

The default path. `src/daemon/` finds the bundled engine for the host (`bin/amonhen-<os>-<cpu>`), installs it with
`amonhen install`, and talks to the shared daemon (`amonhen --serve`; a Unix domain socket, or a named pipe on Windows)
through the front API: commands and answers in JSON, large data (History rows, graph, diffs) as records. The protocol
is in [front.md](front.md), and the design in item 18 of
[design.md](design.md).

- History, diffs, the Working Copy and writes go through the daemon. The daemon keeps the History, the search index and
  SSH connections, and shares them with the app and the CLI.
- The Undo log is the same file in the same format as the core's (`<git dir>/amonhen/undo.json`). `delegation.ts` keeps
  operations the core already records from being recorded again by the extension.
- Without a matching engine, or with `amonhen.daemon.enabled` off, everything runs on the extension's own git execution
  layer. The sections below on spawning git, `cat-file`, rebase plans and line-level staging describe that path; the
  engine does the same work in Rust.
- People are shown by their mailmap names on both paths. The engine maps them in the core (the details also carry the
  recorded name and email, shown under the name when they differ). The git execution layer reads `log.mailmap` and
  takes `%aN`/`%aE` for History rows (`%aN` maps even when `log.mailmap` is false), and maps the details with
  `git check-mailmap`. When `.mailmap`, a config file or the file `mailmap.file` names changes (their modification
  times, read on each refresh; where `mailmap.file` points is asked of git only after a config file changes), History
  and the details it keeps are read again.
- Range Diff (Compare with Previous Version on a local branch, and Range Diff… in the palette) shows a read-only
  `amonhen:` document in `git range-diff`'s form. The engine runs the core's `previousVersion` and `rangeDiff`; the
  git execution layer reads the branch's reflog (`git log -g`) and runs `git range-diff` itself.

## Implementation approach for the required features

### Sidebar

- Put a TreeView in a custom View Container. The items are Working Copy / Branches / Remotes / Tags / Stashes / Submodules.
- Leave detection of state changes to `vscode.git`'s `onDidChange`, and do not keep our own file watcher.

### Merge / rebase / push by drag and drop

- Implemented with `TreeDragAndDropController`. When a branch is dropped onto a branch, the user picks "Merge A into B" or "Rebase B onto A" in a QuickPick. Dropping onto a remote pushes.
- If the drop target is not the current branch, check it out before running. If a fast-forward is enough, handle it with just `update-ref`, without a checkout.
- Constraint: drag and drop does not work between a TreeView and a Webview. Reordering within the History is done entirely inside the Webview.

### Flexible commit editing

- reword / squash / fixup / drop / reorder / edit / split are all done with a non-interactive rebase. We build the whole todo ourselves (`src/history/rebasePlan.ts`) and overwrite git's todo with it via `GIT_SEQUENCE_EDITOR="cp <todo>"`.
  - Messages are rewritten by inserting `exec git commit --amend -F <file>` right after the target commit. This avoids having to tell on the `GIT_EDITOR` side which commit's message is being edited.
  - squash is `fixup` + amending the message, fixup is just `fixup`, and edit is `pick` + `break`.
  - Only commits reachable from HEAD by first-parent, up to just before the first merge commit, can be edited. Rebases that cross merges are not handled.
  - `--autostash` stashes working changes, and `--no-autosquash` keeps the user's settings from reordering the todo.
- split stops with "`pick` → `exec git reset HEAD^` → `break`"; the flow is to split and commit in the Working Copy, then Continue.
- While a merge or rebase is stopped, Continue / Abort are shown at the top of the Working Copy. The Undo record of an operation stopped by edit or split is held pending (`undo-pending.json`) and finalized when it finishes.
- Record `HEAD` and the related refs before an operation, so that each operation can be undone as a unit.
  - Records are taken both before and after the operation, and up to 50 are kept in `<gitDir>/amonhen/undo.json`. On Undo, check that the current state matches "after the operation", and do nothing if it differs.
  - The checked-out branch is restored with `reset` (`--soft` for a commit, `--keep` for a merge or rebase), and other branches are restored with `update-ref`, passing the old value as a condition. If the operation switched branches, it returns to the original branch.
  - Operations stopped by a conflict are not recorded; reverting them is abort's job. push is not undoable.

### Building commits finely from the diff view

- Staging happens in the Working Copy of the main view: select diff lines (Shift+click for a range) or use the hunk and
  file buttons. The standard diff editor is only for reading.
- With the engine, the daemon stages, unstages and discards the selected rows (`stageRows` / `discardRows`).
- On the git execution layer, the extension computes the line diff between the index and the working tree itself
  (Myers' algorithm, `src/diff/lineDiff.ts`), builds new index content with only the selected changes applied, and
  writes it with `hash-object -w` → `update-index --cacheinfo`. Unstaging does the same with the diff against HEAD.
  Compared with passing a partial patch to `git apply`, this does not stumble over context lines or line endings.

### Conflict resolution

- Reuse the standard 3-way merge editor. The extension is responsible for the list of conflicted files, taking ours / theirs per file (`checkout --ours/--theirs`), and the in-progress display and continue / abort / skip buttons during a merge / rebase.
- There is no public API to open the merge editor, so open it with the internal command `_open.mergeEditor`, which the built-in Git extension also uses, passing base, input1 and input2 (index stages 1, 2 and 3 returned via `amonhen:`) and output (the real file). If that fails, or for a conflict where one side was deleted, open the file and leave it to the standard UI's "Resolve in Merge Editor" and the sidebar's Accept Current / Incoming.
- When marking as resolved, ask for confirmation if conflict markers remain. During a rebase, show the progress (`rebase-merge/msgnum`, `end`) and allow Skip as well.

## Bringing the UI into one Webview

In steps 1–5, the Working Copy was built with a sidebar TreeView and the standard diff editor, and the History with a Webview in the editor area. Because they were split across components with completely different presentations, switching between Working Copy and History did not feel like moving within one screen. So the UI layer is rebuilt and brought into a single Webview in the editor area (the main view). The git execution layer, the state management layer, Undo and the rebase machinery are used as they are.

- The main view shows the nav (Working Copy / History / Branches / Remotes / Tags / Stashes) on the left and the contents on the right. Switching happens entirely inside the Webview, without waiting for a round trip to the extension host.
- Diffs are drawn inside the Webview. Favoring lightness, there is no syntax highlighting; it has word-level diff highlighting and stage / unstage by selecting hunks and lines. Computing diffs and writing to the index are done on the extension host side (`src/diff/lineDiff.ts`).
- The commit form handles multi-line messages and amend.
- The History details use the same diff component, so the diffs of the changed files can be read continuously by scrolling.
- Drag and drop of branches (merge / rebase / push) is done within the nav.
- Conflicts are still resolved by opening the standard 3-way merge editor.
- With Remote-SSH, postMessage goes over SSH, so diffs are sent per file when requested, and large files are split.
- Later, the nav (the entries for Working Copy / History, the in-progress merge / rebase, the branches and the like) moved to a sidebar TreeView (step 11). The main view uses its full width for the contents only, splitting the roles with the sidebar into "choosing" and "viewing and operating". Since the sidebar is a TreeView, its theme and interaction match the other sidebars, and drag and drop and the context menus are handled by the standard mechanisms.
- Staging is consolidated into the main view, and the path for staging by line from the standard diff editor has been removed. When the sidebar is shown via the activity bar icon, the main view opens too.

## Performance

On the git execution layer, the time taken by a small git command is almost entirely the process spawn cost (about 6 ms each on macOS). A query to the resident `cat-file` takes about 0.08 ms. "The number of spawns" and "the scope of updates" matter more than the language, so the following are applied thoroughly.

- Keep `git cat-file --batch-command` resident, rather than spawning for each object fetch. cat-file reads the index only once, at the start, so before querying `:path`, check the index's modification time and restart it if it has changed.
- Detect changes to refs, config and stashes by the modification times of files in `.git` (`HEAD`, the directories under `refs/`, `packed-refs`, `reftable/tables.list`, `config`, `logs/refs/stash`), without spawning. git updates refs by renaming lock files, so directory modification times are enough.
- Re-fetch only what changed. Refs, including ahead/behind, are fetched with a single `for-each-ref`. Worktree changes cannot be detected, so status is fetched every time. Independent commands run in parallel.
- Tags / Stashes / Submodules are not fetched until expanded. The TreeView diffs against the previous result and updates only the nodes that changed.
- Operations such as stage / unstage are reflected in the UI first and confirmed by git's result (optimistic updates).
- Bundle into a single file with esbuild, with no runtime dependency packages. The Webview does not use React, and stays light with virtual scrolling and canvas rendering of the graph.
- Record the time taken by git commands and redraws in Output, and build while measuring against the target of "sidebar updates within 30 ms on a typical repository".
- The git execution layer is behind an interface, so the Rust engine (gix) replaces it where it is available (see "Engine
  (shared daemon)"). History and diffs then come from the daemon as records, without spawning git.
- Get status with `git status --porcelain=v2 -z`. For large repositories, suggest enabling `core.untrackedCache` and fsmonitor.
- Pass `--no-optional-locks` to read commands, so as not to contend for locks with the user's own git operations.
- Fetch the log about 200 entries at a time. The graph's lane computation advances bit by bit on the extension host side, and only minimal arrays are sent to the Webview. The Webview side renders with virtual scrolling.
- Suggesting `git commit-graph write --reachable` speeds up the graph display and ancestry checks.
- With Remote-SSH, postMessage goes over SSH, so large diffs and logs are sent in chunks.

## Distribution

- Stable release: the root `.github/workflows/vscode-release.yml`. On pushing a `vscode-v*` tag (the app and engine use
  `v*`), build the engines (including Windows), sign the macOS one with Developer ID and
  submit it for notarization, bundle them with `scripts/bundle-engine.sh`, `vsce package`, attach a
  build provenance attestation, and create the Release.
- snapshot: the root `.github/workflows/snapshot.yml`. Whenever CI passes on main, `scripts/snapshot-version.mjs` produces
  `<next patch>-snapshot.<number of apps/vscode commits since the last release>+<sha>`, and the `.vsix` with the
  engines bundled goes on the same `snapshot` pre-release as the app and the engine. Under SemVer comparison it is older
  than the stable release, so installing the stable release overwrites it.
- Installing and updating on each machine: download the `.vsix` and `code --install-extension`, or
  the root `scripts/install-vscode.sh [stable|snapshot]` (finds the `.vsix` through the Releases API with `curl` → `code --install-extension --force`; also runnable as `curl … | sh`).

## Implementation order

Build the Working Copy and staging from the diff view, which are used every day, early on, to check how it feels.

- [x] 1. The git execution layer, state management, and the sidebar TreeView
- [x] 2. Working Copy and diff display, hunk- and line-level staging
- [x] 3. Merge / rebase / push by drag and drop, and the Undo foundation
- [x] 4. The History Webview and commit editing (rebase operations)
- [x] 5. The conflict resolution UI

Bringing the UI into one Webview (see "Bringing the UI into one Webview"):

- [x] 6. The main view's foundation (left nav and right area). Move the History into it
- [x] 7. Working Copy: file list, diffs, file-, hunk- and line-level staging, multi-line commit form
- [x] 8. Show diffs in the History details
- [x] 9. Branches and the like in the nav, drag and drop, the toolbar (Undo, Fetch, etc.)
- [x] 10. Tidying up the sidebar TreeView, and documentation
- [x] 11. Move the nav to a sidebar TreeView, and make the main view contents only

Using the engine:

- [ ] 12. History, diffs, the Working Copy and writes through the shared daemon. Tracked as item 18 of
  [design.md](design.md)
