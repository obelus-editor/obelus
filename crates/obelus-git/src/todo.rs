//! What a reader means to come back to.
//!
//! An *obelus* is the mark a scholar put beside a line they doubted. This is
//! that mark, written down: a note made while reading, kept with the project it
//! is about rather than in the reader's own home, because it is about this
//! project and the next person to open it has the same questions.
//!
//! A note may carry a place -- a file and a line -- or carry none, and both
//! are ordinary. "This cache is wrong" belongs to a line; "wire the counts
//! project up to the search" belongs to the project.
//!
//! Nothing here draws and nothing here decides what a key does. What is here
//! is the note, the file it lives in, and the one hard part: a line written
//! down last week is not the line it was, and finding it again is a question
//! for git.

use std::path::{Path, PathBuf};

/// How far one note is indented under the one above it.
///
/// A number about the notes rather than about any one way of showing them:
/// the page indents by it, the caret is measured against it, and what an
/// agent is told the list looks like is written with it. One figure, or
/// three that drift.
pub const INDENT: u16 = 2;

use obelus_text::coordinates::LineNumber;

/// Where a project keeps what it means to come back to.
///
/// In Obelus's own state directory, named after the project rather than
/// after the checkout: a repository and its worktrees are one project, and
/// a reader with three of them open means to come back to one list.
///
/// It used to sit in the project's own `.obelus`, beside the settings, so
/// that the next person to open the project would find the same questions
/// already asked. They never did: `.obelus` is a directory readers
/// gitignore -- Obelus's own repository is one of them -- so the notes were
/// one reader's own already. Being one reader's own, they were also one
/// *checkout's*, which is the half of it that was actually wrong.
///
/// `None` for a tree that has gone, which names no project -- see
/// [`crate::project`]. Beside a checkout as well: a file there is a file in
/// a directory that is not there, and making it would make the checkout
/// again around it.
#[must_use]
pub fn path(root: &Path) -> Option<PathBuf> {
    let Some(state) = obelus_logging::state_directory() else {
        // Nowhere of this machine's own to keep them, so the checkout is
        // the only thing left to keep them beside. A reader on such a
        // machine has a checkout's notes rather than a project's, which is
        // the most that can be said there.
        return beside_a_checkout(root);
    };
    Some(
        state
            .join("todo")
            .join(format!("{}.toml", crate::project(root)?)),
    )
}

/// Where they go on a machine with nowhere of its own to keep state.
///
/// A checkout's rather than a project's, because a checkout is the only
/// thing there is to name them after here.
fn beside_a_checkout(root: &Path) -> Option<PathBuf> {
    (!crate::is_gone(root)).then(|| root.join(".obelus").join("todo.toml"))
}

/// [`GONE`], as the error a write fails with.
fn gone() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, GONE)
}

/// What reading or writing the notes of a tree that has gone says.
///
/// Not "there are none": they are where they always were, under the name
/// of a project this path can no longer be shown to belong to.
const GONE: &str = "The tree these notes belong to has gone";

/// The place a note is about, as it was written down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct At {
    /// The file, relative to the project.
    pub path: PathBuf,
    /// The line, as it was when the note was made.
    pub line: LineNumber,
    /// The commit the file was at when the note was made, where the project is
    /// a git repository.
    ///
    /// Not the note's date and not the file's: what the line number means is
    /// "line 412 of the file *as that commit had it*", and a commit is the
    /// only name for a version of a file that is still there next week.
    /// `None` for a project git has never heard of, where the line is taken at
    /// its word because there is nothing to check it against.
    pub commit: Option<gix::ObjectId>,
}

/// What names one note, for as long as the note exists.
///
/// A note is found by where it is in the list everywhere else, which is
/// fine for a list somebody is looking at and no use at all for anything
/// that has to still mean the same note next week: insert one above and
/// every position below it is about a different note. So a note carries a
/// name of its own, written down beside it.
///
/// Eight characters of Crockford's base32, which is the alphabet without
/// the four letters a person copying by hand gets wrong -- no `I`, `L`, `O`
/// or `U`. Short enough to read out, and the file is the reader's as much
/// as it is Obelus's.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteId(String);

/// The alphabet, without the letters that read as digits.
const CROCKFORD: [u8; 32] = *b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many of them a name is.
const NAME_LENGTH: usize = 8;

impl NoteId {
    /// Mints one.
    ///
    /// No dependency for it. What randomness needs to buy here is only that
    /// two notes made in one second, in two Obeluses, on one project, do not
    /// collide -- the file is read back and a clash is minted over anyway,
    /// so this is a cheap first line rather than the only one. The clock
    /// separates seconds, a counter separates notes within one, and the
    /// hasher's per-process key separates two Obeluses.
    #[must_use]
    pub fn mint() -> Self {
        use std::hash::{BuildHasher as _, Hasher as _};

        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let counted = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |gone| gone.as_nanos() as u64);

        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(since);
        hasher.write_u64(counted);
        let mut bits = hasher.finish();

        let name = (0..NAME_LENGTH)
            .map(|_| {
                let at = (bits & 0x1f) as usize;
                bits >>= 5;
                CROCKFORD[at] as char
            })
            .collect();
        Self(name)
    }

    /// What it says, which is what the file holds.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// One read back out of the file, if it is one.
    ///
    /// Anything else is treated as no name at all and a fresh one is minted
    /// over it: a name is only worth having while everything agrees what it
    /// looks like, and a file somebody has edited by hand is exactly where
    /// that stops being true.
    #[must_use]
    pub fn read(said: &str) -> Option<Self> {
        let right = said.len() == NAME_LENGTH
            && said.bytes().all(|character| CROCKFORD.contains(&character));
        right.then(|| Self(said.to_string()))
    }
}

impl std::fmt::Display for NoteId {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0)
    }
}

/// One note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// What names it, for as long as it exists.
    pub id: NoteId,
    /// What it says. Its first line is the row.
    ///
    /// Never empty in the file: a note that says nothing is not written
    /// down. It can be empty here, and briefly is -- the note the reader
    /// has just started, and the one whose words they have cleared on the
    /// way to throwing it away -- because the page has to have somewhere
    /// for them to type before there is anything to type.
    pub said: String,
    /// Whether it is done.
    ///
    /// Kept rather than taken away, because a list of what is done is how a
    /// reader tells "I decided against it" from "I never got to it".
    pub done: bool,
    /// The place it is about, if it is about one.
    pub at: Option<At>,
    /// How far under the note above it this one sits.
    ///
    /// A number and the order the file already has, rather than a name for
    /// whoever the parent is. The file is the reader's as much as Obelus's
    /// and an outline is something they can write by hand; a graph of names
    /// is not. And the order is already the one thing about a note they can
    /// rely on -- a parent named beside each note would be a second
    /// authority on what comes after what, which is one more than a list can
    /// have.
    ///
    /// What hangs under a note is therefore the run straight after it that
    /// is deeper than it: see [`Todo::under`]. Nothing else records it, so
    /// nothing else can disagree about it.
    pub depth: u16,
}

impl Note {
    /// The line a row shows: the first, and no more.
    #[must_use]
    pub fn title(&self) -> &str {
        self.said.lines().next().unwrap_or("")
    }

    /// The rest of it, which is what folding it open shows.
    #[must_use]
    pub fn body(&self) -> Vec<&str> {
        self.said.lines().skip(1).collect()
    }

    /// Whether there is anything behind the row.
    #[must_use]
    pub fn folds(&self) -> bool {
        self.said.lines().nth(1).is_some()
    }
}

/// What an agent asked Obelus to do to the notes.
///
/// Added, ticked, reworded. Taking one away is the reader's and stays
/// theirs: `done` is already how a list keeps what was decided against, so
/// an agent has no need of the one act that leaves nothing behind.
///
/// Rewording was kept out for a while on an argument that sounds like the
/// same one: an agent should only do what loses nothing, and replacing what
/// a reader wrote loses it. What that missed is that a note is the reader's
/// *question*, and a question the work turns out not to be about is wrong
/// on the one line they read. Hanging a correction under it leaves the list
/// saying two things with the wrong one on top. So the words may be
/// replaced -- and what pays for it is the asking, which is the agent's:
/// the reader sees the new words and agrees to them before the tool is
/// called at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doing {
    /// Write these down, one note each.
    Add {
        /// What they say and how they sit under each other, counted from
        /// the top of the batch rather than from the list's own top: an
        /// agent knows the shape of what it is writing and not where in
        /// somebody else's list it will land.
        notes: Vec<(String, u16)>,
        /// The note they hang under, if they hang under one.
        ///
        /// They go after the whole of what is already under it, which is
        /// where the key that starts a note puts one: between a note and
        /// its children is a place that adopts what is put there.
        under: Option<NoteId>,
    },
    /// Tick this one off, by the name it answers to.
    Finish(NoteId),
    /// Make this one say something else.
    Reword {
        /// The note, by the name it answers to. It is still that note
        /// afterwards: where it points, whether it is done and what hangs
        /// under it are facts about the note rather than about its words,
        /// and an agent rewording one has said nothing about any of them.
        note: NoteId,
        /// The whole of what it says from now on, in place of what it
        /// says. Not a line to add: `Add` with `under` is how something
        /// gets added.
        said: String,
    },
}

/// One thing done to the notes, said so that it still means what it meant
/// against a file somebody else has written since.
///
/// The page used to hand the notes over as it had them, and the file was
/// written from that -- every note in it, including the ones it read ten
/// minutes ago. A second Obelus on the same project writes this file too, and
/// it is not an exotic thing to have running: reading two things at once is
/// what a second window is *for*. A whole file written from a held copy is
/// that other window's ten minutes taken back out, with nobody told.
///
/// So what travels to the file is not the notes, it is the act. It names
/// the note it is about, and applying it to the file *as it is at the
/// moment of writing* puts the reader's change in beside somebody else's
/// rather than over it.
///
/// Every one of these but [`Change::Put`] is about a note that is already
/// in the file, and does nothing at all where that note has gone. That is
/// the whole of what keeps a deleted note deleted: a single change that
/// both updated and inserted would bring a note the other window
/// deliberately took away back from the dead, the moment the reader typed
/// into the copy still on their own screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A note this Obelus started, put in.
    ///
    /// The one change that may add, because the note is the reader's own
    /// and has never been in anybody's file. Idempotent all the same: a
    /// name the file already has is changed rather than doubled, so the
    /// same `Put` arriving twice -- queued before a write and drained after
    /// it -- leaves one note and not two.
    ///
    /// `after` is the note it goes behind, and behind the whole of what
    /// hangs under that one: a note started from a parent is the next thing
    /// at the parent's level, not the first of its children. `None`, or a
    /// name the file no longer has, puts it at the end -- where a note with
    /// nothing left to hang behind belongs.
    Put {
        /// The note itself, at the depth the page gave it.
        note: Note,
        /// What it goes behind.
        after: Option<NoteId>,
    },
    /// What one that is already there says.
    Said {
        /// Which note.
        id: NoteId,
        /// The whole of what it says now.
        said: String,
    },
    /// Whether one that is already there is done.
    Done {
        /// Which note.
        id: NoteId,
        /// Which way it was just put.
        done: bool,
    },
    /// Take one away.
    Remove {
        /// Which note.
        id: NoteId,
        /// Whether what hangs under it goes with it.
        ///
        /// The key that takes a note away means the whole of it, children
        /// and all: a note and what hangs under it are one thing on the
        /// screen. A note that goes because the reader cleared its words
        /// means only itself, and its children come up a level -- emptying
        /// one note is not saying anything about another.
        under: bool,
    },
    /// Move one, and what hangs under it, over the note beside it.
    ///
    /// Said as "over that one" rather than "to position four", because a
    /// position is about a list that has not changed since and a name is
    /// about the note. Where the neighbour it was to step over has itself
    /// gone, the move does not apply: the reader was moving this note past
    /// *that* one, and there is no honest second guess at what they meant.
    Move {
        /// Which note.
        id: NoteId,
        /// The one it steps over.
        over: NoteId,
        /// Which way, which is what "over" means: in front of that one, or
        /// behind it and everything under it.
        up: bool,
    },
    /// Take one, and what hangs under it, a level in or out.
    Shift {
        /// Which note.
        id: NoteId,
        /// Out towards the margin, rather than in under the note above.
        out: bool,
    },
}

impl Change {
    /// The note this carries words for, where it carries any.
    ///
    /// What tells a change that was only ever going to be lost from one
    /// that has something in it nobody else has. A tick or a move that did
    /// not reach the file is a tick that did not happen; words that did not
    /// reach the file are still on somebody's screen, and are the only copy
    /// of themselves there is.
    #[must_use]
    pub const fn words(&self) -> Option<&NoteId> {
        match self {
            Self::Put { note, .. } => Some(&note.id),
            Self::Said { id, .. } => Some(id),
            Self::Done { .. } | Self::Remove { .. } | Self::Move { .. } | Self::Shift { .. } => {
                None
            }
        }
    }
}

/// How deep a note may sit.
///
/// Four levels, counted from nothing. A list in a terminal is as wide as
/// the terminal, and what indenting costs is taken from the one column the
/// reader is actually reading -- so the depth has to stop somewhere, and it
/// may as well stop where an outline of things to do stops being one.
pub const DEEPEST: u16 = 3;

/// Every note a project has, in the order they were written.
///
/// Written order, not sorted: a reader who ticks something off does not want
/// the list to reorder itself underneath them, and a note's place in the
/// list is the one thing about it they can rely on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Todo {
    /// The notes.
    pub notes: Vec<Note>,
    /// Whether reading gave any of them a name it did not have.
    ///
    /// Reading is not allowed to write -- a function called `read` that
    /// touches the disk is one nobody expects to -- so it says instead, and
    /// whoever asked for the notes puts them back. Without that the names
    /// would be minted afresh on every open and nothing could be keyed to
    /// one.
    pub minted: bool,
}

/// What reading a project's notes found.
///
/// "There is no file" and "there is a file Obelus cannot read" are different
/// answers, and they were the same one. Both came back as no notes -- and no
/// notes is a thing Obelus will happily write down: the page opens empty,
/// the reader writes one note in a list they cannot see is not their list,
/// and the file they had is replaced by it. A comma in the wrong place was
/// enough. What is remembered *beside* the notes went the same way: the
/// table of which conversation is about which note is swept against the
/// names the file has, and an empty list of names means every conversation
/// in this project is forgotten.
///
/// The same three answers the settings give, for the same reason and in the
/// same shape -- `obelus_config::Reading`, which had this out first. A file
/// Obelus cannot read is the reader's file all the same, and the one thing
/// it must never do is write over it.
///
/// This is about the file as a whole, not about what is in it. A file that
/// *parses* is read as generously as ever: a note with no name is given one,
/// a depth with nothing over it comes up to where it can hang, a note that
/// says nothing is dropped. Those are a file somebody wrote by hand, which
/// is a thing this file is for.
#[derive(Clone, Debug)]
pub enum Reading {
    /// There is none yet, which is where every project starts.
    Nothing,
    /// Here they are.
    Notes(Todo),
    /// There is one and it could not be read, with what went wrong.
    Unreadable(String, Option<obelus_text::coordinates::Span>),
}

impl Reading {
    /// The notes, where having none is an answer the caller can live with.
    ///
    /// `None` for a file that would not read. Spelled out at every call
    /// rather than folded in here, because `unwrap_or_default` on this is a
    /// caller saying out loud that nothing is a fine answer -- true of a
    /// count in a status row, and never true of anything that writes.
    #[must_use]
    pub fn notes(self) -> Option<Todo> {
        match self {
            Self::Nothing => Some(Todo::default()),
            Self::Notes(todo) => Some(todo),
            Self::Unreadable(..) => None,
        }
    }
}

/// What is in a project's file.
#[must_use]
pub fn read(root: &Path) -> Reading {
    let Some(path) = path(root) else {
        return Reading::Unreadable(GONE.to_string(), None);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Not there yet is the ordinary case and not a failure: the file is
        // written the first time a reader writes a note down.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Reading::Nothing;
        }
        Err(error) => return Reading::Unreadable(error.to_string(), None),
    };
    match text.parse::<toml::Table>() {
        Ok(table) => Reading::Notes(Todo::from_table(&table)),
        // Where, because this is a file a reader can open: they write
        // notes into it from the page and they edit it by hand, and being
        // told the whole list will not read without being told which line
        // is a reader reading it all.
        Err(error) => {
            let at = error
                .span()
                .map(|bytes| obelus_text::span_of_bytes(&text, &bytes));
            Reading::Unreadable(error.to_string(), at)
        }
    }
}

impl Todo {
    fn from_table(table: &toml::Table) -> Self {
        let mut notes = Vec::new();
        let mut minted = false;
        let Some(written) = table.get("todo").and_then(toml::Value::as_array) else {
            return Self { notes, minted };
        };
        for value in written {
            let Some(note) = value.as_table() else {
                continue;
            };
            let said = trimmed(
                note.get("said")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default(),
            );
            // A note that says nothing is a row a reader cannot tell from an
            // empty one, and there is nothing to do about it.
            if said.trim().is_empty() {
                continue;
            }
            let at = note.get("at").and_then(toml::Value::as_str).map(|at| At {
                path: PathBuf::from(at),
                // Written one-based, because that is how a reader counts
                // lines and how every other number Obelus writes down is
                // meant. Held zero-based, which is how it counts them.
                line: LineNumber::new(
                    note.get("line")
                        .and_then(toml::Value::as_integer)
                        .and_then(|line| usize::try_from(line).ok())
                        .unwrap_or(1)
                        .saturating_sub(1),
                ),
                commit: note
                    .get("commit")
                    .and_then(toml::Value::as_str)
                    .and_then(|id| gix::ObjectId::from_hex(id.as_bytes()).ok()),
            });
            // A note with no name, or with one somebody else in this file
            // already has: both get a fresh one, and both mean the file
            // wants writing back. Duplicates are what copying a block by
            // hand produces, and two notes with one name is worse than
            // either having a new one -- whatever was keyed to it would
            // follow whichever came first.
            let named = note
                .get("id")
                .and_then(toml::Value::as_str)
                .and_then(NoteId::read)
                .filter(|id| !notes.iter().any(|note: &Note| note.id == *id));
            let id = match named {
                Some(id) => id,
                None => {
                    minted = true;
                    NoteId::mint()
                }
            };
            // One deeper than the note above, at the most. A file saying
            // otherwise -- a first note already indented, a jump from none
            // to two -- describes a note hanging under one that is not
            // there, and the honest reading of it is the nearest one that
            // is. Clamped here rather than refused, for the reason a note
            // with no name is given one: the file is written by hand as
            // well, and losing the list over an extra space would be the
            // worse answer.
            //
            // Not written back on its own. Reading always clamps, so what
            // is on disk is corrected by the next change to the file rather
            // than by having read it -- and `read` is not a function anybody
            // expects to touch the disk.
            let under = notes
                .last()
                .map_or(0, |last: &Note| last.depth + 1)
                .min(DEEPEST);
            let depth = note
                .get("depth")
                .and_then(toml::Value::as_integer)
                .and_then(|depth| u16::try_from(depth).ok())
                .unwrap_or(0)
                .min(under);
            notes.push(Note {
                id,
                said,
                done: note
                    .get("done")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                at,
                depth,
            });
        }
        Self { notes, minted }
    }

    /// How many notes hang under the one at `at`.
    ///
    /// The run straight after it that is deeper than it is, which -- with
    /// the order the file has and a depth on each note -- is the whole of
    /// what "under" means here. Every key that has to treat a note and what
    /// hangs under it as one thing asks this: moving one, taking one away,
    /// putting a new one after one.
    ///
    /// Zero for a note with nothing under it, and for a position past the
    /// end. A run is contiguous because a note deeper than this one cannot
    /// belong to anything else: whatever ends the run is at this depth or
    /// shallower, and is therefore the next thing at this level or above.
    #[must_use]
    pub fn under(&self, at: usize) -> usize {
        let Some(note) = self.notes.get(at) else {
            return 0;
        };
        self.notes[at + 1..]
            .iter()
            .take_while(|below| below.depth > note.depth)
            .count()
    }

    /// Where the note by this name is, if it is still here.
    ///
    /// The one way anything outside finds a note. A position is about a
    /// list nobody has touched since; a name is about the note, which is
    /// the only thing that still means what it meant after somebody else
    /// wrote the file.
    #[must_use]
    pub fn find(&self, id: &NoteId) -> Option<usize> {
        self.notes.iter().position(|note| note.id == *id)
    }

    /// How deep a note put at this place may be.
    ///
    /// One deeper than what is above it, which is the whole rule: a note
    /// deeper than that hangs under nothing, and writing one would write a
    /// file that reads back a level shallower -- the note moving on its own
    /// between one open and the next.
    #[must_use]
    pub fn room_at(&self, at: usize) -> u16 {
        self.notes
            .get(at.wrapping_sub(1))
            .map_or(0, |above| above.depth + 1)
    }

    /// Where the note before this one at the same depth starts.
    ///
    /// `None` where there is none: the first child of a note has nothing
    /// above it at its own level, and neither has the first note of all.
    /// Walked backwards rather than counted, because what ends the search is
    /// the first note *shallower* than this one -- that is the parent, and
    /// above it is somebody else's list.
    #[must_use]
    pub fn before_it(&self, at: usize) -> Option<usize> {
        let depth = self.notes.get(at)?.depth;
        for (index, note) in self.notes[..at].iter().enumerate().rev() {
            if note.depth < depth {
                return None;
            }
            if note.depth == depth {
                return Some(index);
            }
        }
        None
    }

    /// And where the note after this one at the same depth starts.
    #[must_use]
    pub fn after_it(&self, at: usize) -> Option<usize> {
        let depth = self.notes.get(at)?.depth;
        let next = at + 1 + self.under(at);
        self.notes
            .get(next)
            .filter(|note| note.depth == depth)
            .map(|_| next)
    }

    /// Whether the note at `at` has anywhere to go, in or out.
    ///
    /// The rules, in the one place they are written: the top, the note
    /// above, and the deepest a note may be. Both the key that does it and
    /// whatever says whether the key would do anything ask this, so a key
    /// drawn lit is a key that moves something -- two copies of three rules
    /// would be a hint that goes wrong on its own.
    #[must_use]
    pub fn can_shift(&self, at: usize, out: bool) -> bool {
        let Some(depth) = self.notes.get(at).map(|note| note.depth) else {
            return false;
        };
        if out {
            return depth > 0;
        }
        let deepest = self
            .notes
            .iter()
            .skip(at)
            .take(1 + self.under(at))
            .map(|note| note.depth)
            .max()
            .unwrap_or(depth);
        depth < self.room_at(at) && deepest < DEEPEST
    }

    /// Does one change to these notes, and says whether the note it was
    /// about was still there to do it to.
    ///
    /// `false` is the answer that matters: it means somebody else took that
    /// note away between this Obelus reading the file and writing to it, and
    /// whoever asked has a reader to tell -- a key that appears to do
    /// nothing is the worst way to find out. [`Change::Put`] is always
    /// `true`, because it is about a note that is the reader's own.
    ///
    /// Depths are clamped on the way in, not checked: every caller of this
    /// is a key or an agent working from a list that may have moved, and the
    /// file has to come back out of [`Todo::read`] saying what was written
    /// into it.
    pub fn apply(&mut self, change: &Change) -> bool {
        match change {
            Change::Put { note, after } => {
                // Already there, so this is the same note arriving a second
                // time -- the change was queued before the last write and
                // drained after it. Changed rather than added: two notes by
                // one name is the one thing reading the file cannot repair.
                if let Some(at) = self.find(&note.id) {
                    let depth = note.depth.min(self.room_at(at)).min(DEEPEST);
                    if let Some(there) = self.notes.get_mut(at) {
                        there.said.clone_from(&note.said);
                        there.done = note.done;
                        there.at.clone_from(&note.at);
                        there.depth = depth;
                    }
                    return true;
                }
                // Behind the whole of what hangs under it: the note was
                // started from that one and is the next thing at its level.
                // Between it and its children, it would have been adopted by
                // it without the reader asking for a child at all -- and the
                // children it would be adopted over may be ones this Obelus
                // has never seen, put there by the other window.
                let at = after
                    .as_ref()
                    .and_then(|after| self.find(after))
                    .map_or(self.notes.len(), |at| at + 1 + self.under(at));
                let mut note = note.clone();
                note.depth = note.depth.min(self.room_at(at)).min(DEEPEST);
                self.notes.insert(at, note);
                true
            }
            Change::Said { id, said } => match self.find(id) {
                Some(at) => {
                    if let Some(note) = self.notes.get_mut(at) {
                        note.said.clone_from(said);
                    }
                    true
                }
                None => false,
            },
            Change::Done { id, done } => match self.find(id) {
                Some(at) => {
                    if let Some(note) = self.notes.get_mut(at) {
                        note.done = *done;
                    }
                    true
                }
                None => false,
            },
            Change::Remove { id, under } => {
                let Some(at) = self.find(id) else {
                    return false;
                };
                let below = self.under(at);
                match under {
                    true => {
                        self.notes.drain(at..at + 1 + below);
                    }
                    false => {
                        self.notes.remove(at);
                        // Up a level, because what they hung under has
                        // gone: left where they were they would read as
                        // children of whatever happens to be above now.
                        for note in self.notes.iter_mut().skip(at).take(below) {
                            note.depth = note.depth.saturating_sub(1);
                        }
                    }
                }
                true
            }
            Change::Move { id, over, up } => {
                let (Some(at), Some(neighbour)) = (self.find(id), self.find(over)) else {
                    return false;
                };
                let span = 1 + self.under(at);
                // In front of it, or behind it and the whole of what hangs
                // under it: stepping over one note at a time would put this
                // one in the middle of somebody else's children.
                let to = match up {
                    true => neighbour,
                    false => neighbour + 1 + self.under(neighbour),
                };
                let moved: Vec<Note> = self.notes.drain(at..at + span).collect();
                // Past the hole the drain left, where it was after us.
                let to = match to > at {
                    true => to.saturating_sub(span),
                    false => to,
                };
                self.notes.splice(to..to, moved);
                true
            }
            Change::Shift { id, out } => {
                let Some(at) = self.find(id) else {
                    return false;
                };
                // Refusing is silent, and is not this answer: the note is
                // there, it is simply already as far in as the one above it
                // -- which is on the reader's screen, one row up.
                if !self.can_shift(at, *out) {
                    return true;
                }
                let below = self.under(at);
                for note in self.notes.iter_mut().skip(at).take(1 + below) {
                    note.depth = match out {
                        true => note.depth.saturating_sub(1),
                        false => note.depth + 1,
                    };
                }
                true
            }
        }
    }

    /// Writes them back, making the directory if it is not there.
    ///
    /// The whole file every time, which is only safe for notes that were
    /// read a moment ago: anything that has been *held* must go through
    /// [`change`] instead, which reads the file again under a lock and does
    /// the reader's act to what it finds. This is what that falls back on
    /// when the lock cannot be had.
    ///
    /// Through a name beside it and then a rename, the way the settings are
    /// written: a crash halfway through leaves the old file rather than half
    /// of the new one.
    ///
    /// Not public. Writing the notes from a copy is the bug this module
    /// spent a while having, and an outside caller holding one is exactly
    /// how it came back.
    fn write(&self, root: &Path) -> std::io::Result<()> {
        let path = path(root).ok_or_else(gone)?;
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        // This process's own name beside it and not one every Obelus shares:
        // two writing at once into one shared name truncate each other's
        // half-written file, and the first rename takes the other's away.
        let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
        std::fs::write(&beside, self.to_toml())?;
        std::fs::rename(&beside, &path)
    }

    /// What the file says, as text.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        // A note that says nothing is not written down. It is a real thing
        // on the page -- the one the reader has just started, or the one
        // whose words they have cleared -- and it is nothing at all in a
        // file: an agent asking for the list would be handed a blank
        // entry, and a reader coming back would find a note that says
        // nothing about anything. The page keeps it; leaving drops it.
        for note in self
            .notes
            .iter()
            .filter(|note| !note.said.trim().is_empty())
        {
            out.push_str("[[todo]]\n");
            out.push_str(&format!("id = \"{}\"\n", note.id));
            out.push_str(&format!("said = {}\n", quoted(&note.said)));
            out.push_str(&format!("done = {}\n", note.done));
            out.push_str(&format!("depth = {}\n", note.depth));
            if let Some(at) = &note.at {
                out.push_str(&format!(
                    "at = {}\n",
                    quoted(&at.path.display().to_string())
                ));
                out.push_str(&format!("line = {}\n", at.line.get() + 1));
                if let Some(commit) = at.commit {
                    out.push_str(&format!("commit = \"{commit}\"\n"));
                }
            }
            out.push('\n');
        }
        out
    }
}

/// How long to wait for another Obelus to finish writing.
///
/// Long enough that the wait is never the reason two windows collide --
/// what is held across it is a read of a few notes, a parse and a write, so
/// microseconds -- and short enough that a lock file left behind by an
/// Obelus that was killed costs a pause nobody times rather than a reader
/// who cannot write notes any more.
const WAIT_FOR_THE_OTHER: std::time::Duration = std::time::Duration::from_millis(50);

/// Reads the notes, changes them, and writes them back, holding the file for
/// the whole of it.
///
/// The one door every change goes through. Read-modify-write rather than
/// writing a copy somebody has been holding, for the reason the
/// conversations beside these are written the same way: a second Obelus on
/// the same project is an ordinary thing to have running, and the one that
/// wrote last would otherwise put the file back the way it was before the
/// other one's note -- and neither of them would be told.
///
/// Answers with the notes as they were written, because that is what the
/// page has to show from here on: what it had is what the file said before
/// somebody else's last minute, and it must not go on holding it.
///
/// The lock is `todo.toml.lock`, made and then renamed over the file, which
/// is the same shape as the name-beside-it this used to write through --
/// with the difference that another Obelus finds it in the way. Best effort:
/// where the lock cannot be had the change is made anyway, because the lock
/// is what makes the read and the write one act and not what makes the
/// change safe. A stale lock must never be a reader who cannot write a note
/// down.
pub fn change<T>(root: &Path, what: impl FnOnce(&mut Todo) -> T) -> Result<(Todo, T), NotChanged> {
    use std::io::Write as _;

    let path = path(root).ok_or_else(|| NotChanged::Unwritable(gone()))?;
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(NotChanged::Unwritable)?;
    }
    let held = gix::lock::File::acquire_to_update_resource(
        &path,
        gix::lock::acquire::Fail::AfterDurationWithBackoff(WAIT_FOR_THE_OTHER),
        None,
    );
    let held = match held {
        Ok(held) => Some(held),
        Err(error) => {
            tracing::debug!(%error, "the notes are being written elsewhere, going ahead anyway");
            None
        }
    };
    // Inside the lock, so that what is changed is what the other Obelus
    // just finished writing rather than what was there before it started.
    //
    // And nothing at all where that read fails. Every other way of declining
    // here leaves the file alone; this one would replace what Obelus could
    // not read with what it could -- which is the reader's notes traded for
    // whatever this session happens to be holding, over a typo.
    let mut todo = match read(root) {
        Reading::Nothing => Todo::default(),
        Reading::Notes(todo) => todo,
        Reading::Unreadable(why, _) => return Err(NotChanged::Unreadable(why)),
    };
    let was = todo.clone();
    let answer = what(&mut todo);
    // A change that changed nothing does not touch the file. Half of what
    // comes through here settles a note that says what it already said, or
    // asks after a note that has gone -- and every write is heard by this
    // Obelus and the other one, both of which then read the file to find
    // out that nothing happened. Names minted on the way in are a change:
    // they are not in the file yet, which is the whole reason they have to
    // be written.
    if !todo.minted && todo == was {
        return Ok((todo, answer));
    }
    match held {
        Some(mut held) => {
            held.write_all(todo.to_toml().as_bytes())
                .and_then(|()| held.commit().map_err(|error| error.error))
                .map_err(NotChanged::Unwritable)?;
        }
        None => todo.write(root).map_err(NotChanged::Unwritable)?,
    }
    Ok((todo, answer))
}

/// Why a change to the notes did not happen.
///
/// Two answers because they are two things to say to the reader and two
/// things for them to do about it. A file that will not read is one they can
/// fix, and until they do Obelus is holding off *on purpose*; a file that
/// will not write is a disk or a permission, and nothing they type will help.
#[derive(Debug)]
pub enum NotChanged {
    /// The file is there and will not read, so nothing was written over it.
    Unreadable(String),
    /// It would not write.
    Unwritable(std::io::Error),
}

impl std::fmt::Display for NotChanged {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(why) => write!(out, "the notes will not read: {why}"),
            Self::Unwritable(error) => write!(out, "the notes will not write: {error}"),
        }
    }
}

impl std::error::Error for NotChanged {}

/// What a note says, with the blank line off the end.
///
/// A text that ends in a newline has an empty last line, and that line is a
/// row on the page nobody typed. Obelus never writes one -- but the file is
/// the reader's as much as it is Obelus's, and TOML's multi-line form
/// invites it: closing quotes on a line of their own is the natural way to
/// write one, and it leaves the break behind.
#[must_use]
pub fn trimmed(text: &str) -> String {
    text.trim_end_matches('\n').to_string()
}

/// A string as TOML writes one.
///
/// The multi-line form where there is a newline in it, because a note is
/// allowed to be a paragraph and `"a\nb"` is a paragraph nobody can read in
/// the file. Escaped either way: a note may quote code, and code has quotes
/// and backslashes in it.
fn quoted(text: &str) -> String {
    let escaped: String = text
        .chars()
        .map(|character| match character {
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            '\n' => "\n".to_string(),
            '\t' => "\\t".to_string(),
            other if (other as u32) < 0x20 => format!("\\u{:04x}", other as u32),
            other => other.to_string(),
        })
        .collect();
    match escaped.contains('\n') {
        true => format!("\"\"\"\n{escaped}\"\"\""),
        false => format!("\"{escaped}\""),
    }
}

/// Where a note's line is now, or `None` for a line that has gone.
///
/// The line was written against the file as one commit had it, and the file
/// has been moving ever since. Git is what knows how: the file as that
/// commit had it, against the file as it is, is a diff, and a diff is a map
/// between the two line numberings -- which is `Changes::working_line`,
/// the same arithmetic the blame already walks the other way.
///
/// Without a commit there is nothing to check the number against, so it is
/// taken at its word. That is the answer for a project git has never heard of,
/// and it is honest: Obelus knows where the note *was* put and has no way to
/// know whether it moved.
///
/// `None` only where git can answer and the answer is that the line is gone
/// -- a run the file no longer has. Landing near it would be worse: a note
/// is about something, and pointing at whatever took its place says the
/// note is about that instead.
#[must_use]
pub fn where_now(root: &Path, at: &At) -> Option<LineNumber> {
    let Some(commit) = at.commit else {
        return Some(at.line);
    };
    let full = root.join(&at.path);
    let Some(then) = crate::history::text_at(root, commit, &full) else {
        // The commit does not have the file -- a note older than a rename,
        // or a repository that has been rewritten. The number is all there
        // is left of where it pointed.
        return Some(at.line);
    };
    let Ok(now) = std::fs::read_to_string(&full) else {
        return Some(at.line);
    };
    crate::Changes::between(&then, &now).working_line(at.line)
}

/// The commit a note made now should carry, where the project has one.
#[must_use]
pub fn at_commit(root: &Path) -> Option<gix::ObjectId> {
    crate::history::head_of(root)
}

#[cfg(test)]
mod tests {
    /// Somewhere of this run's own for the notes to be kept in.
    ///
    /// They live in Obelus's state directory, so a test that did not say
    /// this would write into the reader's own and leave it there. One
    /// directory for the binary, because it is set once and the tests name
    /// their projects apart by their scratch paths anyway.
    fn state_of_its_own() {
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-git-state-{}", std::process::id())),
        );
    }

    use super::*;

    fn note(said: &str) -> Note {
        Note {
            id: NoteId::mint(),
            said: said.to_string(),
            done: false,
            at: None,
            depth: 0,
        }
    }

    /// The notes, with nothing minted: what a file that already names them
    /// all reads back as.
    fn named(notes: Vec<Note>) -> Todo {
        Todo {
            notes,
            minted: false,
        }
    }

    /// What goes out comes back, including the parts TOML has opinions about.
    ///
    /// Broken deliberately by writing a note's newlines into a basic string:
    /// the file held `"a\nb"` as three lines of TOML and would not parse, so
    /// the round trip came back empty.
    #[test]
    fn a_note_survives_the_file() {
        let todo = named(vec![
            note("one line"),
            Note {
                id: NoteId::mint(),
                said: "a title\nand a body\nof two lines".to_string(),
                done: true,
                at: None,
                depth: 0,
            },
            Note {
                id: NoteId::mint(),
                said: "quotes \" and \\ backslashes".to_string(),
                done: false,
                at: Some(At {
                    path: PathBuf::from("src/ui/picker.rs"),
                    line: LineNumber::new(411),
                    commit: None,
                }),
                depth: 1,
            },
        ]);
        let table = todo
            .to_toml()
            .parse::<toml::Table>()
            .unwrap_or_else(|error| panic!("{error}\n{}", todo.to_toml()));
        assert_eq!(Todo::from_table(&table), todo);
    }

    /// The line is written the way a reader counts and held the way Obelus
    /// does, and the two are one apart.
    #[test]
    fn the_line_is_written_as_a_reader_would_say_it() {
        let todo = named(vec![Note {
            id: NoteId::mint(),
            said: "here".to_string(),
            done: false,
            at: Some(At {
                path: PathBuf::from("a.rs"),
                line: LineNumber::new(411),
                commit: None,
            }),
            depth: 0,
        }]);
        assert!(todo.to_toml().contains("line = 412"), "{}", todo.to_toml());
    }

    /// A change is about a note by name, and a name the file no longer has
    /// is a change that does not happen.
    ///
    /// The whole of what keeps a deleted note deleted. Broken deliberately
    /// by having `apply` insert what it cannot find: the note the other
    /// window took away comes back the moment this one ticks it off, and
    /// this goes red.
    #[test]
    fn a_change_about_a_note_that_has_gone_does_nothing() {
        let gone = NoteId::mint();
        let mut todo = named(vec![note("the one that stayed")]);
        let was = todo.clone();
        for change in [
            Change::Said {
                id: gone.clone(),
                said: "back from the dead".to_string(),
            },
            Change::Done {
                id: gone.clone(),
                done: true,
            },
            Change::Remove {
                id: gone.clone(),
                under: true,
            },
            Change::Shift {
                id: gone.clone(),
                out: false,
            },
            Change::Move {
                id: gone.clone(),
                over: todo.notes[0].id.clone(),
                up: true,
            },
        ] {
            assert!(!todo.apply(&change), "{change:?} said it had applied");
        }
        assert_eq!(todo, was, "a change about a note that has gone moved one");
    }

    /// The reader's own new note may go in, and only once.
    ///
    /// Idempotent because the same change can be handed over twice -- put in
    /// the queue before a write and taken out of it after one -- and two
    /// notes answering to one name is the one thing reading the file cannot
    /// repair.
    #[test]
    fn a_note_put_in_twice_is_one_note() {
        let mut todo = named(vec![note("the first")]);
        let change = Change::Put {
            note: note("the reader's new one"),
            after: Some(todo.notes[0].id.clone()),
        };
        assert!(todo.apply(&change));
        assert!(todo.apply(&change));
        assert_eq!(todo.notes.len(), 2);
        assert_eq!(todo.notes[1].said, "the reader's new one");
    }

    /// A new note goes behind the whole of what hangs under the note it
    /// follows -- including children this Obelus has never seen.
    ///
    /// The other window put them there while this one was holding the page.
    /// Between the note and its children, this one would have been adopted
    /// by it without the reader asking for a child at all.
    #[test]
    fn a_new_note_goes_behind_what_hangs_under_the_one_it_follows() {
        let mut todo = named(vec![note("a parent"), note("theirs, a child")]);
        todo.notes[1].depth = 1;
        let change = Change::Put {
            note: note("mine"),
            after: Some(todo.notes[0].id.clone()),
        };
        assert!(todo.apply(&change));
        let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
        assert_eq!(said, ["a parent", "theirs, a child", "mine"]);
        assert_eq!(todo.notes[2].depth, 0);
    }

    /// Taking a note away takes what hangs under it; emptying one's words
    /// brings them up a level instead.
    ///
    /// Two doors, and the file has to be able to tell which one a note left
    /// by: the key that takes a note away means the whole of it, and a note
    /// the reader cleared the words out of was not saying anything about its
    /// children.
    #[test]
    fn what_hangs_under_a_note_goes_with_it_or_comes_up_a_level() {
        let parent = |todo: &Todo| todo.notes[0].id.clone();
        let three = || {
            let mut todo = named(vec![note("a parent"), note("a child"), note("after")]);
            todo.notes[1].depth = 1;
            todo
        };

        let mut todo = three();
        assert!(todo.apply(&Change::Remove {
            id: parent(&todo),
            under: true,
        }));
        let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
        assert_eq!(said, ["after"]);

        let mut todo = three();
        assert!(todo.apply(&Change::Remove {
            id: parent(&todo),
            under: false,
        }));
        let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
        assert_eq!(said, ["a child", "after"]);
        assert_eq!(
            todo.notes[0].depth, 0,
            "the child stayed indented under nothing"
        );
    }

    /// Moving one steps over the whole of the neighbour, not one note of it.
    #[test]
    fn a_move_steps_over_the_whole_of_its_neighbour() {
        let mut todo = named(vec![
            note("a parent"),
            note("a child"),
            note("the one being moved"),
        ]);
        todo.notes[1].depth = 1;
        assert!(todo.apply(&Change::Move {
            id: todo.notes[2].id.clone(),
            over: todo.notes[0].id.clone(),
            up: true,
        }));
        let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
        assert_eq!(said, ["the one being moved", "a parent", "a child"]);
    }

    /// "There is no file" and "there is a file that will not read" are
    /// different answers.
    ///
    /// Both came back as no notes, and no notes is a thing Obelus writes
    /// down: the page opens empty, one note is written into a list the
    /// reader cannot see is not their list, and what they had is replaced by
    /// it. A comma in the wrong place was enough.
    ///
    /// Broken deliberately by reading a file that will not parse as an empty
    /// one again: the last of these goes red.
    #[test]
    fn a_file_that_will_not_read_is_not_a_file_with_nothing_in_it() {
        let scratch =
            std::env::temp_dir().join(format!("obelus-todo-reading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        state_of_its_own();
        std::fs::create_dir_all(&scratch).expect("the directory");

        assert!(
            matches!(read(&scratch), Reading::Nothing),
            "a project with no notes file is not a project that starts empty"
        );

        let path = path(&scratch).expect("a tree that is there");
        std::fs::create_dir_all(path.parent().expect("the directory")).expect("the directory");
        std::fs::write(&path, "[[todo]]\nsaid = \"a note\"\n").expect("the notes");
        let Reading::Notes(todo) = read(&scratch) else {
            panic!("a file that reads did not read");
        };
        assert_eq!(todo.notes.len(), 1);

        std::fs::write(&path, "[[todo]]\nsaid = \"half a no").expect("the half-written notes");
        assert!(
            matches!(read(&scratch), Reading::Unreadable(..)),
            "a file that will not parse read as a file with nothing in it"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// And nothing is written over one.
    ///
    /// The one guard that covers every writer, because they all come through
    /// here: what Obelus could not read it does not replace with what it
    /// could.
    ///
    /// Broken deliberately by letting `change` go on with the default where
    /// the read failed: the file becomes one note long and this goes red.
    #[test]
    fn a_file_that_will_not_read_is_not_written_over() {
        let scratch =
            std::env::temp_dir().join(format!("obelus-todo-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        state_of_its_own();
        std::fs::create_dir_all(&scratch).expect("the tree");
        let path = path(&scratch).expect("a tree that is there");
        std::fs::create_dir_all(path.parent().expect("the directory")).expect("the directory");
        let half = "[[todo]]\nid = \"0123456A\"\nsaid = \"half a no";
        std::fs::write(&path, half).expect("the half-written notes");

        let outcome = change(&scratch, |todo| {
            todo.notes.push(Note {
                id: NoteId::mint(),
                said: "one this Obelus made up".to_string(),
                done: false,
                at: None,
                depth: 0,
            });
        });
        assert!(
            matches!(outcome, Err(NotChanged::Unreadable(_))),
            "a change went ahead against a file that will not read"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("the notes"),
            half,
            "the reader's file was written over"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The notes of a tree that has gone are neither read nor written, and
    /// nothing is made where the tree was.
    ///
    /// They are not nothing -- they are where they always were, under a
    /// project this path can no longer be shown to be -- so a page asking
    /// is told they will not read, which is the answer that is never
    /// written over.
    ///
    /// Broken deliberately by letting `project` take a path that has gone
    /// as it came, which is what it did: the notes read back as though
    /// the tree were still there, and the note written next goes into the
    /// file of a project nobody will open again.
    #[test]
    fn the_notes_of_a_tree_that_has_gone_are_left_alone() {
        let scratch = std::env::temp_dir().join(format!("obelus-todo-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        state_of_its_own();
        std::fs::create_dir_all(&scratch).expect("the tree");
        let file = path(&scratch).expect("a tree that is there");
        change(&scratch, |todo| {
            todo.notes.push(note("written while it was there"))
        })
        .expect("a note written down");
        let written = std::fs::read_to_string(&file).expect("the notes");

        std::fs::remove_dir_all(&scratch).expect("the tree going");
        assert!(
            matches!(read(&scratch), Reading::Unreadable(..)),
            "the notes of a tree that has gone read as if it were there"
        );
        assert!(
            matches!(
                change(&scratch, |todo| todo.notes.push(note("written after"))),
                Err(NotChanged::Unwritable(_))
            ),
            "a note was written down for a tree that has gone"
        );
        assert_eq!(
            std::fs::read_to_string(&file).expect("the notes"),
            written,
            "the notes the tree had were changed after it went"
        );
        assert!(
            !scratch.exists(),
            "the tree was made again around its notes"
        );
        let _ = std::fs::remove_file(&file);
    }

    /// A note that says nothing is not a note.
    #[test]
    fn an_empty_note_is_not_kept() {
        let table = "[[todo]]\nsaid = \"   \"\n\n[[todo]]\nsaid = \"real\"\n"
            .parse::<toml::Table>()
            .expect("the table");
        let todo = Todo::from_table(&table);
        assert_eq!(todo.notes.len(), 1);
        assert_eq!(todo.notes[0].said, "real");
    }

    /// A name is minted for a note that has none, and for one whose name
    /// somebody else in the file already has.
    ///
    /// The second is what copying a block by hand produces, and two notes
    /// answering to one name is worse than either getting a new one:
    /// whatever was keyed to it would follow whichever came first and the
    /// reader would never be told.
    #[test]
    fn every_note_ends_up_with_a_name_of_its_own() {
        let table = concat!(
            "[[todo]]\nsaid = \"no name at all\"\n\n",
            "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"named\"\n\n",
            "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"named the same\"\n\n",
            "[[todo]]\nid = \"not a name\"\nsaid = \"named badly\"\n",
        )
        .parse::<toml::Table>()
        .expect("the table");
        let todo = Todo::from_table(&table);

        assert_eq!(todo.notes.len(), 4);
        let names: Vec<&NoteId> = todo.notes.iter().map(|note| &note.id).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(
                !names[at + 1..].contains(name),
                "two notes answer to {name}"
            );
        }
        assert_eq!(
            todo.notes[1].id.as_str(),
            "ABCDEFGH",
            "a name the file already had was not kept"
        );
        assert!(todo.minted, "reading minted names and did not say so");
    }

    /// A file that names every note is read without changing anything.
    ///
    /// Which is what makes the write-back safe: `minted` is what asks for
    /// one, so a file already in order is not rewritten on every open.
    #[test]
    fn a_file_that_names_them_all_is_left_alone() {
        let before = named(vec![note("one"), note("two")]);
        let table = before.to_toml().parse::<toml::Table>().expect("the table");
        let after = Todo::from_table(&table);
        assert_eq!(after, before);
        assert!(!after.minted, "a file in order was rewritten");
    }

    /// A row is the first line; folding it open is the rest.
    /// A note hanging under one that is not there is read as one that is.
    ///
    /// Both shapes a hand-written file produces: a list that starts indented,
    /// and one that skips a level on the way down. Neither describes anything
    /// -- there is no note at the depth they claim to be under -- so the
    /// nearest depth that does is what they mean.
    #[test]
    fn a_depth_with_nothing_over_it_is_read_as_the_nearest_one_that_has() {
        let table = concat!(
            "[[todo]]\nsaid = \"first\"\ndepth = 2\n\n",
            "[[todo]]\nsaid = \"second\"\ndepth = 3\n\n",
            "[[todo]]\nsaid = \"third\"\ndepth = 9\n\n",
            "[[todo]]\nsaid = \"fourth\"\ndepth = 9\n\n",
            "[[todo]]\nsaid = \"fifth\"\ndepth = 9\n",
        )
        .parse::<toml::Table>()
        .expect("the table");
        let depths: Vec<u16> = Todo::from_table(&table)
            .notes
            .iter()
            .map(|note| note.depth)
            .collect();
        // The first can only be at the top; each after it can be one deeper
        // than the one above, and no more -- until the deepest a note is
        // allowed to be, where a chain of them stops going down.
        assert_eq!(depths, vec![0, 1, 2, DEEPEST, DEEPEST]);
    }

    /// And a depth that is already under something is left where it is,
    /// including one that comes back up several levels at once.
    #[test]
    fn a_depth_that_has_something_over_it_is_read_as_written() {
        let table = concat!(
            "[[todo]]\nsaid = \"a\"\n\n",
            "[[todo]]\nsaid = \"b\"\ndepth = 1\n\n",
            "[[todo]]\nsaid = \"c\"\ndepth = 2\n\n",
            "[[todo]]\nsaid = \"d\"\n",
        )
        .parse::<toml::Table>()
        .expect("the table");
        let depths: Vec<u16> = Todo::from_table(&table)
            .notes
            .iter()
            .map(|note| note.depth)
            .collect();
        assert_eq!(depths, vec![0, 1, 2, 0]);
    }

    /// What hangs under a note is the run after it that is deeper.
    ///
    /// Asked of every note in one list rather than of one in several: what
    /// this has to get right is where a run *ends*, and a list with one
    /// parent in it never exercises the end that matters -- the next note at
    /// the same depth.
    #[test]
    fn what_hangs_under_a_note_is_the_run_below_it_that_is_deeper() {
        let deep = |said: &str, depth: u16| Note {
            depth,
            ..note(said)
        };
        let todo = named(vec![
            deep("a", 0),
            deep("a.1", 1),
            deep("a.1.i", 2),
            deep("a.2", 1),
            deep("b", 0),
            deep("b.1", 1),
        ]);
        let under: Vec<usize> = (0..todo.notes.len()).map(|at| todo.under(at)).collect();
        assert_eq!(under, vec![3, 1, 0, 0, 1, 0]);
        // Past the end is nothing rather than a panic: the callers ask about
        // a selection, and a selection can be stale.
        assert_eq!(todo.under(todo.notes.len()), 0);
    }

    #[test]
    fn a_row_is_the_first_line_and_the_fold_is_the_rest() {
        let plain = note("just this");
        assert_eq!(plain.title(), "just this");
        assert!(!plain.folds(), "a one-line note offers a fold");

        let long = note("a title\nand more\nand more");
        assert_eq!(long.title(), "a title");
        assert!(long.folds());
        assert_eq!(long.body(), ["and more", "and more"]);
    }

    /// Another Obelus halfway through writing the notes is left alone.
    ///
    /// Broken deliberately by writing beside them as `toml.writing` again,
    /// with no process number: the other's file is truncated and renamed
    /// away under it.
    #[test]
    fn another_obelus_writing_the_notes_is_left_alone() {
        state_of_its_own();
        let root = std::env::temp_dir().join(format!("obelus-notes-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the project");
        let theirs = path(&root)
            .expect("somewhere")
            .with_extension("toml.writing");
        std::fs::create_dir_all(theirs.parent().expect("a directory")).expect("the directory");
        std::fs::write(&theirs, "another Obelus is halfway through this").expect("theirs");

        named(vec![note("one")]).write(&root).expect("the notes");
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("another Obelus is halfway through this"),
            "the other Obelus's half-written notes were taken"
        );
    }
}
