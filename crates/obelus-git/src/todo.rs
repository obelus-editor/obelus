//! What a reader means to come back to.
//!
//! An *obelus* is the mark a scholar put beside a line they doubted. This is
//! that mark, written down: a note made while reading, kept with the tree it
//! is about rather than in the reader's own home, because it is about this
//! project and the next person to open it has the same questions.
//!
//! A note may carry a place -- a file and a line -- or carry none, and both
//! are ordinary. "This cache is wrong" belongs to a line; "wire the counts
//! tree up to the search" belongs to the project.
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

/// Where a tree keeps what it means to come back to.
///
/// Beside the settings, in the directory obelus keeps a tree's things in.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(".obelus").join("todo.toml")
}

/// The place a note is about, as it was written down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct At {
    /// The file, relative to the tree.
    pub path: PathBuf,
    /// The line, as it was when the note was made.
    pub line: LineNumber,
    /// The commit the file was at when the note was made, where the tree is
    /// a git repository.
    ///
    /// Not the note's date and not the file's: what the line number means is
    /// "line 412 of the file *as that commit had it*", and a commit is the
    /// only name for a version of a file that is still there next week.
    /// `None` for a tree git has never heard of, where the line is taken at
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
/// as it is obelus's.
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
    /// two notes made in one second, in two obeluses, on one tree, do not
    /// collide -- the file is read back and a clash is minted over anyway,
    /// so this is a cheap first line rather than the only one. The clock
    /// separates seconds, a counter separates notes within one, and the
    /// hasher's per-process key separates two obeluses.
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
    /// whoever the parent is. The file is the reader's as much as obelus's
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

/// What an agent asked obelus to do to the notes.
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

/// How deep a note may sit.
///
/// Four levels, counted from nothing. A list in a terminal is as wide as
/// the terminal, and what indenting costs is taken from the one column the
/// reader is actually reading -- so the depth has to stop somewhere, and it
/// may as well stop where an outline of things to do stops being one.
pub const DEEPEST: u16 = 3;

/// Every note a tree has, in the order they were written.
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

impl Todo {
    /// What is in a tree's file, or nothing where there is no file.
    ///
    /// A file that will not parse is not a reason to stop: obelus goes on
    /// with no notes and says so in the log, the same way a settings file
    /// that will not read is handled. Losing the view over a typo somebody
    /// made by hand would be the worse answer.
    #[must_use]
    pub fn read(root: &Path) -> Self {
        let path = path(root);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match text.parse::<toml::Table>() {
            Ok(table) => Self::from_table(&table),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "not read, so there are no notes");
                Self::default()
            }
        }
    }

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
                // lines and how every other number obelus writes down is
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

    /// Writes them back, making the directory if it is not there.
    ///
    /// The whole file every time. It is a handful of notes, obelus is the
    /// only thing that writes it, and a merge of what somebody else might
    /// have changed in the meantime would be a lot of machinery for a file
    /// nobody edits from two places at once.
    ///
    /// Through a name beside it and then a rename, the way the settings are
    /// written: a crash halfway through leaves the old file rather than half
    /// of the new one.
    pub fn write(&self, root: &Path) -> std::io::Result<()> {
        let path = path(root);
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        let beside = path.with_extension("toml.writing");
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

/// What a note says, with the blank line off the end.
///
/// A text that ends in a newline has an empty last line, and that line is a
/// row on the page nobody typed. obelus never writes one -- but the file is
/// the reader's as much as it is obelus's, and TOML's multi-line form
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
/// taken at its word. That is the answer for a tree git has never heard of,
/// and it is honest: obelus knows where the note *was* put and has no way to
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

/// The commit a note made now should carry, where the tree has one.
#[must_use]
pub fn at_commit(root: &Path) -> Option<gix::ObjectId> {
    crate::history::head_of(root)
}

#[cfg(test)]
mod tests {
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

    /// The line is written the way a reader counts and held the way obelus
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
}
