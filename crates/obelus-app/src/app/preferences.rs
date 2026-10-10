//! The settings, and the file they are kept in.
//!
//! The view is [`obelus_component::settings`] and the file is
//! [`obelus_config`]; what is here is applying a setting to the running
//! program and writing it back.
//!
//! A project may carry settings, and a project is not the reader.
//! `.obelus/config.toml` in the working directory, laid *over* the reader's own
//! file key by key: the project says what this project needs -- wrapped lines,
//! a theme -- and says nothing about everything else, which stays theirs. Read
//! into a fresh config instead of over theirs and a project with one line in it
//! would turn off a reader's wrapping, which is what every "project settings"
//! feature that replaces rather than layers actually does.
//!
//! A directory rather than a dotfile, because settings are not the only thing a
//! project will keep for Obelus -- a theme of its own, whatever comes after it
//! -- and one directory is one thing to find, to copy between machines and to
//! name in a `.gitignore`, where a dotfile per kind of thing is a row of them
//! at the top of every listing. Only the working directory itself, never
//! walking up: Obelus has one answer to which project it is on -- the file list
//! walks it, the counts count it, git is read from it.
//!
//! The project's settings have a page of their own, `open-project-settings`, a
//! command rather than a fifth tab: the tabs there are *groups* of settings and
//! a scope among them would be one list holding two kinds of thing. The same
//! page otherwise -- same tabs, same rows, same keys -- because they are the
//! same settings; what differs is the file a change is written to, which is on
//! the tab row and stays there.
//!
//! On the reader's page a setting the project has is not theirs to change, and
//! the row says so rather than doing nothing when pressed: the file's name
//! where they would have reached, a lock against the control, the whole row in
//! the dim ink that means unusable everywhere else. Sublime's project settings
//! win silently, and "I changed it and nothing happened" is the bug that
//! follows.
//!
//! On the project's page the ink goes the *other* way, because dim means "not
//! yours to use here" and there a row the project has not got is the one thing
//! a reader can do something to: pressing it is how a setting becomes the
//! project's. So the row is ordinary, and the word saying which layer the value
//! comes from -- `project`, `global`, `default` -- is dim except on the rows
//! the project itself has, with the control it belongs to. `global` is what
//! `git config` has taught everybody who works in a repository, and is less
//! slippery than "yours" on a page where everything is in some sense theirs;
//! all three have a word, because a column where one of them is blank asks a
//! reader to read an absence. Drawn like the reader's page, a fresh project was
//! a page of grey with nothing on it to look at, which is a rule applied past
//! the point where it still meant anything. `delete` takes it out again, which
//! is what that key means on the keys page too. The two tabs a project may not
//! have are not on it at all: a tab is for somewhere else to go, and one that
//! goes nowhere is a tab that lies.
//!
//! That file is *edited*, not rewritten -- and so, now, is the reader's own
//! (`obelus_config::over`, which says why). A project's is written by hand and
//! committed, so it has comments in it, an order somebody chose, and possibly
//! keys this version has never heard of -- `toml_edit` keeps all three where a
//! round trip through a `toml::Table` would throw them away on the first switch
//! a reader flipped. What is written above a key goes with it when it goes,
//! except for whatever is above the last blank line: a comment touching a key
//! is about that key, and a heading an empty line away is the file's own.
//!
//! **Anything read once at startup must be re-read when somebody else changes
//! it.** Several Obelus processes on one project is the normal case. The
//! settings were read at startup and never again, so a theme changed in one
//! window was a theme changed in one window. The watcher -- already there for
//! open files -- watches the settings file too, and `App::reread_config`
//! applies what it finds: `apply_config` rather than `configure`, for the
//! reason its doc gives. The agent is the one setting that does *not* reach
//! in: a conversation is this window's, and restarting it under the reader
//! because another window chose differently is somebody else's decision
//! arriving as an interruption.
//!
//! What is watched is the file the project *would* have, not the one it has.
//! The ordinary project has no settings of its own until somebody gives it some
//! -- the window next door writing the first one, or a pull bringing it -- and
//! watching only what was there at startup is the "read once at startup"
//! mistake with a longer fuse, because it looks right until the file is
//! created. The same path answers the change when it arrives.
//!
//! The reader's settings are the layer the project's is laid over, and Obelus
//! keeps both. `readers_config` is what their file says; `config` is that
//! with the project's over it, rebuilt from the bottom every time either
//! changes. Laid over what is already there instead, a setting the project has
//! *stopped* naming would stay in force -- deleting a line from the project's
//! file would do nothing until Obelus was started again.
//!
//! **What Obelus cannot make of a file it reads is a mark on that file.** The
//! settings, a project's settings: what would not read used to reach the
//! reader as a line in the log and a sentence on the status row. Neither says
//! *where*, which is the one thing the reader needs -- a file that will not
//! parse has a line that will not parse, and Obelus is holding it. So it is
//! said on the file, into the same list a server's diagnostics go in
//! (`App::obelus_says`, which has why they are one list). Every file Obelus
//! reads for its own sake gets it, not the settings alone: a theme that will
//! not parse is marked on the theme's file, and the notes on theirs -- the
//! notes because a reader opens that file, writing into it from the page and
//! editing it by hand, and being told the whole list will not read without
//! being told which line is a reader reading it all themselves.
//!
//! A theme *name* nothing answers to is marked on the line that names it, in
//! the settings: the name is a fact about the settings and the file it would
//! name does not exist, so there is nothing else to put a mark on. The colours
//! on screen stay as they are either way, which is the other half of one
//! judgement -- a reader who cannot read the screen cannot fix the file -- and
//! that is exactly why the mark is the only way they find out.
//!
//! What is deliberately *not* marked is what Obelus writes for itself: which
//! conversation belongs to which note, an install record, a claim. A reader
//! does not write those and will not open them, so a mark on one is a mark
//! nobody is standing where it can be seen. They stay a line in the log.
//!
//! A *line* that did nothing is marked too: one Obelus has never heard of, one
//! a project is not allowed to set, one that is not the shape it has to be,
//! and one of the `[keys]` table that bound nothing. That last is where
//! `why_not` was always headed -- it exists so that a key which cannot fire is
//! refused where the reader can see it, and the config file was the one place
//! a refusal still happened in silence. The reason it gives is the reason the
//! page that binds keys shows, in its own words, because it is one judgement.
//! From the outside a line like that looks exactly like a line that was
//! obeyed, which is what made it worth a word in the first place -- and a
//! warning rather than an error, because the file read and everything else in
//! it took. Where each line is comes from `obelus_config::spans_in`.

use super::*;

/// The settings as they stand, and where each part of them came from.
///
/// One field on `App` rather than seven, because the seven move together:
/// the reader's file is read, the keys it named are noted, the project's file
/// is laid over the top, and the page says whose a value is out of all of
/// it. A change that set one and forgot another would be a page confidently
/// naming the wrong layer, which is exactly the bug this grouping is here
/// to stop happening twice.
#[derive(Debug)]
pub(super) struct Settled {
    /// What Obelus is actually going by: the reader's own, with the project's
    /// laid over it.
    pub config: obelus_config::Config,
    /// Where the reader's file is, or `None` for an application that was
    /// never told -- which is every test, and is why a test cannot write
    /// over the reader's real settings.
    pub path: Option<PathBuf>,
    /// False once that file has been found unreadable: what is in it is the
    /// reader's, and saving over something Obelus could not read would
    /// replace settings it never saw. True again the moment it reads.
    pub readable: bool,
    /// The reader's own layer, before the project's went over it.
    ///
    /// Kept so the project's page can say whose value a row is showing. Worked
    /// out from the merged config it cannot be -- by then the two are one.
    pub readers: obelus_config::Config,
    /// The theme the settings name, where nothing answers to that name.
    ///
    /// Held for the same reason as [`Settled::unbound`], and said in the
    /// same place.
    pub no_theme: Option<String>,
    /// The lines of the reader's `[keys]` table that bound nothing.
    ///
    /// Written down where the keymap is built and said where the file is
    /// read: the keymap is built again after every change, and saying it
    /// there would be saying it once per change to a file nobody touched.
    pub unbound: Vec<obelus_editing::keymap::Unbound>,
    /// Which settings the reader's file named.
    ///
    /// Which, not what they came to: a reader who writes a setting down has
    /// said something about it even where what they said is what Obelus
    /// would have done anyway.
    pub named: Vec<&'static str>,
    /// The project's own settings file, while the project has one.
    ///
    /// Never written to by a reader changing their own settings: it belongs
    /// to whoever wrote the project, and their next commit would carry it.
    pub project: Option<PathBuf>,
    /// The settings that file set, which are the ones the reader cannot
    /// change here.
    pub pinned: Vec<&'static str>,
}

impl Default for Settled {
    fn default() -> Self {
        Self {
            config: obelus_config::Config::default(),
            path: None,
            // Until something says otherwise: a file nobody has failed to
            // read is a file Obelus may write.
            readable: true,
            readers: obelus_config::Config::default(),
            named: Vec::new(),
            unbound: Vec::new(),
            no_theme: None,
            project: None,
            pinned: Vec::new(),
        }
    }
}

impl App {
    /// What the reader has decided.
    #[must_use]
    pub const fn config(&self) -> &obelus_config::Config {
        &self.settled.config
    }

    /// The settings view, while it is open.
    #[must_use]
    pub const fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    /// Opens the settings.
    pub fn open_settings(&mut self) {
        self.make_room(Room::Region);
        // Asked for now rather than when the tab is reached: the fetch
        // takes a moment, and a reader who walks to the agents tab should
        // find a list there rather than watch one arrive.
        self.refresh_registry();
        // And what the active agent offers, asked of it now: the page draws
        // what was last heard until the answer arrives.
        self.ask_what_the_agent_offers();
        self.settings = Some(Settings::new());
    }

    /// Opens the project's own settings, which are a page of the same shape.
    ///
    /// A command of its own rather than a tab on the other page: the tabs
    /// there are *groups* of settings, and a scope among them would be one
    /// list holding two kinds of thing.
    pub fn open_project_settings(&mut self) {
        self.make_room(Room::Region);
        self.refresh_registry();
        // Nothing asked of the agent: what it starts on is the reader's
        // alone, and the project's page has no group for it.
        self.settings = Some(Settings::for_project());
    }

    /// Offers a key to the settings page, and says whether it took it.
    ///
    /// The page is the whole editor region while it is open and every
    /// printable character is its own, to filter with -- so what falls
    /// through here is only what it has no use for, which is the chords.
    /// The room the settings page has, which is what a page of movement
    /// and a window are measured against.
    ///
    /// Asked of the view, because that is where the arithmetic lives --
    /// see [`obelus_ui::settings::rows_region`], which the drawing and the
    /// pointer already go through. What went in here was the editor's own
    /// height, two rows more than the page is ever drawn into: the tabs and
    /// their rule come off the top and the foot takes two more off the
    /// bottom, so the window let the focus walk two rows past the last one
    /// on screen before it moved, and a page step overshot by the same two.
    pub(super) fn settings_room(
        &self,
        area: Rect,
        offering: Option<&obelus_component::settings::Offering>,
    ) -> (u16, u16) {
        let Some(settings) = self.settings.as_ref() else {
            return (area.width, area.height);
        };
        let region = obelus_ui::settings::rows_region(area, settings, offering);
        (region.width, region.height)
    }

    pub(super) fn settings_key(&mut self, key: &KeyEvent) -> bool {
        if self.settings.is_none() {
            return false;
        }
        // What the remote page says, before a key is judged against its
        // rows: two keys can arrive between frames -- a chat chosen, then
        // the arrow down to its first field -- and the second would walk the
        // rows the page had before the first.
        self.settle_the_remote_page();
        // The agents the page would show, worked out before the component
        // is borrowed: it needs them to know what enter means on a card,
        // and it is not the thing that knows them.
        let listed = self.listed_agents();
        // Cloned for the same reason: the page needs the table to say which
        // key each command is on, and it is the application that owns it.
        let keymap = self.keymap.clone();
        // And what the active agent offers, for the same reason again: the
        // page lists its settings and knows nothing about where they came
        // from.
        let offering = self.agent_offering();
        let room = self.settings_room(self.editor_area, offering.as_ref());
        let Some(settings) = self.settings.as_mut() else {
            return false;
        };
        match settings.handle_key(
            key,
            &self.settled.config,
            &keymap,
            &listed,
            offering.as_ref(),
            room,
        ) {
            SettingsOutcome::Consumed => true,
            SettingsOutcome::Cancelled => {
                self.leave(Layer::Settings);
                true
            }
            SettingsOutcome::Changed(key, value) => {
                self.change_setting(key, &value);
                true
            }
            SettingsOutcome::Unset(key) => {
                self.unset_setting(key);
                true
            }
            SettingsOutcome::Bind(command, chord) => {
                self.rebind(command, chord);
                true
            }
            SettingsOutcome::Choose(key, choices, word) => {
                self.open_choices(key, choices, &word);
                true
            }
            SettingsOutcome::Names(key) => {
                self.open_names(key);
                true
            }
            SettingsOutcome::Type(key, said) => {
                self.ask_on_the_status_row(Prompt::about(PromptKind::Setting(key), said));
                true
            }
            SettingsOutcome::Install(id) => {
                self.install_agent(&id);
                true
            }
            SettingsOutcome::Activate(id) => {
                self.activate_agent(&id);
                true
            }
            SettingsOutcome::Deactivate => {
                self.deactivate_agent();
                true
            }
            SettingsOutcome::ChooseForAgent(setting) => {
                self.open_agent_default(&setting);
                true
            }
            SettingsOutcome::UnsetForAgent(setting) => {
                self.set_agent_default(&setting, None);
                true
            }
            SettingsOutcome::Remote(reaching) => {
                self.reach(reaching);
                true
            }
            SettingsOutcome::EditVariable(name) => {
                self.name_a_variable(&name);
                true
            }
            SettingsOutcome::AddVariable => {
                self.ask_on_the_status_row(Prompt::new(PromptKind::VariableName));
                true
            }
            SettingsOutcome::RemoveVariable(name) => {
                self.set_agent_variable(&name, None);
                true
            }
            SettingsOutcome::Ignored => false,
        }
    }

    /// Opens the list a setting's names are built in.
    ///
    /// What may go in it is what this machine has, which only the thing
    /// drawing Obelus knows: a window says so when it starts, and until it
    /// has, the list is what the reader already chose and whatever they
    /// type.
    ///
    /// Public because it is the whole of what opening it means, and
    /// because the setting it is for is a window's -- a test running as a
    /// terminal has no row to press enter on.
    pub fn open_names(&mut self, key: &'static str) {
        // What it covers goes first, the same as every other opener: a
        // question left under a band is a question nobody can see the
        // answer to.
        self.make_room(obelus_component::layers::Layer::Names.room());
        let chosen = match self.settled.config.value_of(key) {
            Some(obelus_config::Value::Names(names)) => names,
            _ => Vec::new(),
        };
        let mut names = obelus_component::names::Names::new(
            chosen,
            self.fonts_here.clone(),
            self.monospace_here.clone(),
        );
        // The caller's words, because the list knows names and nothing
        // about what they name -- and this one is the faces a window
        // draws with.
        names.before_typing("Filter the faces on this machine");
        self.names = Some((key, names));
    }

    /// Takes a key while that list is open.
    pub(super) fn names_key(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        let page = self.names.as_ref().map_or(1, |(_, names)| {
            obelus_ui::names::rows_drawn(names, self.picker_area())
        });
        let Some((setting, names)) = self.names.as_mut() else {
            return false;
        };
        let setting = *setting;
        match names.handle_key(key, page) {
            obelus_component::names::Outcome::Consumed => true,
            obelus_component::names::Outcome::Ignored => false,
            obelus_component::names::Outcome::Changed => {
                let chosen = names.chosen().to_vec();
                self.change_setting(setting, &obelus_config::Value::Names(chosen));
                true
            }
            obelus_component::names::Outcome::Leave => {
                self.leave(obelus_component::layers::Layer::Names);
                true
            }
        }
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
        // The themes are the one list here the table cannot hold: which of
        // them there are is a question about two directories, and the table
        // is static data about what a setting *is*. So it is asked of the
        // application, which is the thing that knows.
        //
        // The workflows are asked of the application too, for what each one
        // does: a name like `feature-branch` says what it is only to
        // somebody who knows the term, and the words that say it are in
        // the file beside what the agent is handed.
        let choices: Vec<(String, Option<String>)> = match key {
            "theme" => self.themes().into_iter().map(|name| (name, None)).collect(),
            "workflow" => super::opening::workflows()
                .map(|(name, _, about)| (name.to_string(), Some(about.to_string())))
                .collect(),
            // What each layout puts the opening keys on, because the name
            // is a word to remember it by and not a description of it.
            "keys_from" => choices
                .iter()
                .map(|choice| {
                    let about = match *choice {
                        "classic" => Some("The function keys, in banks of four"),
                        "mnemonic" => Some(
                            "Control and alt, on the letter of the word -- which a running terminal keeps for its program",
                        ),
                        _ => None,
                    };
                    ((*choice).to_string(), about.map(str::to_string))
                })
                .collect(),
            // The chats Obelus can be reached from, after none: which there
            // are is the remote crate's list, not a row of the table.
            "remote" => std::iter::once(String::new())
                .chain(
                    obelus_remote::platform::ALL
                        .iter()
                        .map(|platform| platform.key.to_string()),
                )
                .map(|key| (key, None))
                .collect(),
            _ => choices
                .iter()
                .map(|choice| ((*choice).to_string(), None))
                .collect(),
        };
        let label = |word: &str| {
            self.called(key, word)
                .map_or_else(|| word.to_string(), std::borrow::Cow::into_owned)
        };
        // The one in force, as the list says it: it finds what to open on
        // by the label rather than the word the file has.
        let preferred = label(word);
        let items: Vec<PickerItem> = choices
            .into_iter()
            .map(|(choice, about)| PickerItem {
                prose: false,
                marker: None,
                icon: None,
                label: label(&choice),
                version: None,
                detail: about,
                trailing: None,
                changed: None,
                value: PickerValue::Setting { key, word: choice },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            })
            .collect();
        // Taller for the workflows, whose choices are a few rows each: the
        // list only ever draws a choice whole, so at the ordinary height the
        // second of two left a blank where it should have been, and the
        // first of three went off the top.
        let rows = match key {
            "workflow" => COMPACT_ROWS * 3,
            _ => COMPACT_ROWS,
        };
        let mut picker = Picker::new(items, PickerLayout::Compact { rows });
        picker.before_typing("Filter values");
        picker.when_empty("This setting has no choices");
        // Read whole, the way the list of conversations is, because what
        // is under each workflow is a few sentences rather than a word --
        // and all of them, because they are Obelus's own. A layout's is
        // one, and its last words are the ones about a terminal.
        if matches!(key, "workflow" | "keys_from") {
            picker.wraps(None);
            picker.details_whole();
        }
        // Opened on the one in force, so the list starts by saying which
        // that is.
        picker.prefer(preferred);
        // What the theme was, for the same reason the theme list keeps it:
        // walking this list wears each colour in turn, and the one that was
        // on is only in the running program.
        if key == "theme" {
            self.theme_before = Some((self.theme_name().to_string(), *self.theme()));
        }
        self.show_list(picker);
    }

    /// Offers one of the agent's settings, as the same compact list.
    ///
    /// With what the agent offers, and with Obelus's own row in front of
    /// it: the third answer these have and Obelus's own settings do not,
    /// which is to say nothing and let the agent open where it opens.
    pub(super) fn open_agent_default(&mut self, setting: &str) {
        let Some(offering) = self.agent_offering() else {
            return;
        };
        let Some(agent) = self.config().agent.clone() else {
            return;
        };
        let Some(offer) = offering.offers.iter().find(|offer| offer.id == setting) else {
            return;
        };
        let chosen = offering.chosen.get(setting).cloned();
        let row = |label: String,
                   detail: Option<String>,
                   value: Option<String>,
                   current: bool|
         -> PickerItem {
            PickerItem {
                prose: false,
                marker: None,
                icon: None,
                label,
                version: None,
                detail,
                // The one in force says so in words, the way the
                // conversation's own list does: a list where the selected
                // row and the current value look alike cannot say which of
                // the two it is showing.
                trailing: current.then(|| "current".to_string()),
                changed: None,
                value: PickerValue::AgentDefault {
                    agent: agent.clone(),
                    setting: setting.to_string(),
                    value,
                },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            }
        };
        let mut items = vec![row(
            "Agent's own".to_string(),
            Some("Whatever it opens a conversation on".to_string()),
            None,
            chosen.is_none(),
        )];
        items.extend(offer.values.iter().map(|value| {
            row(
                value.name.clone(),
                value.about.clone(),
                Some(value.id.clone()),
                chosen.as_deref() == Some(value.id.as_str()),
            )
        }));
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.before_typing("Filter agents");
        picker.ask(&offer.name);
        picker.when_empty("This one has nothing to choose from");
        // Opened on what it is on, so the list starts by saying where the
        // reader is rather than at whatever happens to be first.
        picker.prefer(match &chosen {
            None => "Agent's own".to_string(),
            Some(value) => offer.name_of(value).unwrap_or(value).to_string(),
        });
        self.show_list(picker);
    }

    /// Says what one of the active agent's settings is to start on, or
    /// stops saying.
    ///
    /// Written to the reader's own file and nowhere else: what an agent may
    /// do without being asked is not a thing a downloaded project gets to
    /// decide, which is the same reason the agent itself is the reader's.
    ///
    /// The conversation on screen is not touched. These rows are about the
    /// next one, which is what the heading over them says -- and a change
    /// here reaching into a conversation already under way would be Obelus
    /// answering a question the reader asked about another one.
    pub(super) fn set_agent_default(&mut self, setting: &str, value: Option<&str>) {
        let Some(agent) = self.config().agent.clone() else {
            return;
        };
        self.change_agent_default(&agent, setting, value);
    }

    /// The same, for an agent named outright.
    ///
    /// Named because the choice may come back from a list opened before
    /// the reader changed agents, and a value meant for one agent must not
    /// land on another.
    pub(super) fn change_agent_default(&mut self, agent: &str, setting: &str, value: Option<&str>) {
        match value {
            Some(value) => self
                .settled
                .readers
                .set_agent_default(agent, setting, value),
            None => self.settled.readers.unset_agent_default(agent, setting),
        }
        // Over the project's, the way every other change to the reader's file
        // is -- which here can take nothing away, because no project may set
        // this one.
        self.apply_project();
        let Some(path) = self.settled.path.clone() else {
            return;
        };
        if !self.settled.readable {
            self.wrong("Not saved: the settings will not read".to_string());
            return;
        }
        if let Err(error) = obelus_config::save_to(&path, &self.settled.readers) {
            tracing::warn!(%error, "not saving the configuration");
            self.wrong(format!("Not saved: {error}"));
        }
    }

    /// Says what one of the active agent's variables is, or with `None`
    /// takes it away.
    ///
    /// Nothing is started again: a variable reaches an agent once, when it
    /// starts, and stopping one is stopping every conversation it is in.
    /// So where one is running the status row says when this will arrive,
    /// which is the one thing about these rows a reader cannot see.
    pub(super) fn set_agent_variable(&mut self, name: &str, value: Option<&str>) {
        let Some(agent) = self.config().agent.clone().filter(|id| !id.is_empty()) else {
            return;
        };
        match value {
            Some(value) => self.settled.readers.set_agent_variable(&agent, name, value),
            None => self.settled.readers.unset_agent_variable(&agent, name),
        }
        self.apply_project();
        let Some(path) = self.settled.path.clone() else {
            return;
        };
        if !self.settled.readable {
            self.wrong("Not saved: the settings will not read".to_string());
            return;
        }
        if let Err(error) = obelus_config::save_to(&path, &self.settled.readers) {
            tracing::warn!(%error, "not saving the configuration");
            self.wrong(format!("Not saved: {error}"));
            return;
        }
        let running = self
            .talker
            .as_ref()
            .is_some_and(|talker| talker.id() == agent && talker.is_alive());
        if running && let Some(offering) = self.agent_offering() {
            self.say(format!(
                "Given to {} the next time it starts",
                offering.name
            ));
        }
    }

    /// Hears the name of a variable the reader is adding, and asks for its
    /// value.
    ///
    /// One already set is asked for with what it holds, which makes adding
    /// one that is there the same as changing it rather than a second row
    /// of the same name.
    pub(super) fn name_a_variable(&mut self, name: &str) {
        if name.starts_with(|character: char| character.is_ascii_digit()) {
            self.wrong("A variable's name cannot start with a digit");
            self.ask_on_the_status_row(Prompt::about(PromptKind::VariableName, name.to_string()));
            return;
        }
        let Some(agent) = self.config().agent.clone() else {
            return;
        };
        let now = self
            .config()
            .agent_environment(&agent)
            .get(name)
            .cloned()
            .unwrap_or_default();
        self.ask_on_the_status_row(Prompt::about(PromptKind::Variable(name.to_string()), now));
    }

    /// Where a theme file may be, nearest first.
    ///
    /// The project's own `.obelus/themes` and then the reader's, which is the
    /// order the settings themselves are laid: what a project says about
    /// itself goes over what the reader says about everything. A theme is
    /// only colours -- there is no code in one, and nothing in a file here
    /// can be run -- so a project may hand one over on the same terms it hands
    /// over a wrapped line.
    fn theme_directories(&self) -> Vec<PathBuf> {
        let project = self
            .settled
            .project
            .as_deref()
            .and_then(obelus_theme::written::beside);
        let readers = self
            .settled
            .path
            .as_deref()
            .and_then(obelus_theme::written::beside);
        project.into_iter().chain(readers).collect()
    }

    /// What one of a setting's words is called on screen, where it is
    /// called something other than the word.
    ///
    /// The one answer the list a word is chosen from and the settings row
    /// it goes back to both read, so the two cannot call one choice by two
    /// names. A word like `feature-branch` is the setting's, written in the
    /// reader's file, and a name rather than copy -- and a number of days is
    /// a number in the file and a length of time on the page, because `30`
    /// alone does not say thirty of what. Any number of them, not only the
    /// ones offered: a reader who wrote `14` into the file by hand is owed
    /// the same answer.
    #[must_use]
    pub fn called(&self, key: &str, word: &str) -> Option<std::borrow::Cow<'static, str>> {
        match key {
            "workflow" => super::opening::workflows()
                .find(|(name, ..)| *name == word)
                .map(|(_, title, _)| title.into()),
            // A platform by its name, and none by the page's word for it.
            "remote" => match word {
                "" => Some("Off".into()),
                key => obelus_remote::platform::named(key).map(|platform| platform.name.into()),
            },
            "keys_from" => match word {
                "classic" => Some("Classic".into()),
                "mnemonic" => Some("Mnemonic".into()),
                _ => None,
            },
            // A share of the machine, with what it comes to on this one:
            // the word is the same on every machine the file is read on,
            // and the number is what the reader is actually choosing.
            "build_jobs" => {
                let cpus = logical_cpus();
                match word {
                    "unlimited" => Some("Unlimited".into()),
                    "all" => Some(format!("All {}", of_the(cpus)).into()),
                    "half" => jobs_for(word)
                        .map(|jobs| format!("Half: {jobs} of {}", of_the(cpus)).into()),
                    _ => None,
                }
            }
            "conversation_days" => match word {
                "0" => Some("Never".into()),
                "7" => Some("A week".into()),
                "30" => Some("A month".into()),
                "90" => Some("Three months".into()),
                "365" => Some("A year".into()),
                "1" => Some("1 day".into()),
                days => days
                    .parse::<usize>()
                    .ok()
                    .map(|days| format!("{days} days").into()),
            },
            _ => None,
        }
    }

    /// Every theme there is to choose from, nearest first and without
    /// repeats.
    ///
    /// A file shadows a built-in theme of the same name: it is the reader's
    /// own file and the built-in one is still a rename away, where the other
    /// way round would be a file that quietly did nothing.
    #[must_use]
    pub fn themes(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .theme_directories()
            .iter()
            .flat_map(|directory| obelus_theme::written::found_in(directory))
            .map(|(name, _)| name)
            .collect();
        names.extend(builtin::ALL.iter().map(|(name, _)| (*name).to_string()));
        let mut seen = std::collections::HashSet::new();
        names.retain(|name| seen.insert(name.clone()));
        names
    }

    /// The colours a name stands for, out of the files and then the
    /// built-in ones.
    ///
    /// `None` where nothing answers to it, which is a setting naming a theme
    /// that has been renamed or deleted: the colours on screen stay as they
    /// are, because a reader who cannot read the screen cannot fix the file.
    #[must_use]
    pub fn theme_called(&mut self, name: &str) -> Option<Theme> {
        let Some(path) = self.theme_file(name) else {
            return builtin::by_name(name).copied();
        };
        match obelus_theme::written::read_where(&path) {
            Ok(theme) => {
                self.nothing_wrong_with(&path);
                Some(theme)
            }
            Err(wrong) => {
                tracing::warn!(why = wrong.why, "a theme that would not read");
                // Not opening on the name, which a sentence may not.
                self.wrong(format!("The theme `{name}` will not read"));
                // And on the theme's own file, where the line is. The
                // colours on screen stay as they are: a reader who cannot
                // read the screen cannot fix the file.
                self.nothing_wrong_with(&path);
                self.obelus_says(
                    &path,
                    wrong.at,
                    obelus_lsp::trouble::Severity::Error,
                    &format!("This theme will not read\n{}", wrong.why),
                );
                None
            }
        }
    }

    /// The file a name is written in, where one is.
    #[must_use]
    fn theme_file(&self, name: &str) -> Option<PathBuf> {
        self.theme_directories().into_iter().find_map(|directory| {
            obelus_theme::written::found_in(&directory)
                .into_iter()
                .find(|(called, _)| called == name)
                .map(|(_, path)| path)
        })
    }

    /// The directories a change to the theme in force could arrive in.
    ///
    /// The two themes are looked for in, and wherever the one in force
    /// really lives. That last one is the whole reason this is not simply
    /// the first two: a theme file is often a link into a directory
    /// something else owns -- a dotfiles repository, or a desktop that
    /// themes every program it has -- and what rewrites it rewrites that
    /// directory, which a watch on the link's own would never hear about.
    pub(super) fn theme_watches(&self) -> Vec<PathBuf> {
        let mut directories = self.theme_directories();
        if let Some(path) = self.theme_file(&self.settled.config.theme)
            && let Some(parent) = obelus_config::resolved(&path).parent()
        {
            directories.push(parent.to_path_buf());
        }
        directories
    }

    /// Whether a path that changed is one of the theme's.
    pub(super) fn is_a_theme(&self, path: &Path) -> bool {
        self.theme_watches()
            .iter()
            .any(|directory| path.starts_with(directory))
    }

    /// Watches wherever a change to the theme would arrive, giving up
    /// whatever was being watched for it before.
    ///
    /// Taken up again on every re-read rather than once at startup, because
    /// the directory a theme really lives in can be *replaced* -- which is
    /// how a desktop swaps a whole theme at once, and which leaves the watch
    /// pointing at a directory nothing will ever write to again.
    pub(super) fn watch_theme(&mut self) {
        let wanted = self.theme_watches();
        let held = std::mem::take(&mut self.theme_watched);
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        for directory in held {
            watcher.unwatch_directory(&directory);
        }
        for directory in &wanted {
            if let Err(error) = watcher.watch_directory(directory) {
                tracing::debug!(%error, directory = %directory.display(), "not watching for themes");
            }
        }
        self.theme_watched = wanted;
    }

    /// Reads the theme in force again, because its file has changed.
    ///
    /// The name in the settings has not moved -- nobody chose anything --
    /// and what that name stands for has. Which is the whole of what a
    /// desktop that themes every program it has does to Obelus: it writes
    /// the file, and Obelus is wearing the colours a moment later without
    /// anybody having to tell it.
    pub(super) fn reread_theme(&mut self) {
        let called = self.settled.config.theme.clone();
        match self.theme_called(&called) {
            Some(theme) => {
                self.settled.no_theme = None;
                self.set_theme(&called, theme);
            }
            // Said where the file is read, like the one above it.
            None => self.settled.no_theme = Some(called),
        }
        // And wherever it lives now, which a swap of the whole directory has
        // just moved out from under the old watch.
        self.watch_theme();
    }

    /// Applies a setting the settings view changed, and writes the file.
    ///
    /// Applied first and saved second, so a file that cannot be written
    /// still leaves the reader with the setting they asked for until they
    /// restart -- and with a note saying it will not last.
    pub(super) fn change_setting(&mut self, key: &'static str, value: &obelus_config::Value) {
        // Which file this change is for is the page's own question: the
        // reader's settings, or the project's. Asked here rather than carried
        // in the outcome, because it is a fact about what is open and not
        // about which key was pressed.
        if self.settings.as_ref().is_some_and(Settings::on_project) {
            self.write_to_project(key, Some(value));
            return;
        }
        if let Some(path) = self.pinned_by(key) {
            // Nothing happens, and nothing needs saying: the row itself
            // carries the name of the file that has it, in the dim ink
            // every unusable thing here is drawn in.
            tracing::debug!(key, path = %path.display(), "the project has this one");
            return;
        }
        // The reader's own layer, and then the project's back over it: a
        // setting they change is theirs, and what the project has is still the
        // project's.
        self.settled.readers.set(key, value);
        self.apply_project();
        let Some(path) = self.settled.path.clone() else {
            // Nobody said where the file is, so there is nothing to write
            // to: an application that never read one does not write one.
            return;
        };
        if !self.settled.readable {
            // Said when it was found to be unreadable, and again here,
            // because this is the moment the reader finds out their change
            // is not being kept.
            self.wrong("Not saved: the settings will not read".to_string());
            return;
        }
        if let Err(error) = obelus_config::save_to(&path, &self.settled.readers) {
            tracing::warn!(%error, "not saving the configuration");
            self.wrong(format!("Not saved: {error}"));
        }
    }

    /// Takes a setting out of the project's file, from the project's own page.
    pub(super) fn unset_setting(&mut self, key: &'static str) {
        self.write_to_project(key, None);
    }

    /// Writes one key to the project's settings, or takes it out.
    ///
    /// Making the file if the project has none: a reader who has opened the
    /// project's settings and changed something has said plainly enough that
    /// this project should have them.
    ///
    /// What a project may not set is refused here as well as when the file is
    /// read. Refused rather than written and then ignored, which would be a
    /// file that says something Obelus will not do.
    fn write_to_project(&mut self, key: &'static str, value: Option<&obelus_config::Value>) {
        if obelus_config::reach_of(key) != obelus_config::Reach::Anywhere {
            tracing::debug!(key, "a project may not set this one");
            return;
        }
        let path = obelus_config::project_path_for(&self.working_directory);
        if let Err(error) = obelus_config::write_project(&path, key, value) {
            tracing::warn!(%error, path = %path.display(), "not writing the project's settings");
            self.wrong(format!("Not saved: {error}"));
            return;
        }
        // Read back the way any other change to that file arrives, so the
        // page shows what the file says rather than what Obelus meant to
        // put in it.
        self.reread_config();
    }

    /// Opens the settings file itself, for a reader who would rather see
    /// them all at once -- or edit one Obelus has no control for.
    ///
    /// Written first if it is not there yet, because the file Obelus would
    /// write is the answer to "what are the settings": a reader sent to a
    /// path that does not exist has been told nothing, and the file with
    /// every default in it is what they need in front of them to change one
    /// by hand.
    ///
    /// Read, not applied. Obelus reads this file when it starts and writes
    /// it when the reader changes something on the settings page; a change
    /// made in it by hand is picked up the next time Obelus starts.
    pub fn open_config_file(&mut self) {
        let Some(path) = self.settled.path.clone() else {
            self.wrong("This system has nowhere for a settings file".to_string());
            return;
        };
        if !path.exists()
            && let Err(error) = obelus_config::save_to(&path, &self.settled.config)
        {
            tracing::warn!(%error, "not writing the configuration");
            self.wrong(format!("No settings file, and none written: {error}"));
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
    /// spellings and keycodes would be Obelus's own business rather than
    /// something a reader can edit.
    pub(super) fn rebind(&mut self, command: obelus_command::Command, chord: Option<KeyChord>) {
        let written = chord.map(|chord| chord.label_in(false)).unwrap_or_default();
        self.settled
            .config
            .keys
            .insert(command.name().to_string(), written);
        self.keymap.rebind(command, chord);
        let Some(path) = self.settled.path.clone() else {
            return;
        };
        if let Err(error) = obelus_config::save_to(&path, &self.settled.config) {
            tracing::warn!(%error, "not saving the configuration");
            self.wrong(format!("Not saved: {error}"));
        }
    }

    /// Makes the running program match the configuration.
    ///
    /// One place, called at startup and after every change, so a setting
    /// cannot mean one thing on the way in and another when it is edited.
    fn apply_config(&mut self) {
        let called = self.settled.config.theme.clone();
        match self.theme_called(&called) {
            Some(theme) => {
                self.settled.no_theme = None;
                self.set_theme(&called, theme);
            }
            // Written down rather than said, for the same reason a key
            // that would not bind is: this runs after every change, and a
            // reader flipping a switch has not renamed their theme.
            None => self.settled.no_theme = Some(called),
        }
        // A theme chosen is a theme living somewhere else, so what is
        // watched for a change to it moves with it.
        self.watch_theme();
        // The switch is a terminal's: a window carries the face the marks
        // are in, so they are drawn there whatever a file written on
        // another machine says -- and the settings page does not offer the
        // row at all. See `obelus_config::Drawn`.
        obelus_icons::use_glyphs(self.settled.config.icons || obelus_config::in_a_window());
        obelus_text::lay_tabs_at(self.settled.config.tab_width);
        // And the one setting the application cannot act on itself: how
        // big the text is, which means something only where Obelus draws
        // its own pixels and is that front end's to do something about.
        if let Some(drawing) = self.drawing.as_ref() {
            drawing.text_size(self.settled.config.font_size);
            drawing.use_fonts(&self.settled.config.fonts);
            drawing.animates(self.settled.config.animation);
        }
        // The table the reader's own bindings leave. Built rather than
        // patched: what is in the file is a list of changes over the
        // defaults, and applying them to a table that has already had them
        // applied would leave a rebind that was undone in the file still in
        // force.
        // A layout nothing answers to has already been marked where the
        // file was read, and starts from the default like a missing line.
        let layout = obelus_editing::keymap::Layout::named(&self.settled.config.keys_from)
            .unwrap_or_default();
        let (keymap, unbound) =
            obelus_editing::keymap::Keymap::with(layout, &self.settled.config.keys);
        self.keymap = keymap;
        self.settled.unbound = unbound;
        // What a server works out is drawn or it is not, and the switch has
        // to reach the screen either way: turned off it takes what is
        // already drawn away, and turned on it asks for what was never
        // asked for.
        self.hints_switched();
        // A switch turned on is a question asked now. Nothing before the
        // loop has a channel, so on the way up this waits for `start`.
        self.ask_about_releases();
        self.settle_the_pool();
    }

    /// Puts this Obelus in the machine's pool of build jobs at the size the
    /// settings ask for, or takes it out.
    ///
    /// Not before the loop, like the releases: what is never started --
    /// a test that lays a screen out and reads it -- is never in a pool,
    /// and starts nothing that would be told about one. Asked again after
    /// every change, which costs a lock and a short file when nothing
    /// moved.
    ///
    /// What a pool that will not be made costs is the pool: the reason goes
    /// in the log and every build sizes itself, which is what happened
    /// before there was one.
    pub(super) fn settle_the_pool(&mut self) {
        if self.events.is_none() {
            return;
        }
        let Some(jobs) = jobs_for(&self.settled.config.build_jobs) else {
            self.jobs = None;
            return;
        };
        if let Some(pool) = &self.jobs {
            if let Err(error) = pool.resize(jobs) {
                tracing::warn!(%error, jobs, "the pool of build jobs would not change size");
            }
            return;
        }
        let Some(state) = obelus_logging::state_directory() else {
            tracing::info!("nowhere to keep a pool of build jobs, so none");
            return;
        };
        match obelus_jobs::Pool::join(&state.join("jobs"), jobs) {
            Ok(pool) => self.jobs = Some(pool),
            Err(error) => tracing::warn!(%error, "no pool of build jobs"),
        }
    }

    /// Reads the configuration file and applies it.
    ///
    /// Separate from [`App::new`] so that a test gets the defaults rather
    /// than whatever the machine it runs on has in `~/.config`.
    pub fn load_config(&mut self) {
        self.settled.path = obelus_config::path();
        if self.settled.path.is_none() {
            tracing::info!("nowhere to keep settings, so the defaults");
            return;
        }
        self.read_the_settings();
    }

    /// Reads whatever [`Settled::path`] names, and applies it.
    ///
    /// The whole of what reading the reader's settings does, so that a
    /// test pointing Obelus at a file of its own goes down the same road
    /// as a reader -- including the road a file that will not read takes,
    /// which is the one worth testing and was the one a test could not
    /// reach.
    fn read_the_settings(&mut self) {
        let Some(path) = self.settled.path.clone() else {
            return;
        };
        // Which file, and what was in it: "my setting did nothing" is
        // answered by the path Obelus actually read, and a reader with two
        // machines or an `XDG_CONFIG_HOME` has more than one candidate.
        match obelus_config::read_from(&path) {
            obelus_config::Reading::Settings {
                config,
                named,
                ignored,
                spans,
            } => {
                tracing::info!(path = %path.display(), settings = ?named, "read the settings");
                self.nothing_wrong_with(&path);
                self.lines_that_did_nothing(&path, &ignored);
                // After, because what the keymap would not take is worked
                // out while the config is being applied.
                self.configure(*config, named);
                self.what_the_settings_could_not_use(&path, &spans);
            }
            obelus_config::Reading::Nothing | obelus_config::Reading::Nowhere => {
                tracing::info!(path = %path.display(), "no settings file yet, so the defaults");
            }
            obelus_config::Reading::Unreadable(why, at) => {
                self.settings_unreadable(&path, &why, at);
            }
        }
        self.apply_project();
    }

    /// Lays the project's own settings over the reader's.
    ///
    /// After theirs, every time theirs is read: the project is the narrower
    /// answer -- it is about *this* project -- so it wins where it says
    /// anything, and says nothing everywhere else.
    ///
    /// A file that will not read is a line in the log and nothing more. The
    /// reader's settings are what Obelus has, and throwing them away because
    /// a project somebody else wrote has a typo in it would be the project
    /// deciding something it was never given.
    pub(super) fn apply_project(&mut self) {
        // From the reader's own answers up, every time. Laid over what is
        // already there instead, a setting the project has *stopped* naming
        // would stay in force: nothing would have put the reader's answer
        // back underneath it, and deleting a line from the project's file
        // would do nothing until Obelus was started again.
        self.settled.config = self.settled.readers.clone();
        self.settled.pinned.clear();
        self.settled.project = obelus_config::project_path(&self.working_directory);
        let Some(path) = self.settled.project.clone() else {
            self.apply_config();
            return;
        };
        match obelus_config::read_table(&path) {
            Ok(Some((table, text))) => {
                let applied = obelus_config::apply(
                    &mut self.settled.config,
                    &table,
                    obelus_config::Whose::Project,
                );
                self.settled.pinned = applied.set;
                self.nothing_wrong_with(&path);
                self.lines_that_did_nothing(&path, &obelus_config::placed(applied.ignored, &text));
                tracing::info!(
                    path = %path.display(),
                    settings = ?self.settled.pinned,
                    "the project has settings of its own",
                );
            }
            Ok(None) => {}
            Err(why) => {
                tracing::warn!(path = %path.display(), why, "the project's settings will not read");
            }
        }
        self.apply_config();
    }

    /// The reader's own settings, under whatever the project lays over them.
    #[must_use]
    pub const fn readers_config(&self) -> &obelus_config::Config {
        &self.settled.readers
    }

    /// The settings the project has set, which are the ones the reader cannot
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

    /// The project's own settings file, while the project has one.
    #[must_use]
    pub fn project_config(&self) -> Option<&Path> {
        self.settled.project.as_deref()
    }

    /// Which file has this setting, if it is not the reader's to change.
    #[must_use]
    pub fn pinned_by(&self, key: &str) -> Option<&Path> {
        self.settled
            .pinned
            .contains(&key)
            .then_some(self.settled.project.as_deref())
            .flatten()
    }

    /// Takes the settings file as it stands now, because somebody else
    /// changed it.
    ///
    /// Another Obelus on the same project, or the reader's own editor: what
    /// is in the file is what Obelus is set to, whichever process wrote it.
    /// Only the settings, not [`App::configure`]'s second half -- that puts
    /// every open file back to the reading the settings ask for, and a
    /// reader who has turned a preview off should not have it come back
    /// because somebody in another window changed the theme.
    pub(super) fn reread_config(&mut self) {
        if let Some(path) = self.settled.path.clone() {
            match obelus_config::read_from(&path) {
                obelus_config::Reading::Settings {
                    config,
                    named,
                    ignored,
                    spans,
                } => {
                    tracing::info!(path = %path.display(), "the settings changed under us");
                    self.nothing_wrong_with(&path);
                    self.lines_that_did_nothing(&path, &ignored);
                    self.settled.readers = (*config).clone();
                    self.settled.named = named;
                    self.settled.config = *config;
                    self.apply_config();
                    self.what_the_settings_could_not_use(&path, &spans);
                    self.settled.readable = true;
                }
                // Gone, which is somebody deleting it or an editor writing
                // it in a way Obelus caught mid-flight. Neither is a reason
                // to throw away what this session is set to.
                obelus_config::Reading::Nothing | obelus_config::Reading::Nowhere => {}
                obelus_config::Reading::Unreadable(why, at) => {
                    self.settings_unreadable(&path, &why, at);
                }
            }
        }
        // And the project's over the top, from the reader's file up: a layer
        // laid over what already has it would keep a setting the project has
        // since stopped naming, because nothing would have put the reader's
        // own answer back underneath it.
        self.apply_project();
    }

    /// Marks the lines of a settings file that did nothing.
    ///
    /// The words are written here and the facts come from
    /// [`obelus_config`], which is the split it keeps everywhere: what
    /// could not be made of a file is a fact about the file, and what to
    /// say about it to a reader is copy. A line that did nothing is a
    /// warning and not an error -- the file read, and everything else in
    /// it took.
    fn lines_that_did_nothing(&mut self, path: &Path, ignored: &[obelus_config::Ignored]) {
        for one in ignored {
            let said = match &one.why {
                // Never starting with the name, which is the rule every
                // sentence with one in it follows: `Nothing is bound to
                // open-file`, not `open-file has no key`.
                //
                // And the name in backticks, the way a compiler writes one
                // in the middle of a sentence: it is what the reader wrote
                // and what they will look for in the file, and among the
                // words around it a name like `wrap` reads as one of them.
                // Not the setting where it names the kind of thing the word
                // was meant to be -- `No theme is called` -- which is the
                // sentence's own noun.
                obelus_config::Why::NoSuchSetting => {
                    format!("No setting is called `{}`", one.key)
                }
                obelus_config::Why::NotForAProject => {
                    format!("A project may not set `{}`", one.key)
                }
                obelus_config::Why::NotATable => {
                    format!(
                        "What `{}` is set to is not a table of its settings",
                        one.key
                    )
                }
                // Not "No workflow is called ...", which opens on the title
                // of one of the workflows. Which setting it is goes without
                // saying: the mark is on the line that sets it.
                obelus_config::Why::NoSuchChoice(word) => {
                    format!("Nothing answers to `{word}`")
                }
            };
            self.obelus_says(path, one.at, obelus_lsp::trouble::Severity::Warning, &said);
        }
    }

    /// Marks the lines the settings named that Obelus could not use: a
    /// theme nothing answers to, and a key table's lines that bound
    /// nothing.
    ///
    /// Said where the file is read rather than where the keymap is built,
    /// because the keymap is built again after every change and a reader
    /// flipping a switch has not touched their key table.
    ///
    /// Under the table they are in (`keys.open-file`), which is how the
    /// file names them and the only way to say which of several lines is
    /// the one that is wrong.
    fn what_the_settings_could_not_use(
        &mut self,
        path: &Path,
        spans: &std::collections::BTreeMap<String, obelus_text::coordinates::Span>,
    ) {
        if let Some(called) = self.settled.no_theme.clone() {
            // Never starting with the name: `dark` is a theme's own
            // spelling and a sentence may not open on one. In backticks,
            // as every name these sentences carry is.
            self.obelus_says(
                path,
                spans.get("theme").copied(),
                obelus_lsp::trouble::Severity::Warning,
                &format!("No theme is called `{called}`"),
            );
        }
        for one in std::mem::take(&mut self.settled.unbound) {
            let said = match one.why {
                // Never starting with the name, and saying what Obelus did
                // rather than what the reader wrote: what is left to say is
                // the thing they cannot see.
                obelus_editing::keymap::Unbindable::NoSuchCommand => {
                    format!("No command is called `{}`", one.name)
                }
                obelus_editing::keymap::Unbindable::Unreadable => {
                    format!(
                        "Nothing is bound to `{}`: `{}` is not a key",
                        one.name, one.text
                    )
                }
                // The reason the page that binds keys gives, in its own
                // words, because it is the same judgement.
                obelus_editing::keymap::Unbindable::NotAllowed(why) => {
                    format!("Nothing is bound to `{}`: {why}", one.name)
                }
            };
            let at = spans.get(&format!("keys.{}", one.name)).copied();
            self.obelus_says(path, at, obelus_lsp::trouble::Severity::Warning, &said);
        }
    }

    /// Says the settings file cannot be read, and stops writing to it.
    ///
    /// What is in it is the reader's, and Obelus cannot read it: saving
    /// over it would replace settings it never saw with whatever this
    /// session happens to be set to. So nothing is saved until it reads
    /// -- which it will, the moment somebody fixes the file, because the
    /// watcher is on it.
    fn settings_unreadable(
        &mut self,
        path: &Path,
        why: &str,
        at: Option<obelus_text::coordinates::Span>,
    ) {
        tracing::warn!(path = %path.display(), why, "the settings file will not read");
        // And on the file itself, where a reader who opens it sees the
        // line rather than a number they have to go and count to. The
        // whole of the parser's words go in the message, under Obelus's
        // own sentence: the first line is what a list of problems shows,
        // and `TOML parse error at line 14` is not a sentence about what
        // Obelus did.
        self.nothing_wrong_with(path);
        self.obelus_says(
            path,
            at,
            obelus_lsp::trouble::Severity::Error,
            &format!("The settings will not read, so none are saved\n{why}"),
        );
        self.settled.readable = false;
        // Short, because the status row is one row and shares it with the
        // file and the position: which file and what went wrong are in the
        // log, where there is room for them.
        self.wrong("The settings will not read, so none are saved".to_string());
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
    pub fn configure(&mut self, config: obelus_config::Config, named: Vec<&'static str>) {
        // The reader's own layer, which is what the project's is laid over --
        // and what a setting goes back to when the project stops naming it.
        self.settled.readers = config.clone();
        self.settled.named = named;
        self.settled.config = config;
        self.apply_config();
        self.apply_project();
    }

    /// Reads and writes settings at a path of the caller's choosing.
    ///
    /// For a test: the reader's own file is not something a test may write
    /// to, and a test of "does changing this save it" has to have a file.
    pub fn config_file_for_test(&mut self, path: PathBuf) {
        // Where the file is, before what is in it: applying a setting can
        // send Obelus looking beside that file for something -- a theme is
        // in the directory next to it -- and a path set afterwards is a
        // path that was not there when it was needed. The real way in sets
        // it first for the same reason.
        self.settled.path = Some(path);
        self.settled.readable = true;
        self.read_the_settings();
    }
}

/// How many jobs at once a share of the machine comes to, or none for no
/// pool at all.
fn jobs_for(share: &str) -> Option<usize> {
    match share {
        "all" => Some(logical_cpus()),
        "half" => Some((logical_cpus() / 2).max(1)),
        _ => None,
    }
}

/// How many CPUs this machine has, as a build counts them when it sizes
/// itself.
fn logical_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// A number of CPUs, as a sentence says it.
fn of_the(cpus: usize) -> String {
    match cpus {
        1 => "1 CPU".to_string(),
        cpus => format!("{cpus} CPUs"),
    }
}

#[cfg(test)]
mod tests {
    /// One CPU is said in the singular, on the one machine that has it.
    ///
    /// Deliberate break: take out the arm for one, and the page says
    /// `All 1 CPUs`.
    #[test]
    fn one_cpu_is_one_cpu() {
        assert_eq!(super::of_the(1), "1 CPU");
        assert_eq!(super::of_the(24), "24 CPUs");
    }
}
