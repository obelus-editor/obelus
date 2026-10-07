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
use obelus_text::text_width;
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
        // Where the view has got to, every frame, so a window can slide
        // the screen by however far the program or the reader moved it --
        // the same thing a file and a conversation say. Whole width and no
        // bar, because a terminal has none.
        crate::shapes::scrolled(area, self.terminal.top(), None);
        let screen = self.terminal.screen();
        let (rows, columns) = screen.size();
        let held = self.terminal.held();
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
                let contents = as_given(contents, cell.is_wide());
                // As much room as is left on the row, so a wide character
                // in the last column is not drawn over the edge.
                let room = usize::from(area.width - column);
                let mut style = style_of(cell, self.theme);
                // What the reader has hold of, in the colour a selection is
                // everywhere else.
                if held.is_some_and(|(first, last)| (first..=last).contains(&(row, column))) {
                    style = style.bg(self.theme.selection_background);
                }
                cells.set_stringn(area.x + column, area.y + row, contents, room, style);
            }
        }
    }
}

/// A cell of a program's screen, as wide as the program was given it.
///
/// Which is the one thing about a cell that is not Obelus's to decide. A
/// character a window draws as a picture is two cells to Obelus and one to
/// the program that wrote it, and drawn at two it covers the next one -- so
/// it is asked for as text, which is one cell and is what the program
/// meant. Everything else is left as it came.
#[must_use]
pub fn as_given(contents: &str, wide: bool) -> std::borrow::Cow<'_, str> {
    match !wide && text_width(contents) > 1 {
        true => format!("{contents}\u{fe0e}").into(),
        false => contents.into(),
    }
}

/// How a terminal's program ended, in the words every place that says so
/// uses: the status row and the terminal's row in the list of what is open.
#[must_use]
pub fn how_it_ended(ended: &obelus_terminal::Ended) -> String {
    match (&ended.signal, ended.succeeded()) {
        (_, true) => "Ended".to_string(),
        (Some(signal), false) => format!("Stopped by {signal}"),
        (None, false) => format!("Exited {}", ended.code),
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
