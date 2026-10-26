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
        mpsc::Receiver,
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_app::event::Event;

use crate::{
    Input, Measured, Showing, Update,
    link::{Answered, Challenge, Down, Front, Knock, Up},
};

/// The longest wait between two knocks on a server that has gone quiet.
const LONGEST: Duration = Duration::from_secs(10);

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
}

impl Afar {
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
    unhex(letters.trim()).context("that key is not one a server prints")
}

/// The letters a server prints, as the key they are.
fn unhex(letters: &str) -> Option<Vec<u8>> {
    if letters.is_empty() || !letters.len().is_multiple_of(2) {
        return None;
    }
    (0..letters.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(letters.get(at..at + 2)?, 16).ok())
        .collect()
}

/// The key as the letters a server prints.
fn hex(key: &[u8]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Knocks on the server, and says what it answered and the connection.
fn knock(
    afar: &Afar,
    front: &Front,
    measured: &Measured,
    window: Option<&str>,
) -> Result<(TcpStream, Answered)> {
    let mut stream = TcpStream::connect(afar.address())
        .with_context(|| format!("connecting to {}", afar.address()))?;
    stream.set_nodelay(true)?;
    let ours = obelus_wire::Hello::here(afar.build);
    obelus_wire::write(&mut stream, &ours)?;
    let theirs: obelus_wire::Hello =
        obelus_wire::read(&mut stream)?.context("the server went before it said hello")?;
    if let Some(why) = ours.refused(&theirs) {
        anyhow::bail!(why);
    }
    let challenge: Challenge =
        obelus_wire::read(&mut stream)?.context("the server went before it asked for the key")?;
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
        obelus_wire::read(&mut stream)?.context("the server went before it answered")?;
    Ok((stream, answered))
}

/// Why there is no more connection to have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// The Obelus there ended, or the reader left with nobody there to ask.
    Finished,
    /// The reader asked to come back to this machine.
    Home,
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
/// Where there was never a connection, or the server would not let it in.
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
    std::thread::scope(|scope| {
        // The connection on a thread of its own, and what the front end
        // says read here: the channel is this thread's to read and no
        // other's.
        let connecting = std::thread::Builder::new()
            .name("obelus server to front end".to_string())
            .spawn_scoped(scope, || {
                let left = connection(afar, front, here, measured, &writing, &leaving);
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
        connecting
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("the connection's thread stopped")))
    })
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
) -> Result<Left> {
    let mut window: Option<String> = None;
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
            Ok((_, Answered::Refused(why))) => anyhow::bail!(why),
            // Never in at all: the machine or the port is wrong, and saying
            // so beats a window waiting on nothing.
            Err(error) if window.is_none() => return Err(error),
            Err(error) => {
                tracing::info!(%error, ?wait, "the server is not answering yet");
                std::thread::sleep(wait);
                wait = (wait * 2).min(LONGEST);
                continue;
            }
        };
        if let (Ok(mut writing), Ok(clone)) = (writing.lock(), stream.try_clone()) {
            *writing = Some(clone);
        }
        let heard = hear(&mut stream, here, writing);
        if let Ok(mut writing) = writing.lock() {
            *writing = None;
        }
        if let Some(left) = heard {
            return Ok(left);
        }
        tracing::info!("the connection to the server went; knocking again");
    }
}

/// Hears what the server says until the connection goes, and says why
/// where it went because Obelus there ended -- nothing for a connection
/// that only went.
fn hear(
    stream: &mut TcpStream,
    here: &dyn Showing,
    writing: &Mutex<Option<TcpStream>>,
) -> Option<Left> {
    let answer = |up: &Up| {
        if let Ok(mut writing) = writing.lock()
            && let Some(stream) = writing.as_mut()
        {
            let _ = obelus_wire::write(stream, up);
        }
    };
    loop {
        match obelus_wire::read::<_, Down>(stream) {
            Ok(Some(Down::Update(update))) => {
                // What ends a frame, or arrives with none behind it, is
                // what makes the front end look -- as `Cells` and the
                // picture's reader make it look on this machine.
                let looks = matches!(update, Update::Frame | Update::Picture(_));
                if here.show(update).is_err() {
                    return Some(Left::Finished);
                }
                if looks {
                    here.wake();
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
            Ok(Some(Down::Picture(number))) => answer(&Up::Pictured {
                number,
                picture: obelus_clipboard::picture(),
            }),
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
                Down::Update(Update::Cell {
                    x: 1,
                    y: 2,
                    cell: Box::new(cell),
                }),
                Down::Update(Update::Frame),
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
        assert_eq!(
            hear(&mut stream, &here, &writing),
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
