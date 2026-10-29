//! A front end drawing an Obelus that runs on another machine: `obg
//! --connect` and `ob --connect`, which differ in what they are and what
//! they draw on, and in nothing here.
//!
//! **The front end is the same front end.** What arrives is what Obelus on
//! this machine would have said to it -- the frames, and for a window what
//! the settings tell one, the shapes and the marks -- and it is handed over
//! through the same door ([`Showing`]). What the reader does goes back the
//! way it would have reached Obelus's loop, as an [`Input`]. Nothing else
//! about the front end knows the difference.
//!
//! **What only this machine can do is done here.** The reader's clipboard
//! and browser are this machine's, so what Obelus there asks of them is
//! answered here (`crate::link::Down`).
//!
//! **What is large goes up behind the keys.** A picture on the clipboard
//! and a file dropped on the window are read and sent a piece at a time on
//! a thread of their own ([`carry`]), letting go of the connection between
//! pieces -- so a key pressed meanwhile goes between them, and the screen
//! goes on being heard while they cross.
//!
//! **A connection that goes is knocked on again.** With the window's name,
//! which takes up the Obelus that was there -- still running, with
//! whatever it was doing -- and is told the whole screen again. Nothing
//! says when a network has come back, so it is asked, a moment at a time,
//! each moment longer than the last up to [`LONGEST`]; the last screen
//! stays up meanwhile.

use std::{
    net::TcpStream,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_app::event::Event;

use crate::{
    Input, Measured, Showing, Update,
    link::{Answered, Carried, Challenge, Down, Front, Knock, PIECE, Up},
};

/// The longest wait between two knocks on a server that has gone quiet.
const LONGEST: Duration = Duration::from_secs(10);

/// How long a connection that went is knocked on before the reader is
/// told: long enough for a network that blinked, short enough that a
/// screen gone still is not mistaken for a slow one.
const WHILE_IT_BLINKS: Duration = Duration::from_secs(2);

/// How long a server that took the connection is given to answer each
/// step of the knock.
const KNOCKING: Duration = Duration::from_secs(10);

/// How long a machine is given to take a connection. A name nobody is at
/// otherwise waits on the system's own patience, which is twenty seconds
/// on Windows and two minutes elsewhere -- and the window has nothing to
/// draw meanwhile.
const CONNECTING: Duration = Duration::from_secs(3);

/// How long a first connection is waited for before the page saying it is
/// connecting goes up: less than a reader notices, so a machine that
/// answers puts up no page, and one that does not leaves no blank window.
const AT_ONCE: Duration = Duration::from_millis(300);

/// Asks whether the machine at `afar` answers, a moment at a time and each
/// moment longer, and says so on `nudge` as `Event::Reachable` once it
/// does -- for a page waiting on it. Stops when `stop` says to, which is
/// the page having gone: nothing else could say the machine is back.
///
/// Each try that comes to nothing says why, as `Event::Unreachable`: the
/// page that said it was connecting says why it cannot instead. The first
/// try is `first` from now.
pub fn wait_for(
    afar: &Afar,
    nudge: std::sync::mpsc::Sender<Event>,
    first: Duration,
) -> std::sync::Arc<AtomicBool> {
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopped = std::sync::Arc::clone(&stop);
    let asked = Afar {
        at: afar.at.clone(),
        key: Vec::new(),
        path: None,
        build: afar.build,
        window: None,
    };
    let started = std::thread::Builder::new()
        .name("obelus waiting for a machine".to_string())
        .spawn(move || {
            let mut wait = first;
            let mut then = Duration::from_secs(1);
            while !stopped.load(Ordering::Acquire) {
                std::thread::sleep(wait);
                if stopped.load(Ordering::Acquire) {
                    return;
                }
                match asked.reached() {
                    Ok(()) => {
                        let _ = nudge.send(Event::Reachable(asked.at));
                        return;
                    }
                    Err(why) => {
                        let _ = nudge.send(Event::Unreachable {
                            at: asked.at.clone(),
                            why,
                        });
                    }
                }
                wait = then;
                then = (then * 2).min(LONGEST);
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "nothing to wait for the machine on");
    }
    stop
}

/// Where a server listens when the reader names only the machine.
const PORT: u16 = 7787;

/// The server a front end is for, and what it is to open there.
#[derive(Debug)]
pub struct Afar {
    /// The machine, and the port where one was given.
    pub at: String,
    /// The key it printed.
    pub key: Vec<u8>,
    /// What to open there, as `ob path` would be told.
    pub path: Option<String>,
    /// Which build of Obelus this is, for the hello.
    pub build: &'static str,
    /// The window there this is, where it is one whose connection went --
    /// taken up as it was, rather than started.
    pub window: Option<String>,
}

impl Afar {
    /// The key, as the letters the server printed.
    #[must_use]
    pub fn letters(&self) -> String {
        hex(&self.key)
    }

    /// Whether the machine answers on the server's port at all, which is
    /// all a page waiting for it needs to know before knocking -- or why
    /// not, as the connection said it.
    ///
    /// # Errors
    ///
    /// Why the machine did not take the connection.
    pub fn reached(&self) -> Result<(), String> {
        connected(&self.address())
            .map(drop)
            .map_err(|error| error.to_string())
    }

    /// Where to connect: the port the reader gave, or the server's own.
    fn address(&self) -> String {
        match self.at.rsplit_once(':') {
            Some((_, port)) if port.parse::<u16>().is_ok() => self.at.clone(),
            _ => format!("{}:{PORT}", self.at),
        }
    }
}

/// The key for a server: the one the reader gave, or the one kept from the
/// last time a window got in there (`obelus_app::app::servers`).
///
/// # Errors
///
/// Where there is none, or it is not the letters a server prints.
pub fn key(at: &str, given: Option<String>) -> Result<Vec<u8>> {
    let letters = match given {
        Some(given) => given,
        None => obelus_app::app::servers::key_for(at)
            .context("no key: give the one the server printed with --key")?,
    };
    obelus_app::app::servers::as_a_key(&letters).context("that key is not one a server prints")
}

/// The key as the letters a server prints.
fn hex(key: &[u8]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A connection to `address`, given [`CONNECTING`] to be taken at each of
/// the places the name is -- or why there is none.
fn connected(address: &str) -> std::io::Result<TcpStream> {
    use std::net::ToSocketAddrs as _;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "Nothing is at that name");
    for at in address.to_socket_addrs()? {
        match TcpStream::connect_timeout(&at, CONNECTING) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// What a front end draws before it knocks on `afar`: nothing, where the
/// machine takes the connection in a moment -- and otherwise the page that
/// says so, rather than a window with nothing in it while the system
/// decides. Connecting, with the machine asked again behind it, where the
/// first try has said nothing yet; that it cannot be reached and why,
/// where it already has.
///
/// # Errors
///
/// Where an Obelus of this machine's would not start, to draw the page.
pub fn first(afar: &Afar, nudge: &std::sync::mpsc::Sender<Event>) -> Result<Option<Next>> {
    first_within(afar, nudge, AT_ONCE)
}

/// [`first`], waiting `moment` for the first try.
fn first_within(
    afar: &Afar,
    nudge: &std::sync::mpsc::Sender<Event>,
    moment: Duration,
) -> Result<Option<Next>> {
    let (said, heard) = std::sync::mpsc::channel();
    let address = afar.address();
    // On a thread of its own, which goes on after the moment is up: what
    // it says then is nobody's, and the page asks again for itself.
    let started = std::thread::Builder::new()
        .name("obelus connecting".to_string())
        .spawn(move || {
            let _ = said.send(
                connected(&address)
                    .map(drop)
                    .map_err(|error| error.to_string()),
            );
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no thread to connect on, so the knock waits");
        return Ok(None);
    }
    match heard.recv_timeout(moment) {
        Ok(Ok(())) => Ok(None),
        Ok(Err(why)) => after(
            afar,
            Left::Lost {
                window: afar.window.clone(),
                why,
                again: true,
            },
            nudge,
        )
        .map(Some),
        Err(_) => {
            let mut app = obelus_app::startup::start(&[], afar.build)?;
            app.the_machine_has_gone(obelus_app::app::MachineGone {
                at: afar.at.clone(),
                key: afar.letters(),
                window: afar.window.clone(),
                why: None,
                trying: true,
            });
            let waiting = wait_for(afar, nudge.clone(), Duration::ZERO);
            Ok(Some(Next::Here(Box::new(app), Some(waiting))))
        }
    }
}

/// Knocks on the server, and says what it answered and the connection.
fn knock(
    afar: &Afar,
    front: &Front,
    measured: &Measured,
    window: Option<&str>,
) -> Result<(TcpStream, Answered)> {
    let mut stream =
        connected(&afar.address()).with_context(|| format!("connecting to {}", afar.address()))?;
    stream.set_nodelay(true)?;
    // A server that took the connection and then said nothing is a page
    // saying so in a moment, not a window frozen for as long as the
    // kernel would wait.
    stream.set_read_timeout(Some(KNOCKING))?;
    crate::link::kept_alive(&stream)?;
    let ours = obelus_wire::Hello::here(afar.build);
    obelus_wire::write(&mut stream, &ours)?;
    let theirs: obelus_wire::Hello =
        obelus_wire::read(&mut stream)?.context("The server went before it said hello")?;
    if let Some(why) = ours.refused(&theirs) {
        anyhow::bail!(why);
    }
    let challenge: Challenge =
        obelus_wire::read(&mut stream)?.context("The server went before it asked for the key")?;
    let size = measured.size();
    let pixels = measured.pixels();
    obelus_wire::write(
        &mut stream,
        &Knock {
            signed: crate::link::sign(&afar.key, &challenge),
            front: front.clone(),
            size: [size.width, size.height, pixels.width, pixels.height],
            path: afar.path.clone(),
            window: window.map(str::to_string),
        },
    )?;
    let answered: Answered =
        obelus_wire::read(&mut stream)?.context("The server went before it answered")?;
    // In now, and quiet for as long as the reader is: what says the
    // connection went without a word from here on is the kernel's knocking.
    stream.set_read_timeout(None)?;
    Ok((stream, answered))
}

/// A picture arriving in pieces: which, what the theme said of it, how
/// long it is, and what has come.
struct Arriving {
    number: u64,
    wallpaper: obelus_theme::Wallpaper,
    length: u64,
    bytes: Vec<u8>,
}

impl Arriving {
    /// Takes in a piece of the picture of `number`, and hands the picture
    /// back once it is whole. A piece of any other is about a picture that
    /// has been let go of, and goes nowhere.
    fn took(&mut self, number: u64, piece: &[u8]) -> Option<crate::Hanging> {
        if number != self.number {
            return None;
        }
        // Room made as the pieces come, rather than for what the other end
        // said the length would be.
        self.bytes.extend_from_slice(piece);
        let had = u64::try_from(self.bytes.len()).unwrap_or(u64::MAX);
        (had >= self.length).then(|| crate::Hanging {
            wallpaper: self.wallpaper.clone(),
            bytes: std::mem::take(&mut self.bytes),
        })
    }
}

/// Hangs a picture that has arrived whole, and answers whether there is
/// still a front end to hang it on.
fn hung(here: &dyn Showing, whole: crate::Hanging) -> bool {
    if here.show(Update::Picture(Some(whole))).is_err() {
        return false;
    }
    here.wake();
    true
}

/// Why there is no more connection to have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Left {
    /// The Obelus there ended, or the reader left with nobody there to ask.
    Finished,
    /// The reader asked to come back to this machine.
    Home,
    /// The machine could not be reached, or would not let the window in.
    Lost {
        /// The window there this was, to take up where it answers again.
        window: Option<String>,
        /// Why, in words for the reader.
        why: String,
        /// Whether it is worth asking again: not of a server that answered
        /// and said no.
        again: bool,
    },
}

/// Draws the Obelus at `afar` for a front end that is `front`, until there
/// is no more connection to have -- with what arrives going to `here`, and
/// what the front end says arriving on `done` as `measured` measures it.
///
/// `done` is lent and given back, because after this the front end may be
/// drawing an Obelus of its own on the same channel; `nudge` is a way into
/// it, which is how what was reading it is told to stop. `heard` sees each
/// thing the front end says before it goes, which is where a terminal
/// measures itself on a resize.
///
/// # Errors
///
/// Where there is no thread to keep the connection on.
pub fn run(
    afar: &Afar,
    front: &Front,
    here: &dyn Showing,
    measured: &Measured,
    done: &Receiver<Event>,
    nudge: &std::sync::mpsc::Sender<Event>,
    heard: &dyn Fn(&Event),
) -> Result<Left> {
    let writing: Mutex<Option<TcpStream>> = Mutex::default();
    let leaving = AtomicBool::new(false);
    let over = AtomicBool::new(false);
    let (writing, leaving, over) = (&writing, &leaving, &over);
    std::thread::scope(|scope| {
        // Ends once both ends of `to_carry` have gone, which is the two
        // below ending.
        let (to_carry, carrying) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("obelus carrying to the server".to_string())
            .spawn_scoped(scope, move || carry(&carrying, writing))
            .map_err(anyhow::Error::from)?;
        let asked_to_carry = to_carry.clone();
        // The connection on a thread of its own, and what the front end
        // says read here: the channel is this thread's to read and no
        // other's.
        let connecting = std::thread::Builder::new()
            .name("obelus server to front end".to_string())
            .spawn_scoped(scope, move || {
                let left = connection(
                    afar,
                    front,
                    here,
                    measured,
                    writing,
                    leaving,
                    &asked_to_carry,
                );
                drop(asked_to_carry);
                // And what is reading the front end, which is waiting on
                // it: told to stop, and woken to hear it.
                over.store(true, Ordering::Release);
                let _ = nudge.send(Event::Tick);
                left
            })
            .map_err(anyhow::Error::from)?;
        while let Ok(event) = done.recv() {
            if over.load(Ordering::Acquire) {
                break;
            }
            heard(&event);
            // A file dropped goes as what it holds, not where it is -- read
            // and sent behind the keys, not here ahead of them.
            if let Event::Dropped(path) = event {
                let _ = to_carry.send(Carry::Dropped(path));
                continue;
            }
            let closed = matches!(event, Event::Closed);
            let Some(input) = Input::of(event, measured) else {
                continue;
            };
            let mut writing = writing
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let sent = writing
                .as_mut()
                .is_some_and(|stream| obelus_wire::write(stream, &Up::Input(input)).is_ok());
            // Closed with nobody to ask: there is no Obelus to say whether
            // anything is unwritten, so the front end goes -- once the
            // connection has stopped knocking.
            if closed && !sent {
                leaving.store(true, Ordering::Release);
            }
        }
        drop(to_carry);
        connecting
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the connection's thread stopped")))
    })
}

/// What a front end draws once the Obelus at the other end has ended.
pub enum Next {
    /// Nothing: the reader has left.
    Done,
    /// An Obelus of this machine's -- and, where it shows a machine that
    /// went and is waiting for it, what stops the waiting.
    Here(
        Box<obelus_app::app::App>,
        Option<std::sync::Arc<AtomicBool>>,
    ),
}

/// What a front end draws once the Obelus at `afar` has ended as `left`:
/// nothing, a new one of this machine's, or one saying the machine went --
/// waiting for it, on `nudge`, where it is worth waiting for.
///
/// # Errors
///
/// Where an Obelus of this machine's would not start.
pub fn after(afar: &Afar, left: Left, nudge: &std::sync::mpsc::Sender<Event>) -> Result<Next> {
    match left {
        Left::Finished => Ok(Next::Done),
        Left::Home => Ok(Next::Here(
            Box::new(obelus_app::startup::start(&[], afar.build)?),
            None,
        )),
        Left::Lost { window, why, again } => {
            let mut app = obelus_app::startup::start(&[], afar.build)?;
            app.the_machine_has_gone(obelus_app::app::MachineGone {
                at: afar.at.clone(),
                key: afar.letters(),
                window,
                why: Some(why),
                trying: again,
            });
            let waiting = again.then(|| wait_for(afar, nudge.clone(), Duration::from_secs(1)));
            Ok(Next::Here(Box::new(app), waiting))
        }
    }
}

/// Where a front end goes once this machine's Obelus has ended: to the
/// Obelus on another machine the reader asked for, or nowhere.
///
/// # Errors
///
/// Where the key the reader gave is not one a server prints.
pub fn from_here(app: &obelus_app::app::App, build: &'static str) -> Result<Option<Afar>> {
    let Some(obelus_app::app::Going::To { at, key, window }) = app.goes_to().cloned() else {
        return Ok(None);
    };
    let key = self::key(&at, Some(key))?;
    Ok(Some(Afar {
        at,
        key,
        path: None,
        build,
        window,
    }))
}

/// The largest file dropped on a window that is sent to the machine
/// Obelus is on: well under what one frame may carry, and far more than a
/// picture or a log an agent is shown.
const DROPPED_MOST: u64 = 32 << 20;

/// A file dropped on the window, as it crosses: its name, and what it holds
/// or why it does not go -- a folder, a file too large, one that will not
/// read.
fn dropped(path: &std::path::Path) -> (String, Result<Vec<u8>, String>) {
    let name = path.file_name().map_or_else(
        || "dropped".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let file = match std::fs::metadata(path) {
        Ok(found) if found.is_dir() => Err(format!("A folder cannot be sent: {name}")),
        Ok(found) if found.len() > DROPPED_MOST => Err(format!(
            "Larger than {} MiB, so not sent: {name}",
            DROPPED_MOST >> 20
        )),
        Ok(_) => std::fs::read(path).map_err(|error| format!("Would not read {name}: {error}")),
        Err(error) => Err(format!("Would not read {name}: {error}")),
    };
    (name, file)
}

/// What the carrying thread is asked to send.
#[derive(Debug)]
enum Carry {
    /// A file dropped on the window.
    Dropped(std::path::PathBuf),
    /// The picture on the clipboard, to the question of that number.
    Picture(u64),
}

/// Reads and sends what is asked for, one after another, each a [`PIECE`]
/// at a time -- letting go of the connection between pieces, so that
/// whatever else is said goes between them.
///
/// What was on its way when the connection went is not sent again: the
/// Obelus at the other end lets go of what it had of it when a window comes
/// back (`ToChild::Attached`), and the question a picture answered has
/// stopped waiting.
fn carry(asked: &Receiver<Carry>, writing: &Mutex<Option<TcpStream>>) {
    let mut carrying = 0_u64;
    while let Ok(asked) = asked.recv() {
        carrying += 1;
        let carried = |bytes: &[u8]| Carried {
            carrying,
            length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        };
        let (said, bytes) = match asked {
            Carry::Dropped(path) => match dropped(&path) {
                (name, Ok(bytes)) => (
                    Up::Dropped {
                        name,
                        file: Ok(carried(&bytes)),
                    },
                    bytes,
                ),
                (name, Err(why)) => (
                    Up::Dropped {
                        name,
                        file: Err(why),
                    },
                    Vec::new(),
                ),
            },
            Carry::Picture(number) => match obelus_clipboard::picture() {
                Some((shape, bytes)) => (
                    Up::Pictured {
                        number,
                        picture: Some((shape, carried(&bytes))),
                    },
                    bytes,
                ),
                None => (
                    Up::Pictured {
                        number,
                        picture: None,
                    },
                    Vec::new(),
                ),
            },
        };
        if !said_up(writing, &said) {
            continue;
        }
        for piece in bytes.chunks(PIECE) {
            let piece = Up::Piece {
                carrying,
                bytes: piece.to_vec(),
            };
            if !said_up(writing, &piece) {
                break;
            }
            // The lock is nobody's turn in particular: given back this way,
            // a key waiting on it is the next to have it rather than this
            // thread again.
            std::thread::yield_now();
        }
    }
}

/// Says one thing up the connection there is now, and answers whether it
/// went.
fn said_up(writing: &Mutex<Option<TcpStream>>, up: &Up) -> bool {
    let Ok(mut writing) = writing.lock() else {
        return false;
    };
    writing
        .as_mut()
        .is_some_and(|stream| obelus_wire::write(stream, up).is_ok())
}

/// Knocks, hears, and knocks again when the connection goes, until there
/// is none to have.
fn connection(
    afar: &Afar,
    front: &Front,
    here: &dyn Showing,
    measured: &Measured,
    writing: &Mutex<Option<TcpStream>>,
    leaving: &AtomicBool,
    to_carry: &Sender<Carry>,
) -> Result<Left> {
    let mut window: Option<String> = afar.window.clone();
    // Since when the connection has been gone, for a window that was in.
    let mut gone_since: Option<std::time::Instant> = None;
    let mut wait = Duration::from_millis(250);
    loop {
        if leaving.load(Ordering::Acquire) {
            return Ok(Left::Finished);
        }
        let mut stream = match knock(afar, front, measured, window.as_deref()) {
            Ok((
                stream,
                Answered::Open {
                    window: name,
                    again,
                },
            )) => {
                tracing::info!(name, again, at = afar.address(), "let in");
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| {
                        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
                    });
                obelus_app::app::servers::remember(&afar.at, &hex(&afar.key), now);
                window = Some(name);
                wait = Duration::from_millis(250);
                stream
            }
            Ok((_, Answered::Refused(why))) => {
                return Ok(Left::Lost {
                    window,
                    why,
                    again: false,
                });
            }
            // A connection that only blinked is knocked on again for a
            // moment before the reader is told anything: most come back
            // before a page could have been read.
            Err(error) if gone_since.is_some_and(|since| since.elapsed() < WHILE_IT_BLINKS) => {
                tracing::info!(%error, ?wait, "the server is not answering yet");
                std::thread::sleep(wait);
                wait = (wait * 2).min(LONGEST);
                continue;
            }
            // Never in at all, or gone for longer than a blink: the reader
            // is told, and the page that tells them waits for the machine.
            // On the page, what was in the way and nothing of what Obelus
            // was doing: the page already says which machine. The words
            // Obelus put round it are for the log.
            Err(error) => {
                tracing::info!(error = format!("{error:#}"), "the server cannot be reached");
                return Ok(Left::Lost {
                    window,
                    why: error.root_cause().to_string(),
                    again: true,
                });
            }
        };
        if let (Ok(mut writing), Ok(clone)) = (writing.lock(), stream.try_clone()) {
            *writing = Some(clone);
        }
        let heard = hear(&mut stream, here, writing, to_carry);
        if let Ok(mut writing) = writing.lock() {
            *writing = None;
        }
        if let Some(left) = heard {
            return Ok(left);
        }
        tracing::info!("the connection to the server went; knocking again");
        gone_since = Some(std::time::Instant::now());
    }
}

/// Hears what the server says until the connection goes, and says why
/// where it went because Obelus there ended -- nothing for a connection
/// that only went.
fn hear(
    stream: &mut TcpStream,
    here: &dyn Showing,
    writing: &Mutex<Option<TcpStream>>,
    to_carry: &Sender<Carry>,
) -> Option<Left> {
    let answer = |up: &Up| {
        if let Ok(mut writing) = writing.lock()
            && let Some(stream) = writing.as_mut()
        {
            let _ = obelus_wire::write(stream, up);
        }
    };
    // Read through a buffer: a frame is one message, but each message is
    // still a length and then a body, and two reads of the socket for each
    // is two more than there need be.
    let mut reading = std::io::BufReader::new(&*stream);
    // The picture whose pieces are arriving, where one is.
    let mut arriving: Option<Arriving> = None;
    loop {
        match obelus_wire::read::<_, Down>(&mut reading) {
            Ok(Some(Down::Updates(updates))) => {
                // What ends a frame, or arrives with none behind it, is
                // what makes the front end look -- as `Cells` and the
                // picture's reader make it look on this machine. Once for
                // the lot, after the lot.
                let mut looks = false;
                for update in &updates {
                    looks |= matches!(update, Update::Frame | Update::Picture(_));
                    // A picture let go of is one whose pieces still to come
                    // are about a page that has gone.
                    if matches!(update, Update::Picture(None)) {
                        arriving = None;
                    }
                }
                // Handed over together, the way `Cells` hands over a frame
                // on this machine: a page scrolled is thousands of cells,
                // and a window taking them one at a time sends on its
                // channel once for each.
                if here.show_all(updates).is_err() {
                    return Some(Left::Finished);
                }
                if looks {
                    here.wake();
                }
            }
            Ok(Some(Down::Hung {
                number,
                wallpaper,
                length,
            })) => {
                arriving = Some(Arriving {
                    number,
                    wallpaper,
                    length,
                    bytes: Vec::new(),
                });
                if let Some(whole) = arriving.as_mut().and_then(|now| now.took(number, &[])) {
                    arriving = None;
                    if !hung(here, whole) {
                        return Some(Left::Finished);
                    }
                }
            }
            Ok(Some(Down::Piece { number, bytes })) => {
                if let Some(whole) = arriving.as_mut().and_then(|now| now.took(number, &bytes)) {
                    arriving = None;
                    if !hung(here, whole) {
                        return Some(Left::Finished);
                    }
                }
            }
            Ok(Some(Down::Copy(words))) => {
                if let Err(error) = obelus_clipboard::copy(&words) {
                    tracing::warn!(%error, "copying for the server");
                }
            }
            Ok(Some(Down::Paste(number))) => answer(&Up::Pasted {
                number,
                words: obelus_clipboard::paste(),
            }),
            // Megabytes, so behind the keys and not here: this thread would
            // stop hearing the screen for as long as they took to cross.
            Ok(Some(Down::Picture(number))) => {
                let _ = to_carry.send(Carry::Picture(number));
            }
            Ok(Some(Down::HoldsAPicture(number))) => answer(&Up::Holds {
                number,
                picture: obelus_clipboard::holds_a_picture(),
            }),
            Ok(Some(Down::Open(url))) => {
                if let Err(error) = obelus_clipboard::links::open(&url) {
                    tracing::warn!(%error, url, "opening a link for the server");
                }
            }
            Ok(Some(Down::Finished)) => return Some(Left::Finished),
            Ok(Some(Down::Home)) => return Some(Left::Home),
            Ok(None) => return None,
            Err(error) => {
                tracing::info!(%error, "the connection to the server broke");
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A front end that keeps what it was shown, and counts its looks.
    #[derive(Default)]
    struct Kept {
        shown: Mutex<Vec<Update>>,
        woken: std::sync::atomic::AtomicUsize,
    }

    impl Showing for Kept {
        fn show(&self, update: Update) -> Result<(), crate::Closed> {
            self.shown.lock().expect("what was shown").push(update);
            Ok(())
        }

        fn wake(&self) {
            self.woken.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A port nothing listens on, found by listening on one and letting it
    /// go.
    fn nothing_there() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let at = listener.local_addr().expect("an address");
        drop(listener);
        at.to_string()
    }

    /// What a front end draws before it knocks: nothing where the machine
    /// takes the connection, the page saying it cannot be reached where it
    /// said no at once, and the page saying it is connecting where it has
    /// said nothing yet.
    ///
    /// Broken deliberately by knocking whatever the first try says (`first`
    /// answering nothing): no page went up for a machine nobody is at. And
    /// by waiting out the first try however long it takes (`recv` for
    /// `recv_timeout` in `first_within`): the moment passed with nothing
    /// drawn, which is the blank window this is for.
    #[test]
    fn before_knocking_a_machine_is_drawn_as_what_it_has_said() {
        let afar = |at: String| Afar {
            at,
            key: vec![1],
            path: None,
            build: "the test",
            window: None,
        };
        let page = |next: Option<Next>| match next {
            Some(Next::Here(app, waiting)) => {
                if let Some(waiting) = waiting {
                    waiting.store(true, Ordering::Release);
                }
                app.machine_gone().map(|gone| gone.why.clone())
            }
            _ => None,
        };
        let (nudge, _heard) = std::sync::mpsc::channel();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let there = afar(listener.local_addr().expect("an address").to_string());
        assert!(
            first_within(&there, &nudge, Duration::from_secs(10))
                .expect("asked")
                .is_none(),
            "a page went up for a machine that is there"
        );

        let refused = page(
            first_within(&afar(nothing_there()), &nudge, Duration::from_secs(10)).expect("asked"),
        );
        assert!(
            matches!(refused, Some(Some(_))),
            "a machine that said no was not said to be gone: {refused:?}"
        );

        let still = page(first_within(&there, &nudge, Duration::ZERO).expect("asked"));
        assert_eq!(still, Some(None), "the page did not say it was connecting");
    }

    /// A machine that cannot be reached at all is said to be lost, worth
    /// asking again, and with no window there to take up.
    ///
    /// Broken deliberately by giving up on it for good (`again: false` for
    /// an error knocking): the page would never have waited for it.
    #[test]
    fn a_machine_nobody_answers_on_is_lost_and_worth_asking_again() {
        let afar = Afar {
            at: nothing_there(),
            key: vec![1],
            path: None,
            build: "the test",
            window: None,
        };
        let (nudge, done) = std::sync::mpsc::channel();
        let left = run(
            &afar,
            &Front::Window {
                screen: [1, 1],
                command_is_control: false,
            },
            &Kept::default(),
            &Measured::default(),
            &done,
            &nudge,
            &|_| {},
        )
        .expect("ran");
        assert!(
            matches!(
                left,
                Left::Lost {
                    window: None,
                    again: true,
                    ..
                }
            ),
            "{left:?}"
        );
    }

    /// A file dropped crosses as what it holds, under its own name; a
    /// folder, and a file that is not there, cross as why not.
    ///
    /// Broken deliberately by sending a folder as if it were a file (the
    /// `is_dir` arm left out): it crossed as a read that failed, with no
    /// word for the reader about what they had dropped.
    #[test]
    fn a_dropped_file_crosses_as_what_it_holds() {
        let directory =
            std::env::temp_dir().join(format!("obelus-dropping-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a directory");
        let file = directory.join("shot.png");
        std::fs::write(&file, b"pixels").expect("the file");
        assert_eq!(
            dropped(&file),
            ("shot.png".to_string(), Ok(b"pixels".to_vec()))
        );
        assert!(matches!(
            dropped(&directory),
            (_, Err(why)) if why.starts_with("A folder")
        ));
        assert!(matches!(dropped(&directory.join("not-there")), (_, Err(_))));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A file dropped goes up as what it is and then its bytes, a piece at
    /// a time, so that a key pressed meanwhile can go between the pieces.
    ///
    /// Broken deliberately by sending it whole (`bytes.chunks(PIECE)` in
    /// `carry` made `bytes.chunks(bytes.len().max(1))`): the file went as
    /// one piece, which every key behind it waited for.
    #[test]
    fn a_dropped_file_goes_up_a_piece_at_a_time() {
        let directory =
            std::env::temp_dir().join(format!("obelus-carrying-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a directory");
        let file = directory.join("log.txt");
        let bytes: Vec<u8> = (0..200_000_u32).map(|at| (at % 251) as u8).collect();
        std::fs::write(&file, &bytes).expect("the file");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let stream =
            TcpStream::connect(listener.local_addr().expect("an address")).expect("connected");
        let (mut server, _) = listener.accept().expect("the window");
        // Read as it is sent: more than a socket holds, so a reader that
        // waited for the end would wait for ever.
        let reading = std::thread::spawn(move || {
            let mut heard = Vec::new();
            while let Some(up) = obelus_wire::read::<_, Up>(&mut server).expect("read") {
                heard.push(up);
            }
            heard
        });
        let writing = Mutex::new(Some(stream));
        let (to_carry, carrying) = std::sync::mpsc::channel();
        to_carry.send(Carry::Dropped(file)).expect("asked");
        drop(to_carry);
        carry(&carrying, &writing);
        drop(writing);
        let heard = reading.join().expect("read");
        let Some(Up::Dropped {
            name,
            file: Ok(Carried { carrying, length }),
        }) = heard.first().cloned()
        else {
            panic!("not what the file is first: {:?}", heard.first());
        };
        assert_eq!(name, "log.txt");
        assert_eq!(length, 200_000);
        let pieces: Vec<&Vec<u8>> = heard[1..]
            .iter()
            .map(|up| match up {
                Up::Piece {
                    carrying: which,
                    bytes,
                } if *which == carrying => bytes,
                other => panic!("not a piece of it: {other:?}"),
            })
            .collect();
        assert!(pieces.len() > 1, "it went whole");
        assert!(pieces.iter().all(|piece| piece.len() <= PIECE));
        assert_eq!(
            pieces.into_iter().flatten().copied().collect::<Vec<u8>>(),
            bytes
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A frame arrives at the front end as one handing over, not a cell at
    /// a time.
    ///
    /// Broken deliberately by handing them over one at a time (`show` for
    /// each update in `hear`, as it was): a frame of a hundred cells was a
    /// hundred and one handings over.
    #[test]
    fn a_frame_is_handed_to_the_front_end_at_once() {
        #[derive(Default)]
        struct Counted {
            handings: std::sync::atomic::AtomicUsize,
        }
        impl Showing for Counted {
            fn show(&self, _: Update) -> Result<(), crate::Closed> {
                self.handings.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }

            fn show_all(&self, _: Vec<Update>) -> Result<(), crate::Closed> {
                self.handings.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }

            fn wake(&self) {}
        }

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let at = listener.local_addr().expect("an address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the window");
            let mut cell = ratatui::buffer::Cell::default();
            cell.set_symbol("z");
            let mut frame: Vec<Update> = (0..100)
                .map(|x| Update::Cell {
                    x,
                    y: 0,
                    cell: Box::new(cell.clone()),
                })
                .collect();
            frame.push(Update::Frame);
            obelus_wire::write(&mut stream, &Down::Updates(frame)).expect("said");
            obelus_wire::write(&mut stream, &Down::Finished).expect("said");
        });
        let mut stream = TcpStream::connect(at).expect("connected");
        let writing = Mutex::new(Some(stream.try_clone().expect("both halves")));
        let here = Counted::default();
        let (to_carry, _carrying) = std::sync::mpsc::channel();
        assert_eq!(
            hear(&mut stream, &here, &writing, &to_carry),
            Some(Left::Finished)
        );
        server.join().expect("the server");
        assert_eq!(here.handings.load(Ordering::Relaxed), 1);
    }

    /// Waiting for a machine says so once it answers.
    ///
    /// Broken deliberately by waiting without asking (`answers` left out
    /// of `wait_for`'s loop): nothing was ever said.
    #[test]
    fn a_machine_waited_for_is_said_to_answer_once_it_does() {
        let at = nothing_there();
        let afar = Afar {
            at: at.clone(),
            key: Vec::new(),
            path: None,
            build: "the test",
            window: None,
        };
        let (nudge, heard) = std::sync::mpsc::channel();
        let stop = wait_for(&afar, nudge, Duration::from_secs(1));
        let listener = std::net::TcpListener::bind(&at).expect("the machine back");
        let said = heard
            .recv_timeout(Duration::from_secs(20))
            .expect("nothing said the machine answers");
        assert!(matches!(said, Event::Reachable(ref named) if *named == at));
        stop.store(true, Ordering::Release);
        drop(listener);
    }

    /// A try at a machine waited for that comes to nothing says why, for
    /// the page that said it was connecting -- and says it at once where
    /// asked to.
    ///
    /// Broken deliberately by keeping quiet about a try that failed (no
    /// `Event::Unreachable` in `wait_for`): nothing was heard, and the page
    /// said it was connecting to a machine that had already said no.
    #[test]
    fn a_try_at_a_machine_waited_for_says_why_it_came_to_nothing() {
        let at = nothing_there();
        let afar = Afar {
            at: at.clone(),
            key: Vec::new(),
            path: None,
            build: "the test",
            window: None,
        };
        let (nudge, heard) = std::sync::mpsc::channel();
        let stop = wait_for(&afar, nudge, Duration::ZERO);
        let said = heard
            .recv_timeout(Duration::from_secs(10))
            .expect("nothing said why");
        stop.store(true, Ordering::Release);
        assert!(
            matches!(said, Event::Unreachable { at: ref named, ref why } if *named == at && !why.is_empty()),
            "{said:?}"
        );
    }

    /// A machine named alone is reached at the server's own port, and one
    /// named with a port at that one.
    ///
    /// Broken deliberately by always adding the server's port: a port the
    /// reader gave became `host:9000:7787`.
    #[test]
    fn a_port_given_is_the_port_used() {
        let at = |at: &str| Afar {
            at: at.to_string(),
            key: Vec::new(),
            path: None,
            build: "the test",
            window: None,
        };
        assert_eq!(at("devbox").address(), "devbox:7787");
        assert_eq!(at("devbox:9000").address(), "devbox:9000");
        assert_eq!(at("10.0.0.2").address(), "10.0.0.2:7787");
    }

    /// What the server says reaches the front end, the front end looks when
    /// a frame ends, a question about the reader's clipboard is answered
    /// from this machine's, and Obelus there ending is said to be the end.
    ///
    /// Broken deliberately by leaving the clipboard's question unanswered
    /// (no `answer` for `Down::Paste`): the server waited for nothing.
    /// A picture that arrives in pieces is hung once it is whole, and one
    /// let go of before it was whole is not hung at all -- not even when the
    /// rest of its pieces come after.
    ///
    /// Broken deliberately by keeping the pieces of a picture let go of
    /// (the `arriving = None` for `Update::Picture(None)` left out of
    /// `hear`): the first picture was hung, whole, after the page had let
    /// go of it -- an old theme's picture under the new one's colours.
    #[test]
    fn a_picture_arrives_in_pieces_and_one_let_go_of_is_not_hung() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let at = listener.local_addr().expect("an address");
        let wallpaper = obelus_theme::Wallpaper {
            file: std::path::PathBuf::from("dusk.jpg"),
            opacity: 80,
            fit: obelus_theme::wallpaper::Fit::Cover,
            anchor: obelus_theme::wallpaper::Anchor::default(),
            modified: None,
        };
        let said = wallpaper.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the window");
            for down in [
                Down::Hung {
                    number: 1,
                    wallpaper: said.clone(),
                    length: 6,
                },
                Down::Piece {
                    number: 1,
                    bytes: vec![1, 2, 3],
                },
                Down::Updates(vec![Update::Picture(None)]),
                Down::Piece {
                    number: 1,
                    bytes: vec![4, 5, 6],
                },
                Down::Hung {
                    number: 2,
                    wallpaper: said,
                    length: 4,
                },
                Down::Piece {
                    number: 2,
                    bytes: vec![7, 8],
                },
                Down::Piece {
                    number: 2,
                    bytes: vec![9, 9],
                },
                Down::Finished,
            ] {
                obelus_wire::write(&mut stream, &down).expect("said");
            }
        });
        let mut stream = TcpStream::connect(at).expect("connected");
        let writing = Mutex::new(Some(stream.try_clone().expect("both halves")));
        let here = Kept::default();
        let (to_carry, _carrying) = std::sync::mpsc::channel();
        assert_eq!(
            hear(&mut stream, &here, &writing, &to_carry),
            Some(Left::Finished)
        );
        server.join().expect("the server");
        let hung: Vec<Vec<u8>> = here
            .shown
            .lock()
            .expect("what was shown")
            .iter()
            .filter_map(|update| match update {
                Update::Picture(Some(hanging)) => Some(hanging.bytes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(hung, [vec![7, 8, 9, 9]], "not the one picture, whole");
        let _ = wallpaper;
    }

    #[test]
    fn what_the_server_says_reaches_the_front_end_and_is_answered() {
        obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
        obelus_clipboard::copy("on this machine").expect("the clipboard here");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let at = listener.local_addr().expect("an address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the window");
            let mut cell = ratatui::buffer::Cell::default();
            cell.set_symbol("z");
            for down in [
                Down::Updates(vec![
                    Update::Cell {
                        x: 1,
                        y: 2,
                        cell: Box::new(cell),
                    },
                    Update::Frame,
                ]),
                Down::Paste(5),
            ] {
                obelus_wire::write(&mut stream, &down).expect("said");
            }
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("a timeout");
            let answer: Up = obelus_wire::read(&mut stream)
                .expect("read")
                .expect("an answer");
            obelus_wire::write(&mut stream, &Down::Finished).expect("said");
            answer
        });
        let mut stream = TcpStream::connect(at).expect("connected");
        let writing = Mutex::new(Some(stream.try_clone().expect("both halves")));
        let here = Kept::default();
        let (to_carry, _carrying) = std::sync::mpsc::channel();
        assert_eq!(
            hear(&mut stream, &here, &writing, &to_carry),
            Some(Left::Finished),
            "the end was not heard as one"
        );
        assert_eq!(
            server.join().expect("the server"),
            Up::Pasted {
                number: 5,
                words: Some("on this machine".to_string()),
            }
        );
        let arrived = here.shown.lock().expect("what was shown");
        assert!(
            arrived
                .iter()
                .any(|update| matches!(update, Update::Cell { x: 1, y: 2, .. })),
            "the cell did not reach the front end: {arrived:?}"
        );
        assert!(
            here.woken.load(Ordering::Relaxed) >= 1,
            "the front end never looked"
        );
    }
}
