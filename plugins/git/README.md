# Git (`git`)

The repository's status as a document in Kalem: the changed files with their diffs under them, moved through with the arrows, staged, unstaged and reverted by file, hunk or selected lines with Doom Emacs's `SPC g` keys, committed without leaving the editor, and read with the editor's own highlighting, search, selection and motions, the same in the window and in the terminal. Lazygit's simplicity, magit's place inside the editor. Kalem's task T2.7i.11 and roadmap item R5.15; the design is [`DESIGN.md`](DESIGN.md).

**Status: phase 1.** `SPC g g` (Word-like keys: the palette's "Git: Status") opens the status as a document of its own, in lazygit's order and magit's form: a line of keys at the top, the branch with its last commit and upstream, the untracked, unstaged and staged files, the local branches, the unpushed, unpulled or recent commits, and the stashes. Tab on a file shows its diff under it, hunk by hunk, highlighted as a diff; the changed lines of every file opened are marked beside the lines, in both editors. A status bar item (`⎇ main ↑1 ↓2 •3`) and the Git panel stay. It needs Kalem's plugin API 0.2.5 (`process`, `documents`, `decorations`) and a repository inside one of Kalem's projects.

A **Git menu** stands in the menu bar (and in the F10 list of both editors) while the current file is in a git repository, and in the status: Status (the same as `SPC g g`), commit, amend, push, pull, fetch, branches, stash, and the file's stage, unstage, revert, delete and blame. It needs Kalem 0.4.1.

## Keys

Nothing has to be known first: the status's first line names the keys, Alt+Enter (or `?`) lists what can be done with the line under the cursor and then every command, each with its key, and Escape closes the status.

| In the status | Does |
|---|---|
| Up, Down; Alt+Up, Alt+Down | Move; the previous and next section, file, hunk or commit |
| Tab; Shift+Tab | Fold or unfold the section, file or hunk; everything, step by step |
| Right, Left | Unfold (on an unfolded one: into it); fold (on a folded one: to what holds it) |
| Enter | The obvious thing: open the file (at the diff's line), switch to the branch, the stash's Apply, Pop or Drop, the `Head:` and `Upstream:` lines' menus |
| Alt+Enter, `?` | What can be done here, then every command |
| `s`, `u` | Stage, unstage the file, the hunk, or the lines selected (a section's heading: all of it) |
| `S`, `U` | Stage every change; unstage everything |
| `x` | Discard the file, the hunk or the lines (asks first) |
| `c c`, `c a`, `c e`, `c w` | Commit; amend (the last message's first line offered, its body kept); extend; reword |
| `P p`, `F p`, `f p` | Push, pull, fetch (`P u`, `F u`, `f u`, `f a` too) |
| `b b`, `b c` | Switch branch; a new branch |
| `Z z` | Stash the changes |
| Escape, `q` | Close the status |

The letters are magit's (as Doom's evil-collection has them: `x` discards) and apply in the status only, in Vim's command mode and with the Word-like keys. Doom's `SPC g` keys work in it and in a file:

| Key | Does |
|---|---|
| `SPC g g` | The status (in it: refresh) |
| `SPC g S`, `SPC g U` | Stage, unstage the file (in the status: the file under the cursor) |
| `SPC g R`, `SPC g D` | Revert the file; delete it (asks first) |
| `SPC g c c`, `SPC g c a`, `SPC g c e`, `SPC g c w` | Commit, amend, extend, reword |
| `SPC g F` | Fetch |
| `SPC g ]`, `SPC g [` | The next and previous change of the file (Alt+F5, Shift+Alt+F5) |
| `SPC g B` | This line's commit |
| `SPC g /`, `SPC g .` | Every git command; this file's |

The marks beside the lines compare with the index (what `s` would stage; the setting `gutter_base = "head"` compares with the last commit) and come again on each save; the setting `gutter = false` takes them away. The log, blame, the time machine and the remote's links come later (DESIGN.md, section 8).

## Try it

Until Kalem runs the plugin, the library comes with a command line:

```sh
cargo run -p kalem-plugin-git --example git -- status --open
cargo run -p kalem-plugin-git --example git -- stage hunk:unstaged:0:src/lib.rs
cargo run -p kalem-plugin-git --example git -- stage lines:unstaged:0:3,4:src/lib.rs
cargo run -p kalem-plugin-git --example git -- unstage file:staged:src/lib.rs
cargo run -p kalem-plugin-git --example git -- discard hunk:unstaged:1:src/lib.rs --yes
cargo run -p kalem-plugin-git --example git -- commit 'Fix the table parser'
cargo run -p kalem-plugin-git --example git -- log src/lib.rs --graph
cargo run -p kalem-plugin-git --example git -- blame src/lib.rs
cargo run -p kalem-plugin-git --example git -- url src/lib.rs 10-20
```

`-C DIR` first runs it in another repository; `--trace` prints the git commands that ran.

## How it works

The plugin runs the `git` program (2.23 or later) and reads its porcelain output; it holds no git of its own, so the user's configuration, hooks, credential helpers, signing, worktrees and submodules work as in a terminal. Its one permission is `subprocess:git`: it reads and writes no file itself. A hunk or some lines are staged, unstaged and discarded by patches the plugin makes from the diff the status shows and `git apply` applies, as magit and lazygit do; a test makes random edits and random selections in real repositories and checks that the index and the working tree hold what the selection says.

## Tests

`cargo test -p kalem-plugin-git`: the readers of git's output, the patches, the documents and the actions on fixed inputs; then the library against a real git in repositories the tests make (`tests/repo.rs`, git isolated from the user's configuration); the plugin's state machine driven as Kalem drives the component, its questions answered from a script (`tests/app.rs`); the manifest's conformance. The component itself builds for `wasm32-unknown-unknown` against a Kalem with plugin API 0.2.5.
