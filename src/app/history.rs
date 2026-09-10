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
    #[must_use]
    pub const fn opened_hunk(&self) -> Option<LineNumber> {
        self.opened
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
        let Some(hunk) = self.changes().and_then(|changes| changes.hunk_at(line)) else {
            self.note = Some("nothing changed here".to_string());
            return;
        };
        // Every hunk opens, including one that replaced nothing: opening it
        // is what puts the change type behind the lines, and "which lines
        // exactly are new here" is a question the margin's one column cannot
        // answer. An added hunk simply has nothing to show above itself.
        let anchor = hunk.line;
        self.opened = if self.opened == Some(anchor) {
            None
        } else {
            Some(anchor)
        };
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
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            self.note = Some("no file open".to_string());
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
            self.note = Some(if forward {
                "no change below here".to_string()
            } else {
                "no change above here".to_string()
            });
            return;
        };

        // A jump, so `go.back` comes back: stepping to a change is a leap
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
    /// One entry per line of the *committed* file: the caller maps a line of
    /// the working tree onto it, because the two are not the same file once
    /// the reader has changed anything.
    #[must_use]
    pub fn blame(&self) -> Option<&[Option<git::Blamed>]> {
        if !self.showing_blame {
            return None;
        }
        let path = self.current_buffer()?.path();
        self.blames.get(path).map(Vec::as_slice)
    }

    /// Shows or stops showing who changed each line.
    pub fn toggle_blame(&mut self) {
        self.showing_blame = !self.showing_blame;
        if self.showing_blame {
            self.refresh_blame();
        } else {
            self.note = Some("not showing who changed each line".to_string());
        }
    }

    /// Starts a walk of history for the file being read, once per file.
    pub(super) fn refresh_blame(&mut self) {
        if !self.showing_blame {
            return;
        }
        let Some(path) = self
            .current_buffer()
            .map(|buffer| buffer.path().to_path_buf())
        else {
            return;
        };
        if self.blames.contains_key(&path) || !self.asking_blame.insert(path.clone()) {
            return;
        }
        if let Some(sender) = self.events.clone() {
            git::blame::spawn_blame(&path, sender);
        }
    }

    /// Asks git what has changed, if it has not already been asked about
    /// this version of this file.
    pub(super) fn refresh_changes(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.changes = None;
            return;
        };
        let at = (buffer.path().to_path_buf(), buffer.version());
        if self
            .changes
            .as_ref()
            .is_some_and(|changed| changed.at == at)
        {
            return;
        }

        // No committed text is every way this can have no answer -- not a
        // repository, a file git has never heard of, no commits yet -- and
        // they all mean the same thing in the margin: nothing to say.
        self.changes = git::head_text(buffer.path()).map(|committed| Changed {
            changes: git::Changes::between(&committed, &buffer.text().rope().to_string()),
            at,
        });
        // A hunk that was open belonged to the diff that has just been
        // replaced. Leaving it open would show removed lines that are no
        // longer removed anywhere.
        self.opened = None;
    }
}

/// A file's changes, and which version of which file they are about.
#[derive(Debug)]
pub(super) struct Changed {
    changes: git::Changes,
    at: (PathBuf, i32),
}
