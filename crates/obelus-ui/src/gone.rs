//! What Obelus shows once the project it was on has gone from disk.
//!
//! **The whole of the screen, and two keys.** Everything a window does is
//! about a project, and there is not one: what was open in it was closed
//! with it, and a status row saying `Tree gone` under the welcome screen
//! offered keys that all went nowhere. So the page says what happened and
//! the two things that can happen next -- another project, or leaving.
//!
//! Laid out the way the page that asks which project is, because it is the
//! page before that one: what it is along the top under a rule, and its
//! keys along the foot over another.

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_command::Command;
use obelus_editing::keymap::{KeyChord, Keymap};
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{Hint, Screen, write};

/// What the page is, along the top of it.
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
        let hints = hints(self.keymap);
        let page = crate::footed(area, &hints);
        let row = |y: u16| Rect {
            y,
            height: 1,
            ..page
        };
        let ink = Style::new().fg(self.theme.foreground);
        write(cells, page.x + 2, page.y, TITLE, ink);
        crate::rule(cells, row(page.y.saturating_add(1)), self.theme);
        // A sentence about a path, which is a name: so the sentence starts
        // somewhere else rather than with a path whose first letter it
        // would have to change.
        if page.height > 2 {
            write(
                cells,
                page.x + 2,
                page.y + 2,
                &format!("Nothing is left at {}", self.was),
                ink,
            );
        }
        crate::foot_without_a_card(cells, area, &hints, self.theme);
    }
}
