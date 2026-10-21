//! A file shown as its reading, with a cursor in it.
//!
//! **A reading is a page like any other.** It used to be rows scrolled by
//! the arrows with nowhere for a cursor to be, on the argument that its rows
//! are not the file's lines -- which is true, and is also true of a
//! transcript and of a pull request, and neither of those went without one.
//! What a reader does with a README on screen is read it and take a copy of
//! a sentence of it, and with no cursor there was nothing to take hold of
//! the sentence with. So the cursor is [`crate::page`]'s: the arrows walk the
//! words, shift holds them, a drag of the pointer holds them, and a copy
//! takes what is held.
//!
//! **The rows are not this module's.** A reading is laid out by the
//! application once per change of text, width or file, and kept there; this
//! is where the cursor is and what is held, and nothing else. The rows are
//! lent a hold for as long as somebody asks ([`Laid`]) rather than copied to
//! be marked, which is what a page that owns its rows can afford and a
//! README of a few thousand rows, asked on every key, should not pay.
//!
//! A log is a reading too, and walks the same way: its rows carry nothing
//! about where in the file they came from, so a log's cursor walks and holds
//! nothing. Markdown's rows say where every word came from.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_row::{Row, Span};

use crate::{
    page::{Hold, Place, Room, Spot, Walked},
    window::Window,
};

/// One row of a reading, with which of its characters are held.
#[derive(Clone, Debug)]
pub struct Laid<'a> {
    /// The row, as the reading laid it out.
    pub row: &'a Row,
    held: Option<Range<usize>>,
}

impl<'a> Laid<'a> {
    /// A row with nothing of it held.
    #[must_use]
    pub const fn of(row: &'a Row) -> Self {
        Self { row, held: None }
    }

    /// What the row says, with nothing about how it is drawn.
    #[must_use]
    pub fn text(&self) -> String {
        crate::page::text(self)
    }
}

impl crate::page::Line for Laid<'_> {
    /// A reading is laid out from one text, the file.
    type Text = ();

    fn spans(&self) -> &[Span] {
        &self.row.spans
    }

    /// Every row, the layout's own blanks and borders too: which runs of
    /// a row came from the file is said run by run, and a row with none
    /// has no place in it for a selection to start whichever way this
    /// answers.
    fn laid_from(&self) -> Option<()> {
        Some(())
    }

    fn indent(&self) -> u16 {
        0
    }

    /// Nothing: enter does nothing on a reading.
    fn acting(_: &[Self], _: usize) -> Option<Range<usize>> {
        None
    }

    fn held(&self) -> Option<&Range<usize>> {
        self.held.as_ref()
    }

    fn hold(&mut self, held: Range<usize>) {
        self.held = Some(held);
    }
}

/// What a key did on a reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It was not the reading's.
    Ignored,
    /// It was, and the cursor and the view are where it left them.
    Consumed,
}

/// Where the cursor is on a reading, and what the reader has hold of.
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    place: Place,
    held: Hold<()>,
}

impl Cursor {
    /// A cursor on the first row of what is on screen, `top` rows in: a
    /// reading taken up again somewhere down its length, where a cursor at
    /// its head would drag the view back there on the first key.
    #[must_use]
    pub fn at(top: usize) -> Self {
        Self {
            place: Place {
                row: top,
                character: 0,
            },
            held: Hold::default(),
        }
    }

    /// Where the cursor is.
    #[must_use]
    pub const fn place(&self) -> Place {
        self.place
    }

    /// Puts the cursor somewhere, for a press there.
    pub const fn stand(&mut self, at: Place) {
        self.place = at;
    }

    /// The rows, with what is held marked on them.
    #[must_use]
    pub fn laid<'a>(&self, rows: &'a [Row]) -> Vec<Laid<'a>> {
        let mut laid: Vec<Laid<'a>> = rows.iter().map(Laid::of).collect();
        self.held.mark(&mut laid);
        laid
    }

    /// Keeps the cursor on one of the rows, which a narrower window or a
    /// file shortened under it can leave it past the end of.
    pub fn settle(&mut self, rows: &[Row]) {
        let last = rows.len().saturating_sub(1);
        if self.place.row > last {
            self.place = Place {
                row: last,
                character: rows
                    .get(last)
                    .map_or(0, |row| crate::page::characters(&Laid::of(row))),
            };
        }
    }

    /// Takes hold of a place, letting go of anything else.
    pub fn hold_from(&mut self, spot: Spot<()>) {
        self.held.from(spot);
    }

    /// Drags the far end of what is held.
    pub fn hold_to(&mut self, spot: Spot<()>) {
        self.held.to(spot);
    }

    /// Lets go of whatever was held.
    pub fn let_go(&mut self) {
        self.held.let_go();
    }

    /// Lets go of a hold with nothing in it, which a press that never
    /// became a drag leaves.
    pub fn let_go_of_nothing(&mut self) {
        self.held.let_go_of_nothing();
    }

    /// Whether anything is held.
    #[must_use]
    pub const fn holding(&self) -> bool {
        self.held.holding()
    }

    /// What is held, as it is drawn.
    #[must_use]
    pub fn held_text(&self, rows: &[Row]) -> Option<String> {
        if !self.held.holding() {
            return None;
        }
        crate::page::held_text(&self.laid(rows))
    }

    /// Handles a key, on a reading `room` big whose view starts `top` rows
    /// in -- and moves `top` where the cursor has to be seen.
    ///
    /// The cursor's keys and nothing else: the arrows, the ends and the
    /// paging keys, with shift holding what they pass over and control
    /// taking the ends to the ends of the whole reading -- and escape,
    /// which lets go of what is held before it means anything else.
    /// Control and an arrow are not here: those move the view and leave
    /// the cursor, as they do in the file (`keys::editor_scrolling`).
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        rows: &[Row],
        room: Room,
        top: &mut usize,
    ) -> Outcome {
        let Some(modifiers) = obelus_keymap::modifiers_of(key) else {
            return Outcome::Ignored;
        };
        let bare = modifiers == KeyModifiers::NONE;
        let ends = matches!(key.code, KeyCode::Home | KeyCode::End);
        if bare || modifiers == KeyModifiers::SHIFT || (ends && modifiers == KeyModifiers::CONTROL)
        {
            let laid: Vec<Laid<'_>> = rows.iter().map(Laid::of).collect();
            // The view is the buffer's, which is the one answer to where a
            // reading is scrolled to that the bar and the wheel read: lent
            // to the walk as a window, and what it made of it handed back.
            let mut window = Window::new();
            window.set_count(rows.len());
            window.drag_to(*top);
            if let Some(walked) = crate::page::walk(
                self.place,
                key.code,
                modifiers,
                &laid,
                room,
                &mut self.held,
                &mut window,
            ) {
                if let Walked::To { place, .. } = walked {
                    self.place = place;
                    // `control+end` sends the window to the end without
                    // moving its top, which is the window's settling to do
                    // and nothing settles this one: the cursor is put on
                    // screen here, the way every other key's is.
                    crate::page::show(&mut window, place.row, rows.len(), room.height);
                }
                *top = window.top();
                return Outcome::Consumed;
            }
        }
        match key.code {
            KeyCode::Esc if bare && self.held.holding() => {
                self.held.let_go();
                Outcome::Consumed
            }
            _ => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Room = Room {
        height: 3,
        reading: 40,
    };

    fn press(cursor: &mut Cursor, code: KeyCode, modifiers: KeyModifiers, rows: &[Row]) -> usize {
        let mut top = 0;
        cursor.handle_key(&KeyEvent::new(code, modifiers), rows, ROOM, &mut top);
        top
    }

    /// Shift and the arrows hold the words of a markdown file's reading,
    /// and what is held is what is drawn: the bullet in front of a list's
    /// item comes with it, and the hashes in front of the heading do not.
    ///
    /// Broken deliberately by not marking what is held on the rows in
    /// `Cursor::laid`: nothing is held to copy.
    #[test]
    fn shift_and_the_arrows_hold_what_is_drawn() {
        let rows = obelus_markdown::render("# Title\n\n- one\n- two\n", 40);
        let mut cursor = Cursor::default();
        press(&mut cursor, KeyCode::Down, KeyModifiers::SHIFT, &rows);
        press(&mut cursor, KeyCode::Down, KeyModifiers::SHIFT, &rows);
        press(&mut cursor, KeyCode::End, KeyModifiers::SHIFT, &rows);
        let held = cursor.held_text(&rows).expect("something is held");
        assert!(held.starts_with("Title"), "{held:?}");
        assert!(!held.contains('#'), "the hashes came with it: {held:?}");
        assert!(held.contains("one"), "{held:?}");

        // A bare arrow lets go, the way it does in the file.
        press(&mut cursor, KeyCode::Down, KeyModifiers::NONE, &rows);
        assert!(!cursor.holding(), "a bare arrow kept hold");
    }

    /// The view follows the cursor down, and `control+end` takes both to
    /// the end of the reading.
    ///
    /// Broken deliberately by leaving out the `show` after the walk:
    /// `control+end` puts the cursor on the last row with the view still at
    /// the top.
    #[test]
    fn the_view_follows_the_cursor() {
        let source: String = (1..=10).map(|n| format!("Paragraph {n}.\n\n")).collect();
        let rows = obelus_markdown::render(&source, 40);
        let mut cursor = Cursor::default();
        let top = press(&mut cursor, KeyCode::End, KeyModifiers::CONTROL, &rows);
        assert_eq!(cursor.place().row, rows.len() - 1);
        assert_eq!(
            top,
            rows.len() - usize::from(ROOM.height),
            "the view stayed"
        );
    }

    /// Escape lets go of what is held, and is not the reading's when
    /// nothing is.
    #[test]
    fn escape_lets_go_first() {
        let rows = obelus_markdown::render("Some words here.\n", 40);
        let mut cursor = Cursor::default();
        press(&mut cursor, KeyCode::End, KeyModifiers::SHIFT, &rows);
        assert!(cursor.holding());
        let mut top = 0;
        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            cursor.handle_key(&escape, &rows, ROOM, &mut top),
            Outcome::Consumed
        );
        assert!(!cursor.holding());
        assert_eq!(
            cursor.handle_key(&escape, &rows, ROOM, &mut top),
            Outcome::Ignored
        );
    }
}
