//! Handing a URL to whatever the reader opens URLs with.
//!
//! One thing Obelus does with the outside world that is not a file: an agent
//! that needs the reader to sign in somewhere sends a URL, and Obelus asks
//! the machine to open it.
//!
//! A door of its own rather than a call to `open::that_detached` where it is
//! needed, for the reason [`crate`] is a door of its own: a suite
//! that ran against the real one would open a browser on whoever is running
//! it, once per test.

/// What Obelus does with a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opener {
    /// Whatever the machine opens links with.
    System,
    /// Nothing outside Obelus at all, which is what a test gets.
    Kept,
}

/// Which one to use, where a test has said.
static CHOSEN: std::sync::Mutex<Option<Opener>> = std::sync::Mutex::new(None);

/// The last link Obelus was asked to open, for a test to read back.
static KEPT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Asks the machine to open this.
///
/// Detached, with the child's own output thrown away -- the crate does both.
/// Obelus is in the alternate screen, and a launcher that prints one line
/// has torn the page up; and waiting on a browser would stop the loop for as
/// long as one takes to start.
///
/// What comes back says a launcher *started*, not that a browser appeared.
/// Nothing says the second, on any platform, without waiting for something
/// that may never end.
///
/// # Errors
///
/// When there is nothing on this machine that opens links -- a server over
/// ssh, a container -- which is a thing the reader has to be told rather
/// than a thing to swallow.
pub fn open(url: &str) -> std::io::Result<()> {
    if let Ok(mut kept) = KEPT.lock() {
        *kept = Some(url.to_string());
    }
    match chosen() {
        Opener::System => open::that_detached(url),
        Opener::Kept => Ok(()),
    }
}

/// Which opener is in force.
fn chosen() -> Opener {
    CHOSEN
        .lock()
        .ok()
        .and_then(|chosen| *chosen)
        .unwrap_or(Opener::System)
}

/// Uses an opener of the caller's choosing, for a test.
pub fn use_opener_for_test(opener: Opener) {
    if let Ok(mut chosen) = CHOSEN.lock() {
        *chosen = Some(opener);
    }
    if let Ok(mut kept) = KEPT.lock() {
        *kept = None;
    }
}

/// The last link Obelus was asked to open, for a test to read back.
#[must_use]
pub fn opened() -> Option<String> {
    KEPT.lock().ok().and_then(|kept| kept.clone())
}
