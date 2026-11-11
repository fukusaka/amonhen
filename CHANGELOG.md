# Changelog

User-visible changes, by release. The app and the engine (CLI and daemon) are released together with `v*` tags, and the
VSCode extension separately with `vscode-v*` tags, so each has its own section. Changes not released yet go under
`Unreleased`; on release, that heading becomes `### [x.y.z] - YYYY-MM-DD`, followed by a short summary paragraph that
becomes the GitHub Release notes.

## App and engine

### Unreleased

#### Added

- When Commit can't be clicked, its tooltip says why: no files are checked, no message, or a commit in progress.
- Merge chooses how to integrate in its confirmation: Follow git's settings, Fast-Forward Only, Always Create a Merge
  Commit, or Squash into One Commit (committed right away; one that conflicts stops as Squash in progress).

#### Fixed

- Repository settings that are showing read the configuration again when it changes outside the app or on Refresh.
- The Pull sheet no longer leaves a blank space when the chosen mode can't pull: it keeps the graph as it is now and
  says so, and why, where the after graph would be.
- The merge commit the Pull sheet draws has the subject git pull writes (`Merge branch 'main' of <URL>`, with
  `url.<base>.insteadOf` applied), and the Merge and Pull sheets add ` into <branch>` the way the git in use does,
  following `merge.suppressDest`.
- Publish, Commit and Publish and Pull work on a branch whose upstream was deleted on the remote: they stopped with
  "Can't read the status". Publish creates the branch again, and Pull asks which remote branch to pull from.
- GTK4 matches macOS in more places: app shortcuts work in the Activity and Settings windows, Refresh re-reads the
  mailmap, and Settings re-read the chosen git and follow the worktree being edited.
- GTK4 History uses one-line rows when author, date and ID are all hidden, its diff find no longer misses matches on
  lines with characters such as `İ`, and Ctrl+G in a File History tab finds in that diff.
- GTK4 sheets look like macOS: the drawings' arrow is centred, combined commits stay bold, highlights follow the accent
  colour, the Apply Patches drawing is on top, and Return in Delete Ignored Items cancels.
- GTK4 removes and prunes worktrees from the opened repository, so a worktree whose folder is gone can be removed.
- macOS: the AI command no longer hangs on a lot of error output or freezes the window while sending a large prompt,
  and runs in an empty folder of its own.
- macOS: Shift+Return in the toolbar search searches the new text, Show Only Matching Commits no longer restarts a
  running search, and the Cherry-pick and Fast-Forward sheets name the worktree only where it matters.
- Renaming a repository group keeps its place, a Windows drive path remote is no longer shown as an SSH host, partial
  search results no longer mark the wrong rows while filtered, and SSH host settings no longer flash "git wasn't found"
  while connecting.
- History shows the CI marks when the local branch is behind GitHub: they are read from the local and upstream tips,
  not only from the newest commits of the branch on GitHub, which left an old local branch without any.
- Run in Each Submodule… keeps what the command printed in Activity, and `amonhen submodule foreach` prints it; only
  git's error output was kept before.

### [0.2.10] - 2026-11-09

The sheets for Push, Reset, Merge, Pull, Cherry-pick, Revert, Undo and History edits draw the graph before and after.
Branches merged by a GitHub pull request are marked and Clean Up Branches… tidies them away, subtrees join submodules
in the sidebar, and worktrees, stashes, rerere and ignored files get more of git's commands. The GTK4 app now
looks and behaves like the macOS app, and both apps act on the worktree you chose rather than the one you opened.

#### Added

- The engine is built with syntax highlighting (tree-sitter), and its front-end interface gives a diff's or a text's
  syntax colors (`diffSyntax`, `textSyntax`).
- Push, Force Push and Reset draw the graph before and after in their sheet: where the branch or `origin/<branch>`
  moves, and which commits a force push removes or a reset takes off. Reset also says where the changes go for each
  mode and when pushing afterwards will need Force Push.
- Merge, Pull, Cherry-pick and Revert draw the graph before and after too: the merge commit or the new commits appear
  as dashed dots, and Pull draws the mode chosen in its sheet.
- Fast-forwarding a branch, deleting one with commits nowhere else, Undo and applying patches draw the graph before and
  after as well; Undo shows commits it brings back from Reflog.
- Merging several branches in one commit draws a lane for each, and History edits now confirm Apply Changes with the
  commits before and after (dropped, combined and reworded).
- A branch merged by a GitHub pull request (squash or rebase) gets the merged mark in the sidebar, says "Merged by
  pull request #123" in its popover, and is deleted with a plain confirmation naming the pull request.
- Clean Up Branches… (Repository menu and the Branches heading) lists the branches merged into the default branch,
  merged by a GitHub pull request (squash or rebase), or whose upstream is gone, and deletes the checked ones; one
  Undo brings them all back.
- Subtrees, when git has `git subtree`: Add Subtree… brings another repository's branch into a folder, and the
  sidebar's Subtrees group pulls, pushes and splits them, remembering the source and squash. `amonhen subtree` too.
- A changed file's menu in History and Reflog restores it as it is in that commit or as it was before, and File
  History's as it is in a commit, optionally staged; Undo brings the file back. `amonhen restore --source` too.
- The Stash sheet stashes all changes or only the staged ones (git 2.35), can keep the staged changes, and Working
  Copy's file menu has Stash… for the selected files.
- A worktree's menu locks and unlocks it, moves it, and reconnects one whose folder was moved by hand (git 2.29).
  `amonhen worktree lock|unlock|move|repair` too.
- Repository > New Orphan Branch… starts a branch with no history, empty or keeping the current files staged.
- The merge editor forgets a resolution rerere applied and brings the conflict back, and Repository Settings forgets
  all recorded resolutions.
- Submodules can move their repository data into `.git/modules` and run a command in each one
  (`amonhen submodule absorb|foreach`).
- Repository > Delete Ignored Files… lists the files `.gitignore` covers (build output and the like) and deletes them.
- When origin is a fork on GitHub, both apps create pull requests on its parent, from any branch including the
  default one; the branch is pushed to the fork first, and Pull Requests lists the ones opened from the fork.
- On Linux and the BSDs, Settings > Performance > fsmonitor speeds up status with Watchman when it is installed (git
  2.26 or later); the engine answers git as its fsmonitor hook, and nothing is written to the repository's config.

#### Changed

- Japanese wording is consistent across dialogs and menus: removing is 削除, adding is 追加, git's verbs are capitalized
  and repeated by the button, and titles read target, then verb. Removing a remote, submodule or worktree says it can't
  be undone, and adding a remote presses Add.
- In the GTK4 app, typing in History opens its search bar with what was typed, as GNOME apps do.
- In the GTK4 app, Settings > General > External Editor chooses another program with the same column picker as
  Programs, and its list is divided like the macOS one.
- The GTK4 app's View > Customize Toolbar… opens a palette under the header bar like the macOS one: drag items onto
  the header bar, drag them off to remove them, or drag the default set. Search is now one of the toolbar items.
- The GTK4 app's ≡ menu follows the macOS menu bar: Edit has Undo, Cut, Copy, Paste and Select All, Window has
  Minimize (Ctrl+M), Zoom, Repositories, Activity and Bring All to Front, and the menu ends with Quit Amon Hen.
- The GTK4 app opens Blame and File History as tabs beside the repository tab, with the repository's toolbar, as the
  macOS app does, instead of separate windows.
- The GTK4 app's Settings show on/off items as switches on the right, as the macOS app does.
- The GTK4 app's Settings forms sit in a centred column of the macOS width instead of spanning the window; tables
  such as Remotes and Keys and Values still use the full width.
- The sidebar marks the current branch with a branch in a filled disc on macOS and GTK4, which stays recognisable on a selected row, instead of only tinting its icon.
- The GTK4 Activity window shades every other row in its tables, as the macOS one does.
- The GTK4 app shades every other row in the Remotes, Hooks, LFS and Environment Variables tables, as the macOS app does.
- The GTK4 app follows the macOS app's look more closely: its dates use the same formats, links in the details are
  link blue without an underline, and History, Blame and the key/value table have tighter rows.
- In the GTK4 app, Working Copy, Stashes, Reflog, the commit message box and Settings open at the macOS sizes, a list
  without the keyboard focus shows its selection in grey, and the key/value table and Repositories shade every other row.

#### Fixed

- Editing commits right after another tool moved HEAD no longer fails with "HEAD isn’t in the History".
- Writing to the repository (stash, commit and the like) no longer fails now and then with "index.lock: File exists"
  because the engine was refreshing Working Copy at the same moment.
- On Windows, removing a worktree whose folder another program was using for a moment no longer leaves the folder
  behind, unregistered and impossible to remove from the app, and removing such a submodule no longer fails.
- GTK4: the details follow ref changes, a multi-commit selection shows every file, an outside config change updates
  Repository Settings, and a deleted repository is reported missing instead of opening the one around it.
- GTK4: smaller differences from macOS in the diff's find highlights, the CI popover, the Repositories page, the
  Rebase sheet and a few sheets; on GNOME the Rebase sheet no longer opens enlarged and cut off.
- macOS: AI prompts keep `\` and `$1` in the inserted text, a quick second accept in the merge editor waits for the
  conflicts to be re-read, and a repository that isn't a fork is no longer looked up again on every GitHub load.
- GTK4: ↑ and ↓ in the sidebar skip headings and folders, as on macOS, instead of stopping on them.
- In a narrow GTK4 merge editor, the bar hides its lesser buttons instead of cutting every button's name.
- The GTK4 Rebase sheet no longer cuts commit subjects, its Push sheet no longer leaves an empty band, and disabled
  Commit, Push and Delete buttons lose their colour.
- The macOS app ends long Reflog and Stashes rows with "…", and a new window starts with focus in History rather than
  the branch filter.
- GTK4: flexible spaces in the toolbar go anywhere and share the free width equally, with no fixed boundary between the
  two ends of the header.
- macOS: in a narrow window the merge editor's bar hides its lesser buttons, keeping the conflict position, Save and
  Mark Resolved usable.
- The merge editor's safe rules run once per conflict, so saving no longer reapplies what Undo took back.
- Rows that only look consecutive in a filtered History are no longer treated as a range, and a binary conflict no
  longer counts as resolved without markers.
- macOS: choosing a branch while History shows only matches selects its commit, and selected branches stay highlighted
  through a sidebar refresh.
- macOS: Pull's enabled state follows the worktree being edited, and Repository Settings keeps its section when
  switching worktree.
- macOS: the merge editor no longer blocks while computing conflicts through the shared daemon.
- macOS: a failing tag signature check no longer repeats, a branch folder named like a sidebar group no longer gets
  that group's menu, and a `git rm`'d file no longer offers Blame or open items.
- GTK4: the toolbar takes several flexible spaces, Repositories columns can be reordered, and Edit menu items follow
  what the focused widget can do.
- GTK4: scan folders notice `git init` and a removed `.git`, and the sidebar keeps its scroll position on a refresh.
- GTK4: smaller differences from macOS in History, Working Copy, the commit message aids and the merge editor.
- A file that isn't UTF-8 no longer opens in the merge editor, whose save replaced its invalid bytes; take one side
  instead, which keeps the bytes.
- Mark N Without Markers Resolved saves unsaved Result edits first instead of dropping them.
- Combine N Commits refuses rows that only look adjacent because History shows only matching commits.
- macOS: Go to HEAD, New Worktree from a detached worktree and Repository Settings follow the worktree being edited,
  and a stopped operation always opens the first conflict.
- GTK4: Add to .gitignore no longer overwrites the file when reading it fails, and an unreadable bookmarks file no
  longer empties the list.
- GTK4: a commit's diff is its own when History shows only matches, and the changed-files list starts at the first file.
- GTK4: the failure band's buttons work, and a cancelled operation's message no longer hides a later failure.
- GTK4: closing edit mode and switching worktree stay on the current page, and launching with a path restores the
  session and adds a tab.
- GTK4: Rebuild History rebuilds the History on screen, Open on GitHub escapes the branch name, filtering a search
  with no matches keeps History, and dropping a commit reads the worktree being edited.
- GTK4: messages filled in by the app stay when switching worktree, Return in the path picker chooses the open folder,
  and the SSH host's Connection page no longer rebuilds on every refresh.
- History on an SSH host no longer fails with "bad object" when a commit it remembered is gone (pruned by gc, or the
  repository was made again at the same path); it reads the history again.
- `~` in a new worktree path means the home folder on the machine where git runs, including an SSH host.
- Pull requests from a fork owned by an organization are listed, among the parent's newest 300.
- Forgetting a resolution in a separate merge window shows the conflict again instead of the old Result.
- Create Pull Request… works for a repository on an SSH host.
- A submodule inside an added worktree opens as itself, not as the main repository.
- In the GTK4 app, the sidebar opens a submodule from the worktree being edited, and Forget This Resolution in a
  separate merge window stays in its own worktree.
- New Worktree's two fields and Clone's URL and Location fields no longer collapse to a sliver.
- Create Pull Request… is no longer offered on the default branch, and the default branch no longer shows the
  "Merged into" mark, when the default branch is a remote one such as `origin/main`.
- The GTK4 sidebar shortens a long branch name to keep its marks (merged, upstream gone), as the macOS app does,
  instead of hiding the marks.
- Undo of deleting a branch brings back its upstream and its archived and favorite marks, which git removes with it.
- The GTK4 app's subtree sheets name the worktree they act on and add, pull, push or split there, even when another
  worktree is selected while the sheet is open.
- macOS: the subtree sheets name the worktree they act on and run there, not in the worktree selected after the
  sheet opened.
- macOS: the sidebar's Subtrees group lists the subtrees of the selected worktree's HEAD, not the opened one's.
- Working Copy no longer fails with "Is a directory" when an untracked repository sits inside the working tree; its
  diff is empty, as for a submodule.
- Fetch, Pull and Push no longer stop now and then with "Stopped after 120 seconds without a response" while git is
  still answering; measuring the silence could wrap around when output arrived at that moment.
- The GTK4 app's automatic external editor falls back to a text editor, never a word processor such as LibreOffice Writer.
- When the AI CLI fails, both apps show its reason, such as "Not logged in", instead of only the exit code, and never
  take a failure that exits with code 0 as the answer.
- The macOS app shows commit details in the Standard style on first launch, as intended, instead of Brief.
- macOS merge editor: ⌥⌘↑ and ⌥⌘↓ move between conflicts, ⌘↩ no longer races Commit, a file open in its own window
  isn't loaded again in Working Copy, and the AI buttons follow the AI setting.
- macOS: Reset, Cherry-pick, Revert and Apply Patch say which worktree they act on; Remove Worktree counts the changes
  again before asking; Undo works on the SSH host git settings; Settings keeps up with the History menus.
- macOS: edit mode no longer rewords a merge row on double-click, Discard File is off for fully staged files, Working
  Copy says why a diff can't be read, and the Rebase sheet stops "Checking for conflicts…" when the check fails.
- macOS: Create Pull Request needs a title and base, the Branches count includes stacked-on branches, Stashes updates
  while filtering, a renamed group stays selected, Clone doesn't default to an SSH folder, and Activity keeps its place.
- macOS: opening an added worktree through the shared daemon edits that worktree when its main repository wasn't open
  yet.
- macOS: a conflicted file shows the merge editor in Working Copy even when Working Copy hadn't been opened yet in that
  window.
- macOS: Checkout #N, Open on GitHub and the CI marks use the selected worktree and its branch, not the opened one.
- macOS: Quick Open keeps the row picked with the arrow keys while row details load, so Return opens that repository.
- macOS: a repository whose status can't be read (such as a worktree whose main repository was removed) is no longer
  read again and again while it is listed.
- macOS: the Reflog and Range Diff details no longer offer Edit Message…, which dropped the message on Save.
- macOS: a conflicted file fixed in another editor reloads in its unedited merge editor, so Save doesn't write the old
  markers back.
- macOS: closing a repository window or tab, or quitting, asks before losing unsaved Result edits in a merge editor, as
  the separate merge window does.
- macOS: with another worktree selected, the window subtitle and the branch switcher's recent list use that worktree's
  branch.
- macOS: New Tab from a File History or Blame tab in its own window adds the Repositories tab to that window.
- macOS: Commit and Push stops with "Can't read the status" when it can't read the branch state, instead of beeping or
  pushing without asking.
- macOS: Settings > Appearance also follows Highlight Ancestors, Monospaced message, Show Avatars, Stack diffs, File
  tree and Fold Merged Branches when they change from the View menu.
- macOS: Shift+Return in the History search field searches a newly typed query instead of moving within the old results.
- macOS: Combine N Commits… refuses commits off HEAD's first-parent line instead of quietly combining fewer.
- macOS: picking from Choose from My GitHub Repositories in the Clone sheet updates the sheet at once, as typing a URL
  does.
- macOS: the path picker says "Choose a host to see its folders." for a host name with a space or "/" instead of showing
  empty columns.
- macOS: the Commit ▾ menu no longer gains a separator after every commit.
- macOS: the Commit button is off while a commit runs, so a second ⌘↩ can't start another one.
- macOS: closing the AI panel stops the claude or codex run, so the activity indicator stops and nothing is cached.
- macOS: typing in Working Copy's Description field can be undone with ⌘Z.
- macOS: the merge editor's Resolve with AI and Explain with AI no longer beep when the AI question is cancelled.
- macOS: closing a separate merge window without resolving puts the merge editor back in Working Copy if that file is
  shown.
- macOS: Take Current File, Take Incoming File and Mark Resolved also heed unsaved Result edits in the file's separate
  merge window, and taking a side closes that window.
- macOS: the Stashes, Reflog and Range Diff details show ref names as plain text, not links that did nothing.
- macOS: author and committer names in the Stashes, Reflog and Range Diff details are no longer links that did nothing.
- macOS: the signature popover in the Stashes, Reflog and Range Diff details says where the signature was checked, as
  History's does, including on SSH hosts.
- macOS: Blame scrolls sideways for long code lines instead of cutting them off with "…".
- macOS: Working Copy's Description keeps its own ⌘Z steps, so an inserted trailer can be undone and filling the field
  no longer wipes undo for Summary, the merge editor's Result or Repository Settings.
- macOS: switching a tab to another repository from the toolbar's repository list asks before losing unsaved Result
  edits in a merge editor, as closing the tab does.
- macOS: Working Copy's List, Tree and File Name layout belongs to each tab and worktree; changing it in one changed
  the others' lists while their buttons showed the old choice.
- macOS: Take Current File, Take Incoming File and Mark Resolved leave the diff bar when Working Copy's list empties,
  such as after `git merge --abort`.
- macOS: Repository Settings > Hooks · LFS opens and edits the hooks of the main repository's `.git` in an added
  worktree, where git looks for them, instead of a missing `.git/hooks` folder.
- macOS: with another worktree selected, the Merge… and Rebase… chooser leaves out that worktree's branch instead of
  the opened one's.
- macOS: "Pulled into …" and "Merged into …" in the status bar name the selected worktree's branch.
- macOS: with several History commits selected, right-clicking a file in the changed-file list offers File History.
- In the GTK4 app's sheets, Return runs the operation (Pull, Fetch, Publish, Reset…) even when a drop-down or a radio
  button has the focus, as on macOS; it opened the drop-down or picked the radio instead.
- In the GTK4 app, opening a repository that is already open in a new window, or switching to a worktree whose
  repository is open, brings its tab forward instead of opening a second one.
- The GTK4 app's separate merge window saves, marks resolved and asks AI in the worktree it was opened for; after
  selecting another worktree, Save wrote its Result over the same file there.
- The GTK4 app's Working Copy keeps Amend and the file filter per worktree; Amend ticked in one worktree stayed on in
  another, so Commit amended that worktree's last commit.
- The GTK4 app keeps each commit, push, discard, stage and confirmed operation in the worktree it started in: Commit
  and Push could push another worktree selected during the commit (or skip its Publish sheet), and Discard, Add to
  .gitignore, Take Current/Incoming, Fetch…, Pull…, Push…, Checkout…, Merge, Rebase, Cherry-pick, Delete Branch,
  Apply Patch, History's edit mode, Export Archive…, Save as Bundle…, Fetch from Bundle…, the submodule sheets and
  Undo… could act on the one selected meanwhile.
- macOS: Activity > History lists and undoes the operations of the worktree being edited in the window; with another
  worktree selected, Undo to Here undid that worktree's operations while listing the opened one's.
- The macOS app shows “Couldn’t Commit” and the hooks output even when you leave Working Copy or select another
  worktree while the hooks run; they were lost, so a rejected Commit and Push silently didn't push.
- Selecting worktrees quickly ends on the last one picked, in the macOS and GTK4 apps; one that took longer to open
  could take over after a later pick.
- macOS: the toolbar search field keeps its text when History reloads after a fetch or commit.
- macOS: Combine with Older Commit… no longer marks a commit whose parent is a merge for squashing onto the merge.
- macOS: J and K step through every hunk in History, Stashes and File History diffs; they stopped at the same one.
- macOS: a merge editor in its own window no longer shows the Open in a Separate Window button, which did nothing
  there.
- macOS: folders collapsed in Working Copy's tree stay collapsed when the status reloads.
- macOS: Revert under the ignore patterns in Repository Settings and Settings > Git puts back the saved text; it did
  nothing.
- macOS: Return no longer confirms removing a remote; Delete is red and Return cancels.
- macOS: Return no longer confirms forgetting an SSH host with repositories or folders to scan.
- macOS: opening an open repository through a symbolic link or /tmp instead of /private/tmp brings its tab forward
  instead of opening a second one.
- macOS: New Worktree on an SSH host sends a ~ folder as typed instead of turning it into this Mac's home folder.
- macOS: dragging a favorite branch while the sidebar is filtered no longer reorders the favorites wrongly.
- macOS: changing One Line per Commit or History Items keeps several selected commits selected.
- macOS: dragging one of several selected History rows moves the commit you grabbed, not the newest selected one.
- macOS: in History's edit mode, undoing every change clears the conflict marks on the rows.
- macOS: ⌥J and ⌥K move to the next or previous file in History's diff when its files aren't stacked; they only
  beeped.
- macOS: when a commit's changed files can't be read, History says it couldn't load the diff instead of No changes.
- macOS: Find in a diff highlights matches in the file headers' paths too.
- macOS: diffs scroll sideways as far as their longest line, no more and no less; lines past about 3,000 points were
  cut off.
- macOS: Commit ▾ > Commit and Push is off when nothing is staged or the message is empty, as Commit is.
- macOS: Working Copy no longer says No changes before the first status has loaded.
- macOS: Reflog's Compare with Current HEAD compares with the HEAD of the worktree shown, not the opened one.
- macOS: Save as Bundle's Save… is off while Only commits after: is chosen with an empty field; it quietly saved all
  history.
- macOS: the password and passphrase dialog shows its buttons in the system language instead of always in English.
- macOS: when the Clone destination already exists, the Clone sheet comes back with what you typed.
- macOS: Add to Favorites and Remove from Favorites in the Repositories list apply to every selected row, as Fetch,
  Group and Color Label do.
- macOS: Activity's History tab updates its Undo list after each operation in the repository windows.
- macOS: Japanese messages that point to a settings page name it as 「設定 > …」, as the menu calls it.
- macOS: clearing the sidebar's branch filter closes again the headings that the filter opened.
- macOS: with another worktree chosen to work in, Repository > New Branch…, New Tag…, Rename Branch…, Set Upstream…,
  Compare with Previous Version, Create Pull Request… and the Merge…/Rebase… titles use that worktree's branch, not
  the opened one's.
- macOS: with another worktree chosen to work in, Switch Branch… lists the branch checked out in the main worktree and
  no longer offers the current worktree's own branch.
- macOS: a tab's ↑ and ↓ marks follow the branch of the worktree chosen to work in, like its ● mark.
- macOS: Repository Settings writes the git settings of one choice in turn (such as Signing's two keys or a Fetch-only
  pull), so the second no longer fails on the config lock and leaves it half applied.
- macOS: a Repository Settings change that git couldn't write no longer adds an Undo step.
- macOS: Keys and Values' Restore line goes away after any other settings change, so Restore no longer undoes that
  change instead of the removal.
- macOS: undoing or restoring the removal of a key with several values, such as two remote.origin.fetch, puts all of
  them back instead of only the last.
- macOS: warnings flashed in the status bar show an orange triangle instead of a green one.
- macOS: an authentication or host key failure in a daemon or SSH repository no longer offers Open in Terminal, which
  only beeped there.
- macOS: changing From: in the Fetch sheet keeps the line naming the worktree.
- macOS: Publish is off while Name there is empty; it closed the sheet and did nothing.
- macOS: after the Clone sheet comes back because the destination exists, typing a bundle path still switches it to
  bundle mode.
- macOS: adding a folder to scan or changing its Search Depth while a scan runs takes effect when that scan ends.
- macOS: History's edit mode stays open with your plan when Apply Changes fails, and closes once the rewrite has run.
- macOS: with unsaved edits in the merge editor under Working Copy, Mark Resolved saves Result first, and Take Current
  File or Take Incoming File asks before losing them.
- Cherry-pick and Revert of several commits run as one git cherry-pick or git revert: when one stops on a conflict,
  Continue and Skip go on with the rest, and Abort goes back to before the first, as in git; the rest were silently
  dropped.
- macOS: the submodule sheets say which worktree they act on, and run there even if another worktree is selected
  meanwhile; Fetch from Bundle names the worktree as Fetch… does.
- macOS: with another worktree selected, the Submodules marks and Open in New Window follow that worktree, not the
  opened one.

### [0.2.9] - 2026-11-04

The GTK4 app for Linux catches up with the macOS app, and large repositories are supported: shallow, partial and
sparse clones, background maintenance, and History searches that fetch only what they need. Several branches merge in
one commit, commits are exported as archives or bundles, submodules are managed from the sidebar, and every
confirmation and History edit now asks in one sheet that shows what moves and what will conflict.

#### Added

- The GTK4 app for Linux catches up with the macOS app: tabs, worktrees, the Repositories window and Quick Open,
  Repository Settings and the machine settings (Programs, Environment Variables, SSH Hosts), Stashes, Reflog, Range
  Diff, patches, History's edit mode, GitHub (CI marks, avatars, pull requests), AI, and large repositories
  (shallow, partial and sparse clones, git maintenance, the Clone sheet's choices). `docs/frontends.md` lists what
  each front end has.

- Several branches, tags or commits selected in the sidebar or History merge in one commit, as `git merge a b c` does,
  from a sheet showing their order and the conflicts expected for each; it offers to merge them one by one when git
  couldn't. `amonhen merge-check a b c` checks the same.
- In a shallow clone, History ends with a band to fetch any number of older commits, up to a date, or all of it, and
  boundary commits are marked as having parents not fetched. `amonhen log` notes where the history stops.
- Repository Settings has a Fetching and Folders page: fetch more of a shallow clone, fetch all contents of a partial
  clone (git 2.36), and choose the folders of a sparse checkout from HEAD's tree (git 2.25). The git line, the
  sidebar and Working Copy show when a repository is shallow, partial, sparse or registered for maintenance.
- In a partial clone, a History search with `contents:` says how many contents it will fetch and fetches them in one
  go (git 2.29); past 50,000 it asks to narrow the search or fetch all contents. `amonhen status` notes shallow,
  partial and sparse repositories.
- The History search takes `after:` and `before:` (a date or `"1 year ago"`), and `contents:` with `path:` looks
  only at that path's changes. A `contents:` search shows the matches and its progress as it goes.
- Fetching and Folders has a switch to keep the repository maintained in the background (`git maintenance start`,
  git 2.31); the app then leaves writing the commit-graph to it. Turning it off lets git's automatic maintenance
  run again.
- The Clone sheet chooses what to download (everything, file contents when needed, or recent history only) and can
  check out only some folders, picked right after the clone (git 2.27). `amonhen clone` takes `--filter`, `--depth`
  and `--sparse`.
- Export Archive… writes a commit's files as a zip, tar.gz or tar file (`git archive`), optionally with a top folder
  and only one folder, from the Commit menu, History and the sidebar's branches and tags. `amonhen archive` does the same.
- Save as Bundle… writes branches and tags, or the whole repository, into one file that git can clone or fetch from
  (`git bundle`), optionally only the commits after a base. `amonhen bundle create` does the same.
- The Clone sheet clones from a bundle file (typed, or Choose Bundle…), choosing the branch to check out when the
  bundle has no HEAD. `amonhen clone` takes a bundle file and `-b`.
- Fetch from Bundle… puts a bundle's branches under a name of their own, like a remote's, also into a repository on an
  SSH host. `amonhen bundle fetch` does the same.
- Submodules: the sidebar marks those not checked out, at another commit than the one recorded, with changes inside or
  in conflict, and their menus add, update (optionally recursive or to the branch they follow), change the URL or the
  branch to follow, sync and remove them (keeping `.git/modules` unless asked). The Clone sheet can check them out
  too. `amonhen submodule` and `amonhen clone --recurse-submodules` do the same.

#### Changed

- On Linux, `amonhen open` (and `amonhen <path>`) opens the repository in the GTK4 app, as a tab of the running app.

- The GTK4 app now needs libadwaita (`libadwaita-1-0` on Ubuntu). Its tabs can be dragged out into a new window on
  Wayland too, and it follows the system's light or dark style.

- The Fetch sheet chooses the remote: all of them, as before, or one. The core's `fetch` takes the remote too.

- History's edit mode and the Rebase sheets say which stacked branches move with the rewrite or stay and why, which
  are pushed and will need Force Push, and which commits are expected to conflict (git 2.40 for the forecast). The
  Rebase sheets draw the branch before and after, and "Also move the stacked branches" decides for that rebase or
  edit only. `amonhen rewrite-preview` prints the same.

- Push and Pull each confirm in one sheet: a diverged Push offers Pull… or Force Push there, and Pull with no upstream
  picks the remote branch there. Repository > Pull With is gone, and Create Pull Request goes on after its push.
- Force Push replaces the remote branch only if it is still at the commit the sheet showed, so commits fetched in the
  background are not lost unseen.
- Push is disabled when there is nothing to send, and a conflict opens Working Copy at the first conflicted file
  instead of an alert.
- Every confirmation is a sheet on the window, and an operation asks the same way from the toolbar, a context menu
  or a drag. Fetch, Checkout, Cherry-pick, Revert, Apply and a sidebar Merge or Rebase now ask first; Reset picks
  Soft, Mixed or Hard in its sheet.
- The menu bar and context menus use one name per operation (New Branch from, 8-character short IDs, translated
  Checkout, Merge and Push items). The View menu is History's display menu, now with the commit detail toggles.
- Everything in the menu bar is also in the window: a sidebar toggle and a More menu (≡) in the toolbar, Rebuild
  History under Refresh, Go to HEAD in History's bar and a find button above the diff.
- Commits are edited in History's edit mode: choose Keep, Edit Message, Combine with Older, Drop or Stop to Change
  Files for each commit and drag them into order, then Apply Changes runs it all as one rebase with one Undo. It also
  reaches past merges, which are recreated as they were (git's `--rebase-merges`).
- The in-progress, failure and connection bars moved to a status bar at the bottom of the window, which also says what
  a finished operation did. A failure clears once a later operation succeeds.
- Working Copy's file context menu stages, discards, copies the path and opens the file.
- Deleting several branches offers Delete Anyway for those with commits nowhere else. Only what can't be undone has a
  red button, and failures read "Couldn't …".
- Edit Message and Squash use git's own `reword` and `squash`, so notes follow `notes.rewrite.*` as in git, and
  Edit Message runs the pre-commit and commit-msg hooks as git does. Lines starting with `#` are still kept.

#### Fixed

- Adding, renaming and removing remotes and changing their URLs work over SSH and through the shared daemon; they
  failed to reach the engine.

- The GTK4 app's Undo button asks once; it opened the question twice, so one stayed after undoing.
- A `.mailmap`, or a mailmap setting (`log.mailmap`, `mailmap.file`), changed while the engine was reading the old
  one could be missed until it changed again; the names now follow it.
- With newer GTK (4.24), the GTK4 app's sidebar shows Working Copy's change count and the sparse checkout mark.
- Activity no longer says “daemon” twice for a repository opened through the shared daemon, nor “(daemon)” for an
  SSH host connected directly.
- Saving patches no longer fails with “No such file or directory” when another save runs at the same time in the same
  repository over SSH or through the shared daemon.
- When nothing sets how to pull (`pull.rebase`, `pull.ff`) and the branch has diverged, the Pull sheet says git won't
  pull (git 2.33.1 and later refuse) and asks for Merge or Rebase for this pull, instead of promising a merge that then
  failed. No settings are written.

- Errors from an SSH host opened through the shared daemon are in the language of the app or CLI that asked, not the
  daemon's. So are the SSH host engine and why a connection was lost, in Activity and `amonhen daemon status`.
- A lost connection to an SSH host names that host instead of "the daemon", and the bar no longer repeats it.
- Reconnecting after a lost connection shows the page that was open, not History.
- With "Open repositories through the shared daemon" on, Repositories and Open Recent list a repository once, not
  again as `daemon:…`.

- `amonhen diff` with a name that is neither a commit nor a file in the working tree stops with an error, as git does,
  instead of printing an empty diff. Paths after `--` are taken as paths.

- A repository over SSH or through the shared daemon no longer stops answering when it changes just as its mailmap is
  checked; the connection waited forever for an answer it had stopped reading.

- Combine with Older Commit… on a single commit in History marks it in the edit mode; it opened with nothing marked.

- Working Copy puts a check box back when staging fails or changes nothing; it kept showing the click.

- The merge editor keeps the bands to Current and Incoming for a conflict edited by hand in Result; they disappeared
  and its side showed it as resolved.
- The change count in Repositories and the toolbar's repository list includes new files, as Working Copy does; it
  left them out.

- Working Copy reloads the shown diff and line counts when an already changed file changes again, and keeps the
  selected lines when the diff is the same.
- Working Copy keeps several selected files selected when it reloads (after a file is edited elsewhere, for
  example), dropping only those no longer listed; it went back to one file.
- History opens in a shallow clone: it stopped with "Commit … not found" at the boundary, and now ends there as
  `git log` does. After `git fetch --deepen` or `--unshallow`, History is read again with the older commits.
- Starting a merge, rebase, cherry-pick, revert, pull or patch while another one is stopped no longer reports a
  conflict; it says which one is in progress, to continue or abort first.
- Error messages show what went wrong: the macOS app showed only "AmonhenError error 1" (a locked index, for
  example), and a git command that failed without a message showed a raw exit status; it now names the command, its
  exit code, or the branch or commit that doesn't exist.
- Diffs no longer come up empty after many fetches while a repository stays open (each partial clone fetch adds a
  pack, and only about 32 new packs could be read).
- Editing the message of an empty commit (made with `git commit --allow-empty`) finishes and keeps the commit; with
  older git (2.23, for example) the rebase stopped on it.

### [0.2.8] - 2026-10-30

Commits can be saved as patches and patches applied, a branch compared with its version before a rebase (Range Diff),
and conflicts that git's rerere already resolved are told apart, with rerere's switches in Settings. Settings > SSH
Hosts gathers each host's connection, engine, environment and git settings, and repositories on Windows SSH hosts
open too. People are shown by their mailmap names, tag signatures are verified, notes survive Squash and Split, and the
git chosen in Settings is used by the CLI and the VSCode extension as well.

#### Added

- Conflicted files with no conflict markers left (git's rerere applied an earlier resolution, or another editor
  resolved them) are marked in Working Copy, counted in the band with a button to mark them all resolved, and open in
  the merge editor with both sides from the index. `amonhen status` notes them too.
- Range Diff compares a branch with its version before the last rebase, History edit or amend (found in its reflog),
  or any two ranges, as `git range-diff` does: from a branch's menu, Repository, Reflog or two commits in History.
  `amonhen range-diff` does the same (`--previous`, `--json`).
- Repositories on an SSH host running Windows open in the app and the CLI (`ssh://host/C:/src/repo`), whichever
  default shell its OpenSSH server uses; the app bundles the Windows engines and sends them by scp.
- Tag signatures are verified and shown like commit signatures: a Tag line per tag in the commit details, the
  signature on each Merged Tag line, and a mark on signed tags in the sidebar. `amonhen refs --verify` adds them too,
  and `amonhen show` prints the commit's signature and its tags' and merged tags'.
- People are shown by their names in the repository's mailmap (`.mailmap`, `mailmap.file`), as `git log` does, in
  History, the details, File History and the CLI, with the recorded name and email under them in the details when
  they differ. `author:` matches the mapped and the recorded name. git's `log.mailmap` turns it off, on the Author and
  Signing page with the mailmap file; changing either shows the names again without rebuilding History.
- The Merge section of Repository Settings and Settings > Git has git's rerere switches: reuse how conflicts were
  resolved before (`rerere.enabled`) and stage the files it resolves (`rerere.autoUpdate`).
- Every change in Repository Settings and Settings > Git can be undone and redone with ⌘Z / ⇧⌘Z while the page is
  shown, apart from the repository's Undo (⌥⌘Z).
- A local branch that isn't checked out can be fast-forwarded to its upstream from its sidebar menu (as of the last
  Fetch, undoably), and its menu also offers Push when it is only ahead and Publish when it has no upstream. The core
  and CLI get the same as `fastForward`.
- Settings > SSH Hosts: the SSH hosts Amon Hen knows, with + and −, and for each its connection (state, Check
  Connection, which engine to run there and whether to send one, which keys verify commit signatures, the folders to
  scan) and the pages of Settings > Git for every repository on that host, including the git that runs there, kept in
  the host's `~/.config/amonhen/settings.json` so the host's CLI and VSCode extension use it too.
  Settings… on a host in Places opens it, and on This Mac opens Settings > Git.
- Settings > SSH Hosts has an Environment Variables page for each host: the engine's environment there, and a switch to
  start the engine with the host's login shell's environment, for the PATH and `GIT_*` set in `~/.profile` and the
  like (the amonhen on the PATH is looked for in that PATH too).
- Paths on an SSH host (Other… for a program or the engine, + for a folder to scan, Choose… when adding a repository)
  are chosen in a sheet that shows the host's folders in columns, like the Finder (arrow keys, typing a name, ⏎), with
  git repositories marked; Windows hosts' drives and `\` paths work too. The core and CLI get `listDirectory`, which
  lists a folder on the machine that runs git.
- The engine runs on an SSH host without a repository (`amonhen --stdio` with no path) to read and write that
  machine's git settings, and `ssh://host` without a path connects to it. The core and CLI get `programs`, which lists
  the git, gpg, gpgsm or ssh-keygen found on the machine that runs git, with versions, and `excludesFile` /
  `saveExcludesFile` for the user's global excludes file.
- Commits can be saved as patches (one file per commit as `git format-patch` does, or one mbox) from History's
  context menu and Commit > Save as Patch…, and patches applied with Repository > Apply Patch… or by dropping them on
  the window, after showing what they change: mail with `git am`, plain diffs with `git apply`. A stopped am says why
  in the banner, with Retry with 3-Way Merge. Both can be undone; the CLI gets `format-patch`, `am` and `apply`.

#### Changed

- Through the shared daemon, Settings > SSH Hosts and the Activity window show the engine running on the SSH host,
  as they do for a direct connection.
- Whether commit signatures in an SSH host's repositories are verified with this Mac's keys or the host's is chosen
  per host, in Settings > SSH Hosts (hosts without a choice keep the previous setting).
- Settings > Git > Environment Variables (it was Performance > Terminal Environment) shows the login shell's variables
  with their values, PATH-like ones place by place, and lets you check more to pass to git and ssh. A change applies at
  once, to the shared daemon too, and `HOMEBREW_*` is no longer taken unless checked. `AMONHEN_PASS_ENV` does the same
  for the CLI and the extension.
- The git chosen in Settings > Git > Programs is kept in `~/.config/amonhen/settings.json`, so the CLI, the shared
  daemon and the VSCode extension on the Mac use it too. If it isn't there, operations stop with an error instead of
  running the PATH's git.
- In Keys and Values, the column you can write shows plain text, with a field outline only on hover or for the one
  selected row.
- Push reads Publish for a branch without an upstream, and asks which remote to create it on (following
  `branch.<name>.pushRemote` and `remote.pushDefault`) and its name there, instead of always pushing to origin.
- Files and folders on this Mac are chosen in the same column sheet as on an SSH host instead of the Open panel:
  repositories are marked, a path can be typed, and folders like `/opt/homebrew/bin` are one menu item away.
- History's Edit (Stop Here) stops as git's `edit` does: changes staged while stopped are amended into the commit on
  Continue, and with `notes.rewriteRef` its notes also go to the commits added while stopped.

#### Fixed

- A `git am` in progress was shown as a rebase.
- Undo of a branch-moving operation no longer fails with "not uptodate" right after undoing an applied patch.
- The Undo confirmation says what it puts back: undoing Discard or an applied diff restores the files and no longer
  claims to move HEAD and the branches.
- The toolbar's Undo follows operations recorded outside the app (the CLI, the VSCode extension) without a Refresh.
- Split and Edit in History are recorded for Undo once after Continue; they could be recorded twice.
- In Japanese, "author" is 作者 everywhere, as in the docs: the Author and Signing page and its Author section, and
  the author in the commit details, showed 作成者.
- The CLI finds the repository without running git, so when the chosen git isn't there it can still read and
  choose another (`setMachineGit`); only what runs git stops.
- Check Connection when adding an SSH connection no longer reports a failure when the host has no `amonhen` on its
  PATH.
- On a Mac without the Command Line Tools, Amon Hen no longer runs `/usr/bin/git`, which only brings up the install
  dialog; it says git isn't installed instead.
- SSH hosts whose login shell is tcsh, csh or fish connect: the commands sent there now run in sh.
- Launched from a terminal or another app, the app read the login shell's PATH as a nested shell, so a shell set up
  to build PATH only at the top level gave the system's git instead of the one a terminal uses. Variables git uses that
  the app inherited from what launched it (`GIT_EDITOR`, for one) no longer reach git unless the shell sets them.
- Dropping a branch on one checked out in another worktree no longer fast-forwards it behind that worktree's back;
  the menu offers to open the worktree instead.
- With git older than 2.34, an SSH-signed commit no longer makes git crash during signature checks, which lost the
  marks of the commits checked with it and the commit's notes and line counts; its signature shows Can’t verify.
- With git older than 2.26, which doesn't report the trust level, a good GPG signature showed Untrusted key; the trust
  now comes from the signing key's validity.
- In a repository on an SSH host checked with this Mac's keys, a commit's details again get their notes, line counts
  and signature, and the commit selected on opening is no longer verified with the SSH host's keys.
- A message template file for a repository on an SSH host is chosen on the host, not on this Mac.
- A commit's details grow to fit a long message in SSH hosts' repositories too: they could stop short, by up to
  the height of the changed files list, and leave the rest to scroll.
- A message template file chosen for a repository now wins over a shared template chosen for all repositories, in
  Repository Settings and the commit box.
- None for a repository's message template now cancels a shared template chosen for all repositories.
- Squash into Parent and Squash N Commits keep the notes of the commits folded in, as `git rebase -i` does when
  `notes.rewriteRef` is set: they used to keep only the oldest commit's.
- Split moves the commit's notes to the last of the new commits, as git's own steps for splitting a commit do (`edit`,
  then `reset HEAD^`); they were lost.
- History starts from HEAD, branches, remote branches and tags, as the sidebar does (`git log --branches --remotes
  --tags HEAD`): commits kept by other refs, such as git notes' or `git maintenance`'s `refs/prefetch`, no longer show.

### [0.2.7] - 2026-10-26

Settings is reorganized around what Amon Hen decides, and Repository Settings is redone as a page in the sidebar, with
Settings > Git as the same screen for all repositories; tags can be pushed to and deleted from remotes. Merges, pulls
and rebases show which files would conflict beforehand, File History opens in a tab and is much faster in large
repositories, and SSH hosts no longer fail with "Permission denied (publickey)" after the Mac restarts.

#### Added

- Tags can be pushed to a remote, deleted (undoably), replaced on a remote and deleted from a remote from their
  context menu in the sidebar, one at a time or several selected together.
- Fetch also reads which remote has which tag, and a tag row is marked when no remote has it or a remote has a
  different tag with the name; its tooltip and popover list every remote, and the menu offers only what applies.
- File History can list merge commits too (Show Merge Commits in the list's context menu, or Settings > Appearance >
  File History); they stay out by default.
- Blame colors the code by syntax, as diffs do.
- View > Show Tab Bar (⇧⌘T) shows the tab bar of a window with one tab, so its tab can be dragged into another
  window, and Window > Move Tab to Window ▸ moves the front tab into the window chosen (for File History and Blame,
  the repository's window comes first).
- File History marks where the file was renamed with a heading for each name, and its commits have a context menu
  (Show in History, Blame at the commit, copying the ID or path), with Return and ⌘C.
- The Pull sheet, the Merge… and Rebase… sheets and a branch's popover show which files would conflict before you
  merge, pull or rebase (git 2.38 or newer; for rebase it is an estimate). `amonhen merge-check` does the same.
- Each repository's Settings chooses whether a rebase or a History edit moves the branches stacked under the current
  one along (git's `rebase.updateRefs`; git 2.38 or newer).
- Pull follows git's own settings (`pull.rebase`, `pull.ff`), as `git pull` in a terminal does: the Pull sheet starts
  from "Follow git's settings" and says what those settings choose; Repository > Pull With picks another way once.
- Settings is reorganized around what Amon Hen itself decides, in a resizable window (960 wide by default): General
  (bookmarking, automatic fetch, the external editor), Git, Accounts (GitHub), Appearance, Diff & Merge, AI and
  Performance (commit-graph, fsmonitor, the shared daemon, the shell environment). Nothing folds away, and help is a
  few words where a row needs it. There is no Advanced tab.
- Settings > Git is the Repository Settings screen for all repositories on this Mac: the same pages and rows, writing
  `git config --global`. Commits holds the shared template library; Working Copy has Ignore everywhere
  (`core.excludesFile`); Programs chooses git, gpg, ssh-keygen and gpgsm the same way (Automatic, the ones found with
  where they come from and their version, or a path). The git line under a repository's Settings list opens it.

#### Changed

- The app, the shared daemon and an SSH host's engine work together only when they are the same build (a hash of the
  sources), not when a hand-raised protocol number matches; `amonhen --version`, `amonhen daemon status` and Activity
  show it.
- A development build looks for the engines to send to SSH hosts and to run as the shared daemon only inside its own
  checkout, and the shared daemon is not started from an engine of another build.
- Opening an SSH host uses an engine of the same build already placed there (by another Mac, the VSCode extension or
  `amonhen install`) without sending one, and still opens when the engine here is of another build.
- The History cache is kept per format, so builds that run side by side (and share the shared daemon's cache folder)
  don't rewrite each other's file; the first launch of this version builds History once more.
- The shared daemon runs git and ssh for each app, CLI or VSCode window with that one's environment (`PATH`, the SSH
  agent, `GIT_*`, the app's password dialog), not with that of whichever started the daemon first.
- Repository Settings is a page at the end of the sidebar's Workspace instead of a sheet (⌘; still opens it), redone
  from the person using it: rows ask in plain words with a sentence of help under each, show the value in effect with
  where it comes from (git's default, your settings, this repository) and ↩︎ to stop overriding, and ⓘ for the key and
  file. Its pages are Author and Signing, Commits, Operations (pull, fetch, rebase, merge), Branches (the default
  branch, which Branches measures the others against: detected, or chosen for repositories without one), Working Copy
  (line endings, the executable bit, ignoring in this repository only), Remotes, Hooks · LFS and Keys and Values, with
  the git that runs the repository shown under the list. Settings that only a terminal honors stay in Keys and Values.
- Signing in Repository Settings is one choice, No signing / OpenPGP / SSH / X.509, and the key is chosen from the
  ones found (gpg's secret keys with the ones for your email first, `~/.ssh/*.pub` and ssh-agent's keys, gpgsm's
  certificates; on an SSH host, those on the host), with Other… for a key that is not listed.
- Remotes in Repository Settings are added, edited and removed in one sheet: name, fetch URL, push URL (empty means
  the same), what the URL points to (GitHub, GitLab, a path, an SSH host) and Check Connection (`git ls-remote`, on
  the SSH host for a repository there) before saving. The table shows what each remote points to. The same actions are
  on the sidebar: Add Remote… on the Remotes group, and Rename…, Edit URL…, Prune and Remove… on a remote, and
  Repository > Add Remote… in the menu bar; removing a remote asks the same question wherever it starts.
- Adding an SSH connection on the Repositories screen is a sheet of the same shape as the remotes sheet: host (with
  the hosts in `~/.ssh/config` to pick from), the repository's path (Choose… browses the host), Check Connection (git's
  version and whether the engine is on the host) and Add and Open. A host's context menu offers Remove … from List…,
  which takes its repositories and folders to scan off the list after the same kind of confirmation.
- Settings has a General tab with the external editor: Automatic (the first code editor installed, such as VS Code,
  Cursor or Zed, else the default text editor) or any application. Used wherever Amon Hen opens a file in an editor.
- Hooks · LFS in Repository Settings is two tables with actions: each hook with when git runs it, an Enabled switch
  (the executable bit), the selected hook's script under the table with syntax colors and Edit in the external editor (a menu offers
  the other installed editors), Create Hook… to put a skeleton in place; the LFS patterns from `.gitattributes` with
  Track Pattern… (`git lfs track`) and Remove, and how many files are in LFS.
- In Working Copy, an untracked file's context menu offers Add to .gitignore for the file and for its extension. The
  exclude editor in Repository Settings > Working Copy, and the editor of the repository's own commit template in
  Repository Settings > Commits, fill the window's height.
- Working Copy's commit message box can be made taller: a divider between the commit input and the file list is
  dragged, and its position is remembered; resizing the window changes the file list, not the message box.
- Settings > Git > Commits keeps a library of commit message templates shared by every repository. Repository
  Settings > Commits chooses a shared one (kept as `amonhen.commitTemplate` in the repository's git config; shown under
  the row with Edit Shared Template… and Edit for This Repository), the repository's own (`.git/amonhen/commit-template`,
  which `commit.template` points to so git in a terminal uses it too; edited under the row), or a file with a file
  chooser.
- Keys and Values in Repository Settings is git config as a table like Xcode's Build Settings: one row per key, grouped by the key's dots, with the value in effect
  and a column per source (system-wide, all repositories, this repository, this worktree), the source in effect in
  bold. The column you can write looks like a field; one click edits it (empty removes the key), + adds a key, − and ⌫ remove the
  selected keys, ⌘Z / ⇧⌘Z undo and redo every change made from the table, ⌘C copies the selected rows as `key = value`; search matches keys and values, and every column sorts. Above it, Xcode's switches: Basic
  (the keys the settings pages ask about, set or not), Customized (keys set in this repository, the default) or All (every variable
  git knows, blank when set nowhere), and Combined (the value in effect and its source) or Levels (a column per source).
- Fetch automatically is one interval for all open repositories, in Settings > General, instead of a setting per
  repository.
- `.gitignore` is no longer edited in Repository Settings (it is a file of the repository); `.git/info/exclude` is,
  under Working Copy.
- Fetch removes remote-tracking branches deleted on the remote only when `fetch.prune` is on, as `git fetch` does;
  it used to always remove them.
- File History and Blame open in a tab next to the repository's tab instead of a separate window, with the same
  toolbar, so switching tabs keeps the toolbar and the size (drag the tab out for a window).
- File History opens at once with the newest 100 commits and adds the rest as they are read.
- File History is much faster in large repositories (a file in the linux kernel repository: 86 s → 1.6 s): it
  follows renames itself instead of using `git log --follow`, so history simplification and commit-graph's changed-path
  Bloom filters apply.
- With git's `rebase.updateRefs` set to true (git 2.38 or later), editing History moves the branches stacked under the
  current one along, as a rebase does. Undo puts back the branches a rebase or a History edit moved.
- Undo restores the branches that are not checked out in one transaction, so it changes all of them or none.
- Amon Hen needs git 2.23 or later (also on SSH hosts), as documented now; when a git command fails with an older
  git, the error says so.
- With Settings > Performance > Write commit-graph automatically, a repository that already has a commit-graph gets
  the new commits added to it (and their changed-path Bloom filters) when it is opened and after each fetch or pull.

#### Fixed

- The shared daemon also runs with the front end's environment the git it starts from threads a request splits into
  (diffs of many files, path and content searches, other worktrees' changes), and takes PATH from Windows front ends.
- Opening an SSH host and git over SSH no longer fail with "Permission denied (publickey)" after the Mac restarts:
  the app kept the SSH agent from an earlier login, and now checks the agent each time it starts ssh or git.
- When an SSH host refuses the public key and no SSH agent can be reached, the error says so, and Settings > Performance >
  Terminal Environment shows whether the SSH agent is there.
- Blame of uncommitted contents and File History read the worktree chosen in Working Copy, not the repository's main
  checkout.
- Writing commit-graph in the background also works with git older than 2.27, without the changed-path Bloom filters.
- Undoing a branch rename works for the checked-out branch, and keeps the branch's settings (upstream, description,
  Archive and Favorites).
- Commit details, worktrees, repository settings and the signature's trust level work with git older than 2.36.
- Folders in the sidebar (`/` folders of branches and tags, and the remotes under Remotes) stay open when a fetch, a
  push or another change to the refs rebuilds it, instead of folding up each time.

### [0.2.6] - 2026-10-23

History and Reflog rows can reset the current branch to a commit (Soft, Mixed or Hard, all undoable), and a Reflog
view in the sidebar's Workspace brings back commits left behind by a reset, rebase or amend. Repositories on SSH hosts
or through the daemon get their tab at once and are restored at launch, and Windows gets a one-line installer for the
command line tool.

#### Added

- Reflog in the sidebar's Workspace opens a view of where HEAD has been, marking commits no branch reaches any more
  (left behind by a reset, rebase or amend), with New Branch… and Checkout to bring them back (View > Reflog ⌘5).
- Reset ▸ Soft, Mixed or Hard in the context menu of History and Reflog rows moves the current branch to that commit,
  after asking; Hard keeps uncommitted changes as a stash first, and all three can be undone.
- `scripts/install-engine.ps1` installs or updates the command line tool on Windows in one line (with the attestation
  checked when the GitHub CLI is logged in).

#### Changed

- A repository on an SSH host or opened through the daemon gets its tab in place at once, showing that it is
  connecting; if connecting fails, the tab stays with the error and Try Again instead of an alert, replacing the
  separate connecting window.

#### Fixed

- Clicking the Dock icon with no window open shows the repository page (or brings back minimized windows) instead of
  doing nothing.
- Repositories on SSH hosts or opened through the daemon are restored at launch in their windows and tab order, and
  one that could not connect is still reopened next time.
- Working Copy and Workspace in the sidebar stay folded when you fold them, instead of opening again whenever the
  repository changes outside the app.

### [0.2.5] - 2026-10-22

Stashes moved into the sidebar's Workspace and open as a view of their own, showing where each stash's base stands
against HEAD, with Branch… to make a branch from one. Bookmarks are no longer lost when more than one copy of the app
runs, and branches are nested correctly when the app is opened from a linked worktree.

#### Added

- Stashes shows where each stash's base stands against HEAD (in orange when it is not in HEAD's history), and
  Branch… creates a branch from a stash (`git stash branch`).

#### Changed

- Stashes moved into the sidebar's Workspace as one row that opens a Stashes view: the stashes with their branch, base
  and date on the left, the selected one's changes on the right, and Apply, Pop and Drop (View > Stashes ⌘4).
- The main worktree always stays at the top of the sidebar's worktrees, instead of being folded into "No changes" when
  it has none.

#### Fixed

- The repository page no longer stalls for a moment whenever a scan finishes: when only the counts change, the left
  side is redrawn instead of rebuilt.
- Bookmarks, groups and folders to scan are no longer lost when more than one copy of the app runs (the stable
  release, the snapshot, a development build): each change now rereads the file and writes on top of it.
- Opened from a linked worktree (`git worktree add`), the sidebar still nests a branch under the base it was created
  on when the base has moved on since: where a branch was created is now read through the git backend, which finds
  the reflog in the shared git dir. The remote protocol is now version 23 (the stash bases took it to 22).

### [0.2.4] - 2026-10-22

The sidebar nests branches under the one they are built on more reliably: a branch forked from the default branch no
longer lands under an unrelated pushed branch, and pushed branches are stacked too, so a stack of pull requests shows
as a stack.

#### Changed

- A pushed branch built on another one is nested under it in the sidebar too (↳), so a stack of pull requests shows
  as a stack. Which of two branches is the base is read from where each was created (its reflog) when the base has
  moved on since, and only a branch's own line (first parents) counts, so an integration branch is not nested under
  what it merged.

#### Fixed

- A branch forked from the default branch is no longer nested under an unrelated pushed branch in the sidebar (nor
  "Built on" it in the status popover). A pushed branch counts as the base only where its own commits, not the default
  branch's, are in the history, and the walk stops at the first base it reaches.

### [0.2.3] - 2026-10-21

History rows sit right next to the graph, and what a row shows and the commit order (date or topological) can be
chosen. Push shows what it sends before sending, and a pull request is created from the branch once it is pushed.
HTTPS pushes no longer hang in Git Credential Manager, and the launch crash of 0.2.2 is fixed.

#### Added

- A filter field at the top of the sidebar (⌥⌘F) narrows branches, remotes and tags by name.
- Several branches or tags can be selected in the sidebar: History shows the commits reachable from any of them, and
  the context menu copies their names, or adds them to Favorites, archives or deletes them together (Delete asks first).
- Kotlin (`.kt`, `.kts`) is highlighted in diffs.
- The Push confirmation is a window of its own (movable, resizable) that shows what it sends: warnings worth a look (straight to the default branch, fixup!/WIP commits,
  files that may hold secrets, binaries added), the commits on the left, and on the right the message, files and
  diff of the selected commit or of all changes together, as in the detail column.
- Clicking a commit's CI mark in History, or a pull request's in the sidebar, opens a popover listing its checks
  (result, name, duration), each linking to its log, with a button to the checks page on GitHub. Re-runs are
  collapsed to one row per check: a success hides the cancelled runs before it, and a check that ended cancelled
  shows as cancelled, not failed.
- View > History Items chooses what each row shows besides the subject: branches and tags, author, date, commit ID,
  signature and CI status. With none of author, date and ID, rows are one line.
- View > History Order switches History between date order and topological order, which keeps a branch's commits
  together under the merge that brought them in. The CLI has `amonhen log --topo-order`.
- Both are also in Settings > Appearance and in the menu of the new button at the right of History's bar (and the
  header's context menu), which also has One Line per Commit, Fold Merged Branches and Highlight Ancestors.
- Create Pull Request… is also in a local branch's context menu and in the branch's status popover (once the branch
  is pushed; an open pull request is shown and opened instead), and can start from a branch other than the current
  one. A branch not pushed yet gets the Push confirmation window instead of a plain alert.

#### Changed

- Highlighting picks the language by the shebang when the file name says nothing (`#!/usr/bin/env python3`, `#!/bin/sh`
  and so on), treats a `.h` that looks like C++ as C++, and knows more file names (Cargo.lock, Podfile, BUILD, …).
- Deleting a branch asks first. A branch with commits that are in neither the current branch nor its upstream lists
  them and offers Delete Anyway (git refused and nothing happened before). Dropping a stash asks too.
- In History, each row's message starts right after that row's graph (its dot and the lines passing it) instead
  of at a fixed 120 pt, and the graph column is only as wide as the visible lanes need.

#### Fixed

- Push, Pull and Fetch over HTTPS no longer hang in Git Credential Manager when run from the app. git inherited the
  app's blocked SIGCHLD; git and ssh started by the engine now begin with an empty signal mask.
- The app no longer crashes at every launch after the first (the shell environment's background read ran on the
  main actor's behalf). In 0.2.2, turn off "Take the login shell's environment at launch" to work around it.

### [0.2.2] - 2026-10-20

Fetch, Pull and Push work from the app as they do in a terminal: the app takes the login shell's environment at
launch, so Homebrew's git, credential helpers and SSH agents are found, and a dialog asks for a password or passphrase
when git needs one. Failures say what failed, why, and what to do next, and the bar for a rebase, merge, cherry-pick or
revert says what is going on and where it stopped. The sidebar's branches are arranged for getting around: Favorites,
the current branch with the branches it is built on, branches nested under the one they build on, marks for merged and
gone branches with an explaining popover, and Inactive and Archived folded away. History follows branches that other
programs rebase or commit to instead of going empty, the Pull sheet remembers the chosen way, and Working Copy shows a
file's line endings when they matter. The remote protocol is now version 21.

#### Added

- Working Copy shows a file's line endings beside its diff when they matter: `CRLF → LF` when git converts them
  (`core.autocrlf`, the `text` and `eol` attributes), and a warning when they are mixed or CRLF where checkout would
  write LF, which the next commit or checkout may change for the whole file. The tooltip names the conversion.
  `amonhen diff` prints the same under the file's header, and `--json` has `lineEndings`. The remote protocol is now
  version 21.
- `scripts/install-app.sh --user` installs the app into your own Applications folder (`~/Applications`); `--system`
  picks `/Applications`, and `--dir <folder>` any folder. Without any of them, an update replaces the app where it is
  already installed.
- Branches in the sidebar are arranged for getting around: Favorites (★, in the order you choose by dragging) first,
  then the current branch together with the branches it is built on and the default branch, then the active branches
  newest first with `/` names in folders, and at the end Inactive (no commit for 90 days) and Archived, both folded.
  A branch built on top of another one is nested under it with a ↳ mark, and a branch already merged into the default
  branch shows a merge mark at the right, next to the mark for a gone upstream; clicking those marks opens a popover
  that explains them, shows the branch's upstream, base and last commit, and offers Delete Branch (only when merged)
  and Archive. Add to Favorites and Archive are in a branch's menu; both are kept in
  git's config (`branch.<name>.amonhenPinned`, `branch.<name>.amonhenArchived`), so they follow a rename and go away
  with the branch. `amonhen run '{"op":"branchGroups"}'` gives the same facts (`merged`, `parent`, `pinned`,
  `archived`, `mine`, the tip's `time`).
- Selecting a branch that has diverged from its upstream shows both in History, so the commits ahead and behind are
  in the list.
- `amonhen run` answers a failed command with `errorKind` (`worktreeInUse`, `auth`, `hostKey`, `network`, `rejected`,
  `diverged`, `localChanges`, `locked` or `noUpstream`) and `errorPath` when git's message tells what went wrong, and
  `{"op":"sequence"}` has `info` with the branch, where it is going, the step and the commit it stopped at.

#### Changed

- The Pull sheet starts with the way you pulled last time (merge, rebase or fast-forward only), and the note under it
  always explains the chosen way, not only when your branch and upstream have diverged.
- A failed operation names what failed (Pull Failed, Couldn’t Switch…), says why in plain words instead of git's first
  line, and offers the next step: open the worktree that has the branch checked out, Open in Terminal for sign-in
  problems, Try Again, Pull before Push, Open Working Copy or Set Upstream. Details adds how to fix it.
- The bar shown during a rebase, merge, cherry-pick or revert is taller and tinted, and says what is going on
  ("Rebasing topic onto main · step 2 of 5"), where it stopped and why (conflicts, a commit to edit, a failed command)
  and what to do next.
- Checkout in the sidebar's menu, and Rebase when dropping onto a branch, open the worktree that has the branch
  checked out instead of failing.

#### Fixed

- Fetch, Pull and Push now work from the app as they do in a terminal. Launched from the Finder or the Dock, the app
  had launchd's bare environment, so Homebrew's git, credential helpers and SSH agents such as 1Password were not
  found; it now takes `PATH`, `SSH_AUTH_SOCK`, `GIT_*` and the like from your login shell at launch (cached, refreshed
  in the background; Settings > Advanced > Terminal Environment shows what was taken, reads it again, and turns it
  off; `AmonHen --print-environment` prints it). When git or ssh needs a password, a token or a key passphrase, a
  dialog asks for it instead of failing with "terminal prompts disabled".
- History no longer goes empty when another program (a terminal, an editor) rebases, amends or commits while it shows
  the current branch or a branch picked in the sidebar: it follows the branch to its new commit. Refreshing after such
  a change keeps the scroll position, loads the visible rows before swapping so the list does not blank out, cross-fades
  the two, and leaves the commit details alone when the selected commit is the same.
- A rebase, cherry-pick or revert finished by another program no longer stays shown as in progress (most often when a
  hook or autostash ran at the end).

### [0.2.1] - 2026-10-19

Worktrees live in their repository's tab, listed with their changes under Working Copy and edited one at a time, also
over SSH with many worktrees. Pull, Push, Pop and Undo show what will happen before running, repositories are chosen on
a page that opens as a tab, and the app and engine speak Japanese. New: a GTK4 app for Linux (early), Apple's
on-device model for AI, folders to scan on SSH hosts, and correct line endings in Windows diffs. The remote protocol is
now version 20; the app sends the matching engine to SSH hosts.

#### Changed

- Repositories are chosen on a repository page that opens as a tab (⌘T, ⌘N, ⌘O, ⌘⇧R), instead of a separate window.
  Categories on the left (All, Favorites, This Mac and each SSH host with their folders to scan and other folders,
  groups) and
  each repository once on the right, with its branch, status, last commit and when it was last opened. The toolbar has
  a Repositories button for the page, and the repository name next to it opens the same list to switch the current tab.
- Pull, Push, Pop and Undo no longer run on a single click. Each opens a sheet with what will happen: the commits to
  send or bring in, how Pull integrates them (merge, rebase or fast-forward only, chosen there) and what that does,
  uncommitted changes in the way, the stash or operation being undone. Push suggests Pull when upstream has commits
  you lack, and Force Push lists the commits upstream would lose. The ▾ menus on Pull, Push and Branch are gone.
- Discarding a whole file in Working Copy asks first.
- No more doubled names: History hides its own search field while the toolbar has one, and the window subtitle leaves
  out the branch while the toolbar shows it. The Summary field spans the commit box, with its length next to Commit.
- Sidebar headings show how many items they hold and open or close with a click, so a closed Remotes or Tags no longer
  looks empty.
- On SSH hosts, paths under the home folder are shown with `~`, so `~/prj` and `/home/me/prj` sit in one tree.
- The repository page: folders under a place no longer count repositories already under a folder to scan, the status
  column says "changes n" and explains itself, and empty Groups are hidden.

#### Added

- A GTK4 app for Linux (`amonhen-gtk-<version>-linux-<cpu>.tar.gz`, early): History with the graph, search and
  multi-select, commit details and diffs, Working Copy with line staging, the sidebar and its operations, the macOS
  app's menus in the header bar, the merge editor, Blame, File History, Activity and Settings, opening repositories
  locally, through the shared daemon or over SSH.
- The app is in English and Japanese, following the system language. git's operations and the app's screens keep
  their English names (Commit, Push, Working Copy, History, …) in Japanese too. English counts read "1 commit",
  not "1 commits".
- Messages from the core, the CLI and the daemon are in English and Japanese too, in the language of whoever shows
  them: the app's UI language, the CLI's `LC_ALL` / `LANG`, and for the shared daemon and SSH hosts the language each
  client names when it connects (the remote protocol is now version 20, so SSH hosts need the matching engine; the app sends it).
- Before sending an engine to an SSH host, the app checks that the bundled engine speaks its protocol, and says so
  instead of sending an old one that the connection would then refuse.
- AI can use Apple Intelligence's on-device model (macOS 26): nothing leaves the Mac, and it is the default where
  available. Claude Code and Codex models are chosen from a pop-up (Codex lists the models its CLI knows).
- Worktrees live in their repository's tab, the way git sees them. The sidebar lists them under Working Copy with their
  changes; picking one makes it the worktree you edit (✎ next to the repository name), and Working Copy,
  commits, checkout, Pull, Push, stash and Undo act there, while History and branches stay shared. A branch checked out
  in another worktree takes you to that worktree. New Worktree…, Remove Worktree and Prune Worktrees; confirmation
  sheets name the worktree. In the sidebar the main worktree comes first, then
  the rest by name; worktrees with no changes are folded into "No changes", and worktrees whose folders share a name,
  like `build/aslr/src` and `build/ssp/src`, show as `aslr/src` and `ssp/src`. History labels each
  worktree's HEAD with its name and number of changed files. The current branch is the edited worktree's; branches checked out in
  other worktrees are marked in Branches, History and the branch switcher. Opening a worktree's folder opens its repository's tab, and the repository page no longer
  lists worktrees as repositories of their own.
- Tags are grouped into folders by `/` like branches (`backup/…`, `archive/…`), and clicking a folder or a remote in
  the sidebar opens or closes it.
- Copy a branch or tag name from the sidebar's right-click menu, or from a commit's menu in History.
- `amonhen worktree` (list, add, remove, prune), the same as `git worktree`, through the core (so over SSH too);
  `list --changes` adds each worktree's number of changed files.
- Folders to scan on SSH hosts: the engine on the host finds the repositories (`amonhen find-repositories`). Add one
  from "+" or the Places menu for any host, or from a listed host's menu.
- Choosing an SSH folder: adding a folder to scan or an SSH connection suggests hosts from `~/.ssh/config` and
  `~/.ssh/known_hosts` and lists the folders on the host (double-click to go in, repositories in green), instead of
  only typing the path.

#### Fixed

- Opening an SSH repository with many worktrees no longer fails with "Session open refused by peer", and no longer
  keeps an engine and a connection per worktree: the opened repository counts every worktree's changes in one request
  (`worktreeChanges`), only the edited worktree and the two before it stay open, and past five
  engines per host each gets its own SSH connection (sshd allows only a few sessions per connection; short commands it
  refuses are retried on their own). The engine watches the worktrees' HEAD and index (not their files) and counts
  again only the ones that changed, plus those with changes every 30 seconds. With 59 worktrees on a Linux host this went from 59 engines to
  2; counting all of them takes under a second, and asking again when nothing changed runs no git.
- History no longer crashes when showing only the commits that match a search scrolled down past the matches.
- Fetch, Pull and Push no longer hang forever when git waits for credentials nobody can see: after 120 seconds with no
  output they stop and say so.
- The repository and branch names in the toolbar have a fixed width, so the buttons after them stay put when switching
  tabs.
- The repository list no longer jumps back to the top while scrolling (each row's branch and change count used to
  rebuild the whole list as it arrived).
- Line numbers, file headers and stage checkboxes in diffs no longer come out garbled after the window moves
  between screens of different scale (1x and 2x).
- Building History no longer spins up threads for every few commits. On the Linux engines (musl) it was 5 times
  slower than it should be; on git/git it now takes about 0.1 s instead of 0.6 s there, and about 20% less elsewhere.
- On Windows, the shared daemon no longer flashes a console window each time it runs git or ssh (for example on
  every branch selected in the VSCode extension).
- Working Copy diffs follow git's line ending conversion (`core.autocrlf`, `core.eol` and the `text` and `eol`
  attributes). On Windows, where git checks files out with CRLF, every changed file used to show as entirely rewritten,
  and staging lines put CRs into the index. Discarding lines changes only those lines, leaving the line endings of the
  rest of the file as they were.

### [0.2.0] - 2026-10-17

The first release signed with Developer ID and notarized. The app is reworked around a menu bar, tabs and bookmarks,
with History search and folding, a three-pane merge editor with Magic Merge and AI assistance. New: the `amonhen` CLI,
a shared daemon for the app, the CLI and the VSCode extension, and engines for Linux and Windows.

#### Added

- Menu bar with every operation, a Settings window and a reworked toolbar with a branch switcher and History search.
- Repositories open as window tabs, and the tab layout is restored. Bookmarks with groups, scanned folders, colors and
  Quick Open; repositories opened from outside go to the window used last.
- Working Copy: commit message first, changes with checkboxes, staged lines marked in one diff, a flat file layout,
  line counts, drafts, recalled messages and trailers, `J` / `K` to move between hunks and files.
- History:
  - Fold merged branches, and show commit density beside the scroll bar.
  - Search from an in-memory index of subjects and authors; step through matches, or show only matching commits.
  - Compare two selected commits (also in reverse or from their merge base), act on several commits at once, and
    squash a contiguous range into one.
  - Commit details: all files in one diff with sticky headers and Viewed marks, a file tree, find in the details.
  - GitHub CI state at the right end of each row, and GitHub avatars (both can be turned off).
  - Rebuild History.
- Three-pane merge editor with Magic Merge (safe rules, joining added imports, one conflict at a time) and a banner for
  a merge or rebase in progress.
- Clone from a URL or from your GitHub repositories. List pull requests and create them through the GitHub CLI.
- AI (Claude Code or Codex): translate, explain a hunk, suggest commit messages and resolve conflicts, with prompts you
  can try on sample input.
- Repository settings: identity, branches, remotes, ignore files (with the files each pattern matches), general, hooks
  and LFS.
- Pull can merge, rebase or fast-forward only. Git progress is shown, network operations can be cancelled, and
  failures are reported in a banner.
- Activity window: operations, the undo log, timings, events, the shared daemon's state and the engine used on SSH
  hosts.
- App icon, with a separate one for snapshot builds.
- `amonhen` CLI: open in the app, a graph log, status, diff, show, stage, discard, commit, refs, watch, clone, run,
  undo and Magic Merge for files (`merge-file`, usable as git's mergetool). Everything takes `--json`.
- Shared daemon (`amonhen --serve`): the app, the CLI and the VSCode extension share one History, search index, undo
  log and SSH connection per repository. Unix domain sockets, and named pipes on Windows.
- Engines for macOS arm64, Linux x86_64 / aarch64 (static musl) and Windows x86_64 / aarch64 on each release.
- `amonhen install` places the engine in one location per machine (`~/.local/share/amonhen`, Windows:
  `%LOCALAPPDATA%\amonhen`) and links `~/.local/bin/amonhen`. On SSH hosts the engine is provisioned the way VS Code
  Server is.
- Install scripts that need only `curl`: `scripts/install-app.sh`, `scripts/install-engine.sh` and
  `scripts/install-vscode.sh`. Running them again updates in place, and `install-app.sh` tells you when the app is
  still running the old version.
- The app, the engine archives and the `.vsix` include the licenses and a list of the third-party licenses built into
  them.
- Releases are signed with Developer ID and notarized (snapshots are signed only), and every file carries a build
  provenance attestation.

#### Changed

- The CLI and the daemon are one executable, `amonhen`.
- The engine reports the version it was released as, so `amonhen install` can tell snapshots from releases.
- Licensed under MIT OR Apache-2.0.
- The docs are in English, with Japanese versions in `README.ja.md` and `docs/ja/`.

#### Fixed

- Text and graphs are redrawn when a window moves between screens of different scale.
- On Windows, cancelling stops git together with its children.

### [0.1.0] - 2026-10-16

First release.

- History drawn with Metal: graph layout computed incrementally, a persistent cache so it shows right after launch
  even on very large repositories, ancestor highlight of the selected commit, fisheye and lane animation.
- Commit details in compact, standard or detailed style: signatures checked in batches with a trust indicator, author
  and committer with avatars, merged tags, push state, related commits, rewording in place.
- Sidebar with branches, remotes, tags, stashes and submodules, updated through FSEvents.
- Diffs drawn with Metal and colored by syntax (tree-sitter), with text selection, copy and search.
- Working Copy: file, hunk and line-level staging and commits. Status uses the untracked cache and fsmonitor.
- Checkout, merge, rebase, push and fetch, and commit editing (reword, squash, fixup, drop, reorder, split) with drag
  and drop. Every operation can be undone.
- Conflict resolution per conflict and per file in a three-pane merge editor.
- Repositories on SSH hosts through a daemon on the host.

## VSCode extension

### Unreleased

#### Changed

- History's rows show the subject on one line and the author, date and ID on a second, as in the app; One Line per
  Commit in History Display brings back one line.
- With the engine, Highlight Ancestors dims only the lines and dots that aren't the selected commit's ancestors.
- The commit message gets its own pane above the files and diffs, with a divider to drag and ⤢ to fill the column.
- Range Diff is a page of the main view, as in the app: the pairs on the left, the diff of diffs or the commit on the
  right, and ranges you can edit and compare again. Reflog rows can be compared with the current HEAD.
- The Push sheet opens the diff of all the changes to send, or of one commit, without closing.
- Keys and Values switches between Combined and Levels, as in the app; Levels shows a column per source with the value
  in effect in bold.
- Context menus use the app's names, such as Edit Message…, Combine with Older Commit…, Checkout as Local Branch… and
  Rename…, and items that ask first end with “…”.
- When git or ssh asks for a user name, password or passphrase, VSCode's input box asks, as the app's dialog does;
  fetch, pull and push that needed one used to fail.
- Run in Each Submodule… shows what the command printed in an Output channel.
- Rebasing a branch other than the current one, by dropping a branch on it, says whether conflicts are expected.
- Status uses git's fsmonitor, as the app does (`amonhen.fsmonitor`, on by default).
- `amonhen.writeCommitGraph` writes the commit-graph when a repository is opened and after fetch and pull, as the
  app's Performance setting does (off by default).
- Merge into Current Branch…, dropping a branch on a branch and Merge Branches… ask how to merge: following git's
  settings, fast-forward only, always with a merge commit, or squashed into one commit, as in the app.

#### Fixed

- `scripts/install-vscode.sh` no longer ends with "unbound variable" after installing when the shell's locale isn't
  UTF-8.

### [0.2.6] - 2026-11-09

Working Copy matches the app: one diff with a check on each line and hunk, a file tree, and Commit's ▾ with Commit and
Push, Amend and Fixup. Push, Pull, Fetch, Checkout and Merge ask first and say what will happen, and with the engine
History gets syntax colors, several selected commits, an edit mode, and submodules, subtrees and branch clean-up.

#### Added

- While there are conflicts, the sidebar shows Conflicts with their count; clicking it opens the first in the merge
  editor.
- With the engine, diffs in History and Working Copy and the lines in Blame get syntax colors.
- The sidebar's Working Copy row says "some folders" in a sparse checkout.
- Save as Patch asks for a version (`-v`), naming the files `v2-0001-…` and the subjects `[PATCH v2]`.
- With the engine, Working Copy shows one HEAD → working tree diff with a check on each changed line and hunk;
  clicking a check stages or unstages it, and Space does so for the selected lines.
- Rebuild History (the sidebar's ⋯) discards the engine's History cache and builds it again.
- Rebasing the current branch shows the commits now and after the rebase, with where each branch ends up.
- Switch Branch… finds a branch to switch to, recent ones first; a remote branch gets a tracking branch and a new
  name creates one.
- Export Archive… writes a branch, tag or commit as zip, tar.gz or tar; Save as Bundle… and Fetch from Bundle… use
  `git bundle` with the engine, and Clone from Bundle… clones a bundle file.
- Remotes lists branches under a name that isn't a configured remote, such as ones fetched from a bundle.
- History can show several selected refs, marks each worktree's HEAD, and its ⋯ button picks the order, the row items, folding merged branches and highlighting ancestors.
- With the engine, History's Edit Commits from Here… opens an edit mode: choose what to do with each commit, drag to
  reorder and apply it all as one rebase that keeps merges; Edit Message…, Combine, Drop… and dragging a commit open it.
- History's details collapse each file and mark it Viewed, start lockfiles and large changes collapsed, show line
  counts, list the files as a tree beside the diffs or show one file at a time, and ⌘F finds text in them.
- Working Copy shows line counts per file, discards with ⌫, moves with J/K and ⌥J/⌥K, adds untracked files to
  `.gitignore`, and notes a sparse checkout above the list.
- Commit's ▾ adds Commit and Push (⌘⇧↩), Amend Last Commit, and Fixup and Squash Commit For. What hooks wrote, and why a
  commit failed, open in a readable sheet.
- The commit box counts the summary's length, keeps a draft per repository, starts from `commit.template`, brings back
  recent messages (⌥↑ and ⌥↓ too) and inserts Conventional types, Co-authored-by and Signed-off-by.
- Working Copy's file list filters on paths and shows paths, a folder tree (folders stage and unstage together) or
  file names first.
- With the engine, several commits can be selected in History (⌘- or Ctrl-click, ⇧-click, ⇧+arrows): the details show
  their diff, and the menu cherry-picks, reverts, combines, merges, saves patches, range-diffs two and copies the IDs.
- Clean Up Branches… lists branches merged into the default branch, merged by a pull request on GitHub, or whose
  upstream is gone, and deletes the checked ones; one Undo brings them back (with the engine).
- With the engine, a branch merged by a GitHub pull request (squash or rebase) gets the merged mark, names the pull
  request in its tooltip, and is deleted with one confirmation.
- With the engine, submodules show the app's marks and the URL and branch they follow, and can be added, updated
  (nested ones, or to the branch followed), pointed elsewhere, synced and removed (keeping `.git/modules` unless asked).
- With the engine and a git that has `git subtree`, a Subtrees group lists the folders added with it; Add Subtree…,
  Pull…, Push… and Split… start from the repository, branch and squash used last.
- With the engine, a file's right-click in History's details has Restore This Version… and Restore the Version
  Before…, which put that version in the working tree (and the index if chosen); Undo takes the file back.
- With the engine, Stash can take only the staged changes (git 2.35) or keep them staged, and a Working Copy
  file's right-click has Stash… for that file alone.
- With the engine, a worktree's right-click has Lock…, Unlock and Move…, and a worktree whose folder is gone has
  Locate Moved Folder… (git 2.29).
- With the engine, a submodule cloned by hand before it was added has Move Repository Data into .git/modules, and
  the Submodules group has Run in Each Submodule… (`git submodule foreach`).
- With the engine, a conflict rerere resolved has Forget Resolution on its Working Copy right-click (the markers
  come back), and Repository Settings > Operations has Forget All… for the remembered resolutions.
- With the engine, New Orphan Branch… (Command Palette, the Branches group's right-click) switches to a branch with
  no history, empty or keeping the current files staged.
- With the engine, Delete Ignored Files… (Command Palette, Working Copy's right-click) lists what `git clean -X` would
  delete, folders as one entry, and deletes the ones kept chosen after asking.

#### Changed

- A failure's notification gives its cause (git's `fatal:` and `error:` lines), with Details for the whole output and
  the git command.
- Checkout, Merge, Cherry-pick, Revert and applying or popping a stash ask first, saying what happens (a tracking
  branch, a detached HEAD, uncommitted changes coming along).
- Working Copy's commit box is above the file list, as in the app.
- Without the engine, a fetch or push that prints nothing for 120 seconds (usually a sign-in nobody sees) is
  stopped with an error saying so, as the engine does.
- Push asks first, showing the commits and anything worth a look (the default branch, fixup!/WIP commits, secrets,
  binaries); when the upstream moved on it offers Pull… or a Force Push that replaces only what it showed.
- Pull asks first: how to integrate (git's settings, which it now follows, or merge, rebase or fast-forward only for
  this pull), what that does here, and the commits; with no upstream it asks which remote branch to pull from.
- Fetch asks first, from all remotes or one, and prunes as git's settings say (`remote.<name>.prune`, `fetch.prune`)
  instead of always.
- The extension's own messages, prompts and settings descriptions are in English. Messages from the engine still follow
  VSCode's display language.

#### Fixed

- When the preview before a rebase or History edit can't be read, the confirmation says why instead of leaving it out.
- History stopped answering search and scrolling until the next reload, when an older reload finished after a newer one.
- History's toolbar wraps at narrow widths; Matching Only no longer overlaps HEAD and the commit count.
- The message editor in History (Reword, Squash) is no longer covered by the headings of the diffs behind it.
- A branch merged into the default branch now says so in its tooltip.
- Undo of a deleted branch without the engine also brings back its settings (upstream, archived, favorite), as the
  engine does.
- Remove Worktree counts the worktree's changes again before asking, so files added since the last count are
  mentioned and removed with it, as in the app; before, git could refuse and leave the folder.
- A folder opened through a symbolic link (macOS's `/var`, for one) selects its own repository at startup rather
  than one of its submodules.
- An expanded Submodules, Tags or Stashes group is read again after switching repositories, rather than staying empty.

### [0.2.5] - 2026-11-04

History gains the app's search, Reflog, File History and Blame, and with the engine the sidebar arranges branches,
tags, remotes and worktrees as the app does. Rebases and History edits ask first and say which stacked branches move,
and Repository Settings shows the app's pages of git settings.

#### Added

- History has a search field with the app's syntax (`author:`, `path:`, `contents:`, `after:` …), highlighting and
  moving between matches, and Matching Only; it needs the engine. In a partial clone a `contents:` search first says
  how many file contents it will fetch.
- Merge Branches… in the Command Palette merges several branches or tags in one commit, as `git merge a b c` does,
  showing the conflicts expected for each first; it offers to merge them one by one when git couldn't.
- Reflog, at the bottom of the sidebar, lists where HEAD has been with each entry's commit, marking commits no branch
  reaches; from there a branch, tag, checkout or reset brings them back. It needs the engine.
- History's commit menu has Checkout (Detached) and Reset Current Branch to Here… (Mixed, Soft or Hard; a hard reset
  stashes uncommitted changes first, and Undo puts the branch back).
- With the engine, tags show what the remotes have (as of the last Fetch) and their signature, and their menu pushes,
  replaces or deletes them on a remote and deletes them here (with Undo). Remotes can be added, renamed, pointed
  elsewhere, pruned and removed from the sidebar.
- With the engine, worktrees are listed under Working Copy with their change counts; selecting one makes it the one
  the extension works on. New Worktree…, Remove Worktree… and Prune Worktrees are in the sidebar and History menus.
- File History, from a file's right-click or the Command Palette, lists the commits that changed it across renames,
  with the selected commit's diff of the file beside them. It needs the engine.
- Blame, from the same menus, shows which commit last changed each line of the working tree or of a commit's version.
- With the engine, Branches are arranged as in the app (favorites, the current stack, newest first, Inactive,
  Archived, stacked branches nested), and favorites and archiving are on a branch's menu, shared with the app.
- Several branches or tags can be selected in the sidebar to merge them in one commit, delete them or add them to
  favorites or archive them together.
- The filter button on the sidebar's title shows only the branches, remote branches and tags whose names contain the
  words typed.
- With the engine, History marks signed commits, the details show the signature and the commit's children, and the
  HEAD button and Go to Parent / Go to Child move between commits.
- With the engine, History shows a shallow clone's boundary and a band to fetch more or all of its history, and a
  partial clone's label fetches all its contents.
- Stashes shows only while there are stashes, its rows say what the app's do (branch, base against HEAD, untracked),
  a stash can become a branch, and Pop Latest Stash… asks first.
- Undo on the sidebar's title and in the Command Palette says what it puts back and asks first, and Undo History…
  undoes several operations down to the one chosen.
- With the engine, Repository Settings shows this repository's git settings in the app's pages: Author and Signing,
  Commits (the message template), Operations,
  Branches (the default branch), Working Copy (line endings, the executable bit, `.git/info/exclude`) and Fetching
  and Folders (fetching more, sparse checkout, git maintenance), Remotes, Hooks and LFS, and Keys and Values.

#### Changed

- A rebase and every History edit ask first. With the engine, the question says which stacked branches move or stay,
  which pushed branches will need Force Push and which commits are expected to conflict, and a second button moves or
  keeps the stacked branches for that operation only. History edits then run in the engine, as in the app.
- Reword and Squash use git's own `reword` and `squash`, so notes follow `notes.rewrite.*` as in git, and Reword runs
  the pre-commit and commit-msg hooks as git does. Lines starting with `#` are still kept.
- The conflict notice names the sidebar row whose Continue and Abort to use, since VSCode moves the notice to the
  notification center after a few seconds.
- On macOS, opening History for the first time after the engine is installed or updated no longer waits for the OS
  to check the new engine; the extension starts it once in the background at startup.

#### Fixed

- With the engine, deleting a branch that isn't merged now offers Delete Anyway instead of only showing git's error.
- History no longer stays empty when it is opened while the engine's History is still being read; the view's first
  request for rows made the extension think there were none.
- Pull on a diverged branch asks for Merge or Rebase for this pull when nothing sets how to pull (`pull.rebase`,
  `pull.ff`), since git 2.33.1 and later refuse to pull then; it used to merge.
- Starting a merge, rebase, cherry-pick or revert while another one is stopped no longer reports a conflict; it says
  which one is in progress, to continue or abort first.
- In a History with many branches side by side (git/git, linux), the graph no longer pushes the commit text out of the
  list: lanes with only lines are squeezed and the graph stays within a fixed width, as in the app.
- With the engine, History on Current Branch no longer goes empty after a commit made outside VSCode (a terminal,
  another tool); it filtered the engine's previous History, which didn't have the new commit yet.

#### Fixed

- With the engine, History opens in a shallow clone and follows `git fetch --deepen` and `--unshallow`; it stopped
  with "Commit … not found".

### [0.2.4] - 2026-10-30

The bundled engines are the 0.2.8 ones, the same build as the 0.2.8 app and CLI, so all three share one daemon. Commits
can be saved as patches and patches applied, branches compared with Range Diff, and conflicts with no markers left
are marked; people are shown by their mailmap names, and without the engine the extension works with git 2.23.

#### Added

- Conflicted files with no conflict markers left (git's rerere, another editor) are marked in the Working Copy list,
  lead with Mark as Resolved, and can all be marked resolved from the Conflicts heading.
- Compare with Previous Version on a local branch and Range Diff… show `git range-diff` of two versions of a
  branch in a read-only document, with or without the engine.
- A local branch that isn't checked out can be fast-forwarded to its upstream (Fast-Forward to Upstream) or pushed
  (Push to Upstream) from its menu in the nav.
- History and the commit details show people by their mailmap names (following `log.mailmap`), with the recorded
  name and email under them in the details when they differ, with or without the engine.
- A commit's menu in History saves it, or the commits from it up to HEAD, as patches (one file per commit or one
  mbox), and Apply Patch… (Command Palette, or a patch file's menu in the Explorer) shows what patches change and
  applies mail with `git am` or a diff with `git apply`. A stopped am says why in the nav, with Retry with 3-Way
  Merge. Both can be undone.

#### Changed

- The shared daemon runs the extension's git and ssh with the extension's environment (`PATH`, the SSH agent,
  `GIT_*`), even when the app started the daemon.
- The extension runs the git chosen for the machine in Amon Hen's settings (`~/.config/amonhen/settings.json`), and
  VSCode's `git.path` only when none is chosen; a chosen git that isn't there stops commands with an error.
- A change to VSCode's `git.path` applies to open repositories at once, through the shared daemon too, without
  reopening them.
- Push no longer sends a branch without an upstream to origin by itself: the nav shows Publish Branch, which asks
  for the remote (following `branch.<name>.pushRemote` and `remote.pushDefault`) and the name there.
- Edit in History stops as git's `edit` does: changes staged while stopped are amended into the commit on Continue,
  and with `notes.rewriteRef` its notes also go to the commits added while stopped.

#### Fixed

- Merge (and Rebase that amounts to a fast-forward) no longer moves a branch checked out in another worktree.
- Without the engine, the extension works with git 2.23, the supported minimum: it no longer opens repositories with
  `rev-parse --path-format` (git 2.31) or reads objects with `cat-file --batch-command` (git 2.36).
- A `git am` in progress was shown as a rebase.
- Undo of a branch-moving operation no longer fails with "not uptodate" right after undoing another one that put
  files back.
- Selecting a file in Working Copy just as its changes went away clears the diff instead of failing silently, and other
  failures of the main view's requests are now reported.
- Squash into Parent keeps the notes of the commit folded in, and Split moves them to the last of the new commits, as
  git does when `notes.rewriteRef` is set.

### [0.2.3] - 2026-10-21

The bundled engines are the 0.2.3 ones and still speak remote protocol 21, so the extension shares one daemon with the
0.2.3 app and CLI. Nothing changed in the extension itself.

### [0.2.2] - 2026-10-20

The bundled engines are the 0.2.2 ones and speak remote protocol 21, like the 0.2.2 app and CLI, so all three share
one daemon again. The Working Copy diff shows a file's line endings when they matter.

#### Added

- The Working Copy diff shows the file's line endings when they matter: `CRLF → LF` when git converts them, and a
  warning when they are mixed or CRLF where checkout would write LF. The tooltip names the conversion.

### [0.2.1] - 2026-10-19

Fixes for Windows: no more console windows flashing, and Working Copy diffs of files checked out with CRLF show only
the changed lines. The bundled engines speak remote protocol 20 like the 0.2.1 app and CLI, so they share one daemon
again. The engine's messages follow VSCode's display language.

#### Changed

- Messages from the engine (errors and the like) are in VSCode's display language: Japanese or English.

#### Fixed

- On Windows, git and the engine no longer flash a console window when the extension runs them.
- Working Copy diffs and line staging follow git's line ending conversion (`core.autocrlf`, `core.eol` and the `text`
  and `eol` attributes), with or without the bundled engine. On Windows, changed files no longer show as entirely
  rewritten, and staging lines no longer puts CRs into the index. Discarding lines changes only those lines.

### [0.2.0] - 2026-10-17

The first stable release, versioned with the app. History, diffs and operations run on the bundled Amon Hen engine
through the daemon shared with the app and the CLI. Adds discarding changes, stashes, creating branches and tags,
cherry-pick and revert.

#### Added

- In the Working Copy, discard unstaged changes by file, hunk or line (Discard). Discarded content can be restored with
  Undo.
- Create branches and tags (New Branch / New Tag) and rename branches, from branches and tags in the sidebar and from
  right-clicking a commit in the History.
- Save, apply, pop and drop stashes, and show their contents (including untracked files). Dropping can be undone.
- Cherry-pick / Revert by right-clicking a commit in the History.
- Update submodules and open them in another window.
- The Amon Hen engine (shared with the app; bundled for macOS arm64, Linux x86_64 / aarch64 and Windows x86_64 /
  aarch64) runs History (graph, commit details and diffs), Working Copy diffs, line-level staging and discarding, and
  operations such as commit, branches and tags, merge, rebase, cherry-pick, revert, stash, fetch and push through the
  shared daemon. It works without any setup: the engine is placed in `~/.local/share/amonhen` and links
  `~/.local/bin/amonhen` (the CLI). The app, the CLI and the extension share the same daemon, History, search index and
  undo log. It can be turned off with `amonhen.daemon.enabled`, and on an OS and CPU without a matching engine the
  extension runs on its git execution layer.

#### Changed

- The nav (entries for the Working Copy and History, merges and rebases in progress, branches and the like) moved to
  the sidebar, and the main view shows only the Working Copy or History, wide. Fetch / Pull / Push / Undo became
  buttons in the sidebar title.
- Branches are grouped into folders by `/`.

#### Removed

- The changed-file list in the sidebar and line-level staging in the standard diff editor opened from it
  (`ctrl+alt+s` / `ctrl+alt+u`). Staging is done in the Working Copy of the main view.
- The one-line commit field in the sidebar. Commits are made in the Working Copy of the main view.

#### Fixed

- The undo log is read again every time. Operations done in the app or the CLI were missed, and their records could be
  erased.

### [0.0.2] - 2026-10-14

#### Added

- The main view: one Webview in the editor area with the nav on the left (Working Copy / History / branches and the
  like) and the screen on the right. Opened from the Amon Hen icon in the activity bar.
- Working Copy: the list of changed files, per-hunk diffs (with word-level highlighting), staging and unstaging by line,
  hunk and file, a multi-line commit field and amend.
- History: the commit graph and the details of the selected commit (message, author, parents, changed files and their
  diffs). History loads 200 entries at a time, and diffs from the files that come into view.
- Reword / Squash / Fixup / Edit / Split / Drop / reorder commits by right-clicking in the History (reordering also by
  dragging).
- Branches / Remotes / Tags / Stashes / Submodules in the nav. Selecting a branch shows its history in the History, and
  double-clicking switches to it (for a remote branch, creates a branch that tracks it). Right-click for Checkout /
  Merge / Rebase / Delete.
- Merge / Rebase / Push / Force Push by dragging and dropping branches. If the drop target is not checked out and a
  fast-forward is enough, it moves forward without a checkout.
- Fetch / Pull / Push / Undo in the nav toolbar.
- Undo the last operation (commit, Merge, Rebase, fast-forward, Pull, deleting a branch, editing commits). The record
  survives restarting VSCode.
- When a merge or rebase has stopped, its progress is shown, with Continue / Skip / Abort.
- Open conflicted files in the standard 3-way merge editor. Resolve per file with Accept Current / Accept Incoming;
  when marking as resolved, you are asked to confirm if markers remain.
- A Changes view in the sidebar. Open changed files in the standard diff editor and stage / unstage the selected lines
  (`ctrl+alt+s` / `ctrl+alt+u`).
- A script that installs the stable release or the snapshot from GitHub Releases.
