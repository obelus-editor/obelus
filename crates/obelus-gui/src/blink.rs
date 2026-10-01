//! How often the caret blinks, asked of the system rather than decided.
//!
//! A terminal's caret blinks at whatever rate the reader set for their
//! terminal, and every other window on their screen blinks at the rate they
//! set for the desktop. A window of Obelus's own that picked a number would
//! be the one thing on the screen blinking to its own time -- which is not
//! a detail: a caret is what a reader's eye goes to, and two rhythms in one
//! field of view are felt before they are noticed.
//!
//! The three platforms answer in three different shapes, and only one of
//! them has an API for it.
//!
//! **Windows** does: `GetCaretBlinkTime`, which gives the time between one
//! toggle and the next -- half a cycle, not a whole one -- and returns
//! `INFINITE` for a caret the reader has stopped.
//!
//! **macOS** has no call, but it has the two defaults AppKit itself reads,
//! `NSTextInsertionPointBlinkPeriodOn` and `...Off`, in seconds. They are
//! unset unless somebody has said otherwise, and unset means half a second
//! each way.
//!
//! **Linux** has nothing of its own at all -- not in X11, not in Wayland,
//! not in the kernel. What exists is the *desktop's* setting, and there are
//! two of those: GNOME keeps `org.gnome.desktop.interface cursor-blink`,
//! `cursor-blink-time` and `cursor-blink-timeout`, and KDE keeps
//! `CursorBlinkRate` in `kdeglobals`, which is the same number in the same
//! units (a whole cycle, in milliseconds) because Qt's `cursorFlashTime`
//! is. Both are asked, in that order, through the command each desktop
//! ships for reading them.
//!
//! The properly portable way to ask on Linux is the XDG settings portal --
//! `org.freedesktop.portal.Settings.Read`, which GNOME's and KDE's portals
//! both answer with the GNOME key, and which is also the only way that
//! works from inside a sandbox. It wants a DBus client, which is a dozen
//! crates for one question asked once, so it is not done here: it is what
//! to reach for if Obelus ever grows a DBus dependency for another reason.
//!
//! Asked once, on the way up. A reader who changes it while Obelus is open
//! sees it at the next start, which is the cost of not having a listener.

use std::time::Duration;

/// What a whole cycle is where nobody says otherwise.
///
/// GNOME's own default, which is also within a fifth of KDE's and of
/// Windows'. A number that had to be picked, picked where everyone else
/// picked it.
///
/// Read on the platforms that have to work it out, which is every one
/// but Windows: there the system is asked for the rate outright and
/// nobody needs a default. `SETTLES` below says the same thing one
/// platform narrower -- this one is wanted on macOS as well, which is a
/// unix.
#[cfg(any(test, unix))]
const CYCLE: Duration = Duration::from_millis(1200);

/// How long after the last key a caret stops blinking, by the same default.
///
/// Only a Linux desktop says anything about stopping; macOS and Windows
/// never stop a caret, so there this is read by nothing but the tests.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
const SETTLES: Duration = Duration::from_secs(10);

/// What a program says when asked, or nothing where it is not installed or
/// would not answer.
///
/// Only on the two platforms that have no call for this.
#[cfg(any(unix, target_os = "macos"))]
#[cfg(not(windows))]
fn said(program: &str, arguments: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|ran| ran.status.success())
        .map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string())
        .filter(|said| !said.is_empty())
}

/// How a caret blinks here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Blink {
    /// How long between one toggle and the next, which is half a cycle.
    pub(crate) every: Duration,
    /// How long after the last key it stops and stays visible.
    ///
    /// `None` for a caret that blinks for as long as the window is open,
    /// which is what a timeout of zero means.
    pub(crate) settles: Option<Duration>,
}

impl Blink {
    /// What this system says, or `None` where it says not to blink.
    pub(crate) fn asked() -> Option<Self> {
        let asked = Self::of_this_platform();
        // Where it came from is worth a line: a caret blinking to the wrong
        // time is the sort of thing a reader reports as "it feels wrong",
        // and the answer is which of these was read.
        tracing::info!(?asked, "how a caret blinks here");
        asked
    }

    /// Windows keeps the time between toggles, which is what this wants.
    #[cfg(windows)]
    fn of_this_platform() -> Option<Self> {
        // Milliseconds between one toggle and the next, or `INFINITE` for a
        // caret the reader has turned off. Safe: it takes nothing, reads
        // nothing of ours, and cannot fail.
        let between = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() };
        if between == 0 || between == u32::MAX {
            return None;
        }
        Some(Self {
            every: Duration::from_millis(u64::from(between)),
            // Windows does not stop a caret blinking, so neither does this.
            settles: None,
        })
    }

    /// macOS keeps the two halves of the cycle, in seconds, and usually
    /// keeps neither.
    #[cfg(target_os = "macos")]
    fn of_this_platform() -> Option<Self> {
        let default = |key: &str| {
            said("defaults", &["read", "-g", key])?
                .parse::<f64>()
                .ok()
                .filter(|seconds| *seconds > 0.0)
        };
        // Unset is the ordinary case and means half a second each way.
        // Both are read because a reader who set one may have set the
        // other, and the longer of the two is what the eye follows.
        let on = default("NSTextInsertionPointBlinkPeriodOn");
        let off = default("NSTextInsertionPointBlinkPeriodOff");
        let every = match (on, off) {
            (None, None) => CYCLE / 2,
            (on, off) => Duration::from_secs_f64(on.or(off).unwrap_or(0.5)),
        };
        Some(Self {
            every,
            settles: None,
        })
    }

    /// And a desktop has to be asked which desktop it is.
    #[cfg(all(unix, not(target_os = "macos")))]
    fn of_this_platform() -> Option<Self> {
        let gnome = |key: &str| said("gsettings", &["get", "org.gnome.desktop.interface", key]);
        if let Some(blinking) = gnome("cursor-blink") {
            return Self::from_settings(
                &blinking,
                gnome("cursor-blink-time").as_deref(),
                gnome("cursor-blink-timeout").as_deref(),
            );
        }
        // KDE's, which is the same number in the same units: Qt's
        // `cursorFlashTime` is a whole cycle in milliseconds, and this is
        // what fills it. Both the current name and the one before it,
        // because a machine has one or the other.
        let kde = |read: &str| {
            said(
                read,
                &[
                    "--file",
                    "kdeglobals",
                    "--group",
                    "KDE",
                    "--key",
                    "CursorBlinkRate",
                ],
            )
        };
        if let Some(rate) = kde("kreadconfig6").or_else(|| kde("kreadconfig5")) {
            return Self::from_settings("true", Some(&rate), None);
        }
        // A desktop that answers neither -- and there are plenty -- gets
        // what everyone's default is rather than a caret that sits still.
        tracing::info!("no desktop setting says how a caret blinks, so the usual");
        Some(Self::usual())
    }

    /// What everyone's default is.
    #[cfg(any(test, all(unix, not(target_os = "macos"))))]
    fn usual() -> Self {
        Self {
            every: CYCLE / 2,
            settles: Some(SETTLES),
        }
    }

    /// Reads the three answers, whatever of them arrived.
    ///
    /// Separate from asking so that what is read can be checked without a
    /// desktop to read it from.
    #[cfg(any(test, all(unix, not(target_os = "macos"))))]
    fn from_settings(blinking: &str, cycle: Option<&str>, timeout: Option<&str>) -> Option<Self> {
        if blinking.trim() == "false" {
            return None;
        }
        // A whole cycle, on and off together -- so the time between
        // toggles is half of it. Reading it as the half is the mistake
        // that hides: a caret blinking at twice the rate of everything
        // else on the screen looks like a caret, just a hurried one.
        let cycle = cycle
            .and_then(|said| said.trim().parse::<u64>().ok())
            .filter(|said| *said > 0)
            .map_or(CYCLE, Duration::from_millis);
        let settles = timeout
            .and_then(|said| said.trim().parse::<u64>().ok())
            .map_or(Some(SETTLES), |seconds| {
                // Zero is the setting's own way of saying "never stop".
                (seconds > 0).then(|| Duration::from_secs(seconds))
            });
        Some(Self {
            every: cycle / 2,
            settles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The setting is a whole cycle, and a toggle is half of one.
    ///
    /// Deliberate break: taking the number as the time between toggles --
    /// which is what Windows' own call gives, and what makes this read
    /// like the right thing -- blinks at twice the rate of every other
    /// window on the screen.
    #[test]
    fn a_toggle_is_half_of_what_the_desktop_calls_a_cycle() {
        let blink = Blink::from_settings("true", Some("1200"), Some("10")).expect("a blink");
        assert_eq!(blink.every, Duration::from_millis(600));
        assert_eq!(blink.settles, Some(Duration::from_secs(10)));
    }

    /// A reader who turned blinking off gets a caret that sits still.
    ///
    /// Deliberate break: answering `Some` whatever the first setting says
    /// makes this a blinking caret.
    #[test]
    fn a_desktop_that_says_not_to_blink_is_obeyed() {
        assert_eq!(
            Blink::from_settings("false", Some("1200"), Some("10")),
            None
        );
    }

    /// And a timeout of zero is the setting saying "never stop".
    ///
    /// Deliberate break: reading zero as a duration stops the blinking
    /// immediately, which is a caret that never blinks at all.
    #[test]
    fn a_timeout_of_zero_blinks_for_ever() {
        let blink = Blink::from_settings("true", Some("1200"), Some("0")).expect("a blink");
        assert_eq!(blink.settles, None);
    }

    /// An answer that is missing or is not a number is the usual one.
    ///
    /// Deliberate break: `parse().unwrap_or(0)` gives a cycle of nothing,
    /// which is a caret toggling on every frame.
    #[test]
    fn what_cannot_be_read_is_the_usual() {
        let missing = Blink::from_settings("true", None, None).expect("a blink");
        assert_eq!(missing, Blink::usual());
        let nonsense = Blink::from_settings("true", Some("often"), Some("")).expect("a blink");
        assert_eq!(nonsense, Blink::usual());
    }
}
