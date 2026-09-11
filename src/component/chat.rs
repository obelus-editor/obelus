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
    pub fn tool(&mut self, id: &str, title: &str, status: &str) {
        let existing = self
            .said
            .iter_mut()
            .rev()
            .find(|said| said.tag.as_deref() == Some(id));
        match existing {
            Some(said) => {
                // A later update carries only what changed, so an empty
                // title means "the one you already have".
                if !title.is_empty() {
                    said.text = title.to_string();
                }
                said.state = Some(status.to_string());
            }
            None => {
                self.said.push(Said {
                    speaker: Speaker::Tool,
                    text: title.to_string(),
                    tag: Some(id.to_string()),
                    state: Some(status.to_string()),
                });
            }
        }
    }

    /// Everything said, as rows wrapped to a width.
    ///
    /// A blank row between one speaker and the next: a transcript with no
    /// space in it reads as one voice.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, said) in self.said.iter().enumerate() {
            if index > 0 {
                rows.push(Row {
                    speaker: said.speaker,
                    text: String::new(),
                    first: false,
                    state: None,
                });
            }
            for (row, words) in crate::text::wrapped(&said.text, width)
                .into_iter()
                .enumerate()
            {
                rows.push(Row {
                    speaker: said.speaker,
                    text: words,
                    first: row == 0,
                    state: (row == 0).then(|| said.state.clone()).flatten(),
                });
            }
        }
        rows
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
                if !self.input.up(room.writing) {
                    self.scroll_by(-1);
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
        });
    }

    /// Scrolls by rows.
    fn scroll_by(&mut self, rows: isize) {
        self.window.scroll(rows);
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// A screen with ten rows of transcript and a box twenty cells wide.
    const ROOM: Room = Room {
        transcript: 10,
        writing: 20,
    };

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
        chat.tool("t1", "Read the file", "pending");
        chat.tool("t2", "Run the tests", "pending");
        // A later update carries only what changed, so an empty title means
        // the one already there.
        chat.tool("t1", "", "completed");

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
