//! The settings view.
//!
//! A tab row, a rule, and a row per setting: its name on the left, what it
//! does after that in the dim colour, and its control on the right. Whatever
//! has the focus wears the selected row's background, which is what that
//! background means everywhere else on this screen.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::{App, agents::Listed},
    component::settings::{DESCRIPTION_INDENT, Refused, Settings},
    config::{Config, Kind, Value},
    theme::Theme,
    ui::{Marked, Matched, fill, put, rule, text_width, write, write_marked},
};

/// How wide a control's column is.
///
/// Fixed, so the controls line up down the screen: a column of `on` and
/// `off` and theme names at ragged left edges is three columns pretending to
/// be one.
const CONTROL_WIDTH: u16 = 12;

/// How far a card's words are indented from its edge.
///
/// A card whose text touches the edge of its own background reads as a row
/// of a table rather than as a card.
const INDENT: u16 = 3;

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
    /// The agents the registry lists, with what obelus knows about each.
    agents: Vec<Listed>,
    /// Why the list could not be fetched, if it could not.
    failure: Option<&'a str>,
    /// The agents' own marks, for a terminal that can draw one.
    images: &'a crate::ui::image::Images,
    /// Every command and the key it is on, for the keys page.
    keys: Vec<(crate::command::Command, Option<crate::keymap::KeyChord>)>,
    /// The reader's own settings, under whatever the tree lays over them.
    ///
    /// What a row on the tree's page says beside a setting the tree has not
    /// got: the value showing is the reader's, or nobody's.
    readers: Config,
    /// The settings the tree has set, and the file it set them in.
    ///
    /// Written the way the reader would write it -- `.obelus.toml`, not the
    /// whole path -- because it is a file in the tree they are looking at.
    pinned: Vec<&'static str>,
    /// That file, if there is one.
    tree: Option<String>,
}

impl<'a> SettingsView<'a> {
    /// Borrows what the view needs, or nothing if the settings are not open.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            settings: app.settings()?,
            config: app.config(),
            theme: app.theme(),
            agents: app.listed_agents(),
            failure: app.registry_failure(),
            images: app.images(),
            keys: app.settings()?.keys(app.keymap()),
            readers: app.readers_config().clone(),
            pinned: app.pinned().to_vec(),
            // The file the tree has, or the one it would get: the tree's
            // page says which file it is writing before there is a file to
            // write, because that is the question a reader opening it has.
            tree: (app.settings().is_some_and(Settings::on_tree) || app.tree_config().is_some())
                .then(|| {
                    crate::ui::relative_to(
                        &crate::config::tree_path_for(app.working_directory()),
                        app.working_directory(),
                    )
                    .display()
                    .to_string()
                }),
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

        // The tabs, through the same function every other tab row goes
        // through: what a tab looks like is not this page's business.
        crate::ui::tabs(
            cells,
            area,
            &Settings::tabs(),
            self.settings.tab(),
            self.theme,
        );
        // Which file a change on this page is written to, on the tab row
        // and kept there: it is the whole of what makes this page different
        // from the other one, and a reader who cannot see it is a reader
        // editing something they have to remember.
        if let Some(tree) = self
            .settings
            .on_tree()
            .then_some(self.tree.as_deref())
            .flatten()
        {
            // Past the arrows that walk the tabs, which sit at the edge.
            let arrows = 4;
            let width = u16::try_from(crate::ui::text_width(tree)).unwrap_or(0);
            if area.width > width + arrows + 2 {
                write(
                    cells,
                    area.right() - width - arrows - 2,
                    area.y,
                    tree,
                    Style::new().fg(self.theme.gutter).bg(self.theme.background),
                );
            }
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

        // The agents are a page of cards rather than a column of controls:
        // a reader choosing between forty programs is reading about them,
        // and a row of a table has nowhere to say what one is.
        if self.settings.on_agents() {
            self.agents(
                cells,
                Rect {
                    y: area.y + 2,
                    height: area.height.saturating_sub(2),
                    ..area
                },
            );
            return;
        }

        let region = Rect {
            y: area.y + 2,
            height: area.height.saturating_sub(2),
            ..area
        };

        // The keys are a column of the same rows: a command, what it does,
        // and the key it is on -- with the row the reader is binding saying
        // so where its description was.
        // A tree may not move the keys or choose the agent, so on its page
        // those two tabs say so rather than showing rows nothing will
        // accept: a page of controls that all refuse is a page that has to
        // be tried before it can be understood.
        if self.settings.on_tree() && (self.settings.on_keys() || self.settings.on_agents()) {
            crate::ui::nothing(
                cells,
                Rect {
                    height: 1,
                    ..region
                },
                match self.settings.on_keys() {
                    true => "a tree may not move the keys",
                    false => "a tree may not choose the agent",
                },
                self.theme,
            );
            return;
        }

        if self.settings.on_keys() {
            let rows: Vec<Row> = self
                .keys
                .iter()
                .map(|(command, chord)| Row {
                    label: command.name().to_string(),
                    matched: self.settings.matched_in(command.name()),
                    detail: self.saying(*command),
                    aside: Aside::Words(chord.map(|chord| chord.label()).unwrap_or_default()),
                    body: Vec::new(),
                    pinned: None,
                    inherited: None,
                })
                .collect();
            self.column(cells, region, &rows, "no command by that name");
            return;
        }

        let settings = self.settings.rows();
        let rows: Vec<Row> = settings
            .iter()
            .map(|setting| Row {
                label: setting.name.to_string(),
                matched: self.settings.matched(setting),
                detail: None,
                // What it does, on its own rows under the name: beside it,
                // the two were competing for one row -- and the one that
                // lost was the description, cut off with an ellipsis on
                // exactly the rows that had most to explain.
                body: self.settings.wrapped(
                    setting.about,
                    crate::component::settings::description_width(region.width),
                ),
                aside: Aside::Control(setting.kind, Settings::value_of(setting, self.config)),
                // On the reader's page, the file that has this one instead
                // of them. On the tree's, nothing: a setting the tree has
                // is exactly what that page is for.
                pinned: (!self.settings.on_tree())
                    .then(|| {
                        self.pinned
                            .contains(&setting.key)
                            .then(|| self.tree.clone())
                            .flatten()
                    })
                    .flatten(),
                // And on the tree's page, whose value is showing where the
                // tree has not set this one.
                inherited: self
                    .settings
                    .on_tree()
                    .then(|| self.inherited(setting))
                    .flatten(),
            })
            .collect();
        self.column(cells, region, &rows, "no setting by that name");
    }
}

/// One row of a page: what it is called, which of its characters the query
/// matched, what it is, and what sits on the right.
struct Row {
    label: String,
    matched: Option<std::ops::Range<usize>>,
    detail: Option<(String, ratatui::style::Color)>,
    aside: Aside,
    /// What it does, under the name and indented, already broken into the
    /// rows it takes. Empty on a page whose rows are one row each.
    body: Vec<String>,
    /// Whose value is showing, on a page that is not the one that has the
    /// setting.
    ///
    /// `None` where this page has it, which is what makes the row bright:
    /// the ink says whether a row belongs to the page it is on, the way it
    /// does everywhere else in obelus.
    inherited: Option<&'static str>,
    /// The file that has this one, when it is not the reader's to change.
    ///
    /// Named on the row rather than said when the reader tries to move it:
    /// a row that answers only when pushed is a row that looks like every
    /// other until it is, and what a reader wants to know here is which of
    /// these are theirs.
    pinned: Option<String>,
}

/// What a row shows on the right.
enum Aside {
    /// A setting's control: a switch, or the word it is set to.
    Control(Kind, Value),
    /// Words -- the key a command is on, and nothing when it is on none.
    Words(String),
}

impl SettingsView<'_> {
    /// A page's rows, drawn one to a row.
    ///
    /// One loop for the settings and for the keys, because they are the
    /// same row: something named on the left, something to the right of it,
    /// and the characters the query matched marked the way every list marks
    /// them. What differs is what the two halves hold.
    fn column(&self, cells: &mut CellBuffer, region: Rect, rows: &[Row], empty: &str) {
        let plain = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        if rows.is_empty() {
            crate::ui::nothing(
                cells,
                Rect {
                    height: 1,
                    ..region
                },
                empty,
                self.theme,
            );
            return;
        }

        // The same bar every other list has when there is more of it than
        // there is screen.
        let window = self.settings.window();
        let scrolling = window.scrollable(region.height);
        if scrolling {
            crate::ui::scrollbar(cells, region, window.top(), rows.len(), self.theme);
        }
        let room = match scrolling {
            true => region
                .width
                .saturating_sub(crate::ui::editor::SCROLLBAR_WIDTH),
            false => region.width,
        };
        let aside_at = region.x + room.saturating_sub(CONTROL_WIDTH + 1);
        // Where what a row *does* starts, measured over the whole page
        // rather than taken from each name: a column of names is read down,
        // and prose that started at a different column on every row would
        // be four beginnings to find rather than one.
        let names = rows
            .iter()
            .filter(|row| row.detail.is_some())
            .map(|row| crate::ui::text_width(&row.label))
            .max()
            .unwrap_or(0);
        let detail_at = region.x + 1 + u16::try_from(names).unwrap_or(0) + 2;
        // Walked by height rather than by row, because an entry is as tall
        // as what it has to say: a name, the rows its description takes, and
        // a blank so that the next name is not read as part of it. Every
        // height here is one on the pages whose rows are one row each, which
        // is the same walk it always was.
        let first = window.top().min(window.focus());
        let mut y = region.y;
        for (index, row) in rows.iter().enumerate().skip(first) {
            if y >= region.bottom() {
                break;
            }
            let focused = index == self.settings.focus();
            let background = if focused {
                self.theme.selected_row_background
            } else {
                self.theme.background
            };
            // The background covers the name and what it says, but not the
            // blank under them: a run of colour that reached into the gap
            // would close it up again.
            let tall = u16::try_from(row.body.len()).unwrap_or(0) + 1;
            let area = Rect {
                y,
                height: tall.min(region.bottom().saturating_sub(y)),
                width: room,
                ..region
            };
            fill(cells, area, plain.bg(background));
            let area = Rect { height: 1, ..area };

            // What the row says about where it came from, measured before
            // the name is: it is written between the two, so the name is cut
            // to what is left rather than to the whole row.
            // The lock is the reader's page saying "not here". The tree's
            // page has none: there, everything is here.
            let lock = u16::from(crate::icons::enabled() && row.pinned.is_some()) * 2;
            // Where the value showing comes from, on whichever page is not
            // the one that has it: the file's name and a lock on the
            // reader's, the word `yours` or `default` on the tree's. One
            // column, because it is one question.
            let source = row.pinned.as_deref().or(row.inherited).map(|source| {
                let room = aside_at.saturating_sub(region.x + 4 + lock);
                clipped(source, room)
            });
            let reserved = source.as_deref().map_or(0, |source| {
                u16::try_from(crate::ui::text_width(source)).unwrap_or(0) + lock + 1
            });

            // Cut to what is left before the right-hand column: a line
            // running under it reads as part of it.
            let width = aside_at.saturating_sub(region.x + 2 + reserved);
            let label = clipped(&row.label, width);
            // Through the shared writer, so the characters the query
            // matched carry the background every other list marks a match
            // with: a row in a narrowed list has to say why it is in it.
            // The ink says whether a row can be used, which is the rule
            // everywhere here: a setting the tree has is not this reader's
            // to move, and a row that looked live until they pressed it
            // would be a row that lied.
            // Dim says "not yours to use here", which on the reader's page
            // is exactly what a setting the tree has taken is. On the
            // tree's page it would be the opposite of the truth: a row the
            // tree has not got is the one thing on that page a reader *can*
            // do something to -- pressing it is how a setting becomes the
            // project's. So there the row is ordinary, and what is dim is
            // the word saying where the value showing comes from, and the
            // control showing it.
            let ink = match row.pinned {
                Some(_) => plain.fg(self.theme.gutter),
                None => plain,
            };
            let after = write_marked(
                cells,
                area,
                region.x + 1,
                y,
                &label,
                ink.bg(background),
                &Marked::matched(
                    run_of(row.matched.clone()),
                    self.theme.picker_match_background,
                ),
            );
            if let Some((detail, colour)) = row.detail.as_ref() {
                // Into its own column, two past the longest name: near
                // enough to the name to read as one phrase, and in a line
                // down the page so that the page is read as a table.
                let at = detail_at.max(after + 2);
                let left = aside_at.saturating_sub(at + reserved);
                write(
                    cells,
                    at,
                    y,
                    &clipped(detail, left),
                    plain.fg(*colour).bg(background),
                );
            }
            // Where it came from, in the space the reader would otherwise
            // reach across, and a lock against the control itself: the name
            // says which file has it and the lock says it is shut, which
            // between them is the whole answer without a word of prose.
            if let Some(source) = &source {
                let width = u16::try_from(crate::ui::text_width(source)).unwrap_or(0);
                write(
                    cells,
                    aside_at.saturating_sub(width + lock + 1),
                    y,
                    source,
                    plain.fg(self.theme.gutter).bg(background),
                );
                if crate::icons::enabled() && row.pinned.is_some() {
                    put(
                        cells,
                        aside_at.saturating_sub(2),
                        y,
                        '\u{f033e}',
                        plain.fg(self.theme.gutter).bg(background),
                    );
                }
            }

            match &row.aside {
                Aside::Control(kind, value) => draw_control(
                    cells,
                    ratatui::layout::Position { x: aside_at, y },
                    *kind,
                    value,
                    self.theme,
                    background,
                    row.pinned.is_none() && row.inherited.is_none(),
                ),
                Aside::Words(words) => {
                    write(
                        cells,
                        aside_at,
                        y,
                        &clipped(words, CONTROL_WIDTH),
                        plain.fg(self.theme.foreground).bg(background),
                    );
                }
            }

            // What it does, under its name and indented under it, in the
            // dim colour: it is the answer to a question the name has
            // already asked, so it is read after the name or not at all.
            for (offset, line) in row.body.iter().enumerate() {
                let Ok(offset) = u16::try_from(offset + 1) else {
                    break;
                };
                if y + offset >= region.bottom() {
                    break;
                }
                write(
                    cells,
                    region.x + DESCRIPTION_INDENT,
                    y + offset,
                    line,
                    plain.fg(self.theme.gutter).bg(background),
                );
            }

            // And a blank before the next one, which is the whole of what
            // makes an entry an entry.
            y += tall + u16::from(!row.body.is_empty());
        }
    }

    /// Whose value a row is showing, where this page is not the one that
    /// has the setting.
    ///
    /// Two answers and not three: the reader's, or nobody's. "Nobody's" is
    /// the default obelus ships with, and worth saying plainly -- a reader
    /// looking at a tree's settings wants to know which of these anybody
    /// has an opinion about at all.
    fn inherited(&self, setting: &crate::config::Setting) -> Option<&'static str> {
        if self.pinned.contains(&setting.key) {
            return None;
        }
        let theirs = Settings::value_of(setting, &self.readers);
        let default = Settings::value_of(setting, &Config::default());
        Some(if theirs == default {
            "default"
        } else {
            "yours"
        })
    }

    /// What a command's row says beside its name.
    ///
    /// What it does, unless the reader is binding it: then it is what the
    /// page is waiting for, or why the key they pressed will not do. On the
    /// row because that is where they are looking and it is that binding
    /// the answer is about -- the status row here is the page's filter, and
    /// a passing note would be cleared by the very next keystroke.
    fn saying(&self, command: crate::command::Command) -> Option<(String, ratatui::style::Color)> {
        if self.settings.binding() != Some(command) {
            return Some((command.spec().title.to_string(), self.theme.gutter));
        }
        match self.settings.refused() {
            Some((chord, Refused::Taken(taken))) => Some((
                format!("{} is {}", chord.label(), taken.name()),
                self.theme.change_removed,
            )),
            Some((chord, Refused::Never(why))) => Some((
                format!("{}: {why}", chord.label()),
                self.theme.change_removed,
            )),
            None => Some((
                "press a key, or delete to unbind".to_string(),
                self.theme.change_modified,
            )),
        }
    }
}

/// How many rows a card takes, before its reason for having failed.
///
/// Three: what it is called, what it is, and who wrote it. A fourth when an
/// install went wrong, and a blank one after each so that a card reads as a
/// card rather than as three rows of a table.
impl SettingsView<'_> {
    /// The agents, as cards.
    fn agents(&self, cells: &mut CellBuffer, area: Rect) {
        let plain = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        let dim = plain.fg(self.theme.gutter);

        let listed = self.settings.agents(&self.agents);
        if listed.is_empty() {
            let reason = match (self.agents.is_empty(), self.failure) {
                // Two ways to have nothing, and the reader's next move
                // differs: wait, or look at their network.
                (true, Some(why)) => format!("could not fetch the list of agents: {why}"),
                (true, None) => "fetching the list of agents\u{2026}".to_string(),
                (false, _) => "no agent by that name".to_string(),
            };
            write(
                cells,
                area.x + INDENT,
                area.y,
                &clipped(&reason, area.width.saturating_sub(INDENT * 2)),
                dim,
            );
            return;
        }

        // Which card is at the top is the page's own, moved only when the
        // focus leaves the window -- so a step that is not at an edge
        // scrolls nothing. Heights here are for laying the cards out, not
        // for deciding where the window is.
        let width = area.width.saturating_sub(7);
        let heights: Vec<u16> = listed
            .iter()
            .map(|agent| self.settings.card_rows(agent, width) + 1)
            .collect();
        let focus = self.settings.focus().min(listed.len().saturating_sub(1));
        let first = self.settings.top().min(focus);

        let mut y = area.y;
        for (index, agent) in listed.iter().enumerate().skip(first) {
            if y >= area.bottom() {
                break;
            }
            self.card(cells, Rect { y, ..area }, agent, index == focus);
            y += heights[index];
        }
    }

    /// One card.
    ///
    /// Three rows and a blank one: what it is called, what it is, and the
    /// facts about it -- version, who wrote it, under what licence. The
    /// version lives with those rather than beside the name, where it
    /// competes with the one thing a reader is scanning for; the right-hand
    /// corner carries only the thing that can be pressed.
    ///
    /// The focused card is the one with a background. No bar down its side:
    /// a block of coloured rows already reads as one thing, and a bar is a
    /// second mark saying the same thing in a different language.
    fn card(&self, cells: &mut CellBuffer, area: Rect, agent: &Listed, focused: bool) {
        let background = if focused {
            self.theme.selected_row_background
        } else {
            self.theme.background
        };
        let plain = Style::new().fg(self.theme.foreground).bg(background);
        let dim = plain.fg(self.theme.gutter);

        // Where the words start, and how much room they have. Indented,
        // because a card whose text touches the edge of its background
        // reads as a row of a table.
        let left = area.x + INDENT;
        let inner = area.width.saturating_sub(INDENT * 2);
        let failure = match &agent.status {
            crate::agent::Status::Failed(why) => Some(why.as_str()),
            _ => None,
        };
        let described = self.settings.wrapped(&agent.agent.description, inner);
        let rows = self.settings.card_rows(agent, inner);

        for row in 0..rows {
            if area.y + row >= area.bottom() {
                break;
            }
            fill(
                cells,
                Rect {
                    y: area.y + row,
                    height: 1,
                    ..area
                },
                plain,
            );
        }

        let mut name_at = left;
        if crate::icons::enabled() {
            // The agent's real mark where the terminal can show a picture,
            // and a glyph where it cannot. Both are two cells wide, so the
            // words start in the same place either way -- which is why the
            // one that fails is allowed to fail silently.
            if !self
                .images
                .draw(cells, left, area.y, &agent.agent.id, focused)
            {
                put(
                    cells,
                    left,
                    area.y,
                    crate::icons::for_agent(&agent.agent.id),
                    plain.fg(self.theme.gutter_current),
                );
            }
            // Two blank columns after a glyph: one the glyph bleeds into,
            // because a Nerd Font draws these two cells wide, and one to
            // read by.
            name_at = left + 3;
        }

        let (state, colour) = self.state_of(agent);
        let taken = u16::try_from(text_width(&state)).unwrap_or(0);
        let room = inner.saturating_sub(taken + 2);
        write_marked(
            cells,
            Rect {
                y: area.y,
                height: 1,
                ..area
            },
            name_at,
            area.y,
            &clipped(&agent.agent.name, room),
            plain,
            &Marked::matched(
                run_of(self.settings.matched_in(&agent.agent.name)),
                self.theme.picker_match_background,
            ),
        );
        if let Ok(offset) = u16::try_from(
            usize::from(area.width).saturating_sub(text_width(&state) + usize::from(INDENT)),
        ) {
            write(cells, area.x + offset, area.y, &state, plain.fg(colour));
        }

        // Under the name rather than under the glyph: the glyph is a mark
        // on the card, and the words are a column.
        let mut y = area.y + 1;
        for row in &described {
            if y >= area.bottom() {
                return;
            }
            write(cells, name_at, y, row, dim);
            y += 1;
        }
        if described.is_empty() {
            y += 1;
        }

        if y < area.bottom() {
            write(
                cells,
                name_at,
                y,
                &clipped(&self.facts(agent), inner.saturating_sub(3)),
                dim,
            );
            y += 1;
        }
        // Why it did not install, under the card it is about: a reason on
        // the status bar is gone by the next keystroke, and this one is
        // about something still on screen.
        if let Some(why) = failure
            && y < area.bottom()
        {
            write(
                cells,
                name_at,
                y,
                &clipped(&format!("\u{f0159} {why}"), inner.saturating_sub(3)),
                plain.fg(self.theme.change_removed),
            );
        }
    }

    /// The line of facts under a card's description.
    ///
    /// The version first, because it is the one a reader compares against
    /// what they have -- and when there is something newer, both numbers
    /// and the arrow between them.
    fn facts(&self, agent: &Listed) -> String {
        let mut facts: Vec<String> = Vec::new();
        match &agent.status {
            crate::agent::Status::Outdated { installed } => {
                facts.push(format!("{installed} \u{2192} {}", agent.agent.version));
            }
            _ if !agent.agent.version.is_empty() => facts.push(agent.agent.version.clone()),
            _ => {}
        }
        if !agent.agent.authors.is_empty() {
            facts.push(agent.agent.authors.join(", "));
        }
        if !agent.agent.license.is_empty() {
            facts.push(agent.agent.license.clone());
        }
        facts.join(" \u{b7} ")
    }

    /// What a card's right-hand corner says, and in what colour.
    fn state_of(&self, agent: &Listed) -> (String, ratatui::style::Color) {
        use crate::agent::Status;
        match &agent.status {
            Status::Installed if agent.active => {
                ("\u{25cf} active".to_string(), self.theme.change_added)
            }
            Status::Installed => ("installed".to_string(), self.theme.gutter),
            Status::Outdated { .. } => ("update \u{25b8}".to_string(), self.theme.change_modified),
            Status::Missing | Status::Failed(_) => {
                ("install \u{25b8}".to_string(), self.theme.foreground)
            }
            Status::Installing => (self.installing(agent), self.theme.foreground),
            Status::Unavailable(why) => ((*why).to_string(), self.theme.gutter),
        }
    }

    /// What an install that is running says about itself.
    ///
    /// Bytes and a time only when there are bytes to count: a package
    /// manager is asked to do the whole job and says nothing until it has,
    /// so the card says it is running and nothing it cannot know.
    fn installing(&self, agent: &Listed) -> String {
        let Some(progress) = agent.progress else {
            return "installing\u{2026}".to_string();
        };
        match (progress.fraction(), progress.remaining()) {
            (Some(fraction), Some(left)) => format!(
                "installing {}% \u{b7} {}s left",
                (fraction * 100.0).round() as u32,
                left.as_secs().max(1)
            ),
            (Some(fraction), None) => {
                format!("installing {}%", (fraction * 100.0).round() as u32)
            }
            _ => "installing\u{2026}".to_string(),
        }
    }
}

/// A matched run, as the shared writer takes it.
fn run_of(run: Option<std::ops::Range<usize>>) -> Matched<'static> {
    run.map_or(Matched::Nothing, |run| Matched::Run(run.start, run.end))
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
    at: ratatui::layout::Position,
    kind: Kind,
    value: &Value,
    theme: &Theme,
    background: ratatui::style::Color,
    usable: bool,
) {
    let (x, y) = (at.x, at.y);
    let style = Style::new().bg(background);
    // A control the reader cannot move is drawn in the dim ink, value and
    // all: what it is set to is still worth seeing -- it is what this tree
    // has decided -- and what they cannot do about it is said the way
    // everything unusable here says it.
    let ink = if usable {
        theme.foreground
    } else {
        theme.gutter
    };
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
                style.bg(theme.control_background),
            );
            // Bright when on and dim when off, rather than a colour: the
            // knob's *position* already says which way it is, so a hue
            // would be a second answer to a question already answered --
            // and green here would mean something different from green in
            // the margin, where it means a line git has never seen.
            let (at, colour) = if *on {
                (x + TRACK_WIDTH / 2, ink)
            } else {
                (x, theme.gutter)
            };
            for cell in 0..TRACK_WIDTH / 2 {
                put(
                    cells,
                    at + cell,
                    y,
                    '\u{25a0}',
                    Style::new().fg(colour).bg(theme.control_background),
                );
            }
        }
        (Kind::Choice(_), Value::Choice(word)) => {
            let after = write(cells, x, y, word, style.fg(ink));
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
