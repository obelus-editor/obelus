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
    /// Take out what is behind the cursor.
    Backward,
    /// Take out what is in front of it.
    Forward,
}

/// What a key types, if it types anything.
///
/// Shift is allowed through: it is how a capital arrives, and the character
/// crossterm reports already has it applied. Every other modifier is
/// somebody else's -- a `ctrl` chord is a command, and typing one would put
/// a character in where the reader asked for an action.
pub(super) fn typing_for(key: &KeyEvent) -> Option<Typing> {
    let modifiers = keymap::modifiers_of(key)?;
    let bare = modifiers == KeyModifiers::NONE;
    let shifted = modifiers == KeyModifiers::SHIFT;
    if !bare && !shifted {
        return None;
    }
    match key.code {
        KeyCode::Char(character) => Some(Typing::Character(character)),
        KeyCode::Enter if bare => Some(Typing::Newline),
        KeyCode::Tab if bare => Some(Typing::Tab),
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
                (at, put_in(typing), doing)
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
        let changed = self
            .buffers
            .get_mut(index)
            .and_then(Option::as_mut)
            .is_some_and(|buffer| buffer.edit(span, with, doing));
        if changed {
            // The server's copy of this document is now a document nobody
            // has. Everything else keyed on the version notices by itself.
            self.change_document(index);
        }
    }
}

/// What a key puts in, where it puts anything.
///
/// Spaces for a tab, as many as the width obelus lays a tab out at. Which
/// the reader cannot yet choose -- a number is not a shape the settings page
/// has -- so it is the one the rest of the program already uses.
fn put_in(typing: Typing) -> String {
    match typing {
        Typing::Character(character) => character.to_string(),
        Typing::Newline => "\n".to_string(),
        Typing::Tab => " ".repeat(crate::text::TAB_WIDTH),
        Typing::Backward | Typing::Forward => String::new(),
    }
}
