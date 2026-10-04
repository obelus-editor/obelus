//! A line of typing on the status bar.
//!
//! Not a picker. A picker is a *list* with a query over it, and its layout,
//! its selection and its preview all exist to serve the list; a prompt has
//! nothing to list. Asking for a line number through a picker means a region
//! of screen holding one row that says "type a line number", which is a list
//! of nothing pretending to be a hint.
//!
//! What a prompt owns is what it needs: a label, what has been typed, and
//! what the answer is for. It draws on the status row and nowhere else, so
//! nothing it does covers the code.

use crossterm::event::{KeyCode, KeyEvent};

use crate::field::Field;

/// What a prompt is asking for.
///
/// One variant per question, so the application can tell what it is being
/// told without the prompt having to know what any of it means.
///
/// Searching a file is the next one, and it is why the two things that
/// differ per question -- the label and which characters are allowed -- are
/// methods on this rather than fields of the prompt: a search takes any
/// character and says `find: `, and that is the whole of the difference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    /// A line to go to.
    Line,
    /// A new name for the symbol under the cursor.
    Name,
    /// Where a file should be instead: a path rather than a name, so that
    /// the one question moves a file as well as renames it.
    Path,
    /// Where a file that is not there yet should go.
    ///
    /// Its own kind rather than [`PromptKind::Path`] with another label,
    /// because what the answer *means* is the thing that differs: one
    /// moves a file and the other makes one, and the row the reader
    /// answers on is the only place that says which.
    NewPath,
    /// What one of a chat platform's fields is: a token, an id.
    ///
    /// A secret is drawn with its middle hidden. Not all of it: the start
    /// says which token was pasted into which row -- the commonest way to
    /// get this wrong -- and the end says whether it was the one meant; the
    /// rest is the part a reader sharing their screen would not want shown.
    Told(&'static obelus_remote::platform::Field),
    /// What one of Obelus's own settings is, where it is typed rather than
    /// chosen: by the setting's key.
    Setting(&'static str),
}

impl PromptKind {
    /// What to write in front of the answer.
    ///
    /// Part of the question: a bare caret on the status bar says something
    /// is being asked without saying what.
    #[must_use]
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(match self {
            Self::Told(field) => return format!("{}: ", field.name).into(),
            Self::Setting(key) => {
                let name = obelus_config::Setting::named(key).map_or(key, |setting| setting.name);
                return format!("{name}: ").into();
            }
            Self::Line => "Line: ",
            Self::Name => "Rename to: ",
            // Not "Rename to: ", which is what a symbol's prompt says:
            // two prompts with the same words on the same row are one
            // prompt as far as a reader glancing at it is concerned, and
            // these two change very different things.
            Self::Path => "Call it: ",
            // The same argument once more, and this pair is the closer
            // one: both answers are a path, and only the words say
            // whether the file at the end of it is being moved or made.
            Self::NewPath => "New file: ",
        })
    }

    /// Whether a character belongs in the answer.
    ///
    /// A line number is digits. Refusing everything else as it is typed is
    /// better than accepting it and complaining afterwards: the reader finds
    /// out at the keystroke rather than at the answer, and there is no way
    /// to leave a prompt holding something it will not take.
    ///
    /// Per kind, because this is the part that differs: searching a file
    /// will accept anything that can be typed.
    #[must_use]
    pub const fn accepts(self, character: char) -> bool {
        match self {
            Self::Line => character.is_ascii_digit(),
            // Whatever a name can be, which differs by language and is the
            // server's to judge: it is the one who will refuse. What is
            // refused here is only what cannot be part of any name --
            // a newline is the key that answers, and a blank is the reader
            // having typed nothing.
            Self::Name => !character.is_whitespace(),
            // A path may hold anything a file name may, blanks included:
            // `My Notes.md` is a file, and a reader typing one is not
            // making a mistake. Only the key that answers is refused.
            Self::Path | Self::NewPath => character != '\n' && character != '\r',
            // A token or an id is one word, and a paste that brought a
            // newline or a space along with it should not keep them.
            Self::Told(_) => !character.is_whitespace(),
            // A name may have a blank in it -- a reader may well want the
            // agent to be "the assistant" -- and only the key that answers
            // is refused.
            Self::Setting(_) => character != '\n' && character != '\r',
        }
    }

    /// The same rule, as the line's own.
    ///
    /// A line takes a function rather than an enum, because the rules do
    /// not fall into kinds: the next question asked will have its own.
    #[must_use]
    pub const fn accepts_fn(self) -> crate::field::Accepts {
        match self {
            Self::Line => |character| character.is_ascii_digit(),
            Self::Name => |character| !character.is_whitespace(),
            Self::Path | Self::NewPath => |character| character != '\n' && character != '\r',
            Self::Told(_) => |character| !character.is_whitespace(),
            Self::Setting(_) => |character| character != '\n' && character != '\r',
        }
    }
}

/// A secret with its middle hidden: as much of its start as the platform's
/// prefix is long, and its last four.
///
/// The start whether or not it is the prefix this row wants: a token's
/// prefix says which kind it is and nothing else, and a bot token pasted
/// into the app token's row is found by reading those few characters.
///
/// One `•` for every character hidden, so a row drawing it is as wide as a
/// row drawing the secret, and a caret measured against either lands in the
/// same place.
#[must_use]
pub fn hidden(said: &str, looks_like: &str) -> String {
    let characters: Vec<char> = said.chars().collect();
    let start = looks_like.chars().count().min(characters.len());
    let end = characters.len().saturating_sub(4).max(start);
    characters
        .iter()
        .enumerate()
        .map(|(at, character)| match at >= start && at < end {
            true => '\u{2022}',
            false => *character,
        })
        .collect()
}

/// What came of a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptOutcome {
    /// Not a key the prompt knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// Answered.
    Accepted(String),
    /// Given up on.
    Cancelled,
}

/// A question on the status bar.
#[derive(Clone, Debug)]
pub struct Prompt {
    kind: PromptKind,
    text: Field,
}

impl Prompt {
    /// Asks something.
    #[must_use]
    pub fn new(kind: PromptKind) -> Self {
        Self {
            kind,
            text: Field::taking(kind.accepts_fn()),
        }
    }

    /// Asks something with an answer already in it.
    ///
    /// A rename starts at the name being renamed: it is what the reader is
    /// changing, most renames are an edit of it rather than a new word,
    /// and a blank prompt would make them type the whole thing again.
    #[must_use]
    pub fn about(kind: PromptKind, text: String) -> Self {
        Self {
            kind,
            text: Field::about(&text, kind.accepts_fn()),
        }
    }

    /// What it is asking for.
    #[must_use]
    pub const fn kind(&self) -> PromptKind {
        self.kind
    }

    /// What has been typed.
    #[must_use]
    pub fn text(&self) -> String {
        self.text.said()
    }

    /// Where the caret is in the answer, for whoever draws the row.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.text.caret().get()
    }

    /// Which characters of the answer the reader has hold of.
    #[must_use]
    pub fn held(&self) -> Option<std::ops::Range<usize>> {
        self.text.held()
    }

    /// Puts a run of text into the answer, which is what a paste is.
    pub fn put(&mut self, said: &str) {
        self.text.put(said);
    }

    /// Puts the answer's caret where a cell of its row is.
    pub fn place_at_cell(&mut self, cell: u16, extend: bool) {
        self.text.place_at_cell(cell, extend);
    }

    /// Takes hold of the word under the caret, or of the whole answer.
    pub fn hold(&mut self, all: bool) {
        match all {
            true => self.text.hold_all(),
            false => self.text.hold_word(),
        }
    }

    /// What a copy takes from the answer: what is held, or all of it.
    #[must_use]
    pub fn copied(&self) -> (String, &'static str) {
        self.text.copied()
    }

    /// The same, and takes it out.
    pub fn cut(&mut self) -> (String, &'static str) {
        self.text.cut()
    }

    /// The whole row: the label and the answer so far.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}{}", self.kind.label(), self.shown())
    }

    /// What has been typed, as the row draws it: a secret with its middle
    /// hidden, one hidden character for one typed so that the caret still
    /// lands where it is.
    #[must_use]
    pub fn shown(&self) -> String {
        let said = self.text.said();
        let PromptKind::Told(field) = self.kind else {
            return said;
        };
        let obelus_remote::platform::FieldKind::Secret { looks_like } = field.kind else {
            return said;
        };
        hidden(&said, looks_like)
    }

    /// Handles a key.
    ///
    /// Modifiers are judged the way every other path judges them, so
    /// `ctrl+q` still leaves Obelus from here and a stray `super` disqualifies
    /// a key rather than being ignored.
    pub fn handle_key(&mut self, key: &KeyEvent) -> PromptOutcome {
        let Some(modifiers) = obelus_editing::keymap::modifiers_of(key) else {
            return PromptOutcome::Ignored;
        };
        let bare = modifiers.is_empty();

        match key.code {
            KeyCode::Esc if bare => PromptOutcome::Cancelled,
            // An empty answer is not an answer: it is a reader pressing
            // enter on an empty prompt, which should do nothing rather than
            // something arbitrary.
            KeyCode::Enter if bare => {
                if self.text.is_empty() {
                    PromptOutcome::Consumed
                } else {
                    PromptOutcome::Accepted(self.text.said())
                }
            }
            // Everything else goes to the answer, which is a line with a
            // caret in it: the arrows, the words, what is held, what is
            // typed, and the question's own rule about which characters
            // belong in it. A key the line refuses is still a key it saw,
            // which is how `ctrl+q` reaches the key table and a stray
            // letter does not run a command from inside a question.
            _ => match self.text.handle_key(key) {
                true => PromptOutcome::Consumed,
                false => PromptOutcome::Ignored,
            },
        }
    }
}
