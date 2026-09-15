# obelus

A terminal code reader. **It doesn't want you to type.**

In the AI era every line you read is a line you did not write. Reading is the
product here; editing is incidental — and, so far, absent.

An *obelus* (÷, †) is the mark a scholar put in a manuscript's margin to flag a
line as doubtful. A reader's mark, made while reading.

## What works

```
ob src/main.rs      # open a file
ob                  # or nothing, and pick one from the welcome screen
```

- Full-screen viewer: line-number gutter, syntax highlighting, soft wrap
- Arrow keys and `PageUp`/`PageDown`, with the viewport following the cursor
- Status bar: the file on the left, `line:column` on the right
- **Automatic reload.** When something rewrites the file — an agent, a
  formatter, `git checkout` — the screen follows, without losing your place.
  A file that has gone away is marked `[stale]` and keeps its contents.
- Four pickers over one component: files, open buffers, themes, and every
  command by name. Files carry a Nerd Font glyph — which needs a patched
  terminal font, and is the first thing here that will want a config file.
- Two built-in themes, `dark` and `light`, switched without a reparse
- **How much code is here.** `count-lines` counts the tree with tokei and
  puts it in two pages: the languages, biggest first, with the prose that is
  written *inside* each one on a row of its own — 90 of this repository's
  Rust files carry 7,000 lines of Markdown — and the files, as a tree of
  directories that folds. `enter` opens a directory or reads a file, `alt+f`
  does the folding, and siblings are ordered by size whichever they are. The
  first row of the languages is the whole tree, and choosing a language
  leaves only its files.

Long lines wrap on word boundaries, using the Unicode line breaking algorithm
— so Chinese, which has no spaces, breaks between characters, and an English
word on the same line still does not get cut in half. A run with nowhere to
break falls back to the margin. Continuation rows keep their line's
indentation, and only the first row of a wrapped line is numbered. Wrapping is
not yet switchable.

Rust, TOML and JSON are highlighted. Adding a language is one row in
`src/syntax/mod.rs`.

## Keys

| Key | Does |
|---|---|
| `↑` `↓` `←` `→` | Move the cursor — up and down by one *visual* row |
| `PageUp` `PageDown` | Move a screenful |
| `ctrl+Home` `ctrl+End` | Start and end of the file |
| `Home` `End` | Start and end of the line |
| `ctrl+f` | Open a file |
| `ctrl+e` | Switch to an open file |
| `ctrl+p` | Run a command by name |
| `ctrl+r` | Re-read this file from disk |
| `ctrl+q` | Leave |

Inside a picker: type to narrow, `↑`/`↓` to choose, `PageUp`/`PageDown` to
cover ground, `Home`/`End` (or `ctrl+Home`/`ctrl+End`) for either end,
`Enter` to accept, `Esc` to cancel. Themes are
reached from the command palette.

Every *action* is a named command and the key table is data, so rebinding is a
matter of loading a different table — and everything that shows a key reads
that table, so the palette and the welcome screen follow a rebind rather than
lying about it. Navigation keys are deliberately not
commands: `cursor.up` is meaningless to invoke by name, and a command per
printable character is where that road ends.

## Where it is going

The point of a reader that knows both the syntax tree and the repository is to
join them:

- **Jump to a definition from inside a diff.** The semantic layer stays
  anchored to the working tree, so one language server and one warm index
  cover every git view — no per-commit checkout, no cold start.
- **A symbol's history**, not a file's: which commits actually changed *this
  function*, found by parsing each version's blob with tree-sitter, which needs
  no build environment at all.
- **A bridge to the agent**: export what you are looking at as context, and
  review a patch in the same semantic view you read the code in.

## Building

```
cargo build --release           # stable
cargo +nightly fmt              # nightly only: the rustfmt options require it
cargo clippy --all-targets
cargo test
UPDATE_FIXTURES=1 cargo test    # after an intended rendering change
```

Tests assert on the terminal cell grid, colours included: a widget can write
every character correctly and paint none of them, and an assertion on the text
cannot tell.
