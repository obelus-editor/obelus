//! The page saying the machine a window was drawing an Obelus on cannot be
//! reached: what it says, and what each of its keys does.

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, Going, MachineGone},
    event::Event,
};

use crate::support;

/// A window whose machine went, as the front end leaves it.
fn gone(trying: bool) -> App {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 80, 24);
    app.the_machine_has_gone(MachineGone {
        at: "devbox".to_string(),
        key: "00ff".to_string(),
        window: Some("w1".to_string()),
        why: "connection refused".to_string(),
        trying,
    });
    app
}

/// Where trying again takes the window: back to the Obelus it was on.
fn back() -> Going {
    Going::To {
        at: "devbox".to_string(),
        key: "00ff".to_string(),
        window: Some("w1".to_string()),
    }
}

/// The page says which machine, why, that it is being tried again, and
/// the three keys -- over the whole screen.
///
/// Broken deliberately by leaving the machine out of `GoneView::new`
/// (`machine_gone` not asked): the screen showed nothing of it.
#[test]
fn the_page_says_which_machine_and_what_can_be_done() {
    let mut app = gone(true);
    let screen = support::render(&mut app, 80, 24);
    for said in [
        "Obelus cannot reach devbox",
        "connection refused",
        "Trying again",
        "Try again",
        "Work on this machine",
        "Leave",
    ] {
        assert!(
            screen.contains(said),
            "the page does not say {said:?}:\n{screen}"
        );
    }
}

/// Trying again takes the window back to the Obelus it was on, under the
/// window's own name.
///
/// Broken deliberately by going back without the window's name
/// (`window: None` in `try_the_machine_again`): a new Obelus would have
/// been started there, and the one with the reader's work in it left.
#[test]
fn trying_again_goes_back_to_the_window_it_was() {
    let mut app = gone(true);
    support::press(&mut app, KeyCode::Char(obelus_ui::gone::AGAIN));
    assert_eq!(app.goes_to(), Some(&back()));
    assert!(app.should_quit());
}

/// Working on this machine lets the other go and asks which project, as a
/// start with nothing to go on does.
///
/// Broken deliberately by leaving the machine where it was on enter (no
/// `machine_gone = None`): the page stayed up over the question.
#[test]
fn working_here_asks_which_project() {
    let mut app = gone(true);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.machine_gone().is_none(), "the page stayed up");
    assert!(!app.has_a_project(), "nothing asked which project");
    assert_eq!(app.goes_to(), None);
}

/// The machine answering again takes the window back -- where it is being
/// waited for, and only for that machine.
///
/// Broken deliberately by going back whoever answered (`gone.at == at`
/// left out of `reachable`): another machine answering took the window to
/// this one.
#[test]
fn the_machine_answering_takes_the_window_back() {
    let mut app = gone(true);
    app.handle(Event::Reachable("elsewhere".to_string()));
    assert_eq!(
        app.goes_to(),
        None,
        "another machine answering moved the window"
    );
    app.handle(Event::Reachable("devbox".to_string()));
    assert_eq!(app.goes_to(), Some(&back()));

    // And not where it would not let the window in: answering is not
    // agreeing, and going back would be refused again.
    let mut refused = gone(false);
    refused.handle(Event::Reachable("devbox".to_string()));
    assert_eq!(refused.goes_to(), None);
}

/// A reason longer than the page is wide is said whole, over the rows it
/// takes -- both what was being done, at its head, and what the system
/// answered, at its tail.
///
/// Broken deliberately by cutting it to one row from the left, as it was
/// (`truncate_from_left(why, room)` for the wrapping in `GoneView`): the
/// head went, and the page said only the end of a sentence.
#[test]
fn a_long_reason_is_said_whole() {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 24);
    app.the_machine_has_gone(MachineGone {
        at: "devbox".to_string(),
        key: "00ff".to_string(),
        window: None,
        why: "connecting to devbox:7787: No connection could be made because the target \
              machine actively refused it."
            .to_string(),
        trying: false,
    });
    let screen = support::render(&mut app, 60, 24);
    for said in ["connecting to devbox:7787", "actively refused it."] {
        assert!(
            screen.contains(said),
            "the page does not say {said:?}:\n{screen}"
        );
    }
}
