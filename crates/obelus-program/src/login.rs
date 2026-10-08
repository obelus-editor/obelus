//! What the reader's shell would have handed a program it started, for a
//! program it did not start.
//!
//! A window opened from the Dock or a launcher is started by the desktop,
//! and the desktop never read `.zshrc`. On a Mac the whole of its `PATH` is
//! `/usr/bin:/bin:/usr/sbin:/sbin`, so `/opt/homebrew/bin` and every
//! directory nvm made are not on it: npm was "not on the path" on a machine
//! where the reader types `npm` every day, and an agent npm installed --
//! a script that starts with `#!/usr/bin/env node` -- died before it said a
//! word, which reached the reader as a conversation that ended. A Linux
//! session reads `~/.profile` and no further, which is the same thing for
//! everybody whose nvm is in `.bashrc`.
//!
//! So the shell is asked, the way the reader's terminal would ask it: as a
//! login shell and an interactive one, because zsh reads `.zshrc` only when
//! it is interactive and bash reads `.bash_profile` only when it is a login.
//! Everything it has, not only `PATH`: a proxy, an API key, where a version
//! manager keeps its versions are all set in the same files, and an agent
//! needs them as much as it needs `node`.
//!
//! An interactive shell is a shell somebody may have taught to talk: a
//! greeting, a prompt that draws itself early, a question. What it prints
//! before Obelus's mark is not the environment, and a shell that does not
//! finish in time is let go -- a slow `.zshrc` costs the start a few seconds
//! and no more, and the window opens with what the desktop gave it.

use std::{
    ffi::OsString,
    io::Read as _,
    os::unix::ffi::OsStringExt as _,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

/// Printed by the shell just before the environment, so that whatever its
/// files printed first can be told apart from it.
const MARK: &str = "--obelus-environment--";

/// What the shell's own bookkeeping is, rather than anything the reader set:
/// where that shell was, how deep it was nested, and the last program it
/// ran. Taken, each would describe a shell that has already gone.
const THE_SHELLS_OWN: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// Everything `shell` has in its environment when started as the reader's
/// terminal would start it, waiting `within` for it and no longer.
///
/// # Errors
///
/// What went wrong, in words for the log: the shell did not start, did not
/// finish in time, or finished without printing an environment.
pub fn environment(shell: &Path, within: Duration) -> Result<Vec<(OsString, OsString)>, String> {
    asked(asking(shell), within)
}

/// The shell, started the way a terminal starts it.
///
/// Nothing to read from, because a shell that asks something has nobody to
/// answer it; and nowhere for its complaints to go -- an interactive shell
/// with no terminal says so, and so does a `.zshrc` that expected one.
fn asking(shell: &Path) -> Command {
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c"])
        .arg(format!("printf '%s' '{MARK}'; /usr/bin/env -0"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

fn asked(mut command: Command, within: Duration) -> Result<Vec<(OsString, OsString)>, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("the shell did not start: {error}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    // A thread of its own, because reading is the one way to know the shell
    // has finished and it does not stop for a clock. It may outlive this
    // function: a `.zshrc` that starts something in the background lends it
    // the pipe, and the read ends only when that goes. It touches nothing
    // but the pipe, so it is no reason not to change the environment.
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut printed = Vec::new();
        let _ = stdout.read_to_end(&mut printed);
        let _ = said.send(printed);
    });
    let Ok(printed) = heard.recv_timeout(within) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("the shell had not finished after {within:?}"));
    };
    let _ = child.wait();
    read(&printed).ok_or_else(|| "the shell printed no environment".to_string())
}

/// The environment in what the shell printed, where there is one.
///
/// Only entries the terminating NUL says are whole: a `.zlogout` may print
/// after `env` has, and that is not the value of the last variable.
fn read(printed: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let at = printed
        .windows(MARK.len())
        .position(|window| window == MARK.as_bytes())?;
    let after = &printed[at + MARK.len()..];
    let whole = &after[..after
        .iter()
        .rposition(|byte| *byte == 0)
        .map_or(0, |end| end + 1)];
    let found = whole
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let equals = entry.iter().position(|byte| *byte == b'=')?;
            let (name, value) = (&entry[..equals], &entry[equals + 1..]);
            // An empty name is not one `set_var` takes.
            (!name.is_empty() && !THE_SHELLS_OWN.iter().any(|own| own.as_bytes() == name)).then(
                || {
                    (
                        OsString::from_vec(name.to_vec()),
                        OsString::from_vec(value.to_vec()),
                    )
                },
            )
        })
        .collect::<Vec<_>>();
    (!found.is_empty()).then_some(found)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    use super::{MARK, asked, asking, read};

    /// A directory to be somebody's home in, gone when the test is.
    struct Home(PathBuf);

    impl Home {
        fn with_a_profile(name: &str, profile: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("obelus-login-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&path).expect("a home");
            std::fs::write(path.join(".profile"), profile).expect("a profile");
            Self(path)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn value(found: &[(OsString, OsString)], name: &str) -> Option<String> {
        found
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    /// What a login shell's own files set is what comes back -- which is the
    /// whole of the point, because what the desktop already had would have
    /// come back from any shell at all.
    ///
    /// `/bin/sh`, because it is on every machine this runs on and reads
    /// `~/.profile` as a login shell, whichever shell it turns out to be.
    ///
    /// Broken deliberately by dropping `-l` from `asking`: the profile is not
    /// read and the variable is not there.
    #[test]
    fn what_the_login_files_set_comes_back() {
        let home = Home::with_a_profile(
            "profile",
            "echo 'a greeting nobody asked for'\nexport OBELUS_FROM_THE_PROFILE='it was read'\n",
        );
        let mut shell = asking(Path::new("/bin/sh"));
        shell.env("HOME", &home.0);
        let found = asked(shell, Duration::from_secs(30)).expect("an environment");
        assert_eq!(
            value(&found, "OBELUS_FROM_THE_PROFILE").as_deref(),
            Some("it was read"),
            "the login files were not read"
        );
        assert_eq!(value(&found, "HOME"), Some(home.0.display().to_string()));
        // The shell's own count of how deep it was, which would tell every
        // program Obelus starts that it is inside a shell.
        assert_eq!(value(&found, "SHLVL"), None);
    }

    /// A shell that does not finish is let go, and the start goes on.
    ///
    /// Broken deliberately by waiting on `recv` with no clock: the test takes
    /// as long as the profile sleeps, and fails on the time.
    #[test]
    fn a_shell_that_does_not_finish_is_let_go() {
        let home = Home::with_a_profile("slow", "sleep 30\n");
        let mut shell = asking(Path::new("/bin/sh"));
        shell.env("HOME", &home.0);
        let started = Instant::now();
        assert!(asked(shell, Duration::from_millis(500)).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the shell was waited on for {:?}",
            started.elapsed()
        );
    }

    /// What a shell's files print around the environment is not part of it.
    ///
    /// Broken deliberately by reading from the start rather than from the
    /// mark: the greeting becomes the name of the first variable. And by
    /// keeping what follows the last NUL: the farewell becomes a variable.
    #[test]
    fn what_is_printed_around_the_environment_is_not_in_it() {
        let printed = format!(
            "Welcome=to the shell\n{MARK}PATH=/opt/homebrew/bin:/usr/bin\0EMPTY=\0SHLVL=2\0Goodbye=from .zlogout"
        );
        let found = read(printed.as_bytes()).expect("an environment");
        assert_eq!(
            found,
            [
                ("PATH".into(), "/opt/homebrew/bin:/usr/bin".into()),
                ("EMPTY".into(), "".into()),
            ]
        );
        assert_eq!(read(b"no mark at all\0PATH=/usr/bin\0"), None);
    }
}
