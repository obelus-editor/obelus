//! The other windows: starting an Obelus on another tree, and bringing one
//! forward.
//!
//! What the application asks for is [`obelus_app::app::Windows`], and what
//! answers it is here, because both of its halves are about a window and
//! the compositor that manages it. A window may not put itself in front of
//! the reader -- that is what stops a program taking the keys out from
//! under them -- so whatever comes forward comes forward on the
//! permission of the window the reader is in, asked for against the key
//! they just pressed. How that permission is spelled is each platform's:
//!
//! * **Wayland** is a token, from `xdg_activation_v1`, which winit asks for. A
//!   new Obelus is handed it in `XDG_ACTIVATION_TOKEN`, which winit reads as it
//!   makes the window; one already open is handed it across the knock and
//!   activates its own surface with it (`wayland`).
//! * **X11** has a token for a window being started -- the startup
//!   notification's id, in `DESKTOP_STARTUP_ID` -- and nothing for one already
//!   open. That one asks the window manager itself, saying the request comes
//!   from the reader's own hand (`x11`).
//! * **macOS and Windows** have neither. The window asks to be activated, and
//!   Windows lets it only where the window the reader was in said that it may.
//!
//! Each of those waits, so a request goes out and is answered later on the
//! window's loop: the application is never kept waiting for a compositor.

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

use std::path::{Path, PathBuf};

use obelus_app::app::Door;
use winit::{
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::Window,
};

use crate::window::Waking;

/// What the application is told it can ask of the window.
///
/// It sends rather than does: the application is on a thread of its own,
/// and every one of these is the window's to carry out.
#[derive(Debug)]
pub(crate) struct Elsewhere {
    proxy: EventLoopProxy<Waking>,
    can_bring: bool,
}

impl Elsewhere {
    /// Answers for the window behind `proxy`.
    pub(crate) const fn new(proxy: EventLoopProxy<Waking>, can_bring: bool) -> Self {
        Self { proxy, can_bring }
    }
}

impl obelus_app::app::Windows for Elsewhere {
    fn open(&self, tree: &Path) {
        let _ = self
            .proxy
            .send_event(Waking::Going(Going::Open(tree.to_path_buf())));
    }

    fn bring(&self, door: &Door) {
        let _ = self
            .proxy
            .send_event(Waking::Going(Going::Bring(door.clone())));
    }

    fn come_forward(&self, token: Option<String>) {
        let _ = self.proxy.send_event(Waking::ComeForward(token));
    }

    fn can_bring(&self) -> bool {
        self.can_bring
    }
}

/// Somewhere the reader asked to go, waiting on the permission to.
#[derive(Clone, Debug)]
pub(crate) enum Going {
    /// A new Obelus, on this tree.
    Open(PathBuf),
    /// The Obelus behind this door, already open.
    Bring(Door),
}

/// What this window has that the others are brought forward with.
pub(crate) struct Here {
    #[cfg(target_os = "linux")]
    wayland: Option<wayland::Activation>,
    #[cfg(target_os = "linux")]
    x11: bool,
}

impl Here {
    /// Finds out what the display this window is on can do.
    ///
    /// Once there is a window, which is the first moment there is a
    /// display connection to adopt.
    #[cfg(target_os = "linux")]
    pub(crate) fn take(events: &ActiveEventLoop) -> Self {
        use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

        let handle = events.display_handle().map(|handle| handle.as_raw());
        match handle {
            // Safety: winit's display, which the event loop holds open for
            // as long as there is a window -- and the window is where these
            // are asked from.
            Ok(RawDisplayHandle::Wayland(display)) => Self {
                wayland: unsafe { wayland::Activation::take(display.display) },
                x11: false,
            },
            Ok(RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)) => Self {
                wayland: None,
                x11: true,
            },
            _ => Self {
                wayland: None,
                x11: false,
            },
        }
    }

    /// And on the platforms with nothing to adopt.
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn take(_events: &ActiveEventLoop) -> Self {
        Self {}
    }

    /// Whether another window can be brought forward from here.
    ///
    /// A Wayland compositor without the protocol is the one place the
    /// answer is no: there is nothing to ask it with, and a knock the other
    /// window cannot act on is a key that does nothing.
    #[cfg(target_os = "linux")]
    pub(crate) const fn can_bring(&self) -> bool {
        self.wayland.is_some() || self.x11
    }

    /// Everywhere else, the window asks for itself.
    #[cfg(not(target_os = "linux"))]
    pub(crate) const fn can_bring(&self) -> bool {
        true
    }

    /// Whether going there wants a token from the compositor first.
    ///
    /// Wayland's for both, because both are a window coming forward on
    /// this one's permission. X11's for a new window alone: what it is
    /// handed is a startup notification, which is about a program being
    /// started.
    #[cfg(target_os = "linux")]
    pub(crate) const fn wants_a_token(&self, going: &Going) -> bool {
        match going {
            Going::Open(_) => self.wayland.is_some() || self.x11,
            Going::Bring(_) => self.wayland.is_some(),
        }
    }

    /// Nowhere else has a token to ask for.
    #[cfg(not(target_os = "linux"))]
    pub(crate) const fn wants_a_token(&self, _going: &Going) -> bool {
        false
    }

    /// Brings this window forward, for another Obelus that asked.
    #[cfg(target_os = "linux")]
    pub(crate) fn come_forward(&self, window: &Window, token: Option<String>) {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Ok(handle) = window.window_handle().map(|handle| handle.as_raw()) else {
            return;
        };
        match handle {
            RawWindowHandle::Wayland(surface) => match (self.wayland.as_ref(), token) {
                // Safety: this window's own surface, which lives as long as
                // the window this was asked through.
                (Some(activation), Some(token)) => unsafe {
                    activation.activate(token, surface.surface);
                },
                // Nothing to come forward with. Said, because the reader
                // pressed a key in the other window and nothing happened.
                _ => tracing::info!("asked to come forward with nothing to do it with"),
            },
            RawWindowHandle::Xlib(xlib) => {
                x11::come_forward(u32::try_from(xlib.window).unwrap_or_default());
            }
            RawWindowHandle::Xcb(xcb) => x11::come_forward(xcb.window.get()),
            _ => {}
        }
    }

    /// Brings this window forward, for another Obelus that asked.
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn come_forward(&self, window: &Window, _token: Option<String>) {
        // Out of the dock or the taskbar first: a minimised window that is
        // given the focus stays minimised on both.
        window.set_minimized(false);
        window.focus_window();
    }
}

/// The token a window started on this one's behalf comes forward with,
/// where it was started with one.
///
/// Read once, as the window is made: it is a permission for this window's
/// first appearance and for nothing after it.
#[cfg(target_os = "linux")]
pub(crate) fn started_with(
    events: &ActiveEventLoop,
    attributes: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    use winit::platform::startup_notify::{
        EventLoopExtStartupNotify as _, WindowAttributesExtStartupNotify as _,
    };

    match events.read_token_from_env() {
        Some(token) => attributes.with_activation_token(token),
        None => attributes,
    }
}

/// Nowhere else is started with one.
#[cfg(not(target_os = "linux"))]
pub(crate) const fn started_with(
    _events: &ActiveEventLoop,
    attributes: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    attributes
}

/// Asks the compositor for what going there will take, or `None` where
/// there is nothing to ask for and it can be done now.
#[cfg(target_os = "linux")]
pub(crate) fn ask(window: &Window) -> Option<winit::event_loop::AsyncRequestSerial> {
    use winit::platform::startup_notify::WindowExtStartupNotify as _;

    match window.request_activation_token() {
        Ok(serial) => Some(serial),
        Err(error) => {
            tracing::info!(%error, "no token to go to another window with");
            None
        }
    }
}

/// Nowhere else has anything to ask for.
#[cfg(not(target_os = "linux"))]
pub(crate) const fn ask(_window: &Window) -> Option<winit::event_loop::AsyncRequestSerial> {
    None
}

/// Goes there, with whatever the compositor gave for it.
pub(crate) fn go(going: Going, token: Option<String>) {
    match going {
        Going::Open(tree) => open(&tree, token.as_deref()),
        Going::Bring(door) => {
            let_it_come_forward();
            obelus_app::app::knock(&door, token.as_deref());
        }
    }
}

/// Starts another Obelus in a window of its own, on `tree`.
///
/// The same binary as this one, which is what "a window" means here: `obg`
/// starts `obg`. With the token in both of the variables a toolkit reads it
/// from, set on the child alone -- the one this process was started with is
/// a permission that has been used.
///
/// On the runtime, whose process handling reaps a child that is let go of:
/// the new window is the reader's from here, and nothing in this one waits
/// for it to close.
fn open(tree: &Path, token: Option<&str>) {
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            tracing::warn!(%error, "no program to start another window with");
            return;
        }
    };
    let _inside = obelus_runtime::handle().enter();
    let mut command = tokio::process::Command::new(&program);
    command
        .arg(tree)
        .current_dir(tree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID");
    // It has this window's environment, which is the shell's or the
    // terminal's already, and its standard files are nobody's: without this
    // it asks the shell again and opens a second or two later.
    #[cfg(unix)]
    command.env(obelus_program::login::PASSED_ON, "1");
    if let Some(token) = token {
        command
            .env("XDG_ACTIVATION_TOKEN", token)
            .env("DESKTOP_STARTUP_ID", token);
    }
    // A group of its own, so that the terminal this window may have been
    // started from does not take the new one with it when it closes: the
    // reader opened another window, not a child of this one.
    #[cfg(unix)]
    command.process_group(0);
    match command.spawn() {
        Ok(_) => tracing::info!(tree = %tree.display(), "started another window"),
        Err(error) => {
            tracing::warn!(%error, program = %program.display(), "another window would not start");
        }
    }
}

/// Lets the window about to be asked come forward, where that is this
/// window's to allow.
///
/// Windows': a process may take the foreground only from the one that has
/// it, and only if that one says so. Any process, because the door says
/// where the other window listens and not which process it is -- and the
/// permission lasts until the next keypress.
#[cfg(windows)]
fn let_it_come_forward() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

    // Safety: a flag and nothing through a pointer.
    if unsafe { AllowSetForegroundWindow(ASFW_ANY) } == 0 {
        tracing::info!("this window could not let another come forward");
    }
}

/// Everywhere else the permission is the token, or nothing at all.
#[cfg(not(windows))]
const fn let_it_come_forward() {}
