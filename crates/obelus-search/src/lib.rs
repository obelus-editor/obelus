//! Searching: one question at three scopes.
//!
//! The scopes are one UI because the reader's question is one question --
//! "where is this" -- and only its radius changes. A separate view per scope
//! would ask them to remember which key opens which, and to retype the query
//! when the answer was not in the file after all.
//!
//! And how much code is here ([`counts`]), which shares no code with the
//! searching: it is the other question asked of the whole tree, and it is
//! here for that reason rather than for anything it borrows.

pub mod counts;
pub mod tree;

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
    /// The document being read: a file, or a page with words in it.
    File,
    /// Every file under the working directory.
    Project,
    /// The names the language server knows, across the project.
    Symbols,
}

impl Scope {
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

    /// What typing into a search of this radius does, for the row before
    /// anything has been typed.
    ///
    /// The whole sentence and not the tab's word, because the row is read
    /// on its own: a reader who has not looked up at the tabs is told
    /// `Search`, and told nothing about what.
    #[must_use]
    pub const fn before_typing(self) -> &'static str {
        match self {
            Self::File => "Search this file",
            Self::Project => "Search the project",
            Self::Symbols => "Search the project's symbols",
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
///
/// Always a pattern, a literal included, and one pattern compiled twice: by
/// `regex` for a line already in hand, and by ripgrep's matcher for a whole
/// file at a time. The same string and the same engine underneath, so the
/// walk and the view cannot disagree about a line -- which a literal found
/// by lowering each line and `str::find` did, about the few characters
/// whose lower case is not their case folding.
#[derive(Clone, Debug)]
pub struct Needle {
    /// What the reader typed.
    said: String,
    /// The pattern, for a line already in hand.
    pattern: Option<Regex>,
    /// The same pattern, for a whole file at a time.
    ///
    /// Compiled knowing that a match never crosses a line, which is what
    /// lets it look through a file at once rather than a line at a time --
    /// and is also why a pattern with a newline in it will not compile
    /// here, and so is not a pattern at all (`broken`), where it once
    /// compiled and quietly matched nothing.
    matcher: Option<grep_regex::RegexMatcher>,
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
        let mut needle = Self {
            said: said.to_string(),
            pattern: None,
            matcher: None,
            broken: false,
        };
        if said.is_empty() {
            return needle;
        }
        // Smart case unless the reader has said otherwise: one in lower
        // case matches either, one with a capital in it means it. Asked of
        // what they typed and not of the pattern's literals, which is where
        // this differs from ripgrep's own smart case: `\W` has a capital in
        // it, and a reader who typed one has typed a capital.
        let sensitive = how.sensitive || said.chars().any(char::is_uppercase);
        // Escaped where it is not a pattern, so whole words can be asked for
        // without the query becoming one: a reader searching for `a.b` with
        // words on means those three characters.
        let body = match how.regex {
            true => said.to_string(),
            false => regex::escape(said),
        };
        // `\b` around it, and the group so that a pattern with an
        // alternation in it has the boundaries around the whole of it
        // rather than around its first branch. Written here rather than
        // asked of ripgrep's `word`, which puts half-boundaries round it and
        // so answers differently for a query that starts with a `-`.
        let body = match how.word {
            true => format!("\\b(?:{body})\\b"),
            false => body,
        };
        let body = match sensitive {
            true => body,
            false => format!("(?i){body}"),
        };
        let matcher = grep_regex::RegexMatcherBuilder::new()
            // `^` and `$` at every line, because what it is handed is a whole
            // file: without this they are the file's ends, and `;$` found
            // the last line of each file and nothing else.
            .multi_line(true)
            .line_terminator(Some(b'\n'))
            // `$` before a `\r\n` as well, which is where a line read with
            // `str::lines` ends.
            .crlf(true)
            .build(&body);
        match (Regex::new(&body), matcher) {
            (Ok(pattern), Ok(matcher)) => {
                needle.pattern = Some(pattern);
                needle.matcher = Some(matcher);
            }
            (Err(error), _) => {
                tracing::debug!(%error, said, "not a pattern yet");
                needle.broken = true;
            }
            (_, Err(error)) => {
                tracing::debug!(%error, said, "not a pattern a line can hold");
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
    #[must_use]
    pub fn found_in(&self, line: &str) -> Option<Range<usize>> {
        let found = self.pattern.as_ref()?.find(line)?;
        let from = line[..found.start()].chars().count();
        Some(from..from + line[found.range()].chars().count())
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
/// `ignored` offers the files the tree has said to ignore as well, and
/// `hidden` the ones a system keeps out of sight. Two switches and not
/// one, because they keep two different things out: what a project said to
/// ignore, and what a convention says not to show. A reader after
/// `.github/workflows/ci.yml` is not asking to see `target`.
///
/// `.git` comes with the second, which is the cost of it meaning what it
/// says -- a directory whose files are a database is still a directory of
/// files, and an exception carved out here would be this walk deciding
/// which of the reader's hidden files they meant.
///
/// A thread because `WalkBuilder` is a blocking API, and the walk of a large
/// tree is long enough that the picker has to be usable while it runs.
pub fn spawn_walk(
    root: &Path,
    wanted: cancel::Wanted,
    ignored: bool,
    hidden: bool,
    sender: impl Sink<Event>,
) {
    let root = root.to_path_buf();
    obelus_runtime::handle().spawn_blocking(move || {
        // The files the tree keeps, first and on their own: they are what
        // a reader is usually after, and this is the quick walk -- it is
        // the one that does not descend into `target`.
        let mut sent = HashSet::new();
        if !walk(&root, true, hidden, &wanted, &sender, &mut sent) || !ignored {
            return;
        }
        // And then the ones it does not keep, which are whatever the
        // first walk did not send. Asking a second matcher whether a path
        // is ignored would be asking the same question twice and leaving
        // the two answers free to differ; this way "ignored" means
        // exactly "the walk that obeys the rules did not offer it".
        walk(&root, false, hidden, &wanted, &sender, &mut sent);
    });
}

/// A walk from `from`, obeying the ignore rules or not, and showing what a
/// system hides or not.
///
/// One, for the flat list and for the tree, so what counts as a file worth
/// showing has one answer at both depths and in both shapes of the list.
fn walker(from: &Path, obeying: bool, hidden: bool) -> WalkBuilder {
    let mut walk = WalkBuilder::new(from);
    walk.git_ignore(obeying)
        .git_global(obeying)
        .git_exclude(obeying)
        .ignore(obeying)
        .parents(obeying)
        // The other way round from every switch above it: `hidden(true)`
        // is what skips them, so what the reader turned on is what this
        // turns off.
        //
        // What counts as hidden is `ignore`'s answer and not one written
        // here, which is why the setting says it in its own words: a name
        // beginning with a dot everywhere, and on Windows a file the
        // system has marked as well. A rule of Obelus's own would be a
        // second answer to a question the walk is already answering, and
        // the two would differ on exactly the platform nobody tests on by
        // hand.
        .hidden(!hidden);
    walk
}

/// One walk over the tree, sending what it finds in batches.
///
/// `obeying` says whether the ignore rules apply. The walk that obeys them
/// remembers in `sent` what it offered; the walk that does not obey them
/// leaves those out and sends the rest, marked as ignored.
///
/// `hidden` says whether the ones a system keeps out of sight come too,
/// and it is the reader's switch rather than this walk's own: both walks
/// obey it, so "ignored" keeps meaning exactly "the walk that obeys the
/// rules did not offer it" whichever way it is set.
///
/// Returns whether there is still anybody to send to.
fn walk(
    root: &Path,
    obeying: bool,
    hidden: bool,
    wanted: &cancel::Wanted,
    sender: &impl Sink<Event>,
    sent: &mut HashSet<PathBuf>,
) -> bool {
    let mut batch: Vec<PathBuf> = Vec::with_capacity(WALK_BATCH);
    let walk = walker(root, obeying, hidden);

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

/// Scans every file under `root` for what `needle` is looking for, on
/// threads of its own.
///
/// What counts as a match is [`Needle`]'s to say, which is also what the
/// search of the open file asks and what the view marks. What a file *says*
/// is `obelus_encoding`'s, which is also what opening it asks: a file in GBK
/// is searched as the text a reader sees when they open it, on the lines
/// they see it on, and a file that will not open is not searched.
///
/// The walk is `ignore`'s parallel one, a thread a core, and each file is
/// looked through whole by ripgrep's searcher rather than a line at a time
/// -- most files have nothing in them, and finding that out is one pass of
/// a fast search rather than a copy of every line. Which file is answered
/// first is now whichever thread got there: the rows of one file stay in
/// its order, the files do not, and when there are more than [`MOST`] which
/// ones are kept is not the same twice.
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
/// stop a walk with -- a walk in `ignore` cannot be interrupted from
/// outside -- so every thread has to ask. [`obelus_runtime::cancel`] is
/// where that asking lives.
///
/// `ignored` searches the files the project has said to ignore as well, which
/// is the reader's `ignored_files` and not a question of this walk's own: a
/// file list that offers `target` beside a search that cannot see into it
/// is two answers about one tree.
pub fn spawn_scan(
    root: &Path,
    needle: &Needle,
    wanted: cancel::Wanted,
    ignored: bool,
    hidden: bool,
    sender: impl Sink<Event> + Clone,
) {
    let root = root.to_path_buf();
    let needle = needle.clone();
    obelus_runtime::handle().spawn_blocking(move || {
        // A needle with nothing to look for finds nothing, and says so.
        let Some(matcher) = needle.matcher.clone() else {
            let _ = sender.send(Event::Matches {
                generation: wanted.generation(),
                hits: Vec::new(),
                done: true,
            });
            return;
        };
        let found = Found::new(wanted.clone(), sender);

        let mut walk = WalkBuilder::new(&root);
        walk.git_ignore(!ignored)
            .git_global(!ignored)
            .git_exclude(!ignored)
            .ignore(!ignored)
            .parents(!ignored)
            // The same switch the list of files obeys: a reader who turned
            // these on is reading them, and a search that skipped them
            // would be looking at a different project from the one the
            // list is offering.
            .hidden(!hidden);
        walk.build_parallel().run(|| {
            let matcher = matcher.clone();
            let found = &found;
            let root = &root;
            let wanted = wanted.clone();
            // One per thread: a searcher keeps its buffers between files.
            // Told nothing about encodings or binary files, because what it
            // is handed has already been read the way a buffer reads it.
            //
            // A line ends where the matcher's does, at `\r\n` or `\n`: the
            // two have to agree, and the searcher refuses if they do not.
            let mut searcher = grep_searcher::SearcherBuilder::new()
                .line_terminator(grep_matcher::LineTerminator::crlf())
                .line_number(true)
                .binary_detection(grep_searcher::BinaryDetection::none())
                .bom_sniffing(false)
                .build();
            Box::new(move |entry| {
                use ignore::WalkState;
                // Per file rather than per line: a file is the unit of work
                // here, and reading the flag for every line of a large file
                // would be a cost of its own.
                if !wanted.still() || found.finished() {
                    return WalkState::Quit;
                }
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                    return WalkState::Continue;
                }
                if entry.metadata().is_ok_and(|data| data.len() > BIGGEST_FILE) {
                    return WalkState::Continue;
                }
                let Ok(bytes) = std::fs::read(entry.path()) else {
                    return WalkState::Continue;
                };
                // What will not open is not searched: a binary file, or one
                // in no encoding Obelus can write back.
                let Some(read) = obelus_encoding::read(&bytes) else {
                    return WalkState::Continue;
                };
                let mut lines = Vec::new();
                let searched = searcher.search_slice(
                    &matcher,
                    read.text.as_bytes(),
                    Lines { into: &mut lines },
                );
                if let Err(error) = searched {
                    tracing::debug!(%error, path = %entry.path().display(), "not searched");
                    return WalkState::Continue;
                }
                if lines.is_empty() {
                    return WalkState::Continue;
                }
                let path = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or_else(|_| entry.path())
                    .to_path_buf();
                let hits = lines.into_iter().map(|(line, text)| Hit {
                    path: path.clone(),
                    line,
                    text,
                });
                match found.add(hits) {
                    true => WalkState::Continue,
                    false => WalkState::Quit,
                }
            })
        });
        found.finish();
    });
}

/// What every thread of one scan has found, and the one place it is sent
/// from.
///
/// Behind a lock so that the batches keep the size they had when there was
/// one thread -- a message per file with something in it would redraw the
/// list once per file -- and so that "done" is said once, last, and never
/// followed by anything.
struct Found<S> {
    wanted: cancel::Wanted,
    sender: S,
    state: std::sync::Mutex<Sending>,
    /// Whether the scan has said all it is going to, for the threads to ask
    /// without taking the lock.
    finished: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct Sending {
    batch: Vec<Hit>,
    count: usize,
    /// Whether `done` has gone, or nobody is listening any more: either way
    /// there is nothing more to send.
    over: bool,
}

impl<S: Sink<Event>> Found<S> {
    fn new(wanted: cancel::Wanted, sender: S) -> Self {
        Self {
            wanted,
            sender,
            state: std::sync::Mutex::new(Sending::default()),
            finished: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Takes one file's hits. Returns whether to go on.
    fn add(&self, hits: impl Iterator<Item = Hit>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.over {
            return false;
        }
        for hit in hits {
            state.batch.push(hit);
            state.count += 1;
            let done = state.count >= MOST;
            if (state.batch.len() >= BATCH || done) && (!self.send(&mut state, done) || done) {
                return false;
            }
        }
        true
    }

    /// Sends what is waiting, saying whether it is the last of it. Returns
    /// whether anybody is still listening.
    fn send(&self, state: &mut Sending, done: bool) -> bool {
        // A scan the reader has typed past says nothing at all, not even
        // that it is finished: nobody is waiting for it.
        if !self.wanted.still() {
            state.over = true;
            self.finished
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return false;
        }
        let hits = std::mem::replace(&mut state.batch, Vec::with_capacity(BATCH));
        let sent = self.sender.send(Event::Matches {
            generation: self.wanted.generation(),
            hits,
            done,
        });
        if done || sent.is_err() {
            state.over = true;
            self.finished
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        sent.is_ok()
    }

    /// The walk is over: what is left, and that it is the last.
    fn finish(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if !state.over {
            self.send(&mut state, true);
        }
    }
}

/// Where a searcher puts the lines it finds in one file: each line's number
/// and the line as a row shows it.
struct Lines<'a> {
    into: &'a mut Vec<(usize, String)>,
}

impl grep_searcher::Sink for Lines<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _: &grep_searcher::Searcher,
        found: &grep_searcher::SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        // Counted from one by the searcher and from zero by a row.
        let number = found
            .line_number()
            .and_then(|number| usize::try_from(number).ok())
            .unwrap_or(1)
            .saturating_sub(1);
        // Valid, because what was searched was a `str`; and a line, because
        // the matcher was told no match crosses one.
        let line = String::from_utf8_lossy(found.bytes());
        let trimmed = line.trim();
        let text = match trimmed.char_indices().nth(WIDEST_LINE) {
            Some((at, _)) => trimmed[..at].to_string(),
            None => trimmed.to_string(),
        };
        self.into.push((number, text));
        Ok(true)
    }
}
