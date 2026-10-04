//! What is to be said while a connection is not up yet, kept in order.
//!
//! **Kept, and only so much of it.** A platform that cannot be reached is
//! tried again, and one that refused is too -- what a platform calls a
//! refusal includes being busy -- so a window can wait for one for as long
//! as it is open, and everything said meanwhile was kept for it, without
//! end. What is kept now is the newest of it, which is what a reader
//! catching up wants to read; what is let go of is said in the log, and a
//! thread that was being asked for is said not to have opened, so that the
//! conversation it was for is not left waiting on it.

use std::{collections::VecDeque, sync::Arc};

use obelus_sink::Sink;

use crate::{Event, model::Out};

/// How much is kept: a turn's worth of a busy afternoon, several times over.
const KEPT: usize = 256;

/// What is waiting to be said, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Waiting {
    outs: VecDeque<Out>,
    /// Whether anything has been let go of yet, so the log says it once.
    dropped: bool,
}

impl Waiting {
    /// Keeps one more, letting go of the oldest where there is no room.
    pub(crate) fn keep(&mut self, out: Out, sink: &Arc<dyn Sink<Event>>) {
        if self.outs.len() >= KEPT
            && let Some(oldest) = self.outs.pop_front()
        {
            if !self.dropped {
                tracing::warn!(kept = KEPT, "too much waiting to be said; the oldest goes");
                self.dropped = true;
            }
            if let Out::Open { asked, .. } = oldest {
                let _ = sink.send(Event::Unopened {
                    asked,
                    waited: true,
                });
            }
        }
        self.outs.push_back(out);
    }

    /// What was kept, oldest first.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = Out> + '_ {
        self.outs.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (sender, heard) = std::sync::mpsc::channel::<Event>();
        let sink: Arc<dyn Sink<Event>> = Arc::new(sender);
        let mut waiting = Waiting::default();
        waiting.keep(
            Out::Open {
                asked: 7,
                room: "R".to_string(),
                head: crate::model::Head {
                    title: "t".to_string(),
                    place: "p".to_string(),
                    state: None,
                },
            },
            &sink,
        );
        for number in 0..300 {
            waiting.keep(
                Out::Name {
                    id: number.to_string(),
                },
                &sink,
            );
        }
        let kept: Vec<Out> = waiting.drain().collect();
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
}
