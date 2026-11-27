# Icons

[日本語](ja/icons.md)

Every front end draws the same icons: one set of shapes, kept as SVG in `crates/icons/svg`, drawn by each front end with its own renderer. This page records the rules, what each shape means, and how the set was decided.

## Why

Each front end used to take its icons from its own platform, so the same command looked different everywhere:

- The macOS app used SF Symbols.
- The GTK4 app drew about 75 icons with GSK paths, shaped after the macOS app's SF Symbols, since icon themes may lack branch and tag icons.
- The Windows app used WinUI's `Symbol` font, which has no Git shapes: Merge was Shuffle, Rebase was Sort, Stash was Save, Pop was OpenFile, Terminal was Keyboard. Tag signatures were emoji.
- The VSCode extension used codicons and a few characters (✎ ⌂ ⑂).
- The terminal front end uses text.

Pull, for one, was a circled arrow to a line on macOS, `repo-pull` in VSCode and Import on Windows. SF Symbols may only be used in apps for Apple platforms, so copying their shapes was no way to make the other front ends match.

Two directions were weighed: a shared, original set drawn by every front end, or each platform's own set with a shared table of meanings (and our own shapes only where a platform has none). The second keeps each platform's look but leaves the shapes different, which [frontends.md](frontends.md) would count as △ for anything the platform does not force. The shared set was chosen.

## Rules

- A 16×16 grid, strokes with round ends and round joins. Outline, not filled. The shapes are drawn with 1.5-unit strokes; each front end strokes them at its platform's weight (below).
- One color, chosen by the front end (normally the text color; the accent color for the current branch, and so on).
- A circle is a commit. A line across the top is the remote. An arrow is the direction something moves. Dots are something that is not there yet, or where something used to be.
- Shapes are drawn new, not traced from any platform's set. Tools everyone knows (search, refresh, terminal) keep their familiar shapes, since those read fastest.

![What each shape means](images/icons/intent.en.png)

## How the set was decided

1. The toolbar's 19 icons were drawn first and compared at 16, 20 and 48 points on light and dark backgrounds. At 16 the first Merge read as a person and Rebase's dotted line vanished, so both were redrawn, and Pull and Push got smaller arrowheads with more room.
2. Five shapes still didn't say what they mean: Merge (a common motif), Rebase (it only said that something moved), Stash and Pop (they looked like download and upload) and Repositories (it read as a cake or a tray). Each got three or four alternatives.
3. The chosen ones: Merge C (Branch upside down, so the two pair up), Rebase B (dots where the branch used to start), Stash and Pop B (a drawer), Repositories B (a bookshelf).
4. In the apps they looked out of place next to each platform's own icons. Measured on screenshots: the 1.5-unit strokes were heavier than SF Symbols on macOS (1.7pt against 1.4pt), Segoe Fluent Icons on Windows (1.9px against 1px) and codicons in VSCode (1.5 against 1), lighter than Adwaita in GTK4 (1.5px against 2px), and blurred at 100% because they fell between pixels. Two fixes were drawn as samples: each front end at its platform's weight, or one weight (1.25) everywhere. The first was chosen, with straight lines moved onto whole pixels.
5. The other 72 shapes (sidebar sections, branch and worktree states, tools, settings pages, signatures, clone states, CI, AI, image comparison) were drawn the same way. Where the first draft didn't read well, 31 of them, three or more alternatives were drawn, and more when none of those worked (16 for the main worktree). The pull request's shape was chosen from four drafts later.

![The drafts and the choice](images/icons/alternatives.en.png)

The other shapes, what each one means and the drafts they were chosen from:

![What the other shapes mean](images/icons/intent-rest.en.png)

![The other shapes' drafts and the choice](images/icons/alternatives-rest.en.png)

## Implementation

`amonhen-icons` (`crates/icons`) reads `svg/*.svg` at build time and keeps each icon as strokes: SVG path data (circles and rectangles turned into paths) with an optional dash pattern, and the same path as a list of moves, lines, cubic Béziers and closes (arcs approximated by Béziers). It reads only the elements and attributes our own files use (`path`, `circle`, `rect`; `d`, `cx`, `cy`, `r`, `x`, `y`, `width`, `height`, `rx`, `stroke-dasharray`) and stops the build on anything else.

Front ends draw `Icon::strokes(size, pixel_scale, stroke)`: the shape at the drawing size and the front end's weight, with vertical and horizontal lines moved onto whole pixels (centred on a pixel for odd widths, on a pixel edge for even ones). A coordinate used by a straight line moves the same way everywhere, and parts without straight lines (arrowheads, circles) move whole with the point they join or, failing that, their centre, so the shapes keep their symmetry. Lines are stroked with round caps and joins; a dash of length 0 with round caps makes a dot.

- GTK4: 2px at 16px (Adwaita's symbolic icons), in the text colour itself (the older icons are drawn at 70%). `apps/gtk4/src/icon.rs` builds a GSK path per name, size and scale factor.
- macOS: 1.4pt at 18pt (SF Symbols' regular weight in the toolbar), in proportion at other sizes, snapped for the screen it is drawn on. The C ABI hands over the strokes (`amonhen_icon_strokes`); `SharedIcons` in AmonhenKit turns them into `CGPath`s and `AppIcons` in the app strokes them into template images, so the toolbar and sidebar tint them like SF Symbols. `NSImage` reads SVG on current macOS, but whether it does on 14.4, the oldest supported version, has not been checked, so the app doesn't depend on it. The toolbar's sidebar toggle and search field are AppKit's own items and keep their system look, and Finder keeps Finder's folder.
- Windows: 1px at 20px (Segoe Fluent Icons), snapped for the window's scale. WinUI's icon elements (`PathIcon`, `FontIcon`) only fill, so the toolbar and its customization palette stroke each shape with XAML's `Path` shape (dashes in units of the thickness, the text colour from the theme). windows-reactor had no `Path`; it was added to the vendored copy, with the `Shape` and `Path` bindings generated by windows-bindgen from the Windows App SDK's winmd. Files keeps Explorer's folder from Segoe Fluent Icons.
- VSCode: 1px at 16px (codicons), on the sidebar's title buttons (Fetch, Pull, Push / Publish, Undo, Select Repository, Refresh, Switch Branch, New Branch, Stash) and branch rows. VSCode draws an extension's SVG icons as images, which can't take the theme's colors (`currentColor` comes out black), so the shapes are glyphs of an icon font, `media/amonhen-icons.ttf`, registered as `amonhen-<name>` in `package.json`'s `contributes.icons` and drawn like codicons: they take the theme's icon color, high contrast and the selected row's color, and a state's color is a theme color (`charts.green` for a branch checked out in another worktree). Each glyph is the 1px stroke snapped for 100% (which also lands on whole pixels at 200%) turned into an outline, on a 16px em. The font and `contributes.icons` are made with `cargo run -p amonhen-icons --example vscode` (`crates/icons/tools/vscode_font.rs`), and a test fails when they no longer match the shapes. It was chosen over a light and a dark SVG of each shape after comparing both in VSCode's light, dark and high contrast themes: the shapes and their sharpness were the same, and only the font followed the theme. The main view's tab keeps the activity bar mark as SVG, since a webview panel takes only images.

The figures on this page come from `crates/icons/tools/figures.swift` (on macOS) with the data next to it; the drafts are kept in `crates/icons/tools/alternatives`:

```bash
cd crates/icons/tools
swift figures.swift intent intent.en.tsv ../svg ../../../docs/images/icons/intent.en.png
swift figures.swift alternatives alternatives.en.tsv alternatives ../../../docs/images/icons/alternatives.en.png
swift figures.swift intent intent-rest.en.tsv ../svg ../../../docs/images/icons/intent-rest.en.png
swift figures.swift alternatives alternatives-rest.en.tsv alternatives ../../../docs/images/icons/alternatives-rest.en.png
```

## The terminal front end

The terminal front end can't stroke shapes, so it shows one character for each shared name. The map below is decided but not in the code yet; the terminal front end still uses its own characters.

- Only characters whose Unicode East Asian Width is N or Na, so each takes one cell in every locale. Ambiguous ones such as ● ↑ ★ ▸ take two cells in GNOME Terminal set up for Japanese, while `unicode-width` counts them as one.
- No character with the Emoji property: Windows Terminal draws some of them as color emoji when its font lacks them (✔ and ⏲ did).
- Checked in three terminals with their default fonts: GNOME Terminal (Ubuntu Sans Mono, Japanese locale), macOS Terminal (SF Mono) and Windows Terminal (Cascadia Mono). Most of the characters aren't in those fonts and come from a fallback: macOS Terminal shrinks the glyph to the cell, GNOME Terminal lets it spill into the next cell, and Windows Terminal takes it from Segoe UI Symbol. A character that spilled over in any of them was replaced, which rules out most round shapes (circled arrows, gears, circled stars).
- Characters follow the shared shapes where they can: the line on top for a remote (`⊤`, and `⍡` for a tag that differs there), two-headed diagonal arrows for expanding the details (`⤢`) and the other diagonal for restoring them (`⤡`; no character has arrows pointing in), a frame for zoom to fit (`⬚`), `!` for cannot verify like its circled !.
- Names with the same meaning share a character (`✗` for failed and CI failed, `⊘` for cancelled and CI cancelled, `⁺` for both new-branch names, `✎` for the worktree to write to and Working Copy, which have the same shape); bad signature also uses `✗`, as it only appears among signatures. Different meanings get different characters: no changes `=`, signed `ʃ` and CI passed `✓`; cannot verify `!` (a circled !) and a file of unknown kind `?` (a sheet with ?).
- CI running `⣾` is a frame of the braille spinner, so the front end can step through ⣾⣽⣻⢿⡿⣟⣯⣷. Refresh `⤸` is drawn small in macOS Terminal; the circled arrows that read better spill over in GNOME Terminal.

| Name | Meaning | Character | Code point |
| --- | --- | --- | --- |
| `account` | Accounts | `@` | U+0040 |
| `activity` | Activity | `≋` | U+224B |
| `ahead-behind` | Ahead / behind | `↨` | U+21A8 |
| `ai` | AI explanation and suggestion | `✦` | U+2726 |
| `appearance` | Appearance | `☾` | U+263E |
| `back` | Back | `‹` | U+2039 |
| `bad-signature` | Bad signature | `✗` | U+2717 |
| `branch` | Branch | `⑂` | U+2442 |
| `cancelled` | Cancelled | `⊘` | U+2298 |
| `cannot-verify` | Can't verify with this git | `!` | U+0021 |
| `changed-pixels` | Changed pixels | `⊹` | U+22B9 |
| `checkerboard` | Checkerboard | `▚` | U+259A |
| `ci-cancelled` | CI cancelled | `⊘` | U+2298 |
| `ci-failed` | CI failed | `✗` | U+2717 |
| `ci-none` | No CI | `◌` | U+25CC |
| `ci-passed` | CI passed | `✓` | U+2713 |
| `ci-running` | CI running | `⣾` | U+28FE |
| `clean` | No changes | `=` | U+003D |
| `collapse-all` | Collapse all | `⊟` | U+229F |
| `conflict` | Conflict | `‼` | U+203C |
| `current-branch` | Current branch | `▹` | U+25B9 |
| `disconnected` | Disconnected | `⌁` | U+2301 |
| `display-options` | Display options | `⁑` | U+2051 |
| `edit-mode` | Edit mode | `✐` | U+2710 |
| `edited-worktree` | Worktree to write to | `✎` | U+270E |
| `editor` | Editor | `⟨` | U+27E8 |
| `expand` | Expand the details | `⤢` | U+2922 |
| `expand-all` | Expand all | `⊞` | U+229E |
| `expired-key` | Expired key | `⧖` | U+29D6 |
| `failed` | Failed | `✗` | U+2717 |
| `favorite` | Favorite | `✭` | U+272D |
| `fetch` | Fetch | `⇣` | U+21E3 |
| `files` | Files | `▯` | U+25AF |
| `filter` | Filter | `⋎` | U+22CE |
| `folder` | Folder | `▭` | U+25AD |
| `go-to-head` | Go to HEAD | `⇥` | U+21E5 |
| `history` | History | `↺` | U+21BA |
| `list` | List | `≣` | U+2263 |
| `main-worktree` | Main worktree | `⌂` | U+2302 |
| `merge` | Merge | `⑃` | U+2443 |
| `missing-key` | Missing key | `⚷` | U+26B7 |
| `modified` | Modified | `⁎` | U+204E |
| `new-branch` | New Branch | `⁺` | U+207A |
| `new-branch-row` | New branch row | `⁺` | U+207A |
| `not-checked-out` | Not checked out | `∘` | U+2218 |
| `onion-skin` | Onion skin | `⊓` | U+2293 |
| `partial` | Partial clone | `⊏` | U+228F |
| `performance` | Performance | `ϟ` | U+03DF |
| `pop` | Pop | `⤉` | U+2909 |
| `preview` | Preview | `⌾` | U+233E |
| `publish` | Publish | `⇡` | U+21E1 |
| `pull` | Pull | `⤓` | U+2913 |
| `pull-request` | Pull request | `⥂` | U+2942 |
| `push` | Push | `⤒` | U+2912 |
| `range-diff` | Range Diff | `≍` | U+224D |
| `rebase` | Rebase | `↱` | U+21B1 |
| `reflog` | Reflog | `↜` | U+219C |
| `refresh` | Refresh | `⤸` | U+2938 |
| `remote` | Remotes | `⊤` | U+22A4 |
| `remote-conflict` | Tag differs on the remote | `⍡` | U+2361 |
| `repositories` | Repositories | `⦙` | U+2999 |
| `restore` | Restore the details | `⤡` | U+2921 |
| `revoked-key` | Revoked key | `⌀` | U+2300 |
| `search` | Search | `⌕` | U+2315 |
| `settings` | Settings | `⋮` | U+22EE |
| `shallow` | Shallow clone | `⋯` | U+22EF |
| `side-by-side` | Side by side | `⫴` | U+2AF4 |
| `sidebar` | Sidebar | `⎸` | U+23B8 |
| `signed` | Verified | `ʃ` | U+0283 |
| `signer-mismatch` | Different signer | `≁` | U+2241 |
| `sparse` | Sparse checkout | `⁘` | U+2058 |
| `stacked` | Stacked branch | `⫶` | U+2AF6 |
| `stash` | Stash | `⤈` | U+2908 |
| `stashes` | Stashes | `⊔` | U+2294 |
| `submodule` | Submodules | `⊡` | U+22A1 |
| `subtree` | Subtrees | `⤷` | U+2937 |
| `swipe` | Swipe | `⇹` | U+21F9 |
| `tag` | Tags | `⬦` | U+2B26 |
| `terminal` | Terminal | `≻` | U+227B |
| `theme` | Theme | `◬` | U+25EC |
| `translate` | AI translation | `⥊` | U+294A |
| `tree` | Tree | `⊢` | U+22A2 |
| `trust-unknown` | Trust undecided | `⸮` | U+2E2E |
| `undo` | Undo | `↩` | U+21A9 |
| `unknown-file` | Unknown kind | `?` | U+003F |
| `upstream-gone` | Upstream gone | `⍊` | U+234A |
| `whitespace` | Whitespace | `␣` | U+2423 |
| `working-copy` | Working Copy | `✎` | U+270E |
| `wrap-lines` | Wrap lines | `↵` | U+21B5 |
| `zoom-fit` | Zoom to fit | `⬚` | U+2B1A |
| `zoom-in` | Zoom in | `+` | U+002B |
| `zoom-out` | Zoom out | `-` | U+002D |

## Status

- [x] The rules and the toolbar's 19 icons
- [x] GTK4: the toolbar (and the same kinds elsewhere, such as Branch in the sidebar)
- [x] macOS: the toolbar and the sidebar's branches
- [x] Windows: the toolbar
- [x] The other 72 shapes (sidebar sections, branch and worktree states, tools, settings pages, signatures, clone states, CI, AI, image comparison), chosen from three or four drafts each where the first one didn't read well
- [x] The other 72 shapes in the front ends:
  - GTK4: every icon is from the shared set; the hand-drawn paths are gone.
  - macOS: SF Symbols with a shared shape are mapped by name (`AppIcons.bySymbol`), including History's signature marks; system controls (chevrons, plus, close, info) and CI's small dots in History stay as SF Symbols.
  - VSCode: tree rows (sections, worktrees, branches, tags and their signatures, stashes, remotes, submodules, subtrees) and the branch and ignored-file pickers; codicons stay in tooltips and for rows without a shared shape (a paused sequence, the inactive and archived branch groups, a signature being checked).
  - Windows: the file list's buttons, diff options, edit mode, All Files and the image diff (drawn with Direct2D). The sidebar puts the shared icon in front of every row (the current branch and the open worktree in the accent color) and uses the shared shapes for the marks at a row's end.
- [x] VSCode: the sidebar's title buttons and branch rows
- [x] A pull request's shape (the sidebar's pull request rows in macOS, GTK4 and Windows), chosen from four drafts: D, B's arrow under the line for a remote, as the request is made on GitHub
- [x] The Windows app's icon: the macOS app icon's picture in Windows' frame (`scripts/make-icon.swift --windows`: it fills the canvas, without a shadow and with smaller corners, and leaves out the faint stars at 32 px and below), in the executable and set on each window
- [x] VSCode's activity bar mark (one color, which VSCode tints): the two ridges kept, redrawn with 1.5 lines, and the summit made the seat's ring with the outer ridge stopping short of it. Chosen from drafts that put the app icon's ridge, lanes, seat and star into lines; a ring between two lines reads as a person, and a four-pointed star is AI's sparkle
- [x] The GTK4 app's icon: the same picture in GNOME's shape (`scripts/make-icon.swift --gnome`: a rounded square on the 128 grid, 104 wide with corners of 8, a darker 4 px edge underneath for depth and no shadow, without the faint stars at 32 px and below), installed into `hicolor` at each size from 16 to 512. Chosen over a circle (which cuts off the hills and stars) and a square without the edge
- [x] The VSCode extension's icon (`package.json`'s `icon`, shown in VSCode's Extensions view; the extension isn't on the Marketplace): the Windows icon's picture at 256 px (`scripts/make-icon.swift --vscode`), since VSCode draws it as it is, without a mask
- [x] The terminal front end's characters: one for each shared name, checked in GNOME Terminal, macOS Terminal and Windows Terminal (the table above). Not used by the terminal front end yet
- [x] VSCode's format: an icon font (`contributes.icons`) instead of SVG copies, to follow the theme
