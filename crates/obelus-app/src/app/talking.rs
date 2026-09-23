//! Talking to the active agent.
//!
//! One agent at a time, started when the reader first opens a conversation
//! and left running until they close obelus or choose another. A
//! conversation is a document in the list of what is open, so leaving one is
//! going somewhere else rather than closing it: what was said is still there
//! when the reader comes back to that row.
//!
//! What arrives from the agent is an
//! [`Event::Agent`](crate::event::Event::Agent) like every other background
//! source, so nothing here waits on anything.

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_agent::{Talking, acp};
use obelus_component::{
    card::{Card, CardOutcome, Choice},
    chat::{Chat, Speaker},
};

use super::*;
use crate::conversation::{Asking, Topic};

/// Which conversation a message from the agent is for.
///
/// Its own type rather than an `Option<DocumentId>` because the two cases
/// are not "one or none": a message about a session belongs to the document
/// that opened it and nowhere else, and a message the protocol puts no
/// session on belongs to whoever the reader is with. An `Option` would have
/// read the second as "no conversation".
#[derive(Clone, Copy, Debug)]
enum Whose {
    /// The conversation that opened the session it names.
    One(DocumentId),
    /// Whichever one the reader is in, for the two the protocol names no
    /// session on: an elicitation, and somewhere to go.
    Whoever,
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
        self.ask_for_a_session(None);
    }

    /// Goes to the conversation about one note, opening one if there is
    /// none.
    ///
    /// By the note's name rather than its place in the list, which is what
    /// the name is for: the list is read from the file every time it opens,
    /// and a note added above would otherwise hand the reader somebody
    /// else's conversation.
    pub(super) fn talk_about(&mut self, note: &obelus_git::todo::NoteId) {
        self.make_room(Room::Region);
        let wanted = Topic::Note(note.clone());
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == wanted)
        });
        let at = at.unwrap_or_else(|| {
            let (told, introduced) = self.remembered_telling(note);
            let talk = crate::conversation::Conversation {
                told,
                introduced,
                topic: wanted,
                ..crate::conversation::Conversation::default()
            };
            self.documents.push(Some(talk.into()));
            self.documents.len() - 1
        });
        self.go_to_document(DocumentId::new(at));
        // Started where nothing is running, and then asked about *this*
        // conversation just the same. Returning here is what the reader
        // met every morning: the first note they opened after obelus
        // started did the starting and stopped, before the line below
        // that looks up the name written down beside the note. So it
        // asked for nothing, the session the agent opens on its way up
        // was handed to it as the first one wanting one, and a
        // conversation the agent still had every word of came back blank.
        // Only the first, because the second found the agent running.
        //
        // Nothing to wait for: the handle is made here and the asks go
        // down a channel the connection reads when it is up.
        if self.talker.is_none() {
            self.start_agent();
        }
        self.ask_for_a_session(Some(note));
    }

    /// Asks the agent for the conversation the one on screen wants.
    ///
    /// The one place a session is asked for, and nothing is opened behind
    /// a conversation's back. The connection used to mint one the moment
    /// it came up -- and a view that opens on a note already naming a
    /// conversation then had a second, empty one to go with it, which the
    /// agent does not keep and obelus could still write down against the
    /// note in place of the one the reader had been talking in.
    ///
    /// The one it had before where obelus wrote the name down: the agent
    /// kept every word of it, which is why obelus keeps none. A new one
    /// otherwise -- one agent, several conversations, because an agent
    /// holds a project's worth of context and a second process would pay
    /// for all of it twice.
    ///
    /// Nothing at all where this conversation has a session, or has asked
    /// for one and not been answered yet. The key that opens a
    /// conversation is a key a reader can press twice, and an agent takes
    /// a moment to answer: without the second half of that, the second
    /// press opened a conversation the first press was already opening.
    fn ask_for_a_session(&mut self, note: Option<&obelus_git::todo::NoteId>) {
        let settled = self
            .conversation()
            .is_some_and(|talk| talk.session.is_some() || talk.asked_for.is_some());
        if settled {
            return;
        }
        let had = note.and_then(|note| self.remembered_session(note));
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        match had {
            Some(session) => {
                talker.reopen(&session);
                if let Some(talk) = self.conversation_mut() {
                    talk.asked_for = Some(obelus_agent::acp::SessionId::new(session));
                }
            }
            None => talker.open(),
        }
    }

    /// The conversation obelus had about this note with the agent that is
    /// running, if it wrote one down.
    fn remembered_session(&self, note: &obelus_git::todo::NoteId) -> Option<String> {
        let agent = self.talker.as_ref()?.id();
        // Looking one up, so remembering none is an answer this can live
        // with: the cost of it is the conversation being started again.
        let kept = obelus_agent::acp::sessions::read(&self.working_directory)
            .remembered()
            .unwrap_or_default();
        Some(kept.get(note, agent)?.session.clone())
    }

    /// What this agent has already been told, in a conversation about this
    /// note that obelus wrote down.
    ///
    /// Both halves together, because they are read at one moment for one
    /// purpose -- filling in a conversation that is being picked up where
    /// it was left -- and asking the file twice for two fields of one row
    /// is two answers that can disagree.
    fn remembered_telling(&self, note: &obelus_git::todo::NoteId) -> (Option<String>, bool) {
        let Some(agent) = self.talker.as_ref().map(obelus_agent::acp::Talk::id) else {
            return (None, false);
        };
        let kept = obelus_agent::acp::sessions::read(&self.working_directory)
            .remembered()
            .unwrap_or_default();
        kept.get(note, agent)
            .map_or((None, false), |kept| (kept.told.clone(), kept.introduced))
    }

    /// Whether each note has a conversation about it, in the order the
    /// notes are in -- which is what a row of the notes names.
    ///
    /// Two places count. A conversation open right now is one, and a name
    /// written down against the note is the other: obelus keeps those so
    /// that a note talked over yesterday can be taken up again, and a list
    /// that only knew about open ones would say "nobody has talked about
    /// this" to a reader whose agent still holds every word of it.
    ///
    /// Against the agent the reader is set up to talk to rather than any
    /// agent that ever was. A conversation held with one they have since
    /// switched away from is not one they can reach, and saying there is
    /// one would send them to a note that opens an empty page.
    ///
    /// Worked out each time rather than kept. The file is a few lines and
    /// it is read only while the notes are on screen, and the thing a
    /// cache would buy here is the one thing this must not have: an answer
    /// that goes on saying a note has a conversation after it has not.
    #[must_use]
    pub fn talked_about(&self) -> Vec<obelus_component::todo::Talked> {
        use obelus_component::todo::Talked;

        let Some(notes) = self.notes() else {
            return Vec::new();
        };
        let kept = obelus_agent::acp::sessions::read(&self.working_directory)
            .remembered()
            .unwrap_or_default();
        let agent = self.settled.config.agent.clone().unwrap_or_default();
        notes
            .todo()
            .notes
            .iter()
            .map(|note| {
                let open = self
                    .documents
                    .iter()
                    .flatten()
                    .filter_map(Document::chat)
                    .find(|talk| matches!(&talk.topic, Topic::Note(id) if *id == note.id));

                if open.is_some_and(|talk| talk.card.is_some()) {
                    return Talked::Waiting;
                }
                // And an agent at work in it. Asked of the talker rather
                // than of the conversation, because thinking is the
                // agent's state and not the page's -- the same question
                // the list of open documents asks about the same
                // conversation.
                if open.is_some_and(|talk| {
                    self.talker
                        .as_ref()
                        .is_some_and(|talker| talker.is_thinking(talk.session.as_ref()))
                }) {
                    return Talked::Working;
                }
                let written = !agent.is_empty() && kept.get(&note.id, &agent).is_some();
                match open.is_some() || written {
                    true => Talked::Yes,
                    false => Talked::Not,
                }
            })
            .collect()
    }

    /// Forgets the conversation written down against one note.
    ///
    /// For the one case where obelus knows there is nothing to come back
    /// to: the agent was asked for it by name and said it has no such
    /// thing. Left in the file, that name is asked for again on the next
    /// start and refused again, and the note goes on saying there is a
    /// conversation in it.
    fn forget_the_conversation(&self, note: &obelus_git::todo::NoteId) {
        let Some(agent) = self.talker.as_ref().map(|talker| talker.id().to_string()) else {
            return;
        };
        // `None` where the file will not read, so that nothing is swept
        // against a list obelus does not have: what is remembered here is
        // keyed to notes, and an empty list of names would forget every
        // conversation this tree has.
        let notes: Option<Vec<obelus_git::todo::NoteId>> =
            obelus_git::todo::read(&self.working_directory)
                .notes()
                .map(|todo| todo.notes.into_iter().map(|note| note.id).collect());
        let notes = notes.as_deref();
        obelus_agent::acp::sessions::change(&self.working_directory, notes, |kept| {
            kept.forget(note, &agent);
        });
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
        let mine: Vec<(obelus_git::todo::NoteId, obelus_agent::acp::sessions::Kept)> = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .filter_map(|talk| {
                let Topic::Note(note) = &talk.topic else {
                    return None;
                };
                let session = talk.session.as_ref()?;
                // Nothing said in it yet, so there is nothing to come
                // back to -- and writing it down would take the place of
                // a conversation there *is* something to come back to.
                //
                // An agent does not keep a session nobody said anything
                // in: claude-agent-acp writes a conversation's file on
                // the first turn, so a name minted and never used is a
                // name that will not be there tomorrow. Written down the
                // moment it was minted, it displaced the note's real
                // conversation, and the next morning obelus asked for it,
                // was told there is no such thing, opened another empty
                // one and wrote *that* down. The reader's conversation
                // went in the first round of it and every round after was
                // the same round again.
                //
                // Anything *said*, rather than anything on the page:
                // obelus writes in a conversation of its own accord, and
                // a page holding nothing but "starting again, because"
                // is a page with nothing to come back to.
                if !talk.chat.anything_said() {
                    return None;
                }
                Some((
                    note.clone(),
                    obelus_agent::acp::sessions::Kept {
                        session: session.0.to_string(),
                        title: talker
                            .and_then(|talker| talker.title(Some(session)))
                            .map(str::to_string),
                        told: talk.told.clone(),
                        introduced: talk.introduced,
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
        // `None` where the file will not read, so that nothing is swept
        // against a list obelus does not have: what is remembered here is
        // keyed to notes, and an empty list of names would forget every
        // conversation this tree has.
        let notes: Option<Vec<obelus_git::todo::NoteId>> =
            obelus_git::todo::read(&self.working_directory)
                .notes()
                .map(|todo| todo.notes.into_iter().map(|note| note.id).collect());
        let notes = notes.as_deref();
        obelus_agent::acp::sessions::change(&self.working_directory, notes, |kept| {
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
        obelus_git::todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default()
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
            .map(|card| obelus_ui::card::width_of(obelus_ui::chat::bands_for(area, card).writing));
        let width = obelus_ui::chat::writing_width(area);
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

    /// What obelus is doing about an agent, in the conversation being read.
    ///
    /// Past the state of the process itself, every rung is about *this*
    /// conversation rather than about the agent: both readers of this ask
    /// it while they are drawing one or taking a key in one, and what they
    /// want to know is whether the page in front of them is waiting on
    /// something.
    #[must_use]
    pub fn talking(&self) -> Talking {
        let Some(talker) = self.talker.as_ref() else {
            return match self.settled.config.agent.as_deref() {
                None | Some("") => Talking::Nobody,
                Some(_) => Talking::Idle,
            };
        };
        if talker.has_exited() {
            return Talking::Gone;
        }
        // With no conversation being read there is no session for the two
        // questions below to be about, and they answered anyway: `None` is
        // not a session the agent has, so `is_started` said no and this
        // said "starting..." for as long as an agent was up.
        //
        // Nobody ever saw it -- the row that says it lives in a
        // transcript, and there is no transcript here. The ticker did: a
        // frame asks whether anything is moving, "starting..." is a word
        // with a turning mark beside it, and so obelus woke twelve times a
        // second behind every file, every list and the notes, for a turn
        // that was not running. An agent that is up and being asked
        // nothing is ready, which is what this now says.
        if self.conversation().is_none() {
            return Talking::Ready;
        }
        let held = self.session_now();
        let session = held.as_ref();
        if talker.is_thinking(session) {
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
                opens: None,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask(&question);
        picker.when_empty("This one has nothing to choose from");
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
        // What this agent does not know yet, if anything: who it is talking
        // to, and what the conversation is about. Worked out here rather
        // than kept, because a note is a file the reader can change between
        // any two messages.
        let introduced = self.conversation().is_some_and(|talk| talk.introduced);
        let told = self.conversation().and_then(|talk| talk.told.clone());
        let opening = self.opening(introduced, told.as_deref());
        if let Some(talk) = self.conversation_mut() {
            // A new turn starts with no plan: an agent that made one last
            // turn and makes none this turn would otherwise have the old
            // one shown against the new work.
            talk.chat.plan_forgotten();
            if let Some(opening) = &opening {
                // Written down as told before the agent has answered,
                // because it is the prompt that carries it and the prompt
                // has gone. An agent that never answers has still been
                // told.
                talk.introduced = opening.introduced;
                if let Some(now) = &opening.now {
                    talk.told = Some(now.clone());
                }
                // What obelus sends in the reader's name is the reader's
                // to see.
                for said in &opening.said {
                    talk.chat.note(said);
                }
            }
            talk.chat.asked(text);
        }
        let opening = opening.map(|opening| opening.words);
        // There is something to come back to now, which is the moment a
        // conversation becomes worth writing down against its note. The
        // other moment is the session arriving, and both are needed: a
        // session is handed out before a word is said, and a word can be
        // said before the session arrives -- whichever happens second is
        // the one that finds a conversation with both.
        self.remember_the_conversations();
        // An agent that has stopped is started again by talking to it,
        // which is what the view tells the reader to do. The handle of the
        // one that ended is dropped first: it is still a handle, so a
        // check for "is there one" would find it and say the message to a
        // channel nobody is reading.
        if self
            .talker
            .as_ref()
            .is_none_or(obelus_agent::acp::Talk::has_exited)
        {
            self.stop_agent();
            self.start_agent();
            // The session went with the process that held it, so this
            // conversation asks for its own again -- the one written down
            // against its note where there is one, which is how a
            // conversation survives the agent dying under it. Left as it
            // was, it names a conversation the new process never heard
            // of; and nothing opens one behind its back any more.
            let note = match self.conversation().map(|talk| &talk.topic) {
                Some(Topic::Note(note)) => Some(note.clone()),
                _ => None,
            };
            if let Some(talk) = self.conversation_mut() {
                talk.session = None;
                talk.asked_for = None;
            }
            self.ask_for_a_session(note.as_ref());
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
    ///
    /// And stops the commands obelus is running for this conversation,
    /// rather than only asking. The processes are obelus's -- it started
    /// them -- and an agent told to stop is under no obligation to release
    /// a terminal on its way out: one that did not would leave a build
    /// running that the reader has just said they want stopped, with
    /// nothing left on screen that could stop it.
    ///
    /// The half obelus owes for not asking before it runs them: a key
    /// stops it.
    pub(super) fn interrupt_agent(&mut self) {
        let running: Vec<String> = self
            .conversation()
            .map(|talk| talk.chat.commands())
            .unwrap_or_default();
        for id in running {
            self.runs.stop(&id);
            self.tell_whoever_waited(&id);
        }
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
    ///
    /// In every conversation rather than the one being read. One agent is
    /// behind all of them, and both callers put that agent down -- one
    /// because the reader said so, one because it died -- so every question
    /// it had asked stops being a question at the same moment. Clearing
    /// only the one on screen left the rest holding a card nobody was
    /// waiting for, and wearing the mark the list of open documents puts on
    /// a conversation with a question in it: a reader sent across the list
    /// to answer something that had already stopped being asked.
    ///
    /// And says so where there was one, because otherwise the question
    /// leaves nothing behind at all -- the card gone, the mark gone, and no
    /// reason on the page for either. Which is the whole of what a reader
    /// who was somewhere else would have to go on.
    fn forget_the_question(&mut self) {
        for document in self.documents.iter_mut().flatten() {
            let Some(talk) = Document::chat_mut(document) else {
                continue;
            };
            // The card rather than the channels: it is what the reader
            // could see, so it is what their page has to account for.
            let asked = talk.card.is_some();
            talk.permission = None;
            talk.asking = None;
            talk.going = None;
            talk.card = None;
            if asked {
                talk.chat.note("It stopped waiting for an answer");
            }
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
                opens: None,
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
                obelus_ui::picker::rows_drawn(
                    slash,
                    obelus_ui::chat::above_writing(self.editor_area, chat, self.card()),
                )
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

    /// Whether any conversation is waiting on an answer from the reader.
    ///
    /// Any of them, not the one being read: a question about a conversation
    /// goes to that conversation whether or not it is on screen, so "is
    /// there one" and "is there one here" are two questions now. This is
    /// the first, which is what anything telling the reader there is
    /// somewhere to go back to has to ask -- the list of open documents
    /// answers the same thing a row at a time, by the mark it puts on a
    /// conversation with a card in it.
    #[must_use]
    pub fn anything_waiting(&self) -> bool {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talk.card.is_some())
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
                obelus_ui::chat::bands(self.editor_area, &talk.chat, talk.card.as_ref())
                    .transcript
                    .height
            }),
            reading: obelus_ui::chat::reading_width(self.editor_area),
            writing: obelus_ui::chat::writing_width(self.editor_area),
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
                if let Err(error) = obelus_clipboard::links::open(&url) {
                    tracing::warn!(%error, "the link was not opened");
                    if let Some(talk) = self.conversation_mut() {
                        talk.chat.note("Nothing here opens links");
                    }
                }
                true
            }
            // Where a row of the transcript says the agent was. Going
            // there is switching to that file, which is a document like
            // this one -- so the conversation stays exactly where it was
            // and `alt+left` comes back to it. It used to have to be hidden,
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
            obelus_ui::card::width_of(obelus_ui::chat::bands_for(self.editor_area, card).writing);
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
    fn show_the_question(&mut self, whose: Whose) {
        // The clearing is for the screen, so it happens only where the
        // question is going onto it. A question waiting in a conversation
        // the reader is not in has nothing to clear and no business
        // closing what is in front of them.
        if self.is_here(whose) {
            self.make_room(Room::Region);
        }
        // The one case left that has nowhere to be asked: a question the
        // protocol puts no session on, arriving while the reader is in a
        // file. Those are for whoever is here and there is nobody here, so
        // this is the conversation being opened -- the only thing left of
        // what this used to do to every question that came.
        if matches!(whose, Whose::Whoever) && self.conversation().is_none() {
            self.open_agent();
        }
        // And nothing of the reader's own over it. The list of the agent's
        // commands follows what is being typed in the box, and the box is
        // what the card covers: left open it would be a list over a
        // question, about words the keys are no longer going to.
        if let Some(talk) = self.talk_mut(whose) {
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
        self.show_the_question(Whose::Whoever);
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
        self.show_the_question(Whose::Whoever);
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
        let icons = obelus_icons::enabled();
        let mut card = Card::new(
            vec![
                Choice {
                    id: "open".to_string(),
                    name: "Open it".to_string(),
                    about: Some("Opens it in your browser".to_string()),
                    icon: icons.then_some(obelus_icons::ui::AWAY),
                    chosen: false,
                },
                Choice {
                    id: "no".to_string(),
                    name: "no".to_string(),
                    about: None,
                    icon: icons.then_some(obelus_icons::ui::STAYING),
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
                talk.chat.note("Not opened");
            }
            let _ = going.answer.send(false);
            return;
        }
        if let Err(error) = obelus_clipboard::links::open(&going.url) {
            tracing::warn!(%error, "the link was not opened");
            // Not an answer: nothing was opened, so the reader has not been
            // sent anywhere. The card stays, with the URL still on it --
            // which on a machine with no browser is the only way they will
            // get it.
            if let Some(talk) = self.conversation_mut() {
                talk.chat.note("Nothing here opens links");
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
            self.in_transcript(|chat| chat.note(&format!("That is outside {asked}")));
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
            self.in_transcript(|chat| chat.note("It stopped waiting for an answer"));
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
            talk.chat.note("Not opened");
            return;
        }
        let Some(asking) = self.conversation_mut().and_then(|talk| talk.asking.take()) else {
            return;
        };
        if let Some(talk) = self.conversation_mut() {
            talk.chat.note("Not answered");
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
        // The width the rows are laid out at, and the band they are drawn
        // in, asked of the two functions the view asks -- not worked out
        // again here.
        //
        // This had its own arithmetic for both, and the width was one
        // column out. Wrapping at a column wider than the view's makes
        // fewer rows than the view then draws, so the window was told the
        // transcript was shorter than it is; following the end of a list
        // that is longer than you think leaves its last rows below the
        // band. The last row is the one that says whether anything is
        // happening at all -- so on a long conversation, and only on a
        // long one, an agent could work for half a minute with nothing on
        // screen saying so. A short one has nothing wrapped and the two
        // counts agree, which is why it took a real day's conversation to
        // show.
        //
        // And the band was worked out as though the foot of the region
        // were the box a message is written in. While a question is up it
        // is the card, which is taller.
        let region = obelus_ui::chat::bands(editor_area, &talk.chat, talk.card.as_ref()).transcript;
        let rows = talk
            .chat
            .rows(obelus_ui::chat::reading_width(editor_area))
            .len();
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

    /// Which conversation a message from the agent belongs in.
    ///
    /// The routing, in one place. What names a conversation goes in that
    /// one -- the document that opened the session, whether or not the
    /// reader is looking at it.
    ///
    /// It used to be the conversation *on screen* or nowhere, and that made
    /// walking away from a turn a way of throwing it out: every word, every
    /// call, every plan and every question the agent sent while the reader
    /// was in a file or in another conversation was dropped where it
    /// arrived, and what they came back to was a transcript with a hole in
    /// it where the turn had been. A conversation is a document, and a
    /// document does not stop existing when it stops being drawn.
    ///
    /// `None` for a session nothing open has, which is the one case with
    /// nowhere to put anything.
    ///
    /// The two that name none are the two the protocol does not put a
    /// session on: an elicitation, and a request for a file. Those go to
    /// whoever is here, which is right while one conversation is waiting on
    /// the agent and is a guess when two are. The protocol is where that has
    /// to be fixed, so this is where it is written down.
    fn whose(&self, incoming: &acp::Incoming) -> Option<Whose> {
        let named = match incoming {
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
            | acp::Incoming::Write { .. }
            // A command names no conversation: `CreateTerminalRequest` has
            // a session on it, but the four that follow have only the
            // command's own name, and obelus runs them for whoever asked.
            | acp::Incoming::Run { .. }
            | acp::Incoming::Wrote { .. }
            | acp::Incoming::Waited { .. }
            | acp::Incoming::Stop { .. }
            | acp::Incoming::Forget { .. } => None,
        };
        let Some(session) = named else {
            return Some(Whose::Whoever);
        };
        // The one that has it, or the one that asked for it by name and
        // has not been told yet. Both are the same conversation: a
        // conversation asks for a session it had before, and the moment
        // the agent answers it owns that name.
        //
        // The second is not a nicety. An agent replaying a conversation
        // sends the words as notifications while it is still answering
        // the request that asked for them, so they arrive *before* the
        // answer that says which conversation the name belongs to. On the
        // name alone there was nothing here that had it, and the whole
        // replay went on the floor -- a conversation taken up again with
        // every word of it dropped on the way in, which from a reader's
        // side is indistinguishable from not having been taken up at all.
        self.conversation_at(|talk| {
            talk.session.as_ref() == Some(session) || talk.asked_for.as_ref() == Some(session)
        })
        .map(|at| Whose::One(DocumentId::new(at)))
    }

    /// Where the first conversation this is true of sits among the
    /// documents.
    ///
    /// Which conversation asked for a session, which has none yet, which
    /// opened the one a message names: three questions that were three
    /// copies of the same walk with a different line in the middle.
    fn conversation_at(
        &self,
        is: impl Fn(&crate::conversation::Conversation) -> bool,
    ) -> Option<usize> {
        self.documents
            .iter()
            .position(|document| document.as_ref().and_then(Document::chat).is_some_and(&is))
    }

    /// Whether what `whose` names is what the reader is looking at.
    fn is_here(&self, whose: Whose) -> bool {
        match whose {
            Whose::Whoever => true,
            Whose::One(id) => self.current == Some(id),
        }
    }

    /// The conversation `whose` names, to change.
    fn talk_mut(&mut self, whose: Whose) -> Option<&mut crate::conversation::Conversation> {
        match whose {
            Whose::Whoever => self.conversation_mut(),
            Whose::One(id) => self.document_mut(id).and_then(Document::chat_mut),
        }
    }

    /// Writes in the transcript of the conversation `whose` names.
    ///
    /// The counterpart of [`Self::in_transcript`], which writes in the one
    /// on screen: what the agent sends belongs to the conversation it was
    /// sent about, and that is not always the one being read.
    fn in_talk(&mut self, whose: Whose, what: impl FnOnce(&mut obelus_component::chat::Chat)) {
        if let Some(talk) = self.talk_mut(whose) {
            what(&mut talk.chat);
        }
    }

    /// Whether any command obelus was asked to run is still going.
    #[must_use]
    pub fn anything_running(&self) -> bool {
        self.runs.anything_running()
    }

    /// Puts what obelus's commands are doing on the rows that are about
    /// them.
    ///
    /// Every frame, from the runner rather than from anything kept: the
    /// process owns its output, and a row drawn from a copy is a row that
    /// can be a moment behind what the reader is watching.
    ///
    /// This is the half obelus owes for not asking. The agent decides
    /// whether to ask before running something; obelus decides that once
    /// it runs, the reader sees the command in the words it was run in and
    /// everything it printed.
    pub(super) fn show_what_is_running(&mut self) {
        let runs = &mut self.runs;
        let mut said: Vec<(String, obelus_component::chat::Doing)> = Vec::new();
        for document in self.documents.iter().flatten() {
            let Some(talk) = Document::chat(document) else {
                continue;
            };
            for id in talk.chat.commands() {
                let Some(command) = runs.said(&id).map(str::to_string) else {
                    continue;
                };
                let (output, truncated, ended) = runs
                    .output(&id)
                    .unwrap_or_else(|| (String::new(), false, None));
                let mut words = format!("$ {command}");
                if !output.is_empty() {
                    words.push('\n');
                    words.push_str(output.trim_end());
                }
                if truncated {
                    words.push_str("\n\u{2026} and more, which obelus did not keep");
                }
                // How it ended, where that is not simply well. The mark on
                // the row says a command failed and cannot say what a
                // reader needs next, which is *how*: a `grep` that matched
                // nothing exits 1 and a command that is not installed
                // exits 127, and one of those is an answer and the other
                // is a morning wasted. The number was nowhere on the page
                // -- obelus kept it, told the agent when it asked, and
                // showed the reader a glyph.
                //
                // Last, under the output, because that is where the
                // command's own account ends.
                match ended {
                    Some(obelus_agent::running::Ended {
                        signal: Some(signal),
                        ..
                    }) => words.push_str(&format!("\n\u{2026} and was stopped by {signal}")),
                    Some(obelus_agent::running::Ended {
                        code: Some(code), ..
                    }) if code != 0 => {
                        words.push_str(&format!("\n\u{2026} and exited {code}"));
                    }
                    _ => {}
                }
                // The call's state, not a second mark beside it: a call
                // running a command that failed is a call that failed, and
                // a reader scanning a turn reads one glyph.
                let state = ended.map(|ended| match ended {
                    obelus_agent::running::Ended { code: Some(0), .. } => "completed".to_string(),
                    _ => "failed".to_string(),
                });
                said.push((id, obelus_component::chat::Doing { words, state }));
            }
        }
        if said.is_empty() {
            return;
        }
        let said: std::collections::HashMap<String, obelus_component::chat::Doing> =
            said.into_iter().collect();
        for document in self.documents.iter_mut().flatten() {
            let Some(talk) = Document::chat_mut(document) else {
                continue;
            };
            talk.chat.running(&|id| said.get(id).cloned());
        }
    }

    /// Answers whoever is waiting on a command that has ended.
    ///
    /// Once a frame, like the language servers' own check: a command ends
    /// when it ends, and nothing tells obelus but asking.
    pub(super) fn check_runs(&mut self) {
        if self.waiting_on.is_empty() {
            return;
        }
        let waited: Vec<String> = self.waiting_on.iter().map(|(id, _)| id.clone()).collect();
        for id in waited {
            if self.runs.ended(&id).is_some() {
                self.tell_whoever_waited(&id);
            }
        }
    }

    /// Tells everyone waiting on this command how it ended.
    ///
    /// Told rather than dropped. A channel that goes away is an error to
    /// whoever was listening, and "it ended" is not an error -- an agent
    /// given one for a command that finished would have to guess whether
    /// it ran at all.
    fn tell_whoever_waited(&mut self, id: &str) {
        let ended = self.runs.ended(id);
        let (theirs, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting_on)
            .into_iter()
            .partition(|(waited, _)| waited == id);
        self.waiting_on = rest;
        for (_, answer) in theirs {
            let _ = answer.send(ended);
        }
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
            let at = self.conversation_at(|talk| talk.asked_for.as_ref() == Some(session));
            // And the name goes with it. The agent has said it has no
            // such conversation, which is as certain as this gets, and a
            // name left in the file is one obelus asks for again on the
            // next start -- the same refusal, the same fresh start, every
            // morning, with the note's row saying all the while that
            // there is something to come back to.
            let note = match at.and_then(|at| self.documents.get(at)?.as_ref()?.chat()) {
                Some(talk) => match &talk.topic {
                    Topic::Note(note) => Some(note.clone()),
                    Topic::Loose => None,
                },
                None => None,
            };
            if let Some(talk) = at
                .and_then(|at| self.documents.get_mut(at))
                .and_then(Option::as_mut)
                .and_then(Document::chat_mut)
            {
                talk.asked_for = None;
                // A fresh session has heard none of it, whichever of the
                // two it is: both go back to "not said yet" together, or
                // the half that is left behind is the half never said.
                talk.told = None;
                talk.introduced = false;
                talk.chat.note(&format!("Starting again, because {why}"));
            }
            if let Some(note) = note {
                self.forget_the_conversation(&note);
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
                .conversation_at(|talk| talk.asked_for.as_ref() == Some(&session))
                .or_else(|| {
                    self.conversation_at(|talk| talk.session.is_none() && talk.asked_for.is_none())
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
        // And everything else goes to the conversation it names, which is
        // a document rather than whatever is on screen: a reader who walks
        // off mid-turn comes back to the whole of it.
        let Some(whose) = self.whose(&incoming) else {
            return;
        };
        match incoming {
            acp::Incoming::Update { update, .. } => match update {
                acp::Update::Said(text) => {
                    self.in_talk(whose, |chat| chat.chunk(Speaker::Agent, &text))
                }
                acp::Update::Thought(text) => {
                    self.in_talk(whose, |chat| chat.chunk(Speaker::Thought, &text))
                }
                // The reader's own words, as the agent has them. What this
                // is for is a conversation taken up again after obelus was
                // shut: the transcript is the agent's, and this is the half
                // of it obelus cannot write itself.
                acp::Update::Heard(text) => self.in_talk(whose, |chat| chat.heard(&text)),
                acp::Update::Tool { call, status } => {
                    self.in_talk(whose, |chat| chat.tool(&call, &status))
                }
                // What it means to do about this turn. Not a thing said --
                // it never goes in the transcript -- so it is handed to the
                // row that says what is happening now, which is where a
                // state belongs and where one cannot be left behind.
                acp::Update::Plan(steps) => self.in_talk(whose, |chat| chat.planning(steps)),
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
                    "cancelled" => self.in_talk(whose, |chat| chat.note("Stopped")),
                    "refusal" => self.in_talk(whose, |chat| chat.note("It declined to answer")),
                    "max_tokens" => {
                        self.in_talk(whose, |chat| chat.note("It ran out of room to answer in"));
                    }
                    other => self.in_talk(whose, |chat| chat.note(other)),
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
                self.in_talk(whose, |chat| {
                    chat.note(
                        "Taken up where you left it; this agent cannot send back what was said",
                    );
                });
            }
            acp::Incoming::Permission {
                call,
                reason,
                options,
                answer,
                ..
            } => self.ask_permission(whose, &call, reason.as_deref(), &options, answer),
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
            // A command the agent asked for. Run without asking the
            // reader -- the agent asks, which is the rule obelus's own
            // tools follow too -- and put on the page while it runs.
            acp::Incoming::Run {
                command,
                args,
                env,
                cwd,
                limit,
                answer,
            } => {
                let root = self.working_directory.clone();
                let started = self
                    .runs
                    .start(&command, &args, &env, cwd.as_deref(), &root, limit);
                let id = match started {
                    Ok(id) => Some(id),
                    Err(error) => {
                        tracing::warn!(%error, %command, "a command would not start");
                        None
                    }
                };
                let _ = answer.send(id);
            }
            acp::Incoming::Wrote { id, answer } => {
                let _ = answer.send(self.runs.output(&id));
            }
            // Kept rather than answered: the command has not ended, and
            // the loop that draws cannot wait for one that takes minutes.
            // `check_runs` answers it when it does.
            acp::Incoming::Waited { id, answer } => match self.runs.ended(&id) {
                Some(ended) => {
                    let _ = answer.send(Some(ended));
                }
                None if self.runs.said(&id).is_some() => self.waiting_on.push((id, answer)),
                None => {
                    let _ = answer.send(None);
                }
            },
            acp::Incoming::Stop { id, answer } => {
                self.runs.stop(&id);
                self.tell_whoever_waited(&id);
                let _ = answer.send(());
            }
            acp::Incoming::Forget { id, answer } => {
                self.runs.stop(&id);
                // Told before it is forgotten, or a waiter is left holding
                // a channel about a command nothing knows any more.
                self.tell_whoever_waited(&id);
                self.runs.release(&id);
                let _ = answer.send(());
            }
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
                    Some(why) => self.in_talk(whose, |chat| {
                        chat.note(&format!("The agent stopped: {why}"))
                    }),
                    None => self.in_talk(whose, |chat| chat.note("The agent stopped")),
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
                chat.note("This system has nowhere for obelus to keep an agent")
            });
            return;
        };
        // What the install wrote down when it finished. Nothing here means
        // no install finished -- the reader removed it, or obelus was shut
        // while one was running -- and the agents page is where that is
        // fixed, so that is where they are sent.
        let Some(installed) = obelus_agent::installation(&id, &root) else {
            tracing::warn!(
                id,
                "No agent to talk to: nothing is installed under that name"
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
        whose: Whose,
        call: &acp::Call,
        reason: Option<&str>,
        options: &[acp::Choice],
        answer: acp::Answer<Option<String>>,
    ) {
        self.show_the_question(whose);
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
        self.in_talk(whose, |chat| chat.tool(call, "pending"));
        let choices = options
            .iter()
            .map(|choice| Choice {
                id: choice.id.clone(),
                name: choice.name.clone(),
                about: None,
                icon: obelus_icons::enabled().then(|| obelus_icons::for_permission(&choice.kind)),
                chosen: false,
            })
            .collect();
        let mut card = Card::new(choices, false);
        card.needs_one();
        // What it is actually about to do, above the answers: "allow" and
        // "refuse" are answers to a question, and the question is which
        // command on which file rather than the line the title fits in.
        //
        // Nothing where the row above is carrying the question itself, for
        // the reason the line obelus used to write here was deleted: those
        // words are on the row, whole and foldable and still there after
        // the answer, and they are the reader's to scroll rather than the
        // card's to quote. A card is five rows tall, so a copy here is the
        // first fifth of something the reader can already see all of --
        // and for a plan, which is what the longest of these are, the
        // first fifth is the heading. It costs the transcript the rows it
        // takes, too, which for a plan is the rows the last step was on.
        //
        // And otherwise the reason, or the call's title where there is no
        // reason, which is the least this can say and still have asked
        // something. Nothing at all was the rule for that case as well,
        // on the grounds that what it is asking about is the row above the
        // card -- but the row is at the head of the region and the card is
        // at the foot of it, with the rest of the turn and an empty
        // half-screen in between. What that left on screen was a Yes and a
        // No with no subject: a reader who looked away came back to two
        // words and nothing saying what they answered.
        //
        // "Already said it" is words of the call's own, not merely words:
        // an agent may send the title back as the call's content, which is
        // the row saying the one line twice and the card still saying
        // nothing. The row reads it by the same rule, from the same place,
        // because the two drifting apart is a card with no subject.
        let said_its_own = obelus_component::chat::its_own_words(&call.title, &call.said)
            .next()
            .is_some();
        let about = reason
            .filter(|reason| !reason.trim().is_empty())
            .unwrap_or(call.title.as_str());
        if !said_its_own && !about.trim().is_empty() {
            card.about(about);
        }
        if let Some(talk) = self.talk_mut(whose) {
            talk.permission = Some(answer);
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
            self.in_transcript(|chat| chat.note("It stopped waiting for an answer"));
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
            talk.chat.note("Not answered");
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

    /// The tree, spelled the way `canonicalize` spells a path.
    ///
    /// Both sides of the fence have to be spelled the same way or it is not
    /// a comparison. On Windows `canonicalize` hands back a verbatim path --
    /// `\\?\E:\work\obelus\...` -- where the working directory is an
    /// ordinary one, so `starts_with` was asking whether a `\\?\E:` prefix
    /// begins with an `E:` one. It does not, ever: every file an agent asked
    /// to read was outside the tree, the tree's own included, and what the
    /// reader saw was an agent reading nothing and being refused everything.
    ///
    /// The working directory unresolved where it will not resolve, which is
    /// what the comparison had before and is never worse than it.
    fn fenced(&self) -> PathBuf {
        self.working_directory
            .canonicalize()
            .unwrap_or_else(|_| self.working_directory.clone())
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
            .is_ok_and(|full| full.starts_with(self.fenced()));
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
                        buffer.edit(span, text, obelus_buffer::undo::Doing::Whole)
                    })
                });
                if changed {
                    self.change_document(index);
                    self.note = Some("The agent changed this file".to_string());
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
            .is_some_and(|full| full.starts_with(self.fenced()));
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
