//! The settings view.
//!
//! A tab row, a rule, and a row per setting: its name on the left, what it
//! does after that in the dim colour, and its control on the right. Whatever
//! has the focus wears the selected row's background, which is what that
//! background means everywhere else on this screen.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::settings::Settings,
    config::{Config, Kind, Value},
    theme::Theme,
    ui::{fill, put, rule, text_width, write},
};

/// How wide a control's column is.
///
/// Fixed, so the controls line up down the screen: a column of `on` and
/// `off` and theme names at ragged left edges is three columns pretending to
/// be one.
const CONTROL_WIDTH: u16 = 12;

/// How wide a switch's track is, in cells.
///
/// Four: two for the knob and two for the room it slides into. Anything
/// narrower stops looking like something that slides.
const TRACK_WIDTH: u16 = 4;

/// The settings, over the whole editor region.
pub struct SettingsView<'a> {
    settings: &'a Settings,
    config: &'a Config,
    theme: &'a Theme,
}

impl<'a> SettingsView<'a> {
    /// Borrows what the view needs, or nothing if the settings are not open.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            settings: app.settings()?,
            config: app.config(),
            theme: app.theme(),
        })
    }
}

impl Widget for SettingsView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
        if area.height < 3 || area.width < CONTROL_WIDTH + 4 {
            return;
        }

        let plain = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        let dim = plain.fg(self.theme.gutter);

        // The tabs, the same shape the pickers use: the one showing wears
        // the selected background, and the arrows say how to change it.
        let mut column = 1u16;
        for (index, name) in Settings::tabs().iter().enumerate() {
            let style = if index == self.settings.tab() {
                plain.bg(self.theme.picker_selected_background)
            } else {
                dim
            };
            column = write(cells, area.x + column, area.y, &format!(" {name} "), style)
                .saturating_sub(area.x);
        }
        // The arrows and nothing else, because the arrows are what walks
        // them: a hint that named a key which does nothing is worse than no
        // hint, and the tab key's glyph in a Nerd Font reads as a return
        // arrow, which was read as one.
        let keys = "\u{2190} \u{2192}";
        if let Ok(offset) =
            u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
            && offset > column
        {
            write(cells, area.x + offset, area.y, keys, dim);
        }
        rule(
            cells,
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
            self.theme,
        );

        let rows = self.settings.rows();
        if rows.is_empty() {
            write(
                cells,
                area.x + 1,
                area.y + 2,
                "no setting by that name",
                dim,
            );
            return;
        }

        let top = area.y + 2;
        let control_at = area.x + area.width.saturating_sub(CONTROL_WIDTH + 1);
        for (index, setting) in rows.iter().enumerate() {
            let Ok(offset) = u16::try_from(index) else {
                break;
            };
            let y = top + offset;
            if y >= area.bottom() {
                break;
            }
            let focused = index == self.settings.focus();
            let background = if focused {
                self.theme.picker_selected_background
            } else {
                self.theme.background
            };
            let row = Rect {
                y,
                height: 1,
                ..area
            };
            fill(cells, row, plain.bg(background));

            // Cut to what is left before the control's column: a line
            // running under the controls reads as part of them.
            let label = clipped(setting.label, control_at.saturating_sub(area.x + 2));
            // In three pieces, so the characters the query matched can carry
            // the background every other list marks a match with: a row in a
            // narrowed list has to say why it is in it.
            let matched = self
                .settings
                .matched(setting)
                .filter(|run| run.start < label.chars().count());
            let mut at = area.x + 1;
            match matched {
                Some(run) => {
                    let end = run.end.min(label.chars().count());
                    let piece = |from: usize, to: usize| -> String {
                        label.chars().take(to).skip(from).collect()
                    };
                    at = write(cells, at, y, &piece(0, run.start), plain.bg(background));
                    at = write(
                        cells,
                        at,
                        y,
                        &piece(run.start, end),
                        plain.bg(self.theme.picker_match_background),
                    );
                    write(
                        cells,
                        at,
                        y,
                        &piece(end, label.chars().count()),
                        plain.bg(background),
                    );
                }
                None => {
                    write(cells, at, y, &label, plain.bg(background));
                }
            }
            draw_control(
                cells,
                control_at,
                y,
                setting.kind,
                &Settings::value_of(setting, self.config),
                self.theme,
                background,
            );
        }
    }
}

/// As much of a sentence as fits, with a mark where it was cut.
fn clipped(text: &str, room: u16) -> String {
    let room = usize::from(room);
    if text_width(text) <= room {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(room.saturating_sub(1)).collect();
    kept.push('\u{2026}');
    kept
}

/// Writes a control: a switch, or the word a droplist is set to.
fn draw_control(
    cells: &mut CellBuffer,
    x: u16,
    y: u16,
    kind: Kind,
    value: &Value,
    theme: &Theme,
    background: ratatui::style::Color,
) {
    let style = Style::new().bg(background);
    match (kind, value) {
        (Kind::Switch, Value::Switch(on)) => {
            // A slider: a square knob at one end of a short track. The
            // shape says which way it is without a word to read, and says
            // what the left and right arrows will do to it.
            //
            // Squares rather than full blocks: a full block fills its
            // cell's whole height, so the knobs of two rows one above the
            // other touch and read as one tall bar. A square leaves a
            // margin above and below, which is the gap between them.
            crate::ui::fill(
                cells,
                ratatui::layout::Rect {
                    x,
                    y,
                    width: TRACK_WIDTH,
                    height: 1,
                },
                style.bg(theme.status_background),
            );
            // Bright when on and dim when off, rather than a colour: the
            // knob's *position* already says which way it is, so a hue
            // would be a second answer to a question already answered --
            // and green here would mean something different from green in
            // the margin, where it means a line git has never seen.
            let (at, colour) = if *on {
                (x + TRACK_WIDTH / 2, theme.foreground)
            } else {
                (x, theme.gutter)
            };
            for cell in 0..TRACK_WIDTH / 2 {
                put(
                    cells,
                    at + cell,
                    y,
                    '\u{25a0}',
                    Style::new().fg(colour).bg(theme.status_background),
                );
            }
        }
        (Kind::Choice(_), Value::Choice(word)) => {
            let after = write(cells, x, y, word, style.fg(theme.foreground));
            // Pointing right, at the value: the list it opens is the
            // ordinary compact one and comes up wherever that comes up, so
            // an arrow pointing down would be pointing at whatever happens
            // to be under this row.
            put(cells, after + 1, y, '\u{25b8}', style.fg(theme.gutter));
        }
        (kind, value) => {
            tracing::debug!(?kind, ?value, "a control with nothing to draw");
        }
    }
}
