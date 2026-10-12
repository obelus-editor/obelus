//! Keys to commands.
//!
//! The table is data on the application, not a `static`. That is the
//! whole mechanism behind "the user can rebind keys": loading a table from a
//! file later replaces a constructor, not the lookup path.
//!
//! `keymap::why_not` is the one judgement of what may be bound. It is asked
//! by the page that binds keys, by the table read out of the config file, and
//! by the test that holds the shipped table to the same rule -- so Obelus
//! cannot give itself a key it refuses the reader, and a reason is written
//! once. A reader may have any key Obelus can be handed, whether or not the
//! next terminal can send it; what is refused is only what never reaches the
//! table at all, so a binding to it could not fire anywhere:
//!
//! * the arrows, `home`, `end` and the paging keys without `alt` -- the editor
//!   takes those before the table is reached, and the ones it does not take it
//!   has said it wants (`ctrl` and an arrow is a word motion, `ctrl` and a
//!   paging key is the previous and next buffer);
//! * a bare character, `enter`, `tab`, `backspace`, `delete` -- typing, and the
//!   keys every list and box takes itself;
//! * `escape`, which Obelus keeps: give up on the nearest thing is not
//!   negotiable, and it is the one default a reader cannot move.
//!
//! What Obelus *ships* is held to more than that, by the families below and
//! by reasons argued where each key is bound -- and by a test, because
//! `why_not` no longer says it (`what_obelus_ships_keeps_to_the_families`):
//! nothing with two modifiers, a function key bare, and `ctrl+shift` on a
//! letter only where another editor's layout borrows it -- with an alt chord
//! beside it, because a terminal without the keyboard protocol sends `ctrl` and
//! the letter, and one with it often keeps the chord for itself.
//!
//! `ctrl+b` belongs to tmux, so Obelus does not ship it -- a reader outside
//! tmux may still have it. `ctrl+a` is screen's prefix and is shipped anyway,
//! because "all of it" is what that key means in every program with a
//! selection.
//!
//! Keys are rebound on the keys page, and the file holds the changes. The
//! table is data on `App`, so a rebinding is `Keymap::rebind` plus a line in
//! the config's `[keys]` -- command *name* to its chords spelled out
//! (`ctrl+p`, or a list of them, the first being the one shown),
//! because an enum's spelling and a keycode are Obelus's business rather than
//! the reader's. What is in the file is a list of changes over the defaults, so
//! a reader who moved one key still gets the new default for everything else,
//! and a name or a chord Obelus cannot read is skipped with a word in the log.
//! Rebinding moves *every* binding of the command -- `close-file` is bound in
//! `Normal` and in `Buffers` and is still one command with its keys -- and a
//! command that had none gets them in `Normal`, which is where a key a reader
//! presses belongs. A chord already spoken for is refused on the row that asked
//! for it, with what has it: the row is where the reader is looking, the status
//! row there is the page's own filter, and a passing note would be cleared by
//! the very next keystroke.
//!
//! Which table the changes are laid over is a setting of its own,
//! `keys_from`, on the same page: the function keys ([`Layout::Obelus`]),
//! the same commands on control and alt ([`Layout::Mnemonic`]), or another
//! editor's keys ([`Layout::VsCode`], [`Layout::TextMate`]) -- adapted to
//! what Obelus has, not a copy of either.
//!
//! Modifiers are judged exactly, in one place. `modifiers_of` is the
//! only judge -- it lives in [`obelus_editing`], because the caret asks it too;
//! `SUPER`/`HYPER`/`META` disqualify a key rather than being masked
//! away. Masking meant `ctrl+super+q` quit. Nor is super ever read as
//! control: on Linux and Windows it is the desktop's.
//!
//! On macOS a window is told command where everything else is told control,
//! and `obg` reports one as the other before a key reaches this table. So
//! there is still one table, and it is drawn the way the reader presses it:
//! [`draw_control_as_command`]. Which is how every layout -- the editors'
//! included -- is on command in a window on a Mac and on control everywhere
//! else, without a binding of its own for either.

use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use obelus_command::Command;
/// Here as well as in [`obelus_editing`], where the motions ask it: the key
/// table and the caret draw the line in the same place because they ask the
/// same function.
pub use obelus_editing::{BINDABLE_MODIFIERS, modifiers_of};

mod shipped;

/// Whether control is drawn as command.
///
/// A global, as the glyph switch is: which key the reader holds for control
/// is a fact about the front end and the machine, decided once, and every
/// chord on the screen is drawn through [`KeyChord::label`] without being
/// handed it. Off unless `obg` on macOS turns it on, which is the one place
/// that also turns command into control on the way in -- the two halves have
/// to agree, or the foot names a key that does nothing.
static COMMAND: AtomicBool = AtomicBool::new(false);

/// Draws control as command from now on, or stops.
pub fn draw_control_as_command(on: bool) {
    COMMAND.store(on, Ordering::Relaxed);
}

/// Whether control is drawn as command.
#[must_use]
pub fn control_drawn_as_command() -> bool {
    COMMAND.load(Ordering::Relaxed)
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
        self.label_as(obelus_icons::enabled(), control_drawn_as_command())
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
    /// -- `Ctrl`, `F1`, `PageDown` -- and this is the spelling: it is what
    /// the screen shows and what `[keys]` in the settings file is written
    /// in, because two spellings of one name is somewhere for them to
    /// differ. The one exception is control in `obg` on macOS, which the
    /// screen calls `Cmd` ([`KeyChord::label_as`]) because that is the key
    /// pressed there; the file is shared with a terminal, where it is not,
    /// and `parse` reads `Cmd` back as well, so what the screen says is
    /// still something the file can say. An older file in the other case still
    /// reads: `parse` never minded, and that is what makes the change safe.
    ///
    /// A letter is written small, and shift is written as `Shift+`. A
    /// shifted letter is *held* as the capital -- it is how a chord with
    /// shift on a letter is normalised -- but what is written is read
    /// without regard to case, so `Ctrl+A` and `ctrl+a` are one key and
    /// `Ctrl+Shift+a` is the other.
    #[must_use]
    pub fn label_in(self, glyphs: bool) -> String {
        self.label_as(glyphs, false)
    }

    /// The same, with control written as command or not.
    ///
    /// Only the screen asks for command, and only from `obg` on macOS: the
    /// settings file is shared with `ob` in a terminal on the same machine,
    /// where the reader holds control, so it keeps the one spelling
    /// [`KeyChord::label_in`] writes. What the screen says still reads back
    /// -- [`KeyChord::parse`] takes `Cmd` for control -- so a reader who
    /// copies a chord off the foot into the file gets the key they saw.
    #[must_use]
    pub fn label_as(self, glyphs: bool, command: bool) -> String {
        let control = match command {
            true => ("Cmd", obelus_icons::key::COMMAND),
            false => ("Ctrl", obelus_icons::key::CONTROL),
        };
        // A shifted letter is held as the capital (`KeyChord::new`), and is
        // written as shift and the small letter: what is written is read
        // without regard to case, so `Ctrl+P` could not say it.
        let (code, modifiers) = match self.code {
            KeyCode::Char(character) if character.is_ascii_uppercase() => (
                KeyCode::Char(character.to_ascii_lowercase()),
                self.modifiers | KeyModifiers::SHIFT,
            ),
            code => (code, self.modifiers),
        };
        let mut label = String::new();
        for (modifier, name, glyph) in [
            (KeyModifiers::CONTROL, control.0, control.1),
            (KeyModifiers::ALT, "Alt", obelus_icons::key::ALT),
            (KeyModifiers::SHIFT, "Shift", obelus_icons::key::SHIFT),
        ] {
            if modifiers.contains(modifier) {
                if glyphs {
                    label.push(glyph);
                    label.push(' ');
                } else {
                    label.push_str(name);
                    label.push('+');
                }
            }
        }

        let named = match code {
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
        match (named, code) {
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
    /// a chord written into the config file reads back as itself -- and of
    /// [`KeyChord::label_as`] with command, so one copied off the screen of
    /// a Mac does too. Spelled
    /// rather than drawn: a file is typed into by hand, and `ctrl+p` is
    /// something a reader can type where a private-use codepoint is not.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut modifiers = KeyModifiers::NONE;
        let mut rest = text.trim();
        // Asked a byte at a time with `get` rather than sliced: a chord is
        // often a mark after its modifier -- `Alt+\u{2191}` is how Obelus
        // writes one -- and slicing five bytes into that cuts the arrow in
        // half, which panicked on the reader's own settings file.
        let after = |rest: &'_ str, name: &str| -> Option<usize> {
            let head = rest.get(..name.len())?;
            (head.eq_ignore_ascii_case(name) && rest.len() > name.len()).then_some(name.len())
        };
        'modifiers: loop {
            for (name, modifier) in [
                ("ctrl+", KeyModifiers::CONTROL),
                // What the screen calls control in `obg` on macOS -- see
                // `label_as`.
                ("cmd+", KeyModifiers::CONTROL),
                ("alt+", KeyModifiers::ALT),
                ("shift+", KeyModifiers::SHIFT),
            ] {
                if let Some(length) = after(rest, name) {
                    modifiers |= modifier;
                    rest = &rest[length..];
                    continue 'modifiers;
                }
            }
            break;
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
            // Written by `label_in` as the key's own name, so it has to read
            // back: `replace-toggle` and the desktop's paste are on it.
            "insert" => KeyCode::Insert,
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
                // A letter in either case is the letter: shift is said with
                // `shift+`, the way `label_in` writes it, so `Ctrl+T` and
                // `ctrl+t` are one key. Anything else is the character as it
                // was written -- `?` is not `/`.
                None => {
                    let mut characters = rest.chars();
                    let character = characters.next()?;
                    if characters.next().is_some() {
                        return None;
                    }
                    KeyCode::Char(character.to_ascii_lowercase())
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
    /// `ctrl+w` to close it as anything else is closed, the key that leaves
    /// Obelus, and the function
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

    /// The context a layout's file names, in the word it is written in there.
    fn named(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "normal" => Self::Normal,
            "documents" => Self::Documents,
            "chat" => Self::Chat,
            "terminal" => Self::Terminal,
            "dialog" => Self::Dialog,
            _ => return None,
        })
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

/// Where the keys that open something to look at are.
///
/// The function keys are a row some keyboards do not have and some laptops
/// put behind `fn`, so a reader can start from a table with none: the same
/// twelve commands on control and alt, by the letter of the word -- or on
/// the keys another editor taught them, where it has the same command
/// ([`Keymap::laid_out`]). None of the last three has a function key.
/// A choice of where to *start*, and nothing more -- the reader's own
/// `[keys]` are laid over whichever it is, the way they are laid over the
/// shipped table, so switching keeps them.
///
/// The words are `obelus_config::KEY_LAYOUTS`, and each one's name and what
/// it is are `obelus_config::key_layout`'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Obelus's own keys: the function keys, in their banks of four.
    #[default]
    Obelus,
    /// Control and alt, on the letter of the word.
    Mnemonic,
    /// VS Code's keys, where Obelus has the command: `ctrl+p` the files,
    /// `ctrl+shift+p` the palette.
    VsCode,
    /// TextMate's: `ctrl+t` the files, `ctrl+shift+t` this file's symbols.
    TextMate,
}

impl Layout {
    /// Every layout, in the order a list offers them.
    pub const ALL: [Self; 4] = [Self::Obelus, Self::Mnemonic, Self::VsCode, Self::TextMate];

    /// What it is called in the settings file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Obelus => "obelus",
            Self::Mnemonic => "mnemonic",
            Self::VsCode => "vscode",
            Self::TextMate => "textmate",
        }
    }

    /// The layout the file's word names, if one does.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.name() == name)
    }
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
    /// this whole program is for. Each is argued where it is bound, in
    /// `keymaps/obelus.json`.
    ///
    /// **Control does something to the file in front of you**, on the
    /// letter of the word: `p` the palette, `w` close, `r` re-read, `d`
    /// display it the way its format reads, `t` the notes, `l` a line
    /// number, `a` all of it, `c` copy, `q` leave. `ctrl+v` was left alone
    /// until there was a paste to give it, and that is the one chord every
    /// reader will try there.
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
    /// [`why_not`]. The editors' layouts borrow chords with shift in them,
    /// and they are those editors' names rather than Obelus's
    /// ([`Self::laid_out`]).
    ///
    /// Everything else is reached from the palette. A chord for every
    /// command is how a key table stops being memorable, and most of what
    /// is left -- the theme, the log, restarting a server -- is done once
    /// and not again.
    #[must_use]
    pub fn new() -> Self {
        Self::laid_out(Layout::Obelus)
    }

    /// The bindings a layout starts from, as its file in `keymaps/` says.
    ///
    /// The tables are data and live there, each binding with the reason for
    /// it beside it: [`Layout::Mnemonic`] is [`Self::new`] with the twelve
    /// function keys moved onto letters, and the two editors' layouts move
    /// the same twelve and the palette onto those editors' keys -- on
    /// control, which is Command in `obg` on a Mac
    /// ([`draw_control_as_command`]), with an alt chord beside each one with
    /// shift in it for a terminal that cannot send it.
    #[must_use]
    pub fn laid_out(layout: Layout) -> Self {
        Self::from_bindings(shipped::table(layout).to_vec())
    }

    /// Binds a chord in one context, taking it off whatever had it there.
    fn put(&mut self, command: Command, context: Context, chord: KeyChord) {
        self.bindings
            .retain(|binding| binding.chord != chord || binding.context != context);
        self.bindings.push(Binding {
            command,
            context,
            chord,
        });
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

    /// Which command a chord runs, wherever it is bound: the first that has
    /// it, in whatever context.
    ///
    /// What a table put on a key, asked by tests of the shipped layouts.
    /// The page that binds keys asks the same question of the keys it was
    /// last told of (`obelus_component::settings::Settings::hear_keymap`).
    #[must_use]
    pub fn command_on(&self, chord: KeyChord) -> Option<Command> {
        self.bindings
            .iter()
            .find(|binding| binding.chord == chord)
            .map(|binding| binding.command)
    }

    /// Puts a command on these keys and no others, or takes every key
    /// away with none.
    ///
    /// Every binding of it, because a command bound in two contexts is one
    /// command with its keys: `close-file` closes the file being read and
    /// the file on the row of a list, and a reader who rebinds it means
    /// both. A command that had no key gets them where the reader is
    /// reading, which is where a key they press belongs.
    ///
    /// The whole list, in order, because the first is the one every other
    /// place shows ([`Self::chord_for`]) and the page that binds keys shows
    /// them all -- so a key the reader did not choose cannot stay firing
    /// behind the ones they can see.
    pub fn rebind(&mut self, command: Command, chords: &[KeyChord]) {
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
        let contexts = match contexts.is_empty() {
            true => vec![Context::Normal],
            false => contexts,
        };
        for chord in chords {
            for context in &contexts {
                self.bindings.push(Binding {
                    command,
                    context: *context,
                    chord: *chord,
                });
            }
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
    ///
    /// Laid over the layout the reader chose, and a line of theirs beats
    /// it: a chord the file gives one command is taken off whatever the
    /// layout put there. Otherwise switching layouts could put a layout's
    /// key over one the reader had chosen, and the first binding in the
    /// table is the one that fires.
    ///
    /// Taken off only where the reader's command is bound: a line putting a
    /// file's command on `ctrl+p` is no reason for a terminal, where that
    /// command cannot run, to lose its way to the palette.
    ///
    /// A line may name several keys, and the command is on those and no
    /// others; one that cannot be read or given is said, and the rest are
    /// bound. A line all of whose keys are wrong leaves the command where
    /// the layout put it, rather than on nothing.
    #[must_use]
    pub fn with(
        layout: Layout,
        bindings: &std::collections::BTreeMap<String, Vec<String>>,
    ) -> (Self, Vec<Unbound>) {
        let mut keymap = Self::laid_out(layout);
        let mut unbound = Vec::new();
        let skipped = |name: &String, text: &String, why: Unbindable| Unbound {
            name: name.clone(),
            text: text.clone(),
            why,
        };
        for (name, texts) in bindings {
            let Some(command) = obelus_command::by_name(name) else {
                tracing::warn!(name, "no command by that name to bind");
                unbound.push(skipped(name, &texts.join(", "), Unbindable::NoSuchCommand));
                continue;
            };
            // No keys is the reader having taken them away, which is a
            // decision and not a mistake.
            if texts.is_empty() {
                keymap.rebind(command, &[]);
                continue;
            }
            let mut chords = Vec::new();
            for text in texts {
                let Some(chord) = KeyChord::parse(text) else {
                    tracing::warn!(name, text, "not a key Obelus can read");
                    unbound.push(skipped(name, text, Unbindable::Unreadable));
                    continue;
                };
                // The same judgement the page that binds keys makes. A
                // chord that cannot fire is worse in a file than on a page:
                // there is nothing on screen to say why the key does nothing.
                if let Some(why) = why_not(chord) {
                    tracing::warn!(name, text, why, "not a key Obelus can be given");
                    unbound.push(skipped(name, text, Unbindable::NotAllowed(why)));
                    continue;
                }
                if !chords.contains(&chord) {
                    chords.push(chord);
                }
            }
            if chords.is_empty() {
                continue;
            }
            keymap.rebind(command, &chords);
            let contexts: Vec<Context> = keymap
                .bindings
                .iter()
                .filter(|binding| binding.command == command)
                .map(|binding| binding.context)
                .collect();
            keymap.bindings.retain(|binding| {
                !chords.contains(&binding.chord)
                    || binding.command == command
                    || !contexts.contains(&binding.context)
            });
        }
        (keymap, unbound)
    }

    /// Every key a command is on, each once, in the order the table has
    /// them: what the page that binds keys lists, the first of them being
    /// [`Self::chord_for`]'s.
    #[must_use]
    pub fn chords_for(&self, command: Command) -> Vec<KeyChord> {
        let mut chords = Vec::new();
        for binding in &self.bindings {
            if binding.command == command && !chords.contains(&binding.chord) {
                chords.push(binding.chord);
            }
        }
        chords
    }

    /// The key bound to a command, if one is: the first of its keys, which
    /// is the one every place that shows a single key shows.
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
/// rule.
///
/// A reader may put a command on any key Obelus can be handed, whether or not
/// the next terminal can send it -- a window can, a terminal speaking the
/// keyboard protocol mostly can, and which is the reader's to know. What is
/// refused is only what Obelus itself never hands the table, so that a
/// binding to it could not fire anywhere:
///
/// * a character with nothing held, which is typing;
/// * `escape`, which always gives up on the nearest thing;
/// * `enter`, `tab`, `backspace` and `delete`, which every list and box takes
///   itself -- all but `alt+enter`, which nothing takes;
/// * the arrows, `home`, `end` and the paging keys without `alt`, which the
///   editor takes before the table is reached, or has said it wants.
#[must_use]
pub fn why_not(chord: KeyChord) -> Option<&'static str> {
    let alone = chord.modifiers.is_empty();
    let alt = chord.modifiers.contains(KeyModifiers::ALT);
    match chord.code {
        // The keys that move about a file. The editor takes them before the
        // table is reached -- bare and with shift -- and the rest are
        // spoken for: `ctrl` and left or right is the word motion, `ctrl`
        // and up or down scrolls the view a row (or what a list names,
        // under it), and `ctrl` and a paging key pages a list over what it
        // names. With alt held they are how changes and history are
        // walked, and free.
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
        // Enter under alt is the only one of these that is a chord: the
        // rest are what a dialog takes and what typing will mean.
        KeyCode::Enter if chord.modifiers == KeyModifiers::ALT => None,
        KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Backspace | KeyCode::Delete => {
            Some("Every list and box takes this one itself")
        }
        // Escape has one meaning everywhere: give up on the nearest thing.
        KeyCode::Esc => Some("Escape always backs out of the nearest thing"),
        KeyCode::Char(_) if alone => Some("Typing, not a command"),
        // Copy and paste, under the names the desktop knows them by --
        // `shift+Insert` and `ctrl+Insert` are shipped, and are not Obelus
        // naming a command with shift. These two are the chords a terminal
        // has meant by copy and paste since long before any of this, and
        // they are what a desktop sends a *terminal* when the reader
        // presses its own one chord for copy: omarchy's `super+c` is
        // `ctrl+Insert` where it thinks it is talking to a terminal and
        // `ctrl+c` where it does not, and `super+v` is `shift+Insert` or
        // `ctrl+v` the same way. Obelus is both and is read as either, so it
        // answers all four, and which of the two it is taken for stops
        // mattering. One table, so binding it once binds it for `ob` and
        // `obg` together. The same shape as `shift+enter` and `alt+enter`
        // being taken together: one act, two chords, because what arrives
        // is not Obelus's to decide.
        //
        // No terminal shipped here binds them for itself: foot, kitty,
        // alacritty and wezterm all put copy on `ctrl+shift+c`. A terminal
        // that is *configured* to take them keeps them and Obelus never
        // sees them -- which is what a desktop does to make its own chord
        // land somewhere in a shell (omarchy adds them to foot's bindings
        // for exactly that), and is the one case this cannot reach.
        // Measured, by pressing it at `ob` in such a terminal and at `ob` in
        // the same terminal with that one binding turned off: the first
        // copies nothing, the second copies the selection.
        //
        // And the two halves come apart there. With the terminal holding
        // both keys, paste still works and copy cannot, because pasting is
        // something a terminal can do on behalf of the program inside it and
        // copying is not: it puts the words down the pty, where they arrive
        // as an ordinary bracketed paste. Copy it cannot do, because what is
        // selected is the program's and the terminal does not know -- and
        // `ob` has taken the mouse, so the selection the terminal *would*
        // copy is empty. The key does nothing at all. Which is the shape of
        // the whole problem: a desktop can tell a terminal from a window,
        // and nothing can tell a shell from a program that has taken the
        // terminal over. Not something Obelus can answer from the inside --
        // the terminal decides before Obelus is asked, and decides
        // unconditionally.
        //
        // And bare, the one key with a name and no family: every terminal
        // reports it as itself, it cannot be typing, and the mode it turns
        // on is the one it is named after.
        KeyCode::Insert => None,
        KeyCode::Char(_)
        | KeyCode::F(_)
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown => None,
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

/// A chord for `ctrl` plus a character.
#[must_use]
pub fn control(character: char) -> KeyChord {
    KeyChord::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers};

    use super::{Context, KeyChord, Keymap, Layout};

    /// What Obelus ships is held to more than what a reader may bind: the
    /// families, on keys every terminal sends as themselves.
    ///
    /// `why_not` lets a reader have a chord the next terminal cannot send,
    /// because which terminal is theirs to know; a default cannot know it.
    /// So a shipped key has at most one modifier -- shift only on `Insert`,
    /// where it is the desktop's paste -- a function key is bare, control
    /// is never on one of the seven keys the wire cannot tell from another
    /// (`i`, `m`, `j`, `h`, `[`, space and `2`), and shift on a letter --
    /// under control or under alt, where it is folded into the capital --
    /// is only the editors' layouts', each control one with an alt chord
    /// beside it (`every_shifted_chord_has_one_every_terminal_can_send`).
    ///
    /// Broken deliberately by giving redo `shift+f5` in vscode.json, and
    /// apart by giving it `ctrl+i`, and apart by giving it `alt+shift+x` in
    /// obelus.json: each failed here and passed `why_not`.
    #[test]
    fn what_obelus_ships_keeps_to_the_families() {
        for layout in Layout::ALL {
            for binding in Keymap::laid_out(layout).bindings() {
                let chord = binding.chord;
                let (code, modifiers) = (chord.code, chord.modifiers);
                let name = || {
                    format!(
                        "{} on {} in {layout:?}",
                        binding.command.name(),
                        chord.label_in(false)
                    )
                };
                let one = modifiers.is_empty()
                    || modifiers == KeyModifiers::CONTROL
                    || modifiers == KeyModifiers::ALT
                    || (modifiers == KeyModifiers::SHIFT && code == KeyCode::Insert);
                assert!(
                    one,
                    "{}: two modifiers, or shift on a key it names nothing on",
                    name()
                );
                if let KeyCode::F(_) = code {
                    assert!(
                        modifiers.is_empty(),
                        "{}: a function key with something held",
                        name()
                    );
                }
                if let KeyCode::Char(character) = code
                    && modifiers == KeyModifiers::CONTROL
                {
                    assert!(
                        !"imjh[ 2".contains(character),
                        "{}: the wire sends another key for it",
                        name()
                    );
                }
                // Shift on a letter names no command of Obelus's own: a
                // capital is only another editor's chord, borrowed.
                if let KeyCode::Char(character) = code
                    && character.is_ascii_uppercase()
                    && !modifiers.is_empty()
                {
                    assert!(
                        matches!(layout, Layout::VsCode | Layout::TextMate),
                        "{}: shift on a letter, outside another editor's layout",
                        name()
                    );
                }
            }
        }
    }

    /// Every binding Obelus ships with is one a reader could have made.
    ///
    /// The shipped table and the page that binds keys answer to the same
    /// function, so a default outside the families would be a key Obelus
    /// gives itself and refuses to the reader. Every layout: broken
    /// deliberately by refusing control with a capital in `why_not`, as it
    /// was before the editors' layouts, and `ctrl+P` was a key Obelus
    /// shipped and refused.
    #[test]
    fn every_default_binding_is_one_the_reader_could_make() {
        let tables = Layout::ALL.map(Keymap::laid_out);
        for binding in tables.iter().flat_map(Keymap::bindings) {
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

    /// What never reaches the table is refused, with a reason, and what
    /// only some terminals send is not.
    ///
    /// Every refused one is a key that would do nothing anywhere -- the
    /// same as a binding that silently never fires. The ones a terminal may
    /// not send are the reader's to know: a window sends them all.
    ///
    /// Broken deliberately by refusing two modifiers again, as `why_not`
    /// did: `ctrl+alt+x` was refused.
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
            (KeyCode::Esc, KeyModifiers::CONTROL),
            (KeyCode::Tab, KeyModifiers::CONTROL),
            (KeyCode::Enter, KeyModifiers::CONTROL),
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
            (KeyCode::Home, KeyModifiers::ALT),
            // What some terminal will not send, or will send as another
            // key: a window sends every one of them.
            (KeyCode::F(5), KeyModifiers::SHIFT),
            (
                KeyCode::Char('x'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            (
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            (KeyCode::Char('i'), KeyModifiers::CONTROL),
            (KeyCode::Char('['), KeyModifiers::CONTROL),
            (KeyCode::Char(' '), KeyModifiers::CONTROL),
            (KeyCode::Insert, KeyModifiers::ALT),
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
            keymap.command_on(super::control('d')),
            Some(obelus_command::Command::PreviewToggle)
        );
        // The third bank opens with git's history at two widths -- this
        // file, the project.
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(9), KeyModifiers::NONE)),
            Some(obelus_command::Command::HistoryFile)
        );
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(10), KeyModifiers::NONE)),
            Some(obelus_command::Command::HistoryProject)
        );
        // Then the jump this program was written for.
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(11), KeyModifiers::NONE)),
            Some(obelus_command::Command::SymbolDefinition)
        );
        // And what is on its way into the history: the pull requests, and
        // the issues beside them.
        assert_eq!(
            keymap.command_on(KeyChord::new(KeyCode::F(12), KeyModifiers::NONE)),
            Some(obelus_command::Command::PullRequestReview)
        );
    }

    /// The mnemonic layout has every command the function keys open, and
    /// on keys of its own: none of them a function key, and none of them a
    /// key something else is already on.
    ///
    /// Broken deliberately by putting `search-symbols` on `alt+u`, the key
    /// the outline is on, and by not rebinding at all in `laid_out`.
    #[test]
    fn the_mnemonic_layout_moves_the_function_keys_onto_letters() {
        let obelus = Keymap::laid_out(Layout::Obelus);
        let mnemonic = Keymap::laid_out(Layout::Mnemonic);
        for number in 1..=12 {
            let function = KeyChord::new(KeyCode::F(number), KeyModifiers::NONE);
            let command = obelus.command_on(function).expect("a bank with a gap");
            assert_eq!(
                mnemonic.command_on(function),
                None,
                "f{number} is still bound"
            );
            let chord = mnemonic
                .chord_for(command)
                .unwrap_or_else(|| panic!("{} lost its key", command.name()));
            assert_eq!(
                mnemonic.command_on(chord),
                Some(command),
                "{} is on {}, which {} is on first",
                command.name(),
                chord.label_in(false),
                mnemonic
                    .command_on(chord)
                    .map_or("nothing", |other| other.name()),
            );
        }
        // And nothing else moved: every key the obelus table has that is
        // not a function key means the same in the mnemonic one, in the
        // same context -- `ctrl+p` is bound in two of them.
        for binding in obelus.bindings() {
            if matches!(binding.chord.code, KeyCode::F(_)) {
                continue;
            }
            assert_eq!(
                mnemonic.find(binding.chord, binding.context),
                Some(binding.command),
                "{} moved in {:?}",
                binding.chord.label_in(false),
                binding.context
            );
        }
    }

    /// A key the reader gave a command in their own file is that command's,
    /// whatever the layout under it had put there.
    ///
    /// Broken deliberately by leaving the layout's binding where it was in
    /// `Keymap::with`: `ctrl+e` still opens the list of what is open,
    /// because that binding comes first.
    #[test]
    fn a_line_of_the_readers_beats_the_layout() {
        let lines = [("choose-theme".to_string(), vec!["ctrl+e".to_string()])]
            .into_iter()
            .collect();
        let (keymap, unbound) = Keymap::with(Layout::Mnemonic, &lines);
        assert!(unbound.is_empty(), "{unbound:?}");
        assert_eq!(
            keymap.lookup(
                &crossterm::event::KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                Context::Normal
            ),
            Some(obelus_command::Command::ThemeSelect)
        );
        assert_eq!(
            keymap.chord_for(obelus_command::Command::DocumentList),
            None
        );
    }

    /// A line of the reader's takes its chord only where its own command is
    /// bound, so what the chord means anywhere else stays.
    ///
    /// Broken deliberately by taking the chord off every other command in
    /// every context, which is what `Keymap::with` first did: a terminal
    /// lost its way to the palette to a command it cannot run.
    #[test]
    fn a_line_of_the_readers_takes_its_chord_only_where_it_is_bound() {
        let lines = [("choose-theme".to_string(), vec!["ctrl+p".to_string()])]
            .into_iter()
            .collect();
        let control_p = crossterm::event::KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        for layout in [Layout::Obelus, Layout::Mnemonic] {
            let (keymap, _) = Keymap::with(layout, &lines);
            assert_eq!(
                keymap.lookup(&control_p, Context::Normal),
                Some(obelus_command::Command::ThemeSelect),
                "{layout:?}"
            );
            assert_eq!(
                keymap.lookup(&control_p, Context::Terminal),
                Some(obelus_command::Command::CommandPalette),
                "{layout:?}"
            );
        }
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
        keymap.rebind(obelus_command::Command::ThemeSelect, &[free]);
        assert_eq!(
            keymap.lookup(
                &crossterm::event::KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT),
                Context::Normal
            ),
            Some(obelus_command::Command::ThemeSelect)
        );
    }
}
