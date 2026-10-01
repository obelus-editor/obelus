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
//! The transcript has a cursor, and it stands only on rows that do
//! something. A tool call names a file; prose does not. The cursor steps over
//! what cannot be opened -- the rule a list follows for a row that cannot be
//! chosen -- so a reader walking a conversation never lands somewhere enter
//! does nothing, and the lit row is the promise: what is marked is what opens.
//!
//! The arrows move the nearest thing that can still move. Where there is a row
//! to stand on they walk to it and the view follows; where there is none they
//! scroll a row, which is what they have always done and what a conversation of
//! nothing but words still needs. Enter opens what the row names -- in a
//! buffer, and the conversation hides itself, because going somewhere means
//! seeing it. Escape comes back out to the box without closing anything, and
//! typing goes to the box wherever the cursor was, because a reader who starts
//! typing means to type.
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
    /// What the row is, where it is a line of a change: gone, new, or the
    /// line it is at.
    pub marker: Option<obelus_text::marker::Marker>,
    /// How much a change adds and takes away, on the row that heads it.
    pub changed: Option<(usize, usize)>,
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
    /// Whether the cursor can stand on this row.
    ///
    /// Only rows that do something when they are chosen: a tool call names
    /// a file and enter opens it, a heading opens what is under it, a row
    /// the reader was sent away by sends them again. Prose is stepped over
    /// rather than landed on -- the same rule a list follows for a row that
    /// cannot be chosen -- so a reader walking the transcript never reaches
    /// a row where enter does nothing.
    ///
    /// The list is the one the key's own `match` answers, and the two have
    /// to say the same thing: a row this lets the cursor stand on and that
    /// does nothing is a key that appears not to work.
    #[must_use]
    pub const fn acts(&self) -> bool {
        self.place.is_some() || self.folds.is_some() || self.away.is_some() || self.unsent.is_some()
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
    /// Put these words back in the box: the reader took back something
    /// they had said that had not gone yet.
    TakeBack(String),
    /// Move to the agent's next way of working.
    StepMode,
    /// Open what a row of the transcript names.
    GoTo(obelus_agent::acp::Place),
    /// Send the reader to this web address again.
    Away(String),
    /// Open the values of one of the agent's settings, by its id.
    Choose(String),
    /// Flip one of its switches, by its id.
    Toggle(String),
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
    /// One of the settings on the status row, by its place in the list.
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
    /// The rows as they were last laid out, and the width they were laid
    /// out at.
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
    laid: std::cell::RefCell<Option<(u16, Vec<Row>)>>,
    /// What is happening now, if anything is.
    ///
    /// One slot rather than a line of the transcript: a state has no
    /// history, and the next one replaces it. Which is also why it cannot
    /// go stale -- what is not stored cannot be left on screen saying
    /// something that has stopped being true.
    doing: Option<String>,
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

/// Whether what this voice says arrives as the protocol's own text
/// content, which is the thing it asks clients to read as markdown.
///
/// The agent's answer and its thinking do. The reader's own message does
/// not -- Obelus has it as they typed it, and reflowing somebody's words
/// back at them is Obelus deciding what they meant. Nor do the rows Obelus
/// makes up itself: a note, a state, a step of a plan, a run's heading.
///
/// Nor a tool call, which is not one voice: what it carries is the
/// agent's text content and is read as markdown where the row is built,
/// and its title is a line the protocol calls human-readable rather than
/// markdown. Both are decided there, where which of the two is in hand is
/// known.
const fn reads_as_markdown(speaker: Speaker) -> bool {
    matches!(speaker, Speaker::Agent | Speaker::Thought)
}

/// Which characters of a row fall between two places in what it was laid
/// out from.
///
/// The runs of a row carry the bytes they were laid out from, so this is
/// where those bytes fall inside the window and what that is in characters
/// of the row. The runs that carry nothing -- what the reading drew rather
/// than read -- are counted past rather than held: whether they are inside
/// is a question about the rows around them, and [`Chat::mark_held`] is
/// where that is answered.
fn row_held(row: &Row, from: usize, to: usize) -> Option<std::ops::Range<usize>> {
    let mut held: Option<std::ops::Range<usize>> = None;
    let mut column = 0usize;
    for span in &row.spans {
        let bytes = span.from.clone();
        for (offset, character) in span.text.char_indices() {
            if let Some(bytes) = bytes.as_ref() {
                let byte = bytes.start + offset;
                if byte >= from && byte < to {
                    let next = column..column + 1;
                    held = Some(match held {
                        Some(had) => had.start..next.end,
                        None => next,
                    });
                }
            }
            let _ = character;
            column += 1;
        }
    }
    held
}

/// One run of a row, in the colour the words themselves have nothing to
/// say about.
#[must_use]
fn plain(text: String) -> Vec<Span> {
    vec![Span::new(text, Ink::Plain)]
}

/// A thing said, laid out for a width, as the runs of each row.
///
/// Markdown where the protocol says the text is markdown, and plain
/// wrapping everywhere else. What an agent said, what it was thinking and
/// what a tool call carries all arrive as `ContentBlock::Text`, which is
/// the one the protocol writes "Clients SHOULD render this text as
/// Markdown" about. A tool call's *title* is not one of those, nor are the
/// lines of a change, nor the reader's own message -- Obelus has that as
/// they typed it and has no business reflowing it -- nor anything Obelus
/// says in its own voice.
///
/// The wrapping is markdown's own, because that is where the difficulty
/// lives: a fenced block does not wrap like a paragraph and a bullet's
/// second line is indented under its first.
fn laid_out(text: &str, width: u16, markdown: bool) -> Vec<Vec<Span>> {
    if !markdown {
        return obelus_text::wrapped_from(text, width)
            .into_iter()
            .map(|(said, from)| vec![Span::from_source(said, Ink::Plain, from)])
            .collect();
    }
    obelus_markdown::render(text, width)
        .into_iter()
        .map(|row| match row.rule {
            // A rule has no words of its own, and the transcript has no
            // room for a view that draws one: it is a row like the others,
            // so it is drawn as what it is.
            true => vec![Span::new(
                "\u{2500}".repeat(usize::from(width.max(1))),
                Ink::Mark,
            )],
            false => row.spans,
        })
        .collect()
}

impl Chat {
    /// An empty conversation, following its own end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            said: Vec::new(),
            laid: std::cell::RefCell::new(None),
            doing: None,
            plan: Vec::new(),
            plan_open: false,
            input: Composer::new(),
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

    /// Puts pasted text in the box, wherever the caret is.
    ///
    /// Into the box and nowhere else: a transcript is what was said, and the
    /// one place in a conversation that takes text is the half of it the
    /// reader is writing.
    pub fn paste(&mut self, what: &str, width: u16) {
        self.input.write_in(what, width);
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
    pub fn take_back(&mut self, at: usize) -> Option<String> {
        if !self.said.get(at).is_some_and(|said| said.unsent) {
            return None;
        }
        self.forget_the_layout();
        Some(self.said.remove(at).text)
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
            said.state = Some(status.to_string());
        }
    }

    /// Everything said, as rows wrapped to a width.
    ///
    /// A blank row between one speaker and the next: a transcript with no
    /// space in it reads as one voice.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<Row> {
        let mut rows = self.laid_out(width);
        // And what the reader has hold of, once there are rows to hold:
        // the two ends of a selection are places in what was said, and
        // which characters of which rows that is depends on the width
        // these were just laid out at.
        //
        // Here rather than in what is kept, so that a drag across a long
        // conversation does not lay the whole of it out again for every
        // report the terminal sends.
        self.mark_held(&mut rows);
        rows
    }

    /// The rows, laid out or remembered from the last time they were.
    fn laid_out(&self, width: u16) -> Vec<Row> {
        if let Some(rows) = self
            .laid
            .borrow()
            .as_ref()
            .filter(|(at, _)| *at == width)
            .map(|(_, rows)| rows.clone())
        {
            return rows;
        }
        let rows = self.lay_out(width);
        *self.laid.borrow_mut() = Some((width, rows.clone()));
        rows
    }

    /// Every row of the conversation, at a width.
    fn lay_out(&self, width: u16) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut at = 0;
        let mut first = true;
        while at < self.said.len() {
            let run = self.run_from(at);
            if !first {
                rows.push(self.blank(at));
            }
            first = false;
            match run.len() >= LEAST_TO_FOLD {
                // A run of one kind, under a heading of its own: thirty
                // tool calls in a turn is a log, and a reader looking for
                // what the agent *did* should not have to scroll past the
                // machine to find it.
                true => rows.extend(self.run_rows(run.clone(), width)),
                false => {
                    for index in run.clone() {
                        rows.extend(self.said_rows(index, 0, width));
                    }
                }
            }
            at = run.end;
        }
        // And what is happening now, under the last of it: the foot of the
        // transcript is where the next thing will appear, which is where a
        // reader is already looking.
        if let Some(doing) = self.doing.as_deref() {
            if !first {
                rows.push(self.blank(self.said.len()));
            }
            // The agent's list for this turn, where it has one: one row
            // saying which step it is on, and the whole of it under that
            // for a reader who wants to see whether it understood the job
            // -- which is the moment they would interrupt it.
            //
            // Folded to the one row by default. While the agent works the
            // transcript is filling with the thing the reader is actually
            // watching, and seven rows of checklist is a third of the
            // screen held for the whole turn.
            let planning = !self.plan.is_empty();
            rows.push(Row {
                speaker: Speaker::Doing,
                spans: plain(match planning {
                    true => self.step_now(),
                    false => doing.to_string(),
                }),
                first: true,
                state: None,
                kind: String::new(),
                place: None,
                away: None,
                from: None,
                held: None,
                // Anchored one past the end of what was said, which is the
                // one index that can never name a [`Said`]: a plan is not a
                // thing that was said, and giving it an index into the
                // transcript would be filing it as one.
                folds: planning.then_some(Folds::Plan),
                open: self.plan_open,
                unsent: None,
                marker: None,
                changed: None,
                depth: 0,
            });
            if planning && self.plan_open {
                rows.extend(self.plan.iter().flat_map(|step| {
                    obelus_text::wrapped(&step.said, width.saturating_sub(DEEPER))
                        .into_iter()
                        .enumerate()
                        .map(|(line, text)| Row {
                            speaker: Speaker::Step,
                            spans: plain(text),
                            from: None,
                            held: None,
                            first: false,
                            // On the first row of a step only, so a step
                            // that wraps is one step with one mark.
                            state: (line == 0).then(|| step.state.clone()),
                            kind: String::new(),
                            away: None,
                            place: None,
                            folds: None,
                            open: false,
                            unsent: None,
                            marker: None,
                            changed: None,
                            depth: 1,
                        })
                }));
            }
        }
        rows
    }

    /// The run of things said that begins at `at`: as many tool calls of
    /// one kind as follow one another, or the one thing that is not.
    fn run_from(&self, at: usize) -> std::ops::Range<usize> {
        let Some(said) = self.said.get(at) else {
            // Never empty: the caller walks by the end of what this
            // returns, and an empty range would leave it where it was.
            return at..at + 1;
        };
        if said.speaker != Speaker::Tool {
            return at..at + 1;
        }
        let mut end = at + 1;
        while self
            .said
            .get(end)
            .is_some_and(|next| next.speaker == Speaker::Tool && next.kind == said.kind)
        {
            end += 1;
        }
        at..end
    }

    /// A run of tool calls: its heading, and its members when it is open.
    fn run_rows(&self, run: std::ops::Range<usize>, width: u16) -> Vec<Row> {
        let Some(said) = self.said.get(run.start) else {
            return Vec::new();
        };
        let members: Vec<&Said> = self.said[run.clone()].iter().collect();
        let count = run.len();
        // What they have in common, where they have it: a run of reads is a
        // run of files, and a run of commands is a run of calls.
        let what = match members.iter().all(|said| !said.places.is_empty()) {
            true => "Files",
            false => "Calls",
        };
        // The state of the run is the state of the worst of it: one that
        // failed is the news, and one still running is why the row moves.
        let state = members
            .iter()
            .filter_map(|said| said.state.as_deref())
            .fold(None, |worst: Option<&str>, state| match (worst, state) {
                (Some("failed"), _) | (_, "failed") => Some("failed"),
                (Some("in_progress"), _) | (_, "in_progress") => Some("in_progress"),
                (Some("pending"), _) | (_, "pending") => Some("pending"),
                (_, state) => Some(state),
            });
        let open = self.is_run_open(run.start);
        let mut rows = vec![Row {
            speaker: Speaker::Tool,
            spans: plain(format!("{count} {what}")),
            from: None,
            held: None,
            first: true,
            state: state.map(str::to_string),
            kind: said.kind.clone(),
            place: None,
            away: None,
            folds: Some(Folds::Run(run.start)),
            open,
            unsent: None,
            marker: None,
            changed: None,
            depth: 0,
        }];
        if open {
            for index in run {
                rows.extend(self.said_rows(index, 1, width));
            }
        }
        rows
    }

    /// One thing said, as the rows it takes.
    fn said_rows(&self, at: usize, depth: u8, width: u16) -> Vec<Row> {
        let Some(said) = self.said.get(at) else {
            return Vec::new();
        };
        let room = width.saturating_sub(u16::from(depth) * DEEPER);
        let inside = room.saturating_sub(DEEPER);

        // What a call carries, under the call's own row -- which is what
        // folds it, because a diff is the one thing an agent sends that is
        // longer than the screen and a plan is the other. Neither is prose:
        // the row says what the call is and this is what it is about.
        //
        // One shape for the words and the lines, and both of them where a
        // call has both. They were two arms once, the change first and
        // returning, so a call that said why it was changing something had
        // the why dropped -- and the protocol puts them in one list.
        //
        // The words first: they are the account of the change, and an
        // account after the thing it accounts for is a footnote.
        // What the call carries that is not its own title over again --
        // dropped here rather than where the rows are drawn, because what
        // is wrong is the row: a fold is a promise of something behind it,
        // and behind a copy of the heading there is nothing. Nothing is
        // assumed about what an agent means by it: a call whose words say
        // something of their own keeps every row it had, and one that
        // starts saying something gets them back in the update that does.
        // Kept with which of the words each is, because a place in the
        // transcript has to say which text of a thing said it is in and
        // the filtering above loses the count.
        let carried: Vec<(usize, &String)> = said
            .words
            .iter()
            .enumerate()
            .filter(|(_, words)| words.trim() != said.text.trim())
            .collect();
        if said.speaker == Speaker::Tool && (!carried.is_empty() || !said.change.is_empty()) {
            // Wrapped, like the title of a call that carries nothing --
            // which is the same title and was the only one being wrapped.
            // Put down as one run however long it was, the calls whose
            // headings ran off the side were exactly the ones with
            // something behind them to read, and a command Obelus had run
            // is a title as long as the command.
            //
            // The first row is what opens and what folds; the rest are the
            // rest of the same words, at the same depth and from the same
            // text, so a place in the title is still a place in the title.
            //
            // A few rows of it are shown whatever the fold says, and the
            // rest goes behind it.
            //
            // All of it was drawn once, which nobody noticed while a title
            // was a handful of words: it wrapped to one row and there was
            // no rest of it. A command is a title as long as the command,
            // and an agent that writes a script into a heredoc sends the
            // whole script as the title -- so a closed call sat there with
            // twenty rows of shell under a mark saying it was shut, and
            // the key that was supposed to put it away moved one row of
            // output.
            //
            // Then all of it went behind the fold, and a command of two
            // rows -- which is most of them -- could not be read without
            // opening the call and reading it past its own output. The
            // reader is looking at a transcript of commands: the command
            // is the thing on the row.
            //
            // So a cap, which is what a card's own prose gets and for the
            // same reason: somebody else's text may be as long as it likes
            // and may not push what it belongs to off the screen. Under it
            // the whole command is on the page and opening the call is
            // about the output. Over it the arrow on the first row says
            // there is more, which is what that arrow says about
            // everything else behind it.
            let mut title = laid_out(&said.text, room, reads_as_markdown(said.speaker)).into_iter();
            let mut rows = vec![Row {
                changed: (!said.change.is_empty())
                    .then(|| obelus_git::change::counted(&said.change)),
                ..self.opening(
                    said,
                    title.next().unwrap_or_else(|| plain(String::new())),
                    depth,
                    Some(Folds::Said(at)),
                    Some((at, Source::Text)),
                )
            }];
            // The rows of it that are shown closed as well as open.
            let open = self.is_open(at);
            let shown = match open {
                true => usize::MAX,
                false => MOST_TITLE_ROWS.saturating_sub(1),
            };
            rows.extend(
                title
                    .by_ref()
                    .take(shown)
                    .map(|spans| Self::under(said, spans, depth, Some((at, Source::Text)))),
            );
            if open {
                // Markdown, unless Obelus is running a command for this
                // call: then these words are the command and what it has
                // printed, put here by [`Chat::running`], and a terminal's
                // bytes are not prose. Read as markdown they lose the line
                // between the command and its output -- one newline is a
                // soft break -- which is a call saying it ran something
                // that it never ran.
                rows.extend(carried.iter().flat_map(|(which, words)| {
                    laid_out(words, inside, said.ran.is_none())
                        .into_iter()
                        .map(|spans| {
                            Self::under(said, spans, depth + 1, Some((at, Source::Words(*which))))
                        })
                }));
                // A diff's own markers, which words do not get: "it is
                // changing this" and "it is saying this" are different
                // news, and the markers are where a reader takes that in.
                rows.extend(said.change.iter().enumerate().flat_map(|(which, line)| {
                    laid_out(&line.text, inside, false)
                        .into_iter()
                        .map(move |spans| Row {
                            marker: line.marker,
                            ..Self::under(said, spans, depth + 1, Some((at, Source::Change(which))))
                        })
                }));
            }
            return rows;
        }

        let words = laid_out(&said.text, room, reads_as_markdown(said.speaker));
        // Thinking long enough to be worth putting away gets a heading of
        // its own, which is what folds it. Obelus does not fold it away by
        // itself -- an agent's reasoning about the code is often the most
        // of what a turn is worth -- but a reader who has read it should be
        // able to close it. Short thinking is just the words: a heading
        // over three words is two rows saying one thing.
        if said.speaker != Speaker::Thought || words.len() < LEAST_TO_FOLD {
            return words
                .into_iter()
                .enumerate()
                .map(|(row, spans)| match row {
                    0 => self.opening(said, spans, depth, None, Some((at, Source::Text))),
                    _ => Self::under(said, spans, depth, Some((at, Source::Text))),
                })
                .collect();
        }
        // The heading is Obelus's word for what is behind it, not the
        // agent's: there is nothing in it to take a copy of.
        let mut rows = vec![self.opening(
            said,
            plain("thought".to_string()),
            depth,
            Some(Folds::Said(at)),
            None,
        )];
        if self.is_open(at) {
            rows.extend(
                laid_out(&said.text, inside, true)
                    .into_iter()
                    .map(|spans| Self::under(said, spans, depth + 1, Some((at, Source::Text)))),
            );
        }
        rows
    }

    /// The row a thing said begins with.
    ///
    /// It carries the glyph and everything that is true of the whole of it:
    /// where it has got to, the file it names, and whether there is more
    /// behind it than it is showing.
    fn opening(
        &self,
        said: &Said,
        spans: Vec<Span>,
        depth: u8,
        folds: Option<Folds>,
        from: Option<(usize, Source)>,
    ) -> Row {
        Row {
            speaker: said.speaker,
            spans,
            from,
            held: None,
            first: true,
            state: said.state.clone(),
            kind: said.kind.clone(),
            place: said
                .places
                .first()
                .map(|place| (place.clone(), said.places.len() - 1)),
            // Where it points on the web, which only one voice has: a row
            // the reader was sent away by keeps the address it sent them
            // to, so pressing the key on it sends them again.
            away: match said.speaker {
                Speaker::Away => said.words.first().cloned(),
                _ => None,
            },
            folds,
            open: folds.is_some_and(|what| self.is_open_now(what)),
            unsent: match said.unsent {
                true => from.map(|(at, _)| at),
                false => None,
            },
            marker: None,
            changed: None,
            depth,
        }
    }

    /// A row that continues something already begun.
    fn under(said: &Said, spans: Vec<Span>, depth: u8, from: Option<(usize, Source)>) -> Row {
        Row {
            speaker: said.speaker,
            spans,
            from,
            held: None,
            first: false,
            state: None,
            kind: said.kind.clone(),
            place: None,
            away: None,
            folds: None,
            open: false,
            unsent: None,
            marker: None,
            changed: None,
            depth,
        }
    }

    /// The row that separates one thing said from the next.
    fn blank(&self, at: usize) -> Row {
        Row {
            speaker: self.said.get(at).map_or(Speaker::Note, |said| said.speaker),
            spans: Vec::new(),
            // A blank is not a word of anybody's.
            from: None,
            held: None,
            first: false,
            state: None,
            kind: String::new(),
            place: None,
            away: None,
            folds: None,
            open: false,
            unsent: None,
            marker: None,
            changed: None,
            depth: 0,
        }
    }

    /// Whether the thing a mark folds is open.
    ///
    /// The one answer for all three, because the row that draws the mark
    /// has to agree with the key that presses it: which of them a row is
    /// about is now on the row, so neither has to guess from an index.
    fn is_open_now(&self, what: Folds) -> bool {
        match what {
            Folds::Said(at) => self.is_open(at),
            Folds::Run(at) => self.is_run_open(at),
            Folds::Plan => self.plan_open,
        }
    }

    /// Whether what one thing said begins is open.
    fn is_open(&self, at: usize) -> bool {
        let Some(said) = self.said.get(at) else {
            return false;
        };
        if let Some(open) = said.opened {
            return open;
        }
        if said.speaker == Speaker::Thought {
            return true;
        }
        // A change is open while it is the question: the agent is asking to
        // make it, and what it is asking about is the lines. Once it is
        // made the file itself has them, and Obelus draws a file's changes
        // in the margin beside them -- so the block folds away and the row
        // that opens it stays. A command still running is the same case:
        // what it is printing is the thing being waited on.
        //
        // Having failed is not on this list, and was. The reasoning was
        // that a reader whose tests have just failed is looking for the
        // failure -- true, and not Obelus's to act on, because "failed" is
        // a word from the agent and a great many commands say it without
        // anything being wrong. `grep` exits 1 with nothing to report,
        // `diff` exits 1 on a difference, `test` exits 1 for false: an
        // agent asking a question with a command gets a non-zero answer
        // and marks the call failed, and Obelus was throwing the output of
        // every one of those open and holding it open. What is left is the
        // mark, which says where to look, and the key, which is one press.
        matches!(said.state.as_deref(), Some("pending" | "in_progress"))
    }

    /// Whether the run of calls beginning at `at` is open.
    ///
    /// What the reader said about it, and otherwise shut. A run is a log:
    /// Obelus makes one out of a stretch of calls of a kind exactly
    /// because nobody reads a log line by line, and a run that decides for
    /// itself when to be a log again is a run the reader cannot keep shut.
    ///
    /// It used to open itself when any of its calls had failed, and that
    /// is what this is about. One call in eight says failed -- which an
    /// agent says of a `grep` that matched nothing as readily as of a
    /// build that fell over -- and eight calls came open, with a cross on
    /// the heading over them. Seven of them had nothing to say. The
    /// heading still carries the worst state of what is inside it, so
    /// nothing is hidden: what has gone is a shut thing opening itself.
    ///
    /// Asked of the run rather than of the call it begins at, which is what
    /// it used to be. A run is named by that call and so they shared an
    /// answer: opening the run set the call's `opened`, the call's own mark
    /// read it back, and enter on the first call of a run shut the run.
    fn is_run_open(&self, at: usize) -> bool {
        self.said
            .get(at)
            .and_then(|said| said.run_opened)
            .unwrap_or(false)
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

    /// Lets go of whatever was held.
    pub const fn let_go(&mut self) {
        self.held = None;
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

    /// Marks the rows the reader has hold of.
    ///
    /// In two passes, because what a middle row holds cannot be worked out
    /// from the row alone. The ends of the selection are places in what was
    /// said; everything between them is held whole -- including the rows
    /// that came from nothing anybody wrote, the blank between two things
    /// said and the bullet in front of a list item. Those are on the screen
    /// between the words that are held, so they are part of what the reader
    /// is pointing at, and leaving them out would copy something other than
    /// what is in front of them.
    fn mark_held(&self, rows: &mut [Row]) {
        let Some((anchor, other)) = self.held else {
            return;
        };
        let (first, last) = match anchor <= other {
            true => (anchor, other),
            false => (other, anchor),
        };
        let mut ends: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let Some((said, source)) = row.from else {
                continue;
            };
            let here = (said, source);
            if here < (first.said, first.source) || here > (last.said, last.source) {
                continue;
            }
            let from = match here == (first.said, first.source) {
                true => first.at,
                false => 0,
            };
            let to = match here == (last.said, last.source) {
                true => last.at,
                false => usize::MAX,
            };
            if let Some(held) = row_held(row, from, to) {
                ends.push((index, held));
            }
        }
        let (Some((top, first_held)), Some((foot, last_held))) = (ends.first(), ends.last()) else {
            return;
        };
        let (top, foot) = (*top, *foot);
        for (index, row) in rows.iter_mut().enumerate() {
            if index < top || index > foot {
                continue;
            }
            let whole = row.text().chars().count();
            row.held = Some(match (index == top, index == foot) {
                (true, true) => first_held.start..last_held.end,
                (true, false) => first_held.start..whole,
                (false, true) => 0..last_held.end,
                (false, false) => 0..whole,
            });
        }
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

    /// Handles a key.
    ///
    /// `thinking` decides what escape means: while the agent is working it
    /// stops the agent, and otherwise there is nothing here to stop and the
    /// key is not this component's. Escape everywhere in Obelus means "stop
    /// what is happening", and once a conversation is a document rather than
    /// something over one, leaving it is not stopping anything.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        thinking: bool,
        room: Room,
        settings: &[obelus_agent::acp::Setting],
    ) -> ChatOutcome {
        let Some(modifiers) = obelus_editing::keymap::modifiers_of(key) else {
            return ChatOutcome::Ignored;
        };
        // The line break that every terminal can report. `shift+enter` is
        // the one a reader reaches for and it needs the kitty keyboard
        // protocol to arrive at all -- alt is the escape prefix, which is
        // as old as terminals.
        if key.code == KeyCode::Enter && modifiers == KeyModifiers::ALT {
            self.focus = Focus::Writing;
            self.input.newline();
            return ChatOutcome::Consumed;
        }
        // Everything else with a modifier on it is the application's:
        // a conversation is a document rather than something over one, so
        // `ctrl+q` leaves and `f2` lists what is open from inside it. A
        // conversation that swallowed those would be one a reader cannot
        // use Obelus from.
        //
        // Except the two ends of the transcript, which are nobody else's.
        // The arms below share Home and End three ways -- bare moves the
        // caret, shift holds what it passes over, control goes to the end
        // of the whole transcript -- and the modifier a reader reaches for
        // to jump to the end of a long document is control. It reached
        // nothing: the guard turned it away before the arm that was
        // waiting for it, and in a conversation there is no file for the
        // editor to take it instead, so the key did nothing at all.
        let ends = matches!(key.code, KeyCode::Home | KeyCode::End);
        if !ends && modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return ChatOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let page = usize::from(room.transcript).max(1);

        // The row of settings, while that is what the reader is in. What it
        // does not take falls through to the box below -- and the keys that
        // are the box's own take the focus back with them, because a reader
        // who starts typing means to type.
        if let Focus::Settings(at) = self.focus
            && let Some(outcome) = self.on_settings(key, bare, at, settings)
        {
            return outcome;
        }

        // The transcript, while the reader is walking it. What it does not
        // take falls through to the box below, the same way.
        if let Focus::Transcript(at) = self.focus
            && let Some(outcome) = self.on_transcript(key, modifiers, at, room)
        {
            return outcome;
        }

        match key.code {
            // Stopping the agent is the one thing escape does here. A
            // conversation is a document, not something over one, and
            // escape is what leaves whatever is over the document being
            // read -- so with nothing in flight there is nothing for it to
            // give up on, and it leaves the box alone rather than taking
            // the reader somewhere.
            KeyCode::Esc if bare && thinking => ChatOutcome::Interrupt,
            // Which is why the box takes shift: a message to an agent is a
            // paragraph, and enter is how you send one.
            KeyCode::Enter if !bare => {
                self.input.newline();
                ChatOutcome::Consumed
            }
            KeyCode::Enter => {
                if self.input.is_blank() {
                    return ChatOutcome::Consumed;
                }
                ChatOutcome::Send(self.input.take_parts())
            }
            // Shift and tab, which arrives as its own key and needs no
            // protocol to be asked for.
            KeyCode::BackTab => ChatOutcome::StepMode,

            KeyCode::Backspace if bare => {
                self.input.backspace();
                ChatOutcome::Consumed
            }
            KeyCode::Delete if bare => {
                self.input.delete();
                ChatOutcome::Consumed
            }
            KeyCode::Left if bare => {
                self.input.left();
                ChatOutcome::Consumed
            }
            KeyCode::Right if bare => {
                self.input.right();
                ChatOutcome::Consumed
            }
            // The box owns the arrows while its caret has somewhere to go
            // in it. Past the top of it the caret carries on into the
            // transcript, which is where a reader who has run out of box
            // presses the key next.
            KeyCode::Up if bare => {
                if self.input.up(room.writing) {
                    return ChatOutcome::Consumed;
                }
                // On to the last row, which is the words nearest the box
                // and so the ones the reader was looking at. Not the
                // nearest row that *does* something, which is where this
                // used to land: the cursor walks the words now, and tab is
                // what goes to the next thing enter opens.
                match self.back_into_the_transcript(room) {
                    Some(place) => {
                        self.focus = Focus::Transcript(place);
                        self.show_row(place.row, room);
                    }
                    None => self.scroll_by(-1),
                }
                ChatOutcome::Consumed
            }
            KeyCode::Down if bare => {
                // Down the box, then down the transcript, then out of the
                // box altogether: one key, walking whatever is still able
                // to move, in the order the things are on screen. The row
                // of settings is under the box, so it is last -- and only
                // once the transcript has nothing left to scroll, or the
                // way back down from having scrolled up would be gone.
                if self.input.down(room.writing) {
                    ChatOutcome::Consumed
                } else if self.window.at_the_end() {
                    if !settings.is_empty() {
                        self.focus = Focus::Settings(0);
                    }
                    ChatOutcome::Consumed
                } else {
                    self.scroll_by(1);
                    ChatOutcome::Consumed
                }
            }
            KeyCode::PageUp => {
                self.scroll_by(-isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::PageDown => {
                self.scroll_by(isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::Home if bare => {
                self.input.home(room.writing);
                ChatOutcome::Consumed
            }
            // Shift extends, here as everywhere. It used to be the
            // transcript's -- the top of a long answer, on the grounds
            // that the box had nowhere else to ask for it -- and then
            // control was given the two ends, which is what the rule over
            // the box names. So the reason was spent, and what was left
            // was shift naming a command in the one place a reader reaches
            // for it to hold a line: they pressed it to take back what
            // they had just written, and the whole conversation flew to
            // its beginning.
            KeyCode::Home if modifiers == KeyModifiers::SHIFT => {
                // One selection between the two halves, so taking hold in
                // here lets the transcript go.
                self.let_go();
                self.input.hold_home(room.writing);
                ChatOutcome::Consumed
            }
            // And control is the two ends of the transcript, from the box
            // as from inside it.
            KeyCode::Home => {
                self.window.home();
                ChatOutcome::Consumed
            }
            KeyCode::End if bare => {
                self.input.end(room.writing);
                ChatOutcome::Consumed
            }
            KeyCode::End if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_end(room.writing);
                ChatOutcome::Consumed
            }
            KeyCode::End => {
                self.window.end();
                ChatOutcome::Consumed
            }
            KeyCode::Char(character) => {
                self.input.insert(character);
                ChatOutcome::Consumed
            }
            _ => ChatOutcome::Ignored,
        }
    }

    /// What a key does while the row of settings is what the reader is in.
    ///
    /// `None` means the key is not this row's: the box below gets it, and
    /// for the keys that are the box's own -- typing, and the two that
    /// delete -- the focus goes back there first.
    fn on_settings(
        &mut self,
        key: &KeyEvent,
        bare: bool,
        at: usize,
        settings: &[obelus_agent::acp::Setting],
    ) -> Option<ChatOutcome> {
        match key.code {
            // Along the row, and round: it is a short cycle, and a reader
            // walking off one end means the other end.
            KeyCode::Left if bare && !settings.is_empty() => {
                let last = settings.len() - 1;
                self.focus = Focus::Settings(if at == 0 { last } else { at - 1 });
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Right if bare && !settings.is_empty() => {
                self.focus = Focus::Settings((at + 1) % settings.len());
                Some(ChatOutcome::Consumed)
            }
            // Whatever the setting under the focus is: a list of values is
            // a list to open, and a switch has nowhere to go, so it flips.
            // The same judgement the row's drawing makes.
            KeyCode::Enter if bare => Some(settings.get(at).map_or(
                ChatOutcome::Consumed,
                |setting| match setting.kind {
                    obelus_agent::acp::Kind::Select => ChatOutcome::Choose(setting.id.clone()),
                    obelus_agent::acp::Kind::Switch => ChatOutcome::Toggle(setting.id.clone()),
                },
            )),
            // Back to the box. Escape as well, because escape gives up on
            // the nearest thing first, and the nearest thing is being here
            // rather than the whole conversation.
            KeyCode::Up | KeyCode::Esc if bare => {
                self.focus = Focus::Writing;
                Some(ChatOutcome::Consumed)
            }
            // Nothing is under this row.
            KeyCode::Down if bare => Some(ChatOutcome::Consumed),
            // The box's own keys, which take the focus back with them.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter => {
                self.focus = Focus::Writing;
                None
            }
            // Everything else -- the paging keys, the ends of the
            // transcript -- goes on meaning what it means, and the focus
            // stays where the reader put it.
            _ => None,
        }
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
        });
    }

    /// Scrolls by rows.
    fn scroll_by(&mut self, rows: isize) {
        self.window.scroll(rows);
    }

    /// The same, for a caller holding the rows already.
    fn in_view_of(&self, count: usize, room: Room) -> std::ops::Range<usize> {
        let top = self.window.top();
        top.min(count)..(top + usize::from(room.transcript)).min(count)
    }

    /// Puts a row of the transcript on screen, moving the window as little
    /// as it takes.
    fn show_row(&mut self, at: usize, room: Room) {
        let count = self.rows(room.reading).len();
        self.show_to(at, count, room);
    }

    /// The same, for a caller holding the rows already.
    fn show_to(&mut self, at: usize, count: usize, room: Room) {
        let view = self.in_view_of(count, room);
        if at < view.start {
            self.scroll_by(-isize::try_from(view.start - at).unwrap_or(1));
        } else if at >= view.end {
            self.scroll_by(isize::try_from(at + 1 - view.end).unwrap_or(1));
        }
    }

    /// Goes back to the box, remembering which column of the transcript
    /// this was.
    fn leave_the_transcript(&mut self, at: Place) {
        self.stood = Some(at.character);
        self.focus = Focus::Writing;
    }

    /// Where the cursor goes when it comes in from the box.
    ///
    /// The last row always: it is the words nearest the box, and it is
    /// where whatever was said while the reader was typing has gone. And
    /// the column it left in, where it has been in here -- otherwise the
    /// end of that row.
    fn back_into_the_transcript(&self, room: Room) -> Option<Place> {
        let rows = self.rows(room.reading);
        let row = rows.len().checked_sub(1)?;
        let characters = rows[row].characters();
        Some(Place {
            row,
            // The row is not the one the column was left in, and even if
            // it were it may be a different length than it was: the window
            // is resized, a run is opened, the agent says something that
            // reflows what is above it.
            character: self.stood.unwrap_or(characters).min(characters),
        })
    }

    /// The next row worth standing on above or below `at`, among rows the
    /// caller is holding already.
    fn next_stop_in(at: usize, up: bool, laid: &[Row]) -> Option<usize> {
        let mut stops = laid
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at);
        match up {
            true => stops.take_while(|stop| *stop < at).last(),
            false => stops.find(|stop| *stop > at),
        }
    }

    /// The place in the words the cursor is at, or the nearest there is.
    ///
    /// Nearest, because a cursor can stand where there are no words: on a
    /// blank, on a plan's step, on the heading over a folded run. A
    /// selection started from one of those has to begin somewhere, and the
    /// somewhere a reader means is the words they are next to.
    fn spot_near(place: Place, laid: &[Row]) -> Option<Spot> {
        let rows = laid;
        if let Some(spot) = rows.get(place.row)?.spot_at(place.character) {
            return Some(spot);
        }
        // Downwards first: the rows Obelus draws itself sit above what they
        // are about -- a heading over its run, a blank before what follows
        // it -- so the words they belong to are the ones under them.
        rows.iter()
            .skip(place.row)
            .find_map(|row| row.spot_at(0))
            .or_else(|| {
                rows.iter()
                    .take(place.row)
                    .rev()
                    .find_map(|row| row.spot_at(row.characters()))
            })
    }

    /// Where a motion takes the cursor, or nothing where it runs out of
    /// transcript.
    ///
    /// All of it in rows and characters, because that is what the keys
    /// mean: a reader pressing the right arrow means the next character on
    /// the screen, wherever in the words it happens to come from.
    fn walked(place: Place, key: KeyCode, laid: &[Row], room: Room) -> Option<Place> {
        let characters = |row: usize| laid.get(row).map_or(0, Row::characters);
        let rows = laid.len();
        let here = characters(place.row);
        Some(match key {
            KeyCode::Right if place.character < here => Place {
                character: place.character + 1,
                ..place
            },
            // Off the end of a row and on to the start of the next, which
            // is where the words carry on.
            KeyCode::Right if place.row + 1 < rows => Place {
                row: place.row + 1,
                character: 0,
            },
            KeyCode::Left if place.character > 0 => Place {
                character: place.character - 1,
                ..place
            },
            KeyCode::Left if place.row > 0 => Place {
                row: place.row - 1,
                character: characters(place.row - 1),
            },
            // Up and down keep the column where they can, the way they do
            // in any text: a row too short for it takes the caret to its
            // end rather than refusing the key.
            KeyCode::Up if place.row > 0 => Place {
                row: place.row - 1,
                character: place.character.min(characters(place.row - 1)),
            },
            KeyCode::Down if place.row + 1 < rows => Place {
                row: place.row + 1,
                character: place.character.min(characters(place.row + 1)),
            },
            KeyCode::Home => Place {
                character: 0,
                ..place
            },
            KeyCode::End => Place {
                character: here,
                ..place
            },
            KeyCode::PageUp => {
                let page = usize::from(room.transcript.max(1));
                let row = place.row.saturating_sub(page);
                Place {
                    row,
                    character: place.character.min(characters(row)),
                }
            }
            KeyCode::PageDown => {
                let page = usize::from(room.transcript.max(1));
                let row = (place.row + page).min(rows.saturating_sub(1));
                Place {
                    row,
                    character: place.character.min(characters(row)),
                }
            }
            _ => return None,
        })
    }

    /// Walks the transcript with the cursor, and holds what it walks over
    /// while shift is down.
    ///
    /// What it does not take falls through to the box below, which is what
    /// keeps a reader from ever being stuck in here: the keys that are the
    /// box's own take the focus back with them.
    fn on_transcript(
        &mut self,
        key: &KeyEvent,
        modifiers: KeyModifiers,
        at: Place,
        room: Room,
    ) -> Option<ChatOutcome> {
        let bare = modifiers == KeyModifiers::NONE;
        // Laid out once, and everything below asks these rows rather than
        // the conversation. A width makes rows out of every word ever said
        // in it, which is real work -- tens of milliseconds for a long
        // morning's conversation -- and a cursor that moves on every arrow
        // pays it on every arrow. Asked five times over for one keypress,
        // as this was, the caret crawls.
        let laid = self.rows(room.reading);
        // What shift means on a motion is "and hold what I pass over", and
        // it is the only modifier that does: control reaches here too --
        // the guard lets the two ends through -- and asking only whether
        // the key was bare made `ctrl+end` hold the row it was on instead
        // of going to the end, while the rule over the box went on naming
        // it as the way back.
        let holding = modifiers == KeyModifiers::SHIFT;
        match key.code {
            // Which is that way back, with the cursor in here: the two ends
            // of the transcript. They move the cursor and the view follows,
            // because in here the keys move the cursor -- a view sent to
            // the end with the cursor left behind is dragged back by the
            // next arrow.
            KeyCode::Home | KeyCode::End if modifiers == KeyModifiers::CONTROL => {
                let last = laid.len().saturating_sub(1);
                let (row, character) = match key.code {
                    KeyCode::Home => (0, 0),
                    _ => (last, laid.get(last).map_or(0, Row::characters)),
                };
                self.let_go();
                self.focus = Focus::Transcript(Place { row, character });
                match key.code {
                    KeyCode::Home => self.window.home(),
                    _ => self.window.end(),
                }
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => {
                let moved = Self::walked(at, key.code, &laid, room);
                // Down off the end of the transcript is the box, which is
                // where a reader who has walked to the bottom is going
                // next. Every other motion that runs out simply stops.
                let Some(moved) = moved else {
                    if key.code == KeyCode::Down && !holding {
                        self.let_go();
                        self.leave_the_transcript(at);
                        return Some(ChatOutcome::Consumed);
                    }
                    return Some(ChatOutcome::Consumed);
                };
                match holding {
                    // A shift-motion with nothing held yet starts the
                    // selection where the cursor was, not where it is
                    // going: what the reader means to hold is what the key
                    // passed over.
                    true => {
                        if self.held.is_none()
                            && let Some(from) = Self::spot_near(at, &laid)
                        {
                            self.hold_from(from);
                            // One selection between the two halves, so
                            // taking hold here lets the box go.
                            self.input.let_go();
                        }
                        if let Some(to) = Self::spot_near(moved, &laid) {
                            self.hold_to(to);
                        }
                    }
                    // And a bare one lets go, the way it does in the box
                    // and in the file: a caret moved on its own is a
                    // reader who has finished with what they had.
                    false => self.let_go(),
                }
                self.focus = Focus::Transcript(moved);
                self.show_to(moved.row, laid.len(), room);
                Some(ChatOutcome::Consumed)
            }
            // The next thing enter would open, which is what the arrows
            // used to land on and no longer do: they walk the words now, so
            // getting to a heading in a long run of them is its own key.
            KeyCode::Tab | KeyCode::BackTab => {
                let up = key.code == KeyCode::BackTab;
                // Consumed either way. Nothing that way is a key that does
                // nothing, not a key that falls through: shift and tab
                // steps the agent's way of working, and a reader who
                // pressed it once too often while reading would have
                // changed how the agent works without meaning to.
                let Some(stop) = Self::next_stop_in(at.row, up, &laid) else {
                    return Some(ChatOutcome::Consumed);
                };
                self.let_go();
                self.focus = Focus::Transcript(Place {
                    row: stop,
                    character: 0,
                });
                self.show_to(stop, laid.len(), room);
                Some(ChatOutcome::Consumed)
            }
            // Whatever the row is: a heading opens and closes what is
            // under it, and a row that names a file goes there. Both are
            // "do what this row is for", which is what enter means
            // everywhere else in Obelus. On a row that is only words it
            // does nothing, because there is nothing there to do.
            KeyCode::Enter if bare => {
                let row = laid.get(at.row).cloned();
                match row {
                    Some(row) => match (row.unsent, row.folds, row.place, row.away) {
                        // Something they said that has not gone: the only
                        // row here whose key gives rather than opens.
                        (Some(which), ..) => match self.take_back(which) {
                            // Back to the box with them, where the caret
                            // is: taking something back is almost always
                            // meaning to say it again differently.
                            Some(words) => {
                                self.leave_the_transcript(at);
                                Some(ChatOutcome::TakeBack(words))
                            }
                            None => Some(ChatOutcome::Consumed),
                        },
                        (None, Some(begins), _, _) => {
                            self.fold(begins);
                            // The heading stays under the reader: what
                            // moved is what is below it.
                            self.show_to(at.row, laid.len(), room);
                            Some(ChatOutcome::Consumed)
                        }
                        (None, None, Some((place, _)), _) => Some(ChatOutcome::GoTo(place)),
                        // And a row that points at a web address goes
                        // there, which is the same rule about the same key.
                        (None, None, None, Some(url)) => Some(ChatOutcome::Away(url)),
                        (None, None, None, None) => Some(ChatOutcome::Consumed),
                    },
                    None => Some(ChatOutcome::Consumed),
                }
            }
            // Back to the box: escape gives up on the nearest thing first,
            // and the nearest thing is walking about in here. What is held
            // goes with it -- a selection nobody can see the cursor of is
            // one the reader has left behind.
            KeyCode::Esc if bare => {
                self.let_go();
                self.leave_the_transcript(at);
                Some(ChatOutcome::Consumed)
            }
            // The box's own keys take the focus back with them, because a
            // reader who starts typing means to type.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter => {
                self.let_go();
                self.leave_the_transcript(at);
                None
            }
            _ => None,
        }
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
    fn the_cursor_walks_the_words_and_tab_goes_to_what_acts() {
        let mut chat = walked();
        let rows = chat.rows(ROOM.reading);
        let stops: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at)
            .collect();
        assert_eq!(stops.len(), 2, "the tool calls are the two stops");
        let last = rows.len() - 1;

        // Up from the box lands at the end of the last row, which is the
        // words nearest the box.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: rows[last].characters(),
            })
        );

        // The left arrow walks back along it a character at a time, rather
        // than leaving the row altogether.
        chat.handle_key(&key(KeyCode::Left), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: last,
                character: rows[last].characters() - 1,
            }),
            "the left arrow did not walk the words"
        );
        // And home takes it to the start of the row it is on.
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[]);
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
        assert_eq!(last, stops[1], "the last row is the second tool call");
        chat.handle_key(&key(KeyCode::BackTab), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[0],
                character: 0
            }),
            "shift and tab did not reach the thing enter opens"
        );

        // Enter opens what the row names.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
            ChatOutcome::GoTo(place("/a.rs", 3))
        );
        // And on a row of nothing but words it does nothing, rather than
        // doing whatever the last row it was on did.
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
            ChatOutcome::Consumed,
            "enter on a row of words did something"
        );
        // And tab forwards is the way back to the other one.
        chat.handle_key(&key(KeyCode::Tab), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[1],
                character: 0
            }),
            "tab did not reach the next thing enter opens"
        );

        // Escape is the way back: it gives up on the nearest thing, which
        // is being in the transcript rather than the conversation.
        let outcome = chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[]);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Writing, "escape did not reach the box");
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Writing, "down did not reach the box");
        chat.chunk(
            Speaker::Agent,
            ", and here is the rest of it, which arrived while the reader \
             was writing",
        );
        chat.settle(chat.rows(ROOM.reading).len(), ROOM.transcript);
        let grown = chat.rows(ROOM.reading).len() - 1;
        assert!(grown > foot, "the rest of the answer made no rows");
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]);
        chat.scroll(-1);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[]);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[]);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("re".to_string(), "selection"),
            "shift and the left arrow did not hold what it passed over"
        );

        // And a bare motion lets go.
        chat.handle_key(&key(KeyCode::Left), false, ROOM, &[]);
        assert!(!chat.holding(), "a bare motion kept the selection");
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

        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Home), false, ROOM, &[]);
        chat.handle_key(&shifted(KeyCode::End), false, ROOM, &[]);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("a message I typed".to_string(), "selection"),
            "shift and end did not hold the line in the box"
        );
        assert!(chat.at_the_end(), "shift and end moved the transcript");

        // One selection between the two halves: taking hold in the box
        // lets the transcript go. Up twice, because the first press is
        // what lets go of what the box is holding.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&shifted(KeyCode::Left), false, ROOM, &[]);
        assert!(chat.holding(), "the transcript did not take hold");
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[]);
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
        chat.handle_key(&control(KeyCode::Home), false, short, &[]);
        assert_eq!(chat.focus(), Focus::Writing, "the box lost the keys");
        assert!(!chat.at_the_end(), "control and home did not leave the end");

        // And from inside it, where the cursor goes with the view.
        chat.handle_key(&key(KeyCode::Up), false, short, &[]);
        chat.handle_key(&control(KeyCode::End), false, short, &[]);
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

        chat.handle_key(&control(KeyCode::Home), false, short, &[]);
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
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        for _ in 0..3 {
            chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        }
        for _ in 0..9 {
            chat.handle_key(&key(KeyCode::Right), false, ROOM, &[]);
        }
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: 0,
                character: 9
            }),
            "the cursor is not in the middle of the first bullet"
        );

        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[]);
        assert_eq!(
            chat.copied(ROOM.reading),
            ("alpha b".to_string(), "selection"),
            "shift and home did not hold what is before the cursor"
        );

        // The same on a quote, whose bar is drawn the same way.
        for _ in 0..3 {
            chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        }
        for _ in 0..7 {
            chat.handle_key(&key(KeyCode::Right), false, ROOM, &[]);
        }
        chat.handle_key(&shifted(KeyCode::Home), false, ROOM, &[]);
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
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(Place {
                row: stops[1],
                character: chat.rows(ROOM.reading)[stops[1]].characters(),
            }),
            "the cursor did not come in at the end of the last row"
        );
        chat.handle_key(&key(KeyCode::PageUp), false, ROOM, &[]);
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

    /// Typing takes the cursor back to the box, wherever it was.
    ///
    /// A reader who starts typing means to type -- the same rule the row of
    /// settings under the box follows.
    #[test]
    fn typing_in_the_transcript_goes_to_the_box() {
        let mut chat = walked();
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        chat.handle_key(&key(KeyCode::Char('h')), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Writing);
        assert_eq!(chat.writing().text(), "h");
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
            chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        }
        chat.settle(rows, 10);
        assert_eq!(chat.top(), rows - 10);
        chat.note("and another");
        let rows = chat.rows(40).len();
        chat.settle(rows, 10);
        assert_eq!(chat.top(), rows - 10, "it stopped following the end");
    }

    /// What escape means depends on whether anything is happening, and
    /// nothing else about the keys does.
    #[test]
    fn escape_stops_the_agent_and_otherwise_does_nothing() {
        let mut chat = Chat::new();
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), true, ROOM, &[]),
            ChatOutcome::Interrupt
        );
        // And with nothing in flight it is not the conversation's key: a
        // conversation is a document, and escape leaves what is *over* a
        // document. There is nothing over this one.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]),
            ChatOutcome::Ignored
        );
    }

    /// Typing goes into the row being typed, and enter sends it.
    #[test]
    fn what_is_typed_is_sent_once() {
        let mut chat = Chat::new();
        for character in "hello".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, ROOM, &[]);
        }
        chat.handle_key(&key(KeyCode::Backspace), false, ROOM, &[]);
        assert_eq!(chat.writing().text(), "hell");

        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
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
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
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
            chat.handle_key(&quit, false, ROOM, &[]),
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
