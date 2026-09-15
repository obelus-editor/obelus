//! Which navigation a key means.
//!
//! The counterpart of [`crate::keymap`], which is the table of *commands*:
//! these keys are the high-frequency ones -- the arrows, the paging keys,
//! the ends of a line -- and they are not commands, because `:cursor.up`
//! called by name in a palette means nothing.

use super::*;

/// How far a key moves a rendered view, in rows.
///
/// The arrows, the paging keys and the ends of the document, over a document
/// whose rows are all there is: no columns, no cursor, nothing to remember.
pub(super) fn view_step(key: &KeyEvent, height: u16) -> Option<isize> {
    let modifiers = keymap::modifiers_of(key)?;
    let page = isize::from(height.max(1) as i16).max(1);
    match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Down) => Some(1),
        (KeyModifiers::NONE, KeyCode::Up) => Some(-1),
        (KeyModifiers::NONE, KeyCode::PageDown) => Some(page),
        (KeyModifiers::NONE, KeyCode::PageUp) => Some(-page),
        (KeyModifiers::CONTROL, KeyCode::End) => Some(isize::MAX),
        (KeyModifiers::CONTROL, KeyCode::Home) => Some(isize::MIN),
        _ => None,
    }
}

/// How many screenfuls a bare paging key moves the file by.
///
/// Separate from the motions because paging is not one: what moves is the
/// window on the file, not the place in it.
pub(super) fn editor_paging(key: &KeyEvent) -> Option<(isize, bool)> {
    match (keymap::modifiers_of(key)?, key.code) {
        (KeyModifiers::NONE, KeyCode::PageDown) => Some((1, false)),
        (KeyModifiers::NONE, KeyCode::PageUp) => Some((-1, false)),
        (KeyModifiers::SHIFT, KeyCode::PageDown) => Some((1, true)),
        (KeyModifiers::SHIFT, KeyCode::PageUp) => Some((-1, true)),
        _ => None,
    }
}

/// The motion a navigation key stands for.
///
/// A modifier obelus has no meaning for disqualifies the key: `ctrl+left` is a
/// word motion it does not have yet, and treating it as a plain left would be
/// a wrong answer rather than a missing one.
pub(super) fn motion_for(key: &KeyEvent) -> Option<(Motion, bool)> {
    // Judged the same way the key table judges, so a key means the same thing
    // in both places or nothing in both places.
    let modifiers = keymap::modifiers_of(key)?;

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

/// What a key puts into the document, or takes out of it.
///
/// Its own thing rather than a command, because `Backspace`, `Delete`,
/// `Enter` and `Tab` cannot be commands: [`crate::keymap::why_not`] refuses
/// all four with "every list and box takes this one itself", and a printable
/// character is not a name anybody would type in a palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Typing {
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

/// What a key types, if it types anything.
///
/// Shift is allowed through: it is how a capital arrives, and the character
/// crossterm reports already has it applied. Every other modifier is
/// somebody else's -- a `ctrl` chord is a command, and typing one would put
/// a character in where the reader asked for an action.
pub(super) fn typing_for(key: &KeyEvent) -> Option<Typing> {
    let modifiers = keymap::modifiers_of(key)?;
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

impl App {
    /// Puts what a key typed into the document, or takes out what it asked
    /// to remove.
    ///
    /// One place for all five, because they differ only in what span they
    /// are about and what goes in it -- and because every one of them has
    /// the same answer when there is a selection: the selection is what it
    /// is about.
    pub(super) fn typed(&mut self, typing: Typing) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let selection = buffer.selection();
        let cursor = buffer.cursor();
        let text = buffer.text();

        // Indenting is about lines, not about what is selected in them, so
        // it comes before the rule that a selection is what a key is about.
        // `tab` over a selection used to put the indent *in place of* it,
        // which is the one thing no reader has ever meant by it.
        match typing {
            Typing::Outdent => return self.shift_indent(false),
            Typing::Tab if selection.is_some() => return self.shift_indent(true),
            _ => {}
        }

        let (span, with, doing) = match typing {
            // Whatever is selected goes, and what was typed takes its place.
            // Backspace and delete over a selection mean the selection, not
            // the character beside it, which is the one thing every reader
            // expects of them without being told.
            _ if selection.is_some() => {
                let span = selection.unwrap_or(crate::coordinates::Span {
                    line: cursor.line,
                    column: cursor.column,
                    end_line: cursor.line,
                    end_column: cursor.column,
                });
                (span, put_in(typing), crate::buffer::undo::Doing::Whole)
            }
            // A closing bracket typed as the first thing on a line lines
            // up with whatever opened the block, rather than sitting one
            // step in from it -- which is where the indent this line
            // inherited would have left it.
            Typing::Character(character)
                if crate::syntax::brackets::closes(character)
                    && text
                        .line(cursor.line)
                        .chars()
                        .take(cursor.column.get())
                        .all(char::is_whitespace) =>
            {
                let out = cursor
                    .column
                    .get()
                    .saturating_sub(crate::text::tab_width().min(cursor.column.get()));
                (
                    crate::coordinates::Span {
                        line: cursor.line,
                        column: CharColumn::new(out),
                        end_line: cursor.line,
                        end_column: cursor.column,
                    },
                    character.to_string(),
                    crate::buffer::undo::Doing::Whole,
                )
            }
            Typing::Outdent => return,
            // A word at a time, by the rule the arrow keys move by.
            Typing::BackwardWord => {
                let (line, column) = buffer.word_before();
                if (line, column) == (cursor.line, cursor.column) {
                    return;
                }
                (
                    crate::coordinates::Span {
                        line,
                        column,
                        end_line: cursor.line,
                        end_column: cursor.column,
                    },
                    String::new(),
                    crate::buffer::undo::Doing::Deleting,
                )
            }
            Typing::ForwardWord => {
                let (end_line, end_column) = buffer.word_after();
                if (end_line, end_column) == (cursor.line, cursor.column) {
                    return;
                }
                (
                    crate::coordinates::Span {
                        line: cursor.line,
                        column: cursor.column,
                        end_line,
                        end_column,
                    },
                    String::new(),
                    crate::buffer::undo::Doing::Deleting,
                )
            }
            Typing::Character(_) | Typing::Newline | Typing::Tab => {
                let at = crate::coordinates::Span {
                    line: cursor.line,
                    column: cursor.column,
                    end_line: cursor.line,
                    end_column: cursor.column,
                };
                let doing = match typing {
                    Typing::Character(_) => crate::buffer::undo::Doing::Typing,
                    _ => crate::buffer::undo::Doing::Whole,
                };
                let what = match typing {
                    Typing::Newline => {
                        "\n".to_string() + &indent_after(text, cursor.line, cursor.column)
                    }
                    Typing::Tab => buffer.indent(),
                    _ => put_in(typing),
                };
                (at, what, doing)
            }
            // The character behind the cursor, which is the end of the line
            // above when there is nothing behind it on this one.
            Typing::Backward => {
                let (line, column) = match cursor.column.get() {
                    0 if cursor.line.get() == 0 => return,
                    0 => {
                        let above = cursor.line.saturating_sub(1);
                        (above, text.line_length(above))
                    }
                    _ => (cursor.line, cursor.column.saturating_sub(1)),
                };
                (
                    crate::coordinates::Span {
                        line,
                        column,
                        end_line: cursor.line,
                        end_column: cursor.column,
                    },
                    String::new(),
                    crate::buffer::undo::Doing::Deleting,
                )
            }
            // And the one in front, which is the line break when there is
            // nothing in front of it on this one.
            Typing::Forward => {
                let (end_line, end_column) = match cursor.column == text.line_length(cursor.line) {
                    true if cursor.line >= text.last_line() => return,
                    true => (cursor.line.saturating_add(1), CharColumn::new(0)),
                    false => (cursor.line, cursor.column.saturating_add(1)),
                };
                (
                    crate::coordinates::Span {
                        line: cursor.line,
                        column: cursor.column,
                        end_line,
                        end_column,
                    },
                    String::new(),
                    crate::buffer::undo::Doing::Deleting,
                )
            }
        };

        self.change(span, &with, doing);
    }

    /// Moves the lines the reader is on one step in or out.
    ///
    /// The lines a selection touches, whole, or the line the cursor is on
    /// when nothing is selected -- and one change rather than one per line,
    /// so that putting a block right takes one `ctrl+z` to put back.
    ///
    /// The selection comes out over the same lines, whole: a reader lining
    /// a block up presses this more than once, and a selection that went
    /// with the first press would make the second press about something
    /// else.
    pub(super) fn shift_indent(&mut self, deeper: bool) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let indent = buffer.indent();
        let text = buffer.text();
        let (first, last) = match buffer.selection() {
            Some(span) => (span.line, span.end_line),
            None => (buffer.cursor().line, buffer.cursor().line),
        };
        let mut lines = Vec::new();
        for line in first.get()..=last.get() {
            let line = LineNumber::new(line);
            let was = text.line(line).to_string();
            lines.push(match deeper {
                // Nothing to put in front of a line with nothing on it: an
                // indent on an empty line is trailing blanks, which is what
                // every other tool in the reader's way then takes back out.
                true if was.trim().is_empty() => was,
                true => indent.clone() + &was,
                false => outdented(&was, &indent),
            });
        }
        let span = crate::coordinates::Span {
            line: first,
            column: CharColumn::new(0),
            end_line: last,
            end_column: text.line_length(last),
        };
        let with = lines.join("\n");
        if with == text.text_in(span) {
            return;
        }
        self.change(span, &with, crate::buffer::undo::Doing::Whole);
        if let Some(buffer) = self.current_buffer_mut() {
            let end = buffer.text().line_length(last);
            buffer.select(crate::coordinates::Span {
                line: first,
                column: CharColumn::new(0),
                end_line: last,
                end_column: end,
            });
        }
    }

    /// Moves the lines the reader is on up or down by one.
    ///
    /// The lines a selection touches, whole, or the line the cursor is on.
    /// One change rather than two, so a line walked three rows down is
    /// three steps back rather than six -- and the lines go as they are,
    /// without being re-indented: a reader moving a line has a place in
    /// mind for it, and a line that changed shape on the way is a line they
    /// have to look at again.
    pub fn move_lines(&mut self, up: bool) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let text = buffer.text();
        let (first, last) = lines_in_hand(buffer);
        // Nowhere to go: the block is already against the end it is being
        // moved towards.
        let last_line = text.last_line();
        if (up && first.get() == 0) || (!up && last >= last_line) {
            return;
        }
        // The line it swaps with, and the two spans that make it one edit:
        // everything from the first line to the last, in the order the move
        // puts them.
        let (from, to) = match up {
            true => (first.saturating_sub(1), last),
            false => (first, last.saturating_add(1)),
        };
        let span = crate::coordinates::Span {
            line: from,
            column: CharColumn::new(0),
            end_line: to,
            end_column: text.line_length(to),
        };
        let lines: Vec<String> = (from.get()..=to.get())
            .map(|line| text.line(LineNumber::new(line)).to_string())
            .collect();
        let with = match up {
            true => lines[1..].join("\n") + "\n" + &lines[0],
            false => {
                let end = lines.len() - 1;
                lines[end].clone() + "\n" + &lines[..end].join("\n")
            }
        };
        let cursor = buffer.cursor();
        let selected = buffer.selection().is_some();
        self.change(span, &with, crate::buffer::undo::Doing::Whole);

        // The reader keeps hold of what they moved: the lines under the
        // selection, or the cursor on the line it was on.
        let moved = |line: LineNumber| match up {
            true => line.saturating_sub(1),
            false => line.saturating_add(1),
        };
        if let Some(buffer) = self.current_buffer_mut() {
            match selected {
                true => {
                    let end = moved(last);
                    buffer.select(crate::coordinates::Span {
                        line: moved(first),
                        column: CharColumn::new(0),
                        end_line: end,
                        end_column: buffer.text().line_length(end),
                    });
                }
                false => buffer.place_cursor(moved(cursor.line), cursor.column),
            }
        }
    }

    /// Comments the lines the reader is on out, or takes the comment off.
    ///
    /// Off where every line that has anything on it is already commented,
    /// and on otherwise: a block half commented is a block somebody was in
    /// the middle of commenting, and finishing it is what they meant.
    ///
    /// The token goes at the shallowest indentation of the lines it is
    /// about, so that the marks line up under each other rather than
    /// stepping in and out with the code. Blank lines are left alone --
    /// a comment on one is trailing blanks.
    pub fn toggle_comment(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let Some(token) = buffer
            .language()
            .and_then(|language| language.line_comment())
        else {
            self.note = Some("no line comment in this language".to_string());
            return;
        };
        let text = buffer.text();
        let (first, last) = lines_in_hand(buffer);
        let rows: Vec<String> = (first.get()..=last.get())
            .map(|line| text.line(LineNumber::new(line)).to_string())
            .collect();

        // Three passes' worth of questions, asked in one: whether every
        // line with anything on it is already commented, how far in the
        // shallowest of them starts, and whether every mark is followed by
        // a blank -- because where one is not, taking a blank off the rest
        // would eat a character somebody wrote.
        let mut commented = true;
        let mut margin = true;
        let mut indent = usize::MAX;
        for row in rows.iter().filter(|row| !row.trim().is_empty()) {
            let at = row
                .chars()
                .take_while(|character| character.is_whitespace())
                .count();
            indent = indent.min(at);
            let rest: String = row.chars().skip(at).collect();
            match rest.strip_prefix(token) {
                Some(after) => margin &= after.starts_with(' '),
                None => commented = false,
            }
        }
        if indent == usize::MAX {
            return;
        }

        let with: Vec<String> = rows
            .iter()
            .map(|row| {
                if row.trim().is_empty() {
                    return row.clone();
                }
                if !commented {
                    let (before, after): (String, String) = (
                        row.chars().take(indent).collect(),
                        row.chars().skip(indent).collect(),
                    );
                    return format!("{before}{token} {after}");
                }
                let at = row
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .count();
                let kept: String = row
                    .chars()
                    .skip(at + token.chars().count() + usize::from(margin))
                    .collect();
                row.chars().take(at).collect::<String>() + &kept
            })
            .collect();

        let span = crate::coordinates::Span {
            line: first,
            column: CharColumn::new(0),
            end_line: last,
            end_column: text.line_length(last),
        };
        let selected = buffer.selection().is_some();
        self.change(span, &with.join("\n"), crate::buffer::undo::Doing::Whole);
        if selected && let Some(buffer) = self.current_buffer_mut() {
            let end = buffer.text().line_length(last);
            buffer.select(crate::coordinates::Span {
                line: first,
                column: CharColumn::new(0),
                end_line: last,
                end_column: end,
            });
        }
    }

    /// Makes one change to the document being read, and tells everything
    /// that has to hear about it.
    pub(super) fn change(
        &mut self,
        span: crate::coordinates::Span,
        with: &str,
        doing: crate::buffer::undo::Doing,
    ) {
        let Some(index) = self.current.map(BufferId::get) else {
            return;
        };
        let before = self
            .buffers
            .get(index)
            .and_then(Option::as_ref)
            .map_or(0, |buffer| buffer.text().line_count());
        // Where the edit lands and how much it moves, in the one coordinate
        // a snippet's holes are kept in. Taken before the edit, because
        // afterwards the document it is measured against is gone.
        let moving = self
            .buffers
            .get(index)
            .and_then(Option::as_ref)
            .map(|buffer| {
                let text = buffer.text();
                let at = text.char_offset(span.line, span.column);
                let to = text.char_offset(span.end_line, span.end_column);
                (at, to.get().saturating_sub(at.get()), with.chars().count())
            });
        let changed = self
            .buffers
            .get_mut(index)
            .and_then(Option::as_mut)
            .is_some_and(|buffer| buffer.edit(span, with, doing));
        if changed {
            // The places the reader can go back to are line numbers in this
            // document, and the edit has just moved some of them.
            let after = self
                .buffers
                .get(index)
                .and_then(Option::as_ref)
                .map_or(0, |buffer| buffer.text().line_count());
            self.jumps.keep_across(
                BufferId::new(index),
                span.line,
                span.end_line,
                after as isize - before as isize,
            );
            // And the holes a snippet left, which are places in this
            // document too -- the reader is typing into one of them.
            if let Some((at, removed, inserted)) = moving {
                self.keep_filling_across(at, removed, inserted);
            }
            // The server's copy of this document is now a document nobody
            // has. Everything else keyed on the version notices by itself.
            self.change_document(index);
        }
    }
}

/// The lines a command about lines is about: the ones a selection touches,
/// or the one the cursor is on.
fn lines_in_hand(buffer: &crate::buffer::Buffer) -> (LineNumber, LineNumber) {
    match buffer.selection() {
        Some(span) => (span.line, span.end_line),
        None => (buffer.cursor().line, buffer.cursor().line),
    }
}

/// A line with one step of its indentation taken off.
///
/// A tab where the line begins with one, and otherwise up to a step's worth
/// of spaces -- fewer where there are fewer, because a line indented by two
/// spaces in a file of four should come out at the margin rather than stay
/// where it is.
fn outdented(line: &str, indent: &str) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        return rest.to_string();
    }
    let blanks = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    line.chars()
        .skip(blanks.min(indent.chars().count()))
        .collect()
}

/// The blank a new line starts with, following the line it came off.
///
/// The indentation of the line the reader was on, and one step more where
/// that line ended by opening something. Not from a grammar: tree-sitter has
/// queries for this and obelus ships none of them, and the line above is
/// what a reader would have copied by hand anyway. It is also the rule that
/// is right in a file obelus cannot parse at all, which is the case a
/// grammar cannot help with.
fn indent_after(text: &crate::text::Text, line: LineNumber, column: CharColumn) -> String {
    let characters: Vec<char> = text.line(line).chars().collect();
    let blank: String = characters
        .iter()
        .take_while(|character| character.is_whitespace())
        .collect();
    // What was in front of the cursor is what moves down, so what is behind
    // it is what decides the indent -- pressing return in the middle of a
    // line does not indent by what came after.
    let deeper = characters
        .iter()
        .take(column.get())
        .rev()
        .find(|character| !character.is_whitespace())
        .is_some_and(|character| crate::syntax::brackets::opens(*character));
    match deeper {
        true => blank + &" ".repeat(crate::text::tab_width()),
        false => blank,
    }
}

/// What a key puts in, where it puts anything.
///
/// Spaces for a tab, as many as the width tabs are laid out at -- so what
/// the key puts in looks the same as what a tab in the file already there
/// looks like.
fn put_in(typing: Typing) -> String {
    match typing {
        Typing::Character(character) => character.to_string(),
        Typing::Newline => "\n".to_string(),
        // Never asked for `Tab`: what one step of indentation is belongs to
        // the file, so [`App::typed`] takes it from the document rather
        // than from here.
        Typing::Tab
        | Typing::Outdent
        | Typing::Backward
        | Typing::Forward
        | Typing::BackwardWord
        | Typing::ForwardWord => String::new(),
    }
}
