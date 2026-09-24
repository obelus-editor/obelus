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
}
