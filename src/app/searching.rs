//! Searching, at its three scopes.
//!
//! The view is [`crate::component::picker`] and the scanning is
//! [`crate::search`]; what is here is which of them to ask, what to do with
//! what comes back, and how a row of a result is coloured.

use super::*;

impl App {
    /// Opens the search, at one of its three scopes.
    ///
    /// One view with three tabs rather than three views: the question is
    /// "where is this", and only its radius changes. The query survives a
    /// walk between the tabs, which is the whole point -- a reader who does
    /// not find it in this file looks in the project without retyping it.
    pub fn open_search(&mut self, scope: Scope) {
        // Only the scopes that can answer. A tab that says "no file open"
        // whenever it is walked onto is a tab in the way of the two that
        // work, and the tabs are how a reader moves between them.
        let scopes = self.searchable();
        let Some(tab) = scopes.iter().position(|shown| *shown == scope) else {
            self.note = Some(match scope {
                Scope::File => "no file open".to_string(),
                Scope::Symbols => "no language server to ask".to_string(),
                Scope::Project => "nowhere to search".to_string(),
            });
            return;
        };

        let names: Vec<&str> = scopes.iter().map(|scope| scope.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        picker.with_scopes(&names);
        picker.searches();
        picker.previews();
        // Three keys of its own, so it says what they are and which way each
        // is set: a switch a reader has to flip to find out is not one.
        picker.says_its_keys();
        picker.go_to_tab(tab);
        self.searching = scopes;
        self.picker = Some(picker);
        self.refresh_search();
    }

    /// Which scopes have something to search, in the order their tabs sit
    /// in.
    ///
    /// Settled when the search opens rather than watched while it is open: a
    /// server starts when a file does, so by the time a reader is searching
    /// there either is one or there is not -- and tabs appearing under the
    /// arrow keys would move the ground while they walk it.
    fn searchable(&self) -> Vec<Scope> {
        Scope::ALL
            .into_iter()
            .filter(|scope| match scope {
                Scope::File => self.current_buffer().is_some(),
                // Somewhere to walk is all this one needs, and there always
                // is: obelus is started in a directory.
                Scope::Project => true,
                Scope::Symbols => self
                    .current_buffer()
                    .and_then(Buffer::language)
                    .is_some_and(|language| self.servers.contains_key(&language)),
            })
            .collect()
    }

    /// Which scope the search is showing, if a search is open.
    pub(super) fn searching(&self) -> Option<Scope> {
        let picker = self.picker.as_ref()?;
        if !picker.is_searching() {
            return None;
        }
        self.searching.get(picker.tab()).copied()
    }

    /// Whether a row of the list on screen *is* the line it names.
    ///
    /// True of a search of a file and of the project: the row is that line,
    /// trimmed of its indentation, so a column of the row is a column of
    /// the line and the characters the query matched are characters of the
    /// code. False of everything else -- a symbol search's rows are names,
    /// an outline's are names, a file list's are paths -- and there a
    /// column of the row means nothing in the file.
    pub(super) fn rows_are_lines(&self) -> bool {
        self.searching().is_some_and(Scope::lists_lines)
    }

    /// The same, for a test: which list is showing is not otherwise
    /// visible, and this is a rule about two views agreeing.
    #[must_use]
    pub fn rows_are_lines_for_test(&self) -> bool {
        self.rows_are_lines()
    }

    /// How many files the search has parsed to colour its rows.
    ///
    /// Bounded by the rows that have been on screen, which is the rule worth
    /// stating: a project search can hold two thousand rows, and parsing
    /// every one of their files would be a search that takes as long as
    /// reading the project.
    #[must_use]
    pub fn files_parsed_for_rows(&self) -> usize {
        self.row_syntax.len()
    }

    /// Which search the rows arriving belong to.
    ///
    /// A reader types faster than a tree can be walked, so every batch
    /// carries the generation it was asked under and anything older is
    /// dropped. Public because the scan is started from outside the loop in
    /// tests, which have to say which search they are answering.
    #[must_use]
    pub fn search_generation(&self) -> u64 {
        self.search_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Says that what is being asked has changed, and returns the generation
    /// the answers must now carry. Every earlier scan learns from this that
    /// it can stop.
    fn ask_again(&mut self) -> u64 {
        self.search_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// Works out what the characters of the rows on screen *are*.
    ///
    /// A search result is a line of code, and it reads like one only if it
    /// is coloured like one. Only the rows on screen, and only once each:
    /// a project search can hold two thousand of them, and the query for
    /// one line's worth of a tree is small but not free.
    pub(super) fn colour_visible_rows(&mut self, height: u16) {
        if self.picker.is_none() {
            // Nothing is listing files any more, so nothing needs the trees
            // that were parsed to colour them.
            self.row_syntax.clear();
            return;
        }
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        // Whatever the list is, not only a search: a row that names a place
        // in a file is a row whose text can be coloured, and a list of
        // references is made of those too.
        let wanted: Vec<(usize, PathBuf, u32)> = picker
            .visible(height)
            .into_iter()
            .filter_map(|index| {
                let item = picker.rows_at(index)?;
                if item.colours.is_some() {
                    return None;
                }
                match &item.value {
                    PickerValue::Place { path, line, .. } => Some((index, path.clone(), *line)),
                    _ => None,
                }
            })
            .collect();

        for (index, path, line) in wanted {
            let colours = self.colours_of(&path, line);
            if let Some(picker) = self.picker.as_mut()
                && let Some(item) = picker.row_mut(index)
            {
                item.colours = Some(colours);
            }
        }
    }

    /// The kinds of the characters of one line of one file.
    ///
    /// Relative to the row's label, which starts at the line's first
    /// character that is not a space: the label is the line trimmed, and
    /// both ends of that trim are the same rule wherever a row is built.
    ///
    /// An empty list for a file obelus cannot parse or cannot read, which is
    /// an answer rather than a miss: the row is not asked about again.
    fn colours_of(&mut self, path: &Path, line: u32) -> Vec<Colouring> {
        // The file being read is already parsed, and its tree is the one
        // that matches what the reader is looking at.
        let open = self
            .buffers
            .iter()
            .flatten()
            .find(|buffer| buffer.path() == path);
        let buffer = match open {
            Some(buffer) => buffer,
            None => {
                if !self.row_syntax.contains_key(path) {
                    // Only as many as the rows that have been on screen, and
                    // dropped with the list. A file that will not open is
                    // not retried, because the row remembers the answer.
                    match Buffer::open(path) {
                        Ok(buffer) => {
                            self.row_syntax.insert(path.to_path_buf(), buffer);
                        }
                        Err(error) => {
                            tracing::debug!(%error, "no colours for a row");
                            return Vec::new();
                        }
                    }
                }
                match self.row_syntax.get(path) {
                    Some(buffer) => buffer,
                    None => return Vec::new(),
                }
            }
        };

        let Some(state) = buffer.syntax() else {
            return Vec::new();
        };
        let text = buffer.text();
        let line = LineNumber::new(line as usize);
        if line.get() >= text.line_count() {
            return Vec::new();
        }
        let start = text.line_start_byte(line);
        let end = text.line_start_byte(line.saturating_add(1));
        let mut highlights = Highlights::default();
        highlights.refresh(state, text, start..end);

        // The label starts at the first character that is not a space, and
        // its characters are counted from there.
        let contents = text.line(line).to_string();
        let indent = contents.chars().take_while(|c| c.is_whitespace()).count();
        let mut runs: Vec<Colouring> = Vec::new();
        let mut byte = start.get() + contents.char_indices().nth(indent).map_or(0, |(at, _)| at);
        for (column, character) in contents.chars().skip(indent).enumerate() {
            let kind = highlights.kind_at(ByteOffset::new(byte));
            byte += character.len_utf8();
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            match (kind, runs.last_mut()) {
                (Some(kind), Some(last)) if last.2 == kind && last.1 == column => last.1 += 1,
                (Some(kind), _) => runs.push((column, column + 1, kind)),
                (None, _) => {}
            }
        }
        runs
    }

    /// Fills the search with the rows of whichever scope is showing.
    ///
    /// Asked whenever the query or the tab moves, and every scope answers
    /// the same way: the rows are what has the query in it. It used to take
    /// a flag saying which of the two had moved, because the file's rows
    /// were every line of it and were worth gathering only once per visit --
    /// which is exactly the arrangement that let a fuzzy matcher stand
    /// between the reader and their answer.
    pub(super) fn refresh_search(&mut self) {
        let tab = self.picker.as_ref().map(Picker::tab);
        let Some(scope) = tab.and_then(|tab| self.searching.get(tab).copied()) else {
            return;
        };
        // What the foot says the switches are set to. Nothing on the symbols
        // tab: those rows come from a language server that did its own
        // matching and has never heard of our pattern.
        let how = (scope != Scope::Symbols).then_some(self.looking);
        // And the other way round: reaching past the project is a question
        // only an index that reaches past it can answer.
        let outside = (scope == Scope::Symbols).then_some(self.outside);
        if let Some(picker) = self.picker.as_mut() {
            picker.looking_how(how);
            picker.reaching_outside(outside);
        }
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        match scope {
            Scope::File => {
                if picker.query().is_empty() {
                    // Nothing asked, so nothing found -- not every line of
                    // the file, which the reader is looking at already and
                    // which says nothing a list could add. With no file at
                    // all the reason is that, which is a fact about the
                    // world rather than an invitation to type.
                    let reason = match self.current_buffer().is_some() {
                        true => "type to search this file",
                        false => "no file open",
                    };
                    self.searched = None;
                    if let Some(picker) = self.picker.as_mut() {
                        picker.replace(Vec::new());
                        picker.while_empty(reason);
                    }
                } else {
                    self.search_this_file();
                }
            }
            Scope::Project => self.search_the_project(),
            Scope::Symbols => self.search_the_symbols(),
        }
    }

    /// Whatever one of the search's own keys means, if it is one of them.
    ///
    /// Four switches, and which of them mean anything depends on the tab:
    /// three are about reading a text and the fourth is about how far an
    /// index reaches. Answered here rather than in the picker for the reason
    /// the file list's one is: the picker knows about rows and a query, and
    /// these are about how the rows were found.
    pub(super) fn searching_key(&mut self, key: &KeyEvent) -> bool {
        if key.modifiers != KeyModifiers::ALT {
            return false;
        }
        // Only where they mean something: on the symbols tab the server did
        // the matching, and the foot greys them there.
        let Some(picker) = self.picker.as_ref().filter(|picker| picker.is_searching()) else {
            return false;
        };
        let reading = picker.looks_how().is_some();
        let beyond = picker.reaches_outside().is_some();
        let KeyCode::Char(letter) = key.code else {
            return false;
        };
        match letter {
            'r' if reading => self.looking.regex = !self.looking.regex,
            'w' if reading => self.looking.word = !self.looking.word,
            'c' if reading => self.looking.sensitive = !self.looking.sensitive,
            'o' if beyond => self.outside = !self.outside,
            _ => return false,
        }
        self.refresh_search();
        true
    }

    /// What the search is looking for, the way the reader asked.
    fn needle(&self) -> search::Needle {
        let query = self
            .picker
            .as_ref()
            .map_or_else(String::new, |picker| picker.query().to_string());
        search::Needle::new(&query, self.looking)
    }

    /// The lines of the file being read that have the query in them.
    ///
    /// The matches rather than every line, by the same rule the walk of the
    /// tree follows: one question, and only its radius changes. The rows
    /// were the lines once, with the picker's fuzzy matcher narrowing them,
    /// which meant `ac` found `abc` -- an answer to a question about
    /// resemblance, in a view whose question is where a string is.
    ///
    /// Done here and now rather than on a thread: the file is already in
    /// memory, and a pass over it per keystroke is nothing beside the walk
    /// the project tab starts for the same key.
    pub(super) fn search_this_file(&mut self) {
        let needle = self.needle();
        if needle.is_broken() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("that is not a pattern");
            }
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.when_empty("no file open");
            }
            return;
        };
        let path = buffer.path().to_path_buf();
        let version = buffer.version();
        let text = buffer.text();
        // The file's own encoding if a server is attached to it, because the
        // row's position is handed back through the same door a server's
        // answer goes through.
        let encoding = buffer
            .syntax()
            .map(SyntaxState::language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |language| {
                self.encoding_for(language)
            });

        let items: Vec<PickerItem> = (0..text.line_count())
            .filter_map(|number| {
                let line = LineNumber::new(number);
                // Against the line as it is written, not as the row shows
                // it: a row is trimmed of its indentation, and a query for
                // a run of spaces would otherwise find nothing anywhere.
                let said = text.line(line).to_string();
                needle.found_in(&said)?;
                let at = position::to_lsp(text, line, CharColumn::new(0), &encoding);
                let end = position::to_lsp(text, line, text.line_length(line), &encoding);
                Some(PickerItem {
                    prose: false,
                    marker: None,
                    icon: None,
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: 0,
                    kind: None,
                    // Trimmed at the front: the indentation is the same on
                    // every row of a block, so showing it spends the width
                    // where the answer is.
                    label: said.trim_end().trim_start().to_string(),
                    detail: None,
                    trailing: Some(format!("{}", number + 1)),
                    changed: None,
                    value: PickerValue::Place {
                        path: path.clone(),
                        line: at.line,
                        character: at.character,
                        end_line: end.line,
                        end_character: end.character,
                    },
                    tab: None,
                })
            })
            .collect();

        self.searched = Some((path, version));
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.while_empty("no match in this file");
        }
    }

    /// Starts a walk of the tree looking for the query.
    ///
    /// A thread per query, and the answers carry the generation they were
    /// asked under: a reader types faster than a tree can be walked, so the
    /// rows for "sc" must not land in a list that is now asking about
    /// "scope".
    fn search_the_project(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().to_string());
        let Some(query) = query else { return };

        // Only the empty query is not a search: it matches every line of
        // every file, which is the tree rather than an answer. One letter is
        // a real question -- and the cheapest one there is, because it fills
        // the row limit in the first few files and stops.
        let generation = self.ask_again();
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("type to search every file");
            }
            return;
        }

        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty("searching\u{2026}");
        }
        let needle = self.needle();
        if needle.is_broken() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("that is not a pattern");
            }
            return;
        }
        if let Some(sender) = self.events.clone() {
            search::spawn_scan(
                &self.working_directory,
                &needle,
                generation,
                self.config().ignored_files,
                &self.search_generation,
                sender,
            );
        }
    }

    /// Puts a batch of matching lines into the list waiting for them.
    pub(super) fn on_matches(&mut self, generation: u64, hits: Vec<search::Hit>, done: bool) {
        if generation != self.search_generation() {
            tracing::debug!(
                generation,
                "dropping matches for a query already typed past"
            );
            return;
        }
        let root = self.working_directory.clone();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if self.searching.get(picker.tab()) != Some(&Scope::Project) {
            return;
        }
        picker.extend(hits.into_iter().map(|hit| PickerItem {
            prose: false,
            marker: None,
            icon: None,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            label: hit.text,
            detail: None,
            trailing: Some(format!("{}:{}", hit.path.display(), hit.line + 1)),
            changed: None,
            // The line, not the column: the file is not open, so its text --
            // which is what a column in the protocol's units is counted
            // against -- is not here to count with. The row highlights what
            // matched, which is where the reader is looking anyway.
            value: PickerValue::Place {
                path: root.join(&hit.path),
                line: u32::try_from(hit.line).unwrap_or(u32::MAX),
                character: 0,
                end_line: u32::try_from(hit.line).unwrap_or(u32::MAX),
                end_character: 0,
            },
            tab: None,
        }));
        if done {
            // Now it is true that there is no match, and the row count says
            // it about the *project* rather than about the list: the picker's
            // own "no match" is about a query against rows it was given, and
            // here the rows never existed.
            picker.while_empty("no match in the project");
        }
    }

    /// Asks the language server for the names it knows across the project.
    fn search_the_symbols(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().to_string());
        let Some(query) = query else { return };

        let language = self.current_buffer().and_then(Buffer::language);
        let Some(language) = language.filter(|language| self.servers.contains_key(language)) else {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                // The server is per language, and the language comes from
                // the file being read: with nothing open there is nobody to
                // ask, which is a different thing from an empty answer.
                picker.while_empty("no language server to ask");
            }
            return;
        };
        // An empty query asks a server for every name it knows, which is
        // its whole index; the protocol allows it and no server means it.
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("type to search the project's symbols");
            }
            return;
        }

        let asked = self.ask_workspace_symbols(language, &query);
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty(if asked {
                "asking the language server\u{2026}"
            } else {
                "the language server would not answer"
            });
        }
    }
}
