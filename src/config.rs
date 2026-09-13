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

use std::path::{Path, PathBuf};

/// Everything the reader can decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Which built-in theme to use, by name.
    pub theme: String,
    /// Whether to draw Nerd Font glyphs.
    pub icons: bool,
    /// Whether to say who last changed the line the cursor is on.
    pub blame: bool,
    /// Whether a line too long for the screen continues on the next row.
    pub wrap: bool,
    /// Whether a file that has a reading opens in it.
    ///
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: crate::theme::builtin::DARK.name.to_string(),
            icons: true,
            blame: true,
            // Off, so a line is a line: a reader counting rows, comparing
            // two files side by side, or looking at a table in a comment is
            // reading something the screen has not rearranged. The reader
            // who wants it can say so, and then it is a line's own choice
            // no longer.
            wrap: false,
            // On: a file with a reading has one because reading it that way
            // is better, and `f10` is how to see the bytes instead.
            // None until the reader installs one: obelus does not choose an
            // agent for anybody.
            agent: None,
            // Nothing moved: the table obelus ships with.
            keys: std::collections::BTreeMap::new(),
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
}

/// What sort of control a setting gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Switch,
    /// One of a fixed list of words.
    Choice(&'static [&'static str]),
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
}

impl Group {
    /// Every group, in the order their tabs sit in.
    pub const ALL: [Self; 2] = [Self::Appearance, Self::Reading];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Reading => "reading",
        }
    }
}

/// Who may set a setting.
///
/// A tree's own settings are written by whoever wrote the tree, and a reader
/// who opens somebody's repository has not agreed to everything in it. Most
/// of these are harmless to hand over -- a theme, a wrapped line, a name in
/// the margin -- and some are not: `agent` says which agent obelus starts,
/// and a program starting because a file in a downloaded tree said so is a
/// decision that belongs to the person at the keyboard. The keys are the
/// same: a tree that could rebind them could put a reader's `quit` somewhere
/// they would find by accident.
///
/// A kind rather than a list of exceptions, because the next one of these
/// will be found the way this one was -- by asking, of a new setting,
/// whether a stranger may set it -- and the asking should be part of writing
/// the setting down rather than something to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Either file: the reader's own, or the tree's.
    Anywhere,
    /// The reader's own file alone. A tree naming it is ignored, with a word
    /// in the log for whoever wrote that file.
    ReaderOnly,
}

/// Which file a table of settings came out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The reader's, wherever this system keeps such things.
    Reader,
    /// The tree obelus was opened on.
    Tree,
}

/// One setting, as data.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    /// What it is called in the file.
    pub key: &'static str,
    /// What the view calls it: one line, which is both its name and its
    /// description.
    ///
    /// One rather than two, because two of them side by side on a row read
    /// as a heading and a footnote -- and the footnote said what the
    /// heading already had.
    pub label: &'static str,
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

/// Every setting obelus has.
pub const ALL: &[Setting] = &[
    Setting {
        key: "theme",
        label: "Colour theme",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Choice(THEMES),
    },
    Setting {
        key: "icons",
        label: "Nerd Font glyphs in lists and on the status bar",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "wrap",
        label: "Wrap a line too long for the screen onto the next row",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
    },
    Setting {
        key: "blame",
        label: "Who last changed the line the cursor is on",
        group: Group::Reading,
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
            "blame" => Some(Value::Switch(self.blame)),
            "wrap" => Some(Value::Switch(self.wrap)),
            "agent" => Some(Value::Choice(self.agent.clone().unwrap_or_default())),
            _ => None,
        }
    }

    /// Sets one setting, ignoring a value of the wrong shape.
    pub fn set(&mut self, key: &str, value: &Value) {
        match (key, value) {
            ("theme", Value::Choice(word)) => self.theme = word.clone(),
            ("icons", Value::Switch(on)) => self.icons = *on,
            ("blame", Value::Switch(on)) => self.blame = *on,
            ("wrap", Value::Switch(on)) => self.wrap = *on,
            // An empty word is nobody, which is how a reader stops talking
            // to an agent without a second setting meaning "off".
            ("agent", Value::Choice(word)) => {
                self.agent = (!word.is_empty()).then(|| word.clone());
            }
            _ => tracing::debug!(key, ?value, "a setting that does not take this"),
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

/// Where a tree keeps settings of its own, if it keeps any.
///
/// `.obelus/config.toml` first and `.obelus.toml` after it: the directory is
/// the form with room in it -- a theme belonging to the tree will go beside
/// the config in there -- and the single file is for a tree that only ever
/// wants the one line. Both, because making a directory to set one line is
/// asking too much, and a tree that has grown past one file should not have
/// to keep a stray dotfile beside the directory holding the rest.
///
/// The working directory itself, without walking up: obelus has one answer
/// to which tree it is on -- the file list walks it, the counts count it,
/// git is read from it -- and settings found by walking somewhere else would
/// be a second answer to that question.
#[must_use]
pub fn tree_path(root: &Path) -> Option<PathBuf> {
    let inside = root.join(".obelus").join("config.toml");
    if inside.is_file() {
        return Some(inside);
    }
    let beside = root.join(".obelus.toml");
    beside.is_file().then_some(beside)
}

/// The table a file holds, for a caller that means to lay it over something.
///
/// Three answers, like [`read_from`]'s four: there is none, here it is, or
/// it will not read. A tree's file that will not read is *not* a reason to
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
        Ok(table) => Reading::Settings(from_table(&table)),
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
    /// There is one, and this is what it says.
    Settings(Config),
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
    from_table(&table)
}

/// The same, from a table already parsed.
fn from_table(table: &toml::Table) -> Config {
    let mut config = Config::default();
    apply(&mut config, table, Whose::Reader);
    config
}

/// Which files may set the key `key`.
///
/// Asked of a *key* rather than of a [`Setting`], because two of the things
/// in the file are not rows on the settings page: the agent is chosen on a
/// page of its own, and the keys are a table. Both are the reader's alone.
///
/// A key obelus has never heard of reaches nowhere, which costs nothing --
/// nothing reads it either way -- and means a key added to the file before
/// it is added here cannot arrive from a tree.
#[must_use]
pub fn reach_of(key: &str) -> Reach {
    match key {
        "agent" | "keys" => Reach::ReaderOnly,
        _ => Setting::named(key).map_or(Reach::ReaderOnly, |setting| setting.reach),
    }
}

/// Lays a table of settings over a config, and says which keys it set.
///
/// Over, rather than into a fresh one: a tree's file names the few settings
/// that tree cares about, and everything it does not name is the reader's
/// and stays theirs. Reading it into a default config and taking that would
/// be a tree with one line in it turning off a reader's wrapped lines.
///
/// What the tree may not set is left alone, with a word in the log for
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
            tracing::warn!(key, "a tree may not set this, so it is left alone");
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
    if let Some(on) = table.get("blame").and_then(toml::Value::as_bool)
        && allowed("blame")
    {
        config.blame = on;
    }
    if let Some(on) = table.get("wrap").and_then(toml::Value::as_bool)
        && allowed("wrap")
    {
        config.wrap = on;
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
    set
}

/// The file's contents for a config.
#[must_use]
pub fn to_toml(config: &Config) -> String {
    let mut table = toml::Table::new();
    table.insert("theme".to_string(), config.theme.clone().into());
    table.insert("icons".to_string(), config.icons.into());
    table.insert("blame".to_string(), config.blame.into());
    table.insert("wrap".to_string(), config.wrap.into());
    // Written even when there is nobody, so the file says what obelus read
    // rather than leaving the reader to wonder whether it noticed.
    table.insert(
        "agent".to_string(),
        config.agent.clone().unwrap_or_default().into(),
    );
    // Only when the reader has moved something: an empty table in the file
    // says obelus was thinking about keys, which it was not.
    if !config.keys.is_empty() {
        let mut keys = toml::Table::new();
        for (name, chord) in &config.keys {
            keys.insert(name.clone(), chord.clone().into());
        }
        table.insert("keys".to_string(), keys.into());
    }
    toml::to_string(&table).unwrap_or_default()
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
    let Some(directory) = path.parent() else {
        return std::fs::write(path, to_toml(config));
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
    std::fs::write(&beside, to_toml(config))?;
    std::fs::rename(&beside, path)
}

#[cfg(test)]
mod tests {
    use super::{Config, Value, from_toml, save_to, to_toml};

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

    /// Written and read back is the same config: the file is the only place
    /// a setting survives, so anything that does not survive the round trip
    /// is a setting the reader has to set twice.
    #[test]
    fn a_config_survives_the_file() {
        let config = Config {
            theme: "light".to_string(),
            icons: false,
            blame: false,
            wrap: true,
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
                (kind, value) => {
                    panic!("{} is a {kind:?} holding a {value:?}", setting.key)
                }
            }
        }
    }
}
