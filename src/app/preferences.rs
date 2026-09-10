//! The settings, and the file they are kept in.
//!
//! The view is [`crate::component::settings`] and the file is
//! [`crate::config`]; what is here is applying a setting to the running
//! program and writing it back.

use super::*;

impl App {
    /// What the reader has decided.
    #[must_use]
    pub const fn config(&self) -> &crate::config::Config {
        &self.config
    }

    /// The settings view, while it is open.
    #[must_use]
    pub const fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    /// Opens the settings.
    pub fn open_settings(&mut self) {
        // Nothing else can be open under it: it is a full-screen view with
        // its own typing, and two of those would take the same keys.
        self.picker = None;
        self.prompt = None;
        // Asked for now rather than when the tab is reached: the fetch
        // takes a moment, and a reader who walks to the agents tab should
        // find a list there rather than watch one arrive.
        self.refresh_registry();
        self.settings = Some(Settings::new());
    }

    /// Offers a setting's choices, as the ordinary compact list.
    ///
    /// The same list the symbol menu is, for the same reasons: it filters by
    /// typing, it scrolls, it knows what a selected row looks like, and a
    /// droplist of its own would be a second answer to all three.
    pub(super) fn open_choices(
        &mut self,
        key: &'static str,
        choices: &'static [&'static str],
        word: &str,
    ) {
        let items: Vec<PickerItem> = choices
            .iter()
            .map(|choice| PickerItem {
                icon: None,
                label: (*choice).to_string(),
                detail: None,
                trailing: None,
                value: PickerValue::Setting {
                    key,
                    word: (*choice).to_string(),
                },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.when_empty("this setting has no choices");
        // Opened on the one in force, so the list starts by saying which
        // that is.
        picker.prefer(word.to_string());
        self.picker = Some(picker);
    }

    /// Applies a setting the settings view changed, and writes the file.
    ///
    /// Applied first and saved second, so a file that cannot be written
    /// still leaves the reader with the setting they asked for until they
    /// restart -- and with a note saying it will not last.
    pub(super) fn change_setting(&mut self, key: &'static str, value: &crate::config::Value) {
        self.config.set(key, value);
        self.apply_config();
        let Some(path) = self.config_path.clone() else {
            // Nobody said where the file is, so there is nothing to write
            // to: an application that never read one does not write one.
            return;
        };
        if let Err(error) = crate::config::save_to(&path, &self.config) {
            tracing::warn!(%error, "not saving the configuration");
            self.note = Some(format!("not saved: {error}"));
        }
    }

    /// Opens the settings file itself, for a reader who would rather see
    /// them all at once -- or edit one obelus has no control for.
    ///
    /// Written first if it is not there yet, because the file obelus would
    /// write is the answer to "what are the settings": a reader sent to a
    /// path that does not exist has been told nothing, and the file with
    /// every default in it is what they need in front of them to change one
    /// by hand.
    ///
    /// Read, not applied. Obelus reads this file when it starts and writes
    /// it when the reader changes something on the settings page; a change
    /// made in it by hand is picked up the next time obelus starts.
    pub fn open_config_file(&mut self) {
        let Some(path) = self.config_path.clone() else {
            self.note = Some("this system has nowhere for a settings file".to_string());
            return;
        };
        if !path.exists()
            && let Err(error) = crate::config::save_to(&path, &self.config)
        {
            tracing::warn!(%error, "not writing the configuration");
            self.note = Some(format!("no settings file, and none written: {error}"));
            return;
        }
        self.open(&path);
    }

    /// Moves a command onto a key, or takes its key away, and writes the
    /// file.
    ///
    /// The same shape as changing a setting -- applied first, saved second
    /// -- and through the same table: what is written down is the command's
    /// name and the chord spelled out, because a table full of enum
    /// spellings and keycodes would be obelus's own business rather than
    /// something a reader can edit.
    pub(super) fn rebind(&mut self, command: crate::command::Command, chord: Option<KeyChord>) {
        let written = chord.map(|chord| chord.label_in(false)).unwrap_or_default();
        self.config.keys.insert(command.name().to_string(), written);
        self.keymap.rebind(command, chord);
        let Some(path) = self.config_path.clone() else {
            return;
        };
        if let Err(error) = crate::config::save_to(&path, &self.config) {
            tracing::warn!(%error, "not saving the configuration");
            self.note = Some(format!("not saved: {error}"));
        }
    }

    /// Makes the running program match the configuration.
    ///
    /// One place, called at startup and after every change, so a setting
    /// cannot mean one thing on the way in and another when it is edited.
    fn apply_config(&mut self) {
        if let Some(theme) = builtin::ALL
            .iter()
            .find(|theme| theme.name == self.config.theme)
        {
            self.theme = theme;
        }
        icons::use_glyphs(self.config.icons);
        self.showing_blame = self.config.blame;
        // The table the reader's own bindings leave. Built rather than
        // patched: what is in the file is a list of changes over the
        // defaults, and applying them to a table that has already had them
        // applied would leave a rebind that was undone in the file still in
        // force.
        self.keymap = crate::keymap::Keymap::with(&self.config.keys);
    }

    /// Reads the configuration file and applies it.
    ///
    /// Separate from [`App::new`] so that a test gets the defaults rather
    /// than whatever the machine it runs on has in `~/.config`.
    pub fn load_config(&mut self) {
        self.config_path = crate::config::path();
        self.configure(crate::config::load());
    }

    /// Uses a configuration without reading a file.
    ///
    /// The way in for anything that has settings from somewhere else -- a
    /// test that needs wrapping on, a file that has already been read.
    /// Nothing is written back unless a path has been named as well.
    pub fn configure(&mut self, config: crate::config::Config) {
        self.config = config;
        self.apply_config();
    }

    /// Reads and writes settings at a path of the caller's choosing.
    ///
    /// For a test: the reader's own file is not something a test may write
    /// to, and a test of "does changing this save it" has to have a file.
    pub fn config_file_for_test(&mut self, path: PathBuf) {
        self.configure(crate::config::from_toml(
            &std::fs::read_to_string(&path).unwrap_or_default(),
        ));
        self.config_path = Some(path);
    }
}
