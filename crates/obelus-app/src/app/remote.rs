//! Being reached from a chat: what this window keeps about it, and the page
//! a reader sets it up on.
//!
//! **What a platform has to be told is kept in two places and asked of one.**
//! A secret is in the keyring and anything else is in the reader's own
//! settings file, under `[remotes.<platform>]` -- and the page asks neither:
//! it is handed a [`Reached`] built here, the way the agents' page is handed
//! the agent's offering. The keyring is asked on the blocking pool when the
//! page first needs to say whether a token is kept, because asking it is a
//! D-Bus call that may put up a prompt to unlock it, and a prompt is not
//! something a keystroke waits for.
//!
//! **A setting the reader turned on is not a reason to refuse them.** A chat
//! chosen with no tokens is a chat that says so, at the top of the page, and
//! waits for them -- not a choice taken back.

use std::collections::BTreeMap;

use obelus_component::{
    prompt::{Prompt, PromptKind},
    settings::{Reached, Reaching},
};
use obelus_remote::{
    State,
    platform::{Description, Field, FieldKind, Setup},
    secrets::Trouble,
};

use super::*;
use crate::event::Event;

/// What this window knows about the chat it can be reached from.
#[derive(Debug, Default)]
pub(super) struct Remote {
    /// What each of the platform's secrets ends in, by field, for the
    /// platform they were read for -- never the secret itself, which stays
    /// in the keyring until something has to connect with it.
    kept: BTreeMap<&'static str, String>,
    /// Which secrets have been asked for and not yet answered, and for
    /// which platform: an answer for a platform the reader has since
    /// switched away from is about rows no longer on the page.
    asking: Option<(&'static str, usize)>,
    /// Which platform `kept` is about. `None` until the keyring has been
    /// asked, which is a different thing from having nothing in it.
    read_for: Option<&'static str>,
    /// Why the keyring would not answer, where it would not.
    trouble: Option<Trouble>,
    /// What is wrong with a field, by its key.
    troubles: BTreeMap<&'static str, String>,
    /// The code waiting to be sent to the bot, while there is one.
    pairing: Option<String>,
}

/// What a secret is drawn as on its row: the platform's prefix where it
/// starts with it, and its last four.
fn ends(secret: &str, looks_like: &str) -> String {
    let characters: Vec<char> = secret.chars().collect();
    let tail: String = characters[characters.len().saturating_sub(4)..]
        .iter()
        .collect();
    match secret.starts_with(looks_like) {
        true => format!("{looks_like}\u{2026}{tail}"),
        false => format!("\u{2026}{tail}"),
    }
}

impl App {
    /// The chat this machine is set to be reached from, where one is and
    /// Obelus knows it.
    pub(super) fn platform(&self) -> Option<&'static Description> {
        obelus_remote::platform::named(self.config().remote.as_deref()?)
    }

    /// Where this machine stands with it.
    ///
    /// Worked out from what is known rather than kept, so that it cannot
    /// say something the parts of it no longer do.
    pub(super) fn remote_state(&self) -> State {
        let Some(platform) = self.platform() else {
            return State::Off;
        };
        match &self.remote.trouble {
            Some(Trouble::NoKeyring) => return State::NoKeyring,
            Some(Trouble::Locked) => return State::Locked,
            Some(Trouble::Failed(_)) | None => {}
        }
        if self.remote.read_for != Some(platform.key) {
            return State::Connecting;
        }
        let told = |field: &Field| match field.kind {
            FieldKind::Secret { .. } => self.remote.kept.contains_key(field.key),
            FieldKind::Choice(_) | FieldKind::Text => self
                .config()
                .remote_value(platform.key, field.key)
                .is_some(),
        };
        match platform.fields.iter().all(told) {
            true => State::Connecting,
            false => State::Unready,
        }
    }

    /// What the remote page shows, from what this window knows.
    pub(super) fn reached(&self) -> Reached {
        let platform = self.platform();
        let mut kept = BTreeMap::new();
        if let Some(platform) = platform {
            for field in platform.fields {
                let shown = match field.kind {
                    FieldKind::Secret { .. } => self.remote.kept.get(field.key).cloned(),
                    FieldKind::Choice(_) | FieldKind::Text => self
                        .config()
                        .remote_value(platform.key, field.key)
                        .map(str::to_string),
                };
                if let Some(shown) = shown {
                    kept.insert(field.key, shown);
                }
            }
        }
        Reached {
            platform,
            state: self.remote_state(),
            kept,
            troubles: self.remote.troubles.clone(),
            people: platform
                .and_then(|platform| self.config().remote_of(platform.key))
                .map(|remote| {
                    remote
                        .people
                        .iter()
                        .map(|person| person.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
            pairing: self.remote.pairing.clone(),
        }
    }

    /// Hands the settings page what it shows about the chat, asking the
    /// keyring first where the page is about to say what it holds.
    ///
    /// Once a frame, from what is true: which platform is set can change
    /// under the page -- another window, the reader's own file -- and a page
    /// told only at the moments this window changed it would be told too
    /// rarely. Building the account is a handful of strings, and the page
    /// moves nothing unless it differs.
    pub(super) fn settle_the_remote_page(&mut self) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        if settings.on_remote()
            && let Some(platform) = self.platform()
            && self.remote.read_for != Some(platform.key)
            && self
                .remote
                .asking
                .is_none_or(|(asked, _)| asked != platform.key)
        {
            self.read_the_secrets(platform);
        }
        let reached = self.reached();
        if let Some(settings) = self.settings.as_mut() {
            settings.reach(reached);
        }
    }

    /// Asks the keyring what it keeps for each of a platform's secrets.
    fn read_the_secrets(&mut self, platform: &'static Description) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        let secrets: Vec<&'static Field> = platform
            .fields
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::Secret { .. }))
            .collect();
        self.remote.kept.clear();
        self.remote.trouble = None;
        if secrets.is_empty() {
            self.remote.read_for = Some(platform.key);
            return;
        }
        self.remote.asking = Some((platform.key, secrets.len()));
        for field in secrets {
            let sender = sender.clone();
            obelus_runtime::handle().spawn_blocking(move || {
                let read = obelus_remote::secrets::read(platform.key, field.key);
                let _ = sender.send(Event::Remote(obelus_remote::Event::Kept {
                    platform: platform.key,
                    field: field.key,
                    read,
                }));
            });
        }
    }

    /// Hears what the keyring said.
    pub(super) fn remote_event(&mut self, event: obelus_remote::Event) {
        match event {
            obelus_remote::Event::Kept {
                platform,
                field,
                read,
            } => self.kept(platform, field, read),
            obelus_remote::Event::Written {
                platform,
                field,
                written,
            } => match written {
                Ok(kept) => {
                    if self.platform().is_some_and(|set| set.key == platform) {
                        match kept {
                            Some(kept) => self.remote.kept.insert(field, kept),
                            None => self.remote.kept.remove(field),
                        };
                        self.remote.trouble = None;
                    }
                }
                Err(trouble) => {
                    tracing::warn!(platform, field, %trouble, "the keyring would not keep a token");
                    self.wrong(format!("Not kept: {trouble}"));
                    self.remote.trouble = Some(trouble);
                }
            },
        }
        self.settle_the_remote_page();
    }

    /// One secret's answer, read for a platform.
    fn kept(
        &mut self,
        platform: &'static str,
        field: &'static str,
        read: Result<Option<String>, Trouble>,
    ) {
        let Some((asked, waiting)) = self.remote.asking else {
            return;
        };
        if asked != platform {
            return;
        }
        let looks_like = obelus_remote::platform::named(platform)
            .and_then(|platform| platform.field(field))
            .and_then(|field| match field.kind {
                FieldKind::Secret { looks_like } => Some(looks_like),
                FieldKind::Choice(_) | FieldKind::Text => None,
            })
            .unwrap_or_default();
        match read {
            Ok(Some(secret)) => {
                self.remote.kept.insert(field, ends(&secret, looks_like));
            }
            Ok(None) => {}
            Err(trouble) => {
                tracing::warn!(platform, field, %trouble, "the keyring would not say what it keeps");
                self.remote.trouble = Some(trouble);
            }
        }
        match waiting {
            0 | 1 => {
                self.remote.asking = None;
                self.remote.read_for = Some(platform);
            }
            _ => self.remote.asking = Some((asked, waiting - 1)),
        }
    }

    /// Does what a row of the remote page asked for.
    pub(super) fn reach(&mut self, reaching: Reaching) {
        match reaching {
            Reaching::Platform => self.choose_a_platform(),
            Reaching::Edit(field) => match field.kind {
                // A word from a few: the next one, the way a switch is
                // flipped. A list for two or three words is a list nobody
                // needs.
                FieldKind::Choice(words) => {
                    let Some(platform) = self.platform() else {
                        return;
                    };
                    let now = self.config().remote_value(platform.key, field.key);
                    let at = words.iter().position(|word| Some(*word) == now);
                    let next = words[at.map_or(0, |at| (at + 1) % words.len())];
                    self.change_remote(|config| {
                        config.set_remote_value(platform.key, field.key, Some(next));
                    });
                }
                FieldKind::Secret { .. } | FieldKind::Text => {
                    // A secret is asked for from nothing: what is kept is
                    // not shown, so there is nothing to start from. Text is
                    // shown, and edited where it stands.
                    let text = match field.kind {
                        FieldKind::Text => self
                            .platform()
                            .and_then(|platform| {
                                self.config().remote_value(platform.key, field.key)
                            })
                            .unwrap_or_default()
                            .to_string(),
                        _ => String::new(),
                    };
                    self.ask_on_the_status_row(Prompt::about(PromptKind::Told(field), text));
                }
            },
            Reaching::Forget(field) => self.tell_the_remote(field, None),
            // Nothing to do yet but say so: who may talk is changed by
            // pairing, and taken away from the list that this opens.
            Reaching::People => self.open_the_people(),
            Reaching::Pair => self.pair(),
            Reaching::Setup => {
                let Some(platform) = self.platform() else {
                    return;
                };
                match &platform.setup {
                    Setup::Copy { what, text, .. } => self.copied(&text(), what),
                    Setup::Steps { url, .. } => {
                        if let Err(error) = obelus_clipboard::links::open(url) {
                            tracing::warn!(%error, url, "the steps would not open");
                            self.wrong(format!("Could not open {url}"));
                        }
                    }
                }
            }
        }
    }

    /// Hears what the reader typed for one of the platform's fields, or
    /// with `None` forgets it.
    ///
    /// A secret goes to the keyring on the blocking pool and the page hears
    /// back when it is kept; anything else is a line in the settings file,
    /// written the way every setting is.
    pub(super) fn tell_the_remote(&mut self, field: &'static Field, said: Option<&str>) {
        let Some(platform) = self.platform() else {
            return;
        };
        let said = said.map(str::trim).filter(|said| !said.is_empty());
        match field.kind {
            FieldKind::Secret { looks_like } => {
                // The prefix is what tells one token from another, and the
                // commonest mistake is the other one pasted into this row:
                // said where they are looking, and the prompt kept so that
                // the paste can be put right.
                if let Some(said) = said
                    && !looks_like.is_empty()
                    && !said.starts_with(looks_like)
                {
                    self.ask_on_the_status_row(Prompt::about(
                        PromptKind::Told(field),
                        said.to_string(),
                    ));
                    self.wrong(format!("{} starts with {looks_like}", field.name));
                    return;
                }
                let Some(sender) = self.events.clone() else {
                    return;
                };
                let said = said.map(str::to_string);
                obelus_runtime::handle().spawn_blocking(move || {
                    let written = match &said {
                        Some(said) => obelus_remote::secrets::write(platform.key, field.key, said)
                            .map(|()| Some(ends(said, looks_like))),
                        None => {
                            obelus_remote::secrets::forget(platform.key, field.key).map(|()| None)
                        }
                    };
                    let _ = sender.send(Event::Remote(obelus_remote::Event::Written {
                        platform: platform.key,
                        field: field.key,
                        written,
                    }));
                });
            }
            FieldKind::Choice(_) | FieldKind::Text => {
                self.change_remote(|config| config.set_remote_value(platform.key, field.key, said));
            }
        }
    }

    /// Changes what the reader's settings keep about a chat, and writes it.
    ///
    /// The same three steps every change to the reader's file takes:
    /// theirs, then the project's back over it -- which here takes nothing
    /// away, because no project may set any of this -- then the file.
    pub(super) fn change_remote(&mut self, change: impl FnOnce(&mut obelus_config::Config)) {
        change(&mut self.settled.readers);
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

    /// Offers the chats there are, and none.
    ///
    /// Through the same compact list every setting with choices opens,
    /// which asks the application for this one's words.
    fn choose_a_platform(&mut self) {
        let now = self.config().remote.clone().unwrap_or_default();
        self.open_choices("remote", &[], &now);
    }

    /// The list of people who may talk to this machine.
    fn open_the_people(&mut self) {
        // Pairing is how somebody is let in; this is how somebody is let
        // go, and it is the next piece of this page.
        tracing::debug!("the list of people is not drawn yet");
    }

    /// Makes a code for somebody to pair with, where there is anything
    /// listening for it.
    fn pair(&mut self) {
        // Silent where there is nothing to send it to: the row is drawn dim
        // there, and a key does nothing where its row is dim.
        if !self.remote_state().connected() {
            return;
        }
        tracing::debug!("pairing waits for the relay");
    }
}
