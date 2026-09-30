//! Putting text on the reader's clipboard, and getting it back.
//!
//! Through whatever the machine actually has, which is the design helix and
//! nvim both arrived at and which neither reaches by linking a clipboard
//! library. An outside program -- `wl-copy`, `xclip`, `pbcopy`, `tmux` --
//! when there is one, and OSC 52 when there is not.
//!
//! OSC 52 hands the text to whatever is drawing Obelus, and that program owns
//! it from then on. Two things follow, and they are why it is the fallback
//! rather than nothing:
//!
//! - **It survives Obelus exiting.** On Wayland and X11 the clipboard has no
//!   server; the content belongs to a live client, and a library that offered
//!   it from inside this process would lose it the moment the process ended.
//!   Copy, quit, paste is the most ordinary thing a reader does with a copy.
//! - **It works over ssh.** The terminal is on the reader's own machine while
//!   Obelus is not, and a display-server connection has nothing to connect to
//!   at this end.
//!
//! It cannot be *read*, though. Many terminals refuse -- a program that could
//! ask what is on your clipboard is a program that can read your passwords --
//! and helix does not even try: its own OSC 52 provider answers a read with
//! "not supported". So Obelus keeps whatever it last copied or cut, and hands
//! that back when nothing else can answer. Text from outside arrives instead
//! by the terminal's own paste, which is bracketed and comes in as an event.
//!
//! A copy does not survive the *terminal* exiting either: the terminal is
//! then the client that has gone. Making a copy outlive everything is a
//! clipboard manager's job, not an editor's.

pub mod links;
mod native;

use std::{
    io,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Whatever Obelus last copied or cut.
///
/// What a paste falls back to. Kept whether or not the provider took the
/// copy, because the case it is for is exactly the one where the provider
/// cannot be asked afterwards.
static KEPT: Mutex<Option<String>> = Mutex::new(None);

/// Which way this machine talks to its clipboard, worked out once.
static PROVIDER: OnceLock<Provider> = OnceLock::new();

/// Obelus's own hold on the clipboard, where a front end can take one.
///
/// Installed once, by the front end, at startup. A `OnceLock` because there
/// is one clipboard and one front end, and a second owner would be a second
/// answer to every question below.
static OWNER: OnceLock<Box<dyn Owner>> = OnceLock::new();

/// A provider a test asked for, which stands in front of the detected one.
///
/// Its own thing rather than seeding `PROVIDER`, because a `OnceLock` can be
/// set once and a suite has more than one test in it.
static ASKED: Mutex<Option<Provider>> = Mutex::new(None);

/// The ways there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    /// `wl-copy` and `wl-paste`.
    Wayland,
    /// `xclip`.
    XClip,
    /// `xsel`.
    XSel,
    /// `pbcopy` and `pbpaste`.
    Pasteboard,
    /// `tmux load-buffer` and `save-buffer`, which reach the outer terminal.
    Tmux,
    /// `win32yank.exe`.
    Win32Yank,
    /// The escape sequence, which writes and cannot read.
    Osc52,
    /// Nothing outside Obelus at all.
    ///
    /// What a test gets, so that running the suite does not reach into the
    /// clipboard of whoever is running it -- and what the copy and paste
    /// commands are really made of, since every other provider falls back
    /// to this the moment it cannot answer.
    Kept,
}

impl Provider {
    /// The two commands it is, or `None` for the one that is not a command.
    const fn commands(
        self,
    ) -> Option<(
        &'static str,
        &'static [&'static str],
        &'static str,
        &'static [&'static str],
    )> {
        match self {
            Self::Wayland => Some((
                "wl-copy",
                &["--type", "text/plain"],
                "wl-paste",
                &["--no-newline"],
            )),
            Self::XClip => Some((
                "xclip",
                &["-selection", "clipboard"],
                "xclip",
                &["-selection", "clipboard", "-o"],
            )),
            Self::XSel => Some(("xsel", &["--nodetach", "-i", "-b"], "xsel", &["-o", "-b"])),
            Self::Pasteboard => Some(("pbcopy", &[], "pbpaste", &[])),
            Self::Tmux => Some((
                "tmux",
                &["load-buffer", "-w", "-"],
                "tmux",
                &["save-buffer", "-"],
            )),
            Self::Win32Yank => Some((
                "win32yank.exe",
                &["-i", "--crlf"],
                "win32yank.exe",
                &["-o", "--lf"],
            )),
            Self::Osc52 | Self::Kept => None,
        }
    }

    /// The command that says what the clipboard is offering, where there
    /// is one.
    ///
    /// Only the two that can be asked. A clipboard holds one thing in
    /// several shapes at once -- a file manager's copy is a path, a name
    /// and a picture -- and which shapes are there is a question
    /// `pbpaste`, `xsel` and a terminal cannot answer at all: they hand
    /// over text and that is the whole of their vocabulary. The two
    /// platforms whose clipboard is a system service are not asked this
    /// way at all; see [`native`].
    const fn listing(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Self::Wayland => Some(("wl-paste", &["--list-types"])),
            // `TARGETS` is X11's own name for the list, and it comes back
            // as one name per line like every other selection.
            Self::XClip => Some(("xclip", &["-selection", "clipboard", "-t", "TARGETS", "-o"])),
            _ => None,
        }
    }

    /// And the one that reads a particular shape.
    fn reading(self, mime: &str) -> Option<(&'static str, Vec<String>)> {
        match self {
            Self::Wayland => Some((
                "wl-paste",
                vec![
                    "--no-newline".to_string(),
                    "--type".to_string(),
                    mime.to_string(),
                ],
            )),
            Self::XClip => Some((
                "xclip",
                vec![
                    "-selection".to_string(),
                    "clipboard".to_string(),
                    "-t".to_string(),
                    mime.to_string(),
                    "-o".to_string(),
                ],
            )),
            _ => None,
        }
    }
}

/// Obelus holding the clipboard itself, rather than handing it to a program.
///
/// Every provider above gives the text to somebody else, and that somebody
/// offers it in one shape. A clipboard holds one thing in several shapes at
/// once -- a file manager's copy is a path, a name and a picture -- and
/// offering several is not something the programs can be asked for:
/// `wl-copy -t` and `xclip -t` each name one, per invocation, and the
/// second call replaces the first. So a copy that is words for everybody
/// *and* Obelus's own shape for another Obelus has to come from a client
/// that owns the selection. Which needs a display connection, and that is
/// the window: a terminal has none, and over ssh there is nothing at this
/// end to connect to.
///
/// What it costs is why it is not simply better. The selection belongs to a
/// live client, so what Obelus owns is gone the moment Obelus is -- and
/// copy, quit, paste is the most ordinary thing a reader does with a copy.
/// [`hand_over`] is the other half, and the reason the programs stay.
///
/// Only writing. Reading somebody else's clipboard is what the programs are
/// already good at, in every shape, and an owner that read as well would be
/// a second answer to a question that has one.
pub trait Owner: Send + Sync {
    /// Takes the selection, offering each shape by the name it goes by.
    ///
    /// `false` where it could not, which puts the copy back on the
    /// provider -- a compositor with no manager to bind, a window that has
    /// never been touched and so has no serial to ask with.
    fn offer(&self, shapes: Vec<(String, Vec<u8>)>) -> bool;

    /// One shape of what Obelus is offering, where Obelus is what owns the
    /// selection.
    ///
    /// `None` where somebody else owns it, which is the ordinary case: then
    /// the question is theirs to answer and a program asks it.
    fn holding(&self, mime: &str) -> Option<Vec<u8>>;

    /// And their names, for the same reason.
    fn holds(&self) -> Vec<String>;
}

/// The three names plain text goes by on a selection.
///
/// `text/plain;charset=utf-8` is what everything modern asks for;
/// `UTF8_STRING` is X11's own name for the same bytes, which Wayland
/// clients inherited by being ports of X11 ones; `text/plain` is the one a
/// program that has not thought about encodings asks for, and it gets the
/// same bytes because there is nothing else to give it.
const WORDS: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

/// Obelus is this front end's to hold, from now on.
///
/// Called by the window once it has a display connection. Nothing calls it
/// in a terminal, where there is no such connection to have.
pub fn owned_by(owner: Box<dyn Owner>) {
    if OWNER.set(owner).is_err() {
        tracing::warn!("the clipboard already has an owner");
    }
}

/// Whoever that is, if anybody.
fn owner() -> Option<&'static dyn Owner> {
    OWNER.get().map(AsRef::as_ref)
}

/// Gives what Obelus is holding to a program that will keep it, on the way
/// out.
///
/// The selection belongs to a live client, so a copy made in Obelus dies
/// with Obelus. The last thing the window does with the clipboard is
/// therefore hand the words to `wl-copy`, which forks and goes on serving
/// them to whatever pastes next.
///
/// The words only. The shapes Obelus offers for its own sake mean nothing
/// to the program taking over, and nothing that asks for them afterwards
/// would know what to do with the answer.
///
/// Not waited for, unlike an ordinary copy: this is the last moment of the
/// process, and one of the programs (`xsel`, with `--nodetach`) stays in
/// the foreground for as long as it holds the selection. Waiting for that
/// one is a window that will not close.
pub fn hand_over() {
    let Some(owner) = owner() else { return };
    let Some(words) = owner.holding(WORDS[0]) else {
        return;
    };
    let Ok(words) = String::from_utf8(words) else {
        return;
    };
    // The escape sequence is not a hand-over: it is addressed to a
    // terminal, and what owns a selection is a window.
    if provider().commands().is_none() {
        tracing::info!("nothing to leave the clipboard with");
        return;
    }
    if let Err(error) = to_a_program(&words, Waiting::No) {
        tracing::warn!(%error, "handing the clipboard over");
    }
}

/// What this machine has, asked once and remembered.
///
/// The order is helix's, which is nvim's: a multiplexer first, because it is
/// what is between Obelus and the terminal; then the display server the
/// environment says is running; then the escape sequence, which needs
/// nothing and can be wrong about nothing except whether the terminal was
/// listening.
pub fn provider() -> Provider {
    if let Some(asked) = ASKED.lock().ok().and_then(|asked| *asked) {
        return asked;
    }
    *PROVIDER.get_or_init(|| {
        let have = |program: &str| {
            Command::new(program)
                .arg("--help")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        };
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());

        let found = if set("TMUX") && have("tmux") {
            Provider::Tmux
        } else if set("WAYLAND_DISPLAY") && have("wl-copy") && have("wl-paste") {
            Provider::Wayland
        } else if set("DISPLAY") && have("xclip") {
            Provider::XClip
        } else if set("DISPLAY") && have("xsel") {
            Provider::XSel
        } else if have("pbcopy") && have("pbpaste") {
            Provider::Pasteboard
        } else if have("win32yank.exe") {
            Provider::Win32Yank
        } else {
            Provider::Osc52
        };
        tracing::info!(?found, "the clipboard");
        found
    })
}

/// What the clipboard is offering, by name.
///
/// The names are mime types, on all three platforms: the two whose
/// clipboard is a system service have vocabularies of their own -- a UTI on
/// macOS, a numbered format on Windows -- and each says its answer in these
/// words rather than making every caller learn three.
///
/// Empty where nothing is on it, and empty where the machine cannot be
/// asked: `pbpaste`, `xsel` and a terminal have no way to say. A caller
/// reads that as "text, or nothing", which is what those clipboards are.
#[must_use]
pub fn types() -> Vec<String> {
    if let Some(owner) = owner() {
        let holds = owner.holds();
        if !holds.is_empty() {
            return holds;
        }
    }
    if let Some(names) = native::types() {
        return names;
    }
    let Some((program, arguments)) = provider().listing() else {
        return Vec::new();
    };
    let outcome = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match outcome {
        Ok(ran) if ran.status.success() => offered(&String::from_utf8_lossy(&ran.stdout)),
        Ok(ran) => {
            // An empty clipboard is a failure for both of them, and is not
            // worth a word: it is the ordinary state of a machine that has
            // just started.
            tracing::debug!(program, status = ?ran.status, "nothing to list");
            Vec::new()
        }
        Err(error) => {
            tracing::warn!(program, %error, "asking what is on the clipboard");
            Vec::new()
        }
    }
}

/// One of those shapes, as the bytes it is.
///
/// Bytes rather than text: what this is for is the shapes that are not
/// text -- a list of files, a picture -- and a caller that wants a string
/// has [`paste`].
#[must_use]
pub fn paste_as(mime: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = owner().and_then(|owner| owner.holding(mime)) {
        return Some(bytes);
    }
    if let Some(bytes) = native::paste_as(mime) {
        return Some(bytes);
    }
    let (program, arguments) = provider().reading(mime)?;
    let outcome = Command::new(program)
        .args(&arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !outcome.status.success() {
        tracing::debug!(program, mime, status = ?outcome.status, "nothing of that shape");
        return None;
    }
    Some(outcome.stdout)
}

/// The name a list of files goes by.
///
/// The one shape Obelus asks for besides text, and the reason any of this
/// exists: a reader copies a file in their file manager and pastes it where
/// Obelus is asking for one. It is what X11 and Wayland call it, and what
/// the other two platforms' answers are translated into.
pub const FILES: &str = "text/uri-list";

/// The names in a listing, one per line.
fn offered(said: &str) -> Vec<String> {
    let mut names: Vec<String> = said
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        // X11 answers with its own questions among the shapes -- `TARGETS`,
        // `TIMESTAMP`, `MULTIPLE` -- which are about the selection rather
        // than about what is on it.
        .filter(|name| !matches!(*name, "TARGETS" | "TIMESTAMP" | "MULTIPLE" | "SAVE_TARGETS"))
        .map(str::to_string)
        .collect();
    names.dedup();
    names
}

/// The shapes a picture arrives in, best first.
///
/// Best meaning what survives being sent on: PNG is lossless and is what a
/// screenshot tool puts down, and the others are here because a reader who
/// copied a photograph out of a browser has whatever that browser offered.
/// No `image/svg+xml`, which is a document rather than a picture -- an agent
/// handed one would be handed markup it can already read as text.
const PICTURES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

/// A picture on the clipboard, in the first shape that is offered.
///
/// `None` where there is none, which is the ordinary case and not worth a
/// word anywhere: a reader pressing paste with words on the clipboard wants
/// the words.
///
/// Asked as two questions rather than one, because the shapes a clipboard
/// holds are cheap to list and a picture is not cheap to copy: a megabyte
/// comes over a pipe, and asking for one that is not there would be a
/// megabyte of nothing on every paste of ordinary text.
#[must_use]
pub fn picture() -> Option<(String, Vec<u8>)> {
    let offered = types();
    let wanted = PICTURES
        .iter()
        .find(|mime| offered.iter().any(|held| held == *mime))?;
    let bytes = paste_as(wanted)?;
    // A shape that is offered and comes back empty is not a picture. The
    // clipboard can say it holds something it can no longer produce -- the
    // program that owned it has gone -- and an empty payload sent to an
    // agent is a question about nothing.
    if bytes.is_empty() {
        tracing::warn!(
            mime = wanted,
            "the clipboard offered a picture and gave nothing"
        );
        return None;
    }
    Some(((*wanted).to_string(), bytes))
}

/// What a `text/uri-list` names, as paths on this machine.
///
/// Only `file:` URIs, because only those are paths. A list that names
/// something on a web server names nothing this machine can open, and a
/// path that is not one is worse than none: it is a path a reader watches
/// Obelus fail to open.
#[must_use]
pub fn files(list: &[u8]) -> Vec<std::path::PathBuf> {
    String::from_utf8_lossy(list)
        .lines()
        .map(str::trim)
        // The format's own comments, which is how it carries anything that
        // is not a URI.
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let rest = line.strip_prefix("file://")?;
            // `file://host/path`, and the host is empty for this machine --
            // which is the only one whose files are here.
            if !rest.starts_with('/') {
                return None;
            }
            Some(std::path::PathBuf::from(decoded(rest)))
        })
        .collect()
}

/// A URI's percent escapes, put back.
///
/// By hand rather than with a crate: what arrives here is a path somebody
/// else wrote, the rule is three characters long, and the alternative is a
/// dependency for one function.
fn decoded(said: &str) -> String {
    let mut out = Vec::with_capacity(said.len());
    let mut bytes = said.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        let digits: Vec<u8> = bytes.clone().take(2).collect();
        let digits = String::from_utf8_lossy(&digits).to_string();
        match u8::from_str_radix(&digits, 16) {
            Ok(decoded) => {
                out.push(decoded);
                bytes.next();
                bytes.next();
            }
            // Not an escape after all, and a percent sign is a character a
            // file may be named with.
            Err(_) => out.push(byte),
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Puts text back, from wherever it can be got.
///
/// The provider first, and what Obelus kept when the provider cannot read --
/// which is OSC 52 always, and any of the others when the program is not
/// there any more or says nothing.
#[must_use]
pub fn paste() -> Option<String> {
    let kept = || KEPT.lock().ok().and_then(|kept| kept.clone());
    if let Some(words) = owner().and_then(|owner| owner.holding(WORDS[0]))
        && let Ok(words) = String::from_utf8(words)
    {
        return Some(words);
    }
    let Some((_, _, program, arguments)) = provider().commands() else {
        return kept();
    };
    let outcome = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match outcome {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            // An empty clipboard is not an answer worth having over one
            // Obelus is sure of.
            Ok(text) if !text.is_empty() => Some(text),
            _ => kept(),
        },
        Ok(_) | Err(_) => kept(),
    }
}

/// Uses a provider of the caller's choosing, for a test.
///
/// A suite that ran against whatever the machine has would reach into the
/// clipboard of whoever ran it, and would pass or fail by what happened to
/// be on it.
pub fn use_provider_for_test(provider: Provider) {
    if let Ok(mut asked) = ASKED.lock() {
        *asked = Some(provider);
    }
    if let Ok(mut kept) = KEPT.lock() {
        *kept = None;
    }
}

/// Remembers what Obelus put on the clipboard.
fn keep(text: &str) {
    if let Ok(mut kept) = KEPT.lock() {
        *kept = Some(text.to_string());
    }
}

/// Hands `text` to the terminal for its clipboard.
///
/// `c` is the selection to set: the clipboard proper rather than the primary
/// selection, which is the one a paste reaches for.
///
/// Written straight to stdout and flushed. Nothing else may write there --
/// stdout is the drawing surface, and a stray line lands in the middle of a
/// frame -- but an escape sequence *is* what that surface is for, and this
/// one has to arrive before the next frame rather than whenever a buffer
/// happens to fill.
pub fn copy(text: &str) -> io::Result<()> {
    // Kept first and whatever happens: the case this is for is the one where
    // the provider cannot be asked for it back.
    keep(text);
    let shapes: Vec<(String, Vec<u8>)> = WORDS
        .iter()
        .map(|name| ((*name).to_string(), text.as_bytes().to_vec()))
        .collect();
    // The platforms whose clipboard is a service take it directly. There
    // is nothing to own there: the content is the system's the moment it
    // is handed over, and it outlives every process without anybody
    // holding it -- which is why those two need no owner and no
    // hand-over.
    if native::copy(&shapes) {
        return Ok(());
    }
    if owner().is_some_and(|owner| owner.offer(shapes)) {
        return Ok(());
    }
    to_a_program(text, Waiting::Yes)
}

/// Whether a copy waits for the program it handed the text to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    /// An ordinary copy: until it has taken the text it has not got it, and
    /// a program left running is a program still holding a pipe.
    Yes,
    /// A hand-over on the way out; see [`hand_over`].
    No,
}

/// The half of a copy that is somebody else's program, or the terminal.
fn to_a_program(text: &str, waiting: Waiting) -> io::Result<()> {
    let Some((program, arguments, _, _)) = provider().commands() else {
        return write_to(&mut io::stdout().lock(), text);
    };
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        io::Write::write_all(stdin, text.as_bytes())?;
    }
    drop(child.stdin.take());
    if waiting == Waiting::Yes {
        child.wait()?;
    }
    Ok(())
}

/// The same, to somewhere a test can read.
///
/// Split out because the sequence itself is the whole of what this module
/// does, and a wrong one is invisible: the terminal ignores it, so nothing
/// is copied and there is nothing to report.
fn write_to<W: io::Write>(out: &mut W, text: &str) -> io::Result<()> {
    // Inside tmux this passes through to the outer terminal on the default
    // `set-clipboard external`; `off` drops it, which is the one setting
    // that makes this quietly do nothing.
    let sequence = format!("\u{1b}]52;c;{}\u{7}", STANDARD.encode(text));
    out.write_all(sequence.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence, byte for byte. A wrong one is the worst kind of bug
    /// here: the terminal ignores it, so nothing is copied and nothing is
    /// reported.
    /// A picture is taken in the first shape that is offered, and a shape
    /// that produces nothing is not one.
    ///
    /// The order matters because a browser offers several: PNG is lossless
    /// and is what the reader would have got by saving the image, so taking
    /// whichever the clipboard happens to list first would send an agent a
    /// re-encoded JPEG of a screenshot.
    ///
    /// One owner holding a changing hand, because `OWNER` is a `OnceLock`
    /// and the second `owned_by` in a process is a warning and nothing
    /// else. Which also means this is the only test in this binary that may
    /// own the clipboard -- a second one would be answered by this one's
    /// owner, and would pass or fail for reasons of its own.
    ///
    /// Broken deliberately two ways. Answering with the first of `types()`
    /// that starts with `image/` takes the JPEG, because that is the order
    /// this hand lists them in. And dropping the empty check hands back a
    /// picture of no bytes, which an agent is then asked to look at.
    #[test]
    fn a_picture_is_taken_in_the_best_shape_that_has_bytes() {
        #[derive(Default)]
        struct Hand(std::sync::Mutex<Vec<(String, Vec<u8>)>>);
        impl Hand {
            fn holding(&self, shapes: &[(&str, &[u8])]) {
                *self.0.lock().expect("the hand") = shapes
                    .iter()
                    .map(|(name, bytes)| ((*name).to_string(), (*bytes).to_vec()))
                    .collect();
            }
        }
        impl Owner for &'static Hand {
            fn offer(&self, _shapes: Vec<(String, Vec<u8>)>) -> bool {
                true
            }
            fn holding(&self, mime: &str) -> Option<Vec<u8>> {
                let held = self.0.lock().expect("the hand");
                held.iter()
                    .find(|(name, _)| name == mime)
                    .map(|(_, bytes)| bytes.clone())
            }
            fn holds(&self) -> Vec<String> {
                let held = self.0.lock().expect("the hand");
                held.iter().map(|(name, _)| name.clone()).collect()
            }
        }
        static HAND: std::sync::OnceLock<Hand> = std::sync::OnceLock::new();
        let hand = HAND.get_or_init(Hand::default);
        hand.holding(&[("text/plain", b"hello")]);
        owned_by(Box::new(hand));

        // Words alone are not a picture.
        assert!(picture().is_none(), "text was taken for a picture");

        // Listed worst first, which is what makes the order a claim.
        hand.holding(&[
            ("image/jpeg", &[0xff, 0xd8, 0xff]),
            ("image/png", &[0x89, b'P', b'N', b'G']),
        ]);
        let (mime, bytes) = picture().expect("a picture");
        assert_eq!(mime, "image/png", "the lossless shape was not preferred");
        assert_eq!(bytes, vec![0x89, b'P', b'N', b'G']);

        // Offered and empty, which a clipboard whose owner has gone does.
        hand.holding(&[("image/png", &[])]);
        assert!(
            picture().is_none(),
            "a shape that produced no bytes was taken for a picture"
        );
    }

    #[test]
    fn the_sequence_is_osc_52_around_base64() {
        let mut written = Vec::new();
        write_to(&mut written, "fn main() {}").expect("writing to a vector");
        assert_eq!(
            String::from_utf8(written).expect("utf-8"),
            "\u{1b}]52;c;Zm4gbWFpbigpIHt9\u{7}"
        );

        // The introducer, the selection, and the terminator, each of which
        // the terminal is matching on.
        let mut written = Vec::new();
        write_to(&mut written, "").expect("writing to a vector");
        assert_eq!(written, b"\x1b]52;c;\x07");

        // The vectors from RFC 4648, which is what says the padding is
        // right -- and padding is exactly what a hand-rolled encoder gets
        // wrong.
        assert_eq!(STANDARD.encode(""), "");
        assert_eq!(STANDARD.encode("f"), "Zg==");
        assert_eq!(STANDARD.encode("fo"), "Zm8=");
        assert_eq!(STANDARD.encode("foo"), "Zm9v");
        assert_eq!(STANDARD.encode("foob"), "Zm9vYg==");
        assert_eq!(STANDARD.encode("fooba"), "Zm9vYmE=");
        assert_eq!(STANDARD.encode("foobar"), "Zm9vYmFy");

        // And text that is not ASCII, which is the reason the payload is
        // encoded at all: the sequence is terminated by a control character,
        // so the text inside it cannot contain arbitrary bytes.
        assert_eq!(STANDARD.encode("你好"), "5L2g5aW9");
    }

    /// A listing is one name per line, and X11's questions about the
    /// selection are not shapes of what is on it.
    ///
    /// Deliberate break: keeping `TARGETS` makes a caller asking "are
    /// there files here" read a list whose first entry is the word for
    /// "what is here", which is not a shape anything can be read as.
    #[test]
    fn a_listing_is_the_shapes_and_nothing_else() {
        let said = "TARGETS\nTIMESTAMP\ntext/uri-list\nUTF8_STRING\n\n  text/plain  \n";
        assert_eq!(
            offered(said),
            ["text/uri-list", "UTF8_STRING", "text/plain"]
        );
        assert!(offered("").is_empty());
    }

    /// A `text/uri-list` is paths, and only the ones that are paths here.
    ///
    /// Deliberate break: taking every line makes a copy from a browser --
    /// which puts `https://` on the clipboard in this shape -- into a path
    /// Obelus opens and fails to find.
    #[test]
    fn a_uri_list_is_the_files_it_names() {
        // The third line is a path and not a URI, which this format does
        // not carry: taking it would mean taking the path out of every
        // `https://` line as well, since by then the two look alike.
        let list = b"# a comment the format allows\r\nfile:///home/sunli/note.md\r\nhttps://example.com/x\r\n/home/sunli/bare.txt\r\nfile:///tmp/two.rs\r\n";
        let files = files(list);
        assert_eq!(
            files,
            [
                std::path::PathBuf::from("/home/sunli/note.md"),
                std::path::PathBuf::from("/tmp/two.rs")
            ]
        );
    }

    /// And the escapes in one are put back, because a file may be named
    /// with a space.
    ///
    /// Deliberate break: handing the line over as it stands gives a path
    /// with `%20` in it, which is a file nobody has.
    #[test]
    fn a_path_with_a_space_in_it_survives_the_uri() {
        assert_eq!(
            files(b"file:///home/sunli/two%20words.txt"),
            [std::path::PathBuf::from("/home/sunli/two words.txt")]
        );
        // A percent that is not an escape is a character a file may be
        // named with, and stays one.
        assert_eq!(
            files(b"file:///tmp/100%.txt"),
            [std::path::PathBuf::from("/tmp/100%.txt")]
        );
        // Every byte of a name that is not ASCII arrives escaped, one
        // escape per byte.
        assert_eq!(
            files(b"file:///tmp/%E4%B8%AD%E6%96%87.rs"),
            [std::path::PathBuf::from("/tmp/\u{4e2d}\u{6587}.rs")]
        );
    }
}
