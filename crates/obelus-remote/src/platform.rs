//! What a chat platform says about itself, before anything is connected.
//!
//! **A platform declares; it does not keep.** What it needs to be told --
//! two tokens for Slack, an id, a secret and which of two servers for
//! Feishu -- is a list of [`Field`]s, and where each one is kept is decided
//! once, here, for all of them: a secret in the keyring, anything else in the
//! reader's settings file under `[remotes.<platform>]`. The settings page lays
//! its rows out from the same list, so a platform added later is a
//! description and an adapter, and not a page of its own.
//!
//! The alternative was each platform storing its own, which is three copies
//! of the keyring's rules -- where a development build keeps them, that a
//! test must not touch the reader's keyring, that a project may not set any
//! of it -- and a settings page that would have to ask every platform what
//! its values are.

use crate::slack;

/// One platform, as the settings page and the relay know it.
#[derive(Debug)]
pub struct Description {
    /// Its key: what `remote = "…"` names, the table its settings are kept
    /// under, and the directory its relay keeps its state in.
    pub key: &'static str,
    /// What it is called, which is a name and keeps its own spelling.
    pub name: &'static str,
    /// What it has to be told, in the order a reader setting it up for the
    /// first time fills them in.
    pub fields: &'static [Field],
    /// How a reader makes the app on the platform's side.
    pub setup: Setup,
}

impl Description {
    /// One of its fields, by key.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&'static Field> {
        self.fields.iter().find(|field| field.key == key)
    }
}

/// Something a platform has to be told.
#[derive(Debug)]
pub struct Field {
    /// The platform's own name for it, which is also how it is kept.
    pub key: &'static str,
    /// What the row calls it.
    pub name: &'static str,
    /// Where to find it, under the name.
    pub about: &'static str,
    /// What sort of thing it is, which decides where it is kept.
    pub kind: FieldKind,
}

/// What sort of thing a field is.
#[derive(Debug)]
pub enum FieldKind {
    /// Kept in the keyring and never in a file, and never drawn whole.
    Secret {
        /// What one starts with, where the platform says: a token pasted
        /// into the wrong row is the commonest way to get this wrong, and
        /// the prefix is what tells them apart.
        looks_like: &'static str,
    },
    /// One of a few words, kept in the settings file.
    Choice(&'static [&'static str]),
    /// A line of text, kept in the settings file.
    Text,
}

/// How the app on the platform's side is made.
#[derive(Debug)]
pub enum Setup {
    /// Something to copy and paste into the platform: Slack makes an app
    /// from a manifest.
    Copy {
        /// What the row calls it.
        name: &'static str,
        /// Where it goes, under the name.
        about: &'static str,
        /// What a copy of it is called, mid-sentence: `Copied {what}`.
        what: &'static str,
        /// The text itself.
        text: fn() -> String,
    },
    /// A page of steps to follow, for a platform with nothing to paste.
    Steps {
        /// What the row calls it.
        name: &'static str,
        /// What the steps are for, under the name.
        about: &'static str,
        /// Where they are.
        url: &'static str,
    },
}

/// Every platform Obelus can be reached from, in the order they are offered.
pub const ALL: &[&Description] = &[&slack::DESCRIPTION];

/// A platform by its key.
#[must_use]
pub fn named(key: &str) -> Option<&'static Description> {
    ALL.iter().copied().find(|platform| platform.key == key)
}
