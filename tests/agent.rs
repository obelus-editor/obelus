//! Talking to an agent, through obelus's own side of the protocol.
//!
//! The agent here is `tests/fixtures/fake-agent.sh`, a real process on the
//! other end of a real pipe that plays one conversation: it thinks, answers
//! in pieces, reads a file back through obelus, uses a tool, and asks
//! permission before finishing. Everything below drives obelus by keys and
//! reads the screen, so what is asserted is what a reader would see.
//!
//! It is `sh` on purpose. A fake agent written in python, node, or a second
//! Rust binary is a test that stops running on somebody else's machine.
//!
//! It is also what holds obelus to its promises, because the protocol's
//! crate cannot: the fixture checks the handshake it was given and answers
//! to the name "Wrong Client" if the client offered to write files, and it
//! asks for a write during the turn and reports back whether it was
//! refused. Both land in `a_whole_turn_of_conversation`. It does the same
//! for the settings -- the boolean one is only offered to a client that
//! said in the handshake that it can draw a switch.

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

/// Escape stops what is happening, and never closes the conversation.
///
/// Mid-turn it is the interrupt; with nothing in flight it does nothing at
/// all, because a conversation is a document and escape leaves whatever is
/// *over* the document being read. It used to close the view, which is what
/// made it the one place in obelus where escape threw something away.
#[test]
fn escape_stops_the_turn_and_never_closes_the_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
        text.contains("Stopped"),
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
    // Enter, which is what every completion in obelus is taken with.
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
        text.contains("Changing the mode"),
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
    // The row of settings is still obelus's status row: a card is part of
    // the conversation rather than a list opened over it.
    assert!(
        asking[asking.len() - 1].contains("ask first"),
        "the card took the status row:\n{dump}"
    );

    // Allow or refuse, and nothing to say in your own words: so there is
    // nowhere on this card for text to go, and the key that would paste it
    // is not offered rather than putting it in the box behind.
    assert!(
        !app.offers(obelus::command::Command::Paste),
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    assert!(
        screen(&mut app).contains("and I was refused"),
        "the answer the reader chose did not reach the agent"
    );
}

/// obelus's own keys work inside a conversation.
///
/// They did not while it was a region over the editor: it answered
/// `Context::Dialog`, where nothing is bound, so `ctrl+p` did nothing and
/// the only way to the palette was to leave. A conversation is a document
/// now, and a document is what obelus's keys are for -- a list opens over
/// it the way it opens over a file, and leaving the list leaves the
/// conversation where it was.
#[test]
fn obeluss_own_keys_work_inside_a_conversation() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
/// The card is drawn inside the conversation, so a question asked while the
/// reader is looking at something else would be a card nobody can see --
/// taking their keys, and holding up an agent waiting for an answer it
/// never showed them. Agents ask before anything is said to them: a login,
/// a workspace.
#[test]
fn a_question_asked_while_the_conversation_is_away_brings_it_back() {
    let (mut app, events) = playing(&["asks-at-once"]);

    // Away from it before it has even opened: a file is opened over the top
    // of it, the way switching documents does, and the agent goes on
    // starting with nobody looking.
    app.open_for_test(std::path::Path::new("tests/fixtures/sample.rs"));
    assert!(
        app.chat().is_none(),
        "the conversation is still what is being read"
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
    let directory = support::Scratch::new("agent-restart");
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
    assert!(text.contains("4 Files"), "the run is not folded:\n{text}");
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
        shown[0].contains("Fake Agent") && !shown[0].contains("thinking"),
        "the header is still saying what is happening:\n{dump}"
    );
    let doing = shown
        .iter()
        .position(|row| row.contains("thinking\u{2026}"))
        .unwrap_or_else(|| panic!("nothing says it is working:\n{dump}"));
    assert!(
        shown[doing].contains("Esc stops it"),
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
    // Where the reader left their own file. Not `current_buffer`, because
    // the conversation is what is current now -- it is a document, and
    // opening it is switching to it.
    let reading = app
        .file(obelus::buffer::DocumentId::new(0))
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
        "the conversation went away for a file that never opened"
    );
    // Which file the reader still has, rather than which is current: the
    // conversation is what is current, because going nowhere went nowhere.
    let buffer = app
        .file(obelus::buffer::DocumentId::new(0))
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
/// It was the one scrolling thing in obelus with no bar: a reader could page
/// through a conversation with nothing on screen answering "how much of this
/// is there, and which part am I looking at". The column it takes is already
/// spare -- the rows are wrapped to leave it -- so nothing moves to make
/// room for it.
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

/// What obelus has to say reaches the reader in a conversation too.
///
/// The conversation draws its own status row, so a note has to be one of the
/// things that row carries. Before it was, every command that answers by
/// saying something answered a reader standing in a conversation with
/// silence -- the key worked, and nothing on the screen said so.
#[test]
fn a_note_is_said_on_a_conversations_own_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the handshake", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
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
/// The agent has no other way to know: `Topic` never left obelus, so a
/// conversation about a note looked exactly like one about nothing, and an
/// agent that cannot name a note will not call the tool that finishes one.
/// It goes in a block of its own beside the reader's first words -- not as a
/// turn of its own, which would have the agent talking before the reader had
/// said anything.
#[test]
fn a_conversation_about_a_note_says_so_in_its_first_message() {
    let scratch = support::Scratch::new("agent-note-opening");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
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
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    assert!(app.chat().is_some(), "no conversation about the note");
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2"),
        "the note did not go with the message:\n{text}"
    );
    assert!(
        text.contains("first=obelus"),
        "obelus's own block is not the first of them:\n{text}"
    );
    // And the reader can see that obelus said it.
    assert!(
        text.contains("Told the agent what this conversation is about"),
        "obelus spoke in the reader's name without saying so:\n{text}"
    );

    // Once. The agent keeps every word of a conversation, so a second
    // message carrying it again would be telling it twice.
    support::type_text(&mut app, "/blocks again");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the second answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("blocks=1"))
        })
    });
}

/// A conversation about nothing in particular says nothing.
///
/// obelus does not put words in the reader's mouth where it has no fact of
/// its own to add: what a loose conversation is about is whatever they type.
#[test]
fn a_loose_conversation_carries_no_opening() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/blocks");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "what it got", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=1"),
        "something went with a conversation about nothing:\n{text}"
    );
}

/// A conversation the agent has forgotten starts a fresh one, in place.
///
/// Agents sweep their conversations up, so a name obelus wrote down last
/// week may mean nothing today. The reply to that is a new session -- which
/// the protocol side already did -- and the conversation it belongs to has
/// to be put back to how one with no session yet looks, or the session
/// arriving next belongs to nobody and every word the reader types is held
/// for a session that is never coming. What told the agent about the note
/// goes with the old session, so it is said again.
#[test]
fn a_conversation_the_agent_has_forgotten_is_started_again() {
    let scratch = support::Scratch::new("agent-forgotten");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    let note = "0123456J";
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");
    // Written down against a name the agent will refuse.
    let id = obelus::todo::NoteId::read(note).expect("a name");
    obelus::acp::sessions::change(scratch.path(), std::slice::from_ref(&id), |remembered| {
        remembered.put(
            &id,
            "fake",
            obelus::acp::sessions::Kept {
                session: "s-gone".to_string(),
                title: None,
            },
        );
    });

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.talk_to(
        "fake",
        Path::new("sh"),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    pump(&mut app, &events, "a session of some kind", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
                .any(|row| row.text.contains("blocks="))
        })
    });
    let text = screen(&mut app);
    assert!(
        text.contains("blocks=2") && text.contains("first=obelus"),
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    assert!(
        app.what_this_conversation_is_about().is_none(),
        "this conversation is about a note, so it proves nothing"
    );

    // The same question `todo_add` puts, arriving the same way.
    let (answer, answered) = futures::channel::oneshot::channel();
    app.handle(Event::Acp(obelus::acp::Incoming::Ask {
        message: "worth writing down?".to_string(),
        fields: vec![obelus::acp::Field {
            name: "notes".to_string(),
            title: "keep which of these".to_string(),
            about: None,
            takes: obelus::acp::Takes::Some {
                values: vec![obelus::acp::Value {
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
    }));
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

/// A plan an agent asks leave to act on is read in the transcript.
///
/// It arrives as the words of the call that is asking -- the protocol's
/// `ToolCallContent::Content`, beside the diff obelus already kept -- and it
/// is longer than a card is tall. So the card carries the answers and the
/// transcript carries the plan: there is where things are read, with the
/// scrolling and the folding and the room, and the plan is still there after
/// the answer is given.
#[test]
fn a_plan_is_read_in_the_transcript_and_the_card_holds_the_answers() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
    let call = at("Approve Plan");
    let heading = at("# The plan");
    let last = at("5. Stop");
    assert!(call < heading, "the plan is not under its call:\n{dump}");
    assert!(heading < last, "the plan is out of order:\n{dump}");
    assert!(
        last < at("No, keep planning"),
        "the plan is not above the answers:\n{dump}"
    );
    // And once, because the card does not print what the reader can
    // already read above it.
    assert_eq!(
        screen
            .iter()
            .filter(|row| row.contains("# The plan"))
            .count(),
        1,
        "the plan is on the screen twice:\n{dump}"
    );

    // Answered: the plan puts itself away and the row keeps the handle, so
    // what was agreed to is a keypress away rather than gone.
    // Down to the second answer and take it, the way a card is walked.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/steps");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the list", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("Step 2 of 3"))
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
    use obelus::component::chat::Speaker;
    let said = app.chat().expect("A conversation").rows(WIDTH);
    assert!(
        said.iter()
            .filter(|row| row.text.contains("write the test"))
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
/// that drops what obelus does not show: the row says nothing and this goes
/// red.
#[test]
fn how_full_the_agent_is_goes_on_the_status_row() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
    // The code, not a sign obelus guessed.
    assert!(
        !status.contains('$'),
        "obelus made up a currency sign:\n{dump}"
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
        app.talking() == obelus::app::talking::Talking::Ready
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
/// declines a mode obelus cannot put: no card, and this goes red.
#[test]
fn somewhere_to_go_is_put_on_a_card_and_opened() {
    let _turn = support::clipboard_turn();
    obelus::links::use_opener_for_test(obelus::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
        obelus::links::opened().as_deref(),
        Some(
            "https://console.example.com/oauth/authorize?client_id=9d1c4a&scope=user%3Ainference&code=1&state=7f2b"
        ),
        "the link was not opened"
    );
    pump(&mut app, &events, "what the agent made of it", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("you went"))
        })
    });
}

/// A URL obelus will not hand to the machine is refused, not declined.
///
/// What happens to one of these is that obelus asks the machine to open it
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
    obelus::links::use_opener_for_test(obelus::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/nowhere");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    assert!(app.card().is_none(), "obelus put a file: url to the reader");
    assert_eq!(obelus::links::opened(), None, "obelus opened a file: url");
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        !rows(&dump).iter().any(|row| row.contains("vscode://")),
        "the url reached the page:\n{dump}"
    );
}

/// Once the reader has been sent, the card gives way to a row.
///
/// The agent is not waiting on obelus any more -- it was told they went --
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
    obelus::links::use_opener_for_test(obelus::links::Opener::Kept);

    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
                .find(|row| row.text.contains("sign in to continue"))
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
                row.text.contains("sign in to continue")
                    && row.state.as_deref() == Some("completed")
            })
        })
    });

    // And the key on the row sends them again, for the tab they closed.
    obelus::links::use_opener_for_test(obelus::links::Opener::Kept);
    assert_eq!(
        obelus::links::opened(),
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
        obelus::links::opened().is_some_and(|url| url.contains("console.example.com")),
        "the row would not send them again"
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
        app.talking() == obelus::app::talking::Talking::Ready
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
    assert!(wide.contains("94%") && wide.contains("mode") && wide.contains("ask first"));

    // Where it does not, the number goes and the other two stay whole.
    let narrow = row(&mut app, 30);
    assert!(
        !narrow.contains("94%"),
        "the number stayed on a row that could not hold it: {narrow:?}"
    );
    assert!(
        narrow.contains("mode"),
        "the keys were pushed off the end: {narrow:?}"
    );
    assert!(
        narrow.contains("ask first"),
        "the settings were cut down to nothing: {narrow:?}"
    );
}

/// The reader's own half of a conversation comes back from the agent.
///
/// `user_message_chunk` was one of the updates obelus dropped. In a live
/// turn it is news obelus already has -- it put those words there itself --
/// but the turn it is for is the one nobody was here for: `session/load`
/// replays a conversation to a client that may be a fresh process, and the
/// reader's half of it comes back only this way. Dropped, a conversation
/// taken up again is a run of answers with no questions above them.
///
/// Broken deliberately by leaving `SessionUpdate::UserMessageChunk` in the
/// arm that drops what obelus does not show: the words never arrive and
/// this goes red.
#[test]
fn the_readers_own_words_come_back_from_the_agent() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/relay");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the turn to end", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });

    // In the reader's voice, because that is whose words they are.
    let said = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .find(|row| row.text.contains("what did we settle on"))
        .map(|row| row.speaker);
    assert_eq!(
        said,
        Some(obelus::component::chat::Speaker::Reader),
        "the words the agent had of the reader are not on the page:\n{}",
        support::render(&mut app, WIDTH, HEIGHT)
    );
}

/// An agent that echoes the prompt back does not put it on the page twice.
///
/// obelus writes the reader's words when they press send, and some agents
/// send the same words back over the session. What makes dropping the
/// repeat safe is that the only rows obelus writes in that voice are the
/// ones it was handed by the reader: a repeat of what is already there is
/// the agent's copy of it, not a second thing they said.
///
/// Broken deliberately by taking the `echoed` check out of `Chat::heard`:
/// the prompt is on the page twice and this goes red.
#[test]
fn an_agent_that_echoes_the_prompt_does_not_say_it_twice() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/echo");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the answer", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("heard you"))
        })
    });

    // Counted in the words, not in the rows: an echo merges into the row
    // that is already there, so twice over is one row saying it twice.
    let said: String = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .filter(|row| row.speaker == obelus::component::chat::Speaker::Reader)
        .map(|row| row.text.clone())
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
/// up, which is the only way to reach a reopen: obelus asks for one it had
/// before when the note it is about has a name written down beside it.
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
    let scratch = support::Scratch::new(name);
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        format!("[[todo]]\nid = \"{note}\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n"),
    )
    .expect("the notes");
    let id = obelus::todo::NoteId::read(note).expect("a name");
    obelus::acp::sessions::change(scratch.path(), std::slice::from_ref(&id), |remembered| {
        remembered.put(
            &id,
            "fake",
            obelus::acp::sessions::Kept {
                session: session.to_string(),
                title: None,
            },
        );
    });

    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    let mut arguments = vec!["tests/fixtures/fake-agent.sh".to_string()];
    arguments.extend(how.iter().map(|word| (*word).to_string()));
    app.talk_to("fake", Path::new("sh"), &arguments);
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::TodoOpen);
    support::press_alt(&mut app, 'a');
    (scratch, app, events)
}

/// Whether obelus has said anything about the conversation it asked for.
///
/// `Talking::Ready` is not that: it is true as soon as the agent has
/// spoken at all, which is before it has answered about this conversation.
fn settled(app: &App) -> bool {
    app.chat().is_some_and(|chat| {
        chat.rows(WIDTH)
            .iter()
            .any(|row| row.speaker == obelus::component::chat::Speaker::Note)
    })
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
    // Until obelus has something to say about the old conversation --
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
        "obelus asked to replay a conversation this agent cannot replay:\n{text}"
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

    // The note carries why, and the why is obelus's own reading of the
    // handshake -- not an error the agent sent back. An agent asked
    // anyway refuses in its own words, and those would be here instead.
    let text = screen(&mut app);
    assert!(
        text.contains("Starting again"),
        "nothing says the old conversation was not there:\n{text}"
    );
    assert!(
        text.contains("take a conversation up again"),
        "the reason is not obelus's own:\n{text}"
    );
    assert!(
        !text.contains("cannot replay"),
        "obelus asked for a conversation the agent said it could not give:\n{text}"
    );
}

/// A command the agent asks for is run, and what it said comes back.
///
/// The five `terminal/*` methods want a process, not a screen: started,
/// its output read, its exit status waited for, and a way to stop it.
/// obelus does not ask the reader first -- the agent asks, which is the
/// rule obelus's own tools follow too -- and what it owes instead is that
/// the command is on the page and a key stops it.
///
/// Broken deliberately by declaring `terminal(false)` in the handshake: a
/// well-behaved agent stops asking and this goes red.
#[test]
fn a_command_the_agent_asks_for_is_run() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
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
                .any(|row| row.text.contains("it said"))
        })
    });

    // Its output and its exit status, both as the command really gave
    // them: the agent read them back out of obelus.
    let said = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .map(|row| row.text.clone())
        .collect::<String>();
    assert!(
        said.contains("it said obelus-ran-this and ended 3"),
        "the command's own words and code did not come back: {said:?}"
    );
}

/// A command is on the page while it runs, in the words it was run in.
///
/// The half obelus owes for not asking before it runs one. The agent
/// decides whether to ask; obelus decides that once it runs, the reader
/// sees the command line itself -- not the agent's title for it -- and
/// everything it printed, and that the call's own state says how it ended.
///
/// Broken deliberately by leaving `Chat::running` uncalled: the row
/// carries the agent's title and nothing else, and this goes red.
#[test]
fn a_command_is_on_the_page_in_the_words_it_was_run_in() {
    let (mut app, events) = talking();
    pump(&mut app, &events, "the session", |app| {
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/run");
    support::press(&mut app, KeyCode::Enter);
    // Until the command has *ended*, not until its words are on the page:
    // the command line says them too, so a row carrying them is the row
    // that says what is being run.
    pump(&mut app, &events, "the command to finish", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH).iter().any(|row| {
                row.text.contains("Run the tests") && row.state.as_deref() == Some("failed")
            })
        })
    });

    let dump = support::render(&mut app, WIDTH, HEIGHT);
    let screen = rows(&dump);
    assert!(
        screen
            .iter()
            .any(|row| row.contains("$ sleep 0.3; printf %s obelus-ran-this; exit 3")),
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
    // And the call says it failed, because the command did.
    let state = app
        .chat()
        .expect("the conversation")
        .rows(WIDTH)
        .iter()
        .find(|row| row.text.contains("Run the tests"))
        .and_then(|row| row.state.clone());
    assert_eq!(
        state,
        Some("failed".to_string()),
        "a call whose command exited 3 does not say it failed"
    );
}

/// The key that stops the agent stops what obelus is running for it.
///
/// The processes are obelus's -- it started them -- and an agent told to
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
        app.talking() == obelus::app::talking::Talking::Ready
    });
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, &events, "the command to start", |app| {
        app.chat().is_some_and(|chat| {
            chat.rows(WIDTH)
                .iter()
                .any(|row| row.text.contains("sleep 300"))
        })
    });
    assert!(
        app.anything_running(),
        "the command obelus was asked to run is not running"
    );

    support::press(&mut app, KeyCode::Esc);
    assert!(
        !app.anything_running(),
        "a command the reader asked to stop is still running"
    );
}
