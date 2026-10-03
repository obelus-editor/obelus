//! The remote page: which chat this machine can be reached from, and what is
//! kept for it.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;

/// The secrets are kept in one directory for the whole process, and the
/// settings are applied to process-wide state, so these take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Where this process keeps its secrets, emptied for the test about to use
/// it -- never the reader's keyring, which a test has no business writing.
fn secrets_of_its_own() {
    let directory = std::env::temp_dir().join(format!("obelus-secrets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    obelus_remote::secrets::secrets_for_test(directory);
}

/// The settings, open on the remote tab, with a settings file of the test's
/// own and a channel the keyring's answers come back on.
fn on_the_remote_page(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    secrets_of_its_own();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 66, 20);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Tab);
    support::press(&mut app, KeyCode::Tab);
    assert!(
        app.settings().expect("the settings").on_remote(),
        "not on the remote page"
    );
    (app, events)
}

/// Handles what comes back until `done` says so, or gives up.
fn until(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<Event>,
    what: &str,
    done: impl Fn(&mut App) -> bool,
) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let _ = support::render(app, 66, 20);
        if done(app) {
            return;
        }
        let left = until
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_else(|| panic!("gave up waiting for {what}"));
        if let Ok(event) = events.recv_timeout(left) {
            app.handle(event);
        }
    }
}

/// Chooses Slack, from the list the platform row opens.
fn choose_slack(app: &mut App) {
    support::press(app, KeyCode::Enter);
    assert!(app.picker().is_some(), "the platforms did not open");
    support::type_text(app, "Slack");
    support::press(app, KeyCode::Enter);
    assert_eq!(
        app.config().remote.as_deref(),
        Some("slack"),
        "Slack was not chosen"
    );
}

/// Puts the focus on the row with this name.
fn to_the_row(app: &mut App, name: &str) {
    for _ in 0..10 {
        let settings = app.settings().expect("the settings");
        let at = settings
            .rows(None)
            .get(settings.focus())
            .map(|row| row.label().to_string());
        if at.as_deref() == Some(name) {
            return;
        }
        support::press(app, KeyCode::Down);
    }
    panic!("no row called {name}");
}

/// With no chat set, the page is one row and says it is off.
///
/// Broken deliberately by listing the fields whatever is set: the page grew
/// rows nobody could use, and the fixture changed under it.
#[test]
fn with_no_chat_the_page_is_one_row() {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("remote-off");
    let (mut app, _events) = on_the_remote_page(&scratch);
    support::check("remote_off_66x20", &support::render(&mut app, 66, 20));
}

/// Choosing a chat writes it to the reader's file and puts what it has to
/// be told on the page, saying it is not set up until it has been.
///
/// Broken deliberately by building the page's account from the default
/// config rather than the one in force: the rows stayed one and the state
/// stayed `Off`.
#[test]
fn choosing_a_chat_shows_what_it_has_to_be_told() {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("remote-chosen");
    let (mut app, events) = on_the_remote_page(&scratch);
    choose_slack(&mut app);
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(written.contains("remote = \"slack\""), "{written}");

    until(&mut app, &events, "the keyring to be asked", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_remote::State::Unready)
    });
    support::check("remote_unset_66x20", &support::render(&mut app, 66, 20));
}

/// A token goes to the keyring, is drawn by its ends, and is forgotten with
/// `delete`; one pasted into the wrong row is refused where it was typed.
///
/// Broken deliberately four ways. Writing the token whole into the row's
/// account, once where it is kept and once where it is read back: the row
/// showed all of it. Taking the prefix check out: the bot
/// token was kept as the app token. And leaving `delete` unanswered: the
/// row still showed the token afterwards.
#[test]
fn a_token_is_kept_drawn_by_its_ends_and_forgotten() {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("remote-token");
    let (mut app, events) = on_the_remote_page(&scratch);
    choose_slack(&mut app);
    to_the_row(&mut app, "App token");

    support::press(&mut app, KeyCode::Enter);
    assert!(app.prompt().is_some(), "nothing asked for the token");
    support::type_text(&mut app, "xoxb-2-the-wrong-one");
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.prompt().is_some(),
        "a bot token was taken as the app token"
    );
    assert_eq!(app.note(), Some("App token starts with xapp-"));
    assert_eq!(
        obelus_remote::secrets::read("slack", "app_token"),
        Ok(None),
        "the wrong token was kept"
    );

    // The row it is typed into hides the middle of it, as it is typed.
    let prompt = app.prompt().expect("still asking");
    assert_eq!(prompt.line(), "App token: xoxb-•••••••••••-one");

    for _ in 0..40 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "xapp-1-A0B1C2D3E4-9f2a");
    assert_eq!(
        app.prompt().expect("still asking").line(),
        "App token: xapp-•••••••••••••9f2a"
    );
    support::press(&mut app, KeyCode::Enter);
    until(&mut app, &events, "the token to be kept", |app| {
        app.settings().is_some_and(|settings| {
            settings.reached().kept.get("app_token").map(String::as_str) == Some("xapp-…9f2a")
        })
    });
    assert_eq!(
        obelus_remote::secrets::read("slack", "app_token"),
        Ok(Some("xapp-1-A0B1C2D3E4-9f2a".to_string()))
    );
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(
        !written.contains("A0B1C2D3E4"),
        "a token went into the settings file:\n{written}"
    );

    // And another window, which has to ask the keyring rather than remember
    // what it was told, draws it by its ends as well.
    let mut other = App::new(vec![support::open_fixture("sample.rs")]);
    other.config_file_for_test(scratch.join("config.toml"));
    let heard = support::drive(&mut other);
    support::lay_out(&mut other, 66, 20);
    dispatch::dispatch(&mut other, Command::ConfigOpen);
    support::press(&mut other, KeyCode::Tab);
    support::press(&mut other, KeyCode::Tab);
    until(&mut other, &heard, "the keyring to be read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().kept.contains_key("app_token"))
    });
    assert_eq!(
        other
            .settings()
            .expect("the settings")
            .reached()
            .kept
            .get("app_token")
            .map(String::as_str),
        Some("xapp-…9f2a"),
        "a token read back from the keyring is drawn whole"
    );

    support::press(&mut app, KeyCode::Delete);
    until(&mut app, &events, "the token to be forgotten", |app| {
        app.settings()
            .is_some_and(|settings| !settings.reached().kept.contains_key("app_token"))
    });
    assert_eq!(obelus_remote::secrets::read("slack", "app_token"), Ok(None));
}

/// The manifest row copies the manifest, and says so.
///
/// Broken deliberately by copying the platform's name instead: the note
/// still said it was copied, and the clipboard held no manifest.
#[test]
fn the_manifest_is_copied() {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _clipboard = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let scratch = support::Scratch::new("remote-manifest");
    let (mut app, _events) = on_the_remote_page(&scratch);
    choose_slack(&mut app);
    to_the_row(&mut app, "Manifest");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.note(), Some("Copied the manifest"));
    let copied = obelus_clipboard::paste().expect("something copied");
    assert!(copied.contains("\"socket_mode_enabled\": true"), "{copied}");
}

/// Pairing waits for something to send the code to.
///
/// Broken deliberately by offering it whatever the state: the foot offered
/// `Pair` on a chat nothing is connected to.
#[test]
fn pairing_waits_for_a_connection() {
    let _turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("remote-pair");
    let (mut app, _events) = on_the_remote_page(&scratch);
    choose_slack(&mut app);
    to_the_row(&mut app, "Pair");
    let dump = support::render(&mut app, 66, 20);
    assert!(
        !dump.contains("Enter  Pair"),
        "pairing is offered with nothing to pair with:\n{dump}"
    );
}
