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
    /// under it, the change under it, the bracket that matches it -- and
    /// the arrows, which step between changes and through the places the
    /// reader has been.
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
                // `ctrl+a` for all of it, as in every program with a
                // selection. It is also screen's prefix and tmux's other
                // one: a reader inside either of those has to rebind it,
                // and the keys page is where.
                Binding {
                    command: Command::SelectionAll,
                    context: Context::Normal,
                    chord: control('a'),
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
                // `alt+d` for the diff of this line: a question about the
                // line under the cursor, asked with the first letter of the
                // answer. Who wrote it is not here -- that is a setting,
                // because it is on until the reader says otherwise.
                Binding {
                    command: Command::GitHunk,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('d'), KeyModifiers::ALT),
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
            // The same judgement the page that binds keys makes. A chord
            // that cannot fire is worse in a file than on a page: there is
            // nothing on screen to say why the key does nothing.
            if let Some(why) = why_not(chord) {
                tracing::warn!(name, text, why, "not a key obelus can be given");
                continue;
            }
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

/// Why a chord cannot be bound to a command, or `None` if it can.
///
/// One judgement, used by the page that binds keys, by the table read out of
/// the config file, and by the test that holds the shipped table to the same
/// rule. The three families are the whole of what is allowed:
///
/// * a bare function key, which is what the most used commands are on;
/// * `ctrl` and a character, which is what does something to the file;
/// * `alt` and a character, an arrow or enter, which is what asks about the
///   cursor.
///
/// Everything else is refused with the reason, because every one of them is
/// a key that would do nothing -- or would do nothing *here*, or nothing on
/// the next machine -- and a binding that silently never fires is worse
/// than a key the reader cannot have.
#[must_use]
pub fn why_not(chord: KeyChord) -> Option<&'static str> {
    let alone = chord.modifiers.is_empty();
    let control = chord.modifiers == KeyModifiers::CONTROL;
    let alt = chord.modifiers == KeyModifiers::ALT;

    // Two of them, or shift with another: shift extends and reverses and
    // names nothing, and a second modifier is either the desktop's
    // (`ctrl+alt`) or something only a terminal speaking the keyboard
    // protocol can report (`ctrl+shift`).
    if !alone && !control && !alt {
        return Some("shift extends, and two modifiers is the desktop's");
    }

    match chord.code {
        // The keys that move about a file. The editor takes them before the
        // table is reached -- bare and with shift -- and the rest are
        // spoken for: `ctrl` and an arrow is the word motion obelus does
        // not have yet, and `ctrl` and a paging key is the previous and
        // next buffer.
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
            if !alt =>
        {
            Some("the editor's own, for moving about a file")
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => None,
        // A function key, which is the one family that wants nothing held.
        KeyCode::F(_) if alone => None,
        KeyCode::F(_) => Some("a function key is bare, or it is two keys on the next terminal"),
        // Enter under alt is the only one of these that is a chord: the
        // rest are what a dialog takes and what typing will mean.
        KeyCode::Enter if alt => None,
        KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Backspace | KeyCode::Delete => {
            Some("every list and box takes this one itself")
        }
        // Escape has one meaning everywhere: give up on the nearest thing.
        KeyCode::Esc => Some("escape always backs out of the nearest thing"),
        KeyCode::Char(_) if alone => Some("typing, not a command"),
        // `ctrl+shift+p` folds onto `ctrl+P`, and a control byte cannot
        // carry a letter's case: only a terminal speaking the keyboard
        // protocol tells the two apart, so the binding would work on this
        // machine and not the next. Alt is different -- it is the escape
        // prefix, so `alt+P` really is the shifted letter.
        KeyCode::Char(character) if control && character.is_ascii_uppercase() => {
            Some("a control byte cannot say which case the letter was")
        }
        // The six the wire cannot tell from tab, enter, newline, backspace,
        // escape and NUL, whatever the reader pressed.
        KeyCode::Char(character) if control && "imjh[ 2".contains(character) => {
            Some("the terminal sends another key for this")
        }
        KeyCode::Char(_) => None,
        _ => Some("not a key obelus can be given"),
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

    /// Every binding obelus ships with is one a reader could have made.
    ///
    /// The shipped table and the page that binds keys answer to the same
    /// function, so a default outside the families would be a key obelus
    /// gives itself and refuses to the reader.
    #[test]
    fn every_default_binding_is_one_the_reader_could_make() {
        for binding in Keymap::new().bindings() {
            let chord = binding.chord;
            // Escape is the one key obelus keeps and a reader cannot have:
            // the rule is about what may be *taken*, and what escape means
            // -- give up on the nearest thing -- is not negotiable.
            if chord.code == KeyCode::Esc {
                continue;
            }
            assert_eq!(
                super::why_not(chord),
                None,
                "{} is on {}, which obelus would not let a reader bind",
                binding.command.name(),
                chord.label_in(false)
            );
            // And on a key every terminal has: a function key past the
            // twelfth is one a reader's keyboard may send and a default
            // cannot assume.
            if let KeyCode::F(number) = chord.code {
                assert!(
                    (1..=12).contains(&number),
                    "{} is on f{number}, which not every keyboard has",
                    binding.command.name()
                );
            }
            // `ctrl+b` is tmux's prefix, so obelus does not ship it --
            // though a reader outside tmux may have it.
            assert_ne!(
                chord,
                super::control('b'),
                "{} is on tmux's prefix",
                binding.command.name()
            );
        }
    }

    /// Each way out of the families is refused, and with a reason.
    ///
    /// Every one of these is a key that would do nothing, or nothing here,
    /// or nothing on the next machine -- which is the same as a binding
    /// that silently never fires.
    #[test]
    fn the_keys_that_cannot_be_bound_are_refused() {
        let refused = [
            // What the editor takes before the table is reached, and what
            // it has said it wants next.
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Left, KeyModifiers::SHIFT),
            (KeyCode::Home, KeyModifiers::NONE),
            (KeyCode::End, KeyModifiers::CONTROL),
            (KeyCode::PageDown, KeyModifiers::NONE),
            (KeyCode::PageUp, KeyModifiers::CONTROL),
            (KeyCode::Right, KeyModifiers::CONTROL),
            // Typing, and the keys every list and box takes itself.
            (KeyCode::Char('x'), KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
            (KeyCode::Tab, KeyModifiers::NONE),
            (KeyCode::Backspace, KeyModifiers::NONE),
            (KeyCode::Delete, KeyModifiers::NONE),
            (KeyCode::Esc, KeyModifiers::NONE),
            // Two keys on the next terminal.
            (KeyCode::F(5), KeyModifiers::SHIFT),
            (
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            (
                KeyCode::Char('x'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            // The six the wire cannot tell from another key.
            (KeyCode::Char('i'), KeyModifiers::CONTROL),
            (KeyCode::Char('m'), KeyModifiers::CONTROL),
            (KeyCode::Char('j'), KeyModifiers::CONTROL),
            (KeyCode::Char('h'), KeyModifiers::CONTROL),
            (KeyCode::Char('['), KeyModifiers::CONTROL),
            (KeyCode::Char(' '), KeyModifiers::CONTROL),
        ];
        for (code, modifiers) in refused {
            let chord = KeyChord::new(code, modifiers);
            assert!(
                super::why_not(chord).is_some(),
                "{} is offered, and it would never fire",
                chord.label_in(false)
            );
        }

        // And the families themselves are not refused, including the two
        // the editor leaves alone: a function key past the twelfth is one
        // the reader's own terminal sends, and alt with an arrow is how
        // changes and history are walked.
        let allowed = [
            (KeyCode::F(1), KeyModifiers::NONE),
            (KeyCode::F(12), KeyModifiers::NONE),
            (KeyCode::F(13), KeyModifiers::NONE),
            (KeyCode::Char('x'), KeyModifiers::CONTROL),
            (KeyCode::Char('b'), KeyModifiers::CONTROL),
            (KeyCode::Char('x'), KeyModifiers::ALT),
            (KeyCode::Enter, KeyModifiers::ALT),
            (KeyCode::Up, KeyModifiers::ALT),
            (KeyCode::Left, KeyModifiers::ALT),
        ];
        for (code, modifiers) in allowed {
            let chord = KeyChord::new(code, modifiers);
            assert_eq!(
                super::why_not(chord),
                None,
                "{} is refused, and it is one of the families",
                chord.label_in(false)
            );
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
