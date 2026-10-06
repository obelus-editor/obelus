//! A terminal of Obelus's own: the reader's shell, and an agent's sign-in.
//!
//! Real programs on real ptys, driven by keys and read off the screen, so
//! what is asserted is what a reader would see. Unix only where the program
//! is a shell line: the parser and the pty are the same on Windows, and the
//! crate's own tests run there with `cmd`, but a script read from a ConPTY by
//! a POSIX shell is a second thing to be wrong about at once.
//!
//! The agent is `tests/fixtures/signing-in-agent.sh`, which refuses to open
//! a conversation until the reader has signed in, and offers two ways to.

#![cfg(unix)]

mod support;

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{app::App, event::Event};

const WIDTH: u16 = 76;
const HEIGHT: u16 = 24;

/// How long to wait for a program to say something -- see `agent.rs`, whose
/// reasons are these.
fn patience() -> Duration {
    std::env::var("OBELUS_PATIENCE")
        .ok()
        .and_then(|seconds| seconds.trim().parse().ok())
        .map_or(Duration::from_secs(180), Duration::from_secs)
}

/// An application with the loop's channel, and the channel.
fn wired() -> (App, Receiver<Event>) {
    let (sender, events) = channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    (app, events)
}

/// Handles what arrives until `until` holds, drawing between.
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

/// What the terminal being read has on it.
fn on_the_terminal(app: &App) -> String {
    app.terminal()
        .map(|terminal| terminal.screen().contents())
        .unwrap_or_default()
}

/// The screen's text, as drawn.
fn screen(app: &mut App) -> String {
    let dump = support::render(app, WIDTH, HEIGHT);
    support::text_block(&dump).to_string()
}

fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle(Event::Key(KeyEvent::new(code, modifiers)));
}

/// The key that closes a terminal, which `ctrl+w` does not.
fn close_the_terminal(app: &mut App) {
    press(
        app,
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
}

/// Opens the reader's shell and waits for it to be ready to read a line.
///
/// `/bin/sh`, whatever the reader's is: a reader's `zsh` with a prompt of
/// its own -- and a title it sets -- is a test about their prompt.
fn a_shell() -> (App, Receiver<Event>) {
    let (mut app, events) = wired();
    app.shell_for_test(PathBuf::from("/bin/sh"));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TerminalOpen);
    assert!(app.terminal().is_some(), "no terminal opened");
    // Something to wait for that every shell prints: what it was told.
    support::type_text(&mut app, "echo ready-$((40 + 2))");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the shell", |app| {
        on_the_terminal(app).contains("ready-42")
    });
    (app, events)
}

/// What a program draws is on the screen, and what is typed reaches it.
///
/// Broken deliberately by drawing nothing for a terminal in `draw_the_frame`
/// (the screen comes back blank) and by making `terminal_key` drop the key
/// (the shell never prints the answer and the wait runs out).
#[test]
fn a_shell_is_drawn_and_typed_to() {
    let (mut app, _events) = a_shell();
    assert!(
        screen(&mut app).contains("ready-42"),
        "the terminal is not drawn:\n{}",
        screen(&mut app)
    );
}

/// The keys a shell uses are the shell's: escape, `ctrl+c` and `ctrl+w` do
/// to it what they do in any terminal, and none of them closes or copies.
///
/// The pty is put in raw mode first and three bytes read off it whole, so
/// what arrived is what was sent rather than what the line discipline made
/// of it -- in the ordinary mode `ctrl+w` is the tty's own word rubbed out,
/// and never reaches a program at all. Broken deliberately by asking a
/// file's table instead of the terminal's in `App::terminal_key`: escape
/// clears a selection there and never reaches the program.
#[test]
fn escape_and_control_keys_go_to_the_program() {
    let (mut app, events) = a_shell();
    support::type_text(
        &mut app,
        "stty raw -echo; echo go-$((2 + 2)); dd bs=1 count=3 2>/dev/null | od -An -c; stty sane",
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pty to be raw", |app| {
        on_the_terminal(app).contains("go-4")
    });
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    support::type_text(&mut app, "x");
    press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    pump(&mut app, &events, "the bytes to be read back", |app| {
        on_the_terminal(app).contains("033   x 027")
    });
    assert!(app.terminal().is_some(), "ctrl+w closed the terminal");
    // And `ctrl+c` stops what is running, which is the shell's to do: back
    // at the prompt, a line typed is run.
    //
    // Once it is running and not before: a `ctrl+c` the tty reads ahead of
    // the line that started it throws that line away, which is a test of
    // how fast the shell reads.
    support::type_text(&mut app, "echo sleeping-$((3 + 3)); sleep 30");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the sleep to start", |app| {
        on_the_terminal(app).contains("sleeping-6")
    });
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    support::type_text(&mut app, "echo after-$((1 + 1))");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the shell after ctrl+c", |app| {
        on_the_terminal(app).contains("after-2")
    });
}

/// The key that leaves Obelus leaves it from inside a terminal too, though
/// every other control letter there is the program's -- asking first,
/// because the program would go with it.
///
/// Broken deliberately by taking `quit` out of the terminal's table (the key
/// goes down the pty, nothing is asked and Obelus stays), and by leaving
/// out the question in `request_quit` (Obelus leaves at the first press).
#[test]
fn ctrl_q_leaves_from_inside_a_terminal_and_asks_first() {
    let (mut app, _events) = a_shell();
    press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(!app.should_quit(), "left without asking");
    assert!(
        support::ways(&app)
            .iter()
            .any(|way| way == "Stop it and leave")
    );
    support::answer(&mut app, "Stop it and leave");
    assert!(app.should_quit(), "the answer did not leave");
}

/// Where the terminal's cells are on screen: the whole document region.
fn terminal_area(app: &mut App) -> ratatui::layout::Rect {
    let cells = support::cells_of(app, WIDTH, HEIGHT);
    obelus_ui::editor_canvas(cells.area)
}

/// Where these words are on screen, as the pointer would find them.
fn place_of_words(app: &mut App, words: &str) -> (u16, u16) {
    let area = terminal_area(app);
    let rows: Vec<String> = on_the_terminal(app).lines().map(str::to_string).collect();
    let (row, line) = rows
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.contains(words) && !line.contains("echo"))
        .unwrap_or_else(|| panic!("no {words:?} on the terminal"));
    let column = line[..line.find(words).expect("the words")].chars().count();
    (
        area.x + u16::try_from(column).expect("a column"),
        area.y + u16::try_from(row).expect("a row"),
    )
}

fn pointer(app: &mut App, kind: obelus_app::event::Pointer, (x, y): (u16, u16)) {
    app.handle(Event::Pointer { kind, x, y });
}

/// A drag takes hold of what a shell printed, it is drawn held, and the
/// key that copies copies it -- `ctrl+c` included, which with nothing held
/// is the shell's interrupt.
///
/// Broken deliberately by dropping the copy in `App::terminal_key` (the
/// `ctrl+c` goes to the shell and the clipboard is empty), and by not
/// drawing what is held (the cell under it has the page's ground).
#[test]
fn a_drag_takes_hold_and_ctrl_c_copies_it() {
    use obelus_app::event::Pointer;

    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (mut app, events) = a_shell();
    support::type_text(&mut app, "echo take-$((6 * 7))-these");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the words", |app| {
        on_the_terminal(app).contains("take-42-these")
    });
    let (x, y) = place_of_words(&mut app, "take-42-these");
    pointer(&mut app, Pointer::Pressed, (x, y));
    pointer(&mut app, Pointer::Dragged, (x + 6, y));
    pointer(&mut app, Pointer::Released, (x + 6, y));
    let cells = support::cells_of(&mut app, WIDTH, HEIGHT);
    assert_eq!(cells[(x + 3, y)].bg, app.theme().selection_background);
    press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(obelus_clipboard::paste().as_deref(), Some("take-42"));
    // And let go of, so the next `ctrl+c` is the shell's again.
    assert!(
        app.terminal()
            .and_then(|terminal| terminal.held())
            .is_none()
    );
}

/// `ctrl+v` pastes into the program, the way it does everywhere else in
/// Obelus -- and the way a desktop's own paste reaches a window.
///
/// Broken deliberately by taking `ctrl+v` out of the terminal's table: the
/// shell is sent a `^V` and the line never runs.
#[test]
fn ctrl_v_pastes_into_the_program() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (mut app, events) = a_shell();
    obelus_clipboard::copy("echo pasted-$((5 * 5))").expect("a copy");
    press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pasted line to run", |app| {
        on_the_terminal(app).contains("pasted-25")
    });
}

/// A program that asked for the pointer is told what it did, in the
/// encoding it asked for; shift held is a selection all the same.
///
/// The pty is raw and the bytes read back whole, as in the test of the
/// keys. Broken deliberately by never handing the pointer to the program
/// (`wants_the_pointer` answering no: the press takes hold instead, and the
/// bytes never come), and by ignoring shift (the shifted press goes to the
/// program and nothing is held).
#[test]
fn a_program_that_asked_for_the_pointer_is_told() {
    use obelus_app::event::Pointer;

    let (mut app, events) = a_shell();
    support::type_text(
        &mut app,
        "printf '\\033[?1000h\\033[?1006h'; stty raw -echo; echo go-$((3 + 4)); \
         dd bs=1 count=9 2>/dev/null | od -An -c; stty sane",
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pty to be raw", |app| {
        on_the_terminal(app).contains("go-7")
    });
    assert!(
        app.terminal()
            .is_some_and(|terminal| terminal.wants_the_pointer())
    );
    // Shift first, which is Obelus's: nothing goes to the program.
    let area = terminal_area(&mut app);
    app.handle(Event::Shifted(true));
    pointer(&mut app, Pointer::Pressed, (area.x + 1, area.y + 1));
    pointer(&mut app, Pointer::Dragged, (area.x + 4, area.y + 1));
    assert!(
        app.terminal()
            .and_then(|terminal| terminal.held())
            .is_some(),
        "shift and a drag did not take hold"
    );
    app.handle(Event::Shifted(false));
    // Then a click, column 3 and row 2 counted from one.
    pointer(&mut app, Pointer::Pressed, (area.x + 2, area.y + 1));
    pump(&mut app, &events, "the press to be read back", |app| {
        on_the_terminal(app).contains("033   [   <   0   ;   3   ;   2   M")
    });
}

/// The wheel in a program that took over the screen and did not ask for the
/// pointer is arrow keys, which is what every terminal sends a pager.
///
/// Broken deliberately by scrolling back regardless (`Terminal::wheel`
/// ignoring the screen it is on): nothing reaches the program.
#[test]
fn the_wheel_over_a_pager_is_arrow_keys() {
    let (mut app, events) = a_shell();
    support::type_text(
        &mut app,
        "printf '\\033[?1049h'; stty raw -echo; echo go-$((4 + 4)); \
         dd bs=1 count=3 2>/dev/null | od -An -c; stty sane",
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pty to be raw", |app| {
        on_the_terminal(app).contains("go-8")
    });
    app.handle(Event::Scroll(1));
    pump(&mut app, &events, "the arrow to be read back", |app| {
        on_the_terminal(app).contains("033   [   B")
    });
}

/// A function key still opens what it opens, from inside a terminal.
///
/// Broken deliberately by dropping the function keys' fall-back in
/// `Keymap::lookup`: `f1` goes down the pty and no list opens.
#[test]
fn a_function_key_opens_what_it_opens() {
    let (mut app, _events) = a_shell();
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    let rows: Vec<String> = app
        .picker()
        .expect("the list of what is open")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    // The terminal is one of them, called by its program.
    assert!(
        rows.iter().any(|row| row == "/bin/sh"),
        "the terminal is not in the list: {rows:?}"
    );
}

/// What a program prints in red is the theme's red: the sixteen colours a
/// program names by number are the page's, in `ob` and `obg` alike, rather
/// than whatever palette the front end happens to have.
///
/// Broken deliberately by passing the number through (`colour_of` answering
/// `Color::Indexed`): the cell's ink is a palette slot, not the theme's.
#[test]
fn a_colour_named_by_number_is_the_themes() {
    let (mut app, events) = a_shell();
    support::type_text(&mut app, "printf '\\033[31mred-%s\\033[0m\\n' $((2 + 3))");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the red words", |app| {
        on_the_terminal(app).contains("red-5")
    });
    let cells = support::cells_of(&mut app, WIDTH, HEIGHT);
    let area = cells.area;
    let ink = (area.y..area.bottom())
        .find_map(|y| {
            let row: String = (area.x..area.right())
                .map(|x| cells[(x, y)].symbol().to_string())
                .collect();
            // The row the words were printed on, not the line that printed
            // them, which says `red-%s`.
            let at = row.find("red-5")?;
            let column = u16::try_from(row[..at].chars().count()).ok()?;
            Some(cells[(area.x + column, y)].fg)
        })
        .expect("the words on screen");
    assert_eq!(ink, app.theme().syntax.error);
}

/// Closing a terminal whose program is running asks first, on the key every
/// terminal closes a tab with -- and closing it stops the program.
///
/// Broken deliberately by emptying `ask_before_stopping`: the terminal goes
/// at the first press, with nothing asked. And by taking the terminal's
/// context away (`App::context` returning `Normal`): the key is handed back
/// to a table that does not have it, and nothing is asked or closed.
#[test]
fn closing_a_running_terminal_asks_first() {
    let (mut app, _events) = a_shell();
    close_the_terminal(&mut app);
    assert!(app.terminal().is_some(), "closed without asking");
    support::answer(&mut app, "Stop it and close");
    assert!(app.terminal().is_none(), "still open after the answer");
}

/// A program that has ended leaves its terminal to be read, and the keys go
/// back to Obelus: `ctrl+w` closes it like any document.
///
/// Broken deliberately by keeping the terminal's context once the program
/// has ended (`typing_to_a_program` ignoring the end): `ctrl+w` goes nowhere
/// and the terminal stays.
#[test]
fn a_program_that_has_ended_is_read_like_a_file() {
    let (mut app, events) = a_shell();
    support::type_text(&mut app, "exit 3");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the shell to exit", |app| {
        app.terminal()
            .is_some_and(|terminal| terminal.ended().is_some())
    });
    assert!(
        screen(&mut app).contains("Exited 3"),
        "the status row does not say how it ended:\n{}",
        screen(&mut app)
    );
    press(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(app.terminal().is_none(), "ctrl+w did not close it");
}

/// Where the signing-in agent writes down that it has been signed in.
fn marker(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("obelus-signing-in-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("log"));
    path
}

/// What the agent was asked, in order.
fn asked_of(marker: &Path) -> Vec<String> {
    std::fs::read_to_string(format!("{}.log", marker.display()))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A conversation with the signing-in agent, waiting on the reader.
fn asked_to_sign_in(marker: &Path) -> (App, Receiver<Event>) {
    let (mut app, events) = wired();
    app.talk_to(
        "signing-in",
        Path::new("sh"),
        &[
            "tests/fixtures/signing-in-agent.sh".to_string(),
            marker.display().to_string(),
        ],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the sign-in card", |app| {
        app.card().is_some_and(|card| {
            card.choices()
                .iter()
                .any(|choice| choice.name == "Type a code")
        })
    });
    (app, events)
}

/// Chooses the row of the card called `name`, by the keys.
fn choose(app: &mut App, name: &str) {
    let at = app
        .card()
        .expect("a card")
        .choices()
        .iter()
        .position(|choice| choice.name == name)
        .unwrap_or_else(|| panic!("no {name:?} on the card"));
    for _ in 0..at {
        press(app, KeyCode::Down, KeyModifiers::NONE);
    }
    press(app, KeyCode::Enter, KeyModifiers::NONE);
}

/// Whether the conversation's transcript says this.
fn said_in_transcript(app: &App, words: &str) -> bool {
    app.chat().is_some_and(|chat| {
        chat.rows(WIDTH)
            .iter()
            .any(|row| row.text().contains(words))
    })
}

/// An agent that wants a sign-in is offered the ways it has, and a way that
/// is a program is run in a terminal the reader answers it in. Ending well
/// takes them back to the conversation, which opens -- on the same agent,
/// asked again, rather than one started over.
///
/// What the agent says it needs a sign-in for is on the card, in its words.
/// Broken deliberately in three places: declaring no `auth.terminal` in the
/// handshake (the agent offers nothing, and there is no card to wait for);
/// ending the connection on `auth_required` the way it did before (the card
/// never comes and the conversation ends); and not asking again after the
/// sign-in (`Ask::SignedIn` doing nothing: the session never opens).
#[test]
fn a_sign_in_that_is_a_program_is_answered_in_a_terminal() {
    let marker = marker("terminal");
    let (mut app, events) = asked_to_sign_in(&marker);
    assert!(
        app.card()
            .and_then(|card| card.what_about())
            .is_some_and(|about| about.contains("Sign in to the fake first")),
        "the card does not say what the agent said"
    );
    choose(&mut app, "Type a code");
    pump(&mut app, &events, "the sign-in to ask for a code", |app| {
        on_the_terminal(app).contains("Code:")
    });
    support::type_text(&mut app, "right");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the conversation to open", |app| {
        app.chat().is_some() && app.talking() == obelus_agent::Talking::Ready
    });
    assert!(said_in_transcript(&app, "Signed in"));
    assert!(app.terminal().is_none(), "the sign-in's terminal stayed");
    // One process the whole way: asked once at the handshake, refused, and
    // asked again once signed in.
    let asked = asked_of(&marker);
    assert_eq!(
        asked
            .iter()
            .filter(|method| *method == "initialize")
            .count(),
        1,
        "the agent was started again: {asked:?}"
    );
    assert_eq!(
        asked
            .iter()
            .filter(|method| *method == "session/new")
            .count(),
        2,
        "{asked:?}"
    );
    // And it talks.
    support::type_text(&mut app, "hello");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the answer", |app| {
        said_in_transcript(app, "glad you are in")
    });
}

/// A sign-in that fails stays where the reader can read why, and the
/// question goes back up in the conversation.
///
/// Broken deliberately by closing the terminal whatever the exit status:
/// the reader is taken back with nothing to say why it did not work.
#[test]
fn a_sign_in_that_fails_stays_to_be_read() {
    let marker = marker("failing");
    let (mut app, events) = asked_to_sign_in(&marker);
    choose(&mut app, "Type a code");
    pump(&mut app, &events, "the sign-in to ask for a code", |app| {
        on_the_terminal(app).contains("Code:")
    });
    support::type_text(&mut app, "wrong");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the sign-in to end", |app| {
        app.terminal()
            .is_some_and(|terminal| terminal.ended().is_some())
    });
    assert!(on_the_terminal(&app).contains("Not that code"));
    // The conversation has the card again, and the way back to it is the
    // way back from anywhere a key took the reader.
    press(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert!(app.chat().is_some(), "not in the conversation");
    assert!(said_in_transcript(&app, "Signing in did not finish"));
    assert!(app.card().is_some(), "the question did not go back up");
}

/// A way in that is the agent's own is asked of the agent.
///
/// Broken deliberately by sending nothing for `How::Asked`: the agent is
/// never asked to `authenticate`, and the conversation never opens.
#[test]
fn a_sign_in_the_agent_does_itself_is_asked_of_it() {
    let marker = marker("agent");
    let (mut app, events) = asked_to_sign_in(&marker);
    choose(&mut app, "Let it sign in");
    pump(&mut app, &events, "the conversation to open", |app| {
        app.talking() == obelus_agent::Talking::Ready && said_in_transcript(app, "Signed in")
    });
    assert!(
        asked_of(&marker)
            .iter()
            .any(|method| method == "authenticate")
    );
}

/// Not signing in ends the conversation the way it ended before anybody
/// could sign in, rather than leaving it opening for ever.
///
/// Broken deliberately by making `Ask::GiveUp` do nothing: the agent is
/// still waiting, and nothing says the conversation is over.
#[test]
fn not_signing_in_ends_the_conversation() {
    let marker = marker("declined");
    let (mut app, events) = asked_to_sign_in(&marker);
    choose(&mut app, "Not now");
    pump(&mut app, &events, "the conversation to end", |app| {
        app.talking() == obelus_agent::Talking::Gone
    });
    assert!(said_in_transcript(&app, "Not signed in"));
}
