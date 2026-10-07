# Git (`git`)

The repository's status as a document in Kalem: the changed files with their diffs under them, moved through with the arrows, staged, unstaged and reverted by file, hunk or selected lines with Doom Emacs's `SPC g` keys, committed without leaving the editor, and read with the editor's own highlighting, search, selection and motions, the same in the window and in the terminal. Lazygit's simplicity, magit's place inside the editor. Kalem's task T2.7i.11 and roadmap item R5.15; the design is [`DESIGN.md`](DESIGN.md).

**Status: phase 0.** Inside Kalem the plugin shows a Git panel (the branch, the last commit, Refresh, Commit, Pull and Push, the staged, unstaged and untracked files with a box to tick to stage) and a status bar item (`⎇ main ↑1 ↓2 •3`), and its `SPC g` keys stage, unstage, revert and delete the file being edited, commit with a one-line message, fetch, pull and push (asking where to push a new branch, and how to pull when the branches diverged), and show the commit of the line under the cursor. It needs Kalem's plugin API 0.2.4 (the `process` interface) and a repository inside one of Kalem's projects. The status as a document, with its diffs, comes with phase 1 and Kalem's `documents` interface (DESIGN.md, 9.2).

## Keys

Phase 0 binds `SPC g g` (the panel; Ctrl+Shift+G with Word-like keys), `SPC g S`, `SPC g U`, `SPC g R`, `SPC g D`, `SPC g c c`, `SPC g F`, `SPC g B`, `SPC g /` and `SPC g .`; Pull and Push are the panel's buttons and entries of `SPC g /`. The rest of the table comes with the git documents of phase 1.

In a git document the editor's keys move: Up and Down, Alt+Up and Alt+Down to the next file or section, Right and Left unfold and fold, Tab and Shift+Tab fold, Enter does the obvious thing with the line (opens the file at that line, shows the commit), Alt+Enter offers everything that can be done with it, Escape goes back. Every action is a Doom key, the same in a git document as in a file:

| Key | Does |
|---|---|
| `SPC g g` | The status |
| `SPC g S`, `SPC g U` | Stage, unstage the file |
| `SPC g s`, `SPC g u` | Stage, unstage the hunk, or the selected lines |
| `SPC g R`, `SPC g r` | Revert the file; discard the hunk or the lines (asks first) |
| `SPC g D` | Delete the file (asks first) |
| `SPC g c c` | Commit: the message as a document, saved to commit |
| `SPC g c a`, `SPC g c e`, `SPC g c w`, `SPC g c f` | Amend, amend without editing, reword, fixup |
| `SPC g F` | Fetch |
| `SPC g b`, `SPC g B` | Blame the file; this line's commit |
| `SPC g L`, `SPC g t` | The log; the time machine |
| `SPC g o o`, `SPC g y` | Open on the remote; copy the link |
| `SPC g /`, `SPC g .` | Every git command; this file's |

Push, pull, branches and stashes have no Doom key: they are Enter on the status's `Upstream:`, `Head:` and stash lines, and entries of `SPC g /`.

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

`cargo test -p kalem-plugin-git`: the readers of git's output, the patches, the documents and the actions on fixed inputs; then the library against a real git in repositories the tests make (`tests/repo.rs`, git isolated from the user's configuration); the plugin's state machine driven as Kalem drives the component, its questions answered from a script (`tests/app.rs`); the manifest's conformance. The component itself builds for `wasm32-unknown-unknown` against a Kalem with plugin API 0.2.4.
