# Obelus

A code **editor** shaped to one person's habits, with an agent built in. In
the AI era most lines you read are lines you did not write, so much of the
time in it is spent reading what an agent changed, beside the code it changed.
The eventual point is to join the LSP semantic graph to the git timeline —
jump to a definition from inside a diff, symbol-level history — which no
terminal tool does today.

It is an editor, and not a powerful one, on purpose. What it has is measured
against how its author works, not against an editor's checklist (multiple
cursors? macros?) — none of which it wants. Say so wherever it is
described: an editor that fits one person's way of working, with the agent
part of it, rather than one competing on features.

## Commands

```
cargo build
cargo test
cargo +nightly fmt              # NOT `cargo fmt`
cargo clippy --all-features --all-targets
cargo run -- crates/obelus-app/src/app/mod.rs               # ob, here
cargo run -p obelus-gui -- crates/obelus-app/src/app/mod.rs # obg, in a window
UPDATE_FIXTURES=1 cargo test    # regenerate golden cell grids
cargo test -- --ignored         # the slow real-server tests, and the diff sweep
OBELUS_REQUIRE_LSP=1 cargo test # a missing rust-analyzer fails rather than skips
OBELUS_PATIENCE=10 cargo test   # an agent test that fails gives up in 10s, not 180
```

`.rustfmt.toml` uses five nightly-only options. Stable `cargo fmt` silently
ignores them, and mixing the two makes the formatting oscillate. **Always
`cargo +nightly fmt`.** The build itself is stable.

Clippy must be silent. The lints are the workspace's, in the root
`Cargo.toml` -- `missing_docs`, `unreachable_pub`, `private_interfaces` --
because a `#![deny(..)]` in one crate root would silently stop applying to
the other twenty-six. Every member opts in with a `[lints] workspace = true`
of its own, which Cargo does not inherit for it, and every public item needs
a doc comment.

## Rules that are easy to break by accident

**Nothing reaches `master` except through a pull request.** Windows, macOS
and Linux on arm exist only in CI, so a change green at this terminal is one
nobody has run on three of the systems it ships to -- and pushed straight to
`master` it goes red there with the tree broken until a fix lands. CI runs
on `pull_request` as well as on a push, so a branch is answered before it is
merged. What this cost last was a claim's lock: whole-file on Windows, it
passed every test on Linux and failed the one that reads the bytes under it.

**Every test must be checked by breaking the thing it covers.** Write the test,
then deliberately break that path and watch the test fail. Seven tests here
passed while the feature under them was broken; each was found this way and no
other way. If a break does not fail the test, the test is not testing what its
name says — fix the test, and say in a comment what the deliberate break was.
Beware of "breaks" that are equivalent rewrites, and of assertions that ask the
rule under test what it expects.

**All coordinate arithmetic lives in `obelus-text`.** Five newtypes with
private fields (`ByteOffset`, `CharOffset`, `LineNumber`, `CharColumn`,
`DisplayColumn`, plus `Utf16Column`) exist so that byte, character, display
column and LSP column cannot be mixed up. Nowhere else adds or subtracts them.

**One thing on screen is drawn by one piece of code.** A screen stays
coherent because there is nowhere for two answers to the same question to
drift apart, not because everybody remembered the convention. So before
writing a view, look for the piece that already does it:

    ui::write_marked   a row's text: what matched marked, what the file
                       colours coloured, clipped to the list it is in
    ui::tabs           a tab row and the arrows that walk it
    ui::status::typed  the glyph and the words on a row that is typed into
    ui::nothing        what a list says when it has nothing in it
    ui::rule           a boundary between two things
    ui::scrollbar      how much of something longer than the screen is above,
                       and where a press takes hold of it (ui::bars)
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

**Copy starts with a capital; a name keeps its own spelling.** Everything
Obelus says to the reader begins with a capital -- a key's word at the foot
(`Read it`, `Leave`), a note on the status row, an empty list's line, a card,
a tab, a setting's name and its gloss. What is *not* copy is a name, and a
name is written the way it is written everywhere else: a command (`open-file`
-- also the word in the config file's `[keys]` table), a theme (`dark`), a
language as it is counted (`Rust`, `TOML`, `Plain Text`), and a file's path.
Where a sentence would have to start with one of those, reword it rather than
misspell the name -- `Nothing is bound to open-file`, not `open-file has no
key`.

Obelus's own name is a name like any other, and it is written `Obelus`
wherever the name is being written -- copy, a log line, a refusal sent to an
agent, a comment. What stays lowercase is not the name but the things named
after it: the `ob` binary, the `obelus-*` crates, `~/.config/obelus` and the
project's `.obelus`, the `obelus::` log targets, and the word it gives itself
on the wire (`Implementation`, and the MCP server it offers an agent). The
lowercase *obelus* is also a word of its own -- the mark a scholar put beside
a doubtful line -- and where the text means the mark rather than the program
it keeps its small letter.

Three things are not Obelus's to capitalise, and are left exactly as they
arrive: what an agent sends (its modes, its tool titles, its questions),
what a language server sends, and a protocol's own words -- a tool call's
`state` is `"failed"` because that is what the wire says, even though the
row drawn from it reads `Failed`. Nor is a fragment that lands mid-sentence:
`Copied {what}` takes `selection`, and `Starting again, because {why}` takes
a reason, both lowercase where they join. Log lines and `anyhow` contexts are
not copy either -- they keep the lowercase Rust writes them in.

**A buffer's path is where it is *called*, not always where its bytes came
from.** `Content` is what says which, and everything that assumed the two
were the same had to be asked: the watcher must not re-read the file over a
commit's version of it, opening the file must not hand back the buffer that
happens to wear its name, the language server must not be told that this is
what that path holds, the diff cache must key on the content as well as the
path and the version -- both start at version one -- and a blame must not be
laid beside it, because a blame is a walk from `HEAD` and its lines are the
lines of the file as it is now. The status row says which commit, where the
mode and the staleness go: they are all the same kind of fact, that what is
on screen is not simply the file at this path.

A commit's version is marked against the commit *before* it, so the margin
says what that commit did rather than how it differs from today -- which is
a question about a file the reader is not looking at.

So two buffers can wear one path, and a path alone cannot say which: match on
the path *and* on `content().at()`, and a list of them says which is which.

**A global is right when the thing is a decision the whole program shares.**
How wide a tab is drawn is measured by nine methods on `Text` and every
caller of each; threading it through would put a parameter on the arithmetic
rather than on the setting. The glyph switch is the precedent. The cost is
paid by the tests, which have to take turns -- and go in a binary of their
own, because a test beside them that sets a config moves it underneath.

**What an agent does, the reader can see and take back -- and where it
cannot be taken back, seeing it is the whole of the answer.** The refusal
was never that an agent should not write. It was that a reader could not see
the change arrive or undo it, which an undo answers: a write goes through the
buffer as one change, and `ctrl+z` is what it was.
A fake agent that asks to write a file in the repository is a test that
rewrites the repository -- it asks about a path outside the project instead.

Running a command has no undo, so it is held to the other half only, and
held to it harder. Obelus runs what it is asked without asking the reader:
*the agent* asks -- that is what `session/request_permission` is for, and a
client asking again is a second question about one thing, which is the same
rule Obelus's own tools follow. What Obelus owes in exchange is that the
command is on the page in the words it was actually run in (not the agent's
title for it), that everything it printed is there and a failed one stays
open, and that the key which stops the agent stops the process too -- Obelus
started it, and nothing else can.

**What a frame asks every frame must answer without doing the work.** The
conversation keeps its rows laid out and throws them away when what they
are made of changes -- and the two things asked on *every* frame, what is
happening now and what Obelus's own commands have printed, threw them away
before looking at whether the answer had moved. So every keypress laid the
whole transcript out from its bytes again: measured at 75ms on a thousand
rows, against 213us for the rows it already had, which is a cursor a reader
watches arrive. Both ask first now. The same shape as the ticker's rule --
asked from what is true, and quiet when nothing is.

The other half of that cache is its width. It holds one laying out, keyed
by the width it was made at, so two callers asking at two widths lay the
whole thing out twice per frame between them: 146ms a keypress in a
measurement where the keys used the region's width and the drawing used the
transcript band's. They are the same number today because a band is as wide
as its region, and that is why `App::chat_key` takes its room from the same
functions the view lays itself out with rather than from anything of its
own.

And a file read is the same mistake in a costume, because it does not look
like work. Three of them were behind questions a view asks: whether this
project has any conversation to take up, drawn on the conversation's status
row; which of the notes has one, drawn on the notes page; and what each
agent's install record says, drawn on the settings. Parsing that table is
37us for one conversation and 351us for twenty, and the notes turn a mark
while any agent is at work -- twelve times a second, for a page nobody is
typing on.

**Nothing is polled.** What to do about a question a frame asks is either to
ask a cheaper question or to be told, and there is no third answer. Ask a
cheaper question, or none at all; or keep the answer and hear the change --
which conversation a note has is a table somebody *writes*, a write is
something a watcher hears, and `sessions::change` hands back what it wrote, so
Obelus's own write re-reads nothing. The one that looked like a third answer
was the claims: a claim is a lock, and taking one writes nothing. Something
does tell you -- the kernel closes a dying process's files, and a watcher
reports a file closed by a process that had it open for writing -- which is
inotify's alone, and what it costs elsewhere is a sentence rather than a lock
(`obelus_watch`, `obelus_agent::chats`). So the test for which shape a thing
wants is not how dear it is but whether anything could tell you it moved, and
the question is worth asking twice before settling for a poll.

That leaves three moments a thing is read: when the view that shows it opens
(a watch says what happens next, not what was already there), when the watcher
says so, and when Obelus is the one who changed it. Which is not the same as
the moments a *watch* is taken: `App::settle_the_watches` holds all of them
and is asked from what is open, once a frame, and `Watched` keeps what is
*wanted* apart from what is *held* -- only the first decides when to read.

And a number is a reason to look; it is not on its own a reason to add
machinery. The agents page reads an install record per agent three times a
frame -- 150us on a keystroke nobody is waiting on, on a page nothing animates
-- and was left alone.

**Somebody else's text is capped.** A call's title, an agent's `about`, a
card's prose may be as long as they like and may not push what they belong to
off the screen -- and a row works out the room for what it says about itself
before the words get what is left, so a long title cannot take the tail with
it (`component::chat`, `component::card`).

**A diagnostic is a mark against a piece of a file, and a server is not the
only thing that can make one.** What Obelus cannot make of a file it reads for
its own sake -- the settings, a theme, the notes, a `[keys]` line that bound
nothing -- goes into the same list a server's go in, under Obelus's own name
(`Reported::source`), so everything that draws or walks problems reads one
list. What Obelus writes for itself and a reader never opens stays a line in
the log, and what went wrong on the way up -- the marks on files nobody has
opened yet, and what has no line to mark -- is a list put up over the first
screen of a start, read there and let go with escape
(`app/preferences`, `obelus_lsp::trouble`, `App::tell_what_went_wrong`).

**A setting the reader turned on is not a reason to refuse them.** Saving
with formatting on and no server to ask writes the file unformatted. The
alternative is a file that is never written because of something the reader
cannot see.

**A document has one door to change through.** A buffer holds a great deal
beside its text -- a parse tree, the folds, the blocks hanging between lines,
the cursor, a version five separate caches key on -- and every one of them is
measured against the text. Spread the changing across the program and each of
those becomes a thing somebody adds without remembering the others.
`Buffer::edit` is the door and the list lives there.

**A key that cannot be a command goes where the motions go.** `why_not`
refuses `Enter`, `Tab`, `Backspace` and `Delete` because every list and box
takes them itself, and a printable character is not a name anybody would type
into a palette.

**"Are you sure" is the same key again.** The status row takes a line of
text, not an answer, and pressing save or quit twice is what every editor a
reader has used already means by it. The sentence has to fit a narrow row:
one that does not is dropped whole, and a warning nobody sees is not a
warning.

**A limit on a list is a limit on what can be found in it.** The history's two
hundred commits on the main thread froze the key and made "no match" a lie
about rows that were never fetched; walking all of it on a thread costs
nothing at the key. Reach for a thread before reaching for a number
(`obelus_git::history::spawn_log`).

**A key that opens a thing may only close that thing.** Blocks carry a `kind`
for exactly this: ask it.

**What Obelus writes, Obelus has to be able to read.** A format with a writer
and a reader in the same program has a test that the one reads the other, or
they drift and the symptom turns up somewhere that looks unrelated -- the log
was once the one file Obelus could not give a reading
(`obelus-reading/src/log.rs`).

**Ask the question you mean.** *Whether* anything changed is not *how many*
things did: ask for the first one rather than the length of a list of them
(`has_any`, `obelus_git::anything_changed`). And measure afterwards, honestly:
a cost that lives in the walk is not saved by rephrasing, and saying otherwise
is a win claimed rather than got.

**A setting about how something is drawn is not a setting about whether it
can be asked.** Turning the margin's names off turned off the walk that
finds them, so the key that opens the commit behind a line went dead and
said "still reading who wrote this" while nothing was being read -- the one
answer that was false. The margin obeys the setting; the question does not,
and the key starts the walk itself when nobody else has. A reader who wants
no names in the margin has not said they never want to know.

**Gate a command on the question, not on the answer.** `f11` was offered
only once the blame naming that line had arrived -- which reads as
precision and is a trap: for a reader with the margin's names off nothing
ever starts that walk, so the row was greyed out for ever and the palette
was the one place they could not get started from. Requirements have to be
things that are known without doing the work. Then the key has to hold the
question while the work runs, or it is a key that needs pressing twice for
exactly the readers it was greyed out for.

**A key does nothing where its command is dim.** `App::offers` is the one
judgement of whether a command can do its job here: the palette draws a row
it refuses as dim and will not run it, and `App::handle_key` asks the same
question before dispatching, so a command cannot be off in one place and
live in the other. It is silent about it -- `f2` with no file open used to
draw an empty list of open files, `f3` on a clean project wrote "nothing has
changed" across the status row, and `ctrl+c` with no selection said "nothing
selected"; three answers to a question the palette had already said could
not be asked. So `Requires` is where that work goes, and a note inside a
command for "you cannot do that here" is dead code unless the condition
cannot answer exactly (the bracket scan is the one that cannot: `ABracket`
is the character under the cursor, and a bracket inside a string is offered
and finds no partner).

Which makes a condition worth a walk: `AChangedFile` asks git what has
changed in the project, once when the palette opens and once per press of the
key. This repository answers in two milliseconds.

And `AProject` is the one that is known without any walk at all: a start
from a desktop menu names no project and begins in the home directory, so
until the page that asks which project is answered there is none -- which
`open-changed-file` and `show-project-history` used to discover by walking a
repository that was not there.

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
  open file, a changed file, a conversation) and `f5`-`f8` are finding, one
  question at four radii -- this file or every file, its text or its names.
  `f9`-`f12` are git's and the jump to a definition. Bare, never with a
  modifier: one terminal reports `shift+f5` and the next reports `f17` for the
  same press.
* **Control does something to the file in front of you**, on the letter of the
  word (`ctrl+p` the palette, `ctrl+w` close, `ctrl+c` copy, `ctrl+q` leave).
* **Alt asks about the cursor, or walks what was found** -- and alt is the
  escape prefix, so it arrives everywhere.
* **Shift names no command of Obelus's own.** It only extends (`shift` plus an
  arrow) or reverses (`shift+tab`), which leaves it meaning one thing
  everywhere. `shift+Insert` and `ctrl+Insert` are the exception that says
  what the rule is about: they are the names a desktop already sends a
  terminal for paste and copy. Why Obelus answers them, and why copy still
  cannot work where a terminal keeps them, is argued in `keymap::why_not`.

**Escape always gives up on the nearest thing**, and everything else is
reached from the palette: a chord for every command is how a key table stops
being memorable. Which key is which is argued where it is bound, in
`Keymap::new`.

**What is showing owns the keys.** A list and the settings page are dialogs:
each takes the keys bound *in* its context and nothing else, so a global key
cannot open a second thing over the first. A new dialog gets a context, and a
key it should keep gets a binding in it -- not a fall-through
(`keymap::Context`). Except a key that names another whole view: from inside
one it swaps the two rather than stacking them, so there is still one thing on
screen and one escape back to the file -- and a view that has bound the key
itself beats the swap (`app/switching`).

**One mark for "the keys are here", and it says nothing else.** Every list,
page and card in Obelus puts `selected_row_background` behind the row the
reader is on -- a picker's rows, the settings', an agent's question, the
transcript -- and the same colour behind the one item of a row of them, for
the things laid out across a row rather than down a column. Where there is a
caret there is no background: a box is marked by the caret sitting in it,
and two marks for one fact is one too many.

And only the nearest thing on screen wears it. A page or a list under
another is drawn as it was, rows and all, but its row is not marked: two
lit rows are two places saying the keys are here (`ui::in_front`, which
every view that marks a row asks).

Whether a row can be *used* is said in the ink, never by taking the
background away. A card's `submit` row did the latter while it was short of
what the agent asked for, so the reader stood on a row that had stopped
saying it was under them: they pressed enter, got nothing, and had nothing
on screen to tell them which row had refused.

**The wheel moves the view; the keys move the cursor.** A notch scrolls what
is on screen and leaves the cursor where it was -- `scroll_by` on a buffer
moves the viewport and nothing else -- and the paging keys move the cursor by
a screenful, with the view following it. Two gestures, two jobs: a reader
spinning a wheel is looking around, and one pressing a key is going
somewhere. A list is the exception that proves it: there a notch steps the
selection, because a list's view *is* its selection and there is nothing
else in it to scroll. A bar taken hold of is not the exception: it is a
picture of where the view is, so dragging one moves the window and leaves
the selection chosen until a key moves it (`component::window`).

**Wherever enter means something else, a line is `shift+enter` *and*
`alt+enter`.** Both, every time: `shift+enter` arrives only from a terminal
that speaks the kitty keyboard protocol -- `main` pushes its narrowest flag
for this, and nothing else in Obelus depends on it -- and `alt+enter` arrives
from the rest, because alt is the escape prefix. A place that took one left
the other falling through to whatever was underneath, so the pair is taken
together, and before the modifier check (`component::composer`).

**`dispatch` has no wildcard arm** and warns on one, so a new `Command` fails
to compile until it is handled. Same idea in `theme`: only fields with readers.

**Nothing writes to stdout.** stdout is the drawing surface; `tracing` goes to
a file. A stray `println!` lands in the middle of a frame and stays there.

**The main loop is threads and one channel, not a runtime.** One
`std::sync::mpsc` channel, one producer thread per event source (keyboard, file
walk, watcher, each server's stdout), the main loop blocking on `recv()` and
draining with `try_recv()`. Not a rule against `async` or against tokio --
tokio is in the project, and everything that *waits* rather than works runs
as a task on the one runtime (`obelus-runtime`): a server's pipe, an agent's
connection, a clock, a download. None of those needs a thread of its own, and
each of them had one. What the rule is about is the loop: one owner of `&mut
App`, and no `.await` between a key arriving and the screen it produced.

Writing to a server's stdin needs its own thread, because a busy server stops
draining the pipe. An answer that arrives after the world has moved on is the
normal case, which is why requests record the version they asked against.

**Obelus is drawn on two things, and the loop knows neither.** `ob` is the
terminal and `obg` is a window -- gvim's relation to vim, not a second
program: the same grid, the same `component/` and `ui/`, the same `App`. The
window is for the two things a terminal cannot give: keys a terminal sends as
one byte or not at all, and a font carried in the binary rather than guessed
at. The seam is one trait and one channel -- `ratatui::backend::Backend`, and
the receiving end of the loop's channel handed to `app::run` -- and a key is a
`crossterm::event::KeyEvent` everywhere inside Obelus, `obg` included. What is
not shared -- the drawing, the clipboard, which settings apply (`Drawn`), an
input method's spelling -- is argued in `obelus-gui`'s modules and
`obelus-clipboard`; and a measurement in columns has to ask which front end it
is for.

**The caret's shape says what the next character will do**: a bar between two
characters, a block on one, following the mode, and only in a document
(`App::caret`). Only a window can draw the shape, so the status row says
`Replacing` in a word as well.

**Obelus does not split its window, so several Obelus processes is the
normal case.** A terminal already splits, tiles and tabs better than an
editor can from the inside, so Obelus has one region and no panes. What that
buys has to be paid for on the other side: two or three of them on one
project, plus the reader's own shell in the same repository, is how Obelus is
actually used, and nothing it writes outside a buffer belongs to it alone.

The rules that come out of that, every one of them broken once:

*Anything read once at startup must be re-read when somebody else changes it.*
The watcher watches the settings file and `App::reread_config` applies what it
finds -- all but the agent, because a conversation is this window's.

*Anything written must survive another process writing it at the same moment.*
`save_to` writes beside the file and renames over it, and a file that is there
but will not read stops Obelus writing at all until it reads again.

*Anything cached about the world must be dropped when the world moves.* `HEAD`
and `index` are watched, and `App::forget_what_git_said` drops what git said
when either moves.

*A tree that goes takes the project and everything open in it.* `git
worktree remove` or an `rm` in the reader's shell is somebody else changing
the world: a page saying so goes over the whole screen, on top of what the
reader was in, and answers two keys. Enter lets go of everything the
project was -- what was open, unwritten work and all, because there is
nowhere left to write it, and the servers, the agent and the tools with it
-- and asks which project next; the key that leaves leaves, and asks
nothing. Nothing of the project is written into a tree that is not there
(`App::the_tree_has_gone`, `ui::gone`, `obelus_git::project`).

*A repository and its worktrees are one project.* What Obelus keeps about a
project -- the notes, which conversation is about which note -- is keyed by
`obelus_git::project` (git's `common_dir`, canonicalised) and nothing else;
what a tree *shows*, like its branch, is its own (`head_of_the_tree`). The
test is whether two worktrees should agree about the answer.

*A conversation is not a thing two of them may have open at once.* The queue
that keeps Obelus to one prompt turn lives in a process, so a conversation is
claimed: a lock the kernel holds and gives up with the process, and a file
beside it for the watcher to wake on. What is claimed is the conversation, not
the note (`ChatId`), and a refused claim is drawn, never said
(`obelus_agent::chats`).

*And a note somebody else is talking about is read here, not changed* -- by a
key or by an agent's tool, because taking a note away is what destroys a
conversation; the lock runs upwards (`component::todo`). *And a note that has
gone takes no conversation off the screen*: the document keeps what it has,
and closing it is the reader's (`App::the_notes_are_now`).

Two smaller ones: an install claims the agent's directory with the same kind
of lock, so two windows do not run two `npm`s into one prefix
(`obelus_agent::claim`), and every log line carries the process's number,
because several Obelus processes share one log.

What is *not* shared is worth saying too: a language server and an agent per
process, which is the cost of not having panes. Three windows on one Rust
project is three rust-analyzers.

## Shape

```
crates/
  obelus-app/       state, the loop's handler, and every picker's item
                    source, by aspect: documents, moving, searching,
                    choosing, agents -- plus what one conversation is
                    (conversation), what the loop reacts to (event), and
                    everything done before there is a screen (startup)
                    · a commit's message hangs above its file, and the branch
                      is read where Obelus is told where it is (app/history); a
                      view is split by the errand, not by the shape of the
                      answer, and an answer on screen is an answer about a
                      moment (app/history_view); a preview is of a subject, not
                      of a path, and the paging keys belong to whatever is
                      being read (app/previewing); a project may carry
                      settings, and it is not the reader, and the reader's is
                      the layer it is laid over; anything read once at startup
                      must be re-read when somebody else changes it; what
                      Obelus cannot make of a file it reads is a mark on that
                      file (app/preferences); what Obelus says before the
                      reader's first words is one piece that is always said and
                      one the topic adds, and the reader's own words go into a
                      template last (app/opening); opening a conversation opens
                      its session, and saying something makes it the reader's
                      (app/talking); what a reader said outlives the window,
                      newest first, a conversation belongs to the agent that
                      had it and to the checkout it was had in, and a watch
                      is settled from what is open
                      (app/conversations); where Obelus has been is
                      remembered however the project was named, and only a
                      worktree, and what a start with nothing to go on asks
                      (app/projects); what was open is kept by the tree,
                      and the last window to change it wins (app/reopening);
                      what an agent offers is asked of it,
                      not remembered (app/agents); a note that has gone takes
                      no conversation off the screen (app/noting); a key that
                      names another whole view swaps rather than stacks, and a
                      view that bound the key beats the swap (app/switching); a
                      newer Obelus is asked about once a day, not once a start
                      (app/releases); only where Obelus draws its own window, a
                      window on a tree is a claim held the way a
                      conversation's is, a claim appears already held, and a
                      tree that has gone is nowhere to go (app/worktrees); a
                      conversation takes one prompt turn at a time, what is
                      waiting waits where the reader's words live, and it goes
                      as one prompt (conversation); nothing but the animation
                      waits on its clock, and a guard on a deadline earns its
                      place only when something else can reach the work (event)
  obelus-text/      the Rope wrapper: the only place coordinates convert
  obelus-editing/   a text with a caret in it -- the file being read and the
                    box a note is written in differ in what they are *about*
                    and not in what down does
                    · chords, contexts, the default table, modifiers_of;
                      `why_not` is the one judgement of what may be bound, and
                      `shift+Insert` is the desktop's name for paste and not
                      Obelus's; a dialog gets a context, not a fall-through;
                      keys are rebound on the keys page; modifiers are judged
                      exactly, in one place (keymap)
  obelus-buffer/    one open file: text, syntax, cursor, viewport
                    · an edit knows where it happened; do not read over an
                      edit; two modes, and two is enough (lib); a folded line
                      has no rows, what folds comes from the indentation, a
                      hunk opens where it is, a fold across an edit is not one
                      across a re-read (folds); the text's rows and the
                      screen's rows are two counts, what is highlighted is what
                      is drawn (moving); undo groups by what the reader was
                      doing (undo)
  obelus-row/       a row of laid-out text: the currency between whoever
                    lays something out and whoever draws it, belonging to
                    neither
  obelus-icons/     the Nerd Font switch and every glyph behind it
  obelus-theme/     colours, and only fields the renderer reads
  obelus-config/    the settings, their file, and what each one is
                    · what a project may set is a property of the setting; the
                      file holds preferences, not state; a setting says where
                      it is `Drawn`; do not delete what you do not recognise;
                      anything written must survive another process writing it
                      at the same moment (lib)
  obelus-logging/   two logs split by module, and where a panic goes
                    · every line says whose it is (lib)
  obelus-command/   the Command enum, its table, its groups, what each one
                    requires and where it is `Drawn` -- running one is
                    obelus-app's, in src/app/dispatch.rs
  obelus-component/ picker (one component, several instantiations), settings,
                    the conversation, the box a message is written in, and
                    the window every list shares
                    · a query is about the rows the list is of, whether it
                      ranks is settled per tab, a list still arriving sits
                      still, say "still reading" where it moves nothing, which
                      tabs a view has must be cheap, a row of a list says what
                      is true now, in one answer (picker); a list Obelus offers
                      is the reader's own project (picker/files); a question is
                      a card, not a picker, enter acts on the row the reader is
                      on, and a question the reader did not start says what it
                      is about (card); a tool call is somewhere to go, the
                      transcript's cursor stands only on rows that do
                      something, a run of tool calls is one row, a change that
                      has happened is the working tree's and one that has not
                      is the agent's to show, thinking is not folded away, a
                      call's title is three rows shut (chat); folding is one
                      act and the notes are the fourth place it happens, a note
                      somebody else is talking about is read here and not
                      changed, and a box is the reader's once they have put
                      something in it (todo); a setting is two rows, and a name
                      and a gloss (settings); what a call takes is about a
                      place, and somebody else's text is capped (signature);
                      wherever enter means something else a line is
                      `shift+enter` and `alt+enter` (composer); a list the
                      reader builds is not a picker (names); a screen that
                      cannot be left, two boxes that never share their
                      text, why no numbers, and what could finish a path
                      is an ordinary list somewhere else (chooser)
  obelus-reading/   what a file is when it is not code: markdown, a log
                    · a log's format is decided by its lines, not its name;
                      what Obelus writes, Obelus has to be able to read (log)
  obelus-markdown/  markdown, laid out into rows, from the tree Obelus
                    already parses
  obelus-search/    one question at three scopes, and how much code is here:
                    tokei's walk, in the two orderings the view reads it in
                    (counts)
  obelus-syntax/    the language registry (two dozen grammars), parsing,
                    highlights, tags
  obelus-lsp/       transport, client, actions, positions, outline, the call
                    the cursor is inside
                    · a parameter nothing is on is not the first parameter, and
                      every signature the server sent is kept (signature); a
                      server's running commentary is not news, that it is
                      running is (client); a server is not the only thing that
                      can make a diagnostic (trouble)
  obelus-git/       gix, reading only: head text, statuses, hunks, blame,
                    history -- and the notes beside a project (todo)
                    · nothing here writes, and what was measured before
                      deciding so; the diff base is the blob a checkout would
                      write, and reading it must not run anything; ask the
                      question you mean; a repository and its worktrees are one
                      project, and the branch is the tree's own (lib); a blame
                      is about a version, and the margin knew which commit
                      (blame); what the remote has not seen is marked, and a
                      list worth searching is worth threading (history)
  obelus-agent/     the ACP registry, installing an agent, its marks, and
                    the protocol through its own crate with the thread that
                    joins it to the loop (acp/)
                    · an agent is installed when the install says so, in
                      writing (install); an install is claimed with the same
                      lock a conversation is (lib); a conversation is claimed
                      by what names it, and one Obelus at a time has it, a
                      claim is held by a writer and looked at through a read,
                      says which checkout holds it, and a refused claim is
                      drawn, never said (chats); why the
                      two directions are not symmetrical, the one ordering the
                      protocol does not promise, what waits on the reader
                      does not wait in the handler, what an agent asking
                      something may ask for, and a command is the agent's
                      namespace while a setting is Obelus's to draw (acp/link);
                      an agent that stopped is started again by talking to it,
                      every word says which connection it came from, and Obelus
                      numbers its own turns (acp/mod)
  obelus-mcp/       the tools Obelus offers an agent -- and why none of them
                    asks the reader anything itself
  obelus-ui/        editor, status bar, picker, settings, chat, welcome,
                    which project, a project that has gone, images, shapes,
                    shared cell writers
                    · what the bar measures is what is shown, the caret can be
                      in the block, a bar is a block, a column a file might
                      need is reserved for the whole file (editor); everything
                      that scrolls says so (lib); a header says what a thing
                      is, the foot says what is happening, a row keeps the room
                      for what it says about itself, and a place the title
                      named is not said after it (chat); a list open over
                      anything owns the status row (status); asking which
                      project is a page of its own, and the welcome screen
                      comes after (projects); a
                      window draws the marks itself, and the view does not
                      know the difference (image); a view says
                      what a region *is* and the front end says what that looks
                      like, nothing may be said there that the cells do not
                      already say in their own way, and what is said carries
                      enough to be checked against them (shapes); the argument
                      being marked is the one thing that may not be clipped
                      away (signature); every layer is asked the same question
                      (settings)
  obelus-watch/     what changed on disk: a freshness mechanism and not a
                    correctness one, so nothing that matters hangs on it
                    · a file closed by a writer is the one access that is news,
                      and only Linux says so (lib)
  obelus-sink/      where a background worker's events go: a worker names
                    only what it produces, and the application is the only
                    thing that has heard of every worker
  obelus-runtime/   the one runtime the waiting is done on -- a server's
                    pipe, an agent's connection, a clock, a download -- and
                    giving up on a walk (cancel)
  obelus-program/   whether this machine has a program, and starting what it
                    turned out to be, which are two questions on Windows
  obelus-clipboard/ the clipboard through whatever the machine actually has,
                    and opening a link
                    · a copy in several shapes needs a client that owns the
                      selection, and the programs stay because an owner dies
                      with its process (lib); where the clipboard is a service
                      there is nothing to own, so a terminal offers Obelus's
                      own shape too (native)
  obelus-cli/       the `ob` binary: Obelus drawn on a terminal
                    · one key needed the terminal's permission, and nothing
                      else depends on the protocol (main)
  obelus-gui/       the `obg` binary, and the window: what a screenful of
                    cells becomes when it is not a terminal -- the grid on
                    its way over, the glyphs, the quads, and the one clock
                    the window keeps
                    · Obelus is drawn on two things and the loop knows neither
                      (main); `App` crosses a thread and stays there, and what
                      an input method is spelling is on the page and is not in
                      the file (window); closing the window is the key that
                      leaves (keys); the page is what Obelus said and motion is
                      only how the window is showing it, a moment and a rate
                      are two kinds of waiting, and none of it crosses to the
                      application (motion); a full-width character takes the
                      cells it covers with it, because the diff will not
                      (grid); the colours go through untouched, only a window
                      can draw the caret's shape, a pane joined to the page has
                      one edge and so no corners while a box joined to nothing
                      has four, a line is drawn where its glyph would be and
                      the glass starts at the line, glass is a bend and a light
                      before it is a blur, a region of the frame is put
                      back somewhere else rather than drawn again, and every
                      pane and every box is glass over everything said
                      before it (paint); a
                      mark is one cell here and two in a terminal, so a column
                      asks which front end it is for, and a key's cap is the
                      one place the grid is not what a cell is measured in
                      (font); a window owns the selection and hands the words
                      over on the way out (clipboard); a thread that borrows
                      somebody else's connection stops before the owner takes
                      it back (clipboard/wayland); a window may not put itself
                      in front of the reader, so whatever comes forward comes
                      on the permission of the window they are in (elsewhere)
  */tests/          integration tests, most of them `obelus-app`'s, plus
                    obelus-app/tests/fixtures/*.txt golden grids
                    · why the fake agent is `sh`, and what it checks back
                      (obelus-app/tests/agent); a test whose input is the
                      checkout's own history only passes at one moment
                      (obelus-app/tests/git)
```

A `·` line is the rules that live in that module's own doc rather than here,
by the sentence they open with. They are there because they are read at the
moment they matter -- and listed here because a rule you only meet by opening
the file is a rule you break while deciding not to open it.

**Keep this file short.** It is loaded whole into every session, so a rule
that is one module's goes in that module's doc and here only as a few words
on its `·` line; this file holds what crosses modules. A story told twice
drifts.

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
Obelus has none. So a test, a pipe and most terminals draw the glyph path,
which is why the fixtures never contain pixels.

**Nothing but the animation waits on the animation's clock.** Something on
screen moves only while something is moving -- asked every frame from what is
true (`App::wants_animating`), never switched on and off from the places that
change it, which is how a ticker outlives its reason -- and the ticker answers
`None` over a network, where an animation is a luxury paid for in round trips.
That is right for a sheen and wrong for anything the reader is owed, so every
wait has a one-shot clock of its own (`event::Pause`, through
`App::come_back_in`): a pause is a moment and an animation is a frame rate.
**A guard on a deadline earns its place exactly when something other than that
deadline's own clock can reach the work.**

**Pressing install still runs `npm`, so a test must not press it.**
`App::agents_root_for_test` lets a test write the record a finished install
leaves and drive `Event::Installed`; `App::talk_to` takes the command directly
(`obelus-app/tests/agent`).

**Nothing draws a band of colour across a row.** The status row is the page's
own colour, like the conversation's row below the box: it has a rule above it
saying it is a different subject from the file, and saying that twice makes a
strip -- the heaviest thing Obelus draws -- out of the smallest part of the
screen. What is left of that band is `raised_background`, one shade off the
page, behind the cap a key is drawn in at the foot of a view -- a few cells
wide, and a box, never a row. Not behind a panel, which is on the page's
colour with a frame round it: in a window its ground is its glass.

A preview's margin comes from git, so a fixture that shows one depends on
the fixture file being *committed*: edit `obelus-app/tests/fixtures/long.rs`
without
committing and the preview grows change marks. Which is the feature working,
and a surprising way to see it.

**A test that only passes in one checkout is a broken test, not a rule.** The
welcome screen prints the working directory, so its fixture once carried
`~/Work/obelus` and failed in a `git worktree` for a reason that had nothing
to do with the change under test -- and `UPDATE_FIXTURES=1` there wrote the
worktree's path into the fixture, which then failed everywhere else. The note
here used to say to run the suite in the real checkout. The test says which
directory it is on instead (`working_directory_for_test`, a path outside
`$HOME` so the `~` is nobody's either), which is the fix; the two places that
ask the process for its own directory compare against that same answer, so
they hold anywhere. The suite runs wherever it is checked out.

The git tests build real repositories in a temp directory, with one
deliberate exception: `the_committed_text_comes_from_git` reads *this*
repository's `crates/obelus-app/src/lib.rs` through `git show`, because a
diff of what git
actually has against what is on disk is the only thing that says the two
halves agree. It asserts nothing about whether that file is currently dirty.

**A test whose input is the checkout's own history only passes at one moment**
-- the same rule with a clock on it. The diff pairs are fixtures answered by
real `git diff --no-index`, and the sweep that finds new ones is `#[ignore]`d
(`obelus-app/tests/git`).

## Comments

Comments say *why*, and are worth writing where the code is right for a reason
that is not visible — an ordering that matters, a rule that fails silently, a
plausible alternative that is wrong. Don't narrate what the line does. The
existing code is the style guide; match its density.

## Not now

The diff/semantic bridge (M3) and symbol-level history (M4). Don't start on
these without being asked.

**git stays read-only.** `gix` does the reading -- head text, statuses, hunks,
blame, history, refs -- and nothing writes. Committing, pushing, pulling and
fetching were investigated and turned down: gix cannot push, and a commit made
through it leaves the index, the hooks, signing and the filters behind. What
was measured, what zed and helix do, and how to shell out if it is ever
revisited are in `obelus-git`'s crate doc, because the investigation is
expensive to redo.
