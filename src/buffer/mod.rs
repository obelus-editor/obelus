//! An open document: its text, where the cursor is, and what part of it is on
//! screen.

pub mod folds;
mod moving;
pub mod undo;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::{
    coordinates::{CharColumn, CharOffset, DisplayColumn, LineNumber, Span},
    syntax::{
        LanguageId,
        parse::{self, SyntaxState},
    },
    text::{Edit, Text},
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Content {
    /// A file on disk.
    #[default]
    File,
    /// A file as a commit had it.
    ///
    /// The path is still the file's own -- the highlighting, the icon and
    /// the name on the status bar all come from it -- but the bytes are
    /// not what is on disk, and everything that assumes otherwise has to
    /// ask. The watcher must not re-read over it, the language server must
    /// not be told this is what that path holds, and a list of open files
    /// must not hand back this buffer to a reader who asked for the file.
    Commit {
        /// Which commit.
        id: gix::ObjectId,
    },
}

impl Content {
    /// Whether these bytes are the file on disk.
    #[must_use]
    pub const fn is_file(&self) -> bool {
        matches!(self, Self::File)
    }

    /// The commit this was read from, if it was read from one.
    #[must_use]
    pub const fn at(&self) -> Option<gix::ObjectId> {
        match self {
            Self::File => None,
            Self::Commit { id } => Some(*id),
        }
    }

    /// That commit in the short form a reader recognises it by.
    #[must_use]
    pub fn short(&self) -> Option<String> {
        Some(self.at()?.to_string().chars().take(7).collect())
    }
}

/// How a buffer is being shown.
///
/// A file is text by default, whatever it is: opening a README should show
/// what is in it. Another mode is a *reading* of the same bytes -- the text
/// is untouched and the mode can be turned off again -- and the status bar
/// names it, because a screen showing something other than the file needs to
/// say so.
///
/// Two, and two is enough: the bytes, or a reading of them. *Which* reading
/// is the file's own business -- markdown is laid out, a log is put in
/// columns -- and that is decided by what the file is, not by the mode. A
/// third variant would be this enum answering a question the format already
/// answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The bytes, highlighted. What a file with no reading is always shown
    /// as, and what every file can be shown as.
    #[default]
    Edit,
    /// The reading its format has, laid out.
    Preview,
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
            Self::Preview => Some("preview"),
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
    /// The start of the word to the left, or the one the cursor is in.
    WordLeft,
    /// Past the end of the word to the right.
    WordRight,
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

/// What somebody else has done to a file since a document and it were the
/// same bytes.
///
/// Three states and not a flag, because the ways out of them are different:
/// a file that was written over can be taken instead of the document, and a
/// file that is gone cannot -- there is nothing there to take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Disk {
    /// Nothing. What is there is what this was read from or written to.
    #[default]
    Unchanged,
    /// Written over by somebody else.
    Written,
    /// Taken away.
    Deleted,
}

/// What `stat` says about a file: the cheap half of "has it changed".
///
/// Cheap and not conclusive in either direction. `touch` moves the
/// timestamp without moving a byte, and a write that kept the length inside
/// one tick of the timestamp's resolution moves neither. It is what gets
/// asked first, and what says whether the other half is worth paying for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stat {
    /// When it was last written, where the filesystem says.
    modified: Option<std::time::SystemTime>,
    /// How long it was.
    length: u64,
}

impl Stat {
    /// What the file at `path` looks like now, if it can be asked.
    fn of(path: &std::path::Path) -> Option<Self> {
        let data = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: data.modified().ok(),
            length: data.len(),
        })
    }
}

/// What the file was, the last time the document and the file were the same
/// bytes.
///
/// Which is the only moment this is recorded: opening, saving, and taking
/// what is on disk. So the digest costs nothing to take -- the bytes are
/// already in hand -- and reading the file to compare against it is paid
/// for only when [`Stat`] has already said something moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    /// What `stat` said then.
    stat: Option<Stat>,
    /// What was in it then.
    digest: u64,
    /// And how long that was.
    ///
    /// Beside the digest because it is the cheap half of the same question:
    /// a document that is not even the length of the one on disk is not it,
    /// and finding that out costs nothing where taking a digest costs a
    /// pass over the file.
    length: usize,
}

/// Whether a file indents with tabs.
///
/// The first line that begins with a blank settles it, which is as much of
/// the file as anyone reads to find out. A file that never indents is a
/// file with no answer, and spaces are the answer that is wrong in the
/// fewest places.
fn indented_with_tabs(text: &Text) -> bool {
    text.rope()
        .lines()
        .find_map(|line| match line.chars().next() {
            Some('\t') => Some(true),
            Some(' ') => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

/// A digest of some bytes, for telling one version of a file from another.
///
/// Sixty-four bits of `SipHash`, which is what a `HashMap` key gets. Two
/// different files colliding would mean a save that went over somebody's
/// change without asking, so it is worth saying what the odds are: this is
/// asked once per save of files that are almost always the same file, and a
/// collision needs someone to have gone looking for one.
fn digest_of(bytes: &[u8]) -> u64 {
    use std::hash::Hasher as _;

    // `write` rather than `Hash::hash`, which adds a terminator per string
    // it is given: a rope that has been edited is chunked differently from
    // one just read, and the digest has to be about the bytes rather than
    // about where ropey happened to cut them.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

/// The same, of a document.
fn digest_of_text(text: &Text) -> u64 {
    use std::hash::Hasher as _;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for chunk in text.rope().chunks() {
        hasher.write(chunk.as_bytes());
    }
    hasher.finish()
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

/// Lines a hunk replaced, opened in place above the line that replaced
/// them.
///
/// Rows of the screen that the file does not have -- they have no line
/// numbers and nothing in the file is at them -- and the reader can walk
/// into them all the same: they are text, they are what the file used to
/// say, and a reader who can see them wants to read them and take a copy.
///
/// Held by the buffer because the caret and the viewport both need them:
/// one to have somewhere to be, the other to count the rows on screen. One
/// block, because one hunk is open at a time.
#[derive(Debug)]
pub struct Block {
    /// The line of the file they are drawn above.
    pub above: LineNumber,
    /// What they said, as a document of its own.
    ///
    /// A [`Text`] rather than a list of strings, so that everything the
    /// file's own lines get is theirs too: they wrap at the same width, tabs
    /// reach the same stops, a wide glyph takes two cells, the caret moves
    /// by visual rows, and a selection in them is a span like any other.
    /// The alternative was a second, smaller set of all of that, and a
    /// second set is a second set of bugs.
    pub text: Text,
    /// What kind of thing these rows are.
    pub kind: Held,
    /// Every line it was opened with, so folding has something to put back.
    ///
    /// The text is what is *on screen* and the fold rewrites it; this is what
    /// the block holds. Keeping both means everything downstream -- wrapping,
    /// the caret, a selection, a copy -- goes on asking the text and gets an
    /// answer about what the reader can actually see.
    full: Vec<String>,
    /// Whether it is showing its opening lines rather than all of them.
    folded: bool,
    /// What the commit did to the file the block hangs over, where that is
    /// known: lines added, lines taken away.
    ///
    /// Beside the message rather than inside it. It is a fact *about* the
    /// commit and not a thing the commit said, so it is not the reader's to
    /// copy back as if the author had written it, it does not wrap or fold
    /// as prose, and it can be drawn in the colours the margin already uses
    /// for the same fact.
    pub changed: Option<(usize, usize)>,
    /// How many lines the hunk actually replaced.
    ///
    /// Kept apart from the text, which cannot tell "nothing was removed"
    /// from "one empty line was removed": both join to the empty string.
    /// An added hunk is the first of those, and it has no rows at all.
    lines: usize,
    /// How many rows it takes at a width, worked out once for that width.
    ///
    /// The viewport's arithmetic asks for this inside loops -- it is the
    /// height of the thing between two lines of the file -- and wrapping
    /// every line of a long deletion each time round would be the frame's
    /// whole budget. Nothing in the block changes while it is open, so the
    /// answer only depends on the width.
    rows: std::cell::Cell<Option<(u16, usize)>>,
}

/// How many of a message's opening lines a folded block shows.
///
/// Five, because that is what a message is made of: the commit and who wrote
/// it, a blank, the subject, a blank, and the first line of the body. A
/// commit at a glance -- what it is called and the sentence it starts with --
/// and the rest a keypress away.
///
/// A commit's message hangs above a file the reader asked to see. Forty rows
/// of somebody's prose in front of it is the file pushed off the screen by
/// its own footnote.
pub const FOLDED_LINES: usize = 5;

/// The row that says a folded block has more behind it.
///
/// A row of its own rather than a mark in the gutter, because these rows do
/// not have one: they are not lines of the file and take no line numbers, so
/// there is nowhere beside them to put it.
const MORE: &str = "\u{2026}";

impl Block {
    /// Whether folding it would hide anything.
    ///
    /// Only a message. A hunk's removed lines are already a thing the reader
    /// opened and can close with the same key, and a second way to half-close
    /// it would be two answers to one question.
    #[must_use]
    pub fn can_fold(&self) -> bool {
        self.kind == Held::Message && self.full.len() > FOLDED_LINES
    }

    /// Whether it is showing its opening lines rather than all of them.
    #[must_use]
    pub const fn folded(&self) -> bool {
        self.folded
    }

    /// Shows all of it, or its opening lines and a mark.
    ///
    /// Rewrites the text, which is the whole trick: everything that draws a
    /// block, walks it or copies from it asks the text, so none of it has to
    /// learn what a fold is. The cached height goes with it -- it was
    /// measured against rows that are no longer there.
    fn refold(&mut self, folded: bool) {
        self.folded = folded && self.can_fold();
        let lines = match self.folded {
            true => {
                let mut kept: Vec<&str> = self
                    .full
                    .iter()
                    .take(FOLDED_LINES)
                    .map(String::as_str)
                    .collect();
                kept.push(MORE);
                kept.join("\n")
            }
            false => self.full.join("\n"),
        };
        self.text = Text::from_string(&lines);
        self.rows.set(None);
    }

    /// How many rows the whole block takes at a width.
    #[must_use]
    pub fn rows(&self, width: u16) -> usize {
        if self.is_empty() {
            return 0;
        }
        if let Some((at, rows)) = self.rows.get()
            && at == width
        {
            return rows;
        }
        let rows = (0..self.text.line_count())
            .map(|line| self.text.row_count(LineNumber::new(line), width))
            .sum();
        self.rows.set(Some((width, rows)));
        rows
    }

    /// How many rows come before one of its lines.
    #[must_use]
    pub fn rows_before(&self, line: LineNumber, width: u16) -> usize {
        (0..line.get().min(self.text.line_count()))
            .map(|line| self.text.row_count(LineNumber::new(line), width))
            .sum()
    }

    /// Which of its lines, and which row of that line, a row of the block
    /// is.
    ///
    /// The other direction of [`Block::rows_before`], for a caret arriving
    /// at a row of the screen: a page lands on a row, and what it lands on
    /// is a place in a line.
    #[must_use]
    pub fn place_at_row(&self, row: usize, width: u16) -> (LineNumber, usize) {
        let mut left = row;
        for line in 0..self.text.line_count() {
            let rows = self.text.row_count(LineNumber::new(line), width);
            if left < rows {
                return (LineNumber::new(line), left);
            }
            left -= rows;
        }
        let last = self.text.last_line();
        (last, self.text.row_count(last, width).saturating_sub(1))
    }

    /// Whether it has anything in it.
    ///
    /// An added hunk replaced nothing, and opening one is still worth doing
    /// -- the tint behind its lines is what says what kind of change it is
    /// -- but there is nothing there for a caret to walk into.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }
}

/// What an opened block is holding.
///
/// Two things are drawn the same way -- rows of text the file does not have,
/// between two lines it does -- and they are not the same thing, so they do
/// not read the same: lines a commit removed are gone, and a commit's
/// message is a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// The lines a hunk replaced.
    Removed,
    /// What a commit said about itself.
    Message,
}

/// Where the caret is while the reader is in an opened block.
///
/// Apart from the cursor rather than instead of it: those lines are not
/// places in the file, so nothing that asks the file about "here" -- a
/// language server, a jump, the next change -- may be answered from one.
/// The cursor stays on the line the block is anchored to and goes on
/// answering all of that; this says where the caret really is, what it has
/// selected, and nothing else.
///
/// The same [`Cursor`] the file has, because the block is the same kind of
/// thing: a text with rows, columns and a cell to aim for.
#[derive(Clone, Copy, Debug)]
struct InBlock {
    /// Which block, by the line it hangs above.
    above: LineNumber,
    /// Where the caret is in the block's own text.
    cursor: Cursor,
    /// Where a selection started, if one has.
    anchor: Option<Cursor>,
}

/// The run between two places, as a span, or nothing when they are the
/// same place.
///
/// The file's selection and a block's are the same shape: two ends in one
/// text, either way round.
fn span_between(anchor: Cursor, cursor: Cursor) -> Option<Span> {
    let (start, end) = if (anchor.line, anchor.column) <= (cursor.line, cursor.column) {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    ((start.line, start.column) != (end.line, end.column)).then_some(Span {
        line: start.line,
        column: start.column,
        end_line: end.line,
        end_column: end.column,
    })
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
    /// What can be folded away, and what the reader has folded.
    ///
    /// Beside the syntax rather than inside it: what a parse offers is a
    /// fact about the file, and which of those runs are folded is the
    /// reader's, kept across everything except a re-read.
    folds: folds::Folds,
    /// The hunks the reader has opened in this file, by the line each hangs
    /// above.
    ///
    /// A list, because a reader comparing two changes wants both on screen:
    /// one slot meant opening the second closed the first, and the two
    /// things a reader most wants to see together are the two they are
    /// deciding between.
    blocks: Vec<Block>,
    /// And where the caret is in it, if the reader has walked in.
    in_block: Option<InBlock>,
    /// What was on disk when this was last read or written.
    ///
    /// Nothing was kept about a file before, because nothing ever had to ask
    /// whether it had moved: obelus re-read it and that was the whole of the
    /// answer. A document somebody has edited cannot be re-read to find out,
    /// so the question has to be asked of the file rather than of its text.
    seen: Option<Seen>,
    /// Whether this file indents with tabs rather than with spaces.
    ///
    /// Read off the file rather than configured: what a file is indented
    /// with is a fact about the file, and a reader who has to tell obelus
    /// once per project has been asked something their files already say.
    /// A file with no indentation at all gets spaces, which is the answer
    /// that is wrong in the fewest places.
    tabs: bool,
    /// Whether the text differs from what is on disk.
    ///
    /// Worked out whenever the text moves rather than when it is asked:
    /// the status row and every row of the list of open files ask on every
    /// frame, and the answer can cost a pass over the file.
    dirty: bool,
    /// What somebody else has done to the file while this was dirty.
    ///
    /// Set instead of re-reading, because re-reading over an edit is losing
    /// it. What it is for is stopping the save that would lose the other
    /// side instead.
    disk: Disk,
    /// Everything the reader can put back.
    ///
    /// A journal of changes rather than of documents: a copy of the whole
    /// text per keystroke is the obvious thing and is unaffordable in a file
    /// worth reading.
    undo: undo::Undo,
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

        let mut folds = folds::Folds::default();
        folds.offer(folds::of(&text));
        let tabs = indented_with_tabs(&text);
        let seen = Some(Seen {
            stat: Stat::of(&path),
            digest: digest_of_text(&text),
            length: text.byte_length().get(),
        });

        Ok(Self {
            seen,
            path,
            content: Content::File,
            mode: Mode::Edit,
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
            folds,
            detached: false,
            viewport: Viewport {
                left: 0,
                top: LineNumber::new(0),
                top_row: 0,
            },
            blocks: Vec::new(),
            in_block: None,
            undo: undo::Undo::default(),
            disk: Disk::Unchanged,
            dirty: false,
            tabs,
        })
    }

    /// What can be folded here, and what is folded.
    #[must_use]
    pub const fn folds(&self) -> &folds::Folds {
        &self.folds
    }

    /// Folds the run at a line, or unfolds the one that starts there.
    ///
    /// The cursor comes with it: a fold whose lines are hidden cannot be
    /// left with the cursor inside them, and the line the reader pressed on
    /// is where the run now is.
    pub fn toggle_fold(&mut self, line: LineNumber) -> bool {
        if !self.folds.toggle(line) {
            return false;
        }
        self.bring_the_cursor_out();
        true
    }

    /// Folds every run the file offers, bringing the cursor out with them.
    pub fn fold_all(&mut self) -> bool {
        if !self.folds.fold_all() {
            return false;
        }
        self.bring_the_cursor_out();
        true
    }

    /// Opens everything that is folded.
    pub fn unfold_all(&mut self) -> bool {
        self.folds.unfold_all()
    }

    /// Walks the cursor back to the first line of whatever now hides it.
    ///
    /// A cursor on a line that is not on screen is a cursor the reader has
    /// lost, and the line a run starts on is the one line of it still
    /// there.
    fn bring_the_cursor_out(&mut self) {
        // An opened hunk hangs above a line, so folding that line away
        // leaves its rows undrawn -- and a caret in them is a caret nobody
        // can see, on a row the status bar would still name. The same thing
        // `refresh_changes` does when the diff those lines came from is
        // replaced: the block belonged to something that is no longer
        // there.
        // Each block that has lost the line it hangs above, and only
        // those: a run closed over one of them says nothing about the rest.
        let hidden: Vec<LineNumber> = self
            .blocks
            .iter()
            .map(|block| block.above)
            .filter(|above| self.folds.hides(*above))
            .collect();
        for above in hidden {
            self.close_block(above);
        }
        if !self.folds.hides(self.cursor.line) {
            return;
        }
        while self.folds.hides(self.cursor.line) && self.cursor.line.get() > 0 {
            self.cursor.line = self.cursor.line.saturating_sub(1);
        }
        self.cursor.column = self.text.clamp_column(self.cursor.line, self.cursor.column);
        self.clear_selection();
    }

    /// Unfolds whatever hides a line, for arriving at it.
    pub fn reveal(&mut self, line: LineNumber) -> bool {
        self.folds.reveal(line)
    }

    /// Reads a file as a commit had it.
    ///
    /// The path is the file's own so that everything which reads a name --
    /// the highlighting, the icon, the status bar -- goes on working. What
    /// is different is [`Content::Commit`], which is how the rest of obelus
    /// knows these bytes are not the ones on disk.
    #[must_use]
    pub fn at_commit(path: &Path, id: gix::ObjectId, contents: &str) -> Self {
        let mut buffer = Self::from_text(path, contents);
        buffer.content = Content::Commit { id };
        buffer
    }

    /// A buffer holding a commit's message and nothing else.
    ///
    /// No path, because there is no file: a commit is not one, and what it
    /// has to show is what it said. The message is a *block* rather than
    /// the buffer's text, for the reason it is one above a file: it has no
    /// lines of its own to go to, so it gets no line numbers -- and it is
    /// then the same rows, drawn the same way, whether the reader is
    /// looking at a commit or at one of its files.
    #[must_use]
    pub fn from_message(said: &[String]) -> Self {
        let mut buffer = Self::from_text(Path::new(""), "");
        buffer.open_held(LineNumber::new(0), said, Held::Message);
        buffer.enter_block(LineNumber::new(0));
        buffer
    }

    /// A buffer holding text that did not come from the path it names.
    fn from_text(path: &Path, contents: &str) -> Self {
        let text = Text::from_string(contents);
        let syntax =
            LanguageId::for_path(path).and_then(|language| SyntaxState::new(language, &text));
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let mut folds = folds::Folds::default();
        folds.offer(folds::of(&text));

        Self {
            path,
            content: Content::File,
            mode: Mode::Edit,
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
            folds,
            detached: false,
            viewport: Viewport {
                left: 0,
                top: LineNumber::new(0),
                top_row: 0,
            },
            blocks: Vec::new(),
            in_block: None,
            undo: undo::Undo::default(),
            seen: None,
            disk: Disk::Unchanged,
            dirty: false,
            tabs: false,
        }
    }

    /// Opens a hunk's removed lines in place, above the line that replaced
    /// them.
    ///
    /// The buffer holds them because both the caret and the viewport need
    /// them: one for somewhere to stand, the other to count the rows the
    /// screen really has.
    pub fn open_block(&mut self, above: LineNumber, lines: &[String]) {
        self.open_held(above, lines, Held::Removed);
    }

    /// The same, for rows that are not a hunk's.
    pub fn open_held(&mut self, above: LineNumber, lines: &[String], kind: Held) {
        let block = Block {
            above,
            kind,
            // Joined without a trailing newline: a text that ends in one
            // has an empty last line, and the block has exactly the lines
            // the hunk replaced.
            text: Text::from_string(&lines.join("\n")),
            full: lines.to_vec(),
            folded: false,
            changed: None,
            lines: lines.len(),
            rows: std::cell::Cell::new(None),
        };
        // Kept in the order they are drawn in, which is the order they are
        // looked up in: one per line, because one hunk is what a line has.
        match self
            .blocks
            .binary_search_by_key(&above, |block| block.above)
        {
            Ok(at) => self.blocks[at] = block,
            Err(at) => self.blocks.insert(at, block),
        }
        self.in_block = None;
    }

    /// Shows a block's opening lines, or all of them.
    ///
    /// Returns whether there was anything to fold, so a key that means "fold
    /// what the cursor is inside" can fall through to the file's own runs
    /// when it is not inside one of these.
    pub fn fold_block(&mut self, above: LineNumber, folded: bool) -> bool {
        let Ok(at) = self
            .blocks
            .binary_search_by_key(&above, |block| block.above)
        else {
            return false;
        };
        if !self.blocks[at].can_fold() {
            return false;
        }
        self.blocks[at].refold(folded);
        // The caret may have been standing on a row that has just gone. It
        // keeps its column where it can, because a reader who folds while
        // reading the third line expects to still be near the top -- not
        // thrown back to the start.
        let last = self.blocks[at].text.last_line();
        if let Some(inside) = self.in_block.as_mut()
            && inside.above == above
        {
            if inside.cursor.line > last {
                inside.cursor.line = last;
            }
            let columns = self.blocks[at].text.line_length(inside.cursor.line);
            if inside.cursor.column > columns {
                inside.cursor.column = columns;
            }
            // A selection across rows that are no longer there is not a
            // selection of anything a reader can point at.
            inside.anchor = None;
        }
        true
    }

    /// Says what the commit behind a block did to the file it hangs over.
    pub fn mark_block_change(&mut self, above: LineNumber, changed: (usize, usize)) {
        if let Ok(at) = self
            .blocks
            .binary_search_by_key(&above, |block| block.above)
        {
            self.blocks[at].changed = Some(changed);
        }
    }

    /// The same, the other way round from wherever it is now.
    pub fn toggle_block_fold(&mut self, above: LineNumber) -> bool {
        let folded = self.block_above(above).is_some_and(Block::folded);
        self.fold_block(above, !folded)
    }

    /// Closes it, and brings the caret back to the file if it was in there.
    pub fn close_block(&mut self, above: LineNumber) {
        self.blocks.retain(|block| block.above != above);
        if self.in_block.is_some_and(|at| at.above == above) {
            self.in_block = None;
        }
    }

    /// Closes every block holding one kind of thing.
    ///
    /// For when the diff they came from is replaced: a hunk's removed lines
    /// are lines of a file as it was, and the answer about what changed has
    /// moved on. A commit's message is not part of any diff and stays --
    /// closing it would take away the one thing a reader opened this
    /// version of the file to read.
    pub fn close_blocks(&mut self, kind: Held) {
        self.blocks.retain(|block| block.kind != kind);
        if self
            .in_block
            .is_some_and(|at| self.block_above(at.above).is_none())
        {
            self.in_block = None;
        }
    }

    /// Puts the caret at the top of a block, for a reader who is meant to
    /// land in it.
    ///
    /// A commit's message hangs above the first line of the file, so a
    /// cursor on that line is *below* the whole of it and the view scrolls
    /// past it to keep the cursor on screen. What the reader asked for was
    /// the message, so that is where the caret goes.
    pub fn enter_block(&mut self, above: LineNumber) {
        if self.block_above(above).is_none_or(Block::is_empty) {
            return;
        }
        self.in_block = Some(InBlock {
            above,
            cursor: Cursor {
                line: LineNumber::new(0),
                column: CharColumn::new(0),
                remembered_cell: DisplayColumn::new(0),
            },
            anchor: None,
        });
    }

    /// The blocks this file has open, in the order they are drawn.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// The hunk opened in place, if one is.
    #[must_use]
    pub fn block_above(&self, line: LineNumber) -> Option<&Block> {
        self.blocks
            .binary_search_by_key(&line, |block| block.above)
            .ok()
            .map(|at| &self.blocks[at])
    }

    /// Where the caret is in that block, if the reader has walked into it.
    #[must_use]
    pub fn in_block(&self) -> Option<(LineNumber, CharColumn)> {
        self.in_block.map(|at| (at.cursor.line, at.cursor.column))
    }

    /// The block in front of the reader, by the line it hangs above.
    ///
    /// A block sits between two lines and is reached from either side, so
    /// it is theirs from either side: the one the caret is inside, the one
    /// hanging above the line they are on, or -- for a reader who has just
    /// walked out of the top of one -- the one hanging above the line
    /// below them. Standing on a line of the file with a block below it is
    /// the weakest of the three, so a hunk the reader is actually on gets
    /// to open before that one closes.
    #[must_use]
    pub fn block_at_cursor(&self) -> Option<LineNumber> {
        self.caret_block()
            .or_else(|| self.block_above(self.cursor.line).map(|block| block.above))
    }

    /// The block hanging just below the cursor, if one does.
    #[must_use]
    pub fn block_below_cursor(&self) -> Option<LineNumber> {
        self.block_above(self.cursor.line.saturating_add(1))
            .map(|block| block.above)
    }

    /// Which block the caret is in, if it is in one.
    #[must_use]
    pub fn caret_block(&self) -> Option<LineNumber> {
        self.in_block.map(|at| at.above)
    }

    /// What is selected inside the block, in the block's own coordinates.
    #[must_use]
    pub fn block_selection(&self) -> Option<(LineNumber, Span)> {
        let at = self.in_block?;
        let anchor = at.anchor?;
        Some((at.above, span_between(anchor, at.cursor)?))
    }

    /// What the buffer holds.
    #[must_use]
    pub const fn content(&self) -> &Content {
        &self.content
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

    /// Shows this buffer's reading, from the top of it.
    ///
    /// The top, because a reading is a different document from the file:
    /// the cursor's line is not one of its rows, and what the viewport
    /// holds is read as a row while the mode is on. Every way into the
    /// mode goes through here, so there is one answer to "where does it
    /// start".
    pub fn show_reading(&mut self) {
        self.mode = Mode::Preview;
        self.scroll_by(
            isize::MIN / 2,
            TextArea {
                width: 1,
                height: 1,
                wrap: true,
            },
        );
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

    /// Whether the text differs from what is on disk.
    ///
    /// Asked of the journal rather than kept as a flag beside it. A flag is
    /// set by an edit and would have to be *un*set by undoing back to what
    /// was written -- which is not "one undo" but however many it takes,
    /// and which a flag has no way of recognising. The journal knows, so it
    /// is the one that answers.
    ///
    /// A third thing, and not either of the two beside it: `stale` means
    /// the last re-read *failed*, and what git says has changed is about
    /// `HEAD` rather than about disk.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Works out whether the text differs from what is on disk.
    ///
    /// The bytes, and nothing else. How many steps the journal has taken is
    /// not the question -- a reader who typed a character and took it back
    /// out has moved two steps forward and changed nothing -- and neither
    /// is how many it would take to get back, which is a different number
    /// again after an undo and some new work.
    ///
    /// Cheapest half first: a document that is not even the length of the
    /// one on disk is not it, and finding that out costs nothing. While
    /// somebody is typing, it never is -- which is what keeps a pass over
    /// the file out of the keystroke. No test can tell that half from
    /// leaving it out, because the digest answers correctly on its own; it
    /// is there so that the digest is hardly ever asked.
    fn settle_dirty(&mut self) {
        let Some(seen) = self.seen else {
            self.dirty = true;
            return;
        };
        self.dirty = self.text.byte_length().get() != seen.length
            || digest_of_text(&self.text) != seen.digest;
    }

    /// What somebody else has done to the file, as last established.
    ///
    /// Remembered rather than asked, because the status row and the list of
    /// open files draw it on every frame and asking means a `stat` per row
    /// -- which on a network filesystem is not a microsecond but a wait the
    /// reader can feel. What sets it is the watcher and the save, both of
    /// which are already doing I/O.
    #[must_use]
    pub const fn on_disk(&self) -> Disk {
        self.disk
    }

    /// Whether the file is definitely not there any more.
    ///
    /// Definitely: a path that cannot be asked about -- a directory along
    /// it that cannot be searched, say -- is not a deletion, and treating
    /// it as one would offer to write the file back somewhere it cannot go.
    #[must_use]
    pub fn file_gone(&self) -> bool {
        matches!(self.path.try_exists(), Ok(false))
    }

    /// Whether `stat` says the file is not the one that was read or written.
    ///
    /// Cheap, and wrong in both directions -- see [`Stat`]. What it is for
    /// is skipping the read: the commonest change reported about an open
    /// file is obelus's own save arriving back through the watcher, and
    /// that one looks exactly like what was just recorded.
    #[must_use]
    pub fn file_touched(&self) -> bool {
        Stat::of(&self.path) != self.seen.and_then(|seen| seen.stat)
    }

    /// Whether what is on disk is not what was last read or written.
    ///
    /// The honest question, and it reads the file to answer -- but only
    /// when the cheap one has already said something moved. A file that was
    /// touched, or rewritten with the bytes it already had (a formatter
    /// that found nothing to change, a checkout of the commit it was
    /// already on), is not something to make anybody choose about.
    ///
    /// Asked of the file rather than of the text: a document somebody has
    /// edited differs from disk whether or not disk has moved, so the text
    /// cannot answer it.
    #[must_use]
    pub fn file_differs(&self) -> bool {
        if !self.file_touched() {
            return false;
        }
        let Some(seen) = self.seen else {
            return true;
        };
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => digest_of(contents.as_bytes()) != seen.digest,
            // Gone, or no longer something obelus can read. Either way it is
            // not what was read from.
            Err(_) => true,
        }
    }

    /// Whether writing now would go over somebody else's change.
    ///
    /// Asked of disk at the moment of asking rather than of the flag the
    /// watcher sets, because the watcher is allowed to miss things: its
    /// events are dropped on queue overflow, never arrive at all over NFS
    /// and the like, and cannot report what happened before obelus started.
    /// A file is only ever written once, so this is the one place that has
    /// to be right.
    ///
    /// It corrects the flag on the way through, so a file that moved and
    /// then moved back stops asking.
    pub fn conflicted(&mut self) -> Disk {
        // Gone first: a file that is not there also "differs", and the two
        // have different answers -- there is nothing to take from a disk
        // with nothing on it.
        self.disk = match () {
            () if self.file_gone() => Disk::Deleted,
            () if self.file_differs() => Disk::Written,
            () => Disk::Unchanged,
        };
        self.disk
    }

    /// Says what somebody else did, so the screen can say it too.
    pub const fn mark_on_disk(&mut self, disk: Disk) {
        self.disk = disk;
    }

    /// What one step of indentation is in this file.
    ///
    /// A tab where the file is indented with tabs, and the setting's worth
    /// of spaces where it is not.
    #[must_use]
    pub fn indent(&self) -> String {
        match self.tabs {
            true => "\t".to_string(),
            false => " ".repeat(crate::text::tab_width()),
        }
    }

    /// Selects a span, as though the reader had dragged across it.
    pub fn select(&mut self, span: Span) {
        self.selection_anchor = Some(Cursor {
            line: span.line,
            column: span.column,
            ..self.cursor
        });
        self.cursor.line = span.end_line;
        self.cursor.column = span.end_column;
    }

    /// Where the word before the cursor begins.
    #[must_use]
    pub fn word_before(&self) -> (LineNumber, CharColumn) {
        moving::word_left(
            &self.text,
            &self.folds,
            self.cursor.line,
            self.cursor.column,
        )
    }

    /// Where the word after the cursor ends.
    #[must_use]
    pub fn word_after(&self) -> (LineNumber, CharColumn) {
        moving::word_right(
            &self.text,
            &self.folds,
            self.cursor.line,
            self.cursor.column,
        )
    }

    /// Says the text is what is on disk, after writing it there.
    pub fn settle(&mut self) {
        self.seen = Some(Seen {
            stat: Stat::of(&self.path),
            digest: digest_of_text(&self.text),
            length: self.text.byte_length().get(),
        });
        self.disk = Disk::Unchanged;
        // A save is something the reader did between one edit and the next:
        // typing that carried on across it would undo back past the thing
        // they wrote.
        self.undo.close();
        self.dirty = false;
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

    /// Replaces `span` with `with`, and keeps everything in step.
    ///
    /// The one way a document changes. Everything a buffer holds beside its
    /// text is measured against the text -- the parse tree, the folds, the
    /// blocks hanging between lines, the cursor, the version five caches key
    /// on -- and every one of them is a thing somebody adds without
    /// remembering the others. One door, and the list is here.
    ///
    /// Says whether anything changed. Replacing nothing with nothing did
    /// not, and neither did anything at all to a document that is not a file
    /// somebody could write.
    pub fn edit(&mut self, span: Span, with: &str, doing: undo::Doing) -> bool {
        let Some(step) = self.apply(span, with, doing) else {
            return false;
        };
        self.undo.record(step);
        true
    }

    /// Puts back the most recent group of changes.
    ///
    /// Backwards through the group, because a later step in it was made
    /// against the document a earlier one left behind.
    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.undo() else {
            return false;
        };
        for step in group.iter().rev() {
            let span = self.spanning(step.at, step.inserted.chars().count());
            self.apply(span, &step.removed, undo::Doing::Whole);
        }
        true
    }

    /// Does again what [`undo`](Self::undo) put back.
    pub fn redo(&mut self) -> bool {
        let Some(group) = self.undo.redo() else {
            return false;
        };
        for step in &group {
            let span = self.spanning(step.at, step.removed.chars().count());
            self.apply(span, &step.inserted, undo::Doing::Whole);
        }
        true
    }

    /// Whether there is anything to put back.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    /// Whether there is anything to do again.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Says the reader did something that was not an edit, so the next one
    /// starts a group of its own.
    pub const fn settle_undo(&mut self) {
        self.undo.close();
    }

    /// The whole document, as a span.
    ///
    /// For a caller replacing all of it in one change -- which is one step
    /// to undo rather than however many lines it touched.
    #[must_use]
    pub fn spanning_all(&self) -> Span {
        let last = self.text.last_line();
        Span {
            line: LineNumber::new(0),
            column: CharColumn::new(0),
            end_line: last,
            end_column: self.text.line_length(last),
        }
    }

    /// A span from an offset and a length in characters.
    fn spanning(&self, at: CharOffset, characters: usize) -> Span {
        let (line, column) = self.text.position(at);
        let (end_line, end_column) = self.text.position(CharOffset::new(at.get() + characters));
        Span {
            line,
            column,
            end_line,
            end_column,
        }
    }

    /// The change itself, without writing it down.
    ///
    /// Apart from [`edit`](Self::edit) because undoing is this without the
    /// journal: a step put back that wrote itself into the journal would be
    /// a step the reader could undo for ever.
    fn apply(&mut self, span: Span, with: &str, doing: undo::Doing) -> Option<undo::Step> {
        // A commit's version is a document nobody can write, and a reading
        // is a rendering of one rather than the bytes. The three guards on
        // `Content::Commit` elsewhere -- the watcher, the language server,
        // the list of open files -- are the same rule as this one.
        if !self.content.is_file() || self.mode != Mode::Edit {
            return None;
        }
        let empty = span.line == span.end_line && span.column == span.end_column;
        if empty && with.is_empty() {
            return None;
        }

        let lines_before = self.text.line_count();
        // What the edited line contributed to the runs around it, before it
        // was edited. An edit inside a line changes neither how far the line
        // is indented nor what it starts with -- which is most of what
        // typing is -- and then every run in the file is where it was.
        let one_line = span.line == span.end_line && !with.contains('\n');
        let shape = one_line.then(|| folds::shape_of(&self.text, span.line));
        let at = self.text.char_offset(span.line, span.column);
        let removed = match empty {
            true => String::new(),
            false => {
                let (removed, edit) = self.text.remove(span);
                self.reparse(&edit);
                removed
            }
        };
        if !with.is_empty() {
            let edit = self.text.insert(at, with);
            self.reparse(&edit);
        }

        // Five caches key on this -- the language server's idea of the
        // document and whether an answer about it is stale, the rendered
        // reading, the diff against git, the rows of an in-file search --
        // and every one of them is wrong until it moves.
        self.version = self.version.saturating_add(1);
        // Here rather than at the three doors that come through here, so
        // that a fourth cannot forget: after this, what the screen says
        // about the document is true of it.
        self.settle_dirty();

        // What the reader folded, moved by however many lines the edit added
        // or took away. `offer` would be right for a re-read and is wrong
        // here: it drops the lot, which per keystroke means a file that
        // unfolds itself as it is typed into.
        //
        // And only where the runs could have moved at all. Working them out
        // is a pass over the whole file, and it was by a long way the
        // slowest thing a keystroke did -- for an answer that, while
        // somebody types inside a line, is the answer it already had.
        let same_shape = shape.is_some_and(|was| was == folds::shape_of(&self.text, span.line));
        if !same_shape {
            let moved = self.text.line_count() as isize - lines_before as isize;
            self.folds
                .keep_across(folds::of(&self.text), span.line, moved);
        }

        // The diff those came from is stale the instant the text moves, and
        // they are anchored to line numbers the edit may have shifted.
        // `refresh_changes` closes them for the first of those reasons
        // already. A message block belongs to a commit's version, which the
        // guard above refused.
        self.close_blocks(Held::Removed);

        // Where the reader is left: at the end of what they put in, which is
        // where every editor leaves them and saves the caller doing this
        // arithmetic a second time. `place_cursor` clears the selection,
        // which an edit has just consumed.
        let (line, column) = self
            .text
            .position(CharOffset::new(at.get() + with.chars().count()));
        self.place_cursor(line, column);
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        Some(undo::Step {
            at,
            removed,
            inserted: with.to_string(),
            doing,
        })
    }

    /// Tells the parse about an edit, where there is a parse.
    fn reparse(&mut self, edit: &Edit) {
        let Some(state) = self.syntax.as_mut() else {
            return;
        };
        // Where the text moved, always: every node after the edit is at a
        // different offset now, and a tree that has not been told points at
        // the wrong bytes.
        state.note(edit);
        // What it means, only where the grammar can say between one
        // keystroke and the next. Where it cannot, the reader carries on
        // typing against the tree they had and it catches up when they
        // stop: letters appearing is what they are waiting for, and
        // colours a tenth of a second behind are colours nobody notices.
        if state.is_quick() {
            state.settle(&self.text);
        }
    }

    /// Works out what the text means, if a grammar too slow to keep up left
    /// that owed.
    pub fn settle_syntax(&mut self) {
        if let Some(state) = self.syntax.as_mut() {
            state.settle(&self.text);
        }
    }

    /// Says this document's grammar is too slow to keep up, whatever it
    /// really costs.
    pub fn hold_syntax_back_for_test(&mut self) {
        if let Some(state) = self.syntax.as_mut() {
            state.hold_back_for_test();
        }
    }

    /// Whether the tree is older than the text.
    #[must_use]
    pub fn syntax_is_behind(&self) -> bool {
        self.syntax.as_ref().is_some_and(SyntaxState::is_behind)
    }

    /// Writes the document to the file it came from.
    ///
    /// Beside it and renamed over it, which is how obelus writes its own
    /// settings and for the same reason: another program may be reading this
    /// file at this moment, a plain write truncates first, and a reader
    /// landing in that gap sees an empty file. A rename within one directory
    /// is the one filesystem operation with no such gap, and the temporary
    /// file is beside the target because rename is only atomic within a
    /// filesystem.
    ///
    /// Two things a source file needs that a settings file did not. The
    /// bytes are flushed to the disk before the rename, or a machine that
    /// loses power between them has a name pointing at an empty file rather
    /// than at either version. And the mode is carried over, or saving an
    /// executable script quietly disarms it.
    ///
    /// Through a symlink rather than over it: a rename replaces the *name*,
    /// and the name is the link.
    pub fn save(&mut self) -> Result<()> {
        let path = crate::config::resolved(&self.path);
        let beside = path.with_extension("obelus-writing");

        let mut file = std::fs::File::create(&beside)
            .with_context(|| format!("writing beside {}", path.display()))?;
        for chunk in self.text.rope().chunks() {
            std::io::Write::write_all(&mut file, chunk.as_bytes())?;
        }
        file.sync_all()?;
        // Whatever the file was allowed to be, it still is. A file that is
        // not there yet has nothing to copy and keeps what the system gives.
        if let Ok(data) = std::fs::metadata(&path) {
            let _ = file.set_permissions(data.permissions());
        }
        drop(file);

        std::fs::rename(&beside, &path)
            .with_context(|| format!("putting {} in place", path.display()))?;
        self.settle();
        tracing::info!(path = %path.display(), bytes = self.text.byte_length().get(), "saved");
        Ok(())
    }

    /// Replaces the document with what is on disk, keeping the way back.
    ///
    /// One of the two ways out of a file that moved under an edit, and the
    /// dangerous-looking one. An *edit* rather than a re-read, so that it
    /// goes into the journal and undo brings the reader's version back:
    /// [`reload`](Self::reload) forgets, which is right for a document
    /// nobody had changed and would be throwing work away here. It is the
    /// difference between a choice and an accident.
    ///
    /// The document counts as written from here, because what is on screen
    /// is now what is on disk -- and undoing past it makes it unwritten
    /// again, because then it is not.
    pub fn take_from_disk(&mut self) -> Result<bool> {
        let contents = std::fs::read_to_string(&self.path)
            .with_context(|| format!("re-reading {}", self.path.display()))?;
        // Where the reader was. `edit` leaves the cursor after what it put
        // in, which for the whole file is the end of it -- and somebody who
        // asked to see the other version wants to see the part they were
        // looking at.
        let cursor = self.cursor;
        let top = self.viewport.top;

        let changed = self.edit(self.spanning_all(), &contents, undo::Doing::Whole);
        self.cursor.line = self.text.clamp_line(cursor.line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, cursor.column);
        self.viewport.top = self.text.clamp_line(top);
        self.settle();
        self.stale = false;
        Ok(changed)
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
                // A file that is simply gone is not a re-read that went
                // wrong: `stale` means obelus could not find out what is
                // there, and here it has found out.
                match self.file_gone() {
                    true => self.disk = Disk::Deleted,
                    false => self.stale = true,
                }
                return Err(error).with_context(|| format!("re-reading {}", self.path.display()));
            }
        };
        // The file read, so whatever was wrong with it no longer is.
        self.stale = false;
        let new = Text::from_string(&contents);
        if new.rope() == self.text.rope() {
            // The bytes are what this was read from after all: a `touch`, a
            // formatter that found nothing to change, a checkout of the
            // commit it was already on. Recorded, so the next event about
            // this file is answered by one `stat` instead of another read.
            self.settle();
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

        // Whatever was folded was folded in the file that has just been
        // replaced: a run kept across a re-read would hide whichever lines
        // now sit at those numbers, which is a different file's fold. The
        // new text is asked what it offers, which is the same question the
        // open asked.
        self.folds.offer(folds::of(&self.text));
        // Whatever the reader could have put back was about the text that
        // has just been replaced.
        self.undo.forget();
        // And what is on screen is what is on disk again.
        self.settle();

        self.cursor.line = self.text.clamp_line(self.cursor.line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, self.cursor.column);
        // `top_row` is clamped by `scroll_into_view`, which knows the width.
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        Ok(true)
    }
}
