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

**A form is asked one question at a time, and the order is the agent's
needs.** `elicitation/create`'s schema arrives as a *map* of fields -- JSON
objects have no order to keep -- so the order the agent wrote them in is
gone before obelus sees it, and asking in the alphabet's order put an
"Other, if none of these suit" in front of the list it was an alternative
to. What is left to go on is `required`: those first, in the order the agent
listed them, and the rest after.

What it does not need, a reader must be able to say nothing to -- enter on
an empty box for words, a row of its own in a list. Escape is not that
answer: escape gives up on the whole form, which is the one thing a reader
walking past an aside does not mean. Without it an optional "anything else?"
was a question with no way out but abandoning everything already answered.

**An agent that wants to ask something uses `elicitation/create`.** That is
the one way it can put UI on a client's screen, and it is gated on a
capability: no `elicitation.form` in the handshake and an agent either falls
back or gives up. What it may ask for is a flat form of primitives, and
obelus puts it the way it puts everything else -- a list where the answer is
one of a few or a switch, the box where it is words or a number -- one field
at a time, because a terminal reader has one thing on screen and one caret
in it. The whole form goes back as one answer, keyed by the agent's own
names; escape declines it, and the view going away cancels it, because an
agent that hears nothing waits for ever. `elicitation.url` is *not*
declared: obelus is not a browser, and a mode it cannot put is a mode it
should not be sent. Anything else -- a multi-select, a property type it has
never heard of -- is declined with the reason in the transcript rather than
half-filled in.

**A question the reader did not start says what it is about.** The compact
list carries an `about` -- prose above its rows, a rule under it -- and both
questions an agent can ask fill it: a form puts its own message there, and a
permission request what the agent is actually going to do:
the tool call's own content, which is the command or the text it carries,
and the files it names when it has none. The title stays on the prompt row,
because a line is what fits there; "allow" and "refuse" are answers, and a
question with the words missing is not one a reader can answer. It is
wrapped to the width and capped at five rows: it is somebody else's prose,
and an agent explaining itself at length must not push the list it belongs
to off the screen. `raw_input` is not used -- that is the agent's own
arguments in its own shape, and reading meaning into it would be obelus
guessing. A form said it in a transcript line of its own once ("it asks:
..."), which is the same words twice: the question is on screen, and what
it is about belongs over it rather than above the last thing the agent
said. Said once, in front of the first question, because after that the
reader is in the middle of answering and knows what they are answering.

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
