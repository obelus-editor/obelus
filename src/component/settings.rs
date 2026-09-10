//! The settings view: a tab per group, a row per setting, a control per row.
//!
//! It holds no settings of its own. What there is to show comes from
//! [`config::ALL`](crate::config::ALL) and what each row says comes from the
//! [`Config`] it is handed, so a setting added to that table appears here
//! with nothing changed in this file.
//!
//! Changing anything produces [`SettingsOutcome::Changed`] rather than
//! writing to the config: this is a view, and what a change means -- apply
//! it, save the file, say why it could not be saved -- belongs to the
//! application.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    agent::Status,
    app::agents::Listed as Agent,
    config::{self, Config, Group, Kind, Setting, Value},
};

/// What a key did to the settings view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// The key means nothing here; let the key table have it.
    Ignored,
    /// Something moved or opened. Redraw.
    Consumed,
    /// A setting was changed, and this is what to.
    Changed(&'static str, Value),
    /// An agent should be installed.
    Install(String),
    /// An agent should be the one obelus talks to.
    Activate(String),
    /// Whichever agent was active should stop being.
    Deactivate,
    /// A setting with choices wants its list: which setting, what the
    /// choices are, and which one it is set to now.
    ///
    /// The list itself is the ordinary picker, opened by the application
    /// over this view. This one asks; it does not draw a list of its own.
    Choose(&'static str, &'static [&'static str], String),
    /// The reader is done with the view.
    Cancelled,
}

/// How many rows of a description a card will show.
///
/// Three: enough for the longest in the registry, and a limit so that one
/// verbose entry cannot push every other card off the screen.
const MOST_DESCRIPTION_ROWS: usize = 3;

/// The settings view.
#[derive(Debug)]
pub struct Settings {
    /// What has been typed, which narrows the rows.
    query: String,
    /// Which group's tab is showing.
    group: usize,
    /// Which of the showing rows has the focus.
    focus: usize,
    /// Which card the agents page draws first.
    ///
    /// Kept rather than worked out from the focus each frame. Worked out,
    /// the focused card ends up as low on the screen as it will go, so
    /// every step *up* scrolls -- and a list that moves under a reader who
    /// has not reached its edge is a list they have to watch instead of
    /// read. Only the cards need it: a group of settings is a dozen rows
    /// and they all fit.
    top: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// A view on the first group, with nothing typed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            query: String::new(),
            group: 0,
            focus: 0,
            top: 0,
        }
    }

    /// The tab names, in order.
    ///
    /// The groups of settings, and then the agents -- which is a page of a
    /// different shape rather than a group of rows, because what it lists
    /// comes from a registry over the network and changes while it is being
    /// looked at.
    #[must_use]
    pub fn tabs() -> Vec<&'static str> {
        let mut tabs: Vec<&'static str> = Group::ALL.iter().map(|group| group.label()).collect();
        tabs.push("agents");
        tabs
    }

    /// Whether the page showing is the agents rather than settings.
    #[must_use]
    pub fn on_agents(&self) -> bool {
        self.group >= Group::ALL.len()
    }

    /// Which tab is showing.
    #[must_use]
    pub const fn tab(&self) -> usize {
        self.group
    }

    /// What has been typed.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Which row has the focus.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Which card the agents page draws first.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    /// The settings on show: this group's, narrowed by what has been typed.
    ///
    /// Plainly by substring rather than fuzzily: there are a dozen of these
    /// and a reader typing "the" means the word, where a fuzzy match would
    /// also offer everything with a t, an h and an e scattered through it.
    #[must_use]
    pub fn rows(&self) -> Vec<&'static Setting> {
        if self.on_agents() {
            return Vec::new();
        }
        let group = Group::ALL.get(self.group).copied();
        let query = self.query.to_lowercase();
        config::ALL
            .iter()
            .filter(|setting| group.is_some_and(|group| setting.group == group))
            .filter(|setting| {
                query.is_empty()
                    || setting.label.to_lowercase().contains(&query)
                    || setting.key.contains(&query)
            })
            .collect()
    }

    /// The agents on show: all of them, narrowed by what has been typed.
    ///
    /// By name only. A description is a sentence, and matching sentences
    /// puts rows in the list for a word buried in prose -- which the
    /// highlight then cannot point at, because the name is what the card
    /// shows first.
    #[must_use]
    pub fn agents(&self, agents: &[Agent]) -> Vec<Agent> {
        if self.query.is_empty() {
            return agents.to_vec();
        }
        agents
            .iter()
            .filter(|listed| self.matched_in(&listed.agent.name).is_some())
            .cloned()
            .collect()
    }

    /// How many rows a card takes.
    ///
    /// Its name, its description wrapped to the room there is, who wrote
    /// it, and why the last install failed if one did. Variable, because a
    /// description is a sentence and cutting it at the width loses the half
    /// that says what the thing is *for*.
    #[must_use]
    pub fn card_rows(&self, agent: &Agent, width: u16) -> u16 {
        let failed = u16::from(matches!(agent.status, Status::Failed(_)));
        2 + self.wrapped(&agent.agent.description, width).len().max(1) as u16 + failed
    }

    /// A sentence, broken into the rows it takes.
    ///
    /// Through the same word-breaking the editor uses -- a description is
    /// prose, and prose broken at the column is prose nobody reads twice.
    #[must_use]
    pub fn wrapped(&self, sentence: &str, width: u16) -> Vec<String> {
        if sentence.is_empty() {
            return Vec::new();
        }
        let text = crate::text::Text::from_string(sentence);
        let line = crate::coordinates::LineNumber::new(0);
        text.wrap_rows(line, width.max(8))
            .into_iter()
            .map(|row| {
                text.line(line)
                    .chars()
                    .take(row.end.get())
                    .skip(row.first.get())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .take(MOST_DESCRIPTION_ROWS)
            .collect()
    }

    /// How many cards fit in the room the page has.
    ///
    /// Counted from the focused card outwards, because that is the one that
    /// has to be whole -- and cards are not all the same height, so there
    /// is no number to divide by.
    #[must_use]
    pub fn cards_that_fit(&self, agents: &[Agent], room: (u16, u16)) -> usize {
        let width = room.0.saturating_sub(7);
        let height = room.1.saturating_sub(2);
        let listed = self.agents(agents);
        let mut taken = 0;
        let mut fits = 0;
        for agent in listed.iter().skip(self.focus.min(listed.len())) {
            let card = self.card_rows(agent, width) + 1;
            if taken + card > height && fits > 0 {
                break;
            }
            taken += card;
            fits += 1;
        }
        fits
    }

    /// Where what has been typed matched in a name, in characters.
    ///
    /// The same rule the settings rows follow -- a plain substring, so a
    /// match is one run -- and the run is what the view marks.
    #[must_use]
    pub fn matched_in(&self, text: &str) -> Option<std::ops::Range<usize>> {
        if self.query.is_empty() {
            return None;
        }
        let query: Vec<char> = self.query.to_lowercase().chars().collect();
        let text: Vec<char> = text.to_lowercase().chars().collect();
        if query.len() > text.len() {
            return None;
        }
        text.windows(query.len())
            .position(|window| window == query.as_slice())
            .map(|at| at..at + query.len())
    }

    /// Where what has been typed matched in a row's label, in characters.
    ///
    /// The filter is a plain substring, so a match is one run of them -- and
    /// the run is what the view colours. Without it a row in a narrowed list
    /// leaves the reader working out why it is there, which is the question
    /// the highlight answers everywhere else in obelus.
    ///
    /// `None` for a row that matched on its key rather than on its label,
    /// and for no query at all: there is nothing on the row to point at.
    #[must_use]
    pub fn matched(&self, setting: &Setting) -> Option<std::ops::Range<usize>> {
        self.matched_in(setting.label)
    }

    /// What a row's control shows, from the config it is handed.
    #[must_use]
    pub fn value_of(setting: &Setting, config: &Config) -> Value {
        config.value_of(setting.key).unwrap_or(match setting.kind {
            Kind::Switch => Value::Switch(false),
            Kind::Choice(choices) => Value::Choice(
                choices
                    .first()
                    .map(|first| (*first).to_string())
                    .unwrap_or_default(),
            ),
        })
    }

    /// Handles a key, given the settings as they stand and the agents on
    /// show.
    ///
    /// The agents are passed in rather than read here: what is installed
    /// and what is being installed are the application's to know, and a
    /// view that fetched them would be a second place they live.
    /// `room` is the region the page has, which is what a page of movement
    /// means: the rows a group of settings shows, or the cards that fit.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        config: &Config,
        agents: &[Agent],
        room: (u16, u16),
    ) -> SettingsOutcome {
        // The same rule every other view follows: a modifier obelus has no
        // meaning for disqualifies the key rather than being ignored.
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
            return SettingsOutcome::Ignored;
        };
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return SettingsOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let rows = self.rows();
        // How many things the focus can be on: the settings of this group,
        // or the agents that the query leaves.
        let count = if self.on_agents() {
            self.agents(agents).len()
        } else {
            rows.len()
        };

        // How far a page moves: the rows a group shows, or however many
        // cards fit -- which is not a fixed number, because a card is as
        // tall as its description needs.
        let page = if self.on_agents() {
            self.cards_that_fit(agents, room).max(1)
        } else {
            usize::from(room.1.saturating_sub(2)).max(1)
        };

        match key.code {
            KeyCode::Esc if bare => SettingsOutcome::Cancelled,
            // The ends, with and without control: the same keys reach the
            // ends of a document, a list and a rendering, and a key should
            // not mean one thing in one view and nothing in the next.
            KeyCode::Home if count > 0 => {
                self.focus = 0;
                SettingsOutcome::Consumed
            }
            KeyCode::End if count > 0 => {
                self.focus = count - 1;
                SettingsOutcome::Consumed
            }
            // Clamped rather than wrapped, unlike a single step: paging is
            // how you get to the end of a long list, and a page that wraps
            // past it overshoots what you were reaching for.
            KeyCode::PageDown if count > 0 => {
                self.focus = (self.focus + page).min(count - 1);
                SettingsOutcome::Consumed
            }
            KeyCode::PageUp if count > 0 => {
                self.focus = self.focus.saturating_sub(page);
                SettingsOutcome::Consumed
            }
            KeyCode::Down if bare && count > 0 => {
                self.focus = (self.focus + 1) % count;
                SettingsOutcome::Consumed
            }
            KeyCode::Up if bare && count > 0 => {
                self.focus = if self.focus == 0 {
                    count - 1
                } else {
                    self.focus - 1
                };
                SettingsOutcome::Consumed
            }
            // The arrows walk the tabs, as they do in every other view with
            // tabs on it -- which is why a switch is flipped with enter and
            // not by sliding it: one pair of keys with two jobs, decided by
            // whichever row happens to have the focus, is a pair of keys a
            // reader has to think about.
            KeyCode::Right if bare => {
                self.step_tab(true);
                SettingsOutcome::Consumed
            }
            KeyCode::Left if bare => {
                self.step_tab(false);
                SettingsOutcome::Consumed
            }
            // Enter and space open a list or flip a switch: both are what a
            // reader reaches for, and neither has another job here.
            // On the agents page, enter is whatever the card offers: a
            // button while there is one, and the choice of which agent to
            // talk to once there is something to talk to.
            KeyCode::Enter if bare && self.on_agents() => {
                match self.agents(agents).get(self.focus) {
                    Some(listed) => match &listed.status {
                        Status::Missing | Status::Failed(_) | Status::Outdated { .. } => {
                            SettingsOutcome::Install(listed.agent.id.clone())
                        }
                        Status::Installed if listed.active => SettingsOutcome::Deactivate,
                        Status::Installed => SettingsOutcome::Activate(listed.agent.id.clone()),
                        // Nothing to press while it is running, and nothing
                        // to press on one obelus cannot install.
                        Status::Installing | Status::Unavailable(_) => SettingsOutcome::Consumed,
                    },
                    None => SettingsOutcome::Consumed,
                }
            }
            KeyCode::Enter if bare => match rows.get(self.focus) {
                Some(setting) => match setting.kind {
                    Kind::Switch => {
                        let on = matches!(Self::value_of(setting, config), Value::Switch(true));
                        SettingsOutcome::Changed(setting.key, Value::Switch(!on))
                    }
                    Kind::Choice(choices) => {
                        let word = match Self::value_of(setting, config) {
                            Value::Choice(word) => word,
                            Value::Switch(_) => String::new(),
                        };
                        SettingsOutcome::Choose(setting.key, choices, word)
                    }
                },
                None => SettingsOutcome::Consumed,
            },
            KeyCode::Backspace if bare => {
                self.query.pop();
                self.settle();
                SettingsOutcome::Consumed
            }
            KeyCode::Char(character) => {
                self.query.push(character);
                self.settle();
                SettingsOutcome::Consumed
            }
            _ => SettingsOutcome::Ignored,
        }
    }

    /// Moves to the next tab, or the previous one, wrapping.
    fn step_tab(&mut self, forward: bool) {
        // Every tab, not every group: the agents are a tab and not a group,
        // and a walk that stopped at the groups could never reach them.
        let last = Self::tabs().len() - 1;
        self.group = match (forward, self.group) {
            (true, at) if at >= last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        self.settle();
    }

    /// Puts the focus back on a row that exists, after the rows change.
    ///
    /// And the window back to the top, because the rows a query leaves are
    /// not the rows the window was scrolled through.
    fn settle(&mut self) {
        let rows = self.rows().len();
        self.focus = self.focus.min(rows.saturating_sub(1));
        self.top = 0;
    }

    /// Moves the window of cards if the focused one has left it, and no
    /// further.
    ///
    /// The same rule the lists follow -- the least that puts the focus back
    /// on screen -- counted in rows rather than in cards, because a card is
    /// as tall as its description needs. So a step towards either edge
    /// moves nothing until the card at that edge is the focused one.
    /// Called once a frame with the room the page has, rather than from
    /// every key that moves the focus: the window depends on the geometry,
    /// and the geometry is only settled at that point -- which is also what
    /// makes a resize move the window rather than leave the focus off the
    /// screen.
    pub fn settle_cards(&mut self, agents: &[Agent], room: (u16, u16)) {
        let listed = self.agents(agents);
        if listed.is_empty() {
            self.top = 0;
            return;
        }
        self.focus = self.focus.min(listed.len() - 1);
        // Above the window, and when a narrowed list has left the window
        // past the end of it: either way the window comes to the focus.
        self.top = self.top.min(self.focus);

        let width = room.0.saturating_sub(7);
        let height = room.1.saturating_sub(2).max(1);
        let heights: Vec<u16> = listed
            .iter()
            .map(|agent| self.card_rows(agent, width) + 1)
            .collect();
        // Forward a card at a time until the focused card's last row is on
        // screen. A focused card taller than the whole page stops here with
        // itself at the top, which is the most of it that can be shown.
        while self.top < self.focus {
            let taken: u16 = heights[self.top..=self.focus].iter().sum();
            if taken <= height {
                break;
            }
            self.top += 1;
        }
    }
}
