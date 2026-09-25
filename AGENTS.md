# Obelus

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
cargo run -- src/app.rs               # ob, in this terminal
cargo run -p obelus-gui -- src/app.rs # obg, in a window
UPDATE_FIXTURES=1 cargo test    # regenerate golden cell grids
cargo test -- --ignored         # the slow real-server tests, and the diff sweep
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

**Nothing is polled.** What to do about one of these is either to ask a
cheaper question or to be told, and there is no third answer. **Ask a
cheaper question**: whether there is *anything* to take up is the size of a
file (285ns) rather than what is in it, because Obelus writes that file
whole and writes it empty when it has nothing to say -- which leaves the
gate a shade generous, so the list it opens says what it actually found.
**Or keep it and hear the change**: which note has a conversation is a
table somebody *writes*, and a write is something a watcher hears, so it is
read when the page opens, when the watcher says so, and when Obelus writes
it -- and `sessions::change` hands back what it wrote, so that last one
re-reads nothing.

The one that looked like a third answer was the claims. A claim is a lock:
taking one writes nothing, and an Obelus that is killed gives its lock up
with nothing on disk to say so -- which reads as "nothing can tell you, so
keep asking", and was a walk of the claims directory on every frame the
notes were showing. Something does tell you. The kernel closes a dying
process's files, and a watcher reports a file closed by a process that had
it open for *writing*. So a claim is **held** by a writer, on purpose, and
**looked at** through a read, on purpose -- because a look that announced
itself would be Obelus waking itself to look again, for ever. `obelus_watch`
lets that one Access event through and refuses the rest; `chats` opens for
reading everywhere but `claim`; `flock` allows it, where `fcntl` locks would
not. Being refused a claim counts as being told, too, and is the freshest
news there is: the row the reader pressed goes dim under them.

That leaves three moments a thing is read: when the view that shows it
opens (a watch says what happens next, not what was already there), when
the watcher says so, and when Obelus is the one who changed it.

Which is not the same as the three moments a *watch* is taken.
`App::settle_the_watches` holds all of them and is asked from what is open,
once a frame; the reading is the view's own, done as it opens. They were
one thing briefly and it was wrong twice over: an Obelus whose watcher
would not start read nothing at all for the rest of the session, and a view
whose keys act before the next frame -- the notes are opened by one key and
talked about with the next -- got its answer a keystroke late. So `Watched`
keeps two fields, what is *wanted* and what is *held*, and only the first
decides when to read.

The test for which shape a thing wants is not how dear it is. It is whether
there is anything that could tell you it moved -- and the answer was yes
in the one place it looked like no, so the question is worth asking twice
before settling for a poll.

One was left alone, which is the other half of the rule. The agents page
reads an install record per agent, three times a frame between the page,
its window of cards and its pictures: forty misses measured at 51us, on one
page, that nothing animates -- so it is 150us on a keystroke nobody is
waiting on. A number is a reason to look; it is not on its own a reason to
add machinery.

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

**Opening a conversation starts nothing; saying something does.** A view is
somewhere to look, and looking is not talking. `f4` and the notes' `Talk`
used to run the agent's process and mint a conversation on it before the
reader had typed a character -- so a key pressed to see what was said
yesterday started a node, and left behind an empty conversation the agent
does not keep and Obelus could still write down against the note in place
of the one the reader had been talking in. There are two moments a session
is asked for now: the reader's first message, where `App::say_in` starts
the process and the handle holds the words until there is somewhere to send
them, and their choosing one from the list to take up again. The claim is
still taken when the view opens, because the claim is not about the agent
-- it is this window saying the note's conversation is its own, and that
has to be said before another window says it.

Which makes `Talking::Idle` a state a reader sits in rather than a blink,
so it needed words of its own: `starting...` under a still mark, about a
conversation nothing is starting, is the one thing that row must not say.
And "starting" has to be told from "nothing has been asked for", which used
to be the same thing -- `Conversation::opening` is the half of that
`asked_for` cannot carry, because a fresh conversation has no name until
the answer brings one.

**What a reader said about a project outlives the window they said it in.**
The agent keeps every word and Obelus keeps the one thing it cannot --
which conversation is which -- so `f4` inside a conversation is a list of
them, newest first, over the one they are in. Ordered by when something was
last said in it, which is a field (`Kept::last`) because it cannot be
worked out from anything else, and written in the same words a commit's row
uses (`how_long_ago`).

**A row of a list says what is true now, and says it in one answer.** The
rows of a list are a snapshot -- building them reads files and walks git,
which is not work a frame can do -- so what changes under the reader while
the list is up is asked again instead of rebuilt: `Picker::remark`, which
the list of open documents already used for the mark that turns while an
agent works. What it carried was the mark alone, and the list of
conversations needs two things -- the lock, and whether the key works on
that row -- which are one fact wearing two faces. So a remark carries
`Said`, both together: a list that could refresh one without the other is a
list that draws a lock on a row and lets the reader into it anyway, which
is the same shape as the card whose `submit` row stopped saying the keys
were on it.

And the waking is the other half. A claim is a lock, and a lock is
invisible to a watcher -- nothing is written when one is taken, which is
the whole reason the claim has a file -- so the directory of them is what
one Obelus wakes another on, and without a watch on it the rows would be
asked again only when the reader happened to press something. The watch is
counted, so the notes page and this list can hold it at once; which of them
holds it is decided every frame from what is showing, rather than switched
on where a list opens and off in each of the ways it closes. A watch
switched on in one place and off in three outlives its reason the first
time somebody adds a fourth way out -- the ticker's rule, one level along.

**A conversation belongs to the agent that had it.** A session id is a name
one agent minted and means nothing to another, so only the agent in use can
be asked to take one up. The others are shown all the same -- a tab each,
their rows dim, the reason above them -- because the alternative is a
reader who changed agents finding their conversations gone and nothing
saying where. A tab exists only where that agent has something in it, so
the ordinary case of one agent has no tab row at all; the tabs are scopes
and not groups, because a picker's group tabs come with an `All` in front
of them and `All` is the one tab this list must not have -- it would mix
the rows that can be taken up with the rows that cannot. Nothing here
switches the agent: that is a setting, and doing it from a row would drop
the session of every conversation open, including the one the reader is
standing in.

**A conversation takes one prompt turn at a time, so what the reader says
into a running one waits.** The protocol puts no turn on either end of the
exchange: `session/cancel` names a session, and the answer to
`session/prompt` says the turn is over with nothing on it saying which turn.
So two prompts in flight is two answers Obelus cannot tell apart, and the
first one home put the conversation back to resting while the other turn
worked on -- no `thinking...`, no mark turning, and `interrupt` gated on the
same flag, so escape would not even send the cancellation. zed queues for
this reason too, and "send it now" there is `cancel` awaited and *then* the
prompt, never the two at once.

**What is waiting is the reader's, so it waits where their words live.** It
goes straight into the transcript, dim, one row per thing they said, and
enter on one takes that one back into the box. It was a count over the box
-- `2 waiting` -- which said how many and never which, and offered nowhere
to stand to change their mind; and the key that released it was enter on an
empty box, one key that was harmless with words in the box and a stop to a
running turn without them, a pair of presses apart.

**And it goes as one prompt, not one per turn.** Three things typed into a
running turn are one thing the reader is saying -- fix the tests, and the
lint, and then commit -- so they are joined with a blank line, which is what
the box's own `alt+enter` makes. One per turn meant the agent answered the
first without ever seeing the second, and the third did not reach it until
two turns had run. The rows stay the rows they were: the page is what the
reader said, and Obelus adds to their half of it rather than rewriting it.

Stopping the turn releases them. Escape means "stop what the agent is
doing", not "unsay what I said" -- it used to mean both, because the words
had been taken off the page into a queue and Obelus sending them unasked
would have been Obelus speaking for them. They are on the page now, and
taking one back is a key on the row it is about. They still go in the order
they were typed: a queue that let a later message overtake an earlier one
would put their own words to the agent back to front.

**So Obelus numbers its own turns**, because the protocol will not: the
number goes out with the prompt, comes back on the answer, and an answer
about a turn that is not the one running is dropped where the count is
kept. It replaced a flag that said only "given up on", which the next
prompt cleared -- so the cancelled turn's own answer, which a well-behaved
agent sends because the protocol tells it to, was delivered after all and
ended the turn that had replaced it. zed numbers them too, and has a test
whose name is this paragraph.

The queue is what makes two turns rare; the number is what makes the rare
one harmless. Both, because the first is Obelus's own discipline and the
second is about what arrives.

**And a turn the reader stopped takes its calls with it.** A tool call's
state is the agent's, and an agent told to stop is *asked* to send the
updates it owes -- one that never saw the cancellation sends none, and its
calls sit at `in_progress` for ever under a conversation Obelus has said is
resting. Worse than wrong: nothing wakes the screen for a conversation that
is not working, so the mark on that row is a spinner frozen mid-turn.
`cancelled` is the protocol's own word for a call stopped before it
finished, so writing it down is Obelus saying what the agent would have
said, not inventing a state of its own. The exception is a call Obelus is
running the command for, whose state comes from the runner every frame and
is not Obelus's to overwrite.

**A server's running commentary is not news; that it is running is.**
`rust-analyzer` sends a few hundred progress messages over a cold start --
every crate scanned, every file indexed -- and each landed on the status row
between the file's name and the cursor's position, which are the two things
a reader looks at that row to read. What those words were really for is one
bit: an empty answer while a server is reading the project and an empty
answer about a symbol with no definition are the same message on the wire,
and the row is the only thing that tells them apart. So the badge that names
the server turns while it is busy, in the same braille everything else in
Obelus turns in, and the words stay in the log.

Which needed the ticker woken for it, like every other mark that turns --
one drawn once and never again is a mark saying nothing is happening. And
`ServerState` gained no variant for it: "busy" is not a state beside
`Ready`, it is a thing a ready server is doing, and a fourth variant would
have had `f10`'s "rust-analyzer is not answering" said about a server that
was answering fine.

Of what a server says in words, only what it calls an error reaches the
reader. All four kinds went to the log at `debug`, which for the loudest is
the wrong place: a workspace it could not discover is why every question for
the rest of the session comes back empty. The other three are a diary, and
the log is what a diary is for.

**Folding is one act, and the notes are the fourth place it happens.** A run
of lines in a file, a run of tool calls in a transcript, a commit's files in
a list, and now what hangs under a note: one row standing in for several,
`alt+f`, and the same arrow. So `Command::Fold` asks whichever document is
being read rather than the file always -- which is what it did, folding a
run of lines behind the notes that the reader could not see. `Requires`
follows it, because the palette and the key are one judgement.

What folds is what hangs *under* a note, never a note's own lines: a list
showing a third of each note is a list a reader has to open one row at a
time to read, which is what they opened it to avoid. Which notes are shut is
this session's and is kept by *name* -- `todo.toml` is a file another window
is writing, and a set of positions belongs to whichever order the notes were
in when it was made.

The arrow gets a column of its own, always, whether or not anything on the
page folds. The counts can spend theirs only when something does; a note's
words are wrapped to what is left of the row, so a column that came and went
would re-wrap the page the moment a reader put the first note under another.

**A box is the reader's once they have put something in it, not because
it exists.** A reread keeps the box the caret is in, so that somebody
else's write cannot take back a word the reader has just typed. But the
notes open with the caret already in a note -- there is no mode to get into
-- so a box exists before anything has been typed into it, holding this
page's own copy of what the note said. Kept over the file, that copy is
this window saying the other window's change did not happen: the note a
reader happened to be standing on was the one note no other window could
ever change under them, and since a page opens on the first note, that was
usually the note they were both looking at. `TodoView::reread` asks whether
the box still says what the page's copy said -- the same question it
already asked about a note the other window *deleted* -- and where it does,
takes the file's words and puts them in the box.

Both halves need a test, because each passes with the other broken: one
window's write arriving in the other, and the reader's unwritten words
surviving a write to the very note they are in. Neither had one, which is
how this lasted.

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

**What Obelus writes, Obelus has to be able to read.** The log gained a
process id at the front of every line so that sessions running at once
could be told apart; the reader of that format was not told. It splits on
the first space expecting a timestamp, got a number, refused every line,
and the file Obelus writes was the one file it could not give a reading --
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
file whole, on the grounds that Obelus wrote all of it. That is not true --
readers put lines in by hand -- so writing it whole silently took out
everything Obelus did not know: a setting from a newer version, a key
renamed since, a line with a typo in it, and the comment beside them. It
took them out on the next switch the reader flipped, which is nowhere near
where they would look. A project's file was already edited rather than
rewritten, with `toml_edit`, for exactly this reason; the reader's is now
too.

**Ask the same question of every layer.** The column saying where a value
came from asked the project "does your file name this setting" and asked the
reader "does your value differ from the default". So a reader who wrote a
setting down and happened to agree with Obelus was told they had never been
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
files. That is a key of its own (`f10`), not a third tab. It used to cost an
escape -- Obelus's commands did not run from inside a list, so the project's
history was escape and then `f10` -- and that went when a view's key started
reaching it from inside another view (below): `f10` from a file's history
goes straight to the project's, and the two stay two views.

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
  patch to review, a panel of what a commit touched. Which is also why one
  reaches its view from inside another: every view it names takes the whole
  screen, so going to it is a swap and never a stack.
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

**Except a key that names another whole view.** What the rule above prevents
is a view opened *over* a view. A key whose command takes the whole screen
(`Command::opens_a_view`) does not do that from inside another whole-screen
view: `App::switch_view` leaves the one showing the way escape would -- which
puts back what it changed -- and opens the other, so there is still one thing
on screen and one escape back to the file. Where the key names a tab of the
view already showing (`f3` in `f1`'s list, `f6` in the search) it walks to
the tab and the query stays. A list over the file rather than instead of it
-- the palette, a menu -- keeps its keys: it is somewhere the reader is
choosing, and a function key is not a way out of it. The card of every key
was `f1` until this, and moved to `ctrl+k` (`keymap::keys_card`) because `f1`
names the files.

And a view that has bound the key itself beats the swap. `App::handle_key`
asked the swap first, so a key the showing view had taken was answered one
level out and its own binding was dead: `f4` in a conversation swapped the
conversation for itself. `Keymap::bound_here` is that question -- what this
context binds, with no falling back -- and it is the same precedence
`Keymap::lookup` already uses. Which is what `f4` inside a conversation is
for: it opens the conversation from anywhere else and means *which one*
inside the one it named, the same move `f3` makes in `f1`'s list. So
`talk-to-agent` is bound in `Context::Normal` rather than everywhere: the
only two contexts with global keys are Normal and Chat, and Chat has its own
answer now.

**One mark for "the keys are here", and it says nothing else.** Every list,
page and card in Obelus puts `selected_row_background` behind the row the
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
prefix and always arrives. Nothing else in Obelus depends on the protocol.

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

**Obelus is drawn on two things, and the loop knows neither.** `ob` is the
terminal and `obg` is a window -- gvim's relation to vim, not a second
program: the same grid, the same `component/` and `ui/`, the same `App`.
What the window is for is the two things a terminal cannot give. It cannot
tell `ctrl+i` from `Tab`, `ctrl+m` from `Enter` or `ctrl+[` from `Escape`
-- one byte each -- and most terminals send nothing at all for
`ctrl+shift+X`, so the key table is written around what will get through.
And it draws with the font the reader installed, which is what
`icons::NERD_FONT` is: Obelus guessing about somebody else's machine. In
the window the presses arrive as themselves and the marks are compiled into
the binary.

The seam is one trait and one channel. `ratatui::backend::Backend` is where
a screenful of cells becomes escape sequences or becomes quads on a texture,
and `app::run` is handed the *receiving* end of the loop's channel because
the other end belongs to whichever front end is running: the terminal reads
keys on a thread of its own, and a window gets them from the event loop a
platform obliges it to run on the process's first thread. Which is why
`App` crosses a thread in `obg` and stays there -- one owner of `&mut App`,
as before, just not on `main`'s thread. What a send needs there and did not
need in a terminal is a wake: a thread parked in `recv` wakes because
something was sent, and a thread parked in the platform's own wait does
not, so the backend holds the window's proxy and pokes it when a frame is
done.

A key is still a `crossterm::event::KeyEvent` everywhere inside Obelus,
including inside `obg`, which has no terminal in it at all. The window
translates its own presses into that type in one file (`keys.rs`), and the
twenty-odd files that read a key never hear that there are two front ends.
The same trick is not available for what a *window* can say and a terminal
cannot: closing one is the reader asking to leave, so it is an event
(`Event::Closed`) that goes the same way the key that leaves goes, question
about unwritten files included -- and the window stays open until the
application says it is done.

What is not shared is the drawing. `obg` has no images (the three terminal
picture protocols are a terminal's, and the glyph is what a window draws),
and its cells are painted by one pipeline: a quad per run of background, a
quad per glyph, a quad for the caret, all from one texture. The colours go
through untouched, which is why the surface is viewed without its sRGB
conversion -- a theme's `#1e1e2e` is the colour the reader picked, and a
pipeline that corrects it draws a different one.

Nor is every setting shared. A terminal draws with the font the reader gave
the terminal and a window draws with its own, so `font_size` means nothing
in one and the glyph switch means nothing in the other -- and a row shown
where it does nothing is worse than a missing one, because the reader
changes it, watches nothing happen, and has learnt something untrue. So a
setting says where it is `Drawn`, the page shows the ones that apply, and
the *file* keeps them all: the other Obelus on that machine is the one they
are for. What a front end owns, it is told about -- `App::drawn_by` and one
method, called when it says who it is and again after every change, which
in `obg` goes down the frames channel like everything else about what is on
the screen.

A full-width character is two columns of the grid and one glyph, and the
second column is a cell `ratatui` has *reset*: no text, no colours. A
terminal never draws it, because the terminal advanced two columns itself.
A window draws every cell, so it has to be told -- `Look::columns` -- or it
paints the default background behind the right half of every Chinese
character. The glyph itself is left the size the face drew it and centred
in its two cells, scaled down only where it does not fit: cosmic-text's
`monospace_width` closes the gap the other way, by growing the glyph a
fifth until its advance is a whole number of cells, and that is a line of
Chinese visibly larger than the Latin above it.

**The caret's shape says what the next character will do.** A bar stands
between two characters and says the next one goes in there; a block stands
on one and says the next one takes its place. So there are two shapes and
they follow the mode: `Insert` toggles typing over, which is the one
binding outside the three key families and is there because that is what
the key is called. The shape is asked of the application on every frame --
it depends on where the caret ended up, which is not known until the frame
is laid out -- and only a window can draw it, because a terminal's caret is
the terminal's and Obelus does not own its shape. Which is why the status
row says `Replacing` in a word as well: that half works in both, and a mode
with no sign is a mode the reader is in without knowing.

Only in a document. A box the reader is typing into is one short line --
a query, a message, a note -- where typing over means nothing, so the
caret there stays a bar whatever the mode says. A caret that claimed
otherwise would be the one thing a caret must not be, which is wrong about
what the next key does.

**What an input method is spelling is on the page, and is not in the
file.** Typing Chinese is spelling a word before it exists: several
keypresses that are not characters, and then one character that is. The
window draws that spelling itself, over the cells to the right of the
caret, underlined, in the colours of the place it is going into -- and the
application is never told about it. Not because it could not be: because
what would arrive is half-typed pinyin in a buffer with an undo history and
a file that has changed. What arrives instead, when the input method
commits, is a paste -- which is the same question Obelus already answered
about where typed text goes when several things are on screen.

The keys during that spelling belong to the input method, so a plain
character is swallowed while one is being composed: it is arriving twice,
once as the spelling and once as the word. A chord is not -- `ctrl+s` means
save whatever is being typed.

**Obelus does not split its window, so several Obelus processes is the
normal case.** A terminal already splits, tiles and tabs better than an
editor can from the inside, so Obelus has one region and no panes. What that
buys has to be paid for on the other side: two or three of them on one
project, plus the reader's own shell in the same repository, is how Obelus is
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
moment.* `save_to` wrote in place, which truncates first; a second Obelus
reading in that gap got an empty file, took it for "no settings", and wrote
its defaults over everything the reader had. It writes beside the file and
renames over it now -- the one filesystem operation with no gap in it -- and
reading distinguishes "there is no file" from "there is a file Obelus cannot
read". The second stops Obelus writing at all: what is in that file is the
reader's, and saving over something it could not read replaces settings it
never saw. It says so on the status row and starts saving again the moment
the file reads, which the watcher notices.

*Anything cached about the world must be dropped when the world moves.* What
has changed in a file is a question about the file *and* about the commit it
is compared with, and the cache was keyed only by the file: a commit in
another window left the margin drawing a diff against a commit that was no
longer the one the file is against. `HEAD` and `index` are watched, and
`App::forget_what_git_said` drops the hunks and the blame when either moves.

*A repository and its worktrees are one project.* The notes are about the
code and the code is the same code, so a reader with three worktrees open
means to come back to one list -- and what Obelus keeps about a project is
keyed by `common_dir`, which is git's own answer to which repository this
is. `obelus_git::project` is that key and everything using it must use the
same one: the notes and the table saying which conversation is about which
note were keyed apart once, and one note with two conversations under it is
a reader opening it in one worktree and finding an empty page in the next.
Canonicalised, because gix hands `common_dir` back untidied -- a linked
worktree answers `.../.git/worktrees/one/../..` -- and because on Windows
that is what turns the spelling the reader typed into the one the disk has.
The notes moved out of the project's own `.obelus` for this: they were never
shared with the next person anyway, since `.obelus` is a directory readers
gitignore.

*A conversation is not a thing two of them may have open at once.* The agent
takes one prompt turn at a time and the queue that keeps Obelus to one lives
in a process, so a second process prompting the same conversation walks
straight past it -- no `2 waiting` anywhere, because neither Obelus can see
the other's queue. So a conversation is claimed, and the claim is a
lock the system holds rather than anything Obelus writes down: an Obelus that
is killed, crashes or loses power gives it up without being asked, which a
process number in a file cannot do -- it has to be believed, checked against
a process that may be somebody else's by now, and given a staleness nobody
can pick. The claim has a *file* as well, created and removed with it,
because a lock is invisible to the watcher: nothing is written when one is
taken, so the file is what another Obelus wakes on. The file's existence
means nothing on its own -- one left behind by a process that died is a file
nobody holds -- and asking for the lock is what says which it is.

Nothing is said on the status row when the key is refused. The lock beside
the note says it, and the foot says it again by not offering `Talk` there:
the reader is told before they press, which is the rule the palette follows
for a command it will not run. The list of conversations says the same
thing the same way -- the lock in the marker column, the row dim -- and
asks for the claim *again* when the row is chosen, because the list was
built a moment ago and another Obelus may have walked in since. Where it
has, the list stays open and the row goes dim under the reader, which is
the answer; nothing happening at all is a key that looks broken.

*And what is claimed is the conversation, not the note.* `ChatId` is which
one: a note where there is one, and the agent's own name for it where there
is not. It has to be the note for a note's conversation -- the notes page
reaches one before a session exists, which is why the claim is taken before
the conversation is opened -- and it has to be the session for the other
kind, because nothing else names one. Two keys for one conversation would be
two doors with an Obelus behind each.

Two smaller ones, in the same spirit. An install claims the agent's directory
with a file created exclusively, so two windows asked for the same agent do
not run two `npm`s into one prefix; the claim is given up by being dropped,
and one left behind by a killed process is taken over after ten minutes. And
every log line carries the process's number, because several Obelus
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
                    (preferences); what Obelus says before the reader's
                    first words is one piece that is always said and one
                    the topic adds, and the reader's own words go into a
                    template last (opening); a conversation belongs to the
                    agent that had it (conversations)
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
                    Obelus offers is the reader's own project (picker/files);
                    a question is a card, not a picker (card); a tool call
                    is somewhere to go, the transcript's cursor stands only
                    on rows that do something, a run of tool calls is one
                    row, thinking is not folded away (chat); a setting is
                    two rows, and a name and a gloss (settings)
  counts.rs       how much code is here: tokei's walk, in the two orderings
                  the view reads it in
  reading/        what a file is when it is not code: markdown, a log
                  · a log's format is decided by its lines, not its name
  syntax/         language registry (15 languages), parsing, highlights, tags
  lsp/            transport, client, actions, positions, outline
  git/            gix, reading only: head text, statuses, hunks, blame, history
                  · the diff base is the blob a checkout would write, and
                    reading it must not run anything (mod); a blame is about
                    a version, and the margin knew which commit (blame);
                    what the remote has not seen is marked (history)
  agent/          the ACP registry, installing an agent, its marks
                  · an agent is installed when the install says so, in
                    writing (install); a conversation is claimed by what
                    names it, and one Obelus at a time has it (chats)
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
Obelus has none. So a test, a pipe and most terminals draw the glyph path,
which is why the fixtures never contain pixels.

**The agent protocol comes from its own crate, joined to the loop on one
thread.** `agent-client-protocol` is the reference implementation and it is
built around `async`; `acp::link` is the join, and it is the only place in
Obelus where a runtime exists. Its module doc has the rest: why the runtime
is current-thread, why the two directions are not symmetrical, and the one
ordering the protocol does not promise. `tests/agent.rs` drives a real
process at it -- `tests/fixtures/fake-agent.sh`, which also checks Obelus
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

**Nothing but the animation waits on the animation's clock.** Answering
`None` over a network is right for a sheen and wrong for anything the reader
is owed, and five things that are owed hung on the tick or on the frames it
kept coming: the notes' three hundred milliseconds, a tree a slow grammar
left behind, the five seconds a rename gives a server, the standing
questions a server is asked once the reader stops, and the pointer's rest.
So over ssh none of the five ever happened -- a note typed reached no file
until the reader walked out of it, the colours stopped arriving, a rename
waited on a stuck server for the rest of the session, and the pointer could
rest for ever and never ask. Each has a one-shot clock of its own now
(`event::Pause`, an event each, all started through `App::come_back_in`), so
`animate` asks one question again -- whether anything is *moving* -- and
`Event::Tick` is the animation's: a phase and a drag. A pause is a moment
and an animation is a frame rate; the two only ever looked alike.

The waiting is the shared part and the meaning is not, so each waiter names
its own event and its own rule for restarting one. The notes and a
document's standing questions measure the reader *stopping*, so every key
starts theirs again; a tree that is behind wants catching up soon whether or
not they have paused, so `catch_up_soon` is set by the first frame that
notices and not put back by the ones after -- which is the watcher's
debouncing rule, kept for the same reason. Where the clock has an owner it
lives in it: the rename's is a field on the wait, so finishing takes the
wait and drops the clock with it.

Saying the deadline twice is what a repeating clock forces, and three of
these said it twice. `rename_without_them` was asked on every tick whether
the question was five seconds old; the notes and the standing questions each
kept an `Instant` for a frame to measure. A one-shot arriving *is* the wait
having run out, so those checks went and `Waiting::asked`, `notes_settling`
and `Settling::since` went with them. The one that stayed is the pointer's,
and the rule is what decides it: `settle_hover` is still reached every
frame, because the rest of what it does is letting go of an answer the
pointer has moved off -- that is about where the pointer is now, not about a
moment passing -- so a frame arriving for some other reason must not be
taken for the dwell. **A guard on a deadline earns its place exactly when
something other than that deadline's own clock can reach the work.**

**A change that has happened is the working tree's; a change that has not is
the agent's to show.** An agent that edits a file leaves the file different
from the last commit, and drawing that is what Obelus does all day: the
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
so Obelus diffs the two with `Changes::between` -- the engine the margins
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
Obelus. No new key was needed -- not even space, which everywhere else in
Obelus is a character.

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
of Obelus's own.** It used to write "asking to run the tests" as a note and
then put the question underneath -- the same words twice, once Obelus
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
arguments in its own shape, and reading meaning into it would be Obelus
guessing. A form said it in a transcript line of its own once ("it asks:
..."), which is the same words twice: the question is on screen, and what
it is about belongs over it rather than above the last thing the agent
said.

**A list open over anything owns the status row.** It is the thing taking
the keys and holding the caret, so `StatusView` draws its prompt before the
settings' filter or the conversation's own row. A row belonging to what is
behind the list is a prompt with somebody else's words in it, and the caret
sitting in it says the words are being typed there.

**A command is the agent's namespace; a setting is Obelus's to draw.** Two
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

Obelus used to take `/model` for itself: the agent's command and Obelus's
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
options is declared as, Obelus now writes to the log as it arrives: how an
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
strip -- the heaviest thing Obelus draws -- out of the smallest part of the
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

**A test whose input is the checkout's own history only passes at one
moment.** Where Obelus draws a run of changes is checked against real `git
diff`, and it was checked over the last sixty commits of whatever checkout
it ran in -- so it meant something different after every commit, could go
red over a change that had nothing to do with the one under test, and went
green again on its own when the offending commit slid out of the window. It
is the same rule as the fixture that carried `~/Work/obelus`, with a clock
on it.

The pairs live in `tests/fixtures/diffs/` now and the answer comes from real
`git diff --no-index`, so nothing is written down that could only agree with
itself. Every pair in there is a diff the tidying changes -- a hand-written
one is drawn the same way with it and without, so a corpus of those would
pass however the diff was written.

The sweep is kept, because it is what *finds* them, and it is `#[ignore]`d:
run deliberately, skipping the pairs it has already handed over, so red
means there is a new one to look at. When it turns one up, that pair joins
the fixtures. One of them, `serving`, is there because Obelus and git draw
it differently and always will: two independent implementations of one
algorithm broke a tie differently, both diffs put the file back, and
Obelus's is four edits the smaller. What is held to there is that the hunks
reconstruct the file, which is the claim underneath the other one.

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
choice is not which library; it is whether to write at all, and Obelus does not.

If that is ever revisited: shell out for all four verbs, because one of them
(push) has no other option and two mechanisms for one act is worse than one.
`GIT_TERMINAL_PROMPT=0` fails cleanly without touching the terminal;
`GIT_ASKPASS=<program>` is called once per credential with the prompt as
`argv[1]` and the answer read from stdout, which is how a TUI asks for a
password without losing the screen. Both measured.

Also waiting on a configuration file, which does not exist: the Nerd Font
switch, user theme colours, the server table, and the word-wrap toggle all
want one.
