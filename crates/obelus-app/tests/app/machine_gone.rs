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
        why: Some("connection refused".to_string()),
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

/// Escape gives up on the other machine and asks which project here, as a
/// start with nothing to go on does -- and enter, which this page once
/// took for it, does nothing.
///
/// Broken deliberately by leaving the machine where it was on escape (no
/// `machine_gone = None`): the page stayed up over the question. And by
/// hearing enter for it again (`KeyCode::Enter` for `Esc` in
/// `the_page_saying_it_has_gone`): enter let go of the machine.
#[test]
fn working_here_asks_which_project() {
    let mut app = gone(true);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.machine_gone().is_some(), "enter gave up on the machine");
    support::press(&mut app, KeyCode::Esc);
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
        why: Some(
            "No connection could be made because the target machine actively refused it. \
              (os error 10061)"
                .to_string(),
        ),
        trying: false,
    });
    let screen = support::render(&mut app, 60, 24);
    for said in ["No connection could be made", "(os error 10061)"] {
        assert!(
            screen.contains(said),
            "the page does not say {said:?}:\n{screen}"
        );
    }
}

/// A machine still being connected to says so, with a mark that turns and
/// no key to try again -- there is nothing yet to try again -- and becomes
/// the page saying it cannot be reached once a try says why.
///
/// Broken deliberately twice. By not hearing what the try said
/// (`Event::Unreachable` handled as nothing): the page said it was
/// connecting for ever. And by trying again on the key while connecting
/// (the `why.is_some()` guard left out of the page's keys): `r` let go of
/// the window to start the try that was already under way again.
#[test]
fn a_machine_being_connected_to_says_so_until_a_try_says_why_not() {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 80, 24);
    app.the_machine_has_gone(MachineGone {
        at: "devbox".to_string(),
        key: "00ff".to_string(),
        window: None,
        why: None,
        trying: true,
    });
    let screen = support::render(&mut app, 80, 24);
    let connecting = format!("{} Connecting to devbox", obelus_ui::spinning(app.phase()));
    assert!(
        screen.contains(&connecting),
        "the page does not say it is connecting:
{screen}"
    );
    assert!(
        !screen.contains("Try again"),
        "a key to try again while trying:
{screen}"
    );
    support::press(&mut app, KeyCode::Char(obelus_ui::gone::AGAIN));
    assert_eq!(app.goes_to(), None, "the key started the try again");

    app.handle(Event::Unreachable {
        at: "devbox".to_string(),
        why: "connection refused".to_string(),
    });
    let screen = support::render(&mut app, 80, 24);
    for said in [
        "Obelus cannot reach devbox",
        "connection refused",
        "Try again",
    ] {
        assert!(
            screen.contains(said),
            "the page does not say {said:?}:
{screen}"
        );
    }
}
