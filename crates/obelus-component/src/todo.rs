//! What a reader means to come back to, as something to walk through.
//!
//! A list of notes, each a row, some of them with somewhere to go. A dialog
//! rather than a page of the settings, for the reason the counts are one: it
//! is a question about the project rather than a switch the reader keeps,
//! and it is left with escape like every other dialog here.
//!
//! What the keys are is said at the foot of the view rather than learned:
//! there are six of them, three do nothing anywhere else in obelus, and a
//! view whose keys can only be found by reading the source is a view nobody
//! uses twice.
//!
//! The notes themselves, and the file they live in, are [`obelus_git::todo`].
//! Nothing here reads or writes that file -- the application does, because
//! it is the one that knows which tree this is.

use std::{ops::Range, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_git::todo::{INDENT, Note, Todo};
use obelus_text::coordinates::LineNumber;

use crate::{
    composer::{Composer, Laid},
    window::Window,
};

/// One row of the view.
///
/// Flat, the way the counts' rows are: a note's own lines are rows under it
/// when it is open, and what tells them apart is what the row *is* rather
/// than where it sits.
#[derive(Clone, Debug)]
pub struct Row {
    /// Which note it belongs to.
    pub note: usize,
    /// What it says.
    pub said: String,
    /// Whether this is the note's own row, or a line of its body.
    pub head: bool,
    /// Whether this row is where the note points rather than what it says.
    ///
    /// Its own row, under the note. Hung off the end of the first line it
    /// was a place the note's own words had to be laid out around -- and
    /// where half the notes have none, the right-hand edge it lined up
    /// against was not a column anybody could read down.
    pub place: bool,
    /// Whether the note is done, for the whole of it to be drawn as such.
    pub done: bool,
    /// How far under the note above its note sits.
    ///
    /// On every row of a note, not only its first: a body line that did not
    /// indent with its head would break the column its head is in, and the
    /// row where a note points is part of the note too.
    pub depth: u16,
    /// Which of `said`'s characters the reader has hold of, if any.
    ///
    /// Counted from the start of this row rather than of the note: a row is
    /// what gets drawn, and a selection given in the note's own lines would
    /// have to be taken apart again by whoever draws it.
    pub held: Option<Range<usize>>,
}

/// Whether a note has a conversation about it, and whether that
/// conversation wants something.
///
/// Which a list of notes has to say, because the answer outlives the
/// session: obelus writes down which conversation is about which note, so
/// a note talked over yesterday is one the agent still has every word of
/// -- and until this, the only way to find out was to open it and see
/// whether anything came back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Talked {
    /// Nobody has talked about it.
    #[default]
    Not,
    /// There is a conversation: open now, or written down against this
    /// note and waiting to be taken up again.
    Yes,
    /// And it is waiting on an answer. The same thing the list of open
    /// documents says about a conversation with a question in it, said
    /// here too: a reader who walked away from one is more likely to come
    /// back through the note than through the list.
    Waiting,
}

/// What a key did.
#[derive(Debug)]
pub enum TodoOutcome {
    /// Not a key this view knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// Handled, and the notes changed: write them down.
    Changed,
    /// Go to this place.
    Go(PathBuf, LineNumber),
    /// Talk to an agent about this note.
    Talk(obelus_git::todo::NoteId),
    /// The reader gave up.
    Cancelled,
    /// Put this on the clipboard.
    Copy {
        /// What to copy.
        text: String,
        /// What it was, for the reader to be told what they got.
        what: &'static str,
    },
    /// The same, for text this has just taken out: write the notes down
    /// too, and say it was cut rather than copied.
    Cut {
        /// What to copy.
        text: String,
        /// What it was.
        what: &'static str,
    },
    /// Put whatever is on the clipboard into the note the caret is in.
    ///
    /// Asked for rather than done, because what is on the clipboard is the
    /// application's question: this knows about a text and a caret in it.
    Paste,
}

/// The notes, while they are showing.
#[derive(Debug, Default)]
pub struct TodoView {
    /// What the tree has to come back to.
    todo: Todo,
    /// Where each note points *now*, worked out when the view opens.
    ///
    /// Beside the notes rather than in them: what a note says is what was
    /// written down, and where it points today is an answer about the file
    /// as it is. Keeping them apart is what stops the answer being written
    /// back into the file as though the reader had said it.
    ///
    /// `None` where git says the line has gone.
    where_now: Vec<Option<LineNumber>>,

    /// The rows as they stand, rebuilt when anything changes rather than per
    /// frame -- the draw path is the one place that cannot afford to do work
    /// it could have done once.
    rows: Vec<Row>,
    /// Which row is selected and which is on top.
    window: Window,
    /// How wide a note's own text is, and whether it wraps there.
    ///
    /// Kept because the rows depend on it: a note of one long line is one
    /// row or four, and the window, the caret and the drawing have to be
    /// counting the same rows. Set from the frame, which is the only place
    /// that knows.
    laid: (u16, bool),
    /// The note the caret is in, and what is in it.
    ///
    /// Always, where there is a note to be in. There is no editing *mode*
    /// here: the view is a page of notes being written, and a page that had
    /// to be unlocked before it would take a letter is a page where every
    /// change costs two keys it should not.
    ///
    /// The box a message to an agent is written in, because a note is the
    /// same shape: a paragraph, sometimes pasted, with a caret in it.
    writing: Option<(usize, Composer)>,
}

impl TodoView {
    /// Takes the file again, keeping what the reader was doing.
    ///
    /// Somebody else has written it -- a second obelus, or the reader's own
    /// editor -- and what they wrote is now what the file says. Reopening
    /// the view would be the simple answer and would throw away the note
    /// being written, so instead the notes are swapped and the two things
    /// that are the reader's are put back by *name*: which note the caret
    /// is in, and which one they are part-way through typing.
    ///
    /// A note being typed into that somebody else has deleted is kept, at
    /// the end. The reader is looking at it and has their hands on it; the
    /// other writer did not know that, and of the two of them only one is
    /// here to be surprised.
    pub fn reread(&mut self, todo: Todo, where_now: Vec<Option<LineNumber>>) {
        // The box itself is what carries across, not what it would have
        // written: `keep` takes the box with it, and what it writes into is
        // a copy of the notes this is about to throw away. So the note is
        // taken for its name and the box is kept whole -- which also keeps
        // the caret where the reader left it.
        let writing = self
            .writing
            .take()
            .and_then(|(at, composer)| Some((at, self.todo.notes.get(at)?.clone(), composer)));
        let focused = self.selected_note().map(|note| note.id.clone());

        self.todo = todo;
        self.where_now = where_now;

        let writing = writing.map(|(was, mut note, composer)| {
            let at = self.todo.notes.iter().position(|other| other.id == note.id);
            let at = at.unwrap_or_else(|| {
                // A note that says nothing has never been in anybody's
                // file: obelus does not write one down, so its not being
                // there is not somebody having taken it away. It goes back
                // where the reader had it -- put at the end instead, a
                // note just started would walk to the bottom of the list
                // the moment anything else wrote the file.
                if note.said.trim().is_empty() {
                    let at = was.min(self.todo.notes.len());
                    note.depth = note
                        .depth
                        .min(self.room_at(at))
                        .min(obelus_git::todo::DEEPEST);
                    self.todo.notes.insert(at, note);
                    self.where_now.insert(at, None);
                    return at;
                }
                // At the end, so under whatever is last there rather than
                // under the note it used to hang under -- that note is in
                // somebody else's file now, and may not be in it at all. A
                // depth deeper than the end can carry would be written to
                // disk illegal and read back a level shallower, which is the
                // note moving on its own between one open and the next.
                let room = self.room_at(self.todo.notes.len());
                note.depth = note.depth.min(room).min(obelus_git::todo::DEEPEST);
                self.todo.notes.push(note);
                self.where_now.push(None);
                self.todo.notes.len() - 1
            });
            (at, composer)
        });
        // The box back before the rows are built, not after: a note the
        // caret is in is laid out from what is in the box, and rebuilding
        // without it laid that note out from the file instead. The words
        // the reader had just put there were gone from the page until the
        // next thing rebuilt it -- which was them typing into the note
        // again, so the words came back and nothing said why.
        self.writing = writing;
        self.rebuild();

        // The caret back where it was, by name. A note that has gone leaves
        // the reader at the top, which is where a list with nothing to
        // return to puts them.
        if let Some(id) = focused {
            self.focus(&id);
        }
        self.follow_caret();
    }

    /// How deep a note put at this place may be.
    ///
    /// One deeper than what is above it, which is the whole rule: a note
    /// deeper than that is a child of nothing, and writing one would be
    /// writing a file that reads back a level shallower -- the note moving
    /// on its own between one open and the next.
    fn room_at(&self, at: usize) -> u16 {
        self.todo
            .notes
            .get(at.wrapping_sub(1))
            .map_or(0, |above| above.depth + 1)
    }

    /// Puts the selection on the note with this name, if it is still there.
    ///
    /// By name rather than by position, which is the whole reason a note has
    /// one: the list is read from the file every time it opens, and a note
    /// inserted above moves every position below it.
    pub fn focus(&mut self, to: &obelus_git::todo::NoteId) {
        let Some(at) = self.todo.notes.iter().position(|note| note.id == *to) else {
            return;
        };
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| row.note == at && !row.place)
        {
            self.window.set_focus(row);
        }
    }

    /// Opens the view over what a tree has, with where each note points
    /// worked out.
    #[must_use]
    pub fn new(todo: Todo, where_now: Vec<Option<LineNumber>>, laid: (u16, bool)) -> Self {
        let mut view = Self {
            todo,
            where_now,
            laid,
            ..Self::default()
        };
        view.rebuild();
        // The caret goes in straight away: there is nothing to unlock, and a
        // page that opened with no caret would be a page that looks like a
        // list until the reader finds out otherwise.
        if !view.todo.notes.is_empty() {
            view.enter_note(0, false);
        }
        view
    }

    /// Says how wide a note's text is and whether it wraps, and lays the
    /// rows out again if that has moved.
    ///
    /// Asked every frame, because a terminal is resized and a setting is
    /// changed while this is open.
    pub fn lay_out(&mut self, room: u16, wrap: bool) {
        if self.laid == (room, wrap) {
            return;
        }
        self.laid = (room, wrap);
        self.rebuild();
        self.follow_caret();
    }

    /// How a note's text is laid out: its width, and whether it wraps.
    #[must_use]
    pub const fn laid(&self) -> (u16, bool) {
        self.laid
    }

    /// The notes as they stand.
    #[must_use]
    pub const fn todo(&self) -> &Todo {
        &self.todo
    }

    /// The notes as the reader has them, for whoever writes them down.
    ///
    /// The words in the box are the note's: the box is where the reader
    /// put them, and `keep` only moves them into the note when they leave
    /// it. Every save used to ask for `todo` and so wrote the note as it
    /// *stood* -- without whatever the reader could see in it -- and the
    /// file was a keystroke behind the page for as long as a note was open.
    #[must_use]
    pub fn as_written(&self) -> Todo {
        let mut todo = self.todo.clone();
        if let Some((at, composer)) = self.writing.as_ref()
            && let Some(note) = todo.notes.get_mut(*at)
        {
            note.said = obelus_git::todo::trimmed(&composer.text());
        }
        todo
    }

    /// The rows as they stand.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Where the selection is.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// The note being written, if one is.
    #[must_use]
    pub fn writing(&self) -> Option<&Composer> {
        self.writing.as_ref().map(|(_, composer)| composer)
    }

    /// The same, to change: what a pointer landing in the box moves.
    pub fn writing_mut(&mut self) -> Option<&mut Composer> {
        self.writing.as_mut().map(|(_, composer)| composer)
    }

    /// Puts an empty note at the end and opens it for writing.
    ///
    /// Written where it will live rather than on the status bar: a note is
    /// read in this list, so it should be written in the shape it will be
    /// read in -- and there is nowhere else on this screen it belongs.
    ///
    /// Nothing is written down until it says something. An empty note taken
    /// away again on escape never existed, which is why it is put in here
    /// and only saved when it is kept.
    pub fn write_new(&mut self, at: Option<obelus_git::todo::At>) {
        self.keep();
        self.todo.notes.push(Note {
            id: obelus_git::todo::NoteId::mint(),
            said: String::new(),
            done: false,
            at,
            // At the end and under nothing: a note made about a line was
            // made somewhere else, and there is no note it was made beneath.
            depth: 0,
        });
        self.where_now.push(None);
        self.enter_note(self.todo.notes.len() - 1, true);
    }

    /// Lays the notes out as rows: every line of every one of them.
    ///
    /// Nothing is folded away. A note is what it says, and a list that shows
    /// a third of each note is a list a reader has to open one row at a time
    /// to read -- which is what they opened the list to avoid.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        // Wrapped where the reader asked for wrapping, and one row per line
        // where they did not -- the same answer the file behind this view
        // gives, because it is the same question, and the same width the
        // caret is measured against, because the two have to be counting
        // the same rows.
        let room = self.caret_width().max(1);
        for (index, note) in self.todo.notes.iter().enumerate() {
            // A note the caret is in shows what is in the box, not what is
            // on disk: the reader is looking at their own typing, and it is
            // the box that knows what of it they have hold of.
            let laid = match self.writing.as_ref().filter(|(at, _)| *at == index) {
                Some((_, composer)) => composer.laid(room),
                None => crate::composer::wrapped(&note.said, room),
            };
            let mut lines = laid.into_iter();
            let first: Laid = lines.next().unwrap_or_default();
            rows.push(Row {
                note: index,
                said: first.said,
                head: true,
                place: false,
                done: note.done,
                depth: note.depth,
                held: first.held,
            });
            for line in lines {
                rows.push(Row {
                    note: index,
                    said: line.said,
                    head: false,
                    place: false,
                    done: note.done,
                    depth: note.depth,
                    held: line.held,
                });
            }
            // And where it points, under what it says: a row of its own
            // rather than the end of the first line, so what a note says is
            // laid out the same whether it points anywhere or not.
            if let Some(at) = note.at.as_ref() {
                rows.push(Row {
                    note: index,
                    said: match self.where_now.get(index) {
                        Some(Some(line)) => format!("{}:{}", at.path.display(), line.get() + 1),
                        // The file still has the note, and the line does not.
                        _ => format!("{}:gone", at.path.display()),
                    },
                    head: false,
                    place: true,
                    done: note.done,
                    depth: note.depth,
                    // Never: it is a fact about the note rather than a word
                    // of it, and it is not the reader's to take a copy of
                    // by selecting it.
                    held: None,
                });
            }
        }
        self.rows = rows;
        self.window.set_count(self.rows.len());
    }

    /// Writes what is being typed into its note, and leaves the caret in
    /// it.
    ///
    /// Which is what [`Self::keep`] does except for the leaving: that one
    /// takes the box, because it is called on the way *out* of a note and
    /// leaving a note is what finishes it. This one is called by a key that
    /// goes nowhere.
    ///
    /// A note with nothing in it is thrown away, the same as anywhere else
    /// -- obelus does not write one down -- and then the caret goes to the
    /// nearest note there still is, because a list with notes in it and no
    /// caret anywhere is a list no key can reach.
    fn settle(&mut self) -> bool {
        let Some((at, composer)) = self.writing.as_ref() else {
            return false;
        };
        let (at, said) = (*at, obelus_git::todo::trimmed(&composer.text()));
        if said.trim().is_empty() {
            self.writing = None;
            self.drop_note(at);
            self.rebuild();
            if !self.todo.notes.is_empty() {
                self.enter_note(at.min(self.todo.notes.len() - 1), false);
            }
            return true;
        }
        match self.todo.notes.get_mut(at) {
            Some(note) if note.said != said => {
                note.said = said;
                true
            }
            _ => false,
        }
    }

    /// Puts the caret in a note, keeping whatever the last one said.
    ///
    /// The commit happens here rather than on a key, because leaving a note
    /// *is* finishing it: there is no other moment, and asking the reader to
    /// mark one would be the mode again under another name.
    fn enter_note(&mut self, to: usize, end: bool) {
        self.keep();
        let Some(note) = self.todo.notes.get(to) else {
            self.writing = None;
            return;
        };
        let mut composer = Composer::new();
        composer.replace(&note.said);
        let room = self.caret_width();
        if !end {
            composer.home(room);
            while composer.up(room) {}
        }
        self.writing = Some((to, composer));
        self.rebuild();
        // Never the place: it is a fact about the note rather than a line
        // of it, and a caret standing on it would be a caret in text the
        // reader cannot change.
        let row = match end {
            true => self
                .rows
                .iter()
                .rposition(|row| row.note == to && !row.place),
            false => self
                .rows
                .iter()
                .position(|row| row.note == to && !row.place),
        };
        if let Some(row) = row {
            self.window.set_focus(row);
        }
    }

    /// Writes what is in the box back into its note, and drops the note if
    /// it says nothing.
    ///
    /// Returns whether anything changed, so a caller can tell a move that
    /// wrote something from one that did not.
    fn keep(&mut self) -> bool {
        let Some((at, composer)) = self.writing.take() else {
            return false;
        };
        // The same shape it would come back in from the file: a note that
        // changed when it was read again would be a note whose rows moved
        // under a reader who had not touched it.
        let said = obelus_git::todo::trimmed(&composer.text());
        if said.trim().is_empty() {
            self.drop_note(at);
            return true;
        }
        match self.todo.notes.get_mut(at) {
            Some(note) if note.said != said => {
                note.said = said;
                true
            }
            _ => false,
        }
    }

    /// Puts a run of text into the note the caret is in, over whatever is
    /// held.
    ///
    /// A note of its own where there is none to be in: a reader who pastes
    /// into an empty page meant to start one.
    pub fn paste(&mut self, what: &str) {
        if self.writing.is_none() {
            self.write_new(None);
        }
        let room = self.caret_width();
        if let Some((_, composer)) = self.writing.as_mut() {
            composer.write_in(what, room);
        }
        self.rebuild();
        self.follow_caret();
    }

    /// Takes the note the caret is in away, and puts the caret on whatever
    /// takes its place. Says whether there was one to take.
    ///
    /// Two keys do this -- `alt+backspace`, which throws it away, and
    /// `ctrl+x`, which takes a copy on the way out -- and a note dropped
    /// half-way by one of them would leave the list a row it cannot fill.
    fn take_note_away(&mut self) -> bool {
        let Some(at) = self.selected() else {
            return false;
        };
        self.writing = None;
        self.drop_subtree(at);
        self.rebuild();
        if !self.todo.notes.is_empty() {
            self.enter_note(at.min(self.todo.notes.len() - 1), false);
        }
        true
    }

    /// Takes a note away, and the marks that pointed past it with it.
    ///
    /// The note alone: what hung under it comes up a level rather than
    /// going with it. This is the door a note leaves by when the reader
    /// empties its text, and emptying one note's text is not asking for
    /// anything to happen to another -- the note goes because a note with
    /// nothing in it is not a note, and its children were never in
    /// question. [`Self::drop_subtree`] is the other door, where they were.
    fn drop_note(&mut self, at: usize) {
        if at >= self.todo.notes.len() {
            return;
        }
        let under = self.todo.under(at);
        self.todo.notes.remove(at);
        self.where_now.remove(at);
        for below in self.todo.notes.iter_mut().skip(at).take(under) {
            below.depth = below.depth.saturating_sub(1);
        }
    }

    /// Takes a note away and everything hanging under it.
    ///
    /// What the key that means "take this note away" does: a note and what
    /// hangs under it are one thing on the screen, and a key that left the
    /// children behind would leave them under whatever happened to be
    /// above -- a result the reader cannot see at the moment they press it.
    fn drop_subtree(&mut self, at: usize) {
        if at >= self.todo.notes.len() {
            return;
        }
        let span = 1 + self.todo.under(at);
        self.todo.notes.drain(at..at + span);
        self.where_now.drain(at..at + span);
    }

    /// Whether the selected note has anywhere to go, in or out.
    ///
    /// The rules, in the one place they are written: the top, the note
    /// above, and the deepest a note may be. Both the key that does it and
    /// whatever says whether the key would do anything ask this, so a key
    /// drawn lit is a key that moves something -- two copies of three rules
    /// would be a hint that goes wrong on its own.
    #[must_use]
    pub fn can_shift(&self, outwards: bool) -> bool {
        let Some(at) = self.selected() else {
            return false;
        };
        let Some(depth) = self.todo.notes.get(at).map(|note| note.depth) else {
            return false;
        };
        if outwards {
            return depth > 0;
        }
        let room = self
            .todo
            .notes
            .get(at.wrapping_sub(1))
            .map_or(0, |above| above.depth + 1);
        let deepest = self
            .todo
            .notes
            .iter()
            .skip(at)
            .take(1 + self.todo.under(at))
            .map(|note| note.depth)
            .max()
            .unwrap_or(depth);
        depth < room && deepest < obelus_git::todo::DEEPEST
    }

    /// Where the note before this one at the same depth starts.
    ///
    /// `None` where there is none: the first child of a note has nothing
    /// above it at its own level, and neither has the first note of all.
    /// Walked backwards rather than counted, because what ends the search
    /// is the first note *shallower* than this one -- that is the parent,
    /// and above it is somebody else's list.
    fn before_it(&self, at: usize) -> Option<usize> {
        let depth = self.todo.notes.get(at)?.depth;
        for (index, note) in self.todo.notes[..at].iter().enumerate().rev() {
            if note.depth < depth {
                return None;
            }
            if note.depth == depth {
                return Some(index);
            }
        }
        None
    }

    /// And where the note after this one at the same depth starts.
    fn after_it(&self, at: usize) -> Option<usize> {
        let depth = self.todo.notes.get(at)?.depth;
        let next = at + 1 + self.todo.under(at);
        self.todo
            .notes
            .get(next)
            .filter(|note| note.depth == depth)
            .map(|_| next)
    }

    /// Moves a note and everything under it to where `to` starts.
    ///
    /// The whole run, because a note and its children are one thing to
    /// move: swapping single notes would step a parent over its own first
    /// child and leave the rest of them behind it. Depths are untouched --
    /// moving is not what changes who a note hangs under.
    fn move_subtree(&mut self, at: usize, to: usize) {
        let span = 1 + self.todo.under(at);
        let notes: Vec<_> = self.todo.notes.drain(at..at + span).collect();
        let marks: Vec<_> = self.where_now.drain(at..at + span).collect();
        // Past the hole the drain left, where it was after us.
        let to = match to > at {
            true => to - span,
            false => to,
        };
        self.todo.notes.splice(to..to, notes);
        self.where_now.splice(to..to, marks);
        self.enter_note(to, false);
    }

    /// Takes a note and everything under it one level in or out.
    ///
    /// Says whether it moved. The subtree keeps its shape: every note in it
    /// shifts by the same step, so a child that was two under its parent
    /// still is.
    ///
    /// Going in is bounded twice over -- by the note above, because a note
    /// may only ever be one deeper than whatever it hangs under, and by the
    /// deepest a note is allowed to be, which the run has to fit inside
    /// whole. Going out is bounded by the top.
    fn shift_subtree(&mut self, step: i16) -> bool {
        if !self.can_shift(step < 0) {
            return false;
        }
        let Some(at) = self.selected() else {
            return false;
        };
        let under = self.todo.under(at);
        for note in self.todo.notes.iter_mut().skip(at).take(1 + under) {
            note.depth = match step > 0 {
                true => note.depth + 1,
                false => note.depth.saturating_sub(1),
            };
        }
        // Rebuilt and followed rather than entered again: the reader is
        // still in the note they were in, and what [`Self::enter_note`] does
        // on the way into another is put away the one being left -- which
        // for a note nobody has typed into yet is to take it away. A reader
        // who starts a note and steps it in before saying anything is doing
        // the ordinary thing, and it took the note from under them.
        //
        // The rows change all the same: the column narrows when the list
        // gets deeper than it was, so what was one row may now be two.
        self.rebuild();
        self.follow_caret();
        true
    }

    /// Which note the selection is on, whichever of its rows that is.
    fn selected(&self) -> Option<usize> {
        self.rows.get(self.window.focus()).map(|row| row.note)
    }

    /// The note the selection is on, for whoever is saying what the keys do.
    #[must_use]
    pub fn selected_note(&self) -> Option<&Note> {
        self.todo.notes.get(self.selected()?)
    }

    /// Whether there is anywhere for enter to go from here.
    #[must_use]
    pub fn can_go(&self) -> bool {
        self.selected()
            .is_some_and(|at| self.where_now.get(at).copied().flatten().is_some())
    }

    /// Whether the list of every key is showing.
    /// Which row the note being written starts on, for the caret.
    #[must_use]
    pub fn writing_at(&self) -> Option<usize> {
        let (note, _) = self.writing.as_ref()?;
        self.rows
            .iter()
            .position(|row| row.note == *note && !row.place)
    }

    /// Whatever a key means here.
    ///
    /// The letters go in the note the caret is in, always: this page is a
    /// page of notes being written. What acts on a note *as a note* -- tick
    /// it, go where it points, move it, take it away -- is under `alt`,
    /// which is the question alt asks everywhere in obelus: about the thing
    /// the cursor is on.
    pub fn handle_key(&mut self, key: &KeyEvent, page: u16) -> TodoOutcome {
        // The width the box is asked about is this view's own answer, not
        // the caller's: the rows were laid out at it, the caret is measured
        // against it, and a third number arriving through the door is a
        // third answer for them to disagree over.
        let room = self.caret_width();
        let bare = key.modifiers.is_empty();
        let alt = key.modifiers == KeyModifiers::ALT;
        let control = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            // Writes down what has been typed, and stays where it is.
            //
            // Escape leaves whatever is *over* what is being read, and
            // nothing is over this: the notes are a document, not a thing
            // on top of one. So what it does here is the writing down, and
            // it must not move the caret -- it went through `keep`, which
            // takes the box because leaving a note is what finishes it, and
            // nothing put the box back. With one note in the list there was
            // nothing to walk to and so nothing that could open a box
            // again: the caret went out on escape and never returned.
            KeyCode::Esc if bare => {
                self.settle();
                self.rebuild();
                TodoOutcome::Cancelled
            }

            // One level in, and out. Taken from the box, which until now
            // put a tab character in a note: a literal tab in a paragraph
            // is worth little, tab is what indents an outline everywhere a
            // reader has met one, and `shift+tab` was already arriving here
            // and doing nothing at all.
            //
            // Refusing is silent. What says a key did nothing is that
            // nothing moved, and a note that will not go further in is a
            // note already as far in as the one above it -- which is on the
            // screen, one row up.
            KeyCode::Tab if bare => match self.shift_subtree(1) {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },
            KeyCode::BackTab => match self.shift_subtree(-1) {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },

            // Another note, which is what enter means in a page being
            // written. Where it points is `alt+enter`: this is not a list of
            // rows to choose from, it is the text of them.
            KeyCode::Enter if bare => {
                // After the whole of what hangs under the selected note,
                // and at its depth: a note started from a parent is the
                // next thing at that level. Put between the parent and its
                // children it would have been adopted by it without the
                // reader asking for a child at all.
                let (after, depth) = self.selected().map_or((0, 0), |at| {
                    let depth = self.todo.notes.get(at).map_or(0, |note| note.depth);
                    (at + 1 + self.todo.under(at), depth)
                });
                self.keep();
                // The note may have gone with the keep, if it said nothing.
                let after = after.min(self.todo.notes.len());
                self.todo.notes.insert(
                    after,
                    Note {
                        id: obelus_git::todo::NoteId::mint(),
                        said: String::new(),
                        done: false,
                        at: None,
                        depth,
                    },
                );
                self.where_now.insert(after, None);
                self.enter_note(after, true);
                // Typing, not a change: what was kept is what was being
                // typed, and the note started is empty. Saying `Changed`
                // wrote the whole file on the keystroke, and what it wrote
                // was a note with nothing in it -- which is not a note, and
                // an agent reading the file found a blank entry in the list.
                // `write_new` is the same thing said from the other key and
                // has never saved. Both reach the file the moment the reader
                // leaves the note, and an empty one is dropped on the way.
                TodoOutcome::Consumed
            }

            // Up and down walk the note's own lines first, and step to the
            // next note when there are none left -- which is what the box
            // answering `false` at its ends is for.
            KeyCode::Up | KeyCode::Down if bare => {
                let down = key.code == KeyCode::Down;
                let inside = self
                    .writing
                    .as_mut()
                    .is_some_and(|(_, composer)| match down {
                        true => composer.down(room),
                        false => composer.up(room),
                    });
                if inside {
                    // Built again, not only followed: a row carries what
                    // of it the reader is holding, and this key is one of
                    // the ways they let go. Following alone left the run
                    // coloured on a note the box had already dropped.
                    self.rebuild();
                    self.follow_caret();
                    return TodoOutcome::Consumed;
                }
                let Some(at) = self.selected() else {
                    return TodoOutcome::Consumed;
                };
                let to = match down {
                    true if at + 1 < self.todo.notes.len() => at + 1,
                    false if at > 0 => at - 1,
                    _ => return TodoOutcome::Consumed,
                };
                self.enter_note(to, !down);
                TodoOutcome::Consumed
            }
            KeyCode::PageUp | KeyCode::PageDown if bare => {
                let by = isize::try_from(page.max(1)).unwrap_or(1);
                let by = match key.code {
                    KeyCode::PageUp => -by,
                    _ => by,
                };
                let landed = self.window.step(by, crate::window::Wrap::No);
                if let Some(to) = self.rows.get(landed).map(|row| row.note) {
                    self.enter_note(to, false);
                }
                TodoOutcome::Consumed
            }

            // Done, or not. On `alt` because space is a space here.
            KeyCode::Char(' ') if alt => match self.selected() {
                Some(at) => {
                    if let Some(note) = self.todo.notes.get_mut(at) {
                        note.done = !note.done;
                    }
                    self.rebuild();
                    TodoOutcome::Changed
                }
                None => TodoOutcome::Consumed,
            },
            // Where it points. A note about the project has nowhere to go,
            // and nothing is the honest answer.
            KeyCode::Enter if alt => match self.selected().and_then(|at| {
                let place = self.todo.notes.get(at)?.at.as_ref()?;
                Some((place.path.clone(), (*self.where_now.get(at)?)?))
            }) {
                Some((path, line)) => {
                    self.keep();
                    TodoOutcome::Go(path, line)
                }
                None => TodoOutcome::Consumed,
            },
            // The whole note, because backspace on its own is a character.
            // Talk about this one. `alt+a` because the agent's other keys
            // are on `a`, and because it reads as a thing done *to* the row
            // the caret is in -- which is what every other `alt` key on this
            // page is.
            KeyCode::Char('a') if alt => match self.selected_note() {
                Some(note) => {
                    let id = note.id.clone();
                    self.keep();
                    TodoOutcome::Talk(id)
                }
                None => TodoOutcome::Consumed,
            },
            KeyCode::Backspace | KeyCode::Delete if alt => match self.take_note_away() {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },
            // Where a note sits is the reader's to decide, so nothing else
            // reorders the list: ticking one leaves it where it is.
            KeyCode::Up | KeyCode::Down if alt => {
                self.keep();
                let Some(at) = self.selected() else {
                    return TodoOutcome::Consumed;
                };
                // Past the neighbour at this note's own level, and past the
                // whole of what hangs under *it*: stepping over one note at
                // a time would put this one in the middle of somebody
                // else's children. The first of a parent's children has
                // nobody above it at its level and nowhere to go, which is
                // what `shift+tab` is for.
                let to = match key.code {
                    KeyCode::Up => self.before_it(at),
                    KeyCode::Down => self
                        .after_it(at)
                        .map(|next| next + 1 + self.todo.under(next)),
                    _ => None,
                };
                let Some(to) = to else {
                    self.enter_note(at, false);
                    return TodoOutcome::Consumed;
                };
                self.move_subtree(at, to);
                TodoOutcome::Changed
            }

            // The four keys a reader arrives already holding. They have to
            // be answered here because a dialog is bound to nothing in the
            // key table -- and a box a reader can select in but not copy out
            // of is a box with half a selection.
            //
            // The whole note where nothing is held, the way the file takes
            // the whole line: copying nothing is not something a key can
            // usefully do, and the note is what a line is here.
            KeyCode::Char('c') if control => {
                let Some((_, composer)) = self.writing.as_ref() else {
                    return TodoOutcome::Consumed;
                };
                match composer.selected() {
                    Some(text) => TodoOutcome::Copy {
                        text,
                        what: "selection",
                    },
                    None => TodoOutcome::Copy {
                        text: composer.text(),
                        what: "note",
                    },
                }
            }
            KeyCode::Char('x') if control => {
                let room = self.caret_width();
                // What the note said before the cut, for the arm below: a
                // cut that took nothing has to know what the whole of it
                // was, and after the fact is too late to ask.
                let taken = self.writing.as_mut().map(|(_, composer)| {
                    let whole = composer.text();
                    (composer.cut(room), whole)
                });
                let Some((held, whole)) = taken else {
                    return TodoOutcome::Consumed;
                };
                if let Some(text) = held {
                    self.rebuild();
                    self.follow_caret();
                    return TodoOutcome::Cut {
                        text,
                        what: "selection",
                    };
                }
                // Nothing held, so the whole note goes. Which is how a note
                // is moved somewhere else, and is `alt+backspace` with a
                // copy taken on the way out.
                match self.take_note_away() {
                    true => TodoOutcome::Cut {
                        text: whole,
                        what: "note",
                    },
                    false => TodoOutcome::Consumed,
                }
            }
            KeyCode::Char('v') if control => TodoOutcome::Paste,
            KeyCode::Char('a') if control => {
                let room = self.caret_width();
                let Some((_, composer)) = self.writing.as_mut() else {
                    return TodoOutcome::Consumed;
                };
                composer.select_all(room);
                self.rebuild();
                self.follow_caret();
                TodoOutcome::Consumed
            }

            // Everything else is the box's, and the box knows which keys
            // those are: it is the same box a message to an agent is written
            // in, and which keys a box answers to is one rule.
            _ => {
                let took = self
                    .writing
                    .as_mut()
                    .is_some_and(|(_, composer)| composer.handle_key(key, room));
                if !took {
                    return TodoOutcome::Ignored;
                }
                self.rebuild();
                self.follow_caret();
                TodoOutcome::Consumed
            }
        }
    }

    /// The width the caret is measured against: the row's, where the text
    /// wraps there, and no limit where it does not.
    ///
    /// The same for every note, because where a note points is a row of its
    /// own: what a note says is laid out the same whether it points anywhere
    /// or not.
    ///
    /// And the same however deep a note sits, which is the reason the
    /// deepest one in the list is what the room is taken from rather than
    /// each note's own: one number is what the rows, the caret and the
    /// wrapping all read, and three answers to how wide a note is would be
    /// three chances for them to disagree. A page whose notes are all at the
    /// top loses nothing to it; indenting the first one narrows the column
    /// once, for every note, which is what happens when a column appears.
    #[must_use]
    pub fn caret_width(&self) -> u16 {
        match self.laid.1 {
            true => self.laid.0.saturating_sub(self.deepest() * INDENT).max(1),
            false => u16::MAX,
        }
    }

    /// How deep the deepest note in the list sits.
    fn deepest(&self) -> u16 {
        self.todo
            .notes
            .iter()
            .map(|note| note.depth)
            .max()
            .unwrap_or(0)
    }

    /// Keeps the selection on the row the caret is really in.
    ///
    /// The window is what scrolls, and it follows the caret rather than the
    /// note: a note of ten lines is ten rows, and a reader typing on the
    /// last of them should not have the view sitting on the first.
    fn follow_caret(&mut self) {
        let Some((at, composer)) = self.writing.as_ref() else {
            return;
        };
        let (line, _) = composer.caret(self.caret_width());
        let Some(first) = self
            .rows
            .iter()
            .position(|row| row.note == *at && !row.place)
        else {
            return;
        };
        self.window
            .set_focus((first + line).min(self.rows.len().saturating_sub(1)));
    }
}
