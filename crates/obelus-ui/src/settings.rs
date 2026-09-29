//! The settings view.
//!
//! A tab row, a rule, and a row per setting: its name on the left, what it
//! does after that in the dim colour, and its control on the right. Whatever
//! has the focus wears the selected row's background, which is what that
//! background means everywhere else on this screen.

use obelus_agent::Listed;
use obelus_component::settings::{
    DESCRIPTION_INDENT, GROUP_INDENT, HEADING_ROWS, Offering, Refused, Settings, Shown,
};
use obelus_config::{Config, Kind, Value};
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    Hint, Marked, Matched, Screen, fill, put, rule, truncate_from_right, write, write_marked,
};

/// How wide a control's column is.
///
/// Fixed, so the controls line up down the screen: a column of `on` and
/// `off` and theme names at ragged left edges is three columns pretending to
/// be one.
const CONTROL_WIDTH: u16 = 12;

/// Where the arrow beside a value goes: after it, and never past the
/// column's own last cell.
///
/// The arrow is put where the value stops, so a value as wide as the
/// column put it in the column beside -- which is the bar's, for the whole
/// page, and a bar with a hole in its own cells is not a bar any more: the
/// window stopped drawing it as a shape and the blocks a terminal has for
/// one came out as a row of squares. The fonts setting did worse without
/// the arrow's help, being a list of names joined with commas and written
/// out to whatever the row had.
fn arrow_at(x: u16, after: u16) -> u16 {
    after.saturating_add(1).min(x + CONTROL_WIDTH)
}

/// What the agent's heading says under its name.
///
/// The one thing about this group that is not true of the others: Obelus's
/// own settings take effect where they stand, and these are about the next
/// conversation rather than the one on screen. Said once, under the name,
/// rather than on every row -- a fact about the group is not a fact about
/// each of its rows, and the rows have a column each of their own to spend.
const WHEN: &str = "What a new conversation starts on.";

/// What an agent's row says where the reader has chosen nothing.
///
/// A word rather than a blank: the third state of one of these rows is a
/// decision like the other two -- leave it to the agent -- and a control
/// showing nothing would read as one Obelus had failed to fill in.
const AGENTS_OWN: &str = "Agent's own";

/// How far a card's words are indented from its edge.
///
/// A card whose text touches the edge of its own background reads as a row
/// of a table rather than as a card.
const INDENT: u16 = 3;

/// The settings, over the whole editor region.
pub struct SettingsView<'a> {
    settings: &'a Settings,
    config: &'a Config,
    theme: &'a Theme,
    /// The agents the registry lists, with what Obelus knows about each.
    agents: Vec<Listed>,
    /// Why the list could not be fetched, if it could not.
    failure: Option<&'a str>,
    /// The agents' own marks, for a terminal that can draw one.
    images: &'a crate::image::Images,
    /// Every command and the key it is on, for the keys page.
    keys: Vec<(
        obelus_command::Command,
        Option<obelus_editing::keymap::KeyChord>,
    )>,
    /// What the active agent offers to be set, and what the reader has
    /// said about each. `None` where no agent is active.
    offering: Option<Offering>,
    /// The settings the project has set, and the file it set them in.
    ///
    /// Written the way the reader would write it -- `.obelus/config.toml`,
    /// not the whole path -- because it is a file in the project they are
    /// looking at.
    pinned: Vec<&'static str>,
    /// Which settings the reader's own file named.
    named: Vec<&'static str>,
    /// That file, if there is one.
    project: Option<String>,
}

impl<'a> SettingsView<'a> {
    /// Borrows what the view needs, or nothing if the settings are not open.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            settings: app.settings()?,
            config: app.config(),
            theme: app.theme(),
            agents: app.listed_agents(),
            failure: app.registry_failure(),
            images: app.images(),
            keys: app.settings()?.keys(app.keymap()),
            offering: app.agent_offering(),
            pinned: app.pinned().to_vec(),
            named: app.readers_named().to_vec(),
            // The file the project has, or the one it would get: the project's
            // page says which file it is writing before there is a file to
            // write, because that is the question a reader opening it has.
            project: (app.settings().is_some_and(Settings::on_project)
                || app.project_config().is_some())
            .then(|| {
                crate::relative_to(
                    &obelus_config::project_path_for(app.working_directory()),
                    app.working_directory(),
                )
                .display()
                .to_string()
            }),
        })
    }
}

/// How many rows the page gives up to the keys at its foot.
#[must_use]
pub fn footed(area: Rect, settings: &Settings, offering: Option<&Offering>) -> Rect {
    crate::footed(area, &hints(settings, offering))
}

/// What the keys do here, and which of them do anything at the moment.
///
/// Not the arrows: they walk the tabs and say so on the tab row, where the
/// key *is* the arrow. What is here is what a reader could not guess -- that
/// this page is filtered by typing at it, and that a setting the project has
/// set can be unset.
#[must_use]
pub fn hints(settings: &Settings, offering: Option<&Offering>) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let bare = |code| obelus_editing::keymap::KeyChord::new(code, KeyModifiers::NONE);
    // Which row the reader is on, where that changes what a key does. The
    // agent's rows have a `delete` of their own and Obelus's do not.
    let focused = settings
        .rows(offering)
        .get(settings.focus())
        .copied()
        .filter(|shown| {
            matches!(
                shown,
                Shown::Agent {
                    chosen: Some(_),
                    ..
                }
            )
        });
    vec![
        Hint::common(bare(KeyCode::Enter), "Change")
            .saying(match settings.on_keys() {
                true => "Put this command on another key",
                false => "Change it, or open what it can be",
            })
            // Asked of whichever page is showing rather than of the
            // settings: the keys page has rows too, and enter does the same
            // sort of thing to one of them.
            .when(settings.row_count(offering) > 0),
        // On an agent's row, and only while there is something to undo:
        // what `delete` leaves there is not a default of Obelus's but the
        // agent's own answer.
        Hint::common(bare(KeyCode::Delete), AGENTS_OWN)
            .saying("Stop saying what this one starts on, and leave it to the agent")
            .when(focused.is_some()),
        // The one thing on this page nothing else says: a page that is
        // filtered by typing at it looks exactly like one that is not.
        Hint::common(bare(KeyCode::Char('a')), "to filter")
            .written("type")
            .saying("Type to narrow the list"),
        Hint::common(bare(KeyCode::Delete), "Unset")
            .saying("Take this setting out of the project's file")
            .when(settings.on_project() && !settings.on_keys() && !settings.on_agents()),
        // The card's, not the foot's: see `crate::foot`.
        Hint::rare(bare(KeyCode::Esc), "Leave").saying("Leave the settings"),
    ]
}

/// Where one row of the page is drawn.
struct Placed {
    /// Which of the rows it is.
    at: usize,
    /// The rows it takes: its name, and what its description needs.
    area: Rect,
}

/// The region the rows are drawn in: under the tabs, above the foot.
///
/// Named so that a pointer can ask where the rows are rather than working
/// it out again from the two things that take room off the page.
#[must_use]
pub fn rows_region(area: Rect, settings: &Settings, offering: Option<&Offering>) -> Rect {
    let under = crate::footed(area, &hints(settings, offering));
    Rect {
        y: under.y + 2,
        height: under.height.saturating_sub(2),
        ..under
    }
}

/// Where each row of the page is drawn, walked once.
///
/// An entry is as tall as what it has to say -- a name, the rows its
/// description takes, and a blank so that the next name is not read as part
/// of it -- and a group's heading takes two more above the first row in it.
/// So which row is at a given height is a walk rather than a division, and
/// a walk written twice is two answers about where a row is. The drawing
/// goes down this and so does the pointer.
fn placed(region: Rect, rows: &[Row], window: &obelus_component::window::Window) -> Vec<Placed> {
    let mut placed = Vec::new();
    let mut y = region.y;
    for (at, row) in rows
        .iter()
        .enumerate()
        .skip(window.top().min(window.focus()))
    {
        if y >= region.bottom() {
            break;
        }
        if let Some(heading) = &row.opens {
            y += heading.rows();
            if y >= region.bottom() {
                break;
            }
        }
        // A row with no name is its prose and nothing else: there is
        // nothing to put on a first line. Counted the same way the
        // component counts it, because the walk down the page and the
        // window deciding what is on screen must not disagree about where
        // a row ends.
        let tall = u16::try_from(row.body.len()).unwrap_or(0) + u16::from(!row.label.is_empty());
        placed.push(Placed {
            at,
            area: Rect {
                y,
                height: tall.min(region.bottom().saturating_sub(y)),
                ..region
            },
        });
        y += tall + u16::from(!row.body.is_empty());
    }
    placed
}

impl SettingsView<'_> {
    /// Which row of the page a point on screen is on, and whether it is on
    /// that row's switch.
    ///
    /// A method rather than a function beside the others, because how tall
    /// a row is depends on the rows the *view* makes: a description is
    /// wrapped to the room there is, and the wrapping is this view's own
    /// arithmetic. Whoever wants to know where a press landed builds the
    /// view and asks it, the way the conversation is asked.
    ///
    /// `None` for a point outside the rows, on a group's heading, or past
    /// the last of them. The switch is the right-hand column, which is
    /// where a row draws what it is set to.
    #[must_use]
    pub fn row_at(&self, area: Rect, x: u16, y: u16) -> Option<(usize, bool)> {
        let region = rows_region(area, self.settings, self.offering.as_ref());
        if x < region.x || x >= region.right() {
            return None;
        }
        let rows = self.rows(region);
        let window = self.settings.window();
        let found = placed(region, &rows, window)
            .into_iter()
            .find(|placed| y >= placed.area.y && y < placed.area.y + placed.area.height)?;
        let room = match window.scrollable(region.height) {
            true => region.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH),
            false => region.width,
        };
        let aside = region.x + room.saturating_sub(CONTROL_WIDTH + 1);
        Some((found.at, x >= aside && x < region.x + room))
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
        // Where the tabs end, which is what says whether anything else
        // fits on this row.
        let after = crate::tabs(
            cells,
            area,
            &self.settings.tabs(),
            self.settings.tab(),
            self.theme,
        );
        // Which file a change on this page is written to, on the tab row
        // and kept there: it is the whole of what makes this page different
        // from the other one, and a reader who cannot see it is a reader
        // editing something they have to remember.
        if let Some(project) = self
            .settings
            .on_project()
            .then_some(self.project.as_deref())
            .flatten()
        {
            // Past the arrows that walk the tabs, which sit at the edge --
            // and only where the tabs themselves have left room for it.
            // Fitting on the row is not the question: a name that fits and
            // starts before the tabs end is a name written over them, which
            // is what a narrow screen got.
            let arrows = 4;
            let width = u16::try_from(obelus_text::text_width(project)).unwrap_or(0);
            let at = area.right().saturating_sub(width + arrows + 2);
            if at > after + 1 {
                write(
                    cells,
                    at,
                    area.y,
                    project,
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

        let hints = hints(self.settings, self.offering.as_ref());
        crate::foot(cells, area, &hints, self.theme);
        let under = crate::footed(area, &hints);

        let region = Rect {
            y: under.y + 2,
            height: under.height.saturating_sub(2),
            ..under
        };

        // The agents are a page of cards rather than a column of controls:
        // a reader choosing between forty programs is reading about them,
        // and a row of a table has nowhere to say what one is.
        if self.settings.on_agents() {
            self.agents(cells, region);
            self.keys_card(cells, area, &hints);
            return;
        }

        // The keys are a column of the same rows: a command, what it does,
        // and the key it is on -- with the row the reader is binding saying
        // so where its description was.
        if self.settings.on_keys() {
            let rows: Vec<Row> = self
                .keys
                .iter()
                .map(|(command, chord)| Row {
                    opens: None,
                    label: command.name().to_string(),
                    matched: self.settings.matched_in(command.name()),
                    detail: self.saying(*command),
                    aside: Aside::Words(chord.map(|chord| chord.label()).unwrap_or_default()),
                    body: Vec::new(),
                    pinned: None,
                    scope: None,
                })
                .collect();
            self.column(cells, region, &rows, "No command by that name");
            self.keys_card(cells, area, &hints);
            return;
        }

        let rows = self.rows(region);
        self.column(cells, region, &rows, "No setting by that name");
        self.keys_card(cells, area, &hints);
    }
}

impl SettingsView<'_> {
    /// The rows of the settings page, as the drawing and the pointer both
    /// need them.
    ///
    /// Made here rather than in the component because how tall one is
    /// belongs to the drawing: a description is wrapped to the room there
    /// is, and the number of rows that takes is what says where the next
    /// name sits.
    fn rows(&self, region: Rect) -> Vec<Row> {
        let width = obelus_component::settings::description_width(region.width);
        self.settings
            .rows(self.offering.as_ref())
            .iter()
            .map(|shown| match shown {
                Shown::Obelus { setting, opens } => Row {
                    opens: opens.map(Heading::Group),
                    label: setting.name.to_string(),
                    matched: self.settings.matched(setting),
                    detail: None,
                    // What it does, on its own rows under the name: beside
                    // it, the two were competing for one row -- and the one
                    // that lost was the description, cut off with an
                    // ellipsis on exactly the rows that had most to
                    // explain.
                    body: self.settings.wrapped(setting.about, width),
                    aside: Aside::Control(setting.kind, Settings::value_of(setting, self.config)),
                    // On the reader's page, the file that has this one
                    // instead of them. On the project's, nothing: a setting
                    // the project has is exactly what that page is for.
                    pinned: (!self.settings.on_project())
                        .then(|| {
                            self.pinned
                                .contains(&setting.key)
                                .then(|| self.project.clone())
                                .flatten()
                        })
                        .flatten(),
                    // And on the project's page, which layer the value showing
                    // comes from -- the project's own included.
                    scope: self.settings.on_project().then(|| self.scope(setting)),
                },
                Shown::Agent {
                    offer,
                    chosen,
                    opens,
                } => {
                    // Three answers, and the word on the row is a different
                    // one in each: what they chose, what the agent is left
                    // to decide, and a choice the agent has since stopped
                    // offering -- which is a line in their settings file
                    // that will do nothing.
                    let (word, said) = match chosen {
                        None => (AGENTS_OWN.to_string(), Said::Agents),
                        Some(value) => match offer.name_of(value) {
                            Some(name) => (name.to_string(), Said::Reader),
                            None => ((*value).to_string(), Said::Gone),
                        },
                    };
                    Row {
                        opens: opens.map(|name| Heading::Agent(name.to_string())),
                        label: offer.name.clone(),
                        matched: self.settings.matched_in(&offer.name),
                        detail: None,
                        body: self
                            .settings
                            .wrapped(offer.about.as_deref().unwrap_or_default(), width),
                        aside: Aside::Chosen(word, said),
                        // Neither column is this group's: what an agent
                        // starts on is the reader's alone, so no project can
                        // have taken it and there is no layer to name.
                        pinned: None,
                        scope: None,
                    }
                }
                Shown::Silent { saying, opens } => Row {
                    opens: Some(Heading::Agent((*opens).to_string())),
                    label: String::new(),
                    matched: None,
                    detail: None,
                    body: self.settings.wrapped(saying, width),
                    aside: Aside::Nothing,
                    pinned: None,
                    scope: None,
                },
            })
            .collect()
    }
}

/// One row of a page: what it is called, which of its characters the query
/// matched, what it is, and what sits on the right.
struct Row {
    /// The heading this row opens, where it is the first of its group.
    ///
    /// Part of the row rather than a row of its own, so the focus never
    /// lands on it and `down` means one distance: an entry is its heading,
    /// its name, what it does, and the blank after.
    opens: Option<Heading>,
    label: String,
    matched: Option<std::ops::Range<usize>>,
    detail: Option<(String, ratatui::style::Color)>,
    aside: Aside,
    /// What it does, under the name and indented, already broken into the
    /// rows it takes. Empty on a page whose rows are one row each.
    body: Vec<String>,
    /// Which layer the value showing comes from, on the project's page.
    ///
    /// `None` on the reader's, where the question is the other one: not
    /// "whose is this" but "who has taken it from me", which the file's
    /// name and a lock answer.
    scope: Option<Scope>,
    /// The file that has this one, when it is not the reader's to change.
    ///
    /// Named on the row rather than said when the reader tries to move it:
    /// a row that answers only when pushed is a row that looks like every
    /// other until it is, and what a reader wants to know here is which of
    /// these are theirs.
    pinned: Option<String>,
}

/// A heading on the settings page: one of Obelus's groups, or the agent.
///
/// Two, because they are not the same height. The agent's carries a line
/// saying that what is under it is about the next conversation, which is
/// the one thing true of that group and no other -- and a heading that is
/// sometimes two rows and sometimes three has to say which it is before
/// anything is laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Heading {
    /// One of Obelus's own groups.
    Group(obelus_config::Group),
    /// The active agent, by the name it goes by.
    Agent(String),
}

impl Heading {
    /// How many rows it takes, the blank under it included.
    const fn rows(&self) -> u16 {
        match self {
            Self::Group(_) => 2,
            Self::Agent(_) => HEADING_ROWS,
        }
    }
}

/// Where the word in an agent row's control came from.
///
/// Three, because a row that is not set is not a row set to something: an
/// agent's setting the reader has said nothing about is in the agent's
/// hands, which is a decision and not a blank. And a value the agent has
/// stopped offering is neither -- it is a line in the settings file that
/// will do nothing, and the only way to find that out is to be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Said {
    /// The reader chose it, and the agent still offers it.
    Reader,
    /// They have said nothing: whatever the agent opens on stands.
    Agents,
    /// They chose it and the agent does not offer it any more.
    Gone,
}

/// Which of the three layers a value comes from.
///
/// The reader's own is `global`, which is the word `git config` has taught
/// everybody who works in a repository, and is less slippery than "yours" on
/// a page where everything is in some sense theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// This project's own settings file.
    Project,
    /// The reader's, wherever this system keeps them.
    Global,
    /// Nobody's: what Obelus ships with.
    Default,
}

impl Scope {
    /// The word for it.
    const fn word(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Global => "Global",
            Self::Default => "Default",
        }
    }
}

/// What a row shows on the right.
enum Aside {
    /// What one of the agent's settings is to start on, and where that
    /// word came from.
    ///
    /// Always a word and an arrow, a switch included: every one of these
    /// has a third answer -- leave it to the agent -- and a tick has two
    /// sides to say it with. A tick that meant "off" and "Obelus says
    /// nothing" by turns would be a control that cannot be read.
    Chosen(String, Said),
    /// Nothing at all, on a row that is prose rather than a setting.
    Nothing,
    /// A setting's control: a switch, or the word it is set to.
    Control(Kind, Value),
    /// Words -- the key a command is on, and nothing when it is on none.
    Words(String),
}

impl SettingsView<'_> {
    /// Every key this page answers to, where the reader asked for them.
    ///
    /// Above the foot, which says how to close it: a card that covered the
    /// way out would be a card with no way out on screen.
    fn keys_card(&self, cells: &mut CellBuffer, area: Rect, hints: &[Hint]) {
        if self.settings.showing_keys() {
            crate::keys_card(cells, crate::footed(area, hints), hints, self.theme);
        }
    }

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
            crate::nothing(
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
            crate::scrollbar(cells, region, window.top(), rows.len(), self.theme);
        }
        let room = match scrolling {
            true => region.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH),
            false => region.width,
        };
        let aside_at = region.x + room.saturating_sub(CONTROL_WIDTH + 1);
        // Where a name starts: in from the group's heading on the settings
        // page, and hard against the edge on the keys page, which has no
        // groups to be in.
        let name_at = region.x + 1 + u16::from(!self.settings.on_keys()) * GROUP_INDENT;
        // Where what a row *does* starts, measured over the whole page
        // rather than taken from each name: a column of names is read down,
        // and prose that started at a different column on every row would
        // be four beginnings to find rather than one.
        let names = rows
            .iter()
            .filter(|row| row.detail.is_some())
            .map(|row| obelus_text::text_width(&row.label))
            .max()
            .unwrap_or(0);
        let detail_at = name_at + u16::try_from(names).unwrap_or(0) + 2;
        // Walked by height rather than by row, because an entry is as tall
        // as what it has to say: a name, the rows its description takes, and
        // a blank so that the next name is not read as part of it. Every
        // height here is one on the pages whose rows are one row each, which
        // is the same walk it always was.
        //
        // Walked once, in `placed`, because a pointer has to land on the
        // row it looks like it landed on: the drawing going one way and the
        // pointing going the other down two copies of this would be two
        // answers about where a row is.
        for at in placed(region, rows, window) {
            let (index, row) = (at.at, &rows[at.at]);
            let y = at.area.y;
            let focused = index == self.settings.focus();
            let background = if focused {
                self.theme.selected_row_background
            } else {
                self.theme.background
            };
            // The background covers the name and what it says, but not the
            // blank under them: a run of colour that reached into the gap
            // would close it up again.
            // The heading above it, where this row opens a group. Outside
            // the entry's own background, because it belongs to the group
            // and not to the setting that happens to come first in it: a
            // heading wearing the selected row's colour would read as part
            // of the row under it.
            if let Some(opens) = row.opens.as_ref() {
                self.heading(
                    cells,
                    Rect {
                        y: y.saturating_sub(opens.rows()),
                        height: opens.rows(),
                        // Short of the bar, the same as the rows under it:
                        // the column a bar is in is the bar's for the whole
                        // page, and a heading that filled the region blanked
                        // the block in its own rows -- so the page's bar came
                        // out with a gap at every group, and in the window,
                        // where a bar is a shape and the blocks are a bar
                        // saying where it is, it stopped being drawn as one.
                        width: room,
                        ..region
                    },
                    opens,
                );
            }
            let area = Rect {
                width: room,
                ..at.area
            };
            fill(cells, area, plain.bg(background));
            let area = Rect { height: 1, ..area };

            // What the row says about where it came from, measured before
            // the name is: it is written between the two, so the name is cut
            // to what is left rather than to the whole row.
            // The lock is the reader's page saying "not here". The project's
            // page has none: there, everything is here.
            let lock = u16::from(obelus_icons::enabled() && row.pinned.is_some()) * 2;
            // Where the value showing comes from, on whichever page is not
            // the one that has it: the file's name and a lock on the
            // reader's, the word `yours` or `default` on the project's. One
            // column, because it is one question.
            let source = row
                .pinned
                .as_deref()
                .or(row.scope.map(Scope::word))
                .map(|source| {
                    let room = aside_at.saturating_sub(name_at + 3 + lock);
                    truncate_from_right(source, usize::from(room))
                });
            let reserved = source.as_deref().map_or(0, |source| {
                u16::try_from(obelus_text::text_width(source)).unwrap_or(0) + lock + 1
            });

            // Cut to what is left before the right-hand column: a line
            // running under it reads as part of it.
            let width = aside_at.saturating_sub(name_at + 1 + reserved);
            let label = truncate_from_right(&row.label, usize::from(width));
            // Through the shared writer, so the characters the query
            // matched carry the background every other list marks a match
            // with: a row in a narrowed list has to say why it is in it.
            // The ink says whether a row can be used, which is the rule
            // everywhere here: a setting the project has is not this reader's
            // to move, and a row that looked live until they pressed it
            // would be a row that lied.
            // Dim says "not yours to use here", which on the reader's page
            // is exactly what a setting the project has taken is. On the
            // project's page it would be the opposite of the truth: a row the
            // project has not got is the one thing on that page a reader *can*
            // do something to -- pressing it is how a setting becomes the
            // project's. So there the row is ordinary, and what is dim is
            // the word saying where the value showing comes from, and the
            // control showing it.
            let ink = match row.pinned {
                Some(_) => plain.fg(self.theme.gutter),
                None => plain,
            };
            // The word saying which layer a value comes from is dim where
            // that layer is not this page's, and ordinary where it is: the
            // column is then read down for what this project has decided.
            let said = match row.scope {
                Some(Scope::Project) | None => plain,
                Some(_) => plain.fg(self.theme.gutter),
            };
            let after = write_marked(
                cells,
                area,
                name_at,
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
                    &truncate_from_right(detail, usize::from(left)),
                    plain.fg(*colour).bg(background),
                );
            }
            // Where it came from, in the space the reader would otherwise
            // reach across, and a lock against the control itself: the name
            // says which file has it and the lock says it is shut, which
            // between them is the whole answer without a word of prose.
            if let Some(source) = &source {
                let width = u16::try_from(obelus_text::text_width(source)).unwrap_or(0);
                write(
                    cells,
                    aside_at.saturating_sub(width + lock + 1),
                    y,
                    source,
                    said.fg(if row.pinned.is_some() {
                        self.theme.gutter
                    } else {
                        said.fg.unwrap_or(self.theme.foreground)
                    })
                    .bg(background),
                );
                if obelus_icons::enabled() && row.pinned.is_some() {
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
                    row.pinned.is_none() && row.scope != Some(Scope::Project),
                ),
                Aside::Words(words) => {
                    let keys = truncate_from_right(words, usize::from(CONTROL_WIDTH));
                    write(
                        cells,
                        aside_at,
                        y,
                        &keys,
                        plain.fg(self.theme.foreground).bg(background),
                    );
                    // The row's own ground either side, whichever row it is:
                    // a cap on the row the reader is on is drawn over the
                    // colour that says so.
                    crate::cap_around(
                        aside_at,
                        y,
                        &keys,
                        text_width(&keys),
                        background,
                        background,
                        self.theme.gutter,
                    );
                }
                Aside::Chosen(word, said) => {
                    // The reader's choice in the ordinary ink; the agent's
                    // own in the dim one, which everywhere here means "not
                    // Obelus's doing"; and one the agent has stopped
                    // offering in the colour a card's failure is in,
                    // because it is a line that will do nothing and the
                    // reader is the only one who can fix it.
                    let ink = match said {
                        Said::Reader => self.theme.foreground,
                        Said::Agents => self.theme.gutter,
                        Said::Gone => self.theme.change_removed,
                    };
                    let after = write(
                        cells,
                        aside_at,
                        y,
                        &truncate_from_right(word, usize::from(CONTROL_WIDTH)),
                        plain.fg(ink).bg(background),
                    );
                    // Pointing right, at the value, the way every other
                    // list on this page does.
                    put(
                        cells,
                        arrow_at(aside_at, after),
                        y,
                        '\u{25b8}',
                        plain.fg(self.theme.gutter).bg(background),
                    );
                }
                // A row that is prose has nothing on the right: there is
                // nothing to set.
                Aside::Nothing => {}
            }

            // What it does, under its name and indented under it, in the
            // dim colour: it is the answer to a question the name has
            // already asked, so it is read after the name or not at all.
            //
            // A row with no name is prose and nothing else -- the reason
            // an agent's group has no rows -- so it starts on the row
            // itself, and at the column the names are in: it stands in for
            // them rather than explaining one.
            let named = usize::from(!row.label.is_empty());
            let indent = match named {
                0 => 0,
                _ => DESCRIPTION_INDENT - 1,
            };
            for (offset, line) in row.body.iter().enumerate() {
                let Ok(offset) = u16::try_from(offset + named) else {
                    break;
                };
                if y + offset >= region.bottom() {
                    break;
                }
                write(
                    cells,
                    name_at + indent,
                    y + offset,
                    line,
                    plain.fg(self.theme.gutter).bg(background),
                );
            }

            // And a blank before the next one, which is the whole of what
            // makes an entry an entry.
        }
    }

    /// A group's name, and the blank that sets it off from its settings.
    ///
    /// The word and nothing else. What says where one group ends and the
    /// next begins is that a group's settings are indented under its name --
    /// the way a description is indented under the setting it is about, and
    /// the way a file is indented under its directory in the counts. So
    /// there is no rule to draw and no second colour to hold: a page whose
    /// groups were told apart by a line across it would have six lines on
    /// it, counting the tabs' and the foot's, and the lines would be the
    /// loudest thing on a page of words.
    fn heading(&self, cells: &mut CellBuffer, area: Rect, opens: &Heading) {
        let plain = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        fill(cells, area, plain);
        let name = match opens {
            Heading::Group(group) => group.label(),
            Heading::Agent(name) => name.as_str(),
        };
        write(
            cells,
            area.x + 1,
            area.y,
            &truncate_from_right(name, usize::from(area.width.saturating_sub(2))),
            plain.fg(self.theme.status_foreground),
        );
        // And under the agent's name, when what is under it takes effect.
        // The group is the only one on this page whose settings are about
        // somewhere else, and a reader who changes one and goes back to a
        // conversation that has not moved has been told nothing.
        if matches!(opens, Heading::Agent(_)) && area.height > 1 {
            write(
                cells,
                area.x + 1,
                area.y + 1,
                &truncate_from_right(WHEN, usize::from(area.width.saturating_sub(2))),
                plain.fg(self.theme.gutter),
            );
        }
    }

    /// Which layer the value on a row comes from, on the project's page.
    ///
    /// All three, including the project's own: a column where two of the three
    /// have a word and the third is blank asks the reader to read an
    /// absence. `git config --global` against a repository's own is the
    /// vocabulary they already have for this.
    fn scope(&self, setting: &obelus_config::Setting) -> Scope {
        if self.pinned.contains(&setting.key) {
            return Scope::Project;
        }
        // Whether their file speaks about it, which is the same question
        // the line above asks of the project's. Whether what it says differs
        // from the default is a different question and the wrong one: a
        // reader who wrote a setting down and happened to agree with Obelus
        // would be told they had never been here.
        match self.named.contains(&setting.key) {
            true => Scope::Global,
            false => Scope::Default,
        }
    }

    /// What a command's row says beside its name.    /// What a command's row
    /// says beside its name.
    ///
    /// What it does, unless the reader is binding it: then it is what the
    /// page is waiting for, or why the key they pressed will not do. On the
    /// row because that is where they are looking and it is that binding
    /// the answer is about -- the status row here is the page's filter, and
    /// a passing note would be cleared by the very next keystroke.
    fn saying(&self, command: obelus_command::Command) -> Option<(String, ratatui::style::Color)> {
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
                "Press a key, or delete to unbind".to_string(),
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
                (false, _) => "No agent by that name".to_string(),
            };
            write(
                cells,
                area.x + INDENT,
                area.y,
                &truncate_from_right(&reason, usize::from(area.width.saturating_sub(INDENT * 2))),
                dim,
            );
            return;
        }

        // Which card is at the top is the page's own, moved only when the
        // focus leaves the window -- so a step that is not at an edge
        // scrolls nothing. Heights here are for laying the cards out, not
        // for deciding where the window is.
        let width = Settings::card_width(area.width);
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
            obelus_agent::Status::Failed(why) => Some(why.as_str()),
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
        if obelus_icons::enabled() {
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
                    obelus_icons::for_agent(&agent.agent.id),
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
            &truncate_from_right(&agent.agent.name, usize::from(room)),
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
                &truncate_from_right(&self.facts(agent), usize::from(inner.saturating_sub(3))),
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
                &truncate_from_right(
                    &format!("\u{f0159} {why}"),
                    usize::from(inner.saturating_sub(3)),
                ),
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
            obelus_agent::Status::Outdated { installed } => {
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
        use obelus_agent::Status;
        match &agent.status {
            Status::Installed if agent.active => {
                ("\u{25cf} active".to_string(), self.theme.change_added)
            }
            Status::Installed => ("Installed".to_string(), self.theme.gutter),
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
    // all: what it is set to is still worth seeing -- it is what this project
    // has decided -- and what they cannot do about it is said the way
    // everything unusable here says it.
    let ink = if usable {
        theme.foreground
    } else {
        theme.gutter
    };
    match (kind, value) {
        // A tick, drawn by the one thing that draws ticks: the foot of a
        // list has them, a card has them, and a note has one in front of
        // it -- a reader who has learnt this shape in one of those should
        // not have to learn a second one here.
        (Kind::Switch, Value::Switch(on)) => {
            crate::ticked(cells, x, y, *on, style.fg(ink));
        }
        (Kind::Count(_), Value::Count(count)) => {
            let after = write(cells, x, y, &count.to_string(), style.fg(ink));
            put(
                cells,
                arrow_at(x, after),
                y,
                '\u{25b8}',
                style.fg(theme.gutter),
            );
        }
        (Kind::Names, Value::Names(names)) => {
            // Joined the way the file writes them, which is the way a
            // reader would say them out loud: the order is the answer, so
            // it is drawn in order and nothing is sorted.
            let said = match names.is_empty() {
                // Not "none": what happens with an empty list is that the
                // machine's own face is used, and a row that said
                // "nothing" would be a row saying no text is drawn.
                true => "The machine's own".to_string(),
                false => names.join(", "),
            };
            let after = write(
                cells,
                x,
                y,
                &truncate_from_right(&said, usize::from(CONTROL_WIDTH)),
                style.fg(ink),
            );
            put(
                cells,
                arrow_at(x, after),
                y,
                '\u{25b8}',
                style.fg(theme.gutter),
            );
        }
        (Kind::Choice(_), Value::Choice(word)) => {
            let after = write(
                cells,
                x,
                y,
                &truncate_from_right(word, usize::from(CONTROL_WIDTH)),
                style.fg(ink),
            );
            // Pointing right, at the value: the list it opens is the
            // ordinary compact one and comes up wherever that comes up, so
            // an arrow pointing down would be pointing at whatever happens
            // to be under this row.
            put(
                cells,
                arrow_at(x, after),
                y,
                '\u{25b8}',
                style.fg(theme.gutter),
            );
        }
        (kind, value) => {
            tracing::debug!(?kind, ?value, "a control with nothing to draw");
        }
    }
}
