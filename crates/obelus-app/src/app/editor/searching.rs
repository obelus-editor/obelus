//! Searching, at its three scopes, each a view of its own.
//!
//! The view is [`obelus_component::picker`] and the scanning is
//! [`obelus_search`]; what is here is which of them to ask, what to do with
//! what comes back, and how a row of a result is coloured.
//!
//! The narrowest scope is whatever is being read, and that is not always a
//! file: a conversation, a pull request's page, a file's reading and the
//! notes are searched too, each in the terms its own cursor stands in.

use obelus_component::page::Place;

use crate::app::*;

/// What the narrowest scope is searching: the document in front, by the
/// kind of thing it is.
///
/// One list whatever it is, because the question is the same -- where in
/// this is the thing I am looking at -- and only what "this" is changes.
/// What changes with it is what a row is counted in and where choosing one
/// puts the cursor: a line of a file, a row of a page laid out at the width
/// it is drawn at, or a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Here {
    /// A file, read as its lines.
    File,
    /// A file shown as its reading: the rows as they are drawn, which is
    /// where the reading's cursor stands, and not the lines behind them.
    Reading,
    /// A conversation's transcript.
    Conversation,
    /// A pull request or an issue.
    Page,
    /// The notes.
    Notes,
}

impl Here {
    /// What typing does, for the row before anything has been typed.
    const fn before_typing(self) -> &'static str {
        match self {
            Self::File => "Search this file",
            Self::Reading => "Search this reading",
            Self::Conversation => "Search this conversation",
            Self::Page => "Search this page",
            Self::Notes => "Search the notes",
        }
    }

    /// What the list says before anything is asked.
    const fn invitation(self) -> &'static str {
        match self {
            Self::File => "Type to search this file",
            Self::Reading => "Type to search this reading",
            Self::Conversation => "Type to search this conversation",
            Self::Page => "Type to search this page",
            Self::Notes => "Type to search the notes",
        }
    }

    /// What the list says when nothing has the query in it.
    const fn nothing_found(self) -> &'static str {
        match self {
            Self::File => "No match in this file",
            Self::Reading => "No match in this reading",
            Self::Conversation => "No match in this conversation",
            Self::Page => "No match on this page",
            Self::Notes => "No match in the notes",
        }
    }
}

/// Where one scope's last search was left.
///
/// Kept so that the search a reader opens again is the one they left: the
/// same words, held so that typing replaces them, and the same row under
/// them. A file's rows are found again, which costs nothing and answers
/// for the text as it is now -- and the row stood on is only stood on again
/// in the document it was a row of. The project's rows are kept whole,
/// because finding them again is a walk of the project, and the first key
/// typed walks it afresh.
#[derive(Debug)]
pub(in crate::app) struct Kept {
    /// Which scope.
    scope: Scope,
    /// What it was asking.
    query: String,
    /// Which row the reader was on.
    selected: usize,
    /// Which document the rows were about, for the narrowest scope.
    about: Option<(DocumentId, Here)>,
    /// What the walk found, for the project's: all of it, or nothing where
    /// it had not finished.
    rows: Option<Vec<PickerItem>>,
}

impl App {
    /// Remembers where the search on screen is, as it is put away.
    ///
    /// Asked wherever a list goes: chosen from, left, or replaced by another
    /// list. Nothing for a list that is not the search.
    pub(in crate::app) fn keep_the_search(&mut self) {
        let Some(picker) = self.picker.as_ref().filter(|picker| picker.is_searching()) else {
            return;
        };
        let Some(scope) = self.search.searching.get(picker.tab()).copied() else {
            return;
        };
        let query = picker.query();
        let rows = (scope == Scope::Project && self.search.walked)
            .then(|| picker.matches().cloned().collect());
        let kept = Kept {
            scope,
            query: query.clone(),
            selected: picker.selected(),
            about: self.current.zip(self.search.here),
            rows,
        };
        self.search.last_query = query;
        self.search.kept.retain(|each| each.scope != scope);
        self.search.kept.push(kept);
    }

    /// Opens the search, at one of its three scopes.
    ///
    /// Three views rather than one with three tabs, because what each is
    /// read beside differs. The document in front is searched from a list on
    /// the status bar, so the document stays on screen and is its own
    /// preview -- the place a row names is the page above it, scrolled
    /// there. The project and its symbols cover the screen, because their
    /// rows are in files nobody has open and the preview under them is the
    /// only place to read those. What the last search asked goes with the
    /// reader from one to the next, held (`Kept`), so a reader who does not
    /// find it in this document looks in the project without retyping it.
    pub fn open_search(&mut self, scope: Scope) {
        let here = self.searched_here();
        let able = match scope {
            Scope::File => here.is_some(),
            // Somewhere to walk is all this one needs, and there always is:
            // Obelus is started in a directory.
            Scope::Project => true,
            Scope::Symbols => self
                .current_buffer()
                .and_then(Buffer::language)
                .is_some_and(|language| self.servers.contains_key(&language)),
        };
        if !able {
            self.wrong(match scope {
                Scope::File => "Nothing here to search".to_string(),
                Scope::Symbols => "No language server to ask".to_string(),
                Scope::Project => "Nowhere to search".to_string(),
            });
            return;
        }
        let (layout, opener) = match scope {
            Scope::File => (
                PickerLayout::Compact {
                    rows: crate::app::COMPACT_ROWS,
                },
                Command::SearchFile,
            ),
            Scope::Project => (PickerLayout::FullArea, Command::SearchProject),
            Scope::Symbols => (PickerLayout::FullArea, Command::SearchSymbols),
        };
        let mut picker = Picker::new(Vec::new(), layout);
        picker.searches();
        picker.previews();
        // Three keys of its own, so it says what they are and which way each
        // is set: a switch a reader has to flip to find out is not one.
        picker.says_its_keys();
        // The key that opened it is this list: pressed again it stays, with
        // what was typed, and another search's key is another view.
        picker.opened_by(opener);
        self.show_list(picker);
        self.search.searching = vec![scope];
        self.search.here = (scope == Scope::File).then_some(here).flatten();
        self.search.looked_within = None;
        self.search.looked_at = None;
        // The words the last search was asking, held, so that the search
        // opened again is that search -- and a reader who meant another one
        // types it over them.
        let query = self.search.last_query.clone();
        if let Some(picker) = self.picker.as_mut()
            && !query.is_empty()
        {
            picker.set_query(&query);
            picker.hold_in_query(true);
        }
        // And the row the reader was on, where this scope was left asking
        // the same thing -- about the same document, for the narrowest.
        let about = self.current.zip(here);
        let kept = self.search.kept.iter().find(|kept| {
            kept.scope == scope
                && kept.query == query
                && (scope != Scope::File || kept.about == about)
        });
        let selected = kept.map(|kept| kept.selected);
        self.search.reuse = kept
            .filter(|kept| kept.scope == Scope::Project)
            .and_then(|kept| kept.rows.clone());
        self.refresh_search();
        if let Some(selected) = selected
            && let Some(picker) = self.picker.as_mut()
        {
            picker.select_row(selected.min(picker.match_count().saturating_sub(1)));
        }
    }

    /// What the narrowest scope would search, if a search opened now.
    ///
    /// Nothing in a terminal, whose screen is the program's and drawn a
    /// screen at a time, and nothing with nothing open.
    pub(in crate::app) fn searched_here(&self) -> Option<Here> {
        if self
            .current_buffer()
            .is_some_and(|buffer| buffer.mode() == Mode::Preview)
            && self.rendering().is_some()
        {
            return Some(Here::Reading);
        }
        match self.document(self.current?)? {
            Document::File(_) => Some(Here::File),
            Document::Chat(_) => Some(Here::Conversation),
            Document::Discussion(_) => Some(Here::Page),
            Document::Notes(_) => Some(Here::Notes),
            Document::Terminal(_) => None,
        }
    }

    /// Which scope the search is showing, if a search is open.
    pub(in crate::app) fn searching(&self) -> Option<Scope> {
        let picker = self.picker.as_ref()?;
        if !picker.is_searching() {
            return None;
        }
        self.search.searching.get(picker.tab()).copied()
    }

    /// Whether a row of the list on screen *is* the line it names.
    ///
    /// True of a search of a file and of the project: the row is that line,
    /// trimmed of its indentation, so a column of the row is a column of
    /// the line and the characters the query matched are characters of the
    /// code. False of everything else -- a symbol search's rows are names,
    /// an outline's are names, a file list's are paths -- and there a
    /// column of the row means nothing in the file.
    pub(in crate::app) fn rows_are_lines(&self) -> bool {
        match self.searching() {
            // A row of a page is a row of the page, and says itself where
            // it matched rather than through a column of a line.
            Some(Scope::File) => self.search.here == Some(Here::File),
            scope => scope.is_some_and(Scope::lists_lines),
        }
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
        self.search.row_syntax.len()
    }

    /// Which search the rows arriving belong to.
    ///
    /// A reader types faster than a project can be walked, so every batch
    /// carries the generation it was asked under and anything older is
    /// dropped. Public because the scan is started from outside the loop in
    /// tests, which have to say which search they are answering.
    #[must_use]
    pub fn search_generation(&self) -> u64 {
        self.search.search_generation.now()
    }

    /// Says that what is being asked has changed, and returns the generation
    /// the answers must now carry. Every earlier scan learns from this that
    /// it can stop.
    fn ask_again(&mut self) -> u64 {
        self.search.search_generation.next()
    }

    /// Works out what the characters of the rows on screen *are*.
    ///
    /// A search result is a line of code, and it reads like one only if it
    /// is coloured like one. Only the rows on screen, and only once each:
    /// a project search can hold two thousand of them, and the query for
    /// one line's worth of a tree is small but not free.
    pub(in crate::app) fn colour_visible_rows(&mut self, height: u16) {
        if self.picker.is_none() {
            // Nothing is listing files any more, so nothing needs the trees
            // that were parsed to colour them.
            self.search.row_syntax.clear();
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
    /// An empty list for a file Obelus cannot parse or cannot read, which is
    /// an answer rather than a miss: the row is not asked about again.
    fn colours_of(&mut self, path: &Path, line: u32) -> Vec<Colouring> {
        // The file being read is already parsed, and its tree is the one
        // that matches what the reader is looking at.
        let open = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .find(|buffer| buffer.path() == path);
        let buffer = match open {
            Some(buffer) => buffer,
            None => {
                if !self.search.row_syntax.contains_key(path) {
                    // Only as many as the rows that have been on screen, and
                    // dropped with the list. A file that will not open is
                    // not retried, because the row remembers the answer.
                    match Buffer::open(path) {
                        Ok(buffer) => {
                            self.search.row_syntax.insert(path.to_path_buf(), buffer);
                        }
                        Err(error) => {
                            tracing::debug!(%error, "no colours for a row");
                            return Vec::new();
                        }
                    }
                }
                match self.search.row_syntax.get(path) {
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
        let mut byte =
            start.saturating_add(contents.char_indices().nth(indent).map_or(0, |(at, _)| at));
        for (column, character) in contents.chars().skip(indent).enumerate() {
            let kind = highlights.kind_at(byte);
            byte = byte.saturating_add(character.len_utf8());
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            match (kind, runs.last_mut()) {
                (Some(kind), Some(last)) if last.2 == Tint::Code(kind) && last.1 == column => {
                    last.1 += 1;
                }
                (Some(kind), _) => runs.push((column, column + 1, Tint::Code(kind))),
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
    pub(in crate::app) fn refresh_search(&mut self) {
        let tab = self.picker.as_ref().map(Picker::tab);
        let Some(scope) = tab.and_then(|tab| self.search.searching.get(tab).copied()) else {
            return;
        };
        // What the foot says the switches are set to. Nothing on the symbols
        // tab: those rows come from a language server that did its own
        // matching and has never heard of our pattern.
        let how = (scope != Scope::Symbols).then_some(self.search.looking);
        // And the other way round: reaching past the project is a question
        // only an index that reaches past it can answer.
        let outside = (scope == Scope::Symbols).then_some(self.search.outside);
        // Which files the walk reads, which is the file list's two switches
        // and asked of the same settings: a search obeying them without
        // saying so is a search that finds `target` and does not say why.
        // Only the project's tab walks anything.
        let walking = scope == Scope::Project;
        let ignored = walking.then_some(self.config().ignored_files);
        let hidden = walking.then_some(self.config().hidden_files);
        let here = self.search.here.unwrap_or(Here::File);
        // A row of a page names nowhere a preview could show: only the
        // page's own view can draw it, and that is the page under the list.
        let previews = scope != Scope::File || here == Here::File;
        if let Some(picker) = self.picker.as_mut() {
            if previews {
                picker.previews();
            } else {
                picker.stops_previewing();
            }
            picker.looking_how(how);
            picker.reaching_outside(outside);
            picker.offering_ignored(ignored);
            picker.offering_hidden(hidden);
            // And what typing does, which is the one thing on this row
            // that changes with the tab: the same keystroke looks in one
            // file, in every file, or at a server's index.
            picker.before_typing(match scope {
                Scope::File => here.before_typing(),
                _ => scope.before_typing(),
            });
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
                    let reason = match self.searched_here() {
                        Some(_) => here.invitation(),
                        None => "Nothing here to search",
                    };
                    self.search.searched = None;
                    if let Some(picker) = self.picker.as_mut() {
                        picker.replace(Vec::new());
                        picker.while_empty(reason);
                    }
                } else if here == Here::File {
                    self.search_this_file();
                } else {
                    self.search_this_document(here);
                }
            }
            // What the last walk found for the same words, put back rather
            // than walked for again -- once: the next key asks afresh.
            Scope::Project => match self.search.reuse.take() {
                Some(rows) => {
                    // Whatever is still walking is answering a question
                    // nobody is asking now.
                    self.ask_again();
                    self.search.walked = true;
                    if let Some(picker) = self.picker.as_mut() {
                        picker.replace(rows);
                        picker.while_empty("No match in the project");
                    }
                }
                None => self.search_the_project(),
            },
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
    ///
    /// The project's tab has two more, which files it walks, and those are
    /// the file list's settings and answered where the file list's are
    /// (`listing_key`).
    pub(in crate::app) fn searching_key(&mut self, key: &KeyEvent) -> bool {
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
            'r' if reading => self.search.looking.regex = !self.search.looking.regex,
            'w' if reading => self.search.looking.word = !self.search.looking.word,
            'c' if reading => self.search.looking.sensitive = !self.search.looking.sensitive,
            'o' if beyond => self.search.outside = !self.search.outside,
            _ => return false,
        }
        self.refresh_search();
        true
    }

    /// What the search is looking for, the way the reader asked.
    fn needle(&self) -> obelus_search::Needle {
        let query = self
            .picker
            .as_ref()
            .map_or_else(String::new, |picker| picker.query().to_string());
        obelus_search::Needle::new(&query, self.search.looking)
    }

    /// The lines of the file being read that have the query in them.
    ///
    /// The matches rather than every line, by the same rule the walk of the
    /// project follows: one question, and only its radius changes. The rows
    /// were the lines once, with the picker's fuzzy matcher narrowing them,
    /// which meant `ac` found `abc` -- an answer to a question about
    /// resemblance, in a view whose question is where a string is.
    ///
    /// Done here and now rather than on a thread: the file is already in
    /// memory, and a pass over it per keystroke is nothing beside the walk
    /// the project tab starts for the same key.
    pub(in crate::app) fn search_this_file(&mut self) {
        let needle = self.needle();
        if needle.is_broken() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("That is not a pattern");
            }
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.when_empty("No file open");
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
                    opens: None,
                    kind: None,
                    // Trimmed at the front: the indentation is the same on
                    // every row of a block, so showing it spends the width
                    // where the answer is.
                    label: said.trim_end().trim_start().to_string(),
                    version: None,
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
                    section: None,
                })
            })
            .collect();

        self.search.searched = Some((path, version));
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.while_empty("No match in this file");
        }
    }

    /// The rows of the document in front that have the query in them, where
    /// that is not a file.
    ///
    /// A row each, as it is drawn: a page is laid out at the width it is
    /// read at, and its cursor stands on those rows, so a row is what a
    /// match can be gone to. Which also means a query that a row's end
    /// breaks in two is not found, and neither is anything folded shut,
    /// which has no rows -- the same as a reader running their eye down
    /// it. The notes are the exception, being notes rather than rows: a
    /// row for each line of a note's words, gone to by the note.
    fn search_this_document(&mut self, here: Here) {
        let needle = self.needle();
        if needle.is_broken() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("That is not a pattern");
            }
            return;
        }
        let found = match here {
            // Not here: a file's lines are `search_this_file`'s.
            Here::File => Vec::new(),
            Here::Reading => self.rendering().map_or_else(Vec::new, |rows| {
                let laid: Vec<_> = rows
                    .iter()
                    .map(obelus_component::reading::Laid::of)
                    .collect();
                rows_found(&laid, &needle)
            }),
            Here::Conversation => {
                let width = obelus_ui::chat::reading_width(self.editor_area);
                self.chat()
                    .map_or_else(Vec::new, |chat| rows_found(&chat.rows(width), &needle))
            }
            Here::Page => {
                let width = obelus_ui::discussion::reading_width(self.editor_area);
                self.discussion()
                    .map_or_else(Vec::new, |page| rows_found(&page.rows(width), &needle))
            }
            Here::Notes => self
                .notes()
                .map_or_else(Vec::new, |notes| notes_found(notes, &needle)),
        };
        let items: Vec<PickerItem> = found
            .into_iter()
            .map(|(row, found, said)| PickerItem {
                prose: false,
                marker: None,
                icon: None,
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                label: said.trim().to_string(),
                version: None,
                detail: None,
                // No number: a row of a page is a place the width made, and
                // its number says nothing a reader could go and find.
                trailing: None,
                changed: None,
                value: PickerValue::Within {
                    row,
                    character: found.start,
                    end: found.end,
                },
                tab: None,
                section: None,
            })
            .collect();
        self.search.searched = None;
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.while_empty(here.nothing_found());
        }
    }

    /// Goes to a row of the search of the document in front, chosen.
    ///
    /// By what the search searched rather than by what is in front now,
    /// which is the same thing -- the list covered it -- unless something
    /// moved it underneath, and then the row is about a page that is not
    /// there and goes nowhere.
    pub(in crate::app) fn go_within(&mut self, row: usize, character: usize) {
        // Going there, so there is no going back to where it was, and the
        // cursor there says where the match is rather than a mark.
        self.forget_the_find();
        self.search.looked_within = None;
        let Some(here) = self.search.here.take() else {
            return;
        };
        if self.searched_here() != Some(here) {
            return;
        }
        let at = Place { row, character };
        match here {
            Here::File => {}
            Here::Reading => {
                if let Some(rendered) = self.rendered.as_mut() {
                    rendered.cursor.let_go();
                    rendered.cursor.stand(at);
                }
                self.show_in_the_reading(row);
            }
            Here::Conversation => {
                let room = self.chat_room();
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.stand_in_the_transcript(at, room);
                }
            }
            Here::Page => {
                let room = self.discussion_room();
                if let Some(page) = self.discussion_mut() {
                    page.stand_in_view(at, room);
                }
            }
            Here::Notes => {
                if let Some(notes) = self.notes_mut() {
                    notes.stand_in(row, character);
                }
            }
        }
    }

    /// Puts a row of the reading on screen.
    ///
    /// The view is the buffer's, and moves only as far as it takes to show
    /// the row: the arithmetic a key's walk does, on a window lent for it.
    fn show_in_the_reading(&mut self, row: usize) {
        let room = self.reading_room();
        let Some(top) = self
            .current_buffer()
            .map(|buffer| buffer.viewport().top.get())
        else {
            return;
        };
        let Some(count) = self.rendered_rows() else {
            return;
        };
        let mut window = obelus_component::window::Window::new();
        window.set_count(count);
        window.drag_to(top);
        obelus_component::page::show(&mut window, row, count, room.height);
        let by = isize::try_from(window.top()).unwrap_or(isize::MAX)
            - isize::try_from(top).unwrap_or(isize::MAX);
        if by != 0
            && let Some(buffer) = self.current_buffer_mut()
        {
            buffer.scroll_rendering(by, count, room.height);
        }
    }

    /// Shows the reader where the search's selection is, in the page the
    /// list sits on.
    ///
    /// What `look_at_the_selection` is for a file, for every other document:
    /// the list is on the status bar so that the page stays readable, and
    /// then the page is the preview. A look and not a move -- the cursor
    /// stays where the reader left it, and the view goes back where it was
    /// if they leave without choosing (`put_the_page_back`). Asked every
    /// frame, and does nothing unless the row has moved since the last.
    pub(in crate::app) fn look_within_at_the_selection(&mut self) {
        let Some(here) = self.search.here.filter(|here| *here != Here::File) else {
            return;
        };
        let Some(picker) = self.picker.as_ref().filter(|picker| picker.is_searching()) else {
            return;
        };
        if self.searched_here() != Some(here) {
            return;
        }
        let found = match picker.selected_item().map(|item| item.value.clone()) {
            Some(PickerValue::Within {
                row,
                character,
                end,
            }) => Some((row, character..end)),
            _ => None,
        };
        let at = found.as_ref().map(|(row, found)| (*row, found.start));
        if self.search.looked_at == at {
            return;
        }
        self.search.looked_at = at;
        // What it found is marked on the page, and a list that has found
        // nothing takes the last mark away.
        self.find_within(here, found);
        let Some((row, character)) = at else {
            return;
        };
        if self.search.looked_within.is_none() {
            self.search.looked_within = self.page_top(here);
        }
        match here {
            Here::File => {}
            Here::Reading => self.show_in_the_reading(row),
            Here::Conversation => {
                // The list is drawn over a conversation rather than taking
                // room from it, so what can be seen of the transcript is
                // what is above the list.
                let mut room = self.chat_room();
                let above = self
                    .picker
                    .as_ref()
                    .map(|picker| obelus_ui::picker::room_above(picker, self.editor_area));
                if let (Some(above), Some(talk)) = (above, self.conversation()) {
                    let band =
                        obelus_ui::chat::bands(self.editor_area, &talk.chat, talk.card_in_front())
                            .transcript;
                    let seen = (above.y + above.height).saturating_sub(band.y);
                    room.transcript = room.transcript.min(seen).max(1);
                }
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.look_at_row(row, room);
                }
            }
            Here::Page => {
                let room = self.discussion_room();
                if let Some(page) = self.discussion_mut() {
                    page.look_at_row(row, room);
                }
            }
            Here::Notes => {
                let height = obelus_ui::todo::list_region(self.editor_area).height;
                if let Some(notes) = self.notes_mut() {
                    notes.look_at_note(row, character, height);
                }
            }
        }
    }

    /// Works out what the search of the file marks in the file under it:
    /// the characters the query matched on the row the reader is on.
    ///
    /// Asked every frame, and nothing but a line's indentation is read: the
    /// list has already said which characters of the row matched.
    pub(in crate::app) fn mark_the_match(&mut self) {
        self.search.marked.clear();
        if self.search.here != Some(Here::File) || !self.rows_are_lines() {
            return;
        }
        let Some((line, _)) = self.the_selection_in_this_file() else {
            return;
        };
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let columns = picker.indices_at(picker.selected());
        let (Some(first), Some(last)) = (columns.iter().min(), columns.iter().max()) else {
            return;
        };
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // A row is the line trimmed of its indentation, so its columns are
        // the line's from where the indentation ends.
        let indent = buffer
            .text()
            .line(line)
            .chars()
            .take_while(|character| character.is_whitespace())
            .count();
        let at = |column: u32| CharColumn::new(indent + column as usize);
        self.search.marked.push(obelus_text::coordinates::Span {
            line,
            column: at(*first),
            end_line: line,
            end_column: at(*last + 1),
        });
    }

    /// Marks what a search found on the page it is searching, or takes the
    /// mark away: which row, and which of its characters -- for the notes,
    /// which note and which characters of its words.
    fn find_within(&mut self, here: Here, found: Option<(usize, std::ops::Range<usize>)>) {
        match here {
            // The file's is `mark_the_match`, which the editor reads.
            Here::File => {}
            Here::Reading => {
                if let Some(rendered) = self.rendered.as_mut() {
                    rendered.cursor.find(found);
                }
            }
            Here::Conversation => {
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.find(found);
                }
            }
            Here::Page => {
                if let Some(page) = self.discussion_mut() {
                    page.find(found);
                }
            }
            Here::Notes => {
                if let Some(notes) = self.notes_mut() {
                    notes.find(found);
                }
            }
        }
    }

    /// Takes the search's mark off the page it was searching, as the list
    /// goes: chosen from or left.
    pub(in crate::app) fn forget_the_find(&mut self) {
        self.search.looked_at = None;
        if let Some(here) = self.search.here
            && self.searched_here() == Some(here)
        {
            self.find_within(here, None);
        }
    }

    /// Where the page under a search of it is scrolled to, for the pages
    /// that keep that themselves.
    fn page_top(&self, here: Here) -> Option<usize> {
        match here {
            // A file and its reading are scrolled by the buffer, which the
            // list already keeps (`looked_from`).
            Here::File | Here::Reading => None,
            Here::Conversation => self.chat().map(obelus_component::chat::Chat::top),
            Here::Page => self
                .discussion()
                .map(obelus_component::discussion::DiscussionView::top),
            Here::Notes => self.notes().map(|notes| notes.window().top()),
        }
    }

    /// Puts the page under a search back where it was before the list
    /// showed the reader somewhere else in it.
    pub(in crate::app) fn put_the_page_back(&mut self) {
        let (Some(top), Some(here)) = (self.search.looked_within.take(), self.search.here) else {
            return;
        };
        self.search.looked_at = None;
        if self.searched_here() != Some(here) {
            return;
        }
        match here {
            Here::File | Here::Reading => {}
            Here::Conversation => {
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.drag_to(top);
                }
            }
            Here::Page => {
                if let Some(page) = self.discussion_mut() {
                    page.drag_to(top);
                }
            }
            Here::Notes => {
                if let Some(notes) = self.notes_mut() {
                    notes.drag_to(top);
                }
            }
        }
    }

    /// Starts a walk of the project looking for the query.
    ///
    /// A thread per query, and the answers carry the generation they were
    /// asked under: a reader types faster than a project can be walked, so the
    /// rows for "sc" must not land in a list that is now asking about
    /// "scope".
    fn search_the_project(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().to_string());
        let Some(query) = query else { return };

        // Only the empty query is not a search: it matches every line of
        // every file, which is the project rather than an answer. One letter is
        // a real question -- and the cheapest one there is, because it fills
        // the row limit in the first few files and stops.
        let generation = self.ask_again();
        self.search.walked = false;
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("Type to search every file");
            }
            return;
        }

        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty("Searching\u{2026}");
        }
        let needle = self.needle();
        if needle.is_broken() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("That is not a pattern");
            }
            return;
        }
        if let Some(sender) = self.events.clone() {
            obelus_search::spawn_scan(
                &self.working_directory,
                &needle,
                self.search.search_generation.claim(generation),
                self.config().ignored_files,
                self.config().hidden_files,
                sender,
            );
        }
    }

    /// Puts a batch of matching lines into the list waiting for them.
    pub(in crate::app) fn on_matches(
        &mut self,
        generation: u64,
        hits: Vec<obelus_search::Hit>,
        done: bool,
    ) {
        if !self.search.search_generation.is_current(generation) {
            tracing::debug!(
                generation,
                "dropping matches for a query already typed past"
            );
            return;
        }
        let root = self.working_directory.clone();
        // Only into the search of the project. Asked of the list as well as
        // of the scope it was opened at, because the scope outlives the
        // list: a list opened in its place -- the palette -- would be
        // standing on that scope's tab.
        let Some(picker) = self.picker.as_mut().filter(|picker| picker.is_searching()) else {
            return;
        };
        if self.search.searching.get(picker.tab()) != Some(&Scope::Project) {
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
            opens: None,
            kind: None,
            label: hit.text,
            version: None,
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
            section: None,
        }));
        if done {
            // Now it is true that there is no match, and the row count says
            // it about the *project* rather than about the list: the picker's
            // own "no match" is about a query against rows it was given, and
            // here the rows never existed.
            picker.while_empty("No match in the project");
            self.search.walked = true;
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
                picker.while_empty("No language server to ask");
            }
            return;
        };
        // An empty query asks a server for every name it knows, which is
        // its whole index; the protocol allows it and no server means it.
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("Type to search the project's symbols");
            }
            return;
        }

        let asked = self.ask_workspace_symbols(language, &query);
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty(if asked {
                "Asking the language server\u{2026}"
            } else {
                "The language server would not answer"
            });
        }
    }
}

/// The rows of a page that have the query in them: each one's place among
/// them, which of its characters the query found, and what it says.
fn rows_found<L: obelus_component::page::Line>(
    rows: &[L],
    needle: &obelus_search::Needle,
) -> Vec<(usize, std::ops::Range<usize>, String)> {
    rows.iter()
        .enumerate()
        .filter_map(|(at, row)| {
            let said = obelus_component::page::text(row);
            let found = needle.found_in(&said)?;
            Some((at, found, said))
        })
        .collect()
}

/// The lines of the notes that have the query in them: which note, which
/// characters of its words the query found, and the line.
///
/// Counted through the whole of a note's words, its line breaks among
/// them, because that is how far along them the caret is walked.
fn notes_found(
    notes: &obelus_component::todo::TodoView,
    needle: &obelus_search::Needle,
) -> Vec<(usize, std::ops::Range<usize>, String)> {
    let mut found = Vec::new();
    for (at, note) in notes.todo().notes.iter().enumerate() {
        let mut before = 0;
        for line in note.said.split('\n') {
            if let Some(range) = needle.found_in(line) {
                found.push((
                    at,
                    before + range.start..before + range.end,
                    line.to_string(),
                ));
            }
            before += line.chars().count() + 1;
        }
    }
    found
}
