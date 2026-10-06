//! A program running in a terminal of Obelus's own.
//!
//! Not the commands an agent runs (`obelus_agent::running`), which want a
//! process and its output and nothing a screen gives: this is for a program
//! that wants to be *talked to* -- a shell the reader opened, an agent's own
//! sign-in, which asks which account and waits for a code to be pasted back.
//! So a pseudo-terminal, a size, keys, and a grid the program draws on.
//!
//! The bytes the program writes come to the loop as [`Heard`] on its one
//! channel and are parsed by whoever holds the [`Terminal`], which is the one
//! owner of the application's state: no lock between the thread reading the
//! pty and the frame drawing what it read. Reading, writing and waiting for
//! the end are each a blocking call, so each is a blocking task on
//! [`obelus_runtime`] -- and writing is one of its own for the reason a
//! language server's stdin is: a program that has stopped reading would stop
//! the key that wrote to it.
//!
//! The program is the reader's, not Obelus's. What it is asked -- where the
//! cursor is, what kind of terminal this is -- is answered, because a shell
//! that asks and hears nothing waits for an answer; everything else it
//! writes is drawn and nothing more is made of it.

mod keys;

use std::{
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::mpsc,
};

use crossterm::event::KeyEvent;
pub use keys::bytes_of;
use obelus_sink::Sink;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
/// The parser, whose screen [`Terminal::screen`] hands out: one version of
/// it, for whoever draws that screen.
pub use vt100;

/// Which terminal something is about.
///
/// The application's own count: a program's output arrives on the loop's
/// channel with nothing else to say whose it is.
pub type Id = u64;

/// How far back a terminal keeps what scrolled off its top.
///
/// A limit, which a list here is usually not allowed: a terminal is a
/// stream that does not end, and every terminal anybody has used keeps a
/// screenful of thousands and lets the rest go.
const KEPT: usize = 10_000;

/// What a terminal's workers have to tell the loop.
#[derive(Debug)]
pub enum Heard {
    /// The program wrote this.
    Wrote {
        /// Which terminal.
        id: Id,
        /// The bytes, as they came: half a character is the parser's to
        /// hold until the rest arrives.
        bytes: Vec<u8>,
    },
    /// The program has ended.
    ///
    /// From the process rather than from the pty closing, because the two
    /// are not the same moment: a program that leaves something running in
    /// the background ends with the pty still open, and on Windows the
    /// console stays open until it is let go of.
    Ended {
        /// Which terminal.
        id: Id,
        /// How.
        ended: Ended,
    },
}

/// How a program ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    /// What it exited with.
    pub code: u32,
    /// The signal that stopped it, where one did.
    pub signal: Option<String>,
}

impl Ended {
    /// Whether it says it did what it was for.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

/// What to run.
#[derive(Clone, Debug)]
pub enum Program {
    /// The reader's own shell, the one their terminal would start.
    Shell,
    /// This, with these.
    Command {
        /// The program.
        program: PathBuf,
        /// What it is told.
        arguments: Vec<String>,
        /// What it is given on top of Obelus's own environment.
        env: Vec<(String, String)>,
    },
}

/// A program, the pty it is on, and what it has drawn.
pub struct Terminal {
    id: Id,
    /// What the program has drawn, parsed.
    parser: vt100::Parser<Answers>,
    /// Where keys go, by way of the task that writes them.
    keys: mpsc::Sender<Vec<u8>>,
    /// The pty's own end, for telling the program its size.
    master: Box<dyn MasterPty + Send>,
    /// For stopping it.
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// The program in the words it was started with.
    said: String,
    /// How it ended, once it has.
    ended: Option<Ended>,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Terminal")
            .field("id", &self.id)
            .field("said", &self.said)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Terminal {
    /// Starts a program in a terminal `rows` by `columns`, in `cwd`.
    ///
    /// # Errors
    ///
    /// Why it could not be started: no pty to be had, or no program there.
    pub fn start(
        id: Id,
        program: &Program,
        cwd: &Path,
        (rows, columns): (u16, u16),
        events: impl Sink<Heard> + Clone,
    ) -> Result<Self, String> {
        let size = PtySize {
            rows: rows.max(1),
            cols: columns.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(|error| format!("{error:#}"))?;
        let (mut command, said) = command_for(program);
        command.cwd(cwd);
        // What the parser on the other end understands, said the way every
        // program reads it: without it a program run from a window has no
        // `TERM` at all and draws as if on a printer.
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("{error:#}"))?;
        // The program's end, let go of here: while Obelus holds it the pty
        // never closes, and a program that has gone looks like one that is
        // quiet.
        drop(pair.slave);
        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("{error:#}"))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("{error:#}"))?;

        let runtime = obelus_runtime::handle();
        let heard = events.clone();
        runtime.spawn_blocking(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let bytes = buffer[..read].to_vec();
                        if heard.send(Heard::Wrote { id, bytes }).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        runtime.spawn_blocking(move || {
            let ended = match child.wait() {
                Ok(status) => Ended {
                    code: status.exit_code(),
                    signal: status.signal().map(str::to_string),
                },
                Err(error) => {
                    tracing::warn!(%error, "waiting for a terminal's program");
                    Ended {
                        code: u32::MAX,
                        signal: None,
                    }
                }
            };
            let _ = events.send(Heard::Ended { id, ended });
        });
        let (keys, typed) = mpsc::channel::<Vec<u8>>();
        runtime.spawn_blocking(move || {
            // Until the terminal is let go of, which drops the sender.
            while let Ok(bytes) = typed.recv() {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(Self {
            id,
            parser: vt100::Parser::new_with_callbacks(
                size.rows,
                size.cols,
                KEPT,
                Answers::default(),
            ),
            keys,
            master: pair.master,
            killer,
            said,
            ended: None,
        })
    }

    /// Which terminal this is.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The program, in the words it was started with.
    #[must_use]
    pub fn said(&self) -> &str {
        &self.said
    }

    /// What the program calls itself, where it has said.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.parser.callbacks().title.as_deref()
    }

    /// How it ended, once it has.
    #[must_use]
    pub const fn ended(&self) -> Option<&Ended> {
        self.ended.as_ref()
    }

    /// What it has drawn.
    #[must_use]
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Takes what the program wrote.
    pub fn wrote(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        // What it asked while it wrote, answered once all of it is read:
        // the cursor it asks about is the one after everything before the
        // question, which is where parsing has left it.
        let answers = std::mem::take(&mut self.parser.callbacks_mut().owed);
        if !answers.is_empty() {
            self.send(answers);
        }
    }

    /// The program has ended.
    pub fn end(&mut self, ended: Ended) {
        self.ended = Some(ended);
    }

    /// A key, as the program would read it from a terminal.
    ///
    /// Answers whether it was anything a terminal sends: a key that is not
    /// (a lone modifier, a release) is the caller's to do something else
    /// with, or nothing.
    pub fn key(&mut self, key: &KeyEvent) -> bool {
        let Some(bytes) = bytes_of(key, self.parser.screen().application_cursor()) else {
            return false;
        };
        // Back to the bottom, which is where what was typed is going to
        // show: a key pressed while reading back up the screen is a key
        // pressed at the prompt.
        self.parser.screen_mut().set_scrollback(0);
        self.send(bytes);
        true
    }

    /// Words put in at the cursor, the way a terminal pastes them.
    ///
    /// Marked as a paste where the program asked to be told, so that a
    /// shell does not run each line of it as it arrives -- and lines end in
    /// a return, which is what a terminal sends for one, rather than in the
    /// newline the clipboard carries.
    pub fn paste(&mut self, words: &str) {
        let words = words.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = match self.parser.screen().bracketed_paste() {
            true => format!("\u{1b}[200~{words}\u{1b}[201~"),
            false => words,
        };
        self.parser.screen_mut().set_scrollback(0);
        self.send(bytes.into_bytes());
    }

    /// Moves what is shown up the screen by `rows`, or down where negative.
    ///
    /// The view, not the program: a terminal's scrolling back is a reader
    /// looking at what has gone by, and the program is told nothing.
    pub fn scroll_by(&mut self, rows: isize) {
        let now = self.parser.screen().scrollback();
        let wanted = now.saturating_add_signed(rows);
        // The parser stops at what it has kept, so asking for more is
        // asking for the top.
        self.parser.screen_mut().set_scrollback(wanted);
    }

    /// How far back up the screen the view is, in rows.
    #[must_use]
    pub fn scrolled(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// The size it is drawn at, in rows and columns.
    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    /// Makes it this size, and tells the program.
    pub fn resize(&mut self, rows: u16, columns: u16) {
        let (rows, columns) = (rows.max(1), columns.max(1));
        if self.parser.screen().size() == (rows, columns) {
            return;
        }
        self.parser.screen_mut().set_size(rows, columns);
        let size = PtySize {
            rows,
            cols: columns,
            pixel_width: 0,
            pixel_height: 0,
        };
        if let Err(error) = self.master.resize(size) {
            tracing::warn!(error = %format!("{error:#}"), "telling a terminal its size");
        }
    }

    /// Stops the program, if it has not stopped.
    pub fn stop(&mut self) {
        if self.ended.is_none() {
            let _ = self.killer.kill();
        }
    }

    /// Hands bytes to the task that writes them.
    fn send(&self, bytes: Vec<u8>) {
        // A program that has gone takes nothing, and that is not news.
        let _ = self.keys.send(bytes);
    }
}

impl Drop for Terminal {
    /// A terminal let go of takes its program with it: nothing else is
    /// holding it, and nothing else can stop it.
    fn drop(&mut self) {
        self.stop();
    }
}

/// The command for a program, and the words it was started with.
fn command_for(program: &Program) -> (CommandBuilder, String) {
    match program {
        Program::Shell => {
            let command = CommandBuilder::new_default_prog();
            let said = command.get_shell();
            (command, said)
        }
        Program::Command {
            program,
            arguments,
            env,
        } => {
            let (program, arguments) = obelus_program::as_started_here(program, arguments);
            let mut command = CommandBuilder::new(&program);
            command.args(&arguments);
            for (key, value) in env {
                command.env(key, value);
            }
            let said = std::iter::once(program.display().to_string())
                .chain(arguments.iter().map(|argument| quoted(argument)))
                .collect::<Vec<_>>()
                .join(" ");
            (command, said)
        }
    }
}

/// An argument as a shell would need it written, so the words on the page
/// are words that could be run again.
fn quoted(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    match plain {
        true => argument.to_string(),
        false => format!("'{}'", argument.replace('\'', "'\\''")),
    }
}

/// What the program has asked of the terminal, and what it has called
/// itself.
#[derive(Default)]
struct Answers {
    /// Replies owed, in the order they were asked for.
    owed: Vec<u8>,
    /// The window title it set.
    title: Option<String>,
}

impl vt100::Callbacks for Answers {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let title = String::from_utf8_lossy(title).trim().to_string();
        self.title = (!title.is_empty()).then_some(title);
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first: Option<u8>,
        _second: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let param = params
            .first()
            .and_then(|it| it.first())
            .copied()
            .unwrap_or(0);
        match (first, c, param) {
            // Where the cursor is. A shell that draws its prompt where the
            // cursor is asks this before every prompt, and waits.
            (None, 'n', 6) => {
                let (row, column) = screen.cursor_position();
                self.owed
                    .extend(format!("\u{1b}[{};{}R", row + 1, column + 1).into_bytes());
            }
            // Whether the terminal is well.
            (None, 'n', 5) => self.owed.extend(b"\x1b[0n"),
            // What kind of terminal this is: a VT100 with advanced video,
            // which is what the parser is, and nothing it would have to
            // pretend to be.
            (None, 'c', 0) => self.owed.extend(b"\x1b[?1;2c"),
            (Some(b'>'), 'c', 0) => self.owed.extend(b"\x1b[>0;0;0c"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answers, quoted};

    /// A shell that asks where the cursor is is told, at the cursor.
    ///
    /// Broken deliberately by answering `1;1` always: the answer is wrong
    /// for every cursor that is not at the top left, which is every one a
    /// prompt has after the first line.
    #[test]
    fn where_the_cursor_is_is_answered_where_it_is() {
        let mut parser = vt100::Parser::new_with_callbacks(5, 20, 0, Answers::default());
        parser.process(b"one\r\ntwo\x1b[6n");
        assert_eq!(parser.callbacks().owed, b"\x1b[2;4R");
    }

    /// The words on the page are words that could be typed back in.
    #[test]
    fn an_argument_with_a_space_is_quoted() {
        assert_eq!(quoted("login"), "login");
        assert_eq!(quoted("--cli"), "--cli");
        assert_eq!(quoted("two words"), "'two words'");
        assert_eq!(quoted("it's"), "'it'\\''s'");
    }
}
