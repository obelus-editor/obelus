//! What a reader means to come back to, as something to walk through.
//!
//! A list of notes, each a row, some of them with somewhere to go. A dialog
//! rather than a page of the settings, for the reason the counts are one: it
//! is a question about the project rather than a switch the reader keeps,
//! and it is left with escape like every other dialog here.
//!
//! What the keys are is said at the foot of the view rather than learned:
//! there are six of them, three do nothing anywhere else in Obelus, and a
//! view whose keys can only be found by reading the source is a view nobody
//! uses twice.
//!
//! The notes themselves, and the file they live in, are [`obelus_git::todo`].
//! Nothing here reads or writes that file -- the application does, because
//! it is the one that knows which tree this is.
//!
//! **Folding is one act, and the notes are the fourth place it happens.** A
//! run of lines in a file, a run of tool calls in a transcript, a commit's
//! files in a list, and what hangs under a note: one row standing in for
//! several, `alt+f`, and the same arrow. So `Command::Fold` asks whichever
//! document is being read rather than the file always -- which is what it
//! did, folding a run of lines behind the notes that the reader could not
//! see -- and `Requires::AFoldHere` follows it, through `TodoView::can_fold`,
//! because the palette and the key are one judgement. What folds is what
//! hangs *under* a note and never a note's own lines (`rebuild`); which
//! notes are shut is this session's and kept by name (`shut`); and the arrow
//! has a column of its own whether or not anything folds, which is the
//! drawing's (`FOLDS` in `obelus_ui::todo`).
//!
//! **A note somebody else is talking about is read here, not changed.** The
//! claim says a reader is standing in that conversation, and taking the note
//! away is what destroys one -- a note that says nothing is not written to
//! the file at all, so clearing its words does it as surely as the key that
//! drops it. So every key that would change such a note does nothing: its
//! words, its box, its depth, its place. The caret still goes in it and the
//! foot says which keys are left (`elsewhere`); the lock runs upwards
//! (`run_is_elsewhere`); and the tools an agent is given are refused the same
//! note, in words it can repeat, because an agent must not do what the
//! reader in front of it cannot.
//!
//! Read off the claims Obelus last looked at, and not asked of the disk.
//! Asking for a claim opens it for writing, which is the very event a
//! watcher reports, so a key that asked would wake every Obelus on the
//! project -- and a letter held down on a locked note would wake them at the
//! rate the keyboard repeats. A stale lock therefore costs what it already
//! cost the drawing, and the way out is the key the lock is about: `alt+a`
//! asks for the claim outright, and a lock nobody holds gives way to it.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::PathBuf,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_git::todo::{Change, INDENT, Note, NoteId, Todo};
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
    /// Whether this row is the blank between two notes.
    ///
    /// A row of the list rather than a gap the drawing leaves, so that
    /// everything counting rows counts the same ones: the window, the
    /// scrollbar and the keys that page would each have to add it back,
    /// and a page that overshot by however much they disagreed is what
    /// two answers to "how tall is this" buys. Nowhere to stand, like the
    /// boundary row in `names`, so every way the caret moves steps over
    /// it.
    ///
    /// It belongs to the note *below* it, which is the one it announces:
    /// a key that pages onto one lands in the note it was heading for.
    pub gap: bool,
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
    /// Whether this row heads notes hanging under it, and whether they are
    /// showing.
    ///
    /// `None` where nothing hangs under it, which is most notes and every
    /// row that is not a note's first. The mark is the one every other
    /// folding thing in Obelus wears, because it is the same act: one row
    /// standing in for several, and a key that opens it.
    pub under: Option<bool>,
}

impl Row {
    /// Whether the caret can stand here.
    ///
    /// The note's own lines and nothing else: the place it points at is a
    /// fact about the note rather than a line of it, and the blank above
    /// it belongs to no line at all. Asked in one place because the four
    /// callers that look a note's rows up are four chances to remember
    /// only one of them.
    #[must_use]
    pub fn words(&self) -> bool {
        !self.place && !self.gap
    }
}

/// Who has the conversation of a note this window cannot change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// Another window on this same checkout, or one whose claim has not
    /// said where it is held.
    AnotherWindow,
    /// A window on another checkout of the project, by its directory's name.
    Checkout(String),
}

/// Whether a note has a conversation about it, and whether that
/// conversation wants something.
///
/// Which a list of notes has to say, because the answer outlives the
/// session: Obelus writes down which conversation is about which note, so
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
    /// Another Obelus has it open.
    ///
    /// Not this one's to enter and not this one's to describe: what the
    /// agent is doing in there is being told to the window that asked, so
    /// the most this one can say is whose it is not. Said at all because a
    /// row that simply refused the key would read as broken.
    Elsewhere,
    /// An agent is working in it right now.
    ///
    /// Said for the same reason the list of open documents says it: what
    /// is happening in a conversation nobody is looking at is only
    /// findable from somewhere else, and a reader who left a note with an
    /// agent on it comes back through the note.
    Working,
    /// And it is waiting on an answer. The same thing the list of open
    /// documents says about a conversation with a question in it, said
    /// here too: a reader who walked away from one is more likely to come
    /// back through the note than through the list.
    ///
    /// Ahead of [`Self::Working`] where both are true, because it is the
    /// reader's to do something about: an agent thinking will go on
    /// without them and a question will not.
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

    /// What has been done here and not written down yet.
    ///
    /// The page changes the copy it holds, so that the frame can show what
    /// the reader just did, and says what it did here. The file is *not*
    /// written from that copy: another Obelus has this tree open too, and a
    /// file written whole from a copy is that one's last minute taken back
    /// out. What reaches the disk is these, done to the file as it is at the
    /// moment of writing -- see [`obelus_git::todo::Change`].
    pending: Vec<Change>,
    /// The notes whose children the reader has folded away.
    ///
    /// By name and not by position: the file is another window's to change,
    /// and a set of indices is a set that belongs to whichever order the
    /// notes were in when it was made. A name is the one thing about a note
    /// that still means what it meant afterwards.
    ///
    /// This session's and not the file's. Which notes are open is the same
    /// kind of fact as which runs of a file are folded -- something the
    /// reader did to what is in front of them, not something they wrote
    /// down -- and `todo.toml` is a file two Obeluses share.
    shut: HashSet<NoteId>,
    /// The notes whose conversation another Obelus has open.
    ///
    /// Told to the page rather than worked out by it: a claim is a lock on
    /// a file in a directory the application owns, and this view knows
    /// about notes. Set from the frame and again before a key, the way the
    /// room a note is laid out in is.
    ///
    /// What it decides is what may be *changed*. The caret still goes in a
    /// note another window has -- it is the only mark of where the reader
    /// is standing, and a list with notes in it and no caret anywhere is a
    /// list no key can reach -- so the row can be read, selected in and
    /// copied out of, and the keys that would change it do nothing. What
    /// says which is which is the foot, where those keys stop being
    /// offered: the reader is told before they press, which is the rule the
    /// palette follows.
    ///
    /// And who has each, which the status row says of the note the caret
    /// is in: the lock says *that* the keys will do nothing, and a reader
    /// with three worktrees open needs to know which one to go to.
    elsewhere: HashMap<NoteId, Holder>,
    /// The notes this page started that are not in the file yet.
    ///
    /// What tells "put this note in" from "change the one that is there",
    /// which is the difference between a note the reader has just begun and
    /// a note the file has. Without it, typing into a note another window
    /// deleted while this page held it would write the note back -- this
    /// reader's Obelus undoing somebody's deliberate act, on a keystroke
    /// that was about neither of those things.
    unwritten: HashSet<NoteId>,
}

/// Where each note points, carried across a change of shape by name.
///
/// The marks sit in a list beside the notes, so anything that inserts,
/// moves or takes one away has to move them the same way or they belong to
/// the wrong notes. Rebuilt by name instead: a note carries its own, and a
/// note nobody has an answer for yet -- one just started, one another window
/// wrote -- has none until the next time they are worked out.
fn places_by_name(todo: &Todo, known: &[(NoteId, Option<LineNumber>)]) -> Vec<Option<LineNumber>> {
    todo.notes
        .iter()
        .map(|note| {
            known
                .iter()
                .find(|(id, _)| *id == note.id)
                .and_then(|(_, line)| *line)
        })
        .collect()
}

impl TodoView {
    /// Takes the file again, keeping what the reader was doing.
    ///
    /// Somebody else has written it -- a second Obelus, or the reader's own
    /// editor -- and what they wrote is now what the file says. Reopening
    /// the view would be the simple answer and would throw away the note
    /// being written, so instead the notes are swapped and the two things
    /// that are the reader's are put back by *name*: which note the caret
    /// is in, and which one they are part-way through typing.
    ///
    /// **A box is the reader's once they have put something in it, not
    /// because it exists.** The page opens with the caret already in a
    /// note, so a box holding this page's own copy of the note is there
    /// before anything has been typed, and keeping that over the file is
    /// this window saying the other window's change did not happen. So the
    /// box is asked whether it still says what the page's copy said, and
    /// where it does, takes the file's words. Both halves have a test,
    /// because each passes with the other broken:
    /// `a_note_written_in_one_window_arrives_in_the_other` and
    /// `what_is_being_typed_beats_what_the_other_window_wrote`. Neither had
    /// one, which is how a page that kept its own copy lasted.
    ///
    /// A note being typed into that somebody else has deleted is kept, at
    /// the end. The reader is looking at it and has their hands on it; the
    /// other writer did not know that, and of the two of them only one is
    /// here to be surprised. A note the caret had merely walked into goes
    /// like all the rest: there is nothing in it that is this reader's, and
    /// keeping it would be undoing a deliberate act on the strength of where
    /// a caret happened to be standing.
    ///
    /// `unwritten` names the notes whose *words* did not reach the file --
    /// the reader typed, and by the time it was written down the note had
    /// gone. The page cannot work that out for itself once the words are in
    /// its own copy of the note, and they are the only copy of themselves
    /// there is.
    pub fn reread(&mut self, todo: Todo, where_now: Vec<Option<LineNumber>>, unwritten: &[NoteId]) {
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
        let known: Vec<(NoteId, Option<LineNumber>)> = todo
            .notes
            .iter()
            .map(|note| note.id.clone())
            .zip(where_now)
            .collect();

        self.todo = todo;
        // What this page has done and has not written down yet, put back on
        // top of what the file says. They are going to the file at the next
        // write, and a page that showed them undone until then would be
        // showing the reader their own last keystroke being taken back by
        // somebody else's unrelated one.
        let pending = std::mem::take(&mut self.pending);
        for change in &pending {
            self.todo.apply(change);
        }
        self.pending = pending;
        // Whatever the file has is written, whether this page wrote it or
        // the other window did: a name that is in there now is not one this
        // page may insert again.
        let todo = &self.todo;
        self.unwritten.retain(|id| todo.find(id).is_none());

        let mut let_go_at = None;
        let writing = writing.and_then(|(was, mut note, composer)| {
            if let Some(at) = self.todo.find(&note.id) {
                // The box is kept because what is in it is the reader's --
                // and it is only theirs once they have put something there.
                // Untouched, it holds this page's own copy of the note, and
                // keeping that over the file is this page saying the other
                // window's change did not happen. Which is not a corner: a
                // page opens with the caret already in a note, so the note
                // a reader happens to be standing on was the one note no
                // other window could ever change under them.
                //
                // The same question the branch below asks about a note that
                // has gone, asked about one that is still here.
                let said = self.todo.notes[at].said.clone();
                if obelus_git::todo::trimmed(&composer.text()) == note.said && said != note.said {
                    let mut fresh = Composer::new();
                    fresh.replace(&said);
                    return Some((at, fresh));
                }
                return Some((at, composer));
            }
            // A note that says nothing has never been in anybody's file:
            // Obelus does not write one down, so its not being there is not
            // somebody having taken it away. It goes back where the reader
            // had it -- put at the end instead, a note just started would
            // walk to the bottom of the list the moment anything else wrote
            // the file.
            if note.said.trim().is_empty() {
                let at = was.min(self.todo.notes.len());
                note.depth = note
                    .depth
                    .min(self.todo.room_at(at))
                    .min(obelus_git::todo::DEEPEST);
                self.unwritten.insert(note.id.clone());
                self.todo.notes.insert(at, note);
                return Some((at, composer));
            }
            // Somebody else took it away, and the reader has nothing in it
            // that is theirs: the box says what the note said, and no words
            // of theirs were lost on the way to the file. They walked into
            // it and no further, so it goes like every other note the other
            // window took away.
            if obelus_git::todo::trimmed(&composer.text()) == note.said
                && !unwritten.contains(&note.id)
            {
                let_go_at = Some(was);
                return None;
            }
            // They have typed into it, though, and of the two of them only
            // one is here to be surprised. It stays on the page and it is
            // theirs again: not in the file, and so this page's to put back
            // rather than this page's to change, which is what unwritten
            // means.
            //
            // At the end, so under whatever is last there rather than under
            // the note it used to hang under -- that note is in somebody
            // else's file now, and may not be in it at all. A depth deeper
            // than the end can carry would be written to disk illegal and
            // read back a level shallower, which is the note moving on its
            // own between one open and the next.
            let room = self.todo.room_at(self.todo.notes.len());
            note.depth = note.depth.min(room).min(obelus_git::todo::DEEPEST);
            self.unwritten.insert(note.id.clone());
            self.todo.notes.push(note);
            Some((self.todo.notes.len() - 1, composer))
        });
        self.where_now = places_by_name(&self.todo, &known);
        // The box back before the rows are built, not after: a note the
        // caret is in is laid out from what is in the box, and rebuilding
        // without it laid that note out from the file instead. The words
        // the reader had just put there were gone from the page until the
        // next thing rebuilt it -- which was them typing into the note
        // again, so the words came back and nothing said why.
        self.writing = writing;
        self.rebuild();

        // The note the caret was in has gone, so the caret goes to whatever
        // took its place: a list with notes in it and no caret anywhere is a
        // list no key can reach.
        if let Some(was) = let_go_at
            && !self.todo.notes.is_empty()
        {
            self.enter_note(was.min(self.todo.notes.len() - 1), false);
            self.follow_caret();
            return;
        }
        // The caret back where it was, by name. A note that has gone leaves
        // the reader at the top, which is where a list with nothing to
        // return to puts them.
        if let Some(id) = focused {
            self.focus(&id);
        }
        self.follow_caret();
    }

    /// Does one thing to the notes: to the copy on the page now, and to the
    /// file when the page is next written down.
    ///
    /// The one door. Everything a key does to the notes comes through here,
    /// so there is one description of each act and the page and the file
    /// cannot be told two different things -- and the act, rather than the
    /// page's whole copy, is what the file gets.
    ///
    /// Says whether the note it was about was still on the page to do it
    /// to, which is only ever `false` for a note another window took away
    /// while this page held it.
    fn change(&mut self, change: Change) -> bool {
        if !self.only_here(&change) {
            return false;
        }
        self.pending.push(change);
        true
    }

    /// The same, to the copy alone.
    ///
    /// For a note that is in nobody's file: one the reader started and has
    /// not written down. Telling the disk to take away a note it has never
    /// had would be asking about a name that is not there, and being
    /// answered -- rightly -- that somebody else took it away.
    fn only_here(&mut self, change: &Change) -> bool {
        let known: Vec<(NoteId, Option<LineNumber>)> = self
            .todo
            .notes
            .iter()
            .map(|note| note.id.clone())
            .zip(self.where_now.iter().copied())
            .collect();
        if !self.todo.apply(change) {
            return false;
        }
        self.where_now = places_by_name(&self.todo, &known);
        true
    }

    /// Whether anything has been done here that the file has not been told.
    ///
    /// What the pause is armed by, along with the box: an act that reached
    /// the page and no disk is an act that is one crash from never having
    /// happened.
    #[must_use]
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// What has been done here and not written down, and the box with it.
    ///
    /// Taken, not read: whoever asks for these is about to do them to the
    /// file, and a change left behind would be done twice -- which for a
    /// move is a note two places further down than the reader put it.
    ///
    /// The box comes too. What is being typed is part of what the notes say
    /// long before the reader leaves the note, and the pause that brings us
    /// here is exactly the moment a paragraph half typed is meant to reach
    /// the file. An empty one is left alone: a note whose words have been
    /// cleared goes when the reader leaves it, not while they are still
    /// standing in it wondering what to write instead.
    pub fn take_changes(&mut self) -> Vec<Change> {
        if let Some((at, said)) = self
            .writing
            .as_ref()
            .map(|(at, composer)| (*at, obelus_git::todo::trimmed(&composer.text())))
            && !said.trim().is_empty()
            && let Some(change) = self.written_down(at, &said)
        {
            self.change(change);
        }
        std::mem::take(&mut self.pending)
    }

    /// Takes back changes that could not be written down.
    ///
    /// Nothing was written, so these are still the only account of what the
    /// reader did: they go back to the front of the queue and reach the file
    /// the next time anything writes -- which, for the file that would not
    /// read, is the first key after they fix it. Dropped instead, they stayed
    /// on the page until the file was read again and then vanished, which is
    /// the reader's own edit disappearing some minutes after they were told
    /// it had not been saved.
    pub fn put_back(&mut self, changes: Vec<Change>) {
        let mut queue = changes;
        queue.append(&mut self.pending);
        self.pending = queue;
    }

    /// The change that writing a note down is, or nothing where it already
    /// says that.
    ///
    /// The place the two kinds are told apart, and the only place: a note
    /// this page started goes in whole and may be inserted, a note that came
    /// out of the file is changed where it still is and nowhere otherwise.
    fn written_down(&self, at: usize, said: &str) -> Option<Change> {
        let note = self.todo.notes.get(at)?;
        if self.unwritten.contains(&note.id) {
            let mut note = note.clone();
            note.said = said.to_string();
            // What it goes behind, which is where it is on the page: the
            // file may have gained children under that one since, and
            // behind them is still where this note belongs.
            let after = at
                .checked_sub(1)
                .and_then(|before| self.todo.notes.get(before))
                .map(|before| before.id.clone());
            return Some(Change::Put { note, after });
        }
        (note.said != said).then(|| Change::Said {
            id: note.id.clone(),
            said: said.to_string(),
        })
    }

    /// Lets go of the note at `at`, which has nothing in it any more.
    ///
    /// Itself alone: what hung under it comes up a level. Emptying one
    /// note's words is not asking for anything to happen to another.
    fn let_the_note_go(&mut self, at: usize) -> bool {
        let Some(id) = self.todo.notes.get(at).map(|note| note.id.clone()) else {
            return false;
        };
        let change = Change::Remove {
            id: id.clone(),
            under: false,
        };
        match self.unwritten.remove(&id) {
            true => self.only_here(&change),
            false => self.change(change),
        }
    }

    /// Puts the selection on the note with this name, if it is still there.
    ///
    /// By name rather than by position, which is the whole reason a note has
    /// one: the list is read from the file every time it opens, and a note
    /// inserted above moves every position below it.
    fn focus(&mut self, to: &obelus_git::todo::NoteId) {
        let Some(at) = self.todo.notes.iter().position(|note| note.id == *to) else {
            return;
        };
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| row.note == at && row.words())
        {
            self.window.set_focus(row);
        }
    }

    /// Puts the caret in the note with this name, if it is still there.
    ///
    /// Not [`Self::focus`], which moves the selection and leaves the caret
    /// in whatever note it was in: a view just opened has its caret in the
    /// first note, and the next time the rows are laid out the selection
    /// follows the caret back there.
    pub fn put_caret_in(&mut self, to: &obelus_git::todo::NoteId) {
        if let Some(at) = self.todo.notes.iter().position(|note| note.id == *to) {
            self.enter_note(at, false);
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

    /// Says which notes another Obelus has the conversation of.
    ///
    /// Asked the same way [`Self::lay_out`] is -- from the frame, and again
    /// before a key -- because a claim is taken and given up in another
    /// window while this page is open.
    pub fn these_are_elsewhere(&mut self, which: HashMap<NoteId, Holder>) {
        self.elsewhere = which;
    }

    /// Who has the conversation of the note the caret is in, where
    /// somebody else does.
    #[must_use]
    pub fn selected_holder(&self) -> Option<&Holder> {
        self.selected_note()
            .and_then(|note| self.elsewhere.get(&note.id))
    }

    /// Whether the note the caret is in is one of them.
    ///
    /// What the foot reads, so that the keys it offers are keys that work,
    /// and what the keys themselves read, so the two cannot disagree.
    #[must_use]
    pub fn selected_is_elsewhere(&self) -> bool {
        self.selected_note()
            .is_some_and(|note| self.elsewhere.contains_key(&note.id))
    }

    /// Whether this note, or anything hanging under it, is somebody
    /// else's.
    ///
    /// For the two keys that take a whole run with them. Taking a note away
    /// takes its children, and a locked child would go with a parent nobody
    /// has claimed -- which is the lock running *upwards*: a note somebody
    /// is talking about holds back the keys of every note it hangs under,
    /// not the keys of the notes hanging under it. Stepping a run in or out
    /// is the same shape, because [`Change::Shift`] moves what hangs under
    /// the note as well, and a depth is a field of the note it is on.
    ///
    /// Refused whole rather than note by note: each note of the run goes to
    /// the file under its own name, so a run with one locked note in it
    /// would otherwise half-apply and leave the list a child whose parent
    /// has gone.
    ///
    /// Not asked by the keys that only *move* a run past its neighbour:
    /// [`Change::Move`] leaves every note's words, its box, its depth and
    /// its name exactly as they were, so a locked note carried along by its
    /// parent has not been changed.
    #[must_use]
    fn run_is_elsewhere(&self, at: usize) -> bool {
        if self.elsewhere.is_empty() {
            return false;
        }
        let span = 1 + self.todo.under(at);
        self.todo
            .notes
            .get(at..at + span)
            .unwrap_or_default()
            .iter()
            .any(|note| self.elsewhere.contains_key(&note.id))
    }

    /// Puts the window where the row the caret is in is on screen.
    ///
    /// The other half of [`Self::lay_out`], and asked the same way: every
    /// frame, from the room the list is actually drawn in. The width is
    /// what a note's words are wrapped at and this is how many of the rows
    /// they make the reader can see, and the second was never asked at
    /// all -- the window's top sat at zero for the life of the view. A
    /// list longer than the screen walked its selection off the bottom and
    /// stayed where it was, so a reader pressing down went on typing into
    /// a note that was no longer drawn, with no caret anywhere to say
    /// where they were.
    ///
    /// Here rather than at the end of a key, which is where a list and the
    /// counts settle theirs. Those views are drawn wherever they are put;
    /// this one is a document, and the room it has changes without anybody
    /// pressing anything -- a terminal resized, a foot that grows a row
    /// because the note now under the caret has somewhere to go. A key is
    /// followed by a frame either way, so asking here answers both.
    pub fn settle_window(&mut self, rows: u16) {
        self.window.settle(rows);
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

    /// Each note's name and where it points now.
    ///
    /// For whoever is working those answers out again after the file moved:
    /// where a note points is a question for git and the disk, and the
    /// answers this page already has are still the answers for the notes it
    /// already had.
    #[must_use]
    pub fn places(&self) -> Vec<(NoteId, Option<LineNumber>)> {
        self.todo
            .notes
            .iter()
            .map(|note| note.id.clone())
            .zip(self.where_now.iter().copied())
            .collect()
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

    /// Puts this row at the top, for a bar the reader has hold of.
    pub fn drag_to(&mut self, top: usize) {
        self.window.drag_to(top);
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
        let note = Note {
            id: obelus_git::todo::NoteId::mint(),
            said: String::new(),
            done: false,
            at,
            // At the end and under nothing: a note made about a line was
            // made somewhere else, and there is no note it was made beneath.
            depth: 0,
        };
        // In nobody's file until it says something: see
        // [`Self::written_down`].
        self.unwritten.insert(note.id.clone());
        self.todo.notes.push(note);
        self.where_now.push(None);
        self.enter_note(self.todo.notes.len() - 1, true);
    }

    /// Lays the notes out as rows: every line of every note that is showing.
    ///
    /// No note is shown in part. A note is what it says, and a list that
    /// shows a third of each note is a list a reader has to open one row at
    /// a time to read -- which is what they opened the list to avoid. What
    /// folds is what hangs *under* a note, which is a different question
    /// and the one a long list actually asks.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        // Which notes are hidden by a shut one above them, worked out in
        // one pass: `Todo::under` gives a contiguous run, so a note inside
        // one is inside it whatever is shut further down.
        let mut hidden = vec![false; self.todo.notes.len()];
        for at in 0..self.todo.notes.len() {
            if hidden[at] {
                continue;
            }
            let note = &self.todo.notes[at];
            if !self.shut.contains(&note.id) {
                continue;
            }
            for below in hidden
                .iter_mut()
                .take(at + 1 + self.todo.under(at))
                .skip(at + 1)
            {
                *below = true;
            }
        }
        // Wrapped where the reader asked for wrapping, and one row per line
        // where they did not -- the same answer the file behind this view
        // gives, because it is the same question, and the same width the
        // caret is measured against, because the two have to be counting
        // the same rows.
        let room = self.caret_width().max(1);
        for (index, note) in self.todo.notes.iter().enumerate() {
            if hidden[index] {
                continue;
            }
            // Whether anything hangs under it, and whether it is showing.
            // On the note's own row, which is the row the key acts on and
            // the row the mark goes on.
            let under = match self.todo.under(index) {
                0 => None,
                _ => Some(!self.shut.contains(&note.id)),
            };
            // A note the caret is in shows what is in the box, not what is
            // on disk: the reader is looking at their own typing, and it is
            // the box that knows what of it they have hold of.
            let laid = match self.writing.as_ref().filter(|(at, _)| *at == index) {
                Some((_, composer)) => composer.laid(room),
                None => crate::composer::wrapped(&note.said, room),
            };
            let mut lines = laid.into_iter();
            let first: Laid = lines.next().unwrap_or_default();
            // A blank above every note but the first on the page: what
            // one note stops saying and the next starts is the one thing
            // this list has to make plain, and a row of nothing says it
            // in both front ends. The window draws a seam through it as
            // well -- see `shapes::parted` -- which is the same boundary
            // said a second way, for the front end that can.
            if !rows.is_empty() {
                rows.push(Row {
                    note: index,
                    said: String::new(),
                    head: false,
                    gap: true,
                    place: false,
                    done: note.done,
                    depth: note.depth,
                    held: None,
                    under: None,
                });
            }
            rows.push(Row {
                note: index,
                said: first.said,
                head: true,
                gap: false,
                place: false,
                done: note.done,
                depth: note.depth,
                held: first.held,
                under,
            });
            for line in lines {
                rows.push(Row {
                    note: index,
                    said: line.said,
                    head: false,
                    gap: false,
                    place: false,
                    done: note.done,
                    depth: note.depth,
                    held: line.held,
                    // Only the note's own row carries it: the mark is
                    // about the note, and one on every line of it would
                    // be a column of arrows saying the same thing.
                    under: None,
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
                    gap: false,
                    place: true,
                    done: note.done,
                    depth: note.depth,
                    // Never: it is a fact about the note rather than a word
                    // of it, and it is not the reader's to take a copy of
                    // by selecting it.
                    held: None,
                    under: None,
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
    /// -- Obelus does not write one down -- and then the caret goes to the
    /// nearest note there still is, because a list with notes in it and no
    /// caret anywhere is a list no key can reach.
    fn settle(&mut self) -> bool {
        let Some((at, composer)) = self.writing.as_ref() else {
            return false;
        };
        let (at, said) = (*at, obelus_git::todo::trimmed(&composer.text()));
        if said.trim().is_empty() {
            self.writing = None;
            let did = self.let_the_note_go(at);
            self.rebuild();
            if !self.todo.notes.is_empty() {
                self.enter_note(at.min(self.todo.notes.len() - 1), false);
            }
            return did;
        }
        match self.written_down(at, &said) {
            Some(change) => self.change(change),
            None => false,
        }
    }

    /// Puts the caret in one of the notes, by which note it is.
    ///
    /// For a pointer: the keys walk from note to note and have no use for
    /// naming one outright, and a press names one. At its start, because a
    /// press that was about the note rather than about a place in its words
    /// has said nothing about where in them to stand.
    pub fn stand_on(&mut self, note: usize) {
        self.enter_note(note, false);
    }

    /// The note that is on the page in this one's place.
    ///
    /// Itself, unless it hangs under something folded -- then the note
    /// standing in for it is the outermost folded one above it, which is
    /// the row the reader can see. Asked of what is shut rather than of
    /// the rows, because a caller may be moving the caret to a note the
    /// rows do not have yet: one just added, one just moved.
    ///
    /// A caret in a note that is folded away is a caret with no row to sit
    /// on, and nothing draws one -- which is what walking down off a
    /// folded note did.
    fn on_the_page(&self, at: usize) -> usize {
        let Some(note) = self.todo.notes.get(at) else {
            return at;
        };
        let mut depth = note.depth;
        let mut shown = at;
        for (above, note) in self.todo.notes[..at].iter().enumerate().rev() {
            // Only the notes this one hangs under, outwards: anything at
            // this depth or deeper is beside it rather than over it.
            if note.depth >= depth {
                continue;
            }
            depth = note.depth;
            if self.shut.contains(&note.id) {
                shown = above;
            }
            if depth == 0 {
                break;
            }
        }
        shown
    }

    /// Puts the caret in a note, keeping whatever the last one said.
    ///
    /// The commit happens here rather than on a key, because leaving a note
    /// *is* finishing it: there is no other moment, and asking the reader to
    /// mark one would be the mode again under another name.
    fn enter_note(&mut self, to: usize, end: bool) {
        // Never into a note that is folded away. Every way the caret moves
        // comes through here, so this is the one place that has to know it
        // -- and the alternative is each of them remembering.
        let to = self.on_the_page(to);
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
        // Never the place and never the blank: neither is a line of the
        // note, and a caret standing on one would be a caret in text the
        // reader cannot change.
        let row = match end {
            true => self
                .rows
                .iter()
                .rposition(|row| row.note == to && row.words()),
            false => self
                .rows
                .iter()
                .position(|row| row.note == to && row.words()),
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
            return self.let_the_note_go(at);
        }
        match self.written_down(at, &said) {
            Some(change) => self.change(change),
            None => false,
        }
    }

    /// Whether the box has no character in it.
    ///
    /// Not [`Self::keep`]'s question, which is whether there is anything
    /// worth writing down: that one trims, and a note of two spaces is
    /// nothing to it. This one is about whether backspace has a character
    /// to take.
    fn says_nothing(&self) -> bool {
        self.writing
            .as_ref()
            .is_some_and(|(_, composer)| composer.text().is_empty())
    }

    /// Puts a run of text into the note the caret is in, over whatever is
    /// held.
    ///
    /// A note of its own where there is none to be in: a reader who pastes
    /// into an empty page meant to start one.
    pub fn paste(&mut self, what: &str) {
        // The same refusal `ctrl+v` gets, said here as well because a
        // terminal's own paste arrives through this door and not through
        // the key.
        if self.selected_is_elsewhere() {
            return;
        }
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
        // The same question the foot asks: a conversation another window
        // is standing in goes with the note it is about, and the run takes
        // what hangs under it.
        if !self.can_drop() {
            return false;
        }
        self.writing = None;
        // By name, and one name at a time, because a run may hold a note the
        // file has never had: the one the reader has just started. Each note
        // the file *does* have goes by its own name and takes what hangs
        // under it with it; the ones already gone that way are skipped, and
        // a note the file never had leaves the page without the disk being
        // asked about a name it could only answer "somebody else took that
        // away" to.
        let span = 1 + self.todo.under(at);
        let run: Vec<NoteId> = self.todo.notes[at..at + span]
            .iter()
            .map(|note| note.id.clone())
            .collect();
        for id in run {
            if self.todo.find(&id).is_none() {
                continue;
            }
            match self.unwritten.remove(&id) {
                true => {
                    self.only_here(&Change::Remove { id, under: false });
                }
                false => {
                    self.change(Change::Remove { id, under: true });
                }
            }
        }
        self.rebuild();
        if !self.todo.notes.is_empty() {
            self.enter_note(at.min(self.todo.notes.len() - 1), false);
        }
        true
    }

    /// Whether the selected note has anywhere to go, in or out.
    ///
    /// The rules are the notes' own -- the top, the note above, and the
    /// deepest a note may be -- and this is the view asking them about the
    /// note the caret is on. Both the key that does it and whatever says
    /// whether the key would do anything ask this, so a key drawn lit is a
    /// key that moves something.
    #[must_use]
    pub fn can_shift(&self, outwards: bool) -> bool {
        self.selected()
            .is_some_and(|at| !self.run_is_elsewhere(at) && self.todo.can_shift(at, outwards))
    }

    /// Whether the selected note is this window's to take away.
    ///
    /// Itself or anything under it being somebody else's is enough: the key
    /// takes the whole run, so a locked note in it is a key that must do
    /// nothing rather than half of what it says.
    #[must_use]
    pub fn can_drop(&self) -> bool {
        self.selected().is_some_and(|at| !self.run_is_elsewhere(at))
    }

    /// Takes the selected note, and everything under it, a level in or out.
    ///
    /// Says whether it moved. The subtree keeps its shape: every note in it
    /// shifts by the same step, so a child that was two under its parent
    /// still is.
    fn shift(&mut self, out: bool) -> bool {
        let Some(at) = self.selected() else {
            return false;
        };
        // The same question the foot asks, so a key the foot offered cannot
        // refuse and one it held back cannot fire.
        if !self.can_shift(out) {
            return false;
        }
        let Some(id) = self.todo.notes.get(at).map(|note| note.id.clone()) else {
            return false;
        };
        self.change(Change::Shift { id, out });
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

    /// Moves the selected note, and everything under it, over its
    /// neighbour.
    ///
    /// Says whether there was anywhere to go. The whole run, because a note
    /// and its children are one thing to move: stepping over one note at a
    /// time would put this one in the middle of somebody else's children.
    /// The first of a parent's children has nobody above it at its level and
    /// nowhere to go, which is what `shift+tab` is for.
    fn move_over(&mut self, up: bool) -> bool {
        let Some(at) = self.selected() else {
            return false;
        };
        // This note alone: what rides along keeps its words, its depth and
        // its name, so a locked child carried past a neighbour by its
        // parent has not been changed.
        if self.selected_is_elsewhere() {
            return false;
        }
        let neighbour = match up {
            true => self.todo.before_it(at),
            false => self.todo.after_it(at),
        };
        let (Some(id), Some(over)) = (
            self.todo.notes.get(at).map(|note| note.id.clone()),
            neighbour
                .and_then(|it| self.todo.notes.get(it))
                .map(|note| note.id.clone()),
        ) else {
            return false;
        };
        if !self.change(Change::Move {
            id: id.clone(),
            over,
            up,
        }) {
            return false;
        }
        // The caret goes with the note, which is the whole point of the key.
        if let Some(now) = self.todo.find(&id) {
            self.enter_note(now, false);
        }
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

    /// Whether anything hangs under the note the caret is in.
    ///
    /// The question the key is gated on, and it is about the note rather
    /// than about what is showing: a note with children answers yes whether
    /// they are folded or not, because the key both folds and unfolds.
    /// Asked without doing the work, which is what a requirement has to be.
    #[must_use]
    pub fn can_fold(&self) -> bool {
        self.selected().is_some_and(|at| self.todo.under(at) > 0)
    }

    /// Folds what hangs under the note the caret is in, or unfolds it.
    ///
    /// Says whether it did anything, so a key that reached a note with
    /// nothing under it stays silent rather than saying a word about a
    /// thing it did not do.
    pub fn toggle_fold(&mut self) -> bool {
        let Some(at) = self.selected().filter(|at| self.todo.under(*at) > 0) else {
            return false;
        };
        let Some(id) = self.todo.notes.get(at).map(|note| note.id.clone()) else {
            return false;
        };
        if !self.shut.remove(&id) {
            self.shut.insert(id);
        }
        // The caret needs nothing done to it. What folds is what hangs
        // *under* the note it is in, so every row it could be on is a row
        // that is still there -- and what moved is below it. Putting it
        // back on the note looked like care and was a second answer to a
        // question nobody had asked.
        self.rebuild();
        true
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
            .position(|row| row.note == *note && row.words())
    }

    /// Whatever a key means here.
    ///
    /// The letters go in the note the caret is in, always: this page is a
    /// page of notes being written. What acts on a note *as a note* -- tick
    /// it, go where it points, move it, take it away -- is under `alt`,
    /// which is the question alt asks everywhere in Obelus: about the thing
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
            KeyCode::Tab if bare => match self.shift(false) {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },
            KeyCode::BackTab => match self.shift(true) {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },

            // Another note, which is what enter means in a page being
            // written. Where it points is `alt+o`: this is not a list of
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
                let note = Note {
                    id: obelus_git::todo::NoteId::mint(),
                    said: String::new(),
                    done: false,
                    at: None,
                    depth,
                };
                // In nobody's file until it says something, which is what
                // makes it the page's to insert rather than the page's to
                // change: see [`Self::written_down`].
                self.unwritten.insert(note.id.clone());
                self.todo.notes.insert(after, note);
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
                // The next note that is *on the page*: what hangs under a
                // folded one is not somewhere to stand, the same as a
                // folded run of lines in a file. Stepping by one index
                // walked straight into one, and the caret went out.
                let to = match down {
                    true => (at + 1..self.todo.notes.len()).find(|to| self.on_the_page(*to) == *to),
                    false => (0..at).rev().find(|to| self.on_the_page(*to) == *to),
                };
                let Some(to) = to else {
                    return TodoOutcome::Consumed;
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
            KeyCode::Char(' ') if alt => match self
                .selected()
                .filter(|_| !self.selected_is_elsewhere())
                .and_then(|at| self.todo.notes.get(at))
                .map(|note| (note.id.clone(), !note.done))
            {
                Some((id, done)) => {
                    self.change(Change::Done { id, done });
                    self.rebuild();
                    TodoOutcome::Changed
                }
                None => TodoOutcome::Consumed,
            },
            // A line in the note, by either of the two keys every box takes
            // for one: `alt+enter` is the one that arrives where
            // `shift+enter` does not, inside tmux among others. Here rather
            // than left to the box, because neither is typing to
            // `typing_for` -- so the lock below let a line into a note
            // somebody else is talking about.
            KeyCode::Enter if alt || key.modifiers == KeyModifiers::SHIFT => {
                if self.selected_is_elsewhere() {
                    return TodoOutcome::Consumed;
                }
                if let Some((_, composer)) = self.writing.as_mut() {
                    composer.newline();
                    self.rebuild();
                    self.follow_caret();
                }
                TodoOutcome::Consumed
            }
            // Where it points. A note about the project has nowhere to go,
            // and nothing is the honest answer. On `o` for open, because
            // `alt+enter` is the line break here as in every other box.
            KeyCode::Char('o') if alt => match self.selected().and_then(|at| {
                let place = self.todo.notes.get(at)?.at.as_ref()?;
                Some((place.path.clone(), (*self.where_now.get(at)?)?))
            }) {
                Some((path, line)) => {
                    // Written down and stayed in, not left: the reader is
                    // leaving the page rather than the note, and the page
                    // is one they come back to -- by the key that opens
                    // the notes, which goes back to this view as they left
                    // it. Left by `keep`, they came back to a page with no
                    // box in it and no key that could open one.
                    self.settle();
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
                    // The same: a conversation about a note is somewhere
                    // the reader goes *from* this page and comes back to
                    // it, so the box stays where they left it.
                    self.settle();
                    TodoOutcome::Talk(id)
                }
                None => TodoOutcome::Consumed,
            },
            KeyCode::Backspace | KeyCode::Delete if alt => match self.take_note_away() {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },
            // And on their own they take a character while there is one,
            // and the note when there is not. Which is the same act a
            // reader means by them in every outline they have used, and
            // is nothing new here either: a note with nothing in it is
            // already a note Obelus does not keep -- `keep` lets it go the
            // moment the caret leaves -- so what this adds is the leaving,
            // in the direction the key names. Backspace to the end of the
            // note above, delete to the start of the one below.
            //
            // Empty means the box holds no character, not `keep`'s
            // question of whether there is anything worth writing down.
            // They differ over a note of two spaces, and the key's is the
            // right one of the two: a reader who has typed a space has
            // something for backspace to take, and taking the note out
            // from under them instead is the key doing the larger thing
            // when the smaller one was available.
            KeyCode::Backspace | KeyCode::Delete
                if bare && self.says_nothing() && !self.selected_is_elsewhere() =>
            {
                let back = key.code == KeyCode::Backspace;
                let Some(at) = self.selected() else {
                    return TodoOutcome::Consumed;
                };
                // The next note that is on the page, the way the arrows
                // find one: what hangs under a folded note is nowhere to
                // go.
                let to = match back {
                    true => (0..at).rev().find(|to| self.on_the_page(*to) == *to),
                    false => {
                        (at + 1..self.todo.notes.len()).find(|to| self.on_the_page(*to) == *to)
                    }
                };
                // Nowhere to go leaves it alone: an empty note with no
                // neighbour is the whole list, and a page with nothing on
                // it is what taking it away would leave -- which is what
                // the page already looks like, with nowhere for the caret
                // to be.
                let Some(id) = to
                    .and_then(|to| self.todo.notes.get(to))
                    .map(|n| n.id.clone())
                else {
                    return TodoOutcome::Consumed;
                };
                // By name over the drop, not by position: letting the
                // empty note go moves every note under it up one, and the
                // one being walked to is under it whenever the key is
                // delete.
                let went = self.keep();
                if let Some(now) = self.todo.find(&id) {
                    self.enter_note(now, back);
                }
                match went {
                    true => TodoOutcome::Changed,
                    false => TodoOutcome::Consumed,
                }
            }
            // Where a note sits is the reader's to decide, so nothing else
            // reorders the list: ticking one leaves it where it is.
            KeyCode::Up | KeyCode::Down if alt => {
                self.keep();
                // Over the neighbour at this note's own level, and over the
                // whole of what hangs under *it*. Nowhere to go leaves the
                // caret where it was: the first of a parent's children has
                // nobody above it at its level, which is what `shift+tab`
                // is for.
                if !self.move_over(key.code == KeyCode::Up) {
                    if let Some(at) = self.selected() {
                        self.enter_note(at, false);
                    }
                    return TodoOutcome::Consumed;
                }
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
                // Both halves of it change the note: the selection taken
                // out of its words, or the whole of it taken away.
                if self.selected_is_elsewhere() {
                    return TodoOutcome::Consumed;
                }
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
            KeyCode::Char('v') if control => match self.selected_is_elsewhere() {
                true => TodoOutcome::Consumed,
                false => TodoOutcome::Paste,
            },
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
                // Moving about in it, selecting and copying out of it all
                // still work -- what the lock is about is changing the
                // note. The two are told apart by the same pair of
                // functions the box itself asks: a key with a motion in it
                // is the caret's, and one with typing in it is the text's.
                if self.selected_is_elsewhere() && obelus_editing::typing_for(key).is_some() {
                    return TodoOutcome::Consumed;
                }
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
            .position(|row| row.note == *at && row.words())
        else {
            return;
        };
        self.window
            .set_focus((first + line).min(self.rows.len().saturating_sub(1)));
    }
}
