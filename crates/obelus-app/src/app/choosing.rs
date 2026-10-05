//! The lists Obelus offers, and what choosing from one does.
//!
//! The list itself is [`obelus_component::picker`]; what is here is which
//! rows go in it and what a chosen row means.

use super::*;

impl App {
    /// Opens a picker over rows the caller built.
    ///
    /// The only way to reach a list of places without a language server
    /// answering first, which is what a test of the preview needs.
    pub fn open_picker_for_test(&mut self, items: Vec<PickerItem>, layout: PickerLayout) {
        let mut picker = Picker::new(items, layout);
        // Standing in for the application's lists of files and places, which
        // is what every caller of this hands it, and those all preview.
        picker.previews();
        self.show_list(picker);
    }

    /// Offers the built-in themes.
    pub fn open_theme_picker(&mut self) {
        self.theme_before = Some((self.theme_name().to_string(), *self.theme()));
        let items = self
            .themes()
            .into_iter()
            .map(|name| PickerItem {
                prose: false,
                marker: None,
                // The same glyph on every row, which is the honest one: what
                // distinguishes two themes is the colours, and the row's own
                // name is what says which.
                icon: obelus_icons::enabled().then_some(obelus_icons::ui::THEME),
                label: name.clone(),
                detail: None,
                trailing: None,
                changed: None,
                value: PickerValue::Theme(name),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.when_empty("No theme is built in");
        picker.before_typing("Filter themes");
        // Open on the one that is on, so the list starts by saying which
        // theme this is rather than making the reader work it out.
        picker.prefer(self.theme_name().to_string());
        picker.opened_by(Command::ThemeSelect);
        self.show_list(picker);
    }

    /// Offers every command by name.
    pub fn open_command_palette(&mut self) {
        // Every command, and what it can do *here* said by whether its row
        // can be chosen. Leaving out what cannot run makes the palette a
        // list nobody can learn from -- a reader who never sees `show-change`
        // does not find out Obelus has it -- while a row that runs and then
        // reports why it did nothing is a row nobody trusts. Dim and
        // unselectable is both answers at once.
        //
        // Except what this front end can never do, which is not a row at
        // all: dim says "not here, not now", and a command that needs a
        // window is not one a terminal will ever have.
        let items = obelus_command::ALL
            .iter()
            .filter(|spec| spec.command.shown())
            .map(|spec| PickerItem {
                prose: false,
                marker: None,
                icon: obelus_icons::enabled().then(|| obelus_icons::for_command(spec.command)),
                enabled: self.offers(spec.command),
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                label: spec.name.to_string(),
                // The tab it lives under. One past its position in the list
                // of groups, because the picker's own first tab is "all".
                tab: obelus_command::Group::ALL
                    .iter()
                    .position(|group| *group == spec.command.group())
                    .map(|at| at + 1),
                section: None,
                detail: Some(spec.title.to_string()),
                // The key it is bound to, if it is bound to one. A command
                // with nothing here is one the palette is the only way to
                // reach, which is worth being able to see.
                trailing: self.keymap.chord_for(spec.command).map(KeyChord::label),
                changed: None,
                value: PickerValue::Command(spec.command),
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        // It holds every command, so it is only ever empty for a query that
        // matches none of them -- which the picker says itself.
        picker.when_empty("No command by that name");
        picker.before_typing("Filter commands");
        // Tabs over one long list. Fourteen commands is already more than a
        // compact list shows at once, and the groups are what a reader is
        // choosing between when they do not already know the name.
        let names: Vec<&str> = obelus_command::Group::ALL
            .iter()
            .map(|group| group.name())
            .collect();
        picker.with_tabs(&names);
        // Read down as much as it is typed at: most of what the palette
        // offers is what the reader came to find out, so the block stays the
        // height it opened at rather than closing up under the query.
        picker.keeps_height();
        picker.aligns_details();
        picker.opened_by(Command::CommandPalette);
        self.show_list(picker);
    }

    /// Opens the log, as a file like any other.
    ///
    /// A reader is what Obelus is, so the log needs no viewer of its own: it
    /// becomes a buffer, the watcher on its directory reloads it as it grows,
    /// and the cursor stays where it was put. What is in it that no screen
    /// shows is a server's own words -- its stderr, its handshake, and the
    /// requests Obelus sent it.
    pub fn open_log(&mut self) {
        self.open_log_file(obelus_logging::OBELUS, "No log file");
    }

    /// And the language servers' own, which is the other half of the same
    /// idea: a server's handshake and every request Obelus sent it, in a
    /// file of its own because it is somebody else's program talking at a
    /// volume that would bury the dozen lines Obelus has to say.
    pub fn open_server_log(&mut self) {
        self.open_log_file(obelus_logging::SERVERS, "No server log file");
    }

    /// Opens whichever log, or says there is none.
    fn open_log_file(&mut self, prefix: &str, missing: &str) {
        match obelus_logging::current_file(prefix) {
            Some(path) => self.open(&path),
            // Logging is allowed to fail without stopping Obelus starting, so
            // there may genuinely be no file -- and a server log exists only
            // once a server has said something.
            None => self.wrong(missing.to_string()),
        }
    }

    /// Whether a command can do its job right now.
    ///
    /// One exhaustive match over the conditions rather than a test per
    /// command: what each command needs is declared beside it in
    /// [`obelus_command::Command::requires`], and this is the one place that
    /// turns a condition into a yes or no from the application's own state.
    /// A row that silently fails is worse than a row that is not there.
    #[must_use]
    pub fn offers(&self, command: Command) -> bool {
        let buffer = self.current_buffer();
        match command.requires() {
            Requires::Nothing => true,
            // Whether the question has been answered, which is a thing
            // known without doing any work -- and not whether the answer
            // turned out to hold anything, which is what the two history
            // and change conditions below go and find out. There is no
            // project while Obelus is still asking which one, and none
            // once the one it was has gone.
            Requires::AProject => self.has_a_project(),
            Requires::AnotherWorktree => self.another_worktree(),
            // Asking counts as having: a second asking would wait behind
            // the first, and the way to stop asking is the other command.
            Requires::ARemote => self.platform().is_some() && !self.has_the_remote(),
            Requires::TheRemote => self.has_the_remote(),
            Requires::AFileOpen => buffer.is_some(),
            Requires::AFileOnDisk => buffer.is_some_and(|buffer| buffer.content().is_file()),
            // A file, or a box a reader is typing into. The same places a
            // paste goes into, asked as one question rather than in order:
            // what these keys need is that there is somewhere with a caret
            // in it, not which of them it is.
            Requires::ACaret => buffer.is_some() || self.somewhere_to_type(),
            // And everywhere one of those is, plus the document being read
            // where that is a conversation: a transcript is somewhere a
            // reader can take hold of what was said and nowhere they can
            // type, so the question above answers no for the one place the
            // selection was taken in.
            //
            // `reading_nothing` rather than `conversation()`, because what
            // is being asked is whether there is anything on the screen to
            // take a copy of, and a file answers that through the buffer
            // above.
            Requires::SomethingToCopy => {
                buffer.is_some() || self.somewhere_to_type() || !self.reading_nothing()
            }
            // Not `current_buffer`, which is the point of the distinction: a
            // conversation is something open and is not a file.
            Requires::ADocumentOpen => !self.reading_nothing(),
            // A project before the walk, for all three of these: git asked
            // about a tree that has gone looks in the directory above it,
            // and that may well be some other repository.
            Requires::AFileInHistory => {
                buffer.is_some() && self.has_a_project() && self.has_history()
            }
            Requires::AKnownLanguage => buffer.and_then(Buffer::language).is_some(),
            // Either the file has a reading, or it is already showing one
            // -- which is the same question asked from the other side: the
            // key that turns a preview on is the key that turns it off.
            Requires::APreview => {
                self.reading_of_current().is_some()
                    || buffer.is_some_and(|buffer| buffer.mode() == Mode::Preview)
            }
            // The character under the cursor, not the scan the command does.
            // The scan needs the whole file highlighted to know a bracket in
            // a string from one in code, and deciding whether to *list* a
            // row is not worth a pass over the file; a bracket inside a
            // string is then offered and answers "no bracket here", which is
            // a reason rather than a silence.
            Requires::ABracket => buffer.is_some_and(|buffer| {
                let cursor = buffer.cursor();
                let text = buffer.text();
                let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));
                text.rope()
                    .byte_slice(at.get()..)
                    .chars()
                    .next()
                    .is_some_and(|character| matches!(character, '(' | ')' | '[' | ']' | '{' | '}'))
            }),
            // In the file or in a hunk's removed lines: a reader who can
            // put a caret on something can take a copy of it.
            Requires::ASelection => buffer.is_some_and(Buffer::has_selection),
            // Something that changed *and* has something to show: a run of
            // added lines changed nothing that is not already on screen.
            // Or one already open, which this is also the key that closes
            // -- from wherever the reader has walked to inside it.
            Requires::AHunk => buffer.is_some_and(|buffer| {
                buffer.block_at_cursor().is_some()
                    || buffer.block_below_cursor().is_some()
                    || self
                        .changes()
                        .and_then(|changes| changes.hunk_at(buffer.cursor().line))
                        .is_some()
                    || buffer.caret_block().is_some()
            }),
            Requires::AHunkBefore => buffer.is_some_and(|buffer| {
                self.changes()
                    .and_then(|changes| changes.hunk_before(buffer.cursor().line))
                    .is_some()
            }),
            Requires::AHunkAfter => buffer.is_some_and(|buffer| {
                self.changes()
                    .and_then(|changes| changes.hunk_after(buffer.cursor().line))
                    .is_some()
            }),
            Requires::ATroubleBefore => buffer
                .is_some_and(|buffer| self.trouble_from(buffer.cursor().line, false).is_some()),
            Requires::ATroubleAfter => {
                buffer.is_some_and(|buffer| self.trouble_from(buffer.cursor().line, true).is_some())
            }
            Requires::SomewhereBack => self.jumps.can_go_back(),
            Requires::SomewhereForward => self.jumps.can_go_forward(),
            Requires::ARunningServer => buffer
                .and_then(Buffer::language)
                .is_some_and(|language| self.servers.contains_key(&language)),
            // Per command: a server may answer one of these questions and
            // not another, and the menu is built from the same list.
            Requires::AnAnswer => self
                .symbol_actions()
                .unwrap_or_default()
                .iter()
                .any(|action| action.command() == command),
            // The one condition that is a walk rather than a field: what
            // has changed in the project is git's to say, with every ignore
            // rule applied. Asked once when the palette opens and once per
            // press of the key it is on -- this repository answers in two
            // milliseconds, which is worth paying to stop offering a row
            // whose whole answer would be "nothing has changed".
            // What the tree offers on the line the cursor is on, which is
            // a question about this file and this line rather than about
            // the language: a file Obelus parses can still have nothing to
            // fold where the reader is standing.
            // A walk of one commit, which is what "is there a history
            // here" costs: the same trade `AChangedFile` makes.
            Requires::AHistory => self.has_a_project() && self.has_history(),
            Requires::SomethingToUndo => self.current_buffer().is_some_and(Buffer::can_undo),
            Requires::SomethingToRedo => self.current_buffer().is_some_and(Buffer::can_redo),
            // The file's own runs, and nothing a block holds. A block used
            // to be asked as well, because a commit's message was a thing
            // the reader was put inside and the cursor cannot say so -- it
            // stays on the line the block hangs above while the caret is up
            // in rows the file does not have. Nothing a block holds folds
            // now, so the question is the file's again.
            // The notes answer for themselves where they are the document
            // being read: what folds there is what hangs under a note.
            Requires::AFoldHere if self.notes().is_some() => self
                .notes()
                .is_some_and(obelus_component::todo::TodoView::can_fold),
            Requires::AFoldHere => self.current_buffer().is_some_and(|buffer| {
                buffer.folds().is_folded_at(buffer.cursor().line)
                    || buffer.folds().offered_at(buffer.cursor().line).is_some()
            }),
            Requires::AFoldableFile => self
                .current_buffer()
                .is_some_and(|buffer| !buffer.folds().is_empty() && !buffer.folds().all_folded()),
            Requires::SomethingFolded => self
                .current_buffer()
                .is_some_and(|buffer| buffer.folds().any_folded()),
            // Whether there is one, not what they all are: this is asked
            // for every row of the palette, and building a map of every
            // changed path to look at its length walks the project each time.
            Requires::AChangedFile => self.has_a_project() && self.anything_changed(),
            // The language's own fact, and a cheap one: what a file is
            // parsed as is already known.
            Requires::ALineComment => self
                .current_buffer()
                .and_then(Buffer::language)
                .is_some_and(|language| language.line_comment().is_some()),
            Requires::AServerLog => obelus_logging::current_file(obelus_logging::SERVERS).is_some(),
        }
    }

    /// Puts a list up, over whatever it covers.
    ///
    /// Every list goes through here, which is the point: sixteen places
    /// build one, and a rule about what a list covers that sixteen places
    /// had to remember is a rule with sixteen chances of being forgotten.
    /// What it covers is [`Room::Band`] -- a question on the status bar,
    /// and the list that was there -- and it leaves the pages standing,
    /// because a setting's choices open *over* the settings and an agent's
    /// question over the conversation it was asked in.
    pub(super) fn show_list(&mut self, picker: Picker) {
        self.make_room(Room::Band);
        // Whatever the last list was, it is not this one. Said here rather
        // than only on the way out, because a list can be opened over a
        // list without the one underneath being left -- and a radius left
        // behind would have the *new* list's tabs refilled with problems.
        // Which is why `open_troubles` declares its radii after this call
        // and not before.
        self.troubling.clear();
        self.conversing = crate::app::conversations::Conversing::default();
        self.worktrees.not_showing();
        // Where the reader is looking, before the list takes any of it.
        // Taken here rather than when the list first shows them somewhere,
        // because a list that sits on the status bar shortens the editor
        // and the file scrolls to keep the caret in what is left -- so by
        // the time anything is being shown, the place they were looking
        // from has already gone.
        self.looked_from = self
            .current
            .zip(self.current_buffer().map(Buffer::viewport));
        self.picker = Some(picker);
    }

    /// Puts a question on the status bar, over whatever it covers.
    ///
    /// Which is only another question: it is one row, and what it is asking
    /// about is still on screen behind it.
    pub(super) fn ask_on_the_status_row(&mut self, prompt: obelus_component::prompt::Prompt) {
        self.make_room(Room::Row);
        self.prompt = Some(prompt);
    }

    /// Offers a key to the list, and says whether it took it.
    ///
    /// Its own geometry is worked out here rather than passed in, the way
    /// every other view's is: a page is the rows the list is actually
    /// *drawn* in, a full-area list gives half of that to a preview, and a
    /// page of the whole editor would walk the selection twice as far as
    /// the reader can see. Nothing above this needs to know that.
    pub(super) fn picker_key(&mut self, key: &KeyEvent) -> bool {
        let page = self.picker.as_ref().map_or(1, |picker| {
            obelus_ui::picker::rows_drawn(picker, self.picker_area())
        });
        // A tree of calls: its tabs are the two directions, and walking
        // onto one asks the other question. Asked before the list is
        // borrowed, which is the only reason it is up here.
        let calling = self.showing_calls();
        let Some(picker) = self.picker.as_mut() else {
            return false;
        };
        // What a search is asking, before and after the key. The picker
        // owns the query and the tab and knows nothing about where rows
        // come from, so the application watches those two for movement
        // rather than the picker reporting it.
        let searching = picker.is_searching();
        let listing = picker.is_listing();
        // A history has tabs too, and walking onto one is what asks its
        // question: the commits of a file and of a project are two
        // answers, not two views of one.
        let historic = !self.history.radii.is_empty();
        let troubling = !self.troubling.is_empty();
        // And a list of conversations is one answer per agent: the rows of
        // a tab are that agent's, fetched when the reader walks onto it.
        let conversing = !self.conversing.agents.is_empty();
        // And the list of open documents, whose second tab is the
        // repository's worktrees.
        let switching = !self.worktrees.tabs.is_empty();
        let before = (picker.tab(), picker.query().to_string());
        // Where the tree is standing, read before the key can move it: a
        // typed letter filters the rows the list already has, so by the
        // time the query has changed the row the reader was on is not
        // among them any more.
        let tree = listing
            && before.1.is_empty()
            && self.listing.get(before.0).copied() == Some(obelus_component::picker::Listing::All);
        let standing = tree
            .then(|| picker.selected_item())
            .flatten()
            .and_then(super::documents::path_of_row)
            .map(Path::to_path_buf);
        let outcome = picker.handle_key(key, page);
        let after = (picker.tab(), picker.query().to_string());
        match outcome {
            PickerOutcome::Consumed => {
                if searching && after != before {
                    self.refresh_search();
                }
                // The tab, or the query going empty or stopping being
                // empty: a file list is a tree while nothing is typed and
                // the flat filtered list the moment something is, and those
                // are two sets of rows rather than two ways of drawing one.
                if listing && (after.0 != before.0 || after.1.is_empty() != before.1.is_empty()) {
                    // Kept only where there was a tree to put down, which
                    // is what makes clearing the query put the reader back
                    // rather than move them somewhere new.
                    if tree {
                        self.stood_on = standing;
                    }
                    self.refresh_listing();
                }
                if historic && after.0 != before.0 {
                    self.refresh_history();
                }
                // And a list of problems is two answers as well: what is
                // wrong with this file, and what is wrong with the project
                // around it.
                if troubling && after.0 != before.0 {
                    self.refresh_troubles();
                }
                if calling && after.0 != before.0 {
                    self.turn_calls_round();
                }
                if conversing && after.0 != before.0 {
                    self.refresh_conversations();
                }
                if switching && after.0 != before.0 {
                    self.refresh_switching();
                }
                true
            }
            // What is behind a row is the application's: the list reports
            // the key and knows nothing about what opening one costs.
            PickerOutcome::Open => {
                self.open_call();
                true
            }
            PickerOutcome::Cancelled => {
                self.leave(Layer::Picker);
                true
            }
            PickerOutcome::Accepted(value) => {
                self.accept(value);
                true
            }
            PickerOutcome::Ignored => false,
        }
    }

    pub(super) fn accept(&mut self, value: PickerValue) {
        // A commit is not somewhere to go: it opens its files under itself,
        // in place, and the list stays open around them. Asked before the
        // list is torn down, because the list is what it happens to.
        // A commit opens its files under it where there are files to
        // choose between, and opens *the* file where the list is already
        // about one: a row in a file's own history names that file and that
        // commit, which is a document, and nothing else needs choosing.
        // A directory is not somewhere to go: it opens under itself, in
        // place, and the list stays open around it. Asked before the list
        // is torn down, because the list is what it happens to.
        if let PickerValue::Directory(path) = &value {
            self.open_directory(&path.clone());
            return;
        }
        // A conversation is taken up before the list is torn down, because
        // the claim can fail: the list was built a moment ago and another
        // Obelus may have walked into that conversation since. Where it
        // has, the list stays open and the row is drawn with the lock on
        // it, which is the answer -- nothing happening at all is a key that
        // looks broken.
        if let PickerValue::Conversation(at) = &value {
            let at = *at;
            if !self.take_up_conversation(at) {
                self.refresh_conversations();
                return;
            }
            // Out through the door every list leaves by, so that what
            // the list declared -- which agents its tabs were, what each
            // row stood for, the watch on which conversations are open
            // elsewhere -- goes however it is left. The file the list was
            // over is put back first, which also leaves `leave` nothing to
            // put back: the reader is in the conversation now, and the
            // file is as they left it for when they come back.
            self.put_the_file_back();
            self.leave(Layer::Picker);
            return;
        }
        if let PickerValue::Commit(id) = value {
            if self.expand_commit(id) {
                return;
            }
            if let Some(path) = self.commit_opens_at(id) {
                self.picker = None;
                self.open_at_commit(id, &path, None);
                return;
            }
        }
        // Where the query matched in the selected row, for a list whose
        // rows are the lines they name. Worked out here rather than read
        // from the last frame, because a key can arrive before one has been
        // drawn -- and asked before the picker is closed, because it is the
        // picker that knows.
        let lines = self.rows_are_lines();
        let matched = self.picker.as_mut().filter(|_| lines).and_then(|picker| {
            let row = picker.selected();
            picker
                .matched_columns(row)
                .first()
                .map(|column| *column as usize)
        });
        self.picker = None;
        // A place is where the reader meant to be: the list has been
        // showing it to them, and choosing it is going there. Forgotten
        // rather than gone back to -- and forgotten it must be, or the next
        // list they escape out of would put them back at a place they left
        // on purpose two lists ago. Anything else is not somewhere the list
        // was showing, so the file goes back to where it was before the
        // row is acted on: what the row does -- a command that moves the
        // caret, a file opened -- then moves the view from there, as it
        // would have from a file nobody had opened a list over.
        match value {
            PickerValue::Place { .. } => self.looked_from = None,
            _ => self.put_the_file_back(),
        }
        // The history goes with its list: the radii are what says a history
        // is open at all. A tree of calls goes the same way, for the same
        // reason.
        self.history = crate::app::history_view::Showing::default();
        self.close_calls();
        // And the list of conversations with it, which can be left by a row
        // that is not a conversation: the one that starts a new one.
        self.conversing = crate::app::conversations::Conversing::default();
        // A theme worn while walking a list is the reader's choice now,
        // whichever list it was, so there is nothing left to put back. Here
        // rather than on the theme's own arm because the settings page
        // reaches the same choice through a setting's value.
        self.theme_before = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::Action(at) => self.do_action(at),
            PickerValue::Answer(answer) => self.answered(answer),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            // Dealt with before the list is closed, the same as a commit:
            // a directory opens under itself rather than going anywhere.
            PickerValue::Directory(_) => {}
            PickerValue::Document(id) => {
                if self.current != Some(id) {
                    let from = self.here();
                    self.record(from);
                }
                if id.get() < self.documents.len() {
                    // Whatever the row names, not only a file: the list has
                    // conversations in it, and a row that did nothing when
                    // chosen would be a row that lies about being one.
                    self.go_to_document(id);
                }
            }
            PickerValue::Theme(name) => {
                // Kept, now that there is somewhere to keep it: a reader who
                // picks a theme and finds the old one back tomorrow has been
                // given a preview rather than a choice.
                self.change_setting("theme", &obelus_config::Value::Choice(name.clone()));
                if let Some(theme) = self.theme_called(&name) {
                    self.set_theme(&name, theme);
                }
            }
            PickerValue::Place {
                path,
                line,
                character,
                ..
            } => match matched {
                // A row of a search: land on what the query matched, which
                // is where the reader is looking. The list knows which
                // characters those are -- it marked them -- and the first
                // of them is the place.
                Some(column) => self.go_to_match(&path, line, column),
                None => self.go_to(&path, line, character),
            },
            PickerValue::Setting { key, word } => {
                // A number is written down as one. The list it was picked
                // from spells them, because a list is words either way.
                let value = match obelus_config::Setting::named(key).map(|setting| setting.kind) {
                    Some(obelus_config::Kind::Count(_)) => {
                        obelus_config::Value::Count(word.parse().unwrap_or(1))
                    }
                    _ => obelus_config::Value::Choice(word),
                };
                self.change_setting(key, &value);
            }
            PickerValue::AgentValue { setting, value } => {
                self.set_agent_setting(&setting, &value);
            }
            // The agent is named on the row rather than asked for now: the
            // list may have been opened before the reader changed agents,
            // and a value meant for one must not land on another.
            PickerValue::AgentDefault {
                agent,
                setting,
                value,
            } => {
                self.change_agent_default(&agent, &setting, value.as_deref());
            }
            // Dealt with before the list is closed: a commit opens its
            // files under it rather than going anywhere, and a file of one
            // is opened as that commit had it.
            PickerValue::Commit(_) => {}
            PickerValue::CommitFile { id, path } => self.open_at_commit(id, &path, None),
            // Dealt with before the list is closed, for the reason a
            // directory is: choosing one may leave the list where it was.
            PickerValue::Conversation(_) => {}
            PickerValue::Worktree(at) => self.go_to_worktree(at),
            PickerValue::Nothing => {}
        }
    }
}
