//! A key, as the bytes a terminal sends for it.
//!
//! xterm's encoding, which is what `TERM=xterm-256color` promises a program:
//! a control letter is its caret notation, alt is an escape in front, and a
//! key with no character of its own is a sequence -- with the modifiers
//! written into it where any are held, as `1 + shift + 2·alt + 4·ctrl`.
//! The arrows have two spellings, and which one a program wants is a mode
//! it sets (`application_cursor`).
//!
//! The kitty protocol is not spoken here. A program asks for it before it
//! is used, and a terminal that never answers is one it does not use.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What a terminal would send for `key`, or `None` where it sends nothing:
/// a release, a lone modifier, a key no terminal has.
#[must_use]
pub fn bytes_of(key: &KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let modified = shift || alt || control;
    let code = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
    let escaped = |bytes: &[u8]| {
        let mut all = Vec::with_capacity(bytes.len() + 1);
        if alt {
            all.push(0x1b);
        }
        all.extend_from_slice(bytes);
        all
    };
    // A letter-ended sequence: `ESC [ X`, or `ESC O X` where the program
    // asked for the application spelling and nothing is held.
    let lettered = |letter: u8, application: bool| match modified {
        true => format!("\x1b[1;{code}{}", letter as char).into_bytes(),
        false => match application {
            true => vec![0x1b, b'O', letter],
            false => vec![0x1b, b'[', letter],
        },
    };
    // A number-ended one, `ESC [ n ~`.
    let numbered = |number: u8| match modified {
        true => format!("\x1b[{number};{code}~").into_bytes(),
        false => format!("\x1b[{number}~").into_bytes(),
    };
    let bytes = match key.code {
        KeyCode::Char(c) => {
            let mut utf8 = [0_u8; 4];
            match control {
                true => escaped(&[control_of(c)?]),
                false => escaped(c.encode_utf8(&mut utf8).as_bytes()),
            }
        }
        // A line in a shell's box is a return; with shift it is a line
        // feed, which is what tells a program that reads the two apart that
        // the reader wanted a new line rather than to send.
        KeyCode::Enter => match shift && !alt {
            true => vec![0x0a],
            false => escaped(&[0x0d]),
        },
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => escaped(&[0x09]),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => match control {
            true => escaped(&[0x08]),
            false => escaped(&[0x7f]),
        },
        KeyCode::Esc => escaped(&[0x1b]),
        KeyCode::Up => lettered(b'A', application_cursor),
        KeyCode::Down => lettered(b'B', application_cursor),
        KeyCode::Right => lettered(b'C', application_cursor),
        KeyCode::Left => lettered(b'D', application_cursor),
        KeyCode::Home => lettered(b'H', application_cursor),
        KeyCode::End => lettered(b'F', application_cursor),
        KeyCode::Insert => numbered(2),
        KeyCode::Delete => numbered(3),
        KeyCode::PageUp => numbered(5),
        KeyCode::PageDown => numbered(6),
        // The first four are letters, and always the application spelling
        // when nothing is held -- that is how xterm has always sent them.
        KeyCode::F(1) => lettered(b'P', true),
        KeyCode::F(2) => lettered(b'Q', true),
        KeyCode::F(3) => lettered(b'R', true),
        KeyCode::F(4) => lettered(b'S', true),
        KeyCode::F(n @ 5..=12) => numbered([15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)]),
        _ => return None,
    };
    Some(bytes)
}

/// What control and a character send: the character's caret notation, for
/// the characters that have one.
fn control_of(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        letter @ 'a'..='z' => letter as u8 & 0x1f,
        '@' | ' ' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::bytes_of;

    fn sent(code: KeyCode, modifiers: KeyModifiers) -> Option<Vec<u8>> {
        bytes_of(&KeyEvent::new(code, modifiers), false)
    }

    /// The keys a shell's line editing is made of, as a shell reads them.
    #[test]
    fn a_control_letter_is_its_caret_notation() {
        assert_eq!(
            sent(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(vec![0x03])
        );
        assert_eq!(
            sent(KeyCode::Char('w'), KeyModifiers::CONTROL),
            Some(vec![0x17])
        );
        assert_eq!(
            sent(
                KeyCode::Char('C'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            Some(vec![0x03])
        );
        assert_eq!(
            sent(KeyCode::Char('b'), KeyModifiers::ALT),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(
            sent(KeyCode::Char('é'), KeyModifiers::NONE),
            Some("é".as_bytes().to_vec())
        );
        assert_eq!(sent(KeyCode::Esc, KeyModifiers::NONE), Some(vec![0x1b]));
        assert_eq!(
            sent(KeyCode::Backspace, KeyModifiers::NONE),
            Some(vec![0x7f])
        );
    }

    /// An arrow is spelt the way the program asked, and the modifiers go
    /// into the sequence rather than in front of it.
    ///
    /// Broken deliberately by ignoring the mode: a program in application
    /// mode -- `less`, every full-screen editor -- reads `ESC [ A` as an
    /// escape and three letters.
    #[test]
    fn an_arrow_is_spelt_the_way_the_program_asked() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(bytes_of(&up, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(bytes_of(&up, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            sent(KeyCode::Left, KeyModifiers::CONTROL),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            sent(KeyCode::Delete, KeyModifiers::NONE),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            sent(KeyCode::PageUp, KeyModifiers::SHIFT),
            Some(b"\x1b[5;2~".to_vec())
        );
        assert_eq!(
            sent(KeyCode::F(5), KeyModifiers::NONE),
            Some(b"\x1b[15~".to_vec())
        );
    }
}
