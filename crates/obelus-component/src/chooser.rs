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
//! **The last row opens a project that is not in the list**, so the list
//! is never empty and a reader with nothing remembered still has
//! somewhere to press. A verb among rows that are nouns, and set apart
//! from them below rather than above: the commonest answer to "which
//! project" is the one the reader was in last, so that is the row they
//! start on and enter alone takes them back to it. It was the first row
//! once, the way the conversations' list has it, and every start from a
//! launcher cost a key to get past it.
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
//! walked with.
//!
//! **What could finish a path is not here.** It is an ordinary compact
//! list, built and drawn where every other one is -- the agent's own
//! commands while one is being typed are the same arrangement, and the
//! rule they settled is the rule here: the list follows what is in the
//! box, and the box owns the keys. This says *which directory* is being
//! asked about ([`Outcome::Wants`]) and nothing about what is in it.

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
    /// The same, written the way the row shows it.
    ///
    /// Handed over rather than worked out here, and the filter is run
    /// against *this* and not against the path: a reader types what
    /// they can see. Matching the whole path instead meant `home`
    /// matched every row on an ordinary machine while no row had the
    /// word on it, and the characters a match would mark were counted
    /// in a string nobody was looking at.
    pub shown: String,
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

/// The path being typed.
#[derive(Debug)]
struct Naming {
    typed: Field,
    /// The directory last asked about, so that a letter which does not
    /// change it does not ask again. This is the whole of why a reader
    /// typing a long path pays for the directories they pass through and
    /// not for the letters.
    read: Option<PathBuf>,
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
    /// Which row the reader is on: the projects the filter leaves from
    /// nought, and the opening row after the last of them.
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
    pub fn rows(&self) -> Vec<(&Known, Option<(usize, usize)>)> {
        let query = self.filter.said().to_lowercase();
        if query.is_empty() {
            return self.known.iter().map(|known| (known, None)).collect();
        }
        self.known
            .iter()
            .filter_map(|known| {
                // In characters and not in bytes, because what comes back
                // is handed to a view that marks characters -- and a path
                // may hold one that is several bytes wide.
                let shown = known.shown.to_lowercase();
                let at = shown.find(&query)?;
                let first = shown[..at].chars().count();
                Some((known, Some((first, first + query.chars().count()))))
            })
            .collect()
    }

    /// How many rows there are, the opening one included.
    #[must_use]
    pub fn count(&self) -> usize {
        self.rows().len() + 1
    }

    /// Which row the reader is on: the projects from nought, and then the
    /// opening row.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// Which of the projects is on the first row of the list.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.window.top()
    }

    /// Moves the window over the projects if the reader's row has left
    /// it, for a list `height` rows tall.
    ///
    /// Over the projects only: the row that opens another is held under
    /// them rather than scrolled with them, so while the reader is on it
    /// the window keeps the last of the projects -- which is the row the
    /// arrow above it goes to.
    pub fn settle(&mut self, height: u16) {
        let rows = self.rows().len();
        self.window.set_count(rows);
        self.window.set_focus(self.at.min(rows.saturating_sub(1)));
        self.window.settle(height);
    }

    /// Whether the reader is on the row that opens a project not in the
    /// list.
    ///
    /// Asked rather than compared against nought or a count wherever it
    /// matters: which index that row has moved once already.
    fn on_the_opening_row(&self) -> bool {
        self.at >= self.rows().len()
    }

    /// Takes a key, and says what the application has to do about it.
    ///
    /// `height` is how many rows the list on screen has, which the paging
    /// keys need and nothing else does.
    pub fn handle(&mut self, key: KeyEvent, height: u16) -> Outcome {
        match &self.doing {
            Doing::Choosing => self.choosing(key, height),
            Doing::Naming(_) => self.naming(key),
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
            KeyCode::Enter if self.on_the_opening_row() => {
                self.doing = Doing::Naming(Box::new(Naming {
                    // Empty, and deliberately not what the filter holds:
                    // a word that narrowed a list of places is not the
                    // start of a path, and the two boxes mean two things.
                    typed: Field::new(),
                    read: None,
                }));
                self.wants()
            }
            KeyCode::Enter => match self.rows().get(self.at) {
                Some((known, _)) => Outcome::Chose(known.path.clone()),
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
                    // Back to the top, which is the newest that matched --
                    // or, where nothing did, the opening row, which is
                    // then the only one.
                    self.at = 0;
                    return Outcome::Taken;
                }
                Outcome::Ignored
            }
        }
    }

    /// A key while the reader is naming a path.
    ///
    /// Only the box. What could finish the path is a list somewhere else
    /// and it is offered the key first -- the arrangement the agent's own
    /// commands settled: the list follows what is typed, and the box owns
    /// every key the list did not want.
    fn naming(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            // Something nearer to give up on than the screen, so escape
            // does here what it does everywhere: the path box goes and
            // the projects are underneath it again. The list in front of
            // *it* has already had its turn at this key.
            KeyCode::Esc => {
                self.doing = Doing::Choosing;
                // Back on the row the box was opened from.
                self.at = self.rows().len();
                Outcome::Taken
            }
            // What is in the box. A candidate is never chosen here --
            // choosing one puts it *in* the box, which is the list's own
            // doing, and by the time this is reached there is no list.
            KeyCode::Enter => {
                let Doing::Naming(naming) = &self.doing else {
                    return Outcome::Ignored;
                };
                Outcome::Chose(PathBuf::from(expanded(&naming.typed.said())))
            }
            _ => {
                let Doing::Naming(naming) = &mut self.doing else {
                    return Outcome::Ignored;
                };
                if !naming.typed.handle_key(&key) {
                    return Outcome::Ignored;
                }
                self.wants()
            }
        }
    }

    /// Puts a chosen candidate in the box.
    ///
    /// A directory takes a separator with it, so that the next level's
    /// candidates are asked for without the reader typing one -- which is
    /// what makes walking down a tree one key per level.
    pub fn put(&mut self, path: &Path, directory: bool) -> Outcome {
        let Doing::Naming(naming) = &mut self.doing else {
            return Outcome::Ignored;
        };
        let mut said = path.to_string_lossy().into_owned();
        if directory && !said.ends_with(std::path::is_separator) {
            said.push(std::path::MAIN_SEPARATOR);
        }
        naming.typed.replace(&said);
        self.wants()
    }

    /// What is in the box, as a path.
    #[must_use]
    pub fn named(&self) -> Option<PathBuf> {
        match &self.doing {
            Doing::Choosing => None,
            Doing::Naming(naming) => Some(PathBuf::from(expanded(&naming.typed.said()))),
        }
    }

    /// Drops a project from the list.
    ///
    /// For one whose directory has gone since it was written down. The
    /// row is not dimmed, it is taken away: a dim row says "not here",
    /// and what is true of this one is that there is nothing to offer.
    /// Nothing is written back -- the list on disk is tidied the next
    /// time something is remembered, and a reader who reaches a dead row
    /// twice in one session is a reader who pressed it twice.
    pub fn forget(&mut self, path: &Path) {
        self.known.retain(|known| known.path != path);
        self.at = self.at.min(self.known.len());
    }

    /// Which part of the box the list is narrowing by.
    ///
    /// Everything after the last separator: the directory in front of it
    /// is what was read, and these are the letters that choose among what
    /// it holds.
    #[must_use]
    pub fn segment(&self) -> String {
        let Doing::Naming(naming) = &self.doing else {
            return String::new();
        };
        let said = naming.typed.said();
        match said.rfind(std::path::is_separator) {
            Some(at) => said[at + 1..].to_string(),
            None => said,
        }
    }

    /// Which directory the box is about, where it is about one.
    ///
    /// Everything up to and including the last separator. One answer,
    /// asked both by the key that decides whether a directory has to be
    /// read and by the frame that decides whether what was read still
    /// applies -- worked out twice, those two disagreed about an empty
    /// box, and the whole of the last directory stayed on screen under a
    /// box the reader had rubbed out.
    ///
    /// Either separator, because Windows takes both: a reader who types
    /// `C:/Users` and one who types it with a backslash have named the
    /// same directory, and looking only for the one this platform writes
    /// leaves the other with no candidates at all.
    #[must_use]
    pub fn directory_named(&self) -> Option<PathBuf> {
        let Doing::Naming(naming) = &self.doing else {
            return None;
        };
        let said = expanded(&naming.typed.said());
        // The separator itself stays, so that a box holding `/` asks
        // about the root rather than about the empty string.
        let at = said.rfind(std::path::is_separator)?;
        Some(PathBuf::from(&said[..=at]))
    }

    /// Which directory's entries would answer the box as it stands.
    ///
    /// Everything up to the last separator. A box holding `~/Work/ob` is
    /// asking about `~/Work`, and the letters after it are what narrows
    /// the answer -- so the question only changes when a separator is
    /// typed or taken away, and the reading only happens then.
    fn wants(&mut self) -> Outcome {
        let asked = self.directory_named();
        let Doing::Naming(naming) = &mut self.doing else {
            return Outcome::Ignored;
        };
        let Some(directory) = asked else {
            // The box names no directory -- it is empty, or holds a bare
            // name with nowhere to look one up. What was read a moment
            // ago belongs to a path that is no longer in the box, so
            // forgetting it is the whole of the answer: a reader who
            // rubs out everything they typed should not be left looking
            // at the contents of where they used to be.
            naming.read = None;
            return Outcome::Taken;
        };
        if naming.read.as_deref() == Some(directory.as_path()) {
            // The same directory as the letter before, so the candidates
            // in hand are still the answer and nothing has to be read.
            return Outcome::Taken;
        }
        naming.read = Some(directory.clone());
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
    // Either separator, for the reason `wants` gives.
    if !rest.is_empty() && !rest.starts_with(std::path::is_separator) {
        return said.to_string();
    }
    let Some(home) = std::env::home_dir() else {
        return said.to_string();
    };
    format!("{}{rest}", home.display())
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
