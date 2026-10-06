//! Talking to the active agent.
//!
//! One agent at a time, started when the reader first opens a conversation
//! and left running until they close Obelus or choose another. A
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
    composer::Part,
};

use super::*;
use crate::conversation::{Asking, Permission, Topic};

/// The answer on a sign-in's card that is not a way in.
///
/// Not a word an agent would use for one of its own, which is what its
/// ways in are called by on the same card.
const NOT_SIGNING_IN: &str = "obelus:not-now";

/// Which conversation a message from the agent is for.
///
/// Its own type rather than an `Option<DocumentId>` because the two cases
/// are not "one or none": a message about a session belongs to the document
/// that opened it and nowhere else, and a message the protocol puts no
/// session on belongs to whoever the reader is with. An `Option` would have
/// read the second as "no conversation".
#[derive(Clone, Copy, Debug)]
pub(super) enum Whose {
    /// The conversation that opened the session it names.
    One(DocumentId),
    /// Whichever one the reader is in, for the two the protocol names no
    /// session on: an elicitation, and somewhere to go.
    Whoever,
}

impl App {
    /// Goes to a new conversation about nothing in particular.
    ///
    /// A document, so this switches to it the way any key that opens a file
    /// does: the list keeps it, closing it is the key that closes anything,
    /// and what was said is still there when the reader comes back. It was a
    /// flag over the editor, which is why escape used to close it and why
    /// there was a second question -- "is it showing" -- beside the
    /// conversation that answers it.
    ///
    /// One already open with nothing said in it is that new conversation,
    /// and is gone back to rather than joined by a second: a reader who
    /// asks twice would otherwise collect empty pages in the list of what
    /// is open, one per asking.
    ///
    /// The session is not asked for here: the next frame asks for one for
    /// whichever conversation is on screen -- see
    /// [`App::settle_the_sessions`] -- because this is one of half a dozen
    /// ways onto one.
    pub fn new_conversation(&mut self) {
        // Whatever the reader had over the file is not what they asked for.
        self.make_room(Room::Region);
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(crate::conversation::Conversation::is_blank)
        });
        let at = at.unwrap_or_else(|| {
            self.documents
                .push(Some(crate::conversation::Conversation::default().into()));
            self.documents.len() - 1
        });
        self.go_to_document(DocumentId::new(at));
    }

    /// Starts the agent and opens a conversation on it, with nothing said.
    ///
    /// What the next frame does anyway, now rather than then: a test about
    /// what happens inside a running conversation asks for its session
    /// before it has drawn anything. The tests about the opening itself
    /// draw the frame instead, and do not use this.
    pub fn open_a_session_for_test(&mut self) {
        if self.talker.is_none() {
            self.start_agent();
        }
        // The same two lines the reader's first message runs, so that a
        // conversation about a note is taken up rather than replaced.
        let note = match self.conversation().map(|talk| talk.topic.clone()) {
            Some(Topic::Note(note)) => Some(note),
            Some(Topic::Loose) | None => None,
        };
        let had = note.as_ref().and_then(|note| self.remembered_session(note));
        self.ask_for_a_session(Whose::Whoever, had);
    }

    /// Whether the running agent holds a conversation by this name, for a
    /// test about one Obelus should have let go.
    #[must_use]
    pub fn agent_holds_for_test(&self, session: &str) -> bool {
        self.talker
            .as_ref()
            .is_some_and(|talker| talker.holds(&acp::SessionId::new(session)))
    }

    /// Which conversation the one on screen is, by the agent's name for it.
    ///
    /// The one it asked for where an answer has not arrived, because those
    /// are two different things from "no session at all" and a test about
    /// which one was asked for by name cannot wait for the agent.
    #[must_use]
    pub fn chat_session_for_test(&self) -> Option<String> {
        let talk = self.conversation()?;
        talk.session
            .as_ref()
            .or(talk.asked_for.as_ref())
            .map(|id| id.0.to_string())
    }

    /// How many documents are open, for a test about one being opened twice.
    #[must_use]
    pub fn document_count_for_test(&self) -> usize {
        self.documents.iter().flatten().count()
    }

    /// Lets every conversation go, because the agent behind them has.
    ///
    /// A session is a name one agent gave to something, and an agent that
    /// has been stopped -- or swapped for another -- can never be asked
    /// about it again. What is left is the transcript, which is the
    /// reader's, and a conversation that asks the next agent for a session
    /// of its own the moment it is used.
    ///
    /// What each was told goes too: the next agent has been told nothing,
    /// so it is told everything the first time the reader says something.
    /// One repeated telling on the way past, rather than a silence that
    /// lasts.
    pub(super) fn let_the_conversations_go(&mut self) {
        for document in &mut self.documents {
            let Some(talk) = document.as_mut().and_then(Document::chat_mut) else {
                continue;
            };
            let had = talk.session.take().is_some()
                || talk.asked_for.take().is_some()
                || std::mem::take(&mut talk.opening);
            talk.requested = None;
            talk.minted = false;
            // Another agent, and what it offers is its own to say.
            talk.said_not_offered.clear();
            // And asked again, the conversation on screen included: what
            // it had asked this showing it asked of an agent that has
            // gone, and the one there now has not been asked anything --
            // so its settings and its `/` list were empty until the reader
            // typed.
            talk.asked_while_shown = false;
            talk.told = None;
            talk.started_on.clear();
            if had {
                talk.chat
                    .note("The agent this was with is no longer the one Obelus talks to");
            }
        }
    }

    /// Asks the active agent what it can be set to, for the settings page.
    ///
    /// On a conversation of its own, opened and let go on the other side
    /// of the connection: the reader's conversations are theirs, and one
    /// of them existing -- or being named, or being written in -- because
    /// a settings page wanted a list is a conversation that goes wherever
    /// that list goes.
    ///
    /// Every time the page opens, because what an agent offers is a fact
    /// about the agent as it is now and an update changes it. What was
    /// heard last is drawn meanwhile -- see `Agents::offers` -- so it is
    /// only the first opening that waits.
    ///
    /// Only an agent that is installed: one that is not cannot be started,
    /// and starting it would say so into whatever conversation happens to
    /// be behind the page.
    pub(super) fn ask_what_the_agent_offers(&mut self) {
        let Some(id) = self
            .settled
            .config
            .agent
            .clone()
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        if !self.the_chosen_agent_is_installed() {
            return;
        }
        if self
            .talker
            .as_ref()
            .is_none_or(obelus_agent::acp::Talk::has_exited)
        {
            self.stop_agent();
            self.start_agent();
        }
        let Some(talker) = self.talker.as_mut().filter(|talker| talker.id() == id) else {
            return;
        };
        talker.offers();
        self.agents.asking = Some(id);
    }

    /// Whether the agent the settings name has an install to start.
    fn the_chosen_agent_is_installed(&self) -> bool {
        self.settled
            .config
            .agent
            .as_deref()
            .filter(|id| !id.is_empty())
            .is_some_and(|id| self.installed(id).is_some())
    }

    /// The same, of whichever agent is running, for a test that started
    /// one with `talk_to` rather than choosing and installing it.
    pub fn ask_what_the_agent_offers_for_test(&mut self) {
        if let Some(talker) = self.talker.as_mut() {
            talker.offers();
            self.agents.asking = Some(talker.id().to_string());
        }
    }

    /// Goes to the conversation about one note, opening one if there is
    /// none.
    ///
    /// By the note's name rather than its place in the list, which is what
    /// the name is for: the list is read from the file every time it opens,
    /// and a note added above would otherwise hand the reader somebody
    /// else's conversation.
    /// The session is the next frame's to ask for, as it is for
    /// [`App::new_conversation`]. The claim is taken here, because the claim is
    /// not about the agent -- it is this window saying the note's
    /// conversation is its own, and it has to be said before another window
    /// says it.
    pub(super) fn talk_about(&mut self, note: &obelus_git::todo::NoteId) {
        self.make_room(Room::Region);
        let wanted = Topic::Note(note.clone());
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == wanted)
        });
        let at = match at {
            Some(at) => at,
            None => {
                // Claimed before it is opened, and the claim is what
                // decides: asking first and opening after would be two
                // windows both finding it free in the same moment. Where
                // another Obelus has it, nothing opens -- the row in the
                // list already says so, and this is the reader pressing
                // the key on it anyway.
                let which = obelus_agent::chats::ChatId::Note(note.clone());
                let Some(claim) = obelus_agent::chats::claim(&self.working_directory, &which)
                else {
                    // Nothing said, because the row already says it: the
                    // lock beside it, and the foot with no `Talk` on it
                    // while the reader is standing there. A note here
                    // would be a second answer to a question the page has
                    // answered.
                    return;
                };
                // What the note says, for the box: read as this opens,
                // for the reason the notes page reads what it is drawn
                // from as *it* opens. The watch that keeps it level is
                // settled on the next frame.
                self.reread_the_notes_kept();
                let (told, introduced) = self.remembered_telling(note);
                let talk = crate::conversation::Conversation {
                    told,
                    introduced,
                    topic: wanted,
                    claim: Some(claim),
                    ..crate::conversation::Conversation::default()
                };
                self.documents.push(Some(talk.into()));
                self.documents.len() - 1
            }
        };
        self.go_to_document(DocumentId::new(at));
    }

    /// Asks for a session for the conversation on screen, and lets go of the
    /// ones the reader has left without saying anything in.
    ///
    /// Asked from what is showing once a frame, rather than wherever a
    /// conversation is opened or left, for the ticker's reason: there are
    /// half a dozen ways into a conversation and more ways out of one --
    /// another file, another conversation from the list, the notes, the
    /// file being closed -- and a rule kept at each of them is a rule the
    /// next way forgets.
    ///
    /// A conversation opens on a session so that what the agent offers is
    /// there to see and choose from before the first word, the way it is
    /// in any conversation that has had one: the settings on the row, and
    /// what it takes with a slash. Those come with a session and nowhere
    /// else, so a conversation that waited for the first message to ask
    /// for one had a blank row and an empty `/` list at exactly the moment
    /// a reader looks at them, before they decide what to say. zed opens
    /// one with the view for this reason.
    ///
    /// Opening is not binding, which is what went wrong when this was
    /// tried before: a key pressed to see what was said yesterday left
    /// behind an empty conversation that Obelus could write down against
    /// the note in place of the one the reader had been talking in. So
    /// nothing writes it down against a note until something has been said
    /// in it -- see `remember_the_conversations` -- and a session minted for
    /// a view the reader then left is let go, on the agent's side as well
    /// as this one's, so that a note opened by mistake is a note with
    /// nothing under it.
    pub(super) fn settle_the_sessions(&mut self) {
        let showing = self.current.map(DocumentId::get);
        self.let_go_of_what_nothing_was_said_in(showing);
        let Some(at) = showing else {
            return;
        };
        let wants = self
            .documents
            .get(at)
            .and_then(Option::as_ref)
            .and_then(Document::chat)
            .is_some_and(|talk| {
                talk.session.is_none()
                    && talk.asked_for.is_none()
                    && !talk.opening
                    && !talk.asked_while_shown
            });
        if !wants || self.talking() == Talking::Nobody {
            return;
        }
        let whose = Whose::One(DocumentId::new(at));
        if let Some(talk) = self.talk_mut(whose) {
            talk.asked_while_shown = true;
        }
        // Started where it is not running, and again where it has stopped:
        // a conversation the reader has come to is one they are about to
        // talk in, which is what starting it again has always been for.
        //
        // Only one that is installed. Starting one that is not says so into
        // the conversation, which is right when the reader has said
        // something -- `say_in` does -- and is a line more on every visit
        // when all they have done is look.
        if self
            .talker
            .as_ref()
            .is_none_or(obelus_agent::acp::Talk::has_exited)
        {
            if !self.the_chosen_agent_is_installed() {
                return;
            }
            self.stop_agent();
            self.start_agent();
        }
        let note = match self.talk(whose).map(|talk| &talk.topic) {
            Some(Topic::Note(note)) => Some(note.clone()),
            Some(Topic::Loose) | None => None,
        };
        // Taken rather than read: asked for once, the name is on its way,
        // and a conversation that later loses its session -- the agent
        // stopping, another agent chosen -- gets a new one, the way any
        // conversation about nothing in particular does. By then its claim
        // has gone with the session, and another window may have taken
        // the old one up.
        let to_take_up = self.talk_mut(whose).and_then(|talk| talk.to_take_up.take());
        let had = note
            .as_ref()
            .and_then(|note| self.remembered_session(note))
            .or(to_take_up);
        self.ask_for_a_session(whose, had);
    }

    /// Lets go of every session opened here that nothing was said in,
    /// except the one on screen, which `keep` names.
    ///
    /// Only a session minted for a conversation. One taken up again is one
    /// the reader had, and can come back with an empty page -- an agent that
    /// resumes rather than replays sends none of it back -- which looks
    /// exactly like a conversation nothing was said in.
    ///
    /// The agent is told, and the conversation stays, with nothing on the
    /// way: coming back to it asks for another.
    pub(super) fn let_go_of_what_nothing_was_said_in(&mut self, keep: Option<usize>) {
        let mut going = Vec::new();
        for (at, document) in self.documents.iter_mut().enumerate() {
            if Some(at) == keep {
                continue;
            }
            let Some(talk) = document.as_mut().and_then(Document::chat_mut) else {
                continue;
            };
            talk.asked_while_shown = false;
            if !talk.minted || talk.chat.anything_said() {
                continue;
            }
            let Some(session) = talk.session.take() else {
                continue;
            };
            talk.minted = false;
            talk.started_on.clear();
            // A conversation about nothing in particular is claimed by its
            // session's name, which has just gone. One about a note is
            // claimed by the note, and the note's view is still open.
            if talk.topic == Topic::Loose {
                talk.claim = None;
            }
            going.push(session);
        }
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        for session in going {
            tracing::info!(session = %session.0, "letting go of a conversation nothing was said in");
            talker.let_go(&session);
        }
    }

    /// Forgets every session and every request for one, because the agent
    /// that gave them has gone.
    ///
    /// Not [`App::let_the_conversations_go`], which is about another agent
    /// and says so in each: this is the same one, started again when a
    /// conversation is next shown or spoken in, and a conversation about a
    /// note takes its own up again by the name written down.
    fn forget_what_the_agent_held(&mut self) {
        for document in &mut self.documents {
            let Some(talk) = document.as_mut().and_then(Document::chat_mut) else {
                continue;
            };
            talk.session = None;
            talk.asked_for = None;
            talk.opening = false;
            talk.requested = None;
            talk.minted = false;
            talk.started_on.clear();
            // Claimed by the session's name, which has gone; a note's is
            // claimed by the note. And a new one is what a conversation
            // about nothing in particular gets next -- it has no note to
            // find the old one by -- and a new one has been told nothing,
            // so what was told goes too, or its first message goes out
            // without saying who it is talking to.
            if talk.topic == Topic::Loose {
                talk.claim = None;
                talk.told = None;
                talk.introduced = false;
            }
        }
    }

    /// Asks the agent for the conversation `whose` names.
    ///
    /// `had` is the one it wants taken up again, where Obelus wrote a name
    /// down, and nothing where it wants a fresh one. The caller looks that
    /// up rather than this: a conversation about a note finds it by the
    /// note, and one taken up from the list of conversations was chosen by
    /// name, so there is no one question to ask here on their behalf.
    ///
    /// The conversation it is about is named rather than taken to be the
    /// one on screen. Said in a turn that ended while the reader was
    /// reading something else, "the one on screen" is somebody else's
    /// conversation.
    ///
    /// The one place a session is asked for, and nothing is opened behind
    /// a conversation's back. The connection used to mint one the moment
    /// it came up -- and a view that opens on a note already naming a
    /// conversation then had a second, empty one to go with it, which the
    /// agent does not keep and Obelus could still write down against the
    /// note in place of the one the reader had been talking in.
    ///
    /// The one it had before where Obelus wrote the name down: the agent
    /// kept every word of it, which is why Obelus keeps none. A new one
    /// otherwise -- one agent, several conversations, because an agent
    /// holds a project's worth of context and a second process would pay
    /// for all of it twice.
    ///
    /// Nothing at all where this conversation has a session, or has asked
    /// for one and not been answered yet. The key that opens a
    /// conversation is a key a reader can press twice, and an agent takes
    /// a moment to answer: without the second half of that, the second
    /// press opened a conversation the first press was already opening.
    pub(super) fn ask_for_a_session(&mut self, whose: Whose, had: Option<String>) {
        let settled = self
            .talk(whose)
            .is_some_and(|talk| talk.session.is_some() || talk.asked_for.is_some() || talk.opening);
        if settled {
            return;
        }
        // Asked, whichever way it was asked -- the reader's first words, a
        // test, the frame -- so the frame does not ask again this showing.
        // Which matters most when the agent dies under the conversation on
        // screen: its session goes, and the frame after would start the
        // agent again behind the reader's back, where what the page says is
        // that it stopped and that talking starts it.
        if let Some(talk) = self.talk_mut(whose) {
            talk.asked_while_shown = true;
        }
        let tools = self.tools_for(whose);
        // By the session rather than by the note, because a conversation
        // about no note is taken up too, and it is named by nothing else.
        let title = had.as_ref().and_then(|session| {
            let agent = self.talker.as_ref()?.id();
            self.sessions()?
                .all()
                .find(|(_, by, tree, kept)| {
                    *by == agent && *tree == self.working_directory && kept.session == *session
                })
                .and_then(|(_, _, _, kept)| kept.title.clone())
        });
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        match had {
            Some(session) => {
                let asking = talker.reopen(&session, title, tools);
                if let Some(talk) = self.talk_mut(whose) {
                    talk.asked_for = Some(obelus_agent::acp::SessionId::new(session));
                    talk.requested = Some(asking);
                }
            }
            None => {
                let asking = talker.open(tools);
                if let Some(talk) = self.talk_mut(whose) {
                    talk.opening = true;
                    talk.requested = Some(asking);
                }
            }
        }
    }

    /// Where the conversation `whose` names reaches Obelus's own tools.
    ///
    /// Its own address rather than the server's, and asked every time a
    /// session is, because the server's port is this process's: a
    /// conversation taken up again is told where it is now. The number is
    /// the document's, which names this conversation and nothing else for
    /// as long as the process runs -- a closed slot is never filled again.
    fn tools_for(&self, whose: Whose) -> Option<String> {
        let id = match whose {
            Whose::One(id) => id,
            Whose::Whoever => self.current?,
        };
        Some(obelus_mcp::address(self.tools_url.as_deref()?, id.get()))
    }

    /// The project's table of conversations, as Obelus last read it.
    ///
    /// Read when there is a reason and kept until there is another, which
    /// is what lets a view ask it: the reasons are the notes opening, the
    /// watcher saying somebody wrote that file, and Obelus writing it
    /// itself -- and the last of those hands the new table straight back,
    /// so nothing re-reads what it has just written.
    ///
    /// Empty where it has never been read, which is the honest answer for
    /// a reader who has not opened anything that asks.
    ///
    /// Borrowed rather than handed over: the notes page asks once a frame
    /// and a copy of the whole table per frame is the cost this was written
    /// to get rid of, in smaller print.
    #[must_use]
    pub fn sessions(&self) -> Option<&obelus_agent::acp::sessions::Remembered> {
        self.sessions_kept.as_ref()
    }

    /// Reads that table again, because there is a reason to.
    pub(super) fn reread_the_sessions(&mut self) {
        self.sessions_kept = obelus_agent::acp::sessions::read(
            &self.working_directory,
            self.config().conversation_days,
        )
        .remembered();
    }

    /// Whether a path that changed is that table.
    ///
    /// The notes' own file has the same question beside it, for the same
    /// reason: both are files another window writes and this one has to
    /// hear about.
    pub(super) fn is_the_sessions_file(&self, path: &std::path::Path) -> bool {
        obelus_agent::acp::sessions::path(&self.working_directory)
            .is_some_and(|table| path == table)
    }

    /// The conversation Obelus had about this note with the agent that is
    /// running, if it wrote one down.
    fn remembered_session(&self, note: &obelus_git::todo::NoteId) -> Option<String> {
        let agent = self.talker.as_ref()?.id();
        // Looking one up, so remembering none is an answer this can live
        // with: the cost of it is the conversation being started again.
        Some(
            self.sessions()?
                .get(
                    &obelus_agent::chats::ChatId::Note(note.clone()),
                    agent,
                    &self.working_directory,
                )?
                .session
                .clone(),
        )
    }

    /// What this agent has already been told, in a conversation about this
    /// note that Obelus wrote down.
    ///
    /// Both halves together, because they are read at one moment for one
    /// purpose -- filling in a conversation that is being picked up where
    /// it was left -- and asking the file twice for two fields of one row
    /// is two answers that can disagree.
    pub(super) fn remembered_telling(
        &self,
        note: &obelus_git::todo::NoteId,
    ) -> (Option<String>, bool) {
        let Some(agent) = self.talker.as_ref().map(obelus_agent::acp::Talk::id) else {
            return (None, false);
        };
        self.sessions()
            .and_then(|kept| {
                kept.get(
                    &obelus_agent::chats::ChatId::Note(note.clone()),
                    agent,
                    &self.working_directory,
                )
            })
            .map_or((None, false), |kept| (kept.told.clone(), kept.introduced))
    }

    /// Whether another Obelus has this note's conversation open.
    ///
    /// The one answer, because four things ask it and they must not
    /// disagree: the lock drawn beside the note, the keys that will not
    /// change it, and the refusal an agent's tool is given. A row drawing a
    /// lock and taking a letter all the same is the same shape as the card
    /// whose `submit` row stopped saying the keys were on it.
    #[must_use]
    pub(super) fn the_conversation_is_elsewhere(&self, note: &obelus_git::todo::NoteId) -> bool {
        // This Obelus's own claim is a lock like anybody's, so the
        // conversations it is holding are what tell the two apart.
        let mine = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| matches!(&talk.topic, Topic::Note(id) if id == note));
        !mine
            && self
                .held_now()
                .contains_key(&obelus_agent::chats::ChatId::Note(note.clone()))
    }

    /// Every note of this project whose conversation another Obelus has.
    ///
    /// What the page is told before it takes a key, so that the keys which
    /// would change one can refuse. By name, like everything else this page
    /// keeps about notes: the file is another window's to change, and a set
    /// of positions belongs to whichever order the notes were in when it
    /// was made.
    ///
    /// Asked of the claims this Obelus last looked at rather than of the
    /// disk. Looking means opening a claim for writing, which is the very
    /// event a watcher reports -- so a key that asked would wake every
    /// Obelus on the project, and a letter held down on a locked note would
    /// wake them at the rate the keyboard repeats. What a stale lock costs
    /// is what it already cost the drawing, and the way out is the key the
    /// lock is about: `alt+a` asks for the claim outright, and a lock
    /// nobody holds gives way to it.
    #[must_use]
    /// And who has each: a checkout by its directory's name where the claim
    /// names one that is not this, and another window otherwise -- one in
    /// this same checkout, or one whose claim has not said yet.
    pub(super) fn which_notes_are_elsewhere(
        &self,
    ) -> std::collections::HashMap<obelus_git::todo::NoteId, obelus_component::todo::Holder> {
        use obelus_component::todo::Holder;

        let Some(notes) = self.notes() else {
            return std::collections::HashMap::new();
        };
        notes
            .todo()
            .notes
            .iter()
            .filter(|note| self.the_conversation_is_elsewhere(&note.id))
            .map(|note| {
                let tree = self
                    .held_now()
                    .get(&obelus_agent::chats::ChatId::Note(note.id.clone()))
                    .cloned()
                    .flatten()
                    .filter(|tree| *tree != self.working_directory);
                let holder = match tree.as_deref().and_then(std::path::Path::file_name) {
                    Some(name) => Holder::Checkout(name.to_string_lossy().into_owned()),
                    None => Holder::AnotherWindow,
                };
                (note.id.clone(), holder)
            })
            .collect()
    }

    /// Whether each note has a conversation about it, in the order the
    /// notes are in -- which is what a row of the notes names.
    ///
    /// Two places count. A conversation open right now is one, and a name
    /// written down against the note is the other: Obelus keeps those so
    /// that a note talked over yesterday can be taken up again, and a list
    /// that only knew about open ones would say "nobody has talked about
    /// this" to a reader whose agent still holds every word of it.
    ///
    /// Against the agent the reader is set up to talk to rather than any
    /// agent that ever was. A conversation held with one they have since
    /// switched away from is not one they can reach, and saying there is
    /// one would send them to a note that opens an empty page.
    ///
    /// Two questions of two different kinds, and only one of them is asked
    /// of the disk here.
    ///
    /// *Which note has a conversation* is a table Obelus wrote, and a write
    /// to a file is something a watcher hears -- so it is read when there
    /// is a reason to and kept until there is another ([`App::sessions`]).
    /// It used to be parsed on every frame the notes were showing, which
    /// is 37us for one conversation and 351us for twenty -- and the notes
    /// turn a mark while any agent is at work, so that was twelve times a
    /// second for a page nobody was typing on.
    ///
    /// *Which of them somebody else has open* is the same shape and took
    /// longer to see. A claim is a lock: taking one writes nothing, and an
    /// Obelus that is killed gives its lock up with nothing on disk to say
    /// so -- which read as "nothing can tell you, so keep asking", and it
    /// was a walk of the claims on every frame. What tells you is the
    /// kernel closing that process's files, which a watcher reports as a
    /// close by a writer. So this is kept too, and looked at again when a
    /// claim appears, goes, or is closed by whoever was holding it.
    #[must_use]
    pub fn talked_about(&self) -> Vec<obelus_component::todo::Talked> {
        use obelus_component::todo::Talked;

        let Some(notes) = self.notes() else {
            return Vec::new();
        };
        let kept = self.sessions();
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

                // Somebody else's window has it. Ahead of everything
                // below, because those are all this Obelus's account of a
                // conversation it is in and this is the one case where it
                // is in no position to give one: what the agent is doing
                // in there is being told to the Obelus that asked.
                if self.the_conversation_is_elsewhere(&note.id) {
                    return Talked::Elsewhere;
                }
                if open.is_some_and(|talk| talk.card.is_some()) {
                    return Talked::Waiting;
                }
                // And an agent at work in it. Asked of the talker rather
                // than of the conversation, because thinking is the
                // agent's state and not the page's -- the same question
                // the list of open documents asks about the same
                // conversation.
                if open.is_some_and(|talk| {
                    self.talker.as_ref().is_some_and(|talker| {
                        talker.is_thinking(talk.session.as_ref(), talk.requested)
                    })
                }) {
                    return Talked::Working;
                }
                let written = !agent.is_empty()
                    && kept.is_some_and(|kept| {
                        kept.get(
                            &obelus_agent::chats::ChatId::Note(note.id.clone()),
                            &agent,
                            &self.working_directory,
                        )
                        .is_some()
                    });
                match open.is_some() || written {
                    true => Talked::Yes,
                    false => Talked::Not,
                }
            })
            .collect()
    }

    /// Forgets the conversation written down against one note.
    ///
    /// For the one case where Obelus knows there is nothing to come back
    /// to: the agent was asked for it by name and said it has no such
    /// thing. Left in the file, that name is asked for again on the next
    /// start and refused again, and the note goes on saying there is a
    /// conversation in it.
    fn forget_the_conversation(&mut self, note: &obelus_git::todo::NoteId) {
        let Some(agent) = self.talker.as_ref().map(|talker| talker.id().to_string()) else {
            return;
        };
        let here = self.working_directory.clone();
        self.change_the_sessions(|kept| {
            kept.forget(
                &obelus_agent::chats::ChatId::Note(note.clone()),
                &agent,
                &here,
            );
        });
    }

    /// Reads which conversation is which, changes it, and writes it back.
    ///
    /// The one way Obelus writes that table, so that what goes with every
    /// write goes with every write: the notes swept against, and what has
    /// not been talked in for longer than the reader keeps it.
    fn change_the_sessions(
        &mut self,
        what: impl FnOnce(&mut obelus_agent::acp::sessions::Remembered),
    ) {
        // The notes as the file has them, so that anything about a note
        // somebody has taken away goes at the same time. A note can go
        // without Obelus watching, so the collecting is done on the way past
        // rather than when one is deleted.
        // `None` where the file will not read, so that nothing is swept
        // against a list Obelus does not have: what is remembered here is
        // keyed to notes, and an empty list of names would forget every
        // conversation this project has.
        let notes: Option<Vec<obelus_git::todo::NoteId>> =
            obelus_git::todo::read(&self.working_directory)
                .notes()
                .map(|todo| todo.notes.into_iter().map(|note| note.id).collect());
        // Kept, rather than read back: what `change` hands over is the
        // table it has just written, and reading the file again for it
        // would be paying the dear half of this twice.
        let written = obelus_agent::acp::sessions::change(
            &self.working_directory,
            self.config().conversation_days,
            notes.as_deref(),
            what,
        );
        if written.is_some() {
            self.sessions_kept = written;
        }
    }

    /// Writes down which conversation is which.
    ///
    /// Every time one is named or renamed, because the moment Obelus does
    /// not survive is the one nobody plans for: a crash between opening a
    /// conversation and remembering it is a conversation the agent keeps
    /// and nobody can reach.
    ///
    /// The ones about nothing in particular as well as the ones about a
    /// note. They were left out while the only thing that asked was the
    /// notes page, which has no row for one -- and the list of
    /// conversations is a second thing asking, whose whole subject is the
    /// ones a reader would otherwise have no way back to.
    pub(super) fn remember_the_conversations(&mut self) {
        let Some(agent) = self.talker.as_ref().map(|talker| talker.id().to_string()) else {
            return;
        };
        let talker = self.talker.as_ref();
        // Now, for every one being written: what orders the list of
        // conversations and what each row of it says about itself. Taken
        // once rather than per conversation, so that two written in one
        // pass are not a second apart in it.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let mine: Vec<(
            obelus_agent::chats::ChatId,
            obelus_agent::acp::sessions::Kept,
        )> = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .filter_map(|talk| {
                let session = talk.session.as_ref()?;
                let which = talk.which()?;
                // Nothing said in it yet, so there is nothing to come
                // back to -- and writing it down would take the place of
                // a conversation there *is* something to come back to.
                //
                // An agent does not keep a session nobody said anything
                // in: claude-agent-acp writes a conversation's file on
                // the first turn, so a name minted and never used is a
                // name that will not be there tomorrow. Written down the
                // moment it was minted, it displaced the note's real
                // conversation, and the next morning Obelus asked for it,
                // was told there is no such thing, opened another empty
                // one and wrote *that* down. The reader's conversation
                // went in the first round of it and every round after was
                // the same round again.
                //
                // Anything *said*, rather than anything on the page:
                // Obelus writes in a conversation of its own accord, and
                // a page holding nothing but "starting again, because"
                // is a page with nothing to come back to.
                if !talk.chat.anything_said() {
                    return None;
                }
                Some((
                    which,
                    obelus_agent::acp::sessions::Kept {
                        session: session.0.to_string(),
                        title: talker
                            .and_then(|talker| talker.title(Some(session)))
                            .map(str::to_string),
                        told: talk.told.clone(),
                        introduced: talk.introduced,
                        last: Some(now),
                    },
                ))
            })
            .collect();
        if mine.is_empty() {
            return;
        }
        // In the checkout the agent was told, which is the one it will
        // take the conversation up in and no other.
        let here = self.working_directory.clone();
        self.change_the_sessions(|kept| {
            for (which, what) in mine {
                kept.put(&which, &agent, &here, what);
            }
        });
    }

    /// Whether the conversation being read is about a note that is still
    /// there, which is whether there is a note for its key to go back to.
    #[must_use]
    pub fn is_about_a_note(&self) -> bool {
        self.the_note_this_is_about().is_some()
    }

    /// The note the conversation being read is about, as the file last said.
    ///
    /// Kept rather than parsed here, and heard rather than polled: the
    /// reader can change what a note says from the notes page, from their
    /// own editor or from a second Obelus, and a view holding a copy
    /// nothing refreshed would go on saying what the note used to. What
    /// keeps the copy honest is that all three of those *write the file*,
    /// and a write is something a watcher hears.
    ///
    /// It read the file here, which is 29us for one note and 225us for
    /// twenty -- and a conversation builds its view twice a frame, for the
    /// editor and for the status row, so that was the bill twice on every
    /// keystroke of every conversation about a note.
    fn the_note_this_is_about(&self) -> Option<&obelus_git::todo::Note> {
        let Topic::Note(id) = &self.conversation()?.topic else {
            return None;
        };
        self.notes_kept
            .as_ref()?
            .notes
            .iter()
            .find(|note| note.id == *id)
    }

    /// The branch the conversation being read is working on, once its agent
    /// has changed something.
    #[must_use]
    pub fn branch_this_conversation_works_on(&self) -> Option<&obelus_git::Head> {
        self.conversation()?
            .working_in
            .as_ref()
            .map(|(_, head)| head)
    }

    /// Asks every conversation's checkout which branch it is on again,
    /// because the repository moved.
    ///
    /// A tree that has gone answers nothing rather than being asked: git
    /// would look upwards from where it was and answer for whatever
    /// repository is above it, which for a worktree under `.worktree` is
    /// the reader's own.
    pub(super) fn ask_the_conversations_their_branch(&mut self) {
        for document in &mut self.documents {
            let Some(talk) = document.as_mut().and_then(Document::chat_mut) else {
                continue;
            };
            talk.working_in = talk.working_in.take().and_then(|(tree, _)| {
                if obelus_git::is_gone(&tree) {
                    return None;
                }
                obelus_git::head_of_the_tree(&tree).map(|head| (tree, head))
            });
        }
    }

    /// Notes the branch a call changed a file on, if this update finished
    /// a change.
    ///
    /// Asked again on every change rather than once: where the agent works
    /// is not settled by where it first wrote, and a checkout's branch can
    /// move under it. The tree must be this project's, a worktree or the
    /// reader's own: a file outside it is somebody else's repository, or
    /// none, and its branch says nothing about this work.
    fn hear_where_it_wrote(&mut self, whose: Whose, call: &str) {
        let Some(path) = self
            .talk(whose)
            .and_then(|talk| talk.chat.wrote(call))
            .map(|path| self.working_directory.join(path))
        else {
            return;
        };
        let Some(tree) = obelus_git::worktree(&path) else {
            return;
        };
        let ours = obelus_git::project(&self.working_directory);
        if ours.is_none() || obelus_git::project(&tree) != ours {
            return;
        }
        let head = obelus_git::head_of_the_tree(&tree);
        if let Some(talk) = self.talk_mut(whose) {
            talk.working_in = head.map(|head| (tree, head));
        }
        // And its thread, whose head says the branch.
        self.mirror_head(whose, None);
    }

    /// Reads the project's notes again, because there is a reason to.
    ///
    /// What the box of a conversation about a note asks, on every frame,
    /// to know whether the agent has been told what the note says now. The
    /// page has its own copy and does not use this one: what is on the page
    /// is the reader's, edits and all, and is ahead of the file rather than
    /// behind it.
    pub(super) fn reread_the_notes_kept(&mut self) {
        self.notes_kept = obelus_git::todo::read(&self.working_directory).notes();
    }

    /// Puts pasted text into the box a message is written in.
    ///
    /// The whole of what a paste means here: a conversation has one place
    /// text can go, and it is the box. What was said is what was said.
    ///
    /// Dropped under a card with nowhere to write, rather than put behind
    /// it. Not [`App::conversation_takes_text`], which also says no while
    /// the keys are off the box: what an input method commits arrives here,
    /// and a word spelled in the transcript goes where a letter typed there
    /// goes -- into the box, which takes the keys back. The same answer
    /// [`App::takes_text`] gives the window, which turned the input method
    /// on for it.
    pub(super) fn paste_into_conversation(&mut self, what: &str) {
        let covered = self
            .conversation()
            .and_then(|talk| talk.card.as_ref())
            .is_some_and(|card| !card.takes_words());
        if covered {
            return;
        }
        // A path to a picture is the picture. A terminal has no event for a
        // file dragged onto it, only the words for its path, and a reader
        // dragging a screenshot meant the screenshot. Not under a card,
        // which takes words, and not to an agent that said it takes none --
        // but wherever the keys are in the conversation, because a paste
        // takes them back to the box.
        let carded = self.conversation().is_some_and(|talk| talk.card.is_some());
        if !carded && self.agent_takes_pictures() && self.attach_what_is_named(what) {
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

    /// Puts in the pictures a paste names, and the rest of it as words.
    ///
    /// `false`, having put in nothing, where it names no picture that is
    /// there: then the paste is the words it was, untouched, rather than
    /// cut into lines at every path it seemed to have in it. A piece named
    /// like a picture that is not one is words like any other.
    fn attach_what_is_named(&mut self, what: &str) -> bool {
        use obelus_clipboard::dropped::{Piece, picture_at};
        let mut pictures = Vec::new();
        let mut words = Vec::new();
        for piece in obelus_clipboard::dropped::pieces(what) {
            match piece {
                Piece::Words(said) => words.push(said),
                Piece::Picture { said, path } => match picture_at(&path) {
                    Some((mime, bytes)) => {
                        pictures.push(obelus_component::composer::Attached { mime, bytes });
                    }
                    None => words.push(said),
                },
            }
        }
        if pictures.is_empty() {
            return false;
        }
        let room = obelus_ui::chat::writing_width(self.editor_area);
        let Some(talk) = self.conversation_mut() else {
            return false;
        };
        for picture in pictures {
            talk.chat.attach(picture, room);
        }
        if !words.is_empty() {
            talk.chat.paste(&words.join("\n"), room);
        }
        true
    }

    /// A file dropped on the window, put in as a terminal would have put it.
    ///
    /// Into the box a message is written in and nowhere else: a file dropped
    /// on the file being read is not an edit anybody meant, and a drop is
    /// the one paste with no key behind it to have been pressed on purpose.
    /// So it goes where [`App::paste_text`] would put it only when that is a
    /// conversation -- nothing in front of it, and no card over the box
    /// that takes no words -- and is refused everywhere else.
    pub(super) fn dropped(&mut self, path: &std::path::Path) {
        let into_a_conversation = self.layers().nearest().is_none()
            && self.chooser.is_none()
            && self
                .conversation()
                .is_some_and(|talk| talk.card.as_ref().is_none_or(Card::takes_words));
        if !into_a_conversation {
            self.wrong("A file goes in a message to an agent".to_string());
            return;
        }
        // The space after it is the terminal's too: several dropped at once
        // arrive one at a time, and their paths must not run together.
        let typed = format!("{} ", obelus_clipboard::dropped::as_typed(path));
        self.paste_into_conversation(&typed);
    }

    /// The conversation, while it is what the reader is looking at.
    #[must_use]
    pub fn chat(&self) -> Option<&Chat> {
        Some(&self.conversation()?.chat)
    }

    /// What Obelus is doing about an agent, in the conversation being read.
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
        // with a turning mark beside it, and so Obelus woke twelve times a
        // second behind every file, every list and the notes, for a turn
        // that was not running. An agent that is up and being asked
        // nothing is ready, which is what this now says.
        if self.conversation().is_none() {
            return Talking::Ready;
        }
        let held = self.session_now();
        let session = held.as_ref();
        let requested = self.conversation().and_then(|talk| talk.requested);
        if talker.is_thinking(session, requested) {
            Talking::Thinking
        } else if talker.is_started(session) {
            Talking::Ready
        } else if self
            .conversation()
            .is_some_and(|talk| talk.opening || talk.asked_for.is_some())
        {
            Talking::Starting
        } else {
            // Nothing has been asked for: the frame that would ask has not
            // been drawn yet, or the agent could not be started for it.
            // The same answer whether or not the process happens to be up
            // for some other conversation: what this is about is the page
            // in front of the reader, and a transcript saying `starting...`
            // about a conversation nothing is starting is the one thing
            // this row must not say.
            Talking::Idle
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
        // The registry's name rather than its id, which is the same word as
        // the agent's own title for most of them: a header reading
        // `claude-acp` and then `Claude Agent` is the change this was
        // meant to spare the reader.
        let listed = chosen.map(|id| {
            self.agents
                .registry
                .iter()
                .find(|agent| agent.id == id)
                .map_or(id, |agent| agent.name.as_str())
        });
        self.talker.as_ref().and_then(acp::Talk::info).or(listed)
    }

    /// What an agent is called, by the registry's name for it.
    ///
    /// Its id until the registry has arrived, and for one the registry does
    /// not list: what a tab of the list of conversations needs is a word
    /// for the agent that had them, and the id is a word.
    ///
    /// Not [`App::agent_name`], which is about the agent being talked to
    /// and answers with what *it* said in the handshake. This is asked
    /// about agents that are not running and never will be in this window.
    #[must_use]
    pub fn agent_called(&self, id: &str) -> String {
        self.agents
            .registry
            .iter()
            .find(|agent| agent.id == id)
            .map_or_else(|| id.to_string(), |agent| agent.name.clone())
    }

    /// The way of working the agent is in, if it offers one.
    ///
    /// A setting like the others -- the one the agent said is the mode --
    /// named apart because one key steps it.
    #[must_use]
    pub fn agent_mode(&self) -> Option<&acp::Setting> {
        self.agent_settings()
            .iter()
            .find(|setting| setting.category == acp::Category::Mode)
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
    ///
    /// The session's own and nothing else. A conversation has one from the
    /// frame it is shown on, so before it arrives -- a second or two -- the
    /// row is empty: what it will open on is the agent's answer to
    /// `session/new`, and a guess drawn from an earlier one is a list an
    /// update may have changed.
    #[must_use]
    pub fn agent_settings(&self) -> &[acp::Setting] {
        let session = self.session_now();
        self.talker
            .as_ref()
            .map_or(&[] as &[acp::Setting], |talker| {
                talker.settings(session.as_ref())
            })
    }

    /// One setting's values, as the ordinary compact list.
    ///
    /// What enter on the conversation's own row opens, for a setting whose
    /// values are a list. A switch never comes here: it has two sides and
    /// is flipped where it stands.
    pub(super) fn open_agent_setting(&mut self, id: &str) {
        let Some(setting) = self
            .agent_settings()
            .iter()
            .find(|setting| setting.id == id)
            .cloned()
        else {
            return;
        };
        let setting = &setting;
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
                section: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.before_typing("Filter values");
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
    /// values Obelus makes for it is what the settings page needs, and on
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

    /// Keeps what a session says its agent offers, for the settings page.
    ///
    /// Any session's, and whatever it is set to: the page draws what the
    /// agent offers and what the reader chose, and never reads which value
    /// some conversation was left on.
    fn hear_what_the_agent_offers(&mut self, session: &acp::SessionId) {
        let Some(talker) = self.talker.as_ref() else {
            return;
        };
        let settings = talker.settings(Some(session));
        // Nothing said is not "it offers nothing": an agent says what it
        // offers when it opens a session, and some say it a moment later.
        if settings.is_empty() {
            return;
        }
        self.agents.offers = Some((talker.id().to_string(), settings.to_vec()));
    }

    /// Puts a conversation on what the reader said conversations start on.
    ///
    /// Once for each setting, and only for the ones they have said
    /// something about: what they have said nothing about is the agent's,
    /// which is the third state those rows have.
    ///
    /// Nothing is written in the transcript for what goes through: this is
    /// a standing choice they made on another page, not something they just
    /// did here, and the row at the foot says what it is set to either way.
    /// What does not go through is said, once: the settings page marks a
    /// choice the agent no longer offers, but a reader in a conversation is
    /// not looking at that page, and the row showing something other than
    /// what they chose is a question with no answer on screen.
    fn start_the_session_on_what_was_chosen(&mut self, session: &acp::SessionId) {
        // The running agent's own name again: what the reader said is
        // written down under the agent it was said about, and this
        // conversation belongs to whichever one is on the other end.
        let (agent, settings) = match self.talker.as_ref() {
            Some(talker) => (
                talker.id().to_string(),
                talker.settings(Some(session)).to_vec(),
            ),
            None => return,
        };
        // What the reader has pinned for this agent.
        let chosen = self.config().agent_defaults(&agent).clone();
        if chosen.is_empty() {
            return;
        }
        let already = self
            .conversation_at(|talk| talk.session.as_ref() == Some(session))
            .and_then(|at| self.documents.get(at))
            .and_then(Option::as_ref)
            .and_then(Document::chat)
            .map(|talk| talk.started_on.clone())
            .unwrap_or_default();

        let name = self.agent_called(&agent);
        let said_already = self
            .conversation_at(|talk| talk.session.as_ref() == Some(session))
            .and_then(|at| self.documents.get(at))
            .and_then(Option::as_ref)
            .and_then(Document::chat)
            .map(|talk| talk.said_not_offered.clone())
            .unwrap_or_default();
        let mut asked = Vec::new();
        let mut gone = Vec::new();
        for setting in &settings {
            let Some(value) = chosen.get(&setting.id) else {
                continue;
            };
            // Once each. An agent that refused, or that has put it back
            // since, has answered -- and Obelus asking again would be
            // Obelus arguing with it.
            if already.contains(&setting.id) {
                continue;
            }
            // A value it does not offer any more is left alone rather than
            // sent and refused, and said: the settings page is where the
            // reader can do something about it, and this is where they
            // find out there is something to do. Counted as asked, and said
            // once a conversation rather than once a session -- see
            // `Conversation::said_not_offered`.
            if setting.name_of(value).is_none() {
                tracing::debug!(
                    setting = setting.id,
                    value,
                    "what was chosen is not offered any more"
                );
                asked.push(setting.id.clone());
                let said = (setting.id.clone(), value.clone());
                if said_already.contains(&said) {
                    continue;
                }
                // Not opening on the agent's name, for the reason the
                // settings page's line does not: see `Shown::warning`.
                gone.push((
                    said,
                    format!(
                        "No longer offered by {name}: {value} for {}, so this conversation is \
                         on {}",
                        setting.name,
                        setting.current_name().unwrap_or(&setting.current)
                    ),
                ));
                continue;
            }
            asked.push(setting.id.clone());
            if setting.current == *value {
                // Already there. Written down as asked all the same: the
                // question has been settled for this conversation, and an
                // agent that moves it later has moved it itself.
                continue;
            }
            let chosen = acp::Chosen::of(setting, value);
            if let Some(talker) = self.talker.as_mut() {
                talker.set(Some(session), &setting.id, chosen);
            }
        }
        if asked.is_empty() {
            return;
        }
        if let Some(talk) = self
            .conversation_at(|talk| talk.session.as_ref() == Some(session))
            .and_then(|at| self.documents.get_mut(at))
            .and_then(Option::as_mut)
            .and_then(Document::chat_mut)
        {
            talk.started_on.extend(asked);
            for (id, line) in gone {
                talk.chat.note(&line);
                talk.said_not_offered.insert(id);
            }
        }
    }

    /// Sends what the reader typed, or puts it on the page to go when the
    /// turn is over.
    ///
    /// A conversation takes one prompt turn at a time. What the reader says
    /// into a running one waits on the page, dim, where they can see it and
    /// take it back -- see [`crate::conversation`] for why the protocol
    /// leaves no third option.
    pub(super) fn send_to_agent(&mut self, parts: &[Part]) {
        if self.talking() == Talking::Thinking {
            if let Some(talk) = self.conversation_mut() {
                talk.chat.will_say(parts);
            }
            return;
        }
        self.say_in(Whose::Whoever, parts, false);
    }

    /// Says one thing in the conversation `whose` names.
    ///
    /// Two callers, and the difference between them is only which
    /// conversation: the reader pressing enter says it here, and a turn
    /// ending says whatever was waiting behind it -- which may be a
    /// conversation they have since walked away from, because a turn goes
    /// on running while they read something else.
    fn say_in(&mut self, whose: Whose, parts: &[Part], on_the_page: bool) {
        // What this agent does not know yet, if anything: who it is talking
        // to, and what the conversation is about. Worked out here rather
        // than kept, because a note is a file the reader can change between
        // any two messages.
        let Some(talk) = self.talk(whose) else {
            return;
        };
        let (topic, introduced, told) = (talk.topic.clone(), talk.introduced, talk.told.clone());
        let opening = self.opening(&topic, introduced, told.as_deref());
        if let Some(talk) = self.talk_mut(whose) {
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
                // What Obelus sends in the reader's name is the reader's
                // to see. Which the piece saying who the agent is talking
                // to is not -- see `Opening::said`.
                if let Some(said) = opening.said {
                    talk.chat.note(said);
                }
            }
            // Unless they are already there, dim, waiting for this moment:
            // what goes out is those rows joined, and writing them again
            // would be saying the whole of it twice.
            if !on_the_page {
                talk.chat.asked(parts);
            }
        }
        let opening = opening.map(|opening| opening.words);
        // Words that came from a chat carry a line saying so, for the
        // agent; the reader's own here go to the chat, so that the thread
        // has both halves.
        let (afar, echoed) = self.origin_of(whose);
        if echoed {
            self.mirror_typed_here(whose, parts);
        }
        let opening = match (opening, afar) {
            (Some(opening), Some(afar)) => Some(format!("{opening}{afar}")),
            (opening, afar) => opening.or(afar),
        };
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
        //
        // Also a conversation whose session was let go while the reader
        // was elsewhere and that they typed into before the frame that
        // asks for another: the process may be up already, and this one
        // still has no session.
        if self
            .talker
            .as_ref()
            .is_none_or(obelus_agent::acp::Talk::has_exited)
        {
            // The session went with the process that held it, and every
            // conversation was told so when it went -- see
            // `forget_what_the_agent_held` -- so this one asks for its own
            // again below: the one written down against its note where
            // there is one, which is how a conversation survives the agent
            // dying under it.
            self.stop_agent();
            self.start_agent();
        }
        // Asked whether or not the process was just started, and after
        // that block rather than inside it: a conversation with no session
        // and a running agent is what the key that opens one now leaves
        // behind, and the message would otherwise be held for a session
        // nobody had asked for.
        let note = match &topic {
            Topic::Note(note) => Some(note.clone()),
            Topic::Loose => None,
        };
        let had = note.as_ref().and_then(|note| self.remembered_session(note));
        self.ask_for_a_session(whose, had);
        let session = self.talk(whose).and_then(|talk| talk.session.clone());
        let asking = self.talk(whose).and_then(|talk| talk.requested);
        let Some(talker) = self.talker.as_mut() else {
            // `start_agent` has already said why in the transcript.
            return;
        };
        // Held until the session opens, which is the ordinary case for the
        // first thing said: the reader typed while it was starting, and the
        // handle sends it when the answer to this conversation's request
        // arrives -- opening and all, because the opening belongs to
        // whatever goes first.
        talker.say(session.as_ref(), asking, said_of(parts), opening.as_deref());
        // A turn has started, which its thread says at the top of it.
        self.mirror_head(whose, Some(obelus_remote::model::Turning::Working));
    }

    /// Says what the reader had waiting, now that the turn it was waiting
    /// on is over.
    ///
    /// All of it, as one prompt: three things typed into one running turn
    /// are one thing the reader is saying, and an agent handed only the
    /// first of them answers a question it has not been asked the whole
    /// of. Joined by a blank line, which is what the box's own `alt+enter`
    /// makes, so what arrives is what they would have typed.
    ///
    /// A turn the reader stopped has only what they said after pressing
    /// escape and before the stop landed: what was waiting when they
    /// pressed it went back into the box (`interrupt_agent`).
    fn say_what_was_waiting(&mut self, whose: Whose) {
        let Some(talk) = self.talk_mut(whose) else {
            return;
        };
        let waiting = talk.chat.unsent();
        if waiting.is_empty() {
            return;
        }
        let afar = talk.chat.unsent_afar();
        talk.chat.sent();
        self.about_to_say_what_waited(whose, &waiting, &afar);
        // One prompt, so the messages are joined the way the box's own
        // `alt+enter` joins two paragraphs -- and the pictures keep their
        // places between them, because the join is a run of parts and not
        // a run of text.
        let mut parts: Vec<Part> = Vec::new();
        for said in waiting {
            if !parts.is_empty() {
                parts.push(Part::Words("\n\n".to_string()));
            }
            parts.extend(said);
        }
        self.say_in(whose, &parts, true);
    }

    /// Closes a conversation, because its agent asked to -- having asked
    /// the reader, which is the agent's to do.
    ///
    /// The number is the one the conversation's tools address carries,
    /// which is its document's. What comes back is said to the agent, which
    /// is waiting on an answer: closed, or why not.
    ///
    /// At once, and the turn it was asked from is stopped on the way out.
    /// It used to wait for that turn to end, so that whatever the agent
    /// said after the call landed somewhere the reader could see -- and a
    /// turn the agent never ends then kept the conversation open for good,
    /// saying it was thinking. Which happens: Claude's adapter folds a
    /// prompt that arrives while a background task's notification is being
    /// answered into that answer, and never answers the prompt. Stopped,
    /// the end is Obelus's own to write, and the conversation is not left
    /// running where nobody can see it -- taken up again, it would still be
    /// thinking. What is lost is a sentence after the call, and the tool's
    /// description tells the agent there is nobody to say it to.
    ///
    /// Only a turn Obelus asked for can be stopped, because only that one
    /// has a number here. One the agent started of its own accord, with no
    /// prompt of Obelus's in flight, goes on in a session nothing shows --
    /// as it did when the close waited, which closed at once in that case
    /// too.
    pub(super) fn close_for_an_agent(&mut self, conversation: Option<usize>) -> String {
        let Some(id) = conversation.map(DocumentId::new) else {
            return "this address names no conversation, so there is nothing to close".to_string();
        };
        match self.document(id).and_then(Document::chat) {
            Some(talk) if talk.has_the_readers_words() => {
                return "the reader has started writing in it, so it stays open".to_string();
            }
            // A question still up in it goes with the conversation, and the
            // reader never sees it answered. The agent can ask in parallel
            // with this call, and when the close waited for the turn to end
            // the question had been answered by then.
            Some(talk) if talk.is_waiting_on_the_reader() => {
                return "the reader has a question of yours open in it, so it stays open"
                    .to_string();
            }
            Some(_) => {}
            None => return "this conversation is not open any more".to_string(),
        }
        self.interrupt_agent(id);
        self.close_the_conversation(id);
        "closed".to_string()
    }

    /// Closes one conversation and says which, which is the difference
    /// from the reader closing it: they know they did.
    fn close_the_conversation(&mut self, id: DocumentId) {
        // By the name the list of what is open gives it, read before it
        // goes: the reader may have been somewhere else, and "the
        // conversation" is then one of several.
        let named = self.document(id).and_then(Document::chat).and_then(|talk| {
            let notes = obelus_git::todo::read(&self.working_directory)
                .notes()
                .unwrap_or_default();
            self.conversation_name(talk, &notes)
        });
        self.close(id);
        // Bounded the way a path said on this row is (`named`): the name
        // can be a whole line of the reader's, and a sentence the row cannot
        // hold squeezes the file's name out of it or is not said at all.
        self.say(match named {
            Some(named) => format!("Closed {}", obelus_ui::truncate_from_right(&named, 40)),
            None => "Closed the conversation".to_string(),
        });
        // And out of the list of what is open, where that is showing: a
        // row for a conversation that has gone is a row that lies.
        self.refresh_switching();
    }

    /// Asks the agent to stop what it is doing.
    ///
    /// And stops the commands Obelus is running for this conversation,
    /// rather than only asking. The processes are Obelus's -- it started
    /// them -- and an agent told to stop is under no obligation to release
    /// a terminal on its way out: one that did not would leave a build
    /// running that the reader has just said they want stopped, with
    /// nothing left on screen that could stop it.
    ///
    /// The half Obelus owes for not asking before it runs them: a key
    /// stops it.
    ///
    /// And what the reader said while it ran goes back into the box rather
    /// than to the agent. It used to go the moment the turn ended, so the
    /// conversation stopped and started again on one press -- the mark
    /// still turning under `Stopped`, because the words that had gone were
    /// above that line -- and the reader pressed escape again and stopped
    /// their own words. Stop means stop; saying them is enter, in the box,
    /// where they can be changed first.
    ///
    /// The conversation is named rather than read off the screen, because
    /// an agent closing its own is stopped too, and it may not be the one
    /// the reader is in.
    pub(super) fn interrupt_agent(&mut self, id: DocumentId) {
        if let Some(talk) = self.talk_mut(Whose::One(id)) {
            // Escape after `ctrl+enter` and before the stop has landed:
            // what was going to be said is back in the box, so this one
            // is a stop again.
            talk.stopped_to_say = false;
            if let Some(parts) = talk.chat.take_back_waiting() {
                talk.chat.put_back(parts);
            }
        }
        self.stop_the_turn(id);
    }

    /// Stops the turn and says what the reader had waiting, with this
    /// behind it: escape and then enter, in one press.
    ///
    /// Through the queue rather than past it, so the prompt goes when the
    /// turn has ended here and not a moment before -- zed's "send now" is
    /// the cancellation awaited and *then* the prompt, for the reason
    /// [`crate::conversation`] gives for queueing at all. What was waiting
    /// stays on the page as the rows it was, and goes first.
    ///
    /// And the turn's end says nothing: what the reader said is the next
    /// thing on the page, and `Stopped` above it says the half of the
    /// press they did not mean.
    pub(super) fn send_now(&mut self, id: DocumentId, parts: &[Part]) {
        if let Some(talk) = self.talk_mut(Whose::One(id)) {
            talk.stopped_to_say = true;
            talk.chat.will_say(parts);
        }
        self.stop_the_turn(id);
    }

    /// Tells the agent to stop, and stops what it left running here.
    fn stop_the_turn(&mut self, id: DocumentId) {
        let running: Vec<String> = self
            .talk(Whose::One(id))
            .map(|talk| talk.chat.commands())
            .unwrap_or_default();
        for command in running {
            self.runs.stop(&command);
            self.tell_whoever_waited(&command);
        }
        // And the calls it left open, which the agent will not close if it
        // never saw the cancellation: a row that says it is running under a
        // conversation Obelus has said is resting is the one thing on
        // screen that cannot both be true. Worse than wrong, too: nothing
        // wakes the screen for a conversation that is not working, so the
        // mark on that row is a spinner frozen mid-turn. A call Obelus is
        // running the command for is the exception: its state is the
        // runner's, read every frame (`Chat::running`), and it says how the
        // command stopped above ended -- not Obelus's to overwrite.
        let Some(talk) = self.talk_mut(Whose::One(id)) else {
            return;
        };
        talk.chat.stop_the_calls();
        let session = talk.session.clone();
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
        let mut forgotten = Vec::new();
        for (at, document) in self.documents.iter_mut().enumerate() {
            let Some(talk) = document.as_mut().and_then(Document::chat_mut) else {
                continue;
            };
            // The card rather than the channels: it is what the reader
            // could see, so it is what their page has to account for.
            let asked = talk.card.is_some();
            talk.permission = None;
            talk.asking = None;
            talk.going = None;
            talk.signing_in = None;
            talk.sign_in_next = None;
            talk.queued.clear();
            talk.card = None;
            if asked {
                talk.chat.note("It stopped waiting for an answer");
                forgotten.push(DocumentId::new(at));
            }
        }
        // And the same card in a chat is closed, or it is pressed into
        // nothing.
        for id in forgotten {
            self.mirror_withdrawn(Whose::One(id));
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
                // And the line is no longer a name, so a list shut on one
                // has nothing left to be shut on: the next slash is a fresh
                // question. Noticed here because here is where the list is
                // worked out -- one asker, so there is no second answer to
                // drift from this one.
                talk.slash_shut = false;
            }
            return;
        };
        // Shut by the reader, on the name they are still typing. Nothing to
        // build and nothing to settle: what they asked for is no list.
        if self.conversation().is_some_and(|talk| talk.slash_shut) {
            return;
        }
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
                section: None,
            })
            .collect();
        let mut slash = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        slash.before_typing("Filter what the agent offers");
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
                let room = obelus_ui::chat::above_writing(self.editor_area, chat, self.card());
                (obelus_ui::picker::rows_drawn(slash, room), room.width)
            });
        if let (Some((rows, width)), Some(slash)) = (
            rows,
            self.conversation_mut().and_then(|talk| talk.slash.as_mut()),
        ) {
            slash.refresh_indices(rows, width);
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
        // And `ctrl+enter`, which is enter with the turn stopped first:
        // the name is not settled yet, so there is nothing to send, and a
        // half-typed one sent to the agent would stop its turn for nothing.
        let sending_now = key.code == KeyCode::Enter && modifiers == KeyModifiers::CONTROL;
        if modifiers != KeyModifiers::NONE && !sending_now {
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
            // list -- and like every other completion in Obelus, which is
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
                    // Written down, because closing it is not enough: the
                    // list is worked out from the box every frame, so the
                    // next frame would put back what this key took away.
                    talk.slash_shut = true;
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
            ChatOutcome::Send(parts) => {
                self.send_to_agent(&parts);
                true
            }
            ChatOutcome::Interrupt => {
                if let Some(id) = self.current {
                    self.interrupt_agent(id);
                }
                true
            }
            ChatOutcome::SendNow(parts) => {
                if let Some(id) = self.current {
                    self.send_now(id, &parts);
                }
                true
            }
            ChatOutcome::TakeBack(parts) => {
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.put_back(parts);
                }
                true
            }
            // Somewhere the reader was sent, sent again: the browser tab
            // is closed, the sign-in was not finished. The agent is not
            // asked anything -- it was told they went the first time, and
            // it is watching the far end rather than Obelus.
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
                // of Obelus counts them from zero, which is what `go_to`
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
                self.mirror_answered_here(Whose::Whoever, "Not answered");
                match self.is_asking_permission() {
                    true => self.refuse_permission(Whose::Whoever),
                    false => self.refuse_asking(Whose::Whoever),
                }
                self.ask_the_next(Whose::Whoever);
                true
            }
            CardOutcome::Answered { chosen, words } => {
                // What was answered, the way the thread can say it: the
                // names of what was chosen and the words written.
                let said = self
                    .conversation()
                    .and_then(|talk| talk.card.as_ref())
                    .map(|card| {
                        chosen
                            .iter()
                            .map(|id| card.name_of(id).unwrap_or(id).to_string())
                            .chain(words.iter().cloned())
                            .collect::<Vec<String>>()
                            .join(", ")
                    });
                self.mirror_answered_here(Whose::Whoever, &said.unwrap_or_default());
                self.answer_card(Whose::Whoever, &chosen, words.as_deref());
                self.ask_the_next(Whose::Whoever);
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
    /// It does not bring the conversation to the front, and the name is
    /// what is left of one that did: a conversation is a document, so a
    /// question asked in one the reader is not in waits on its row in the
    /// list with a mark saying so, rather than pulling the screen away from
    /// whatever they were reading. Every question names its conversation,
    /// so none is left to be asked wherever the reader happens to be.
    ///
    /// What is still here is the clearing: a card is drawn inside the
    /// conversation, so anything of Obelus's own over that region would be a
    /// card the reader cannot see while the agent waits on it.
    fn show_the_question(&mut self, whose: Whose) {
        // The clearing is for the screen, so it happens only where the
        // question is going onto it. A question waiting in a conversation
        // the reader is not in has nothing to clear and no business
        // closing what is in front of them.
        if self.is_here(whose) {
            self.make_room(Room::Region);
        }
        // And nothing of the reader's own over it. The list of the agent's
        // commands follows what is being typed in the box, and the box is
        // what the card covers: left open it would be a list over a
        // question, about words the keys are no longer going to.
        if let Some(talk) = self.talk_mut(whose) {
            talk.slash = None;
        }
    }

    /// Puts a question to the reader, or behind the one already up.
    ///
    /// There is one card, so a second question waits for the first to be
    /// answered: taking its place would leave the agent waiting for ever on
    /// a question nothing on screen is asking. A permission's call goes in
    /// the transcript the moment it arrives all the same, waiting, so the
    /// page says there is more to answer before the card does.
    fn put_to_the_reader(&mut self, whose: Whose, question: acp::Question) {
        if let Some(talk) = self.talk_mut(whose)
            && talk.is_waiting_on_the_reader()
        {
            if let acp::Question::Permission { call, .. } = &question {
                talk.chat.tool(call, "pending");
            }
            talk.queued.push_back(question);
            return;
        }
        match question {
            acp::Question::Permission {
                call,
                reason,
                options,
                answer,
            } => self.ask_permission(whose, &call, reason.as_deref(), &options, answer),
            acp::Question::Ask {
                message,
                fields,
                answer,
            } => self.ask_reader(whose, &message, fields, answer),
            acp::Question::Open {
                message,
                url,
                id,
                answer,
            } => self.send_the_reader(whose, &message, &url, &id, answer),
        }
        // A form with nothing in it is answered as it is asked, and then
        // nothing else would put up what was waiting behind it.
        self.ask_the_next(whose);
    }

    /// Puts up the question waiting behind the one just answered.
    pub(super) fn ask_the_next(&mut self, whose: Whose) {
        let Some(talk) = self.talk_mut(whose) else {
            return;
        };
        if talk.is_waiting_on_the_reader() {
            return;
        }
        if let Some(why) = talk.sign_in_next.take() {
            self.ask_to_sign_in(whose, why);
            return;
        }
        if let Some(next) = talk.queued.pop_front() {
            self.put_to_the_reader(whose, next);
        }
    }

    /// Takes the sign-in's card down wherever it is up, because the reader
    /// is in: every conversation that was waiting is being asked again,
    /// and a card left in one would ask them to sign in twice.
    pub(super) fn signed_in_everywhere(&mut self) {
        for document in self.documents.iter_mut().flatten() {
            if let Some(talk) = Document::chat_mut(document) {
                talk.sign_in_next = None;
                if talk.signing_in.take().is_some() {
                    talk.card = None;
                    talk.chat.note("Signed in");
                }
            }
        }
    }

    /// Takes down what the agent no longer wants answered.
    ///
    /// The one up or one waiting: each question's answer channel says
    /// whether anybody is still at the other end, so the agent does not have
    /// to say which it took back.
    fn take_back(&mut self, whose: Whose) {
        let Some(talk) = self.talk_mut(whose) else {
            return;
        };
        let (gone, kept) = std::mem::take(&mut talk.queued)
            .into_iter()
            .partition(taken_back);
        talk.queued = kept;
        // A call waiting behind the card was put in the transcript as
        // waiting, and nothing else will say it stopped: the agent took the
        // question back, not necessarily the call.
        for question in gone {
            if let acp::Question::Permission { call, .. } = question {
                talk.chat.tool(&call, "cancelled");
            }
        }
        let up = talk
            .permission
            .as_ref()
            .is_some_and(|asked| asked.answer.is_canceled())
            || talk
                .asking
                .as_ref()
                .is_some_and(|asking| asking.answer.is_canceled())
            || talk
                .going
                .as_ref()
                .is_some_and(|going| going.answer.is_canceled());
        if !up {
            return;
        }
        // The call on the card as well as the ones behind it: taking the
        // question back is not saying what became of the call, and one left
        // waiting turns as though it were running.
        if let Some(asked) = talk.permission.take() {
            talk.chat.tool(&asked.call, "cancelled");
        }
        talk.asking = None;
        talk.going = None;
        talk.card = None;
        talk.chat.note("It stopped waiting for an answer");
        self.mirror_withdrawn(whose);
        self.ask_the_next(whose);
    }

    /// Takes down every question in a conversation whose turn ended
    /// without finishing, the one up and the ones behind it.
    ///
    /// Dropped rather than answered: an answer channel that goes away is
    /// the protocol's "cancelled" to the agent, which is what the protocol
    /// asks a client to send every request still open once a turn is
    /// cancelled -- an agent told nothing waits on them for ever.
    fn give_up_the_questions(&mut self, whose: Whose) {
        let Some(talk) = self.talk_mut(whose) else {
            return;
        };
        for question in std::mem::take(&mut talk.queued) {
            if let acp::Question::Permission { call, .. } = question {
                talk.chat.tool(&call, "cancelled");
            }
        }
        if let Some(asked) = talk.permission.take() {
            talk.chat.tool(&asked.call, "cancelled");
        }
        talk.asking = None;
        talk.going = None;
        if talk.card.take().is_some() {
            self.mirror_withdrawn(whose);
        }
    }

    /// Puts a form the agent asked for to the reader.
    fn ask_reader(
        &mut self,
        whose: Whose,
        message: &str,
        fields: Vec<acp::Field>,
        answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
    ) {
        self.show_the_question(whose);
        if let Some(talk) = self.talk_mut(whose) {
            talk.asking = Some(Asking {
                message: message.to_string(),
                left: fields.into(),
                given: Vec::new(),
                answer,
            });
        }
        self.put_the_question(whose);
    }

    /// Puts somewhere the agent wants the reader to go to the reader.
    ///
    /// On a card, like everything else it asks -- but a card with nothing
    /// to fill in: what it takes is whether the reader will go, and the URL
    /// itself is what the card is about. Shown whole, folded across as many
    /// rows as it takes, because a URL cut short is a URL nobody can use
    /// and this is the one thing on screen a reader may have to read out.
    fn send_the_reader(
        &mut self,
        whose: Whose,
        message: &str,
        url: &str,
        id: &str,
        answer: acp::Answer<bool>,
    ) {
        self.show_the_question(whose);
        if let Some(talk) = self.talk_mut(whose) {
            talk.going = Some(crate::conversation::Going {
                message: message.to_string(),
                url: url.to_string(),
                id: id.to_string(),
                answer,
            });
        }
        self.put_the_place(whose);
    }

    /// The card for it.
    fn put_the_place(&mut self, whose: Whose) {
        let Some(going) = self.talk(whose).and_then(|talk| talk.going.as_ref()) else {
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
        if let Some(talk) = self.talk_mut(whose) {
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
    fn answer_going(&mut self, whose: Whose, chosen: Option<&str>) {
        let Some(going) = self.talk_mut(whose).and_then(|talk| talk.going.take()) else {
            return;
        };
        if chosen != Some("open") {
            if let Some(talk) = self.talk_mut(whose) {
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
            if let Some(talk) = self.talk_mut(whose) {
                talk.chat.note("Nothing here opens links");
                talk.going = Some(going);
            }
            return;
        }
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = None;
            // A row rather than the card kept open: the agent is no longer
            // waiting on Obelus -- it was told they went -- so the box has
            // to come back. What is left is a thing under way, which is
            // what the transcript already has a shape for.
            talk.chat.away(&going.id, &going.message, &going.url);
        }
        let _ = going.answer.send(true);
    }

    /// Asks the reader to sign in, on a card with the ways the agent offers.
    ///
    /// Only when the agent has said it needs it -- an agent that never asks
    /// is an agent the reader was already signed in to, and a card at every
    /// start would be a question about nothing. What it said goes over the
    /// ways in, and each way's own words beside it.
    ///
    /// An agent that wants a sign-in and offers no way to have one is said
    /// to, and given up on: nothing the reader could press would get them
    /// in, and a card with only "no" on it is a sentence with a key.
    pub(super) fn ask_to_sign_in(&mut self, whose: Whose, why: Option<String>) {
        // Behind whatever the agent is already asking, like any question:
        // put over it, the card answered first was the other one's, and
        // the sign-in went with it.
        if let Some(talk) = self.talk_mut(whose)
            && talk.is_waiting_on_the_reader()
            && talk.signing_in.is_none()
        {
            talk.sign_in_next = Some(why);
            return;
        }
        let Some(talker) = self.talker.as_ref() else {
            return;
        };
        let logins = talker.logins().to_vec();
        let agent = talker.info().unwrap_or("the agent").to_string();
        if logins.is_empty() {
            talker.give_up_signing_in();
            self.in_talk(whose, |chat| {
                chat.note(&format!("There is no way to sign in to {agent} from here"));
            });
            return;
        }
        self.show_the_question(whose);
        let icons = obelus_icons::enabled();
        let mut choices: Vec<Choice> = logins
            .iter()
            .map(|login| Choice {
                id: login.id.clone(),
                name: login.name.clone(),
                about: login.about.clone(),
                // A program to answer, or the agent's own business, which
                // for an agent is a page somewhere else.
                icon: icons.then_some(match login.how {
                    acp::How::Run { .. } => obelus_icons::ui::TERMINAL,
                    acp::How::Asked => obelus_icons::ui::AWAY,
                }),
                chosen: false,
            })
            .collect();
        choices.push(Choice {
            id: NOT_SIGNING_IN.to_string(),
            name: "Not now".to_string(),
            about: None,
            icon: icons.then_some(obelus_icons::ui::STAYING),
            chosen: false,
        });
        let mut card = Card::new(choices, false);
        let about = match &why {
            Some(why) => format!("Sign in to {agent} to go on\n\n{why}"),
            None => format!("Sign in to {agent} to go on"),
        };
        card.about(&about);
        if let Some(talk) = self.talk_mut(whose) {
            talk.signing_in = Some(why);
            talk.card = Some(card);
        }
    }

    /// Signs in the way the reader chose, or gives up.
    ///
    /// A way that is a program goes to a terminal of its own, and the
    /// reader with it; a way that is the agent's is asked of the agent. Not
    /// signing in gives up on whatever was waiting for it, which ends a
    /// conversation that was being opened -- the next thing the reader says
    /// starts the agent again, and it asks again.
    fn answer_signing_in(&mut self, whose: Whose, chosen: Option<&str>) {
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = None;
            talk.signing_in = None;
        }
        let login = chosen.and_then(|id| {
            self.talker
                .as_ref()?
                .logins()
                .iter()
                .find(|login| login.id == id)
                .cloned()
        });
        let (Some(login), Some(connection)) =
            (login, self.talker.as_ref().map(acp::Talk::connection))
        else {
            if let Some(talker) = self.talker.as_ref() {
                talker.give_up_signing_in();
            }
            self.in_talk(whose, |chat| chat.note("Not signed in"));
            return;
        };
        self.in_talk(whose, |chat| {
            chat.note(&format!("Signing in: {}", login.name))
        });
        match login.how {
            acp::How::Run {
                program,
                arguments,
                env,
            } => {
                let Some(conversation) = (match whose {
                    Whose::One(id) => Some(id),
                    Whose::Whoever => self.current,
                }) else {
                    return;
                };
                self.sign_in_by_running(
                    conversation,
                    connection,
                    obelus_terminal::Program::Command {
                        program,
                        arguments,
                        env,
                    },
                );
            }
            acp::How::Asked => {
                if let Some(talker) = self.talker.as_ref() {
                    talker.sign_in(&login.id);
                }
            }
        }
    }

    /// The agent says the far end happened, so there is nothing left to
    /// wait for.
    ///
    /// In whichever conversation was sent there, which the notification
    /// does not say and the row does: it names only the question, and the
    /// reader who went to sign in is rarely still looking at the page they
    /// left from.
    fn went_through(&mut self, id: &str) {
        for document in self.documents.iter_mut().flatten() {
            if let Some(talk) = Document::chat_mut(document) {
                talk.chat.arrived(id);
            }
        }
    }

    /// Puts the next field, or answers the form when there is none left.
    fn put_the_question(&mut self, whose: Whose) {
        let Some(asking) = self.talk(whose).and_then(|talk| talk.asking.as_ref()) else {
            return;
        };
        if asking.left.is_empty() {
            self.settle_asking(whose);
            return;
        }
        // What the card says it is about: the field's own question, and the
        // first time the agent's own words over it -- afterwards the reader
        // is in the middle of the form, and what they need to know is which
        // part of it this is.
        let message = match asking.given.is_empty() {
            true => asking.message.clone(),
            false => String::new(),
        };
        let (choice, words) = self.asked_now(whose);
        let mut card = match &choice {
            Some(field) => card_of(field),
            None => Card::new(Vec::new(), false),
        };
        // Under the message only where the field says something of its own:
        // an agent asking several things at once writes "answer these" there
        // and each question in its field's description, which the message
        // alone left on nothing but the transcript -- and a number says what
        // it will take. Asking one thing, an agent writes the question in the
        // message and the title is a label for it, which under the question
        // would be a word on its own.
        let about = match (message.is_empty(), choice.as_ref().or(words.as_ref())) {
            (true, Some(field)) => question(field),
            (false, Some(field)) if question(field) != field.title => {
                format!("{message}\n\n{}", question(field))
            }
            (_, _) => message,
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
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = Some(card);
        }
        self.mirror_asked(whose);
    }

    /// The fields the card on screen is answering: the one it puts the
    /// question about, and the one the reader writes their own answer in.
    ///
    /// One function rather than two places working it out, because putting
    /// the question and taking the answer have to agree about which fields
    /// were on the card.
    fn asked_now(&self, whose: Whose) -> (Option<acp::Field>, Option<acp::Field>) {
        let Some(asking) = self.talk(whose).and_then(|talk| talk.asking.as_ref()) else {
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
    pub(super) fn answer_card(&mut self, whose: Whose, chosen: &[String], words: Option<&str>) {
        // Somewhere to go is neither a form nor a permission: nothing was
        // filled in, and what the answer decides is whether Obelus opens
        // something.
        if self.talk(whose).is_some_and(|talk| talk.going.is_some()) {
            self.answer_going(whose, chosen.first().map(String::as_str));
            return;
        }
        // Nor is a sign-in: what was chosen is a way in.
        if self
            .talk(whose)
            .is_some_and(|talk| talk.signing_in.is_some())
        {
            self.answer_signing_in(whose, chosen.first().map(String::as_str));
            return;
        }
        // A permission request is named answers and nothing else, so the
        // one they chose is the answer.
        if self
            .talk(whose)
            .is_some_and(|talk| talk.permission.is_some())
        {
            if let Some(talk) = self.talk_mut(whose) {
                talk.card = None;
            }
            match chosen.first() {
                Some(option) => self.allow(whose, option),
                None => self.refuse_permission(whose),
            }
            return;
        }
        let (choice, asked) = self.asked_now(whose);
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
                // asked for is Obelus insisting on its own behalf.
                acp::Takes::Number { .. } if text.is_empty() && !field.required => {}
                acp::Takes::Number { whole, least, most } => {
                    let Some(reply) = self.number_of(whose, field, &text, *whole, *least, *most)
                    else {
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
        // The conversation it was asked in, not the one on screen: a card
        // answered from a chat is in a conversation nobody is looking at.
        let Some(asking) = self.talk_mut(whose).and_then(|talk| talk.asking.as_mut()) else {
            return;
        };
        for _ in 0..taken {
            asking.left.pop_front();
        }
        asking.given.extend(given);
        for line in said {
            if let Some(talk) = self.talk_mut(whose) {
                talk.chat.note(&line);
            }
        }
        // Theirs, in the transcript, because that is what they said -- the
        // agent asked in words and this is the answer in words.
        if let Some(text) = words
            && let Some(talk) = self.talk_mut(whose)
        {
            // A card's answer is words and never a picture, so it is one
            // part and the page says the same as before.
            talk.chat.asked(&[Part::Words(text.to_string())]);
        }
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = None;
        }
        self.put_the_question(whose);
    }

    /// A number the reader typed, if it is one the field will take.
    fn number_of(
        &mut self,
        whose: Whose,
        field: &acp::Field,
        text: &str,
        whole: bool,
        least: Option<f64>,
        most: Option<f64>,
    ) -> Option<acp::Reply> {
        let Ok(number) = text.parse::<f64>() else {
            let title = field.title.clone();
            self.in_talk(whose, |chat| {
                chat.note(&format!("{title} takes a number, not {text:?}"))
            });
            return None;
        };
        if least.is_some_and(|least| number < least) || most.is_some_and(|most| number > most) {
            let asked = question(field);
            self.in_talk(whose, |chat| chat.note(&format!("That is outside {asked}")));
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
    fn settle_asking(&mut self, whose: Whose) {
        let Some(asking) = self.talk_mut(whose).and_then(|talk| talk.asking.take()) else {
            return;
        };
        if asking.answer.send(Some(asking.given)).is_err() {
            self.in_talk(whose, |chat| chat.note("It stopped waiting for an answer"));
        }
    }

    /// Says no to the form, whichever field the reader was on.
    pub(super) fn refuse_asking(&mut self, whose: Whose) {
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = None;
        }
        // Somewhere to go, given up on: the channel going away without an
        // answer is what the agent hears as a cancellation, so there is
        // nothing to send.
        if let Some(talk) = self.talk_mut(whose)
            && talk.going.take().is_some()
        {
            talk.chat.note("Not opened");
            return;
        }
        let Some(asking) = self.talk_mut(whose).and_then(|talk| talk.asking.take()) else {
            return;
        };
        if let Some(talk) = self.talk_mut(whose) {
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
        // And what the box offers, by the opening's own test of whether the
        // next message carries the note -- so a note rewritten since offers
        // it again, and one already told does not. From the kept copy of
        // the notes, because this is asked every frame.
        let suggested = self.conversation().and_then(|talk| {
            let now = super::opening::what_the_note_says(self.the_note_this_is_about()?);
            (talk.told.as_deref() != Some(now.as_str())).then_some(super::opening::LOOK)
        });
        if let Some(talk) = self.conversation_mut() {
            talk.chat.suggest(suggested);
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
    /// A question is routed like everything else, by the conversation it
    /// was asked in. It went to whoever was here once, and where nobody was
    /// a conversation was found or made and put on screen for it: the
    /// reader was taken out of what they were reading to answer a question
    /// on a page that was not having the turn, which then went on with
    /// nothing on screen saying so. What is left going to whoever is here
    /// -- a file to read or write, a command to run -- puts nothing on a
    /// page.
    fn whose(&self, incoming: &acp::Incoming) -> Option<Whose> {
        let named = match incoming {
            acp::Incoming::Update { session, .. }
            | acp::Incoming::Ended { session, .. }
            | acp::Incoming::Remembered { session }
            | acp::Incoming::Asked { session, .. }
            | acp::Incoming::Withdrawn { session }
            | acp::Incoming::SignIn {
                session: Some(session),
                ..
            } => Some(session),
            // A sign-in asked for while a conversation was opening names
            // the request rather than a session, and is routed by it
            // before this is reached; one the agent did itself names
            // nothing, and is said wherever the reader is.
            acp::Incoming::SignIn { session: None, .. }
            | acp::Incoming::SignedIn
            | acp::Incoming::Started { .. }
            | acp::Incoming::Lost { .. }
            | acp::Incoming::Ready { .. }
            // What an agent can be set to is about the agent: the
            // conversation it was asked on was opened for the asking and
            // let go before this arrived.
            | acp::Incoming::Offers { .. }
            | acp::Incoming::Failed(..)
            | acp::Incoming::Gone(_)
            | acp::Incoming::Finished { .. }
            | acp::Incoming::Read { .. }
            | acp::Incoming::Write { .. }
            // A command names no conversation: `CreateTerminalRequest` has
            // a session on it, but the four that follow have only the
            // command's own name, and Obelus runs them for whoever asked.
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

    /// The conversation `whose` names, for the rest of the application.
    pub(super) fn talk_of(&self, whose: Whose) -> Option<&crate::conversation::Conversation> {
        self.talk(whose)
    }

    /// Answers the question up in a conversation with what was replied from
    /// a chat, and puts up whatever was waiting behind it.
    pub(super) fn answer_from_afar(
        &mut self,
        whose: Whose,
        chosen: &[String],
        words: Option<&str>,
    ) {
        match self
            .talk(whose)
            .is_some_and(|talk| talk.permission.is_some())
        {
            true => {
                if let Some(talk) = self.talk_mut(whose) {
                    talk.card = None;
                }
                match chosen.first() {
                    Some(option) => self.allow(whose, option),
                    None => self.refuse_permission(whose),
                }
            }
            false => self.answer_card(whose, chosen, words),
        }
        self.ask_the_next(whose);
    }

    /// Says words that came from a chat in a conversation: now, or when the
    /// turn it is in the middle of is over -- the way the reader's own wait
    /// on the page.
    pub(super) fn say_from_afar(&mut self, whose: Whose, parts: &[Part]) {
        let running = self.talk(whose).is_some_and(|talk| {
            self.talker
                .as_ref()
                .is_some_and(|talker| talker.is_thinking(talk.session.as_ref(), talk.requested))
        });
        if running {
            if let Some(talk) = self.talk_mut(whose) {
                talk.chat.will_say_from_afar(parts);
            }
            return;
        }
        self.about_to_say_from_afar(whose);
        self.say_in(whose, parts, false);
    }

    /// The conversation `whose` names.
    fn talk(&self, whose: Whose) -> Option<&crate::conversation::Conversation> {
        match whose {
            Whose::Whoever => self.conversation(),
            Whose::One(id) => self.document(id).and_then(Document::chat),
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
    pub(super) fn in_talk(
        &mut self,
        whose: Whose,
        what: impl FnOnce(&mut obelus_component::chat::Chat),
    ) {
        if let Some(talk) = self.talk_mut(whose) {
            what(&mut talk.chat);
        }
    }

    /// Whether any command Obelus was asked to run is still going.
    #[must_use]
    pub fn anything_running(&self) -> bool {
        self.runs.anything_running()
    }

    /// Puts what Obelus's commands are doing on the rows that are about
    /// them.
    ///
    /// Every frame, from the runner rather than from anything kept: the
    /// process owns its output, and a row drawn from a copy is a row that
    /// can be a moment behind what the reader is watching.
    ///
    /// This is the half Obelus owes for not asking. The agent decides
    /// whether to ask before running something; Obelus decides that once
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
                    words.push_str("\n\u{2026} and more, which Obelus did not keep");
                }
                // How it ended, where that is not simply well. The mark on
                // the row says a command failed and cannot say what a
                // reader needs next, which is *how*: a `grep` that matched
                // nothing exits 1 and a command that is not installed
                // exits 127, and one of those is an answer and the other
                // is a morning wasted. The number was nowhere on the page
                // -- Obelus kept it, told the agent when it asked, and
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
    /// when it ends, and nothing tells Obelus but asking.
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
            // name left in the file is one Obelus asks for again on the
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
                // Still waiting: the fresh one is the answer to the same
                // request, and arrives with its number.
                talk.opening = talk.requested.is_some();
                // A fresh session has heard none of it, whichever of the
                // two it is: both go back to "not said yet" together, or
                // the half that is left behind is the half never said.
                talk.told = None;
                // And what Obelus had already asked that session for goes
                // with it: the one starting is a new conversation, and it
                // opens on what the reader chose like any other.
                talk.started_on.clear();
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
        // A conversation that could not be opened until the reader signs
        // in. By the request's number, which is the only name it has: the
        // agent refused it the session that would have named it.
        if let acp::Incoming::SignIn {
            session: None,
            asking,
            why,
            ..
        } = &incoming
        {
            let at = asking
                .and_then(|asking| self.conversation_at(|talk| talk.requested == Some(asking)));
            let whose = at.map_or(Whose::Whoever, |at| Whose::One(DocumentId::new(at)));
            self.ask_to_sign_in(whose, why.clone());
            return;
        }
        if let acp::Incoming::Started {
            session, asking, ..
        } = &incoming
        {
            let session = session.clone();
            // The one whose request this answers, by its number -- two of
            // them starting at once is the ordinary case now that opening a
            // conversation asks for its session, and told apart by nothing
            // the first answer went to whichever was first in the list.
            // Then the one that asked for this name, and only then the
            // first that has none, for an answer nothing numbered.
            //
            // Only an answer nothing numbered falls back that far. One whose
            // number nobody holds is about a conversation that has gone --
            // closed before its session came -- and the first with none was
            // a conversation waiting on a request of its own, which it then
            // lost.
            let asked = match asking {
                Some(asking) => {
                    let Some(at) = self.conversation_at(|talk| talk.requested == Some(*asking))
                    else {
                        // Asked for by a conversation that has gone, and
                        // nobody's now: let go on the agent's side as well,
                        // the way one the reader left without a word is.
                        if let Some(talker) = self.talker.as_mut() {
                            talker.let_go(&session);
                        }
                        return;
                    };
                    Some(at)
                }
                None => self
                    .conversation_at(|talk| talk.asked_for.as_ref() == Some(&session))
                    .or_else(|| {
                        self.conversation_at(|talk| {
                            talk.session.is_none() && talk.asked_for.is_none()
                        })
                    }),
            };
            // Whether this one was minted rather than taken up again, which
            // decides whether what it arrives set to is the agent's own
            // answer or whatever that conversation was left on -- and
            // whether it may be let go with nothing said in it. Asked here
            // because `asked_for` is cleared just below.
            let minted = !asked
                .and_then(|at| self.documents.get(at)?.as_ref()?.chat())
                .is_some_and(|talk| talk.asked_for.as_ref() == Some(&session));
            let mine = asked.and_then(|at| {
                self.documents
                    .get_mut(at)
                    .and_then(Option::as_mut)
                    .and_then(Document::chat_mut)
            });
            if let Some(talk) = mine {
                talk.asked_for = None;
                talk.opening = false;
                talk.requested = None;
                talk.minted = minted;
                talk.session = Some(session.clone());
            }
            // The first moment a conversation about nothing in particular
            // can be claimed: it is named by its session and by nothing
            // else, and the session has only now arrived. One being taken
            // up again was claimed when the row was chosen, which is where
            // the collision could happen -- this is the fresh one, whose
            // name no other Obelus can have guessed, being written down as
            // this window's so that tomorrow's list says so.
            let fresh = asked.filter(|at| {
                self.documents
                    .get(*at)
                    .and_then(Option::as_ref)
                    .and_then(Document::chat)
                    .is_some_and(|talk| talk.topic == Topic::Loose && talk.claim.is_none())
            });
            if let Some(at) = fresh {
                let claim = obelus_agent::chats::claim(
                    &self.working_directory,
                    &obelus_agent::chats::ChatId::Loose(session.0.to_string()),
                );
                if let Some(talk) = self
                    .documents
                    .get_mut(at)
                    .and_then(Option::as_mut)
                    .and_then(Document::chat_mut)
                {
                    talk.claim = claim;
                }
            }
            self.remember_the_conversations();
            // After the conversation has its name, not before: what Obelus
            // has already asked this one for is written down on the
            // conversation, and a note written before there is one to
            // write it on is a question asked twice.
            self.hear_what_the_agent_offers(&session);
            self.start_the_session_on_what_was_chosen(&session);
            return;
        }
        // What came back from asking on a conversation of Obelus's own.
        // It names none, because by the time it arrives there is none.
        if let acp::Incoming::Offers { offers, .. } = &incoming {
            // For the agent that answered, which is the one running. Empty
            // included: the answer to `session/new` is the whole of what it
            // offers, and nothing is an answer -- not the silence of an
            // agent that would not say.
            if let Some(talker) = self.talker.as_ref() {
                self.agents.offers = Some((talker.id().to_string(), offers.clone()));
            }
            self.agents.asking = None;
            return;
        }
        // And again whenever the agent says what it offers, which is the
        // message that actually carries it: a session opens before an
        // agent has said a word about what it can be set to.
        if let acp::Incoming::Update {
            session,
            update: acp::Update::Settings(_),
        } = &incoming
        {
            // Only for a session a conversation holds. The one opened to
            // ask what the agent offers can say so before the answer that
            // names it arrives, and before that answer nothing knows it is
            // not a real one -- so this asks the conversations rather
            // than the connection. A real session that says it this early
            // loses nothing: `Started` does both of these itself.
            let session = session.clone();
            if self
                .conversation_at(|talk| talk.session.as_ref() == Some(&session))
                .is_none()
            {
                return;
            }
            self.hear_what_the_agent_offers(&session);
            self.start_the_session_on_what_was_chosen(&session);
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
                    self.mirror_said(whose, &text);
                    self.in_talk(whose, |chat| chat.chunk(Speaker::Agent, &text));
                }
                acp::Update::Thought(text) => {
                    self.in_talk(whose, |chat| chat.chunk(Speaker::Thought, &text))
                }
                // The reader's own words, as the agent has them. What this
                // is for is a conversation taken up again after Obelus was
                // shut: the transcript is the agent's, and this is the half
                // of it Obelus cannot write itself.
                acp::Update::Heard(text) => self.in_talk(whose, |chat| chat.heard(&text)),
                acp::Update::Tool { call, status } => {
                    self.mirror_paused(whose);
                    self.in_talk(whose, |chat| chat.tool(&call, &status));
                    self.hear_where_it_wrote(whose, &call.id);
                }
                // What it means to do about this turn. Not a thing said --
                // it never goes in the transcript -- so it is handed to the
                // row that says what is happening now, which is where a
                // state belongs and where one cannot be left behind.
                acp::Update::Plan(steps) => {
                    self.mirror_planned(whose, &steps);
                    self.in_talk(whose, |chat| chat.planning(steps));
                }
                // What the agent calls this conversation, which is the
                // name it goes by in the list of open documents -- so it is
                // written down rather than only shown.
                acp::Update::Titled(_) => {
                    self.remember_the_conversations();
                    self.mirror_head(whose, None);
                    self.say_the_new_name();
                }
                // Kept by the handle, which is where the view reads them:
                // these are facts about the agent rather than things it
                // said, and a transcript with them in it is a log. The
                // settings are answered above, where the conversation they
                // are about is still named.
                acp::Update::Mode(_)
                | acp::Update::Orders(_)
                | acp::Update::Settings(_)
                | acp::Update::Used(_) => {}
            },
            acp::Incoming::Ended { why: reason, .. } => {
                // Only the ends that are not the ordinary one: a turn that
                // finished has its answer above it, and "end turn" under
                // every answer is noise. Nor a stop made to say something,
                // which the reader's words say (`send_now`).
                let to_say = self
                    .talk_mut(whose)
                    .is_some_and(|talk| std::mem::take(&mut talk.stopped_to_say));
                match reason.as_deref() {
                    Ok("end_turn") => {}
                    Ok("cancelled") if to_say => {}
                    Ok("cancelled") => self.in_talk(whose, |chat| chat.note("Stopped")),
                    Ok("refusal") => {
                        self.in_talk(whose, |chat| chat.note("It declined to answer"));
                    }
                    Ok("max_tokens") => {
                        self.in_talk(whose, |chat| chat.note("It ran out of room to answer in"));
                    }
                    Ok(other) => self.in_talk(whose, |chat| chat.note(other)),
                    Err(why) => self.in_talk(whose, |chat| chat.note(&format!("The agent: {why}"))),
                }
                // A turn that was stopped, ran out of room or failed takes
                // its questions with it: whatever was asking is not coming
                // back for the answer. An ordinary end leaves them, because
                // an agent may ask outside a turn and a question up then is
                // not this turn's -- zed draws the line in the same place.
                if matches!(reason.as_deref(), Ok("cancelled" | "max_tokens") | Err(_)) {
                    self.give_up_the_questions(whose);
                }
                // What it said goes to its thread, if it has one, before
                // anything else happens to the conversation.
                self.mirror_turn_over(whose);
                // And then whatever the reader said while it was running.
                // After the line above and not before it, so that the
                // transcript reads in the order the things happened.
                self.say_what_was_waiting(whose);
            }
            acp::Incoming::Failed(what, why) => {
                tracing::warn!(what, why, "the agent");
                // Asking what it can be set to is over, however it went:
                // the page says it has heard nothing rather than that it
                // is still asking.
                if what == obelus_agent::acp::link::ASKING_WHAT_IT_OFFERS {
                    self.agents.asking = None;
                }
                let said = format!("{what}: {why}");
                match self.conversation_mut() {
                    Some(talk) => talk.chat.note(&said),
                    // Nothing on screen is a conversation, which is the
                    // ordinary case for what Obelus asked on its own
                    // account -- the reader is on the settings page. The
                    // status row rather than nowhere: an agent that will
                    // not answer because nobody has signed in says so
                    // here, and a failure written into a log is a failure
                    // the reader never sees.
                    None => self.wrong(said),
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
            acp::Incoming::Asked { question, .. } => self.put_to_the_reader(whose, question),
            acp::Incoming::Withdrawn { .. } => self.take_back(whose),
            acp::Incoming::Finished { id } => self.went_through(&id),
            // A command the agent asked for. Run without asking the
            // reader -- the agent asks, which is the rule Obelus's own
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
                // And every conversation's session, and every request for
                // one: the process that knew those names has gone, and the
                // next one numbers its requests afresh. Left, a conversation
                // looked settled -- showing it asked for nothing -- and what
                // was typed into it went to a name nobody on the other end
                // had given.
                self.forget_what_the_agent_held();
                self.agents.asking = None;
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
            // Answered above, both of them: one before there is a
            // conversation to name, the other because there is none.
            acp::Incoming::Ready { .. }
            | acp::Incoming::Started { .. }
            | acp::Incoming::Offers { .. } => {}
            // A turn that needed a sign-in, which has ended and said so:
            // the ways in go up after it, in the conversation it was.
            acp::Incoming::SignIn { why, .. } => self.ask_to_sign_in(whose, why),
            // The agent signed the reader in itself, and what was waiting
            // on it is already being asked again.
            acp::Incoming::SignedIn => {
                self.in_talk(whose, |chat| chat.note("Signed in"));
                self.signed_in_everywhere();
            }
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
        self.talker = Some(acp::Talk::start(
            id,
            command,
            arguments,
            &self.working_directory,
            sender,
        ));
    }

    /// Starts the active agent, or says why it cannot.
    pub(super) fn start_agent(&mut self) {
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
                chat.note("This system has nowhere for Obelus to keep an agent")
            });
            return;
        };
        // What the install wrote down when it finished. Nothing here means
        // no install finished -- the reader removed it, or Obelus was shut
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
        // -- which is what says the agent is asking about it. Obelus used
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
        // the reason the line Obelus used to write here was deleted: those
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
            talk.permission = Some(Permission {
                call: call.clone(),
                answer,
            });
            talk.card = Some(card);
        }
        self.mirror_asked(whose);
    }

    /// Answers the permission request the reader chose an option for.
    pub(super) fn allow(&mut self, whose: Whose, option: &str) {
        let Some(answer) = self
            .talk_mut(whose)
            .and_then(|talk| talk.permission.take())
            .map(|asked| asked.answer)
        else {
            return;
        };
        if answer.send(Some(option.to_string())).is_err() {
            self.in_talk(whose, |chat| chat.note("It stopped waiting for an answer"));
        }
    }

    /// Tells the agent the reader would not answer.
    ///
    /// The protocol has an outcome for it, and it matters: an agent whose
    /// request is never answered waits for ever, and one that is told it
    /// was cancelled ends the turn and says so.
    pub(super) fn refuse_permission(&mut self, whose: Whose) {
        if let Some(talk) = self.talk_mut(whose) {
            talk.card = None;
        }
        let Some(answer) = self
            .talk_mut(whose)
            .and_then(|talk| talk.permission.take())
            .map(|asked| asked.answer)
        else {
            return;
        };
        let _ = answer.send(None);
        if let Some(talk) = self.talk_mut(whose) {
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
    /// From an open buffer when Obelus has one, because what the reader is
    /// looking at is not always what is on disk -- and the whole point of an
    /// agent inside a reader is that they are looking at the same thing.
    /// Otherwise from disk.
    ///
    /// Refused outside the project, whichever way the text would have come:
    /// an agent asking for something outside the project Obelus was started on
    /// is asking for something the reader did not open it to look at.
    /// Writes a file for the agent.
    ///
    /// Through the buffer where Obelus has one open, so the reader can undo
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

    /// The project, spelled the way `canonicalize` spells a path.
    ///
    /// Both sides of the fence have to be spelled the same way or it is not
    /// a comparison. On Windows `canonicalize` hands back a verbatim path --
    /// `\\?\E:\work\obelus\...` -- where the working directory is an
    /// ordinary one, so `starts_with` was asking whether a `\\?\E:` prefix
    /// begins with an `E:` one. It does not, ever: every file an agent asked
    /// to read was outside the project, the project's own included, and what
    /// the reader saw was an agent reading nothing and being refused
    /// everything.
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
            tracing::info!(path = %full.display(), "the agent asked to write outside the project");
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
                    self.say("The agent changed this file".to_string());
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

/// Whether the agent has taken back a question still waiting to go up.
fn taken_back(question: &acp::Question) -> bool {
    match question {
        acp::Question::Permission { answer, .. } => answer.is_canceled(),
        acp::Question::Ask { answer, .. } => answer.is_canceled(),
        acp::Question::Open { answer, .. } => answer.is_canceled(),
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
    setting.name_of(value).unwrap_or(value).to_string()
}

/// The lines of `text` an agent asked for.
///
/// `line` is counted from one, which is the protocol's own choice and not
/// Obelus's: the file's first line is line 1.
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

/// What the box holds, as the protocol's own blocks.
///
/// The one place the two vocabularies meet. The box knows pictures and
/// nothing about the wire; the link knows blocks and nothing about a
/// caret; and the order is carried across unchanged, because the order is
/// the thing the reader built.
fn said_of(parts: &[Part]) -> Vec<acp::link::Said> {
    parts
        .iter()
        .map(|part| match part {
            Part::Words(words) => acp::link::Said::Words(words.clone()),
            Part::Picture(picture) => acp::link::Said::Picture(acp::link::Picture {
                mime: picture.mime.clone(),
                bytes: picture.bytes.clone(),
            }),
        })
        .collect()
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
