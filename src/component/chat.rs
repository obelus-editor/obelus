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
    /// Close the view, keeping what is in it.
    Cancelled,
}

/// A conversation.
#[derive(Debug, Default)]
pub struct Chat {
    /// What has been said, oldest first.
    said: Vec<Said>,
    /// What is being typed.
    input: String,
    /// The first row of the transcript on screen.
    top: usize,
    /// Whether the view is following the end of the transcript.
    ///
    /// On until the reader scrolls up, and on again when they come back
    /// down. Without it a streaming answer either drags the view around
    /// while it is being read, or arrives off screen with nothing to say so.
    following: bool,
}

impl Chat {
    /// An empty conversation, following its own end.
    #[must_use]
    pub fn new() -> Self {
        Self {
            said: Vec::new(),
            input: String::new(),
            top: 0,
            following: true,
        }
    }

    /// Whether anything has been said.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.said.is_empty()
    }

    /// What is being typed.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The first transcript row on screen.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
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
        let room = usize::from(room).max(1);
        let last = rows.saturating_sub(room);
        if self.following {
            self.top = last;
        } else {
            self.top = self.top.min(last);
            // Scrolled back down to the end, so it follows again. Worked
            // out here rather than by the key, because where the end is
            // depends on the width and the room and a key knows neither.
            self.following = self.top >= last;
        }
    }

    /// Handles a key.
    ///
    /// `thinking` decides what escape means: while the agent is working it
    /// stops the agent, and otherwise it closes the view. One key, and the
    /// thing it does is always "stop what is happening" -- which is what
    /// escape means everywhere else in obelus.
    pub fn handle_key(&mut self, key: &KeyEvent, thinking: bool, room: u16) -> ChatOutcome {
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
            return ChatOutcome::Ignored;
        };
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return ChatOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let page = usize::from(room).max(1);

        match key.code {
            KeyCode::Esc if bare && thinking => ChatOutcome::Interrupt,
            KeyCode::Esc if bare => ChatOutcome::Cancelled,
            KeyCode::Enter if bare => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return ChatOutcome::Consumed;
                }
                self.input.clear();
                ChatOutcome::Send(text)
            }
            KeyCode::Backspace if bare => {
                self.input.pop();
                ChatOutcome::Consumed
            }
            // The transcript scrolls, because there is no cursor in it to
            // move: what a reader wants of an answer they have read past is
            // the answer, not a place in it.
            KeyCode::Up if bare => {
                self.scroll_by(-1);
                ChatOutcome::Consumed
            }
            KeyCode::Down if bare => {
                self.scroll_by(1);
                ChatOutcome::Consumed
            }
            KeyCode::PageUp => {
                self.scroll_by(-isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::PageDown => {
                self.scroll_by(isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::Home => {
                self.following = false;
                self.top = 0;
                ChatOutcome::Consumed
            }
            KeyCode::End => {
                self.following = true;
                ChatOutcome::Consumed
            }
            KeyCode::Char(character) => {
                self.input.push(character);
                ChatOutcome::Consumed
            }
            _ => ChatOutcome::Ignored,
        }
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
    ///
    /// Up leaves the end, which is the whole of what "following" means.
    /// Down does not say it has arrived: where the end is depends on the
    /// width and the room, so `settle` decides that on the next frame.
    fn scroll_by(&mut self, rows: isize) {
        if rows < 0 {
            self.top = self.top.saturating_sub(usize::try_from(-rows).unwrap_or(1));
            self.following = false;
        } else {
            self.top = self.top.saturating_add(usize::try_from(rows).unwrap_or(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
        chat.handle_key(&key(KeyCode::Up), false, 10);
        chat.settle(rows, 10);
        assert_eq!(chat.top(), 28);
        chat.note("something new");
        let rows = chat.rows(40).len();
        chat.settle(rows, 10);
        assert_eq!(chat.top(), 28, "it dragged the view to the end");

        // Back down to the end, and it follows again.
        for _ in 0..5 {
            chat.handle_key(&key(KeyCode::Down), false, 10);
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
            chat.handle_key(&key(KeyCode::Esc), true, 10),
            ChatOutcome::Interrupt
        );
        assert_eq!(
            chat.handle_key(&key(KeyCode::Esc), false, 10),
            ChatOutcome::Cancelled
        );
    }

    /// Typing goes into the row being typed, and enter sends it.
    #[test]
    fn what_is_typed_is_sent_once() {
        let mut chat = Chat::new();
        for character in "hello".chars() {
            chat.handle_key(&key(KeyCode::Char(character)), false, 10);
        }
        chat.handle_key(&key(KeyCode::Backspace), false, 10);
        assert_eq!(chat.input(), "hell");

        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, 10),
            ChatOutcome::Send("hell".to_string())
        );
        // Sent, so the row is empty: a prompt still sitting there after
        // being sent is a prompt that gets sent twice.
        assert_eq!(chat.input(), "");
        // And an empty row sends nothing.
        assert_eq!(
            chat.handle_key(&key(KeyCode::Enter), false, 10),
            ChatOutcome::Consumed
        );
    }

    /// A key with a modifier obelus has no meaning for is not swallowed:
    /// `ctrl+q` still quits with the conversation open.
    #[test]
    fn a_chord_falls_through_to_the_key_table() {
        let mut chat = Chat::new();
        let quit = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(chat.handle_key(&quit, false, 10), ChatOutcome::Ignored);
        assert_eq!(chat.input(), "", "it typed the chord into the row");
    }
}
