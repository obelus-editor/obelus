//! One Obelus for one window at the other end of a connection: `ob
//! --serve-one`, which `ob --server` starts and nobody else does.
//!
//! **Its standard input and output are the connection.** What it draws goes
//! out as [`Down`]s and what the reader does comes in as [`Up`]s, through
//! `ob --server`, which knocked and opened the door and carries the bytes
//! between this and the connection from then on. It never writes anything
//! else to its output, which is the one place Obelus writes to stdout at
//! all -- the frame a terminal would draw on is the window's, and is not
//! here. Its log goes where every Obelus's goes.
//!
//! **It goes on while nobody is watching.** A window whose connection goes
//! leaves this running -- the agent at work, the terminals, the files open
//! as they were -- and the next window to come in under the same name is
//! told all of what is on the screen ([`Screen`]) rather than only what
//! changes next. While nobody is there, what only the reader's machine can
//! answer -- what is on their clipboard -- is answered with nothing.

use std::{
    collections::{HashMap, VecDeque},
    io::Write as _,
    process::ExitCode,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_app::{app, event::Event, startup};
use obelus_front::{
    Closed, Measured, Showing, Update,
    link::{Arriving, Down, Front, PIECE, ToChild, Up},
    replay::Screen,
};

/// How long a question for the reader's machine waits for its answer.
///
/// The loop waits with it, so it is short: on the network this is for an
/// answer takes a millisecond, and one that has not come in a second is
/// not coming.
const PATIENCE: Duration = Duration::from_secs(1);

/// How long what is still to be written is given to go once Obelus has
/// ended, so that the window hears it ended -- and not for ever, with
/// nobody reading.
const LAST_WORDS: Duration = Duration::from_secs(5);

/// The most updates kept for one message, which a frame that never ends --
/// nothing a front end says without a wake after it -- would otherwise
/// grow without end.
const KEPT_MOST: usize = 1 << 16;

/// What `ob --serve-one` writes to, and whether a window is there to read
/// it.
///
/// **A frame is one message.** What the screen is told is kept until the
/// front end is woken -- which is the end of a frame, or of something said
/// with nothing after it -- and goes then as one [`Down::Updates`]. Said a
/// cell at a time it was a message per cell: a page scrolled was thousands
/// of writes to the pipe, and the server's thousands of packets after them.
///
/// **The loop never waits for the network.** Everything is written on a
/// thread of its own ([`Out::write_to`]), and while it is writing, what
/// Obelus goes on saying is kept -- a cell said again taking the place of
/// what was said of it before, and a frame ended again taking the place of
/// the one before it ([`Kept`]). So a connection slower than the screen is
/// changing is sent the screen as it is when there is room to send it,
/// rather than every frame there has been one after another; and the keys
/// are answered at the speed of this machine, with the network behind
/// them, rather than at the speed of the network.
///
/// **A picture goes behind the frames, not among them.** What it is goes at
/// once, as [`Down::Hung`], and its bytes go a [`PIECE`] at a time after
/// anything else there is to say -- so the new theme's colours are drawn the
/// moment they are said and its picture arrives under them, rather than
/// every frame waiting for megabytes to cross first.
struct Out {
    said: Mutex<Said>,
    /// What wakes the writer, and whoever is waiting for it to be done.
    stirred: Condvar,
    /// Whether a window is attached, which is when anything is written.
    attached: AtomicBool,
    /// Whether what is written to has gone, which nothing comes back from.
    closed: AtomicBool,
    /// The screen as everything said leaves it, for a window that comes.
    screen: Mutex<Screen>,
}

/// What is still to be written.
#[derive(Default)]
struct Said {
    /// What has been said that goes before anything kept, in order.
    queued: VecDeque<Down>,
    /// What the screen has been told since the last of it went.
    kept: Kept,
    /// Whether what is kept is to go: a frame has ended, or something was
    /// said with nothing after it.
    due: bool,
    /// The picture going a piece at a time, where one is.
    sending: Option<Sending>,
    /// The last number a picture went under.
    numbered: u64,
    /// Whether the writer has something in its hands.
    writing: bool,
}

/// A picture on its way, and how much of it has gone.
struct Sending {
    number: u64,
    bytes: Vec<u8>,
    sent: usize,
}

/// Updates kept for the next message, with each said again in place of
/// what it says again.
///
/// A cell takes the place of what was said of the same cell before it, and
/// a frame's end takes the place of the end of the frame before it, along
/// with that frame's shapes -- a window replaces its shapes at every frame,
/// so only the last frame's are ever drawn. What is moved is moved earlier,
/// so neither may be moved across what changes the meaning of what came
/// before it: a cell not across the screen cleared, and nothing across a
/// theme going, which is a picture of the screen as it was.
#[derive(Default)]
struct Kept {
    updates: Vec<Option<Update>>,
    /// Where in `updates` the last word on each cell is, by row and then
    /// column -- a grid rather than a map, because every cell of every
    /// frame comes through here, behind or not.
    cells: Vec<Vec<Option<usize>>>,
    /// Which places in `cells` say anything, so that forgetting them is as
    /// much work as there are of them and not as there are cells.
    placed: Vec<(usize, usize)>,
    /// Where the shapes said since the last frame ended are.
    shaping: Vec<usize>,
    /// Where the last frame's end is, and where its shapes are, while both
    /// are still here to be taken the place of.
    framed: Option<(usize, Vec<usize>)>,
}

impl Kept {
    fn keep(&mut self, update: Update) {
        let at = self.updates.len();
        match &update {
            Update::Cell { x, y, .. } => {
                let (x, y) = (usize::from(*x), usize::from(*y));
                if self.cells.len() <= y {
                    self.cells.resize_with(y + 1, Vec::new);
                }
                let row = &mut self.cells[y];
                if row.len() <= x {
                    row.resize(x + 1, None);
                }
                if let Some(was) = row[x] {
                    self.updates[was] = Some(update);
                    return;
                }
                row[x] = Some(at);
                self.placed.push((x, y));
            }
            Update::Cleared => self.forget_the_cells(),
            Update::Recoloured => {
                self.forget_the_cells();
                self.framed = None;
            }
            Update::Frame => {
                if let Some((end, shapes)) = self.framed.take() {
                    self.updates[end] = None;
                    for shape in shapes {
                        self.updates[shape] = None;
                    }
                }
                self.framed = Some((at, std::mem::take(&mut self.shaping)));
            }
            update if update.is_a_shape() => self.shaping.push(at),
            _ => {}
        }
        self.updates.push(Some(update));
    }

    /// Forgets where each cell was said, so that nothing said after is
    /// moved before now.
    fn forget_the_cells(&mut self) {
        for (x, y) in self.placed.drain(..) {
            self.cells[y][x] = None;
        }
    }

    fn len(&self) -> usize {
        self.updates.len()
    }

    fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Everything kept, in order, and nothing kept after.
    fn take(&mut self) -> Vec<Update> {
        // The grid is kept, emptied, rather than made again for every
        // frame.
        self.forget_the_cells();
        self.shaping.clear();
        self.framed = None;
        std::mem::take(&mut self.updates)
            .into_iter()
            .flatten()
            .collect()
    }
}

impl Said {
    /// Keeps something for the screen -- or, for a picture, says what it is
    /// and sets its bytes going.
    fn keep(&mut self, update: Update) {
        match update {
            Update::Picture(Some(hanging)) => {
                // What came before it first, so that the window is told
                // things in the order they were said.
                self.queue_what_is_kept();
                self.numbered += 1;
                let length = u64::try_from(hanging.bytes.len()).unwrap_or(u64::MAX);
                self.queued.push_back(Down::Hung {
                    number: self.numbered,
                    wallpaper: hanging.wallpaper,
                    length,
                });
                self.sending = Some(Sending {
                    number: self.numbered,
                    bytes: hanging.bytes,
                    sent: 0,
                });
                return;
            }
            // A picture let go of is one whose bytes are no longer worth
            // sending.
            Update::Picture(None) => {
                self.sending = None;
                self.kept.keep(update);
            }
            update => self.kept.keep(update),
        }
        if self.kept.len() >= KEPT_MOST {
            self.due = true;
        }
    }

    /// Puts what is kept in the queue as one message, where anything is.
    fn queue_what_is_kept(&mut self) {
        self.due = false;
        if !self.kept.is_empty() {
            self.queued.push_back(Down::Updates(self.kept.take()));
        }
    }

    /// The next thing to write: what was said in order first, then the
    /// screen, and a piece of a picture only when there is nothing else.
    fn next(&mut self) -> Option<Down> {
        if let Some(down) = self.queued.pop_front() {
            return Some(down);
        }
        if self.due && !self.kept.is_empty() {
            self.due = false;
            return Some(Down::Updates(self.kept.take()));
        }
        let sending = self.sending.as_mut()?;
        let end = (sending.sent + PIECE).min(sending.bytes.len());
        let piece = Down::Piece {
            number: sending.number,
            bytes: sending.bytes[sending.sent..end].to_vec(),
        };
        sending.sent = end;
        if end >= sending.bytes.len() {
            self.sending = None;
        }
        Some(piece)
    }

    /// Whether there is anything to write other than a picture's pieces.
    fn waiting(&self) -> bool {
        !self.queued.is_empty() || (self.due && !self.kept.is_empty())
    }
}

impl Out {
    /// Nothing said yet, with a window already reading -- and nothing
    /// written until [`Out::write_to`] says where.
    fn new() -> Self {
        Self {
            said: Mutex::new(Said::default()),
            stirred: Condvar::new(),
            attached: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            screen: Mutex::new(Screen::default()),
        }
    }

    /// Writes to `writer`, on a thread of its own, everything said from now
    /// on and anything said before.
    fn write_to(out: &Arc<Self>, writer: Box<dyn std::io::Write + Send>) {
        let writing = Arc::clone(out);
        let started = std::thread::Builder::new()
            .name("obelus writing to the window".to_string())
            .spawn(move || {
                let out = writing;
                let mut writer = std::io::BufWriter::new(writer);
                loop {
                    let Ok(mut said) = out.said.lock() else {
                        return;
                    };
                    let down = loop {
                        if let Some(down) = said.next() {
                            break down;
                        }
                        said.writing = false;
                        out.stirred.notify_all();
                        said = match out.stirred.wait(said) {
                            Ok(said) => said,
                            Err(_) => return,
                        };
                    };
                    said.writing = true;
                    drop(said);
                    // Nobody to send it to: the window that comes back is
                    // told everything again, from the screen.
                    if !out.attached.load(Ordering::Acquire) {
                        continue;
                    }
                    if obelus_wire::write(&mut writer, &down).is_err() {
                        out.closed.store(true, Ordering::Release);
                        out.stirred.notify_all();
                        return;
                    }
                }
            });
        if let Err(error) = started {
            tracing::warn!(%error, "no thread to write to the window on, so nothing goes");
            Self::closed_now(out);
        }
    }

    /// Says that nothing written will arrive.
    fn closed_now(out: &Self) {
        out.closed.store(true, Ordering::Release);
        out.stirred.notify_all();
    }

    /// Whether what is said can still go anywhere.
    fn open(&self) -> Result<(), Closed> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Closed);
        }
        Ok(())
    }

    /// Keeps something for the screen, to go with the rest of its frame.
    fn keep(&self, update: Update) -> Result<(), Closed> {
        self.keep_all(vec![update])
    }

    /// Keeps several, under the one lock.
    fn keep_all(&self, updates: Vec<Update>) -> Result<(), Closed> {
        self.open()?;
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut said = self.said.lock().map_err(|_| Closed)?;
        for update in updates {
            said.keep(update);
        }
        // A picture said, or a frame grown past what is kept: either is
        // something for the writer.
        let stir = said.waiting() || said.sending.is_some();
        drop(said);
        if stir {
            self.stirred.notify_all();
        }
        Ok(())
    }

    /// Says one thing to the window, where there is one -- after what is
    /// kept for the screen, so that the two arrive in the order said.
    fn say(&self, down: Down) -> Result<(), Closed> {
        self.open()?;
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut said = self.said.lock().map_err(|_| Closed)?;
        said.queue_what_is_kept();
        said.queued.push_back(down);
        drop(said);
        self.stirred.notify_all();
        Ok(())
    }

    /// Sends what is kept.
    fn flush(&self) {
        if let Ok(mut said) = self.said.lock() {
            said.due = true;
        }
        self.stirred.notify_all();
    }

    /// Waits up to `patience` for everything said to have been written --
    /// all but a picture's pieces, which nobody waits for.
    fn drained(&self, patience: Duration) {
        let Ok(said) = self.said.lock() else {
            return;
        };
        let _ = self.stirred.wait_timeout_while(said, patience, |said| {
            (said.writing || said.waiting()) && !self.closed.load(Ordering::Acquire)
        });
    }

    /// A window has come: it is told everything on the screen, and then
    /// whatever comes after.
    ///
    /// Under the screen's lock, so that nothing said between the screen
    /// being read and the window being marked attached is lost or said
    /// twice.
    fn attach(&self) {
        let Ok(screen) = self.screen.lock() else {
            return;
        };
        if let Ok(mut said) = self.said.lock() {
            // Kept for a window that has gone since: the screen says all
            // of it again, the picture with it.
            let numbered = said.numbered;
            *said = Said {
                numbered,
                writing: said.writing,
                ..Said::default()
            };
        }
        self.attached.store(true, Ordering::Release);
        for update in screen.again() {
            let _ = self.keep(update);
        }
        drop(screen);
        self.flush();
    }
}

impl Showing for Out {
    fn show(&self, update: Update) -> Result<(), Closed> {
        // Kept and said under one lock, so that a window attaching in
        // between is told it once.
        let mut screen = self.screen.lock().map_err(|_| Closed)?;
        screen.saw(&update);
        let said = self.keep(update);
        drop(screen);
        said
    }

    // A frame's cells under one lock of each, rather than two locks a cell:
    // a page scrolled is thousands of them.
    fn show_all(&self, updates: Vec<Update>) -> Result<(), Closed> {
        let mut screen = self.screen.lock().map_err(|_| Closed)?;
        for update in &updates {
            screen.saw(update);
        }
        let said = self.keep_all(updates);
        drop(screen);
        said
    }

    fn wake(&self) {
        self.flush();
    }
}

/// What the reader's machine answered.
enum Answer {
    /// What was on the clipboard.
    Words(Option<String>),
    /// The picture on the clipboard, once all of it has arrived.
    Picture(Option<(String, Vec<u8>)>),
    /// Whether the clipboard holds a picture.
    Holds(bool),
    /// The picture on the clipboard is longer than anything waits for.
    TooLarge,
}

/// What is done with an answer once it comes -- or with nothing, where the
/// window that was asked went before it answered.
type Answering = Box<dyn FnOnce(Option<Answer>) + Send>;

/// The reader's machine, at the other end of the connection.
struct Afar {
    out: Arc<Out>,
    /// The last number a question was asked under.
    asked: AtomicU64,
    /// What is waiting for which answer.
    waiting: Mutex<HashMap<u64, Answering>>,
    /// The loop, for a picture that arrives after the paste that asked.
    events: std::sync::mpsc::Sender<Event>,
}

impl std::fmt::Debug for Afar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Afar")
    }
}

impl Afar {
    /// Asks the reader's machine something, and has `then` do what is done
    /// with the answer when it comes -- answering the question's number, or
    /// nothing where there is nobody to ask.
    fn asked(&self, question: impl FnOnce(u64) -> Down, then: Answering) -> Option<u64> {
        if !self.out.attached.load(Ordering::Acquire) {
            return None;
        }
        let number = self.asked.fetch_add(1, Ordering::Relaxed) + 1;
        self.waiting.lock().ok()?.insert(number, then);
        if self.out.say(question(number)).is_err() {
            self.forget(number);
            return None;
        }
        Some(number)
    }

    /// Asks the reader's machine something, and waits `patience` for it.
    fn ask(&self, question: impl FnOnce(u64) -> Down, patience: Duration) -> Option<Answer> {
        let (answer, answered) = std::sync::mpsc::channel();
        let number = self.asked(
            question,
            Box::new(move |heard| {
                let _ = answer.send(heard);
            }),
        )?;
        let heard = answered.recv_timeout(patience).ok().flatten();
        if heard.is_none() {
            tracing::info!(number, "the reader's machine did not answer in time");
        }
        self.forget(number);
        heard
    }

    /// Stops waiting for the answer to a question.
    fn forget(&self, number: u64) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&number);
        }
    }

    /// Stops waiting for every answer, and tells whatever was waiting: the
    /// window that was asked has gone.
    fn forget_everything(&self) {
        let waiting: Vec<Answering> = self
            .waiting
            .lock()
            .map(|mut waiting| waiting.drain().map(|(_, waiting)| waiting).collect())
            .unwrap_or_default();
        for waiting in waiting {
            waiting(None);
        }
    }

    /// Hands an answer to whatever is waiting for it.
    fn answered(&self, number: u64, answer: Answer) {
        let waiting = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&number));
        if let Some(waiting) = waiting {
            waiting(Some(answer));
        }
    }
}

impl app::desk::Desk for Afar {
    fn copy(&self, text: &str) -> std::io::Result<()> {
        self.out
            .say(Down::Copy(text.to_string()))
            .map_err(|_| std::io::Error::other("the window has gone"))?;
        self.out.flush();
        Ok(())
    }

    fn paste(&self) -> Option<String> {
        match self.ask(Down::Paste, PATIENCE)? {
            Answer::Words(words) => words,
            _ => None,
        }
    }

    // Whether there is a picture is a few bytes and worth the moment; the
    // picture is megabytes, and arrives when it has crossed -- or what
    // became of it does.
    fn picture(&self) -> app::desk::Picture {
        if !self.holds_a_picture() {
            return app::desk::Picture::Nothing;
        }
        let events = self.events.clone();
        let asked = self.asked(
            Down::Picture,
            Box::new(move |answer| {
                let _ = events.send(match answer {
                    Some(Answer::Picture(Some((shape, bytes)))) => Event::Pictured(shape, bytes),
                    Some(Answer::TooLarge) => Event::Amiss(
                        "The picture on the clipboard is too large to bring across".to_string(),
                    ),
                    Some(_) => Event::Amiss("The clipboard no longer holds a picture".to_string()),
                    None => {
                        Event::Amiss("The window went before the picture came across".to_string())
                    }
                });
            }),
        );
        match asked {
            Some(_) => app::desk::Picture::Coming,
            None => app::desk::Picture::Nothing,
        }
    }

    fn holds_a_picture(&self) -> bool {
        matches!(
            self.ask(Down::HoldsAPicture, PATIENCE),
            Some(Answer::Holds(true))
        )
    }

    fn open(&self, url: &str) -> std::io::Result<()> {
        self.out
            .say(Down::Open(url.to_string()))
            .map_err(|_| std::io::Error::other("no window to open it in"))?;
        self.out.flush();
        Ok(())
    }
}

/// What is arriving.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Carrying {
    /// A file dropped on the window, by its name.
    Dropped(String),
    /// The picture on the clipboard, answering the question of this
    /// number, in this shape.
    Picture {
        /// The question.
        number: u64,
        /// What shape it is in.
        shape: String,
    },
}

/// Runs one Obelus for the window `ob --server` is carrying the connection
/// of, until the reader leaves it or it is stopped.
///
/// # Errors
///
/// Where it was not told what to open, or what it was told to open would
/// not open.
pub(crate) fn one() -> Result<ExitCode> {
    let mut reading = std::io::stdin();
    let Some(ToChild::Open { front, size, path }) =
        obelus_wire::read::<_, ToChild>(&mut reading).context("told what to open")?
    else {
        anyhow::bail!("nothing said what to open");
    };
    // Before the settings are read, as `obg` does it: what Obelus is drawn
    // on decides what some of them mean.
    if let Front::Window {
        command_is_control, ..
    } = front
    {
        obelus_config::drawn_in_a_window();
        obelus_keymap::draw_control_as_command(command_is_control);
    }
    let paths: Vec<std::path::PathBuf> = path.into_iter().map(Into::into).collect();
    let mut app = startup::start(&paths, env!("OBELUS_BUILD"))?;
    tracing::info!(
        ?front,
        "drawn on a front end at the other end of a connection"
    );

    let out = Arc::new(Out::new());
    Out::write_to(&out, Box::new(std::io::stdout()));
    let showing: Arc<dyn Showing> = out.clone();
    let measured = Arc::new(Measured::default());
    measured.resized(size[0], size[1], u32::from(size[2]), u32::from(size[3]));
    match front {
        Front::Window { .. } => {
            app.drawn_by(Arc::new(obelus_front::Telling::new(Arc::clone(&showing))));
            let marking = obelus_front::Marking::new(Arc::clone(&showing));
            obelus_ui::shapes::drawn_by(Arc::new(marking.clone()));
            app.use_images(obelus_ui::image::Images::drawn_by(Arc::new(marking)));
        }
        Front::Terminal { pictures, keyboard } => {
            if let Some((pictures, cell)) = pictures {
                app.use_images(obelus_ui::image::Images::told(protocol(pictures), cell));
            }
            app.ctrl_enter_arrives(keyboard);
        }
    }
    let (sender, events) = obelus_app::event::channel();
    let afar = Arc::new(Afar {
        out: Arc::clone(&out),
        asked: AtomicU64::new(0),
        waiting: Mutex::new(HashMap::new()),
        events: sender.clone(),
    });
    app.desked_by(Arc::clone(&afar) as Arc<dyn app::desk::Desk>);
    app.drawn_afar();

    let mut heard = Heard {
        events: sender.clone(),
        size: Arc::clone(&measured),
        out: Arc::clone(&out),
        afar: Arc::clone(&afar),
        arriving: Arriving::default(),
    };
    std::thread::Builder::new()
        .name("obelus connection".to_string())
        .spawn(move || {
            loop {
                let told = match obelus_wire::read::<_, ToChild>(&mut reading) {
                    Ok(Some(told)) => told,
                    // `ob --server` has gone, or let this go: nobody can
                    // reach it again, so it stops the way a kill stops one.
                    Ok(None) | Err(_) => ToChild::Leave,
                };
                if heard.heard(told).is_err() {
                    return;
                }
            }
        })
        .context("a thread to hear the connection on")?;
    app.start(sender);

    let cells = obelus_front::Cells::new(showing, measured);
    let mut terminal = ratatui::Terminal::new(cells).context("a screen to draw on")?;
    let outcome = app::run(&mut terminal, &mut app, &events);
    startup::finish(&outcome);
    let last = match app.goes_to() {
        Some(app::Going::Home) => Down::Home,
        _ => Down::Finished,
    };
    let _ = out.say(last);
    out.drained(LAST_WORDS);
    outcome.map(|()| ExitCode::SUCCESS)
}

/// What `ob --server` says to this Obelus, heard: the window's keys, its
/// answers and what it sends a piece at a time, and its coming and going.
struct Heard {
    /// The loop.
    events: std::sync::mpsc::Sender<Event>,
    size: Arc<Measured>,
    out: Arc<Out>,
    afar: Arc<Afar>,
    arriving: Arriving<Carrying>,
}

impl Heard {
    /// Hears one thing, and answers `Err` where the loop has gone or been
    /// told to stop -- after which nothing more is heard.
    fn heard(&mut self, told: ToChild) -> Result<(), ()> {
        match told {
            ToChild::Up(Up::Input(input)) => self.tell(input.into_event(&self.size)),
            ToChild::Up(up) => self.answered(up),
            // A window again, perhaps of another size: the size first, so
            // that the screen it is told is the one that fits it after the
            // resize is heard.
            ToChild::Attached { size } => {
                self.size
                    .resized(size[0], size[1], u32::from(size[2]), u32::from(size[3]));
                // A window that comes back before its old connection was
                // known to have gone is never told `Detached`: what the old
                // one was asked and what it was sending are let go of here
                // too, or a picture asked for waits for good.
                self.let_go()?;
                self.out.attach();
                self.tell(Event::Resize)
            }
            ToChild::Detached => {
                self.out.attached.store(false, Ordering::Release);
                self.let_go()
            }
            ToChild::Open { .. } => {
                tracing::warn!("told what to open twice");
                Ok(())
            }
            ToChild::Leave => {
                let _ = self.events.send(Event::Stopped);
                Err(())
            }
        }
    }

    /// Lets go of everything the window that went was asked and was
    /// sending, and says so: a picture half here is told by what waited for
    /// it, and a file half here by this.
    fn let_go(&mut self) -> Result<(), ()> {
        self.afar.forget_everything();
        for gone in self.arriving.forget() {
            if let Carrying::Dropped(name) = gone {
                self.tell(Event::Amiss(format!(
                    "Not all of {name} came across, so it was not kept"
                )))?;
            }
        }
        Ok(())
    }

    /// An answer from the window, or a piece of something it is sending.
    fn answered(&mut self, up: Up) -> Result<(), ()> {
        let whole = match up {
            Up::Pasted { number, words } => {
                self.afar.answered(number, Answer::Words(words));
                Ok(None)
            }
            Up::Holds { number, picture } => {
                self.afar.answered(number, Answer::Holds(picture));
                Ok(None)
            }
            Up::Pictured {
                number,
                picture: None,
            } => {
                self.afar.answered(number, Answer::Picture(None));
                Ok(None)
            }
            Up::Pictured {
                number,
                picture: Some((shape, carried)),
            } => self.arriving.begin(
                carried.carrying,
                carried.length,
                Carrying::Picture { number, shape },
            ),
            Up::Dropped {
                name,
                file: Ok(carried),
            } => self
                .arriving
                .begin(carried.carrying, carried.length, Carrying::Dropped(name)),
            Up::Dropped { file: Err(why), .. } => return self.tell(Event::Amiss(why)),
            Up::Piece { carrying, bytes } => Ok(self.arriving.took(carrying, &bytes)),
            Up::Input(input) => return self.tell(input.into_event(&self.size)),
        };
        match whole {
            Err(Carrying::Picture { number, .. }) => {
                self.afar.answered(number, Answer::TooLarge);
                Ok(())
            }
            Err(Carrying::Dropped(name)) => self.tell(Event::Amiss(format!(
                "Too large to bring across, so not kept: {name}"
            ))),
            Ok(Some((Carrying::Picture { number, shape }, bytes))) => {
                self.afar
                    .answered(number, Answer::Picture(Some((shape, bytes))));
                Ok(())
            }
            // Written on a thread of its own: tens of megabytes to the disk
            // is long enough for the keys behind it to be felt waiting.
            Ok(Some((Carrying::Dropped(name), bytes))) => {
                let events = self.events.clone();
                let started = std::thread::Builder::new()
                    .name("obelus keeping what was dropped".to_string())
                    .spawn(move || {
                        let heard = match dropped_in(&std::env::temp_dir())
                            .and_then(|directory| kept(&directory, &name, &bytes))
                        {
                            Ok(path) => Event::Dropped(path),
                            Err(why) => Event::Amiss(why),
                        };
                        let _ = events.send(heard);
                    });
                if let Err(error) = started {
                    tracing::warn!(%error, "no thread to keep what was dropped on");
                }
                Ok(())
            }
            Ok(None) => Ok(()),
        }
    }

    /// Tells the loop, and answers `Err` where it has gone.
    fn tell(&self, event: Event) -> Result<(), ()> {
        self.events.send(event).map_err(|_| ())
    }
}

/// How a terminal at the other end draws a picture, as ratatui names it.
const fn protocol(pictures: obelus_front::link::Pictures) -> ratatui_image::picker::ProtocolType {
    use obelus_front::link::Pictures;
    use ratatui_image::picker::ProtocolType;
    match pictures {
        Pictures::Kitty => ProtocolType::Kitty,
        Pictures::Iterm2 => ProtocolType::Iterm2,
        Pictures::Sixel => ProtocolType::Sixel,
    }
}

/// Where a file dropped on a window at the other end is kept on this
/// machine, in `temporary`: beside the machine's other passing things, and
/// not in the project, whose history it is no part of. Not taken away when
/// the window goes -- a conversation taken up tomorrow may still be about
/// it -- which is the machine's own clearing of its temporary files to do.
///
/// The reader's alone. A temporary directory on unix is every account's,
/// so the directory is this one's by its number, made for nobody else --
/// and one of that name made by somebody else first, or opened to anybody
/// since, is not used: whoever owns it could read what is dropped, or
/// change it before the agent reads it. On Windows the temporary directory
/// is already the reader's own.
#[cfg(unix)]
fn dropped_in(temporary: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};

    // Safety: asks the process who it is, and nothing else.
    let whose = unsafe { libc::getuid() };
    let directory = temporary.join(format!("obelus-dropped-{whose}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("Nowhere to keep what was dropped: {error}")),
    }
    let found = std::fs::symlink_metadata(&directory)
        .map_err(|error| format!("Nowhere to keep what was dropped: {error}"))?;
    if !found.is_dir() || found.uid() != whose || found.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "Nothing was kept, because {} is not yours alone",
            directory.display()
        ));
    }
    Ok(directory)
}

/// The same, where the temporary directory is the reader's already.
#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same answer as the platform that can refuse"
)]
fn dropped_in(temporary: &std::path::Path) -> Result<std::path::PathBuf, String> {
    Ok(temporary.join("obelus-dropped"))
}

/// Keeps a dropped file in `directory` under its own name, or under that
/// name with `-1`, `-2` before its extension where the name is taken, and
/// says where.
///
/// The name is the last part of what came and nothing else: a name with a
/// directory in it is somebody else's idea of where on this machine to
/// write.
fn kept(
    directory: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, String> {
    let named = std::path::Path::new(name)
        .file_name()
        .filter(|named| *named != "." && *named != "..")
        .ok_or_else(|| format!("Not a file's name: {name}"))?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Nowhere to keep {name}: {error}"))?;
    let named = std::path::Path::new(named);
    let stem = named
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let extension = named
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned());
    for again in 0_u32.. {
        let called = match (again, &extension) {
            (0, _) => named.to_string_lossy().into_owned(),
            (again, Some(extension)) => format!("{stem}-{again}.{extension}"),
            (again, None) => format!("{stem}-{again}"),
        };
        let path = directory.join(called);
        // Made new or not at all, so two windows dropping one name at once
        // keep two files rather than one over the other.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                return file
                    .write_all(bytes)
                    .map(|()| path)
                    .map_err(|error| format!("Could not keep {name}: {error}"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Could not keep {name}: {error}")),
        }
    }
    Err(format!("Could not keep {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a test's `Out` writes, kept to be read back.
    #[derive(Clone, Default)]
    struct Written(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Written {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("what was written")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Written {
        /// Every message written, in order.
        fn messages(&self) -> Vec<Down> {
            let bytes = self.0.lock().expect("what was written").clone();
            let mut reading = bytes.as_slice();
            std::iter::from_fn(|| obelus_wire::read::<_, Down>(&mut reading).expect("a message"))
                .collect()
        }
    }

    /// A picture of the theme, in a served Obelus, what it is said at once
    /// and its bytes after, in pieces -- never among the frames.
    ///
    /// Broken deliberately by keeping it among the frames (`Update::Picture
    /// (Some(..))` pushed onto `kept` like anything else, in `Said::keep`):
    /// the picture crossed whole inside the frame's message, and every frame
    /// behind it waited for it.
    #[test]
    fn a_picture_goes_as_what_it_is_and_then_its_pieces() {
        let written = Written::default();
        let out = Arc::new(Out::new());
        Out::write_to(&out, Box::new(written.clone()));
        let bytes: Vec<u8> = (0..200_000_u32).map(|at| (at % 251) as u8).collect();
        out.show(Update::Picture(Some(obelus_front::Hanging {
            wallpaper: obelus_theme::Wallpaper {
                file: std::path::PathBuf::from("dusk.jpg"),
                opacity: 80,
                fit: obelus_theme::wallpaper::Fit::Cover,
                anchor: obelus_theme::wallpaper::Anchor::default(),
                modified: None,
            },
            bytes: bytes.clone(),
        })))
        .expect("shown");
        out.show(Update::Frame).expect("shown");
        out.wake();

        let pieces = |messages: &[Down]| -> Vec<u8> {
            messages
                .iter()
                .filter_map(|down| match down {
                    Down::Piece { bytes, .. } => Some(bytes.clone()),
                    _ => None,
                })
                .flatten()
                .collect()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while pieces(&written.messages()).len() < bytes.len() {
            assert!(
                std::time::Instant::now() < deadline,
                "the picture never all went"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let messages = written.messages();
        assert!(
            !messages.iter().any(|down| matches!(
                down,
                Down::Updates(updates)
                    if updates.iter().any(|update| matches!(update, Update::Picture(Some(_))))
            )),
            "the picture went among the frames"
        );
        assert!(
            messages.iter().any(|down| matches!(
                down,
                Down::Hung {
                    length: 200_000,
                    ..
                }
            )),
            "what the picture is was not said"
        );
        assert!(
            messages
                .iter()
                .all(|down| !matches!(down, Down::Piece { bytes, .. } if bytes.len() > PIECE)),
            "a piece larger than a piece"
        );
        assert_eq!(pieces(&messages), bytes, "the pieces are not the picture");
    }

    /// A picture let go of before its pieces went sends none of them.
    ///
    /// Broken deliberately by going on sending it (the `sending = None` for
    /// `Update::Picture(None)` left out of `Said::keep`): megabytes of an
    /// old theme's picture went after the page had let go of it.
    #[test]
    fn a_picture_let_go_of_sends_none_of_itself() {
        let written = Written::default();
        let out = Arc::new(Out::new());
        out.show(Update::Picture(Some(obelus_front::Hanging {
            wallpaper: obelus_theme::Wallpaper {
                file: std::path::PathBuf::from("dusk.jpg"),
                opacity: 80,
                fit: obelus_theme::wallpaper::Fit::Cover,
                anchor: obelus_theme::wallpaper::Anchor::default(),
                modified: None,
            },
            bytes: vec![1; 300_000],
        })))
        .expect("shown");
        out.show(Update::Picture(None)).expect("shown");
        out.wake();
        // Started only now, so that nothing of it can have gone before the
        // letting go.
        Out::write_to(&out, Box::new(written.clone()));
        out.drained(Duration::from_secs(10));
        assert!(
            !written
                .messages()
                .iter()
                .any(|down| matches!(down, Down::Piece { .. })),
            "a piece of a picture let go of went"
        );
    }

    /// Cells handed over together are kept for a window that comes back as
    /// well as said in the frame's one message.
    ///
    /// Broken deliberately by leaving the screen out of `Out::show_all`
    /// (no `screen.saw`): the frame went, and a window coming back was told
    /// a screen with none of it.
    #[test]
    fn cells_handed_over_together_are_kept_and_said() {
        let written = Written::default();
        let out = Arc::new(Out::new());
        Out::write_to(&out, Box::new(written.clone()));
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_symbol("x");
        let cells: Vec<Update> = (0..100)
            .map(|x| Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell.clone()),
            })
            .collect();
        out.show_all(cells).expect("shown");
        out.show(Update::Frame).expect("shown");
        out.wake();
        out.drained(Duration::from_secs(10));
        let messages = written.messages();
        assert_eq!(messages.len(), 1, "not one message for the frame");
        let Down::Updates(updates) = &messages[0] else {
            panic!("not the frame: {:?}", messages[0]);
        };
        assert_eq!(updates.len(), 101);
        let again = out.screen.lock().expect("the screen").again();
        let kept = again
            .iter()
            .filter(|update| matches!(update, Update::Cell { .. }))
            .count();
        assert_eq!(
            kept, 100,
            "the screen kept for a window coming back has none of it"
        );
    }

    /// A frame is one message however many cells it changed, and what is
    /// said beside the screen goes after what came before it.
    ///
    /// Broken deliberately by writing each update as it is kept (`out()`
    /// at the end of `keep`): a frame of a thousand cells went as a
    /// thousand and one messages.
    #[test]
    fn a_frame_goes_as_one_message() {
        let written = Written::default();
        let out = Arc::new(Out::new());
        Out::write_to(&out, Box::new(written.clone()));
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_symbol("x");
        for x in 0..1000 {
            out.show(Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell.clone()),
            })
            .expect("shown");
        }
        out.show(Update::Frame).expect("shown");
        out.drained(Duration::from_secs(10));
        assert!(
            written.messages().is_empty(),
            "said before the frame was done"
        );
        out.wake();
        out.say(Down::Open("u".to_string())).expect("said");
        out.drained(Duration::from_secs(10));

        let messages = written.messages();
        assert_eq!(messages.len(), 2, "not one message for the frame");
        let Down::Updates(updates) = &messages[0] else {
            panic!("the frame was not first: {:?}", messages[0]);
        };
        assert_eq!(updates.len(), 1001);
        assert_eq!(updates.last(), Some(&Update::Frame));
        assert_eq!(messages[1], Down::Open("u".to_string()));
    }

    /// Where a test's `Out` writes, held up until the test lets it go --
    /// a connection slower than the screen.
    struct Stalled {
        written: Written,
        go: Arc<(Mutex<bool>, Condvar)>,
    }

    impl std::io::Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let (gone, going) = &*self.go;
            let mut gone = gone.lock().expect("the gate");
            while !*gone {
                gone = going.wait(gone).expect("the gate");
            }
            drop(gone);
            self.written.write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn cell(symbol: &str) -> Box<ratatui::buffer::Cell> {
        let mut cell = ratatui::buffer::Cell::default();
        cell.set_symbol(symbol);
        Box::new(cell)
    }

    /// Frames said while the window is still being sent the last one wait
    /// as one: each cell as it was last said, the last frame's shapes and
    /// none before them -- and saying them never waits for the window.
    ///
    /// Broken deliberately twice. By keeping every update as it came
    /// (`Kept::keep` pushing and nothing else): a hundred frames went as a
    /// hundred cells and a hundred ends, after the window had caught up.
    /// And by writing on the loop's thread (`Out::flush` writing what is
    /// kept, as it did): the first frame said waited for the window, and
    /// the test with it.
    #[test]
    fn frames_said_while_the_window_is_behind_go_as_one() {
        let written = Written::default();
        let go = Arc::new((Mutex::new(false), Condvar::new()));
        let out = Arc::new(Out::new());
        Out::write_to(
            &out,
            Box::new(Stalled {
                written: written.clone(),
                go: Arc::clone(&go),
            }),
        );
        let saying = {
            let out = Arc::clone(&out);
            std::thread::spawn(move || {
                for frame in 0..100_u16 {
                    out.show(Update::Cell {
                        x: 0,
                        y: 0,
                        cell: cell(&frame.to_string()),
                    })
                    .expect("shown");
                    out.show(Update::Ruled {
                        area: ratatui::layout::Rect::new(0, frame, 1, 1),
                    })
                    .expect("shown");
                    out.show(Update::Frame).expect("shown");
                    out.wake();
                }
            })
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !saying.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "saying a frame waited for the window"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        saying.join().expect("said");
        {
            let (gone, going) = &*go;
            *gone.lock().expect("the gate") = true;
            going.notify_all();
        }
        out.drained(Duration::from_secs(10));

        let updates: Vec<Update> = written
            .messages()
            .into_iter()
            .flat_map(|down| match down {
                Down::Updates(updates) => updates,
                other => panic!("not the screen: {other:?}"),
            })
            .collect();
        // The first frame, which the writer had in its hands when the
        // window stalled, and the last, which is everything after it.
        assert!(
            updates.len() <= 6,
            "{} updates for two frames",
            updates.len()
        );
        let cells: Vec<&str> = updates
            .iter()
            .filter_map(|update| match update {
                Update::Cell { cell, .. } => Some(cell.symbol()),
                _ => None,
            })
            .collect();
        assert_eq!(
            cells.last(),
            Some(&"99"),
            "not the cell as it was last said"
        );
        let rules: Vec<u16> = updates
            .iter()
            .filter_map(|update| match update {
                Update::Ruled { area } => Some(area.y),
                _ => None,
            })
            .collect();
        assert_eq!(rules.last(), Some(&99), "not the last frame's shapes");
        assert!(rules.len() <= 2, "shapes of frames gone by: {rules:?}");
        assert_eq!(updates.last(), Some(&Update::Frame));
    }

    /// A cell is not moved across the screen being cleared, nor anything
    /// across a theme going.
    ///
    /// Broken deliberately by moving them anyway (the `Cleared` and
    /// `Recoloured` arms left out of `Kept::keep`): the `b` said after the
    /// clear went before it, and the window cleared it away.
    #[test]
    fn nothing_is_moved_across_a_clear() {
        let mut kept = Kept::default();
        for update in [
            Update::Cell {
                x: 0,
                y: 0,
                cell: cell("a"),
            },
            Update::Cleared,
            Update::Cell {
                x: 0,
                y: 0,
                cell: cell("b"),
            },
            Update::Recoloured,
            Update::Cell {
                x: 0,
                y: 0,
                cell: cell("c"),
            },
        ] {
            kept.keep(update);
        }
        let said: Vec<String> = kept
            .take()
            .iter()
            .map(|update| match update {
                Update::Cell { cell, .. } => cell.symbol().to_string(),
                Update::Cleared => "cleared".to_string(),
                Update::Recoloured => "recoloured".to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(said, ["a", "cleared", "b", "recoloured", "c"]);
    }

    /// A picture asked for arrives in the loop as itself once the window
    /// has answered, and nothing waits for it meanwhile.
    ///
    /// Broken deliberately by answering that none is coming
    /// (`Desk::picture` answering `Nothing` where it asked): the paste went
    /// on to paste words, with the picture still to arrive.
    #[test]
    fn a_picture_asked_for_arrives_in_the_loop() {
        use obelus_app::app::desk::Desk as _;

        let written = Written::default();
        let out = Arc::new(Out::new());
        Out::write_to(&out, Box::new(written.clone()));
        let (events, heard) = std::sync::mpsc::channel();
        let afar = Arc::new(Afar {
            out: Arc::clone(&out),
            asked: AtomicU64::new(0),
            waiting: Mutex::new(HashMap::new()),
            events,
        });
        // The window, answering whether there is a picture and nothing
        // else: what was asked for after that is answered below.
        let window = {
            let (afar, written) = (Arc::clone(&afar), written.clone());
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(20);
                loop {
                    assert!(std::time::Instant::now() < deadline, "never asked");
                    if let Some(number) = written.messages().iter().find_map(|down| match down {
                        Down::HoldsAPicture(number) => Some(*number),
                        _ => None,
                    }) {
                        afar.answered(number, Answer::Holds(true));
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        assert!(matches!(afar.picture(), app::desk::Picture::Coming));
        window.join().expect("the window");
        out.drained(Duration::from_secs(10));
        let asked = written
            .messages()
            .iter()
            .find_map(|down| match down {
                Down::Picture(number) => Some(*number),
                _ => None,
            })
            .expect("the picture was not asked for");
        assert!(heard.try_recv().is_err(), "arrived before it was sent");
        afar.answered(
            asked,
            Answer::Picture(Some(("image/png".to_string(), vec![1, 2, 3]))),
        );
        assert!(matches!(
            heard.try_recv(),
            Ok(Event::Pictured(shape, bytes)) if shape == "image/png" && bytes == [1, 2, 3]
        ));

        assert!(heard.try_recv().is_err(), "said more than the picture");
    }

    /// A picture asked for of a window that then comes back -- with or
    /// without its old connection having been found to go -- is said not
    /// to be coming, rather than waited for for good; and so is a file the
    /// window was half way through sending.
    ///
    /// Broken deliberately three ways. By letting go of nothing when a
    /// window comes back (`let_go` left out of the `Attached` arm of
    /// `Heard::heard`): the picture was never said to have gone, and the
    /// status row said "Bringing the picture across" for good. By letting go
    /// without saying so (`forget_everything` calling nothing): the same.
    /// And by the picture's own answer saying nothing of a window that went
    /// (its `None` arm in `Desk::picture` sending nothing).
    #[test]
    fn what_a_window_that_came_back_was_asked_is_said_to_have_gone() {
        use obelus_app::app::desk::Desk as _;

        let written = Written::default();
        let out = Arc::new(Out::new());
        Out::write_to(&out, Box::new(written.clone()));
        let (events, heard) = std::sync::mpsc::channel();
        let afar = Arc::new(Afar {
            out: Arc::clone(&out),
            asked: AtomicU64::new(0),
            waiting: Mutex::new(HashMap::new()),
            events: events.clone(),
        });
        let window = {
            let (afar, written) = (Arc::clone(&afar), written.clone());
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(20);
                loop {
                    assert!(std::time::Instant::now() < deadline, "never asked");
                    if let Some(number) = written.messages().iter().find_map(|down| match down {
                        Down::HoldsAPicture(number) => Some(*number),
                        _ => None,
                    }) {
                        afar.answered(number, Answer::Holds(true));
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };
        assert!(matches!(afar.picture(), app::desk::Picture::Coming));
        window.join().expect("the window");

        let mut hearing = Heard {
            events,
            size: Arc::new(Measured::default()),
            out: Arc::clone(&out),
            afar: Arc::clone(&afar),
            arriving: Arriving::default(),
        };
        hearing
            .heard(ToChild::Up(Up::Dropped {
                name: "log.txt".to_string(),
                file: Ok(obelus_front::link::Carried {
                    carrying: 1,
                    length: 10,
                }),
            }))
            .expect("heard");
        hearing
            .heard(ToChild::Up(Up::Piece {
                carrying: 1,
                bytes: vec![1, 2, 3],
            }))
            .expect("heard");
        // Back before anything said the old connection went.
        hearing
            .heard(ToChild::Attached {
                size: [80, 24, 0, 0],
            })
            .expect("heard");

        let said: Vec<String> = heard
            .try_iter()
            .filter_map(|event| match event {
                Event::Amiss(why) => Some(why),
                _ => None,
            })
            .collect();
        assert!(
            said.iter()
                .any(|why| why == "The window went before the picture came across"),
            "the picture was never said to have gone: {said:?}"
        );
        assert!(
            said.iter()
                .any(|why| why == "Not all of log.txt came across, so it was not kept"),
            "the file was never said to have gone: {said:?}"
        );
    }

    /// A dropped file is kept under its own name, and a second of the same
    /// name beside it with a number before its extension -- never over it,
    /// and never anywhere but the directory.
    ///
    /// Broken deliberately by writing over a name that is taken
    /// (`create_new` left out): the second file took the first one's place.
    #[test]
    fn a_dropped_file_is_kept_beside_any_of_the_same_name() {
        let directory = std::env::temp_dir().join(format!("obelus-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let first = kept(&directory, "a.png", b"one").expect("kept");
        let second = kept(&directory, "a.png", b"two").expect("kept");
        let third = kept(&directory, "notes", b"three").expect("kept");
        let fourth = kept(&directory, "notes", b"four").expect("kept");
        assert_eq!(first, directory.join("a.png"));
        assert_eq!(second, directory.join("a-1.png"));
        assert_eq!(third, directory.join("notes"));
        assert_eq!(fourth, directory.join("notes-1"));
        assert_eq!(std::fs::read(&first).expect("read"), b"one");
        assert_eq!(std::fs::read(&second).expect("read"), b"two");
        let escaped = kept(&directory, "../../escaped", b"x").expect("kept");
        assert_eq!(
            escaped,
            directory.join("escaped"),
            "a name wrote outside its directory"
        );
        assert!(kept(&directory, "..", b"x").is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The directory dropped files are kept in is made for the reader
    /// alone, and one opened to anybody else is not used.
    ///
    /// Broken deliberately by taking any directory of the name (the owner
    /// and mode left unasked in `dropped_in`): a directory everybody could
    /// read was used for what the reader dropped.
    #[cfg(unix)]
    #[test]
    fn what_is_dropped_is_kept_where_only_the_reader_can_read_it() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary =
            std::env::temp_dir().join(format!("obelus-dropped-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        std::fs::create_dir_all(&temporary).expect("a temporary directory");
        let directory = dropped_in(&temporary).expect("made");
        let mode = std::fs::metadata(&directory)
            .expect("there")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "made for others too");

        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .expect("opened up");
        assert!(
            dropped_in(&temporary).is_err(),
            "a directory anybody can read was used"
        );
        let _ = std::fs::remove_dir_all(&temporary);
    }
}
