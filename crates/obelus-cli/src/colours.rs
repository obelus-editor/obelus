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
//! a colour: every colour Obelus draws comes through here, a theme's, a
//! program's in the terminal, a picture's marks, and none of what picks them
//! has to know. The setting is read at every frame, because it can change
//! under a frame -- another Obelus writing the settings, or one on another
//! machine saying so.

use obelus_theme::depth::{self, Depth};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
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
}

impl<B> Coloured<B> {
    /// `inner`, on a terminal that said `found`.
    pub(crate) const fn new(inner: B, found: Depth) -> Self {
        Self { inner, found }
    }
}

impl<B: Backend> Backend for Coloured<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let depth = depth::asked().unwrap_or(self.found);
        if depth == Depth::Millions {
            return self.inner.draw(content);
        }
        // Copies, because a frame lends its cells: it is only what changed
        // since the last frame, so a few rows rather than the screen.
        let brought: Vec<(u16, u16, Cell)> = content
            .map(|(x, y, cell)| {
                let mut cell = cell.clone();
                cell.fg = depth.of(cell.fg);
                cell.bg = depth.of(cell.bg);
                cell.underline_color = depth.of(cell.underline_color);
                (x, y, cell)
            })
            .collect();
        self.inner
            .draw(brought.iter().map(|(x, y, cell)| (*x, *y, cell)))
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
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
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
}
