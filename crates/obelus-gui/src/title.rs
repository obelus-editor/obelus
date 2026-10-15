//! The strip along the top of the window, and what is open along it.
//!
//! Two bars, by the reader's setting, `bar_tabs` (`HOLDS_TABS`). Off,
//! which is the default, the bar is the one any window has: the system's
//! height, the title, and on Windows the icon before it. On, it holds a
//! tab for each open document instead, and a press on one goes to it
//! (`tabs`) -- and no title, because a window's title is the program's
//! name, which the reader knows, and the system's icon beside it says the
//! same thing again. A row of tabs wants a bar taller than the system's
//! own, which is what makes it a setting rather than the bar: what is open
//! is a key away in the list of it either way. A terminal has no title
//! bar and so no tabs, and reaches the same documents through that list.
//!
//! What is Obelus's too is what is *under* the bar -- a grey bar across
//! the top of a page drawn in a theme is a frame round the theme in
//! somebody else's colours, and a picture under the page stopping short of
//! the top of the window is the same frame.
//!
//! Which each platform answers differently. macOS lets the page run up
//! under a transparent title bar, so the ground the painter lays under
//! everything is what shows there, and the buttons, the dragging and full
//! screen are all still the system's. The double click is not: the page's
//! own view is under the bar, and the press goes to it rather than to the
//! bar, so AppKit never sees a second one and the window neither zooms nor
//! goes to the Dock. Obelus answers it instead, the way the reader has told
//! the system to (`Presses`) -- and keeps the press off the page, or a
//! double click on the bar would be one on whatever row is under it.
//!
//! Windows has no transparent bar to run the page up under: what it lets an
//! application do is paint its bar one colour, which is the picture under
//! the page stopping short of the top of the window. So there the page is
//! given the bar's room outright -- the frame is the system's still, and
//! with it the resizing, the shadow and the way a window goes to the
//! taskbar, but the strip along the top is taken into the page (`windows`)
//! -- and Obelus draws the three buttons on it (`Bar`). What it does not
//! do is answer them itself. winit keeps the window's messages to itself,
//! so Obelus listens beside it and tells Windows which part of the window
//! is which: the bar, so that dragging, a double click and the menu are
//! the system's, the buttons by name, so that the layouts Windows opens over
//! the maximise button still open there -- and the tabs as the page's, so
//! that a press on one reaches Obelus rather than dragging the window.
//!
//! So the buttons stand where each system puts them: on the right on
//! Windows, which Obelus draws, and on the left on macOS, which AppKit
//! draws and the tabs start after. Where there are tabs the title is
//! hidden on macOS, and on Windows nothing writes one; where there are
//! none, the bar says what the system's own would -- AppKit writes the
//! title on macOS (`show_the_title`), and on Windows Obelus writes the
//! icon and the title at the left, where the system would, in the
//! system's face and at its size. The buttons' ink is the system's on macOS,
//! so what Obelus tells it is whether the page is dark; the tabs are
//! written in the page's ink on both, in the face the system writes its
//! own words in.
//!
//! There is no switch to take the bar away, and there was one briefly. It
//! did something on macOS, Windows and GNOME and nothing at all where most
//! of the readers who would want it are: a tiling compositor like Hyprland
//! draws no bar round a window, so a window there has none either way. Which
//! made it a row on the settings page that changes nothing for the reader
//! looking for it -- and on macOS, where it worked, a window with no bar is a
//! window nothing can move.

mod tabs;
#[cfg(windows)]
mod windows;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use obelus_ui::shapes::Joined;
use ratatui::style::Color;
pub(crate) use tabs::{Painted, Room, Spot, Strip};
#[cfg(windows)]
pub(crate) use windows::{Bar, drawn};
use winit::window::{Theme, Window, WindowAttributes};

/// What the window is called: what a taskbar, a switcher and a list of
/// windows say about it -- and its bar, where the bar holds no tabs:
/// AppKit writes it on macOS, and Obelus on Windows.
pub(crate) const TITLE: &str = "Obelus";

/// Whether the bar holds a tab for each open document: the reader's
/// setting, `bar_tabs`, and off until the application says it.
///
/// Off, the bar is the height the system's own is and has no room for
/// tabs -- the strip is laid out in none (`room`) -- and everything that
/// measures the bar asks this: the grid under it, the buttons in its
/// middle on macOS, the hit test on Windows. One flag for the window, read
/// where the window is measured and where the system asks what a press is,
/// which on Windows is a subclass that holds nothing of the window's but a
/// pointer -- so a static, for the reason the glyph switch is one.
static HOLDS_TABS: AtomicBool = AtomicBool::new(false);

/// Says whether the bar holds the tabs. Whether that changed it.
pub(crate) fn tabbed(on: bool) -> bool {
    HOLDS_TABS.swap(on, Ordering::Relaxed) != on
}

/// Whether the bar holds the tabs.
pub(crate) fn is_tabbed() -> bool {
    HOLDS_TABS.load(Ordering::Relaxed)
}

/// The window as it is asked for, with the page running up under the title
/// bar where the platform allows it.
pub(crate) fn asked_for(attributes: WindowAttributes) -> WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        // The title shown, which is a bar without the tabs -- the setting's
        // default, and what it is until the application says otherwise
        // (`show_the_title`).
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
/// than written down, because in full screen the bar is gone until the
/// pointer asks for it, which the same question answers with nothing --
/// and where there is a bar that holds the tabs, it is made `TALL`.
#[cfg(target_os = "macos")]
pub(crate) fn height(window: &Window) -> f32 {
    let Some(shown) = appkit(window) else {
        return 0.0;
    };
    let points = bar_points(&shown);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a title bar is tens of pixels"
    )]
    let pixels = (points * window.scale_factor()).round() as f32;
    pixels
}

/// How tall the bar is, in points -- the one answer the grid and the
/// buttons both go by, or a bar AppKit made taller still would leave the
/// buttons in the middle of less than the grid leaves.
#[cfg(target_os = "macos")]
fn bar_points(shown: &objc2_app_kit::NSWindow) -> f64 {
    // The frame is the whole window and the layout rectangle is the part
    // the title bar does not cover, both in points.
    tall(
        shown.frame().size.height - shown.contentLayoutRect().size.height,
        is_tabbed(),
    )
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

/// How tall the bar is on macOS, in points: what Safari gives its row of
/// tabs. The system's own is twenty-eight, and a tab in it was a sliver.
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "only macOS asks")
)]
const TALL: f64 = 38.0;

/// How tall a bar AppKit says is this tall is drawn: at least `TALL` where
/// it holds the tabs and the system's own height where it does not, and
/// none at all where there is none, which is full screen.
///
/// Asked whether there is a point of it rather than whether it is nothing:
/// it is a subtraction of two of AppKit's doubles, and a hair over nothing
/// on the way into full screen would otherwise be a whole bar.
#[cfg_attr(
    not(any(target_os = "macos", test)),
    expect(dead_code, reason = "only macOS asks")
)]
fn tall(system: f64, tabbed: bool) -> f64 {
    match (system >= 1.0, tabbed) {
        (true, true) => system.max(TALL),
        (true, false) => system,
        (false, _) => 0.0,
    }
}

/// Stands the system's three buttons in the middle of the bar, the
/// way Zed does it, rather than at the top of the one AppKit lays them out
/// on -- where the bar holds the tabs, and is taller than AppKit's for it.
/// Where it does not, they are AppKit's, as they were before there were
/// tabs: put back once if they were moved, and otherwise left alone.
///
/// Not an empty toolbar, which is how AppKit makes a bar taller of its own
/// accord: the toolbar takes every press on the bar, so a tab could not be
/// pressed and a press on one dragged the window.
///
/// AppKit lays the bar out again whenever it likes and puts the buttons
/// back at the top of it, so this is asked at every moment Zed found it
/// does: whenever the window is measured, which is every resize and every
/// way into and out of full screen; a new appearance (`follow`); and the
/// window getting the keys or coming back into sight. Not in full screen
/// itself, where the bar is a window of its own that comes down when the
/// pointer asks for it, and has no room in the page to be the middle of.
#[cfg(target_os = "macos")]
pub(crate) fn centre_the_buttons(window: &Window) {
    use objc2_app_kit::{NSWindowButton, NSWindowStyleMask};
    let Some(shown) = appkit(window) else {
        return;
    };
    let tall = bar_points(&shown);
    if tall <= 0.0 || shown.styleMask().contains(NSWindowStyleMask::FullScreen) {
        return;
    }
    let [Some(close), Some(minimise), Some(zoom)] = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
    .map(|which| shown.standardWindowButton(which)) else {
        return;
    };
    // The view the whole bar is in, which is AppKit's height for it: made
    // taller from the top of the window down, or a button moved below its
    // foot would be one no press could reach.
    //
    // Safe: `superview` is unsafe because AppKit does not keep the view it
    // hands back alive for the caller, and objc2 retains it on the way out.
    let Some(bar) = (unsafe { close.superview().and_then(|buttons| buttons.superview()) }) else {
        return;
    };
    let buttons = [&close, &minimise, &zoom];
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A bar without the tabs is AppKit's to lay out, as any window's is:
    // where Obelus moved them, they go back where AppKit had them, once,
    // and after that nothing touches them.
    if !is_tabbed() {
        if let Some(was) = kept.take() {
            let mut frame = bar.frame();
            frame.size.height = was.height;
            frame.origin.y = shown.frame().size.height - was.gap;
            bar.setFrame(frame);
            for (button, y) in buttons.iter().zip(was.buttons) {
                let mut frame = button.frame();
                frame.origin.y = y;
                button.setFrame(frame);
            }
            bar.updateTrackingAreas();
            for button in buttons {
                button.updateTrackingAreas();
            }
        }
        return;
    }
    // Where AppKit had them, before they are first moved: the bar by how
    // far down from the window's top its foot is, which holds while the
    // window is resized under it.
    if kept.is_none() {
        let frame = bar.frame();
        *kept = Some(Kept {
            gap: shown.frame().size.height - frame.origin.y,
            height: frame.size.height,
            buttons: buttons.map(|button| button.frame().origin.y),
        });
    }
    let mut frame = bar.frame();
    frame.size.height = tall;
    frame.origin.y = shown.frame().size.height - tall;
    bar.setFrame(frame);
    for button in buttons {
        let mut frame = button.frame();
        frame.origin.y = (tall - frame.size.height) / 2.0;
        button.setFrame(frame);
    }
    // Or a button goes on lighting up where it used to be.
    bar.updateTrackingAreas();
    for button in buttons {
        button.updateTrackingAreas();
    }
}

/// Where AppKit had the bar and its buttons before `centre_the_buttons`
/// first moved them, for putting them back when the bar stops holding the
/// tabs. Nothing until they have been moved.
#[cfg(target_os = "macos")]
static KEPT: std::sync::Mutex<Option<Kept>> = std::sync::Mutex::new(None);

/// What `KEPT` keeps, in AppKit's points.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct Kept {
    /// How far the view the bar is in starts from the window's top.
    gap: f64,
    /// And how tall it was.
    height: f64,
    /// Where each button stood up its height, close first.
    buttons: [f64; 3],
}

/// Nothing: the buttons are where the system put them.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn centre_the_buttons(_window: &Window) {}

/// Has AppKit write the window's title on its bar where the bar holds no
/// tabs, the way it does on any window, and not where it does: the tabs
/// are what the bar says then.
///
/// Asked before `centre_the_buttons`, and the bar laid out again here and
/// now: a title shown or hidden is a new layout of the bar, which puts the
/// buttons back at the top of it -- and left to the next pass AppKit makes,
/// that came after they were centred, and they stayed at the top until
/// the window next had the keys.
#[cfg(target_os = "macos")]
pub(crate) fn show_the_title(window: &Window) {
    use objc2_app_kit::{NSWindowButton, NSWindowTitleVisibility};
    let Some(shown) = appkit(window) else {
        return;
    };
    let wanted = match is_tabbed() {
        true => NSWindowTitleVisibility::Hidden,
        false => NSWindowTitleVisibility::Visible,
    };
    if shown.titleVisibility() == wanted {
        return;
    }
    shown.setTitleVisibility(wanted);
    // Safe: `superview` is unsafe because AppKit does not keep the view it
    // hands back alive for the caller, and objc2 retains it on the way out.
    let bar = shown
        .standardWindowButton(NSWindowButton::CloseButton)
        .and_then(|close| unsafe { close.superview().and_then(|buttons| buttons.superview()) });
    if let Some(bar) = bar {
        bar.layoutSubtreeIfNeeded();
    }
}

/// Nothing: the title is written by whoever draws the bar -- Obelus on
/// Windows, and the compositor elsewhere.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn show_the_title(_window: &Window) {}

/// Where on a bar this tall the tabs may go, on a window this wide.
///
/// Nowhere, where there is no bar in the page: a window in full screen,
/// and on Linux, where the bar is the compositor's.
///
/// Nor where the bar does not hold them -- see `HOLDS_TABS`.
pub(crate) fn room(window: &Window, titled: f32, width: f32, bar: Option<&Drawn>) -> Option<Room> {
    if titled <= 0.0 || !is_tabbed() {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a screen is a few pixels to a point"
    )]
    let scale = window.scale_factor() as f32;
    match bar {
        // Between the frame's edge and the three buttons on the right.
        // Short of the buttons by a stretch that is always the bar's, so
        // that a window with many tabs still has somewhere to take hold of.
        Some(bar) => Some(Room {
            from: bar.inset,
            to: width - 3.0 * bar.button - (bar.words * 4.0).round(),
            titled,
            words: bar.words,
        }),
        None => beside_the_buttons(window, scale).map(|from| Room {
            from,
            to: width - (WORDS * 4.0 * scale).round(),
            titled,
            words: (WORDS * scale).round(),
        }),
    }
}

/// How big a tab's name is written on macOS: the size AppKit writes a
/// window's title in.
#[cfg(target_os = "macos")]
const WORDS: f32 = 13.0;

/// Not asked anywhere else: the bar there is Obelus's, and says its own.
#[cfg(not(target_os = "macos"))]
const WORDS: f32 = 12.0;

/// Where the tabs start on macOS: a little after the three buttons AppKit
/// stands at the left of the bar, wherever it has put them.
#[cfg(target_os = "macos")]
fn beside_the_buttons(window: &Window, scale: f32) -> Option<f32> {
    use objc2_app_kit::NSWindowButton;
    let zoom = appkit(window)?.standardWindowButton(NSWindowButton::ZoomButton)?;
    // In the bar's own points, which start at the window's left edge.
    let frame = zoom.frame();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a title bar is hundreds of points across"
    )]
    let end = (frame.origin.x + frame.size.width) as f32;
    Some(((end + 12.0) * scale).round())
}

/// Nowhere: the bar is the compositor's.
#[cfg(not(target_os = "macos"))]
const fn beside_the_buttons(_window: &Window, _scale: f32) -> Option<f32> {
    None
}

/// Whether a pane runs up under the bar, which is a bar in the page -- `titled`
/// is how tall -- and a pane hanging from the top of the page: its glass
/// then goes on up to the top of the window, over the tabs, and the
/// buttons Obelus draws are drawn over it.
///
/// Whoever draws the bar's buttons. Asked of the bar Obelus draws, it was
/// Windows alone, and on macOS -- where the page runs up under a bar whose
/// buttons are AppKit's -- the settings came down from under a strip of
/// the page left standing above them, rather than from the window's top.
pub(crate) fn from_the_top(titled: f32, row: u16, joined: Joined) -> bool {
    titled > 0.0 && row == 0 && matches!(joined, Joined::Above | Joined::Screen)
}

/// Whether a place in the window, in pixels from the top, is on the title
/// bar rather than the page.
pub(crate) fn under(y: f64, titled: f32) -> bool {
    y < f64::from(titled)
}

/// One of the three buttons Obelus draws on a bar it has taken into the
/// page, in the order they stand along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Minimise,
    /// Or restore, on a window that is maximised already.
    Maximise,
    Close,
}

impl Button {
    /// The three, left to right.
    pub(crate) const ALL: [Self; 3] = [Self::Minimise, Self::Maximise, Self::Close];

    /// How many buttons in from the right-hand edge this one starts.
    const fn places_in(self) -> f32 {
        match self {
            Self::Minimise => 3.0,
            Self::Maximise => 2.0,
            Self::Close => 1.0,
        }
    }
}

/// What a place on a window whose bar is in the page is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "only Windows asks")
)]
pub(crate) enum Part {
    /// Below the bar: the page's own.
    Page,
    /// The edge along the top, which resizes the window: the bar has taken
    /// the frame's place there, and the frame was what the edge was on.
    Edge,
    /// The bar, which drags the window.
    Bar,
    Button(Button),
    /// The tabs, which are the page's: a press on one is Obelus's.
    Tabs,
}

/// What part of a window a place on it is, in pixels from its top left,
/// with the tabs from and to where they are.
///
/// The edge first, over the buttons and the tabs as well, the way it is on
/// a window with the system's own bar -- and not at all on a maximised
/// window, which has no edge to move and whose top row of pixels is the one
/// the pointer stops on, so a button there is one thrown at rather than
/// aimed at. The bar is only what is left: where there are no tabs, which
/// is where a window is taken hold of to move it.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "only Windows asks")
)]
pub(crate) fn part(
    (x, y): (f32, f32),
    width: f32,
    titled: f32,
    [button, captioned]: [f32; 2],
    edge: f32,
    maximised: bool,
    tabs: Option<(f32, f32)>,
) -> Part {
    if y >= titled {
        return Part::Page;
    }
    if !maximised && y < edge {
        return Part::Edge;
    }
    if let Some(button) = Button::ALL.into_iter().find(|&each| {
        let [left, _, wide, tall] = button_at(each, width, captioned, button);
        (left..left + wide).contains(&x) && y < tall
    }) {
        return Part::Button(button);
    }
    match tabs.is_some_and(|(from, to)| (from..to).contains(&x)) {
        true => Part::Tabs,
        false => Part::Bar,
    }
}

/// Where a button is on a window this wide, as left, top, width and height
/// in pixels: against the right-hand edge, from the top, as tall as the
/// system's own bar would be.
///
/// Not the whole height of a bar the tabs have made taller, which is what
/// the system's own tabbed windows do too: a button stretched down beside
/// the tabs is a button that has moved, and the room under it is the bar's
/// to drag.
pub(crate) fn button_at(which: Button, width: f32, captioned: f32, button: f32) -> [f32; 4] {
    [width - which.places_in() * button, 0.0, button, captioned]
}

/// How tall to ask for the page to be on a screen of another density, where
/// the bar is the page's: what winit would have asked, less the bar.
///
/// winit keeps the window's size in points across the move by asking for the
/// old page scaled -- and the frame round it measured as a window with the
/// system's bar, which the bar then hands back to the page. Asked for as it
/// is, the page came out a bar taller at every move between two screens,
/// and the bar is already in the old page being scaled.
pub(crate) fn rescaled(old: (u32, u32), from: f64, to: f64, titled: f32) -> (u32, u32) {
    let scaled = |pixels: u32| (f64::from(pixels) / from * to).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window is thousands of pixels, and never fewer than none"
    )]
    let size = (
        scaled(old.0) as u32,
        (scaled(old.1) - f64::from(titled)).max(1.0) as u32,
    );
    size
}

/// How the bar is to be drawn on this frame: what of it is the system's to
/// measure and what the pointer is doing to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Drawn {
    /// How wide each button is, in pixels.
    pub(crate) button: f32,
    /// How tall, which is as tall as the system's own bar would be.
    pub(crate) captioned: f32,
    /// How wide and tall the mark on one is, in pixels.
    pub(crate) mark: f32,
    /// How wide the pen the marks are drawn with is, in pixels.
    pub(crate) pen: f32,
    /// How far in from the left the tabs start, in pixels: as far as the
    /// frame is wide, which is where the system puts its own icon -- and
    /// where the window's icon stands on a bar without them.
    pub(crate) inset: f32,
    /// How wide and tall the window's icon is, on a bar without the tabs.
    pub(crate) icon: f32,
    /// How big a tab's name is written, in pixels.
    pub(crate) words: f32,
    /// The button the pointer is over.
    pub(crate) hovered: Option<Button>,
    /// The button the left button went down on and is still down.
    pub(crate) pressed: Option<Button>,
    /// Whether the window is maximised, which is when the middle button
    /// restores it instead and says so.
    pub(crate) maximised: bool,
    /// Whether the window has the reader's keys. One that has not draws
    /// its marks fainter, the way the system's own bar does.
    pub(crate) active: bool,
}

/// The red a close button turns under the pointer, which is the system's
/// on every theme: it is the one button whose press loses something.
const CLOSING: [f32; 4] = [196.0 / 255.0, 43.0 / 255.0, 28.0 / 255.0, 1.0];

/// What a button is drawn in on a page this dark: the colour laid over the
/// page behind it, if any, and the colour of its mark.
///
/// The system's own, which are the page's ink at a fraction rather than
/// colours of their own -- a button lit in a grey is a grey square on a
/// picture -- and a mark fainter on a window without the reader's keys,
/// which is how a reader tells which of two windows those are.
pub(crate) fn inks(drawn: &Drawn, button: Button, dark: bool) -> (Option<[f32; 4]>, [f32; 4]) {
    let ink = match dark {
        true => [1.0, 1.0, 1.0],
        false => [0.0, 0.0, 0.0],
    };
    let [r, g, b] = ink;
    let [red, green, blue, _] = CLOSING;
    // Pressed only while the pointer is still over the button it went down
    // on, the way a button anywhere is: dragged off it, it stops looking
    // pressed, because letting go there does nothing.
    match (
        drawn.hovered == Some(button),
        drawn.pressed == Some(button),
        button,
    ) {
        (true, false, Button::Close) => (Some(CLOSING), [1.0; 4]),
        (true, true, Button::Close) => (Some([red, green, blue, 0.9]), [1.0; 4]),
        (true, false, _) => (Some([r, g, b, 0.06]), [r, g, b, 0.9]),
        (true, true, _) => (Some([r, g, b, 0.04]), [r, g, b, 0.62]),
        (false, ..) => match drawn.active {
            true => (None, [r, g, b, 0.9]),
            false => (None, [r, g, b, 0.36]),
        },
    }
}

/// What the bar behind the tabs is laid over with on a page this dark: a
/// shade darker than the page, so that the tab being read -- which is the
/// page's own colour -- stands out of it as part of the page under it.
///
/// Darker on a light page as on a dark one, which is what the system's
/// own tabbed windows do: the page is the lit thing, and the bar the frame
/// round it.
pub(crate) const fn bar_shade(dark: bool) -> [f32; 4] {
    match dark {
        true => [0.0, 0.0, 0.0, 0.28],
        false => [0.0, 0.0, 0.0, 0.07],
    }
}

/// What a tab's name is written in on a page this dark: the page's ink,
/// full on the tab being read and fainter on the rest, and fainter again
/// on a window without the reader's keys.
pub(crate) const fn tab_ink(reading: bool, active: bool, dark: bool) -> [f32; 4] {
    let strength = match (reading, active) {
        (true, true) => 1.0,
        (false, true) => 0.62,
        (true, false) => 0.55,
        (false, false) => 0.4,
    };
    match dark {
        true => [1.0, 1.0, 1.0, strength],
        false => [0.0, 0.0, 0.0, strength],
    }
}

/// What a tab under the pointer is laid over with: the bar's own shade
/// again, a little more of it -- a tab that could be chosen, and not one
/// that has been.
pub(crate) const fn under_the_pointer(dark: bool) -> [f32; 4] {
    match dark {
        true => [1.0, 1.0, 1.0, 0.07],
        false => [0.0, 0.0, 0.0, 0.05],
    }
}

/// The line between two tabs neither of which is lit: the page's ink, a
/// little of it.
pub(crate) const fn seam(dark: bool) -> [f32; 4] {
    match dark {
        true => [1.0, 1.0, 1.0, 0.16],
        false => [0.0, 0.0, 0.0, 0.16],
    }
}

/// What an arrow at the end of the tabs is drawn in: the page's ink, faint
/// where there is nothing further that way.
pub(crate) fn arrow_inks(more: bool, hovered: bool, dark: bool) -> (Option<[f32; 4]>, [f32; 4]) {
    let [r, g, b] = match dark {
        true => [1.0, 1.0, 1.0],
        false => [0.0, 0.0, 0.0],
    };
    match (more, hovered) {
        (true, true) => (Some([r, g, b, 0.06]), [r, g, b, 0.9]),
        (true, false) => (None, [r, g, b, 0.7]),
        (false, _) => (None, [r, g, b, 0.2]),
    }
}

/// What the title is written in on a bar without the tabs, on a page this
/// dark: the page's ink, at full strength on a window with the reader's
/// keys and faint on one without, as the marks on the buttons are.
pub(crate) fn title_ink(drawn: &Drawn, dark: bool) -> [f32; 4] {
    let strength = match drawn.active {
        true => 1.0,
        false => 0.36,
    };
    match dark {
        true => [1.0, 1.0, 1.0, strength],
        false => [0.0, 0.0, 0.0, strength],
    }
}

/// The bar Obelus draws, which only Windows has -- see `windows`.
#[cfg(not(windows))]
pub(crate) enum Bar {}

#[cfg(not(windows))]
impl Bar {
    pub(crate) fn height(&self, _window: &Window) -> f32 {
        match *self {}
    }

    pub(crate) fn drawn(&self, _window: &Window, _active: bool) -> Drawn {
        match *self {}
    }

    pub(crate) fn tabs_are(&self, _tabs: Option<(f32, f32)>) {
        match *self {}
    }
}

/// Nothing: the bar is the system's, or the page already runs up under it.
#[cfg(not(windows))]
pub(crate) const fn drawn(_window: &Window) -> Option<Bar> {
    None
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
    // A new appearance is a new layout of the bar, and the buttons go back
    // to the top of it.
    centre_the_buttons(window);
}

/// Whether a colour is one the system should write light ink on.
///
/// By how bright it looks rather than by how much of each channel there
/// is: pure green and pure blue are the same amount of colour, and green is
/// a light page while blue is a dark one.
pub(crate) fn dark((r, g, b): (u8, u8, u8)) -> bool {
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

    /// A bar without the tabs is the system's own, and full screen is no
    /// bar either way.
    ///
    /// Deliberate break: leave `tabbed` out of `tall`. The bar is made
    /// taller for tabs it is not holding, which is the strip the setting is
    /// for taking away.
    #[test]
    fn a_bar_without_tabs_is_the_systems() {
        assert!((tall(28.0, false) - 28.0).abs() < f64::EPSILON);
        assert!(tall(0.0, false).abs() < f64::EPSILON, "full screen");
    }

    /// The system's bar is made taller, a taller one is left alone, and
    /// full screen -- no bar, or a hair of one -- is still no bar.
    ///
    /// Deliberate breaks: `system > 0.0` for the first arm, and the hair
    /// is a whole bar; `system` alone for its answer, and the system's bar
    /// stays a sliver; `TALL` alone, and a taller one is cut down.
    #[test]
    fn the_bar_is_tall_enough_for_tabs_wherever_there_is_one() {
        assert!(
            (tall(28.0, true) - TALL).abs() < f64::EPSILON,
            "the system's bar"
        );
        assert!(
            (tall(52.0, true) - 52.0).abs() < f64::EPSILON,
            "a taller one"
        );
        assert!(tall(0.0, true).abs() < f64::EPSILON, "full screen");
        assert!(tall(1e-9, true).abs() < f64::EPSILON, "a hair of a bar");
    }

    /// A window 1000 pixels wide with a bar 31 tall, buttons 46 wide and
    /// an edge of 8, which is the system's at its standard density.
    fn on(x: f32, y: f32, maximised: bool) -> Part {
        part(
            (x, y),
            1000.0,
            41.0,
            [46.0, 31.0],
            8.0,
            maximised,
            Some((8.0, 400.0)),
        )
    }

    /// The bar is the bar, the buttons stand against the right-hand edge in
    /// the system's order, and below the bar is the page.
    ///
    /// Deliberate break: count the buttons in from the left, or put close
    /// first. A press meant to minimise closes the window. And answer the
    /// tabs as the bar: a press on one drags the window instead of going
    /// to the document. And leave `y < tall` out of `part`: the buttons
    /// reach down the whole of a bar the tabs made taller, where the
    /// system's own stop at the height of its own bar.
    #[test]
    fn the_buttons_stand_against_the_right_in_the_systems_order() {
        assert_eq!(on(500.0, 20.0, false), Part::Bar);
        assert_eq!(on(999.0, 20.0, false), Part::Button(Button::Close));
        assert_eq!(on(999.0, 35.0, false), Part::Bar, "under the buttons");
        assert_eq!(on(200.0, 20.0, false), Part::Tabs);
        assert_eq!(on(400.0, 20.0, false), Part::Bar, "past the last tab");
        assert_eq!(on(954.0, 20.0, false), Part::Button(Button::Close));
        assert_eq!(on(953.0, 20.0, false), Part::Button(Button::Maximise));
        assert_eq!(on(907.0, 20.0, false), Part::Button(Button::Minimise));
        assert_eq!(on(862.0, 20.0, false), Part::Button(Button::Minimise));
        assert_eq!(on(861.0, 20.0, false), Part::Bar, "left of the three");
        assert_eq!(on(999.0, 41.0, false), Part::Page, "the first row below");
        assert_eq!(on(500.0, 400.0, false), Part::Page);
    }

    /// The top edge resizes a window, over the buttons as well, and a
    /// maximised window has no edge: there the top row is the bar's.
    ///
    /// Deliberate break: leave `maximised` out of `part`. A maximised
    /// window's top row then resizes, which a maximised window cannot, and
    /// a pointer thrown into the corner misses the close button.
    #[test]
    fn the_top_edge_resizes_a_window_that_is_not_maximised() {
        assert_eq!(on(500.0, 0.0, false), Part::Edge);
        assert_eq!(on(200.0, 3.0, false), Part::Edge, "over the tabs too");
        assert_eq!(on(999.0, 7.0, false), Part::Edge, "over close too");
        assert_eq!(on(999.0, 8.0, false), Part::Button(Button::Close));
        assert_eq!(on(500.0, 0.0, true), Part::Bar);
        assert_eq!(on(999.0, 0.0, true), Part::Button(Button::Close));
    }

    /// The title on a bar without the tabs is in the page's ink, and
    /// fainter on a window without the reader's keys.
    ///
    /// Deliberate break: leave `active` out of `title_ink`. A window in the
    /// background writes its title as boldly as the one being typed in, and
    /// a reader has the buttons alone to tell the two apart by.
    #[test]
    fn the_title_is_in_the_pages_ink_and_fainter_without_the_keys() {
        let on = title_ink(&drawn(None, None, true), true);
        let off = title_ink(&drawn(None, None, false), true);
        assert_eq!(on[..3], [1.0, 1.0, 1.0], "a dark page is written light");
        assert!(off[3] < on[3], "{off:?} is not fainter than {on:?}");
        assert_eq!(title_ink(&drawn(None, None, true), false)[..3], [0.0; 3]);
    }

    /// A bar on a page of this darkness, with the pointer as given.
    fn drawn(hovered: Option<Button>, pressed: Option<Button>, active: bool) -> Drawn {
        Drawn {
            button: 46.0,
            captioned: 31.0,
            mark: 10.0,
            pen: 1.0,
            inset: 8.0,
            icon: 16.0,
            words: 12.0,
            hovered,
            pressed,
            maximised: false,
            active,
        }
    }

    /// Under the pointer the close button turns red with a white mark, on
    /// a light page as on a dark one, and the others take a shade of the
    /// page's own ink.
    ///
    /// Deliberate break: draw close lit the way the others are. The one
    /// button that loses something then looks like the two that do not.
    #[test]
    fn close_turns_red_under_the_pointer() {
        let close = Some(Button::Close);
        for dark in [true, false] {
            let (behind, mark) = inks(&drawn(close, None, true), Button::Close, dark);
            assert_eq!(behind, Some(CLOSING), "dark: {dark}");
            assert_eq!(mark, [1.0; 4], "dark: {dark}");
        }
        let (behind, mark) = inks(
            &drawn(Some(Button::Minimise), None, true),
            Button::Minimise,
            false,
        );
        assert_eq!(behind, Some([0.0, 0.0, 0.0, 0.06]));
        assert_eq!(mark[..3], [0.0; 3]);
        let (behind, _) = inks(&drawn(close, None, true), Button::Minimise, false);
        assert_eq!(behind, None, "only the button under the pointer");
    }

    /// A button looks pressed only while the pointer is still over it, and
    /// a window without the keys draws its marks fainter.
    ///
    /// Deliberate break: answer `pressed` without asking `hovered`. A press
    /// dragged off the button still looks pressed, though letting go there
    /// does nothing; and with `active` left out, the reader cannot tell
    /// which of two windows has their keys.
    #[test]
    fn a_button_looks_pressed_only_under_the_pointer() {
        let maximise = Some(Button::Maximise);
        let (held, _) = inks(&drawn(maximise, maximise, true), Button::Maximise, true);
        let (lit, _) = inks(&drawn(maximise, None, true), Button::Maximise, true);
        assert_ne!(held, lit, "pressed is not hovered");
        let (off, _) = inks(
            &drawn(Some(Button::Close), maximise, true),
            Button::Maximise,
            true,
        );
        assert_eq!(off, None, "dragged off it");
        let (_, awake) = inks(&drawn(None, None, true), Button::Maximise, true);
        let (_, asleep) = inks(&drawn(None, None, false), Button::Maximise, true);
        assert!(asleep[3] < awake[3], "fainter without the keys");
    }

    /// On a screen of another density the page is asked for as winit
    /// would ask, less the bar: the frame winit measures round it has a
    /// bar of its own, which goes to the page.
    ///
    /// Deliberate break: leave `titled` out. The page asked for is then the
    /// old page scaled, the frame adds a bar, the bar goes to the page --
    /// and the window is a bar taller after every move between screens.
    #[test]
    fn a_page_on_another_screen_is_asked_for_less_the_bar() {
        assert_eq!(rescaled((1000, 800), 1.0, 1.5, 47.0), (1500, 1153));
        assert_eq!(rescaled((1500, 1153), 1.5, 1.0, 31.0), (1000, 738));
        assert_eq!(rescaled((10, 10), 1.0, 1.0, 31.0).1, 1, "never nothing");
    }
}
