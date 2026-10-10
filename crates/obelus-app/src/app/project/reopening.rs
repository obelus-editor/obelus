//! What was open, so that a tree opens on it again.
//!
//! A reader who leaves in the middle of something comes back to the middle
//! of it: the files they had open, the caret where it was in each, the
//! notes on the note they were on, and the conversations -- and the one
//! they were standing in.
//!
//! **Kept by the tree, not by the project.** Two worktrees of one
//! repository are one project to the notes and the conversations, and two
//! different sets of files to a reader: a path in one is not a path in the
//! other, and what is open in each is the tree's own the way its branch is.
//! The tree is git's answer for wherever Obelus was put, so `ob`, `ob src`
//! and `ob src/main.rs` in one checkout are one record.
//!
//! **Several Obelus on one tree is the normal case, and the last to change
//! what it has open wins.** Each writes the record whole when what it has
//! open changes -- a document opened or closed, another one gone to -- and
//! not when it has not, so a window the reader is only scrolling in leaves
//! another's record alone. Written then rather than only on the way out,
//! because closing the terminal is a way out too, and nothing runs on it.
//! Where the caret is in each file goes into the record with it but does
//! not decide when it is written: a write per arrow key is a file renamed
//! over twelve times a second. So the carets are as they were at the last
//! change, and the way out freshens them -- unless another window has
//! written since, which is the one that changed something last.
//!
//! **Read once, when the tree is settled on, and the files on a thread.**
//! The record is a few lines; the files it names are what costs, and a
//! start is when the reader is looking at an empty screen. So they are read
//! elsewhere and put in the list when they arrive, in the record's order,
//! with the notes and the conversations between them. Until then nothing is
//! written: a record of what has arrived so far would take the place of
//! the one being read. What is passed over is what cannot come back: a file
//! that has gone, a conversation another Obelus has open, and one there is
//! nothing to come back to -- nothing was said in it, or it was had with an
//! agent that is not the one in use now. The record is never read into
//! what is written, so a record that will not read is not one this could
//! make worse.
//!
//! **A tree that has gone takes its record with it.** Looked for on the
//! same thread on every start, which is a moment the records are being read
//! anyway, and by the tree each one names: `git worktree remove` is done in
//! a shell, with no Obelus on the tree to see it go. The cost is the one the
//! list of projects accepts -- a disk not plugged in looks like a tree that
//! was removed, and opening it again starts a record afresh.

use std::path::{Path, PathBuf};

use obelus_agent::chats::ChatId;
use obelus_text::coordinates::{CharColumn, LineNumber};
use obelus_todo::NoteId;

use crate::{
    app::{App, DocumentId, document::Document},
    conversation::Topic,
};

/// One document the record names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Open {
    /// A file on disk, with the caret and the view in it.
    ///
    /// Not a commit's version of one: what a commit had is a question the
    /// reader asked of the history, and the history is where it is asked
    /// again.
    File {
        /// Against the tree where it is in it, as it was named otherwise.
        path: PathBuf,
        /// The line the caret is on, counted from zero the way the text
        /// counts them: a file format has no reader to number lines for.
        line: usize,
        /// The character it is before, counted from zero.
        column: usize,
        /// The first line on screen, counted from zero.
        top: usize,
    },
    /// A conversation, by what claims it.
    Conversation(ChatId),
    /// The notes, with the note the selection was on.
    Notes {
        /// By its name, because a note added above it since would move
        /// every position below.
        on: Option<NoteId>,
    },
}

impl Open {
    /// Whether the two are one document, wherever the caret is in it.
    fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File { path, .. }, Self::File { path: theirs, .. }) => path == theirs,
            (Self::Notes { .. }, Self::Notes { .. }) => true,
            _ => self == other,
        }
    }
}

/// What was open, and which of it was being read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Record {
    open: Vec<Open>,
    /// Which of `open` was on screen, where one of them was.
    current: Option<usize>,
}

impl Record {
    /// Whether the same documents are open in the same order, with the same
    /// one on screen: the question that decides whether to write.
    fn names_the_same(&self, other: &Self) -> bool {
        self.current == other.current
            && self.open.len() == other.open.len()
            && self
                .open
                .iter()
                .zip(&other.open)
                .all(|(one, two)| one.is(two))
    }
}

/// What this window knows about its tree's record.
#[derive(Debug, Default)]
pub(in crate::app) struct Reopening {
    /// Where the record is, while Obelus is on a worktree.
    path: Option<PathBuf>,
    /// The tree, which the files are named against.
    tree: PathBuf,
    /// What this window last wrote.
    written: Option<Record>,
    /// The record being reopened, while its files are read.
    ///
    /// Taken when they arrive, so that an answer nobody is waiting for --
    /// one sent for twice -- puts nothing in the list.
    waiting: Option<Record>,
}

/// Where a tree's record lives.
///
/// Only for a worktree, for the reason only a worktree is a project
/// remembered: a process begun in `$HOME` or in a directory of downloads is
/// where Obelus happened to start, and a record of those is a state
/// directory filling with places nobody works.
fn path_for(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let tree = obelus_git::worktree(root)?;
    // Canonical, so that one tree reached by two spellings is one record.
    let named = obelus_git::file_name_of(&tree.canonicalize().unwrap_or_else(|_| tree.clone()));
    let path = obelus_logging::state_directory()?
        .join("open")
        .join(format!("{named}.toml"));
    Some((path, tree))
}

/// Reads a record, or nothing where there is none or it will not read.
fn read(path: &Path) -> Option<Record> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "what was open will not read");
            return None;
        }
    };
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "what was open will not read");
            return None;
        }
    };
    let counted = |row: &toml::Table, key: &str| {
        row.get(key)
            .and_then(toml::Value::as_integer)
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or(0)
    };
    let open = table
        .get("open")
        .and_then(toml::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_table()?;
                    if let Some(path) = row.get("file").and_then(toml::Value::as_str) {
                        return Some(Open::File {
                            path: PathBuf::from(path),
                            line: counted(row, "line"),
                            column: counted(row, "column"),
                            top: counted(row, "top"),
                        });
                    }
                    if let Some(name) = row.get("conversation").and_then(toml::Value::as_str) {
                        return ChatId::read(name).map(Open::Conversation);
                    }
                    row.get("notes")
                        .and_then(toml::Value::as_bool)
                        .filter(|notes| *notes)
                        .map(|_| Open::Notes {
                            on: row
                                .get("on")
                                .and_then(toml::Value::as_str)
                                .and_then(NoteId::read),
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    let current = table
        .get("current")
        .and_then(toml::Value::as_integer)
        .and_then(|at| usize::try_from(at).ok());
    Some(Record { open, current })
}

/// What the file holds, as text.
///
/// By hand, the way the list of projects is: a few lines a document, and a
/// reader who opens it should find it readable.
fn to_toml(tree: &Path, record: &Record) -> String {
    let quoted = |text: &str| toml::Value::String(text.to_string()).to_string();
    // Which tree, for nothing but finding out it has gone: the file's name
    // is many-to-one and cannot be read back.
    let mut out = format!("tree = {}\n", quoted(&tree.to_string_lossy()));
    if let Some(current) = record.current {
        out.push_str(&format!("current = {current}\n"));
    }
    for open in &record.open {
        out.push_str("\n[[open]]\n");
        match open {
            Open::File {
                path,
                line,
                column,
                top,
            } => {
                out.push_str(&format!("file = {}\n", quoted(&path.to_string_lossy())));
                out.push_str(&format!("line = {line}\ncolumn = {column}\ntop = {top}\n"));
            }
            Open::Conversation(which) => {
                out.push_str(&format!("conversation = {}\n", quoted(&which.file_name())));
            }
            Open::Notes { on } => {
                out.push_str("notes = true\n");
                if let Some(note) = on {
                    out.push_str(&format!("on = {}\n", quoted(note.as_str())));
                }
            }
        }
    }
    out
}

/// Writes a record over whatever is there.
fn write(path: &Path, tree: &Path, record: &Record) {
    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        tracing::warn!(%error, path = %path.display(), "nowhere to say what was open");
        return;
    }
    // Beside it under this process's own name, and a rename: another Obelus
    // on this tree writing at the same moment leaves one whole record or
    // the other.
    let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
    if let Err(error) =
        std::fs::write(&beside, to_toml(tree, record)).and_then(|()| std::fs::rename(&beside, path))
    {
        tracing::warn!(%error, path = %path.display(), "what was open was not written");
        let _ = std::fs::remove_file(&beside);
    }
}

/// Takes away the record of every tree that has gone.
///
/// A record that names no tree, or will not read, is left alone: it is
/// nothing this can say has gone.
fn forget_the_trees_that_have_gone(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let tree = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|table| table.get("tree")?.as_str().map(PathBuf::from));
        if let Some(tree) = tree
            && obelus_git::is_gone(&tree)
        {
            tracing::info!(tree = %tree.display(), "the tree has gone, so what it had open is forgotten");
            let _ = std::fs::remove_file(&path);
        }
    }
}

impl App {
    /// Says which tree's record this window keeps, as it is put on one.
    pub(in crate::app) fn keep_what_is_open_for(&mut self, root: &Path) {
        let (path, tree) = path_for(root).unzip();
        self.reopening = Reopening {
            path,
            tree: tree.unwrap_or_default(),
            ..Reopening::default()
        };
    }

    /// What is open now, as the record would say it.
    fn what_is_open(&self) -> Record {
        let mut record = Record::default();
        for (at, document) in self.documents.iter().enumerate() {
            let open = match document {
                Some(Document::File(buffer)) if buffer.content().is_file() => {
                    let path = buffer.path();
                    let cursor = buffer.cursor();
                    Some(Open::File {
                        path: path
                            .strip_prefix(&self.reopening.tree)
                            .unwrap_or(path)
                            .to_path_buf(),
                        line: cursor.line.get(),
                        column: cursor.column.get(),
                        top: buffer.viewport().top.get(),
                    })
                }
                Some(Document::Chat(talk)) => talk.which().map(Open::Conversation),
                Some(Document::Notes(notes)) => Some(Open::Notes {
                    on: notes.selected_note().map(|note| note.id.clone()),
                }),
                // A program that was running is not one a new window can
                // pick up: what it had is gone with the process that ran it,
                // and starting it again would be running something the
                // reader did not ask for this time.
                Some(Document::File(_) | Document::Terminal(_)) | None => None,
            };
            if let Some(open) = open {
                if self.current == Some(DocumentId::new(at)) {
                    record.current = Some(record.open.len());
                }
                record.open.push(open);
            }
        }
        record
    }

    /// Where this window's record is, where it may write it now.
    ///
    /// Not while the record is still being read: what has arrived so far
    /// would take the place of the one being read.
    fn where_to_write(&self) -> Option<PathBuf> {
        let may = self.settled.config.reopen
            && !self.headless
            && self.has_a_project()
            && self.reopening.waiting.is_none();
        may.then(|| self.reopening.path.clone()).flatten()
    }

    /// Writes down what is open, where it has changed since this window
    /// last did.
    ///
    /// Asked once a frame, from what is open, rather than at each of the
    /// ways a document opens or closes: there are a dozen of those, and a
    /// rule kept at each is a rule the next one forgets. The question is a
    /// comparison of a handful of names, and the write happens only when
    /// the answer moved.
    pub(in crate::app) fn write_down_what_is_open(&mut self) {
        let Some(path) = self.where_to_write() else {
            return;
        };
        let now = self.what_is_open();
        if self
            .reopening
            .written
            .as_ref()
            .is_some_and(|written| written.names_the_same(&now))
        {
            return;
        }
        write(&path, &self.reopening.tree, &now);
        self.reopening.written = Some(now);
    }

    /// Writes down what is open on the way out, with the carets where they
    /// are now.
    ///
    /// Unless another window on this tree has written since this one did:
    /// that window changed what it had open after this one last changed
    /// anything, and the last to change is the one that wins.
    pub(in crate::app) fn write_down_what_is_open_on_leaving(&mut self) {
        let Some(path) = self.where_to_write() else {
            return;
        };
        if let Some(written) = &self.reopening.written
            && read(&path).is_none_or(|there| !there.names_the_same(written))
        {
            return;
        }
        write(&path, &self.reopening.tree, &self.what_is_open());
    }

    /// Opens again what was open the last time this tree was.
    ///
    /// Beside whatever the command line named rather than instead of it,
    /// and the reader is left on what they named: `ob src/main.rs` is a
    /// reader saying what they want to look at, and not that they want to
    /// lose everything else.
    ///
    /// The record is read here and its files on a thread, as soon as there
    /// is a channel to answer on; what it names goes in the list when they
    /// arrive.
    pub fn reopen_what_was_open(&mut self) {
        if !self.settled.config.reopen || self.headless {
            return;
        }
        let Some(path) = self.reopening.path.clone() else {
            return;
        };
        self.reopening.waiting = read(&path);
        self.send_the_reopening();
    }

    /// Reads the files the record names on a thread, where there is a
    /// record waiting and a channel to answer on -- which a start does not
    /// have yet when it reads the record, so it asks again once it does.
    pub(in crate::app) fn send_the_reopening(&mut self) {
        let Some(record) = self.reopening.waiting.as_ref() else {
            return;
        };
        let Some(sender) = self.events.clone() else {
            return;
        };
        let files: Vec<PathBuf> = record
            .open
            .iter()
            .filter_map(|open| match open {
                Open::File { path, .. } => Some(self.reopening.tree.join(path)),
                Open::Conversation(_) | Open::Notes { .. } => None,
            })
            .collect();
        let records = self
            .reopening
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf);
        let tree = self.reopening.tree.clone();
        obelus_runtime::handle().spawn_blocking(move || {
            let read = files
                .into_iter()
                .map(|path| {
                    // A file that has gone does not open, and is passed over.
                    let buffer = obelus_buffer::Buffer::open(&path)
                        .inspect_err(|error| {
                            tracing::info!(%error, path = %path.display(), "not opened again");
                        })
                        .ok();
                    (path, buffer)
                })
                .collect();
            // Before the answer rather than after, so that whatever is told
            // the files have arrived is told the sweep is done too: a few
            // small files, against the files the reader is waiting for.
            if let Some(records) = records {
                forget_the_trees_that_have_gone(&records);
            }
            let _ = sender.send(crate::event::Event::Reopened { tree, files: read });
        });
    }

    /// Puts what was open in the list, now that its files have been read.
    pub(in crate::app) fn take_up_what_was_open(
        &mut self,
        read_for: &Path,
        mut files: Vec<(PathBuf, Option<obelus_buffer::Buffer>)>,
    ) {
        // Read for a tree this window has since left, and so not what it
        // is waiting on: that is still being read.
        if read_for != self.reopening.tree {
            return;
        }
        let Some(record) = self.reopening.waiting.take() else {
            return;
        };
        let tree = self.reopening.tree.clone();
        // Which conversation is which, read once for all of them. Asked of
        // the agent in use, because a session is a name one agent minted.
        //
        // Nothing forgotten for being old: every row asked for here is one
        // the reader left on screen, which is the conversation being read
        // that the forgetting spares -- and nothing has claimed it yet to
        // say so, because coming back is what claims it.
        let agent = self.settled.config.agent.clone();
        let sessions = record
            .open
            .iter()
            .any(|open| matches!(open, Open::Conversation(_)))
            .then(|| obelus_agent::acp::sessions::read(&self.working_directory, 0).remembered())
            .flatten();
        let mut landed: Vec<Option<DocumentId>> = Vec::new();
        let mut talked = false;
        for open in record.open {
            let at = match open {
                Open::File {
                    path,
                    line,
                    column,
                    top,
                } => {
                    let path = tree.join(path);
                    let buffer = files
                        .iter_mut()
                        .find(|(read, _)| *read == path)
                        .and_then(|(_, buffer)| buffer.take());
                    self.reopen_file(&path, buffer, line, column, top)
                }
                Open::Conversation(which) => {
                    let kept = agent.as_deref().and_then(|agent| {
                        sessions
                            .as_ref()?
                            .get(&which, agent, &self.working_directory)
                    });
                    let at = kept
                        .cloned()
                        .and_then(|kept| self.reopen_conversation(which, &kept));
                    talked |= at.is_some();
                    at
                }
                Open::Notes { on } => self.notes_document().or_else(|| self.put_the_notes_up(on)),
            };
            landed.push(at);
        }
        // What a conversation about a note is read against, as taking one up
        // from the list reads it: which session is that note's, when it is
        // shown, and what the note says. Kept as read rather than read again
        // -- the table is in hand.
        if talked {
            self.kept.sessions_kept = sessions;
            self.reread_the_notes_kept();
            self.reread_who_holds_what();
        }
        // Only onto an empty screen: one with something on it is what the
        // command line named, or where the reader went while this was read.
        if self.current.is_none()
            && let Some(id) = record
                .current
                .and_then(|at| landed.get(at).copied().flatten())
                .or_else(|| landed.iter().copied().flatten().next())
        {
            self.go_to_document(id);
        }
    }

    /// Puts a file read again in the list, with the caret and the view where
    /// they were.
    ///
    /// Not over one already open: the reader named it, or opened it while
    /// the rest was being read, and where its caret is now is theirs.
    fn reopen_file(
        &mut self,
        path: &Path,
        buffer: Option<obelus_buffer::Buffer>,
        line: usize,
        column: usize,
        top: usize,
    ) -> Option<DocumentId> {
        if let Some(at) = self.open_at(path) {
            return Some(DocumentId::new(at));
        }
        let at = self.take_in(buffer?);
        let buffer = self.file_mut(DocumentId::new(at))?;
        buffer.place_cursor(LineNumber::new(line), CharColumn::new(column));
        // The view where it was, rather than wherever the caret pulls it:
        // the reader left a screen, and that screen is what they know.
        let top = buffer.text().clamp_line(LineNumber::new(top));
        buffer.look_back(obelus_buffer::Viewport {
            top,
            top_row: 0,
            left: 0,
        });
        Some(DocumentId::new(at))
    }

    /// Opens a conversation again, without going to it.
    ///
    /// Claimed now, the way taking one up from the list claims it, and not
    /// taken up until it is shown: the agent is started for a conversation
    /// the reader comes to, and one they never go back to costs nothing.
    fn reopen_conversation(
        &mut self,
        which: ChatId,
        kept: &obelus_agent::acp::sessions::Kept,
    ) -> Option<DocumentId> {
        let open = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .and_then(crate::conversation::Conversation::which)
                .is_some_and(|open| open == which)
        });
        if let Some(at) = open {
            return Some(DocumentId::new(at));
        }
        let claim = obelus_agent::chats::claim(&self.working_directory, &which)?;
        let to_take_up = match &which {
            ChatId::Loose(session) => Some(session.clone()),
            ChatId::Note(_) | ChatId::PullRequest(_) | ChatId::Issue(_) => None,
        };
        let topic = Topic::of(&which);
        let talk = crate::conversation::Conversation {
            told: kept.told.clone(),
            introduced: kept.introduced,
            topic,
            claim: Some(claim),
            to_take_up,
            ..crate::conversation::Conversation::default()
        };
        self.documents.push(Some(talk.into()));
        Some(DocumentId::new(self.documents.len() - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is written is what is read: the record is the only place a
    /// tree's documents survive the window, and one that does not come
    /// back whole reopens something else.
    ///
    /// Broken deliberately by writing every `line` as 0: the file came
    /// back on its first line, and this failed.
    #[test]
    fn a_record_survives_the_file() {
        let record = Record {
            open: vec![
                Open::File {
                    path: PathBuf::from("src/a \"quoted\" name.rs"),
                    line: 12,
                    column: 5,
                    top: 3,
                },
                Open::Notes {
                    on: NoteId::read("0123456R"),
                },
                Open::Conversation(ChatId::Loose("a session/with odd bytes".to_string())),
                Open::Conversation(ChatId::read("FTMER3E6").expect("a note's name")),
            ],
            current: Some(2),
        };
        let directory =
            std::env::temp_dir().join(format!("obelus-reopening-{}", std::process::id()));
        let path = directory.join("record.toml");
        write(&path, Path::new("/somewhere"), &record);
        let back = read(&path);
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(back, Some(record));
    }

    /// Where the caret is decides nothing about writing; which documents
    /// are open, in what order, and which is on screen decides all of it.
    ///
    /// Broken deliberately by comparing with `==`: a moved caret then
    /// counted as a change, and the first assertion failed. And by taking
    /// the notes' arm out of `Open::is`, which failed the second.
    #[test]
    fn a_caret_moving_is_not_a_change() {
        let at = |line| Record {
            open: vec![Open::File {
                path: PathBuf::from("a.rs"),
                line,
                column: 1,
                top: 1,
            }],
            current: Some(0),
        };
        assert!(at(1).names_the_same(&at(40)), "a moved caret was a change");
        let notes = |on| Record {
            open: vec![Open::Notes {
                on: NoteId::read(on),
            }],
            current: Some(0),
        };
        assert!(
            notes("0123456R").names_the_same(&notes("0123456S")),
            "walking the notes was a change"
        );
        let elsewhere = Record {
            current: None,
            ..at(1)
        };
        assert!(
            !at(1).names_the_same(&elsewhere),
            "going elsewhere was not a change"
        );
    }
}
