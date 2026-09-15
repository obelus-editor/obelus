//! Searching: one question at three scopes.
//!
//! The scopes are one UI because the reader's question is one question --
//! "where is this" -- and only its radius changes. A separate view per scope
//! would ask them to remember which key opens which, and to retype the query
//! when the answer was not in the file after all.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Sender,
    },
};

use ignore::WalkBuilder;

use crate::event::Event;

/// How far a search reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The file being read.
    File,
    /// Every file under the working directory.
    Project,
    /// The names the language server knows, across the project.
    Symbols,
}

impl Scope {
    /// Every scope, in the order their tabs sit in.
    ///
    /// Which of them a search *shows* is a question about the state of the
    /// world -- whether a file is open, whether a server is running -- so
    /// the tab a scope sits on is not fixed and the application keeps the
    /// list it built.
    pub const ALL: [Self; 3] = [Self::File, Self::Project, Self::Symbols];

    /// Whether this scope's rows *are* the lines they name.
    ///
    /// A search of a file or of the project lists lines, trimmed of their
    /// indentation: a column of such a row is a column of the code, so the
    /// characters a query matched are characters of the file -- which is
    /// what the preview marks and where choosing the row lands.
    ///
    /// A symbol search lists *names*. A column of a name means nothing in
    /// the file, and treating one as if it did landed the cursor short of
    /// the symbol, on whatever was in front of it.
    ///
    /// A `match` without a wildcard, so a scope added later has to say
    /// which kind it is.
    pub const fn lists_lines(self) -> bool {
        match self {
            Self::File | Self::Project => true,
            Self::Symbols => false,
        }
    }

    /// The tab's name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Project => "project",
            Self::Symbols => "symbols",
        }
    }
}

/// What a search is looking for, and the rule for finding it.
///
/// A thing rather than a string, because "is it in this line, and where" is
/// one question asked in three places -- by the walk of the tree, by the
/// search of the file being read, and by the view marking what it found --
/// and three literal `contains` beside each other are three rules that will
/// come apart.
///
/// Literal, always. A search asks where a string *is*; `ac` is not in `abc`,
/// and a search that said it was would be answering a question about
/// resemblance that nobody asked. Fuzzy matching is how a reader picks a
/// name out of a list they already hold, which is what the pickers do and
/// what this is not.
#[derive(Clone, Debug)]
pub struct Needle {
    /// What the reader typed.
    said: String,
    /// The same in lower case, for the query that does not care about it.
    folded: String,
    /// Whether case matters, which the query says itself: one in lower case
    /// matches either, and one with a capital in it means it.
    ///
    /// Smart case, the same rule the pickers' matcher follows, so a reader
    /// does not learn two.
    sensitive: bool,
}

impl Needle {
    /// What a reader typed, ready to be looked for.
    #[must_use]
    pub fn new(said: &str) -> Self {
        Self {
            sensitive: said.chars().any(char::is_uppercase),
            folded: said.to_lowercase(),
            said: said.to_string(),
        }
    }

    /// Whether nothing was typed, which is not a question.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.said.is_empty()
    }

    /// Where it is in a line, counted in characters, or nothing.
    ///
    /// Characters because that is what a row is marked in: the view walks
    /// the label a character at a time, and a byte offset would mark the
    /// wrong half of a glyph the moment a line had one in it.
    ///
    /// Found on bytes first, which is what makes walking a tree affordable:
    /// `str::find` is a real string search and stepping character by
    /// character is not. Where case does not matter the line is lowered a
    /// line at a time rather than a file at a time, so a file whose first
    /// line matches does not cost a copy of the whole file.
    #[must_use]
    pub fn found_in(&self, line: &str) -> Option<Range<usize>> {
        if self.said.is_empty() {
            return None;
        }
        if self.sensitive {
            let at = line.find(&self.said)?;
            let from = line[..at].chars().count();
            return Some(from..from + self.said.chars().count());
        }
        let lowered = line.to_lowercase();
        let at = lowered.find(&self.folded)?;
        // Counted in the lowered copy. Lowering can change how many
        // characters a line has -- Turkish dotted I lowers to two -- so this
        // is the right column wherever the two agree, which is everywhere a
        // reader will meet, and a column or two out in the one place it is
        // not. What it can never do is claim a line matched that did not.
        let from = lowered[..at].chars().count();
        Some(from..from + self.folded.chars().count())
    }
}

/// One matching line, ready to become a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Which file, relative to the root that was walked.
    pub path: PathBuf,
    /// Which line of it, counted from zero.
    pub line: usize,
    /// The line itself, trimmed of its indentation and of anything past
    /// [`WIDEST_LINE`]: a row is one line of a list, and a minified file has
    /// lines nothing can show.
    pub text: String,
}

/// How many hits to send at a time, for the same reason the file walk sends
/// batches: one message per hit redraws the list once per hit.
const BATCH: usize = 128;

/// How many hits are worth having.
///
/// A reader does not read the ten-thousandth match; they narrow the query.
/// Stopping is also what keeps a two-letter query on a large tree from
/// spending the whole walk building rows nobody will scroll to.
const MOST: usize = 2000;

/// How much of a line a row can show.
const WIDEST_LINE: usize = 300;

/// How big a file is worth reading.
///
/// Minified bundles, lock files and vendored blobs are megabytes of one line
/// nobody is searching. The limit is generous for anything written by hand.
const BIGGEST_FILE: u64 = 2 * 1024 * 1024;

/// Scans every file under `root` for `query`, on its own thread.
///
/// What counts as a match is [`Needle`]'s to say, which is also what the
/// search of the open file asks and what the view marks.
///
/// `generation` comes back with every batch, because the reader types faster
/// than a tree can be walked and the answers to the previous query must be
/// recognizable as stale. The last batch carries `done`, which is what turns
/// "searching" into "no match" -- with a query typed and nothing found,
/// those two are different facts and only the scan knows which is true.
///
/// `current` is the generation the reader is actually waiting for, and the
/// walk reads it before every file: typing a ten-letter word starts ten
/// scans, and nine of them would otherwise go on reading the whole tree to
/// answer a question nobody is asking any more. There is nothing else to
/// stop a thread with -- a walk in `ignore` cannot be interrupted from
/// outside -- so the thread has to ask.
pub fn spawn_scan(
    root: &Path,
    query: &str,
    generation: u64,
    current: &Arc<AtomicU64>,
    sender: Sender<Event>,
) {
    let root = root.to_path_buf();
    let query = query.to_string();
    let current = Arc::clone(current);
    let outcome = std::thread::Builder::new()
        .name("obelus-search".to_string())
        .spawn(move || {
            let needle = Needle::new(&query);
            let mut batch: Vec<Hit> = Vec::with_capacity(BATCH);
            let mut found = 0usize;

            for entry in WalkBuilder::new(&root).build() {
                // Per file rather than per line: a file is the unit of work
                // here, and reading the flag for every line of a large file
                // would be a cost of its own.
                if current.load(Ordering::Relaxed) != generation {
                    tracing::debug!(generation, "a scan the reader has typed past, stopping");
                    return;
                }
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    continue;
                }
                if entry.metadata().is_ok_and(|data| data.len() > BIGGEST_FILE) {
                    continue;
                }
                // Not UTF-8 is not an error here: it is a binary file, and a
                // reader searching for a word is not searching those.
                let Ok(contents) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let path = entry
                    .path()
                    .strip_prefix(&root)
                    .unwrap_or_else(|_| entry.path())
                    .to_path_buf();

                for (number, line) in contents.lines().enumerate() {
                    if needle.found_in(line).is_none() {
                        continue;
                    }
                    let trimmed = line.trim();
                    let text = match trimmed.char_indices().nth(WIDEST_LINE) {
                        Some((at, _)) => trimmed[..at].to_string(),
                        None => trimmed.to_string(),
                    };
                    batch.push(Hit {
                        path: path.clone(),
                        line: number,
                        text,
                    });
                    found += 1;

                    if batch.len() >= BATCH || found >= MOST {
                        let hits = std::mem::replace(&mut batch, Vec::with_capacity(BATCH));
                        let done = found >= MOST;
                        if sender
                            .send(Event::Matches {
                                generation,
                                hits,
                                done,
                            })
                            .is_err()
                            || done
                        {
                            return;
                        }
                    }
                }
            }

            let _ = sender.send(Event::Matches {
                generation,
                hits: batch,
                done: true,
            });
        });

    if let Err(error) = outcome {
        tracing::warn!(%error, "not searching the tree");
    }
}
