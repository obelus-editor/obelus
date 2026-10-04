//! The top of the direct message: three words, and no agent.
//!
//! **What starts work from a phone is a list and a number.** `notes` lists
//! what this project has still to do, numbered; a number sent back opens
//! that note's conversation -- or takes up the one it had -- and its thread
//! appears under it. `new` opens a conversation about nothing in
//! particular, whose thread is where the reader says the first thing.
//! `note` and a few words writes those words down.
//!
//! An agent here was weighed and put aside: every word to it is a turn,
//! seconds and tokens for "what is there to do", and a list read wrongly is
//! a conversation opened about the wrong note. Three words answer at once
//! and cannot be misread. What else a reader might ask of the top --
//! stopping a turn, closing a conversation -- is asked of the conversation
//! itself: its agent closes it when asked, and the reader's own key stops it.
//!
//! Anything else is answered with what the three words are. Not started as
//! a conversation: a slip of the thumb is not a conversation, and one that
//! appeared for it would be a row the reader has to close.

use obelus_remote::model::{Out, Where};

use super::*;
use crate::conversation::{Conversation, Topic};

/// What the top answers to anything it does not know.
pub(super) const HELP: &str = "Send **notes** to see what there is to do, **new** to start a conversation, or **note** and a few words to write one down. A topic started in the Obelus group is a conversation too.";

impl App {
    /// Somebody on the list said something at the top.
    pub(super) fn heard_at_top(&mut self, from: &str, text: &str) {
        let said = text.trim();
        let (word, rest) = said
            .split_once(char::is_whitespace)
            .map_or((said, ""), |(word, rest)| (word, rest.trim()));
        let answer = match word.to_lowercase().as_str() {
            "notes" if rest.is_empty() => self.offer_the_notes(),
            "new" if rest.is_empty() => self.begin_from_afar(),
            "note" if !rest.is_empty() => self.note_from_afar(rest),
            _ => match said.trim_end_matches('.').parse::<usize>() {
                Ok(number) => self.talk_from_afar(number),
                Err(_) => HELP.to_string(),
            },
        };
        self.say_to(Out::Say {
            to: from.to_string(),
            at: Where::Top,
            text: answer,
            notify: false,
        });
    }

    /// The notes still to do, numbered from one.
    ///
    /// Only those at the top: what hangs under a note is part of it, and a
    /// conversation about the note is about all of that.
    fn offer_the_notes(&mut self) -> String {
        let project = self
            .working_directory
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(todo) = obelus_git::todo::read(&self.working_directory).notes() else {
            return "The notes will not read.".to_string();
        };
        let open: Vec<Topic> = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .map(|talk| talk.topic.clone())
            .collect();
        let left: Vec<&obelus_git::todo::Note> = todo
            .notes
            .iter()
            .filter(|note| note.depth == 0 && !note.done)
            .collect();
        self.mirror.offered = left.iter().map(|note| note.id.clone()).collect();
        if left.is_empty() {
            return format!("Nothing left to do in {project}.");
        }
        let mut said = format!("**{project}**");
        for (at, note) in left.iter().enumerate() {
            let line = note.said.lines().next().unwrap_or_default();
            let talking = match open.contains(&Topic::Note(note.id.clone())) {
                true => " _(open)_",
                false => "",
            };
            said.push_str(&format!("\n{}. {line}{talking}", at + 1));
        }
        said.push_str("\nReply with a number to talk about one.");
        said
    }

    /// The conversation about the note that had this number, opened -- or
    /// taken up again -- without the screen here being taken from what it
    /// has.
    fn talk_from_afar(&mut self, number: usize) -> String {
        let Some(note) = number
            .checked_sub(1)
            .and_then(|at| self.mirror.offered.get(at))
            .cloned()
        else {
            return format!("There is no {number}. Send **notes** for the list.");
        };
        let open = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talk.topic == Topic::Note(note.clone()));
        if open {
            return "That one is open already; its thread is where to talk.".to_string();
        }
        match self.open_about(&note) {
            Some(_) => "Opening it; its thread is next.".to_string(),
            None => "Another Obelus has that one open.".to_string(),
        }
    }

    /// A conversation about nothing in particular, its session asked for
    /// now: one is named by its session, and its thread waits on the name.
    fn begin_from_afar(&mut self) -> String {
        self.a_conversation_from_afar();
        "Starting one; its thread is next.".to_string()
    }

    /// A conversation begun from the chat, and its session asked for.
    pub(super) fn a_conversation_from_afar(&mut self) -> DocumentId {
        self.documents.push(Some(
            Conversation {
                from_afar: true,
                ..Conversation::default()
            }
            .into(),
        ));
        let id = DocumentId::new(self.documents.len() - 1);
        if self
            .talker
            .as_ref()
            .is_none_or(obelus_agent::acp::Talk::has_exited)
        {
            self.stop_agent();
            self.start_agent();
        }
        self.ask_for_a_session(talking::Whose::One(id), None);
        id
    }

    /// Writes words down as a note of their own.
    fn note_from_afar(&mut self, said: &str) -> String {
        let done = self.change_the_notes(obelus_git::todo::Doing::Add {
            notes: vec![(said.to_string(), 0)],
            under: None,
        });
        tracing::info!(done, "a note written down from a chat");
        "Noted.".to_string()
    }
}
