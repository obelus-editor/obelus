//! A window coming forward on Wayland, with a token another window got.
//!
//! winit asks for a token -- that half is the window the reader is in, and
//! winit's own -- and uses one for a window it is making. What it has no
//! call for is the other half: a window that is already open, handed a
//! token from somewhere else, activating itself with it. So that is asked
//! here, of `xdg_activation_v1`, on winit's own connection: a surface is a
//! connection's object, and a second connection has never heard of it.
//! The clipboard adopts the display the same way and for the same reason.
//!
//! No thread and no events: the protocol's global sends nothing, and an
//! `activate` is a request that goes out and is the whole of the answer.

use std::{ffi::c_void, ptr::NonNull};

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    backend::{Backend, ObjectId},
    protocol::{
        wl_registry::{self, WlRegistry},
        wl_surface::WlSurface,
    },
};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::{self, XdgActivationV1};

/// The compositor's activation global, on the window's connection.
pub(super) struct Activation {
    connection: Connection,
    activation: XdgActivationV1,
}

/// What the registry is asked into, once.
#[derive(Default)]
struct Binding {
    activation: Option<XdgActivationV1>,
}

impl Activation {
    /// Adopts `display` -- winit's -- and binds the compositor's
    /// activation, where it has one.
    ///
    /// # Safety
    ///
    /// `display` must be a live `wl_display` that outlives this, which
    /// winit's is: the event loop holds it for as long as there is a window.
    pub(super) unsafe fn take(display: NonNull<c_void>) -> Option<Self> {
        // Adopted, not owned: dropping this backend must not disconnect the
        // display winit is still drawing on.
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        let mut queue = connection.new_event_queue();
        let handle = queue.handle();
        let _registry = connection.display().get_registry(&handle, ());
        let mut binding = Binding::default();
        if let Err(error) = queue.roundtrip(&mut binding) {
            tracing::warn!(%error, "asking the compositor what it has");
            return None;
        }
        let Some(activation) = binding.activation else {
            tracing::info!("the compositor cannot bring a window forward");
            return None;
        };
        Some(Self {
            connection,
            activation,
        })
    }

    /// Brings `surface` forward with `token`.
    ///
    /// # Safety
    ///
    /// `surface` must be a live `wl_surface` on the adopted display, which
    /// the window's own is for as long as the window.
    pub(super) unsafe fn activate(&self, token: String, surface: NonNull<c_void>) {
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) };
        let surface = match id.and_then(|id| WlSurface::from_id(&self.connection, id)) {
            Ok(surface) => surface,
            Err(error) => {
                tracing::warn!(%error, "the window's own surface is not one this can name");
                return;
            }
        };
        self.activation.activate(token, &surface);
        if let Err(error) = self.connection.flush() {
            tracing::warn!(%error, "asking the compositor to bring the window forward");
        }
    }
}

impl Dispatch<WlRegistry, ()> for Binding {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == XdgActivationV1::interface().name
        {
            state.activation = Some(registry.bind(name, version.min(1), handle, ()));
        }
    }
}

impl Dispatch<XdgActivationV1, ()> for Binding {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: xdg_activation_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // It has none.
    }
}
