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
fn a_server(name: &str) -> Server {
    let state = Scratch::new(name);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ob"))
        .args(["--server", "--listen", "127.0.0.1:0"])
        .env("XDG_STATE_HOME", &state.0)
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
                Ok(Some(Down::Update(Update::Cell { x, y, cell }))) => {
                    self.cells.insert((y, x), cell.symbol().to_string());
                }
                Ok(Some(Down::Update(Update::Cleared))) => self.cells.clear(),
                Ok(Some(Down::Finished)) => return false,
                Ok(Some(_)) => {}
                Ok(None) => return false,
                Err(error) => panic!("the connection broke waiting for {what}: {error}"),
            }
        }
        true
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
/// `Detached`): there was nothing to come back to.
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

    // Nothing is unwritten, so leaving asks nothing and Obelus ends.
    window.say(Input::Closed);
    let _ = window.until("Obelus to end", |_| false);
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
