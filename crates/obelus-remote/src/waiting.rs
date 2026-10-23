//! What is to be said while a connection is not up, kept in order -- and
//! what was not said, counted by thread.
//!
//! **Kept, and only so much of it.** A platform that cannot be reached is
//! tried again, and one that refused is too -- what a platform calls a
//! refusal includes being busy -- so a window can wait for one for as long
//! as it is open, and everything said meanwhile was kept for it, without
//! end. What is kept now is the newest of it, which is what a reader
//! catching up wants to read. What is let go of is said: a thread that was
//! being asked for is said not to have opened, and anything for a thread is
//! said not to have arrived, so that neither the conversation it was for
//! nor the reader reading the thread is left thinking it was.
//!
//! **A thread that missed something says so with the next thing it gets.**
//! The reader is on their phone, not at the window the conversation notes
//! it in, so the count of what a thread did not get goes in front of the
//! next message that does arrive there.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use obelus_sink::Sink;

use crate::{
    Event,
    model::Out,
    sending::{ROUNDS, Saying, Stopped},
};

/// How much is kept: a turn's worth of a busy afternoon, several times over.
const KEPT: usize = 256;

/// What is waiting to be said, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Waiting {
    sayings: VecDeque<Saying>,
    /// Whether anything has been let go of yet, so the log says it once.
    dropped: bool,
    /// How many things each thread has not been told, since the last one it
    /// was.
    missed: HashMap<String, usize>,
}

impl Waiting {
    /// Keeps one more, letting go of the oldest where there is no room.
    pub(crate) fn keep(&mut self, saying: Saying, sink: &Arc<dyn Sink<Event>>) {
        if self.sayings.len() >= KEPT
            && let Some(oldest) = self.sayings.pop_front()
        {
            if !self.dropped {
                tracing::warn!(kept = KEPT, "too much waiting to be said; the oldest goes");
                self.dropped = true;
            }
            self.let_go(oldest.out, None, sink);
        }
        self.sayings.push_back(saying);
    }

    /// Puts back at the front what was being said when the connection
    /// stopped answering, so it is said first once it answers again -- or,
    /// where the platform has answered it with "later" [`ROUNDS`] times,
    /// takes it to be the message, and lets it go as refused.
    pub(crate) fn stopped(&mut self, stopped: Stopped, sink: &Arc<dyn Sink<Event>>) {
        let Stopped {
            why,
            answered,
            again,
        } = stopped;
        let mut again = *again;
        if answered {
            again.rounds += 1;
        }
        if again.rounds >= ROUNDS {
            tracing::warn!(%why, rounds = again.rounds, "the same message stopped every connection; it goes");
            self.let_go(again.out, Some(why), sink);
            return;
        }
        self.sayings.push_front(again);
    }

    /// The next thing to say, oldest first.
    pub(crate) fn next(&mut self) -> Option<Saying> {
        self.sayings.pop_front()
    }

    /// Something for a thread that the platform would not take: counted,
    /// and said to the conversation in the platform's words.
    pub(crate) fn refused(&mut self, thread: &str, why: &str, sink: &Arc<dyn Sink<Event>>) {
        *self.missed.entry(thread.to_string()).or_default() += 1;
        let _ = sink.send(Event::unsaid(thread.to_string(), Some(why.to_string())));
    }

    /// `text` as it goes into `thread`: after a line saying how much did
    /// not, where anything did not. Asked before saying it; the count goes
    /// only once it is said ([`Self::said_in`]).
    pub(crate) fn prefaced(&self, thread: &str, text: &str) -> String {
        match self.missed.get(thread).copied().unwrap_or(0) {
            0 => text.to_string(),
            1 => format!("1 message before this one did not arrive.\n\n{text}"),
            missed => format!("{missed} messages before this one did not arrive.\n\n{text}"),
        }
    }

    /// Something has arrived in `thread`, and with it the word about what
    /// had not.
    pub(crate) fn said_in(&mut self, thread: &str) {
        self.missed.remove(thread);
    }

    /// What is let go of, said to whoever was waiting on it: `why` is what
    /// the platform said, or `None` where it was let go of here, waiting.
    fn let_go(&mut self, out: Out, why: Option<String>, sink: &Arc<dyn Sink<Event>>) {
        let thread = match out {
            Out::Open { asked, .. } => {
                let _ = sink.send(Event::Unopened {
                    asked,
                    waited: why.is_none(),
                });
                return;
            }
            // A question let go of is one nothing in the thread can answer:
            // the conversation is told, as when its card is refused.
            Out::Ask { asked, thread, .. } => {
                let _ = sink.send(Event::Unasked { asked });
                thread
            }
            Out::Say { thread, .. } => thread,
            // What closes a card, names a thread or asks a name is not
            // anything the reader reads as a message.
            Out::Settle { .. } | Out::Retitle { .. } | Out::Name { .. } => return,
        };
        *self.missed.entry(thread.clone()).or_default() += 1;
        let _ = sink.send(Event::unsaid(thread, why));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard() -> (Arc<dyn Sink<Event>>, std::sync::mpsc::Receiver<Event>) {
        let (sender, heard) = std::sync::mpsc::channel::<Event>();
        (Arc::new(sender), heard)
    }

    fn say(thread: &str, text: &str) -> Saying {
        Saying::from(Out::Say {
            room: "R".to_string(),
            thread: thread.to_string(),
            to: "U".to_string(),
            text: text.to_string(),
            notify: false,
        })
    }

    /// Only so much is kept, the newest of it, and a thread asked for and
    /// let go of is said not to have opened.
    ///
    /// Broken deliberately three times. Keeping everything: the queue held
    /// all three hundred. Letting an asked-for thread go in silence: no
    /// `Unopened` came, and its conversation would have waited for ever. And
    /// saying it as the platform's refusal: the conversation blamed the
    /// platform for what was let go of here.
    #[test]
    fn only_the_newest_is_kept_and_a_thread_let_go_is_said_not_to_open() {
        let (sink, heard) = heard();
        let mut waiting = Waiting::default();
        waiting.keep(
            Saying::from(Out::Open {
                asked: 7,
                room: "R".to_string(),
                head: crate::model::Head {
                    title: "t".to_string(),
                    place: "p".to_string(),
                    state: None,
                },
            }),
            &sink,
        );
        for number in 0..300 {
            waiting.keep(
                Saying::from(Out::Name {
                    id: number.to_string(),
                }),
                &sink,
            );
        }
        let kept: Vec<Out> = std::iter::from_fn(|| waiting.next())
            .map(|saying| saying.out)
            .collect();
        assert_eq!(kept.len(), KEPT, "everything was kept");
        assert_eq!(
            kept.last(),
            Some(&Out::Name {
                id: "299".to_string()
            })
        );
        assert!(
            heard.try_iter().any(|event| matches!(
                event,
                Event::Unopened {
                    asked: 7,
                    waited: true
                }
            )),
            "the thread let go of was not said not to open"
        );
    }

    /// Words let go of are said not to have arrived -- to the conversation,
    /// and to the thread with the next thing that does.
    ///
    /// Broken deliberately twice. Letting a `Say` go in silence: no
    /// `Unsaid` came. And not counting it: the next message in the thread
    /// went with nothing in front of it.
    #[test]
    fn words_let_go_are_said_not_to_arrive() {
        let (sink, heard) = heard();
        let mut waiting = Waiting::default();
        waiting.keep(say("T", "lost"), &sink);
        for _ in 0..KEPT {
            waiting.keep(say("other", "kept"), &sink);
        }
        assert!(
            heard
                .try_iter()
                .any(|event| matches!(event, Event::Unsaid { thread, why: None } if thread == "T")),
            "the words let go of were not said not to arrive"
        );
        assert_eq!(
            waiting.prefaced("T", "next"),
            "1 message before this one did not arrive.\n\nnext"
        );
        assert_eq!(waiting.prefaced("other", "next"), "next");
    }

    /// The count is the thread's until something arrives there, and then
    /// it is gone.
    ///
    /// Broken deliberately by clearing it when the preface was asked for:
    /// a message that then did not arrive took the count with it, and the
    /// one after said nothing.
    #[test]
    fn the_count_goes_once_something_arrives() {
        let (sink, _heard) = heard();
        let mut waiting = Waiting::default();
        waiting.refused("T", "too long", &sink);
        waiting.refused("T", "too long", &sink);
        assert_eq!(
            waiting.prefaced("T", "next"),
            "2 messages before this one did not arrive.\n\nnext"
        );
        assert_eq!(
            waiting.prefaced("T", "next"),
            "2 messages before this one did not arrive.\n\nnext"
        );
        waiting.said_in("T");
        assert_eq!(waiting.prefaced("T", "next"), "next");
    }

    /// What was being said when the connection went is said first when it
    /// is back.
    ///
    /// Broken deliberately by pushing it to the back: "first" came out
    /// after "second".
    #[test]
    fn what_was_being_said_goes_first_again() {
        let (sink, _heard) = heard();
        let mut waiting = Waiting::default();
        waiting.keep(say("T", "second"), &sink);
        waiting.stopped(
            Stopped {
                why: "down".to_string(),
                answered: false,
                again: Box::new(say("T", "first")),
            },
            &sink,
        );
        let order: Vec<String> = std::iter::from_fn(|| waiting.next())
            .filter_map(|saying| match saying.out {
                Out::Say { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(order, ["first", "second"]);
    }

    /// A message that stops the connection every time it answers is the
    /// message, and goes as refused -- in what the platform said last --
    /// so that what is behind it is said.
    ///
    /// Broken deliberately twice. Never counting the rounds: it went back
    /// to the front every time, and the queue never moved past it. And
    /// letting it go in silence: no `Unsaid` came, with or without the
    /// platform's words.
    #[test]
    fn what_stops_every_connection_goes_as_refused() {
        let (sink, heard) = heard();
        let mut waiting = Waiting::default();
        let mut stuck = say("T", "stuck");
        for _ in 0..ROUNDS {
            waiting.stopped(
                Stopped {
                    why: "answered 502".to_string(),
                    answered: true,
                    again: Box::new(stuck.clone()),
                },
                &sink,
            );
            match waiting.next() {
                Some(again) => stuck = again,
                None => break,
            }
        }
        assert!(waiting.next().is_none(), "it was still being said");
        assert!(
            heard.try_iter().any(|event| matches!(
                event,
                Event::Unsaid { thread, why: Some(why) } if thread == "T" && why == "answered 502"
            )),
            "it went without a word"
        );
    }

    /// A message stopped by no answer at all -- a network coming and
    /// going, a machine asleep -- is put back however often it happens:
    /// that is nothing the message did.
    ///
    /// Broken deliberately by counting every round: after three it went as
    /// refused, in the words "no answer in time".
    #[test]
    fn what_no_answer_stops_is_never_let_go() {
        let (sink, heard) = heard();
        let mut waiting = Waiting::default();
        let mut kept = say("T", "kept");
        for _ in 0..ROUNDS * 3 {
            waiting.stopped(
                Stopped {
                    why: "no answer in time".to_string(),
                    answered: false,
                    again: Box::new(kept.clone()),
                },
                &sink,
            );
            kept = waiting.next().expect("it was let go of");
        }
        assert!(
            heard.try_iter().next().is_none(),
            "something was said of it"
        );
    }
}
