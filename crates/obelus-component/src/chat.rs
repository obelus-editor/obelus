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
//! it was in (`locations`), and obelus kept neither: a row with a title and a
//! tick on it. The kind picks the glyph, because a reader scanning a turn is
//! looking for whether it *changed* anything and that is a picture rather than
//! a sentence; the locations go on the row as a path, written relative to the
//! tree obelus was opened on. Other clients open a preview from one of these;
//! obelus opens a *buffer* -- with its jump list, its definitions, its hunks --
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
//! Thinking is not folded away. Folding is for repetition, and thinking is
//! prose -- often the most of what a turn is worth, since it is where the agent
//! says why it thinks the bug is where it thinks it is. It gets a heading so a
//! reader who has read it can put it away, and only when it is long enough for
//! that to be worth a row: a heading over three words is two rows saying one
//! thing. obelus never closes it by itself, which also means it can never close
//! under somebody who is reading it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_reading::{Ink, Span};

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
    /// obelus itself: what went wrong, what was allowed, what stopped.
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
/// Handed over rather than read: obelus holds the process, and the rows
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
    /// The files a tool call named.
    ///
    /// Kept whole rather than as one line of text, because they are places
    /// to go: obelus opens files for a living, and a tool call is the agent
    /// saying which ones it has been in.
    pub places: Vec<obelus_agent::acp::Place>,
    /// The change it is making, as the rows that draw it.
    ///
    /// Empty for everything that is not a change. What is kept is the rows
    /// rather than the two texts: the diff is worked out once, when it
    /// arrives.
    pub change: Vec<obelus_git::change::Line>,
    /// The command obelus is running for this call, where it is running
    /// one.
    ///
    /// The output is not kept here: obelus holds the process, and what it
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
    /// `None` means nobody has said, and obelus decides: a run of tool
    /// calls of one kind folds itself once there are enough of them to be a
    /// log rather than a story, and its thinking stays open because
    /// thinking is prose somebody may want to read. Once the reader says
    /// otherwise it stays the way they left it.
    pub opened: Option<bool>,
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
    /// Beside `place` and not folded into it: a file obelus opens itself
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
    pub folds: Option<usize>,
    /// Whether what it folds is open, for the mark that says so.
    pub open: bool,
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
    /// `None` for the rows obelus makes up rather than lays out: the blank
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
        self.place.is_some() || self.folds.is_some() || self.away.is_some()
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
    Send(String),
    /// Ask the agent to stop.
    Interrupt,
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
    /// A row of the transcript, by where it is in the rows as they are
    /// drawn. Only ever a row that does something.
    Transcript(usize),
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
    /// The same window every list in obelus has, in its following form: it
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
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// Whether what this voice says arrives as the protocol's own text
/// content, which is the thing it asks clients to read as markdown.
///
/// The agent's answer and its thinking do. The reader's own message does
/// not -- obelus has it as they typed it, and reflowing somebody's words
/// back at them is obelus deciding what they meant. Nor do the rows obelus
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
/// lines of a change, nor the reader's own message -- obelus has that as
/// they typed it and has no business reflowing it -- nor anything obelus
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
    obelus_reading::markdown::render(text, width)
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
            doing: None,
            plan: Vec::new(),
            plan_open: false,
            input: Composer::new(),
            window: Window::following(),
            focus: Focus::Writing,
            held: None,
            left_at: None,
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
    /// there, once a frame, like every other window in obelus.
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

    /// Whether anybody but obelus has said anything in it.
    ///
    /// Which is not "is it empty": obelus writes in a conversation of its
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
    /// invoke rather than obelus's to guess at.
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
    /// list in obelus decides it: a bar on something that fits is a bar
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
    pub fn asked(&mut self, text: &str) {
        self.push(Speaker::Reader, text, None);
    }

    /// Takes the reader's own words back from the agent.
    ///
    /// What this is for is a conversation taken up again: the agent replays
    /// it to a client that may be a fresh process, and the reader's half of
    /// it comes back only this way.
    ///
    /// Some agents also send these during a live turn, echoing back the
    /// prompt they were just given -- which obelus put on the page itself
    /// the moment it was sent. So a chunk the last thing said already says
    /// is dropped: what makes that safe is that the only rows obelus writes
    /// in this voice are the ones it was handed by the reader, so a repeat
    /// of what is already there is the agent's copy of it and not a second
    /// thing they said.
    pub fn heard(&mut self, text: &str) {
        let echoed = self
            .said
            .last()
            .is_some_and(|last| last.speaker == Speaker::Reader && last.text.contains(text));
        if !echoed {
            self.chunk(Speaker::Reader, text);
        }
    }

    /// Adds one of obelus's own remarks.
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
        self.plan = steps;
    }

    /// Forgets it, which a new turn does.
    ///
    /// An agent that sends a plan for one turn and none for the next would
    /// otherwise have the first one shown against the second's work.
    pub fn plan_forgotten(&mut self) {
        self.plan.clear();
        self.plan_open = false;
    }

    /// Says what is happening now, or that nothing is.
    ///
    /// Called with whatever the state is, every frame: it is read from the
    /// state rather than remembered, so there is no way for it to be left
    /// behind.
    pub fn doing(&mut self, what: Option<&str>) {
        // Written down when it moves. This is the row a reader watches to
        // know whether anything is happening at all, and when it says
        // nothing there is nothing else on screen to say why -- so a
        // report that it stayed blank is a report with no evidence in it,
        // and the log is where obelus keeps what a reader cannot show.
        //
        // On the change and not the frame: this is called twelve times a
        // second with the same answer.
        if self.doing.as_deref() != what {
            tracing::info!(was = ?self.doing, now = ?what, "what is happening now");
        }
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
        self.said.push(Said {
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

    /// Says what a command is doing, for every row that is running one.
    ///
    /// Called every frame with whatever obelus's own runner has, the way
    /// [`Chat::doing`] is: the output belongs to the process and the rows
    /// are drawn from it rather than from a copy that could be a moment
    /// behind. Which command a row is about is the row's own `ran`, so a
    /// caller hands over one answer per command and this finds them.
    pub fn running(&mut self, what: &dyn Fn(&str) -> Option<Doing>) {
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
            said.words = vec![words];
            // And how it is going, which is the call's state rather than a
            // second mark: a call running a command that failed is a call
            // that failed.
            if let Some(state) = state {
                said.state = Some(state);
            }
        }
    }

    /// Says what was to happen where the reader was sent has happened.
    ///
    /// Nothing at all for a name nothing is waiting on: an agent may say a
    /// question is over that this obelus never asked -- a second one is
    /// listening to the same session -- and a row invented to mark it done
    /// would be a row about something the reader never did.
    pub fn arrived(&mut self, id: &str) {
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
        let existing = self
            .said
            .iter_mut()
            .rev()
            .find(|said| said.tag.as_deref() == Some(call.id.as_str()));
        let Some(said) = existing else {
            self.said.push(Said {
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
        // call rather than something for obelus to overrule.
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
                folds: planning.then_some(self.said.len()),
                open: self.plan_open,
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
                            marker: None,
                            changed: None,
                            depth: 1,
                        })
                }));
            }
        }
        // And what the reader has hold of, once there are rows to hold:
        // the two ends of a selection are places in what was said, and
        // which characters of which rows that is depends on the width
        // these were just laid out at.
        self.mark_held(&mut rows);
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
        let open = self.is_open(run.start);
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
            folds: Some(run.start),
            open,
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
            let mut rows = vec![Row {
                changed: (!said.change.is_empty())
                    .then(|| obelus_git::change::counted(&said.change)),
                ..self.opening(
                    said,
                    plain(said.text.clone()),
                    depth,
                    Some(at),
                    Some((at, Source::Text)),
                )
            }];
            if self.is_open(at) {
                // Markdown, unless obelus is running a command for this
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
        // its own, which is what folds it. obelus does not fold it away by
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
        // The heading is obelus's word for what is behind it, not the
        // agent's: there is nothing in it to take a copy of.
        let mut rows =
            vec![self.opening(said, plain("thought".to_string()), depth, Some(at), None)];
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
        folds: Option<usize>,
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
            open: folds.is_some_and(|at| self.is_open(at)),
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
            marker: None,
            changed: None,
            depth: 0,
        }
    }

    /// Whether what begins at `at` is open.
    ///
    /// What the reader said, and otherwise what obelus makes of it: a run
    /// of tool calls folds itself, unless one of them failed -- a failure
    /// is the one thing in a turn nobody may have to go looking for.
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
        // made the file itself has them, and obelus draws a file's changes
        // in the margin beside them -- so the block folds away and the row
        // that opens it stays.
        //
        // And open once it has finished badly, which that reason does not
        // cover: what a change said is in the file afterwards, and what a
        // command printed is nowhere at all. A reader whose tests have just
        // failed is looking for the failure, and folding it away hands them
        // a row and a glyph. The same rule the arm below has for a run of
        // calls, which is where it was already written down.
        if !said.change.is_empty() || !said.words.is_empty() {
            return matches!(
                said.state.as_deref(),
                Some("pending" | "in_progress" | "failed")
            );
        }
        self.said[self.run_from(at)]
            .iter()
            .any(|said| said.state.as_deref() == Some("failed"))
    }

    /// Opens what is closed and closes what is open.
    pub fn fold(&mut self, at: usize) {
        // One past the end is the plan, which is not a thing that was said
        // and so has no index among them. It is the one index that can
        // never name a [`Said`], which is what makes it safe to mean
        // something else.
        if at == self.said.len() {
            self.plan_open = !self.plan_open;
            return;
        }
        let open = self.is_open(at);
        if let Some(said) = self.said.get_mut(at) {
            said.opened = Some(!open);
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
    /// went past. obelus's own notes are not news either: they are obelus
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
        self.scroll_by(rows);
    }

    /// Handles a key.
    ///
    /// `thinking` decides what escape means: while the agent is working it
    /// stops the agent, and otherwise there is nothing here to stop and the
    /// key is not this component's. Escape everywhere in obelus means "stop
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
        // use obelus from.
        //
        // Except the two ends of the transcript, which are nobody else's.
        // The arms below already share Home and End between the box and
        // the transcript -- bare is the box's, with a modifier is the
        // transcript's -- and the modifier a reader reaches for to jump to
        // the end of a long document is control. It reached nothing: the
        // guard turned it away before the arm that was waiting for it, and
        // in a conversation there is no file for the editor to take it
        // instead, so the key did nothing at all.
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
            && let Some(outcome) = self.on_transcript(key, bare, at, room)
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
                ChatOutcome::Send(self.input.take())
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
            // The box owns the arrows, because it is what has a caret in
            // it. The transcript has no caret and scrolls by pages and by
            // the wheel -- and by the arrows once the caret is at the edge
            // of the box, which is where a reader presses them next.
            KeyCode::Up if bare => {
                if self.input.up(room.writing) {
                    return ChatOutcome::Consumed;
                }
                // Out of the box and into the transcript, onto the row
                // nearest the box that does something. Where there is no
                // such row -- a conversation of nothing but words, which is
                // most of them -- the key scrolls, as it always has.
                match self.nearest_stop(room) {
                    Some(stop) => {
                        self.focus = Focus::Transcript(stop);
                        self.show_row(stop, room);
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
            // Shift and home is the transcript's, because the box's home is
            // the row it is on: a reader who wants the top of a long answer
            // has nowhere else to ask for it.
            KeyCode::Home => {
                self.window.home();
                ChatOutcome::Consumed
            }
            KeyCode::End if bare => {
                self.input.end(room.writing);
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

    /// What a copy takes out of the box: what is held, or the whole of
    /// what has been written.
    ///
    /// The rule the file follows with its line and the notes follow with
    /// their note: copying nothing is not something a key can usefully do.
    #[must_use]
    pub fn copied(&self) -> (String, &'static str) {
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
        self.said.push(Said {
            speaker,
            text: text.to_string(),
            tag,
            state: None,
            kind: String::new(),
            places: Vec::new(),
            change: Vec::new(),
            ran: None,
            words: Vec::new(),
            opened: None,
        });
    }

    /// Scrolls by rows.
    fn scroll_by(&mut self, rows: isize) {
        self.window.scroll(rows);
    }

    /// The rows of the transcript a cursor can stand on.
    fn stops(&self, width: u16) -> Vec<usize> {
        self.rows(width)
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at)
            .collect()
    }

    /// Which rows are on screen, given the room the transcript has.
    fn in_view(&self, room: Room) -> std::ops::Range<usize> {
        let top = self.window.top();
        let count = self.rows(room.reading).len();
        top.min(count)..(top + usize::from(room.transcript)).min(count)
    }

    /// Puts a row of the transcript on screen, moving the window as little
    /// as it takes.
    fn show_row(&mut self, at: usize, room: Room) {
        let view = self.in_view(room);
        if at < view.start {
            self.scroll_by(-isize::try_from(view.start - at).unwrap_or(1));
        } else if at >= view.end {
            self.scroll_by(isize::try_from(at + 1 - view.end).unwrap_or(1));
        }
    }

    /// The next row worth standing on above or below `at`, if there is one
    /// that way.
    fn next_stop(&self, at: usize, up: bool, room: Room) -> Option<usize> {
        let stops = self.stops(room.reading);
        match up {
            true => stops.iter().rev().find(|stop| **stop < at).copied(),
            false => stops.iter().find(|stop| **stop > at).copied(),
        }
    }

    /// The row worth standing on nearest the box, among those on screen.
    ///
    /// Which is where the cursor comes in from the box, and where a page
    /// leaves it. Nothing on screen to stand on means the key was not a
    /// walk at all, and the caller scrolls instead.
    fn nearest_stop(&self, room: Room) -> Option<usize> {
        let view = self.in_view(room);
        self.stops(room.reading)
            .iter()
            .rev()
            .find(|stop| view.contains(stop))
            .copied()
    }

    /// Walks the transcript, or gives up and says so.
    ///
    /// The arrows move the nearest thing that can still move: a row to
    /// stand on where there is one, and the view itself where there is
    /// not. A transcript of nothing but prose -- which is most of them --
    /// therefore scrolls by a row exactly as it always has.
    fn on_transcript(
        &mut self,
        key: &KeyEvent,
        bare: bool,
        at: usize,
        room: Room,
    ) -> Option<ChatOutcome> {
        match key.code {
            KeyCode::Up if bare => {
                match self.next_stop(at, true, room) {
                    Some(stop) => {
                        self.focus = Focus::Transcript(stop);
                        self.show_row(stop, room);
                    }
                    None => self.scroll_by(-1),
                }
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Down if bare => {
                match self.next_stop(at, false, room) {
                    Some(stop) => {
                        self.focus = Focus::Transcript(stop);
                        self.show_row(stop, room);
                    }
                    // Under the last of them is the box, which is where a
                    // reader who has walked to the end of the transcript
                    // is going next.
                    None => self.focus = Focus::Writing,
                }
                Some(ChatOutcome::Consumed)
            }
            // A page moves the view and takes the cursor with it, onto the
            // nearest row it can stand on in what is now on screen. The
            // wheel is the other way about -- it moves the view and leaves
            // the cursor -- because a reader spinning it is looking around
            // rather than going somewhere.
            KeyCode::PageUp | KeyCode::PageDown if bare => {
                let page = isize::try_from(room.transcript.max(1)).unwrap_or(1);
                self.scroll_by(match key.code {
                    KeyCode::PageUp => -page,
                    _ => page,
                });
                if let Some(stop) = self.nearest_stop(room) {
                    self.focus = Focus::Transcript(stop);
                }
                Some(ChatOutcome::Consumed)
            }
            // Whatever the row is: a heading opens and closes what is
            // under it, and a row that names a file goes there. Both are
            // "do what this row is for", which is what enter means
            // everywhere else in obelus.
            KeyCode::Enter if bare => {
                let row = self.rows(room.reading).get(at).cloned();
                match row {
                    Some(row) => match (row.folds, row.place, row.away) {
                        (Some(begins), _, _) => {
                            self.fold(begins);
                            // The heading stays under the reader: what
                            // moved is what is below it.
                            self.show_row(at, room);
                            Some(ChatOutcome::Consumed)
                        }
                        (None, Some((place, _)), _) => Some(ChatOutcome::GoTo(place)),
                        // And a row that points at a web address goes
                        // there, which is the same rule about the same key.
                        (None, None, Some(url)) => Some(ChatOutcome::Away(url)),
                        (None, None, None) => Some(ChatOutcome::Consumed),
                    },
                    None => Some(ChatOutcome::Consumed),
                }
            }
            // Back to the box: escape gives up on the nearest thing first,
            // and the nearest thing is walking about in here.
            KeyCode::Esc if bare => {
                self.focus = Focus::Writing;
                Some(ChatOutcome::Consumed)
            }
            // The box's own keys take the focus back with them, because a
            // reader who starts typing means to type.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter => {
                self.focus = Focus::Writing;
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

    /// A screen with ten rows of transcript and a box twenty cells wide.
    const ROOM: Room = Room {
        transcript: 10,
        reading: 40,
        writing: 20,
    };

    /// A tool call, as an agent sends one.
    fn call(
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
        chat.asked("what is this file");
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

    /// The cursor walks the rows that do something and steps over the rest.
    ///
    /// Prose is not a place to stand: a reader walking the transcript is
    /// looking for what the agent *did*, and a cursor that stopped on every
    /// line of an answer would take a dozen keys to cross one.
    #[test]
    fn the_cursor_stands_only_on_rows_that_do_something() {
        let mut chat = walked();
        let rows = chat.rows(ROOM.reading);
        let stops: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.acts())
            .map(|(at, _)| at)
            .collect();
        assert_eq!(stops.len(), 2, "the tool calls are the two stops");

        // Up from the box reaches the one nearest it, and up again the one
        // before that. What is between them is walked over.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Transcript(stops[1]));
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Transcript(stops[0]));
        // And there is nothing above it, so the key scrolls instead of
        // leaving the cursor somewhere it cannot be.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Transcript(stops[0]));

        // Enter opens what the row names.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
            ChatOutcome::GoTo(place("/a.rs", 3))
        );

        // Down walks back, and past the last one is the box.
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        assert_eq!(chat.focus(), Focus::Transcript(stops[1]));
        chat.handle_key(&key(KeyCode::Down), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Writing,
            "under the transcript is the box"
        );

        // Escape is the other way back: it gives up on the nearest thing,
        // which is being in the transcript rather than the conversation.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
        let outcome = chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]);
        assert_eq!(outcome, ChatOutcome::Consumed, "escape closed the view");
        assert_eq!(chat.focus(), Focus::Writing);
    }

    /// A run folds itself, and a failure in it opens it again.
    ///
    /// A failure is the one thing in a turn nobody should have to go
    /// looking for. What the reader says about it outlasts both: a run they
    /// closed stays closed, whatever is in it.
    #[test]
    fn a_run_folds_itself_unless_something_in_it_failed() {
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

        // One of them failed, so it is open without anybody asking.
        read(&mut chat, "t3", "failed");
        let rows = chat.rows(ROOM.reading);
        assert!(rows[0].open, "a run with a failure in it stayed folded");
        assert_eq!(rows.len(), 4, "its members are not under it: {rows:?}");

        // And the reader's word beats both ways of deciding.
        chat.fold(0);
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 1, "the reader closed it and it opened itself");
        chat.fold(0);
        assert_eq!(chat.rows(ROOM.reading).len(), 4);
    }

    /// The words a call carries are under it, and a later update replaces
    /// them.
    ///
    /// Which is what the protocol says a later `content` is -- *replace the
    /// content collection* -- and what the diff beside them already did. A
    /// plan put to the reader is replaced by what became of the asking, and
    /// which of the two the row shows is the agent's account of its own
    /// call rather than obelus's to keep both of.
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
    /// is worth, so obelus never closes it for the reader -- and short
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
        assert!(rows[0].open, "obelus closed the thinking by itself");
        assert!(rows.len() > 1, "the thinking is not under its heading");

        chat.fold(0);
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
        chat.fold(0);

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
        chat.doing(Some("thinking\u{2026}"));
        let rows = chat.rows(ROOM.reading);
        assert_eq!(rows.len(), 1, "the states piled up: {rows:?}");
        assert_eq!(rows[0].text(), "thinking\u{2026}");
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
        assert_eq!(chat.focus(), Focus::Transcript(stops[1]));
        chat.handle_key(&key(KeyCode::PageUp), false, ROOM, &[]);
        assert_eq!(
            chat.focus(),
            Focus::Transcript(stops[0]),
            "the page left the cursor behind"
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
        chat.asked("what is this");
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

        // Up, and it stays where it is put -- including when something new
        // arrives, which is the whole point.
        chat.handle_key(&key(KeyCode::Up), false, ROOM, &[]);
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
            ChatOutcome::Send("hell".to_string())
        );
        // Sent, so the row is empty: a prompt still sitting there after
        // being sent is a prompt that gets sent twice.
        assert_eq!(chat.writing().text(), "");
        // And an empty row sends nothing.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, ROOM, &[]),
            ChatOutcome::Consumed
        );
    }

    /// A key with a modifier obelus has no meaning for is not swallowed:
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
