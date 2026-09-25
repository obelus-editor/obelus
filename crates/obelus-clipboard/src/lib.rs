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
    // Waited for, because until it has taken the text it has not got it --
    // and because a program left running is a program still holding a pipe.
    drop(child.stdin.take());
    child.wait()?;
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
