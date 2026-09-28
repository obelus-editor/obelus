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

use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::Instant,
};

use anyhow::{Context, Result};
use obelus_app::{
    app::{self, App},
    event::{Event, Pointer},
};
use obelus_ui::shapes::Bar;
use ratatui::layout::Rect;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::{
    blink::Blink,
    font::Fonts,
    grid::{
        Behind, Capped, Cells, Marked, Marking, Measured, Page, Said, Spelling, Ticked, Update,
    },
    keys,
    motion::{Motion, Wake},
};

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
    /// What an input method is spelling, and where its own caret is in it.
    ///
    /// The window's, not the application's: the word is not in the file
    /// yet, and telling the application about a word that may never be
    /// committed would be putting somebody's half-typed pinyin into a
    /// buffer with an undo history.
    spelling: Option<Spelling>,
    /// Where the marks go on the frame being shown.
    marked: Vec<Marked>,
    /// And where its key caps are, kept the same way and for the same
    /// reason.
    capped: Vec<Capped>,
    /// The ones the frame being laid out has asked for so far.
    capping: Vec<Capped>,
    /// Which cells are switches on the frame being shown.
    ticked: Vec<Ticked>,
    /// And on the one being laid out.
    ticking: Vec<Ticked>,
    /// Which band of rows is a list, how far down it the band has got and
    /// what bar says so, on the frame being shown.
    scrolled: Option<(Rect, i64)>,
    /// And on the frame being laid out.
    scrolling: Option<(Rect, i64)>,
    /// The bar beside the band being shown.
    bar: Option<Bar>,
    /// Which row its mark was on when the slide under way began.
    ///
    /// The slide's other end, and the reason it is kept rather than
    /// worked out: both ends have to be rows the bar was *drawn* at, or
    /// the mark sets out from somewhere it was never drawn and steps
    /// back to catch up.
    bar_origin: Option<u16>,
    /// And the one being laid out.
    barring: Option<Bar>,
    /// The page as it was before the frame being shown.
    ///
    /// Kept only while there is a band that could move, and for one
    /// reason: the rows a list has scrolled past are not on the new page
    /// at all, and a band catching up has to draw them. History, which is
    /// what it is for -- not a stand-in for anything that is still going
    /// on.
    before: Option<Page>,
    /// What is under the pane on the frame being shown, where there is
    /// one.
    behind: Option<Behind>,
    /// And on the frame being laid out.
    behinding: Option<Behind>,
    /// And the ones the frame being laid out has asked for so far.
    ///
    /// Two lists because a frame is drawn from what it said, not from what
    /// the next one is saying: a screen part way through being described
    /// would be marks from two screens at once.
    marking: Vec<Marked>,
    /// What the window is animating, and when it wants waking for it.
    ///
    /// The window's own, and nothing the application is ever told about:
    /// see [`crate::motion`].
    motion: Motion,
    /// How big the text is, in points, as the settings last said.
    ///
    /// Kept because the two things it is measured against move on their
    /// own: the screen's scale changes when the window is dragged to
    /// another monitor, and the setting changes when the reader -- or
    /// another Obelus -- says so.
    points: f32,
}

impl Showing {
    fn new(app: App, proxy: EventLoopProxy<Waking>) -> Self {
        // Read before the application is put on its own thread, because
        // this is what the first window is built at: afterwards the size
        // arrives down the frames channel like every other change to what
        // is drawn.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a size in points, which is two digits"
        )]
        let points = app.config().font_size as f32;
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
            spelling: None,
            marked: Vec::new(),
            marking: Vec::new(),
            capped: Vec::new(),
            capping: Vec::new(),
            ticked: Vec::new(),
            ticking: Vec::new(),
            scrolled: None,
            scrolling: None,
            bar: None,
            barring: None,
            bar_origin: None,
            before: None,
            behind: None,
            behinding: None,
            // The blink is asked once, on the way up: it is a question
            // about the system rather than about this window.
            motion: Motion::new(Blink::asked()),
            points,
        }
    }

    /// The text is a different size: either the reader said so, or the
    /// window moved to a screen of another density.
    ///
    /// Everything about the grid follows from the size of a cell, so all of
    /// it is done again: the faces measure themselves, the glyphs already
    /// drawn are the wrong size, the number of columns has changed, and the
    /// application has to be told so that it lays the next frame out to fit.
    fn redraw_at(&mut self, points: f32) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        self.points = points;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a scale factor is a small number"
        )]
        let pixels = points * window.scale_factor() as f32;
        if let Some(fonts) = self.fonts.as_mut() {
            fonts.resize(pixels);
        }
        if let Some(painter) = self.painter.as_mut() {
            painter.forget_the_glyphs();
        }
        self.remeasure();
        self.tell(Event::Resize);
    }

    /// What Obelus's loop returned, or that it never got to run.
    fn outcome(&mut self) -> Result<()> {
        self.outcome
            .lock()
            .ok()
            .and_then(|mut outcome| outcome.take())
            .unwrap_or(Ok(()))
    }

    /// The reader did something, so the caret is solid again and the
    /// blinking starts over.
    ///
    /// Which is what every caret does: one that went on blinking through a
    /// paragraph being typed would flicker under the reader's hands, and
    /// one that stayed dark for the half cycle it was in the middle of
    /// would be a keypress with no caret after it.
    fn stir(&mut self) {
        if self.motion.stirred(Instant::now()) {
            self.redraw();
        }
    }

    /// Draws again, for something the window knows and the application
    /// does not.
    ///
    /// What is being spelled is the only such thing: every other change on
    /// screen comes from a frame, and a frame asks for its own redraw.
    fn redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
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
        // Past what is being spelled, so the candidates sit under the end
        // of the word rather than under the character it started at.
        let along = self.spelling.as_ref().map_or(0, Spelling::columns);
        window.set_ime_cursor_area(
            PhysicalPosition::new(
                (f32::from(caret.x) + along as f32) * cell.width,
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
        let attributes = marked(named(
            Window::default_attributes()
                .with_title("Obelus")
                .with_inner_size(LogicalSize::new(1100.0, 720.0)),
        ));
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
        let fonts = Fonts::new(self.points * window.scale_factor() as f32);
        let painter = match crate::paint::Painter::new(Arc::clone(&window)) {
            Ok(painter) => painter,
            Err(error) => {
                tracing::error!(?error, "nothing to draw with");
                events.exit();
                return;
            }
        };
        // Once there is a window, which is the first moment there is a
        // display connection to adopt.
        crate::clipboard::take(events);
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

        // What this machine can draw with, for the list a reader builds
        // their own out of. Said once, from here, because the font
        // database is loaded and nothing else in Obelus can see it.
        if let Some(fonts) = self.fonts.as_ref() {
            let here = fonts.here();
            tracing::info!(faces = here.len(), "the faces this machine has");
            let _ = doing.send(Event::Fonts {
                here,
                otherwise: fonts.otherwise().map(str::to_string),
            });
        }

        let proxy = self.proxy.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.send_event(Waking::Frame);
        });
        let cells = Cells::new(
            frames.clone(),
            Arc::clone(&wake),
            Arc::clone(&self.measured),
        );
        // And the other direction: what the settings say about the window,
        // which the application sends when they change and nobody else can
        // answer.
        app.drawn_by(Arc::new(crate::grid::Telling::new(
            frames.clone(),
            Arc::clone(&wake),
        )));
        // And the marks, which a window draws itself. A terminal is asked
        // what it can show and mostly cannot; there is nothing to ask
        // here, because the pixels are the window's own.
        let marking = Marking::new(frames);
        // And what a run of cells *is*, which a window can draw the shape
        // of: said once, because who is drawing is a fact about the
        // process rather than about a frame.
        obelus_ui::shapes::drawn_by(Arc::new(marking.clone()));
        app.use_images(obelus_ui::image::Images::drawn_by(Arc::new(marking)));
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
                let was = self.page.caret();
                let whose_was = self.page.whose();
                let had_a_pane = self.behind.is_some();
                let was_at = self.scrolled;
                // Kept only where a band could move, because that is the
                // only thing it is for.
                let before = was_at.map(|_| self.page.clone());
                let bar_was = self.bar.map(|bar| bar.mark);
                let mut drew = false;
                let mut sized = None;
                let mut faces = None;
                while let Ok(update) = frames.try_recv() {
                    match update {
                        // Not cells, so the page never sees them.
                        Update::TextSize(points) => sized = Some(points),
                        Update::Animates(on) => self.motion.animates(on),
                        Update::Fonts(names) => faces = Some(names),
                        Update::Mark {
                            id,
                            focused,
                            svg,
                            palette,
                        } => {
                            if let Some(painter) = self.painter.as_mut() {
                                painter.carries(id, focused, svg, palette);
                            }
                        }
                        Update::Marked { id, focused, x, y } => {
                            self.marking.push(Marked { id, focused, x, y })
                        }
                        // The last one said wins, which is the pane
                        // nearest the reader: a setting's choices open
                        // over the settings, and what the reader sees
                        // through is the one on top.
                        Update::Behind {
                            area,
                            joined,
                            ground,
                            cells,
                        } => {
                            self.behinding = Some(Behind {
                                area,
                                joined,
                                ground,
                                cells,
                            });
                        }
                        Update::Scrolled { area, top, bar } => {
                            self.scrolling = Some((area, top));
                            self.barring = bar;
                        }
                        Update::Ticked { area, on } => {
                            self.ticking.push(Ticked { area, on });
                        }
                        Update::Capped {
                            keys,
                            area,
                            cap,
                            page,
                            edge,
                        } => self.capping.push(Capped {
                            keys,
                            area,
                            cap,
                            page,
                            edge,
                        }),
                        Update::Frame => {
                            // The frame is over: what it asked for is what
                            // is on screen until the next one says
                            // otherwise.
                            self.marked = std::mem::take(&mut self.marking);
                            self.capped = std::mem::take(&mut self.capping);
                            self.ticked = std::mem::take(&mut self.ticking);
                            self.behind = self.behinding.take();
                            self.scrolled = self.scrolling.take();
                            self.bar = self.barring.take();
                            drew = true;
                        }
                        cells => drew |= self.page.apply(cells),
                    }
                }
                // Where the caret is is the page's; whether it walked
                // there or simply appeared is the window's own question,
                // and it is asked of the drain as a whole rather than of
                // each update in it. Several frames may have been waiting
                // -- a key and the answer it provoked -- and what the
                // reader is owed is the walk from where they last saw the
                // caret to where it is now, not one per frame they never
                // saw.
                self.motion
                    // Nothing where the caret changed hands: a list
                    // opening over the page puts the caret in its own
                    // box, which is a different caret at a different
                    // place. Walked from one to the other, it goes down
                    // the screen from where the reader was reading to
                    // where the list came up.
                    .caret_moved(
                        was.filter(|_| whose_was == self.page.whose()),
                        self.page.caret(),
                        Instant::now(),
                    );
                // A pane opening is the one thing on the screen that
                // arrives rather than changes, and it is worked out from
                // what is there rather than announced: a frame with a pane
                // on it that the one before it had none is a pane opening,
                // and there is nowhere else that can be true.
                match (had_a_pane, self.behind.is_some()) {
                    (false, true) => self.motion.pane_opened(Instant::now()),
                    (true, false) => self.motion.pane_shut(),
                    _ => {}
                }
                // A band that moved is the same band showing a different
                // part of its list. The same band, because a list that was
                // replaced by another one in the same place has not
                // scrolled -- it has been swapped, and sliding between two
                // unrelated lists would say they were one.
                // And no further than the band is tall. A list that
                // moved by more than a screenful did not scroll -- it went
                // somewhere else, and sliding the distance would be a
                // second of rows nobody is reading. It is also the one
                // bound that makes the drawing possible: the rows it left
                // behind exist on one page, and that page is a screenful.
                if let (Some((room, before_top)), Some((now_room, top))) = (was_at, self.scrolled)
                    && room == now_room
                    && top != before_top
                    && top.abs_diff(before_top) <= u64::from(room.height)
                {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a list is rows, and a scroll is a few of them"
                    )]
                    let rows = (top - before_top) as f32;
                    // Only where this begins a fresh slide: one already
                    // under way keeps the page it started with, because
                    // that is the page the rows it scrolled past are on
                    // and the one the distance is counted from.
                    if self
                        .motion
                        .band_moved(rows, f32::from(room.height), Instant::now())
                    {
                        self.before = before;
                        self.bar_origin = bar_was;
                    }
                }
                if let Some(names) = faces {
                    // Which faces text is drawn in decides how wide a cell
                    // is, so this is the same work a new size is: measure
                    // again, throw away the glyphs, and tell the
                    // application what the grid has become.
                    if let Some(fonts) = self.fonts.as_mut() {
                        fonts.use_families(&names);
                    }
                    if let Some(painter) = self.painter.as_mut() {
                        painter.forget_the_glyphs();
                    }
                    self.remeasure();
                    self.tell(Event::Resize);
                    self.redraw();
                }
                if let Some(points) = sized {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a size in points, which is two digits"
                    )]
                    let points = points as f32;
                    // Said on the way up as well as on a change, so this is
                    // usually the size it already is.
                    if (points - self.points).abs() > f32::EPSILON {
                        self.redraw_at(points);
                    }
                }
                if drew {
                    self.point_the_input_method();
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            Waking::Finished => {
                // Before the window goes, because what Obelus is offering
                // is offered *by* this process: the selection belongs to a
                // live client, and in a moment there will not be one.
                obelus_clipboard::hand_over();
                events.exit();
            }
        }
    }

    /// The loop is over, whichever way it got here.
    ///
    /// Every `exit()` above arrives here, which is why what has to happen
    /// once goes here rather than beside each of them: a way out added
    /// later is one nobody has to remember to tell. And it is the last
    /// moment the display is open, which is what the clipboard needs --
    /// see `clipboard::let_go`.
    fn exiting(&mut self, _events: &ActiveEventLoop) {
        crate::clipboard::let_go();
    }

    /// When to wake next, which is whatever the window is animating.
    ///
    /// Obelus's own loop is a thread blocked on a channel and wakes this
    /// one when it has drawn something; everything the window does on a
    /// clock of its own is in [`crate::motion`]. So the deadline is worked
    /// out from what is true -- what is moving, and whether there is a
    /// caret for any of it to be about -- rather than switched on and off
    /// from the places that change any of that. The same rule the ticker
    /// follows, in the crate that has no ticker.
    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        let now = Instant::now();
        let caret = self.page.caret().is_some();
        if self.motion.advance(now, caret) {
            self.redraw();
        }
        events.set_control_flow(match self.motion.wake(now, caret) {
            Some(Wake::At(when)) => ControlFlow::WaitUntil(when),
            // Something is in flight, so the next frame is wanted as soon
            // as the screen will take one. What paces it is the surface
            // itself, which is presented on the vertical blank: that is
            // the rate an animation is meant to run at, and the one
            // number nobody here has to pick.
            Some(Wake::EveryFrame) => ControlFlow::Poll,
            None => ControlFlow::Wait,
        });
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
                if let Err(error) = painter.paint(
                    &self.page,
                    fonts,
                    self.spelling.as_ref(),
                    self.motion.moving(Instant::now()),
                    Said {
                        marked: &self.marked,
                        capped: &self.capped,
                        ticked: &self.ticked,
                        behind: self.behind.as_ref(),
                        band: self
                            .scrolled
                            .zip(self.before.as_ref())
                            .map(|((room, _), before)| (room, before)),
                        bar: self.bar.map(|bar| {
                            let origin = self.bar_origin.unwrap_or(bar.mark);
                            (bar.area, f32::from(origin) - f32::from(bar.mark))
                        }),
                    },
                ) {
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
            // The window is on a screen of another density, so a point is
            // a different number of pixels. Which is the same work as the
            // reader choosing another size, and the window asks its own
            // scale on the way through.
            WindowEvent::ScaleFactorChanged { .. } => self.redraw_at(self.points),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Ime(ime) => {
                self.stir();
                match ime {
                    // The spelling so far, drawn in the file at the place the
                    // word will go. An input method draws a window of its own
                    // for the candidates and Obelus leaves that alone; what it
                    // will not leave alone is *where the word is being typed*,
                    // which belongs on the page with the code it is going into.
                    Ime::Preedit(spelling, caret) => {
                        self.composing = !spelling.is_empty();
                        self.spelling = (!spelling.is_empty()).then(|| Spelling {
                            // Where the input method's own caret is, in
                            // characters rather than in bytes: the window
                            // counts cells, and a byte offset into pinyin is
                            // not one.
                            caret: caret.map_or_else(
                                || spelling.chars().count(),
                                |(start, _)| spelling[..start].chars().count(),
                            ),
                            text: spelling,
                        });
                        self.redraw();
                    }
                    // The word. It goes in the way pasted text goes in --
                    // wherever the reader is writing, as one change -- because
                    // that is the same question and it is already answered.
                    Ime::Commit(word) => {
                        self.composing = false;
                        self.spelling = None;
                        self.tell(Event::Paste(word));
                    }
                    Ime::Enabled | Ime::Disabled => {
                        self.composing = false;
                        self.spelling = None;
                        self.redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                self.stir();
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
                self.stir();
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

/// The icon, as the file itself. Fourteen kilobytes, seven sizes, and the
/// decoder hands back the largest of them.
const MARK: &[u8] = include_bytes!("../../../contrib/desktop/obelus.ico");

/// The mark the window wears.
///
/// X11 is who needs it. A Wayland compositor takes the icon from the
/// desktop entry the name above points it at, and macOS from the bundle,
/// so on those two this is dropped; Windows does take it, and gets the
/// same picture it would have taken out of the executable's own resources
/// anyway, because both are built from the one file below.
///
/// Set on all of them rather than behind a `cfg` for each: a platform that
/// does not want it drops it, and three cfgs would be three places to be
/// wrong about somebody else's rules. Read from the file rather than
/// written out as pixels beside it, because an icon is an icon in one
/// place.
///
/// A window with no icon is a window, so nothing here is fatal.
fn marked(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    let icon = match image::load_from_memory_with_format(MARK, image::ImageFormat::Ico) {
        Ok(mark) => {
            let mark = mark.to_rgba8();
            let (width, height) = mark.dimensions();
            match winit::window::Icon::from_rgba(mark.into_raw(), width, height) {
                Ok(icon) => Some(icon),
                Err(error) => {
                    tracing::warn!(%error, "the window has no icon");
                    None
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "the window has no icon");
            None
        }
    };
    attributes.with_window_icon(icon)
}

#[cfg(test)]
mod tests {
    use super::MARK;

    /// What the window draws and what Windows draws are one file, and this
    /// is the half of that a test can hold: that the file is one an ICO
    /// decoder makes a picture of, at the size the largest entry claims.
    ///
    /// `include_bytes!` is happy with any bytes at all, so nothing else
    /// here would notice the file going wrong -- `marked` answers a warning
    /// in the log and a window with no icon on it. Broken by truncating
    /// `contrib/desktop/obelus.ico`, which fails this and nothing else.
    #[test]
    fn the_window_can_read_its_own_icon() {
        let mark = image::load_from_memory_with_format(MARK, image::ImageFormat::Ico)
            .expect("the icon decodes")
            .to_rgba8();
        assert_eq!(mark.dimensions(), (256, 256));
    }
}
