//! The strip along the top of the window, where the platform writes the
//! title.
//!
//! The title is the platform's to write and Obelus says nothing there of its
//! own: a terminal has no title bar, so anything written in one would be
//! something `ob` does not say, and the file being read is on the status row
//! already. What is Obelus's is the strip's *colour* -- a grey bar across the
//! top of a page drawn in a theme is a frame round the theme in somebody
//! else's colours.
//!
//! Which each platform answers differently, and neither by Obelus drawing a
//! title bar of its own. macOS lets the page run up under a transparent
//! title bar, so the ground the painter lays under everything is what shows
//! there, and the buttons, the dragging and full screen are all still the
//! system's. The double click is not: the page's own view is under the bar,
//! and the press goes to it rather than to the bar, so AppKit never sees a
//! second one and the window neither zooms nor goes to the Dock. Obelus
//! answers it instead, the way the reader has told the system to (`Presses`)
//! -- and keeps the press off the page, or a double click on the bar would
//! be one on whatever row is under it. Windows keeps its own bar and lets it be
//! painted, from Windows 11 on. Drawing the bar would mean drawing the
//! buttons and answering the hit test they need, and winit keeps that
//! message to itself -- so it would cost the snap layouts Windows opens
//! over the maximise button, for a strip with nothing in it.
//!
//! The words on the bar are the system's either way, in the system's ink,
//! so what Obelus tells it is whether the page is dark: that is what
//! decides whether the title and the buttons are drawn light or dark.
//!
//! There is no switch to take the bar away, and there was one briefly. It
//! did something on macOS, Windows and GNOME and nothing at all where most
//! of the readers who would want it are: a tiling compositor like Hyprland
//! draws no bar round a window, so a window there has none either way. Which
//! made it a row on the settings page that changes nothing for the reader
//! looking for it -- and on macOS, where it worked, a window with no bar is a
//! window nothing can move.

use std::time::{Duration, Instant};

use ratatui::style::Color;
use winit::window::{Theme, Window, WindowAttributes};

/// The window as it is asked for, with the page running up under the title
/// bar where the platform allows it.
pub(crate) fn asked_for(attributes: WindowAttributes) -> WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        attributes
    }
}

/// How many pixels at the top of the window the grid has to leave for the
/// title bar.
///
/// Nothing, except where the page runs up under it. Asked of AppKit rather
/// than written down, because the height is the system's -- and because in
/// full screen the bar is gone until the pointer asks for it, which the
/// same question answers with nothing.
#[cfg(target_os = "macos")]
pub(crate) fn height(window: &Window) -> f32 {
    let Some(shown) = appkit(window) else {
        return 0.0;
    };
    // The frame is the whole window and the layout rectangle is the part
    // the title bar does not cover, both in points.
    let points = (shown.frame().size.height - shown.contentLayoutRect().size.height).max(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a title bar is tens of pixels"
    )]
    let pixels = (points * window.scale_factor()).round() as f32;
    pixels
}

/// The window AppKit has behind winit's.
#[cfg(target_os = "macos")]
fn appkit(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use objc2_app_kit::NSView;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // Safe: the view is winit's, it lives as long as the window it is in,
    // and this is asked on the thread the event loop runs on, which is the
    // one AppKit wants.
    let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
    view.window()
}

/// Nothing: the title bar is the platform's own and outside the window.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn height(_window: &Window) -> f32 {
    0.0
}

/// Whether a place in the window, in pixels from the top, is on the title
/// bar rather than the page.
pub(crate) fn under(y: f64, titled: f32) -> bool {
    y < f64::from(titled)
}

/// The presses on the title bar, so that the second of two is a double
/// click.
///
/// Counted here because nothing else will count them: AppKit's own count
/// is on an event winit has already let go of by the time the window hears
/// the press, and the page's double click is the application's, in cells.
#[derive(Default)]
pub(crate) struct Presses {
    /// When the press was that a second one would make a double click.
    first: Option<Instant>,
}

impl Presses {
    /// A press on the title bar, answered as a double click where it is the
    /// second of two.
    pub(crate) fn pressed(&mut self, window: &Window, now: Instant) {
        if self.second(now, interval()) {
            double_clicked(window, action());
        }
    }

    /// Whether a press is the second of two, near enough together.
    ///
    /// A third press begins a pair of its own rather than making the
    /// second one's double click again: the bar would zoom back out again
    /// under a reader who clicked one time too many.
    fn second(&mut self, now: Instant, interval: Duration) -> bool {
        let second = self
            .first
            .is_some_and(|first| now.saturating_duration_since(first) <= interval);
        self.first = match second {
            true => None,
            false => Some(now),
        };
        second
    }
}

/// What a double click on a title bar does, which the reader chose in the
/// system's settings and not in Obelus's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    all(not(target_os = "macos"), not(test)),
    expect(dead_code, reason = "only macOS asks what the reader chose")
)]
enum Action {
    /// Fills the screen, or comes back from it.
    Zoom,
    /// Goes to the Dock.
    Minimise,
    /// Nothing at all.
    Nothing,
}

/// The action a system with these two preferences takes.
///
/// `AppleActionOnDoubleClick` is the setting and `Maximize` is what it was
/// called before it was called Zoom; a system that has never been asked
/// has no such key, and then the older switch for going to the Dock is
/// the one that counts. `Fill`, which the newest systems offer beside
/// Zoom, has no public way to be asked for, and zoom is the nearest thing
/// to it: a window with no frame of its own to zoom to fills the screen.
#[cfg_attr(
    all(not(target_os = "macos"), not(test)),
    expect(dead_code, reason = "only macOS asks what the reader chose")
)]
fn chosen(action: Option<&str>, minimise: bool) -> Action {
    match action {
        Some("Minimize") => Action::Minimise,
        Some("None") => Action::Nothing,
        Some(_) => Action::Zoom,
        None => match minimise {
            true => Action::Minimise,
            false => Action::Zoom,
        },
    }
}

/// How close together two presses have to be to make a double click: the
/// system's, which the reader sets.
#[cfg(target_os = "macos")]
fn interval() -> Duration {
    Duration::try_from_secs_f64(objc2_app_kit::NSEvent::doubleClickInterval())
        .unwrap_or(Duration::from_millis(500))
}

/// Read when the bar is double-clicked rather than once at the start, so a
/// change in the system's settings counts from the next one.
#[cfg(target_os = "macos")]
fn action() -> Action {
    use objc2_foundation::{NSUserDefaults, ns_string};
    let defaults = NSUserDefaults::standardUserDefaults();
    let action = defaults.stringForKey(ns_string!("AppleActionOnDoubleClick"));
    chosen(
        action.map(|action| action.to_string()).as_deref(),
        defaults.boolForKey(ns_string!("AppleMiniaturizeOnDoubleClick")),
    )
}

/// Nowhere else is the page under the bar, so nothing is ever pressed on
/// it.
#[cfg(not(target_os = "macos"))]
const fn interval() -> Duration {
    Duration::ZERO
}

/// See `interval`.
#[cfg(not(target_os = "macos"))]
const fn action() -> Action {
    Action::Nothing
}

/// Does what a double click on the bar was chosen to do.
fn double_clicked(window: &Window, action: Action) {
    match action {
        Action::Zoom => zoom(window),
        Action::Minimise => window.set_minimized(true),
        Action::Nothing => {}
    }
}

/// Zooms the window, or brings it back from being zoomed, the way the
/// green button does.
///
/// Through AppKit rather than winit's `set_maximized`, which zooms the same
/// way but also writes down that the window is maximised, and puts that
/// back on the way out of full screen. Unzoomed since by the green button,
/// which winit does not hear, the window would then zoom again by itself.
#[cfg(target_os = "macos")]
fn zoom(window: &Window) {
    if let Some(shown) = appkit(window) {
        shown.performZoom(None);
    }
}

/// See `interval`.
#[cfg(not(target_os = "macos"))]
fn zoom(window: &Window) {
    window.set_maximized(!window.is_maximized());
}

/// Paints the title bar the colour the page is drawn on, and has the system
/// write on it in ink that can be read there.
pub(crate) fn follow(window: &Window, ground: Color) {
    let rgb = crate::paint::ground_of(ground);
    window.set_theme(Some(match dark(rgb) {
        true => Theme::Dark,
        false => Theme::Light,
    }));
    painted(window, rgb);
}

/// Whether a colour is one the system should write light ink on.
///
/// By how bright it looks rather than by how much of each channel there
/// is: pure green and pure blue are the same amount of colour, and green is
/// a light page while blue is a dark one.
fn dark((r, g, b): (u8, u8, u8)) -> bool {
    let bright = 0.0722f32.mul_add(
        f32::from(b),
        0.2126f32.mul_add(f32::from(r), 0.7152 * f32::from(g)),
    );
    bright < 128.0
}

#[cfg(windows)]
fn painted(window: &Window, (r, g, b): (u8, u8, u8)) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DwmSetWindowAttribute};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    // A `COLORREF`, which is the three bytes the other way round.
    let colour: u32 = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16);
    // Safe: the handle is winit's live window, and DWM reads the four
    // bytes it is pointed at before it returns.
    let answered = unsafe {
        DwmSetWindowAttribute(
            handle.hwnd.get() as _,
            DWMWA_CAPTION_COLOR as _,
            std::ptr::from_ref(&colour).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    // Windows 10 has no such attribute and says so: its bar stays the
    // system's colour, which is the most it can be.
    if answered != 0 {
        tracing::debug!(answered, "the title bar kept the system's colour");
    }
}

/// Nothing to paint: macOS shows the page itself there, and on Linux the
/// bar is the compositor's.
#[cfg(not(windows))]
const fn painted(_window: &Window, _rgb: (u8, u8, u8)) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The second press near enough to the first is a double click, and a
    /// third begins again.
    ///
    /// Deliberate break: leave `first` where it was after a double click.
    /// The third press is then a second double click, and the window zooms
    /// back out under a reader who clicked once too often.
    #[test]
    fn the_second_of_two_presses_is_a_double_click() {
        let interval = Duration::from_millis(500);
        let start = Instant::now();
        let mut presses = Presses::default();
        assert!(!presses.second(start, interval), "one press");
        assert!(
            presses.second(start + Duration::from_millis(200), interval),
            "two close together"
        );
        assert!(
            !presses.second(start + Duration::from_millis(400), interval),
            "a third begins again"
        );
        assert!(
            presses.second(start + Duration::from_millis(600), interval),
            "and makes a pair with the fourth"
        );
    }

    /// Two presses further apart than the system's interval are two
    /// presses, and the later one may be the first of a pair.
    ///
    /// Deliberate break: compare with `>=`, or forget to start again from
    /// the late press. The first makes every slow pair a double click; the
    /// second misses the quick press that follows a slow one.
    #[test]
    fn presses_far_apart_are_two_presses() {
        let interval = Duration::from_millis(500);
        let start = Instant::now();
        let mut presses = Presses::default();
        assert!(!presses.second(start, interval));
        let late = start + Duration::from_millis(900);
        assert!(!presses.second(late, interval), "too far apart");
        assert!(
            presses.second(late + Duration::from_millis(100), interval),
            "the late one is the first of a pair"
        );
    }

    /// A double click does what the system was told it does, and a system
    /// that was never told zooms unless the older switch says the Dock.
    ///
    /// Deliberate break: answer every key that is there with zoom. A reader
    /// who chose the Dock or nothing has the window fill the screen.
    #[test]
    fn a_double_click_does_what_the_system_was_told() {
        assert_eq!(chosen(Some("Maximize"), false), Action::Zoom);
        assert_eq!(chosen(Some("Fill"), false), Action::Zoom);
        assert_eq!(chosen(Some("Minimize"), false), Action::Minimise);
        assert_eq!(chosen(Some("None"), true), Action::Nothing);
        assert_eq!(chosen(None, false), Action::Zoom);
        assert_eq!(chosen(None, true), Action::Minimise);
    }

    /// A page is dark by how bright it looks, not by how much colour is in
    /// it.
    ///
    /// Deliberate break: weigh the three channels the same. Pure green is
    /// then a third of the way up and taken for dark, and a pale blue two
    /// thirds of the way and taken for light -- the wrong ink on both.
    #[test]
    fn a_page_is_dark_by_how_bright_it_looks() {
        assert!(dark((0x1e, 0x1e, 0x2e)), "a dark theme");
        assert!(!dark((0xef, 0xf1, 0xf5)), "a light one");
        assert!(!dark((0, 0xff, 0)), "green is light");
        assert!(dark((0x64, 0x64, 0xff)), "a blue this pale is still dark");
    }
}
