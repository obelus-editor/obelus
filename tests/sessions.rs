//! Several conversations on one agent, and keeping them apart.
//!
//! Driven through [`obelus::acp::Talk`] rather than through the application,
//! because the application still shows one conversation at a time: what is
//! being tested is the half underneath, which is where two of them first
//! become possible and first become able to spoil each other.
//!
//! The agent is `tests/fixtures/fake-agent.sh`, a real process on the other
//! end of a real pipe. It mints `s-1`, `s-2`, … and answers about whichever
//! conversation the request named, so an obelus that routed by "the one
//! opened last" would look correct against an agent that did the same -- and
//! the two would be wrong together.

use std::{
    path::Path,
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use obelus::{
    acp::{Incoming, SessionId, Talk, Update},
    event::Event,
};

/// Generous: it is a shell script starting up, and a test that fails because
/// a machine was busy is a test nobody trusts.
const PATIENCE: Duration = Duration::from_secs(10);

/// The fake agent, running, with the channel its words arrive on.
fn talking() -> (Talk, Receiver<Event>) {
    let (sender, events) = channel();
    let talk = Talk::start(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        Path::new("."),
        sender,
    );
    (talk, events)
}

/// Every message until `until` answers, folding each into the handle.
///
/// Folded as the application folds them, because half of what arrives is
/// state rather than words -- and the half that is state is exactly what
/// two conversations can spoil for each other.
fn pump(
    talk: &mut Talk,
    events: &Receiver<Event>,
    what: &str,
    mut until: impl FnMut(&mut Talk, &Incoming) -> bool,
) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "gave up waiting for {what}");
        let Ok(Event::Acp(incoming)) = events.recv_timeout(left) else {
            continue;
        };
        let seen = talk.on(incoming);
        if let Some(seen) = seen
            && until(talk, &seen)
        {
            return;
        }
    }
}

/// Waits for the next conversation to open, and says which it is.
fn opened(talk: &mut Talk, events: &Receiver<Event>) -> SessionId {
    let mut which = None;
    pump(talk, events, "a conversation to open", |_, incoming| {
        if let Incoming::Started { session, .. } = incoming {
            which = Some(session.clone());
            return true;
        }
        false
    });
    which.expect("the conversation that opened")
}

/// One process, two conversations, and each is told apart from the other.
#[test]
fn a_second_conversation_opens_on_the_same_agent() {
    let (mut talk, events) = talking();
    let first = opened(&mut talk, &events);

    talk.open();
    let second = opened(&mut talk, &events);

    assert_ne!(
        first, second,
        "the agent gave both conversations one name, so nothing below could tell them apart"
    );
}

/// What is said in one conversation arrives in that one.
///
/// The words come back on a `session/update` that names the conversation,
/// and obelus threw that name away until there were two to tell apart. An
/// obelus that still did would put the second agent's answer into the first
/// reader's transcript.
#[test]
fn an_answer_comes_back_in_the_conversation_it_was_asked_in() {
    let (mut talk, events) = talking();
    let first = opened(&mut talk, &events);
    talk.open();
    let second = opened(&mut talk, &events);

    talk.say(Some(&second), "/help");

    let mut whose = None;
    pump(&mut talk, &events, "an answer", |_, incoming| {
        if let Incoming::Update {
            session,
            update: Update::Said(_),
        } = incoming
        {
            whose = Some(session.clone());
            return true;
        }
        false
    });
    assert_eq!(
        whose.as_ref(),
        Some(&second),
        "the answer came back in the wrong conversation"
    );
    assert_ne!(whose.as_ref(), Some(&first));
}

/// Stopping one conversation does not silence the other.
///
/// This is the one worth the whole split. The flag that says "this turn was
/// given up on" was one flag for the connection, read by the callback that
/// carries an answer back -- so interrupting one conversation threw away the
/// answer arriving in the other, with nothing on screen to say so and no way
/// to notice except by talking in two at once.
///
/// Give them one flag again and this fails: the first conversation's
/// interruption sets it, and the second's answer is dropped on the way in.
#[test]
fn interrupting_one_conversation_does_not_swallow_the_others_answer() {
    let (mut talk, events) = talking();
    let first = opened(&mut talk, &events);
    talk.open();
    let second = opened(&mut talk, &events);

    // A turn that stays in flight: the agent says nothing at all about this
    // one until it is cancelled, which is what leaves something to stop.
    talk.say(Some(&first), "do it slowly");
    // A turn that finishes on its own, so that the only thing which could
    // stop it arriving is the interruption meant for the other one.
    talk.say(Some(&second), "/help");
    talk.interrupt(Some(&first));

    let mut answered = None;
    pump(
        &mut talk,
        &events,
        "the other answer",
        |_, incoming| match incoming {
            Incoming::Ended { session, why } if *session == second => {
                answered = Some(why.clone());
                true
            }
            _ => false,
        },
    );
    assert_eq!(
        answered.as_deref(),
        Some("endturn"),
        "the second conversation's turn did not finish on its own"
    );
}
