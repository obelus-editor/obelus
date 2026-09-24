# obelus

A terminal code reader. **It doesn't want you to type.**

[sunli829.github.io/obelus](https://sunli829.github.io/obelus/) — why it is
shaped this way, and how to use it. Also in
[中文](https://sunli829.github.io/obelus/zh/).

In the AI era every line you read is a line you did not write. Reading is the
product here; editing is incidental.

An *obelus* (÷, †) is the mark a scholar put in a manuscript's margin to flag a
line as doubtful. A reader's mark, made while reading.

## Getting it

```
cargo install --path crates/obelus-cli    # installs as `ob`
```

```
ob                   # the welcome screen, on this directory
ob src/main.rs       # read a file — its repository is the project
ob ../another-repo   # a directory: that is the project
```

A Nerd Font and a language server are both optional. The glyphs are off until
`icons` is turned on, so a terminal without a patched font draws no boxes;
without a server, reading still works and there is no definition, no
diagnostics and no outline.

## What works

**Reading a file.** Syntax highlighting for fifteen languages, a gutter,
folding, and soft wrap that breaks on word boundaries using the Unicode line
breaking algorithm — so Chinese, which has no spaces, breaks between
characters, and an English word on the same line is not cut in half.
Continuation rows keep their line's indentation and only the first is numbered.

A file has a *reading* as well as a language: markdown laid out as prose
(`ctrl+t`), a log put in columns — decided by what its lines look like rather
than by its name. And a file can hold more than one language at once: the code
in a markdown fence and the script in an HTML page are parsed as what they are.

**Automatic reload.** When something rewrites the file — an agent, a formatter,
`git checkout` — the screen follows, without losing your place. A file that has
gone away is marked `[stale]` and keeps its contents. Saving asks disk whether
somebody else got there first.

**git, on the page you are reading.** The margin marks what changed, a map of
the whole file sits beside the scrollbar, and `alt+d` opens a hunk in place, so
a diff is not somewhere else to go. Who last changed the line is in the margin.
The file's history, the project's, and the commit that wrote one line each have
a key; what the remote has not seen is marked. Read-only, and no `git` is run.

**A language server.** Definition, type definition, implementations,
references, calls, hover, rename, code actions, diagnostics, the outline, and
the types it infers drawn where they would be written. rust-analyzer, gopls,
clangd, pylsp, typescript-language-server, bash-language-server and the CSS,
HTML and YAML servers are known by name.

**One question at three radii.** This file, every file, or the names the
language server knows — one view, so changing the radius is not retyping the
query. The file list, the open documents, the themes and the command palette
are the same list with different contents.

**What you mean to come back to.** `alt+t` while reading writes a note against
the line under the cursor; `todo` opens them all. A note may be about a line or
about the project, and both are ordinary. The page is always being written: a
letter is a letter, `enter` starts another note, and what acts on a note *as* a
note is under `alt`. The line a note was put beside is found again through git,
so a note written last week still points at what it was written about.

**An agent, over ACP.** obelus implements nobody's model: it installs one from
the protocol's own registry and talks to it. A conversation is one of the open
documents, and a conversation opened from a note stays about that note — come
back tomorrow and it is where you left it. A change the agent is *asking* to
make is drawn in the transcript, the way an opened hunk is drawn in a file, and
stays there afterwards. Over MCP it can also read the notes, add some, tick one
off or reword one — having asked you first.

**Several obelus at once.** There are no splits: two files side by side is
another window. A repository and its worktrees are one project, so they share
one set of notes; two windows writing notes do not erase each other, and a
conversation can only be open in one of them at a time.

## Keys

Three families, and the family is the memorable part. **A function key opens
something to look at**, bare and never with a modifier, because terminals
disagree about what a modified function key sends.

| Key | Does |
|---|---|
| `F1` | Open a file — and, in a view that has one, the list of its keys |
| `F2` `F3` | Switch to an open document · one that changed since the last commit |
| `F4` | Talk to the agent |
| `F5` `F6` | Search this file · every file |
| `F7` `F8` | This file's symbols · the names the server knows |
| `F9` `F10` `F11` | The file's history · the project's · the commit behind this line |
| `F12` | Go to the definition |

**Control does something to the file in front of you**, on the letter of the
word.

| Key | Does |
|---|---|
| `ctrl+p` | Run a command by name |
| `ctrl+w` `ctrl+s` `ctrl+r` | Close · save · re-read from disk |
| `ctrl+t` | Show this file as rendered markdown, or stop |
| `ctrl+l` | Go to a line |
| `ctrl+c` `ctrl+x` `ctrl+v` | Copy · cut · paste, through the system clipboard |
| `ctrl+z` `ctrl+y` `ctrl+a` | Undo · redo · select all |
| `ctrl+/` | Comment out, or take the comment off |
| `ctrl+q` | Leave |

**Alt asks about the cursor, or walks what was found.**

| Key | Does |
|---|---|
| `alt+enter` | What the language server can say about this symbol |
| `alt+h` `alt+a` `alt+r` | Describe it · what can be done here · rename it |
| `alt+e` `alt+[` `alt+]` | Everything wrong with this file · the one above · below |
| `alt+d` `alt+p` `alt+n` | Open the change here · the one above · below |
| `alt+f` `alt+m` `alt+w` | Fold · the matching bracket · widen the selection |
| `alt+↑` `alt+↓` | Move the line, or the selected lines |
| `alt+←` `alt+→` | Back to where the jump was made from, and forward again |
| `alt+t` | Write a note about this line |

Navigation is not a command and is not in those tables: the arrows,
`PageUp`/`PageDown`, `Home`/`End` for the line's ends and `ctrl+Home`/`ctrl+End`
for the file's. Up and down move by one *visual* row. Shift never names a
command; it only ever extends.

Inside any list — a picker, the settings, the notes — type to narrow, `↑`/`↓`
to choose, `enter` to act on the row you are on, `Esc` to cancel, and `F1` for
every key it takes.

Every *action* is a named command and the key table is data, so rebinding is a
matter of loading a different table — and everything that shows a key reads
that table, so the palette and the welcome screen follow a rebind rather than
lying about it.

## Settings

`open-settings` is the page; everything on it is written straight back to
`~/.config/obelus/config.toml`. A project may carry an `.obelus/config.toml`
of its own, laid over yours — except for which agent starts, what it may do
without asking, and where the keys are, which are yours alone.

```toml
theme = "light"
wrap = true

[keys]
open-file = "ctrl+o"
```

Two themes are built in. Beyond them, every `.toml` in
`~/.config/obelus/themes/` is a theme named after its file, and every colour in
one is optional. `contrib/omarchy/` has a template that writes one from the
desktop's palette.

## Where it is going

The point of a reader that knows both the syntax tree and the repository is to
join them:

- **Jump to a definition from inside a diff.** The semantic layer stays
  anchored to the working tree, so one language server and one warm index
  cover every git view — no per-commit checkout, no cold start.
- **A symbol's history**, not a file's: which commits actually changed *this
  function*, found by parsing each version's blob with tree-sitter, which needs
  no build environment at all.
- **More of the bridge to the agent**: export what you are looking at as
  context, and review a patch in the same semantic view you read the code in.

## Building

```
cargo build                     # stable
cargo +nightly fmt              # nightly only: the rustfmt options require it
cargo clippy --all-features --all-targets
cargo test
UPDATE_FIXTURES=1 cargo test    # after an intended rendering change
cargo test -- --ignored         # the slow real-server tests, and the diff sweep
```

Tests assert on the terminal cell grid, colours included: a widget can write
every character correctly and paint none of them, and an assertion on the text
cannot tell.
