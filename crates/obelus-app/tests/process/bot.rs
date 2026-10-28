//! The chat page: which chat this machine can be reached from, and what is
//! kept for it.
//!
//! Not in the application's binary because the secrets are kept in one
//! directory for the whole process and the platform a test connects to is faked
//! for all of it, and because its tests apply settings, which `settings` says
//! the cost of.

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;

use crate::support;

/// A test's turn, with the secrets emptied for it: the binary's turn, because
/// the secrets are kept in one directory for the whole process and the
/// settings are applied to process-wide state.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    let turn = crate::turn();
    secrets_of_its_own();
    // Before the state directory is asked anything: a test comes to its
    // turn before it makes a scratch, and until something has said where
    // the state goes it is the reader's -- whose room and threads this
    // emptied, so that a window paired in a group stopped hearing it.
    support::state_of_its_own();
    // And no threads from a test before: the table of which conversation
    // is which thread is the process's, and a fake agent names its
    // sessions from `s-1` in every test, so a thread one test opened is a
    // thread the next one's conversation would be found to have already.
    if let Some(state) = obelus_logging::state_directory() {
        let _ = std::fs::remove_dir_all(state.join("remote"));
    }
    turn
}

/// Where this process keeps its secrets, emptied for the test about to use
/// it -- never the reader's own secrets, which a test has no business writing.
fn secrets_of_its_own() {
    let directory = the_secrets();
    let _ = std::fs::remove_dir_all(&directory);
    obelus_bot::secrets::secrets_for_test(directory);
}

/// The directory this process keeps its secrets in.
fn the_secrets() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("obelus-secrets-{}", std::process::id()))
}

/// The settings, open on the remote tab, with a settings file of the test's
/// own and a channel the secrets' answers come back on.
fn on_the_bot_page(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 66, 20);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Tab);
    support::press(&mut app, KeyCode::Tab);
    assert!(
        app.settings().expect("the settings").on_bot(),
        "not on the chat page"
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
        app.config().chat.as_deref(),
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
    let _turn = turn();
    let scratch = support::Scratch::new("bot-off");
    let (mut app, _events) = on_the_bot_page(&scratch);
    support::check("bot_off_66x20", &support::render(&mut app, 66, 20));
}

/// Choosing a chat writes it to the reader's file and puts what it has to
/// be told on the page, saying it is not set up until it has been.
///
/// Broken deliberately by building the page's account from the default
/// config rather than the one in force: the rows stayed one and the state
/// stayed `Off`.
#[test]
fn choosing_a_chat_shows_what_it_has_to_be_told() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-chosen");
    let (mut app, events) = on_the_bot_page(&scratch);
    choose_slack(&mut app);
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(written.contains("chat = \"slack\""), "{written}");

    until(&mut app, &events, "the secrets to be read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_bot::State::Unready)
    });
    support::check("bot_unset_66x20", &support::render(&mut app, 66, 20));
}

/// A file of secrets that will not read is said on the rows of the tokens,
/// not taken for no tokens at all.
///
/// Broken deliberately by letting `bot_state` go on to the rows past a
/// read that failed: the state stayed `Unready` and the test gave up
/// waiting.
#[test]
fn secrets_that_will_not_read_are_said_on_their_rows() {
    let _turn = turn();
    let directory = the_secrets();
    std::fs::create_dir_all(&directory).expect("the directory");
    std::fs::write(directory.join("secrets.toml"), "this is = = not toml").expect("writing");
    let scratch = support::Scratch::new("bot-unread");
    let (mut app, events) = on_the_bot_page(&scratch);
    choose_slack(&mut app);
    until(&mut app, &events, "the secrets to fail to read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_bot::State::Unread)
    });
    to_the_row(&mut app, "App token");
    let screen = support::render(&mut app, 66, 20);
    assert!(
        screen.contains("The tokens would not read"),
        "the row says nothing about it:\n{screen}"
    );
}

/// A token goes to its file, is drawn by its ends, and is forgotten with
/// `delete`; one pasted into the wrong row is refused where it was typed.
///
/// Broken deliberately four ways. Writing the token whole into the row's
/// account, once where it is kept and once where it is read back: the row
/// showed all of it. Taking the prefix check out: the bot
/// token was kept as the app token. And leaving `delete` unanswered: the
/// row still showed the token afterwards.
#[test]
fn a_token_is_kept_drawn_by_its_ends_and_forgotten() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-token");
    let (mut app, events) = on_the_bot_page(&scratch);
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
        obelus_bot::secrets::read("slack", "app_token"),
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
        obelus_bot::secrets::read("slack", "app_token"),
        Ok(Some("xapp-1-A0B1C2D3E4-9f2a".to_string()))
    );
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(
        !written.contains("A0B1C2D3E4"),
        "a token went into the settings file:\n{written}"
    );

    // And another window, which has to read the file rather than remember
    // what it was told, draws it by its ends as well.
    let mut other = App::new(vec![support::open_fixture("sample.rs")]);
    other.config_file_for_test(scratch.join("config.toml"));
    let heard = support::drive(&mut other);
    support::lay_out(&mut other, 66, 20);
    dispatch::dispatch(&mut other, Command::ConfigOpen);
    support::press(&mut other, KeyCode::Tab);
    support::press(&mut other, KeyCode::Tab);
    until(&mut other, &heard, "the secrets to be read", |app| {
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
        "a token read back from its file is drawn whole"
    );

    support::press(&mut app, KeyCode::Delete);
    until(&mut app, &events, "the token to be forgotten", |app| {
        app.settings()
            .is_some_and(|settings| !settings.reached().kept.contains_key("app_token"))
    });
    assert_eq!(obelus_bot::secrets::read("slack", "app_token"), Ok(None));
}

/// The manifest row copies the manifest, and says so.
///
/// Broken deliberately by copying the platform's name instead: the note
/// still said it was copied, and the clipboard held no manifest.
#[test]
fn the_manifest_is_copied() {
    let _turn = turn();
    let _clipboard = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let scratch = support::Scratch::new("bot-manifest");
    let (mut app, _events) = on_the_bot_page(&scratch);
    choose_slack(&mut app);
    to_the_row(&mut app, "Manifest");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.note(), Some("Copied the manifest"));
    let copied = obelus_clipboard::paste().expect("something copied");
    assert!(copied.contains("\"socket_mode_enabled\": true"), "{copied}");
}

/// Pairing waits for a chat that could be reached: one told everything
/// it has to be.
///
/// Broken deliberately by offering it whatever the state: the foot offered
/// `Pair` on a chat with no tokens.
#[test]
fn pairing_waits_for_a_chat_that_could_be_reached() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-pair");
    let (mut app, events) = on_the_bot_page(&scratch);
    choose_slack(&mut app);
    until(
        &mut app,
        &events,
        "the secrets to say there are no tokens",
        |app| app.bot_state_for_test() == obelus_bot::State::Unready,
    );
    to_the_row(&mut app, "Pair");
    let dump = support::render(&mut app, 66, 20);
    assert!(
        !dump.contains("Enter  Pair"),
        "pairing is offered with nothing to pair with:\n{dump}"
    );
}

/// Pairing in a window the chat is not in yet takes the chat, and the code
/// comes once it has connected -- one press, not one to find
/// `connect-to-chat` and another to press again.
///
/// Broken deliberately twice. Offering it only once connected: the foot
/// did not offer `Pair`. And taking the chat without keeping what was
/// asked for: it connected, and no code came.
#[test]
fn pairing_takes_the_chat_and_then_makes_the_code() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-pair-takes");
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let (mut app, events) = on_the_bot_page(&scratch);
    until(&mut app, &events, "the tokens to be read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state.may_connect())
    });
    to_the_row(&mut app, "Pair");
    let usable = support::usable_here(&mut app);
    assert!(
        usable.contains(&"Pair".to_string()),
        "pairing is not offered on a chat that could be reached: {usable:?}"
    );
    support::press(&mut app, KeyCode::Enter);
    until(&mut app, &events, "a code on the row", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().pairing.is_some())
    });
    assert!(app.holds_the_bot_for_test(), "the chat is not here");
}

/// Pairing in a window while another has the chat takes it from that one,
/// and the code comes once it is here -- the window hears its own asking
/// for the chat, and that is not somebody asking for it.
///
/// Broken deliberately by letting go of the code asked for whenever an
/// asking is heard: the chat moved, and no code came.
#[test]
fn pairing_takes_the_chat_from_another_window() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-pair-from-another");
    set_up_for_two(&scratch);
    let watched = |scratch: &support::Scratch| {
        let mut app = App::new(Vec::new());
        app.config_file_for_test(scratch.join("config.toml"));
        let (sender, events) = obelus_app::event::channel();
        app.start(sender);
        support::lay_out(&mut app, 76, 24);
        (app, events)
    };
    let (mut first, firsts) = watched(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });

    let (mut second, seconds) = watched(&scratch);
    dispatch::dispatch(&mut second, Command::ConfigOpen);
    support::press(&mut second, KeyCode::Tab);
    support::press(&mut second, KeyCode::Tab);
    until(&mut second, &seconds, "the tokens to be read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state.may_connect())
    });
    to_the_row(&mut second, "Pair");
    support::press(&mut second, KeyCode::Enter);
    // The second hears its own asking before the first lets go, which is
    // the order the bug needed and the one a race may or may not give.
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(20)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the chat to move and a code to come",
        |first, second| {
            !first.holds_the_bot_for_test()
                && second
                    .settings()
                    .is_some_and(|settings| settings.reached().pairing.is_some())
        },
    );
}

/// What the fake platform was handed: where it reports what it hears, and
/// what Obelus asked it to say.
struct Faked {
    told: obelus_bot::platform::Told,
    sink: std::sync::Arc<dyn obelus_sink::Sink<obelus_bot::Event>>,
    said: tokio::sync::mpsc::UnboundedReceiver<obelus_bot::model::Out>,
}

static FAKED: std::sync::Mutex<Option<Faked>> = std::sync::Mutex::new(None);

/// A platform that connects at once and keeps what it is asked to say.
fn fake_connect(
    told: obelus_bot::platform::Told,
    sink: std::sync::Arc<dyn obelus_sink::Sink<obelus_bot::Event>>,
) -> tokio::sync::mpsc::UnboundedSender<obelus_bot::model::Out> {
    let (out, said) = tokio::sync::mpsc::unbounded_channel();
    // Kept before it says it is connected, never after: a test waits for
    // the connection and then speaks for the platform, and this runs on a
    // thread of its own -- said first, the test could get there before
    // the sink did, which Windows CI did. Broken deliberately by sleeping
    // between the two in the old order: every test that speaks for the
    // platform panicked on `connected`.
    let connected = std::sync::Arc::clone(&sink);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Faked { told, sink, said });
    let _ = connected.send(obelus_bot::Event::connection(
        obelus_bot::State::Connected,
        None,
    ));
    out
}

/// What the fake platform has been asked to say since the last look.
fn said_since() -> Vec<obelus_bot::model::Out> {
    let mut faked = FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut said = Vec::new();
    if let Some(faked) = faked.as_mut() {
        while let Ok(out) = faked.said.try_recv() {
            said.push(out);
        }
    }
    said
}

/// Somebody starting a thread in a group the bot is in, the thread named
/// after what they said.
fn somebody_says(from: &str, room: &str, text: &str) {
    let faked = FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let faked = faked.as_ref().expect("connected");
    let _ = faked.sink.send(obelus_bot::Event::Heard {
        from: from.to_string(),
        room: room.to_string(),
        at: obelus_bot::model::Where::Fresh(format!("M-{text}")),
        text: text.to_string(),
    });
}

/// The room, as if somebody had paired in it: the group `C1`.
fn a_room_kept() {
    a_room_kept_on("slack");
}

/// The same, on another platform.
fn a_room_kept_on(platform: &str) {
    let state = obelus_logging::state_directory().expect("a state directory");
    let at = state.join("remote").join(platform);
    std::fs::create_dir_all(&at).expect("the directory");
    std::fs::write(at.join("room.toml"), "room = \"C1\"\n").expect("the room");
}

/// A window set to Slack with both tokens kept, connected to the fake.
fn connected(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let (mut app, events) = on_the_bot_page(scratch);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_bot::State::Connected)
    });
    (app, events)
}

/// A window set to a chat connects to it only once it is told to, with
/// what it was told -- the secrets out of their file -- and says so on its
/// status row and nowhere else: not before, and not on the page, which
/// holds what the reader set and not where it stands.
///
/// Broken deliberately four ways. Connecting whenever a chat is set: the
/// fake platform was handed the tokens before anybody asked. Leaving the
/// secrets out of what the platform is handed: it was given nothing, and
/// the state stayed `Not set up`. Handing the status row no chat: the row
/// said nothing about it. And drawing the state on the page again: it said
/// `Connected` at the top.
#[test]
fn a_chat_that_is_set_is_connected_to_when_told() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-connects");
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let (mut app, events) = on_the_bot_page(&scratch);
    let status = |app: &mut App| {
        let dump = support::render(app, 66, 20);
        support::text_block(&dump)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string()
    };
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 66, 20);
    }
    assert!(
        FAKED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none(),
        "connected before anybody asked"
    );
    support::press(&mut app, KeyCode::Esc);
    assert!(
        !status(&mut app).contains("Slack"),
        "the status row spoke of a chat this window does not talk to"
    );

    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let faked = FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let told = &faked.as_ref().expect("connected").told;
    assert_eq!(
        told.get("app_token").map(String::as_str),
        Some("xapp-1-app")
    );
    assert_eq!(
        told.get("bot_token").map(String::as_str),
        Some("xoxb-1-bot")
    );
    drop(faked);
    let line = status(&mut app);
    assert!(
        line.contains("● Slack"),
        "the status row does not say so: {line}"
    );

    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Tab);
    support::press(&mut app, KeyCode::Tab);
    let dump = support::render(&mut app, 66, 20);
    assert!(
        !dump.contains("Connected"),
        "the page says where it stands:\n{dump}"
    );
}

/// Pairing: a code on the row, the code sent in a group, the person on the
/// list by the name the platform gives them, a word back to them in the
/// thread the code began, and that group kept as the room. A wrong code
/// lets nobody in.
///
/// Broken deliberately five times. Leaving where to begin out of the word
/// back: they were told they were paired and nothing about what next.
/// Comparing the code as it was typed: the code sent split in two let
/// nobody in. Taking any code: the stranger's wrong one was taken, and
/// they were asked their name. Not keeping the group: there was no room
/// afterwards. And drawing the code from letters again: it was not six
/// digits.
#[test]
fn pairing_lets_in_whoever_sends_the_code() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-pairs");
    let (mut app, events) = connected(&scratch);
    to_the_row(&mut app, "Pair");
    support::press(&mut app, KeyCode::Enter);
    let code = app
        .settings()
        .and_then(|settings| settings.reached().pairing.clone())
        .expect("a code on the row");
    let dump = support::render(&mut app, 66, 20);
    assert!(dump.contains(&code), "the code is not on the row:\n{dump}");
    assert!(
        code.len() == 6 && code.chars().all(|digit| digit.is_ascii_digit()),
        "the code is not six digits: {code}"
    );

    // One digit off, so never the code by chance.
    let wrong = code
        .char_indices()
        .map(|(at, digit)| match (at, digit) {
            (0, '9') => '0',
            (0, digit) => char::from(digit as u8 + 1),
            (_, digit) => digit,
        })
        .collect::<String>();
    somebody_says("U0STRANGER", "C7", "hello?");
    somebody_says("U0STRANGER", "C7", &wrong);
    let _ = support::render(&mut app, 66, 20);
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(200)) {
        app.handle(event);
    }
    assert!(said_since().is_empty(), "a stranger was answered");

    let sent = format!("  {} {}  ", &code[..3], &code[3..]);
    somebody_says("U04ABCDEF", "C7", &sent);
    until(
        &mut app,
        &events,
        "the platform to be asked their name",
        |_| {
            FAKED
                .lock()
                .ok()
                .and_then(|faked| faked.as_ref().map(|faked| !faked.said.is_empty()))
                .unwrap_or(false)
        },
    );
    assert_eq!(
        said_since(),
        [obelus_bot::model::Out::Name {
            id: "U04ABCDEF".to_string()
        }]
    );
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_bot::Event::Named {
        id: "U04ABCDEF".to_string(),
        name: "Sunli".to_string(),
    });
    until(&mut app, &events, "them to be let in", |app| {
        app.config()
            .chat_of("slack")
            .is_some_and(|remote| remote.people.iter().any(|person| person.name == "Sunli"))
    });
    assert_eq!(app.note(), Some("Paired Sunli"));
    let said = said_since();
    let code_thread = format!("M-{sent}");
    assert!(
        matches!(
            said.as_slice(),
            [obelus_bot::model::Out::Say { room, thread, to, text, .. }]
                if room == "C7" && *thread == code_thread && to == "U04ABCDEF"
                    && text.contains(obelus_bot::slack::DESCRIPTION.begin)
        ),
        "they were not told, or not told what there is to say: {said:#?}"
    );
    let room = obelus_logging::state_directory()
        .and_then(|state| std::fs::read_to_string(state.join("remote/slack/room.toml")).ok())
        .unwrap_or_default();
    assert!(room.contains("\"C7\""), "the group was not kept: {room}");
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(written.contains("U04ABCDEF"), "{written}");
    assert!(
        app.settings()
            .and_then(|settings| settings.reached().pairing.clone())
            .is_none(),
        "the code outlived its use"
    );
}

/// Everything said to the fake platform from now until `done` holds,
/// handling the window's events meanwhile.
fn said_until(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<Event>,
    what: &str,
    done: impl Fn(&[obelus_bot::model::Out]) -> bool,
) -> Vec<obelus_bot::model::Out> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut said = Vec::new();
    loop {
        support::lay_out(app, 76, 24);
        said.extend(said_since());
        if done(&said) {
            return said;
        }
        let left = until
            .checked_duration_since(std::time::Instant::now())
            .unwrap_or_else(|| panic!("gave up waiting for {what}; said: {said:#?}"));
        if let Ok(event) = events.recv_timeout(left.min(std::time::Duration::from_millis(50))) {
            app.handle(event);
        }
    }
}

/// Whether something was said in this thread with these words in it --
/// what became of a question among them, which is said there where its card
/// never went up, and the fake puts up no cards.
fn in_thread(said: &[obelus_bot::model::Out], thread: &str, words: &str) -> bool {
    said.iter().any(|out| {
        matches!(
            out.clone().in_words(),
            obelus_bot::model::Out::Say { thread: at, text, .. }
                if at == thread && text.contains(words)
        )
    })
}

/// The number of the question put to this thread offering this answer, if
/// one has been.
fn asked_with(said: &[obelus_bot::model::Out], thread: &str, answer: &str) -> Option<u64> {
    said.iter().find_map(|out| match out {
        obelus_bot::model::Out::Ask {
            thread: at,
            asked,
            question,
            ..
        } if at == thread && question.choices.iter().any(|(_, name)| name == answer) => {
            Some(*asked)
        }
        _ => None,
    })
}

/// Every question put to this thread, in order: its number and what it is.
fn asks_in(
    said: &[obelus_bot::model::Out],
    thread: &str,
) -> Vec<(u64, obelus_bot::model::Question)> {
    said.iter()
        .filter_map(|out| match out {
            obelus_bot::model::Out::Ask {
                thread: at,
                asked,
                question,
                ..
            } if at == thread => Some((*asked, question.clone())),
            _ => None,
        })
        .collect()
}

/// The reader on the list pressing one answer on a question's card.
fn pressed(asked: u64, chosen: &str) -> obelus_bot::Event {
    obelus_bot::Event::Answered {
        from: "U1".to_string(),
        asked,
        chosen: vec![chosen.to_string()],
        words: None,
    }
}

/// A conversation and its thread say the same things: a thread opened for
/// it, the reader's words from here marked as from here, the agent's words
/// when its turn is over, its question as a card -- answered by a press on
/// it -- and words from the chat arriving with a line for the agent saying
/// where they came from.
///
/// Broken deliberately five ways, each failing at its own step. Not opening
/// threads: no `Open` came. Not echoing what was typed here. Posting the
/// turn as it streamed rather than when it ended: the reply was split. Not
/// taking a press as the answer: the permission was never given and the
/// turn never ended. And dropping the line in front of
/// words from afar: the agent's log had no "sent from Slack". The thread's
/// head twice: never said again, it stayed as it opened; said again with
/// the state it had, it never said `Waiting` or `Done`. And what it thought
/// three ways: not mirrored, no thought in the thread; run into the words
/// after it, the words did not stand on their own; sent calling the reader,
/// the thought was not quiet.
#[test]
fn a_conversation_and_its_thread_say_the_same_things() {
    let _turn = turn();
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let scratch = support::Scratch::new("bot-mirror");
    std::fs::write(
        scratch.join("config.toml"),
        "chat = \"slack\"\n[chats.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");

    let log = scratch.join("asked.log");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
            "prompts".to_string(),
        ],
    );
    app.new_conversation();
    app.open_a_session_for_test();

    // A thread for the conversation, in the reader's direct message.
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, room, .. } if room == "C1" => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for in the room");
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_bot::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    for _ in 0..5 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }

    // What the reader types here goes there, as they typed it.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        asked_with(said, "T1", "Allow once").is_some()
    });
    let asked = asked_with(&said, "T1", "Allow once").expect("asked");
    assert!(in_thread(&said, "T1", "what is this file"), "{said:#?}");
    // And what the agent said before it asked, whole, ahead of the
    // question.
    assert!(in_thread(&said, "T1", "it is a rust file"), "{said:#?}");
    // And what it thought before it said it, quietly and on its own.
    let words: Vec<(&str, bool)> = said
        .iter()
        .filter_map(|out| match out {
            obelus_bot::model::Out::Say {
                thread,
                text,
                notify,
                ..
            } if thread == "T1" && text != "what is this file" => Some((text.as_str(), *notify)),
            _ => None,
        })
        .collect();
    assert_eq!(
        words.get(..2),
        Some(
            &[
                ("\u{1f4ad} working it out", false),
                ("it is a rust file", false)
            ][..]
        ),
        "not the thought and then the words, quietly: {said:#?}"
    );
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Ask { to, .. } if to == "U1"
        )),
        "the question did not call the reader: {said:#?}"
    );
    // And the head says it is waiting on them.
    let state = |said: &[obelus_bot::model::Out]| {
        said.iter().rev().find_map(|out| match out {
            obelus_bot::model::Out::Retitle { head, .. } => head.state,
            _ => None,
        })
    };
    assert_eq!(
        state(&said),
        Some(obelus_bot::model::Turning::Waiting),
        "the head does not say it is waiting: {said:#?}"
    );

    // A press on its card answers it, and the rest of the turn follows.
    let _ = sink.send(pressed(asked, "once"));
    let said = said_until(
        &mut app,
        &events,
        "the end of the turn in the thread",
        |said| in_thread(said, "T1", "and I was allowed"),
    );
    assert_eq!(
        state(&said),
        Some(obelus_bot::model::Turning::Done),
        "the head does not say the turn is over: {said:#?}"
    );
    assert!(
        said.iter()
            .filter(|out| matches!(out, obelus_bot::model::Out::Say { .. }))
            .count()
            == 1,
        "the turn went out in pieces: {said:#?}"
    );

    // Words from the chat go to the agent with a line saying where from.
    let _ = sink.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("T1".to_string()),
        text: "and now?".to_string(),
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if logged.contains("and now?") {
            assert!(
                logged.contains("sent from Slack"),
                "the agent was not told where the words came from:\n{logged}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the words never reached the agent:\n{logged}"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
}

/// The name an agent gives its conversation is the name its thread goes
/// by: the head is said again with it.
///
/// Broken deliberately by leaving the head alone when the agent names the
/// conversation: the thread kept the name it opened with until the turn
/// ended. And by not saying the head when the thread arrives: the turn had
/// started before it did, and the thread never said so.
#[test]
fn the_name_an_agent_gives_is_the_threads() {
    let _turn = turn();
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let scratch = support::Scratch::new("bot-titled");
    std::fs::write(
        scratch.join("config.toml"),
        "chat = \"slack\"\n[chats.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_bot::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    support::type_text(&mut app, "/titled");
    support::press(&mut app, KeyCode::Enter);
    let said = said_until(&mut app, &events, "the turn to end", |said| {
        said.iter().any(|out| {
            matches!(
                out,
                obelus_bot::model::Out::Retitle { head, .. }
                    if head.state == Some(obelus_bot::model::Turning::Done)
            )
        })
    });
    // While the turn was still going: the end of a turn says the head again
    // anyway, and a rename that waited for it would be a rename that a long
    // turn kept from the reader for as long as it ran.
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Retitle { thread, head, .. }
                if thread == "T1"
                    && head.title == "Renamed by the agent"
                    && head.state == Some(obelus_bot::model::Turning::Working)
        )),
        "the thread was not renamed when the agent named it: {said:#?}"
    );
    // And the turn's start, which happened before the platform had answered
    // with the thread, said on it once it had.
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Retitle { head, .. }
                if head.title == "/titled"
                    && head.state == Some(obelus_bot::model::Turning::Working)
        )),
        "the head moved before the thread was there, and the thread never heard: {said:#?}"
    );
}

/// Feishu's page is made from what Feishu declares: an id written in the
/// settings file, a secret in its file, and a domain that starts on
/// Feishu and goes to Lark with a press -- none of which the page knows the
/// first thing about.
///
/// Broken deliberately twice. Drawing a choice nobody made as unset: the
/// domain said `Not set`. And writing a field that is not secret to the
/// secrets: the id was not in the settings file.
#[test]
fn feishu_is_set_up_from_what_it_declares() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-feishu");
    let (mut app, events) = on_the_bot_page(&scratch);
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "Feishu");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().chat.as_deref(), Some("feishu"));
    until(&mut app, &events, "the secrets to be read", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_bot::State::Unready)
    });
    assert_eq!(
        app.settings()
            .and_then(|settings| settings.reached().kept.get("domain").cloned())
            .as_deref(),
        Some("feishu"),
        "a choice nobody made is not its first word"
    );
    support::check("bot_feishu_66x24", &support::render(&mut app, 66, 24));

    to_the_row(&mut app, "App ID");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "cli_a1b2c3");
    support::press(&mut app, KeyCode::Enter);
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(written.contains("app_id = \"cli_a1b2c3\""), "{written}");

    to_the_row(&mut app, "Domain");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().chat_value("feishu", "domain"), Some("lark"));
}

/// A window set to Slack, with the reader on its list and the fake agent
/// to talk to, connected to the fake platform.
fn paired_with_an_agent(
    scratch: &support::Scratch,
) -> (App, std::sync::mpsc::Receiver<Event>, std::path::PathBuf) {
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    paired_with_an_agent_on(scratch, "slack", "")
}

/// The same on Feishu, which draws a question as a card -- the fake is
/// still the fake, and hands back what it is asked to say as it was asked.
fn paired_on_feishu_with_an_agent(
    scratch: &support::Scratch,
) -> (App, std::sync::mpsc::Receiver<Event>, std::path::PathBuf) {
    obelus_bot::secrets::write("feishu", "app_secret", "secret").expect("kept");
    paired_with_an_agent_on(scratch, "feishu", "app_id = \"cli_1\"\n")
}

/// A window set to a platform, told `told`, with the reader on its list and
/// the fake agent to talk to, connected to the fake platform.
fn paired_with_an_agent_on(
    scratch: &support::Scratch,
    platform: &str,
    told: &str,
) -> (App, std::sync::mpsc::Receiver<Event>, std::path::PathBuf) {
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(
        scratch.join("config.toml"),
        format!(
            "chat = \"{platform}\"\n[chats.{platform}]\n{told}people = [{{ id = \"U1\", name = \"Sunli\" }}]\n"
        ),
    )
    .expect("the settings");
    a_room_kept_on(platform);
    let log = scratch.join("asked.log");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
            "prompts".to_string(),
        ],
    );
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    (app, events, log)
}

/// The platform's sink, for saying something from its side.
fn the_platform() -> std::sync::Arc<dyn obelus_sink::Sink<obelus_bot::Event>> {
    FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected")
}

/// A thread the reader starts in the room is a conversation: their words
/// go to an agent as its first prompt, with the line saying where they came
/// from, and what the agent says goes back to that thread -- with no other
/// thread opened for it, and no head said over the reader's own words.
///
/// Broken deliberately four ways. Not hearing a fresh thread: the words
/// never reached the agent. Leaving out the line in front of them: the
/// agent's log had no "sent from Slack". Asking for a thread like any
/// other: an `Open` came. And saying the head on it: a `Retitle` named the
/// reader's own message.
#[test]
fn a_thread_the_reader_starts_is_a_conversation() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-fresh");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        logged.contains("what is in here") && logged.contains("sent from Slack"),
        "the agent was not given the words, or not told where from:\n{logged}"
    );
    assert!(
        !said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Open { .. } | obelus_bot::model::Out::Retitle { .. }
        )),
        "a thread was asked for, or a head said, over the reader's own: {said:#?}"
    );
}

/// What the agent says before it goes to call something goes to the thread
/// then, quietly, and not when the turn is over -- which for a turn of
/// minutes is minutes of nothing in the thread.
///
/// Broken deliberately by keeping the words for the end of the turn: the
/// turn here never ends, and the thread never heard them. And by calling
/// the reader for them: the phone rang for something nobody was waiting on.
#[test]
fn what_is_said_before_a_call_goes_at_once() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-pausing");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/pausing".to_string(),
    });
    let said = said_until(&mut app, &events, "the words before the call", |said| {
        in_thread(said, "F1", "looking at it first")
    });
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Say { text, notify: false, .. } if text == "looking at it first"
        )),
        "the words went out calling the reader: {said:#?}"
    );
}

/// The agent's plan goes to the thread, quietly and after what it said
/// before it -- and again only when its steps change: a step ticked off is
/// the same list sent again, and is not said.
///
/// Broken deliberately three ways. Not mirroring the plan at all: no plan
/// came. Saying it every time it was sent: three plans came, the middle one
/// the first again with a step ticked. And saying it before the words held
/// for the turn: the plan came ahead of "thinking it over".
#[test]
fn a_plan_is_said_when_its_steps_change() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-plan");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/replanning".to_string(),
    });
    let said = said_until(&mut app, &events, "the end of the turn", |said| {
        in_thread(said, "F1", "done planning")
    });
    let words: Vec<(&str, bool)> = said
        .iter()
        .filter_map(|out| match out {
            obelus_bot::model::Out::Say {
                thread,
                text,
                notify,
                ..
            } if thread == "F1" => Some((text.as_str(), *notify)),
            _ => None,
        })
        .collect();
    let plans: Vec<&(&str, bool)> = words
        .iter()
        .filter(|(text, _)| text.starts_with('\u{1f4cb}'))
        .collect();
    assert_eq!(
        plans,
        [
            &(
                "\u{1f4cb}\n\u{25b8} read the counts tree\n\u{25e6} write the test",
                false
            ),
            &(
                "\u{1f4cb}\n\u{2713} read the counts tree\n\u{25b8} wire it to the search\n\u{25e6} write the test",
                false
            ),
        ],
        "not the two plans, quietly: {words:#?}"
    );
    let at = |what: &str| {
        words
            .iter()
            .position(|(text, _)| text.contains(what))
            .unwrap_or_else(|| panic!("nothing said with {what:?} in it: {words:#?}"))
    };
    assert!(
        at("thinking it over") < at("\u{1f4cb}"),
        "the plan went ahead of what was said before it: {words:#?}"
    );
}

/// Every plan said in this thread, in order, and whether it called the
/// reader.
fn plans_in<'a>(said: &'a [obelus_bot::model::Out], thread: &str) -> Vec<(&'a str, bool)> {
    said.iter()
        .filter_map(|out| match out {
            obelus_bot::model::Out::Say {
                thread: at,
                text,
                notify,
                ..
            } if at == thread && text.starts_with('\u{1f4cb}') => Some((text.as_str(), *notify)),
            _ => None,
        })
        .collect()
}

/// A plan cleared and made again is said again, and so is the plan of a new
/// turn that is the last turn's over again: what the thread is told is what
/// this turn means to do.
///
/// Broken deliberately two ways. Keeping the plan last said across the
/// turn: the second turn's plan was never said, three in all and not four.
/// And keeping it across an empty one: each turn said its plan once.
#[test]
fn a_plan_made_again_is_said_again() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-plan-again");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/sameplan".to_string(),
    });
    let mut said = said_until(&mut app, &events, "the first turn's end", |said| {
        in_thread(said, "F1", "looked again")
    });
    assert_eq!(
        plans_in(&said, "F1").len(),
        2,
        "a plan cleared and made again was not said again: {said:#?}"
    );
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "/sameplan".to_string(),
    });
    said.extend(said_until(
        &mut app,
        &events,
        "the second turn's end",
        |said| in_thread(said, "F1", "looked again"),
    ));
    assert_eq!(
        plans_in(&said, "F1").len(),
        4,
        "the second turn's plan was not said: {said:#?}"
    );
}

/// What an agent replays of a conversation taken up again is not said in
/// its thread: it was said there when it was said. Neither its plan nor its
/// words -- which waited for the end of the next turn, and went out with
/// that turn's as if they were new.
///
/// Broken deliberately two ways. Mirroring the replay's plan: the old plan
/// went to the thread, and the old words with it, ahead of it. And keeping
/// the replay's words for the turn: "where we were" went out with "ran x".
#[test]
fn a_conversation_taken_up_again_is_not_said_again() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-taken-up");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456T\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let id = obelus_todo::NoteId::read("0123456T").expect("a name");
    let which = obelus_agent::chats::ChatId::Note(id.clone());
    obelus_agent::acp::sessions::change(
        scratch.path(),
        0,
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &which,
                "fake",
                scratch.path(),
                obelus_agent::acp::sessions::Kept {
                    session: "s-old".to_string(),
                    title: None,
                    told: Some("a note".to_string()),
                    introduced: true,
                    last: None,
                },
            );
        },
    );
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(
        scratch.join("config.toml"),
        "chat = \"slack\"\n[chats.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "prompts".to_string(),
            "replans".to_string(),
        ],
    );
    dispatch::dispatch(&mut app, Command::TodoOpen);
    support::talk_about_what_is_here(&mut app);
    assert!(app.chat().is_some(), "the note's conversation did not open");
    app.open_a_session_for_test();
    let mut said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let _ = the_platform().send(obelus_bot::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    // `until` leaves what was said where `said_until` finds it.
    until(&mut app, &events, "the old conversation", |app| {
        app.talking() == obelus_agent::Talking::Ready
            && app.chat().is_some_and(|chat| {
                chat.rows(76)
                    .iter()
                    .any(|row| row.text().contains("where we were"))
            })
    });
    support::type_text(&mut app, "/x");
    support::press(&mut app, KeyCode::Enter);
    said.extend(said_until(&mut app, &events, "the turn's end", |said| {
        in_thread(said, "T1", "ran x")
    }));
    assert!(
        plans_in(&said, "T1").is_empty(),
        "the replay's plan was said: {said:#?}"
    );
    assert!(
        !in_thread(&said, "T1", "where we were"),
        "the replay's words were said: {said:#?}"
    );
}

/// Nothing outside the room is heard, even from somebody on the list: a
/// thread they start in another group the bot is in begins nothing and is
/// not answered.
///
/// Broken deliberately by hearing every group: the thread in the other
/// group began a conversation, and the agent was asked.
#[test]
fn words_outside_the_room_are_not_heard() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-outside");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    somebody_says("U1", "C2", "what is in here");
    for _ in 0..20 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !logged.contains("what is in here"),
        "words from another group reached the agent:\n{logged}"
    );
    assert!(
        said_since().is_empty(),
        "words from another group were answered"
    );
}

/// A question answered from the chat is answered in the conversation whose
/// thread it is, whichever is on the screen -- here none of them is, the way
/// a conversation the reader began from their phone never is.
///
/// Broken deliberately by taking the permission from the conversation on
/// screen: the card went from the thread's conversation, nothing reached
/// its agent, and the turn never ended.
#[test]
fn a_question_is_answered_in_its_own_conversation() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-answer-elsewhere");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
    let asked = asked_with(&said, "F1", "Allow once").expect("asked");
    let _ = platform.send(pressed(asked, "once"));
    said_until(&mut app, &events, "the rest of the turn", |said| {
        in_thread(said, "F1", "and I was allowed")
    });
}

/// The card is the answer: put to the thread as a question with the
/// agent's own ids on it, a press on it answering for the reader on the
/// list and nobody else, the card closed with what was chosen. On Feishu,
/// which is set up with fields of its own.
///
/// Broken deliberately three ways. Not hearing presses: the permission was
/// never given and the turn never ended. Taking a press from anybody: the
/// stranger's answered it. And not closing the card: no `Settle` came, and
/// it could be pressed again.
#[test]
fn a_question_is_a_card_and_the_card_is_the_answer() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-card");
    let (mut app, events, _log) = paired_on_feishu_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let asked_in = |said: &[obelus_bot::model::Out]| {
        said.iter().find_map(|out| match out {
            obelus_bot::model::Out::Ask {
                thread,
                asked,
                question,
                ..
            } if thread == "F1" => Some((*asked, question.clone())),
            _ => None,
        })
    };
    let said = said_until(&mut app, &events, "the question as a card", |said| {
        asked_in(said).is_some()
    });
    let (asked, question) = asked_in(&said).expect("asked");
    assert!(
        question
            .choices
            .contains(&("once".to_string(), "Allow once".to_string())),
        "the card does not carry the agent's own answers: {question:#?}"
    );

    let press = |from: &str| obelus_bot::Event::Answered {
        from: from.to_string(),
        asked,
        chosen: vec!["once".to_string()],
        words: None,
    };
    let _ = platform.send(press("U9STRANGER"));
    let settled = |said: &[obelus_bot::model::Out]| {
        said.iter().any(|out| {
            matches!(
                out,
                obelus_bot::model::Out::Settle { asked: closed, said, .. }
                    if *closed == asked && said.contains("Allow once")
            )
        })
    };
    let mut said = Vec::new();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
        said.extend(said_since());
    }
    assert!(
        !settled(&said) && !in_thread(&said, "F1", "and I was allowed"),
        "a stranger's press answered it: {said:#?}"
    );

    let _ = platform.send(press("U1"));
    let said = said_until(&mut app, &events, "the rest of the turn", |said| {
        in_thread(said, "F1", "and I was allowed")
    });
    assert!(settled(&said), "the card was not closed: {said:#?}");
}

/// Words in the thread while a question's card is up are cancel on the
/// card and then the words: the card closed as not answered, the agent told
/// no, and the words its next prompt -- never read as the answer, though
/// `1` is the number of an answer on it.
///
/// Broken deliberately twice. Pointing the words back at the card, as it
/// once did: they never reached the agent. And leaving the question to go
/// with the stopped turn instead of cancelling it: the card was closed as
/// no longer asked rather than as not answered.
#[test]
fn words_while_a_card_is_up_cancel_it_and_are_said() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-words-over-card");
    let (mut app, events, log) = paired_on_feishu_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question as a card", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
    let asked = asked_with(&said, "F1", "Allow once").expect("asked");

    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "1 -- no, read the README instead".to_string(),
    });
    let reached = || {
        std::fs::read_to_string(&log).is_ok_and(|logged| logged.contains("read the README instead"))
    };
    let said = said_until(&mut app, &events, "the words at the agent", |said| {
        reached() && in_thread(said, "F1", "and I was refused")
    });
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Settle { asked: closed, said, .. }
                if *closed == asked && said.contains("Not answered")
        )),
        "the card was not closed as not answered: {said:#?}"
    );
    assert!(
        !in_thread(&said, "F1", "and I was allowed") && !in_thread(&said, "F1", "card above"),
        "the words were read as the answer, or pointed at the card: {said:#?}"
    );
}

/// Words in the thread over a question with another waiting behind it give
/// up on both: the one behind goes with the stopped turn and is never put
/// to the thread.
///
/// Broken deliberately by putting up what was waiting, as a card does when
/// it is cancelled: a second card came to the thread and was taken back.
#[test]
fn words_over_a_question_take_the_ones_behind_it_too() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-words-over-two");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/pair".to_string(),
    });
    said_until(&mut app, &events, "the first question", |said| {
        !asks_in(said, "F1").is_empty()
    });
    // A turn that answers with nothing, so that no question of its own
    // comes to be counted.
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "neither, go slowly".to_string(),
    });
    let mut said = said_until(&mut app, &events, "the words at the agent", |_| {
        std::fs::read_to_string(&log).is_ok_and(|logged| logged.contains("neither, go slowly"))
    });
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
        said.extend(said_since());
    }
    assert_eq!(
        asks_in(&said, "F1").len(),
        0,
        "a question behind the one passed by was put to the thread: {said:#?}"
    );
}

/// Words in the thread while the agent works stop the turn and are said
/// once it has stopped -- `ctrl+enter` here -- rather than waiting for a
/// turn that may never end on its own.
///
/// Broken deliberately by queueing them for the turn's end, as it once did:
/// the fake's turn never ends, and the words never reached it.
#[test]
fn words_into_a_running_turn_stop_it() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-words-stop");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "take this slowly".to_string(),
    });
    let logged =
        |words: &str| std::fs::read_to_string(&log).is_ok_and(|logged| logged.contains(words));
    said_until(&mut app, &events, "the turn under way", |_| {
        logged("take this slowly")
    });

    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "something else first".to_string(),
    });
    said_until(&mut app, &events, "the words at the agent", |_| {
        logged("something else first")
    });
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    let cancelled = logged
        .find("session/cancel")
        .expect("the turn was not stopped");
    assert!(
        cancelled < logged.find("something else first").expect("said"),
        "the words went before the turn was stopped:\n{logged}"
    );
}

/// A form answered on its cards from the chat -- in a conversation begun
/// there, which is never the one on screen -- goes to that conversation a
/// field at a time, each card closed as it is taken; and a number it will
/// not take is said in the thread, the card left open for another go.
///
/// Broken deliberately twice. Taking each answer off the conversation on
/// screen: there was none, the first field was never taken, and the agent
/// waited for ever. And closing a card before its answer was taken: the
/// card that refused `twelve` was closed as answered.
#[test]
fn a_form_is_answered_on_its_cards_in_its_own_conversation() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-form-cards");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/ask".to_string(),
    });
    let press =
        |asked: u64, chosen: Option<&str>, words: Option<&str>| obelus_bot::Event::Answered {
            from: "U1".to_string(),
            asked,
            chosen: chosen.map(str::to_string).into_iter().collect(),
            words: words.map(str::to_string),
        };
    let mut said = said_until(&mut app, &events, "how", |said| {
        asks_in(said, "F1").len() == 1
    });
    let how = asks_in(&said, "F1")[0].0;
    let _ = platform.send(press(how, Some("fast"), None));
    said.extend(said_until(&mut app, &events, "sure", |said| {
        asks_in(said, "F1").len() == 1
    }));
    let sure = asks_in(&said, "F1")[1].0;
    let _ = platform.send(press(sure, Some("on"), None));
    said.extend(said_until(&mut app, &events, "times", |said| {
        asks_in(said, "F1").len() == 1
    }));
    let times = asks_in(&said, "F1")[2].0;
    let closed = |said: &[obelus_bot::model::Out], which: u64| {
        said.iter().any(
            |out| matches!(out, obelus_bot::model::Out::Settle { asked, .. } if *asked == which),
        )
    };
    assert!(
        closed(&said, how) && closed(&said, sure),
        "a card taken was not closed: {said:#?}"
    );

    let _ = platform.send(press(times, None, Some("twelve")));
    let refused = said_until(&mut app, &events, "the number refused", |said| {
        in_thread(said, "F1", "not a number this takes")
    });
    assert!(
        !closed(&refused, times),
        "the card was closed on an answer not taken: {refused:#?}"
    );
    let _ = platform.send(press(times, None, Some("3")));
    let said = said_until(&mut app, &events, "the answers", |said| {
        in_thread(said, "F1", "you said [fast] [true] [3]")
    });
    assert!(
        closed(&said, times),
        "the last card was not closed: {said:#?}"
    );
}

/// Cancel on a card gives up on the whole question, as escape does on the
/// card here: the agent hears it declined, and the card is closed saying
/// so -- for the reader on the list and nobody else.
///
/// Broken deliberately three ways. Taking the cancel as an answer of
/// nothing: the form would not take it, said why, and the agent waited.
/// Not closing the card: no `Settle` came. And taking a cancel from
/// anybody: the stranger's gave up on it.
#[test]
fn cancel_on_a_card_gives_up_on_the_question() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-cancel-card");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/ask".to_string(),
    });
    let said = said_until(&mut app, &events, "how", |said| {
        asks_in(said, "F1").len() == 1
    });
    let how = asks_in(&said, "F1")[0].0;
    let cancel = |from: &str| obelus_bot::Event::Cancelled {
        from: from.to_string(),
        asked: how,
    };

    let _ = platform.send(cancel("U9STRANGER"));
    let mut said = Vec::new();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < until {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
        said.extend(said_since());
    }
    assert!(
        !in_thread(&said, "F1", "you would not say") && app.asking_in_thread_for_test("F1"),
        "a stranger's cancel gave up on it: {said:#?}"
    );

    let _ = platform.send(cancel("U1"));
    let said = said_until(&mut app, &events, "the question given up on", |said| {
        in_thread(said, "F1", "you would not say")
    });
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Settle { asked, said, .. }
                if *asked == how && said.contains("Not answered")
        )),
        "the card was not closed: {said:#?}"
    );
}

/// Words in a thread whose question the chat was never given -- a page
/// to open on the machine -- cancel it all the same, said in words where
/// there is no card to close, and are said.
///
/// Broken deliberately by cancelling only a question the thread has a card
/// for: the page went with the stopped turn, and nothing said it was not
/// answered.
#[test]
fn a_question_the_chat_was_never_given_is_said_to_be_the_machines() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-only-here");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/signin".to_string(),
    });
    // Until the page to open is up here, which the thread is never told.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !app.asking_in_thread_for_test("F1") {
        assert!(
            std::time::Instant::now() < until,
            "gave up waiting for the page"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "never mind signing in".to_string(),
    });
    passed_by(&mut app, &events, &log, "never mind signing in");
}

/// Waits for words in a thread with a question up and no card for it to
/// have cancelled the question -- said in the thread, there being no card
/// to close -- and reached the agent.
fn passed_by(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<Event>,
    log: &std::path::Path,
    words: &str,
) {
    let said = said_until(app, events, "the question passed by", |said| {
        in_thread(said, "F1", "Not answered")
            && std::fs::read_to_string(log).is_ok_and(|logged| logged.contains(words))
    });
    assert!(
        !in_thread(&said, "F1", "card above")
            && !said
                .iter()
                .any(|out| matches!(out, obelus_bot::model::Out::Settle { .. })),
        "the thread was pointed at a card it never had: {said:#?}"
    );
}

/// A question the platform would not take as a card is said in words by
/// the platform, and words after it cancel it in words too -- there is no
/// card it got to close.
///
/// Broken deliberately by not hearing that the card was refused: a card
/// that was never there was closed.
#[test]
fn a_card_the_platform_refused_is_not_pointed_at() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-refused-card");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
    let asked = asked_with(&said, "F1", "Allow once").expect("asked");
    let _ = platform.send(obelus_bot::Event::Unasked { asked });
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "look somewhere else".to_string(),
    });
    passed_by(&mut app, &events, &log, "look somewhere else");
}

/// A question's card in the chat is closed when nothing is waiting on it any
/// more: the conversation closed here, or the agent behind it gone.
///
/// Broken deliberately twice. Not closing the card with the conversation:
/// no `Settle` came, and it could still be pressed. And not with the agent:
/// the same, when the agent died with a question up.
#[test]
fn a_card_closes_when_nothing_waits_on_it() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-card-closes");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let closed = |said: &[obelus_bot::model::Out], which: u64, words: &str| {
        said.iter().any(|out| {
            matches!(out, obelus_bot::model::Out::Settle { asked, said, .. }
                if *asked == which && said.contains(words))
        })
    };

    // A conversation closed here with a question up.
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the first question", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
    let first = asked_with(&said, "F1", "Allow once").expect("asked");
    let last = app.document_count_for_test() - 1;
    app.go_to_document_for_test(obelus_buffer::DocumentId::new(last));
    dispatch::dispatch(&mut app, Command::DocumentClose);
    dispatch::dispatch(&mut app, Command::DocumentClose);
    said_until(
        &mut app,
        &events,
        "the card closed with the conversation",
        |said| closed(said, first, "Closed"),
    );

    // And the agent gone, with a question up in another conversation:
    // a third, begun from the chat, tells it to die. Last, because what
    // starts again after it is the agent the settings name, not the fake.
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F2".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the next question", |said| {
        asked_with(said, "F2", "Allow once").is_some()
    });
    let next = asked_with(&said, "F2", "Allow once").expect("asked");
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F3".to_string()),
        text: "/die".to_string(),
    });
    said_until(
        &mut app,
        &events,
        "the card closed with the agent",
        |said| closed(said, next, "stopped asking"),
    );
}

/// What a conversation says while the connection is on its way back up
/// goes all the same -- the platform holds it until it is -- rather than
/// being dropped for the five seconds it takes.
///
/// Broken deliberately by sending only while connected: the question was
/// never said in the thread.
#[test]
fn what_is_said_while_reconnecting_still_goes() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-reconnecting");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::connection(
        obelus_bot::State::Connecting,
        None,
    ));
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    said_until(&mut app, &events, "the question in the thread", |said| {
        asked_with(said, "F1", "Allow once").is_some()
    });
}

/// A thread that would not open is not asked for again on every frame --
/// a platform that refused once refuses every time -- but is once the
/// connection is up again, and on the next connection, which may be one the
/// reader has mended.
///
/// Broken deliberately five times. Not keeping what would not open: it
/// was asked for again at once. Not letting go of that when the connection
/// is up again: it was never asked for again on it. Not letting go of that
/// on a new connection: the new one was never asked. Not saying so in the
/// conversation: the reader was left to wonder where its thread was. And
/// saying the one thing for both: a thread let go of while waiting was
/// blamed on the platform.
#[test]
fn a_thread_that_would_not_open_is_asked_for_on_the_next_connection() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-unopened");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Unopened {
        asked,
        waited: false,
    });
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    assert!(
        !said_since()
            .iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. })),
        "a thread that would not open was asked for again at once"
    );
    // Asked for again once the connection says it is up -- a socket made
    // again inside one connection, which is when a request that timed out
    // is worth making again.
    let _ = platform.send(obelus_bot::Event::connection(
        obelus_bot::State::Connected,
        None,
    ));
    let said = said_until(&mut app, &events, "the thread asked for again", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    // And the conversation says so, where the reader is.
    let dump = support::render(&mut app, 76, 24);
    assert!(
        dump.contains("Slack would not start a thread"),
        "the conversation said nothing:\n{dump}"
    );
    // And says what it was when it was let go of here, waiting too long
    // for a connection: nothing the platform did.
    let again = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for again");
    let _ = platform.send(obelus_bot::Event::Unopened {
        asked: again,
        waited: true,
    });
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    let dump = support::render(&mut app, 76, 24);
    assert!(
        dump.contains("Slack was not reached in time"),
        "a thread let go of here was blamed on the platform:\n{dump}"
    );

    let (out, mut heard) = tokio::sync::mpsc::unbounded_channel();
    let _ = platform.send(obelus_bot::Event::Started {
        platform: "slack",
        out,
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(obelus_bot::model::Out::Open { .. }) = heard.try_recv() {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "the new connection was never asked for the thread"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
}

/// A connection that has been let go is not heard: what it says late is
/// about a connection nobody wants, and not about the one that replaced
/// it.
///
/// Broken deliberately by hearing every connection: the old one's late
/// refusal was taken as the new one's, and the page said Slack had
/// refused a token that was working.
#[test]
fn a_connection_let_go_is_not_heard() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-let-go");
    let (mut app, events) = connected(&scratch);
    let old = the_platform();
    to_the_row(&mut app, "Platform");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "Off");
    support::press(&mut app, KeyCode::Enter);
    support::lay_out(&mut app, 66, 20);
    choose_slack(&mut app);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the new connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let _ = old.send(obelus_bot::Event::connection(
        obelus_bot::State::Refused,
        None,
    ));
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 66, 20);
    }
    assert_eq!(
        app.bot_state_for_test(),
        obelus_bot::State::Connected,
        "the old connection's word was taken as the new one's"
    );
}

/// A conversation whose agent has gone is still the one its thread is
/// about: what is said there finds it -- by the name it went by, since it
/// has none until a new session arrives -- and starts the agent again, the
/// way typing into it here does.
///
/// Broken deliberately by finding a conversation only by the name it has
/// now: the reply was answered "not open on this machine", which it was.
#[test]
fn a_thread_finds_its_conversation_while_its_agent_is_gone() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-agent-gone");
    // Installed and named in the settings, the way a reader's is, because
    // starting it *again* goes through that.
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let log = scratch.join("asked.log");
    let root = scratch.join("agents");
    obelus_agent::remember(
        "fake",
        std::path::Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
            "prompts".to_string(),
        ],
        "0.1",
        &root,
    )
    .expect("writing what was installed");
    std::fs::write(
        scratch.join("config.toml"),
        "agent = \"fake\"\nchat = \"slack\"\n[chats.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.agents_root_for_test(root);
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/die".to_string(),
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("/die")
    {
        assert!(
            std::time::Instant::now() < until,
            "the agent was never asked"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    for _ in 0..20 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        asked_with(said, "F1", "Allow once").is_some() || in_thread(said, "F1", "not open")
    });
    assert!(
        !in_thread(&said, "F1", "not open"),
        "the conversation was not found: {said:#?}"
    );
}

/// Words from the chat that waited for the turn they stopped to end, and
/// came back to the box here when the reader pressed escape in between, are
/// the reader's once they send them:
/// nothing tells the agent they came from the chat, and the thread hears
/// them as said on this machine.
///
/// Broken deliberately by marking the conversation when the words arrive,
/// as it was: the mark outlived them, the reader's own words went to the
/// agent as sent from Slack, and the thread never heard them.
#[test]
fn words_from_the_chat_taken_back_here_are_the_readers() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-taken-back");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/pausing".to_string(),
    });
    said_until(&mut app, &events, "the words before the call", |said| {
        in_thread(said, "F1", "looking at it first")
    });
    app.go_to_document_for_test(obelus_buffer::DocumentId::new(
        app.document_count_for_test() - 1,
    ));
    // Handed straight in, and escape pressed before anything else is heard:
    // words from the chat stop the turn the moment they arrive, so the one
    // moment they wait to be taken back is between the stop and its end.
    app.handle(Event::Bot(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "and later".to_string(),
    }));
    support::press(&mut app, KeyCode::Esc);
    until(&mut app, &events, "the turn to stop", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::press(&mut app, KeyCode::Enter);
    said_until(&mut app, &events, "the words in the thread", |said| {
        in_thread(said, "F1", "and later")
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let prompt = loop {
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if let Some(line) = logged
            .lines()
            .find(|line| line.contains("\"text\":\"and later"))
        {
            break line.to_string();
        }
        assert!(
            std::time::Instant::now() < until,
            "the words never reached the agent:\n{logged}"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    };
    assert!(
        !prompt.contains("sent from Slack"),
        "the reader's own words went as from the chat: {prompt}"
    );
}

/// Words from the chat and the reader's own, waiting on one turn, go to the
/// agent together when it ends -- which the chat's words make it do -- with
/// nothing saying they came from the
/// chat, since the reader is evidently at the machine -- and the thread
/// hears the reader's own, and only those, as said on this machine.
///
/// Broken deliberately twice. Telling the agent they came from the chat
/// whenever any did: the prompt said so. And leaving the thread to hear
/// the whole of what went: the chat's own words came back to it as said
/// on this machine.
#[test]
fn words_from_here_and_from_the_chat_go_together() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-mixed");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/later".to_string(),
    });
    until(&mut app, &events, "the turn to start", |_| {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("/later")
    });
    let _ = platform.send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "from the phone".to_string(),
    });
    app.go_to_document_for_test(obelus_buffer::DocumentId::new(
        app.document_count_for_test() - 1,
    ));
    support::type_text(&mut app, "from the desk");
    support::press(&mut app, KeyCode::Enter);
    let said = said_until(&mut app, &events, "what was typed here", |said| {
        in_thread(said, "F1", "from the desk")
    });
    assert!(
        !in_thread(&said, "F1", "from the phone"),
        "the chat's own words came back to it: {said:#?}"
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let prompt = loop {
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if let Some(line) = logged.lines().find(|line| line.contains("from the desk")) {
            break line.to_string();
        }
        assert!(
            std::time::Instant::now() < until,
            "the words never reached the agent:\n{logged}"
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    };
    assert!(
        prompt.contains("from the phone"),
        "the two did not go together: {prompt}"
    );
    assert!(
        !prompt.contains("sent from Slack"),
        "the agent was told the reader is away: {prompt}"
    );
}

/// In the room, somebody not on the list is not heard: a thread they start
/// begins nothing and is not answered.
///
/// Broken deliberately by hearing anybody in the room: the stranger's
/// thread began a conversation and the agent was asked.
#[test]
fn somebody_not_on_the_list_is_not_heard_in_the_room() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-stranger");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    somebody_says("U0STRANGER", "C1", "what is in here");
    for _ in 0..20 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !logged.contains("what is in here"),
        "a stranger's words reached the agent:\n{logged}"
    );
    assert!(said_since().is_empty(), "a stranger was answered");
}

/// A window set to Slack with the reader on its list and a room kept, and
/// its channel -- not yet talking to the chat.
fn a_window(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    (app, events)
}

/// Both windows' events, handled, until `done` holds.
fn both_until(
    first: &mut App,
    firsts: &std::sync::mpsc::Receiver<Event>,
    second: &mut App,
    seconds: &std::sync::mpsc::Receiver<Event>,
    what: &str,
    done: impl Fn(&App, &App) -> bool,
) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !done(first, second) {
        assert!(
            std::time::Instant::now() < until,
            "gave up waiting for {what}"
        );
        while let Ok(event) = firsts.try_recv() {
            first.handle(event);
        }
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(20)) {
            second.handle(event);
        }
        support::lay_out(first, 76, 24);
        support::lay_out(second, 76, 24);
    }
}

/// Settings, secrets and a room for two windows to share.
fn set_up_for_two(scratch: &support::Scratch) {
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(
        scratch.join("config.toml"),
        "chat = \"slack\"\n[chats.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
}

/// Connecting in a second window takes the chat from the first: the first
/// hears it asked for, lets go and says so, and the second connects -- one
/// window talking to the chat at a time, whichever the reader last said.
///
/// Broken deliberately twice. Not letting go when asked: the second gave
/// up after ten seconds and the first still had it. And taking the lock
/// without asking for it: both connected at once.
#[test]
fn connecting_in_another_window_takes_the_chat_over() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-take-over");
    set_up_for_two(&scratch);
    // The first with a watcher of its own, which is how it hears.
    let mut first = App::new(Vec::new());
    first.config_file_for_test(scratch.join("config.toml"));
    let (sender, firsts) = obelus_app::event::channel();
    first.start(sender);
    support::lay_out(&mut first, 76, 24);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });

    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    assert!(
        !second.holds_the_bot_for_test(),
        "the second took the chat while the first had it"
    );
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the chat to move",
        |first, second| !first.holds_the_bot_for_test() && second.holds_the_bot_for_test(),
    );
    assert_eq!(first.note(), Some("Slack went to another window"));
    until(&mut second, &seconds, "the second to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
}

/// A window asked for the chat that never hears it -- a watch is freshness
/// and not a promise -- does not leave the asker waiting for ever: it gives
/// up, and says why.
///
/// Broken deliberately twice. Not giving up: no word came, and the mark
/// went on turning. And dropping the answer that came after: the first
/// let go, and nobody had the chat.
#[test]
fn a_window_that_will_not_let_go_is_given_up_on() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-not-let-go");
    set_up_for_two(&scratch);
    // Without a watcher, so it never hears the asking.
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while second.note() != Some("Another window would not let Slack go") {
        assert!(
            std::time::Instant::now() < deadline,
            "the asking was never given up on"
        );
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(50)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    assert!(first.holds_the_bot_for_test());
    assert!(!second.holds_the_bot_for_test());

    // And the first letting go after all -- here by being told to, the way
    // one that heard late would -- leaves the chat with the window that
    // asked, rather than with nobody.
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    until(&mut second, &seconds, "the second to have it", |app| {
        app.holds_the_bot_for_test()
    });
}

/// A window that goes takes its hold on the chat with it -- the kernel's
/// lock, given up with the process -- and a window waiting for it has it.
///
/// Broken deliberately by asking for the lock once rather than waiting on
/// it: the second never had it, though nobody else did.
#[test]
fn a_window_that_goes_leaves_the_chat_to_the_one_waiting() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-goes");
    set_up_for_two(&scratch);
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    drop(first);
    until(&mut second, &seconds, "the second to have it", |app| {
        app.holds_the_bot_for_test()
    });
}

/// `disconnect-from-chat` lets the chat go: the status row stops saying it,
/// and another window has it at once, without asking.
///
/// Broken deliberately by not letting go of the lock: the second window
/// had to ask, and waited on a window that still held it.
#[test]
fn disconnecting_leaves_the_chat_to_no_window() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-disconnect");
    set_up_for_two(&scratch);
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    assert!(!first.holds_the_bot_for_test());
    let dump = support::render(&mut first, 76, 24);
    let status = support::text_block(&dump)
        .lines()
        .last()
        .unwrap_or_default()
        .to_string();
    assert!(
        !status.contains("● Slack"),
        "the status row still says it: {status}"
    );
    let (mut second, _seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    assert!(
        second.holds_the_bot_for_test(),
        "the second had to wait for a chat nobody had"
    );
}

/// A token the chat refused is said on its row, and only there: the page
/// holds what the reader set, and a value that is wrong is something they
/// can mend on it.
///
/// Broken deliberately by saying nothing for a refusal: the row was as it
/// always is, and nothing on the page said which value to look at.
#[test]
fn a_refused_token_is_said_on_its_row() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-refused");
    let (mut app, events) = connected(&scratch);
    let _ = the_platform().send(obelus_bot::Event::connection(
        obelus_bot::State::Refused,
        None,
    ));
    until(&mut app, &events, "the refusal", |app| {
        app.bot_state_for_test() == obelus_bot::State::Refused
    });
    let dump = support::render(&mut app, 66, 30);
    assert!(dump.contains("Refused by Slack"), "{dump}");
}

/// An asking can be taken back with `disconnect-from-chat`, while
/// `connect-to-chat` -- which would only wait behind it -- is dim; and an
/// asking taken back is withdrawn, so the chat stays where it was.
///
/// Broken deliberately three ways. Offering `connect-to-chat` while asking:
/// it was lit and did nothing. Not offering `disconnect-from-chat`: there was
/// no way to stop asking. And taking the lock when it came anyway: the
/// window had the chat the reader had told it not to.
#[test]
fn an_asking_can_be_taken_back() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-take-back");
    set_up_for_two(&scratch);
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    assert!(
        !second.offers(Command::BotConnect),
        "asking again was offered"
    );
    assert!(
        second.offers(Command::BotDisconnect),
        "stopping was not offered"
    );
    dispatch::dispatch(&mut second, Command::BotDisconnect);
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    for _ in 0..20 {
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(30)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    assert!(
        !second.holds_the_bot_for_test(),
        "a window took the chat after being told to stop asking"
    );
}

/// An asking written the moment another window has the chat is heard: the
/// window watches for it wherever a chat is set, not from the frame after
/// it has the chat, which is a frame in which an asking would be missed.
///
/// Broken deliberately by watching only while it has the chat: the asking
/// in that frame was never heard, and the second window gave up.
#[test]
fn an_asking_the_moment_the_chat_is_had_is_heard() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-first-frame");
    set_up_for_two(&scratch);
    let mut first = App::new(Vec::new());
    first.config_file_for_test(scratch.join("config.toml"));
    let (sender, firsts) = obelus_app::event::channel();
    first.start(sender);
    support::lay_out(&mut first, 76, 24);
    let (mut second, seconds) = a_window(&scratch);
    // No frame between the first having it and the second asking.
    dispatch::dispatch(&mut first, Command::BotConnect);
    dispatch::dispatch(&mut second, Command::BotConnect);
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the chat to move",
        |first, second| !first.holds_the_bot_for_test() && second.holds_the_bot_for_test(),
    );
}

/// Asking again after giving up waits on the same thread as before: one
/// thread a window, however often the reader asks a window that will not
/// let go -- and the chat still comes to it when that window does.
///
/// Broken deliberately by starting a thread for every asking: the second
/// asking set a second one waiting.
#[test]
fn asking_again_waits_on_the_same_thread() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-ask-again");
    set_up_for_two(&scratch);
    // Without a watcher, so it never hears the asking.
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while second.note() != Some("Another window would not let Slack go") {
        assert!(
            std::time::Instant::now() < deadline,
            "the asking was never given up on"
        );
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(50)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    dispatch::dispatch(&mut second, Command::BotConnect);
    assert_eq!(
        second.waiters_for_test(),
        1,
        "asking again set another thread waiting"
    );
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    until(&mut second, &seconds, "the second to have it", |app| {
        app.holds_the_bot_for_test()
    });
}

/// A clock that ran out for an asking since taken back gives up on nothing:
/// its word was already on its way when the asking was taken back, and the
/// asking after it waits on the same thread under the same number.
///
/// Broken deliberately by numbering the clock with the asking's number
/// again: the new asking was given up on the moment the old clock's word
/// arrived.
#[test]
fn a_clock_from_an_asking_taken_back_gives_up_on_nothing() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-old-clock");
    set_up_for_two(&scratch);
    // Without a watcher, so it never hears the asking.
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    // The clock's word, caught on its way rather than heard.
    let ran_out = loop {
        match seconds.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(event @ obelus_app::event::Event::NotLetGo(_)) => break event,
            Ok(event) => second.handle(event),
            Err(_) => panic!("the clock never ran out"),
        }
    };
    dispatch::dispatch(&mut second, Command::BotDisconnect);
    dispatch::dispatch(&mut second, Command::BotConnect);
    second.handle(ran_out);
    assert_ne!(
        second.note(),
        Some("Another window would not let Slack go"),
        "the new asking was given up on by the old clock"
    );
    assert!(
        second.offers(Command::BotDisconnect),
        "the new asking is not going on"
    );
}

/// A window whose chat is set to nothing lets go of it, so the next window
/// to want it has it at once, without asking.
///
/// Broken deliberately by keeping the lock when the chat is set to nothing:
/// the second window had to ask, and waited on a window with no chat.
#[test]
fn a_window_set_to_no_chat_lets_go_of_it() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-set-off");
    set_up_for_two(&scratch);
    // The first with a settings file of its own, so that setting its chat
    // to nothing leaves the second's alone.
    std::fs::copy(scratch.join("config.toml"), scratch.join("first.toml")).expect("a copy");
    let mut first = App::new(vec![support::open_fixture("sample.rs")]);
    first.config_file_for_test(scratch.join("first.toml"));
    let firsts = support::drive(&mut first);
    support::lay_out(&mut first, 66, 20);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    dispatch::dispatch(&mut first, Command::ConfigOpen);
    support::press(&mut first, KeyCode::Tab);
    support::press(&mut first, KeyCode::Tab);
    to_the_row(&mut first, "Platform");
    support::press(&mut first, KeyCode::Enter);
    support::type_text(&mut first, "Off");
    support::press(&mut first, KeyCode::Enter);
    assert_eq!(first.config().chat, None, "the chat was not set to nothing");
    support::lay_out(&mut first, 66, 20);
    let (mut second, _seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    assert!(
        second.holds_the_bot_for_test(),
        "the second had to ask a window with no chat"
    );
}

/// An asking taken back stays taken back, even when it was asking again
/// after giving up once: the window that had the chat letting go later
/// leaves it with nobody, not with the window the reader told to stop.
///
/// Broken deliberately by keeping what was given up on when taking the
/// asking back: the late answer to it handed the chat over anyway.
#[test]
fn an_asking_taken_back_after_giving_up_stays_taken_back() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-take-back-again");
    set_up_for_two(&scratch);
    // Without a watcher, so it never hears the asking.
    let (mut first, firsts) = a_window(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while second.note() != Some("Another window would not let Slack go") {
        assert!(
            std::time::Instant::now() < deadline,
            "the asking was never given up on"
        );
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(50)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    dispatch::dispatch(&mut second, Command::BotConnect);
    dispatch::dispatch(&mut second, Command::BotDisconnect);
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    for _ in 0..20 {
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(30)) {
            second.handle(event);
        }
        support::lay_out(&mut second, 76, 24);
    }
    assert!(
        !second.holds_the_bot_for_test(),
        "a window took the chat after being told to stop asking"
    );
}

/// The last row on the screen, which is the status row.
fn status_row(app: &mut App) -> String {
    let dump = support::render(app, 76, 24);
    support::text_block(&dump)
        .lines()
        .last()
        .unwrap_or_default()
        .to_string()
}

/// Handles whatever comes back for a moment, for a test asserting that
/// something did *not* happen.
fn settle(app: &mut App, events: &std::sync::mpsc::Receiver<Event>) {
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
    }
}

/// A window with no file open -- the welcome screen -- set to Slack with
/// both tokens kept and connected to the fake.
fn connected_on_nothing(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    // And the fake's sink kept, which it does after saying it is connected.
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
            && FAKED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
    });
    (app, events)
}

/// Where the chat stands is on every status row of the window that has it:
/// the welcome screen's, the notes', and a conversation's -- which is the
/// row a reader working from a chat is most often on.
///
/// Broken deliberately three ways, one row at a time: dropping the mark
/// from the conversation's row, from the notes' and from the welcome
/// screen's each left that row saying nothing about the chat -- which is
/// how the connection going wrong went unseen. The row that is only words,
/// with no file under them, is `a_chat_that_goes_wrong_says_why_once`'s.
#[test]
fn the_chat_is_marked_on_every_status_row() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-every-row");
    let (mut app, _events) = connected_on_nothing(&scratch);
    // A key, to take away what connecting said: with words on it, the row
    // is the words' and not the welcome screen's.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(app.note(), None);
    let line = status_row(&mut app);
    assert!(line.contains("● Slack"), "the welcome screen's row: {line}");
    dispatch::dispatch(&mut app, Command::TodoOpen);
    assert!(app.notes().is_some(), "the notes did not open");
    let line = status_row(&mut app);
    assert!(line.contains("● Slack"), "the notes' row: {line}");
    drop(app);

    let scratch = support::Scratch::new("bot-every-row-chat");
    let (mut app, _events, _log) = paired_with_an_agent(&scratch);
    dispatch::dispatch(&mut app, Command::ConversationNew);
    assert!(app.chat().is_some(), "no conversation on screen");
    let line = status_row(&mut app);
    assert!(line.contains("● Slack"), "the conversation's row: {line}");
}

/// A connection that goes wrong says why on the status row, once, as it
/// goes -- not again each time it is tried and goes wrong the same way, and
/// again when it goes wrong some other way -- and the mark stays. What no
/// row of the settings page is about is said on the chat's own row there,
/// in the platform's words.
///
/// Broken deliberately five ways. Not saying it: the row had the mark and
/// no words. Saying it every time: the second refusal put the words back
/// after the reader's key had taken them away. Saying it only on the way
/// from right to wrong: the second kind of wrong was never said. Dropping
/// the mark from a row that is only words, with no file under them: the
/// words were there and the mark was not. And leaving the platform's row
/// on the page without a warning: nothing there said why.
#[test]
fn a_chat_that_goes_wrong_says_why_once() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-says-why");
    let (mut app, events) = connected_on_nothing(&scratch);
    let unreachable = || {
        the_platform().send(obelus_bot::Event::connection(
            obelus_bot::State::Unreachable,
            Some("dns error".to_string()),
        ))
    };
    let _ = unreachable();
    until(&mut app, &events, "the words", |app| {
        app.note() == Some("Not connected to Slack: dns error")
    });
    let line = status_row(&mut app);
    assert!(line.contains("Not connected to Slack: dns error"), "{line}");
    assert!(line.contains("✕ Slack"), "{line}");

    support::press(&mut app, KeyCode::Down);
    assert_eq!(app.note(), None, "a key did not take the words away");
    let _ = unreachable();
    settle(&mut app, &events);
    assert_eq!(app.note(), None, "said again for the same thing");
    assert!(status_row(&mut app).contains("✕ Slack"));

    let _ = the_platform().send(obelus_bot::Event::connection(
        obelus_bot::State::Declined,
        Some("the app has no long connection".to_string()),
    ));
    until(&mut app, &events, "the second words", |app| {
        app.note() == Some("Not connected to Slack: the app has no long connection")
    });

    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Tab);
    support::press(&mut app, KeyCode::Tab);
    assert!(
        app.settings().expect("the settings").on_bot(),
        "not on the chat page"
    );
    let dump = support::render(&mut app, 76, 24);
    assert!(dump.contains("the app has no long connection"), "{dump}");
}

/// A chat told to connect with something it has to be told left untold is
/// wrong, not off, and says which thing.
///
/// Broken deliberately by sending `Unready` with no reason: the row said it
/// was not connected and not why.
#[test]
fn a_chat_with_something_untold_says_what() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-untold");
    obelus_bot::platform::connect_for_test(fake_connect);
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the words", |app| {
        app.note() == Some("Not connected to Slack: no Bot token is set")
    });
    let line = status_row(&mut app);
    assert!(line.contains("✕ Slack"), "{line}");
}

/// A window the chat talks to that goes to another worktree still has the
/// chat once it is there.
///
/// Going to another tree lets go of everything the first one was, and the
/// connection went with it: a reader away from the machine who went to
/// another tree had cut themselves off. Broken deliberately by not
/// connecting again in `App::move_to_tree`: the chat is no longer here.
#[test]
fn the_chat_goes_with_a_window_to_another_worktree() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-worktree");
    let git = |directory: &std::path::Path, arguments: &[&str]| {
        let outcome = support::git()
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "obelus")
            .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
            .env("GIT_COMMITTER_NAME", "obelus")
            .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
            .output()
            .expect("running git");
        assert!(outcome.status.success(), "git {arguments:?} failed");
    };
    let main = scratch.join("main");
    std::fs::create_dir_all(&main).expect("the main checkout");
    git(&main, &["init", "--quiet", "--initial-branch=master"]);
    std::fs::write(main.join("file.rs"), "fn main() {}\n").expect("a file");
    git(&main, &["add", "file.rs"]);
    git(&main, &["commit", "--quiet", "-m", "committed"]);
    let feature = scratch.join("feature");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            feature.to_str().expect("a path"),
        ],
    );

    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    app.working_directory_for_test(main);
    let (sender, events) = obelus_app::event::channel();
    app.start(sender);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });

    dispatch::dispatch(&mut app, Command::WorktreeList);
    for _ in 0..4 {
        let on = app
            .picker()
            .and_then(obelus_component::picker::Picker::selected_item)
            .map(|item| item.label.clone());
        if on.as_deref() == Some("feature") {
            break;
        }
        support::press(&mut app, KeyCode::Down);
    }
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.working_directory()
            .canonicalize()
            .expect("a tree that is there"),
        feature.canonicalize().expect("a tree that is there"),
        "the window did not go to feature"
    );
    until(&mut app, &events, "the connection again", |app| {
        app.holds_the_bot_for_test() && app.bot_state_for_test() == obelus_bot::State::Connected
    });
}

/// A window the chat talks to that closes its project has the chat again
/// once another is chosen.
///
/// Broken deliberately by not saying to connect again in
/// `App::leave_the_project`: the chat is no longer here.
#[test]
fn the_chat_comes_back_once_a_closed_project_is_chosen_again() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-closed");
    let project = scratch.join("project");
    std::fs::create_dir_all(&project).expect("the project");
    std::fs::write(project.join("file.rs"), "fn main() {}\n").expect("a file");

    obelus_bot::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "chat = \"slack\"\n").expect("the settings");
    obelus_bot::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_bot::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    app.working_directory_for_test(project.clone());
    let (sender, events) = obelus_app::event::channel();
    app.start(sender);
    dispatch::dispatch(&mut app, Command::BotConnect);
    until(&mut app, &events, "the connection", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });

    dispatch::dispatch(&mut app, Command::ProjectClose);
    assert!(!app.has_a_project(), "the project was not closed");
    // The row that opens a project not in the list, which is the last.
    support::press_control_key(&mut app, KeyCode::End);
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, &project.display().to_string());
    support::press(&mut app, KeyCode::Esc);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.has_a_project(), "it is still asking which project");
    until(&mut app, &events, "the connection again", |app| {
        app.holds_the_bot_for_test() && app.bot_state_for_test() == obelus_bot::State::Connected
    });
}

/// Everything said to the fake platform until `done` holds, handling both
/// windows' events meanwhile.
fn both_said_until(
    first: &mut App,
    firsts: &std::sync::mpsc::Receiver<Event>,
    second: &mut App,
    seconds: &std::sync::mpsc::Receiver<Event>,
    what: &str,
    done: impl Fn(&[obelus_bot::model::Out]) -> bool,
) -> Vec<obelus_bot::model::Out> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut said = Vec::new();
    loop {
        support::lay_out(first, 76, 24);
        support::lay_out(second, 76, 24);
        said.extend(said_since());
        if done(&said) {
            return said;
        }
        assert!(
            std::time::Instant::now() < until,
            "gave up waiting for {what}; said: {said:#?}"
        );
        while let Ok(event) = firsts.try_recv() {
            first.handle(event);
        }
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(20)) {
            second.handle(event);
        }
    }
}

/// A window with a project of its own -- a directory of the scratch -- and
/// the fake agent to talk to, logging what it is asked into `log`.
fn a_window_on(
    scratch: &support::Scratch,
    tree: &str,
    log: &std::path::Path,
) -> (App, std::sync::mpsc::Receiver<Event>) {
    let root = scratch.join(tree);
    std::fs::create_dir_all(&root).expect("the tree");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(root);
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
            format!("called={tree}"),
            "prompts".to_string(),
        ],
    );
    support::lay_out(&mut app, 76, 24);
    (app, events)
}

/// Two windows on two trees, the first holding the chat and the second
/// heard in it through the first.
fn the_first_with_the_chat(
    scratch: &support::Scratch,
) -> (
    (App, std::sync::mpsc::Receiver<Event>),
    (App, std::sync::mpsc::Receiver<Event>),
) {
    set_up_for_two(scratch);
    let (mut first, firsts) = a_window_on(scratch, "one", &scratch.join("one.log"));
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    // Started after the first has the chat, so that its first frame reads
    // the door the first wrote: a window with no watcher hears nothing
    // after that.
    let (mut second, seconds) = a_window_on(scratch, "two", &scratch.join("two.log"));
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second to be heard through the first",
        |_, second| {
            !second.holds_the_bot_for_test()
                && second.bot_state_for_test() == obelus_bot::State::Connected
        },
    );
    ((first, firsts), (second, seconds))
}

/// A window that does not hold the chat is heard in it all the same,
/// through the one that does: its conversation gets a thread, what is said
/// in that thread reaches its agent, and its status row marks the chat
/// without holding it.
///
/// Broken deliberately four ways. Saying nothing through the relay: no
/// thread was asked for. Answering every thread in the window that holds
/// the chat: the words never reached the second window's agent. Marking
/// the chat only in the window that holds it: the second's row was empty.
/// And on Unix, writing the door with the default mode: others could read
/// its key.
#[test]
fn a_window_without_the_chat_is_heard_in_it_through_the_one_with_it() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-relayed");
    let ((mut first, firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    // Its key read by nobody else on the machine: anybody with it is heard
    // in the chat as the bot.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let door = obelus_logging::state_directory()
            .expect("a state directory")
            .join("remote")
            .join("door");
        let mode = std::fs::metadata(&door)
            .expect("the door")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the door's key is readable by others: {mode:o}"
        );
    }
    assert!(
        status_row(&mut second).contains("\u{25cf} Slack"),
        "the second's status row does not mark the chat: {}",
        status_row(&mut second)
    );

    second.new_conversation();
    second.open_a_session_for_test();
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second's conversation to ask for a thread",
        |said| {
            said.iter()
                .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
        },
    );
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for");
    let _ = the_platform().send(obelus_bot::Event::Opened {
        asked,
        thread: "T2".to_string(),
        link: None,
    });
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("T2".to_string()),
        text: "what is in here".to_string(),
    });
    let log = scratch.join("two.log");
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the words to reach the second window's agent",
        |_, _| std::fs::read_to_string(&log).is_ok_and(|logged| logged.contains("what is in here")),
    );
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(logged.contains("sent from Slack"), "{logged}");
    assert!(
        !std::fs::read_to_string(scratch.join("one.log"))
            .unwrap_or_default()
            .contains("what is in here"),
        "the words went to the window holding the chat"
    );
}

/// A thread begun in the chat, with windows on two trees, is answered with
/// a card asking which project it is for; a reply while the card is up is
/// pointed at it, and the press takes what was written to a window on the
/// tree chosen -- which writes its new thread down beside one the other
/// window wrote since it last read the table.
///
/// Broken deliberately three ways. Answering every fresh thread in the
/// window holding the chat: no card was put. Handing the words to the tree
/// the card offered first rather than the one pressed: they reached the
/// first window's agent. And writing the table of threads whole from what
/// the window had read: the first window's thread went out of it.
#[test]
fn a_thread_begun_in_the_chat_goes_to_the_project_chosen() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-which");
    let ((mut first, firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    // A thread of the first window's, written down after the second has
    // read the table.
    first.new_conversation();
    first.open_a_session_for_test();
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the first's conversation to ask for a thread",
        |said| {
            said.iter()
                .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
        },
    );
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for");
    let _ = the_platform().send(obelus_bot::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    let table = obelus_logging::state_directory()
        .expect("a state directory")
        .join("remote")
        .join("slack")
        .join("threads.toml");
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the first's thread in the table",
        |_, _| std::fs::read_to_string(&table).is_ok_and(|written| written.contains("\"T1\"")),
    );
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second to say where it is",
        |first, _| first.windows_placed_for_test() == 1,
    );
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the card asking which project",
        |said| !asks_in(said, "F1").is_empty(),
    );
    let (asked, question) = asks_in(&said, "F1").remove(0);
    let names: Vec<&str> = question
        .choices
        .iter()
        .map(|(_, name)| name.as_str())
        .collect();
    assert_eq!(names, ["one", "two"], "{question:#?}");

    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("F1".to_string()),
        text: "hello?".to_string(),
    });
    both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the reply to be pointed at the card",
        |said| in_thread(said, "F1", "Answer on the card above."),
    );

    let two = question
        .choices
        .iter()
        .find(|(_, name)| name == "two")
        .map(|(id, _)| id.clone())
        .expect("two offered");
    let _ = the_platform().send(pressed(asked, &two));
    let log = scratch.join("two.log");
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the words to reach the second window's agent",
        |said| {
            in_thread(said, "F1", "\u{2714} two")
                && std::fs::read_to_string(&log)
                    .is_ok_and(|logged| logged.contains("what is in here"))
        },
    );
    assert!(
        !std::fs::read_to_string(scratch.join("one.log"))
            .unwrap_or_default()
            .contains("what is in here"),
        "the words went to the first window too: {said:#?}"
    );
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second's thread in the table beside the first's",
        |_, _| {
            std::fs::read_to_string(&table)
                .is_ok_and(|written| written.contains("\"F1\"") && written.contains("\"T1\""))
        },
    );
}

/// The window holding the chat going without letting it go -- closed, or
/// died -- leaves it to a window that was heard through it, which takes it
/// up and connects.
///
/// Broken deliberately by not taking it up when the relay went: the second
/// window was left heard through nobody.
#[test]
fn a_chat_its_window_did_not_let_go_of_is_taken_up() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-relayed-taken-over");
    let ((first, _firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    drop(first);
    until(
        &mut second,
        &seconds,
        "the second to take the chat up",
        |app| {
            app.holds_the_bot_for_test() && app.bot_state_for_test() == obelus_bot::State::Connected
        },
    );
}

/// A chat the reader let go of is let go of: the windows heard through it
/// stop marking it and do not take it up.
///
/// Broken deliberately by taking a relay's `Stopped` for its having gone
/// without a word: the second window took the chat the reader had just
/// let go of.
#[test]
fn a_chat_let_go_of_is_not_taken_up() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-relayed-given-up");
    let ((mut first, _firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    dispatch::dispatch(&mut first, Command::BotDisconnect);
    until(
        &mut second,
        &seconds,
        "the second to stop marking it",
        |app| !status_row(app).contains("Slack"),
    );
    settle(&mut second, &seconds);
    assert!(
        !second.holds_the_bot_for_test(),
        "the second took up a chat the reader let go of"
    );
}

/// A chat handed to a window that asked for it is heard through that
/// window: the windows heard through the one that had it hear where it
/// went, and join it rather than taking it up.
///
/// Broken deliberately by not reading the door again when its file moved:
/// the third window was left heard through nobody. Which needs the third to
/// hear of the handing on before the second has the chat -- after, the door
/// it reads on hearing it is the second's already.
#[test]
fn a_chat_handed_on_is_heard_through_the_window_it_went_to() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-handed-on");
    set_up_for_two(&scratch);
    // Each with a watcher of its own: the first hears the asking, and the
    // third hears where the chat went.
    let started = |scratch: &support::Scratch| {
        let mut app = App::new(Vec::new());
        app.config_file_for_test(scratch.join("config.toml"));
        let (sender, events) = obelus_app::event::channel();
        app.start(sender);
        support::lay_out(&mut app, 76, 24);
        (app, events)
    };
    let (mut first, firsts) = started(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut third, thirds) = started(&scratch);
    both_until(
        &mut first,
        &firsts,
        &mut third,
        &thirds,
        "the third to be heard through it",
        |first, third| {
            first.door_for_test().is_some() && third.joined_for_test() == first.door_for_test()
        },
    );
    let (mut second, seconds) = a_window(&scratch);
    dispatch::dispatch(&mut second, Command::BotConnect);
    until(&mut first, &firsts, "the first to let go", |app| {
        !app.holds_the_bot_for_test()
    });
    // The third hears the chat was handed on before the second has it, so
    // the door it reads then is nobody's: it is the door's file moving that
    // has to bring it to the second.
    until(
        &mut third,
        &thirds,
        "the third to hear it was handed on",
        |app| app.joined_for_test().is_none(),
    );
    assert!(
        !third.holds_the_bot_for_test(),
        "the third took up a chat that was handed on"
    );
    until(&mut second, &seconds, "the second to connect", |app| {
        app.holds_the_bot_for_test() && app.bot_state_for_test() == obelus_bot::State::Connected
    });
    both_until(
        &mut second,
        &seconds,
        &mut third,
        &thirds,
        "the third to be heard through the second",
        |second, third| {
            second.door_for_test().is_some()
                && third.joined_for_test() == second.door_for_test()
                && third.bot_state_for_test() == obelus_bot::State::Connected
        },
    );
    assert!(second.holds_the_bot_for_test());
    assert!(!third.holds_the_bot_for_test());
}

/// A table of threads that will not read is not written over: the window
/// keeps its own conversation's thread, and every other conversation's in
/// the file is left for whoever mends it.
///
/// Broken deliberately by taking a table that will not read for an empty
/// one: the file was written back with one thread in it.
#[test]
fn a_table_of_threads_that_will_not_read_is_not_written_over() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-table-unread");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let table = obelus_logging::state_directory()
        .expect("a state directory")
        .join("remote")
        .join("slack")
        .join("threads.toml");
    let unreadable = "[s-9\nthread = \"T9\"\n";
    std::fs::write(&table, unreadable).expect("the table");
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for");
    let _ = the_platform().send(obelus_bot::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Thread("T1".to_string()),
        text: "what is in here".to_string(),
    });
    until(
        &mut app,
        &events,
        "the words in the thread to reach the agent",
        |_| std::fs::read_to_string(&log).is_ok_and(|logged| logged.contains("what is in here")),
    );
    assert_eq!(
        std::fs::read_to_string(&table).expect("the table"),
        unreadable,
        "a table that would not read was written over"
    );
}

/// Of the windows left when the one holding the chat goes without a word,
/// one takes it up and the rest are heard through that one -- not two
/// windows each holding a chat.
///
/// Broken deliberately by taking it up without asking whether somebody
/// already had: both windows said they held it.
#[test]
fn of_the_windows_left_one_takes_the_chat_up() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-one-takes-up");
    set_up_for_two(&scratch);
    // With watchers, so that the one that does not take it up hears where
    // it went.
    let started = |scratch: &support::Scratch| {
        let mut app = App::new(Vec::new());
        app.config_file_for_test(scratch.join("config.toml"));
        let (sender, events) = obelus_app::event::channel();
        app.start(sender);
        support::lay_out(&mut app, 76, 24);
        (app, events)
    };
    let (mut first, firsts) = started(&scratch);
    dispatch::dispatch(&mut first, Command::BotConnect);
    until(&mut first, &firsts, "the first to connect", |app| {
        app.bot_state_for_test() == obelus_bot::State::Connected
    });
    let (mut second, seconds) = started(&scratch);
    let (mut third, thirds) = started(&scratch);
    // All three driven: the first has to hear both join before its going
    // can say anything to them.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while [&second, &third]
        .iter()
        .any(|app| app.bot_state_for_test() != obelus_bot::State::Connected)
    {
        assert!(
            std::time::Instant::now() < until,
            "gave up waiting for both to be heard through the first"
        );
        for (app, events) in [
            (&mut first, &firsts),
            (&mut second, &seconds),
            (&mut third, &thirds),
        ] {
            while let Ok(event) = events.try_recv() {
                app.handle(event);
            }
            support::lay_out(app, 76, 24);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(second.joined_for_test().is_some() && third.joined_for_test().is_some());
    // Its channel with it, as a process going takes everything it had.
    drop(first);
    drop(firsts);
    both_until(
        &mut second,
        &seconds,
        &mut third,
        &thirds,
        "one to take it up and the other to join it",
        |second, third| {
            let joined = |holder: &App, other: &App| {
                holder.holds_the_bot_for_test()
                    && holder.door_for_test().is_some()
                    && other.joined_for_test() == holder.door_for_test()
            };
            joined(second, third) || joined(third, second)
        },
    );
    assert!(
        !(second.holds_the_bot_for_test() && third.holds_the_bot_for_test()),
        "two windows hold the chat"
    );
}

/// Cancel on a card of a window heard through the one with the chat goes to
/// that window, under the number it asked with: the agent there hears the
/// question declined, and the card is closed saying so.
///
/// Broken deliberately by passing a cancel through as it came: the window
/// with the chat took it for one of its own, found nothing, and the agent
/// in the other window went on waiting.
#[test]
fn a_cancel_goes_to_the_window_whose_question_it_is() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-relayed-cancel");
    let ((mut first, firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second to say where it is",
        |first, _| first.windows_placed_for_test() == 1,
    );
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "/ask".to_string(),
    });
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the card asking which project",
        |said| !asks_in(said, "F1").is_empty(),
    );
    let (which, question) = asks_in(&said, "F1").remove(0);
    let two = question
        .choices
        .iter()
        .find(|(_, name)| name == "two")
        .map(|(id, _)| id.clone())
        .expect("two offered");
    let _ = the_platform().send(pressed(which, &two));
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the agent's question in the thread",
        |said| !asks_in(said, "F1").is_empty(),
    );
    let how = asks_in(&said, "F1")[0].0;
    let _ = the_platform().send(obelus_bot::Event::Cancelled {
        from: "U1".to_string(),
        asked: how,
    });
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the question given up on",
        |said| in_thread(said, "F1", "you would not say"),
    );
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_bot::model::Out::Settle { asked, said, .. }
                if *asked == how && said.contains("Not answered")
        )),
        "the card was not closed: {said:#?}"
    );
}

/// Cancel on the card asking which project closes it, and begins no
/// conversation anywhere.
///
/// Broken deliberately by taking the cancel as this window's own: nothing
/// closed the card.
#[test]
fn cancel_on_which_project_begins_nothing() {
    let _turn = turn();
    let scratch = support::Scratch::new("bot-which-cancel");
    let ((mut first, firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    both_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second to say where it is",
        |first, _| first.windows_placed_for_test() == 1,
    );
    let _ = the_platform().send(obelus_bot::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_bot::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the card asking which project",
        |said| !asks_in(said, "F1").is_empty(),
    );
    let which = asks_in(&said, "F1")[0].0;
    let _ = the_platform().send(obelus_bot::Event::Cancelled {
        from: "U1".to_string(),
        asked: which,
    });
    both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the card closed",
        |said| {
            said.iter().any(|out| {
                matches!(
                    out,
                    obelus_bot::model::Out::Settle { asked, said, .. }
                        if *asked == which && said.contains("Not answered")
                )
            })
        },
    );
    for log in ["one.log", "two.log"] {
        assert!(
            !std::fs::read_to_string(scratch.join(log))
                .unwrap_or_default()
                .contains("what is in here"),
            "a conversation began in {log}"
        );
    }
}

/// Something the platform would not take for a conversation's thread is
/// said in the conversation, in the platform's words -- and as what it was
/// where it was let go of here, waiting for a connection.
///
/// Broken deliberately twice. Not handling `Unsaid`: the conversation said
/// nothing. And saying the one thing for both: a message let go of while
/// waiting was blamed on the platform.
#[test]
fn what_a_thread_was_not_told_is_said_in_its_conversation() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-unsaid");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let platform = the_platform();
    let _ = platform.send(obelus_bot::Event::Opened {
        asked,
        thread: "T-unsaid".to_string(),
        link: None,
    });
    let _ = platform.send(obelus_bot::Event::Unsaid {
        thread: "T-unsaid".to_string(),
        why: Some("message too long".to_string()),
    });
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 100, 24);
    }
    let dump = support::render(&mut app, 100, 24);
    assert!(
        dump.contains("Slack would not take a message for this conversation: message too long"),
        "the conversation said nothing:\n{dump}"
    );
    let _ = platform.send(obelus_bot::Event::Unsaid {
        thread: "T-unsaid".to_string(),
        why: None,
    });
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 100, 24);
    }
    let dump = support::render(&mut app, 100, 24);
    assert!(
        dump.contains("Slack was not reached in time for a message"),
        "a message let go of here was blamed on the platform:\n{dump}"
    );
}

/// Something the platform would not take for a thread of a window joined
/// to the chat is said in that window's conversation, not in the one that
/// holds the chat.
///
/// Broken deliberately twice. Leaving `Unsaid` to the arm that keeps
/// anything else here: the window holding the chat looked for the thread
/// among its own conversations, found none, and nothing was said. And
/// losing the platform's words on the way down: the joined window blamed
/// the wait for a connection instead.
#[test]
fn what_a_joined_window_s_thread_was_not_told_is_said_there() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-relayed-unsaid");
    let ((mut first, firsts), (mut second, seconds)) = the_first_with_the_chat(&scratch);
    second.new_conversation();
    second.open_a_session_for_test();
    let said = both_said_until(
        &mut first,
        &firsts,
        &mut second,
        &seconds,
        "the second's conversation to ask for a thread",
        |said| {
            said.iter()
                .any(|out| matches!(out, obelus_bot::model::Out::Open { .. }))
        },
    );
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_bot::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for");
    let _ = the_platform().send(obelus_bot::Event::Opened {
        asked,
        thread: "T2".to_string(),
        link: None,
    });
    let _ = the_platform().send(obelus_bot::Event::Unsaid {
        thread: "T2".to_string(),
        why: Some("msg_too_long".to_string()),
    });
    let wanted = "Slack would not take a message for this conversation: msg_too_long";
    let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        while let Ok(event) = firsts.try_recv() {
            first.handle(event);
        }
        if let Ok(event) = seconds.recv_timeout(std::time::Duration::from_millis(20)) {
            second.handle(event);
        }
        let dump = support::render(&mut second, 100, 24);
        if dump.contains(wanted) {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "the joined window's conversation said nothing:\n{dump}"
        );
        support::lay_out(&mut first, 100, 24);
    }
    let dump = support::render(&mut first, 100, 24);
    assert!(
        !dump.contains(wanted),
        "the window holding the chat said it:\n{dump}"
    );
}
