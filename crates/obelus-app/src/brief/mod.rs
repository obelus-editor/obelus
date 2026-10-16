//! What a new conversation is started with: something the reader was on
//! when they asked for one.
//!
//! **One door, many places.** The list of conversations offers a row for
//! each thing the reader was on as it opened -- a place in a file, a note --
//! and choosing it is `App::begin_briefed`, whichever it was. A new place a
//! conversation can be started from is a new variant here and a row in that
//! list, not a command of its own: "where to talk" is one question with one
//! key, and a second key for each subject is how that stops being true.
//!
//! **Told once, with the first message.** A brief is what Obelus saw when
//! the list opened, and it goes out in front of the first thing the reader
//! says and is let go. It is not what the conversation is *about* in the
//! sense [`crate::conversation::Topic`] is: a place in a file has no name
//! another window could claim, and a line number moves with the next edit,
//! so there is nothing to keep it under and nothing to tell the agent again.
//! A note is the exception, because a note has all of that already: its
//! brief opens the conversation about it, and the topic does the telling.

use std::path::PathBuf;

use obelus_text::coordinates::{CharColumn, LineNumber, Span};
use obelus_todo::NoteId;

/// What the agent is told about a place in a file.
const PLACE: &str = include_str!("place.txt");

/// What the transcript says where a brief went out, so the reader can see
/// that something of theirs went with their words.
pub const SAID: &str = "Told the agent where this conversation was started from";

/// How much of a selection goes to the agent in its own words.
///
/// Past this the agent is told the lines and reads them itself: it can,
/// and a prompt that is mostly a pasted file is a prompt the reader's own
/// question is lost at the end of.
const SELECTION_LIMIT: usize = 20_000;

/// Something the reader was on when they asked for a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Brief {
    /// A place in a file being read.
    Place(Place),
    /// A note on the notes page, by its name.
    ///
    /// Opens the conversation about the note, which is claimed, kept and
    /// told again when the note is rewritten -- see the module's doc.
    Note {
        /// Which note.
        id: NoteId,
        /// Its first line, for the row.
        title: String,
    },
}

/// Where the caret was in a file, and what was selected there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The file, from the project's root.
    pub path: PathBuf,
    /// The caret's line.
    pub line: LineNumber,
    /// The caret's column.
    pub column: CharColumn,
    /// What was selected, and its words.
    pub selection: Option<(Span, String)>,
    /// The commit whose version of the file this was, where it was not the
    /// file on disk -- by its short name.
    pub commit: Option<String>,
    /// Whether the reader had changed it without saving.
    pub unsaved: bool,
}

impl Brief {
    /// What its row in the list says, after the words starting one.
    #[must_use]
    pub fn label(&self) -> String {
        format!("New conversation about {}", self.title())
    }

    /// What the conversation is called while nothing has named it: where it
    /// was started from.
    #[must_use]
    pub fn title(&self) -> String {
        match self {
            Self::Place(place) => place.title(),
            Self::Note { title, .. } => title.clone(),
        }
    }

    /// What goes to the agent in front of the first message, where Obelus
    /// is the one to say it.
    ///
    /// `None` for a note, whose topic says it -- every time the note
    /// changes, which a brief told once could not.
    #[must_use]
    pub fn words(&self) -> Option<String> {
        match self {
            Self::Place(place) => Some(place.words()),
            Self::Note { .. } => None,
        }
    }
}

impl Place {
    /// The path and the lines, the way a reader would write them.
    fn title(&self) -> String {
        let lines = match self.lines() {
            Some((first, last)) if first != last => format!("{first}-{last}"),
            Some((first, _)) => first.to_string(),
            None => (self.line.get() + 1).to_string(),
        };
        format!("{}:{lines}", self.path.display())
    }

    /// The first and last lines the selection has characters on, counted
    /// from one, where there is a selection.
    ///
    /// A selection that ends at the start of a line -- three whole lines
    /// picked with shift and down -- ends on the line before: the line it
    /// stops at has nothing of it.
    fn lines(&self) -> Option<(usize, usize)> {
        let (span, _) = self.selection.as_ref()?;
        let first = span.line.get() + 1;
        let last = match span.end_column.get() {
            0 if span.end_line > span.line => span.end_line.get(),
            _ => span.end_line.get() + 1,
        };
        Some((first, last))
    }

    /// What the agent is told.
    fn words(&self) -> String {
        let version = match (&self.commit, self.unsaved) {
            (Some(commit), _) => format!(
                "\nWhat they were reading is the file as commit {commit} had it, not the file on disk."
            ),
            (None, true) => "\nThey have changed it without saving, so what is on disk is not what they were reading.".to_string(),
            (None, false) => String::new(),
        };
        let selection = match (&self.selection, self.lines()) {
            (Some((_, text)), Some((first, last))) if text.len() <= SELECTION_LIMIT => {
                format!("\n\nThey had lines {first} to {last} selected:\n\n```\n{text}\n```")
            }
            (Some(_), Some((first, last))) => {
                format!("\n\nThey had lines {first} to {last} selected, too much to quote here.")
            }
            _ => String::new(),
        };
        // The reader's own text last, for the reason a note's is: a
        // selection holding `{path}` would otherwise be filled in.
        PLACE
            .trim()
            .replace("{path}", &self.path.display().to_string())
            .replace("{line}", &(self.line.get() + 1).to_string())
            .replace("{column}", &(self.column.get() + 1).to_string())
            .replace("{version}", &version)
            .replace("{selection}", &selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(selection: Option<(Span, &str)>) -> Place {
        Place {
            path: PathBuf::from("src/lib.rs"),
            line: LineNumber::new(41),
            column: CharColumn::new(4),
            selection: selection.map(|(span, text)| (span, text.to_string())),
            commit: None,
            unsaved: false,
        }
    }

    fn span(line: usize, column: usize, end_line: usize, end_column: usize) -> Span {
        Span {
            line: LineNumber::new(line),
            column: CharColumn::new(column),
            end_line: LineNumber::new(end_line),
            end_column: CharColumn::new(end_column),
        }
    }

    /// Lines are counted from one, the way the gutter counts them.
    ///
    /// Broken deliberately by dropping the `+ 1` from the caret's line.
    #[test]
    fn a_place_is_named_by_the_line_the_gutter_shows() {
        let brief = Brief::Place(place(None));
        assert_eq!(brief.title(), "src/lib.rs:42");
        let words = brief.words().expect("a place says something");
        assert!(words.contains("src/lib.rs"), "{words}");
        assert!(words.contains("line 42, column 5"), "{words}");
    }

    /// Three whole lines picked with shift and down end at the start of a
    /// fourth, which has nothing of them in it.
    ///
    /// Broken deliberately by counting the line the selection stops at.
    #[test]
    fn a_selection_of_whole_lines_ends_on_the_last_of_them() {
        let brief = Brief::Place(place(Some((span(9, 0, 12, 0), "a\nb\nc\n"))));
        assert_eq!(brief.title(), "src/lib.rs:10-12");
        let words = brief.words().expect("a place says something");
        assert!(words.contains("lines 10 to 12 selected"), "{words}");
        assert!(words.contains("a\nb\nc"), "{words}");
    }

    /// What the reader selected goes in as it is, even where it looks like
    /// one of the template's holes.
    ///
    /// Broken deliberately by filling `{selection}` before `{path}`.
    #[test]
    fn a_selection_is_not_read_as_the_template() {
        let brief = Brief::Place(place(Some((span(0, 0, 0, 6), "{path}"))));
        let words = brief.words().expect("a place says something");
        assert!(words.contains("```\n{path}\n```"), "{words}");
    }

    /// A version that is not the file on disk says so: the agent reads the
    /// file on disk.
    ///
    /// Broken deliberately by leaving `{version}` empty.
    #[test]
    fn a_version_that_is_not_on_disk_says_so() {
        let mut at_a_commit = place(None);
        at_a_commit.commit = Some("2113f9c".to_string());
        let words = Brief::Place(at_a_commit).words().unwrap_or_default();
        assert!(words.contains("commit 2113f9c"), "{words}");
        let mut unsaved = place(None);
        unsaved.unsaved = true;
        let words = Brief::Place(unsaved).words().unwrap_or_default();
        assert!(words.contains("without saving"), "{words}");
    }
}
