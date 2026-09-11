//! An open document: its text, where the cursor is, and what part of it is on
//! screen.

mod moving;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::{
    coordinates::{CharColumn, DisplayColumn, LineNumber, Span},
    syntax::{
        LanguageId,
        parse::{self, SyntaxState},
    },
    text::Text,
};

/// Which open document, by position in the list.
///
/// An index rather than a generational key because M0 cannot close a buffer.
/// The day it can, this becomes the thing that has to change, and the type
/// makes that one place instead of every `usize` that happened to be a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BufferId(usize);

impl BufferId {
    /// Wraps a position in the buffer list.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The position in the buffer list.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// What a buffer holds.
///
/// One variant so far. It is here because the answer to "what is a buffer"
/// and the answer to "how is it being shown" are two different questions,
/// and conflating them is how a program ends up with a clock that has a
/// syntax tree: a buffer over a file has text, a version and a language
/// server; a buffer over a clock or a calendar has none of those and still
/// wants a name, a place in the buffer list, and a way to be shown.
///
/// Deliberately not built out yet. A second variant means the text, the
/// syntax and the reload path move inside this one, which is a change worth
/// making when there is something to put beside them and not before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Content {
    /// A file on disk.
    #[default]
    File,
}

/// How a buffer is being shown.
///
/// A file is text by default, whatever it is: opening a README should show
/// what is in it. Another mode is a *reading* of the same bytes -- the text
/// is untouched and the mode can be turned off again -- and the status bar
/// names it, because a screen showing something other than the file needs to
/// say so.
///
/// One variant per way of showing a buffer, and the extension point for both
/// questions above: a rendered diff and a hex view are modes over a file's
/// bytes, and a clock is a mode over a buffer with no bytes at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The bytes, highlighted. What every file starts as.
    #[default]
    Edit,
    /// Markdown, rendered.
    Markdown,
}

impl Mode {
    /// What to call it on the status bar, or `None` for the ordinary one.
    ///
    /// `None` rather than "edit": a marker that is always there says nothing,
    /// and the absence of one is what says "this is the file".
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::Edit => None,
            Self::Markdown => Some("markdown"),
        }
    }
}

/// A direction to move the cursor in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One character left.
    Left,
    /// One character right.
    Right,
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// The first character of the line.
    LineStart,
    /// Past the last character of the line.
    LineEnd,
    /// The start of the document.
    DocumentStart,
    /// The last line of the document.
    DocumentEnd,
}

/// Where the cursor is, and where it would like to be.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    /// The line the cursor is on.
    pub line: LineNumber,
    /// The character the cursor is before.
    pub column: CharColumn,
    /// The cell within a visual row that the cursor is aiming for while moving
    /// vertically.
    ///
    /// Without this, moving down through a short row and back up lands in the
    /// wrong place: the column would have been clamped on the way through and
    /// the original never recovered. Within a *row* rather than within a line,
    /// because with wrapping a row is what moving up and down steps over.
    remembered_cell: DisplayColumn,
}

/// Which part of the document is on screen.
///
/// A line and a visual row within it, because with wrapping a line can be
/// taller than the screen: anchoring only to a line would make everything past
/// the first screenful of one unreachable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// The first line drawn.
    pub top: LineNumber,
    /// Which visual row of `top` the screen starts on.
    pub top_row: usize,
    /// How many cells of each line are off the left-hand edge.
    ///
    /// Always zero while lines wrap: there is nothing off to the side, by
    /// construction. Without wrapping this is how a reader gets to the end
    /// of a long line.
    pub left: usize,
}

/// The room the text has, and whether it wraps in it.
///
/// The numbers together, because with wrapping neither is useful alone: the
/// width decides where lines break and so how many rows they take, and the
/// height decides how many of those rows fit. And the flag with them,
/// because every one of those answers changes when lines do not wrap.
#[derive(Clone, Copy, Debug)]
pub struct TextArea {
    /// Cells across, once the gutter has taken its columns.
    pub width: u16,
    /// Rows down.
    pub height: u16,
    /// Whether a line too long for the width continues on the next row.
    ///
    /// With this off a line is one row however long it is, and the view
    /// scrolls sideways to follow the cursor along it.
    pub wrap: bool,
}

impl TextArea {
    /// The width lines are broken at.
    ///
    /// Effectively no limit when wrapping is off, which is how one set of
    /// rules serves both: every line is then one row, its cells are its
    /// display columns, and nothing above has to ask which mode it is in.
    #[must_use]
    pub const fn wrap_width(self) -> u16 {
        if self.wrap {
            if self.width == 0 { 1 } else { self.width }
        } else {
            u16::MAX
        }
    }
}

/// An open document.
#[derive(Debug)]
pub struct Buffer {
    path: PathBuf,
    /// What it holds.
    content: Content,
    /// How it is being shown.
    mode: Mode,
    /// How many times the reader has come back to it.
    ///
    /// The buffer list is ordered by this. A list in the order files were
    /// opened puts the one opened by accident an hour ago above the one
    /// being read all afternoon.
    activations: u32,
    text: Text,
    syntax: Option<SyntaxState>,
    /// Whether the last attempt to re-read the file failed.
    ///
    /// Set when the file has been deleted, replaced by a directory, or made
    /// unreadable. The buffer keeps showing what it last held — losing the
    /// contents would be worse than showing something out of date — so
    /// without saying so on screen the reader has no way to know.
    stale: bool,
    /// How many times this document has changed, counted from one.
    ///
    /// The protocol needs it on every change, and every request records the
    /// version it was asked against: an answer that arrives after a reload is
    /// about text that is no longer on screen.
    ///
    /// What is sent with it is the whole document, not the range that changed.
    /// The range would have to be in the *old* document's coordinates, which
    /// means a second representation of the edit alongside the byte offsets
    /// tree-sitter needs, converted through whichever units the server
    /// negotiated. That is the shape every coordinate bug in this program has
    /// had, and a source file down a pipe costs nothing.
    version: i32,
    cursor: Cursor,
    /// Where the current selection started, if the reader is extending one.
    ///
    /// The cursor is the other end. Keeping the anchor rather than a range
    /// means changing direction naturally shrinks the selection and can pass
    /// back through it without a special case.
    selection_anchor: Option<Cursor>,
    /// Whether the viewport has been scrolled away from the cursor on
    /// purpose.
    ///
    /// The wheel does that: a glance, with the reader's place left where it
    /// was. While it holds, nothing drags the screen back -- the cursor
    /// being off screen is what was asked for -- and the next cursor move
    /// clears it and brings the screen back, centred. Paging does not set
    /// it, because paging takes the cursor along.
    detached: bool,
    viewport: Viewport,
}

impl Buffer {
    /// Reads a file from disk.
    pub fn open(path: &Path) -> Result<Self> {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let text = Text::from_string(&contents);
        let syntax =
            LanguageId::for_path(path).and_then(|language| SyntaxState::new(language, &text));
        // Made absolute here, once. A relative path is what a reader types,
        // and it is the wrong thing to keep: the protocol needs an absolute
        // URI, the watcher needs a directory, and deciding whether a file is
        // inside the root is a comparison a relative path fails silently.
        //
        // Absolute rather than canonical: resolving symlinks would report the
        // file under a name the reader did not use.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        // The event a session is made of, and the three facts that explain
        // what happens next: how long the parse and the highlighting have
        // to work, and whether there is a language at all.
        tracing::info!(
            path = %path.display(),
            bytes = contents.len(),
            lines = text.line_count(),
            language = ?syntax.as_ref().map(|state| state.language().name()),
            "opened"
        );

        Ok(Self {
            path,
            content: Content::File,
            mode: Mode::Edit,
            activations: 0,
            text,
            syntax,
            stale: false,
            version: 1,
            cursor: Cursor {
                line: LineNumber::new(0),
                column: CharColumn::new(0),
                remembered_cell: DisplayColumn::new(0),
            },
            selection_anchor: None,
            detached: false,
            viewport: Viewport {
                left: 0,
                top: LineNumber::new(0),
                top_row: 0,
            },
        })
    }

    /// What the buffer holds.
    #[must_use]
    pub const fn content(&self) -> Content {
        self.content
    }

    /// How it is being shown.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Shows it a different way.
    pub const fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// How many times the reader has come back to it.
    #[must_use]
    pub const fn activations(&self) -> u32 {
        self.activations
    }

    /// Counts a visit.
    pub const fn activate(&mut self) {
        self.activations = self.activations.saturating_add(1);
    }

    /// Where the document came from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document's text.
    #[must_use]
    pub const fn text(&self) -> &Text {
        &self.text
    }

    /// How many times this document has changed.
    #[must_use]
    pub const fn version(&self) -> i32 {
        self.version
    }

    /// Whether the file could not be re-read the last time obelus tried.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Which language this is, if obelus knows it.
    ///
    /// Taken from the parse rather than from the path a second time, so the
    /// server and the highlighting can never disagree about what a file is.
    #[must_use]
    pub fn language(&self) -> Option<LanguageId> {
        self.syntax.as_ref().map(SyntaxState::language)
    }

    /// The parse, if this is a language obelus knows.
    #[must_use]
    pub const fn syntax(&self) -> Option<&SyntaxState> {
        self.syntax.as_ref()
    }

    /// Re-reads the file and reparses the part that changed.
    ///
    /// Returns whether anything changed. A watcher fires for `touch`, for a
    /// rename, and for a permission change, none of which alter a byte;
    /// comparing first costs one pass over the rope and saves a reparse and a
    /// cursor clamp.
    ///
    /// The cursor and the viewport are kept: an agent rewriting the file
    /// should not send the reader back to line one.
    pub fn reload(&mut self) -> Result<bool> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) => {
                self.stale = true;
                return Err(error).with_context(|| format!("re-reading {}", self.path.display()));
            }
        };
        // The file read, so whatever was wrong with it no longer is.
        self.stale = false;
        let new = Text::from_string(&contents);
        if new.rope() == self.text.rope() {
            return Ok(false);
        }

        let edit = parse::edit_between(&self.text, &new);
        self.text = new;
        self.version = self.version.saturating_add(1);

        match (self.syntax.as_mut(), edit) {
            (Some(state), Some(edit)) => state.reparse(&self.text, &edit),
            // Nothing shared to reuse, so start over. Reached only if the
            // trimming above found no common region at all.
            (Some(state), None) => {
                let language = state.language();
                self.syntax = SyntaxState::new(language, &self.text);
            }
            (None, _) => {}
        }

        self.cursor.line = self.text.clamp_line(self.cursor.line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, self.cursor.column);
        // `top_row` is clamped by `scroll_into_view`, which knows the width.
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        Ok(true)
    }
}
