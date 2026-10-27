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
//!
//! **A knock with a window's name is that window.** Even while the
//! connection it had is still thought to be here: a laptop that changed
//! networks knocks again long before anything says its old connection
//! went, and turning it away would start an Obelus with nothing in it and
//! leave the reader's where nobody can reach it. The old connection is let
//! go of instead.

use std::{
    collections::HashMap,
    io::Write as _,
    net::{SocketAddr, TcpListener, TcpStream},
    process::{ChildStdin, ExitCode, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_front::link::{Answered, Challenge, Knock, ToChild, Up};

/// How long a window that went is waited for before its Obelus is
/// stopped -- what VS Code waits for a window of its own.
const GRACE: Duration = Duration::from_secs(3 * 60 * 60);

/// Where a server listens when it is not told.
pub(crate) const PORT: u16 = 7787;

/// How long a connection is given to say who it is before it is let go of.
const KNOCKING: Duration = Duration::from_secs(10);

/// The most a connection may say in one frame before it has shown it has
/// the key: a hello and a knock are a few dozen bytes and a path.
const KNOCK_MOST: usize = 64 << 10;

/// How many connections may be knocking at once. Anybody on the network
/// can knock, and each one is a thread until it has been answered.
const KNOCKERS: usize = 32;

/// How long a frame waits for a window that has stopped reading before its
/// connection is let go of -- the Obelus behind it waiting all that time,
/// since nothing reads what it says while a frame is stuck.
const STALLED: Duration = Duration::from_secs(10);

/// The connection a window's frames go to.
#[derive(Clone)]
struct Reader {
    stream: Arc<TcpStream>,
}

/// One window's Obelus, and whoever is reading it now.
struct Window {
    /// What is written to it.
    input: Arc<Mutex<ChildStdin>>,
    /// The connection its frames go to, while one is there.
    reading: Arc<Mutex<Option<Reader>>>,
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
    reading: Arc<Mutex<Option<Reader>>>,
    /// Which coming of a connection to it this is.
    coming: u64,
}

/// Every window's Obelus, by the window's name.
type Windows = Arc<Mutex<HashMap<String, Window>>>;

/// One of the [`KNOCKERS`], given back however its knock ends.
struct Knocking(Arc<AtomicUsize>);

impl Knocking {
    /// A place among the knockers, where there is one.
    fn taken(knockers: &Arc<AtomicUsize>) -> Option<Self> {
        knockers
            .try_update(Ordering::AcqRel, Ordering::Acquire, |now| {
                (now < KNOCKERS).then_some(now + 1)
            })
            .ok()
            .map(|_| Self(Arc::clone(knockers)))
    }
}

impl Drop for Knocking {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

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
    let knockers = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "a connection that did not arrive");
                // Out of descriptors, most often, and then every accept
                // fails at once with the connection still waiting: a
                // moment before the next, or this is a loop doing nothing
                // else. Not a poll -- the next accept is still what wakes
                // it.
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        // Refused before a thread is made for it, which is the thing
        // anybody could otherwise have made as many of as they liked.
        let Some(knocking) = Knocking::taken(&knockers) else {
            tracing::warn!("too many connections knocking at once, so one was let go of");
            continue;
        };
        let (key, windows) = (key.clone(), Arc::clone(&windows));
        let started = std::thread::Builder::new()
            .name("obelus server connection".to_string())
            .spawn(move || {
                let from = stream.peer_addr().ok();
                if let Err(error) = let_in(stream, &key, &windows, knocking) {
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
fn let_in(mut stream: TcpStream, key: &[u8], windows: &Windows, knocking: Knocking) -> Result<()> {
    stream.set_nodelay(true)?;
    // Until it has shown it has the key, a connection is anybody's: it is
    // given a moment and a few bytes, and no more.
    stream.set_read_timeout(Some(KNOCKING))?;
    let ours = obelus_wire::Hello::here(env!("OBELUS_BUILD"));
    obelus_wire::write(&mut stream, &ours)?;
    let theirs: obelus_wire::Hello = obelus_wire::read_at_most(&mut stream, KNOCK_MOST)?
        .context("the window went before it said hello")?;
    if let Some(why) = ours.refused(&theirs) {
        anyhow::bail!(why);
    }
    let mut number = [0_u8; 32];
    getrandom::fill(&mut number).map_err(|error| anyhow::anyhow!("{error}"))?;
    let challenge = Challenge(number);
    obelus_wire::write(&mut stream, &challenge)?;
    let knock: Knock = obelus_wire::read_at_most(&mut stream, KNOCK_MOST)?
        .context("the window went before it knocked")?;
    if !obelus_front::link::signed_by(key, &challenge, &knock.signed) {
        obelus_wire::write(
            &mut stream,
            &Answered::Refused("That is not this server's key".to_string()),
        )?;
        anyhow::bail!("a knock not signed with the key");
    }
    drop(knocking);
    // A window now: it may be quiet for as long as the reader is, and what
    // says it went without a word is the kernel's knocking, not a clock of
    // this one's.
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(Some(STALLED))?;
    obelus_front::link::kept_alive(&stream)?;

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
    if let Err(error) = obelus_wire::write(
        &mut stream,
        &Answered::Open {
            window: name.clone(),
            again,
        },
    ) {
        if again {
            went(&name, coming, windows);
        } else {
            // Nobody was told its name, so nobody can come back for it.
            tell(&input, &ToChild::Leave);
        }
        return Err(error.into());
    }
    let stream = Arc::new(stream);
    // Only now, after the answer, so that nothing of the Obelus's reaches
    // the window ahead of it.
    if let Ok(mut reading) = reading.lock() {
        *reading = Some(Reader {
            stream: Arc::clone(&stream),
        });
    }
    if again {
        tell(&input, &ToChild::Attached { size: knock.size });
    }
    tracing::info!(name, again, "a window came in");

    let heard = loop {
        match obelus_wire::read::<_, Up>(&mut &*stream) {
            Ok(Some(up)) => tell(&input, &ToChild::Up(up)),
            Ok(None) => break "the window went".to_string(),
            Err(error) => break error.to_string(),
        }
    };
    went(&name, coming, windows);
    anyhow::bail!(heard)
}

/// The Obelus of a window that knocks with the name of one it has had.
///
/// Even where the connection that window had is still thought to be here:
/// a knock with the key and the name *is* that window, come back over a
/// connection the old one did not know had gone -- a laptop on another
/// network, say. The old one is let go of rather than this one turned away,
/// which would start an Obelus with nothing in it and leave the reader's
/// running where nobody can reach it.
fn taken_up(knock: &Knock, windows: &Windows) -> Option<Entered> {
    let name = knock.window.clone()?;
    let mut windows = windows.lock().ok()?;
    let window = windows.get_mut(&name)?;
    if let Some(old) = window.reading.lock().ok()?.take() {
        // Its read ends with this, and what it says when it ends is about a
        // coming that is no longer the window's -- so it says nothing.
        let _ = old.stream.shutdown(std::net::Shutdown::Both);
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
    let reading: Arc<Mutex<Option<Reader>>> = Arc::default();
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
                // Written with nothing held: a window that has stopped
                // reading holds up this one write, and not a knock from
                // that window coming back or from any other.
                let Some(reader) = carried.lock().ok().and_then(|reading| reading.clone()) else {
                    continue;
                };
                if (&*reader.stream).write_all(&frame).is_err() {
                    // Gone, or stalled past `STALLED`: let go of, which ends
                    // its read, and that is what says it went.
                    let _ = reader.stream.shutdown(std::net::Shutdown::Both);
                }
            }
            // Its Obelus has ended, and with it the window and its name.
            if let Ok(mut gone) = gone.lock() {
                gone.remove(&whose);
            }
            if let Ok(mut reading) = carried.lock()
                && let Some(reader) = reading.take()
            {
                let _ = reader.stream.shutdown(std::net::Shutdown::Both);
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
    {
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
        // Said with the windows held, so that a window coming back -- which
        // has to hold them to come -- is told it is attached after this and
        // never before.
        tell(&window.input, &ToChild::Detached);
    }
    tracing::info!(name, "a window went, and is waited for");
    let (name, windows) = (name.to_string(), Arc::clone(windows));
    // One moment, waited for once on the runtime the waiting is done on:
    // nothing else could say the grace is up, and a window going and coming
    // back a hundred times is a hundred sleeping tasks rather than threads.
    obelus_runtime::handle().spawn(async move {
        tokio::time::sleep(GRACE).await;
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
    // Made only where there is none. A key that is there and will not read
    // -- another account's, or written over by hand -- is not replaced:
    // every window that kept the old one would be turned away with nothing
    // to say why, so the server says so and does not start.
    match std::fs::read_to_string(&path) {
        Ok(kept) => {
            return unhex(kept.trim())
                .filter(|key| !key.is_empty())
                .with_context(|| {
                    format!(
                        "{} does not hold a key; take it away and a new one is made",
                        path.display()
                    )
                });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("reading the key in {}", path.display()));
        }
    }
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).map_err(|error| anyhow::anyhow!("{error}"))?;
    obelus_private::write(&path, hex(&key).as_bytes())?;
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
