//! Searching: one question at three scopes.
//!
//! The scopes are one UI because the reader's question is one question --
//! "where is this" -- and only its radius changes. A separate view per scope
//! would ask them to remember which key opens which, and to retype the query
//! when the answer was not in the file after all.

pub mod counts;

use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
};

use ignore::WalkBuilder;
use obelus_runtime::cancel;
use obelus_sink::Sink;
use regex::Regex;

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
            Self::File => "File",
            Self::Project => "Project",
            Self::Symbols => "Symbols",
        }
    }
}

/// How a search is looking, beside what it is looking for.
///
/// Three switches, which are the three every reader has met: the query as a
/// pattern, the query as a whole word, and the capitals as typed. They are
/// the reader's, kept for as long as Obelus is running and not written
/// down: a pattern answers *this* question, and one turned on to find one
/// thing next Tuesday should not still be on the Tuesday after.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Looking {
    /// Whether the query is a pattern rather than the text itself.
    pub regex: bool,
    /// Whether it has to stand as a word rather than inside one.
    pub word: bool,
    /// Whether the capitals are meant however the query is written.
    ///
    /// Off is not "case does not matter": off is smart case, which is the
    /// query saying so itself -- one in lower case matches either, one with
    /// a capital in it means it. This is for the query that is all lower
    /// case and means it anyway, which smart case has no way to be told.
    pub sensitive: bool,
}

/// What a search is looking for, and the rule for finding it.
///
/// A thing rather than a string, because "is it in this line, and where" is
/// one question asked in three places -- by the walk of the project, by the
/// search of the file being read, and by the view marking what it found --
/// and three literal `contains` beside each other are three rules that will
/// come apart.
///
/// Never fuzzy. A search asks where a string *is*; `ac` is not in `abc`, and
/// a search that said it was would be answering a question about
/// resemblance that nobody asked. Fuzzy matching is how a reader picks a
/// name out of a list they already hold, which is what the pickers do and
/// what this is not.
#[derive(Clone, Debug)]
pub struct Needle {
    /// What the reader typed.
    said: String,
    /// The same in lower case, for the query that does not care about it.
    folded: String,
    /// Whether case matters: what the reader asked for, or what the query
    /// says itself where they asked for nothing.
    sensitive: bool,
    /// The compiled pattern, where the reader asked for one or for whole
    /// words.
    ///
    /// `None` for a plain literal, which is the case worth keeping out of a
    /// regex engine: it is what a whole project is walked with, and
    /// `str::find` is a real string search.
    pattern: Option<Regex>,
    /// Whether what they typed will not compile.
    ///
    /// Kept rather than reported, because a half-typed pattern is the
    /// ordinary state of one being typed: `(fn` is not an error to put in
    /// front of somebody, it is a question that is not finished. What it
    /// matches is nothing, and the view says so where it says "no match".
    broken: bool,
}

impl Needle {
    /// What a reader typed, ready to be looked for the way they asked.
    #[must_use]
    pub fn new(said: &str, how: Looking) -> Self {
        // Smart case unless the reader has said otherwise: one in lower
        // case matches either, one with a capital in it means it.
        let sensitive = how.sensitive || said.chars().any(char::is_uppercase);
        let mut needle = Self {
            folded: said.to_lowercase(),
            said: said.to_string(),
            sensitive,
            pattern: None,
            broken: false,
        };
        if said.is_empty() || !(how.regex || how.word) {
            return needle;
        }
        // Escaped where it is not a pattern, so whole words can be asked for
        // without the query becoming one: a reader searching for `a.b` with
        // words on means those three characters.
        let body = match how.regex {
            true => said.to_string(),
            false => regex::escape(said),
        };
        // `\b` around it, and the group so that a pattern with an
        // alternation in it has the boundaries around the whole of it
        // rather than around its first branch.
        let body = match how.word {
            true => format!("\\b(?:{body})\\b"),
            false => body,
        };
        let body = match sensitive {
            true => body,
            false => format!("(?i){body}"),
        };
        match Regex::new(&body) {
            Ok(pattern) => needle.pattern = Some(pattern),
            Err(error) => {
                tracing::debug!(%error, said, "not a pattern yet");
                needle.broken = true;
            }
        }
        needle
    }

    /// Whether nothing was typed, which is not a question.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.said.is_empty()
    }

    /// Whether what was typed will not compile as a pattern.
    #[must_use]
    pub const fn is_broken(&self) -> bool {
        self.broken
    }

    /// Where it is in a line, counted in characters, or nothing.
    ///
    /// Characters because that is what a row is marked in: the view walks
    /// the label a character at a time, and a byte offset would mark the
    /// wrong half of a glyph the moment a line had one in it.
    ///
    /// Found on bytes first, which is what makes walking a project affordable:
    /// `str::find` is a real string search and stepping character by
    /// character is not. Where case does not matter the line is lowered a
    /// line at a time rather than a file at a time, so a file whose first
    /// line matches does not cost a copy of the whole file.
    #[must_use]
    pub fn found_in(&self, line: &str) -> Option<Range<usize>> {
        if self.said.is_empty() || self.broken {
            return None;
        }
        if let Some(pattern) = self.pattern.as_ref() {
            let found = pattern.find(line)?;
            let from = line[..found.start()].chars().count();
            return Some(from..from + line[found.range()].chars().count());
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
/// Stopping is also what keeps a two-letter query on a large project from
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
/// than a project can be walked and the answers to the previous query must be
/// recognizable as stale. The last batch carries `done`, which is what turns
/// "searching" into "no match" -- with a query typed and nothing found,
/// those two are different facts and only the scan knows which is true.
///
/// `wanted` is asked before every file: typing a ten-letter word starts ten
/// scans, and nine of them would otherwise go on reading the whole project to
/// answer a question nobody is asking any more. There is nothing else to
/// stop a thread with -- a walk in `ignore` cannot be interrupted from
/// outside -- so the thread has to ask. [`obelus_runtime::cancel`] is where
/// that asking lives now; this file, the history walk and the file walk each
/// had their own copy of it.
///
/// `ignored` searches the files the project has said to ignore as well, which
/// is the reader's `ignored_files` and not a question of this walk's own: a
/// file list that offers `target` beside a search that cannot see into it
/// Something a walk of the project found.
#[derive(Debug)]
pub enum Event {
    /// A batch of paths from the file walk.
    FilesFound {
        /// Which walk these came from, so batches from a walk whose picker has
        /// already closed can be dropped.
        generation: u64,
        /// The paths, relative to the walk's root.
        paths: Vec<PathBuf>,
        /// Whether these are files the project said it does not keep.
        ///
        /// Their own batches rather than a flag per path: a walk sends one
        /// kind or the other and never a mixture, because it is two walks
        /// -- one that obeys the ignore rules and one that does not.
        ignored: bool,
    },
    /// A batch of matching lines from a search of the project.
    Matches {
        /// Which search these came from, so answers to a query the reader
        /// has already typed past can be dropped.
        generation: u64,
        /// The matches, in the order the walk found them.
        hits: Vec<Hit>,
        /// Whether this is the last batch. With a query typed and nothing
        /// found, "still looking" and "not there" are different facts, and
        /// only the scan knows which one to show.
        done: bool,
    },
}

/// How many paths to send at a time.
///
/// One message per file would wake the loop once per file and redraw a list
/// that is about to change again. One message for everything would leave the
/// picker empty for as long as the walk takes on a large project.
///
/// Larger than the batch a search sends: a path is a path, and a match
/// carries the line it was found on.
const WALK_BATCH: usize = 512;

/// Walks a directory tree on its own thread, sending paths back in batches.
///
/// `generation` comes back with every batch. The picker can be closed and
/// reopened while a walk is still running, and the batches from the old one
/// have to be recognizable as stale rather than merged into the new list.
///
/// `ignored` offers the files the tree has said to ignore as well. Only the
/// ignore rules go: hidden files stay hidden either way, because `.git` is a
/// directory with one file per object in it and a reader who asked to see
/// what `.gitignore` hides did not ask for that.
///
/// A thread because `WalkBuilder` is a blocking API, and the walk of a large
/// tree is long enough that the picker has to be usable while it runs.
pub fn spawn_walk(root: &Path, wanted: cancel::Wanted, ignored: bool, sender: impl Sink<Event>) {
    let root = root.to_path_buf();
    obelus_runtime::handle().spawn_blocking(move || {
        // The files the tree keeps, first and on their own: they are what
        // a reader is usually after, and this is the quick walk -- it is
        // the one that does not descend into `target`.
        let mut sent = HashSet::new();
        if !walk(&root, true, &wanted, &sender, &mut sent) || !ignored {
            return;
        }
        // And then the ones it does not keep, which are whatever the
        // first walk did not send. Asking a second matcher whether a path
        // is ignored would be asking the same question twice and leaving
        // the two answers free to differ; this way "ignored" means
        // exactly "the walk that obeys the rules did not offer it".
        walk(&root, false, &wanted, &sender, &mut sent);
    });
}

/// One walk over the tree, sending what it finds in batches.
///
/// `obeying` says whether the ignore rules apply. The walk that obeys them
/// remembers in `sent` what it offered; the walk that does not obey them
/// leaves those out and sends the rest, marked as ignored.
///
/// Hidden files are skipped either way. Returns whether there is still
/// anybody to send to.
fn walk(
    root: &Path,
    obeying: bool,
    wanted: &cancel::Wanted,
    sender: &impl Sink<Event>,
    sent: &mut HashSet<PathBuf>,
) -> bool {
    let mut batch: Vec<PathBuf> = Vec::with_capacity(WALK_BATCH);
    let mut walk = WalkBuilder::new(root);
    walk.git_ignore(obeying)
        .git_global(obeying)
        .git_exclude(obeying)
        .ignore(obeying)
        .parents(obeying);

    for entry in walk.build() {
        let entry = match entry {
            Ok(entry) => entry,
            // An unreadable directory is not worth abandoning the walk over.
            Err(error) => {
                tracing::debug!(%error, "skipping an entry");
                continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        // Relative to the root, which is what the picker shows and what the
        // reader typed to get here.
        let path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or_else(|_| entry.path())
            .to_path_buf();
        match obeying {
            true => {
                sent.insert(path.clone());
            }
            false if sent.contains(&path) => continue,
            false => {}
        }
        batch.push(path);

        if batch.len() >= WALK_BATCH {
            // Asked where the walk is already pausing to send. A list
            // opened and closed twice used to leave two walks reading the
            // whole tree for nobody; now the second batch of each is the
            // last thing it does.
            if !wanted.still() {
                return false;
            }
            let paths = std::mem::replace(&mut batch, Vec::with_capacity(WALK_BATCH));
            // The receiver is gone, so the loop has ended.
            if sender
                .send(Event::FilesFound {
                    generation: wanted.generation(),
                    paths,
                    ignored: !obeying,
                })
                .is_err()
            {
                return false;
            }
        }
    }

    if !batch.is_empty() && wanted.still() {
        let _ = sender.send(Event::FilesFound {
            generation: wanted.generation(),
            paths: batch,
            ignored: !obeying,
        });
    }
    true
}

/// is two answers about one tree.
pub fn spawn_scan(
    root: &Path,
    needle: &Needle,
    wanted: cancel::Wanted,
    ignored: bool,
    sender: impl Sink<Event> + Clone,
) {
    let root = root.to_path_buf();
    let needle = needle.clone();
    obelus_runtime::handle().spawn_blocking(move || {
        let mut batch: Vec<Hit> = Vec::with_capacity(BATCH);
        let mut found = 0usize;

        let mut walk = WalkBuilder::new(&root);
        walk.git_ignore(!ignored)
            .git_global(!ignored)
            .git_exclude(!ignored)
            .ignore(!ignored)
            .parents(!ignored);
        for entry in walk.build() {
            // Per file rather than per line: a file is the unit of work
            // here, and reading the flag for every line of a large file
            // would be a cost of its own.
            if !wanted.still() {
                let generation = wanted.generation();
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
                            generation: wanted.generation(),
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
            generation: wanted.generation(),
            hits: batch,
            done: true,
        });
    });
}
