//! The list a setting's names are built in.
//!
//! Two sections of one list, drawn the way every compact list here is
//! drawn: a band along the bottom of the editor region, a rule above it,
//! the query on the status row. What is different is the boundary in the
//! middle of it, which says which half is the reader's own answer and which
//! half is what this machine has to offer.
//!
//! Nothing about fonts, the same as the component it draws: it is handed
//! rows with names on them.

use obelus_component::names::{Names, Row};
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    FOOT_ROWS, Hint, Marked, Matched, editor::SCROLLBAR_WIDTH, fill, foot, footed, rule, scrollbar,
    write, write_marked,
};

/// How many rows of the list a reader gets to walk.
///
/// The same ten a compact picker gives, and fixed for the same reason: a
/// boundary that moved as the query narrowed the list would slide the whole
/// band up and down under a reader who is reading it.
const LIST_ROWS: u16 = 10;

/// What the lower half is.
///
/// On the rule rather than in a row of its own: a boundary that says what
/// is under it is still one boundary.
const OFFERED: &str = " On this machine ";

/// What a name that is in the list but not on this machine says about
/// itself.
///
/// Said rather than hidden, and not a refusal: one settings file is read on
/// every machine the reader uses, so a name that means nothing here means
/// something there -- and a list that quietly dropped it would be a list
/// that changed their answer for them.
const ELSEWHERE: &str = "Not on this machine";

/// What the keys do here, and which of them do anything now.
#[must_use]
pub fn hints(names: &Names) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = obelus_editing::keymap::KeyChord::new;
    let bare = |code| chord(code, KeyModifiers::NONE);
    let on = names.rows().get(names.window().focus());
    vec![
        Hint::common(
            bare(KeyCode::Enter),
            match on.is_some_and(Row::chosen) {
                true => "Take out",
                false => "Add",
            },
        )
        .saying(match on.is_some_and(Row::chosen) {
            true => "Take this name out of the list",
            false => "Put this name at the end of the list",
        })
        .when(on.is_some_and(Row::stands)),
        // One act in two directions, which is why it is one hint: the
        // notes move a row with the same chord.
        Hint::common(chord(KeyCode::Up, KeyModifiers::ALT), "Move")
            .or(chord(KeyCode::Down, KeyModifiers::ALT))
            .saying("Move this name up or down the order")
            .when(on.is_some_and(Row::chosen)),
        Hint::rare(bare(KeyCode::Esc), "Done").saying("Leave the list as it is now"),
    ]
}

/// How many rows of list the band has room for.
#[must_use]
pub fn rows_drawn(names: &Names, editor: Rect) -> u16 {
    let region = region(names, editor);
    footed(region, &hints(names)).height
}

/// Where the band sits: along the bottom of the editor region, as tall as
/// it needs and no taller.
#[must_use]
pub fn region(names: &Names, editor: Rect) -> Rect {
    let rows = u16::try_from(names.rows().len()).unwrap_or(u16::MAX);
    let wanted = rows
        .min(LIST_ROWS.saturating_add(u16::try_from(names.taken()).unwrap_or(0)))
        .saturating_add(FOOT_ROWS)
        .clamp(1, editor.height);
    Rect {
        y: editor.y + editor.height - wanted,
        height: wanted,
        ..editor
    }
}

/// The list, its boundary and its foot.
pub struct NamesView<'a> {
    names: &'a Names,
    theme: &'a Theme,
}

impl<'a> NamesView<'a> {
    /// Borrows what the view needs.
    #[must_use]
    pub const fn new(names: &'a Names, theme: &'a Theme) -> Self {
        Self { names, theme }
    }
}

impl Widget for NamesView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let page = Style::new()
            .bg(self.theme.background)
            .fg(self.theme.foreground);
        fill(cells, area, page);

        let hints = hints(self.names);
        let list = footed(area, &hints);
        let window = self.names.window();
        let rows = self.names.rows();
        let visible = window.visible(list.height);
        let scrolls = window.scrollable(list.height);
        let room = Rect {
            width: list.width.saturating_sub(match scrolls {
                true => SCROLLBAR_WIDTH,
                false => 0,
            }),
            ..list
        };

        for (at, index) in visible.clone().enumerate() {
            let Some(row) = rows.get(index) else {
                break;
            };
            let y = list.y + u16::try_from(at).unwrap_or(u16::MAX);
            let here = index == window.focus() && row.stands();
            self.draw_row(cells, room, y, row, index, here);
        }

        if scrolls {
            scrollbar(cells, list, visible.start, rows.len(), self.theme);
        }
        foot(cells, area, &hints, self.theme);
    }
}

impl NamesView<'_> {
    /// One row, whichever of the four it is.
    fn draw_row(
        &self,
        cells: &mut CellBuffer,
        room: Rect,
        y: u16,
        row: &Row,
        index: usize,
        here: bool,
    ) {
        let page = Style::new()
            .bg(self.theme.background)
            .fg(self.theme.foreground);
        let style = match here {
            // The one mark for "the keys are here", the same one every
            // list in Obelus puts behind the row the reader is on.
            true => page.bg(self.theme.selection_background),
            false => page,
        };
        match row {
            Row::Boundary => {
                rule(
                    cells,
                    Rect {
                        y,
                        height: 1,
                        ..room
                    },
                    self.theme,
                );
                write(
                    cells,
                    room.x + 3,
                    y,
                    OFFERED,
                    Style::new().fg(self.theme.gutter).bg(self.theme.background),
                );
            }
            Row::Empty => {
                // Named where the machine said what it calls monospaced: a
                // reader deciding whether to choose anything is deciding
                // against something, and a name is what that something is.
                // The sentence keeps its capital and the name keeps its own
                // spelling.
                let said = match self.names.otherwise() {
                    Some(face) => format!("Nothing chosen, so {face}"),
                    None => "Nothing chosen, so the machine's own monospaced face".to_string(),
                };
                crate::nothing(
                    cells,
                    Rect {
                        y,
                        height: 1,
                        ..room
                    },
                    &said,
                    self.theme,
                );
            }
            Row::Chosen {
                at,
                name,
                here: has,
            } => {
                fill(
                    cells,
                    Rect {
                        y,
                        height: 1,
                        ..room
                    },
                    style,
                );
                let number = write(
                    cells,
                    room.x + 1,
                    y,
                    &format!("{at}"),
                    style.fg(self.theme.gutter),
                );
                write(cells, number + 2, y, name, style);
                self.say_elsewhere(cells, room, y, *has, style);
            }
            Row::Offer { name, here: has } => {
                fill(
                    cells,
                    Rect {
                        y,
                        height: 1,
                        ..room
                    },
                    style,
                );
                write_marked(
                    cells,
                    room,
                    room.x + 1,
                    y,
                    name,
                    style,
                    &Marked {
                        matched: Matched::Indices(self.names.marks_at(index)),
                        mark: self.theme.picker_match_background,
                        syntax: None,
                        skip: 0,
                    },
                );
                self.say_elsewhere(cells, room, y, *has, style);
            }
        }
    }

    /// The words at the right-hand end of a name this machine does not
    /// have.
    fn say_elsewhere(&self, cells: &mut CellBuffer, room: Rect, y: u16, here: bool, style: Style) {
        if here {
            return;
        }
        let width = u16::try_from(obelus_text::text_width(ELSEWHERE)).unwrap_or(0);
        let x = room.right().saturating_sub(width + 1);
        write(cells, x, y, ELSEWHERE, style.fg(self.theme.gutter));
    }
}
