//! Which project to work in, asked when nothing else has answered.
//!
//! Obelus is normally told where it is: by an argument, or by the
//! directory a reader was standing in when they typed `ob`. A desktop
//! launcher says neither -- the process begins wherever the launcher
//! began, which is the home directory -- so there is a start with no
//! project, and this is the only thing on the screen until there is one.
//!
//! **It cannot be left.** Everything else Obelus draws is a thing opened
//! over a project, and a reader who escaped this would be left in the one
//! state the rest of the application has no answer for: the file list
//! would walk the home directory, the notes and the conversations would be
//! filed under a place nobody works in. So escape gives up on the nearest
//! thing, and here there is nothing nearer -- until the reader is naming a
//! path, where there is.
//!
//! **The first row opens a project that is not in the list**, which is the
//! same decision the list of conversations made: its first row starts a
//! new one, so the list is never empty and a reader with nothing
//! remembered still has somewhere to press. A row that is a verb among
//! rows that are nouns, and it is the first because it is the only one
//! that is always there.
//!
//! **Two boxes, never two meanings.** The filter is about the rows
//! underneath it -- projects this reader has had open -- and the path box
//! is about the filesystem. They are two states of the one row at the foot
//! and they do not share their text: what was typed to narrow a list of
//! places is not the beginning of a path, and carrying it over would be
//! the one mistake that makes the two read as one thing.
//!
//! **No numbers.** A short list elsewhere in Obelus is chosen from with
//! `1`-`9`, and that cannot happen here: both states of the foot are typed
//! into, so a digit is a character. `~/Work/project2` is the example that
//! settles it. The rows are walked with the keys every other list is
//! walked with, and `Tab` -- which no command may be bound to, because
//! every box takes it itself -- is what a path box has always meant by
//! "finish this for me".

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    field::Field,
    window::{Move, Window, Wrap},
};

/// One project the reader has had open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    /// Where it is, as it was named.
    pub path: PathBuf,
    /// When it was last opened, in seconds since the epoch.
    ///
    /// `None` for a row written before Obelus kept one, which is drawn
    /// with nothing where the words about when would go.
    pub last: Option<i64>,
}

/// What the chooser wants the application to do about a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It is handled, and nothing outside has to happen.
    Taken,
    /// The key means nothing here.
    ///
    /// Which is not the same as it being passed on: nothing is reachable
    /// past this screen, and the caller's part is to do nothing rather
    /// than to look further.
    Ignored,
    /// The reader has settled on this project.
    Chose(PathBuf),
    /// What is being typed now names this directory, and its entries are
    /// wanted.
    ///
    /// Asked of the caller rather than read here, for the reason every
    /// picker's rows come from the application: this crate draws and
    /// walks, and the one that knows how to look at a disk is the one that
    /// already does it for the file list. It is also what keeps the
    /// reading down to one per directory -- a question asked only when the
    /// answer could have changed, rather than on every letter.
    Wants(PathBuf),
}

/// What the reader is doing.
#[derive(Debug)]
enum Doing {
    /// Looking through the projects they have had open.
    Choosing,
    /// Naming one that is not in the list.
    ///
    /// Boxed because it is much the larger of the two and the other
    /// carries nothing: every chooser would otherwise be as big as the
    /// state of a box nobody is typing in.
    Naming(Box<Naming>),
}

/// The path being typed, and what could finish it.
#[derive(Debug)]
struct Naming {
    typed: Field,
    /// The directory the candidates were read from, so that a letter which
    /// does not change it does not ask for them again.
    read: Option<PathBuf>,
    /// What that directory holds, in the order it was handed over.
    candidates: Vec<PathBuf>,
    /// Which of them are on screen, and which the reader is on.
    ///
    /// Nothing is on by default: the caret is in the box, and a candidate
    /// is reached by walking down into the list. Which is also what keeps
    /// enter meaning "open what I typed" until the reader has said
    /// otherwise.
    at: Option<usize>,
    window: Window,
}

/// The question, and everything being done about it.
#[derive(Debug)]
pub struct Chooser {
    /// Every project remembered, newest first, as it was handed over.
    known: Vec<Known>,
    /// What narrows them. Never applied to the row that opens a new one:
    /// that row is not one of the projects and a filter that could hide it
    /// would be a screen with no way off it.
    filter: Field,
    /// Which row the reader is on, counting the opening row as nought.
    at: usize,
    window: Window,
    doing: Doing,
}

impl Chooser {
    /// One asking about these projects.
    #[must_use]
    pub fn new(known: Vec<Known>) -> Self {
        let mut window = Window::new();
        window.set_count(known.len() + 1);
        Self {
            known,
            filter: Field::new(),
            at: 0,
            window,
            doing: Doing::Choosing,
        }
    }

    /// Whether the reader is naming a path rather than looking at rows.
    #[must_use]
    pub const fn is_naming(&self) -> bool {
        matches!(self.doing, Doing::Naming(_))
    }

    /// What the foot says, which is one of the two boxes.
    #[must_use]
    pub fn typing(&self) -> &Field {
        match &self.doing {
            Doing::Choosing => &self.filter,
            Doing::Naming(naming) => &naming.typed,
        }
    }

    /// The projects the filter leaves, newest first.
    ///
    /// Worked out rather than kept, because what it is worked out from is
    /// one short string and a list of twenty: a cache here would be a
    /// second answer to a question that costs nothing to ask.
    #[must_use]
    pub fn rows(&self) -> Vec<&Known> {
        let query = self.filter.said().to_lowercase();
        self.known
            .iter()
            .filter(|known| {
                query.is_empty() || known.path.to_string_lossy().to_lowercase().contains(&query)
            })
            .collect()
    }

    /// How many rows there are, the opening one included.
    #[must_use]
    pub fn count(&self) -> usize {
        self.rows().len() + 1
    }

    /// Which row the reader is on, where nought is the opening row.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// What could finish the path being typed, where one is.
    #[must_use]
    pub fn candidates(&self) -> &[PathBuf] {
        match &self.doing {
            Doing::Choosing => &[],
            Doing::Naming(naming) => &naming.candidates,
        }
    }

    /// Which candidate the reader is on, where they are on one.
    #[must_use]
    pub const fn candidate_at(&self) -> Option<usize> {
        match &self.doing {
            Doing::Choosing => None,
            Doing::Naming(naming) => naming.at,
        }
    }

    /// Hands over what a directory holds, in answer to [`Outcome::Wants`].
    ///
    /// Ignored where the reader has typed on since asking, which is the
    /// normal case rather than the exceptional one: the answer describes a
    /// box that may have moved, and a list of candidates for a directory
    /// nobody is typing in is worse than none.
    pub fn offer(&mut self, directory: &Path, entries: Vec<PathBuf>) {
        let Doing::Naming(naming) = &mut self.doing else {
            return;
        };
        if naming.read.as_deref() != Some(directory) {
            return;
        }
        naming.window.set_count(entries.len());
        naming.candidates = entries;
        naming.at = None;
        naming.window.set_focus(0);
    }

    /// Takes a key, and says what the application has to do about it.
    ///
    /// `height` is how many rows the list on screen has, which the paging
    /// keys need and nothing else does.
    pub fn handle(&mut self, key: KeyEvent, height: u16) -> Outcome {
        match &self.doing {
            Doing::Choosing => self.choosing(key, height),
            Doing::Naming(_) => self.naming(key, height),
        }
    }

    /// A key while the reader is looking through what they have opened.
    fn choosing(&mut self, key: KeyEvent, height: u16) -> Outcome {
        if let Some(movement) = movement(key) {
            let count = self.count();
            self.window.set_count(count);
            self.window.set_focus(self.at);
            self.window.apply(movement, height, Wrap::Yes);
            self.at = self.window.focus();
            return Outcome::Taken;
        }
        match key.code {
            // The one row that is always there, and the only way to a
            // project this reader has not opened before.
            KeyCode::Enter if self.at == 0 => {
                self.doing = Doing::Naming(Box::new(Naming {
                    // Empty, and deliberately not what the filter holds:
                    // a word that narrowed a list of places is not the
                    // start of a path, and the two boxes mean two things.
                    typed: Field::new(),
                    read: None,
                    candidates: Vec::new(),
                    at: None,
                    window: Window::new(),
                }));
                self.wants()
            }
            KeyCode::Enter => match self.rows().get(self.at - 1) {
                Some(known) => Outcome::Chose(known.path.clone()),
                // A row that went while the key was travelling. Nothing,
                // rather than a guess at which row was meant.
                None => Outcome::Ignored,
            },
            // Nothing is nearer than this screen, so there is nothing to
            // give up on. Clearing the filter is what escape means where
            // there is one, and a reader who has narrowed to nothing has
            // exactly one key they would reach for.
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.at = 0;
                Outcome::Taken
            }
            KeyCode::Esc => Outcome::Ignored,
            _ => {
                if self.filter.handle_key(&key) {
                    // The rows underneath have moved, so standing on the
                    // fifth of them is standing on a different project.
                    // Back to the row that is always the same one.
                    self.at = 0;
                    return Outcome::Taken;
                }
                Outcome::Ignored
            }
        }
    }

    /// A key while the reader is naming a path.
    fn naming(&mut self, key: KeyEvent, height: u16) -> Outcome {
        // Before the modifiers are looked at, because a box takes these
        // itself: `Tab` is how every path box a reader has used says
        // "finish this", and `keymap::why_not` refuses to bind it for
        // exactly that reason.
        if key.code == KeyCode::Tab {
            return self.finish_it();
        }
        if let Some(movement) = movement(key) {
            let Doing::Naming(naming) = &mut self.doing else {
                return Outcome::Ignored;
            };
            if naming.candidates.is_empty() {
                return Outcome::Ignored;
            }
            naming.window.set_count(naming.candidates.len());
            naming.window.set_focus(naming.at.unwrap_or(0));
            naming.window.apply(movement, height, Wrap::Yes);
            naming.at = Some(naming.window.focus());
            return Outcome::Taken;
        }
        match key.code {
            // Here there *is* something nearer to give up on, so escape
            // does what it does everywhere: the path box goes and the
            // list of projects is underneath it again.
            KeyCode::Esc => {
                self.doing = Doing::Choosing;
                self.at = 0;
                Outcome::Taken
            }
            KeyCode::Enter => {
                let Doing::Naming(naming) = &self.doing else {
                    return Outcome::Ignored;
                };
                // What the reader is standing on, or what they typed
                // where they are standing on nothing. The box is the
                // answer until they walk into the list, which is what
                // makes a path nobody offered still openable.
                let chosen = match naming.at.and_then(|at| naming.candidates.get(at)) {
                    Some(candidate) => candidate.clone(),
                    None => PathBuf::from(expanded(&naming.typed.said())),
                };
                Outcome::Chose(chosen)
            }
            _ => {
                let Doing::Naming(naming) = &mut self.doing else {
                    return Outcome::Ignored;
                };
                if !naming.typed.handle_key(&key) {
                    return Outcome::Ignored;
                }
                // Typing is about the box again, not about whichever
                // candidate was under the reader a moment ago.
                naming.at = None;
                self.wants()
            }
        }
    }

    /// Puts the one thing every candidate agrees on into the box.
    ///
    /// What a reader means by `Tab`, and the half of it that is not
    /// choosing: where every candidate starts the same way, that much is
    /// certain and typing it again is work the machine can do. Where they
    /// do not agree, nothing is put in -- the list below is already
    /// showing what the disagreement is.
    fn finish_it(&mut self) -> Outcome {
        let Doing::Naming(naming) = &mut self.doing else {
            return Outcome::Ignored;
        };
        let Some(shared) = common_prefix(&naming.candidates) else {
            return Outcome::Ignored;
        };
        if shared.len() <= naming.typed.said().len() {
            return Outcome::Ignored;
        }
        naming.typed.replace(&shared);
        naming.at = None;
        self.wants()
    }

    /// Which directory's entries would answer the box as it stands.
    ///
    /// Everything up to the last separator. A box holding `~/Work/ob` is
    /// asking about `~/Work`, and the letters after it are what narrows
    /// the answer -- so the question only changes when a separator is
    /// typed or taken away, and the reading only happens then.
    fn wants(&mut self) -> Outcome {
        let Doing::Naming(naming) = &mut self.doing else {
            return Outcome::Ignored;
        };
        let said = expanded(&naming.typed.said());
        let directory = match said.rfind(std::path::MAIN_SEPARATOR) {
            // The separator itself stays, so that a box holding `/` asks
            // about the root rather than about the empty string.
            Some(at) => PathBuf::from(&said[..=at]),
            // Nothing that looks like a path yet. The reader is typing a
            // name with nowhere to look it up, and an empty list says so
            // better than the whole of the current directory would.
            None => return Outcome::Taken,
        };
        if naming.read.as_deref() == Some(directory.as_path()) {
            // The same directory as the letter before, so the candidates
            // in hand are still the answer and nothing has to be read.
            return Outcome::Taken;
        }
        naming.read = Some(directory.clone());
        naming.candidates.clear();
        naming.at = None;
        Outcome::Wants(directory)
    }
}

/// A leading `~` as the reader's own directory.
///
/// Done here rather than left to the caller because it is about what was
/// *typed*: the box shows `~/Work` and the question it asks the disk is
/// about the directory that names. Only in front, and only on its own or
/// before a separator, so a directory genuinely called `~weird` is left
/// alone.
#[must_use]
fn expanded(said: &str) -> String {
    let Some(rest) = said.strip_prefix('~') else {
        return said.to_string();
    };
    if !rest.is_empty() && !rest.starts_with(std::path::MAIN_SEPARATOR) {
        return said.to_string();
    }
    let Some(home) = std::env::home_dir() else {
        return said.to_string();
    };
    format!("{}{rest}", home.display())
}

/// The longest beginning every candidate shares, where there are any.
///
/// By characters rather than by bytes: a path with a multi-byte character
/// in it would otherwise be cut in the middle of one, and what goes in the
/// box has to be a string.
#[must_use]
fn common_prefix(candidates: &[PathBuf]) -> Option<String> {
    let mut all = candidates
        .iter()
        .map(|path| path.to_string_lossy().into_owned());
    let first = all.next()?;
    let mut shared: Vec<char> = first.chars().collect();
    for each in all {
        let common = shared
            .iter()
            .zip(each.chars())
            .take_while(|(left, right)| **left == *right)
            .count();
        shared.truncate(common);
    }
    match shared.is_empty() {
        true => None,
        false => Some(shared.into_iter().collect()),
    }
}

/// The key as one of the six ways of moving about a list, where it is one.
///
/// Through [`Move::of`], which is the one table of those six: a new way of
/// moving about a list reaches this screen without anybody remembering it
/// is here.
#[must_use]
fn movement(key: KeyEvent) -> Option<Move> {
    // Modifiers judged exactly, and before the code is looked at:
    // `shift+up` extends a selection elsewhere in Obelus and means nothing
    // here, and a key that answered to any modifier would swallow it.
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    Move::of(key.code)
}
