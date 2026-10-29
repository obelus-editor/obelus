//! Being reached from a chat: what this window keeps about it, and the page
//! a reader sets it up on.
//!
//! **What a platform has to be told is kept in two places and asked of one.**
//! A secret is in the keyring and anything else is in the reader's own
//! settings file, under `[chats.<platform>]` -- and the page asks neither:
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
//! hearing half. So none connects until told to, with `connect-to-chat`; the
//! window that is told holds a lock the kernel gives up with its process,
//! and one told while another holds it asks that one to let go -- a file
//! written beside the lock, which the holder watches -- and waits in the
//! kernel for it to. Every other window is heard in the chat through that
//! one (`app/relaying`). Where the chat stands is said in words on the
//! status row of that window and of no other, and marked on all of theirs:
//! the settings page holds what the reader set, and says only what is wrong
//! with it.

use std::collections::BTreeMap;

use obelus_bot::{
    State,
    platform::{Description, Field, FieldKind, Setup},
    secrets::Trouble,
};
use obelus_component::{
    prompt::{Prompt, PromptKind},
    settings::{Reached, Reaching},
};

use crate::{app::*, event::Event};

/// What this window knows about the chat it can be reached from.
#[derive(Debug, Default)]
pub(in crate::app) struct Bot {
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
    /// A code asked for before the chat was here, made once it connects.
    pair_once_connected: bool,
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
    out: Option<tokio::sync::mpsc::UnboundedSender<obelus_bot::model::Out>>,
    /// Which platform `out` is for, or is being asked for.
    reaching: Option<&'static str>,
    /// What the platform last said about the connection, for `reaching`.
    connection: Option<State>,
    /// And why, where it said something was wrong and why.
    why: Option<String>,
    /// The lock that makes this the one window on the machine the chat
    /// talks to, while it is: one bot can have one connection, and which
    /// window has it is the reader's to say, with `connect-to-chat`.
    holding: Option<std::fs::File>,
    /// The number of the last asking for it from a window that had it,
    /// and the clock that asking gives up on. The lock comes back on a
    /// thread waiting in the kernel, and one that comes back for an asking
    /// since given up on is let go again at once.
    taking: Option<(u64, Option<crate::event::Pause>)>,
    /// The last such number handed out.
    asked: u64,
    /// The number of the last clock an asking was given, numbered apart
    /// from the askings: a clock dropped after it has run out has already
    /// said so, and an asking after it waits on the same thread and so
    /// under the same number.
    clocks: u64,
    /// The number of the thread waiting in the kernel for the lock, while
    /// one is. One at a time: an asking given up on leaves its thread
    /// waiting, and asking again is the same wait with a new clock -- a new
    /// thread each time was a thread more each time, all blocked on a
    /// window that would not let go.
    waiter: Option<u64>,
    /// How many such threads this window has started, ever: what a test
    /// counts to see that asking again did not start another.
    waiters: u64,
    /// An asking given up on because the other window did not answer in
    /// time -- not one the reader took back. Its request still stands, so
    /// a window that hears late and lets go leaves the chat here rather
    /// than with nobody.
    given_up: Option<u64>,
    /// What this window last wrote to ask for the chat, which it hears too
    /// and must not take as somebody else asking.
    wrote: Option<String>,
    /// The number of the waiting thread, while this window waits to take
    /// up a chat whose window went without letting it go.
    succeeding: Option<u64>,
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
    pub(in crate::app) fn platform(&self) -> Option<&'static Description> {
        obelus_bot::platform::named(self.config().chat.as_deref()?)
    }

    /// Where this machine stands with it.
    ///
    /// Worked out from what is known rather than kept, so that it cannot
    /// say something the parts of it no longer do.
    pub(in crate::app) fn bot_state(&self) -> State {
        let Some(platform) = self.platform() else {
            return State::Off;
        };
        // What the platform itself said comes first: it is the one that
        // knows whether the tokens work.
        if (self.bot.reaching == Some(platform.key) || self.joined_the_relay())
            && let Some(state) = self.bot.connection
        {
            return state;
        }
        match &self.bot.trouble {
            Some(Trouble::NoKeyring) => return State::NoKeyring,
            Some(Trouble::Locked) => return State::Locked,
            Some(Trouble::Failed(_)) | None => {}
        }
        if self.bot.read_for != Some(platform.key) {
            return State::Connecting;
        }
        let told = |field: &Field| match field.kind {
            FieldKind::Secret { .. } => self.bot.kept.contains_key(field.key),
            // A choice nobody made is its first word, which is an answer.
            FieldKind::Choice(_) => true,
            FieldKind::Text => self.config().chat_value(platform.key, field.key).is_some(),
        };
        match platform.fields.iter().all(told) {
            true => State::Connecting,
            false => State::Unready,
        }
    }

    /// Where this machine stands with its chat, for a test that drives a
    /// window with no settings page open.
    #[must_use]
    pub fn bot_state_for_test(&self) -> State {
        self.bot_state()
    }

    /// What the chat page shows, from what this window knows.
    pub(in crate::app) fn reached(&self) -> Reached {
        let platform = self.platform();
        let mut kept = BTreeMap::new();
        if let Some(platform) = platform {
            for field in platform.fields {
                let shown = match field.kind {
                    FieldKind::Secret { .. } => self.bot.kept.get(field.key).cloned(),
                    // Its first word where nobody has chosen, which is what
                    // connecting will use.
                    FieldKind::Choice(words) => self
                        .config()
                        .chat_value(platform.key, field.key)
                        .or(words.first().copied())
                        .map(str::to_string),
                    FieldKind::Text => self
                        .config()
                        .chat_value(platform.key, field.key)
                        .map(str::to_string),
                };
                if let Some(shown) = shown {
                    kept.insert(field.key, shown);
                }
            }
        }
        Reached {
            platform,
            state: self.bot_state(),
            why: platform
                .filter(|platform| {
                    self.bot.reaching == Some(platform.key) || self.joined_the_relay()
                })
                .and(self.bot.why.clone()),
            kept,
            people: platform
                .and_then(|platform| self.config().chat_of(platform.key))
                .map(|remote| {
                    remote
                        .people
                        .iter()
                        .map(|person| person.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
            pairing: self.bot.pairing.clone(),
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
    pub(in crate::app) fn settle_the_bot_page(&mut self) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        if settings.on_bot()
            && let Some(platform) = self.platform()
            && self.bot.read_for != Some(platform.key)
            && self
                .bot
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
        self.bot.kept.clear();
        self.bot.trouble = None;
        if secrets.is_empty() {
            self.bot.read_for = Some(platform.key);
            return;
        }
        self.bot.asking = Some((platform.key, secrets.len()));
        for field in secrets {
            let sender = sender.clone();
            obelus_runtime::handle().spawn_blocking(move || {
                let read = obelus_bot::secrets::read(platform.key, field.key);
                let _ = sender.send(Event::Bot(obelus_bot::Event::Kept {
                    platform: platform.key,
                    field: field.key,
                    read,
                }));
            });
        }
    }

    /// Hears what the keyring said.
    /// What one connection said, heard only while it is the one wanted.
    pub(in crate::app) fn reached_event(&mut self, number: u64, event: obelus_bot::Event) {
        if number != self.bot.number || (self.bot.reaching.is_none() && !self.joined_the_relay()) {
            tracing::info!(number, "a connection let go said something, not heard");
            return;
        }
        self.bot_event(event);
    }

    /// Whether there is somewhere to send what is to be said: a connection
    /// wanted and started, up or on its way back up. Not whether it is up
    /// this moment -- what is sent while it reconnects waits for it, and a
    /// turn that ended in those five seconds would otherwise never reach
    /// the thread.
    pub(in crate::app) fn chat_is_listening(&self) -> bool {
        self.bot.out.is_some() || self.relayed()
    }

    /// Where to send what is to be said to the platform, while this window
    /// is connected to it itself.
    pub(in crate::app) fn platform_out(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<obelus_bot::model::Out>> {
        self.bot.out.clone()
    }

    /// What the platform last said about the connection, and why.
    pub(in crate::app) fn connection_said(&self) -> Option<(State, Option<String>)> {
        self.bot
            .connection
            .map(|state| (state, self.bot.why.clone()))
    }

    /// Forgets what the relay said about the connection, which was about
    /// a connection this window is no longer heard on.
    pub(in crate::app) fn forget_what_the_relay_said(&mut self) {
        if self.bot.reaching.is_none() {
            self.bot.connection = None;
            self.bot.why = None;
        }
    }

    /// A new number for a connection, and where its events go with it on.
    pub(in crate::app) fn a_numbered_connection(
        &mut self,
    ) -> Option<std::sync::Arc<dyn obelus_sink::Sink<obelus_bot::Event>>> {
        let sender = self.events.clone()?;
        self.bot.number += 1;
        Some(std::sync::Arc::new(Numbered {
            number: self.bot.number,
            sender,
        }))
    }

    pub(in crate::app) fn bot_event(&mut self, event: obelus_bot::Event) {
        let Some(event) = self.whose_is_it(event) else {
            self.settle_the_connection();
            self.settle_the_bot_page();
            return;
        };
        // A window heard through another: the relay has heard who it is
        // from and where, and says nothing about the connection it does
        // not mean this window to say.
        let joined = self.joined_the_relay();
        match event {
            obelus_bot::Event::Kept {
                platform,
                field,
                read,
            } => self.kept(platform, field, read),
            obelus_bot::Event::Started { platform, out } => {
                if self.bot.reaching == Some(platform) {
                    self.bot.out = Some(out);
                    // A new connection knows nothing of what the old one
                    // was asked: those threads are asked for again, here
                    // and in every window heard through this one.
                    self.forget_what_was_on_its_way();
                    self.relay_restarted();
                }
            }
            obelus_bot::Event::Connection { state, why } if joined => {
                // Marked on the status row, and said in words only by the
                // window that holds the chat.
                self.bot.connection = Some(state);
                self.bot.why = why;
                if state == State::Connected {
                    self.no_longer_taking_up_the_chat();
                    self.threads_may_open_again();
                }
            }
            obelus_bot::Event::Connection { state, why } => {
                tracing::info!(?state, ?why, "the chat says where it has got to");
                self.relay_connection(state, why.clone());
                let was = self.bot.connection.replace(state);
                // Said on the status row once, on the way into it: a
                // connection tried again every few seconds that put the
                // same words up every time would be a row nobody could use
                // for anything else. The mark beside it stays.
                if state.wrong()
                    && was != Some(state)
                    && let Some(platform) = self.platform()
                {
                    let said = match &why {
                        Some(why) => format!("Not connected to {}: {why}", platform.name),
                        None => format!("Not connected to {}", platform.name),
                    };
                    self.wrong(said);
                }
                self.bot.why = why;
                // Up again -- a socket made again inside one connection is
                // not a new connection -- so what would not open, whether a
                // refusal or a request that timed out, is asked for again.
                if state == State::Connected {
                    self.threads_may_open_again();
                }
                match state {
                    State::Connected if std::mem::take(&mut self.bot.pair_once_connected) => {
                        self.pair();
                    }
                    // A code for a chat that will not have this window's
                    // tokens is a code nobody will be asked for.
                    State::Refused => self.bot.pair_once_connected = false,
                    _ => {}
                }
            }
            obelus_bot::Event::Heard {
                from,
                room,
                at,
                text,
            } => self.heard(&from, &room, &at, &text),
            obelus_bot::Event::Named { id, name } => self.let_in(id, name),
            obelus_bot::Event::Unasked { asked } => self.question_not_put(asked),
            obelus_bot::Event::Unsaid { thread, why } => self.thread_unsaid(&thread, why),
            obelus_bot::Event::Answered {
                from,
                asked,
                chosen,
                words,
            } => {
                if joined || self.on_the_list(&from) {
                    self.answered_on_a_card(asked, &chosen, words.as_deref());
                }
            }
            obelus_bot::Event::Cancelled { from, asked } => {
                if joined || self.on_the_list(&from) {
                    self.cancelled_on_a_card(asked);
                }
            }
            obelus_bot::Event::Restarted => {
                if joined {
                    self.relay_has_started();
                }
            }
            obelus_bot::Event::Left(over) => {
                if joined {
                    self.left_the_relay(over);
                }
            }
            obelus_bot::Event::Room(room) => {
                if joined {
                    self.room_from_the_relay(room);
                }
            }
            obelus_bot::Event::Window { window, did } => self.window_did(window, did),
            obelus_bot::Event::Opened { asked, thread, .. } => self.thread_opened(asked, thread),
            obelus_bot::Event::Unopened { asked, waited } => {
                self.thread_unopened(asked, waited);
            }
            obelus_bot::Event::PairingOver => {
                self.bot.pairing = None;
                self.bot.pairing_runs_out = None;
            }
            obelus_bot::Event::Written {
                platform,
                field,
                written,
            } => match written {
                Ok(kept) => {
                    if self.platform().is_some_and(|set| set.key == platform) {
                        match kept {
                            Some(kept) => self.bot.kept.insert(field, kept),
                            None => self.bot.kept.remove(field),
                        };
                        self.bot.trouble = None;
                        // Connected again with what it has now: a token
                        // changed under a connection is a connection made
                        // with the old one.
                        self.let_the_chat_go();
                    }
                }
                Err(trouble) => {
                    tracing::warn!(platform, field, %trouble, "the keyring would not keep a token");
                    self.wrong(format!("Not kept: {trouble}"));
                    self.bot.trouble = Some(trouble);
                }
            },
        }
        self.settle_the_connection();
        self.settle_the_bot_page();
    }

    /// Connects to the chat that is set, or lets go of one that is not.
    ///
    /// Asked from what is true, once a frame and after anything that could
    /// move it: which chat is set can change under this window -- the
    /// reader's file, another window -- and a connection that was switched
    /// on and off from the places that changed it would outlive its reason
    /// the first time one of them forgot.
    pub(in crate::app) fn settle_the_connection(&mut self) {
        // A chat set to nothing holds nothing: the next window to want it
        // should not have to ask this one.
        if self.platform().is_none() {
            if self.bot.holding.is_some() {
                self.stop_relaying(obelus_bot::relay::Over::Stopped);
            }
            self.bot.holding = None;
            self.bot.taking = None;
            self.bot.succeeding = None;
        }
        let wanted = self.platform().filter(|_| self.bot.holding.is_some());
        if self.bot.reaching != wanted.map(|platform| platform.key) {
            self.let_the_chat_go();
        }
        let Some(platform) = wanted else {
            return;
        };
        if self.bot.reaching.is_some() {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        self.bot.reaching = Some(platform.key);
        let settled: obelus_bot::platform::Told = platform
            .fields
            .iter()
            .filter_map(|field| {
                Some((
                    field.key,
                    self.config()
                        .chat_value(platform.key, field.key)?
                        .to_string(),
                ))
            })
            .collect();
        tracing::info!(platform = platform.key, "reaching the chat");
        self.bot.number += 1;
        obelus_bot::platform::reach(
            platform,
            settled,
            std::sync::Arc::new(Numbered {
                number: self.bot.number,
                sender,
            }),
        );
    }

    /// Lets go of the connection, which is dropping where to send things.
    fn let_the_chat_go(&mut self) {
        if self.bot.reaching.is_some() {
            tracing::info!(platform = ?self.bot.reaching, "letting the chat go");
        }
        self.bot.out = None;
        self.bot.reaching = None;
        self.bot.connection = None;
        self.bot.why = None;
        self.forget_what_was_on_its_way();
        self.bot.pairing = None;
        self.bot.pairing_runs_out = None;
        self.bot.naming = None;
    }

    /// Says something in a chat: to the platform where this window holds
    /// it, and through the window that does where it is heard through one.
    pub(in crate::app) fn say_to(&mut self, out: obelus_bot::model::Out) {
        match self.bot.out.is_some() {
            true => self.send_to_the_platform(obelus_bot::relay::Asker::Here, out),
            false => self.to_the_relay(out),
        }
    }

    /// Hears somebody in the chat.
    ///
    /// Nobody who is not on the list is answered -- not with "no", not with
    /// anything: a bot that answers strangers is a bot that says it is
    /// there. And nothing outside the room is heard, from anybody. The one
    /// thing that may come from anyone and anywhere is the code, while there
    /// is one, and the group it is sent in becomes the room.
    fn heard(&mut self, from: &str, room: &str, at: &obelus_bot::model::Where, text: &str) {
        let Some(platform) = self.platform() else {
            return;
        };
        // Through a relay, which has heard who it is from and where -- and
        // which may have been told of a person or a room this window's
        // settings have not caught up with yet.
        if self.joined_the_relay() {
            match at {
                obelus_bot::model::Where::Thread(thread) => self.heard_in_thread(thread, text),
                obelus_bot::model::Where::Fresh(thread) => self.heard_fresh(thread, text),
            }
            return;
        }
        if let obelus_bot::model::Where::Fresh(thread) = at
            && let Some(code) = &self.bot.pairing
            && same_code(code, text)
        {
            tracing::info!(platform = platform.key, "somebody sent the code");
            self.bot.pairing = None;
            self.bot.pairing_runs_out = None;
            self.bot.naming = Some((from.to_string(), room.to_string(), thread.clone()));
            self.say_to(obelus_bot::model::Out::Name {
                id: from.to_string(),
            });
            return;
        }
        if self.the_room().as_deref() != Some(room) {
            tracing::info!(platform = platform.key, "words outside the room, not heard");
            return;
        }
        if !self.on_the_list(from) {
            tracing::info!(
                platform = platform.key,
                "somebody not on the list, not answered"
            );
            return;
        }
        self.route_heard(from, room, at, text);
    }

    /// Whether somebody is on the list of who may talk to this machine:
    /// nobody else is answered, in words or on a card.
    pub(in crate::app) fn on_the_list(&self, who: &str) -> bool {
        self.platform().is_some_and(|platform| {
            self.config()
                .chat_of(platform.key)
                .is_some_and(|remote| remote.people.iter().any(|person| person.id == who))
        })
    }

    /// Lets in somebody who sent the code, now that their name is known,
    /// and takes the group they sent it in as the room.
    fn let_in(&mut self, id: String, name: String) {
        let Some((_, room, thread)) = self.bot.naming.take_if(|(naming, ..)| *naming == id) else {
            return;
        };
        let Some(platform) = self.platform() else {
            return;
        };
        self.change_bot(|config| {
            config.add_person(
                platform.key,
                obelus_config::Person {
                    id: id.clone(),
                    name: name.clone(),
                },
            );
        });
        self.keep_the_room(&room);
        self.relay_room(&room);
        self.say(format!("Paired {name}"));
        self.say_to(obelus_bot::model::Out::Say {
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
        let Some((asked, waiting)) = self.bot.asking else {
            return;
        };
        if asked != platform {
            return;
        }
        let looks_like = obelus_bot::platform::named(platform)
            .and_then(|platform| platform.field(field))
            .and_then(|field| match field.kind {
                FieldKind::Secret { looks_like } => Some(looks_like),
                FieldKind::Choice(_) | FieldKind::Text => None,
            })
            .unwrap_or_default();
        match read {
            Ok(Some(secret)) => {
                self.bot.kept.insert(field, ends(&secret, looks_like));
            }
            Ok(None) => {}
            Err(trouble) => {
                tracing::warn!(platform, field, %trouble, "the keyring would not say what it keeps");
                self.bot.trouble = Some(trouble);
            }
        }
        match waiting {
            0 | 1 => {
                self.bot.asking = None;
                self.bot.read_for = Some(platform);
            }
            _ => self.bot.asking = Some((asked, waiting - 1)),
        }
    }

    /// Does what a row of the chat page asked for.
    pub(in crate::app) fn reach(&mut self, reaching: Reaching) {
        self.reach_for(reaching);
        // And the page told at once: the next key may arrive before the
        // next frame, and a code made by this one is what the reader is
        // looking for on the row.
        self.settle_the_bot_page();
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
                        .chat_value(platform.key, field.key)
                        .or(words.first().copied());
                    let at = words.iter().position(|word| Some(*word) == now);
                    let next = words[at.map_or(0, |at| (at + 1) % words.len())];
                    self.change_bot(|config| {
                        config.set_chat_value(platform.key, field.key, Some(next));
                    });
                }
                FieldKind::Secret { .. } | FieldKind::Text => {
                    // A secret is asked for from nothing: what is kept is
                    // not shown, so there is nothing to start from. Text is
                    // shown, and edited where it stands.
                    let text = match field.kind {
                        FieldKind::Text => self
                            .platform()
                            .and_then(|platform| self.config().chat_value(platform.key, field.key))
                            .unwrap_or_default()
                            .to_string(),
                        _ => String::new(),
                    };
                    self.ask_on_the_status_row(Prompt::about(PromptKind::Told(field), text));
                }
            },
            Reaching::Forget(field) => self.tell_the_bot(field, None),
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
                        if let Err(error) = self.desk().open(url) {
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
    pub(in crate::app) fn tell_the_bot(&mut self, field: &'static Field, said: Option<&str>) {
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
                        Some(said) => obelus_bot::secrets::write(platform.key, field.key, said)
                            .map(|()| Some(ends(said, looks_like))),
                        None => obelus_bot::secrets::forget(platform.key, field.key).map(|()| None),
                    };
                    let _ = sender.send(Event::Bot(obelus_bot::Event::Written {
                        platform: platform.key,
                        field: field.key,
                        written,
                    }));
                });
            }
            FieldKind::Choice(_) | FieldKind::Text => {
                self.change_bot(|config| config.set_chat_value(platform.key, field.key, said));
            }
        }
    }

    /// Changes what the reader's settings keep about a chat, and writes it.
    ///
    /// The same three steps every change to the reader's file takes:
    /// theirs, then the project's back over it -- which here takes nothing
    /// away, because no project may set any of this -- then the file.
    pub(in crate::app) fn change_bot(&mut self, change: impl FnOnce(&mut obelus_config::Config)) {
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
        let now = self.config().chat.clone().unwrap_or_default();
        self.open_choices("chat", &[], &now);
    }

    /// The list of people who may talk to this machine.
    fn open_the_people(&mut self) {
        // Pairing is how somebody is let in; this is how somebody is let
        // go, and it is the next piece of this page.
        tracing::debug!("the list of people is not drawn yet");
    }

    /// Makes a code for somebody to pair with, taking the chat into this
    /// window first where it is not here.
    ///
    /// Asked whether the chat could be reached rather than whether it is:
    /// pairing is the first thing a reader does with a chat, so offering it
    /// only once connected was a row that did nothing until they found
    /// `connect-to-chat` -- and then one to press again.
    ///
    /// Good for five minutes, and then it is gone: a code is the one thing a
    /// stranger may send, so it is something that exists only while the
    /// reader is waiting for it. Run out by a clock of its own rather than a
    /// countdown on the row -- nothing on screen moves for it.
    fn pair(&mut self) {
        // Silent where it could not be sent: the row is drawn dim there,
        // and a key does nothing where its row is dim.
        let state = self.bot_state();
        if !state.may_connect() {
            return;
        }
        if !state.connected() || !self.holds_the_bot() {
            self.bot.pair_once_connected = true;
            self.connect_bot();
            return;
        }
        self.bot.pairing = Some(a_code());
        self.bot.pairing_runs_out = self.come_back_in(
            std::time::Duration::from_secs(5 * 60),
            Event::Bot(obelus_bot::Event::PairingOver),
        );
    }
}

/// A code to pair with: six digits, because it is typed on a phone, and
/// a phone's keyboard for digits is one press away.
///
/// Six and not four: nothing counts the wrong ones, and any group the bot
/// is in may try while the code lasts, so ten thousand is a list a script
/// gets through in five minutes and a million is not.
///
/// From the hasher std seeds with randomness, the way a window's key is: a
/// code lasts five minutes and is tried by typing it, so what it has to be
/// is unguessable by somebody who has not seen it, and that is plenty.
fn a_code() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};

    let mut hasher = std::hash::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    format!("{:06}", hasher.finish() % 1_000_000)
}

/// Whether what somebody sent is the code, however they typed it: split
/// in two, with a space either side.
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

impl obelus_sink::Sink<obelus_bot::Event> for Numbered {
    fn send(&self, event: obelus_bot::Event) -> Result<(), obelus_sink::Gone> {
        self.sender
            .send(Event::BotReached(self.number, event))
            .map_err(|_| obelus_sink::Gone)
    }
}

/// What the chat's directory in the state directory is called.
///
/// Still `remote`, the word the chat had before it went to working on
/// another machine: the directory holds which conversation each thread is,
/// the room and the lock that says which window talks to the chat, and
/// renaming it would lose the first two and -- while an Obelus from before
/// the rename runs beside one from after -- let both hold the third.
pub(in crate::app) const KEPT_UNDER: &str = "remote";

/// Where the one window the chat talks to holds it, and where another asks
/// for it. One of each for the machine rather than the project or the
/// platform: a bot has one connection, whichever chat it is.
pub(in crate::app) fn bot_directory(_: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(obelus_logging::state_directory()?.join(KEPT_UNDER))
}

/// The file whose lock is the connection. Never read: on Windows a lock
/// keeps others from reading even the byte it is on.
fn the_lock() -> Option<std::fs::File> {
    let path = bot_directory(std::path::Path::new(""))?.join("talking.lock");
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .ok()
}

/// What a window writes in place of its asking when the reader takes it back.
const WITHDRAWN: &str = "withdrawn";

/// The file a window writes its number into to ask for the connection.
fn the_wanting() -> Option<std::path::PathBuf> {
    Some(bot_directory(std::path::Path::new(""))?.join("wanted"))
}

impl App {
    /// Whether this is the window the chat talks to.
    pub(in crate::app) fn holds_the_bot(&self) -> bool {
        self.bot.holding.is_some()
    }

    /// The same, for a test with two windows.
    #[must_use]
    pub fn holds_the_bot_for_test(&self) -> bool {
        self.holds_the_bot()
    }

    /// How many threads this window has started waiting in the kernel for
    /// the chat, for a test that asks again after giving up.
    #[must_use]
    pub fn waiters_for_test(&self) -> u64 {
        self.bot.waiters
    }

    /// The chat and where it stands, for the status row of the window it
    /// talks to -- or is on its way to, which is connecting -- and of every
    /// window heard in it through that one, and nothing for any other.
    pub(in crate::app) fn bot_badge(&self) -> Option<(&'static str, State)> {
        let platform = self.platform()?;
        match (&self.bot.holding, &self.bot.taking) {
            (Some(_), _) => Some((platform.name, self.bot_state())),
            (None, Some(_)) => Some((platform.name, State::Connecting)),
            // Heard through the window that has it: marked, so the reader
            // knows this window's conversations are in the chat too.
            (None, None) if self.joined_the_relay() => Some((platform.name, self.bot_state())),
            (None, None) => None,
        }
    }

    /// Whether the chat's mark is turning.
    pub(in crate::app) fn bot_turning(&self) -> bool {
        self.bot_badge()
            .is_some_and(|(_, state)| state == State::Connecting)
    }

    /// Whether this window is the one the chat talks to, or is on its way
    /// to being.
    pub(in crate::app) fn has_the_bot(&self) -> bool {
        self.bot.holding.is_some() || self.bot.taking.is_some()
    }

    /// Makes this the window the chat talks to: at once where no other
    /// window has it, and where one has, by asking it to let go and waiting
    /// for the kernel to say it has.
    pub(in crate::app) fn connect_bot(&mut self) {
        if self.has_the_bot() {
            return;
        }
        let Some(platform) = self.platform() else {
            return;
        };
        let Some(lock) = the_lock() else {
            self.wrong(format!("Nowhere to hold {} from", platform.name));
            return;
        };
        if !obelus_claim::held_by_somebody_else(&lock) {
            self.take_the_bot(lock);
            return;
        }
        // The other window hears this through its watch on the directory
        // and lets go; the lock comes back here when it has, or when its
        // process ends, which lets go of it too.
        let number = match self.bot.waiter {
            Some(waiting) => waiting,
            None => {
                self.bot.asked += 1;
                self.bot.asked
            }
        };
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
        self.bot.wrote = Some(asking);
        if self.bot.waiter.is_none()
            && let Some(events) = self.events.clone()
        {
            self.bot.waiter = Some(number);
            self.bot.waiters += 1;
            obelus_runtime::handle().spawn_blocking(move || {
                // Said either way: a thread that ended in silence would be
                // waited on for ever, by every asking after it.
                let _ = match obelus_claim::wait_to_hold(&lock) {
                    true => events.send(Event::Held(number, lock)),
                    false => events.send(Event::NotHeld(number)),
                };
            });
        }
        // Given up on after a while, because the asking travels by a watch,
        // and a watch is freshness rather than a promise: a window that
        // never heard would leave this one turning for ever.
        self.bot.clocks += 1;
        let runs_out = self.come_back_in(
            std::time::Duration::from_secs(10),
            Event::NotLetGo(self.bot.clocks),
        );
        self.bot.taking = Some((number, runs_out));
    }

    /// The lock, come back from the kernel.
    pub(in crate::app) fn held_the_bot(&mut self, number: u64, lock: std::fs::File) {
        if self.bot.waiter == Some(number) {
            self.bot.waiter = None;
        }
        let asking = self.bot.taking.as_ref().map(|(asked, _)| *asked) == Some(number);
        // Or the asking that timed out, answered late: the other window let
        // go, so this one has it -- where nothing has happened since that
        // would make having it wrong.
        let late = self.bot.given_up == Some(number)
            && self.bot.taking.is_none()
            && self.bot.holding.is_none()
            && self.platform().is_some();
        // Or a chat nobody let go of, taken up -- where this window has not
        // joined whoever took it up first.
        let succeeding = self.bot.succeeding == Some(number)
            && self.bot.taking.is_none()
            && self.bot.holding.is_none()
            && !self.joined_the_relay()
            && self.platform().is_some();
        if !asking && !late && !succeeding {
            // An asking taken back, or one since replaced: dropped here,
            // which lets go again.
            return;
        }
        self.take_the_bot(lock);
    }

    /// The thread waiting for the lock could not wait at all: there is no
    /// waiting going on, so the next asking starts it again, and one still
    /// going is over -- said, because nothing else will come for it.
    pub(in crate::app) fn not_held(&mut self, number: u64) {
        self.bot.pair_once_connected = false;
        if self.bot.waiter == Some(number) {
            self.bot.waiter = None;
        }
        if self.bot.taking.as_ref().map(|(asked, _)| *asked) == Some(number) {
            self.bot.taking = None;
            if let Some(platform) = self.platform() {
                self.wrong(format!("Could not wait for {} to come here", platform.name));
            }
        }
    }

    /// The other window never let go.
    pub(in crate::app) fn not_let_go(&mut self, clock: u64) {
        if clock != self.bot.clocks {
            return;
        }
        let Some((number, _)) = self.bot.taking.take() else {
            return;
        };
        self.bot.pair_once_connected = false;
        self.bot.given_up = Some(number);
        if let Some(platform) = self.platform() {
            self.wrong(format!("Another window would not let {} go", platform.name));
        }
    }

    fn take_the_bot(&mut self, lock: std::fs::File) {
        self.bot.taking = None;
        self.bot.given_up = None;
        self.bot.succeeding = None;
        self.bot.holding = Some(lock);
        // Before the next frame would: what this window says to the chat
        // from now on goes to the platform, and nothing more to the relay.
        self.settle_the_relay();
        self.start_relaying();
        // Nothing said: the chat's mark on the status row says it, and a
        // note would have the row and cover that mark until the next key.
    }

    /// Stops this window talking to the chat, which leaves it to no window
    /// until one asks.
    pub(in crate::app) fn disconnect_bot(&mut self) {
        self.bot.pair_once_connected = false;
        // An asking, taken back: the request withdrawn, so that the window
        // that has the chat keeps it when it hears.
        if self.bot.taking.take().is_some() {
            if let Some(path) = the_wanting() {
                let _ = std::fs::write(path, WITHDRAWN);
            }
            self.bot.wrote = None;
            // And whatever it was asked for before, given up on and still
            // waiting: taken back with it, or a holder that lets go later
            // would hand over a chat the reader said they did not want.
            self.bot.given_up = None;
            self.bot.succeeding = None;
            return;
        }
        // Nor taken up by a lock that comes back later: the reader has
        // said they do not want it here.
        self.bot.succeeding = None;
        if self.bot.holding.is_none() {
            return;
        }
        self.stop_relaying(obelus_bot::relay::Over::Stopped);
        self.bot.holding = None;
        self.let_the_chat_go();
        // Nothing said: the chat's mark on the status row says it, and a
        // note would have the row and cover that mark until the next key.
    }

    /// Whether a path that changed is another window asking for the chat.
    pub(in crate::app) fn is_the_bot_wanted(&self, path: &std::path::Path) -> bool {
        the_wanting().is_some_and(|wanting| wanting == path)
    }

    /// Another window has asked for the chat: let go, so that it can have
    /// it. Not this window's own asking, which it hears too.
    pub(in crate::app) fn somebody_wants_the_bot(&mut self) {
        if self.bot.holding.is_none() {
            return;
        }
        let asker = the_wanting().and_then(|path| std::fs::read_to_string(path).ok());
        if (asker.is_some() && asker == self.bot.wrote) || asker.as_deref() == Some(WITHDRAWN) {
            return;
        }
        // Only once it is going: this window hears its own asking too, and
        // a code it is waiting to make is the reason it asked.
        self.bot.pair_once_connected = false;
        self.stop_relaying(obelus_bot::relay::Over::Handed);
        self.bot.holding = None;
        self.let_the_chat_go();
        if let Some(platform) = self.platform() {
            self.say(format!("{} went to another window", platform.name));
        }
    }

    /// The window this one was heard through went without letting the chat
    /// go: taken up here, at once where nobody has it, and otherwise by
    /// waiting in the kernel -- another window left behind may have it
    /// first, and this one joins that one instead.
    ///
    /// Never by asking: the window that went is not there to hear it, and
    /// the window that took it up first would hand it over.
    pub(in crate::app) fn take_up_the_chat(&mut self) {
        if self.platform().is_none() || self.has_the_bot() {
            return;
        }
        let Some(lock) = the_lock() else {
            return;
        };
        if !obelus_claim::held_by_somebody_else(&lock) {
            self.take_the_bot(lock);
            return;
        }
        // On the thread already waiting, where there is one: it comes back
        // with the lock under its own number.
        if let Some(waiting) = self.bot.waiter {
            self.bot.succeeding = Some(waiting);
            return;
        }
        let Some(events) = self.events.clone() else {
            return;
        };
        self.bot.asked += 1;
        let number = self.bot.asked;
        self.bot.waiter = Some(number);
        self.bot.waiters += 1;
        self.bot.succeeding = Some(number);
        obelus_runtime::handle().spawn_blocking(move || {
            let _ = match obelus_claim::wait_to_hold(&lock) {
                true => events.send(Event::Held(number, lock)),
                false => events.send(Event::NotHeld(number)),
            };
        });
    }

    /// Somebody else took the chat up first, and this window is heard
    /// through them: a lock that comes back later is let go again.
    pub(in crate::app) fn no_longer_taking_up_the_chat(&mut self) {
        self.bot.succeeding = None;
    }
}
