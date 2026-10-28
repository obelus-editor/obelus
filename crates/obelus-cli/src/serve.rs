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

/// How long the picture on the reader's clipboard is waited for, which
/// crosses whole: a screenshot is megabytes, and a second is not long
/// enough for one on a network that is merely ordinary. Waited for only
/// when a picture is what is being pasted, the way a paste here waits for
/// the clipboard to hand one over.
const PICTURE_PATIENCE: Duration = Duration::from_secs(10);

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
struct Out {
    writing: Mutex<Said>,
    /// Whether a window is attached, which is when anything is written.
    attached: AtomicBool,
    /// The screen as everything said leaves it, for a window that comes.
    screen: Mutex<Screen>,
}

/// Where what is said goes, and what is kept for the next message.
struct Said {
    writer: std::io::BufWriter<Box<dyn std::io::Write + Send>>,
    kept: Vec<Update>,
}

impl Said {
    /// Writes what is kept, as one message, where anything is.
    fn out(&mut self) -> Result<(), Closed> {
        if self.kept.is_empty() {
            return Ok(());
        }
        let updates = std::mem::take(&mut self.kept);
        obelus_wire::write(&mut self.writer, &Down::Updates(updates)).map_err(|_| Closed)
    }
}

impl Out {
    /// What writes to `writer`, with a window already reading.
    fn new(writer: Box<dyn std::io::Write + Send>) -> Self {
        Self {
            writing: Mutex::new(Said {
                writer: std::io::BufWriter::new(writer),
                kept: Vec::new(),
            }),
            attached: AtomicBool::new(true),
            screen: Mutex::new(Screen::default()),
        }
    }

    /// Keeps something for the screen, to go with the rest of its frame.
    fn keep(&self, update: Update) -> Result<(), Closed> {
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut said = self.writing.lock().map_err(|_| Closed)?;
        said.kept.push(update);
        if said.kept.len() >= KEPT_MOST {
            said.out()?;
        }
        Ok(())
    }

    /// Keeps several, under the one lock.
    fn keep_all(&self, updates: Vec<Update>) -> Result<(), Closed> {
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut said = self.writing.lock().map_err(|_| Closed)?;
        said.kept.extend(updates);
        if said.kept.len() >= KEPT_MOST {
            said.out()?;
        }
        Ok(())
    }

    /// Writes one thing for the window, where there is one -- after what is
    /// kept for the screen, so that the two arrive in the order said.
    fn say(&self, down: &Down) -> Result<(), Closed> {
        if !self.attached.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut said = self.writing.lock().map_err(|_| Closed)?;
        said.out()?;
        obelus_wire::write(&mut said.writer, down).map_err(|_| Closed)
    }

    /// Sends what is kept, and what was written.
    fn flush(&self) {
        if let Ok(mut said) = self.writing.lock() {
            let _ = said.out();
            let _ = said.writer.flush();
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
        if let Ok(mut said) = self.writing.lock() {
            // Kept for a window that has gone since: the screen says all
            // of it again.
            said.kept.clear();
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
    /// Asks the reader's machine something, and waits `patience` for it.
    fn ask(&self, question: impl FnOnce(u64) -> Down, patience: Duration) -> Option<Up> {
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
            .and_then(|()| answered.recv_timeout(patience).ok());
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
        match self.ask(Down::Paste, PATIENCE)? {
            Up::Pasted { words, .. } => words,
            _ => None,
        }
    }

    fn picture(&self) -> Option<(String, Vec<u8>)> {
        match self.ask(Down::Picture, PICTURE_PATIENCE)? {
            Up::Pictured { picture, .. } => picture,
            _ => None,
        }
    }

    fn holds_a_picture(&self) -> bool {
        matches!(
            self.ask(Down::HoldsAPicture, PATIENCE),
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

    let out = Arc::new(Out::new(Box::new(std::io::stdout())));
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
                                    Ok(bytes) => match dropped_in(&std::env::temp_dir())
                                        .and_then(|directory| kept(&directory, name, bytes))
                                    {
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

    /// Cells handed over together are kept for a window that comes back as
    /// well as said in the frame's one message.
    ///
    /// Broken deliberately by leaving the screen out of `Out::show_all`
    /// (no `screen.saw`): the frame went, and a window coming back was told
    /// a screen with none of it.
    #[test]
    fn cells_handed_over_together_are_kept_and_said() {
        let written = Written::default();
        let out = Out::new(Box::new(written.clone()));
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
        let out = Out::new(Box::new(written.clone()));
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
        assert!(
            written.messages().is_empty(),
            "said before the frame was done"
        );
        out.wake();
        out.say(&Down::Open("u".to_string())).expect("said");

        let messages = written.messages();
        assert_eq!(messages.len(), 2, "not one message for the frame");
        let Down::Updates(updates) = &messages[0] else {
            panic!("the frame was not first: {:?}", messages[0]);
        };
        assert_eq!(updates.len(), 1001);
        assert_eq!(updates.last(), Some(&Update::Frame));
        assert_eq!(messages[1], Down::Open("u".to_string()));
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
