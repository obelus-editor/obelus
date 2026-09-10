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

use crate::config::{self, Config, Group, Kind, Setting, Value};

/// What a key did to the settings view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// The key means nothing here; let the key table have it.
    Ignored,
    /// Something moved or opened. Redraw.
    Consumed,
    /// A setting was changed, and this is what to.
    Changed(&'static str, Value),
    /// A setting with choices wants its list: which setting, what the
    /// choices are, and which one it is set to now.
    ///
    /// The list itself is the ordinary picker, opened by the application
    /// over this view. This one asks; it does not draw a list of its own.
    Choose(&'static str, &'static [&'static str], String),
    /// The reader is done with the view.
    Cancelled,
}

/// The settings view.
#[derive(Debug)]
pub struct Settings {
    /// What has been typed, which narrows the rows.
    query: String,
    /// Which group's tab is showing.
    group: usize,
    /// Which of the showing rows has the focus.
    focus: usize,
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
        }
    }

    /// The tab names, in order.
    #[must_use]
    pub fn tabs() -> Vec<&'static str> {
        Group::ALL.iter().map(|group| group.label()).collect()
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

    /// The settings on show: this group's, narrowed by what has been typed.
    ///
    /// Plainly by substring rather than fuzzily: there are a dozen of these
    /// and a reader typing "the" means the word, where a fuzzy match would
    /// also offer everything with a t, an h and an e scattered through it.
    #[must_use]
    pub fn rows(&self) -> Vec<&'static Setting> {
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
        if self.query.is_empty() {
            return None;
        }
        let query: Vec<char> = self.query.to_lowercase().chars().collect();
        let label: Vec<char> = setting.label.to_lowercase().chars().collect();
        label
            .windows(query.len())
            .position(|window| window == query.as_slice())
            .map(|at| at..at + query.len())
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

    /// Handles a key, given the settings as they stand.
    pub fn handle_key(&mut self, key: &KeyEvent, config: &Config) -> SettingsOutcome {
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

        match key.code {
            KeyCode::Esc if bare => SettingsOutcome::Cancelled,
            KeyCode::Down if bare && !rows.is_empty() => {
                self.focus = (self.focus + 1) % rows.len();
                SettingsOutcome::Consumed
            }
            KeyCode::Up if bare && !rows.is_empty() => {
                self.focus = if self.focus == 0 {
                    rows.len() - 1
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
            KeyCode::Enter | KeyCode::Char(' ') if bare => match rows.get(self.focus) {
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
        let last = Group::ALL.len() - 1;
        self.group = match (forward, self.group) {
            (true, at) if at >= last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        self.settle();
    }

    /// Puts the focus back on a row that exists, after the rows change.
    fn settle(&mut self) {
        let rows = self.rows().len();
        self.focus = self.focus.min(rows.saturating_sub(1));
    }
}
