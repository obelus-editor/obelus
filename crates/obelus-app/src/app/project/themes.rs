//! The themes the registry lists, and the ones taken from it.
//!
//! What the settings' Themes page is made of: the registry's index, fetched
//! when the settings open; what is installed, which the directories say;
//! and what is on its way. The page itself is `obelus_component::settings`;
//! what a card says and what pressing it does are here, because both are
//! questions about this machine.
//!
//! **A theme the registry no longer lists is still shown, if it is
//! installed** -- under the group it came from, saying so, with nothing on
//! it but the key that takes it away. It still works and is the reader's to
//! keep; and taking it away is the one thing that cannot be undone from the
//! page, so it takes the same key twice.

use obelus_component::settings::Page;
use obelus_themes::{Card, Group, Listed, Status, install::Progress};

use crate::app::*;

/// What a list of themes says where what was typed matches none of them,
/// and enter takes the words to the themes page.
pub(in crate::app) const NONE_HERE: &str = "No theme here by that name. Enter looks in More themes";

/// What Obelus knows about the registry's themes.
#[derive(Debug, Default)]
pub(in crate::app) struct Registry {
    /// What its index lists, kept and then fetched.
    pub index: Vec<Listed>,
    /// Whether it has been asked for and not yet failed.
    pub asked: bool,
    /// Why it could not be fetched, until it is tried again.
    pub failure: Option<String>,
    /// The installs on their way, and how far each has got.
    pub installing: HashMap<String, Option<Progress>>,
    /// Why the last install of each did not take.
    pub failures: HashMap<String, String>,
    /// The installs on their way that are first installs rather than
    /// updates, which are worn when they land.
    pub fresh: HashSet<String>,
    /// The theme the reader has pressed delete on once, where taking it
    /// away cannot be undone: the next press of the same key on the same
    /// card takes it.
    pub taking: Option<String>,
}

impl App {
    /// Fetches the registry's index, showing whatever was kept while it
    /// runs -- once a session, the way the agents' registry is.
    pub(in crate::app) fn refresh_themes(&mut self) {
        if !self.themes.index.is_empty() || self.themes.asked {
            return;
        }
        self.themes.asked = true;
        self.themes.failure = None;
        if let Some(sender) = self.events.clone() {
            obelus_themes::spawn_fetch(sender);
        }
    }

    /// Takes an index, from wherever it was read.
    ///
    /// A failure leaves whatever list there is and lets the next opening of
    /// the settings try again, for the agents' reason.
    pub(in crate::app) fn on_themes(&mut self, themes: Vec<Listed>, failure: Option<String>) {
        if let Some(why) = failure {
            self.themes.failure = Some(why);
            self.themes.asked = false;
            return;
        }
        if themes.is_empty() {
            return;
        }
        self.themes.failure = None;
        self.themes.index = themes;
    }

    /// Why the index could not be fetched, where it could not.
    #[must_use]
    pub fn themes_failure(&self) -> Option<&str> {
        self.themes.failure.as_deref()
    }

    /// Hands the page an index, for a test: the real one is fetched over a
    /// network, which a test must not reach.
    pub fn themes_index_for_test(&mut self, themes: Vec<Listed>) {
        self.themes.index = themes;
    }

    /// The page's cards: every theme the index lists, each beside what is
    /// true of it here, a group at a time -- and those installed that it no
    /// longer lists, at the foot of the group they came from.
    #[must_use]
    pub fn listed_themes(&self) -> Vec<Card> {
        let root = self.settled.installed.clone();
        let installed = |name: &str| {
            root.as_deref()
                .and_then(|root| obelus_themes::install::installed(name, root))
        };
        let worn = self.theme_name();
        let mut cards = Vec::new();
        for group in Group::ALL {
            for theme in self
                .themes
                .index
                .iter()
                .filter(|theme| theme.group == group)
            {
                let record = installed(&theme.name);
                let status = if let Some(progress) = self.themes.installing.get(&theme.name) {
                    Status::Installing(*progress)
                } else if let Some(why) = self.themes.failures.get(&theme.name) {
                    Status::Failed(why.clone())
                } else {
                    match &record {
                        Some(record) if record.commit == theme.commit => Status::Installed,
                        Some(_) => Status::Outdated,
                        None => Status::Missing,
                    }
                };
                cards.push(self.theme_card(theme.clone(), status, record.is_some(), worn));
            }
            // What is installed and no longer listed: a directory the
            // registry put there, by its record, under no name the index
            // still has.
            let Some(root) = root.as_deref() else {
                continue;
            };
            for (name, _) in obelus_theme::written::found_in(root) {
                let Some(record) = installed(&name) else {
                    continue;
                };
                if record.group != group || self.themes.index.iter().any(|theme| theme.name == name)
                {
                    continue;
                }
                let theme = Listed {
                    name,
                    group,
                    dark: true,
                    author: String::new(),
                    description: String::new(),
                    wallpaper: None,
                    package: String::new(),
                    sha256: String::new(),
                    commit: record.commit,
                    updated: String::new(),
                };
                cards.push(self.theme_card(theme, Status::Delisted, true, worn));
            }
        }
        cards
    }

    /// The row at the foot of a list of themes that goes for more of them.
    ///
    /// A row like the others, filtered with them, because it is where the
    /// list's question goes next: a reader typing a name the list does not
    /// have is a reader on their way to it. One builder for both lists of
    /// themes, the command's and the setting's, so they cannot come to say
    /// it differently.
    pub(in crate::app) fn more_themes() -> PickerItem {
        PickerItem {
            prose: false,
            marker: None,
            icon: obelus_icons::enabled().then_some(obelus_icons::ui::MORE_THEMES),
            label: "More themes".to_string(),
            version: None,
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::MoreThemes,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// Goes to the settings' themes page, with `looking_for` in its filter.
    ///
    /// The reader's settings: the project's have no themes page, because
    /// which theme is worn is the reader's alone -- so a list opened over
    /// those gives way to these.
    pub(in crate::app) fn show_more_themes(&mut self, looking_for: &str) {
        if !self
            .settings
            .as_ref()
            .is_some_and(|settings| !settings.on_project())
        {
            self.open_settings();
        }
        if let Some(settings) = self.settings.as_mut() {
            settings.go_to(Page::Themes, looking_for);
        }
    }

    /// The cards, where the page is showing them, and none where it is not:
    /// they read a record per installed theme, which a frame of any other
    /// page has no reason to.
    pub(in crate::app) fn themes_on_the_page(&self) -> Vec<Card> {
        match self.settings.as_ref().is_some_and(Settings::on_themes) {
            true => self.listed_themes(),
            false => Vec::new(),
        }
    }

    /// One card: the theme, where it stands, and whether it is the one worn
    /// or one of the reader's own answers to its name instead.
    fn theme_card(&self, theme: Listed, status: Status, installed: bool, worn: &str) -> Card {
        let whence = self.theme_whence(&theme.name);
        Card {
            worn: installed && theme.name == worn && whence == Some("Downloaded"),
            shadowed: installed && whence == Some("Local"),
            installed,
            theme,
            status,
        }
    }

    /// Starts installing a theme the index lists, or updating it.
    pub(in crate::app) fn install_theme(&mut self, name: &str) {
        let Some(theme) = self
            .themes
            .index
            .iter()
            .find(|theme| theme.name == name)
            .cloned()
        else {
            return;
        };
        if self.themes.installing.contains_key(name) {
            return;
        }
        let Some(root) = self.settled.installed.clone() else {
            self.wrong("This system has nowhere to keep a theme");
            return;
        };
        // Nothing to say about how far yet, but the card stops offering a
        // button the moment it is pressed.
        self.themes.installing.insert(theme.name.clone(), None);
        self.themes.failures.remove(name);
        self.themes.taking = None;
        // Asked now, while the directory says whether it is there: once the
        // install lands it always is.
        if obelus_themes::install::installed(name, &root).is_none() {
            self.themes.fresh.insert(theme.name.clone());
        }
        if let Some(sender) = self.events.clone() {
            tracing::info!(name, root = %root.display(), "installing a theme");
            obelus_themes::install::spawn(&theme, &root, sender);
        }
    }

    /// Notes how far an install has got.
    pub(in crate::app) fn on_theme_installing(&mut self, name: String, progress: Progress) {
        self.themes.installing.insert(name, Some(progress));
    }

    /// Takes an install's outcome.
    ///
    /// The theme being worn, updated, is worn as it now is: the watch on the
    /// directory would say so a moment later, and the reader pressed the key
    /// now.
    ///
    /// **A theme installed for the first time is worn**, the way choosing it
    /// from the list wears it, setting and all: installing one is the reader
    /// saying they want to see it, and a theme that arrives and changes
    /// nothing is a second step they have to go and find. Not an update to
    /// a theme they are not wearing, which says nothing about wanting it on;
    /// and not one a theme of the reader's own by that name stands in front
    /// of, because wearing the name would wear theirs.
    pub(in crate::app) fn on_theme_installed(&mut self, name: String, failure: Option<String>) {
        self.themes.installing.remove(&name);
        let fresh = self.themes.fresh.remove(&name);
        match failure {
            Some(why) => {
                self.themes.failures.insert(name, why);
            }
            None => {
                self.themes.failures.remove(&name);
                if self.settled.config.theme == name {
                    self.reread_theme();
                } else if fresh && self.theme_whence(&name) == Some("Downloaded") {
                    self.choose_theme(&name);
                }
            }
        }
    }

    /// Wears a theme and keeps it, as choosing it from a list does.
    ///
    /// Kept, because a reader who picks a theme and finds the old one back
    /// tomorrow has been given a preview rather than a choice. And where a
    /// list of themes is open over the page, the one it puts back on escape
    /// is this one: it is the reader's choice now, whatever the list was
    /// walking past.
    pub(in crate::app) fn choose_theme(&mut self, name: &str) {
        self.change_setting("theme", &obelus_config::Value::Choice(name.to_string()));
        if let Some(theme) = self.theme_called(name) {
            self.set_theme(name, theme);
        }
        if self.theme_before.is_some() {
            self.theme_before = Some((name.to_string(), self.worn()));
        }
    }

    /// Takes away a theme taken from the registry -- or, for one it no
    /// longer lists, says the next press of the key will.
    ///
    /// Not the one being worn: a theme taken away from under the reader is
    /// the screen changing to another nobody chose. The page offers no key
    /// for it, and this refuses it as well, for a press that got here some
    /// other way.
    pub(in crate::app) fn take_theme_away(&mut self, name: &str) {
        let Some(card) = self
            .listed_themes()
            .into_iter()
            .find(|card| card.theme.name == name)
        else {
            return;
        };
        if !card.may_be_taken_away() {
            return;
        }
        if card.status == Status::Delisted && self.themes.taking.as_deref() != Some(name) {
            self.themes.taking = Some(name.to_string());
            self.say(format!("Delete again to take {name} away for good"));
            return;
        }
        self.themes.taking = None;
        let Some(root) = self.settled.installed.clone() else {
            return;
        };
        match obelus_themes::install::remove(name, &root) {
            Ok(()) => self.say(format!("Took {name} away")),
            Err(why) => self.wrong(why),
        }
    }
}
