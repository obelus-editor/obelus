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

use crate::coordinates::LineNumber;

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

/// One note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// What it says. Never empty, and its first line is the row.
    pub said: String,
    /// Whether it is done.
    ///
    /// Kept rather than taken away, because a list of what is done is how a
    /// reader tells "I decided against it" from "I never got to it".
    pub done: bool,
    /// The place it is about, if it is about one.
    pub at: Option<At>,
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

/// Every note a tree has, in the order they were written.
///
/// Written order, not sorted: a reader who ticks something off does not want
/// the list to reorder itself underneath them, and a note's place in the
/// list is the one thing about it they can rely on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Todo {
    /// The notes.
    pub notes: Vec<Note>,
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
        let Some(written) = table.get("todo").and_then(toml::Value::as_array) else {
            return Self { notes };
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
            notes.push(Note {
                said,
                done: note
                    .get("done")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                at,
            });
        }
        Self { notes }
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
        for note in &self.notes {
            out.push_str("[[todo]]\n");
            out.push_str(&format!("said = {}\n", quoted(&note.said)));
            out.push_str(&format!("done = {}\n", note.done));
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

/// A string as TOML writes one.
///
/// The multi-line form where there is a newline in it, because a note is
/// allowed to be a paragraph and `"a\nb"` is a paragraph nobody can read in
/// the file. Escaped either way: a note may quote code, and code has quotes
/// and backslashes in it.
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
/// between the two line numberings -- which is [`Changes::working_line`],
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
    let Some(then) = crate::git::history::text_at(root, commit, &full) else {
        // The commit does not have the file -- a note older than a rename,
        // or a repository that has been rewritten. The number is all there
        // is left of where it pointed.
        return Some(at.line);
    };
    let Ok(now) = std::fs::read_to_string(&full) else {
        return Some(at.line);
    };
    crate::git::Changes::between(&then, &now).working_line(at.line)
}

/// The commit a note made now should carry, where the tree has one.
#[must_use]
pub fn at_commit(root: &Path) -> Option<gix::ObjectId> {
    crate::git::history::head_of(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(said: &str) -> Note {
        Note {
            said: said.to_string(),
            done: false,
            at: None,
        }
    }

    /// What goes out comes back, including the parts TOML has opinions about.
    ///
    /// Broken deliberately by writing a note's newlines into a basic string:
    /// the file held `"a\nb"` as three lines of TOML and would not parse, so
    /// the round trip came back empty.
    #[test]
    fn a_note_survives_the_file() {
        let todo = Todo {
            notes: vec![
                note("one line"),
                Note {
                    said: "a title\nand a body\nof two lines".to_string(),
                    done: true,
                    at: None,
                },
                Note {
                    said: "quotes \" and \\ backslashes".to_string(),
                    done: false,
                    at: Some(At {
                        path: PathBuf::from("src/ui/picker.rs"),
                        line: LineNumber::new(411),
                        commit: None,
                    }),
                },
            ],
        };
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
        let todo = Todo {
            notes: vec![Note {
                said: "here".to_string(),
                done: false,
                at: Some(At {
                    path: PathBuf::from("a.rs"),
                    line: LineNumber::new(411),
                    commit: None,
                }),
            }],
        };
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

    /// A row is the first line; folding it open is the rest.
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
