//! The lists obelus offers, and what choosing from one does.
//!
//! The list itself is [`crate::component::picker`]; what is here is which
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
        self.picker = Some(picker);
    }

    /// Offers the built-in themes.
    pub fn open_theme_picker(&mut self) {
        self.theme_before = Some(self.theme);
        let items = builtin::ALL
            .iter()
            .map(|theme| PickerItem {
                prose: false,
                marker: None,
                // The same glyph on every row, which is the honest one: what
                // distinguishes two themes is the colours, and the row's own
                // name is what says which.
                icon: icons::enabled().then_some(icons::ui::THEME),
                label: theme.name.to_string(),
                detail: None,
                trailing: None,
                changed: None,
                value: PickerValue::Theme(theme),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.when_empty("no theme is built in");
        // Open on the one that is on, so the list starts by saying which
        // theme this is rather than making the reader work it out.
        picker.prefer(self.theme.name.to_string());
        self.picker = Some(picker);
    }

    /// Offers every command by name.
    pub fn open_command_palette(&mut self) {
        // Every command, and what it can do *here* said by whether its row
        // can be chosen. Leaving out what cannot run makes the palette a
        // list nobody can learn from -- a reader who never sees `show-change`
        // does not find out obelus has it -- while a row that runs and then
        // reports why it did nothing is a row nobody trusts. Dim and
        // unselectable is both answers at once.
        let items = crate::command::ALL
            .iter()
            .map(|spec| PickerItem {
                prose: false,
                marker: None,
                icon: icons::enabled().then(|| icons::for_command(spec.command)),
                enabled: self.offers(spec.command),
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                label: spec.name.to_string(),
                // The tab it lives under. One past its position in the list
                // of groups, because the picker's own first tab is "all".
                tab: crate::command::Group::ALL
                    .iter()
                    .position(|group| *group == spec.command.group())
                    .map(|at| at + 1),
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
        picker.when_empty("no command by that name");
        // Tabs over one long list. Fourteen commands is already more than a
        // compact list shows at once, and the groups are what a reader is
        // choosing between when they do not already know the name.
        let names: Vec<&str> = crate::command::Group::ALL
            .iter()
            .map(|group| group.name())
            .collect();
        picker.with_tabs(&names);
        // Read down as much as it is typed at: most of what the palette
        // offers is what the reader came to find out, so the block stays the
        // height it opened at rather than closing up under the query.
        picker.keeps_height();
        self.picker = Some(picker);
    }

    /// Opens the log, as a file like any other.
    ///
    /// A reader is what obelus is, so the log needs no viewer of its own: it
    /// becomes a buffer, the watcher on its directory reloads it as it grows,
    /// and the cursor stays where it was put. What is in it that no screen
    /// shows is a server's own words -- its stderr, its handshake, and the
    /// requests obelus sent it.
    pub fn open_log(&mut self) {
        self.open_log_file(crate::logging::OBELUS, "no log file");
    }

    /// And the language servers' own, which is the other half of the same
    /// idea: a server's handshake and every request obelus sent it, in a
    /// file of its own because it is somebody else's program talking at a
    /// volume that would bury the dozen lines obelus has to say.
    pub fn open_server_log(&mut self) {
        self.open_log_file(crate::logging::SERVERS, "no server log file");
    }

    /// Opens whichever log, or says there is none.
    fn open_log_file(&mut self, prefix: &str, missing: &str) {
        match crate::logging::current_file(prefix) {
            Some(path) => self.open(&path),
            // Logging is allowed to fail without stopping obelus starting, so
            // there may genuinely be no file -- and a server log exists only
            // once a server has said something.
            None => self.note = Some(missing.to_string()),
        }
    }

    /// Whether a command can do its job right now.
    ///
    /// One exhaustive match over the conditions rather than a test per
    /// command: what each command needs is declared beside it in
    /// [`crate::command::Command::requires`], and this is the one place that
    /// turns a condition into a yes or no from the application's own state.
    /// A row that silently fails is worse than a row that is not there.
    #[must_use]
    pub fn offers(&self, command: Command) -> bool {
        let buffer = self.current_buffer();
        match command.requires() {
            Requires::Nothing => true,
            Requires::AFileOpen => buffer.is_some(),
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
            // has changed in the tree is git's to say, with every ignore
            // rule applied. Asked once when the palette opens and once per
            // press of the key it is on -- this repository answers in two
            // milliseconds, which is worth paying to stop offering a row
            // whose whole answer would be "nothing has changed".
            // What the tree offers on the line the cursor is on, which is
            // a question about this file and this line rather than about
            // the language: a file obelus parses can still have nothing to
            // fold where the reader is standing.
            // A walk of one commit, which is what "is there a history
            // here" costs: the same trade `AChangedFile` makes.
            Requires::AHistory => self.has_history(),
            Requires::SomethingToUndo => self.current_buffer().is_some_and(Buffer::can_undo),
            Requires::SomethingToRedo => self.current_buffer().is_some_and(Buffer::can_redo),
            Requires::AFoldHere => self.current_buffer().is_some_and(|buffer| {
                // A commit's message is a thing the reader is inside, and
                // the cursor cannot say so: it stays on the line the block
                // hangs above, answering for the *file*, while the caret is
                // up in rows the file does not have. Asking the file where
                // the cursor is would refuse the key on the one screen where
                // the reader can see what it would fold.
                buffer
                    .caret_block()
                    .and_then(|above| buffer.block_above(above))
                    .is_some_and(crate::buffer::Block::can_fold)
                    || buffer.folds().is_folded_at(buffer.cursor().line)
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
            // changed path to look at its length walks the tree each time.
            Requires::AChangedFile => self.anything_changed(),
            // The language's own fact, and a cheap one: what a file is
            // parsed as is already known.
            Requires::ALineComment => self
                .current_buffer()
                .and_then(Buffer::language)
                .is_some_and(|language| language.line_comment().is_some()),
            Requires::AServerLog => crate::logging::current_file(crate::logging::SERVERS).is_some(),
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
        if let PickerValue::Commit(id) = value {
            if self.expand_commit(id) {
                return;
            }
            if let Some(path) = self.commit_opens() {
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
        // The history goes with its list: the radii are what says a history
        // is open at all.
        self.history = crate::app::history_view::Showing::default();
        // A theme worn while walking a list is the reader's choice now,
        // whichever list it was, so there is nothing left to put back. Here
        // rather than on the theme's own arm because the settings page
        // reaches the same choice through a setting's value.
        self.theme_before = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::Answer(answer) => self.answered(answer),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            PickerValue::Buffer(id) => {
                if self.current != Some(id) {
                    let from = self.here();
                    self.record(from);
                }
                if id.get() < self.buffers.len() {
                    self.go_to_buffer(id);
                }
            }
            PickerValue::Theme(theme) => {
                // Kept, now that there is somewhere to keep it: a reader who
                // picks a theme and finds the old one back tomorrow has been
                // given a preview rather than a choice.
                self.change_setting(
                    "theme",
                    &crate::config::Value::Choice(theme.name.to_string()),
                );
                self.set_theme(theme);
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
                let value = match crate::config::Setting::named(key).map(|setting| setting.kind) {
                    Some(crate::config::Kind::Count(_)) => {
                        crate::config::Value::Count(word.parse().unwrap_or(1))
                    }
                    _ => crate::config::Value::Choice(word),
                };
                self.change_setting(key, &value);
            }
            PickerValue::AgentValue { setting, value } => {
                self.set_agent_setting(&setting, &value);
            }
            // Dealt with before the list is closed: a commit opens its
            // files under it rather than going anywhere, and a file of one
            // is opened as that commit had it.
            PickerValue::Commit(_) => {}
            PickerValue::CommitFile { id, path } => self.open_at_commit(id, &path, None),
            PickerValue::Nothing => {}
        }
    }
}
