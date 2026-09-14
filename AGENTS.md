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

**A commit's message hangs above the first line of its file.** Rows on
screen that the file does not have, with no line numbers, that the caret can
walk into and copy from: obelus has one shape for that already, and this is
it. The reader lands *in* it, because they opened this to find out why the
file says what it says and the file itself is a page away. `Held` is what
keeps it apart from a hunk's removed lines -- a deletion is gone and reads
red, a message is a note and reads raised -- and it is why replacing the
diff closes the hunks and leaves the message where it is.

**What the remote has not seen is marked, in the colour a new file wears.**
The few commits a reader has not pushed are the ones still theirs to change,
and they are what someone scanning a history is usually looking for -- the
same argument the file list makes for colouring what git has not seen.
Nothing is marked where the question does not arise: a branch that tracks
nothing, or a repository with no remote at all, has every commit equally
unpushed, and marking all of them says no more than marking none. It is
walked from the tracking branch and stopped as soon as every commit on
screen is accounted for, so the ordinary case -- a remote at or near `HEAD`
-- costs about what the list itself did; a commit the walk did not reach
before its budget ran out is left alone, because telling a reader their work
is not on the remote when it is would be the worse lie.

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

**A list that is still arriving must sit still.** `replace` is for a
different list and starts at the top; `relist` is for the same list with
more in it and keeps the row under the reader. And the history filters
without ranking (`keeps_order`), which is right on its own -- a log is a
timeline, and `git log --grep` keeps it -- and which also means arrivals are
older commits that land at the bottom, where they move nothing. Measured
first: nucleo ties far more often than expected on short subjects, and ties
already break by arrival order, so ranking reorders a log less often than it
looks. Less often is not never, and a guarantee beats a tendency.

**Say "still reading" where it does not move the rows.** A note above the
list that appears and later goes away slides every row twice. The tab row
has room that is already there. And the note carries a count, because a
file's history can find nothing for a second and a half and still be
working: without a number moving, "not found yet" and "not there" look the
same.

**Two buffers can wear one path, so a path alone cannot say which.** A file
and that file as some commit had it live at the same path, and `read_at`
matching on the path alone previewed one at the other's place in it. Match
on the path *and* on which version it is -- `content().at()`.

**Which tabs a view has must be a cheap question.** The search settles its
scopes when it opens and the history settles its radii, and both settle them
on facts they can have for nothing: is a file open, does the project have a
commit. "Does *this file* have a commit" is not such a fact -- every commit
has to be asked whether it touched that path, and the walk that asks is
bounded -- so gating the tab on it made the tab vanish for files nobody had
edited lately, which are exactly the ones whose history a reader goes
looking for. An empty list saying "no commit has touched this file" is an
answer; a missing tab is a key that does nothing.

**A history is one view at two radii.** A file's commits and a project's
differ only in which commits are listed, so they are two tabs of one list
and `f9` and `f10` land on the tab they name -- the shape the finding keys
have, for the same reason: a reader who does not find it in this file looks
in the project without pressing a second key to get there.

A commit in the project's tab is not a file, so there is nothing for
choosing it to open. What it has is the list of files it changed, and that
goes *under* it, in place, the way a run of tool calls opens in the
transcript: one list, one selection, one Escape. In the file's own tab a
commit *is* a document -- that file as that commit had it -- so choosing one
opens it, and there is nothing to put underneath: a list of the files it
changed would be a list with the tab's own name in it. The mark says so -- the
same `▸`/`▾` the transcript and the fold column use, because a reader who
has learned it in one place has learned it. The file's own tab has no marks
at all: a commit there is already about one file, and offering to show
which would be a row repeating the tab's name.

A subject is a sentence, so a row too narrow for it loses its *end*. The
rest of a picker's rows are names -- a path, a symbol -- where the end is
what is being looked for and the head is already known; `…the block the
cursor is in` has lost the half that says which commit this is. The time
and the short id go on the right, where the width is taken out of the
subject's before it is truncated: what must survive the cut is how to find
this commit again.

**Two buffers can wear one path, so a list of them says which is which.**
The file and the file as some commit had it differ in what they say, in
whether they follow the disk, and in what the margin beside them means; two
rows reading `src/parser.rs` are two rows a reader picks between blind. The
short id on the right and no more -- the status row marks the same fact in
the same words, so a reader who has seen one has read the other, and the
list's own job is still to show paths.

**A preview is of a subject, not of a path.** A row does not always name a
file on disk: a commit names what it said, and one of a commit's files names
that file as the commit had it -- a different document from the one at the
same path in the working tree. `Subject` is what a row resolves to, and the
preview is built from it the way the editor would build it, message block and
all, because a preview that showed something other than what choosing the row
gives is a promise obelus does not keep. A commit's message previews as a
block over an empty buffer, which is how it gets no line numbers: a message
has no lines of its own to go to.

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
opens as its *bytes*, whatever reading it has, and `ctrl+t` asks for the
reading; the bytes are where the cursor, the selection and the copy live.

It used to open in the reading when it had one, under a setting that was on
by default. Which reading a file has is the file's own business; whether to
be shown one *instead of the file* is the reader's, and a program whose
whole subject is what is in a file should not answer that for them. The
setting went with the behaviour, because a switch that turns off something
nothing does is a switch with nothing behind it -- and an old config naming
it is simply ignored, the way any key `from_toml` does not know is.

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

**A folded line is a line with no rows.** Folding hides lines; an opened
hunk adds rows the file does not have. Both are the same arithmetic --
`screen_rows_of` and `step_rows` are where a view asks how tall a line is --
so folding is that one term going to zero rather than a second set of counts
beside the first. Everything falls out of it: the caret lands on the row the
line is drawn on, paging moves by what is on screen, the view walks past
what is hidden. What does *not* fall out is where the cursor may rest, which
is why folding over the reader walks them back to the line the run starts on
-- the one line of it still there -- and why arriving somewhere
(`place_cursor`) opens whatever hid it. Walking is the other thing: a step
goes around a fold, because the reader asked for the next line they can see.

**What folds comes from the indentation, and where it ends comes from the
bracket.** Two other sources were built and thrown away, and the reason both
failed is the same half of the question. Deriving *which* lines fold from
tree-sitter's node shapes works; deriving what the folded row should then
*show* does not, because that needs to know a Rust block closes with `}` and
a Python one closes with nothing -- a table per language, wrong the day a
grammar changes, and every rule that guessed it from the text was wrong
somewhere (a "closing mark is at most four characters" test reads the `def`
at the end of a Python function as one). Asking a language server answers
both, but only for files a server will answer about, and its ranges are its
own: rust-analyzer ends a block one character *past* the `}`, so taking the
range at its word drops the brace, and it sends two runs for an `if` -- one
from the keyword, one from the brace.

Indentation gives both halves at once, and it is what Zed settled on too. A
run opens on a line whose next non-blank line is deeper, and closes on the
first line that is no deeper. It starts at the *end* of the line that opens
it, so that line stays whole, `{` and all. It ends just before the closing
bracket when the line it closes on begins with one -- so the bracket comes up
beside the mark and the row reads as `if ready { … }` -- and at the last line
with anything on it when there is none, which is how `def ready(): …` comes
out of the same rule without a word about Python in it. Blank lines are
walked past inside a run and left outside it at the end: they belong to
whatever comes next.

The price is that a file with nothing indented folds nowhere. A TOML file is
a list of tables at column zero, and so is most markdown and so is a
paragraph of `///` comments: there is no block for a reader to close, and a
mark offering to hide "the rest of the file from here" is a different offer.

`syntax::brackets` knows the three pairs already, for the key that matches
them. What folding needs of it is narrower still -- a line that *begins* with
one of `)`, `]` or `}` closes something -- and that is true without knowing
what was opened or where.

**What the bar measures is what is shown.** The scrollbar and the change map
are pictures of the document at the height of the screen, and a closed run
makes the document shorter: drawn from the file's own line numbers they say
the reader is at the top of something long while the whole of it is in front
of them. Both count in lines that are shown, which is why `Folds` can say how
many are hidden above a line and how many altogether. An opened hunk closes
when a fold hides the line it hangs above, for the same reason
`refresh_changes` closes it when the diff is replaced: its rows belong to
something that is no longer on screen, and a caret in them is a caret nobody
can see.

**What is highlighted is what is drawn.** `visible_bytes` walks past folded
runs the way the view does, rather than counting `height` lines down from the
top. It is the same arithmetic as everywhere else here, and getting it wrong
is not subtle: with a two-hundred-line run closed at the top of the screen,
every row below it is a line two hundred further down, outside the range that
was highlighted, and the whole of the rest of the screen is drawn in the
plain foreground.

**A hunk opens where it is, and so does the next one.** `Buffer::blocks` is a
list by the line each hangs above, not one slot: a reader comparing two
changes wants both on screen, and the two they most want side by side are the
two they are deciding between. Which one a key acts on is then a question the
key has to answer, and the answer is not simply "the one above this line":
the caret's own block comes first, then the one belonging to the hunk the
reader is standing in -- which hangs above that hunk's *first* line however
far down it they have walked -- and then one hanging just below them, which
is where a reader who walked out of the top of one is left. A selection is
drawn in the block it was made in and nowhere else, because a span is a pair
of offsets into one text and against another it marks whichever characters
happen to sit there.

**The caret can be in the block; the cursor never is.** `Buffer::block_above`
is an opened hunk's lines *as a `Text`*, and `in_block` is a `Cursor` in one
of them.
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
  binding that works on one machine and not the next. `f9`-`f12` are git's,
  and empty until each is earned -- a file's history and a project's, a
  patch to review, a panel of what a commit touched.
* **Control does something to the file in front of you**, on the letter of
  the word: `p` the palette, `w` close, `r` re-read, `t` toggle the reading
  its format has, `l` a line number, `a` all of it, `c` copy, `q` leave.
  `ctrl+v` is left alone: obelus takes typed text in the agent's box, and
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

**A bar is a block, so no rule has to meet it.** The track is a block a
shade off the page and the thumb the same block brighter: a surface with
something sliding on it, which is what a scrollbar is. Drawn as a *line* --
a column of ┃ with the thumb picked out -- it was one more line on a screen
of lines, and every rule that crossed it then had to decide whether to join.

That decision cost more than it was worth. `rule` and `scrollbar` made it by
reading the grid back: a cell holding ┃ or █ beside a rule meant a bar, and
the rule turned into a corner. But a cell is a cell. A file's own text
answers that question exactly as a control does, and this repository is full
of files that do -- every golden grid under `tests/fixtures` is drawn in box
characters. The note that used to be here called that a cosmetic slip in one
cell; what it looked like on screen was a row of ┬ across the whole width,
under a list previewing a file of grids, and the reader who found it was
looking at an obelus previewing obelus's own fixtures. A markdown table has
the same glyphs and would have done the same thing.

So the shape of the thing says what it is, and nothing reads the grid back.
A rule runs its whole width in one glyph; a block column meets it and needs
nothing from it. The one row of the block that the rule takes is the
boundary between two bars -- a list's and its preview's -- which are two
controls over two different things, and reading as two is right.

**A column a file might need is reserved for the whole file, not for the
lines that need it.** The change margin is there whenever git can answer
about the file, empty rows included; the fold column is there whenever the
file has anything to fold, whether or not anything is folded. A column that
arrived when the reader pressed a key would rewrap the text under them as it
came. What goes *in* the column is every run, open or folded: a reader
cannot press a key on a line that never said it had anything behind it, so
the mark is how folding is discovered at all. The one turned down is the
quieter of the two, which is the right way round -- most runs are open most
of the time, and the eye should be caught by the lines that are hiding
something. The column is decided when the file is read and stays decided,
so nothing a reader does to a fold ever moves the text sideways under them.

A folded run also says so on the row it folded into: the view's mark after
the line's own text, and then whatever is left of the run's last line. That
is the whole rule -- no test for what a closing mark looks like, no table per
language -- because the run was *built* to stop before the bracket. A run
that closes with nothing leaves the mark on its own.

Two colours, because they are two different things. The mark is obelus's own
and is drawn the way its notes are; the closing text *is* the file's and
keeps the colour the highlighting gives it where it really lives. A brace
that changed colour on its way up the screen would read as something else.

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

**A tree may carry settings, and a tree is not the reader.** `.obelus/config.toml`
in the working directory, or `.obelus.toml` beside it, laid *over* the
reader's own file key by key: the tree says what this project needs -- wrapped
lines, a theme -- and says nothing about everything else, which stays
theirs. Read into a fresh config instead of over theirs and a tree with one
line in it would turn off a reader's wrapping, which is what every "project
settings" feature that replaces rather than layers actually does.

The directory form is looked for first, because it is the form with room in
it: a theme belonging to the tree will go beside the config in there. The
single file stays, because making a directory to set one line is asking too
much. Only the working directory itself, never walking up: obelus has one
answer to which tree it is on -- the file list walks it, the counts count
it, git is read from it.

**What a tree may set is a property of the setting**, `Reach`, not a list of
exceptions somewhere: the next setting a stranger should not be trusted with
will be found by asking that question while writing the setting down.
`agent` is `ReaderOnly` because it says which agent obelus *starts*, and a
program starting because a file in a downloaded tree said so is a decision
that belongs to the person at the keyboard; `keys` is `ReaderOnly` because a
tree that could rebind them could put `quit` where a reader would find it by
accident. VS Code learned this one the same way and calls it `machine`
scope.

Nothing is ever written to the tree's file -- it belongs to whoever wrote
the tree, and a reader changing a theme would be editing a file their next
commit carries. So a setting the tree has is not theirs to change here, and
the row says so rather than doing nothing: the file's name sits where they
would have reached, a lock against the control, the whole row in the dim ink
that means unusable everywhere else. Sublime's project settings win
silently, and "I changed it and nothing happened" is the bug that follows.

**A setting is two rows: a name, and what it does under it.** The name on
its own row with its control at the right, what it does on the rows under
that -- indented, in the dim colour, *wrapped* -- and a blank before the
next one, which is what makes an entry an entry rather than three rows of a
table. The same reason the agents' cards have one.

Beside the name, the two competed for one row and the description lost: cut
off with an ellipsis on exactly the rows that had most to explain, and cut
further still on a row a tree had pinned, where the file's name takes the
space as well. Under it, the sentence has the width of the page and can say
what it means -- `wrap` can say that lines break between words, `icons` can
say what a terminal without the font will draw.

So the entries are not all one row tall, and the window is settled by
*height*, the way the page of cards already was. `Settings::setting_rows` is
the one answer to how tall one is, asked by the page laying them out and by
the window deciding which are on screen: two answers there is a reader
walking onto an entry nobody drew.

**A setting is a name and a gloss, not a sentence.** `Colour theme`, `Nerd Font
glyphs`, `Wrap long lines`, `Blame in the margin` -- a noun phrase naming
the thing, not a clause about it. These were whole sentences ("Who last
changed the line the cursor is on") on the grounds that a name and a
description side by side read as a heading and a footnote. True when the
footnote says what the heading already had; what it produced was a page of
prose, where a reader looking for one row had to read every row to find it.
A column of names is *scanned*.

The keys page, whose rows are one row each, starts what a command does in
one column two past the longest name rather than two past its own: four
beginnings to find is four, and one is one.

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
