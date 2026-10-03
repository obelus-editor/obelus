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
//! `app::run` is handed the *receiving* end of the loop's channel because
//! the other end belongs to whichever front end is running: a terminal
//! reads keys on a thread of its own, and a window gets them from the event
//! loop the platform obliges it to run on the process's first thread. So
//! `App` crosses a thread on the way up and stays there -- one owner of
//! `&mut App`, as before, just not on `main`'s thread. Nothing else
//! does: the frames it draws go one way down a channel, the presses go the
//! other way down another, and neither side ever holds the other's state.
//!
//! What a send needs and a terminal's did not is a wake. A thread parked in
//! `recv` wakes because something was sent to it; a thread parked in the
//! platform's own wait does not, and the one way to reach it is to post an
//! event to the loop it is parked in. That is what the proxy is for, and it
//! is why the backend holds one.
//!
//! **What an input method is spelling is on the page, and is not in the
//! file.** Typing Chinese is spelling a word before it exists: several
//! keypresses that are not characters, and then one character that is. The
//! window draws that spelling itself, over the cells to the right of the
//! caret, underlined, in the colours of the place it is going into -- and
//! the application is never told about it. Not because it could not be:
//! because what would arrive is half-typed pinyin in a buffer with an undo
//! history and a file that has changed. What arrives instead, when the
//! input method commits, is a paste -- which is the same question Obelus
//! already answered about where typed text goes when several things are on
//! screen.
//!
//! The keys during that spelling belong to the input method, so a plain
//! character is swallowed while one is being composed: it is arriving
//! twice, once as the spelling and once as the word. A chord is not --
//! `ctrl+s` means save whatever is being typed.

use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use obelus_app::{
    app::{self, App},
    event::{Event, Pointer},
};
use obelus_ui::shapes::{Bar, Joined};
use ratatui::{layout::Rect, style::Color};
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
        Barred, Behind, Capped, Cells, Marked, Marking, Measured, Page, Parted, Rolled, Ruled,
        Said, Sheened, Spelling, Stroked, Ticked, Update,
    },
    keys,
    motion::{Motion, Wake},
};

/// How many rows a notch of the wheel moves, which is what every terminal
/// sends and so what the rest of Obelus already expects.
const NOTCH: isize = 3;

/// How long a wait on the window's side has to be before it is a line in
/// the log.
///
/// The window has been seen to sit still for most of a minute with both of
/// its threads asleep -- the application waiting for an event, the window
/// waiting on the compositor -- and nothing in the log to say which wait it
/// was. A frame is a few milliseconds, so a second is not a slow frame: it
/// is something that did not arrive.
pub(crate) const SLOW: Duration = Duration::from_secs(1);

/// What the window's loop is woken for.
#[derive(Clone, Debug)]
pub(crate) enum Waking {
    /// Obelus drew a frame, at this moment.
    Frame(Instant),
    /// Obelus's loop has ended, which is the window's reason to exist gone.
    Finished,
    /// The reader chose somewhere to go in another window.
    Going(crate::elsewhere::Going),
    /// Another Obelus's reader asked to be brought to this window.
    ComeForward(Option<String>),
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

/// A band of rows on the frame: a list, and where it has got to.
///
/// What the window keeps about one between frames, which is the whole of
/// how a scroll is known: a band that scrolled and a band whose every row
/// changed are the same handful of differing cells, and only `top` says
/// which happened.
#[derive(Clone, Debug)]
struct Rolling {
    /// The rows it is drawn in, which with `under` is what says it is the
    /// same band as the one on the frame before -- see [`Rolling::is`].
    room: Rect,
    /// How far down its list it has got.
    top: i64,
    /// The page as it was when the slide it is in began, or `None` where
    /// it is not in one.
    ///
    /// Per band, and shared. The rows a list has scrolled past are not on
    /// the new page at all, and the only place they exist is the page it
    /// scrolled off -- which is a different page for each band, because
    /// each begins its slide at its own moment: a band that jumped too far
    /// to draw while another was sliding took no page of its own, and its
    /// next step would otherwise have been filled out of one from before
    /// that jump. One clone of the page a frame however many bands hold
    /// it, because what they hold is the same picture and none of them
    /// writes to it.
    before: Option<Arc<Page>>,
    /// The bar beside it, where it has one.
    bar: Option<Bar>,
    /// Which row that bar's mark was on when the slide under way began,
    /// or `None` where nothing is under way.
    ///
    /// The slide's other end, and the reason it is kept rather than
    /// worked out: both ends have to be rows the bar was *drawn* at, or
    /// the mark sets out from somewhere it was never drawn and steps back
    /// to catch up.
    origin: Option<u16>,
    /// Whether a pane was put over it, which is whether it was said before
    /// the pane's backdrop: a view draws the page, then says what is
    /// behind the pane, then draws the pane and its own rows.
    ///
    /// Said rather than worked out from where the two are, because a
    /// full-screen dialog is over everything, so a transcript under one is
    /// as inside it as the dialog's own list.
    under: bool,
}

impl Rolling {
    /// Whether this is the band `was` was, a frame on.
    ///
    /// Where it is and whether a pane is over it. Where alone is not
    /// enough, because a dialog's list can sit exactly where the page under
    /// it has one -- the settings over a conversation share its
    /// transcript's rows -- and the first of the two was taken for both: the
    /// settings, scrolled, were measured against a transcript at the top
    /// every frame, and slid the difference again on every one of them.
    fn is(&self, was: &Self) -> bool {
        self.room == was.room && self.under == was.under
    }
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
    /// Where the input method was last told the caret is, in pixels:
    /// left, top, width, height.
    ///
    /// So that it is told again only when that moves. Telling it commits
    /// the text box's state on Wayland, and an input method that answers a
    /// commit by sending its spelling again would be answered in turn --
    /// for as long as the reader was typing.
    pointed: Option<[f32; 4]>,
    /// Whether the input method is on, which it is only where a character
    /// typed would go into some text -- see `allow_the_input_method`.
    allowed: bool,
    /// Whether the window is on X11, where the input method is on for good.
    on_x11: bool,
    /// Where the marks go on the frame being shown.
    marked: Vec<Marked>,
    /// And where its key caps are, kept the same way and for the same
    /// reason.
    capped: Vec<Capped>,
    /// The ones the frame being laid out has asked for so far.
    capping: Vec<Capped>,
    /// What the page is drawn on, for the margin round the grid.
    ///
    /// The application's, said when it hears who is drawing and again
    /// after every change to the settings. `Reset` until it has: a window
    /// is drawing nothing at all before the first of those, so the default
    /// is never on the screen.
    ground: Color,
    /// How many pixels at the top of the window are the title bar's --
    /// see `title::height`. Measured with the columns and rows, because it
    /// changes when they do: a resize, a screen of another density, and
    /// full screen, which takes the bar away.
    titled: f32,
    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on. The application's, said
    /// at the same two moments the ground is.
    holding: (Color, Color),
    /// Which cells are switches on the frame being shown.
    ticked: Vec<Ticked>,
    /// The mark the light runs across, where the frame drew one.
    sheened: Option<Sheened>,
    /// Which rows begin something new on the frame being shown.
    parted: Vec<Parted>,
    /// And on the one being laid out.
    ticking: Vec<Ticked>,
    sheening: Option<Sheened>,
    parting: Vec<Parted>,
    /// Which columns are bars on the frame being shown.
    barred: Vec<Bar>,
    /// And those again with how the window is showing each, refilled
    /// every frame rather than kept: `shown` is a moment's answer and
    /// the pointer moves between frames. A `Vec` that is cleared and
    /// filled rather than made, because this is on the frame's own path.
    showing: Vec<Barred>,
    /// And on the one being laid out.
    barring_up: Vec<Bar>,
    /// Which rows are rules on the frame being shown.
    ruled: Vec<Ruled>,
    /// And on the one being laid out.
    ruling: Vec<Ruled>,
    /// Which runs of a one-cell column are change marks on the frame being
    /// shown.
    stroked: Vec<Stroked>,
    /// And on the one being laid out.
    stroking: Vec<Stroked>,
    /// Which bands of rows are lists, how far down each has got and what
    /// bar says so, on the frame being shown.
    ///
    /// As many as the screen has one of: a hover over a file is a band
    /// and so is the file under it, and which of them moved is not which
    /// of them the frame mentioned last.
    scrolled: Vec<Rolling>,
    /// And on the frame being laid out.
    scrolling: Vec<Rolling>,
    /// What is under the pane on the frame being shown, where there is
    /// one.
    behind: Option<Behind>,
    /// And on the frame being laid out.
    behinding: Option<Behind>,
    /// Every pane on the frame being shown, by the edge it is joined
    /// along, furthest first -- see `Motion::panes_laid`.
    panes: Vec<Joined>,
    /// And on the frame being laid out.
    paning: Vec<Joined>,
    /// What is behind a box with a frame round it, on the frame being
    /// shown -- a second pane, over the first where there is one.
    cards: Vec<Behind>,
    /// And on the one being laid out.
    carding: Vec<Behind>,
    /// And the ones the frame being laid out has asked for so far.
    ///
    /// Two lists because a frame is drawn from what it said, not from what
    /// the next one is saying: a screen part way through being described
    /// would be marks from two screens at once.
    marking: Vec<Marked>,
    /// When the window first asked to be drawn and has not been yet.
    ///
    /// The earliest unanswered ask, because on Wayland a redraw waits for
    /// the compositor's frame callback, and a callback that never comes is
    /// a screen that stops changing while nothing is busy.
    asked: Cell<Option<Instant>>,
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
    /// What this window brings others forward with, once there is one.
    here: Option<crate::elsewhere::Here>,
    /// Where the reader asked to go, by the token each is waiting on.
    going: Vec<(
        winit::event_loop::AsyncRequestSerial,
        crate::elsewhere::Going,
    )>,
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
            pointed: None,
            allowed: false,
            on_x11: false,
            marked: Vec::new(),
            marking: Vec::new(),
            capped: Vec::new(),
            capping: Vec::new(),
            ground: Color::Reset,
            titled: 0.0,
            holding: (Color::Reset, Color::Reset),
            ticked: Vec::new(),
            sheened: None,
            sheening: None,
            parted: Vec::new(),
            parting: Vec::new(),
            ticking: Vec::new(),
            barred: Vec::new(),
            barring_up: Vec::new(),
            showing: Vec::new(),
            ruled: Vec::new(),
            stroked: Vec::new(),
            stroking: Vec::new(),
            ruling: Vec::new(),
            scrolled: Vec::new(),
            scrolling: Vec::new(),
            behind: None,
            behinding: None,
            panes: Vec::new(),
            paning: Vec::new(),
            cards: Vec::new(),
            carding: Vec::new(),
            // The blink is asked once, on the way up: it is a question
            // about the system rather than about this window.
            motion: Motion::new(Blink::asked()),
            asked: Cell::new(None),
            points,
            here: None,
            going: Vec::new(),
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
            self.asked
                .set(self.asked.get().or_else(|| Some(Instant::now())));
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
        self.titled = crate::title::height(window);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (width, height) = (size.width as f32, size.height as f32 - self.titled);
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

    /// Turns the input method on where a character typed would go into
    /// some text, and off everywhere else.
    ///
    /// Without it on a window gets keys and nothing else, and there is no
    /// way to type any language that is spelled before it is written. With
    /// it on where nothing is typed -- the counts, a list only read, a
    /// commit's version of a file, a key being bound -- the letters a
    /// reader meant as keys are spelling instead, and a list of candidates
    /// comes up for a word with nowhere to go.
    ///
    /// Said only when it moves: the frame says it every time, and turning
    /// an input method on and off is a round trip to it.
    fn allow_the_input_method(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        // On X11 it stays on, as it was before it followed the screen:
        // winit answers every change with a new input context and never
        // focuses it, so one turned back on while the window has the focus
        // hears no keys until the reader goes elsewhere and comes back.
        // Keys swallowed where nothing is typed is the smaller harm.
        if self.on_x11 {
            return;
        }
        let typing = self.page.typing();
        if typing == self.allowed {
            return;
        }
        self.allowed = typing;
        window.set_ime_allowed(typing);
        match typing {
            // Turned on, it has to be told again where the caret is: what
            // it was told before belonged to a text box that has since been
            // put away, and on Wayland that is the box being told.
            true => self.pointed = None,
            // Turned off, a word half spelled is over -- said here rather
            // than waited for, because Windows takes the context away
            // without a word, and `composing` left standing swallows every
            // plain key after it.
            false => {
                self.composing = false;
                self.spelling = None;
            }
        }
    }

    /// Tells the input method where the caret is, so that its candidates
    /// are drawn beside the word being typed rather than in the corner of
    /// the screen.
    ///
    /// Asked on every frame and on every change to what is being spelled,
    /// and said only when the answer moved -- see `pointed`. The spelling
    /// is the one that matters: the window draws it without Obelus hearing
    /// of it, so no frame comes while a word grows, and on macOS and
    /// Windows the candidates go wherever this last said. Asked on frames
    /// alone, they stayed at the first letter of the pinyin.
    fn point_the_input_method(&mut self) {
        let (Some(window), Some(fonts), Some(caret)) =
            (self.window.as_ref(), self.fonts.as_ref(), self.page.caret())
        else {
            return;
        };
        let cell = fonts.cell();
        // Past what is being spelled, so the candidates sit under the end
        // of the word rather than under the character it started at.
        let along = self.spelling.as_ref().map_or(0, Spelling::columns);
        // And put back on here, for the same reason: what an input method
        // is told is a place in the window, not a place in the grid.
        let margin = self.margin();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a caret is a few columns into what is being spelled"
        )]
        let area = [
            (f32::from(caret.x) + along as f32) * cell.width + margin[0],
            f32::from(caret.y) * cell.height + margin[1],
            cell.width,
            cell.height,
        ];
        if self.pointed == Some(area) {
            return;
        }
        self.pointed = Some(area);
        window.set_ime_cursor_area(
            PhysicalPosition::new(area[0], area[1]),
            PhysicalSize::new(area[2], area[3]),
        );
    }

    /// How far in from the window's edges the grid starts.
    ///
    /// Worked out from the window as it is rather than kept: a resize
    /// changes it, and a second copy is a second thing to put back in
    /// step. Nothing where there is no window or no font yet, which is
    /// also a window with nothing on the screen to point at.
    fn margin(&self) -> [f32; 2] {
        let (Some(window), Some(fonts)) = (self.window.as_ref(), self.fonts.as_ref()) else {
            return [0.0, 0.0];
        };
        let cell = fonts.cell();
        let size = window.inner_size();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (across, down) = (size.width as f32, size.height as f32);
        crate::grid::origin(
            [across, down],
            self.titled,
            [cell.width, cell.height],
            [self.page.columns(), self.page.rows()],
        )
    }

    /// Which cell a place in the window is in.
    fn cell_at(&self, at: PhysicalPosition<f64>) -> Option<(u16, u16)> {
        let fonts = self.fonts.as_ref()?;
        let cell = fonts.cell();
        // Taken off first: the grid is middled in the window, so a place
        // in the window is a margin further along than the same place in
        // the grid. A pointer that skipped this would read the last row
        // where the reader was on the one before it.
        let margin = self.margin();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a place inside the window, divided by a cell"
        )]
        let column = (((at.x as f32 - margin[0]).max(0.0) / cell.width) as u32)
            .min(u32::from(u16::MAX)) as u16;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a place inside the window, divided by a cell"
        )]
        let row = (((at.y as f32 - margin[1]).max(0.0) / cell.height) as u32)
            .min(u32::from(u16::MAX)) as u16;
        Some((
            column.min(self.page.columns().saturating_sub(1)),
            row.min(self.page.rows().saturating_sub(1)),
        ))
    }
}

/// Whether this pass through the loop has to draw.
///
/// Two questions, and only the first was ever asked. `stepped` is
/// `Motion::advance`, which speaks for the things that *step* -- the
/// blink, a glide, a slide, a band catching up -- and something drawn
/// from the clock alone steps nothing: the light across the welcome
/// screen's mark is where a cell is at this instant, worked out from
/// `now` at the drawing. So a window that asked only `advance` set
/// `ControlFlow::Poll` for that light and then drew not one frame of it.
/// The loop polled as fast as it could be asked to, the screen stayed at
/// whatever it last held, and the process sat at a hundred per cent --
/// which from the outside is indistinguishable from Obelus having hung.
///
/// `Wake::EveryFrame` is the second question already answered: it is the
/// window saying something is in flight. A pass about to poll for a frame
/// is a pass that wants one, and asking for it is also what lets the
/// surface pace the loop at all -- the vertical blank holds nothing back
/// from a loop that never presents.
fn wants_a_frame(stepped: bool, wake: Option<Wake>) -> bool {
    stepped || wake == Some(Wake::EveryFrame)
}

/// Whether the window is on X11, XWayland included.
fn on_x11(window: &Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window.window_handle().is_ok_and(|handle| {
        matches!(
            handle.as_raw(),
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)
        )
    })
}

impl ApplicationHandler<Waking> for Showing {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        // Once. A platform that suspends and resumes says so again, and the
        // window it is about is the one already here.
        let Some(mut app) = self.starting.take() else {
            return;
        };
        // With the permission to come forward, where another Obelus started
        // this one for a reader who asked for it.
        let attributes = crate::elsewhere::started_with(
            events,
            crate::title::asked_for(marked(named(
                Window::default_attributes()
                    .with_title("Obelus")
                    .with_inner_size(LogicalSize::new(1100.0, 720.0)),
            ))),
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
        // And the other windows, for the same reason: what brings one
        // forward is asked on this display.
        let here = crate::elsewhere::Here::take(events);
        app.windowed_by(Arc::new(crate::elsewhere::Elsewhere::new(
            self.proxy.clone(),
            here.can_bring(),
        )));
        self.here = Some(here);
        // On for good there -- see `allow_the_input_method`.
        self.on_x11 = on_x11(&window);
        if self.on_x11 {
            window.set_ime_allowed(true);
            self.allowed = true;
        }
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
            let _ = proxy.send_event(Waking::Frame(Instant::now()));
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
            Waking::Frame(sent) => {
                let waited = sent.elapsed();
                if waited >= SLOW {
                    tracing::warn!(?waited, "a frame waited for the window to wake");
                }
                let Some(frames) = self.frames.as_ref() else {
                    return;
                };
                // Everything that has arrived, not one frame: several
                // frames may be waiting -- a key and the answer it
                // provoked -- and drawing the older ones would be drawing
                // screens the reader is never meant to see.
                let was = self.page.caret();
                let whose_was = self.page.whose();
                // Which panes, not whether there is one: see
                // `Motion::panes_laid`. The edge each is joined along and
                // not its rectangle, because a rectangle changes when the
                // reader drags the window's own edge, and a pane replaying
                // its arrival on every pixel of a resize is worse than one
                // that never arrives at all.
                let were_panes = self.panes.clone();
                let had_a_card = !self.cards.is_empty();
                // Cloned rather than taken: a wake with no whole frame
                // in it leaves what is on the screen alone, and a band
                // that had been emptied here would be one the next frame
                // has nothing to compare against -- and one the drawing
                // stops asking about in the middle of its slide.
                let was_at = self.scrolled.clone();
                // Kept only where a band could move, because that is the
                // only thing it is for.
                let before = (!was_at.is_empty()).then(|| Arc::new(self.page.clone()));
                let mut drew = false;
                let mut sized = None;
                let mut faces = None;
                while let Ok(update) = frames.try_recv() {
                    match update {
                        // Not cells, so the page never sees them.
                        Update::TextSize(points) => sized = Some(points),
                        Update::Animates(on) => self.motion.animates(on),
                        Update::Fonts(names) => faces = Some(names),
                        Update::Ground(ground) => {
                            // Said again after every change to the
                            // settings, and nearly always the same.
                            if ground != self.ground
                                && let Some(window) = self.window.as_ref()
                            {
                                crate::title::follow(window, ground);
                            }
                            self.ground = ground;
                        }
                        Update::Holding { held, row } => self.holding = (held, row),
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
                        //
                        // A box with a frame is kept beside that rather
                        // than in its place, because it is put over one:
                        // the card of every key opens over a list, the
                        // settings and the counts, all of which are glass.
                        // A sheet said after one was drawn over it, so a
                        // box is nearer the reader than the sheet or it is
                        // not kept at all.
                        Update::Behind {
                            area,
                            joined,
                            ground,
                            cells,
                        } => {
                            let behind = Behind {
                                area,
                                joined,
                                ground,
                                cells,
                            };
                            match joined {
                                Joined::Nowhere => self.carding.push(behind),
                                // A pane, however many edges it is joined
                                // along.
                                Joined::Above | Joined::Below | Joined::Screen => {
                                    self.behinding = Some(behind);
                                    self.paning.push(joined);
                                    self.carding.clear();
                                    for band in &mut self.scrolling {
                                        band.under = true;
                                    }
                                }
                            }
                        }
                        Update::Scrolled { area, top, bar } => {
                            self.scrolling.push(Rolling {
                                room: area,
                                top,
                                before: None,
                                bar,
                                origin: None,
                                under: false,
                            });
                        }
                        Update::Ticked { area, on } => {
                            self.ticking.push(Ticked { area, on });
                        }
                        Update::Parted { area } => {
                            self.parting.push(Parted { area });
                        }
                        Update::Sheened { area, from, to } => {
                            self.sheening = Some(Sheened { area, from, to });
                        }
                        Update::Barred { bar } => self.barring_up.push(bar),
                        Update::Ruled { area } => self.ruling.push(Ruled { area }),
                        Update::Stroked { stroke } => {
                            self.stroking.push(Stroked { stroke });
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
                            self.barred = std::mem::take(&mut self.barring_up);
                            // Said here and not at the drawing: what
                            // stirs a bar is its mark being somewhere
                            // else than it was, and only the frame
                            // arriving knows that. A frame drawn twice
                            // for some other reason must not read as a
                            // reader scrolling.
                            self.motion
                                .bars_drawn(&self.barred, self.pointer, Instant::now());
                            self.ruled = std::mem::take(&mut self.ruling);
                            // Only where the page still holds it: the
                            // welcome screen is what every list is opened
                            // over, and a light run across a list lights
                            // its rows -- see `Sheened::still_said`.
                            self.parted = std::mem::take(&mut self.parting);
                            self.sheened = self
                                .sheening
                                .take()
                                .filter(|mark| mark.still_said(&self.page));
                            // The same shape as the bars': what the frame
                            // said is on screen until another says
                            // otherwise, and whether the light has
                            // anywhere to run is what decides the frames.
                            self.motion
                                .sheen_drawn(self.sheened.is_some(), Instant::now());
                            self.stroked = std::mem::take(&mut self.stroking);
                            self.behind = self.behinding.take();
                            self.panes = std::mem::take(&mut self.paning);
                            self.cards = std::mem::take(&mut self.carding);
                            self.scrolled = std::mem::take(&mut self.scrolling);
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
                //
                // And a pane of another shape where one already was. It
                // asked only whether there was one, which is the same
                // question for "a list opened over a list" and for "the
                // palette went and the settings came" -- so a page that can
                // only be reached through the palette could never arrive,
                // because the palette is a pane and it was still counted as
                // one. Obelus's own pages are all of them reached that way,
                // which is every full-screen dialog there is. And the whole
                // pile rather than its top, because a pane closing over
                // another changes the top too -- see `Motion::panes_laid`.
                self.motion
                    .panes_laid(&were_panes, &self.panes, Instant::now());
                // And a box over the page, the same way and for the same
                // reason. Whether there is one rather than which one:
                // a completion list redrawn on every character the reader
                // types is the same box, and one that came up again on
                // each of them would be a flicker under their hands.
                match (had_a_card, self.cards.is_empty()) {
                    (false, false) => self.motion.card_opened(Instant::now()),
                    (true, true) => self.motion.card_shut(),
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
                let now = Instant::now();
                for band in &mut self.scrolled {
                    let Some(was) = was_at.iter().find(|was| band.is(was)) else {
                        continue;
                    };
                    // What it is already counting from, where it is in the
                    // middle of a slide.
                    band.before.clone_from(&was.before);
                    // Where the bar's mark set out, carried over while the
                    // slide it belongs to runs: both ends have to be rows
                    // the bar was *drawn* at, or the mark sets out from
                    // somewhere it never was and steps back to catch up.
                    band.origin = was.origin;
                    if band.top == was.top
                        || band.top.abs_diff(was.top) > u64::from(band.room.height)
                    {
                        continue;
                    }
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a list is rows, and a scroll is a few of them"
                    )]
                    let rows = (band.top - was.top) as f32;
                    // Only where this begins a fresh slide: one already
                    // under way keeps the page it started with, because
                    // that is the page the rows it scrolled past are on
                    // and the one the distance is counted from.
                    if self
                        .motion
                        .band_moved(band.room, rows, f32::from(band.room.height), now)
                    {
                        band.origin = was.bar.map(|bar| bar.mark);
                        band.before.clone_from(&before);
                    }
                }
                // And what is no longer on the screen is no longer kept.
                let rooms: Vec<Rect> = self.scrolled.iter().map(|band| band.room).collect();
                self.motion.bands_drawn(&rooms);
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
                    self.allow_the_input_method();
                    self.point_the_input_method();
                    self.redraw();
                }
            }
            Waking::Finished => {
                // Before the window goes, because what Obelus is offering
                // is offered *by* this process: the selection belongs to a
                // live client, and in a moment there will not be one.
                obelus_clipboard::hand_over();
                events.exit();
            }
            Waking::Going(going) => {
                let (Some(window), Some(here)) = (self.window.as_ref(), self.here.as_ref()) else {
                    return;
                };
                // Asked now, while the key the reader pressed is the last
                // thing this window was told: that is what the compositor
                // gives the permission against. The going waits for it.
                match here
                    .wants_a_token(&going)
                    .then(|| crate::elsewhere::ask(window))
                    .flatten()
                {
                    Some(serial) => self.going.push((serial, going)),
                    None => crate::elsewhere::go(going, None),
                }
            }
            Waking::ComeForward(token) => {
                if let (Some(window), Some(here)) = (self.window.as_ref(), self.here.as_ref()) {
                    here.come_forward(window, token);
                }
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
        // And what brings other windows forward, which holds the same
        // display for the same reason. Let go of here rather than with the
        // rest of the window: dropped afterwards, its connection destroyed
        // its proxies on a display winit had already closed, and every way
        // out of the window ended in a segfault.
        self.here = None;
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
        let wake = self.motion.wake(now, caret);
        if wants_a_frame(self.motion.advance(now, caret), wake) {
            self.redraw();
        }
        events.set_control_flow(match wake {
            Some(Wake::At(when)) => ControlFlow::WaitUntil(when),
            // Something is in flight, so the next frame is wanted as soon
            // as the screen will take one. What paces it is the screen's
            // refresh -- the surface presented on the vertical blank, or on
            // Wayland the compositor's frame callback (see `paint`): that is
            // the rate an animation is meant to run at, and the one
            // number nobody here has to pick -- and it paces nothing at
            // all unless a frame was actually asked for, which is what
            // the line above is.
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
            WindowEvent::ActivationTokenDone { serial, token } => {
                if let Some(at) = self.going.iter().position(|(asked, _)| *asked == serial) {
                    let (_, going) = self.going.remove(at);
                    crate::elsewhere::go(going, Some(token.into_raw()));
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(asked) = self.asked.take() {
                    let waited = asked.elapsed();
                    if waited >= SLOW {
                        tracing::warn!(?waited, "the window waited to be let draw");
                    }
                }
                // How each bar is being shown, worked out for this frame:
                // the settling is a moment's answer and the pointer moves
                // between frames, so neither is a thing to keep.
                let now = Instant::now();
                self.showing.clear();
                self.showing.extend(self.barred.iter().map(|bar| Barred {
                    bar: *bar,
                    shown: self.motion.bar_shown(bar.area, now),
                    under: self.motion.bar_under(bar.area, now),
                }));

                // How each band is being shown, worked out for this
                // frame the way the bars are: where a band has got to is
                // a moment's answer, and a band that has caught up is not
                // in this at all.
                let rolled: Vec<Rolled<'_>> = self
                    .scrolled
                    .iter()
                    .filter_map(|band| {
                        let (behind, since) = self.motion.band_shown(band.room, now)?;
                        Some(Rolled {
                            room: band.room,
                            under: band.under,
                            before: band.before.as_deref()?,
                            behind,
                            since,
                            bar: band.bar.map(|bar| {
                                let origin = band.origin.unwrap_or(bar.mark);
                                (bar.area, f32::from(origin) - f32::from(bar.mark))
                            }),
                        })
                    })
                    .collect();

                let (Some(painter), Some(fonts)) = (self.painter.as_mut(), self.fonts.as_mut())
                else {
                    return;
                };
                painter.drawn_on(self.ground);
                painter.titled(self.titled);
                painter.holding(self.holding);
                if let Err(error) = painter.paint(
                    &self.page,
                    fonts,
                    self.spelling.as_ref(),
                    self.motion.moving(Instant::now()),
                    Said {
                        marked: &self.marked,
                        capped: &self.capped,
                        ticked: &self.ticked,
                        barred: &self.showing,
                        ruled: &self.ruled,
                        sheened: self.sheened.as_ref(),
                        parted: &self.parted,
                        stroked: &self.stroked,
                        behind: self.behind.as_ref(),
                        cards: &self.cards,
                        bands: &rolled,
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
                        // As it arrived: where an input method says its own
                        // caret is differs by input method and by
                        // compositor, and a caret drawn in the wrong place
                        // is otherwise a question with no answer to look at.
                        tracing::debug!(?spelling, ?caret, "the input method is spelling");
                        self.composing = !spelling.is_empty();
                        self.spelling = Spelling::new(spelling, caret);
                        self.point_the_input_method();
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
    use std::time::Instant;

    use ratatui::layout::Rect;

    use super::{MARK, Rolling, wants_a_frame};
    use crate::motion::{Motion, Wake};

    /// A band is matched with the one it was, not with another in the same
    /// rows: a dialog's list over a page's that happens to sit where it does
    /// -- the settings over a conversation -- is two bands.
    ///
    /// Deliberate break: match on `room` alone in `Rolling::is`. The
    /// settings' list is then taken for the transcript under it, which is at
    /// its top while the list is not, and it slid on every frame.
    #[test]
    fn a_band_under_a_pane_is_not_the_pane_s() {
        let band = |top, under| Rolling {
            room: Rect::new(0, 2, 75, 18),
            top,
            before: None,
            bar: None,
            origin: None,
            under,
        };
        let was = [band(0, true), band(4, false)];
        let settings = band(4, false);
        assert_eq!(
            was.iter().find(|was| settings.is(was)).map(|was| was.top),
            Some(4),
            "the list was taken for the transcript under it"
        );
        let transcript = band(0, true);
        assert_eq!(
            was.iter().find(|was| transcript.is(was)).map(|was| was.top),
            Some(0),
            "the transcript was taken for the list over it"
        );
    }

    /// A pass that is about to poll asks for the frame it is polling for.
    ///
    /// Otherwise the loop polls for nothing: `ControlFlow::Poll` with no
    /// redraw requested is a busy wait that draws nothing and is paced by
    /// nothing, because what paces an animation here is the surface being
    /// presented and a loop that never presents is never held back. The
    /// welcome screen sat at a hundred per cent behind a frozen picture.
    ///
    /// The second half is why the first cannot be `advance` alone, and it
    /// is asked of `Motion` rather than assumed: the light is out, so the
    /// window wants every frame, and nothing stepped to say so.
    ///
    /// Deliberate break: drop the `EveryFrame` arm from `wants_a_frame`.
    /// The third assertion goes red, which is exactly the case this is
    /// about -- in flight, and nothing stepped for it.
    #[test]
    fn a_pass_that_polls_asks_for_the_frame_it_is_polling_for() {
        assert!(!wants_a_frame(false, None), "nothing is moving");
        assert!(wants_a_frame(true, None), "something stepped");
        assert!(
            wants_a_frame(false, Some(Wake::EveryFrame)),
            "in flight, and nothing stepped for it"
        );
        assert!(
            !wants_a_frame(false, Some(Wake::At(Instant::now()))),
            "a moment to come back at is not a rate"
        );

        // And that third case is the one that was on the screen: the light
        // asks for a rate and steps nothing.
        let mut motion = Motion::new(None);
        let now = Instant::now();
        motion.sheen_drawn(true, now);
        assert_eq!(
            motion.wake(now, false),
            Some(Wake::EveryFrame),
            "the light is out"
        );
        assert!(
            !motion.advance(now, false),
            "and nothing stepped, so `advance` cannot be what asks"
        );
    }

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
