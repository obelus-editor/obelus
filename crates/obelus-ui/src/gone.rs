//! What Obelus shows once the project it was on has gone from disk.
//!
//! **The whole of the screen, over whatever the reader was in.** Everything
//! a window does is about a project, and there is not one: so nothing
//! under the page answers a key any more, and the page says what happened
//! and the two things that can happen next -- another project, or leaving.
//! What it is drawn over is what the reader was looking at when the ground
//! went, which a window shows through it as glass.
//!
//! **What happened, in the middle.** The page has one thing to say and no
//! list to lay out, so it is said where the eye is rather than where a
//! page's title goes; the keys are where every page keeps them, along the
//! foot over a rule.

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_command::Command;
use obelus_keymap::{KeyChord, Keymap};
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{Hint, Screen, fill, write};

/// What happened, which is the page's one line in the ink.
const TITLE: &str = "The project has gone";

/// The keys the foot says.
///
/// Leaving is said, as it is on the page that asks which project: escape
/// cannot leave this one either, because there is nothing behind it to go
/// back to.
fn hints(keymap: &Keymap) -> Vec<Hint> {
    let mut hints = vec![Hint::common(
        KeyChord::new(KeyCode::Enter, KeyModifiers::NONE),
        "Open a project",
    )];
    if let Some(chord) = keymap.chord_for(Command::Quit) {
        hints.push(Hint::common(chord, "Leave").saying("Leave Obelus"));
    }
    hints
}

/// The page.
pub struct GoneView<'a> {
    /// Where the project was, as the reader would write it.
    was: String,
    keymap: &'a Keymap,
    theme: &'a Theme,
}

impl<'a> GoneView<'a> {
    /// Borrows what the view needs, where the project has gone.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        if !app.tree_has_gone() {
            return None;
        }
        Some(Self {
            was: crate::with_home_as_tilde(app.working_directory()),
            keymap: app.keymap(),
            theme: app.theme(),
        })
    }
}

impl Widget for GoneView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        // The page's own ground first: in a terminal nothing else covers
        // what was under it, and in a window this is what the glass is
        // tinted with.
        fill(cells, area, Style::new().bg(self.theme.background));
        let hints = hints(self.keymap);
        let page = crate::footed(area, &hints);
        // A sentence about a path, which is a name: so the sentence starts
        // somewhere else rather than with a path whose first letter it
        // would have to change. Cut from the left where it does not fit,
        // the way a path is cut everywhere it is one.
        let lead = "Nothing is left at ";
        let room =
            usize::from(page.width.saturating_sub(4)).saturating_sub(obelus_text::text_width(lead));
        let said = format!("{lead}{}", crate::truncate_from_left(&self.was, room));
        let lines = [
            (TITLE, self.theme.foreground),
            ("", self.theme.foreground),
            (said.as_str(), self.theme.gutter),
        ];
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let top = page.y + page.height.saturating_sub(height) / 2;
        for (offset, (line, ink)) in (0..).zip(lines) {
            let y = top + offset;
            if y >= page.bottom() {
                break;
            }
            let width = u16::try_from(obelus_text::text_width(line)).unwrap_or(u16::MAX);
            let x = page.x + page.width.saturating_sub(width) / 2;
            write(cells, x, y, line, Style::new().fg(ink));
        }
        crate::foot_without_a_card(cells, area, &hints, self.theme);
    }
}
