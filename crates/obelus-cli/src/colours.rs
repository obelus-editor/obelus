//! How many colours this terminal draws in, and the frame brought down to
//! them on its way to the terminal.
//!
//! **What the terminal says, the way everybody asks it.** `COLORTERM` first,
//! which every terminal that draws RGB sets; then the terminal's entry in
//! terminfo, whose `RGB` or `Tc` say the same for a terminal that does not
//! -- tmux's way of being told. Neither is 256 colours: what Helix and bat
//! do, and what macOS's own terminal before macOS 26 needed, which sets
//! neither and drew every RGB colour as one dark red. What it costs is a
//! terminal that draws RGB but says nothing -- a `COLORTERM` that did not
//! cross an `ssh`, say -- and the `colours` setting is for that reader.
//!
//! Windows says nothing either way and draws RGB in both its terminals, so
//! it is not asked.
//!
//! **Brought down where the frame leaves**, rather than where a view picks
//! a colour: every colour a cell carries comes through here, a theme's and
//! a program's in the terminal alike, and none of what picks them has to
//! know. (A picture's own escapes are not a cell's colours and go as they
//! are, to the terminals that draw pictures -- all of which draw RGB.)
//!
//! **A change of depth is the whole screen again.** What ratatui hands a
//! backend is what changed since the last frame, worked out on the colours
//! before they are brought down -- so a reader changing the setting changes
//! no cell, and everything not otherwise redrawn would stay in the colours
//! it was drawn in, which is the very screen they changed it to be rid of.
//! So what was handed over is kept, and drawn again when the depth moves.
//! The setting is read at every frame, because it can change under a frame
//! -- another Obelus writing the settings, or one on another machine saying
//! so.

use obelus_theme::depth::{self, Depth};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Buffer, Cell},
    layout::{Position, Rect, Size},
};

/// How many colours this terminal says it draws.
pub(crate) fn of_this_terminal() -> Depth {
    if cfg!(windows) {
        return Depth::Millions;
    }
    said(std::env::var("COLORTERM").ok().as_deref(), || {
        terminfo::Database::from_env()
            .is_ok_and(|entry| entry.raw("RGB").is_some() || entry.raw("Tc").is_some())
    })
}

/// What `COLORTERM` says, or terminfo where it says nothing -- asked only
/// then, because it is a file to find and read.
fn said(colorterm: Option<&str>, in_terminfo: impl FnOnce() -> bool) -> Depth {
    if matches!(colorterm, Some("truecolor" | "24bit")) || in_terminfo() {
        Depth::Millions
    } else {
        Depth::Palette
    }
}

/// A terminal's backend, drawing at the depth the reader asked for, or the
/// one the terminal said.
#[derive(Debug)]
pub(crate) struct Coloured<B> {
    /// What draws.
    inner: B,
    /// What the terminal said.
    found: Depth,
    /// What the last frame was drawn at.
    drawn_at: Option<Depth>,
    /// Every cell handed over since the terminal was last cleared, as the
    /// frame said it.
    screen: Buffer,
}

impl<B> Coloured<B> {
    /// `inner`, on a terminal that said `found`.
    pub(crate) fn new(inner: B, found: Depth) -> Self {
        Self {
            inner,
            found,
            drawn_at: None,
            screen: Buffer::empty(Rect::ZERO),
        }
    }

    /// Keeps a cell handed over, growing what is kept to take it -- a
    /// frame never says how big it is, only where its cells are.
    fn keep(&mut self, x: u16, y: u16, cell: &Cell) {
        let area = self.screen.area;
        if x >= area.width || y >= area.height {
            self.screen.resize(Rect::new(
                0,
                0,
                area.width.max(x.saturating_add(1)),
                area.height.max(y.saturating_add(1)),
            ));
        }
        self.screen[(x, y)] = cell.clone();
    }
}

/// `cell`, as it is drawn at `depth`.
fn brought(depth: Depth, cell: &Cell) -> Cell {
    let mut cell = cell.clone();
    cell.fg = depth.of(cell.fg);
    cell.bg = depth.of(cell.bg);
    cell.underline_color = depth.of(cell.underline_color);
    cell
}

impl<B: Backend> Backend for Coloured<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let depth = depth::asked().unwrap_or(self.found);
        // Copies, because a frame lends its cells: it is only what changed
        // since the last frame, so a few rows rather than the screen.
        let mut handed: Vec<(u16, u16, Cell)> = Vec::new();
        for (x, y, cell) in content {
            self.keep(x, y, cell);
            handed.push((x, y, brought(depth, cell)));
        }
        let moved = self.drawn_at.is_some_and(|was| was != depth);
        self.drawn_at = Some(depth);
        if moved {
            // Everything kept, the way ratatui draws a screen from nothing:
            // the diff from an empty one, which knows a wide character
            // covers the cell after it.
            let nothing = Buffer::empty(self.screen.area);
            let again: Vec<(u16, u16, Cell)> = nothing
                .diff(&self.screen)
                .into_iter()
                .map(|(x, y, cell)| (x, y, brought(depth, cell)))
                .collect();
            return self
                .inner
                .draw(again.iter().map(|(x, y, cell)| (*x, *y, cell)));
        }
        self.inner
            .draw(handed.iter().map(|(x, y, cell)| (*x, *y, cell)))
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.screen.reset();
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        // Every kind forgets all of it: a cell kept after the terminal has
        // wiped it would be put back by the next change of depth. Only a
        // whole clear reaches here from a screen Obelus fills.
        self.screen.reset();
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use ratatui::{backend::TestBackend, style::Color};

    use super::*;

    /// Held by every test that says what the reader asked for, which is
    /// one thing for the whole binary.
    pub(crate) static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `COLORTERM`'s two words are RGB, and terminfo is asked only where it
    /// says neither -- and nothing said is 256 colours.
    ///
    /// Deliberate break: `in_terminfo()` asked first, unconditionally. The
    /// terminal that said `truecolor` asked terminfo as well.
    #[test]
    fn what_the_terminal_says() {
        let never = || panic!("terminfo was asked after COLORTERM had answered");
        assert_eq!(said(Some("truecolor"), never), Depth::Millions);
        assert_eq!(said(Some("24bit"), never), Depth::Millions);
        assert_eq!(said(None, || true), Depth::Millions, "terminfo's RGB");
        assert_eq!(said(None, || false), Depth::Palette, "macOS's own terminal");
        assert_eq!(said(Some("yes"), || false), Depth::Palette);
    }

    /// A terminal that said 256 is drawn the palette, and one that said
    /// millions is drawn RGB -- until the reader says otherwise, which
    /// wins either way.
    ///
    /// Deliberate break: `draw` handing `content` on as it came. The first
    /// assertion found the theme's RGB on a terminal that cannot draw it.
    #[test]
    fn a_frame_is_drawn_at_the_depth_asked_or_the_one_found() {
        let drawn = |found: Depth| {
            let mut terminal = ratatui::Terminal::new(Coloured::new(TestBackend::new(1, 1), found))
                .expect("a terminal");
            terminal
                .draw(|frame| {
                    let cell = &mut frame.buffer_mut()[(0, 0)];
                    cell.set_fg(Color::Rgb(0xff, 0, 0));
                    cell.set_bg(Color::Rgb(24, 24, 27));
                })
                .expect("drawn");
            let cell = &terminal.backend().inner.buffer()[(0, 0)];
            (cell.fg, cell.bg)
        };
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        depth::ask(None);
        assert_eq!(
            drawn(Depth::Palette),
            (Color::Indexed(196), Color::Indexed(234))
        );
        assert_eq!(
            drawn(Depth::Millions),
            (Color::Rgb(0xff, 0, 0), Color::Rgb(24, 24, 27))
        );
        depth::ask(Some(Depth::Palette));
        let asked = drawn(Depth::Millions);
        depth::ask(Some(Depth::Millions));
        let asked_back = drawn(Depth::Palette);
        depth::ask(None);
        assert_eq!(asked, (Color::Indexed(196), Color::Indexed(234)));
        assert_eq!(asked_back, (Color::Rgb(0xff, 0, 0), Color::Rgb(24, 24, 27)));
    }

    /// A change of depth draws the whole screen again, and not only the
    /// cells the frame changed: changing the setting changes no cell's
    /// colour, so a frame after it hands over little or nothing.
    ///
    /// Deliberate break: `moved` always false. The cell the second frame
    /// did not touch stayed in the palette on a terminal now drawing RGB.
    #[test]
    fn a_change_of_depth_draws_the_whole_screen_again() {
        let _turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut terminal =
            ratatui::Terminal::new(Coloured::new(TestBackend::new(2, 1), Depth::Palette))
                .expect("a terminal");
        let page = Color::Rgb(24, 24, 27);
        depth::ask(None);
        terminal
            .draw(|frame| {
                frame.buffer_mut()[(0, 0)].set_symbol("a").set_bg(page);
                frame.buffer_mut()[(1, 0)].set_symbol("b").set_bg(page);
            })
            .expect("drawn");
        depth::ask(Some(Depth::Millions));
        let drawn = terminal.draw(|frame| {
            frame.buffer_mut()[(0, 0)].set_symbol("c").set_bg(page);
            frame.buffer_mut()[(1, 0)].set_symbol("b").set_bg(page);
        });
        depth::ask(None);
        drawn.expect("drawn");
        let screen = terminal.backend().inner.buffer();
        assert_eq!(screen[(0, 0)].symbol(), "c");
        assert_eq!(screen[(0, 0)].bg, page);
        assert_eq!(screen[(1, 0)].symbol(), "b");
        assert_eq!(
            screen[(1, 0)].bg,
            page,
            "an untouched cell kept its palette colour"
        );
    }
}
