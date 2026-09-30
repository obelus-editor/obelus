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
//! conversation the request named, so an Obelus that routed by "the one
//! opened last" would look correct against an agent that did the same -- and
//! the two would be wrong together.

use obelus_agent::acp::link::Said;

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
    let mut talk = Talk::start(
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
    // Asked for, because nothing is opened on the way up any more: a
    // conversation minted before anybody said what they wanted is one the
    // agent does not keep and nobody owns.
    talk.open();
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

/// A prompt with nowhere to go yet says something is happening.
///
/// The reader pressed enter and their words are on the page, so from where
/// they sit a turn is under way -- and it is, it is waiting for somewhere
/// to go. Nothing on screen said so.
///
/// Which is not a corner anybody has to go looking for. An agent replaying
/// a conversation sends every word of it *before* it answers the request
/// that asked for it, so the page fills and the session arrives after: a
/// reader looking at a conversation that is plainly all there types into
/// it, and that prompt is held. The turn went out when the session landed
/// and the answer came back in its own time, with the whole wait spent
/// looking at a page that said nothing was happening.
///
/// Broken deliberately by asking only the session: a prompt held has no
/// session to ask about, so the answer is no and this goes red.
#[test]
fn a_prompt_waiting_for_a_conversation_says_it_is_thinking() {
    let (sender, _events): (_, Receiver<Event>) = channel();
    let mut talk = Talk::start(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        Path::new("."),
        None,
        sender,
    );
    let asking = talk.open();
    assert!(
        !talk.is_thinking(None, Some(asking)),
        "it says something is happening before anything was said"
    );
    // Nowhere to send it: the session has been asked for, and there cannot
    // be one yet -- the process has only just been started.
    assert!(
        !talk.say(
            None,
            Some(asking),
            vec![Said::Words("hello".to_string())],
            None
        ),
        "a prompt went somewhere when there was nowhere to send it"
    );
    assert!(
        talk.is_thinking(None, Some(asking)),
        "a prompt waiting to be sent says nothing is happening"
    );
}

/// What is typed into a conversation still being opened goes out in that
/// conversation, when two are being opened at once.
///
/// Which is the ordinary case since opening a conversation asks for its
/// session: the reader opens one, goes straight to another and types. One
/// slot held the words and the first session to arrive took them, which
/// was the first conversation's -- the reader's message sent into a
/// conversation they had left, and the one they were typing in with
/// nothing in it.
///
/// Deliberate break: hand what is held to whichever answer comes first,
/// the way the one slot did. The turn is then in flight in `first`.
#[test]
fn a_prompt_held_for_the_second_of_two_goes_out_in_the_second() {
    let (sender, events) = channel();
    let mut talk = Talk::start(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        Path::new("."),
        None,
        sender,
    );
    let one = talk.open();
    let two = talk.open();
    talk.say(
        None,
        Some(two),
        vec![Said::Words("do it slowly".to_string())],
        None,
    );

    // And each answer says which request it is.
    let mut answers = Vec::new();
    pump(&mut talk, &events, "both conversations", |_, incoming| {
        if let Incoming::Started {
            session, asking, ..
        } = incoming
        {
            answers.push((session.clone(), *asking));
        }
        answers.len() == 2
    });
    let [(first, asked_first), (second, asked_second)] = answers.as_slice() else {
        unreachable!("two answers");
    };
    assert_eq!(
        (*asked_first, *asked_second),
        (Some(one), Some(two)),
        "the answers do not say which request they are"
    );
    assert!(
        talk.is_thinking(Some(second), None),
        "the words did not go out in the conversation they were typed in"
    );
    assert!(
        !talk.is_thinking(Some(first), None),
        "the words went out in the other conversation"
    );
}

/// Nothing is opened that nobody asked for.
///
/// One was, the moment the connection came up, on the grounds that the
/// reader opening the view is a request to talk. A view can open on a note
/// that already names a conversation now, and then that first session is
/// one nobody wants: empty, so the agent never writes it down and it is
/// gone by the next start -- and Obelus wrote it against the note in place
/// of the conversation the reader had actually been having, asked for it
/// the next morning, was told there was no such thing, and opened another
/// empty one to replace it. The reader's conversation went in the first
/// round and every round after was the same round again.
///
/// Broken deliberately by opening one on the way up again: a conversation
/// arrives before anything has asked for one, and the first half of this
/// goes red.
#[test]
fn a_conversation_is_opened_only_when_one_is_asked_for() {
    let (sender, events) = channel();
    let mut talk = Talk::start(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        Path::new("."),
        None,
        sender,
    );
    // Up and talking -- the handshake has landed, which is what the name
    // says -- and still nothing open. Its own loop, because the handshake
    // is folded into the handle rather than handed on, so there is no
    // message to wait for: what is waited for is the handle knowing its name.
    let deadline = Instant::now() + PATIENCE;
    let mut shaken = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "gave up waiting for the agent");
        // Once the handshake has landed, a moment longer: an agent that
        // opens one on the way up sends it in the same breath, and a loop
        // that stopped at the handshake would be waiting for a message
        // that had not been sent yet and calling that proof.
        let wait = match shaken {
            true => Duration::from_millis(500),
            false => left,
        };
        let Ok(Event::Agent(obelus_agent::Event::Acp(incoming))) = events.recv_timeout(wait) else {
            if shaken {
                break;
            }
            continue;
        };
        assert!(
            !matches!(incoming, Incoming::Started { .. }),
            "a conversation was opened before anything asked for one"
        );
        talk.on(incoming);
        shaken |= talk.info().is_some();
    }

    // And one when one is asked for.
    talk.open();
    let _ = opened(&mut talk, &events);
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
/// and Obelus threw that name away until there were two to tell apart. An
/// Obelus that still did would put the second agent's answer into the first
/// reader's transcript.
#[test]
fn an_answer_comes_back_in_the_conversation_it_was_asked_in() {
    let (mut talk, events) = talking();
    let first = opened(&mut talk, &events);
    talk.open();
    let second = opened(&mut talk, &events);

    talk.say(
        Some(&second),
        None,
        vec![Said::Words("/help".to_string())],
        None,
    );

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
    talk.say(
        Some(&first),
        None,
        vec![Said::Words("do it slowly".to_string())],
        None,
    );
    // A turn that finishes on its own, so that the only thing which could
    // stop it arriving is the interruption meant for the other one.
    talk.say(
        Some(&second),
        None,
        vec![Said::Words("/help".to_string())],
        None,
    );
    talk.interrupt(Some(&first));

    let mut answered = None;
    pump(
        &mut talk,
        &events,
        "the other answer",
        |_, incoming| match incoming {
            Incoming::Ended { session, why, .. } if *session == second => {
                answered = Some(why.clone());
                true
            }
            _ => false,
        },
    );
    // Spelled the way the wire spells it, which is the way Obelus now
    // reads it: `endturn` was `{:?}` lowercased, and every arm written
    // against the protocol's own `end_turn` was unreachable.
    assert_eq!(
        answered.as_deref(),
        Some("end_turn"),
        "the second conversation's turn did not finish on its own"
    );
}

/// What Obelus has to keep, and what it does not.
///
/// The agent keeps every word: `session/load` replays it, so Obelus keeps
/// none. What Obelus keeps is the one thing the agent cannot know, which is
/// which of its conversations is about which of this tree's notes -- and
/// the name the agent gave that conversation, because that is what a list
/// of open documents calls it and a replay is not obliged to send it again.
#[test]
fn what_is_remembered_is_the_name_and_nothing_that_was_said() {
    use obelus_agent::{acp::sessions, chats::ChatId};
    use obelus_git::todo::NoteId;

    // The table lives in Obelus's state directory, and a test that wrote to
    // the reader's would be a test that left something on their machine.
    // `Scratch` says where it goes instead, for every test that makes one:
    // the notes are kept there too now, so this is no longer the one test
    // in the suite that touches it.
    let scratch = support::Scratch::new("sessions-kept");
    let root = scratch.path();
    let note = NoteId::read("ABCDEFGH").expect("a name");

    sessions::change(root, Some(std::slice::from_ref(&note)), |kept| {
        kept.put(
            &ChatId::Note(note.clone()),
            "fake",
            sessions::Kept {
                session: "s-1".to_string(),
                title: Some("why refilter drops rows".to_string()),
                told: None,
                introduced: false,
                last: None,
            },
        );
    });

    let back = sessions::read(root).remembered().expect("the table");
    let kept = back
        .get(&ChatId::Note(note.clone()), "fake")
        .expect("the conversation");
    assert_eq!(kept.session, "s-1");
    assert_eq!(kept.title.as_deref(), Some("why refilter drops rows"));

    // And a second sitting on the same tree does not put back what the
    // first one wrote: read-modify-write, because a second Obelus on one
    // tree is an ordinary thing to have running.
    let other = NoteId::read("JKMNPQRS").expect("a name");
    sessions::change(root, Some(&[note.clone(), other.clone()]), |kept| {
        kept.put(
            &ChatId::Note(other.clone()),
            "fake",
            sessions::Kept {
                session: "s-2".to_string(),
                title: None,
                told: None,
                introduced: false,
                last: None,
            },
        );
    });
    let back = sessions::read(root).remembered().expect("the table");
    assert!(
        back.get(&ChatId::Note(note.clone()), "fake").is_some(),
        "the second write put back what the first wrote"
    );
    assert!(back.get(&ChatId::Note(other.clone()), "fake").is_some());

    // A note that has gone takes its conversation with it, collected on the
    // way past rather than when the note was deleted -- because a note can
    // go without Obelus watching.
    sessions::change(root, Some(std::slice::from_ref(&other)), |_| {});
    let back = sessions::read(root).remembered().expect("the table");
    assert!(
        back.get(&ChatId::Note(note.clone()), "fake").is_none(),
        "a conversation outlived the note it was about"
    );
}
