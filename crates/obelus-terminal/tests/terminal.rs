//! A real program on a real pty.

use std::{sync::mpsc, time::Duration};

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
