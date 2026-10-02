//! A window coming forward on X11, asked of the window manager.
//!
//! X11 has a token for a window being started and none for one already
//! open, so what is asked is the window manager itself: `_NET_ACTIVE_WINDOW`,
//! sent to the root window. Its first word says who is asking, and the
//! answer is the one the specification gives for a request made on the
//! reader's own behalf -- a pager's, `2` -- because that is what this is:
//! the reader chose this window in another one a moment ago. A request
//! marked as the application's own is the one a window manager guarding
//! the focus refuses, and from a window that has not been touched for an
//! hour it would be right to.
//!
//! A connection of its own, opened for the one request: nothing about
//! winit's is needed, and the window is named by its id.

use x11rb::{
    connection::Connection as _,
    protocol::xproto::{ClientMessageEvent, ConnectionExt as _, EventMask},
};

/// Who is asking, in the words `_NET_ACTIVE_WINDOW` takes.
const ON_THE_READERS_BEHALF: u32 = 2;

/// Asks the window manager to bring `window` forward.
pub(super) fn come_forward(window: u32) {
    if let Err(error) = asked(window) {
        tracing::warn!(%error, "asking the window manager to bring the window forward");
    }
}

/// The request, with whatever went wrong on the way.
fn asked(window: u32) -> Result<(), Box<dyn std::error::Error>> {
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let active = connection
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
        .reply()?
        .atom;
    let event = ClientMessageEvent::new(
        32,
        window,
        active,
        [ON_THE_READERS_BEHALF, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    connection
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )?
        .check()?;
    connection.flush()?;
    Ok(())
}
