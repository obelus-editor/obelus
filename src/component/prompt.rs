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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
}

impl PromptKind {
    /// What to write in front of the answer.
    ///
    /// Part of the question: a bare caret on the status bar says something
    /// is being asked without saying what.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Line => "line: ",
            Self::Name => "rename to: ",
        }
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
        }
    }
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
    text: String,
}

impl Prompt {
    /// Asks something.
    #[must_use]
    pub const fn new(kind: PromptKind) -> Self {
        Self {
            kind,
            text: String::new(),
        }
    }

    /// Asks something with an answer already in it.
    ///
    /// A rename starts at the name being renamed: it is what the reader is
    /// changing, most renames are an edit of it rather than a new word,
    /// and a blank prompt would make them type the whole thing again.
    #[must_use]
    pub fn about(kind: PromptKind, text: String) -> Self {
        Self { kind, text }
    }

    /// What it is asking for.
    #[must_use]
    pub const fn kind(&self) -> PromptKind {
        self.kind
    }

    /// What has been typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The whole row: the label and the answer so far.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}{}", self.kind.label(), self.text)
    }

    /// Handles a key.
    ///
    /// Modifiers are judged the way every other path judges them, so
    /// `ctrl+q` still leaves obelus from here and a stray `super` disqualifies
    /// a key rather than being ignored.
    pub fn handle_key(&mut self, key: &KeyEvent) -> PromptOutcome {
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
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
                    PromptOutcome::Accepted(self.text.clone())
                }
            }
            KeyCode::Backspace if bare => {
                self.text.pop();
                PromptOutcome::Consumed
            }
            // Only a bare or shifted character is text, the same rule the
            // picker's query follows -- and only a character this question
            // has any use for.
            KeyCode::Char(character) if (modifiers - KeyModifiers::SHIFT).is_empty() => {
                if self.kind.accepts(character) {
                    self.text.push(character);
                }
                // Consumed either way: a key the prompt refuses is still a
                // key it *saw*, and letting it fall through to the key table
                // would run a command from inside a prompt.
                PromptOutcome::Consumed
            }
            _ => PromptOutcome::Ignored,
        }
    }
}
