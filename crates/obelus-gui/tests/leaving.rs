//! Giving back the terminal `obg` was typed into.
//!
//! A binary of its own beside `subsystem`, which is about Windows and is
//! nothing on the systems this is about. Both run the real `obg`, so
//! neither links anything of Obelus's.
//!
//! Linux alone: these start `obg` where it cannot open a window, which on a
//! Mac it always can -- and a window opened by a test is one nobody closes.

#![cfg(target_os = "linux")]

use std::{
    ffi::CStr,
    fs::File,
    io::Read as _,
    os::{fd::FromRawFd as _, unix::net::UnixListener},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// `obg` typed into a terminal starts itself again, away from it.
///
/// Which process opened the window is in the log, because every line says
/// whose it is: the start that read the paths is not the one the terminal
/// started. There is no window system here, so both end, and the one the
/// terminal started ends with the other's failure.
///
/// Broken deliberately by taking `leave_the_terminal` out of `main`: the
/// start is the terminal's own process.
#[test]
fn obg_typed_into_a_terminal_starts_again_away_from_it() {
    let scratch = Scratch::new("starts-again");
    let mut started = typed_into_a_terminal(&scratch, &scratch);
    finished(&mut started);
    let logs = scratch.join("state").join("obelus");
    let starting = std::fs::read_dir(&logs)
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
        "the terminal's own obg started"
    );
}

/// A start with no window system to open a window on -- `obg` over `ssh` --
/// says so on the terminal, and the shell hears that it failed.
///
/// Broken deliberately by calling `leaving::up` in `main` once
/// `startup::start` has read the paths, which is where it was first called:
/// the first `obg` hears "up" before there is a window, and ends with
/// success while the other fails out of sight.
#[test]
fn no_window_is_said_on_the_terminal() {
    let scratch = Scratch::new("no-window");
    let mut started = typed_into_a_terminal(&scratch, &scratch);
    let status = finished(&mut started);
    let said = said(&mut started);
    assert!(!status.success(), "obg said it started: {said}");
    assert!(said.contains("no window system"), "obg said: {said}");
}

/// A start that is refused is still said on the terminal, and the shell
/// hears that it failed.
///
/// Broken deliberately by calling `leaving::up` before `startup::start` in
/// `main`: the first `obg` hears "up" before the file is read, and ends
/// with success while the other fails out of sight.
#[test]
fn a_refused_start_is_said_on_the_terminal() {
    let scratch = Scratch::new("refused");
    // Not UTF-8, which is a file Obelus will not read.
    let file = scratch.join("unreadable.txt");
    std::fs::write(&file, [0xff, 0xfe, 0xfd]).expect("the file");
    let mut started = typed_into_a_terminal(&scratch, &file);
    let status = finished(&mut started);
    let said = said(&mut started);
    assert!(!status.success(), "obg said it started: {said}");
    assert!(said.contains("unreadable.txt"), "obg said: {said}");
}

/// `ctrl+c` while the window is starting stops the window, not only the
/// `obg` the terminal sent it to.
///
/// The window system is a socket that never answers, so the start waits
/// there for as long as it is let; the other end of that socket hears when
/// it stops.
///
/// Broken deliberately by taking `pass_on_interrupts_to` out of
/// `leave_the_terminal`: the first `obg` ends and the window goes on
/// waiting.
#[test]
fn an_interrupt_while_starting_stops_the_window() {
    let scratch = Scratch::new("interrupted");
    let listening = UnixListener::bind(scratch.join("wayland-0")).expect("the socket");
    let mut command = obg(&scratch, &scratch);
    command
        .env("XDG_RUNTIME_DIR", &*scratch)
        .env("WAYLAND_DISPLAY", "wayland-0");
    let mut started = command.spawn().expect("obg");
    let (mut window, _) = listening.accept().expect("the window asking");
    // Safety: a signal to a child this test started and has not reaped.
    unsafe {
        libc::kill(i32::try_from(started.id()).expect("a pid"), libc::SIGINT);
    }
    let status = finished(&mut started);
    assert!(!status.success(), "obg said it started");
    window
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("a timeout");
    // Whatever it asked for before it stopped, then the end.
    let mut asked = Vec::new();
    let ended = window.read_to_end(&mut asked);
    assert!(ended.is_ok(), "the window is still starting: {ended:?}");
}

/// A directory of its own, which is also where `obg` keeps what it keeps,
/// and which goes when the test does.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let scratch =
            std::env::temp_dir().join(format!("obelus-gui-leaving-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("the scratch directory");
        Self(scratch)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `obg` on `path`, with a terminal on its standard input and nowhere to
/// open a window.
fn typed_into_a_terminal(scratch: &Path, path: &Path) -> Child {
    obg(scratch, path).spawn().expect("obg")
}

fn obg(scratch: &Path, path: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_obg"));
    command
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
        .env("XDG_RUNTIME_DIR", scratch.join("runtime"));
    command
}

/// The far end of a new pseudo-terminal, which is a terminal to anybody
/// who asks.
fn terminal() -> File {
    // Safety: each call is checked, and the name is copied before the next.
    unsafe {
        let near = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(near >= 0, "no pseudo-terminal");
        assert_eq!(libc::grantpt(near), 0);
        assert_eq!(libc::unlockpt(near), 0);
        let name = libc::ptsname(near);
        assert!(!name.is_null());
        let name = CStr::from_ptr(name).to_owned();
        // Left open for as long as the test runs, so the far end is not
        // hung up on.
        std::mem::forget(File::from_raw_fd(near));
        let far = libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
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

/// What `obg` said on the terminal, once it has ended.
fn said(started: &mut Child) -> String {
    let mut said = String::new();
    started
        .stderr
        .take()
        .expect("piped")
        .read_to_string(&mut said)
        .expect("what it said");
    said
}
