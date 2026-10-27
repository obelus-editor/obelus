//! `ob --server`, knocked on by a window that is this test: the knock, the
//! screen arriving, the window going and coming back, and the reader
//! leaving.
//!
//! A binary of its own because it is `ob`'s only one: it runs the real
//! `ob` with its state somewhere of the test's own, and drives it over a
//! real connection on loopback.

use std::{
    collections::BTreeMap,
    io::BufRead as _,
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use obelus_front::{
    Input, Update,
    link::{Answered, Challenge, Down, Front, Knock, Up},
};

/// How long anything here is waited for.
const PATIENCE: Duration = Duration::from_secs(60);

/// A server running, where it listens, and the key it printed.
struct Server {
    child: Child,
    at: String,
    key: Vec<u8>,
    _state: Scratch,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A directory of the test's own, gone when it is.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("obelus-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Starts `ob --server` on a port of loopback's choosing, and reads off
/// where it listens and its key from what it prints.
///
/// Everything the Obelus it starts for each window reads from home is the
/// test's own: the layout under the reader's settings could have the
/// palette on another key, and their agent or theme start with it. Except
/// on Windows, where the settings are where the system says and no
/// variable moves them -- there the reader's own still apply.
fn a_server(name: &str) -> Server {
    let state = Scratch::new(name);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ob"))
        .args(["--server", "--listen", "127.0.0.1:0"])
        .env("XDG_STATE_HOME", &state.0)
        .env("HOME", state.0.join("home"))
        .env("XDG_CONFIG_HOME", state.0.join("config"))
        .env("XDG_DATA_HOME", state.0.join("data"))
        .env("XDG_CACHE_HOME", state.0.join("cache"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("ob --server");
    let mut lines = std::io::BufReader::new(child.stdout.take().expect("what it prints")).lines();
    let mut at = None;
    let mut key = None;
    for line in lines.by_ref() {
        let line = line.expect("a line");
        if let Some(rest) = line.strip_prefix("Obelus is listening on ") {
            at = Some(rest.trim_end_matches('.').to_string());
        }
        if let Some(rest) = line.strip_prefix("A window gets in with the key ") {
            key = Some(rest.trim_end_matches('.').to_string());
            break;
        }
    }
    let key = key.expect("the key it printed");
    let key = (0..key.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&key[at..at + 2], 16).expect("hex"))
        .collect();
    Server {
        child,
        at: at.expect("where it listens"),
        key,
        _state: state,
    }
}

/// A window at the other end: its connection, and the screen as it has it.
struct Window {
    stream: TcpStream,
    cells: BTreeMap<(u16, u16), String>,
}

impl Window {
    /// Knocks with `key`, asking for `path` -- or for the window `again`
    /// names -- and says how it was answered.
    fn knock(server: &Server, key: &[u8], path: &str, again: Option<&str>) -> (Self, Answered) {
        let mut stream = TcpStream::connect(&server.at).expect("connected");
        stream.set_read_timeout(Some(PATIENCE)).expect("a timeout");
        obelus_wire::write(&mut stream, &obelus_wire::Hello::here("the test")).expect("hello");
        let _: obelus_wire::Hello = obelus_wire::read(&mut stream)
            .expect("read")
            .expect("hello");
        let challenge: Challenge = obelus_wire::read(&mut stream)
            .expect("read")
            .expect("a challenge");
        obelus_wire::write(
            &mut stream,
            &Knock {
                signed: obelus_front::link::sign(key, &challenge),
                front: Front::Terminal {
                    pictures: None,
                    keyboard: false,
                },
                size: [80, 24, 0, 0],
                path: Some(path.to_string()),
                window: again.map(str::to_string),
            },
        )
        .expect("the knock");
        let answered: Answered = obelus_wire::read(&mut stream)
            .expect("read")
            .expect("an answer");
        (
            Self {
                stream,
                cells: BTreeMap::new(),
            },
            answered,
        )
    }

    /// What the screen says, row by row.
    fn text(&self) -> String {
        let mut rows: BTreeMap<u16, String> = BTreeMap::new();
        for ((y, _), symbol) in &self.cells {
            rows.entry(*y).or_default().push_str(symbol);
        }
        rows.into_values().collect::<Vec<_>>().join("\n")
    }

    /// Reads what Obelus says until `until` is true of it, or it has
    /// finished -- and says which.
    fn until(&mut self, what: &str, until: impl Fn(&Self) -> bool) -> bool {
        let deadline = Instant::now() + PATIENCE;
        while !until(self) {
            assert!(Instant::now() < deadline, "gave up waiting for {what}");
            match obelus_wire::read::<_, Down>(&mut self.stream) {
                Ok(Some(Down::Updates(updates))) => {
                    for update in updates {
                        match update {
                            Update::Cell { x, y, cell } => {
                                self.cells.insert((y, x), cell.symbol().to_string());
                            }
                            Update::Cleared => self.cells.clear(),
                            _ => {}
                        }
                    }
                }
                Ok(Some(Down::Finished)) => return false,
                Ok(Some(_)) => {}
                Ok(None) => return false,
                Err(error) => panic!("the connection broke waiting for {what}: {error}"),
            }
        }
        true
    }

    /// Reads what Obelus says until it ends, and says how.
    fn end(&mut self) -> Down {
        loop {
            match obelus_wire::read::<_, Down>(&mut self.stream) {
                Ok(Some(down @ (Down::Finished | Down::Home))) => return down,
                Ok(Some(_)) => {}
                Ok(None) => panic!("the connection went before Obelus said how it ended"),
                Err(error) => panic!("the connection broke waiting for the end: {error}"),
            }
        }
    }

    /// Types `words` a key at a time.
    fn type_text(&mut self, words: &str) {
        for character in words.chars() {
            self.say(Input::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
    }

    fn say(&mut self, input: Input) {
        obelus_wire::write(&mut self.stream, &Up::Input(input)).expect("said");
    }
}

/// A file to open, in a directory of its own.
fn a_file(scratch: &Scratch) -> String {
    let file = scratch.0.join("greeting.txt");
    std::fs::write(&file, "hello from the server\n").expect("the file");
    file.display().to_string()
}

/// A window let in is drawn the file it asked for; one whose connection
/// went and comes back under its name is drawn the whole screen again, by
/// the same Obelus; and a reader who leaves ends it.
///
/// Broken deliberately three ways. A window coming back started again
/// (`taken_up` answering nothing): it was answered `again: false`. A
/// window coming back told nothing until something changed (`Screen`'s
/// replay left out of `Out::attach`): its screen stayed empty. And a
/// connection that went taking its Obelus with it (`ToChild::Leave` for
/// `Detached`): there was nothing to come back to. And by ending the
/// connection without a word when the Obelus ends (no `Down::Finished` in
/// `serve::one`): the window could not tell leaving from a network that
/// went.
#[test]
fn a_window_is_drawn_comes_back_and_leaves() {
    let server = a_server("drawn");
    let files = Scratch::new("drawn-files");
    let path = a_file(&files);

    let (mut window, answered) = Window::knock(&server, &server.key, &path, None);
    let Answered::Open {
        window: name,
        again,
    } = answered
    else {
        panic!("not let in: {answered:?}");
    };
    assert!(!again, "a new window was taken for an old one");
    assert!(
        window.until("the file", |window| window
            .text()
            .contains("hello from the server")),
        "Obelus ended before it drew the file"
    );
    drop(window);

    let (mut window, answered) = Window::knock(&server, &server.key, &path, Some(&name));
    assert_eq!(
        answered,
        Answered::Open {
            window: name,
            again: true
        },
        "the window that went was not taken up again"
    );
    assert!(
        window.until("the screen again", |window| window
            .text()
            .contains("hello from the server")),
        "the window that came back was not told the screen"
    );

    // Nothing is unwritten, so leaving asks nothing and Obelus ends -- and
    // says so, rather than the connection just going.
    window.say(Input::Closed);
    assert_eq!(window.end(), Down::Finished);
}

/// A knock signed with any other key is refused, and nothing is drawn.
///
/// Broken deliberately by letting in any knock (`signed_by` answering
/// `true`): the wrong key got in.
#[test]
fn a_knock_with_the_wrong_key_is_refused() {
    let server = a_server("refused");
    let files = Scratch::new("refused-files");
    let path = a_file(&files);
    let (_, answered) = Window::knock(&server, b"not the key", &path, None);
    assert!(
        matches!(answered, Answered::Refused(_)),
        "the wrong key got in: {answered:?}"
    );
}

/// The reader asking to come back to their own machine, from the Obelus
/// on this one, is told so -- not that Obelus there ended -- so the window
/// draws one of its own rather than going.
///
/// Broken deliberately by ending every Obelus the same way (`Down::Finished`
/// whatever `goes_to` says, in `serve::one`): the window was told it was
/// over, and went.
#[test]
fn a_reader_going_home_is_told_so() {
    let server = a_server("home");
    let files = Scratch::new("home-files");
    let path = a_file(&files);
    let (mut window, answered) = Window::knock(&server, &server.key, &path, None);
    assert!(
        matches!(answered, Answered::Open { .. }),
        "not let in: {answered:?}"
    );
    assert!(window.until("the file", |window| {
        window.text().contains("hello from the server")
    }));
    window.say(Input::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    )));
    window.type_text("disconnect-from-server");
    window.say(Input::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    )));
    assert_eq!(window.end(), Down::Home);
}

/// A file dropped on the window crosses whole and is kept on the server's
/// machine, where the Obelus there can be handed it.
///
/// Broken deliberately by keeping nothing of what arrived (`kept` left
/// out of the `Up::Dropped` arm in `serve::one`): there was no file on
/// this machine to hand on.
#[test]
fn a_dropped_file_is_kept_on_the_server_machine() {
    let server = a_server("dropped");
    let files = Scratch::new("dropped-files");
    let path = a_file(&files);
    let (mut window, answered) = Window::knock(&server, &server.key, &path, None);
    assert!(
        matches!(answered, Answered::Open { .. }),
        "not let in: {answered:?}"
    );
    assert!(window.until("the file", |window| {
        window.text().contains("hello from the server")
    }));
    let name = format!("dropped-by-the-test-{}.txt", std::process::id());
    let kept = dropped_directory().join(&name);
    let _ = std::fs::remove_file(&kept);
    obelus_wire::write(
        &mut window.stream,
        &Up::Dropped {
            name: name.clone(),
            file: Ok(b"from the window".to_vec()),
        },
    )
    .expect("dropped");
    let deadline = Instant::now() + PATIENCE;
    while std::fs::read(&kept).ok().as_deref() != Some(b"from the window".as_slice()) {
        assert!(
            Instant::now() < deadline,
            "the dropped file was never kept at {}",
            kept.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(&kept);
}

/// The key a server makes is its owner's alone: it is the whole of what
/// stands between a connection and running anything here.
///
/// Broken deliberately by writing it the ordinary way (`std::fs::write` for
/// `obelus_private::write` in `server::key`): anybody on the machine could
/// read the key and get in.
#[test]
fn the_key_is_nobody_else_s() {
    let server = a_server("private");
    let path = server._state.0.join("obelus").join("server.key");
    assert!(
        obelus_private::is_private(&path).expect("asked"),
        "anybody may read the key"
    );
}

/// A window that comes back while its old connection is still thought to
/// be here -- a laptop on another network, whose old connection went
/// without a word -- is taken up where it was, and the old connection is
/// let go of.
///
/// Broken deliberately by turning such a knock away again (`taken_up`
/// answering nothing while `reading` holds a connection): the window was
/// answered `again: false`, in a new Obelus with nothing in it.
#[test]
fn a_window_back_before_its_old_connection_went_is_taken_up() {
    let server = a_server("half-open");
    let files = Scratch::new("half-open-files");
    let path = a_file(&files);

    let (mut old, answered) = Window::knock(&server, &server.key, &path, None);
    let Answered::Open { window: name, .. } = answered else {
        panic!("not let in: {answered:?}");
    };
    assert!(old.until("the file", |window| {
        window.text().contains("hello from the server")
    }));

    // The old one is not dropped: as far as the server can tell, it is
    // still there.
    let (mut back, answered) = Window::knock(&server, &server.key, &path, Some(&name));
    assert_eq!(
        answered,
        Answered::Open {
            window: name,
            again: true
        },
        "the window was not taken up"
    );
    assert!(back.until("the screen again", |window| {
        window.text().contains("hello from the server")
    }));
    assert!(
        !old.until("the old connection to be let go of", |_| false),
        "the old connection was not let go of"
    );
}

/// What a connection says before it has shown it has the key is a few
/// bytes or nothing: a frame claiming to be a megabyte ends the
/// connection at once, rather than having the megabyte made and waited
/// for.
///
/// Broken deliberately by reading the hello as any frame is read
/// (`obelus_wire::read` for `read_at_most` in `let_in`): the server made
/// room for the megabyte and waited for it, and the connection was still
/// open when the test stopped waiting.
#[test]
fn a_long_frame_before_the_knock_ends_the_connection() {
    use std::io::{Read as _, Write as _};

    let server = a_server("long-hello");
    let mut stream = TcpStream::connect(&server.at).expect("connected");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a timeout");
    let _: obelus_wire::Hello = obelus_wire::read(&mut stream)
        .expect("read")
        .expect("hello");
    stream
        .write_all(&(1_u32 << 20).to_le_bytes())
        .expect("a length");
    let mut rest = [0_u8; 1];
    match stream.read(&mut rest) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) => {}
        other => panic!("the connection was not ended: {other:?}"),
    }
}

/// A key that is there and will not read is not replaced: the server says
/// so and does not start, and the file is left as it was -- every window
/// that kept the old key would otherwise be turned away with nothing to
/// say why.
///
/// Broken deliberately by making a new key wherever the old will not read
/// (`key` as it was, any error and any contents a reason to write one):
/// the server started with a key nobody had, over the file.
#[test]
fn a_key_that_will_not_read_is_not_replaced() {
    let state = Scratch::new("unreadable-key");
    let kept = state.0.join("obelus");
    std::fs::create_dir_all(&kept).expect("the state directory");
    std::fs::write(kept.join("server.key"), "not a key").expect("the key");
    let mut server = Command::new(env!("CARGO_BIN_EXE_ob"))
        .args(["--server", "--listen", "127.0.0.1:0"])
        .env("XDG_STATE_HOME", &state.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("ob --server");
    // Read until it ends or prints a key, so that a server that did start
    // fails here rather than being waited on for ever.
    let lines = std::io::BufReader::new(server.stdout.take().expect("what it prints")).lines();
    for line in lines {
        if line.expect("a line").contains("gets in with the key") {
            let _ = server.kill();
            let _ = server.wait();
            panic!("the server started, with a key nobody had");
        }
    }
    assert!(
        !server.wait().expect("it ended").success(),
        "the server ended as if all was well"
    );
    assert_eq!(
        std::fs::read_to_string(kept.join("server.key")).expect("still there"),
        "not a key"
    );
}

/// Where the server keeps a file dropped on a window: the reader's own
/// directory in the temporary one, by their number on unix.
fn dropped_directory() -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;

        // Who this is, read off a file this process made.
        let probe = std::env::temp_dir().join(format!("obelus-who-{}", std::process::id()));
        std::fs::write(&probe, "").expect("a file of this process's own");
        let whose = std::fs::metadata(&probe).expect("there").uid();
        let _ = std::fs::remove_file(&probe);
        std::env::temp_dir().join(format!("obelus-dropped-{whose}"))
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("obelus-dropped")
    }
}

/// What the server does is said on the terminal it was started from, as
/// well as in the log: the reader who started it has nothing else to watch.
///
/// Broken deliberately by logging to the files alone (`install(false)` in
/// `main`): the terminal said where it listened and its key, and nothing
/// after.
#[test]
fn the_server_says_what_it_does_on_its_terminal() {
    let state = Scratch::new("stderr");
    let mut server = Command::new(env!("CARGO_BIN_EXE_ob"))
        .args(["--server", "--listen", "127.0.0.1:0"])
        .env("XDG_STATE_HOME", &state.0)
        .env("HOME", state.0.join("home"))
        .env("XDG_CONFIG_HOME", state.0.join("config"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("ob --server");
    // Read on a thread of its own and waited for here: a server that says
    // nothing on its terminal is one whose stderr never ends while it runs.
    let said = server.stderr.take().expect("what it says");
    let (heard, hearing) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(said).lines() {
            let Ok(line) = line else {
                return;
            };
            if heard.send(line).is_err() {
                return;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut line = String::new();
    while !line.contains("a server for windows on other machines") {
        let left = deadline.saturating_duration_since(Instant::now());
        match hearing.recv_timeout(left) {
            Ok(next) => line = next,
            Err(_) => break,
        }
    }
    let _ = server.kill();
    let _ = server.wait();
    assert!(
        line.contains("a server for windows on other machines"),
        "nothing said on its terminal: {line:?}"
    );
}
