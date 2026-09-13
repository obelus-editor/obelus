//! The conversation with an agent: what was said, and what is being typed.
//!
//! Not a buffer. A buffer is a file with a cursor in it and a history behind
//! it; this is a transcript, which grows at the end and is read from the
//! bottom. Keeping it out of the buffer list is also what makes it survive
//! being closed -- the view is hidden, the conversation is still here, and
//! reopening shows what was there.
//!
//! It holds no client. What it has is what arrived, and every key it does
//! not handle itself becomes an outcome for the application to act on --
//! which is what keeps "send this prompt" out of a component that cannot
//! know whether there is an agent to send it to.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::component::{composer::Composer, window::Window};

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
    /// What is happening now.
    ///
    /// Not a thing that was said: the state of the one saying things, at
    /// the foot of the transcript where the next thing will appear. It is
    /// there while it is true and gone when it is not.
    Doing,
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
    pub places: Vec<crate::acp::Place>,
    /// The change it is making, as the rows that draw it.
    ///
    /// Empty for everything that is not a change. What is kept is the rows
    /// rather than the two texts: the diff is worked out once, when it
    /// arrives.
    pub change: Vec<crate::git::change::Line>,
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
    /// The words.
    pub text: String,
    /// Whether it is the first row of what they said, which is the row that
    /// gets the mark saying who is speaking.
    pub first: bool,
    /// A tool call's state, on its first row.
    pub state: Option<String>,
    /// What sort of thing a tool call is doing, on its first row.
    pub kind: String,
    /// The first place a tool call named, on its first row, and how many
    /// more it named.
    pub place: Option<(crate::acp::Place, usize)>,
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
    pub marker: Option<crate::git::change::Marker>,
    /// How much a change adds and takes away, on the row that heads it.
    pub changed: Option<(usize, usize)>,
    /// How deep the row sits: the members of an opened run are drawn under
    /// their own heading, so that a run reads as one thing.
    pub depth: u8,
}

/// Where a call says it was, which for a change is the file it changes.
///
/// An agent editing a file often names no location: what it is doing is the
/// diff, and the diff says which file. Taking the path from there is what
/// makes an edit somewhere the reader can go.
fn places_of(call: &crate::acp::Call) -> Vec<crate::acp::Place> {
    if !call.places.is_empty() {
        return call.places.clone();
    }
    call.change
        .iter()
        .map(|change| crate::acp::Place {
            path: change.path.clone(),
            line: None,
        })
        .collect()
}

/// The change a call is making, as the rows that draw it.
///
/// Worked out once, when it arrives, rather than every time the transcript
/// is drawn: a diff is real work, and a frame is not the place for it.
fn changed_rows(call: &crate::acp::Call) -> Vec<crate::git::change::Line> {
    call.change.as_ref().map_or_else(Vec::new, |change| {
        crate::git::change::drawn(change.before.as_deref().unwrap_or_default(), &change.after)
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
    /// a file, and pressing enter on it opens that file. Prose is stepped
    /// over rather than landed on -- the same rule a list follows for a row
    /// that cannot be chosen -- so a reader walking the transcript never
    /// reaches a row where enter does nothing.
    #[must_use]
    pub const fn acts(&self) -> bool {
        self.place.is_some() || self.folds.is_some()
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
    GoTo(crate::acp::Place),
    /// Open the values of one of the agent's settings, by its id.
    Choose(String),
    /// Flip one of its switches, by its id.
    Toggle(String),
    /// Close the view, keeping what is in it.
    Cancelled,
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

/// A conversation.
#[derive(Debug, Default)]
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
}

impl Chat {
    /// An empty conversation, following its own end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            said: Vec::new(),
            doing: None,
            input: Composer::new(),
            window: Window::following(),
            focus: Focus::Writing,
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

    /// What is being written, for the view to draw.
    #[must_use]
    pub const fn writing(&self) -> &Composer {
        &self.input
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

    /// Adds one of obelus's own remarks.
    pub fn note(&mut self, text: &str) {
        self.push(Speaker::Note, text, None);
    }

    /// Says what is happening now, or that nothing is.
    ///
    /// Called with whatever the state is, every frame: it is read from the
    /// state rather than remembered, so there is no way for it to be left
    /// behind.
    pub fn doing(&mut self, what: Option<&str>) {
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

    /// Takes news of a tool call: a new one, or the same one further along.
    pub fn tool(&mut self, call: &crate::acp::Call, status: &str) {
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
            rows.push(Row {
                speaker: Speaker::Doing,
                text: doing.to_string(),
                first: true,
                state: None,
                kind: String::new(),
                place: None,
                folds: None,
                open: false,
                marker: None,
                changed: None,
                depth: 0,
            });
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
            true => "files",
            false => "calls",
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
            text: format!("{count} {what}"),
            first: true,
            state: state.map(str::to_string),
            kind: said.kind.clone(),
            place: None,
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

        // A change is not prose. It is drawn as the lines it is, under the
        // call's own row -- which is what folds them, because a diff is the
        // one thing an agent sends that is longer than the screen.
        if !said.change.is_empty() {
            let mut rows = vec![Row {
                changed: Some(crate::git::change::counted(&said.change)),
                ..self.opening(said, said.text.clone(), depth, Some(at))
            }];
            if self.is_open(at) {
                rows.extend(said.change.iter().flat_map(|line| {
                    crate::text::wrapped(&line.text, inside)
                        .into_iter()
                        .map(|text| Row {
                            marker: line.marker,
                            ..Self::under(said, text, depth + 1)
                        })
                }));
            }
            return rows;
        }

        let words = crate::text::wrapped(&said.text, room);
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
                .map(|(row, text)| match row {
                    0 => self.opening(said, text, depth, None),
                    _ => Self::under(said, text, depth),
                })
                .collect();
        }
        let mut rows = vec![self.opening(said, "thought".to_string(), depth, Some(at))];
        if self.is_open(at) {
            rows.extend(
                crate::text::wrapped(&said.text, inside)
                    .into_iter()
                    .map(|text| Self::under(said, text, depth + 1)),
            );
        }
        rows
    }

    /// The row a thing said begins with.
    ///
    /// It carries the glyph and everything that is true of the whole of it:
    /// where it has got to, the file it names, and whether there is more
    /// behind it than it is showing.
    fn opening(&self, said: &Said, text: String, depth: u8, folds: Option<usize>) -> Row {
        Row {
            speaker: said.speaker,
            text,
            first: true,
            state: said.state.clone(),
            kind: said.kind.clone(),
            place: said
                .places
                .first()
                .map(|place| (place.clone(), said.places.len() - 1)),
            folds,
            open: folds.is_some_and(|at| self.is_open(at)),
            marker: None,
            changed: None,
            depth,
        }
    }

    /// A row that continues something already begun.
    fn under(said: &Said, text: String, depth: u8) -> Row {
        Row {
            speaker: said.speaker,
            text,
            first: false,
            state: None,
            kind: said.kind.clone(),
            place: None,
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
            text: String::new(),
            first: false,
            state: None,
            kind: String::new(),
            place: None,
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
        if !said.change.is_empty() {
            return matches!(said.state.as_deref(), Some("pending" | "in_progress"));
        }
        self.said[self.run_from(at)]
            .iter()
            .any(|said| said.state.as_deref() == Some("failed"))
    }

    /// Opens what is closed and closes what is open.
    pub fn fold(&mut self, at: usize) {
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
    /// stops the agent, and otherwise it closes the view. One key, and the
    /// thing it does is always "stop what is happening" -- which is what
    /// escape means everywhere else in obelus.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        thinking: bool,
        room: Room,
        settings: &[crate::acp::Setting],
    ) -> ChatOutcome {
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
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
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
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
            KeyCode::Esc if bare && thinking => ChatOutcome::Interrupt,
            KeyCode::Esc if bare => ChatOutcome::Cancelled,
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
        settings: &[crate::acp::Setting],
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
                    crate::acp::Kind::Select => ChatOutcome::Choose(setting.id.clone()),
                    crate::acp::Kind::Switch => ChatOutcome::Toggle(setting.id.clone()),
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
                    Some(row) => match (row.folds, row.place) {
                        (Some(begins), _) => {
                            self.fold(begins);
                            // The heading stays under the reader: what
                            // moved is what is below it.
                            self.show_row(at, room);
                            Some(ChatOutcome::Consumed)
                        }
                        (None, Some((place, _))) => Some(ChatOutcome::GoTo(place)),
                        (None, None) => Some(ChatOutcome::Consumed),
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
        let place = crate::acp::Place {
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
        assert_eq!(row.text, "Read the file", "the title was lost");
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
    fn call(id: &str, title: &str, kind: &str, places: Vec<crate::acp::Place>) -> crate::acp::Call {
        crate::acp::Call {
            id: id.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            places,
            change: None,
        }
    }

    /// Somewhere an agent said it had been.
    fn place(path: &str, line: u32) -> crate::acp::Place {
        crate::acp::Place {
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
        assert_eq!(rows[0].text, "3 files");
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
        assert_eq!(rows[0].text, "thought", "long thinking has no heading");
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
                crate::ui::text_width(&row.text) <= room,
                "{:?} is {} cells wide with {room} to write in",
                row.text,
                crate::ui::text_width(&row.text)
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
        assert_eq!(rows[0].text, "thinking\u{2026}");
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
        let words: Vec<&str> = rows
            .iter()
            .filter(|row| !row.text.is_empty())
            .map(|row| row.text.as_str())
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
        let calls: Vec<(&str, Option<&str>)> = rows
            .iter()
            .filter(|row| row.speaker == Speaker::Tool && row.first)
            .map(|row| (row.text.as_str(), row.state.as_deref()))
            .collect();
        assert_eq!(
            calls,
            [
                ("Read the file", Some("completed")),
                ("Run the tests", Some("pending")),
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
    fn escape_stops_the_agent_first_and_closes_the_view_second() {
        let mut chat = Chat::new();
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), true, ROOM, &[]),
            ChatOutcome::Interrupt
        );
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, ROOM, &[]),
            ChatOutcome::Cancelled
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
