//! A file dropped on Obelus, which arrives as the words for its path.
//!
//! A terminal has no event for a drop: what it does with a file dragged onto
//! it is type the file's path, as a paste. So a paste is the only place a
//! drop can be heard, and what is heard is a terminal's idea of how a path
//! is written -- escaped with backslashes by one, quoted by the next, and
//! several of them on a line with spaces between. The rules here are the
//! ones Claude Code uses for the same question, so a reader who drags a
//! screenshot onto either finds the same thing happens.
//!
//! A window does hear the drop, and hands over the path rather than words.
//! It is written out here the way a terminal would have typed it
//! ([`as_typed`]) and goes the rest of the way as a paste, so the two halves
//! of Obelus are one rule rather than two that agree today.
//!
//! **What is taken for a picture is a file that is one.** The name is only
//! the first question: a piece is a picture when its name ends like one,
//! and then only if the file is there and starts the way that kind of
//! picture starts. Anything short of that is the words that were pasted --
//! which is what a paste of `see /tmp/a.png please` has to stay, and what a
//! path to a file that has gone stays too.

use std::path::{Path, PathBuf};

/// One piece of a paste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Words, as they were pasted.
    Words(String),
    /// What may be a picture: its name ends like one.
    Picture {
        /// The piece as it was pasted, which is what goes in instead if the
        /// file turns out not to be one.
        said: String,
        /// The path it names, with the quoting and the escapes taken off.
        path: PathBuf,
    },
}

/// What a paste is made of, piece by piece.
///
/// Split at a space that a path starts after -- `/`, or a drive's `C:\` --
/// and at every line. A space anywhere else is part of the piece, so an
/// unescaped `/home/me/two words.png` stays one path, and a sentence with a
/// path in it stays one sentence whose end is not a picture's name. What
/// this does not split is two quoted paths on a line, where the space comes
/// before a quote: Claude Code does not either.
#[must_use]
pub fn pieces(pasted: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for line in pasted.lines() {
        for said in split_before_paths(line) {
            if said.trim().is_empty() {
                continue;
            }
            let path = unescaped(unquoted(said.trim()));
            pieces.push(match is_absolute(&path) && is_a_picture_name(&path) {
                true => Piece::Picture {
                    said: said.to_string(),
                    path: PathBuf::from(path),
                },
                false => Piece::Words(said.to_string()),
            });
        }
    }
    pieces
}

/// The words a terminal would have typed for a file dropped on it.
///
/// Escaped with backslashes, as Ghostty and the macOS terminals write it --
/// except a Windows path, which is quoted where it has a space in it,
/// because a backslash there is the path itself. Read back by [`pieces`].
#[must_use]
pub fn as_typed(path: &Path) -> String {
    let path = path.to_string_lossy();
    if is_a_windows_path(&path) {
        return match path.contains(' ') {
            true => format!("\"{path}\""),
            false => path.into_owned(),
        };
    }
    let mut typed = String::with_capacity(path.len());
    for character in path.chars() {
        if matches!(character, '\\' | ' ' | '\'' | '"') {
            typed.push('\\');
        }
        typed.push(character);
    }
    typed
}

/// The picture at `path`, and what shape it is in.
///
/// Read from what the file starts with, never from its name: a `.png` that
/// is a JPEG is sent as the JPEG it is, and one that is not a picture at
/// all is not sent. `None` for a file that is not there, cannot be read,
/// or is empty.
#[must_use]
pub fn picture_at(path: &Path) -> Option<(String, Vec<u8>)> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "no picture at a path that was pasted");
            return None;
        }
    };
    let Some(mime) = shape_of(&bytes) else {
        tracing::warn!(path = %path.display(), "named like a picture and is not one");
        return None;
    };
    Some((mime.to_string(), bytes))
}

/// Which of the shapes a picture is sent in these bytes are.
fn shape_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// A line cut at each space a path starts after.
fn split_before_paths(line: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut from = 0;
    for (at, character) in line.char_indices() {
        if character == ' ' && starts_a_path(&line[at + 1..]) {
            pieces.push(&line[from..at]);
            from = at + 1;
        }
    }
    pieces.push(&line[from..]);
    pieces
}

/// Whether a path starts here: `/`, or a drive's `C:\`.
fn starts_a_path(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    bytes.first() == Some(&b'/')
        || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":\\")
}

/// A path from the root, which is the only kind a terminal types for a
/// file dropped on it. Anything else would be read from wherever this
/// process happens to be standing, which is not the project and may be
/// the reader's home: a bare `logo.png` pasted is a word.
fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || is_a_windows_path(path)
}

/// A Windows path, whose backslashes are its own: `C:\`, or a share's `\\`.
fn is_a_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("\\\\")
        || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":\\")
}

/// One pair of quotes taken off, where they are round the whole of it.
fn unquoted(said: &str) -> &str {
    for quote in ['"', '\''] {
        if said.len() >= 2 && said.starts_with(quote) && said.ends_with(quote) {
            return &said[1..said.len() - 1];
        }
    }
    said
}

/// The backslash escapes taken out: each takes the character after it as
/// it is, so `\\` is one backslash.
fn unescaped(said: &str) -> String {
    if is_a_windows_path(said) {
        return said.to_string();
    }
    let mut out = String::with_capacity(said.len());
    let mut characters = said.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(escaped) => out.push(escaped),
                // A backslash at the end escapes nothing, and stays.
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    out
}

/// Whether a name ends the way a picture's does.
fn is_a_picture_name(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"]
        .iter()
        .any(|ending| lower.ends_with(ending))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(said: &str, path: &str) -> Piece {
        Piece::Picture {
            said: said.to_string(),
            path: PathBuf::from(path),
        }
    }

    fn words(said: &str) -> Piece {
        Piece::Words(said.to_string())
    }

    /// Each way a terminal writes a path it was given is read as that path.
    ///
    /// Deliberate break: leaving the escapes in reads Ghostty's
    /// `two\ words.png` as a file named with a backslash, which nobody has.
    #[test]
    fn a_path_is_read_however_the_terminal_wrote_it() {
        assert_eq!(pieces("/tmp/a.png"), [picture("/tmp/a.png", "/tmp/a.png")]);
        assert_eq!(
            pieces("/tmp/two\\ words.png "),
            [picture("/tmp/two\\ words.png ", "/tmp/two words.png")]
        );
        assert_eq!(
            pieces("'/tmp/two words.png'"),
            [picture("'/tmp/two words.png'", "/tmp/two words.png")]
        );
        assert_eq!(
            pieces("\"C:\\Users\\me\\a b.PNG\""),
            [picture(
                "\"C:\\Users\\me\\a b.PNG\"",
                "C:\\Users\\me\\a b.PNG"
            )]
        );
        // And left alone where it was not escaped at all.
        assert_eq!(
            pieces("/tmp/two words.jpeg"),
            [picture("/tmp/two words.jpeg", "/tmp/two words.jpeg")]
        );
    }

    /// Several dropped at once are several pieces, split where the next
    /// path starts and at every line.
    ///
    /// Deliberate break: splitting at every space cuts the unescaped
    /// `two words.png` in half and finds no picture in either half.
    #[test]
    fn several_paths_are_several_pieces() {
        assert_eq!(
            pieces("/tmp/a.png /tmp/two words.gif C:\\b.webp\n/tmp/c.txt"),
            [
                picture("/tmp/a.png", "/tmp/a.png"),
                picture("/tmp/two words.gif", "/tmp/two words.gif"),
                picture("C:\\b.webp", "C:\\b.webp"),
                words("/tmp/c.txt"),
            ]
        );
    }

    /// A sentence with a path in it is words, all of it.
    ///
    /// Deliberate break: looking for a picture's name anywhere in a piece
    /// rather than at its end makes the second half a picture.
    #[test]
    fn a_sentence_naming_a_picture_is_not_one() {
        assert_eq!(
            pieces("look at /tmp/a.png please"),
            [words("look at"), words("/tmp/a.png please")]
        );
    }

    /// Nor is a name with no root, which a terminal never types for a drop.
    ///
    /// Deliberate break: taking any name ending like a picture's reads
    /// `logo.png` from whatever directory Obelus was started in.
    #[test]
    fn a_name_with_no_root_is_a_word() {
        assert_eq!(pieces("logo.png"), [words("logo.png")]);
        assert_eq!(pieces("look at logo.png"), [words("look at logo.png")]);
    }

    /// What a window writes for a dropped file is read back as that file.
    ///
    /// Deliberate break: not escaping the backslash in `as_typed` turns
    /// `back\slash` into `backslash` on the way back.
    #[test]
    fn what_a_window_types_is_read_back() {
        for path in [
            "/tmp/plain.png",
            "/tmp/two words.png",
            "/tmp/it's \"quoted\".png",
            "/tmp/back\\slash.png",
            "C:\\Users\\me\\two words.png",
            "C:\\Users\\me\\one.png",
        ] {
            let typed = format!("{} ", as_typed(Path::new(path)));
            assert_eq!(
                pieces(&typed),
                [picture(&typed, path)],
                "{path} came back as something else"
            );
        }
    }

    /// A picture is what the file starts with, not what it is called.
    ///
    /// Deliberate break: taking the shape from the name sends the JPEG
    /// below as a PNG, and the text file as a picture.
    #[test]
    fn a_picture_is_what_its_bytes_say() {
        let scratch = std::env::temp_dir().join(format!("obelus-dropped-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let jpeg = scratch.join("really.png");
        std::fs::write(&jpeg, [0xff, 0xd8, 0xff, 0xe0, 0, 0x10]).expect("a jpeg");
        let text = scratch.join("words.png");
        std::fs::write(&text, "not a picture").expect("a text file");
        let empty = scratch.join("empty.png");
        std::fs::write(&empty, "").expect("an empty file");

        assert_eq!(
            picture_at(&jpeg).map(|(mime, _)| mime).as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(picture_at(&text), None);
        assert_eq!(picture_at(&empty), None);
        assert_eq!(picture_at(&scratch.join("gone.png")), None);
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
