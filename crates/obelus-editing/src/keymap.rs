//! Keys to commands.
//!
//! The table is data on the application, not a `static`. That is the
//! whole mechanism behind "the user can rebind keys": loading a table from a
//! file later replaces a constructor, not the lookup path.
//!
//! `keymap::why_not` is the one judgement of what may be bound, and the
//! three families are the whole of it. It is asked by the page that binds keys,
//! by the table read out of the config file, and by the test that holds the
//! shipped table to the same rule -- so Obelus cannot give itself a key it
//! refuses the reader, and a reason is written once. What it refuses, and why
//! each of them would be a binding that silently never fires:
//!
//! * the arrows, `home`, `end` and the paging keys, bare or with `ctrl` or
//!   `shift` -- the editor takes those before the table is reached, and the
//!   ones it does not take it has said it wants (`ctrl` and an arrow is a word
//!   motion, `ctrl` and a paging key is the previous and next buffer). `alt`
//!   and an arrow is the exception, which is how changes and history are
//!   walked;
//! * `ctrl` plus `i`, `m`, `j`, `h`, `[`, space or `2`, which *are* tab, enter,
//!   newline, backspace, escape and NUL on the wire, whatever the reader
//!   pressed;
//! * a bare character, `enter`, `tab`, `backspace`, `delete` -- typing, and the
//!   keys every list and box takes itself;
//! * `escape`, which Obelus keeps: give up on the nearest thing is not
//!   negotiable, and it is the one default a reader cannot move;
//! * anything with two modifiers, and `ctrl` with a capital letter -- a control
//!   byte cannot say which case the letter was, so `ctrl+shift+p` works only on
//!   a terminal speaking the keyboard protocol. `alt+P` is fine, because alt is
//!   the escape prefix and really does carry the shifted letter;
//! * a function key with anything held, for the same reason.
//!
//! `ctrl+b` belongs to tmux, so Obelus does not ship it -- a reader outside
//! tmux may still have it. `ctrl+a` is screen's prefix and is shipped anyway,
//! because "all of it" is what that key means in every program with a
//! selection.
//!
//! Keys are rebound on the keys page, and the file holds the changes. The
//! table is data on `App`, so a rebinding is `Keymap::rebind` plus a line in
//! the config's `[keys]` -- command *name* to chord spelled out (`ctrl+p`),
//! because an enum's spelling and a keycode are Obelus's business rather than
//! the reader's. What is in the file is a list of changes over the defaults, so
//! a reader who moved one key still gets the new default for everything else,
//! and a name or a chord Obelus cannot read is skipped with a word in the log.
//! Rebinding moves *every* binding of the command -- `close-file` is bound in
//! `Normal` and in `Buffers` and is still one command with one key -- and a
//! command that had none gets one in `Normal`, which is where a key a reader
//! presses belongs. A chord already spoken for is refused on the row that asked
//! for it, with what has it: the row is where the reader is looking, the status
//! row there is the page's own filter, and a passing note would be cleared by
//! the very next keystroke.
//!
//! Modifiers are judged exactly, in one place. `keymap::modifiers_of` is the
//! only judge; `SUPER`/`HYPER`/`META` disqualify a key rather than being masked
//! away. Masking meant `ctrl+super+q` quit.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use obelus_command::Command;

/// The modifiers a binding can name.
///
/// `SUPER`, `HYPER` and `META` are not among them. They do arrive -- Obelus
/// asks for the kitty keyboard protocol, and under it a key is reported with
/// every modifier held -- but only from the terminals that speak it, and the
/// desktop takes them first anyway: super is the window manager's modifier
/// on every system Obelus runs on. A binding there would be eaten before the
/// terminal saw it, which looks to a reader like a broken program. The same
/// reason `ctrl+alt+arrow` is refused further down.
///
/// A key arriving with one of them is therefore not a key Obelus understands,
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
        self.label_in(obelus_icons::enabled())
    }

    /// The same, with the glyphs asked for or refused.
    ///
    /// Takes the switch rather than reading it, so both ways of writing a
    /// chord can be tested. With glyphs the modifiers stop being prefixes
    /// and become their glyph, followed by a blank column, because a Nerd
    /// Font's non-`Mono` variants draw them two cells wide.
    ///
    /// Only the modifiers. The key itself is written as its word whichever
    /// way, and the arrows as arrows: a key that is a word was a picture
    /// once -- `End` a bar with an arrow at it, `F10` a keycap -- and
    /// `Ctrl+End` drawn as two pictures was a chord nobody could read. The
    /// modifiers are the three pictures every keyboard prints, and the ones
    /// a chord repeats, so they are where the columns are saved.
    ///
    /// A key that is a word is written the way keys are written everywhere
    /// -- `Ctrl`, `F1`, `PageDown` -- and this is the *only* spelling: it
    /// is what the screen shows and what `[keys]` in the settings file is
    /// written in, because two spellings of one name is somewhere for them
    /// to differ. An older file in the other case still reads: `parse`
    /// never minded, and that is what makes the change safe.
    ///
    /// A letter is left exactly as it is, and that is not tidiness. **A
    /// shifted letter is the capital** -- it is how a chord with shift on a
    /// letter is normalised -- so `Ctrl+A` is `ctrl+shift+a` and nothing
    /// else. Capitalising one here would name a different key on the
    /// screen, and name it again in the file when the reader copied what
    /// they saw.
    #[must_use]
    pub fn label_in(self, glyphs: bool) -> String {
        let mut label = String::new();
        for (modifier, name, glyph) in [
            (KeyModifiers::CONTROL, "Ctrl", obelus_icons::key::CONTROL),
            (KeyModifiers::ALT, "Alt", obelus_icons::key::ALT),
            (KeyModifiers::SHIFT, "Shift", obelus_icons::key::SHIFT),
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

        let named = match self.code {
            KeyCode::Char(' ') => Some("Space"),
            KeyCode::Enter => Some("Enter"),
            KeyCode::Esc => Some("Esc"),
            KeyCode::Home => Some("Home"),
            KeyCode::End => Some("End"),
            KeyCode::PageUp => Some("PageUp"),
            KeyCode::PageDown => Some("PageDown"),
            KeyCode::Backspace => Some("Backspace"),
            KeyCode::Delete => Some("Delete"),
            KeyCode::Tab => Some("Tab"),
            _ => None,
        };
        match (named, self.code) {
            (Some(name), _) => label.push_str(name),
            // `F(1)` is the compiler's word for it rather than anybody's.
            (None, KeyCode::F(number)) => label.push_str(&format!("F{number}")),
            (None, KeyCode::Char(character)) => label.push(character),
            (None, KeyCode::Left) => label.push('\u{2190}'),
            (None, KeyCode::Up) => label.push('\u{2191}'),
            (None, KeyCode::Right) => label.push('\u{2192}'),
            (None, KeyCode::Down) => label.push('\u{2193}'),
            // `BackTab` and the rest already come out of the debug
            // spelling the way a key is written.
            (None, other) => label.push_str(&format!("{other:?}")),
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
            // Both ways of writing an arrow: the drawn one is what Obelus
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
    /// Obelus has no name for.
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
/// settings, the counts -- and those are different worlds as far as the keys
/// go. What is bound everywhere applies to the first and not to the second:
/// a dialog takes the keys it is given here and nothing else, so Obelus's
/// own commands cannot put a second dialog over the first. Before that a
/// global key worked inside them, which is how `ctrl+o` in a conversation
/// put a file list on top of it -- two things on screen, two escapes to
/// leave, and nothing saying which one a key would reach. So a new dialog
/// gets a context, and a key it should keep gets a binding in it -- not a
/// fall-through.
///
/// A conversation is on the first side of that line, not the second: it is
/// one of the things the reader can be reading, so the keys that work over a
/// file work in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// Applies whatever Obelus is showing -- as long as that is a file.
    Always,
    /// Reading a file, with nothing over it.
    Normal,
    /// The list of what is open: the one dialog with a command of its own,
    /// which is the command that closes what a row names.
    Documents,
    /// Reading a conversation with an agent, with nothing over it.
    ///
    /// A document, so Obelus's own keys reach it -- but not quite the same
    /// document as a file: a key that reads as "the note about what I am
    /// looking at" means "write one about this line" in a file and "show me
    /// the one this came from" here, and those are two commands on one key.
    Chat,
    /// Reading a terminal whose program is still running.
    ///
    /// The other way round from every context here: a terminal is the
    /// program's, so a key is the program's unless this says otherwise,
    /// and what it says is very little -- zed's shape, and for zed's reason.
    /// Escape, `ctrl+c` and `ctrl+w` are a shell's before they are
    /// anybody's. What Obelus keeps is the palette, paste (`ctrl+v` and
    /// `shift+Insert`), copy where something is held, the paging keys and
    /// `ctrl+Home` and `ctrl+End` for reading back (`Terminal::read_back`),
    /// the key that closes it, the key that leaves Obelus, and the function
    /// keys, which open something to look at and are the one family a shell
    /// has no use for (`Keymap::lookup`).
    /// Once the program has ended there is nothing to type to, and a
    /// terminal is read like a file.
    Terminal,
    /// Any other dialog.
    ///
    /// Almost nothing is bound here, and that is the point: a global key
    /// that reached a dialog would be a global key opening a second one
    /// over it. The exception is the four that act on what the reader has
    /// hold of -- copy, cut, paste and taking all of it -- because a dialog
    /// with a box in it has a caret, and a box a reader can select in but
    /// not copy out of is a box with half a selection.
    Dialog,
}

impl Context {
    /// Whether what is bound everywhere is bound here.
    ///
    /// Only where a document is what is showing. A global key that reached
    /// a dialog would be a global key opening a second one over it.
    #[must_use]
    pub const fn has_global_keys(self) -> bool {
        matches!(self, Self::Normal | Self::Chat)
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
    /// The bindings Obelus ships with.
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
    /// is a binding that works on one machine and not the next. Which is
    /// also why one reaches its view from inside another: every view it
    /// names takes the whole screen, so going to it is a swap and never a
    /// stack (`App::switch_view`).
    ///
    /// `F9`-`F12` is the bank a view earns a key from, and git's: a file's
    /// history, a project's and a line's, which are one question at three
    /// widths -- and `F12`, the jump to a definition, which is the one jump
    /// this whole program is for. Each is argued where it is bound.
    ///
    /// **Control does something to the file in front of you**, on the
    /// letter of the word: `p` the palette, `w` close, `r` re-read, `t`
    /// toggle the reading its format has, `l` a line number, `a` all of it,
    /// `c` copy, `q` leave. `ctrl+v` was left alone until there was a paste
    /// to give it, and that is the one chord every reader will try there.
    ///
    /// **Alt asks about the cursor, or walks what was found**: `alt+enter`
    /// the symbol under it, `alt+d` its diff, `alt+f` the run of lines it is
    /// inside, `alt+m` its matching bracket -- and the arrows, which step
    /// up and down between changes and left and right through the places
    /// the reader has been.
    ///
    /// Shift never names a command. It only ever extends (`shift` plus an
    /// arrow, in the editor) or reverses (`shift+tab`, in the
    /// conversation), which leaves it meaning one thing everywhere. The one
    /// exception, `shift+Insert`, is not a name Obelus chose -- see
    /// [`why_not`].
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
                //
                // Reached from inside a view as well as from a file: a key
                // that opens a view goes straight to it from another one --
                // see `App::switch_view`. Which is why the card of every key
                // is not `f1` any more, and has a key of its own:
                // [`keys_card`].
                Binding {
                    command: Command::FileOpen,
                    context: Context::Normal,
                    chord: function(1),
                },
                Binding {
                    command: Command::DocumentList,
                    context: Context::Normal,
                    chord: function(2),
                },
                Binding {
                    command: Command::FileChanged,
                    context: Context::Normal,
                    chord: function(3),
                },
                // Which conversation, asked the same way from a file and
                // from inside one, with a new one as the first answer.
                // It opened the conversation outside one and the list
                // inside, which made the list two presses away from
                // anywhere but a conversation and a fresh conversation the
                // only thing the key ever gave a reader in a file. A
                // conversation falls back to the file's table, so this one
                // binding is both.
                Binding {
                    command: Command::ConversationSelect,
                    context: Context::Normal,
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
                // The third bank is git's. A file's history and a
                // project's are one view at two radii, the way the finding
                // keys are one question at four: the key lands on the tab
                // it names and the other is a left or a right away.
                Binding {
                    command: Command::HistoryFile,
                    context: Context::Normal,
                    chord: function(9),
                },
                Binding {
                    command: Command::HistoryProject,
                    context: Context::Normal,
                    chord: function(10),
                },
                // `f11` beside them, because it is the same subject asked
                // at the narrowest width there is: not this file's commits
                // but this *line's* one.
                Binding {
                    command: Command::HistoryLine,
                    context: Context::Normal,
                    chord: function(11),
                },
                // And `f12`, which is not one of those three: it is the one
                // jump this whole program is for. "Join the semantic graph
                // to the git timeline -- jump to a definition from inside a
                // diff" is the first paragraph Obelus was written under, and
                // the jump had no key at all while every editor a reader
                // arrives from puts it here.
                //
                // Held for git's fourth question once, and nothing ever
                // came: the three widths above are what a history has, and a
                // fourth would be a fourth width of the same question rather
                // than a new one. So git's row is three keys and says so.
                //
                // The one exception to "the symbol questions live in the
                // menu" -- see the note over them in `command`. Its
                // neighbours in every other editor cannot follow it here:
                // `shift+f12` and `ctrl+f12` are refused by `why_not`,
                // because a function key with something held is two keys on
                // the next terminal. A reader who learns this one and
                // reaches for those finds the menu, which is where they
                // were all along.
                Binding {
                    command: Command::SymbolDefinition,
                    context: Context::Normal,
                    chord: function(12),
                },
                // `alt+t` for todo, on the letter of the word like the rest
                // of the alt family, and asking the question alt asks: a
                // note is about the line under the cursor.
                //
                // Writing one down has a key and reading them back does
                // not: the banks are full. `todo` is in the palette, and
                // the keys page is where a reader who opens it often puts
                // it on a key of their own.
                Binding {
                    command: Command::TodoAdd,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('t'), KeyModifiers::ALT),
                },
                // The same key in a conversation, meaning the same thing a
                // level up: "the note about what I am looking at". In a
                // file that is one to write; here it is the one this came
                // out of, and the list opens standing on it.
                //
                // Which is also the only way back, and was worth a key on
                // its own account: `TodoOpen` has been in the palette and
                // on nothing since it was written, so the round trip had an
                // outward leg and no return.
                Binding {
                    command: Command::TodoOpen,
                    context: Context::Chat,
                    chord: KeyChord::new(KeyCode::Char('t'), KeyModifiers::ALT),
                },
                // Control, on the letter of the word. `ctrl+p` for the
                // palette; `ctrl+w` is "close this" in every browser and
                // most editors, and in a terminal it is also the shell's
                // "delete the last word" -- which is why a terminal of
                // Obelus's own leaves it to the shell, and closes with
                // `ctrl+shift+w` below.
                Binding {
                    command: Command::CommandPalette,
                    context: Context::Normal,
                    chord: control('p'),
                },
                Binding {
                    command: Command::DocumentClose,
                    context: Context::Normal,
                    chord: control('w'),
                },
                // And the same key in the list of what is open, where it
                // closes the one on the row. One key that means "close
                // this" everywhere beats a second key that works in one
                // place -- and the list is a dialog, so it has to be bound
                // in it to reach it.
                Binding {
                    command: Command::DocumentClose,
                    context: Context::Documents,
                    chord: control('w'),
                },
                // What a terminal keeps for Obelus while its program runs,
                // and nothing else: the palette, which is how everything
                // else is reached from in there; paste, under the name a
                // desktop sends a terminal for it; and closing it, on the
                // key every terminal a reader has used closes a tab with,
                // because `ctrl+w` is the shell's. See `why_not` for why
                // that one may be held with shift. And leaving: `ctrl+q` is
                // a shell's only as the flow control nobody has used since
                // terminals were printers, and a key that leaves Obelus
                // everywhere but in one kind of document is a key a reader
                // cannot trust.
                Binding {
                    command: Command::CommandPalette,
                    context: Context::Terminal,
                    chord: control('p'),
                },
                Binding {
                    command: Command::Quit,
                    context: Context::Terminal,
                    chord: control('q'),
                },
                Binding {
                    command: Command::Paste,
                    context: Context::Terminal,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::SHIFT),
                },
                // And under the name it has everywhere else in Obelus, which
                // is also the one a desktop sends a *window* for its paste:
                // without it, the desktop's own paste in `obg` put a `^V` in
                // front of the shell. Taken from the program for good --
                // vim's block selection and a shell's literal next character
                // -- which is what Windows Terminal and VS Code's terminal
                // decided too.
                Binding {
                    command: Command::Paste,
                    context: Context::Terminal,
                    chord: control('v'),
                },
                // And copy under the same name. `ctrl+c` copies too, where
                // something is held -- which is a question about the
                // terminal rather than a binding, so it is asked there
                // (`App::terminal_key`).
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Terminal,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::CONTROL),
                },
                Binding {
                    command: Command::DocumentClose,
                    context: Context::Terminal,
                    chord: KeyChord::new(
                        KeyCode::Char('w'),
                        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                    ),
                },
                Binding {
                    command: Command::FileSave,
                    context: Context::Normal,
                    chord: control('s'),
                },
                Binding {
                    command: Command::FileReload,
                    context: Context::Normal,
                    chord: control('r'),
                },
                // `ctrl+t` for toggle, which is what the command is
                // called. It sat in the third bank of function keys until
                // that bank was given to git, and it belongs here anyway:
                // control does something to the file in front of the
                // reader, and a reading of the file is that rather than a
                // thing opened to look at. Not `p`, which is the palette,
                // and not `v`, which is the paste it was being kept for.
                Binding {
                    command: Command::PreviewToggle,
                    context: Context::Normal,
                    chord: control('t'),
                },
                // `ctrl+l` for a line. Free in a full-screen program: the
                // shell's `ctrl+l` clears a screen Obelus is drawing.
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
                // The four a reader arrives already knowing. `ctrl+v` was
                // being kept for this one; `ctrl+y` is redo because
                // `ctrl+shift+z` cannot be said here -- a control byte
                // cannot carry the case of a letter, and the chord only
                // reaches a terminal speaking the kitty protocol.
                Binding {
                    command: Command::SelectionCut,
                    context: Context::Normal,
                    chord: control('x'),
                },
                Binding {
                    command: Command::Paste,
                    context: Context::Normal,
                    chord: control('v'),
                },
                // And again inside a dialog. These three are not about the
                // file, they are about whatever has a caret in it -- and a
                // list, the settings and a conversation all have a box a
                // reader types into. Bound rather than left to fall
                // through, because a dialog answers no global key at all.
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Dialog,
                    chord: control('c'),
                },
                Binding {
                    command: Command::SelectionCut,
                    context: Context::Dialog,
                    chord: control('x'),
                },
                Binding {
                    command: Command::Paste,
                    context: Context::Dialog,
                    chord: control('v'),
                },
                // And taking all of what is in the box, which is how a
                // selection to copy or cut is made in one key.
                Binding {
                    command: Command::SelectionAll,
                    context: Context::Dialog,
                    chord: control('a'),
                },
                // And the same two under the names a desktop uses for
                // them. Argued in `why_not`, which has to let them
                // through: they are the chords omarchy's `super+c` and
                // `super+v` turn into where the thing they are sent to
                // looks like a terminal.
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::CONTROL),
                },
                Binding {
                    command: Command::Paste,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::SHIFT),
                },
                Binding {
                    command: Command::SelectionCopy,
                    context: Context::Dialog,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::CONTROL),
                },
                Binding {
                    command: Command::Paste,
                    context: Context::Dialog,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::SHIFT),
                },
                Binding {
                    command: Command::Undo,
                    context: Context::Normal,
                    chord: control('z'),
                },
                Binding {
                    command: Command::Redo,
                    context: Context::Normal,
                    chord: control('y'),
                },
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
                // And in a dialog, where almost nothing is bound, because
                // the reason almost nothing is -- a global key reaching a
                // dialog would open a second thing over it -- is the one
                // reason this key cannot have: leaving opens nothing. It
                // was already written down as the behaviour (`handle_key`
                // says a layer lets `ctrl+q` fall through it) and was not
                // the behaviour: a dialog's context never reaches what is
                // bound everywhere, so the chord was not a command there at
                // all and the key did nothing. `Context::Documents` falls
                // back to this one, and a conversation to the file's, so
                // this is the whole of what was missing.
                Binding {
                    command: Command::Quit,
                    context: Context::Dialog,
                    chord: control('q'),
                },
                // Alt: about the cursor. Enter on top of that is "open what
                // I am on", so the menu of everything a server can say
                // about the name under the caret is where the two meet.
                //
                // Not borrowed from an IDE, whatever the chord looks like:
                // JetBrains' `alt+enter` is its intentions and quick fixes,
                // which here is `alt+a` -- the server offering to *change*
                // the file, not Obelus offering to go and look at it. A
                // comment claiming the muscle memory would be claiming it
                // for the wrong half.
                //
                // Alt is also the escape prefix, so it arrives from every
                // terminal -- unlike `ctrl+enter`, which needs the keyboard
                // protocol.
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
                // `alt+f` for fold, on the letter of the word like the
                // rest of this family. What it folds is whatever the
                // cursor is inside, which is the question alt asks.
                Binding {
                    command: Command::Fold,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('f'), KeyModifiers::ALT),
                },
                // `alt+m` for match, which is what this is called
                // everywhere. Not `%`: Obelus binds no bare keys, because
                // the day it takes typed text is the day every one of them
                // becomes a character.
                Binding {
                    command: Command::GoBracket,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('m'), KeyModifiers::ALT),
                },
                // `alt+n` and `alt+p` for the next and previous change,
                // joining `alt+d` for the diff of this one: three keys about
                // the same thing, on the letters of what they do.
                //
                // The arrows would read better and are spent better below:
                // `alt+arrow` moving a line is the chord every editor with a
                // mouse has taught, and stepping between changes has letters
                // to fall back on where moving a line has none -- the rules
                // refuse two modifiers, and an arrow may only be bound under
                // `alt`.
                Binding {
                    command: Command::GitPrevious,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('p'), KeyModifiers::ALT),
                },
                Binding {
                    command: Command::GitNext,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('n'), KeyModifiers::ALT),
                },
                // `alt+[` and `alt+]` for the previous and next problem.
                //
                // A second pair of previous-and-next keys, in a different
                // idiom from the first, which is worth saying out loud. The
                // letters are spoken for -- `alt+p` is the previous change
                // and `alt+n` the next one -- and the bracket pair is what
                // every editor with two of these reaches for second: Vim's
                // unimpaired bindings, VS Code's `alt+[`/`alt+]` on the
                // Mac. The alternative was giving the brackets to the
                // changes and the letters to the problems, which moves a
                // key readers already have for no gain.
                //
                // Not `f8` and `shift+f8`, which is where VS Code puts
                // these: a function key here is bare or it is two keys on
                // the next terminal, so only half of that pair can be had,
                // and half a pair is worse than neither.
                Binding {
                    command: Command::SymbolTroublePrevious,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('['), KeyModifiers::ALT),
                },
                Binding {
                    command: Command::SymbolTroubleNext,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char(']'), KeyModifiers::ALT),
                },
                // Not `ctrl+alt+arrow`, which GNOME and KDE take for
                // switching workspaces: a key the desktop eats before the
                // terminal sees it looks like a broken program.
                Binding {
                    command: Command::LineUp,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Up, KeyModifiers::ALT),
                },
                Binding {
                    command: Command::LineDown,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Down, KeyModifiers::ALT),
                },
                // `alt+a` for the actions offered here, on the letter like
                // the rest of this family.
                //
                // This is the one an IDE reader reaches for with a chord --
                // JetBrains' `alt+enter`, VS Code's `ctrl+.` -- and it is on
                // a letter anyway, because neither of those can be had here.
                // `alt+enter` is spent above on a menu Obelus invented; and
                // `ctrl+.` is not a key a terminal has a byte for, so it
                // arrives only from the terminals speaking the keyboard
                // protocol and is silence in the rest. The same reason
                // `ctrl+shift+z` is not redo.
                Binding {
                    command: Command::CodeActions,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('a'), KeyModifiers::ALT),
                },
                // `alt+r` for rename, on the letter of the word like the
                // rest of this family -- and the one key here that changes
                // files the reader cannot see, which is why it asks first.
                Binding {
                    command: Command::SymbolRename,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('r'), KeyModifiers::ALT),
                },
                // `alt+h` for what this is -- hover, which is what every
                // editor calls it and what the protocol calls it, on the
                // letter of the word like the rest of this family.
                Binding {
                    command: Command::SymbolHover,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('h'), KeyModifiers::ALT),
                },
                // `alt+s` for signature: what the call the cursor is
                // inside takes, on the letter of the word like the rest of
                // this family. The way in for a reader who is *reading* a
                // call rather than writing one -- every other way asks
                // because a character was typed, and moving the caret into
                // a call that is already written types nothing.
                Binding {
                    command: Command::SymbolSignature,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('s'), KeyModifiers::ALT),
                },
                // `alt+e` for error: what the server says is wrong with
                // this file, on the letter like the rest of this family.
                Binding {
                    command: Command::SymbolTroubles,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('e'), KeyModifiers::ALT),
                },
                // `alt+w` for widen, joining the family that asks about
                // whatever the caret is in: the first step is the word it
                // is in, and the rest are what the grammar says holds it.
                Binding {
                    command: Command::SelectionWiden,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('w'), KeyModifiers::ALT),
                },
                // `alt+c` for comment, on the letter like the rest of this
                // family -- and `ctrl+/`, which is what everyone else uses,
                // for the terminals that can say it. Most send `ctrl+_` or a
                // bare control byte for that chord and Obelus never hears
                // it; the letter is the one that works everywhere.
                Binding {
                    command: Command::CommentToggle,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('c'), KeyModifiers::ALT),
                },
                Binding {
                    command: Command::CommentToggle,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Char('/'), KeyModifiers::CONTROL),
                },
                // The one key that names itself, and the one binding
                // outside the three families. `Insert` is what this does on
                // every keyboard that has the key, it is bare because it is
                // not a letter and cannot be typing, and a chord for it
                // would be a chord nobody would look for.
                Binding {
                    command: Command::ReplaceToggle,
                    context: Context::Normal,
                    chord: KeyChord::new(KeyCode::Insert, KeyModifiers::NONE),
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
        // The list of open files is a dialog with one command of its own,
        // so what every dialog answers it answers too.
        if context == Context::Documents
            && let Some(command) = self.find(chord, Context::Dialog)
        {
            return Some(command);
        }
        // A conversation is a file's context with a handful of keys that
        // mean something else in it. Falling back rather than repeating the
        // whole table: a key added for a reader in a file should work in a
        // conversation too, and a table that had to name both would be a
        // table with two chances to forget.
        if context == Context::Chat
            && let Some(command) = self.find(chord, Context::Normal)
        {
            return Some(command);
        }
        // A function key opens something to look at, whatever is in front
        // -- and a terminal is the one place that family is asked for by
        // name, because everything else in a file's table is the program's
        // there. Bare only, which is the only way a function key is bound.
        if context == Context::Terminal
            && matches!(chord.code, KeyCode::F(_))
            && chord.modifiers.is_empty()
        {
            return self.find(chord, Context::Normal);
        }
        if !context.has_global_keys() {
            return None;
        }
        self.find(chord, Context::Always)
    }

    /// What this key is bound to *in this context itself*, with no falling
    /// back to what is bound in a file or everywhere.
    ///
    /// Which is the question "has this view said what this key means here",
    /// and it has to be asked apart from [`Self::lookup`]: a view that
    /// binds a key is a view that has taken it, and what the table says
    /// about the same key one level out is what it would have meant
    /// somewhere else.
    #[must_use]
    pub fn bound_here(&self, event: &KeyEvent, context: Context) -> Option<Command> {
        self.find(KeyChord::from_event(event)?, context)
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
    /// command with one key: `close-file` closes the file being read and
    /// the file on the row of a list, and a reader who rebinds it means
    /// both. A command that had no key gets one where the reader is
    /// reading, which is where a key they press belongs.
    ///
    /// Which includes the chords a command has that the reader did not
    /// choose -- copy answers to `ctrl+Insert` as well as `ctrl+c` -- so
    /// the reader gets the one key the page shows them and no second one
    /// firing behind it.
    pub fn rebind(&mut self, command: Command, chord: Option<KeyChord>) {
        // Each context once, not once per binding: a command with two
        // chords in one context appears in that context twice, and one
        // binding per appearance is two bindings on the same key.
        let mut contexts: Vec<Context> = Vec::new();
        for binding in &self.bindings {
            if binding.command == command && !contexts.contains(&binding.context) {
                contexts.push(binding.context);
            }
        }
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
    /// Anything the file names that Obelus does not -- a command that has
    /// been renamed, a chord it cannot read -- is skipped with a word in the
    /// log. A config file with a typo in it should leave a reader with
    /// Obelus, not with a table full of holes.
    ///
    /// What it skipped comes back with it, because a line that bound
    /// nothing looks from the outside exactly like one that bound
    /// something -- and the file it is in is a file the reader can be
    /// shown.
    #[must_use]
    pub fn with(bindings: &std::collections::BTreeMap<String, String>) -> (Self, Vec<Unbound>) {
        let mut keymap = Self::new();
        let mut unbound = Vec::new();
        let skipped = |name: &String, text: &String, why: Unbindable| Unbound {
            name: name.clone(),
            text: text.clone(),
            why,
        };
        for (name, text) in bindings {
            let Some(command) = obelus_command::by_name(name) else {
                tracing::warn!(name, "no command by that name to bind");
                unbound.push(skipped(name, text, Unbindable::NoSuchCommand));
                continue;
            };
            // An empty chord is the reader having taken the key away, which
            // is a decision and not a mistake.
            if text.is_empty() {
                keymap.rebind(command, None);
                continue;
            }
            let Some(chord) = KeyChord::parse(text) else {
                tracing::warn!(name, text, "not a key Obelus can read");
                unbound.push(skipped(name, text, Unbindable::Unreadable));
                continue;
            };
            // The same judgement the page that binds keys makes. A chord
            // that cannot fire is worse in a file than on a page: there is
            // nothing on screen to say why the key does nothing.
            if let Some(why) = why_not(chord) {
                tracing::warn!(name, text, why, "not a key Obelus can be given");
                unbound.push(skipped(name, text, Unbindable::NotAllowed(why)));
                continue;
            }
            keymap.rebind(command, Some(chord));
        }
        (keymap, unbound)
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
/// A line of a `[keys]` table that bound nothing, and why.
///
/// Facts, not words: what to say about one is copy, and copy is written
/// where the rest of what Obelus says to the reader is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unbound {
    /// The command, spelled the way the file spells it.
    pub name: String,
    /// And the chord it was to go on.
    pub text: String,
    /// Why nothing happened.
    pub why: Unbindable,
}

/// What was wrong with a line that bound nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unbindable {
    /// Obelus has no command by that name: one renamed since, or a word
    /// spelled wrong.
    NoSuchCommand,
    /// What was written is not a chord Obelus can read at all.
    Unreadable,
    /// It is a chord, and not one a reader may be given -- with the reason
    /// [`why_not`] gives, which is the reason the page that binds keys
    /// shows.
    NotAllowed(&'static str),
}

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

    // Copy and paste, under the names the desktop knows them by. Not
    // Obelus naming a command with shift -- these two are the chords a
    // terminal has meant by copy and paste since long before any of this,
    // and they are what a desktop sends a *terminal* when the reader
    // presses its own one chord for copy: omarchy's `super+c` is
    // `ctrl+Insert` where it thinks it is talking to a terminal and
    // `ctrl+c` where it does not, and `super+v` is `shift+Insert` or
    // `ctrl+v` the same way. Obelus is both and is read as either, so it
    // answers all four, and which of the two it is taken for stops
    // mattering. One table, so binding it once binds it for `ob` and `obg`
    // together. The same shape as `shift+enter` and `alt+enter` being taken
    // together: one act, two chords, because what arrives is not Obelus's
    // to decide.
    //
    // Which is the whole of the exception: `alt+Insert` is still refused,
    // because nothing sends it. Every terminal reports these the same way,
    // unlike a modified function key, and no terminal shipped here binds
    // them for itself: foot, kitty, alacritty and wezterm all put copy on
    // `ctrl+shift+c`. A terminal that is *configured* to take them keeps
    // them and Obelus never sees them -- which is what a desktop does to
    // make its own chord land somewhere in a shell (omarchy adds them to
    // foot's bindings for exactly that), and is the one case this cannot
    // reach. Measured, by pressing it at `ob` in such a terminal and at
    // `ob` in the same terminal with that one binding turned off: the first
    // copies nothing, the second copies the selection.
    //
    // And the two halves come apart there. With the terminal holding both
    // keys, paste still works and copy cannot, because pasting is something
    // a terminal can do on behalf of the program inside it and copying is
    // not: it puts the words down the pty, where they arrive as an ordinary
    // bracketed paste. Copy it cannot do, because what is selected is the
    // program's and the terminal does not know -- and `ob` has taken the
    // mouse, so the selection the terminal *would* copy is empty. The key
    // does nothing at all. Which is the shape of the whole problem: a
    // desktop can tell a terminal from a window, and nothing can tell a
    // shell from a program that has taken the terminal over. Not something
    // Obelus can answer from the inside -- the terminal decides before
    // Obelus is asked, and decides unconditionally.
    if chord.code == KeyCode::Insert && (control || chord.modifiers == KeyModifiers::SHIFT) {
        return None;
    }

    // Closing a terminal, which is the one place Obelus names a command
    // with shift. In a terminal of its own every plain control letter is
    // the program's -- `ctrl+w` is the shell's word rubbed out -- and the
    // key every terminal closes a tab with is this one, so it is the key a
    // reader already has. Where a terminal cannot tell it from `ctrl+w`
    // (one that does not speak the keyboard protocol) it arrives as that,
    // goes to the program, and the palette is how to close it: a key that
    // falls on the shell's side of the line, rather than on Obelus's.
    if chord
        == KeyChord::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )
    {
        return None;
    }

    // Two of them, or shift with another: shift extends and reverses and
    // names nothing, and a second modifier is either the desktop's
    // (`ctrl+alt`) or something only a terminal speaking the keyboard
    // protocol can report (`ctrl+shift`).
    if !alone && !control && !alt {
        return Some("Shift extends, and two modifiers is the desktop's");
    }

    match chord.code {
        // The keys that move about a file. The editor takes them before the
        // table is reached -- bare and with shift -- and the rest are
        // spoken for: `ctrl` and an arrow is the word motion Obelus does
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
            Some("The editor's own, for moving about a file")
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => None,
        // A function key, which is the one family that wants nothing held.
        KeyCode::F(_) if alone => None,
        KeyCode::F(_) => Some("A function key is bare, or it is two keys on the next terminal"),
        // Enter under alt is the only one of these that is a chord: the
        // rest are what a dialog takes and what typing will mean.
        KeyCode::Enter if alt => None,
        KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Backspace | KeyCode::Delete => {
            Some("Every list and box takes this one itself")
        }
        // Escape has one meaning everywhere: give up on the nearest thing.
        KeyCode::Esc => Some("Escape always backs out of the nearest thing"),
        KeyCode::Char(_) if alone => Some("Typing, not a command"),
        // `ctrl+shift+p` folds onto `ctrl+P`, and a control byte cannot
        // carry a letter's case: only a terminal speaking the keyboard
        // protocol tells the two apart, so the binding would work on this
        // machine and not the next. Alt is different -- it is the escape
        // prefix, so `alt+P` really is the shifted letter.
        KeyCode::Char(character) if control && character.is_ascii_uppercase() => {
            Some("A control byte cannot say which case the letter was")
        }
        // The six the wire cannot tell from tab, enter, newline, backspace,
        // escape and NUL, whatever the reader pressed.
        KeyCode::Char(character) if control && "imjh[ 2".contains(character) => {
            Some("The terminal sends another key for this")
        }
        KeyCode::Char(_) => None,
        // The one key with a name and no family. Every terminal reports it
        // as itself and no other key folds onto it, it cannot be typing,
        // and the mode it turns on is the one it is named after -- so a
        // reader may bind it, and Obelus binds it by default.
        KeyCode::Insert if alone => None,
        _ => Some("Not a key Obelus can be given"),
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

/// The key that opens the card of every key, in every view that has one.
///
/// `ctrl+k`, on the letter of the word like the rest of the control family.
/// It was `f1`, which has meant help for longer than any of this -- until a
/// function key in a view went to the view it names, and `f1` names the
/// files. `?` cannot be it: the notes, the settings and every picker take
/// each character typed.
///
/// One definition, because it is answered in four places -- each view that
/// has a card, and the foot that says which key opens it -- and a foot
/// naming one key while the view listens for another is a card nobody can
/// open.
#[must_use]
pub fn keys_card() -> KeyChord {
    control('k')
}

/// Whether a key is [`keys_card`], for a view deciding whether to open its
/// card.
#[must_use]
pub fn is_keys_card(event: &KeyEvent) -> bool {
    KeyChord::from_event(event) == Some(keys_card())
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

    /// Every binding Obelus ships with is one a reader could have made.
    ///
    /// The shipped table and the page that binds keys answer to the same
    /// function, so a default outside the families would be a key Obelus
    /// gives itself and refuses to the reader.
    #[test]
    fn every_default_binding_is_one_the_reader_could_make() {
        for binding in Keymap::new().bindings() {
            let chord = binding.chord;
            // Escape is the one key Obelus keeps and a reader cannot have:
            // the rule is about what may be *taken*, and what escape means
            // -- give up on the nearest thing -- is not negotiable.
            if chord.code == KeyCode::Esc {
                continue;
            }
            assert_eq!(
                super::why_not(chord),
                None,
                "{} is on {}, which Obelus would not let a reader bind",
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
            // `ctrl+b` is tmux's prefix, so Obelus does not ship it --
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

    /// The function keys are three banks of four with nothing missing.
    ///
    /// A gap would be a key that does nothing in the middle of a row of
    /// keys that do, and the banks are how the twelve are remembered.
    #[test]
    fn the_function_keys_are_a_bank_at_a_time() {
        let keymap = Keymap::new();
        for number in 1..=12 {
            assert!(
                keymap
                    .command_on(KeyChord::new(KeyCode::F(number), KeyModifiers::NONE))
                    .is_some(),
                "f{number} does nothing, in the middle of a bank that does"
            );
        }
        assert_eq!(
            keymap.command_on(super::control('t')),
            Some(obelus_command::Command::PreviewToggle)
        );
        // The third bank is git's for three of its four: the same subject
        // at three widths -- this file, the project, this line.
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(9), KeyModifiers::NONE)),
            Some(obelus_command::Command::HistoryFile)
        );
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(10), KeyModifiers::NONE)),
            Some(obelus_command::Command::HistoryProject)
        );
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(11), KeyModifiers::NONE)),
            Some(obelus_command::Command::HistoryLine)
        );
        // And the fourth is not git's. It was held for a fourth question
        // about a history and none came -- three widths is what a history
        // has -- so it went to the jump this program was written for, which
        // is also where every editor a reader arrives from puts it.
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(12), KeyModifiers::NONE)),
            Some(obelus_command::Command::SymbolDefinition)
        );
    }

    /// The context every command with no default binding would land in.
    #[test]
    fn a_command_with_no_key_is_bound_where_the_reader_is_reading() {
        // Any chord the table has not spoken for. `alt+z` is one because
        // nothing wants it, which is the whole requirement -- this is about
        // where a new binding lands, not about which key it is.
        let free = KeyChord::new(KeyCode::Char('z'), KeyModifiers::ALT);
        let mut keymap = Keymap::new();
        assert!(
            keymap.command_on(free).is_none(),
            "the key this test borrows is bound, and it borrows a free one"
        );
        keymap.rebind(obelus_command::Command::ThemeSelect, Some(free));
        assert_eq!(
            keymap.lookup(
                &crossterm::event::KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT),
                Context::Normal
            ),
            Some(obelus_command::Command::ThemeSelect)
        );
    }
}
