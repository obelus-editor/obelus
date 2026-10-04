//! A conversation and its thread: the one in this window, the other in the
//! chat it can be reached from, saying the same things.
//!
//! **One conversation is one thread, and always the same one.** Opened the
//! moment the conversation can be named while the chat is connected -- a
//! note's at once, one about nothing in particular when its session arrives
//! -- and kept by that name in a table beside the platform's other state, so
//! that a conversation closed and taken up again, or a window started again,
//! goes on in the thread it had.
//!
//! **Both halves of what is said are in both places.** The agent's words go
//! to the thread a stretch at a time -- whatever it said before it went off
//! to call something, and what it said last when the turn is over -- rather
//! than as it types, which in a chat is a message edited forty times or
//! forty messages; and only the end of the turn calls the reader. So do the
//! questions it asks, as cards the platform draws. What the reader
//! types here goes there too, marked as said on this machine; what they say
//! there arrives here like anything they typed, with a line in front of it for
//! the agent saying where it came from. What the agent's tools did does not go:
//! a chat is not a transcript, and a run of calls is the part of a turn nobody
//! reads on a phone.
//!
//! **The card is the answer.** A question goes to the thread as a card the
//! platform draws, and a press on it comes back as the ids chosen and the
//! words written, held to the same counts as the card here and taken as if
//! pressed here; a reply in words while it is up is pointed back at it, and
//! once it is answered, here or there, the next reply is talking again.
//! Reading words as an answer was Obelus deciding what the reader meant --
//! `1，3` was no number until it learnt Chinese punctuation -- and taking the
//! question back so that the agent could read them was a turn stopped under
//! it, which the agent talked about and whose words it ran into the answer.

use std::collections::{BTreeMap, BTreeSet};

use obelus_agent::chats::ChatId;
use obelus_component::composer::Part;
use obelus_remote::model::{Head, Out, Question, Turning};

use super::*;
use crate::conversation::{Conversation, Topic};

/// What goes in front of words that came from the chat, for the agent.
const AFAR: &str = include_str!("afar.txt");

/// What this window keeps about the threads its conversations are.
#[derive(Debug, Default)]
pub(super) struct Mirror {
    /// Which thread each conversation is, by the conversation's name.
    threads: BTreeMap<String, Thread>,
    /// Which platform `threads` was read for: the table is the platform's.
    read_for: Option<&'static str>,
    /// Threads asked for and not yet there: the number asked with, and the
    /// conversation.
    opening: BTreeMap<u64, String>,
    /// The last number handed out.
    asked: u64,
    /// What was to be said in a thread still being opened, in order.
    held: BTreeMap<String, Vec<Saying>>,
    /// The question up in each conversation, by the number it was put to
    /// the thread with, while it is up.
    questions: BTreeMap<String, u64>,
    /// What the agent has said in each conversation's turn so far.
    this_turn: BTreeMap<String, String>,
    /// Where the words about to go to each conversation's agent came from,
    /// said the moment before they go and gone the moment after: never
    /// kept for words still waiting, which may be taken back or joined by
    /// the reader's own -- those carry where they came from themselves.
    from_afar: BTreeMap<String, Origin>,
    /// What each thread was last said to be, so that it is said again only
    /// when something on it has moved.
    heads: BTreeMap<String, Head>,
    /// The group the threads are in, which pairing chose: read with the
    /// table, and changed only by pairing again.
    room: Option<String>,
    /// The name each conversation last went by, by document. One about
    /// nothing in particular is named by its session, and a session goes
    /// with the agent that had it: until the next one arrives it has no
    /// name, and then a new one. What is kept under the old name moves to
    /// the new -- its thread first -- and meanwhile it is found by this.
    named: BTreeMap<usize, String>,
    /// Conversations whose thread would not open, asked for again once the
    /// connection is up again rather than on the next frame: a platform
    /// that refused once -- a permission the app lacks -- refuses every
    /// time, and one that could not be reached is worth asking when it can.
    unopened: BTreeSet<String>,
    /// Conversations begun by a thread the reader started, by document,
    /// and that thread: theirs from the start, and named once the session
    /// arrives.
    starting: BTreeMap<usize, String>,
}

/// Something to say in a thread, before it is known which.
#[derive(Debug)]
enum Saying {
    /// Words, and whether to call the reader.
    Words(String, bool),
    /// A question, by its number.
    Ask(u64, Question),
    /// What became of one.
    Settle(u64, String),
}

impl Saying {
    /// Said in this thread.
    fn in_thread(self, room: String, thread: String, to: String) -> Out {
        match self {
            Self::Words(text, notify) => Out::Say {
                room,
                thread,
                to,
                text,
                notify,
            },
            Self::Ask(asked, question) => Out::Ask {
                room,
                thread,
                to,
                asked,
                question,
            },
            Self::Settle(asked, said) => Out::Settle {
                room,
                thread,
                to,
                asked,
                said,
            },
        }
    }
}

/// Where words going to an agent came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The chat, all of them.
    Afar,
    /// Both, so the reader is at the machine.
    Mixed,
}

/// One conversation's thread: what the platform calls it, the group it is
/// in, and whom it is with.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Thread {
    thread: String,
    room: String,
    to: String,
    /// Whether Obelus started it, with a head it can say again. One the
    /// reader started is headed by their own first words, which are theirs
    /// and not Obelus's to change.
    own: bool,
}

/// A card's question as a chat is given it.
fn question_of(card: &obelus_component::card::Card) -> Question {
    Question {
        about: card.what_about().unwrap_or_default().trim().to_string(),
        choices: card
            .choices()
            .iter()
            .map(|choice| (choice.id.clone(), choice.name.clone()))
            .collect(),
        several: card.several(),
        needed: card.needed(),
        words: card
            .placeholder()
            .map(|placeholder| (placeholder.to_string(), card.words_needed())),
    }
}

/// What a conversation is called when it is talked about anywhere but here.
fn chat_of(talk: &Conversation) -> Option<ChatId> {
    match &talk.topic {
        Topic::Note(note) => Some(ChatId::Note(note.clone())),
        Topic::Loose => talk
            .session
            .as_ref()
            .map(|session| ChatId::Loose(session.0.to_string())),
    }
}

/// Where a platform's threads are written down.
fn table_for(platform: &str) -> Option<std::path::PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("remote")
            .join(platform)
            .join("threads.toml"),
    )
}

/// The table, read: a conversation's name to its thread and whose it is.
fn read_the_table(platform: &str) -> BTreeMap<String, Thread> {
    let Some(text) = table_for(platform).and_then(|path| std::fs::read_to_string(path).ok()) else {
        return BTreeMap::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        tracing::warn!(platform, "the table of threads will not read");
        return BTreeMap::new();
    };
    table
        .into_iter()
        .filter_map(|(chat, kept)| {
            let said = |key: &str| kept.get(key)?.as_str().map(str::to_string);
            Some((
                chat,
                Thread {
                    thread: said("thread")?,
                    // A thread from before there were rooms was in a direct
                    // message, which nothing is heard in any more.
                    room: said("room")?,
                    to: said("to")?,
                    // A table from before there was any other kind.
                    own: kept
                        .get("own")
                        .and_then(toml::Value::as_bool)
                        .unwrap_or(true),
                },
            ))
        })
        .collect()
}

/// And written, beside and renamed over, the way every file Obelus keeps
/// is: another window may be reading it.
fn write_the_table(platform: &str, threads: &BTreeMap<String, Thread>) {
    let Some(path) = table_for(platform) else {
        return;
    };
    let table: toml::Table = threads
        .iter()
        .map(|(chat, thread)| {
            let mut kept = toml::Table::new();
            kept.insert("thread".to_string(), thread.thread.clone().into());
            kept.insert("room".to_string(), thread.room.clone().into());
            kept.insert("to".to_string(), thread.to.clone().into());
            kept.insert("own".to_string(), thread.own.into());
            (chat.clone(), toml::Value::Table(kept))
        })
        .collect();
    let written = (|| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        let beside = path.with_extension(format!("toml.{}", std::process::id()));
        std::fs::write(&beside, table.to_string()).ok()?;
        std::fs::rename(&beside, &path).ok()
    })();
    if written.is_none() {
        tracing::warn!(platform, "the table of threads was not written");
    }
}

/// Where the group a platform's threads are in is written down.
fn room_file(platform: &str) -> Option<std::path::PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("remote")
            .join(platform)
            .join("room.toml"),
    )
}

/// The group the threads are in, once somebody has paired in one.
fn read_the_room(platform: &str) -> Option<String> {
    let text = std::fs::read_to_string(room_file(platform)?).ok()?;
    let table = text.parse::<toml::Table>().ok()?;
    table.get("room")?.as_str().map(str::to_string)
}

/// And written, beside and renamed over.
fn write_the_room(platform: &str, room: &str) {
    let Some(path) = room_file(platform) else {
        return;
    };
    let mut table = toml::Table::new();
    table.insert("room".to_string(), room.into());
    let written = (|| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        let beside = path.with_extension(format!("toml.{}", std::process::id()));
        std::fs::write(&beside, table.to_string()).ok()?;
        std::fs::rename(&beside, &path).ok()
    })();
    if written.is_none() {
        tracing::warn!(platform, "the room was not written");
    }
}

impl App {
    /// Whom the threads are with: the first person let in, who is the
    /// reader -- the list exists so that it can be more than one machine of
    /// theirs, not more than one person.
    fn whom(&self) -> Option<String> {
        let platform = self.platform()?;
        Some(
            self.config()
                .remote_of(platform.key)?
                .people
                .first()?
                .id
                .clone(),
        )
    }

    /// Opens a thread for every conversation that can be named and has
    /// none, while the chat is connected.
    ///
    /// Once a frame, from what is open: a conversation can become nameable
    /// in half a dozen ways -- opened on a note, its session arriving,
    /// taken up again -- and each of them is a place to forget. What is
    /// asked of every one is a lookup; the work is only for one that has
    /// no thread, which is a conversation that has just begun.
    pub(super) fn settle_the_threads(&mut self) {
        if !self.chat_is_listening() {
            return;
        }
        let (Some(platform), Some(room), Some(to)) =
            (self.platform(), self.the_room(), self.whom())
        else {
            return;
        };
        self.carry_the_names(platform.key);
        self.adopt_what_the_reader_started(platform.key, &room, &to);
        let wanting: Vec<String> = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .filter_map(chat_of)
            .map(|chat| chat.file_name())
            .filter(|chat| {
                !self.mirror.threads.contains_key(chat)
                    && !self.mirror.opening.values().any(|opening| opening == chat)
                    && !self.mirror.unopened.contains(chat)
            })
            .collect();
        if wanting.is_empty() {
            return;
        }
        let notes = obelus_git::todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default();
        for chat in wanting {
            let Some(talk) = self.talk_named(&chat) else {
                continue;
            };
            let head = self.head_of(talk, &notes, None);
            // What the agent last said, under the head: a thread opened on a
            // conversation that was already going would otherwise start
            // halfway through it with nothing to say where.
            let lately = talk.chat.lately();
            self.mirror.asked += 1;
            let asked = self.mirror.asked;
            self.mirror.opening.insert(asked, chat.clone());
            if let Some(lately) = lately {
                self.mirror
                    .held
                    .entry(chat.clone())
                    .or_default()
                    .push(Saying::Words(lately, false));
            }
            self.mirror.heads.insert(chat.clone(), head.clone());
            self.say_to(Out::Open {
                asked,
                room: room.clone(),
                head,
            });
        }
    }

    /// What a thread says it is: the conversation's name, the project and
    /// the branch its agent works on, and where its turn has got to.
    fn head_of(
        &self,
        talk: &Conversation,
        notes: &obelus_git::todo::Todo,
        state: Option<Turning>,
    ) -> Head {
        let title = self
            .conversation_name(talk, notes)
            .unwrap_or_else(|| "A conversation".to_string());
        let mut place = self
            .working_directory
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some((_, branch)) = &talk.working_in {
            place.push_str(" \u{b7} ");
            place.push_str(match branch {
                obelus_git::Head::Branch(name) => name,
                obelus_git::Head::Detached => "Detached",
            });
        }
        Head {
            title,
            place,
            state,
        }
    }

    /// Says what the thread of the conversation `whose` names is again,
    /// where any of it has moved: `state` where the turn has, `None` to
    /// keep the one it had -- a new name or a branch moves nothing else.
    ///
    /// Asked at the moments something moves rather than once a frame: the
    /// name needs the notes, which are a file, and the moments are few --
    /// a turn starting, a question, an answer, an end, a title, a branch.
    pub(super) fn mirror_head(&mut self, whose: talking::Whose, state: Option<Turning>) {
        if !self.chat_is_listening() {
            return;
        }
        let Some(chat) = self.chat_named(whose) else {
            return;
        };
        let Some(talk) = self.talk_of(whose) else {
            return;
        };
        let notes = obelus_git::todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default();
        let kept = self.mirror.heads.get(&chat).and_then(|head| head.state);
        let head = self.head_of(talk, &notes, state.or(kept));
        if self.mirror.heads.get(&chat) == Some(&head) {
            return;
        }
        self.mirror.heads.insert(chat.clone(), head.clone());
        // Kept even where the thread is not there yet -- the reader's first
        // words go out the moment they press enter, and the platform has
        // not answered with the thread by then -- and said once it is: see
        // `thread_opened`.
        self.retitle(&chat, head);
    }

    /// What this window keeps about the platform's threads, read when the
    /// platform is not the one it was read for.
    fn settle_the_mirror(&mut self) {
        let Some(platform) = self.platform() else {
            return;
        };
        if self.mirror.read_for != Some(platform.key) {
            self.mirror = Mirror {
                threads: read_the_table(platform.key),
                room: read_the_room(platform.key),
                read_for: Some(platform.key),
                ..Mirror::default()
            };
        }
    }

    /// The group the threads are in, where somebody has paired in one.
    pub(super) fn the_room(&mut self) -> Option<String> {
        self.settle_the_mirror();
        self.mirror.room.clone()
    }

    /// Takes the group somebody paired in as the room.
    pub(super) fn keep_the_room(&mut self, room: &str) {
        self.settle_the_mirror();
        if let Some(platform) = self.platform() {
            write_the_room(platform.key, room);
            self.mirror.room = Some(room.to_string());
        }
    }

    /// Says a thread's head again, where it has one Obelus can say.
    fn retitle(&self, chat: &str, head: Head) {
        if let Some(Thread {
            thread,
            room,
            own: true,
            ..
        }) = self.mirror.threads.get(chat).cloned()
        {
            self.say_to(Out::Retitle { room, thread, head });
        }
    }

    /// Writes down the threads the reader started, once the conversations
    /// they began are named -- which for one about nothing in particular is
    /// when its session arrives.
    fn adopt_what_the_reader_started(&mut self, platform: &'static str, room: &str, to: &str) {
        let named: Vec<(usize, String, String)> = self
            .mirror
            .starting
            .iter()
            .filter_map(|(at, thread)| {
                let chat = self
                    .documents
                    .get(*at)?
                    .as_ref()
                    .and_then(Document::chat)
                    .and_then(chat_of)?
                    .file_name();
                Some((*at, thread.clone(), chat))
            })
            .collect();
        if named.is_empty() {
            return;
        }
        for (at, thread, chat) in named {
            self.mirror.starting.remove(&at);
            self.mirror.threads.insert(
                chat,
                Thread {
                    thread,
                    room: room.to_string(),
                    to: to.to_string(),
                    own: false,
                },
            );
        }
        write_the_table(platform, &self.mirror.threads);
    }

    /// The conversation open here by its name elsewhere.
    fn talk_named(&self, chat: &str) -> Option<&Conversation> {
        self.document_named(chat)
            .and_then(|id| self.document(id))
            .and_then(Document::chat)
    }

    /// Which document that is.
    fn document_named(&self, chat: &str) -> Option<DocumentId> {
        (0..self.documents.len())
            .find(|at| self.name_of_document(*at).as_deref() == Some(chat))
            .map(DocumentId::new)
    }

    /// The name a document's conversation goes by: its own, or the last
    /// it had while it is between sessions.
    fn name_of_document(&self, at: usize) -> Option<String> {
        let talk = self.documents.get(at)?.as_ref()?.chat()?;
        chat_of(talk)
            .map(|chat| chat.file_name())
            .or_else(|| self.mirror.named.get(&at).cloned())
    }

    /// Moves what is kept under a conversation's old name to its new one,
    /// and lets go of the names of documents that are no longer
    /// conversations. Once a frame, and a lookup apiece.
    fn carry_the_names(&mut self, platform: &'static str) {
        let mut moved = false;
        for at in 0..self.documents.len() {
            let Some(talk) = self.documents[at].as_ref().and_then(Document::chat) else {
                self.mirror.named.remove(&at);
                continue;
            };
            let Some(now) = chat_of(talk).map(|chat| chat.file_name()) else {
                continue;
            };
            match self.mirror.named.insert(at, now.clone()) {
                Some(was) if was != now => {
                    if !self.mirror.threads.contains_key(&now)
                        && let Some(thread) = self.mirror.threads.remove(&was)
                    {
                        tracing::info!(was, now, "a conversation's thread goes by its new name");
                        self.mirror.threads.insert(now.clone(), thread);
                        moved = true;
                    }
                    if let Some(head) = self.mirror.heads.remove(&was) {
                        self.mirror.heads.insert(now.clone(), head);
                    }
                    if let Some(said) = self.mirror.this_turn.remove(&was) {
                        self.mirror.this_turn.insert(now.clone(), said);
                    }
                    if let Some(asked) = self.mirror.questions.remove(&was) {
                        self.mirror.questions.insert(now.clone(), asked);
                    }
                    if let Some(origin) = self.mirror.from_afar.remove(&was) {
                        self.mirror.from_afar.insert(now, origin);
                    }
                }
                _ => {}
            }
        }
        if moved {
            write_the_table(platform, &self.mirror.threads);
        }
    }

    /// The name elsewhere of the conversation `whose` names.
    fn chat_named(&self, whose: talking::Whose) -> Option<String> {
        let at = match whose {
            talking::Whose::One(id) => Some(id.get()),
            talking::Whose::Whoever => self.current.map(DocumentId::get),
        };
        self.name_of_document(at?)
    }

    /// A thread that would not open: what was held for it let go, and the
    /// conversation left to ask again on the next connection.
    pub(super) fn thread_unopened(&mut self, asked: u64, waited: bool) {
        if let Some(chat) = self.mirror.opening.remove(&asked) {
            tracing::info!(chat, waited, "its thread would not open");
            self.mirror.held.remove(&chat);
            // Said in the conversation, which is where the reader is when it
            // matters: the one place that could have said it otherwise was a
            // word on the header that every other conversation also had.
            // And said as what it was: a thread let go of here, while the
            // connection was down too long, is nothing the platform did.
            if let (Some(id), Some(platform)) = (self.document_named(&chat), self.platform()) {
                let said = match waited {
                    true => format!(
                        "{} was not reached in time to start a thread for this conversation",
                        platform.name
                    ),
                    false => format!(
                        "{} would not start a thread for this conversation",
                        platform.name
                    ),
                };
                self.in_talk(talking::Whose::One(id), |talk| talk.note(&said));
            }
            self.mirror.unopened.insert(chat);
        }
    }

    /// Lets the threads that would not open be asked for again.
    pub(super) fn threads_may_open_again(&mut self) {
        self.mirror.unopened.clear();
    }

    /// What was on its way to a connection that is being let go: threads
    /// asked for that it will never answer, and what was held for them.
    /// The next connection asks for them again, those that would not open
    /// included.
    pub(super) fn forget_what_was_on_its_way(&mut self) {
        self.mirror.opening.clear();
        self.mirror.held.clear();
        self.mirror.unopened.clear();
    }

    /// A thread the platform has opened: written down, and what was held for
    /// it said in it.
    pub(super) fn thread_opened(&mut self, asked: u64, thread: String) {
        let Some(chat) = self.mirror.opening.remove(&asked) else {
            return;
        };
        // Taken now, so that a thread that cannot be written down does not
        // keep what was held for it to say in the next one.
        let held = self.mirror.held.remove(&chat).unwrap_or_default();
        let (Some(platform), Some(room), Some(to)) =
            (self.platform(), self.the_room(), self.whom())
        else {
            return;
        };
        self.mirror.threads.insert(
            chat.clone(),
            Thread {
                thread,
                room,
                to,
                own: true,
            },
        );
        write_the_table(platform.key, &self.mirror.threads);
        // What the head became while the thread was on its way -- a turn
        // started, a name given -- said now there is one to say it on.
        if let Some(head) = self.mirror.heads.get(&chat).cloned() {
            self.retitle(&chat, head);
        }
        for saying in held {
            self.say_to_thread(&chat, saying);
        }
    }

    /// Says something in a conversation's thread, or holds it for the
    /// thread that is on its way.
    fn say_in_thread(&mut self, chat: &str, text: String, notify: bool) {
        self.say_to_thread(chat, Saying::Words(text, notify));
    }

    /// The same, for anything that can be said there.
    fn say_to_thread(&mut self, chat: &str, saying: Saying) {
        if let Some(Thread {
            thread, room, to, ..
        }) = self.mirror.threads.get(chat).cloned()
        {
            self.say_to(saying.in_thread(room, thread, to));
            return;
        }
        if self.mirror.opening.values().any(|opening| opening == chat) {
            self.mirror
                .held
                .entry(chat.to_string())
                .or_default()
                .push(saying);
        }
    }

    /// The same, for the conversation `whose` names, where there is a
    /// connection to send it on.
    fn mirror_in(&mut self, whose: talking::Whose, text: String, notify: bool) {
        if !self.chat_is_listening() {
            return;
        }
        if let Some(chat) = self.chat_named(whose) {
            self.say_in_thread(&chat, text, notify);
        }
    }

    /// Keeps what the agent said, for the end of its turn.
    pub(super) fn mirror_said(&mut self, whose: talking::Whose, text: &str) {
        if !self.chat_is_listening() {
            return;
        }
        if let Some(chat) = self.chat_named(whose) {
            self.mirror
                .this_turn
                .entry(chat)
                .or_default()
                .push_str(text);
        }
    }

    /// The agent has gone to do something: what it said before it went goes
    /// to the thread now, quietly. A turn that takes minutes was minutes of
    /// nothing there while its words waited for the end; a call is where
    /// the agent stops talking, so what it said up to one is said whole.
    pub(super) fn mirror_paused(&mut self, whose: talking::Whose) {
        let Some(chat) = self.chat_named(whose) else {
            return;
        };
        let said = self.mirror.this_turn.remove(&chat).unwrap_or_default();
        if !said.trim().is_empty() {
            self.mirror_in(whose, said.trim().to_string(), false);
        }
    }

    /// The turn is over: what the agent said since its last call goes to the
    /// thread, and the reader is called -- this is the moment it wants them.
    pub(super) fn mirror_turn_over(&mut self, whose: talking::Whose) {
        let Some(chat) = self.chat_named(whose) else {
            return;
        };
        let said = self.mirror.this_turn.remove(&chat).unwrap_or_default();
        let said = said.trim();
        if !said.is_empty() {
            self.mirror_in(whose, said.to_string(), true);
        }
        self.mirror_head(whose, Some(Turning::Done));
    }

    /// The question now up, in words, in the thread -- calling the reader,
    /// because the agent is waiting on them.
    pub(super) fn mirror_asked(&mut self, whose: talking::Whose) {
        let Some(question) = self
            .talk_of(whose)
            .and_then(|talk| talk.card.as_ref())
            .map(question_of)
        else {
            return;
        };
        // What it said before it asked goes out before the question does:
        // an answer here would be to a question nobody there had read yet.
        self.mirror_paused(whose);
        if self.chat_is_listening()
            && let Some(chat) = self.chat_named(whose)
        {
            self.mirror.asked += 1;
            let asked = self.mirror.asked;
            self.mirror.questions.insert(chat.clone(), asked);
            self.say_to_thread(&chat, Saying::Ask(asked, question));
        }
        self.mirror_head(whose, Some(Turning::Waiting));
    }

    /// Says in the thread what became of the question up in it, which
    /// closes its card where it had one.
    fn settle_the_question(&mut self, whose: talking::Whose, said: String) {
        if !self.chat_is_listening() {
            return;
        }
        let Some(chat) = self.chat_named(whose) else {
            return;
        };
        match self.mirror.questions.remove(&chat) {
            Some(asked) => self.say_to_thread(&chat, Saying::Settle(asked, said)),
            None => self.say_in_thread(&chat, said, false),
        }
    }

    /// Says in the thread that a question was answered on this machine.
    pub(super) fn mirror_answered_here(&mut self, whose: talking::Whose, said: &str) {
        let said = match said.trim().is_empty() {
            true => "\u{2714} Answered on this machine".to_string(),
            false => format!("\u{2714} Answered on this machine: {said}"),
        };
        self.settle_the_question(whose, said);
        self.mirror_head(whose, Some(Turning::Working));
    }

    /// Says in the thread that the agent stopped waiting.
    pub(super) fn mirror_withdrawn(&mut self, whose: talking::Whose) {
        self.settle_the_question(whose, "The agent stopped asking.".to_string());
    }

    /// Whether the conversation a thread is has a question up, for a test
    /// that cannot put a conversation begun from the chat on the screen.
    #[must_use]
    pub fn asking_in_thread_for_test(&self, thread: &str) -> bool {
        self.mirror
            .threads
            .iter()
            .find(|(_, kept)| kept.thread == thread)
            .and_then(|(chat, _)| self.talk_named(chat))
            .is_some_and(|talk| talk.card.is_some())
    }

    /// Somebody pressed something on a question's card: taken as the
    /// answer where it is the question still up and the card here holds an
    /// answer to it, and said in the thread why not where it does not -- the
    /// card there stays open for another go.
    pub(super) fn answered_on_a_card(
        &mut self,
        asked: u64,
        chosen: &[String],
        words: Option<&str>,
    ) {
        let Some(chat) = self
            .mirror
            .questions
            .iter()
            .find(|(_, now)| **now == asked)
            .map(|(chat, _)| chat.clone())
        else {
            // Answered already, here or by a press before this one.
            tracing::info!(asked, "a press on a question no longer asked");
            return;
        };
        let Some(id) = self.document_named(&chat) else {
            return;
        };
        let whose = talking::Whose::One(id);
        let Some(card) = self.talk_of(whose).and_then(|talk| talk.card.clone()) else {
            return;
        };
        if let Err(why) = card.takes(chosen, words) {
            self.say_in_thread(&chat, why, false);
            return;
        }
        let said: Vec<String> = chosen
            .iter()
            .map(|id| card.name_of(id).unwrap_or(id).to_string())
            .chain(words.map(str::to_string))
            .collect();
        self.answer_from_afar(whose, chosen, words);
        // Closed only once it is taken, which is known by what came after:
        // the same question still up is one that was not -- a number it
        // would not take, which the page here says why -- and closing it
        // left the reader there with a card that said it was answered.
        let still = self.mirror.questions.get(&chat) == Some(&asked)
            && self.talk_of(whose).is_some_and(|talk| talk.card.is_some());
        if still {
            self.say_in_thread(&chat, "That is not a number this takes".to_string(), false);
            return;
        }
        // By its own number: the next question, if there is one, is up
        // already under a number of its own.
        if self.mirror.questions.get(&chat) == Some(&asked) {
            self.mirror.questions.remove(&chat);
        }
        self.say_to_thread(
            &chat,
            Saying::Settle(asked, format!("\u{2714} {}", said.join(", "))),
        );
        self.mirror_head(whose, Some(Turning::Working));
    }

    /// Says in the thread what the reader typed here.
    pub(super) fn mirror_typed_here(&mut self, whose: talking::Whose, parts: &[Part]) {
        let words: String = parts
            .iter()
            .filter_map(|part| match part {
                Part::Words(words) => Some(words.as_str()),
                Part::Picture(_) => None,
            })
            .collect();
        if words.trim().is_empty() {
            return;
        }
        self.mirror_in(whose, format!("_On this machine:_ {}", words.trim()), false);
    }

    /// Says in the thread that the conversation was closed here.
    pub(super) fn mirror_closed(&mut self, talk: &Conversation) {
        if !self.chat_is_listening() {
            return;
        }
        if let Some(chat) = chat_of(talk).map(|chat| chat.file_name()) {
            self.say_in_thread(&chat, "Closed.".to_string(), false);
            // Said here rather than through `mirror_head`, which asks the
            // conversation -- and by now the document has gone.
            if let Some(head) = self.mirror.heads.get(&chat).cloned() {
                let head = Head {
                    state: Some(Turning::Closed),
                    ..head
                };
                self.mirror.heads.insert(chat.clone(), head.clone());
                self.retitle(&chat, head);
            }
        }
    }

    /// Says that the words about to go to this conversation's agent came
    /// from the chat.
    pub(super) fn about_to_say_from_afar(&mut self, whose: talking::Whose) {
        if let Some(chat) = self.chat_named(whose) {
            self.mirror.from_afar.insert(chat, Origin::Afar);
        }
    }

    /// Says where the words that waited for a turn to end came from, now
    /// that they go together: from the chat, all of them, and the agent is
    /// told so; or some from here, and the reader is evidently at the
    /// machine -- so the agent is told nothing, and the thread hears what
    /// was typed here now, the chat's own words being there already.
    pub(super) fn about_to_say_what_waited(
        &mut self,
        whose: talking::Whose,
        waiting: &[Vec<Part>],
        afar: &[bool],
    ) {
        if !afar.contains(&true) {
            return;
        }
        let Some(chat) = self.chat_named(whose) else {
            return;
        };
        if !afar.contains(&false) {
            self.mirror.from_afar.insert(chat, Origin::Afar);
            return;
        }
        let mut here: Vec<Part> = Vec::new();
        for (said, _) in waiting.iter().zip(afar).filter(|(_, afar)| !**afar) {
            if !here.is_empty() {
                here.push(Part::Words("\n\n".to_string()));
            }
            here.extend(said.iter().cloned());
        }
        self.mirror_typed_here(whose, &here);
        self.mirror.from_afar.insert(chat, Origin::Mixed);
    }

    /// Where the words going to the agent now came from: what goes in
    /// front of them, and whether the thread still has to hear them.
    pub(super) fn origin_of(&mut self, whose: talking::Whose) -> (Option<String>, bool) {
        // A conversation the reader started from a thread has no name yet
        // when its first words go, and every word in it so far is theirs
        // from afar.
        let starting = match whose {
            talking::Whose::One(id) => self.mirror.starting.contains_key(&id.get()),
            talking::Whose::Whoever => false,
        };
        let said = self
            .chat_named(whose)
            .and_then(|chat| self.mirror.from_afar.remove(&chat));
        match (starting, said) {
            (true, _) | (_, Some(Origin::Afar)) => {
                let platform = self.platform().map_or("a chat", |platform| platform.name);
                (Some(AFAR.replace("{platform}", platform)), false)
            }
            (false, Some(Origin::Mixed)) => (None, false),
            (false, None) => (None, true),
        }
    }

    /// A conversation begun from the chat, and its session asked for.
    ///
    /// Said into before anything else happens -- see `heard_fresh` -- so it
    /// is never one of the conversations opened here and left without a
    /// word, which the next frame lets go.
    fn a_conversation_from_afar(&mut self) -> DocumentId {
        self.documents.push(Some(Conversation::default().into()));
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

    /// Somebody on the list started a thread in the room the threads are
    /// in: a conversation begun, that thread its own, and what they wrote
    /// the first thing said in it.
    pub(super) fn heard_fresh(&mut self, thread: &str, text: &str) {
        // Heard twice is one thread: a platform sends an event again when it
        // thinks it was not taken.
        if self.mirror.starting.values().any(|kept| kept == thread)
            || self
                .mirror
                .threads
                .values()
                .any(|kept| kept.thread == thread)
        {
            return;
        }
        let id = self.a_conversation_from_afar();
        self.mirror.starting.insert(id.get(), thread.to_string());
        self.say_from_afar(talking::Whose::One(id), &[Part::Words(text.to_string())]);
    }

    /// Somebody on the list said something in a thread.
    pub(super) fn heard_in_thread(&mut self, thread: &str, text: &str) {
        let Some(chat) = self
            .mirror
            .threads
            .iter()
            .find(|(_, kept)| kept.thread == thread)
            .map(|(chat, _)| chat.clone())
        else {
            tracing::info!(thread, "words in a thread this window keeps nothing about");
            return;
        };
        let Some(id) = self.document_named(&chat) else {
            self.say_in_thread(
                &chat,
                "This conversation is not open on this machine.".to_string(),
                false,
            );
            return;
        };
        let whose = talking::Whose::One(id);
        // A card up is the conversation waiting on exactly this, on the
        // card: words are not read as an answer. And pointed at a card only
        // where the thread has one -- a page to open on the machine is never
        // put to it, and a card the platform would not take never got there.
        if self.talk_of(whose).is_some_and(|talk| talk.card.is_some()) {
            let said = match self.mirror.questions.contains_key(&chat) {
                true => "Answer on the card above.",
                false => "This question can only be answered on the machine.",
            };
            self.say_in_thread(&chat, said.to_string(), false);
            return;
        }
        let parts = [Part::Words(text.to_string())];
        self.say_from_afar(whose, &parts);
    }
}
