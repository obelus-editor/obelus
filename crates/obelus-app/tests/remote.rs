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

/// Somebody saying something to the fake platform.
fn somebody_says(from: &str, text: &str) {
    let faked = FAKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let faked = faked.as_ref().expect("connected");
    let _ = faked.sink.send(obelus_remote::Event::Heard {
        from: from.to_string(),
        at: obelus_remote::model::Where::Top,
        text: text.to_string(),
    });
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

/// Pairing: a code on the row, the code sent from the chat, the person on
/// the list by the name the platform gives them, and a word back to them.
/// A wrong code lets nobody in.
///
/// Broken deliberately twice. Comparing the code as it was typed: the code
/// sent in small letters without its dash let nobody in. And taking any
/// code: the stranger's wrong one was taken, and they were asked their
/// name.
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

    somebody_says("U0STRANGER", "hello?");
    somebody_says("U0STRANGER", "ABC-DEF");
    let _ = support::render(&mut app, 66, 20);
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(200)) {
        app.handle(event);
    }
    assert!(said_since().is_empty(), "a stranger was answered");

    somebody_says(
        "U04ABCDEF",
        &format!("  {}  ", code.replace('-', "").to_lowercase()),
    );
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
    assert!(
        matches!(
            said_since().as_slice(),
            [obelus_remote::model::Out::Say { to, .. }] if to == "U04ABCDEF"
        ),
        "they were not told"
    );
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
            obelus_remote::model::Out::Say { at: obelus_remote::model::Where::Thread(at), text, .. }
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
/// words from afar: the agent's log had no "sent from Slack". The header
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
            obelus_remote::model::Out::Open { asked, to, .. } if to == "U1" => Some(*asked),
            _ => None,
        })
        .expect("a thread asked for in the reader's direct message");
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

    // A number from the chat answers it, and the rest of the turn follows.
    let _ = sink.send(obelus_remote::Event::Heard {
        from: "U1".to_string(),
        at: obelus_remote::model::Where::Thread("T1".to_string()),
        text: "1".to_string(),
    });
    let said = said_until(
        &mut app,
        &events,
        "the end of the turn in the thread",
        |said| in_thread(said, "T1", "and I was allowed"),
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
