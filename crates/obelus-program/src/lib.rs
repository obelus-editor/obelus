//! Finding a program this machine has, and starting the thing it turned out
//! to be.
//!
//! Two questions that look like one question and are not, and both of them
//! are only interesting on Windows.
//!
//! What a program is *called* there is not what it is called here:
//! `rust-analyzer` is `rust-analyzer.exe`, and the endings that count are the
//! reader's own `PATHEXT` rather than a list Obelus is entitled to write out.
//! [`which`] answers that one, and answers more of it than a search written
//! here did -- the application's directory and the working directory come
//! before `PATH` on that platform, and a name with a separator in it is not a
//! search at all.
//!
//! And what a program *is* there is not always a program. Everything npm
//! installs arrives as a `.cmd` beside a shell script of the same name, and a
//! `.cmd` is a file for the command processor to read rather than one the
//! kernel can start: `CreateProcess` -- which is what `std::process::Command`
//! is -- reads the shell script's `#!` line as machine code and says the file
//! is not a valid application. So the processor is what gets started, and the
//! shim becomes the first thing it is told to do. Nothing answers that one,
//! so it is here.
//!
//! Both are here rather than beside the one caller because there are two: the
//! language servers ask whether a server is installed, and the agents start
//! `npm` and then whatever `npm` wrote. One answer, so the two cannot come to
//! disagree about what this machine has.
//!
//! And a third that is only Windows's: a program started there gets a console
//! window of its own unless it is told not to, wherever the program starting
//! it has none to share -- which is `obg`, a window and not a console program.
//! [`without_a_window`] is the telling, and every program Obelus starts with
//! its pipes held asks it.
//!
//! And one that is everywhere but Windows: what is on the path at all
//! depends on who started Obelus, and a desktop is not a shell
//! ([`login`]).

#[cfg(unix)]
pub mod login;

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Starts a program without a console window of its own, on Windows, and does
/// nothing anywhere else.
///
/// Every program Obelus starts talks to it through pipes and has nothing to
/// show a reader in a window -- a language server, `npm`, a command an agent
/// asked for. Started from `obg`, which has no console to lend them, each
/// would put up one of its own and leave it on the screen for as long as it
/// ran. Not `DETACHED_PROCESS`, which gives it no console at all: whatever it
/// starts in turn would then put up a window of its own instead.
pub fn without_a_window(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command
}

/// Whether a command can be found.
#[must_use]
pub fn on_path(command: &str) -> bool {
    found(command).is_some()
}

/// The file a command names, where there is one.
#[must_use]
pub fn found(command: &str) -> Option<PathBuf> {
    which::which(command).ok()
}

/// A program and its arguments, as this machine can actually start them.
///
/// Unchanged nearly always, and the whole of what it is for is the `.cmd` and
/// `.bat` a Windows install leaves behind: those are started by handing them
/// to the command processor, which `COMSPEC` names.
#[must_use]
pub fn as_started_here(command: &Path, arguments: &[String]) -> (PathBuf, Vec<String>) {
    if !is_batch(command) {
        return (command.to_path_buf(), arguments.to_vec());
    }
    let processor =
        std::env::var_os("COMSPEC").map_or_else(|| PathBuf::from("cmd.exe"), PathBuf::from);
    let mut all = vec!["/C".to_string(), command.display().to_string()];
    all.extend(arguments.iter().cloned());
    (processor, all)
}

/// Whether a file is one the command processor reads rather than one this
/// machine runs.
///
/// Only asked on Windows. A unix file may be named `deploy.bat` and be an
/// ordinary program, and handing that to a command processor which is not
/// there would turn a program that runs into one that does not.
fn is_batch(command: &Path) -> bool {
    cfg!(windows)
        && command
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ending| {
                ending.eq_ignore_ascii_case("cmd") || ending.eq_ignore_ascii_case("bat")
            })
}

#[cfg(test)]
mod tests {
    use super::{as_started_here, found, on_path};

    /// The probe has to reject a directory and a non-executable file, or
    /// Obelus tries to spawn something that cannot run and reports it as the
    /// server failing rather than as never having been there.
    ///
    /// And it has to find what is there. The name is this platform's own,
    /// because the two are not the same question: `cmd` is on every Windows
    /// machine as `cmd.exe`, so finding it is finding a program whose name on
    /// the path is not the name it was asked for -- which is every program
    /// there, and was the whole of the bug.
    #[test]
    fn the_probe_finds_a_real_command_and_nothing_else() {
        let certain = match cfg!(windows) {
            true => "cmd",
            false => "sh",
        };
        assert!(on_path(certain), "{certain} should be on PATH");
        assert!(found(certain).is_some_and(|it| it.is_absolute()));
        assert!(!on_path("obelus-not-a-real-command"));
        // A directory that exists on PATH-like paths must not count.
        assert!(!on_path("."));
    }

    /// A program npm installed is started by whatever can start it.
    ///
    /// Run rather than asserted about: the question is whether this machine
    /// will start the thing, and the only answer to that is the thing having
    /// started. The arguments go through it too, because a shim that runs and
    /// is told nothing is an agent that is handed no `--stdio` and sits there
    /// saying nothing -- which looks exactly like a server still indexing.
    ///
    /// Broken deliberately by returning the shim unchanged: the spawn fails
    /// with "not a valid application" and no agent npm installed would start
    /// at all.
    #[cfg(windows)]
    #[test]
    fn a_shim_the_command_processor_owns_is_started_through_it() {
        let home = std::env::temp_dir().join(format!("obelus-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("a directory");
        let shim = home.join("say.cmd");
        std::fs::write(&shim, "@echo off\r\necho said %1\r\n").expect("the shim");

        let (program, arguments) = as_started_here(&shim, &["--stdio".to_string()]);
        let outcome = std::process::Command::new(program)
            .args(arguments)
            .output()
            .expect("starting it");
        assert!(outcome.status.success(), "the shim did not run");
        assert_eq!(
            String::from_utf8_lossy(&outcome.stdout).trim(),
            "said --stdio",
            "the shim ran without what it was told"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// A program started here has no console window, which is what `obg`
    /// starting a language server would otherwise put on the reader's
    /// screen for as long as the server ran.
    ///
    /// The program is this test binary again, running the test below it,
    /// which says whether it was given a window. Started the ordinary way
    /// it shares the one `cargo test` is in, or is given one of its own
    /// where there is none, and says so either way.
    ///
    /// Broken deliberately by taking `creation_flags` out of
    /// `without_a_window`.
    #[cfg(windows)]
    #[test]
    fn a_program_started_here_has_no_console_window() {
        let mut command = std::process::Command::new(std::env::current_exe().expect("this test"));
        command
            .args([
                "--exact",
                "tests::says_whether_it_has_a_console_window",
                "--nocapture",
            ])
            .env("OBELUS_SAY_THE_WINDOW", "1");
        let outcome = super::without_a_window(&mut command)
            .output()
            .expect("starting it");
        let said = String::from_utf8_lossy(&outcome.stdout);
        // Its own line, and not merely the absence of the other one: a
        // child that never got as far as asking says neither.
        assert!(said.contains("window: none"), "it said {said:?}");
    }

    /// The other half of the one above, and nothing when run on its own.
    #[cfg(windows)]
    #[test]
    fn says_whether_it_has_a_console_window() {
        if std::env::var_os("OBELUS_SAY_THE_WINDOW").is_none() {
            return;
        }
        // Safety: it takes nothing and only answers.
        let window = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() };
        println!(
            "window: {}",
            match window.is_null() {
                true => "none",
                false => "some",
            }
        );
    }

    /// Anything else is handed over as it is.
    #[test]
    fn an_ordinary_program_is_started_as_itself() {
        let program = std::path::Path::new("rust-analyzer");
        let (started, arguments) = as_started_here(program, &["--stdio".to_string()]);
        assert_eq!(started, program);
        assert_eq!(arguments, ["--stdio"]);
    }
}
