//! The pointer, as the bytes a terminal sends a program that asked for it.
//!
//! A program asks with a mode -- presses only, presses and releases, those
//! and a drag, or every move -- and an encoding: the original one, which
//! cannot say a column past 223; the same with the numbers as UTF-8, which
//! can; and SGR's, which says them in decimal and is what every program
//! written since asks for. What it did not ask for it is not sent: a
//! program that wanted presses and is sent releases reads them as presses.
//!
//! Only the left button and the wheel. The middle and right buttons are
//! the terminal's or the window's -- a paste, a menu -- and never reach
//! Obelus at all.

use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// What the pointer did, where a program might want to hear about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    /// The left button went down.
    Pressed,
    /// It moved with the button down.
    Dragged,
    /// The button came up.
    Released,
    /// It moved with nothing held.
    Moved,
    /// A notch of the wheel, up the screen.
    WheelUp,
    /// A notch down.
    WheelDown,
}

/// What a program in `mode` and `encoding` is sent for `what` at a cell,
/// counted from zero -- or `None` where it did not ask to hear of it.
#[must_use]
pub(crate) fn bytes_of(
    what: Mouse,
    (row, column): (u16, u16),
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    let wanted = match what {
        Mouse::Pressed | Mouse::WheelUp | Mouse::WheelDown => mode != MouseProtocolMode::None,
        Mouse::Released => matches!(
            mode,
            MouseProtocolMode::PressRelease
                | MouseProtocolMode::ButtonMotion
                | MouseProtocolMode::AnyMotion
        ),
        Mouse::Dragged => matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        ),
        Mouse::Moved => mode == MouseProtocolMode::AnyMotion,
    };
    if !wanted {
        return None;
    }
    // The button, with 32 for a move and 64 for the wheel. A release is
    // button 3 in the old encodings, which cannot say which came up, and
    // the button itself in SGR's, which says so with its last letter.
    let sgr = encoding == MouseProtocolEncoding::Sgr;
    let button: u32 = match what {
        Mouse::Pressed => 0,
        Mouse::Released if sgr => 0,
        Mouse::Released => 3,
        Mouse::Dragged => 32,
        Mouse::Moved => 32 + 3,
        Mouse::WheelUp => 64,
        Mouse::WheelDown => 65,
    };
    let (x, y) = (u32::from(column) + 1, u32::from(row) + 1);
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = match what {
                Mouse::Released => 'm',
                _ => 'M',
            };
            Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes())
        }
        MouseProtocolEncoding::Default => {
            // One byte a number, offset by 32: nothing past 223 can be
            // said at all, and a cell that cannot be said is not sent as
            // some other cell.
            let say = |number: u32| u8::try_from(number + 32).ok();
            Some(vec![0x1b, b'[', b'M', say(button)?, say(x)?, say(y)?])
        }
        MouseProtocolEncoding::Utf8 => {
            let mut bytes = b"\x1b[M".to_vec();
            for number in [button, x, y] {
                let character = char::from_u32(number + 32)?;
                let mut utf8 = [0_u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut utf8).as_bytes());
            }
            Some(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use vt100::{MouseProtocolEncoding, MouseProtocolMode};

    use super::{Mouse, bytes_of};

    /// A press and a release in SGR's encoding, which every program written
    /// since asks for: one-based, decimal, and the release says itself.
    #[test]
    fn a_click_is_said_the_way_the_program_asked() {
        let sgr = |what| {
            bytes_of(
                what,
                (4, 9),
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr,
            )
        };
        assert_eq!(sgr(Mouse::Pressed), Some(b"\x1b[<0;10;5M".to_vec()));
        assert_eq!(sgr(Mouse::Released), Some(b"\x1b[<0;10;5m".to_vec()));
        assert_eq!(sgr(Mouse::WheelDown), Some(b"\x1b[<65;10;5M".to_vec()));
        // And the original encoding, with its offset and its release that
        // names no button.
        assert_eq!(
            bytes_of(
                Mouse::Released,
                (0, 0),
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Default,
            ),
            Some(vec![0x1b, b'[', b'M', 35, 33, 33])
        );
    }

    /// What a program did not ask for it is not sent.
    ///
    /// Broken deliberately by sending a drag in every mode: a program that
    /// asked for presses alone reads every cell of a drag as a click.
    #[test]
    fn a_program_is_sent_only_what_it_asked_for() {
        let at = (0, 0);
        let encoding = MouseProtocolEncoding::Sgr;
        assert_eq!(
            bytes_of(Mouse::Pressed, at, MouseProtocolMode::None, encoding),
            None
        );
        assert_eq!(
            bytes_of(
                Mouse::Dragged,
                at,
                MouseProtocolMode::PressRelease,
                encoding
            ),
            None
        );
        assert!(
            bytes_of(
                Mouse::Dragged,
                at,
                MouseProtocolMode::ButtonMotion,
                encoding
            )
            .is_some()
        );
        assert_eq!(
            bytes_of(Mouse::Moved, at, MouseProtocolMode::ButtonMotion, encoding),
            None
        );
        assert!(bytes_of(Mouse::Moved, at, MouseProtocolMode::AnyMotion, encoding).is_some());
    }
}
