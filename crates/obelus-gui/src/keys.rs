//! What a press is called once it is inside Obelus.
//!
//! Every part of Obelus that reads a key reads a `crossterm::event::KeyEvent`
//! -- the keymap, the boxes, the pickers, the conversation -- and none of
//! them is going to learn that there are two front ends. So the window's
//! presses are translated into that type here, and this file is the only
//! place in `obg` that knows what a key is called on either side.
//!
//! The same trick is not available for what a *window* can say and a
//! terminal cannot: closing one is the reader asking to leave, so it is an
//! event (`Event::Closed`) that goes the same way the key that leaves goes,
//! question about unwritten files included -- and the window stays open
//! until the application says it is done.
//!
//! Which is also where the window earns its keep. A terminal cannot tell
//! `ctrl+i` from `Tab`, `ctrl+m` from `Enter` or `ctrl+[` from `Escape` --
//! they are one byte each -- and most terminals send nothing at all for
//! `ctrl+shift+X`. Here they arrive as themselves, because a window is told
//! which key went down rather than which bytes to read.
//!
//! And where a platform names a modifier differently from the rest. On macOS
//! command is what a reader presses a program with -- `cmd+s`, `cmd+w`,
//! `cmd+q` -- and control is the text fields' own, for the line motions
//! Obelus does not have. So there command is reported as control, and the
//! one table answers both; control still works, which is the hand a reader
//! brings over from `ob` in a terminal, where command never arrives because
//! the terminal keeps it. Both held together is still a key Obelus does not
//! understand, as `ctrl+super+q` is everywhere else (`modifiers_of`).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use winit::keyboard::{Key, KeyCode as Physical, ModifiersState, NamedKey, PhysicalKey};

/// Whether command is reported as control: on macOS, and nowhere else,
/// where super is the window manager's and never reaches a window.
///
/// One constant for the two halves -- what a key arrives as and what the
/// foot calls it (`obelus_keymap::draw_control_as_command`, from `main`) --
/// because a foot that says `Cmd` over a key that does nothing is worse than
/// either alone.
pub(crate) const COMMAND_IS_CONTROL: bool = cfg!(target_os = "macos");

/// What the window system is asked for with command, rather than Obelus.
///
/// The three a Mac window answers whatever program it belongs to. The menu
/// winit puts up answered the first two, and its `Quit` ended the process
/// without asking about unwritten files, so `obg` has no menu and answers
/// these itself -- and `cmd+q` is the key that leaves, question included.
///
/// Which mends the key and not every way of quitting. Quit from the Dock,
/// from the switcher and at logging out still end the process unasked:
/// they go to the application rather than to a key, and winit gives no way
/// to answer them (`applicationShouldTerminate:` is not one it implements).
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// `cmd+h`: put Obelus away.
    Hide,
    /// `alt+cmd+h`: put everything else away.
    HideOthers,
    /// `cmd+m`: this window into the Dock.
    Minimize,
}

/// Which of [`Asked`] a press is, if any.
///
/// Asked before [`pressed`], which would make every one of them control.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn asked_of_the_window(
    logical: &Key,
    physical: PhysicalKey,
    modifiers: ModifiersState,
) -> Option<Asked> {
    if !modifiers.super_key() || modifiers.control_key() || modifiers.shift_key() {
        return None;
    }
    let alt = modifiers.alt_key();
    // Asked as the control it is about to become, so a layout that is not
    // Latin -- where `cmd+h` arrives as `р` -- is taken back to the key the
    // way a binding is (`code_of`).
    let mut bits = KeyModifiers::CONTROL;
    bits.set(KeyModifiers::ALT, alt);
    match (code_of(logical, physical, bits)?, alt) {
        (KeyCode::Char('h'), false) => Some(Asked::Hide),
        (KeyCode::Char('h'), true) => Some(Asked::HideOthers),
        (KeyCode::Char('m'), false) => Some(Asked::Minimize),
        _ => None,
    }
}

/// Whether a press is reported as control because command was held --
/// which a terminal in Obelus has to know (`Event::Command`).
pub(crate) fn by_command(modifiers: ModifiersState) -> bool {
    command_stands_for_control(modifiers, COMMAND_IS_CONTROL)
}

/// Whether command is held and stands for control: not with control as
/// well, which is a chord Obelus does not understand.
fn command_stands_for_control(modifiers: ModifiersState, command_is_control: bool) -> bool {
    command_is_control && modifiers.super_key() && !modifiers.control_key()
}

/// The modifiers held, as the rest of Obelus names them.
fn modifiers_of(modifiers: ModifiersState, command_is_control: bool) -> KeyModifiers {
    let command = command_stands_for_control(modifiers, command_is_control);
    let mut bits = KeyModifiers::empty();
    bits.set(KeyModifiers::SHIFT, modifiers.shift_key());
    bits.set(KeyModifiers::CONTROL, modifiers.control_key() || command);
    bits.set(KeyModifiers::ALT, modifiers.alt_key());
    bits.set(KeyModifiers::SUPER, modifiers.super_key() && !command);
    bits
}

/// Whether a key event is the reader pressing a key.
///
/// Not a release, which a terminal never reports either. And not a press
/// winit made up: a window that gains the focus is handed a press for every
/// key still held at that moment, and the moment it is handed them is the
/// one after `alt+tab` -- so the tab that brought the reader back was typed
/// into the file, once for every time they came back. A key the reader
/// pressed somewhere else is not a key they pressed here.
pub(crate) fn heard(state: winit::event::ElementState, synthetic: bool) -> bool {
    state == winit::event::ElementState::Pressed && !synthetic
}

/// Translates a press, or `None` for one Obelus has no name for.
///
/// `None` covers the modifier keys themselves and the keys a platform sends
/// that mean nothing here. A terminal reports neither -- Obelus asks for the
/// narrowest keyboard protocol precisely so that it does not get key
/// releases and modifier presses -- so dropping them keeps the two front
/// ends saying the same thing.
pub(crate) fn pressed(
    logical: &Key,
    physical: PhysicalKey,
    modifiers: ModifiersState,
    repeat: bool,
) -> Option<KeyEvent> {
    let bits = modifiers_of(modifiers, COMMAND_IS_CONTROL);
    let code = code_of(logical, physical, bits)?;
    // `shift+tab` is its own code in crossterm, and the whole of Obelus
    // reads it as one: it is the key that goes backwards through a row of
    // tabs. The shift stays on, which is what a terminal reports too.
    let code = match (code, bits.contains(KeyModifiers::SHIFT)) {
        (KeyCode::Tab, true) => KeyCode::BackTab,
        (code, _) => code,
    };
    Some(KeyEvent {
        code,
        modifiers: bits,
        // A held key repeats, and a repeat is a press: the keymap reads
        // `Press | Repeat` and refuses `Release`. Holding an arrow down has
        // to walk a list, which is the whole reason the distinction is
        // carried rather than flattened.
        kind: match repeat {
            true => KeyEventKind::Repeat,
            false => KeyEventKind::Press,
        },
        state: KeyEventState::NONE,
    })
}

/// Which key it is, as the rest of Obelus names keys.
fn code_of(logical: &Key, physical: PhysicalKey, bits: KeyModifiers) -> Option<KeyCode> {
    match logical {
        Key::Named(named) => named_code(*named),
        Key::Character(typed) => {
            let mut characters = typed.chars();
            let first = characters.next()?;
            // More than one character is a dead key resolving, or an input
            // method committing a word. Neither is a key Obelus binds, and
            // the text it produced arrives as text rather than as a press.
            if characters.next().is_some() {
                return None;
            }
            // A chord is named by the key, not by what the layout made of
            // it. macOS composes with alt -- `alt+f` produces `ƒ` -- and a
            // binding written `alt+f` would never match it. Asking the
            // physical key only in that case leaves the ordinary path
            // alone, which matters for a layout that is not QWERTY: there
            // `ctrl+c` is the letter the reader sees on the key they
            // pressed, and that is the letter a terminal would have
            // reported too.
            if bits.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !first.is_ascii()
                && let Some(plain) = ascii_of(physical)
            {
                return Some(KeyCode::Char(plain));
            }
            Some(KeyCode::Char(first))
        }
        // A dead key on its way to composing something, and whatever a
        // platform could not name. Neither is a press to act on.
        Key::Dead(_) | Key::Unidentified(_) => None,
    }
}

/// The named keys Obelus has a name for.
fn named_code(named: NamedKey) -> Option<KeyCode> {
    Some(match named {
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Space => KeyCode::Char(' '),
        NamedKey::Backspace => KeyCode::Backspace,
        NamedKey::Delete => KeyCode::Delete,
        NamedKey::Insert => KeyCode::Insert,
        NamedKey::Escape => KeyCode::Esc,
        NamedKey::ArrowUp => KeyCode::Up,
        NamedKey::ArrowDown => KeyCode::Down,
        NamedKey::ArrowLeft => KeyCode::Left,
        NamedKey::ArrowRight => KeyCode::Right,
        NamedKey::Home => KeyCode::Home,
        NamedKey::End => KeyCode::End,
        NamedKey::PageUp => KeyCode::PageUp,
        NamedKey::PageDown => KeyCode::PageDown,
        NamedKey::F1 => KeyCode::F(1),
        NamedKey::F2 => KeyCode::F(2),
        NamedKey::F3 => KeyCode::F(3),
        NamedKey::F4 => KeyCode::F(4),
        NamedKey::F5 => KeyCode::F(5),
        NamedKey::F6 => KeyCode::F(6),
        NamedKey::F7 => KeyCode::F(7),
        NamedKey::F8 => KeyCode::F(8),
        NamedKey::F9 => KeyCode::F(9),
        NamedKey::F10 => KeyCode::F(10),
        NamedKey::F11 => KeyCode::F(11),
        NamedKey::F12 => KeyCode::F(12),
        // Everything else, which is mostly the modifiers themselves and the
        // media keys. Obelus binds none of them, and a variant nothing
        // reads is indistinguishable from a broken feature.
        _ => return None,
    })
}

/// The letter or digit on a key, whatever the layout makes of it.
fn ascii_of(physical: PhysicalKey) -> Option<char> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    Some(match code {
        Physical::KeyA => 'a',
        Physical::KeyB => 'b',
        Physical::KeyC => 'c',
        Physical::KeyD => 'd',
        Physical::KeyE => 'e',
        Physical::KeyF => 'f',
        Physical::KeyG => 'g',
        Physical::KeyH => 'h',
        Physical::KeyI => 'i',
        Physical::KeyJ => 'j',
        Physical::KeyK => 'k',
        Physical::KeyL => 'l',
        Physical::KeyM => 'm',
        Physical::KeyN => 'n',
        Physical::KeyO => 'o',
        Physical::KeyP => 'p',
        Physical::KeyQ => 'q',
        Physical::KeyR => 'r',
        Physical::KeyS => 's',
        Physical::KeyT => 't',
        Physical::KeyU => 'u',
        Physical::KeyV => 'v',
        Physical::KeyW => 'w',
        Physical::KeyX => 'x',
        Physical::KeyY => 'y',
        Physical::KeyZ => 'z',
        Physical::Digit0 => '0',
        Physical::Digit1 => '1',
        Physical::Digit2 => '2',
        Physical::Digit3 => '3',
        Physical::Digit4 => '4',
        Physical::Digit5 => '5',
        Physical::Digit6 => '6',
        Physical::Digit7 => '7',
        Physical::Digit8 => '8',
        Physical::Digit9 => '9',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Command is control where it is told to be, and both held together is
    /// neither.
    ///
    /// Deliberate break: dropping `command` from the control bit fails the
    /// first assertion, and dropping `!modifiers.control_key()` from
    /// `command_stands_for_control` fails the last -- `ctrl+cmd+q` would
    /// leave.
    #[test]
    fn command_is_control_where_it_is_told_to_be() {
        let command = ModifiersState::SUPER;
        assert_eq!(modifiers_of(command, true), KeyModifiers::CONTROL);
        assert_eq!(modifiers_of(command, false), KeyModifiers::SUPER);
        assert_eq!(
            modifiers_of(command | ModifiersState::SHIFT, true),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        );
        assert_eq!(
            modifiers_of(ModifiersState::CONTROL, true),
            KeyModifiers::CONTROL,
            "control is still control"
        );
        assert!(
            modifiers_of(command | ModifiersState::CONTROL, true).contains(KeyModifiers::SUPER),
            "control and command together is a key Obelus does not understand"
        );
    }

    /// What a press with command arrives as follows the platform's constant,
    /// and so does whether a terminal is told it was command.
    ///
    /// Deliberate break: passing `false` to `modifiers_of` in `pressed`
    /// fails this on macOS, and passing `true` fails it everywhere else.
    #[test]
    fn a_press_with_command_is_what_the_platform_says() {
        let save = pressed(
            &character("s"),
            PhysicalKey::Code(Physical::KeyS),
            ModifiersState::SUPER,
            false,
        )
        .expect("cmd+s is a press");
        let wanted = match COMMAND_IS_CONTROL {
            true => KeyModifiers::CONTROL,
            false => KeyModifiers::SUPER,
        };
        assert_eq!(save.modifiers, wanted);
        assert_eq!(by_command(ModifiersState::SUPER), COMMAND_IS_CONTROL);
        assert!(!by_command(ModifiersState::CONTROL));
    }

    /// The three keys a Mac window answers are the window's, and the rest of
    /// command is Obelus's.
    ///
    /// Deliberate break: answering `h` whether alt is held or not fails the
    /// second assertion; asking `code_of` without control fails the one on
    /// a Russian layout; dropping the `control_key` refusal fails the last.
    #[test]
    fn the_window_keeps_three_keys_of_command() {
        let h = PhysicalKey::Code(Physical::KeyH);
        let command = ModifiersState::SUPER;
        assert_eq!(
            asked_of_the_window(&character("h"), h, command),
            Some(Asked::Hide)
        );
        // With command held winit names the key without alt, so this is
        // `h` rather than the `˙` alt alone would compose.
        assert_eq!(
            asked_of_the_window(&character("h"), h, command | ModifiersState::ALT),
            Some(Asked::HideOthers)
        );
        // And on a layout that is not Latin it names the layout's letter.
        assert_eq!(
            asked_of_the_window(&character("\u{440}"), h, command),
            Some(Asked::Hide),
            "cmd+h on a Russian layout is still cmd+h"
        );
        assert_eq!(
            asked_of_the_window(&character("m"), PhysicalKey::Code(Physical::KeyM), command),
            Some(Asked::Minimize)
        );
        assert_eq!(
            asked_of_the_window(&character("q"), PhysicalKey::Code(Physical::KeyQ), command),
            None,
            "cmd+q is the key that leaves, which is Obelus's"
        );
        assert_eq!(
            asked_of_the_window(&character("h"), h, ModifiersState::CONTROL),
            None
        );
        assert_eq!(
            asked_of_the_window(&character("h"), h, command | ModifiersState::CONTROL),
            None
        );
    }

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    /// A press winit made up when the window came back is not heard, and
    /// neither is a release; a press the reader made is.
    ///
    /// Deliberate break: dropping `!synthetic` from `heard` fails this --
    /// and puts a tab in the file for every `alt+tab` back to the window,
    /// which is how it was found. What this cannot see is the window asking
    /// `heard` at all, which takes a window to see.
    #[test]
    fn a_key_held_on_the_way_back_is_not_pressed_here() {
        use winit::event::ElementState::{Pressed, Released};
        assert!(heard(Pressed, false));
        assert!(!heard(Pressed, true), "a press winit made up was heard");
        assert!(!heard(Released, false));
    }

    /// The three presses a terminal cannot tell from another key arrive as
    /// themselves.
    ///
    /// Deliberate break: naming `KeyCode::Tab` for a `ctrl+i` -- which is
    /// what the terminal's byte would have made of it -- fails here and
    /// nowhere else.
    #[test]
    fn a_window_tells_the_ambiguous_chords_apart() {
        let control = ModifiersState::CONTROL;
        let eye = pressed(
            &character("i"),
            PhysicalKey::Code(Physical::KeyI),
            control,
            false,
        )
        .expect("ctrl+i is a press");
        assert_eq!(eye.code, KeyCode::Char('i'));
        assert_eq!(eye.modifiers, KeyModifiers::CONTROL);

        let tab = pressed(
            &Key::Named(NamedKey::Tab),
            PhysicalKey::Code(Physical::Tab),
            ModifiersState::empty(),
            false,
        )
        .expect("tab is a press");
        assert_eq!(tab.code, KeyCode::Tab);
        assert_ne!(eye.code, tab.code);
    }

    /// Shift and tab is the key that walks backwards, and says so in the
    /// code rather than only in the modifier.
    ///
    /// Deliberate break: leaving `KeyCode::Tab` alone when shift is held
    /// fails this.
    #[test]
    fn shift_and_tab_is_the_key_that_goes_back() {
        let back = pressed(
            &Key::Named(NamedKey::Tab),
            PhysicalKey::Code(Physical::Tab),
            ModifiersState::SHIFT,
            false,
        )
        .expect("shift+tab is a press");
        assert_eq!(back.code, KeyCode::BackTab);
        assert!(back.modifiers.contains(KeyModifiers::SHIFT));
    }

    /// A chord is named by the key, not by what a layout composed from it.
    ///
    /// Deliberate break: taking the logical character unconditionally makes
    /// this `ƒ`, which no binding matches.
    #[test]
    fn a_composed_chord_is_named_by_its_key() {
        let composed = pressed(
            &character("ƒ"),
            PhysicalKey::Code(Physical::KeyF),
            ModifiersState::ALT,
            false,
        )
        .expect("alt+f is a press");
        assert_eq!(composed.code, KeyCode::Char('f'));
        assert_eq!(composed.modifiers, KeyModifiers::ALT);
    }

    /// Typing a character a layout produces is left exactly as the layout
    /// produced it.
    ///
    /// Deliberate break: reaching for the physical key without a chord
    /// turns a Dvorak reader's `j` into `c`, which this catches.
    #[test]
    fn what_the_layout_typed_is_what_arrives() {
        let typed = pressed(
            &character("j"),
            PhysicalKey::Code(Physical::KeyC),
            ModifiersState::empty(),
            false,
        )
        .expect("a letter is a press");
        assert_eq!(typed.code, KeyCode::Char('j'));
    }

    /// A modifier on its own is not a press.
    ///
    /// Deliberate break: falling through to `KeyCode::Null` instead of
    /// `None` sends the application an event on every shift.
    #[test]
    fn a_modifier_by_itself_says_nothing() {
        assert!(
            pressed(
                &Key::Named(NamedKey::Shift),
                PhysicalKey::Code(Physical::ShiftLeft),
                ModifiersState::SHIFT,
                false,
            )
            .is_none()
        );
    }

    /// A held key repeats, and the keymap is told it was a repeat.
    ///
    /// Deliberate break: reporting `Press` for a repeat passes every other
    /// test here.
    #[test]
    fn a_held_key_arrives_as_a_repeat() {
        let held = pressed(
            &Key::Named(NamedKey::ArrowDown),
            PhysicalKey::Code(Physical::ArrowDown),
            ModifiersState::empty(),
            true,
        )
        .expect("a held arrow is a press");
        assert_eq!(held.kind, KeyEventKind::Repeat);
        assert_eq!(held.code, KeyCode::Down);
    }
}
