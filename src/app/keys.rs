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
