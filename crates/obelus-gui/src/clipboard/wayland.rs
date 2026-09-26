//! Obelus as the owner of the Wayland selection.
//!
//! On a second event queue of winit's own connection, rather than a
//! connection of Obelus's own. Taking the selection has to be asked with a
//! *serial* -- a number the compositor gave the seat when the reader last
//! touched something -- and serials are a connection's: a second connection
//! sees none of them and every `set_selection` from it is ignored. So the
//! window's `wl_display` is adopted, which is what
//! [`Backend::from_foreign_display`] is for, and a queue is made on it that
//! nothing else dispatches.
//!
//! Two threads, because there are two directions. Requests go out from
//! whichever thread is copying -- a proxy may be used from any thread, and
//! sending one is all a copy does -- while the events come back on a thread
//! of this module's own, since the compositor asks for the bytes long after
//! the copy: a `send` arrives when somebody else pastes, which may be
//! tomorrow.

use std::{
    io::Write,
    os::fd::OwnedFd,
    ptr::NonNull,
    sync::{Arc, Mutex},
};

use obelus_clipboard::Owner;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::Backend,
    event_created_child,
    protocol::{
        wl_data_device::{self, WlDataDevice},
        wl_data_device_manager::WlDataDeviceManager,
        wl_data_offer::WlDataOffer,
        wl_data_source::{self, WlDataSource},
        wl_keyboard::{self, WlKeyboard},
        wl_registry::{self, WlRegistry},
        wl_seat::{self, WlSeat},
    },
};

/// One copy's shapes, shared with the source that is offering them.
///
/// The source's own user data rather than something to look up: a second
/// copy replaces the first, and a `send` that arrived for the older source
/// would otherwise be answered with the newer one's bytes.
type Offering = Arc<Vec<(String, Vec<u8>)>>;

/// What the copying thread and the listening thread share.
#[derive(Default)]
struct Held {
    /// What Obelus is offering, or nothing where somebody else owns the
    /// selection.
    mine: Option<Offering>,
    manager: Option<WlDataDeviceManager>,
    device: Option<WlDataDevice>,
    /// The last serial the seat sent.
    ///
    /// Zero until the reader has touched the window, which is a copy this
    /// module has to refuse: the compositor ignores a `set_selection` asked
    /// with a serial it did not give out, silently, so the copy would
    /// simply not happen.
    serial: u32,
}

/// The state the listening thread dispatches into.
struct Watching {
    held: Arc<Mutex<Held>>,
    seat: Option<WlSeat>,
}

/// Obelus's hold on the selection.
pub(crate) struct Clipboard {
    held: Arc<Mutex<Held>>,
    connection: Connection,
    queue: QueueHandle<Watching>,
}

impl Clipboard {
    /// Adopts `display` -- winit's -- and starts listening on it.
    ///
    /// # Safety
    ///
    /// `display` must be a live `wl_display` that outlives this process's
    /// use of the clipboard, which winit's is: the event loop holds it for
    /// as long as there is a window.
    pub(crate) unsafe fn take(display: NonNull<std::ffi::c_void>) -> Option<Self> {
        // Adopted, not owned: dropping this backend must not disconnect the
        // display winit is still drawing on.
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        let held = Arc::new(Mutex::new(Held::default()));
        let mut watching = Watching {
            held: Arc::clone(&held),
            seat: None,
        };

        let _registry = connection.display().get_registry(&handle, ());
        // The globals arrive in whatever order the compositor lists them,
        // so the device is made after they are all in rather than from
        // inside the handler for either half of it.
        if let Err(error) = queue.roundtrip(&mut watching) {
            tracing::warn!(%error, "asking the compositor what it has");
            return None;
        }
        let seat = watching.seat.clone()?;
        {
            let mut inside = held.lock().ok()?;
            let manager = inside.manager.clone()?;
            inside.device = Some(manager.get_data_device(&seat, &handle, ()));
        }

        let name = "obelus clipboard".to_string();
        let started = std::thread::Builder::new().name(name).spawn(move || {
            // Until the connection goes, which is the process ending.
            while queue.blocking_dispatch(&mut watching).is_ok() {}
        });
        if let Err(error) = started {
            tracing::warn!(%error, "no thread to listen for pastes on");
            return None;
        }
        Some(Self {
            held,
            connection,
            queue: handle,
        })
    }
}

impl Owner for Clipboard {
    fn offer(&self, shapes: Vec<(String, Vec<u8>)>) -> bool {
        let Ok(mut held) = self.held.lock() else {
            return false;
        };
        let (Some(manager), Some(device)) = (held.manager.clone(), held.device.clone()) else {
            return false;
        };
        if held.serial == 0 {
            tracing::debug!("nothing has been pressed, so there is no serial to copy with");
            return false;
        }
        let offering: Offering = Arc::new(shapes);
        let source = manager.create_data_source(&self.queue, Arc::clone(&offering));
        for (name, _) in offering.iter() {
            source.offer(name.clone());
        }
        device.set_selection(Some(&source), held.serial);
        held.mine = Some(offering);
        drop(held);
        // Nothing else dispatches this queue, so nothing else would send
        // the requests above.
        if let Err(error) = self.connection.flush() {
            tracing::warn!(%error, "taking the clipboard");
            return false;
        }
        true
    }

    fn holding(&self, mime: &str) -> Option<Vec<u8>> {
        let held = self.held.lock().ok()?;
        let mine = held.mine.as_ref()?;
        mine.iter()
            .find(|(name, _)| name == mime)
            .map(|(_, bytes)| bytes.clone())
    }

    fn holds(&self) -> Vec<String> {
        let Ok(held) = self.held.lock() else {
            return Vec::new();
        };
        held.mine
            .iter()
            .flat_map(|mine| mine.iter().map(|(name, _)| name.clone()))
            .collect()
    }
}

impl Dispatch<WlRegistry, ()> for Watching {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _connection: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            // Five is where a seat gained a name, and three is where a
            // source gained the drag-and-drop actions. Neither is wanted
            // here; the minimum is what matters, and asking for more than
            // the compositor has is a protocol error rather than an older
            // object.
            "wl_seat" => state.seat = Some(registry.bind(name, version.min(5), queue, ())),
            "wl_data_device_manager" => {
                if let Ok(mut held) = state.held.lock() {
                    held.manager = Some(registry.bind(name, version.min(3), queue, ()));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for Watching {
    fn event(
        _state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        (): &(),
        _connection: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        // The keyboard is bound for its serials and nothing else: every
        // event it sends carries the number that a copy has to be asked
        // with, and there is no other way to be told one. The presses
        // themselves are winit's, which has a keyboard of its own on the
        // same seat.
        if let wl_seat::Event::Capabilities {
            capabilities: wayland_client::WEnum::Value(has),
        } = event
            && has.contains(wl_seat::Capability::Keyboard)
        {
            seat.get_keyboard(queue, ());
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Watching {
    fn event(
        state: &mut Self,
        _keyboard: &WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        let serial = match event {
            wl_keyboard::Event::Enter { serial, .. }
            | wl_keyboard::Event::Leave { serial, .. }
            | wl_keyboard::Event::Key { serial, .. }
            | wl_keyboard::Event::Modifiers { serial, .. } => serial,
            _ => return,
        };
        if let Ok(mut held) = state.held.lock() {
            held.serial = serial;
        }
    }
}

impl Dispatch<WlDataDeviceManager, ()> for Watching {
    fn event(
        _state: &mut Self,
        _manager: &WlDataDeviceManager,
        _event: <WlDataDeviceManager as Proxy>::Event,
        (): &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDevice, ()> for Watching {
    fn event(
        _state: &mut Self,
        _device: &WlDataDevice,
        event: wl_data_device::Event,
        (): &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        // What somebody else is offering is not read here -- that is what
        // the programs are for -- but the compositor makes an object for
        // every selection anybody sets, and one that is never destroyed is
        // one that stays in the connection's table for the session.
        if let wl_data_device::Event::Selection { id: Some(offer) } = event {
            offer.destroy();
        }
    }

    event_created_child!(Watching, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlDataOffer, ()> for Watching {
    fn event(
        _state: &mut Self,
        _offer: &WlDataOffer,
        _event: <WlDataOffer as Proxy>::Event,
        (): &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataSource, Offering> for Watching {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        offering: &Offering,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                let Some((_, bytes)) = offering.iter().find(|(name, _)| *name == mime_type) else {
                    // Only the shapes that were offered can be asked for,
                    // so this is a compositor's mistake rather than a
                    // reader's -- but the pipe still has to be closed, or
                    // whoever is pasting waits for ever.
                    tracing::warn!(mime_type, "asked for a shape that was not offered");
                    return;
                };
                hand_the_bytes_over(fd, bytes.clone());
            }
            wl_data_source::Event::Cancelled => {
                source.destroy();
                // Only if it is still the current one: a copy made a
                // moment ago is what cancelled this source, and its shapes
                // are the answer now.
                if let Ok(mut held) = state.held.lock()
                    && held
                        .mine
                        .as_ref()
                        .is_some_and(|mine| Arc::ptr_eq(mine, offering))
                {
                    held.mine = None;
                }
            }
            // The rest is drag and drop, which Obelus does not do.
            _ => {}
        }
    }
}

/// Writes one shape down the pipe the compositor handed over.
///
/// On a thread of its own, because the pipe holds 64k and a picture is
/// larger: a write that fills it waits for whoever is pasting to read, and
/// doing that on the listening thread would stop Obelus answering anything
/// else about the clipboard until they did.
fn hand_the_bytes_over(fd: OwnedFd, bytes: Vec<u8>) {
    let started = std::thread::Builder::new()
        .name("obelus paste".to_string())
        .spawn(move || {
            let mut pipe = std::fs::File::from(fd);
            if let Err(error) = pipe.write_all(&bytes) {
                // The ordinary one is a broken pipe: whoever was pasting
                // changed their mind, which is theirs to do.
                tracing::debug!(%error, "handing over what was copied");
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no thread to hand over what was copied");
    }
}
