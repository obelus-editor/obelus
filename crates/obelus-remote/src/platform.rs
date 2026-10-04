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

use std::{collections::BTreeMap, sync::Arc};

use obelus_sink::Sink;

use crate::{Event, feishu, model::Out, slack};

/// Everything a platform has been told, by field: the secrets read from the
/// keyring and the rest from the settings, put together only on the way to
/// connecting.
pub type Told = BTreeMap<&'static str, String>;

/// How a platform is connected to: what it has been told, and where what it
/// hears goes. What it hands back is where to send what is to be said;
/// dropping that is how it is stopped.
pub type Connect = fn(Told, Arc<dyn Sink<Event>>) -> tokio::sync::mpsc::UnboundedSender<Out>;

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
    /// How a conversation is begun in the room, said to the reader when
    /// they are let in: a thread they start is a conversation, and what
    /// starts a thread is the platform's.
    pub begin: &'static str,
    /// How it is connected to.
    pub connect: Connect,
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

impl Setup {
    /// What its row is called.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Copy { name, .. } | Self::Steps { name, .. } => name,
        }
    }

    /// What it says under the name.
    #[must_use]
    pub const fn about(&self) -> &'static str {
        match self {
            Self::Copy { about, .. } | Self::Steps { about, .. } => about,
        }
    }
}

/// Each of these is a static, made once and never copied, so two are the
/// same one exactly when they are at the same address -- which is a question
/// asked about a row on the page, never about what the rows say.
macro_rules! the_same_one {
    ($($kind:ty),*) => {$(
        impl PartialEq for $kind {
            fn eq(&self, other: &Self) -> bool {
                std::ptr::eq(self, other)
            }
        }
        impl Eq for $kind {}
    )*};
}
the_same_one!(Description, Field, Setup);

/// What connects instead of the platform's own, for a test: one that went
/// out to Slack would be a test that needs Slack.
static CONNECT_FOR_TEST: std::sync::Mutex<Option<Connect>> = std::sync::Mutex::new(None);

/// Connects with this instead of any platform's own, for the rest of the
/// process.
pub fn connect_for_test(connect: Connect) {
    if let Ok(mut set) = CONNECT_FOR_TEST.lock() {
        *set = Some(connect);
    }
}

/// Reads what a platform has been told and connects to it, on the runtime's
/// blocking pool -- reading a secret may put up the keyring's own prompt.
///
/// What comes back comes through the sink: `Event::Started` with where to
/// send things, or the state that says why not -- a field nobody has filled
/// in, a keyring that would not open.
pub fn reach(platform: &'static Description, settled: Told, sink: Arc<dyn Sink<Event>>) {
    obelus_runtime::handle().spawn_blocking(move || {
        let mut told = settled;
        // A choice nobody has made is its first word, the way the page
        // draws it: Feishu's domain is Feishu until the reader says Lark.
        for field in platform.fields {
            if let FieldKind::Choice(words) = field.kind
                && let Some(first) = words.first()
            {
                told.entry(field.key)
                    .or_insert_with(|| (*first).to_string());
            }
        }
        for field in platform.fields {
            if let FieldKind::Secret { .. } = field.kind {
                match crate::secrets::read(platform.key, field.key) {
                    Ok(Some(secret)) => {
                        told.insert(field.key, secret);
                    }
                    Ok(None) => {}
                    Err(trouble) => {
                        let state = match trouble {
                            crate::secrets::Trouble::NoKeyring => crate::State::NoKeyring,
                            crate::secrets::Trouble::Locked => crate::State::Locked,
                            crate::secrets::Trouble::Failed(_) => crate::State::Unreachable,
                        };
                        let _ = sink.send(Event::connection(state, Some(trouble.to_string())));
                        return;
                    }
                }
            }
        }
        let untold: Vec<&str> = platform
            .fields
            .iter()
            .filter(|field| !told.contains_key(field.key))
            .map(|field| field.name)
            .collect();
        if !untold.is_empty() {
            let _ = sink.send(Event::connection(
                crate::State::Unready,
                Some(format!("no {} is set", untold.join(" or "))),
            ));
            return;
        }
        let connect = CONNECT_FOR_TEST
            .lock()
            .ok()
            .and_then(|set| *set)
            .unwrap_or(platform.connect);
        let out = connect(told, sink.clone());
        let _ = sink.send(Event::Started {
            platform: platform.key,
            out,
        });
    });
}

/// Every platform Obelus can be reached from, in the order they are offered.
pub const ALL: &[&Description] = &[&feishu::DESCRIPTION, &slack::DESCRIPTION];

/// A platform by its key.
#[must_use]
pub fn named(key: &str) -> Option<&'static Description> {
    ALL.iter().copied().find(|platform| platform.key == key)
}
