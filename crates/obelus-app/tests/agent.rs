//! Talking to an agent, through Obelus's own side of the protocol.
//!
//! The agent here is `tests/fixtures/fake-agent.sh`, a real process on the
//! other end of a real pipe that plays one conversation: it thinks, answers
//! in pieces, reads a file back through Obelus, uses a tool, and asks
//! permission before finishing. Everything below drives Obelus by keys and
//! reads the screen, so what is asserted is what a reader would see.
//!
//! It is `sh` on purpose. A fake agent written in python, node, or a second
//! Rust binary is a test that stops running on somebody else's machine.
//!
//! It is also what holds Obelus to its promises, because the protocol's
//! crate cannot: the fixture checks the handshake it was given and answers
//! to the name "Wrong Client" if the client offered to write files, and it
//! asks for a write during the turn and reports back whether it was
//! refused. Both land in `a_whole_turn_of_conversation`. It does the same
//! for the settings -- the boolean one is only offered to a client that
//! said in the handshake that it can draw a switch.
//!
//! **Pressing install still runs `npm`, so a test must not press it.** The
//! root has a hook (`App::agents_root_for_test`), which is what lets a test
//! write the record a finished install would leave and then drive
//! `Event::Installed`. Everything about talking to one goes through
//! `App::talk_to`, which takes the command directly and needs no registry,
//! no install and no network.

mod support;

use std::{
    path::Path,
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use crossterm::event::KeyCode;
use obelus_app::{app::App, event::Event};
use obelus_component::card::{Card, On};

/// The screen these tests use.
const WIDTH: u16 = 76;
/// Tall enough for a whole turn's transcript to be on screen at once.
const HEIGHT: u16 = 24;

/// How long to wait for the agent to say something.
///
/// Generous: it is a shell script starting up, and a test that fails because
/// a machine was busy is a test nobody trusts.
///
/// And what it has to clear is not how long a script takes to start. A
/// hundred and eighteen tests in this binary run at once, most of them
/// starting a shell script of their own, on a runner with two cores -- so
/// a test can be starved for as long as its siblings take. Measured on
/// the arm runner: the binary's own wall time was 63.5 seconds and the
/// one that failed spent 60 of them waiting, which is nearly the whole
/// run. So the deadline has to be comfortably past what the *binary*
/// takes, not past what a shell does.
///
/// Ten seconds was the first try and sixty the second, each set from the
/// wrong measurement. It costs nothing where nothing is wrong: it is a
/// deadline, not a sleep.
///
/// Where something *is* wrong it is three minutes a test, which is most of
/// the time spent watching a test fail on purpose -- reproducing a fault,
/// or breaking the path a test covers to see it go red. `OBELUS_PATIENCE`
/// is that many seconds instead, for a run on a machine that is not
/// starved; unset, or not a number, it is the runner's three minutes.
fn patience() -> Duration {
    std::env::var("OBELUS_PATIENCE")
        .ok()
        .and_then(|seconds| seconds.trim().parse().ok())
        .map_or(Duration::from_secs(180), Duration::from_secs)
}

/// Where these tests let Obelus keep things about agents.
///
/// Its own directory, because a test that installs one -- or writes the
/// record an install leaves -- into the reader's real data directory is a
/// test with a side effect on the machine it ran on.
fn agents_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("obelus-agent-tests-{}", std::process::id()))
}

/// The same, for one test alone.
///
/// Tests in one file share a process, and an install's record is written
/// under the agent's own name: two tests installing the fixture would
/// write the same file. A test that is about that file, or about a log the
/// fixture keeps beside it, needs one nobody else has.
fn agents_root_for(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("obelus-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// An application with the loop's channel, and the channel.
fn wired() -> (App, Receiver<Event>) {
    let (sender, events) = channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(agents_root());
    support::lay_out(&mut app, WIDTH, HEIGHT);
    (app, events)
}

/// Starts the fake agent and opens the conversation on it.
fn talking() -> (App, Receiver<Event>) {
    playing(&[])
}

/// The same, with the agent told to play a different shape.
fn playing(how: &[&str]) -> (App, Receiver<Event>) {
    let (mut app, events) = wired();
    let mut arguments = vec!["tests/fixtures/fake-agent.sh".to_string()];
    arguments.extend(how.iter().map(|word| (*word).to_string()));
    app.talk_to("fake", Path::new("sh"), &arguments);
    app.new_conversation();
    // Now rather than on the first frame, which is what would ask for it
    // otherwise: what these tests are about is what happens in a
    // conversation that is running, from the first thing they draw.
    app.open_a_session_for_test();
    (app, events)
}

/// Goes from the notes to the conversation about the note under the reader,
/// and starts it.
///
/// The key opens the view and the next frame asks for its session; these
/// tests are about what happens in a conversation that is running, so they
/// ask now, the same way `playing` does for the loose conversation.
fn talk_about_the_note(app: &mut App) {
    support::press_alt(app, 'a');
    // Only where the key was honoured: a note another Obelus has open opens
    // nothing, and asking for a session there would ask for one in whatever
    // conversation the reader was in before.
    if app.chat().is_some() {
        app.open_a_session_for_test();
    }
}

/// Whether the transcript has these words in it anywhere.
fn said_in_transcript(app: &App, words: &str) -> bool {
    app.chat().is_some_and(|chat| {
        chat.rows(WIDTH)
            .iter()
            .any(|row| row.text().contains(words))
    })
}

/// The rows of the list of conversations that are conversations, past the
/// one that starts a new one -- which every such list opens with.
fn listed_conversations(app: &App) -> Vec<&obelus_component::picker::PickerItem> {
    app.picker()
        .expect("the list of conversations")
        .matches()
        .filter(|item| {
            matches!(
                item.value,
                obelus_component::picker::PickerValue::Conversation(_)
            )
        })
        .collect()
}

/// Writes a conversation into the project's table, as one that was had and
/// left.
///
/// A conversation about nothing in particular, which is what the key opens
/// and what the list is mostly of. `last` is when something was last said
/// in it, which is what the list is ordered by.
fn remember_a_conversation(
    scratch: &support::Scratch,
    agent: &str,
    session: &str,
    title: &str,
    last: Option<i64>,
) {
    obelus_agent::acp::sessions::change(scratch.path(), None, |remembered| {
        remembered.put(
            &obelus_agent::chats::ChatId::Loose(session.to_string()),
            agent,
            scratch.path(),
            obelus_agent::acp::sessions::Kept {
                session: session.to_string(),
                title: Some(title.to_string()),
                told: None,
                introduced: false,
                last,
            },
        );
    });
}

/// Says what a watcher says when a claim appears, goes, or is closed by the
/// process that was holding it.
///
/// A real one hears all three: a file created, a file removed, and -- the
/// one that matters -- a file closed by a writer, which is what the kernel
/// does to the files of an Obelus it is killing. These tests take and drop
/// claims by hand, so they say it by hand; without it they would be asking
/// whether Obelus goes and looks of its own accord, which is the thing it
/// stopped doing.
fn the_claims_changed(app: &mut App, root: &Path, which: &obelus_agent::chats::ChatId) {
    let path = obelus_agent::chats::directory(root)
        .expect("somewhere to keep claims")
        .join(which.file_name());
    app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path,
    }));
}

/// Writes down a conversation about one of the project's notes.
fn remember_a_note_conversation(scratch: &support::Scratch, note: &str, session: &str) {
    remember_telling(scratch, note, session, None);
}

/// The same, with the agent already told what the note said.
fn remember_telling(scratch: &support::Scratch, note: &str, session: &str, told: Option<&str>) {
    obelus_agent::acp::sessions::change(scratch.path(), None, |remembered| {
        remembered.put(
            &obelus_agent::chats::ChatId::Note(
                obelus_git::todo::NoteId::read(note).expect("a name"),
            ),
            "fake",
            scratch.path(),
            obelus_agent::acp::sessions::Kept {
                session: session.to_string(),
                title: None,
                told: told.map(str::to_string),
                introduced: false,
                last: Some(1_700_000_000),
            },
        );
    });
}

/// The command the fake agent asks to have run, in the words the shell
/// that runs it takes.
///
/// Said twice -- here and in the fake agent, beside its `cygpath` -- and
/// it has to be: one of them is a shell script and the other is this, and
/// what the row shows is the command line as it was sent. `cmd` knows
/// neither `;` nor a quoted argument surviving `/C`, so that side chains
/// with `&`, waits with `ping` and prints with `set /p`. No space before
/// that `&`: `set /p` prints everything up to the separator, so one there
/// is a space on the end of the output, and the page came back saying
/// `obelus-ran-this  and ended 3`.
fn ran_command() -> &'static str {
    match cfg!(windows) {
        true => "ping -n 2 127.0.0.1 >nul & <nul set /p =obelus-ran-this& exit 3",
        false => "sleep 0.3; printf %s obelus-ran-this; exit 3",
    }
}

/// Handles events until the application satisfies `until`, or gives up.
///
/// Draws between events, because a frame is where Obelus settles what it is
/// showing -- and because a test that only handles events would not notice a
/// view that cannot draw what arrived.
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

/// Waits for the fake agent to have been sent a request, handling whatever
/// arrives meanwhile.
///
/// Not [`pump`], which waits for an event before it looks again: some of
/// what Obelus owes an agent is a request whose answer changes nothing it
/// shows -- a session let go -- so nothing arrives to wake it, and the
/// request is only visible in what the agent wrote down.
fn asked(app: &mut App, events: &Receiver<Event>, log: &Path, request: &str) {
    let deadline = Instant::now() + patience();
    loop {
        if std::fs::read_to_string(log).is_ok_and(|said| said.contains(request)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the agent was never asked `{request}`:\n{}",
            std::fs::read_to_string(log).unwrap_or_default()
        );
        if let Ok(event) = events.recv_timeout(Duration::from_millis(20)) {
            app.handle(event);
        }
        support::lay_out(app, WIDTH, HEIGHT);
    }
}

/// Handles whatever arrives for a moment, and does not mind if nothing
/// does.
///
/// The counterpart of [`pump`], for the cases where what is being tested is
/// that something does *not* happen: there is no state to wait for, so what
/// the test needs is the chance for the thing to go wrong.
fn settle(app: &mut App, events: &Receiver<Event>, how_long: Duration) {
    let deadline = Instant::now() + how_long;
    while let Ok(event) = events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        app.handle(event);
        support::lay_out(app, WIDTH, HEIGHT);
    }
}

/// The rows of a dump's text block, one string per screen row.
///
/// The block starts with a newline, so `lines()` on its own is one out.
fn rows(dump: &str) -> Vec<&str> {
    support::text_block(dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect()
}

/// Which screen row a dump's text is on, by its own row numbers.
fn row_of(dump: &str, needle: &str) -> u16 {
    rows(dump)
        .into_iter()
        .find(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no {needle} on screen:\n{dump}"))
        .split('|')
        .next()
        .and_then(|number| number.trim().parse().ok())
        .expect("a row number")
}

/// What is painted behind the row a needle is on.
///
/// Read out of the dump's own legend rather than compared against a colour
/// written down here: a test that names `#27272a` is a test that fails when
/// somebody picks a better grey.
fn behind(dump: &str, needle: &str) -> String {
    let at = usize::from(row_of(dump, needle));
    let letter = support::style_block(dump)
        .lines()
        .find(|row| row.trim_start().starts_with(&format!("{at}|")))
        .and_then(|row| row.split_once('|'))
        .and_then(|(_, marks)| marks.chars().next())
        .unwrap_or_else(|| panic!("no styles for row {at}:\n{dump}"));
    support::legend_block(dump)
        .lines()
        .find(|line| line.starts_with(&format!("{letter} ")))
        .and_then(|line| line.split_once("bg="))
        .map(|(_, colour)| colour.trim().to_string())
        .unwrap_or_else(|| panic!("no colour for {letter}:\n{dump}"))
}

/// The transcript's text, as it is on screen.
fn screen(app: &mut App) -> String {
    let dump = support::render(app, WIDTH, HEIGHT);
    support::text_block(&dump).to_string()
}

/// What is painted behind a run of words on the row they are on.
///
/// The whole legend entry, and one per distinct style: a test about a hold
/// wants to know that every cell of it is marked the same way, and what
/// that mark is.
fn behind_words(dump: &str, needle: &str) -> Vec<String> {
    let at = usize::from(row_of(dump, needle));
    let row = rows(dump)
        .into_iter()
        .find(|row| row.contains(needle))
        .expect("the row");
    let from = support::column_of(row, needle);
    let wide = obelus_text::text_width(needle);
    let marks = support::style_block(dump)
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{at}|")))
        .and_then(|line| line.split_once('|'))
        .map(|(_, marks)| marks.to_string())
        .unwrap_or_else(|| panic!("no styles for row {at}:\n{dump}"));
    let mut seen: Vec<String> = Vec::new();
    for letter in marks.chars().skip(from).take(wide) {
        let entry = support::legend_of(dump, letter);
        let ground = entry
            .split_once("bg=")
            .map(|(_, colour)| colour.trim().to_string())
            .unwrap_or_default();
        if !seen.contains(&ground) {
            seen.push(ground);
        }
    }
    seen
}

/// Before the agent has said who it is, it is called what the registry
/// calls it, and not by its id.
///
/// The id is a word for the settings file. A header that reads
/// `claude-acp` until the handshake and `Claude Agent` after it changes
/// under the reader for no reason they can see.
///
/// Broken deliberately by answering with the id in `App::agent_name`.
#[test]
fn an_agent_not_yet_started_is_called_what_the_registry_calls_it() {
    let (mut app, _events) = wired();
    app.configure(
        obelus_config::Config {
            agent: Some("claude-acp".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "claude-acp".to_string(),
            name: "Claude Agent".to_string(),
            version: "0.84.0".to_string(),
            description: "Claude, over the protocol".to_string(),
            authors: vec!["Somebody".to_string()],
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "@agentclientprotocol/claude-agent-acp@0.84.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));
    assert_eq!(app.agent_name(), Some("Claude Agent"));
}

/// One whole turn: the handshake, a prompt, what comes back while it works,
/// a file read through Obelus, a permission request, and the end.
#[test]
fn a_whole_turn_of_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // What the agent calls itself, which Obelus only knows because it asked
    // -- its title, and not the name it gives for programs or its version.
    // Broken by taking `info.name` in the handshake instead.
    assert_eq!(app.agent_name(), Some("Fake Agent"));

    support::type_text(&mut app, "what is this file");
    // What is being written is in the box, near the foot of the region,
    // with the caret after it.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let box_row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("what is this file"))
        .expect("the box")
        .to_string();
    let row: u16 = box_row
        .split('|')
        .next()
        .expect("a row number")
        .trim()
        .parse()
        .expect("a row number");
    assert!(
        row > HEIGHT / 2,
        "the box is not at the foot of the region:\n{dump}"
    );
    assert_eq!(
        support::cursor_line(&dump),
        &format!("{},{row}", 4 + "what is this file".len()),
        "the caret is not after what was written:\n{dump}"
    );

    support::press(&mut app, KeyCode::Enter);
    // Sent, so the row is empty again and the transcript has the question.
    assert!(screen(&mut app).contains("what is this file"));

    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    let text = screen(&mut app);
    // Everything the agent said while it worked, in the order it said it:
    // its thinking apart from its answer, its answer joined up out of the
    // pieces it arrived in, and the tool it used.
    assert!(text.contains("working it out"), "no thinking:\n{text}");
    assert!(
        text.contains("it is a rust file"),
        "the chunks were not joined up:\n{text}"
    );
    assert!(text.contains("Read the file"), "no tool call:\n{text}");
    // Including the one it is asking about, which is a row like any other
    // -- waiting, which is what says the question is about it.
    assert!(
        text.contains("Run the tests"),
        "what it is asking about is not in the transcript:\n{text}"
    );
    // And which file it was in, written the way a reader writes a path --
    // relative to the tree Obelus was opened on. This is what makes a tool
    // call somewhere to go rather than something to read about.
    assert!(
        text.contains("tests/fixtures/many_lines.rs:7"),
        "the tool call does not say where it was:\n{text}"
    );
    // And the file it asked Obelus to read, which Obelus answered from the
    // tree it was started on.
    assert!(
        text.contains("saying this is what Obelus handed over"),
        "the file was not read back:\n{text}"
    );
    // And the one it asked Obelus to *write*, which Obelus refuses: it said
    // so in the handshake, and the agent reports back what it was told.
    assert!(
        text.contains("and it refused to write"),
        "Obelus wrote a file for an agent:\n{text}"
    );
    // The question is a list, which is what every choice in Obelus is.
    assert!(text.contains("Allow once"), "no options:\n{text}");
    assert!(text.contains("Reject"), "no options:\n{text}");

    // Allow it, and the turn finishes.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("and I was allowed"),
        "the answer did not reach the agent:\n{text}"
    );
    // The tool call is one row that changed, not two rows.
    assert_eq!(
        text.matches("Read the file").count(),
        1,
        "the tool call was listed twice:\n{text}"
    );
    assert!(!app.is_asking_permission());

    // And the whole screen, once: the header saying who is being talked to,
    // the rule under it, who said what in which colour, and the row being
    // typed on the status bar. The text assertions above say what is there;
    // this says what it looks like.
    app.phase_for_test(0);
    support::check(
        &format!("agent_{WIDTH}x{HEIGHT}"),
        &support::render(&mut app, WIDTH, HEIGHT),
    );
}

/// Walking away from the question answers it.
///
/// An agent whose permission request is never answered waits for ever, so
/// escape on that list is not "no answer" -- it is the protocol's own
/// cancelled outcome.
#[test]
fn escape_on_the_question_tells_the_agent_it_was_not_answered() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "go on");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );

    support::press(&mut app, KeyCode::Esc);
    assert!(!app.is_asking_permission());
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("and I was refused"),
        "the agent was not told:\n{text}"
    );
}

/// Escape stops what is happening, and never closes the conversation.
///
/// Mid-turn it is the interrupt; with nothing in flight it does nothing at
/// all, because a conversation is a document and escape leaves whatever is
/// *over* the document being read. It used to close the view, which is what
/// made it the one place in Obelus where escape threw something away.
#[test]
fn escape_stops_the_turn_and_never_closes_the_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "remember this");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "An answer", |app| {
        app.chat()
            .is_some_and(|chat| !chat.rows(60).is_empty() && app.is_asking_permission())
    });

    // The question first, because a list is open over the conversation.
    support::press(&mut app, KeyCode::Esc);
    // And now it is finishing the turn, so escape is still "stop what is
    // happening" rather than "close this".
    assert!(app.chat().is_some(), "escape closed it mid-turn");
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // And with nothing in flight it does nothing at all: a conversation is
    // a document, and escape leaves whatever is *over* the document being
    // read. There is nothing over this one.
    support::press(&mut app, KeyCode::Esc);
    assert!(app.chat().is_some(), "escape closed a document");

    support::press_function(&mut app, 4);
    let text = screen(&mut app);
    assert!(
        text.contains("remember this"),
        "reopening lost the conversation:\n{text}"
    );
}

/// Escape stops an agent that is working, rather than closing the view.
///
/// One key, and what it means is the same everywhere in Obelus: stop what is
/// happening. What is happening while an agent is thinking is the agent.
#[test]
fn escape_stops_an_agent_that_is_working() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "think about it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });

    support::press(&mut app, KeyCode::Esc);
    assert!(app.chat().is_some(), "escape closed the view instead");
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Stopped"),
        "it did not say it stopped:\n{text}"
    );
}

/// What the reader says into a running turn waits for it, and goes when it
/// ends.
///
/// A conversation takes one prompt turn at a time. `session/cancel` names a
/// session and not a turn, and the answer to `session/prompt` says "the turn
/// is over" with nothing on it saying *which* turn -- so Obelus sending a
/// second prompt into a running one cannot tell the two answers apart. It
/// did not: the first one home put the conversation back to resting, and
/// the turn still working went on with no spinner, no `thinking...`, and no
/// key that would stop it. The reader was told nothing was happening while
/// their agent read the repository.
///
/// So the second thing they say waits. Held and not dropped, and said as
/// soon as there is a turn free for it.
///
/// Broken deliberately by taking the `thinking || queued` check out of
/// `send_to_agent`, which sends it straight away: the agent answers
/// `/blocks` while `/forever` is still in flight, and `blocks=` is on the
/// screen before anything has ended.
#[test]
fn what_the_reader_says_into_a_running_turn_waits_for_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A turn that does not end on its own, so there is no race about when
    // the second message is typed.
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });

    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.chat().map(|chat| chat.unsent()),
        Some(vec![vec![obelus_component::composer::Part::Words(
            "/blocks".to_string()
        )]]),
        "what the reader typed was not held back"
    );
    // Still working on the first one, and still saying so -- which is the
    // whole of what went wrong.
    assert_eq!(app.talking(), obelus_agent::Talking::Thinking);
    let text = screen(&mut app);
    assert!(
        !text.contains("blocks="),
        "the second message went into the running turn:\n{text}"
    );
    // And the reader can see it, in their own words, where their words go.
    assert!(
        text.contains("/blocks"),
        "what is waiting is not on the page:\n{text}"
    );
}

/// A turn the reader stopped puts what they said into it back in the box.
///
/// It let them go, as the next turn, the moment the stop arrived: so one
/// press of escape stopped the agent and started it again, the mark went on
/// turning under `Stopped` -- the words that went were above that line --
/// and the reader pressed escape a second time and stopped their own words.
/// Stop means stop, and enter is the key that says something.
///
/// All of them, in the order they were said, joined the way they would have
/// gone; and in front of what was already in the box, which is the reader's
/// too.
///
/// Broken deliberately by taking the `take_back_waiting` out of
/// `interrupt_agent`: the turn's end lets them go, and the box is left with
/// only what was being typed. And by dropping the arm in `Chat::handle_key`
/// that empties the box: the second escape leaves all of it there.
#[test]
fn a_turn_the_reader_stopped_puts_what_was_waiting_back_in_the_box() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "and then");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "half typed");

    support::press(&mut app, KeyCode::Esc);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Stopped"),
        "it did not say it stopped:\n{text}"
    );
    assert_eq!(
        app.chat().map(|chat| chat.writing().text()),
        Some("/blocks\n\nand then\n\nhalf typed".to_string()),
        "what was waiting is not back in the box, in order"
    );
    assert_eq!(
        app.chat().map(|chat| chat.unsent()),
        Some(Vec::new()),
        "it is in the box and still waiting on the page as well"
    );
    assert!(
        !text.contains("blocks="),
        "what was waiting went to the agent:\n{text}"
    );

    // And the next press takes all of it back, which is what a reader who
    // stopped the agent to stop saying those things wants.
    support::press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.chat().map(|chat| chat.writing().text()),
        Some(String::new()),
        "the second escape left what was waiting in the box"
    );
}

/// Several things said into one running turn arrive as one prompt.
///
/// Three things typed into a turn are one thing the reader is saying -- fix
/// the tests, and the lint, and then commit -- and an agent handed only the
/// first of them answers a question it has not been asked the whole of. It
/// used to go one per turn: the second waited on the answer to the first,
/// which the agent wrote without ever seeing it.
///
/// Asked of the agent rather than of the page, because the page shows the
/// reader's rows either way and what is in question is what was *sent*.
/// The fake agent reads a prompt by what is in it, and `/forever` is the
/// arm it tries before `/blocks`: joined, the prompt reaches the arm named
/// by its second half, and `blocks=` never arrives.
///
/// Behind `/run`, which ends on its own once its command has: a turn the
/// reader stops puts what was waiting back in the box instead.
///
/// Broken deliberately by sending only `unsent()[0]` from
/// `say_what_was_waiting`, which leaves `/forever` behind and answers
/// `/blocks` on its own.
#[test]
fn what_was_waiting_goes_as_one_prompt() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/run");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.chat().map(|chat| chat.unsent().len()),
        Some(2),
        "both should be waiting"
    );

    // On what goes when the turn ends, rather than on anything the turn
    // shows: the command's output is Obelus's own and is on the page while
    // the agent is still reading it, which on Windows is long enough to
    // outlast the wait below.
    pump(&mut app, &events, "the first turn to end", |app| {
        app.chat().is_some_and(|chat| chat.unsent().is_empty())
    });
    // Long enough for an answer to either of them to have arrived.
    settle(&mut app, &events, Duration::from_millis(500));
    let text = screen(&mut app);
    assert!(
        !text.contains("blocks="),
        "the first went on its own, without the second:\n{text}"
    );
    // And something did go: the second half of the one prompt is a turn
    // that does not end.
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Thinking,
        "nothing was sent at all:\n{text}"
    );
}

/// Enter on something the reader said that has not gone takes it back.
///
/// The one row in a transcript whose key hands something back rather than
/// opening it. What is waiting is the reader's, and it is on the page in
/// their own words precisely so that there is somewhere to stand to change
/// their mind -- a count on the status row said how many were waiting and
/// never which, and offered no way to reach one.
///
/// The words go back in the box rather than away, because taking something
/// back is almost always meaning to say it again differently.
///
/// Broken deliberately twice: dropping the `(Some(which), ..)` arm from
/// the enter match in `Chat::handle_key` leaves the row doing what it used
/// to do, which is nothing; and taking `unsent` out of `Row::acts` leaves
/// the key working with nothing on the row saying so, which is a key
/// nobody finds.
#[test]
fn enter_on_something_not_yet_sent_takes_it_back() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.chat().map(|chat| chat.unsent().len()), Some(1));

    // Up to the row, which is the last thing said and so the first one the
    // cursor reaches.
    support::press(&mut app, KeyCode::Up);
    // And the row says what the key will do, which is the only way to find
    // out: this is the one row in a transcript that hands something back.
    let text = screen(&mut app);
    assert!(
        text.contains("Enter takes it back"),
        "the row says nothing about the key standing on it:\n{text}"
    );
    support::press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.chat().map(|chat| chat.unsent()),
        Some(Vec::new()),
        "it is still waiting to be said"
    );
    assert_eq!(
        app.chat().map(|chat| chat.writing().text()),
        Some("/blocks".to_string()),
        "the words were not handed back to the box"
    );
    // And it is not on the page any more: a row saying they said it, under
    // a box holding the same words, is the same thing twice.
    let text = screen(&mut app);
    assert_eq!(
        text.matches("/blocks").count(),
        1,
        "the row it was taken back from is still there:\n{text}"
    );
}

/// Something waiting is dim on every row of it, not only the first.
///
/// `Row::unsent` is on the first row alone, because that is where the key
/// that takes it back stands -- and the ink was read off it, so a waiting
/// message long enough to wrap was dim for one row and in the reader's
/// colour for the rest, which reads as two things said.
///
/// Broken deliberately by reading the ink off `row.unsent` again: the
/// second row comes out in the reader's colour.
#[test]
fn something_waiting_is_dim_on_every_row_of_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    support::type_text(
        &mut app,
        "/blocks waiting words that run on long enough to need a second row of the \
         transcript, and the end of them",
    );
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.chat().map(|chat| chat.unsent().len()), Some(1));

    let cells = support::cells_of(&mut app, WIDTH, HEIGHT);
    let ink_of = |needle: &str| {
        (0..HEIGHT)
            .find_map(|y| {
                let row: String = (0..WIDTH).map(|x| cells[(x, y)].symbol()).collect();
                let from = row.find(needle)?;
                let x = u16::try_from(row[..from].chars().count()).ok()?;
                Some(cells[(x, y)].fg)
            })
            .unwrap_or_else(|| panic!("{needle:?} is not on the screen"))
    };
    let first = ink_of("/blocks waiting");
    let rest = ink_of("end of them");
    assert_eq!(
        first, rest,
        "one thing waiting is drawn in two colours: {first:?} and then {rest:?}"
    );
}

/// A call still running when the reader stops the turn stops saying so.
///
/// A tool call's state is the agent's, and an agent that is told to stop is
/// asked to send the updates it owes on the way out. One that never sees
/// the cancellation sends none, and its calls sit at `in_progress` for
/// ever: Obelus tells the reader the conversation is resting and draws, two
/// rows above, a call that says it is running. The mark on it does not even
/// turn, because nothing wakes the screen for a conversation Obelus
/// believes is idle -- a spinner frozen mid-turn, which reads worse than no
/// spinner at all.
///
/// `cancelled` is the protocol's own word for a call stopped before it
/// finished, so this is Obelus writing down what the agent would have said
/// rather than inventing a state.
///
/// Broken deliberately by taking `stop_the_calls` out of `interrupt_agent`,
/// which leaves the row at `in_progress` under a conversation that has been
/// told it is over.
#[test]
fn a_call_still_running_when_the_reader_stops_says_it_was_stopped() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A call of the agent's own, which is the case this is about: a call
    // Obelus is running the command for has a state of its own from the
    // runner, and one the agent runs itself has only what the agent last
    // said about it.
    support::type_text(&mut app, "read something for me");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the call to start", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.state.as_deref() == Some("in_progress"))
        })
    });

    support::press(&mut app, KeyCode::Esc);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let chat = app.chat().expect("the conversation");
    let rows = chat.rows(WIDTH);
    let still = rows
        .iter()
        .filter(|row| matches!(row.state.as_deref(), Some("pending" | "in_progress")))
        .count();
    assert_eq!(
        still, 0,
        "a call says it is running under a conversation that has stopped"
    );
    assert!(
        rows.iter()
            .any(|row| row.state.as_deref() == Some("cancelled")),
        "the call that was running says nothing about having been stopped"
    );
}

/// A title too long for the row does not take the row's own marks with it.
///
/// An agent titles a call with its own text, and for a command it ran that
/// is the whole command line -- a hundred columns of `grep`. Obelus wrote
/// everything the row says about itself from wherever those words happened
/// to stop, so a title that reached the edge of the screen left no room for
/// any of it: no mark saying the row can be opened, nothing saying how the
/// call went, and for a call that rewrote a file nothing saying how much it
/// changed. And the words simply ran out at the last column, which reads as
/// a line Obelus lost the end of rather than one with more behind it.
///
/// Broken deliberately by drawing the words to `words_end(area) + 1` again
/// instead of taking the tail off first: the ellipsis goes, and so does the
/// mark that says the call is done.
#[test]
fn a_title_longer_than_the_row_keeps_the_row_its_own_end() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/longtitle");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the call", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("grep -rn"))
        })
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let row = screen
        .iter()
        .find(|row| row.contains("grep -rn"))
        .unwrap_or_else(|| panic!("no call on screen:\n{dump}"));
    // Cut where it stops, rather than running out at the edge.
    assert!(
        row.contains('\u{2026}'),
        "the title was cut off with nothing saying so:\n{dump}"
    );
    // And the row still says how the call went: the word a finished call
    // wears without the glyphs, which is the thing the title used to push
    // off the screen.
    assert!(
        row.contains("Done"),
        "the row lost its own account of the call:\n{dump}"
    );
}

/// The answer to a turn Obelus cancelled does not end the turn after it.
///
/// An agent that is told to stop does what the protocol asks: it answers
/// the prompt it was working on with `cancelled`. That answer arrives after
/// Obelus has already said the turn is over and started the next one -- and
/// it says "the turn is over" with nothing on it saying which turn, because
/// the protocol puts no turn on it.
///
/// So Obelus counts its own turns: the number goes out with the prompt,
/// comes back on the answer, and an answer about a turn that is not the one
/// running is dropped by the handle. Without that the stale answer ends the
/// turn that replaced it and the conversation goes to resting with an agent
/// still working in it -- the whole bug this queue was written for,
/// arriving by the one door the queue leaves open, which is the reader
/// saying something in the moment between pressing escape and the stop
/// landing: it waits, and goes when the stop lands.
///
/// Broken deliberately by dropping the `open.turn != Some(turn)` guard in
/// `Talk::on`, so that any answer ends whatever is running: this reads
/// `Ready` with `/forever` in flight.
#[test]
fn the_answer_to_a_cancelled_turn_does_not_end_the_next_one() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    // Stop the first turn, and say something before the stop has landed
    // -- a second turn that also does not end on its own, so that "is it
    // still thinking" is a question about the second one and nothing else.
    // It waits, and goes out when the stop lands; the agent answers the
    // cancelled prompt with `cancelled` somewhere behind it.
    support::press(&mut app, KeyCode::Esc);
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the first turn to end", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("Stopped"))
        })
    });
    assert_eq!(
        app.chat().map(|chat| chat.unsent()),
        Some(Vec::new()),
        "what was waiting did not go when the cancelled turn ended"
    );
    // Long enough for the agent's own answer to the cancelled prompt to
    // arrive and be ignored.
    settle(&mut app, &events, Duration::from_millis(500));
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Thinking,
        "a turn that is running was reported as over:\n{}",
        screen(&mut app)
    );
}

/// With no agent chosen there is nothing to talk to, and the view says so
/// rather than being empty.
#[test]
fn the_view_says_when_nobody_is_chosen() {
    let (mut app, _events) = wired();
    app.new_conversation();
    let text = screen(&mut app);
    assert!(
        text.contains("No agent is active"),
        "it did not say why it is empty:\n{text}"
    );
    assert_eq!(app.talking(), obelus_agent::Talking::Nobody);
}

/// The box holds more than one line, and enter sends the lot.
///
/// `shift+enter` is what a reader reaches for and it only arrives where the
/// terminal implements the kitty keyboard protocol; `alt+enter` is the
/// escape prefix, which is as old as terminals and always arrives. Both
/// break the line.
#[test]
fn the_box_takes_a_paragraph() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    support::type_text(&mut app, "first");
    support::press_shift(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "second");
    support::press_alt_key(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "third");

    // All three lines are in the box -- asserted on the box itself rather
    // than on the screen, because a line that was *sent* instead of broken
    // is also somewhere on the screen: in the transcript.
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        "first\nsecond\nthird",
        "the box does not hold the paragraph"
    );

    // And three rows of it are drawn, so the transcript gave up the room.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let text = support::text_block(&dump);
    for line in ["first", "second", "third"] {
        assert!(text.contains(line), "{line} is not drawn:\n{dump}");
    }

    // The caret is on the last row of the box, after what was typed.
    let (column, row) = support::cursor_line(&dump)
        .split_once(',')
        .expect("a caret");
    let caret_row: usize = row.parse().expect("a row");
    assert!(
        rows(&dump)[caret_row].contains("third"),
        "the caret is not on the row being typed:\n{dump}"
    );
    assert_eq!(column, (4 + "third".len()).to_string());

    // And enter sends all three lines as one message.
    support::press(&mut app, KeyCode::Enter);
    let text = screen(&mut app);
    assert!(
        text.contains("first") && text.contains("second") && text.contains("third"),
        "the message did not reach the transcript:\n{text}"
    );
    assert!(
        app.chat().expect("the chat").writing().text().is_empty(),
        "the box still holds what was sent"
    );
}

/// Shift and tab walk the ways of working the agent offers, and the
/// conversation's own status row says which one is on.
#[test]
fn shift_and_tab_walk_the_agents_modes() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the modes", |app| {
        app.agent_mode().is_some()
    });
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("ask first")
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    // The status row, which while a conversation is showing is the
    // conversation's: the foot of the screen, and the only one.
    let screen = rows(&dump);
    let status = screen[screen.len() - 1].to_string();
    assert!(
        status.contains("ask first"),
        "the mode is not on the status row:\n{dump}"
    );

    support::press_shift(&mut app, KeyCode::BackTab);
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("write code"),
        "shift+tab did not walk the modes"
    );
    // And round, because there are two of them.
    support::press_shift(&mut app, KeyCode::BackTab);
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("ask first")
    );
}

/// A message whose first character is a slash is a command: the agent's own
/// list of them is offered, tab fills one in, and what goes out is the line
/// as typed. Anything else is a message and nothing is offered.
#[test]
fn a_slash_is_a_command_and_anything_else_is_a_message() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the commands", |app| {
        !app.agent_orders().is_empty()
    });

    // An ordinary message offers nothing, even with a slash inside it.
    support::type_text(&mut app, "what is /usr for");
    let text = screen(&mut app);
    assert!(
        !text.contains("Summarise the conversation"),
        "a slash inside a message was read as a command:\n{text}"
    );
    for _ in 0.."what is /usr for".len() {
        support::press(&mut app, KeyCode::Backspace);
    }

    // A slash first offers what the agent takes, narrowed as it is typed.
    support::type_text(&mut app, "/c");
    let text = screen(&mut app);
    assert!(text.contains("/compact"), "no commands offered:\n{text}");
    assert!(text.contains("/cost"), "not all of them offered:\n{text}");
    support::type_text(&mut app, "omp");
    let text = screen(&mut app);
    assert!(text.contains("/compact"), "the list went away:\n{text}");
    assert!(!text.contains("/cost"), "the list did not narrow:\n{text}");

    // The list is the ordinary compact one, so what it shows is what
    // every list shows: the row, what it is, and what it takes.
    assert!(
        text.contains("Summarise the conversation"),
        "the rows do not say what the commands do:\n{text}"
    );

    // And it is chosen from the way every list is chosen from: the arrows
    // move, tab and enter take the row that is on.
    for _ in 0.."omp".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    // A frame, because the list follows what is being typed and it is a
    // frame that tells it what that is now.
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    let first = app
        .slash()
        .and_then(|slash| slash.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is chosen");
    support::press(&mut app, KeyCode::Down);
    let second = app
        .slash()
        .and_then(|slash| slash.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is chosen");
    assert_ne!(first, second, "the arrows did not move the selection");
    // Enter, which is what every completion in Obelus is taken with.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        format!("{second} "),
        "enter did not take the row that was on"
    );
    // The blank after the name settles it, so the list has nothing left to
    // offer and is gone.
    assert!(
        app.slash().is_none(),
        "the list stayed after the name was settled"
    );

    // Rubbing the slash out closes it too: without one, what is being
    // written is a message.
    while !app.chat().expect("the chat").writing().text().is_empty() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "/co");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.slash().is_some(), "no list while a name is typed");
    // Above the box, not over it. The list is a list of what is being
    // typed, so the one row it must not cover is the row being typed on --
    // which is the row the caret is on.
    let caret = support::cursor_line(&dump);
    let typing: u16 = caret
        .split(',')
        .nth(1)
        .and_then(|row| row.parse().ok())
        .expect("the caret's row");
    assert!(
        rows(&dump)[usize::from(typing)].contains("/co"),
        "what is being typed is covered:\n{dump}"
    );
    for word in ["/compact", "/cost"] {
        assert!(
            row_of(&dump, word) < typing,
            "{word} is drawn on or below the box:\n{dump}"
        );
    }
    // And the rule over the box is still there, so the list has one under
    // it as well as over it rather than sitting straight on the box.
    let over = rows(&dump)[usize::from(typing) - 1]
        .split_once('|')
        .expect("a row")
        .1;
    assert!(
        over.chars().all(|drawn| drawn == '\u{2500}'),
        "the rule over the box is gone:\n{dump}"
    );
    for _ in 0.."/co".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.slash().is_none(), "the list outlived the slash");

    // A name that matches no command is not a list either -- and enter has
    // to reach the box: a list of nothing that took the key would leave the
    // reader unable to send what they had typed.
    support::type_text(&mut app, "/c");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.slash().is_some(), "no list while a name is typed");
    support::type_text(&mut app, "zzz");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.slash().is_none(),
        "a list of nothing is still a list:\n{}",
        screen(&mut app)
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the message to go", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(text.contains("ran czzz"), "enter did not send it:\n{text}");

    support::type_text(&mut app, "/compact ");
    // And what goes out is the line: the agent parses the name itself, and
    // says which one it ran.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the command to run", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("ran compact"),
        "the command did not run:\n{text}"
    );
}

/// Escape shuts that list, and it stays shut while the name is still being
/// typed.
///
/// The list is not a thing a key opens: it is worked out from the box on
/// every frame, so a key that only set it to `None` would be undone by the
/// frame it was pressed for. Which is why both halves are checked here --
/// that the frame after escape has no list, and that a list shut on a name
/// comes back once the line is no longer one.
///
/// Broken deliberately, both ways: dropping `slash_shut = true` from the
/// escape arm fails the frame after escape, and dropping the clearing in
/// `refresh_slash` fails the list coming back at the end.
#[test]
fn escape_shuts_the_list_of_commands_and_leaves_the_words() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the commands", |app| {
        !app.agent_orders().is_empty()
    });

    support::type_text(&mut app, "/c");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.slash().is_some(), "no list while a name is typed");

    support::press(&mut app, KeyCode::Esc);
    // Escape gives up on the nearest thing, which is the list: what the
    // reader typed is still theirs to send.
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        "/c",
        "escape took the words with the list"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.slash().is_none(),
        "the frame after escape put the list back:\n{dump}"
    );
    assert!(
        !dump.contains("/compact"),
        "the list is shut and still drawn:\n{dump}"
    );

    // And on: a list that came back with the next character is escape
    // working for one keystroke.
    support::type_text(&mut app, "o");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.slash().is_none(),
        "typing the name on brought the shut list back:\n{}",
        screen(&mut app)
    );

    // Rubbing the slash out is what ends it -- the line is no longer a
    // name, so the next one is asked afresh.
    for _ in 0.."/co".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    support::type_text(&mut app, "/c");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.slash().is_some(),
        "the list never came back:\n{}",
        screen(&mut app)
    );
}

/// The conversation's status row says what the session is set to: every
/// setting the agent offers, in its own order, as short as it can be said.
///
/// Values, not names and values: what a select is on names itself -- `Fast`
/// is plainly a model and `ask first` is plainly a way of working -- so a
/// name in front of it would be a label on something already labelled. A
/// switch is the other way round, because "on" says nothing and the thing it
/// is about is its name, so the name is written behind a box, the one every
/// switch in Obelus is drawn as, and being off is said by an empty box and
/// dim ink.
#[test]
fn the_status_row_says_what_the_session_is_set_to() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let status = screen[screen.len() - 1].to_string();
    // Broken by having `said` give a switch no tick: the name alone fails
    // this.
    assert!(
        status.contains("ask first \u{b7} Fast \u{b7} \u{25a1} Allow everything"),
        "not every setting, in the agent's order, with the switch in a box \
         that is empty while it is off:\n{dump}"
    );
    // The names of the selects are not on it: the row would be twice as
    // long and say the same thing.
    assert!(
        !status.contains("Model"),
        "a select's name is on the row as well as its value:\n{dump}"
    );

    // And the switch is dim while it is off, which is the colour Obelus
    // draws everything that is there and not in force. The values are not.
    let styles = support::style_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let ink = |needle: &str| {
        // In cells rather than bytes: the box in front of the switch is
        // one cell and three bytes.
        let at = status[..status.find(needle).expect("the words")]
            .chars()
            .count();
        // Three characters of row number and the bar before the cells.
        styles[styles.len() - 1].chars().nth(at).expect("a cell")
    };
    assert_ne!(
        ink("ask first"),
        ink("Allow everything"),
        "the switch that is off looks like a value:\n{dump}"
    );
    assert_eq!(
        ink("ask first"),
        ink("Fast"),
        "two values are drawn differently:\n{dump}"
    );
}

/// Down, at the bottom of the box, goes to the row under it -- and what is
/// there can then be walked and changed.
///
/// One key, walking whatever is still able to move, in the order the things
/// are on screen: down the box, then down the transcript, then out of the
/// box altogether. Which is the gesture that was already there, one step
/// longer.
#[test]
fn down_from_the_box_reaches_the_settings_and_changes_them() {
    use obelus_component::chat::Focus;

    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    let focus = |app: &App| app.chat().expect("the chat").focus();
    assert_eq!(
        focus(&app),
        Focus::Writing,
        "the box does not have the keys"
    );

    // The transcript is at its end -- which is where it sits until somebody
    // scrolls it -- so down from the box reaches the row.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        focus(&app),
        Focus::Settings(0),
        "down did not reach the row"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        support::cursor_line(&dump),
        "none",
        "the caret stayed in the box while the keys were elsewhere:\n{dump}"
    );
    // The mode is the first of them, and being on it says so with the mark
    // Obelus puts on everything with a list behind it.
    let screen = rows(&dump);
    assert!(
        screen[screen.len() - 1].contains("ask first \u{25b8}"),
        "nothing says the thing under the focus opens:\n{dump}"
    );

    // Along the row, and round.
    support::press(&mut app, KeyCode::Right);
    assert_eq!(focus(&app), Focus::Settings(1));
    support::press(&mut app, KeyCode::Left);
    support::press(&mut app, KeyCode::Left);
    assert_eq!(focus(&app), Focus::Settings(2), "the row does not go round");

    // The switch is last, and enter flips it where it stands: a list of two
    // values is a list nobody wants.
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "a switch opened a list");
    pump(&mut app, &events, "the switch to go on", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "allow_all" && setting.current == "on")
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen[screen.len() - 1].contains("Allow everything"),
        "the switch left the row:\n{dump}"
    );

    // A select opens the list of its values instead, which is the ordinary
    // compact list -- and the focus is still on the row behind it, so the
    // next one can be changed without going back down.
    support::press(&mut app, KeyCode::Left);
    support::press(&mut app, KeyCode::Enter);
    let names: Vec<String> = app
        .picker()
        .expect("the list")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(names, ["Fast", "Careful"], "not the model's values");
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the model to change", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model" && setting.current == "careful")
    });
    assert_eq!(focus(&app), Focus::Settings(1), "the row lost the focus");

    // Up comes back to the box, and so does typing -- a reader who starts
    // typing means to type, and the character is not lost on the way.
    support::press(&mut app, KeyCode::Up);
    assert_eq!(focus(&app), Focus::Writing);
    support::press(&mut app, KeyCode::Down);
    support::type_text(&mut app, "h");
    assert_eq!(focus(&app), Focus::Writing, "typing did not come back down");
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        "h",
        "the character that came back was swallowed"
    );

    // And escape from the row is leaving the row, not leaving the
    // conversation: it gives up on the nearest thing first.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Esc);
    assert_eq!(focus(&app), Focus::Writing);
    assert!(app.chat().is_some(), "escape closed the conversation");
}

/// While the transcript is scrolled up, down brings it back before it leaves
/// the box: the key that was already there keeps its job, and the row is one
/// step further on.
#[test]
fn down_scrolls_the_transcript_before_it_leaves_the_box() {
    use obelus_component::chat::Focus;

    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    // A turn's worth of transcript, and a screen too short for it.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let short = 14;
    support::lay_out(&mut app, WIDTH, short);
    support::press(&mut app, KeyCode::PageUp);
    support::lay_out(&mut app, WIDTH, short);

    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.chat().expect("the chat").focus(),
        Focus::Writing,
        "down left the box while the transcript still had somewhere to go"
    );
    // And once it is back at the end, the next one does leave.
    support::lay_out(&mut app, WIDTH, short);
    for _ in 0..20 {
        support::press(&mut app, KeyCode::Down);
        support::lay_out(&mut app, WIDTH, short);
    }
    assert_eq!(
        app.chat().expect("the chat").focus(),
        Focus::Settings(0),
        "the row was never reached"
    );
}

/// The wheel scrolls the conversation, and leaves the cursor where it is.
///
/// A conversation is a document, and the wheel over a document moves the
/// view: that is what it does over a file, and a transcript is the one
/// place in Obelus with more text than the screen where it did nothing at
/// all. A reader with a mouse in their hand had to reach back to the
/// keyboard to see what an agent had said a minute ago.
///
/// And it leaves the cursor, which is the whole difference between the
/// wheel and the keys: the keys go somewhere, the wheel looks around. The
/// same rule the file follows.
///
/// Broken deliberately by taking the conversation back out of `App::scroll`,
/// which is where it was never put: the notch reaches the file behind the
/// conversation, which is not on screen, and the transcript does not move.
#[test]
fn the_wheel_scrolls_the_transcript_and_leaves_the_cursor() {
    use obelus_component::chat::Focus;

    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A screen too short for the turn, so there is something to scroll.
    let short = 14;
    support::lay_out(&mut app, WIDTH, short);
    let end = app.chat().expect("the chat").top();
    assert!(
        end > 0,
        "the transcript fits, so nothing here means anything"
    );

    app.handle(Event::Scroll(-3));
    support::lay_out(&mut app, WIDTH, short);
    assert_eq!(
        app.chat().expect("the chat").top(),
        end - 3,
        "the wheel did not scroll the transcript"
    );
    assert_eq!(
        app.chat().expect("the chat").focus(),
        Focus::Writing,
        "the wheel took the cursor with it"
    );

    // And back down to the end, which is where a transcript sits.
    app.handle(Event::Scroll(3));
    support::lay_out(&mut app, WIDTH, short);
    assert_eq!(
        app.chat().expect("the chat").top(),
        end,
        "the wheel did not bring it back"
    );
    assert_eq!(app.chat().expect("the chat").focus(), Focus::Writing);
}

/// A selection dragged above the top of the transcript carries on scrolling.
///
/// The other half of the same fix, and the one the code already promised:
/// `App::drag_on` says in as many words that a drag held past the edge
/// reaches the view through the one function a notch of the wheel goes
/// through. In a conversation that function reached nothing, so a reader
/// dragging up the transcript to take hold of a paragraph stopped at the
/// first row on screen and could go no further.
///
/// Broken deliberately the same way as the notch: with the conversation out
/// of `App::scroll`, the tick scrolls nothing and the transcript stands
/// still under the pointer.
#[test]
fn a_drag_held_above_the_transcript_carries_on_scrolling() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let short = 14;
    support::lay_out(&mut app, WIDTH, short);
    let end = app.chat().expect("the chat").top();
    assert!(
        end > 0,
        "the transcript fits, so nothing here means anything"
    );

    // Taken hold of inside the transcript, and dragged out above it.
    let band = obelus_ui::chat::bands(
        app.editor_area_for_test(),
        app.chat().expect("the chat"),
        app.card(),
    )
    .transcript;
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 10,
        y: band.y + 1,
    });
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Dragged,
        x: 10,
        y: band.y.saturating_sub(1),
    });
    app.handle(Event::Tick);
    support::lay_out(&mut app, WIDTH, short);
    assert!(
        app.chat().expect("the chat").top() < end,
        "the drag stopped at the top row on screen"
    );
}

/// A row too narrow for everything says so rather than stopping silently: a
/// reader who cannot see a setting cannot know it is there.
#[test]
fn a_status_row_with_no_room_says_there_is_more() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    support::lay_out(&mut app, 34, HEIGHT);
    let dump = support::render(&mut app, 34, HEIGHT);
    let screen = rows(&dump);
    let status = screen[screen.len() - 1].to_string();
    assert!(
        status.contains('\u{2026}'),
        "nothing says the row was cut:\n{dump}"
    );
    assert!(
        !status.contains("Allow everything"),
        "it all fitted, so this tests nothing:\n{dump}"
    );
}

/// An agent with nothing to configure says so. An empty row would leave a
/// reader wondering whether Obelus had failed to read something.
#[test]
fn an_agent_with_nothing_to_change_says_so() {
    let (mut app, events) = playing(&["nothing-to-change"]);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen[screen.len() - 1].contains("Nothing to change"),
        "the row says nothing at all:\n{dump}"
    );
}

/// The mode is a setting like the others, and there is only ever one of it.
///
/// The protocol is dropping the dedicated mode methods in favour of a config
/// option with `category: "mode"`, so an agent part-way through that change
/// offers both at once -- to be understood by clients on either side of it.
/// A client that showed both would show the same choice twice, and one that
/// took the old one would be using the door that is being closed. Which one
/// it is comes from what the agent said the option is *about*, not from
/// Obelus recognising a name.
#[test]
fn a_mode_offered_both_ways_is_one_setting() {
    // The old way only, which is what the ordinary fake agent plays: the
    // mode comes from `session/new`'s own field, and Obelus reads it into a
    // setting called what Obelus calls it.
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model")
    });
    let old: Vec<&str> = app
        .agent_settings()
        .iter()
        .map(|setting| setting.name.as_str())
        .collect();
    assert_eq!(old, ["Mode", "Model", "Allow everything"]);
    assert_eq!(
        app.agent_mode().map(|mode| mode.name.as_str()),
        Some("Mode"),
        "the old mode methods did not become the mode"
    );
    assert!(
        app.agent_mode().is_some_and(|mode| mode.legacy),
        "a mode from the old methods is not marked as going out the old door"
    );

    // Both ways at once: the option wins, the old one is left out, and it
    // is the agent's own name for it that shows.
    let (mut app, events) = playing(&["mode-as-option"]);
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model")
    });
    let named: Vec<&str> = app
        .agent_settings()
        .iter()
        .map(|setting| setting.name.as_str())
        .collect();
    assert_eq!(
        named,
        ["Way of working", "Model", "Allow everything"],
        "the mode is in the list twice, or not the agent's own"
    );
    let mode = app.agent_mode().expect("a mode");
    assert_eq!(mode.id, "way");
    assert!(!mode.legacy, "the option was taken for the old methods");
    assert_eq!(mode.current_name(), Some("ask first"));

    // And stepping it goes out as a change to that option -- which answers
    // with the whole set again, so what is shown is what the agent took.
    support::press_shift(&mut app, KeyCode::BackTab);
    pump(&mut app, &events, "the mode to move", |app| {
        app.agent_mode()
            .is_some_and(|mode| mode.current_name() == Some("write code"))
    });
}

/// A mode the agent will not take is taken back off the screen.
///
/// `session/set_mode` answers with nothing at all -- there is no room in
/// that answer for the mode it is now in -- so Obelus shows the new one the
/// moment the key is pressed, which is the only way that key can feel like
/// anything. That guess has to be given up if the agent refuses, or the row
/// goes on naming a way of working the agent is not in. (The other door
/// needs none of this: a config option's answer *is* the whole set of them
/// again.)
#[test]
fn a_mode_the_agent_refuses_goes_back() {
    let (mut app, events) = playing(&["refuse-mode"]);
    pump(&mut app, &events, "the mode", |app| {
        app.agent_mode().is_some()
    });
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("ask first")
    );

    support::press_shift(&mut app, KeyCode::BackTab);
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("write code"),
        "the key did nothing while the agent was being asked"
    );

    pump(&mut app, &events, "the refusal", |app| {
        app.agent_mode().and_then(|mode| mode.current_name()) == Some("ask first")
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Changing the mode"),
        "nothing said why it went back:\n{text}"
    );
}

/// A failure that is not a turn's leaves the turns running.
///
/// What fails outside a prompt -- a mode, a setting, a question Obelus
/// cannot put -- arrives naming no conversation, and it used to stop every
/// conversation's turn on the grounds that it could not tell which one it
/// was about. None of them had stopped: the agent went on working in each,
/// and the row that says so had gone.
///
/// Broken deliberately by clearing every turn in `Talk::on`'s `Failed` arm
/// again: the conversation goes to rest under a turn still running.
#[test]
fn a_failure_that_is_not_a_turns_leaves_the_turn_running() {
    let (mut app, events) = playing(&["refuse-mode"]);
    pump(&mut app, &events, "the mode", |app| {
        app.agent_mode().is_some()
    });
    support::type_text(&mut app, "take it slowly");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.talking(), obelus_agent::Talking::Thinking);

    support::press_shift(&mut app, KeyCode::BackTab);
    assert_eq!(
        app.agent_mode().and_then(|mode| mode.current_name()),
        Some("write code"),
        "the mode was not asked for, so nothing can be refused"
    );
    pump(&mut app, &events, "the refusal", |app| {
        app.agent_mode().and_then(|mode| mode.current_name()) == Some("ask first")
    });
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Thinking,
        "a mode the agent would not take stopped a turn it is still working on"
    );
}

/// A prompt the agent cannot take ends that turn, and says so there.
///
/// It was a `Failed`, which names no conversation: the words went to
/// whichever conversation was on screen, and the turn ended only because a
/// failure ended every turn -- which was the other half of this being
/// wrong.
///
/// Broken deliberately by sending the prompt's error as a `Failed` again:
/// nothing ends the turn, and the wait for it gives up.
#[test]
fn a_prompt_the_agent_cannot_take_ends_its_turn() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/broken");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.talking(), obelus_agent::Talking::Thinking);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("nobody has signed in"),
        "the conversation does not say why its turn ended:\n{text}"
    );
}

/// A command named like one of the session's settings is still the agent's
/// command, and still goes to the agent.
///
/// Obelus used to take `/model` for itself and open that setting's values
/// instead of sending it. The names matched, and Copilot's own answer to
/// that command is a dialog it cannot open down a pipe, so the interception
/// looked like a kindness -- but it was a guess about somebody else's
/// namespace. A command and a config option are two different things in the
/// protocol: one is a name the agent takes in a prompt, the other is a
/// choice the client is meant to draw itself, and nothing promises that the
/// same word means the same thing in both. The settings have a door of their
/// own now, which is the row under the box.
#[test]
fn a_command_named_like_a_setting_still_goes_to_the_agent() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the commands", |app| {
        !app.agent_orders().is_empty()
            && app
                .agent_settings()
                .iter()
                .any(|setting| setting.id == "model")
    });
    // The agent offers `/model` and Obelus also has a `model` setting, so
    // this is the case that used to be taken.
    assert!(
        app.agent_orders().iter().any(|order| order.name == "model"),
        "the agent does not offer that command, so this tests nothing"
    );
    assert!(
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model"),
        "there is no setting of that name, so this tests nothing"
    );

    support::type_text(&mut app, "/model");
    // The list of the agent's commands follows what is typed, and it is a
    // frame that settles it.
    support::lay_out(&mut app, WIDTH, HEIGHT);
    // The first enter settles the name in the box, the way it does for any
    // other command -- rather than Obelus taking the row for itself.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.chat().expect("the chat").writing().text(), "/model ");
    assert!(app.picker().is_none(), "Obelus opened a list of its own");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to run it", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("ran model"),
        "the command did not reach the agent:\n{text}"
    );
}

/// One setting's values, as the list the row opens: what each is called,
/// what it is, which one is on -- and the agent's own account of itself
/// afterwards, whichever side asked for the change.
#[test]
fn a_settings_values_are_a_list_and_the_agent_answers_with_all_of_them() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model")
    });

    // Down to the row, along to the model, and open it.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Right);
    support::press(&mut app, KeyCode::Enter);
    let text = screen(&mut app);
    assert!(text.contains("Careful"), "no list of models:\n{text}");
    assert!(
        text.contains("Slower, and better"),
        "the rows do not say what they are:\n{text}"
    );
    assert!(
        text.contains("current"),
        "nothing says which one is on:\n{text}"
    );
    // A row says what it is only when that is not its name again: agents
    // fill both in for every row whether they have anything to add or not.
    let row = rows(&support::render(&mut app, WIDTH, HEIGHT))
        .into_iter()
        .find(|row| row.contains("Fast"))
        .expect("the row")
        .to_string();
    assert_eq!(
        row.matches("Fast").count(),
        1,
        "the row says it twice: {row}"
    );

    // Chosen the way every list is chosen from, and taken by the agent:
    // what comes back is its own account of its settings, which is what
    // Obelus then shows -- and what the reader did is in the transcript,
    // because it is a thing they did to the conversation.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to take it", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model" && setting.current == "careful")
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Model: Careful"),
        "the change is not in the conversation:\n{text}"
    );

    // And it runs the other way too: an agent that puts itself on another
    // model says so, and the row shows what the agent last said.
    support::type_text(&mut app, "answer this one quickly");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the agent to change its own model",
        |app| {
            app.agent_settings()
                .iter()
                .any(|setting| setting.id == "model" && setting.current == "fast")
        },
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen[screen.len() - 1].contains("Fast"),
        "the model it moved itself to is not on the row:\n{dump}"
    );
}

/// A permission question says what it is actually about to do, above the
/// answers.
///
/// "Allow once" and "Reject" are answers, and the question they answer is
/// which command on which file -- not the line the title fits in. The
/// protocol carries that as the tool call's own content, which is what an
/// agent fills in to be shown.
#[test]
fn a_permission_question_says_what_it_will_do() {
    let (mut app, events) = talking();
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let asking = rows(&dump);
    let at = |needle: &str| {
        asking
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?} on screen:\n{dump}"))
    };
    // The command is under the call that is asking, in the transcript --
    // and above the answers, because the card is at the foot. The card does
    // not carry it as well: a question a reader can already read in full is
    // not a thing to print a second time in a five-row window.
    let said = at("cargo test --all-features");
    let allow = at("Allow once");
    let asked = at("Run the tests");
    assert!(asked < said, "the words are not under their call:\n{dump}");
    assert_eq!(said, asked + 1, "something came between them:\n{dump}");
    assert!(
        said < allow,
        "the question is not above the answers:\n{dump}"
    );
    assert_eq!(
        asking
            .iter()
            .filter(|row| row.contains("cargo test --all-features"))
            .count(),
        1,
        "the same words twice on one screen:\n{dump}"
    );
    // The row of settings is still Obelus's status row: a card is part of
    // the conversation rather than a list opened over it.
    assert!(
        asking[asking.len() - 1].contains("ask first"),
        "the card took the status row:\n{dump}"
    );

    // Allow or refuse, and nothing to say in your own words: so there is
    // nowhere on this card for text to go, and the key that would paste it
    // is not offered rather than putting it in the box behind.
    assert!(
        !app.offers(obelus_command::Command::Paste),
        "paste was offered on a card with nothing to write in"
    );
    app.handle(Event::Paste("nowhere for this".to_string()));
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump)
            .iter()
            .any(|row| row.contains("nowhere for this")),
        "the paste landed somewhere on a card that takes no words:\n{dump}"
    );
    assert!(
        app.chat().is_some_and(|chat| chat.writing().is_blank()),
        "the paste went into the message box under the card"
    );

    // And the answers are still answers: the list walks and chooses.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert!(
        screen(&mut app).contains("and I was refused"),
        "the answer the reader chose did not reach the agent"
    );
}

/// Obelus's own keys work inside a conversation.
///
/// They did not while it was a region over the editor: it answered
/// `Context::Dialog`, where nothing is bound, so `ctrl+p` did nothing and
/// the only way to the palette was to leave. A conversation is a document
/// now, and a document is what Obelus's keys are for -- a list opens over
/// it the way it opens over a file, and leaving the list leaves the
/// conversation where it was.
#[test]
fn obeluss_own_keys_work_inside_a_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    support::press_control(&mut app, 'p');
    assert!(
        app.picker().is_some(),
        "the palette would not open in a conversation"
    );

    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "the list would not close");
    assert!(
        app.chat().is_some(),
        "leaving the list took the conversation with it"
    );
}

/// A form's answers and the room to write your own are one card.
///
/// A form arrives as a *map* of fields -- JSON objects have no order to
/// keep -- so the order the agent wrote them in is gone by the time Obelus
/// sees it, and asking in the alphabet's order put an "Other, if none of
/// these suit" in front of the list it was an alternative to. What is left
/// to go on is what the agent said it needs, which is the question itself.
///
/// The two go on the one card because they are one question: these
/// answers, or say what you want instead. Choosing one answers the whole
/// card, so a reader who wants nothing but a named answer presses one key.
#[test]
fn a_form_puts_its_answers_and_room_for_your_own_on_one_card() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // What the form is about, above the answers rather than in a line of
    // its own in the transcript: a list of answers with nothing saying
    // what they answer is not a question.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let asking = rows(&dump);
    let at = |needle: &str| {
        asking
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?} on screen:\n{dump}"))
    };
    let said = at("what would you like to do");
    let first = at("Write the weekly report");
    assert!(said < first, "it is not above the answers:\n{dump}");
    assert!(
        asking[said + 1].contains('\u{2500}'),
        "nothing separates it from them:\n{dump}"
    );

    // The choice, though its name sorts after the optional field's -- and
    // its rows say what each answer is, which is what the agent wrote
    // beside them.
    let listed: Vec<(String, Option<String>)> = app
        .card()
        .expect("the question")
        .choices()
        .iter()
        .map(|choice| (choice.name.clone(), choice.about.clone()))
        .collect();
    assert_eq!(
        listed.first().map(|(name, _)| name.as_str()),
        Some("Write the weekly report"),
        "not the field the agent said it needs: {listed:?}"
    );
    assert_eq!(
        listed
            .first()
            .and_then(|(_, about)| about.clone())
            .as_deref(),
        Some("Gather the git changes of the week and write them up"),
        "the rows do not say what they are: {listed:?}"
    );
    // And under them, the field the agent does not need: a row saying what
    // it is for, which is what a box says while nothing is in it.
    assert_eq!(
        app.card().and_then(Card::placeholder),
        Some("Other"),
        "no room to write an answer of your own:\n{dump}"
    );
    let other = at("Other");
    assert!(other > first, "it is not under the answers:\n{dump}");

    // And the card as a whole: the answer under the reader marked the way
    // every list marks it, the one they are not on plain, and the row for
    // their own answer saying what it is for in the colour Obelus writes
    // everything that is not there yet in.
    app.phase_for_test(0);
    support::check(
        &format!("asked_{WIDTH}x{HEIGHT}"),
        &support::render(&mut app, WIDTH, HEIGHT),
    );

    // Chosen the way every named answer is chosen, and that answers the
    // card: nothing was written, so nothing is sent for the field the
    // agent said it does not need.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer to go back", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Task: Review the code"),
        "the answer is not in the conversation:\n{text}"
    );
    assert!(
        text.contains("you picked review and nothing else"),
        "the chosen value or the omitted field went back wrong:\n{text}"
    );
}

/// What the reader types goes in the box on the card, wherever they were.
///
/// The box is the only thing on a card that takes characters, and a reader
/// who starts typing means to type. What they write is sent with whatever
/// they choose, because both are on the card in front of them.
#[test]
fn writing_your_own_answer_goes_with_the_one_you_choose() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // Typed while the reader is on the first answer: it goes in the box
    // under them rather than nowhere.
    support::type_text(&mut app, "tests as well");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("tests as well")),
        "what was typed is not on the card:\n{dump}"
    );

    // Words alone will not do here, because the agent said it needs one of
    // its own answers -- and the card says so when the reader asks it to
    // send, rather than sending a form the agent will not take.
    support::press(&mut app, KeyCode::Enter);
    assert!(app.is_asking(), "the form went back without what it needs");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("Choose one")),
        "nothing says why it did not go:\n{dump}"
    );

    // And walking back up to the answers does not lose it: the choice and
    // the words are two halves of one answer. Up from the box reaches the
    // last of them, so the second is three rows above it.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Up);
    }
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer to go back", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you picked review and something else"),
        "the words did not go with the choice:\n{text}"
    );
}

/// The agent asks the reader something, and Obelus puts the question.
///
/// A form of three fields, each a card of its own: named answers, the same
/// for a switch, and a box where it takes a number. What goes back is the
/// whole form, keyed by the names the agent gave.
#[test]
fn a_form_the_agent_asks_for_is_put_one_field_at_a_time() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/ask ");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    // What it is asking, in its own words, and the first field's answers.
    let text = screen(&mut app);
    assert!(
        text.contains("which way should I do it"),
        "the question is not on the card:\n{text}"
    );
    for word in ["Quickly", "Carefully", "and slowly"] {
        assert!(text.contains(word), "no {word} on the card:\n{text}");
    }

    // Chosen the way every named answer is, and the next field is there
    // straight away: the agent is waiting on all of them.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let sides: Vec<String> = app
        .card()
        .expect("the switch")
        .choices()
        .iter()
        .map(|choice| choice.name.clone())
        .collect();
    assert_eq!(sides, ["on", "off"], "the switch has other sides");
    assert_eq!(
        app.card().map(Card::on),
        Some(On::Choice(1)),
        "the switch did not open on the side the agent suggested"
    );
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);

    // The last one takes a number, so the card is a box -- and it says what
    // it will take, because a reader who types the wrong thing otherwise
    // finds out afterwards.
    let card = app.card().expect("the number");
    assert!(card.choices().is_empty(), "a number was put as a list");
    let text = screen(&mut app);
    assert!(
        text.contains("whole number from 1 to 9"),
        "the question does not say what it takes:\n{text}"
    );

    // What the reader types is the answer rather than a message, and one
    // that will not do is said and asked again -- with what they typed
    // still there to be fixed.
    support::type_text(&mut app, "later");
    support::press(&mut app, KeyCode::Enter);
    let text = screen(&mut app);
    assert!(
        text.contains("takes a number"),
        "words went in as a number:\n{text}"
    );
    for _ in 0..5 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "12");
    support::press(&mut app, KeyCode::Enter);
    let text = screen(&mut app);
    assert!(
        text.contains("outside"),
        "a number past the end was taken:\n{text}"
    );
    assert!(
        app.is_asking(),
        "the form was answered with what will not do"
    );

    for _ in 0..2 {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "3");
    support::press(&mut app, KeyCode::Enter);
    // And the agent says what it was given: the id of the row, the switch
    // as a boolean, the number as a number.
    pump(&mut app, &events, "what the agent was given", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you said [careful] [true] [3]"),
        "the form did not go back as it was filled in:\n{text}"
    );
}

/// Several answers at once are ticked, and sent from a row of their own.
///
/// A multi-select is the one question where the row under the reader is not
/// the answer -- the ticks are -- so enter ticks and the card is sent from
/// the row that says so. Which is also why the box has a tick of its own
/// here: on a card where everything is ticked, a row that meant something
/// else would be a second way of saying yes.
///
/// How many the agent will take is the agent's to say, and until it has
/// them the row that sends the card says so rather than doing nothing.
#[test]
fn several_answers_are_ticked_and_sent_from_a_row_of_their_own() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/several");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // Every answer with a box in front of it, none of them ticked: a card
    // that ticked something for the reader would be answering for them.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let box_before = |dump: &str, word: &str| -> char {
        rows(dump)
            .iter()
            .find(|row| row.contains(word))
            .map(|row| support::glyph_before(row, word))
            .unwrap_or_else(|| panic!("no row for {word:?}:\n{dump}"))
    };
    let untouched = box_before(&dump, "src/acp");
    assert_eq!(
        box_before(&dump, "Other"),
        untouched,
        "the box of the reader's own answer is not one of the ticks:\n{dump}"
    );

    // Enter ticks, and the card stays: ticking and sending cannot both be
    // enter.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.is_asking(), "a tick answered the question");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert_ne!(
        box_before(&dump, "src/acp"),
        untouched,
        "the answer was not ticked:\n{dump}"
    );

    // One is not enough, and the row that sends the card says which: what
    // cannot be done is drawn dim with the reason on it.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.is_asking(), "the form went back short of what it needs");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("at least 2")),
        "nothing says how many it takes:\n{dump}"
    );
    // And it is still the row the reader is standing on. Two things are
    // being said and they are said in two ways: the background is where the
    // keys are, and the ink is whether this can be used. A row that lost
    // its background for being unusable would leave the reader unable to
    // see where they are -- pressing enter, getting nothing, and with no
    // way to tell that it was this row that refused.
    assert_ne!(
        behind(&dump, "at least 2"),
        behind(&dump, "src/app"),
        "the row the reader is on is not marked while it cannot be used"
    );

    // A second one ticked, and then the card goes.
    for _ in 0..2 {
        support::press(&mut app, KeyCode::Up);
    }
    support::press(&mut app, KeyCode::Enter);
    for _ in 0..2 {
        support::press(&mut app, KeyCode::Down);
    }
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer to go back", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Areas: src/acp, src/ui"),
        "what was ticked is not in the conversation:\n{text}"
    );
    assert!(
        text.contains("you picked two"),
        "the ticks did not go back as a list:\n{text}"
    );
}

/// Ticking the box on such a card opens it, and what is written goes with
/// the ticks.
#[test]
fn the_box_on_a_ticked_card_is_ticked_open() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/several");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // Two of them ticked.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    // Then the box: ticking it is saying you mean to write something, so
    // that is where the keys go next.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.card().map(Card::on),
        Some(On::Words),
        "ticking the box left the reader somewhere else"
    );
    support::type_text(&mut app, "tests too");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("tests too")),
        "what was typed is not on the card:\n{dump}"
    );

    // And the whole card, once: what it is about over a rule, the answers
    // with their ticks, the box under them, and the row that sends it --
    // with the conversation's own status row still at the foot of the
    // screen, because a card is part of the conversation rather than a
    // list opened over it.
    app.phase_for_test(0);
    support::check(
        &format!("card_{WIDTH}x{HEIGHT}"),
        &support::render(&mut app, WIDTH, HEIGHT),
    );

    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer to go back", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you picked two and said where else"),
        "the words did not go with the ticks:\n{text}"
    );
}

/// A question asked in the same breath as the answer that opens the
/// session is asked in that conversation.
///
/// Agents ask before anything is said to them: a login, a workspace. The
/// question names the session the answer has just named, and the crate
/// does not promise that an answer awaited elsewhere reaches Obelus before
/// the next thing the agent sends -- so the question could arrive about a
/// conversation nothing had been told of yet, and be dropped as being about
/// nothing.
///
/// Deliberate break: `open_session` awaiting its answer with `block_task`
/// again. A race, but one the old code lost every time it was run -- eight
/// out of eight -- because the question is already in the pipe behind the
/// answer.
#[test]
fn a_question_asked_as_the_conversation_opens_is_asked_in_it() {
    let (mut app, events) = playing(&["asks-at-once"]);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("which workspace am I in")),
        "the question is not on screen:\n{dump}"
    );
}

/// A question asked while the reader is away from the conversation waits
/// in it, and does not take them back to it.
///
/// It used to: an elicitation was taken to name no conversation, so it was
/// asked wherever the reader was, and where they were in no conversation
/// one was found or made and put on screen -- the first about nothing in
/// particular, or a new one. A reader on the notes while a note's
/// conversation was working was taken to an empty page, answered there, and
/// was left looking at a conversation with no turn in it while the one with
/// the turn went on out of sight. The question names its conversation; it
/// waits there, wearing the mark the list of open documents puts on a
/// conversation with something in it to answer.
///
/// Deliberate breaks: `show_the_question` going to the conversation it
/// asks in, which fails the reader still being on the notes; and `whose`
/// routing `Ask` nowhere, which leaves the question with no conversation to
/// wait in and the wait for it gives up.
#[test]
fn a_question_asked_while_the_reader_is_away_waits_in_its_conversation() {
    let scratch = support::Scratch::new("agent-form-away");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456P\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // A turn that asks a form, walked away from before the form arrives.
    support::type_text(&mut app, "/wordy");
    support::press(&mut app, KeyCode::Enter);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    assert!(
        app.chat().is_none(),
        "the reader is still in the conversation"
    );
    pump(&mut app, &events, "the question", App::anything_waiting);

    // Still on the notes.
    assert!(
        app.chat().is_none(),
        "the question took the reader out of what they were reading"
    );

    // And it is there when they go back, under what they said, with the
    // turn that asked it still running.
    talk_about_the_note(&mut app);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.card().is_some(), "the question is not there:\n{dump}");
    assert!(
        rows(&dump).iter().any(|row| row.contains("/wordy")),
        "the question is not in the conversation that asked it:\n{dump}"
    );
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Thinking,
        "the conversation the question is in does not say its turn is running"
    );
}

/// Escape says no to the form, and the agent hears that rather than nothing.
#[test]
fn escape_on_a_form_tells_the_agent_it_was_not_answered() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/ask ");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    support::press(&mut app, KeyCode::Esc);
    assert!(!app.is_asking(), "the form is still waiting");
    // And escape went to the question, not to the conversation: the
    // nearest thing first.
    assert!(app.chat().is_some(), "escape closed the conversation");
    pump(&mut app, &events, "the agent to hear it", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you would not say"),
        "the agent was not told:\n{text}"
    );
}

/// That list is the picker, so it scrolls and it marks what matched.
///
/// Both of those come from the geometry: the window follows the selection
/// only when it knows how many rows are on screen, and the matched
/// characters are worked out for the rows about to be drawn. A list that
/// never got told either would stand still with the selection walking off
/// the bottom of it.
#[test]
fn the_list_of_commands_scrolls_and_says_what_matched() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the commands", |app| {
        app.agent_orders().len() > 10
    });
    support::type_text(&mut app, "/");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        dump.contains("/compact"),
        "the list does not start at the top:\n{dump}"
    );
    assert!(
        !dump.contains("/usage"),
        "there are fewer commands than rows, so nothing here scrolls:\n{dump}"
    );

    // Walking to the last row brings it on screen, and the rows above it
    // have gone off the top -- which is what scrolling is.
    for _ in 0..app.agent_orders().len() {
        support::press(&mut app, KeyCode::Down);
    }
    let last = app
        .slash()
        .and_then(|slash| slash.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is chosen");
    assert_eq!(last, "/usage", "the arrows did not reach the last row");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        dump.contains("/usage"),
        "the chosen row is not on screen:\n{dump}"
    );
    assert!(
        !dump.contains("/compact"),
        "the list did not scroll:\n{dump}"
    );

    // And the query's characters are marked in the rows, the way they are
    // in every other list.
    while !app.chat().expect("the chat").writing().text().is_empty() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "/cst");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    let marked = app
        .slash()
        .map(|slash| slash.indices_at(0).to_vec())
        .expect("the list");
    assert!(
        !marked.is_empty(),
        "the list does not say which characters matched"
    );
}

/// An agent that stopped is started again by talking to it.
///
/// Which is what the view tells the reader to do, and what it did not do:
/// the handle of the conversation that ended stayed in place, so the check
/// for "is there an agent" found one and said the message into a channel
/// whose other end had gone. The reader's only way out was the settings.
#[test]
fn talking_to_an_agent_that_stopped_starts_it_again() {
    // Started the way a reader's is -- an installed agent named in the
    // settings -- because that is what starting it *again* goes through.
    let (mut app, events) = wired();
    let directory = support::Scratch::new("agent-restart");
    let root = directory.join("agents");
    obelus_agent::remember(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        "0.1",
        &root,
    )
    .expect("writing what was installed");
    app.agents_root_for_test(root);
    let file = directory.join("config.toml");
    std::fs::write(&file, "agent = \"fake\"\n").expect("a settings file");
    app.config_file_for_test(file);
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/die");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to stop", |app| {
        app.talking() == obelus_agent::Talking::Gone
    });

    // What it says is a line, not the protocol crate's own error with the
    // source path of a cargo registry in it.
    //
    // The words around the number are `std`'s rather than Obelus's, and
    // `ExitStatus` says them differently per platform: `exit status: 3`
    // where a process has a status, `exit code: 3` where it has a code.
    // What Obelus owes is the number, and one line to read it on.
    let text = screen(&mut app);
    let said_as = match cfg!(windows) {
        true => "exit code: 3",
        false => "exit status: 3",
    };
    assert!(
        text.contains(said_as),
        "it does not say why it stopped:\n{text}"
    );
    assert!(
        !text.contains("spawned_at"),
        "the transcript has the protocol's own JSON in it:\n{text}"
    );

    // Asked something, it starts again and answers.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start again", |app| {
        matches!(
            app.talking(),
            obelus_agent::Talking::Thinking | obelus_agent::Talking::Ready
        )
    });
    pump(
        &mut app,
        &events,
        "the next question",
        App::is_asking_permission,
    );
    let text = screen(&mut app);
    assert!(
        text.contains("working it out"),
        "the second turn never started:\n{text}"
    );
}

/// An agent that stops takes its question with it.
///
/// A card is answered into a channel, and the far end of that channel died
/// with the agent. Left on screen it would be a question the reader can
/// answer and nobody can hear.
#[test]
fn a_question_goes_when_the_agent_does() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // The agent's thread says the conversation ended, which is the one way
    // Obelus hears about it however the agent went.
    app.handle(Event::Agent(obelus_agent::Event::Acp(
        obelus_agent::acp::Incoming::Gone(None),
    )));
    assert!(app.card().is_none(), "the card outlived the agent");
    assert!(
        !app.is_asking(),
        "the form is still waiting on a dead agent"
    );
}

/// A row of the transcript that names a file is a place to go.
///
/// Which is what Obelus has that a client showing a preview does not: the
/// reader lands in a buffer, with its jump list, its definitions and its
/// hunks. The cursor only ever stands on a row that does something, so
/// there is no way to reach one where enter does nothing.
#[test]
fn a_tool_call_in_the_transcript_opens_the_file_it_was_in() {
    let (mut app, events) = talking();
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Up from the box, which is empty: the caret cannot move in it, so it
    // carries on into the transcript. Then shift and tab twice, which is
    // what goes to the rows that do something -- the command it asked
    // about, and then the file it read. The arrows walk the words now, so
    // reaching a tool call at the top of a turn is its own key.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::BackTab);
    assert!(
        matches!(
            app.chat().map(obelus_component::chat::Chat::focus),
            Some(obelus_component::chat::Focus::Transcript(_))
        ),
        "up from the box did not reach the transcript"
    );
    // And it is lit, the way a chosen row is lit in every list.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let lit = rows(&dump)
        .iter()
        .position(|row| row.contains("Read the file"))
        .expect("the tool call");
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    assert!(
        styles[lit + 1].contains('d'),
        "the row the reader is on is not marked:\n{dump}"
    );

    // Enter opens what it names, at the line it named -- and the
    // conversation gets out of the way, because going somewhere means
    // seeing it.
    support::press(&mut app, KeyCode::Enter);
    assert!(app.chat().is_none(), "the conversation is still over it");
    let buffer = app.current_buffer().expect("the file it read");
    assert!(
        buffer.path().ends_with("many_lines.rs"),
        "it opened {}",
        buffer.path().display()
    );
    assert_eq!(
        buffer.cursor().line.get(),
        6,
        "it did not land on the line the agent named"
    );
}

/// A conversation with nothing said in it keeps the caret in the box.
///
/// The arrows move the nearest thing that can still move. In a conversation
/// with words in it that is the cursor, which walks them; in one with no
/// words at all there is nowhere for a cursor to be, and the key must not
/// leave the caret pointing into an empty band.
#[test]
fn the_arrows_leave_an_empty_transcript_alone() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let chat = app.chat().expect("the conversation");
    assert!(
        chat.rows(60).is_empty(),
        "this conversation has something in it after all"
    );

    support::press(&mut app, KeyCode::Up);
    assert!(
        matches!(
            app.chat().map(obelus_component::chat::Chat::focus),
            Some(obelus_component::chat::Focus::Writing)
        ),
        "the cursor went into a transcript with nothing in it"
    );
}

/// A run of tool calls of one kind is one row until the reader opens it.
///
/// Thirty calls in a turn is a log, and a reader looking for what the agent
/// *did* should not have to scroll past the machine to find it. Opened, the
/// members are rows of their own -- each one a file to go to.
#[test]
fn a_run_of_tool_calls_folds_into_one_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/many");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Four reads, one row -- and the one that is not a read is its own row,
    // because a run is a run of one kind.
    let text = screen(&mut app);
    assert!(text.contains("4 Files"), "the run is not folded:\n{text}");
    assert!(
        !text.contains("Read src/app"),
        "a folded run is showing its members:\n{text}"
    );
    assert!(
        text.contains("Run the tests"),
        "the call that is not a read was folded in with them:\n{text}"
    );

    // Into the transcript, then shift and tab back to the run's heading --
    // past the failed call, which names nothing and so is not a row enter
    // opens -- and enter opens it where it is.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    support::check(&format!("folded_{WIDTH}x{HEIGHT}"), &dump);
    let opened = rows(&dump);
    let heading = opened
        .iter()
        .position(|row| row.contains("4 Files"))
        .expect("the heading");
    assert!(
        opened[heading + 1].contains("Read src/app"),
        "opening it did not put its members under it:\n{dump}"
    );

    // And a member is a place to go, like any other tool call.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.chat().is_none(), "the conversation is still over it");
    let buffer = app.current_buffer().expect("the file it read");
    assert!(
        buffer.path().ends_with("many_lines.rs"),
        "it opened {}",
        buffer.path().display()
    );
}

/// The header says who, and the transcript says what is happening.
///
/// A header says what the thing it names *is*, which for an agent is its
/// name. What is *happening* belongs at the foot of the transcript, where
/// the next thing will appear and where the reader is already looking --
/// and it takes the one hint that goes with it, because escape stopping the
/// agent is the thing a reader could not guess.
#[test]
fn what_is_happening_is_in_the_transcript_and_not_in_the_header() {
    let (mut app, events) = talking();
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let shown = rows(&dump);
    assert!(
        shown[0].contains("Fake Agent") && !shown[0].contains("Thinking"),
        "the header is still saying what is happening:\n{dump}"
    );
    let doing = shown
        .iter()
        .position(|row| row.contains("Thinking\u{2026}"))
        .unwrap_or_else(|| panic!("nothing says it is working:\n{dump}"));
    assert!(
        shown[doing].contains("Esc stops it"),
        "how to stop it is not beside the thing it stops:\n{dump}"
    );

    // And when it is not working, it is not there.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        !text.contains("Thinking\u{2026}"),
        "it is still saying it is working:\n{text}"
    );
}

/// A row naming a file that is not there leaves the reader where they are.
///
/// An agent can name a file it deleted, or one it made up. Opening it
/// cannot work, and the two things that must not happen are the cursor
/// moving in the reader's *own* file to a line from somebody else's, and
/// the conversation hiding itself to show a file that never opened.
#[test]
fn a_row_naming_a_file_that_is_gone_changes_nothing() {
    // With a file of the reader's own open behind the conversation, which
    // is the one that must not be moved about.
    let (sender, events) = channel();
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    app.events_for_test(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // Where the reader left their own file. Not `current_buffer`, because
    // the conversation is what is current now -- it is a document, and
    // opening it is switching to it.
    let reading = app
        .file(obelus_buffer::DocumentId::new(0))
        .expect("the reader's own file")
        .cursor()
        .line;
    support::type_text(&mut app, "/nowhere");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.chat().is_some(),
        "the conversation went away for a file that never opened"
    );
    // Which file the reader still has, rather than which is current: the
    // conversation is what is current, because going nowhere went nowhere.
    let buffer = app
        .file(obelus_buffer::DocumentId::new(0))
        .expect("the reader's own file");
    assert!(
        buffer.path().ends_with("many_lines.rs"),
        "it opened {} out of nothing",
        buffer.path().display()
    );
    assert_eq!(
        buffer.cursor().line,
        reading,
        "the cursor moved in the reader's own file to a line from somebody else's"
    );
}

/// A change an agent is asking to make is in the transcript, open.
///
/// The lines it would write are in neither the file nor the last commit, so
/// this is the only place they exist -- and the reader is being asked to
/// agree to them. They go where everything else the agent did goes, and
/// they stay there afterwards, which is how a reader finds out later what
/// they agreed to.
#[test]
fn a_change_it_is_asking_to_make_is_read_in_the_transcript() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/edit");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking_permission);

    // The file, how much it changes, and the lines themselves -- worked out
    // by Obelus from the two texts the agent sent, with the engine it works
    // out every other change with.
    app.phase_for_test(0);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let shown = rows(&dump);
    let heading = shown
        .iter()
        .position(|row| row.contains("Edit the file"))
        .unwrap_or_else(|| panic!("the change is not in the transcript:\n{dump}"));
    assert!(
        shown[heading].contains("many_lines.rs") && shown[heading].contains("+2 \u{2212}2"),
        "the row does not say what it changes:\n{dump}"
    );
    assert!(
        shown[heading + 2].contains("fn step_rows(row: usize)"),
        "the line it would replace is not shown:\n{dump}"
    );
    assert!(
        shown[heading + 4].contains("fn step_rows(row: ScreenRow)"),
        "the line it would write is not shown:\n{dump}"
    );
    // And the whole of it: the tint a line of a change carries, which is
    // the one an opened hunk carries in a file, and the bar in its own
    // colour at the edge of it.
    support::check(&format!("change_{WIDTH}x{HEIGHT}"), &dump);

    // Answered, it folds: the change is in the file now, and a file's own
    // changes are drawn in the margin beside them. The row that opens it
    // stays, with the count still on it.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("+2 \u{2212}2"),
        "the row forgot what it changed:\n{text}"
    );
    assert!(
        !text.contains("fn step_rows(row: ScreenRow)"),
        "the change is still open after it was made:\n{text}"
    );
}

/// The row that says it is working turns, and stops when it is not.
///
/// A picture of a cog says a tool was used; only movement says it is still
/// going. And the thread that moves it lives exactly as long as its reason:
/// one waking twelve times a second behind a screen where nothing is
/// happening is the one cost an animation must not have.
#[test]
fn the_row_that_says_it_is_working_turns_while_it_is_working() {
    let (mut app, events) = talking();
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    // The first thing drawn on the row, past the row number the dump puts
    // in front of it.
    let turning = |app: &mut App| {
        let dump = support::render(app, WIDTH, HEIGHT);
        rows(&dump)
            .iter()
            .find(|row| row.contains("Thinking"))
            .and_then(|row| row.split_once('|'))
            .and_then(|(_, drawn)| drawn.trim_start().chars().next())
            .expect("the row that says it is working")
    };
    let first = turning(&mut app);

    // A tick arrives on its own, because the agent working is what the
    // ticker is for -- and the frame it brings is a different one.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "nothing is animating while it works");
        let Ok(event) = events.recv_timeout(left) else {
            panic!("nothing is animating while it works");
        };
        let ticked = matches!(event, Event::Tick);
        app.handle(event);
        if ticked && turning(&mut app) != first {
            break;
        }
    }

    // Answered, the turn ends and the ticking stops with it.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A frame, so the ticker is asked for again and not wanted, and then
    // whatever was already on its way. Bounded: a ticker that has not
    // stopped would keep this draining for ever, and a test that hangs
    // says less than one that fails.
    support::lay_out(&mut app, WIDTH, HEIGHT);
    for _ in 0..50 {
        let Ok(event) = events.recv_timeout(Duration::from_millis(100)) else {
            break;
        };
        app.handle(event);
        support::lay_out(&mut app, WIDTH, HEIGHT);
    }
    assert!(
        events.recv_timeout(Duration::from_millis(400)).is_err(),
        "it is still animating with nothing to animate"
    );
}

/// The shade of every cell down the bar's column, top to bottom.
///
/// The track and the thumb are one block in two colours, so which is which
/// is a question about the style grid rather than about the glyphs -- one
/// letter per cell, and the same column in both blocks.
fn shades_down_the_bar(dump: &str) -> Vec<char> {
    let drawn = |block: &str| -> Vec<String> {
        block
            .lines()
            .filter_map(|row| row.split_once('|'))
            .map(|(_, drawn)| drawn.to_string())
            .collect()
    };
    let text = drawn(support::text_block(dump));
    let styles = drawn(support::style_block(dump));
    text.iter()
        .zip(styles)
        .filter_map(|(row, style)| {
            let column = row.chars().position(|glyph| glyph == '\u{2588}')?;
            style.chars().nth(column)
        })
        .collect()
}

/// A transcript long enough to scroll says so, like everything else.
///
/// It was the one scrolling thing in Obelus with no bar: a reader could page
/// through a conversation with nothing on screen answering "how much of this
/// is there, and which part am I looking at". The column it takes is already
/// spare -- the rows are wrapped to leave it -- so nothing moves to make
/// room for it.
#[test]
fn a_transcript_with_more_than_fits_has_a_bar() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Nothing said yet, so nothing to scroll and no bar to say so.
    let dump = support::render(&mut app, 60, 22);
    let bar = |dump: &str| -> Vec<char> {
        rows(dump)
            .iter()
            .filter_map(|row| row.split_once('|'))
            .filter_map(|(_, drawn)| drawn.trim_end().chars().last())
            .filter(|glyph| matches!(glyph, '\u{2502}' | '\u{2588}'))
            .collect()
    };
    assert!(
        bar(&dump).is_empty(),
        "a conversation that fits is drawing a bar:\n{dump}"
    );

    // Said enough to fill it twice over.
    for turn in 0..8 {
        support::type_text(&mut app, &format!("tell me about number {turn}"));
        support::press(&mut app, KeyCode::Enter);
        pump(&mut app, &events, "the question", App::is_asking_permission);
        support::press(&mut app, KeyCode::Enter);
        pump(&mut app, &events, "the turn", |app| {
            app.talking() == obelus_agent::Talking::Ready
        });
    }

    let dump = support::render(&mut app, 60, 22);
    let drawn = bar(&dump);
    assert!(!drawn.is_empty(), "the transcript has no bar:\n{dump}");
    // The track and the thumb are one block in two shades, so which is
    // which is a question about the colours rather than the glyphs.
    let shades = shades_down_the_bar(&dump);
    assert!(
        shades.len() >= 3,
        "the bar is too short to have a thumb on it:\n{dump}"
    );
    // Following the end, so the thumb is at the bottom of the track.
    assert_ne!(
        shades.first(),
        shades.last(),
        "the whole bar is one shade:\n{dump}"
    );
    let thumb = shades.last().copied();
    assert_eq!(
        shades.iter().rev().position(|shade| Some(*shade) != thumb),
        shades
            .iter()
            .rev()
            .position(|shade| Some(*shade) != thumb)
            .filter(|run| *run > 0),
        "the thumb is not where the reader is:\n{dump}"
    );

    // Scrolled back, the thumb comes with it.
    for _ in 0..3 {
        app.handle(Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        )));
    }
    let scrolled = support::render(&mut app, 60, 22);
    assert_ne!(
        shades_down_the_bar(&scrolled).last().copied(),
        thumb,
        "the thumb stayed at the end while the reader went back:\n{scrolled}"
    );
}

/// What Obelus has to say reaches the reader in a conversation too.
///
/// The conversation draws its own status row, so a note has to be one of the
/// things that row carries. Before it was, every command that answers by
/// saying something answered a reader standing in a conversation with
/// silence -- the key worked, and nothing on the screen said so.
#[test]
fn a_note_is_said_on_a_conversations_own_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A jump forward from a conversation nobody jumped back from: the whole
    // of what it does is say so.
    app.go_forward();
    assert_eq!(app.note(), Some("Nowhere further forward"));
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let last = rows(&dump).last().copied().unwrap_or_default().to_string();
    assert!(
        last.contains("Nowhere further forward"),
        "the note is not on the conversation's row:\n{dump}"
    );
}

/// A paste reaches the card the agent is waiting on an answer from.
///
/// The card covers the box a message is written in, so the box behind it is
/// not where a paste can go: text put there is text nobody can see until the
/// question has been answered. What is in front of the reader is the card,
/// and the half of it that takes words is the half they are writing.
#[test]
fn a_paste_goes_into_the_card_and_not_behind_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // Pasted while the reader is on the first of the named answers, which
    // is where the card opens: it goes in the box the same way typing does.
    app.handle(Event::Paste("what the clipboard had".to_string()));
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("what the clipboard had")),
        "the paste is not on the card:\n{dump}"
    );
    // And not in the box behind it, which is what it would have reached if
    // the card were not asked first.
    assert!(
        app.chat().is_some_and(|chat| chat.writing().is_blank()),
        "the paste went into the message box under the card"
    );
}

/// A conversation opened on a note tells the agent so, once.
///
/// The agent has no other way to know: `Topic` never left Obelus, so a
/// conversation about a note looked exactly like one about nothing, and an
/// agent that cannot name a note will not call the tool that finishes one.
/// It goes in a block of its own beside the reader's first words -- not as a
/// turn of its own, which would have the agent talking before the reader had
/// said anything.
#[test]
fn a_conversation_about_a_note_says_so_in_its_first_message() {
    let scratch = support::Scratch::new("agent-note-opening");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456J\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    // Into the notes and on to the one note's conversation.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    assert!(app.chat().is_some(), "no conversation about the note");
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2"),
        "the note did not go with the message:\n{text}"
    );
    assert!(
        text.contains("first=always+note"),
        "Obelus's own block did not carry both halves:\n{text}"
    );
    // And the reader can see that Obelus said it.
    assert!(
        text.contains("Told the agent what this conversation is about"),
        "Obelus spoke in the reader's name without saying so:\n{text}"
    );

    // Once. The agent keeps every word of a conversation, so a second
    // message carrying it again would be telling it twice.
    support::type_text(&mut app, "/blocks again");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks=1"))
        })
    });
}

/// A conversation opened on a note offers the reader something to say.
///
/// The note is the question, and the agent is about to be told it, so what
/// is left to say is usually only "go on" -- which a reader had to type
/// every time before the empty box would send anything. The words are grey
/// in the box and are not theirs until the right arrow puts them in: enter
/// on the grey words sends nothing, as on any empty box, so nothing goes in
/// the reader's name that they did not take. And once the agent has been
/// told the note they are gone, because what was being answered has been
/// asked.
///
/// Broken deliberately three ways. Taking the right arrow's arm out leaves
/// the box empty after the key. Sending the suggestion on a bare enter
/// sends a message before the right arrow. And offering it whatever the
/// agent has been told leaves it in the box after the first message.
#[test]
fn a_conversation_about_a_note_offers_something_to_say() {
    let scratch = support::Scratch::new("agent-note-suggestion");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456K\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    let text = screen(&mut app);
    assert!(
        text.contains("Look into this") && text.contains("Fill it in"),
        "the empty box offered nothing:\n{text}"
    );

    // Not the reader's yet: enter on it is enter on an empty box.
    support::press(&mut app, KeyCode::Enter);
    let _ = screen(&mut app);
    assert!(
        app.chat().is_some_and(|chat| !chat.anything_said()),
        "enter sent the grey words before the reader took them"
    );

    support::press(&mut app, KeyCode::Right);
    assert_eq!(
        app.chat().map(|chat| chat.writing().text()).as_deref(),
        Some("Look into this"),
        "the right arrow did not put the words in the box"
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("a rust file"))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("Told the agent what this conversation is about"),
        "the words went without the note:\n{text}"
    );
    // Told, so there is nothing left to offer.
    assert!(
        app.chat().is_some_and(|chat| chat.suggestion().is_none()) && !text.contains("Fill it in"),
        "the box still offered the words after the note was told:\n{text}"
    );
}

/// A question that arrives while the reader is elsewhere waits for them.
///
/// Not dropped, which is what happened: everything about a conversation was
/// routed to the conversation *on screen* or nowhere, so walking away
/// mid-turn threw the rest of it out -- the words, the calls, and the
/// question, which the agent then heard as a refusal it was never given.
/// Nor does it drag them back: they are reading. It waits on the
/// conversation it is about, wearing the mark the list of open documents
/// puts on a conversation with something waiting in it, and the whole turn
/// is there when they go.
///
/// Broken deliberately by routing on the screen again -- keeping only the
/// conversation being read -- after which nothing arrives at all and the
/// wait for the question times out.
#[test]
fn a_question_that_arrives_while_the_reader_is_away_waits_in_its_own_conversation() {
    let scratch = support::Scratch::new("agent-question-away");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456M\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Asked for, and walked away from before a word of the answer has been
    // taken in: what the agent sends next arrives while the reader is on
    // the notes.
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    assert!(
        app.chat().is_none(),
        "the reader is still in the conversation"
    );
    pump(&mut app, &events, "the question", App::anything_waiting);

    // Nothing reached across the screen to ask it.
    assert!(
        app.card().is_none(),
        "the question took a screen the reader was reading"
    );
    // The list of what is open says where it is waiting.
    app.open_document_picker();
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains(obelus_icons::ui::READER)),
        "nothing says a conversation is waiting on an answer:\n{dump}"
    );
    support::press(&mut app, KeyCode::Esc);

    // And the whole turn is there when they go: the call it is asking
    // about, and the question under it.
    support::press_alt(&mut app, 'a');
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.card().is_some(), "the question is not there:\n{dump}");
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("List crates and app crate sources")),
        "the call the question is about was dropped while they were away:\n{dump}"
    );
    assert!(
        rows(&dump).iter().any(|row| row.contains("/twice")),
        "what the reader said was dropped while they were away:\n{dump}"
    );
}

/// An agent that stops takes its questions with it, wherever they were.
///
/// One agent is behind every conversation, so when it goes every question
/// it asked stops being one. Only the conversation on screen was cleared,
/// which left the rest holding a card nobody was waiting for and wearing
/// the mark that says there is something in them to answer -- a reader sent
/// across the list of what is open to answer into nothing. And the page
/// says what became of it, because a card that went while they were
/// somewhere else would otherwise leave no trace of having been asked.
///
/// Broken deliberately twice, because it makes two claims: clearing only
/// the conversation being read leaves one still waiting, and dropping the
/// note leaves a question that went with no reason on the page.
#[test]
fn an_agent_that_stops_takes_the_questions_in_every_conversation_with_it() {
    let scratch = support::Scratch::new("agent-question-stopped");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456N\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // A question waiting in a conversation the reader has left.
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    pump(&mut app, &events, "the question", App::anything_waiting);

    // And the agent goes while they are still away.
    app.handle(Event::Agent(obelus_agent::Event::Acp(
        obelus_agent::acp::Incoming::Gone(None),
    )));
    assert!(
        !app.anything_waiting(),
        "a conversation is still waiting on an answer nobody wants"
    );

    // What is left in it is the reason, rather than a question that went
    // without one.
    support::press_alt(&mut app, 'a');
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(app.card().is_none(), "the question is still there:\n{dump}");
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("It stopped waiting for an answer")),
        "the question went with no reason on the page:\n{dump}"
    );
}

/// What an agent says is read as markdown, because it is markdown.
///
/// The protocol says so where it defines the block every one of these
/// arrives in: "Text content. May be plain text or formatted with
/// Markdown. Clients SHOULD render this text as Markdown." Obelus drew the
/// characters, so an agent laying its answer out -- a heading, a list, a
/// fenced block, a name in backticks -- was sending punctuation to a reader
/// who had to do the rendering in their head.
///
/// Broken deliberately by taking `Speaker::Agent` out of `reads_as_markdown`,
/// after which the hashes and the backticks are on the screen as
/// characters and the fence is a row of its own saying "```rust".
#[test]
fn what_an_agent_says_is_read_as_markdown() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/markdown");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("pure function"))
        })
    });

    // The mark that turns is on this screen, so the phase is pinned: a
    // golden screen holding a frame of an animation is a golden screen
    // about how quickly the machine got there. It said `\u{280b}` here
    // and `\u{2819}` on a slower runner, which is the whole difference
    // this fixture had.
    app.phase_for_test(0);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    // The words are all there.
    for words in [
        "What I would do",
        "cheap",
        "closer_for",
        "fn closer_for(open: char)",
    ] {
        assert!(
            screen.iter().any(|row| row.contains(words)),
            "{words:?} is not on the page:\n{dump}"
        );
    }
    // And the marks that said how to draw them are not: they were drawn.
    for mark in ["##", "**", "```"] {
        assert!(
            !screen.iter().any(|row| row.contains(mark)),
            "the markdown {mark:?} is on screen as characters:\n{dump}"
        );
    }
    // The whole of it, drawn: the heading in a heading's colour, the code
    // in code's, the rest in the voice that said it.
    support::check(&format!("markdown_{WIDTH}x{HEIGHT}"), &dump);

    // And the cursor can walk on to the box the code is in, which is where
    // the caret used to disappear.
    //
    // The rows of a fenced block are padded to the full width so that the
    // far side of the box lines up under the corners above it. The place
    // after the last character of a row like that is one column past the
    // words -- the column the scrollbar has -- so the caret was drawn
    // where nothing can be seen. `the_caret_stays_off_the_scrollbar` holds
    // the arithmetic up; this holds up that a reader really can get there,
    // over a conversation the agent actually sent.
    //
    // Broken deliberately by putting the caret back on the band's last
    // column, which puts it under the bar.
    support::press(&mut app, KeyCode::Up);
    let width = obelus_ui::chat::reading_width(app.editor_area_for_test());
    for _ in 0..20 {
        let on = match app.chat().expect("a conversation").focus() {
            obelus_component::chat::Focus::Transcript(place) => place,
            other => panic!("the cursor left the transcript: {other:?}"),
        };
        let row = app.chat().expect("a conversation").rows(width)[on.row].text();
        if row.starts_with('\u{2514}') {
            break;
        }
        support::press(&mut app, KeyCode::Up);
    }
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let at = dump
        .split("-- cursor --")
        .nth(1)
        .and_then(|rest| rest.trim().split_once(','))
        .map(|(x, _)| x.to_string())
        .expect("a caret in the transcript");
    assert_eq!(
        at,
        (WIDTH - 2).to_string(),
        "the caret is not on the last column the words have:\n{dump}"
    );
}

/// What an answer means is written out under it, wrapped, never cut.
///
/// A card's answers used to be one row each: the name, and as much of the
/// line the agent wrote about it as fitted after it, ending in an ellipsis.
/// Those lines are the reason the agent wrote them -- they are what tells
/// one answer from another -- and cutting every one of them at the same
/// column is a question that has hidden its own answers. So the name gets
/// a row, what it means gets as many as it needs under it, and the answer
/// the reader is on is marked over the whole block.
///
/// Broken deliberately by putting the line back on the name's row through
/// `truncate_from_right`, after which the tail of the sentence is off the
/// screen and the ellipsis is on it.
#[test]
fn what_an_answer_means_is_written_out_under_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/wordy");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    // The whole sentence, in pieces, each piece on the screen somewhere
    // under the name it belongs to.
    let name = at_row(&screen, "Sink the free functions", &dump);
    for piece in [
        "Move the few hundred lines",
        "out to the crates they belong in",
        "takes very little off the top",
    ] {
        assert!(
            at_row(&screen, piece, &dump) > name,
            "{piece:?} is not under the answer it belongs to:\n{dump}"
        );
    }
    // And nothing was cut to make it fit.
    assert!(
        !screen
            .iter()
            .any(|row| row.contains("\u{2026}") && row.contains("Move the few hundred")),
        "the line about the answer was cut short:\n{dump}"
    );
}

/// Where a needle is on screen, or a panic naming it.
fn at_row(screen: &[&str], needle: &str, dump: &str) -> usize {
    screen
        .iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} on screen:\n{dump}"))
}

/// A list opened over a question is given the room above it.
///
/// A reader who has been asked something and wants to go and look before
/// they answer reaches for the list of open documents, and the list is
/// drawn over the conversation. It was given everything above the box a
/// message is written in -- and a card is taller than a box, so the rows it
/// was handed were rows the card had already been drawn in. It painted over
/// the top of the card, which is the half that says what is being asked:
/// the question went on the way to going and looking it up.
///
/// Broken deliberately by working the box's rows out in `above_writing`
/// again instead of asking `bands`, which puts the list back over the top
/// of the card and takes the question off the screen.
#[test]
fn a_list_over_a_question_does_not_paint_over_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // Somewhere else to be, so the list has two rows and is worth opening.
    app.open_for_test(Path::new("src/lib.rs"));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    let asked = "List crates and app crate sources";
    let before = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&before).iter().any(|row| row.contains(asked)),
        "the card is not asking anything to begin with:\n{before}"
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    // The list is there, and so is the question it was opened in front of.
    assert!(
        rows(&dump).iter().any(|row| row.contains("A conversation")),
        "the list of open documents did not open:\n{dump}"
    );
    assert!(
        rows(&dump).iter().any(|row| row.contains(asked)),
        "the list painted over what the card was asking:\n{dump}"
    );
}

/// A list the reader opened covers the conversation it is over.
///
/// A picker has the keys and it has the status row, so what shows under it
/// is a second view with nothing to say. It used to be given everything
/// above the box a message is written in whether or not there was a
/// question on it, which left the box -- and the rule over it -- drawn
/// between the list's own foot and the list's own prompt, with whatever
/// the reader had half-typed still in it.
///
/// Broken deliberately by giving the picker `above_writing` again in
/// `room_for_a_picker`, which puts the box back under the list.
#[test]
fn a_list_over_a_conversation_covers_the_box() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // Somewhere else to be, so the list has two rows and is worth opening.
    app.open_for_test(Path::new("src/lib.rs"));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);

    let half_written = "what I was in the middle of saying";
    support::type_text(&mut app, half_written);
    let before = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&before).iter().any(|row| row.contains(half_written)),
        "the box is not showing what was typed to begin with:\n{before}"
    );

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("A conversation")),
        "the list of open documents did not open:\n{dump}"
    );
    assert!(
        !rows(&dump).iter().any(|row| row.contains(half_written)),
        "the box was left showing under the list:\n{dump}"
    );
}

/// A question about a conversation is asked in that conversation.
///
/// It went to the conversation about nothing in particular whichever one it
/// was about, because the path that shows a question was the key's own --
/// and that path goes there, and makes one where there is none. A reader
/// whose conversation was about a note was taken out of it and asked on a
/// page they had never typed in: one tool call on it, "starting" under that
/// because the page had no session of its own, and the turn they were
/// actually having left behind in the document they came from.
///
/// Broken deliberately by putting `new_conversation` back at the head of
/// `show_the_question`, which empties the screen of everything the reader
/// said and leaves the one tool call on a page that is still starting.
#[test]
fn a_question_about_a_notes_conversation_is_asked_in_it() {
    let scratch = support::Scratch::new("agent-note-question");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456L\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    // Into the notes and on to the one note's conversation.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    // The reader's own words are still above the question, which is what
    // says it was asked where they were.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("/twice")),
        "the question was asked on a page the reader had never been on:\n{dump}"
    );
    // And the conversation it is in is the one with the session: an empty
    // one opened to hold the card would say it was starting.
    assert_ne!(
        app.talking(),
        obelus_agent::Talking::Starting,
        "the question is in a conversation with no session of its own:\n{dump}"
    );
}

/// A conversation about nothing in particular still says who it is with.
///
/// This said nothing at all once, on the grounds that Obelus does not put
/// words in the reader's mouth where it has no fact of its own to add. It
/// has one: the agent is talking to somebody at a terminal, and the way to
/// put a question to them is a card Obelus draws from `elicitation/create`.
/// That is a fact about Obelus rather than about the reader, so saying it
/// is not speaking for them -- and a conversation about nothing in
/// particular is exactly where an agent asked a question by writing
/// `1) ... 2) ...` into its answer, because nothing had told it otherwise.
///
/// What the reader's own topic adds is still the reader's: a loose one adds
/// nothing, which is the half of the old rule that survives and is asserted
/// here as `first=always` and not `always+note`.
///
/// And it goes without a word about it. Obelus says on the page what it
/// puts in a prompt in the reader's name, because those are the reader's
/// own words going somewhere -- this piece is not in their name, it is the
/// client saying what it is, and a line about it would be the same line at
/// the head of every conversation anybody ever opens.
///
/// Broken deliberately two ways. Moving the opening into `Topic::Note`'s
/// arm of `about_the_topic`, which is where it used to live, stops the
/// block going at all: this reads `blocks=1 first=reader`. And giving
/// `Opening::said` a line for it again puts the note back on the page.
#[test]
fn a_loose_conversation_is_told_who_it_is_with_and_no_more() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2") && text.contains("first=always"),
        "a conversation about nothing was not told who it is with:\n{text}"
    );
    // And nothing about a note, because there is none to be about. Asked
    // of the note and not of everything after `always`, because the
    // workflow is on by default and its line goes with any conversation.
    assert!(
        !text.contains("+note"),
        "something about a note went with a conversation about nothing:\n{text}"
    );
    // The reader is told nothing about it: it went in their name but it is
    // not their words, and there is nothing in it for them to have a view
    // about. Asked of the voice rather than of the words, because the row
    // Obelus speaks in carries four other kinds of remark -- how a turn
    // ended, what became of a question, what happened to the agent -- and
    // what this is about is that Obelus said nothing at all.
    //
    // With words on it: the blank row between two things said wears the
    // speaker of what follows it, and past the last of them that is this
    // one -- so a transcript with no remark in it still ends on a row that
    // says it is Obelus's.
    assert!(
        app.chat().is_some_and(|chat| {
            !chat.rows(WIDTH).iter().any(|row| {
                row.speaker == obelus_component::chat::Speaker::Note && !row.text().is_empty()
            })
        }),
        "Obelus announced its own introduction:\n{text}"
    );
    // Once. The agent keeps every word, so the message after carries none
    // of it.
    support::type_text(&mut app, "/blocks again");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks=1 first=reader"))
        })
    });
}

/// The workflow a project has chosen, as its tool hands it over.
fn the_workflow(app: &mut App) -> String {
    let (answer, mut said) = futures::channel::oneshot::channel();
    app.handle(Event::Tools(obelus_mcp::Asked {
        wanted: obelus_mcp::Wanted::Workflow,
        answer,
    }));
    said.try_recv()
        .ok()
        .flatten()
        .expect("the tool answered nothing")
}

/// Chooses a workflow the way the reader's settings file would.
fn choose_workflow(app: &mut App, workflow: &str) {
    let mut config = app.config().clone();
    config.workflow = workflow.to_string();
    app.configure(config, vec!["workflow"]);
}

/// Asks Obelus to close a conversation, the way its tool does, and says
/// what the agent is told.
fn close_it(app: &mut App, conversation: usize) -> String {
    let (answer, mut said) = futures::channel::oneshot::channel();
    app.handle(Event::Tools(obelus_mcp::Asked {
        wanted: obelus_mcp::Wanted::Close {
            conversation: Some(conversation),
        },
        answer,
    }));
    said.try_recv()
        .ok()
        .flatten()
        .expect("the tool answered nothing")
}

/// Whether the conversation in that slot is still open.
fn is_open(app: &App, conversation: usize) -> bool {
    app.document(obelus_buffer::DocumentId::new(conversation))
        .and_then(obelus_app::app::document::Document::chat)
        .is_some()
}

/// Says something in the conversation on screen and waits for the answer,
/// so that it is not the blank one a new conversation would go back to.
fn say_something(app: &mut App, events: &Receiver<Event>) {
    support::type_text(app, "/cost");
    support::press(app, KeyCode::Enter);
    pump(app, events, "the answer", |app| {
        said_in_transcript(app, "ran cost") && app.talking() == obelus_agent::Talking::Ready
    });
}

/// Each conversation is told an address of its own for Obelus's tools,
/// and the number on the end is its document's.
///
/// Which is the whole of how `close_conversation` knows which conversation
/// is calling: MCP says nothing about who is on the other end, and one
/// address for all of them made "this conversation" a guess.
///
/// Deliberate break: have `tools_for` hand out `self.tools_url` as it is,
/// and both conversations are told `.../mcp` -- the wait for `/mcp/0`
/// gives up.
#[test]
fn every_conversation_is_told_an_address_of_its_own() {
    let root = agents_root_for("addresses");
    std::fs::create_dir_all(&root).expect("the root");
    let log = root.join("asked.log");
    let _ = std::fs::remove_file(&log);
    let (mut app, events) = wired();
    app.tools_url_for_test("http://127.0.0.1:9/mcp");
    let arguments = vec![
        "tests/fixtures/fake-agent.sh".to_string(),
        format!("log={}", log.display()),
    ];
    app.talk_to("fake", Path::new("sh"), &arguments);
    app.new_conversation();
    app.open_a_session_for_test();
    asked(&mut app, &events, &log, "tools http://127.0.0.1:9/mcp/0\n");
    say_something(&mut app, &events);
    app.new_conversation();
    app.open_a_session_for_test();
    asked(&mut app, &events, &log, "tools http://127.0.0.1:9/mcp/1\n");
}

/// An agent that closes its conversation from inside a turn has it closed
/// when the turn ends, and not before.
///
/// From inside a turn is the only way it can: the tool is called while
/// the agent is working, and it usually has something left to say after
/// it. Closed at once, that went to a conversation nobody could see.
///
/// Broken deliberately two ways. Dropping the `thinking` check in
/// `close_for_an_agent` closes it at once, and the conversation is gone
/// while the question is still up. Dropping the call to
/// `close_as_the_agent_asked` in the `Ended` arm leaves it open for good,
/// and the wait for it to close gives up.
#[test]
fn a_conversation_an_agent_closes_closes_when_its_turn_ends() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );

    assert_eq!(close_it(&mut app, 0), "it closes when this turn ends");
    assert!(is_open(&app, 0), "closed in the middle of its own turn");

    // Allowed, and the turn finishes.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the conversation to close", |app| {
        !is_open(app, 0)
    });
}

/// A conversation the reader has started writing in stays open, whatever
/// the agent asks -- and closing one leaves the reader where they are.
///
/// A box is the reader's once they have put something in it, and closing
/// the conversation would take it with it. The other half is that the
/// number names one conversation and not the one on screen.
///
/// Broken deliberately two ways. Dropping the `has_the_readers_words`
/// check in `close_for_an_agent` closes the one with words in its box.
/// Closing `self.current` in place of the document the number names closes
/// the second conversation rather than the first.
#[test]
fn a_conversation_with_the_readers_words_in_it_stays_open() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    say_something(&mut app, &events);
    support::type_text(&mut app, "one more thing");
    assert_eq!(
        close_it(&mut app, 0),
        "the reader has started writing in it, so it stays open"
    );
    assert!(is_open(&app, 0), "closed with the reader's words in it");

    // The box emptied again, and a second conversation the reader is now
    // in: the first is the one asked about.
    for _ in 0.."one more thing".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    app.new_conversation();
    app.open_a_session_for_test();
    assert_eq!(close_it(&mut app, 0), "closed");
    assert!(
        !is_open(&app, 0),
        "the conversation that asked is still open"
    );
    assert!(is_open(&app, 1), "the other conversation went");
    assert!(
        app.chat().is_some(),
        "the reader was taken off the conversation they were in"
    );
}

/// A turn the reader stopped is not the turn the agent meant to be the
/// last one, so the conversation stays open.
///
/// Deliberate break: take the `!finished` out of
/// `close_as_the_agent_asked`, and the stop closes the conversation.
#[test]
fn a_conversation_whose_closing_turn_was_stopped_stays_open() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    say_something(&mut app, &events);
    support::type_text(&mut app, "take it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    assert_eq!(close_it(&mut app, 0), "it closes when this turn ends");

    support::press(&mut app, KeyCode::Esc);
    pump(&mut app, &events, "the turn to stop", |app| {
        !is_open(app, 0) || said_in_transcript(app, "Stopped")
    });
    assert!(is_open(&app, 0), "the stop closed the conversation");
}

/// What the reader says while the closing turn runs keeps the conversation
/// open, and goes to the agent the way anything waiting does.
///
/// The turn here ends as finished when the reader changes a setting, which
/// is something they can do in the middle of one -- and not a stop, which
/// Obelus ends itself, as stopped. So the turn ends the way a closing turn
/// ends, and the only thing keeping the conversation open is the reader's
/// words.
///
/// Deliberate break: take `|| talk.has_the_readers_words()` out of
/// `close_as_the_agent_asked`, and the conversation closes with the
/// reader's words waiting in it.
#[test]
fn words_said_into_the_closing_turn_keep_the_conversation_open() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    support::type_text(&mut app, "/ends-on-a-setting");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    assert_eq!(close_it(&mut app, 0), "it closes when this turn ends");
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);

    // Down to the row of settings, round to the switch at its end, and
    // flipped -- which ends the turn.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Left);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what was waiting", |app| {
        !is_open(app, 0) || said_in_transcript(app, "blocks=")
    });
    assert!(
        is_open(&app, 0),
        "closed with the reader's words waiting in it"
    );
}

/// Choosing another agent while the closing turn runs takes the asking
/// with it.
///
/// The connection that turn ran on is put down, and nothing it says is
/// heard again -- so the end of that turn never arrives. The next turn to
/// end is the reader's own, on an agent that counts its turns from one:
/// closing then is closing a conversation the reader has just gone back
/// to talking in.
///
/// Deliberate break: have `close_as_the_agent_asked` take any closing that
/// was asked for, whatever turn it was asked in, and the conversation
/// closes under the answer to the reader's next message.
#[test]
fn another_agent_takes_the_closing_with_the_one_it_replaced() {
    let (mut app, events) = wired();
    let root = agents_root_for("replaced-closing");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    app.agents_root_for_test(root);
    the_fixture_is_listed(&mut app);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A turn of its own first, so the closing one is running on the
    // agent's side rather than held here for a session on its way.
    say_something(&mut app, &events);
    support::type_text(&mut app, "take it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    assert_eq!(close_it(&mut app, 0), "it closes when this turn ends");

    // Off and on again on the agents page, which puts that connection down
    // and starts another.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConfigOpen);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    assert_ne!(app.config().agent.as_deref(), Some("fake"), "not let go");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().agent.as_deref(), Some("fake"), "not chosen");
    support::press(&mut app, KeyCode::Esc);
    assert!(app.chat().is_some(), "not back in the conversation");

    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        !is_open(app, 0)
            || (said_in_transcript(app, "heard you")
                && app.talking() == obelus_agent::Talking::Ready)
    });
    assert!(
        is_open(&app, 0),
        "the conversation closed at the end of the reader's own turn"
    );
}

/// A conversation an agent closes is named where Obelus says it closed,
/// and goes from the list of what is open while that list is showing.
///
/// The name is the one that list gives it -- here the note it is about --
/// because the reader may have been somewhere else, and "the conversation"
/// is then one of several. And the list is a row of what is true now: a
/// row for a conversation that has gone is a row whose enter does nothing.
///
/// Broken deliberately two ways. Saying "Closed the conversation" whatever
/// the name fails the first assertion. Taking the call to
/// `refresh_switching` out of `close_the_conversation` leaves the row in
/// the list.
#[test]
fn a_conversation_an_agent_closes_is_named_and_leaves_the_list() {
    let scratch = support::Scratch::new("agent-closed-named");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456S\"\nsaid = \"wire the counts up\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // The notes are the first document and the conversation the second.
    assert!(is_open(&app, 1), "no conversation about the note");

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    assert_eq!(close_it(&mut app, 1), "closed");
    assert_eq!(app.note(), Some("Closed wire the counts up"));
    let listed = app
        .picker()
        .expect("the list of what is open")
        .matches()
        .any(|item| {
            matches!(
                item.value,
                obelus_component::picker::PickerValue::Document(id)
                    if id == obelus_buffer::DocumentId::new(1)
            )
        });
    assert!(
        !listed,
        "the closed conversation is still a row of the list"
    );
}

/// A project that has chosen a workflow says so in the first message -- and
/// says only where to read it, because most conversations change nothing
/// and the workflow is several paragraphs.
///
/// Broken deliberately two ways. Dropping the `workflow()` condition in
/// `opening` leaves the line out and this reads `first=always`; pushing
/// the workflow itself in its place reads `first=always+steps`.
#[test]
fn a_chosen_workflow_is_pointed_at_in_the_first_message() {
    let (mut app, events) = talking();
    choose_workflow(&mut app, "feature-branch");
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("first=always+workflow "),
        "the agent was not pointed at the workflow, or was handed it:\n{text}"
    );
}

/// And a project that has chosen none says nothing about one: the line
/// would send the agent to a tool that answers that there is nothing to
/// follow.
///
/// Broken deliberately by pushing the line whatever the setting says,
/// which reads `first=always+workflow`.
#[test]
fn no_workflow_is_not_pointed_at() {
    let (mut app, events) = talking();
    choose_workflow(&mut app, "none");
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("first=always "),
        "the agent was pointed at a workflow nobody chose:\n{text}"
    );
}

/// The tool answers from the settings as they are when it is asked, not as
/// they were when the conversation began: a reader who turns the workflow
/// off half-way is heard the next time the agent reads it, and one who
/// turns it back on is heard too.
///
/// A project that chose nothing has the feature branch, because that is
/// the default.
///
/// Broken deliberately by answering with the workflow whatever the setting
/// says: the assertion after `none` is chosen fails.
#[test]
fn the_workflow_is_read_from_the_settings_when_it_is_asked_for() {
    let (mut app, _events) = wired();
    let said = the_workflow(&mut app);
    assert!(
        said.contains("git worktree add -b"),
        "a project that chose nothing was not handed the default: {said}"
    );
    choose_workflow(&mut app, "none");
    assert!(
        the_workflow(&mut app).contains("no workflow"),
        "choosing none was not heard"
    );
    choose_workflow(&mut app, "feature-branch");
    let said = the_workflow(&mut app);
    assert!(
        said.contains("git worktree add -b"),
        "going back to the feature branch was not heard: {said}"
    );
}

/// A conversation the agent has forgotten starts a fresh one, in place.
///
/// A `todo.toml` that will not read does not forget every conversation in
/// the tree.
///
/// What is remembered about a conversation is keyed to a note, and the table
/// is swept against the names the notes file has -- on the way past, because
/// a note can go without Obelus watching. Reading that file answered with an
/// empty list for a file it could not read, so a half-finished hand edit and
/// one message was every conversation in this tree gone: the agent still had
/// them, and nothing here could name one again.
///
/// Broken deliberately by sweeping against an empty list where the notes
/// could not be read: the conversation goes and this goes red.
#[test]
fn notes_that_will_not_read_do_not_forget_the_conversations() {
    let scratch = support::Scratch::new("agent-notes-unreadable");
    support::make_room_for_notes(scratch.path());
    let note = "0123456N";
    let notes = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &notes,
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "a session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Said once, so that there is a conversation written down to lose. The
    // shape that answers and stops, because what is being watched here is
    // the table beside the notes and not what the agent said.
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let id = obelus_git::todo::NoteId::read(note).expect("a name");
    assert!(
        obelus_agent::acp::sessions::read(scratch.path())
            .remembered()
            .expect("the table")
            .get(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
            )
            .is_some(),
        "the conversation was never written down, so this proves nothing"
    );

    // The reader is half-way through editing the file by hand.
    std::fs::write(&notes, "[[todo]]\nsaid = \"unfinished").expect("the half-written notes");

    // And says something else, which is one of the moments the table is
    // written.
    support::type_text(&mut app, "/echo again");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    assert!(
        obelus_agent::acp::sessions::read(scratch.path())
            .remembered()
            .expect("the table")
            .get(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
            )
            .is_some(),
        "the conversation was forgotten because the notes would not read"
    );
}

/// A note taken away in another window leaves the conversation standing.
///
/// The notes are one file for the whole tree and a second Obelus on it is an
/// ordinary thing to have running, so what arrives here is somebody else's
/// write. Reconciling the open conversations against it shut the one the
/// reader was standing in: `close` put them on the nearest open document,
/// which is the notes page that conversation was opened from, and let go of
/// the session on the way -- so the view jumped and there was nothing to go
/// back to. A note cleared of its words is enough to do it, because a note
/// that says nothing is not written to the file at all.
///
/// The same rule a file deleted in another window follows -- the document
/// keeps what it has, says what it can, and closing it is the reader's.
///
/// Broken deliberately by putting the sweep back on the reread: the
/// conversation is gone from the screen and the second turn has nowhere to
/// go.
#[test]
fn a_note_taken_away_elsewhere_leaves_the_conversation_standing() {
    let scratch = support::Scratch::new("agent-note-taken-away");
    support::make_room_for_notes(scratch.path());
    let note = "0123456P";
    let notes = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &notes,
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "a session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/echo first");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert!(
        said_in_transcript(&app, "first"),
        "nothing was said, so there is no conversation to lose"
    );
    assert!(
        app.is_about_a_note(),
        "the conversation is not about the note, so this proves nothing"
    );

    // The other window clears that note's words, which takes the note out of
    // the file: `Todo::to_toml` does not write one that says nothing.
    std::fs::write(&notes, "").expect("the emptied notes");
    app.handle(Event::Watched(obelus_watch::Changed {
        path: notes.clone(),
    }));
    let dump = support::render(&mut app, WIDTH, HEIGHT);

    // Heard, rather than ignored: the conversation has stopped naming the
    // note, which is the one thing the view can honestly say about it.
    assert!(
        !app.is_about_a_note(),
        "the write was never heard, so standing still proves nothing:\n{dump}"
    );
    assert!(
        said_in_transcript(&app, "first"),
        "the conversation was shut by another window's write:\n{dump}"
    );

    // And the session is still the reader's, which is what a second turn
    // asks: one that was let go has nothing to answer it.
    support::type_text(&mut app, "/echo second");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert!(
        said_in_transcript(&app, "second"),
        "the conversation could not be talked in after the note went:\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
}

/// Agents sweep their conversations up, so a name Obelus wrote down last
/// week may mean nothing today. The reply to that is a new session -- which
/// the protocol side already did -- and the conversation it belongs to has
/// to be put back to how one with no session yet looks, or the session
/// arriving next belongs to nobody and every word the reader types is held
/// for a session that is never coming. What told the agent about the note
/// goes with the old session, so it is said again.
///
/// Which is the other half of
/// `an_agent_is_told_what_the_note_says_only_when_it_does_not_know_it`:
/// Obelus tells an agent what it does not know, and an agent that has lost
/// the conversation knows nothing about the note again -- the words that
/// told it went with the session. Broken deliberately by leaving what was
/// told standing when the session is refused, which hands the fresh
/// conversation to an agent Obelus thinks has already read the note.
#[test]
fn a_conversation_the_agent_has_forgotten_is_started_again() {
    let scratch = support::Scratch::new("agent-forgotten");
    support::make_room_for_notes(scratch.path());
    let note = "0123456J";
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");
    // Written down against a name the agent will refuse, and with what the
    // note said when the agent was told about it: a conversation held
    // yesterday is one the agent was told, which is the state the clearing
    // below has to undo.
    let id = obelus_git::todo::NoteId::read(note).expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
                obelus_agent::acp::sessions::Kept {
                    session: "s-gone".to_string(),
                    title: None,
                    told: Some("a note".to_string()),
                    introduced: true,
                    last: None,
                },
            );
        },
    );

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "a session of some kind", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // Said in the conversation it is about, rather than in whichever one the
    // reader happens to be looking at.
    let text = screen(&mut app);
    assert!(
        text.contains("Starting again"),
        "nothing says the old conversation was not there:\n{text}"
    );

    // And it works: the message goes, and the note goes with it, because
    // what told the agent the first time went with the session that is gone.
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2") && text.contains("first=always+note"),
        "the fresh conversation was not told what it is about:\n{text}"
    );
}

/// The tool that offers notes reaches a conversation about nothing.
///
/// Most of what is worth writing down turns up while talking about
/// something else, so `todo_add` is not gated on the conversation having
/// been opened on a note -- nothing in `src/mcp.rs` asks what it is about.
/// Driven through the event the tool sends rather than over the socket,
/// which is the half this is about: the card comes up where the reader is.
#[test]
fn a_note_can_be_offered_in_a_conversation_about_nothing() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert!(
        !app.is_about_a_note(),
        "this conversation is about a note, so it proves nothing"
    );

    // The same question `todo_add` puts, arriving the same way.
    let (answer, answered) = futures::channel::oneshot::channel();
    app.handle(Event::Agent(obelus_agent::Event::Acp(
        obelus_agent::acp::Incoming::Asked {
            // The fake agent's first, which is this conversation's.
            session: obelus_agent::acp::SessionId::new("s-1"),
            question: obelus_agent::acp::Question::Ask {
                message: "worth writing down?".to_string(),
                fields: vec![obelus_agent::acp::Field {
                    name: "notes".to_string(),
                    title: "keep which of these".to_string(),
                    about: None,
                    takes: obelus_agent::acp::Takes::Some {
                        values: vec![obelus_agent::acp::Value {
                            id: "0".to_string(),
                            name: "the cache is wrong".to_string(),
                            about: None,
                        }],
                        least: Some(0),
                        most: None,
                        chosen: Vec::new(),
                    },
                    required: false,
                }],
                answer,
            },
        },
    )));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("the cache is wrong")),
        "the card did not come up in a loose conversation:\n{dump}"
    );
    drop(answered);
}

/// A call that says nothing but its own title again says it once, and the
/// card is what asks the question.
///
/// An agent may send the tool's description as both the call's title and
/// the call's content, and claude-agent-acp does it for every command it
/// runs. Obelus drew both: the title on the row, the copy folded open
/// under it because a call that is the question opens itself, and nothing
/// at all on the card, on the grounds that the row above was carrying it.
/// Three rows for one line -- and the answers, at the foot of a region
/// with the rest of the turn and an empty half-screen above them, with no
/// subject.
///
/// Broken deliberately by letting the row keep words that are its title
/// again, which puts the line back under the row and makes three of them.
#[test]
fn a_call_that_only_repeats_its_title_says_it_once_and_the_card_asks_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let echoed = "List crates and app crate sources";
    // Twice: the row it is asking about, and the card asking it. Not a
    // third time under the row, which is where the copy used to go.
    assert_eq!(
        screen.iter().filter(|row| row.contains(echoed)).count(),
        2,
        "the one line is not on screen exactly twice:\n{dump}"
    );
    let asking = screen
        .iter()
        .rposition(|row| row.contains(echoed))
        .expect("nothing on screen says what is being asked");
    let answers = screen
        .iter()
        .position(|row| row.contains("Yes"))
        .unwrap_or_else(|| panic!("no answers on screen:\n{dump}"));
    assert!(
        asking < answers,
        "the question is not above the answers:\n{dump}"
    );
    // And the row is one row: nothing folds it, because there is nothing
    // behind it to fold away.
    let row = screen
        .iter()
        .position(|row| row.contains(echoed))
        .expect("the call is not in the transcript");
    assert!(
        !screen[row + 1].contains(echoed),
        "the row still says it under itself:\n{dump}"
    );
}

/// A plan an agent asks leave to act on is read in the transcript.
///
/// It arrives as the words of the call that is asking -- the protocol's
/// `ToolCallContent::Content`, beside the diff Obelus already kept -- and it
/// is longer than a card is tall. So the card carries the answers and the
/// transcript carries the plan: there is where things are read, with the
/// scrolling and the folding and the room, and the plan is still there after
/// the answer is given.
#[test]
fn a_plan_is_read_in_the_transcript_and_the_card_holds_the_answers() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/plan");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the plan", |app| app.card().is_some());

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let at = |needle: &str| {
        screen
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?} on screen:\n{dump}"))
    };
    // The whole of it, not the first fifth: the last step is as visible as
    // the heading.
    //
    // And the heading is a heading rather than a hash and four words: what
    // an agent sends is markdown by the protocol's own word for it, so a
    // plan is read the way its author wrote it.
    let call = at("Approve Plan");
    let heading = at("The plan");
    let last = at("5. Stop");
    assert!(
        !screen.iter().any(|row| row.contains("# The plan")),
        "the plan's markdown is on screen as characters:\n{dump}"
    );
    assert!(call < heading, "the plan is not under its call:\n{dump}");
    assert!(heading < last, "the plan is out of order:\n{dump}");
    assert!(
        last < at("No, keep planning"),
        "the plan is not above the answers:\n{dump}"
    );
    // And once, because the card does not print what the reader can
    // already read above it.
    assert_eq!(
        screen.iter().filter(|row| row.contains("The plan")).count(),
        1,
        "the plan is on the screen twice:\n{dump}"
    );

    // Answered: the plan puts itself away and the row keeps the handle, so
    // what was agreed to is a keypress away rather than gone.
    // Down to the second answer and take it, the way a card is walked.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen.iter().any(|row| row.contains("Approve Plan")),
        "the call's row went with the card:\n{dump}"
    );
    assert!(
        !screen.iter().any(|row| row.contains("# The plan")),
        "an answered plan stayed open:\n{dump}"
    );
}

/// The list an agent keeps while it works is what is happening, not history.
///
/// It goes on the row that says what is happening now, folded to one line --
/// which step of how many, and what that step is. Opened, it is the whole
/// list with how far along each one is. It is never written into the
/// transcript: a finished list of completed steps is a log, and what is kept
/// of a turn is what the agent said and did.
#[test]
fn what_the_agent_means_to_do_is_one_row_that_opens() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/steps");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the list", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("Step 2 of 3"))
        })
    });

    // One row, and it says where it has got to.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen
            .iter()
            .any(|row| row.contains("Step 2 of 3") && row.contains("wire it to the search")),
        "the row does not say which step it is on:\n{dump}"
    );
    assert!(
        !screen.iter().any(|row| row.contains("write the test")),
        "the whole list is open before anybody asked:\n{dump}"
    );

    // Opened: the whole of it, in the order the agent gave. Up from the
    // box reaches it in one, because it is the last row of the transcript
    // and it is now a row that does something.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    app.phase_for_test(0);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    for step in [
        "read the counts tree",
        "wire it to the search",
        "write the test",
    ] {
        assert!(
            screen.iter().any(|row| row.contains(step)),
            "{step:?} is not on the opened list:\n{dump}"
        );
    }
    // How to stop it belongs to the one row that says something is going.
    // The steps under it are the same speaker, so without saying which row
    // it is for, every step of the list carried it too.
    assert_eq!(
        screen
            .iter()
            .filter(|row| row.contains("Esc stops it"))
            .count(),
        1,
        "the hint is on more than the row it is about:\n{dump}"
    );
    // And the whole of it as a picture, because the rest of what is wrong
    // with a list like this is where things sit: a status drawn in front of
    // its step *and* after it is two marks for one fact, and no assertion
    // about the words can see it.
    support::check("plan_76x24", &dump);

    // And none of it was written into the transcript: every row carrying a
    // step is one the row that says what is happening owns -- itself, or a
    // step under it. It is state, so it goes when it stops being true
    // rather than staying as a record of itself.
    use obelus_component::chat::Speaker;
    let said = app.chat().expect("A conversation").rows(WIDTH);
    assert!(
        said.iter()
            .filter(|row| row.text().contains("write the test"))
            .all(|row| matches!(row.speaker, Speaker::Doing | Speaker::Step)),
        "the list was written into the transcript as something said"
    );
}

/// How full the agent is goes on the row the session's own facts go on.
///
/// A proportion and the cost, beside the keys rather than beside the
/// settings: the settings scroll along that row to keep the focused one on
/// screen, and a number that slid about with them is a number to find again
/// every time the reader steps one. And it is never said in the transcript
/// -- the agent sends several of these a turn, and a transcript with them
/// in it is a log.
///
/// Broken deliberately by leaving `SessionUpdate::UsageUpdate` in the arm
/// that drops what Obelus does not show: the row says nothing and this goes
/// red.
#[test]
fn how_full_the_agent_is_goes_on_the_status_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/used");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it has used", |app| {
        app.agent_usage().is_some_and(|used| used.used == 188_000)
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let status = screen.last().expect("the status row");
    assert!(
        status.contains("94%"),
        "the row does not say how full it is:\n{dump}"
    );
    assert!(
        status.contains("1.13 USD"),
        "the row does not say what it has cost:\n{dump}"
    );
    // The code, not a sign Obelus guessed.
    assert!(
        !status.contains('$'),
        "Obelus made up a currency sign:\n{dump}"
    );
    // Not a thing said.
    assert!(
        !screen[..screen.len() - 1]
            .iter()
            .any(|row| row.contains("94%")),
        "how full it is was written into the transcript:\n{dump}"
    );

    // And over the mark it stops being furniture: the row says three things
    // in three greys already, so this one is not a grey.
    assert_eq!(
        colour_of(&dump, "94%"),
        support::spelled(app.theme().status_stale),
        "an agent nearly out of room says so in the colour of the row's \
         other greys:\n{dump}"
    );
}

/// The colour the first row carrying this text is written in.
///
/// Counted in cells rather than in bytes: a row of this screen has glyphs
/// several bytes wide in it, and the style grid has one letter per cell.
fn colour_of(dump: &str, needle: &str) -> String {
    let cells = |block: &str| -> Vec<Vec<char>> {
        block
            .lines()
            .filter_map(|row| row.split_once('|'))
            .map(|(_, row)| row.chars().collect())
            .collect()
    };
    let words = cells(support::text_block(dump));
    let styles = cells(support::style_block(dump));
    for (said, style) in words.iter().zip(&styles) {
        let row: String = said.iter().collect();
        let Some(byte) = row.find(needle) else {
            continue;
        };
        let at = row[..byte].chars().count();
        let letter = *style.get(at).expect("a style for the cell");
        return support::legend_of(dump, letter)
            .split_whitespace()
            .find_map(|part| part.strip_prefix("fg=").map(str::to_string))
            .expect("a foreground");
    }
    panic!("no {needle:?} on the page:\n{dump}")
}

/// Under the mark it is furniture, and an agent that counts no cost leaves
/// no gap where one would have been.
///
/// Broken deliberately by colouring it `status_stale` whatever the number,
/// or by writing the cost as an empty string rather than leaving it out.
#[test]
fn an_agent_with_room_left_says_so_quietly() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/room");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it has used", |app| {
        app.agent_usage().is_some_and(|used| used.used == 62_000)
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let status = rows(&dump).last().expect("the status row").to_string();
    assert!(
        status.contains("31%"),
        "the row does not say how full it is:\n{dump}"
    );
    assert_eq!(
        colour_of(&dump, "31%"),
        support::spelled(app.theme().gutter),
        "a number nobody has to act on is not furniture:\n{dump}"
    );
    // Nothing at all where a cost would have been, not an empty one.
    assert!(
        !status.contains(" \u{b7} ") || status.matches('\u{b7}').count() < 3,
        "a cost the agent never gave left something behind:\n{status}"
    );
}

/// Somewhere to go is put on a card, and going is what answers it.
///
/// The other kind of elicitation: nothing to fill in, a URL to visit. The
/// URL is shown whole -- folded across as many rows as it takes -- because
/// one cut short is one nobody can use, and on a machine with no browser it
/// is the only way the reader will get it. Answered the moment they are
/// sent, not when they come back: the agent watches the far end itself.
///
/// Broken deliberately by leaving `ElicitationMode::Url` in the arm that
/// declines a mode Obelus cannot put: no card, and this goes red.
#[test]
fn somewhere_to_go_is_put_on_a_card_and_opened() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/signin");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the card", |app| app.card().is_some());

    // What it is for, then where -- whole, across as many rows as it takes.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen.iter().any(|row| row.contains("sign in to continue")),
        "the card does not say what it is for:\n{dump}"
    );
    // The rows run together, less their own numbers and the blanks that
    // pad them: a url folded across two rows is one url again.
    let shown: String = screen
        .iter()
        .filter_map(|row| row.split_once('|'))
        .flat_map(|(_, said)| said.chars())
        .filter(|letter| !letter.is_whitespace())
        .collect();
    assert!(
        shown.contains("https://console.example.com/oauth/authorize?client_id=9d1c4a"),
        "the url was cut short:\n{dump}"
    );
    assert!(
        shown.contains("state=7f2b"),
        "the end of the url is not on the page:\n{dump}"
    );
    assert!(
        screen.iter().any(|row| row.contains("Open it")),
        "there is no way to go:\n{dump}"
    );

    // Going opens it, and the agent hears that they went.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        obelus_clipboard::links::opened().as_deref(),
        Some(
            "https://console.example.com/oauth/authorize?client_id=9d1c4a&scope=user%3Ainference&code=1&state=7f2b"
        ),
        "the link was not opened"
    );
    pump(&mut app, &events, "what the agent made of it", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("you went"))
        })
    });
}

/// A URL Obelus will not hand to the machine is refused, not declined.
///
/// What happens to one of these is that Obelus asks the machine to open it
/// with whatever is registered for the scheme, and the string came from the
/// agent: `file:` reaches the disk, and a program that registers a scheme of
/// its own turns a link into a way to start it. So only `http` and `https`,
/// with a host -- and the agent hears that its request was wrong rather than
/// that the reader said no, because the reader was never asked.
///
/// Broken deliberately by returning the URL from `somewhere_to_go` whatever
/// its scheme: a card goes up and this goes red.
#[test]
fn a_url_obelus_will_not_open_never_reaches_the_reader() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/nowhere");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    assert!(app.card().is_none(), "Obelus put a file: url to the reader");
    assert_eq!(
        obelus_clipboard::links::opened(),
        None,
        "Obelus opened a file: url"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump).iter().any(|row| row.contains("vscode://")),
        "the url reached the page:\n{dump}"
    );
}

/// Once the reader has been sent, the card gives way to a row.
///
/// The agent is not waiting on Obelus any more -- it was told they went --
/// so the box has to come back, and what is left is a thing under way,
/// which the transcript already has a shape for. It says it is still under
/// way until the agent says the far end happened; pressing the key on it
/// sends the reader again, for the tab they closed.
///
/// Broken deliberately by leaving the card up instead, or by making
/// `Chat::arrived` do nothing: the row never settles and this goes red.
#[test]
fn where_the_reader_was_sent_stays_on_the_page_until_it_is_done() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/signin");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the card", |app| app.card().is_some());
    support::press(&mut app, KeyCode::Enter);

    // The card is gone and the row is there, saying it is under way.
    assert!(
        app.card().is_none(),
        "the card stayed after it was answered"
    );
    let waiting = |app: &App| {
        app.chat().and_then(|chat| {
            chat.rows(WIDTH)
                .iter()
                .find(|row| row.text().contains("sign in to continue"))
                .map(|row| row.state.clone())
        })
    };
    assert_eq!(
        waiting(&app),
        Some(Some("in_progress".to_string())),
        "the row does not say it is still under way"
    );

    // And the agent, having watched the far end, says the waiting is over.
    pump(&mut app, &events, "the far end", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH).iter().any(|row| {
                row.text().contains("sign in to continue")
                    && row.state.as_deref() == Some("completed")
            })
        })
    });

    // And the key on the row sends them again, for the tab they closed.
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);
    assert_eq!(
        obelus_clipboard::links::opened(),
        None,
        "the test did not start clean"
    );
    let at = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .position(|row| row.away.is_some())
        .expect("the row that points somewhere");
    let last = app.chat().expect("the conversation").rows(WIDTH).len() - 1;
    for _ in 0..=(last - at) {
        support::press(&mut app, KeyCode::Up);
    }
    support::press(&mut app, KeyCode::Enter);
    assert!(
        obelus_clipboard::links::opened().is_some_and(|url| url.contains("console.example.com")),
        "the row would not send them again"
    );
}

/// The far end happening is said in the conversation that sent the reader
/// there, wherever the reader has gone since.
///
/// `elicitation/complete` names only the question, and it was marked in
/// whichever conversation was on screen when it arrived -- which, for a
/// reader who has just gone off to sign in, is rarely the one they left:
/// the row said it was under way for good.
///
/// Broken deliberately by marking only the conversation on screen in
/// `went_through`: the row never settles and the wait for it gives up.
#[test]
fn the_far_end_is_marked_where_the_reader_was_sent_from() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/signin");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the card", |app| app.card().is_some());
    support::press(&mut app, KeyCode::Enter);

    // Off to something else before the agent says the far end happened.
    app.open_for_test(std::path::Path::new("tests/fixtures/sample.rs"));
    assert!(app.chat().is_none(), "still in the conversation");
    pump(
        &mut app,
        &events,
        "the far end, in the conversation",
        |app| {
            app.document(obelus_buffer::DocumentId::new(0))
                .and_then(|document| document.chat())
                .is_some_and(|talk| {
                    talk.chat.rows(WIDTH).iter().any(|row| {
                        row.text().contains("sign in to continue")
                            && row.state.as_deref() == Some("completed")
                    })
                })
        },
    );
}

/// On a row too narrow for all three, the number is the one that goes.
///
/// The settings say what the session is set to and the keys say what they
/// do; how full the agent is is a number. A row that kept it had the
/// settings cut to a letter and the keys pushed off the end, which is two
/// things lost to keep one.
///
/// Broken deliberately by taking the `filter` off the usage in
/// `ChatView::status`: the keys go and this goes red.
#[test]
fn a_narrow_row_keeps_the_settings_and_the_keys() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/used");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it has used", |app| {
        app.agent_usage().is_some_and(|used| used.used == 188_000)
    });

    let row = |app: &mut App, width: u16| {
        let dump = support::render(app, width, 14);
        rows(&dump)
            .last()
            .and_then(|row| row.split_once('|'))
            .map(|(_, said)| said.to_string())
            .expect("the status row")
    };

    // Where it fits, all three are there.
    let wide = row(&mut app, WIDTH);
    assert!(wide.contains("94%") && wide.contains("Mode") && wide.contains("ask first"));

    // Where it does not, the number goes and the other two stay whole.
    let narrow = row(&mut app, 30);
    assert!(
        !narrow.contains("94%"),
        "the number stayed on a row that could not hold it: {narrow:?}"
    );
    assert!(
        narrow.contains("Mode"),
        "the keys were pushed off the end: {narrow:?}"
    );
    assert!(
        narrow.contains("ask first"),
        "the settings were cut down to nothing: {narrow:?}"
    );
}

/// The reader's own half of a conversation comes back from the agent.
///
/// `user_message_chunk` was one of the updates Obelus dropped. In a live
/// turn it is news Obelus already has -- it put those words there itself --
/// but the turn it is for is the one nobody was here for: `session/load`
/// replays a conversation to a client that may be a fresh process, and the
/// reader's half of it comes back only this way. Dropped, a conversation
/// taken up again is a run of answers with no questions above them.
///
/// Broken deliberately by leaving `SessionUpdate::UserMessageChunk` in the
/// arm that drops what Obelus does not show: the words never arrive and
/// this goes red.
#[test]
fn the_readers_own_words_come_back_from_the_agent() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/relay");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // In the reader's voice, because that is whose words they are.
    let said = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .find(|row| row.text().contains("what did we settle on"))
        .map(|row| row.speaker);
    assert_eq!(
        said,
        Some(obelus_component::chat::Speaker::Reader),
        "the words the agent had of the reader are not on the page:\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
}

/// An agent that echoes the prompt back does not put it on the page twice.
///
/// Obelus writes the reader's words when they press send, and some agents
/// send the same words back over the session. What makes dropping the
/// repeat safe is that the only rows Obelus writes in that voice are the
/// ones it was handed by the reader: a repeat of what is already there is
/// the agent's copy of it, not a second thing they said.
///
/// Broken deliberately by taking the `echoed` check out of `Chat::heard`:
/// the prompt is on the page twice and this goes red.
#[test]
fn an_agent_that_echoes_the_prompt_does_not_say_it_twice() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("heard you"))
        })
    });

    // Counted in the words, not in the rows: an echo merges into the row
    // that is already there, so twice over is one row saying it twice.
    let said: String = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .filter(|row| row.speaker == obelus_component::chat::Speaker::Reader)
        .map(|row| row.text())
        .collect();
    assert_eq!(
        said.matches("/echo").count(),
        1,
        "the prompt is on the page more than once: {said:?}\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
}

/// A tree with a note, and a conversation remembered against it.
///
/// The shape `a_conversation_the_agent_has_forgotten_is_started_again` sets
/// up, which is the only way to reach a reopen: Obelus asks for one it had
/// before when the note it is about has a name written down beside it.
///
/// What the note said when the agent was told about it goes down with the
/// session's name, because that is what a conversation held yesterday
/// looks like: the agent was told, and what it was told still matches.
///
/// The name must be one the fake agent never mints for itself -- it numbers
/// its own `s-1`, `s-2` -- or the session it opens on the way up is taken
/// for the one that was asked for.
fn remembering(
    name: &str,
    note: &str,
    session: &str,
    how: &[&str],
) -> (support::Scratch, App, Receiver<Event>) {
    remembering_how(name, note, session, how, None)
}

/// The same, with Obelus offering an agent its own tools.
///
/// The address rather than a server: what is being tested is what an agent
/// is *told*, and a test that opened a port would be testing the machine.
fn remembering_with_tools(
    name: &str,
    note: &str,
    session: &str,
    how: &[&str],
) -> (support::Scratch, App, Receiver<Event>) {
    remembering_how(name, note, session, how, Some("http://127.0.0.1:9/mcp"))
}

fn remembering_how(
    name: &str,
    note: &str,
    session: &str,
    how: &[&str],
    tools: Option<&str>,
) -> (support::Scratch, App, Receiver<Event>) {
    let scratch = support::Scratch::new(name);
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");
    let id = obelus_git::todo::NoteId::read(note).expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
                obelus_agent::acp::sessions::Kept {
                    session: session.to_string(),
                    title: None,
                    told: Some("a note".to_string()),
                    introduced: true,
                    last: None,
                },
            );
        },
    );

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    if let Some(tools) = tools {
        app.tools_url_for_test(tools);
    }
    let mut arguments = vec!["tests/fixtures/fake-agent.sh".to_string()];
    arguments.extend(how.iter().map(|word| (*word).to_string()));
    app.talk_to("fake", Path::new("sh"), &arguments);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    (scratch, app, events)
}

/// Whether Obelus has said anything about the conversation it asked for.
///
/// `Talking::Ready` is not that: it is true as soon as the agent has
/// spoken at all, which is before it has answered about this conversation.
fn settled(app: &App) -> bool {
    app.chat().is_some_and(|chat| {
        chat.rows(WIDTH)
            .iter()
            .any(|row| row.speaker == obelus_component::chat::Speaker::Note)
    })
}

/// A note says whether anybody has talked about it.
///
/// Which conversation is about which note is written down and outlives the
/// session, and there was nowhere on screen that said so: the only way to
/// find out whether a note already had one was to open it and see whether
/// anything came back. Two columns of their own in front of the box, the
/// pair the list of open documents gives a conversation: what it is doing,
/// and that it is there. Both are in the same place at every depth, and a
/// note nobody has talked about leaves them empty rather than moving the
/// words.
///
/// It makes three claims and was broken deliberately three times.
/// Answering `Talked::Not` for a note with a name written down against it
/// takes the mark off a note that has a conversation. Answering
/// `Talked::Yes` where a card is up leaves the column that says what is
/// happening empty on a conversation that is waiting to be answered. And
/// drawing the waiting glyph in the column the agent's own mark is in
/// loses the mark that says there is a conversation at all.
#[test]
fn a_note_says_whether_anybody_has_talked_about_it() {
    let scratch = support::Scratch::new("agent-note-marks");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Q\"\nsaid = \"talked about\"\ndone = false\ndepth = 0\n\n         [[todo]]\nid = \"0123456R\"\nsaid = \"never mentioned\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let id = obelus_git::todo::NoteId::read("0123456Q").expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
                obelus_agent::acp::sessions::Kept {
                    session: "s-old".to_string(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: None,
                },
            );
        },
    );

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    let root = scratch.join("agents");
    obelus_agent::remember(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        "0.1",
        &root,
    )
    .expect("writing what was installed");
    app.agents_root_for_test(root);
    let file = scratch.join("config.toml");
    std::fs::write(&file, "agent = \"fake\"\n").expect("a settings file");
    app.config_file_for_test(file);

    // Nothing running yet: the mark is about what is written down, which
    // is the half a restart has to survive.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let marked = |dump: &str, said: &str| {
        rows(dump)
            .iter()
            .find(|row| row.contains(said))
            .unwrap_or_else(|| panic!("no note saying {said:?}:\n{dump}"))
            .contains(MARK)
    };
    assert!(
        marked(&dump, "talked about"),
        "a note with a conversation written down says nothing about it:\n{dump}"
    );
    assert!(
        !marked(&dump, "never mentioned"),
        "a note nobody has talked about is marked as though they had:\n{dump}"
    );

    // And once that conversation is waiting on an answer, the mark says
    // so -- in the colour the list of open documents uses for the same
    // thing, which is what the whole frame is checked for.
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::anything_waiting);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let waiting = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        marked(&waiting, "talked about"),
        "the note lost its mark while its conversation was waiting:\n{waiting}"
    );
    let asked = |dump: &str, said: &str| {
        rows(dump)
            .iter()
            .find(|row| row.contains(said))
            .unwrap_or_else(|| panic!("no note saying {said:?}:\n{dump}"))
            .contains(WAITING)
    };
    assert!(
        asked(&waiting, "talked about"),
        "nothing on the note says its conversation is waiting:\n{waiting}"
    );
    assert!(
        !asked(&waiting, "never mentioned"),
        "a note with no conversation says one is waiting:\n{waiting}"
    );
    support::check(&format!("notes_waiting_{WIDTH}x{HEIGHT}"), &waiting);
}

/// A note whose agent is at work says so with a mark that turns.
///
/// The two columns are what the list of open documents draws: a picture of
/// an agent says there is a conversation, and only movement beside it says
/// something is happening in it. Which is the half worth having here, and
/// the half a reader cannot get any other way -- a conversation left with
/// an agent in it is one they walked away from, and the note is what they
/// come back through.
///
/// It makes three claims and was broken deliberately three times.
/// Answering `Talked::Yes` for a conversation its agent is thinking in
/// leaves the turning column empty, so the note says only that somebody
/// once talked about it. Leaving the notes out of what wants animating
/// stops the ticker, and a mark nothing wakes is a mark that stands still
/// for exactly as long as the work takes. And drawing the mark from the
/// note rather than from the ticker gives a frame that never changes,
/// which is a picture of work rather than a sign of it.
#[test]
fn a_note_whose_agent_is_working_turns_beside_it() {
    let (_scratch, mut app, events) = remembering("agent-note-working", "0123456S", "s-old", &[]);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A turn that never ends on its own, so the agent is still thinking
    // when the reader has gone back to the notes -- which is the whole
    // case: nobody is looking at the conversation.
    support::type_text(&mut app, "slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to start", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);

    // The ticker has to be running at all, which nothing else on this
    // page would ask for: the conversation is not what is being read, so
    // the only thing here with a reason to wake the screen is the mark
    // beside the note. Waited for off the channel, the way the
    // conversation's own turning mark is.
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "nothing is animating beside the note");
        let Ok(event) = events.recv_timeout(left) else {
            panic!("nothing is animating beside the note");
        };
        let ticked = matches!(event, Event::Tick);
        app.handle(event);
        support::lay_out(&mut app, WIDTH, HEIGHT);
        if ticked {
            break;
        }
    }

    // The frame is the ticker's, so it is asked for at two of them: a
    // mark drawn from anything else is the same character both times.
    let frame = |app: &mut App, phase: u32| {
        app.phase_for_test(phase);
        let dump = support::render(app, WIDTH, HEIGHT);
        let row = rows(&dump)
            .iter()
            .find(|row| row.contains("a note"))
            .and_then(|row| row.split_once('|'))
            .map(|(_, drawn)| drawn.to_string())
            .unwrap_or_else(|| panic!("no row for the note:\n{dump}"));
        assert!(
            row.contains(MARK),
            "the note lost the mark saying it has a conversation:\n{dump}"
        );
        assert!(
            row.contains(obelus_ui::spinning(phase)),
            "nothing beside the note says its agent is at work:\n{dump}"
        );
        row
    };
    let first = frame(&mut app, 0);
    let next = frame(&mut app, 1);
    assert_ne!(
        first, next,
        "the mark beside the note does not turn while its agent works"
    );
}

/// The clock stops when the reader leaves the conversation.
///
/// What Obelus is doing about an agent is a question about the
/// conversation being read, and with none being read there is no session
/// for it to be about. It answered anyway: no session is not a session
/// the agent has, so it said "starting..." for as long as an agent was
/// up. Nobody saw the word -- it is drawn in a transcript, and there is
/// no transcript on a file or on the notes -- but the frame asks whether
/// anything is moving, and a turning mark is something moving. So opening
/// an agent once put Obelus on a twelve-a-second clock behind everything
/// else it drew, for a turn that was not running.
///
/// Broken deliberately by answering about the session again where there
/// is no conversation, which is the state this is about.
#[test]
fn the_clock_stops_when_the_reader_leaves_the_conversation() {
    let (_scratch, mut app, events) = remembering("agent-clock-stops", "0123456V", "s-old", &[]);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // The notes, which are a document and not a conversation -- and have
    // nothing of their own to animate, because nobody is working in the
    // one this note has.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Ready,
        "an agent being asked nothing says it is starting"
    );

    // Whatever was already on its way, and then the listening. Bounded:
    // a clock that has not stopped would keep this draining for ever, and
    // a test that hangs says less than one that fails.
    for _ in 0..50 {
        let Ok(event) = events.recv_timeout(Duration::from_millis(100)) else {
            break;
        };
        app.handle(event);
        support::lay_out(&mut app, WIDTH, HEIGHT);
    }
    assert!(
        events.recv_timeout(Duration::from_millis(400)).is_err(),
        "the clock is still running with nothing to animate"
    );
}

/// A list of open documents says what is happening now, not what was
/// happening when it opened.
///
/// The rows are built once, and they have to be: building one asks git
/// about the whole tree and reads the notes off disk, which is not work a
/// frame can do. The mark that says an agent is at work was built with
/// them and went stale with them -- and the staleness was invisible,
/// because the turning frame comes from the ticker. A conversation whose
/// turn had ended went on spinning for as long as the reader kept the list
/// up, and one that started working while it was up never said a word.
///
/// Broken deliberately by taking the freshening out of the frame, which
/// leaves the mark turning after the turn it is about is over.
#[test]
fn the_list_of_open_documents_says_what_is_happening_now() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A turn that ends on its own, so it can end while the reader is
    // looking at the list rather than at the conversation.
    support::type_text(&mut app, "/run");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the command to start", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("Run the tests"))
        })
    });

    // The mark while it is working, which is what the list is for.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentList);
    let working = support::render(&mut app, WIDTH, HEIGHT);
    let row = |dump: &str| {
        rows(dump)
            .iter()
            .find(|row| row.contains("A conversation"))
            .unwrap_or_else(|| panic!("no row for the conversation:\n{dump}"))
            .to_string()
    };
    assert!(
        SPINNING.iter().any(|frame| row(&working).contains(*frame)),
        "the list says nothing about the agent at work in it:\n{working}"
    );

    // And the turn ends under the reader, with the list still up.
    let deadline = Instant::now() + patience();
    while app.talking() == obelus_agent::Talking::Thinking {
        assert!(Instant::now() < deadline, "the turn never ended");
        if let Ok(event) = events.recv_timeout(Duration::from_millis(200)) {
            app.handle(event);
        }
        support::lay_out(&mut app, WIDTH, HEIGHT);
    }
    let ended = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !SPINNING.iter().any(|frame| row(&ended).contains(*frame)),
        "the list is still turning for a turn that is over:\n{ended}"
    );
}

/// A tool call that is still running turns at the front of its row.
///
/// A picture of a cog says a tool was used; only movement says it is still
/// going, and the only movement Obelus had was one row at the foot of the
/// transcript saying the agent was thinking. That row is below whatever
/// the call is printing, so a reader watching a build could not see from
/// the row they were watching whether it had stalled -- the call wore the
/// same still glyph it would wear if it had.
///
/// It turns in the column the kind glyph has, so nothing on the row moves,
/// and the kind comes back when the call ends. The mark after the title
/// goes the other way: it says how the call went, and stays away while the
/// front is saying that it has not gone yet.
///
/// It makes three claims and was broken deliberately three times. Drawing
/// the kind glyph while the call runs leaves the row still. Taking the
/// frame from anywhere but the ticker leaves it the same glyph twice. And
/// leaving the state after the title while it runs puts a still mark back
/// on a row that is moving, which is the pair of marks this replaced.
#[test]
fn a_tool_call_that_is_still_running_turns_at_the_front_of_its_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // The ordinary turn: the agent reads a file and is still reading it
    // when it stops to ask permission for the next thing.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );

    let call = |app: &mut App, phase: u32| {
        app.phase_for_test(phase);
        let dump = support::render(app, WIDTH, HEIGHT);
        rows(&dump)
            .iter()
            .find(|row| row.contains("Read the file"))
            .unwrap_or_else(|| panic!("no row for the call:\n{dump}"))
            .to_string()
    };
    let running = call(&mut app, 0);
    assert!(
        SPINNING.iter().any(|frame| running.contains(*frame)),
        "the call that is still running is drawn as still as one that has stopped:\n{running}"
    );
    assert_ne!(
        running,
        call(&mut app, 3),
        "the mark on a running call does not turn"
    );
    assert!(
        !running.contains("Running"),
        "the row says it is running twice, once of them standing still:\n{running}"
    );

    // Answered, the call ends: the mark a tool call wears comes back --
    // without the glyphs, the one mark every kind shares -- and the end of
    // the row says how it went.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let ended = call(&mut app, 0);
    assert!(
        !SPINNING.iter().any(|frame| ended.contains(*frame)),
        "a call that has finished is still turning:\n{ended}"
    );
    assert!(
        ended.contains("+  Read the file"),
        "the call never got its mark back:\n{ended}"
    );
    assert!(
        ended.contains("Done"),
        "a call that finished does not say so:\n{ended}"
    );
}

/// Every frame of the mark that turns.
///
/// Which frame is on screen depends on how many ticks have landed, and a
/// test that pinned one would be a test about the machine it ran on.
const SPINNING: [&str; 10] = [
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}",
];

/// The mark a note wears when there is a conversation about it.
///
/// The plain one, because the glyphs are off until the reader turns them
/// on: where they are on it is `obelus_icons::ui::AGENT`.
const MARK: char = '*';

/// And the one it wears beside that while the conversation is waiting on
/// an answer -- where the glyphs are on, the same one the list of open
/// documents puts on a conversation with a question in it.
const WAITING: char = '?';

/// The first conversation opened after Obelus starts is taken up too.
///
/// Which note goes with which conversation is written down and survives a
/// restart -- and the first one the reader opened never got the benefit.
/// Going to a note's conversation starts the agent where none is running,
/// and that did the starting and then returned, before the line that looks
/// up the name written down beside the note. So the conversation asked for
/// nothing, the session the agent opens on its way up was handed to it as
/// the first one wanting one, and the reader got a blank page and an agent
/// that had forgotten the whole thing. Every restart, and only the first
/// note opened: the second found the agent already running and was taken
/// up correctly, which is why no test had ever seen it.
///
/// It makes two claims and was broken deliberately twice. Putting the
/// `return` back after `start_agent` asks for nothing, so nothing is
/// replayed and the page is a new conversation. And routing on the session
/// a conversation *has* rather than the one it asked for drops the replay:
/// an agent sends those words while it is still answering the request that
/// asked for them, so they arrive before the answer saying whose they are.
#[test]
fn the_first_conversation_opened_after_a_restart_is_taken_up() {
    let scratch = support::Scratch::new("agent-note-first");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456P\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let id = obelus_git::todo::NoteId::read("0123456P").expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                scratch.path(),
                obelus_agent::acp::sessions::Kept {
                    session: "s-old".to_string(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: None,
                },
            );
        },
    );

    // Started the way a reader's morning is: nothing running, an agent
    // named in the settings, and the conversation reached from the note.
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    let root = scratch.join("agents");
    obelus_agent::remember(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        "0.1",
        &root,
    )
    .expect("writing what was installed");
    app.agents_root_for_test(root);
    let file = scratch.join("config.toml");
    std::fs::write(&file, "agent = \"fake\"\n").expect("a settings file");
    app.config_file_for_test(file);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    talk_about_the_note(&mut app);
    // What the agent replays, which is what says it was asked for the
    // conversation written down rather than handed a new one. A load that
    // worked writes no note -- there is nothing to explain, the words are
    // simply back -- so the words are what to wait for.
    pump(&mut app, &events, "the old conversation", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("where we were"))
        })
    });

    let text = screen(&mut app);
    assert!(
        text.contains("what did we settle on"),
        "the reader's half of the old conversation did not come back:\n{text}"
    );
    assert!(
        !text.contains("Starting again"),
        "the conversation was thrown away rather than taken up:\n{text}"
    );
}

/// A conversation the agent has not got is forgotten, and an empty one is
/// never written down in its place.
///
/// This pair is what destroyed a reader's conversation, one morning at a
/// time. An agent does not keep a session nobody said anything in --
/// claude-agent-acp writes a conversation's file on its first turn -- so a
/// name minted and written down before the reader spoke was a name that
/// would not be there tomorrow. And it was written down over the note's
/// real conversation. The next start asked for it, was told there is no
/// such thing, opened another empty one and wrote *that* down. The
/// conversation went in the first round and every round after was the same
/// round again, each one looking like a fresh start rather than a loss.
///
/// So a name the agent has denied is taken out of the file, and a
/// conversation with nothing in it is not put in: there is nothing to come
/// back to, and saying there is takes the place of something there might
/// have been.
///
/// Broken deliberately twice. Leaving the dead name in the file keeps it
/// there to be asked for again. And writing a conversation down whatever
/// is in it puts the fresh empty one in its place, which is the same file
/// with a different name in it -- either way the note ends up pointing at
/// a conversation that is not there.
#[test]
fn a_conversation_the_agent_has_not_got_is_forgotten_rather_than_replaced() {
    let (scratch, mut app, events) = remembering("agent-forgets-dead", "0123456S", "s-gone", &[]);
    pump(
        &mut app,
        &events,
        "what became of the old conversation",
        settled,
    );
    let text = screen(&mut app);
    assert!(
        text.contains("Starting again"),
        "the agent was not asked for the conversation that is gone:\n{text}"
    );

    // With the conversation it was started again with in hand, so that
    // "nothing is written down" is a fact about what Obelus decided and
    // not about what has arrived yet.
    pump(&mut app, &events, "the conversation it started", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Nothing is written down against the note: the name that was there
    // is gone, because the agent said it has no such thing, and the one
    // opened in its place has nothing said in it to come back to.
    let id = obelus_git::todo::NoteId::read("0123456S").expect("a name");
    let kept = obelus_agent::acp::sessions::read(scratch.path())
        .remembered()
        .expect("the table");
    assert_eq!(
        kept.get(
            &obelus_agent::chats::ChatId::Note(id.clone()),
            "fake",
            scratch.path(),
        )
        .map(|kept| kept.session.clone()),
        None,
        "the note still points at a conversation nobody can reach"
    );

    // And once there is something in it, it is written down -- that is
    // what the file is for.
    support::type_text(&mut app, "/echo hello");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("hello"))
        })
    });
    let kept = obelus_agent::acp::sessions::read(scratch.path())
        .remembered()
        .expect("the table");
    assert!(
        kept.get(
            &obelus_agent::chats::ChatId::Note(id.clone()),
            "fake",
            scratch.path(),
        )
        .is_some(),
        "a conversation with something in it was not written down"
    );
}

/// The pointer takes hold of what was said, and a copy takes what is held.
///
/// A conversation is the half of Obelus a reader cannot type into, and
/// until this it was the half they could not take a copy out of either:
/// the pointer served the box alone, and everything the agent said was
/// behind an Obelus that had taken the terminal's own selection away.
///
/// What comes out is what is on the screen. The reader dragged across
/// rows, and rows are what a width made of what was said -- so the copy is
/// read back off them, marks and blank lines and all.
///
/// Broken deliberately by handing the press to the box's own mapping,
/// which answers for nothing outside it and so takes hold of nothing; or
/// by copying out of the box first, which is empty here and copies an
/// empty message over what the reader was pointing at.
#[test]
fn the_pointer_takes_hold_of_what_was_said() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH - 5)
                .iter()
                .any(|row| row.text().contains("heard you"))
        })
    });

    // The row the agent's words are on, and the columns its own text
    // occupies: the transcript draws a glyph and an indent before them.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let at = row_of(&dump, "heard you");
    let start = support::column_of(rows(&dump)[usize::from(at)], "heard you");
    let Ok(start) = u16::try_from(start) else {
        panic!("the answer is off the screen:\n{dump}");
    };

    // Dragged across the first word of it.
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: start,
        y: at,
    });
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Dragged,
        x: start + 5,
        y: at,
    });

    let (text, what) = app.chat().expect("a conversation").copied(WIDTH - 5);
    assert_eq!(what, "selection", "the copy did not take what was held");
    assert_eq!(text, "heard", "the copy is not what was dragged across");
}

/// A transcript scrolled away from its end says how to get back, and what
/// arrived while the reader was not looking.
///
/// The scrollbar beside it can only say how much there is. What somebody
/// who has scrolled up actually wants to know is whether the agent has
/// answered them yet -- and there was nothing on screen that said so, nor
/// anything naming the key that goes back.
///
/// The agent's own words and nothing else are counted: a turn is a dozen
/// tool calls and a paragraph, and how much machinery went past is not
/// news. Nor are Obelus's own notes, which are Obelus talking about the
/// conversation rather than anything said in it.
///
/// Broken deliberately three ways. Drawing the way back whatever the
/// window is doing leaves it sitting at the end of a conversation nobody
/// has scrolled. Counting every kind of thing said turns one answer into
/// "4 new messages" the moment it uses a tool. And putting the guard on
/// `Chat::handle_key` back the way it was -- shift or nothing -- makes
/// `ctrl+home` and `ctrl+end` do nothing at all, which is what they did:
/// the guard turned them away before the arms that were waiting for them,
/// and a conversation has no file for the editor to take them instead.
#[test]
fn a_transcript_scrolled_up_says_how_to_get_back() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // At the end, which is where it sits: nothing to say.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump).iter().any(|row| row.contains("ctrl+end")),
        "a conversation nobody has scrolled offers a way back:\n{dump}"
    );

    // Something to scroll, and scrolled away from the end of it.
    support::type_text(&mut app, "/filler");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "something to scroll", |app| {
        app.chat()
            .is_some_and(|chat| chat.rows(WIDTH - 5).len() > usize::from(HEIGHT))
    });
    support::press_control_key(&mut app, KeyCode::Home);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("To the end")),
        "nothing says how to get back:\n{dump}"
    );

    // And the agent says something while they are up there.
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH - 5)
                .iter()
                .any(|row| row.text().contains("heard you"))
        })
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("1 new message")),
        "nothing says the agent has answered:\n{dump}"
    );

    // Back to the end, and there is nothing left to say.
    support::press_control_key(&mut app, KeyCode::End);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump).iter().any(|row| row.contains("ctrl+end")),
        "the way back is still offered at the end:\n{dump}"
    );
}

/// A long conversation still says when the agent is working.
///
/// The row that says what is happening now is the last row of the
/// transcript, and the transcript follows its own end -- so it is only
/// ever on screen if the window agrees with the view about where the end
/// is. It did not: the following was settled against a width one column
/// wider than the one the rows are drawn at, and prose wrapped a column
/// wider makes fewer rows than the view then lays out. The window was
/// told the transcript was shorter than it is and followed the end of
/// something else, leaving the real last rows below the band.
///
/// Only on a long one. A short conversation has nothing wrapped, the two
/// counts agree, and everything is where it should be -- which is why
/// this hid behind every test and every quick try, and showed up on a
/// day's conversation taken up again: an agent working for half a minute
/// with nothing on screen saying so.
///
/// Broken deliberately by settling against `region.width - 4` again, the
/// view's own width being `width - 5`: the page then says nothing while
/// the agent works, which is the report this came from. The prose here is
/// lines of exactly the wider width -- seven words of eight and one of
/// nine -- because ordinary prose only lands on that column by luck, and
/// the first go at this test wrapped the same at both widths and passed
/// with the thing it covers broken.
#[test]
fn a_long_conversation_still_says_when_the_agent_is_working() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A day's worth of it, in paragraphs that wrap.
    support::type_text(&mut app, "/filler");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "something to scroll", |app| {
        app.chat()
            .is_some_and(|chat| chat.rows(WIDTH - 5).len() > usize::from(HEIGHT))
    });

    // And a turn the agent takes its time over, with the reader left at
    // the end of the transcript where the answer will appear.
    support::type_text(&mut app, "take it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to start", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("Thinking")),
        "the agent is working and the page does not say so:\n{dump}"
    );
}

/// A conversation taken up again says it is thinking when it is.
///
/// The row that says what is happening now is read off the handle, by the
/// session the conversation on screen is in -- so it is only ever right if
/// that conversation really holds the session the agent answered about.
#[test]
fn a_conversation_taken_up_again_says_it_is_thinking() {
    let (_scratch, mut app, events) = remembering("agent-thinks-again", "0123456T", "s-old", &[]);
    pump(&mut app, &events, "the old conversation", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("where we were"))
        })
    });

    // A turn the agent takes its time over, so it is still in flight when
    // the screen is looked at.
    support::type_text(&mut app, "take it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to start", |app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("Thinking")),
        "the conversation does not say it is thinking:\n{dump}"
    );
}

/// An agent that keeps a conversation but cannot replay it is asked for the
/// one it can do, and the reader is told why the page is empty.
///
/// `session/resume` takes a conversation up with its context intact and
/// sends none of it back. Which of the two to ask is read off the handshake
/// rather than found out by asking the fullest and reading the error: an
/// agent that says it cannot replay, sent `session/load`, costs a round
/// trip and gets its conversation thrown away as lost -- while it still
/// held every word of the context.
///
/// Broken deliberately by asking `LoadSessionRequest` whatever the agent
/// said: the fake agent has no answer for it, the conversation is put back
/// as lost, and this goes red.
#[test]
fn an_agent_that_can_only_resume_is_asked_to_resume() {
    let (_scratch, mut app, events) =
        remembering("agent-resumes", "0123456K", "s-old", &["only-resumes"]);
    // Until Obelus has something to say about the old conversation --
    // `Talking::Ready` is true as soon as the agent has spoken at all,
    // which is before it has answered about this one.
    pump(
        &mut app,
        &events,
        "what became of the old conversation",
        settled,
    );

    let text = screen(&mut app);
    assert!(
        text.contains("cannot send back what was said"),
        "nothing says why the page is empty:\n{text}"
    );
    // Taken up, not started again: the agent still has the context.
    assert!(
        !text.contains("Starting again"),
        "the conversation was thrown away rather than taken up:\n{text}"
    );
    // And it was resume that was asked for, not load: what the fake agent
    // replays to a load is on the page for anyone who sent one.
    assert!(
        !text.contains("where we were"),
        "Obelus asked to replay a conversation this agent cannot replay:\n{text}"
    );
}

/// An agent that can do neither is not asked at all.
///
/// It said so at the handshake. A request sent to be told that again is a
/// round trip spent learning nothing, and until the answer comes back the
/// reader is looking at a conversation that may or may not be there.
///
/// Broken deliberately by asking `LoadSessionRequest` whatever the agent
/// said: the fake agent answers it and replays, so the words come back and
/// this goes red.
#[test]
fn an_agent_that_can_do_neither_is_not_asked() {
    let (_scratch, mut app, events) =
        remembering("agent-forgets", "0123456M", "s-old", &["forgets"]);
    pump(
        &mut app,
        &events,
        "what became of the old conversation",
        settled,
    );

    // The note carries why, and the why is Obelus's own reading of the
    // handshake -- not an error the agent sent back. An agent asked
    // anyway refuses in its own words, and those would be here instead.
    let text = screen(&mut app);
    assert!(
        text.contains("Starting again"),
        "nothing says the old conversation was not there:\n{text}"
    );
    assert!(
        text.contains("take a conversation up again"),
        "the reason is not Obelus's own:\n{text}"
    );
    assert!(
        !text.contains("cannot replay"),
        "Obelus asked for a conversation the agent said it could not give:\n{text}"
    );
}

/// A command the agent asks for is run, and what it said comes back.
///
/// The five `terminal/*` methods want a process, not a screen: started,
/// its output read, its exit status waited for, and a way to stop it.
/// Obelus does not ask the reader first -- the agent asks, which is the
/// rule Obelus's own tools follow too -- and what it owes instead is that
/// the command is on the page and a key stops it.
///
/// Broken deliberately by declaring `terminal(false)` in the handshake: a
/// well-behaved agent stops asking and this goes red.
#[test]
fn a_command_the_agent_asks_for_is_run() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/run");
    support::press(&mut app, KeyCode::Enter);
    // Until the *agent* says what it read back, not until the words are
    // anywhere on the page: the row that shows the command carries them
    // too, which is the point of the test beside this one.
    pump(&mut app, &events, "what the agent read back", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("it said"))
        })
    });

    // Its output and its exit status, both as the command really gave
    // them: the agent read them back out of Obelus.
    let said = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .map(|row| row.text())
        .collect::<String>();
    assert!(
        said.contains("it said obelus-ran-this and ended 3"),
        "the command's own words and code did not come back: {said:?}"
    );
}

/// A command Obelus ran is on the page in the words it was run in, and
/// says how it ended.
///
/// The half Obelus owes for not asking before it runs one. The agent
/// decides whether to ask; Obelus decides that once it runs, the reader
/// sees the command line itself -- not the agent's title for it -- and
/// everything it printed, and how it came out.
///
/// Opened here, because a command that has ended folds away like
/// everything else a call carries: while it runs the call is open and the
/// output arrives under it, and afterwards it is a row and a key. What the
/// reader must be able to reach is what this is about.
///
/// And the number, which was nowhere. Obelus kept the exit code, handed it
/// to the agent when it asked, and showed the reader a mark saying only
/// that something had failed -- so `grep` matching nothing and a command
/// that is not installed looked alike, and one of those is an answer.
///
/// It makes three claims and was broken deliberately three times. Leaving
/// `Chat::running` uncalled carries the agent's title and nothing else.
/// Dropping the exit line loses how it came out. And answering the fold
/// with the reader's own word ignored keeps the call shut when they have
/// just opened it.
#[test]
fn a_command_is_on_the_page_in_the_words_it_was_run_in() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/run");
    support::press(&mut app, KeyCode::Enter);
    // Until the command has *ended*, not until its words are on the page:
    // the command line says them too, so a row carrying them is the row
    // that says what is being run.
    pump(&mut app, &events, "the command to finish", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH).iter().any(|row| {
                row.text().contains("Run the tests") && row.state.as_deref() == Some("failed")
            })
        })
    });

    // The call says it failed, because the command did -- and it is shut,
    // because it is over.
    let state = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .find(|row| row.text().contains("Run the tests"))
        .and_then(|row| row.state.clone());
    assert_eq!(
        state,
        Some("failed".to_string()),
        "a call whose command exited 3 does not say it failed"
    );

    // Opened by a press on its row, which is what a reader has.
    let shut = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&shut).iter().any(|row| row.contains(ran_command())),
        "a command that is over is still holding the page open:\n{shut}"
    );
    let y = row_of(&shut, "Run the tests");
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: app.editor_area_for_test().x + 6,
        y,
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen.iter().any(|row| row.contains(ran_command())),
        "the command the reader never typed is not on the page:\n{dump}"
    );
    // And what it printed, under it -- a row of its own, which is what
    // makes it the command's output rather than more of the title.
    assert!(
        screen
            .iter()
            .filter_map(|row| row.split_once('|'))
            .any(|(_, said)| said.trim() == "obelus-ran-this"),
        "what the command printed is not on the page:\n{dump}"
    );
    // And the number it came out with, which the mark cannot say.
    assert!(
        screen.iter().any(|row| row.contains("and exited 3")),
        "the page does not say what the command exited with:\n{dump}"
    );
}

/// The key that stops the agent stops what Obelus is running for it.
///
/// The processes are Obelus's -- it started them -- and an agent told to
/// stop is under no obligation to release a terminal on its way out. One
/// that did not would leave a build running that the reader has just said
/// they want stopped, with nothing on screen that could stop it.
///
/// Broken deliberately by taking the stopping out of `interrupt_agent`:
/// the command is still going and this goes red.
#[test]
fn the_key_that_stops_the_agent_stops_what_it_is_running() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the command to start", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("sleep 300"))
        })
    });
    assert!(
        app.anything_running(),
        "the command Obelus was asked to run is not running"
    );

    support::press(&mut app, KeyCode::Esc);
    assert!(
        !app.anything_running(),
        "a command the reader asked to stop is still running"
    );
}

/// A drag held past the bottom of a transcript keeps going.
///
/// A terminal reports a drag when the pointer moves and says nothing at
/// all while a held pointer is still, so a reader selecting their way down
/// a morning's conversation used to stop where the screen did: the rows
/// they wanted were below the edge, and leaning on the edge did nothing
/// because the pointer was no longer moving. What is remembered is that
/// the drag is out there, and every tick carries the conversation up under
/// it.
///
/// It goes through the same one function a notch of the wheel goes
/// through, which is why one piece of code serves the transcript and the
/// file both.
///
/// Broken deliberately by taking `drag_on` out of the tick, which leaves
/// the copy ending on the last row that was on screen when the pointer
/// reached the edge; or by measuring the edge against the whole region
/// rather than the transcript's own band, which puts the edge under the
/// writing box and never reports a drag past it at all.
#[test]
fn a_drag_held_past_the_bottom_of_a_transcript_keeps_selecting() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/filler");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "something to scroll", |app| {
        app.chat()
            .is_some_and(|chat| chat.rows(WIDTH - 5).len() > usize::from(HEIGHT) * 2)
    });
    support::press_control_key(&mut app, KeyCode::Home);

    // From the first row of the answer, down past the bottom of the
    // screen: the reader has run out of room and is leaning on the edge.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let at = row_of(&dump, "aaaaaaaa");
    let start = support::column_of(rows(&dump)[usize::from(at)], "aaaaaaaa");
    let Ok(start) = u16::try_from(start) else {
        panic!("the answer is off the screen:\n{dump}");
    };
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: start,
        y: at,
    });
    // Onto the writing box, which is where a reader dragging down the
    // transcript actually runs out of transcript: the screen goes on below
    // it, so an edge measured against the whole region is never reached.
    let below = row_of(&dump, "To the end") + 1;
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Dragged,
        x: start,
        y: below,
    });
    let held = |app: &App| {
        app.chat()
            .expect("a conversation")
            .copied(WIDTH - 5)
            .0
            .lines()
            .count()
    };
    let stopped = held(&app);

    for _ in 0..5 {
        app.handle(Event::Tick);
    }
    let carried = held(&app);
    assert!(
        carried > stopped + 5,
        "the copy stopped at {stopped} rows while the pointer was held past the edge, \
         reaching only {carried}:\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
}

/// A heading the transcript folds under opens when it is clicked.
///
/// Three kinds of row fold: the heading over a run of tool calls, the one
/// over a piece of thinking, and the one over the agent's plan. All three
/// wear the mark that says so, and a mark like that is clicked everywhere a
/// reader has met one.
///
/// It costs the selection nothing, and not by luck. Every row that folds is
/// one Obelus drew itself rather than anybody's words, so it has no place
/// in what was said -- a press on one already meant nothing but "let go".
/// The rows that *do* carry words are not the rows that fold.
///
/// Broken deliberately by letting the press fall through to taking hold,
/// which is what it did: the run stays shut and the reader is left having
/// cleared their selection for nothing.
#[test]
fn a_heading_the_transcript_folds_under_opens_when_it_is_clicked() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/many");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Four reads under one heading, shut.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let at = row_of(&dump, "4 Files");
    assert!(
        !rows(&dump).iter().any(|row| row.contains("Read src/app")),
        "the run is open before anybody asked:\n{dump}"
    );

    // Clicked on, it opens.
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 8,
        y: at,
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("Read src/app")),
        "the click did not open the run:\n{dump}"
    );
    // And nothing was taken hold of on the way.
    assert!(
        app.chat().is_some_and(|chat| !chat.holding()),
        "the click took hold of the heading"
    );

    // And again, it shuts.
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 8,
        y: at,
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump).iter().any(|row| row.contains("Read src/app")),
        "the same click did not shut it again:\n{dump}"
    );
}

/// A conversation taken up again is told where Obelus's tools are.
///
/// Obelus serves its own tools -- the ones an agent finishes a note with --
/// over HTTP on a port the machine hands out when the process starts, so the
/// address is a different one every run. A conversation outlives the run it
/// was started in: that is the whole point of asking for it again by name.
///
/// Told only at `session/new`, the agent went on calling the address it was
/// given the first time, which died with the process that gave it. The notes
/// worked all morning and then stopped, and from the agent's side the tools
/// had simply gone -- it said so, and nothing on Obelus's side had anything
/// to say about it, because from here the server was still listening and
/// nobody had called.
///
/// So the offer goes with every way of taking a conversation up, the same as
/// it goes with opening one. The protocol carries it on all three -- new,
/// load and resume.
///
/// Broken deliberately by taking the tools off the load request, which is
/// what it did: the fixture then answers `tools=missing`.
#[test]
fn a_conversation_taken_up_again_is_told_where_the_tools_are() {
    let (_scratch, mut app, events) =
        remembering_with_tools("agent-resume-tools", "0123456V", "s-old", &[]);
    pump(&mut app, &events, "the conversation", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("tools="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("tools=given"),
        "the agent was not told where Obelus's tools are:\n{text}"
    );
}

/// An agent is told what the note says when it does not know it, and not
/// otherwise.
///
/// One rule for three cases that used to be three rules. The paragraph
/// Obelus sends is the whole of what the note says, where it points, its
/// name, and what to call when its work is done; it goes in front of the
/// reader's own words, and what Obelus told the agent is written down
/// beside the session's name. So the question asked before every message
/// is "does this agent know what the note says now", and the answer is
/// worked out from the note's own file, which is the reader's to rewrite
/// whenever they like.
///
/// A conversation taken up again is told nothing: Obelus asked for it by
/// name because the agent kept every word of it, and telling it again put
/// a page of instructions about a note it had read in front of the first
/// thing the reader said on coming back, every morning.
///
/// A note rewritten since gets the difference and nothing else -- not its
/// name and not the tools, neither of which has changed. The reader
/// rewrites a note because they thought better of it overnight, and the
/// protocol gives a client no channel to a conversation but the next
/// prompt: what is not said in one is not said at all.
///
/// And once. What the agent was told is written down as the prompt goes,
/// so the message after carries nothing.
///
/// The fourth case is the same rule read the other way and is held up by
/// `a_conversation_the_agent_has_forgotten_is_started_again`: a session
/// the agent refuses has been told nothing, because the words that told it
/// went with the session.
///
/// Broken deliberately two ways. Taking out the check for "what it was
/// told is what the note says now" sends the whole paragraph with every
/// message, to an agent that has it. And not writing down what was told as
/// it goes tells it the note has been rewritten again and again, once per
/// message, for one rewrite. Offering the box's words whatever the agent
/// has been told leaves them in a conversation that knows its note.
#[test]
fn an_agent_is_told_what_the_note_says_only_when_it_does_not_know_it() {
    let (scratch, mut app, events) = remembering("agent-resume-opening", "0123456S", "s-old", &[]);
    // The conversation the agent kept, back on the page: what it replays is
    // proof it has the words that told it about the note.
    pump(&mut app, &events, "the conversation", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("where we were"))
        })
    });
    // Nothing offered in the box either: what it would offer to say is a
    // reply to being told the note, and this agent has been.
    let text = screen(&mut app);
    assert!(
        app.chat().is_some_and(|chat| chat.suggestion().is_none()) && !text.contains("Fill it in"),
        "a conversation that knows its note was offered words for it:\n{text}"
    );
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=1") && text.contains("first=reader"),
        "the agent was told about the note again:\n{text}"
    );
    // And the reader is not told Obelus spoke in their name, because it
    // did not.
    assert!(
        !text.contains("Told the agent what this conversation is about"),
        "the transcript says Obelus said something it did not:\n{text}"
    );

    // Then the reader rewrites the note, which is a file of theirs and
    // theirs to rewrite -- in Obelus, in their own editor, between any two
    // messages. Now the agent is out of date, and the next thing said
    // carries the difference.
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456S\"\nsaid = \"a note, thought better of\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    support::type_text(&mut app, "/blocks after");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text().contains("first=rewritten"))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2") && text.contains("first=rewritten"),
        "the agent was not told the note had changed:\n{text}"
    );
    assert!(
        text.contains("Told the agent the note has been rewritten"),
        "nothing on the page says Obelus spoke in the reader's name:\n{text}"
    );

    // Once. What it was told is written down as it goes, so the message
    // after it carries nothing again.
    //
    // Counted rather than looked for: the answer to the first message says
    // `first=reader` too, so finding one on the page says nothing at all
    // about the third.
    let answers = |app: &App| -> Vec<String> {
        app.chat()
            .expect("a conversation")
            .rows(WIDTH)
            .iter()
            .map(|row| row.text())
            .filter(|text| text.contains("blocks="))
            .collect()
    };
    support::type_text(&mut app, "/blocks once more");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the third answer", |app| {
        answers(app).len() == 3
    });
    let all = answers(&app);
    assert_eq!(
        all[2].trim(),
        "blocks=1 first=reader",
        "the agent was told the note had been rewritten a second time: {all:?}"
    );
}

/// The key that goes from a conversation to the notes lands the caret in
/// them.
///
/// `alt+t` is the way across, and the conversation's own status row names
/// it. What it reaches is the notes as the reader left them -- going back
/// to an open page on purpose, because reading the file again would lose
/// their place in the list -- so a page whose box had been taken away was
/// one this key delivered the reader into and left them stuck in.
///
/// Which is how it was met: escape in the notes took the box, and then this
/// key went back to the page that no longer had one. The fault was escape's
/// and is fixed where escape is, but the route is worth holding down
/// separately: it is the one the reader was on, and it crosses two
/// documents and a view that deliberately does not read the file again.
///
/// Broken deliberately by having escape write the note down the way leaving
/// it does, which takes the box: the caret is gone on the way back and no
/// key here can return it.
#[test]
fn the_key_from_a_conversation_to_the_notes_lands_the_caret_in_them() {
    let scratch = support::Scratch::new("agent-alt-t");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Y\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, events) = talking();
    app.working_directory_for_test(scratch.path().to_path_buf());
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let caret = |app: &mut App| {
        let dump = support::render(app, WIDTH, HEIGHT);
        dump.split("-- cursor --")
            .nth(1)
            .unwrap_or("?")
            .trim()
            .to_string()
    };

    // Across to the notes, which puts the caret in one of them.
    support::press_alt(&mut app, 't');
    assert!(app.notes().is_some(), "alt+t did not reach the notes");
    let in_a_note = caret(&mut app);
    assert_ne!(in_a_note, "none", "alt+t reached the notes with no caret");

    // Escape, back to the conversation, and across again: the page is the
    // one the reader left, so the caret has to still be in it.
    support::press(&mut app, KeyCode::Esc);
    assert_eq!(
        caret(&mut app),
        in_a_note,
        "escape took the caret out of the note"
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    assert!(app.chat().is_some(), "the conversation is not back");
    support::press_alt(&mut app, 't');
    assert_eq!(
        caret(&mut app),
        in_a_note,
        "coming back to the notes left the reader with no caret"
    );
}

/// A press on a card's answer answers it.
///
/// A card is a question that has taken part of the screen and is waiting,
/// and every row of it is a thing the reader answers with. Until now a
/// press anywhere on one did nothing: the card covers the box, so the box
/// said the press was not its own, and the transcript said the same because
/// the card had taken that part of the band. A question with `Yes` and `No`
/// on it and no way to press either.
///
/// So a press does what the key does on the row it landed on -- down the
/// card's own path, rather than a second way to answer. Unlike a list drawn
/// over a file there is nothing here to browse past: the rows *are* the
/// answers.
///
/// Broken deliberately by handing the press back to the transcript, which
/// leaves the question waiting.
#[test]
fn a_press_on_a_cards_answer_answers_it() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/twice");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking_permission);

    // The row `Yes` is drawn on, and a press on it.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let y = row_of(&dump, "Yes");
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 4,
        y,
    });
    assert!(
        !App::is_asking_permission(&app),
        "the press did not answer the question:\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
    // And the agent got it: it is waiting on the answer, so the turn ends
    // only if one arrived. Waiting for that is the assertion.
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
}

/// A press on a setting on the status row does what its key does.
///
/// While a conversation is what the screen is showing, the status row is
/// the conversation's: a word for each of the agent's settings, saying what
/// the session is set to. Each is one key away -- a switch flips, one with
/// a list behind it opens the list -- and the row is the only thing on
/// screen saying so. A press on one did nothing: the row's own handler
/// knows about the three boxes a reader types in, and says a press is not
/// its own when none of them is showing.
///
/// The press goes to the setting and then down the key's own path, so a
/// switch on the row and the same switch on the settings page cannot come
/// apart.
///
/// Broken deliberately by handing the press back, which leaves the session
/// set as it was.
#[test]
fn a_press_on_a_setting_on_the_status_row_does_what_its_key_does() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.kind == obelus_agent::acp::Kind::Switch)
    });
    let switch = app
        .agent_settings()
        .iter()
        .position(|setting| setting.kind == obelus_agent::acp::Kind::Switch)
        .expect("a switch among the settings");
    let was = app.agent_settings()[switch].current.clone();

    // The word it is drawn as, and where on the row that is.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let status = rows(&dump)[usize::from(HEIGHT) - 1].to_string();
    let name = app.agent_settings()[switch].name.clone();
    let x = support::column_of(&status, &name);
    app.handle(Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: u16::try_from(x).expect("a column"),
        y: HEIGHT - 1,
    });
    pump(&mut app, &events, "the setting to change", |app| {
        app.agent_settings()
            .get(switch)
            .is_some_and(|setting| setting.current != was)
    });
}

/// Shift and home hold the line the reader is writing, and the box draws it.
///
/// Shift extends and never names a command -- one rule, everywhere -- and
/// the box was the one place in Obelus where it named one: the pair sent
/// the whole transcript to the beginning of the conversation and held
/// nothing. The two ends of a long transcript are control's, which is what
/// the rule over the box already says in as many words.
///
/// Drawn as well as held, because the box had never drawn a selection at
/// all -- it was given the strings of its rows and not what was marked on
/// them -- so even the one a drag makes was invisible. And drawn in the
/// colour the file and the transcript use for the same fact, which is the
/// point of asking one question in one place.
///
/// Broken deliberately three ways: by putting `window.home()` back on the
/// arm, which moves the transcript and leaves the words unmarked; by
/// handing `writing` the plain strings again, which holds the line and
/// shows nothing; and by painting it in some other colour, which makes two
/// answers to "what does held look like".
#[test]
fn shift_and_home_hold_the_line_in_the_box() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // A turn's worth of transcript, so that there is something in the
    // other half to compare the mark against.
    support::type_text(&mut app, "what is this file");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the permission request",
        App::is_asking_permission,
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "a message I typed");
    support::lay_out(&mut app, WIDTH, HEIGHT);

    support::press_shift(&mut app, KeyCode::Home);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let dump = support::render(&mut app, WIDTH, HEIGHT);

    // The conversation stayed where it was: the way back is only ever
    // drawn once the reader has left the end of it.
    assert!(
        !dump.contains("To the end"),
        "shift and home moved the transcript instead of holding a line:\n{dump}"
    );
    // And every cell of the line is marked, in one colour -- one that is
    // not simply the row's own ground, which is what "nothing is drawn"
    // looks like from here.
    let held = behind_words(&dump, "a message I typed");
    assert_eq!(
        held.len(),
        1,
        "the held line is not marked all through: {held:?}\n{dump}"
    );
    assert_ne!(
        held.first().map(String::as_str),
        Some(behind(&dump, "a message I typed").as_str()),
        "nothing is drawn behind the held line:\n{dump}"
    );

    // The same colour the transcript marks a hold in, because it is the
    // same fact about the same screen. Up twice: the first press is what
    // lets go of what the box is holding.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Up);
    for _ in 0..7 {
        support::press_shift(&mut app, KeyCode::Left);
    }
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let there = behind_words(&dump, "allowed");
    assert_eq!(
        there, held,
        "the box marks a hold differently from the transcript:\n{dump}"
    );
}

/// What a reader takes hold of in the transcript, `ctrl+c` copies.
///
/// It did not, and the way it did not is the interesting part: the key was
/// found, the command was found, and the gate in front of it said no in
/// silence. `copy-selection` asked for `ACaret` -- somewhere with a caret
/// in it -- which a conversation answers only while the keys are in the
/// box. Taking hold of the transcript moves them out of it, so selecting
/// and copying were mutually exclusive, and nothing said so.
///
/// Broken deliberately by putting `Requires::ACaret` back on
/// `SelectionCopy`: the selection is still made, `ctrl+c` still does
/// nothing, and the note stays empty.
#[test]
fn what_is_held_in_the_transcript_is_copied() {
    // The provider is one place for the whole process, so a test that
    // takes neither the turn nor a provider reaches into the clipboard of
    // whoever ran it.
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/many");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Up leaves the box for the transcript, and shift holds what it passes
    // over -- which is the only way there is to say "this", and the reason
    // the keys are not in the box any more.
    support::press(&mut app, KeyCode::Up);
    for key in [KeyCode::Left, KeyCode::Left, KeyCode::Up] {
        support::press_shift(&mut app, key);
    }
    let width = obelus_ui::chat::reading_width(app.editor_area_for_test());
    let held = app
        .chat()
        .expect("the conversation")
        .held_text(width)
        .expect("something held in the transcript");

    support::press_control(&mut app, 'c');
    assert_eq!(
        app.note(),
        Some("Copied selection"),
        "ctrl+c over a held transcript did nothing, and said nothing about it"
    );
    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some(held.as_str()),
        "something other than what was held went to the clipboard"
    );
}

/// A conversation opens on what the reader said conversations should open
/// on, and says nothing about having done it.
///
/// The whole point of the group on the settings page: a standing choice
/// that the next conversation is already in, rather than three keys to
/// press every time one is opened. Silent, because it is not something the
/// reader did just now -- the row at the foot of the conversation says
/// what it is on either way.
///
/// Broken deliberately by taking the call to
/// `start_the_session_on_what_was_chosen` out of `on_acp`: the conversation
/// opened on the agent's own model and the settings file might as well not
/// have existed.
#[test]
fn a_conversation_opens_on_what_the_reader_chose() {
    let (mut app, events) = wired();
    // What the reader has said about this agent, before it is started:
    // `fake` is the name the fixture is talked to under.
    let mut config = obelus_config::Config::default();
    config.set_agent_default("fake", "model", "careful");
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();

    pump(&mut app, &events, "the model to be the chosen one", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "model" && setting.current == "careful")
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let status = screen[screen.len() - 1].to_string();
    assert!(
        status.contains("Careful"),
        "the row does not say the conversation is on it:\n{dump}"
    );
    // And the transcript says nothing about it: this is a standing choice,
    // not a thing the reader did in this conversation.
    assert!(
        !support::text_block(&dump).contains("Model:"),
        "the transcript was written in about a choice made elsewhere:\n{dump}"
    );
}

/// A value the agent has stopped offering is not sent to it.
///
/// A settings file outlives an agent's versions: what the reader chose a
/// month ago may not be among the values the agent offers today, and
/// sending it would be Obelus asking for something it has been told does
/// not exist. The row on the settings page is where that is dealt with,
/// because the reader is the only one who can.
///
/// And it is said, once, in the conversation: the settings page marks the
/// choice, but a reader in a conversation is not looking at that page, and
/// a row showing something other than what they chose is a question with no
/// answer on screen.
///
/// Broken deliberately by taking out the check that the value is among the
/// ones offered: the agent was asked for `brilliant`, answered with the
/// settings unchanged, and nothing anywhere said why. And by taking out the
/// check against `started_on`, which is what makes each setting settled
/// once: the agent restates every setting whenever one moves, and the
/// sentence is said a second time.
#[test]
fn a_value_the_agent_no_longer_offers_is_not_sent() {
    let (mut app, events) = wired();
    let mut config = obelus_config::Config::default();
    // One the agent has never heard of, and one it has. The second is
    // what makes this test settle: it comes after the model in the
    // agent's own order, so an agent that has answered about it has
    // already answered about anything Obelus sent before it -- and
    // waiting on the model itself would be waiting for a message Obelus
    // is supposed not to send.
    config.set_agent_default("fake", "model", "brilliant");
    config.set_agent_default("fake", "allow_all", "on");
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();

    pump(&mut app, &events, "the switch to be on", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "allow_all" && setting.current == "on")
    });
    // Still on the agent's own, because Obelus never asked for the other.
    assert_eq!(
        app.agent_settings()
            .iter()
            .find(|setting| setting.id == "model")
            .map(|setting| setting.current.as_str()),
        Some("fast"),
        "a value the agent does not offer was sent anyway"
    );
    // Wrapped to the transcript's width, so the first of its rows and then
    // the second.
    let said = "No longer offered by fake: brilliant for Model, so this conversation is on";
    let times = app.chat().map_or(0, |chat| {
        chat.rows(WIDTH)
            .iter()
            .filter(|row| row.text().contains("No longer offered by"))
            .count()
    });
    assert!(
        said_in_transcript(&app, said) && said_in_transcript(&app, "Fast"),
        "the conversation does not say why it is not on what was chosen:\n{}",
        screen(&mut app)
    );
    assert_eq!(times, 1, "said more than once:\n{}", screen(&mut app));

    // And not again when the agent says what it is set to afterwards,
    // which it does with every setting at once: asked to be quick, the
    // fixture says what its settings are, unasked, before it ends the turn.
    support::type_text(&mut app, "do it quickly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let times = app.chat().map_or(0, |chat| {
        chat.rows(WIDTH)
            .iter()
            .filter(|row| row.text().contains("No longer offered by"))
            .count()
    });
    assert_eq!(
        times,
        1,
        "said again when the agent restated its settings:\n{}",
        screen(&mut app)
    );
}

/// The conversation opened only to ask what the agent offers is not taken
/// for a real one, whatever the agent says about it afterwards.
///
/// An agent writes about a session in the same breath as the answer that
/// names it, on either side of it. The fixture says what it can be set to
/// just before, and what it takes with a slash just after -- and those
/// words used to be answered as if a conversation held the session: the
/// reader's standing choice sent to a session the agent was about to be
/// asked to delete, and the session kept in Obelus's mirror for good.
///
/// Deliberate break: take the guard out of the `Update::Settings` arm in
/// `on_acp`, and the word before the answer -- which nothing yet knows is
/// about a session thrown away -- is answered: the log has
/// `session/set_config_option s-1`. The other half, a word arriving after
/// the session has been let go, this cannot make happen on demand: here
/// the word after the answer still reaches the mirror before the answer
/// does, and is cleared with the rest. `Talk`'s own test feeds it both
/// orders instead.
#[test]
fn a_conversation_opened_to_ask_is_not_answered_as_one() {
    let (mut app, events) = wired();
    let root = agents_root_for("thrown");
    std::fs::create_dir_all(&root).expect("the root");
    app.agents_root_for_test(root.clone());
    let log = root.join("asked.log");
    let mut config = obelus_config::Config::default();
    config.set_agent_default("fake", "model", "careful");
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "tells-settings".to_string(),
            format!("log={}", log.display()),
        ],
    );
    app.ask_what_the_agent_offers_for_test();

    asked(&mut app, &events, &log, "session/delete s-1");
    let asked = || std::fs::read_to_string(&log).unwrap_or_default();
    // And a moment more, for whatever the agent wrote after the answer to
    // reach the loop and be answered, if it is going to be.
    settle(&mut app, &events, Duration::from_millis(300));
    assert!(
        !asked().contains("session/set_config_option s-1"),
        "a choice was sent to a conversation nobody holds:\n{}",
        asked()
    );
    assert!(
        !app.agent_holds_for_test("s-1"),
        "the conversation opened to ask is still held"
    );
}

/// The fixture's entry in the registry, as the agents page is handed it.
fn the_fixture_is_listed(app: &mut App) {
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "fake".to_string(),
            name: "The fixture".to_string(),
            version: "1.0.0".to_string(),
            description: "Plays one conversation".to_string(),
            authors: Vec::new(),
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "fake@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));
}

/// What an install leaves: how to start it, which here is the fixture.
fn the_fixture_is_installed(root: &Path) {
    obelus_agent::remember(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        "1.0.0",
        root,
    )
    .expect("the record");
}

/// Activating an agent asks it what it can be set to, on a conversation of
/// its own, and leaves the reader's alone.
///
/// The settings page lists what an agent offers, and the protocol says it
/// in the answer to `session/new` and nowhere else -- `initialize` carries
/// the capabilities and the ways to sign in, and neither of those is this.
/// So Obelus asks, the moment the reader chooses the agent, rather than
/// leaving them to find out that a page about conversations needs a
/// conversation first.
///
/// On one of its own, because the alternative is a conversation of theirs
/// existing, being named, or being written in because a settings page
/// wanted a list -- which the log shows: a second session, let go, and the
/// reader's untouched.
///
/// Deliberate break: drop the call to `ask_what_the_agent_offers` from
/// `activate_agent`, and nothing asks -- opening the page did not, with no
/// agent chosen yet -- so the wait for `s-2` to be let go gives up.
#[test]
fn activating_an_agent_asks_it_what_it_can_be_set_to() {
    use obelus_command::Command;

    let root = agents_root_for("asks");
    std::fs::create_dir_all(&root).expect("the root");
    let log = root.join("asked.log");
    let (mut app, events) = playing(&[&format!("log={}", log.display())]);
    app.agents_root_for_test(root.clone());
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    the_fixture_is_installed(&root);
    let conversation = support::render(&mut app, WIDTH, HEIGHT);

    // Chosen the way a reader chooses one: on the agents page, on its card.
    the_fixture_is_listed(&mut app);
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().agent.as_deref(), Some("fake"), "not activated");

    // Its own conversation, opened and let go: `s-1` is the reader's.
    asked(&mut app, &events, &log, "session/delete s-2");
    pump(&mut app, &events, "what it offers", |app| {
        app.agent_offering()
            .is_some_and(|offering| offering.offers.iter().any(|offer| offer.id == "model"))
    });

    // And the reader's own conversation is where they left it: the asking
    // was done somewhere else, and nothing was said in theirs.
    support::press(&mut app, KeyCode::Esc);
    assert_eq!(
        support::text_block(&support::render(&mut app, WIDTH, HEIGHT)),
        support::text_block(&conversation),
        "the conversation changed under the reader"
    );
}

/// The settings page asks the agent what it can be set to every time it
/// opens, and says it is asking until it hears.
///
/// Every time, because an update changes what an agent offers and a list
/// kept from before it answers for a version that is gone. And "asking" in
/// words while it waits, which is not the same thing as "it has not said":
/// the second is what an agent that would not answer leaves.
///
/// Deliberate break: take the call to `ask_what_the_agent_offers` out of
/// `open_settings`. The page says it has heard nothing and the wait for
/// the list gives up.
#[test]
fn the_settings_page_asks_what_the_agent_offers_each_time_it_opens() {
    use obelus_command::Command;

    let root = agents_root_for("page-asks");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let log = root.join("asked.log");
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
        ],
    );

    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    let silence = app
        .agent_offering()
        .and_then(|offering| offering.silence)
        .unwrap_or_default();
    assert_eq!(silence, "Asking fake what it can be set to");
    pump(&mut app, &events, "what it offers", |app| {
        app.agent_offering()
            .is_some_and(|offering| !offering.offers.is_empty())
    });

    // Again, on the next opening: what was heard is drawn meanwhile.
    support::press(&mut app, KeyCode::Esc);
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    assert!(
        app.agent_offering()
            .is_some_and(|offering| !offering.offers.is_empty()),
        "what was heard is not drawn while the page asks again"
    );
    asked(&mut app, &events, &log, "session/delete s-2");
}

/// Installing an agent again throws away what the version before it
/// offered.
///
/// An update is exactly what makes the list stale, so the copy kept in
/// memory goes with it rather than going on answering for a version that
/// is gone. Deliberate break: take the clearing out of `on_installed`, and
/// the old list is still handed out.
#[test]
fn installing_an_agent_again_forgets_what_it_offered() {
    let (mut app, _events) = wired();
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.agent_offers_for_test(
        "fake",
        vec![obelus_agent::acp::Setting {
            id: "model".to_string(),
            name: "Model".to_string(),
            about: None,
            values: Vec::new(),
            current: String::new(),
            kind: obelus_agent::acp::Kind::Select,
            category: obelus_agent::acp::Category::Other,
            legacy: false,
        }],
    );
    assert!(
        app.agent_offering()
            .is_some_and(|offering| !offering.offers.is_empty()),
        "nothing was heard, so this proves nothing"
    );
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "fake".to_string(),
        failure: None,
    }));
    assert!(
        app.agent_offering()
            .is_some_and(|offering| offering.offers.is_empty()),
        "what the old version offered is still handed out"
    );
}

/// Choosing another agent stops the one that was running and lets its
/// conversations go.
///
/// A session is a name one agent gave to something. The agent that gave it
/// has been stopped, so nothing can ever be asked about it again -- and a
/// conversation still holding one asks nobody for a new one, because
/// holding one is how Obelus knows it has one. The reader was then left
/// with a conversation they could type in and never send from.
///
/// It was also left talking to the old agent: `start_agent` only starts one
/// where there is none, so the process nobody had chosen went on answering
/// every conversation, with the card on the settings page saying the other
/// one was active.
///
/// Broken deliberately by taking the `stop_agent`/`let_the_conversations_go`
/// pair back out of `activate_agent`: the conversation kept its session,
/// and what answered the next message was the agent the reader had just
/// stopped choosing.
#[test]
fn choosing_another_agent_stops_the_one_that_was_running() {
    use obelus_command::Command;

    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    assert!(app.agent_settings().len() > 2, "nothing was running");

    // Another agent, installed and chosen. Its command need not work: what
    // is being watched is what happens to the one that was running.
    let root = agents_root_for("switch");
    app.agents_root_for_test(root.clone());
    obelus_agent::remember(
        "other",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
        "1.0.0",
        &root,
    )
    .expect("the record");
    app.handle(Event::Agent(obelus_agent::Event::Registry {
        agents: vec![obelus_agent::Agent {
            id: "other".to_string(),
            name: "The other one".to_string(),
            version: "1.0.0".to_string(),
            description: "Somebody else".to_string(),
            authors: Vec::new(),
            license: "MIT".to_string(),
            website: None,
            icon: None,
            distribution: obelus_agent::Distribution::Node {
                package: "other@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        }],
        failure: None,
    }));
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().agent.as_deref(), Some("other"));

    // The one that was running is not the one being talked to any more,
    // and its settings went with it.
    support::press(&mut app, KeyCode::Esc);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        support::text_block(&dump).contains("no longer the one Obelus talks to"),
        "the conversation was left holding a session nobody can reach:\n{dump}"
    );
}

/// A note whose conversation another Obelus has open says so, and the key
/// does not open a second one.
///
/// Obelus does not split its window, so two of them on one project is the
/// ordinary case -- and a conversation is not a thing two may have open at
/// once. The agent takes one prompt turn at a time and the queue that keeps
/// it to one lives in a process, so a second process prompting the same
/// conversation walks straight past it: no `2 waiting` anywhere, because the
/// two Obelus cannot see each other's queues.
///
/// The claim another Obelus holds is a lock on a file of its own, which is
/// what this takes out from under Obelus: a lock belongs to the open file
/// rather than to the process, so one taken here is as much somebody else's
/// as one taken in another window.
///
/// Two deliberate breaks, which are the two halves of it. Opening the
/// conversation whether or not `chats::claim` answered lets the second
/// window in -- `app.chat()` comes back `Some`. And offering `Talk` in the
/// foot regardless leaves the page promising a key that does nothing.
///
/// What it does *not* cover is a claim nobody holds reading as one, because
/// here the file exists and is locked and the two answers agree. That is
/// `chats::tests::a_file_nobody_holds_is_not_a_claim`, where they do not.
#[test]
fn a_conversation_another_obelus_has_open_is_not_opened_again() {
    let scratch = support::Scratch::new("agent-note-elsewhere");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456T\"\nsaid = \"somebody else has this\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let id = obelus_git::todo::NoteId::read("0123456T").expect("a name");

    // The other Obelus, holding it for as long as this is held.
    let theirs = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(id.clone()),
    )
    .expect("their claim");

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Elsewhere),
        "the row does not say the conversation is somebody else's"
    );

    // And the key on it opens nothing, because the claim is the answer and
    // the claim is not this Obelus's to have.
    support::press_alt(&mut app, 'a');
    assert!(
        app.chat().is_none(),
        "a second window was let into the conversation"
    );
    // And the foot does not offer the key it will not honour: the lock
    // beside the note says it, and the foot says it again by having nothing
    // to say. What the status row says is where it is being talked about,
    // which here is this same checkout.
    let dump = support::render(&mut app, WIDTH, 18);
    let text = support::text_block(&dump).to_string();
    let foot = text
        .lines()
        .find(|line| line.contains("Another"))
        .unwrap_or_default();
    assert!(
        !foot.contains("Talk"),
        "the foot offers a key that does nothing here:\n{dump}"
    );
    assert!(
        text.lines()
            .last()
            .is_some_and(|status| status.contains("Talked about in another window")),
        "the status row does not say the note is talked about elsewhere:\n{dump}"
    );

    // Given up, it is the reader's again: they closed it in the other
    // window and this one does not have to be restarted. Heard rather than
    // noticed -- the file going is what a watcher reports.
    drop(theirs);
    the_claims_changed(
        &mut app,
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(id.clone()),
    );
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Not),
        "the note is still somebody else's after they let it go"
    );
    let dump = support::render(&mut app, WIDTH, 18);
    assert!(
        dump.contains("Talk"),
        "the key is still held back after the other Obelus let go:\n{dump}"
    );
    support::press_alt(&mut app, 'a');
    assert!(
        app.chat().is_some(),
        "the conversation could not be opened after it was given up"
    );
}

/// A note another Obelus is talking about is read here, not changed.
///
/// The claim is what says somebody is standing in that conversation, and
/// taking the note away is the one change that destroys one. The words, the
/// box and the depth go with it: the lock says the row is not this window's,
/// and a rule that held for one key and not the next is not a rule a reader
/// could learn.
///
/// The caret stays in it. It is the only mark of where the reader is
/// standing -- this page has no selected-row colour, a box is marked by the
/// caret sitting in it -- so taking it away would leave them unable to tell
/// which note they were on. What says the next key will do nothing is the
/// foot, where the keys that would change it stop being offered: the reader
/// is told before they press, which is the rule the palette follows.
///
/// Broken deliberately one key at a time -- the guard in `take_note_away`,
/// in `can_shift`, in `move_over`, in `alt+space`'s arm, in the line break's
/// arm, or in the box's own arm -- and this goes red on whichever one was let
/// through. The note below
/// is what says the page did not simply go read-only.
#[test]
fn a_note_another_obelus_is_talking_about_is_not_changed_here() {
    let scratch = support::Scratch::new("agent-note-elsewhere-keys");
    support::make_room_for_notes(scratch.path());
    // The locked one second, so that every key it refuses had somewhere to
    // go: a note at the top of the list cannot be stepped under the one
    // above it or moved past it whoever holds it, and a test standing there
    // would pass with the lock taken out.
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456V\"\nsaid = \"mine\"\ndone = false\ndepth = 0\n\
         \n[[todo]]\nid = \"0123456T\"\nsaid = \"theirs\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let theirs = obelus_git::todo::NoteId::read("0123456T").expect("a name");
    let mine = obelus_git::todo::NoteId::read("0123456V").expect("a name");

    let held = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(theirs.clone()),
    )
    .expect("their claim");

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let dump = support::render(&mut app, WIDTH, 18);
    assert_eq!(
        app.talked_about().get(1),
        Some(&obelus_component::todo::Talked::Elsewhere),
        "the second note is not the locked one, so this proves nothing:\n{dump}"
    );
    // The page opens on the first note, so the reader walks down to theirs.
    support::press(&mut app, KeyCode::Down);
    let dump = support::render(&mut app, WIDTH, 18);
    assert!(
        obelus_ui::todo::caret(app.editor_area_for_test(), app.notes().expect("the notes"))
            .is_some(),
        "the caret left the note, so nothing says where the reader is:\n{dump}"
    );

    // A letter does not go in, and nothing comes out. Read off the whole
    // page rather than the note: what is typed lives in the box until the
    // caret leaves, so the page is the only thing that says what the key
    // did -- and comparing the whole of it catches a character landing
    // wherever the caret happens to be, which on a page that opens at the
    // start of a note is in front of its words rather than after them.
    let was = support::render(&mut app, WIDTH, 18);
    let before = support::text_block(&was).to_string();
    support::type_text(&mut app, "zz");
    support::press(&mut app, KeyCode::Delete);
    support::press_shift(&mut app, KeyCode::Enter);
    support::press_alt_key(&mut app, KeyCode::Enter);
    let now = support::render(&mut app, WIDTH, 18);
    let after = support::text_block(&now).to_string();
    assert_eq!(
        before, after,
        "a key changed a note another Obelus is talking about"
    );

    // Ticked off, moved, stepped under the one above, taken away: each of
    // these reaches the page's copy the moment it happens, so the copy is
    // what says they did not.
    //
    // One at a time, and asked after each. The two moves undo one another
    // if they are pressed together, so a key let through would put the note
    // back before anything looked -- and a failure reported against the
    // wrong key is a failure that sends the next reader to the wrong guard.
    let order = |app: &App| -> Vec<obelus_git::todo::NoteId> {
        app.notes()
            .expect("the notes")
            .todo()
            .notes
            .iter()
            .map(|note| note.id.clone())
            .collect()
    };
    let as_written = vec![mine.clone(), theirs.clone()];

    support::press_alt(&mut app, ' ');
    assert!(
        !app.notes().expect("the notes").todo().notes[1].done,
        "a note another Obelus is talking about was ticked off"
    );
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.notes().expect("the notes").todo().notes[1].depth,
        0,
        "a note another Obelus is talking about was stepped in under the one above"
    );
    support::press_alt_key(&mut app, KeyCode::Backspace);
    assert_eq!(
        order(&app),
        as_written,
        "a note another Obelus is talking about was taken away"
    );
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(
        order(&app),
        as_written,
        "a note another Obelus is talking about was moved up"
    );
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(
        order(&app),
        as_written,
        "a note another Obelus is talking about was moved down"
    );
    // The cut is the same act again -- the selection out of its words, or
    // the whole of it -- and a paste arrives by a door of its own, which is
    // the terminal's own rather than the key's.
    support::press_control(&mut app, 'x');
    assert_eq!(
        order(&app),
        as_written,
        "a note another Obelus is talking about was cut away"
    );
    // With something held, which is the other half of that key: nothing
    // held takes the whole note and is refused where the key that drops one
    // is, and this is the half that reaches into its words.
    support::press_control(&mut app, 'a');
    support::press_control(&mut app, 'x');
    let cut = support::render(&mut app, WIDTH, 18);
    assert!(
        support::text_block(&cut).contains("theirs"),
        "a cut took the words out of somebody else's note:\n{cut}"
    );
    app.handle(Event::Paste("pasted".to_string()));
    let pasted = support::render(&mut app, WIDTH, 18);
    assert!(
        !support::text_block(&pasted).contains("pasted"),
        "a paste went into a note another Obelus is talking about:\n{pasted}"
    );

    // And the foot does not offer the keys it will not honour, while still
    // offering the ones that work on a note anybody may read.
    let dump = support::render(&mut app, WIDTH, 18);
    for word in ["Done", "Drop", "Move", "Under"] {
        assert!(
            !dump.contains(word),
            "the foot offers {word}, which does nothing on this note:\n{dump}"
        );
    }
    assert!(
        dump.contains("Another"),
        "the foot stopped offering a key that still works:\n{dump}"
    );

    // The note above is the reader's own, and the same key works on it --
    // which is what says the lock was refused and not the page.
    //
    // Twice, because the selection taken above is still held and the first
    // press is what lets go of it: a key that both dropped the selection
    // and left the box would do two things at once.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Up);
    support::press_alt(&mut app, ' ');
    assert!(
        app.notes()
            .expect("the notes")
            .todo()
            .notes
            .iter()
            .any(|note| note.id == mine && note.done),
        "the reader's own note would not take the key either"
    );
    drop(held);
}

/// A locked note holds back the keys of what it hangs under, and not the
/// other way round.
///
/// Taking a note away takes its children, so a note somebody else is
/// talking about would go with a parent nobody has claimed -- the lock
/// running *upwards*. Downwards there is nothing to run: a child is another
/// note with its own name and its own claim, and changing it leaves the
/// locked one's words, its box and its depth exactly as they were.
///
/// Refused whole, because each note of a run goes to the file under its own
/// name: half of it applied leaves the list a child whose parent has gone.
///
/// Broken deliberately by asking `selected_is_elsewhere` in `can_drop`
/// instead of `run_is_elsewhere`: the parent goes and the locked child is
/// left behind, hanging under nothing.
#[test]
fn a_locked_note_holds_back_the_keys_of_what_it_hangs_under() {
    let scratch = support::Scratch::new("agent-note-elsewhere-run");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456W\"\nsaid = \"the parent\"\ndone = false\ndepth = 0\n\
         \n[[todo]]\nid = \"0123456X\"\nsaid = \"theirs\"\ndone = false\ndepth = 1\n",
    )
    .expect("the notes");
    let parent = obelus_git::todo::NoteId::read("0123456W").expect("a name");
    let child = obelus_git::todo::NoteId::read("0123456X").expect("a name");

    let held = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(child.clone()),
    )
    .expect("their claim");

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let dump = support::render(&mut app, WIDTH, 18);
    assert_eq!(
        app.talked_about().get(1),
        Some(&obelus_component::todo::Talked::Elsewhere),
        "the child is not the locked one, so this proves nothing:\n{dump}"
    );

    // The caret opens on the parent, which nobody has claimed.
    support::press_alt_key(&mut app, KeyCode::Backspace);
    let left: Vec<obelus_git::todo::NoteId> = app
        .notes()
        .expect("the notes")
        .todo()
        .notes
        .iter()
        .map(|note| note.id.clone())
        .collect();
    assert_eq!(
        left,
        vec![parent.clone(), child.clone()],
        "a note another Obelus is talking about went with its parent"
    );
    let dump = support::render(&mut app, WIDTH, 18);
    assert!(
        !dump.contains("Drop"),
        "the foot offers a key that would take somebody else's note away:\n{dump}"
    );
    // Moving the run is not changing it: every note in it keeps its words,
    // its depth and its name, so that key is still the reader's.
    assert!(
        dump.contains("Move"),
        "the foot held back a key that changes nothing about the locked note:\n{dump}"
    );
    drop(held);
}

/// And an agent's tool is refused that note too, in words.
///
/// One judgement: an agent must not do what the reader standing in front of
/// it cannot. Said rather than silently dropped, because the tools answer an
/// agent in words and the agent says why in the transcript -- which is where
/// a reader who asked for it is looking.
///
/// A note hung *under* the locked one is still written down, for the reason
/// `alt+up` is still allowed to carry a locked child past a neighbour: it
/// changes that note's words, its box and its depth not at all.
///
/// Broken deliberately by letting the tool through: the note comes back
/// ticked off.
#[test]
fn an_agents_tool_is_refused_a_note_another_obelus_is_talking_about() {
    let scratch = support::Scratch::new("agent-note-elsewhere-tool");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Y\"\nsaid = \"theirs\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let theirs = obelus_git::todo::NoteId::read("0123456Y").expect("a name");

    let held = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(theirs.clone()),
    )
    .expect("their claim");

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let dump = support::render(&mut app, WIDTH, 18);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Elsewhere),
        "the note is not locked, so this proves nothing:\n{dump}"
    );

    let said = what_the_tool_said(&mut app, obelus_git::todo::Doing::Finish(theirs.clone()));
    assert!(
        said.contains("another Obelus"),
        "the tool was not told why it was refused: {said:?}"
    );
    let reworded = what_the_tool_said(
        &mut app,
        obelus_git::todo::Doing::Reword {
            note: theirs.clone(),
            said: "the agent's words".to_string(),
        },
    );
    assert!(
        reworded.contains("another Obelus"),
        "rewording was not refused: {reworded:?}"
    );
    let now = obelus_git::todo::read(scratch.path())
        .notes()
        .expect("the notes");
    assert_eq!(now.notes[0].said, "theirs", "the agent reworded it anyway");
    assert!(!now.notes[0].done, "the agent ticked it off anyway");

    // A note of its own is still the agent's to write down.
    let added = what_the_tool_said(
        &mut app,
        obelus_git::todo::Doing::Add {
            notes: vec![("a new one".to_string(), 0)],
            under: Some(theirs.clone()),
        },
    );
    assert!(
        added.contains("written down"),
        "a note hung under a locked one was refused: {added:?}"
    );
    drop(held);
}

/// What one of the agent's tools answered, through the door it comes in by.
fn what_the_tool_said(app: &mut App, doing: obelus_git::todo::Doing) -> String {
    let (answer, mut said) = futures::channel::oneshot::channel();
    app.handle(Event::Tools(obelus_mcp::Asked {
        wanted: obelus_mcp::Wanted::Notes(doing),
        answer,
    }));
    said.try_recv()
        .ok()
        .flatten()
        .expect("the tool answered nothing")
}

/// Opening a conversation opens its session, before the reader has said a
/// word, and the first word goes in it.
///
/// So that what the agent offers is there to see before anything is said:
/// the settings on the row and what it takes with a slash both come with a
/// session and nowhere else, and a conversation that waited for the first
/// message to ask for one had a blank row and an empty `/` list for exactly
/// the moment a reader looks at them -- before they decide what to say.
///
/// Deliberate break: take `settle_the_sessions` out of `prepare`. Nothing
/// is asked for until the reader types, and the wait for the session gives
/// up.
#[test]
fn opening_a_conversation_opens_its_session_before_a_word_is_said() {
    let (mut app, events) = wired();
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    assert!(app.chat().is_some(), "the view did not open");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    // Waited for rather than looked at once the session is there: the
    // agent says what it takes after the answer that opens the session,
    // and that answer is told first.
    pump(
        &mut app,
        &events,
        "what it takes with a slash, before a word is said",
        |app| app.talking() == obelus_agent::Talking::Ready && !app.agent_orders().is_empty(),
    );

    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    // The agent's own words, not the reader's: what they typed is on the
    // page the moment they press enter, so a transcript holding it says
    // only that they pressed enter.
    pump(&mut app, &events, "the answer", |app| {
        said_in_transcript(app, "heard you")
    });
}

/// A note's conversation opened and left without a word leaves nothing
/// behind: nothing written down against the note, and the session let go.
///
/// Which is the reader opening a note's conversation and changing their
/// mind. The session was opened for the view, not for them, and a note
/// with a conversation under it that nobody had is a note that says there
/// is something to come back to when there is not. And coming back asks
/// for a new one, because the last one has gone.
///
/// Deliberate break: take the call to `let_go_of_what_nothing_was_said_in`
/// out of `settle_the_sessions`. The agent is never told, and the wait for
/// it to be gives up. Take the `anything_said` test out of `Kept`'s writer
/// -- `remember_the_conversations` -- and the note has one written down.
#[test]
fn a_note_s_conversation_left_without_a_word_keeps_nothing() {
    let scratch = support::Scratch::new("agent-note-left");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456P\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    let log = scratch.path().join("asked.log");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    assert!(app.chat().is_some(), "no conversation about the note");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // And back to the notes, having said nothing.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    asked(&mut app, &events, &log, "session/delete s-1");
    let id = obelus_git::todo::NoteId::read("0123456P").expect("a name");
    let kept = obelus_agent::acp::sessions::read(scratch.path())
        .remembered()
        .and_then(|remembered| {
            remembered
                .get(
                    &obelus_agent::chats::ChatId::Note(id),
                    "fake",
                    scratch.path(),
                )
                .cloned()
        });
    assert!(
        kept.is_none(),
        "a conversation nothing was said in was written down against the note: {kept:?}"
    );

    // Coming back asks for another.
    support::press_alt(&mut app, 'a');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "a session of its own again", |app| {
        app.chat_session_for_test().as_deref() == Some("s-2")
    });
}

/// A conversation taken up again is not let go for being quiet.
///
/// An agent that resumes rather than replays sends nothing of it back, so
/// the page is empty -- which is what a conversation nothing was said in
/// looks like from here. It is the reader's all the same, and a delete
/// sent for it would be the agent forgetting what they said last week.
///
/// Deliberate break: let go of every session nothing was said in, minted or
/// not -- drop the `minted` test in `let_go_of_what_nothing_was_said_in` --
/// and the log has `session/delete s-old`.
#[test]
fn a_conversation_taken_up_again_is_not_let_go_for_being_quiet() {
    let scratch = support::Scratch::new("agent-note-quiet");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Q\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    remember_a_note_conversation(&scratch, "0123456Q", "s-old");
    let log = scratch.path().join("asked.log");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "only-resumes".to_string(),
            format!("log={}", log.display()),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the conversation taken up", |app| {
        app.chat_session_for_test().as_deref() == Some("s-old")
            && app.talking() == obelus_agent::Talking::Ready
    });

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    settle(&mut app, &events, Duration::from_millis(300));
    let asked = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        asked.contains("session/resume s-old"),
        "it was never taken up, so this proves nothing:\n{asked}"
    );
    assert!(
        !asked.contains("session/delete s-old"),
        "a conversation the reader had was let go:\n{asked}"
    );
}

/// The list offers what this project has had, newest first, and taking one
/// up goes to it.
///
/// What a reader said to an agent outlives the window they said it in: the
/// agent keeps every word and Obelus keeps which conversation is which.
/// Without a way back to one, the only conversations a reader has are the
/// ones this Obelus happens to be holding.
///
/// Broken deliberately by sorting the rows the other way round in
/// `App::conversation_rows` -- `Reverse` off the key -- which puts last
/// week's conversation above this minute's.
#[test]
fn the_list_offers_the_conversations_this_project_has_had() {
    let scratch = support::Scratch::new("agent-conversation-list");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    // The setting is what says which agent's conversations these are, and
    // the process is what will be asked for one: the list is built from the
    // first and has to reach the second.
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    remember_a_conversation(&scratch, "fake", "s-old", "count the lines", Some(1_000));
    remember_a_conversation(&scratch, "fake", "s-new", "the margin lies", Some(2_000));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    // The key rather than the command, because the key is half of it: `f4`
    // is the list from inside a conversation as well as from a file.
    support::press(&mut app, KeyCode::F(4));
    let rows: Vec<(String, bool)> = listed_conversations(&app)
        .into_iter()
        .map(|item| (item.label.clone(), item.enabled))
        .collect();
    assert_eq!(
        rows,
        [
            ("the margin lies".to_string(), true),
            ("count the lines".to_string(), true),
        ],
        "the conversations are not there, or not newest first"
    );
    // No tab row: one agent has talked here, and a row of tabs over the
    // only agent a reader has used says nothing and costs two rows.
    assert!(
        app.picker().expect("the list").tabs().is_empty(),
        "a list of one agent's conversations has a row of tabs"
    );

    // Starting on the new one, because the conversation the reader is in
    // has nothing said in it and so is not in the list. Broken by putting
    // the new row last in `App::conversation_rows`.
    assert_eq!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.as_str()),
        Some("New conversation"),
        "the list does not start on the new conversation"
    );
    // Taking one up opens it and asks the agent for that one by name --
    // one row down, past the new one.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(
        app.chat_session_for_test().as_deref(),
        Some("s-new"),
        "it did not ask for the conversation the row named"
    );
}

/// On a project nobody has talked about, the list still opens, holding the
/// one row that starts a conversation.
///
/// `f4` is the way to the agent from anywhere, so a key that did nothing
/// until something had been said would be a way in that only works for a
/// reader who has already found another.
///
/// Broken deliberately two ways. Leaving the agent in use out of the tabs
/// where it has said nothing -- the `push` in `open_conversation_picker` --
/// leaves the list with no rows at all. And taking the new row out of
/// `App::conversation_rows` leaves enter with nothing to choose.
#[test]
fn the_list_of_conversations_starts_one_where_there_are_none() {
    let scratch = support::Scratch::new("agent-conversation-none");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    assert!(
        app.chat().is_none(),
        "a conversation was open before the key"
    );

    support::press(&mut app, KeyCode::F(4));
    let rows: Vec<String> = app
        .picker()
        .expect("the list, on a project nobody has talked about")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        ["New conversation"],
        "the list is not the new row alone"
    );
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the list stayed open");
    assert!(
        app.chat().is_some(),
        "choosing the new row did not open a conversation"
    );
}

/// A new conversation is a new one once something has been said, and not
/// before.
///
/// One already open with nothing in it *is* the new conversation, so
/// asking again goes back to it: a reader pressing the row twice would
/// otherwise collect empty pages in the list of what is open.
///
/// Broken deliberately two ways. Having `App::new_conversation` push a
/// document every time: the count goes up while nothing has been said. And
/// taking `anything_said` out of `Conversation::is_blank`: the reader is
/// sent back into the conversation they asked to leave.
#[test]
fn a_new_conversation_is_new_once_something_is_said_in_the_last() {
    let scratch = support::Scratch::new("agent-conversation-new");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    let documents = app.document_count_for_test();
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::FileOpen);
    support::press(&mut app, KeyCode::Esc);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    assert_eq!(
        app.document_count_for_test(),
        documents,
        "a second empty conversation was opened beside the first"
    );

    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        said_in_transcript(app, "heard you")
    });
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    assert_eq!(
        app.document_count_for_test(),
        documents + 1,
        "the reader was sent back into the conversation they had talked in"
    );
    assert!(
        !said_in_transcript(&app, "heard you"),
        "the new conversation has the old one's words in it"
    );
}

/// The list of conversations is read whole, in runs by the day each was
/// last spoken in.
///
/// A title is a sentence -- often the whole of what the reader first said
/// -- and one row of it was a row saying there had been more. And the
/// headings are over the rows that match, so a run the query empties
/// takes its heading with it.
///
/// Broken deliberately two ways. Taking `picker.wraps` out of
/// `open_conversation_picker` puts the title on one row, and the end of it
/// is gone. And heading every row that has a section in `Above::of`, rather
/// than the first of its run, puts a `Today` over each of the two.
#[test]
fn the_list_of_conversations_is_read_whole_in_runs_by_day() {
    let scratch = support::Scratch::new("agent-conversation-runs");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    remember_a_conversation(
        &scratch,
        "fake",
        "s-long",
        "the margin says a line changed when nothing did, and again after every save",
        Some(now),
    );
    remember_a_conversation(&scratch, "fake", "s-short", "a short one", Some(now));
    remember_a_conversation(
        &scratch,
        "fake",
        "s-old",
        "count the lines",
        Some(now - 40 * 86_400),
    );

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    let rows = |app: &mut App| -> Vec<String> {
        support::text_block(&support::render(app, WIDTH, HEIGHT))
            .lines()
            // Past the row's number, which the dump puts in front of it.
            .map(|row| {
                row.split_once('|')
                    .map_or(row, |(_, row)| row)
                    .trim()
                    .to_string()
            })
            .collect()
    };

    let shown = rows(&mut app);
    let first = shown
        .iter()
        .position(|row| row.contains("the margin says"))
        .unwrap_or_else(|| panic!("the long title is not there:\n{}", shown.join("\n")));
    assert!(
        shown[first].ends_with("just now"),
        "the time is not on the title's first row: {:?}",
        shown[first]
    );
    assert!(
        shown[first + 1].contains("every save"),
        "the title did not go on to a second row:\n{}",
        shown.join("\n")
    );
    let headings = |shown: &[String]| -> Vec<String> {
        shown
            .iter()
            .filter(|row| ["Today", "Yesterday", "This week", "Earlier"].contains(&row.as_str()))
            .cloned()
            .collect()
    };
    assert_eq!(
        headings(&shown),
        ["Today", "Earlier"],
        "the runs are not headed by day:\n{}",
        shown.join("\n")
    );

    support::type_text(&mut app, "count");
    let shown = rows(&mut app);
    assert_eq!(
        headings(&shown),
        ["Earlier"],
        "a run the query emptied kept its heading:\n{}",
        shown.join("\n")
    );
}

/// A conversation another Obelus has open is in the list and cannot be
/// taken up.
///
/// The same promise the notes page makes, arriving by the other door: a
/// conversation is not a thing two Obelus may have open at once, and the
/// list is now a second way to reach one. Said in the ink and the lock
/// rather than on the status row -- the reader is told before they press,
/// which is the rule the palette follows for a command it will not run.
///
/// Two halves, because each passes with the other broken. Giving every row
/// `enabled: true` in `App::conversation_rows` stops the row saying so
/// before the reader presses. And taking the claim out of
/// `App::take_up_conversation` -- opening it whether or not `chats::claim`
/// answered -- lets the second window in when the other Obelus arrived
/// after the list was built, which is the moment the row cannot have
/// covered.
#[test]
fn a_conversation_another_obelus_has_open_cannot_be_taken_up_from_the_list() {
    let scratch = support::Scratch::new("agent-conversation-held");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    remember_a_conversation(&scratch, "fake", "s-held", "somebody has this", Some(2_000));
    remember_a_conversation(&scratch, "fake", "s-free", "nobody has this", Some(1_000));

    // The other Obelus, holding it for as long as this is held. A lock
    // belongs to the open file rather than to the process, so one taken
    // here is as much somebody else's as one taken in another window.
    let theirs = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-held".to_string()),
    )
    .expect("their claim");

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    let rows: Vec<(String, bool, bool)> = listed_conversations(&app)
        .into_iter()
        .map(|item| (item.label.clone(), item.enabled, item.marker.is_some()))
        .collect();
    assert_eq!(
        rows,
        [
            ("somebody has this".to_string(), false, true),
            ("nobody has this".to_string(), true, false),
        ],
        "the row does not say the conversation is somebody else's"
    );

    // And the other half: a conversation that was free when the list was
    // built and is not free by the time the reader presses enter on it. The
    // claim is asked for at the press and not read off the row, so the
    // answer is the row saying so -- the list stays open and the lock
    // arrives on it.
    let also_theirs = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-free".to_string()),
    )
    .expect("their second claim");
    // Not heard yet, which is the point of this half: the row still says
    // the conversation is free, and what refuses the reader is the claim
    // being asked for again at the press. Down from the new one steps over
    // the row that is somebody else's.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.picker().is_some(),
        "the list went, so a second window was let into the conversation"
    );
    assert!(
        app.chat_session_for_test().is_none(),
        "a second window was let into the conversation"
    );
    assert!(
        listed_conversations(&app)
            .into_iter()
            .all(|item| !item.enabled),
        "the row the reader pressed still says it can be taken up"
    );

    // Given up, they are the reader's again.
    drop(theirs);
    drop(also_theirs);
    the_claims_changed(
        &mut app,
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-held".to_string()),
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    assert!(
        app.picker()
            .expect("the list")
            .matches()
            .all(|item| item.enabled),
        "the conversations are still somebody else's after they were let go"
    );
}

/// A conversation had with another agent gets a tab of its own, and its
/// rows cannot be taken up.
///
/// A session id is a name one agent minted and means nothing to another, so
/// only the agent in use can be asked to take one up. Shown all the same,
/// with the reason above them: the alternative is a reader who changed
/// agents finding their conversations gone and nothing saying where.
///
/// Broken deliberately by taking the `mine &&` off `enabled` in
/// `App::conversation_rows`: the rows offer to take up a conversation the
/// agent in use never had, and the agent answers by opening a new one.
#[test]
fn another_agents_conversations_get_a_tab_and_cannot_be_taken_up() {
    let scratch = support::Scratch::new("agent-conversation-tabs");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    remember_a_conversation(&scratch, "fake", "s-mine", "mine", Some(2_000));
    remember_a_conversation(&scratch, "other", "s-theirs", "theirs", Some(1_000));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    let picker = app.picker().expect("the list");
    assert_eq!(
        picker.tabs(),
        ["fake", "other"],
        "the agents that have talked here are not the tabs, or not in use first"
    );
    assert!(
        picker.what_about().is_none(),
        "the tab whose rows all work is explaining a rule nothing has broken"
    );
    assert!(
        picker.matches().all(|item| item.enabled),
        "the agent in use cannot take up its own conversations"
    );

    // One tab to the right: the other agent's, said and not offered.
    support::press(&mut app, KeyCode::Tab);
    let picker = app.picker().expect("the list");
    let rows: Vec<(String, bool)> = picker
        .matches()
        .map(|item| (item.label.clone(), item.enabled))
        .collect();
    assert_eq!(
        rows,
        [("theirs".to_string(), false)],
        "the other agent's conversations are missing, or offered"
    );
    assert!(
        picker
            .what_about()
            .is_some_and(|said| said.contains("only be taken up by the agent that had it")),
        "a tab of rows that cannot be chosen says nothing about why"
    );
}

/// A conversation had in another checkout of the project is listed and
/// cannot be taken up, and says which checkout it was.
///
/// The table is every worktree's, because the notes are; the agent is told
/// one directory and keeps a conversation under it, so asked for it from
/// another it answers that there is no such thing.
///
/// Broken deliberately by taking `there.is_none() &&` off `enabled` in
/// `App::conversation_rows`: the other checkout's row is offered.
#[test]
fn another_checkouts_conversations_are_listed_and_cannot_be_taken_up() {
    let scratch = support::Scratch::new("agent-conversation-checkouts");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    remember_a_conversation(&scratch, "fake", "s-here", "here", Some(2_000));
    obelus_agent::acp::sessions::change(scratch.path(), None, |remembered| {
        remembered.put(
            &obelus_agent::chats::ChatId::Loose("s-there".to_string()),
            "fake",
            &scratch.path().join("worktree-two"),
            obelus_agent::acp::sessions::Kept {
                session: "s-there".to_string(),
                title: Some("there".to_string()),
                told: None,
                introduced: false,
                last: Some(1_000),
            },
        );
    });

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    // Whether the row names the other checkout, beside when it was said.
    let rows: Vec<(String, bool, bool)> = listed_conversations(&app)
        .into_iter()
        .map(|item| {
            (
                item.label.clone(),
                item.trailing
                    .as_deref()
                    .is_some_and(|trailing| trailing.starts_with("worktree-two  ")),
                item.enabled,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("here".to_string(), false, true),
            ("there".to_string(), true, false),
        ],
        "the other checkout's conversation is missing, offered, or not said to be elsewhere"
    );
    assert!(
        app.picker()
            .and_then(|picker| picker.what_about())
            .is_some_and(|said| said.contains("checkout it was had in")),
        "a row that cannot be chosen says nothing about why"
    );
}

/// A note's conversation from another checkout is not asked for here, and
/// is still there for the checkout that had it.
///
/// Asked for, the agent refused it -- it keeps a conversation under the
/// directory it was told -- and the refusal forgot it out of the table
/// every worktree reads: one press of the key in one worktree, and the
/// conversation was gone from the other as well.
///
/// Broken deliberately by having `Remembered::get` answer with any
/// checkout's row: `session/resume s-there` is in the log.
#[test]
fn a_notes_conversation_from_another_checkout_is_not_asked_for() {
    let scratch = support::Scratch::new("agent-note-other-checkout");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456W\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let there = scratch.path().join("worktree-two");
    let id = obelus_git::todo::NoteId::read("0123456W").expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        Some(std::slice::from_ref(&id)),
        |remembered| {
            remembered.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "fake",
                &there,
                obelus_agent::acp::sessions::Kept {
                    session: "s-there".to_string(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: Some(1_700_000_000),
                },
            );
        },
    );
    let log = scratch.path().join("asked.log");

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "only-resumes".to_string(),
            format!("log={}", log.display()),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Not),
        "the note says it has a conversation this checkout cannot take up"
    );
    talk_about_the_note(&mut app);
    pump(&mut app, &events, "a session for the note", |app| {
        app.chat_session_for_test().is_some() && app.talking() == obelus_agent::Talking::Ready
    });

    let asked = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !asked.contains("s-there"),
        "another checkout's conversation was asked for here:\n{asked}"
    );
    assert!(
        obelus_agent::acp::sessions::read(scratch.path())
            .remembered()
            .expect("the table")
            .get(&obelus_agent::chats::ChatId::Note(id), "fake", &there)
            .is_some_and(|kept| kept.session == "s-there"),
        "the other checkout's conversation was forgotten"
    );
}

/// A note locked from another checkout says which one, on the status row.
///
/// The lock says the keys will do nothing; a reader with three worktrees of
/// one project open needs to know which of them to go to. The claims are
/// every worktree's, like the notes, so the holder writes where it is.
///
/// Broken deliberately by answering `Holder::AnotherWindow` for every note
/// in `App::which_notes_are_elsewhere`: the row names no checkout.
#[test]
fn a_note_locked_from_another_checkout_says_which() {
    let scratch = support::Scratch::new("agent-note-held-there");
    let git = |arguments: &[&str]| {
        let outcome = std::process::Command::new("git")
            .arg("-C")
            .arg(scratch.path())
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "obelus")
            .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
            .env("GIT_COMMITTER_NAME", "obelus")
            .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
            .output()
            .expect("running git");
        assert!(outcome.status.success(), "git {arguments:?} failed");
    };
    git(&["init", "--quiet", "--initial-branch=master"]);
    std::fs::write(scratch.path().join("one.rs"), "fn main() {}\n").expect("a file");
    // A worktree needs a commit to branch from.
    git(&["add", "one.rs"]);
    git(&["commit", "--quiet", "-m", "committed"]);
    // Short, because the status row drops the sentence whole where it does
    // not fit beside the count.
    let there = scratch
        .path()
        .with_file_name(format!("obelus-two-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&there);
    git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "elsewhere",
        there.to_str().expect("a path"),
    ]);

    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456T\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let which = obelus_agent::chats::ChatId::Note(
        obelus_git::todo::NoteId::read("0123456T").expect("a name"),
    );
    let _held = obelus_agent::chats::claim(&there, &which).expect("the other checkout's claim");

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    the_claims_changed(&mut app, scratch.path(), &which);
    let text = screen(&mut app);
    let _ = std::fs::remove_dir_all(&there);

    let name = there
        .file_name()
        .expect("a name")
        .to_string_lossy()
        .into_owned();
    let status = text.lines().last().unwrap_or_default();
    assert!(
        status.contains(&format!("Talked about in {name}")),
        "the status row does not say which checkout has the note:\n{text}"
    );
}

/// With no agent chosen, the tab a new conversation starts on says so.
///
/// The agent in use always has a tab, and with none chosen its name is no
/// name at all -- a tab with nothing on it, beside another agent's.
///
/// Broken deliberately by naming the tabs with `agent_called` alone in
/// `open_conversation_picker`: the first tab is blank.
#[test]
fn with_no_agent_chosen_the_first_tab_says_so() {
    let scratch = support::Scratch::new("agent-conversation-no-agent");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(obelus_config::Config::default(), Vec::new());
    remember_a_conversation(&scratch, "other", "s-theirs", "theirs", Some(1_000));

    support::press(&mut app, KeyCode::F(4));
    let picker = app.picker().expect("the list");
    assert_eq!(
        picker.tabs(),
        ["No agent", "other"],
        "the tab of no agent has no name"
    );
}

/// A conversation this Obelus already has open is gone to, not opened
/// twice.
///
/// The list answers "which conversation", not "where it is kept": a reader
/// choosing a row means to be reading it, and whether Obelus has to ask the
/// agent for it is Obelus's business.
///
/// Broken deliberately by dropping the `listed.open` arm from
/// `App::take_up_conversation`: a second document appears for one
/// conversation, and the claim it asks for is one this Obelus already
/// holds, so nothing opens at all.
#[test]
fn a_conversation_already_open_here_is_gone_to() {
    // A project of its own, because the list is of everything said about
    // one: a test sharing the checkout with every other test in this binary
    // would find their conversations in its list, and one of them held.
    let scratch = support::Scratch::new("agent-conversation-open-here");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    // Which agent's conversations the list is of is the setting, not
    // whichever process happens to be running: the list has to answer
    // before anything is started.
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        said_in_transcript(app, "heard you")
    });
    let open = app.chat_session_for_test().expect("a session");
    let documents = app.document_count_for_test();

    // The list, from inside the conversation.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationSelect);
    // It is in the list at all, which is a conversation about nothing in
    // particular being written down: those were left out while the only
    // thing that asked was the notes page, which has no row for one.
    assert_eq!(
        listed_conversations(&app).len(),
        1,
        "a conversation about nothing in particular was not written down"
    );
    // And its row says it is the one the reader came from -- after a frame,
    // because a frame is where what a row says about itself is asked
    // again, and asking again must not take the mark away. Broken by
    // answering `None` for it in `App::listed_mark`.
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        listed_conversations(&app)
            .first()
            .and_then(|item| item.marker.as_ref())
            .map(|(_, mark)| mark.as_str()),
        Some("\u{2022}"),
        "the conversation the reader is in is not marked as theirs"
    );
    // Enter with nothing pressed before it, because the list starts on the
    // conversation the reader is in rather than on the new one. Broken by
    // taking the `select_item` out of `open_conversation_picker`: the
    // selection stays on the new row, and enter opens a second document.
    support::press(&mut app, KeyCode::Enter);
    // The list went, which is what says the row was taken. It would not
    // have: the claim on a conversation this Obelus is already holding is
    // one it cannot take twice, so a row that asked for it again would
    // refuse the reader their own conversation.
    assert!(
        app.picker().is_none(),
        "the row would not take the reader to a conversation they already have open"
    );
    assert_eq!(
        app.chat_session_for_test().as_deref(),
        Some(open.as_str()),
        "the row did not take the reader to the conversation it named"
    );
    assert_eq!(
        app.document_count_for_test(),
        documents,
        "a second document was opened for one conversation"
    );

    // And from a file, where it is not the conversation the reader is in
    // and the list starts on the new row instead: one row down is it.
    app.open_for_test(Path::new("src/lib.rs"));
    assert!(app.chat().is_none(), "the file is not what is showing");
    support::press(&mut app, KeyCode::F(4));
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.picker().is_none(),
        "the row would not take the reader back from a file"
    );
    assert_eq!(
        app.chat_session_for_test().as_deref(),
        Some(open.as_str()),
        "the row did not take the reader back to the conversation it named"
    );
    assert_eq!(
        app.document_count_for_test(),
        documents + 1,
        "a second document was opened for one conversation, from a file"
    );
}

/// A conversation taken up in another window says so under the reader,
/// without them pressing anything.
///
/// The one thing on these rows that changes while the list is up and is
/// nobody's keystroke. A claim is a lock and a lock is invisible to a
/// watcher -- nothing is written when one is taken -- which is why the
/// claim has a file: the directory is what wakes this Obelus, and the
/// frame that wakes asks the rows again.
///
/// The notice is said by hand here, which is what this test is *not*
/// about: whether anything would ever say it is
/// `the_notes_hear_a_claim_through_a_watch_they_took_themselves`, which
/// takes a real watcher and delivers nothing itself.
///
/// Two deliberate breaks. Taking `freshen_the_conversation_rows` out of
/// `App::prepare` leaves the row saying it is free for as long as the
/// reader looks at it. Answering `Said { enabled: true, .. }` there lets
/// them into a conversation the row has just drawn a lock on -- the half
/// that would have drifted if a remark carried only the mark.
#[test]
fn a_conversation_taken_up_elsewhere_says_so_while_the_reader_looks_at_it() {
    let scratch = support::Scratch::new("agent-conversation-live");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    remember_a_conversation(&scratch, "fake", "s-one", "the first", Some(2_000));
    remember_a_conversation(&scratch, "fake", "s-two", "the second", Some(1_000));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConversationNew);
    support::press(&mut app, KeyCode::F(4));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(
        listed_conversations(&app)
            .into_iter()
            .all(|item| item.enabled && item.marker.is_none()),
        "a conversation nobody has open is drawn as somebody's"
    );
    // The other Obelus arrives, and the reader presses nothing: the frame
    // its watch wakes is what asks the rows again.
    let theirs = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-one".to_string()),
    )
    .expect("their claim");
    the_claims_changed(
        &mut app,
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-one".to_string()),
    );
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let rows: Vec<(String, bool, bool)> = listed_conversations(&app)
        .into_iter()
        .map(|item| (item.label.clone(), item.enabled, item.marker.is_some()))
        .collect();
    assert_eq!(
        rows,
        [
            ("the first".to_string(), false, true),
            ("the second".to_string(), true, false),
        ],
        "the row went on saying the conversation was free to take up"
    );

    // And back again when they let it go, because what a row says is
    // worked out from what Obelus was last told rather than written down
    // once.
    drop(theirs);
    the_claims_changed(
        &mut app,
        scratch.path(),
        &obelus_agent::chats::ChatId::Loose("s-one".to_string()),
    );
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(
        listed_conversations(&app)
            .into_iter()
            .all(|item| item.enabled && item.marker.is_none()),
        "the row is still somebody else's after they let it go"
    );
}

/// A table Obelus cannot read is said over the list, which still opens.
///
/// The list is never empty -- it starts a new conversation as well -- so a
/// project whose table will not parse would look exactly like one nobody
/// has talked about: the new row and nothing under it. That reports
/// Obelus's own trouble as the reader's history, which is the answer the
/// notes and the settings both had to learn to tell apart.
///
/// Broken deliberately two ways. Having `open_conversation_picker` return
/// early on a table it cannot read leaves the key doing nothing at all. And
/// taking the `unreadable` arm out of `say_whose_conversations` says
/// nothing over the list, which is the new row alone.
#[test]
fn a_table_obelus_cannot_read_is_not_a_project_nobody_has_talked_about() {
    let scratch = support::Scratch::new("agent-conversation-unreadable");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    // Half a table, as another program's crash leaves one.
    let table = obelus_agent::acp::sessions::path(scratch.path()).expect("somewhere to keep it");
    std::fs::create_dir_all(table.parent().expect("a directory")).expect("the directory");
    std::fs::write(&table, "[[talked]]\nagent = \"half a na").expect("the half-written table");

    support::press(&mut app, KeyCode::F(4));
    assert!(
        listed_conversations(&app).is_empty(),
        "rows were made out of a table that would not read"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        dump.contains("cannot read what it wrote down"),
        "the list blamed the reader for Obelus's own unreadable file:\n{dump}"
    );
}

/// The notes stay level with another window writing down a conversation.
///
/// Which of them has one is a table Obelus keeps rather than parses on
/// every frame the page draws -- 37us for one conversation and 351us for
/// twenty, twelve times a second while any agent is at work. What makes
/// keeping it honest is that a write to a file is something a watcher
/// hears, which is the whole difference between this and the claim beside
/// it: a claim is given up by a lock going, with nothing written.
///
/// Broken deliberately two ways. Dropping the `is_the_sessions_file` arm
/// from the watcher leaves the page saying nobody has talked about the
/// note for the rest of the session. And having `App::sessions` read the
/// file itself again puts the parse back on every frame -- which this
/// cannot see, so the measurement above is what stands for it, and the
/// second half of this test is about the other direction: that the kept
/// copy is there at all before anybody writes anything.
#[test]
fn the_notes_hear_a_conversation_written_down_by_another_window() {
    let scratch = support::Scratch::new("agent-notes-hear-the-table");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456V\"\nsaid = \"somebody else will talk about this\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Not),
        "the note claims a conversation nobody has had"
    );
    // The page cannot hear the file until there is a directory to watch,
    // and on a project nobody has ever had a conversation in there is
    // none. This is the precondition rather than the hearing -- the
    // assertion below drives the event by hand, so it passes either way,
    // and this is the half that would not.
    let table = obelus_agent::acp::sessions::path(scratch.path()).expect("a path");
    assert!(
        table.parent().is_some_and(std::path::Path::is_dir),
        "there is no directory to watch, so the first write elsewhere goes unheard"
    );

    // The other Obelus writes one down, and this one hears the file.
    obelus_agent::acp::sessions::change(scratch.path(), None, |kept| {
        kept.put(
            &obelus_agent::chats::ChatId::Note(
                obelus_git::todo::NoteId::read("0123456V").expect("a name"),
            ),
            "fake",
            scratch.path(),
            obelus_agent::acp::sessions::Kept {
                session: "s-theirs".to_string(),
                title: None,
                told: None,
                introduced: false,
                last: Some(1_700_000_000),
            },
        );
    });
    app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path: obelus_agent::acp::sessions::path(scratch.path()).expect("a path"),
    }));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Yes),
        "the page never heard that the conversation was written down"
    );
}

/// A note rewritten in another window is offered to the agent again.
///
/// The box asks on every frame whether the agent has been told what the
/// note says now, and offers to ask about it where it has not -- twice a
/// frame, for the editor and for the status row. Reading the notes file
/// there is 29us for one note and 225us for twenty, so the words are kept;
/// and a kept copy with nothing to refresh it is a box comparing against
/// what the note used to say. What makes keeping it honest is that every
/// way a note changes -- this page, the reader's own editor, a second
/// Obelus -- writes the file, and a write is something a watcher hears.
///
/// Broken deliberately two ways. Taking the `reread_the_notes_kept` out of
/// the watcher's arm never offers the rewritten note. Taking the one out of
/// the key that opens the conversation leaves it with no note at all until
/// somebody writes the file, because a watch says what happens next and
/// not what was already there.
#[test]
fn a_note_rewritten_elsewhere_is_offered_again() {
    let scratch = support::Scratch::new("agent-note-header-follows");
    support::make_room_for_notes(scratch.path());
    let notes = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &notes,
        "[[todo]]\nid = \"0123456W\"\nsaid = \"what it said on Monday\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    // The agent was told it already, so there is nothing to offer yet.
    remember_telling(
        &scratch,
        "0123456W",
        "s-kept",
        Some("what it said on Monday"),
    );

    let (mut app, _events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    // Running, because what the agent was told is written down against
    // the agent that was told it.
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    assert!(app.chat().is_some(), "no conversation about the note");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.is_about_a_note(),
        "the conversation has no note to compare against"
    );
    assert_eq!(
        app.chat().and_then(|chat| chat.suggestion()),
        None,
        "the box offered a note the agent has already been told"
    );

    // The other window rewrites it, and this one hears the file.
    std::fs::write(
        &notes,
        "[[todo]]\nid = \"0123456W\"\nsaid = \"what it says on Tuesday\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes again");
    app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path: notes,
    }));
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.chat().and_then(|chat| chat.suggestion()),
        Some("Look into this"),
        "the box went on comparing against what the note used to say"
    );
}

/// The header says which branch the agent's changes are on, once it has
/// made one -- found from where the change landed, not from anything it
/// said.
///
/// A real repository with a linked worktree beside it, which is what an
/// agent following the project's workflow makes, and a second repository
/// that has nothing to do with this one. The calls are delivered by hand
/// because the fake agent's own edit is of a file in this checkout, and
/// what is being asked is which tree a path is in.
///
/// Each change is asked about with its kind and its file, and finished by
/// an update that carries its id and its state and nothing else, which is
/// how an agent says it: whatever an update leaves out is what it said
/// before.
///
/// Broken deliberately seven ways, and each fails here: taking the state
/// out of `Chat::wrote` names the branch while the change is still being
/// asked about; reading the kind and the file off the update rather than
/// the row never names one; taking the project out of
/// `hear_where_it_wrote` names the other repository's branch; taking the
/// call to it out of the tool arm never names one; leaving the branch out
/// of `header` never draws it; and taking the conversations out of
/// `forget_what_git_said` keeps the branch the checkout has moved off;
/// and asking a tree that has gone without looking names the reader's.
#[test]
fn the_header_says_which_branch_the_agent_changed_files_on() {
    let scratch = support::Scratch::new("agent-header-branch");
    let git = |directory: &Path, arguments: &[&str]| {
        let outcome = std::process::Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "obelus")
            .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
            .env("GIT_COMMITTER_NAME", "obelus")
            .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
            .output()
            .expect("running git");
        assert!(
            outcome.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
    };
    let (main, elsewhere) = (scratch.join("main"), scratch.join("elsewhere"));
    for (tree, branch) in [(&main, "master"), (&elsewhere, "theirs")] {
        std::fs::create_dir_all(tree).expect("a checkout");
        git(
            tree,
            &["init", "--quiet", &format!("--initial-branch={branch}")],
        );
        std::fs::write(tree.join("file.rs"), "fn main() {}\n").expect("a file");
        git(tree, &["add", "file.rs"]);
        git(tree, &["commit", "--quiet", "-m", "committed"]);
    }
    // Inside the main checkout, where the workflow puts it.
    let feature = main.join(".worktree").join("feature");
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

    let (mut app, events) = wired();
    app.working_directory_for_test(main.clone());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    let said = |app: &mut App, call: obelus_agent::acp::Call, status: &str| {
        app.handle(Event::Agent(obelus_agent::Event::Acp(
            obelus_agent::acp::Incoming::Update {
                session: obelus_agent::acp::SessionId::new("s-1"),
                update: obelus_agent::acp::Update::Tool {
                    call: Box::new(call),
                    status: status.to_string(),
                },
            },
        )));
    };
    let asked = |app: &mut App, id: &str, tree: &Path| {
        let call = obelus_agent::acp::Call {
            id: id.to_string(),
            title: "Edit file.rs".to_string(),
            kind: "edit".to_string(),
            places: vec![obelus_agent::acp::Place {
                path: tree.join("file.rs"),
                line: None,
            }],
            ..obelus_agent::acp::Call::default()
        };
        said(app, call, "pending");
    };
    let done = |app: &mut App, id: &str| {
        let call = obelus_agent::acp::Call {
            id: id.to_string(),
            ..obelus_agent::acp::Call::default()
        };
        said(app, call, "completed");
    };
    // The header's own row, so that a branch said anywhere else on the
    // screen is not taken for it.
    let header = |app: &mut App| {
        screen(app)
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .to_string()
    };

    // Asked about, not made: there is no branch to speak of yet.
    asked(&mut app, "c-1", &feature);
    assert_eq!(
        app.branch_this_conversation_works_on(),
        None,
        "a change still waiting on the reader named a branch"
    );

    done(&mut app, "c-1");
    let row = header(&mut app);
    assert_eq!(
        app.branch_this_conversation_works_on(),
        Some(&obelus_git::Head::Branch("feature".to_string())),
        "the change in the worktree named no branch:\n{row}"
    );
    assert!(
        row.contains("feature"),
        "the header does not say the branch:\n{row}"
    );

    // Another repository is not this work.
    asked(&mut app, "c-2", &elsewhere);
    done(&mut app, "c-2");
    assert_eq!(
        app.branch_this_conversation_works_on(),
        Some(&obelus_git::Head::Branch("feature".to_string())),
        "a change in another repository took the header"
    );

    // And the reader's own checkout is, and is the one most worth seeing.
    asked(&mut app, "c-3", &main);
    done(&mut app, "c-3");
    assert_eq!(
        app.branch_this_conversation_works_on(),
        Some(&obelus_git::Head::Branch("master".to_string())),
        "a change in the reader's checkout did not move the header"
    );

    // The reader moves their checkout in a shell, and the watch on it says
    // so: the header follows the status row rather than keeping the branch
    // the change was made on.
    git(&main, &["switch", "--quiet", "-c", "fix"]);
    app.handle(Event::Watched(obelus_watch::Changed {
        path: main.join(".git").join("HEAD"),
    }));
    assert_eq!(
        app.branch_this_conversation_works_on(),
        Some(&obelus_git::Head::Branch("fix".to_string())),
        "the header kept a branch the checkout has moved off"
    );

    // And a worktree taken away says nothing, rather than the branch of
    // the checkout it was inside.
    asked(&mut app, "c-4", &feature);
    done(&mut app, "c-4");
    git(
        &main,
        &["worktree", "remove", feature.to_str().expect("a path")],
    );
    app.handle(Event::Watched(obelus_watch::Changed {
        path: main.join(".git").join("HEAD"),
    }));
    assert_eq!(
        app.branch_this_conversation_works_on(),
        None,
        "a worktree that has gone was named by the checkout around it"
    );
}

/// The notes page hears a conversation being taken up elsewhere, through a
/// watch it took itself.
///
/// Every other test of this says the notice by hand, which asks only what
/// Obelus does once it has been told. This one asks the question those
/// cannot: whether anything would ever tell it. A real watcher, a real
/// claim taken in the claims directory, and no event delivered by the test
/// at all.
///
/// Broken deliberately by taking the `watch_directory` out of the notes'
/// opening, or the `create_dir_all` in front of it -- a watch on a
/// directory that is not there is a watch on nothing. Either way nothing
/// arrives and this waits out its deadline.
#[test]
fn the_notes_hear_a_claim_through_a_watch_they_took_themselves() {
    let scratch = support::Scratch::new("agent-notes-real-watch");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456X\"\nsaid = \"somebody will take this up\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.agents_root_for_test(agents_root());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    // The whole of what starting means, which is the only way to an
    // application with a watcher in it.
    app.start(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Not),
        "the note claims a conversation nobody has had"
    );

    // Another Obelus takes it up. Nothing is delivered by hand: what has to
    // happen is that the claim's file appearing reaches this application.
    let theirs = obelus_agent::chats::claim(
        scratch.path(),
        &obelus_agent::chats::ChatId::Note(
            obelus_git::todo::NoteId::read("0123456X").expect("a name"),
        ),
    )
    .expect("their claim");
    pump(&mut app, &events, "the claim to be heard", |app| {
        app.talked_about().first() == Some(&obelus_component::todo::Talked::Elsewhere)
    });

    // And letting go is heard the same way.
    drop(theirs);
    pump(&mut app, &events, "the claim being let go", |app| {
        app.talked_about().first() == Some(&obelus_component::todo::Talked::Not)
    });
}

/// And the table beside the claims, through a watch the notes took
/// themselves.
///
/// Which note has a conversation is kept rather than parsed on every frame
/// the page draws. A kept answer is only as honest as what refreshes it,
/// and what refreshes this one is hearing the file -- so the watch existing
/// is the whole of the promise, and every other test of it says the notice
/// by hand.
///
/// Broken deliberately by taking the `watch` off the table in the notes'
/// opening, or the `create_dir_all` in front of it: on a project where no
/// conversation has ever been written down there is no directory to watch,
/// which is exactly the project this test builds.
#[test]
fn the_notes_hear_the_table_through_a_watch_they_took_themselves() {
    let scratch = support::Scratch::new("agent-notes-real-table-watch");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Y\"\nsaid = \"somebody will talk about this\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.agents_root_for_test(agents_root());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.start(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert_eq!(
        app.talked_about().first(),
        Some(&obelus_component::todo::Talked::Not),
        "the note claims a conversation nobody has had"
    );

    obelus_agent::acp::sessions::change(scratch.path(), None, |kept| {
        kept.put(
            &obelus_agent::chats::ChatId::Note(
                obelus_git::todo::NoteId::read("0123456Y").expect("a name"),
            ),
            "fake",
            scratch.path(),
            obelus_agent::acp::sessions::Kept {
                session: "s-theirs".to_string(),
                title: None,
                told: None,
                introduced: false,
                last: Some(1_700_000_000),
            },
        );
    });
    pump(&mut app, &events, "the table to be heard", |app| {
        app.talked_about().first() == Some(&obelus_component::todo::Talked::Yes)
    });
}

/// A conversation about a note takes its own watch on the notes, because
/// the page that usually holds one may never have been opened.
///
/// Reached from the list of conversations rather than from the notes, which
/// is the way in that leaves that page shut: the box offers a rewritten
/// note again from a kept copy of the notes, and without a watch of its own
/// that copy would be whatever the file said when the conversation opened,
/// for ever.
///
/// Broken deliberately by taking the notes out of `settle_the_watches` --
/// the rewritten note is never offered -- or by taking the
/// `reread_the_notes_kept` out of the list's way in, which leaves the
/// conversation with no note at all.
#[test]
fn a_conversation_about_a_note_watches_the_notes_without_the_page() {
    let scratch = support::Scratch::new("agent-header-real-watch");
    support::make_room_for_notes(scratch.path());
    let notes = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &notes,
        "[[todo]]\nid = \"0123456Z\"\nsaid = \"what it said on Monday\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    remember_telling(
        &scratch,
        "0123456Z",
        "s-kept",
        Some("what it said on Monday"),
    );

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.agents_root_for_test(agents_root());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.start(sender);
    // Running, because what the agent was told is written down against
    // the agent that was told it.
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    support::lay_out(&mut app, WIDTH, HEIGHT);

    // In through the list, so the notes page is never opened and never
    // takes the watch this is about.
    support::press(&mut app, KeyCode::F(4));
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.notes().is_none(), "the notes page is open after all");
    let _ = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        app.is_about_a_note(),
        "the conversation has no note to compare against"
    );
    assert_eq!(
        app.chat().and_then(|chat| chat.suggestion()),
        None,
        "the box offered a note the agent has already been told"
    );

    std::fs::write(
        &notes,
        "[[todo]]\nid = \"0123456Z\"\nsaid = \"what it says on Tuesday\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes again");
    pump(&mut app, &events, "the note being rewritten", |app| {
        app.chat().and_then(|chat| chat.suggestion()).is_some()
    });
}

/// An Obelus that is killed gives its conversations up, and the window
/// beside it says so without being touched.
///
/// The whole of the promise, end to end: a real lock on a real claim, heard
/// through a watch this application took itself, let go the way a killed
/// process lets go -- the descriptor closed by the kernel, the file left
/// exactly where it was -- and the note goes back to being the reader's,
/// with nothing pressed and no event delivered by the test.
///
/// It is the case the claim was written for: "an Obelus that is killed,
/// crashes or loses power gives it up without being asked", and the one
/// that read as having no notice at all. The notice is the kernel closing
/// the dead process's files, which a watcher reports as a close by a
/// writer.
///
/// The pieces have tests of their own -- `obelus_watch` for the event,
/// `chats` for a lock seen through a descriptor that cannot write, and the
/// two beside this for the watch being taken -- and this is the one that
/// puts them together.
///
/// Linux's, and only there. What the notice is made of is inotify's
/// `IN_CLOSE_WRITE` -- and neither macOS's FSEvents nor Windows's
/// `ReadDirectoryChangesW` has an event for a file another process
/// closed, so there is nothing for `notify` to hand over. See
/// `obelus_watch` for what that costs, which is a stale lock on a page
/// the reader is already looking at and not a conversation they cannot
/// open.
#[test]
#[cfg(target_os = "linux")]
fn an_obelus_that_is_killed_gives_its_conversation_back() {
    let scratch = support::Scratch::new("agent-killed-gives-back");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"01234560\"\nsaid = \"they will die holding this\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let which = obelus_agent::chats::ChatId::Note(
        obelus_git::todo::NoteId::read("01234560").expect("a name"),
    );
    let claims = obelus_agent::chats::directory(scratch.path()).expect("somewhere");
    std::fs::create_dir_all(&claims).expect("the claims directory");
    let claim = claims.join(which.file_name());

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.agents_root_for_test(agents_root());
    app.configure(
        obelus_config::Config {
            agent: Some("fake".to_string()),
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.start(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::lay_out(&mut app, WIDTH, HEIGHT);

    // The other Obelus, holding a real exclusive lock on the claim. A lock
    // belongs to the open file and not to the process, so one taken here is
    // as much somebody else's as one taken in another window -- which is
    // what lets this test be both windows.
    let mut theirs = obelus_agent::chats::claim(scratch.path(), &which).expect("their claim");
    pump(&mut app, &events, "their claim to be heard", |app| {
        app.talked_about().first() == Some(&obelus_component::todo::Talked::Elsewhere)
    });

    // And it dies, without tidying anything up: the lock goes because the
    // kernel closes the descriptor, and the file stays exactly where it
    // was. That is what makes this the hard case -- nothing was written and
    // nothing was removed.
    theirs.as_if_this_obelus_died_for_test();
    drop(theirs);
    assert!(
        claim.exists(),
        "the file went with the claim, so this is the easy case after all"
    );
    pump(&mut app, &events, "the claim to be given up", |app| {
        app.talked_about().first() == Some(&obelus_component::todo::Talked::Not)
    });
}

/// The agent dying takes every conversation's session with it, so one the
/// reader goes back to afterwards asks for another rather than talking into
/// a name the new process never gave.
///
/// A process that has gone has taken its sessions with it; what a
/// conversation was holding is a name nobody on the other end knows. Kept,
/// it was a conversation that looked settled: showing it asked for nothing,
/// and what the reader typed into it went to that name and nowhere.
///
/// Deliberate break: leave the conversations as they are in the `Gone` arm
/// of `on_acp`. The words typed into the first conversation after the
/// agent has been started again for the second are never answered.
#[test]
fn a_conversation_the_agent_died_under_asks_for_another() {
    let scratch = support::Scratch::new("agent-died");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456R\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    // Chosen and installed, because starting it again is starting what the
    // settings name, the way the install left it.
    let root = agents_root_for("died");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    // One conversation the reader has said something in.
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        said_in_transcript(app, "heard you")
    });
    let first = app.current_document_for_test().expect("the conversation");

    // The agent dies.
    support::type_text(&mut app, "/die");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to go", |app| {
        app.talking() == obelus_agent::Talking::Gone
    });
    // The reader goes to a note's conversation, which starts the agent
    // again to ask for its session.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the note's session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // And back to the first, where they say something again.
    app.go_to_document_for_test(first);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::type_text(&mut app, "/echo again");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "an answer in the first conversation",
        |app| {
            app.chat().is_some_and(|chat| {
                chat.rows(WIDTH)
                    .iter()
                    .filter(|row| row.text().contains("heard you"))
                    .count()
                    == 2
            })
        },
    );
}

/// An answer about a request nobody is waiting on any more goes to nobody,
/// rather than to a conversation that is waiting on a request of its own.
///
/// The reader opened a conversation and closed it before its session came.
/// The answer to it named a request no conversation held, and the rule for
/// an answer nothing numbered -- the first conversation with no session --
/// handed it to the next conversation, which was waiting on its own: that
/// one wore the wrong session, and its own answer went to nobody.
///
/// Deliberate break: fall back to the first conversation with no session
/// whatever the answer's number, the way it did. The note's conversation
/// ends up on `s-1`, the session asked for the one that was closed.
#[test]
fn an_answer_nobody_is_waiting_for_goes_to_nobody() {
    let scratch = support::Scratch::new("agent-closed-opening");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456S\"\nsaid = \"wire the counts tree up to the search\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    // A conversation opened -- which asks for its session -- and closed
    // before the answer is read: nothing here takes events off the channel
    // until the pump below.
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentClose);
    // And a note's, which asks for its own.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the note's session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert_eq!(
        app.chat_session_for_test().as_deref(),
        Some("s-2"),
        "the note's conversation wears the session asked for another"
    );
}

/// The settings page stops saying it is asking once the agent has gone.
///
/// Deliberate break: leave `agents.asking` alone in the `Gone` arm. The
/// page says it is asking an agent that is not there to answer.
#[test]
fn the_settings_page_stops_asking_an_agent_that_has_gone() {
    use obelus_command::Command;

    let root = agents_root_for("asking-gone");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, _events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    assert!(
        app.agent_offering()
            .and_then(|offering| offering.silence)
            .is_some_and(|silence| silence.starts_with("Asking")),
        "it never asked, so this proves nothing"
    );
    app.handle(Event::Agent(obelus_agent::Event::Acp(
        obelus_agent::acp::Incoming::Gone(Some("it fell over".to_string())),
    )));
    let silence = app
        .agent_offering()
        .and_then(|offering| offering.silence)
        .unwrap_or_default();
    assert!(
        !silence.starts_with("Asking"),
        "the page is still asking an agent that has gone: {silence}"
    );
}

/// An agent that answers that it has nothing to be set is not an agent
/// that has not answered.
///
/// Deliberate break: throw away an empty answer in the `Offers` arm, the
/// way it did. The page says it has heard nothing.
#[test]
fn an_agent_with_nothing_to_set_says_so() {
    use obelus_command::Command;

    let root = agents_root_for("nothing-to-set");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "nothing-to-change".to_string(),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    let silence = |app: &App| {
        app.agent_offering()
            .and_then(|offering| offering.silence)
            .unwrap_or_default()
    };
    pump(&mut app, &events, "the answer", |app| {
        !silence(app).starts_with("Asking")
    });
    assert_eq!(silence(&app), "There is nothing to set for fake");
}

/// That a choice is not offered any more is said once in a conversation,
/// however many sessions it has had.
///
/// A conversation nothing has been said in gets a new session each time
/// the reader comes back to it, and what had been asked of the last one
/// goes with it -- which is what the sentence was counted against.
///
/// Deliberate break: count what has been said against `started_on` again.
/// Coming back says it a second time.
#[test]
fn a_choice_not_offered_is_said_once_however_many_sessions() {
    let (mut app, events) = wired();
    let mut config = obelus_config::Config::default();
    config.set_agent_default("fake", "model", "brilliant");
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let said = |app: &App| {
        app.chat().map_or(0, |chat| {
            chat.rows(WIDTH)
                .iter()
                .filter(|row| row.text().contains("No longer offered by"))
                .count()
        })
    };
    // Waited for rather than read at `Ready`: what the session is set to
    // arrives after the answer that opens it.
    pump(&mut app, &events, "it to be said", |app| said(app) == 1);

    // Away to a file and back, with nothing said: a session of its own
    // again.
    app.open_buffer_for_test(support::open_fixture("sample.rs"));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "a session of its own again", |app| {
        app.chat_session_for_test().as_deref() == Some("s-2")
            && app
                .agent_settings()
                .iter()
                .any(|setting| setting.id == "model")
    });
    // And a moment more, for the settings to be answered if they will be.
    settle(&mut app, &events, Duration::from_millis(300));
    assert_eq!(said(&app), 1, "said again for the second session");
}

/// Installing the agent again does not ask the process still running the
/// version before it.
///
/// The copy is thrown away because it is the old version's list; asked of
/// the process that is still running that version, it came straight back.
///
/// Deliberate break: ask again in `on_installed`, the way it did. The log
/// has a second `session/new`.
#[test]
fn installing_does_not_ask_the_version_before_it() {
    use obelus_command::Command;

    let root = agents_root_for("install-no-ask");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let log = root.join("asked.log");
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    asked(&mut app, &events, &log, "session/delete s-1");
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "fake".to_string(),
        failure: None,
    }));
    settle(&mut app, &events, Duration::from_millis(300));
    let asked = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        asked.matches("session/new").count(),
        1,
        "the version before the install was asked again:\n{asked}"
    );
}

/// Showing a conversation for an agent that is not installed does not say
/// so into it every time.
///
/// Deliberate break: start the agent in `settle_the_sessions` without
/// asking whether it is installed. Each visit adds a line.
#[test]
fn an_agent_not_installed_is_not_said_into_the_conversation_on_every_visit() {
    let (mut app, _events) = wired();
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    for _ in 0..2 {
        app.new_conversation();
        support::lay_out(&mut app, WIDTH, HEIGHT);
        app.open_buffer_for_test(support::open_fixture("sample.rs"));
        support::lay_out(&mut app, WIDTH, HEIGHT);
    }
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let said = app.chat().map_or(0, |chat| {
        chat.rows(WIDTH)
            .iter()
            .filter(|row| row.text().contains("is not installed"))
            .count()
    });
    assert_eq!(
        said,
        0,
        "said on a visit, where it waits for a word:\n{}",
        screen(&mut app)
    );
}

/// Choosing the agent again after turning it off gives the conversation on
/// screen a session on the new process, and nothing the old one says on
/// its way out is taken for the new one's.
///
/// Two things went wrong here. The process that was stopped says it has
/// gone as its connection closes, which is after the next one has started
/// -- and a word from the connection that is not the one running was read
/// as the one running's, so the new process was taken to have died with
/// the old. And the conversation on screen had asked for its session this
/// showing already, so it did not ask the new agent for one: its row and
/// its `/` list stayed empty until the reader typed.
///
/// Deliberate breaks: hand the running agent every word whichever
/// connection it came from, in the `Heard` arm of the loop, and the new
/// process is taken to have died with the old one before the conversation
/// has its session; leave `asked_while_shown` alone in
/// `let_the_conversations_go`, and nothing is asked for. Either way the
/// wait for the session gives up.
#[test]
fn choosing_the_agent_again_gives_the_conversation_a_session_on_it() {
    use obelus_command::Command;

    let root = agents_root_for("chosen-again");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });

    // Off, and on again, on its card.
    the_fixture_is_listed(&mut app);
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.config().agent, None, "not turned off");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.config().agent.as_deref(),
        Some("fake"),
        "not chosen again"
    );
    support::press(&mut app, KeyCode::Esc);
    support::lay_out(&mut app, WIDTH, HEIGHT);

    pump(&mut app, &events, "a session on the new process", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    // And it stays there: the old process's last word arrives after the
    // new one has answered as often as before.
    settle(&mut app, &events, Duration::from_millis(300));
    assert_eq!(
        app.talking(),
        obelus_agent::Talking::Ready,
        "the new process was taken for the old one:\n{}",
        screen(&mut app)
    );
}

/// An agent chosen again gets a session of its own for the conversation on
/// screen, whatever the one before it had already said.
///
/// Not only what the stopped connection says after it has been stopped:
/// what it said before, and nobody has read yet, is in the loop's queue
/// when the next connection starts -- and the answer to a request the old
/// one was answering was taken as the answer to the new one's first, so
/// the conversation wore a session on a process that had gone.
///
/// Deliberate break: hand every word to the running agent whichever
/// connection it came from, as `on_acp` did. The conversation's session is
/// the old process's `s-1`.
#[test]
fn a_word_the_last_agent_had_already_said_is_not_the_next_ones() {
    use obelus_command::Command;

    let root = agents_root_for("queued");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    // A conversation asks the first process for a session, and the answer
    // is left in the queue: nothing here reads it until the pump below.
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    std::thread::sleep(Duration::from_millis(500));

    // Off and on again, before any of it has been read.
    the_fixture_is_listed(&mut app);
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::BackTab);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Esc);
    support::lay_out(&mut app, WIDTH, HEIGHT);

    // The new process opens `s-1` to say what it offers and `s-2` for the
    // conversation.
    pump(&mut app, &events, "the conversation's session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    settle(&mut app, &events, Duration::from_millis(300));
    assert_eq!(
        app.chat_session_for_test().as_deref(),
        Some("s-2"),
        "the conversation wears a session on the process that was stopped"
    );
}

/// A session nobody is waiting for any more is let go, on the agent's side
/// as well.
///
/// Deliberate break: leave an answer nobody holds where it lands. The
/// agent is never told, and the wait for the log to say so gives up.
#[test]
fn a_session_nobody_is_waiting_for_is_let_go() {
    let scratch = support::Scratch::new("agent-orphan");
    let log = scratch.path().join("asked.log");
    let (mut app, events) = wired();
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            format!("log={}", log.display()),
        ],
    );
    // Opened, which asks, and closed before the answer is read.
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::DocumentClose);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    asked(&mut app, &events, &log, "session/delete s-1");
}

/// A conversation that goes on after the agent died under it tells the new
/// session what it has told none: who it is talking to.
///
/// A conversation about nothing in particular takes nothing up again -- it
/// has no note to find its old session by -- so the session it gets after
/// the agent is started again is a new one, and a new one has been told
/// nothing. It was sent the reader's words and nothing else, because the
/// conversation still counted itself as having said its piece.
///
/// Deliberate break: leave `introduced` alone in
/// `forget_what_the_agent_held`. The second message arrives as one block.
#[test]
fn a_conversation_after_the_agent_died_introduces_itself_again() {
    let root = agents_root_for("reintroduced");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let blocks = |app: &App, how_many: &str| {
        app.chat().map_or(0, |chat| {
            chat.rows(WIDTH)
                .iter()
                .filter(|row| row.text().contains(&format!("blocks={how_many}")))
                .count()
        })
    };
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the first answer", |app| {
        blocks(app, "2") == 1
    });

    support::type_text(&mut app, "/die");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to go", |app| {
        app.talking() == obelus_agent::Talking::Gone
    });
    support::type_text(&mut app, "/blocks again");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        blocks(app, "2") + blocks(app, "1") == 2
    });
    assert_eq!(
        blocks(&app, "2"),
        2,
        "the new session was not told who it is talking to:\n{}",
        screen(&mut app)
    );
}

/// What an agent says it can be set to after the answer that opened the
/// session to ask is heard, and not taken for "nothing".
///
/// Deliberate break: drop what the session opened to ask says after its
/// answer, the way `Talk::on` did. The list never has the model in it.
#[test]
fn what_an_agent_offers_after_its_answer_is_heard() {
    use obelus_command::Command;

    let root = agents_root_for("offers-later");
    std::fs::create_dir_all(&root).expect("the root");
    the_fixture_is_installed(&root);
    let (mut app, events) = wired();
    app.agents_root_for_test(root);
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &[
            "tests/fixtures/fake-agent.sh".to_string(),
            "options-later".to_string(),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    pump(&mut app, &events, "the model among what it offers", |app| {
        app.agent_offering()
            .is_some_and(|offering| offering.offers.iter().any(|offer| offer.id == "model"))
    });
}

/// Installing the agent in use while nothing is running asks what the new
/// version offers.
///
/// Nothing running means nothing that could answer for the version before,
/// so starting it now starts the new one -- and the page, which has just
/// thrown its list away, says it has heard nothing otherwise.
///
/// Deliberate break: ask nothing in `on_installed`. The wait for the list
/// gives up.
#[test]
fn installing_the_agent_in_use_asks_the_new_version() {
    use obelus_command::Command;

    let root = agents_root_for("installed-asks");
    std::fs::create_dir_all(&root).expect("the root");
    let (mut app, events) = wired();
    app.agents_root_for_test(root.clone());
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    obelus_app::app::dispatch::dispatch(&mut app, Command::ConfigOpen);
    the_fixture_is_installed(&root);
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "fake".to_string(),
        failure: None,
    }));
    pump(&mut app, &events, "what the new version offers", |app| {
        app.agent_offering()
            .is_some_and(|offering| !offering.offers.is_empty())
    });
}

/// A second choice the agent no longer offers is said as well, when the
/// first has been said already.
///
/// Deliberate break: remember what has been said by the setting alone.
/// The reader's new choice, gone too, is never mentioned.
#[test]
fn a_second_choice_not_offered_is_said_too() {
    let (mut app, events) = wired();
    let mut config = obelus_config::Config::default();
    config.set_agent_default("fake", "model", "brilliant");
    app.configure(config, Vec::new());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let said = |app: &App| {
        app.chat().map_or(0, |chat| {
            chat.rows(WIDTH)
                .iter()
                .filter(|row| row.text().contains("No longer offered by"))
                .count()
        })
    };
    pump(&mut app, &events, "the first to be said", |app| {
        said(app) == 1
    });

    // Another choice, which the agent does not offer either, and a session
    // of its own again to open on it.
    let mut config = obelus_config::Config::default();
    config.set_agent_default("fake", "model", "superb");
    app.configure(config, Vec::new());
    app.open_buffer_for_test(support::open_fixture("sample.rs"));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the second to be said", |app| {
        said(app) == 2
    });
}

/// A conversation shown while its agent was not installed asks for a
/// session once it is.
///
/// Deliberate break: leave `asked_while_shown` as it was when the install
/// finishes. Nothing is asked until the reader types, and the wait gives
/// up.
#[test]
fn a_conversation_asks_for_its_session_once_the_agent_is_installed() {
    let root = agents_root_for("installed-late");
    std::fs::create_dir_all(&root).expect("the root");
    let (mut app, events) = wired();
    app.agents_root_for_test(root.clone());
    let config = obelus_config::Config {
        agent: Some("fake".to_string()),
        ..obelus_config::Config::default()
    };
    app.configure(config, Vec::new());
    app.new_conversation();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    the_fixture_is_installed(&root);
    app.handle(Event::Agent(obelus_agent::Event::Installed {
        id: "fake".to_string(),
        failure: None,
    }));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
}

/// A question up in one conversation does not hold another still.
///
/// One agent is behind every conversation in the window, on one connection,
/// and the protocol's crate reads nothing more from it while a handler is
/// running. The permission handler used to wait for the reader inside the
/// handler, so a card in one conversation stopped every other one: a
/// second conversation opened beside it never got its session, and what
/// was said in it was never answered, until the card was.
///
/// Deliberate break: `answered.await` back inside the permission handler,
/// responding there. The second conversation's session never arrives.
#[test]
fn a_question_in_one_conversation_does_not_hold_another() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/edit");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking_permission);

    // Another conversation, with the first one's card still up and nobody
    // answering it.
    app.new_conversation();
    app.open_a_session_for_test();
    support::lay_out(&mut app, WIDTH, HEIGHT);
    pump(&mut app, &events, "a session of its own", |app| {
        app.chat_session_for_test().as_deref() == Some("s-2")
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer in the second", |app| {
        said_in_transcript(app, "heard you")
    });
}

/// Two questions at once in one conversation are put one after the other.
///
/// An agent that is not held while it waits can ask twice before the
/// reader has answered once -- two tool calls side by side, each wanting
/// permission. There is one card, so the second waits behind the first;
/// taking its place would cancel the first, which the agent hears as the
/// reader refusing a thing they were never shown. The second call is in
/// the transcript as soon as it is asked, so the page says there is more
/// to come before the card does.
///
/// Deliberate breaks: put a question up whatever is already up, as
/// `ask_permission` did -- the first comes back `[cancelled]`; and leave
/// the waiting call out of the transcript -- the second row is not on the
/// page while the first is asked.
#[test]
fn two_questions_at_once_are_put_one_after_the_other() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pair");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "both questions", |app| {
        app.is_asking_permission() && said_in_transcript(app, "Read the second file")
    });
    assert!(
        said_in_transcript(&app, "Read the first file"),
        "the call being asked about is not in the transcript"
    );

    // The first is the one on the card: allowing it is answered as the
    // first, and the second goes up in its place.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the first answer", |app| {
        said_in_transcript(app, "the first was")
    });
    assert!(
        said_in_transcript(&app, "the first was [once]"),
        "the first question was not the reader's to answer:\n{}",
        screen(&mut app)
    );
    assert!(
        app.is_asking_permission(),
        "the second question never went up"
    );
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        said_in_transcript(app, "the second was")
    });
    assert!(
        said_in_transcript(&app, "the second was [once]"),
        "the second question was not the reader's to answer:\n{}",
        screen(&mut app)
    );
    assert!(!app.is_asking_permission());
}

/// A question the agent takes back is taken off the card.
///
/// `$/cancel_request` is how an agent says it no longer wants an answer --
/// the call it was about overtaken, the turn moved on. Left up, the card is
/// a question nobody is asking, and what the reader chose on it goes to a
/// request that has already been answered.
///
/// Deliberate breaks: answer in `answer_when` without watching for the
/// cancellation -- the agent is never told, and never says so; and drop
/// `Incoming::Withdrawn` where it arrives -- the card stays up after the
/// agent has said it gave up.
#[test]
fn a_question_taken_back_comes_off_the_card() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/takeback");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the agent to hear it was taken back",
        |app| said_in_transcript(app, "it was taken back and I was told"),
    );
    assert!(
        !app.is_asking_permission(),
        "the card is still asking a question nobody is:\n{}",
        screen(&mut app)
    );
}

/// A list opened over a question and closed again leaves the question.
///
/// The question is a card in the conversation, not the list: walking away
/// from a list used to refuse whatever the conversation on screen was
/// asking, which was right when questions were lists and answered "no" for
/// a reader who had only looked at what else was open.
///
/// Deliberate break: refuse the permission in `leave(Layer::Picker)` again.
/// The question is gone once the list closes.
#[test]
fn a_list_closed_over_a_question_leaves_it_asked() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/edit");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking_permission);

    support::press(&mut app, KeyCode::F(2));
    assert!(
        app.picker().is_some(),
        "F2 opened no list over the question"
    );
    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape left the list open");
    assert!(
        app.is_asking_permission(),
        "closing a list refused the question under it"
    );
}

/// Refusing the question up puts up the one waiting behind it.
///
/// Escape answers the card -- the protocol's cancelled -- and the agent is
/// still waiting on the second question, so refusing is as much an end to
/// the first as answering it.
///
/// Deliberate break: leave out `ask_the_next` after `CardOutcome::Cancelled`.
/// The first is refused and nothing goes up after it.
#[test]
fn refusing_a_question_puts_up_the_next() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/pair");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "both questions", |app| {
        app.is_asking_permission() && said_in_transcript(app, "Read the second file")
    });

    support::press(&mut app, KeyCode::Esc);
    pump(&mut app, &events, "the first answer", |app| {
        said_in_transcript(app, "the first was")
    });
    assert!(
        said_in_transcript(&app, "the first was [cancelled]"),
        "escape did not refuse the first:\n{}",
        screen(&mut app)
    );
    assert!(
        app.is_asking_permission(),
        "the second question never went up after the first was refused"
    );
}

/// A question taken back while it waits behind the card is never put, and
/// its call says it stopped.
///
/// It is not on the card, so nothing comes down; what the reader has of it
/// is the call in the transcript, put there waiting when it was asked, and
/// nothing else would ever say that it is not waiting any more.
///
/// Deliberate breaks: mark the call but keep the question in the queue in
/// `take_back` -- it goes up once the first is answered, a question nobody
/// is asking; and drop it without marking the call -- its row still says
/// it is waiting.
#[test]
fn a_question_taken_back_while_it_waits_is_never_put() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/secondback");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        &events,
        "the agent to hear it was taken back",
        |app| said_in_transcript(app, "the waiting one was taken back and I was told"),
    );
    assert!(
        app.is_asking_permission(),
        "the question that was not taken back came off the card"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let row = rows(&dump)
        .into_iter()
        .find(|row| row.contains("Delete the cache"))
        .unwrap_or_else(|| panic!("the waiting call is not in the transcript:\n{dump}"))
        .to_string();
    assert!(
        row.contains("Stopped"),
        "the call taken back still says it is waiting:\n{dump}"
    );

    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    assert!(
        !app.is_asking_permission(),
        "a question taken back was put after all"
    );
}
