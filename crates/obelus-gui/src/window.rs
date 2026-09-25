//! The window, and the thread Obelus runs on behind it.
//!
//! Two loops, and which is on which thread is not a choice. A platform's
//! event loop has to own the process's first thread -- macOS refuses
//! outright, and Windows delivers messages to the thread that made the
//! window -- so the window's loop is `main`'s, and Obelus's own loop gets a
//! thread of its own. Which suits it: that loop is a thread blocked on a
//! channel and has been from the beginning, and everything it waits on is
//! already something on another thread sending to it. The window is simply
//! one more sender.
//!
//! So `App` crosses a thread on the way up and stays there. Nothing else
//! does: the frames it draws go one way down a channel, the presses go the
//! other way down another, and neither side ever holds the other's state.
//!
//! What a send needs and a terminal's did not is a wake. A thread parked in
//! `recv` wakes because something was sent to it; a thread parked in the
//! platform's own wait does not, and the one way to reach it is to post an
//! event to the loop it is parked in. That is what the proxy is for, and it
//! is why the backend holds one.

use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
};

use anyhow::{Context, Result};
use obelus_app::{
    app::{self, App},
    event::{Event, Pointer},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::{
    font::Fonts,
    grid::{Cells, Measured, Page, Update},
    keys,
};

/// How big the text is, in points.
///
/// One number, until there is a setting for it: what it will be is a line
/// in the settings page like every other preference, and what it must not
/// be is a command.
const SIZE: f32 = 14.0;

/// How many rows a notch of the wheel moves, which is what every terminal
/// sends and so what the rest of Obelus already expects.
const NOTCH: isize = 3;

/// What the window's loop is woken for.
#[derive(Clone, Copy, Debug)]
enum Waking {
    /// Obelus drew a frame.
    Frame,
    /// Obelus's loop has ended, which is the window's reason to exist gone.
    Finished,
}

/// Runs Obelus in a window until the reader leaves.
pub(crate) fn show(app: App) -> Result<()> {
    let events = EventLoop::<Waking>::with_user_event()
        .build()
        .context("no window system to open a window on")?;
    let mut showing = Showing::new(app, events.create_proxy());
    events.run_app(&mut showing).context("the window stopped")?;
    showing.outcome()
}

/// Everything the window has, and Obelus on the other side of it.
struct Showing {
    /// Held until there is a window to run it against, which is the first
    /// moment the number of columns is known.
    starting: Option<App>,
    window: Option<Arc<Window>>,
    painter: Option<crate::paint::Painter>,
    fonts: Option<Fonts>,
    page: Page,
    /// The frames Obelus has drawn, waiting to be applied.
    frames: Option<Receiver<Update>>,
    /// And what the reader did, on its way to Obelus.
    doing: Option<Sender<Event>>,
    measured: Arc<Measured>,
    proxy: EventLoopProxy<Waking>,
    /// What Obelus's loop returned, once it has.
    outcome: Arc<Mutex<Option<Result<()>>>>,
    modifiers: ModifiersState,
    /// Which cell the pointer is in, so that moving within one cell is not
    /// news: the rest of Obelus counts the pointer in cells, and a window
    /// reports it in pixels.
    pointer: Option<(u16, u16)>,
    held: bool,
    /// Wheels that report pixels rather than lines, added up until they
    /// amount to a row.
    rolled: f32,
    /// Whether an input method is in the middle of a word.
    ///
    /// While it is, the keys belong to it: a reader typing `n` `i` `h` `a`
    /// `o` is spelling one character, and passing those presses on as well
    /// would put the spelling in the file and the word after it.
    composing: bool,
}

impl Showing {
    fn new(app: App, proxy: EventLoopProxy<Waking>) -> Self {
        Self {
            starting: Some(app),
            window: None,
            painter: None,
            fonts: None,
            page: Page::default(),
            frames: None,
            doing: None,
            measured: Arc::new(Measured::default()),
            proxy,
            outcome: Arc::new(Mutex::new(None)),
            modifiers: ModifiersState::empty(),
            pointer: None,
            held: false,
            rolled: 0.0,
            composing: false,
        }
    }

    /// What Obelus's loop returned, or that it never got to run.
    fn outcome(&mut self) -> Result<()> {
        self.outcome
            .lock()
            .ok()
            .and_then(|mut outcome| outcome.take())
            .unwrap_or(Ok(()))
    }

    /// Tells Obelus what the reader did.
    ///
    /// A send that fails is Obelus's loop having ended, which the window
    /// hears about by its own route: nothing to do here but let the press
    /// go.
    fn tell(&self, event: Event) {
        if let Some(doing) = &self.doing {
            let _ = doing.send(event);
        }
    }

    /// Works out how many columns and rows the window now holds, and tells
    /// both sides.
    fn remeasure(&mut self) {
        let (Some(window), Some(fonts)) = (self.window.as_ref(), self.fonts.as_ref()) else {
            return;
        };
        let size = window.inner_size();
        let cell = fonts.cell();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (width, height) = (size.width as f32, size.height as f32);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a division of two positive pixel counts, floored"
        )]
        let columns = ((width / cell.width) as u32).clamp(1, u32::from(u16::MAX)) as u16;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a division of two positive pixel counts, floored"
        )]
        let rows = ((height / cell.height) as u32).clamp(1, u32::from(u16::MAX)) as u16;
        self.measured
            .resized(columns, rows, size.width, size.height);
        self.page.resized(columns, rows);
    }

    /// Tells the input method where the caret is, so that its candidates
    /// are drawn beside the word being typed rather than in the corner of
    /// the screen.
    fn point_the_input_method(&self) {
        let (Some(window), Some(fonts), Some(caret)) =
            (self.window.as_ref(), self.fonts.as_ref(), self.page.caret())
        else {
            return;
        };
        let cell = fonts.cell();
        window.set_ime_cursor_area(
            PhysicalPosition::new(
                f32::from(caret.x) * cell.width,
                f32::from(caret.y) * cell.height,
            ),
            PhysicalSize::new(cell.width, cell.height),
        );
    }

    /// Which cell a place in the window is in.
    fn cell_at(&self, at: PhysicalPosition<f64>) -> Option<(u16, u16)> {
        let fonts = self.fonts.as_ref()?;
        let cell = fonts.cell();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a place inside the window, divided by a cell"
        )]
        let column = ((at.x.max(0.0) as f32 / cell.width) as u32).min(u32::from(u16::MAX)) as u16;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a place inside the window, divided by a cell"
        )]
        let row = ((at.y.max(0.0) as f32 / cell.height) as u32).min(u32::from(u16::MAX)) as u16;
        Some((
            column.min(self.page.columns().saturating_sub(1)),
            row.min(self.page.rows().saturating_sub(1)),
        ))
    }
}

impl ApplicationHandler<Waking> for Showing {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        // Once. A platform that suspends and resumes says so again, and the
        // window it is about is the one already here.
        let Some(mut app) = self.starting.take() else {
            return;
        };
        let attributes = named(
            Window::default_attributes()
                .with_title("Obelus")
                .with_inner_size(LogicalSize::new(1100.0, 720.0)),
        );
        let window = match events.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                tracing::error!(%error, "no window");
                events.exit();
                return;
            }
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a scale factor is a small number"
        )]
        let fonts = Fonts::new(SIZE * window.scale_factor() as f32);
        let painter = match crate::paint::Painter::new(Arc::clone(&window)) {
            Ok(painter) => painter,
            Err(error) => {
                tracing::error!(?error, "nothing to draw with");
                events.exit();
                return;
            }
        };
        // Without this a window gets keys and nothing else, and there is
        // no way to type any language that is spelled before it is written.
        window.set_ime_allowed(true);
        self.window = Some(Arc::clone(&window));
        self.fonts = Some(fonts);
        self.painter = Some(painter);
        self.remeasure();

        let (frames, drawn) = std::sync::mpsc::channel();
        let (doing, done) = std::sync::mpsc::channel();
        self.frames = Some(drawn);
        self.doing = Some(doing.clone());

        let proxy = self.proxy.clone();
        let wake = Arc::new(move || {
            let _ = proxy.send_event(Waking::Frame);
        });
        let cells = Cells::new(frames, wake, Arc::clone(&self.measured));
        // The one place Obelus is told where its events go, which is what
        // starts the watcher, the servers and the walk of the project.
        app.start(doing);

        let proxy = self.proxy.clone();
        let outcome = Arc::clone(&self.outcome);
        let started = std::thread::Builder::new()
            .name("obelus".to_string())
            .spawn(move || {
                // Whatever happens to the loop -- returning, or panicking
                // on the way -- the window is told, or it waits for a frame
                // that is never coming.
                let _finished = Finished(proxy);
                let mut terminal = match ratatui::Terminal::new(cells) {
                    Ok(terminal) => terminal,
                    Err(error) => {
                        *outcome.lock().expect("the outcome") =
                            Some(Err(error).context("no screen to draw on"));
                        return;
                    }
                };
                let ran = app::run(&mut terminal, &mut app, done);
                *outcome.lock().expect("the outcome") = Some(ran);
            });
        if let Err(error) = started {
            tracing::error!(%error, "Obelus could not be started behind the window");
            events.exit();
        }
    }

    fn user_event(&mut self, events: &ActiveEventLoop, waking: Waking) {
        match waking {
            Waking::Frame => {
                let Some(frames) = self.frames.as_ref() else {
                    return;
                };
                // Everything that has arrived, not one frame: several
                // frames may be waiting -- a key and the answer it
                // provoked -- and drawing the older ones would be drawing
                // screens the reader is never meant to see.
                let mut drew = false;
                while let Ok(update) = frames.try_recv() {
                    drew |= self.page.apply(update);
                }
                if drew {
                    self.point_the_input_method();
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            Waking::Finished => events.exit(),
        }
    }

    fn window_event(&mut self, events: &ActiveEventLoop, _window: WindowId, event: WindowEvent) {
        match event {
            // The close button means what the key that leaves means, asking
            // about unwritten files included. So the window does not close
            // here: Obelus is asked, and the window goes when Obelus says
            // it is done.
            WindowEvent::CloseRequested => self.tell(Event::Closed),
            WindowEvent::RedrawRequested => {
                let (Some(painter), Some(fonts)) = (self.painter.as_mut(), self.fonts.as_mut())
                else {
                    return;
                };
                if let Err(error) = painter.paint(&self.page, fonts) {
                    tracing::error!(?error, "the frame was not drawn");
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(painter) = self.painter.as_mut() {
                    painter.resized(size.width, size.height);
                }
                self.remeasure();
                self.tell(Event::Resize);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a scale factor is a small number"
                )]
                if let Some(fonts) = self.fonts.as_mut() {
                    fonts.resize(SIZE * scale_factor as f32);
                }
                if let Some(painter) = self.painter.as_mut() {
                    painter.forget_the_glyphs();
                }
                self.remeasure();
                self.tell(Event::Resize);
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Ime(ime) => match ime {
                // The spelling so far, which the input method draws itself
                // in a window of its own beside the caret. What Obelus does
                // with it is know that the keys are spoken for.
                Ime::Preedit(spelling, _) => self.composing = !spelling.is_empty(),
                // The word. It goes in the way pasted text goes in --
                // wherever the reader is writing, as one change -- because
                // that is the same question and it is already answered.
                Ime::Commit(word) => {
                    self.composing = false;
                    self.tell(Event::Paste(word));
                }
                Ime::Enabled | Ime::Disabled => self.composing = false,
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                let Some(key) = keys::pressed(
                    &event.logical_key,
                    event.physical_key,
                    self.modifiers,
                    event.repeat,
                ) else {
                    return;
                };
                // A plain character while an input method is spelling a
                // word is that spelling, and it arrives again as the word.
                // A chord is not: `ctrl+s` means save whatever is being
                // typed.
                let plain = self.modifiers.is_empty()
                    || self.modifiers == winit::keyboard::ModifiersState::SHIFT;
                if self.composing && plain {
                    return;
                }
                self.tell(Event::Key(key));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let Some(at) = self.cell_at(position) else {
                    return;
                };
                if self.pointer == Some(at) {
                    return;
                }
                self.pointer = Some(at);
                self.tell(Event::Pointer {
                    kind: match self.held {
                        true => Pointer::Dragged,
                        false => Pointer::Moved,
                    },
                    x: at.0,
                    y: at.1,
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // The left button only, the same as in a terminal: the
                // others belong to the window system, and taking them would
                // be taking away the menu the reader expects.
                if button != MouseButton::Left {
                    return;
                }
                self.held = state == ElementState::Pressed;
                let Some((x, y)) = self.pointer else {
                    return;
                };
                self.tell(Event::Pointer {
                    kind: match state {
                        ElementState::Pressed => Pointer::Pressed,
                        ElementState::Released => Pointer::Released,
                    },
                    x,
                    y,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let rows = match delta {
                    // A notch, which is what a mouse sends.
                    MouseScrollDelta::LineDelta(_, lines) => {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a wheel turns by lines, not by millions of them"
                        )]
                        let rows = (-lines * NOTCH as f32) as isize;
                        rows
                    }
                    // Pixels, which is what a touchpad sends: kept until
                    // they amount to a row, or a slow drag scrolls nothing
                    // at all.
                    MouseScrollDelta::PixelDelta(moved) => {
                        let height = self.fonts.as_ref().map_or(1.0, |fonts| fonts.cell().height);
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "pixels under a finger, divided by a row"
                        )]
                        let scrolled = self.rolled - moved.y as f32;
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "pixels under a finger, divided by a row"
                        )]
                        let rows = (scrolled / height) as isize;
                        #[expect(clippy::cast_precision_loss, reason = "a handful of rows")]
                        let taken = rows as f32 * height;
                        self.rolled = scrolled - taken;
                        rows
                    }
                };
                if rows != 0 {
                    self.tell(Event::Scroll(rows));
                }
            }
            WindowEvent::Destroyed => events.exit(),
            _ => {}
        }
    }
}

/// Says that Obelus's loop is over, however it ended.
///
/// A guard rather than a line at the end of the thread: a panic in the loop
/// unwinds past that line, and a window left waiting for a frame that is
/// never coming is a window that cannot even be closed.
struct Finished(EventLoopProxy<Waking>);

impl Drop for Finished {
    fn drop(&mut self) {
        let _ = self.0.send_event(Waking::Finished);
    }
}

/// What this window is called by the thing that manages windows.
///
/// Not the title, which is what the reader sees: this is the name a
/// compositor matches its own rules against, and a window that does not
/// give one is a window nobody can write a rule for -- no size to open at,
/// no workspace to go to, no icon. It is also how a taskbar finds the
/// desktop entry.
///
/// Lowercase, like every other thing named after Obelus rather than being
/// the name: the binary, the crates, the config directory.
#[cfg(all(unix, not(target_os = "macos")))]
fn named(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11};

    // Both, because which one is in force is the session's business and
    // neither call is an error on the other. Named by trait, because the
    // two of them each have a `with_name` and the compiler cannot know
    // which session this will be.
    let attributes = WindowAttributesExtX11::with_name(attributes, "obelus", "obelus");
    WindowAttributesExtWayland::with_name(attributes, "obelus", "obelus")
}

/// Windows and macOS name an application, not a window, and both are told
/// by the bundle or the executable rather than here.
#[cfg(not(all(unix, not(target_os = "macos"))))]
fn named(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attributes
}
