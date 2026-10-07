# Kalem git plugin: design document

Status: **draft, proposed** (2026-10-07; keys revised the same day on the owner's direction: arrows, Enter and Escape in the documents, Doom's `SPC g` map for the actions). A plugin of `getkalem/plugins` (`plugins/git`, crate `kalem-plugin-git`, id `org.kalem.git`). Kalem's task T2.7i.11 and roadmap item R5.15; decisions D11, D28 and D29 of Kalem's design document apply. The decisions this document proposes (G1 to G12, section 12) and the asks of the core (section 9) wait for the owner. Phase 0's library is written and tested (the README says what runs); where writing it settled a detail, this document says what was written.

This file is `plugins/git/DESIGN.md` in `getkalem/plugins`; the README of the plugin is written from its sections 2 and 5.

## Contents

0. About this document
1. The idea in one page
2. What the user sees
3. Architecture
4. Running git
5. Keys
6. Settings
7. Both frontends, batch, languages
8. Phases
9. Changes to Kalem's core
10. Testing
11. Risks
12. Decisions
13. Non-goals
14. References

Appendices: A, the WIT sketches; B, the git commands used; C, the manifest; D, the status document's lines; E, remote URLs.

---

## 0. About this document

Kalem reserves `SPC g` for a git plugin that does not exist yet (`tests/keys/doom-leader.toml` lists sixteen keys as "needs the git plugin of getkalem/plugins"; the evaluation of 2026-10-04 lists "git anywhere" as missing for everyone). This document designs that plugin.

It takes from two programs:

- **lazygit**, for its simplicity: everything on one screen, one action at a time, nothing to configure, no manual needed after the first ten minutes. Its weakness, named by the owner: it lives outside the editor, and the pane that shows a changed file's diff is a small, dumb viewer: no search of your own, no motions, no jump to the file, no editor font or colors.
- **magit**, for its editor integration: the repository's status is a *buffer* in the editor, with the changed files and their diffs under them, folded and unfolded in place, staged hunk by hunk or line by line, and `Enter` on a diff line visits the file at that line. Its weakness: transient popups with dozens of switches, a vocabulary of its own, and a learning curve.

The plugin keeps magit's shape (status as a document in the editor) and lazygit's simplicity, but not lazygit's letters. The owner's direction (2026-10-07): in the plugin's documents a user moves and folds with the **arrow keys, Tab, Enter and Escape**, and acts with **Doom Emacs's `SPC g` keys**, which Kalem already lists; the plugin invents as few keys as it can. It is written in the style of Kalem's design document so that it can be read beside it; section numbers in the form §2.7 or §11.x refer to that document, D-numbers to its decisions, T-numbers to `docs/todo.md`, R-numbers to `docs/roadmap.md`, W-numbers to `docs/wasm_todo.md`.

## 1. The idea in one page

**One sentence.** `SPC g g` opens the repository's status as a document in the editor: the changed files with their diffs under them, moved through with the arrows, staged with the same `SPC g S` that stages a file from inside it, committed without leaving the editor, and read with the editor's own highlighting, search, selection and motions, the same in the window and in the terminal.

**Principles.**

1. **Status is a document** (magit's half). The status, a log, a commit, a blame, a file at a revision are read-only documents Kalem shows with its text engine, as Dired's listing is (§2.7, "a read-only text document, so the cursor, search, Vim motions and scrolling work unchanged"). The plugin writes the text and says what each stretch of it stands for; the editor owns the cursor, the selection, search and keys. A document works the same in both frontends (D26) and is read by a screen reader (§7.4).
2. **Arrows, Enter and Escape move; the leader acts** (the owner's direction). Up and Down move, Right and Left unfold and fold, Enter does the one obvious thing with what is under the cursor (opens the file, shows the commit, offers the choice where there are several), Escape goes back. Every action is a Doom key: `SPC g S` stages the file under the cursor exactly as it stages the file being edited, `SPC g s` the hunk or the selected lines, `SPC g c c` commits, `SPC g b` blames, `SPC g L` shows the log. A git document adds no key of its own. Where Doom has no key, the plugin adds none: the action is in the item's menu (Alt+Enter, or `SPC g /`), in the Git menu, or in the palette.
3. **The diff is first class.** A file's diff unfolds under the file, hunk by hunk, with the `diff` highlighter's colors. The unit of staging is the file, the hunk, or the selected lines. `Enter` on a diff line opens the file at that line.
4. **Git does the git** (G1). The plugin runs the `git` program and parses its porcelain output; it holds no git implementation of its own. Hooks, `.gitconfig`, credential helpers, signing, worktrees and submodules behave as they do in a terminal.
5. **Never a wait.** Every git call is asynchronous (as `net.fetch` is); the editor stays live during a push; a refresh that is overtaken by a newer one is dropped.
6. **Nothing hidden.** Every git command the plugin ran, with its folder, duration, exit status and output, is in the process log, so a user can see what happened and repeat it in a terminal.
7. **Safe by default.** Discarding, deleting a file or a branch, dropping a stash, a hard reset, aborting a merge, and any force confirm first and name what is lost; a push is forced only with `--force-with-lease` and only through a choice whose label says so.
8. **A plugin, not a part of the core** (D29). The plugin runs in the sandbox of D28 with one permission, `subprocess:git`. What it needs of the core beyond today's API is three interfaces that other plugins need too (section 9).

**What it is not.** Not a forge client (pull requests, issues: a later plugin on `net:fetch`), not a merge tool with three panes, not a graph drawer, not a replacement for the user's git configuration (section 13).

## 2. What the user sees

### 2.1 The status document (`SPC g g`)

`SPC g g` (Word-like: Ctrl+Shift+G; palette: "Git: Status") opens the status of the repository that holds the current document, or the current project's folder when there is no document, in the current pane; Escape goes back to the document it replaced. The text:

```
Head:      main   Fix the table parser's column alignment
Upstream:  origin/main   ↑1 ↓2
Tags:      v0.1.0 (3 commits ago)

▾ Untracked files (1)
▸ spikes/notes.txt

▾ Unstaged changes (2)
▸ modified     crates/org-syntax/src/table.rs   +12 −3
▾ modified     docs/design_document.md   +1 −1
@@ -1294,7 +1294,7 @@ ### 11.6 Security and resource limits
 - **Time limit:** fuel metering; a synchronous call exceeding its budget
-  (100 ms by default) is cancelled with a warning
+  (100 ms by default) is cancelled with a warning, and the plugin's log says so

▾ Staged changes (1)
▸ new file     plugins/git/README.md   +40 −0

▾ Stashes (1)
  stash@{0}  On main: table parser, half done

▾ Unpushed to origin/main (1)
  a1b2c3d  Fix the table parser's column alignment

▾ Unpulled from origin/main (2)
  e4f5a6b  Spreadsheets: sparklines
  c7d8e9f  Spreadsheets: Goal Seek
```

The plugin writes the fold marks (`▸` folded, `▾` unfolded) at the start of the line of what folds; a hunk's lines start at the first column, as `git diff` prints them, so that the `diff` highlighter colors them, and every other line is indented or starts with a word or a mark, so that the highlighter leaves it alone.

- **Head lines.** The branch and the subject of its last commit; the upstream with how far ahead (`↑`) and behind (`↓`); the nearest tag. During a merge, rebase, cherry-pick, revert or bisect a line says so (`Rebasing 3/7 onto main`, `Merging feature (2 unmerged)`), and the section **Unmerged** lists the conflicts at the top.
- **Sections** in this order: Unmerged, Untracked files, Unstaged changes, Staged changes, Stashes, Unpushed, Unpulled; when nothing is unpushed or unpulled, **Recent commits** (10). An empty section is left out. A section's heading carries its count.
- **A file line** shows the kind of change (`modified`, `new file`, `deleted`, `renamed a → b`, `copied`, `type changed`, `submodule`, `conflict`), the path, and the added and removed line counts. `▸` is folded, `▾` unfolded (`>` and `v` with the setting `glyphs = "ascii"`, for a terminal font without them). An untracked file unfolds too, its content shown as a new file's diff. A file's diff is loaded when it is first unfolded and kept until the next refresh.
- **A hunk** is shown as `git diff` prints it, highlighted by Kalem's `diff` syntax (`kalem-highlight` has it), with the function or heading context in its `@@` line. The default context is 3 lines; the hunk's menu (2.3) offers more and fewer.
- **Sections, files and hunks fold.** Right unfolds the item under the cursor, Left folds it, and Left on a folded item moves to its parent; Tab toggles, as it folds a headline in an Org document; Shift+Tab cycles the whole document (all folded, files open, everything open). The plugin keeps what is folded across refreshes, by file path and hunk, and regenerates the text; the editor folds nothing itself. A status is a tree, not prose, so Left and Right fold rather than move the cursor; Shift with them still selects, and Vim's `h` and `l` still move.
- **The cursor stays where it was** across a refresh: on the same file, hunk or commit when it still exists, else on the same line.
- **Mouse** in the graphical editor: a click on `▸` or `▾` toggles; a double click on a file line opens the file; a right click opens the item's menu; the diff lines select as text. The terminal editor follows with its own mouse support.

A **status bar item** (today's `ui.status`) shows ` main ↑1 ↓2 •3` (the branch, ahead and behind, the count of changed files) for every document inside a repository; a click opens the status. During a push, pull or fetch it shows the operation. While a git document is active, the status bar's other side shows the keys of the document in one line: `↑↓ move · →← fold · Enter open · Esc back · Alt+Enter menu · SPC g … act`.

### 2.2 Moving, Enter, and the item's menu

Movement in every git document:

| Key | Does |
|---|---|
| Up, Down | A line up or down (Vim: `k`, `j`, and every other motion) |
| Alt+Up, Alt+Down | The previous and next file, commit, or section heading (Vim: `{` and `}` work too, sections being separated by blank lines) |
| Right, Left | Unfold and fold the item under the cursor; Left on a folded item goes to its parent |
| Tab, Shift+Tab | Toggle the item; cycle the whole document |
| Home, End, PageUp, PageDown | The editor's own |
| Shift with the arrows (Vim: `v`, `V`) | Select lines of a hunk, for the leader's hunk commands |
| Escape | Back: closes the document and shows the one before it. With a selection, it clears the selection first (Vim: leaves visual mode first) |
| Enter | The one obvious thing with the item under the cursor (the table below) |
| Alt+Enter | The item's menu: a quick-pick of everything that can be done with the item, the obvious thing first. The same as `SPC g /` on the item, and as a right click |

What Enter does, by line:

| Line | Enter |
|---|---|
| A file (untracked, unstaged, staged, renamed) | Opens the file; a deleted file opens as it was at HEAD (a revision document, 2.5) |
| A hunk's `@@` line | Opens the file at the hunk's first line |
| A diff line | Opens the file at that line: the new side for `+` and context lines, the place of the removed lines for `-` |
| A commit (unpushed, unpulled, recent; in a log) | Shows the commit (2.4) |
| A stash | Shows the stash as a commit |
| `Head:` | The branch menu: Switch to… (the local branches, then the remote ones), New branch…, Delete…, Rename…, Set upstream…, Merge…, Rebase onto…, Log of this branch |
| `Upstream:` | The remote menu: Push, Pull, Fetch, Push and set upstream, Force with lease (these have no Doom key, so the choice is the obvious thing) |
| `Tags:` | The tags (phase 3) |
| `Rebasing…`, `Merging…`, `Cherry-picking…` | Continue, Skip, Abort |
| A section heading | Toggles the fold, as Right and Left do; the Stashes heading's menu (Alt+Enter) adds Stash all, Stash staged only, Stash with untracked |
| `Load 200 more…` (the log's last line) | Loads them |

The item's menu (Alt+Enter, `SPC g /`, a right click) has, for a file: Open, Stage, Unstage, Discard changes…, Delete…, Blame, Log, Time machine, Diff as a document, Open on the remote, Copy link, Copy path, Add to .gitignore (phase 2); for a hunk: Stage, Unstage, Discard…, More context, Less context, Open at this line; for a commit: Show, Check out…, Cherry-pick, Revert…, Reset here (soft, mixed, hard…), Fixup staged changes into it, Copy hash, Open on the remote; for a stash: Show, Apply, Pop, Drop…; after the item's own entries, every git command. Each entry names its leader key where it has one, so the menu teaches the keys. An entry ending in `…` confirms or asks.

### 2.3 Acting: Doom's keys on the item under the cursor

Every action is a command of the `SPC g` map (section 5 has the whole table). In a file it acts on that file and the hunk at the cursor; in a git document it acts on the item under the cursor, or on the selected lines. The same key, the same meaning:

| Key | In the status, on | Does |
|---|---|---|
| `SPC g S` | a file; a section heading | Stages the file (`git add`); the whole section (`Untracked files` adds every untracked file, after confirming) |
| `SPC g U` | a staged file; the Staged heading | Unstages the file; everything |
| `SPC g s` | a hunk; selected lines | Stages the hunk, or those lines alone, by a patch applied to the index (3.6) |
| `SPC g u` | a staged hunk; selected lines | Unstages them |
| `SPC g R` | a file | Discards its changes after confirming: an unstaged change is restored (`git restore`); a staged change is taken out of the index and the working tree by its reverse patch, leaving the file's other changes alone |
| `SPC g r` | a hunk; selected lines | Discards them after confirming |
| `SPC g D` | a file | Deletes it after confirming: an untracked file with `git clean`, a tracked one with `git rm` |
| `SPC g c c` | anywhere | Opens the commit message document (2.4) |
| `SPC g c a` | anywhere | Amends the last commit, its message opened to edit |
| `SPC g c e` | anywhere | Amends the last commit without editing its message (`--amend --no-edit`, magit's "extend") |
| `SPC g c w` | anywhere | Rewords the last commit (the message document, nothing added) |
| `SPC g c f` | a commit | Makes a fixup commit of the staged changes into that commit (`--fixup`); anywhere else, a quick-pick of the recent commits |
| `SPC g c r` | anywhere | `git init` in the project's folder, after confirming (where there is no repository yet) |
| `SPC g F` | anywhere | Fetches (`--prune` by setting) |
| `SPC g b` | a file | Blames it (2.5) |
| `SPC g L` | a file; a commit; anywhere | The log of that file; the log from that commit; the branch's log (2.4) |
| `SPC g t` | a file | The time machine for it (2.5) |
| `SPC g [`, `SPC g ]` | anywhere | The previous and next hunk in the document |
| `SPC g o o`, `SPC g y` | a file, a diff line, a commit | Opens it on the remote; copies the link (2.6) |
| `SPC g /` | anywhere | The item's menu, then every command (2.2) |
| `SPC g .` | a file; anywhere | The file's menu (Doom's "file dispatch") |
| `SPC g g` | anywhere | Refreshes the status; in another git document, shows the status |

Push, pull, branches and stashes have no Doom key and get none: they are Enter on the `Upstream:` line, Enter on the `Head:` line, Enter on a stash or the Stashes heading's menu, and entries of `SPC g /`, the Git menu, the palette, and the panel's buttons (2.9). Should the owner want keys for them, `SPC g p` (push) and `SPC g P` (pull) are free in Doom's map; this document does not take them.

**Selected lines.** Lines of a hunk selected with the editor's own means (Shift with the arrows; Vim's `v` and motions; the mouse) make `SPC g s`, `SPC g u` and `SPC g r` act on those lines alone: the plugin builds the patch that stages, unstages or discards them (3.6), as magit and lazygit do. A selection over several hunks or files acts on each.

**Confirmation** (principle 7): `SPC g R` and `SPC g r` always (unless `confirm_discard` is off); `SPC g D`; `SPC g S` on the Untracked heading; Delete and Rename in the branch menu; Drop in a stash's menu; Abort; Force with lease; Reset hard; Check out a commit. Each says what is lost in one line ("Discard 12 changed lines of src/lib.rs? They are not in any commit."). Nothing else asks.

**Unsaved documents.** A command that stages or commits a file open with unsaved changes (`SPC g S`, `SPC g s`) first offers to save it, as magit does; declining cancels.

### 2.4 Committing, the log, a commit

**The commit message.** `SPC g c c` opens the commit message as an editable document titled `COMMIT_EDITMSG`, of the text type `git-commit`:

```

# Please enter the commit message for your changes. Lines starting
# with '#' will be ignored, and an empty message aborts the commit.
#
# On branch main
# Your branch is ahead of 'origin/main' by 1 commit.
#
# Changes to be committed:
#	modified:   crates/org-syntax/src/table.rs
#
# ------------------------ >8 ------------------------
# Do not modify or remove the line above.
# Everything below it will be ignored.
diff --git a/crates/org-syntax/src/table.rs b/crates/org-syntax/src/table.rs
…
```

The cursor is on the first line. The staged diff below the scissors line is what `git commit -v` shows. On saving, the plugin cuts the text at the scissors line and git strips the comment lines (`--cleanup=strip`), as `git commit -v` does with a message the user edited; with `-F`, git takes a message as not edited, so `--cleanup=scissors` would keep the comments (found by the tests). The user's `commit.template` is honored.

- **Saving commits** (Ctrl+S, `:w`, `ZZ`): the text, cut at the scissors line, goes to `git commit --cleanup=strip -F -` on standard input; the document closes; the status refreshes; the status bar says "Committed a1b2c3d". A `commit-msg` hook that refuses, or git that refuses (an empty message, nothing staged), shows the reason as a notification and keeps the document open to fix.
- **Closing without saving cancels** (Ctrl+W, `:q!`). The status bar line says so while the document is open: "Save commits · Close cancels". Escape is the editor's here (it leaves insert mode in Vim), since the document is editable.
- `SPC g c a` (amend) and `SPC g c w` (reword) open the same document with the last commit's message.
- The first line longer than 72 characters, or a blank second line missing, is marked with the `error` style (phase 2, when styles apply to editable documents); nothing refuses.

Why a document and not a prompt: a commit message is several lines, written with the editor's own keys, spell checking and Vim mode; the diff under it is what one reads while writing it. Why saving, not a key of its own: every user knows how to save and how to close, and both profiles have the keys (G7).

**The log** (`SPC g L`; Enter on `Head:` offers it too) is a document of the text type `git-log`, one commit per line:

```
a1b2c3d  2026-10-07  Mehmet Sekercioglu  Fix the table parser's column alignment  (HEAD -> main)
e4f5a6b  2026-10-06  Mehmet Sekercioglu  Spreadsheets: sparklines  (origin/main, tag: v0.1.0)
```

200 commits at a time, then a last line `Load 200 more…` that Enter loads. Right unfolds a commit's message and stat under its line (its diff stays in the commit view, one Enter away); Left folds it. The graph (`--graph`) is an entry of the document's menu and a setting. Enter shows the commit; Alt+Enter is the commit's menu (Check out…, Cherry-pick, Revert…, Reset here…, Fixup, Copy hash, Open on the remote); `SPC g c f`, `SPC g o o` and `SPC g y` work on the commit under the cursor; `SPC g L` on a commit shows the log from it. Escape goes back.

**A commit** (Enter on a commit line, on a stash, on `Head:`'s menu) is a read-only document of the text type `git-commit-view`, highlighted as `diff`: the header (`git show --format=fuller`: hash, author, committer, dates, refs, the message), the stat, then the diff, with the status's movement (Right and Left fold a file, Enter on a diff line opens the file at that line, Alt+Enter the hunk's menu for context) and the leader's `SPC g o o`, `SPC g y`, `SPC g c f`. Nothing is staged from a commit view.

### 2.5 Blame and the time machine

**Blame** (`SPC g b`) opens the current file, or the file under the cursor, as a read-only document of the text type `git-blame`, titled `table.rs (blame)`: each line with its commit's short hash, author and date in a column at the left, the hash shown once per run of lines from one commit, and the file's line after a `│`:

```
a1b2c3d 2026-10-07 Mehmet    │ fn align(columns: &[Column]) -> Vec<usize> {
                             │     let mut widths = vec![0; columns.len()];
e4f5a6b 2026-09-30 Ayşe      │     for row in rows {
```

The cursor starts on the line the editor's cursor was on. Enter shows the commit of the line; **Alt+Left** blames the file as it was before that commit (back in time, as magit's `b`), **Alt+Right** comes forward again; Escape goes back to the file. Uncommitted lines show `0000000  not committed`. Left and Right move the cursor here, as in any text: a blame is a file, not a tree.

`SPC g B` ("blame this line") shows the line's commit in a notification (hash, author, date, subject) without opening anything; Enter in the notification shows the commit (phase 2: a notification with an action).

Inline blame, as magit draws it over the file itself, comes in phase 2 with the decorations contract (T3.1.9c): the same annotation as virtual text before the first line of each run, toggled by `SPC g b` a second time, the document editable meanwhile.

**The time machine** (`SPC g t`, git-timemachine) opens the current file at its last commit as a read-only document titled `table.rs @ a1b2c3d`, of the file's own language (so it is highlighted) and the text type `git-revision`. **Alt+Left** and **Alt+Right** step to the older and newer commits of the file; the status bar says `3 of 42 · a1b2c3d · 2026-10-01 · Ayşe · Spreadsheets: sparklines`. Enter shows that commit; Alt+Enter offers the diff to the next revision, and copying the hash; Escape goes back to the file. Renames are followed (`--follow`). The same two keys mean the same in blame and in the time machine: Alt+Left is older, Alt+Right is newer.

### 2.6 The remote

`SPC g o o` opens the current file, the selected lines, or, in a git document, the file, diff line or commit under the cursor on the remote's web site; `SPC g y` copies that link instead. The URL is built from `git remote get-url` of the upstream's remote (`origin` when there is none) and the patterns of appendix E: GitHub, GitLab (and self-hosted, by a setting), Bitbucket, Gitea and Forgejo, sourcehut, Azure DevOps. The link names the branch by default and the commit by the setting `remote_link = "commit"`; a file link carries the line or the range (`#L10-L20`). A remote with no known pattern says so and copies nothing.

### 2.7 The process log

"Process log" in `SPC g /` and in the palette opens a read-only document of the text type `git-process`: the last 100 git commands the plugin ran, newest first, each with the folder, the arguments, the duration and the exit status, and its output folded under it (Right unfolds, standard error first). It is the answer to "what did it do?", and the first thing to paste into a bug report. A command line is text: select it and copy it, as in any document.

### 2.8 Inside a file

The `SPC g` keys of section 5 work in any document inside a repository, without the status open: `SPC g S` and `SPC g U` stage and unstage the file, `SPC g R` restores it to the index's version after confirming, `SPC g s` and `SPC g r` stage and discard the hunk at the cursor (phase 2), `SPC g [` and `SPC g ]` move over the changed hunks (the core's motions over the gutter marks, phase 2), `SPC g c c` commits, `SPC g b` blames, `SPC g L` shows the file's log, `SPC g t` opens the time machine, `SPC g o o` and `SPC g y` reach the remote, `SPC g .` is the file's menu and `SPC g /` every command; `SPC t d` toggles the gutter marks.

**Gutter marks** (phase 2, decorations): a saved file under git shows, in the gutter of both editors, a mark on each added line, changed line, and between lines where lines were removed, against the index by default (what `SPC g s` would stage; `gutter_base = "head"` compares with the last commit instead). They refresh on save, and while typing in the active document (3.5).

### 2.9 The Git panel

Beside the documents, and with today's API alone (D11, the `ui` interface), a side panel **Git** lists what the status lists: the branch line; the buttons Refresh, Commit, Pull, Push; then Staged, Unstaged, Untracked and Unmerged as tree entries, each file a checkbox that is ticked when staged (ticking stages, unticking unstages), its added and removed counts as the detail, a click on the label opening the file; a progress bar during a push or pull. A panel is driven with the arrows, Enter and Space by the editor already. The panel is the quick picture at the side; the status document is the working surface. `panel = false` hides it. In phase 0 (section 8) it is all there is.

## 3. Architecture

### 3.1 The plugin

A component of the `extension` world (`kalem.wit`: commands, keys, events; `ui`, `settings`; the three interfaces of section 9 as they come), built with `kalem plugin build`, activated at startup (`"activation": ["onStartup"]`: activation registers commands and costs nothing until a document in a repository opens). Manifest in appendix C. One permission in phase 1: `subprocess:git` (3.3). No `fs`: the plugin reads and writes no file itself; everything it knows comes from git's output, and everything it changes goes through git (G5). Phase 2's "Add to .gitignore" is the one exception, and asks `fs:write:workspace` for it.

Crate layout, after the template:

| Path | Contents |
|---|---|
| `src/lib.rs` | The crate's map; with Kalem's interfaces, `activate`: the commands, their keys and scopes, the subscriptions, the dispatch of answers |
| `src/git/` | The command lines (`cmd.rs`) and the readers of git's output: `status.rs` (`--porcelain=v2 -z`), `diff.rs` (diffs and `--numstat -z`), `log.rs`, `blame.rs`, `refs.rs` (branches, stashes, the nearest tag, the operation under way) |
| `src/model.rs` | `Repo`: what the status knows (3.5); `Folds`: what is folded, by region key |
| `src/refresh.rs` | A refresh as data in and commands out (`Refresh::start`, `feed`, `finish`), so that the component runs the commands as their answers arrive; the `Runner` trait and `load`, which runs them one after another for the example and the tests |
| `src/content.rs` | A document's content: text, styles, nested regions (appendix A.2's `content`) |
| `src/target.rs` | What a region key names, what the cursor or a selection stands for |
| `src/actions.rs` | What the leader's keys do to it: plans of git commands with the question to ask first |
| `src/patch.rs` | Patches for hunks and for selected lines, forward and reverse (3.6) |
| `src/views/` | One module per document: `status.rs`, `log.rs`, `commit.rs` (a commit shown), `blame.rs`, `process.rs`; later `message.rs` (the commit message) and `revision.rs` (the time machine) |
| `src/url.rs` | Remote URLs to web links (appendix E) |
| `src/native.rs` | Git run with `std::process` (not built for WebAssembly) |
| `examples/git.rs` | `cargo run -p kalem-plugin-git --example git -- status` prints the status document; `log`, `show`, `blame`, `stage`, `unstage`, `discard`, `delete`, `patch`, `commit`, `url`: the plugin usable and testable before the host interfaces exist, as the xlsx plugin's example was |
| `tests/` | The conformance test of the manifest; the library against a real git (`repo.rs`), random line selections included (section 10) |

The plugin draws nothing: documents are text with styles, panels are widget trees, notifications and quick-picks are the host's (D11, D28).

### 3.2 Views are documents

The status, log, commit, blame, revision, message and process views are **documents the plugin writes**, through a new host interface, `documents` (appendix A.2; the ask of 9.2). The precedent is the core's own file manager: `kalem_core::dired` keeps what each line stands for and how it is styled, the listing is a read-only document of the mode `directory`, and the commands scoped to `directory` have keys of their own because the text is read-only. The plugin's documents work the same way:

- `open(spec)` shows a document with an ID (`git.status`), a key that tells its instances apart (the repository's root), a title, a **text type** for the scopes of commands (`git-status`), a **highlighter** if any (`diff`, or the file's language for a revision), whether it is editable, and the path it stands for if any. Opening an ID and key that is already open shows it again.
- `set(doc, content, at)` replaces the text. `content` is the text, its **styles** (ranges with the `text-style` of `ui.wit`: strong, muted, heading, error, code; the diff lines are the highlighter's), and its **regions**: ranges with the plugin's own key for what they stand for (`file:docs/design_document.md`, `hunk:docs/design_document.md:2`, `commit:a1b2c3d`, `stash:0`, `section:unstaged`), each saying whether it folds and whether it is folded. The cursor goes to the region named by `at` when there is one, else stays on its line.
- A command scoped to the document's type runs in it: `editor.selected()` and `editor.text()` answer for the document, and the plugin maps the cursor and the selection to its regions itself. `documents.current()` names the plugin's document the command runs in. So `SPC g S`, one command `git.stageFile` with the scope `all`, asks `documents.current()`: in a git document it stages the file of the region under the cursor, elsewhere the file of the document; the key is one and the command is one.
- The user's folding (Right, Left, Tab arrive as the plugin's own fold commands, scoped to the tree-like types), their edits in an editable document, and closing arrive at the plugin (`on-document`: `shown`, `saved(text)`, `closed`). Folding is the plugin's: it keeps the fold state and regenerates the text (magit does the same; a region's `folded` flag asks the editor to draw `▸`).
- The editor owns the cursor, the selection, search, scrolling, Vim mode, the mouse, the open-files list and the tab. Where the document opens (in place, or in a split beside) is the user's setting (`status_placement`, section 6); Escape runs `file.close`, which shows the document before it.

Why not the widget tree of D11 for the status: a diff is text that one reads, searches, selects by line and visits by line; a list of items and labels gives none of that, and no highlighting. Why not a view of the editor area (§11.10's "Views", T3.1.9d): a view is a view *of a document* (a kanban over headlines); the status is a document of its own. The `documents` interface is smaller than either and serves the log view of T2.7g.2, search results, a REPL's output, a build log, and any plugin that lists things to act on.

### 3.3 Running git

The plugin runs the `git` program through a new host interface, `process` (appendix A.1; the ask of 9.1), granted by the manifest's permission `subprocess:git`: the program's name after the colon, so that the permission the user approves says "runs the program git in your project folders" and nothing else, as `net:fetch:DOMAIN` names a domain (§11.6 lists `subprocess` with "separate, explicit warning"; R5.15 calls it the `process` permission; the template's conformance test knows `subprocess`). A run does not wait, as `net.fetch` does not: `run` returns at once with the run's number, and the exit status with the output arrives at the plugin's `on-process`.

What the host does with a run: resolves the program on the user's PATH (the one Kalem's language server client resolves on, which on macOS is the login shell's, not the application's); starts it in the folder the plugin names, which must be inside a project Kalem knows; without a terminal and, on Windows, without a console window; with the user's environment plus the plugin's variables; writes the standard input the plugin gave and closes it; reads both outputs on their own threads (as `kalem-lsp`'s client and `lsp::run_formatter` do); caps the output at 16 MB (`MAX_BYTES`, as `fs` and `net`) and says when it cut; answers on the editor's thread at the next tick. `kill` stops a run; every run of a plugin is killed when it is deactivated. There is no shell, so no quoting and no injection: arguments are a list.

Why the program and not a library (gitoxide, libgit2): the user's git is the one with their `.gitconfig`, hooks, credential helpers, SSH agent, commit signing, worktrees, submodules, sparse checkouts and LFS filters, and a library reimplements each with a gap; lazygit and magit run the program for the same reason. A library in the sandbox would also need raw, binary file access to `.git` and locking, which the `fs` interface does not and should not give. What the program costs: a process per call (about 5 ms each on a warm repository; a status with two diffs is three), a minimum git version, and parsing text (G1).

The plugin requires **git 2.23** or later (`git restore` and `git switch`; `--porcelain=v2` is 2.11) and checks the version at its first run; an older or missing git shows one notification naming the version and the setting `programs.git`, and the plugin's commands say "git was not found" after that instead of failing one by one.

### 3.4 Finding the repository

A command runs for the repository of the active document's folder (or of the project's folder when the document has no file or there is none): `git rev-parse --show-toplevel --git-dir --abbrev-ref HEAD` from that folder, cached by folder until the status refreshes. Worktrees and submodules are repositories like any other, since git answers for them. A folder outside every project of Kalem is refused by the host (`process` runs only in projects), and a command there says so and offers nothing but adding the project (`SPC p a`). A folder that is not inside a repository: the status bar shows nothing, `SPC g g` offers `git init` in the project's folder after confirming (`SPC g c r` does the same), and the other commands say "not in a git repository". Bare repositories are not supported.

### 3.5 State and refresh

`Repo`, the model behind the status: the root; the head (branch or detached hash, its subject); the upstream with ahead and behind; the nearest tag; the operation in progress (merge, rebase with its step, cherry-pick, revert, bisect); the entries, each with its path, its index and working tree status, the renamed-from path, the counts, and its diff once loaded (`Option<Diff>`, a list of hunks); the stashes; the unpushed and unpulled commits; the recent commits; the fold state by key; a generation number.

**A refresh** is one `git status --porcelain=v2 --branch --show-stash -z` (the entries, the head, the upstream, ahead and behind, the stash count), then, in parallel, `git log` for unpushed and unpulled (`@{upstream}..HEAD` and `HEAD..@{upstream}`, 20 each) or the recent commits, `git stash list`, and `git describe --tags`. A file's diff is fetched when it is unfolded, one `git diff` (or `--cached`, or `--no-index /dev/null` for an untracked file) per file, so a repository with a thousand changed files costs one status and nothing more until a file is opened. Answers carry the generation they were asked in; an older generation's answer is dropped.

**Triggers:** every plugin command that changes the repository; `document-after-save` of a file inside the repository; `workspace-file-changed`; the status document shown again (`on-document shown`); `SPC g g` in the status. A trigger during a refresh queues one more. Debounce: 100 ms. What the plugin cannot see today: a commit or checkout made in a terminal, because `kalem-project`'s watcher skips `.git` (its tests assert `.git/index` is no change); the ask of 9.4 adds `.git/HEAD`, `.git/index`, `.git/refs/` and `.git/MERGE_HEAD` and the like to `workspace-file-changed`, which lazygit watches too. Until then: `SPC g g`, and the refresh when the status is shown again.

**Gutter marks** (phase 2): on `document-after-save` the plugin asks `git diff -U0 -- path` (against the index or HEAD) and sets the marks. While typing, the plugin's `document-changed` handler runs its own command `git.refreshGutter` through `kalem.run` (a command runs with the `editor` interface, a handler does not): the command reads the document's text, diffs it against the index's version of the file (`git show :path`, cached per refresh) with the plugin's own diff (the `imara-diff` crate, Myers, histogram for the large), and sets the marks; 300 ms after the last keystroke, the active document only.

**Budgets** (§11.6): every call of the plugin returns within 100 ms and the plugin lives in 64 MB. Parsing a status of 10,000 entries, or a diff of 50,000 lines, fits; rendering does not grow with what is folded. A file's diff over 10,000 lines is not unfolded in the status: Right on it shows one line, `12,000 changed lines: open the diff as a document`, which Enter opens; a binary file says "Binary file changed" and stages whole. The manifest may raise the memory limit as the workbook plugin's does if a corpus shows the need.

### 3.6 Patches: hunks and lines

Staging a hunk, or lines of it, is a patch applied to the index (`git apply --cached`, with `--unidiff-zero` when the diffs have no context lines); unstaging is the reverse patch of the cached diff (`--reverse`); discarding is the reverse patch applied to the working tree, and for a staged change to both (`--index`). The patch builder (`src/patch.rs`), as magit's `magit-apply` and lazygit's `patch` package:

- A hunk patch is the file header and the hunk, unchanged.
- A patch for selected lines keeps the hunk's context, keeps the selected `+` and `-` lines, drops the unselected `+` lines, turns the unselected `-` lines into context, and recounts the `@@` header. For the reverse direction the roles swap.
- `\ No newline at end of file`, CRLF files (`core.autocrlf` left to git), a hunk at the file's end, a new file (`--- /dev/null`), a deleted file, and a mode change are each a test case. Renames with changes, binary files, submodules and symbolic links stage whole; the status says so when a hunk command is given on them.
- An untracked file has no hunks until it is added; `SPC g S` on one is `git add`. Intent-to-add (`git add -N`) for staging parts of a new file is phase 3.

Correctness is checked against git itself (section 10): a property test makes random edits in a temporary repository, stages random hunks and lines through the builder, and compares the index with what `git add -p` would give; staging then unstaging round-trips to the original index.

### 3.7 Errors and messages

A git command that fails: a notification with the first line of its standard error (git's messages are good: "nothing to commit", "Your local changes would be overwritten"), the whole output in the process log, the status refreshed anyway. A command that cannot start (git missing, a folder gone): the same, once. A run that produces more than 16 MB is cut and the document says "output cut at 16 MB; run it in a terminal". The plugin never shows a stack trace; a panic reaches the host's log through `diagnostics` (W10).

**Credentials and signing.** The plugin sets `GIT_TERMINAL_PROMPT=0`, so a push that needs a password fails at once with git's message rather than hanging; the notification adds "set up a credential helper, or push once from a terminal". SSH keys through an agent, and credential helpers (macOS Keychain, Git Credential Manager), work as they do in a terminal. GPG and SSH signing work when the agent or pinentry needs no terminal. `GIT_ASKPASS` served by Kalem's own prompt (`ui.prompt` with `password`) is phase 3 and needs the host (9.6).

**Hooks** run as git runs them; a failing `pre-commit` or `pre-push` is shown like any failure. `prepare-commit-msg` runs, but its changes to the message are not seen before the commit, since the message comes on standard input; stated in the README.

**Locales.** Runs use `LC_ALL=C` for the commands the plugin parses, so git's messages the plugin reads are English; messages the user reads (a failure's first line) come from a second run without it only where it matters (none in phase 1: git's porcelain formats are not localized).

## 4. Running git: the commands

Appendix B lists every command line with its flags. The rules: `--no-pager` and `-c color.ui=never` always; `GIT_OPTIONAL_LOCKS=0` for the commands that only read, so that a refresh never takes the index's lock from a command the user runs; `-z` and `--porcelain=v2` for status, `-z` for every list of paths; `--format=` with `%x00` separators for `git log`; `--porcelain` for blame; `-c core.quotepath=off`; `--no-edit` for merge, pull, revert and cherry-pick so that git never opens an editor; `GIT_EDITOR=false` as a guard. Paths cross as bytes and are shown lossily when not UTF-8. The plugin never runs a shell and never passes a user's text as an argument that git could read as an option: paths after `--`, refs checked against `git check-ref-format`.

## 5. Keys

### 5.1 Three rules

1. **In a git document, movement and folding are the editor's keys: the arrows, Tab, Enter and Escape** (2.2). Up and Down move; Alt+Up and Alt+Down jump between files, commits and sections; Right and Left unfold and fold (in the tree-like documents: status, log, commit view, process log; in blame and the time machine they move the cursor, and Alt+Left and Alt+Right step through time); Tab toggles and Shift+Tab cycles; Enter does the one obvious thing; Alt+Enter opens the item's menu; Escape goes back. No letter is bound, so a Vim user's motions and search work untouched, and a Word-like user's arrows do what arrows do in every tree view.
2. **Actions are Doom's `SPC g` keys, the same inside a git document and inside a file.** A command acts on the file and hunk under the cursor in a git document, and on the file being edited otherwise (3.2). A git document adds no key of its own.
3. **Where Doom has no key, the plugin adds none.** Push, pull, branches, stashes, cherry-pick, revert, reset, context lines and the process log are reached by Enter on the line they belong to, by the item's menu (Alt+Enter, `SPC g /`, a right click), by the Git menu, and by the palette. The four keys the plugin adds under Doom's prefixes (`SPC g u`, `SPC g c a`, `SPC g c e`, `SPC g c w`) are the pairs of keys Doom has.

### 5.2 The `SPC g` map

The keys of `tests/keys/doom-leader.toml` as the owner listed them, bound by the plugin's command specs (`keys: ["space g g"]`) so that the which-key popup shows them under `+git` and the table's reason changes from "needs the git plugin" to the command's title; then Doom's own keys the table does not list; then the plugin's four.

| Key | Command | Title | In a file | In a git document, on the item under the cursor | Source |
|---|---|---|---|---|---|
| `SPC g g` | `git.status` | Git: Status | Opens the status | Refreshes; from another git document, shows the status | table |
| `SPC g G` | `git.statusHere` | Git: Status Here | The status with the cursor on this file, unfolded | | table |
| `SPC g S` | `git.stageFile` | Git: Stage File | Stages this file | Stages the file; a section heading stages the section | table |
| `SPC g U` | `git.unstageFile` | Git: Unstage File | Unstages this file | Unstages the file; the Staged heading, everything | table |
| `SPC g s` | `git.stageHunk` | Git: Stage Hunk | Stages the hunk at the cursor (phase 2) | Stages the hunk, or the selected lines | table |
| `SPC g u` | `git.unstageHunk` | Git: Unstage Hunk | Unstages the hunk at the cursor (phase 2) | Unstages the hunk, or the selected lines | plugin |
| `SPC g R` | `git.revertFile` | Git: Revert File | Restores this file to the index's version | Discards the file's changes | table |
| `SPC g r` | `git.revertHunk` | Git: Revert Hunk | Discards the hunk at the cursor (phase 2) | Discards the hunk, or the selected lines | table |
| `SPC g D` | `git.deleteFile` | Git: Delete File | Deletes this file (`git rm`) | Deletes the file: `git clean` for an untracked one | Doom |
| `SPC g c c` | `git.commit` | Git: Commit | The message document | The same | table |
| `SPC g c a` | `git.amend` | Git: Amend | Amend, message edited | The same | plugin |
| `SPC g c e` | `git.extend` | Git: Amend Without Editing | `--amend --no-edit` | The same | plugin |
| `SPC g c w` | `git.reword` | Git: Reword | The message document, nothing staged added | The same | plugin |
| `SPC g c f` | `git.fixup` | Git: Fixup | A quick-pick of recent commits | Into the commit under the cursor | Doom |
| `SPC g c r` | `git.init` | Git: Init | `git init` here, after confirming | | Doom |
| `SPC g F` | `git.fetch` | Git: Fetch | Fetches | The same | Doom |
| `SPC g b` | `git.blame` | Git: Blame | Blames this file | Blames the file under the cursor | table |
| `SPC g B` | `git.blameLine` | Git: Blame This Line | The line's commit, as a notification | | table |
| `SPC g L` | `git.log` | Git: Log | The log of this file | Of the file, from the commit, or of the branch | table |
| `SPC g t` | `git.timeMachine` | Git: Time Machine | This file through its commits | The file under the cursor | table |
| `SPC g [`, `SPC g ]` | the core's previous-change and next-change (names the core's) | | Over the gutter marks (phase 2) | The plugin's: the previous and next hunk in the document | table |
| `SPC g o o` | `git.openOnRemote` | Git: Open on the Remote | This file, these lines | The file, line or commit under the cursor | table |
| `SPC g y` | `git.copyRemoteLink` | Git: Copy Link to the Remote | The same, copied | The same | table |
| `SPC g /` | `git.dispatch` | Git: Commands | Every git command | The item's menu, then every command | table |
| `SPC g .` | `git.fileMenu` | Git: This File | This file's menu | The file under the cursor's menu | Doom |
| `SPC t d` | `git.toggleGutter` | Toggle Diff Marks | Shows or hides the gutter marks (phase 2) | | table |

Doom's `SPC g f`, `SPC g l`, `SPC g o`, `SPC g c` prefixes are honored as prefixes (`o o`, `c c`, `c f`, `c r`; the others of `f` and `l` are forge's or magit's listings and stay unbound). A note for the owner: in Doom, `SPC g b` is "switch branch" and `SPC g B` is blame; the table in `tests/keys/doom-leader.toml` has blame and blame-this-line, and the plugin follows the table. Should the owner want push and pull on keys, `SPC g p` and `SPC g P` are free in Doom's map; this document does not take them (rule 3).

Scope: every command is `all` with the when-clause `hasFile` where a file on disk is needed; inside a git document the "current file" is the file under the cursor, as Dired's is the entry at the cursor. In the status and the other git documents the leader commands work as the table's last column says.

### 5.3 The Word-like profile

Kalem's Word-like profile has no leader. There: Ctrl+Shift+G opens the status; the **Git** menu, which the category "Git" gives the plugin's commands (§11.2), lists every command with its title; the right click opens the item's menu in a git document; the palette has everything; the arrows, Tab, Enter, Alt+Enter and Escape are the same. No other chord is bound by default, so that nothing of Kalem's Word-like keys is taken; `keymap.json` adds any.

### 5.4 Discoverability

The which-key popup of T2.7i.19 shows what follows `SPC g` and `SPC g c` with the plugin's titles. The status bar shows the document's five keys while a git document is active (2.1). Every entry of an item's menu names its leader key, so the menu is how the keys are learned; and every action is in the palette ("Git: Stage File", "Git: Push"), so a user without the key finds it.

## 6. Settings

In the user's `settings.toml`, under `[plugins."org.kalem.git"]`, read through `settings.own` and watched:

| Key | Default | Meaning |
|---|---|---|
| `programs.git` | `""` | Where git is when it is not the one on the PATH: a full path. Kalem's host reads it, not the plugin, which only ever names `git` (9.1) |
| `status_placement` | `"here"` | Where the status opens: `"here"` (the current pane, Escape returns), `"split"` (a pane beside) |
| `context_lines` | `3` | Context lines of a hunk |
| `untracked` | `"normal"` | `git status`'s `--untracked-files`: `"normal"` (folders collapsed), `"all"`, `"no"` |
| `pull` | `"config"` | How Pull pulls: `"config"` (git's `pull.rebase`), `"rebase"`, `"merge"`, `"ff-only"` |
| `fetch_prune` | `true` | `--prune` on fetch |
| `log_graph` | `false` | The ASCII graph in the log |
| `remote_link` | `"branch"` | What a remote link names: `"branch"` or `"commit"` |
| `remote_hosts` | `{}` | Self-hosted forges: `{ "git.example.com" = "gitlab" }` |
| `gutter` | `true` | Diff marks in the gutter (phase 2) |
| `gutter_base` | `"index"` | The marks against the index (`"index"`) or the last commit (`"head"`) |
| `panel` | `true` | The Git side panel |
| `log_count` | `200` | Commits a log loads at a time |
| `confirm_discard` | `true` | `SPC g R` and `SPC g r` ask; off for those who never want to be asked (the other confirmations stay) |
| `signoff` | `false` | `--signoff` on commits |
| `glyphs` | `"unicode"` | Fold marks and arrows as `▸ ▾ ↑ ↓`, or as `> v` and words for a terminal font without them |

The settings panel of Kalem lists them from the manifest's `settings` table (`plugin_settings.rs` reads it: a type, a default, a description, choices or a range), as it lists the Elixir plugin's.

## 7. Both frontends, batch, languages

- **Terminal** (D26): every document, panel, quick-pick and notification is the same; `▸`/`▾` and `↑`/`↓` are written as ASCII with the setting `glyphs = "ascii"`; the diff's colors are the theme's `diff` syntax colors where the terminal has true color, else bold and dim. The status bar item and the key line are there. Alt with the arrows and Enter reaches most terminals as an escape prefix; where a terminal does not send it, `terminalKeys` (§7.3) gives Alt+Enter as `SPC g /`'s twin and the section jumps as Vim's `{` and `}`, and the status bar line says which. The mouse works where the terminal gives it.
- **Graphical:** the same, plus clicks on the fold marks, double clicks to open, the right-click menu, buttons in the panel, and the Git menu.
- **Batch:** the plugin's commands run from `kalem run git.status`, which prints the status document's text, and the example (`examples/git.rs`) does the same without Kalem. A `kalem git` subcommand (the CLI extension point, `coverage.toml`'s "CLI subcommands") is phase 3.
- **Languages:** the plugin's strings in English and Turkish, chosen by Kalem's `ui.language` setting (read through `settings.get`), as plugins bring their own strings (§7.4). Git's own messages stay git's.
- **Accessibility:** the status is text, so a screen reader reads it (§7.4); the fold marks are announced by their words; the arrows and Enter are the keys a screen reader user expects of a tree.

## 8. Phases

Each phase ships as a release of the plugin (`git-vX.Y.Z`, W2's rule for the API it binds), with its tests, in both frontends.

**Phase 0: today's API, plus `process`.** Needs 9.1 only. The crate from the template; the model, the parsers and the patch builder as a library with the example CLI (`status`, `log`, `show`, `blame`, `patch`), tested natively against a real git. In Kalem: the status bar item; the Git panel (2.9) with its checkboxes, buttons and progress; `SPC g S`, `SPC g U`, `SPC g R`, `SPC g D`; `SPC g c c` with a one-line message through `ui.prompt` (the message document waits for phase 1); `SPC g F`; push and pull from the panel's buttons and `SPC g /`, with their quick-picks; `SPC g B` as a notification; `SPC g /` and `SPC g .`. *Exit:* the owner stages, commits and pushes a change to `getkalem/plugins` from Kalem without a terminal.

**Phase 0, as built** (2026-10-07). The library, then `src/app.rs`, the plugin as a state machine of inputs (a command with its document, a git run's end, an answer, a click in the panel, an event) and effects (run git, notify, the status bar item, the panel, a question), tested with a real git and scripted answers (`tests/app.rs`), and `src/component.rs`, which turns the effects into `kalem_plugin` calls. The component was run in Kalem's host as the editors run it: its commands from the registry with their keys, its git runs through `process`, the panel, the status bar item, the commit message asked through Kalem's own prompt, a line's blame. Where it differs from the plan above: `SPC g g` shows the panel through Kalem's `view.pluginPanel`, which shows or hides it, so a second `SPC g g` hides it until the status document of phase 1; the setting `panel` is not read yet (the panel is always there); `SPC g D` (delete) came with the others; a repository works only inside one of Kalem's projects (the host's rule for `process`), and a command outside them says to add it (`SPC p a`). Waiting on others: Kalem's `main` with `process` on GitHub, so that this repository's lock takes it and CI builds the component.

**Phase 1: the documents.** Needs 9.2. The status document (2.1 to 2.3) with the arrows, Enter, Alt+Enter and Escape, files, hunks and lines staged, unstaged and discarded by the leader's keys; the commit message document, the log and the commit view (2.4); blame and the time machine as documents (2.5); the remote with `SPC g o o` and `SPC g y` (needs 9.5); the process log; the status bar's key line. *Exit:* a week of the owner's own git work done in Kalem; the leader table's sixteen `SPC g` rows bound; the terminal snapshots of every document.

**Phase 2: the editor side.** Needs 9.3 and 9.4. Gutter marks on save and while typing; `SPC g s`, `SPC g u` and `SPC g r` in a file; the core's `SPC g [` and `]`; `SPC t d`; inline blame; `.git` watched; branches, merges and rebases through `Head:`'s menu with the in-progress line and the Unmerged section, Continue, Skip and Abort, conflict markers highlighted in the file with "keep ours", "keep theirs", "keep both" in the file's menu (edits through `editor.replace`); cherry-pick, revert and reset from a commit's menu; "Add to .gitignore" (`fs:write:workspace`); the commit message's 72-column mark. *Exit:* a rebase with conflicts resolved inside Kalem.

**Phase 3: the rest.** Interactive rebase through an editable generated document (the todo list, saved like the commit message; git's sequence editor served by Kalem, 9.6); `GIT_ASKPASS` through Kalem's prompt; intent-to-add for parts of new files; tags (Enter on `Tags:`); remotes (add, remove, rename); submodules and worktrees listed; word-level highlighting inside changed lines; the file's own language highlighted inside hunks (the core's `diff` language pack, T2.7a.7); `kalem git` on the command line; bisect.

## 9. Changes to Kalem's core

The plugin needs eleven changes in `getkalem/kalem`. Three are new plugin interfaces that other plugins need too (`process`, `documents`, `decorations`); the rest are small. The table says what and when; a subsection per change says where in the core it goes, how it behaves, how it is tested, and what it changes in the documents. Appendix A has the WIT.

| | Need | Exists today | Where | Phase | Size |
|---|---|---|---|---|---|
| 9.1 | **`process`**: run a program named by `subprocess:PROGRAM`, asynchronously, in a project's folder; `on-process` with the exit and the output; `kill` | No. `Grants` says "others (`subprocess`) are not the host's"; the spawning exists in `kalem-lsp` and `lsp::run_formatter` | `kalem-plugin/wit/process.wit`, `kalem-script::extension`, `kalem-core::extensions`, `plugin_store.rs`, the template's `SCOPES` | 0 | S–M |
| 9.2 | **`documents`**: a document the plugin writes, read-only or editable, with a text type, a highlighter, styles and regions; `on-document` | No. Precedents: `DocumentMode::Directory` with `read_only`, `file.scratch`, `Request::OpenAt` | `documents.wit`; `DocumentMode::Generated` in `kalem-core`; both frontends' rendering | 1 | M |
| 9.3 | **Decorations** (T3.1.9c, R5.13): gutter marks and virtual text by line, per path; the core's previous-change and next-change motions over the marks | Planned | `decorations.wit`; both editors' gutters; `keymaps/vim.json` | 2 | M |
| 9.4 | `workspace-file-changed` for the state files of `.git` | No: `kalem-project` filters `.git` out | `kalem-project::files` | 2 | S |
| 9.5 | Open a URL and set the clipboard from a plugin | The core has `Request::OpenLink` and the clipboard; no command takes a URL or a text as an argument | `builtin.rs`: `link.open`, `edit.copyText` | 1 | S |
| 9.6 | Kalem as git's editor and askpass | No | `kalem-cli`, `kalem-core::extensions` | 3 | M |
| 9.7 | `file.open` with `line` and `column` | `Request::OpenAt` and `edit.gotoLine` exist | `builtin.rs` | 1 | XS |
| 9.8 | Styles on an editable generated document, and an `edited` event | Part of 9.2 | `documents.wit`, `kalem-core` | 2 | S |
| 9.9 | The leader table and which-key: rows bound by a plugin, nested prefix names | `+git` is named; rows say "needs the git plugin" | `tests/keys/doom-leader.toml`, `key_tables.rs`, `keymap.rs`, `leader_keys.rs` | 1 | XS–S |
| 9.10 | Theme colors for the `diff` syntax's scopes | No: `[syntax]` has keyword, string, comment, number, function, type | `kalem-core/themes/*.toml`, `kalem-highlight` | 1 | XS |
| 9.11 | Alt with the arrows and Enter in the terminal | Alt is decoded (`KeyModifiers::ALT` in `kalem-tui`); the five keys are not tested | `kalem-tui`, the manual's terminal page | 1 | XS |

**Versions.** The `extension` world is not frozen (`wit-frozen/` holds the viewer interfaces only), so its `plugin` interface can gain `on-process` and `on-document` before its first release. Each new interface comes in a patch version of the API (`process` 0.2.4, `documents` 0.2.5, `decorations` 0.2.6), is copied to `wit-frozen/` when a plugin is released against it, and never changes after, as the Book's "Versions of the plugin API" says (W2).

**Order.** 9.1 first (the plugin's phase 0 needs nothing else); then 9.2 with 9.5, 9.7, 9.9, 9.10 and 9.11 for phase 1; 9.3, 9.4 and 9.8 for phase 2; 9.6 for phase 3. In the roadmap, R5.15 splits into R5.15a (`process`), R5.15b (`documents`), R5.15c (the plugin), and 9.3 is the first item of R5.13 brought forward.

### 9.1 The `process` interface

**What.** A plugin runs a program its manifest names, without a shell and without waiting, and hears how it ended. Appendix A.1 has the WIT.

**Why the plugin needs it.** Everything the plugin does is a git command (3.3). Today a component has no way to start a program: `Grants::from_permissions` reads `fs:…` and `net:fetch:…` and says of `subprocess` that it "is not the host's".

**The permission.** `subprocess:PROGRAM`, one per program, the name alone (`git`; no path, no `..`, lower case). A bare `subprocess` keeps its meaning for declarative language plugins, whose servers the core runs (the Elixir plugin's manifest has it), and grants no `process` interface. `Grants` gains `programs: Vec<String>` and `Grants::process()`; the `process` interface is added to the linker only when a program is granted, as `fs` and `net` are added only with theirs (`kalem-script/src/extension.rs`, the `add_to_linker` calls), so a component that imports `process` without the grant is refused at link with the import named, as today.

**The plugin's side** (`crates/kalem-plugin`). A module `process` over the `extension` world, shaped like `net`: `process::run(command, done)` returns the run's number and keeps `done` in the thread-local table the fetches use; `kalem::dispatch_process(run, result)` is called by the export `on-process` that `export_plugin!` writes. A builder: `Command::new("git").args(["status", "--porcelain=v2"]).cwd(root).stdin(bytes).env("GIT_TERMINAL_PROMPT", "0")`.

**The host's side** (`crates/kalem-script/src/extension.rs`). `Session` gains `running: BTreeSet<u64>` beside `fetching`. `impl process::Host for Session`: `run` refuses at once a program that is not granted ("`PROGRAM` is not a program the plugin may run"), a program that is not a bare name, a `cwd` outside the folders of `Editor::workspace()` (the check `reach` makes for `fs`), and more than 16 runs of one plugin at a time ("too many programs running"); otherwise it numbers the run and calls `Editor::spawn(plugin, run, command)`. `kill` calls `Editor::kill(run)`. The `Editor` trait gains those two. The exit comes back through `Extension::process_done(run, result)`, which removes the run from `running` and calls the guest's `on-process`, as the fetch's response does at `fetching.remove`; it is called on the editor's thread at the next tick, never from the reading threads.

**The editor's side.** The `Editor` implementation of `kalem-cli/src/extensions.rs` (the bridge) and `kalem_core::extensions`. `spawn` finds the program with `kalem_core::extensions::find_program`: where the user's setting `programs.NAME` in the plugin's table says, else on the PATH as the language server client finds a server (`kalem_lsp::find_program`). A program not found ends the run at once with "git was not found: install it, or give its path as the setting programs.git of the plugin". Otherwise `start_run` runs it on a thread of its own (`run_program`): the arguments as given, the folder, the user's environment plus the plugin's variables, the three streams piped, `CREATE_NO_WINDOW` on Windows; a thread writes the standard input and closes it (as `lsp::run_formatter` does); two threads read the outputs, each kept to `MAX_OUTPUT` (16 MB), `truncated` set past it and the rest drained; the child is polled (1 to 20 ms) so that `kill_run` or ten minutes can stop it, the end arriving then without a status. The end is handed to the plugin from the run's thread under the plugins' lock (`process_done`), as a fetch's response is. The plugin's runs are killed when it is deactivated. (Written on Kalem's branch `git-process`; see "What was built" below.)

**What the user sees.** The installer's summary already had a line for a language plugin's servers, "Runs programs on this computer: … (when installed; Kalem never installs them)"; a `subprocess:NAME` permission names its program there ("Runs programs on this computer: git") and not again among the other permissions. The decision is kept in `plugins.toml` as other permissions are. The plugin console (§11.9) will list the runs with their durations.

**Tests.** `kalem-script` (`tests/extension.rs`, with a new test plugin `tests/plugins/run`): the grant read from the permissions (`subprocess:git` and `subprocess:git-lfs` granted; a bare `subprocess`, `subprocess:`, `subprocess:..` and `subprocess:/bin/sh` not); a component importing `process` without the grant refused, the interface named; a run started with its arguments and folder, and in the first project without one; refused at once: another program, a path as the program, a folder outside the projects, a relative folder, a missing folder; the end delivered once, a stranger's dropped, a start's error delivered; sixteen runs at most; `process.kill` reaching the editor; deactivation killing the runs. `kalem-core` (`extensions::process_tests`, on Unix): a program reading its input, writing both outputs, in its folder, with the plugin's variable, ending with its status; output past 16 MB cut while the program still ends; a program killed when asked and when late; a missing program an error; the program found on the PATH, or where `programs.NAME` says. `plugin_store`: the installer's summary naming the program. The template's conformance test of `getkalem/plugins` accepts `subprocess:git` and refuses `subprocess:` and `subprocess:/usr/bin/git`.

**Documents.** Design §11.4 adds `pub mod process`; §11.6 writes the scope as `subprocess:<program>`; the Book's Part III (`plugins.org`) gets the permission's row, a section on running programs and the version 0.2.4; CHANGELOG. `wit-frozen/0.2.4/process.wit` comes with the plugin's first release.

**What was built** (2026-10-07, Kalem's branch `git-process`): `crates/kalem-plugin/wit/process.wit` and the package at 0.2.4 (`kalem_script::API_VERSION`); `on-process` in `kalem.wit`'s `plugin` (its parameter is `outcome`: `result` is a keyword of WIT); `kalem_plugin::process` (`Command`, `run`, `kill`) and `on_process` in `export_plugin!`; `Grants::programs`, `MAX_RUNS`, `process::Host` for the session and `Extension::process_done` in `kalem-script`; `ProcessRequest`, `ProcessExit`, `start_run`, `kill_run`, `fail_run`, `run_program`, `find_program` and `process_done` in `kalem_core::extensions`; the bridge's `spawn` and `kill` and the plugins' `process_done` in `kalem-cli`; the installer's summary. The bare `subprocess` of language plugins keeps its meaning.

### 9.2 The `documents` interface

**What.** A plugin opens a document of its own, read-only or editable, writes its text with styles and regions, hears when it is shown, saved or closed, and asks which of its documents a command runs in. Appendix A.2 has the WIT.

**Why the plugin needs it.** The status, log, commit, blame, revision, message and process views are documents (3.2). The core has every piece but the interface: a read-only document with a mode of its own (`DocumentMode::Directory`, `DocumentState::read_only`), a document without a file (`file.scratch`, `DocumentsRequest::Scratch`), opening at a place (`Request::OpenAt`), and the styles of a line decided by a module (`kalem_core::dired`).

**The manifest.** A `documents` table lists the plugin's kinds with a title each (`{ "git-status": { "title": "Git status" }, … }`, beside `settings`), so the text types are known when the plugin is installed: the palette lists commands by type, `kalem commands --type git-status` works, and the report of bindings that never apply (§11.2) counts them as known. `plugin_store.rs` reads them; the command registry's type vocabulary takes them when the plugin is enabled.

**The plugin's side** (`crates/kalem-plugin`). A module `documents`: `open(&spec) -> Result<Doc>`, `set(doc, &content, at)`, `current()`, `close(doc)`; a `Content` builder that appends lines with a style and opens and closes nested regions by key, so that a view is written top to bottom; `kalem::dispatch_document` for the export `on-document`.

**The editor's side** (`crates/kalem-core`).

- `DocumentMode::Generated { kind: String, language: Option<String> }` in `mode.rs`. Its text type is `kind` (where `Directory` maps to `"directory"` today); its highlighter is `language`, chosen as `Text { language }` chooses one. `DocumentState` gains `generated: Option<Generated>` with the plugin's ID, the spec's `id` and `key`, the host's document number, the regions, the styles, and `editable`; `read_only` is `!editable`. It has no path: the commands that need a file (save as, reveal, rename) are not offered, by `document_context` (§11.2); its title stands in the open-files list and the tab.
- **Opening.** `open` asks the editor through a `DocumentsRequest::Generated(spec)` beside `Scratch`. A document already open with the same plugin, `id` and `key` is shown again, not made again. It opens in the current pane; a plugin that wants a split runs `pane.splitRight` first (3.2).
- **`set`.** Replaces the whole text in one version step, with no undo history for a read-only document. The cursor goes to the first line of the region named by `at` when there is one, else to the same line number, clamped, the column kept; the scroll stays where the cursor stays visible. For an editable document, `set` is refused while it is modified ("the document has unsaved edits").
- **`current`.** The host's `Editor::document()` already names the document a command runs in; `current` answers its host number when it is a generated document of the calling plugin.
- **Events.** `shown` when the document becomes the active one (opened, or its tab chosen); `closed` when it is closed, after which the host drops its state; `saved(text)` when `app.save` runs on an editable one: no file is written, `modified` clears, and the text goes to the plugin; `fold-toggled(key)` when the user clicks a region's fold mark. Closing a modified editable document asks as any document does. A plugin disabled or deactivated closes its documents with a notice.
- **Rendering**, in both frontends through the plain-text path with the `language` highlighter. `styles` map `text-style` to the theme as the panels of D11 map it already: `heading` the headline color, `strong` bold, `emphasis` italic, `muted` the dimmed text, `code` the code color, `error` the error color; a style wins over the highlighter on its span. The plugin writes a foldable region's mark (`▸`, `▾`) as the first character of its first line, so the editor draws nothing of its own; a click on that character of a `foldable` region sends `fold-toggled`.
- **Keys.** The plugin's commands scoped to the kind bind `enter`, `escape`, `tab`, `shift+tab`, `right`, `left`, `alt+up`, `alt+down` and `alt+enter`. A binding scoped to a text type must come before the editor's own use of those keys and before the Vim layer's normal-mode handling, as `dired.open` on Enter does in the file manager; `escape` is the one to check, since the Vim layer takes it to clear the search highlight: a scoped binding whose `when` holds (`!hasSelection`) wins. `keymap.rs`' precedence, with a test.
- **Batch.** Under `kalem run`, `open` and `set` print the text to standard output; `current` answers none; `on-document` is never sent.

**Tests.** `kalem-core`: open, set, current; the cursor kept by region and by line; a read-only document refuses typing and paste; an editable one delivers `saved` and clears `modified`; `set` refused while modified; the same `id` and `key` opened twice is one document; closing sends `closed`; the title in the open-files list; `document_context` hides the file commands. `kalem-script`: the test extension plugin opens a document and receives `shown`, `saved` and `fold-toggled`. `kalem-tui` and `kalem-ui`: snapshots of a document with every style, a fold mark folded and not, the `diff` highlighter under a `muted` style; the keymap's precedence for Enter, Escape, Tab and the arrows scoped to a kind, in both profiles. The template's conformance test checks `documents`.

**Documents.** §11.10 gets the row "Documents | A text the plugin writes, read-only or editable | Editor area | Editor area | `kalem run` prints it"; §11.4 adds `pub mod documents`; `coverage.toml` gets the point "Documents" with `interface = "documents"`; the Book's Part III gets the page "Documents a plugin writes"; CHANGELOG; `wit-frozen/0.2.5/documents.wit`.

### 9.3 Decorations: the git plugin's part of T3.1.9c

**What.** Gutter marks and virtual text by line, per file, set by a plugin and drawn by both editors; the core's motions over the marks. Appendix A.3 has the subset the git plugin needs; T3.1.9c's full contract (highlights, badges) can grow around it.

**Why the plugin needs it.** The diff marks in the gutter, inline blame, and `SPC g [` and `]` (2.8); T2.7i.11 names "the gutter marks and the hunk motions when a `git` plugin registers decorations" as the core's part.

**The interface** (`decorations.wit`, 0.2.6, imported by `extension`, no permission): `set-gutter(path, marks)` with a `line-mark` per line (`added`, `changed`, `removed`); `set-virtual(path, id, texts)` with a `virtual-text` per line (the text, a `text-style`, `before-line` or `end-of-line`); `clear(path, id)`. At most 10,000 marks and 10,000 virtual texts per document; more are refused with the count.

**The editor's side.** Decoration sets kept per open document, keyed by plugin and `id`, dropped when the document closes or the plugin is disabled. A mark names a line as the document was when it was set; the editor moves it with later edits, as it moves bookmarks (`bookmarks.rs`) and the language servers' diagnostics, until the plugin sets the marks again. Drawing: in the window, a bar in the gutter beside the line number in the theme's `inserted`, `changed` and `deleted` colors (9.10), a removed mark as a small triangle between the lines; in the terminal, a one-column gutter with `▎` (`+`, `~` and `_` as the fallback) in the same colors. Virtual text is a run of glyphs that stands for no source, drawn before the line or after its end in the given style: `tui-rich-text`'s `Glyph::decoration` and `kalem-ui/src/line.rs`'s `prepare_decoration` are the hooks; it is not selectable, not searched, not saved, and the motions skip it.

**The motions.** Two commands of the core, previous-change and next-change (their names the core's), move to the previous and next gutter mark of any plugin in the document and say "no more changes" at the ends; bound to `SPC g [` and `SPC g ]` in `keymaps/vim.json`, the leader table's rows.

**Tests.** `kalem-core`: marks moved by edits above, inside and below them; the limits; `clear` by `id`; the motions. Both frontends: snapshots of the three marks and of virtual text in each style, in the light and the dark theme. The conformance suite's contract test for the interface.

**Documents.** `coverage.toml`'s "Decorations" moves from `planned` to `interface = "decorations"`; §11.10's row stays; the Book's Part III page "Decorations"; `wit-frozen/0.2.6`.

### 9.4 The file watcher and `.git`

**What.** `workspace-file-changed` events for the files of `.git` that say what the repository is doing, while the project's file list keeps ignoring `.git`.

**Why the plugin needs it.** A commit, checkout, fetch or rebase made in a terminal changes the status; the plugin cannot see it today (3.5).

**The change** (`crates/kalem-project/src/files.rs`). The watcher already receives events under `.git`, since the project's root is watched; `comes_or_goes` filters them out, rightly, for the file list. A second filter, `vcs_state_changed(root, path)`, accepts the paths `.git/HEAD`, `ORIG_HEAD`, `FETCH_HEAD`, `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG`, `index`, `packed-refs`, `logs/HEAD`, and anything under `refs/`, `rebase-merge/` and `rebase-apply/`, and refuses `*.lock` (git writes a lock file and renames it) and everything else (`objects/` above all). The event path of the host (`kalem-core`'s `workspace-file-changed` emission) sends these to the plugins, debounced at 100 ms per path; the file list is untouched. A worktree's `.git` is a file whose `gitdir:` points outside the project, not watched: a documented limit, covered by the refresh on the plugin's own commands and on `SPC g g`. Only `.git` for now; the list of folders is one constant beside `VCS`.

**Tests.** `events_that_change_no_file_found` keeps asserting that `.git/index` changes no file; a new test asserts the event for `.git/index`, `.git/HEAD` and `.git/refs/heads/main`, and none for `.git/objects/ab/cdef`, `.git/index.lock` and `.git/config`.

### 9.5 A URL and the clipboard from a plugin

**What.** Two built-in commands a plugin runs through `kalem.run`: `link.open { url }` and `edit.copyText { text }`.

**Why the plugin needs it.** `SPC g o o` opens a page; `SPC g y` and "Copy hash" set the clipboard (2.6). No command takes a URL or a text as an argument today; the core has `Request::OpenLink(LinkAction::Url(…))` and `ctx.clipboard`.

**The change** (`crates/kalem-core/src/builtin.rs`). `link.open`: argument `url`, a string, required; the scheme must be `http`, `https` or `mailto` (a plugin opens no `file:` and no custom scheme); pushes `Request::OpenLink`. `edit.copyText`: argument `text`, required; records it on the clipboard as `edit.copy` does. Both of scope `all`, with no default keys and not in the menus (they are for plugins and keymaps), with their schemas in the argument table beside `file.open`'s. The alternative is a `ui-2` interface with `open-url` and `copy`; the commands cost no WIT and serve keymaps too, so they are proposed.

**Tests.** The commands' golden tests; a refused scheme with its message.

### 9.6 Kalem as git's editor and askpass (phase 3)

**What.** `kalem --edit-for SOCKET FILE` opens `FILE` in the running Kalem and exits when it is saved and closed (0) or closed unsaved (1), as `emacsclient` does; `kalem --askpass SOCKET PROMPT` asks through the running Kalem's prompt, hidden as typed, and prints the answer (exit 1 when cancelled).

**Why the plugin needs it.** An interactive rebase's todo list and git's password prompts (3.7, phase 3). Git runs `GIT_SEQUENCE_EDITOR` and `GIT_ASKPASS` through a shell, so the plugin passes `GIT_SEQUENCE_EDITOR="$KALEM_BIN" --edit-for "$KALEM_SOCKET"` and `GIT_ASKPASS="$KALEM_BIN" --askpass "$KALEM_SOCKET"`.

**The change.** The host adds `KALEM_BIN` (the running executable) and `KALEM_SOCKET` to the environment of every `process.run`. The socket is a private one the editor opens for this purpose in the state directory (mode 0600; a named pipe on Windows), not the debug socket of §11.9 (off by default, and wider); it accepts two requests: open a file and wait for its save and close, and ask for a secret. `kalem-cli` gets the two flags. The answer of `--askpass` goes to standard output only, never to the log.

**Tests.** `kalem-cli` golden tests of both flags against a test editor; a `git rebase -i` in a temporary repository with the todo edited by the test.

### 9.7 `file.open` with a line

**What.** `file.open` takes optional `line` and `column`; given, it opens the file there (`Request::OpenAt`, which the project search uses) instead of at the top.

**Why the plugin needs it.** Enter on a diff line opens the file at that line (2.2). The plugin can run `file.open` then `edit.gotoLine` today; one command is cleaner and does not scroll twice.

**The change.** The schema entry of `file.open` in `builtin.rs` gains `line` and `column` (integers, optional); the handler pushes `OpenAt` when `line` is given. A test.

### 9.8 Styles on an editable generated document

**What.** Part of 9.2, in phase 2: styles set on an editable document stay while its text is the version they were set for and are dropped at the first edit; `on-document` gains `edited(version)`, debounced as `document-changed` is, so the plugin can read the text (through `editor.text` in a command it runs) and set the styles again.

**Why the plugin needs it.** The commit message's 72-column mark (2.4).

### 9.9 The leader table and the which-key popup

**What.** The sixteen `SPC g` rows and `SPC t d` in `tests/keys/doom-leader.toml` stop saying "needs the git plugin" and name the command and the plugin that binds them; the which-key popup shows the plugin's title when the plugin is installed and "(plugin: Git)" with the plugin's ID when it is not; nested prefixes under the leader get Doom's names.

**The change.** `key_tables.rs` reads a new field `plugin = "org.kalem.git"` on a row with `command`; `leader_keys.rs` skips a row whose plugin is not installed in the test, since it runs the commands it finds; `Keymap::which_key` looks up the registered binding first, then the table (today it marks "(plugin)" without saying which); `keymap.rs`' group names gain the nested `g c` → `+commit` and `g o` → `+open in browser`, Doom's names, beside the top-level `g` → `+git`. Both tests of the tables run on the new field.

### 9.10 Theme colors for diffs

**What.** The `[syntax]` tables of `kalem-core/themes/light.toml` and `dark.toml` have six roles (keyword, string, comment, number, function, type); the `Diff` syntax's scopes `markup.inserted`, `markup.deleted`, `markup.changed`, `meta.diff.header` and `meta.diff.range` fall to the default color, so a diff is not green and red today.

**The change.** Four roles added to both themes, `inserted`, `deleted`, `changed` and `diff-header`, mapped from those scopes in `kalem-highlight`'s scope-to-role table, and used by the gutter marks of 9.3 for the same meanings; a user theme in `themes/` overrides them as any role. The terminal uses the same roles where it has true color, bold and dim where it has not.

**Tests.** A snapshot of a `.diff` file in both editors and both themes.

### 9.11 Alt with the arrows and Enter in the terminal

**What.** `alt+up`, `alt+down`, `alt+left`, `alt+right` and `alt+enter` reach the keymap from the terminal.

**The change.** `kalem-tui` reads `KeyModifiers::ALT` already (`app.rs`); the five keys are added to the key parser's and the input's tests, with the escape-prefixed forms and the kitty keyboard protocol's; the manual's terminal page says that Terminal.app and iTerm2 need Option set to send Meta, as it says for Alt+P. Where a terminal does not send them, the plugin's twins stand (section 11) and the status bar line names them.

## 10. Testing

| Layer | Method |
|---|---|
| Parsers (`git/`) | A corpus of recorded git outputs in `tests/corpus/`: status v2 with every entry kind (ordinary, renamed, unmerged, untracked, ignored, submodule), ahead and behind, detached head, an in-progress rebase; diffs with new, deleted, binary, mode-changed and renamed files, no-newline, CRLF; logs with the graph; blame with boundary and uncommitted lines; outputs of git 2.23 and the current release. Snapshots (insta) of the parsed model |
| Patch builder | The corpus's hunks with the expected patches for whole hunks and for selected lines, both directions; a property test (proptest) in a temporary repository with a real git: random edits, random selections, `git apply --cached` must succeed, the index must equal `git add -p`'s choice, stage then unstage must restore the index. Runs where git is installed (CI has it) |
| Views | Snapshots of each document's text, styles and regions from fixed models; the cursor-to-region map and each region's menu; the fold state across a refresh; what Enter does on every kind of line; a 10,000-entry status and a 50,000-line diff rendered under 100 ms (a test with a ceiling, as `component_speed.rs`) |
| Remote URLs | A table of remote URLs (https, ssh, scp-like, with and without `.git`, self-hosted) to web links for files, lines, ranges and commits |
| The plugin | The fake host (T3.1.17's conformance suite): scripted answers to runs, every command exercised in a file and on every kind of region, every confirmation declined and accepted, every error path; the manifest's conformance test; a test that no key but the arrows, Tab, Enter, Alt+Enter and Escape is bound with a git document's scope |
| In Kalem | `kalem-tui` tests with the built component, as the viewers' (W7): a temporary repository, `SPC g g`, Right on a file, `SPC g s` on a hunk, `SPC g c c`, save, the snapshot of the screen; the same in `kalem-ui`'s harness |
| Manual, per release | macOS, Linux and Windows (Git for Windows: paths, CRLF, `NUL`, no console window); terminals (Alt with the arrows and Enter in Terminal.app, iTerm2, kitty, WezTerm, Windows Terminal); a failing `pre-commit`; a signed commit; a credential helper; a worktree; a submodule; a repository with 100,000 files |

## 11. Risks

- **The three interfaces are not there.** The plugin's phase 0 is thin without `documents`. Mitigation: the library with the example CLI is useful and tested from the first week; the interfaces are small and serve other plugins; the owner holds both repositories.
- **Three keys to stage.** `SPC g S` is three keystrokes where lazygit has one. Accepted by the owner's direction: a Doom user types them already in every buffer, the key is the same in a file and in the status, and the menu (Alt+Enter) is two. If it proves slow in use, the first thing to try is the owner's own binding in `keymap.json`, not a letter in the plugin.
- **Alt in terminals.** Alt+Enter and Alt with the arrows do not reach every terminal (9.11). Every one of them has a twin without Alt: `SPC g /` for the menu, Vim's `{` and `}` for the sections, the time machine's and blame's steps in the item's menu; the status bar line names the twin where the terminal lacks the key.
- **Large repositories.** `git status` itself takes seconds on a cold 100,000-file repository (git's own `core.untrackedCache` and `core.fsmonitor` help, and the README says so); the plugin shows "Refreshing…" in the status bar and never blocks. Diffs are loaded per file and capped.
- **Windows.** Program resolution, paths, CRLF and the console window are the host's; the plugin's tests cover CRLF patches. First release on macOS and Linux, Windows in the release checklist.
- **Scope creep.** Section 13 says what the plugin is not; every feature beyond phase 3 is a new row of the roadmap, not a quiet addition.
- **A user's expectations from magit or lazygit.** No transients, no single letters, no forge. The README names what each will miss and where it went (the item's menu, the leader map, the palette, a later plugin).

## 12. Decisions

Proposed; the owner decides. Numbered G1 to G12 here so as not to take Kalem's D-numbers.

| ID | Decision | Options | Proposed | Why |
|---|---|---|---|---|
| G1 | How git is run | The `git` program through `process`; gitoxide in the component over raw file access; libgit2 (C, not allowed by D54's pure-Rust rule) | **The program** | The user's config, hooks, credentials, signing, worktrees and submodules work; no binary file access in the sandbox; lazygit and magit do the same (3.3) |
| G2 | What the views are | Documents the plugin writes; widget-tree panels (D11); a view of the editor area (T3.1.9d) | **Documents** | A diff is read, searched, selected and visited like text; the same in both frontends; the precedent of Dired (3.2). The panel stays for the file list |
| G3 | Keys | lazygit's single letters; magit's transients; the arrows, Tab, Enter, Escape and Doom's `SPC g` map, with menus where Doom has no key | **Arrows, Enter, Escape; Doom's map; menus** (the owner's direction, 2026-10-07) | One vocabulary in a file and in the status; nothing new to learn beyond what Kalem lists already; a tree's keys for a tree (section 5) |
| G4 | Where the status opens | In place, Escape returns; a split; a bottom panel | **In place** by default, a split by setting | magit's and Dired's way; the whole pane for the diff; the terminal has no room for a split |
| G5 | Permissions | `subprocess:git` only; plus `fs` | **`subprocess:git` only** in phase 1 | Everything the plugin knows comes from git; the one file write (`.gitignore`) is phase 2's and optional |
| G6 | Blocking | Asynchronous runs with generations; synchronous runs within the budget | **Asynchronous** | A push takes seconds; §11.6's 100 ms; `net.fetch`'s shape |
| G7 | The commit message | An editable document, saved to commit; a prompt; a panel input | **A document; saving commits, closing cancels** | Several lines, the editor's keys, the diff under it; no new key to learn |
| G8 | Safety | Confirm destructive actions; a backup stash before every discard (magit's wip modes); nothing confirms | **Confirm, name what is lost; force only with lease** | Simple and honest; a backup stash is state the user did not ask for |
| G9 | Gutter base | The index; HEAD | **The index**, HEAD by setting | The marks show what `SPC g s` would stage, so they agree with the status |
| G10 | Minimum git | 2.23; older with fallbacks; a bundled git | **2.23**, no bundle | `restore` and `switch`; 2.23 is from 2019; bundling a git is not a plugin's job |
| G11 | Blame | A document first, inline later; inline only | **Document first** (phase 1), inline with decorations (phase 2) | The document needs nothing new; inline needs T3.1.9c |
| G12 | Names | | Plugin `git`, id `org.kalem.git`, crate `kalem-plugin-git`, commands `git.*`, category "Git", text types `git-status`, `git-log`, `git-commit`, `git-commit-view`, `git-blame`, `git-revision`, `git-process` | The conventions of §11.2 and of the other plugins |

## 13. Non-goals

- **Forges:** pull requests, issues, reviews, CI status. A later `forge` plugin on `net:fetch:github.com` and the `documents` interface, as magit's forge is a package of its own.
- **A three-pane merge tool.** Conflicts are resolved in the file, with the markers highlighted and ours/theirs/both in the file's menu (phase 2).
- **A commit graph drawn as a picture.** `--graph`'s ASCII in the log is the graph.
- **Other version control systems.** Mercurial, Jujutsu and Subversion are sibling plugins on the same two interfaces.
- **Managing git itself:** installing git, editing `.gitconfig` (a text file; Kalem opens it), credential storage, GPG keys.
- **A terminal.** Kalem has none; the process log shows what ran, and a user who needs more runs git in their terminal; the status refreshes.
- **lazygit's letters.** Deliberately (G3); `keymap.json` binds any of them for a user who wants them.

## 14. References

- Kalem's design document: §2.7 (the file manager, the precedent), §2.8 (projects), §7.3 (keys, the Doom leader map, `terminalKeys`), §11.2 to §11.6 (commands, events, the API, the package, security), §11.10 (extension points: decorations, panels), §11.13 (viewers, for the contract's shape), D11, D26, D28, D29.
- `docs/todo.md` T2.7i.11 (the `SPC g` map, the core's part), T2.7i.19 (which-key), T2.7a.7 (the diff language pack), T3.1.9c (decorations), T3.1.9d (views), T3.1.17 (the conformance suite); `docs/roadmap.md` R5.13, R5.15; `docs/wasm_todo.md` W2 (API versions), W10 (diagnostics, `kalem plugin dev`).
- `tests/keys/doom-leader.toml`: the `SPC g` rows and `SPC t d`.
- `crates/kalem-plugin/wit/`: `kalem.wit`, `ui.wit`, `access.wit`, `editor.wit`; `crates/kalem-script/src/extension.rs` (`Grants`); `crates/kalem-core/src/dired.rs`; `crates/kalem-lsp/src/client.rs` (spawning).
- Doom Emacs: the `+git` leader map of `modules/config/default/+evil-bindings.el` (`g g`, `g G`, `g S`, `g U`, `g R`, `g s`, `g r`, `g D`, `g c c`, `g c f`, `g c r`, `g F`, `g L`, `g t`, `g [`, `g ]`, `g o o`, `g y`, `g /`, `g .`).
- lazygit: its `pkg/commands/patch` package (line patches), its file watcher of `.git`; its keys, as what this plugin does not copy.
- magit: the status buffer and its sections, `magit-apply` (hunk and region patches), `magit-blame`, `with-editor` (the commit message, its scissors line); git-timemachine; browse-at-remote.
- git: `git-status(1)` porcelain format version 2; `git-diff(1)`; `git-apply(1)` (`--cached`, `--reverse`, `--unidiff-zero`); `git-cat-file(1)` (`--batch-check`); `git-commit(1)` (`-F -`, `--cleanup=strip`, `--fixup`); `git-blame(1)` (`--porcelain`); `git-log(1)` (`--format`); `gitrevisions(7)` (`@{upstream}`).

---

## Appendix A: the WIT sketches

Proposed interfaces, in the style of `crates/kalem-plugin/wit/`. Each is a new interface in a patch version of the API (W2); the `extension` world imports them, and `plugin` gains the two callbacks.

### A.1 `process.wit`

```wit
package kalem:plugin@0.2.4;

/// Programs (design §11.6's `subprocess`): a plugin runs a program its
/// manifest names (`subprocess:git`), never a shell, in a folder of a
/// project, and hears its end at `on-process`. The host finds the
/// program on the user's PATH and starts it without a terminal. A run
/// does not wait: the call returns the run's number, as `net.fetch`
/// returns a request's.
interface process {
    /// A program to run.
    record command {
        /// `git`; refused at once when the manifest does not name it.
        program: string,
        /// Its arguments, as given: no shell, no quoting.
        args: list<string>,
        /// Its folder: inside a project Kalem knows, else refused; the
        /// first project's when none.
        cwd: option<string>,
        /// Written to its standard input, which is then closed.
        stdin: option<list<u8>>,
        /// Added to the user's environment.
        env: list<tuple<string, string>>,
    }

    /// How a run ended.
    record exit {
        /// Its exit status; none when a signal stopped it.
        status: option<s32>,
        stdout: list<u8>,
        stderr: list<u8>,
        /// Output past 16 MB was dropped.
        truncated: bool,
    }

    /// Starts `command`; the exit arrives at the plugin's `on-process`.
    run: func(command: command) -> result<u64, string>;

    /// Stops run `run`; its exit still arrives, with no status.
    kill: func(run: u64);
}
```

In `plugin`: `on-process: func(run: u64, outcome: result<exit, string>);` (`result` is a keyword of WIT) (an error for a program that could not start).

Permission: `subprocess:PROGRAM`, one per program, shown as "Runs the program PROGRAM in your project folders"; `Grants` gains `programs: Vec<String>`; the template's conformance test accepts the prefix as it accepts `net:fetch:`.

### A.2 `documents.wit`

```wit
package kalem:plugin@0.2.5;

/// Documents a plugin writes: a text the editor shows as a document of
/// its own, as the file manager's listing is (design §2.7), so that the
/// cursor, the selection, search, Vim motions and scrolling work
/// unchanged, and the plugin's commands scoped to the document's kind
/// act on what the cursor is on. The plugin writes the text and says
/// what each part stands for; it draws nothing and folds nothing.
interface documents {
    use kalem.{range};
    use ui.{text-style};

    /// A document to show.
    record document-spec {
        /// `pluginId.name` (`git.status`).
        id: string,
        /// Tells the documents of one ID apart: a repository's root.
        key: string,
        /// In the open-files list and the tab.
        title: string,
        /// Its text type, for the scopes of commands (`git-status`).
        kind: string,
        /// Its highlighter (`diff`, `python`), if any.
        language: option<string>,
        /// Whether the user may edit it: the text saved arrives at
        /// `on-document`.
        editable: bool,
        /// The file it stands for, if any, for the editor's own commands.
        path: option<string>,
    }

    /// A stretch of the text and what it stands for.
    record region {
        range: range,
        /// The plugin's name for it: `file:src/lib.rs`, `hunk:src/lib.rs:2`.
        key: string,
        /// It folds: the plugin writes its mark first on its first
        /// line, and folds and unfolds it by writing the text again; a
        /// click on the mark sends `fold-toggled`.
        foldable: bool,
        folded: bool,
    }

    /// How a stretch of the text is shown; the highlighter does the rest.
    record styled {
        range: range,
        style: text-style,
    }

    /// A document's text and what it stands for.
    record content {
        text: string,
        styles: list<styled>,
        regions: list<region>,
    }

    /// Shows the document, or shows it again when open.
    open: func(spec: document-spec) -> result<u64, string>;

    /// Replaces the text. The cursor goes to the region named by `at`
    /// when there is one, else stays on its line.
    set: func(doc: u64, content: content, at: option<string>) -> result<_, string>;

    /// The plugin's document the running command is in, if any.
    current: func() -> option<u64>;

    /// Closes it; the document before it shows again.
    close: func(doc: u64);
}
```

In `plugin`: `on-document: func(doc: u64, event: document-event);` with `variant document-event { shown, closed, saved(string), fold-toggled(string), edited(u64) }`: `fold-toggled` carries the region's key when the user clicks its fold mark, `edited` the version of an editable document's text after a change (9.8).

The arrows, Tab, Enter, Alt+Enter and Escape are ordinary commands of the plugin (`git.unfold`, `git.fold`, `git.toggle`, `git.cycle`, `git.enter`, `git.menu`, `git.back`, `git.nextItem`, `git.previousItem`) with the keys `right`, `left`, `tab`, `shift+tab`, `enter`, `alt+enter`, `escape`, `alt+down`, `alt+up`, scoped to the tree-like text types; blame and the revision document bind `alt+left` and `alt+right` to `git.older` and `git.newer`. Escape's clause is `!hasSelection`, so the editor clears a selection first.

### A.3 What the git plugin needs of `decorations` (T3.1.9c)

The subset, for the planning of that task: `set-gutter(path, marks: list<line-mark>)` with `line-mark { line: u32, kind: added | changed | removed }`; `set-virtual(path, id, texts: list<virtual-text>)` with `virtual-text { line: u32, text: string, style: text-style, place: before-line | end-of-line }`; `clear(path, id: option<string>)`. The core's previous-change and next-change motions move over the gutter marks of any plugin.

## Appendix B: the git commands used

Every run has `--no-pager`, `-c color.ui=never`, `-c core.quotepath=off`, `GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=false`, and `LC_ALL=C` where the output is parsed.

| Purpose | Command |
|---|---|
| Repository | `git rev-parse --show-toplevel --git-dir --abbrev-ref HEAD` |
| Version | `git --version` |
| Status | `git status --porcelain=v2 --branch --show-stash -z --untracked-files=SETTING` |
| In progress | `git cat-file --batch-check` with `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD` and `refs/bisect/bad` on its standard input says, in one process, which operation is under way (a line `NAME missing` for each absent one). A rebase's step (`3/7`) lives in `rebase-merge/msgnum` and `rebase-merge/end`, plain files of the git dir that only `fs` could read; phase 2 takes the step from `git status`'s long form under `LC_ALL=C` (`rebase in progress; onto HASH`, `Last command done (N commands done)`), or asks `fs:read:workspace` for the two files if the owner prefers the porcelain-only rule kept strictly |
| Diff of a file | `git diff -U3 -- PATH` · `git diff --cached -U3 -- PATH` · `git diff --no-index -U3 -- /dev/null PATH` (untracked) |
| Counts | `git diff --numstat -z -M` · `git diff --cached --numstat -z -M` |
| Stage | `git add -- PATHS` · `git add -u` (the Unstaged heading) · `git add -A` · `git apply --cached -` (a patch on stdin) |
| Unstage | `git restore --staged -- PATHS` (a rename's both paths) · `git rm --cached -r -q -- PATHS` (no commit yet) · `git reset -q` (everything) · `git apply --cached --reverse -` |
| Discard | `git restore -- PATHS` · `git restore --source=HEAD --staged --worktree -- PATHS` (a staged file without unstaged changes) · `git apply --reverse -` (a hunk, lines) · the same then `git apply --cached --reverse -` (a staged hunk, lines, or a staged file with unstaged changes: the working tree first, so that nothing changes when it differs there). Every `apply` adds `--unidiff-zero` when the diffs have no context lines |
| Delete | `git clean -f -- PATH` (untracked) · `git rm -- PATH` (tracked) |
| Commit | `git commit --cleanup=strip -F -`, the message cut at the scissors line · `--amend` · `--amend --no-edit` · `--fixup=HASH` · `--signoff` |
| Message template | `git config --get commit.template` · `git log -1 --format=%B` (amend, reword) |
| Log | `git log --format=%H%x00%h%x00%an%x00%ae%x00%aI%x00%s%x00%D%x00 -n 200 [--skip N] [--graph] [--follow -- PATH] [HASH]` · `git show --format=%B --stat HASH` (a commit unfolded in the log) |
| Unpushed, unpulled | `git log … @{upstream}..HEAD` · `git log … HEAD..@{upstream}` |
| Tag | `git describe --tags --long`: `TAG-N-gHASH`, the tag and the commits since it in one process |
| Commit view | `git show --format=fuller --stat --patch -U3 HASH` |
| Blame | `git blame --porcelain [REV] -- PATH` |
| Revision | `git log --format=%H --follow -- PATH` · `git show REV:PATH` · `git show HEAD:PATH` (a deleted file opened) |
| Index version | `git show :PATH` |
| Stash | `git stash list --format=%gd%x00%s` · `git stash push [-m MSG] [--staged] [--include-untracked]` · `git stash pop|apply|drop stash@{N}` · `git stash show -p stash@{N}` |
| Branches | `git for-each-ref --format=%(refname:short)%x00%(upstream:short)%x00%(upstream:track)%x00%(HEAD) refs/heads refs/remotes` · `git switch BRANCH` · `git switch -c NAME [--track REMOTE/NAME]` · `git switch --detach HASH` · `git branch -d|-D NAME` · `git branch -m OLD NEW` · `git branch --set-upstream-to=REMOTE/NAME` |
| Merge, rebase | `git merge --no-edit BRANCH` · `git rebase BRANCH` · `git merge|rebase|cherry-pick|revert --continue|--skip|--abort` |
| Fetch, pull, push | `git fetch [--prune]` · `git pull [--rebase|--no-rebase|--ff-only] --no-edit` · `git push` · `git push --set-upstream REMOTE BRANCH` · `git push --force-with-lease` |
| Cherry-pick, revert, reset | `git cherry-pick --no-edit HASH` · `git revert --no-edit HASH` · `git reset --soft|--mixed|--hard HASH` |
| Remote | `git remote get-url REMOTE` · `git rev-parse --abbrev-ref @{upstream}` |
| Conflicts | `git diff --name-only --diff-filter=U -z` · `git grep -c '^<<<<<<< ' -- PATH` (markers left) · `git checkout --ours|--theirs -- PATH` |
| Ignore | append to `.gitignore` (phase 2, `fs:write:workspace`) |
| Init | `git init` |
| Ref check | `git check-ref-format --branch NAME` |

## Appendix C: the manifest

The manifest is `plugin.json` beside this document. Its settings are the table of section 6, each with its type, default, description, and choices or range, in the `settings` table Kalem's settings panel reads (as the Elixir plugin's are). Its permission is `subprocess:git` alone; its API is `^0.2.4`, the version that adds `process` (9.1), raised to the version of `documents` when phase 1 uses it.

The commands and keys are registered by `activate` (section 5), not listed in the manifest, as the `extension` world has them.

## Appendix D: the status document's lines

What each line of the status is, for the renderer and its tests. Regions nest (a hunk inside a file inside a section). A path may hold a colon, so a key puts its numbers before the path. A line of a hunk is no region of its own: its index in the hunk is its line's distance from the `@@` line, so a 50,000-line diff is not 50,000 regions. The leader keys act on the region under the cursor as section 5.2 says; this table gives Enter, Right and the menu.

| Line | Region key | Style | Enter | Right / Left | The menu (Alt+Enter) |
|---|---|---|---|---|---|
| `Head:      main   subject` | `head` | the branch `strong` | The branch menu | | Switch to…, New branch…, Delete…, Rename…, Set upstream…, Merge…, Rebase onto…, Log |
| `Upstream:  origin/main   ↑1 ↓2` | `upstream` | counts `muted` | The remote menu | | Push, Pull, Fetch, Push and set upstream, Force with lease… |
| `Tags:      v0.1.0 (3 commits ago)` | `tag` | `muted` | The tags (phase 3) | | |
| `Rebasing 3/7 onto main` | `operation` | `error` | Continue, Skip, Abort… | | the same |
| `▾ Unstaged changes (2)` | `section:unstaged` | `heading` | Toggles the fold | fold, unfold | Stage all, Unstage all, Discard all… |
| `▾ Stashes (1)` | `section:stashes` | `heading` | Toggles the fold | fold, unfold | Stash all, Stash staged only, Stash with untracked |
| `▾ modified     path   +12 −3` | `file:unstaged:PATH` | the kind `muted`, counts `muted` | Opens the file | unfolds, folds the diff | Open, Stage, Unstage, Discard…, Delete…, Blame, Log, Time machine, Diff as a document, Open on the remote, Copy link, Copy path, Add to .gitignore |
| `@@ -10,7 +10,8 @@ fn f()` | `hunk:unstaged:N:PATH` | the highlighter's | Opens the file at the hunk | unfolds, folds the hunk | Stage, Unstage, Discard…, More context, Less context |
| ` context`, `-removed`, `+added` | its hunk's; the line's index counted from the `@@` line | the highlighter's | Opens the file at the line | | the hunk's; with a selection, Stage lines, Unstage lines, Discard lines… |
| `  12,000 changed lines: Enter opens the diff as a document` | `big:unstaged:PATH` | `muted` | Opens the diff document | | |
| `  stash@{0}  message` | `stash:0` | | Shows the stash | | Show, Apply, Pop, Drop… |
| `  a1b2c3d  subject` | `commit:HASH` | the hash `code` | Shows the commit | unfolds the message and stat (log) | Show, Check out…, Cherry-pick, Revert…, Reset here…, Fixup, Copy hash, Open on the remote |
| `▸ spikes/notes.txt` (untracked) | `file:untracked:PATH` | | Opens the file | unfolds, folds its content | Open, Stage, Delete…, Add to .gitignore |
| `▸ both modified  path` (unmerged) | `file:unmerged:PATH` | `error` | Opens the file | | Open, Mark resolved (when no markers remain), Keep ours, Keep theirs, Abort… |

A hunk's lines keep `git diff`'s first column (` `, `-`, `+`, `\`) at the start of the line, so the `diff` highlighter colors them; a stash's and a commit's lines are indented by two spaces; tabs are kept; a carriage return at a line's end is not shown and other control characters are shown as `^X`, so that the document's lines are the diff's lines (the patch is made from the diff's bytes, never from the document's text).

## Appendix E: remote URLs

The upstream's remote URL (`git remote get-url`) in its forms: `https://HOST/OWNER/REPO[.git]`, `ssh://git@HOST[:PORT]/OWNER/REPO[.git]`, `git@HOST:OWNER/REPO[.git]`, with GitLab's subgroups (`OWNER/GROUP/REPO`) kept whole. The host decides the pattern; `remote_hosts` maps a self-hosted name to a kind.

| Kind | Hosts | File at a ref | Lines | Commit |
|---|---|---|---|---|
| GitHub | `github.com` | `https://HOST/OWNER/REPO/blob/REF/PATH` | `#L10` · `#L10-L20` | `/commit/HASH` |
| GitLab | `gitlab.com` | `/-/blob/REF/PATH` | `#L10` · `#L10-20` | `/-/commit/HASH` |
| Bitbucket | `bitbucket.org` | `/src/REF/PATH` | `#lines-10` · `#lines-10:20` | `/commits/HASH` |
| Gitea, Forgejo, Codeberg | `codeberg.org`, by setting | `/src/branch/REF/PATH` (`/src/commit/HASH/PATH` for a commit) | `#L10` · `#L10-L20` | `/commit/HASH` |
| sourcehut | `git.sr.ht` | `/tree/REF/item/PATH` | `#L10` | `/commit/HASH` |
| Azure DevOps | `dev.azure.com`, `visualstudio.com` | `?path=/PATH&version=GBREF` | `&line=10&lineEnd=20` | `/commit/HASH` |

`REF` is the branch (URL-encoded) or, with `remote_link = "commit"` or a detached head, the hash. A remote of no known kind: "no web address is known for HOST; add it to `remote_hosts`".
