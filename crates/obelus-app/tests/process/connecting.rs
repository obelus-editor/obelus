//! `connect-to-server` and `disconnect-from-server`: the list of machines,
//! the row that asks for a new one, and where the window is told to go.
//!
//! Here and not beside the application's tests because the list is one
//! file for the process -- the machines a window has got into, and their
//! keys -- and these write it.

use crossterm::event::KeyCode;
use obelus_app::app::{App, Going};
use obelus_command::Command;
use obelus_component::picker::PickerValue;

use crate::support;

/// The machines the list is to have, written where Obelus keeps them.
fn keep_servers(text: &str) {
    support::state_of_its_own();
    let directory = obelus_logging::state_directory().expect("somewhere to keep them");
    std::fs::create_dir_all(&directory).expect("the state directory");
    std::fs::write(directory.join("servers.toml"), text).expect("the servers");
}

/// A window on nothing in particular, with the list of machines open.
fn connecting() -> App {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 80, 24);
    obelus_app::app::dispatch::dispatch(&mut app, Command::ServerConnect);
    app
}

/// What the list's rows are, in order.
fn rows(app: &App) -> Vec<(String, Option<String>)> {
    app.picker()
        .expect("the list of machines")
        .matches()
        .map(|item| (item.label.clone(), item.section.clone()))
        .collect()
}

/// The list opens on the row that asks for a new machine, and the ones
/// there have been come after it, newest first, in their runs of days;
/// choosing one goes to it with the key kept for it.
///
/// Broken deliberately twice. By listing the machines before the new row
/// (`once(new)` chained after them): the list opened on a machine. And by
/// going nowhere on a machine chosen (`connect_to` leaving `going` alone):
/// the window was told to leave, not to go.
#[test]
fn the_list_offers_a_new_machine_first_and_the_ones_there_have_been_after() {
    let _turn = crate::turn();
    let now = jiff::Timestamp::now().as_second();
    keep_servers(&format!(
        "[[server]]\nat = \"old\"\nkey = \"aa\"\nlast = 0\n\n\
         [[server]]\nat = \"devbox:9000\"\nkey = \"00ff\"\nlast = {now}\n"
    ));
    let mut app = connecting();
    assert_eq!(
        rows(&app),
        [
            ("New connection".to_string(), None),
            ("devbox:9000".to_string(), Some("Today".to_string())),
            ("old".to_string(), Some("Earlier".to_string())),
        ]
    );
    assert!(matches!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| &item.value),
        Some(PickerValue::NewServer)
    ));
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.goes_to(),
        Some(&Going::To {
            at: "devbox:9000".to_string(),
            key: "00ff".to_string(),
            window: None,
        })
    );
    assert!(
        app.should_quit(),
        "the window was not let go of to go there"
    );
}

/// The new row asks where on the row the list was filtered on, and then
/// for the key of a machine none is kept for.
///
/// Broken deliberately by going to a new machine with no key asked for
/// (`connect_to` leaving for it whatever is kept): the window was sent off
/// with nothing to knock with.
#[test]
fn a_new_machine_is_asked_for_where_and_then_for_its_key() {
    let _turn = crate::turn();
    keep_servers("");
    let mut app = connecting();
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.picker().is_none(),
        "the list stayed up over the question"
    );
    assert_eq!(
        app.prompt()
            .map(obelus_component::prompt::Prompt::line)
            .as_deref(),
        Some("Connect to: ")
    );
    support::type_text(&mut app, "fresh:9000");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.goes_to(), None, "gone with no key");
    assert_eq!(
        app.prompt()
            .map(obelus_component::prompt::Prompt::line)
            .as_deref(),
        Some("Key: ")
    );
    support::type_text(&mut app, "abcdef12");
    assert_eq!(
        app.prompt()
            .map(obelus_component::prompt::Prompt::line)
            .as_deref(),
        Some("Key: \u{2022}\u{2022}\u{2022}\u{2022}ef12"),
        "the key was drawn as it was typed"
    );
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.goes_to(),
        Some(&Going::To {
            at: "fresh:9000".to_string(),
            key: "abcdef12".to_string(),
            window: None,
        })
    );
}

/// A key typed that is not one a server prints is turned down at the
/// question, and the window stays where it is -- rather than letting go of
/// this machine's Obelus and finding out only then, with nothing left to
/// draw.
///
/// Broken deliberately by going with whatever was typed (the `as_a_key`
/// check left out of `connect_with`): the window was let go of with a key
/// nothing could read, and ended.
#[test]
fn a_key_that_is_not_one_is_turned_down_at_the_question() {
    let _turn = crate::turn();
    keep_servers("");
    let mut app = connecting();
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "fresh:9000");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "not a key");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.goes_to(), None, "gone with a key nothing reads");
    assert!(!app.should_quit(), "the window was let go of");
    let screen = support::render(&mut app, 100, 24);
    assert!(
        screen.contains("Not a key a server prints"),
        "nothing said why:\n{screen}"
    );
}

/// A reader who asked to go to a machine and then asked to leave has asked
/// to leave.
///
/// Broken deliberately by keeping where the window was going when leaving
/// is asked for (no `going = None` in `request_quit`): the key that leaves
/// went to the machine instead.
#[test]
fn leaving_after_going_is_leaving() {
    let _turn = crate::turn();
    keep_servers("[[server]]\nat = \"devbox\"\nkey = \"00ff\"\nlast = 1\n");
    let mut app = connecting();
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.goes_to().is_some());
    obelus_app::app::dispatch::dispatch(&mut app, Command::Quit);
    assert_eq!(app.goes_to(), None);
}

/// Connecting is offered on this machine and the way back on another, and
/// neither where the other is.
///
/// Broken deliberately by offering the way back everywhere
/// (`Requires::AnotherMachine` answering `true`): a window on this machine
/// offered to go back to it.
#[test]
fn the_way_back_is_offered_only_on_another_machine() {
    let _turn = crate::turn();
    let mut app = App::new(Vec::new());
    assert!(app.offers(Command::ServerConnect));
    assert!(!app.offers(Command::ServerDisconnect));
    app.drawn_afar();
    assert!(!app.offers(Command::ServerConnect));
    assert!(app.offers(Command::ServerDisconnect));
    obelus_app::app::dispatch::dispatch(&mut app, Command::ServerDisconnect);
    assert_eq!(app.goes_to(), Some(&Going::Home));
}

/// The list a machine is remembered in holds its key, so nobody else on
/// this machine may read it -- and what was there before is kept with it.
///
/// Broken deliberately by writing the list the ordinary way (`std::fs::write`
/// for `obelus_private::write` in `servers::remember`): every key a window
/// had been let in with was readable by anybody here.
#[test]
fn the_keys_remembered_are_nobody_else_s() {
    let _turn = crate::turn();
    keep_servers("[[server]]\nat = \"old\"\nkey = \"aa\"\nlast = 0\n");
    assert!(obelus_app::app::servers::remember("devbox", "00ff", 1));
    let path = obelus_logging::state_directory()
        .expect("somewhere to keep them")
        .join("servers.toml");
    assert!(
        obelus_private::is_private(&path).expect("asked"),
        "anybody may read the keys"
    );
    assert_eq!(
        obelus_app::app::servers::key_for("old").as_deref(),
        Some("aa")
    );
    assert_eq!(
        obelus_app::app::servers::key_for("devbox").as_deref(),
        Some("00ff")
    );
}
