# obelus

A terminal code **reader**. In the AI era every line you read is a line you did
not write, so browsing is the product and editing is incidental. The eventual
point is to join the LSP semantic graph to the git timeline — jump to a
definition from inside a diff, symbol-level history — which no terminal tool
does today.

Say *code reader*, never *editor*. Called an editor, it gets measured by an
editor's checklist (multiple cursors? macros? completion?), none of which it
wants.

## Commands

```
cargo build
cargo test
cargo +nightly fmt              # NOT `cargo fmt`
cargo clippy --all-features --all-targets
cargo run -- src/app.rs
UPDATE_FIXTURES=1 cargo test    # regenerate golden cell grids
cargo test -- --ignored         # the slow real-server test
```

`.rustfmt.toml` uses five nightly-only options. Stable `cargo fmt` silently
ignores them, and mixing the two makes the formatting oscillate. **Always
`cargo +nightly fmt`.** The build itself is stable.

Clippy must be silent. `src/lib.rs` denies missing documentation, so every
public item needs a doc comment.

## Rules that are easy to break by accident

**Every test must be checked by breaking the thing it covers.** Write the test,
then deliberately break that path and watch the test fail. Seven tests here
passed while the feature under them was broken; each was found this way and no
other way. If a break does not fail the test, the test is not testing what its
name says — fix the test, and say in a comment what the deliberate break was.
Beware of "breaks" that are equivalent rewrites, and of assertions that ask the
rule under test what it expects.

**All coordinate arithmetic lives in `text.rs`.** Five newtypes with private
fields (`ByteOffset`, `CharOffset`, `LineNumber`, `CharColumn`,
`DisplayColumn`, plus `Utf16Column`) exist so that byte, character, display
column and LSP column cannot be mixed up. Nowhere else adds or subtracts them.

**One thing on screen is drawn by one piece of code.** A screen stays
coherent because there is nowhere for two answers to the same question to
drift apart, not because everybody remembered the convention. So before
writing a view, look for the piece that already does it:

    ui::write_marked   a row's text: what matched marked, what the file
                       colours coloured, clipped to the list it is in
    ui::tabs           a tab row and the arrows that walk it
    ui::typed          the glyph and the words on a row that is typed into
    ui::nothing        what a list says when it has nothing in it
    ui::rule           a boundary between two things
    ui::scrollbar      how much of something longer than the screen is above
    component::window  which rows are on screen, and when that changes
    component::window::Move  the six keys that move about a list
    ui::editor         a document with a gutter: the file being read, and a
                       preview of somewhere else, which *is* this view

The costs of not doing this were all paid twice: "the window moves only when
the focus leaves it" was fixed once for the pickers and again for the agents'
cards; match highlighting had two writers and the newest list had neither;
the settings shipped with no paging and no ends, because those keys were a
list of arms rather than a table.

Reuse stops where the *subject* differs, and forcing those together is the
mistake in the other direction: the editor is not a list, a message being
written is not a buffer (no undo, no syntax, no file), and an agent's card is
not a picker row (it is as tall as its description needs and carries a
button). Share the mechanism, not the meaning.

**Commands are actions; navigation is not a command.** Arrow keys, paging, a
picker's selection keys belong to whichever component owns the state they move.
`:cursor.up` is meaningless to invoke by name. The key table is *data* on
`App`, so anything that displays a key reads that table and a rebind changes
every display of it.

**Modifiers are judged exactly, in one place.** `keymap::modifiers_of` is the
only judge; `SUPER`/`HYPER`/`META` disqualify a key rather than being masked
away. Masking meant `ctrl+super+q` quit.

**One key needed the terminal's permission.** A traditional terminal sends the
same byte for `enter` and `shift+enter`, so a program cannot tell them apart —
and `shift+enter` is how a paragraph is written in the box a message to an
agent goes in. `main` pushes the *narrowest* kitty-keyboard flag
(`DISAMBIGUATE_ESCAPE_CODES`) for it, pops it on the way out and from a panic
hook, and `alt+enter` breaks the line as well, because alt is the escape
prefix and always arrives. Nothing else in obelus depends on the protocol.

**`dispatch` has no wildcard arm** and warns on one, so a new `Command` fails
to compile until it is handled. Same idea in `theme`: only fields with readers.

**Nothing writes to stdout.** stdout is the drawing surface; `tracing` goes to
a file. A stray `println!` lands in the middle of a frame and stays there.

**No async runtime.** One `std::sync::mpsc` channel, one producer thread per
event source (keyboard, file walk, watcher, each server's stdout), the main
loop blocking on `recv()` and draining with `try_recv()`. Writing to a server's
stdin needs its own thread, because a busy server stops draining the pipe. An
answer that arrives after the world has moved on is the normal case, which is
why requests record the version they asked against.

**The configuration file holds preferences, not state.** `config.rs` is the
whole of it — one table, `dirs` for where it lives, written the moment
anything changes. Rebindable keys are still guaranteed by the key table being
*data* rather than by the file. What is not a preference does not go in it:
how to start an installed agent is written beside the install, not here.

## Shape

```
src/
  app/            state, the loop's handler, and every picker's item source,
                  by aspect: documents, moving, searching, choosing, agents
  text.rs         the Rope wrapper: the only place coordinates convert
  buffer/         one open file: text, syntax, cursor, viewport
  keymap.rs       chords, contexts, the default table, modifiers_of
  icons.rs        the Nerd Font switch and every glyph behind it
  config.rs       the settings, their file, and what each one is
  command/        the Command enum, its table, groups, and dispatch
  component/      picker (one component, several instantiations), settings,
                  the conversation, the box a message is written in, and the
                  window every list shares
  syntax/         language registry (14 languages), parsing, highlights, tags
  lsp/            transport, client, actions, positions, outline
  git/            gix: head text, statuses, hunks, blame
  agent/          the ACP registry, installing an agent, its marks
  acp/            the protocol, through its own crate, and the thread that
                  joins it to the loop
  ui/             editor, status bar, picker, settings, chat, welcome,
                  images, shared cell writers
tests/            integration tests plus tests/fixtures/*.txt golden grids
```

Two rules the views share and neither enforces: **leave a blank column after
a Nerd Font glyph** (the non-`Mono` variants draw two cells wide while the
terminal allocates one), and **whether the font has a glyph cannot be
detected** — a terminal's column advance comes from Unicode width tables, not
from the font, so there is one switch (`icons::NERD_FONT`) and a fallback
behind every glyph.

A picture is a third thing again. A terminal that speaks kitty's graphics
protocol, iTerm2's inline images or sixels can be handed pixels, and
`ui::image` does that for the agent registry's SVG marks -- but **whether it
can is asked once, before the alternate screen** (`Images::detect`, from
`main`), because asking means writing an escape sequence and reading the
answer. Everything else gets the glyph: half-blocks are for photographs and
obelus has none. So a test, a pipe and most terminals draw the glyph path,
which is why the fixtures never contain pixels.

**The agent protocol comes from its own crate.** `agent-client-protocol` is
the reference implementation: every method has a type whose field names the
compiler checks, which is the point -- obelus had the nine methods it needs
written out by hand and checked once against the schema, and a protocol that
renames an outcome would have gone on compiling and quietly stopped matching.
It is executor-agnostic (its tokio is a dev-dependency), so `acp::link` runs
the connection on one thread with `futures::executor::block_on` and joins it
to the loop: what obelus wants becomes an `Ask` sent to that thread, and
everything the agent says becomes an `Event`.

The two directions are not symmetrical, which is the part worth knowing. What
obelus asks is fire-and-forget -- the answer arrives as an event, because by
then the reader may be looking at something else. What the *agent* asks --
permission, the text of a file -- obelus cannot answer without the reader, so
the handler sends the question to the loop with a `oneshot` to answer through
and waits. Waiting is right there: the agent has stopped, and what it is
waiting for is a keystroke. This is why `Event` is not `Clone`.

One thing the crate does not promise: that a notification sent after a
request leaves after it. A cancellation typed in the same instant as a prompt
can reach the agent first, so an interruption tells the agent *and* ends the
turn on obelus's side, and a late answer to a turn the reader stopped is
dropped.

The agent is a real process in the tests. `tests/fixtures/fake-agent.sh`
speaks the protocol -- handshake, session, streamed answer, a file read back
through obelus, a permission request -- and `tests/agent.rs` drives obelus at
it by keys and reads the screen. It is `sh` on purpose: a fake agent written
in python, node, or a second Rust binary is a test that stops running on
somebody else's machine.

That fake agent is also what holds obelus to its promises, because the crate
cannot: it checks the handshake it was given and answers to the name "Wrong
Client" if the client offered to write files, and it asks for a write during
the turn and reports back whether it was refused. Both assertions are in
`a_whole_turn_of_conversation`.

**The agents page's buttons touch the real data directory.** `install` runs
`npm` and `activate` writes a start record under `dirs::data_dir()`, neither
of which has a test hook -- so a test must not press enter on an agent card.
Everything about talking to one goes through `App::talk_to`, which takes the
command directly and needs no registry, no install and no network.

Golden fixtures dump every cell's symbol, foreground and background, plus the
cursor position. Colours are in them because highlighting, themes and the
status bar's background are otherwise not asserted at all: a list of file names
can render perfectly and show nothing.

A preview's margin comes from git, so a fixture that shows one depends on
the fixture file being *committed*: edit `tests/fixtures/long.rs` without
committing and the preview grows change marks. Which is the feature working,
and a surprising way to see it.

**Run them in the real checkout, not in a `git worktree`.** The welcome screen
prints the working directory (`welcome_64x20`, `welcome_narrow_34x10` both
carry `~/Work/obelus`), so every fixture that shows it fails in a worktree for
a reason that has nothing to do with the change under test — and
`UPDATE_FIXTURES=1` there writes the worktree's path into the fixture, which
then fails everywhere else. `cargo check` and `cargo clippy` in a worktree are
fine; `cargo test` belongs in the checkout.

The git tests build real repositories in a temp directory, with one
deliberate exception: `the_committed_text_comes_from_git` reads *this*
repository's `src/lib.rs` through `git show`, because a diff of what git
actually has against what is on disk is the only thing that says the two
halves agree. It asserts nothing about whether that file is currently dirty.

## Comments

Comments say *why*, and are worth writing where the code is right for a reason
that is not visible — an ordering that matters, a rule that fails silently, a
plausible alternative that is wrong. Don't narrate what the line does. The
existing code is the style guide; match its density.

## Not now

Workspace symbols and hover (the file outline is done), M1c (diagnostics, a
gutter that holds more than line numbers), searching a file (`ctrl+f` is left
unbound for it), the rest of M2's git (history, blame, tree diffs, staging --
the working tree's own diff is done: `src/git/`, the margin, the map beside
the scrollbar, `git.hunk` and the steps between hunks), the diff/semantic
bridge (M3), symbol-level history (M4), the agent bridge (M5), and a minimal
editing set, last. Don't start on these without being asked.

`git show` and `git status` are shelled out, behind `git::head_text` and
`git::statuses`. A library (`gix`) is worth its weight when the views arrive
and can take over behind those two without anything above noticing; four
hundred crates for one blob read is not.

Also waiting on a configuration file, which does not exist: the Nerd Font
switch, user theme colours, the server table, and the word-wrap toggle all
want one.
