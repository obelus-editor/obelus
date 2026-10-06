//! Logging, which must never reach stdout.
//!
//! stdout is the drawing surface. A stray line written to it lands in the
//! middle of the rendered frame and stays there, so the subscriber writes to a
//! file and failing to open that file is not allowed to stop Obelus starting.
//!
//! Two logs, split by module. `obelus.log` is what Obelus says about itself
//! and `lsp.log` is what the language servers say -- a handshake, every
//! request, and whatever they write to their stderr, at a volume that would
//! bury the dozen lines Obelus has of its own. `logging::is_server` decides by
//! the event's target, which `tracing` takes from the module it came from, so a
//! call site needs to know nothing and a module moved into `lsp` takes its
//! lines with it. `open-log` and `open-server-log` open them; both are ordinary
//! buffers, like anything else Obelus opens.
//!
//! The default filter names `ob` as well as `obelus`: the binary is its own
//! crate, so everything `main` logged -- what started, and that it left -- was
//! filtered out of its own log until it was added.
//!
//! A panic goes in the log (`logging::catch_panics`, chained like every
//! other hook). It is the one thing a log has to have and the one thing it had
//! none of: the message goes to stderr, which is behind the alternate screen,
//! so the log simply stopped mid-session with no reason in it. It earned its
//! keep immediately -- two real crashes on absurd terminal sizes, both fixed in
//! the same slice as the line that found them.

use std::path::{Path, PathBuf};

use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

/// What Obelus writes about itself.
pub const OBELUS: &str = "obelus";

/// What the language servers say, and what Obelus said to them.
///
/// Its own file because it is somebody else's program talking: a handshake,
/// a stream of requests and whatever the server writes to its stderr, at a
/// volume that would bury the dozen lines Obelus has to say about itself.
/// Two files, two commands, and each of them is readable.
pub const SERVERS: &str = "lsp";

/// Whether an event belongs to the servers' log rather than Obelus's own.
///
/// By the module it came from, which `tracing` uses as an event's target
/// unless one is given -- so nothing at the call sites has to know which
/// file it is writing to, and a module moved into `lsp` moves its lines
/// with it.
#[must_use]
pub fn is_server(target: &str) -> bool {
    // Two spellings because a module path names the crate first, and the
    // reading of a language server is its own crate: `obelus::lsp::client`
    // while Obelus was one crate, `obelus_lsp::client` now that it is
    // several. Matching one of them and not the other is silent -- the
    // servers' log is simply empty, and the handshake it should have held
    // is in Obelus's own log instead.
    ["obelus::lsp", "obelus_lsp"].iter().any(|name| {
        target == *name
            || target
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with("::"))
    })
}

/// The crates Obelus is, as `tracing` spells them.
///
/// A target is a module path and its first segment is the crate, and a
/// filter directive matches whole segments -- so `obelus` does not cover
/// `obelus_lsp`, and a crate missing from this list writes nothing above
/// `warn`. That is the failure this list exists to prevent, and it is one
/// nothing reports: the log is not empty, it is just missing the half of
/// Obelus that was left out of it.
///
/// `ob` is the binary rather than a library: the lines main writes -- what
/// started, and that it left -- carry the target of the crate the `ob`
/// target is compiled as.
pub const OURS: &[&str] = &[
    "ob",
    "obelus_agent",
    "obelus_app",
    "obelus_buffer",
    "obelus_clipboard",
    "obelus_command",
    "obelus_component",
    "obelus_config",
    "obelus_editing",
    "obelus_git",
    "obelus_icons",
    "obelus_logging",
    "obelus_lsp",
    "obelus_mcp",
    "obelus_program",
    "obelus_reading",
    "obelus_runtime",
    "obelus_search",
    "obelus_sink",
    "obelus_syntax",
    "obelus_text",
    "obelus_theme",
    "obelus_watch",
];

/// What is logged when `RUST_LOG` says nothing.
///
/// `tokei=off` because counting a tree warns once per file whose extension
/// it does not know -- `Cargo.lock` alone does it on this repository --
/// and that is a fact about the tree rather than anything Obelus has to
/// say about itself. A line per unrecognised file would bury the dozen
/// Obelus writes, which is the thing this log is for.
fn ours_at_info() -> EnvFilter {
    let mut filter = EnvFilter::new("warn,tokei=off");
    for name in OURS {
        filter = filter.add_directive(
            format!("{name}=info")
                .parse()
                .expect("a crate name and a level are a directive"),
        );
    }
    filter
}

/// Installs the file subscriber and returns its flush guards.
///
/// The guards must be held for as long as logging is wanted: dropping them
/// flushes and shuts down the writer threads.
///
/// Verbosity comes from `RUST_LOG`, which is `tracing-subscriber`'s own
/// convention rather than a setting Obelus invents.
///
/// `on_stderr` is for `ob --headless`, where nothing is drawn and the log
/// is the only thing anybody watching it will read: Obelus's own lines go
/// to stderr as well, through the same filter, so `RUST_LOG` moves both.
/// Not the servers', which are the volume the second file is there to keep
/// out of the first. And still there where the files cannot be made, which
/// is when a process nobody can see most needs to say something.
#[must_use]
pub fn install(on_stderr: bool) -> Option<(WorkerGuard, WorkerGuard)> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| ours_at_info());

    let mut layers = Vec::new();
    let files = files();
    let guards = files.map(|(ours, theirs, guards)| {
        // Two layers over one registry, each taking the events the other
        // does not: the split is by target, so an event goes to exactly one
        // file and neither file has to be read with the other in mind.
        //
        // Boxed into a list because a per-layer filter fixes the subscriber
        // it belongs to at the moment it is built, so two of them cannot be
        // stacked one after the other -- they go on together or not at all.
        layers.push(
            tracing_subscriber::fmt::layer()
                // No escape sequences: this is a file, and a pager should
                // not have to strip colour out of it.
                .with_ansi(false)
                .with_writer(Marked(ours))
                .with_filter(tracing_subscriber::filter::filter_fn(|event| {
                    !is_server(event.target())
                }))
                .boxed(),
        );
        layers.push(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(Marked(theirs))
                .with_filter(tracing_subscriber::filter::filter_fn(|event| {
                    is_server(event.target())
                }))
                .boxed(),
        );
        guards
    });
    if on_stderr {
        // Unmarked: one process's stderr has nobody else's lines in it.
        layers.push(
            tracing_subscriber::fmt::layer()
                .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
                .with_writer(std::io::stderr)
                .with_filter(tracing_subscriber::filter::filter_fn(|event| {
                    !is_server(event.target())
                }))
                .boxed(),
        );
    }
    tracing_subscriber::registry()
        .with(layers)
        .with(filter)
        .init();

    guards
}

/// The two files, where they can be made.
fn files() -> Option<(
    tracing_appender::non_blocking::NonBlocking,
    tracing_appender::non_blocking::NonBlocking,
    (WorkerGuard, WorkerGuard),
)> {
    let directory = log_directory()?;
    std::fs::create_dir_all(&directory).ok()?;
    let (ours, kept) = writer(&directory, OBELUS)?;
    let (theirs, also_kept) = writer(&directory, SERVERS)?;
    Some((ours, theirs, (kept, also_kept)))
}

/// One day-rotated file, and the guard that flushes it.
fn writer(
    directory: &Path,
    prefix: &str,
) -> Option<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(prefix)
        .filename_suffix("log")
        .max_log_files(5)
        .build(directory)
        .ok()?;
    Some(tracing_appender::non_blocking(appender))
}

/// A writer that puts this process's number in front of every line.
///
/// Obelus does not split its own window -- the terminal does that -- so
/// several of them on one project is the ordinary way to work, and they
/// share one log. Two interleaved stories with nothing to tell them apart
/// are neither of them readable.
///
/// On the line rather than in the filename: one file still rotates as one
/// file, and a reader following what happened does not have to open one
/// per window and put them back in order by hand.
struct Marked<M>(M);

impl<'writer, M: tracing_subscriber::fmt::MakeWriter<'writer>>
    tracing_subscriber::fmt::MakeWriter<'writer> for Marked<M>
{
    type Writer = Whose<M::Writer>;

    fn make_writer(&'writer self) -> Self::Writer {
        Whose {
            inner: self.0.make_writer(),
            said: false,
        }
    }
}

/// One event's writer, which says whose it is before it says anything else.
struct Whose<W> {
    inner: W,
    said: bool,
}

impl<W: std::io::Write> std::io::Write for Whose<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        // Once per event: `make_writer` is called for each of them, so the
        // first write of each is the head of a line.
        if !self.said {
            self.said = true;
            write!(self.inner, "{} ", std::process::id())?;
        }
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Chains a panic hook that writes the panic to the log.
///
/// A panic is the one thing a log has to have and the one thing it had none
/// of: the message goes to stderr, which is behind the alternate screen
/// while Obelus is drawing, and the log simply stopped mid-session with no
/// reason in it. Chained, like every other hook here, so whatever was
/// already installed still runs.
pub fn catch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // The location rather than a backtrace: a backtrace needs symbols
        // and an environment variable, and the line that panicked is what
        // says where to look.
        match panic.location() {
            Some(where_it_was) => {
                tracing::error!(at = %where_it_was, "panicked: {}", panic);
            }
            None => tracing::error!("panicked: {}", panic),
        }
        previous(panic);
    }));
}

/// The log file being written now, if there is one.
///
/// Found by date on disk rather than by rebuilding the name the appender
/// chose: the rotation and the naming belong to `tracing-appender`, and a
/// second copy of its convention here would be wrong on the first day it
/// changed. Newest wins, which is today's file.
#[must_use]
pub fn current_file(prefix: &str) -> Option<PathBuf> {
    newest_log(&log_directory()?, prefix)
}

/// The newest log file in a directory.
///
/// Split out from [`current_file`] so it can be tested against a directory
/// that exists: the two rules in it -- which names count, and which of them
/// wins -- are both quiet when wrong, and a wrong answer here opens the wrong
/// file or none.
fn newest_log(directory: &Path, prefix: &str) -> Option<PathBuf> {
    let start = format!("{prefix}.");
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&start) && name.ends_with(".log")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Somewhere else to keep it, for the tests.
///
/// The notes and the table of which conversation is about which note are
/// both kept in here, so a suite that did not say otherwise would write
/// into the reader's own state directory and leave it there. Set once and
/// shared by every test in the binary: the environment would do it too, and
/// is what the first test that needed it reached for, but setting the
/// environment while other tests are running is the thing the language made
/// unsafe -- and this is wanted by tests that run at the same time.
#[doc(hidden)]
pub fn state_directory_for_test(directory: PathBuf) {
    let _ = ELSEWHERE.set(directory);
}

/// Where [`state_directory_for_test`] put it, if anywhere.
static ELSEWHERE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Where Obelus keeps what it has worked out and could work out again.
///
/// State rather than config or data, which is what the directory is for: a
/// reader who deleted all of this would lose nothing they wrote and nothing
/// they chose. The logs are here, and what Obelus remembers about the
/// conversations it has had.
///
/// `None` is a session that keeps none of it -- no log, and no conversation
/// to come back to -- which is what a Windows machine got, because the only
/// answers here were an XDG variable and `HOME`. Where `dirs` is asked at
/// all it is asked for the *local* data directory rather than the roaming
/// one the settings use: this is what Obelus worked out about this machine,
/// and following a reader to another machine is the one thing it must not
/// do.
#[must_use]
pub fn state_directory() -> Option<PathBuf> {
    // Before the environment, because a test that has said where its state
    // goes has said so about a machine whose `XDG_STATE_HOME` is the
    // reader's own.
    if let Some(elsewhere) = ELSEWHERE.get() {
        return Some(elsewhere.clone());
    }
    // Said by name, wherever it is said. A reader who sets this has told
    // every program they run where its state goes, and Obelus is one.
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("obelus"));
    }
    // `HOME` is not asked on Windows. It is set there by whichever
    // unix-shaped thing was installed last -- git, most often -- and that
    // is that installer talking, not the reader.
    if cfg!(windows) {
        return Some(dirs::data_local_dir()?.join("obelus"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/obelus"))
}

/// Where the log files go.
fn log_directory() -> Option<PathBuf> {
    state_directory()
}

#[cfg(test)]
mod tests {
    /// There is somewhere to keep what Obelus works out, and on Windows it
    /// is somewhere that platform keeps such things.
    ///
    /// Both halves, because the first one alone passes for the wrong
    /// reason. `None` is a session with no log and no conversation kept --
    /// which is what a Windows machine got -- and the log is the only thing
    /// a reader has to send back when Obelus does something they cannot
    /// describe. But `HOME` *is* set on the Windows machine this was
    /// written on, by git's installer, so asking only whether there is an
    /// answer would have found one there and none on a machine without git.
    ///
    /// Broken deliberately by reading `HOME` on every platform, which is
    /// what this did: the answer came back under a dot-directory in the
    /// reader's profile, which is not where anything on that platform
    /// looks.
    #[test]
    fn there_is_somewhere_to_keep_what_obelus_works_out() {
        let Some(kept_in) = super::state_directory() else {
            panic!("this machine has nowhere for a log, so there will not be one");
        };
        // Unless the reader has said where, in which case that is the
        // answer and there is nothing else to ask about it.
        if cfg!(windows) && std::env::var_os("XDG_STATE_HOME").is_none() {
            let local = dirs::data_local_dir().expect("somewhere of this machine's own");
            assert!(
                kept_in.starts_with(&local),
                "{} is not under {}",
                kept_in.display(),
                local.display()
            );
        }
    }

    /// Every line says which Obelus wrote it.
    ///
    /// Several of them share the log, because several of them on one
    /// project is the ordinary way to work. A line that does not say whose
    /// it is belongs to whichever story the reader guesses.
    #[test]
    fn every_line_says_whose_it_is() {
        use std::io::Write as _;

        use tracing_subscriber::fmt::MakeWriter as _;

        #[derive(Clone, Default)]
        struct Kept(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Kept {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("the lock").extend_from_slice(buffer);
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Kept {
            type Writer = Self;
            fn make_writer(&'writer self) -> Self::Writer {
                self.clone()
            }
        }

        let kept = Kept::default();
        let marked = super::Marked(kept.clone());
        // Two events, each written the way the subscriber writes one: a
        // fresh writer, then the pieces of the line.
        for words in ["starting\n", "leaving\n"] {
            let mut writer = marked.make_writer();
            write!(writer, "INFO ").expect("the level");
            write!(writer, "{words}").expect("the words");
        }

        let written = String::from_utf8(kept.0.lock().expect("the lock").clone()).expect("utf-8");
        let pid = std::process::id();
        assert_eq!(
            written,
            format!("{pid} INFO starting\n{pid} INFO leaving\n"),
            "a line does not say whose it is"
        );
    }

    /// Which file an event goes to, by the module it came from. The split
    /// is the whole point of having two, and it is silent when wrong: a
    /// server's stream in Obelus's own log buries it, and Obelus's lines in
    /// the servers' log are lost in it.
    #[test]
    fn the_servers_lines_are_told_apart_by_their_module() {
        assert!(super::is_server("obelus::lsp"));
        assert!(super::is_server("obelus::lsp::client"));
        assert!(super::is_server("obelus::lsp::outline"));
        assert!(!super::is_server("obelus::app"));
        assert!(!super::is_server("obelus::app::semantics"));
        assert!(!super::is_server("obelus"));
        // A module whose name starts the same way and is not it.
        assert!(!super::is_server("obelus::lspish"));
        // The same question once Obelus is a workspace and the crate is
        // named in the target rather than a module of one crate.
        assert!(super::is_server("obelus_lsp"));
        assert!(super::is_server("obelus_lsp::client"));
        assert!(!super::is_server("obelus_lspish"));
        assert!(!super::is_server("obelus_app::semantics"));
    }

    /// The list every crate of the workspace has to be on, and the filter
    /// built from it. A crate left off logs nothing above `warn` and says
    /// nothing about having been left off, so what is pinned here is the
    /// spelling: `tracing` sees the name a module path uses, which is the
    /// one with underscores, and a directive written with the hyphen of the
    /// package name would silently match nothing.
    #[test]
    fn every_crate_of_obelus_is_named_the_way_a_target_is() {
        for name in super::OURS {
            assert!(
                !name.contains('-'),
                "{name} is a package name; a target uses underscores"
            );
        }
        let _ = super::ours_at_info();
    }

    use std::{fs, time::Duration};

    use super::*;

    /// The appender's own naming decides which files are logs, and the newest
    /// is the one being written now. Both are read off the directory rather
    /// than rebuilt here, so both are worth pinning down -- and so is which
    /// of the two logs a name belongs to, because they share a directory.
    #[test]
    fn the_newest_log_wins_and_only_logs_count() {
        let directory =
            std::env::temp_dir().join(format!("obelus-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("a directory to test in");

        fs::write(directory.join("obelus.2026-01-01.log"), "old").expect("the old log");
        // The servers' log shares the directory, and is not Obelus's own.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("lsp.2026-01-03.log"), "theirs").expect("the server log");
        // Written second, so it is the newer of the two whatever the clock
        // does with the names.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.2026-01-02.log"), "new").expect("the new log");
        // Neither of these is a log, however recently it was written.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.log.swp"), "not a log").expect("a decoy");
        fs::write(directory.join("notes.log"), "not ours").expect("another decoy");

        assert_eq!(
            newest_log(&directory, OBELUS),
            Some(directory.join("obelus.2026-01-02.log")),
            "the servers' log, or the older one, was taken for Obelus's own"
        );
        assert_eq!(
            newest_log(&directory, SERVERS),
            Some(directory.join("lsp.2026-01-03.log"))
        );

        let empty = directory.join("empty");
        fs::create_dir_all(&empty).expect("an empty directory");
        assert_eq!(newest_log(&empty, OBELUS), None);
        assert_eq!(newest_log(&directory.join("gone"), OBELUS), None);

        let _ = fs::remove_dir_all(&directory);
    }
}
