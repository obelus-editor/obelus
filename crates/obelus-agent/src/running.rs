//! Commands an agent asked to run, while they run.
//!
//! The protocol calls these terminals, which overstates them: there is no
//! `terminal/write`, no size, no keys. What the five `terminal/*` methods
//! ask for is a command started, its output read, its exit status waited
//! for, and a way to stop it. So this is a process runner with the output
//! kept, not a terminal -- Obelus has no terminal to offer and does not
//! need one.
//!
//! Its pipes are read on [`obelus_runtime`], not on threads of their own:
//! reading a pipe is waiting, which is what that runtime is for, and a
//! reader may have several commands going at once.
//!
//! Obelus does not ask the reader before running one. The agent asks --
//! that is what `session/request_permission` is for, and a client asking
//! again is a second question about one thing. What Obelus owes instead is
//! that the command is *on the page*, in the words it was actually run in,
//! and that a key stops it: what cannot be undone has to be visible while
//! it happens.

use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
};

use tokio::{
    io::AsyncReadExt as _,
    process::{Child, ChildStderr, ChildStdout, Command},
};

/// What the agent calls one of these.
pub type RunId = String;

/// How a command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ended {
    /// What it exited with, where it exited rather than was killed.
    pub code: Option<u32>,
    /// The signal that stopped it, where one did.
    pub signal: Option<&'static str>,
}

/// One command, running or finished.
#[derive(Debug)]
struct Running {
    /// The command as it was run, for the row that says what is happening.
    said: String,
    /// The process, until it is reaped.
    child: Option<Child>,
    /// Everything the process has started, for stopping all of it.
    group: Group,
    /// What it has written, both streams in the order they arrived.
    ///
    /// One buffer rather than two: what a reader wants to see is what the
    /// command printed, and a command that writes its progress to stderr
    /// and its answer to stdout is telling one story.
    said_so_far: Arc<Mutex<Output>>,
    /// How it ended, once it has.
    ended: Option<Ended>,
}

/// What a command has written, and whether that is all of it.
#[derive(Debug, Default)]
struct Output {
    /// The bytes kept.
    text: String,
    /// Whether anything was dropped for being past the limit.
    truncated: bool,
    /// How much may be kept. `None` is everything the command writes,
    /// which is what the protocol means by leaving the limit out.
    limit: Option<usize>,
    /// How many of its streams are still being read.
    open: usize,
}

impl Output {
    /// Takes what a stream wrote, up to the limit.
    ///
    /// The *first* bytes are kept rather than the last. A command that
    /// fails says why at the top -- the compiler error, the stack trace --
    /// and the ten thousand lines after it are the same news repeated.
    fn take(&mut self, more: &str) {
        let Some(limit) = self.limit else {
            self.text.push_str(more);
            return;
        };
        let room = limit.saturating_sub(self.text.len());
        if room == 0 {
            self.truncated = !more.is_empty() || self.truncated;
            return;
        }
        if more.len() <= room {
            self.text.push_str(more);
            return;
        }
        // On a character, not in the middle of one: the text is written to
        // a screen, and half a character is a glyph nobody can draw.
        let mut cut = room;
        while cut > 0 && !more.is_char_boundary(cut) {
            cut -= 1;
        }
        self.text.push_str(&more[..cut]);
        self.truncated = true;
    }
}

/// Every command this Obelus has running, and the ones that have finished
/// and not been let go of.
///
/// Kept until the agent says `terminal/release`: it may ask for the output
/// after the command has exited, and a client that forgot on exit would
/// answer about a command nobody can see any more.
#[derive(Debug, Default)]
pub struct Runs {
    running: HashMap<RunId, Running>,
    /// What the next one is called. The agent's own ids name *its*
    /// terminals; these are Obelus's, and the protocol says the client
    /// mints them.
    next: u64,
}

impl Runs {
    /// Starts one, and says what it is called.
    ///
    /// Through a shell, because what an agent sends is a command line: it
    /// writes `cargo test 2>&1 | tail`, and handing that to `execvp` looks
    /// for a program with spaces and pipes in its name.
    ///
    /// # Errors
    ///
    /// When the shell will not start, which is the only failure there is:
    /// a command that does not exist fails *inside* the shell and is an
    /// exit status like any other.
    pub fn start(
        &mut self,
        command: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: Option<&Path>,
        root: &Path,
        limit: Option<usize>,
    ) -> std::io::Result<RunId> {
        let said = match args.is_empty() {
            true => command.to_string(),
            false => format!("{command} {}", args.join(" ")),
        };
        // Inside the runtime, because a child's pipes register with it --
        // the same reason a language server is started this way.
        let _inside = obelus_runtime::handle().enter();
        let (shell, said_with) = shell();
        let mut process = Command::new(shell);
        process
            .arg(said_with)
            .arg(&said)
            .current_dir(cwd.unwrap_or(root))
            // Nothing to type into. The protocol has no way to send a
            // key to one of these, and Obelus's own input is the reader's
            // terminal in raw mode -- inherited, the command would be
            // taking their keystrokes out of the page they are reading.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            process.env(name, value);
        }
        // And nothing that stops to be read. A pager holds the command
        // open on a keystroke that cannot arrive, so `git log` would hang
        // for ever rather than print.
        process.env("PAGER", "");
        process.env("GIT_PAGER", "cat");
        // Nor anything that draws to a terminal it does not have.
        process.env("TERM", "dumb");
        process.env("NO_COLOR", "1");
        // A group of its own, so that a stop reaches what the shell started
        // as well as the shell -- see `Group`.
        #[cfg(unix)]
        process.process_group(0);

        let mut child = process.spawn()?;
        let group = Group::of(&child);
        let streams: Vec<Stream> = [
            child.stdout.take().map(Stream::Out),
            child.stderr.take().map(Stream::Err),
        ]
        .into_iter()
        .flatten()
        .collect();
        let output = Arc::new(Mutex::new(Output {
            limit,
            open: streams.len(),
            ..Output::default()
        }));
        for stream in streams {
            read_into(stream, Arc::clone(&output));
        }

        self.next += 1;
        let id = format!("run-{}", self.next);
        self.running.insert(
            id.clone(),
            Running {
                said,
                child: Some(child),
                group,
                said_so_far: output,
                ended: None,
            },
        );
        Ok(id)
    }

    /// The command one of them was run with, for the row that shows it.
    #[must_use]
    pub fn said(&self, id: &str) -> Option<&str> {
        Some(self.running.get(id)?.said.as_str())
    }

    /// What it has written so far, whether that is all of it, and how it
    /// ended if it has.
    #[must_use]
    pub fn output(&mut self, id: &str) -> Option<(String, bool, Option<Ended>)> {
        let ended = self.ended(id);
        let run = self.running.get(id)?;
        let said = run.said_so_far.lock().ok()?;
        Some((said.text.clone(), said.truncated, ended))
    }

    /// How it ended, or nothing while it is still running.
    ///
    /// Asked of the operating system rather than remembered, the way the
    /// language servers are: a process that ended has to be reaped by
    /// somebody, and the somebody is whoever asks first.
    ///
    /// And not before everything it wrote has been read, which is a later
    /// moment than the exit: the streams are read on the runtime, and the
    /// last of them can still be in the pipe when the process is gone. An
    /// agent told a command had ended asked for its output next and got
    /// the front of it -- or, on a slow machine, none. Whatever the process
    /// started and left holding the pipe keeps it running too, which is
    /// what `Command::output` means by a command finishing as well.
    pub fn ended(&mut self, id: &str) -> Option<Ended> {
        let run = self.running.get_mut(id)?;
        if let Some(ended) = run.ended {
            return Some(ended);
        }
        if run.said_so_far.lock().ok()?.open > 0 {
            return None;
        }
        let child = run.child.as_mut()?;
        let status = child.try_wait().ok()??;
        run.child = None;
        run.group.reaped();
        let ended = ended_as(&status);
        run.ended = Some(ended);
        Some(ended)
    }

    /// Stops one, whether the reader asked or the agent did.
    ///
    /// Two halves, because they answer to different things. The signal is
    /// sent *here*, synchronously: Obelus may be on its way out, and a
    /// kill queued behind a runtime nobody polls again is a process that
    /// outlives the reader. The reaping is a task, because waiting for a
    /// process to die is waiting -- and a killed child nobody waits for is
    /// a zombie.
    ///
    /// How it ended is written down rather than read back. Obelus killed
    /// it, so `KILL` is the truth whatever the wait would later say, and
    /// the answer is owed to whoever asked now rather than a frame later.
    ///
    /// The group before the shell, and only while the shell is unreaped:
    /// its number is the group's, and a number nobody holds any more is one
    /// the system may give to somebody else's process.
    pub fn stop(&mut self, id: &str) {
        let Some(run) = self.running.get_mut(id) else {
            return;
        };
        let Some(mut child) = run.child.take() else {
            return;
        };
        run.group.stop();
        let _ = child.start_kill();
        run.ended = Some(Ended {
            code: None,
            signal: Some("KILL"),
        });
        obelus_runtime::handle().spawn(async move {
            let _ = child.wait().await;
        });
    }

    /// Lets go of one: it is stopped if it is still going, and forgotten.
    pub fn release(&mut self, id: &str) {
        self.stop(id);
        self.running.remove(id);
    }

    /// Stops and forgets every one of them.
    fn release_all(&mut self) {
        let ids: Vec<RunId> = self.running.keys().cloned().collect();
        for id in ids {
            self.release(&id);
        }
    }

    /// Whether any of them is still going, for the row that says so.
    #[must_use]
    pub fn anything_running(&self) -> bool {
        self.running.values().any(|run| run.child.is_some())
    }
}

impl Drop for Runs {
    /// Stops everything still running when these go.
    ///
    /// `Child` does not, deliberately: a child outliving its parent is the
    /// usual thing to want, which is what every daemon started from a
    /// shell depends on. It is not the thing to want here, for the reason
    /// `obelus_lsp::Client` gives in its own words -- and more sharply, because
    /// these are commands a reader never typed. A `cargo build` left
    /// running after Obelus is gone is a process eating a machine on
    /// behalf of a conversation nobody can see any more.
    fn drop(&mut self) {
        self.release_all();
    }
}

/// What a command started, held so that stopping it stops all of it.
///
/// The shell is the one process Obelus has a handle on, and an agent's
/// command line is seldom one program: `cargo build && cargo test` is a
/// shell waiting on a cargo that is running rustcs, and a kill sent to the
/// shell is not passed on to any of them. So a stop that killed only the
/// shell left the build the reader had just stopped running, with nothing
/// on screen that could stop it.
///
/// Two ways of saying "everything under this", and neither platform is made
/// to carry the other's. On Unix the shell leads a process group of its own
/// and the group is killed; what leaves the group on purpose -- a daemon
/// calling `setsid` -- has said it is not part of the command. On Windows
/// the shell goes into a job, which whatever it starts is in as well. It is
/// put there just after it starts rather than before, because the standard
/// library will not start a process suspended and hand back its thread, so
/// a command fast enough to start something in that moment would leave it
/// out: `cmd` takes a good deal longer than that to read its own line.
///
/// A group of its own is also a group the terminal does not hang up on.
/// When the reader closes the terminal `ob` is in, the hangup goes to `ob`'s
/// group, and that used to take the commands with it because they were in
/// it. So `ob` passes it on (`stop_on_hangup`). Windows has no such thing
/// to lose: a console being closed is told to everything attached to it,
/// and a job does not detach anything.
#[derive(Debug)]
struct Group {
    /// The group's number, which is the shell's, until the shell is reaped
    /// and the number is the system's to give to somebody else.
    #[cfg(unix)]
    leader: Option<libc::pid_t>,
    /// The job, closed when this goes. `None` where the system would not
    /// make one, and then the shell alone is what a stop reaches.
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
}

impl Group {
    /// The group a process that has just started leads.
    #[cfg(unix)]
    fn of(child: &Child) -> Self {
        let leader = child.id().and_then(|it| libc::pid_t::try_from(it).ok());
        if let Some(leader) = leader
            && let Ok(mut leaders) = LEADERS.lock()
        {
            leaders.push(leader);
            stop_on_hangup();
        }
        Self { leader }
    }

    /// A job, with a process that has just started in it.
    #[cfg(windows)]
    fn of(child: &Child) -> Self {
        use std::os::windows::io::{FromRawHandle as _, OwnedHandle};

        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW},
        };

        let Some(process) = child.raw_handle() else {
            return Self { job: None };
        };
        // Safety: both handles are live for the length of the calls -- the
        // job because it was just made and is closed only on the failure
        // path, the process because `child` holds it -- and no pointer is
        // read through but the two nulls, which mean "the defaults".
        let job = unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                None
            } else if AssignProcessToJobObject(job, process) == 0 {
                CloseHandle(job);
                None
            } else {
                Some(OwnedHandle::from_raw_handle(job))
            }
        };
        Self { job }
    }

    /// Kills everything in it. What has gone already is not an error.
    #[cfg(unix)]
    fn stop(&mut self) {
        let Some(leader) = self.leader else {
            return;
        };
        // Safety: `killpg` takes two numbers and reads nothing through a
        // pointer.
        unsafe {
            libc::killpg(leader, libc::SIGKILL);
        }
        self.reaped();
    }

    /// Lets go of the number, because the shell that held it is gone.
    #[cfg(unix)]
    fn reaped(&mut self) {
        let Some(leader) = self.leader.take() else {
            return;
        };
        if let Ok(mut leaders) = LEADERS.lock() {
            leaders.retain(|it| *it != leader);
        }
    }

    /// Kills everything in it. What has gone already is not an error.
    #[cfg(windows)]
    fn stop(&mut self) {
        use std::os::windows::io::AsRawHandle as _;

        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let Some(job) = &self.job else {
            return;
        };
        // Safety: the handle is the job's and outlives the call.
        unsafe {
            TerminateJobObject(job.as_raw_handle(), 1);
        }
    }

    /// Nothing to let go of: a job is not a number anybody else is given.
    #[cfg(windows)]
    fn reaped(&mut self) {}
}

/// Every group this process has running, for a hangup to stop.
///
/// The whole process's rather than one `Runs`'s, because a signal is the
/// whole process's.
#[cfg(unix)]
static LEADERS: Mutex<Vec<libc::pid_t>> = Mutex::new(Vec::new());

/// Kills every command still running when the terminal hangs up, and then
/// goes the way the hangup would have taken it.
///
/// Gone the same way so that nothing else about a hangup changes: whoever
/// started `ob` sees it end on `HUP` as it always did, and the language
/// servers and the agent, which are still in `ob`'s group, were hung up on
/// with it already. `Drop` on the runs is not reached either way, which is
/// why this is a list of numbers rather than a walk of them.
///
/// Listened for from the first command on, not from the start: an `ob`
/// that never runs one keeps the disposition it was given.
#[cfg(unix)]
fn stop_on_hangup() {
    use tokio::signal::unix::{SignalKind, signal};

    static LISTENING: std::sync::Once = std::sync::Once::new();
    LISTENING.call_once(|| {
        let _inside = obelus_runtime::handle().enter();
        let Ok(mut hangup) = signal(SignalKind::hangup()) else {
            return;
        };
        obelus_runtime::handle().spawn(async move {
            hangup.recv().await;
            let leaders = LEADERS.lock().map(|it| it.clone()).unwrap_or_default();
            // Safety: `killpg`, `signal` and `raise` take numbers and read
            // nothing through a pointer; `SIG_DFL` is a disposition, not an
            // address anything calls.
            unsafe {
                for leader in leaders {
                    libc::killpg(leader, libc::SIGKILL);
                }
                libc::signal(libc::SIGHUP, libc::SIG_DFL);
                libc::raise(libc::SIGHUP);
            }
        });
    });
}

/// Which end of a process a reader is on.
enum Stream {
    Out(ChildStdout),
    Err(ChildStderr),
}

/// Reads one of a process's streams into the output it shares.
///
/// A task on the one runtime, not a thread: reading a pipe is waiting, and
/// waiting is what that runtime is for. Two of these per command, and a
/// command is a thing a reader can start several of.
fn read_into(stream: Stream, into: Arc<Mutex<Output>>) {
    obelus_runtime::handle().spawn(async move {
        let mut reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = match stream {
            Stream::Out(out) => Box::new(out),
            Stream::Err(err) => Box::new(err),
        };
        let mut buffer = [0u8; 4096];
        loop {
            let read = match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => {
                    if let Ok(mut output) = into.lock() {
                        output.open -= 1;
                    }
                    return;
                }
                Ok(read) => read,
            };
            // Lossily, because a command's output is bytes and Obelus
            // draws characters: a program writing something that is not
            // UTF-8 is a program whose output is still worth showing.
            let more = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let Ok(mut output) = into.lock() else {
                return;
            };
            output.take(&more);
        }
    });
}

/// The shell to run a command line with, and the flag that hands it one.
///
/// The reader's own, because the command an agent writes is the command
/// they would have typed -- their aliases are not here, but their shell's
/// syntax is what the agent was writing in. `SHELL` is where that is said,
/// on Windows as well: nothing sets it there except a POSIX shell the
/// reader installed, and one they installed is one they meant to use.
///
/// The fallback is the part that has to differ. `/bin/sh` is not a file on
/// a Windows machine, so a run failed at the spawn -- before the command
/// was read, with nothing on the page about it -- where what that machine
/// has is named by `COMSPEC`, and calls `-c` `/C`.
fn shell() -> (String, &'static str) {
    if let Ok(named) = std::env::var("SHELL") {
        return (named, "-c");
    }
    match cfg!(windows) {
        true => (
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            "/C",
        ),
        false => ("/bin/sh".to_string(), "-c"),
    }
}

/// How a process ended, as the protocol says it.
fn ended_as(status: &std::process::ExitStatus) -> Ended {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return Ended {
                code: None,
                signal: Some(named(signal)),
            };
        }
    }
    Ended {
        code: status.code().and_then(|code| u32::try_from(code).ok()),
        signal: None,
    }
}

/// The name of a signal, for the few a command is stopped by.
///
/// Beside the one arm that calls it: a platform with no signals to report
/// has no use for their names, and an uncalled function there is a warning
/// in a build that is meant to have none.
#[cfg(unix)]
const fn named(signal: i32) -> &'static str {
    match signal {
        2 => "INT",
        9 => "KILL",
        15 => "TERM",
        _ => "signal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a command wrote, once it has finished writing it.
    fn finished(runs: &mut Runs, id: &str) -> (String, bool, Option<Ended>) {
        // No pause after the end for the last of the output to land: a
        // command has not ended until all of it has. The pause that was
        // here was twenty milliseconds, which a Windows runner outran.
        for _ in 0..200 {
            if runs.ended(id).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        runs.output(id).expect("the command")
    }

    /// Both streams, in one story.
    ///
    /// A command that writes its progress to stderr and its answer to
    /// stdout is saying one thing, and a reader looking for what happened
    /// reads it in the order it happened.
    ///
    /// Broken deliberately by piping stderr to `Stdio::null`: half of it
    /// goes missing and this goes red.
    #[test]
    fn what_a_command_writes_is_kept_whichever_stream_it_used() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                "echo out; echo err 1>&2",
                &[],
                &[],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        let (text, truncated, ended) = finished(&mut runs, &id);
        assert!(text.contains("out"), "stdout is missing: {text:?}");
        assert!(text.contains("err"), "stderr is missing: {text:?}");
        assert!(!truncated);
        assert_eq!(
            ended,
            Some(Ended {
                code: Some(0),
                signal: None
            })
        );
    }

    /// A command line is a command line, not a program name.
    ///
    /// An agent writes `cargo test 2>&1 | tail`, and handing that to
    /// `execvp` looks for a program with spaces and pipes in its name.
    ///
    /// Broken deliberately by running `command` itself with `args`: the
    /// pipe is a word rather than a pipe and this goes red.
    #[test]
    fn a_command_line_is_run_by_a_shell() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                "printf 'a\\nb\\nc\\n' | tail -1",
                &[],
                &[],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        let (text, _, ended) = finished(&mut runs, &id);
        assert_eq!(text.trim(), "c");
        assert_eq!(ended.and_then(|it| it.code), Some(0));
    }

    /// A command has not ended until what it wrote has been read.
    ///
    /// The process exiting is not that moment. Here the shell is gone at
    /// once and what it started holds the pipe a second longer -- which is
    /// the race a slow machine runs for every command, made certain: the
    /// end was reported while the last of the output was still in the pipe.
    ///
    /// Broken deliberately by asking only whether the process has exited
    /// in `ended`: the end arrives with nothing written.
    ///
    /// Not on Windows, where the shell is `cmd`: it has no way to leave a
    /// child holding the pipe, and reads the line as arguments to `sleep`.
    /// The race there is the one `a_command_line_is_run_by_a_shell` lost on
    /// a slow runner, and that test no longer waits on a clock to win it.
    #[cfg(unix)]
    #[test]
    fn a_command_has_not_ended_until_what_it_wrote_has_been_read() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                "(sleep 1; echo late) &",
                &[],
                &[],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        let (text, _, ended) = finished(&mut runs, &id);
        assert!(ended.is_some(), "it never ended");
        assert_eq!(text.trim(), "late");
    }

    /// The first of a long answer is kept, and it says it was cut.
    ///
    /// The first rather than the last: a command that fails says why at
    /// the top -- the compiler error, the stack trace -- and the ten
    /// thousand lines after it are the same news repeated.
    ///
    /// Broken deliberately by ignoring the limit: nothing is truncated and
    /// this goes red.
    #[test]
    fn a_long_answer_is_kept_from_the_top_and_says_it_was_cut() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                // Written in the shell that will run it -- see `shell`.
                // What is under test is not: the limit is kept in code
                // with no `cfg` in it, and this is only how a thousand
                // characters are asked for.
                match cfg!(windows) {
                    true => "for /L %i in (1,1,100) do @echo aaaaaaaaaa",
                    false => "printf 'aaaaaaaaaa%.0s' $(seq 1 100)",
                },
                &[],
                &[],
                None,
                std::path::Path::new("."),
                Some(64),
            )
            .expect("the shell");
        let (text, truncated, _) = finished(&mut runs, &id);
        assert_eq!(text.len(), 64, "the limit was not kept to: {text:?}");
        assert!(truncated, "nothing said it was cut");
        assert!(text.starts_with('a'));
    }

    /// A command that fails is an exit status, not an error.
    ///
    /// Only the shell failing to start is an error: a command that does
    /// not exist fails *inside* the shell, and the agent is owed that
    /// answer rather than a refusal.
    #[test]
    fn a_command_that_does_not_exist_is_an_exit_status() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                "this-command-does-not-exist",
                &[],
                &[],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        let (_, _, ended) = finished(&mut runs, &id);
        assert!(
            ended.is_some_and(|it| it.code.is_some_and(|code| code != 0)),
            "a missing command did not come back as a failure: {ended:?}"
        );
    }

    /// Nothing a command starts stops to be read.
    ///
    /// A pager holds the command open on a keystroke that cannot arrive --
    /// there is no `terminal/write` in the protocol -- so `git log` would
    /// hang for ever rather than print. Checked by reading the environment
    /// the command is actually given, because a test that ran a pager
    /// would be a test that hangs when it fails.
    ///
    /// Broken deliberately by leaving `PAGER` alone: the child sees the
    /// reader's own and this goes red.
    #[test]
    fn a_command_is_given_nothing_that_stops_to_be_read() {
        let mut runs = Runs::default();
        let id = runs
            .start(
                // The same again: the three are set in code with no `cfg`
                // in it, and this asks the shell to say them back. `^|`
                // is how cmd is told a bar is a character rather than a
                // pipe.
                match cfg!(windows) {
                    true => "echo %PAGER%^|%GIT_PAGER%^|%TERM%",
                    false => "printf '%s|%s|%s' \"$PAGER\" \"$GIT_PAGER\" \"$TERM\"",
                },
                &[],
                // Even where the reader's own environment has one.
                &[("PAGER".to_string(), "less".to_string())],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        let (text, _, _) = finished(&mut runs, &id);
        // `%PAGER%` unexpanded is cmd saying there is no such variable,
        // which is Windows keeping the same promise in its own terms: an
        // environment variable set to nothing there *is* an absent one,
        // and `Command::env(name, "")` leaves the child without it. The
        // other two are set to something and come back as it.
        assert_eq!(
            text.trim(),
            match cfg!(windows) {
                true => "%PAGER%|cat|dumb",
                false => "|cat|dumb",
            },
            "the command was given something that waits for a key"
        );
    }

    /// A command that waits a moment and then leaves a mark, written for
    /// whichever shell this machine runs commands in -- and how long a
    /// test may wait before looking for it.
    ///
    /// Two spellings, because no one line means the same thing to a
    /// POSIX shell and to `cmd`: `;` does not separate commands there,
    /// and `sleep` is not a program it has.
    ///
    /// **The mark is a directory and not a redirect into a file.** `cmd`
    /// creates a redirect's file while it is setting the command up --
    /// before the command runs at all, and before a kill can stop it --
    /// so `: > mark` had `cmd` making the very mark these tests were
    /// about to look for. They failed about half the time, on whether
    /// the kill landed before `cmd` got that far, which is a race about
    /// nothing: neither test was asking what its name says. A directory
    /// is made by a program, and killing the shell is what stops the
    /// program from being reached.
    ///
    /// The wait has to outlast the pause, or a kill that did not work
    /// would leave nothing to find and the test would pass for the
    /// wrong reason.
    fn a_mark_left_in_a_moment(mark: &std::path::Path) -> (String, std::time::Duration) {
        match shell().1 {
            // `cmd` has nothing that pauses for less than a second, so
            // both numbers are bigger here and the test is slower for
            // it. `&` is what separates two commands there.
            "/C" => (
                format!("ping -n 2 127.0.0.1 > nul & mkdir \"{}\"", mark.display()),
                std::time::Duration::from_millis(2_500),
            ),
            _ => (
                format!("sleep 0.4; mkdir \"{}\"", mark.display()),
                std::time::Duration::from_millis(900),
            ),
        }
    }

    /// Nothing is left running when the runs go.
    ///
    /// A command a reader never typed, still going after Obelus is gone,
    /// is a process eating a machine on behalf of a conversation nobody
    /// can see any more.
    ///
    /// Broken deliberately by taking the `Drop` off `Runs`: the sleep
    /// outlives them and this goes red.
    #[test]
    fn nothing_is_left_running_when_the_runs_go() {
        let mut runs = Runs::default();
        // Writes its own name where the test can see it, so that "still
        // running" is a question the test can ask the machine rather than
        // the thing that was just dropped.
        let mark = std::env::temp_dir().join(format!("obelus-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mark);
        let (line, wait) = a_mark_left_in_a_moment(&mark);
        runs.start(&line, &[], &[], None, std::path::Path::new("."), None)
            .expect("the shell");
        drop(runs);
        std::thread::sleep(wait);
        assert!(!mark.exists(), "a command outlived the runs it belonged to");
    }

    /// Stopping one stops it, and says how it stopped.
    ///
    /// Asked of the machine and not only of the books: Obelus writes down
    /// `KILL` the moment it sends the signal rather than waiting to be
    /// told, so a version that wrote it down and sent nothing would say
    /// the same thing while the command carried on.
    ///
    /// Broken deliberately by taking the `start_kill` out of `stop`: the
    /// command reaches its own end and leaves its mark.
    #[test]
    fn a_command_that_is_stopped_says_so() {
        let mut runs = Runs::default();
        let mark = std::env::temp_dir().join(format!("obelus-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mark);
        let (line, wait) = a_mark_left_in_a_moment(&mark);
        let id = runs
            .start(&line, &[], &[], None, std::path::Path::new("."), None)
            .expect("the shell");
        assert!(runs.anything_running());
        runs.stop(&id);
        assert_eq!(runs.ended(&id).and_then(|it| it.signal), Some("KILL"));
        assert!(!runs.anything_running());
        std::thread::sleep(wait);
        assert!(!mark.exists(), "a command that was stopped ran on");

        // And letting go of it forgets it, which is what the agent's
        // `terminal/release` means.
        runs.release(&id);
        assert!(runs.output(&id).is_none());
    }

    /// Stopping one stops what it started, not only the shell.
    ///
    /// The shell is the one process Obelus has a handle on, and an agent's
    /// command line is seldom one program: `cargo build && cargo test`
    /// is the shell waiting on a cargo, and that cargo is running rustcs.
    /// A kill sent to the shell alone is not passed on, so the build the
    /// reader just stopped carried on without anything on screen that
    /// could stop it. Here the program is a subshell's, which no shell
    /// can `exec` its way out of.
    ///
    /// Broken deliberately by taking the `group.stop()` out of `stop`: the
    /// shell dies, the subshell under it leaves its mark, and this goes red.
    #[test]
    fn a_command_that_is_stopped_takes_what_it_started_with_it() {
        let mut runs = Runs::default();
        let mark = std::env::temp_dir().join(format!("obelus-group-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mark);
        let (line, wait) = a_mark_left_in_a_moment(&mark);
        // One level down, in each shell's own spelling: a POSIX shell
        // forks for a parenthesis, and `cmd` is started again, with `^`
        // keeping the `&` and the `>` for the inner one to read. And it
        // says it has started, because a kill that lands before the shell
        // has got as far as starting it stops everything there is, and the
        // test passed that way with nothing but the shell being killed.
        let line = match shell().1 {
            "/C" => format!(
                "cmd /C echo started {}",
                format!("& {line}").replace('&', "^&").replace('>', "^>")
            ),
            _ => format!("(echo started; {line}); true"),
        };
        let id = runs
            .start(&line, &[], &[], None, std::path::Path::new("."), None)
            .expect("the shell");
        for _ in 0..200 {
            if runs
                .output(&id)
                .is_some_and(|(text, ..)| text.contains("started"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        runs.stop(&id);
        std::thread::sleep(wait);
        assert!(!mark.exists(), "what a stopped command started ran on");
    }

    /// Closing the terminal takes the commands with it.
    ///
    /// They lead groups of their own, so the hangup that goes to `ob`'s
    /// group does not reach them, and `ob` dies of it without running a
    /// `Drop`. Asked of a process of its own -- this test binary again,
    /// running the one below -- because a hangup is the whole process's,
    /// and it has to be seen to die of it.
    ///
    /// Broken deliberately by taking the `stop_on_hangup()` out of
    /// `Group::of`: the hangup kills the process and nothing else, the
    /// command leaves its mark, and this goes red. And by taking out the
    /// `raise`: the process outlives the hangup, and this goes red on how
    /// it ended.
    #[cfg(unix)]
    #[test]
    fn a_hangup_takes_the_commands_with_it() {
        use std::os::unix::process::ExitStatusExt as _;

        let mark = std::env::temp_dir().join(format!("obelus-hangup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mark);
        let status = std::process::Command::new(std::env::current_exe().expect("this test"))
            .args([
                "--exact",
                "running::tests::hang_up_on_a_running_command",
                "--ignored",
            ])
            .env("OBELUS_HANGUP_MARK", &mark)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("this test, again");
        assert_eq!(
            status.signal(),
            Some(libc::SIGHUP),
            "it did not die of the hangup"
        );
        std::thread::sleep(a_mark_left_in_a_moment(&mark).1);
        assert!(
            !mark.exists(),
            "a command outlived the terminal it was run from"
        );
    }

    /// The other half of the test above, which it runs in a process of its
    /// own. Ignored, because run anywhere else it hangs up on the test
    /// binary; and it does nothing without the mark it is given.
    #[cfg(unix)]
    #[test]
    #[ignore = "run by a_hangup_takes_the_commands_with_it, in a process of its own"]
    fn hang_up_on_a_running_command() {
        let Some(mark) = std::env::var_os("OBELUS_HANGUP_MARK") else {
            return;
        };
        let mut runs = Runs::default();
        let (line, wait) = a_mark_left_in_a_moment(std::path::Path::new(&mark));
        let id = runs
            .start(
                &format!("echo started; {line}"),
                &[],
                &[],
                None,
                std::path::Path::new("."),
                None,
            )
            .expect("the shell");
        for _ in 0..200 {
            if runs
                .output(&id)
                .is_some_and(|(text, ..)| text.contains("started"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Safety: `raise` takes a number.
        unsafe {
            libc::raise(libc::SIGHUP);
        }
        // Past the end of the command, so that a hangup nobody acted on
        // is one this process outlives -- and `forget` rather than letting
        // `runs` drop, which would stop the command and pass for the fix.
        std::thread::sleep(wait * 3);
        std::mem::forget(runs);
    }
}
