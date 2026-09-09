//! Keys to commands.
//!
//! The table is data on [`App`](crate::app::App), not a `static`. That is the
//! whole mechanism behind "the user can rebind keys": loading a table from a
//! file later replaces a constructor, not the lookup path.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::command::Command;

/// A key plus its modifiers, in a form two equivalent presses agree on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyChord {
    /// How to write the chord for a reader to type.
    ///
    /// Anything that shows a key to the user goes through this, so a rebound
    /// key changes what is displayed rather than leaving a hint that lies.
    #[must_use]
    pub fn label(self) -> String {
        let mut label = String::new();
        for (modifier, name) in [
            (KeyModifiers::CONTROL, "ctrl"),
            (KeyModifiers::ALT, "alt"),
            (KeyModifiers::SHIFT, "shift"),
        ] {
            if self.modifiers.contains(modifier) {
                label.push_str(name);
                label.push('+');
            }
        }
        match self.code {
            KeyCode::Char(' ') => label.push_str("space"),
            KeyCode::Char(character) => label.push(character),
            KeyCode::Left => label.push('\u{2190}'),
            KeyCode::Up => label.push('\u{2191}'),
            KeyCode::Right => label.push('\u{2192}'),
            KeyCode::Down => label.push('\u{2193}'),
            KeyCode::Enter => label.push_str("enter"),
            KeyCode::Esc => label.push_str("esc"),
            KeyCode::Home => label.push_str("home"),
            KeyCode::End => label.push_str("end"),
            KeyCode::PageUp => label.push_str("pageup"),
            KeyCode::PageDown => label.push_str("pagedown"),
            KeyCode::Backspace => label.push_str("backspace"),
            other => label.push_str(&format!("{other:?}").to_lowercase()),
        }
        label
    }

    /// Builds a chord, normalizing it.
    #[must_use]
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        // Terminals disagree about shifted letters: some report `Char('A')`
        // with SHIFT, some `Char('a')` with SHIFT, some `Char('A')` bare.
        // Fold all three onto the uppercase character with SHIFT removed.
        // Modifiers beyond these three are dropped: SUPER and HYPER arrive
        // only from terminals speaking the kitty protocol, and a binding on
        // one would work in some terminals and not others.
        let modifiers =
            modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        match code {
            KeyCode::Char(character) if modifiers.contains(KeyModifiers::SHIFT) => Self {
                code: KeyCode::Char(character.to_ascii_uppercase()),
                modifiers: modifiers - KeyModifiers::SHIFT,
            },
            _ => Self { code, modifiers },
        }
    }

    /// The chord a key event stands for, or `None` if the event is not a press.
    ///
    /// Release events must be discarded rather than matched. Terminals
    /// speaking the kitty keyboard protocol report both a press and a release,
    /// and treating them alike fires every binding twice.
    #[must_use]
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        match event.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                Some(Self::new(event.code, event.modifiers))
            }
            KeyEventKind::Release => None,
        }
    }
}

/// Which set of bindings applies.
///
/// Only two so far. The fallback to [`Context::Always`] looks redundant at two
/// contexts, but the diff view and the reference panel each add one, and the
/// lookup path should not have to change then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Applies whatever obelus is showing.
    Always,
    /// Reading a file, with no picker open.
    Normal,
}

/// One key bound to one command in one context.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    /// What the key does.
    pub command: Command,
    /// Where the binding applies.
    pub context: Context,
    /// The key.
    pub chord: KeyChord,
}

/// The whole key table.
#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Keymap {
    /// The bindings obelus ships with.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: vec![
                Binding {
                    command: Command::FileOpen,
                    context: Context::Normal,
                    chord: control('f'),
                },
                Binding {
                    command: Command::BufferList,
                    context: Context::Normal,
                    chord: control('e'),
                },
                Binding {
                    command: Command::CommandPalette,
                    context: Context::Normal,
                    chord: control('p'),
                },
                // `theme.select` has no key. It is reached from the palette,
                // which is what the palette is for; giving every command a
                // chord is how a key table stops being memorable.
                Binding {
                    command: Command::FileReload,
                    context: Context::Normal,
                    chord: control('r'),
                },
                Binding {
                    command: Command::Quit,
                    context: Context::Always,
                    chord: control('q'),
                },
            ],
        }
    }

    /// The command a key event runs in `context`, if any.
    ///
    /// The specific context wins over [`Context::Always`], so a view can
    /// shadow a global binding.
    #[must_use]
    pub fn lookup(&self, event: &KeyEvent, context: Context) -> Option<Command> {
        let chord = KeyChord::from_event(event)?;
        if let Some(command) = self.find(chord, context) {
            return Some(command);
        }
        if context == Context::Always {
            return None;
        }
        self.find(chord, Context::Always)
    }

    fn find(&self, chord: KeyChord, context: Context) -> Option<Command> {
        self.bindings
            .iter()
            .find(|binding| binding.context == context && binding.chord == chord)
            .map(|binding| binding.command)
    }

    /// Builds a table from bindings.
    ///
    /// The point of the table being data: a configuration file becomes a
    /// second caller of this rather than a change to lookup.
    #[must_use]
    pub fn from_bindings(bindings: Vec<Binding>) -> Self {
        Self { bindings }
    }

    /// The key bound to a command, if one is.
    #[must_use]
    pub fn chord_for(&self, command: Command) -> Option<KeyChord> {
        self.bindings
            .iter()
            .find(|binding| binding.command == command)
            .map(|binding| binding.chord)
    }

    /// Every binding, for tests and for showing keys next to command names.
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

/// A chord for `ctrl` plus a character.
#[must_use]
pub fn control(character: char) -> KeyChord {
    KeyChord::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}
