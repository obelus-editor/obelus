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

**A command's name is what it does, verb first, hyphenated.** `open-file`,
`go-to-definition`, `select-all`. Not a family and a member (`file.open`,
`selection.all`), which read backwards: a reader reaching for the palette
knows the verb and types it first, and a list filtered on such a name buries
that verb behind a noun they have to guess. The family is
`Command::group()`, which is a tab on the palette and part of no name -- so
nothing may be read off a name either, which is why `icons::for_command`
takes the `Command` and matches it exhaustively instead of splitting a
prefix off a string and guessing. The names are also what the config file's
`[keys]` table is written in, so renaming one leaves an old file's line
unbound with a word in the log.

**A file that is open is previewed where it is being read.** Whichever list
names it -- the open files, or the whole tree -- because it is one question
with one answer: a file's own place in it is the thing a reader remembers it
by, and choosing the row takes them back to exactly that, so the list reads
as something folded over the file rather than as a way somewhere new. A file
nothing has opened has no such place and starts at the top. `App::read_at`
is the one answer; a list that had its own would be a list where choosing a
row moved the screen under the reader.

**A list obelus offers is a list of the reader's own tree.** A language
server answers `workspace/symbol` with everything it has indexed, which for
rust-analyzer is every dependency of the project: a search for `new` in a
repository of a dozen files comes back with hundreds of rows from the
registry, and the one the reader meant is somewhere among them. So
`outline::found_in` takes the root and drops everything outside it -- an
argument rather than a filter at the call site, because a rule a caller can
forget is a rule that comes back. The file list has always worked this way,
and it is why a path can be shown relative to the root at all. Going *to* a
definition in a dependency is a different thing and still goes there: that
is a jump the reader asked for by name, not a list to choose from.

**The paging keys belong to whatever is being read, not to the list.** A
list with a preview under it is two things on screen, and only one of them
is read a screenful at a time: the list is ten rows walked one at a time
with `ctrl+home` and `ctrl+end` a keypress from either end, while the
preview is a file. So `App::page_preview` is asked before the list is, and
answers no when there is nothing to page -- a compact list, or a terminal
too short for a preview -- where the bare keys page the list as before. The
same keys with `ctrl` always page the list, which is the other half of the
swap and the only way through a long one.

**Two modes, and two is enough: `Edit` and `Preview`.** The bytes, or a
reading of them -- and *which* reading is the file's own business, not the
mode's: markdown is laid out as prose, a log is put in columns, and a third
mode would be `Mode` answering a question the format already answers. A file
that has a reading opens in it (the `preview` setting, on by default) and
`f10` shows the bytes instead, which is where the cursor, the selection and
the copy live. The default is applied when a buffer is *made*, never in
`apply_config`: a default that reapplied itself would put a preview back
over a reader who had turned it off, which is the `blame` mistake again.

**A log's format is decided by its lines, not its name.** `syslog`,
`access.log`, `obelus.2026-09-11.log` -- the extension says nothing, so
`log::format_of` offers the first twenty lines to each format and takes the
one that claims a majority. Only lines starting at the left edge count: an
indented line is how every log writes what ran on -- a panic's second line,
a stack trace -- and counting those against a format is how a real log fails
to be recognised as one. A line no format claims is kept as it was written,
which is what makes the reading safe to try on anything: the worst it can do
is show the file.

Three of the four formats are written out in `log.rs` -- obelus's own
`tracing` layout and syslog's two -- because each is a fixed run of fields
with fixed separators, which is less code than reading somebody else's
parser. The fourth is `access_log_parser`, which earns the dependency: an
access log's fields are typed and its status code is the level. `rsyslog`
was measured and left out -- it parses RFC 5424 and fails on the RFC 3164
line that is actually in `/var/log/syslog`, which is the file a reader
opens.

**A key does nothing where its command is dim.** `App::offers` is the one
judgement of whether a command can do its job here: the palette draws a row
it refuses as dim and will not run it, and `App::handle_key` asks the same
question before dispatching, so a command cannot be off in one place and
live in the other. It is silent about it -- `f2` with no file open used to
draw an empty list of open files, `f3` on a clean tree wrote "nothing has
changed" across the status row, and `ctrl+c` with no selection said "nothing
selected"; three answers to a question the palette had already said could
not be asked. So `Requires` is where that work goes, and a note inside a
command for "you cannot do that here" is dead code unless the condition
cannot answer exactly (the bracket scan is the one that cannot: `ABracket`
is the character under the cursor, and a bracket inside a string is offered
and finds no partner).

Which makes a condition worth a walk: `AChangedFile` asks git what has
changed in the tree, once when the palette opens and once per press of the
key. This repository answers in two milliseconds.

**The text's rows and the screen's rows are two counts.** A cursor moves
through the text; a viewport is a window on the screen, and the screen can
hold rows the file does not have -- an opened hunk draws the lines it
replaced above the line that replaced them. So `Text::row_count` is the
text's count and `Buffer::screen_rows_of` is the screen's, and every path
that moves the *viewport* uses the second (`step_screen_rows`,
`cursor_screen_row`), while the cursor's own stepping keeps the first.

One function answering both is how a deletion taller than the screen became
unreadable: the block was drawn only from its first row, the viewport could
not express being inside it, and two patches -- a row count re-derived in
`ui::editor` for the caret, and a height shrunk in `App::prepare` for the
scrolling -- kept the caret honest without making the rows reachable. Both
are gone.

**The caret can be in the block; the cursor never is.** `Buffer::block` is
the opened hunk's lines *as a `Text`*, and `in_block` is a `Cursor` in it.
A text, so those lines get everything the file's get from the same code: they
wrap at the same width, their tabs reach the same stops, a wide glyph takes
two cells, the caret moves by visual rows, a selection in them is a `Span`,
copying is `text_in`, and the rows are drawn by the writer every other row
goes through. The alternative was a second, smaller set of all of that --
which is a second set of bugs, and was one: a line wider than the screen was
cut with the caret walking off the edge of it.

The *cursor* stays on the line the block is anchored to, so everything that
asks the file about "here" -- a language server, a jump, the next change, the
margin -- goes on being answered from a line the file has. The status row
says `-4:7` while the caret is in there, because that place has no line
number in this file and a number without the minus would name one it is
nowhere near. The anchor of a selection belongs to whichever of the two the
caret is in, and `clear_selection` reaches both. A page that lands on one of
those rows puts the caret there, which is how the paging keys walk a block of
any size; anything that puts the cursor somewhere outright (`place_cursor`)
brings it back, as does closing the hunk -- which the key that opened it does
from wherever the reader has walked to, because "the hunk at the cursor" is
not the hunk in front of them once they have walked into it.

**A command does something; a preference is a setting.** A switch that
should outlive the session is a setting and nothing else -- the only key to
it is the one that opens the settings. A command may *change* a setting, as
`choose-theme` does by writing it, but no command may own a bit a setting
owns as well: showing who wrote each line was both `config.blame` and a
`git.blame` command flipping a field of `App`, so turning the names off with
the key lasted until the next time anything on the settings page changed --
`apply_config` put the field back from the setting, without a word. The
command is gone and `App::blame` reads the setting.

**The key table has three families, and the family is the memorable part.**

* **A function key opens something to look at.** Two banks of four, which is
  how they sit on the keyboard: `f1`-`f4` are the things to read (a file, an
  open file, a changed file, the agent) and `f5`-`f8` are finding, which is
  one question at four radii -- this file or every file, its text or its
  names. Bare, never with a modifier: one terminal reports `shift+f5` and
  the next reports `f17` for the same press, so a modified function key is a
  binding that works on one machine and not the next. `f9`-`f12` are empty
  on purpose, for the views that will earn them -- a diff, a commit log, a
  panel of references, a patch to review.
* **Control does something to the file in front of you**, on the letter of
  the word: `p` the palette, `w` close, `r` re-read, `l` a line number, `a`
  all of it, `c` copy, `q` leave.
* **Alt asks about the cursor, or walks what was found**: `alt+enter` the
  symbol under it (an IDE's context actions, and alt is the escape prefix so
  it arrives everywhere), `alt+d` its diff, `alt+b` its blame, `alt+m` its
  matching bracket, and the arrows -- up and down between changes, left and
  right through the places the reader has been.
* **Shift never names a command.** It only extends (`shift` plus an arrow)
  or reverses (`shift+tab`), which leaves it meaning one thing everywhere.
  **Escape always gives up on the nearest thing**, and everything else is
  reached from the palette: a chord for every command is how a key table
  stops being memorable.

**`keymap::why_not` is the one judgement of what may be bound**, and the
three families are the whole of it. It is asked by the page that binds keys,
by the table read out of the config file, and by the test that holds the
shipped table to the same rule -- so obelus cannot give itself a key it
refuses the reader, and a reason is written once. What it refuses, and why
each of them would be a binding that silently never fires:

* the arrows, `home`, `end` and the paging keys, bare or with `ctrl` or
  `shift` -- the editor takes those before the table is reached, and the
  ones it does not take it has said it wants (`ctrl` and an arrow is a word
  motion, `ctrl` and a paging key is the previous and next buffer). `alt`
  and an arrow is the exception, which is how changes and history are
  walked;
* `ctrl` plus `i`, `m`, `j`, `h`, `[`, space or `2`, which *are* tab, enter,
  newline, backspace, escape and NUL on the wire, whatever the reader
  pressed;
* a bare character, `enter`, `tab`, `backspace`, `delete` -- typing, and the
  keys every list and box takes itself;
* `escape`, which obelus keeps: give up on the nearest thing is not
  negotiable, and it is the one default a reader cannot move;
* anything with two modifiers, and `ctrl` with a capital letter -- a control
  byte cannot say which case the letter was, so `ctrl+shift+p` works only on
  a terminal speaking the keyboard protocol. `alt+P` is fine, because alt is
  the escape prefix and really does carry the shifted letter;
* a function key with anything held, for the same reason.

`ctrl+b` belongs to tmux, so obelus does not ship it -- a reader outside
tmux may still have it. `ctrl+a` is screen's prefix and is shipped anyway,
because "all of it" is what that key means in every program with a
selection.

**Keys are rebound on the keys page, and the file holds the changes.** The
table is data on `App`, so a rebinding is `Keymap::rebind` plus a line in
the config's `[keys]` -- command *name* to chord spelled out (`ctrl+p`),
because an enum's spelling and a keycode are obelus's business rather than
the reader's. What is in the file is a list of changes over the defaults, so
a reader who moved one key still gets the new default for everything else,
and a name or a chord obelus cannot read is skipped with a word in the log.
Rebinding moves *every* binding of the command -- `close-file` is bound in
`Normal` and in `Buffers` and is still one command with one key -- and a
command that had none gets one in `Normal`, which is where a key a reader
presses belongs. A chord already spoken for is refused on the row that asked
for it, with what has it: the row is where the reader is looking, the status
row there is the page's own filter, and a passing note would be cleared by
the very next keystroke.

**What is showing owns the keys.** A list, the settings page and a
conversation are dialogs: each takes the keys bound *in* its context and
nothing else, so `Keymap::lookup` reaches the everywhere bindings only from
`Context::Normal`. Before that a global key worked inside them, which is how
`ctrl+o` in a conversation put a file list on top of it -- two things on
screen, two escapes to leave, and nothing saying which one a key would
reach. The one exception is `Context::Buffers`: the list of open files binds
the key that closes a file, because the thing to close is the row. So a new
dialog gets a context, and a key it should keep gets a binding in it -- not a
fall-through.

**One mark for "the keys are here", and it says nothing else.** Every list,
page and card in obelus puts `selected_row_background` behind the row the
reader is on -- a picker's rows, the settings', an agent's question, the
transcript -- and the same colour behind the one item of a row of them, for
the things laid out across a row rather than down a column. Where there is a
caret there is no background: a box is marked by the caret sitting in it,
and two marks for one fact is one too many.

Whether a row can be *used* is said in the ink, never by taking the
background away. A card's `submit` row did the latter while it was short of
what the agent asked for, so the reader stood on a row that had stopped
saying it was under them: they pressed enter, got nothing, and had nothing
on screen to tell them which row had refused.

**A rule meets the bar it crosses, and meets it with the right corner.**
Drawn through, it left the bar in two pieces, and a control in two pieces
reads as one that is broken. The glyph comes from all four directions rather
than from the two a bar can be on, because the arms that are *not* there are
what makes a corner: the bar is in the last column, so a ┬ there hangs
half a stroke over the edge of the screen with nothing to join to. A screen
with a list, a preview and a status bar on it reads as one frame -- ┐ down
into the list's bar, ┤ where it runs through, ┘ closing the bottom.
`rule` and `scrollbar` join from both sides -- the rule looks at the cells above and
below each of its own, the bar at the cell beyond each of its ends -- because
which of the two is drawn first depends on the view, and neither of them
should have to know. A list over a file draws its edge after the file's bar;
a list's own bar is drawn after the rule under its tabs. Both come out
closed.

The cost is knowing what is drawn in a cell, which means reading the grid
back. The only thing that can go wrong is a file whose own text has a bar
glyph directly above or below a rule, which would take one cell of it for a
junction -- a cosmetic slip in a file drawing box characters, against
threading a list of every rule through every view.

**Everything that scrolls says so, in the last column of the region it is
in.** A file, a preview, a list, a page of settings, a conversation -- the
last of those had no bar at all, which left a reader paging through it with
nothing on screen answering "how much of this is there, and which part am I
looking at". The editor's used to
sit one short of it, because the map of where a file has changed had the
edge: a list opened over a file made the bar jump sideways, and inside one
screen a list with a preview under it had its bar in two columns with a rule
between them.

The map is *inside* the bar now rather than outside it. They are the same
picture at the same scale -- the whole file squeezed into the height of the
screen -- so they belong side by side, and the reader reads across them:
here is where you are, and here is what has changed.

**The wheel moves the view; the keys move the cursor.** A notch scrolls what
is on screen and leaves the cursor where it was -- `scroll_by` on a buffer
moves the viewport and nothing else -- and the paging keys move the cursor by
a screenful, with the view following it. Two gestures, two jobs: a reader
spinning a wheel is looking around, and one pressing a key is going
somewhere. A list is the exception that proves it: there a notch steps the
selection, because a list's view *is* its selection and there is nothing
else in it to scroll.

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

**Wherever enter means something else, a line is `shift+enter` *and*
`alt+enter`.** Both, every time, and it is one rule rather than a decision
per box: `shift+enter` is what a reader reaches for and it arrives only from
a terminal that speaks the protocol above; `alt+enter` is what arrives from
the rest. A place that took one of them left the other falling through to
whatever was underneath — in the agent's card, into the box it covers, where
the line went into a message nobody could see and was sent afterwards. So
the pair is taken together, and taken *before* the modifier check, since alt
disqualifies a key everywhere else. Where the reader has nowhere to type at
all, the pair is swallowed rather than passed on, for the same reason a
plain character is.

**`dispatch` has no wildcard arm** and warns on one, so a new `Command` fails
to compile until it is handled. Same idea in `theme`: only fields with readers.

**Nothing writes to stdout.** stdout is the drawing surface; `tracing` goes to
a file. A stray `println!` lands in the middle of a frame and stays there.

**Two logs, split by module.** `obelus.log` is what obelus says about itself
and `lsp.log` is what the language servers say -- a handshake, every request,
and whatever they write to their stderr, at a volume that would bury the
dozen lines obelus has of its own. `logging::is_server` decides by the
event's target, which `tracing` takes from the module it came from, so a
call site needs to know nothing and a module moved into `lsp` takes its
lines with it. `open-log` and `open-server-log` open them; both are ordinary
buffers, because obelus is a reader.

The default filter names **`ob` as well as `obelus`**: the binary is its own
crate, so everything `main` logged -- what started, and that it left -- was
filtered out of its own log until it was added.

**A panic goes in the log** (`logging::catch_panics`, chained like every
other hook). It is the one thing a log has to have and the one thing it had
none of: the message goes to stderr, which is behind the alternate screen,
so the log simply stopped mid-session with no reason in it. It earned its
keep immediately -- two real crashes on absurd terminal sizes, both fixed in
the same slice as the line that found them.

**No async runtime.** One `std::sync::mpsc` channel, one producer thread per
event source (keyboard, file walk, watcher, each server's stdout), the main
loop blocking on `recv()` and draining with `try_recv()`. Writing to a server's
stdin needs its own thread, because a busy server stops draining the pipe. An
answer that arrives after the world has moved on is the normal case, which is
why requests record the version they asked against.

**obelus does not split its window, so several obelus processes is the
normal case.** A terminal already splits, tiles and tabs better than an
editor can from the inside, so obelus has one region and no panes. What that
buys has to be paid for on the other side: two or three of them on one
project, plus the reader's own shell in the same repository, is how obelus is
actually used, and nothing it writes outside a buffer belongs to it alone.

Three rules come out of that, and every one of them was broken:

*Anything read once at startup must be re-read when somebody else changes
it.* The settings were read at startup and never again, so a theme changed in
one window was a theme changed in one window. The watcher -- already there
for open files -- now watches the settings file too, and `App::reread_config`
applies what it finds. `apply_config` rather than `configure`: the second
half of `configure` puts every open file back to the reading the settings
ask for, and a reader who turned a preview off should not have it come back
because somebody in another window changed the theme. The agent is the one
setting that does *not* reach in: a conversation is this window's, and
restarting it under the reader because another window chose differently is
somebody else's decision arriving as an interruption.

*Anything written must survive another process writing it at the same
moment.* `save_to` wrote in place, which truncates first; a second obelus
reading in that gap got an empty file, took it for "no settings", and wrote
its defaults over everything the reader had. It writes beside the file and
renames over it now -- the one filesystem operation with no gap in it -- and
reading distinguishes "there is no file" from "there is a file obelus cannot
read". The second stops obelus writing at all: what is in that file is the
reader's, and saving over something it could not read replaces settings it
never saw. It says so on the status row and starts saving again the moment
the file reads, which the watcher notices.

*Anything cached about the world must be dropped when the world moves.* What
has changed in a file is a question about the file *and* about the commit it
is compared with, and the cache was keyed only by the file: a commit in
another window left the margin drawing a diff against a commit that was no
longer the one the file is against. `HEAD` and `index` are watched, and
`App::forget_what_git_said` drops the hunks and the blame when either moves.

Two smaller ones, in the same spirit. An install claims the agent's directory
with a file created exclusively, so two windows asked for the same agent do
not run two `npm`s into one prefix; the claim is given up by being dropped,
and one left behind by a killed process is taken over after ten minutes. And
every log line carries the process's number, because several obelus
processes share one log and two interleaved stories with nothing to tell them
apart are neither of them readable.

What is *not* shared is worth saying too: a language server and an agent per
process, which is the cost of not having panes. Three windows on one Rust
project is three rust-analyzers.

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
  counts.rs       how much code is here: tokei's walk, in the two orderings
                  the view reads it in
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
It is executor-agnostic, so `acp::link` runs the connection on one thread
with a current-thread tokio runtime and joins it to the loop: what obelus
wants becomes an `Ask` sent to that thread, and everything the agent says
becomes an `Event`. One agent, one connection, so a work-stealing pool would
be threads nobody asked for. The channels stay `futures`' -- that is what the
protocol's crate speaks, and a channel is runtime-agnostic; tokio is there to
drive them.

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
`a_whole_turn_of_conversation`. It does the same for the settings: the
boolean one is only offered to a client that said in the handshake that it
can show a switch.

**A form is asked on a card, and the order is the agent's needs.**
`elicitation/create`'s schema arrives as a *map* of fields -- JSON objects
have no order to keep -- so the order the agent wrote them in is gone before
obelus sees it, and asking in the alphabet's order put an "Other, if none of
these suit" in front of the list it was an alternative to. What is left to
go on is `required`: those first, in the order the agent listed them, and
the rest after.

A named-answer field and a words field next to it go on the *one* card,
because that pair is one question -- "these, or say what you want instead" --
however many fields it takes to write down. Everything else is a card of its
own, in turn.

What the agent does not need, a reader must be able to say nothing to:
they send the card with the box empty, and the field is left out of the
answer. Escape is not that answer -- escape gives up on the whole form,
which is the one thing a reader walking past an aside does not mean.

**An agent that wants to ask something uses `elicitation/create`.** That is
the one way it can put UI on a client's screen, and it is gated on a
capability: no `elicitation.form` in the handshake and an agent either falls
back or gives up. What it may ask for is a flat form of primitives: one of a
list, several of a list, a switch, words, a number. The whole form goes back
as one answer, keyed by the agent's own names; escape declines it, and the
view going away cancels it, because an agent that hears nothing waits for
ever. `elicitation.url` is *not* declared: obelus is not a browser, and a
mode it cannot put is a mode it should not be sent. A property type it has
never heard of is declined with the reason in the transcript rather than
half-filled in.

**An agent that stopped is started again by talking to it.** Which is what
the view tells the reader to do, and what it did not do: the handle of a
conversation that had ended stayed in place, so the check for "is there an
agent" found one and said the message into a channel whose far end had gone.
The handle stays -- the view reads the state off it, and a screen that
forgot the agent had died would have nothing to say about why nothing
happens -- so what asks is whether it has *exited*, not whether it is there.
Everything it was waiting on goes at the same time: a card the reader can
answer into a dead channel is worse than no card.

Its last words are a line. The protocol crate's `Display` is its message
followed by every field of `data` pretty-printed, which for an agent that
exited is four rows of JSON carrying one sentence and the source path of a
crate in the cargo registry. The sentence goes in the transcript and the
whole of it in the log.

**A tool call is somewhere to go, not something to read about.** The
protocol says what sort of thing the agent is doing (`kind`) and which files
it was in (`locations`), and obelus kept neither: a row with a title and a
tick on it. The kind picks the glyph, because a reader scanning a turn is
looking for whether it *changed* anything and that is a picture rather than
a sentence; the locations go on the row as a path, written relative to the
tree obelus was opened on. Other clients open a preview from one of these;
obelus opens a *buffer* -- with its jump list, its definitions, its hunks --
which is the one thing a reader has that they do not.

Everything on the row is kept rather than rebuilt, because an update carries
only what changed. An agent saying "it finished" and nothing else is not
saying the file it was in has stopped being the file it was in, and a row
rebuilt from that update would lose its kind, its title and its place at the
moment it succeeded.

**The transcript has a cursor, and it stands only on rows that do
something.** A tool call names a file; prose does not. The cursor steps over
what cannot be opened -- the rule a list follows for a row that cannot be
chosen -- so a reader walking a conversation never lands somewhere enter
does nothing, and the lit row is the promise: what is marked is what opens.

The arrows move the nearest thing that can still move. Where there is a row
to stand on they walk to it and the view follows; where there is none they
scroll a row, which is what they have always done and what a conversation of
nothing but words still needs. Enter opens what the row names -- in a
buffer, and the conversation hides itself, because going somewhere means
seeing it. Escape comes back out to the box without closing anything, and
typing goes to the box wherever the cursor was, because a reader who starts
typing means to type.

**One screen animates at a time, and only while something is moving.** The
welcome screen's sheen and the row that says an agent is working are the
only two, so one question decides it every frame: the conversation's, while
it is showing, and the welcome screen's otherwise. Asked from what is true
rather than switched on and off from the places that change either -- which
is how a ticker outlives its reason and wakes twelve times a second behind a
screen where nothing is happening.

The row that says something is happening *turns*: a picture of a cog says a
tool was used, and only movement says it is still going. Braille, so it needs
no particular font, and drawn whether or not glyphs are -- it is the one
thing on screen that has to be legible without them. `Ticker::start` still
answers `None` over a network, where an animation is a luxury paid for in
round trips.

**A header says what a thing *is*; the foot of the transcript says what is
happening.** The conversation's header carried five states, and they were
the wrong five: two said what the screen already said better ("nobody is
chosen", beside a header already reading "no agent" and over a transcript
already saying where to fix it), one said "not started yet" about an agent
that had *failed* to start, and the two that were real -- starting, thinking
-- belong where the next thing will appear, because that is where the reader
is looking. So the header is the name, and nothing else.

What is happening is one row at the foot of the transcript, worked out from
the state every frame rather than written into the transcript. A state has
no history: the next one replaces it and Ready removes it, and what is not
stored cannot be left on screen saying something that has stopped being
true. What *went wrong* is the opposite and stays a line in the transcript
where it went wrong -- an agent that died, and why, is the thing a reader
needs next, and a row that overwrote it would take away the only record.
`esc stops it` rides on the row that says something is going, beside the
thing it would stop.

**A change that has happened is the working tree's; a change that has not is
the agent's to show.** An agent that edits a file leaves the file different
from the last commit, and drawing that is what obelus does all day: the
margin, `show-change`, `alt+d`. Rendering the agent's own diff over it would
be a second answer to the same question, and the wrong one when something
else has touched the file too -- so an edit that has been made is a row with
`+12 -4` on it, and the file is where it is read.

A change it is *asking* to make is the opposite: the lines are in neither
the file nor the last commit, and the reader is being asked to agree to
them. Those go in the transcript, under the call's own row, open -- and they
stay there afterwards, which is how a reader finds out later what they
agreed to. Drawn the way an opened hunk is drawn in a file, tinted to the
edge with the marker's bar against the text, because it is the same thing
being said. `Theme::marker_colour` is where both views ask what a change
looks like.

The protocol sends the file as it is and as it would be rather than a patch,
so obelus diffs the two with `Changes::between` -- the engine the margins
come from. Nothing parses anybody's patch text, and a proposal is read with
the same hunks as everything else. The rows are worked out once, when the
call arrives: a frame is not the place to diff a file.

A change folds itself once the call it belongs to is finished, because by
then the file has the lines and the margin has the change. While the call is
pending it stays open: it *is* the question. The reader's word beats both,
as everywhere else.

**A run of tool calls of one kind is one row until the reader opens it.**
Thirty calls in a turn is a log, and a reader looking for what the agent
*did* should not have to scroll past the machine to find it. Three in a row
is where they stop reading them and start scrolling past them, so three is
where a run folds itself. Opened -- enter on the heading, which is the same
"do what this row is for" enter means everywhere -- the members are rows of
their own, each one a file to go to.

A failure in a run opens it, because a failure is the one thing in a turn
nobody should have to go looking for. What the reader said about a run beats
both: one they closed stays closed, whatever is in it.

**Thinking is not folded away.** Folding is for repetition, and thinking is
prose -- often the most of what a turn is worth, since it is where the agent
says why it thinks the bug is where it thinks it is. It gets a heading so a
reader who has read it can put it away, and only when it is long enough for
that to be worth a row: a heading over three words is two rows saying one
thing. obelus never closes it by itself, which also means it can never close
under somebody who is reading it.

**A question is a card, not a picker.** A picker is for finding one thing
among many by typing at it: a query, a fuzzy match, tabs, rows arriving from
a walk. A question is somebody else asking, with a handful of named answers
and sometimes room to write your own. Strip the filtering from a picker and
nothing of it is left but the row drawing -- and what a card needs on top of
that is a row that *grows*, which the list machinery cannot have: every
picker in obelus counts one row per screen row, and a file list of thousands
must not pay for a box one caller wants. So `component/card.rs` composes the
two halves obelus already has -- the rows, and the `Composer` a message is
written in -- and `ui/card.rs` draws them.

The card sits where the box sits, because while the agent is waiting there
is no message to send, and the transcript shrinks by however much it needs.
The conversation keeps the status row: a card is part of the conversation
rather than a list opened over it.

**Enter acts on the row the reader is on, and that is the whole key table.**
One answer: enter on it answers the card, with whatever is in the box. Many:
enter ticks, and the card is sent from a row that says `submit`, because
ticking and sending cannot both be enter. In the box: enter sends, `alt` and
enter makes a line, which is what enter does in the box anywhere else in
obelus. No new key was needed -- not even space, which everywhere else in
obelus is a character.

Walking does *not* choose. The box is under the answers, so every way to it
walks over them, and a card whose answer followed the focus would answer
with whichever row the reader passed on their way somewhere else. Typing
goes to the box wherever the reader is, and a card with no box swallows what
is typed rather than letting it fall through to the box underneath, which is
covered and would carry it to the agent as a message afterwards.

What the card cannot do yet it says rather than refuses silently -- `at
least 2` on the row that sends it, or a row of its own where there is none
-- and only once the reader has asked for it. A card that opens saying
"choose one" is telling somebody who has tried nothing yet that they have
got it wrong.

**What a permission request is about is a row in the transcript, not a line
of obelus's own.** It used to write "asking to run the tests" as a note and
then put the question underneath -- the same words twice, once obelus
started putting the call itself in the transcript. Now the call goes where
every call goes, waiting, which is what says the agent is asking about it;
the card below carries the answers and whatever the agent said about why.

**A question the reader did not start says what it is about.** The card
carries an `about` -- prose above its answers, a rule under it -- and both
questions an agent can ask fill it: a form puts its own message there, and a
permission request what the agent is actually going to do: the tool call's
own content, which is the command or the text it carries, and the files it
names when it has none. "Allow" and "refuse" are answers, and a question
with the words missing is not one a reader can answer. The title is in the
transcript directly above the card, where what the agent is doing is said.
The compact list keeps an `about` of its own for the same reason, whichever
list needs one next. It is
wrapped to the width and capped at five rows: it is somebody else's prose,
and an agent explaining itself at length must not push the list it belongs
to off the screen. `raw_input` is not used -- that is the agent's own
arguments in its own shape, and reading meaning into it would be obelus
guessing. A form said it in a transcript line of its own once ("it asks:
..."), which is the same words twice: the question is on screen, and what
it is about belongs over it rather than above the last thing the agent
said.

**A list open over anything owns the status row.** It is the thing taking
the keys and holding the caret, so `StatusView` draws its prompt before the
settings' filter or the conversation's own row. A row belonging to what is
behind the list is a prompt with somebody else's words in it, and the caret
sitting in it says the words are being typed there.

**A command is the agent's namespace; a setting is obelus's to draw.** Two
things in the protocol, and they must not be mistaken for each other. An
agent's slash commands are names it takes *in a prompt* -- a client offers
them and sends the text, and that is all. Session *config options* are the
other kind: `session/new` and `session/update` carry the whole set,
`session/set_config_option` changes one, the answer is the whole set again
because one value can change what another offers, and the client draws them
itself (a boolean one only if it advertised
`session.configOptions.boolean`). So the conversation's status row is every
option with its current value, walked and changed there -- not a command,
because the keys that move what is on a screen belong to that screen, the
way `shift+tab` always has.

obelus used to take `/model` for itself: the agent's command and obelus's
setting had the same name, and Copilot's own answer to that command is "the
model-picker dialog is only available in the interactive CLI", so opening
the setting's values instead looked like a kindness. It was a guess about
somebody else's namespace -- nothing promises that a command means what an
option of the same name means -- and it is gone. `/model` goes to the agent,
whose answer is its own business; the same choice is one key away on the row.

What Copilot offers, probed at 1.0.83: `mode`, `model`, `reasoning_effort`
and `allow_all`. It does not elicit for `/model` either -- with
`elicitation.form` advertised it still answers in words. Its mode ids are
URLs, and most of its rows describe themselves with their own name, which is
why a description that repeats the name is dropped. What kind each of those
options is declared as, obelus now writes to the log as it arrives: how an
agent declares one decides how it is drawn and what enter does to it, so
that line is where "why is this one drawn like that" is answered.

**An agent is installed when the install says so, in writing.** The last
thing `install::spawn` does is write `agents/<id>/installed.json` -- the
command, its arguments, and the version -- and every later question reads
that one file: is it installed, which version, how is it started. Nothing
infers an install from the files a package manager left, because that cannot
be done: `npm` writes a package's manifest before it links the executable,
so a run killed halfway leaves a directory shaped exactly like a finished
one. It did read them once, and the cost was a reader whose obelus was shut
mid-install and who then had a card reading "active" over an agent nothing
could start, with no button on it but the one that turned it off.
Working out what to run happens *inside* the install, where the registry's
entry is in hand: an install that cannot say how to start what it installed
has failed. So an interrupted install is simply not an install, `activate`
refuses an agent with no record, and a card says "active" only for one that
is really there.

`agent::home` is the one place an id from the registry becomes a path, so it
is the one place that checks the name, and it returns `None` for one it will
not make a directory of.

**Pressing install still runs `npm`, so a test must not press it.** The root
has a hook (`App::agents_root_for_test`), which is what lets a test write
the record a finished install would leave and then drive `Event::Installed`.
Everything about talking to one goes through `App::talk_to`, which takes the
command directly and needs no registry, no install and no network.

Golden fixtures dump every cell's symbol, foreground and background, plus the
cursor position. Colours are in them because highlighting, themes and the tints
behind an opened hunk are otherwise not asserted at all: a list of file names
can render perfectly and show nothing.

**Nothing draws a band of colour across a row.** The status row is the page's
own colour, like the conversation's row below the box: it has a rule above it
saying it is a different subject from the file, and saying that twice makes a
strip -- the heaviest thing obelus draws -- out of the smallest part of the
screen. What is left of that band is `control_background`, one shade off the
page, behind the track a switch's knob slides along.

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
the scrollbar, `show-change` and the steps between hunks), the diff/semantic
bridge (M3), symbol-level history (M4), the agent bridge (M5), and a minimal
editing set, last. Don't start on these without being asked.

`git show` and `git status` are shelled out, behind `git::head_text` and
`git::statuses`. A library (`gix`) is worth its weight when the views arrive
and can take over behind those two without anything above noticing; four
hundred crates for one blob read is not.

Also waiting on a configuration file, which does not exist: the Nerd Font
switch, user theme colours, the server table, and the word-wrap toggle all
want one.
