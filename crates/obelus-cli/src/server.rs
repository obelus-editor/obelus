//! `ob --server`: Obelus for windows on other machines.
//!
//! **One Obelus for each window.** A window that knocks and is let in gets
//! an `ob --serve-one` of its own -- its own open files, agent and
//! terminals, the way a second Obelus started on this machine would have
//! them -- and this carries the bytes between the two. Nothing of what is
//! said is read here past the knock: a frame from the window is handed on
//! as it came, wrapped in what says it is the window's, and a frame from
//! the Obelus is handed back as it came.
//!
//! **A key, and a knock signed with it.** The key is made the first time
//! and kept where this machine's Obelus keeps its state, readable by its
//! owner alone; it is printed when the server starts, which is how the
//! reader carries it to the window. A knock that is not signed with it is
//! told so and let go of, with nothing said before (`obelus_front::link`).
//!
//! **A window that goes is waited for.** A connection that ends without the
//! Obelus behind it ending -- a network that went, a laptop closed --
//! leaves the Obelus running and the window's name kept, and a knock with
//! that name takes it up where it was. One nobody comes back to for
//! [`GRACE`] is stopped, the way a kill stops one. A window the reader
//! closed is not waiting for anybody: its Obelus ends with it, and so does
//! its name.

use std::{
    collections::HashMap,
    io::Write as _,
    net::{SocketAddr, TcpListener, TcpStream},
    process::{ChildStdin, ExitCode, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_front::link::{Answered, Challenge, Knock, ToChild, Up};

/// How long a window that went is waited for before its Obelus is
/// stopped -- what VS Code waits for a window of its own.
const GRACE: Duration = Duration::from_secs(3 * 60 * 60);

/// Where a server listens when it is not told.
pub(crate) const PORT: u16 = 7787;

/// One window's Obelus, and whoever is reading it now.
struct Window {
    /// What is written to it.
    input: Arc<Mutex<ChildStdin>>,
    /// The connection its frames go to, while one is there.
    reading: Arc<Mutex<Option<TcpStream>>>,
    /// How many times a connection has come to it, so that a wait for one
    /// that went knows whether another has come since.
    comings: u64,
}

/// The window a knock was let into.
struct Entered {
    /// Its name.
    name: String,
    /// Whether it was there already.
    again: bool,
    /// What is written to its Obelus.
    input: Arc<Mutex<ChildStdin>>,
    /// Where its Obelus's frames go.
    reading: Arc<Mutex<Option<TcpStream>>>,
    /// Which coming of a connection to it this is.
    coming: u64,
}

/// Every window's Obelus, by the window's name.
type Windows = Arc<Mutex<HashMap<String, Window>>>;

/// Listens on `at` until stopped, letting in every window that knocks with
/// the key.
///
/// # Errors
///
/// Where there is nowhere to keep the key, or nothing to listen on.
pub(crate) fn serve(at: SocketAddr) -> Result<ExitCode> {
    let key = key().context("the key a window knocks with")?;
    let listener = TcpListener::bind(at).with_context(|| format!("listening on {at}"))?;
    let at = listener.local_addr().unwrap_or(at);
    // Said on the terminal the server was started from, which is the one
    // place the reader can read it off to carry to the window. Not a frame
    // and not a screen: nothing draws here.
    println!("Obelus is listening on {at}.");
    println!("A window gets in with the key {}.", hex(&key));
    tracing::info!(%at, "a server for windows on other machines");
    let windows: Windows = Arc::default();
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "a connection that did not arrive");
                continue;
            }
        };
        let (key, windows) = (key.clone(), Arc::clone(&windows));
        let started = std::thread::Builder::new()
            .name("obelus server connection".to_string())
            .spawn(move || {
                let from = stream.peer_addr().ok();
                if let Err(error) = let_in(stream, &key, &windows) {
                    tracing::info!(?from, %error, "a connection ended");
                }
            });
        if let Err(error) = started {
            tracing::warn!(%error, "no thread for a connection");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Answers one connection: the knock, and then the window's frames until
/// it goes.
fn let_in(mut stream: TcpStream, key: &[u8], windows: &Windows) -> Result<()> {
    stream.set_nodelay(true)?;
    let ours = obelus_wire::Hello::here(env!("OBELUS_BUILD"));
    obelus_wire::write(&mut stream, &ours)?;
    let theirs: obelus_wire::Hello =
        obelus_wire::read(&mut stream)?.context("the window went before it said hello")?;
    if let Some(why) = ours.refused(&theirs) {
        anyhow::bail!(why);
    }
    let mut number = [0_u8; 32];
    getrandom::fill(&mut number).map_err(|error| anyhow::anyhow!("{error}"))?;
    let challenge = Challenge(number);
    obelus_wire::write(&mut stream, &challenge)?;
    let knock: Knock =
        obelus_wire::read(&mut stream)?.context("the window went before it knocked")?;
    if !obelus_front::link::signed_by(key, &challenge, &knock.signed) {
        obelus_wire::write(
            &mut stream,
            &Answered::Refused("That is not this server's key".to_string()),
        )?;
        anyhow::bail!("a knock not signed with the key");
    }

    let Entered {
        name,
        again,
        input,
        reading,
        coming,
    } = match taken_up(&knock, windows) {
        Some(found) => found,
        None => started(&knock, windows)?,
    };
    obelus_wire::write(
        &mut stream,
        &Answered::Open {
            window: name.clone(),
            again,
        },
    )?;
    // Only now, after the answer, so that nothing of the Obelus's reaches
    // the window ahead of it.
    if let Ok(mut reading) = reading.lock() {
        *reading = Some(stream.try_clone()?);
    }
    if again {
        tell(&input, &ToChild::Attached { size: knock.size });
    }
    tracing::info!(name, again, "a window came in");

    let heard = loop {
        match obelus_wire::read::<_, Up>(&mut stream) {
            Ok(Some(up)) => tell(&input, &ToChild::Up(up)),
            Ok(None) => break "the window went".to_string(),
            Err(error) => break error.to_string(),
        }
    };
    went(&name, coming, windows);
    anyhow::bail!(heard)
}

/// The Obelus of a window that knocks with the name of one whose
/// connection went, where there is one nobody else has taken up since.
fn taken_up(knock: &Knock, windows: &Windows) -> Option<Entered> {
    let name = knock.window.clone()?;
    let mut windows = windows.lock().ok()?;
    let window = windows.get_mut(&name)?;
    // A name already being read is somebody else's window: two windows
    // cannot be the same one.
    if window.reading.lock().ok()?.is_some() {
        return None;
    }
    window.comings += 1;
    Some(Entered {
        name,
        again: true,
        input: Arc::clone(&window.input),
        reading: Arc::clone(&window.reading),
        coming: window.comings,
    })
}

/// A new Obelus for a new window.
fn started(knock: &Knock, windows: &Windows) -> Result<Entered> {
    let mut named = [0_u8; 12];
    getrandom::fill(&mut named).map_err(|error| anyhow::anyhow!("{error}"))?;
    let name = hex(&named);
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("--serve-one")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("an Obelus for the window")?;
    let input = Arc::new(Mutex::new(child.stdin.take().context("its input")?));
    let mut output = child.stdout.take().context("its output")?;
    tell(
        &input,
        &ToChild::Open {
            front: knock.front.clone(),
            size: knock.size,
            path: knock.path.clone(),
        },
    );
    let reading: Arc<Mutex<Option<TcpStream>>> = Arc::default();
    let carried = Arc::clone(&reading);
    let gone = Arc::clone(windows);
    let whose = name.clone();
    std::thread::Builder::new()
        .name("obelus server window".to_string())
        .spawn(move || {
            // Every frame as it came, to whoever is reading now, and to
            // nobody while nobody is: the Obelus says everything again to
            // the next window that comes.
            while let Some(frame) = framed(&mut output) {
                let Ok(mut reading) = carried.lock() else {
                    break;
                };
                if let Some(stream) = reading.as_mut()
                    && stream.write_all(&frame).is_err()
                {
                    *reading = None;
                }
            }
            // Its Obelus has ended, and with it the window and its name.
            if let Ok(mut gone) = gone.lock() {
                gone.remove(&whose);
            }
            if let Ok(mut reading) = carried.lock()
                && let Some(stream) = reading.take()
            {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
            let _ = child.wait();
            tracing::info!(whose, "a window's Obelus ended");
        })?;
    if let Ok(mut windows) = windows.lock() {
        windows.insert(
            name.clone(),
            Window {
                input: Arc::clone(&input),
                reading: Arc::clone(&reading),
                comings: 1,
            },
        );
    }
    Ok(Entered {
        name,
        again: false,
        input,
        reading,
        coming: 1,
    })
}

/// A window's connection went: its Obelus is told nobody is reading, and is
/// stopped if nobody has come back for it by the time the grace is up.
fn went(name: &str, coming: u64, windows: &Windows) {
    let input = {
        let Ok(windows) = windows.lock() else {
            return;
        };
        let Some(window) = windows.get(name) else {
            return;
        };
        if window.comings != coming {
            return;
        }
        if let Ok(mut reading) = window.reading.lock() {
            *reading = None;
        }
        Arc::clone(&window.input)
    };
    tell(&input, &ToChild::Detached);
    tracing::info!(name, "a window went, and is waited for");
    let (name, windows) = (name.to_string(), Arc::clone(windows));
    // One moment, waited for once: nothing else could say the grace is up.
    let _ = std::thread::Builder::new()
        .name("obelus server grace".to_string())
        .spawn(move || {
            std::thread::sleep(GRACE);
            let still = windows.lock().ok().and_then(|windows| {
                let window = windows.get(&name)?;
                let nobody = window.reading.lock().ok()?.is_none();
                (window.comings == coming && nobody).then(|| Arc::clone(&window.input))
            });
            if let Some(input) = still {
                tracing::info!(name, "nobody came back for a window, so it stops");
                tell(&input, &ToChild::Leave);
            }
        });
}

/// Says something to a window's Obelus.
fn tell(input: &Mutex<ChildStdin>, said: &ToChild) {
    if let Ok(mut input) = input.lock() {
        let _ = obelus_wire::write(&mut *input, said);
    }
}

/// One frame of what a window's Obelus said, length and all, as it came --
/// or nothing once it has stopped saying anything.
fn framed(reading: &mut impl std::io::Read) -> Option<Vec<u8>> {
    let mut length = [0_u8; 4];
    reading.read_exact(&mut length).ok()?;
    let body = u32::from_le_bytes(length) as usize;
    if body > obelus_wire::MOST {
        tracing::warn!(body, "a frame larger than any Obelus says");
        return None;
    }
    let mut frame = Vec::with_capacity(4 + body);
    frame.extend_from_slice(&length);
    frame.resize(4 + body, 0);
    reading.read_exact(&mut frame[4..]).ok()?;
    Some(frame)
}

/// The key, made the first time and kept after.
fn key() -> Result<Vec<u8>> {
    let directory = obelus_logging::state_directory().context("no state directory")?;
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("server.key");
    if let Ok(kept) = std::fs::read_to_string(&path)
        && let Some(key) = unhex(kept.trim())
    {
        return Ok(key);
    }
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        // Its owner's alone: the key is the whole of what stands between a
        // connection and running anything here.
        file.mode(0o600);
    }
    file.open(&path)?.write_all(hex(&key).as_bytes())?;
    Ok(key)
}

/// Bytes as the letters a reader copies.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The letters back as bytes, or nothing where they are not.
pub(crate) fn unhex(letters: &str) -> Option<Vec<u8>> {
    if !letters.len().is_multiple_of(2) {
        return None;
    }
    (0..letters.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(letters.get(at..at + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key as it is printed reads back as the key.
    ///
    /// Broken deliberately by printing the bytes in decimal: the window's
    /// key never matched.
    #[test]
    fn a_key_read_off_the_screen_is_the_key() {
        let key = [0_u8, 1, 15, 16, 255];
        assert_eq!(unhex(&hex(&key)), Some(key.to_vec()));
        assert_eq!(unhex("0g"), None);
        assert_eq!(unhex("abc"), None);
    }

    /// A frame is handed on whole, length and all, and a stream that has
    /// stopped hands on nothing.
    ///
    /// Broken deliberately by handing on the body without its length: the
    /// window read the first four bytes of every frame as how long it was.
    #[test]
    fn a_frame_is_handed_on_whole() {
        let frame = obelus_wire::framed(&ToChild::Detached).expect("framed");
        let mut two = frame.clone();
        two.extend_from_slice(&frame);
        let mut reading = std::io::Cursor::new(two);
        assert_eq!(framed(&mut reading), Some(frame.clone()));
        assert_eq!(framed(&mut reading), Some(frame));
        assert_eq!(framed(&mut reading), None);
    }
}
