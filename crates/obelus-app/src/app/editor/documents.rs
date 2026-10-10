//! Opening, closing and re-reading what the reader can be in.
//!
//! What a buffer *is* belongs to [`obelus_buffer`]; what is here is which
//! documents are open, which one is being read, and what happens when the
//! file behind one of them changes on disk. Not only files: a conversation
//! is a document too, and the list, the closing and the switching are the
//! same for both -- which is the whole of what [`crate::app::document`] bought.

use obelus_buffer::Disk;

use crate::app::*;

impl App {
    /// One row for an open file.
    fn file_row(
        index: usize,
        buffer: &Buffer,
        statuses: &std::collections::HashMap<PathBuf, obelus_git::Standing>,
        root: &Path,
    ) -> PickerItem {
        PickerItem {
            prose: false,
            // A mark for a document with changes that are not on disk.
            // `marker` rather than `status`, which is git's and colours
            // the whole row: "git says this file changed" and "Obelus
            // has not written this" are two different things, and
            // telling them apart is what this is for.
            //
            // The glyph the status row uses for the same fact, so that
            // the file on screen and its row in the list are visibly
            // saying one thing rather than two.
            marker: buffer.is_dirty().then(|| {
                let glyph = match obelus_icons::enabled() {
                    true => obelus_icons::ui::UNSAVED.to_string(),
                    false => "\u{2022}".to_string(),
                };
                (Marking::Unwritten, glyph)
            }),
            icon: obelus_icons::enabled().then(|| obelus_icons::for_path(buffer.path())),
            label: relative(buffer.path(), root),
            // Which commit, for a buffer read from one. Two buffers can
            // wear a path -- the file, and the file as some commit had
            // it -- and without this they are two rows a reader has no
            // way to tell apart, of documents that differ in what they
            // say, in whether they follow the disk, and in what the
            // margin beside them means. The short id and no more: the
            // status row marks the same fact in the same words, so a
            // reader who has seen one has read the other.
            version: buffer.content().short(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::Document(DocumentId::new(index)),
            enabled: true,
            colours: None,
            // What git says of the file on disk, which a commit's version
            // is not: its row coloured as modified said that commit had
            // changes nobody had committed.
            status: statuses
                .get(buffer.path())
                .filter(|_| buffer.content().is_file())
                .map(|standing| standing.status),
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// What is happening in a conversation that the reader is not
    /// watching.
    ///
    /// The reason a list of conversations animates at all: a mark that only
    /// turns while you are looking at the conversation it is about is a mark
    /// that never turns.
    ///
    /// Its own function because two callers need the same answer and they
    /// ask at different times -- the row is built when the list opens, and
    /// [`App::freshen_the_document_marks`] asks again on every frame the
    /// list is up. Two copies of this `match` would be two lists: the one
    /// that was true when the reader pressed the key, and the one in front
    /// of them.
    fn conversation_mark(
        talk: &crate::conversation::Conversation,
        talker: Option<&obelus_agent::acp::Talk>,
    ) -> Option<(Marking, String)> {
        match (
            talk.card.is_some(),
            talker.is_some_and(|talker| talker.is_thinking(talk.session.as_ref(), talk.requested)),
        ) {
            (true, _) => Some((Marking::Waiting, obelus_icons::ui::READER.to_string())),
            (_, true) => Some((Marking::Working, String::new())),
            _ => None,
        }
    }

    /// Puts today's marks back on a list of open documents.
    ///
    /// Asked every frame a list is up, because what an agent is doing is
    /// the one thing in a list that moves without the reader touching
    /// anything. The rows themselves are not rebuilt: building them asks
    /// git about the whole tree and reads the notes off disk, which is not
    /// work a frame can do -- and that is exactly why the mark went stale,
    /// since the mark was built with them.
    ///
    /// What made it hard to see is that the stale mark still *moved*: the
    /// turning frame comes from the ticker, so a conversation whose turn
    /// had ended went on spinning, and one that started working while the
    /// list was up sat blank however long the reader watched it.
    ///
    /// Only rows that name a conversation. A file's row keeps its own mark
    /// -- there is no typing while a list is up, so nothing it says can
    /// have changed -- and every other list is left alone entirely.
    pub(in crate::app) fn freshen_the_document_marks(&mut self) {
        if self.picker.is_none() {
            return;
        }
        // Worked out before the list is borrowed to change, because both
        // halves are this application's.
        let talker = self.talker.as_ref();
        let whose = self.whose_conversation();
        let marks: Vec<(DocumentId, Option<(Marking, String)>)> = self
            .documents
            .iter()
            .enumerate()
            .filter_map(|(index, document)| {
                let talk = document.as_ref()?.chat()?;
                Some((
                    DocumentId::new(index),
                    Self::conversation_mark(talk, talker),
                ))
            })
            .collect();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.remark(|value| match value {
            PickerValue::Document(id) => marks
                .iter()
                .find(|(whose, _)| whose == id)
                // Always choosable: every row of this list is something
                // open, and going to it is what the list is for.
                .map_or(Remark::Keep, |(_, mark)| {
                    Remark::Now(obelus_component::picker::Said {
                        marker: mark.clone(),
                        enabled: true,
                        trailing: whose.clone(),
                    })
                }),
            _ => Remark::Keep,
        });
    }

    /// Whose an open conversation is, at the end of its row.
    ///
    /// By the name the conversation's header calls the agent, and through
    /// the same function, so the two cannot drift: the id is a word for the
    /// settings file. Asked again on every frame the list is up, because
    /// the agent's own name arrives with its handshake, which can be after
    /// the list opened.
    fn whose_conversation(&self) -> Option<String> {
        self.talker
            .as_ref()
            .and(self.agent_name())
            .map(str::to_string)
    }

    /// One row for an open conversation.
    ///
    /// The agent's own name for it where it has given one, and the note's
    /// first line until then: a note says what the reader set out to do and
    /// the session's title says what the conversation became, and for a
    /// list of what they are *in* the second is the more useful of the two.
    ///
    /// Which costs something and it is worth saying where: `detail` is not
    /// matched by the query and says so on purpose, so a conversation can no
    /// longer be found by typing words from its note. The notes list
    /// searches note titles, and a note reaches its conversation in one key.
    fn conversation_row(
        &self,
        index: usize,
        talk: &crate::conversation::Conversation,
        talker: Option<&obelus_agent::acp::Talk>,
        notes: &obelus_todo::Todo,
    ) -> PickerItem {
        let about = self.conversation_about(talk, notes);
        let titled = self.conversation_title(talk);
        PickerItem {
            prose: true,
            marker: Self::conversation_mark(talk, talker),
            icon: obelus_icons::enabled().then_some(obelus_icons::ui::AGENT),
            label: self
                .conversation_name(talk, notes)
                .unwrap_or_else(|| "A conversation".to_string()),
            // The note it is about, under the name the agent gave it. Two
            // facts that are both worth having: what the reader meant to do,
            // and what came of it.
            version: None,
            detail: titled.and(about),
            // Whose conversation it is: two rows that differ in who is
            // answering are two rows a reader cannot otherwise tell apart.
            trailing: self.whose_conversation(),
            changed: None,
            value: PickerValue::Document(DocumentId::new(index)),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// What a conversation is called: the agent's name for it, or the note
    /// it is about, or the reader's first words, where it has any of them.
    ///
    /// The one answer, which the list of what is open, the line saying one
    /// was closed and the conversation's own header all read, so none of
    /// them can call it something different.
    ///
    /// The first words are for the wait before the agent names it, which is
    /// the whole of the first turn, and a first turn can run for minutes --
    /// two of them side by side were two rows both saying `A conversation`.
    /// They are shown and never written down: the name kept for a
    /// conversation is the agent's.
    pub(in crate::app) fn conversation_name(
        &self,
        talk: &crate::conversation::Conversation,
        notes: &obelus_todo::Todo,
    ) -> Option<String> {
        self.conversation_title(talk)
            .or_else(|| self.conversation_about(talk, notes))
            .or_else(|| talk.chat.first_words())
    }

    /// Whether the agent has named a conversation, which is the one name
    /// that needs nothing read.
    pub(in crate::app) fn has_a_title(&self, talk: &crate::conversation::Conversation) -> bool {
        self.conversation_title(talk).is_some()
    }

    /// What the agent calls a conversation: what it has said, or what
    /// Obelus wrote down the last time it did.
    ///
    /// The second half is for a conversation reopened at the start, which
    /// is not taken up until it is shown -- so until then its agent has
    /// said nothing about it, not even the name Obelus already has.
    fn conversation_title(&self, talk: &crate::conversation::Conversation) -> Option<String> {
        let talker = self.talker.as_ref();
        if let Some(title) = talker.and_then(|talker| talker.title(talk.session.as_ref())) {
            return Some(title.to_string());
        }
        // The agent in use where one has started, and the one the settings
        // name where none has: the one a session is taken up with.
        let agent = talker
            .map(|talker| talker.id().to_string())
            .or_else(|| self.settled.config.agent.clone())?;
        self.sessions()?
            .get(&talk.which()?, &agent, &self.working_directory)?
            .title
            .clone()
    }

    /// The note a conversation is about, by its title -- or the pull request
    /// it reviews.
    fn conversation_about(
        &self,
        talk: &crate::conversation::Conversation,
        notes: &obelus_todo::Todo,
    ) -> Option<String> {
        match &talk.topic {
            crate::conversation::Topic::Note(id) => notes
                .notes
                .iter()
                .find(|note| note.id == *id)
                .map(|note| note.title().to_string()),
            crate::conversation::Topic::PullRequest(number) => {
                Some(self.what_a_review_is_called(*number))
            }
            crate::conversation::Topic::Issue(number) => {
                Some(self.what_an_answer_is_called(*number))
            }
            crate::conversation::Topic::Loose => None,
        }
    }

    /// The notes, as a row of the list of what is open.
    ///
    /// Called `todo` rather than `notes`, which is Obelus's own word for it
    /// in prose: `todo` is what the reader types into the palette, what
    /// `ctrl+t` and `alt+t` stand for, and what the file is called. The mark
    /// is the command's, so the row and the row that opened it wear the same
    /// one.
    fn notes_row(index: usize, notes: &obelus_component::todo::TodoView) -> PickerItem {
        let left = notes.todo().notes.iter().filter(|note| !note.done).count();
        PickerItem {
            prose: false,
            marker: None,
            icon: obelus_icons::enabled()
                .then(|| obelus_icons::for_command(obelus_command::Command::TodoOpen)),
            label: "Todo".to_string(),
            // How many are still to come back to, where a changed file puts
            // how much it moved: it is the one number about this row that
            // says whether it is worth opening.
            version: None,
            detail: None,
            trailing: (left > 0).then(|| left.to_string()),
            changed: None,
            value: PickerValue::Document(DocumentId::new(index)),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// Offers every file under the working directory.
    pub fn open_file_picker(&mut self) {
        self.open_files(Listing::All);
    }

    /// Offers the files git says have changed, or says that none have.
    pub fn open_changed_files(&mut self) {
        self.open_files(Listing::Changed);
    }

    /// Asks git what has changed in the project, or takes what a test said.
    fn gather_statuses(&mut self) {
        self.statuses = self.project_statuses();
    }

    /// What git says about the project, asked now.
    ///
    /// The walk itself, without keeping the answer: what needs it kept is
    /// the list of files, and what needs it fresh is the question of
    /// whether there is anything to list at all.
    pub(in crate::app) fn project_statuses(&self) -> HashMap<PathBuf, obelus_git::Standing> {
        match &self.given_statuses {
            Some(given) => given.clone(),
            None => obelus_git::statuses(&self.working_directory),
        }
    }

    /// Whether git says anything in the tree has changed.
    ///
    /// What a test said, where a test said anything: the same door
    /// `project_statuses` goes through, so a test that sets up a clean project
    /// gets a clean answer here too.
    pub(in crate::app) fn anything_changed(&self) -> bool {
        match &self.given_statuses {
            Some(given) => !given.is_empty(),
            None => obelus_git::anything_changed(&self.working_directory),
        }
    }

    /// Says what git would say about the tree, for a test.
    ///
    /// The tests run in a checkout whose dirtiness is not theirs to depend
    /// on: with a real answer, a list of files looks one way on a clean tree
    /// and another while someone is working in it -- and the second is the
    /// one anyone runs them on.
    pub fn statuses_for_test(&mut self, statuses: HashMap<PathBuf, obelus_git::Standing>) {
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
            self.wrong("Nothing has changed".to_string());
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
        picker.before_typing("Filter files");
        picker.previews();
        // This list has a key of its own, so it says so. The others have
        // none, and a foot saying "enter chooses" would be a row spent on
        // what the reader just did.
        picker.says_its_keys();
        self.show_list(picker);
        self.listing = listings;
        // Open on the file being read, which in a tree means opening every
        // directory above it: a list that opened at the root would make the
        // reader walk down to where they already are.
        //
        // And on the file being read rather than on wherever the list was
        // left: a row kept from the last time it was open is about a query
        // the reader has finished with, and the file they are reading is
        // the thing they have in front of them now.
        self.stood_on = None;
        self.reveal_current();
        // Started once, when the list opens, rather than when the reader
        // first types: the tree is what is on screen until they do, and the
        // walk running behind it is what makes the first keystroke land on
        // a list rather than on an empty one.
        self.start_walk();
        self.refresh_listing();
    }

    /// Opens every directory above the file being read.
    ///
    /// What "open on the file being read" means in a tree. The row itself
    /// is selected by name once the rows exist, which is the picker's own
    /// [`Picker::prefer`].
    fn reveal_current(&mut self) {
        let Some(path) = self
            .current_buffer()
            .map(|buffer| buffer.path().to_path_buf())
        else {
            return;
        };
        let Ok(relative) = path.strip_prefix(&self.working_directory) else {
            return;
        };
        let mut above = relative.parent();
        while let Some(directory) = above {
            if directory.as_os_str().is_empty() {
                break;
            }
            self.opened.insert(directory.to_path_buf());
            above = directory.parent();
        }
    }

    /// Whether the flat listing of everything is what the list is showing.
    ///
    /// Which is the tab those rows are about, with something typed: the
    /// same list is a tree while nothing is, and the other tab is git's
    /// answer rather than a walk's.
    pub(in crate::app) fn showing_found(&self) -> bool {
        let Some(picker) = self.picker.as_ref() else {
            return false;
        };
        !picker.query().is_empty() && self.listing.get(picker.tab()).copied() == Some(Listing::All)
    }

    /// Starts the walk whose paths the flat listing is made of.
    ///
    /// Bumping the generation first is what tells the walk before it --
    /// another opening, another answer about which files to offer -- that
    /// nobody is waiting for it any more.
    pub(in crate::app) fn start_walk(&mut self) {
        let mine = self.walk_generation.next();
        self.found.clear();
        let Some(sender) = self.events.clone() else {
            return;
        };
        obelus_search::spawn_walk(
            &self.working_directory,
            self.walk_generation.claim(mine),
            self.config().ignored_files,
            self.config().hidden_files,
            sender,
        );
    }

    /// Has whatever scoring the list is waiting on done somewhere else.
    ///
    /// The picker asks and this answers, because spawning is the
    /// application's: it is the only part of Obelus that has heard of every
    /// worker, which is the same reason the walk is started here and not in
    /// the list it fills.
    ///
    /// Nothing is cancelled when a newer one is asked for. A scoring holds
    /// no lock and writes nothing, so the cost of one nobody wants is the
    /// thread it finishes on -- and the answer is dropped where the count
    /// is kept, which is the same shape the walk's generations have.
    pub(in crate::app) fn send_the_scan(&mut self) {
        let Some(asked) = self.picker.as_mut().and_then(Picker::wanted_scan) else {
            return;
        };
        let Some(sender) = self.events.clone() else {
            return;
        };
        obelus_runtime::handle().spawn_blocking(move || {
            let _ = sender.send(Event::Scanned(Box::new(obelus_component::picker::scan(
                &asked,
            ))));
        });
    }

    /// Whatever a key means to a file list, beyond moving about in it.
    ///
    /// One key, and it is a setting: which files the list offers. Answered
    /// here rather than in the picker because the picker knows about rows
    /// and a query, and this is about where the rows come from -- and
    /// because a setting is the application's to keep.
    pub(in crate::app) fn listing_key(&mut self, key: &KeyEvent) -> bool {
        if key.modifiers != KeyModifiers::ALT {
            return false;
        }
        // Where the row is, which is a question about the row rather than
        // about the list: it is the one key here that acts on what the
        // reader is standing on.
        if key.code == KeyCode::Char('n') {
            return self.rename_selected();
        }
        // Two switches about which files the list offers, and they keep
        // two different things out: what the project said to ignore, and
        // what a system keeps out of sight. The dot is what a file
        // manager puts the second on, and what it is called after on every
        // machine but one.
        let setting = match key.code {
            KeyCode::Char('i') => "ignored_files",
            KeyCode::Char('.') => "hidden_files",
            _ => return false,
        };
        // Only where the key means something: on the changed tab these rows
        // are git's answer, and the foot greys it there.
        if !self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.offers_ignored().is_some())
        {
            return false;
        }
        // A tree that has pinned it has said so for everybody who opens it,
        // and the row on the settings page says which file did. Here there
        // is no row to say it, so the status bar does.
        if let Some(path) = self.pinned_by(setting) {
            self.wrong(format!("{} says which files to offer", path.display()));
            return true;
        }
        let showing = !self
            .config()
            .value_of(setting)
            .is_some_and(|value| matches!(value, obelus_config::Value::Switch(true)));
        self.change_setting(setting, &obelus_config::Value::Switch(showing));
        // A different answer to "which files", so a different walk.
        self.start_walk();
        self.refresh_listing();
        true
    }

    /// The rows of the flat listing, from what the walk has found.
    fn found_rows(&self) -> Vec<PickerItem> {
        self.found
            .iter()
            .map(|(path, ignored)| PickerItem {
                prose: false,
                marker: None,
                icon: obelus_icons::enabled().then(|| obelus_icons::for_path(path)),
                label: path.display().to_string(),
                version: None,
                detail: None,
                trailing: None,
                changed: None,
                value: PickerValue::File(path.clone()),
                enabled: true,
                colours: None,
                // Only there because the reader asked for it, which is the
                // one thing to say about such a row.
                status: match ignored {
                    true => Some(obelus_git::FileStatus::Ignored),
                    false => self
                        .statuses
                        .get(&self.working_directory.join(path))
                        .map(|standing| standing.status),
                },
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            })
            .collect()
    }

    /// The rows of the tree a file list shows while nothing is typed.
    ///
    /// Walked from the root through whichever directories are open, which
    /// is what makes it a tree rather than a listing: a directory nobody
    /// opened is one row, and the rows under it do not exist.
    fn tree_rows(&self) -> Vec<PickerItem> {
        let ignored = self.config().ignored_files;
        let hidden = self.config().hidden_files;
        let mut rows = Vec::new();
        self.tree_rows_under(&self.working_directory, 0, ignored, hidden, &mut rows);
        rows
    }

    /// The same, for one directory and everything open under it.
    fn tree_rows_under(
        &self,
        directory: &Path,
        depth: u16,
        ignored: bool,
        hidden: bool,
        rows: &mut Vec<PickerItem>,
    ) {
        for entry in
            obelus_search::tree::inside(&self.working_directory, directory, ignored, hidden)
        {
            let open = self.opened.contains(&entry.path);
            let full = self.working_directory.join(&entry.path);
            let name = entry.path.file_name().map_or_else(
                || entry.path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            rows.push(PickerItem {
                prose: false,
                icon: obelus_icons::enabled().then(|| match entry.directory {
                    true => obelus_icons::ui::DIRECTORY,
                    false => obelus_icons::for_path(&entry.path),
                }),
                marker: None,
                // Only where opening it would show something. The mark is
                // the only thing a row says about itself before it is
                // pressed, and one that offers to open an empty directory
                // is one nobody presses twice.
                opens: (entry.directory && entry.holds).then_some(open),
                label: name,
                version: None,
                detail: None,
                trailing: None,
                changed: None,
                value: match entry.directory {
                    true => PickerValue::Directory(entry.path.clone()),
                    false => PickerValue::File(entry.path.clone()),
                },
                depth,
                // What git says about it, or that the tree was told to
                // keep it out: the second is the only thing to say about a
                // row that is only there because the reader asked for it.
                status: match entry.ignored {
                    true => Some(obelus_git::FileStatus::Ignored),
                    false => self.statuses.get(&full).map(|standing| standing.status),
                },
                enabled: true,
                colours: None,
                kind: None,
                tab: None,
                section: None,
            });
            if entry.directory && open {
                self.tree_rows_under(&full, depth.saturating_add(1), ignored, hidden, rows);
            }
        }
    }

    /// Opens a directory of the tree, or closes it again.
    pub(in crate::app) fn open_directory(&mut self, path: &Path) {
        if !self.opened.remove(path) {
            self.opened.insert(path.to_path_buf());
        }
        let row = self.picker.as_ref().map(Picker::selected);
        self.refresh_listing();
        // Back onto the row the key was pressed on: the rows below it have
        // moved, and a selection that jumped to the top would leave the
        // reader somewhere they did not ask to be.
        if let (Some(picker), Some(row)) = (self.picker.as_mut(), row) {
            picker.select_row(row);
        }
    }

    /// Fills a file list with the rows of whichever listing is showing.
    pub(in crate::app) fn refresh_listing(&mut self) {
        let showing = self
            .picker
            .as_ref()
            .and_then(|picker| self.listing.get(picker.tab()).copied())
            .unwrap_or(Listing::All);
        match showing {
            // Nothing typed: the tree of the project, read rather than
            // searched. Typing is how a reader says they know what they are
            // looking for, and until they do the shape of the tree is what
            // there is to go on.
            Listing::All if self.picker.as_ref().is_some_and(|it| it.query().is_empty()) => {
                let rows = self.tree_rows();
                let ignored = self.config().ignored_files;
                let hidden = self.config().hidden_files;
                let prefer = self
                    .current_buffer()
                    .and_then(|buffer| buffer.path().file_name())
                    .map(|name| name.to_string_lossy().into_owned());
                // Where the reader was before they typed, if that row is
                // still in the tree. By its path rather than by its name,
                // which is what the tree draws: half the rows in a project
                // are called `mod.rs`.
                let back = self.stood_on.take().and_then(|path| {
                    rows.iter().position(|row| match &row.value {
                        PickerValue::File(at) | PickerValue::Directory(at) => *at == path,
                        _ => false,
                    })
                });
                if let Some(picker) = self.picker.as_mut() {
                    picker.offering_ignored(Some(ignored));
                    picker.offering_hidden(Some(hidden));
                    // The same: the row a query's answer was on says
                    // nothing about where it is in the tree. What puts the
                    // reader back is the name.
                    picker.replace(rows);
                    picker.when_empty("No files under this directory");
                    match back {
                        // A query that came and went is not a reason to
                        // move, so the row the reader left wins over the
                        // file they have open -- which is only ever a guess
                        // about where they would like to start.
                        Some(row) => picker.select_row(row),
                        None => {
                            if let Some(prefer) = prefer {
                                picker.prefer(prefer);
                            }
                        }
                    }
                }
            }
            // Something typed: the flat list of everything, which is what a
            // query is asked against.
            Listing::All if !self.found.is_empty() => {
                let rows = self.found_rows();
                let ignored = self.config().ignored_files;
                let hidden = self.config().hidden_files;
                if let Some(picker) = self.picker.as_mut() {
                    picker.offering_ignored(Some(ignored));
                    picker.offering_hidden(Some(hidden));
                    // Replaced rather than relisted: these are not the tree
                    // with more in it, they are a different question's
                    // answer, and the row the reader was on in the tree is
                    // a number that means nothing here. The top is where a
                    // query's answer starts.
                    picker.replace(rows);
                }
            }
            // Something typed, and the walk has not reached anything yet.
            // Said about the search rather than about the result, because
            // this is also what a tree with nothing in it looks like.
            Listing::All => {
                let ignored = self.config().ignored_files;
                let hidden = self.config().hidden_files;
                let prefer = self
                    .current_buffer()
                    .map(|buffer| relative(buffer.path(), &self.working_directory));
                if let Some(picker) = self.picker.as_mut() {
                    picker.offering_ignored(Some(ignored));
                    picker.offering_hidden(Some(hidden));
                    picker.relist(Vec::new());
                    picker.when_empty("No files under this directory");
                    // Open on the file being read. The walk decides where in
                    // the list it is, and it may be in the last batch, so the
                    // picker holds on to the name and selects the row when it
                    // turns up.
                    if let Some(prefer) = prefer {
                        picker.prefer(prefer);
                    }
                }
            }
            Listing::Changed => {
                // The walk behind the other tab keeps going: what it finds
                // is put away rather than drawn, and the reader walking
                // back to that tab should not have to wait for it twice.
                //
                // What a project ignores is not a question about this tab:
                // these rows are git's answer about what has changed, and
                // git does not report a file it was told to ignore.
                if let Some(picker) = self.picker.as_mut() {
                    picker.offering_ignored(None);
                    picker.offering_hidden(None);
                }
                let root = self.working_directory.clone();
                let mut rows: Vec<(String, obelus_git::Standing)> = self
                    .statuses
                    .iter()
                    .map(|(path, standing)| (relative(path, &root), standing.clone()))
                    .collect();
                // By name, because the order git reports them in is the order
                // it walked the tree, and a list that reorders itself between
                // openings cannot be learned.
                rows.sort_by(|left, right| left.0.cmp(&right.0));
                // How much each has changed, asked once for the whole list:
                // opening the repository and resolving the head tree is most
                // of the cost, and doing it per row would pay it as many
                // times as the tree has changed files.
                let counts = obelus_git::counted_against_head(
                    &self
                        .statuses
                        .keys()
                        .cloned()
                        .collect::<Vec<std::path::PathBuf>>(),
                );
                // What the last commit is, for the rows naming a file that
                // is not there any more: the only version of it left is the
                // one git has, and that is what such a row opens.
                let head = obelus_git::head_commit(&root);
                let items = rows
                    .into_iter()
                    .map(|(name, standing)| PickerItem {
                        prose: false,
                        marker: None,
                        // What it did to the file, at the row's right-hand
                        // end: a list of changed files is read for which of
                        // them to look at first, and how much each moved is
                        // most of that answer.
                        changed: counts.get(&root.join(&name)).copied(),
                        icon: obelus_icons::enabled()
                            .then(|| obelus_icons::for_path(std::path::Path::new(&name))),
                        // A submodule is another repository at a path, and
                        // Obelus has no notion of one. It is listed because
                        // it is a change to this tree and git reports it as
                        // one; it cannot be pressed, and the row is drawn in
                        // the colour that says so rather than waiting to be
                        // pressed to say it.
                        enabled: !standing.submodule,
                        colours: None,
                        status: Some(standing.status),
                        depth: 0,
                        opens: None,
                        kind: None,
                        label: name.clone(),
                        // What it was called before, where git says it was
                        // moved, and what it is, where it is not a file.
                        // The same arrow the history uses for the same
                        // fact, pointing back at the name it had.
                        version: None,
                        detail: match (&standing.was, standing.submodule) {
                            (Some(was), _) => Some(format!("\u{2190} {}", was.display())),
                            (None, true) => Some("submodule".to_string()),
                            (None, false) => None,
                        },
                        trailing: None,
                        // A file that is gone opens the way a commit's own
                        // version of a file opens: read-only, and saying
                        // above the first line which commit it is. There is
                        // no other version of it to offer.
                        value: match (standing.status, head) {
                            (obelus_git::FileStatus::Gone, Some(id)) => PickerValue::CommitFile {
                                id,
                                path: std::path::PathBuf::from(&name),
                            },
                            _ => PickerValue::File(std::path::PathBuf::from(&name)),
                        },
                        tab: None,
                        section: None,
                    })
                    .collect();
                if let Some(picker) = self.picker.as_mut() {
                    picker.replace(items);
                    picker.when_empty("Nothing has changed");
                }
            }
        }
    }

    /// Offers whatever is already open.
    ///
    /// Files and conversations in one list, because they are one list: what
    /// the reader can switch between. A second list for the conversations
    /// would be a second key, a second thing to learn, and two answers to
    /// "where was I".
    pub fn open_document_picker(&mut self) {
        self.open_switching(crate::app::worktrees::Tab::Documents);
    }

    /// The rows of what is already open.
    pub(in crate::app) fn document_rows(&mut self) -> Vec<PickerItem> {
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
            .documents
            .iter()
            .enumerate()
            // Closed slots are holes, not rows.
            .filter_map(|(index, document)| Some((index, document.as_ref()?)));

        let statuses = &self.statuses;
        let talker = self.talker.as_ref();
        // A row's count, so having none is an answer this can live with.
        let notes = obelus_todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default();
        open.map(|(index, document)| match document {
            Document::Chat(talk) => self.conversation_row(index, talk, talker, &notes),
            Document::File(buffer) => {
                Self::file_row(index, buffer, statuses, &self.working_directory)
            }
            Document::Notes(notes) => Self::notes_row(index, notes),
            Document::Terminal(terminal) => Self::terminal_row(index, terminal),
        })
        .collect()
    }

    /// Stops showing whatever is being read.
    ///
    /// The slot stays: [`DocumentId`] is an index, and the jump list holds
    /// them. What goes is the document -- a file with the language server's
    /// copy of it and the watch on it, or a conversation -- and the reader
    /// is left on whatever is open nearest, or on the welcome screen if that
    /// was the last one.
    pub fn close_current(&mut self) {
        // Asked about rather than done, for the reason leaving is asked
        // about: a closed buffer takes its undo with it.
        let which = self.selected_document().or(self.current);
        let unsaved = which
            .and_then(|id| self.file(id))
            .is_some_and(Buffer::is_dirty);
        if let Some(id) = which
            && unsaved
        {
            self.ask_before_closing(id);
            return;
        }
        // And a program still running, which is the same loss by another
        // name: what it had goes with it.
        if let Some(id) = which
            && self.ask_before_stopping(id)
        {
            return;
        }
        // Whichever file the screen is about. With the list of what is open
        // that is the row under the selection, not the file behind it:
        // the list is what the reader is pointing at, and one key that
        // means "close this" everywhere beats a second key that only
        // works in one place.
        if let Some(id) = self.selected_document() {
            // Among the rows showing, not the list's own: the same query
            // lets the same rows through, less the one closed.
            let at = self.picker.as_ref().and_then(|picker| {
                picker
                    .matches()
                    .position(|item| matches!(item.value, PickerValue::Document(row) if row == id))
            });
            self.close(id);
            // Rebuilt rather than patched, keeping whatever was typed: a
            // patched list would have to agree with the buffers about which
            // slots are holes.
            let query = self
                .picker
                .as_ref()
                .map(|picker| picker.query().to_string())
                .unwrap_or_default();
            self.open_document_picker();
            if let Some(picker) = self.picker.as_mut() {
                picker.set_query(&query);
                // Where the closed row was, which is the row after it, or
                // the one before where it was the last. Not where a list
                // opens, on what is being read: that is usually the first
                // row, and closing three in a row walked back up to it
                // between each.
                if let Some(at) = at {
                    picker.select_row(at);
                }
            }
            return;
        }

        let Some(id) = self.current else {
            self.wrong("No file to close".to_string());
            return;
        };
        self.close(id);
    }

    /// Moves to a file, and to nothing else.
    ///
    /// What the jump list and the history go through: both land on a line,
    /// and a conversation has none. Choosing a row of the list of what is
    /// open goes through [`App::go_to_document`] instead, because that list
    /// has conversations in it.
    pub(in crate::app) fn go_to_file(&mut self, id: DocumentId) {
        if self.file(id).is_some() {
            self.current = Some(id);
        }
    }

    /// Puts a file on the reader's screen, because an agent offered to.
    ///
    /// The same door every other jump goes through, which is the whole of
    /// why this is a thing an agent may do at all: the file opens, or the
    /// reader is taken to it where it is already open, the jump is
    /// recorded so `alt+left` comes back, and a file that will not open
    /// leaves them where they were. What an agent does the reader can see
    /// and take back, and this one is nothing but the seeing.
    ///
    /// The line is the agent's, counted from one the way the reader's own
    /// screen numbers them and the way `todo_list` prints the line a note
    /// points at. None of it leaves it where the file was last read.
    ///
    /// Says what happened, because the agent is waiting on an answer and
    /// "I have put it in front of them" and "there is no such file" are
    /// not the same turn.
    pub(in crate::app) fn open_for_an_agent(&mut self, path: &str, line: Option<u32>) -> String {
        // Nobody's screen to put it on -- and a file opened here would hold
        // what the agent writes to it for a save nobody makes.
        if self.is_headless() {
            return "Nobody is at Obelus's screen: it is reached from a chat, and opens no file"
                .to_string();
        }
        let asked = Path::new(path);
        // A path of its own is taken as it is. Against the project
        // otherwise, which is what an agent has been talking in: the tools
        // are opened on a project and every path it has seen is relative
        // to one.
        let full = match asked.is_absolute() {
            true => asked.to_path_buf(),
            false => self.working_directory.join(asked),
        };
        match line {
            Some(line) => self.go_to(&full, line.saturating_sub(1), 0),
            None => self.open(&full),
        }
        // Whether it landed, asked of the same thing `go_to` asks: a file
        // that would not open has left the reader where they were, and
        // telling the agent otherwise is telling it the reader is looking
        // at something they are not.
        if self
            .current_buffer()
            .is_none_or(|buffer| buffer.path() != full)
        {
            return format!("{path} would not open");
        }
        match line {
            Some(line) => format!("{path} is on the reader's screen, at line {line}"),
            None => format!("{path} is on the reader's screen"),
        }
    }

    /// Moves to whatever is in that slot, file or not.
    ///
    /// The slot has to hold something: an id whose document was closed
    /// names nothing, and going to nothing would leave the reader on a
    /// screen with no document and no way back to one.
    pub(in crate::app) fn go_to_document(&mut self, id: DocumentId) {
        if self.document(id).is_some() {
            self.current = Some(id);
        }
    }

    /// What the open list's selection names, if that is what the list is.
    pub(in crate::app) fn selected_document(&self) -> Option<DocumentId> {
        match self.picker.as_ref()?.selected_item()?.value {
            PickerValue::Document(id) => Some(id),
            _ => None,
        }
    }

    /// Stops showing one document, whichever the reader is on.
    pub(in crate::app) fn close(&mut self, id: DocumentId) {
        // The notes, before the slot is emptied. Typing waits for the
        // reader to stop before it is written, and closing a second after
        // typing is the one moment that pause has not come -- so it is
        // taken here. A note lives nowhere but the file.
        //
        // Nothing is unwatched here, though this is where the notes' three
        // watches used to be given up: what a view is drawn from is settled
        // from what is open, on the next frame, by
        // `App::settle_the_watches`.
        if let Some(changes) = self
            .documents
            .get_mut(id.get())
            .and_then(Option::as_mut)
            .and_then(Document::notes_mut)
            .map(obelus_component::todo::TodoView::take_changes)
        {
            self.do_to_the_notes(changes);
        }
        let Some(document) = self.documents.get_mut(id.get()).and_then(Option::take) else {
            return;
        };
        // What shutting a *file* means -- a server to tell, a watch to drop,
        // the tokens it was told about. A conversation has none of those and
        // is shut by the slot being empty, which has already happened: the
        // early return this used to take left `current` naming a slot with
        // nothing in it.
        if let Some(buffer) = document.file() {
            // Tell the server before dropping it: the message needs the path,
            // and a server left believing a file is open answers
            // questions about a version that no longer exists
            // anywhere.
            //
            // Not for a commit's version, which was never opened to it -- the
            // three notifications that go the other way all refuse one, and a
            // close for a document nobody announced tells a server to forget
            // the *file* at that path, which is open.
            if buffer.content().is_file()
                && let Some(language) = buffer.language()
                && let Some(client) = self.servers.get_mut(&language)
                && let Ok(uri) = obelus_lsp::client::uri_for(buffer.path())
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
            // Nothing said about it. The reader closed it and is looking at
            // what is there instead; a line naming what went is news to
            // nobody, and for a file outside the project -- what background
            // work wrote, somewhere under a temporary directory -- it was a
            // path the row had no room for.
        }
        // And whatever the row was saying goes with it, however it was
        // closed. A key quiets the row on its own way in; a press, a card's
        // answer, a sign-in's terminal ending does not, and what was said
        // about the document that went was left standing over the next one.
        self.quiet();
        // And a conversation's thread hears that it was, so that a reader
        // there is not left talking to something nobody is listening for.
        if let Some(talk) = document.chat() {
            self.mirror_closed(talk);
        }
        drop(document);

        // Whichever document is nearest, before the closed one for
        // preference: closing the last of several usually means going back
        // to the one before it.
        if self.current == Some(id) {
            self.current = self.nearest_open(id.get());
        }
    }

    /// Whatever is open nearest to a slot, looking back first.
    fn nearest_open(&self, from: usize) -> Option<DocumentId> {
        (0..from)
            .rev()
            .chain(from + 1..self.documents.len())
            .find(|index| self.documents.get(*index).is_some_and(Option::is_some))
            .map(DocumentId::new)
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
            self.wrong(match self.current_buffer() {
                Some(_) => "Nothing to preview in this file".to_string(),
                None => "No file to preview".to_string(),
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
        self.current_buffer()
            .and_then(|buffer| obelus_reading::of(buffer.path(), buffer.text()))
    }

    /// The reading on screen, if the current file is being shown as one.
    #[must_use]
    pub fn rendering(&self) -> Option<&[obelus_row::Row]> {
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
    pub(in crate::app) fn refresh_rendering(&mut self, width: u16) {
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
        let Some(reading) = obelus_reading::of(buffer.path(), buffer.text()) else {
            self.rendered = None;
            return;
        };
        let rows = obelus_reading::render(reading, &buffer.text().rope().to_string(), width);
        self.rendered = Some(Rendered { at, rows });
    }

    /// Opens a file, or switches to it if it is already open.
    pub(in crate::app) fn open(&mut self, path: &Path) {
        // Where the reader was, before they are somewhere else. Opening a
        // file is a leap, and the history is for leaps: without this, a
        // session of opening files leaves nothing to go back *to*, and
        // `go-back` answers "nowhere further back" to a reader who has been
        // three files deep. `push` drops a repeat of the same place, so
        // re-opening the file already being read records nothing.
        let from = self.here();

        if let Some(index) = self
            .documents
            .iter()
            // The file on disk, not a commit's version of it: those share
            // a path and are different documents, and a reader asking to
            // open the file means the one they can edit elsewhere.
            .position(|buffer| {
                buffer
                    .as_ref()
                    .and_then(Document::file)
                    .is_some_and(|open| open.path() == path && open.content().is_file())
            })
        {
            let id = DocumentId::new(index);
            if self.current != Some(id) {
                self.record(from);
            }
            self.go_to_file(id);
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(watcher) = self.watcher.as_mut()
                    && let Err(error) = watcher.watch(buffer.path())
                {
                    tracing::warn!(%error, path = %buffer.path().display(), "not watching");
                }
                self.documents.push(Some(Document::from(buffer)));
                let index = self.documents.len() - 1;
                // Only once the file is known to be readable: a path that
                // turns out to be a directory leaves the reader where they
                // were, and a history entry for a leap that did not happen
                // is a place `go-back` would take them for no reason.
                self.record(from);
                let id = DocumentId::new(index);
                self.go_to_file(id);
                self.serve(index);
            }
            // A path from the walk can have gone away, or be a file this
            // user cannot read. Neither is a reason to stop -- but silence
            // is: a list closes on the way here, so a reader who pressed
            // enter watched it vanish and put them back where they already
            // were with nothing said. A key that does nothing and a broken
            // key look the same.
            //
            // In the reader's own words rather than the error's. The two
            // that happen are a path that is gone and a path that is a
            // directory, and both read better as the fact than as an
            // `io::Error` about a path the reader can see on the row above.
            Err(error) => {
                tracing::warn!(%error, "could not open");
                let name = relative(path, &self.working_directory);
                self.wrong(if !path.exists() {
                    format!("{name} is not there any more")
                } else if path.is_dir() {
                    format!("{name} is a directory")
                } else {
                    format!("Could not open {name}: {error}")
                });
            }
        }
    }

    /// Asks where a file that is not there yet should go.
    ///
    /// Filled in with the directory the reader is in, because that is
    /// where the next file almost always goes -- the same argument the
    /// rename's prompt makes about starting at the name being changed.
    /// The trailing separator is part of it: a reader who takes the
    /// suggestion types a name onto the end, and one who does not holds
    /// backspace, which is cheaper than typing the directory out.
    ///
    /// Empty where nothing is open, which is the reader `ob
    /// some-directory` leaves on a list with no file behind it -- and the
    /// one most likely to be making the first file in a project. There is
    /// nothing to suggest, and a guess would be Obelus inventing a place.
    ///
    /// Empty for a file in the project's own root as well, and by the same
    /// arithmetic rather than by a case of its own: `join` on a path with
    /// nothing in it adds nothing, so the root suggests nothing and every
    /// other directory suggests itself with a separator after it. Written
    /// with a `/` it would have been `src\lsp/` on Windows -- one row in
    /// two conventions -- and a bare `/` at the root, which is the disk's.
    pub fn new_file(&mut self) {
        let here = self
            .current_buffer()
            .map(Buffer::path)
            .and_then(Path::parent)
            .map(|directory| self.as_a_directory(directory))
            .unwrap_or_default();
        self.ask_on_the_status_row(obelus_component::prompt::Prompt::about(
            obelus_component::prompt::PromptKind::NewPath,
            here,
        ));
    }

    /// Where an answer to the new-file question points.
    ///
    /// One place, because two things read it: this is what `make_file`
    /// acts on and what the question's own row says it will do. Worked out
    /// twice, the row would promise one directory and the file land in
    /// another -- and the reader would find out afterwards.
    ///
    /// Against the working directory and not against the file being read,
    /// which is what the question is filled in with: the suggestion is
    /// where the reader *is*, and the answer is a path like any other path
    /// a reader says.
    /// A directory as this row writes one: where it is, with a separator
    /// after it so a name typed onto the end lands in it.
    ///
    /// Relative to the project where it is under it, which is how every
    /// other path on that row is written, and whole where it is not --
    /// there is no shorter way to say that one which is still true.
    ///
    /// Empty for the project's own root, because `join` on a path with
    /// nothing in it adds nothing rather than a separator -- and a bare
    /// separator is the root of the *disk*. What the two callers make of
    /// an empty answer differs and is theirs: one has nothing to suggest,
    /// and the other has the one thing it most needs to say.
    fn as_a_directory(&self, directory: &Path) -> String {
        directory
            .strip_prefix(&self.working_directory)
            .unwrap_or(directory)
            .join("")
            .display()
            .to_string()
    }

    fn landing(&self, answer: &Path) -> PathBuf {
        match answer.is_absolute() {
            true => answer.to_path_buf(),
            false => self.working_directory.join(answer),
        }
    }

    /// The directory the new-file question would put the file in, as the
    /// row says it.
    ///
    /// `None` unless that question is the one being asked, because it is
    /// the only question on that row whose answer is a place.
    ///
    /// The directory is everything up to the last separator in what has
    /// been typed, which is what the reader is looking at -- rather than
    /// the parent of the resolved path, which is not the same thing twice
    /// over: an answer with nothing in it resolves to the project and its
    /// parent is the project's parent, and an answer ending in a separator
    /// already *is* the directory.
    ///
    /// Written the way every other path on this row is written: relative
    /// to the project, with the project's own root as `.` rather than as
    /// nothing, because nothing is what the reader was shown when they
    /// were standing in it. Somewhere outside the project keeps its whole
    /// path -- there is no shorter way to say it that is still true.
    pub fn making_in(&self) -> Option<String> {
        let prompt = self.prompt.as_ref()?;
        if *prompt.kind() != obelus_component::prompt::PromptKind::NewPath {
            return None;
        }
        let typed = prompt.text();
        // `is_separator` rather than a list, because which characters
        // those are is the platform's answer: `\` is one on Windows and an
        // ordinary character in a file's name everywhere else.
        let cut = typed.rfind(std::path::is_separator).map_or(0, |at| at + 1);
        let shown = self.as_a_directory(&self.landing(Path::new(&typed[..cut])));
        // Empty is the project's own root, and the one directory that has
        // to be said rather than left out: nothing is what the reader was
        // shown while they were standing in it.
        Some(match shown.is_empty() {
            true => Path::new(".").join("").display().to_string(),
            false => shown,
        })
    }

    /// Makes the file the answer names, and opens it.
    ///
    /// On disk at once, rather than in a buffer that is written later.
    /// Everything past the `create_new` is the path any other file takes
    /// -- the watcher is on it, the server has been told, git counts it
    /// among the untracked, `ctrl+r` has something to re-read and the
    /// status row is saying what it says about a file. A document that
    /// existed only in memory would be a second kind of open file, and
    /// every one of those would have to learn about it.
    ///
    /// `create_new` and not `create`: a file already there is refused
    /// rather than emptied, which is the one thing here that cannot be
    /// put back. The same refusal a rename makes, in the same words --
    /// one fact, one sentence, wherever the reader meets it.
    pub(in crate::app) fn make_file(&mut self, answer: &Path) {
        let path = self.landing(answer);
        // Somewhere else entirely. A rename may leave the project --
        // taking a path rather than a name is what moving a file *is*,
        // and the file it moves is one the reader already had -- but this
        // makes one, and where Obelus makes a file is the project it was
        // opened on. `../../etc/hosts` typed into a question that opens
        // blank is a slip, not a plan.
        //
        // Folded before it is asked, because `starts_with` is spelling
        // and not place: `<project>/../elsewhere` begins with the
        // project's own path and is nowhere near it.
        if !folded(&path).starts_with(folded(&self.working_directory)) {
            self.wrong(format!(
                "{} is outside the project",
                self.named(&path, answer)
            ));
            return;
        }
        // A directory that is not there yet, which is what taking a path
        // rather than a name is for: a reader starting a module should not
        // have to leave to make the directory it goes in. The rename says
        // the same thing about moving a file into one.
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(%error, path = %parent.display(), "could not make the directory");
            let shown = self.named(parent, answer);
            // Asked of the path rather than read off the error, the same
            // as the file below: what happens to a reader is a path with a
            // file somewhere along it, and that is a thing to say rather
            // than an errno to translate. Anything else -- a permission,
            // a read-only disk -- has no such fact behind it and gets the
            // shape that names no reason.
            self.wrong(match parent.exists() && !parent.is_dir() {
                true => format!("{shown} is not a directory"),
                false => format!("Could not make {shown}"),
            });
            return;
        }
        let name = self.named(&path, answer);
        match std::fs::File::create_new(&path) {
            Ok(_) => self.open(&path),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not make");
                // Asked of the path and not of the error's kind: something
                // already there is `AlreadyExists` for a file and
                // `IsADirectory` for a directory -- `src` and `src/` are
                // the two, and which errno an OS picks between them is not
                // a difference the reader is being told about.
                //
                // No reason on the other shape. Why it would not go is an
                // error chain, and the row this goes on is shared with the
                // file's own name: a sentence too long for it is dropped
                // whole, and a warning nobody sees is not a warning. The
                // whole of it is in the log, which is where `save-file`
                // puts its own for the same reason.
                self.wrong(match path.exists() {
                    true => format!("{name} is already there"),
                    false => format!("Could not make {name}"),
                });
            }
        }
    }

    /// What to call a path in something said about it.
    ///
    /// Where it is, the way every other path on the status row is written.
    /// Falling back to what the reader typed where that comes to nothing:
    /// `.` resolves to the project and strips to an empty path, and a
    /// sentence with a blank where the name goes is the shape `write_now`
    /// already refuses.
    ///
    /// Bounded, because the rest of it is the reader's own text and the
    /// row it goes on is shared with the file's name. Forty columns is
    /// about as much as is left of a narrow one once a path and a position
    /// have had theirs.
    fn named(&self, path: &Path, answer: &Path) -> String {
        const NAMED: usize = 40;
        let shown = relative(path, &self.working_directory);
        let shown = match shown.is_empty() {
            true => answer.display().to_string(),
            false => shown,
        };
        obelus_ui::truncate_from_right(&shown, NAMED)
    }

    /// Every open document, as its path and whether it is unwritten.
    ///
    /// For a test about a change made to files the reader is not looking
    /// at: what matters is that they are documents and that they are
    /// unwritten, and nothing else can see either.
    #[must_use]
    pub fn buffers_for_test(&self) -> Vec<(std::path::PathBuf, bool)> {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .map(|buffer| (buffer.path().to_path_buf(), buffer.is_dirty()))
            .collect()
    }

    /// Opens a file without going to it, and says where it landed.
    ///
    /// What a rename needs: the files it changes have to be documents --
    /// so the change is an edit the reader can undo and a file they decide
    /// when to write -- and taking the reader to each of them in turn
    /// would be a tour of a dozen files they did not ask for.
    ///
    /// The server is told about it, as it is for any file Obelus opens: it
    /// is about to be edited, and a server that has not been told has a
    /// different document.
    pub(in crate::app) fn open_quietly(&mut self, path: &Path) -> Option<usize> {
        if let Some(index) = self.open_at(path) {
            return Some(index);
        }
        match Buffer::open(path) {
            Ok(buffer) => Some(self.take_in(buffer)),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not open");
                None
            }
        }
    }

    /// Where the file at this path is open, as the file on disk.
    pub(in crate::app) fn open_at(&self, path: &Path) -> Option<usize> {
        self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::file)
                .is_some_and(|open| open.path() == path && open.content().is_file())
        })
    }

    /// Puts a buffer read somewhere else in the list, watched and served
    /// the way one opened here is.
    pub(in crate::app) fn take_in(&mut self, buffer: Buffer) -> usize {
        if let Some(watcher) = self.watcher.as_mut()
            && let Err(error) = watcher.watch(buffer.path())
        {
            tracing::warn!(%error, path = %buffer.path().display(), "not watching");
        }
        self.documents.push(Some(Document::from(buffer)));
        let index = self.documents.len() - 1;
        self.serve(index);
        index
    }

    /// Folds what the cursor is in, or unfolds what it is on.
    ///
    /// Silent when there is nothing here to fold, because `AFoldHere` has
    /// already answered that: the key is dim and does nothing, and a note
    /// saying so would be the third answer to a question the palette
    /// settled.
    pub fn toggle_fold(&mut self) {
        // The notes, where they are the document being read. The same act
        // on a different subject: one row standing in for several, and the
        // key that opens it. What hangs under a note is the run there, the
        // way a run of lines is the run in a file -- and the file behind
        // the notes is not what the reader is looking at, which is what
        // this used to fold.
        if let Some(notes) = self.notes_mut() {
            notes.toggle_fold();
            return;
        }
        // The file's own runs, always. A block used to be asked first --
        // "fold what the cursor is inside", and a commit's message was a
        // thing the reader was put inside -- but nothing a block holds
        // folds now: a message is read whole in the preview, and a hunk's
        // removed lines are a thing the reader opened and closes with the
        // same key, where a second way to half-close it would be two
        // answers to one question.
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

    /// Puts a buffer somebody else made into the list, for a test.
    ///
    /// A commit's version is made from bytes git handed over rather than
    /// from a path, so there is no opening it.
    pub fn open_buffer_for_test(&mut self, buffer: Buffer) {
        self.documents.push(Some(Document::from(buffer)));
        let index = self.documents.len() - 1;
        self.go_to_file(DocumentId::new(index));
    }

    /// Opens a path the way choosing it from a list does.
    ///
    /// For a test: the lists that reach this are filled from a walk on
    /// another thread, and a test that pumped the walk to press one key
    /// would be a test of the walk.
    /// Opens a path the way choosing it from a list does.
    ///
    /// For a test: the lists that reach this are filled from a walk on
    /// another thread, and a test that pumped the walk to press one key
    /// would be a test of the walk.
    pub fn open_for_test(&mut self, path: &Path) {
        self.open(path);
    }

    /// Re-reads whichever open buffers came from `path`.
    ///
    /// The watch is on a directory, so most of what arrives here is about
    /// files Obelus does not have open.
    pub(in crate::app) fn reload_path(&mut self, path: &Path) {
        for index in 0..self.documents.len() {
            let Some(buffer) = file_in_mut(&mut self.documents, DocumentId::new(index)) else {
                continue;
            };
            // A commit's version of a file does not change when the file
            // does: those bytes are what that commit said, and re-reading
            // over them would replace a document the reader chose with one
            // they did not.
            if buffer.path() != path || !buffer.content().is_file() {
                continue;
            }
            // The commonest change reported about an open file is Obelus's
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
                self.ask_standing_questions(index);
            }
        }
    }

    /// Writes the file being read back to disk.
    pub fn save_current(&mut self) {
        let Some(index) = self.current.map(DocumentId::get) else {
            self.wrong("No file open".to_string());
            return;
        };
        let Some(buffer) = self.file_mut(DocumentId::new(index)) else {
            return;
        };
        // A commit's version is not a file anybody can write back, and the
        // path it wears belongs to a different document.
        if !buffer.content().is_file() {
            self.wrong("This is a commit's version, not the file".to_string());
            return;
        }
        if !buffer.is_dirty() {
            self.wrong("Nothing to save".to_string());
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
            Disk::Written => self.ask_before_saving(DocumentId::new(index)),
            // A different question with different answers: there is nothing
            // on a disk that has nothing on it to take instead.
            Disk::Deleted => self.ask_before_writing_back(DocumentId::new(index)),
        }
    }

    /// Lays a document out, if that was asked for, and writes it.
    ///
    /// Apart from the command because the answer to a question about a file
    /// that moved comes back here too, having settled the one thing the
    /// command stopped for.
    pub(in crate::app) fn save_now(&mut self, index: usize) {
        // What the server offers to do to the whole file first, where
        // that was asked for. Before the layout, because those change the
        // text and the layout is about the text that ends up written --
        // and each step is a round trip whose answer picks the next one
        // up, rather than the whole program being held still waiting for
        // another process to reply.
        if self.settled.config.code_actions_on_save {
            self.next_on_save(index, 0);
            return;
        }
        self.format_then_write(index);
    }

    /// The rest of a save: laid out where that was asked for, and written.
    ///
    /// Its own step because the imports come first and their answer
    /// arrives here, having settled the one thing it stopped for.
    pub(in crate::app) fn format_then_write(&mut self, index: usize) {
        if self.settled.config.format_on_save && self.ask_formatting(index) {
            self.say("Laying it out\u{2026}".to_string());
            return;
        }
        self.write_now(index);
    }

    /// Throws away what the reader wrote and re-reads the file.
    ///
    /// One of the two ways out of a file that moved. The undo goes with it:
    /// [`Buffer::reload`] forgets, because what could have been put back was
    /// about text that is not there any more.
    pub(in crate::app) fn take_what_is_on_disk(&mut self, index: usize) {
        let Some(buffer) = self.file_mut(DocumentId::new(index)) else {
            return;
        };
        match buffer.take_from_disk() {
            Ok(changed) => {
                if changed {
                    self.change_document(index);
                    self.ask_standing_questions(index);
                }
                self.say("Took what is on disk -- undo brings yours back".to_string());
            }
            Err(error) => {
                tracing::warn!(%error, "taking what is on disk failed");
                self.wrong("Could not read it".to_string());
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
    pub(in crate::app) fn write_now(&mut self, index: usize) -> bool {
        let Some(buffer) = self.file_mut(DocumentId::new(index)) else {
            return false;
        };
        match buffer.save() {
            Ok(()) => {
                self.say("Saved".to_string());
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
                // Two shapes rather than one with a blank in it: after a
                // name it reads as a label and its reason, and with no name
                // it is a sentence of its own and starts like one.
                // Worked out before it is said, because saying it is a
                // method on the application and the buffer is borrowed out
                // of the application to have been saved at all.
                let said = match buffer.path().file_name() {
                    Some(name) => format!("{}: not saved", name.to_string_lossy()),
                    None => "Not saved".to_string(),
                };
                self.wrong(said);
                false
            }
        }
    }

    /// Re-reads the current file and reparses what changed.
    pub fn reload_current(&mut self) {
        let Some(index) = self.current.map(DocumentId::get) else {
            return;
        };
        if let Some(buffer) = self.file_mut(DocumentId::new(index))
            && reload(buffer)
        {
            self.change_document(index);
            self.ask_standing_questions(index);
        }
    }
}

/// A reading laid out, and what it was made from.
///
/// The three things it depends on, so that a change to any of them is
/// noticed: which file, which version of it, and how wide the screen was.
#[derive(Debug)]
pub(in crate::app) struct Rendered {
    at: (PathBuf, i32, u16),
    rows: Vec<obelus_row::Row>,
}

/// Re-reads one buffer, and says whether the text changed.
///
/// Reports rather than propagates a failure: a file that has been deleted or
/// replaced by a directory leaves the buffer showing what it last held.
/// Losing the contents would be worse than showing something a moment out of
/// date.
/// A path with its `.` and `..` folded away, without asking the disk.
///
/// Lexical, and deliberately so twice over. `canonicalize` wants the path
/// to exist, which the one being made by definition does not; and it
/// resolves symlinks, which would judge a path by a name the reader did
/// not use -- the same reason `Buffer::open` keeps an absolute path rather
/// than a canonical one.
///
/// Which leaves a symlink inside the project pointing out of it, and that
/// is a file the reader put there on purpose. What this is for is the slip.
fn folded(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            // Nothing to climb above keeps the `..`: an answer is resolved
            // against the project before this, so one that still has them
            // is one that walked past the root.
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push(part);
                }
            }
            other => out.push(other),
        }
    }
    out
}

pub(in crate::app) fn reload(buffer: &mut Buffer) -> bool {
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

/// The path a row of a file list stands for, relative to the tree's root.
///
/// `None` for a row that is not a place on disk. Both shapes of the list
/// put the same value in a row -- the flat listing's label is the whole
/// path and the tree's is only the last part of it -- so this is what tells
/// two rows apart when their labels cannot.
pub(in crate::app) fn path_of_row(item: &PickerItem) -> Option<&Path> {
    match &item.value {
        PickerValue::File(path) | PickerValue::Directory(path) => Some(path),
        _ => None,
    }
}
