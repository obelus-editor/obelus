//! Opening, closing and re-reading files.
//!
//! What a buffer *is* belongs to [`crate::buffer`]; what is here is which
//! ones are open, which one is being read, and what happens when one of
//! them changes on disk.

use super::*;
use crate::buffer::Disk;

impl App {
    /// Offers every file under the working directory.
    pub fn open_file_picker(&mut self) {
        self.open_files(Listing::All);
    }

    /// Offers the files git says have changed, or says that none have.
    pub fn open_changed_files(&mut self) {
        self.open_files(Listing::Changed);
    }

    /// Asks git what has changed in the tree, or takes what a test said.
    fn gather_statuses(&mut self) {
        self.statuses = self.tree_statuses();
    }

    /// What git says about the tree, asked now.
    ///
    /// The walk itself, without keeping the answer: what needs it kept is
    /// the list of files, and what needs it fresh is the question of
    /// whether there is anything to list at all.
    pub(super) fn tree_statuses(&self) -> HashMap<PathBuf, git::FileStatus> {
        match &self.given_statuses {
            Some(given) => given.clone(),
            None => git::statuses(&self.working_directory),
        }
    }

    /// Whether git says anything in the tree has changed.
    ///
    /// What a test said, where a test said anything: the same door
    /// `tree_statuses` goes through, so a test that sets up a clean tree
    /// gets a clean answer here too.
    pub(super) fn anything_changed(&self) -> bool {
        match &self.given_statuses {
            Some(given) => !given.is_empty(),
            None => git::anything_changed(&self.working_directory),
        }
    }

    /// Says what git would say about the tree, for a test.
    ///
    /// The tests run in a checkout whose dirtiness is not theirs to depend
    /// on: with a real answer, a list of files looks one way on a clean tree
    /// and another while someone is working in it -- and the second is the
    /// one anyone runs them on.
    pub fn statuses_for_test(&mut self, statuses: HashMap<PathBuf, git::FileStatus>) {
        self.given_statuses = Some(statuses);
        self.gather_statuses();
    }

    /// Offers files, at one of the two listings.
    ///
    /// Tabs like the search's, and for the same reason: "which file do I
    /// want" and "what have I been working on" are different questions, and
    /// a reader coming back to a project asks the second one first. The
    /// changed listing gets a tab only when something has changed -- a tab
    /// that is always empty in a clean tree is a tab in the way.
    fn open_files(&mut self, listing: Listing) {
        // Asked once, here, rather than per row: `git status` walks the tree
        // and applies every ignore rule on the way, and a list of ten
        // thousand files would ask ten thousand times. It also decides
        // whether there is a second tab at all.
        self.gather_statuses();
        let listings: Vec<Listing> = Listing::ALL
            .into_iter()
            .filter(|shown| *shown == Listing::All || !self.statuses.is_empty())
            .collect();
        let Some(tab) = listings.iter().position(|shown| *shown == listing) else {
            self.note = Some("nothing has changed".to_string());
            return;
        };

        let names: Vec<&str> = listings.iter().map(|listing| listing.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        // Only when there is a second one: a row of tabs with one tab on it
        // says there is somewhere else to go when there is not.
        if listings.len() > 1 {
            picker.with_scopes(&names);
            picker.go_to_tab(tab);
        }
        picker.lists_files();
        self.picker = Some(picker);
        self.listing = listings;
        self.refresh_listing();
    }

    /// Fills a file list with the rows of whichever listing is showing.
    pub(super) fn refresh_listing(&mut self) {
        let showing = self
            .picker
            .as_ref()
            .and_then(|picker| self.listing.get(picker.tab()).copied())
            .unwrap_or(Listing::All);
        match showing {
            Listing::All => {
                // A fresh walk rather than a remembered one: the walk
                // streams and is over in a moment, and keeping a second copy
                // of every path in the tree to switch back to costs more
                // than walking it again.
                self.walk_generation += 1;
                let prefer = self
                    .current_buffer()
                    .map(|buffer| relative(buffer.path(), &self.working_directory));
                if let Some(picker) = self.picker.as_mut() {
                    picker.replace(Vec::new());
                    // Shown for the moment before the first batch arrives as
                    // well as for a tree with nothing in it, which is why it
                    // is about the search rather than about the result.
                    picker.when_empty("no files under this directory");
                    // Open on the file being read. The walk decides where in
                    // the list it is, and it may be in the last batch, so the
                    // picker holds on to the name and selects the row when it
                    // turns up.
                    if let Some(prefer) = prefer {
                        picker.prefer(prefer);
                    }
                }
                if let Some(sender) = self.events.clone() {
                    files::spawn_walk(&self.working_directory, self.walk_generation, sender);
                }
            }
            Listing::Changed => {
                // The walk in flight is answering the other tab's question.
                self.walk_generation += 1;
                let root = self.working_directory.clone();
                let mut rows: Vec<(String, git::FileStatus)> = self
                    .statuses
                    .iter()
                    .map(|(path, status)| (relative(path, &root), *status))
                    .collect();
                // By name, because the order git reports them in is the order
                // it walked the tree, and a list that reorders itself between
                // openings cannot be learned.
                rows.sort_by(|left, right| left.0.cmp(&right.0));
                let items = rows
                    .into_iter()
                    .map(|(name, status)| PickerItem {
                        prose: false,
                        marker: None,
                        icon: Some(icons::for_path(std::path::Path::new(&name))),
                        enabled: true,
                        colours: None,
                        status: Some(status),
                        depth: 0,
                        kind: None,
                        label: name.clone(),
                        detail: None,
                        trailing: None,
                        value: PickerValue::File(std::path::PathBuf::from(name)),
                        tab: None,
                    })
                    .collect();
                if let Some(picker) = self.picker.as_mut() {
                    picker.replace(items);
                    picker.when_empty("nothing has changed");
                }
            }
        }
    }

    /// Offers the files already open.
    pub fn open_buffer_picker(&mut self) {
        // Before the buffers are borrowed to build the rows.
        self.gather_statuses();
        // In the order they were opened, which is the order the slots are
        // in. Not sorted by how often each has been come back to: that
        // reorders the list under a reader between one press of the key and
        // the next, so the row they are reaching for is never where it was
        // last time. A list worth learning is a list that holds still --
        // and the one file whose place they might have to hunt for, the one
        // they are in, is the row the list opens on anyway.
        let open = self
            .buffers
            .iter()
            .enumerate()
            // Closed slots are holes, not rows.
            .filter_map(|(index, buffer)| buffer.as_ref().map(|buffer| (index, buffer)));

        let statuses = &self.statuses;
        let items = open
            .map(|(index, buffer)| PickerItem {
                prose: false,
                // A mark for a document with changes that are not on disk.
                // `marker` rather than `status`, which is git's and colours
                // the whole row: "git says this file changed" and "obelus
                // has not written this" are two different things, and
                // telling them apart is what this is for.
                //
                // The glyph the status row uses for the same fact, so that
                // the file on screen and its row in the list are visibly
                // saying one thing rather than two.
                marker: buffer.is_dirty().then(|| match icons::enabled() {
                    true => icons::ui::UNSAVED.to_string(),
                    false => "\u{2022}".to_string(),
                }),
                icon: Some(icons::for_path(buffer.path())),
                label: relative(buffer.path(), &self.working_directory),
                detail: None,
                // Which commit, for a buffer read from one. Two buffers can
                // wear a path -- the file, and the file as some commit had
                // it -- and without this they are two rows a reader has no
                // way to tell apart, of documents that differ in what they
                // say, in whether they follow the disk, and in what the
                // margin beside them means. The short id and no more: the
                // status row marks the same fact in the same words, so a
                // reader who has seen one has read the other.
                trailing: buffer.content().short(),
                value: PickerValue::Buffer(BufferId::new(index)),
                enabled: true,
                colours: None,
                status: statuses.get(buffer.path()).copied(),
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::FullArea);
        // Reachable with nothing open at all, which is how obelus starts.
        picker.when_empty("no file is open");
        // Opened on the file being read, like the file list: the rows are in
        // most-visited order, so the one the reader is *in* is not
        // necessarily first, and a list that starts somewhere arbitrary
        // makes them find their own file before they can leave it.
        if let Some(buffer) = self.current_buffer() {
            picker.prefer(relative(buffer.path(), &self.working_directory));
        }
        self.picker = Some(picker);
    }

    /// Stops showing the current file.
    ///
    /// The slot stays: [`BufferId`] is an index, and the jump list holds
    /// them. What goes is the file, the language server's copy of it, and
    /// the watch on it -- and the reader is left on whichever file is
    /// nearest, or on the welcome screen if that was the last one.
    pub fn close_current(&mut self) {
        // Asked about rather than done, for the reason leaving is asked
        // about: a closed buffer takes its undo with it.
        let which = self.selected_buffer().or(self.current);
        let unsaved = which
            .and_then(|id| self.buffers.get(id.get()))
            .and_then(Option::as_ref)
            .is_some_and(Buffer::is_dirty);
        if let Some(id) = which
            && unsaved
        {
            self.ask_before_closing(id);
            return;
        }
        // Whichever file the screen is about. With the buffer list open that
        // is the row under the selection, not the file behind it: the list is
        // what the reader is pointing at, and one key that means "close this"
        // everywhere beats a second key that only works in one place.
        if let Some(id) = self.selected_buffer() {
            self.close(id);
            // Rebuilt rather than patched, keeping whatever was typed: a
            // patched list would have to agree with the buffers about which
            // slots are holes.
            let query = self
                .picker
                .as_ref()
                .map(|picker| picker.query().to_string())
                .unwrap_or_default();
            self.open_buffer_picker();
            if let Some(picker) = self.picker.as_mut() {
                picker.set_query(&query);
            }
            return;
        }

        let Some(id) = self.current else {
            self.note = Some("no file to close".to_string());
            return;
        };
        self.close(id);
    }

    /// Moves to a buffer.
    pub(super) fn go_to_buffer(&mut self, id: BufferId) {
        if self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .is_some()
        {
            self.current = Some(id);
        }
    }

    /// The buffer the open picker's selection names, if that is what it is.
    pub(super) fn selected_buffer(&self) -> Option<BufferId> {
        match self.picker.as_ref()?.selected_item()?.value {
            PickerValue::Buffer(id) => Some(id),
            _ => None,
        }
    }

    /// Stops showing one file, whichever the reader is on.
    pub(super) fn close(&mut self, id: BufferId) {
        let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::take) else {
            return;
        };

        // Tell the server before dropping it: the message needs the path, and
        // a server left believing a file is open answers questions about a
        // version that no longer exists anywhere.
        //
        // Not for a commit's version, which was never opened to it -- the
        // three notifications that go the other way all refuse one, and a
        // close for a document nobody announced tells a server to forget
        // the *file* at that path, which is open.
        if buffer.content().is_file()
            && let Some(language) = buffer.language()
            && let Some(client) = self.servers.get_mut(&language)
            && let Ok(uri) = lsp::client::uri_for(buffer.path())
        {
            let _ = client.notify(
                "textDocument/didClose",
                &serde_json::json!({ "textDocument": { "uri": uri } }),
            );
        }
        if let Some(watcher) = self.watcher.as_mut() {
            watcher.unwatch(buffer.path());
        }
        // What the server said this file's tokens were goes with it. The
        // entry would answer correctly for as long as the file stayed shut,
        // and then be one version behind whoever opened it next.
        self.tokens.remove(buffer.path());
        self.note = Some(format!(
            "closed {}",
            relative(buffer.path(), &self.working_directory)
        ));
        drop(buffer);

        // Whichever file is nearest, before the closed one for preference:
        // closing the last of several usually means going back to the one
        // before it.
        if self.current == Some(id) {
            self.current = self.nearest_open(id.get());
        }
    }

    /// The open buffer nearest to a slot, looking back first.
    fn nearest_open(&self, from: usize) -> Option<BufferId> {
        (0..from)
            .rev()
            .chain(from + 1..self.buffers.len())
            .find(|index| self.buffers.get(*index).is_some_and(Option::is_some))
            .map(BufferId::new)
    }

    /// Shows the current file as whatever reading it has, or stops.
    ///
    /// A file with no reading gets a note, which is the honest answer to a
    /// key that cannot do anything here: a reading that does not fit the
    /// bytes produces a screen of nonsense.
    pub fn toggle_preview(&mut self) {
        let showing = self
            .current_buffer()
            .is_some_and(|buffer| buffer.mode() == Mode::Preview);
        if showing {
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.set_mode(Mode::Edit);
            }
            self.rendered = None;
            return;
        }
        if self.reading_of_current().is_none() {
            self.note = Some(match self.current_buffer() {
                Some(_) => "nothing to preview in this file".to_string(),
                None => "no file to preview".to_string(),
            });
            return;
        }
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.show_reading();
        }
    }

    /// Which reading the current file has, if it has one.
    #[must_use]
    pub fn reading_of_current(&self) -> Option<Reading> {
        self.current_buffer().and_then(reading::of)
    }

    /// The reading on screen, if the current file is being shown as one.
    #[must_use]
    pub fn rendering(&self) -> Option<&[reading::Row]> {
        self.rendered
            .as_ref()
            .map(|rendered| rendered.rows.as_slice())
    }

    /// How many rows it has, for the keys that scroll it.
    #[must_use]
    pub fn rendered_rows(&self) -> Option<usize> {
        self.rendering().map(<[_]>::len)
    }

    /// Lays the current file out, if it needs laying out.
    ///
    /// Once per change of text, width or file, not once per frame: the
    /// layout is the expensive part, and a reader scrolling a README would
    /// otherwise pay for it on every row moved.
    pub(super) fn refresh_rendering(&mut self, width: u16) {
        let Some(buffer) = self
            .current_buffer()
            .filter(|buffer| buffer.mode() == Mode::Preview)
        else {
            self.rendered = None;
            return;
        };
        let at = (buffer.path().to_path_buf(), buffer.version(), width);
        if self
            .rendered
            .as_ref()
            .is_some_and(|rendered| rendered.at == at)
        {
            return;
        }
        // Asked on a miss and nowhere else: deciding which reading a file
        // has means reading its first lines, and the answer changes only
        // when one of the three things above does.
        let Some(reading) = reading::of(buffer) else {
            self.rendered = None;
            return;
        };
        let rows = reading::render(reading, &buffer.text().rope().to_string(), width);
        self.rendered = Some(Rendered { at, rows });
    }

    /// Opens a file, or switches to it if it is already open.
    pub(super) fn open(&mut self, path: &Path) {
        // Where the reader was, before they are somewhere else. Opening a
        // file is a leap, and the history is for leaps: without this, a
        // session of opening files leaves nothing to go back *to*, and
        // `go-back` answers "nowhere further back" to a reader who has been
        // three files deep. `push` drops a repeat of the same place, so
        // re-opening the file already being read records nothing.
        let from = self.here();

        if let Some(index) = self
            .buffers
            .iter()
            // The file on disk, not a commit's version of it: those share
            // a path and are different documents, and a reader asking to
            // open the file means the one they can edit elsewhere.
            .position(|buffer| {
                buffer
                    .as_ref()
                    .is_some_and(|open| open.path() == path && open.content().is_file())
            })
        {
            let id = BufferId::new(index);
            if self.current != Some(id) {
                self.record(from);
            }
            self.go_to_buffer(id);
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(watcher) = self.watcher.as_mut()
                    && let Err(error) = watcher.watch(buffer.path())
                {
                    tracing::warn!(%error, path = %buffer.path().display(), "not watching");
                }
                self.buffers.push(Some(buffer));
                let index = self.buffers.len() - 1;
                // Only once the file is known to be readable: a path that
                // turns out to be a directory leaves the reader where they
                // were, and a history entry for a leap that did not happen
                // is a place `go-back` would take them for no reason.
                self.record(from);
                let id = BufferId::new(index);
                self.go_to_buffer(id);
                self.serve(index);
            }
            // A path from the walk can have gone away, or be a file this user
            // cannot read. Neither is a reason to stop.
            Err(error) => tracing::warn!(%error, "could not open"),
        }
    }

    /// Folds what the cursor is in, or unfolds what it is on.
    ///
    /// Silent when there is nothing here to fold, because `AFoldHere` has
    /// already answered that: the key is dim and does nothing, and a note
    /// saying so would be the third answer to a question the palette
    /// settled.
    pub fn toggle_fold(&mut self) {
        let line = self.current_buffer().map(|buffer| buffer.cursor().line);
        if let (Some(buffer), Some(line)) = (self.current_buffer_mut(), line) {
            buffer.toggle_fold(line);
        }
    }

    /// Folds every run the file offers.
    ///
    /// The cursor comes out with the lines, the same as folding one run
    /// does: there is nowhere inside to stand.
    pub fn fold_all(&mut self) {
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.fold_all();
        }
    }

    /// Opens everything that is folded.
    pub fn unfold_all(&mut self) {
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.unfold_all();
        }
    }

    /// Re-reads whichever open buffers came from `path`.
    ///
    /// The watch is on a directory, so most of what arrives here is about
    /// files obelus does not have open.
    /// Opens a path the way choosing it from a list does.
    ///
    /// For a test: the lists that reach this are filled from a walk on
    /// another thread, and a test that pumped the walk to press one key
    /// would be a test of the walk.
    /// Puts a buffer somebody else made into the list, for a test.
    ///
    /// A commit's version is made from bytes git handed over rather than
    /// from a path, so there is no opening it.
    pub fn open_buffer_for_test(&mut self, buffer: Buffer) {
        self.buffers.push(Some(buffer));
        let index = self.buffers.len() - 1;
        self.go_to_buffer(BufferId::new(index));
    }

    /// Opens a path the way choosing it from a list does.
    ///
    /// For a test: the lists that reach this are filled from a walk on
    /// another thread, and a test that pumped the walk to press one key
    /// would be a test of the walk.
    pub fn open_for_test(&mut self, path: &Path) {
        self.open(path);
    }

    pub(super) fn reload_path(&mut self, path: &Path) {
        for index in 0..self.buffers.len() {
            let Some(buffer) = self.buffers[index].as_mut() else {
                continue;
            };
            // A commit's version of a file does not change when the file
            // does: those bytes are what that commit said, and re-reading
            // over them would replace a document the reader chose with one
            // they did not.
            if buffer.path() != path || !buffer.content().is_file() {
                continue;
            }
            // The commonest change reported about an open file is obelus's
            // own save arriving back, and that one looks exactly like the
            // file that was just recorded. Asked before anything is read,
            // because it is one `stat` and the alternative is reading the
            // whole file to learn nothing.
            if !buffer.file_touched() {
                continue;
            }
            // An agent rewriting a file while it is open is the ordinary
            // case and reloading by itself is the whole point of watching.
            // Over a document somebody has edited it is losing their work,
            // so a dirty buffer is marked and left alone: the save is where
            // the two versions meet, and where the reader is asked.
            if buffer.is_dirty() {
                // The same question the save will ask, asked early so the
                // screen can say so. Worth one read of the file: the mark
                // is what tells the reader they will have to choose, and a
                // `touch` or a formatter that changed nothing is not a
                // choice.
                buffer.conflicted();
                continue;
            }
            if reload(buffer) {
                self.change_document(index);
                // The document has stopped moving on a version nobody has
                // classified, which is where the server is worth asking.
                self.ask_tokens(index);
            }
        }
    }

    /// Writes the file being read back to disk.
    pub fn save_current(&mut self) {
        let Some(index) = self.current.map(BufferId::get) else {
            self.note = Some("no file open".to_string());
            return;
        };
        let Some(buffer) = self.buffers.get_mut(index).and_then(Option::as_mut) else {
            return;
        };
        // A commit's version is not a file anybody can write back, and the
        // path it wears belongs to a different document.
        if !buffer.content().is_file() {
            self.note = Some("this is a commit's version, not the file".to_string());
            return;
        }
        if !buffer.is_dirty() {
            self.note = Some("nothing to save".to_string());
            return;
        }
        // The file moved under the reader while they were editing it.
        // Saving now would put their version over somebody else's without
        // either of them being asked, so the reader is asked.
        //
        // Asked of disk here rather than of the flag the watcher set: a
        // file is written once, and the watcher is allowed to have missed
        // it. It is also the place to find out that the file moved back.
        match buffer.conflicted() {
            Disk::Unchanged => self.save_now(index),
            Disk::Written => self.ask_before_saving(BufferId::new(index)),
            // A different question with different answers: there is nothing
            // on a disk that has nothing on it to take instead.
            Disk::Deleted => self.ask_before_writing_back(BufferId::new(index)),
        }
    }

    /// Lays a document out, if that was asked for, and writes it.
    ///
    /// Apart from the command because the answer to a question about a file
    /// that moved comes back here too, having settled the one thing the
    /// command stopped for.
    pub(super) fn save_now(&mut self, index: usize) {
        // Laid out first, where the reader asked for that and somebody can
        // do it. The answer comes back later and writes the file then --
        // the alternative is holding the whole program still waiting for
        // another process to reply.
        if self.settled.config.format_on_save && self.ask_formatting(index) {
            self.note = Some("laying it out\u{2026}".to_string());
            return;
        }
        self.write_now(index);
    }

    /// Throws away what the reader wrote and re-reads the file.
    ///
    /// One of the two ways out of a file that moved. The undo goes with it:
    /// [`Buffer::reload`] forgets, because what could have been put back was
    /// about text that is not there any more.
    pub(super) fn take_what_is_on_disk(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get_mut(index).and_then(Option::as_mut) else {
            return;
        };
        match buffer.take_from_disk() {
            Ok(changed) => {
                if changed {
                    self.change_document(index);
                    self.ask_tokens(index);
                }
                self.note = Some("took what is on disk -- undo brings yours back".to_string());
            }
            Err(error) => {
                tracing::warn!(%error, "taking what is on disk failed");
                self.note = Some("could not read it".to_string());
            }
        }
    }

    /// Writes a document that is ready to be written, and says whether it
    /// went.
    ///
    /// Apart from the command because the formatting answer comes back here
    /// too, and by then everything the command checked has been checked.
    /// It reports because leaving depends on the answer: a save that failed
    /// on the way out is the whole reason the reader was asked.
    pub(super) fn write_now(&mut self, index: usize) -> bool {
        let Some(buffer) = self.buffers.get_mut(index).and_then(Option::as_mut) else {
            return false;
        };
        match buffer.save() {
            Ok(()) => {
                self.note = Some("saved".to_string());
                self.saved_document(index);
                true
            }
            Err(error) => {
                tracing::warn!(%error, path = %buffer.path().display(), "saving failed");
                // Which file, and nothing else. Why it would not go is a
                // path and an error chain, which is longer than the status
                // row has and so would be dropped whole -- and a reader
                // leaving with four files open needs the name most. The
                // whole of it is in the log.
                let name = buffer
                    .path()
                    .file_name()
                    .map_or_else(String::new, |name| format!("{}: ", name.to_string_lossy()));
                self.note = Some(format!("{name}not saved"));
                false
            }
        }
    }

    /// Re-reads the current file and reparses what changed.
    pub fn reload_current(&mut self) {
        let Some(index) = self.current.map(BufferId::get) else {
            return;
        };
        if let Some(buffer) = self.buffers.get_mut(index).and_then(Option::as_mut)
            && reload(buffer)
        {
            self.change_document(index);
            self.ask_tokens(index);
        }
    }
}

/// A reading laid out, and what it was made from.
///
/// The three things it depends on, so that a change to any of them is
/// noticed: which file, which version of it, and how wide the screen was.
#[derive(Debug)]
pub(super) struct Rendered {
    at: (PathBuf, i32, u16),
    rows: Vec<reading::Row>,
}

/// Re-reads one buffer, reporting rather than propagating a failure.
///
/// A file that has been deleted or replaced by a directory leaves the buffer
/// showing what it last held. Losing the contents would be worse than showing
/// something a moment out of date.
/// Re-reads one buffer, and says whether the text changed.
pub(super) fn reload(buffer: &mut Buffer) -> bool {
    match buffer.reload() {
        Ok(true) => {
            tracing::debug!(path = %buffer.path().display(), "reloaded");
            true
        }
        Ok(false) => {
            tracing::trace!(path = %buffer.path().display(), "no change");
            false
        }
        Err(error) => {
            tracing::warn!(%error, path = %buffer.path().display(), "reload failed");
            false
        }
    }
}
