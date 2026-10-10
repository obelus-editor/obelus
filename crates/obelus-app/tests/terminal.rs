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

/// The key that closes a terminal, which is the key that closes anything.
fn close_the_terminal(app: &mut App) {
    press(app, KeyCode::Char('w'), KeyModifiers::CONTROL);
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

/// The reader's shell is started knowing where the machine's pool of build
/// jobs is: a build typed in a terminal inside Obelus is one Obelus started.
///
/// Broken deliberately by leaving the pool out of `command_for` in
/// `obelus-terminal`: the shell prints `pool-42:` and nothing after it.
#[test]
fn a_shell_is_told_where_the_pool_of_build_jobs_is() {
    let (mut app, events) = wired();
    app.configure(obelus_config::Config::default(), Vec::new());
    app.shell_for_test(PathBuf::from("/bin/sh"));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TerminalOpen);
    support::type_text(&mut app, "echo pool-$((40 + 2)):$CARGO_MAKEFLAGS:end");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    // The answer by what only the answer has in it, the sum, and not by
    // where its line starts: typed before the shell is up, the line is
    // echoed before the prompt is drawn, and the prompt lands at the start
    // of the answer instead -- which is a line that never starts with the
    // word, and a wait that ran out on one machine in CI.
    pump(&mut app, &events, "the shell's answer", |app| {
        on_the_terminal(app)
            .lines()
            .any(|line| line.contains("pool-42:") && line.trim_end().ends_with(":end"))
    });
    // This binary's own pool, by its process's number: run from inside an
    // Obelus, the shell would inherit the outer one's whether or not it was
    // told anything.
    let this_pool = format!("obelus-jobs-{}-", std::process::id());
    let said = on_the_terminal(&app);
    assert!(
        said.lines()
            .any(|line| line.contains("pool-42:") && line.contains(&this_pool)),
        "the shell was not told where this pool is:\n{said}"
    );
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

/// The keys a shell uses are the shell's: escape, `ctrl+c` and the control
/// letters Obelus has not kept do to it what they do in any terminal, and
/// none of them copies.
///
/// The pty is put in raw mode first and three bytes read off it whole, so
/// what arrived is what was sent rather than what the line discipline made
/// of it. Broken deliberately by asking a file's table instead of the
/// terminal's in `App::terminal_key`: escape clears a selection there and
/// never reaches the program.
#[test]
fn escape_and_control_keys_go_to_the_program() {
    let (mut app, events) = a_shell();
    support::type_text(
        &mut app,
        "stty raw -echo; echo go-$((2 + 2)); dd bs=1 count=3 2>/dev/null | od -An -c; stty sane; \
         echo sane-$((4 + 4))",
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pty to be raw", |app| {
        on_the_terminal(app).contains("go-4")
    });
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    support::type_text(&mut app, "x");
    press(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL);
    pump(&mut app, &events, "the bytes to be read back", |app| {
        on_the_terminal(app).contains("033   x 005")
    });
    // And the pty cooked again before anything else is typed. A line typed
    // while it is still raw ends in a `\r` nothing turns into a newline,
    // and once `stty sane` has run that ends no line: dash sits waiting for
    // the rest of it. `od` printing is not `stty` having run, and on a slow
    // runner the gap between them is a line typed into the raw pty.
    pump(&mut app, &events, "the pty to be cooked again", |app| {
        on_the_terminal(app).contains("sane-8")
    });
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

/// Closing the project asks first about a program still running in a
/// terminal, because the program would go with it -- the question leaving
/// asks, with nothing unsaved to ask it alongside.
///
/// Broken deliberately by leaving the count of running terminals out of
/// `App::close_the_project`: the project closes at the first press.
#[test]
fn closing_the_project_asks_about_a_running_terminal_first() {
    let (mut app, _events) = a_shell();
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ProjectClose);
    assert!(app.has_a_project(), "closed without asking");
    assert_eq!(
        support::ways(&app),
        ["Stop it and close the project", "cancel"],
        "the question was not about the terminal"
    );
    support::answer(&mut app, "Stop it and close the project");
    assert!(!app.has_a_project(), "the answer did not close the project");
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
/// `ctrl+c` goes to the shell and the clipboard is empty), by not drawing
/// what is held (the cell under it has the page's ground), and by offering
/// the copy with nothing held, as a file does.
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
    // Nothing held, nothing to copy: the palette says so by the row's ink.
    assert!(!app.offers(obelus_command::Command::SelectionCopy));
    let (x, y) = place_of_words(&mut app, "take-42-these");
    pointer(&mut app, Pointer::Pressed, (x, y));
    pointer(&mut app, Pointer::Dragged, (x + 6, y));
    pointer(&mut app, Pointer::Released, (x + 6, y));
    let cells = support::cells_of(&mut app, WIDTH, HEIGHT);
    assert_eq!(cells[(x + 3, y)].bg, app.theme().selection_background);
    assert!(app.offers(obelus_command::Command::SelectionCopy));
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

/// The paging keys read back up what a shell printed, a screenful at a time
/// and to either end -- and are Obelus's over a full-screen program too.
///
/// Broken deliberately by sending them down the pty (`read_back` answering
/// no): the view never moves, and the pager is sent its key.
#[test]
fn the_paging_keys_read_back_and_are_never_the_programs() {
    let (mut app, events) = a_shell();
    support::type_text(&mut app, "seq 1 200; echo done-$((5 + 5))");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the numbers", |app| {
        on_the_terminal(app).contains("done-10")
    });
    let back = |app: &App| app.terminal().map_or(0, |terminal| terminal.scrolled());
    let rows = app
        .terminal()
        .map_or(0, |terminal| usize::from(terminal.size().0));
    press(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(back(&app), rows - 1);
    press(&mut app, KeyCode::PageDown, KeyModifiers::NONE);
    assert_eq!(back(&app), 0);
    press(&mut app, KeyCode::Home, KeyModifiers::CONTROL);
    assert!(back(&app) > 150, "not at the top: {}", back(&app));
    press(&mut app, KeyCode::End, KeyModifiers::CONTROL);
    assert_eq!(back(&app), 0);
    // And over a program that has the other screen, still not its: the
    // first byte it reads is the one typed after the paging key.
    support::type_text(
        &mut app,
        "printf '\\033[?1049h'; stty raw -echo; echo go-$((6 + 6)); \
         dd bs=1 count=1 2>/dev/null | od -An -c; stty sane",
    );
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the pty to be raw", |app| {
        on_the_terminal(app).contains("go-12")
    });
    press(&mut app, KeyCode::PageUp, KeyModifiers::NONE);
    support::type_text(&mut app, "z");
    pump(&mut app, &events, "the byte to be read back", |app| {
        let said = on_the_terminal(app);
        said.contains("   z") || said.contains("033")
    });
    assert!(
        on_the_terminal(&app).contains("   z"),
        "the pager was sent the paging key:\n{}",
        on_the_terminal(&app)
    );
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
/// `Keymap::lookup`: `f2` goes down the pty and no list opens.
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

/// Closing a terminal whose program is running asks first, on the key that
/// closes anything in Obelus -- and closing it stops the program.
///
/// Broken deliberately by emptying `ask_before_stopping`: the terminal goes
/// at the first press, with nothing asked. And by putting the command first
/// in the question again, which the rule about names forbids. And by taking
/// `ctrl+w` out of the terminal's table: the key goes to the shell, and
/// nothing is asked.
#[test]
fn closing_a_running_terminal_asks_first() {
    let (mut app, _events) = a_shell();
    close_the_terminal(&mut app);
    assert!(app.terminal().is_some(), "closed without asking");
    // Saying which, after the words rather than in front of them: a
    // command line is a name, and a sentence does not start with one.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        support::said(&dump).contains("Still running: /bin/sh"),
        "{dump}"
    );
    support::answer(&mut app, "Stop it and close");
    assert!(app.terminal().is_none(), "still open after the answer");
}

/// A program that has ended leaves its terminal to be read, and the keys go
/// back to Obelus: `ctrl+w` closes it like any document.
///
/// Broken deliberately by keeping the terminal's context once the program
/// has ended (`typing_to_a_program` ignoring the end): `ctrl+w` goes nowhere
/// and the terminal stays. And by spelling the row's end its own way again:
/// the list says something the status row does not.
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
    // And its row in the list of what is open says it in the same words.
    press(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    let trailing: Vec<Option<String>> = app
        .picker()
        .expect("the list of what is open")
        .matches()
        .map(|item| item.trailing.clone())
        .collect();
    assert!(
        trailing
            .iter()
            .any(|said| said.as_deref() == Some("Exited 3")),
        "{trailing:?}"
    );
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
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
        Path::new(support::sh()),
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

/// The program that signs an agent in is given what the reader added to
/// that agent's environment, the same as the agent.
///
/// Obelus runs it, not the agent, so nothing reaches it that Obelus does
/// not pass on -- a proxy the agent goes through and its sign-in does not
/// is a sign-in that cannot reach the server.
///
/// And where the two name the same variable, the agent's is the one it
/// gets: that is what this particular sign-in was said to need.
///
/// Broken deliberately by handing the terminal only what the agent asked
/// for: the sign-in started without the word. And by putting the agent's
/// before the reader's: the sign-in got the reader's word in place of the
/// agent's.
#[test]
fn a_sign_in_is_given_what_the_reader_added() {
    let marker = marker("environment");
    let settings = marker.with_extension("toml");
    std::fs::write(
        &settings,
        "[environment.signing-in]\nOBELUS_FAKE_WORD = \"obelus-heard\"\n\
         OBELUS_FAKE_AGENTS_WORD = \"the-readers\"\n",
    )
    .expect("the settings");
    let (mut app, events) = wired();
    app.config_file_for_test(settings);
    app.talk_to(
        "signing-in",
        Path::new(support::sh()),
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
    choose(&mut app, "Type a code");
    pump(&mut app, &events, "the sign-in to ask for a code", |app| {
        on_the_terminal(app).contains("Code:")
    });
    assert!(
        on_the_terminal(&app).contains("Started with obelus-heard"),
        "the sign-in was not given the reader's variable:\n{}",
        on_the_terminal(&app)
    );
    assert!(
        on_the_terminal(&app).contains("Agent's word the-agents"),
        "the reader's variable took the place of the agent's own:\n{}",
        on_the_terminal(&app)
    );
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
    // Both, and not the end and then the words: what a program wrote and
    // that it ended come from two threads, so the end can arrive with its
    // last line still on the way -- one run in eight, here and on CI.
    pump(&mut app, &events, "the sign-in to end, and why", |app| {
        app.terminal()
            .is_some_and(|terminal| terminal.ended().is_some())
            && on_the_terminal(app).contains("Not that code")
    });
    // The conversation has the card again, and the way back to it is the
    // way back from anywhere a key took the reader.
    press(&mut app, KeyCode::Left, KeyModifiers::ALT);
    assert!(app.chat().is_some(), "not in the conversation");
    assert!(said_in_transcript(&app, "Signing in did not finish"));
    assert!(app.card().is_some(), "the question did not go back up");
}

/// A conversation taken up again that the agent cannot take up, and whose
/// fresh one wants a sign-in, asks for the sign-in rather than ending.
///
/// The signing-in agent says nothing about taking conversations up, so
/// asking for one written down beside a note goes straight to a new one --
/// which it refuses until the reader is in. Broken deliberately by putting
/// the `?` back on that `open_session`: the connection ends instead, and
/// no card ever comes.
#[test]
fn a_conversation_taken_up_again_asks_for_the_sign_in_too() {
    let marker = marker("again");
    let scratch = support::Scratch::new("terminal-sign-in-again");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456Q\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let id = obelus_todo::NoteId::read("0123456Q").expect("a name");
    obelus_agent::acp::sessions::change(
        scratch.path(),
        0,
        Some(std::slice::from_ref(&id)),
        |kept| {
            kept.put(
                &obelus_agent::chats::ChatId::Note(id.clone()),
                "signing-in",
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
    app.talk_to(
        "signing-in",
        Path::new(support::sh()),
        &[
            "tests/fixtures/signing-in-agent.sh".to_string(),
            marker.display().to_string(),
        ],
    );
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    press(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
    assert!(app.chat().is_some(), "the note's conversation did not open");
    app.open_a_session_for_test();
    pump(&mut app, &events, "the sign-in card", |app| {
        app.talking() == obelus_agent::Talking::Gone
            || app.card().is_some_and(|card| {
                card.choices()
                    .iter()
                    .any(|choice| choice.name == "Type a code")
            })
    });
    assert_ne!(
        app.talking(),
        obelus_agent::Talking::Gone,
        "the connection ended"
    );
}

/// Two conversations waiting on one sign-in, the reader in the second.
///
/// The second -- about a note, so it is not the first one again -- asks
/// for its session after the agent has refused the first, so it is held
/// behind it without the agent being asked.
fn two_waiting(name: &str) -> (App, Receiver<Event>) {
    let marker = marker(name);
    let scratch = support::Scratch::new(&format!("terminal-sign-in-{name}"));
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456R\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    let (mut app, events) = wired();
    app.working_directory_for_test(scratch.path().to_path_buf());
    // By its whole path: a sign-in runs in the project, and this project is
    // not the directory the tests run in.
    let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signing-in-agent.sh");
    app.talk_to(
        "signing-in",
        Path::new(support::sh()),
        &[agent.display().to_string(), marker.display().to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    pump(&mut app, &events, "the first conversation's card", asking);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    press(&mut app, KeyCode::Char('a'), KeyModifiers::ALT);
    assert!(app.chat().is_some(), "the note's conversation did not open");
    app.open_a_session_for_test();
    pump(&mut app, &events, "the second conversation's card", asking);
    (app, events)
}

/// Whether the conversation on screen is asking for a sign-in.
fn asking(app: &App) -> bool {
    app.card().is_some_and(|card| {
        card.choices()
            .iter()
            .any(|choice| choice.name == "Type a code")
    })
}

/// Goes back past the notes to the first conversation, and says whether it
/// has let go of its question.
fn the_first_has_let_go(app: &mut App) {
    press(app, KeyCode::Left, KeyModifiers::ALT);
    press(app, KeyCode::Left, KeyModifiers::ALT);
    assert!(app.chat().is_some(), "not back in the first conversation");
    assert!(
        app.card().is_none(),
        "the first conversation is still asking"
    );
    assert!(said_in_transcript(app, "Signed in"));
}

/// Every conversation waiting on a sign-in says so, and signing in once
/// lets all of them go on -- here by the agent's own way in.
///
/// Broken deliberately by saying nothing for a request held behind another
/// (the second conversation never shows a card), and by leaving the first's
/// card up when the agent says the reader is in (it is still asking
/// afterwards).
#[test]
fn every_conversation_waiting_on_a_sign_in_is_asked_and_let_go() {
    let (mut app, events) = two_waiting("two");
    choose(&mut app, "Let it sign in");
    pump(
        &mut app,
        &events,
        "the second conversation to open",
        |app| app.talking() == obelus_agent::Talking::Ready && app.card().is_none(),
    );
    the_first_has_let_go(&mut app);
}

/// And by a way in that is a program, whose ending well is the sign-in.
///
/// Broken deliberately by leaving the first's card up when the sign-in's
/// terminal ends well.
#[test]
fn a_sign_in_in_a_terminal_lets_every_conversation_go_on() {
    let (mut app, events) = two_waiting("two-terminal");
    choose(&mut app, "Type a code");
    pump(&mut app, &events, "the sign-in to ask for a code", |app| {
        on_the_terminal(app).contains("Code:")
    });
    support::type_text(&mut app, "right");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(
        &mut app,
        &events,
        "the second conversation to open",
        |app| app.chat().is_some() && app.talking() == obelus_agent::Talking::Ready,
    );
    the_first_has_let_go(&mut app);
}

/// A turn that finds the reader no longer signed in ends, and asks.
///
/// Broken deliberately by not saying `SignIn` after a turn's
/// `auth_required`: the turn ends with the agent's words and nothing asks.
#[test]
fn a_turn_that_needs_a_sign_in_asks_for_one() {
    let marker = marker("turn");
    let (mut app, events) = asked_to_sign_in(&marker);
    choose(&mut app, "Let it sign in");
    pump(&mut app, &events, "the conversation to open", |app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/refuse now");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the card", |app| {
        app.card().is_some_and(|card| {
            card.choices()
                .iter()
                .any(|choice| choice.name == "Type a code")
        })
    });
}

/// Leaving with a file unwritten and a program running says both in the
/// one question, because either answer stops the program.
///
/// Broken deliberately by asking about the file alone, as it did: the
/// question says nothing of the terminal.
#[test]
fn leaving_says_what_is_running_beside_what_is_unwritten() {
    let scratch = support::Scratch::new("terminal-leaving");
    let file = scratch.join("unwritten.txt");
    std::fs::write(&file, "words\n").expect("a file");
    let (sender, events) = channel();
    let mut app = App::new(vec![obelus_buffer::Buffer::open(&file).expect("the file")]);
    app.events_for_test(sender);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::type_text(&mut app, "more ");
    app.shell_for_test(PathBuf::from("/bin/sh"));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TerminalOpen);
    support::type_text(&mut app, "echo up-$((1 + 8))");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    pump(&mut app, &events, "the shell", |app| {
        on_the_terminal(app).contains("up-9")
    });
    press(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert!(
        support::said(&dump).contains("and a terminal is still running"),
        "{dump}"
    );
}

/// A way in that will not even start puts the question back, with why: what
/// it was asking for is still waiting, and nothing else would ask again.
///
/// Broken deliberately by not putting the question back where the terminal
/// would not start: the card is gone, and the conversation waits for ever.
#[test]
fn a_sign_in_that_will_not_start_asks_again() {
    let marker = marker("missing");
    let (mut app, _events) = asked_to_sign_in(&marker);
    choose(&mut app, "Run what is not there");
    assert!(app.terminal().is_none(), "a terminal opened for nothing");
    assert!(said_in_transcript(&app, "The sign-in would not start"));
    assert!(
        app.card().is_some_and(|card| card
            .choices()
            .iter()
            .any(|choice| choice.name == "Type a code")),
        "the question did not go back up"
    );
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
