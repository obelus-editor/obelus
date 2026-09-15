//! What a reader means to come back to, drawn.
//!
//! A title, a rule, the notes, and a foot saying what the keys do. The notes
//! are the only thing here with more than one shape: a row is a box, what it
//! says, and where it points, and a note with more behind it turns the mark
//! every other folding thing in obelus turns.
//!
//! Where a note points is right-aligned in a column of its own, the way a
//! changed file's counts are: a place hung off the ragged end of a sentence
//! is a place a reader has to find again on every row.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::todo::{Row, TodoView as Notes},
    theme::Theme,
    ui::{Hint, editor::SCROLLBAR_WIDTH, fill, foot, footed, put, text_width, write},
};

/// The box in front of a note, ticked and not.
const OPEN: char = '\u{25a1}';
const DONE: char = '\u{2611}';

/// Where the notes go, inside a region this size.
///
/// One answer, asked by the drawing and by the keys that move about the
/// list: a page is worth what is on screen, and two answers to how much that
/// is would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn list_region(area: Rect, hints: &[Hint]) -> Rect {
    // Nothing above it. There are no tabs here and nothing to filter by, so
    // a title row would be one row of the reader's screen spent saying what
    // they just asked for.
    footed(area, hints)
}

/// What the keys do here, and which of them do anything at the moment.
///
/// One list, read two ways: the foot draws the common ones that can be
/// pressed, and the card draws all of them with the rest greyed. A view that
/// kept two lists would be a view whose card and foot could disagree about
/// what it answers to.
#[must_use]
pub fn hints(notes: &Notes) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = crate::keymap::KeyChord::new;
    let bare = |code| chord(code, KeyModifiers::NONE);
    let alt = |code| chord(code, KeyModifiers::ALT);
    let on = notes.selected_note();
    vec![
        // Nothing about typing, the arrows or `shift+enter`: this is a page
        // being written, and what a page being written does with a letter is
        // not news. What is worth a row is what it does with a *note*.
        Hint::common(bare(KeyCode::Enter), "another"),
        Hint::common(alt(KeyCode::Char(' ')), "done").when(on.is_some()),
        Hint::common(alt(KeyCode::Enter), "go there").when(notes.can_go()),
        Hint::common(bare(KeyCode::Esc), "leave"),
        Hint::rare(alt(KeyCode::Up), "move it up or down")
            .or(alt(KeyCode::Down))
            .when(notes.rows().len() > 1),
        Hint::rare(alt(KeyCode::Backspace), "take the whole note away").when(on.is_some()),
    ]
}

/// Where the terminal should put its caret: in the note being written.
///
/// Worked out from the same rows the drawing lays out, so the caret is in
/// the row the reader can see their typing in rather than a row the view
/// happens to agree about.
#[must_use]
pub fn caret(area: Rect, notes: &Notes) -> Option<ratatui::layout::Position> {
    let composer = notes.writing()?;
    let at = notes.writing_at()?;
    let hints = hints(notes);
    let list = list_region(area, &hints);
    let window = notes.window();
    let row = at.checked_sub(window.top())?;
    let (line, cell) = composer.caret(list.width.saturating_sub(MARGIN));
    let y = list.y + u16::try_from(row + line).ok()?;
    (y < list.bottom()).then(|| ratatui::layout::Position {
        x: (list.x + MARGIN + cell.get()).min(list.right().saturating_sub(1)),
        y,
    })
}

/// How far in a note's own text starts: the fold column, the box, and the
/// blank after it.
const MARGIN: u16 = 4;

/// The notes, over the whole editor region.
pub struct TodoUi<'a> {
    notes: &'a Notes,
    theme: &'a Theme,
}

impl<'a> TodoUi<'a> {
    /// Borrows what the view needs, or nothing if the notes are not open.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            notes: app.notes()?,
            theme: app.theme(),
        })
    }
}

impl Widget for TodoUi<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
        let hints = hints(self.notes);
        foot(cells, area, &hints, self.theme);

        let list = list_region(area, &hints);
        if list.height == 0 {
            return;
        }
        if self.notes.rows().is_empty() {
            crate::ui::nothing(cells, list, "nothing to come back to", self.theme);
            self.keys(cells, area, &hints);
            return;
        }

        let window = self.notes.window();
        for (at, row) in self
            .notes
            .rows()
            .iter()
            .enumerate()
            .skip(window.top())
            .take(usize::from(list.height))
        {
            let Ok(offset) = u16::try_from(at - window.top()) else {
                break;
            };
            self.row(
                cells,
                Rect {
                    y: list.y + offset,
                    height: 1,
                    ..list
                },
                row,
                at == window.focus(),
            );
        }

        // Only where there is somewhere to scroll: a track with no thumb on
        // it is a control that does not work.
        if window.scrollable(list.height) {
            crate::ui::scrollbar(
                cells,
                list,
                window.top(),
                self.notes.rows().len(),
                self.theme,
            );
        }

        self.keys(cells, area, &hints);
    }
}

impl TodoUi<'_> {
    /// Every key this view answers to, where the reader asked for them.
    ///
    /// Over everything, because it is what they asked for and the list is
    /// what they asked about.
    fn keys(&self, cells: &mut CellBuffer, area: Rect, hints: &[Hint]) {
        if self.notes.showing_keys() {
            // Above the foot: the foot says how to close this, and a card
            // that covered it would be a card with no way out on screen.
            crate::ui::keys_card(cells, footed(area, hints), hints, self.theme);
        }
    }

    /// One row: the mark, the box, what it says, and where it points.
    fn row(&self, cells: &mut CellBuffer, area: Rect, row: &Row, selected: bool) {
        // One mark for "the keys are here", and it says nothing else.
        let background = match selected {
            true => self.theme.selected_row_background,
            false => self.theme.background,
        };
        fill(
            cells,
            Rect {
                width: area.width.saturating_sub(SCROLLBAR_WIDTH),
                ..area
            },
            Style::new().bg(background),
        );
        // A note that is done is said in the ink, never by taking it away: a
        // list of what is done is how a reader tells "I decided against it"
        // from "I never got to it".
        let ink = match row.done {
            true => self.theme.gutter,
            false => self.theme.foreground,
        };
        let style = Style::new().fg(ink).bg(background);
        let dim = Style::new().fg(self.theme.gutter).bg(background);
        let y = area.y;

        // The box, on the note's own row only: a line of a body is part of
        // the note above it and is not separately done. Its column is kept
        // on the rows below, so a note's lines line up under its first.
        let mut x = area.x + 1;
        if row.head {
            put(cells, x, y, if row.done { DONE } else { OPEN }, style);
        }
        x += 2;

        let at = row.at.as_deref().unwrap_or_default();
        let reserved = match at.is_empty() {
            true => 0,
            false => u16::try_from(text_width(at) + 2).unwrap_or(u16::MAX),
        };
        let edge = area
            .x
            .saturating_add(area.width)
            .saturating_sub(SCROLLBAR_WIDTH + reserved);
        write(
            cells,
            x,
            y,
            &crate::ui::truncate_from_right(&row.said, usize::from(edge.saturating_sub(x))),
            style,
        );

        if !at.is_empty()
            && let Some(offset) = area
                .width
                .checked_sub(SCROLLBAR_WIDTH + reserved.saturating_sub(1))
        {
            write(cells, area.x + offset, y, at, dim);
        }
    }
}
