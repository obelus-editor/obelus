//! One conversation with an agent: what was said, and what it is waiting on.
//!
//! Five things that were five fields on the application, and are one thing:
//! the transcript and the box, the agent's own commands while a list of them
//! is following what is being typed, the form it asked the reader to fill
//! in, the card that form is answered on, and the channel an answer goes
//! back through.
//!
//! They were five because there was one conversation. The moment there are
//! two, four of them are wrong as singletons and one is wrong in a way that
//! loses work: a form the agent is waiting on arrives with a live channel at
//! the other end, and a second conversation putting its own there would
//! leave the first agent waiting for an answer nobody can give it any more.
//!
//! Nothing here talks to an agent. What arrives from one is folded in by
//! [`crate::app`], which is the half that knows there is a process.
//!
//! **A conversation takes one prompt turn at a time, so what the reader
//! says into a running one waits.** The protocol puts no turn on either end
//! of the exchange: `session/cancel` names a session, and the answer to
//! `session/prompt` says the turn is over with nothing on it saying which
//! turn. So two prompts in flight is two answers Obelus cannot tell apart,
//! and the first one home put the conversation back to resting while the
//! other turn worked on -- no `thinking...`, no mark turning, and
//! `interrupt` gated on the same flag, so escape would not even send the
//! cancellation. zed queues for this reason too, and "send it now" there is
//! `cancel` awaited and *then* the prompt, never the two at once. The queue
//! is what makes two turns rare; the turn's number (see `obelus_agent::acp`)
//! is what makes the rare one harmless.
//!
//! **What is waiting is the reader's, so it waits where their words live.**
//! It goes straight into the transcript, dim, one row per thing they said,
//! and enter on one takes that one back into the box. It was a count over
//! the box -- `2 waiting` -- which said how many and never which, and
//! offered nowhere to stand to change their mind; and the key that released
//! it was enter on an empty box, one key that was harmless with words in
//! the box and a stop to a running turn without them, a pair of presses
//! apart.
//!
//! **And it goes as one prompt, not one per turn.** Three things typed into
//! a running turn are one thing the reader is saying -- fix the tests, and
//! the lint, and then commit -- so they are joined with a blank line, which
//! is what the box's own `alt+enter` makes. One per turn meant the agent
//! answered the first without ever seeing the second, and the third did not
//! reach it until two turns had run. The rows stay the rows they were: the
//! page is what the reader said, and Obelus adds to their half of it rather
//! than rewriting it.
//!
//! Stopping the turn puts them back in the box, all of them, joined the way
//! they would have gone. It released them for a while, on the grounds that
//! escape means "stop what the agent is doing" and not "unsay what I said":
//! so one press stopped the turn and started the next, the mark went on
//! turning under `Stopped`, and the second press -- which every reader made
//! -- stopped their own words. In the box they are neither unsaid nor said
//! for them, and enter is the one key that sends. They still go in the
//! order they were typed: a queue that let a later message overtake an
//! earlier one would put their own words to the agent back to front.
//!
//! `ctrl+enter` is the escape and the enter in one press, and through the
//! queue rather than past it: the box joins what is waiting, the turn is
//! stopped, and all of it goes when the stop has ended the turn here.

use obelus_agent::acp;
use obelus_component::{card::Card, chat::Chat, picker::Picker};
use obelus_git::todo::NoteId;

/// What a conversation is about.
///
/// A note, mostly: a note is something the reader wrote down to come back
/// to, and a conversation is something they come back to, so the two are
/// the same shape and pairing them costs nothing. The other kind is the one
/// started with the key, about nothing in particular.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Topic {
    /// Nothing in particular: opened with the key rather than from a note.
    #[default]
    Loose,
    /// One of the project's notes, by the name that outlives its position.
    Note(NoteId),
}

/// A conversation with an agent, whether or not it is on screen.
///
/// Kept rather than opened: a reader moves between what is open all day, and
/// a conversation that started again every time they went to a file would be
/// a conversation nobody could leave for a minute.
#[derive(Debug, Default)]
pub struct Conversation {
    /// What it is about.
    pub topic: Topic,
    /// Which conversation on the agent this is, once it has opened one.
    ///
    /// `None` until then, which is a real state and not a gap: opening the
    /// view starts the process, and a reader types faster than node starts.
    /// And `None` again once a conversation nothing was said in has been
    /// left: its session was let go, and coming back asks for another.
    pub session: Option<acp::SessionId>,
    /// The one it asked the agent to take up again, while it waits.
    ///
    /// Only for a conversation being reopened, where Obelus already knows
    /// the name because it wrote it down. It is not put straight into
    /// `session`, because until the agent has answered the conversation is
    /// still starting and the screen should say so -- and it is kept apart
    /// from "no session yet" because two conversations opening at once would
    /// otherwise be told apart by nothing, and could take each other's.
    pub asked_for: Option<acp::SessionId>,
    /// Whether it has asked for a fresh one and not been answered.
    ///
    /// The other half of [`Self::asked_for`], which says the same thing
    /// about a conversation being taken up by name. Two fields rather than
    /// one, because that one carries the name and a fresh conversation has
    /// no name until the answer brings it.
    ///
    /// What it is for is telling "starting" from "nothing has been asked
    /// for yet": a conversation opened while no agent could be started has
    /// nothing on the way, and a transcript reading `starting...` under a
    /// still mark says the opposite.
    pub opening: bool,
    /// Which request for a session it is waiting on, by the connection's
    /// count, where it is waiting on one.
    ///
    /// What the answer is matched against. Opening a conversation asks for
    /// its session, so two of them can be waiting at once -- the reader
    /// opened one and went straight to another -- and "the first that has
    /// none" handed the first answer to whichever happened to be earlier in
    /// the list of documents.
    pub requested: Option<acp::Asking>,
    /// Whether its session was opened for it here, rather than taken up
    /// again.
    ///
    /// Which decides whether it may be let go when the reader leaves
    /// without a word. A minted one was asked for because the view opened,
    /// and nothing but the reader's saying something makes it theirs; one
    /// taken up again is a conversation they had, and one taken up by
    /// `session/resume` comes back with an empty page -- nothing said in
    /// it, as far as this window can see, and not a thing to delete.
    pub minted: bool,
    /// Whether it has asked for a session since it last came on screen.
    ///
    /// Once a showing, so that an agent that will not start -- not
    /// installed, a process that dies on the way up -- is tried when the
    /// reader comes to the conversation and not again on every frame they
    /// spend looking at why it did not.
    pub asked_while_shown: bool,
    /// What the note said when Obelus last told the agent about it.
    ///
    /// What the agent has been *told*, rather than what Obelus has to say:
    /// the second is worked out from the first every time the reader sends
    /// something, by asking what the note says now. So there is one rule --
    /// tell it what it does not know -- and no flag anybody has to clear.
    ///
    /// `None` for a conversation the agent has been told nothing about,
    /// which is a fresh one, and for one about nothing in particular. Read
    /// from beside the note for a conversation being picked up where it was
    /// left: the agent kept every word of that one, and what told it the
    /// first time is among them.
    pub told: Option<String>,
    /// Whether the agent has been told who it is talking to.
    ///
    /// A bit rather than a fingerprint, which is the difference between
    /// this and [`Self::told`]: what it carries never changes, so there is
    /// nothing to compare it against and no such thing as saying it again
    /// because it is out of date. Said once per conversation, and written
    /// down beside the note like `told` so that picking one up again does
    /// not repeat it.
    pub introduced: bool,
    /// This Obelus's claim on the conversation, while it is open.
    ///
    /// A conversation is not a thing two Obelus may have open at once: the
    /// agent takes one prompt turn at a time and the queue that keeps it to
    /// one lives in a process, so a second process prompting the same
    /// conversation walks straight past it. Held here because here is what
    /// goes when the document does -- a claim given up by hand is a claim
    /// some way out of a conversation forgets.
    ///
    /// `None` for a conversation about no note: there is nothing for two
    /// windows to collide over, because nothing else can name it.
    pub claim: Option<obelus_agent::chats::Claim>,
    /// The checkout the agent last changed a file in, and its branch.
    ///
    /// Learnt from where its changes landed rather than from anything it
    /// says: an agent told to work in a worktree of its own may not, and
    /// one that changes the reader's own checkout is the one most worth
    /// seeing. `None` until it has changed something, which is when there
    /// starts to be a branch to speak of. A session taken up again replays
    /// its calls, so this comes back with them and is written down nowhere.
    ///
    /// The tree is kept beside the branch so that the branch can be asked
    /// again when the repository moves, the way the status row's is. What
    /// tells Obelus that is a watch on the reader's own checkout; an agent's
    /// worktree has none, and is asked again when the agent next changes
    /// something in it.
    pub working_in: Option<(std::path::PathBuf, obelus_git::Head)>,
    /// The session to take up when it is first shown, for one about nothing
    /// in particular that was open when the window last closed.
    ///
    /// Only for that one: a conversation about a note is found again by
    /// the note, and nothing else names a loose one.
    pub to_take_up: Option<String>,
    /// What was said, and what is being typed.
    pub chat: Chat,
    /// The agent's own commands, while one is being typed in the box.
    ///
    /// Its own list rather than the application's picker, because it does
    /// not take the keys: it follows what is being typed and the box keeps
    /// them.
    pub slash: Option<Picker>,
    /// Whether the reader has shut that list on the name they are typing.
    ///
    /// The list is not a state a key opens and a key closes: it is worked
    /// out from the box on every frame, which can say "a name is being
    /// typed" and "none is" and has nowhere to put the third thing --
    /// *a name is being typed and the reader does not want the list*. So
    /// escape writes it down here, and the frame that would otherwise put
    /// the list straight back reads it.
    ///
    /// It lasts as long as the line is still a command's name. Cleared
    /// where the name goes -- the slash rubbed out, a blank after it, the
    /// message sent -- because that is the reader starting again, and a
    /// latch that outlived it would be a list they could never get back.
    /// Not cleared by the next character, which would be escape working
    /// for one keystroke.
    pub slash_shut: bool,
    /// The form the agent asked the reader to fill in, while one is open.
    pub asking: Option<Asking>,
    /// The card whatever the agent asked is answered on.
    ///
    /// One field for both kinds of question it can ask -- a form's field and
    /// a request for permission -- because on screen they are the same
    /// thing: what it wants to know, what the answers are, and room to say
    /// one in your own words where it will take those.
    pub card: Option<Card>,
    /// The permission request waiting on the reader.
    pub permission: Option<Permission>,
    /// Somewhere the agent wants the reader to go, while they have not
    /// said whether they will.
    pub going: Option<Going>,
    /// That the agent wants the reader signed in, while they have not said
    /// which way -- what it said about it, where it said anything.
    pub signing_in: Option<Option<String>>,
    /// What the agent asked while a question was already up, oldest first.
    ///
    /// One card, so one question at a time, and the next goes up when the
    /// one before it is answered. Not the second taking the first one's
    /// place: the connection is no longer held while a question waits, so
    /// an agent can ask again before the first is answered -- two tool
    /// calls side by side, say -- and neither was the reader's to lose.
    pub queued: std::collections::VecDeque<acp::Question>,
    /// Which of the reader's standing choices Obelus has already asked
    /// this conversation for, by the agent's id for the setting.
    ///
    /// Once each, and never again. What is in here is not "this setting is
    /// on that value" -- it is "Obelus has said its piece about this one"
    /// -- and the difference is the whole point: an agent that refuses a
    /// value, or that puts one back mid-turn, has answered, and Obelus
    /// asking again would be Obelus arguing with it.
    ///
    /// A set rather than a flag on the conversation, because the settings
    /// do not all arrive at once: choosing a model can bring a thinking
    /// level into being that was not there when the conversation opened,
    /// and that one has not been asked for yet.
    pub started_on: std::collections::BTreeSet<String>,
    /// Which of the reader's choices it has said the agent no longer
    /// offers: the agent's id for the setting, and the value.
    ///
    /// The value too, because a reader who chooses again and finds that
    /// gone as well is owed the second sentence.
    ///
    /// Kept apart from `started_on` because the two last differently: a
    /// conversation nothing has been said in has its session let go when
    /// the reader leaves and gets another when they come back, and what was
    /// asked of the last one goes with it -- while the sentence is about the
    /// reader's settings, which have not moved, and said again it is the
    /// same line down the page once for every visit.
    pub said_not_offered: std::collections::BTreeSet<(String, String)>,
    /// Whether the turn running now was stopped to say what is waiting,
    /// rather than stopped.
    ///
    /// Written down by the key rather than worked out from what is waiting
    /// when the turn ends: a reader who presses escape and types before the
    /// stop has landed has words waiting too, and they asked for a stop.
    pub stopped_to_say: bool,
}

impl Conversation {
    /// What claims it, which is also what names it wherever it is written
    /// down.
    ///
    /// One about nothing in particular is named by its session, because
    /// nothing else names it: a note is a thing in the project that outlives
    /// the conversation, and this one has only the agent's word for it that
    /// it exists. So one that has no session yet -- none had, none asked
    /// for, none waiting to be taken up -- has no name.
    #[must_use]
    pub fn which(&self) -> Option<obelus_agent::chats::ChatId> {
        match &self.topic {
            Topic::Note(note) => Some(obelus_agent::chats::ChatId::Note(note.clone())),
            Topic::Loose => self
                .session
                .as_ref()
                .or(self.asked_for.as_ref())
                .map(|session| session.0.to_string())
                .or_else(|| self.to_take_up.clone())
                .map(obelus_agent::chats::ChatId::Loose),
        }
    }

    /// Whether the reader has words in it that have not been answered:
    /// something in the box, or something said that waits for the turn.
    ///
    /// Which is what keeps a conversation open that the agent has asked
    /// to close. A box is the reader's once they have put something in
    /// it, and closing the conversation would take it with it.
    #[must_use]
    pub fn has_the_readers_words(&self) -> bool {
        let writing = self.chat.writing();
        !writing.is_blank() || writing.has_pictures() || !self.chat.unsent().is_empty()
    }

    /// Whether the agent is waiting on the reader in this one.
    #[must_use]
    pub fn is_waiting_on_the_reader(&self) -> bool {
        self.permission.is_some()
            || self.asking.is_some()
            || self.going.is_some()
            || self.signing_in.is_some()
    }

    /// Whether this is a new conversation nobody has said anything in yet.
    ///
    /// About nothing in particular, and not one taken up again: those can
    /// come back with an empty page -- an agent that resumes rather than
    /// replays sends none of it -- and are the reader's all the same. One
    /// taken up is waiting on the name it asked for, or holds a session it
    /// did not mint.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.topic == Topic::Loose
            && !self.chat.anything_said()
            && self.asked_for.is_none()
            && (self.session.is_none() || self.minted)
    }
}

/// A permission request on the card: what it is about, and where the
/// answer goes.
///
/// The call as well as the channel, because an agent may take the question
/// back without saying what became of the call, and a call left waiting
/// turns for as long as the turn goes on -- so the one on the card is marked
/// as stopped the way the ones queued behind it are.
#[derive(Debug)]
pub struct Permission {
    /// The call it asks about.
    pub call: acp::Call,
    /// Where the answer goes: the option chosen, or nothing for no answer.
    pub answer: acp::Answer<Option<String>>,
}

/// A place on the web the agent wants the reader to go: to sign in
/// somewhere, to authorise something.
///
/// Not a form, and not a question in the sense the card usually puts:
/// there is nothing to fill in, and the answer is "I went" or "I will
/// not". Held apart from [`Asking`] for that reason -- one field would be
/// two different things with a `match` on which, and every reader of it
/// would have to do the matching.
#[derive(Debug)]
pub struct Going {
    /// What the agent said it is for, in its own words.
    pub message: String,
    /// Where. Checked before it reached here: `http` or `https`, with a
    /// host, and nothing a launcher would read as two arguments.
    pub url: String,
    /// The agent's own name for the question, which is how it later says
    /// the far end happened.
    pub id: String,
    /// Where the answer goes. `true` once the reader has been sent, which
    /// is what the agent asked for -- it watches the far end itself.
    pub answer: acp::Answer<bool>,
}

/// A form an agent asked the reader to fill in.
///
/// One field at a time, in the order the agent listed them: a list where the
/// answer is one of a few, the box where it is words. What has been answered
/// is kept here until the last field is, because the protocol takes the
/// whole form as one answer.
#[derive(Debug)]
pub struct Asking {
    /// What the agent said the form is about, in its own words.
    ///
    /// Kept because it belongs above whichever question is showing rather
    /// than in a line of its own: a form is one question with several parts,
    /// and saying what it is about twice is saying it once too often.
    pub message: String,
    /// The fields nobody has answered yet, the next one first.
    pub left: std::collections::VecDeque<acp::Field>,
    /// What has been answered, in the order it was.
    pub given: Vec<(String, acp::Reply)>,
    /// Where the answers go when the last one is in.
    pub answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
}
