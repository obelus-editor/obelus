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
//! around Obelus's two marks is not the environment, and a shell that does
//! not finish in time is let go -- a slow `.zshrc` costs the start a few
//! seconds and no more, and the window opens with what the desktop gave it.
//!
//! And a shell's files do things as well as say them, every time it is
//! asked: an `eval $(ssh-agent)` there starts an agent for each window the
//! desktop opens, and the `SSH_AUTH_SOCK` taken is that agent's rather than
//! the one the desktop had. That is the reader's terminal's behaviour too,
//! which is the argument for asking at all.

use std::{
    ffi::OsString,
    io::Read as _,
    os::unix::ffi::OsStringExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

/// Printed by the shell just before the environment, so that whatever its
/// files printed first can be told apart from it.
const MARK: &str = "--obelus-environment--";

/// Printed just after it, which is how the shell is known to have answered.
/// Not the pipe closing: a `.zshrc` that starts something in the background
/// lends it the pipe, and the pipe closes when that does -- which was every
/// start waiting out the clock and then taking nothing.
const ENDS: &str = "--obelus-environment-ends--";

/// Set on a window started by another window, which has that one's
/// environment and need not ask again.
pub const PASSED_ON: &str = "OBELUS_ENVIRONMENT_PASSED_ON";

/// What the shell's own bookkeeping is, rather than anything the reader set:
/// where that shell was, how deep it was nested, and the last program it
/// ran. Taken, each would describe a shell that has already gone.
const THE_SHELLS_OWN: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// Everything `shell` has in its environment when started as the reader's
/// terminal would start it, waiting `within` for it and no longer.
///
/// # Errors
///
/// What went wrong, in words for the log, beginning with which shell: it
/// did not start, did not finish in time, or finished without printing an
/// environment.
pub fn environment(shell: &Path, within: Duration) -> Result<Vec<(OsString, OsString)>, String> {
    asked(asking(shell), within).map_err(|why| format!("{}: {why}", shell.display()))
}

/// The reader's shell: what `SHELL` says, and where nothing does, what the
/// account says.
///
/// A session a compositor or systemd started may have no `SHELL` at all,
/// and the reader still has a shell -- the one their terminal starts, which
/// is the account's.
#[must_use]
pub fn the_readers_shell() -> Option<PathBuf> {
    std::env::var_os("SHELL")
        .filter(|named| !named.is_empty())
        .map(PathBuf::from)
        .or_else(the_accounts_shell)
}

fn the_accounts_shell() -> Option<PathBuf> {
    use std::ffi::CStr;
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let mut room = vec![0 as libc::c_char; 16 * 1024];
    // Safety: every pointer is to something that lives past the call, and
    // `room` is as long as it is said to be. The reentrant one, because
    // nothing here can promise another thread is not asking too.
    let asked = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            room.as_mut_ptr(),
            room.len(),
            &raw mut found,
        )
    };
    if asked != 0 || found.is_null() {
        return None;
    }
    // Safety: `found` is `entry`, filled in, and its strings are in `room`.
    let shell = unsafe { (*found).pw_shell };
    if shell.is_null() {
        return None;
    }
    // Safety: a string the call wrote into `room`, ended as C ends one.
    let shell = unsafe { CStr::from_ptr(shell) }.to_bytes();
    (!shell.is_empty()).then(|| PathBuf::from(OsString::from_vec(shell.to_vec())))
}

/// The shell, started the way a terminal starts it.
///
/// Nothing to read from, because a shell that asks something has nobody to
/// answer it; and nowhere for its complaints to go -- an interactive shell
/// with no terminal says so, and so does a `.zshrc` that expected one.
///
/// And a session of its own, so that it has no terminal at all. `-i` turns
/// on job control, and an interactive shell finds its terminal through
/// `/dev/tty` whatever its standard input is: dash started in a background
/// group stops the whole group -- Obelus with it, before its clock can
/// run out -- and zsh hands the terminal's foreground to a group that is
/// about to be gone, taking it from the shell the reader typed into. That
/// is a window opened on another tree from one started in a terminal. A
/// group of its own is not enough; the terminal is the session's.
///
/// Not `-l` for the C shells, which refuse it beside anything else; `-i`
/// reads `.tcshrc`, which is where a C shell's reader sets a path.
fn asking(shell: &Path) -> Command {
    use std::os::unix::process::CommandExt as _;
    let c_shell = shell
        .file_name()
        .is_some_and(|name| name == "csh" || name == "tcsh");
    let mut command = Command::new(shell);
    command
        .args(match c_shell {
            true => &["-i", "-c"][..],
            false => &["-l", "-i", "-c"][..],
        })
        .arg(format!(
            "printf '%s' '{MARK}'; /usr/bin/env -0; printf '%s' '{ENDS}'"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Safety: `setsid` is async-signal-safe, and it is the only thing done
    // between the fork and the exec.
    unsafe {
        command.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
    command
}

fn asked(mut command: Command, within: Duration) -> Result<Vec<(OsString, OsString)>, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("the shell did not start: {error}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    // A thread of its own, because a read does not stop for a clock. It may
    // outlive this function, blocked on a pipe something in the background
    // still holds; it touches nothing but the pipe, so it is no reason not
    // to change the environment.
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut printed = Vec::new();
        let mut more = [0; 8192];
        loop {
            match stdout.read(&mut more) {
                Ok(0) | Err(_) => break,
                Ok(got) => printed.extend_from_slice(&more[..got]),
            }
            if read(&printed).is_some() {
                break;
            }
        }
        let _ = said.send(printed);
    });
    let Ok(printed) = heard.recv_timeout(within) else {
        // The group, which is the session the shell leads: whatever it was
        // in the middle of starting goes with it.
        // Safety: it takes a number and only sends a signal.
        if let Ok(group) = i32::try_from(child.id()) {
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
        let _ = child.wait();
        return Err(format!("it had not answered after {within:?}"));
    };
    // The shell alone, and before it reads its logout files, which are for
    // a reader leaving a terminal. What its files started in the background
    // was meant to outlive it.
    let _ = child.kill();
    let _ = child.wait();
    read(&printed).ok_or_else(|| "it printed no environment".to_string())
}

/// The environment in what the shell printed, once all of it is there.
fn read(printed: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let starts = find(printed, MARK)? + MARK.len();
    let ends = starts + find(&printed[starts..], ENDS)?;
    let found = printed[starts..ends]
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

fn find(printed: &[u8], mark: &str) -> Option<usize> {
    printed
        .windows(mark.len())
        .position(|window| window == mark.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    use super::{ENDS, MARK, asked, asking, read, the_accounts_shell};

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
        // Said here rather than inherited, because dash keeps no count of
        // its own and a test started from nothing would have none to drop.
        shell.env("HOME", &home.0).env("SHLVL", "7");
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

    /// The shell leads a group of its own, and not this one.
    ///
    /// Half of what is asked of it: what matters is that it has no
    /// terminal, and neither a test run in CI nor one run here has a
    /// terminal to take away -- that half needs a pty. What can be seen
    /// without one is that the shell is not in its caller's group, which a
    /// session of its own implies.
    ///
    /// Broken deliberately by taking the `setsid` out of `asking`: the
    /// shell is in the test's group.
    #[test]
    fn the_shell_is_in_a_session_of_its_own() {
        let home = Home::with_a_profile(
            "session",
            "export OBELUS_THE_GROUP=\"$(ps -o pgid= -p $$ | tr -d ' ')\"\nexport \
             OBELUS_THE_SHELL=$$\n",
        );
        let mut shell = asking(Path::new("/bin/sh"));
        shell.env("HOME", &home.0);
        let found = asked(shell, Duration::from_secs(30)).expect("an environment");
        let group = value(&found, "OBELUS_THE_GROUP").expect("the shell's group");
        assert_eq!(
            Some(&group),
            value(&found, "OBELUS_THE_SHELL").as_ref(),
            "the shell does not lead a group"
        );
        // Safety: it takes nothing and only answers.
        let ours = unsafe { libc::getpgrp() };
        assert_ne!(group, ours.to_string(), "the shell is in the test's group");
    }

    /// A shell that does not finish is let go, and the start goes on -- and
    /// what it was waiting on goes with it, rather than sleeping on behind a
    /// window that has long since opened.
    ///
    /// Broken deliberately two ways. Waiting on `recv` with no clock: the
    /// test takes as long as the profile sleeps, and fails on the time. And
    /// killing the shell alone rather than its group: the `sleep` is still
    /// there.
    #[test]
    fn a_shell_that_does_not_finish_is_let_go() {
        let home = Home::with_a_profile("slow", "sleep 30 &\necho $! > \"$HOME/waiting\"\nwait\n");
        let mut shell = asking(Path::new("/bin/sh"));
        shell.env("HOME", &home.0);
        let started = Instant::now();
        assert!(asked(shell, Duration::from_millis(500)).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the shell was waited on for {:?}",
            started.elapsed()
        );
        let waiting: i32 = std::fs::read_to_string(home.0.join("waiting"))
            .expect("what the shell was waiting on")
            .trim()
            .parse()
            .expect("a process");
        // A moment for the kernel to reap it: it was the shell's, and the
        // shell has gone, so it is init's to wait on now.
        let gone = (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            // Safety: signal 0 sends nothing, and only asks.
            unsafe { libc::kill(waiting, 0) != 0 }
        });
        assert!(gone, "what the shell was waiting on outlived it");
    }

    /// A C shell is asked in words it takes.
    ///
    /// Only where there is one: macOS has `tcsh` in `/bin`, and a Linux
    /// machine need not.
    ///
    /// Broken deliberately by asking it with `-l` as well: tcsh refuses the
    /// option and prints nothing.
    #[test]
    fn a_c_shell_is_asked_in_words_it_takes() {
        let tcsh = Path::new("/bin/tcsh");
        if !tcsh.exists() {
            return;
        }
        let home = Home::with_a_profile("tcsh", "");
        std::fs::write(
            home.0.join(".tcshrc"),
            "setenv OBELUS_FROM_THE_TCSHRC yes\n",
        )
        .expect("a tcshrc");
        let mut shell = asking(tcsh);
        shell.env("HOME", &home.0);
        let found = asked(shell, Duration::from_secs(30)).expect("an environment");
        assert_eq!(
            value(&found, "OBELUS_FROM_THE_TCSHRC").as_deref(),
            Some("yes")
        );
    }

    /// The account names a shell, which is where one is found when nothing
    /// else says.
    ///
    /// Broken deliberately by asking for the account of a user nobody is
    /// (`uid_t::MAX`): nothing is found.
    #[test]
    fn the_account_names_a_shell() {
        let shell = the_accounts_shell().expect("the account's shell");
        assert!(shell.is_absolute(), "{}", shell.display());
    }

    /// A shell whose files start something in the background has answered
    /// when it has printed the environment, not when the pipe closes -- the
    /// pipe is lent to what it started, and closes when that ends.
    ///
    /// Broken deliberately by reading until the pipe closes: the clock runs
    /// out on a shell that answered at once, and nothing is taken.
    #[test]
    fn a_shell_has_answered_when_it_has_said_so() {
        let home = Home::with_a_profile("background", "sleep 30 &\n");
        let mut shell = asking(Path::new("/bin/sh"));
        shell.env("HOME", &home.0);
        let started = Instant::now();
        let found = asked(shell, Duration::from_secs(10));
        assert!(found.is_ok(), "{found:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the shell was waited on for {:?}",
            started.elapsed()
        );
    }

    /// What a shell's files print around the environment is not part of it.
    ///
    /// Broken deliberately by reading from the start rather than from the
    /// mark: the greeting becomes the name of the first variable. And by
    /// reading to the end rather than to the second mark: the farewell
    /// becomes a variable.
    #[test]
    fn what_is_printed_around_the_environment_is_not_in_it() {
        let printed = format!(
            "Welcome=to the shell\n{MARK}PATH=/opt/homebrew/bin:/usr/bin\0EMPTY=\0SHLVL=2\0{ENDS}Goodbye=from .zlogout\0"
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
        // Not yet: what has arrived is not all of it until the second mark.
        assert_eq!(read(format!("{MARK}PATH=/usr/bin\0").as_bytes()), None);
    }
}
