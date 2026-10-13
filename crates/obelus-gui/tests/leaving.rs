//! Giving back the terminal `obg` was typed into.
//!
//! A binary of its own beside `subsystem`, which is what `obg` says it is
//! to Windows. Both run the real `obg`, so neither links anything of
//! Obelus's.
//!
//! Linux and Windows. On Linux these start `obg` where it cannot open a
//! window, which on a Mac it always can -- and a window opened by a test is
//! one nobody closes. On Windows one is opened, and closed by the test that
//! opened it, because what is let go of once it is up is the half that is
//! Windows' own.

#![cfg(any(target_os = "linux", windows))]

#[cfg(target_os = "linux")]
use std::{
    ffi::CStr,
    fs::File,
    os::{fd::FromRawFd as _, unix::net::UnixListener},
};
use std::{
    io::Read as _,
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
#[cfg(target_os = "linux")]
#[test]
fn obg_typed_into_a_terminal_starts_again_away_from_it() {
    let scratch = Scratch::new("starts-again");
    let mut started = typed_into_a_terminal(&scratch, &scratch);
    finished(&mut started);
    assert_ne!(
        whose_start(&scratch).expect("one start in the log"),
        started.id(),
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
fn typed_into_a_terminal(scratch: &Path, path: &Path) -> Child {
    obg(scratch, path).spawn().expect("obg")
}

#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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

/// `obg` typed into a Windows console gives it back once the window is up:
/// the `obg` the shell started ends, and nothing the shell gave it is still
/// held -- not its standard error, and not its console.
///
/// nushell and Git Bash wait for a window program as they wait for any
/// other, and `obg | ...` waits for the far end of the pipe as well; a
/// window still on the console is ended with it when the terminal closes.
///
/// Broken deliberately four ways: by taking `leave_the_terminal` out of
/// `main` (the console's own `obg` is the window), by taking
/// `hand_on_none_of_the_shells_handles` out of it (the window keeps a copy
/// of the standard error and the read never ends), by leaving standard
/// error open in `let_go_of_the_terminal` (the same), and by taking
/// `FreeConsole` out of it (the window is on the console).
#[cfg(windows)]
#[test]
fn obg_typed_into_a_console_gives_it_back() {
    let scratch = Scratch::new("gives-it-back");
    let file = scratch.join("read.txt");
    std::fs::write(&file, "read\n").expect("the file");
    let mut started = typed_into_a_console(&scratch, &file);
    let mut error = started.stderr.take().expect("piped");
    let status = finished(&mut started);
    let window = Window(the_window(&scratch));
    assert!(status.success(), "obg said it did not start");
    assert_ne!(
        window.0,
        started.id(),
        "the console's own obg is the window"
    );
    let (sent, said) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut said = String::new();
        let _ = error.read_to_string(&mut said);
        let _ = sent.send(said);
    });
    assert!(
        said.recv_timeout(Duration::from_secs(30)).is_ok(),
        "the window still holds the standard error"
    );
    assert!(
        !on_this_console().contains(&window.0),
        "the window is still on the console"
    );
}

/// A start that is refused is still said on the console, and the shell
/// hears that it failed -- from the `obg` started again, which is the one
/// that read the file.
///
/// Broken deliberately by taking `leave_the_terminal` out of `main`: the
/// start is the console's own process. And by sending the standard error
/// of the `obg` started again to nowhere: nothing is said.
#[cfg(windows)]
#[test]
fn a_refused_start_is_said_on_the_console() {
    let scratch = Scratch::new("refused-on-a-console");
    // Not UTF-8, which is a file Obelus will not read.
    let file = scratch.join("unreadable.txt");
    std::fs::write(&file, [0xff, 0xfe, 0xfd]).expect("the file");
    let mut started = typed_into_a_console(&scratch, &file);
    let status = finished(&mut started);
    let said = said(&mut started);
    assert!(!status.success(), "obg said it started: {said}");
    assert!(said.contains("unreadable.txt"), "obg said: {said}");
    assert_ne!(
        whose_start(&scratch).expect("one start in the log"),
        started.id(),
        "the console's own obg started"
    );
}

/// `obg` on `path`, with a console on its standard input.
///
/// Only where its log goes is moved: the settings are found where Windows
/// says they are rather than through the environment, so these read the
/// reader's own -- which a start does not write.
#[cfg(windows)]
fn typed_into_a_console(scratch: &Path, path: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_obg"))
        .arg(path)
        .current_dir(scratch)
        .stdin(console())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("XDG_STATE_HOME", scratch.join("state"))
        .spawn()
        .expect("obg")
}

/// The input of this test's console, or of one of its own where it was
/// started without one -- kept out of sight, because it is only there to
/// be a terminal to anybody who asks.
#[cfg(windows)]
fn console() -> std::fs::File {
    use windows_sys::Win32::{
        System::Console::{AllocConsole, GetConsoleWindow},
        UI::WindowsAndMessaging::{SW_HIDE, ShowWindow},
    };
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONIN$")
    };
    open()
        .or_else(|_| {
            // Safety: each takes nothing but what the one before gave, and
            // a failure is said by the open after them.
            unsafe {
                AllocConsole();
                ShowWindow(GetConsoleWindow(), SW_HIDE);
            }
            open()
        })
        .expect("a console")
}

/// Which process the log says started, if exactly one did.
fn whose_start(scratch: &Path) -> Option<u32> {
    let logs = scratch.join("state").join("obelus");
    let starting = std::fs::read_dir(logs)
        .ok()?
        .flat_map(|entry| std::fs::read_to_string(entry.ok()?.path()).ok())
        .flat_map(|log| {
            log.lines()
                .filter(|line| line.contains("Obelus starting"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let [starting] = &starting[..] else {
        return None;
    };
    starting.split_whitespace().next()?.parse().ok()
}

/// The window's process, once its log says it started.
#[cfg(windows)]
fn the_window(scratch: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(whose) = whose_start(scratch) {
            return whose;
        }
        assert!(Instant::now() < deadline, "the log says nothing started");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The processes on this test's console.
#[cfg(windows)]
fn on_this_console() -> Vec<u32> {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    let mut processes = vec![0; 64];
    // Safety: the length given is the buffer's own.
    let count = unsafe {
        GetConsoleProcessList(
            processes.as_mut_ptr(),
            u32::try_from(processes.len()).expect("a length"),
        )
    };
    processes.truncate(usize::try_from(count).expect("a count"));
    processes
}

/// A window a test opened, closed when the test is done with it -- before
/// its scratch directory, which the window is working in.
#[cfg(windows)]
struct Window(u32);

#[cfg(windows)]
impl Drop for Window {
    fn drop(&mut self) {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &self.0.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
