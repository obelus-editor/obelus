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

use hmac::{KeyInit as _, Mac as _};

use crate::{Input, Update};

/// What is drawing, which decides what Obelus is at the far end: the same
/// grid either way, and a window told the things only a window draws.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Front {
    /// A window: `obg`.
    Window {
        /// The screen it opened on, in pixels.
        screen: [u32; 2],
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
    /// Something on the screen.
    Update(Update),
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
    /// Whether it holds a picture, to the question of that number.
    Holds {
        /// The number.
        number: u64,
        /// Whether.
        picture: bool,
    },
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
