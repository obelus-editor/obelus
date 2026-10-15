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
//! **What an input method is spelling is written in, the way Zed does it.**
//! Typing Chinese is spelling a word before it exists: several keypresses
//! that are not characters, and then one character that is. Every change to
//! the spelling goes to the application, which writes it where the reader
//! is writing, over the spelling before it -- so it wraps where the box
//! wraps and pushes along what comes after it, as the word will. The window
//! once drew it over the cells after the caret instead, and a word spelled
//! at the end of a row ran on off the side of the box and of the screen.
//! When the input method commits, the word arrives as a paste, which is the
//! same question Obelus already answered about where typed text goes when
//! several things are on screen; the application has already been told the
//! spelling is over by then, and the word goes where it was.
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
use obelus_font::Fonts;
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
    grid::{
        Barred, Behind, Capped, Cells, Going, Marked, Marking, Measured, Page, Paired, Parted,
        Rolled, Ruled, Said, Sheened, Spun, Stroked, Ticked, Update,
    },
    keys,
    motion::{Lane, Motion, Wake},
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
    /// A file was dropped on the window, and only the display connection
    /// Obelus listens on itself heard it: winit hears none on Wayland.
    #[cfg(target_os = "linux")]
    Dropped(std::path::PathBuf),
}

/// Runs Obelus in a window until the reader leaves.
pub(crate) fn show(app: App) -> Result<()> {
    let mut events = EventLoop::<Waking>::with_user_event();
    // No menu, because the one winit puts up quits on `cmd+q` without
    // asking about unwritten files -- see `keys::Asked`.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        events.with_default_menu(false);
    }
    let events = events
        .build()
        .context("no window system to open a window on")?;
    let mut showing = Showing::new(app, events.create_proxy());
    events.run_app(&mut showing).context("the window stopped")?;
    let outcome = showing.outcome();
    // Written down as well as returned: a start from a menu has no console
    // to return it to, and one that fails before its first frame has no
    // window either -- see `Showing::shown` -- so the log is all there is.
    if let Err(error) = &outcome {
        tracing::error!(?error, "Obelus stopped");
    }
    outcome
}

/// The screen a pane went from, kept until the painter has a picture of
/// it -- see `Painter::keep`.
///
/// What the frame said about the page as well as the page, because a pane is
/// drawn from both: its caps, its bars and its rules are said beside its
/// cells rather than in them. Taken rather than copied, at the frame that
/// replaced them, which is the one moment they are let go of anyway.
struct Left {
    page: Arc<Page>,
    marked: Vec<Marked>,
    capped: Vec<Capped>,
    ticked: Vec<Ticked>,
    spun: Vec<Spun>,
    showing: Vec<Barred>,
    ruled: Vec<Ruled>,
    spelled: Vec<(Rect, usize)>,
    sheened: Option<Sheened>,
    parted: Vec<Parted>,
    stroked: Vec<Stroked>,
    paired: Vec<Paired>,
    stack: Vec<Behind>,
    /// Which of its panes went.
    going: Vec<Going>,
}

/// The screen in the theme that has just gone, and the two colours of that
/// theme the window paints for itself -- see `Showing::recolouring`.
///
/// The colours with it rather than read off the painter when it is kept,
/// because the painter is handed them at a redraw and the theme can change
/// before the first: then what it had was the colour it starts at, and the
/// screen going had a black margin round it.
struct Recolouring {
    was: Left,
    ground: Color,
    holding: (Color, Color),
}

impl Left {
    /// What it said, the way a frame being drawn says it.
    fn said(&self) -> Said<'_> {
        Said {
            marked: &self.marked,
            capped: &self.capped,
            ticked: &self.ticked,
            spun: &self.spun,
            barred: &self.showing,
            ruled: &self.ruled,
            spelled: &self.spelled,
            sheened: self.sheened.as_ref(),
            parted: &self.parted,
            stroked: &self.stroked,
            paired: &self.paired,
            fading: &[],
            stack: &self.stack,
            bands: &[],
        }
    }

    /// The panes of its pile above the ones that stayed, less any inside
    /// another that went: that one is in the picture of the one under it,
    /// and goes with it.
    fn going(stack: &[Behind], stayed: usize) -> Vec<Going> {
        let mut going: Vec<Going> = Vec::new();
        for over in stack.iter().filter(|over| !over.is_a_box()).skip(stayed) {
            if !going
                .iter()
                .any(|gone| gone.area.intersection(over.area) == over.area)
            {
                going.push(Going {
                    area: over.area,
                    joined: over.joined,
                });
            }
        }
        going
    }
}

/// A band of rows on the frame: a list, and where it has got to.
///
/// What the window keeps about one between frames, which is the whole of
/// how a scroll is known: a band that scrolled and a band whose every row
/// changed are the same handful of differing cells, and only `top` says
/// which happened.
#[derive(Clone, Debug)]
struct Rolling {
    /// The rows it is drawn in, which with `layer` is what says it is the
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
    /// How many panes were put over it, which is how many were said after
    /// it: a view draws the page, then says what is behind the pane, then
    /// draws the pane and its own rows.
    ///
    /// Said rather than worked out from where the two are, because a
    /// full-screen dialog is over everything, so a transcript under one is
    /// as inside it as the dialog's own list. And a count rather than
    /// whether, because a pane coming or going over a band is a band that
    /// is not the one it was -- see `Rolling::is`.
    under: u8,
    /// How many panes were said before it, which is which of them it is
    /// in: the page's own lists are on none, the settings' on one.
    layer: u8,
}

impl Rolling {
    /// Whether this is the band `was` was, a frame on.
    ///
    /// Where it is, which pane it is in, and how many are over it. Where
    /// alone is not enough, because a dialog's list can sit exactly where
    /// the page under it has one -- the settings over a conversation share
    /// its transcript's rows -- and the first of the two was taken for
    /// both: the settings, scrolled, were measured against a transcript at
    /// the top every frame, and slid the difference again on every one of
    /// them.
    ///
    /// Which pane, because the panes over a band alone do not say: on the
    /// frame the settings opened their list had nothing over it, as the
    /// transcript had on the frame before, and it took that transcript's
    /// slide and the page of the conversation it was sliding out of. And
    /// how many over it, because the page a band kept is a picture of
    /// whatever was over it then: a transcript that carried its slide out
    /// from under the settings as they closed drew their rows sliding into
    /// the conversation.
    fn is(&self, was: &Self) -> bool {
        self.room == was.room && self.layer == was.layer && self.under == was.under
    }

    /// Which band it is to the clock its slide runs on.
    const fn lane(&self) -> Lane {
        (self.room, self.layer)
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
    /// Whether the pointer is over the title bar, where the page runs up
    /// under it -- see `title::under`. The page's cells stop below it, so a
    /// press there is not a press on the row underneath.
    over_the_title: bool,
    /// Whether the left button went down on the title bar, so that letting
    /// it go is the bar's too and not a release Obelus never saw pressed.
    pressed_on_the_title: bool,
    /// The presses on the title bar, for the second of two to be a double
    /// click -- see `title::Presses`.
    title_presses: crate::title::Presses,
    /// The bar along the top, where Obelus draws it -- see `title::Bar`.
    bar: Option<crate::title::Bar>,
    /// What is open, as tabs along the bar -- see `title::tabs`.
    strip: crate::title::Strip,
    /// How many pixels to a point the screen the window is on has, as it
    /// was last said -- which a change of screen is measured from.
    scale: f64,
    /// Whether the window has the reader's keys, which a bar Obelus draws
    /// says the way the system's would.
    active: bool,
    /// Wheels that report pixels rather than lines, added up until they
    /// amount to a row.
    rolled: f32,
    /// Whether an input method is in the middle of a word.
    ///
    /// While it is, the keys belong to it: a reader typing `n` `i` `h` `a`
    /// `o` is spelling one character, and passing those presses on as well
    /// would put the spelling in the file and the word after it.
    composing: bool,
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
    /// draws nothing before the first frame, which comes after it -- see
    /// `shown` -- so the default is never on the screen.
    ground: Color,
    /// Whether the window is on the screen, which it is from the first
    /// frame the application sends and not before.
    ///
    /// Shown as it is made, a window is on the screen for the second it
    /// takes to start the drawing and the application, and what it shows
    /// there is nobody's: the system's colour where the page will be, and
    /// the system's title bar until the ground arrives to paint it. Beside
    /// a dark Obelus the reader has just opened it from, that is a light
    /// window flashing up and changing its mind. So it is kept hidden, and
    /// a redraw asked for before then draws nothing -- on Wayland, where a
    /// window cannot be hidden, a surface appears with the first thing
    /// drawn on it, and that has to be the page rather than an empty one
    /// on `Reset`.
    shown: bool,
    /// The system says nobody can see the window: covered, on another
    /// desktop, minimised. Only macOS and X11 say so; elsewhere this stays
    /// `false` and the screen is what holds back a frame nobody can see.
    covered: bool,
    /// How many pixels at the top of the window are the title bar's --
    /// see `title::height`. Measured with the columns and rows, because it
    /// changes when they do: a resize, a screen of another density, and
    /// full screen, which takes the bar away.
    titled: f32,
    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on. The application's, said
    /// at the same two moments the ground is.
    holding: (Color, Color),
    /// Which asking for a picture under the page was the last one drawn,
    /// so that an older one decoded later is not -- see `wallpaper`.
    hung: u64,
    /// What takes the picture's place once it has gone down into the
    /// page's colour: another picture, or none -- see `Motion::picture_going`.
    after_the_veil: Option<Option<Arc<crate::wallpaper::Hung>>>,
    /// Which cells are switches on the frame being shown.
    ticked: Vec<Ticked>,
    /// Which cells are the mark that turns on the frame being shown.
    spun: Vec<Spun>,
    /// The mark the light runs across, where the frame drew one.
    sheened: Option<Sheened>,
    /// Which rows begin something new on the frame being shown.
    parted: Vec<Parted>,
    /// And on the one being laid out.
    ticking: Vec<Ticked>,
    spinning: Vec<Spun>,
    sheening: Option<Sheened>,
    parting: Vec<Parted>,
    /// Which columns are bars on the frame being shown, each with how many
    /// panes and boxes had been put over the page when it was said.
    barred: Vec<(Bar, usize)>,
    /// And those again with how the window is showing each, refilled
    /// every frame rather than kept: `shown` is a moment's answer and
    /// the pointer moves between frames. A `Vec` that is cleared and
    /// filled rather than made, because this is on the frame's own path.
    showing: Vec<Barred>,
    /// And on the one being laid out.
    barring_up: Vec<(Bar, usize)>,
    /// Which rows are rules on the frame being shown.
    ruled: Vec<Ruled>,
    /// And on the one being laid out.
    ruling: Vec<Ruled>,
    /// Which cells are the words of a link on the frame being shown.
    linked: Vec<(Rect, usize)>,
    /// And on the one being laid out.
    linking: Vec<(Rect, usize)>,
    /// Which cells are what an input method is still spelling on the frame
    /// being shown.
    spelled: Vec<(Rect, usize)>,
    /// And on the one being laid out.
    spelling_out: Vec<(Rect, usize)>,
    /// Whether the pointer is a hand, so that it is changed only when
    /// what is under it does.
    pointing: bool,
    /// Which runs of a one-cell column are change marks on the frame being
    /// shown.
    stroked: Vec<Stroked>,
    /// And on the one being laid out.
    stroking: Vec<Stroked>,
    /// Which cells are the cursor's pair of brackets on the frame being
    /// shown.
    paired: Vec<Paired>,
    /// And on the one being laid out.
    pairing: Vec<Paired>,
    /// And the pairs the caret was in before, while their light goes out.
    unpaired: Vec<Paired>,
    /// Which bands of rows are lists, how far down each has got and what
    /// bar says so, on the frame being shown.
    ///
    /// As many as the screen has one of: a hover over a file is a band
    /// and so is the file under it, and which of them moved is not which
    /// of them the frame mentioned last.
    scrolled: Vec<Rolling>,
    /// And on the frame being laid out.
    scrolling: Vec<Rolling>,
    /// What is under each pane and each box on the frame being shown,
    /// furthest first: each is glass over everything said before it, so
    /// the order they were said in is the order they are stacked in.
    stack: Vec<Behind>,
    /// And on the frame being laid out.
    stacking: Vec<Behind>,
    /// Every pane on the frame being shown, by the edge it is joined
    /// along, furthest first -- see `Motion::panes_laid`.
    panes: Vec<Joined>,
    /// The screen a pane has just gone from, until the painter has kept it.
    left: Option<Left>,
    /// The screen in the theme that has just gone, until the painter has
    /// kept it -- see `Painter::recolour`. Kept the way a pane's leaving
    /// is, and with nothing going: what goes is all of it.
    recolouring: Option<Recolouring>,
    /// The marks said while it waits, in the new theme's ink: handed to the
    /// painter only once it has kept the screen going, because a new ink is
    /// every mark it had drawn let go, and the screen going would have them
    /// in the colours it is going to.
    carried: Vec<(String, bool, String, obelus_ui::image::Palette)>,
    /// And on the frame being laid out.
    paning: Vec<Joined>,
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
            over_the_title: false,
            pressed_on_the_title: false,
            title_presses: crate::title::Presses::default(),
            strip: crate::title::Strip::default(),
            bar: None,
            scale: 1.0,
            active: true,
            rolled: 0.0,
            composing: false,
            pointed: None,
            allowed: false,
            on_x11: false,
            marked: Vec::new(),
            marking: Vec::new(),
            capped: Vec::new(),
            capping: Vec::new(),
            ground: Color::Reset,
            shown: false,
            covered: false,
            titled: 0.0,
            holding: (Color::Reset, Color::Reset),
            hung: 0,
            after_the_veil: None,
            ticked: Vec::new(),
            spun: Vec::new(),
            spinning: Vec::new(),
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
            paired: Vec::new(),
            pairing: Vec::new(),
            unpaired: Vec::new(),
            ruling: Vec::new(),
            linked: Vec::new(),
            linking: Vec::new(),
            spelled: Vec::new(),
            spelling_out: Vec::new(),
            pointing: false,
            scrolled: Vec::new(),
            scrolling: Vec::new(),
            stack: Vec::new(),
            stacking: Vec::new(),
            panes: Vec::new(),
            left: None,
            recolouring: None,
            carried: Vec::new(),
            paning: Vec::new(),
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
    /// Why the window did not open, said where `obg` was started rather
    /// than only in the log -- see `leaving`.
    fn failed(&self, why: Result<()>) {
        if let Ok(mut outcome) = self.outcome.lock() {
            *outcome = Some(why);
        }
    }

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

    /// A press on a tab, which goes to its document, or on an arrow, which
    /// slides the tabs on to the next one that end is cutting.
    fn press_on_the_tabs(&mut self, spot: crate::title::Spot) {
        match spot {
            crate::title::Spot::Tab(index) => {
                if let Some(document) = self.strip.document(index) {
                    self.tell(Event::Chose(document));
                }
            }
            towards => {
                if self.strip.step(towards) {
                    self.redraw();
                }
            }
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

    /// Puts the system's buttons back in the middle of the bar, at the
    /// moments AppKit lays it out again without the window being measured --
    /// see `title::centre_the_buttons`.
    fn centre_the_buttons(&self) {
        if let Some(window) = self.window.as_ref() {
            crate::title::centre_the_buttons(window);
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
        crate::title::centre_the_buttons(window);
        self.titled = match self.bar.as_ref() {
            Some(bar) => bar.height(window),
            None => crate::title::height(window),
        };
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
            false => self.give_up_spelling(),
        }
    }

    /// Says a word half spelled is over, where one was: its letters are
    /// written in, and an input method that goes away without a word would
    /// leave them there.
    fn give_up_spelling(&mut self) {
        if std::mem::take(&mut self.composing) {
            self.tell(Event::Spelling(String::new(), None));
        }
    }

    /// Tells the input method where the caret is, so that its candidates
    /// are drawn beside the word being typed rather than in the corner of
    /// the screen.
    ///
    /// Asked on every frame, and said only when the answer moved -- see
    /// `pointed`. The caret is the application's, after the spelling it has
    /// written in, so the candidates follow the word as it grows and wraps.
    fn point_the_input_method(&mut self) {
        let (Some(window), Some(fonts), Some(caret)) =
            (self.window.as_ref(), self.fonts.as_ref(), self.page.caret())
        else {
            return;
        };
        let cell = fonts.cell();
        // The margin put back on: what an input method is told is a place
        // in the window, not a place in the grid.
        let margin = self.margin();
        let area = [
            f32::from(caret.x) * cell.width + margin[0],
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

    /// Makes the pointer a hand over the words of a link, and only there.
    fn point_at_links(&mut self) {
        let over = self
            .pointer
            .is_some_and(|(x, y)| on_a_link(&self.linked, &self.page, x, y));
        if over == self.pointing {
            return;
        }
        self.pointing = over;
        if let Some(window) = self.window.as_ref() {
            window.set_cursor(match over {
                true => winit::window::CursorIcon::Pointer,
                false => winit::window::CursorIcon::Default,
            });
        }
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
/// window saying something is in flight. A pass that wants a rate is a
/// pass that wants the next frame, and asking for it is what wakes the
/// loop at all now that it sleeps in between -- see `control_flow`.
///
/// And no frame at all for a window the system says is `covered`, which
/// is the one thing that holds a frame back where the screen does not.
/// macOS hands a covered window no surface to draw on and says so at
/// once, so a frame asked for there is skipped, asked for again on the
/// next pass, and skipped again -- the loop going round as fast as it can
/// for the whole of an animation nobody can see.
fn wants_a_frame(stepped: bool, wake: Option<Wake>, covered: bool) -> bool {
    !covered && (stepped || wake == Some(Wake::EveryFrame))
}

/// Tells the tabs whether a pane runs up under the bar: they are the
/// page's, and it is over them -- see `Strip::cover`.
///
/// The moment the window hears which panes there are, not at the frame
/// that draws them: a press in between is a press on what the reader can
/// already see is going to be there. And the system told at once on
/// Windows, where it is the system that says what a press on the bar is.
///
/// Of the window's pieces and not of the window, because both places that
/// ask hold some other part of it.
fn cover_the_tabs(
    strip: &mut crate::title::Strip,
    stack: &[Behind],
    titled: f32,
    bar: Option<&crate::title::Bar>,
) {
    strip.cover(stack.iter().any(|over| {
        !over.is_a_box() && crate::title::from_the_top(titled, over.area.y, over.joined)
    }));
    if let Some(bar) = bar {
        bar.tabs_are(strip.covers());
    }
}

/// Has the system fill the window with the page's ground where it shows
/// the window before Obelus has drawn it.
///
/// Windows does not wait for a frame before it shows a window at the size
/// it was dragged to, and what is in the part the frame has not reached is
/// the window class's brush -- which winit leaves empty, so it was black: a
/// strip down the edge being dragged, as wide as the drag had got ahead of
/// the frame, and nothing a frame could reach, because it was never in one.
/// Found by painting the brush magenta, after a frame cleared to magenta
/// and a frame that could not be narrower than the window both left it
/// black. The page's ground rather than the picture under it, because a
/// brush is one colour, and the strip is there for the moment the frame
/// takes to arrive.
///
/// The class is one window's here: `obg` opens one.
#[cfg(windows)]
fn filled_with(window: &Window, ground: Color) {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        Graphics::Gdi::{CreateSolidBrush, DeleteObject},
        UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW},
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let (r, g, b) = crate::paint::ground_of(ground);
    // A `COLORREF`, which is the three bytes the other way round.
    let colour: u32 = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16);
    // Safe: the handle is winit's live window. The brush the class had is
    // let go of once it has been replaced, and only if it was one of ours:
    // winit registers the class with none.
    unsafe {
        let brush = CreateSolidBrush(colour);
        if brush.is_null() {
            return;
        }
        // Nothing before and a failure both answer zero, and only the
        // error says which -- a failure leaves the new brush nobody's.
        SetLastError(0);
        let was = SetClassLongPtrW(handle.hwnd.get() as _, GCLP_HBRBACKGROUND, brush as isize);
        if was != 0 {
            DeleteObject(was as _);
        } else if GetLastError() != 0 {
            DeleteObject(brush);
        }
    }
}

/// Nothing to fill: the strip was only ever seen on Windows. On Wayland
/// the compositor waits for the frame, and neither X11 nor macOS has been
/// seen to show one before it.
#[cfg(not(windows))]
const fn filled_with(_window: &Window, _ground: Color) {}

/// How the loop waits after a pass that worked out `wake`.
///
/// Something in flight waits for the frame `wants_a_frame` has just
/// asked for, and the screen is what lets that frame through: on Wayland
/// the compositor's frame callback, which the loop sleeps until (see
/// `paint`), and elsewhere the vertical blank, which the frame is held
/// at as it is presented. That is the rate an animation is meant to run
/// at, and the one number nobody here has to pick.
///
/// It was `ControlFlow::Poll`, which never sleeps: the loop ran flat out
/// between the frames the screen let through, and a Wayland compositor
/// lets through none at all to a window it is not showing. So the
/// welcome screen left on another workspace held a whole core for the two
/// and a half seconds of every pass of its light, for hours, with nothing
/// drawn. A window covered on macOS or X11 is the other half, and is
/// `wants_a_frame`'s; one minimised on Windows, which says nothing about
/// being covered, is still neither's.
fn control_flow(wake: Option<Wake>) -> ControlFlow {
    match wake {
        Some(Wake::At(when)) => ControlFlow::WaitUntil(when),
        Some(Wake::EveryFrame) | None => ControlFlow::Wait,
    }
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
            crate::title::asked_for(marked(named(hidden(
                Window::default_attributes()
                    .with_title(crate::title::TITLE)
                    .with_inner_size(LogicalSize::new(1100.0, 720.0)),
            )))),
        );
        let window = match events.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                tracing::error!(%error, "no window");
                self.failed(Err(error).context("no window"));
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
                self.failed(Err(error).context("nothing to draw with"));
                events.exit();
                return;
            }
        };
        // The last thing that can stop a start, so the terminal `obg` was
        // typed into is the reader's again -- see `leaving`.
        crate::leaving::up();
        // Once there is a window, which is the first moment there is a
        // display connection to adopt.
        crate::clipboard::take(events, self.proxy.clone());
        // And the other windows, for the same reason: what brings one
        // forward is asked on this display.
        let here = crate::elsewhere::Here::take(events);
        app.windowed_by(Arc::new(crate::elsewhere::Elsewhere::new(
            self.proxy.clone(),
            here.can_bring(),
        )));
        self.here = Some(here);
        // And Obelus opened again from Finder, which is this process's to
        // answer, once there is a window to come forward with.
        #[cfg(target_os = "macos")]
        crate::elsewhere::again::listen(self.proxy.clone());
        // On for good there -- see `allow_the_input_method`.
        self.on_x11 = on_x11(&window);
        if self.on_x11 {
            window.set_ime_allowed(true);
            self.allowed = true;
        }
        // Before the first measuring, which leaves the bar's room above the
        // grid once the bar is the page's.
        self.bar = crate::title::drawn(&window);
        self.scale = window.scale_factor();
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
        // The screen it opened on, which is as large as a picture under the
        // page is ever drawn; a window moved to a larger one later draws
        // the picture a little soft rather than keeping one this large in
        // case. Two common screens where the platform will not say.
        let screen = window.current_monitor().map_or([3840, 2160], |monitor| {
            let size = monitor.size();
            [size.width, size.height]
        });
        app.drawn_by(Arc::new(crate::grid::Telling::new(
            frames.clone(),
            Arc::clone(&wake),
            screen,
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
        // Not shown yet: the first frame shows it -- see `shown`.
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
                let had_a_card = self.stack.iter().any(Behind::is_a_box);
                // Cloned rather than taken: a wake with no whole frame
                // in it leaves what is on the screen alone, and a band
                // that had been emptied here would be one the next frame
                // has nothing to compare against -- and one the drawing
                // stops asking about in the middle of its slide.
                let was_at = self.scrolled.clone();
                // Kept only where a band could move or a pane could go,
                // because those are the only things it is for.
                let before = (!was_at.is_empty() || !were_panes.is_empty())
                    .then(|| Arc::new(self.page.clone()));
                // And what the first frame in this drain replaced, where a
                // pane could go: the rest of what the reader last saw.
                let mut left = None;
                let mut drew = false;
                let mut sized = None;
                // Whether the bar came to hold the tabs, or stopped: a bar
                // of another height, which is a grid of another size.
                let mut retabbed = false;
                let mut faces = None;
                // A picture arrives on its own, with no frame behind it --
                // it was being decoded while the frames went by -- so it
                // asks for the screen to be drawn again by itself.
                let mut rehung = false;
                while let Ok(update) = frames.try_recv() {
                    match update {
                        // Not cells, so the page never sees them.
                        Update::TextSize(points) => sized = Some(points),
                        Update::Animates(on) => self.motion.animates(on),
                        Update::BarTabs(on) => retabbed |= crate::title::tabbed(on),
                        Update::Fonts(names) => faces = Some(names),
                        Update::Ground(ground) => {
                            // Said again after every change to the
                            // settings, and nearly always the same.
                            if ground != self.ground
                                && let Some(window) = self.window.as_ref()
                            {
                                crate::title::follow(window, ground);
                                filled_with(window, ground);
                            }
                            self.ground = ground;
                            self.page.drawn_on(ground);
                        }
                        Update::Holding { held, row } => self.holding = (held, row),
                        Update::Tabs(tabs) => self.strip.told(tabs),
                        // Kept now, while the page is the one the reader was
                        // shown: the frame in the new colours is behind this
                        // in the queue. The first of several, which is the
                        // one on the screen.
                        Update::Recoloured if self.shown && self.recolouring.is_none() => {
                            let was = Left {
                                page: Arc::new(self.page.clone()),
                                marked: self.marked.clone(),
                                capped: self.capped.clone(),
                                ticked: self.ticked.clone(),
                                spun: self.spun.clone(),
                                showing: self.showing.clone(),
                                ruled: self.ruled.clone(),
                                spelled: self.spelled.clone(),
                                sheened: self.sheened,
                                parted: self.parted.clone(),
                                stroked: self.stroked.clone(),
                                paired: self.paired.clone(),
                                stack: self.stack.clone(),
                                going: Vec::new(),
                            };
                            self.recolouring = Some(Recolouring {
                                was,
                                ground: self.ground,
                                holding: self.holding,
                            });
                        }
                        Update::Recoloured => {}
                        Update::Wallpaper { number, hung } => {
                            if number > self.hung
                                && let Some(painter) = self.painter.as_mut()
                            {
                                self.hung = number;
                                let now = Instant::now();
                                match hung {
                                    // Already going: what comes after it is
                                    // none now, and it goes on going from where
                                    // it is. Asked again, it was a picture
                                    // already gone into the page come back at
                                    // all of itself to go a second time.
                                    None if self.after_the_veil.is_some() => {
                                        self.after_the_veil = Some(None);
                                    }
                                    // Going, and down into the page's colour
                                    // first: the window waits for it before
                                    // hanging whatever comes next.
                                    None if painter.hanging() && self.motion.picture_going(now) => {
                                        self.after_the_veil = Some(None);
                                    }
                                    None => {
                                        self.after_the_veil = None;
                                        painter.hang(None);
                                    }
                                    // Decoded while the one before is still
                                    // going: it waits.
                                    Some(hung) if self.after_the_veil.is_some() => {
                                        self.after_the_veil = Some(Some(hung));
                                    }
                                    // The same file again, fitted or covered
                                    // differently: there is nothing to come
                                    // up, only the same picture redrawn.
                                    Some(hung) if painter.hanging() => {
                                        painter.hang(Some(&hung));
                                    }
                                    Some(hung) => {
                                        painter.hang(Some(&hung));
                                        self.motion.picture_hung(now);
                                    }
                                }
                                rehung = true;
                            }
                        }
                        Update::Mark {
                            id,
                            focused,
                            svg,
                            palette,
                        } => {
                            if self.recolouring.is_some() {
                                self.carried.push((id, focused, svg, palette));
                            } else if let Some(painter) = self.painter.as_mut() {
                                painter.carries(id, focused, svg, palette);
                            }
                        }
                        Update::Marked { id, focused, x, y } => self.marking.push(Marked {
                            id,
                            focused,
                            x,
                            y,
                            depth: self.stacking.len(),
                        }),
                        // Every one said is kept, in the order it was
                        // said, which is the order they are stacked in: a
                        // setting's choices open over the settings. Each is
                        // glass, and what the reader sees through one is
                        // everything said before it -- the settings kept
                        // their glass under a list once there was more
                        // than one place to keep a pane.
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
                                depth: self.stacking.len(),
                            };
                            // A pane, however many edges it is joined
                            // along. A box goes on the pile too, and what
                            // was said before either is under it.
                            if joined != Joined::Nowhere {
                                self.paning.push(joined);
                                for band in &mut self.scrolling {
                                    band.under = band.under.saturating_add(1);
                                }
                            }
                            self.stacking.push(behind);
                        }
                        Update::Scrolled { area, top, bar } => {
                            self.scrolling.push(Rolling {
                                room: area,
                                top,
                                before: None,
                                bar,
                                origin: None,
                                under: 0,
                                layer: u8::try_from(self.paning.len()).unwrap_or(u8::MAX),
                            });
                        }
                        Update::Ticked { area, on } => {
                            self.ticking.push(Ticked {
                                area,
                                on,
                                depth: self.stacking.len(),
                            });
                        }
                        Update::Spun { area } => self.spinning.push(Spun {
                            area,
                            depth: self.stacking.len(),
                        }),
                        Update::Parted { area } => {
                            self.parting.push(Parted { area });
                        }
                        Update::Sheened { area, from, to } => {
                            self.sheening = Some(Sheened {
                                area,
                                from,
                                to,
                                depth: self.stacking.len(),
                            });
                        }
                        // Each shape with how much had been put over the
                        // page when it was said, which is whose its cells
                        // are -- see `Page::layer`.
                        Update::Barred { bar } => {
                            self.barring_up.push((bar, self.stacking.len()));
                        }
                        Update::Ruled { area } => self.ruling.push(Ruled {
                            area,
                            depth: self.stacking.len(),
                        }),
                        Update::Linked { area } => {
                            self.linking.push((area, self.stacking.len()));
                        }
                        Update::Spelled { area } => {
                            self.spelling_out.push((area, self.stacking.len()));
                        }
                        Update::Paired { area, pair } => self.pairing.push(Paired {
                            area,
                            pair,
                            depth: self.stacking.len(),
                            held: None,
                        }),
                        Update::Stroked { stroke } => {
                            self.stroking.push(Stroked {
                                stroke,
                                depth: self.stacking.len(),
                            });
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
                            depth: self.stacking.len(),
                        }),
                        Update::Frame => {
                            if left.is_none()
                                && let Some(page) = before.clone()
                                && !were_panes.is_empty()
                            {
                                left = Some(Left {
                                    page,
                                    marked: std::mem::take(&mut self.marked),
                                    capped: std::mem::take(&mut self.capped),
                                    ticked: std::mem::take(&mut self.ticked),
                                    spun: std::mem::take(&mut self.spun),
                                    showing: self.showing.clone(),
                                    ruled: std::mem::take(&mut self.ruled),
                                    spelled: std::mem::take(&mut self.spelled),
                                    sheened: self.sheened.take(),
                                    parted: std::mem::take(&mut self.parted),
                                    stroked: std::mem::take(&mut self.stroked),
                                    paired: std::mem::take(&mut self.paired),
                                    stack: std::mem::take(&mut self.stack),
                                    going: Vec::new(),
                                });
                            }
                            // Whose each cell is, before anything below asks
                            // -- see `Page::layer`.
                            self.page.put_over(&self.stacking);
                            // The frame is over: what it asked for is what
                            // is on screen until the next one says
                            // otherwise.
                            self.marked = std::mem::take(&mut self.marking);
                            self.capped = std::mem::take(&mut self.capping);
                            self.ticked = std::mem::take(&mut self.ticking);
                            // Only the ones still on screen, for the
                            // light's reason: whether anything here turns
                            // is what decides the frames -- see `turning`.
                            self.spun = turning(
                                std::mem::take(&mut self.spinning),
                                &self.page,
                                &self.stacking,
                            );
                            self.motion.spin_drawn(!self.spun.is_empty());
                            self.barred = std::mem::take(&mut self.barring_up);
                            // Said here and not at the drawing: what
                            // stirs a bar is its mark being somewhere
                            // else than it was, and only the frame
                            // arriving knows that. A frame drawn twice
                            // for some other reason must not read as a
                            // reader scrolling.
                            self.motion.bars_drawn(
                                self.barred.iter().map(|&(bar, _)| bar),
                                self.pointer,
                                Instant::now(),
                            );
                            self.ruled = std::mem::take(&mut self.ruling);
                            self.linked = std::mem::take(&mut self.linking);
                            self.spelled = std::mem::take(&mut self.spelling_out);
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
                            pairs_arrived(
                                std::mem::take(&mut self.pairing),
                                &mut self.paired,
                                &mut self.unpaired,
                                &self.page,
                                &mut self.motion,
                                Instant::now(),
                            );
                            self.stack = std::mem::take(&mut self.stacking);
                            cover_the_tabs(
                                &mut self.strip,
                                &self.stack,
                                self.titled,
                                self.bar.as_ref(),
                            );
                            self.panes = std::mem::take(&mut self.paning);
                            self.scrolled = std::mem::take(&mut self.scrolling);
                            drew = true;
                        }
                        cells => drew |= self.page.apply(cells),
                    }
                }
                // And the pointer asked again: what is under it changes with
                // the page as well as with it.
                self.point_at_links();
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
                //
                // And whatever of the old pile is not in the new one went,
                // and is drawn going out of the screen it went from.
                if let Some(stayed) =
                    self.motion
                        .panes_laid(&were_panes, &self.panes, Instant::now())
                    && let Some(mut left) = left
                {
                    left.going = Left::going(&left.stack, stayed);
                    self.left = Some(left);
                }
                // And a box over the page, the same way and for the same
                // reason. Whether there is one rather than which one:
                // a completion list redrawn on every character the reader
                // types is the same box, and one that came up again on
                // each of them would be a flicker under their hands.
                match (had_a_card, !self.stack.iter().any(Behind::is_a_box)) {
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
                        .band_moved(band.lane(), rows, f32::from(band.room.height), now)
                    {
                        band.origin = was.bar.map(|bar| bar.mark);
                        band.before.clone_from(&before);
                    }
                }
                // And what is no longer on the screen is no longer kept.
                let lanes: Vec<Lane> = self.scrolled.iter().map(Rolling::lane).collect();
                self.motion.bands_drawn(&lanes);
                let refaced = faces.is_some_and(|names| {
                    self.fonts
                        .as_mut()
                        .is_some_and(|fonts| fonts.use_families(&names))
                });
                let regrid = Regrid::from(refaced, sized, self.points);
                // A cell of another size is a screen the kept one was not
                // laid out for, and a pane cut from it would be cut from the
                // wrong place: what was leaving is simply gone, as it is
                // when the window is resized.
                if regrid.moved() {
                    self.left = None;
                    self.recolouring = None;
                }
                if regrid.refaced {
                    // Which faces text is drawn in decides how wide a cell
                    // is, so this is the same work a new size is: measure
                    // again, throw away the glyphs, and tell the
                    // application what the grid has become.
                    if let Some(painter) = self.painter.as_mut() {
                        painter.forget_the_glyphs();
                    }
                    self.remeasure();
                    self.tell(Event::Resize);
                    self.redraw();
                }
                if let Some(points) = regrid.resized {
                    self.redraw_at(points);
                }
                if retabbed {
                    self.left = None;
                    self.remeasure();
                    self.tell(Event::Resize);
                    self.redraw();
                }
                if drew {
                    self.allow_the_input_method();
                    self.point_the_input_method();
                    // Shown here, by the window's own thread on its way back
                    // to answering the system, which is what the taskbar
                    // needs on Windows -- see `hidden`. And drawn by the
                    // redraw like any other frame rather than on the spot,
                    // which would keep the thread from the taskbar's
                    // question for as long as the first page takes.
                    if !self.shown
                        && let Some(window) = self.window.as_ref()
                    {
                        // Whatever the ground: one that is the reset colour
                        // is the colour `ground` starts at, which a change
                        // to it never reaches.
                        filled_with(window, self.ground);
                        window.set_visible(true);
                        self.shown = true;
                        // Asked rather than assumed: a window the system
                        // brought up behind the one the reader is in is
                        // never told it lost the keys, because it never had
                        // them -- see `elsewhere`. And `Focused(true)` says
                        // so when it does get them.
                        let focused = window.has_focus();
                        self.active = focused;
                        self.motion.focused(focused, Instant::now());
                    }
                    self.redraw();
                } else if rehung && self.shown {
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
            #[cfg(target_os = "linux")]
            Waking::Dropped(path) => self.tell(Event::Dropped(path)),
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
        // The tabs sliding to where an arrow sent them, which is a clock of
        // their own and asked here the same way: from whether they are
        // still on their way.
        let sliding = self.strip.sliding();
        let wake = match sliding {
            true => Some(Wake::EveryFrame),
            false => self.motion.wake(now, caret),
        };
        if wants_a_frame(
            self.motion.advance(now, caret) || sliding,
            wake,
            self.covered,
        ) {
            self.redraw();
        }
        events.set_control_flow(control_flow(wake));
    }

    fn window_event(&mut self, events: &ActiveEventLoop, _window: WindowId, event: WindowEvent) {
        match event {
            // The close button means what the key that leaves means, asking
            // about unwritten files included. So the window does not close
            // here: Obelus is asked, and the window goes when Obelus says
            // it is done.
            WindowEvent::CloseRequested => self.tell(Event::Closed),
            // One for each file, where several are dropped at once. Never
            // on Wayland, where winit does not take part in a drop and the
            // clipboard's own connection hears it instead.
            WindowEvent::DroppedFile(path) => self.tell(Event::Dropped(path)),
            WindowEvent::ActivationTokenDone { serial, token } => {
                if let Some(at) = self.going.iter().position(|(asked, _)| *asked == serial) {
                    let (_, going) = self.going.remove(at);
                    crate::elsewhere::go(going, Some(token.into_raw()));
                }
            }
            WindowEvent::RedrawRequested => {
                // Nothing until the first frame has shown the window -- see
                // `shown`. Nor counted as a wait: what kept it waiting was
                // the application starting, not the screen.
                if !self.shown {
                    self.asked.take();
                    return;
                }
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
                self.showing
                    .extend(self.barred.iter().map(|&(bar, depth)| Barred {
                        bar,
                        depth,
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
                        let (behind, since) = self.motion.band_shown(band.lane(), now)?;
                        Some(Rolled {
                            room: band.room,
                            under: band.under > 0,
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
                // The screen in the theme that went, before the painter is
                // handed anything of the new one -- see `Painter::recolour`.
                let now = Instant::now();
                if let Some(Recolouring {
                    was,
                    ground,
                    holding,
                }) = self.recolouring.take()
                {
                    let going = self.motion.moving(now).recoloured;
                    if self.motion.recolouring(now) {
                        painter.drawn_on(ground);
                        painter.holding(holding);
                        painter.titled(self.titled);
                        painter.recolour(&was.page, fonts, was.said(), going);
                    }
                }
                // Whether or not it was kept: a screen of another size lets
                // go of it without a redraw, and these are the new theme's.
                for (id, focused, svg, palette) in self.carried.drain(..) {
                    painter.carries(id, focused, svg, palette);
                }
                // The picture that went has gone down into the page's colour,
                // so what takes its place is hung, and comes up.
                if self.after_the_veil.is_some() && !self.motion.picture_still_going(now) {
                    let next = self.after_the_veil.take().flatten();
                    painter.hang(next.as_deref());
                    if next.is_some() {
                        self.motion.picture_hung(now);
                    }
                }
                painter.unveiled(self.motion.picture_shown(now));
                painter.drawn_on(self.ground);
                painter.titled(self.titled);
                let drawn = self
                    .bar
                    .as_ref()
                    .zip(self.window.as_ref())
                    .map(|(bar, window)| bar.drawn(window, self.active));
                // Laid out here, with the frame, because where the tabs go
                // is a question about the window's width and the faces,
                // and both are the frame's.
                let room = self.window.as_ref().and_then(|window| {
                    #[expect(clippy::cast_precision_loss, reason = "pixels on a screen")]
                    let width = window.inner_size().width as f32;
                    crate::title::room(window, self.titled, width, drawn.as_ref())
                });
                // Again here, for a bar whose height changed with nothing
                // said -- full screen, a new density.
                cover_the_tabs(&mut self.strip, &self.stack, self.titled, self.bar.as_ref());
                self.strip.lay(
                    room,
                    |said, size| fonts.caption_width(said, size),
                    Instant::now(),
                    self.motion.animated(),
                );
                if let Some(bar) = self.bar.as_ref() {
                    bar.tabs_are(self.strip.covers());
                }
                painter.bar(drawn);
                painter.tabs(self.strip.painted(self.active));
                painter.holding(self.holding);
                if let Some(left) = self.left.take() {
                    painter.keep(&left.page, fonts, left.said(), &left.going);
                }
                // One moment for the whole frame, so each pair going out
                // is as far out as the clock that says how far the rest is.
                let now = Instant::now();
                let fading: Vec<(Paired, f32)> = self
                    .unpaired
                    .iter()
                    .filter_map(|half| self.motion.fading(half.pair, now).map(|lit| (*half, lit)))
                    .collect();
                if let Err(error) = painter.paint(
                    &self.page,
                    fonts,
                    self.motion.moving(now),
                    Said {
                        marked: &self.marked,
                        capped: &self.capped,
                        ticked: &self.ticked,
                        spun: &self.spun,
                        barred: &self.showing,
                        ruled: &self.ruled,
                        spelled: &self.spelled,
                        sheened: self.sheened.as_ref(),
                        parted: &self.parted,
                        stroked: &self.stroked,
                        paired: &self.paired,
                        fading: &fading,
                        stack: &self.stack,
                        bands: &rolled,
                    },
                ) {
                    tracing::error!(?error, "the frame was not drawn");
                }
            }
            WindowEvent::Resized(size) => {
                // A screen of the old size, which a pane would leave from
                // the wrong place: what was leaving is simply gone.
                self.left = None;
                self.recolouring = None;
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
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                mut inner_size_writer,
            } => {
                // The page asked for less the bar, where the bar is the
                // page's -- see `title::rescaled`. Not when maximised,
                // which winit leaves the size of and the screen decides.
                if let (Some(bar), Some(window)) = (self.bar.as_ref(), self.window.as_ref())
                    && !window.is_maximized()
                {
                    let old = window.inner_size();
                    let (width, height) = crate::title::rescaled(
                        (old.width, old.height),
                        self.scale,
                        scale_factor,
                        bar.height(window),
                    );
                    let _ = inner_size_writer
                        .request_inner_size(winit::dpi::PhysicalSize::new(width, height));
                }
                self.scale = scale_factor;
                self.redraw_at(self.points);
            }
            // Uncovered, the window is drawn as things are now: what moved
            // while it was covered moved on its clock, and only the frames
            // were left out -- see `wants_a_frame`.
            WindowEvent::Occluded(covered) => {
                self.covered = covered;
                if !covered {
                    self.centre_the_buttons();
                    self.redraw();
                }
            }
            // Drawn again for the caret, which a window without the keys
            // does not show, and for a bar Obelus draws, whose marks are
            // fainter there.
            WindowEvent::Focused(true) => {
                self.active = true;
                self.centre_the_buttons();
                if self.motion.focused(true, Instant::now()) || self.bar.is_some() {
                    self.redraw();
                }
            }
            // A window losing the reader takes what they were holding with
            // it: the key comes up somewhere else, and nothing tells this
            // window it did -- so shift held on the way out was held for
            // good, and every press in a terminal after it took hold of
            // words rather than reaching the program.
            WindowEvent::Focused(false) => {
                self.active = false;
                if self.motion.focused(false, Instant::now()) || self.bar.is_some() {
                    self.redraw();
                }
                if self.modifiers.shift_key() {
                    self.tell(Event::Shifted(false));
                }
                self.modifiers = ModifiersState::empty();
                // And a word half spelled: some input methods drop it
                // without a word when the window goes, and its letters are
                // written in the reader's text, so they would stay there.
                self.give_up_spelling();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let shifted = modifiers.state().shift_key();
                if shifted != self.modifiers.shift_key() {
                    self.tell(Event::Shifted(shifted));
                }
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(ime) => {
                self.stir();
                match ime {
                    // The spelling so far, written in where the word will go.
                    // An input method draws a window of its own for the
                    // candidates and Obelus leaves that alone; what it will
                    // not leave alone is *where the word is being typed*,
                    // which is in the box with the words it is going among.
                    Ime::Preedit(spelling, caret) => {
                        // As it arrived: what an input method sends differs
                        // by input method and by compositor, and is otherwise
                        // a question with no answer to look at.
                        tracing::debug!(?spelling, ?caret, "the input method is spelling");
                        // Where nothing was being spelled and nothing is, an
                        // input method saying so is news to nobody.
                        if self.composing || !spelling.is_empty() {
                            self.composing = !spelling.is_empty();
                            let caret = caret_in(&spelling, caret);
                            self.tell(Event::Spelling(spelling, caret));
                        }
                    }
                    // The word. It goes in the way pasted text goes in --
                    // wherever the reader is writing, as one change -- because
                    // that is the same question and it is already answered.
                    // An empty spelling has come first and taken the letters
                    // back out.
                    Ime::Commit(word) => {
                        self.composing = false;
                        self.tell(Event::Paste(word));
                    }
                    Ime::Enabled | Ime::Disabled => self.give_up_spelling(),
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if !keys::heard(event.state, is_synthetic) {
                    return;
                }
                self.stir();
                #[cfg(target_os = "macos")]
                if let Some(asked) = keys::asked_of_the_window(
                    &event.logical_key,
                    event.physical_key,
                    self.modifiers,
                ) {
                    use winit::platform::macos::ActiveEventLoopExtMacOS;
                    match asked {
                        keys::Asked::Hide => events.hide_application(),
                        keys::Asked::HideOthers => events.hide_other_applications(),
                        keys::Asked::Minimize => {
                            if let Some(window) = self.window.as_ref() {
                                window.set_minimized(true);
                            }
                        }
                    }
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
                self.tell(match keys::by_command(self.modifiers) {
                    true => Event::Command(key),
                    false => Event::Key(key),
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.over_the_title = crate::title::under(position.y, self.titled);
                #[expect(clippy::cast_possible_truncation, reason = "pixels on a screen")]
                let over = (self.over_the_title && !self.held).then_some(position.x as f32);
                if self.strip.point(over) {
                    self.redraw();
                }
                // Off the page, the way leaving the window is, so that a
                // link on the first row does not put a hand over the bar.
                // Not while a drag is held: one that goes up past the page
                // is still choosing on its first row.
                if self.over_the_title && !self.held {
                    self.pointer = None;
                    self.point_at_links();
                    return;
                }
                let Some(at) = self.cell_at(position) else {
                    return;
                };
                if self.pointer == Some(at) {
                    return;
                }
                self.pointer = Some(at);
                self.point_at_links();
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
                // Asked again at every press rather than cleared at the
                // release: a press that drags the window by its bar may
                // never be let go of in the window at all.
                if state == ElementState::Pressed {
                    self.pressed_on_the_title = self.over_the_title;
                }
                // A tab, or an arrow walking them, on the press: what a tab
                // does is not something a reader drags off to take back.
                // And not counted towards a double click, which on the bar
                // zooms the window: two quick presses on two tabs are two
                // choices.
                if self.pressed_on_the_title
                    && let Some(spot) = self.strip.hovered()
                {
                    if state == ElementState::Pressed {
                        self.press_on_the_tabs(spot);
                    }
                    return;
                }
                if self.pressed_on_the_title {
                    if state == ElementState::Pressed
                        && let Some(window) = self.window.as_ref()
                    {
                        self.title_presses.pressed(window, Instant::now());
                    }
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
            // The wheel over the tabs scrolls them: the page under the bar
            // is not what the reader is pointing at. Whichever way the
            // wheel turns further, so a wheel that only turns down still
            // walks a row that runs across.
            WindowEvent::MouseWheel { delta, .. } if self.over_the_title => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(across, down) => match across.abs() > down.abs() {
                        true => across,
                        false => -down,
                    },
                    // Pixels, from a touchpad, as the part of a notch they
                    // are: the row is scrolled by the pixel either way.
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_precision_loss,
                        reason = "pixels under a finger, and a notch of three rows"
                    )]
                    MouseScrollDelta::PixelDelta(moved) => {
                        let height = self.fonts.as_ref().map_or(1.0, |fonts| fonts.cell().height);
                        let moved = match moved.x.abs() > moved.y.abs() {
                            true => moved.x as f32,
                            false => -moved.y as f32,
                        };
                        moved / (height * NOTCH as f32)
                    }
                };
                if notches != 0.0 && self.strip.scroll(notches) {
                    self.redraw();
                }
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
            // Out of the window -- or, on Windows, onto the part of the bar
            // that drags it, which the window does not hear the pointer on.
            WindowEvent::CursorLeft { .. } => {
                if self.strip.point(None) {
                    self.redraw();
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

/// What a frame said about the cursor's pair of brackets, taken as the
/// frame arrives: the pair it said, where the page still holds it, and
/// the pairs that were, kept for their light to go out on.
///
/// Said here rather than at the drawing, for the light's reason: whether
/// the pair is another one than the last frame drew is what starts it
/// coming on or going out, and only the frame arriving knows that. And
/// kept only where the caret left it rather than the text moving it, only
/// while the cell is still that bracket, and not once it has gone out --
/// so what is kept is what is going rather than what once went. Added to
/// rather than replaced, because each goes out on its own clock: the
/// pair left two presses ago is still going out when the next one goes.
fn pairs_arrived(
    said: Vec<Paired>,
    paired: &mut Vec<Paired>,
    unpaired: &mut Vec<Paired>,
    page: &Page,
    motion: &mut Motion,
    now: Instant,
) {
    let was = std::mem::replace(paired, said);
    paired.retain(|half| half.still_said(page));
    for half in paired.iter_mut() {
        half.hold(page);
    }
    if motion.pairs_drawn(paired.iter().map(|half| half.pair), now) {
        unpaired.extend(was);
    }
    unpaired.retain(|half| half.still_there(page) && motion.fading(half.pair, now).is_some());
}

/// Whether a cell is among the words of a link the frame said, and still
/// is.
///
/// Asked of the cells as well as of what the view said, because the cells
/// are the truth: a link a list was opened over was said and is not on the
/// page, and a hand over the list's row would be a promise the row does not
/// keep. A link's words are underlined, and a cell that is not is somebody
/// else's.
fn on_a_link(linked: &[(Rect, usize)], page: &Page, x: u16, y: u16) -> bool {
    linked.iter().any(|&(area, depth)| {
        area.contains(ratatui::layout::Position { x, y }) && page.layer(x, y) <= depth
    }) && page
        .look(x, y)
        .modifier
        .contains(ratatui::style::Modifier::UNDERLINED)
}

/// The marks that turn which are still on screen: on the page, or in
/// the picture of what a pane or a box was put over, where that picture is
/// drawn at all -- see `paint::drawn`.
///
/// The second half because the glass shows that picture. A transcript an
/// agent was writing in, with a file picker opened over it, is still seen
/// through the picker -- and a mark dropped here for not being on the page
/// was the braille the window draws instead of, and stood still: in a
/// window the application leaves the turning to this clock, which a mark
/// nobody kept never started. A mark that stands still says nothing is
/// happening.
fn turning(spinning: Vec<Spun>, page: &Page, stack: &[Behind]) -> Vec<Spun> {
    spinning
        .into_iter()
        .filter(|mark| {
            mark.round(page).is_some()
                || crate::paint::drawn(stack, page)
                    .iter()
                    .any(|over| mark.behind(over).is_some())
        })
        .collect()
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

/// A window kept off the screen until there is a page on it -- see
/// `Showing::shown`.
///
/// Everywhere, and on Windows for a second reason as well. The taskbar
/// asks a window for its icon the moment it is shown, and gives up on one
/// that does not answer: it draws the system's blank program and does not
/// ask again until something else about the button changes. A window shown
/// as it is made is shown a second before this thread is done loading the
/// faces and starting the drawing, and that second is when it was asked --
/// measured by putting a progress bar on the blank button from outside,
/// which made the taskbar look again and find the icon that had been on the
/// window all along.
///
/// Showing it later on macOS does not take the reader from whatever they
/// turned to while it started: a window ordered front there comes in front
/// of its own program's windows only, unless that program is the one the
/// reader is in. Wayland has no hidden window at all, and asking for one
/// is ignored: a surface appears with the first thing drawn on it, which is
/// why the drawing waits as well.
fn hidden(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attributes.with_visible(false)
}

/// The icon, as the file itself. Fourteen kilobytes, seven sizes, and the
/// decoder hands back the largest of them.
const MARK: &[u8] = include_bytes!("../../../contrib/desktop/obelus.ico");

/// The mark the window wears.
///
/// X11 is who needs it. A Wayland compositor takes the icon from the
/// desktop entry the name above points it at, and macOS from the bundle,
/// so on those two this is dropped. Windows takes it twice, because it
/// keeps two: winit's window icon is only `ICON_SMALL`, the one in the
/// title bar, and the taskbar and alt-tab read `ICON_BIG`, which winit
/// clears unless it is given a taskbar icon as well -- and its window
/// class has none to fall back on, so the executable's own resources are
/// never asked. When the taskbar asks is another matter: see `hidden`.
///
/// Set on all of them rather than behind a `cfg` for each: a platform that
/// does not want it drops it, and three cfgs would be three places to be
/// wrong about somebody else's rules. The taskbar's is the one `cfg`,
/// because only Windows has the method. Read from the file rather than
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
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attributes.with_taskbar_icon(icon.clone())
    };
    attributes.with_window_icon(icon)
}

/// What the faces and the size said with a batch of frames do to the grid.
///
/// Both are said again after every change to the settings, and are nearly
/// always what they already are -- so only a real change counts as one.
/// Counting the saying was a change of theme from the settings page with
/// no recolouring: the theme is a setting too, and the same change of
/// settings said the faces and the size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Regrid {
    /// Whether the faces are other ones, as `Fonts::use_families` says.
    refaced: bool,
    /// The size the text is to be drawn at, where it is another one.
    resized: Option<f32>,
}

impl Regrid {
    /// From whether the faces changed, the size said if one was, and the
    /// size the text is drawn at now.
    fn from(refaced: bool, sized: Option<usize>, points: f32) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a size in points, which is two digits"
        )]
        let resized = sized
            .map(|sized| sized as f32)
            .filter(|sized| (sized - points).abs() > f32::EPSILON);
        Self { refaced, resized }
    }

    /// Whether a cell is another size, so that what was kept of the screen
    /// before was laid out for a grid that is not there any more.
    const fn moved(self) -> bool {
        self.refaced || self.resized.is_some()
    }
}

/// How many characters of a spelling come before the input method's own
/// caret, or `None` for after all of them.
///
/// `caret` is as winit hands it on: a run of bytes, or nothing for a caret
/// the input method would rather not show. A run whose two ends meet is a
/// caret; one whose ends differ is text the input method is highlighting,
/// and the caret is its *far* end, which is where typing carries on from.
/// fcitx5 says `(0, 6)` for `ni hao` -- the whole of it, ending where the
/// next letter goes -- and a caret taken from the near end sat at the front
/// of the pinyin however much was typed.
///
/// In characters rather than bytes: the application counts where it goes
/// in a text, and a byte offset into what is being spelled is not one.
fn caret_in(spelling: &str, caret: Option<(usize, usize)>) -> Option<usize> {
    let (_, end) = caret?;
    Some(
        spelling
            .get(..end)
            .map_or(0, |before| before.chars().count()),
    )
}

#[cfg(test)]
mod tests {
    /// The caret in a spelling is at the far end of what the input method
    /// named, which is where the next letter goes.
    ///
    /// Deliberate break: take the near end, `Some((start, _))`, as this
    /// once did. The first case below is what fcitx5 actually sent, read
    /// off the log, and with the near end the caret sat at the front of
    /// the pinyin however much of it had been typed.
    #[test]
    fn a_spelling_carries_on_from_the_far_end_of_what_is_named() {
        use super::caret_in;
        assert_eq!(
            caret_in("ni hao", Some((0, 6))),
            Some(6),
            "fcitx5's whole run"
        );
        assert_eq!(
            caret_in("ni hao", Some((2, 2))),
            Some(2),
            "a caret of its own"
        );
        assert_eq!(caret_in("ni hao", None), None, "a caret not shown");
        // Bytes in, characters out: two characters of Chinese are six bytes.
        assert_eq!(caret_in("\u{4f60}\u{597d}hao", Some((0, 6))), Some(2));
    }

    use std::time::{Duration, Instant};

    use ratatui::layout::Rect;
    use winit::event_loop::ControlFlow;

    use super::{
        MARK, Regrid, Rolling, control_flow, on_a_link, pairs_arrived, turning, wants_a_frame,
    };
    use crate::{
        grid::{Behind, Page, Spun, Update},
        motion::{Motion, Wake},
    };

    /// The faces and the size said again as they are leave the grid -- and
    /// what was kept of the screen with it, a change of theme's included --
    /// while either one changed moves it.
    ///
    /// Deliberate breaks: `from` keeping a size said as the one there is
    /// (the size said again then moves the grid), and `moved` leaving the
    /// faces out (other faces then keep a screen laid out for the old ones).
    #[test]
    fn the_faces_and_the_size_said_again_leave_the_grid() {
        assert!(!Regrid::from(false, None, 14.0).moved(), "nothing said");
        assert!(
            !Regrid::from(false, Some(14), 14.0).moved(),
            "the size said again"
        );
        assert_eq!(Regrid::from(false, Some(16), 14.0).resized, Some(16.0));
        assert!(Regrid::from(false, Some(16), 14.0).moved(), "another size");
        assert!(Regrid::from(true, Some(14), 14.0).moved(), "other faces");
        assert_eq!(Regrid::from(true, Some(14), 14.0).resized, None);
    }

    /// The pair the caret left is kept for its light to go out on, while
    /// its cell is still that bracket, and let go of once it has gone out.
    ///
    /// Deliberate breaks: drop the `unpaired.extend(was)` and there is
    /// nothing to go out, or replace with it and the pair left first is
    /// put out by the next; drop the `still_there` and the light goes out
    /// on the `x` the file scrolled into the `(`'s cell; drop the `fading`
    /// and what went out is kept for good.
    #[test]
    fn the_pair_the_caret_left_is_kept_while_it_goes_out() {
        let write = |page: &mut Page, x: u16, text: &str| {
            let mut cell = ratatui::buffer::Cell::default();
            cell.set_symbol(text);
            page.apply(Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell),
            });
        };
        let mut page = Page::default();
        page.resized(6, 1);
        for (x, text) in [(0, "("), (1, "a"), (2, ")"), (3, "("), (4, "b"), (5, ")")] {
            write(&mut page, x, text);
        }
        let half = |x, open| crate::grid::Paired {
            area: Rect::new(x, 0, 1, 1),
            pair: obelus_ui::shapes::Pair {
                document: 1,
                version: 1,
                open,
            },
            depth: 0,
            held: None,
        };
        let xs = |halves: &[crate::grid::Paired]| -> Vec<u16> {
            halves.iter().map(|half| half.area.x).collect()
        };
        let mut motion = Motion::new(None);
        let (mut paired, mut unpaired) = (Vec::new(), Vec::new());
        let mut arrive = |said, page: &Page, at| {
            pairs_arrived(said, &mut paired, &mut unpaired, page, &mut motion, at);
            (xs(&paired), xs(&unpaired))
        };
        let base = Instant::now();
        assert_eq!(
            arrive(vec![half(0, 0), half(2, 0)], &page, base),
            (vec![0, 2], vec![])
        );

        // Into the second pair: the first is kept to go out.
        let later = base + Duration::from_millis(500);
        assert_eq!(
            arrive(vec![half(3, 3), half(5, 3)], &page, later),
            (vec![3, 5], vec![0, 2]),
            "nothing kept to go out"
        );
        // And out of it while the first is still going: both are kept.
        let on = later + Duration::from_millis(30);
        assert_eq!(
            arrive(Vec::new(), &page, on),
            (vec![], vec![0, 2, 3, 5]),
            "the first was put out by the second"
        );

        // The file scrolled under the first `(`.
        write(&mut page, 0, "x");
        assert_eq!(
            arrive(Vec::new(), &page, on).1,
            vec![2, 3, 5],
            "a light on what scrolled in"
        );

        // And once they have gone out, nothing is kept.
        assert!(
            arrive(Vec::new(), &page, on + Duration::from_secs(1))
                .1
                .is_empty(),
            "kept after it went out"
        );
    }

    /// A mark that turns is kept while the page holds it or a pane's
    /// picture does, and not otherwise.
    ///
    /// Deliberate break: keep only `mark.round(page)`, which is what this
    /// was -- and the transcript's mark behind a file picker went back to
    /// braille, though the picture under the glass knew how to draw it.
    #[test]
    fn a_mark_behind_the_glass_is_still_turning() {
        let frame = |text: &str| {
            let mut cell = ratatui::buffer::Cell::default();
            cell.set_symbol(text);
            cell
        };
        let mut page = Page::default();
        page.resized(3, 1);
        // The page has the mark in its first cell, and a list's letters
        // where the second and third were.
        for (x, text) in ["\u{2819}", "a", "b"].into_iter().enumerate() {
            page.apply(Update::Cell {
                x: u16::try_from(x).expect("three"),
                y: 0,
                cell: Box::new(frame(text)),
            });
        }
        // And what the list was put over: the transcript's mark in the
        // second, and a letter in the third.
        let behind = Behind {
            area: Rect::new(1, 0, 2, 1),
            joined: obelus_ui::shapes::Joined::Below,
            ground: ratatui::style::Color::Reset,
            cells: vec![frame("\u{2819}"), frame("c")],
            depth: 0,
        };
        let one = |x| Spun {
            area: Rect::new(x, 0, 1, 1),
            depth: 0,
        };
        let kept: Vec<u16> = turning(
            vec![one(0), one(1), one(2)],
            &page,
            std::slice::from_ref(&behind),
        )
        .iter()
        .map(|mark| mark.area.x)
        .collect();
        assert_eq!(kept, vec![0, 1], "on the page, and behind the glass");

        // And a second pane over the whole of the first, which makes the
        // first a picture nobody sees: its mark is not on screen.
        //
        // Deliberate break: ask `stack` rather than `paint::drawn(stack,
        // page)` in `turning`, and the hidden mark keeps the window drawing.
        let over = Behind {
            cells: vec![frame("d"), frame("e")],
            depth: 1,
            ..behind.clone()
        };
        let kept: Vec<u16> = turning(vec![one(0), one(1), one(2)], &page, &[behind, over])
            .iter()
            .map(|mark| mark.area.x)
            .collect();
        assert_eq!(kept, vec![0], "only on the page");
    }

    /// The pointer is a hand over the cells of a link the frame said, while
    /// they are still underlined, and nowhere else -- not over a link a
    /// list was opened over, and not over an underline that is somebody
    /// else's, like a server's complaint.
    ///
    /// Deliberate breaks: leaving the cells out of `on_a_link` makes the
    /// covered link a hand, and leaving the view's word out of it makes
    /// the complaint one.
    #[test]
    fn the_pointer_is_a_hand_over_a_link_on_the_page() {
        let mut page = Page::default();
        page.resized(6, 1);
        for x in 0..6 {
            let mut cell = ratatui::buffer::Cell::default();
            cell.set_symbol("x");
            // A link in the first three, a covered one in the fourth, and
            // a complaint in the last two.
            if x != 3 {
                cell.modifier = ratatui::style::Modifier::UNDERLINED;
            }
            page.apply(Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell),
            });
        }
        let linked = [(Rect::new(0, 0, 4, 1), 0)];
        assert!(on_a_link(&linked, &page, 1, 0), "not a hand over the link");
        assert!(
            !on_a_link(&linked, &page, 3, 0),
            "a hand over a covered link"
        );
        assert!(!on_a_link(&linked, &page, 4, 0), "a hand over a complaint");
        // And a dialog put over it that underlines words of its own in the
        // same cells: the welcome screen's link is under the settings.
        // Deliberate break: take `page.layer` out of `on_a_link`.
        page.put_over(&[crate::grid::Behind {
            area: Rect::new(0, 0, 6, 1),
            joined: obelus_ui::shapes::Joined::Screen,
            ground: ratatui::style::Color::Reset,
            cells: Vec::new(),
            depth: 0,
        }]);
        assert!(
            !on_a_link(&linked, &page, 1, 0),
            "a hand over a dialog's own words"
        );
    }

    /// A band is matched with the one it was, not with another in the same
    /// rows: a dialog's list over a page's that happens to sit where it does
    /// -- the settings over a conversation -- is two bands.
    ///
    /// Deliberate break: match on `room` alone in `Rolling::is`. The
    /// settings' list is then taken for the transcript under it, which is at
    /// its top while the list is not, and it slid on every frame.
    ///
    /// The bands are written here as the drain would count them, and the
    /// counting is not under test: it happens where a window takes a
    /// frame, and setting `layer` to nothing there passes this. What was
    /// checked by hand is that it fails in the window -- the settings
    /// slide with a transcript scrolling under them.
    #[test]
    fn a_band_under_a_pane_is_not_the_pane_s() {
        let band = |top, under, layer| Rolling {
            room: Rect::new(0, 2, 75, 18),
            top,
            before: None,
            bar: None,
            origin: None,
            under,
            layer,
        };
        let was = [band(0, 1, 0), band(4, 0, 1)];
        let settings = band(4, 0, 1);
        assert_eq!(
            was.iter().find(|was| settings.is(was)).map(|was| was.top),
            Some(4),
            "the list was taken for the transcript under it"
        );
        let transcript = band(0, 1, 0);
        assert_eq!(
            was.iter().find(|was| transcript.is(was)).map(|was| was.top),
            Some(0),
            "the transcript was taken for the list over it"
        );
        // And on the frame the settings open, the frame before has only
        // the transcript, with nothing over it -- which is what the
        // settings' list has over it too. Deliberate break: match on
        // `under` rather than `layer`, which is what this was. The list
        // is then the transcript a frame on, takes its slide and the page
        // of the conversation it kept, and draws that page sliding.
        let was = [band(3, 0, 0)];
        let settings = band(0, 0, 1);
        assert!(
            !was.iter().any(|was| settings.is(was)),
            "the settings' list was taken for the transcript it opened over"
        );
        // And on the frame they close, the transcript is not the one that
        // was under them: what it kept while they were up is a picture with
        // the settings in its rows. Deliberate break: leave `under` out of
        // `Rolling::is`, and the transcript carries that page out and slides
        // it into the conversation.
        let was = [band(3, 1, 0), band(0, 0, 1)];
        let transcript = band(3, 0, 0);
        assert!(
            !was.iter().any(|was| transcript.is(was)),
            "the transcript took the page it kept under the settings out with it"
        );
    }

    /// A pass that wants a rate asks for the frame it wants.
    ///
    /// Otherwise nothing ever arrives: the loop sleeps until a frame wakes
    /// it, and a frame nobody asked for is never let through. When the
    /// loop polled instead, the same mistake was a busy wait that drew
    /// nothing -- the welcome screen sat at a hundred per cent behind a
    /// frozen picture -- and now it would be a light that never moves.
    ///
    /// The second half is why the first cannot be `advance` alone, and it
    /// is asked of `Motion` rather than assumed: the light is out, so the
    /// window wants every frame, and nothing stepped to say so.
    ///
    /// Deliberate break: drop the `EveryFrame` arm from `wants_a_frame`.
    /// The third assertion goes red, which is exactly the case this is
    /// about -- in flight, and nothing stepped for it.
    #[test]
    fn a_pass_that_wants_a_rate_asks_for_the_frame_it_wants() {
        assert!(!wants_a_frame(false, None, false), "nothing is moving");
        assert!(wants_a_frame(true, None, false), "something stepped");
        assert!(
            wants_a_frame(false, Some(Wake::EveryFrame), false),
            "in flight, and nothing stepped for it"
        );
        assert!(
            !wants_a_frame(false, Some(Wake::At(Instant::now())), false),
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

    /// Something in flight sleeps between frames, the way nothing at all
    /// does, and a moment is slept until.
    ///
    /// Asked with the light across the welcome screen's mark, which is
    /// what held a core: a window on another workspace is let draw no
    /// frame at all, and a loop that polled for one spun the whole time
    /// the light was out.
    ///
    /// Deliberate break: answer `ControlFlow::Poll` for `EveryFrame` in
    /// `control_flow`, as it did. The first assertion goes red.
    #[test]
    fn a_window_waiting_for_a_frame_sleeps_until_the_screen_lets_it_draw() {
        let mut motion = Motion::new(None);
        let now = Instant::now();
        motion.sheen_drawn(true, now);
        // Asked first, or a light that stopped asking for a rate would pass
        // the assertion after it for the reason the next one does.
        let wake = motion.wake(now, false);
        assert_eq!(wake, Some(Wake::EveryFrame), "the light is out");
        assert_eq!(
            control_flow(wake),
            ControlFlow::Wait,
            "and the loop sleeps until its frame"
        );
        assert_eq!(control_flow(None), ControlFlow::Wait, "nothing moving");
        let later = now + Duration::from_secs(1);
        assert_eq!(
            control_flow(Some(Wake::At(later))),
            ControlFlow::WaitUntil(later),
            "a moment"
        );
    }

    /// A window nobody can see asks for no frame, however much is moving,
    /// and is drawn again once it can be seen.
    ///
    /// The light again, because it is what moves on a screen left alone,
    /// and a covered window on macOS is told at once that there is nothing
    /// to draw on: a frame asked for on every pass is a loop going round
    /// flat out for the whole of every pass of the light.
    ///
    /// Deliberate break: drop `!covered &&` from `wants_a_frame`. The
    /// first assertion goes red. The coming back is not asked here -- it
    /// is the `Occluded(false)` arm asking for a frame, which needs a
    /// window to send it.
    #[test]
    fn a_window_nobody_can_see_asks_for_no_frame() {
        let mut motion = Motion::new(None);
        let now = Instant::now();
        motion.sheen_drawn(true, now);
        let wake = motion.wake(now, false);
        assert_eq!(wake, Some(Wake::EveryFrame), "the light is out");
        assert!(
            !wants_a_frame(motion.advance(now, false), wake, true),
            "and nobody can see it"
        );
        assert!(
            !wants_a_frame(true, None, true),
            "nor for something that stepped"
        );
        assert!(
            wants_a_frame(motion.advance(now, false), wake, false),
            "until somebody can"
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
