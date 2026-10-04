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

/// A test's turn, with the secrets emptied for it.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    let turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    secrets_of_its_own();
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
/// it -- never the reader's keyring, which a test has no business writing.
fn secrets_of_its_own() {
    let directory = std::env::temp_dir().join(format!("obelus-secrets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    obelus_remote::secrets::secrets_for_test(directory);
}

/// The settings, open on the remote tab, with a settings file of the test's
/// own and a channel the keyring's answers come back on.
fn on_the_remote_page(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
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
    let _turn = turn();
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
    let _turn = turn();
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
    let _turn = turn();
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
    let _turn = turn();
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
    let _turn = turn();
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

/// What the fake platform was handed: where it reports what it hears, and
/// what Obelus asked it to say.
struct Faked {
    told: obelus_remote::platform::Told,
    sink: std::sync::Arc<dyn obelus_sink::Sink<obelus_remote::Event>>,
    said: tokio::sync::mpsc::UnboundedReceiver<obelus_remote::model::Out>,
}

static FAKED: std::sync::Mutex<Option<Faked>> = std::sync::Mutex::new(None);

/// A platform that connects at once and keeps what it is asked to say.
fn fake_connect(
    told: obelus_remote::platform::Told,
    sink: std::sync::Arc<dyn obelus_sink::Sink<obelus_remote::Event>>,
) -> tokio::sync::mpsc::UnboundedSender<obelus_remote::model::Out> {
    let (out, said) = tokio::sync::mpsc::unbounded_channel();
    let _ = sink.send(obelus_remote::Event::Connection(
        obelus_remote::State::Connected,
    ));
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Faked { told, sink, said });
    out
}

/// What the fake platform has been asked to say since the last look.
fn said_since() -> Vec<obelus_remote::model::Out> {
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
    let _ = faked.sink.send(obelus_remote::Event::Heard {
        from: from.to_string(),
        room: room.to_string(),
        at: obelus_remote::model::Where::Fresh(format!("M-{text}")),
        text: text.to_string(),
    });
}

/// The room, as if somebody had paired in it: the group `C1`.
fn a_room_kept() {
    let state = obelus_logging::state_directory().expect("a state directory");
    let at = state.join("remote").join("slack");
    std::fs::create_dir_all(&at).expect("the directory");
    std::fs::write(at.join("room.toml"), "room = \"C1\"\n").expect("the room");
}

/// A window set to Slack with both tokens kept, connected to the fake.
fn connected(scratch: &support::Scratch) -> (App, std::sync::mpsc::Receiver<Event>) {
    obelus_remote::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(scratch.join("config.toml"), "remote = \"slack\"\n").expect("the settings");
    obelus_remote::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_remote::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let (mut app, events) = on_the_remote_page(scratch);
    until(&mut app, &events, "the connection", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_remote::State::Connected)
    });
    (app, events)
}

/// A window set to a chat connects to it with what it was told -- the
/// secrets out of the keyring -- and says so at the top of the page.
///
/// Broken deliberately by leaving the secrets out of what the platform is
/// handed: it was given nothing, and the state stayed `Not set up`. And by
/// handing the status row no chat: the row said nothing about it.
#[test]
fn a_chat_that_is_set_is_connected_to() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-connects");
    let (mut app, _events) = connected(&scratch);
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
    let dump = support::render(&mut app, 66, 20);
    assert!(dump.contains("● Connected"), "{dump}");

    // And on the status row once the page is left, beside the server: the
    // one place that says it while the reader reads.
    support::press(&mut app, KeyCode::Esc);
    let dump = support::render(&mut app, 66, 20);
    let status = support::text_block(&dump)
        .lines()
        .last()
        .unwrap_or_default()
        .to_string();
    assert!(
        status.contains("● Slack"),
        "the status row does not say so:\n{dump}"
    );
}

/// Pairing: a code on the row, the code sent in a group, the person on the
/// list by the name the platform gives them, a word back to them in the
/// thread the code began, and that group kept as the room. A wrong code
/// lets nobody in.
///
/// Broken deliberately four times. Leaving where to begin out of the word
/// back: they were told they were paired and nothing about what next.
/// Comparing the code as it was typed: the code
/// sent in small letters without its dash let nobody in. Taking any
/// code: the stranger's wrong one was taken, and they were asked their
/// name. And not keeping the group: there was no room afterwards.
#[test]
fn pairing_lets_in_whoever_sends_the_code() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-pairs");
    let (mut app, events) = connected(&scratch);
    to_the_row(&mut app, "Pair");
    support::press(&mut app, KeyCode::Enter);
    let code = app
        .settings()
        .and_then(|settings| settings.reached().pairing.clone())
        .expect("a code on the row");
    let dump = support::render(&mut app, 66, 20);
    assert!(dump.contains(&code), "the code is not on the row:\n{dump}");

    somebody_says("U0STRANGER", "C7", "hello?");
    somebody_says("U0STRANGER", "C7", "ABC-DEF");
    let _ = support::render(&mut app, 66, 20);
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(200)) {
        app.handle(event);
    }
    assert!(said_since().is_empty(), "a stranger was answered");

    let sent = format!("  {}  ", code.replace('-', "").to_lowercase());
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
        [obelus_remote::model::Out::Name {
            id: "U04ABCDEF".to_string()
        }]
    );
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_remote::Event::Named {
        id: "U04ABCDEF".to_string(),
        name: "Sunli".to_string(),
    });
    until(&mut app, &events, "them to be let in", |app| {
        app.config()
            .remote_of("slack")
            .is_some_and(|remote| remote.people.iter().any(|person| person.name == "Sunli"))
    });
    assert_eq!(app.note(), Some("Paired Sunli"));
    let said = said_since();
    let code_thread = format!("M-{sent}");
    assert!(
        matches!(
            said.as_slice(),
            [obelus_remote::model::Out::Say { room, thread, to, text, .. }]
                if room == "C7" && *thread == code_thread && to == "U04ABCDEF"
                    && text.contains(obelus_remote::slack::DESCRIPTION.begin)
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
    done: impl Fn(&[obelus_remote::model::Out]) -> bool,
) -> Vec<obelus_remote::model::Out> {
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

/// Whether something was said in this thread with these words in it.
fn in_thread(said: &[obelus_remote::model::Out], thread: &str, words: &str) -> bool {
    said.iter().any(|out| {
        matches!(
            out,
            obelus_remote::model::Out::Say { thread: at, text, .. }
                if at == thread && text.contains(words)
        )
    })
}

/// A conversation and its thread say the same things: a thread opened for
/// it, the reader's words from here marked as from here, the agent's words
/// when its turn is over, its question as numbered words -- answered by a
/// number from the chat -- and words from the chat arriving with a line for
/// the agent saying where they came from.
///
/// Broken deliberately five ways, each failing at its own step. Not opening
/// threads: no `Open` came. Not echoing what was typed here. Posting the
/// turn as it streamed rather than when it ended: the reply was split. Not
/// reading a reply as the answer while a card is up: the permission was
/// never given and the turn never ended. And dropping the line in front of
/// words from afar: the agent's log had no "sent from Slack". The thread's
/// head twice: never said again, it stayed as it opened; said again with
/// the state it had, it never said `Waiting` or `Done`. The header
/// twice more: drawn from no chat, it never said `Slack`; drawn whether or
/// not there is a thread, it said so before there was one.
#[test]
fn a_conversation_and_its_thread_say_the_same_things() {
    let _turn = turn();
    obelus_remote::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let scratch = support::Scratch::new("remote-mirror");
    std::fs::write(
        scratch.join("config.toml"),
        "remote = \"slack\"\n[remotes.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_remote::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_remote::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");

    let log = scratch.join("asked.log");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    app.talk_to(
        "fake",
        std::path::Path::new("sh"),
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
            .any(|out| matches!(out, obelus_remote::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_remote::model::Out::Open { asked, room, .. } if room == "C1" => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for in the room");
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_remote::Event::Opened {
        asked,
        thread: "T1".to_string(),
        link: None,
    });
    // The header says where else the conversation is, once it is there.
    let header = |app: &mut App| {
        let dump = support::render(app, 76, 24);
        support::text_block(&dump)
            .lines()
            .find(|row| row.trim_start().starts_with("0|"))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        !header(&mut app).contains("Slack"),
        "the header said it before the thread was"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !header(&mut app).contains("Slack") {
        assert!(
            std::time::Instant::now() < deadline,
            "the header does not say so: {}",
            header(&mut app)
        );
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(50)) {
            app.handle(event);
        }
    }

    // What the reader types here goes there, marked as said here.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        in_thread(said, "T1", "Allow once")
    });
    assert!(
        in_thread(&said, "T1", "On this machine:_ what is this file"),
        "{said:#?}"
    );
    // And what the agent said before it asked, whole, ahead of the
    // question.
    assert!(in_thread(&said, "T1", "it is a rust file"), "{said:#?}");
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_remote::model::Out::Say { text, notify: true, .. } if text.contains("1. Allow once")
        )),
        "the question did not call the reader: {said:#?}"
    );
    // And the head says it is waiting on them.
    let state = |said: &[obelus_remote::model::Out]| {
        said.iter().rev().find_map(|out| match out {
            obelus_remote::model::Out::Retitle { head, .. } => head.state,
            _ => None,
        })
    };
    assert_eq!(
        state(&said),
        Some(obelus_remote::model::Turning::Waiting),
        "the head does not say it is waiting: {said:#?}"
    );

    // A number from the chat answers it, and the rest of the turn follows.
    let _ = sink.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Thread("T1".to_string()),
        text: "1".to_string(),
    });
    let said = said_until(
        &mut app,
        &events,
        "the end of the turn in the thread",
        |said| in_thread(said, "T1", "and I was allowed"),
    );
    assert_eq!(
        state(&said),
        Some(obelus_remote::model::Turning::Done),
        "the head does not say the turn is over: {said:#?}"
    );
    assert!(
        said.iter()
            .filter(|out| matches!(out, obelus_remote::model::Out::Say { .. }))
            .count()
            == 1,
        "the turn went out in pieces: {said:#?}"
    );

    // Words from the chat go to the agent with a line saying where from.
    let _ = sink.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Thread("T1".to_string()),
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
    obelus_remote::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let scratch = support::Scratch::new("remote-titled");
    std::fs::write(
        scratch.join("config.toml"),
        "remote = \"slack\"\n[remotes.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_remote::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_remote::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    app.talk_to(
        "fake",
        std::path::Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_remote::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_remote::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let sink = FAKED
        .lock()
        .ok()
        .and_then(|faked| faked.as_ref().map(|faked| faked.sink.clone()))
        .expect("connected");
    let _ = sink.send(obelus_remote::Event::Opened {
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
                obelus_remote::model::Out::Retitle { head, .. }
                    if head.state == Some(obelus_remote::model::Turning::Done)
            )
        })
    });
    // While the turn was still going: the end of a turn says the head again
    // anyway, and a rename that waited for it would be a rename that a long
    // turn kept from the reader for as long as it ran.
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_remote::model::Out::Retitle { thread, head, .. }
                if thread == "T1"
                    && head.title == "Renamed by the agent"
                    && head.state == Some(obelus_remote::model::Turning::Working)
        )),
        "the thread was not renamed when the agent named it: {said:#?}"
    );
    // And the turn's start, which happened before the platform had answered
    // with the thread, said on it once it had.
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_remote::model::Out::Retitle { head, .. }
                if head.title == "A conversation"
                    && head.state == Some(obelus_remote::model::Turning::Working)
        )),
        "the head moved before the thread was there, and the thread never heard: {said:#?}"
    );
}

/// Feishu's page is made from what Feishu declares: an id written in the
/// settings file, a secret in the keyring, and a domain that starts on
/// Feishu and goes to Lark with a press -- none of which the page knows the
/// first thing about.
///
/// Broken deliberately twice. Drawing a choice nobody made as unset: the
/// domain said `Not set`. And writing a field that is not secret to the
/// keyring: the id was not in the settings file.
#[test]
fn feishu_is_set_up_from_what_it_declares() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-feishu");
    let (mut app, events) = on_the_remote_page(&scratch);
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "Feishu");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().remote.as_deref(), Some("feishu"));
    until(&mut app, &events, "the keyring to be asked", |app| {
        app.settings()
            .is_some_and(|settings| settings.reached().state == obelus_remote::State::Unready)
    });
    assert_eq!(
        app.settings()
            .and_then(|settings| settings.reached().kept.get("domain").cloned())
            .as_deref(),
        Some("feishu"),
        "a choice nobody made is not its first word"
    );
    support::check("remote_feishu_66x24", &support::render(&mut app, 66, 24));

    to_the_row(&mut app, "App ID");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "cli_a1b2c3");
    support::press(&mut app, KeyCode::Enter);
    let written = std::fs::read_to_string(scratch.join("config.toml")).expect("the file");
    assert!(written.contains("app_id = \"cli_a1b2c3\""), "{written}");

    to_the_row(&mut app, "Domain");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().remote_value("feishu", "domain"), Some("lark"));
}

/// A window set to Slack, with the reader on its list and the fake agent
/// to talk to, connected to the fake platform.
fn paired_with_an_agent(
    scratch: &support::Scratch,
) -> (App, std::sync::mpsc::Receiver<Event>, std::path::PathBuf) {
    obelus_remote::platform::connect_for_test(fake_connect);
    *FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    std::fs::write(
        scratch.join("config.toml"),
        "remote = \"slack\"\n[remotes.slack]\npeople = [{ id = \"U1\", name = \"Sunli\" }]\n",
    )
    .expect("the settings");
    a_room_kept();
    obelus_remote::secrets::write("slack", "app_token", "xapp-1-app").expect("kept");
    obelus_remote::secrets::write("slack", "bot_token", "xoxb-1-bot").expect("kept");
    let log = scratch.join("asked.log");
    let mut app = App::new(Vec::new());
    app.config_file_for_test(scratch.join("config.toml"));
    let events = support::drive(&mut app);
    app.agents_root_for_test(scratch.join("agents"));
    support::lay_out(&mut app, 76, 24);
    app.talk_to(
        "fake",
        std::path::Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
            "prompts".to_string(),
        ],
    );
    until(&mut app, &events, "the connection", |app| {
        app.remote_state_for_test() == obelus_remote::State::Connected
    });
    (app, events, log)
}

/// The platform's sink, for saying something from its side.
fn the_platform() -> std::sync::Arc<dyn obelus_sink::Sink<obelus_remote::Event>> {
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
    let scratch = support::Scratch::new("remote-fresh");
    let (mut app, events, log) = paired_with_an_agent(&scratch);
    let _ = the_platform().send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    let said = said_until(&mut app, &events, "the question in the thread", |said| {
        in_thread(said, "F1", "Allow once")
    });
    let logged = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        logged.contains("what is in here") && logged.contains("sent from Slack"),
        "the agent was not given the words, or not told where from:\n{logged}"
    );
    assert!(
        !said.iter().any(|out| matches!(
            out,
            obelus_remote::model::Out::Open { .. } | obelus_remote::model::Out::Retitle { .. }
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
    let scratch = support::Scratch::new("remote-pausing");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let _ = the_platform().send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Fresh("F1".to_string()),
        text: "/pausing".to_string(),
    });
    let said = said_until(&mut app, &events, "the words before the call", |said| {
        in_thread(said, "F1", "looking at it first")
    });
    assert!(
        said.iter().any(|out| matches!(
            out,
            obelus_remote::model::Out::Say { text, notify: false, .. } if text == "looking at it first"
        )),
        "the words went out calling the reader: {said:#?}"
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
    let scratch = support::Scratch::new("remote-outside");
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
    let scratch = support::Scratch::new("remote-answer-elsewhere");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    said_until(&mut app, &events, "the question in the thread", |said| {
        in_thread(said, "F1", "Allow once")
    });
    let _ = platform.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Thread("F1".to_string()),
        text: "1".to_string(),
    });
    said_until(&mut app, &events, "the rest of the turn", |said| {
        in_thread(said, "F1", "and I was allowed")
    });
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
    let scratch = support::Scratch::new("remote-reconnecting");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    let platform = the_platform();
    let _ = platform.send(obelus_remote::Event::Connection(
        obelus_remote::State::Connecting,
    ));
    let _ = platform.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        room: "C1".to_string(),
        at: obelus_remote::model::Where::Fresh("F1".to_string()),
        text: "what is in here".to_string(),
    });
    said_until(&mut app, &events, "the question in the thread", |said| {
        in_thread(said, "F1", "Allow once")
    });
}

/// A thread that would not open is not asked for again on every frame --
/// a platform that refused once refuses every time -- but is on the next
/// connection, which may be one the reader has mended.
///
/// Broken deliberately twice. Not keeping what would not open: it was
/// asked for again at once. And not letting go of that on a new
/// connection: the new one was never asked.
#[test]
fn a_thread_that_would_not_open_is_asked_for_on_the_next_connection() {
    let _turn = turn();
    let scratch = support::Scratch::new("remote-unopened");
    let (mut app, events, _log) = paired_with_an_agent(&scratch);
    app.new_conversation();
    app.open_a_session_for_test();
    let said = said_until(&mut app, &events, "a thread to be asked for", |said| {
        said.iter()
            .any(|out| matches!(out, obelus_remote::model::Out::Open { .. }))
    });
    let asked = said
        .iter()
        .find_map(|out| match out {
            obelus_remote::model::Out::Open { asked, .. } => Some(*asked),
            _ => None,
        })
        .expect("asked for");
    let platform = the_platform();
    let _ = platform.send(obelus_remote::Event::Unopened { asked });
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 76, 24);
    }
    assert!(
        !said_since()
            .iter()
            .any(|out| matches!(out, obelus_remote::model::Out::Open { .. })),
        "a thread that would not open was asked for again at once"
    );

    let (out, mut heard) = tokio::sync::mpsc::unbounded_channel();
    let _ = platform.send(obelus_remote::Event::Started {
        platform: "slack",
        out,
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(obelus_remote::model::Out::Open { .. }) = heard.try_recv() {
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
    let scratch = support::Scratch::new("remote-let-go");
    let (mut app, events) = connected(&scratch);
    let old = the_platform();
    to_the_row(&mut app, "Platform");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "Off");
    support::press(&mut app, KeyCode::Enter);
    support::lay_out(&mut app, 66, 20);
    choose_slack(&mut app);
    until(&mut app, &events, "the new connection", |app| {
        app.remote_state_for_test() == obelus_remote::State::Connected
    });
    let _ = old.send(obelus_remote::Event::Connection(
        obelus_remote::State::Refused,
    ));
    for _ in 0..10 {
        if let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(30)) {
            app.handle(event);
        }
        support::lay_out(&mut app, 66, 20);
    }
    assert_eq!(
        app.remote_state_for_test(),
        obelus_remote::State::Connected,
        "the old connection's word was taken as the new one's"
    );
}
