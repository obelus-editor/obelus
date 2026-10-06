//! A terminal of Obelus's own: what its program drew, cell for cell.
//!
//! Nothing here is laid out. The program said what goes in every cell of a
//! screen the size it was told, and the screen is drawn the way it said.
//! What Obelus decides is what its colours are: the default is the page's
//! ground and ink, and the sixteen a program names by number are the
//! theme's (`Theme::terminal_colour`) -- a program that prints red means
//! the red the rest of the screen is using. An exact colour goes through as
//! it was named.

use obelus_terminal::{Terminal, vt100};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::{Screen, fill};

/// The terminal being read.
pub struct TerminalView<'a> {
    terminal: &'a Terminal,
    theme: &'a Theme,
}

impl<'a> TerminalView<'a> {
    /// Borrows what the view needs, or nothing if no terminal is being read.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            terminal: app.terminal()?,
            theme: app.theme(),
        })
    }
}

impl Widget for TerminalView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let page = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        fill(cells, area, page);
        let screen = self.terminal.screen();
        let (rows, columns) = screen.size();
        for row in 0..rows.min(area.height) {
            for column in 0..columns.min(area.width) {
                let Some(cell) = screen.cell(row, column) else {
                    continue;
                };
                // The right half of a wide character, which the left half
                // already covers.
                if cell.is_wide_continuation() {
                    continue;
                }
                let contents = match cell.contents() {
                    "" => " ",
                    said => said,
                };
                // As much room as is left on the row, so a wide character
                // in the last column is not drawn over the edge.
                let room = usize::from(area.width - column);
                cells.set_stringn(
                    area.x + column,
                    area.y + row,
                    contents,
                    room,
                    style_of(cell, self.theme),
                );
            }
        }
    }
}

/// Where the program's cursor is, if it is showing one and the view is on
/// the screen it is on.
#[must_use]
pub fn caret(area: Rect, terminal: &Terminal) -> Option<Position> {
    let screen = terminal.screen();
    // Read back up the screen, the cursor is below what is shown; and a
    // program that has ended has no cursor to type at.
    if screen.hide_cursor() || terminal.scrolled() > 0 || terminal.ended().is_some() {
        return None;
    }
    let (row, column) = screen.cursor_position();
    (row < area.height && column < area.width).then(|| Position::new(area.x + column, area.y + row))
}

/// How one cell is drawn.
fn style_of(cell: &vt100::Cell, theme: &Theme) -> Style {
    let mut ink = colour_of(cell.fgcolor(), theme).unwrap_or(theme.foreground);
    let mut ground = colour_of(cell.bgcolor(), theme).unwrap_or(theme.background);
    if cell.inverse() {
        std::mem::swap(&mut ink, &mut ground);
    }
    let mut style = Style::new().fg(ink).bg(ground);
    for (on, modifier) in [
        (cell.bold(), Modifier::BOLD),
        (cell.dim(), Modifier::DIM),
        (cell.italic(), Modifier::ITALIC),
        (cell.underline(), Modifier::UNDERLINED),
    ] {
        if on {
            style = style.add_modifier(modifier);
        }
    }
    style
}

/// A colour as the program named it, or nothing for the default.
///
/// The sixteen by number are the theme's: left as numbers, `ob` drew them in
/// the palette of the terminal it was in and `obg` in a palette of its own,
/// and neither was the page around them.
const fn colour_of(colour: vt100::Color, theme: &Theme) -> Option<Color> {
    match colour {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => match theme.terminal_colour(index) {
            Some(ours) => Some(ours),
            None => Some(Color::Indexed(index)),
        },
        vt100::Color::Rgb(red, green, blue) => Some(Color::Rgb(red, green, blue)),
    }
}
