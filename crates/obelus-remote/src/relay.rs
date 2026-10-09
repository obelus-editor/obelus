//! One chat for every Obelus on the machine.
//!
//! **One window talks to the chat, and every window is heard in it.** A bot
//! has one connection, and a platform hands each message to one of an app's
//! connections at random -- so the window that holds the chat is a relay for
//! the others. They join it and say through it what they would have said to
//! the platform, and it hands each of them what the platform said about
//! their own threads. A window that joins has no connection of its own to
//! lose: the relay is the platform, as far as its conversations can tell.
//!
//! **A number is the relay's.** Each window numbers what it asks for -- a
//! thread, a question -- from one, so two windows' sevens are two different
//! things. Every asking gets a number of the relay's own on its way out, and
//! the answer goes back to the window that asked under the number it asked
//! with ([`Numbers`]).
//!
//! **The door is the one a window is brought forward through.** An address
//! on the loopback and a key in a file only this reader can read --
//! `app/worktrees` knocks on another window the same way -- which is one way
//! on all three systems, where a socket would have been two.
//!
//! **A relay that goes says why.** A window let go of the chat says so to
//! everybody joined to it before it closes the door ([`Over`]), and a window
//! that died says nothing: which is how the windows left behind tell a chat
//! handed on from one that has nobody holding it.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use obelus_sink::Sink;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

use crate::{
    Event, State,
    model::{Out, Where},
};

/// Where to reach the window the chat talks to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Door {
    /// Its address on the loopback.
    pub address: SocketAddr,
    /// What a window says first, which a stranger on the same machine does
    /// not have.
    pub key: String,
    /// What the relay says back, which is how the window knows it is the
    /// relay: a door's file outlives a relay that died, and whatever listens
    /// on its port after is somebody else -- who would otherwise be heard
    /// as the reader, and answer the agent's questions for them.
    pub answer: String,
}

impl Door {
    /// The door as its file says it: the address, then the two keys.
    #[must_use]
    pub fn written(&self) -> String {
        format!("{}\n{}\n{}\n", self.address, self.key, self.answer)
    }

    /// And read back, where the file says one.
    #[must_use]
    pub fn read(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let address = lines.next()?.parse().ok()?;
        let mut a_key = || {
            lines
                .next()
                .filter(|key| !key.is_empty())
                .map(str::to_string)
        };
        let key = a_key()?;
        let answer = a_key()?;
        Some(Self {
            address,
            key,
            answer,
        })
    }
}

/// Whether two keys are the same, taking as long whichever character they
/// differ at: one that answered sooner the further it got would tell
/// somebody knocking how much of a key they had right.
fn same_key(said: &str, key: &str) -> bool {
    said.len() == key.len()
        && said
            .bytes()
            .zip(key.bytes())
            .fold(0, |differ, (a, b)| differ | (a ^ b))
            == 0
}

/// Where a window is: what a thread begun in the chat is asked to choose
/// between, where more than one tree has a window on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    /// The tree, which is what two windows on the same one have in common.
    pub tree: PathBuf,
    /// What the card calls it: the project, and the branch.
    pub name: String,
}

/// What a window joined to the relay tells it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Up {
    /// Where it is, and nowhere while it is on no project.
    Place(Option<Place>),
    /// Which threads its conversations are: what is said in one of them
    /// goes to it.
    Threads(BTreeSet<String>),
    /// Something to ask of the platform.
    Out(Out),
}

/// What the relay tells a window joined to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Down {
    /// The relay has a new connection to the platform: what was on its way
    /// to the last one will not arrive.
    Started,
    /// Where the connection to the platform has got to.
    Connection {
        /// Where.
        state: State,
        /// Why, where it is something wrong.
        why: Option<String>,
    },
    /// Somebody said something, in a thread that is this window's or in a
    /// fresh one the reader chose this window for.
    Heard {
        /// Their id.
        from: String,
        /// Which group.
        room: String,
        /// Where in it.
        at: Where,
        /// What.
        text: String,
    },
    /// A thread this window asked for is there.
    Opened {
        /// The number this window asked with.
        asked: u64,
        /// What the platform calls it.
        thread: String,
        /// Where a reader can be sent to read it.
        link: Option<String>,
    },
    /// A thread this window asked for did not open.
    Unopened {
        /// The number this window asked with.
        asked: u64,
        /// Whether it was let go of rather than refused.
        waited: bool,
    },
    /// A question's card was not taken, and was said in words instead.
    Unasked {
        /// The number this window asked with.
        asked: u64,
    },
    /// Somebody answered a question of this window's on its card.
    Answered {
        /// Their id.
        from: String,
        /// The number this window asked with.
        asked: u64,
        /// The ids of what they chose.
        chosen: Vec<String>,
        /// What they wrote.
        words: Option<String>,
    },
    /// Somebody gave up on a question of this window's on its card.
    Cancelled {
        /// Their id.
        from: String,
        /// The number this window asked with.
        asked: u64,
    },
    /// The group the threads are in, as the relay has it: paired in there,
    /// perhaps while this window was already joined.
    Room(String),
    /// The relay is going, and why.
    Over(Over),
}

impl Down {
    /// What a window hears this as: what it would have heard from the
    /// platform itself, except for the two things only a relay says.
    #[must_use]
    pub fn heard(self) -> Event {
        match self {
            Self::Started => Event::Restarted,
            Self::Connection { state, why } => Event::Connection { state, why },
            Self::Heard {
                from,
                room,
                at,
                text,
            } => Event::Heard {
                from,
                room,
                at,
                text,
            },
            Self::Opened {
                asked,
                thread,
                link,
            } => Event::Opened {
                asked,
                thread,
                link,
            },
            Self::Unopened { asked, waited } => Event::Unopened { asked, waited },
            Self::Unasked { asked } => Event::Unasked { asked },
            Self::Answered {
                from,
                asked,
                chosen,
                words,
            } => Event::Answered {
                from,
                asked,
                chosen,
                words,
            },
            Self::Cancelled { from, asked } => Event::Cancelled { from, asked },
            Self::Room(room) => Event::Room(room),
            Self::Over(over) => Event::Left(over),
        }
    }
}

/// Why the relay a window was joined to went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Over {
    /// The reader let the chat go, or set none: nobody is to take it up.
    Stopped,
    /// It went to a window that asked for it, which will say where it is.
    Handed,
    /// The relay went without a word -- closed, or died -- and the chat is
    /// nobody's: one of the windows left takes it up.
    Went,
    /// The door was never answered, or not by the relay: whatever its file
    /// says is from a relay that has gone since.
    Unreached,
}

/// What a window joined to the relay did, as the relay hears it.
#[derive(Debug)]
pub enum Window {
    /// It joined, and this is where what is for it goes.
    Came(tokio::sync::mpsc::UnboundedSender<Down>),
    /// It said something.
    Said(Up),
    /// It went.
    Went,
}

/// Who asked for something the platform will answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Asker {
    /// The window the relay is in, for one of its own conversations.
    Here,
    /// A window joined to it, by the relay's number for it.
    Window(u64),
    /// The relay itself, asking which project a thread begun in the chat is
    /// for.
    Routing,
}

/// The relay's numbers for what has been asked, and whose each was.
///
/// **A number is the process's, not the relay's.** A card carries this
/// process's mark and its number and nothing else, so a card left up when
/// the chat was handed on could be pressed once it came back -- and a relay
/// that counted from one again had put a new question up under the same
/// number, which the press then answered. So the numbers go on from where
/// the last relay in this process left them, and only what they stood for
/// is forgotten.
#[derive(Debug, Default)]
pub struct Numbers {
    /// Threads asked for and not yet answered.
    opens: BTreeMap<u64, (Asker, u64)>,
    /// Questions put, until what became of them is.
    asks: BTreeMap<u64, (Asker, u64)>,
}

impl Numbers {
    /// What is asked of the platform, numbered the relay's way.
    ///
    /// What became of a question the relay has no number for -- one put
    /// before it was the relay -- gets a number nothing was asked with,
    /// which the platform says in words since it has no card for it.
    pub fn out(&mut self, from: Asker, out: Out) -> Out {
        match out {
            Out::Open { asked, room, head } => {
                let ours = a_number();
                self.opens.insert(ours, (from, asked));
                Out::Open {
                    asked: ours,
                    room,
                    head,
                }
            }
            Out::Ask {
                room,
                thread,
                to,
                asked,
                question,
            } => {
                let ours = a_number();
                self.asks.insert(ours, (from, asked));
                Out::Ask {
                    room,
                    thread,
                    to,
                    asked: ours,
                    question,
                }
            }
            Out::Settle {
                room,
                thread,
                to,
                asked,
                said,
            } => {
                let ours = self
                    .asks
                    .iter()
                    .find(|(_, whose)| **whose == (from, asked))
                    .map(|(ours, _)| *ours);
                let ours = match ours {
                    Some(ours) => {
                        self.asks.remove(&ours);
                        ours
                    }
                    None => a_number(),
                };
                Out::Settle {
                    room,
                    thread,
                    to,
                    asked: ours,
                    said,
                }
            }
            out => out,
        }
    }

    /// Whose thread this is, by the number it was asked for with, and the
    /// number they asked with -- once: a thread opens or does not once.
    pub fn opened(&mut self, asked: u64) -> Option<(Asker, u64)> {
        self.opens.remove(&asked)
    }

    /// Whose question this is, by the number it was put with, and the
    /// number they put it with. Kept, because a card can be pressed again
    /// until what became of it is said.
    #[must_use]
    pub fn asked(&self, asked: u64) -> Option<(Asker, u64)> {
        self.asks.get(&asked).copied()
    }

    /// A new connection to the platform, which knows nothing of the threads
    /// the last one was asked for. Its cards are this process's, and can
    /// still be pressed.
    pub fn restarted(&mut self) {
        self.opens.clear();
    }

    /// A window has gone, and nothing of its will be answered to it.
    pub fn went(&mut self, window: u64) {
        let theirs = |(whose, _): &mut (Asker, u64)| *whose != Asker::Window(window);
        self.opens.retain(|_, whose| theirs(whose));
        self.asks.retain(|_, whose| theirs(whose));
    }
}

/// A number for something asked of the platform: from one for the process
/// -- see [`Numbers`].
fn a_number() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}

/// A window's number, the relay's way: from one for the process, so that a
/// window from a relay since let go can never be taken for one of the next.
fn a_window() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}

/// How long the first line may be: a key, and no more. A stranger who sends
/// a megabyte is not knocking.
const KNOCK_AT_MOST: u64 = 4096;

/// How long either end waits for the other's key: one that never comes is
/// a connection held open by something that is not a window or not the
/// relay, and a window waiting on it would turn its mark for ever.
const KNOCK_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

/// The relay's door, open for as long as this is kept.
#[derive(Debug)]
pub struct Listening {
    door: Door,
    accepting: tokio::task::AbortHandle,
}

impl Listening {
    /// Where it is.
    #[must_use]
    pub const fn door(&self) -> &Door {
        &self.door
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        // Only the accepting: a window already in has a writer of its own,
        // which ends when the relay drops where its words go -- and says
        // `Over` first, where the relay had a reason to give.
        self.accepting.abort();
    }
}

/// Opens the relay's door: each window that comes with the key is
/// [`Window::Came`], what it says is [`Window::Said`], and its going is
/// [`Window::Went`] -- all as [`Event::Window`], by a number of its own.
///
/// # Errors
///
/// Where the loopback would not be listened on.
pub fn listen(sink: Arc<dyn Sink<Event>>) -> std::io::Result<Listening> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let key = a_key();
    let answer = a_key();
    let expected = key.clone();
    let says = answer.clone();
    let accepting = obelus_runtime::handle().spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "the relay is not listening");
                return;
            }
        };
        while let Ok((stream, _)) = listener.accept().await {
            obelus_runtime::handle().spawn(a_window_in(
                stream,
                expected.clone(),
                says.clone(),
                sink.clone(),
            ));
        }
    });
    Ok(Listening {
        door: Door {
            address,
            key,
            answer,
        },
        accepting: accepting.abort_handle(),
    })
}

/// One window, from its key to its going.
async fn a_window_in(
    stream: tokio::net::TcpStream,
    expected: String,
    answer: String,
    sink: Arc<dyn Sink<Event>>,
) {
    let (reading, mut writing) = stream.into_split();
    let mut reading = tokio::io::BufReader::new(reading);
    let mut line = String::new();
    let knocked = tokio::time::timeout(
        KNOCK_WITHIN,
        tokio::io::AsyncReadExt::take(&mut reading, KNOCK_AT_MOST).read_line(&mut line),
    )
    .await;
    if !matches!(knocked, Ok(Ok(_))) || !same_key(line.trim_end(), &expected) {
        tracing::warn!("a window came to the relay without its key");
        return;
    }
    if writing
        .write_all(format!("{answer}\n").as_bytes())
        .await
        .is_err()
    {
        return;
    }
    let window = a_window();
    let (down, mut downs) = tokio::sync::mpsc::unbounded_channel::<Down>();
    if sink
        .send(Event::Window {
            window,
            did: Window::Came(down),
        })
        .is_err()
    {
        return;
    }
    obelus_runtime::handle().spawn(async move {
        while let Some(down) = downs.recv().await {
            let Ok(mut said) = serde_json::to_string(&down) else {
                continue;
            };
            said.push('\n');
            if writing.write_all(said.as_bytes()).await.is_err() {
                return;
            }
        }
        // Everything the relay had for it is written, `Over` last where
        // there was one: the window reads the rest of it and then the end.
        let _ = writing.shutdown().await;
    });
    loop {
        line.clear();
        match reading.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => match serde_json::from_str::<Up>(&line) {
                Ok(up) => {
                    if sink
                        .send(Event::Window {
                            window,
                            did: Window::Said(up),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "a window said something the relay cannot read")
                }
            },
        }
    }
    let _ = sink.send(Event::Window {
        window,
        did: Window::Went,
    });
}

/// Joins the relay behind `door`: what it says comes through `sink` as what
/// the platform would have said ([`Down::heard`]), and its going as
/// [`Event::Left`] with why. What is sent on the channel handed back goes to
/// it, waiting for the door to open; dropping that is leaving.
pub fn join(door: Door, sink: Arc<dyn Sink<Event>>) -> tokio::sync::mpsc::UnboundedSender<Up> {
    let (up, mut ups) = tokio::sync::mpsc::unbounded_channel::<Up>();
    obelus_runtime::handle().spawn(async move {
        let stream = match tokio::net::TcpStream::connect(door.address).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::info!(%error, address = %door.address, "the relay did not answer");
                let _ = sink.send(Event::Left(Over::Unreached));
                return;
            }
        };
        let (reading, mut writing) = stream.into_split();
        let mut reading = tokio::io::BufReader::new(reading);
        let mut line = String::new();
        let knock = format!("{}\n", door.key);
        let answered = async {
            writing.write_all(knock.as_bytes()).await.ok()?;
            tokio::io::AsyncReadExt::take(&mut reading, KNOCK_AT_MOST)
                .read_line(&mut line)
                .await
                .ok()
        };
        // Nothing is said to it, and nothing it says is heard, until it has
        // answered as the relay: see `Door::answer`.
        let answered = tokio::time::timeout(KNOCK_WITHIN, answered).await;
        if !matches!(answered, Ok(Some(_))) || !same_key(line.trim_end(), &door.answer) {
            tracing::warn!(address = %door.address, "what is at the door did not answer as the relay");
            let _ = sink.send(Event::Left(Over::Unreached));
            return;
        }
        obelus_runtime::handle().spawn(async move {
            while let Some(said) = ups.recv().await {
                let Ok(mut said) = serde_json::to_string(&said) else {
                    continue;
                };
                said.push('\n');
                if writing.write_all(said.as_bytes()).await.is_err() {
                    return;
                }
            }
            let _ = writing.shutdown().await;
        });
        let mut over = Over::Went;
        loop {
            line.clear();
            match reading.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => match serde_json::from_str::<Down>(&line) {
                    // Kept for the end rather than said now: what was written
                    // before it is still being read, and a window that left at
                    // `Over` would have missed nothing only by luck.
                    Ok(Down::Over(why)) => over = why,
                    Ok(down) => {
                        if sink.send(down.heard()).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the relay said something this window cannot read");
                    }
                },
            }
        }
        let _ = sink.send(Event::Left(over));
    });
    up
}

/// A key nobody else has, for the door: the hasher std seeds with
/// randomness for every map, twice -- the same as a window's own door.
fn a_key() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};

    let half = || {
        let mut hasher = std::hash::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Head, Question};

    fn open(asked: u64) -> Out {
        Out::Open {
            asked,
            room: "C1".to_string(),
            head: Head {
                title: "A conversation".to_string(),
                place: "obelus".to_string(),
                state: None,
            },
        }
    }

    fn ask(asked: u64) -> Out {
        Out::Ask {
            room: "C1".to_string(),
            thread: "T1".to_string(),
            to: "U1".to_string(),
            asked,
            question: Question {
                about: "Read the file?".to_string(),
                choices: vec![("once".to_string(), "Allow once".to_string())],
                several: false,
                needed: true,
                words: None,
            },
        }
    }

    fn settle(asked: u64) -> Out {
        Out::Settle {
            room: "C1".to_string(),
            thread: "T1".to_string(),
            to: "U1".to_string(),
            asked,
            said: "Closed.".to_string(),
        }
    }

    fn numbered(out: &Out) -> u64 {
        match out {
            Out::Open { asked, .. } | Out::Ask { asked, .. } | Out::Settle { asked, .. } => *asked,
            _ => panic!("not numbered: {out:?}"),
        }
    }

    /// Two windows asking with the same number are two askings to the
    /// platform, and each answer goes back to the window that asked, under
    /// the number it asked with.
    ///
    /// Broken deliberately by passing the number through as it came: the
    /// platform was asked for two threads both called one, and the second
    /// window's answer went to the first.
    #[test]
    fn two_windows_sevens_are_two_askings() {
        let mut numbers = Numbers::default();
        let here = numbered(&numbers.out(Asker::Here, open(7)));
        let there = numbered(&numbers.out(Asker::Window(3), open(7)));
        assert_ne!(here, there, "two askings went to the platform as one");
        assert_eq!(numbers.opened(there), Some((Asker::Window(3), 7)));
        assert_eq!(numbers.opened(here), Some((Asker::Here, 7)));
        assert_eq!(numbers.opened(here), None, "a thread opened twice");
    }

    /// A question's card is the asker's until what became of it is said:
    /// pressed twice, it is theirs twice, and settling it closes the card
    /// the platform knows by the relay's number.
    ///
    /// Broken deliberately by settling under the window's own number: the
    /// platform was told to close a card it never put up.
    #[test]
    fn a_card_is_the_askers_until_it_is_settled() {
        let mut numbers = Numbers::default();
        let _ = numbers.out(Asker::Here, ask(2));
        let put = numbered(&numbers.out(Asker::Window(5), ask(2)));
        assert_eq!(numbers.asked(put), Some((Asker::Window(5), 2)));
        assert_eq!(numbers.asked(put), Some((Asker::Window(5), 2)));
        let settled = numbered(&numbers.out(Asker::Window(5), settle(2)));
        assert_eq!(settled, put, "the card closed was not the one put up");
        assert_eq!(numbers.asked(put), None);
        // And one nobody here put up is closed under a number nothing was
        // asked with, which the platform says in words.
        let unknown = numbered(&numbers.out(Asker::Window(5), settle(9)));
        assert_eq!(numbers.asked(unknown), None);
        assert_ne!(unknown, put);
    }

    /// A window that goes takes its askings with it, and a new connection
    /// forgets the threads the last was asked for and not the cards.
    ///
    /// Broken deliberately by forgetting nothing when a window went: the
    /// thread was still answered to a window that had gone.
    #[test]
    fn a_window_that_goes_is_answered_nothing() {
        let mut numbers = Numbers::default();
        let thread = numbered(&numbers.out(Asker::Window(1), open(1)));
        let card = numbered(&numbers.out(Asker::Window(1), ask(1)));
        let kept = numbered(&numbers.out(Asker::Here, ask(1)));
        numbers.went(1);
        assert_eq!(numbers.opened(thread), None);
        assert_eq!(numbers.asked(card), None);
        let thread = numbered(&numbers.out(Asker::Here, open(2)));
        numbers.restarted();
        assert_eq!(numbers.opened(thread), None);
        assert_eq!(numbers.asked(kept), Some((Asker::Here, 1)));
    }

    /// A relay after the last goes on numbering from where it left off: a
    /// card left up from before is no question of the new one's.
    ///
    /// Broken deliberately by counting from one in every relay: the old
    /// card's number was the new question's.
    #[test]
    fn a_relay_after_the_last_goes_on_numbering() {
        let mut before = Numbers::default();
        let old = numbered(&before.out(Asker::Here, ask(1)));
        let mut after = Numbers::default();
        let new = numbered(&after.out(Asker::Here, ask(1)));
        assert_ne!(old, new, "two questions under one number");
        assert_eq!(
            after.asked(old),
            None,
            "the old card answers the new question"
        );
    }

    /// Events from the two ends of a door, as they arrive.
    fn heard() -> (Arc<dyn Sink<Event>>, std::sync::mpsc::Receiver<Event>) {
        let (sender, events) = std::sync::mpsc::channel::<Event>();
        (Arc::new(sender), events)
    }

    /// The next thing heard, or nothing in a while.
    fn next(events: &std::sync::mpsc::Receiver<Event>) -> Option<Event> {
        events.recv_timeout(KNOCK_WITHIN * 2).ok()
    }

    /// A window with both keys is let in, and hears what the relay says.
    ///
    /// Broken deliberately by the relay not saying its answer: the window
    /// took the door for somebody else's and left.
    #[test]
    fn a_window_with_the_keys_is_let_in() {
        let (relays, at_the_relay) = heard();
        let listening = listen(relays).expect("listening");
        let (windows, at_the_window) = heard();
        let _up = join(listening.door().clone(), windows);
        let Some(Event::Window {
            did: Window::Came(down),
            ..
        }) = next(&at_the_relay)
        else {
            panic!("the window was not let in");
        };
        let _ = down.send(Down::Room("C1".to_string()));
        assert!(
            matches!(next(&at_the_window), Some(Event::Room(room)) if room == "C1"),
            "the window did not hear the relay"
        );
    }

    /// A window without the key is not let in.
    ///
    /// Broken deliberately by letting in whatever knocks: the relay heard a
    /// window come with the wrong key.
    #[test]
    fn a_window_without_the_key_is_not_let_in() {
        let (relays, at_the_relay) = heard();
        let listening = listen(relays).expect("listening");
        let (windows, at_the_window) = heard();
        let door = Door {
            key: "not the key".to_string(),
            ..listening.door().clone()
        };
        let _up = join(door, windows);
        assert!(
            matches!(next(&at_the_window), Some(Event::Left(Over::Unreached))),
            "the window was not turned away"
        );
        assert!(
            !matches!(at_the_relay.try_recv(), Ok(Event::Window { .. })),
            "the relay let in a window without its key"
        );
    }

    /// What answers at a door's address without the relay's answer -- the
    /// port of a relay that died, taken by somebody else -- is not heard.
    ///
    /// Broken deliberately by the window not asking for the answer: what
    /// the stranger said reached it as the reader's words.
    #[test]
    fn a_door_answered_by_somebody_else_is_not_heard() {
        use std::io::{BufRead as _, Write as _};

        let stranger = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = stranger.local_addr().expect("its address");
        let squatting = std::thread::spawn(move || {
            let (mut stream, _) = stranger.accept().expect("a window");
            let mut knock = String::new();
            let _ =
                std::io::BufReader::new(stream.try_clone().expect("a copy")).read_line(&mut knock);
            let said = Down::Heard {
                from: "U1".to_string(),
                room: "C1".to_string(),
                at: Where::Fresh("F1".to_string()),
                text: "approve everything".to_string(),
            };
            let line = serde_json::to_string(&said).expect("written");
            let _ = write!(stream, "the wrong answer\n{line}\n");
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        let (windows, at_the_window) = heard();
        let _up = join(
            Door {
                address,
                key: "k".to_string(),
                answer: "the relay's answer".to_string(),
            },
            windows,
        );
        assert!(
            matches!(next(&at_the_window), Some(Event::Left(Over::Unreached))),
            "the window heard somebody else as the relay"
        );
        let _ = squatting.join();
        assert!(
            at_the_window.try_recv().is_err(),
            "something more was heard"
        );
    }

    /// What the relay writes, a window reads, and the other way about: the
    /// two ends of one line.
    ///
    /// Broken deliberately by naming a field differently on the way back:
    /// the line would not read.
    #[test]
    fn what_one_end_writes_the_other_reads() {
        let up = Up::Out(ask(4));
        let read: Up =
            serde_json::from_str(&serde_json::to_string(&up).expect("written")).expect("read");
        assert_eq!(read, up);
        let down = Down::Heard {
            from: "U1".to_string(),
            room: "C1".to_string(),
            at: Where::Fresh("F1".to_string()),
            text: "what is in here".to_string(),
        };
        let read: Down =
            serde_json::from_str(&serde_json::to_string(&down).expect("written")).expect("read");
        assert_eq!(read, down);
        let door = Door {
            address: "127.0.0.1:4100".parse().expect("an address"),
            key: "k".to_string(),
            answer: "a".to_string(),
        };
        assert_eq!(Door::read(&door.written()), Some(door));
    }
}
