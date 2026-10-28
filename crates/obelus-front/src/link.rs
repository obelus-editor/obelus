//! What a front end and Obelus on another machine say to each other: who
//! is knocking, and then the screen one way and the reader the other.
//!
//! **A connection is a knock first.** After both ends have said which words
//! they speak (`obelus_wire::Hello`), Obelus sends a number it chose
//! ([`Challenge`]) and the front end answers with that number signed by the
//! key Obelus printed when it started ([`Knock`]) -- so the key itself never
//! crosses -- and says what it is and what it wants opened. Obelus answers
//! with the name of the window it opened, or why not ([`Answered`]). A
//! connection is somebody able to run anything on that machine, which is
//! why nothing at all is said before the knock is answered.
//!
//! **What is said after it is not hidden.** The connection is not
//! encrypted: what the reader types and what Obelus draws cross as they
//! are, which is a connection for a network the reader trusts.
//!
//! **A window is named so that it can be found again.** The name Obelus
//! answers with goes back in the next knock from the same window, and a
//! window whose connection went is taken up where it was rather than
//! started again.
//!
//! **A connection that went without a word is found out by the kernel.**
//! A network changed under a laptop, or a NAT that forgot it, ends a
//! connection with nothing said: the end still waiting to read would wait
//! for ever, and the end still writing for the minutes a retransmission
//! takes. Both ends ask the kernel to knock now and then while nothing
//! else crosses ([`kept_alive`]), which is the one thing that can tell
//! them -- and is not a poll, because nothing here asks.

use std::time::Duration;

use hmac::{KeyInit as _, Mac as _};

use crate::{Input, Update};

/// How long a connection may say nothing before the kernel knocks.
const QUIET: Duration = Duration::from_secs(10);

/// How long after one knock the kernel knocks again.
const AGAIN: Duration = Duration::from_secs(5);

/// How many knocks go unanswered before the connection is taken to have
/// gone: so a connection that went is found out in about half a minute.
const UNANSWERED: u32 = 3;

/// Has the kernel find out when `stream` went without a word.
///
/// # Errors
///
/// Where the kernel would not be asked.
pub fn kept_alive(stream: &std::net::TcpStream) -> std::io::Result<()> {
    let knocking = socket2::TcpKeepalive::new()
        .with_time(QUIET)
        .with_interval(AGAIN)
        .with_retries(UNANSWERED);
    socket2::SockRef::from(stream).set_tcp_keepalive(&knocking)
}

/// What is drawing, which decides what Obelus is at the far end: the same
/// grid either way, and a window told the things only a window draws.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Front {
    /// A window: `obg`.
    Window {
        /// The screen it opened on, in pixels.
        screen: [u32; 2],
        /// Whether command is control there and drawn so, which is a fact
        /// about the reader's machine and not this one -- see
        /// `obelus_keymap::draw_control_as_command`.
        command_is_control: bool,
    },
    /// A terminal: `ob`.
    Terminal {
        /// What it can draw a picture with, and how large a cell is in
        /// pixels -- or nothing, where it draws none.
        pictures: Option<(Pictures, [u16; 2])>,
        /// Whether it speaks the keyboard protocol, which is what makes
        /// `ctrl+enter` arrive as itself.
        keyboard: bool,
    },
}

/// How a terminal draws a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Pictures {
    /// kitty's graphics protocol.
    Kitty,
    /// iTerm2's inline images.
    Iterm2,
    /// Sixels.
    Sixel,
}

/// The number a front end is asked to sign.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Challenge(pub [u8; 32]);

/// A front end's knock: the challenge signed, and what it wants.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Knock {
    /// The challenge, signed with the key.
    pub signed: [u8; 32],
    /// What is drawing.
    pub front: Front,
    /// How many columns and rows it has room for, and how many pixels that
    /// is where it knows.
    pub size: [u16; 4],
    /// What to open, as `ob path` would be told, or nothing for wherever
    /// Obelus starts on that machine.
    pub path: Option<String>,
    /// The name of the window this was, where it was one and its
    /// connection went.
    pub window: Option<String>,
}

/// Obelus's answer to a knock.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Answered {
    /// Come in: the window's name, and whether it is one taken up again.
    Open {
        /// The name, for the next knock.
        window: String,
        /// Whether this is a window that was already there.
        again: bool,
    },
    /// Not, and why, in words for the reader.
    Refused(String),
}

/// What Obelus says, once the knock is answered.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Down {
    /// What changed on the screen, a frame's worth at a time: every cell a
    /// frame changed and whatever was said about the window with it, as one
    /// message rather than one each -- a page scrolled is thousands of
    /// cells, and a message for each was thousands of reads and writes and
    /// packets for one frame. A run of alike cells crosses once
    /// (`wire::updates`).
    Updates(#[serde(with = "crate::wire::updates")] Vec<Update>),
    /// Words for the reader's clipboard.
    Copy(String),
    /// What is on the reader's clipboard, asked under this number.
    Paste(u64),
    /// The picture on the reader's clipboard, asked under this number.
    Picture(u64),
    /// Whether the reader's clipboard holds a picture, asked under this
    /// number.
    HoldsAPicture(u64),
    /// A link, to open where the reader is.
    Open(String),
    /// Obelus has ended: the reader left, and the window goes with it.
    Finished,
    /// Obelus has ended because the reader asked to come back to their own
    /// machine: the window stays, and draws that.
    Home,
}

/// What a front end says, once the knock is answered.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Up {
    /// What the reader did.
    Input(Input),
    /// What was on the clipboard, to the question of that number.
    Pasted {
        /// The number.
        number: u64,
        /// The words, or nothing.
        words: Option<String>,
    },
    /// The picture on the clipboard, to the question of that number.
    Pictured {
        /// The number.
        number: u64,
        /// What shape it is in and the picture, or nothing.
        picture: Option<(String, Vec<u8>)>,
    },
    /// A file dropped on the window, sent whole: there is no path on the
    /// window's machine that means anything on Obelus's.
    Dropped {
        /// What it is called.
        name: String,
        /// What it holds, or why it is not sent.
        file: Result<Vec<u8>, String>,
    },
    /// Whether it holds a picture, to the question of that number.
    Holds {
        /// The number.
        number: u64,
        /// Whether.
        picture: bool,
    },
}

/// What `ob --server` tells the Obelus it started for a window, on that
/// Obelus's standard input.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ToChild {
    /// What to be, first and once: what is drawing, the size it has, and
    /// what to open.
    Open {
        /// What is drawing.
        front: Front,
        /// Columns, rows, and pixels across and down.
        size: [u16; 4],
        /// What to open.
        path: Option<String>,
    },
    /// What the window said.
    Up(Up),
    /// A window has come in under this one's name, this size: tell it the
    /// whole screen.
    Attached {
        /// Columns, rows, and pixels across and down.
        size: [u16; 4],
    },
    /// The window's connection went, and nobody is reading.
    Detached,
    /// Nobody came back in time: stop.
    Leave,
}

/// A challenge signed with `key`, which is what a knock carries.
#[must_use]
pub fn sign(key: &[u8], challenge: &Challenge) -> [u8; 32] {
    let Ok(mut mac) = hmac::Hmac::<sha2::Sha256>::new_from_slice(key) else {
        // Any length of key will do for an HMAC, so this is never.
        return [0; 32];
    };
    mac.update(&challenge.0);
    mac.finalize().into_bytes().into()
}

/// Whether `signed` is `challenge` signed with `key` -- compared in a time
/// that does not depend on how much of it was right.
#[must_use]
pub fn signed_by(key: &[u8], challenge: &Challenge, signed: &[u8; 32]) -> bool {
    let Ok(mut mac) = hmac::Hmac::<sha2::Sha256>::new_from_slice(key) else {
        return false;
    };
    mac.update(&challenge.0);
    mac.verify_slice(signed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connection kept alive has the kernel knocking on it.
    ///
    /// Broken deliberately by asking the kernel nothing in `kept_alive`
    /// (an `Ok(())` and no call): the connection had no knocking on it, and
    /// one that went without a word would have been waited on for ever.
    #[test]
    fn a_connection_kept_alive_is_knocked_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let stream =
            std::net::TcpStream::connect(listener.local_addr().expect("where")).expect("connected");
        let asked = socket2::SockRef::from(&stream);
        assert!(!asked.keepalive().expect("asked"), "knocked on already");
        kept_alive(&stream).expect("kept alive");
        assert!(asked.keepalive().expect("asked"), "not knocked on");
    }

    /// A challenge signed with the key is taken, and one signed with any
    /// other key, or a different challenge, is not.
    ///
    /// Broken deliberately by taking any knock (`signed_by` answering
    /// `true`): a front end with the wrong key was let in.
    #[test]
    fn only_a_knock_signed_with_the_key_is_taken() {
        let key = b"the key the server printed";
        let challenge = Challenge([7; 32]);
        let signed = sign(key, &challenge);
        assert!(signed_by(key, &challenge, &signed));
        assert!(!signed_by(b"another key", &challenge, &signed));
        assert!(!signed_by(key, &Challenge([8; 32]), &signed));
    }

    /// What a knock and its answer say crosses as itself.
    ///
    /// Broken deliberately by leaving the window's name out of the bytes
    /// (`serde(skip)` on `Knock::window`): a window that came back was
    /// started again.
    #[test]
    fn a_knock_and_its_answer_cross_as_themselves() {
        let knock = Knock {
            signed: [1; 32],
            front: Front::Terminal {
                pictures: Some((Pictures::Kitty, [9, 18])),
                keyboard: true,
            },
            size: [80, 24, 0, 0],
            path: Some("~/work/obelus".to_string()),
            window: Some("w1".to_string()),
        };
        let frame = obelus_wire::framed(&knock).expect("framed");
        let back: Knock = obelus_wire::unframed(&frame[4..]).expect("read");
        assert_eq!(back, knock);
        for down in [
            Down::Copy("words".to_string()),
            Down::Paste(3),
            Down::Open("https://example.com".to_string()),
            Down::Finished,
        ] {
            let frame = obelus_wire::framed(&down).expect("framed");
            let back: Down = obelus_wire::unframed(&frame[4..]).expect("read");
            assert_eq!(back, down);
        }
        let up = Up::Pasted {
            number: 3,
            words: Some("from here".to_string()),
        };
        let frame = obelus_wire::framed(&up).expect("framed");
        let back: Up = obelus_wire::unframed(&frame[4..]).expect("read");
        assert_eq!(back, up);
    }
}
