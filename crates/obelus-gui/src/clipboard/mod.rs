//! Obelus holding the reader's clipboard itself.
//!
//! What a window has and a terminal has not is a connection to the display
//! server, and that is the whole of why this is here rather than in
//! `obelus-clipboard`. The programs that crate reaches for offer one shape
//! per invocation -- `wl-copy -t`, `xclip -t` -- so a copy that is words
//! for everybody *and* Obelus's own shape for another Obelus cannot be made
//! by asking one of them. It has to be made by a client that owns the
//! selection, and only this half of Obelus can be one.
//!
//! It is offered rather than imposed: `obelus_clipboard::owned_by` takes an
//! owner where there is one, and everything falls back to the programs
//! where there is not -- a compositor without the globals, a window nobody
//! has touched yet, an X server this build cannot talk to. And the last
//! thing the window does is hand the words to a program that will keep
//! them, because the selection belongs to a live client and Obelus is about
//! to stop being one.

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

/// Takes the clipboard, if this machine is one Obelus can take it on.
///
/// Called once the window exists, which is the first moment there is a
/// display connection to adopt. On Wayland what is dropped on the window is
/// heard on the same connection, and is handed to `proxy`.
#[cfg(target_os = "linux")]
pub(crate) fn take(
    events: &winit::event_loop::ActiveEventLoop,
    proxy: winit::event_loop::EventLoopProxy<crate::window::Waking>,
) {
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

    let handle = match events.display_handle() {
        Ok(handle) => handle.as_raw(),
        Err(error) => {
            tracing::warn!(%error, "no display to own the clipboard on");
            return;
        }
    };
    match handle {
        // Safety: winit's display, which the event loop holds open for as
        // long as there is a window -- and the window outlives Obelus's
        // loop, which is what closes it.
        RawDisplayHandle::Wayland(wayland) => {
            let dropped: wayland::Dropped = std::sync::Arc::new(move |path| {
                let _ = proxy.send_event(crate::window::Waking::Dropped(path));
            });
            if let Some(clipboard) = unsafe { wayland::Clipboard::take(wayland.display, dropped) } {
                obelus_clipboard::owned_by(Box::new(clipboard));
                tracing::info!(on = "wayland", "the clipboard is Obelus's own");
            }
        }
        // A connection of its own, unlike the Wayland half: X11 asks for
        // the selection with a timestamp rather than with a serial, so
        // there is nothing about winit's connection that this needs.
        RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => {
            if let Some(clipboard) = x11::Clipboard::take() {
                obelus_clipboard::owned_by(Box::new(clipboard));
                tracing::info!(on = "x11", "the clipboard is Obelus's own");
            }
        }
        other => tracing::info!(?other, "the clipboard is left to the programs"),
    }
}

/// Gives back whatever `take` took that outlives a frame.
///
/// Called from winit's `exiting`, which is the one place every way out of
/// the loop passes through -- a key, the window's own button, a failure on
/// the way up. What it is for is argued in [`wayland`]: a thread of
/// Obelus's own is blocked on winit's display, and winit is about to close
/// it.
#[cfg(target_os = "linux")]
pub(crate) fn let_go() {
    wayland::let_go();
}

/// And on the platforms where it is not taken at all.
#[cfg(not(target_os = "linux"))]
pub(crate) fn take(
    _events: &winit::event_loop::ActiveEventLoop,
    _proxy: winit::event_loop::EventLoopProxy<crate::window::Waking>,
) {
}

/// Nor given back.
#[cfg(not(target_os = "linux"))]
pub(crate) fn let_go() {}
