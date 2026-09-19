//! Talking to the active agent.
//!
//! One agent at a time, started when the reader first opens a conversation
//! and left running until they close obelus or choose another. A
//! conversation is a document in the list of what is open, so leaving one is
//! going somewhere else rather than closing it: what was said is still there
//! when the reader comes back to that row.
//!
//! What arrives from the agent is an [`Event::Acp`] like every other
//! background source, so nothing here waits on anything.

use crossterm::event::{KeyCode, KeyModifiers};

use super::*;
use crate::{
    acp,
    component::{
        card::{Card, CardOutcome, Choice},
        chat::{Chat, Speaker},
    },
    conversation::{Asking, Topic},
};

/// What obelus is doing about an agent, for the view to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Talking {
    /// No agent has been chosen.
    Nobody,
    /// One has been chosen and is not running: nothing has needed it yet.
    Idle,
    /// Starting, or opening a session.
    Starting,
    /// There is a session, and it is waiting to be asked something.
    Ready,
    /// It is working on a prompt.
    Thinking,
    /// It was running and has stopped.
    Gone,
}

impl App {
    /// Goes to the conversation, opening one if there is none.
    ///
    /// A document, so this switches to it the way any key that opens a file
    /// does: the list keeps it, closing it is the key that closes anything,
    /// and what was said is still there when the reader comes back. It was a
    /// flag over the editor, which is why escape used to close it and why
    /// there was a second question -- "is it showing" -- beside the
    /// conversation that answers it.
    pub fn open_agent(&mut self) {
        // Whatever the reader had over the file is not what they asked for.
        self.make_room(Room::Region);
        // The one about nothing in particular, which is what this key opens.
        // Any conversation would do while there was one; with a conversation
        // per note it would take the reader into whichever note's happened
        // to be first in the list.
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == Topic::Loose)
        });
        let at = at.unwrap_or_else(|| {
            self.documents
                .push(Some(crate::conversation::Conversation::default().into()));
            self.documents.len() - 1
        });
        self.go_to_document(DocumentId::new(at));
        if self.talker.is_none() {
            self.start_agent();
        }
    }

    /// Goes to the conversation about one note, opening one if there is
    /// none.
    ///
    /// By the note's name rather than its place in the list, which is what
    /// the name is for: the list is read from the file every time it opens,
    /// and a note added above would otherwise hand the reader somebody
    /// else's conversation.
    pub(super) fn talk_about(&mut self, note: &crate::todo::NoteId) {
        self.make_room(Room::Region);
        let wanted = Topic::Note(note.clone());
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == wanted)
        });
        let at = at.unwrap_or_else(|| {
            let talk = crate::conversation::Conversation {
                opening: self.about_the_note(note),
                topic: wanted,
                ..crate::conversation::Conversation::default()
            };
            self.documents.push(Some(talk.into()));
            self.documents.len() - 1
        });
        self.go_to_document(DocumentId::new(at));
        if self.talker.is_none() {
            self.start_agent();
            return;
        }
        if self
            .conversation()
            .is_some_and(|talk| talk.session.is_some())
        {
            return;
        }
        // The process is up and this conversation has no session of its own
        // yet: one agent, several conversations, because an agent holds a
        // project's worth of context and a second process would pay for all
        // of it twice.
        //
        // Or one it had before, if obelus wrote the name down: the agent
        // kept every word of it, which is why obelus keeps none.
        let had = self.remembered_session(note);
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        match had {
            Some(session) => {
                talker.reopen(&session);
                if let Some(talk) = self.conversation_mut() {
                    talk.asked_for = Some(crate::acp::SessionId::new(session));
                }
            }
            None => talker.open(),
        }
    }

    /// What obelus tells an agent a conversation about a note is about.
    ///
    /// The whole of what the note says rather than its first line: the first
    /// line is what the reader called it and the rest is what they meant,
    /// and this is the one time the agent is told any of it. Its name goes
    /// with it, because `todo_finish` takes a name and an agent that had to
    /// guess which of the notes it was looking at would rather not call it
    /// at all.
    ///
    /// Read from the file, like everything else that asks what a note says:
    /// the reader may have rewritten it in their own editor since.
    fn about_the_note(&self, note: &crate::todo::NoteId) -> Option<String> {
        let about = crate::todo::Todo::read(&self.working_directory)
            .notes
            .into_iter()
            .find(|other| other.id == *note)?;
        let at = about.at.as_ref().map_or_else(String::new, |at| {
            format!("\nIt is about {}:{}.", at.path.display(), at.line.get() + 1)
        });
        Some(format!(
            "This conversation is about one of obelus's notes, which says:\n\n\
             {}\n\n\
             Its name is {}.{at}\n\n\
             When its work is done, tick it off with `todo_finish` and that \
             name. Work this turns up that belongs to it goes under it: \
             `todo_add` with `under` set to that name. Both write the \
             reader's file and neither asks for you, so ask them first.",
            about.said, about.id,
        ))
    }

    /// The same, for a conversation by where it is in the list.
    fn opening_for(&self, at: usize) -> Option<String> {
        let Topic::Note(note) = &self.documents.get(at)?.as_ref()?.chat()?.topic else {
            return None;
        };
        self.about_the_note(&note.clone())
    }

    /// The conversation obelus had about this note with the agent that is
    /// running, if it wrote one down.
    fn remembered_session(&self, note: &crate::todo::NoteId) -> Option<String> {
        let agent = self.talker.as_ref()?.id();
        let kept = crate::acp::sessions::read(&self.working_directory);
        Some(kept.get(note, agent)?.session.clone())
    }

    /// Writes down which conversation is about which note.
    ///
    /// Every time one is named or renamed, because the moment obelus does
    /// not survive is the one nobody plans for: a crash between opening a
    /// conversation and remembering it is a conversation the agent keeps
    /// and nobody can reach.
    pub(super) fn remember_the_conversations(&self) {
        let Some(agent) = self.talker.as_ref().map(|talker| talker.id().to_string()) else {
            return;
        };
        let talker = self.talker.as_ref();
        let mine: Vec<(crate::todo::NoteId, crate::acp::sessions::Kept)> = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .filter_map(|talk| {
                let Topic::Note(note) = &talk.topic else {
                    return None;
                };
                let session = talk.session.as_ref()?;
                Some((
                    note.clone(),
                    crate::acp::sessions::Kept {
                        session: session.0.to_string(),
                        title: talker
                            .and_then(|talker| talker.title(Some(session)))
                            .map(str::to_string),
                    },
                ))
            })
            .collect();
        if mine.is_empty() {
            return;
        }
        // The notes as the file has them, so that anything about a note
        // somebody has taken away goes at the same time. A note can go
        // without obelus watching, so the collecting is done on the way past
        // rather than when one is deleted.
        let notes: Vec<crate::todo::NoteId> = crate::todo::Todo::read(&self.working_directory)
            .notes
            .into_iter()
            .map(|note| note.id)
            .collect();
        crate::acp::sessions::change(&self.working_directory, &notes, |kept| {
            for (note, what) in mine {
                kept.put(&note, &agent, what);
            }
        });
    }

    /// The note the conversation being read is about, in the words the
    /// reader wrote.
    ///
    /// Read from the file rather than kept, like everything else about the
    /// notes: the reader can change what one says from the notes page, from
    /// their own editor, or from a second obelus, and a header holding a
    /// copy would go on saying what the note used to.
    #[must_use]
    pub fn what_this_conversation_is_about(&self) -> Option<String> {
        let Topic::Note(id) = &self.conversation()?.topic else {
            return None;
        };
        crate::todo::Todo::read(&self.working_directory)
            .notes
            .into_iter()
            .find(|note| note.id == *id)
            .map(|note| note.title().to_string())
    }

    /// Puts pasted text into the box a message is written in.
    ///
    /// The whole of what a paste means here: a conversation has one place
    /// text can go, and it is the box. What was said is what was said.
    ///
    /// Through [`App::conversation_takes_text`], which is half of what
    /// `ctrl+v` is offered on, so the key and the terminal's own paste land
    /// in the same place or in no place -- and a paste with nowhere to go is
    /// dropped rather than put behind whatever is over the box.
    pub(super) fn paste_into_conversation(&mut self, what: &str) {
        if !self.conversation_takes_text() {
            return;
        }
        // Each against the width its own rows are drawn at, which is what
        // the wrapping is worked out from: a card's rows are inset inside
        // the band the box would have had.
        let area = self.editor_area;
        let card_width = self
            .conversation()
            .and_then(|talk| talk.card.as_ref())
            .map(|card| crate::ui::card::width_of(crate::ui::chat::bands_for(area, card).writing));
        let width = crate::ui::chat::writing_width(area);
        let Some(talk) = self.conversation_mut() else {
            return;
        };
        // Into the card while one is up: it is what covers the box, so the
        // half of it that takes words is the only place on screen the reader
        // could be writing.
        match (talk.card.as_mut(), card_width) {
            (Some(card), Some(room)) => {
                card.paste(what, room);
            }
            _ => talk.chat.paste(what, width),
        }
    }

    /// The conversation, while it is what the reader is looking at.
    #[must_use]
    pub fn chat(&self) -> Option<&Chat> {
        Some(&self.conversation()?.chat)
    }

    /// What obelus is doing about an agent.
    #[must_use]
    pub fn talking(&self) -> Talking {
        let held = self.session_now();
        let session = held.as_ref();
        let Some(talker) = self.talker.as_ref() else {
            return match self.settled.config.agent.as_deref() {
                None | Some("") => Talking::Nobody,
                Some(_) => Talking::Idle,
            };
        };
        if talker.has_exited() {
            Talking::Gone
        } else if talker.is_thinking(session) {
            Talking::Thinking
        } else if talker.is_started(session) {
            Talking::Ready
        } else {
            Talking::Starting
        }
    }

    /// What to call the agent on screen.
    ///
    /// What it calls itself once it has said, and what the registry called
    /// it until then: a name that appears only after the handshake is a
    /// header that changes under the reader.
    #[must_use]
    pub fn agent_name(&self) -> Option<&str> {
        let chosen = match self.settled.config.agent.as_deref() {
            None | Some("") => None,
            Some(id) => Some(id),
        };
        self.talker.as_ref().and_then(acp::Talk::info).or(chosen)
    }

    /// The way of working the agent is in, if it offers one.
    ///
    /// A setting like the others -- the one the agent said is the mode --
    /// named apart because one key steps it.
    #[must_use]
    pub fn agent_mode(&self) -> Option<&acp::Setting> {
        self.talker.as_ref()?.mode(self.session_now().as_ref())
    }

    /// The commands it says it takes.
    #[must_use]
    pub fn agent_orders(&self) -> &[acp::Order] {
        self.talker
            .as_ref()
            .map_or(&[], |talker| talker.orders(self.session_now().as_ref()))
    }

    /// Moves to the agent's next way of working.
    pub(super) fn step_agent_mode(&mut self) {
        let session = self.session_now();
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        talker.step_mode(session.as_ref());
    }

    /// How full the agent's memory of this conversation is, once it has
    /// said -- and what it has cost, where it counts that too.
    #[must_use]
    pub fn agent_usage(&self) -> Option<&acp::Usage> {
        self.talker.as_ref()?.usage(self.session_now().as_ref())
    }

    /// The settings it lets the reader change.
    #[must_use]
    pub fn agent_settings(&self) -> &[acp::Setting] {
        self.talker
            .as_ref()
            .map_or(&[], |talker| talker.settings(self.session_now().as_ref()))
    }

    /// One setting's values, as the ordinary compact list.
    ///
    /// What enter on the conversation's own row opens, for a setting whose
    /// values are a list. A switch never comes here: it has two sides and
    /// is flipped where it stands.
    pub(super) fn open_agent_setting(&mut self, id: &str) {
        let session = self.session_now();
        let Some(setting) = self
            .talker
            .as_ref()
            .and_then(|talker| talker.setting(session.as_ref(), id))
        else {
            return;
        };
        let question = setting.name.clone();
        let current = setting.current_name().map(str::to_string);
        let items = setting
            .values
            .iter()
            .map(|value| PickerItem {
                prose: false,
                marker: None,
                icon: None,
                label: value.name.clone(),
                detail: value.about.clone(),
                // The one that is on says so in words. A list where the
                // selected row and the current value look the same cannot
                // say which of the two it is showing.
                trailing: (value.id == setting.current).then(|| "current".to_string()),
                changed: None,
                value: PickerValue::AgentValue {
                    setting: id.to_string(),
                    value: value.id.clone(),
                },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask(&question);
        picker.when_empty("this one has nothing to choose from");
        // Opened on what it is already on, so the list starts by saying
        // where the reader is rather than at whatever happens to be first.
        if let Some(current) = current {
            picker.prefer(current);
        }
        self.show_list(picker);
    }

    /// Flips one of the agent's switches to its other side.
    ///
    /// Which is the whole of what a switch can be asked: the list of two
    /// values obelus makes for it is what the settings page needs, and on
    /// the conversation's own row a list of two is a list nobody wants.
    pub(super) fn flip_agent_setting(&mut self, id: &str) {
        let session = self.session_now();
        let Some(other) = self
            .talker
            .as_ref()
            .and_then(|talker| talker.setting(session.as_ref(), id))
            .map(|setting| match setting.current == "on" {
                true => "off",
                false => "on",
            })
        else {
            return;
        };
        self.set_agent_setting(id, other);
    }

    /// Asks for one of them to be put on one of its values.
    pub(super) fn set_agent_setting(&mut self, setting: &str, value: &str) {
        let session = self.session_now();
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        let Some(known) = talker.setting(session.as_ref(), setting) else {
            return;
        };
        let (name, told) = (known.name.clone(), what_to_say(known, value));
        let chosen = acp::Chosen::of(known, value);
        talker.set(session.as_ref(), setting, chosen);
        // In the transcript, because it is a thing the reader did to the
        // conversation: what the agent answers with is the whole set of
        // settings again, which is not something to show.
        if let Some(talk) = self.conversation_mut() {
            talk.chat.note(&format!("{name}: {told}"));
        }
    }

    /// Sends what the reader typed.
    pub(super) fn send_to_agent(&mut self, text: &str) {
        // What obelus has to say about this conversation, if it has not said
        // it yet. Taken, not read: gone is what "already said" looks like,
        // and the transcript carries a line saying obelus said it -- what
        // obelus sends in the reader's name is the reader's to see.
        let opening = self.conversation_mut().and_then(|talk| talk.opening.take());
        if let Some(talk) = self.conversation_mut() {
            // A new turn starts with no plan: an agent that made one last
            // turn and makes none this turn would otherwise have the old
            // one shown against the new work.
            talk.chat.plan_forgotten();
            if opening.is_some() {
                talk.chat
                    .note("told the agent what this conversation is about");
            }
            talk.chat.asked(text);
        }
        // An agent that has stopped is started again by talking to it,
        // which is what the view tells the reader to do. The handle of the
        // one that ended is dropped first: it is still a handle, so a
        // check for "is there one" would find it and say the message to a
        // channel nobody is reading.
        if self
            .talker
            .as_ref()
            .is_none_or(crate::acp::Talk::has_exited)
        {
            self.stop_agent();
            self.start_agent();
        }
        let session = self.session_now();
        let Some(talker) = self.talker.as_mut() else {
            // `start_agent` has already said why in the transcript.
            return;
        };
        // Held until the session opens, which is the ordinary case for the
        // first thing said: the reader typed while it was starting, and the
        // handle sends it when there is somewhere to send it -- opening and
        // all, because the opening belongs to whatever goes first.
        talker.say(session.as_ref(), text, opening.as_deref());
    }

    /// Asks the agent to stop what it is doing.
    pub(super) fn interrupt_agent(&mut self) {
        let session = self.session_now();
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        talker.interrupt(session.as_ref());
    }

    /// Stops the agent, if one is running.
    pub(super) fn stop_agent(&mut self) {
        if let Some(mut talker) = self.talker.take() {
            talker.shutdown();
        }
        self.forget_the_question();
    }

    /// Drops whatever the agent was waiting on an answer to.
    ///
    /// Dropped rather than answered: there is no longer anything to answer,
    /// and dropping the channel is what tells the other side so. The card
    /// goes with them, because a question on screen that nobody is waiting
    /// for is a question the reader would answer into nothing.
    fn forget_the_question(&mut self) {
        if let Some(talk) = self.conversation_mut() {
            talk.permission = None;
        }
        if let Some(talk) = self.conversation_mut() {
            talk.asking = None;
        }
        if let Some(talk) = self.conversation_mut() {
            talk.card = None;
        }
    }

    /// The agent's own commands, while one is being typed.
    ///
    /// A list of them is the ordinary compact list -- the same rows, the
    /// same chosen row, the same marking of what matched -- rather than
    /// something drawn for this one screen. What it does not have is the
    /// keys: the box below it owns those, because that is where the reader
    /// is typing, and this list follows what they type.
    #[must_use]
    pub fn slash(&self) -> Option<&Picker> {
        self.conversation().and_then(|talk| talk.slash.as_ref())
    }

    /// Builds or refreshes that list, once a frame.
    ///
    /// It exists exactly while a command's *name* is being typed: a slash
    /// opens it, a blank after the name settles it and closes it, and
    /// rubbing the slash out closes it too.
    pub(super) fn refresh_slash(&mut self) {
        let name = self
            .chat()
            .filter(|_| !self.agent_orders().is_empty())
            .and_then(Chat::typing_command);
        let Some(name) = name else {
            if let Some(talk) = self.conversation_mut() {
                talk.slash = None;
            }
            return;
        };
        if let Some(slash) = self.conversation_mut().and_then(|talk| talk.slash.as_mut()) {
            if slash.query() != name {
                slash.set_query(&name);
            }
            // A list of nothing is not a list. It also has to stop being
            // one: a name that matches no command is an ordinary message
            // as far as the box is concerned, and a list that stayed would
            // swallow the enter that sends it.
            if slash.match_count() == 0
                && let Some(talk) = self.conversation_mut()
            {
                talk.slash = None;
            }
            self.settle_slash();
            return;
        }
        let items = self
            .agent_orders()
            .iter()
            .map(|order| PickerItem {
                prose: false,
                marker: None,
                // No glyph: a column of the same one down a list says
                // nothing, and the slash in front of the name is what says
                // what these rows are.
                icon: None,
                label: format!("/{}", order.name),
                detail: Some(order.description.clone()),
                trailing: order.hint.clone(),
                changed: None,
                value: PickerValue::Nothing,
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut slash = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        slash.set_query(&name);
        if slash.match_count() > 0
            && let Some(talk) = self.conversation_mut()
        {
            talk.slash = Some(slash);
        }
        self.settle_slash();
    }

    /// Gives that list the geometry it is about to be drawn in.
    ///
    /// The same thing [`App::prepare`] does for the other list, and for the
    /// same two reasons: the window follows the selection only when it
    /// knows how many rows are on screen, and the matched characters are
    /// worked out for the rows that will be drawn. Without it the list
    /// neither scrolls nor says what the query matched -- it is the picker,
    /// so it needs what the picker needs.
    fn settle_slash(&mut self) {
        let rows = self
            .conversation()
            .and_then(|talk| talk.slash.as_ref())
            .zip(self.chat())
            .map(|(slash, chat)| {
                ui::picker::rows_drawn(slash, ui::chat::above_writing(self.editor_area, chat))
            });
        if let (Some(rows), Some(slash)) = (
            rows,
            self.conversation_mut().and_then(|talk| talk.slash.as_mut()),
        ) {
            slash.refresh_indices(rows);
        }
    }

    /// Whatever a key means to that list, if it means anything.
    ///
    /// Only the keys that move about a list and the ones that choose from
    /// it. Everything else -- every character, every line break -- belongs
    /// to the box, which is what makes the list a list of what is being
    /// typed rather than a mode the reader is in.
    pub(super) fn slash_key(&mut self, key: &KeyEvent) -> bool {
        let Some(slash) = self.conversation_mut().and_then(|talk| talk.slash.as_mut()) else {
            return false;
        };
        let Some(modifiers) = keymap::modifiers_of(key) else {
            return false;
        };
        if modifiers != KeyModifiers::NONE {
            return false;
        }
        match key.code {
            // The window is not moved here: the frame that follows settles
            // it, which is the one place that knows how many rows are on
            // screen.
            KeyCode::Up => {
                slash.move_selection_by(-1);
                true
            }
            KeyCode::Down => {
                slash.move_selection_by(1);
                true
            }
            // Enter chooses from the list, like enter chooses in every other
            // list -- and like every other completion in obelus, which is
            // one rule rather than a key per panel. What sends the message
            // is enter *after* the name is settled, by which time there is
            // no list.
            KeyCode::Enter => {
                let chosen = slash.selected_item().map(|item| item.label.clone());
                if let Some(name) = chosen {
                    // The name and a blank after it: the blank is what
                    // settles the name, so the list is done and whatever
                    // the command takes is typed next.
                    if let Some(talk) = self.conversation_mut() {
                        talk.chat.put(&format!("{name} "));
                        talk.slash = None;
                    }
                }
                true
            }
            // The list, not the conversation: escape gives up on the
            // nearest thing first, and what the reader typed stays.
            KeyCode::Esc => {
                if let Some(talk) = self.conversation_mut() {
                    talk.slash = None;
                }
                true
            }
            _ => false,
        }
    }

    /// The card an agent's question is on, while one is up.
    #[must_use]
    pub fn card(&self) -> Option<&Card> {
        self.conversation().and_then(|talk| talk.card.as_ref())
    }

    /// Offers a key to the conversation, and says whether it took it.
    ///
    /// Two things in one, because the reader sees one: the agent's own
    /// commands, while a list of them is following what is being typed, and
    /// then the conversation itself -- the transcript, the box, and the row
    /// of settings under it.
    pub(super) fn chat_key(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        if self.conversation().is_none() {
            return false;
        }
        // The card an agent's question is answered on, which is nearer than
        // anything else here: it covers the box a message would be written
        // in, because while the agent is waiting on an answer there is no
        // message to send. The conversation's own order, now that the
        // conversation is a document rather than something over one.
        if self.card_key(key) {
            return true;
        }
        let thinking = self.talking() == Talking::Thinking;
        // The room the two halves have, from the same functions the view
        // lays them out with: a page of scrolling is the page on screen,
        // and the caret moves by the rows the box really has.
        let room = ChatRoom {
            transcript: self.conversation().map_or(0, |talk| {
                crate::ui::chat::bands(self.editor_area, &talk.chat, talk.card.as_ref())
                    .transcript
                    .height
            }),
            reading: crate::ui::chat::reading_width(self.editor_area),
            writing: crate::ui::chat::writing_width(self.editor_area),
        };
        // The list of the agent's own commands, when one is showing: it
        // follows what is being typed in the box, so it takes the keys that
        // move about a list and leaves the rest to the box.
        if self.slash_key(key) {
            return true;
        }
        // What the agent lets the reader change, which is what the row under
        // the box is showing -- so the keys that walk it need it as much as
        // the view does. Cloned because the box is about to be borrowed to
        // take the key.
        let settings = self.agent_settings().to_vec();
        let Some(talk) = self.conversation_mut() else {
            return false;
        };
        match talk.chat.handle_key(key, thinking, room, &settings) {
            ChatOutcome::Consumed => true,
            ChatOutcome::Send(text) => {
                self.send_to_agent(&text);
                true
            }
            ChatOutcome::Interrupt => {
                self.interrupt_agent();
                true
            }
            // Somewhere the reader was sent, sent again: the browser tab
            // is closed, the sign-in was not finished. The agent is not
            // asked anything -- it was told they went the first time, and
            // it is watching the far end rather than obelus.
            ChatOutcome::Away(url) => {
                if let Err(error) = crate::links::open(&url) {
                    tracing::warn!(%error, "the link was not opened");
                    if let Some(talk) = self.conversation_mut() {
                        talk.chat.note("nothing here opens links");
                    }
                }
                true
            }
            // Where a row of the transcript says the agent was. Going
            // there is switching to that file, which is a document like
            // this one -- so the conversation stays exactly where it was
            // and `ctrl+o` comes back to it. It used to have to be hidden,
            // because hiding it was the only way to show a file.
            ChatOutcome::GoTo(place) => {
                // The protocol counts a file's lines from one and the rest
                // of obelus counts them from zero, which is what `go_to`
                // takes: a language server's numbering, because that is who
                // it was written for.
                let line = place.line.unwrap_or(1).saturating_sub(1);
                self.go_to(&place.path, line, 0);
                true
            }
            ChatOutcome::Choose(id) => {
                self.open_agent_setting(&id);
                true
            }
            ChatOutcome::Toggle(id) => {
                self.flip_agent_setting(&id);
                true
            }
            ChatOutcome::StepMode => {
                self.step_agent_mode();
                true
            }
            ChatOutcome::Ignored => false,
        }
    }

    /// Gives a key to the card.
    ///
    /// What it does not take falls through to the table, so `ctrl+q` quits
    /// from a card the way it quits from a list.
    pub(super) fn card_key(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        let Some(card) = self.conversation().and_then(|talk| talk.card.as_ref()) else {
            return false;
        };
        // The width a card's own rows have, which is what its caret is
        // worked out against.
        let width =
            crate::ui::card::width_of(crate::ui::chat::bands_for(self.editor_area, card).writing);
        let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut()) else {
            return false;
        };
        match card.handle_key(key, width) {
            CardOutcome::Ignored => false,
            CardOutcome::Consumed => true,
            // Escape gives up on the nearest thing, and while a question is
            // on screen the nearest thing is the question.
            CardOutcome::Cancelled => {
                match self.is_asking_permission() {
                    true => self.refuse_permission(),
                    false => self.refuse_asking(),
                }
                true
            }
            CardOutcome::Answered { chosen, words } => {
                self.answer_card(&chosen, words.as_deref());
                true
            }
        }
    }

    /// Whether the agent is waiting on an answer to something it asked.
    #[must_use]
    pub fn is_asking(&self) -> bool {
        self.conversation()
            .is_some_and(|talk| talk.asking.is_some())
    }

    /// Makes room for a question the agent is waiting on an answer to.
    ///
    /// It no longer brings the conversation to the front, and the name is
    /// what is left of one that did: a conversation is a document now, so a
    /// question asked in one the reader is not in waits on its row in the
    /// list with a mark saying so, rather than pulling the screen away from
    /// whatever they were reading.
    ///
    /// What is still here is the clearing: a card is drawn inside the
    /// conversation, so anything of obelus's own over that region would be a
    /// card the reader cannot see while the agent waits on it.
    ///
    /// And whatever question was already up, which is the same clearing
    /// for the same reason. There is one card, so a second question takes
    /// the first one's place; letting the first stay would leave the agent
    /// waiting for ever on a question nothing on screen is asking, and --
    /// where the two were of different kinds -- would send the answer to
    /// the card on screen back to the wrong one of them.
    ///
    /// One agent cannot do this: the protocol's dispatch loop hands a
    /// message to one handler at a time and waits for it, and obelus's
    /// elicitation handler waits for the reader -- so a second question
    /// from the same connection is not read until the first is answered.
    /// Two agents are two connections and two loops, and both of their
    /// questions land on whichever conversation the reader is in, which is
    /// the same "one of them, for now" this file says elsewhere. That is
    /// why this is here and why no test drives it.
    fn show_the_question(&mut self) {
        // To the conversation, because the card is inside it: a question
        // asked while the reader is in a file would be a card nobody can
        // see, taking their keys and holding up an agent waiting for an
        // answer they were never shown.
        self.open_agent();
        // And nothing of the reader's own over it. The list of the agent's
        // commands follows what is being typed in the box, and the box is
        // what the card covers: left open it would be a list over a
        // question, about words the keys are no longer going to.
        if let Some(talk) = self.conversation_mut() {
            talk.slash = None;
            // Dropped rather than answered: a channel that goes away is
            // what the agent hears as a cancellation, which is the truth
            // about a question nobody was ever shown.
            talk.asking = None;
            talk.going = None;
        }
    }

    /// Puts a form the agent asked for to the reader.
    fn ask_reader(
        &mut self,
        message: &str,
        fields: Vec<acp::Field>,
        answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
    ) {
        self.show_the_question();
        if let Some(talk) = self.conversation_mut() {
            talk.asking = Some(Asking {
                message: message.to_string(),
                left: fields.into(),
                given: Vec::new(),
                answer,
            });
        }
        self.put_the_question();
    }

    /// Puts somewhere the agent wants the reader to go to the reader.
    ///
    /// On a card, like everything else it asks -- but a card with nothing
    /// to fill in: what it takes is whether the reader will go, and the URL
    /// itself is what the card is about. Shown whole, folded across as many
    /// rows as it takes, because a URL cut short is a URL nobody can use
    /// and this is the one thing on screen a reader may have to read out.
    fn send_the_reader(&mut self, message: &str, url: &str, id: &str, answer: acp::Answer<bool>) {
        self.show_the_question();
        if let Some(talk) = self.conversation_mut() {
            talk.going = Some(crate::conversation::Going {
                message: message.to_string(),
                url: url.to_string(),
                id: id.to_string(),
                answer,
            });
        }
        self.put_the_place();
    }

    /// The card for it.
    fn put_the_place(&mut self) {
        let Some(going) = self.conversation().and_then(|talk| talk.going.as_ref()) else {
            return;
        };
        // The agent's words, then the URL under them. One text rather than
        // two fields, because the card lays out what it is about as one
        // wrapped block -- and a blank line between them is what makes the
        // second read as a thing rather than as more of the sentence.
        let about = match going.message.trim().is_empty() {
            true => going.url.clone(),
            false => format!("{}\n\n{}", going.message, going.url),
        };
        let icons = crate::icons::enabled();
        let mut card = Card::new(
            vec![
                Choice {
                    id: "open".to_string(),
                    name: "open it".to_string(),
                    about: Some("obelus opens it in your browser".to_string()),
                    icon: icons.then_some(crate::icons::ui::AWAY),
                    chosen: false,
                },
                Choice {
                    id: "no".to_string(),
                    name: "no".to_string(),
                    about: None,
                    icon: icons.then_some(crate::icons::ui::STAYING),
                    chosen: false,
                },
            ],
            false,
        );
        card.about(&about);
        if let Some(talk) = self.conversation_mut() {
            talk.card = Some(card);
        }
    }

    /// Sends the reader there, or tells the agent they will not go.
    ///
    /// Answered the moment they are sent, not when they come back: what the
    /// agent asked for is that the reader be directed somewhere, and it
    /// watches the far end itself. Holding the answer until a sign-in
    /// finished would hold a turn open for as long as somebody takes to
    /// find their password.
    fn answer_going(&mut self, chosen: Option<&str>) {
        let Some(going) = self.conversation_mut().and_then(|talk| talk.going.take()) else {
            return;
        };
        if chosen != Some("open") {
            if let Some(talk) = self.conversation_mut() {
                talk.card = None;
                talk.chat.note("not opened");
            }
            let _ = going.answer.send(false);
            return;
        }
        if let Err(error) = crate::links::open(&going.url) {
            tracing::warn!(%error, "the link was not opened");
            // Not an answer: nothing was opened, so the reader has not been
            // sent anywhere. The card stays, with the URL still on it --
            // which on a machine with no browser is the only way they will
            // get it.
            if let Some(talk) = self.conversation_mut() {
                talk.chat.note("nothing here opens links");
                talk.going = Some(going);
            }
            return;
        }
        if let Some(talk) = self.conversation_mut() {
            talk.card = None;
            // A row rather than the card kept open: the agent is no longer
            // waiting on obelus -- it was told they went -- so the box has
            // to come back. What is left is a thing under way, which is
            // what the transcript already has a shape for.
            talk.chat.away(&going.id, &going.message, &going.url);
        }
        let _ = going.answer.send(true);
    }

    /// The agent says the far end happened, so there is nothing left to
    /// wait for.
    fn went_through(&mut self, id: &str) {
        if let Some(talk) = self.conversation_mut() {
            talk.chat.arrived(id);
        }
    }

    /// Puts the next field, or answers the form when there is none left.
    fn put_the_question(&mut self) {
        let Some(asking) = self.conversation().and_then(|talk| talk.asking.as_ref()) else {
            return;
        };
        if asking.left.is_empty() {
            self.settle_asking();
            return;
        }
        // What the card says it is about: the agent's own words the first
        // time, and afterwards the field's own question -- by then the
        // reader is in the middle of the form, and what they need to know
        // is which part of it this is.
        let about = match asking.given.is_empty() {
            true => asking.message.clone(),
            false => String::new(),
        };
        let (choice, words) = self.asked_now();
        let mut card = match &choice {
            Some(field) => card_of(field),
            None => Card::new(Vec::new(), false),
        };
        let about = match (about.is_empty(), &choice, &words) {
            (false, _, _) => about,
            // A card with nothing but a box on it has nothing else to say
            // what it is for, so the field's question goes above it.
            (true, None, Some(field)) => question(field),
            (true, Some(field), _) => question(field),
            (true, None, None) => String::new(),
        };
        if !about.is_empty() {
            card.about(&about);
        }
        if let Some(field) = &words {
            let suggested = match &field.takes {
                acp::Takes::Words(suggested) => suggested.clone(),
                _ => None,
            };
            // Its title rather than its question: the row it names is one
            // row, and what it is for has been said above.
            card.writing(&field.title, field.required, suggested.as_deref());
        }
        if let Some(talk) = self.conversation_mut() {
            talk.card = Some(card);
        }
    }

    /// The fields the card on screen is answering: the one it puts the
    /// question about, and the one the reader writes their own answer in.
    ///
    /// One function rather than two places working it out, because putting
    /// the question and taking the answer have to agree about which fields
    /// were on the card.
    fn asked_now(&self) -> (Option<acp::Field>, Option<acp::Field>) {
        let Some(asking) = self.conversation().and_then(|talk| talk.asking.as_ref()) else {
            return (None, None);
        };
        let Some(field) = asking.left.front().cloned() else {
            return (None, None);
        };
        match field.takes {
            // Words, and nothing to choose from: the card is the box.
            acp::Takes::Words(_) | acp::Takes::Number { .. } => (None, Some(field)),
            // Named answers, and -- when the agent asked for words next --
            // room to write one of your own under them. Both on the one
            // card, because "one of these, or say what you want instead"
            // is one question however many fields it takes to write down.
            _ => {
                let words = asking
                    .left
                    .get(1)
                    .filter(|next| matches!(next.takes, acp::Takes::Words(_)))
                    .cloned();
                (Some(field), words)
            }
        }
    }

    /// Takes what the reader put on the card.
    pub(super) fn answer_card(&mut self, chosen: &[String], words: Option<&str>) {
        // Somewhere to go is neither a form nor a permission: nothing was
        // filled in, and what the answer decides is whether obelus opens
        // something.
        if self.conversation().is_some_and(|talk| talk.going.is_some()) {
            self.answer_going(chosen.first().map(String::as_str));
            return;
        }
        // A permission request is named answers and nothing else, so the
        // one they chose is the answer.
        if self.is_asking_permission() {
            if let Some(talk) = self.conversation_mut() {
                talk.card = None;
            }
            match chosen.first() {
                Some(option) => self.allow(option),
                None => self.refuse_permission(),
            }
            return;
        }
        let (choice, asked) = self.asked_now();
        let mut given: Vec<(String, acp::Reply)> = Vec::new();
        let mut said: Vec<String> = Vec::new();
        if let Some(field) = &choice {
            match &field.takes {
                acp::Takes::One(values) => match chosen.first() {
                    Some(id) => {
                        said.push(format!("{}: {}", field.title, called(values, id)));
                        given.push((field.name.clone(), acp::Reply::Value(id.clone())));
                    }
                    // Nothing chosen, on a question that did not have to be:
                    // the reader answered in their own words instead, and
                    // the field is left out.
                    None => said.push(format!("{}: left blank", field.title)),
                },
                acp::Takes::Some { values, .. } => {
                    // Nothing ticked is the field left blank, like an empty
                    // box: the key is left out rather than sent as an empty
                    // list, because "I did not answer that" and "none of
                    // them" are not the same answer. A field the agent
                    // needs never gets here -- the card will not send one
                    // with nothing ticked.
                    let names: Vec<String> = chosen.iter().map(|id| called(values, id)).collect();
                    match names.is_empty() {
                        true => said.push(format!("{}: left blank", field.title)),
                        false => {
                            said.push(format!("{}: {}", field.title, names.join(", ")));
                            given.push((field.name.clone(), acp::Reply::Values(chosen.to_vec())));
                        }
                    }
                }
                acp::Takes::Switch(_) => {
                    let on = chosen.first().is_some_and(|id| id == "on");
                    let side = match on {
                        true => "on",
                        false => "off",
                    };
                    said.push(format!("{}: {side}", field.title));
                    given.push((field.name.clone(), acp::Reply::Switch(on)));
                }
                // Not what a card with named answers is asking.
                acp::Takes::Words(_) | acp::Takes::Number { .. } => {}
            }
        }
        if let Some(field) = &asked {
            let text = words.unwrap_or_default().trim().to_string();
            match &field.takes {
                // Nothing typed, and nothing needed: left out of the
                // answer, the way an empty box leaves out a field that
                // takes words. A complaint about a number nobody was
                // asked for is obelus insisting on its own behalf.
                acp::Takes::Number { .. } if text.is_empty() && !field.required => {}
                acp::Takes::Number { whole, least, most } => {
                    let Some(reply) = self.number_of(field, &text, *whole, *least, *most) else {
                        // The reader's slip, so it is said and the card
                        // stays: an answer nobody can give is worse than a
                        // question asked twice.
                        return;
                    };
                    given.push((field.name.clone(), reply));
                }
                _ => {
                    if let Some(text) = words {
                        given.push((field.name.clone(), acp::Reply::Words(text.to_string())));
                    }
                }
            }
        }
        let taken = usize::from(choice.is_some()) + usize::from(asked.is_some());
        let Some(asking) = self
            .conversation_mut()
            .and_then(|talk| talk.asking.as_mut())
        else {
            return;
        };
        for _ in 0..taken {
            asking.left.pop_front();
        }
        asking.given.extend(given);
        for line in said {
            if let Some(talk) = self.conversation_mut() {
                talk.chat.note(&line);
            }
        }
        // Theirs, in the transcript, because that is what they said -- the
        // agent asked in words and this is the answer in words.
        if let Some(text) = words
            && let Some(talk) = self.conversation_mut()
        {
            talk.chat.asked(text);
        }
        if let Some(talk) = self.conversation_mut() {
            talk.card = None;
        }
        self.put_the_question();
    }

    /// A number the reader typed, if it is one the field will take.
    fn number_of(
        &mut self,
        field: &acp::Field,
        text: &str,
        whole: bool,
        least: Option<f64>,
        most: Option<f64>,
    ) -> Option<acp::Reply> {
        let Ok(number) = text.parse::<f64>() else {
            let title = field.title.clone();
            self.in_transcript(|chat| chat.note(&format!("{title} takes a number, not {text:?}")));
            return None;
        };
        if least.is_some_and(|least| number < least) || most.is_some_and(|most| number > most) {
            let asked = question(field);
            self.in_transcript(|chat| chat.note(&format!("that is outside {asked}")));
            return None;
        }
        Some(match whole {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a whole number the reader typed, and the protocol takes an i64"
            )]
            true => acp::Reply::Whole(number as i64),
            false => acp::Reply::Number(number),
        })
    }

    /// Answers the form, now that every field has one.
    fn settle_asking(&mut self) {
        let Some(asking) = self.conversation_mut().and_then(|talk| talk.asking.take()) else {
            return;
        };
        if asking.answer.send(Some(asking.given)).is_err() {
            self.in_transcript(|chat| chat.note("it stopped waiting for an answer"));
        }
    }

    /// Says no to the form, whichever field the reader was on.
    pub(super) fn refuse_asking(&mut self) {
        if let Some(talk) = self.conversation_mut() {
            talk.card = None;
        }
        // Somewhere to go, given up on: the channel going away without an
        // answer is what the agent hears as a cancellation, so there is
        // nothing to send.
        if let Some(talk) = self.conversation_mut()
            && talk.going.take().is_some()
        {
            talk.chat.note("not opened");
            return;
        }
        let Some(asking) = self.conversation_mut().and_then(|talk| talk.asking.take()) else {
            return;
        };
        if let Some(talk) = self.conversation_mut() {
            talk.chat.note("not answered");
        }
        let _ = asking.answer.send(None);
    }

    /// Follows the transcript, and notices an agent that has died.
    ///
    /// Once a frame, like the language servers' own check: an agent that
    /// has exited is not otherwise noticed -- its reader thread stops, and
    /// every prompt after that goes unanswered with nothing to say so.
    pub(super) fn settle_chat(&mut self, editor_area: Rect) {
        // Nothing to check: the thread says when the conversation has
        // ended, and `on_acp` puts that in the transcript once.
        let Some(talk) = self.conversation() else {
            return;
        };
        let width = crate::ui::chat::writing_width(editor_area);
        let needed = talk.chat.writing().rows(width).len();
        let region = crate::ui::chat::regions(editor_area, needed).transcript;
        let rows = talk.chat.rows(region.width.saturating_sub(4)).len();
        if let Some(talk) = self.conversation_mut() {
            talk.chat.settle(rows, region.height);
        }
        // And the focus on the row under the box, against the settings
        // that are really there: they are the agent's, and it can take one
        // away in the middle of a sentence -- a model with no thinking
        // levels does exactly that.
        let settings = self.agent_settings().len();
        if let Some(talk) = self.conversation_mut() {
            talk.chat.settle_focus(settings);
        }
    }

    /// Which conversation on the agent the reader is in, if they are in one.
    fn session_now(&self) -> Option<acp::SessionId> {
        self.conversation()?.session.clone()
    }

    /// Whether a message from the agent belongs to the conversation on
    /// screen.
    ///
    /// The routing, in one place. Most of what arrives names the
    /// conversation it is about, and a message that names one obelus is not
    /// looking at must not put its words in the one it is.
    ///
    /// The two that name none are the two the protocol does not put a
    /// session on: an elicitation, and a request for a file. Those go to
    /// whoever is here, which is right while one conversation is waiting on
    /// the agent and is a guess when two are. The protocol is where that has
    /// to be fixed, so this is where it is written down.
    fn is_this_conversation(&self, incoming: &acp::Incoming) -> bool {
        let whose = match incoming {
            acp::Incoming::Update { session, .. }
            | acp::Incoming::Ended { session, .. }
            | acp::Incoming::Remembered { session }
            | acp::Incoming::Permission { session, .. } => Some(session),
            acp::Incoming::Started { .. }
            | acp::Incoming::Lost { .. }
            | acp::Incoming::Ready(_)
            | acp::Incoming::Failed(..)
            | acp::Incoming::Gone(_)
            | acp::Incoming::Ask { .. }
            | acp::Incoming::Open { .. }
            | acp::Incoming::Finished { .. }
            | acp::Incoming::Read { .. }
            | acp::Incoming::Write { .. } => None,
        };
        whose.is_none_or(|session| self.session_now().as_ref() == Some(session))
    }

    /// Takes one message from the agent.
    pub(super) fn on_acp(&mut self, incoming: acp::Incoming) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        // What the protocol needs of it is dealt with in there -- the
        // handshake, the session, which mode is on -- and what a reader
        // needs to see comes back.
        let Some(incoming) = talker.on(incoming) else {
            return;
        };
        // A conversation that could not be picked up where it was left. The
        // agent has forgotten it -- a session it swept up, a version of it
        // that keeps them differently -- and a fresh one is already on its
        // way. What this has to do is put the conversation back to how one
        // with no session yet looks, because that is what the session
        // arriving next will be looking for; and tell the agent again what
        // the conversation is about, because the words that told it the
        // first time went with the session.
        if let acp::Incoming::Lost { session, why } = &incoming {
            let at = self.documents.iter().position(|document| {
                document
                    .as_ref()
                    .and_then(Document::chat)
                    .is_some_and(|talk| talk.asked_for.as_ref() == Some(session))
            });
            let opening = at.and_then(|at| self.opening_for(at));
            if let Some(talk) = at
                .and_then(|at| self.documents.get_mut(at))
                .and_then(Option::as_mut)
                .and_then(Document::chat_mut)
            {
                talk.asked_for = None;
                talk.opening = opening;
                talk.chat.note(&format!("starting again, because {why}"));
            }
            return;
        }
        // A conversation opening is the one message that is not routed by a
        // session: it is what *hands out* one. It goes to whichever
        // conversation has not got one yet, because a conversation asks for
        // a session only when it is opened and only ever needs the one.
        if let acp::Incoming::Started { session, .. } = &incoming {
            let session = session.clone();
            // The one that asked for this name, if one did -- a conversation
            // being taken up again knows which it wants. Only then the first
            // that has none, which is what a freshly opened one is.
            //
            // Two of them starting at once is the case this is for: told
            // apart by nothing, the second answer would go to whichever
            // happened to be first in the list.
            let asked = self
                .documents
                .iter()
                .position(|document| {
                    document
                        .as_ref()
                        .and_then(Document::chat)
                        .is_some_and(|talk| talk.asked_for.as_ref() == Some(&session))
                })
                .or_else(|| {
                    self.documents.iter().position(|document| {
                        document
                            .as_ref()
                            .and_then(Document::chat)
                            .is_some_and(|talk| talk.session.is_none() && talk.asked_for.is_none())
                    })
                });
            let mine = asked.and_then(|at| {
                self.documents
                    .get_mut(at)
                    .and_then(Option::as_mut)
                    .and_then(Document::chat_mut)
            });
            if let Some(talk) = mine {
                talk.asked_for = None;
                talk.session = Some(session);
            }
            self.remember_the_conversations();
            return;
        }
        // And everything else goes to the conversation it names. One of
        // them, for now -- but asking is what stops a second one's words
        // landing in the first.
        if !self.is_this_conversation(&incoming) {
            return;
        }
        match incoming {
            acp::Incoming::Update { update, .. } => match update {
                acp::Update::Said(text) => {
                    self.in_transcript(|chat| chat.chunk(Speaker::Agent, &text))
                }
                acp::Update::Thought(text) => {
                    self.in_transcript(|chat| chat.chunk(Speaker::Thought, &text))
                }
                // The reader's own words, as the agent has them. What this
                // is for is a conversation taken up again after obelus was
                // shut: the transcript is the agent's, and this is the half
                // of it obelus cannot write itself.
                acp::Update::Heard(text) => self.in_transcript(|chat| chat.heard(&text)),
                acp::Update::Tool { call, status } => {
                    self.in_transcript(|chat| chat.tool(&call, &status))
                }
                // What it means to do about this turn. Not a thing said --
                // it never goes in the transcript -- so it is handed to the
                // row that says what is happening now, which is where a
                // state belongs and where one cannot be left behind.
                acp::Update::Plan(steps) => self.in_transcript(|chat| chat.planning(steps)),
                // What the agent calls this conversation, which is the
                // name it goes by in the list of open documents -- so it is
                // written down rather than only shown.
                acp::Update::Titled(_) => self.remember_the_conversations(),
                // Kept by the handle, which is where the view reads them:
                // these are facts about the agent rather than things it
                // said, and a transcript with them in it is a log.
                acp::Update::Mode(_)
                | acp::Update::Orders(_)
                | acp::Update::Settings(_)
                | acp::Update::Used(_) => {}
            },
            acp::Incoming::Ended { why: reason, .. } => {
                // Only the ends that are not the ordinary one: a turn that
                // finished has its answer above it, and "end turn" under
                // every answer is noise.
                match reason.as_str() {
                    "end_turn" => {}
                    "cancelled" => self.in_transcript(|chat| chat.note("stopped")),
                    "refusal" => self.in_transcript(|chat| chat.note("it declined to answer")),
                    "max_tokens" => {
                        self.in_transcript(|chat| chat.note("it ran out of room to answer in"));
                    }
                    other => self.in_transcript(|chat| chat.note(other)),
                }
            }
            acp::Incoming::Failed(what, why) => {
                tracing::warn!(what, why, "the agent");
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.note(&format!("{what}: {why}"));
                }
            }
            // Answered above, before the session it is about can arrive.
            acp::Incoming::Lost { .. } => {}
            // Taken up where the reader left it, by an agent that cannot
            // send back what was said. The page is empty and the agent is
            // not: without this the reader is looking at a conversation
            // that appears to have nothing in it, and starts explaining it
            // all again to something that already knows.
            acp::Incoming::Remembered { .. } => {
                self.in_transcript(|chat| {
                    chat.note("taken up where you left it; this agent cannot send back what was said");
                });
            }
            acp::Incoming::Permission {
                call,
                reason,
                options,
                answer,
                ..
            } => self.ask_permission(&call, reason.as_deref(), &options, answer),
            acp::Incoming::Ask {
                message,
                fields,
                answer,
            } => self.ask_reader(&message, fields, answer),
            acp::Incoming::Open {
                message,
                url,
                id,
                answer,
            } => self.send_the_reader(&message, &url, &id, answer),
            acp::Incoming::Finished { id } => self.went_through(&id),
            acp::Incoming::Read {
                path,
                line,
                limit,
                answer,
            } => self.read_for_agent(&path, line, limit, answer),
            acp::Incoming::Write { path, text, answer } => {
                self.write_for_agent(&path, &text, answer);
            }
            acp::Incoming::Gone(why) => {
                // Whatever it was waiting on goes with it. The handle
                // stays, dead, because the view reads the state off it --
                // and talking to it again is what starts the next one.
                self.forget_the_question();
                match why {
                    Some(why) => {
                        self.in_transcript(|chat| chat.note(&format!("the agent stopped: {why}")))
                    }
                    None => self.in_transcript(|chat| chat.note("the agent stopped")),
                }
            }
            // Folded into the handle above, or -- for a conversation
            // opening -- dealt with before the routing, because it is what
            // hands out the name the routing goes by.
            acp::Incoming::Ready(_) | acp::Incoming::Started { .. } => {}
        }
    }

    /// Starts an agent, whatever it is and wherever it came from.
    ///
    /// The seam between "which agent" and "talking to one": what is above
    /// this reads the reader's choice out of the settings, and what is below
    /// it only needs a command. Public because it is also how a test gets a
    /// conversation without a registry, an install, or a network.
    pub fn talk_to(&mut self, id: &str, command: &Path, arguments: &[String]) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        tracing::info!(id, command = %command.display(), "starting an agent");
        // Nothing to fail here: the process is started on the thread, and
        // an agent that will not run says so as the conversation ending
        // with a reason -- which is the same path as one that dies later.
        // What obelus offers the agent back, if it could take a socket.
        // Started once and kept for as long as obelus runs: the address is
        // what each agent is told, so a second one started later reaches the
        // same tools.
        let tools = self.tools_url.clone();
        self.talker = Some(acp::Talk::start(
            id,
            command,
            arguments,
            &self.working_directory,
            tools,
            sender,
        ));
    }

    /// Starts the active agent, or says why it cannot.
    fn start_agent(&mut self) {
        let Some(id) = self
            .settled
            .config
            .agent
            .clone()
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        let Some(root) = self.agents_root() else {
            self.in_transcript(|chat| {
                chat.note("this system has nowhere for obelus to keep an agent")
            });
            return;
        };
        // What the install wrote down when it finished. Nothing here means
        // no install finished -- the reader removed it, or obelus was shut
        // while one was running -- and the agents page is where that is
        // fixed, so that is where they are sent.
        let Some(installed) = crate::agent::installation(&id, &root) else {
            tracing::warn!(
                id,
                "no agent to talk to: nothing is installed under that name"
            );
            if let Some(talk) = self.conversation_mut() {
                talk.chat.note(&format!(
                    "{id} is not installed \u{2014} open the settings and install it"
                ));
            }
            return;
        };
        self.talk_to(&id, &installed.command, &installed.arguments);
    }

    /// Puts a permission request to the reader, on the card everything
    /// else it asks is answered on.
    ///
    /// Which is what it is: a question in the agent's words with a few
    /// named answers and no room to write your own, because the protocol
    /// takes one of its own options and nothing else.
    fn ask_permission(
        &mut self,
        call: &acp::Call,
        reason: Option<&str>,
        options: &[acp::Choice],
        answer: acp::Answer<Option<String>>,
    ) {
        self.show_the_question();
        // The call goes in the transcript, where every call goes, waiting
        // -- which is what says the agent is asking about it. obelus used
        // to write a line of its own here ("asking to run the tests"), and
        // that is the same words twice now that what it is asking about is
        // a row above the question.
        //
        // A change it is asking to make goes with it, open, because the
        // lines it would write *are* the question: they are in neither the
        // file nor the last commit, so this is the only place they exist.
        // After the answer they stay, which is how a reader finds out later
        // what they agreed to.
        if let Some(talk) = self.conversation_mut() {
            talk.chat.tool(call, "pending");
        }
        let choices = options
            .iter()
            .map(|choice| Choice {
                id: choice.id.clone(),
                name: choice.name.clone(),
                about: None,
                icon: icons::enabled().then(|| icons::for_permission(&choice.kind)),
                chosen: false,
            })
            .collect();
        let mut card = Card::new(choices, false);
        card.needs_one();
        // What it is actually about to do, above the answers: "allow" and
        // "refuse" are answers to a question, and the question is which
        // command on which file rather than the line the title fits in.
        // Nothing at all where it said nothing -- what it is asking about
        // is the row above the card, and an empty block is a rule around
        // silence.
        //
        // And nothing where the call said it in words either, for the same
        // reason the line obelus used to write here was deleted: those
        // words are on the row above now, whole and foldable and still
        // there after the answer. A card is five rows tall, so a copy here
        // is the first fifth of something the reader can already see all
        // of -- and for a plan, which is what the longest of these are, the
        // first fifth is the heading.
        if let Some(reason) = reason
            .filter(|reason| !reason.trim().is_empty())
            .filter(|_| call.said.is_empty())
        {
            card.about(reason);
        }
        if let Some(talk) = self.conversation_mut() {
            talk.permission = Some(answer);
        }
        if let Some(talk) = self.conversation_mut() {
            talk.card = Some(card);
        }
    }

    /// Answers the permission request the reader chose an option for.
    pub(super) fn allow(&mut self, option: &str) {
        let Some(answer) = self
            .conversation_mut()
            .and_then(|talk| talk.permission.take())
        else {
            return;
        };
        if answer.send(Some(option.to_string())).is_err() {
            self.in_transcript(|chat| chat.note("it stopped waiting for an answer"));
        }
    }

    /// Tells the agent the reader would not answer.
    ///
    /// The protocol has an outcome for it, and it matters: an agent whose
    /// request is never answered waits for ever, and one that is told it
    /// was cancelled ends the turn and says so.
    pub(super) fn refuse_permission(&mut self) {
        if let Some(talk) = self.conversation_mut() {
            talk.card = None;
        }
        let Some(answer) = self
            .conversation_mut()
            .and_then(|talk| talk.permission.take())
        else {
            return;
        };
        let _ = answer.send(None);
        if let Some(talk) = self.conversation_mut() {
            talk.chat.note("not answered");
        }
    }

    /// Whether a permission request is waiting on the reader.
    #[must_use]
    pub fn is_asking_permission(&self) -> bool {
        self.conversation()
            .is_some_and(|talk| talk.permission.is_some())
    }

    /// Answers the agent's request for a file's text.
    ///
    /// From an open buffer when obelus has one, because what the reader is
    /// looking at is not always what is on disk -- and the whole point of an
    /// agent inside a reader is that they are looking at the same thing.
    /// Otherwise from disk.
    ///
    /// Refused outside the project, whichever way the text would have come:
    /// an agent asking for something outside the tree obelus was started on
    /// is asking for something the reader did not open it to look at.
    /// Writes a file for the agent.
    ///
    /// Through the buffer where obelus has one open, so the reader can undo
    /// it. That is the whole of why this is allowed at all: the objection
    /// was never that an agent should not change a file, it was that a
    /// reader could not see the change arrive or take it back. A document on
    /// screen that changed under them is one `ctrl+z` from what it was.
    ///
    /// Inside the working directory only, the same fence a read is behind.
    /// Writes a file for the agent, as a test asks it to.
    ///
    /// Straight in rather than through an `Incoming`: that road needs a live
    /// agent to have opened it, and what is worth testing here is what
    /// happens to the document rather than how the message arrived.
    pub fn write_for_agent_for_test(&mut self, path: &Path, text: &str) -> bool {
        let (answer, answered) = futures::channel::oneshot::channel();
        self.write_for_agent(path, text, answer);
        futures::executor::block_on(answered).unwrap_or(false)
    }

    fn write_for_agent(&mut self, path: &Path, text: &str, answer: acp::Answer<bool>) {
        let full = match path.is_absolute() {
            true => path.to_path_buf(),
            false => self.working_directory.join(path),
        };
        // A path that does not exist yet cannot be canonicalised, so the
        // fence is tested on the directory it would go in.
        let inside = full
            .canonicalize()
            .or_else(|_| {
                full.parent()
                    .map(std::path::Path::canonicalize)
                    .unwrap_or_else(|| Err(std::io::ErrorKind::NotFound.into()))
            })
            .is_ok_and(|full| full.starts_with(&self.working_directory));
        if !inside {
            tracing::info!(path = %full.display(), "the agent asked to write outside the tree");
            let _ = answer.send(false);
            return;
        }

        let open = self
            .documents
            .iter()
            .enumerate()
            .find(|(_, buffer)| {
                buffer
                    .as_ref()
                    .and_then(Document::file)
                    .is_some_and(|buffer| buffer.path() == full && buffer.content().is_file())
            })
            .map(|(index, _)| index);

        let wrote = match open {
            // The whole document replaced as one change, which is one step
            // back. The protocol has no way to say "this part", so what
            // arrives is the file with the change already in it.
            Some(index) => {
                let whole = self
                    .file(DocumentId::new(index))
                    .map(|buffer| buffer.spanning_all());
                let changed = whole.is_some_and(|span| {
                    self.file_mut(DocumentId::new(index)).is_some_and(|buffer| {
                        buffer.edit(span, text, crate::buffer::undo::Doing::Whole)
                    })
                });
                if changed {
                    self.change_document(index);
                    self.note = Some("the agent changed this file".to_string());
                }
                // A write of what is already there changed nothing and is
                // not a failure: the agent asked for a state, and that is
                // the state.
                true
            }
            None => match std::fs::write(&full, text) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, path = %full.display(), "writing for the agent failed");
                    false
                }
            },
        };
        let _ = answer.send(wrote);
    }

    fn read_for_agent(
        &mut self,
        path: &Path,
        line: Option<u32>,
        limit: Option<u32>,
        answer: acp::Answer<Option<String>>,
    ) {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_directory.join(path)
        };
        let inside = full
            .canonicalize()
            .ok()
            .is_some_and(|full| full.starts_with(&self.working_directory));
        let text = if inside {
            self.documents
                .iter()
                .flatten()
                .filter_map(Document::file)
                .find(|buffer| buffer.path() == full)
                .map(|buffer| buffer.text().rope().to_string())
                .or_else(|| std::fs::read_to_string(&full).ok())
        } else {
            None
        };

        // A line and a limit, when it asked for them: an agent reading a
        // large file asks for a window of it, and answering with the whole
        // thing is a different answer.
        let _ = answer.send(text.map(|text| window(&text, line, limit)));
    }
}

/// One field, as a question in the transcript.
///
/// The title, and what it will take: a number with bounds is a question
/// that has to say them, because a reader who types the wrong one only
/// finds out afterwards.
fn question(field: &acp::Field) -> String {
    let mut asked = field.title.clone();
    if let acp::Takes::Number { whole, least, most } = field.takes {
        let kind = match whole {
            true => "a whole number",
            false => "a number",
        };
        let bounds = match (least, most) {
            (Some(least), Some(most)) => format!(": {kind} from {least} to {most}"),
            (Some(least), None) => format!(": {kind}, {least} or more"),
            (None, Some(most)) => format!(": {kind}, {most} or less"),
            (None, None) => format!(": {kind}"),
        };
        asked.push_str(&bounds);
    }
    match field.about.as_deref() {
        Some(about) => format!("{asked} \u{2014} {about}"),
        None => asked,
    }
}

/// What to call one of a field's values in the transcript.
///
/// Its name, or the agent's id for it when it offered one it does not
/// list -- which is still what the reader chose.
fn called(values: &[acp::Value], id: &str) -> String {
    values
        .iter()
        .find(|value| value.id == id)
        .map_or(id, |value| value.name.as_str())
        .to_string()
}

/// One field's card, before it is told what the question is about.
fn card_of(field: &acp::Field) -> Card {
    let mut card = match &field.takes {
        acp::Takes::One(values) => Card::new(choices_of(values, &[]), false),
        acp::Takes::Some {
            values,
            least,
            most,
            chosen,
        } => {
            let mut card = Card::new(choices_of(values, chosen), true);
            card.counts(*least, *most);
            card
        }
        // On and off, which is a choice of two -- and the one it is on
        // starts chosen, because that is the answer until the reader says
        // otherwise.
        acp::Takes::Switch(on) => {
            let sides = [
                acp::Value {
                    id: "on".to_string(),
                    name: "on".to_string(),
                    about: None,
                },
                acp::Value {
                    id: "off".to_string(),
                    name: "off".to_string(),
                    about: None,
                },
            ];
            let mut card = Card::new(choices_of(&sides, &[]), false);
            card.prefer(match on {
                true => "on",
                false => "off",
            });
            card
        }
        // A card with nothing to choose from is a card with a box on it,
        // which the caller puts there: the field it writes is not always
        // this one.
        acp::Takes::Words(_) | acp::Takes::Number { .. } => Card::new(Vec::new(), false),
    };
    if field.required {
        card.needs_one();
    }
    card
}

/// A field's values, as the card's rows.
fn choices_of(values: &[acp::Value], chosen: &[String]) -> Vec<Choice> {
    values
        .iter()
        .map(|value| Choice {
            id: value.id.clone(),
            name: value.name.clone(),
            about: value.about.clone(),
            icon: None,
            chosen: chosen.contains(&value.id),
        })
        .collect()
}

/// What to call a value in the transcript.
///
/// Its name, or its id if the agent offered one it does not list -- which is
/// still what the reader chose.
fn what_to_say(setting: &acp::Setting, value: &str) -> String {
    setting
        .values
        .iter()
        .find(|known| known.id == value)
        .map_or(value, |known| known.name.as_str())
        .to_string()
}

/// The lines of `text` an agent asked for.
///
/// `line` is counted from one, which is the protocol's own choice and not
/// obelus's: the file's first line is line 1.
fn window(text: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let first = line.unwrap_or(1).max(1) as usize - 1;
    let taken = limit.map_or(usize::MAX, |limit| limit as usize);
    let mut out = String::new();
    for row in text.lines().skip(first).take(taken) {
        out.push_str(row);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::window;

    /// The protocol counts a file's lines from one, and asking for line 2 of
    /// a three-line file has to give the second line rather than the third.
    #[test]
    fn a_window_starts_at_the_line_it_was_asked_for() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(window(text, None, None), text);
        assert_eq!(window(text, Some(2), None), "two\nthree\n");
        assert_eq!(window(text, Some(1), Some(1)), "one\n");
        assert_eq!(window(text, Some(2), Some(1)), "two\n");
        // A line past the end is no lines, not a panic.
        assert_eq!(window(text, Some(9), Some(1)), "");
        // Line zero does not exist; the first line is what was meant.
        assert_eq!(window(text, Some(0), Some(1)), "one\n");
    }
}
