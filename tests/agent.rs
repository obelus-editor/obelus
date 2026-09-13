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
use obelus::{
    app::App,
    component::card::{Card, On},
    event::Event,
};

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
    playing(&[])
}

/// The same, with the agent told to play a different shape.
fn playing(how: &[&str]) -> (App, Receiver<Event>) {
    let (mut app, events) = wired();
    let mut arguments = vec!["tests/fixtures/fake-agent.sh".to_string()];
    arguments.extend(how.iter().map(|word| (*word).to_string()));
    app.talk_to("fake", Path::new("sh"), &arguments);
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
    // Including the one it is asking about, which is a row like any other
    // -- waiting, which is what says the question is about it.
    assert!(
        text.contains("Run the tests"),
        "what it is asking about is not in the transcript:\n{text}"
    );
    // And which file it was in, written the way a reader writes a path --
    // relative to the tree obelus was opened on. This is what makes a tool
    // call somewhere to go rather than something to read about.
    assert!(
        text.contains("tests/fixtures/many_lines.rs:7"),
        "the tool call does not say where it was:\n{text}"
    );
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

    support::press_function(&mut app, 4);
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

/// The conversation's status row says what the session is set to: every
/// setting the agent offers, in its own order, as short as it can be said.
///
/// Values, not names and values: what a select is on names itself -- `Fast`
/// is plainly a model and `ask first` is plainly a way of working -- so a
/// name in front of it would be a label on something already labelled. A
/// switch is the other way round, because "on" says nothing and the thing it
/// is about is its name, so the name is written and being off is said by
/// writing it dim.
#[test]
fn the_status_row_says_what_the_session_is_set_to() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the settings", |app| {
        app.agent_settings().len() > 2
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    let status = screen[screen.len() - 1].to_string();
    assert!(
        status.contains("ask first \u{b7} Fast \u{b7} Allow everything"),
        "not every setting, in the agent's order:\n{dump}"
    );
    // The names of the selects are not on it: the row would be twice as
    // long and say the same thing.
    assert!(
        !status.contains("Model"),
        "a select's name is on the row as well as its value:\n{dump}"
    );

    // And the switch is dim while it is off, which is the colour obelus
    // draws everything that is there and not in force. The values are not.
    let styles = support::style_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let ink = |needle: &str| {
        let at = status.find(needle).expect("the words");
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
    use obelus::component::chat::Focus;

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
    // obelus puts on everything with a list behind it.
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
    use obelus::component::chat::Focus;

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
        app.talking() == obelus::app::talking::Talking::Ready
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
/// reader wondering whether obelus had failed to read something.
#[test]
fn an_agent_with_nothing_to_change_says_so() {
    let (mut app, events) = playing(&["nothing-to-change"]);
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen[screen.len() - 1].contains("nothing to change"),
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
/// obelus recognising a name.
#[test]
fn a_mode_offered_both_ways_is_one_setting() {
    // The old way only, which is what the ordinary fake agent plays: the
    // mode comes from `session/new`'s own field, and obelus reads it into a
    // setting called what obelus calls it.
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
/// that answer for the mode it is now in -- so obelus shows the new one the
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
        text.contains("changing the mode"),
        "nothing said why it went back:\n{text}"
    );
}

/// A command named like one of the session's settings is still the agent's
/// command, and still goes to the agent.
///
/// obelus used to take `/model` for itself and open that setting's values
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
    // The agent offers `/model` and obelus also has a `model` setting, so
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
    // other command -- rather than obelus taking the row for itself.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.chat().expect("the chat").writing().text(), "/model ");
    assert!(app.picker().is_none(), "obelus opened a list of its own");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the agent to run it", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
    // obelus then shows -- and what the reader did is in the transcript,
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
    // The command, above the options, with a rule between them -- and the
    // title in the transcript, directly above the card, because that is
    // where what the agent is doing is said.
    let said = at("cargo test --all-features");
    let allow = at("Allow once");
    assert!(said < allow, "the reason is not above the answers:\n{dump}");
    assert!(
        asking[said + 1].contains('\u{2500}'),
        "nothing separates the words from the answers:\n{dump}"
    );
    let asked = at("Run the tests");
    assert!(
        asked < said,
        "what it is asking about is not above the question:\n{dump}"
    );
    // The row of settings is still obelus's status row: a card is part of
    // the conversation rather than a list opened over it.
    assert!(
        asking[asking.len() - 1].contains("ask first"),
        "the card took the status row:\n{dump}"
    );

    // And the answers are still answers: the list walks and chooses.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    assert!(
        screen(&mut app).contains("and I was refused"),
        "the answer the reader chose did not reach the agent"
    );
}

/// The conversation is a dialog: nothing of obelus's own opens over it.
///
/// It is the whole region and it has its own keys, so a command that put a
/// list on top of it would leave two things on screen with one caret and no
/// way to tell which was listening. Escape closes the conversation, and the
/// keys are obelus's again after that.
#[test]
fn nothing_of_obeluss_own_opens_over_the_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let conversation = screen(&mut app);
    for key in ['o', 'e', 'p', 'q'] {
        support::press_control(&mut app, key);
    }
    assert!(
        app.picker().is_none(),
        "a list opened over the conversation"
    );
    assert!(!app.should_quit(), "ctrl+q reached the key table");
    assert_eq!(
        screen(&mut app),
        conversation,
        "something opened over the conversation"
    );

    support::press(&mut app, KeyCode::Esc);
    // A key whose command can run with nothing open: the list of open
    // files is dim on this screen, and a dim command's key does nothing.
    support::press_function(&mut app, 1);
    assert!(
        app.picker().is_some(),
        "escape did not give the key table back"
    );
}

/// A form's answers and the room to write your own are one card.
///
/// A form arrives as a *map* of fields -- JSON objects have no order to
/// keep -- so the order the agent wrote them in is gone by the time obelus
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
        app.talking() == obelus::app::talking::Talking::Ready
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
    // their own answer saying what it is for in the colour obelus writes
    // everything that is not there yet in.
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        rows(&dump).iter().any(|row| row.contains("choose one")),
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you picked review and something else"),
        "the words did not go with the choice:\n{text}"
    );
}

/// The agent asks the reader something, and obelus puts the question.
///
/// A form of three fields, each a card of its own: named answers, the same
/// for a switch, and a box where it takes a number. What goes back is the
/// whole form, keyed by the names the agent gave.
#[test]
fn a_form_the_agent_asks_for_is_put_one_field_at_a_time() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/several");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // Every answer with a box in front of it, none of them ticked: a card
    // that ticked something for the reader would be answering for them.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("[ ] src/acp")),
        "the answers are not ticks:\n{dump}"
    );
    assert!(
        rows(&dump).iter().any(|row| row.contains("[ ] Other")),
        "the box has no tick of its own:\n{dump}"
    );

    // Enter ticks, and the card stays: ticking and sending cannot both be
    // enter.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.is_asking(), "a tick answered the question");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump).iter().any(|row| row.contains("[x] src/acp")),
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
    support::check(
        &format!("card_{WIDTH}x{HEIGHT}"),
        &support::render(&mut app, WIDTH, HEIGHT),
    );

    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer to go back", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        text.contains("you picked two and said where else"),
        "the words did not go with the ticks:\n{text}"
    );
}

/// A question asked while the reader is away from the conversation brings
/// it back.
///
/// The card is drawn inside the conversation, so a question asked after
/// they escaped out of it would be a card nobody can see -- taking their
/// keys, and holding up an agent waiting for an answer it never showed
/// them. Agents ask before anything is said to them: a login, a workspace.
#[test]
fn a_question_asked_while_the_conversation_is_away_brings_it_back() {
    let (mut app, events) = playing(&["asks-at-once"]);

    // Away from it before it has even opened, which escape does while it is
    // not working: the agent goes on starting, and asks with nobody there.
    support::press(&mut app, KeyCode::Esc);
    assert!(
        app.chat().is_none(),
        "escape did not close the conversation"
    );
    assert!(app.card().is_none(), "the question was already here");

    pump(&mut app, &events, "the question", |app| {
        app.card().is_some()
    });
    assert!(
        app.chat().is_some(),
        "the question is on a card nobody can see"
    );
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        rows(&dump)
            .iter()
            .any(|row| row.contains("which workspace am I in")),
        "the question is not on screen:\n{dump}"
    );
}

/// Escape says no to the form, and the agent hears that rather than nothing.
#[test]
fn escape_on_a_form_tells_the_agent_it_was_not_answered() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
    let directory =
        std::env::temp_dir().join(format!("obelus-agent-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");
    let root = directory.join("agents");
    obelus::agent::remember(
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
    app.open_agent();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/die");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "it to stop", |app| {
        app.talking() == obelus::app::talking::Talking::Gone
    });

    // What it says is a line, not the protocol crate's own error with the
    // source path of a cargo registry in it.
    let text = screen(&mut app);
    assert!(
        text.contains("exit status: 3"),
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
            obelus::app::talking::Talking::Thinking | obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/pick");
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking);

    // The agent's thread says the conversation ended, which is the one way
    // obelus hears about it however the agent went.
    app.handle(Event::Acp(obelus::acp::Incoming::Gone(None)));
    assert!(app.card().is_none(), "the card outlived the agent");
    assert!(
        !app.is_asking(),
        "the form is still waiting on a dead agent"
    );
}

/// A row of the transcript that names a file is a place to go.
///
/// Which is what obelus has that a client showing a preview does not: the
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
        app.talking() == obelus::app::talking::Talking::Ready
    });

    // Up from the box, which is empty: the caret cannot move in it, so the
    // key goes to the nearest row of the transcript worth standing on --
    // the command it asked about -- and again to the file it read.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Up);
    assert!(
        matches!(
            app.chat().map(obelus::component::chat::Chat::focus),
            Some(obelus::component::chat::Focus::Transcript(_))
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

/// A conversation of nothing but words scrolls the way it always has.
///
/// The arrows move the nearest thing that can still move: a row to stand on
/// where there is one, and the view itself where there is not. Most
/// conversations have nothing to stand on at all, and they must not have
/// lost a key for it.
#[test]
fn the_arrows_still_scroll_a_transcript_with_nowhere_to_stand() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let chat = app.chat().expect("the conversation");
    assert!(
        chat.rows(60).iter().all(|row| !row.acts()),
        "this conversation has somewhere to stand after all"
    );

    support::press(&mut app, KeyCode::Up);
    assert!(
        matches!(
            app.chat().map(obelus::component::chat::Chat::focus),
            Some(obelus::component::chat::Focus::Writing)
        ),
        "the cursor went somewhere there was nothing to stand on"
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/many");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    // Four reads, one row -- and the one that is not a read is its own row,
    // because a run is a run of one kind.
    let text = screen(&mut app);
    assert!(text.contains("4 files"), "the run is not folded:\n{text}");
    assert!(
        !text.contains("Read src/app"),
        "a folded run is showing its members:\n{text}"
    );
    assert!(
        text.contains("Run the tests"),
        "the call that is not a read was folded in with them:\n{text}"
    );

    // Up from the box walks past the failed call to the run's heading, and
    // enter opens it where it is.
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    support::check(&format!("folded_{WIDTH}x{HEIGHT}"), &dump);
    let opened = rows(&dump);
    let heading = opened
        .iter()
        .position(|row| row.contains("4 files"))
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
        shown[0].contains("Fake Agent") && !shown[0].contains("thinking"),
        "the header is still saying what is happening:\n{dump}"
    );
    let doing = shown
        .iter()
        .position(|row| row.contains("thinking\u{2026}"))
        .unwrap_or_else(|| panic!("nothing says it is working:\n{dump}"));
    assert!(
        shown[doing].contains("esc stops it"),
        "how to stop it is not beside the thing it stops:\n{dump}"
    );

    // And when it is not working, it is not there.
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the end of the turn", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let text = screen(&mut app);
    assert!(
        !text.contains("thinking\u{2026}"),
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
    app.open_agent();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    let reading = app
        .current_buffer()
        .expect("the reader's own file")
        .cursor()
        .line;
    support::type_text(&mut app, "/nowhere");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.chat().is_some(),
        "the conversation hid itself for a file that never opened"
    );
    let buffer = app.current_buffer().expect("the reader's own file");
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/edit");
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the question", App::is_asking_permission);

    // The file, how much it changes, and the lines themselves -- worked out
    // by obelus from the two texts the agent sent, with the engine it works
    // out every other change with.
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
        app.talking() == obelus::app::talking::Talking::Ready
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
            .find(|row| row.contains("thinking"))
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
        app.talking() == obelus::app::talking::Talking::Ready
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

/// A transcript long enough to scroll says so, like everything else.
///
/// It was the one scrolling thing in obelus with no bar: a reader could page
/// through a conversation with nothing on screen answering "how much of this
/// is there, and which part am I looking at". The column it takes is already
/// spare -- the rows are wrapped to leave it -- so nothing moves to make
/// room for it, and the rules above and below close off against it the way
/// they do everywhere else.
#[test]
fn a_transcript_with_more_than_fits_has_a_bar() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
            app.talking() == obelus::app::talking::Talking::Ready
        });
    }

    let dump = support::render(&mut app, 60, 22);
    let drawn = bar(&dump);
    assert!(!drawn.is_empty(), "the transcript has no bar:\n{dump}");
    // Following the end, so the thumb is at the bottom of the track.
    assert_eq!(
        drawn.last(),
        Some(&'\u{2588}'),
        "the thumb is not where the reader is:\n{dump}"
    );
    // And the rules above and below it are closed off against it.
    let ends: Vec<char> = rows(&dump)
        .iter()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, drawn)| drawn.trim_end())
        .filter(|drawn| drawn.contains('\u{2500}'))
        .filter_map(|drawn| drawn.chars().last())
        .collect();
    assert_eq!(
        ends,
        ['\u{2510}', '\u{2518}', '\u{2500}'],
        "the rules do not meet the transcript's bar:\n{dump}"
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
        bar(&scrolled).last(),
        Some(&'\u{2588}'),
        "the thumb stayed at the end while the reader went back:\n{scrolled}"
    );
}
