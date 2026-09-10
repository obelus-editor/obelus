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
        // A function key is written the way its keycap is, in both forms:
        // there is no glyph for one, and `F(1)` is the compiler's word for
        // it rather than anybody's.
        if let KeyCode::F(number) = self.code {
            label.push_str(&format!("f{number}"));
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: vec![
                // `ctrl+o` for open, as in most things that open a file.
                // `ctrl+f` is deliberately left unbound: it means *find* in
                // every browser and editor, and obelus will want it for
                // searching a file.
                Binding {
                    command: Command::FileOpen,
                    context: Context::Normal,
                    chord: control('o'),
                },
                Binding {
                    command: Command::BufferList,
                    context: Context::Normal,
                    chord: control('e'),
                },
                // `ctrl+w` is "close this" in every browser and most
                // editors. In a terminal it is also the shell's "delete the
                // last word", which obelus has no use for: nothing here is
                // typed at a shell.
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
                    command: Command::CommandPalette,
                    context: Context::Normal,
                    chord: control('p'),
                },
                // `alt+d` for diff, beside `alt+m` for match: both are
                // questions about the line under the cursor rather than
                // things that move the reader.
                Binding {
                    command: Command::GitHunk,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('d'), KeyModifiers::ALT),
                },
                // `ctrl+d` for the files that differ, beside `alt+d` for
                // the way this line differs: the same letter for the same
                // question at two sizes.
                Binding {
                    command: Command::FileChanged,
                    context: Context::Normal,
                    chord: control('d'),
                },
                // `ctrl+f` for find, and `alt` for the same question asked
                // wider: `alt+f` over every file, `alt+s` over the names a
                // server knows. Which is also why one view holds all three
                // -- the reader's question is the same and only its radius
                // changed, so the tabs carry the query between them.
                //
                // `ctrl+f` was left unbound for this from the beginning.
                // `ctrl+s` is not used for the project because it is XOFF on
                // a terminal that has not turned flow control off, and a key
                // that freezes the display on some machines is not a key.
                Binding {
                    command: Command::SearchFile,
                    context: Context::Normal,
                    chord: control('f'),
                },
                Binding {
                    command: Command::SearchProject,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('f'), KeyModifiers::ALT),
                },
                Binding {
                    command: Command::SearchSymbols,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('s'), KeyModifiers::ALT),
                },
                // The arrows under `alt`, because stepping between changes
                // is the arrows' own motion at the scale of the diff rather
                // than the line -- and because `alt` is already what asks
                // about the line under the cursor here, with `alt+d` for the
                // diff and `alt+m` for the match.
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
                // `alt+m` for match, which is what this is called
                // everywhere. Not `%`: obelus binds no bare keys, because
                // the day it takes typed text is the day every one of them
                // becomes a character.
                Binding {
                    command: Command::GoBracket,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('m'), KeyModifiers::ALT),
                },
                // `ctrl+l` for a line. Free in a full-screen program: the
                // shell's `ctrl+l` clears a screen obelus is drawing.
                Binding {
                    command: Command::GoLine,
                    context: Context::Normal,
                    chord: control('l'),
                },
                // `ctrl+t` for the table of contents, which is what an
                // outline is. Also vim's tag stack, which is the same idea
                // reached a different way.
                Binding {
                    command: Command::SymbolOutline,
                    context: Context::Normal,
                    chord: control('t'),
                },
                Binding {
                    command: Command::SymbolMenu,
                    context: Context::Normal,
                    chord: control('g'),
                },
                // The browser's keys, for the browser's idea: a history of
                // places, walked in both directions. vim's `ctrl+o` and
                // `ctrl+i` cannot both be used — `ctrl+i` *is* tab.
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
                // `theme.select` has no key. It is reached from the palette,
                // which is what the palette is for; giving every command a
                // chord is how a key table stops being memorable.
                Binding {
                    command: Command::FileReload,
                    context: Context::Normal,
                    chord: control('r'),
                },
                // Raw mode makes `ctrl+c` an input event rather than SIGINT,
                // and it is the copy chord every terminal can report. A
                // desktop's `super+c` can map to this later, but cannot be a
                // portable default because many terminals never receive it.
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
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Normal,
                    chord: control('c'),
                },
                // The conversation with an agent, on the alt family with
                // the rest of the second tier. `ctrl+a` would be the
                // mnemonic, and it is the one chord a reader's shell,
                // tmux and screen all want for themselves.
                Binding {
                    command: Command::AgentOpen,
                    context: Context::Always,
                    chord: KeyChord::new(KeyCode::Char('a'), KeyModifiers::ALT),
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

/// A chord for `ctrl` plus a character.
#[must_use]
pub fn control(character: char) -> KeyChord {
    KeyChord::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}
