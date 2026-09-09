//! Putting text on the reader's clipboard.
//!
//! Through the terminal, with OSC 52: the escape sequence hands the text to
//! whatever is drawing obelus, and that program owns it from then on. Two
//! things follow, and they are the reasons for choosing this over a library
//! that talks to the display server:
//!
//! - **It survives obelus exiting.** On Wayland and X11 the clipboard has no
//!   server; the content belongs to a live client, and a library that offers it
//!   from inside this process loses it the moment the process ends. Copy, quit,
//!   paste is the most ordinary thing a reader does with a copy.
//! - **It works over ssh.** The terminal is on the reader's own machine while
//!   obelus is not, and a display-server connection has nothing to connect to
//!   at this end.
//!
//! What it costs: a terminal that does not implement the sequence copies
//! nothing and says nothing, because there is no reply to wait for. Every
//! terminal obelus is likely to be read in does implement it, and the
//! alternative -- asking the display server -- fails outright in the two
//! cases above rather than silently in an unlikely one.
//!
//! It does not survive the *terminal* exiting either, for the same reason:
//! the terminal is then the client that has gone. Making a copy outlive
//! everything is a clipboard manager's job, not an editor's.

use std::io;

use base64::{Engine as _, engine::general_purpose::STANDARD};

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
    write_to(&mut io::stdout().lock(), text)
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
