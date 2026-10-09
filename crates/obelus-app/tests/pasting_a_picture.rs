//! A picture on the clipboard, pasted into a conversation.
//!
//! Its own binary because the only way a test can put a picture on the
//! clipboard is to be its owner (`obelus_clipboard::owned_by`), and that is
//! installed once per process: every paste in a binary beside these would
//! find a picture. The owner never changes what it holds, so the tests here
//! need not take turns.
//!
//! The agent is `tests/fixtures/fake-agent.sh`, told to say in the handshake
//! that a prompt may carry a picture -- see `agent.rs` for why it is `sh`.

mod support;

use std::{
    path::Path,
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use crossterm::event::KeyCode;
use obelus_app::{app::App, event::Event};
use obelus_clipboard::{Owner, Provider};

const WIDTH: u16 = 76;
const HEIGHT: u16 = 24;

/// How long to wait for the agent to say something -- see `agent.rs`, whose
/// reasons are these.
fn patience() -> Duration {
    std::env::var("OBELUS_PATIENCE")
        .ok()
        .and_then(|seconds| seconds.trim().parse().ok())
        .map_or(Duration::from_secs(180), Duration::from_secs)
}

/// A clipboard holding a screenshot and nothing else.
struct Screenshot;

impl Owner for Screenshot {
    fn offer(&self, _shapes: Vec<(String, Vec<u8>)>) -> bool {
        true
    }

    fn holding(&self, mime: &str) -> Option<Vec<u8>> {
        (mime == "image/png").then(|| b"\x89PNG\r\n\x1a\n and the rest of it".to_vec())
    }

    fn holds(&self) -> Vec<String> {
        vec!["image/png".to_string()]
    }
}

/// The fake agent, talking, with a screenshot on the clipboard.
fn talking() -> (App, Receiver<Event>) {
    static OWNED: std::sync::Once = std::sync::Once::new();
    OWNED.call_once(|| obelus_clipboard::owned_by(Box::new(Screenshot)));
    // Nothing of the clipboard of whoever is running the suite.
    obelus_clipboard::use_provider_for_test(Provider::Kept);

    let (sender, events) = channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(
        std::env::temp_dir().join(format!("obelus-picture-tests-{}", std::process::id())),
    );
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app.talk_to(
        "fake",
        Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "pictures".to_string(),
        ],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    (app, events)
}

/// Handles what arrives until `until` holds, drawing between.
fn pump(app: &mut App, events: &Receiver<Event>, what: &str, until: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + patience();
    while !until(app) {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "gave up waiting for {what}");
        match events.recv_timeout(left) {
            Ok(event) => app.handle(event),
            Err(_) => panic!("nothing arrived while waiting for {what}"),
        }
        support::lay_out(app, WIDTH, HEIGHT);
    }
}

/// How many pictures are in the box a message is written in.
fn pictures_in_the_box(app: &App) -> usize {
    use obelus_component::composer::Part;
    app.chat()
        .expect("the chat")
        .writing()
        .parts()
        .into_iter()
        .filter(|part| matches!(part, Part::Picture(_)))
        .count()
}

/// With nothing over the box, a picture pasted goes in it.
///
/// What says the clipboard below is one the paste can read, so that the
/// next test's "nothing arrived" is about the card and not about the fake.
///
/// Deliberate break: `can_take_a_picture` answering `false` leaves the box
/// empty.
#[test]
fn a_picture_pasted_in_a_conversation_goes_in_the_box() {
    let (mut app, _events) = talking();
    app.paste();
    assert_eq!(
        pictures_in_the_box(&app),
        1,
        "the picture is not in the box"
    );
}

/// A card with room for the reader's own words takes words, and a picture
/// pasted while it is up goes nowhere -- not into the box the card covers,
/// where it would wait unseen and go with the next message -- and the
/// reader is told where a picture does go.
///
/// Deliberate break: `box_takes_a_picture` answering
/// `conversation_takes_text` alone, which is what it was, puts the picture
/// in the box behind the card.
#[test]
fn a_picture_pasted_on_a_card_does_not_go_behind_it() {
    let (mut app, events) = talking();
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    // Twice, as `agent.rs` does: the first takes the command the list
    // under the box offers, the second sends it.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    app.paste();
    assert_eq!(
        pictures_in_the_box(&app),
        0,
        "the picture went into the message box under the card"
    );
    assert_eq!(app.note(), Some("A picture goes in a message to an agent"));
}
