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
cargo test -- --ignored         # the slow real-server tests
OBELUS_REQUIRE_LSP=1 cargo test # a missing rust-analyzer fails rather than skips
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

**Copy starts with a capital; a name keeps its own spelling.** Everything
obelus says to the reader begins with a capital -- a key's word at the foot
(`Read it`, `Leave`), a note on the status row, an empty list's line, a card,
a tab, a setting's name and its gloss. What is *not* copy is a name, and a
name is written the way it is written everywhere else: a command (`open-file`
-- also the word in the config file's `[keys]` table), a theme (`dark`), a
language as it is counted (`Rust`, `TOML`, `Plain Text`), a file's path, and
obelus itself, which spells itself lowercase. Where a sentence would have to
start with one of those, reword it rather than misspell the name --
`Not a language obelus knows`, not `Obelus does not know this language`.

Three things are not obelus's to capitalise, and are left exactly as they
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
held to it harder. obelus runs what it is asked without asking the reader:
*the agent* asks -- that is what `session/request_permission` is for, and a
client asking again is a second question about one thing, which is the same
rule obelus's own tools follow. What obelus owes in exchange is that the
command is on the page in the words it was actually run in (not the agent's
title for it), that everything it printed is there and a failed one stays
open, and that the key which stops the agent stops the process too -- obelus
started it, and nothing else can.

**What a frame asks every frame must answer without doing the work.** The
conversation keeps its rows laid out and throws them away when what they
are made of changes -- and the two things asked on *every* frame, what is
happening now and what obelus's own commands have printed, threw them away
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

**A call's title is the command, so a few rows of it are shown shut.** An
agent titles a call with its own text and for a command that is the command
line, which is the thing a reader of a transcript of commands is reading:
one row of `grep` and an ellipsis is a row that has to be opened to be read
at all. Three rows, and the rest behind the same arrow as everything else --
because an agent that writes a script into a heredoc sends the whole script
as the title, and a closed call sat there with twenty rows of shell under a
mark saying it was shut. The same cap a card's own prose gets, for the same
reason: somebody else's text may be as long as it likes and may not push
what it belongs to off the screen.

Which is also why a row keeps the room for what it says about itself. Where
the call was, the mark saying it can be opened, how much it changes, how it
went -- all of that used to be written from wherever the words happened to
stop, so a title that reached the edge took every one of them with it. The
tail is worked out first and the words get what is left, in one list rather
than four writes in a row: the room has to be known before the words are
drawn, and what is drawn has to be the same thing that was measured.

And a place the title has already named is not said after it. `Write
src/app.rs` followed by `src/app.rs` is the rule a setting's description
already follows when it is the setting's name again -- what the title has
*not* said is what is left to say, so `line 20  +2` where it named the file
and the whole of `src/app.rs:20  +2` where it did not.

**A conversation takes one prompt turn at a time, so what the reader says
into a running one waits.** The protocol puts no turn on either end of the
exchange: `session/cancel` names a session, and the answer to
`session/prompt` says the turn is over with nothing on it saying which turn.
So two prompts in flight is two answers obelus cannot tell apart, and the
first one home put the conversation back to resting while the other turn
worked on -- no `thinking...`, no mark turning, and `interrupt` gated on the
same flag, so escape would not even send the cancellation. zed queues for
this reason too, and "send it now" there is `cancel` awaited and *then* the
prompt, never the two at once.

What is waiting is the reader's, so it is on screen (over the box, where the
way back to the end goes) and it is theirs to release: enter on an empty box
stops the turn in front of it, which was the one keypress in a conversation
that did nothing at all. Stopping the turn themselves holds it back instead
-- sending what they typed the moment the thing they just stopped comes to a
halt is obelus speaking for them straight after they said not to -- and the
next thing they send picks it up again, in front of nothing: a queue that
let a later message overtake an earlier one would put their own words to the
agent back to front.

**So obelus numbers its own turns**, because the protocol will not: the
number goes out with the prompt, comes back on the answer, and an answer
about a turn that is not the one running is dropped where the count is
kept. It replaced a flag that said only "given up on", which the next
prompt cleared -- so the cancelled turn's own answer, which a well-behaved
agent sends because the protocol tells it to, was delivered after all and
ended the turn that had replaced it. zed numbers them too, and has a test
whose name is this paragraph.

The queue is what makes two turns rare; the number is what makes the rare
one harmless. Both, because the first is obelus's own discipline and the
second is about what arrives.

**And a turn the reader stopped takes its calls with it.** A tool call's
state is the agent's, and an agent told to stop is *asked* to send the
updates it owes -- one that never saw the cancellation sends none, and its
calls sit at `in_progress` for ever under a conversation obelus has said is
resting. Worse than wrong: nothing wakes the screen for a conversation that
is not working, so the mark on that row is a spinner frozen mid-turn.
`cancelled` is the protocol's own word for a call stopped before it
finished, so writing it down is obelus saying what the agent would have
said, not inventing a state of its own. The exception is a call obelus is
running the command for, whose state comes from the runner every frame and
is not obelus's to overwrite.

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

**A list long enough to be worth searching is long enough to be worth
threading.** The history used to ask for two hundred commits on the main
thread. Both halves of that were wrong, and wrong together: the bound made
the key cost 346ms on a busy file and 674ms on a rarely-touched one in a
40,000-commit project -- a visible freeze -- and it also made the query lie,
because rows that were never fetched are rows a query cannot match and the
reader is told "no match" either way. Walking the whole history on a thread
costs about two seconds there and nothing at all at the key, so the bound
had nothing left to buy. A limit on a list is a limit on what can be found
in it: reach for a thread before reaching for a number.

**A key that opens a thing may only close that thing.** `alt+d` toggles a
hunk by closing "the block in front of the reader" -- and in a commit's
version the block in front of the reader is usually the commit's message,
which is what they opened that version to read. It closed it. Blocks carry
a `kind` for exactly this: ask it. And a line has room for one block, so a
hunk on the first line of a commit's version has nowhere to go -- say so,
because the margin says that line changed and a key that answers nothing
looks broken.

**An answer on screen is an answer about a moment, and the moment passes.**
A history is read at a `HEAD`, and the reader commits in another window,
amends, checks something out. The watcher already reports what git writes,
so the list reads itself again -- but only when `HEAD` actually moved:
`git add` writes the index on every use and changes no commit, and
re-reading on that would throw away a walk in progress for nothing. Keep
the commit the reader was on, not the row they were on: a re-read history
is the same history with rows added on top, and row seven is a different
commit afterwards.

**What obelus writes, obelus has to be able to read.** The log gained a
process id at the front of every line so that sessions running at once
could be told apart; the reader of that format was not told. It splits on
the first space expecting a timestamp, got a number, refused every line,
and the file obelus writes was the one file it could not give a reading --
so `ctrl+t` was greyed out on it. A format with a writer and a reader in
the same program has a test that the one reads the other, or they drift and
the symptom turns up somewhere that looks unrelated.

**Ask the question you mean.** A gate that wants to know *whether* anything
in the project has changed was building a map of every changed path and taking
its length -- on every command in the palette. The history already makes
this distinction (`has_any` asks for one commit), and now the project does.
Measured afterwards, and honestly: it is two and a half times cheaper on a
project with something in it and no cheaper at all on a clean one, because the
walk has to reach the end to find nothing. The phrasing was wrong; the cost
lives in the walk, and saying otherwise would have been a win claimed
rather than got.

**Do not delete what you do not recognise.** Obelus wrote its own settings
file whole, on the grounds that obelus wrote all of it. That is not true --
readers put lines in by hand -- so writing it whole silently took out
everything obelus did not know: a setting from a newer version, a key
renamed since, a line with a typo in it, and the comment beside them. It
took them out on the next switch the reader flipped, which is nowhere near
where they would look. A project's file was already edited rather than
rewritten, with `toml_edit`, for exactly this reason; the reader's is now
too.

**Ask the same question of every layer.** The column saying where a value
came from asked the project "does your file name this setting" and asked the
reader "does your value differ from the default". So a reader who wrote a
setting down and happened to agree with obelus was told they had never been
here. Both answers were available -- `apply` returns the keys a table set --
and one of them was being thrown away.

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

**Two buffers can wear one path, so a path alone cannot say which.** A file
and that file as some commit had it live at the same path, and `read_at`
matching on the path alone previewed one at the other's place in it. Match
on the path *and* on which version it is -- `content().at()`.

**A view is split by the errand, not by the shape of the answer.** `f9` and
the refs tab both end in the same thing -- a version of the file being read,
one found by time and one by place -- so the arrow between them stays inside
one errand. The project's commits are the odd one out: its rows stop being
about the file on screen, and what hangs under them is somebody else's
files. That is a key of its own (`f10`), not a third tab. The cost is real
and was taken deliberately: obelus's commands do not run from inside a list,
so reaching the project's history from a file's is escape and then `f10`.

**Two buffers can wear one path, so a list of them says which is which.**
The file and the file as some commit had it differ in what they say, in
whether they follow the disk, and in what the margin beside them means; two
rows reading `src/parser.rs` are two rows a reader picks between blind. The
short id on the right and no more -- the status row marks the same fact in
the same words, so a reader who has seen one has read the other, and the
list's own job is still to show paths.

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
opens as its *bytes*, whatever reading it has, and `ctrl+t` asks for the
reading; the bytes are where the cursor, the selection and the copy live.

It used to open in the reading when it had one, under a setting that was on
by default. Which reading a file has is the file's own business; whether to
be shown one *instead of the file* is the reader's, and a program whose
whole subject is what is in a file should not answer that for them. The
setting went with the behaviour, because a switch that turns off something
nothing does is a switch with nothing behind it -- and an old config naming
it is simply ignored, the way any key `from_toml` does not know is.

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

**What is highlighted is what is drawn.** `visible_bytes` walks past folded
runs the way the view does, rather than counting `height` lines down from the
top. It is the same arithmetic as everywhere else here, and getting it wrong
is not subtle: with a two-hundred-line run closed at the top of the screen,
every row below it is a line two hundred further down, outside the range that
was highlighted, and the whole of the rest of the screen is drawn in the
plain foreground.

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
  binding that works on one machine and not the next. `f9`-`f12` are git's,
  and empty until each is earned -- a file's history and a project's, a
  patch to review, a panel of what a commit touched.
* **Control does something to the file in front of you**, on the letter of
  the word: `p` the palette, `w` close, `r` re-read, `t` toggle the reading
  its format has, `l` a line number, `a` all of it, `c` copy, `q` leave.
  `ctrl+v` was left alone until there was a paste to give it, and
  that is the one chord every reader will try there.
* **Alt asks about the cursor, or walks what was found**: `alt+enter` the
  symbol under it (an IDE's context actions, and alt is the escape prefix so
  it arrives everywhere), `alt+d` its diff, `alt+b` its blame, `alt+f` the
  run of lines it is inside, `alt+m` its matching bracket, and the arrows --
  up and down between changes, left and right through the places the reader
  has been.
* **Shift never names a command.** It only extends (`shift` plus an arrow)
  or reverses (`shift+tab`), which leaves it meaning one thing everywhere.
  **Escape always gives up on the nearest thing**, and everything else is
  reached from the palette: a chord for every command is how a key table
  stops being memorable.

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

**The wheel moves the view; the keys move the cursor.** A notch scrolls what
is on screen and leaves the cursor where it was -- `scroll_by` on a buffer
moves the viewport and nothing else -- and the paging keys move the cursor by
a screenful, with the view following it. Two gestures, two jobs: a reader
spinning a wheel is looking around, and one pressing a key is going
somewhere. A list is the exception that proves it: there a notch steps the
selection, because a list's view *is* its selection and there is nothing
else in it to scroll.

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

**The main loop is threads and one channel, not a runtime.** One
`std::sync::mpsc` channel, one producer thread per event source (keyboard, file
walk, watcher, each server's stdout), the main loop blocking on `recv()` and
draining with `try_recv()`. Not a rule against `async` or against tokio --
tokio is in the project and `acp::link` runs a current-thread runtime on a thread
of its own, because the protocol's crate is built around it. What the rule is
about is the loop: one owner of `&mut App`, and no `.await` between a key
arriving and the screen it produced.

Writing to a server's stdin needs its own thread, because a busy server stops
draining the pipe. An answer that arrives after the world has moved on is the
normal case, which is why requests record the version they asked against.

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

## Shape

```
src/
  app/            state, the loop's handler, and every picker's item source,
                  by aspect: documents, moving, searching, choosing, agents
                  · a commit's message hangs above its file (history); a
                    history is one view at two radii (history_view); a
                    preview is of a subject, not of a path (previewing); a
                    project may carry settings, and it is not the reader,
                    and the reader's is the layer it is laid over
                    (preferences); what obelus says before the reader's
                    first words is one piece that is always said and one
                    the topic adds, and the reader's own words go into a
                    template last (opening)
  text.rs         the Rope wrapper: the only place coordinates convert
  buffer/         one open file: text, syntax, cursor, viewport
                  · an edit knows where it happened; do not read over an
                    edit (mod); a folded line has no rows, what folds comes
                    from the indentation, a hunk opens where it is, a fold
                    across an edit is not one across a re-read (folds);
                    undo groups by what the reader was doing (undo)
  keymap.rs       chords, contexts, the default table, modifiers_of
                  · `why_not` is the one judgement of what may be bound;
                    keys are rebound on the keys page; modifiers are judged
                    exactly, in one place
  icons.rs        the Nerd Font switch and every glyph behind it
  config.rs       the settings, their file, and what each one is
                  · what a project may set is a property of the setting; the
                    file holds preferences, not state
  logging.rs      two logs split by module, and where a panic goes
  command/        the Command enum, its table, groups, and dispatch
  component/      picker (one component, several instantiations), settings,
                  the conversation, the box a message is written in, and the
                  window every list shares
                  · a query is about the rows the list is of, whether it
                    ranks is settled per tab, a list still arriving sits
                    still, say "still reading" where it moves nothing,
                    which tabs a view has must be cheap (picker); a list
                    obelus offers is the reader's own project (picker/files);
                    a question is a card, not a picker (card); a tool call
                    is somewhere to go, the transcript's cursor stands only
                    on rows that do something, a run of tool calls is one
                    row, thinking is not folded away (chat); a setting is
                    two rows, and a name and a gloss (settings)
  counts.rs       how much code is here: tokei's walk, in the two orderings
                  the view reads it in
  reading/        what a file is when it is not code: markdown, a log
                  · a log's format is decided by its lines, not its name
  syntax/         language registry (14 languages), parsing, highlights, tags
  lsp/            transport, client, actions, positions, outline
  git/            gix, reading only: head text, statuses, hunks, blame, history
                  · the diff base is the blob a checkout would write, and
                    reading it must not run anything (mod); a blame is about
                    a version, and the margin knew which commit (blame);
                    what the remote has not seen is marked (history)
  agent/          the ACP registry, installing an agent, its marks
                  · an agent is installed when the install says so, in
                    writing (install)
  acp/            the protocol, through its own crate, and the thread that
                  joins it to the loop
                  · why the runtime is current-thread, why the two
                    directions are not symmetrical, the one ordering the
                    protocol does not promise, what waiting on the reader
                    costs the whole connection, and what an agent asking
                    something may ask for (link); an agent that stopped is
                    started again by talking to it (mod)
  ui/             editor, status bar, picker, settings, chat, welcome,
                  images, shared cell writers
                  · what the bar measures is what is shown, the caret can be
                    in the block, a bar is a block, a column a file might
                    need is reserved for the whole file (editor); everything
                    that scrolls says so (mod); a header says what a thing
                    is, the foot says what is happening (chat)
tests/            integration tests plus tests/fixtures/*.txt golden grids
                  · why the fake agent is `sh`, and what it checks back
                    (agent)
```

A `·` line is the rules that live in that module's own doc rather than here,
by the sentence they open with. They are there because they are read at the
moment they matter -- and listed here because a rule you only meet by opening
the file is a rule you break while deciding not to open it.

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

**The agent protocol comes from its own crate, joined to the loop on one
thread.** `agent-client-protocol` is the reference implementation and it is
built around `async`; `acp::link` is the join, and it is the only place in
obelus where a runtime exists. Its module doc has the rest: why the runtime
is current-thread, why the two directions are not symmetrical, and the one
ordering the protocol does not promise. `tests/agent.rs` drives a real
process at it -- `tests/fixtures/fake-agent.sh`, which also checks obelus
kept the promises it made in the handshake.

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
screen. What is left of that band is `raised_background`, one shade off the
page, behind the cap a key is drawn in at the foot of a view and behind the
card that lists every key -- a few cells wide, and a box, never a row.

A preview's margin comes from git, so a fixture that shows one depends on
the fixture file being *committed*: edit `tests/fixtures/long.rs` without
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

**git stays read-only.** `gix` does the reading -- head text, statuses, hunks,
blame, history, refs -- and nothing writes. Committing, pushing, pulling and
fetching were investigated and turned down; the investigation is worth keeping
because it is expensive to redo.

gix has no push at all: `gix-transport` knows the name `git-receive-pack` and
nothing in `gix` ever asks for it. It *can* commit -- `edit_tree` builds the
tree, `repo.commit()` writes the object and moves the ref -- and doing so
leaves the index untouched, which is not a cosmetic problem: after a commit
made that way `git status` reports the newly committed file as deleted, and the
reader's next ordinary `git commit -a` removes it from history. Two lines
(`index_from_tree` then `index.write`) fix that, and four more things stay
broken: a `pre-commit` hook that exits 1 does not stop it, `commit-msg` never
runs, `commit.gpgsign` is ignored, and `.gitattributes` filters are not applied.
All measured against a real repository, not read off the documentation.

Which is why nobody does it. zed writes -- and has no git library at all: 21
subcommands shelled out, blame and diff included, plus its own `GIT_ASKPASS`
script talking back over a socket. helix reads -- and uses gix, with no network
feature and no git commands whatsoever. There is no third combination. The
choice is not which library; it is whether to write at all, and obelus does not.

If that is ever revisited: shell out for all four verbs, because one of them
(push) has no other option and two mechanisms for one act is worse than one.
`GIT_TERMINAL_PROMPT=0` fails cleanly without touching the terminal;
`GIT_ASKPASS=<program>` is called once per credential with the prompt as
`argv[1]` and the answer read from stdout, which is how a TUI asks for a
password without losing the screen. Both measured.

Also waiting on a configuration file, which does not exist: the Nerd Font
switch, user theme colours, the server table, and the word-wrap toggle all
want one.
