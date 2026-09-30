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
//! there, and the buttons, the dragging, the double-click and full screen
//! are all still the system's. Windows keeps its own bar and lets it be
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
