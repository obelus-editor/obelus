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
    collections::HashMap,
    io::Write as _,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    time::Duration,
};

use anyhow::{Context as _, Result};
use obelus_app::{app, event::Event, startup};
use obelus_front::{
    Closed, Measured, Showing, Update,
    link::{Down, Front, ToChild, Up},
    replay::Screen,
};

/// How long a question for the reader's machine waits for its answer.
///
/// The loop waits with it, so it is short: on the network this is for an
/// answer takes a millisecond, and one that has not come in a second is
/// not coming.
const PATIENCE: Duration = Duration::from_secs(1);

/// What `ob --serve-one` writes to, and whether a window is there to read
/// it.
struct Out {
    writing: Mutex<std::io::BufWriter<std::io::Stdout>>,
    /// Whether a window is attached, which is when anything is written.
    attached: AtomicBool,
    /// The screen as everything said leaves it, for a window that comes.
    screen: Mutex<Screen>,
}

impl Out {
    /// Writes one thing for the window, where there is one.
    fn say(&self, down: &Down) -> Result<(), Closed> {
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut writing = self.writing.lock().map_err(|_| Closed)?;
        obelus_wire::write(&mut *writing, down).map_err(|_| Closed)
    }

    /// Sends what was written.
    fn flush(&self) {
        if let Ok(mut writing) = self.writing.lock() {
            let _ = writing.flush();
        }
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
        self.attached.store(true, Ordering::Release);
        for update in screen.again() {
            let _ = self.say(&Down::Update(update));
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
        let said = self.say(&Down::Update(update));
        drop(screen);
        said
    }

    fn wake(&self) {
        self.flush();
    }
}

/// The reader's machine, at the other end of the connection.
struct Afar {
    out: Arc<Out>,
    /// The last number a question was asked under.
    asked: AtomicU64,
    /// Who is waiting for which answer.
    waiting: Mutex<HashMap<u64, Sender<Up>>>,
}

impl std::fmt::Debug for Afar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Afar")
    }
}

impl Afar {
    /// Asks the reader's machine something, and waits a moment for it.
    fn ask(&self, question: impl FnOnce(u64) -> Down) -> Option<Up> {
        if !self.out.attached.load(Ordering::Acquire) {
            return None;
        }
        let number = self.asked.fetch_add(1, Ordering::Relaxed) + 1;
        let (answer, answered) = std::sync::mpsc::channel();
        self.waiting.lock().ok()?.insert(number, answer);
        let asked = self.out.say(&question(number));
        self.out.flush();
        let heard = asked
            .ok()
            .and_then(|()| answered.recv_timeout(PATIENCE).ok());
        if heard.is_none() {
            tracing::info!(number, "the reader's machine did not answer in time");
        }
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&number);
        }
        heard
    }

    /// Hands an answer to whoever is waiting for it.
    fn answered(&self, number: u64, up: Up) {
        let waiting = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&number));
        if let Some(waiting) = waiting {
            let _ = waiting.send(up);
        }
    }
}

impl app::desk::Desk for Afar {
    fn copy(&self, text: &str) -> std::io::Result<()> {
        self.out
            .say(&Down::Copy(text.to_string()))
            .map_err(|_| std::io::Error::other("the window has gone"))?;
        self.out.flush();
        Ok(())
    }

    fn paste(&self) -> Option<String> {
        match self.ask(Down::Paste)? {
            Up::Pasted { words, .. } => words,
            _ => None,
        }
    }

    fn picture(&self) -> Option<(String, Vec<u8>)> {
        match self.ask(Down::Picture)? {
            Up::Pictured { picture, .. } => picture,
            _ => None,
        }
    }

    fn holds_a_picture(&self) -> bool {
        matches!(
            self.ask(Down::HoldsAPicture),
            Some(Up::Holds { picture: true, .. })
        )
    }

    fn open(&self, url: &str) -> std::io::Result<()> {
        self.out
            .say(&Down::Open(url.to_string()))
            .map_err(|_| std::io::Error::other("no window to open it in"))?;
        self.out.flush();
        Ok(())
    }
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

    let out = Arc::new(Out {
        writing: Mutex::new(std::io::BufWriter::new(std::io::stdout())),
        attached: AtomicBool::new(true),
        screen: Mutex::new(Screen::default()),
    });
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
    let afar = Arc::new(Afar {
        out: Arc::clone(&out),
        asked: AtomicU64::new(0),
        waiting: Mutex::new(HashMap::new()),
    });
    app.desked_by(Arc::clone(&afar) as Arc<dyn app::desk::Desk>);
    app.drawn_afar();

    let (sender, events) = obelus_app::event::channel();
    let hearing = sender.clone();
    let heard_size = Arc::clone(&measured);
    let heard_out = Arc::clone(&out);
    std::thread::Builder::new()
        .name("obelus connection".to_string())
        .spawn(move || {
            loop {
                match obelus_wire::read::<_, ToChild>(&mut reading) {
                    Ok(Some(ToChild::Up(Up::Input(input)))) => {
                        if hearing.send(input.into_event(&heard_size)).is_err() {
                            return;
                        }
                    }
                    Ok(Some(ToChild::Up(up))) => {
                        let number = match &up {
                            Up::Pasted { number, .. }
                            | Up::Pictured { number, .. }
                            | Up::Holds { number, .. } => *number,
                            Up::Dropped { name, file } => {
                                let heard = match file {
                                    Ok(bytes) => match kept(&dropped_here(), name, bytes) {
                                        Ok(path) => Event::Dropped(path),
                                        Err(why) => Event::Amiss(why),
                                    },
                                    Err(why) => Event::Amiss(why.clone()),
                                };
                                if hearing.send(heard).is_err() {
                                    return;
                                }
                                continue;
                            }
                            Up::Input(_) => continue,
                        };
                        afar.answered(number, up);
                    }
                    // A window again, perhaps of another size: the size
                    // first, so that the screen it is told is the one that
                    // fits it after the resize is heard.
                    Ok(Some(ToChild::Attached { size })) => {
                        heard_size.resized(
                            size[0],
                            size[1],
                            u32::from(size[2]),
                            u32::from(size[3]),
                        );
                        heard_out.attach();
                        if hearing.send(Event::Resize).is_err() {
                            return;
                        }
                    }
                    Ok(Some(ToChild::Detached)) => {
                        heard_out.attached.store(false, Ordering::Release);
                    }
                    Ok(Some(ToChild::Open { .. })) => {
                        tracing::warn!("told what to open twice");
                    }
                    // `ob --server` has gone, or let this go: nobody can
                    // reach it again, so it stops the way a kill stops one.
                    Ok(Some(ToChild::Leave) | None) | Err(_) => {
                        let _ = hearing.send(Event::Stopped);
                        return;
                    }
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
    let _ = out.say(&last);
    out.flush();
    outcome.map(|()| ExitCode::SUCCESS)
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
/// machine: beside the machine's other passing things, and not in the
/// project, whose history it is no part of. Not taken away when the window
/// goes -- a conversation taken up tomorrow may still be about it -- which
/// is the machine's own clearing of its temporary files to do.
fn dropped_here() -> std::path::PathBuf {
    std::env::temp_dir().join("obelus-dropped")
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
}
