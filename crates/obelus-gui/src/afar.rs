//! A window on an Obelus running on another machine: `obg --connect`.
//!
//! **The window is the same window.** What arrives is what Obelus on this
//! machine would have put on the window's channel -- the frames, what the
//! settings tell a window, the shapes and the marks -- and it goes into
//! that channel through the same door (`grid::Here`), the picture under
//! the page decoded here as it always is. What the reader does goes back
//! the way it would have reached Obelus's loop, as an `Input`. Nothing
//! else about the window knows the difference.
//!
//! **What only this machine can do is done here.** The reader's clipboard
//! and browser are this machine's, so what Obelus there asks of them is
//! answered here (`obelus_front::link::Down`).
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
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_app::event::Event;
use obelus_front::{
    Input, Measured, Showing as _, Update,
    link::{Answered, Challenge, Down, Front, Knock, Up},
};

/// The longest wait between two knocks on a server that has gone quiet.
const LONGEST: Duration = Duration::from_secs(10);

/// Where a server listens when the reader names only the machine.
const PORT: u16 = 7787;

/// The server a window is for, and what it is to open there.
#[derive(Debug)]
pub(crate) struct Afar {
    /// The machine, and the port where one was given.
    pub(crate) at: String,
    /// The key it printed.
    pub(crate) key: Vec<u8>,
    /// What to open there, as `ob path` would be told.
    pub(crate) path: Option<String>,
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

/// The key for a server: the one the reader gave, kept for next time, or
/// the one kept from last time.
///
/// # Errors
///
/// Where there is none, or it is not the letters a server prints.
pub(crate) fn key(at: &str, given: Option<String>) -> Result<Vec<u8>> {
    let kept = obelus_logging::state_directory().map(|directory| {
        let file: String = at
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        directory.join("servers").join(file)
    });
    let letters = match given {
        Some(given) => {
            if let Some(kept) = &kept {
                if let Some(parent) = kept.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(error) = std::fs::write(kept, given.trim()) {
                    tracing::warn!(%error, "the key was not kept for next time");
                }
            }
            given
        }
        None => kept
            .and_then(|kept| std::fs::read_to_string(kept).ok())
            .context("no key: give the one the server printed with --key")?,
    };
    let letters = letters.trim();
    (0..letters.len())
        .step_by(2)
        .map(|at| {
            letters
                .get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .filter(|key| !key.is_empty() && letters.len() % 2 == 0)
        .context("that key is not one a server prints")
}

/// Knocks on the server, and says what it answered and the connection.
fn knock(
    afar: &Afar,
    measured: &Measured,
    screen: [u32; 2],
    window: Option<&str>,
) -> Result<(TcpStream, Answered)> {
    let mut stream = TcpStream::connect(afar.address())
        .with_context(|| format!("connecting to {}", afar.address()))?;
    stream.set_nodelay(true)?;
    let ours = obelus_wire::Hello::here(env!("OBELUS_BUILD"));
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
            signed: obelus_front::link::sign(&afar.key, &challenge),
            front: Front::Window {
                screen,
                command_is_control: crate::keys::COMMAND_IS_CONTROL,
            },
            size: [size.width, size.height, pixels.width, pixels.height],
            path: afar.path.clone(),
            window: window.map(str::to_string),
        },
    )?;
    let answered: Answered =
        obelus_wire::read(&mut stream)?.context("the server went before it answered")?;
    Ok((stream, answered))
}

/// Starts the window's connection to `afar`, with what arrives going to
/// the window through `here` and what the window says arriving on `done`.
///
/// The window is told to go when Obelus there has ended, or will not let
/// it in, with what became of it in `outcome`.
///
/// # Errors
///
/// Where there is no thread to keep the connection on.
pub(crate) fn start(
    afar: Afar,
    here: Arc<crate::grid::Here>,
    measured: Arc<Measured>,
    done: Receiver<Event>,
    proxy: winit::event_loop::EventLoopProxy<crate::window::Waking>,
    outcome: Arc<Mutex<Option<Result<()>>>>,
    screen: [u32; 2],
) -> std::io::Result<()> {
    let writing: Arc<Mutex<Option<TcpStream>>> = Arc::default();
    let leaving = Arc::new(AtomicBool::new(false));

    // What the window says, to whichever connection there is now.
    let (said_to, said_size, said_leaving) = (
        Arc::clone(&writing),
        Arc::clone(&measured),
        Arc::clone(&leaving),
    );
    let telling = proxy.clone();
    std::thread::Builder::new()
        .name("obelus window to server".to_string())
        .spawn(move || {
            while let Ok(event) = done.recv() {
                let closed = matches!(event, Event::Closed);
                let Some(input) = Input::of(event, &said_size) else {
                    continue;
                };
                let mut writing = said_to
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let sent = writing
                    .as_mut()
                    .is_some_and(|stream| obelus_wire::write(stream, &Up::Input(input)).is_ok());
                // Closed with nobody to ask: there is no Obelus to say
                // whether anything is unwritten, so the window goes.
                if closed && !sent {
                    said_leaving.store(true, Ordering::Release);
                    let _ = telling.send_event(crate::window::Waking::Finished);
                    return;
                }
            }
        })?;

    std::thread::Builder::new()
        .name("obelus server to window".to_string())
        .spawn(move || {
            let _finished = crate::window::Finished(proxy);
            let mut window: Option<String> = None;
            let mut wait = Duration::from_millis(250);
            loop {
                if leaving.load(Ordering::Acquire) {
                    return;
                }
                let knocked = knock(&afar, &measured, screen, window.as_deref());
                let mut stream = match knocked {
                    Ok((
                        stream,
                        Answered::Open {
                            window: name,
                            again,
                        },
                    )) => {
                        tracing::info!(name, again, at = afar.address(), "let in");
                        window = Some(name);
                        wait = Duration::from_millis(250);
                        stream
                    }
                    Ok((_, Answered::Refused(why))) => {
                        *outcome
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            Some(Err(anyhow::anyhow!(why)));
                        return;
                    }
                    // Never in at all: the machine or the port is wrong,
                    // and saying so beats a window waiting on nothing.
                    Err(error) if window.is_none() => {
                        *outcome
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Err(error));
                        return;
                    }
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
                let ended = hear(&mut stream, &here, &writing);
                if let Ok(mut writing) = writing.lock() {
                    *writing = None;
                }
                if ended {
                    *outcome
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Ok(()));
                    return;
                }
                tracing::info!("the connection to the server went; knocking again");
            }
        })?;
    Ok(())
}

/// Hears what the server says until the connection goes, and says whether
/// it went because Obelus there ended.
fn hear(
    stream: &mut TcpStream,
    here: &crate::grid::Here,
    writing: &Mutex<Option<TcpStream>>,
) -> bool {
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
                // what makes the window look -- as `Cells` and the picture's
                // reader make it look on this machine.
                let looks = matches!(update, Update::Frame | Update::Picture(_));
                if here.show(update).is_err() {
                    return true;
                }
                if looks {
                    obelus_front::Showing::wake(here);
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
            Ok(Some(Down::Finished)) => return true,
            Ok(None) => return false,
            Err(error) => {
                tracing::info!(%error, "the connection to the server broke");
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert_eq!(at("devbox").address(), "devbox:7787");
        assert_eq!(at("devbox:9000").address(), "devbox:9000");
        assert_eq!(at("10.0.0.2").address(), "10.0.0.2:7787");
    }

    /// What the server says reaches the window, the window looks when a
    /// frame ends, a question about the reader's clipboard is answered
    /// from this machine's, and Obelus there ending is said to be the end.
    ///
    /// Broken deliberately by leaving the clipboard's question unanswered
    /// (no `answer` for `Down::Paste`): the server waited for nothing.
    #[test]
    fn what_the_server_says_reaches_the_window_and_is_answered() {
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
        let (frames, drawn) = std::sync::mpsc::channel();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wakes = Arc::clone(&woken);
        let here = crate::grid::Here::new(
            frames,
            Arc::new(move || {
                wakes.fetch_add(1, Ordering::Relaxed);
            }),
            [1920, 1080],
        );
        assert!(
            hear(&mut stream, &here, &writing),
            "the end was not heard as one"
        );
        assert_eq!(
            server.join().expect("the server"),
            Up::Pasted {
                number: 5,
                words: Some("on this machine".to_string()),
            }
        );
        let arrived: Vec<_> = drawn.try_iter().collect();
        assert!(
            arrived
                .iter()
                .any(|update| matches!(update, Update::Cell { x: 1, y: 2, .. })),
            "the cell did not reach the window: {arrived:?}"
        );
        assert!(
            woken.load(Ordering::Relaxed) >= 1,
            "the window never looked"
        );
    }
}
