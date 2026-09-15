//! What obelus says about a file's history.
//!
//! The reading is [`crate::git`]; what is here is when to ask it, what to
//! keep, and how the answers reach the views.

use super::*;

impl App {
    /// What has changed in the current file, if obelus can tell.
    #[must_use]
    pub fn changes(&self) -> Option<&git::Changes> {
        self.changes.as_ref().map(|changed| &changed.changes)
    }

    /// The hunk the reader has opened in place, if any.
    ///
    /// The buffer holds it: its removed lines are rows of that file's
    /// screen and a place its caret can be, so the two things that need
    /// them are both in there.
    #[must_use]
    pub fn opened_hunks(&self) -> Vec<LineNumber> {
        self.current_buffer().map_or_else(Vec::new, |buffer| {
            buffer.blocks().iter().map(|block| block.above).collect()
        })
    }

    /// Opens what changed at the cursor, in place, or closes it again.
    ///
    /// In place rather than in a panel: what a changed line means is what it
    /// replaced, and the two belong next to each other. The removed lines
    /// push the file's lines down while they are open, which is what makes
    /// it obvious they are not part of the file.
    pub fn toggle_hunk(&mut self) {
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            self.note = Some("no file open".to_string());
            return;
        };
        // Which block is in front of the reader, and it is not always the
        // one hanging above the line they are on. The caret's own comes
        // first; then the one belonging to the hunk they are standing in,
        // which hangs above that hunk's *first* line however far down it
        // they have walked; then one hanging just below them, which is
        // where a reader who walked out of the top of one is left.
        let hunk = self
            .changes()
            .and_then(|changes| changes.hunk_at(line))
            .cloned();
        let anchor = hunk.as_ref().map(|hunk| hunk.line);
        let open = self.current_buffer().and_then(|buffer| {
            buffer.caret_block().or_else(|| {
                anchor
                    .filter(|anchor| buffer.block_above(*anchor).is_some())
                    .or_else(|| buffer.block_at_cursor())
                    .or_else(|| buffer.block_below_cursor())
            })
        });
        // Only rows this key put there answer to it. A commit's message is
        // a block too, and it is the thing the reader opened this version of
        // the file to read -- closing it to answer a question about one line
        // would take away the answer to the question they came with.
        if let Some(above) = open {
            let kind = self
                .current_buffer()
                .and_then(|buffer| buffer.block_above(above))
                .map(|block| block.kind);
            if kind == Some(crate::buffer::Held::Removed)
                && let Some(buffer) = self.current_buffer_mut()
            {
                buffer.close_block(above);
                return;
            }
            if kind == Some(crate::buffer::Held::Message) {
                // A line has room for one block, and this line's is spoken
                // for. Said rather than done quietly: the margin says this
                // line changed, so a key that asks what it changed *from*
                // and appears to do nothing is a key that looks broken.
                self.note = Some("the commit's message hangs where this hunk would".to_string());
                return;
            }
        }
        let Some(hunk) = hunk.as_ref() else {
            self.note = Some("nothing changed here".to_string());
            return;
        };
        // Every hunk opens, including one that replaced nothing: opening it
        // is what puts the change type behind the lines, and "which lines
        // exactly are new here" is a question the margin's one column cannot
        // answer. An added hunk simply has nothing to show above itself.
        let (anchor, removed) = (hunk.line, hunk.removed.clone());
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.open_block(anchor, &removed);
        }
    }

    /// Moves the cursor to the change above it.
    pub fn go_to_previous_change(&mut self) {
        self.go_to_change(false);
    }

    /// Moves the cursor to the change below it.
    pub fn go_to_next_change(&mut self) {
        self.go_to_change(true);
    }

    /// Moves the cursor to the first line of the next change one way or the
    /// other.
    ///
    /// No wrapping. A reader who steps past the last change and lands back
    /// at the top has lost their place to a keystroke that looked like it
    /// did nothing; the command is not offered when there is nothing that
    /// way, which is the honest version of the same information.
    fn go_to_change(&mut self, forward: bool) {
        // Nothing said on the way out of any of these: the command is not
        // offered unless there is a change that way, so a reader can only
        // arrive here with one -- and a note nobody can see is a sentence
        // written for nobody.
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            return;
        };
        let target = self.changes().and_then(|changes| {
            if forward {
                changes.hunk_after(line)
            } else {
                changes.hunk_before(line)
            }
            .map(|hunk| hunk.line)
        });
        let Some(target) = target else {
            return;
        };

        // A jump, so `go-back` comes back: stepping to a change is a leap
        // across the file, the same as typing a line number.
        let from = self.here();
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.place_cursor(target, CharColumn::new(0));
            // Centred only when the change was somewhere else entirely, the
            // same as arriving at a bracket: a hunk already on screen is a
            // short hop, and moving the view for it throws away the
            // reader's place.
            if buffer.cursor_screen_cell(area).is_none() {
                buffer.center_on_cursor(area);
            }
        }
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Who last changed each line of the file being read, if the answer has
    /// arrived and the reader wants to see it.
    ///
    /// One entry per line of the version that was blamed: for the file on
    /// disk the caller maps a line onto it, because the two are not the same
    /// file once anything has changed since the commit; for a commit's own
    /// version they are the same file and the lines line up.
    /// Read straight from the setting, which is the only thing that says
    /// whether the names are wanted. It was a field here as well, set from
    /// the setting at startup and flipped by a command -- so the command's
    /// answer lasted until the next time anything on the settings page
    /// changed, and then went back without a word.
    #[must_use]
    pub fn blame(&self) -> Option<&[Option<git::Blamed>]> {
        if !self.settled.config.blame_margin {
            return None;
        }
        self.blamed_lines()
    }

    /// The blame of the version being read, whether or not its names are
    /// wanted in the margin.
    ///
    /// The setting says whether to *write a name beside every line*, which
    /// is a question about the margin. Whether the commit behind one line
    /// can be asked for is a different question, and a reader who turned
    /// the names off has not said they never want to know.
    ///
    /// The version as well as the path: a file and that file as some commit
    /// had it share a path, and the answer about one laid beside the other
    /// would name whoever last touched whatever is at those numbers in the
    /// other -- a confident answer about the wrong lines.
    #[must_use]
    pub fn blamed_lines(&self) -> Option<&[Option<git::Blamed>]> {
        let buffer = self.current_buffer()?;
        self.blames
            .get(&(buffer.path().to_path_buf(), buffer.content().at()))
            .map(Vec::as_slice)
    }

    /// What the blame says about one line of the text on screen.
    ///
    /// The line is carried onto the version that was blamed first, which is
    /// the same arithmetic the margin does -- and the same call, so the two
    /// cannot come to disagree about which line a name belongs to.
    #[must_use]
    pub fn blamed_at(&self, line: crate::coordinates::LineNumber) -> Option<&git::Blamed> {
        let here = self
            .current_buffer()
            .is_some_and(|buffer| buffer.content().at().is_some());
        let at = git::blame::line_of(line, self.changes(), here)?;
        self.blamed_lines()?.get(at.get())?.as_ref()
    }

    /// Starts a walk of history for the file being read, once per file.
    ///
    /// Only when the names are wanted: a reader who turned them off is not
    /// paying for a walk of every file they open. The key that asks about
    /// one line asks for it itself.
    pub(super) fn refresh_blame(&mut self) {
        if !self.settled.config.blame_margin {
            return;
        }
        self.ask_blame();
    }

    /// Asks who wrote the version being read, if nobody has asked yet.
    pub(super) fn ask_blame(&mut self) {
        let Some(asked) = self
            .current_buffer()
            .map(|buffer| (buffer.path().to_path_buf(), buffer.content().at()))
        else {
            return;
        };
        if self.blames.contains_key(&asked) || !self.asking_blame.insert(asked.clone()) {
            return;
        }
        if let Some(sender) = self.events.clone() {
            git::blame::spawn_blame(&asked.0, asked.1, sender);
        }
    }

    /// Throws away what git said, because the repository has moved.
    ///
    /// A commit, a checkout, a stage -- in another obelus or in a shell.
    /// What has changed in a file is a question about the file *and* about
    /// the commit it is being compared with, and the caches here are keyed
    /// only by the file: without this the margin goes on showing a diff
    /// against a commit that is no longer the one the file is against, and
    /// goes on showing it until the reader types something.
    pub(super) fn forget_what_git_said(&mut self) {
        tracing::info!("the repository moved, so what it said about it is dropped");
        self.changes = None;
        self.committed = None;
        self.blames.clear();
        self.asking_blame.clear();
        // And a history on screen is about the repository that moved.
        self.reread_history();
    }

    /// Asks git what has changed, if it has not already been asked about
    /// this version of this file.
    pub(super) fn refresh_changes(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.changes = None;
            return;
        };
        // The content as well as the path and the version: a commit's
        // version of a file and the file itself share a path, and both
        // start at version one, so a key of the first two would hand one
        // buffer's diff to the other.
        let at = (
            buffer.path().to_path_buf(),
            buffer.version(),
            buffer.content().clone(),
        );
        if self
            .changes
            .as_ref()
            .is_some_and(|changed| changed.at == at)
        {
            return;
        }

        // What this text is a change *from*. For the file on disk that is
        // the last commit; for a commit's version of it that is the commit
        // before -- so the margin beside it says what that commit did,
        // rather than how it differs from today, which is a question about
        // a file the reader is not looking at.
        //
        // Read once per file rather than once per keystroke: it is opening
        // the repository, finding the commit, walking its tree and
        // unpacking the blob, and the answer moves only when the repository
        // does -- which [`App::forget_what_git_said`] is already told about.
        let asked = (buffer.path().to_path_buf(), buffer.content().at());
        // Taken before the cache below is touched, which ends the borrow of
        // the buffer: what is being compared is this text, whether or not
        // the other side of the comparison has to be read again.
        let now = buffer.text().rope().to_string();
        let asked_path = asked.0.clone();
        if self
            .committed
            .as_ref()
            .is_none_or(|committed| committed.of != asked)
        {
            let text = match asked.1 {
                Some(id) => crate::git::history::text_before(&self.working_directory, id, &asked.0),
                None => git::head_text(&asked.0),
            };
            self.committed = Some(crate::app::Committed { of: asked, text });
        }
        let before = self
            .committed
            .as_ref()
            .and_then(|committed| committed.text.as_deref());
        // No text to compare with is every way this can have no answer --
        // not a repository, a file git has never heard of, no commits yet,
        // a commit that added the file -- and they all mean the same thing
        // in the margin: nothing to say.
        let changes = before.map(|committed| Changed {
            changes: git::Changes::between(committed, &now),
            at,
        });
        if changes.is_none() {
            // Said once per file, because "why is the margin empty" is a
            // question with no other answer on screen.
            tracing::debug!(
                path = %asked_path.display(),
                "nothing committed to compare with, so no changes"
            );
        }
        self.changes = changes;
        // A hunk that was open belonged to the diff that has just been
        // replaced. Leaving it open would show removed lines that are no
        // longer removed anywhere.
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.close_blocks(crate::buffer::Held::Removed);
        }
    }
}

/// A file's changes, and which version of which file they are about.
#[derive(Debug)]
pub(super) struct Changed {
    changes: git::Changes,
    at: (PathBuf, i32, crate::buffer::Content),
}
