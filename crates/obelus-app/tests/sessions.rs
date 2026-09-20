//! Several conversations on one agent, and keeping them apart.
//!
//! Driven through [`obelus_agent::acp::Talk`] rather than through the
//! application, because the half underneath is where two conversations first
//! become possible and first become able to spoil each other -- a session
//! routed to the wrong one is wrong before anything has been drawn. What the
//! application does with them is [`tests/layers.rs`] and [`tests/agent.rs`].
//!
//! The agent is `tests/fixtures/fake-agent.sh`, a real process on the other
//! end of a real pipe. It mints `s-1`, `s-2`, … and answers about whichever
//! conversation the request named, so an obelus that routed by "the one
//! opened last" would look correct against an agent that did the same -- and
//! the two would be wrong together.

mod support;

use std::{
    path::Path,
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use obelus_agent::acp::{Incoming, SessionId, Talk, Update};
use obelus_app::event::Event;

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
        // No tools offered: what these tests are about is the sessions, and
        // an agent told about a server nobody is running would be an agent
        // spending its first moments failing to reach one.
        None,
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
        let Ok(Event::Agent(obelus_agent::Event::Acp(incoming))) = events.recv_timeout(left) else {
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

    talk.say(Some(&second), "/help", None);

    let mut whose = None;
    pump(&mut talk, &events, "An answer", |_, incoming| {
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
    talk.say(Some(&first), "do it slowly", None);
    // A turn that finishes on its own, so that the only thing which could
    // stop it arriving is the interruption meant for the other one.
    talk.say(Some(&second), "/help", None);
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
    // Spelled the way the wire spells it, which is the way obelus now
    // reads it: `endturn` was `{:?}` lowercased, and every arm written
    // against the protocol's own `end_turn` was unreachable.
    assert_eq!(
        answered.as_deref(),
        Some("end_turn"),
        "the second conversation's turn did not finish on its own"
    );
}

/// What obelus has to keep, and what it does not.
///
/// The agent keeps every word: `session/load` replays it, so obelus keeps
/// none. What obelus keeps is the one thing the agent cannot know, which is
/// which of its conversations is about which of this tree's notes -- and
/// the name the agent gave that conversation, because that is what a list
/// of open documents calls it and a replay is not obliged to send it again.
#[test]
fn what_is_remembered_is_the_name_and_nothing_that_was_said() {
    use obelus_agent::acp::sessions;
    use obelus_git::todo::NoteId;

    let scratch = support::Scratch::new("sessions-kept");
    // The table lives in obelus's state directory, and a test that wrote to
    // the reader's would be a test that left something on their machine.
    // Set for this binary, which is the only one that touches it.
    //
    // SAFETY: nothing else in this test binary reads the environment, and
    // the tests that share it do not touch the state directory at all.
    unsafe {
        std::env::set_var("XDG_STATE_HOME", scratch.path());
    }
    let root = scratch.path();
    let note = NoteId::read("ABCDEFGH").expect("a name");

    sessions::change(root, std::slice::from_ref(&note), |kept| {
        kept.put(
            &note,
            "fake",
            sessions::Kept {
                session: "s-1".to_string(),
                title: Some("why refilter drops rows".to_string()),
            },
        );
    });

    let back = sessions::read(root);
    let kept = back.get(&note, "fake").expect("the conversation");
    assert_eq!(kept.session, "s-1");
    assert_eq!(kept.title.as_deref(), Some("why refilter drops rows"));

    // And a second sitting on the same tree does not put back what the
    // first one wrote: read-modify-write, because a second obelus on one
    // tree is an ordinary thing to have running.
    let other = NoteId::read("JKMNPQRS").expect("a name");
    sessions::change(root, &[note.clone(), other.clone()], |kept| {
        kept.put(
            &other,
            "fake",
            sessions::Kept {
                session: "s-2".to_string(),
                title: None,
            },
        );
    });
    let back = sessions::read(root);
    assert!(
        back.get(&note, "fake").is_some(),
        "the second write put back what the first wrote"
    );
    assert!(back.get(&other, "fake").is_some());

    // A note that has gone takes its conversation with it, collected on the
    // way past rather than when the note was deleted -- because a note can
    // go without obelus watching.
    sessions::change(root, std::slice::from_ref(&other), |_| {});
    let back = sessions::read(root);
    assert!(
        back.get(&note, "fake").is_none(),
        "a conversation outlived the note it was about"
    );
}
