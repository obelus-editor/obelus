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
//! chosen with no tokens is a chat whose rows say so, and waits for them --
//! not a choice taken back.
//!
//! **One window talks to the chat, and the reader says which.** A bot has
//! one connection, and a platform hands each message to one of an app's
//! connections at random, so two windows connected is two windows each
//! hearing half. So none connects until told to, with `connect-remote`; the
//! window that is told holds a lock the kernel gives up with its process,
//! and one told while another holds it asks that one to let go -- a file
//! written beside the lock, which the holder watches -- and waits in the
//! kernel for it to. Where the chat stands is said on the status row of
//! that window and of no other: the settings page holds what the reader
//! set, and says only what is wrong with it.

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
    /// The code waiting to be sent to the bot, while there is one.
    pairing: Option<String>,
    /// The clock it runs out on. Dropped with the code, which stops it: a
    /// new code is a new clock, and a used one needs none.
    pairing_runs_out: Option<crate::event::Pause>,
    /// The number of the connection now wanted: every connection's events
    /// carry theirs, and one let go may still be saying something -- a late
    /// `Connected`, a `Refused` for a token since mended -- that is not
    /// about the one that replaced it.
    number: u64,
    /// Somebody who sent the code, while the platform is asked what they
    /// are called: who, the group they sent it in, and the thread it began.
    naming: Option<(String, String, String)>,
    /// Where to send what is to be said, while connected -- or connecting:
    /// the platform takes what is sent before it is up and says it once it
    /// is.
    out: Option<tokio::sync::mpsc::UnboundedSender<obelus_remote::model::Out>>,
    /// Which platform `out` is for, or is being asked for.
    reaching: Option<&'static str>,
    /// What the platform last said about the connection, for `reaching`.
    connection: Option<State>,
    /// The lock that makes this the one window on the machine the chat
    /// talks to, while it is: one bot can have one connection, and which
    /// window has it is the reader's to say, with `connect-remote`.
    holding: Option<std::fs::File>,
    /// The number of the last asking for it from a window that had it,
    /// and the clock that asking gives up on. The lock comes back on a
    /// thread waiting in the kernel, and one that comes back for an asking
    /// since given up on is let go again at once.
    taking: Option<(u64, Option<crate::event::Pause>)>,
    /// The last such number handed out.
    asked: u64,
    /// What this window last wrote to ask for the chat, which it hears too
    /// and must not take as somebody else asking.
    wrote: Option<String>,
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
        // What the platform itself said comes first: it is the one that
        // knows whether the tokens work.
        if self.remote.reaching == Some(platform.key)
            && let Some(state) = self.remote.connection
        {
            return state;
        }
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
            // A choice nobody made is its first word, which is an answer.
            FieldKind::Choice(_) => true,
            FieldKind::Text => self
                .config()
                .remote_value(platform.key, field.key)
                .is_some(),
        };
        match platform.fields.iter().all(told) {
            true => State::Connecting,
            false => State::Unready,
        }
    }

    /// Where this machine stands with its chat, for a test that drives a
    /// window with no settings page open.
    #[must_use]
    pub fn remote_state_for_test(&self) -> State {
        self.remote_state()
    }

    /// What the remote page shows, from what this window knows.
    pub(super) fn reached(&self) -> Reached {
        let platform = self.platform();
        let mut kept = BTreeMap::new();
        if let Some(platform) = platform {
            for field in platform.fields {
                let shown = match field.kind {
                    FieldKind::Secret { .. } => self.remote.kept.get(field.key).cloned(),
                    // Its first word where nobody has chosen, which is what
                    // connecting will use.
                    FieldKind::Choice(words) => self
                        .config()
                        .remote_value(platform.key, field.key)
                        .or(words.first().copied())
                        .map(str::to_string),
                    FieldKind::Text => self
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
    /// What one connection said, heard only while it is the one wanted.
    pub(super) fn reached_event(&mut self, number: u64, event: obelus_remote::Event) {
        if number != self.remote.number || self.remote.reaching.is_none() {
            tracing::info!(number, "a connection let go said something, not heard");
            return;
        }
        self.remote_event(event);
    }

    /// Whether there is somewhere to send what is to be said: a connection
    /// wanted and started, up or on its way back up. Not whether it is up
    /// this moment -- what is sent while it reconnects waits for it, and a
    /// turn that ended in those five seconds would otherwise never reach
    /// the thread.
    pub(super) fn chat_is_listening(&self) -> bool {
        self.remote.out.is_some()
    }

    pub(super) fn remote_event(&mut self, event: obelus_remote::Event) {
        match event {
            obelus_remote::Event::Kept {
                platform,
                field,
                read,
            } => self.kept(platform, field, read),
            obelus_remote::Event::Started { platform, out } => {
                if self.remote.reaching == Some(platform) {
                    self.remote.out = Some(out);
                    // A new connection knows nothing of what the old one
                    // was asked: those threads are asked for again.
                    self.forget_what_was_on_its_way();
                }
            }
            obelus_remote::Event::Connection(state) => {
                tracing::info!(?state, "the chat says where it has got to");
                self.remote.connection = Some(state);
            }
            obelus_remote::Event::Heard {
                from,
                room,
                at,
                text,
            } => self.heard(&from, &room, &at, &text),
            obelus_remote::Event::Named { id, name } => self.let_in(id, name),
            obelus_remote::Event::Opened { asked, thread, .. } => self.thread_opened(asked, thread),
            obelus_remote::Event::Unopened { asked } => self.thread_unopened(asked),
            obelus_remote::Event::PairingOver => {
                self.remote.pairing = None;
                self.remote.pairing_runs_out = None;
            }
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
                        // Connected again with what it has now: a token
                        // changed under a connection is a connection made
                        // with the old one.
                        self.let_the_chat_go();
                    }
                }
                Err(trouble) => {
                    tracing::warn!(platform, field, %trouble, "the keyring would not keep a token");
                    self.wrong(format!("Not kept: {trouble}"));
                    self.remote.trouble = Some(trouble);
                }
            },
        }
        self.settle_the_connection();
        self.settle_the_remote_page();
    }

    /// Connects to the chat that is set, or lets go of one that is not.
    ///
    /// Asked from what is true, once a frame and after anything that could
    /// move it: which chat is set can change under this window -- the
    /// reader's file, another window -- and a connection that was switched
    /// on and off from the places that changed it would outlive its reason
    /// the first time one of them forgot.
    pub(super) fn settle_the_connection(&mut self) {
        // A chat set to nothing holds nothing: the next window to want it
        // should not have to ask this one.
        if self.platform().is_none() {
            self.remote.holding = None;
            self.remote.taking = None;
        }
        let wanted = self.platform().filter(|_| self.remote.holding.is_some());
        if self.remote.reaching != wanted.map(|platform| platform.key) {
            self.let_the_chat_go();
        }
        let Some(platform) = wanted else {
            return;
        };
        if self.remote.reaching.is_some() {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        self.remote.reaching = Some(platform.key);
        let settled: obelus_remote::platform::Told = platform
            .fields
            .iter()
            .filter_map(|field| {
                Some((
                    field.key,
                    self.config()
                        .remote_value(platform.key, field.key)?
                        .to_string(),
                ))
            })
            .collect();
        tracing::info!(platform = platform.key, "reaching the chat");
        self.remote.number += 1;
        obelus_remote::platform::reach(
            platform,
            settled,
            std::sync::Arc::new(Numbered {
                number: self.remote.number,
                sender,
            }),
        );
    }

    /// Lets go of the connection, which is dropping where to send things.
    fn let_the_chat_go(&mut self) {
        if self.remote.reaching.is_some() {
            tracing::info!(platform = ?self.remote.reaching, "letting the chat go");
        }
        self.remote.out = None;
        self.remote.reaching = None;
        self.remote.connection = None;
        self.forget_what_was_on_its_way();
        self.remote.pairing = None;
        self.remote.pairing_runs_out = None;
        self.remote.naming = None;
    }

    /// Says something in a chat.
    pub(super) fn say_to(&self, out: obelus_remote::model::Out) {
        if let Some(sending) = &self.remote.out
            && sending.send(out).is_err()
        {
            tracing::warn!("the chat has stopped listening");
        }
    }

    /// Hears somebody in the chat.
    ///
    /// Nobody who is not on the list is answered -- not with "no", not with
    /// anything: a bot that answers strangers is a bot that says it is
    /// there. And nothing outside the room is heard, from anybody. The one
    /// thing that may come from anyone and anywhere is the code, while there
    /// is one, and the group it is sent in becomes the room.
    fn heard(&mut self, from: &str, room: &str, at: &obelus_remote::model::Where, text: &str) {
        let Some(platform) = self.platform() else {
            return;
        };
        if let obelus_remote::model::Where::Fresh(thread) = at
            && let Some(code) = &self.remote.pairing
            && same_code(code, text)
        {
            tracing::info!(platform = platform.key, "somebody sent the code");
            self.remote.pairing = None;
            self.remote.pairing_runs_out = None;
            self.remote.naming = Some((from.to_string(), room.to_string(), thread.clone()));
            self.say_to(obelus_remote::model::Out::Name {
                id: from.to_string(),
            });
            return;
        }
        if self.the_room().as_deref() != Some(room) {
            tracing::info!(platform = platform.key, "words outside the room, not heard");
            return;
        }
        let known = self
            .config()
            .remote_of(platform.key)
            .is_some_and(|remote| remote.people.iter().any(|person| person.id == from));
        if !known {
            tracing::info!(
                platform = platform.key,
                "somebody not on the list, not answered"
            );
            return;
        }
        match at {
            obelus_remote::model::Where::Thread(thread) => self.heard_in_thread(thread, text),
            obelus_remote::model::Where::Fresh(thread) => self.heard_fresh(thread, text),
        }
    }

    /// Lets in somebody who sent the code, now that their name is known,
    /// and takes the group they sent it in as the room.
    fn let_in(&mut self, id: String, name: String) {
        let Some((_, room, thread)) = self.remote.naming.take_if(|(naming, ..)| *naming == id)
        else {
            return;
        };
        let Some(platform) = self.platform() else {
            return;
        };
        self.change_remote(|config| {
            config.add_person(
                platform.key,
                obelus_config::Person {
                    id: id.clone(),
                    name: name.clone(),
                },
            );
        });
        self.keep_the_room(&room);
        self.say(format!("Paired {name}"));
        self.say_to(obelus_remote::model::Out::Say {
            room,
            thread,
            to: id,
            // And where to begin, now that there is somebody to begin: the
            // first thing a reader wonders after pairing is what to do next.
            text: format!(
                "Paired. This machine takes notes from you now.\n{}",
                platform.begin
            ),
            notify: false,
        });
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
        self.reach_for(reaching);
        // And the page told at once: the next key may arrive before the
        // next frame, and a code made by this one is what the reader is
        // looking for on the row.
        self.settle_the_remote_page();
    }

    fn reach_for(&mut self, reaching: Reaching) {
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
                    // From the word it is on, which is the first where
                    // nobody has chosen: the row says so.
                    let now = self
                        .config()
                        .remote_value(platform.key, field.key)
                        .or(words.first().copied());
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
    ///
    /// Good for five minutes, and then it is gone: a code is the one thing a
    /// stranger may send, so it is something that exists only while the
    /// reader is waiting for it. Run out by a clock of its own rather than a
    /// countdown on the row -- nothing on screen moves for it.
    fn pair(&mut self) {
        // Silent where there is nothing to send it to: the row is drawn dim
        // there, and a key does nothing where its row is dim.
        if !self.remote_state().connected() {
            return;
        }
        self.remote.pairing = Some(a_code());
        self.remote.pairing_runs_out = self.come_back_in(
            std::time::Duration::from_secs(5 * 60),
            Event::Remote(obelus_remote::Event::PairingOver),
        );
    }
}

/// A code to pair with: six characters nobody misreads, in two halves.
///
/// From the hasher std seeds with randomness, the way a window's key is: a
/// code lasts five minutes and is tried by typing it, so what it has to be
/// is unguessable by somebody who has not seen it, and that is plenty.
fn a_code() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};

    const LETTERS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    let mut drawn = hasher.finish();
    let mut code = String::new();
    for at in 0..6 {
        if at == 3 {
            code.push('-');
        }
        let index = usize::try_from(drawn % LETTERS.len() as u64).unwrap_or(0);
        code.push(char::from(LETTERS[index]));
        drawn /= LETTERS.len() as u64;
    }
    code
}

/// Whether what somebody sent is the code, however they typed it: in small
/// letters, without the dash, with a space either side.
fn same_code(code: &str, sent: &str) -> bool {
    let plain = |said: &str| {
        said.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|character| character.to_ascii_uppercase())
            .collect::<String>()
    };
    plain(code) == plain(sent)
}

/// Where one connection's events go, with its number on them.
struct Numbered {
    number: u64,
    sender: std::sync::mpsc::Sender<Event>,
}

impl obelus_sink::Sink<obelus_remote::Event> for Numbered {
    fn send(&self, event: obelus_remote::Event) -> Result<(), obelus_sink::Gone> {
        self.sender
            .send(Event::Reached(self.number, event))
            .map_err(|_| obelus_sink::Gone)
    }
}

/// Where the one window the chat talks to holds it, and where another asks
/// for it. One of each for the machine rather than the project or the
/// platform: a bot has one connection, whichever chat it is.
pub(super) fn remote_directory(_: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(obelus_logging::state_directory()?.join("remote"))
}

/// The file whose lock is the connection. Never read: on Windows a lock
/// keeps others from reading even the byte it is on.
fn the_lock() -> Option<std::fs::File> {
    let path = remote_directory(std::path::Path::new(""))?.join("talking.lock");
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()
}

/// The file a window writes its number into to ask for the connection.
fn the_wanting() -> Option<std::path::PathBuf> {
    Some(remote_directory(std::path::Path::new(""))?.join("wanted"))
}

impl App {
    /// Whether this is the window the chat talks to.
    pub(super) fn holds_the_remote(&self) -> bool {
        self.remote.holding.is_some()
    }

    /// The same, for a test with two windows.
    #[must_use]
    pub fn holds_the_remote_for_test(&self) -> bool {
        self.holds_the_remote()
    }

    /// The chat and where it stands, for the status row of the window it
    /// talks to -- or is on its way to, which is connecting -- and nothing
    /// for any other.
    pub(super) fn remote_badge(&self) -> Option<(&'static str, State)> {
        let platform = self.platform()?;
        match (&self.remote.holding, &self.remote.taking) {
            (Some(_), _) => Some((platform.name, self.remote_state())),
            (None, Some(_)) => Some((platform.name, State::Connecting)),
            (None, None) => None,
        }
    }

    /// Whether the chat's mark is turning.
    pub(super) fn remote_turning(&self) -> bool {
        self.remote_badge()
            .is_some_and(|(_, state)| state == State::Connecting)
    }

    /// Whether this window is the one the chat talks to, or is on its way
    /// to being.
    pub(super) fn has_the_remote(&self) -> bool {
        self.remote.holding.is_some() || self.remote.taking.is_some()
    }

    /// Makes this the window the chat talks to: at once where no other
    /// window has it, and where one has, by asking it to let go and waiting
    /// for the kernel to say it has.
    pub(super) fn connect_remote(&mut self) {
        if self.has_the_remote() {
            return;
        }
        let Some(platform) = self.platform() else {
            return;
        };
        let Some(lock) = the_lock() else {
            self.wrong(format!("Nowhere to hold {} from", platform.name));
            return;
        };
        if !obelus_agent::chats::held_by_somebody_else(&lock) {
            self.take_the_remote(lock);
            return;
        }
        // The other window hears this through its watch on the directory
        // and lets go; the lock comes back here when it has, or when its
        // process ends, which lets go of it too.
        self.remote.asked += 1;
        let number = self.remote.asked;
        // Words nobody else could write: the process, the asking, and the
        // moment. Not the process alone, which two windows in one process
        // share.
        let asking = format!(
            "{} {number} {}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        );
        if let Some(path) = the_wanting()
            && let Err(error) = std::fs::write(&path, &asking)
        {
            tracing::warn!(%error, "could not ask the other window for the chat");
        }
        self.remote.wrote = Some(asking);
        if let Some(events) = self.events.clone() {
            obelus_runtime::handle().spawn_blocking(move || {
                if obelus_agent::chats::wait_to_hold(&lock) {
                    let _ = events.send(Event::Held(number, lock));
                }
            });
        }
        // Given up on after a while, because the asking travels by a watch,
        // and a watch is freshness rather than a promise: a window that
        // never heard would leave this one turning for ever.
        let runs_out =
            self.come_back_in(std::time::Duration::from_secs(10), Event::NotLetGo(number));
        self.remote.taking = Some((number, runs_out));
    }

    /// The lock, come back from the kernel.
    pub(super) fn held_the_remote(&mut self, number: u64, lock: std::fs::File) {
        if self.remote.taking.as_ref().map(|(asked, _)| *asked) != Some(number) {
            // An asking given up on: dropped here, which lets go again.
            return;
        }
        self.take_the_remote(lock);
    }

    /// The other window never let go.
    pub(super) fn not_let_go(&mut self, number: u64) {
        if self.remote.taking.as_ref().map(|(asked, _)| *asked) != Some(number) {
            return;
        }
        self.remote.taking = None;
        if let Some(platform) = self.platform() {
            self.wrong(format!("Another window would not let {} go", platform.name));
        }
    }

    fn take_the_remote(&mut self, lock: std::fs::File) {
        self.remote.taking = None;
        self.remote.holding = Some(lock);
        if let Some(platform) = self.platform() {
            self.say(format!("{} talks to this window now", platform.name));
        }
    }

    /// Stops this window talking to the chat, which leaves it to no window
    /// until one asks.
    pub(super) fn disconnect_remote(&mut self) {
        if self.remote.holding.take().is_none() {
            return;
        }
        self.let_the_chat_go();
        if let Some(platform) = self.platform() {
            self.say(format!("{} talks to no window now", platform.name));
        }
    }

    /// Whether a path that changed is another window asking for the chat.
    pub(super) fn is_the_remote_wanted(&self, path: &std::path::Path) -> bool {
        the_wanting().is_some_and(|wanting| wanting == path)
    }

    /// Another window has asked for the chat: let go, so that it can have
    /// it. Not this window's own asking, which it hears too.
    pub(super) fn somebody_wants_the_remote(&mut self) {
        if self.remote.holding.is_none() {
            return;
        }
        let asker = the_wanting().and_then(|path| std::fs::read_to_string(path).ok());
        if asker.is_some() && asker == self.remote.wrote {
            return;
        }
        self.remote.holding = None;
        self.let_the_chat_go();
        if let Some(platform) = self.platform() {
            self.say(format!("{} went to another window", platform.name));
        }
    }
}
