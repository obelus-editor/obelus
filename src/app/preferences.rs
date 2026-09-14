//! The settings, and the file they are kept in.
//!
//! The view is [`crate::component::settings`] and the file is
//! [`crate::config`]; what is here is applying a setting to the running
//! program and writing it back.

use super::*;

/// The settings as they stand, and where each part of them came from.
///
/// One field on `App` rather than seven, because the seven move together:
/// the reader's file is read, the keys it named are noted, the tree's file
/// is laid over the top, and the page says whose a value is out of all of
/// it. A change that set one and forgot another would be a page confidently
/// naming the wrong layer, which is exactly the bug this grouping is here
/// to stop happening twice.
#[derive(Debug)]
pub(super) struct Settled {
    /// What obelus is actually going by: the reader's own, with the tree's
    /// laid over it.
    pub config: crate::config::Config,
    /// Where the reader's file is, or `None` for an application that was
    /// never told -- which is every test, and is why a test cannot write
    /// over the reader's real settings.
    pub path: Option<PathBuf>,
    /// False once that file has been found unreadable: what is in it is the
    /// reader's, and saving over something obelus could not read would
    /// replace settings it never saw. True again the moment it reads.
    pub readable: bool,
    /// The reader's own layer, before the tree's went over it.
    ///
    /// Kept so the tree's page can say whose value a row is showing. Worked
    /// out from the merged config it cannot be -- by then the two are one.
    pub readers: crate::config::Config,
    /// Which settings the reader's file named.
    ///
    /// Which, not what they came to: a reader who writes a setting down has
    /// said something about it even where what they said is what obelus
    /// would have done anyway.
    pub named: Vec<&'static str>,
    /// The tree's own settings file, while the tree has one.
    ///
    /// Never written to by a reader changing their own settings: it belongs
    /// to whoever wrote the tree, and their next commit would carry it.
    pub tree: Option<PathBuf>,
    /// The settings that file set, which are the ones the reader cannot
    /// change here.
    pub pinned: Vec<&'static str>,
}

impl Default for Settled {
    fn default() -> Self {
        Self {
            config: crate::config::Config::default(),
            path: None,
            // Until something says otherwise: a file nobody has failed to
            // read is a file obelus may write.
            readable: true,
            readers: crate::config::Config::default(),
            named: Vec::new(),
            tree: None,
            pinned: Vec::new(),
        }
    }
}

impl App {
    /// What the reader has decided.
    #[must_use]
    pub const fn config(&self) -> &crate::config::Config {
        &self.settled.config
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

    /// Opens the tree's own settings, which are a page of the same shape.
    ///
    /// A command of its own rather than a tab on the other page: the tabs
    /// there are *groups* of settings, and a scope among them would be one
    /// list holding two kinds of thing.
    pub fn open_project_settings(&mut self) {
        self.picker = None;
        self.prompt = None;
        self.refresh_registry();
        self.settings = Some(Settings::for_tree());
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
                prose: false,
                marker: None,
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
        // What the theme was, for the same reason the theme list keeps it:
        // walking this list wears each colour in turn, and the one that was
        // on is only in the running program.
        if key == "theme" {
            self.theme_before = Some(self.theme);
        }
        self.picker = Some(picker);
    }

    /// Applies a setting the settings view changed, and writes the file.
    ///
    /// Applied first and saved second, so a file that cannot be written
    /// still leaves the reader with the setting they asked for until they
    /// restart -- and with a note saying it will not last.
    pub(super) fn change_setting(&mut self, key: &'static str, value: &crate::config::Value) {
        // Which file this change is for is the page's own question: the
        // reader's settings, or the tree's. Asked here rather than carried
        // in the outcome, because it is a fact about what is open and not
        // about which key was pressed.
        if self.settings.as_ref().is_some_and(Settings::on_tree) {
            self.write_to_tree(key, Some(value));
            return;
        }
        if let Some(path) = self.pinned_by(key) {
            // Nothing happens, and nothing needs saying: the row itself
            // carries the name of the file that has it, in the dim ink
            // every unusable thing here is drawn in.
            tracing::debug!(key, path = %path.display(), "the tree has this one");
            return;
        }
        // The reader's own layer, and then the tree's back over it: a
        // setting they change is theirs, and what the tree has is still the
        // tree's.
        self.settled.readers.set(key, value);
        self.apply_tree();
        let Some(path) = self.settled.path.clone() else {
            // Nobody said where the file is, so there is nothing to write
            // to: an application that never read one does not write one.
            return;
        };
        if !self.settled.readable {
            // Said when it was found to be unreadable, and again here,
            // because this is the moment the reader finds out their change
            // is not being kept.
            self.note = Some("not saved: the settings will not read".to_string());
            return;
        }
        if let Err(error) = crate::config::save_to(&path, &self.settled.readers) {
            tracing::warn!(%error, "not saving the configuration");
            self.note = Some(format!("not saved: {error}"));
        }
    }

    /// Takes a setting out of the tree's file, from the tree's own page.
    pub(super) fn unset_setting(&mut self, key: &'static str) {
        self.write_to_tree(key, None);
    }

    /// Writes one key to the tree's settings, or takes it out.
    ///
    /// Making the file if the tree has none: a reader who has opened the
    /// tree's settings and changed something has said plainly enough that
    /// this tree should have them.
    ///
    /// What a tree may not set is refused here as well as when the file is
    /// read. Refused rather than written and then ignored, which would be a
    /// file that says something obelus will not do.
    fn write_to_tree(&mut self, key: &'static str, value: Option<&crate::config::Value>) {
        if crate::config::reach_of(key) != crate::config::Reach::Anywhere {
            tracing::debug!(key, "a tree may not set this one");
            return;
        }
        let path = crate::config::tree_path_for(&self.working_directory);
        if let Err(error) = crate::config::write_tree(&path, key, value) {
            tracing::warn!(%error, path = %path.display(), "not writing the tree's settings");
            self.note = Some(format!("not saved: {error}"));
            return;
        }
        // Read back the way any other change to that file arrives, so the
        // page shows what the file says rather than what obelus meant to
        // put in it.
        self.reread_config();
    }

    /// Opens the settings file itself, for a reader who would rather see    ///
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
        let Some(path) = self.settled.path.clone() else {
            self.note = Some("this system has nowhere for a settings file".to_string());
            return;
        };
        if !path.exists()
            && let Err(error) = crate::config::save_to(&path, &self.settled.config)
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
        self.settled
            .config
            .keys
            .insert(command.name().to_string(), written);
        self.keymap.rebind(command, chord);
        let Some(path) = self.settled.path.clone() else {
            return;
        };
        if let Err(error) = crate::config::save_to(&path, &self.settled.config) {
            tracing::warn!(%error, "not saving the configuration");
            self.note = Some(format!("not saved: {error}"));
        }
    }

    /// Makes the running program match the configuration.
    ///
    /// One place, called at startup and after every change, so a setting
    /// cannot mean one thing on the way in and another when it is edited.
    fn apply_config(&mut self) {
        if let Some(theme) = builtin::by_name(&self.settled.config.theme) {
            self.theme = theme;
        }
        icons::use_glyphs(self.settled.config.icons);
        // The table the reader's own bindings leave. Built rather than
        // patched: what is in the file is a list of changes over the
        // defaults, and applying them to a table that has already had them
        // applied would leave a rebind that was undone in the file still in
        // force.
        self.keymap = crate::keymap::Keymap::with(&self.settled.config.keys);
    }

    /// Reads the configuration file and applies it.
    ///
    /// Separate from [`App::new`] so that a test gets the defaults rather
    /// than whatever the machine it runs on has in `~/.config`.
    pub fn load_config(&mut self) {
        self.settled.path = crate::config::path();
        let Some(path) = self.settled.path.clone() else {
            tracing::info!("nowhere to keep settings, so the defaults");
            return;
        };
        // Which file, and what was in it: "my setting did nothing" is
        // answered by the path obelus actually read, and a reader with two
        // machines or an `XDG_CONFIG_HOME` has more than one candidate.
        match crate::config::read_from(&path) {
            crate::config::Reading::Settings(config, named) => {
                tracing::info!(path = %path.display(), settings = ?named, "read the settings");
                self.configure(config, named);
            }
            crate::config::Reading::Nothing | crate::config::Reading::Nowhere => {
                tracing::info!(path = %path.display(), "no settings file yet, so the defaults");
            }
            crate::config::Reading::Unreadable(why) => self.settings_unreadable(&path, &why),
        }
        self.apply_tree();
    }

    /// Lays the tree's own settings over the reader's.
    ///
    /// After theirs, every time theirs is read: the tree is the narrower
    /// answer -- it is about *this* project -- so it wins where it says
    /// anything, and says nothing everywhere else.
    ///
    /// A file that will not read is a line in the log and nothing more. The
    /// reader's settings are what obelus has, and throwing them away because
    /// a tree somebody else wrote has a typo in it would be the tree
    /// deciding something it was never given.
    pub(super) fn apply_tree(&mut self) {
        // From the reader's own answers up, every time. Laid over what is
        // already there instead, a setting the tree has *stopped* naming
        // would stay in force: nothing would have put the reader's answer
        // back underneath it, and deleting a line from the tree's file
        // would do nothing until obelus was started again.
        self.settled.config = self.settled.readers.clone();
        self.settled.pinned.clear();
        self.settled.tree = crate::config::tree_path(&self.working_directory);
        let Some(path) = self.settled.tree.clone() else {
            self.apply_config();
            return;
        };
        match crate::config::read_table(&path) {
            Ok(Some(table)) => {
                self.settled.pinned = crate::config::apply(
                    &mut self.settled.config,
                    &table,
                    crate::config::Whose::Tree,
                );
                tracing::info!(
                    path = %path.display(),
                    settings = ?self.settled.pinned,
                    "the tree has settings of its own",
                );
            }
            Ok(None) => {}
            Err(why) => {
                tracing::warn!(path = %path.display(), why, "the tree's settings will not read");
            }
        }
        self.apply_config();
    }

    /// The reader's own settings, under whatever the tree lays over them.
    #[must_use]
    pub const fn readers_config(&self) -> &crate::config::Config {
        &self.settled.readers
    }

    /// The settings the tree has set, which are the ones the reader cannot
    /// change from here.
    #[must_use]
    pub fn pinned(&self) -> &[&'static str] {
        &self.settled.pinned
    }

    /// Which settings the reader's own file named.
    #[must_use]
    pub fn readers_named(&self) -> &[&'static str] {
        &self.settled.named
    }

    /// The tree's own settings file, while the tree has one.
    #[must_use]
    pub fn tree_config(&self) -> Option<&Path> {
        self.settled.tree.as_deref()
    }

    /// Which file has this setting, if it is not the reader's to change.
    #[must_use]
    pub fn pinned_by(&self, key: &str) -> Option<&Path> {
        self.settled
            .pinned
            .contains(&key)
            .then_some(self.settled.tree.as_deref())
            .flatten()
    }

    /// Takes the settings file as it stands now, because somebody else
    /// changed it.
    ///
    /// Another obelus on the same project, or the reader's own editor: what
    /// is in the file is what obelus is set to, whichever process wrote it.
    /// Only the settings, not [`App::configure`]'s second half -- that puts
    /// every open file back to the reading the settings ask for, and a
    /// reader who has turned a preview off should not have it come back
    /// because somebody in another window changed the theme.
    pub(super) fn reread_config(&mut self) {
        if let Some(path) = self.settled.path.clone() {
            match crate::config::read_from(&path) {
                crate::config::Reading::Settings(config, named) => {
                    tracing::info!(path = %path.display(), "the settings changed under us");
                    self.settled.readers = config.clone();
                    self.settled.named = named;
                    self.settled.config = config;
                    self.apply_config();
                    self.settled.readable = true;
                }
                // Gone, which is somebody deleting it or an editor writing
                // it in a way obelus caught mid-flight. Neither is a reason
                // to throw away what this session is set to.
                crate::config::Reading::Nothing | crate::config::Reading::Nowhere => {}
                crate::config::Reading::Unreadable(why) => self.settings_unreadable(&path, &why),
            }
        }
        // And the tree's over the top, from the reader's file up: a layer
        // laid over what already has it would keep a setting the tree has
        // since stopped naming, because nothing would have put the reader's
        // own answer back underneath it.
        self.apply_tree();
    }

    /// Says the settings file cannot be read, and stops writing to it.
    ///
    /// What is in it is the reader's, and obelus cannot read it: saving
    /// over it would replace settings it never saw with whatever this
    /// session happens to be set to. So nothing is saved until it reads
    /// -- which it will, the moment somebody fixes the file, because the
    /// watcher is on it.
    fn settings_unreadable(&mut self, path: &Path, why: &str) {
        tracing::warn!(path = %path.display(), why, "the settings file will not read");
        self.settled.readable = false;
        // Short, because the status row is one row and shares it with the
        // file and the position: which file and what went wrong are in the
        // log, where there is room for them.
        self.note = Some("the settings will not read, so none are saved".to_string());
    }

    /// Uses a configuration without reading a file.
    ///
    /// The way in for anything that has settings from somewhere else -- a
    /// test that needs wrapping on, a file that has already been read.
    /// Nothing is written back unless a path has been named as well.
    /// `named` is which settings their file spoke about. A setting they
    /// wrote down is theirs whether or not it says anything the default did
    /// not: working that out by comparing with the default cannot tell a
    /// reader who agreed from a reader who never came.
    pub fn configure(&mut self, config: crate::config::Config, named: Vec<&'static str>) {
        // The reader's own layer, which is what the tree's is laid over --
        // and what a setting goes back to when the tree stops naming it.
        self.settled.readers = config.clone();
        self.settled.named = named;
        self.settled.config = config;
        self.apply_config();
        self.apply_tree();
    }

    /// Reads and writes settings at a path of the caller's choosing.
    ///
    /// For a test: the reader's own file is not something a test may write
    /// to, and a test of "does changing this save it" has to have a file.
    pub fn config_file_for_test(&mut self, path: PathBuf) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let named = match crate::config::read_from(&path) {
            crate::config::Reading::Settings(_, named) => named,
            _ => Vec::new(),
        };
        self.configure(crate::config::from_toml(&text), named);
        self.settled.path = Some(path);
        self.settled.readable = true;
    }
}
