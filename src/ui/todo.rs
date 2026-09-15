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
    ui::{Hint, editor::SCROLLBAR_WIDTH, fill, foot, footed, put, rule, text_width, write},
};

/// The rows that are not the list: the title and the rule under it.
const FURNITURE: u16 = 2;

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
    let area = footed(area, hints);
    Rect {
        y: area.y + FURNITURE,
        height: area.height.saturating_sub(FURNITURE),
        ..area
    }
}

/// What the keys do here.
///
/// In the order they survive a narrow screen, which is the order they are
/// worth: going to what a note is about is the whole point of a note that
/// points anywhere, and leaving is the one key every reader already knows.
#[must_use]
pub fn hints() -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = |code, modifiers| crate::keymap::KeyChord::new(code, modifiers);
    vec![
        Hint::new(chord(KeyCode::Enter, KeyModifiers::NONE), "go there"),
        Hint::new(chord(KeyCode::Char(' '), KeyModifiers::NONE), "done"),
        Hint::new(chord(KeyCode::Char('f'), KeyModifiers::ALT), "open"),
        Hint::new(chord(KeyCode::Char('e'), KeyModifiers::ALT), "write"),
        Hint::new(chord(KeyCode::Delete, KeyModifiers::NONE), "drop"),
        Hint::new(chord(KeyCode::Esc, KeyModifiers::NONE), "leave"),
    ]
}

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
        if area.height < FURNITURE {
            return;
        }

        let hints = hints();
        write(
            cells,
            area.x + 2,
            area.y,
            "todo",
            Style::new()
                .fg(self.theme.status_foreground)
                .bg(self.theme.background),
        );
        rule(
            cells,
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
            self.theme,
        );
        foot(cells, area, &hints, self.theme);

        let list = list_region(area, &hints);
        if list.height == 0 {
            return;
        }
        if self.notes.rows().is_empty() {
            crate::ui::nothing(cells, list, "nothing to come back to", self.theme);
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
    }
}

impl TodoUi<'_> {
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

        let mut x = area.x + 1;
        // The fold mark, in a column every row leaves for it so that what a
        // note says and what its body says start in the same place.
        if let Some(open) = row.open {
            put(cells, x, y, crate::ui::opens(open), style);
        }
        x += 1;
        // And the box, on the note's own row only: a line of a body is part
        // of the note above it and is not separately done.
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
