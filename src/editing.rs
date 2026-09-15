//! A text with a caret in it.
//!
//! What a reader does to words, wherever the words are. The file being read
//! is one of these and so is the box a note is written in; they differ in
//! what they are *about* -- a path, a syntax tree, a journal to undo through
//! -- and not in what down does, or what a word is, or where the caret goes
//! when a line runs out.
//!
//! That was two implementations until it was one. The box kept its lines as
//! `Vec<String>` and wrote its own stepping over them, so a reader who had
//! learnt `ctrl+left` in the file found it did nothing in a note -- and
//! every key added to one side was a key the other did not have.
//!
//! [`crate::text::Text`] is underneath: the rope, the wrapping, the columns
//! and the arithmetic that keeps a caret in the right cell when a line holds
//! wide glyphs. What is here is the caret, what it is anchored to, and the
//! two vocabularies -- a [`Motion`] to move it and a [`Typing`] to change
//! what it is in.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    coordinates::{CharColumn, CharOffset, DisplayColumn, LineNumber, Span},
    text::Text,
};

/// Which lines a motion steps over without ever stopping on one.
///
/// One method, because that is all a motion asks: a folded run is lines the
/// reader cannot see, and a caret that could stand inside one would be a
/// caret nobody could find. A box with nothing folded answers `false` to
/// everything, which is what `()` does.
pub trait Hides {
    /// Whether this line is folded away.
    fn hides(&self, line: LineNumber) -> bool;
}

impl Hides for () {
    fn hides(&self, _line: LineNumber) -> bool {
        false
    }
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
    pub(crate) remembered_cell: DisplayColumn,
}

impl Cursor {
    /// At the top of a text, before its first character.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            line: LineNumber::new(0),
            column: CharColumn::new(0),
            remembered_cell: DisplayColumn::new(0),
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

/// What a key puts into a text, or takes out of it.
///
/// Its own vocabulary rather than a command, because `Backspace`, `Delete`
/// and a letter are the same kind of thing to a text and different kinds of
/// thing to a key table: one of them is bound and the rest are typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typing {
    /// One character, as it was typed.
    Character(char),
    /// A line break.
    Newline,
    /// An indent.
    Tab,
    /// One step back out of an indent.
    Outdent,
    /// Take out what is behind the cursor.
    Backward,
    /// Take out what is in front of it.
    Forward,
    /// Take out the word behind it.
    BackwardWord,
    /// And the word in front.
    ForwardWord,
}

/// The visual row `rows` away in a text, crossing line boundaries and
/// stopping at either end of it.
fn step_rows(
    text: &Text,
    folds: &dyn Hides,
    mut line: LineNumber,
    mut row: usize,
    rows: isize,
    width: u16,
) -> (LineNumber, usize) {
    // The line this many lines on that is not folded away, or the end of
    // what is shown. A folded run is not somewhere the cursor can rest: its
    // lines are not on the screen, and a cursor the reader cannot see is a
    // cursor they have lost.
    let shown = |from: LineNumber, down: bool| {
        let last = text.last_line();
        let mut at = from;
        loop {
            if down {
                if at >= last {
                    return None;
                }
                at = at.saturating_add(1);
            } else {
                if at.get() == 0 {
                    return None;
                }
                at = at.saturating_sub(1);
            }
            if !folds.hides(at) {
                return Some(at);
            }
        }
    };
    for _ in 0..rows.unsigned_abs() {
        if rows > 0 {
            if row + 1 < text.row_count(line, width) {
                row += 1;
            } else if let Some(next) = shown(line, true) {
                line = next;
                row = 0;
            } else {
                break;
            }
        } else if row > 0 {
            row -= 1;
        } else if let Some(above) = shown(line, false) {
            line = above;
            row = text.row_count(line, width).saturating_sub(1);
        } else {
            break;
        }
    }
    (line, row)
}

/// Moves a cursor through a text, and says whether it moved at all.
///
/// The file's text or an opened block's: a caret in either is a place in a
/// text, and the answer to "what does down do here" is the same in both.
/// The answer is also how a block's edges are found -- a move that could
/// not go further up or down is a move off the end of one.
///
/// Up and down step one *visual* row, not one line. With wrapping on, a
/// long line is many rows tall, and stepping over all of them at once is
/// not what pressing down once looks like it should do.
fn move_within(
    text: &Text,
    folds: &dyn Hides,
    cursor: &mut Cursor,
    motion: Motion,
    width: u16,
) -> bool {
    let was = (cursor.line, cursor.column);
    let (row, _) = text.visual_position(cursor.line, cursor.column, width);
    let moved = |cursor: &Cursor| (cursor.line, cursor.column) != was;

    let rows = match motion {
        Motion::Up => -1,
        Motion::Down => 1,
        // Off either end of a line is the line beside it: what lies to
        // the left of column zero is the newline above, and where that
        // character is, is the end of that line. The word motions have
        // always crossed lines this way -- a plain arrow stopping dead at
        // the margin was the odd one out, and it is the one key a reader
        // holds down to walk through a file.
        //
        // Over a fold, the line beside it is the next one that is *shown*:
        // a caret cannot be put on a line nobody can see, and the two
        // steppers that answer this are the ones the word motions use.
        Motion::Left => {
            if cursor.column.get() == 0 {
                if let Some(above) = previous_line(folds, cursor.line) {
                    cursor.line = above;
                    cursor.column = text.line_length(above);
                }
            } else {
                cursor.column = cursor.column.saturating_sub(1);
            }
            remember(text, cursor, width);
            return moved(cursor);
        }
        Motion::Right => {
            if cursor.column >= text.line_length(cursor.line) {
                if let Some(below) = next_line(text, folds, cursor.line) {
                    cursor.line = below;
                    cursor.column = CharColumn::new(0);
                }
            } else {
                cursor.column = text.clamp_column(cursor.line, cursor.column.saturating_add(1));
            }
            remember(text, cursor, width);
            return moved(cursor);
        }
        // The start of the *row*, which on an unwrapped line is the start
        // of the line. A reader who wrapped their lines reads a row as a
        // line -- it is what `home` looks like it means, and jumping to the
        // far top of a paragraph is not what they pressed it for.
        Motion::LineStart => {
            let (first, end) = row_of(text, cursor.line, row, width);
            // The indent is not where a line starts to a reader: what they
            // reach for `home` to get to is the first thing they wrote.
            // Pressing it again goes the rest of the way, so the margin is
            // still one key away and neither of the two places needs a key
            // of its own.
            let written = first_written(text, cursor.line, first, end);
            cursor.column = match cursor.column == written {
                true => first,
                false => written,
            };
            remember(text, cursor, width);
            return moved(cursor);
        }
        // Both land on column zero rather than one keeping the column and
        // the other not. Symmetry is worth more here than either
        // convention: the last line of a file that ends in a newline is
        // empty, so its start and its end are the same place anyway.
        Motion::DocumentStart => {
            cursor.line = LineNumber::new(0);
            cursor.column = CharColumn::new(0);
            remember(text, cursor, width);
            return moved(cursor);
        }
        Motion::DocumentEnd => {
            // The last line there is to stand on, which is not the last
            // line of the file when a fold reaches it.
            cursor.line = text.last_line();
            while folds.hides(cursor.line) && cursor.line.get() > 0 {
                cursor.line = cursor.line.saturating_sub(1);
            }
            cursor.column = CharColumn::new(0);
            remember(text, cursor, width);
            return moved(cursor);
        }
        // Past the last character, where a cursor legitimately sits -- and
        // of the row rather than the line, for the reason `home` is.
        Motion::LineEnd => {
            let (first, end) = row_of(text, cursor.line, row, width);
            // In front of the row's last character, on a row that is not
            // the line's last. Past it is where the row below begins: one
            // place with two names, and a caret standing there is drawn on
            // the row below, which reads as the key having missed. On a row
            // that fills the screen it is worse -- there is no cell out
            // past the last column, and the caret is not drawn at all.
            //
            // Zed does the same and for the same reason, though it draws in
            // pixels and could put a caret out there: its `line_end` clips
            // the position with `Bias::Left`, which steps back a column
            // when it lands on the break a soft wrap inserted. Movement
            // there is in display coordinates, where the two places are
            // different -- but the selection is stored as a position in the
            // text, where they are one, and stepping back is what keeps the
            // caret off the seam entirely. Nothing then has to remember
            // which side of it the reader meant.
            //
            // The last row of a line keeps the end it has always had: there
            // is no row below for that place to belong to.
            cursor.column = match row + 1 < text.row_count(cursor.line, width) {
                true => CharColumn::new(end.get().saturating_sub(1)).max(first),
                false => end,
            };
            remember(text, cursor, width);
            return moved(cursor);
        }
        // A word at a time. What counts as one is the same rule every
        // editor uses without saying so: a run of letters, digits and
        // underscores is a word, a run of anything else that is not blank
        // is a word, and blank is what lies between them.
        Motion::WordLeft => {
            let (line, column) = word_left(text, folds, cursor.line, cursor.column);
            cursor.line = line;
            cursor.column = column;
            remember(text, cursor, width);
            return moved(cursor);
        }
        Motion::WordRight => {
            let (line, column) = word_right(text, folds, cursor.line, cursor.column);
            cursor.line = line;
            cursor.column = column;
            remember(text, cursor, width);
            return moved(cursor);
        }
    };

    let (line, row) = step_rows(text, folds, cursor.line, row, rows, width);
    cursor.line = line;
    // Aim for the remembered cell, then take whatever column covers it on
    // the row arrived at.
    cursor.column = text.column_in_row(line, row, cursor.remembered_cell, width);
    moved(cursor)
}

/// Where a visual row begins and ends, in the line's own columns.
///
/// The whole line where it is not wrapped, which is what makes `home` and
/// `end` mean what they have always meant for a reader who left wrapping
/// off.
fn row_of(text: &Text, line: LineNumber, row: usize, width: u16) -> (CharColumn, CharColumn) {
    let rows = text.wrap_rows(line, width);
    rows.get(row).map_or_else(
        || (CharColumn::new(0), text.line_length(line)),
        |wrapped| (wrapped.first, wrapped.end),
    )
}

/// The first character of a row that is not blank.
///
/// The end of the row where the whole of it is blank: a line of spaces has
/// nothing written on it, and the far end is the only place on it a reader
/// could mean.
fn first_written(text: &Text, line: LineNumber, first: CharColumn, end: CharColumn) -> CharColumn {
    let characters: Vec<char> = text.line(line).chars().collect();
    let mut at = first.get();
    while at < end.get() && characters.get(at).is_some_and(|c| c.is_whitespace()) {
        at += 1;
    }
    CharColumn::new(at)
}

/// Whether a character is part of a word rather than between words.
pub(crate) fn wordish(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Which end of a selection a motion collapses it to, if it collapses it.
///
/// Only the two arrows that move by a character: `home` and `end` are
/// about the line rather than the selection, the word motions are about
/// words, and up and down carry on from the caret because a selection has
/// no top or bottom end that a reader is pointing at.
pub(crate) fn end_of(span: Span, motion: Motion) -> Option<(LineNumber, CharColumn)> {
    match motion {
        Motion::Left => Some((span.line, span.column)),
        Motion::Right => Some((span.end_line, span.end_column)),
        _ => None,
    }
}

/// Which of the three kinds a character is, for telling one word from the
/// next.
///
/// Three rather than two, so that `foo.bar` is three words rather than one:
/// punctuation is not blank, and a reader stepping through code expects to
/// stop at the dot.
fn kind_of(character: char) -> u8 {
    match character {
        _ if character.is_whitespace() => 0,
        _ if wordish(character) => 1,
        _ => 2,
    }
}

/// The characters of a line, as a vector the walks below can index.
fn characters(text: &Text, line: LineNumber) -> Vec<char> {
    text.line(line).chars().collect()
}

/// The start of the word to the left, stepping onto the line above where
/// there is nothing to the left on this one.
pub(crate) fn word_left(
    text: &Text,
    folds: &dyn Hides,
    mut line: LineNumber,
    mut column: CharColumn,
) -> (LineNumber, CharColumn) {
    loop {
        let characters = characters(text, line);
        let mut at = column.get().min(characters.len());
        // Over the blank behind the cursor, then over the run it lands in.
        while at > 0 && kind_of(characters[at - 1]) == 0 {
            at -= 1;
        }
        if at > 0 {
            let kind = kind_of(characters[at - 1]);
            while at > 0 && kind_of(characters[at - 1]) == kind {
                at -= 1;
            }
            return (line, CharColumn::new(at));
        }
        // Nothing left on this line: the end of the line above, which is
        // where the character to the left of column zero actually is.
        let Some(above) = previous_line(folds, line) else {
            return (line, CharColumn::new(0));
        };
        line = above;
        column = text.line_length(line);
        if column.get() > 0 {
            continue;
        }
        return (line, column);
    }
}

/// Past the end of the word to the right, stepping onto the line below.
pub(crate) fn word_right(
    text: &Text,
    folds: &dyn Hides,
    mut line: LineNumber,
    mut column: CharColumn,
) -> (LineNumber, CharColumn) {
    loop {
        let characters = characters(text, line);
        let mut at = column.get().min(characters.len());
        while at < characters.len() && kind_of(characters[at]) == 0 {
            at += 1;
        }
        if at < characters.len() {
            let kind = kind_of(characters[at]);
            while at < characters.len() && kind_of(characters[at]) == kind {
                at += 1;
            }
            return (line, CharColumn::new(at));
        }
        let Some(below) = next_line(text, folds, line) else {
            return (line, CharColumn::new(characters.len()));
        };
        line = below;
        column = CharColumn::new(0);
        if text.line_length(line).get() > 0 {
            continue;
        }
        return (line, column);
    }
}

/// The line above, skipping whatever a fold has hidden.
fn previous_line(folds: &dyn Hides, line: LineNumber) -> Option<LineNumber> {
    let mut above = line.get().checked_sub(1)?;
    while folds.hides(LineNumber::new(above)) {
        above = above.checked_sub(1)?;
    }
    Some(LineNumber::new(above))
}

/// The line below, likewise.
fn next_line(text: &Text, folds: &dyn Hides, line: LineNumber) -> Option<LineNumber> {
    let last = text.last_line().get();
    let mut below = line.get() + 1;
    while below <= last && folds.hides(LineNumber::new(below)) {
        below += 1;
    }
    (below <= last).then(|| LineNumber::new(below))
}

/// Records the cell a cursor is at, as the column to aim for later.
fn remember(text: &Text, cursor: &mut Cursor, width: u16) {
    let (_, cell) = text.visual_position(cursor.line, cursor.column, width);
    cursor.remembered_cell = cell;
}

/// The motion a navigation key stands for.
///
/// A modifier obelus has no meaning for disqualifies the key: `ctrl+left` is a
/// word motion it does not have yet, and treating it as a plain left would be
/// a wrong answer rather than a missing one.
pub(crate) fn motion_for(key: &KeyEvent) -> Option<(Motion, bool)> {
    // Judged the same way the key table judges, so a key means the same thing
    // in both places or nothing in both places.
    let modifiers = crate::keymap::modifiers_of(key)?;

    match (modifiers, key.code) {
        // Not `ctrl+PageUp`/`ctrl+PageDown`: those mean previous and next tab
        // almost everywhere, and the nearest thing obelus has to a tab is a
        // buffer, so they are worth leaving free.
        // A word at a time, which is the other thing `ctrl` and an arrow
        // mean everywhere a reader has been.
        (KeyModifiers::CONTROL, KeyCode::Left) => Some((Motion::WordLeft, false)),
        (KeyModifiers::CONTROL, KeyCode::Right) => Some((Motion::WordRight, false)),
        (m, KeyCode::Left) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::WordLeft, true))
        }
        (m, KeyCode::Right) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::WordRight, true))
        }
        (KeyModifiers::CONTROL, KeyCode::Home) => Some((Motion::DocumentStart, false)),
        (KeyModifiers::CONTROL, KeyCode::End) => Some((Motion::DocumentEnd, false)),
        // With shift as well, the same two motions extend the selection.
        // Without these the ends of the file are the one place a selection
        // cannot reach, and the rule that a modifier obelus has no meaning
        // for disqualifies the key made them do nothing at all.
        (m, KeyCode::Home) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::DocumentStart, true))
        }
        (m, KeyCode::End) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::DocumentEnd, true))
        }
        (KeyModifiers::SHIFT, code) => match code {
            KeyCode::Left => Some((Motion::Left, true)),
            KeyCode::Right => Some((Motion::Right, true)),
            KeyCode::Up => Some((Motion::Up, true)),
            KeyCode::Down => Some((Motion::Down, true)),
            KeyCode::Home => Some((Motion::LineStart, true)),
            KeyCode::End => Some((Motion::LineEnd, true)),
            _ => None,
        },
        (KeyModifiers::NONE, code) => match code {
            KeyCode::Left => Some((Motion::Left, false)),
            KeyCode::Right => Some((Motion::Right, false)),
            KeyCode::Up => Some((Motion::Up, false)),
            KeyCode::Down => Some((Motion::Down, false)),
            KeyCode::Home => Some((Motion::LineStart, false)),
            KeyCode::End => Some((Motion::LineEnd, false)),
            _ => None,
        },
        _ => None,
    }
}

/// What a key types, if it types anything.
///
/// Shift is allowed through: it is how a capital arrives, and the character
/// crossterm reports already has it applied. Every other modifier is
/// somebody else's -- a `ctrl` chord is a command, and typing one would put
/// a character in where the reader asked for an action.
pub(crate) fn typing_for(key: &KeyEvent) -> Option<Typing> {
    let modifiers = crate::keymap::modifiers_of(key)?;
    // The one pair of `ctrl` chords that type rather than command: they
    // take out a word, which is the pair of `ctrl` with the arrows moving
    // over one. Before the rule below, which is what refuses the rest.
    if modifiers == KeyModifiers::CONTROL {
        return match key.code {
            KeyCode::Backspace => Some(Typing::BackwardWord),
            KeyCode::Delete => Some(Typing::ForwardWord),
            _ => None,
        };
    }
    let bare = modifiers == KeyModifiers::NONE;
    let shifted = modifiers == KeyModifiers::SHIFT;
    if !bare && !shifted {
        return None;
    }
    match key.code {
        KeyCode::Char(character) => Some(Typing::Character(character)),
        KeyCode::Enter if bare => Some(Typing::Newline),
        KeyCode::Tab if bare => Some(Typing::Tab),
        // Whichever way the terminal reports it: some send `BackTab` with
        // shift still on it and some send it bare.
        KeyCode::BackTab => Some(Typing::Outdent),
        KeyCode::Backspace if bare => Some(Typing::Backward),
        KeyCode::Delete if bare => Some(Typing::Forward),
        _ => None,
    }
}

/// A text with a caret in it, and what the caret has hold of.
///
/// The three places obelus puts a caret were three of these written by hand:
/// the file being read, a block opened above one of its lines, and the box a
/// note or a message is written in. They agreed about nothing except by
/// accident, so `ctrl+left` walked a word in the first and did nothing in
/// the last.
///
/// What is *not* here is everything that makes those three different: a
/// path, a syntax tree, a journal to undo through, a viewport. Those belong
/// to whoever holds one of these.
#[derive(Clone, Debug)]
pub struct Editing {
    text: Text,
    cursor: Cursor,
    /// Where the selection started, if one is being made.
    ///
    /// The cursor is the other end. Keeping the anchor rather than a range
    /// means changing direction naturally shrinks the selection and can pass
    /// back through it without a special case.
    anchor: Option<Cursor>,
}

impl Editing {
    /// One holding this text, with the caret at the start of it.
    #[must_use]
    pub fn new(said: &str) -> Self {
        Self::over(Text::from_string(said))
    }

    /// The same, over a text that is already made.
    ///
    /// What a file needs: its language and its folds are both read off the
    /// text, and they are read before there is anywhere for a caret to be.
    #[must_use]
    pub fn over(text: Text) -> Self {
        Self {
            text,
            cursor: Cursor::start(),
            anchor: None,
        }
    }

    /// What it holds.
    #[must_use]
    pub const fn text(&self) -> &Text {
        &self.text
    }

    /// The same, to be changed.
    pub const fn text_mut(&mut self) -> &mut Text {
        &mut self.text
    }

    /// Where the caret is.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// The same, for a holder doing arithmetic of its own with it.
    ///
    /// What a viewport, a fold and an opened block make necessary: those
    /// work out a place in rows and cells that this knows nothing about,
    /// and then the caret has to go there.
    pub const fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// Puts the caret somewhere, clamped to a place the text has.
    ///
    /// Arriving rather than moving: the cell a vertical move aims for is
    /// left alone, because that is what the reader was last aiming at along
    /// a line and being *put* somewhere is not a step along one.
    pub fn arrive(&mut self, line: LineNumber, column: CharColumn) {
        let line = self.text.clamp_line(line);
        self.cursor.line = line;
        self.cursor.column = self.text.clamp_column(line, column);
    }

    /// The same, and the caret is now aiming at the cell it landed on.
    ///
    /// For a caret that got there by typing: what was typed is where the
    /// reader is, so it is what down should aim for.
    pub fn place(&mut self, line: LineNumber, column: CharColumn, width: u16) {
        self.arrive(line, column);
        remember(&self.text, &mut self.cursor, width.max(1));
    }

    /// What it says, as one string.
    #[must_use]
    pub fn said(&self) -> String {
        self.text.rope().to_string()
    }

    /// Replaces the whole of it, leaving the caret at the end.
    pub fn replace(&mut self, said: &str, width: u16) {
        self.text = Text::from_string(said);
        self.anchor = None;
        let last = self.text.last_line();
        self.place(last, self.text.line_length(last), width);
    }

    /// Whether it says nothing, or nothing but blanks.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.text.rope().to_string().trim().is_empty()
    }

    /// Moves the caret, leaving hold of anything alone.
    ///
    /// The two below are this with the anchor dropped or kept. Both are
    /// here separately because a holder with its own rules about when a
    /// selection starts -- the file has them, because a caret in an opened
    /// block selects in the block and not in the file -- wants neither.
    pub fn step(&mut self, motion: Motion, hides: &dyn Hides, width: u16) -> bool {
        move_within(&self.text, hides, &mut self.cursor, motion, width.max(1))
    }

    /// Moves the caret, and says whether it moved at all.
    pub fn move_to(&mut self, motion: Motion, hides: &dyn Hides, width: u16) -> bool {
        self.anchor = None;
        self.step(motion, hides, width)
    }

    /// The same, dragging a selection behind it.
    pub fn extend_to(&mut self, motion: Motion, hides: &dyn Hides, width: u16) -> bool {
        self.hold();
        self.step(motion, hides, width)
    }

    /// Takes hold from here, unless it is holding something already.
    pub fn hold(&mut self) {
        self.anchor.get_or_insert(self.cursor);
    }

    /// Takes hold from there, whatever it was holding.
    pub const fn hold_from(&mut self, anchor: Cursor) {
        self.anchor = Some(anchor);
    }

    /// What is selected, if anything is.
    #[must_use]
    pub fn selection(&self) -> Option<Span> {
        let anchor = self.anchor?;
        span_between(anchor, self.cursor)
    }

    /// And what it says.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        Some(self.text.text_in(self.selection()?))
    }

    /// Lets go of it, leaving the caret where it is.
    pub const fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Takes hold of the whole of it.
    pub fn select_all(&mut self, width: u16) {
        self.anchor = Some(Cursor::start());
        let last = self.text.last_line();
        self.place(last, self.text.line_length(last), width);
        self.anchor = Some(Cursor::start());
    }

    /// Whether the reader has hold of anything.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }
}

/// The run between two places, or nothing where they are the same place.
fn span_between(anchor: Cursor, cursor: Cursor) -> Option<Span> {
    let (from, to) = match (anchor.line, anchor.column) <= (cursor.line, cursor.column) {
        true => (anchor, cursor),
        false => (cursor, anchor),
    };
    ((from.line, from.column) != (to.line, to.column)).then_some(Span {
        line: from.line,
        column: from.column,
        end_line: to.line,
        end_column: to.column,
    })
}

impl Editing {
    /// Whatever a key means to the text, or `false` for one that means
    /// nothing to it.
    ///
    /// The keys of a text, in one place. Which ones those are is a rule
    /// rather than a preference, and a rule written twice is a rule that
    /// will be true in one place -- which is what it was, and why a reader
    /// who had learnt `ctrl+left` in a file found it did nothing in a note.
    ///
    /// `enter` and `esc` are *not* here. Finishing and giving up are the
    /// caller's, and the callers answer them differently: one sends a
    /// message, one keeps a note, one puts a line break in. A text deciding
    /// either would be deciding something it knows nothing about.
    ///
    /// Up and down answer `false` at the ends, so a caller with rows above
    /// and below can step out rather than have the key swallowed.
    pub fn handle_key(&mut self, key: &KeyEvent, hides: &dyn Hides, width: u16) -> bool {
        if let Some((motion, extend)) = motion_for(key) {
            return match extend {
                true => self.extend_to(motion, hides, width),
                false => self.move_to(motion, hides, width),
            };
        }
        match typing_for(key) {
            Some(typing) => {
                self.apply(typing, hides, width);
                true
            }
            None => false,
        }
    }

    /// Puts something in, or takes something out.
    ///
    /// Over a selection, every one of them replaces it: what the reader has
    /// hold of is what they meant, and a letter typed onto a selection is
    /// the selection gone and the letter in its place.
    pub fn apply(&mut self, typing: Typing, hides: &dyn Hides, width: u16) {
        if let Some(span) = self.selection() {
            let taken = matches!(
                typing,
                Typing::Backward | Typing::Forward | Typing::BackwardWord | Typing::ForwardWord
            );
            self.remove(span, width);
            if taken {
                return;
            }
        }
        match typing {
            Typing::Character(character) => self.put(&character.to_string(), width),
            Typing::Newline => self.put("\n", width),
            Typing::Tab => self.put("\t", width),
            // Nothing yet: what an outdent means depends on what the lines
            // around it are indented with, which is the file's business.
            Typing::Outdent => {}
            Typing::Backward => {
                let from = self.cursor;
                if self.move_to(Motion::Left, hides, width) {
                    self.cut_between(self.cursor, from, width);
                }
            }
            Typing::Forward => {
                let from = self.cursor;
                if self.move_to(Motion::Right, hides, width) {
                    self.cut_between(from, self.cursor, width);
                }
            }
            Typing::BackwardWord => {
                let from = self.cursor;
                if self.move_to(Motion::WordLeft, hides, width) {
                    self.cut_between(self.cursor, from, width);
                }
            }
            Typing::ForwardWord => {
                let from = self.cursor;
                if self.move_to(Motion::WordRight, hides, width) {
                    self.cut_between(from, self.cursor, width);
                }
            }
        }
    }

    /// Writes something in at the caret, and leaves the caret after it.
    fn put(&mut self, what: &str, width: u16) {
        let at = self.text.char_offset(self.cursor.line, self.cursor.column);
        self.text.insert(at, what);
        let (line, column) = self
            .text
            .position(CharOffset::new(at.get() + what.chars().count()));
        self.anchor = None;
        self.place(line, column, width);
    }

    /// Takes a run out, and leaves the caret where it began.
    fn remove(&mut self, span: Span, width: u16) {
        self.text.remove(span);
        self.anchor = None;
        self.place(span.line, span.column, width);
    }

    /// The same, between two places.
    fn cut_between(&mut self, from: Cursor, to: Cursor, width: u16) {
        self.remove(
            Span {
                line: from.line,
                column: from.column,
                end_line: to.line,
                end_column: to.column,
            },
            width,
        );
    }
}
