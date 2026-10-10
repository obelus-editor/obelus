//! The conversation with an agent: what was said, and what is being typed.
//!
//! Not a buffer. A buffer is a file with a cursor in it and a history behind
//! it; this is a transcript, which grows at the end and is read from the
//! bottom. It is in the list of what is open all the same -- as a
//! `Document::Chat`, which is the shape that let it be
//! listed without having to become one.
//!
//! It holds no client. What it has is what arrived, and every key it does
//! not handle itself becomes an outcome for the application to act on --
//! which is what keeps "send this prompt" out of a component that cannot
//! know whether there is an agent to send it to.
//!
//! A tool call is somewhere to go, not something to read about. The
//! protocol says what sort of thing the agent is doing (`kind`) and which files
//! it was in (`locations`), and Obelus kept neither: a row with a title and a
//! tick on it. The kind picks the glyph, because a reader scanning a turn is
//! looking for whether it *changed* anything and that is a picture rather than
//! a sentence; the locations go on the row as a path, written relative to the
//! tree Obelus was opened on. Other clients open a preview from one of these;
//! Obelus opens a *buffer* -- with its jump list, its definitions, its hunks --
//! which is the one thing a reader has that they do not.
//!
//! Everything on the row is kept rather than rebuilt, because an update carries
//! only what changed. An agent saying "it finished" and nothing else is not
//! saying the file it was in has stopped being the file it was in, and a row
//! rebuilt from that update would lose its kind, its title and its place at the
//! moment it succeeded.
//!
//! The transcript has a cursor, and it walks the words; tab goes to what
//! acts. The arrows move it a character and a row at a time, over every row
//! drawn -- prose, a blank, a heading -- because a reader taking a copy of a
//! sentence has to be able to stand in it, and a press puts it where the
//! pointer landed. Tab and shift and tab step to the next row that does
//! something: a tool call names a file, a heading opens what is under it.
//! None of them is lit while the cursor is on it: the caret says where the
//! cursor is, a key at the row's end says what enter does there, and a light
//! behind the row was drawn over what the reader had hold of in it.
//!
//! Enter opens what the row names -- in a buffer, and the conversation hides
//! itself, because going somewhere means seeing it. Escape comes back out to
//! the box without closing anything. Typing out here goes nowhere, and does
//! not take the keys back to the box either: it used to, on the grounds that
//! a reader who starts typing means to type, and what it meant in practice
//! was a stray letter pressed while reading throwing the caret out of what
//! the reader had walked to. The box is one escape or one arrow away, and
//! going back there is the reader's to say.
//!
//! A link is opened by a click, not by enter. One markdown wrote and an
//! address written out in the words are both underlined where they are,
//! and a row may hold several -- and may already have enter for something
//! of its own: taking a message back, opening a call. Enter on a link took
//! that key away wherever the two met, and a message that was nothing but
//! an address could not be taken back at all. The pointer says which link
//! by landing on it, and a press that becomes a drag is taking hold of the
//! words rather than following them.
//!
//! Shift extends, and control goes to the ends. Shift and a motion holds
//! what the motion passed over, in the box as in the transcript, because
//! shift means one thing everywhere -- and the box was where it meant
//! something else: `shift+home` threw the whole conversation to its
//! beginning and held nothing, in the one place a reader reaches for the
//! pair to take back the line they have just written. The two ends of the
//! transcript are control's, which is what the rule over the box says in as
//! many words, and they are the same key from either half: from the box
//! they move the view, and from inside the transcript they take the cursor
//! with them, because a view sent to the end with the cursor left behind is
//! dragged back by the next arrow.
//!
//! A run of tool calls of one kind is one row until the reader opens it.
//! Thirty calls in a turn is a log, and a reader looking for what the agent
//! *did* should not have to scroll past the machine to find it. Three in a row
//! is where they stop reading them and start scrolling past them, so three is
//! where a run folds itself. Opened -- enter on the heading, which is the same
//! "do what this row is for" enter means everywhere -- the members are rows of
//! their own, each one a file to go to.
//!
//! A failure in a run opens it, because a failure is the one thing in a turn
//! nobody should have to go looking for. What the reader said about a run beats
//! both: one they closed stays closed, whatever is in it.
//!
//! **A change that has happened is the working tree's; a change that has
//! not is the agent's to show.** An agent that edits a file leaves the file
//! different from the last commit, and drawing that is what Obelus does all
//! day: the margin, `show-change`, `alt+d`. Rendering the agent's own diff
//! over it would be a second answer to the same question, and the wrong one
//! when something else has touched the file too -- so an edit that has been
//! made is a row with `+12 -4` on it, and the file is where it is read.
//!
//! A change it is *asking* to make is the opposite: the lines are in neither
//! the file nor the last commit, and the reader is being asked to agree to
//! them. Those go in the transcript, under the call's own row, open -- and
//! they stay there afterwards, which is how a reader finds out later what
//! they agreed to. Drawn the way an opened hunk is drawn in a file, tinted
//! to the edge with the marker's bar against the text, because it is the
//! same thing being said: `Theme::marker_colour` is where both views ask
//! what a change looks like.
//!
//! The protocol sends the file as it is and as it would be rather than a
//! patch, so Obelus diffs the two with `Changes::between` -- the engine the
//! margins come from, through `obelus_git::change::drawn`. Nothing parses
//! anybody's patch text, and a proposal is read with the same hunks as
//! everything else. The rows are worked out once, when the call arrives
//! (`changed_rows`): a frame is not the place to diff a file. It folds
//! itself once the call is finished and stays open while it is pending
//! (`Chat::is_open` says why), and the reader's word beats both, as
//! everywhere else.
//!
//! Thinking is not folded away. Folding is for repetition, and thinking is
//! prose -- often the most of what a turn is worth, since it is where the agent
//! says why it thinks the bug is where it thinks it is. It gets a heading so a
//! reader who has read it can put it away, and only when it is long enough for
//! that to be worth a row: a heading over three words is two rows saying one
//! thing. Obelus never closes it by itself, which also means it can never close
//! under somebody who is reading it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_row::{Ink, Span};

use crate::{composer::Composer, window::Window};

mod keys;
mod layout;

/// Who said something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    /// The reader.
    Reader,
    /// The agent.
    Agent,
    /// The agent thinking out loud, which agents send apart from their
    /// answer so that it can be shown as what it is.
    Thought,
    /// The agent using a tool.
    Tool,
    /// Obelus itself: what went wrong, what was allowed, what stopped.
    Note,
    /// Somewhere on the web the agent sent the reader, and whether what
    /// was to happen there has happened.
    ///
    /// Its own voice rather than a tool call that happens to point at a
    /// URL: what a call does is the agent's, and this is a thing the
    /// reader did -- and what ends it comes from the agent watching the far
    /// end rather than from the call finishing.
    Away,
    /// What is happening now.
    ///
    /// Not a thing that was said: the state of the one saying things, at
    /// the foot of the transcript where the next thing will appear. It is
    /// there while it is true and gone when it is not.
    Doing,
    /// One step of the list the agent is working through.
    ///
    /// Its own voice rather than a [`Self::Doing`] row that happens not to
    /// be the first: what a row is has to be something it says, not
    /// something read off two other fields. While it was inferred it
    /// inherited what belongs to the row above it -- the way to stop the
    /// turn, on every step, and a status drawn both in front of the words
    /// and behind them.
    Step,
}

/// What a command is doing, for the row that is about it.
///
/// Handed over rather than read: Obelus holds the process, and the rows
/// are filled from it every frame the way [`Chat::doing`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doing {
    /// The command and everything it has printed, as the row's own words.
    pub words: String,
    /// How the call stands now, where the command has ended: a call
    /// running a command that failed is a call that failed.
    pub state: Option<String>,
}

/// One thing that was said.
#[derive(Clone, Debug)]
pub struct Said {
    /// Who said it.
    pub speaker: Speaker,
    /// What they said. May have newlines in it.
    pub text: String,
    /// The agent's own id for a tool call, so a later update about the same
    /// call replaces this line rather than adding another.
    pub tag: Option<String>,
    /// Where a tool call has got to.
    pub state: Option<String>,
    /// What sort of thing a tool call is doing, which is what picks its
    /// glyph: a reader scans a turn for "did it *change* anything".
    pub kind: String,
    /// What the reader put together, where this row is theirs.
    ///
    /// `text` is what the page shows and this is what goes to the agent,
    /// written at the same moment by the same call so the two cannot come
    /// apart. Only a reader's row has any: nothing else carries a picture,
    /// and a row with none sends its `text`.
    pub parts: Vec<crate::composer::Part>,
    /// The files a tool call named.
    ///
    /// Kept whole rather than as one line of text, because they are places
    /// to go: Obelus opens files for a living, and a tool call is the agent
    /// saying which ones it has been in.
    pub places: Vec<obelus_agent::acp::Place>,
    /// The change it is making, as the rows that draw it.
    ///
    /// Empty for everything that is not a change. What is kept is the rows
    /// rather than the two texts: the diff is worked out once, when it
    /// arrives.
    pub change: Vec<obelus_git::change::Line>,
    /// The command Obelus is running for this call, where it is running
    /// one.
    ///
    /// The output is not kept here: Obelus holds the process, and what it
    /// has printed is read off that every frame -- the same rule
    /// [`Chat::doing`] follows, so there is no way for the rows to be
    /// showing a command's output from a moment ago.
    pub ran: Option<String>,
    /// What a tool call says, in the order it gave it -- and, for a row
    /// the reader was sent away by, the one address it sent them to.
    ///
    /// Two uses of one field, told apart by [`Self::speaker`] and nowhere
    /// else: what is here is read back only where that says what it is,
    /// which is why the voice and not the shape decides. A field of its
    /// own would be a field that is `None` on every row but one kind.
    ///
    /// Replaced by a later update rather than added to, which is what the
    /// protocol says `content` means -- *replace the content collection* --
    /// and the same thing [`Self::change`] beside it already does. A call
    /// carries what it says now: a plan put to the reader is replaced by
    /// what became of the asking, and which of the two a row shows is the
    /// agent's account of its own call.
    pub words: Vec<String>,
    /// Whether the reader has opened or closed what this begins.
    ///
    /// `None` means nobody has said, and Obelus decides: a change under a
    /// call folds itself once the change is made, and its thinking stays
    /// open because thinking is prose somebody may want to read. Once the
    /// reader says otherwise it stays the way they left it.
    pub opened: Option<bool>,
    /// And whether they have opened or closed the *run* that begins here.
    ///
    /// Its own answer, because a run and the first call in it are two
    /// rows and were one fold. A run is named by the call it starts at --
    /// there is nothing else to name it by -- so the heading and that
    /// call both asked [`Said::opened`], and enter on the first call of a
    /// run shut the whole run instead of that call. Nothing in between
    /// could have told them apart: they are the same index, and the
    /// question is which of the two things beginning there the reader
    /// meant.
    pub run_opened: Option<bool>,
    /// Whether this is something the reader has said that has not gone yet.
    ///
    /// A conversation takes one prompt turn at a time, so what the reader
    /// says into a running one sits on the page until that turn ends. On
    /// the page rather than in a queue beside it because the words are the
    /// reader's: a count on the status row said how many were waiting and
    /// never which, and there was nowhere to stand to take one back.
    ///
    /// Only ever true of [`Speaker::Reader`]. It goes false for all of
    /// them at once, when they go together -- see [`Chat::sent`], where
    /// the ink stops being dim, which is the receipt.
    pub unsent: bool,
    /// Whether it came from a chat rather than from the box here, while
    /// it waits: what goes to the agent with it says where the reader is,
    /// and that is decided when it goes, by what it went with.
    pub afar: bool,
}

/// A link on a row of the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Which of the row's characters are its words.
    pub characters: std::ops::Range<usize>,
    /// The web address it goes to.
    pub to: String,
}

/// What the mark on a row opens and closes.
///
/// Three things fold in a transcript and they are three different things,
/// which an index alone cannot say. A run is named by the call it begins
/// at, so `Said(n)` and `Run(n)` are the same number meaning two rows;
/// and the plan is not a thing that was said at all -- it used to borrow
/// the one index that can never name one, which worked and explained
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folds {
    /// One thing said, by its place among them.
    Said(usize),
    /// A run of calls of one kind, by the first of them.
    Run(usize),
    /// What the agent means to do about this turn.
    Plan,
}

/// One row of the transcript, wrapped to a width.
#[derive(Clone, Debug)]
pub struct Row {
    /// Who is speaking on this row.
    pub speaker: Speaker,
    /// The words, in runs, each saying how it is drawn.
    ///
    /// Runs rather than one string because what an agent says is markdown
    /// -- the protocol says so in as many words: "Text content. May be
    /// plain text or formatted with Markdown. Clients SHOULD render this
    /// text as Markdown." A heading, a fenced block and a `name` in the
    /// middle of a sentence are three different things on one row, and a
    /// row with one look could only ever draw them as the same thing.
    ///
    /// Most rows are still one run of [`Ink::Plain`], which is left in
    /// whatever colour the voice speaking it is drawn in.
    pub spans: Vec<Span>,
    /// Whether it is the first row of what they said, which is the row that
    /// gets the mark saying who is speaking.
    pub first: bool,
    /// A tool call's state, on its first row.
    pub state: Option<String>,
    /// What sort of thing a tool call is doing, on its first row.
    pub kind: String,
    /// The first place a tool call named, on its first row, and how many
    /// more it named.
    pub place: Option<(obelus_agent::acp::Place, usize)>,
    /// The web address it points at, where it points at one.
    ///
    /// Beside `place` and not folded into it: a file Obelus opens itself
    /// and a URL it hands to the machine are two different things to do,
    /// and a row that said only "somewhere" would make whoever pressed the
    /// key work out which.
    pub away: Option<String>,
    /// The links on this row.
    ///
    /// Beside `away` and not the same thing: that is the whole of what was
    /// said pointing somewhere, and these are words inside it that do.
    ///
    /// Opened by a click and by nothing else. A row may hold several, and
    /// it may already have a key of its own -- a message enter takes back,
    /// a call's title enter opens -- so enter cannot be what follows one
    /// without taking that key away wherever the two meet: a message that
    /// was nothing but an address could not be taken back. The pointer
    /// says which link by landing on it.
    pub links: Vec<Link>,
    /// What this row opens and closes, by where what it begins is in the
    /// transcript.
    ///
    /// A run of tool calls of one kind, or a piece of thinking: both are
    /// one row with a mark on it until the reader opens them, and both are
    /// opened by the same key on the same sort of row.
    pub folds: Option<Folds>,
    /// Whether what it folds is open, for the mark that says so.
    pub open: bool,
    /// Which thing said this is, where it is one the reader has said that
    /// has not gone yet.
    ///
    /// The index and not a flag, because the key that stands here takes
    /// that one back and has to name it. On the first row only, like
    /// everything else that is true of the whole of what was said.
    pub unsent: Option<usize>,
    /// Which thing said this is, where it is one the reader has said and
    /// that has gone.
    ///
    /// Beside `unsent` rather than folded into it, because the key does
    /// something else here: what has gone cannot be taken back, so enter
    /// puts a copy in the box and leaves the page as it was. On the first
    /// row only, like `unsent`.
    pub again: Option<usize>,
    /// What the row is, where it is a line of a change: gone, new, or the
    /// line it is at.
    pub marker: Option<obelus_text::marker::Marker>,
    /// How much a change adds and takes away, on the row that heads it.
    pub changed: Option<(usize, usize)>,
    /// The block of code this row is part of, which enter copies whole.
    ///
    /// On every row of it, the box's sides included, so the key answers
    /// wherever in the block the reader is standing. What it copies is the
    /// block's own lines: a selection takes what is on the screen, box and
    /// all, and a line the width broke in two comes out as two -- which is
    /// not the command the agent wrote.
    pub code: Option<obelus_row::Code>,
    /// How deep the row sits: the members of an opened run are drawn under
    /// their own heading, so that a run reads as one thing.
    pub depth: u8,
    /// Which text of which thing said this row was laid out from.
    ///
    /// `None` for the rows Obelus makes up rather than lays out: the blank
    /// between two things said, the heading over a folded run, the row
    /// that says what is happening now, a step of a plan. Those are not
    /// anybody's words and there is nothing in them to take a copy of --
    /// the same answer the notes give for the row that says where a note
    /// points.
    pub from: Option<(usize, Source)>,
    /// Which of this row's characters the reader has hold of.
    ///
    /// Counted in characters of this row rather than in bytes of what was
    /// said, because a row is what gets drawn and marked: a selection
    /// given in the source would have to be worked out again by whoever
    /// draws it, at the width they happen to be drawing at.
    pub held: Option<std::ops::Range<usize>>,
}

/// Where a call says it was, which for a change is the file it changes.
///
/// An agent editing a file often names no location: what it is doing is the
/// diff, and the diff says which file. Taking the path from there is what
/// makes an edit somewhere the reader can go.
fn places_of(call: &obelus_agent::acp::Call) -> Vec<obelus_agent::acp::Place> {
    if !call.places.is_empty() {
        return call.places.clone();
    }
    call.change
        .iter()
        .map(|change| obelus_agent::acp::Place {
            path: change.path.clone(),
            line: None,
        })
        .collect()
}

/// The change a call is making, as the rows that draw it.
///
/// Worked out once, when it arrives, rather than every time the transcript
/// is drawn: a diff is real work, and a frame is not the place for it.
fn changed_rows(call: &obelus_agent::acp::Call) -> Vec<obelus_git::change::Line> {
    call.change.as_ref().map_or_else(Vec::new, |change| {
        obelus_git::change::drawn(change.before.as_deref().unwrap_or_default(), &change.after)
    })
}

/// How many tool calls of one kind in a row it takes before they are folded
/// under a heading.
///
/// Two of anything is not a log. Three is where a reader stops reading them
/// and starts scrolling past them.
const LEAST_TO_FOLD: usize = 3;

/// How many rows of a call's title are shown while it is closed.
///
/// A command is a title as long as the command, and the reader of a
/// transcript of commands is reading the commands: one row of `grep` and
/// an ellipsis is a row that has to be opened to be read at all. Three
/// rows is most of them.
///
/// Capped rather than free, which is what a card's own prose gets and for
/// the same reason: an agent may write as much as it likes and may not
/// push what it belongs to off the screen. An agent that sends a heredoc
/// script as a call's title is the case that earned this number -- twenty
/// rows of shell sat under a mark saying the call was shut.
const MOST_TITLE_ROWS: usize = 3;

/// How far in the members of an opened run are drawn.
///
/// Shared with the view: the rows are wrapped to what is left after it and
/// drawn starting at it, so the two have to be the same number or the words
/// run off the end.
pub const DEEPER: u16 = 2;

impl Row {
    /// Whether enter does something on this row.
    ///
    /// A tool call names a file and enter opens it, a heading opens what is
    /// under it, a row the reader was sent away by sends them again, and a
    /// block of code is copied. The cursor stands on any row; these are the
    /// ones tab goes to.
    ///
    /// The list is the one the key's own `match` answers, and the two have
    /// to say the same thing: a row this lets the cursor stand on and that
    /// does nothing is a key that appears not to work.
    #[must_use]
    pub const fn acts(&self) -> bool {
        self.place.is_some()
            || self.folds.is_some()
            || self.away.is_some()
            || self.unsent.is_some()
            || self.again.is_some()
            || self.code.is_some()
    }

    /// The rows a key pressed on `row` acts on, or none where it does
    /// nothing.
    ///
    /// The words of a thing said are one thing however many rows they wrap
    /// to, and enter on any of them answers from the first of them, which
    /// carries `unsent`, `again`, the fold, the place and the address. Only
    /// the first used to answer: enter on the rest of a message did
    /// nothing.
    ///
    /// The words and nothing under them: what an opened call carries is
    /// deeper and from somewhere else, and is not the call.
    ///
    /// A block of code is the one thing inside the words that acts on its
    /// own, and it is asked first: it is copied whole from any row of it,
    /// and the message around it does nothing.
    #[must_use]
    pub fn acting(rows: &[Self], row: usize) -> Option<std::ops::Range<usize>> {
        let here = rows.get(row)?;
        if let Some(code) = &here.code {
            let same = |row: &&Self| {
                row.code.as_ref().is_some_and(|it| it.at == code.at) && row.from == here.from
            };
            let start = row - rows[..row].iter().rev().take_while(same).count();
            let end = row + rows[row..].iter().take_while(same).count();
            return Some(start..end);
        }
        if let Some((said, Source::Text)) = here.from {
            let same =
                |row: &&Self| row.from == Some((said, Source::Text)) && row.depth == here.depth;
            let start = row - rows[..row].iter().rev().take_while(same).count();
            let end = row + rows[row..].iter().take_while(same).count();
            if rows[start].acts() {
                return Some(start..end);
            }
        }
        here.acts().then(|| row..row + 1)
    }

    /// How many characters the row draws.
    #[must_use]
    pub fn characters(&self) -> usize {
        self.spans
            .iter()
            .map(|span| span.text.chars().count())
            .sum()
    }

    /// The place in what was said that this row's nth character came from.
    ///
    /// Characters of the row going in and a place in the words coming out,
    /// which is the seam between the two halves of this. A row is what a
    /// width made of what was said and is made again every frame; a place
    /// in the words outlives the width, the folding and the row itself. So
    /// everything that points at the transcript -- the pointer, the cursor,
    /// both ends of a selection -- is kept as the second and worked in the
    /// first.
    ///
    /// Past the end of the row is the place just after its last character,
    /// so that a cursor at the end of a row is somewhere rather than
    /// nowhere.
    ///
    /// A run the reading drew rather than anybody wrote -- a bullet, a
    /// marker's bar -- has no place in the words at all. Standing on one is
    /// standing between the words around it, which is the same answer the
    /// pointer gives for landing on one.
    ///
    /// At the *start* of a row there is no word before, so it is the word
    /// after, on this row. Handing back nothing there was the markdown
    /// selection bug: only markdown lays out runs nobody wrote -- a plain
    /// wrapping gives one run carrying the whole row -- so a bullet or a
    /// quote's bar was the only place this arose. [`Chat::spot_near`] read
    /// the nothing as "this row has no words at all", went looking down
    /// the transcript the way it does for a blank or a heading, and landed
    /// past the row the cursor was on: `shift+home` on a bullet held the
    /// rows *after* the cursor.
    #[must_use]
    pub fn spot_at(&self, characters: usize) -> Option<Spot> {
        let (said, source) = self.from?;
        let mut seen = 0usize;
        let mut after: Option<Spot> = None;
        // Whether a drawn run at the start of the row is still waiting for
        // the word that follows it.
        let mut waiting = false;
        for span in &self.spans {
            let bytes = span.from.clone();
            for (offset, character) in span.text.char_indices() {
                if let Some(bytes) = bytes.as_ref() {
                    let at = bytes.start + offset;
                    if seen == characters || waiting {
                        return Some(Spot { said, source, at });
                    }
                    after = Some(Spot {
                        said,
                        source,
                        at: at + character.len_utf8(),
                    });
                } else if seen == characters {
                    match after {
                        Some(spot) => return Some(spot),
                        None => waiting = true,
                    }
                }
                seen += 1;
            }
        }
        after
    }

    /// Where in this row a place in the words is, counted in characters.
    ///
    /// The way back, for drawing a cursor kept as a place in the words at
    /// the width it is being drawn at. Nothing where the place is not in
    /// this row at all.
    #[must_use]
    pub fn characters_at(&self, spot: Spot) -> Option<usize> {
        if self.from? != (spot.said, spot.source) {
            return None;
        }
        let mut seen = 0usize;
        let mut end: Option<usize> = None;
        for span in &self.spans {
            let bytes = span.from.clone();
            for (offset, character) in span.text.char_indices() {
                if let Some(bytes) = bytes.as_ref() {
                    let at = bytes.start + offset;
                    if at == spot.at {
                        return Some(seen);
                    }
                    // The end of the row is a place too: a cursor after the
                    // last word of a wrapped line is on that line, not on
                    // the next one.
                    if at + character.len_utf8() == spot.at {
                        end = Some(seen + 1);
                    }
                }
                seen += 1;
            }
        }
        end
    }
}

/// How much room the conversation's two halves have.
///
/// Both are needed by the keys: a page of scrolling is the transcript's
/// height, and moving the caret up a row depends on the width the box wraps
/// at. Passed in rather than kept, because only a frame knows them.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    /// The rows the transcript has.
    pub transcript: u16,
    /// The cells a row of the transcript has, which is what its rows are
    /// wrapped to -- and so what decides which row is which.
    pub reading: u16,
    /// The cells a row of the box has.
    pub writing: u16,
}

/// What a key did to the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatOutcome {
    /// The key means nothing here; let the key table have it.
    Ignored,
    /// Something moved or was typed. Redraw.
    Consumed,
    /// Send this to the agent.
    Send(Vec<crate::composer::Part>),
    /// Ask the agent to stop.
    Interrupt,
    /// Stop the turn the agent is on, and send this behind whatever was
    /// waiting for it.
    SendNow(Vec<crate::composer::Part>),
    /// Put these words back in the box: the reader took back something
    /// they had said that had not gone yet, or wants to say again
    /// something that had.
    TakeBack(Vec<crate::composer::Part>),
    /// Move to the agent's next way of working.
    StepMode,
    /// Open what a row of the transcript names.
    GoTo(obelus_agent::acp::Place),
    /// Send the reader to this web address again.
    Away(String),
    /// Put this on the clipboard: a block of code, as it was written.
    Copy(String),
    /// Open the values of one of the agent's settings, by its id.
    Choose(String),
    /// Flip one of its switches, by its id.
    Toggle(String),
    /// Open the list of the work the agent goes on with in the background.
    Tasks,
}

/// What the conversation's keys are moving.
///
/// Two things are typed into or walked on this screen: the box, and the row
/// of what the session is set to. A key means one thing or the other
/// depending on which of them the reader is in, and there is no third.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// Somewhere in the transcript.
    Transcript(Place),
    /// The box. What is typed goes in it, and the caret is in it.
    #[default]
    Writing,
    /// One of the settings on the status row, by its place in the list --
    /// or one past the last of them, which is the count of the agent's
    /// background work where the row carries one.
    Settings(usize),
}

/// The words of a call that are not its own title over again.
///
/// An agent may send the line it put in the title as the call's content --
/// claude-agent-acp does it for every command it runs, because the tool's
/// description is what it sends as both -- and a copy is not a second thing
/// to read.
///
/// Two places have to know it and they have to agree. The row folds open on
/// what a call carries, and a call whose content is its heading again was
/// three rows saying one line. The card a question is asked on says nothing
/// while the row is carrying the question, and "carrying" cannot mean
/// "carrying anything" or a row with nothing on it but its title leaves the
/// card with no subject. One rule, asked here, so the two cannot drift.
pub fn its_own_words<'a>(title: &'a str, words: &'a [String]) -> impl Iterator<Item = &'a String> {
    words.iter().filter(move |said| said.trim() != title.trim())
}

/// Which of the texts of a thing said a row was laid out from.
///
/// A thing said is not one text. A tool call has a title, the words it
/// carries and the lines of the change it is making, and all three are
/// laid out into rows of the same transcript -- so a place in the
/// transcript has to say which of them it is in before it says where.
///
/// The order is the order they are drawn in, which is what lets two
/// places be compared: a point in the title comes before a point in the
/// words, whatever the widths and whatever is folded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// What it says: a message in the agent's or the reader's own words,
    /// or a tool call's title.
    Text,
    /// One of the things a call carries.
    Words(usize),
    /// One line of a change it is making.
    Change(usize),
}

/// A place in the transcript, as the transcript itself holds it.
///
/// Not a row and a column. Rows are what a width makes of what was said,
/// and they are made again every frame: a place kept as one moves when the
/// window is resized, and vanishes when something above it is folded away.
/// Bytes of what was actually said do neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot {
    /// Which of the things said.
    pub said: usize,
    /// Which of its texts.
    pub source: Source,
    /// How many bytes into that text.
    pub at: usize,
}

/// Where the cursor is in the transcript.
///
/// A row and a character of it, which is where it is on the screen rather
/// than where it is in the words. That is the opposite of how a selection
/// is kept, and on purpose.
///
/// A selection has to outlive the width it was made at, because the reader
/// makes one and then does something else -- reads on, folds a run, resizes
/// the terminal -- before they copy it. So both its ends are [`Spot`]s,
/// places in what was said.
///
/// A cursor is where the reader is looking while they are pressing keys,
/// and it has to be able to stand on rows that are in nobody's words at
/// all: the blank between two things said, a plan's steps, and -- the one
/// that settles it -- the heading over a folded run, which is the row
/// `enter` opens. Those rows have no place in the words to be, so a cursor
/// kept as a `Spot` could not point at the very thing this was for.
///
/// What crosses between the two is [`Row::spot_at`]: a shift-motion asks
/// the row under the cursor where in the words it is, and holds that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    /// Which row, among the rows as they are drawn now.
    pub row: usize,
    /// How many characters into it.
    ///
    /// Its length means the place after its last character, where a caret
    /// at the end of a line sits.
    pub character: usize,
}

/// What a conversation's rows were laid out against: a width, and a
/// version of `obelus_text`'s table of pictures.
type LaidAt = (u16, u64);

/// A conversation.
///
/// `Default` is `new` rather than derived, because the two differed and the
/// difference was invisible: a derived one gets a plain [`Window`], and a
/// transcript wants the one that stays at the end until the reader scrolls
/// up. A conversation built the wrong way looked right and stopped
/// following, which is the kind of thing a type should not let happen twice.
#[derive(Debug)]
pub struct Chat {
    /// What has been said, oldest first.
    said: Vec<Said>,
    /// The rows as they were last laid out, and what they were laid out
    /// against: the width, and which characters the window draws as
    /// pictures.
    ///
    /// Laying out a conversation means making rows of every word ever said
    /// in it, at the width of the moment -- wrapping it, reading the
    /// markdown, working out what folds. For a long morning that is tens of
    /// milliseconds, and it is asked for several times over for a single
    /// keypress: once by the keys, again to put the caret, again to draw.
    /// Handing back a copy of what was worked out is a twentieth of the
    /// cost of working it out again.
    ///
    /// What a reader has hold of is deliberately not in here. It is marked
    /// on to the rows *after* they come out of this, so dragging a
    /// selection across a morning's conversation does not lay it out again
    /// on every report the terminal sends.
    ///
    /// Dropped by anything that changes what there is to lay out. A stale
    /// one is a screen that has stopped saying what happened, so the rule
    /// is to drop it wherever there is a doubt: laying out again costs
    /// milliseconds and being wrong costs the reader their conversation.
    laid: std::cell::RefCell<Option<(LaidAt, Vec<Row>)>>,
    /// What is happening now, if anything is.
    ///
    /// One slot rather than a line of the transcript: a state has no
    /// history, and the next one replaces it. Which is also why it cannot
    /// go stale -- what is not stored cannot be left on screen saying
    /// something that has stopped being true.
    doing: Option<String>,
    /// Whether a turn is running and `ctrl+enter` can reach it, read off
    /// the state every frame the way [`Self::doing`] is: what decides
    /// whether the box offers to send now, which is drawn in the box and can
    /// take a row of it, and so has to be known wherever the box is measured.
    can_send_now: bool,
    /// What the agent means to do about this turn, while it is doing it.
    ///
    /// Never written into [`Self::said`]: a finished list of seven
    /// completed steps is a log, and what is kept of a turn is what the
    /// agent said and did rather than the order it meant to do it in. This
    /// is state, and it is drawn where state is drawn.
    plan: Vec<obelus_agent::acp::Step>,
    /// Whether the reader has opened it.
    ///
    /// Folded to its one row by default: while the agent works the
    /// transcript is filling with what the reader is watching, and a
    /// checklist holding a third of the screen for a whole turn is a
    /// checklist they did not ask for.
    plan_open: bool,
    /// What is being written.
    input: Composer,
    /// What the box offers to say while nothing is in it, if anything.
    ///
    /// Not in the box, because it is not the reader's until they take it:
    /// enter on it sends nothing, as it does on any empty box, and only the
    /// right arrow -- the key that would otherwise do nothing there -- puts
    /// it in as words they can send or change.
    suggested: Option<&'static str>,
    /// Which rows of the transcript are on screen.
    ///
    /// The same window every list in Obelus has, in its following form: it
    /// sits at the end until the reader scrolls up, and goes back to
    /// following when they come back down. Without that a streaming answer
    /// either drags the view around while it is being read, or arrives off
    /// screen with nothing to say so.
    window: Window,
    /// Which of the two things on this screen the keys are moving.
    focus: Focus,
    /// The two ends of what the reader has hold of, while they have hold
    /// of anything.
    ///
    /// The anchor and then the other end, in the order they were made
    /// rather than in the order they come: keeping the anchor is what lets
    /// a drag turn back on itself and shrink, and pass through where it
    /// started, without the two ends swapping meaning underneath it.
    held: Option<(Spot, Spot)>,
    /// How much had been said when the reader last left the end of it.
    ///
    /// `None` while they are at the end, which is where a transcript sits
    /// until they scroll up. What it is for is the one question somebody
    /// who has scrolled up actually has -- has it answered me yet -- and
    /// the answer is what arrived after they stopped looking.
    ///
    /// Taken and dropped by [`Chat::settle`] rather than by whatever
    /// scrolls, because there are four ways to leave the end and one of
    /// them is the list growing under a window that is not following. One
    /// place that asks "are we at the end now" cannot miss any of them.
    left_at: Option<usize>,
    /// Which column the cursor was in when it last left the transcript.
    ///
    /// The column and not the row, because the two do not keep the same
    /// way. Up from the box goes to the foot of the transcript, which is
    /// the words nearest the box and where whatever arrived while the
    /// reader was typing is; a row kept from before that is a number that
    /// has since come to mean different words, which is what [`Place`]
    /// says about itself and why [`Spot`] exists. So the row is asked for
    /// again and the column is the reader's.
    ///
    /// And the column is worth keeping: coming back to the end of the row
    /// rather than to the column left from put the cursor on the far side
    /// of the page, for anybody reading down a long answer who stepped one
    /// row too far and pressed up again.
    stood: Option<usize>,
}

impl Default for Chat {
    fn default() -> Self {
        Self::new()
    }
}

impl Row {
    /// What the row says, with nothing about how it is drawn.
    ///
    /// Derived from the runs rather than kept beside them: a row that
    /// carried both would be two answers to "what does this say", and the
    /// one that is drawn is the runs.
    #[must_use]
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_ref()).collect()
    }
}

impl Chat {
    /// An empty conversation, following its own end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            said: Vec::new(),
            laid: std::cell::RefCell::new(None),
            doing: None,
            can_send_now: false,
            plan: Vec::new(),
            plan_open: false,
            input: Composer::new(),
            suggested: None,
            window: Window::following(),
            focus: Focus::Writing,
            held: None,
            left_at: None,
            stood: None,
        }
    }

    /// Which of the two things on this screen the keys are moving.
    #[must_use]
    pub const fn focus(&self) -> Focus {
        self.focus
    }

    /// Keeps the focus on something that is still there.
    ///
    /// The settings are the agent's and it can change them mid-sentence --
    /// a model with no thinking levels takes that row away -- so the place
    /// the focus names has to be checked against the list that is really
    /// there, once a frame, like every other window in Obelus.
    ///
    /// `settings` is everything on the row the keys can stand on: the
    /// settings, and the count of background work after them where there
    /// is one.
    pub fn settle_focus(&mut self, settings: usize) {
        if let Focus::Settings(at) = self.focus {
            self.focus = match settings {
                0 => Focus::Writing,
                count => Focus::Settings(at.min(count - 1)),
            };
        }
    }

    /// Whether anything has been said.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.said.is_empty()
    }

    /// Whether anybody but Obelus has said anything in it.
    ///
    /// Which is not "is it empty": Obelus writes in a conversation of its
    /// own accord -- why the agent stopped, why the old one could not be
    /// taken up -- and a page holding nothing but that is a page with
    /// nothing to come back to. The difference matters where something is
    /// deciding whether this conversation is worth keeping a name for.
    #[must_use]
    pub fn anything_said(&self) -> bool {
        self.said.iter().any(|said| said.speaker != Speaker::Note)
    }

    /// The first line of the first thing the reader said, its blanks run
    /// together, for a conversation nobody has named yet.
    ///
    /// The first line rather than the whole message, because what comes
    /// after it is mostly pasted code or a log. And its words rather than
    /// its spelling where it has parts: a picture's label is Obelus's word
    /// for the picture, not the reader's for the conversation. A row taken
    /// up again has no parts, and is its words already.
    ///
    /// Not cut to any length: whatever draws it knows how much room it has.
    #[must_use]
    pub fn first_words(&self) -> Option<String> {
        let said = self
            .said
            .iter()
            .find(|said| said.speaker == Speaker::Reader)?;
        let words = match said.parts.is_empty() {
            true => said.text.clone(),
            false => said
                .parts
                .iter()
                .filter_map(|part| match part {
                    crate::composer::Part::Words(words) => Some(words.as_str()),
                    crate::composer::Part::Picture(_) => None,
                })
                .collect(),
        };
        words
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .find(|line| !line.is_empty())
    }

    /// Puts pasted text in the box, wherever the caret is.
    ///
    /// Into the box and nowhere else: a transcript is what was said, and the
    /// one place in a conversation that takes text is the half of it the
    /// reader is writing.
    ///
    /// And it takes the keys back with it, which a character typed in the
    /// transcript or on the row of settings does not: a paste is a reader
    /// putting something somewhere on purpose -- a terminal's own paste, or
    /// a file dragged onto one, which arrives as the words for its path --
    /// where a letter out there is as often a slip. Left where it was, the
    /// text went into a box the caret was not in. An input method, whose
    /// word also arrives as a paste, is off out there (`App::takes_text`),
    /// so a word spelled while reading does not come this way -- except on
    /// X11, where `obg` leaves the input method on throughout, and a word
    /// spelled in the transcript arrives here and goes in while the letters
    /// that spell Latin go nowhere. Turning it off there costs the box its
    /// keys until the window is left and come back to, which is worse.
    pub fn paste(&mut self, what: &str, width: u16) {
        self.take_the_keys_back();
        self.input.write_in(what, width);
    }

    /// Puts a picture in the box, wherever the caret is -- taking the keys
    /// back the way a paste does, because a picture dragged in is a paste
    /// of its path.
    pub fn attach(&mut self, picture: crate::composer::Attached, width: u16) {
        self.take_the_keys_back();
        self.input.attach(picture, width);
    }

    /// The keys back in the box, from wherever in the conversation they were.
    fn take_the_keys_back(&mut self) {
        match self.focus {
            Focus::Transcript(at) => {
                self.let_go();
                self.leave_the_transcript(at);
            }
            Focus::Settings(_) => self.focus = Focus::Writing,
            Focus::Writing => {}
        }
    }

    /// Says what the box offers while it is empty, or that it offers
    /// nothing.
    pub const fn suggest(&mut self, what: Option<&'static str>) {
        self.suggested = what;
    }

    /// What the box offers, while nothing at all has been put in it.
    ///
    /// Nothing at all rather than nothing but blanks: a space the reader
    /// typed is the start of something of theirs, and grey words beside it
    /// would be Obelus finishing their sentence.
    #[must_use]
    pub fn suggestion(&self) -> Option<&'static str> {
        self.suggested.filter(|_| self.input.text().is_empty())
    }

    /// What is being written, for the view to draw.
    #[must_use]
    pub const fn writing(&self) -> &Composer {
        &self.input
    }

    /// The same, to change: what a pointer landing in the box moves.
    pub const fn writing_mut(&mut self) -> &mut Composer {
        &mut self.input
    }

    /// The name of the command being typed, if the line is one.
    ///
    /// A message is a command when its first character is a slash and
    /// nothing else: that is the whole rule, and it is the reader's to
    /// invoke rather than Obelus's to guess at.
    #[must_use]
    pub fn typing_command(&self) -> Option<String> {
        let text = self.input.text();
        let rest = text.strip_prefix('/')?;
        // Only while the *name* is being typed. A blank after it settles
        // it: what follows is the command's own input, and a list of
        // commands over the box then has nothing left to offer.
        let settled = rest.contains(char::is_whitespace);
        (!settled).then(|| rest.to_string())
    }

    /// The first transcript row on screen.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.window.top()
    }

    /// Whether there is more of what was said than there is room for.
    ///
    /// What decides whether the transcript draws a bar, the way every other
    /// list in Obelus decides it: a bar on something that fits is a bar
    /// that says nothing.
    #[must_use]
    pub fn scrollable(&self, room: u16) -> bool {
        self.window.scrollable(room)
    }

    /// The window itself, for the view: what is on screen, and whether
    /// there is more of it than there is screen.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Puts this row at the top, for a bar the reader has hold of.
    pub fn drag_to(&mut self, top: usize) {
        self.window.drag_to(top);
    }

    /// Adds a line of the reader's own.
    pub fn asked(&mut self, parts: &[crate::composer::Part]) {
        self.push(Speaker::Reader, &Composer::spelling(parts), None);
        if let Some(said) = self.said.last_mut() {
            said.parts = parts.to_vec();
        }
    }

    /// Adds one the reader has said into a turn that is still running.
    ///
    /// It goes on the page where everything they say goes, and says about
    /// itself that it has not gone. What it is waiting for is the turn in
    /// front of it: [`Self::unsent`] is what leaves with that turn.
    pub fn will_say(&mut self, parts: &[crate::composer::Part]) {
        self.push(Speaker::Reader, &Composer::spelling(parts), None);
        if let Some(said) = self.said.last_mut() {
            said.parts = parts.to_vec();
            said.unsent = true;
        }
    }

    /// The same, for words that came from a chat.
    pub fn will_say_from_afar(&mut self, parts: &[crate::composer::Part]) {
        self.will_say(parts);
        if let Some(said) = self.said.last_mut() {
            said.afar = true;
        }
    }

    /// Of what has not gone, which came from a chat, in the same order as
    /// [`Self::unsent`].
    #[must_use]
    pub fn unsent_afar(&self) -> Vec<bool> {
        self.said
            .iter()
            .filter(|said| said.unsent)
            .map(|said| said.afar)
            .collect()
    }

    /// Says whether a turn is running that `ctrl+enter` can reach.
    pub const fn can_send_now(&mut self, can: bool) {
        self.can_send_now = can;
    }

    /// Whether the box offers to send now: a turn is running that the key
    /// can reach, and there are words in the box for it to send.
    ///
    /// The box and not what is waiting: the offer is about what is being
    /// written, and over an empty box it reads as a key for nothing. What
    /// is waiting goes with the words when there are some, and goes back
    /// into the box with escape when there are not.
    #[must_use]
    pub fn offers_sending_now(&self) -> bool {
        self.can_send_now && !self.input.is_blank()
    }

    /// Everything the reader has said that has not gone, in the order they
    /// said it.
    ///
    /// All of it, because all of it goes as one prompt: three things
    /// typed into one running turn are one thing the reader is saying, and
    /// an agent given only the first of them answers a question it has not
    /// been asked the whole of.
    #[must_use]
    pub fn unsent(&self) -> Vec<Vec<crate::composer::Part>> {
        self.said
            .iter()
            .filter(|said| said.unsent)
            .map(|said| said.parts.clone())
            .collect()
    }

    /// Whether the thing said at `at` is one of those.
    #[must_use]
    pub fn waits(&self, at: usize) -> bool {
        self.said.get(at).is_some_and(|said| said.unsent)
    }

    /// Says that what was waiting has gone.
    ///
    /// The rows stay as they are and stop being dim. They are not merged
    /// into the one prompt they left as: the reader said three things and
    /// the page is what they said, so rewriting their own half of it under
    /// them would be Obelus editing the page rather than adding to it.
    pub fn sent(&mut self) {
        if !self.said.iter().any(|said| said.unsent) {
            return;
        }
        // The dim ink is laid out from this, so the rows it made are no
        // longer the rows it would make.
        self.forget_the_layout();
        for said in &mut self.said {
            said.unsent = false;
        }
    }

    /// Takes back one thing the reader said before it went, and hands the
    /// words back.
    ///
    /// Only something that has not gone: a thing already said to an agent
    /// cannot be unsaid, and a row that has gone does not offer this.
    pub fn take_back(&mut self, at: usize) -> Option<Vec<crate::composer::Part>> {
        if !self.said.get(at).is_some_and(|said| said.unsent) {
            return None;
        }
        self.forget_the_layout();
        Some(self.said.remove(at).parts)
    }

    /// A copy of something the reader said that has gone, to say again.
    ///
    /// Its words when it has no parts: what comes back from a conversation
    /// taken up again is the agent's copy of it, which is text and nothing
    /// else.
    #[must_use]
    pub fn again(&self, at: usize) -> Option<Vec<crate::composer::Part>> {
        let said = self
            .said
            .get(at)
            .filter(|said| said.speaker == Speaker::Reader)?;
        Some(match said.parts.is_empty() {
            true => vec![crate::composer::Part::Words(said.text.clone())],
            false => said.parts.clone(),
        })
    }

    /// Takes back everything the reader said that has not gone, joined the
    /// way it would have gone.
    ///
    /// And lets go of whatever the transcript held: a hold is a place in
    /// what was said, and with these out of it the place names other words.
    pub fn take_back_waiting(&mut self) -> Option<Vec<crate::composer::Part>> {
        if !self.said.iter().any(|said| said.unsent) {
            return None;
        }
        self.forget_the_layout();
        let (waiting, kept): (Vec<Said>, Vec<Said>) = std::mem::take(&mut self.said)
            .into_iter()
            .partition(|said| said.unsent);
        self.said = kept;
        self.let_go();
        let mut parts = Vec::new();
        for said in waiting {
            if !parts.is_empty() {
                parts.push(crate::composer::Part::Words("\n\n".to_string()));
            }
            parts.extend(said.parts);
        }
        Some(parts)
    }

    /// Takes the reader's own words back from the agent.
    ///
    /// What this is for is a conversation taken up again: the agent replays
    /// it to a client that may be a fresh process, and the reader's half of
    /// it comes back only this way.
    ///
    /// Some agents also send these during a live turn, echoing back the
    /// prompt they were just given -- which Obelus put on the page itself
    /// the moment it was sent. So a chunk the last thing said already says
    /// is dropped: what makes that safe is that the only rows Obelus writes
    /// in this voice are the ones it was handed by the reader, so a repeat
    /// of what is already there is the agent's copy of it and not a second
    /// thing they said.
    pub fn heard(&mut self, text: &str) {
        self.forget_the_layout();
        let echoed = self
            .said
            .last()
            .is_some_and(|last| last.speaker == Speaker::Reader && last.text.contains(text));
        if !echoed {
            self.chunk(Speaker::Reader, text);
        }
    }

    /// Adds one of Obelus's own remarks.
    pub fn note(&mut self, text: &str) {
        self.push(Speaker::Note, text, None);
    }

    /// The one row a folded plan is: which step it is on, and what it is.
    ///
    /// The one being worked on, because that is what "now" means. With
    /// none of them under way -- it has said what it will do and not
    /// started -- there is no step to name, so it says how many there are.
    fn step_now(&self) -> String {
        let at = self
            .plan
            .iter()
            .position(|step| step.state == "in_progress");
        let total = self.plan.len();
        match at.and_then(|at| Some((at, self.plan.get(at)?))) {
            Some((at, step)) => format!("Step {} of {total} \u{2014} {}", at + 1, step.said),
            None => format!("{total} steps"),
        }
    }

    /// Takes the agent's list for this turn, replacing whatever it had.
    ///
    /// Replaced and not merged, because that is what the protocol says an
    /// update is: the whole list with every entry's status, every time.
    pub fn planning(&mut self, steps: Vec<obelus_agent::acp::Step>) {
        self.forget_the_layout();
        self.plan = steps;
    }

    /// Forgets it, which a new turn does.
    ///
    /// An agent that sends a plan for one turn and none for the next would
    /// otherwise have the first one shown against the second's work.
    pub fn plan_forgotten(&mut self) {
        self.forget_the_layout();
        self.plan.clear();
        self.plan_open = false;
    }

    /// Says what is happening now, or that nothing is.
    ///
    /// Called with whatever the state is, every frame: it is read from the
    /// state rather than remembered, so there is no way for it to be left
    /// behind.
    pub fn doing(&mut self, what: Option<&str>) {
        // Nothing at all where nothing has moved, which is almost every
        // call: this is asked twelve times a second and answers the same
        // thing. Throwing the rows away here laid the whole conversation
        // out again on every frame -- 75ms of it on a transcript of a
        // thousand rows, against 213us for the rows it already had -- so
        // walking the cursor down a long turn was a keypress the reader
        // could watch arrive.
        if self.doing.as_deref() == what {
            return;
        }
        // Written down when it moves. This is the row a reader watches to
        // know whether anything is happening at all, and when it says
        // nothing there is nothing else on screen to say why -- so a
        // report that it stayed blank is a report with no evidence in it,
        // and the log is where Obelus keeps what a reader cannot show.
        tracing::info!(was = ?self.doing, now = ?what, "what is happening now");
        self.forget_the_layout();
        self.doing = what.map(str::to_string);
    }

    /// Takes a piece of an answer, or of the agent's thinking.
    ///
    /// Chunks arrive a few words at a time, so a chunk that continues what
    /// the same speaker was saying is appended rather than started: one
    /// paragraph per thing said, not one per packet.
    pub fn chunk(&mut self, speaker: Speaker, text: &str) {
        match self.said.last_mut() {
            Some(last) if last.speaker == speaker && last.tag.is_none() => {
                last.text.push_str(text);
                // The rows this was laid out into are rows of the text as
                // it was a packet ago. Every other way of changing what
                // there is to lay out drops them; this one appends to a
                // string in place, which is exactly the change that is
                // easy to make without noticing it is one.
                self.forget_the_layout();
            }
            _ => self.push(speaker, text, None),
        }
    }

    /// The last thing the agent said, whole, where it has said anything.
    ///
    /// For somewhere that cannot be shown the transcript -- a chat thread
    /// opened on a conversation already going -- which wants the one
    /// paragraph that says where things are, not the whole of how they got
    /// there.
    #[must_use]
    pub fn lately(&self) -> Option<String> {
        self.said
            .iter()
            .rev()
            .find(|said| said.speaker == Speaker::Agent && said.tag.is_none())
            .map(|said| said.text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    /// Says the reader was sent somewhere, and that what was to happen
    /// there has not happened yet.
    ///
    /// Tagged with the agent's own name for the question, which is how
    /// [`Chat::arrived`] finds it again: the agent watches the far end and
    /// names this when it sees it.
    pub fn away(&mut self, id: &str, message: &str, url: &str) {
        self.forget_the_layout();
        self.said.push(Said {
            parts: Vec::new(),
            speaker: Speaker::Away,
            text: message.to_string(),
            tag: Some(id.to_string()),
            state: Some("in_progress".to_string()),
            kind: String::new(),
            places: Vec::new(),
            change: Vec::new(),
            ran: None,
            // Kept rather than shown: the row says what it was for, and
            // the address is what pressing the key on it does.
            words: vec![url.to_string()],
            opened: None,
            run_opened: None,
            unsent: false,
            afar: false,
        });
    }

    /// Every command a row of this conversation is about.
    #[must_use]
    pub fn commands(&self) -> Vec<String> {
        self.said
            .iter()
            .filter_map(|said| said.ran.clone())
            .collect()
    }

    /// Marks every call that had not finished as one nobody finished.
    ///
    /// For the turn the reader stopped. A call's state is the agent's, and
    /// an agent that is told to stop is asked to send the updates it owes
    /// -- but one that never saw the cancellation does not, and its calls
    /// sit at `in_progress` for ever: a mark that says a thing is running
    /// while Obelus has told the reader nothing is. `cancelled` is the
    /// protocol's own word for it, so this is Obelus writing down what the
    /// agent would have said rather than a state of its own invention.
    ///
    /// The whole conversation rather than the last turn, because Obelus
    /// does not keep the boundary between turns: a call from a turn that
    /// ended properly is not in one of these states to begin with.
    pub fn stop_the_calls(&mut self) {
        let mut any = false;
        for said in &mut self.said {
            if matches!(said.state.as_deref(), Some("pending" | "in_progress")) {
                said.state = Some("cancelled".to_string());
                any = true;
            }
        }
        if any {
            self.forget_the_layout();
        }
    }

    /// Says what a command is doing, for every row that is running one.
    ///
    /// Called every frame with whatever Obelus's own runner has, the way
    /// [`Chat::doing`] is: the output belongs to the process and the rows
    /// are drawn from it rather than from a copy that could be a moment
    /// behind. Which command a row is about is the row's own `ran`, so a
    /// caller hands over one answer per command and this finds them.
    pub fn running(&mut self, what: &dyn Fn(&str) -> Option<Doing>) {
        // Only where something moved, for the reason [`Chat::doing`] gives:
        // this is asked on every frame and a command that has printed
        // nothing since the last one has nothing to lay out again.
        let mut moved = false;
        for said in &mut self.said {
            let Some(id) = said.ran.as_deref() else {
                continue;
            };
            let Some(Doing { words, state }) = what(id) else {
                continue;
            };
            // The command and what it has printed, in the place a call's
            // own words go: for a call that is a command, this *is* what
            // it says.
            if said.words.len() != 1 || said.words[0] != words {
                said.words = vec![words];
                moved = true;
            }
            // And how it is going, which is the call's state rather than a
            // second mark: a call running a command that failed is a call
            // that failed.
            if let Some(state) = state
                && said.state.as_deref() != Some(state.as_str())
            {
                said.state = Some(state);
                moved = true;
            }
        }
        if moved {
            self.forget_the_layout();
        }
    }

    /// Says what was to happen where the reader was sent has happened.
    ///
    /// Nothing at all for a name nothing is waiting on: an agent may say a
    /// question is over that this Obelus never asked -- a second one is
    /// listening to the same session -- and a row invented to mark it done
    /// would be a row about something the reader never did.
    pub fn arrived(&mut self, id: &str) {
        self.forget_the_layout();
        if let Some(said) = self
            .said
            .iter_mut()
            .rev()
            .find(|said| said.speaker == Speaker::Away && said.tag.as_deref() == Some(id))
        {
            said.state = Some("completed".to_string());
        }
    }

    /// Takes news of a tool call: a new one, or the same one further along.
    pub fn tool(&mut self, call: &obelus_agent::acp::Call, status: &str) {
        self.forget_the_layout();
        let existing = self
            .said
            .iter_mut()
            .rev()
            .find(|said| said.tag.as_deref() == Some(call.id.as_str()));
        // A call that returned while what it started goes on has not
        // completed anything a reader would call done.
        let status = match (call.backgrounded, status) {
            (true, "completed") => obelus_agent::acp::BACKGROUNDED,
            _ => status,
        };
        let Some(said) = existing else {
            self.said.push(Said {
                parts: Vec::new(),
                speaker: Speaker::Tool,
                text: call.title.clone(),
                tag: Some(call.id.clone()),
                state: Some(status.to_string()),
                kind: call.kind.clone(),
                places: places_of(call),
                change: changed_rows(call),
                ran: call.ran.clone(),
                words: call.said.clone(),
                opened: None,
                run_opened: None,
                unsent: false,
                afar: false,
            });
            return;
        };
        // A later update carries only what changed, so what arrives empty
        // means "the one you already have" -- for the title, the kind, the
        // places it named, the change it is making and where it has got to.
        if !call.title.is_empty() {
            said.text = call.title.clone();
        }
        if !call.kind.is_empty() {
            said.kind = call.kind.clone();
        }
        let places = places_of(call);
        if !places.is_empty() {
            said.places = places;
        }
        if call.change.is_some() {
            said.change = changed_rows(call);
        }
        // Replaced, the way the change above it is, because that is what
        // the protocol says a later `content` means: *replace the content
        // collection*. What a call says is what it says now, not a log of
        // what it has said -- a plan put to the reader is replaced by what
        // became of the asking, and that is the agent's account of that
        // call rather than something for Obelus to overrule.
        if !call.said.is_empty() {
            said.words = call.said.clone();
        }
        if !status.is_empty() {
            // Said once, by the update that carried the marker; the ones
            // after it repeat `completed` without it, and are not news.
            let kept = said.state.as_deref() == Some(obelus_agent::acp::BACKGROUNDED)
                && status == "completed";
            if !kept {
                said.state = Some(status.to_string());
            }
        }
    }

    /// What the work a call started has come to, said on the call's row:
    /// [`obelus_agent::acp::BACKGROUNDED`] while it goes on, and the
    /// protocol's own word for how a call ends once it has.
    ///
    /// The work's word and not the row's: once a call has returned, what
    /// its row says is whatever the work last said, whichever order the two
    /// arrived in -- a work that was reported stopped and then taken back,
    /// or one that ended before the call that started it was marked. A call
    /// still under way is left alone, because it has not returned yet.
    ///
    /// Whatever the call itself ended as, too. A call the reader stopped, or
    /// one that failed, may have left work running all the same, and a row
    /// saying `Stopped` over a server that is still up is the one thing
    /// this row must not say.
    ///
    /// Forgets the rows only where one changed: news of the work arrives
    /// with every beat of its progress, and almost none of it moves the row.
    pub fn background_says(&mut self, id: &str, state: &str) {
        let Some(said) = self.call_mut(id) else {
            return;
        };
        let under_way = matches!(said.state.as_deref(), Some("pending" | "in_progress"));
        if under_way || said.state.as_deref() == Some(state) {
            return;
        }
        said.state = Some(state.to_string());
        self.forget_the_layout();
    }

    /// Every row still saying its work goes on, ended as stopped: nothing is
    /// left that could say otherwise.
    pub fn background_ended_everywhere(&mut self) {
        let mut changed = false;
        for said in &mut self.said {
            if said.state.as_deref() == Some(obelus_agent::acp::BACKGROUNDED) {
                said.state = Some("cancelled".to_string());
                changed = true;
            }
        }
        if changed {
            self.forget_the_layout();
        }
    }

    /// The row of one call, by its id.
    fn call_mut(&mut self, id: &str) -> Option<&mut Said> {
        self.said
            .iter_mut()
            .rev()
            .find(|said| said.speaker == Speaker::Tool && said.tag.as_deref() == Some(id))
    }

    /// The file the call with this id changed, once it has.
    ///
    /// Asked of the row rather than of the update, because an update says
    /// only what changed: the kind and the place arrive with the call, and
    /// that it is done arrives later on its own. Done and not asked: a
    /// change put to the reader for permission has not happened, and may
    /// not.
    #[must_use]
    pub fn wrote(&self, id: &str) -> Option<&std::path::Path> {
        let said = self
            .said
            .iter()
            .rev()
            .find(|said| said.tag.as_deref() == Some(id))?;
        let changes =
            matches!(said.kind.as_str(), "edit" | "delete" | "move") || !said.change.is_empty();
        (changes && said.state.as_deref() == Some("completed"))
            .then(|| said.places.first())
            .flatten()
            .map(|place| place.path.as_path())
    }

    /// Opens what is closed and closes what is open.
    pub fn fold(&mut self, what: Folds) {
        self.forget_the_layout();
        let open = self.is_open_now(what);
        match what {
            Folds::Plan => self.plan_open = !open,
            Folds::Said(at) => {
                if let Some(said) = self.said.get_mut(at) {
                    said.opened = Some(!open);
                }
            }
            Folds::Run(at) => {
                if let Some(said) = self.said.get_mut(at) {
                    said.run_opened = Some(!open);
                }
            }
        }
    }

    /// Moves the window if it has to, once a frame.
    ///
    /// Following the end is the ordinary state, so this is where a streaming
    /// answer gets shown: the rows arrive between frames and the window
    /// follows them. A reader who has scrolled up keeps their place, and the
    /// window is only pulled back if the transcript shrank under it.
    pub fn settle(&mut self, rows: usize, room: u16) {
        self.window.set_count(rows);
        self.window.settle(room);
        // Where the reader left, so that what has arrived since can be
        // counted. Taken on the first frame they are away and dropped the
        // moment they are back.
        match self.window.at_the_end() {
            true => self.left_at = None,
            false => {
                self.left_at.get_or_insert(self.said.len());
            }
        }
    }

    /// Takes hold of a place in the transcript, letting go of anything
    /// else.
    pub fn hold_from(&mut self, spot: Spot) {
        self.held = Some((spot, spot));
    }

    /// Drags the far end of it to another place.
    ///
    /// The anchor stays where it was put, which is what lets a drag turn
    /// back on itself: the two ends are compared when they are used, not
    /// stored in order.
    pub fn hold_to(&mut self, spot: Spot) {
        if let Some((_, other)) = self.held.as_mut() {
            *other = spot;
        }
    }

    /// Forgets the rows, because what they are made of has changed.
    ///
    /// Called by everything that changes what there is to lay out: by
    /// [`Chat::push`], which is the one door for everything that adds
    /// something said, and by each of the ones that reach into what is
    /// already there -- a call's state, an address answered, a run opened,
    /// the plan, what is happening now.
    ///
    /// The cost of calling it where it was not needed is one laying out.
    /// The cost of not calling it where it was is a screen that has
    /// stopped saying what happened, which is why the test that walks
    /// every one of them is where the promise really lives.
    fn forget_the_layout(&mut self) {
        self.laid.get_mut().take();
    }

    /// Puts the keys on one of the settings on the status row.
    ///
    /// For a pointer: the keys walk along the row and have no use for
    /// naming one outright, and a press names one.
    pub fn stand_on_setting(&mut self, at: usize) {
        self.focus = Focus::Settings(at);
    }

    /// Puts the keys on a place in the transcript, for a press there.
    ///
    /// Without it a press took hold of words and left the keys in the box,
    /// so the arrows after it walked something the reader had not pointed
    /// at.
    pub fn stand_in_transcript(&mut self, at: Place) {
        self.focus = Focus::Transcript(at);
    }

    /// Puts the keys back in the box, for a press on it.
    ///
    /// A caret placed in the box while the keys were in the transcript or
    /// on the row of settings was a caret drawn where nothing typed would
    /// go: the press moved it and the keys stayed where they were.
    pub fn stand_in_the_box(&mut self) {
        match self.focus {
            Focus::Transcript(at) => self.leave_the_transcript(at),
            Focus::Settings(_) => self.focus = Focus::Writing,
            Focus::Writing => {}
        }
    }

    /// Lets go of whatever was held.
    pub const fn let_go(&mut self) {
        self.held = None;
    }

    /// Whether what is held is a press at `at` that never became a drag,
    /// which is what a click there leaves.
    ///
    /// At the place it was let go, as well as unmoved: a press on one link
    /// let go on another, with nothing reported between, is not a click on
    /// either.
    #[must_use]
    pub fn clicked(&self, at: Spot) -> bool {
        self.held.is_some_and(|(from, to)| from == to && from == at)
    }

    /// Lets go of a hold with nothing in it, which is what a press that
    /// never became a drag leaves.
    ///
    /// Nothing is drawn for one, and it was not nothing: escape spent its
    /// first press letting go of it rather than emptying the box, and a
    /// shift and an arrow held from where the pointer had been rather than
    /// from the cursor.
    pub fn let_go_of_nothing(&mut self) {
        if self.held.is_some_and(|(from, to)| from == to) {
            self.held = None;
        }
    }

    /// Whether anything in the transcript is held.
    #[must_use]
    pub const fn holding(&self) -> bool {
        self.held.is_some()
    }

    /// What is held, as it is drawn.
    ///
    /// Which is the whole of the promise: a reader takes a copy of what is
    /// on the screen in front of them. So it is read off the rows -- a
    /// fold that has put something away has put it out of this too, and
    /// the marks a reading draws between held words come with them.
    #[must_use]
    pub fn held_text(&self, width: u16) -> Option<String> {
        self.held?;
        let said: Vec<String> = self
            .rows(width)
            .into_iter()
            .filter_map(|row| {
                let held = row.held.clone()?;
                Some(row.text().chars().take(held.end).skip(held.start).collect())
            })
            .collect();
        (!said.is_empty()).then(|| said.join("\n"))
    }

    /// Whether the transcript is showing its own end.
    ///
    /// Which is where it sits until the reader scrolls up: everything
    /// arrives at the end, so the end is where they are unless they said
    /// otherwise.
    #[must_use]
    pub const fn at_the_end(&self) -> bool {
        self.window.at_the_end()
    }

    /// How many times the agent has spoken since the reader left the end.
    ///
    /// The agent's own words and nothing else. A turn is a dozen tool
    /// calls and a paragraph, and what somebody who has scrolled up wants
    /// to know is whether it has answered them -- not how much machinery
    /// went past. Obelus's own notes are not news either: they are Obelus
    /// talking about the conversation rather than anything said in it.
    ///
    /// A streaming answer is one of these and stays one: chunks are
    /// appended to what the agent was already saying, so the count does
    /// not climb while a single answer is being written.
    #[must_use]
    pub fn said_since(&self) -> usize {
        let Some(left_at) = self.left_at else {
            return 0;
        };
        self.said
            .iter()
            .skip(left_at)
            .filter(|said| said.speaker == Speaker::Agent)
            .count()
    }

    /// Scrolls the transcript, for the wheel.
    ///
    /// Which is not a key: it moves the view and leaves the caret in the
    /// box where the reader put it.
    pub fn scroll(&mut self, rows: isize) {
        // And what the cursor kept is a column, which says nothing about
        // where the reader is looking. Coming in from the box goes to the
        // foot of the transcript whether the wheel has moved or not, so
        // there is nothing here to forget.
        self.scroll_by(rows);
    }

    /// What a copy takes out of a conversation: what is held in the
    /// transcript, or what is held in the box, or the whole of what has
    /// been written.
    ///
    /// The transcript first, because taking hold of it is the plainest way
    /// a reader has of saying *this* -- and because only one of the two is
    /// ever held: taking hold of either lets the other go.
    ///
    /// And the box's own rule after it, which is the rule the file follows
    /// with its line and the notes follow with their note: copying nothing
    /// is not something a key can usefully do.
    #[must_use]
    pub fn copied(&self, width: u16) -> (String, &'static str) {
        if let Some(held) = self.held_text(width) {
            return (held, "selection");
        }
        match self.input.selected() {
            Some(held) => (held, "selection"),
            None => (self.input.text(), "message"),
        }
    }

    /// The same, and takes it out.
    pub fn cut(&mut self, room: u16) -> (String, &'static str) {
        match self.input.cut(room.max(1)) {
            Some(held) => (held, "selection"),
            None => (self.input.take(), "message"),
        }
    }

    /// Puts a whole message in the box, with the caret after it.
    ///
    /// For completing a command from the list of them: what the reader
    /// typed is replaced by the whole name, and they carry on typing its
    /// input after it.
    pub fn put(&mut self, words: &str) {
        self.input.replace(words);
    }

    /// Puts what the reader took back into the box, in front of whatever
    /// they had started typing and as its own paragraph: neither of the two
    /// is Obelus's to throw away.
    pub fn put_back(&mut self, mut parts: Vec<crate::composer::Part>) {
        if !self.input.is_blank() {
            parts.push(crate::composer::Part::Words("\n\n".to_string()));
            parts.extend(self.input.parts());
        }
        self.input.put_parts(parts);
    }

    /// Adds something said, and keeps the view at the end.
    fn push(&mut self, speaker: Speaker, text: &str, tag: Option<String>) {
        self.forget_the_layout();
        self.said.push(Said {
            speaker,
            text: text.to_string(),
            tag,
            state: None,
            kind: String::new(),
            parts: Vec::new(),
            places: Vec::new(),
            change: Vec::new(),
            ran: None,
            words: Vec::new(),
            opened: None,
            run_opened: None,
            unsent: false,
            afar: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::composer::Part;

    /// One call that failed does not throw the run it is in open.
    ///
    /// A run is a log: Obelus makes one out of a stretch of calls of a kind
    /// exactly because nobody reads a log line by line. It opened itself
    /// whenever any call in it had failed, and "failed" is a word from the
    /// agent, said of a `grep` that matched nothing as readily as of a
    /// build that fell over -- so one call in eight threw all eight open,
    /// with every line of their output, over a cross on the heading. Seven
    /// of them had nothing to say.
    ///
    /// The heading still carries the worst state of what is under it, which
    /// is the whole of what a shut run owes the reader: something in here
    /// failed, and here is the key.
    ///
    /// It makes three claims and was broken deliberately three times.
    /// Opening a run whose calls include a failure is the fault itself.
    /// Opening the failed call on its own is the same fault one row down.
    /// And dropping the worst state from the heading leaves a shut run
    /// saying everything went well.
    /// A message that waits for a running turn keeps its pictures.
    ///
    /// What the reader says into a running turn goes on the page and leaves
    /// with the next turn, and a picture is part of what they said. The row
    /// carries the parts as well as the words for exactly this: the words
    /// are what the page shows and the parts are what goes, written at the
    /// same moment by the same call so they cannot come apart.
    ///
    /// Without it the page would show `[Image 1]` on a row whose message
    /// reaches the agent with the picture gone -- a reader asking about
    /// something the agent was never shown.
    ///
    /// Broken deliberately by having `unsent` answer with each row's `text`
    /// again: the words come back and the picture does not.
    #[test]
    fn a_message_that_waits_keeps_its_pictures() {
        let picture = Part::Picture(crate::composer::Attached {
            mime: "image/png".to_string(),
            bytes: vec![1, 2, 3],
        });
        let mut chat = Chat::new();
        chat.will_say(&[
            Part::Words("look at ".to_string()),
            picture.clone(),
            Part::Words(" please".to_string()),
        ]);

        assert_eq!(
            chat.unsent(),
            vec![vec![
                Part::Words("look at ".to_string()),
                picture,
                Part::Words(" please".to_string()),
            ]],
            "what waits lost the picture between its words"
        );
    }

    /// A conversation's first words are the first line the reader wrote in
    /// it, blanks run together -- not Obelus's remark before it, not a
    /// picture's label, and not what they said next.
    ///
    /// Broken deliberately four ways: answering the first row of anybody's
    /// reads `Starting again`; answering the row's `text` reads `[Image 1]`;
    /// taking the whole message rather than its first line keeps the second
    /// line; and leaving the blanks as they came keeps the run of spaces.
    #[test]
    fn the_first_words_are_the_first_line_the_reader_wrote() {
        let picture = Part::Picture(crate::composer::Attached {
            mime: "image/png".to_string(),
            bytes: vec![1, 2, 3],
        });
        let mut chat = Chat::new();
        assert_eq!(chat.first_words(), None);
        chat.note("Starting again");
        chat.asked(&[
            picture,
            Part::Words("\n  fix   the counts\nand everything after them".to_string()),
        ]);
        chat.asked(&[Part::Words("and then this".to_string())]);
        assert_eq!(chat.first_words().as_deref(), Some("fix the counts"));

        // Taken up again, where what the reader said is the agent's copy
        // and has no parts.
        let mut chat = Chat::new();
        chat.heard("what   next\nsecond");
        assert_eq!(chat.first_words().as_deref(), Some("what next"));
    }

    #[test]
    fn one_call_that_failed_does_not_throw_the_run_open() {
        let mut chat = Chat::new();
        for (index, state) in ["completed", "failed", "completed"].iter().enumerate() {
            let mut one = call(
                &format!("t{index}"),
                &format!("command {index}"),
                "execute",
                vec![],
            );
            one.said = vec![format!("output {index}")];
            chat.tool(&one, state);
        }

        let rows = chat.rows(ROOM.reading);
        assert_eq!(
            rows.len(),
            1,
            "one call that failed threw the whole run open: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        assert_eq!(
            rows[0].state.as_deref(),
            Some("failed"),
            "a shut run says nothing about the failure inside it"
        );

        // And the failed call, once the run is open, is shut like the rest
        // of them until the reader says otherwise.
        chat.fold(rows[0].folds.expect("the run folds"));
        let rows = chat.rows(ROOM.reading);
        assert_eq!(
            rows.len(),
            4,
            "the members are not one row each: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        assert!(
            !rows.iter().any(|row| row.text() == "output 1"),
            "the call that failed opened itself: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
    }

    /// A run and the first call in it fold separately.
    ///
    /// A run is named by the call it begins at -- there is nothing else to
    /// name it by -- and for as long as a fold was only an index, the
    /// heading and that call asked the same question and wrote the same
    /// answer. So the first call of an opened run wore the run's arrow,
    /// and enter on it shut the whole run: the one call in a run a reader
    /// could not put away on its own was the first, which is the one they
    /// reach first.
    ///
    /// It makes three claims and was broken deliberately three times.
    /// Giving the run `Folds::Said(run.start)` puts them back on one
    /// answer. Keeping a run's state in `opened` beside the call's does
    /// the same through the other door. And folding the first call by the
    /// run's key shuts the run instead of the call.
    #[test]
    fn a_run_and_the_first_call_in_it_fold_separately() {
        let mut chat = Chat::new();
        for index in 0..3 {
            let mut one = call(
                &format!("t{index}"),
                &format!("command {index}"),
                "execute",
                vec![],
            );
            one.said = vec![format!("output {index}")];
            chat.tool(&one, "completed");
        }
        // Three of a kind is a run, and a run arrives folded.
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 1, "three calls are not a run: {rows:?}");
        let heading = rows[0].folds.expect("the run's heading folds something");
        assert_eq!(heading, Folds::Run(0), "the run is not named as a run");

        // Opened, its first call is a row of its own -- and what that row
        // folds is the call, not the run it begins.
        chat.fold(heading);
        let rows = chat.rows(ROOM.reading);
        let first = rows[1].folds.expect("the first call folds something");
        assert_eq!(first, Folds::Said(0), "the first call still folds the run");
        assert_ne!(first, heading, "the run and its first call are one fold");

        // And pressing it opens that call, leaving the run open.
        chat.fold(first);
        let rows = chat.rows(ROOM.reading);
        assert!(
            rows.len() > 4,
            "opening the first call shut something else: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        assert_eq!(rows[0].text(), "3 Calls", "the run did not stay open");
        assert!(
            rows.iter().any(|row| row.text() == "output 0"),
            "the first call did not open: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        // The others are untouched: one call's fold is one call's.
        assert!(
            !rows.iter().any(|row| row.text() == "output 1"),
            "opening one call opened its neighbours: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
    }

    /// A frame that changes nothing does not lay the conversation out again.
    ///
    /// Both of these are asked on every frame -- what is happening now, and
    /// what the commands Obelus is running have printed -- and both threw
    /// the rows away before looking at whether the answer had moved. So
    /// every keypress laid the whole conversation out from its bytes:
    /// 75ms of it on a transcript of a thousand rows, against 213us for
    /// the rows it already had. Walking the cursor down a long turn was a
    /// keypress a reader could watch arrive.
    ///
    /// The other half is what makes that safe, and is asserted here too: an
    /// answer that *has* moved still drops them, or the row says what was
    /// happening a minute ago.
    ///
    /// Broken deliberately by putting `forget_the_layout()` back at the top
    /// of either one, which fails the first pair, or by dropping the call
    /// from the moved branch, which fails the second.
    #[test]
    fn a_frame_that_changes_nothing_does_not_lay_the_conversation_out_again() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "a word about it");
        chat.tool(
            &saying("c1", "Run the tests", &["nothing yet"]),
            "in_progress",
        );
        chat.said[1].ran = Some("r1".to_string());

        let held = |chat: &Chat| chat.laid.borrow().is_some();

        chat.doing(Some("Thinking\u{2026}"));
        let _ = chat.rows(ROOM.reading);
        assert!(held(&chat), "the rows were not kept at all");
        chat.doing(Some("Thinking\u{2026}"));
        assert!(
            held(&chat),
            "the same answer about what is happening threw the rows away"
        );
        chat.doing(Some("starting\u{2026}"));
        assert!(
            !held(&chat),
            "what is happening changed and the rows stayed as they were"
        );

        let printed = |words: &str| {
            let words = words.to_string();
            move |_: &str| {
                Some(Doing {
                    words: words.clone(),
                    state: None,
                })
            }
        };
        let _ = chat.rows(ROOM.reading);
        chat.running(&printed("$ cargo test"));
        assert!(
            !held(&chat),
            "a command that printed something kept the rows"
        );
        let _ = chat.rows(ROOM.reading);
        chat.running(&printed("$ cargo test"));
        assert!(
            held(&chat),
            "a command that printed nothing new threw the rows away"
        );
    }

    /// A call's row says what its work says once the call has returned,
    /// however the call ended -- and nothing while the call is under way.
    ///
    /// Broken deliberately: have `background_says` change only a row that
    /// says `completed` or that its work goes on, and the stopped and the
    /// failed calls go on saying so over work that runs; drop the
    /// `under_way` guard, and the call still running says its work does.
    #[test]
    fn a_calls_row_says_what_its_work_says_however_the_call_ended() {
        let mut chat = Chat::new();
        for (id, ended) in [("c1", "cancelled"), ("c2", "failed"), ("c3", "in_progress")] {
            chat.tool(&saying(id, "npm run dev", &[]), ended);
            chat.background_says(id, obelus_agent::acp::BACKGROUNDED);
        }
        let state = |id: &str| {
            chat.said
                .iter()
                .find(|said| said.tag.as_deref() == Some(id))
                .and_then(|said| said.state.clone())
        };
        let goes_on = Some(obelus_agent::acp::BACKGROUNDED.to_string());
        assert_eq!(state("c1"), goes_on, "a stopped call");
        assert_eq!(state("c2"), goes_on, "a failed call");
        assert_eq!(
            state("c3"),
            Some("in_progress".to_string()),
            "a call under way"
        );
    }

    /// News of background work that leaves its call's row saying what it
    /// said keeps the rows; news that changes it does not.
    ///
    /// Work reports its progress as often as it likes, and almost none of
    /// that moves the row -- laid out again for each, a busy test run was
    /// the whole transcript laid out a few times a second.
    ///
    /// Broken deliberately by putting `forget_the_layout()` back at the top
    /// of `background_says`, which fails the first claim, or of
    /// `background_ended_everywhere`, which fails the last; and by dropping
    /// it from where the row changes, which fails the second.
    #[test]
    fn news_of_work_that_moves_no_row_does_not_lay_the_conversation_out_again() {
        let mut chat = Chat::new();
        let mut started = saying("c1", "npm run dev", &[]);
        started.backgrounded = true;
        chat.tool(&started, "completed");
        let held = |chat: &Chat| chat.laid.borrow().is_some();

        let _ = chat.rows(ROOM.reading);
        chat.background_says("c1", obelus_agent::acp::BACKGROUNDED);
        assert!(
            held(&chat),
            "the work going on as it was threw the rows away"
        );

        chat.background_says("c1", "completed");
        assert!(
            !held(&chat),
            "the work ended and the rows stayed as they were"
        );

        let _ = chat.rows(ROOM.reading);
        chat.background_ended_everywhere();
        assert!(
            held(&chat),
            "ending work nothing was waiting on threw the rows away"
        );
    }

    /// A closed call shows its title, and no more of it than it is allowed.
    ///
    /// Three claims, and each was the fault at some point.
    ///
    /// The whole of a wrapped title was drawn whatever the fold said, which
    /// nobody noticed while a title was a handful of words. Then an agent
    /// wrote a script into a heredoc and sent the whole script as the
    /// call's title, and a closed call sat there with twenty rows of shell
    /// under a mark saying it was shut.
    ///
    /// So all of it went behind the fold -- and a command of two rows,
    /// which is most of them, could not be read at all without opening the
    /// call. A transcript of commands is read for the commands.
    ///
    /// Broken deliberately three ways. Taking the cap off `shown` leaves a
    /// closed call as tall as its title. Setting it to nothing puts the
    /// command back behind the fold. And dropping the `if open` arm loses
    /// the rest of a long title from the opened call, which is the half a
    /// reader opened it for.
    #[test]
    fn a_closed_call_shows_its_title_up_to_the_cap() {
        let script = "python3 - <<'PY'\nimport pathlib\np = pathlib.Path('a.rs')\nPY";
        let mut chat = Chat::new();
        chat.tool(&saying("c1", script, &["what it printed"]), "completed");

        // Closed, which is how a call that is over arrives.
        let rows = chat.rows(ROOM.reading);
        assert!(!rows[0].open, "a call that is over did not fold itself");
        assert!(
            rows.len() > 1,
            "a closed call showed nothing of its title but the first row: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        assert!(
            rows.len() <= super::MOST_TITLE_ROWS,
            "a closed call is {} rows tall, which is more than it is allowed: {:?}",
            rows.len(),
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        // Long enough to be capped, or the assertion above passes for the
        // wrong reason.
        assert!(
            script.lines().count() > super::MOST_TITLE_ROWS,
            "this title fits inside the cap, so nothing here can show it working"
        );

        // Opened, the whole of the title is there, and what it carries
        // under it.
        chat.fold(Folds::Said(0));
        let rows: Vec<String> = chat.rows(ROOM.reading).iter().map(Row::text).collect();
        for line in script.lines() {
            assert!(
                rows.iter().any(|row| row == line),
                "opening it did not bring back {line:?}: {rows:?}"
            );
        }
        assert!(
            rows.iter().any(|row| row == "what it printed"),
            "opening it did not bring back what the command printed: {rows:?}"
        );
    }

    /// A key on any row of a call's title acts on the whole title, and on
    /// nothing the opened call carries under it.
    ///
    /// Broken deliberately by putting back the `Speaker::Reader` test on
    /// the walk in `acting`: the second row of the command acts on nothing.
    /// And by walking every row from the same thing said, whatever part of
    /// it: the call's output joins its title.
    #[test]
    fn every_row_of_a_call_s_title_acts_on_the_call() {
        let script = "python3 - <<'PY'\nimport pathlib\nPY";
        let mut chat = Chat::new();
        chat.tool(&saying("c1", script, &["what it printed"]), "completed");
        let rows = chat.rows(ROOM.reading);
        assert_eq!(
            rows.len(),
            3,
            "{:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
        for row in 0..3 {
            assert_eq!(Row::acting(&rows, row), Some(0..3), "from row {row}");
        }

        chat.fold(Folds::Said(0));
        let rows = chat.rows(ROOM.reading);
        let printed = rows
            .iter()
            .position(|row| row.text() == "what it printed")
            .expect("what it printed");
        assert_eq!(Row::acting(&rows, 1), Some(0..3));
        assert_eq!(Row::acting(&rows, printed), None, "what it printed acts");
    }

    /// Shift and tab from a lower row of a call's title goes to the call
    /// before it, not to the first row of the one it was pressed in.
    ///
    /// Broken deliberately by stepping back from `at` again rather than
    /// from where what it is part of starts: the cursor lands on `b1`.
    #[test]
    fn shift_and_tab_leaves_the_call_it_is_on() {
        let mut chat = Chat::new();
        chat.tool(&saying("c1", "a1\na2\na3", &["x"]), "completed");
        chat.tool(&saying("c2", "b1\nb2\nb3", &["y"]), "completed");
        let laid = |chat: &Chat| chat.rows(ROOM.reading);
        chat.settle(laid(&chat).len(), ROOM.transcript);

        // Into the transcript, which lands on its last row: the second
        // call's last row of title.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        let Focus::Transcript(at) = chat.focus() else {
            panic!("the cursor is not in the transcript");
        };
        assert_eq!(
            laid(&chat)[at.row].text(),
            "b3",
            "not on the title's last row"
        );

        chat.handle_key(&key(KeyCode::BackTab), false, ROOM, &[], false);
        let Focus::Transcript(at) = chat.focus() else {
            panic!("the cursor left the transcript");
        };
        assert_eq!(
            laid(&chat)[at.row].text(),
            "a1",
            "shift and tab stayed on the call"
        );
    }

    /// Closing a call from a row of its title that closing takes away puts
    /// the cursor on the call's first row.
    ///
    /// Broken deliberately by dropping the `if !kept` arm after the fold:
    /// the cursor stays on row four, which is past the end of a transcript
    /// three rows long.
    #[test]
    fn closing_a_call_from_deep_in_its_title_keeps_the_cursor_on_it() {
        let script = "a1\na2\na3\na4\na5";
        let mut chat = Chat::new();
        chat.tool(&saying("c1", script, &["what it printed"]), "completed");
        chat.fold(Folds::Said(0));
        let laid = |chat: &Chat| chat.rows(ROOM.reading);
        chat.settle(laid(&chat).len(), ROOM.transcript);

        // Into the transcript, on what the call printed, and up onto the
        // title's last row.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        let Focus::Transcript(at) = chat.focus() else {
            panic!("the cursor is not in the transcript");
        };
        assert_eq!(
            laid(&chat)[at.row].text(),
            "a5",
            "not on the title's last row"
        );

        chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false);
        let rows = laid(&chat);
        assert!(!rows[0].open, "enter did not close the call");
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: 0,
                character: 0
            }),
            "the cursor was left behind: {:?}",
            rows.iter().map(Row::text).collect::<Vec<_>>()
        );
    }

    /// A call's title is wrapped whether or not it carries anything.
    ///
    /// It was wrapped on a call carrying nothing and put down as one run,
    /// however long, on a call carrying something -- which is backwards.
    /// A call with something behind it is a call worth reading, so the
    /// headings that ran off the side of the screen were exactly those.
    /// And Obelus runs commands for an agent, where the title *is* the
    /// command: the longest titles there are belong to the calls that
    /// always carry what the command printed.
    ///
    /// The rows a carrying call opens with have to be the same rows, not
    /// merely short enough: they are the same words at the same width, and
    /// a second way of laying them out is a second answer that can drift.
    ///
    /// Opened, because the rest of a long title is behind the fold with
    /// everything else the call carries -- a closed one is its first row
    /// and nothing more, which is what
    /// [`a_closed_call_is_one_row_however_long_its_title`] is about.
    ///
    /// Broken deliberately by putting `plain(said.text.clone())` back as
    /// the opening row's runs: the whole title comes back as one row, far
    /// wider than it was laid out for.
    #[test]
    fn a_call_wraps_its_title_whether_or_not_it_carries_anything() {
        let long = "cd into the tree and grep every manifest in it for the \
                    dependencies it names, then say whether any of them is a cycle";
        let opening = |carried: &[&str]| -> Vec<String> {
            let mut chat = Chat::new();
            chat.tool(&saying("c1", long, carried), "completed");
            if !carried.is_empty() {
                chat.fold(Folds::Said(0));
            }
            chat.rows(ROOM.reading)
                .into_iter()
                .map(|row| row.text())
                .collect()
        };

        let alone = opening(&[]);
        assert!(
            alone.len() > 1,
            "the title does not wrap at this width, so nothing here can show the fault"
        );
        let carrying = opening(&["what the command printed"]);
        assert_eq!(
            carrying.get(..alone.len()),
            Some(alone.as_slice()),
            "a call carrying something laid its title out some other way: {carrying:?}"
        );
        for row in &carrying {
            assert!(
                obelus_text::text_width(row) <= usize::from(ROOM.reading),
                "a row is {} wide where there is room for {}: {row:?}",
                obelus_text::text_width(row),
                ROOM.reading
            );
        }
    }

    /// What is held is the same words however wide the window is.
    ///
    /// The whole reason a selection is kept as places in what was said
    /// rather than as rows and columns. Rows are what a width makes of a
    /// conversation and they are made again every frame, so a selection
    /// kept as one would slide across the words the moment the terminal
    /// changed size -- and a reader who copied after that would get
    /// something they never pointed at.
    ///
    /// Broken deliberately by marking a middle row from its own runs
    /// rather than whole: the blank between two things said carries no
    /// runs of anybody's, so it goes, and the copy loses the line that was
    /// on the screen between them.
    ///
    /// The other half -- holding a row and a column instead of a place in
    /// what was said -- is not a break this can perform, because there is
    /// nowhere in the model to put one. What holds that up is the widths
    /// above: a selection that moved with the rows would give different
    /// words at each of them.
    #[test]
    fn what_is_held_is_the_same_words_at_any_width() {
        let mut chat = Chat::new();
        chat.chunk(
            Speaker::Agent,
            "the first thing it said, which is long enough that it has to wrap somewhere",
        );
        chat.chunk(Speaker::Reader, "and what I said back to it afterwards");
        chat.hold_from(Spot {
            said: 0,
            source: Source::Text,
            at: 4,
        });
        chat.hold_to(Spot {
            said: 1,
            source: Source::Text,
            at: 8,
        });

        let words = |width: u16| {
            chat.held_text(width)
                .expect("something held")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let wide = words(70);
        assert!(
            wide.starts_with("first thing it said"),
            "it did not begin where it was taken hold of: {wide:?}"
        );
        assert!(
            wide.ends_with("and what"),
            "it did not end where it was dragged to: {wide:?}"
        );
        // And the blank between the two comes with them: it is on the
        // screen between words that are held, so it is part of what the
        // reader is pointing at.
        assert!(
            chat.held_text(70).expect("something held").contains("\n\n"),
            "the blank between the two things said was left out"
        );
        for width in [24, 40, 55] {
            assert_eq!(
                words(width),
                wide,
                "held at 70 and at {width} are not the same words"
            );
        }
    }

    /// A later update about a tool call carries only what changed.
    ///
    /// Which is the protocol's own arrangement, and the reason everything
    /// on the row is kept rather than rebuilt: an agent that says "it
    /// finished" and nothing else is not saying the file it was in has
    /// stopped being the file it was in. Rebuilding the row from that
    /// update would leave a call whose kind, title and place vanish the
    /// moment it succeeds.
    #[test]
    fn an_update_that_says_only_the_state_keeps_the_rest() {
        let mut chat = Chat::new();
        let place = obelus_agent::acp::Place {
            path: std::path::PathBuf::from("/tree/src/app.rs"),
            line: Some(20),
        };
        chat.tool(
            &call("t1", "Read the file", "read", vec![place.clone()]),
            "in_progress",
        );
        chat.tool(&call("t1", "", "", Vec::new()), "completed");

        let rows = chat.rows(60);
        let row = rows.first().expect("the tool call");
        assert_eq!(row.text(), "Read the file", "the title was lost");
        assert_eq!(row.kind, "read", "the kind was lost");
        assert_eq!(row.place, Some((place, 0)), "where it was, was lost");
        assert_eq!(row.state.as_deref(), Some("completed"), "the state is old");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The same, with shift down, which on a motion means "and hold what I
    /// pass over".
    fn shifted(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    /// The same, with control down, which on Home and End means the two
    /// ends of the whole transcript.
    fn control(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    /// A screen with ten rows of transcript and a box twenty cells wide.
    const ROOM: Room = Room {
        transcript: 10,
        reading: 40,
        writing: 20,
    };

    /// A tool call, as an agent sends one.
    pub(super) fn call(
        id: &str,
        title: &str,
        kind: &str,
        places: Vec<obelus_agent::acp::Place>,
    ) -> obelus_agent::acp::Call {
        obelus_agent::acp::Call {
            id: id.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            places,
            change: None,
            ran: None,
            said: Vec::new(),
            backgrounded: false,
        }
    }

    /// A call that both changes a file and says why.
    ///
    /// Which the protocol allows -- the content of a call is a list, and a
    /// diff and some words are two of the things that go in it.
    fn saying_and_changing(id: &str, said: &str) -> obelus_agent::acp::Call {
        obelus_agent::acp::Call {
            said: vec![said.to_string()],
            change: Some(obelus_agent::acp::Change {
                path: std::path::PathBuf::from("a.rs"),
                before: None,
                after: "a line\n".to_string(),
            }),
            ..call(id, "Write a.rs", "edit", Vec::new())
        }
    }

    /// The same, carrying words.
    fn saying(id: &str, title: &str, said: &[&str]) -> obelus_agent::acp::Call {
        obelus_agent::acp::Call {
            said: said.iter().map(|words| (*words).to_string()).collect(),
            ..call(id, title, "other", Vec::new())
        }
    }

    /// Somewhere an agent said it had been.
    fn place(path: &str, line: u32) -> obelus_agent::acp::Place {
        obelus_agent::acp::Place {
            path: std::path::PathBuf::from(path),
            line: Some(line),
        }
    }

    /// A conversation with two tool calls and prose between them.
    fn walked() -> Chat {
        let mut chat = Chat::new();
        chat.asked(&[Part::Words("what is this file".to_string())]);
        chat.chunk(Speaker::Agent, "let me look");
        chat.tool(
            &call("t1", "Read a file", "read", vec![place("/a.rs", 3)]),
            "completed",
        );
        chat.chunk(Speaker::Agent, "and another");
        chat.tool(
            &call("t2", "Read another", "read", vec![place("/b.rs", 9)]),
            "completed",
        );
        chat
    }

    /// The cursor walks the words, and tab goes to what enter opens.
    ///
    /// Two keys for two things, which is the change: the arrows used to hop
    /// between the rows that *do* something and step over everything else,
    /// so a reader could reach a tool call quickly and could not reach a
    /// word at all. Now the arrows walk the transcript as text -- which is
    /// what lets a keyboard hold a piece of it -- and tab is what jumps to
    /// the next heading, file or address.
    ///
    /// Broken deliberately two ways. Taking the left and right arrows out
    /// of the motion arm leaves the caret unable to stand anywhere but the
    /// start of a row, which is not a place in the words. And taking out
    /// the tab arm leaves a reader pressing an arrow eighty times to reach
    /// the tool call at the end of a paragraph -- and, because the key then
    /// falls through to the box behind, stepping the agent's way of working
    /// on the way.
    #[test]
    fn a_paste_off_the_box_takes_the_keys_back_to_it() {
        // A paste is put somewhere on purpose -- a terminal's own, or a file
        // dragged onto one -- so it takes the keys back where a letter typed
        // out here does not.
        //
        // Deliberate break: taking either arm out of the match in `paste`,
        // and the word is in a box the focus is not in.
        let mut chat = walked();
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert!(matches!(chat.focus(), Focus::Transcript(_)));
        chat.paste("你好", ROOM.writing);
        assert_eq!(chat.focus(), Focus::Writing, "the transcript kept the keys");
        assert_eq!(chat.writing().text(), "你好");

        let mut chat = walked();
        chat.focus = Focus::Settings(0);
        chat.paste("你好", ROOM.writing);
        assert_eq!(chat.focus(), Focus::Writing, "the row kept the keys");
    }

    /// The cursor in the transcript steps over a picture and its selector
    /// in one press, the way the caret in a file does, and up and down
    /// never leave it between the two.
    ///
    /// Deliberate break: a step of one character, which is what the arrows
    /// were -- the first stop is between the heart and its selector.
    #[test]
    fn the_cursor_in_the_transcript_steps_over_a_whole_cluster() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "a\u{2764}\u{fe0f}b");
        let rows = chat.rows(ROOM.reading);
        let row = rows
            .iter()
            .position(|row| row.text().contains('\u{2764}'))
            .expect("the heart is on a row");
        let start = rows[row]
            .text()
            .chars()
            .position(|c| c == 'a')
            .expect("the a");
        chat.focus = Focus::Transcript(Place {
            row,
            character: start,
        });
        let mut stops = Vec::new();
        for _ in 0..3 {
            chat.handle_key(&key(KeyCode::Right), false, ROOM, &[], false);
            if let Focus::Transcript(place) = chat.focus() {
                stops.push(place.character - start);
            }
        }
        assert_eq!(stops, [1, 3, 4], "stepping right");

        // Standing between the two, which a place carried over from a row
        // laid out differently could leave it: left goes to the start of
        // the picture, not past it.
        chat.focus = Focus::Transcript(Place {
            row,
            character: start + 2,
        });
        chat.handle_key(&key(KeyCode::Left), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row,
                character: start + 1
            }),
            "stepping left from inside the picture"
        );
    }

    /// The links on a row are found where their words are -- one markdown
    /// wrote as a link, and one written out in a sentence -- each by the
    /// characters it covers, so that two on one row are two things and a
    /// link that wraps is a link on both rows. An address in a block of
    /// code is a line of the code, and enter does what the row did before.
    ///
    /// Broken deliberately five ways. By leaving markdown's links off its
    /// rows: the link's words are no link. By keeping the full stop on the
    /// written-out address: it goes somewhere else. By answering from the
    /// row rather than the characters in `links_on` (every link from the
    /// row's first character): the second link is the first. By
    /// looking for addresses in a block of code: the code has a link in it.
    /// And by having enter follow the link under the caret: the reader's
    /// own address is no longer a message they can copy back.
    #[test]
    fn a_link_is_the_characters_its_words_are_on() {
        let (one, two, docs, bare, theirs) = (
            "https://a.example/one",
            "https://a.example/two",
            "https://a.example/docs",
            "https://a.example/a_(b)",
            "https://a.example/theirs",
        );
        let mut chat = Chat::new();
        chat.chunk(
            Speaker::Agent,
            &format!(
                "[one]({one}) and [two]({two}) here.\n\n[the documentation for the whole of \
                 this feature]({docs}) runs on.\n\nOr see {bare}.\n\n```\ncurl {docs}\n```\n"
            ),
        );
        chat.chunk(Speaker::Reader, theirs);
        let rows = chat.rows(ROOM.reading);
        let on = |words: &str| {
            rows.iter()
                .enumerate()
                .find_map(|(row, it)| {
                    let at = it.text().find(words)?;
                    Some(Place {
                        row,
                        character: it.text()[..at].chars().count(),
                    })
                })
                .unwrap_or_else(|| panic!("no row says {words:?}: {rows:?}"))
        };
        let goes = |at: Place| {
            rows[at.row]
                .links
                .iter()
                .find(|link| link.characters.contains(&at.character))
                .map(|link| link.to.as_str())
        };
        let (first, second, between) = (on("one"), on("two"), on("and two"));
        let (link, rest) = (on("the documentation"), on("feature"));
        let (written, code, reader) = (on("https://a.example/a_"), on("curl"), on(theirs));
        assert_eq!(first.row, second.row, "the two links are not on one row");
        assert!(link.row < rest.row, "the link did not wrap: {rows:?}");

        for (at, to) in [
            (first, one),
            (second, two),
            (link, docs),
            (rest, docs),
            (written, bare),
            (reader, theirs),
        ] {
            assert_eq!(goes(at), Some(to), "at {at:?}");
        }
        assert_eq!(
            goes(between),
            None,
            "the words between two links are a link"
        );
        let address = Place {
            character: code.character + "curl ".len(),
            ..code
        };
        assert_eq!(
            goes(address),
            None,
            "an address in a block of code is a link"
        );

        chat.focus = Focus::Transcript(reader);
        assert!(
            matches!(
                chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false),
                ChatOutcome::TakeBack(_)
            ),
            "enter on the reader's own address did not copy their words to the box"
        );
    }

    /// Enter copies a block of code from any row of it, and tab stops on it
    /// once, at the first character of the code.
    ///
    /// The line is longer than the box, so a copy read off the rows would
    /// be two lines between bars; what enter hands over is the line as the
    /// agent wrote it. And the prose around the block is not the block:
    /// enter there does what it did before, which is nothing.
    ///
    /// Broken deliberately four ways. By leaving the arm out of the key's
    /// `match`: enter on the code does nothing. By leaving the block out of
    /// `Row::acting`: every row of it is a thing of its own, so the first
    /// of them is the box's corner and tab lands there. By leaving
    /// `next_stop_in` to stop on every row
    /// that acts: the second tab is the next row of the block. And by
    /// standing at the start of the stop: tab puts the cursor on the
    /// corner of the box.
    #[test]
    fn enter_copies_a_block_of_code_and_tab_stops_on_it_once() {
        let line = "echo 'kernel.perf_event_paranoid = 1' | sudo tee /etc/sysctl.d/99-perf.conf";
        let mut chat = Chat::new();
        chat.chunk(
            Speaker::Agent,
            &format!("Run this:\n\n```sh\n{line}\n```\n\nThen look.\n\n```\nls\n```\n"),
        );
        let rows = chat.rows(ROOM.reading);
        let at = |row: &Row| row.code.as_ref().map(|code| code.at);
        let first = rows.iter().find_map(at).expect("no block of code");
        let block: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| at(row) == Some(first))
            .map(|(at, _)| at)
            .collect();
        let (Some(&top), Some(&bottom)) = (block.first(), block.last()) else {
            panic!("no block of code: {rows:?}");
        };
        let second = rows
            .iter()
            .rposition(|row| row.text().contains("ls"))
            .expect("the second block");
        assert!(
            bottom - top > 2,
            "the line did not wrap, so this shows nothing"
        );
        let prose = rows
            .iter()
            .position(|row| row.text().contains("Then look"))
            .expect("the prose after the block");

        let copied = |chat: &mut Chat, row: usize| {
            chat.focus = Focus::Transcript(Place { row, character: 0 });
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false)
        };
        for row in [top, top + 1, bottom] {
            assert_eq!(
                copied(&mut chat, row),
                ChatOutcome::Copy(line.to_string()),
                "enter on row {row} of {top}..={bottom}"
            );
        }
        assert_eq!(copied(&mut chat, prose), ChatOutcome::Consumed);

        // From above the block, tab lands in the code rather than on its
        // box, the next tab goes on past the rest of it to the next block,
        // and shift and tab comes back to where the first one landed.
        chat.focus = Focus::Transcript(Place {
            row: 0,
            character: 0,
        });
        chat.handle_key(&key(KeyCode::Tab), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: top + 1,
                character: 1,
            }),
            "tab did not land on the first character of the code"
        );
        chat.handle_key(&key(KeyCode::Tab), false, ROOM, &[], false);
        assert!(
            matches!(chat.focus(), Focus::Transcript(place) if place.row == second),
            "tab did not go on to the next block: {:?}",
            chat.focus()
        );
        chat.handle_key(&shifted(KeyCode::BackTab), false, ROOM, &[], false);
        assert!(
            matches!(chat.focus(), Focus::Transcript(place) if place.row == top + 1),
            "shift and tab did not come back to the first block: {:?}",
            chat.focus()
        );
    }

    #[test]
    fn the_cursor_walks_the_words_and_tab_goes_to_what_acts() {
        let mut chat = walked();
        let rows = chat.rows(ROOM.reading);
        let stops: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            stops.len(),
            3,
            "what the reader asked and the two tool calls are the stops"
        );
        let last = rows.len() - 1;

        // Up from the box lands at the end of the last row, which is the
        // words nearest the box.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: rows[last].characters(),
            })
        );

        // The left arrow walks back along it a character at a time, rather
        // than leaving the row altogether.
        chat.handle_key(&key(KeyCode::Left), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: rows[last].characters() - 1,
            }),
            "the left arrow did not walk the words"
        );
        // And home takes it to the start of the row it is on.
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: 0
            })
        );

        // The last row here is the second tool call, so the cursor came in
        // already standing on one of the two: shift and tab goes to the
        // other.
        assert_eq!(last, stops[2], "the last row is the second tool call");
        chat.handle_key(&key(KeyCode::BackTab), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[1],
                character: 0
            }),
            "shift and tab did not reach the thing enter opens"
        );

        // Enter opens what the row names.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false),
            ChatOutcome::GoTo(place("/a.rs", 3))
        );
        // And on a row of nothing but words it does nothing, rather than
        // doing whatever the last row it was on did.
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[], false);
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false),
            ChatOutcome::Consumed,
            "enter on a row of words did something"
        );
        // And tab forwards is the way back to the other one.
        chat.handle_key(&key(KeyCode::Tab), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[2],
                character: 0
            }),
            "tab did not reach the next thing enter opens"
        );

        // Escape is the way back: it gives up on the nearest thing, which
        // is being in the transcript rather than the conversation.
        let outcome = chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false);
        assert_eq!(outcome, ChatOutcome::Consumed, "escape closed the view");
        assert_eq!(chat.focus(), Focus::Writing);
    }

    /// Up from the box goes to the foot of the transcript, in the column
    /// it left.
    ///
    /// The two halves of where the cursor was keep differently. The column
    /// is the reader's: coming back to the *end* of the row rather than to
    /// the column it was left from put the cursor on the far side of the
    /// page, for anybody reading down a long answer who stepped one row too
    /// far and pressed up again. The row is a number that stops meaning the
    /// same words -- the agent answers while the reader is typing, and the
    /// row they left is now somewhere in the middle of what they have not
    /// read. So the row is asked for again, and the answer is the foot,
    /// which is the words nearest the box and where what arrived has gone.
    ///
    /// The wheel is not the exception it used to be. It moves the view, and
    /// what is kept is a column, which says nothing about where the view is
    /// looking.
    ///
    /// Broken deliberately three ways: by coming back to the end of the
    /// row, which is what it did before the column was kept; by coming back
    /// to the row the cursor left, which is what it did after; and by
    /// forgetting the column when the wheel turns.
    #[test]
    fn up_from_the_box_goes_to_the_foot_of_the_transcript() {
        let mut chat = Chat::new();
        chat.chunk(
            Speaker::Agent,
            "hello there, this is the answer, and it is long enough to be \
             several rows of it at the width these tests read at",
        );
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);
        let foot = chat.rows(ROOM.reading).len() - 1;

        // In at the foot, to the start of that row, and then two rows up:
        // away from the foot, in a column that is the end of nothing.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        let Focus::Transcript(left) = chat.focus() else {
            panic!("the cursor is not in the transcript");
        };
        assert_eq!(left.character, 0, "home did not reach the start of the row");
        assert!(
            left.row < foot,
            "up did not leave the foot of the transcript"
        );

        // Out to the box and back in: the column it left, on the foot
        // rather than on the row it left.
        chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false);
        assert_eq!(chat.focus(), Focus::Writing, "escape did not reach the box");
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: foot,
                character: 0
            }),
            "coming back did not come back to the foot, in the column it left"
        );

        // Down is the other way out, and what arrives while the reader is
        // in the box moves the foot. Which is what they are taken to: the
        // row they left is now in the middle of what they have not read.
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[], false);
        assert_eq!(chat.focus(), Focus::Writing, "down did not reach the box");
        chat.chunk(
            Speaker::Agent,
            ", and here is the rest of it, which arrived while the reader \
             was writing",
        );
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);
        let grown = chat.rows(ROOM.reading).len() - 1;
        assert!(grown > foot, "the rest of the answer made no rows");
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: grown,
                character: 0
            }),
            "coming back went to where the foot used to be"
        );

        // And the wheel keeps the column, because a column is not a place
        // the view can be moved away from.
        chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false);
        chat.scroll(-1);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: grown,
                character: 0
            }),
            "the wheel forgot the column"
        );
    }

    /// Shift and a motion hold what the motion passed over.
    ///
    /// The whole point of a cursor in here. The pointer could already take
    /// hold of what was said; a reader whose hands were on the keyboard
    /// could not, and had to reach for the mouse to copy a line of an
    /// answer.
    ///
    /// What is held is kept as places in the words even though the cursor
    /// is kept as a row and a character, because the two outlive different
    /// things: a selection is made and then read, folded, resized before
    /// it is copied, and a cursor is where the reader is looking while
    /// their hand is on the key.
    ///
    /// Broken deliberately by anchoring the selection where the motion
    /// *ends* rather than where it began, which holds nothing at all on
    /// the first press; or by letting a bare motion keep what was held,
    /// which leaves a reader who walks away from a selection with a copy
    /// of something they are no longer looking at.
    #[test]
    fn shift_and_a_motion_hold_what_it_passed_over() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "hello there");
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);

        // In at the end of the words, then back over the last two of them
        // with shift down.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("re".to_string(), "selection"),
            "shift and the left arrow did not hold what it passed over"
        );

        // And a bare motion lets go.
        chat.handle_key(&key(KeyCode::Left), false, ROOM, &[], false);
        assert!(!chat.holding(), "a bare motion kept the selection");
    }

    /// Shift and an arrow hold a character of the box at a time.
    ///
    /// The pair fell through every arm and did nothing, so the one way to
    /// take hold of part of a message from the keyboard was the whole line.
    ///
    /// Broken deliberately by taking the two arms out, which holds nothing;
    /// and by leaving the transcript's hold alone, which leaves two
    /// selections on one screen.
    #[test]
    fn shift_and_an_arrow_hold_characters_in_the_box() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "hello there");
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);
        chat.put("abcdef");

        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("ef".to_string(), "selection"),
            "shift and left did not hold what they passed over in the box"
        );
        chat.handle_key(&shifted(KeyCode::Right), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("f".to_string(), "selection"),
            "shift and right did not give back what was held"
        );

        // One selection between the two halves. Held with the pointer,
        // which leaves the caret in the box: walking up into the transcript
        // and back lets go on the way down, and would ask nothing of these.
        chat.hold_from(Spot {
            said: 0,
            source: Source::Text,
            at: 0,
        });
        chat.hold_to(Spot {
            said: 0,
            source: Source::Text,
            at: 5,
        });
        assert!(chat.holding(), "the transcript did not take hold");
        chat.handle_key(&shifted(KeyCode::Right), false, ROOM, &[], false);
        assert!(
            !chat.holding(),
            "the box took hold and the transcript kept its own"
        );
    }

    /// Shift and home hold a line of the box, in the box.
    ///
    /// Shift extends and never names a command, which is one rule
    /// everywhere -- and the box was the one place it named one: shift and
    /// home threw the whole transcript to the beginning of the
    /// conversation. A reader reaching for the pair means the line they
    /// have just written, and the two ends of a long transcript are
    /// control's, which is what the rule over the box names.
    ///
    /// Broken deliberately by putting `self.window.home()` back on the arm,
    /// which holds nothing and leaves the conversation somewhere the reader
    /// did not ask to be; or by leaving the transcript's own hold alone,
    /// which leaves two selections on one screen and a copy that takes the
    /// older of them.
    #[test]
    fn shift_and_home_hold_the_line_in_the_box() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "hello there");
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);
        chat.put("a message I typed");

        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("a message I typed".to_string(), "selection"),
            "shift and home did not hold the line in the box"
        );
        assert!(
            chat.at_the_end(),
            "shift and home moved the transcript instead of holding anything"
        );

        // And the other end. A bare motion lets go first, the way it does
        // everywhere: shift and end straight after shift and home walks the
        // caret back to where the hold was anchored, and holds nothing.
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::End), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("a message I typed".to_string(), "selection"),
            "shift and end did not hold the line in the box"
        );
        assert!(chat.at_the_end(), "shift and end moved the transcript");

        // One selection between the two halves: taking hold in the box
        // lets the transcript go. Up twice, because the first press is
        // what lets go of what the box is holding.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        assert!(chat.holding(), "the transcript did not take hold");
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[], false);
        assert!(
            !chat.holding(),
            "the box took hold and the transcript kept its own"
        );
    }

    /// The two ends of the transcript are control's, from either half.
    ///
    /// The rule over the box names `ctrl+end` as the way back, so the key
    /// has to work wherever the reader is when they read it. With the
    /// cursor in the transcript it did not: every modifier that reached
    /// `on_transcript` was taken for shift, so the key held the row it was
    /// standing on and the view never moved.
    ///
    /// Broken deliberately by asking `!bare` rather than shift on the
    /// motion arm, which is what it did: `ctrl+end` then holds a row
    /// instead of reaching the end. Or by sending the view to the end and
    /// leaving the cursor behind, which the next arrow drags straight back.
    #[test]
    fn control_and_the_ends_reach_the_ends_from_either_half() {
        let mut chat = Chat::new();
        for turn in 0..8 {
            chat.asked(&[Part::Words(format!("question {turn}"))]);
            chat.chunk(Speaker::Agent, &format!("answer {turn}"));
        }
        let short = Room {
            transcript: 3,
            ..ROOM
        };
        chat.settle(chat.rows(short.reading).len(), short.transcript);

        // From the box, where there is no cursor in the transcript to move.
        chat.handle_key(&control(KeyCode::Home), false, short, &[], false);
        assert_eq!(chat.focus(), Focus::Writing, "the box lost the keys");
        assert!(!chat.at_the_end(), "control and home did not leave the end");

        // And from inside it, where the cursor goes with the view.
        chat.handle_key(&key(KeyCode::Up), false, short, &[], false);
        chat.handle_key(&control(KeyCode::End), false, short, &[], false);
        let rows = chat.rows(short.reading);
        let last = rows.len() - 1;
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: rows[last].characters(),
            }),
            "control and end did not take the cursor to the last row"
        );
        assert!(chat.at_the_end(), "control and end did not reach the end");
        assert!(!chat.holding(), "control and end held a row instead");

        chat.handle_key(&control(KeyCode::Home), false, short, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: 0,
                character: 0
            }),
            "control and home did not take the cursor to the first row"
        );
        assert!(!chat.holding(), "control and home held a row instead");
    }

    /// Shift and home hold what is before the cursor, on a row that starts
    /// with something the reading drew.
    ///
    /// Only markdown lays out runs nobody wrote -- a bullet, a quote's bar
    /// -- because a plain wrapping gives one run carrying the whole row. So
    /// this was a fault nothing but an agent's own answer could reach, and
    /// it did not look like an off-by-one: `spot_at` handed back nothing
    /// for the start of such a row, `spot_near` read that as "no words on
    /// this row at all" and went looking *down* the transcript the way it
    /// does for a blank or a heading, and the selection ran forwards from
    /// the cursor across the rows below it.
    ///
    /// Broken deliberately by returning `after` from the drawn-run arm of
    /// `spot_at` rather than waiting for the word that follows it: the
    /// first assertion below then holds the two rows after the cursor
    /// instead of the words before it.
    #[test]
    fn shift_and_home_on_a_bullet_holds_what_is_before_the_cursor() {
        let bulleted = "- alpha beta gamma\n- delta epsilon zeta\n\n> a quote here\n";
        let laid = |chat: &Chat| chat.rows(ROOM.reading);
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, bulleted);
        chat.settle(laid(&chat).len(), ROOM.transcript);

        // The rows this is about: ones whose first run is Obelus's own.
        let rows = laid(&chat);
        assert!(
            rows[0]
                .spans
                .first()
                .is_some_and(|span| span.from.is_none()),
            "the first row does not start with a run the reading drew: {rows:?}"
        );

        // Into the transcript -- which lands at the end of the last row --
        // and then up to the first bullet and along it with the arrows,
        // because the cursor in here is only ever put somewhere by a key.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        for _ in 0..3 {
            chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        }
        for _ in 0..9 {
            chat.handle_key(&key(KeyCode::Right), false, ROOM, &[], false);
        }
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: 0,
                character: 9
            }),
            "the cursor is not in the middle of the first bullet"
        );

        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("alpha b".to_string(), "selection"),
            "shift and home did not hold what is before the cursor"
        );

        // The same on a quote, whose bar is drawn the same way.
        for _ in 0..3 {
            chat.handle_key(&key(KeyCode::Down), false, ROOM, &[], false);
        }
        for _ in 0..7 {
            chat.handle_key(&key(KeyCode::Right), false, ROOM, &[], false);
        }
        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("a quo".to_string(), "selection"),
            "shift and home did not hold what is before the cursor, in a quote"
        );

        // And the start of such a row is the first word on it, not a place
        // on some row below: the seam the whole fault came through.
        assert_eq!(
            rows[0].spot_at(0),
            rows[0].spot_at(2),
            "the start of the row is not where its words start"
        );
    }

    /// A row that a soft break joined is held whole.
    ///
    /// The space a soft break becomes is in nobody's words, and the run
    /// that started on it pointed at nothing for the rest of the row: the
    /// second line of an agent's paragraph could be neither dragged across
    /// nor held with the keys, so a selection stopped at the break.
    ///
    /// Broken deliberately by letting the reading's run go on from that
    /// space into the words after it: this then copies `Two` alone.
    #[test]
    fn a_row_joined_by_a_soft_break_is_held_whole() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Agent, "**Two**\nopen the page and press it\n");
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);

        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::End), false, ROOM, &[], false);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("Two open the page and press it".to_string(), "selection"),
            "the words after the soft break could not be held"
        );
    }

    /// A run folds itself, and only the reader opens it.
    ///
    /// Calls of one kind stop being a story and become a log once there are
    /// enough of them, and a log is read by going to it. Nothing but the
    /// reader's own word opens one -- a failure inside used to, which is
    /// what [`one_call_that_failed_does_not_throw_the_run_open`] is about.
    #[test]
    fn a_run_folds_itself_and_the_reader_opens_it() {
        let read = |chat: &mut Chat, id: &str, state: &str| {
            chat.tool(
                &call(id, "Read a file", "read", vec![place("/a.rs", 1)]),
                state,
            );
        };
        let mut chat = Chat::new();
        read(&mut chat, "t1", "completed");
        read(&mut chat, "t2", "completed");
        // Two of them are drawn as they are, and tight: calls of one kind
        // in a row are one block rather than two paragraphs.
        assert_eq!(chat.rows(ROOM.reading).len(), 2, "two calls, two rows");

        // The third makes it a run, and a run is one row.
        read(&mut chat, "t3", "completed");
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 1, "a run of three is not one row: {rows:?}");
        assert_eq!(rows[0].text(), "3 Files");
        assert!(rows[0].folds.is_some() && !rows[0].open);

        // Opened, its members are under it; closed again, they are not.
        // The reader's word is the whole of what decides it.
        chat.fold(Folds::Run(0));
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 4, "its members are not under it: {rows:?}");
        chat.fold(Folds::Run(0));
        assert_eq!(
            chat.rows(ROOM.reading).len(),
            1,
            "the reader closed it and it stayed open"
        );
    }

    /// The words a call carries are under it, and a later update replaces
    /// them.
    ///
    /// Which is what the protocol says a later `content` is -- *replace the
    /// content collection* -- and what the diff beside them already did. A
    /// plan put to the reader is replaced by what became of the asking, and
    /// which of the two the row shows is the agent's account of its own
    /// call rather than Obelus's to keep both of.
    #[test]
    fn a_call_shows_what_it_says_now_and_a_later_word_replaces_it() {
        let mut chat = Chat::new();
        chat.tool(
            &saying("c1", "Approve Plan", &["the plan itself"]),
            "pending",
        );
        let said: Vec<String> = chat
            .rows(ROOM.reading)
            .iter()
            .map(|row| row.text())
            .collect();
        assert!(
            said.iter().any(|text| text == "Approve Plan"),
            "no row for the call: {said:?}"
        );
        assert!(
            said.iter().any(|text| text == "the plan itself"),
            "the words are not under it: {said:?}"
        );

        // Answered: what the call says now is what became of the asking.
        chat.tool(&saying("c1", "", &["and what came of asking"]), "completed");
        assert_eq!(chat.said[0].words, ["and what came of asking"]);

        // An update that carries no words leaves them alone, the way an
        // update that carries no title leaves the title alone. Silence is
        // "the same as before", which is not the same as "nothing".
        chat.tool(&call("c1", "", "other", Vec::new()), "completed");
        assert_eq!(
            chat.said[0].words,
            ["and what came of asking"],
            "silence wiped what was said"
        );
    }

    /// A call that changes a file and says why shows both.
    ///
    /// The protocol puts them in one list, so a reader gets both or the
    /// arm that ran first decided for them: the words went when the change
    /// was read on its own and returned.
    #[test]
    fn a_call_that_changes_something_and_says_why_shows_both() {
        let mut chat = Chat::new();
        chat.tool(
            &saying_and_changing("c1", "because the cache is wrong"),
            "pending",
        );
        let said: Vec<String> = chat
            .rows(ROOM.reading)
            .iter()
            .map(|row| row.text())
            .collect();
        assert!(
            said.iter().any(|text| text == "because the cache is wrong"),
            "the words went with the change: {said:?}"
        );
        assert!(
            said.iter().any(|text| text == "a line"),
            "the change went with the words: {said:?}"
        );
        // The account before the thing it accounts for.
        assert!(
            said.iter()
                .position(|text| text == "because the cache is wrong")
                < said.iter().position(|text| text == "a line"),
            "the why is a footnote to the what: {said:?}"
        );
    }

    /// A call that is waiting on the reader shows what it is waiting about.
    ///
    /// The same rule a change goes by, and for the same reason: while it is
    /// the question, what it is asking about is the thing to read. Once it
    /// is answered the row stays and puts its words away -- there to be
    /// opened again, and not in the way.
    #[test]
    fn a_call_shows_its_words_while_it_is_the_question() {
        let open = |state: &str| {
            let mut chat = Chat::new();
            chat.tool(&saying("c1", "Approve Plan", &["the plan itself"]), state);
            chat.rows(ROOM.reading)
                .iter()
                .any(|row| row.text() == "the plan itself")
        };
        assert!(open("pending"), "a call waiting on the reader hid it");
        assert!(open("in_progress"), "a call still working hid it");
        assert!(!open("completed"), "an answered call left it open");
    }

    /// Thinking is not folded away, but it can be put away.
    ///
    /// An agent's reasoning about the code is often the most of what a turn
    /// is worth, so Obelus never closes it for the reader -- and short
    /// thinking has no heading at all, because a heading over three words
    /// is two rows saying one thing.
    #[test]
    fn thinking_is_open_and_only_the_reader_closes_it() {
        let mut chat = Chat::new();
        chat.chunk(Speaker::Thought, "hmm");
        assert_eq!(
            chat.rows(ROOM.reading).len(),
            1,
            "short thinking was given a heading"
        );

        let mut chat = Chat::new();
        chat.chunk(
            Speaker::Thought,
            "step_rows is answering two questions at once: the rows of the \
             text, and the rows of the screen. An opened hunk is where they \
             stop being the same number, which is why the caret jumped.",
        );
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows[0].text(), "thought", "long thinking has no heading");
        assert!(rows[0].open, "Obelus closed the thinking by itself");
        assert!(rows.len() > 1, "the thinking is not under its heading");

        chat.fold(Folds::Said(0));
        assert_eq!(
            chat.rows(ROOM.reading).len(),
            1,
            "the reader could not put it away"
        );
    }

    /// What is drawn in from the edge is wrapped to what is left of it.
    ///
    /// The members of an opened run are indented, and the view draws them
    /// at that indent: a row wrapped to the full width would run off the
    /// end by exactly as far as it was moved in. Two numbers that have to
    /// agree, so they are one number -- and this is what says so.
    #[test]
    fn rows_drawn_in_from_the_edge_are_wrapped_to_what_is_left() {
        let mut chat = Chat::new();
        // One cell short of the room a row has at the edge, so that it
        // fits there and does not fit an indent further in: the one text
        // that tells the two widths apart.
        let title = "x".repeat(usize::from(ROOM.reading) - 1);
        let title = title.as_str();
        for id in ["t1", "t2", "t3"] {
            chat.tool(
                &call(id, title, "read", vec![place("/a.rs", 1)]),
                "completed",
            );
        }
        // Opened: folded, its members are not drawn at all.
        chat.fold(Folds::Run(0));

        let rows = chat.rows(ROOM.reading);
        assert!(
            rows.iter().any(|row| row.depth > 0),
            "the run did not open: {rows:?}"
        );
        for row in &rows {
            let room = usize::from(ROOM.reading.saturating_sub(u16::from(row.depth) * DEEPER));
            assert!(
                obelus_text::text_width(&row.text()) <= room,
                "{:?} is {} cells wide with {room} to write in",
                row.text(),
                obelus_text::text_width(&row.text())
            );
        }
    }

    /// What is happening now is one row, always last, and always current.
    ///
    /// A state has no history: the next one replaces it rather than piling
    /// up under it, and nothing at all removes it. Being worked out from
    /// the state every frame is what makes it impossible to leave behind
    /// saying something that has stopped being true.
    #[test]
    fn what_is_happening_is_one_row_that_is_replaced() {
        let mut chat = Chat::new();
        chat.doing(Some("starting\u{2026}"));
        chat.doing(Some("Thinking\u{2026}"));
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 1, "the states piled up: {rows:?}");
        assert_eq!(rows[0].text(), "Thinking\u{2026}");
        assert_eq!(rows[0].speaker, Speaker::Doing);
        assert!(
            !rows[0].acts(),
            "the cursor can stand on what is happening now"
        );

        // Whatever is said while it is going, it stays under all of it.
        chat.note("the agent stopped: exit status 3");
        let rows = chat.rows(ROOM.reading);
        assert_eq!(
            rows.last().map(|row| row.speaker),
            Some(Speaker::Doing),
            "something was said under what is happening: {rows:?}"
        );

        chat.doing(None);
        assert!(
            chat.rows(ROOM.reading)
                .iter()
                .all(|row| row.speaker != Speaker::Doing),
            "it outlived what it was about"
        );
    }

    /// A page moves the view and takes the cursor with it; the wheel moves
    /// the view and leaves it.
    ///
    /// Two gestures, two jobs: a reader pressing a key is going somewhere,
    /// and one spinning a wheel is looking around. The same split the
    /// editor has had all along.
    #[test]
    fn a_page_takes_the_cursor_with_it_and_the_wheel_does_not() {
        let mut chat = Chat::new();
        chat.tool(
            &call("t1", "Read a file", "read", vec![place("/a.rs", 3)]),
            "completed",
        );
        // Enough between them that the two cannot be on screen together:
        // the transcript here is ten rows.
        for index in 0..8 {
            chat.note(&format!("something happened {index}"));
        }
        chat.tool(
            &call("t2", "Read another", "read", vec![place("/b.rs", 9)]),
            "completed",
        );
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);

        let rows = chat.rows(ROOM.reading);
        let stops: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at)
            .collect();
        assert_eq!(stops.len(), 2);

        // In at the one nearest the box, then a page up: the view moves,
        // and the cursor lands on what the view now holds.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[1],
                character: chat.rows(ROOM.reading)[stops[1]].characters(),
            }),
            "the cursor did not come in at the end of the last row"
        );
        chat.handle_key(&key(KeyCode::PageUp), false, ROOM, &[], false);
        let Focus::Transcript(place) = chat.focus() else {
            panic!("the page took the cursor out of the transcript");
        };
        assert!(
            place.row < stops[1],
            "the page left the cursor behind: it is still on row {}",
            place.row
        );
        assert!(
            chat.in_view_of(chat.rows(ROOM.reading).len(), ROOM)
                .contains(&place.row),
            "the page moved the view and left the cursor off it"
        );

        // And the wheel over the same ground leaves the cursor alone.
        let before = chat.focus();
        chat.scroll(3);
        assert_eq!(chat.focus(), before, "the wheel moved the cursor");
    }

    /// Typing in the transcript goes nowhere, and leaves the cursor and what
    /// it holds where they were.
    ///
    /// It used to take the keys back to the box, and a letter pressed while
    /// reading threw the reader out of the place they had walked to. Every
    /// key the box calls its own is asked, and with shift as well as bare:
    /// shift and enter is a line break, and a capital is shift and a letter.
    ///
    /// Broken deliberately twice: putting back the arm that left the
    /// transcript and fell through to the box, and the focus and the box
    /// both go red; and letting go of what is held in that arm, and the
    /// selection does.
    #[test]
    fn typing_in_the_transcript_goes_nowhere() {
        let mut chat = walked();
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[], false);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[], false);
        let at = chat.focus();
        assert!(matches!(at, Focus::Transcript(_)), "not in the transcript");
        assert!(chat.holding(), "nothing held, so this proves nothing");
        for pressed in [
            key(KeyCode::Char('h')),
            shifted(KeyCode::Char('H')),
            key(KeyCode::Backspace),
            key(KeyCode::Delete),
            shifted(KeyCode::Enter),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
        ] {
            chat.handle_key(&pressed, false, ROOM, &[], false);
            assert_eq!(chat.focus(), at, "{pressed:?} moved the cursor");
            assert!(chat.holding(), "{pressed:?} let go of what was held");
        }
        assert_eq!(chat.writing().text(), "", "something reached the box");
    }

    /// Typing on the row of settings goes nowhere too, and is still taken
    /// there: a key the row let fall through would reach the application,
    /// where `Backspace` with a modifier or a letter is somebody else's.
    ///
    /// Broken deliberately twice: putting back the arm that moved the focus
    /// to the box and fell through, and the focus goes red; and answering
    /// `None` from that arm without moving the focus, and the box does,
    /// because the box's own arms below take whatever falls through.
    #[test]
    fn typing_on_the_row_of_settings_goes_nowhere() {
        let settings = [obelus_agent::acp::Setting {
            id: "allow_all".to_string(),
            name: "Allow everything".to_string(),
            about: None,
            values: Vec::new(),
            current: "off".to_string(),
            kind: obelus_agent::acp::Kind::Switch,
            category: obelus_agent::acp::Category::Other,
            legacy: false,
        }];
        let mut chat = walked();
        chat.focus = Focus::Settings(0);
        for pressed in [
            key(KeyCode::Char('h')),
            shifted(KeyCode::Char('H')),
            key(KeyCode::Backspace),
            key(KeyCode::Delete),
            shifted(KeyCode::Enter),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
        ] {
            let outcome = chat.handle_key(&pressed, false, ROOM, &settings, false);
            assert!(
                matches!(outcome, ChatOutcome::Consumed),
                "{pressed:?} was not taken by the row"
            );
            assert_eq!(
                chat.focus(),
                Focus::Settings(0),
                "{pressed:?} took the keys away"
            );
        }
        assert_eq!(chat.writing().text(), "", "something reached the box");
    }

    /// Chunks arrive a few words at a time, and what a reader should see is
    /// one answer rather than one paragraph per packet.
    #[test]
    fn pieces_of_one_answer_are_one_thing_said() {
        let mut chat = Chat::new();
        chat.asked(&[Part::Words("what is this".to_string())]);
        chat.chunk(Speaker::Agent, "it is ");
        chat.chunk(Speaker::Agent, "a rust file");
        // Thinking is not the answer, so it starts something of its own --
        // and the answer after it does too, rather than being appended to
        // the thinking.
        chat.chunk(Speaker::Thought, "hmm");
        chat.chunk(Speaker::Agent, "in fact");

        let rows = chat.rows(40);
        let words: Vec<String> = rows
            .iter()
            .map(Row::text)
            .filter(|text| !text.is_empty())
            .collect();
        assert_eq!(
            words,
            ["what is this", "it is a rust file", "hmm", "in fact"]
        );
        let speakers: Vec<Speaker> = rows
            .iter()
            .filter(|row| row.first)
            .map(|row| row.speaker)
            .collect();
        assert_eq!(
            speakers,
            [
                Speaker::Reader,
                Speaker::Agent,
                Speaker::Thought,
                Speaker::Agent
            ]
        );
    }

    /// A tool call is one row that changes, not a row per update.
    #[test]
    fn a_tool_call_is_one_row_however_often_it_changes() {
        let mut chat = Chat::new();
        chat.tool(&call("t1", "Read the file", "read", Vec::new()), "pending");
        chat.tool(
            &call("t2", "Run the tests", "execute", Vec::new()),
            "pending",
        );
        // A later update carries only what changed, so an empty title means
        // the one already there.
        chat.tool(&call("t1", "", "", Vec::new()), "completed");

        let rows = chat.rows(40);
        let calls: Vec<(String, Option<&str>)> = rows
            .iter()
            .filter(|row| row.speaker == Speaker::Tool && row.first)
            .map(|row| (row.text(), row.state.as_deref()))
            .collect();
        assert_eq!(
            calls,
            [
                ("Read the file".to_string(), Some("completed")),
                ("Run the tests".to_string(), Some("pending")),
            ]
        );
    }

    /// The view follows the end while the reader is at the end, and stays
    /// put once they have scrolled up to read something.
    #[test]
    fn it_follows_the_end_until_the_reader_scrolls_away_from_it() {
        let mut chat = Chat::new();
        for line in 0..20 {
            chat.note(&format!("line {line}"));
        }
        // Twenty things said, each a row with a blank row between: forty
        // rows less the one before the first.
        let rows = chat.rows(40).len();
        assert_eq!(rows, 39);

        chat.settle(rows, 10);
        assert_eq!(chat.top(), 29, "it did not start at the end");

        // Scrolled up, and it stays where it is put -- including when
        // something new arrives, which is the whole point.
        //
        // By the wheel rather than by the up arrow. The arrow moves the
        // cursor now, and a cursor stepping on to a row that is already on
        // screen moves nothing: what this is about is the window, so it is
        // moved by the thing whose whole job is moving the window.
        chat.scroll(-1);
        chat.settle(rows, 10);
        assert_eq!(chat.top(), 28);
        chat.note("something new");
        let rows = chat.rows(40).len();
        chat.settle(rows, 10);
        assert_eq!(chat.top(), 28, "it dragged the view to the end");

        // Back down to the end, and it follows again.
        for _ in 0..5 {
            chat.handle_key(&key(KeyCode::Down), false, ROOM, &[], false);
        }
        chat.settle(rows, 10);
        assert_eq!(chat.top(), rows - 10);
        chat.note("and another");
        let rows = chat.rows(40).len();
        chat.settle(rows, 10);
        assert_eq!(chat.top(), rows - 10, "it stopped following the end");
    }

    /// What escape means depends on whether anything is happening, and
    /// after that on what is in the box.
    ///
    /// Stopping first and emptying the box second, one press each: a
    /// press that stopped the agent and emptied the box as well would
    /// throw away what the reader was in the middle of saying.
    ///
    /// And a selection before the box it is in, because the box has no undo.
    ///
    /// Deliberate breaks: drop the arm that empties the box, and the last
    /// escape is `Ignored` with `hello` still in it; drop the arm that lets
    /// go, and the escape meant for the selection empties the box.
    #[test]
    fn escape_stops_the_agent_then_empties_the_box() {
        let mut chat = Chat::new();
        for character in "hello".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, ROOM, &[], false);
        }
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), true, ROOM, &[], false),
            ChatOutcome::Interrupt
        );
        assert_eq!(chat.writing().text(), "hello", "stopping emptied the box");
        // What is held goes before the words do.
        chat.handle_key(
            &KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
            false,
            ROOM,
            &[],
            false,
        );
        assert!(chat.writing().selected().is_some(), "nothing was held");
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false),
            ChatOutcome::Consumed
        );
        assert_eq!(
            chat.writing().text(),
            "hello",
            "escape took the words with the selection"
        );
        assert!(chat.writing().selected().is_none(), "escape kept hold");
        // And the transcript's, held with the pointer while the caret
        // stays in the box.
        chat.note("something said");
        chat.hold_from(Spot {
            said: 0,
            source: Source::Text,
            at: 0,
        });
        chat.hold_to(Spot {
            said: 0,
            source: Source::Text,
            at: 5,
        });
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false),
            ChatOutcome::Consumed
        );
        assert_eq!(
            chat.writing().text(),
            "hello",
            "escape took the words with the transcript's selection"
        );
        assert!(!chat.holding(), "escape kept hold of the transcript");
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false),
            ChatOutcome::Consumed
        );
        assert_eq!(chat.writing().text(), "", "escape left the box as it was");
        // And with neither it is not the conversation's key: a
        // conversation is a document, and escape leaves what is *over* a
        // document. There is nothing over this one.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[], false),
            ChatOutcome::Ignored
        );
    }

    /// Shift and up holds a row of the box, and shift and down gives it
    /// back -- and neither carries the caret out of the box, the way the
    /// bare keys do at its ends.
    ///
    /// Deliberate breaks: drop the two arms, and shift and up is `Ignored`
    /// with nothing held -- the bug as it was reported; send the shifted up
    /// on into the transcript when the caret did not move, as the bare one
    /// goes, and the focus is the transcript's after the second press.
    #[test]
    fn shift_and_up_or_down_holds_rows_of_the_box() {
        let shift = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
        let mut chat = Chat::new();
        chat.note("something said");
        for character in "ab".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, ROOM, &[], false);
        }
        chat.handle_key(&shift(KeyCode::Enter), false, ROOM, &[], false);
        for character in "cd".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, ROOM, &[], false);
        }
        assert_eq!(
            chat.handle_key(&shift(KeyCode::Up), false, ROOM, &[], false),
            ChatOutcome::Consumed
        );
        assert_eq!(chat.writing().selected().as_deref(), Some("\ncd"));
        // From the top row there is nowhere to go, and the box keeps it.
        chat.handle_key(&shift(KeyCode::Up), false, ROOM, &[], false);
        assert_eq!(chat.focus(), Focus::Writing, "it left the box");
        assert!(chat.writing().selected().is_some(), "it let go");
        // And back down, to where it started.
        chat.handle_key(&shift(KeyCode::Down), false, ROOM, &[], false);
        chat.handle_key(&shift(KeyCode::Down), false, ROOM, &[], false);
        assert_eq!(chat.focus(), Focus::Writing, "it left the box");
        assert_eq!(chat.writing().text(), "ab\ncd", "the keys wrote something");
    }

    /// What was waiting goes back in the box with its pictures, in front of
    /// what was being typed.
    ///
    /// It went back as the words the page spells it with, so a picture
    /// came back as `[Image 1]` -- words, which enter then sent as words.
    ///
    /// Deliberate breaks: have `take_back_waiting` hand back each one's
    /// `text` as words rather than its `parts`, and the picture is gone;
    /// take out its `let_go`, and the transcript is still holding.
    #[test]
    fn what_was_waiting_goes_back_with_its_pictures() {
        use crate::composer::{Attached, Part};
        let picture = Attached {
            mime: "image/png".to_string(),
            bytes: b"png".to_vec(),
        };
        let mut chat = Chat::new();
        chat.will_say(&[
            Part::Words("look at ".to_string()),
            Part::Picture(picture.clone()),
        ]);
        chat.will_say(&[Part::Words("and this".to_string())]);
        chat.put("half typed");
        // Held across both, which do not survive it.
        chat.hold_from(Spot {
            said: 0,
            source: Source::Text,
            at: 0,
        });
        chat.hold_to(Spot {
            said: 1,
            source: Source::Text,
            at: 3,
        });
        assert!(chat.holding(), "the transcript did not take hold");

        let parts = chat.take_back_waiting();
        assert!(chat.unsent().is_empty(), "it is still waiting on the page");
        assert!(!chat.holding(), "the hold outlived the words it was on");
        chat.put_back(parts.expect("nothing came back"));
        assert_eq!(
            chat.writing().parts(),
            vec![
                Part::Words("look at ".to_string()),
                Part::Picture(picture),
                Part::Words("\n\nand this\n\nhalf typed".to_string()),
            ]
        );
    }

    /// Typing goes into the row being typed, and enter sends it.
    #[test]
    fn what_is_typed_is_sent_once() {
        let mut chat = Chat::new();
        for character in "hello".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, ROOM, &[], false);
        }
        chat.handle_key(&key(KeyCode::Backspace), false, ROOM, &[], false);
        assert_eq!(chat.writing().text(), "hell");

        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false),
            ChatOutcome::Send(vec![Part::Words("hell".to_string())])
        );
        // Sent, so the row is empty: a prompt still sitting there after
        // being sent is a prompt that gets sent twice.
        assert_eq!(chat.writing().text(), "");
        // And an empty box sends nothing at all. It used to stand for
        // "send what I have waiting", which was one key meaning two
        // things: harmless with words in the box, and a stop to a running
        // turn without them. What is waiting is rows in the transcript
        // now, and each of them has its own key.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[], false),
            ChatOutcome::Consumed
        );
    }

    /// A key with a modifier Obelus has no meaning for is not swallowed:
    /// `ctrl+q` still quits with the conversation open.
    #[test]
    fn a_chord_falls_through_to_the_key_table() {
        let mut chat = Chat::new();
        let quit = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(
            chat.handle_key(&quit, false, ROOM, &[], false),
            ChatOutcome::Ignored
        );
        assert_eq!(chat.writing().text(), "", "it typed the chord into the row");
    }
}

#[cfg(test)]
mod remembering {
    use super::*;
    use crate::composer::Part;

    /// One way of changing a conversation, for walking the list of them.
    type Way = Box<dyn Fn(&mut Chat)>;

    /// What is on the page is what was said, every time.
    ///
    /// The rows of a conversation are worked out from every word ever said
    /// in it, at the width of the moment, and that is tens of milliseconds
    /// for a long morning -- paid several times over for one keypress, by
    /// the keys, by the caret and by the drawing. So they are kept, and a
    /// copy is handed out: a twentieth of the cost.
    ///
    /// Which buys a way to be wrong that Obelus did not have before. Rows
    /// kept past the moment they stopped being true are a screen that has
    /// stopped saying what happened -- an answer that never appears, a
    /// tool call stuck on "pending", a run that will not open. So every
    /// way of changing what there is to lay out is walked here, and each
    /// one has to show on the page.
    ///
    /// What a reader has hold of is not among them on purpose: it is
    /// marked on to the rows after they come out of what is kept, so that
    /// dragging across a morning does not lay it out again for every
    /// report the terminal sends. The last case checks that too -- from
    /// the other side, that holding still reaches the page.
    ///
    /// Broken deliberately by taking the forgetting out of any one of the
    /// methods below: that one line of the list goes quiet, and the page
    /// goes on showing the conversation as it was before it.
    #[test]
    fn what_is_kept_never_outlives_what_it_was_made_of() {
        const WIDTH: u16 = 40;

        // The whole of each row rather than its words: some of these
        // change what a row *is* without changing what it says -- the
        // address a reader was sent to coming back answered, a call going
        // from pending to done -- and a page that went on drawing the old
        // glyph would be just as wrong.
        let said = |chat: &Chat| -> Vec<String> {
            chat.rows(WIDTH)
                .iter()
                .map(|row| format!("{row:?}"))
                .collect()
        };
        // Each of them changes what there is to lay out, so each has to
        // change what is on the page.
        let ways: Vec<(&str, Way)> = vec![
            (
                "asked",
                Box::new(|chat: &mut Chat| chat.asked(&[Part::Words("a question".to_string())])),
            ),
            ("heard", Box::new(|chat: &mut Chat| chat.heard("an answer"))),
            ("note", Box::new(|chat: &mut Chat| chat.note("a note"))),
            (
                "chunk",
                Box::new(|chat: &mut Chat| chat.chunk(Speaker::Agent, "a piece of one")),
            ),
            // The second piece of the same answer, which is the other
            // half of what a chunk does: the first starts something said
            // and the second appends to it in place. That one shipped
            // without the forgetting -- an answer streaming into a
            // paragraph already on screen stopped growing until something
            // else happened to drop the rows.
            (
                "chunk again",
                Box::new(|chat: &mut Chat| chat.chunk(Speaker::Agent, " and another piece")),
            ),
            (
                "away",
                Box::new(|chat: &mut Chat| chat.away("w1", "go and sign in", "https://example")),
            ),
            ("arrived", Box::new(|chat: &mut Chat| chat.arrived("w1"))),
            (
                "tool",
                Box::new(|chat: &mut Chat| {
                    chat.tool(
                        &super::tests::call("t9", "Read a file", "read", Vec::new()),
                        "pending",
                    );
                }),
            ),
            (
                "tool again",
                Box::new(|chat: &mut Chat| {
                    chat.tool(
                        &super::tests::call("t9", "Read a file", "read", Vec::new()),
                        "completed",
                    );
                }),
            ),
            (
                "doing",
                Box::new(|chat: &mut Chat| chat.doing(Some("Thinking…"))),
            ),
            (
                "planning",
                Box::new(|chat: &mut Chat| {
                    chat.planning(vec![obelus_agent::acp::Step {
                        said: "wire it up".to_string(),
                        state: "pending".to_string(),
                        priority: "medium".to_string(),
                    }]);
                }),
            ),
            ("plan_forgotten", Box::new(Chat::plan_forgotten)),
            (
                "doing nothing",
                Box::new(|chat: &mut Chat| chat.doing(None)),
            ),
        ];

        let mut chat = Chat::new();
        let mut before = said(&chat);
        for (what, change) in ways {
            change(&mut chat);
            let after = said(&chat);
            assert_ne!(
                before, after,
                "the page did not change when the conversation did, at {what}"
            );
            before = after;
        }

        // Folding is the other kind: nothing new was said, and the rows
        // are different anyway.
        let mut folding = Chat::new();
        for index in 0..4 {
            folding.tool(
                &super::tests::call(&format!("r{index}"), "Read a file", "read", Vec::new()),
                "completed",
            );
        }
        let before = said(&folding);
        folding.fold(Folds::Run(0));
        assert_ne!(
            before,
            said(&folding),
            "the page did not change when a run was opened"
        );

        // And what is held, which is not kept with the rows but marked on
        // to them after: it still has to reach the page.
        let mut holding = Chat::new();
        holding.chunk(Speaker::Agent, "hello there");
        let rows = holding.rows(WIDTH);
        let spot = rows[0].spot_at(0).expect("a place in the words");
        let to = rows[0].spot_at(5).expect("another");
        assert!(holding.rows(WIDTH)[0].held.is_none());
        holding.hold_from(spot);
        holding.hold_to(to);
        assert_eq!(
            holding.rows(WIDTH)[0].held,
            Some(0..5),
            "what is held did not reach the page"
        );
    }
}
