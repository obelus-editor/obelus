//! What the reader has decided, and where it is kept.
//!
//! One flat file of `key = value` lines, written whole every time anything
//! changes. Whole rather than edited in place because there is nothing in it
//! worth preserving that obelus does not know about -- no comments it wrote,
//! no ordering it chose -- and a rewrite cannot half-apply.
//!
//! Missing, unreadable, or nonsense all mean the same thing: the defaults.
//! A reader whose config file has a typo in it should get obelus, not an
//! error message where their editor was.
//!
//! The settings are a *table* ([`ALL`]), the way the commands are: a setting
//! is a row with a name, a group, a kind of control and a way to read and
//! write it. The settings view is built from that table and knows nothing
//! about any particular setting.
//!
//! What a project may set is a property of the setting, `Reach`, not a list of
//! exceptions somewhere: the next setting a stranger should not be trusted with
//! will be found by asking that question while writing the setting down.
//! `agent` is `ReaderOnly` because it says which agent obelus *starts*, and a
//! program starting because a file in a downloaded project said so is a
//! decision that belongs to the person at the keyboard; `keys` is `ReaderOnly`
//! because a project that could rebind them could put `quit` where a reader
//! would find it by accident; and `agents` is `ReaderOnly` for the first reason
//! twice over -- what an agent may do without asking is the setting a
//! downloaded project would most like to write. VS Code learned this one the
//! same way and calls it `machine` scope.
//!
//! The configuration file holds preferences, not state. `config.rs` is the
//! whole of it — one table, `dirs` for where it lives, written the moment
//! anything changes. Rebindable keys are still guaranteed by the key table
//! being *data* rather than by the file. What is not a preference does not go
//! in it: how to start an installed agent is written beside the install, not
//! here.

use std::path::{Path, PathBuf};

/// The theme a reader who has not chosen one gets.
///
/// Here rather than beside the themes themselves because it is a written
/// default like every other field below it -- the name of a theme, not a
/// theme -- and the list of names obelus will accept is already next door.
pub const DEFAULT_THEME: &str = "dark";

/// Everything the reader can decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Which built-in theme to use, by name.
    pub theme: String,
    /// Whether to draw Nerd Font glyphs.
    pub icons: bool,
    /// Whether the margin says who last changed each line.
    ///
    /// About the margin, not about blame: whether the commit behind one
    /// line can be asked for is a different question, and a reader who
    /// wants no names beside their code has not said never to ask it.
    pub blame_margin: bool,
    /// Whether a line too long for the screen continues on the next row.
    pub wrap: bool,
    /// How wide a tab is drawn, and how many spaces the tab key puts in.
    pub tab_width: usize,
    /// Whether to ask a language server to lay the file out before writing.
    pub format_on_save: bool,
    /// Whether to make the server's whole-file fixes before writing.
    ///
    /// The protocol's `source.` code actions: what a server offers to do
    /// to a file rather than to a place in one -- the imports sorted, the
    /// corrections it can make on its own. They are the only actions
    /// obelus takes without a key being pressed, which is why they are the
    /// only ones it asks for by name.
    pub code_actions_on_save: bool,
    /// Whether what a language server works out is drawn in the file.
    ///
    /// A type nobody wrote down, the name of the parameter an argument is
    /// passed to. On, because the thing a reader wants to know about a
    /// `let` with no type on it is the type -- and a switch nobody finds
    /// is a feature nobody has. They are cells the file does not contain,
    /// which is what the switch is for: turned off, the file on disk is
    /// what is on screen, to the column.
    pub inlay_hints: bool,
    /// Whether what a server says is wrong with the line the caret is on is
    /// opened under it.
    ///
    /// The underline is not this switch: what is wrong is marked on every
    /// line that has something wrong with it, always, and costs no room.
    /// This is the *words*, which take a row of the file's own space -- so
    /// a reader who would rather keep the shape of the code and read the
    /// complaint from the list can have that.
    pub diagnostics: bool,
    /// Whether the file list offers the files a project has said to ignore.
    ///
    /// About the list, not about the files: what `.gitignore` keeps out is
    /// kept out of the *offer*, and a reader who knows the path can still
    /// open one. Which is why this is worth a key as well as a switch --
    /// "where is that build log" is a question asked once and then not
    /// again for a week.
    pub ignored_files: bool,
    /// How long the pointer has to rest on a word before obelus asks what
    /// it is, in milliseconds.
    ///
    /// Zero is off: the pointer then asks nothing, and `alt+h` is the way
    /// in. A time rather than a switch because the answer people want
    /// differs by more than on and off -- a reader who knows the code
    /// wants it slow enough never to appear by accident, and one reading
    /// somebody else's wants it as fast as their hand stops.
    pub hover_delay: usize,
    /// Which agent obelus talks to, by the registry's own name for it.
    ///
    /// One, or none. Two would mean every question having to say which
    /// agent it was for, and a reader having to know.
    pub agent: Option<String>,
    /// The keys the reader has moved, by the command's own name.
    ///
    /// Changes rather than the whole table: a reader who rebinds one key
    /// should be given the new default for everything they said nothing
    /// about. An empty chord is a key taken away, which is also a decision.
    pub keys: std::collections::BTreeMap<String, String>,
    /// What each agent is to start a conversation on, in its own words.
    ///
    /// The agent's id, then the agent's id for one of the settings it
    /// offers, then the agent's id for the value. Obelus understands none
    /// of the three and is not meant to: which settings an agent has is
    /// the agent's, and this is the reader saying which of them they would
    /// rather not choose again every time.
    ///
    /// Only what they have said. A setting that is not in here is one
    /// obelus says nothing about, and the conversation starts on whatever
    /// the agent starts it on -- which is a different thing from starting
    /// on the value the agent happened to be on last time, and the reason
    /// this is a map of what was said rather than a copy of a session.
    pub agents: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            // Off, because a patched font is a thing the reader has to have
            // gone and got, and whether they have cannot be asked: a
            // default that assumes it draws a box beside every name for
            // everybody who has not, and a box is how obelus looks broken.
            // Without the font, the fallbacks read correctly; with it, one
            // switch turns the glyphs on.
            icons: false,
            blame_margin: true,
            // Off, so a line is a line: a reader counting rows, comparing
            // two files side by side, or looking at a table in a comment is
            // reading something the screen has not rearranged. The reader
            // who wants it can say so, and then it is a line's own choice
            // no longer.
            wrap: false,
            // What the code obelus is written in uses, which is also what
            // the rest of the program laid a tab out at before a reader
            // could say otherwise.
            tab_width: obelus_text::TAB_WIDTH,
            // Off: a formatter that ran without being asked would rewrite
            // a file somebody opened to read, and the first they would know
            // of it is the diff.
            format_on_save: false,
            // Off, for the reason above it: a file somebody opened to read
            // should not come back from a save with its imports rearranged
            // and three lines somewhere else rewritten.
            code_actions_on_save: false,
            inlay_hints: true,
            diagnostics: true,
            // Off, because a project says what it ignores and mostly means it:
            // a list whose first hundred rows are `target` is a list nobody
            // can find anything in.
            ignored_files: false,
            // Long enough that crossing a line of code does not ask about
            // every word on the way, short enough that a reader who has
            // stopped does not wonder whether obelus noticed. The figure
            // every editor with a mouse uses.
            hover_delay: 400,
            // None until the reader installs one: obelus does not choose an
            // agent for anybody.
            agent: None,
            // Nothing moved: the table obelus ships with.
            keys: std::collections::BTreeMap::new(),
            // And nothing said about any agent: every conversation starts
            // where the agent starts it.
            agents: std::collections::BTreeMap::new(),
        }
    }
}

/// A setting's value, in whichever shape its control has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// On or off.
    Switch(bool),
    /// One of a list of words.
    Choice(String),
    /// A number.
    ///
    /// Its own thing rather than a `Choice` that happens to spell a number:
    /// a reader opening the file sees `tab_width = 4` rather than `"4"`, and
    /// what uses it wants a number rather than a parse.
    Count(usize),
}

/// What sort of control a setting gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Switch,
    /// One of a fixed list of words.
    Choice(&'static [&'static str]),
    /// A number, offered as the few that anybody picks.
    ///
    /// The same control a choice gets, because picking from a short list is
    /// what a reader is doing either way -- and a spinner for a number with
    /// three sensible values is a control nobody needs.
    Count(&'static [&'static str]),
}

/// Which group of settings a setting belongs to.
///
/// Coarse on purpose: a reader looking for a setting scans one screen, and
/// a dozen groups of two rows each is a worse index than two groups of a
/// dozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// How obelus looks.
    Appearance,
    /// What it says about the file being read.
    Reading,
    /// Which files obelus offers, and where it looks for them.
    Files,
}

impl Group {
    /// Every group, in the order their tabs sit in.
    pub const ALL: [Self; 3] = [Self::Appearance, Self::Reading, Self::Files];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Reading => "Reading",
            Self::Files => "Files",
        }
    }
}

/// Who may set a setting.
///
/// A project's own settings are written by whoever wrote the project, and a
/// reader who opens somebody's repository has not agreed to everything in it.
/// Most of these are harmless to hand over -- a theme, a wrapped line, a name
/// in the margin -- and some are not: `agent` says which agent obelus starts,
/// and a program starting because a file in a downloaded project said so is a
/// decision that belongs to the person at the keyboard. The keys are the
/// same: a project that could rebind them could put a reader's `quit` somewhere
/// they would find by accident.
///
/// A kind rather than a list of exceptions, because the next one of these
/// will be found the way this one was -- by asking, of a new setting,
/// whether a stranger may set it -- and the asking should be part of writing
/// the setting down rather than something to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Either file: the reader's own, or the project's.
    Anywhere,
    /// The reader's own file alone. A project naming it is ignored, with a word
    /// in the log for whoever wrote that file.
    ReaderOnly,
}

/// Which file a table of settings came out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The reader's, wherever this system keeps such things.
    Reader,
    /// The project obelus was opened on.
    Project,
}

/// One setting, as data.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    /// What it is called in the file.
    pub key: &'static str,
    /// What the view calls it: one word where one will do.
    ///
    /// A *name*, not a sentence. These were sentences -- "Colour theme",
    /// "Who last changed the line the cursor is on" -- on the grounds that a
    /// name and a description side by side read as a heading and a footnote.
    /// Which is true when the footnote says what the heading already had;
    /// what it actually produced was a column of prose, where a reader
    /// looking for the one row they came for had to read every row to find
    /// it. A column of names is scanned. The palette has said a name and a
    /// description side by side since the beginning, and this is that row.
    pub name: &'static str,
    /// What it does, in the dim colour after the name.
    ///
    /// Empty where the name is the whole of it: `Theme` needs no gloss, and
    /// a line of prose saying "the colour theme" beside it is the footnote
    /// that objection was about.
    pub about: &'static str,
    /// Which tab it lives under.
    pub group: Group,
    /// What sort of control it gets.
    pub kind: Kind,
    /// Which files may set it.
    pub reach: Reach,
}

impl Setting {
    /// Whether a table out of `whose` file may set this.
    #[must_use]
    pub fn settable_by(&self, whose: Whose) -> bool {
        whose == Whose::Reader || self.reach == Reach::Anywhere
    }

    /// The setting a key names, if obelus has one.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        ALL.iter().find(|setting| setting.key == key)
    }
}

/// The themes a reader can choose between.
const THEMES: &[&str] = &["dark", "light"];

/// The tab widths anybody sets.
const WIDTHS: &[&str] = &["2", "4", "8"];

/// How long a rest is, in milliseconds, as the few anybody picks.
///
/// Zero is the list's way of saying "not at all": a fourth control meaning
/// off, beside a list that already has a slowest, is a second way to say
/// the same thing.
const DELAYS: &[&str] = &["0", "200", "400", "800"];

/// Every setting obelus has.
pub const ALL: &[Setting] = &[
    Setting {
        key: "theme",
        name: "Colour theme",
        about: "The colours obelus draws in",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Choice(THEMES),
    },
    Setting {
        key: "icons",
        name: "Nerd Font glyphs",
        about: "in lists, on the status bar, and beside a file's name -- a terminal without a patched font draws a box instead",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "wrap",
        name: "Wrap long lines",
        about: "A line too long for the screen carries onto the next row, broken between words",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "blame_margin",
        name: "Blame in the margin",
        about: "Who last changed the line the cursor is on",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "tab_width",
        name: "Tab width",
        about: "How wide a tab is drawn, and how many spaces one puts in",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Count(WIDTHS),
    },
    Setting {
        key: "hover_delay",
        name: "Ask on a rest",
        about: "how long the pointer has to rest on a word before obelus says what it is, in milliseconds -- zero asks only when a key does",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Count(DELAYS),
    },
    Setting {
        key: "format_on_save",
        name: "Format when saving",
        about: "Ask the language server to lay the file out before writing it",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "code_actions_on_save",
        name: "Server fixes when saving",
        about: "before writing, make the changes the language server offers for the whole file -- the imports sorted, the corrections it can make on its own. Servers differ in how much of this they do and some offer none",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "inlay_hints",
        name: "What the server works out",
        about: "draw the types and parameter names a language server infers, in the places they would be written. They are not in the file: nothing in one can be selected or copied, and turning this off puts every column back where the file has it",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "diagnostics",
        name: "What a server says is wrong",
        about: "open the words under the line the caret is on. What is wrong is underlined on every line either way; this is whether the complaint itself is read where it is, which costs that line a row of the file's own space",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "ignored_files",
        name: "Files a project ignores",
        about: "offer them in the file list as well -- what `.gitignore` keeps out is build output most days and the file you are looking for on the others",
        group: Group::Files,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
];

impl Config {
    /// What a setting is set to.
    ///
    /// One match per setting rather than a map, so a setting added to the
    /// table without being read here does not compile.
    #[must_use]
    pub fn value_of(&self, key: &str) -> Option<Value> {
        match key {
            "theme" => Some(Value::Choice(self.theme.clone())),
            "icons" => Some(Value::Switch(self.icons)),
            "blame_margin" => Some(Value::Switch(self.blame_margin)),
            "wrap" => Some(Value::Switch(self.wrap)),
            "tab_width" => Some(Value::Count(self.tab_width)),
            "hover_delay" => Some(Value::Count(self.hover_delay)),
            "format_on_save" => Some(Value::Switch(self.format_on_save)),
            "code_actions_on_save" => Some(Value::Switch(self.code_actions_on_save)),
            "inlay_hints" => Some(Value::Switch(self.inlay_hints)),
            "diagnostics" => Some(Value::Switch(self.diagnostics)),
            "ignored_files" => Some(Value::Switch(self.ignored_files)),
            "agent" => Some(Value::Choice(self.agent.clone().unwrap_or_default())),
            _ => None,
        }
    }

    /// Sets one setting, ignoring a value of the wrong shape.
    pub fn set(&mut self, key: &str, value: &Value) {
        match (key, value) {
            ("theme", Value::Choice(word)) => self.theme = word.clone(),
            ("icons", Value::Switch(on)) => self.icons = *on,
            ("blame_margin", Value::Switch(on)) => self.blame_margin = *on,
            ("wrap", Value::Switch(on)) => self.wrap = *on,
            // Clamped where it is read rather than refused here: a file
            // somebody typed `0` into should not make every tab nothing.
            ("tab_width", Value::Count(width)) => self.tab_width = *width,
            ("hover_delay", Value::Count(delay)) => self.hover_delay = *delay,
            ("format_on_save", Value::Switch(on)) => self.format_on_save = *on,
            ("code_actions_on_save", Value::Switch(on)) => self.code_actions_on_save = *on,
            ("inlay_hints", Value::Switch(on)) => self.inlay_hints = *on,
            ("diagnostics", Value::Switch(on)) => self.diagnostics = *on,
            ("ignored_files", Value::Switch(on)) => self.ignored_files = *on,
            // An empty word is nobody, which is how a reader stops talking
            // to an agent without a second setting meaning "off".
            ("agent", Value::Choice(word)) => {
                self.agent = (!word.is_empty()).then(|| word.clone());
            }
            _ => tracing::debug!(key, ?value, "a setting that does not take this"),
        }
    }

    /// What this agent is to start one of its settings on, if the reader
    /// has said.
    #[must_use]
    pub fn agent_default(&self, agent: &str, setting: &str) -> Option<&str> {
        Some(self.agents.get(agent)?.get(setting)?.as_str())
    }

    /// Everything they have said about one agent.
    ///
    /// Empty where they have said nothing, which is the ordinary case and
    /// not a state worth a second answer: there is no difference between
    /// an agent nobody has set anything on and one whose settings were all
    /// unset again.
    #[must_use]
    pub fn agent_defaults(&self, agent: &str) -> &std::collections::BTreeMap<String, String> {
        static NOTHING: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
            std::sync::LazyLock::new(std::collections::BTreeMap::new);
        self.agents.get(agent).unwrap_or(&NOTHING)
    }

    /// Says what one of them is to start on.
    pub fn set_agent_default(&mut self, agent: &str, setting: &str, value: &str) {
        self.agents
            .entry(agent.to_string())
            .or_default()
            .insert(setting.to_string(), value.to_string());
    }

    /// Stops saying, which puts the setting back in the agent's hands.
    ///
    /// The agent's own table goes when the last of its settings does: a
    /// table with nothing in it says obelus was thinking about that agent,
    /// which after this it is not.
    pub fn unset_agent_default(&mut self, agent: &str, setting: &str) {
        let Some(chosen) = self.agents.get_mut(agent) else {
            return;
        };
        chosen.remove(setting);
        if chosen.is_empty() {
            self.agents.remove(agent);
        }
    }
}

/// Where the file lives, or `None` on a system with nowhere to put it.
///
/// `dirs` rather than `$HOME/.config` spelled out here: the answer differs
/// per platform, and being wrong about it means writing a file the reader
/// will never find.
#[must_use]
pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("obelus").join("config.toml"))
}

/// Where a project keeps settings of its own, if it keeps any.
///
/// `.obelus/config.toml`, and only that. A project keeps more than settings for
/// obelus -- a theme of its own, whatever comes after it -- and one
/// directory holding all of it is one thing to find, to copy between
/// machines and to name in a `.gitignore`, where a dotfile per kind of thing
/// is a row of them at the top of every listing of the project.
///
/// The working directory itself, without walking up: obelus has one answer
/// to which project it is on -- the file list walks it, the counts count it,
/// git is read from it -- and settings found by walking somewhere else would
/// be a second answer to that question.
#[must_use]
pub fn project_path(root: &Path) -> Option<PathBuf> {
    let inside = root.join(".obelus").join("config.toml");
    inside.is_file().then_some(inside)
}

/// The table a file holds, for a caller that means to lay it over something.
///
/// Three answers, like [`read_from`]'s four: there is none, here it is, or
/// it will not read. A project's file that will not read is *not* a reason to
/// stop -- obelus goes on with the reader's own settings and says so in the
/// log -- which is why this hands back the reason rather than a config with
/// the defaults in it.
pub fn read_table(path: &Path) -> Result<Option<toml::Table>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    text.parse::<toml::Table>()
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Reads the file, or the defaults for every way it can decline.
#[must_use]
pub fn load() -> Reading {
    let Some(path) = path() else {
        return Reading::Nowhere;
    };
    read_from(&path)
}

/// The same, from a path the caller names.
#[must_use]
pub fn read_from(path: &Path) -> Reading {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // Not there yet is the ordinary case, not an error: obelus writes
        // the file the first time something is changed.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Reading::Nothing;
        }
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    match text.parse::<toml::Table>() {
        Ok(table) => {
            let (config, named) = from_table(&table);
            Reading::Settings(config, named)
        }
        Err(error) => Reading::Unreadable(error.to_string()),
    }
}

/// What reading the settings file found.
///
/// "There is no file" and "there is a file obelus cannot read" are different
/// answers and were the same one: both became the defaults, and a session
/// that started on the defaults writes the defaults back the first time
/// anything is changed. A file being written by another obelus at that
/// moment, or edited by hand into something that will not parse, is then a
/// file whose contents obelus has thrown away.
#[derive(Clone, Debug)]
pub enum Reading {
    /// This system has nowhere to keep one.
    Nowhere,
    /// There is none yet, which is where everybody starts.
    Nothing,
    /// There is one; this is what it says, and these are the settings it
    /// named. A setting it named is the reader's whether or not what they
    /// wrote differs from what obelus would have done.
    Settings(Config, Vec<&'static str>),
    /// There is one and it could not be read, with what went wrong.
    Unreadable(String),
}

/// The config a file's contents describe, taking the default for anything
/// missing or of the wrong type.
#[must_use]
pub fn from_toml(text: &str) -> Config {
    let Ok(table) = text.parse::<toml::Table>() else {
        tracing::warn!("the config file is not toml, so the defaults it is");
        return Config::default();
    };
    from_table(&table).0
}

/// The same, from a table already parsed, with the keys it named.
///
/// Which keys, not only what they came to: a reader who writes a setting
/// down has said something about it even where what they said is what
/// obelus would have done anyway, and a page that worked that out by
/// comparing with the default could not tell them from a reader who said
/// nothing at all.
fn from_table(table: &toml::Table) -> (Config, Vec<&'static str>) {
    let mut config = Config::default();
    let named = apply(&mut config, table, Whose::Reader);
    (config, named)
}

/// Which files may set the key `key`.
///
/// Asked of a *key* rather than of a [`Setting`], because three of the
/// things in the file are not rows on the settings page: the agent is
/// chosen on a page of its own, the keys are a table, and what an agent
/// starts on is a table of the agent's own words. All three are the
/// reader's alone.
///
/// A key obelus has never heard of reaches nowhere, which costs nothing --
/// nothing reads it either way -- and means a key added to the file before
/// it is added here cannot arrive from a project.
#[must_use]
pub fn reach_of(key: &str) -> Reach {
    match key {
        "agent" | "keys" | "agents" => Reach::ReaderOnly,
        _ => Setting::named(key).map_or(Reach::ReaderOnly, |setting| setting.reach),
    }
}

/// Lays a table of settings over a config, and says which keys it set.
///
/// Over, rather than into a fresh one: a project's file names the few settings
/// that project cares about, and everything it does not name is the reader's
/// and stays theirs. Reading it into a default config and taking that would
/// be a project with one line in it turning off a reader's wrapped lines.
///
/// What the project may not set is left alone, with a word in the log for
/// whoever wrote that file: from the outside it is a line that did nothing,
/// which is worth being able to find out about.
pub fn apply(config: &mut Config, table: &toml::Table, whose: Whose) -> Vec<&'static str> {
    let mut set = Vec::new();
    let mut allowed = |key: &'static str| {
        if reach_of(key) == Reach::Anywhere || whose == Whose::Reader {
            set.push(key);
            return true;
        }
        if table.contains_key(key) {
            tracing::warn!(key, "a project may not set this, so it is left alone");
        }
        false
    };

    if let Some(word) = table.get("theme").and_then(toml::Value::as_str)
        && allowed("theme")
    {
        config.theme = word.to_string();
    }
    if let Some(on) = table.get("icons").and_then(toml::Value::as_bool)
        && allowed("icons")
    {
        config.icons = on;
    }
    if let Some(on) = table.get("blame_margin").and_then(toml::Value::as_bool)
        && allowed("blame_margin")
    {
        config.blame_margin = on;
    }
    if let Some(on) = table.get("wrap").and_then(toml::Value::as_bool)
        && allowed("wrap")
    {
        config.wrap = on;
    }
    if let Some(width) = table.get("tab_width").and_then(toml::Value::as_integer)
        && allowed("tab_width")
    {
        config.tab_width = usize::try_from(width).unwrap_or(obelus_text::TAB_WIDTH);
    }
    if let Some(delay) = table.get("hover_delay").and_then(toml::Value::as_integer)
        && allowed("hover_delay")
    {
        config.hover_delay = usize::try_from(delay).unwrap_or(0);
    }
    if let Some(on) = table.get("format_on_save").and_then(toml::Value::as_bool)
        && allowed("format_on_save")
    {
        config.format_on_save = on;
    }
    if let Some(on) = table
        .get("code_actions_on_save")
        .and_then(toml::Value::as_bool)
        && allowed("code_actions_on_save")
    {
        config.code_actions_on_save = on;
    }
    if let Some(on) = table.get("inlay_hints").and_then(toml::Value::as_bool)
        && allowed("inlay_hints")
    {
        config.inlay_hints = on;
    }
    if let Some(on) = table.get("diagnostics").and_then(toml::Value::as_bool)
        && allowed("diagnostics")
    {
        config.diagnostics = on;
    }
    if let Some(on) = table.get("ignored_files").and_then(toml::Value::as_bool)
        && allowed("ignored_files")
    {
        config.ignored_files = on;
    }
    if let Some(word) = table.get("agent").and_then(toml::Value::as_str)
        && allowed("agent")
    {
        config.agent = (!word.is_empty()).then(|| word.to_string());
    }
    if let Some(keys) = table.get("keys").and_then(toml::Value::as_table)
        && allowed("keys")
    {
        // Whatever is a string. A command obelus has never heard of and a
        // chord it cannot read are dealt with where the table is built,
        // which is the one place that knows what either of those is.
        for (name, chord) in keys {
            if let Some(chord) = chord.as_str() {
                config.keys.insert(name.clone(), chord.to_string());
            }
        }
    }
    if let Some(agents) = table.get("agents").and_then(toml::Value::as_table)
        && allowed("agents")
    {
        // A table per agent, of strings. Whether an agent by that name
        // exists, whether it still has a setting by that name, and whether
        // that setting still takes that value are three questions asked
        // where the agent is -- here there is nobody to ask, and a line
        // about an agent that is not installed is not a line with anything
        // wrong with it.
        for (agent, chosen) in agents {
            let Some(chosen) = chosen.as_table() else {
                tracing::warn!(agent, "what this agent is set to is not a table");
                continue;
            };
            for (setting, value) in chosen {
                if let Some(value) = value.as_str() {
                    config.set_agent_default(agent, setting, value);
                }
            }
        }
    }

    // And a word for the lines obelus walked past. A key it has never heard
    // of -- a setting that has gone, a name that has changed, a word spelled
    // wrong -- is read, ignored, and from the outside looks exactly like one
    // that was obeyed. The line above says as much when a project oversteps,
    // for the same reason: from the outside it is a line that did nothing,
    // and that is worth being able to find out about.
    for key in table.keys() {
        if !known(key) {
            tracing::warn!(key, "no setting by this name, so the line does nothing");
        }
    }
    set
}

/// Whether a key in a settings file names a setting obelus has.
///
/// The three that are not rows in [`ALL`] count: they are settings a reader
/// writes by hand or sets somewhere else on the page, not settings obelus
/// has stopped having.
fn known(key: &str) -> bool {
    matches!(key, "agent" | "keys" | "agents") || Setting::named(key).is_some()
}

/// The file's contents for a config, with nothing else in it.
#[must_use]
pub fn to_toml(config: &Config) -> String {
    over("", config)
}

/// The file's contents for a config, laid over a file that already exists.
///
/// Edited rather than rewritten, the way a project's file is. Obelus used to
/// write its own file whole on the grounds that obelus wrote all of it,
/// which is not true: readers open it and put lines in by hand. Writing it
/// whole took out everything obelus did not recognise -- a setting from a
/// newer version, a key that has been renamed since, a line with a typo in
/// it -- silently, on the next switch they flipped. A program that will not
/// edit a file it is reading should not quietly delete from one it owns.
///
/// A file that is not toml at all is started again from nothing: there is
/// no document to lay anything over, and obelus has already said so
/// elsewhere.
#[must_use]
pub fn over(existing: &str, config: &Config) -> String {
    let mut document = existing
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_default();
    document["theme"] = toml_edit::value(config.theme.clone());
    document["icons"] = toml_edit::value(config.icons);
    document["blame_margin"] = toml_edit::value(config.blame_margin);
    document["wrap"] = toml_edit::value(config.wrap);
    document["tab_width"] = toml_edit::value(i64::try_from(config.tab_width).unwrap_or(4));
    document["hover_delay"] = toml_edit::value(i64::try_from(config.hover_delay).unwrap_or(400));
    document["format_on_save"] = toml_edit::value(config.format_on_save);
    document["code_actions_on_save"] = toml_edit::value(config.code_actions_on_save);
    document["inlay_hints"] = toml_edit::value(config.inlay_hints);
    document["diagnostics"] = toml_edit::value(config.diagnostics);
    document["ignored_files"] = toml_edit::value(config.ignored_files);
    // Written even when there is nobody, so the file says what obelus read
    // rather than leaving the reader to wonder whether it noticed.
    document["agent"] = toml_edit::value(config.agent.clone().unwrap_or_default());
    // Only while the reader has moved something: an empty table in the file
    // says obelus was thinking about keys, which it was not.
    if config.keys.is_empty() {
        document.remove("keys");
    } else {
        let mut keys = toml_edit::Table::new();
        for (name, chord) in &config.keys {
            keys[name] = toml_edit::value(chord.clone());
        }
        document["keys"] = toml_edit::Item::Table(keys);
    }
    // The same rule, an agent at a time: a table for an agent the reader
    // has said nothing about is obelus writing down that it thought about
    // it. `unset_agent_default` already drops one that empties; this is
    // the same answer for a config that arrived from anywhere else.
    let agents: Vec<(&String, &std::collections::BTreeMap<String, String>)> = config
        .agents
        .iter()
        .filter(|(_, chosen)| !chosen.is_empty())
        .collect();
    if agents.is_empty() {
        document.remove("agents");
    } else {
        let mut table = toml_edit::Table::new();
        // Implicit, so the file says `[agents.claude-code]` rather than an
        // empty `[agents]` and a table under it: the reader opening this
        // file is looking for the agent's name.
        table.set_implicit(true);
        for (agent, chosen) in agents {
            let mut settings = toml_edit::Table::new();
            for (setting, value) in chosen {
                settings[setting] = toml_edit::value(value.clone());
            }
            table[agent] = toml_edit::Item::Table(settings);
        }
        document["agents"] = toml_edit::Item::Table(table);
    }
    document.to_string()
}

/// What a path really names, following any links.
///
/// A reader who keeps their settings in git links the place obelus looks at
/// the file in their repository, which makes the difference between the two
/// paths matter twice. Writing has to go *through* the link, because a
/// rename replaces what the name refers to -- the link would become an
/// ordinary file on the first setting they changed, and every change after
/// that would go somewhere the repository never sees, silently. And
/// watching has to follow it, because what a `git pull` rewrites is the
/// file at the far end: a watch on the link's own directory hears nothing,
/// so settings arriving from another machine would sit on disk until obelus
/// was next started.
///
/// A path that is not there yet cannot be resolved, and is its own answer:
/// there is no link to follow.
#[must_use]
pub fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Sets or removes one key in a project's own settings file.
///
/// Edited rather than rewritten. Obelus's own file it writes whole, because
/// obelus wrote all of it; a project's is written by hand and committed, so it
/// has comments in it, an order somebody chose, and possibly keys this
/// version has never heard of. A round trip through a `toml::Table` would
/// throw all three away on the first switch a reader flipped.
///
/// `None` takes the key out, which is how a setting stops being the project's
/// and goes back to being the reader's.
///
/// The file need not exist: setting the first key makes it, which is the
/// ordinary way a project acquires one.
pub fn write_project(path: &Path, key: &str, value: Option<&Value>) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    match value {
        Some(Value::Switch(on)) => document[key] = toml_edit::value(*on),
        Some(Value::Choice(word)) => document[key] = toml_edit::value(word.clone()),
        Some(Value::Count(count)) => {
            document[key] = toml_edit::value(i64::try_from(*count).unwrap_or(0));
        }
        None => {
            // What was written above the key goes with it, except for
            // whatever is above the last blank line: a comment touching a
            // key is about that key, and anything an empty line away from
            // it is the file's own -- a heading, or a note about the lot.
            // Removing the first key of a file took its heading with it.
            let above = document
                .as_table()
                .get_key_value(key)
                .and_then(|(key, _)| key.leaf_decor().prefix().cloned())
                .and_then(|prefix| prefix.as_str().map(str::to_string));
            document.remove(key);
            let next = document
                .as_table()
                .iter()
                .next()
                .map(|(key, _)| key.to_string());
            if let Some(above) = above
                && let Some(end) = above.rfind("\n\n")
                && let Some(next) = next
                && let Some(mut next) = document.as_table_mut().key_mut(&next)
            {
                let kept = &above[..end + 2];
                let already = next
                    .leaf_decor()
                    .prefix()
                    .and_then(|prefix| prefix.as_str().map(str::to_string))
                    .unwrap_or_default();
                next.leaf_decor_mut().set_prefix(format!("{kept}{already}"));
            }
        }
    }

    let Some(directory) = path.parent() else {
        return std::fs::write(path, document.to_string());
    };
    std::fs::create_dir_all(directory)?;
    // Beside it and renamed over it, for the reason the reader's own file is
    // written that way: another obelus on this project may be reading it at
    // this moment, and a plain write truncates first.
    let beside = path.with_extension("toml.writing");
    std::fs::write(&beside, document.to_string())?;
    std::fs::rename(&beside, path)
}

/// Where a project's settings *would* go, for a project that has none yet.
///
/// The directory, always. A project keeps more than settings for obelus -- a
/// theme of its own, whatever comes after it -- and one directory holding
/// all of it is one thing to find, to copy between machines and to put in a
/// `.gitignore`, where a dotfile per kind of thing is a row of them at the
/// top of every listing of the project.
#[must_use]
pub fn project_path_for(root: &Path) -> PathBuf {
    root.join(".obelus").join("config.toml")
}

/// Writes a config to a path, making its directory if it is not there.
///
/// The path is passed in rather than looked up here, so that nothing can
/// write to the reader's real file by accident: a test, or a probe run while
/// working on obelus, has to say where it is writing.
pub fn save_to(path: &Path, config: &Config) -> std::io::Result<()> {
    // Through the link rather than over it: a rename replaces what the name
    // refers to, and where the settings are kept in a dotfiles repository
    // the name refers to a link.
    let resolved = resolved(path);
    let path = resolved.as_path();
    // What is in it already, so that whatever obelus does not recognise
    // stays there. Nothing, where there is nothing: the first write makes
    // the file.
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let Some(directory) = path.parent() else {
        return std::fs::write(path, over(&existing, config));
    };
    std::fs::create_dir_all(directory)?;
    // Written beside it and renamed over it, because another obelus may be
    // reading this file at this moment: a plain write truncates first, and
    // a reader landing in that gap sees an empty file, takes it for "no
    // settings", and writes its defaults over everything the reader has.
    // A rename within one directory is the one filesystem operation that
    // has no such gap.
    //
    // Beside it rather than in a temporary directory: rename is only atomic
    // within a filesystem, and the only directory known to be on the same
    // one is this one.
    let beside = path.with_extension("toml.writing");
    std::fs::write(&beside, over(&existing, config))?;
    std::fs::rename(&beside, path)
}

#[cfg(test)]
mod tests {
    // Both of the tests that save a file are unix's: one makes a symbolic
    // link and the other reads an inode. Imported beside them so a Windows
    // build is not warned about a name nothing there uses.
    #[cfg(unix)]
    use super::save_to;
    use super::{Config, Value, Whose, apply, from_toml, known, over, to_toml};

    /// A path that is a link is written *through*, not over.
    ///
    /// Which is how anybody keeps their settings in git: the file lives in a
    /// dotfiles repository and the place obelus looks is a link to it. The
    /// atomic rename replaces what the name refers to, and the name refers
    /// to the link -- so saving turned the link into an ordinary file and
    /// the repository stopped hearing about changes, with nothing on screen
    /// saying so.
    ///
    /// Broken deliberately by taking the `canonicalize` out of `save_to`:
    /// the link came back an ordinary file and the file in the repository
    /// still held the old theme.
    #[cfg(unix)]
    #[test]
    fn saving_through_a_link_keeps_the_link_and_writes_what_it_points_at() {
        let directory = std::env::temp_dir().join(format!("obelus-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let repository = directory.join("dotfiles");
        let config_home = directory.join("config");
        std::fs::create_dir_all(&repository).expect("a directory");
        std::fs::create_dir_all(&config_home).expect("a directory");

        let real = repository.join("config.toml");
        std::fs::write(&real, "theme = \"dark\"\n").expect("the file");
        let linked = config_home.join("config.toml");
        std::os::unix::fs::symlink(&real, &linked).expect("a link");

        let config = Config {
            theme: "light".to_string(),
            ..Config::default()
        };
        save_to(&linked, &config).expect("saving");

        assert!(
            std::fs::symlink_metadata(&linked)
                .expect("the link")
                .file_type()
                .is_symlink(),
            "saving replaced the link with a file of its own"
        );
        let written = std::fs::read_to_string(&real).expect("the file it points at");
        assert!(
            written.contains("light"),
            "the file in the repository did not get the change: {written:?}"
        );
        // And nothing left beside either of them.
        assert!(
            !config_home.join("config.toml.writing").exists()
                && !repository.join("config.toml.writing").exists(),
            "a half-written file was left behind"
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Saving replaces the file rather than rewriting it where it lies.
    ///
    /// Which is what makes it safe for another obelus to be reading it at
    /// that moment: a write in place truncates first, and a reader landing
    /// in that gap sees an empty file, takes it for "no settings", and
    /// writes its own defaults over everything the reader had. A rename
    /// within one directory has no such gap -- and the file being a new
    /// one afterwards is how that shows from the outside.
    #[cfg(unix)]
    #[test]
    fn saving_replaces_the_file_rather_than_emptying_it_first() {
        use std::os::unix::fs::MetadataExt as _;

        let directory = std::env::temp_dir().join(format!("obelus-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");
        let path = directory.join("config.toml");

        let mut config = Config::default();
        config.set("theme", &Value::Choice("light".to_string()));
        save_to(&path, &config).expect("saving");
        let first = std::fs::metadata(&path).expect("the file").ino();

        config.set("theme", &Value::Choice("dark".to_string()));
        save_to(&path, &config).expect("saving again");
        let second = std::fs::metadata(&path).expect("the file").ino();

        assert_ne!(
            first, second,
            "the settings were rewritten where they lay, which another obelus can read half of"
        );
        assert_eq!(
            from_toml(&std::fs::read_to_string(&path).expect("the file")).theme,
            "dark",
            "the new settings are not what is in the file"
        );
        // And nothing left beside it: a file called `config.toml.writing`
        // in a reader's config directory is obelus's mess, not theirs.
        let beside: Vec<_> = std::fs::read_dir(&directory)
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "config.toml")
            .collect();
        assert!(beside.is_empty(), "it left {beside:?} behind");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A key obelus has never heard of is a line that did nothing, and the
    /// only way to find out from the outside is to be told.
    #[test]
    fn a_key_obelus_does_not_know_is_reported() {
        assert!(known("theme"), "a setting on the page is not known");
        assert!(known("agent"), "a setting written by hand is not known");
        assert!(known("keys"), "the key table is not known");
        assert!(
            !known("blame"),
            "a name obelus has stopped using is still known"
        );
        assert!(!known("prevlew"), "a name spelled wrong is known");
    }

    /// Written and read back is the same config: the file is the only place
    /// a setting survives, so anything that does not survive the round trip
    /// is a setting the reader has to set twice.
    #[test]
    fn a_config_survives_the_file() {
        let config = Config {
            theme: "light".to_string(),
            icons: false,
            blame_margin: false,
            wrap: true,
            tab_width: 8,
            hover_delay: 800,
            format_on_save: true,
            code_actions_on_save: true,
            inlay_hints: true,
            diagnostics: true,
            ignored_files: true,
            agent: Some("claude-acp".to_string()),
            // A key moved and a key taken away: both are decisions, and
            // both have to survive the file or the reader makes them again
            // every time obelus starts.
            keys: [
                ("open-file".to_string(), "alt+o".to_string()),
                ("close-file".to_string(), String::new()),
            ]
            .into_iter()
            .collect(),
            // Two agents, because the file keeps a table each and one of
            // them would not say whether the name on it is the agent's.
            agents: [
                (
                    "claude-acp".to_string(),
                    [("mode".to_string(), "accept-edits".to_string())]
                        .into_iter()
                        .collect(),
                ),
                (
                    "gemini-cli".to_string(),
                    [("model".to_string(), "flash".to_string())]
                        .into_iter()
                        .collect(),
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(from_toml(&to_toml(&config)), config);
        assert_eq!(
            from_toml(&to_toml(&Config::default())),
            Config::default(),
            "the defaults do not survive their own file"
        );
    }

    /// A file with a typo in it gives obelus, not an error message where the
    /// editor was: every field falls back on its own.
    #[test]
    fn nonsense_in_the_file_is_the_default() {
        assert_eq!(from_toml("this is not toml at all ["), Config::default());
        assert_eq!(from_toml(""), Config::default());
        // The wrong type for a field leaves that field alone and keeps the
        // rest of the file.
        let mixed = from_toml("theme = 7\nicons = false\n");
        assert_eq!(mixed.theme, Config::default().theme);
        assert!(!mixed.icons);
        // And a key obelus does not know is not an error either: an older
        // obelus reading a newer file should still start.
        assert_eq!(from_toml("nonsense = true"), Config::default());
    }

    /// A setting takes only the shape of value its control produces.
    #[test]
    fn a_setting_ignores_the_wrong_shape_of_value() {
        let mut config = Config::default();
        config.set("icons", &Value::Choice("dark".to_string()));
        assert_eq!(config, Config::default(), "a word set a switch");
        config.set("theme", &Value::Switch(false));
        assert_eq!(config, Config::default(), "a switch set a word");

        config.set("icons", &Value::Switch(false));
        assert_eq!(config.value_of("icons"), Some(Value::Switch(false)));
    }

    /// Every setting in the table can be read and written, which is what the
    /// view assumes: a row it cannot read has nothing to draw.
    #[test]
    fn every_setting_in_the_table_is_a_field() {
        let config = Config::default();
        for setting in super::ALL {
            let value = config
                .value_of(setting.key)
                .unwrap_or_else(|| panic!("{} cannot be read", setting.key));
            match (setting.kind, &value) {
                (super::Kind::Switch, Value::Switch(_)) => {}
                (super::Kind::Choice(choices), Value::Choice(word)) => assert!(
                    choices.contains(&word.as_str()),
                    "{} defaults to {word:?}, which is not one of its choices",
                    setting.key
                ),
                (super::Kind::Count(counts), Value::Count(count)) => {
                    assert!(
                        counts.contains(&count.to_string().as_str()),
                        "{} defaults to {count}, which is not one of its choices",
                        setting.key
                    );
                    // The list is spelled, because the control that offers
                    // it offers words. Every one of them has to be a number
                    // or the reader picks something that parses to nothing.
                    for offered in counts {
                        assert!(
                            offered.parse::<usize>().is_ok(),
                            "{} offers {offered:?}, which is not a number",
                            setting.key
                        );
                    }
                }
                (kind, value) => {
                    panic!("{} is a {kind:?} holding a {value:?}", setting.key)
                }
            }
        }
    }

    /// What an agent is to start on survives the file, agent by agent.
    ///
    /// The file is where this lives between sittings, and it is a table of
    /// tables -- neither of which the flat settings above it exercise. A
    /// round trip is the whole of the claim: what was said is what comes
    /// back, under the agent it was said about.
    ///
    /// Broken deliberately by writing the inner tables under one shared
    /// name rather than the agent's: both agents came back with the
    /// second one's settings.
    #[test]
    fn what_an_agent_starts_on_survives_the_file() {
        let mut config = Config::default();
        config.set_agent_default("claude-code", "mode", "accept-edits");
        config.set_agent_default("claude-code", "thinking", "high");
        config.set_agent_default("gemini-cli", "mode", "plan");

        let written = to_toml(&config);
        assert!(
            written.contains("[agents.claude-code]"),
            "the agent is not its own table: {written}"
        );
        let read = from_toml(&written);
        assert_eq!(
            read.agent_default("claude-code", "mode"),
            Some("accept-edits")
        );
        assert_eq!(read.agent_default("claude-code", "thinking"), Some("high"));
        assert_eq!(read.agent_default("gemini-cli", "mode"), Some("plan"));
        assert_eq!(
            read.agent_default("gemini-cli", "thinking"),
            None,
            "an agent was given what another agent was set to"
        );
    }

    /// A project may not say what an agent starts on.
    ///
    /// The reason `agent` is the reader's alone, twice over: a downloaded
    /// project that could write this one could say that the agent it starts
    /// may edit files without asking.
    ///
    /// Broken deliberately by taking "agents" out of `reach_of` *and*
    /// making the fallback for an unknown key `Anywhere`: the project's line
    /// was obeyed and the mode came back as the project's. Both, because
    /// taking out the named arm alone changes nothing -- the fallback
    /// already refuses a key with no row on the page. The arm stays all
    /// the same: the rule is that a project may not set this, and a rule that
    /// holds only because obelus happens to have no setting by that name
    /// is a rule nobody has written down.
    #[test]
    fn a_tree_may_not_say_what_an_agent_starts_on() {
        let table = r#"
            [agents.claude-code]
            mode = "yolo"
        "#
        .parse::<toml::Table>()
        .expect("a table");

        let mut readers = Config::default();
        readers.set_agent_default("claude-code", "mode", "plan");
        let mut config = readers.clone();
        apply(&mut config, &table, Whose::Project);
        assert_eq!(
            config.agent_default("claude-code", "mode"),
            Some("plan"),
            "a project set what the agent starts on"
        );

        // And the reader's own file is obeyed, so the test is about who
        // wrote it rather than about the line being unreadable.
        let mut config = readers;
        apply(&mut config, &table, Whose::Reader);
        assert_eq!(config.agent_default("claude-code", "mode"), Some("yolo"));
    }

    /// Unsetting the last of an agent's settings takes its table out.
    ///
    /// The same rule the keys follow: a table with nothing in it says
    /// obelus was thinking about that agent, which after this it is not --
    /// and a reader who opens the file looking for what they undid would
    /// find the heading still there.
    ///
    /// Broken deliberately by leaving the empty map in `agents`: the file
    /// kept `[agents.claude-code]` with nothing under it.
    #[test]
    fn unsetting_the_last_of_an_agents_settings_takes_its_table_out() {
        let mut config = Config::default();
        config.set_agent_default("claude-code", "mode", "plan");
        config.unset_agent_default("claude-code", "mode");
        assert!(config.agents.is_empty(), "the agent kept an empty table");

        let written = over("[agents.claude-code]\nmode = \"plan\"\n", &config);
        assert!(
            !written.contains("agents"),
            "the file kept a table with nothing in it: {written}"
        );
    }

    /// An agent obelus has never installed keeps what the reader wrote.
    ///
    /// The table is obelus's to write whole, the way the keys are -- which
    /// is only lossless because reading takes *every* agent out of the
    /// file, including ones this machine has never installed and ones a
    /// newer obelus knows about. A read that kept only the installed ones
    /// would quietly empty the file on the next switch anybody flipped.
    ///
    /// Broken deliberately by having the writer keep only the agents it
    /// had read this session: the hand-written one went on the first save.
    #[test]
    fn an_agent_obelus_has_never_installed_keeps_what_the_reader_wrote() {
        let read = from_toml("[agents.some-other-agent]\nmode = \"ask\"\n");
        assert_eq!(read.agent_default("some-other-agent", "mode"), Some("ask"));

        let mut config = read;
        config.set_agent_default("claude-code", "mode", "plan");
        let read = from_toml(&to_toml(&config));
        assert_eq!(
            read.agent_default("some-other-agent", "mode"),
            Some("ask"),
            "writing one agent down lost another"
        );
        assert_eq!(read.agent_default("claude-code", "mode"), Some("plan"));
    }
}
