//! Giving back the terminal `obg` was typed into.
//!
//! A binary of its own beside `subsystem`, which is about Windows and is
//! nothing on the systems this is about. Both run the real `obg`, so
//! neither links anything of Obelus's.
//!
//! Linux alone: these start `obg` with no window system to open a window
//! on, which a Mac always has.

#![cfg(target_os = "linux")]

use std::{
    ffi::CStr,
    fs::File,
    os::fd::FromRawFd as _,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// `obg` typed into a terminal ends as soon as the window is up, rather than
/// when the window closes.
///
/// There is no window system here, so the window never opens, and an `obg`
/// that kept the terminal ends there with a failure: ending with success is
/// the one that went first, having heard from the one away from the
/// terminal that it got as far as a window.
///
/// Broken deliberately by taking `leave_the_terminal` out of `main`: `obg`
/// fails with "no window system to open a window on".
#[test]
fn obg_typed_into_a_terminal_gives_it_back() {
    let scratch = scratch("gives-it-back");
    let status = finished(&mut typed_into_a_terminal(&scratch, &scratch));
    assert!(status.success(), "obg kept the terminal: {status}");
}

/// A start that is refused is still said on the terminal, and the shell
/// hears that it failed.
///
/// Broken deliberately by moving `leaving::up` in `main` above
/// `startup::start`: the first `obg` hears "up" before the file is read, and
/// ends with success while the other fails out of sight.
#[test]
fn a_refused_start_is_said_on_the_terminal() {
    let scratch = scratch("refused");
    // Not UTF-8, which is a file Obelus will not read.
    let file = scratch.join("unreadable.txt");
    std::fs::write(&file, [0xff, 0xfe, 0xfd]).expect("the file");
    let mut started = typed_into_a_terminal(&scratch, &file);
    let status = finished(&mut started);
    let mut said = String::new();
    std::io::Read::read_to_string(&mut started.stderr.take().expect("piped"), &mut said)
        .expect("what it said");
    assert!(!status.success(), "obg said it started: {said}");
    assert!(said.contains("unreadable.txt"), "obg said: {said}");
}

/// A directory of its own, which is also where `obg` keeps what it keeps.
fn scratch(name: &str) -> PathBuf {
    let scratch =
        std::env::temp_dir().join(format!("obelus-gui-leaving-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch directory");
    scratch
}

/// `obg` on `path`, with a terminal on its standard input and nowhere to
/// open a window.
fn typed_into_a_terminal(scratch: &Path, path: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_obg"))
        .arg(path)
        .current_dir(scratch)
        .stdin(terminal())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("HOME", scratch)
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("XDG_DATA_HOME", scratch.join("data"))
        .env("XDG_STATE_HOME", scratch.join("state"))
        .env("XDG_CACHE_HOME", scratch.join("cache"))
        .spawn()
        .expect("obg")
}

/// The far end of a new pseudo-terminal, which is a terminal to anybody
/// who asks.
fn terminal() -> File {
    // Safety: each call is checked, and the name is copied before the next.
    unsafe {
        let near = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(near >= 0, "no pseudo-terminal");
        assert_eq!(libc::grantpt(near), 0);
        assert_eq!(libc::unlockpt(near), 0);
        let name = libc::ptsname(near);
        assert!(!name.is_null());
        let name = CStr::from_ptr(name).to_owned();
        // Left open for as long as the test runs, so the far end is not
        // hung up on.
        std::mem::forget(File::from_raw_fd(near));
        let far = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        assert!(far >= 0, "the far end");
        File::from_raw_fd(far)
    }
}

/// How `obg` ended, within a minute.
fn finished(started: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = started.try_wait().expect("obg") {
            return status;
        }
        assert!(Instant::now() < deadline, "obg is still waiting");
        std::thread::sleep(Duration::from_millis(20));
    }
}
