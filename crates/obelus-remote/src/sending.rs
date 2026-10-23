//! Saying one thing to a platform: asked again until it is taken, refused,
//! or the platform is plainly not there.
//!
//! **A platform that did not answer is asked again; one that said no is
//! not.** What it calls busy, a limit on how fast, its own trouble, or no
//! answer at all is worth a few more tries a moment apart. What it said no
//! to -- too long, not allowed, a thread that has gone -- is the same no
//! however often it is asked, and is said where the reader will see it.
//!
//! **Still not answered is the connection, not the message.** A message
//! asked after a few times with no answer goes back to the front of what
//! is waiting, and the connection is tried again the way it was at the
//! start: one message at a time each giving up on its own was a minute of
//! the network down losing everything said in it, one after another.
//!
//! **Said once, however many times it was asked.** A request that timed out
//! may have been taken all the same, so every thing said carries an id of
//! its own from the first try to the last, and each platform uses it to
//! tell a second asking from a second message ([`Saying::id`]).

use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::model::Out;

/// How long between tries before the connection is taken to be down.
pub(crate) const PAUSES: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// The longest a platform's own "try again after" is waited for, here: a
/// platform asking for longer is a connection to be tried again from the
/// top, not a message to sit on.
const LONGEST: Duration = Duration::from_secs(30);

/// How long one try is given to answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(20);

/// How many times the platform may answer a message with "later", a round
/// of tries each, before it is taken to be the message: one Slack or Feishu
/// cannot take, for a reason that looks like trouble on its side, would
/// otherwise hold up everything behind it for as long as the window is
/// open. A round with no answer at all is not counted: that is the
/// network's, and a message waits out a network however long it takes.
pub(crate) const ROUNDS: u32 = 3;

/// Why one try did not do it.
#[derive(Debug)]
pub(crate) enum Failure {
    /// Worth asking again: no answer, or an answer that says later.
    Again {
        /// What was said, or what would not answer.
        why: String,
        /// How long the platform asked to be left, where it said.
        after: Option<Duration>,
        /// Whether the platform answered at all: a no-answer is the
        /// network's, and says nothing about the message.
        answered: bool,
    },
    /// Said no, and would say it again.
    Refused(String),
}

impl Failure {
    /// No answer at all: worth asking again, and nothing to do with the
    /// message.
    pub(crate) fn again(why: impl ToString) -> Self {
        Self::Again {
            why: why.to_string(),
            after: None,
            answered: false,
        }
    }

    /// An answer that says later, after `after` where it says how long.
    pub(crate) fn later(why: impl ToString, after: Option<Duration>) -> Self {
        Self::Again {
            why: why.to_string(),
            after,
            answered: true,
        }
    }
}

/// Why it was not said in the end.
#[derive(Debug)]
pub(crate) enum Failed {
    /// Asked every time there was a try for, and never taken.
    Down {
        /// What the last try came to.
        why: String,
        /// Whether that was the platform saying later, rather than no
        /// answer at all.
        answered: bool,
    },
    /// The platform said no.
    Refused(String),
}

/// Something to be said, with the id it is said under every time it is
/// asked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Saying {
    /// What.
    pub(crate) out: Out,
    /// This process's mark and a count: unique on the machine, and short
    /// enough for Feishu's fifty characters.
    pub(crate) id: String,
    /// How many rounds of tries the platform has answered with "later"
    /// ([`ROUNDS`]).
    pub(crate) rounds: u32,
    /// When it was first asked for, in seconds since the epoch: from then
    /// on the platform may have it, on this connection or the next.
    pub(crate) first_tried: Option<u64>,
}

impl From<Out> for Saying {
    fn from(out: Out) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            out,
            id: format!(
                "{}-{}",
                crate::this_process(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
            rounds: 0,
            first_tried: None,
        }
    }
}

impl Saying {
    /// Something else said in its place -- a question in words -- under an
    /// id of its own, since it is another message.
    pub(crate) fn instead(&self, out: Out) -> Self {
        Self {
            out,
            id: format!("{}w", self.id),
            rounds: self.rounds,
            first_tried: None,
        }
    }

    /// The same id, and how often it has stopped things, for what is to be
    /// said again in its place.
    pub(crate) fn again(&self, out: Out) -> Self {
        Self {
            out,
            id: self.id.clone(),
            rounds: self.rounds,
            first_tried: self.first_tried,
        }
    }

    /// When it was first asked for, which is now if it never has been.
    pub(crate) fn first_tried(&mut self) -> u64 {
        *self.first_tried.get_or_insert_with(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
    }
}

/// What could not be said for the connection being down: what to say
/// again once it is up, which may not be what was first asked -- a question
/// already turned into words stays in words.
#[derive(Debug)]
pub(crate) struct Stopped {
    /// What would not answer.
    pub(crate) why: String,
    /// Whether the platform answered, saying later: only those count
    /// towards [`ROUNDS`], so a network that comes and goes does not wear
    /// out a message that did nothing wrong.
    pub(crate) answered: bool,
    /// What to say again, boxed: it is the whole message, and this is the
    /// error of every call that says one.
    pub(crate) again: Box<Saying>,
}

/// Tries `attempt` until it is done, refused, or has had a try after every
/// one of `pauses`. Each try is told which it is, from nought, so that one
/// after the first can ask whether the last was taken after all.
pub(crate) async fn tried<T, F, Fut>(pauses: &[Duration], mut attempt: F) -> Result<T, Failed>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let mut pauses = pauses.iter();
    let mut tries = 0;
    loop {
        // A request that never answers would hold up everything after it --
        // what is said goes one thing at a time -- and not every client a
        // platform is reached through gives up on its own.
        let answered = tokio::time::timeout(ANSWER_WITHIN, attempt(tries))
            .await
            .unwrap_or_else(|_| Err(Failure::again("no answer in time")));
        match answered {
            Ok(done) => return Ok(done),
            Err(Failure::Refused(why)) => return Err(Failed::Refused(why)),
            Err(Failure::Again {
                why,
                after,
                answered,
            }) => {
                let Some(pause) = pauses.next() else {
                    return Err(Failed::Down { why, answered });
                };
                tracing::info!(%why, tries, "asking the platform again");
                tokio::time::sleep(after.map_or(*pause, |after| after.min(LONGEST))).await;
                tries += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    const NONE: [Duration; 3] = [Duration::ZERO; 3];

    /// Every try asked of `tried`, and what it answered.
    async fn asked(answers: Vec<Result<u32, Failure>>) -> (Result<u32, Failed>, usize) {
        let answers = Arc::new(Mutex::new(answers.into_iter()));
        let count = Arc::new(Mutex::new(0));
        let done = tried(&NONE, |_| {
            let answers = answers.clone();
            let count = count.clone();
            async move {
                *count.lock().unwrap() += 1;
                answers
                    .lock()
                    .unwrap()
                    .next()
                    .expect("asked once too often")
            }
        })
        .await;
        let count = *count.lock().unwrap();
        (done, count)
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    /// A platform that did not answer is asked again until it does.
    ///
    /// Broken deliberately by returning on the first `Again`: the message
    /// was given up after one try and the count was one.
    #[test]
    fn what_did_not_answer_is_asked_again() {
        let (done, count) = run(asked(vec![
            Err(Failure::again("timed out")),
            Err(Failure::again("busy")),
            Ok(7),
        ]));
        assert_eq!(done.ok(), Some(7));
        assert_eq!(count, 3);
    }

    /// A no is not asked again, and is said as a no.
    ///
    /// Broken deliberately by treating `Refused` as `Again`: it was asked
    /// four times and came back `Down`.
    #[test]
    fn a_refusal_is_not_asked_again() {
        let (done, count) = run(asked(vec![Err(Failure::Refused("too long".to_string()))]));
        assert!(matches!(done, Err(Failed::Refused(why)) if why == "too long"));
        assert_eq!(count, 1);
    }

    /// Asked once and after every pause, then taken to be down -- with the
    /// last thing that would not answer as the reason.
    ///
    /// Broken deliberately by giving up before the last pause's try: the
    /// count was three, not four.
    #[test]
    fn never_answered_is_down() {
        let (done, count) = run(asked((0..4).map(|n| Err(Failure::again(n))).collect()));
        assert!(matches!(done, Err(Failed::Down { why, answered: false }) if why == "3"));
        assert_eq!(count, NONE.len() + 1);
    }

    /// One id for one thing said, kept when it is said again, and a
    /// different one for the next -- and for the words said instead of a
    /// card, which are another message.
    ///
    /// Broken deliberately three ways. Counting once per process rather
    /// than per saying: the next had the first's id. Giving `again` a fresh
    /// id: a retried message was not one Feishu could tell from a new one.
    /// And giving `instead` the card's id: Feishu took the words for the
    /// card it had refused, and said nothing.
    #[test]
    fn a_saying_keeps_its_id() {
        let out = || Out::Name {
            id: "U".to_string(),
        };
        let mut first = Saying::from(out());
        let tried = first.first_tried();
        let again = first.again(out());
        let instead = first.instead(out());
        let next = Saying::from(out());
        assert_eq!(first.id, again.id);
        assert_eq!(again.first_tried, Some(tried));
        assert_ne!(first.id, instead.id);
        assert_eq!(instead.first_tried, None);
        assert_ne!(first.id, next.id);
        assert!(first.id.len() <= 50, "past Feishu's limit: {}", first.id);
    }
}
