//! What `obg` says it is to Windows, and what that costs a shell.

/// `obg` is a window and not a console program, so that starting it from a
/// menu does not put up a console beside it.
///
/// Read out of the executable itself, where Windows reads it: the PE header's
/// `Subsystem`, which is 2 for a window and 3 for a console.
///
/// Broken deliberately by taking `windows_subsystem` out of `main.rs`: the
/// field is 3.
#[cfg(windows)]
#[test]
fn obg_is_a_window_and_not_a_console_program() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_obg")).expect("the executable");
    let at = |offset: usize, width: usize| {
        bytes[offset..offset + width]
            .iter()
            .rev()
            .fold(0usize, |value, byte| value << 8 | usize::from(*byte))
    };
    // The DOS header says where the PE header is; after its signature come
    // the 20 bytes of the file header, and the subsystem is 68 bytes into
    // the optional header after that.
    let pe = at(0x3c, 4);
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "not a PE file");
    assert_eq!(
        at(pe + 4 + 20 + 68, 2),
        2,
        "obg says it is a console program"
    );
}

/// `obg` typed into a console starts itself again, away from it: nushell
/// and Git Bash wait for a window program as they wait for any other, and
/// would otherwise have no prompt until the window closed. A start that is
/// refused is still said on the console, and the shell hears that it
/// failed.
///
/// Refused rather than started, because a window opened by a test is one
/// nobody closes; the log says which process read the paths, because every
/// line says whose it is.
///
/// Broken deliberately by taking `leave_the_terminal` out of `main`: the
/// start is the console's own process. And by taking the standard error
/// out of what the second `obg` inherits: nothing is said.
#[cfg(windows)]
#[test]
fn obg_typed_into_a_console_starts_again_away_from_it() {
    use std::{
        io::Read as _,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    let scratch = std::env::temp_dir().join(format!(
        "obelus-gui-subsystem-leaving-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch directory");
    // Not UTF-8, which is a file Obelus will not read.
    let file = scratch.join("unreadable.txt");
    std::fs::write(&file, [0xff, 0xfe, 0xfd]).expect("the file");
    let mut started = Command::new(env!("CARGO_BIN_EXE_obg"))
        .arg(&file)
        .current_dir(&scratch)
        .stdin(console())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("XDG_DATA_HOME", scratch.join("data"))
        .env("XDG_STATE_HOME", scratch.join("state"))
        .env("XDG_CACHE_HOME", scratch.join("cache"))
        .spawn()
        .expect("obg");
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = started.try_wait().expect("obg") {
            break status;
        }
        assert!(Instant::now() < deadline, "obg is still waiting");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut said = String::new();
    started
        .stderr
        .take()
        .expect("piped")
        .read_to_string(&mut said)
        .expect("what it said");
    assert!(!status.success(), "obg said it started: {said}");
    assert!(said.contains("unreadable.txt"), "obg said: {said}");

    let starting = std::fs::read_dir(scratch.join("state").join("obelus"))
        .expect("the log")
        .flat_map(|entry| std::fs::read_to_string(entry.expect("a log").path()))
        .flat_map(|log| {
            log.lines()
                .filter(|line| line.contains("Obelus starting"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let [starting] = &starting[..] else {
        panic!("one start, and the log says {starting:?}");
    };
    let whose = starting.split_whitespace().next().expect("whose line");
    assert_ne!(
        whose,
        started.id().to_string(),
        "the console's own obg started"
    );
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The input of a console, which is a terminal to anybody who asks -- this
/// test's own, or one of its own where it was started without.
#[cfg(windows)]
fn console() -> std::fs::File {
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONIN$")
    };
    open()
        .or_else(|_| {
            // Safety: it takes nothing, and a failure is said by the open
            // after it.
            unsafe { windows_sys::Win32::System::Console::AllocConsole() };
            open()
        })
        .expect("a console")
}
