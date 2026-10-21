# Amon Hen Standalone plan

[日本語](ja/design.md)

## Overview

A native Git client with the same feel as the Amon Hen VSCode extension, built without the constraints of VSCode's UI components. Thoroughly low latency comes first. The core is written in Rust and reused on each OS; the first FE (frontend) is built for macOS. Like the extension, it is for my own use and is distributed through GitHub Releases.

| Principle | Details |
| --- | --- |
| Speed | Within 1 frame from input to display (8.3 ms at 120Hz). The UI thread never waits on git |
| Structure | Core logic in Rust, UI native to each OS. macOS (AppKit) only at first |
| Reuse | The core runs on macOS / iOS / Windows / Linux / Android. Each FE can choose the data format best for it |
| No compromises | No SwiftUI, Qt or WebView for heavy screens. History and diffs are drawn by our own code |
| Expand step by step | The core is shaped to run in the same process, as a local daemon, or over SSH; at first only the same-process form is built |

### Requirements

Inherit all of the extension's requirements ("Overview" in vscode-amonhen's `docs/design.md`) and add the following.

| Requirement | Category |
| --- | --- |
| Scroll History at 120fps in repositories with 100k–1M commits | Required |
| Show the previous History immediately after launch (persistent cache) | Required |
| Japanese input and accessibility behave as the OS standard | Required |
| The core can be reused on macOS / iOS / Windows / Linux / Android | Required |
| Each FE can choose the best data format (string encoding, columns, GPU vertex format), with no wasted copies in conversion | Required |
| Refinements to the History graph (emphasized trunk, ancestor highlighting, folding, animation) | Required |
| The same core can be used from a CLI (`amonhen`) | Desired |
| Repositories on an SSH host can be operated as fast as local ones | Desired |
| The VSCode extension's git execution layer and state management layer can be replaced by the core's daemon | Desired |

### Non-goals

- FEs other than macOS (the core and abstractions support them from the start, and CI builds all targets, but no FE for now)
- Implementing with Qt, Electron, Tauri or Flutter (considered and ruled out)
- Drawing the whole screen with Metal (input fields, settings and dialogs use standard components)

## Technology choices

| Layer | Choice | Reason |
| --- | --- | --- |
| Core | Rust | No GC pauses, and memory layout can be controlled |
| Reading git | gix (gitoxide) in the same process | Removes the spawn cost (about 6 ms each) and pipe copies. Can read commit-graph directly |
| Writing git | Spawn `git` on desktop, gix and libgit2 on iOS / Android | Leave rebase, merge, push etc. to upstream git to keep correctness and compatibility. git cannot be launched on mobile. Swapped via `GitBackend` |
| Async | tokio (I/O, processes) + rayon (CPU) | Stale requests are cancelled immediately |
| Diff | imara-diff, highlighting with tree-sitter | Highlighting comes later; show without colors first, immediately |
| Bridge | Built on a C ABI (cbindgen), UniFFI (Swift / Kotlin), Linux and Windows use it directly from Rust | Type-safe for operations, zero-copy for row data |
| OS features | Injected from the FE via the `Platform` trait. File watching with notify | Swap how the main thread is woken, credentials, SSH and storage locations per OS |
| UI skeleton, heavy screens | AppKit | Direct control over row reuse, draw regions and layers |
| UI periphery | SwiftUI (embedded with `NSHostingView`) | Build settings, dialogs and Welcome quickly |
| GPU | Draw History and diffs with Metal (sharing the glyph atlas). Everything else with Core Animation and standard components | Metal text drawing has to be built anyway for History labels, so reuse it for diffs |
| First FE | macOS 14.4 or later, Apple Silicon | Uses `os_sync_wait_on_address`, `NSView.displayLink` |
| Core build targets | `aarch64-apple-darwin`, `aarch64-apple-ios`, `x86_64` / `aarch64-pc-windows-msvc`, `x86_64` / `aarch64-unknown-linux-gnu`, `aarch64-linux-android` | CI keeps OS-specific code from slipping into the core |

## Overall structure

```
┌──────────────────────── macOS app (Swift / AppKit) ───────────────────────────┐
│ Sidebar          History               Diff view            Commit input     │
│ NSOutlineView   NSTableView           Custom NSView        NSTextView        │
│                 + Metal graph          Metal (shared text)  (IME by the OS)  │
│ 3-way merge (custom NSView) · Settings, dialogs (SwiftUI)                    │
│ ──────────────────────────────────────────────────────────────────────────── │
│ Thin Swift layer: read only visible rows → draw / send operations as Intents │
└──────────────▲───────────────────────────────────────────────┬───────────────┘
               │ Change notices (changed row ranges) / row data (zero-copy) │ Intent
┌──────────────┴────────── CoreClient (abstracts the transport) ───▼───────────────┐
│ ① Same process: C ABI ② Local daemon: socket + shared memory ③ SSH: stdio    │
└──────────────▲───────────────────────────────────────────────┬───────────────┘
┌──────────────┴──────────────────── Rust core ─────────────────▼───────────────┐
│ model      Internal model (knows no FE. UTF-8, references into decoded buffers) │
│ core       State, snapshots, Intent handling (one-way data flow)              │
│ projection Conversion to each FE's shape (only changed chunks, only once)     │
│ backend    GitBackend: gix / git CLI / libgit2                               │
│ platform   Platform: waking, credentials, SSH, storage (injected from the FE) │
│ index   Persistent cache of commit info (mmap, same layout as in memory)      │
│ graph   Incremental lane computation, recomputing only the difference        │
│ diff    imara-diff, word diff, tree-sitter highlighting                       │
│ watch   Watching with notify (FSEvents / inotify / ReadDirectoryChangesW)    │
│ oplog   Operation log for Undo (records HEAD before the operation and related refs) │
│ search  log and pickaxe search (parallel with rayon)                         │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Screen layout

| Screen | Left | Right |
| --- | --- | --- |
| Working Copy | Commit input (top) and the list of changed files (bottom; checkboxes decide whether each goes into the next commit) | Diff |
| History | List of commits (graph and subject) | From the top: message and metadata (refs, author, date, ID, parents), changed files, diff |

Selecting a conflicted file in Working Copy shows a merge editor on the right (it can also be moved to a separate window). Current is on the left, Result (starting from the working tree contents) in the middle, and Incoming on the right (left and right are the contents with every conflict taken from that side; with diff3, Base can be shown too), and bands between the columns connect the conflicts. Each conflict is accepted with the ≫ / ≪ on the bands or with buttons, or edited by hand, then saved and marked resolved. ✨ Magic Merge resolves in one go the conflicts that fall into one of these: both sides are the same, only one side changed, only whitespace differs, or different words on the same line were changed. The resolution is computed by the core (`mergeSides`, `resolveText`, `conflictRegions`, `magicMerge`, `saveMerge`). During a merge, rebase etc., a band is shown under the window's toolbar (the number of conflicts and Continue, Skip, Abort).

The top pane of History's right column (commit details) lists the metadata, the subject, then the rest of the body. There are three presentations to choose from (top right of the pane; the choice is remembered).

- Compact: one line with a small avatar, author, author date, refs and parents, plus the subject. The pane shrinks to the height of the text.
- Standard: a top band with author and committer side by side (Gravatar avatar, name, email, date), below it a two-column table with right-aligned headings (ID, parents, signature, refs, merged tags, number of commits brought in), then the subject and body.
- Detailed: the same shape as Standard, with the table adding children, the position in `HEAD~2^2~1` form (can be passed to git as is), containing branches and tags, the preceding tag, whether it is pushed, 20 of the commits brought in, commits the message refers to (cherry-pick and revert sources, Fixes:), and notes.

The signature is one line (result, signer, key, trust level); parents and children can be selected and copied as 40 hex digits. Dates are absolute date and time with time zone, not relative. Line counts (+/−) are shown on each row of the changed file list. Message trailers are shown within the message and not copied into the table. Avatars are fetched from gravatar.com by the MD5 of the email address and remembered only while the app is running (can be turned off with View > Show Avatars).

What can be shown right away (what can be read from the header and message; `commitInfo`) is drawn first, and the slow parts (signature verification, notes and line counts from git; children, containing branches and tags, preceding and following tags, position relative to HEAD and commits brought in from the History graph; `commitExtras`, 30–55 ms on linux) are read in the background and then added. The body can be monospaced or proportional (View menu) and is shown in full without folding, however long. Each time a commit is selected, the details pane fits the height of the message (up to half of the right column; the rest scrolls within the pane), and ⤢ at the top right expands it to the full right column. Fitting the height relays out the whole right column (about 5 ms on linux), so it is done only once, together, after display is finished. While searching in History, matches in the subject and body are highlighted.

Operations: URLs, commit IDs and `#123` (to the issue if origin is GitHub or GitLab) are links. Clicking the author filters to that person's commits; clicking a ref or branch filters to its history. Right-click copies the ID, subject, message or `git show`. ⌥⌘↓ and ⌥⌘↑ move to the first parent and the first child. Non-merge commits contained in HEAD can be reworded in place with "Reword Message…" on the right-click menu (save with ⌘↩; confirms if already pushed; can be reverted with Undo).

The review of UI and operations (tabs, bookmarks, settings, toolbar, refining Working Copy, History and the merge editor, Activity, integration with GitHub and AI) is collected in `docs/ux.md`.

### Repository layout

Built in this repository, separate from the extension (vscode-amonhen).

```
crates/
  layout/            #[repr(C)] records shared by all FEs (numeric columns, graph Node / Edge), Slab, Blob
  model/             Internal model (knows nothing of FEs), GitBackend, Operations, search index, Activity
  core/              State, snapshots, command (JSON) handling, layout, diff, folding
  projection/        Conversion to each FE's shape (string encoding, column selection, alignment)
  platform/          Platform trait
  backend-gix/       GixBackend (reading)
  backend-cli/       git CLI (writing, network, signature verification)
  watch/             Detecting repository changes (notify)
  syntax/            Highlighting with tree-sitter
  remote/            Communication with the daemon (frames, request shapes), RemoteBackend, Server, connecting to the shared daemon,
                     shared memory (local, shm), pushing over SSH (ssh)
  daemon/            daemon (engine amonhen's --stdio: SSH host, --serve: local shared daemon)
  ffi-c/             C ABI + cbindgen header
  cli/               CLI (amonhen)
apps/
  macos/             SwiftPM package (AmonhenKit and the AppKit app)
```
To add later: `backend-git2/` (for mobile), bindings for Swift / Kotlin.
## Data design

### Immutable snapshots

- Each time the state changes, the core builds a new snapshot and publishes it with `arc-swap`. Published snapshots are never modified. The UI needs no locks to read.
- Row data is split into chunks of 4096 rows, and only changed chunks are rebuilt (the rest are shared with the previous generation).
- Each snapshot has a `generation`. The UI uses the generation number to detect drawing with stale data.

### Internal model and projection

- The internal model holds only shapes convenient for git processing and computation, and knows no FE. Strings reference the `Arc<[u8]>` decoded by gix by offset and length, without copying.
- At connection time, the FE declares a `FrontendProfile` (string encoding, required columns, GPU vertex format, alignment). The projection layer converts to that shape chunk by chunk.
- Only the visible range and the prefetch range are converted. Results are cached by "generation + chunk + format", and unchanged chunks are shared with the previous generation. Multiple FEs with the same format share them.
- Conversion results are also written to the persistent cache per format, so the next launch only needs to mmap them. The internal persistent cache (things closed inside Rust, such as checkpoints and the ref index) may use rkyv, but the shape handed to FEs is defined by us (rkyv's layout is not documented across languages, and its strings are fixed to UTF-8).

### Zero-copy rules

- If the shape is the same, do not convert. Numeric columns (dates, indices, lanes, flags) and graph `Node` / `Edge` share the same `#[repr(C)]` records across all FEs, and on the GPU side the vertex descriptor adapts how they are read.
- Data is held as per-column arrays (SoA). An FE choosing columns just chooses pointers, with no repacking.
- Strings are unified as "a common byte sequence if ASCII, the FE's preferred encoding only otherwise". ASCII is both UTF-8 and Latin-1 as is, so most strings (hashes, paths, English messages, branch names) are copy-free for every FE. Conversion is needed only when passing non-ASCII strings to UTF-16-based FEs, and it is written once, directly from the decoded buffer into the FE's region.
- References are held as offsets from the start of a region rather than pointers (so they work as is across shared memory, files and the network). In other processes and remotely, gathering into a contiguous region and converting the encoding are done at the same time, so there is a single write.
- Chunk regions are allocated aligned to page boundaries for all FEs (so they can be shown to the GPU as is with `MTLBuffer(bytesNoCopy:)`, `VK_EXT_external_memory_host`). With a discrete GPU, a single transfer over PCIe cannot be avoided.
- The only work that remains in principle is "decoding git objects" and "drawing". Both are done only once and cached.

| FE | Strings | Copy-free handoff | Drawing |
| --- | --- | --- | --- |
| macOS / iOS | ASCII + UTF-16 | Read directly with `UnsafeBufferPointer` (raw pointers never leave `withSnapshot { }`). `CFStringCreateWithBytesNoCopy` (ASCII) / `CFStringCreateWithCharactersNoCopy` (UTF-16) | Metal + Core Text |
| Windows | ASCII + UTF-16 | UTF-16 as `PCWSTR` as is. ASCII is widened on the stack at draw time, only for visible rows | Direct2D / D3D + DirectWrite |
| Linux | UTF-8 | The UI is also written in Rust (gtk-rs) and references the data without FFI | GTK4 (GSK) / Vulkan |
| Android | ASCII (Latin-1) + UTF-16 | JNI's `NewDirectByteBuffer`. No Java `String` is created; a `CharSequence` referencing the buffer is passed to the drawing API | Compose / Canvas, Vulkan if needed |
| VSCode / Web | UTF-16 or UTF-8 | `ArrayBuffer` via the daemon, or referencing WASM memory directly | Canvas / WebGL |

In the same process, the number of writes is 0 for numeric columns, the graph and ASCII strings on every FE, and for non-ASCII strings 0 on UTF-8 FEs and 1 on UTF-16-based FEs (only when they change).

### Change notifications

- The core's worker threads push events onto an SPSC ring buffer (`rtrb`) and wake the FE's main thread with the waker received from `Platform`. On macOS / iOS this is `DispatchSource` (`.data_add`), which coalesces signals automatically, so the UI updates only once per frame. Other OSes use `PostMessage` (Windows), the GLib main loop (Linux), and Looper / Handler (Android).

### When moving to another process or remote

- ② Local shared daemon (`amonhen --serve`): one per user, listening on a Unix domain socket. The daemon writes History to the persistent cache file, and the connecting side mmaps it. Diff file contents are passed in POSIX shared memory. Commands, and Working Copy operations that write Undo records, run in the daemon's core.
  - SSH hosts can be opened too (`daemon:ssh://host/path`; the CLI's `-C ssh://…` always uses this). The local daemon prepares and connects to the engine (`amonhen --stdio`) on the SSH host, and shares that connection and the core (History, search index, diffs) among the clients that opened the same SSH host. Only work that touches git is requested from the SSH host (the same split as `ssh://…`, where the app opens the SSH host directly). If the connection has dropped, it reconnects the next time it is opened. In the CLI, the first connection takes 1.6 seconds, and 0.01 seconds from the second time (measured with a fake ssh treating the local machine as the SSH host).
  - Unix domain sockets rather than XPC: the same thing runs on Linux (also used by the VSCode extension and the CLI), and the round-trip delay is small enough, p50 0.08 ms for small requests (0.04 ms in the same process). Large data is not put on the socket but passed via file mmap and shared memory, so XPC's region handoff offers no advantage either.
  - The original plan (send requests and notifications over shared-memory rings too, waking with `os_sync_wake_by_address_any` only when asleep) was not built, because the measured socket delay turned out not to be a problem.
- ③ SSH: the daemon (`amonhen --stdio <repo>`, `crates/daemon`) only executes `GitBackend`, `Operations` and `RepoFiles` on request on the SSH host, while layout, persistent cache, diffs and highlighting are done in the local core (`RemoteBackend` in `crates/remote`). The C ABI and FE can be used as is. The UI reads only local memory, and only the parts that touch git go over the wire. History keeps its persistent cache locally, and after reconnecting it receives only the new commits since the previous starting points (the daemon finds them with `git rev-list`). Requests are numbered and sent concurrently, and path and content searches can be cancelled.
- In every mode, the UI reads only local memory. Communication is used only for background synchronization.

### Commit signature verification

- Whether there is a signature and which kind (GPG, SSH, X.509) can be told from `gpgsig` in the commit header, so it is taken together when reading History rows (`CommitText::signature`, the chunk's `signatures` column). gpg is not used.
- The verification result (the single character of `git log --format=%G?`) is left to git. It launches gpg once per commit, so each takes about 15 ms.
- Verification does not let gpg go fetch keys. With gpg's `auto-key-retrieve` enabled, every time a signature from an unknown key is verified it queries key servers and WKD and adds the key to the keyring (verifying hundreds in the background, going out to the network and growing the user's keyring without asking). git has no setting to add arguments to gpg, so a script that calls the configured gpg with `--no-auto-key-retrieve` is placed in a temporary folder, and only for verification `-c gpg.program=` points to it. Signatures from unknown keys become "No key".
  - History: only visible signed rows are verified, 0.15 seconds after scrolling stops, 8 at a time in a single git call (`git log --no-walk=unsorted --format=%H%x00%G?%x00%GS%x00%GK%x00%GF%x00%GT --stdin`; valid signatures are returned with the committer's email attached). Icons are replaced as results come in. Drawing only reads remembered results and never waits for gpg. Icons are drawn by putting SF Symbols into Metal's text atlas.
  - Details pane: the signer, key and trust level are read in the background after the header is shown (`commitExtras`).
- Where to verify:
  - Local repositories: the local git and keys (`~/.gnupg`, `gpg.ssh.allowedSignersFile`, `gpg.minTrustLevel`, etc.).
  - Repositories on an SSH host: chosen in settings (local by default).
    - Local (default): receive the commit object (a few KB), write it to a local scratch bare repository (`$TMPDIR/amonhen-verify.git`), and have the local git verify it with `%G?`. Trees and other objects are not needed. The rules, keys and trust levels are the same as for local repositories. If the written object's ID differs from the original ID, it is rejected.
    - SSH host: send a request to the daemon and verify with the SSH host's git and keys. If the SSH host has no public key, the result is "cannot verify".
  - The trust level is "how much I trust that key", so judging by the local keyring is the default.
- The presentation (`SignatureStatus`) is decided from `%G?` plus the trust level (`%GT`) and "whether the signer's email address matches the committer". Wording is English, matching GitHub.

  | State | Icon (SF Symbols) | Color | Wording | Trust scale |
  | --- | --- | --- | --- | --- |
  | G, trust ultimate / full (for SSH, if in allowed signers) | checkmark.seal.fill | Green | Verified | 3 |
  | G, trust marginal | checkmark.seal | Green | Verified | 2 |
  | U, or trust undefined / never | checkmark.shield | Yellow | Untrusted key | 1 |
  | G, but none of the key's email addresses match the committer | person.crop.circle.badge.questionmark | Yellow | Unverified signer | — |
  | X, Y | clock.badge.exclamationmark | Orange | Expired | — |
  | R | exclamationmark.shield.fill | Red | Revoked key | — |
  | B | xmark.seal.fill | Red | Bad signature | — |
  | E | questionmark.circle | Gray | No key | — |
  | Verifying | seal | Light gray | Checking… | — |

  Whether the signer matches the committer is decided by all the email addresses registered on the signing key, not by git's `%GS` (only the key's primary user ID). For GPG, `gpg --list-keys --with-colons` is looked up by the primary key's fingerprint (`%GP`) (gpg runs once no matter how many keys), and the validity of each user ID is read too. For SSH, the principal git found in allowed signers (`%GS`) is used. The popover's "Committer email" line shows whether the committer's email address is registered on the key: if it is on a valid user ID (u, f, m), "registered on the key"; if only on user IDs of unknown validity, "registered on the key, but that user ID is not verified" (since anyone can write someone else's address into a user ID); if only on revoked or expired ones, or nowhere, "not registered on the key"; if the key's email addresses cannot be read, "cannot be checked".

  The Signature line in the details pane shows the icon, wording, scale, method and signer. Hovering shows a short explanation; clicking shows a popover (explanation, signer, match with the committer, method, key ID, fingerprint split into groups of 4 characters, trust level, where it was verified). History rows show only the icon (the trust level can be told from the icon's shape).
- Results are remembered per commit by the core (the local `Repository`) (since commits do not change). They are discarded when the app quits.
- When the verifying side is changed, all remembered results are discarded. Keys and trust levels differ, so results are not carried over. The core advances a counter and does not remember results of verifications started before the change, even if they arrive later. The History dots and the details pane also discard results and verify again.

## Multi-platform

### GitBackend

`git` cannot be launched on iOS / Android, so git operations are abstracted behind a trait and swapped.

| Implementation | Use |
| --- | --- |
| `GixBackend` | Reading on all OSes. Writing is also moved over, starting with the operations gix supports |
| `CliBackend` | Writing on desktop (rebase, merge, push). Correctness first |
| `Git2Backend` (libgit2) | Fills in the writes gix does not yet support on mobile (merge, push, etc.) |

Non-interactive rebase is also implemented per backend: on desktop by injecting the todo through `GIT_SEQUENCE_EDITOR`, on mobile by re-stacking commits with gix / libgit2.

### Platform

Implemented by the FE and passed to the core.

| Feature | macOS / iOS | Windows | Linux | Android |
| --- | --- | --- | --- | --- |
| Waking the main thread | DispatchSource | `PostMessage` | GLib main loop | Looper / Handler |
| File watching (notify) | FSEvents | `ReadDirectoryChangesW` | inotify | inotify |
| Credentials | Keychain | Credential Manager | libsecret | Android Keystore |
| SSH | The OS's ssh / russh (iOS) | The OS's ssh | The OS's ssh | russh |
| Storage location | `~/Library/Caches/<bundle ID>` etc. | `%LOCALAPPDATA%` | `$XDG_CACHE_HOME` | The app's cache directory |

### Bindings

- The C ABI (header generated with `cbindgen`) is the foundation for everything. Paths that need speed (row data, graph, notifications) go through it.
- UniFFI generates typed APIs for Swift (macOS / iOS) and Kotlin (Android).
- Linux (gtk-rs) and Windows (if the UI is also Rust with windows-rs) use it directly from Rust.

## History graph

### Layout (Rust)

- Read parents and generation numbers from commit-graph. Messages and authors are fetched later, only for visible rows.
- Lay out incrementally from newest, and display as soon as the first screenful is ready. Every 4096 rows, the lane state is saved as a checkpoint.
- When new commits are added, recompute from the top and stop once the state matches a checkpoint.
- The first-parent trunk is fixed to lane 0. Colors are decided by the branch's origin rather than the lane number, so they do not change with scrolling or recomputation.
- Long lines are not split per row but held as a single span (`row_start..row_end`). Lines in the visible range are looked up in O(log n) with a span index.

### Drawing (Metal)

- Dots, lines, curves and label backgrounds are drawn as quad instances in a single draw call. Shapes are drawn with SDFs in the fragment shader and antialiased analytically.
- Instances for the visible range ± a few screens are kept on the GPU, and scrolling only changes the offset in the constant buffer.
- `CAMetalLayer.maximumDrawableCount = 2`. On input, draw and present without waiting for the next vsync.

### Input

- Receive `scrollWheel` directly, and draw at the position predicted for display time from the event timestamp and velocity. Momentum follows `momentumPhase`.
- Hit testing is O(1): row = `y / row height`, lane = `x / lane width`.

### Presentation refinements

- Draw the trunk thick and branches thin.
- Emphasize the ancestor path of the hovered commit and fade the rest (ancestry is computed in Rust as a bit set and passed to the GPU as a single texture).
- A mode that focuses on a branch, folding of merged branches.
- When there are many lanes, compact unused lanes and widen the area around the hover position with a fisheye.
- Show uncommitted changes as a dotted virtual node at the top, stashes with a dedicated shape, and ahead / behind spans with distinct colors.
- When the layout changes, match old and new by commit ID and animate by interpolating positions in the vertex shader.
- Draw the graph's density (commit frequency, branch activity) on the scroll bar.

## Diff view

- Do not use `NSTextView`; draw with Metal from the start. The text drawing machinery is shared with History labels.
  1. Leave text layout to Core Text's CTLine (Japanese, ligatures and font fallback at the same quality as the OS).
  2. Extract glyph IDs, positions and fonts from CTRun.
  3. Rasterize only unregistered glyphs with `CTFontDrawGlyphs` and add them to the atlas (the same rendering as the OS, so the look matches). Emoji go into a separate color atlas.
  4. Draw each glyph as one quad instance, in a single draw call. Horizontal positions are rounded to 1/4 pixel.
- CTLines are cached by "generation + line number", and scrolling, as in History, only changes a constant offset.
- File contents share the `Arc<[u8]>` decoded by gix across diff computation, highlighting and display.
- Text selection, copy, search, the right-click menu and hunk / line-level staging are implemented by our own code.

## Speed targets and measurement

| Item | Target |
| --- | --- |
| Launch and show History | A few ms (with persistent cache) |
| Full layout of 1.3M commits (linux) | Around 100 ms in the background |
| Data transfer to the GPU while scrolling | One constant |
| Draw calls per frame | 1 for History |
| From input until the GPU finishes drawing | Within 8.3 ms (display is a further 2 frames later due to WindowServer compositing) |
| Cost of the Rust–Swift boundary | Under a µs per frame |

- Measurement uses criterion for Rust and `os_signpost` + Instruments for Swift.
- Always measure from the input event timestamp to the display time from `MTLDrawable.addPresentedHandler`, and show it in the debug display.
- CI uses linux- and chromium-scale repositories to detect regressions in launch time, dropped frames and input latency.
- Confirm that `CFString`'s NoCopy really does not copy. `apps/macos/Tests` tests that for UTF-16 `CFStringGetCharactersPtr` returns the original array, and for ASCII that rewriting the original buffer changes the contents too. Short ASCII strings are turned into tagged pointers (characters embedded in the pointer value) by CoreFoundation, so they do not reference the buffer, but there is no heap allocation either, so this is fine.

### Measurement log

linux (1,485,021 commits, 952 refs, Apple Silicon, release build):

| Item | Time |
| --- | --- |
| Loading History (without commit-graph) | 2.5 s (`git rev-list --all --topo-order` takes 11 s) |
| Same (with commit-graph, measured on a copy made with `git clone --shared`) | 0.97 s (git takes 2.1 s) |
| Graph layout (671 lanes) | 0.23 s |
| Reload when starting points are unchanged | 9 ms |
| Reload after adding 1 commit | 139 ms |
| Open in the app and show History (with cache, 102 MB) | 18 ms |
| Same (without cache) | 2.4 s |
| Main-thread work while scrolling p50 / p95 | 2.0 / 2.6 ms (0 dropped frames) |

What was fixed to get here:

- gix's topological-order walk read every commit twice, decoding objects again on the second pass (28 s). Now each commit is read only once (using commit-graph if present), commits at the same depth are read in parallel, and sorting is done in memory (`model::topo`).
- Sorting looked up a hash table three times, and with a 1.5M-entry table cache misses were heavy (0.91 s). Now the first 8 bytes of the object ID are used directly as the hash value (`OidMap`), and parents are converted to indices only once (0.22 s).
- Layout scanned all lanes on every row (1.0 s with 821 lanes). Now it keeps an index from awaited rows to lanes and a set of free lanes (0.18–0.23 s).
- When the starting points changed, the whole history was reloaded. Now it stops when it hits the previous History and adds only the new commits (`model::topo::extend`).

The 2.4 s for the first load without cache becomes about 1.2 s with commit-graph. Whether to run `git commit-graph write --reachable` in the background in repositories without commit-graph is chosen in settings (`writeCommitGraph`, off by default, since it writes into the repository's `.git`).

Loading History (collecting starting points + topological-order walk, `crates/backend-gix/examples/walk.rs`, Apple Silicon, release build):

| Repository | Commits | gix | `git rev-list --all --topo-order` |
| --- | --- | --- | --- |
| android/development (without commit-graph) | 34,261 | 267 ms | 286 ms |
| Same (with commit-graph) | 34,261 | 31 ms | 111 ms |
| coreutils (without commit-graph) | 30,176 | 157 ms | 225 ms |

- Without gix's object cache enabled, it takes 1.3 seconds without commit-graph (64 MB set with `object_cache_size_if_unset`).
- With or without commit-graph differs by an order of magnitude. For repositories without commit-graph, suggest writing it, or make up for it with the core's own persistent cache.

Graph layout (`crates/core/benches/graph.rs`, synthetic history, criterion):

| Item | Time |
| --- | --- |
| Full layout of 100k commits | 9.4 ms |
| Full layout of 1M commits | 90 ms |
| First 60 rows of 1M commits | 1.3 ms |
| Recompute after adding 10 commits to 1M commits | 6.9 ms |
| Searching lines overlapping the visible 60 rows | 0.9 µs |

Scrolling the History screen (`AmonHen <repo> --measure-scroll`, 600 frames, 1.5 rows each, 60Hz display, release build):

| Repository | Main-thread work p50 / p95 | Until the GPU finishes drawing p50 / p95 | Until display | Dropped frames |
| --- | --- | --- | --- | --- |
| coreutils | 1.9 / 2.5 ms | 2.2 / 2.8 ms | 33.3 ms | 0 |
| android/development | 2.2 / 4.2 ms | 2.3 / 4.2 ms | 33.3 ms | 0 |

- Display takes 2 frames. In windowed display, WindowServer composites at the next vsync and it reaches the screen at the vsync after that, so the app side cannot shorten it. It did not change with or without `presentsWithTransaction` either, so the enabled setting is used, which lets the table and graph move in the same frame.
- The maximum (8–12 ms) was the moment of synchronously reading subjects for a new chunk. Reading in the background and prefetching the neighboring chunks brought it to p95 2.0 ms and max 2.6 ms.
- On linux (1.4M commits), with a 120Hz display and fast scrolling of 40 rows per frame, CPU drawing of the table cells (NSTextField) took about 60% of the CA commit, and only 532 of 600 frames were displayed (49 dropped frames). Using 3 drawables and drawing subject, author and date with Metal like the graph (the glyph atlas shared with the diff view, with per-row laid-out glyphs cached) gave 599 frames displayed (1 dropped frame), with main-thread work at p50 2.1 / p95 2.5 ms.
- Graph dots and lines are written into a GPU buffer every frame, only for the visible range (a few KB). Aligning the layout arrays to page boundaries, passing them with `MTLBuffer(bytesNoCopy:)` and changing only a constant while scrolling is for later.

The FE–core boundary (breakdown of `--measure-scroll`, `--measure-select N`, linux, release build):

| Item | Before | After |
| --- | --- | --- |
| Main thread during fast scrolling (40 rows/frame) p50 / p95 | 3.35 / 3.93 ms | 1.09 / 1.72 ms |
| From selecting a commit until the diff appears p50 / p95 (to distant rows of history) | 24.4 / 46.9 ms | 8.1 / 15.4 ms |
| From selecting a commit until the message appears p50 / p95 | 12.4 / 39.2 ms | 6.9 / 11.1 ms |

- Core calls (line search, lane positions, commit IDs) total 0.03 ms or less per frame. What was heavy was laying out rows with Core Text on the FE side; row text is now laid out on the same thread when the chunk is read in the background, and the main thread only looks up the atlas.
- Loading commit details itself (changed files 1.1 ms, diff 0.7 ms) is light; the delay was waiting. It no longer rereads refs from disk for each detail view (15–30 ms), prefetch chunks and highlighting have lower priority than details, loading starts without waiting for the main thread to draw, and the first file's diff is read together with the changed files.

Over the daemon (`cargo run --release -p amonhen-remote --example daemon_cost -- <amonhend> <repo>`, connected to a local linux as a child process; excludes SSH latency and bandwidth):

| Item | Time | Amount received |
| --- | --- | --- |
| First load of History | 2.4 s | 54 MB (local cache afterwards) |
| Reloading History (no change) | 29 ms | 19 KB |
| Chunk of subjects etc. (256 rows) | 3 ms | 502 KB |
| Changed files and the first diff | 3 ms | 127 KB |
| Subject search | 1.4 s | 1.37 GB |

- After reducing transfer (size on the wire): chunk 502 KB → 13 KB, changed files and the first diff 127 KB → 56 KB, subject search 1.37 GB → 5 MB. The first History load is 47 MB (commit IDs barely shrink), and afterwards only the difference from the local cache.
- Compression is LZ4 (`lz4_flex`, written purely in Rust) rather than zstd. zstd contains C code, which would require a C compiler for type-checking all targets and for static linking on Linux.
- The engine's location has the same shape on every machine: `${XDG_DATA_HOME:-~/.local/share}/amonhen/engine/<content hash>/amonhen` (versions can sit side by side, and the neighboring `engines` also holds the ones for other OSes and CPUs to send to SSH hosts), `current` points to the newest version, and `~/.local/bin/amonhen` points to `current/amonhen`. The app (in the background at every launch), `scripts/install-app.sh`, and people who extracted the tar.gz place themselves with `amonhen install`, repointing `current` if they are not older than the current `current` (`0.1.0` < `0.1.1-snapshot.N` < `0.1.1`). The shared daemon is started with the one `amonhen engine` prints (`current` if its protocol version is the same, otherwise itself), so whichever FE starts first, it ends up as the same daemon (`crates/cli/src/install.rs`). Development builds of the app do not place it.
- What runs on the SSH host is the engine (`amonhen`, a single executable serving as both CLI and daemon); for x86_64 and aarch64 Linux it is statically linked with musl (`scripts/build-engine.sh`, linked with zig, 5–6 MB). The order of use is the one specified by `AMONHEN_REMOTE_ENGINE` → `amonhen` on the SSH host's `PATH` (if the protocol version matches) → one sent earlier → push one (stopped with `AMONHEN_PUSH=0`). It is sent to `~/.local/share/amonhen/engine/<content hash>/amonhen` (the same shape as the local location), and after writing, its contents are checked on the SSH host with `sha1sum` or `shasum`. Other versions are not deleted (so different versions of Amon Hen using the same SSH host do not fight), the `used` mark is updated on each use, and only those unused for 30 days are cleaned up. The engine in use and how it was prepared are shown in `amonhen daemon status` and Activity (the same approach as VSCode's Remote-SSH and Zed's remote development).

Syntax highlighting (`cargo run --release -p amonhen-syntax --example highlight -- <file>`, files from linux):

| File | Size | Time |
| --- | --- | --- |
| kernel/fork.c | 3,431 lines, 84 KB | 16 ms |
| kernel/workqueue.c | 8,644 lines, 245 KB | 29 ms |
| drivers/gpu/drm/amd/amdgpu/gfx_v10_0.c | 10,253 lines, 451 KB | 58 ms |

- The first time for each language adds 8–14 ms for loading queries.
- Diffs are shown without colors first, and redrawn after the whole before and after files are parsed in the background. Since the whole file is parsed rather than fragments, comments and strings continuing from outside the hunk are colored correctly too.

Memory for History row text (subject, author, date):

- Read in chunks of 256 rows, and keep only the 24 (about 6,000 rows) closest to the visible area. Distant ones are discarded and reread when scrolled back to (a few ms per chunk, in the background). Chunks point into the contents of commit objects, so holding all of them would be several GB on linux.
- Physical memory (`footprint`) while scrolling linux quickly from top to bottom kept growing to 4 GB in 8 seconds without discarding, but with discarding it settles at 300–380 MB. Scrolling speed does not change (main thread p50 1.14 ms).

Persistent cache (`crates/core/src/cache.rs`):

| Item | Time |
| --- | --- |
| Writing 1M commits | 19 ms |
| Reading 1M commits (mmap + integrity check + building the line index) | 8.1 ms |
| Open android/development and show History (with cache) | 3–4 ms |
| Same (without cache, read and shown in the background) | About 400 ms |

- If the starting points and HEAD are the same as last time, neither the walk nor the layout runs (commits are immutable, so History is the same too).
- Most of the load time is the check and building the index. mmap itself is nearly 0. If needed, the index can go into the cache too.

Working Copy status (`crates/core/examples/status.rs`, second run onward):

| Repository | Plain git status | untracked cache | + fsmonitor |
| --- | --- | --- | --- |
| linux (96,080 files, local macOS) | About 500 ms | 125 ms | 45 ms |
| minix (73,545 files, Linux on an SSH host, over the daemon) | 115 ms | 32 ms | Not available |

- Most of the time goes to finding untracked files (373 ms on linux). The untracked cache is not used unless `status.showUntrackedFiles` agrees with `--untracked-files=all`, so both are passed.
- A normal status does not take the index lock (so it does not fight with the user's git), so the cache is not written back. It is written back the first time, and after that only once every 60 seconds.
- fsmonitor is macOS and Windows only, and `git fsmonitor--daemon` stays resident per repository. It can be turned off in the app's settings (on by default).
- Reloads do not run overlapping. If a change notice arrives midway, it reloads only once after finishing.

Ancestor emphasis (`crates/core/src/lineage.rs`, `crates/core/examples/lineage.rs`):

- On linux (1.48M commits, 1.74M lines), finding the ancestors of the selected commit and the lines to light up takes 9–25 ms. It is computed in the background when a commit is selected, and redrawn once found. It is too heavy to compute on every hover, so it is done only on selection.
- Lines hold the child's and parent's rows (`Edge::child`, `Edge::parent`; merging lines are `Edge::RIDER`). A line grew from 16 bytes to 24 bytes, and the linux cache from 103 MB to 117 MB (format 2). Read and write times barely changed.
- Lines waiting for the same parent share the vertical line of the lane's owner. If only the merging child is an ancestor, only the part of the vertical line below the merge point lights up (split in two when drawing).
- Only lines in the visible range are drawn, so the main-thread time for fast scrolling does not change (p50 1.07 ms).

On linux (1.48M commits), the median number of lines crossing a row is 365, and the median maximum lane number is 452 (`crates/core/examples/layout.rs`). Even in topological order (the same ordering as `git log --topo-order`) it is 303 and 637, so ordering does not solve it. Instead, only lanes with dots in the visible range get full width, lanes with only lines are packed into thin, faint bands, and empty lanes get width 0 (fisheye). Lane movement when the range changes is bridged with an animation of about 0.15 seconds. Each row's text starts right after that row's graph (the dot and the lines passing the row, from the positions drawn that frame), so the left edge of the text is ragged and never far from the graph. The graph column itself is only as wide as the visible lanes need (up to 120 pt); it widens at the same speed as the lanes and narrows slowly, so the header does not sway while scrolling.

In real repositories (`crates/core/examples/layout.rs`), coreutils (30,176 commits) takes 1.2 ms and android/development (34,261 commits, 318 starting points) 17 ms. The latter has many release branches and spreads to 673 lanes, so presentation refinements such as compacting unused lanes and widening with a fisheye (step 13 of the implementation order) are needed.

## Distribution

- Put the arm64 `.app` on GitHub Releases. As with the extension, pushing a `v*` tag makes GitHub Actions build and attach it, and pushing to main recreates the `snapshot` pre-release.
- Signing uses a self-signed code signing certificate. With ad-hoc signing the identity changes with every build, and macOS permissions (folder access, keychain, etc.) are reset on every update. The certificate and private key are kept in GitHub Actions secrets.
- The app name is `Amon Hen` and the bundle ID is `io.github.fukusaka.amonhen`. The bundle ID is what settings, caches and permissions are tied to, so it does not change. The snapshot build is `Amon Hen Snapshot` (`io.github.fukusaka.amonhen.snapshot`), so it can be installed alongside the stable release.
- The `.app` is assembled with `scripts/build-app.sh [stable|snapshot]` (zip in `dist/`). The certificate is imported from the secrets `AMONHEN_CERT_P12`, `AMONHEN_CERT_PASSWORD` and `AMONHEN_CERT_NAME` into a temporary keychain in CI (`scripts/import-signing-certificate.sh`).
- Install and update with `scripts/install-app.sh [--user|--system|--dir <folder>] [stable|snapshot]`. It fetches with `curl` (no `gh` needed; also runnable as `curl … | sh` without a clone) and places it in `/Applications` (`--user` for `~/Applications`, `--dir` for any folder; without any of them, where it is already installed). No quarantine attribute is attached, and the app is notarized anyway.
- Git's environment: an app launched from the Finder or the Dock does not get the terminal's environment (launchd's PATH is `/usr/bin:/bin:/usr/sbin:/sbin`, `SSH_AUTH_SOCK` is the system agent, no `GIT_*`), so Homebrew's git, credential helpers and agents such as 1Password were not found and fetch, pull and push failed where they worked in a terminal. At launch the app runs the login shell once (`$SHELL -ilc 'env -0'`, 3-second limit; `AMONHEN_NO_SHELL_ENV` turns it off) and takes `PATH` (prepended), `SSH_AUTH_SOCK`, `SSH_AGENT_PID`, `GIT_*`, `HOMEBREW_*` and `XDG_*` into its own environment, which git, the shared daemon and gh inherit; the result is cached in the defaults so later launches apply it at once and refresh it in the background (`ShellEnvironment`; Settings > Advanced > Terminal Environment shows what was taken, with Read Again and a switch; `AmonHen --print-environment` prints it). Git and ssh still have no terminal to ask for a password or a passphrase, so the app points `GIT_ASKPASS` and `SSH_ASKPASS` (with `SSH_ASKPASS_REQUIRE=prefer`) at `Contents/Resources/amonhen-askpass`, a script that asks in a dialog (passed through `AMONHEN_ASKPASS`, read by `crates/backend-cli` when it runs git).
- Developer ID signing and notarization: if the `release` environment's secrets hold a Developer ID Application certificate (`AMONHEN_CERT_P12`, `AMONHEN_CERT_PASSWORD`, `AMONHEN_CERT_NAME`) and an App Store Connect API key (`AMONHEN_NOTARY_KEY`, `AMONHEN_NOTARY_KEY_ID`, `AMONHEN_NOTARY_ISSUER`), the snapshot and stable release CI sign the app and the engine with the hardened runtime and a timestamp, and the stable release CI also submits them for notarization (snapshots are for my own use and installed with curl, which attaches no quarantine mark, so they are not notarized; `scripts/build-app.sh`, `scripts/notarize.sh`). The result is stapled to the app; the standalone engine is only submitted for notarization (the Mac checks it the first time it runs). Without them, builds stay ad-hoc (fetching with `gh` or `curl` attaches no quarantine mark, so they run as is with `scripts/install-app.sh`). Since the repository is public, only the signing job reads the `release` environment's secrets, and third-party Actions in the release CI are pinned by commit hash. Secrets are not passed to pull requests from forks. Auto-update (Sparkle) will be considered if I ever want to distribute to others.

## Relationship to the existing extension

- The extension is maintained as is. Standalone is built as a separate product.
- The design ideas (fewer spawns, partial fetching, optimistic updates, the Undo operation log, generating the todo for non-interactive rebase) carry over as is. The non-interactive rebase procedure (vscode-amonhen's `src/history/rebasePlan.ts`) is ported to Rust.
- Once the daemon exists, consider replacing the extension's git execution layer and state management layer with a connection to the daemon (with Remote-SSH, launch `amonhend --stdio` on the SSH host).

## Risks and validation

| Risk | How to validate |
| --- | --- |
| gix cannot keep up with writes and special configurations (reftable, sparse checkout, partial clone, etc.) | Only reading uses gix, writing uses git. Keep a path that falls back to spawning git when something cannot be read |
| Building Japanese input, selection and accessibility into a custom diff view is heavy | Input fields use the standard `NSTextView`. The diff view is read-only, and `NSAccessibility` is provided per line |
| Having both UniFFI and the C ABI makes the boundary complex | Move everything but row data to UniFFI; the C ABI is only for fetching History, diff and status rows |
| More projection formats mean more test combinations | Prepare a test per format that "converts from the internal model → converts back and compares". The prototype implements only the macOS format |
| On mobile, writes with gix and libgit2 diverge from git | Prepare tests comparing the same operations against CliBackend. Unsupported operations are not offered on mobile |
| Memory safety with shared memory and ring buffers | Wrap the Swift side in APIs with limited ranges, and test the boundaries on the Rust side with Miri and AddressSanitizer |
| Machines older than macOS 14.4 | Out of scope |

## Implementation order

Prototype the heaviest part, History, first, and confirm the boundary design and the speed targets can be met before expanding.

Prototype:

- [x] 1. The Rust workspace and the skeleton of `model` / `core` / `layout`, the shapes of `GitBackend` / `Platform` / `FrontendProfile`, reading commit-graph with gix. CI builds all targets
- [x] 2. Graph layout (incremental computation, checkpoints, span representation) and criterion benchmarks
- [x] 3. Projection (macOS format only), publishing snapshots over the C ABI, zero-copy reading from Swift
- [x] 4. AppKit History (`NSTableView`) and Metal graph drawing, measurement from input to display
- [x] 5. Validate the speed targets on the linux repository. If they cannot be met, revisit the design

Main body:

- [x] 6. Persistent cache (mmap) and immediate display right after launch
- [x] 7. Sidebar (Branches / Remotes / Tags / Stashes / Submodules) and change detection with FSEvents
- [x] 8. Diff view (Metal, glyph atlas shared with History, CTLine cache) and deferred highlighting (tree-sitter, `crates/syntax`). History's subject, author and date are also drawn with Metal. Selecting, copying and searching text in diffs
- [x] 9. Working Copy: status, file / hunk / line-level staging, commit input
- [x] 10. Write operations (checkout, merge, rebase, push, fetch) and the Undo operation log
- [x] 11. Commit editing (reword / squash / fixup / drop / reorder / split) and drag and drop
- [x] 12. 3-way merge (resolving per conflict and per file, 3-pane merge editor)
- [x] 13. Graph presentation refinements (ancestor emphasis, folding, fisheye, animation, density display). Fisheye (`crates/core/src/lanes.rs`), animation of lane movement, ancestor emphasis for the selected commit (`crates/core/src/lineage.rs`), folding of merged branches (`crates/core/src/fold.rs`; keeps the trunk followed from each tip through first parents only, and shows the number of commits on the merged side on the merge row; opened per merge with right-click; View > Fold Merged Branches), and the density band at History's right edge (commit frequency and branch activity per span, a frame for the visible range, clicking jumps there) are also done. On linux (1.48M commits), folding and layout take 47 ms (down to 77k rows) and density 10 ms (`crates/core/examples/fold.rs`)
- [x] 14. Distribution (GitHub Actions, snapshot, self-signing, install.sh)

Extensions:

- [x] 15. CLI (`amonhen`, `crates/cli`). Uses the core without a UI. `amonhen [<path>]` and `amonhen open <path|ssh://…>` open in an app tab (the app accepts folders and `amonhen://open?location=…`), `amonhen log` (a box-drawing graph from the core's layout; only lanes with dots or curves get a column, and lanes that lines merely pass through are collapsed into ┆; `--fold`, `--branch`, search syntax), `amonhen undo` (`--list`, `--peek`; the same operation log as the app), `amonhen merge-file` (Magic Merge; exits with code 1 if anything remains, so it can be used as git's mergetool). History starts from the app's persistent cache, so log takes 85 ms even on linux (1.48M commits). `scripts/install-app.sh` creates a link at `~/.local/bin/amonhen`. Everything the core can do is available from the CLI (the path the VSCode extension and other FEs will take is first tried out with the CLI): `status`, `diff` (unstaged, `--staged`, `--combined` with per-line stage marks, one or two commits), `show`, `stage`/`unstage` (`--rows` for individual diff lines), `discard` (also per line; can be undone), `commit`, `refs`, `watch` (stays connected and receives change notices), `clone`, `run` (`run_json` commands as is; without arguments it answers line by line to one command per line on standard input, a prototype of the extension using the CLI as its front API). All of them output the shape FEs read with `--json`. `crates/cli/tests/cli.rs` runs the same commands through the three ways of opening — directly in-process, `daemon:` and `ssh://` (fake ssh) — and confirms they behave the same
- [x] 16. Local shared daemon (originally planned as XPC + shared memory; went with a Unix domain socket + shared memory, for the reasons in "When moving to another process or remote"). Proceeds in stages: ① A shared daemon resident on the local machine (`amonhend --serve`, `crates/daemon/src/serve.rs`). One per user, listening on a Unix domain socket (`$TMPDIR/amonhend-<protocol version>.sock`, 0600), and opening the repository at the path the client sends first. The `GitBackend` and watching for the same repository are shared among connected clients, and changes are notified to everyone. It exits after 10 minutes with nobody connected, and is started again when needed. Communication uses the same frames as SSH, without compression. The app's `daemon:/path` and the CLI's `-C daemon:/path` use this. Opening local repositories through the daemon: a toggle in Settings > Advanced (always), Open via Daemon on the repository list's right-click menu (just one), `amonhen open --daemon`. Checking its state: `amonhen daemon status [--json]`, `start`, `stop` (if the first thing sent on connecting is `\0status` or `\0stop`, it becomes an instruction to the daemon rather than a repository), and the Daemon tab in Window > Activity (the daemon's process, the connections per open repository, History, watching, search index, and the bytes sent and received on this app's connection to the daemon). On linux, `amonhen log` from the second time is 0.09 seconds, the same as in-process (the first time is 1.7 seconds for starting the daemon and handing over all commits) — done. The round-trip delay is p50 0.08 ms for small requests (0.04 ms in-process), and a 256-row chunk is the same 2 ms (`crates/daemon/examples/latency.rs`), so moving logic into the daemon does not make it slower, and it is moved over step by step as designed. ② Building History (loading, layout) in the daemon: the daemon builds only one per repository and writes it to the persistent cache file, returning its location to the connecting side (`Request::PreparedHistory`, `GitBackend::prepared_history`). The connecting side does not compute the layout but mmaps the file, so the contents are shared through the page cache without copying (effectively shared memory). Rewrites are renames, so anything being read is not broken. The FE needs no changes. On linux, `amonhen log` is 0.12 seconds from the second time (0.09 seconds in-process), 0.17 seconds after the daemon restarts, and 2.3 seconds the first time (the daemon builds and writes it) — done. ③ Moving operations into the daemon: `run_json` commands (operations, Undo, settings, Magic Merge, etc.) and Working Copy operations that write Undo records (commit, discarding changes) run in the daemon's core when connected to the local shared daemon (`GitBackend::delegate`, `Request::Delegate`, `crates/core/src/delegate.rs`). Operations and Undo records end up in one place (two cores do not separately rewrite the same record file). Per-line staging writes no record and its git work already runs in the daemon, so it is not delegated. Only the commands that read or undo the daemon's records run locally — done. Opening local repositories through the daemon is currently chosen in Settings (off by default). Diffs are also computed in the daemon and returned in a single round trip, with file contents passed in shared memory (`crates/remote/src/shm.rs`; the daemon lays them out in one POSIX shared memory region, and only the region's name and each part's position are sent over the socket; the receiving side only reads and maps it, using it as `amonhen_layout::Blob` without copying all the way to display). On linux p50 1.6 ms, p95 6.3 ms, max 17 ms (in-process 1.4, 5.3, 15 ms; the earlier form that fetched contents per file 1.7, 8.3, 27 ms; the form that sent contents over the socket 1.6, 9.2, 34 ms) — done. Working Copy status already runs git in the daemon, so there is no difference (124 ms). History search (subject, author name, ID) reads everything into an index (`amonhen_model::search::TextIndex`) the first time subjects and author names are searched, and from then on only searches it (rereading from git every time was most of the slowness). When commits are added, it carries over from the previous index and reads only the added ones. The local core, the SSH host's daemon and the shared daemon use the same thing; in the shared daemon the index and counted commits are shared among clients of the same repository and kept for 10 minutes even after the last client leaves. On linux, a single search went from 1.1 seconds → 6–31 ms (1.4 seconds the first time to build the index; the index is about 100 MB), and `amonhen log -C daemon:… <query>` is 0.1 seconds from the second time (`crates/core/examples/search.rs`). SQLite full-text search (FTS5 trigram) was also compared, but it was faster only for rare terms (0.5 ms), and the index was 950 MB and took 22 seconds to build, so it was not adopted. Moving between matching commits uses ↩ / ⇧↩ (History and the toolbar search field), ⌘G / ⌘⇧G (from any field; when searching within the diff or details pane, there instead), and ∧ ∨ next to the count. A toggle next to the count shows only matching commits (`CommitGraph::retain_linked`; non-matching commits are skipped, and lines follow first parents to connect to the next matching commit; on linux, laying out the 249k matches for fix takes 49 ms). Rebuilding History: Repository > Rebuild History, `amonhen log --rebuild`, `amonhen daemon rebuild [<path>|ssh://…]` (`Repository::rebuild_history`; discards the persistent cache and builds without carrying over from the previous History; through the shared daemon, it has the daemon's core rebuild and rereads that file)
- [x] 17. Daemon over SSH and a local replica. Stages A0 (core file operations into `RepoFiles`), A (`amonhen-remote` and `amonhend`, checked with a local child process), B (opening from the app with `daemon:/path`; amonhend bundled with the app), C (with `ssh://host/path`, push an amonhend matching the SSH host's OS and CPU to `~/.cache/amonhen/` and launch it; chunks carry only subjects etc., subject search on the daemon side, large frames compressed with LZ4) and D ("Add Remote…" in the repository list, a connecting window while connecting, the amount received for the first load, a band and "Reconnect" when disconnected) are done. Confirmed it can open a real SSH host (Linux x86_64) too
- [x] 18. Using the daemon from the VSCode extension. To do first: the CLI's `log --json` and `diff --json` turn large data into JSON and do not go the way the extension is meant to (reading `repr(C)` records and string tables without copying through an `ArrayBuffer` or WASM memory). Add an outlet to the CLI that outputs records as is (e.g. `amonhen log --records <file>`, a profile for VSCode), compare with JSON on linux (writing, size, and time and memory to read in Node), and then decide how to connect (via the CLI, napi, WASM). Measurement (linux's 1.48M commits, `crates/core/examples/transport.rs` and Node 26): for one screenful (256 rows of dots, lines, IDs, subjects, authors, dates), JSON takes 0.51 ms to build, 82 KB, 0.18 ms to read in Node; records take 0.013 ms to build, 54 KB, 0.16 ms to read (mostly turning into strings), so either is enough. For all rows (dots, lines, IDs), JSON takes 1.1 seconds to build, 137 MB, 0.53 seconds to read, heap +386 MB; records take 7.8 ms to build, 89 MB, 0.001 ms to read (just viewing through typed arrays). Handing to another thread (the same structured clone as the webview's postMessage) takes, for all rows, 1.1 seconds for JSON objects, 12 ms to clone an ArrayBuffer, and 0.75 ms to transfer it. So large data goes as records (ArrayBuffer), and commands as JSON. As for connecting, move the core entirely into the shared daemon, and have the extension connect directly from TypeScript to the daemon's front API (no napi: with the core in the daemon, the only advantages left for napi would be mmap and shared memory, and getting to the webview is an inter-process copy anyway). ① The front API (`crates/front`, `docs/front.md`) is done: commands and answers are JSON, large data is `repr(C)` records, frames are the same as the daemon's (no compression), and on connecting it first sends `\u0001` + the location. History and Working Copy diffs are held by number, and searches can be cancelled per client (`Repository::search_history_until`). The CLI passes every command through this front API (`InProcess` in the same process locally, the daemon for `daemon:` and `ssh://`). On linux, `amonhen log -n 30` over the daemon is 0.03 seconds from the second time (0.09 seconds when opening in the same process each time), and the `log fix` search is 0.05 seconds from the second time (1.5 seconds likewise). ② The extension's TS connecting side (vscode-amonhen's `src/daemon/`; asks `amonhend --socket` for the socket location and launches it if not running) is done. ③ When the setting `amonhen.daemon.path` is set, History (the graph converts the core's layout into per-row line segments), commit details and diffs, Working Copy diffs, and line-level staging and discarding are done through the daemon — done (confirmed by opening ktor in the Extension Development Host). ④ Undo records were already common (the core and the extension read and write the same `<git_dir>/amonhen/undo.json` in the same shape), and a test confirmed that lines discarded through the daemon can be restored with the extension's Undo. ⑤ Writes (commit, file-level staging, branches and tags, merge, rebase, cherry-pick, revert, stash, fetch, push, continue / abort / skip, submodules) are done with commands to the daemon's core (vscode-amonhen's `src/daemon/writes.ts`; replaces the writes in the extension's git execution layer, while reads and the extension's own procedures (discard that deletes untracked files, index writes, rebase todo) stay in the git execution layer) — done. Operations for which the core records Undo are not recorded by the extension (conveyed with `AsyncLocalStorage`). The extension's Undo records kept their contents in memory, missed records written by the app, CLI and daemon, and erased them on the next save, so they are now reread every time (the core already rereads every time). ⑥ Distribution was sorted out: the engine is a single executable `amonhen` serving as both CLI and daemon (darwin-aarch64, musl linux-x86_64 and linux-aarch64; tar.gz attached to releases), its location on every machine is `~/.local/share/amonhen` (`amonhen install`, `amonhen engine`), and it is placed the same way on SSH hosts. The extension is a single common .vsix bundling the three engines (about 8 MB), places the matching one at the location on startup, and uses the shared daemon with no settings (can be turned off with `amonhen.daemon.enabled`; if there is no matching engine it runs on the git execution layer). For the extension's CI to read the standalone releases, a key that can read them (`AMONHEN_STANDALONE_TOKEN`) is needed, since the repository is private. ⑦ Remote-SSH: the extension's daemon-related tests passed when run with the distributed Linux engine on a local Linux machine of the same kind as a Remote-SSH host (x86_64, Node 24) (socket in `$XDG_RUNTIME_DIR`, shared memory in `/dev/shm`). Windows: the shared daemon listens on a named pipe (the same choice as git's fsmonitor, VSCode and Docker; `crates/remote/src/endpoint.rs`, the `interprocess` crate; overlapped I/O allows reading and writing at the same time, the name includes a hash of the user's SID and the protocol version, permissions are only the user's own SID, and the first creation uses `FILE_FLAG_FIRST_PIPE_INSTANCE`). Shared memory comes later (Node cannot use it; it pays off with a Rust FE; the protocol already supports both). The engine's location is `%LOCALAPPDATA%\amonhen` (`current` is a file containing the hash, and the CLI is copied to `bin\amonhen.exe`), and the History cache is `%LOCALAPPDATA%\amonhen\cache`. The Windows engines (x86_64, aarch64) are built on CI's Windows machines (`.github/workflows/windows-engine.yml`), attached to releases as zip, and bundled in the .vsix. CI's Windows tests run the CLI over the shared daemon (`works_through_the_shared_daemon_on_every_platform`). Also confirmed on the Remote-SSH screen (connected the Extension Development Host to a local Linux machine with Remote-SSH; the extension placed the bundled Linux engine at the location on the SSH host, linked `~/.local/bin/amonhen`, and showed toybox's History, commit details and diffs through the shared daemon in `$XDG_RUNTIME_DIR`). On Windows, the CLI test over the shared daemon passed over named pipes on CI's Windows machine. The extension's tests run in CI on macOS, Ubuntu and Windows (on Windows, the `C:/…` paths git reports are turned into the OS form, and temporary directories a just-stopped git still holds are removed with retries). Checked on a real Windows machine (x64, Windows 11 Pro; [windows.md](windows.md)): the extension in VSCode placed the bundled engine in `%LOCALAPPDATA%\amonhen` and showed History, commit details and diffs, Working Copy with line staging, commit and Undo through the shared daemon on a named pipe, shared with the CLI. This found three problems, now fixed: the daemon (which has no console) flashed a console window each time it ran git or ssh (every spawn now sets `CREATE_NO_WINDOW`, `amonhen_model::process::command`); Working Copy diffs ignored git's line ending conversion, so with `core.autocrlf=true` every changed file showed as entirely rewritten and staging lines put CRs into the index (the working tree is now read as git stores it, through gix's filter pipeline with git's built-in filters only, and discarding lines keeps the other lines' bytes; the protocol went up to 18 for the SSH host to convert); and the extension's tests removed a temporary repository before the resident `git cat-file` had exited, which Windows refuses. Next: WASM (run the core as WASM in environments without the engine; it builds for wasm32-wasip1 and ran with Node's `node:wasi`, but gix reads packs with mmap, so they cannot be read under WASI; the main candidate is leaving git reads and writes to the extension's git execution layer)
- [ ] 19. FEs for other OSes (iOS / Linux / Android / Windows). Projection formats and Platform implementations; add Git2Backend for mobile. Linux (GTK4, `apps/gtk4`, gtk4-rs; nothing in it is Linux-specific, so it also runs on the BSDs, and on Windows and macOS where GTK runs, though those get their own native FEs; a separate Cargo workspace so the root workspace needs no GTK): started. Opens a local repository and shows History (the graph drawn with GSK paths, grouped by color and width; ref labels, subject, author, date and ID with Pango; only the visible rows are drawn by one `GtkScrollable` widget; subjects are read 256 rows at a time in the background with `FrontendProfile::GTK`, and only the 24 chunks near the visible area are kept), with selection by click and keys, fisheye lanes with the same animation, the same persistent cache, and rereading when refs change (`amonhen-watch`). Selecting a commit shows its details in the macOS app's detailed layout (author and committer on top, then a table with English headings — Commit, Parent, Child, Rev, Signature, Refs, Branches, Tags, Previous Tag, Pushed, Merged — then the subject, body and notes; `commit_info` first, and `commit_extras` added when it arrives, the pane fitted to its content up to half the right column) and the diff of all its files (`selection_diff`, drawn like History: only the visible rows, word highlights and syntax colors from the core turned into Pango attributes as they are, since the offsets are already UTF-8 bytes; clicking a file's header folds it). The sidebar (`GtkListView` over a `GtkTreeListModel`, so only the visible rows get widgets): Workspace (Working Copy with its number of changed files, History), Branches (names like `feature/x` grouped into folders, the current branch in bold, ↑↓ against the upstream, gone), Remotes (per remote, then folders), Tags (closed at first when there are many), Stashes (by message) and Submodules; the icons are drawn with GSK paths in the text color, since icon themes may lack branch and tag icons. Selecting a ref selects its commit in History (the row index is built in the background on first use). Right-clicking a ref offers the same operations as the macOS app (local branches: checkout, merge, rebase, push to the upstream, new branch or tag, rename, delete; remote branches: check out as a local branch, merge, rebase, new branch; tags: merge, new branch; stashes: apply, pop, drop; submodules: update), and dropping a branch or tag on a local branch offers merge and rebase, and a local branch on a remote or remote branch offers push and force push (asked before). Operations run the core's commands (`run_json`) in the background, report failures and conflicts, and reread History, refs and status; Undo (in the header bar, and Ctrl+Z when no text field takes it) shows the next operation to undo. Menu items have the macOS app's names, order and separators (in English), placed the GNOME way: a Repository menu button at the left of the header bar is the macOS Repository menu (Fetch, Pull, Pull With, Push, Force Push (with Lease)…, New Branch…, Rename Branch…, Set Upstream…, New Tag…, Merge…, Rebase… (chosen from a list like the macOS app), Continue, Skip, Abort, Stash…, Pop Stash, Undo, Rebuild History, Open in Terminal, Reveal in Files), and the primary menu (≡) at the right end has File, View, Navigate and Commit as submenus of the same names, then Settings, Activity, Keyboard Shortcuts and About Amon Hen. Items the GTK4 app has no feature for yet (Edit, search, view toggles, GitHub, tabs) are left out. Items are window actions; shortcuts map ⌘ to Ctrl and ⌃ and ⌥ to Alt (Ctrl+O, Ctrl+1, Ctrl+2, Ctrl+R, Ctrl+Shift+B, Ctrl+Shift+H, Alt+↑↓, Ctrl+Alt+F, L, P, B, M, R, S, Z, A, Ctrl+Return, Ctrl+,), leaving out keys Ubuntu takes globally (Ctrl+Alt+T for New Tag), and are listed in a `GtkShortcutsWindow`. The header bar also carries the macOS toolbar's default buttons in the same order (Branch with the current branch's name and a menu, Fetch, Pull and Push with their menus and ↓/↑ counts against the upstream, Merge, Rebase, Stash, Pop, and Refresh and Undo at the right, with the same tooltips), drawn as icons in the text color; Repositories open from a path, `daemon:/path` (the local shared daemon, started with the engine at `~/.local/share/amonhen/current` or `amonhen` on PATH) or `ssh://host/path` (the SSH host's daemon; engines to push are taken from `AMONHEN_ENGINES` or an `engines` folder next to the executable, like the macOS app's Resources/engines, and History is cached locally per location), on the command line or with File > Add Remote…; through a daemon, changes come from the daemon's notices instead of watching files. Opened from macOS over SSH, luna.local's openbsd-src showed History, details and diffs. Searching History works like the macOS toolbar's search field, shown the GNOME way as a search bar above History (the search button or Ctrl+F): Enter searches with the core's query syntax (`search_history`, in the background), matching rows are tinted, the count shows n / total, and Enter / Shift+Enter (Ctrl+G / Ctrl+Shift+G) move between matches; a newer History is searched again; the toggle next to the arrows shows only the matching commits (`history_matching`, lines following first parents to the next match), as in the macOS app, and searching always runs on the full History. The sidebar's context menu has the macOS items too (Checkout main, Merge X into Y, Rebase Y onto X, New Branch from X…, Open X in New Window for submodules, …), as `GtkPopoverMenu`s built from menu models. Right-clicking a commit in History selects it and offers the macOS app's commit menu (New Branch at, New Tag at, Checkout (Detached); for non-merge commits Edit Message…, Squash into Parent…, Fixup into Parent, Edit (Stop Here), Split…, Drop (asked first); Cherry-pick, Revert; Copy Commit ID), using the core's `rewrite`, `commitMessage` and `squashMessage` with the macOS dialogs. Several commits can be selected (Ctrl+click, Shift+click, Shift+↑↓): the details pane then shows how many are selected, how the diff is taken (consecutive: one diff from the oldest's parent; two apart: A..B with switches to reverse it or take it from the merge base; scattered: each commit's diff by file) and a line per commit, the diff comes from `selection_diff`, and the right-click menu offers Cherry-pick N Commits, Revert N Commits, Squash N Commits… (consecutive, no merges), Copy Commit IDs and Copy Short IDs, as in the macOS app. Working Copy (a page next to History, switched in the sidebar): the commit message (subject, body, amend, which fills in HEAD's message), the changed files with a check for whether each goes into the next commit (the index's state itself, partly staged shown as mixed; conflicts first), and the selected file's diff. The diff is one diff from HEAD to the working tree with a mark per changed line for whether it is in the index (`working_diff_combined`; clicking a mark includes or excludes that line, and lines picked with click, Shift and Ctrl, or a hunk's header, are included, excluded or discarded with the buttons). Files that cannot be shown that way get a switch between the working tree side and the staged side. Status is reread when the index or the working tree changes. The labels follow the macOS app (Summary, Amend, Commit / Amend Commit with Commit and Push and Amend Last Commit, Changes with staged / total, Unstaged / Staged, Stage Lines / Unstage Lines, Discard Lines / Discard File); each file shows its line counts (`workingStats`) and, on hover, a button to discard its changes (asked first; untracked files are deleted, which Undo cannot bring back); the list can be filtered by path and shown, as in the macOS app, as paths, as a directory tree (single-child directories joined into one level, folders folding on click) or by file name with the parent directory dimmed; and a conflicted file shows its content with markers (`conflict_view`, Current and Incoming tinted) with Take Current File, Take Incoming File, Open Merge Editor and Mark Resolved (asked first when conflicts remain). The merge editor opens in its own window like the macOS app's: Current, Result (editable, starting from the working tree's content) and Incoming side by side, with Base at the left when the conflict has a common ancestor (diff3); conflicts are tinted in every pane, those already resolved dimmed in the side panes, and the panes scroll together, aligned at the conflicts. The selected conflict (moved with ∧ ∨ or Ctrl+Alt+↑↓, or by clicking in a pane) is resolved with Accept Current, Accept Incoming, Accept Both or Accept Base, and ✨ Magic Merge resolves what the core's rules can; each replaces only the changed part of Result, so Ctrl+Z undoes them one at a time. On opening, the unmistakable conflicts (both sides the same, one side changed) are resolved (Settings > Git can turn this off). Save (Ctrl+S) and Save & Mark Resolved (Ctrl+Return, asked first when conflicts remain) write with `saveMerge`, and closing with unsaved changes asks first. The conflicts are computed with `amonhen_core::conflict` in the app, since it works on text only, and the sides are read with `mergeSides`, so it works through a daemon too. The AI buttons and the bands joining the panes are left out. `--snapshot out.png` and `--measure-scroll`. Measured with GTK on macOS (linux, 1.48M commits, 40 rows per frame): the History snapshot takes p50 3.2 / p95 4.1 ms, of which the graph is about 0.1 ms and most of the rest is Pango shaping the newly visible rows on the main thread. With the cairo renderer the main thread is p50 5.7 / p95 6.8 ms per frame at 60 Hz (11 dropped frames of 600); the GL renderer on macOS runs at half rate regardless of the work, so the GPU renderers still need measuring on real Linux. On Ubuntu 24.04 (GTK 4.14, GNOME on Xorg, NVIDIA RTX 5060 Ti, 60 Hz; openbsd-src, 248k commits, 40 rows per frame) the main thread takes p50 1.6 / p95 2.2 ms per frame (History snapshot p50 1.1 ms) with the default renderer, and about the same with vulkan, ngl and cairo. About 220 of 600 frames are still dropped, but just as many at 1 row per frame (0.8 ms of work), so that is the X11 session's frame pacing rather than the app. In a Wayland session on the same machine, frames are no longer dropped: with the default renderer (ngl) the main thread takes p50 1.5 / p95 2.2 ms per frame at 40 rows per frame with 2 of 600 frames dropped (0.6 / 1.0 ms and none at 1 row per frame), ngl and cairo are about the same, and vulkan is p50 2.5 / p95 3.6 ms with none dropped at 40 rows (19 at 1 row, around a 146 ms stall at the start). At 200% (Xwayland with `GDK_SCALE=2`, since Wayland takes the scale from the compositor; the window fills the 1920×1080 screen, about twice the pixels of the 1280×760 window at 100%) at 40 rows per frame, cairo is p50 1.7 / p95 2.5 ms and vulkan p50 2.3 / p95 3.9 ms, both about the same as at 100%: on this CPU (Ryzen 9 9950X3D) cairo's software rendering does not grow with twice the pixels, and vulkan spends more on the CPU and stalls once at the start (191 ms). With cairo the app does not touch the GPU (it is absent from `nvidia-smi`; it paints on the CPU into shared memory and mutter uploads it), so the main thread's time leaves out the GPU's drawing for ngl and vulkan and the compositor's upload for cairo. From the start of a frame (when the scroll advances) to the presentation time the compositor reports (`GdkFrameTimings`), every renderer takes the same whole frames: on Wayland p50 33.3 ms (2 frames) with p95 33.7 (ngl), 33.9 (vulkan) and 34.4 ms (cairo), and through Xwayland at 200% about 48.6 ms (3 frames) for all three. The delay is set by the compositor's pipeline, not by how the app draws (the macOS app likewise shows 2 frames through WindowServer). The app does not pick a renderer and leaves it to GTK (`GSK_RENDERER` still works). Ubuntu's fonts are larger than macOS's and are given in pixels, so History's row height follows the font and smaller fonts are scaled in the same unit; History and the diff use the theme's `view` background so dark themes work. Blame and File History open in their own windows as in the macOS app, from a file header in History's diff (Blame at <commit>, File History) and from a Working Copy file's right-click (Blame, File History): Blame groups lines by commit with the ID, author and date on a group's first line and the subject on the second, shading every other group, and File History lists the commits that changed the file (following renames) with that commit's diff of the file; both are `GtkListView`s, and double-clicking (activating) a row selects its commit in History. Activity (the primary menu or Ctrl+Alt+A) opens a window with the macOS app's tabs, reread every second while open and rebuilt only when their contents change: Operations (the git processes and core commands from `amonhen_model::activity`, plus the daemon's through `remoteActivity` read in the background, newest first, with ✓ / ✗ / ● for running ones with their progress and a cancel button, filtered by kind, failures only or text, and exported as JSON), History (the undo log, with Undo on the newest and Undo Back to Here on the others), Performance (History and status reloads and diff loads: count, p50, p95, max and last), Events (what changed when the watcher reported refs, the index or the working tree) and Daemon (the shared daemon's pid, version, uptime, connections and repositories, with start and stop). Settings (the primary menu or Ctrl+,) has the macOS app's tabs with the items the GTK4 app has a feature for, under the same keys: Git (Pull's default: merge, rebase or fast-forward only, used by Pull and Ctrl+Alt+L; resolving the unmistakable conflicts when the merge editor opens), Appearance (the commit body in a monospaced font), Performance (write a missing commit-graph in the background when opening a local repository) and Advanced (open repositories through the shared daemon). They are kept in `~/.config/amonhen/gtk.ini` (a GLib key file; GSettings needs a schema installed, which an unpacked executable does not have), along with the Working Copy's list layout, so the next window uses it too. Packaging: `scripts/package-gtk4.sh` builds `dist/amonhen-gtk-<version>-linux-<arch>.tar.gz` on Linux (the executable, the Linux engines from `dist/engine` to push over SSH, a `.desktop` file, the icon and `install.sh`, which puts them under `~/.local`, adds the app to the desktop's list and installs the bundled engine for the shared daemon); GTK itself is not bundled, so it needs GTK 4.14 or later (Ubuntu 24.04, Debian 13 and later). CI builds the package on Ubuntu 24.04, and releases and snapshots carry it for x86_64 and aarch64 (`build.yml`'s `gtk` job, with the version shown in About).
- [ ] 20. Branch operations. Done: the sidebar's Branches arranged for getting around (Favorites ★ in the order chosen by dragging, the current branch with the branches it is built on, the default branch, the active branches newest first with `/` folders, then Inactive and Archived folded), branches built on another one nested under it (↳; the base is the upstream's local branch, else the first branch reached walking back along the tip's first parents: one whose tip is there, or a pushed branch one of whose own commits is there — own meaning on its first-parent line and newer than where it was created, read from its reflog, so a base that moved on is not taken for the branch stacked on it; the default branch's history counts for no branch, and a pushed branch is stacked like any other; `crates/core/src/branches.rs`, `{"op":"branchGroups"}`), marks at the right of a row (merged into the default branch, upstream gone, checked out in another worktree, ahead/behind, editing) with a popover explaining each and offering the next step (Delete Branch, Archive, Open Worktree), Archive and Favorites kept in git's config (`branch.<name>.amonhenArchived`, `amonhenPinned`), and History showing a branch together with its upstream when they have diverged. Planned: delete the merged branches in one go (a sheet listing the branches merged into the default branch, excluding the current one, favorites and those checked out in other worktrees; `git branch -d`, so nothing unmerged goes; recorded for Undo as `git branch <name> <old tip>`); mark branches whose pull request was merged on GitHub by squash or rebase as merged too (read merged PRs with `gh pr list --state merged`, match the head branch by name and tip, say "merged by PR #123" in the popover, and allow `-D` for those only in the sheet); prune gone upstreams (`fetch --prune` already does; offer to delete local branches whose upstream is gone and that are merged); archive several branches at once (multi-select in the sidebar); a per-repository default branch setting for repositories without `origin/HEAD`, `main` or `master`, where merged and the trunk cannot be told
