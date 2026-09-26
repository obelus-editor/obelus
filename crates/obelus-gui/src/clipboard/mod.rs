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

/// Takes the clipboard, if this machine is one Obelus can take it on.
///
/// Called once the window exists, which is the first moment there is a
/// display connection to adopt.
#[cfg(target_os = "linux")]
pub(crate) fn take(events: &winit::event_loop::ActiveEventLoop) {
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
            if let Some(clipboard) = unsafe { wayland::Clipboard::take(wayland.display) } {
                obelus_clipboard::owned_by(Box::new(clipboard));
                tracing::info!("the clipboard is Obelus's own");
            }
        }
        other => tracing::info!(?other, "the clipboard is left to the programs"),
    }
}

/// And on the platforms where it is not taken at all.
#[cfg(not(target_os = "linux"))]
pub(crate) fn take(_events: &winit::event_loop::ActiveEventLoop) {}
