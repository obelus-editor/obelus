//! The bar on Windows, taken into the page.
//!
//! What a window's frame is, Windows works out by asking the window
//! (`WM_NCCALCSIZE`), and what a place on it is the same way
//! (`WM_NCHITTEST`). winit answers both and keeps them to itself, so Obelus
//! listens beside it -- a subclass, which is Windows' own way for a second
//! party to hear a window's messages first -- and answers those two
//! differently: the frame without the strip along the top, and the strip as
//! the bar and its three buttons.
//!
//! Everything else stays the system's because the window keeps the style of
//! a window with a bar: the resizing on three sides, the shadow and the
//! corners DWM draws, maximising and minimising as the system animates them,
//! the menu on alt+space and on a right click on the bar. That is why the
//! frame is cut down rather than the window asked for without one, which is
//! what winit offers: a window with no frame has none of those, and a
//! maximised one covers the taskbar.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{RDW_INTERNALPAINT, RedrawWindow, ScreenToClient},
    UI::{
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
        Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GWL_STYLE, GetClientRect, GetWindowLongW, HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON,
            HTMINBUTTON, HTTOP, IsZoomed, NCCALCSIZE_PARAMS, PostMessageW, SC_CLOSE, SC_MAXIMIZE,
            SC_MINIMIZE, SC_RESTORE, SM_CXPADDEDBORDER, SM_CYCAPTION, SM_CYSIZEFRAME,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
            WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
            WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
            WS_CAPTION,
        },
    },
};
use winit::window::Window;

use super::{Button, Drawn, Part};

/// Which of the window's subclasses this is. Any number will do, as long as
/// it is the same one each time: nothing else subclasses this window.
const LISTENING: usize = 0x0b31;

/// How wide a button is at the system's standard density, which is what the
/// system's own are on Windows 10 and 11. No metric says it: `SM_CXSIZE` is
/// the size of the bitmaps the buttons had before DWM drew them.
const BUTTON: i32 = 46;

/// How wide and tall the mark on a button is at that density. Not the size
/// of the glyphs the system draws its own in, which is ten: that is the
/// face's em, and the ink inside it is eight -- measured off the system's
/// own bar at 250%, where it is twenty pixels and ten would have been
/// twenty-five.
const MARK: f32 = 8.0;

/// The bar Obelus draws on Windows: what the pointer is doing to its
/// buttons, which the window's messages say and the frame reads.
pub(crate) struct Bar {
    shared: Arc<Shared>,
}

/// What the subclass hears and the frame draws, both on the window's own
/// thread -- atomics because the subclass holds it through a pointer and
/// not through anything the borrow checker can see.
#[derive(Default)]
struct Shared {
    /// The button the pointer is over, as `held`'s number for it.
    hovered: AtomicU8,
    /// The button the left button went down on.
    pressed: AtomicU8,
}

/// Takes the bar into the page, or says why it could not and leaves it the
/// system's.
pub(crate) fn drawn(window: &Window) -> Option<Bar> {
    let hwnd = handle(window)?;
    let shared = Arc::new(Shared::default());
    // Given to the subclass for as long as it listens, and taken back when
    // the window goes -- see `WM_NCDESTROY` below.
    let data = Arc::into_raw(Arc::clone(&shared)) as usize;
    // Safe: the window is winit's and alive, and this is its own thread.
    let listening = unsafe { SetWindowSubclass(hwnd, Some(listen), LISTENING, data) };
    if listening == 0 {
        // Safe: the subclass was never set, so this is the only owner.
        drop(unsafe { Arc::from_raw(data as *const Shared) });
        tracing::warn!("the title bar stays the system's");
        return None;
    }
    // Asked again, now that the answer is Obelus's: the window was made
    // with the system's bar, and its frame is what was measured then.
    //
    // Safe: the same window, on the same thread.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    Some(Bar { shared })
}

impl Bar {
    /// How many pixels at the top of the window are the bar's: the height
    /// the system's own bar had, which the page now starts below.
    pub(crate) fn height(&self, window: &Window) -> f32 {
        handle(window)
            .filter(|&hwnd| captioned(hwnd))
            .map_or(0.0, |hwnd| {
                #[expect(clippy::cast_precision_loss, reason = "tens of pixels")]
                let titled = measured(hwnd).titled as f32;
                titled
            })
    }

    /// The bar as this frame draws it.
    pub(crate) fn drawn(&self, window: &Window, active: bool) -> Drawn {
        let hwnd = handle(window);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a screen is a few pixels to a point"
        )]
        let scale = window.scale_factor() as f32;
        // Floored: the system draws its pen two pixels wide at 250%, and
        // rounded it is three, which is a mark drawn in bold.
        let pen = scale.floor().max(1.0);
        // The same oddness as the pen, so that a line through the middle
        // of the mark is on whole pixels rather than across two.
        let mut mark = (MARK * scale).round();
        if (mark - pen).rem_euclid(2.0) != 0.0 {
            mark += 1.0;
        }
        #[expect(clippy::cast_precision_loss, reason = "tens of pixels")]
        let button = hwnd.map_or(0, |hwnd| measured(hwnd).button) as f32;
        Drawn {
            button,
            mark,
            pen,
            hovered: held(self.shared.hovered.load(Ordering::Relaxed)),
            pressed: held(self.shared.pressed.load(Ordering::Relaxed)),
            maximised: window.is_maximized(),
            active,
        }
    }
}

/// What the system's bar measured, in pixels at the window's density.
struct Measured {
    /// How tall the bar is.
    titled: i32,
    /// How far down from the top the edge that resizes the window reaches.
    edge: i32,
    /// How wide each button is.
    button: i32,
}

/// The window's own measures, asked at its density rather than the
/// system's: a window on a second screen is drawn at that screen's.
fn measured(hwnd: HWND) -> Measured {
    // Safe: the window is alive, which is all either call asks.
    let dpi = match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        dpi => dpi,
    };
    let metric = |index| unsafe { GetSystemMetricsForDpi(index, dpi) };
    let edge = metric(SM_CYSIZEFRAME) + metric(SM_CXPADDEDBORDER);
    #[expect(
        clippy::cast_possible_wrap,
        reason = "a density is a few hundred dots to the inch"
    )]
    let button = (BUTTON * dpi as i32 + 48) / 96;
    Measured {
        titled: metric(SM_CYCAPTION) + edge,
        edge,
        button,
    }
}

/// The window's handle, where it is a Win32 window.
fn handle(window: &Window) -> Option<HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as HWND)
}

/// Whether the window has a bar's style, which is when there is a bar to
/// take: a window winit has made full screen has none.
fn captioned(hwnd: HWND) -> bool {
    // Safe: the window is alive.
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) }.cast_unsigned();
    style & WS_CAPTION == WS_CAPTION
}

/// A button as `Shared` holds it, nought for none.
const fn holding(button: Option<Button>) -> u8 {
    match button {
        None => 0,
        Some(Button::Minimise) => 1,
        Some(Button::Maximise) => 2,
        Some(Button::Close) => 3,
    }
}

/// The other way.
const fn held(number: u8) -> Option<Button> {
    match number {
        1 => Some(Button::Minimise),
        2 => Some(Button::Maximise),
        3 => Some(Button::Close),
        _ => None,
    }
}

/// The button a hit test answered, as the system sends it back with every
/// message about the pointer off the page.
const fn hit(answer: WPARAM) -> Option<Button> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a hit test is a small number"
    )]
    match answer as u32 {
        HTMINBUTTON => Some(Button::Minimise),
        HTMAXBUTTON => Some(Button::Maximise),
        HTCLOSE => Some(Button::Close),
        _ => None,
    }
}

/// Puts a button where `Shared` holds it, and draws the window again if
/// that changed what is on it.
fn hold(hwnd: HWND, slot: &AtomicU8, button: Option<Button>) {
    if slot.swap(holding(button), Ordering::Relaxed) != holding(button) {
        // What winit's own `request_redraw` does, which arrives as the
        // `RedrawRequested` every other frame does.
        //
        // Safe: the window is alive.
        unsafe {
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_INTERNALPAINT,
            )
        };
    }
}

/// Does what a button is for, the way the system's own would: as a command
/// from the window's menu, so that the window animates and winit hears the
/// window being minimised or asked to close as it hears any other.
fn press(hwnd: HWND, button: Button) {
    let command = match button {
        Button::Minimise => SC_MINIMIZE,
        // Safe: the window is alive.
        Button::Maximise if unsafe { IsZoomed(hwnd) } != 0 => SC_RESTORE,
        Button::Maximise => SC_MAXIMIZE,
        Button::Close => SC_CLOSE,
    };
    // Posted rather than sent: this is the middle of a message, and the
    // command opens a modal loop of its own while the window animates.
    //
    // Safe: the window is alive.
    unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, command as WPARAM, 0) };
}

/// The window's messages, heard before winit hears them.
unsafe extern "system" fn listen(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass: usize,
    data: usize,
) -> LRESULT {
    // Safe throughout: the system calls this on the window's own thread
    // with the window alive and the pointer `drawn` gave it, which stays
    // good until `WM_NCDESTROY` takes it back.
    let passed = || unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    let shared = unsafe { &*(data as *const Shared) };
    match message {
        // The frame, with the system's answer taken for every side but the
        // top: there the page goes up to the window's own edge.
        WM_NCCALCSIZE if wparam != 0 && captioned(hwnd) => {
            let params = lparam as *mut NCCALCSIZE_PARAMS;
            let RECT { left, top, .. } = unsafe { (*params).rgrc[0] };
            let answered = passed();
            if answered != 0 {
                return answered;
            }
            let rect = unsafe { &mut (*params).rgrc[0] };
            // A maximised window is placed with its frame hanging off the
            // screen on every side, so the top has to come in by as much
            // as the side did, or the bar's top row is off the screen.
            rect.top = match unsafe { IsZoomed(hwnd) } != 0 {
                true => top + (rect.left - left),
                false => top,
            };
            0
        }
        // What a place on the window is. The system's answer stands
        // everywhere it is not the page -- the sides and the bottom, which
        // are its frame -- and where it is, Obelus says which of it is the
        // bar.
        WM_NCHITTEST => {
            let answered = passed();
            if answered != HTCLIENT as LRESULT || !captioned(hwnd) {
                return answered;
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "two signed words, which is what the system packed in it"
            )]
            let mut at = POINT {
                x: i32::from(lparam as i16),
                y: i32::from((lparam >> 16) as i16),
            };
            let mut page = RECT::default();
            unsafe {
                ScreenToClient(hwnd, &raw mut at);
                GetClientRect(hwnd, &raw mut page);
            }
            let measured = measured(hwnd);
            #[expect(clippy::cast_precision_loss, reason = "pixels on a screen")]
            let part = super::part(
                (at.x as f32, at.y as f32),
                page.right as f32,
                measured.titled as f32,
                measured.button as f32,
                measured.edge as f32,
                unsafe { IsZoomed(hwnd) } != 0,
            );
            let answer = match part {
                Part::Page => HTCLIENT,
                Part::Edge => HTTOP,
                Part::Bar => HTCAPTION,
                // By name, which is what has Windows 11 open its layouts
                // over the maximise button.
                Part::Button(Button::Minimise) => HTMINBUTTON,
                Part::Button(Button::Maximise) => HTMAXBUTTON,
                Part::Button(Button::Close) => HTCLOSE,
            };
            answer as LRESULT
        }
        // The pointer over the bar, which is off the page and so never
        // reaches winit as a pointer at all. Followed until it leaves, so
        // a button is not left lit behind a pointer gone elsewhere.
        WM_NCMOUSEMOVE => {
            hold(hwnd, &shared.hovered, hit(wparam));
            let mut leaving = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&raw mut leaving) };
            passed()
        }
        // Off the bar, onto the page or out of the window. A press is let
        // go of with it: the button's release is somewhere else.
        WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
            hold(hwnd, &shared.hovered, None);
            hold(hwnd, &shared.pressed, None);
            passed()
        }
        // A button, pressed. Kept from the system, which would draw the
        // buttons of a window from before DWM over the page -- and which
        // does the button's work on the press, where a button does it on
        // the release, over the button it went down on.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if hit(wparam).is_some() => {
            hold(hwnd, &shared.pressed, hit(wparam));
            0
        }
        WM_NCLBUTTONUP if hit(wparam).is_some() => {
            let pressed = held(shared.pressed.load(Ordering::Relaxed));
            hold(hwnd, &shared.pressed, None);
            if let Some(button) = hit(wparam).filter(|&button| pressed == Some(button)) {
                press(hwnd, button);
            }
            0
        }
        // The window going, and the share of `Shared` it held with it.
        WM_NCDESTROY => {
            unsafe {
                RemoveWindowSubclass(hwnd, Some(listen), LISTENING);
                drop(Arc::from_raw(data as *const Shared));
            }
            passed()
        }
        _ => passed(),
    }
}
