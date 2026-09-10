//! Talking to an agent, through obelus's own side of the protocol.
//!
//! The agent here is `tests/fixtures/fake-agent.sh`, a real process on the
//! other end of a real pipe that plays one conversation: it thinks, answers
//! in pieces, reads a file back through obelus, uses a tool, and asks
//! permission before finishing. Everything below drives obelus by keys and
//! reads the screen, so what is asserted is what a reader would see.

mod support;

use std::{
    path::Path,
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use crossterm::event::KeyCode;
use obelus::{app::App, event::Event};

/// The screen these tests use.
const WIDTH: u16 = 76;
/// Tall enough for a whole turn's transcript to be on screen at once.
const HEIGHT: u16 = 24;

/// How long to wait for the agent to say something.
///
/// Generous: it is a shell script starting up, and a test that fails because
/// a machine was busy is a test nobody trusts.
const PATIENCE: Duration = Duration::from_secs(10);

/// An application with the loop's channel, and the channel.
fn wired() -> (App, Receiver<Event>) {
    let (sender, events) = channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    (app, events)
}

/// Starts the fake agent and opens the conversation on it.
fn talking() -> (App, Receiver<Event>) {
    let (mut app, events) = wired();
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.open_agent();
    (app, events)
}

/// Handles events until the application satisfies `until`, or gives up.
///
/// Draws between events, because a frame is where obelus settles what it is
/// showing -- and because a test that only handles events would not notice a
/// view that cannot draw what arrived.
fn pump(app: &mut App, events: &Receiver<Event>, what: &str, until: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + PATIENCE;
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

/// The transcript's text, as it is on screen.
fn screen(app: &mut App) -> String {
    let dump = support::render(app, WIDTH, HEIGHT);
    support::text_block(&dump).to_string()
}

/// One whole turn: the handshake, a prompt, what comes back while it works,
/// a file read through obelus, a permission request, and the end.
#[test]
fn a_whole_turn_of_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    // What the agent calls itself, which obelus only knows because it asked.
    assert_eq!(app.agent_name(), Some("Fake Agent 0.1"));

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
    // And the file it asked obelus to read, which obelus answered from the
    // tree it was started on.
    assert!(
        text.contains("saying this is what obelus handed over"),
        "the file was not read back:\n{text}"
    );
    // And the one it asked obelus to *write*, which obelus refuses: it said
    // so in the handshake, and the agent reports back what it was told.
    assert!(
        text.contains("and it refused to write"),
        "obelus wrote a file for an agent:\n{text}"
    );
    // The question is a list, which is what every choice in obelus is.
    assert!(text.contains("Allow once"), "no options:\n{text}");
    assert!(text.contains("Reject"), "no options:\n{text}");

    // Allow it, and the turn finishes.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("and I was refused"),
        "the agent was not told:\n{text}"
    );
}

/// Escape closes the view and the conversation is still there.
#[test]
fn closing_it_keeps_what_was_said() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "remember this");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "an answer", |app| {
        app.chat()
            .is_some_and(|chat| !chat.rows(60).is_empty() && app.is_asking_permission())
    });

    // The question first, because a list is open over the conversation.
    support::press(&mut app, KeyCode::Esc);
    // And now it is finishing the turn, so escape is still "stop what is
    // happening" rather than "close this".
    assert!(app.chat().is_some(), "escape closed it mid-turn");
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    // Idle, so now it closes.
    support::press(&mut app, KeyCode::Esc);
    assert!(app.chat().is_none(), "escape did not close it");

    support::press_alt_key(&mut app, KeyCode::Char('a'));
    let text = screen(&mut app);
    assert!(
        text.contains("remember this"),
        "reopening lost the conversation:\n{text}"
    );
}

/// Escape stops an agent that is working, rather than closing the view.
///
/// One key, and what it means is the same everywhere in obelus: stop what is
/// happening. What is happening while an agent is thinking is the agent.
#[test]
fn escape_stops_an_agent_that_is_working() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "think about it slowly");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to start thinking", |app| {
        app.talking() == obelus::app::talking::Talking::Thinking
    });

    support::press(&mut app, KeyCode::Esc);
    assert!(app.chat().is_some(), "escape closed the view instead");
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("stopped"),
        "it did not say it stopped:\n{text}"
    );
}

/// With no agent chosen there is nothing to talk to, and the view says so
/// rather than being empty.
#[test]
fn the_view_says_when_nobody_is_chosen() {
    let (mut app, _events) = wired();
    app.open_agent();
    let text = screen(&mut app);
    assert!(
        text.contains("no agent is active"),
        "it did not say why it is empty:\n{text}"
    );
    assert_eq!(app.talking(), obelus::app::talking::Talking::Nobody);
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        !app.agent_modes().is_empty()
    });
    assert_eq!(
        app.agent_mode().map(|mode| mode.name.as_str()),
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
        app.agent_mode().map(|mode| mode.name.as_str()),
        Some("write code"),
        "shift+tab did not walk the modes"
    );
    // And round, because there are two of them.
    support::press_shift(&mut app, KeyCode::BackTab);
    assert_eq!(
        app.agent_mode().map(|mode| mode.name.as_str()),
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
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        format!("{second} "),
        "tab did not take the row that was on"
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(text.contains("ran czzz"), "enter did not send it:\n{text}");

    support::type_text(&mut app, "/compact ");
    // And what goes out is the line: the agent parses the name itself, and
    // says which one it ran.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the command to run", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("ran compact"),
        "the command did not run:\n{text}"
    );
}

/// The reader types `/model`, and obelus answers with the model list.
///
/// Not the agent: the choice is one of the session's settings, and the
/// agent's own answer to that command is a dialog it cannot open down a
/// pipe. So the command is never sent, and what opens is the ordinary
/// compact list of what the setting can be.
#[test]
fn a_command_that_names_a_setting_offers_its_values() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        !app.agent_settings().is_empty()
    });
    // Both of them. The switch is only there because obelus said in the
    // handshake that it can show one, so this is that promise as well.
    let named: Vec<&str> = app
        .agent_settings()
        .iter()
        .map(|setting| setting.name.as_str())
        .collect();
    assert_eq!(named, ["Model", "Allow everything"], "the settings");

    // Typed as a command, because that is what the agent calls it and what
    // its own list of commands offers -- so the list of commands is what
    // the reader sees first, and choosing the row is choosing the setting.
    support::type_text(&mut app, "/model");
    let text = screen(&mut app);
    assert!(
        text.contains("/model"),
        "the command is not offered:\n{text}"
    );
    support::press(&mut app, KeyCode::Enter);
    let text = screen(&mut app);
    // The list, with what it is on now marked as such.
    assert!(text.contains("Careful"), "no list of models:\n{text}");
    assert!(
        text.contains("Slower, and better"),
        "the rows do not say what they are:\n{text}"
    );
    assert!(
        text.contains("current"),
        "nothing says which one is on:\n{text}"
    );
    // And nothing was said: the command was a choice, not a message. Which
    // also means the box it was typed in is empty again.
    assert!(
        !text.contains("/model"),
        "the command went into the conversation:\n{text}"
    );
    assert_eq!(
        app.chat().expect("the chat").writing().text(),
        "",
        "what was typed is still in the box"
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
    // obelus then shows.
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

    // The whole list of them is a command of obelus's own, and it says
    // what each setting is on.
    assert!(
        app.offers(obelus::command::Command::AgentSettings),
        "the settings are not offered while an agent is offering some"
    );
    app.open_agent_settings();
    let text = screen(&mut app);
    for word in ["Model", "Careful", "Allow everything", "off"] {
        assert!(text.contains(word), "no {word} in the settings:\n{text}");
    }

    // The switch is two rows of the same list, opened on the side it is on
    // -- and set with a boolean rather than a value id, which is what the
    // agent reads it back as.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let sides: Vec<String> = app
        .picker()
        .expect("the list")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(sides, ["on", "off"], "the switch has other sides");
    assert_eq!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.as_str()),
        Some("off"),
        "the list did not open on the side it is on"
    );
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the switch to go on", |app| {
        app.agent_settings()
            .iter()
            .any(|setting| setting.id == "allow_all" && setting.current == "on")
    });

    // And it runs the other way too: an agent that puts itself on another
    // model says so, and what obelus shows is what the agent last said.
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
    app.open_agent_settings();
    let text = screen(&mut app);
    assert!(
        text.contains("Fast"),
        "the model it moved itself to is not shown:\n{text}"
    );
}
