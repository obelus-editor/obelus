//! Obelus as the owner of the X11 clipboard selection.
//!
//! On a connection of its own, which is where this half differs from the
//! Wayland one. Wayland asks for the selection with a *serial* -- a number
//! the compositor gave the seat when the reader last touched something --
//! so it has to be asked for on the connection that was given the number.
//! X11 asks with a timestamp and an ordinary window, and any client's
//! window will do: a connection of this module's own, with one unmapped
//! one-pixel window on it, is what arboard does and what every other
//! clipboard on this display is.
//!
//! Which is not a preference. Sharing winit's would mean taking its `xcb`
//! connection out of an Xlib `Display`, and the events would then arrive in
//! winit's queue to be handed back: the connection is shared, so the
//! *events* are, and a selection request the window loop dropped is a paste
//! that never answers.
//!
//! A selection is a conversation. Taking it writes nothing anywhere: the
//! owner is a window, and everything about what is on the clipboard is
//! asked of that window afterwards, one `SelectionRequest` at a time, for
//! as long as Obelus is running. So there is a thread doing nothing else.

use std::sync::{Arc, Mutex};

use obelus_clipboard::Owner;
use x11rb::{
    connection::{Connection as _, RequestConnection as _},
    errors::ReplyError,
    protocol::{
        Event,
        xproto::{
            Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
            SelectionNotifyEvent, Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

/// One copy's shapes, by the atom each goes by on this server.
///
/// By atom rather than by name because that is what a request names. The
/// name is kept beside it for the two questions Obelus asks of its own
/// clipboard, which are asked in mime types like everything else.
type Offering = Arc<Vec<(Atom, String, Vec<u8>)>>;

/// The names this module has to have interned before it can say anything.
struct Atoms {
    /// The clipboard proper, rather than the primary selection -- which is
    /// the middle button's, and is not what a copy means.
    clipboard: Atom,
    /// What a requestor asks for when it wants the list of shapes.
    targets: Atom,
}

/// What the copying thread and the answering thread share.
#[derive(Default)]
struct Held {
    /// What Obelus is offering, or nothing where somebody else owns the
    /// selection.
    mine: Option<Offering>,
    /// The freshest timestamp the server has been seen to use.
    ///
    /// `CurrentTime` is allowed here and is a race: a request that arrives
    /// with it is ordered against everything else by when the server
    /// happens to process it, so two clients taking the selection at once
    /// can both believe they have it. A real timestamp is one the server
    /// gave out, and the only way to be given one without the reader doing
    /// anything is to change a property of one's own window and read the
    /// time off the event that comes back.
    when: u32,
}

/// Obelus's hold on the selection.
pub(crate) struct Clipboard {
    held: Arc<Mutex<Held>>,
    connection: Arc<RustConnection>,
    window: Window,
    atoms: Atoms,
}

impl Clipboard {
    /// Opens a connection, makes a window to own the selection with, and
    /// starts answering on it.
    pub(crate) fn take() -> Option<Self> {
        let (connection, screen) = match x11rb::connect(None) {
            Ok(open) => open,
            Err(error) => {
                tracing::warn!(%error, "no X connection to own the clipboard on");
                return None;
            }
        };
        let connection = Arc::new(connection);
        let held = Arc::new(Mutex::new(Held::default()));
        let made = make_a_window(&connection, screen).and_then(|window| {
            let atoms = Atoms {
                clipboard: interned(&connection, "CLIPBOARD")?,
                targets: interned(&connection, "TARGETS")?,
            };
            Some((window, atoms))
        });
        let (window, atoms) = made?;

        {
            let Ok(mut inside) = held.lock() else {
                return None;
            };
            inside.when = a_timestamp(&connection, window)?;
        }

        let answering = Answering {
            connection: Arc::clone(&connection),
            held: Arc::clone(&held),
            window,
            clipboard: atoms.clipboard,
            targets: atoms.targets,
        };
        let name = "obelus clipboard".to_string();
        let started = std::thread::Builder::new()
            .name(name)
            .spawn(move || answering.run());
        if let Err(error) = started {
            tracing::warn!(%error, "no thread to answer selection requests on");
            return None;
        }
        Some(Self {
            held,
            connection,
            window,
            atoms,
        })
    }
}

impl Owner for Clipboard {
    fn offer(&self, shapes: Vec<(String, Vec<u8>)>) -> bool {
        // Every name at once, because each is a round trip and three round
        // trips in a row is three times the wait for a key the reader
        // pressed.
        let asked: Vec<_> = shapes
            .iter()
            .filter_map(|(name, _)| {
                self.connection
                    .intern_atom(false, name.as_bytes())
                    .ok()
                    .map(|cookie| (cookie, name, ()))
            })
            .collect();
        let offering: Offering = Arc::new(
            asked
                .into_iter()
                .zip(shapes.iter().map(|(_, bytes)| bytes))
                .filter_map(|((cookie, name, ()), bytes)| {
                    let atom = cookie.reply().ok()?.atom;
                    Some((atom, name.clone(), bytes.clone()))
                })
                .collect(),
        );
        if offering.len() != shapes.len() {
            tracing::warn!("the X server would not name every shape of a copy");
            return false;
        }

        let Ok(mut held) = self.held.lock() else {
            return false;
        };
        let taken = self
            .connection
            .set_selection_owner(self.window, self.atoms.clipboard, held.when)
            .map_err(ReplyError::from)
            .and_then(x11rb::cookie::VoidCookie::check);
        if taken.is_err() {
            tracing::warn!("the X server refused the clipboard");
            return false;
        }
        held.mine = Some(offering);
        drop(held);
        // The server tells nobody who owns a selection; whoever wants to
        // know asks. So the only thing that can be checked is that it
        // agrees the window is the owner, which it will not if the
        // timestamp was stale.
        match self
            .connection
            .get_selection_owner(self.atoms.clipboard)
            .map_err(ReplyError::from)
            .and_then(x11rb::cookie::Cookie::reply)
        {
            Ok(owner) if owner.owner == self.window => true,
            Ok(owner) => {
                tracing::warn!(owner = owner.owner, "somebody else has the clipboard");
                false
            }
            Err(error) => {
                tracing::warn!(%error, "asking who has the clipboard");
                false
            }
        }
    }

    fn holding(&self, mime: &str) -> Option<Vec<u8>> {
        let held = self.held.lock().ok()?;
        let mine = held.mine.as_ref()?;
        mine.iter()
            .find(|(_, name, _)| name == mime)
            .map(|(_, _, bytes)| bytes.clone())
    }

    fn holds(&self) -> Vec<String> {
        let Ok(held) = self.held.lock() else {
            return Vec::new();
        };
        held.mine
            .iter()
            .flat_map(|mine| mine.iter().map(|(_, name, _)| name.clone()))
            .collect()
    }
}

/// The window the selection is owned by.
///
/// One pixel and never mapped: nothing is drawn on it and nobody sees it.
/// A selection's owner is a window because that is what a request is sent
/// to, and this one exists to be that address.
fn make_a_window(connection: &RustConnection, screen: usize) -> Option<Window> {
    let root = connection.setup().roots.get(screen)?.root;
    let window = connection.generate_id().ok()?;
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            // For the one property change below, which is how a timestamp
            // is come by.
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .ok()?
        .check()
        .ok()?;
    Some(window)
}

/// An atom, by name.
fn interned(connection: &RustConnection, name: &str) -> Option<Atom> {
    match connection
        .intern_atom(false, name.as_bytes())
        .map_err(ReplyError::from)
        .and_then(x11rb::cookie::Cookie::reply)
    {
        Ok(reply) => Some(reply.atom),
        Err(error) => {
            tracing::warn!(name, %error, "the X server would not name it");
            None
        }
    }
}

/// A timestamp the server itself gave out.
///
/// By appending nothing to a property of our own window, which changes the
/// property and so produces a `PropertyNotify` with the server's own idea
/// of the time on it. The alternative is `CurrentTime`, which is a race;
/// see [`Held::when`].
fn a_timestamp(connection: &RustConnection, window: Window) -> Option<u32> {
    connection
        .change_property8(
            PropMode::APPEND,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            &[],
        )
        .ok()?;
    connection.flush().ok()?;
    loop {
        match connection.wait_for_event() {
            Ok(Event::PropertyNotify(notified)) if notified.window == window => {
                return Some(notified.time);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "waiting for the server to say what time it is");
                return None;
            }
        }
    }
}

/// The thread that answers for the selection.
struct Answering {
    connection: Arc<RustConnection>,
    held: Arc<Mutex<Held>>,
    window: Window,
    clipboard: Atom,
    targets: Atom,
}

impl Answering {
    /// Until the connection goes, which is the process ending.
    fn run(self) {
        loop {
            let event = match self.connection.wait_for_event() {
                Ok(event) => event,
                Err(error) => {
                    tracing::debug!(%error, "the X connection ended");
                    return;
                }
            };
            match event {
                Event::SelectionRequest(asked) => self.answer(asked.into()),
                // Somebody else took it, which is theirs to do. What
                // Obelus was holding is not the answer any more.
                Event::SelectionClear(cleared) if cleared.selection == self.clipboard => {
                    if let Ok(mut held) = self.held.lock() {
                        held.mine = None;
                    }
                }
                Event::PropertyNotify(notified) if notified.window == self.window => {
                    if let Ok(mut held) = self.held.lock() {
                        held.when = notified.time;
                    }
                }
                _ => {}
            }
        }
    }

    /// One request: put the bytes on the requestor's window, then tell it
    /// they are there.
    ///
    /// The property is set *before* the notification, because the
    /// notification is what says to go and read it.
    fn answer(&self, asked: Asked) {
        // A requestor old enough to send no property means "put it where I
        // asked for it", which is the obsolete convention and costs one
        // line to keep.
        let property = if asked.property == x11rb::NONE {
            asked.target
        } else {
            asked.property
        };
        let answered = self.put(&asked, property);
        let notice = SelectionNotifyEvent {
            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: asked.time,
            requestor: asked.requestor,
            selection: asked.selection,
            target: asked.target,
            // No property is how a refusal is said: the shape was not one
            // of the ones offered.
            property: if answered { property } else { x11rb::NONE },
        };
        let sent = self
            .connection
            .send_event(false, asked.requestor, EventMask::NO_EVENT, notice)
            .map(|_| self.connection.flush());
        if let Err(error) = sent {
            tracing::debug!(%error, "answering a paste");
        }
    }

    /// Writes the answer on the requestor's window, or says there is none.
    fn put(&self, asked: &Asked, property: Atom) -> bool {
        let Ok(held) = self.held.lock() else {
            return false;
        };
        let Some(mine) = held.mine.clone() else {
            return false;
        };
        drop(held);

        if asked.target == self.targets {
            // `TARGETS` is itself one of the answers: a requestor that
            // asked what is on offer can ask that again.
            let mut list: Vec<Atom> = vec![self.targets];
            list.extend(mine.iter().map(|(atom, _, _)| *atom));
            return self
                .connection
                .change_property32(
                    PropMode::REPLACE,
                    asked.requestor,
                    property,
                    AtomEnum::ATOM,
                    &list,
                )
                .map_err(ReplyError::from)
                .and_then(x11rb::cookie::VoidCookie::check)
                .is_ok();
        }

        let Some((_, _, bytes)) = mine.iter().find(|(atom, _, _)| *atom == asked.target) else {
            return false;
        };
        // One request, so one property's worth. A shape larger than the
        // server will take in a single request is what `INCR` is for --
        // handing it over a piece at a time -- and Obelus does not do that
        // yet: what it offers is the words, and a copy that big is not one
        // a reader made by selecting text.
        if bytes.len() > self.connection.maximum_request_bytes() {
            tracing::warn!(
                bytes = bytes.len(),
                "too much to hand over in one piece, and INCR is not done"
            );
            return false;
        }
        self.connection
            .change_property8(
                PropMode::REPLACE,
                asked.requestor,
                property,
                asked.target,
                bytes,
            )
            .map_err(ReplyError::from)
            .and_then(x11rb::cookie::VoidCookie::check)
            .is_ok()
    }
}

/// What a `SelectionRequest` says, which is all [`Answering::answer`]
/// needs of it.
struct Asked {
    time: u32,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
}

impl From<x11rb::protocol::xproto::SelectionRequestEvent> for Asked {
    fn from(asked: x11rb::protocol::xproto::SelectionRequestEvent) -> Self {
        Self {
            time: asked.time,
            requestor: asked.requestor,
            selection: asked.selection,
            target: asked.target,
            property: asked.property,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use x11rb::protocol::xproto::GetPropertyReply;

    use super::*;

    /// A requestor: the other half of every conversation this module has.
    ///
    /// Its own connection, because that is what a paste is -- some other
    /// program asking this one for the bytes.
    struct Asking {
        connection: RustConnection,
        window: Window,
        clipboard: Atom,
        slot: Atom,
    }

    impl Asking {
        fn new() -> Self {
            let (connection, screen) = x11rb::connect(None).expect("an X server to ask");
            let window = make_a_window(&connection, screen).expect("a window to ask from");
            let clipboard = interned(&connection, "CLIPBOARD").expect("CLIPBOARD");
            let slot = interned(&connection, "OBELUS_TEST_SLOT").expect("somewhere to be answered");
            Self {
                connection,
                window,
                clipboard,
                slot,
            }
        }

        /// Asks for one shape, and waits for the answer to be put on the
        /// window.
        fn ask(&self, target: Atom) -> Option<GetPropertyReply> {
            self.connection
                .convert_selection(
                    self.window,
                    self.clipboard,
                    target,
                    self.slot,
                    x11rb::CURRENT_TIME,
                )
                .expect("asking for a shape");
            self.connection.flush().expect("asking for a shape");

            // Polled rather than blocked on, so that a broken answer is a
            // test that fails rather than one that hangs.
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                match self.connection.poll_for_event().expect("waiting") {
                    Some(Event::SelectionNotify(answered)) => {
                        // No property is the refusal.
                        if answered.property == x11rb::NONE {
                            return None;
                        }
                        return Some(
                            self.connection
                                .get_property(
                                    true,
                                    self.window,
                                    self.slot,
                                    AtomEnum::ANY,
                                    0,
                                    u32::MAX / 4,
                                )
                                .expect("reading the answer")
                                .reply()
                                .expect("reading the answer"),
                        );
                    }
                    Some(_) => {}
                    None => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            panic!("the owner never answered");
        }
    }

    /// Obelus owns the selection, lists what it is offering, and hands over
    /// each shape when it is asked for.
    ///
    /// Against a real X server, because there is no other kind: a selection
    /// is a conversation between two clients through the server, and the
    /// half this module is only means anything with the other half opposite
    /// it. `#[ignore]`d because it needs a display, and because it takes
    /// the running reader's clipboard for the length of the run.
    ///
    /// Deliberate breaks, each failing its own assertion: `offer` taking
    /// the selection without writing down what it is offering; `TARGETS`
    /// answered without itself in the list; `answer` sending the requested
    /// property on a shape that was not offered rather than `NONE`.
    ///
    /// What this does *not* check is the one ordering that matters here --
    /// the property written before the notification that says to go and
    /// read it. Both go down one connection in one buffer, and the
    /// requestor below polls, so the property lands first however it is
    /// written. A test that caught it would be one that passed or failed
    /// by how fast the machine was.
    #[test]
    #[ignore = "needs an X server, and takes the clipboard while it runs"]
    fn the_owner_answers_for_every_shape_it_offered() {
        let clipboard = Clipboard::take().expect("taking the X clipboard");
        assert!(
            clipboard.offer(vec![
                ("UTF8_STRING".to_string(), b"a note".to_vec()),
                ("application/x-obelus-test".to_string(), vec![1, 2, 3]),
            ]),
            "the server would not give Obelus the clipboard"
        );

        let asking = Asking::new();
        let targets = interned(&asking.connection, "TARGETS").expect("TARGETS");
        let words = interned(&asking.connection, "UTF8_STRING").expect("UTF8_STRING");
        let ours =
            interned(&asking.connection, "application/x-obelus-test").expect("our own shape");
        let never = interned(&asking.connection, "application/x-obelus-never").expect("a shape");

        let listed: Vec<Atom> = asking
            .ask(targets)
            .expect("a list of shapes")
            .value32()
            .expect("atoms")
            .collect();
        assert!(listed.contains(&words), "the words were not offered");
        assert!(listed.contains(&ours), "Obelus's own shape was not offered");
        // A requestor that asked what is on offer can ask that again.
        assert!(listed.contains(&targets), "TARGETS was not in TARGETS");

        assert_eq!(asking.ask(words).expect("the words").value, b"a note");
        assert_eq!(
            asking.ask(ours).expect("our own shape").value,
            vec![1, 2, 3]
        );
        // A shape that was never offered is refused, rather than answered
        // with nothing -- which a requestor reads as an empty clipboard.
        assert!(asking.ask(never).is_none(), "a shape nobody offered");
    }
}
