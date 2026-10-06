//! A real program on a real pty.

use std::{sync::mpsc, time::Duration};

// For the tests of a shell, which are unix's alone.
#[cfg(unix)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_terminal::{Heard, Program, Terminal};

/// Feeds a terminal what its workers say until it has ended, and gives back
/// how.
fn until_it_ends(terminal: &mut Terminal, heard: &mpsc::Receiver<Heard>) -> obelus_terminal::Ended {
    loop {
        match heard
            .recv_timeout(Duration::from_secs(20))
            .expect("the program to say something")
        {
            Heard::Wrote { bytes, .. } => terminal.wrote(&bytes),
            Heard::Ended { ended, .. } => {
                // What it wrote just before it went may still be on its way:
                // the end is the process's news, and the bytes are the pty's.
                while let Ok(Heard::Wrote { bytes, .. }) =
                    heard.recv_timeout(Duration::from_millis(300))
                {
                    terminal.wrote(&bytes);
                }
                terminal.end(ended.clone());
                return ended;
            }
        }
    }
}

fn program(script: &str) -> Program {
    match cfg!(windows) {
        true => Program::Command {
            program: "cmd.exe".into(),
            arguments: vec!["/C".to_string(), script.to_string()],
            env: Vec::new(),
        },
        false => Program::Command {
            program: "/bin/sh".into(),
            arguments: vec!["-c".to_string(), script.to_string()],
            env: vec![("OBELUS_SAYS".to_string(), "hello".to_string())],
        },
    }
}

/// What the program draws is on the screen, and how it ended is said.
///
/// Broken deliberately by dropping the bytes `wrote` is given: the screen
/// stays blank and the first assertion fails.
#[test]
fn what_a_program_draws_is_on_the_screen() {
    let (events, heard) = mpsc::channel::<Heard>();
    let script = match cfg!(windows) {
        true => "echo drawn & exit 3",
        false => "printf 'drawn %s' \"$OBELUS_SAYS\"; exit 3",
    };
    let mut terminal = Terminal::start(1, &program(script), &std::env::temp_dir(), (5, 40), events)
        .expect("a terminal");
    let ended = until_it_ends(&mut terminal, &heard);
    let contents = terminal.screen().contents();
    assert!(contents.contains("drawn"), "the screen says {contents:?}");
    if cfg!(unix) {
        assert!(
            contents.contains("drawn hello"),
            "the environment it was given: {contents:?}"
        );
    }
    assert_eq!(ended.code, 3);
    assert!(!ended.succeeded());
}

/// A key typed is a key the program reads.
///
/// Broken deliberately by making `key` send nothing: the program waits on
/// its `read` for ever, and the wait for it to end runs out.
#[cfg(unix)]
#[test]
fn a_key_typed_is_read_by_the_program() {
    let (events, heard) = mpsc::channel::<Heard>();
    let mut terminal = Terminal::start(
        2,
        &program("read answer; printf 'got %s' \"$answer\""),
        &std::env::temp_dir(),
        (5, 40),
        events,
    )
    .expect("a terminal");
    for c in "yes".chars() {
        assert!(terminal.key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    assert!(terminal.key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    let ended = until_it_ends(&mut terminal, &heard);
    assert!(ended.succeeded(), "{ended:?}");
    assert!(terminal.screen().contents().contains("got yes"));
}

/// A terminal let go of takes its program with it.
///
/// Broken deliberately by emptying `Drop`: the program sleeps on, and no
/// end ever arrives.
#[cfg(unix)]
#[test]
fn letting_go_stops_the_program() {
    let (events, heard) = mpsc::channel::<Heard>();
    let terminal = Terminal::start(
        3,
        &program("sleep 60"),
        &std::env::temp_dir(),
        (5, 40),
        events,
    )
    .expect("a terminal");
    drop(terminal);
    let ended = loop {
        match heard
            .recv_timeout(Duration::from_secs(10))
            .expect("the program to end")
        {
            Heard::Ended { ended, .. } => break ended,
            Heard::Wrote { .. } => {}
        }
    };
    assert!(!ended.succeeded());
}

/// A terminal with a program that writes nothing, for a test that writes
/// to the parser itself: what is on screen is then exactly what it said.
#[cfg(unix)]
fn quiet(rows: u16, columns: u16) -> Terminal {
    let (events, heard) = mpsc::channel::<Heard>();
    // What the program sends is never read, so it cannot land between two
    // things the test wrote.
    std::mem::forget(heard);
    Terminal::start(
        9,
        &program("sleep 60"),
        &std::env::temp_dir(),
        (rows, columns),
        events,
    )
    .expect("a terminal")
}

/// What is held stays with its words as they go up the screen, and can
/// still be copied once they have gone off the top of it.
///
/// Broken deliberately by letting go of what is held whenever the program
/// writes, which is what it did: the second assertion finds nothing held.
#[cfg(unix)]
#[test]
fn what_is_held_goes_up_the_screen_with_its_words() {
    let mut terminal = quiet(5, 20);
    terminal.wrote(b"one\r\ntwo\r\nthree\r\n");
    terminal.hold_from((1, 0));
    terminal.hold_to((1, 2));
    assert_eq!(terminal.held_text().as_deref(), Some("two"));
    for line in 0..20 {
        terminal.wrote(format!("line {line}\r\n").as_bytes());
    }
    assert!(terminal.is_holding(), "the words were let go of");
    assert_eq!(terminal.held_text().as_deref(), Some("two"));
    // Off the top, so not drawn -- and drawn where they are once the view
    // is back up there.
    assert_eq!(terminal.held(), None);
    terminal.scroll_by(20);
    assert_eq!(terminal.held(), Some(((1, 0), (1, 2))));
}

/// What is held goes when the program writes over it: a copy then would
/// be of words the reader never chose.
///
/// Broken deliberately by keeping it whatever was written: still held, and
/// the copy says `TWO`.
#[cfg(unix)]
#[test]
fn what_is_held_goes_when_the_program_writes_over_it() {
    let mut terminal = quiet(5, 20);
    terminal.wrote(b"one\r\ntwo\r\n");
    terminal.hold_from((1, 0));
    terminal.hold_to((1, 2));
    terminal.wrote(b"\x1b[2;1HTWO");
    assert!(!terminal.is_holding(), "held over words that changed");
}

/// The view's place goes up by a row for every row that goes up the
/// screen, after what is kept is full as well as before.
///
/// Broken deliberately by counting what is kept growing, which is the
/// count the parser has: past ten thousand rows it stops growing, and the
/// last write moves nothing.
#[cfg(unix)]
#[test]
fn every_row_up_the_screen_is_counted_once_what_is_kept_is_full() {
    let mut terminal = quiet(5, 20);
    let mut chunk = String::new();
    for line in 0..10_200 {
        chunk.push_str(&format!("{line}\r\n"));
        if chunk.len() > 4000 {
            terminal.wrote(chunk.as_bytes());
            chunk.clear();
        }
    }
    terminal.wrote(chunk.as_bytes());
    let before = terminal.top();
    terminal.wrote(b"a\r\nb\r\nc\r\n");
    assert_eq!(terminal.top() - before, 3);
}

/// Going over to the screen a full-screen program draws on and back, or
/// starting the terminal over, moves no row up the screen -- and does not
/// bring the terminal down, which taking one screen's count from another's
/// did the moment there was a screenful to keep.
///
/// Broken deliberately by counting across the change of screen as if it
/// were one screen: coming back counts every row the shell had kept as
/// having just gone up. (Before the subtraction saturated, going over
/// panicked instead.)
#[cfg(unix)]
#[test]
fn another_screen_or_a_reset_moves_no_row_up() {
    let mut terminal = quiet(5, 20);
    for line in 0..20 {
        terminal.wrote(format!("{line}\r\n").as_bytes());
    }
    let before = terminal.top();
    terminal.wrote(b"\x1b[?1049h");
    terminal.wrote(b"drawn over\r\n");
    terminal.wrote(b"\x1b[?1049l");
    assert_eq!(terminal.top(), before);
    terminal.wrote(b"\x1bc");
    terminal.wrote(b"again\r\n");
}

/// Read back to the oldest row kept, every row up the screen still counts:
/// the oldest are let go, so what is at the top of the view is newer by
/// that many.
///
/// Broken deliberately by putting the view back by where the reader had it
/// rather than by one row: a view at the oldest row has nowhere further to
/// go, and the write moves nothing.
#[cfg(unix)]
#[test]
fn rows_up_the_screen_count_with_the_view_at_the_oldest() {
    let mut terminal = quiet(5, 20);
    let mut chunk = String::new();
    for line in 0..10_200 {
        chunk.push_str(&format!("{line}\r\n"));
        if chunk.len() > 4000 {
            terminal.wrote(chunk.as_bytes());
            chunk.clear();
        }
    }
    terminal.wrote(chunk.as_bytes());
    terminal.scroll_by(isize::MAX);
    let before = terminal.top();
    terminal.wrote(b"a\r\nb\r\nc\r\n");
    assert_eq!(terminal.top() - before, 3);
}

/// What is held is copied whole, however much taller than the screen it is.
///
/// Broken deliberately by reading only the screenful the first row is on:
/// the copy stops at the screen's last row.
#[cfg(unix)]
#[test]
fn what_is_held_taller_than_the_screen_is_copied_whole() {
    let mut terminal = quiet(5, 20);
    for line in 0..30 {
        terminal.wrote(format!("row {line}\r\n").as_bytes());
    }
    // From `row 10`, read back up to, down to `row 22`.
    terminal.scroll_by(isize::MAX);
    terminal.scroll_by(-10);
    terminal.hold_from((0, 0));
    terminal.scroll_by(-12);
    terminal.hold_to((0, 5));
    let copied = terminal.held_text().expect("something held");
    let wanted: Vec<String> = (10..=22).map(|line| format!("row {line}")).collect();
    assert_eq!(copied, wanted.join("\n"));
}
