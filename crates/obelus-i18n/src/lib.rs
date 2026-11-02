//! What Obelus says, in the reader's language.
//!
//! The words are Fluent, in `locales/<language>/<file>.ftl`, because a
//! plural and the order a sentence puts its parts in belong to the language
//! and not to the code asking for the sentence: `{ $n } files` is one
//! message in English with two forms and one in Chinese with one, and
//! neither is anything the caller should know.
//!
//! Each message is a function, written by the build -- `settings.ftl`'s
//! `group-appearance` is [`settings::group_appearance`] -- so a message that
//! is gone does not compile wherever it was asked for, and one asked for
//! with the wrong variables does not either. The build also holds every
//! language to the first: a message missing from one, or one reaching for a
//! variable the first does not pass, stops it (`build/generate.rs`).
//!
//! What is *not* here is as much the rule as what is. A command's name
//! (`open-file`), a setting's key and a theme's name are names, written the
//! way the reader's files write them, in any language. What Obelus says to
//! an agent -- the opening brief, the tools' descriptions, a refusal -- is
//! a prompt and stays English. And what arrives from elsewhere -- an agent,
//! a server, the system's own error -- is left exactly as it came.
//!
//! A message that takes nothing is said once per language, the first time
//! any is asked for, and handed out as a `&'static str` after that: the
//! tables that hold copy today hold `&'static str`, and a lookup on every
//! frame of something that never changes would be work a frame does not
//! need to do.

use std::sync::{
    LazyLock, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use fluent_bundle::{FluentArgs, FluentResource, FluentValue, concurrent::FluentBundle};

include!(concat!(env!("OUT_DIR"), "/messages.rs"));

/// A language Obelus can speak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    /// English, which every other language is checked against, and what is
    /// said where nothing else has been asked for.
    #[default]
    English,
    /// Chinese, in simplified characters.
    SimplifiedChinese,
}

/// Every word the `language` setting may hold: the system's, then each
/// language by its tag.
pub const CHOICES: &[&str] = &["auto", "en", "zh-CN"];

// A language here the build does not have messages for, or the other way
// round, would be a choice that says nothing or a translation nobody can
// choose.
const _: () = assert!(Language::ALL.len() == WORDS.len());
const _: () = assert!(CHOICES.len() == WORDS.len() + 1);

impl Language {
    /// Every language, in the order the build keeps their messages.
    pub const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    /// Its tag, which is the word the `language` setting holds for it.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
        }
    }

    /// What it calls itself. A name, in its own spelling: a reader looking
    /// for their language looks for it as they write it, which is the one
    /// thing they can be sure of reading.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        }
    }

    /// The language a tag names, if Obelus speaks it.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.word() == word)
    }

    /// Where its messages are kept.
    const fn index(self) -> usize {
        self as usize
    }

    /// The language a system's locale asks for, or English where it asks
    /// for one Obelus does not speak.
    ///
    /// By the language alone, whatever the system calls the rest:
    /// `zh_CN.UTF-8`, `zh-Hans-CN` and `zh-TW` are all Chinese, and
    /// simplified is closer for a reader of traditional than English is.
    #[must_use]
    pub fn of_the_locale(locale: &str) -> Self {
        let language = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match language.as_str() {
            "zh" => Self::SimplifiedChinese,
            _ => Self::English,
        }
    }
}

/// Which language is being spoken, by [`Language::index`].
///
/// A global, like the glyph switch and the tab width: one decision the whole
/// program shares, read by every function that says anything, and threading
/// it through would put a parameter on every sentence. English until the
/// settings say otherwise, so a test that never applies them reads English
/// whatever machine it runs on.
static SPOKEN: AtomicUsize = AtomicUsize::new(0);

/// What the system asked for, once somebody has asked it.
///
/// Asked from the start of a real Obelus and nowhere else, which is what
/// keeps `auto` from meaning the test machine's language in a test.
static SYSTEM: OnceLock<Language> = OnceLock::new();

/// Asks the system which language its reader reads, for `auto`.
///
/// Once, before the settings are first applied: a locale is not something
/// that changes under a running program, and asking is reading the
/// environment or the registry.
pub fn hear_the_system() {
    SYSTEM.get_or_init(|| {
        let locale = sys_locale::get_locale().unwrap_or_default();
        let language = Language::of_the_locale(&locale);
        tracing::info!(locale, language = language.word(), "the system's language");
        language
    });
}

/// Speaks what the `language` setting says: one of [`CHOICES`].
///
/// `auto` is the system's, where it has been heard, and English where it
/// has not. A word that names nothing is English too -- the settings have
/// already marked it in the file, and the program has to say something.
pub fn speak(choice: &str) {
    let language = match choice {
        "auto" => SYSTEM.get().copied().unwrap_or_default(),
        word => Language::named(word).unwrap_or_default(),
    };
    SPOKEN.store(language.index(), Ordering::Relaxed);
}

/// The language being spoken.
#[must_use]
pub fn spoken() -> Language {
    Language::ALL
        .get(SPOKEN.load(Ordering::Relaxed))
        .copied()
        .unwrap_or_default()
}

/// Something a message is given: a word or a number.
///
/// Its own type rather than Fluent's, so nothing outside this crate names
/// Fluent. A number stays a number, because which plural it takes is the
/// language's question.
#[derive(Debug)]
pub struct Arg(FluentValue<'static>);

impl From<&str> for Arg {
    fn from(word: &str) -> Self {
        Self(FluentValue::from(word.to_string()))
    }
}

impl From<String> for Arg {
    fn from(word: String) -> Self {
        Self(FluentValue::from(word))
    }
}

impl From<&String> for Arg {
    fn from(word: &String) -> Self {
        Self(FluentValue::from(word.clone()))
    }
}

impl From<std::borrow::Cow<'_, str>> for Arg {
    fn from(word: std::borrow::Cow<'_, str>) -> Self {
        Self(FluentValue::from(word.into_owned()))
    }
}

macro_rules! numbers {
    ($($number:ty),+) => {
        $(impl From<$number> for Arg {
            fn from(number: $number) -> Self {
                Self(FluentValue::from(number))
            }
        })+
    };
}

numbers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// One language's messages, and the ones that take nothing already said.
struct Catalogue {
    /// A bundle a file, so a name need only be one message in its own file.
    bundles: Vec<FluentBundle<FluentResource>>,
    plain: Vec<String>,
}

impl Catalogue {
    fn of(language: Language) -> Self {
        let tag = language.word();
        let bundles: Vec<_> = SOURCES[language.index()]
            .iter()
            .map(|source| bundle(tag, source))
            .collect();
        let plain = PLAIN
            .iter()
            .map(|(file, id)| match bundles.get(*file) {
                Some(bundle) => say(bundle, tag, id, None),
                None => id.to_string(),
            })
            .collect();
        Self { bundles, plain }
    }
}

/// One file's messages in one language.
fn bundle(tag: &str, source: &str) -> FluentBundle<FluentResource> {
    let identifier = tag.parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![identifier]);
    // Fluent puts invisible marks round every variable by default, so a
    // name written right to left cannot turn the sentence round it. They
    // are characters, and a terminal gives each a cell: every row with a
    // variable in it would come out wider than it measured.
    bundle.set_use_isolating(false);
    let resource = match FluentResource::try_new(source.to_string()) {
        Ok(resource) => resource,
        // The build parsed the same text and would have stopped.
        Err((resource, errors)) => {
            tracing::error!(tag, ?errors, "messages that will not parse");
            resource
        }
    };
    if let Err(errors) = bundle.add_resource(resource) {
        tracing::error!(tag, ?errors, "messages said twice");
    }
    bundle
}

/// Every language's messages, made the first time anything is said.
static CATALOGUES: LazyLock<Vec<Catalogue>> =
    LazyLock::new(|| Language::ALL.into_iter().map(Catalogue::of).collect());

fn catalogue() -> &'static Catalogue {
    let catalogues = &*CATALOGUES;
    catalogues
        .get(spoken().index())
        .or_else(|| catalogues.first())
        .expect("there is always English")
}

/// One message, formatted.
fn say(
    bundle: &FluentBundle<FluentResource>,
    tag: &str,
    id: &str,
    args: Option<&FluentArgs<'_>>,
) -> String {
    let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) else {
        // The build checked every language has every message.
        tracing::error!(tag, id, "no such message");
        return id.to_string();
    };
    let mut errors = Vec::new();
    let said = bundle
        .format_pattern(pattern, args, &mut errors)
        .into_owned();
    if !errors.is_empty() {
        tracing::warn!(tag, id, ?errors, "a message that would not format");
    }
    said
}

/// A message that takes nothing, by where [`PLAIN`] has it.
fn plain(index: usize) -> &'static str {
    catalogue().plain.get(index).map_or("", String::as_str)
}

/// A message that takes something, by its file's place in [`SOURCES`].
fn formatted(file: usize, id: &str, args: impl IntoIterator<Item = (&'static str, Arg)>) -> String {
    let mut fluent = FluentArgs::new();
    for (name, Arg(value)) in args {
        fluent.set(name, value);
    }
    match catalogue().bundles.get(file) {
        Some(bundle) => say(bundle, spoken().word(), id, Some(&fluent)),
        None => id.to_string(),
    }
}

#[cfg(test)]
#[path = "../build/generate.rs"]
#[allow(dead_code)]
mod generate;

#[cfg(test)]
mod tests;
