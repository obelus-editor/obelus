//! Keys to commands.
//!
//! The table is data on [`App`](crate::app::App), not a `static`. That is the
//! whole mechanism behind "the user can rebind keys": loading a table from a
//! file later replaces a constructor, not the lookup path.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{command::Command, icons};

/// The modifiers a binding can name.
///
/// `SUPER`, `HYPER` and `META` are not among them: they reach a terminal
/// program only through the kitty keyboard protocol, which obelus does not
/// ask for, so a binding on one would work in some terminals and not others.
///
/// A key arriving with one of them is therefore not a key obelus understands,
/// and [`KeyChord::from_event`] gives no chord for it. That is deliberately
/// different from ignoring the modifier: `ctrl+super+q` is not `ctrl+q`, and
/// quitting because of the half of the chord we recognize is a wrong answer
/// rather than a missing one.
pub const BINDABLE_MODIFIERS: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT);

/// The modifiers held down, or `None` if any of them is not [bindable].
///
/// Every path that reads a key goes through this — the key table, the editor's
/// motions, the picker — so all of them draw the line in the same place.
///
/// [bindable]: BINDABLE_MODIFIERS
#[must_use]
pub fn modifiers_of(event: &KeyEvent) -> Option<KeyModifiers> {
    (event.modifiers - BINDABLE_MODIFIERS)
        .is_empty()
        .then_some(event.modifiers)
}

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
        self.label_in(icons::enabled())
    }

    /// The same, with the glyphs asked for or refused.
    ///
    /// Takes the switch rather than reading it, so both ways of writing a
    /// chord can be tested. With glyphs, a key that is a *word* -- `pagedown`
    /// is eight columns -- becomes one column, and the modifiers stop being
    /// prefixes; the arrow keys stay arrows either way, being symbols
    /// already. Every glyph is followed by a blank column, because a Nerd
    /// Font's non-`Mono` variants draw them two cells wide.
    #[must_use]
    pub fn label_in(self, glyphs: bool) -> String {
        let mut label = String::new();
        for (modifier, name, glyph) in [
            (KeyModifiers::CONTROL, "ctrl", icons::key::CONTROL),
            (KeyModifiers::ALT, "alt", icons::key::ALT),
            (KeyModifiers::SHIFT, "shift", icons::key::SHIFT),
        ] {
            if self.modifiers.contains(modifier) {
                if glyphs {
                    label.push(glyph);
                    label.push(' ');
                } else {
                    label.push_str(name);
                    label.push('+');
                }
            }
        }

        let named: Option<(&str, char)> = match self.code {
            KeyCode::Char(' ') => Some(("space", icons::key::SPACE)),
            KeyCode::Enter => Some(("enter", icons::key::ENTER)),
            KeyCode::Esc => Some(("esc", icons::key::ESCAPE)),
            KeyCode::Home => Some(("home", icons::key::HOME)),
            KeyCode::End => Some(("end", icons::key::END)),
            KeyCode::PageUp => Some(("pageup", icons::key::PAGE_UP)),
            KeyCode::PageDown => Some(("pagedown", icons::key::PAGE_DOWN)),
            KeyCode::Backspace => Some(("backspace", icons::key::BACKSPACE)),
            KeyCode::Delete => Some(("delete", icons::key::DELETE)),
            KeyCode::Tab => Some(("tab", icons::key::TAB)),
            _ => None,
        };
        // A function key gets its own keycap, which a patched font has one
        // of for each of the twelve. Spelled out otherwise -- `F(1)` is the
        // compiler's word for it rather than anybody's.
        if let KeyCode::F(number) = self.code {
            match icons::key::function(number).filter(|_| glyphs) {
                Some(keycap) => label.push(keycap),
                None => label.push_str(&format!("f{number}")),
            }
            return label;
        }
        match (named, self.code) {
            (Some((_, glyph)), _) if glyphs => label.push(glyph),
            (Some((name, _)), _) => label.push_str(name),
            (None, KeyCode::Char(character)) => label.push(character),
            (None, KeyCode::Left) => label.push('\u{2190}'),
            (None, KeyCode::Up) => label.push('\u{2191}'),
            (None, KeyCode::Right) => label.push('\u{2192}'),
            (None, KeyCode::Down) => label.push('\u{2193}'),
            (None, other) => label.push_str(&format!("{other:?}").to_lowercase()),
        }
        label
    }

    /// Builds a chord, normalizing it.
    #[must_use]
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        // Terminals disagree about shifted letters: some report `Char('A')`
        // with SHIFT, some `Char('a')` with SHIFT, some `Char('A')` bare.
        // Fold all three onto the uppercase character with SHIFT removed.
        //
        // Modifiers are otherwise kept as given. A chord naming one outside
        // `BINDABLE_MODIFIERS` matches no event, which is the honest outcome:
        // a binding that cannot fire, rather than one that fires on a
        // different key.
        match code {
            KeyCode::Char(character) if modifiers.contains(KeyModifiers::SHIFT) => Self {
                code: KeyCode::Char(character.to_ascii_uppercase()),
                modifiers: modifiers - KeyModifiers::SHIFT,
            },
            _ => Self { code, modifiers },
        }
    }

    /// The chord some text names, or `None` if it names none.
    ///
    /// The other direction of [`KeyChord::label_in`] with the glyphs off, so
    /// a chord written into the config file reads back as itself. Spelled
    /// rather than drawn: a file is typed into by hand, and `ctrl+p` is
    /// something a reader can type where a private-use codepoint is not.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut modifiers = KeyModifiers::NONE;
        let mut rest = text.trim();
        loop {
            let (modifier, tail) = match rest {
                _ if rest.len() > 5 && rest[..5].eq_ignore_ascii_case("ctrl+") => {
                    (KeyModifiers::CONTROL, &rest[5..])
                }
                _ if rest.len() > 4 && rest[..4].eq_ignore_ascii_case("alt+") => {
                    (KeyModifiers::ALT, &rest[4..])
                }
                _ if rest.len() > 6 && rest[..6].eq_ignore_ascii_case("shift+") => {
                    (KeyModifiers::SHIFT, &rest[6..])
                }
                _ => break,
            };
            modifiers |= modifier;
            rest = tail;
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            // Both ways of writing an arrow: the drawn one is what obelus
            // writes, and the word is what somebody typing the file by hand
            // reaches for.
            "left" | "\u{2190}" => KeyCode::Left,
            "up" | "\u{2191}" => KeyCode::Up,
            "right" | "\u{2192}" => KeyCode::Right,
            "down" | "\u{2193}" => KeyCode::Down,
            other => match other
                .strip_prefix('f')
                .and_then(|number| number.parse().ok())
            {
                Some(number) => KeyCode::F(number),
                // The character as it was written, not as it was matched
                // on: a shifted letter *is* the capital -- that is how a
                // chord with shift on a letter is normalised -- so reading
                // the lowercased form back would turn `A` into `a`.
                None => {
                    let mut characters = rest.chars();
                    let character = characters.next()?;
                    if characters.next().is_some() {
                        return None;
                    }
                    KeyCode::Char(character)
                }
            },
        };
        Some(Self::new(code, modifiers))
    }

    /// The chord a key event stands for, or `None` if there is not one.
    ///
    /// Two reasons there is not. Release events must be discarded rather than
    /// matched: terminals speaking the kitty keyboard protocol report both a
    /// press and a release, and treating them alike fires every binding twice.
    /// And a key held with a modifier outside [`BINDABLE_MODIFIERS`] is a key
    /// obelus has no name for.
    #[must_use]
    pub fn from_event(event: &KeyEvent) -> Option<Self> {
        match event.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                Some(Self::new(event.code, modifiers_of(event)?))
            }
            KeyEventKind::Release => None,
        }
    }
}

/// Which set of bindings applies.
///
/// The reader is either reading a file or inside something -- a list, the
/// settings, a conversation -- and those are different worlds as far as the
/// keys go. What is bound everywhere applies to the first and not to the
/// second: a dialog takes the keys it is given here and nothing else, so
/// obelus's own commands cannot put a second dialog over the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Applies whatever obelus is showing -- as long as that is a file.
    Always,
    /// Reading a file, with nothing over it.
    Normal,
    /// The list of open files: the one dialog with a command of its own,
    /// which is the command that closes a file.
    Buffers,
    /// Any other dialog. Nothing is bound here, and that is the point.
    Dialog,
}

impl Context {
    /// Whether what is bound everywhere is bound here.
    ///
    /// Only where a file is what is showing. A global key that reached a
    /// dialog would be a global key opening a second one over it.
    #[must_use]
    pub const fn has_global_keys(self) -> bool {
        matches!(self, Self::Normal)
    }
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
    ///
    /// Three families, and the family is the memorable part.
    ///
    /// **A function key opens something to look at.** Two banks of four,
    /// which is how they sit on the keyboard: `F1`-`F4` are the things to
    /// read -- three sources of files and the one person who can be asked
    /// -- and `F5`-`F8` are finding, which is the same question at four
    /// radii: this file or every file, its text or its names. Bare, never
    /// with a modifier: a terminal that sends `F5+shift` and one that sends
    /// `F17` for the same press are both common, so a modified function key
    /// is a binding that works on one machine and not the next.
    ///
    /// `F9`-`F12` are empty on purpose. The views that would earn them --
    /// a diff, a commit log, a panel of references, a patch to review --
    /// do not exist yet, and filling the bank now would mean moving them
    /// later.
    ///
    /// **Control does something to the file in front of you**, on the
    /// letter of the word: the palette, closing, re-reading, a line
    /// number, copying, leaving.
    ///
    /// **Alt asks about the cursor, or walks what was found**: the symbol
    /// under it, the change under it, who wrote the line under it, the
    /// bracket that matches it -- and the arrows, which step between
    /// changes and through the places the reader has been.
    ///
    /// Shift never names a command. It only ever extends (`shift` plus an
    /// arrow, in the editor) or reverses (`shift+tab`, in the
    /// conversation), which leaves it meaning one thing everywhere.
    ///
    /// Everything else is reached from the palette. A chord for every
    /// command is how a key table stops being memorable, and most of what
    /// is left -- the theme, the log, restarting a server -- is done once
    /// and not again.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: vec![
                // F1-F4: what to read. The three ways into a file, and the
                // agent, which is the fourth thing a reader turns to.
                Binding {
                    command: Command::FileOpen,
                    context: Context::Normal,
                    chord: function(1),
                },
                Binding {
                    command: Command::BufferList,
                    context: Context::Normal,
                    chord: function(2),
                },
                Binding {
                    command: Command::FileChanged,
                    context: Context::Normal,
                    chord: function(3),
                },
                Binding {
                    command: Command::AgentOpen,
                    context: Context::Always,
                    chord: function(4),
                },
                // F5-F8: finding, as a square. Across: the text, then the
                // names a server knows. Down: this file, then every file.
                // One view holds all four, because the reader's question is
                // the same and only its radius changed -- which is why the
                // tabs carry the query between them.
                Binding {
                    command: Command::SearchFile,
                    context: Context::Normal,
                    chord: function(5),
                },
                Binding {
                    command: Command::SearchProject,
                    context: Context::Normal,
                    chord: function(6),
                },
                Binding {
                    command: Command::SymbolOutline,
                    context: Context::Normal,
                    chord: function(7),
                },
                Binding {
                    command: Command::SearchSymbols,
                    context: Context::Normal,
                    chord: function(8),
                },
                // Control, on the letter of the word. `ctrl+p` for the
                // palette; `ctrl+w` is "close this" in every browser and
                // most editors, and in a terminal it is also the shell's
                // "delete the last word", which obelus has no use for
                // because nothing here is typed at a shell.
                Binding {
                    command: Command::CommandPalette,
                    context: Context::Normal,
                    chord: control('p'),
                },
                Binding {
                    command: Command::BufferClose,
                    context: Context::Normal,
                    chord: control('w'),
                },
                // And the same key in the list of open files, where it
                // closes the one on the row. One key that means "close
                // this" everywhere beats a second key that works in one
                // place -- and the list is a dialog, so it has to be bound
                // in it to reach it.
                Binding {
                    command: Command::BufferClose,
                    context: Context::Buffers,
                    chord: control('w'),
                },
                Binding {
                    command: Command::FileReload,
                    context: Context::Normal,
                    chord: control('r'),
                },
                // `ctrl+l` for a line. Free in a full-screen program: the
                // shell's `ctrl+l` clears a screen obelus is drawing.
                Binding {
                    command: Command::GoLine,
                    context: Context::Normal,
                    chord: control('l'),
                },
                // Raw mode makes `ctrl+c` an input event rather than SIGINT,
                // and it is the copy chord every terminal can report. A
                // desktop's `super+c` can map to this later, but cannot be a
                // portable default because many terminals never receive it.
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Normal,
                    chord: control('c'),
                },
                Binding {
                    command: Command::Quit,
                    context: Context::Always,
                    chord: control('q'),
                },
                // Alt: about the cursor. `alt+enter` is what a reader who
                // has used an IDE presses to ask what can be done with the
                // thing under the caret, and that is exactly what the
                // symbol menu is. Alt is also the escape prefix, so it
                // arrives from every terminal -- unlike `ctrl+enter`, which
                // needs the keyboard protocol.
                Binding {
                    command: Command::SymbolMenu,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Enter, KeyModifiers::ALT),
                },
                // `alt+d` for the diff of this line and `alt+b` for its
                // blame: two questions about the line under the cursor,
                // asked with the first letter of the answer.
                Binding {
                    command: Command::GitHunk,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('d'), KeyModifiers::ALT),
                },
                Binding {
                    command: Command::GitBlame,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('b'), KeyModifiers::ALT),
                },
                // `alt+m` for match, which is what this is called
                // everywhere. Not `%`: obelus binds no bare keys, because
                // the day it takes typed text is the day every one of them
                // becomes a character.
                Binding {
                    command: Command::GoBracket,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('m'), KeyModifiers::ALT),
                },
                // The arrows under `alt`, because stepping between changes
                // is the arrows' own motion at the scale of the diff rather
                // than the line.
                //
                // Not `ctrl+alt+arrow`, which GNOME and KDE take for
                // switching workspaces: a key the desktop eats before the
                // terminal sees it looks like a broken program.
                Binding {
                    command: Command::GitPrevious,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Up, KeyModifiers::ALT),
                },
                Binding {
                    command: Command::GitNext,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Down, KeyModifiers::ALT),
                },
                // The browser's keys, for the browser's idea: a history of
                // places, walked in both directions. vim's `ctrl+o` and
                // `ctrl+i` cannot both be used -- `ctrl+i` *is* tab.
                Binding {
                    command: Command::GoBack,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Left, KeyModifiers::ALT),
                },
                Binding {
                    command: Command::GoForward,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Right, KeyModifiers::ALT),
                },
                // Escape, which means "never mind" everywhere. It reaches
                // the key table only when no picker and no prompt is open,
                // because each of those takes it first -- so this is escape
                // pressed at the file itself, and the only thing there to
                // give up on is a selection.
                Binding {
                    command: Command::SelectionClear,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Esc, KeyModifiers::NONE),
                },
            ],
        }
    }

    /// The command a key event runs in `context`, if any.
    ///
    /// The specific context wins over [`Context::Always`], so a view can
    /// shadow a global binding -- and a dialog's context does not reach the
    /// global bindings at all, which is what makes a dialog a dialog.
    #[must_use]
    pub fn lookup(&self, event: &KeyEvent, context: Context) -> Option<Command> {
        let chord = KeyChord::from_event(event)?;
        if let Some(command) = self.find(chord, context) {
            return Some(command);
        }
        if !context.has_global_keys() {
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

    /// Which command a chord runs, wherever it is bound.
    ///
    /// For the page that binds keys: what makes a chord unavailable is that
    /// it already means something *somewhere*, whatever context that is.
    /// One key, one meaning, is a rule a reader can hold in their head.
    #[must_use]
    pub fn command_on(&self, chord: KeyChord) -> Option<Command> {
        self.bindings
            .iter()
            .find(|binding| binding.chord == chord)
            .map(|binding| binding.command)
    }

    /// Moves a command onto another key, or takes its key away.
    ///
    /// Every binding of it, because a command bound in two contexts is one
    /// command with one key: `buffer.close` closes the file being read and
    /// the file on the row of a list, and a reader who rebinds it means
    /// both. A command that had no key gets one where the reader is
    /// reading, which is where a key they press belongs.
    pub fn rebind(&mut self, command: Command, chord: Option<KeyChord>) {
        let contexts: Vec<Context> = self
            .bindings
            .iter()
            .filter(|binding| binding.command == command)
            .map(|binding| binding.context)
            .collect();
        self.bindings.retain(|binding| binding.command != command);
        let Some(chord) = chord else {
            return;
        };
        let contexts = match contexts.is_empty() {
            true => vec![Context::Normal],
            false => contexts,
        };
        for context in contexts {
            self.bindings.push(Binding {
                command,
                context,
                chord,
            });
        }
    }

    /// The table with the reader's own bindings applied over the defaults.
    ///
    /// What is in the file is a list of changes, not the whole table: a
    /// reader who rebinds one key should still be given the new default for
    /// everything they said nothing about.
    ///
    /// Anything the file names that obelus does not -- a command that has
    /// been renamed, a chord it cannot read -- is skipped with a word in the
    /// log. A config file with a typo in it should leave a reader with
    /// obelus, not with a table full of holes.
    #[must_use]
    pub fn with(bindings: &std::collections::BTreeMap<String, String>) -> Self {
        let mut keymap = Self::new();
        for (name, text) in bindings {
            let Some(command) = crate::command::by_name(name) else {
                tracing::warn!(name, "no command by that name to bind");
                continue;
            };
            // An empty chord is the reader having taken the key away, which
            // is a decision and not a mistake.
            if text.is_empty() {
                keymap.rebind(command, None);
                continue;
            }
            let Some(chord) = KeyChord::parse(text) else {
                tracing::warn!(name, text, "not a key obelus can read");
                continue;
            };
            keymap.rebind(command, Some(chord));
        }
        keymap
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

/// A chord for a function key, with nothing held.
///
/// Bare is the whole point: `F5` is the same press on every terminal, while
/// `shift+F5` is `F5+shift` on some and `F17` on others.
#[must_use]
pub fn function(number: u8) -> KeyChord {
    KeyChord::new(KeyCode::F(number), KeyModifiers::NONE)
}

/// A chord for `ctrl` plus a character.
#[must_use]
pub fn control(character: char) -> KeyChord {
    KeyChord::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::{Context, KeyChord, Keymap};

    /// Every binding obelus ships with belongs to one of the three families.
    ///
    /// The families are the whole of what makes the table memorable, so a
    /// binding outside them is a key nobody will guess -- and each of the
    /// ways out of them is a key that does not work somewhere:
    ///
    /// * a function key with a modifier, which one terminal reports as
    ///   `shift+F5` and the next as `F17`;
    /// * `ctrl` plus `i`, `m`, `j`, `h`, `[` or space, which the wire cannot
    ///   tell from tab, enter, newline, backspace, escape and NUL;
    /// * `ctrl+a` or `ctrl+b`, which screen and tmux take before obelus is
    ///   asked;
    /// * `shift` naming a command of its own, when everywhere else it only
    ///   extends or reverses what another key does.
    #[test]
    fn every_default_binding_belongs_to_a_family() {
        for binding in Keymap::new().bindings() {
            let chord = binding.chord;
            let name = binding.command.name();
            assert!(
                !chord.modifiers.contains(KeyModifiers::SHIFT),
                "{name} is on a shifted key, and shift names no commands"
            );
            match (chord.code, chord.modifiers) {
                (KeyCode::F(number), KeyModifiers::NONE) => assert!(
                    (1..=12).contains(&number),
                    "{name} is on f{number}, which not every keyboard has"
                ),
                (KeyCode::F(number), modifiers) => {
                    panic!("{name} is on f{number} with {modifiers:?} held, which is two keys")
                }
                (KeyCode::Char(character), KeyModifiers::CONTROL) => assert!(
                    !"imjh[ ab2".contains(character),
                    "ctrl+{character} is not a key obelus can be given"
                ),
                (KeyCode::Char(_) | KeyCode::Enter, KeyModifiers::ALT)
                | (
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right,
                    KeyModifiers::ALT,
                ) => {}
                // Escape, and nothing else, is bound bare: every other bare
                // key is a character the day obelus takes typed text.
                (KeyCode::Esc, KeyModifiers::NONE) => {}
                (code, modifiers) => {
                    panic!("{name} is on {code:?} with {modifiers:?}, which is no family")
                }
            }
        }
    }

    /// The function keys are two banks of four with nothing missing.
    ///
    /// A gap would be a key that does nothing in the middle of a row of
    /// keys that do, and the banks are how the twelve are remembered.
    #[test]
    fn the_function_keys_are_a_bank_at_a_time() {
        let keymap = Keymap::new();
        for number in 1..=8 {
            assert!(
                keymap
                    .command_on(KeyChord::new(KeyCode::F(number), KeyModifiers::NONE))
                    .is_some(),
                "f{number} does nothing, in the middle of a bank that does"
            );
        }
        for number in 9..=12 {
            assert!(
                keymap
                    .command_on(KeyChord::new(KeyCode::F(number), KeyModifiers::NONE))
                    .is_none(),
                "f{number} is bound, and that bank is being kept for the views that will earn it"
            );
        }
    }

    /// The context every command with no default binding would land in.
    #[test]
    fn a_command_with_no_key_is_bound_where_the_reader_is_reading() {
        let mut keymap = Keymap::new();
        keymap.rebind(
            crate::command::Command::ThemeSelect,
            Some(KeyChord::new(KeyCode::F(9), KeyModifiers::NONE)),
        );
        assert_eq!(
            keymap.lookup(
                &crossterm::event::KeyEvent::new(KeyCode::F(9), KeyModifiers::NONE),
                Context::Normal
            ),
            Some(crate::command::Command::ThemeSelect)
        );
    }
}
