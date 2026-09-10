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
}

/// The themes a reader can choose between.
const THEMES: &[&str] = &["dark", "light"];

/// Every setting obelus has.
pub const ALL: &[Setting] = &[
    Setting {
        key: "theme",
        label: "Colour theme",
        group: Group::Appearance,
        kind: Kind::Choice(THEMES),
    },
    Setting {
        key: "icons",
        label: "Nerd Font glyphs in lists and on the status bar",
        group: Group::Appearance,
        kind: Kind::Switch,
    },
    Setting {
        key: "wrap",
        label: "Wrap a line too long for the screen onto the next row",
        group: Group::Reading,
        kind: Kind::Switch,
    },
    Setting {
        key: "blame",
        label: "Who last changed the line the cursor is on",
        group: Group::Reading,
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

/// Reads the file, or the defaults for every way it can decline.
#[must_use]
pub fn load() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        // Not there yet is the ordinary case, not an error: obelus writes it
        // the first time something is changed.
        return Config::default();
    };
    from_toml(&text)
}

/// The config a file's contents describe, taking the default for anything
/// missing or of the wrong type.
#[must_use]
pub fn from_toml(text: &str) -> Config {
    let mut config = Config::default();
    let Ok(table) = text.parse::<toml::Table>() else {
        tracing::warn!("the config file is not toml, so the defaults it is");
        return config;
    };
    if let Some(word) = table.get("theme").and_then(toml::Value::as_str) {
        config.theme = word.to_string();
    }
    if let Some(on) = table.get("icons").and_then(toml::Value::as_bool) {
        config.icons = on;
    }
    if let Some(on) = table.get("blame").and_then(toml::Value::as_bool) {
        config.blame = on;
    }
    if let Some(on) = table.get("wrap").and_then(toml::Value::as_bool) {
        config.wrap = on;
    }
    config
}

/// The file's contents for a config.
#[must_use]
pub fn to_toml(config: &Config) -> String {
    let mut table = toml::Table::new();
    table.insert("theme".to_string(), config.theme.clone().into());
    table.insert("icons".to_string(), config.icons.into());
    table.insert("blame".to_string(), config.blame.into());
    table.insert("wrap".to_string(), config.wrap.into());
    toml::to_string(&table).unwrap_or_default()
}

/// Writes a config to a path, making its directory if it is not there.
///
/// The path is passed in rather than looked up here, so that nothing can
/// write to the reader's real file by accident: a test, or a probe run while
/// working on obelus, has to say where it is writing.
pub fn save_to(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(path, to_toml(config))
}

#[cfg(test)]
mod tests {
    use super::{Config, Value, from_toml, to_toml};

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
