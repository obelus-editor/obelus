//! What Obelus shows once the project it was on has gone from disk -- or
//! the machine it was drawing an Obelus on cannot be reached.
//!
//! **The whole of the screen, over whatever the reader was in.** Everything
//! a window does is about a project, and there is not one: so nothing
//! under the page answers a key any more, and the page says what happened
//! and what can happen next -- another project, or leaving, and for a
//! machine that went, trying it again. What it is drawn over is what the
//! reader was looking at when the ground went, which a window shows through
//! it as glass.
//!
//! **One page for both.** A project gone from under the window and a
//! machine gone from the other end of its connection are the same thing
//! to the reader -- what they were working in is not there -- and are said
//! by the same page in the same place, with the keys that differ.
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

/// The key that tries a machine that went again, at once.
pub const AGAIN: char = 'r';

/// The most rows the reason a machine went is given.
const WHY_ROWS: usize = 3;

/// What has gone.
enum Gone {
    /// The project, which was here.
    Project(String),
    /// The machine at the other end, why, and whether it is being tried.
    Machine {
        at: String,
        why: String,
        trying: bool,
    },
}

/// The keys the foot says.
///
/// Leaving is said, as it is on the page that asks which project: escape
/// cannot leave this one either, because there is nothing behind it to go
/// back to.
fn hints(keymap: &Keymap, gone: &Gone) -> Vec<Hint> {
    let mut hints = match gone {
        Gone::Project(_) => vec![Hint::new(
            KeyChord::new(KeyCode::Enter, KeyModifiers::NONE),
            "Open a project",
        )],
        Gone::Machine { .. } => vec![
            Hint::new(
                KeyChord::new(KeyCode::Char(AGAIN), KeyModifiers::NONE),
                "Try again",
            ),
            Hint::new(
                KeyChord::new(KeyCode::Enter, KeyModifiers::NONE),
                "Work on this machine",
            ),
        ],
    };
    if let Some(chord) = keymap.chord_for(Command::Quit) {
        hints.push(Hint::new(chord, "Leave"));
    }
    hints
}

/// The page.
pub struct GoneView<'a> {
    gone: Gone,
    keymap: &'a Keymap,
    theme: &'a Theme,
}

impl<'a> GoneView<'a> {
    /// Borrows what the view needs, where the project or the machine has
    /// gone.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        let gone = match app.machine_gone() {
            Some((at, why, trying)) => Gone::Machine {
                at: at.to_string(),
                why: why.to_string(),
                trying,
            },
            None if app.tree_has_gone() => {
                Gone::Project(crate::with_home_as_tilde(app.working_directory()))
            }
            None => return None,
        };
        Some(Self {
            gone,
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
        let hints = hints(self.keymap, &self.gone);
        let page = crate::footed(area, &hints);
        let room = usize::from(page.width.saturating_sub(4));
        let lines: Vec<(String, ratatui::style::Color)> = match &self.gone {
            // A sentence about a path, which is a name: so the sentence
            // starts somewhere else rather than with a path whose first
            // letter it would have to change. Cut from the left where it
            // does not fit, the way a path is cut everywhere it is one.
            Gone::Project(was) => {
                let lead = "Nothing is left at ";
                let room = room.saturating_sub(obelus_text::text_width(lead));
                vec![
                    ("The project has gone".to_string(), self.theme.foreground),
                    (String::new(), self.theme.foreground),
                    (
                        format!("{lead}{}", crate::truncate_from_left(was, room)),
                        self.theme.gutter,
                    ),
                ]
            }
            // The machine's name mid-sentence, where it keeps its own
            // spelling; and what went wrong as the connection said it,
            // which is not Obelus's to capitalise. That is a sentence and not
            // a path, so it is wrapped rather than cut: its head says what
            // was being done and its tail what the system answered, and
            // cutting either loses half the answer. Somebody else's text,
            // so a few rows of it at most, the last cut where it ends.
            Gone::Machine { at, why, trying } => {
                let lead = "Obelus cannot reach ";
                let room_for_at = room.saturating_sub(obelus_text::text_width(lead));
                let mut lines = vec![
                    (
                        format!("{lead}{}", crate::truncate_from_left(at, room_for_at)),
                        self.theme.foreground,
                    ),
                    (String::new(), self.theme.foreground),
                ];
                let mut said = obelus_text::wrapped(why, u16::try_from(room).unwrap_or(u16::MAX));
                if said.len() > WHY_ROWS {
                    let rest = said.split_off(WHY_ROWS - 1).join(" ");
                    said.push(crate::truncate_from_right(&rest, room));
                }
                lines.extend(said.into_iter().map(|row| (row, self.theme.gutter)));
                if *trying {
                    lines.push(("Trying again".to_string(), self.theme.gutter));
                }
                lines
            }
        };
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let top = page.y + page.height.saturating_sub(height) / 2;
        for (offset, (line, ink)) in (0..).zip(&lines) {
            let y = top + offset;
            if y >= page.bottom() {
                break;
            }
            let width = u16::try_from(obelus_text::text_width(line)).unwrap_or(u16::MAX);
            let x = page.x + page.width.saturating_sub(width) / 2;
            write(cells, x, y, line, Style::new().fg(*ink));
        }
        crate::foot(cells, area, &hints, self.theme);
    }
}
