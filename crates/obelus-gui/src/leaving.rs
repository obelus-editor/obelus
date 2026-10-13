//! Giving back the terminal `obg` was typed into.
//!
//! **A window started from a terminal gives the terminal back.** The window
//! is somewhere else on the screen, and the shell it was typed into is the
//! reader's to go on typing in -- gvim's `fork`, for the same reason. Not a
//! `fork` here, because by the time there is anything to fork for there are
//! threads, and a forked child has only the one that forked it: `obg`
//! starts itself again in a session of its own, which no terminal hangs up
//! on when it closes, and the first one goes.
//!
//! On Windows that session is a console, and not having one: `obg` is a
//! window program, which `cmd` and PowerShell do not wait for -- but
//! nushell and Git Bash wait for whatever they start, window or not. So it
//! starts itself again there too, and the second takes the same console the
//! first did (`main`) and lets go of it once it is up; a window still
//! attached to a console is ended with it when the terminal closes.
//!
//! Not straight away. What stops a start -- a file that will not read, no
//! window system to open a window on, as over `ssh` -- is said on the
//! terminal, and the shell's prompt comes after it with the status of the
//! start that failed; so the first `obg` waits for the second to say it is
//! up, or to stop. Up is a window with something to draw it with: what goes
//! wrong after that goes wrong where a window started from a menu says it,
//! in the log.
//!
//! While it waits, `ctrl+c` is the window's too: the reader pressed it to
//! stop a start, and the first `obg` is only the half that heard it. On a
//! Windows console there is nothing to pass on, because both are on it and
//! it tells both -- which is not said here for mintty, Git Bash's own
//! window, where it is not a console that hears the key.
//!
//! The test is a terminal on standard input, and that is all it can be: a
//! script started from a terminal has one there as well, and so does `git
//! commit` running `GIT_EDITOR=obg`, and both stop waiting for the window.
//! Neither is what `obg` is for -- the editor in a terminal is `ob` -- and
//! one that has to wait gives `obg` an input that is not a terminal,
//! `obg </dev/null` -- `<NUL` in `cmd`.

use std::{
    io::{IsTerminal as _, Read as _, Write as _},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
};
#[cfg(unix)]
use std::{
    os::{fd::AsRawFd as _, unix::process::CommandExt as _},
    sync::atomic::AtomicI32,
};

/// Set on the `obg` started again, which is how it knows that something
/// is waiting on its standard output to hear that it is up.
const SAY_WHEN_UP: &str = "OBELUS_SAY_WHEN_UP";

/// Whether something is waiting to hear [`up`], which is said once.
static WAITED_FOR: AtomicBool = AtomicBool::new(false);

/// Starts `obg` again away from the terminal it was typed into, and ends
/// this one once that one is up -- with its status, where it stopped first.
///
/// Returns where there is no terminal to give back, or `obg` could not be
/// started again; this one is the window then, as it was before.
///
/// Before anything else, because what it starts is this whole program
/// again.
pub(crate) fn leave_the_terminal() {
    if !std::io::stdin().is_terminal() {
        return;
    }
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(_) => return,
    };
    let Ok((mut heard, said)) = std::io::pipe() else {
        return;
    };
    let mut command = Command::new(program);
    command
        .args(std::env::args_os().skip(1))
        // Standard error stays the terminal's until the window is up, which
        // is where a refused start is said.
        .stdin(Stdio::null())
        .stdout(said)
        .env(SAY_WHEN_UP, "1");
    // It has this one's environment, which is the terminal's: without
    // this, a standard error sent elsewhere leaves it with no terminal to
    // see, and it asks the login shell over the top of it.
    #[cfg(unix)]
    command.env(obelus_program::login::PASSED_ON, "1");
    // Safety: `setsid` is async-signal-safe, and it is the only thing done
    // between the fork and the exec.
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
    #[cfg(windows)]
    hand_on_none_of_the_shells_handles();
    let Ok(mut started) = command.spawn() else {
        return;
    };
    // Its copy of the pipe's other end, which would otherwise keep this one
    // reading after the window stopped without a word.
    drop(command);
    #[cfg(unix)]
    pass_on_interrupts_to(&started);
    let mut byte = [0];
    let heard = loop {
        match heard.read(&mut byte) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            heard => break heard,
        }
    };
    if let Ok(1) = heard
        && byte == *b"!"
    {
        std::process::exit(0);
    }
    let status = started.wait().ok().and_then(|status| {
        // As a shell says it, where a signal stopped the start: one more
        // than 128 for each.
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt as _;
            status.code().or(status.signal().map(|signal| 128 + signal))
        };
        #[cfg(windows)]
        let status = status.code();
        status
    });
    std::process::exit(status.unwrap_or(1));
}

/// Keeps the window from the handles the shell gave this `obg`.
///
/// Windows gives a child every handle its parent may hand on, and the three
/// standard ones a shell gives are among them: kept by the window, the far
/// end of `obg | ...` holds the shell until the window closes, which is the
/// wait this is all here to be rid of. What the window is meant to have it
/// is given as copies of its own, which this does not touch (`Stdio`).
#[cfg(windows)]
fn hand_on_none_of_the_shells_handles() {
    use windows_sys::Win32::{
        Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation},
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // Safety: handles this process owns, and only whether they are
        // handed on changes.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// The `obg` started again, for [`pass_on`].
#[cfg(unix)]
static STARTED: AtomicI32 = AtomicI32::new(0);

/// Hands `ctrl+c` to the window that is starting, which no longer has the
/// terminal to hear it from.
#[cfg(unix)]
fn pass_on_interrupts_to(started: &std::process::Child) {
    let Ok(id) = i32::try_from(started.id()) else {
        return;
    };
    STARTED.store(id, Ordering::Relaxed);
    // Safety: `pass_on` does nothing but what a signal handler may.
    unsafe {
        libc::signal(libc::SIGINT, pass_on as *const () as libc::sighandler_t);
    }
}

#[cfg(unix)]
extern "C" fn pass_on(signal: libc::c_int) {
    // Safety: `kill` is async-signal-safe.
    unsafe {
        libc::kill(STARTED.load(Ordering::Relaxed), signal);
    }
}

/// Hears whether this `obg` was started by [`leave_the_terminal`], and so
/// owes it [`up`]. Taken off the environment so that nothing this one
/// starts is told it too.
///
/// Before anything starts a thread, for the same reason.
pub(crate) fn hear_who_waits() {
    if std::env::var_os(SAY_WHEN_UP).is_some() {
        // Safety: nothing else is running yet -- see `main`.
        unsafe { std::env::remove_var(SAY_WHEN_UP) };
        WAITED_FOR.store(true, Ordering::Relaxed);
    }
}

/// Tells the `obg` typed into the terminal that this one is up, and lets go
/// of the terminal's standard error, so that nothing is written to a
/// terminal that may since have closed.
pub(crate) fn up() {
    if !WAITED_FOR.swap(false, Ordering::Relaxed) {
        return;
    }
    let _ = std::io::stdout().write_all(b"!");
    let _ = std::io::stdout().flush();
    let_go_of_the_terminal();
}

#[cfg(unix)]
fn let_go_of_the_terminal() {
    let Ok(nowhere) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return;
    };
    // Safety: three descriptors this process owns, and `dup2` only
    // replaces what the two standard ones refer to.
    unsafe {
        libc::dup2(nowhere.as_raw_fd(), libc::STDOUT_FILENO);
        libc::dup2(nowhere.as_raw_fd(), libc::STDERR_FILENO);
    }
}

/// And of the console, which on Windows ends every program still attached
/// to it when its window closes -- this one included.
///
/// Standard output is the pipe to the `obg` that has gone, and is put
/// aside rather than closed. Standard error is closed, because it may be
/// the far end of `obg 2>&1 | ...` and the shell waits until it is. Not an
/// exchange the way `dup2` is: what Obelus writes asks for the handle on
/// every write, and is held off while it changes; C's own streams kept the
/// number at the start, and nothing Obelus links writes through them.
#[cfg(windows)]
fn let_go_of_the_terminal() {
    use std::os::windows::io::IntoRawHandle as _;

    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Console::{
            FreeConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        },
    };
    let held = (std::io::stdout().lock(), std::io::stderr().lock());
    for (which, close) in [(STD_OUTPUT_HANDLE, false), (STD_ERROR_HANDLE, true)] {
        let Ok(nowhere) = std::fs::OpenOptions::new().write(true).open("NUL") else {
            continue;
        };
        // Safety: handles this process owns, and the old one is closed only
        // once nothing can ask for it.
        unsafe {
            let was = GetStdHandle(which);
            if SetStdHandle(which, nowhere.into_raw_handle()) != 0 && close && !was.is_null() {
                CloseHandle(was);
            }
        }
    }
    drop(held);
    // Whatever became of the two above, because a window still on the
    // console goes when the terminal does.
    //
    // Safety: it takes nothing, and a failure is only an answer.
    unsafe {
        FreeConsole();
    }
}
