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
//! to the thread when its turn is over -- once, whole, rather than as it
//! types, which in a chat is a message edited forty times or forty
//! messages -- and so do the questions it asks, as the card would ask them
//! in words. What the reader types here goes there too, marked as said on
//! this machine; what they say there arrives here like anything they typed,
//! with a line in front of it for the agent saying where it came from. What
//! the agent's tools did does not go: a chat is not a transcript, and a run
//! of calls is the part of a turn nobody reads on a phone.
//!
//! **A reply is an answer while something is asked, and the next words
//! otherwise.** The conversation is waiting on the reader while a card is
//! up, so there is nothing else the reply could be; and once the card is
//! answered, here or there, the next reply is talking again.

use std::collections::{BTreeMap, BTreeSet};

use obelus_agent::chats::ChatId;
use obelus_component::composer::Part;
use obelus_remote::model::{Head, Out, Turning, Where};

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
    held: BTreeMap<String, Vec<(String, bool)>>,
    /// What the agent has said in each conversation's turn so far.
    this_turn: BTreeMap<String, String>,
    /// Conversations whose words waiting for the turn to end came from the
    /// chat, so that they go to the agent saying so.
    from_afar: BTreeSet<String>,
    /// What each thread was last said to be, so that it is said again only
    /// when something on it has moved.
    heads: BTreeMap<String, Head>,
    /// The notes the top was last offered, in the order they were
    /// numbered: a number sent back means the note that had it then.
    pub(super) offered: Vec<obelus_git::todo::NoteId>,
}

/// One conversation's thread: what the platform calls it, and whose direct
/// message it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Thread {
    thread: String,
    to: String,
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
                    to: said("to")?,
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
            kept.insert("to".to_string(), thread.to.clone().into());
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
        if !self.remote_state().connected() {
            return;
        }
        let (Some(platform), Some(to)) = (self.platform(), self.whom()) else {
            return;
        };
        if self.mirror.read_for != Some(platform.key) {
            self.mirror = Mirror {
                threads: read_the_table(platform.key),
                read_for: Some(platform.key),
                ..Mirror::default()
            };
        }
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
                    .push((lately, false));
            }
            self.mirror.heads.insert(chat.clone(), head.clone());
            self.say_to(Out::Open {
                asked,
                to: to.clone(),
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
        let title = Self::conversation_name(talk, self.talker.as_ref(), notes)
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
        if !self.remote_state().connected() {
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
        if let Some(Thread { thread, to }) = self.mirror.threads.get(&chat).cloned() {
            self.say_to(Out::Retitle { to, thread, head });
        }
    }

    /// The chat the conversation on screen is mirrored to, while it is:
    /// asked of the table and the state, which are both kept, so a frame
    /// can ask it.
    pub(super) fn conversation_mirrored_to(&self) -> Option<&'static str> {
        let chat = chat_of(self.conversation()?)?.file_name();
        let platform = self.platform()?;
        (self.remote_state().connected()
            && self.mirror.read_for == Some(platform.key)
            && self.mirror.threads.contains_key(&chat))
        .then_some(platform.name)
    }

    /// The conversation open here by its name elsewhere.
    fn talk_named(&self, chat: &str) -> Option<&Conversation> {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .find(|talk| chat_of(talk).is_some_and(|named| named.file_name() == chat))
    }

    /// Which document that is.
    fn document_named(&self, chat: &str) -> Option<DocumentId> {
        self.documents
            .iter()
            .position(|document| {
                document
                    .as_ref()
                    .and_then(Document::chat)
                    .and_then(chat_of)
                    .is_some_and(|named| named.file_name() == chat)
            })
            .map(DocumentId::new)
    }

    /// The name elsewhere of the conversation `whose` names.
    fn chat_named(&self, whose: talking::Whose) -> Option<String> {
        self.talk_of(whose)
            .and_then(chat_of)
            .map(|chat| chat.file_name())
    }

    /// A thread the platform has opened: written down, and what was held for
    /// it said in it.
    pub(super) fn thread_opened(&mut self, asked: u64, thread: String) {
        let Some(chat) = self.mirror.opening.remove(&asked) else {
            return;
        };
        let (Some(platform), Some(to)) = (self.platform(), self.whom()) else {
            return;
        };
        self.mirror
            .threads
            .insert(chat.clone(), Thread { thread, to });
        write_the_table(platform.key, &self.mirror.threads);
        // What the head became while the thread was on its way -- a turn
        // started, a name given -- said now there is one to say it on.
        if let (Some(head), Some(Thread { thread, to })) = (
            self.mirror.heads.get(&chat).cloned(),
            self.mirror.threads.get(&chat).cloned(),
        ) {
            self.say_to(Out::Retitle { to, thread, head });
        }
        for (text, notify) in self.mirror.held.remove(&chat).unwrap_or_default() {
            self.say_in_thread(&chat, text, notify);
        }
    }

    /// Says something in a conversation's thread, or holds it for the
    /// thread that is on its way.
    fn say_in_thread(&mut self, chat: &str, text: String, notify: bool) {
        if let Some(Thread { thread, to }) = self.mirror.threads.get(chat).cloned() {
            self.say_to(Out::Say {
                to,
                at: Where::Thread(thread),
                text,
                notify,
            });
            return;
        }
        if self.mirror.opening.values().any(|opening| opening == chat) {
            self.mirror
                .held
                .entry(chat.to_string())
                .or_default()
                .push((text, notify));
        }
    }

    /// The same, for the conversation `whose` names, where the chat is
    /// connected at all.
    fn mirror_in(&mut self, whose: talking::Whose, text: String, notify: bool) {
        if !self.remote_state().connected() {
            return;
        }
        if let Some(chat) = self.chat_named(whose) {
            self.say_in_thread(&chat, text, notify);
        }
    }

    /// Keeps what the agent said, for the end of its turn.
    pub(super) fn mirror_said(&mut self, whose: talking::Whose, text: &str) {
        if !self.remote_state().connected() {
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

    /// The turn is over: what the agent said in it goes to the thread, and
    /// the reader is called -- this is the moment it wants them.
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
        let Some(asked) = self
            .talk_of(whose)
            .and_then(|talk| talk.card.as_ref())
            .map(obelus_component::card::Card::in_words)
        else {
            return;
        };
        // What it said before it asked goes out before the question does:
        // an answer here would be to a question nobody there had read yet.
        let chat = self.chat_named(whose);
        let said = chat
            .and_then(|chat| self.mirror.this_turn.remove(&chat))
            .unwrap_or_default();
        if !said.trim().is_empty() {
            self.mirror_in(whose, said.trim().to_string(), false);
        }
        self.mirror_in(whose, asked, true);
        self.mirror_head(whose, Some(Turning::Waiting));
    }

    /// Says in the thread that a question was answered on this machine.
    pub(super) fn mirror_answered_here(&mut self, whose: talking::Whose, said: &str) {
        let said = match said.trim().is_empty() {
            true => "\u{2714} Answered on this machine".to_string(),
            false => format!("\u{2714} Answered on this machine: {said}"),
        };
        self.mirror_in(whose, said, false);
        self.mirror_head(whose, Some(Turning::Working));
    }

    /// Says in the thread that the agent stopped waiting.
    pub(super) fn mirror_withdrawn(&mut self, whose: talking::Whose) {
        self.mirror_in(whose, "The agent stopped asking.".to_string(), false);
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
        if !self.remote_state().connected() {
            return;
        }
        if let Some(chat) = chat_of(talk).map(|chat| chat.file_name()) {
            self.say_in_thread(&chat, "Closed.".to_string(), false);
            // Said here rather than through `mirror_head`, which asks the
            // conversation -- and by now the document has gone.
            if let (Some(Thread { thread, to }), Some(head)) = (
                self.mirror.threads.get(&chat).cloned(),
                self.mirror.heads.get(&chat).cloned(),
            ) {
                let head = Head {
                    state: Some(Turning::Closed),
                    ..head
                };
                self.mirror.heads.insert(chat, head.clone());
                self.say_to(Out::Retitle { to, thread, head });
            }
        }
    }

    /// What goes to the agent in front of words from the chat, if these are.
    pub(super) fn afar_for(&mut self, whose: talking::Whose) -> Option<String> {
        let chat = self.chat_named(whose)?;
        self.mirror.from_afar.remove(&chat).then(|| {
            let platform = self.platform().map_or("a chat", |platform| platform.name);
            AFAR.replace("{platform}", platform)
        })
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
        // A card up is the conversation waiting on exactly this.
        if let Some(card) = self.talk_of(whose).and_then(|talk| talk.card.clone()) {
            match card.answered_by(text) {
                Ok((chosen, words)) => {
                    self.answer_from_afar(whose, &chosen, words.as_deref());
                    self.mirror_head(whose, Some(Turning::Working));
                }
                Err(why) => self.say_in_thread(&chat, why, false),
            }
            return;
        }
        let parts = [Part::Words(text.to_string())];
        self.mirror.from_afar.insert(chat);
        self.say_from_afar(whose, &parts);
    }
}
