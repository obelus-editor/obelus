//! What Obelus asks a language server, and what it does with the answers.
//!
//! One half of the semantic layer: [`obelus_lsp`] speaks the protocol, and
//! this decides when to speak it and what a reply means to the views. The
//! split is deliberate -- everything here needs the application's state, and
//! nothing there does.

use obelus_text::coordinates::{CharOffset, End, Replacement, Span};

use super::*;

/// What Obelus calls itself where a diagnostic says which tool said it.
///
/// Its own name, spelled the way the name is spelled everywhere -- and it
/// is what tells Obelus's own from a server's when one of them is taken
/// away again.
pub(super) const OBELUS: &str = "Obelus";

impl App {
    /// What a language server is busy with, if one is.
    ///
    /// Shown so that an empty answer during indexing can be told from an
    /// empty answer about a symbol that has no definition. They are the same
    /// message on the wire.
    #[must_use]
    pub fn server_working_on(&self) -> Option<&str> {
        self.servers.values().find_map(Client::working_on)
    }

    /// Whether the server behind the file being read is busy.
    ///
    /// The badge's question rather than [`Self::server_working_on`]'s: that
    /// one is any server, and this names the one the badge names.
    ///
    /// A bit, not the words. What a server says it is doing is its own
    /// running commentary -- `rust-analyzer` sends a few hundred of them
    /// over a cold start -- and a row that changed every few frames put
    /// that commentary between the file's name and the cursor's position,
    /// where a reader is trying to read two facts that do not move. What
    /// they need from it is that it is busy, which is one bit and a mark
    /// that turns.
    #[must_use]
    pub fn server_busy(&self) -> bool {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            return false;
        };
        self.servers
            .get(&language)
            .is_some_and(|client| client.working_on().is_some())
    }

    /// The server for the file being read, and what it is doing.
    ///
    /// Only the current file's: a status bar listing every server Obelus has
    /// started would be a table, and the question a reader has is whether
    /// *this* file's questions can be answered.
    #[must_use]
    pub fn server_state(&self) -> Option<(&'static str, obelus_lsp::ServerState)> {
        let language = self.current_buffer()?.language()?;
        let client = self.servers.get(&language)?;
        Some((obelus_lsp::command_for(language)?, client.state()))
    }

    /// Asks each server whether it is still running.
    ///
    /// Once a frame, from [`App::prepare`]: reaping needs `&mut`, and a dead
    /// server is otherwise silent -- the reader thread stops and the
    /// questions simply stop being answered.
    pub(super) fn check_servers(&mut self) {
        for client in self.servers.values_mut() {
            client.check_alive();
        }
    }

    /// Starts a server for a buffer's language, if there is one to start and
    /// it is not already running.
    ///
    /// Only for files under the root. A server is rooted at the working
    /// directory, and asking it about a file outside its own project gets
    /// answers about a project it cannot see.
    pub(super) fn serve(&mut self, index: usize) {
        let Some(buffer) = file_in(&self.documents, DocumentId::new(index)) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        if !buffer.path().starts_with(&self.working_directory) {
            tracing::debug!(path = %buffer.path().display(), "outside the root, so no server");
            return;
        }
        // A commit's version of a file is not what that path holds. Telling
        // a server otherwise makes every answer about it wrong -- the
        // definitions it finds would be at lines of a file nobody has.
        if !buffer.content().is_file() {
            tracing::debug!(path = %buffer.path().display(), "read from a commit, so no server");
            return;
        }
        if self.stopped.contains(&language) {
            tracing::debug!(
                language = language.name(),
                "stopped on purpose, so no server"
            );
            return;
        }

        if !self.servers.contains_key(&language) {
            let Some(server) = obelus_lsp::server_for(language) else {
                return;
            };
            let command = server.command;
            if !obelus_lsp::on_path(command) {
                tracing::info!(%command, "not on PATH, so no server for {}", language.name());
                return;
            }
            let Some(sender) = self.events.clone() else {
                return;
            };
            match Client::start(language, server, &self.working_directory, sender) {
                Ok(client) => {
                    tracing::info!(%command, "started");
                    self.servers.insert(language, client);
                }
                Err(error) => {
                    tracing::warn!(%error, %command, "could not start");
                    return;
                }
            }
        }

        self.open_document(index);
    }

    /// Tells the server about a document.
    fn open_document(&mut self, index: usize) {
        let Some(buffer) = self.file(DocumentId::new(index)) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        // Nothing about a commit's version of the file: the server is
        // being told what is at this path, and this is not it.
        if !buffer.content().is_file() {
            return;
        }
        let text = buffer.text().rope().to_string();
        let version = buffer.version();
        let name = language.name();

        if let Some(client) = self.servers.get_mut(&language) {
            let _ = client.notify(
                "textDocument/didOpen",
                &serde_json::json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": name,
                        "version": version,
                        "text": text,
                    }
                }),
            );
        }
        self.ask_standing_questions(index);
    }

    /// Tells a server the document has been written to disk.
    ///
    /// Some of them do work only then -- a linter that runs on save, a
    /// formatter's idea of the last good version -- and none of them can
    /// know from `didChange`, which says only that the text moved.
    pub(super) fn saved_document(&mut self, index: usize) {
        let Some(buffer) = self.file(DocumentId::new(index)) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        // Nothing about a commit's version, the same as its three siblings.
        if !buffer.content().is_file() {
            return;
        }
        if let Some(client) = self.servers.get_mut(&language) {
            let _ = client.notify(
                "textDocument/didSave",
                &serde_json::json!({ "textDocument": { "uri": uri } }),
            );
        }
        // The document has stopped moving, which is when a classification
        // of it is worth having: see [`Standing::Tokens`].
        self.ask_standing_questions(index);
    }

    /// Tells the server a document changed.
    pub(super) fn change_document(&mut self, index: usize) {
        let Some(buffer) = file_in(&self.documents, DocumentId::new(index)) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        // Nothing about a commit's version of the file: the server is
        // being told what is at this path, and this is not it.
        if !buffer.content().is_file() {
            return;
        }
        let version = buffer.version();
        // The whole document, which is what Obelus sends and means to: a
        // range needs the *old* document's coordinates in the encoding the
        // server agreed to, which is the shape every coordinate bug in this
        // program has had.
        //
        // Copied out only where there is somebody to send it to, though.
        // With no server running -- no language, none installed, one that
        // died -- this was a copy of the file per keystroke that nothing
        // ever read.
        // Whatever a server works out about it is about the file as it was
        // a keystroke ago. Noted rather than asked: the reader is still
        // typing, and a question per keystroke is a whole file's worth of
        // answer thrown away per keystroke.
        //
        // Only where there is somebody to ask. A file no server is reading
        // has no question waiting for it to stop moving, and one noted
        // anyway would wake the screen for a third of a second after every
        // keystroke in it -- which is most of what Obelus opens.
        if self.servers.contains_key(&language) {
            self.will_settle(DocumentId::new(index));
        }
        let Some(buffer) = file_in(&self.documents, DocumentId::new(index)) else {
            return;
        };
        let text = buffer.text().rope().to_string();
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        let _ = client.notify(
            "textDocument/didChange",
            &serde_json::json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
    }

    /// Offers what a language server can say about the symbol under the
    /// cursor.
    ///
    /// One key for every question rather than a key each: the questions all
    /// take the same argument and differ only in what comes back, and a menu
    /// can say which ones this server actually answers.
    pub fn open_symbol_menu(&mut self) {
        // Whatever the tree still owes, before it is asked what is under
        // the cursor: this is a question the reader acts on, and a tenth of
        // a second is nothing to pay for the right answer.
        self.settle_syntax();
        let actions = match self.symbol_actions() {
            Ok(actions) => actions,
            // No menu at all. A list with one row explaining itself is still
            // a list: it covers the code, it has to be dismissed, and it
            // offers nothing. The reason belongs on the status bar, which is
            // where every other passing word about state goes.
            Err(why) => {
                self.wrong(why);
                return;
            }
        };

        // Rows over commands, like the palette, so both offer the same things
        // and both show whatever key the table has for them.
        let items: Vec<PickerItem> = actions
            .into_iter()
            .map(|action| {
                let command = action.command();
                PickerItem {
                    prose: false,
                    marker: None,
                    icon: obelus_icons::enabled().then(|| obelus_icons::for_command(command)),
                    label: command.spec().name.to_string(),
                    detail: Some(command.spec().title.to_string()),
                    trailing: self.keymap.chord_for(command).map(KeyChord::label),
                    changed: None,
                    value: PickerValue::Command(command),
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: 0,
                    opens: None,
                    kind: None,
                    tab: None,
                    section: None,
                }
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.opened_by(Command::SymbolMenu);
        self.show_list(picker);
    }

    /// Which questions about the name under the caret this server will
    /// answer, or why none of them.
    ///
    /// [`SymbolAction`]s, which are questions: where it is defined, what
    /// uses it. Not the server's code actions, which are offers to change
    /// the file and live in [`crate::app`]'s other half -- the two were
    /// both called actions, and the command for the second was called
    /// `SymbolActions` while being about a range rather than a symbol.
    pub(super) fn symbol_actions(&self) -> Result<Vec<SymbolAction>, String> {
        let buffer = self
            .current_buffer()
            .ok_or_else(|| "No file open".to_string())?;
        let language = buffer
            .language()
            .ok_or_else(|| "Not a language Obelus knows".to_string())?;

        // On a name, before anything about servers. Every question in the
        // menu is about the thing under the cursor, and on a bracket or a
        // blank line there is no thing: the answer would be nothing, four
        // different ways. Asked first because it is the reason a reader can
        // act on -- move the cursor -- where the others are about the
        // machine.
        if !self.name_at(buffer) {
            return Err("No symbol here".to_string());
        }

        if let Some(why) = self.why_not_asking(language) {
            return Err(why);
        }
        let Some(capabilities) = self.servers.get(&language).and_then(Client::capabilities) else {
            return Err(format!("{} is still starting", named(language)));
        };
        let actions: Vec<SymbolAction> = action::ALL
            .iter()
            .copied()
            .filter(|action| action.supported(capabilities))
            .collect();
        if actions.is_empty() {
            return Err("The language server answers none of these".to_string());
        }
        Ok(actions)
    }

    /// Tells the server about the file being read, as opening one does.
    ///
    /// For a test that wants the cold start: a server started and a file
    /// announced to it before it has answered its handshake.
    pub fn serve_current_for_test(&mut self) {
        if let Some(id) = self.current {
            self.serve(id.get());
        }
    }

    /// How long a document has to stop moving before it is asked about.
    ///
    /// Long enough not to fire between two keystrokes of somebody typing,
    /// short enough that it has happened by the time they have looked at
    /// what they wrote. Not the reader's to set, unlike the pointer's
    /// dwell: how long a hover should wait is a matter of taste, and this
    /// is a guess at when a person stopped.
    pub const SETTLES_AFTER: std::time::Duration = std::time::Duration::from_millis(300);

    /// Says the document has just changed, so that it is asked about once
    /// it stops.
    pub(super) fn will_settle(&mut self, id: DocumentId) {
        self.settling = Some(Settling { buffer: id });
        // Started again by every change, so what it measures is the reader
        // stopping. It used to be a frame that asked whether they had, and
        // frames come from the animation: a reader over a network typed and
        // the colours never caught up, because nothing was animated to keep
        // the frames coming.
        self.changes_pause = self.come_back_in(Self::SETTLES_AFTER, Event::ChangesSettled);
    }

    /// Asks what a server works out about a document the reader has
    /// stopped changing.
    ///
    /// The one moment Obelus asks for these that does not depend on the
    /// server saying anything. A save asks, and so does a server finishing
    /// its own work -- but a server that reports no work of its own never
    /// finishes any, and a reader who has not saved has changed the file
    /// without anything asking about it since.
    pub(super) fn settle_changes(&mut self) {
        let Some(settling) = self.settling.take() else {
            return;
        };
        self.changes_pause = None;
        // Not the semantic tokens. They are a whole file's worth of answer
        // per ask, which is why they wait for a save -- and while the
        // reader types, the tree Obelus parses itself is what answers for
        // them.
        self.ask_standing(settling.buffer.get(), Standing::Colours);
        self.ask_standing(settling.buffer.get(), Standing::Hints);
    }

    /// Asks one of the standing questions about a document.
    ///
    /// Standing because nobody asks them: they are what a server can say
    /// about a whole file, and Obelus asks them wherever the file has
    /// stopped moving. One function for the three because they differ in
    /// four things and agree in everything else -- which document, whether
    /// it is a file at all, what it is called, which version, and what to
    /// do with the answer when it comes.
    pub(super) fn ask_standing(&mut self, index: usize, what: Standing) {
        let id = DocumentId::new(index);
        // The reader's answer first, where it is theirs to give: a question
        // asked with nowhere to put the answer is a question not worth a
        // server's time.
        if !what.wanted(self.config()) {
            return;
        }
        let Some(buffer) = self.file(id) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        // Nothing about a commit's version of the file: the server is being
        // asked about what is at this path, and that is not it.
        if !buffer.content().is_file() {
            return;
        }
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let version = buffer.version();
        // The document's own end, not one past it. A line count is one more
        // than the last line number wherever a file ends in a newline --
        // which is almost everywhere -- and a server handed a range that
        // ends past the file refuses the whole request: measured against
        // rust-analyzer, `Invalid offset LineCol { line: 271, col: 0 }
        // (line index length: 9349)`, for a file of two hundred and
        // seventy lines.
        let last = buffer.text().last_line();
        let end = buffer.text().line_length(last);
        let encoding = self.encoding_for(language);
        let Some(buffer) = self.file(id) else {
            return;
        };
        let stop = position::to_lsp(buffer.text(), last, end, &encoding);
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client
            .capabilities()
            .is_some_and(|can| what.answered_by(can))
        {
            return;
        }
        if let Ok(request) = client.request(what.method(), &what.params(&uri, stop)) {
            self.remember(
                language,
                request,
                Question {
                    asked: what.asked(),
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Asks all of them about a document.
    pub(super) fn ask_standing_questions(&mut self, index: usize) {
        for what in Standing::ALL {
            self.ask_standing(index, what);
        }
    }

    /// Asks the standing questions about every open file of a language.
    ///
    /// For the moment a server finishes its handshake, which is the moment
    /// it can first be asked anything: everything below is refused while a
    /// server cannot say what it answers, and the files were opened before
    /// that. Not `didOpen` again -- that one is queued by the client until
    /// the handshake finishes and has already gone.
    pub(super) fn ask_about_open_files(&mut self, language: LanguageId) {
        let indices: Vec<usize> = (0..self.documents.len())
            .filter(|index| {
                self.documents
                    .get(*index)
                    .and_then(Option::as_ref)
                    .and_then(Document::file)
                    .and_then(Buffer::language)
                    .is_some_and(|of| of == language)
            })
            .collect();
        for index in indices {
            self.ask_standing_questions(index);
        }
    }

    /// Takes the reader's answer about hints to the screen.
    ///
    /// Both ways round, because a switch that only works one way is a
    /// switch a reader has to restart to use: off takes away what is drawn
    /// and on asks for what was never asked for.
    pub(super) fn hints_switched(&mut self) {
        let documents: Vec<DocumentId> = (0..self.documents.len()).map(DocumentId::new).collect();
        if self.settled.config.inlay_hints {
            // Only where nobody has asked. This runs after every change to
            // every setting -- a theme, a tab width -- and a question per
            // open file per keystroke in the settings page is a question
            // nobody wanted.
            for id in documents {
                let asked = self
                    .file(id)
                    .is_some_and(|buffer| self.hints.contains_key(buffer.path()));
                if !asked {
                    self.ask_standing(id.get(), Standing::Hints);
                }
            }
            return;
        }
        self.hints.clear();
        for id in documents {
            self.redraw_cells(id);
        }
    }

    /// Keeps what a server would have the reader know, if the answer is
    /// still about this document.
    fn on_hints(&mut self, id: DocumentId, version: i32, reply: Reply) {
        let Some(buffer) = self.file(id) else {
            return;
        };
        // The document it was asked about, unchanged since: these are
        // places in a text, and a text that has moved has moved them.
        if buffer.version() != version {
            return;
        }
        let path = buffer.path().to_path_buf();
        let encoding = buffer
            .language()
            .map_or(lsp_types::PositionEncodingKind::UTF16, |language| {
                self.encoding_for(language)
            });
        let Some(buffer) = self.file(id) else {
            return;
        };
        let found = obelus_lsp::hint::in_reply(&reply.result, buffer.text(), &encoding);
        match found.is_empty() {
            true => self.hints.remove(&path),
            false => self.hints.insert(path, found),
        };
        self.redraw_cells(id);
    }

    /// Works out what is drawn in a document that the document does not
    /// contain, and tells the text how wide each of them is.
    ///
    /// One list from both answers, because a cell points at one entry and
    /// cannot say which of two lists it meant -- and one pass, because the
    /// text has to be told all of them at once: what it is told replaces
    /// whatever it was told before.
    ///
    /// Which is the one place either answer can be out of date. An edit
    /// carries the cells along with the text it is drawing them in, so
    /// what is on screen stays right; these are the places those cells
    /// were *made* from, and they are about the file as it was until the
    /// answer about the file as it is arrives. Between one answer landing
    /// and the other -- tens of milliseconds, both having been asked
    /// together -- a hint can be drawn an edit's width from where it
    /// belongs. The alternative is forgetting them, which takes every cell
    /// off the screen on every keystroke and puts them all back a fifth of
    /// a second later: measured, and far worse to read.
    /// Puts what is wrong with the line the caret is on under the reader's
    /// eye, and takes away the one that was there.
    ///
    /// Every frame, from the troubles and the caret rather than from
    /// anything remembered -- the same rule the row that says what is
    /// happening follows, so there is no way for a complaint to be left on
    /// a line that no longer has one.
    ///
    /// Only where the caret is standing. One of these covers the code
    /// under the line, and a file with thirty of them open at once is a
    /// file nobody can read. Every other one is said by the underline,
    /// which costs no room at all and is on all of them.
    pub(super) fn show_what_is_wrong(&mut self) {
        if !self.config().diagnostics {
            self.complaining = None;
            return;
        }
        // Whatever the reader is looking at, which is the caret's line
        // until a list is showing them somewhere else. Then it is the row
        // they have walked to: the file scrolls to it and the complaint
        // opens under it, so the row in the list and the place in the file
        // are obviously the same thing rather than two things a reader has
        // to pair up by line number.
        let chosen = self.the_selection_in_this_file();
        let Some(line) = self
            .current_buffer()
            .map(|buffer| chosen.map_or_else(|| buffer.cursor().line, |(line, _)| line))
        else {
            return;
        };
        let Some(path) = self
            .current_buffer()
            .map(|buffer| buffer.path().to_path_buf())
        else {
            return;
        };
        let here = self.troubles.get(&path).map_or(&[] as &[_], Vec::as_slice);
        // Where the caret is, because the box is about the thing it is
        // standing on. A list's row brings its own column and wins.
        let column = self
            .current_buffer()
            .map_or_else(|| CharColumn::new(0), |buffer| buffer.cursor().column);
        self.complaining = what_is_wrong(here, line, column, chosen.map(|(_, column)| column));
    }

    /// Where each thing a server said is wrong with this document starts
    /// and finishes, as offsets into its text as it is now.
    ///
    /// Taken before an edit, for [`App::keep_troubles_across`]: afterwards
    /// the text the spans were counted against is gone.
    pub(super) fn hold_troubles(&self, index: usize) -> Option<Vec<(CharOffset, CharOffset)>> {
        let buffer = self.file(DocumentId::new(index))?;
        let text = buffer.text();
        let held = self
            .troubles
            .get(buffer.path())?
            .iter()
            .map(|trouble| {
                let span = trouble.span;
                (
                    text.char_offset(span.line, span.column),
                    text.char_offset(span.end_line, span.end_column),
                )
            })
            .collect();
        Some(held)
    }

    /// Moves what a server said is wrong across an edit the reader made.
    ///
    /// Each one is about a piece of text, and until the server looks again
    /// the piece it named has moved: an underline left where it was is
    /// under whatever the edit pushed into its place. So it goes with the
    /// text -- the same rule a snippet's holes follow -- and one whose text
    /// the edit took away goes with that, because what it was about is not
    /// there. helix and zed both keep them this way, until the next set.
    pub(super) fn keep_troubles_across(
        &mut self,
        index: usize,
        held: Vec<(CharOffset, CharOffset)>,
        edit: Replacement,
    ) {
        let Self {
            documents,
            troubles,
            ..
        } = self;
        let Some(buffer) = file_in(documents, DocumentId::new(index)) else {
            return;
        };
        let Some(these) = troubles.get_mut(buffer.path()) else {
            return;
        };
        let text = buffer.text();
        let mut held = held.into_iter();
        these.retain_mut(|trouble| {
            let Some((start, finish)) = held.next() else {
                return true;
            };
            let (from, to) = (
                edit.carry(start, End::Start),
                edit.carry(finish, End::Finish),
            );
            // A run that had text and has none left was taken away. One
            // that never had any is a place, which a server may name.
            if to <= from && finish > start {
                return false;
            }
            let ((line, column), (end_line, end_column)) = (text.position(from), text.position(to));
            trouble.span = Span {
                line,
                column,
                end_line,
                end_column,
            };
            true
        });
        if these.is_empty() {
            let path = buffer.path().to_path_buf();
            troubles.remove(&path);
        }
    }

    pub(super) fn redraw_cells(&mut self, id: DocumentId) {
        let Some(buffer) = self.file(id) else {
            return;
        };
        let path = buffer.path().to_path_buf();
        let colours = self.colours.get(&path).map_or(&[] as &[_], Vec::as_slice);
        let hints = self.hints.get(&path).map_or(&[] as &[_], Vec::as_slice);

        // Both sources through one loop, numbered where they are put
        // together: two lists built side by side with an offset between
        // them is two chances to number them differently, and a cell
        // numbered wrong draws a hint as a colour.
        let mut cells = Vec::with_capacity(colours.len() + hints.len());
        let mut drawn = Vec::with_capacity(colours.len() + hints.len());
        let swatches = colours.iter().map(|coloured| {
            (
                coloured.span.line,
                coloured.span.column,
                obelus_ui::swatch_cells(),
                obelus_ui::Drawn::Swatch(coloured.colour),
            )
        });
        let worked_out = hints.iter().map(|hint| {
            (
                hint.line,
                hint.column,
                hint.cells(),
                obelus_ui::Drawn::Hint(hint.clone()),
            )
        });
        for (line, column, wide, what) in swatches.chain(worked_out) {
            cells.push(obelus_text::Phantom {
                line,
                column,
                cells: wide,
                which: drawn.len(),
            });
            drawn.push(what);
        }

        if let Some(buffer) = file_in_mut(&mut self.documents, id) {
            buffer.show(&cells);
        }
        match drawn.is_empty() {
            true => self.drawn.remove(&path),
            false => self.drawn.insert(path, drawn),
        };
    }

    /// What is drawn in the file being read that the file does not contain.
    #[must_use]
    pub fn drawn(&self) -> &[obelus_ui::Drawn] {
        self.current_buffer()
            .and_then(|buffer| self.drawn.get(buffer.path()))
            .map_or(&[], Vec::as_slice)
    }

    /// Keeps where the colours are, if the answer is still about this
    /// document.
    fn on_colours(&mut self, id: DocumentId, version: i32, reply: Reply) {
        let Some(buffer) = self.file(id) else {
            return;
        };
        // The document it was asked about, unchanged since: these are
        // places in a text, and a text that has moved has moved them.
        if buffer.version() != version {
            return;
        }
        let path = buffer.path().to_path_buf();
        let encoding = buffer
            .language()
            .map_or(lsp_types::PositionEncodingKind::UTF16, |language| {
                self.encoding_for(language)
            });
        let Some(buffer) = self.file(id) else {
            return;
        };
        let found = obelus_lsp::colour::in_reply(&reply.result, buffer.text(), &encoding);
        match found.is_empty() {
            true => self.colours.remove(&path),
            false => self.colours.insert(path, found),
        };
        self.redraw_cells(id);
    }

    /// Where the colours are in the file being read.
    #[must_use]
    pub fn colours(&self) -> &[obelus_lsp::colour::Coloured] {
        self.current_buffer()
            .and_then(|buffer| self.colours.get(buffer.path()))
            .map_or(&[], Vec::as_slice)
    }

    /// The same, about a version of the document that has been left
    /// behind -- which is what a late answer is.
    pub fn colours_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_colours(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands Obelus what a server would have the reader know.
    pub fn hints_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self
            .current_buffer()
            .map_or(0, obelus_buffer::Buffer::version);
        self.on_hints(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// The same, about a version of the document that has been left behind.
    pub fn hints_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_hints(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// How many of them are showing.
    #[must_use]
    pub fn hints_for_test_count(&self) -> usize {
        self.current_buffer()
            .and_then(|buffer| self.hints.get(buffer.path()))
            .map_or(0, Vec::len)
    }

    /// Hands Obelus an answer about the colours, as a server would.
    pub fn colours_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self
            .current_buffer()
            .map_or(0, obelus_buffer::Buffer::version);
        self.on_colours(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Keeps a classification of a document, if it is still about it.
    fn on_tokens(
        &mut self,
        id: DocumentId,
        version: i32,
        language: LanguageId,
        reply: obelus_lsp::client::Reply,
    ) {
        let Ok(result) = reply.result else {
            return;
        };
        let Some(data) = result.get("data").and_then(|data| data.as_array()) else {
            return;
        };
        let Some(client) = self.servers.get(&language) else {
            return;
        };
        let Some(legend) = client
            .capabilities()
            .and_then(|capabilities| capabilities.semantic_tokens_provider.as_ref())
            .map(|provider| match provider {
                lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
                    &options.legend
                }
                lsp_types::SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                    options,
                ) => &options.semantic_tokens_options.legend,
            })
        else {
            return;
        };
        let numbers: Vec<u32> = data
            .iter()
            .filter_map(serde_json::Value::as_u64)
            .map(|number| u32::try_from(number).unwrap_or(u32::MAX))
            .collect();
        let tokens = obelus_lsp::tokens::Tokens::decode(
            &numbers,
            legend,
            client.encoding().clone(),
            version,
        );
        let Some(path) = self.file(id).map(|buffer| buffer.path().to_path_buf()) else {
            return;
        };
        self.tokens.insert(path, tokens);
    }

    /// Whether the thing under the cursor is a name anybody could ask about.
    ///
    /// The server's answer where it has given one about this very version of
    /// the document, and the parse tree's where it has not. They disagree in
    /// one place that matters: a keyword is a leaf made of letters, so the
    /// tree can only say it is *shaped* like a name, while the server knows
    /// what it is.
    pub(super) fn name_at(&self, buffer: &Buffer) -> bool {
        let cursor = buffer.cursor();
        let from_server = self.tokens.get(buffer.path()).and_then(|tokens| {
            let at = obelus_lsp::position::to_lsp(
                buffer.text(),
                cursor.line,
                cursor.column,
                tokens.encoding(),
            );
            tokens.name_at(buffer.version(), at)
        });
        from_server.unwrap_or_else(|| {
            let at = buffer
                .text()
                .byte_of_char(buffer.text().char_offset(cursor.line, cursor.column));
            buffer
                .syntax()
                .is_some_and(|state| state.is_name_at(buffer.text(), at))
        })
    }

    /// Asks whichever question a command names.
    pub fn ask_about_symbol(&mut self, command: obelus_command::Command) {
        let Some(action) = SymbolAction::for_command(command) else {
            return;
        };
        self.ask(action);
    }

    /// Asks one of those questions.
    fn ask(&mut self, action: SymbolAction) {
        let Some(id) = self.current else { return };
        let Some(buffer) = file_in(&self.documents, id) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let cursor = buffer.cursor();
        let version = buffer.version();

        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        let at = position::to_lsp(buffer.text(), cursor.line, cursor.column, client.encoding());
        let mut params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
        });
        if action == SymbolAction::References {
            // Without this the definition itself is left out, which reads as
            // one reference missing rather than as a deliberate omission.
            params["context"] = serde_json::json!({ "includeDeclaration": true });
        }

        // A tree of calls starts with an item rather than with a place, and
        // the item is what every question after this one is asked about.
        let asked = match action == SymbolAction::Calls {
            true => Asked::Prepared,
            false => Asked::Symbol(action),
        };
        match client.request(action.method(), &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked,
                        buffer: id,
                        version,
                    },
                );
                self.say(format!("{}\u{2026}", action.title()));
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask");
                self.wrong("The language server is not listening".to_string());
            }
        }
    }

    /// Tells every running server that a file on disk changed.
    ///
    /// Every server rather than the one for that language: a change to a
    /// `Cargo.toml` is news to rust-analyzer, and a change to a `.proto`
    /// is news to whoever generates from it. Which of them cares is the
    /// server's to decide, and the protocol is built that way -- the
    /// client reports, the server filters.
    ///
    /// Only for a file in the project. A server is about this tree, and
    /// what Obelus watches outside it is its own -- the settings, which
    /// window holds what, which window asks for the chat -- none of which
    /// is a file any server has. Told about one anyway, rust-analyzer goes
    /// to look, and its mark turns on a window whose code nobody touched.
    /// Asked both ways, as the path was given and as it resolves, because
    /// a watcher may report either.
    pub(super) fn told_servers_about(&mut self, path: &Path) {
        let root = &self.working_directory;
        let ours = path.starts_with(root)
            || std::fs::canonicalize(root).is_ok_and(|root| path.starts_with(root));
        if !ours {
            return;
        }
        let Some(params) = obelus_lsp::watched_change(path) else {
            return;
        };
        for client in self.servers.values_mut() {
            let _ = client.notify("workspace/didChangeWatchedFiles", &params);
        }
    }

    /// Writes down a question that is out, and stops waiting for whatever
    /// it replaces.
    ///
    /// One door for all of them, because they all want the same thing: the
    /// *latest* answer to a kind of question is the only one anybody is
    /// going to look at. A reader typing a word asks for a completion per
    /// letter, moves the caret and asks what is under it per move; without
    /// this every superseded question is computed in full by the server
    /// and thrown away here, and the ones that are never answered stay in
    /// the table for the rest of the session.
    pub(super) fn remember(&mut self, language: LanguageId, request: i64, question: Question) {
        // The same kind, about the same document. Two questions of
        // different kinds are two things the reader wants; two of the same
        // kind are one thing asked twice.
        let kind = std::mem::discriminant(&question.asked);
        let buffer = question.buffer;
        let stale: Vec<i64> = self
            .asked
            .iter()
            .filter(|((asked, _), earlier)| {
                *asked == language
                    && earlier.buffer == buffer
                    && std::mem::discriminant(&earlier.asked) == kind
            })
            .map(|((_, id), _)| *id)
            .collect();
        for id in stale {
            self.asked.remove(&(language, id));
            if let Some(client) = self.servers.get_mut(&language) {
                client.cancel(id);
            }
        }
        self.asked.insert((language, request), question);
    }

    /// Tells a stand-in server's client what that server can do.
    ///
    /// Through the handshake reply, which is how a real one arrives: what
    /// the client ends up holding is then whatever the real path would
    /// have put there, and the queue it was holding goes out.
    pub fn declared_for_test(&mut self, language: LanguageId, capabilities: serde_json::Value) {
        // Through the door a server's messages come in by, rather than
        // straight at the client: finishing a handshake is a moment the
        // application acts on -- it is when the files already open can
        // first be asked about -- and a test that reached past it would be
        // testing a path Obelus does not have.
        self.handle(crate::event::Event::Lsp(obelus_lsp::Message {
            language,
            message: serde_json::json!({
                "id": 0, "result": { "capabilities": capabilities },
            }),
        }));
    }

    /// Why the server for a file cannot be asked anything, if it cannot.
    ///
    /// `None` when there is one and it has finished its handshake, which
    /// is the only state in which a question is worth sending. Every key
    /// that asks a server something reads this, so that four keys cannot
    /// give four different accounts of one missing program.
    pub(super) fn why_not_asking(&self, language: LanguageId) -> Option<String> {
        let Some(client) = self.servers.get(&language) else {
            return Some(match obelus_lsp::command_for(language) {
                Some(command) if !obelus_lsp::on_path(command) => {
                    format!("{command} is not installed")
                }
                Some(_) => format!("No server running for {}", language.name()),
                None => format!("No language server for {}", language.name()),
            });
        };
        // Running and not ready, which is the first second or two of every
        // session. Worth its own sentence because it is the one of these
        // that fixes itself: a reader told this presses the key again.
        client
            .capabilities()
            .is_none()
            .then(|| format!("{} is still starting", named(language)))
    }

    /// Whether a document is still the one a question was asked about.
    ///
    /// Every answer that names places in a text turns on this, and they
    /// all mean the same thing by it: the ranges a server sent are places
    /// in the file as it was, and a file that has changed has moved them.
    /// The damage a stale answer does is quiet -- an old line and column
    /// is usually still a real place in the new text, so the edit lands
    /// somewhere wrong rather than failing.
    #[must_use]
    pub(super) fn unmoved(&self, id: DocumentId, version: i32) -> bool {
        self.file(id).map(Buffer::version) == Some(version)
    }

    /// How many messages have gone to the servers.
    ///
    /// For a test about a notification that has no answer: nothing comes
    /// back from telling a server something, so the only way to see that
    /// it was told is to count what went out.
    #[must_use]
    pub fn told_servers_for_test(&self) -> usize {
        self.servers.values().map(Client::sent).sum()
    }

    /// Puts a server in front of the application, for a test that needs
    /// the whole way a message comes in rather than the end of it.
    ///
    /// The program is the test's, because what is being read is Obelus's
    /// half of the conversation: one that says nothing back is enough for
    /// that, and a real server cannot be made to ask an awkward question
    /// on demand. Nothing reads what Obelus says to it -- the wire is
    /// read elsewhere, where a server that echoes is the point.
    pub fn stand_in_server_for_test(
        &mut self,
        language: LanguageId,
        command: &'static str,
    ) -> bool {
        // The application's own channel where it has one, so that a test
        // holding the other end of it reads whatever Obelus writes -- a
        // server that echoes turns the wire into something a test can
        // assert about. A throwaway otherwise.
        let sender = self.events.clone().unwrap_or_else(|| {
            let (sender, receiver) = crate::event::channel();
            drop(receiver);
            sender
        });
        let server = obelus_lsp::Server {
            command,
            arguments: &[],
        };
        match Client::start(language, server, &self.working_directory, sender) {
            Ok(client) => {
                self.servers.insert(language, client);
                true
            }
            Err(_) => false,
        }
    }

    /// Hands Obelus a reply, by the id it was asked under.
    ///
    /// Through the same door the event loop uses, which is the point: what
    /// a reply *means* is decided by the question it was remembered as,
    /// and a test that calls the handler itself has chosen that for
    /// Obelus.
    pub fn answer_for_test(
        &mut self,
        language: LanguageId,
        id: i64,
        result: Result<serde_json::Value, String>,
    ) {
        self.on_reply(language, Reply { id, result });
    }

    /// How many questions are still out.
    ///
    /// For a test about the table not growing: a question that is
    /// superseded and never answered would otherwise sit in it for the
    /// rest of the session, and nothing else can see that happening.
    #[must_use]
    pub fn outstanding_for_test(&self) -> usize {
        self.asked.len()
    }

    /// Takes an answer, if it still means anything.
    ///
    /// What it means is worked out by [`action::outcome_of`], which is a
    /// function of the reply and two numbers: this is only what to do about
    /// each answer.
    pub(super) fn on_reply(&mut self, language: LanguageId, reply: Reply) {
        let Some(question) = self.asked.remove(&(language, reply.id)) else {
            tracing::debug!(id = reply.id, "an answer with nothing waiting for it");
            return;
        };

        let now = self.file(question.buffer).map(Buffer::version);
        let action = match question.asked {
            Asked::Symbol(action) => action,
            Asked::Outline => {
                self.on_outline(reply);
                return;
            }
            Asked::Workspace => {
                self.on_workspace_symbols(reply);
                return;
            }
            Asked::Formatting => {
                self.on_formatting(question.buffer, question.version, reply);
                return;
            }
            Asked::Colours => {
                self.on_colours(question.buffer, question.version, reply);
                return;
            }
            Asked::Hints => {
                self.on_hints(question.buffer, question.version, reply);
                return;
            }
            Asked::Tokens => {
                self.on_tokens(question.buffer, question.version, language, reply);
                return;
            }
            Asked::Completion { from } => {
                self.on_completion(question.buffer, from, reply);
                return;
            }
            Asked::Actions => {
                self.on_code_actions(question.buffer, question.version, reply);
                return;
            }
            Asked::Saving { kind } => {
                self.on_saving(question.buffer, question.version, kind, reply);
                return;
            }
            Asked::Saved { kind } => {
                self.on_saved(question.buffer, question.version, kind, reply);
                return;
            }
            Asked::Action { at } => {
                self.on_action(at, question.buffer, question.version, reply);
                return;
            }
            Asked::Rename => {
                self.on_rename(question.buffer, question.version, reply);
                return;
            }
            Asked::WillRename => {
                self.on_will_rename(language, reply);
                return;
            }
            Asked::Prepared => {
                self.on_prepared(question.buffer, language, reply);
                return;
            }
            Asked::Called { direction, id } => {
                self.on_called(direction, id, reply);
                return;
            }
            Asked::Behind { direction, id } => {
                self.on_behind(direction, id, reply);
                return;
            }
            Asked::Uses => {
                self.on_uses(question.buffer, question.version, reply);
                return;
            }
            Asked::Hover { at, pointed } => {
                self.on_hover(question.buffer, at, pointed, reply);
                return;
            }
            Asked::Signature { line } => {
                self.on_signature(question.buffer, line, reply);
                return;
            }
            Asked::Resolve { index } => {
                self.on_resolve(index, reply);
                return;
            }
        };
        let indexing = self.server_working_on().is_some();
        match action::outcome_of(reply.result, question.version, now, indexing) {
            Outcome::Stale => {
                tracing::debug!(
                    asked_against = question.version,
                    now = ?now,
                    "dropping an answer about a version that has been replaced"
                );
                self.wrong("The file changed while asking".to_string());
            }
            Outcome::Failed(message) => self.wrong(message),
            Outcome::NotYet => self.wrong("Still indexing".to_string()),
            Outcome::Nothing => {
                self.wrong(format!("Nothing for {}", action.title()));
            }
            Outcome::Places(mut places) if places.len() == 1 => {
                let place = places.remove(0);
                self.quiet();
                self.go_to(&place.path, place.line, place.character);
            }
            Outcome::Places(places) => {
                let items = place_rows(&places, &self.working_directory);
                self.quiet();
                let mut picker = Picker::new(items, PickerLayout::FullArea);
                picker.before_typing("Filter places");
                picker.previews();
                self.show_list(picker);
            }
        }
    }

    /// Stops the server for the current file and starts it again.
    ///
    /// The way out of a server that has died, or wedged, or was installed
    /// after Obelus started: those are the three states where every question
    /// gets the same silence, and none of them is worth restarting the whole
    /// program over. With none running it simply starts one, which is why it
    /// is offered whether or not there is one.
    pub fn restart_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.wrong("No file to restart a server for".to_string());
            return;
        };

        self.stop(language);
        // Deliberately stopped and now deliberately started: the restart is
        // the way back from a stop, so it lifts one.
        self.stopped.remove(&language);

        // Announce every open file of that language to the new server, not
        // just the current one: the others are still open, and a server that
        // has not been told about a file answers nothing about it.
        let indices: Vec<usize> = (0..self.documents.len())
            .filter(|index| {
                self.documents
                    .get(*index)
                    .and_then(Option::as_ref)
                    .and_then(Document::file)
                    .and_then(Buffer::language)
                    .is_some_and(|of| of == language)
            })
            .collect();
        for index in indices {
            self.serve(index);
        }

        match obelus_lsp::command_for(language) {
            Some(command) if self.servers.contains_key(&language) => {
                self.say(format!("Restarted {command}"));
            }
            Some(command) if !obelus_lsp::on_path(command) => {
                self.wrong(format!("{command} is not installed"));
            }
            Some(command) => self.wrong(format!("{command} would not start")),
            None => self.wrong(format!("No language server for {}", language.name())),
        }
    }

    /// Stops the server for the current file and leaves it stopped.
    ///
    /// For a server that is costing more than it is answering. It stays
    /// stopped until `restart-server`, because otherwise opening the next
    /// file of that language would start it again.
    pub fn stop_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.wrong("No file to stop a server for".to_string());
            return;
        };
        let was_running = self.stop(language);
        self.stopped.insert(language);
        match (obelus_lsp::command_for(language), was_running) {
            (Some(command), true) => self.say(format!("Stopped {command}")),
            (Some(command), false) => self.wrong(format!("{command} was not running")),
            (None, _) => self.wrong(format!("No language server for {}", language.name())),
        }
    }

    /// Everything the current file defines, to jump into.
    ///
    /// A full-area picker, so it gets the preview the file picker has: the
    /// list on top, the symbol in its own code below, marked. Filtering is
    /// the prompt, moving is the arrows, and choosing is a jump -- all of
    /// which the picker already does. What is new here is only where the
    /// rows come from.
    ///
    /// From the syntax tree. A language server knows more, and asking it is
    /// the next step; the tree is what makes the outline work on a file with
    /// no server, before indexing has finished, and outside the project
    /// root.
    pub fn open_outline(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.wrong("No file to outline".to_string());
            return;
        };
        let path = buffer.path().to_path_buf();
        let Some(language) = buffer.syntax().map(SyntaxState::language) else {
            self.wrong("Not a language Obelus knows".to_string());
            return;
        };

        // A server, if one is running for this language, gets asked. What it
        // knows is not what a tags query knows: the nesting is real, a
        // method is a method rather than a function that happens to sit
        // inside something, and a name it reports is a name the rest of the
        // semantic layer will agree about.
        //
        // The cost is that the answer arrives afterwards, so the list opens
        // saying it is waiting. That is the price of the better answer, and
        // it is a few milliseconds once the project is indexed.
        if self.servers.contains_key(&language) && self.ask_outline(&path, language) {
            let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
            picker.before_typing("Filter symbols");
            picker.when_empty("Asking the language server\u{2026}");
            picker.is_outline_of(path);
            picker.previews();
            self.show_list(picker);
            return;
        }

        self.outline_from_tree();
    }

    /// The outline the syntax tree gives.
    ///
    /// The floor: no server, or one that will not answer. Also what a
    /// server's empty answer falls back to, which is why it is its own
    /// method rather than the tail of the one above.
    fn outline_from_tree(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let path = buffer.path().to_path_buf();
        let Some(state) = buffer.syntax() else {
            return;
        };
        let language = state.language();
        let symbols = tags::outline(state, buffer.text());
        let text = buffer.text();
        let encoding = self.encoding_for(language);

        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| {
                let at = position::to_lsp(text, symbol.line, symbol.column, &encoding);
                let end = position::to_lsp(text, symbol.line, symbol.end_column, &encoding);
                let (line, character) = (at.line, at.character);
                let end_character = end.character;
                PickerItem {
                    prose: false,
                    marker: None,
                    icon: obelus_icons::enabled().then(|| obelus_icons::for_kind(symbol.kind)),
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                    opens: None,
                    kind: Some(symbol.kind),
                    label: symbol.name.clone(),
                    detail: None,
                    trailing: Some(format!("{}", symbol.line.get() + 1)),
                    changed: None,
                    value: PickerValue::Place {
                        path: path.clone(),
                        line,
                        character,
                        end_line: line,
                        end_character,
                    },
                    tab: None,
                    section: None,
                }
            })
            .collect();

        let mut picker = Picker::new(items, PickerLayout::FullArea);
        picker.before_typing("Filter symbols");
        picker.when_empty(if tags::has_tags(language) {
            "This file defines nothing"
        } else {
            // Not the same fact, and the difference is the reader's next
            // move: one means look elsewhere, the other means do not bother
            // pressing this key for this language.
            "No outline for this language"
        });
        // On the symbol the cursor is in, or the nearest one above it, which
        // is the answer to "where am I" that an outline is usually opened to
        // ask.
        if let Some(here) = nearest_symbol(&symbols, self.current_buffer().map(Buffer::cursor)) {
            picker.prefer(here);
        }
        picker.is_outline_of(path);
        picker.previews();
        self.show_list(picker);
    }

    /// Asks a server what a file defines, and says whether the question got
    /// out.
    fn ask_outline(&mut self, path: &Path, language: LanguageId) -> bool {
        let Ok(uri) = obelus_lsp::client::uri_for(path) else {
            return false;
        };
        let Some(id) = self.current else { return false };
        let Some(version) = self.file(id).map(Buffer::version) else {
            return false;
        };
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        match client.request("textDocument/documentSymbol", &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Outline,
                        buffer: id,
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask for an outline");
                false
            }
        }
    }

    /// Puts a server's answer into the list that is waiting for it.
    ///
    /// Nothing to put it in means the reader has closed the list or opened a
    /// different one, and an answer nobody is looking at is dropped. An
    /// empty answer falls back to the syntax tree: a server that will not
    /// answer this question yet should not cost the reader the outline.
    fn on_outline(&mut self, reply: Reply) {
        let Some(path) = self
            .picker
            .as_ref()
            .and_then(Picker::outline_of)
            .map(Path::to_path_buf)
        else {
            tracing::debug!("an outline with nothing waiting for it");
            return;
        };

        let symbols = obelus_lsp::outline::symbols_in(reply.result);
        if symbols.is_empty() {
            tracing::debug!("the server has no outline, so the tree's it is");
            self.outline_from_tree();
            return;
        }

        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| PickerItem {
                prose: false,
                marker: None,
                icon: obelus_icons::enabled().then(|| obelus_icons::for_kind(symbol.kind)),
                enabled: true,
                colours: None,
                status: None,
                depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                opens: None,
                kind: Some(symbol.kind),
                label: symbol.name.clone(),
                detail: None,
                trailing: Some(format!("{}", symbol.line.saturating_add(1))),
                changed: None,
                value: PickerValue::Place {
                    path: path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
                section: None,
            })
            .collect();

        let here = self
            .current_buffer()
            .map(Buffer::cursor)
            .and_then(|cursor| {
                symbols
                    .iter()
                    .rfind(|symbol| symbol.line as usize <= cursor.line.get())
            })
            .map(|symbol| symbol.name.clone());
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.when_empty("This file defines nothing");
            if let Some(here) = here {
                picker.prefer(here);
            }
        }
    }

    /// Which units a server counts positions in, or UTF-16 if none will say.
    pub(super) fn encoding_for(&self, language: LanguageId) -> lsp_types::PositionEncodingKind {
        self.servers
            .get(&language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            })
    }

    /// Asks how the file should be laid out, so it can be written that way.
    ///
    /// Says whether anybody was asked. A save that nobody could format goes
    /// ahead unformatted rather than waiting for an answer that is not
    /// coming: the reader pressed save, and a setting they turned on is not
    /// a reason to refuse them.
    pub(super) fn ask_formatting(&mut self, index: usize) -> bool {
        let Some(buffer) = self.file(DocumentId::new(index)) else {
            return false;
        };
        let (Some(language), true) = (buffer.language(), buffer.content().is_file()) else {
            return false;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return false;
        };
        let version = buffer.version();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "options": {
                "tabSize": self.settled.config.tab_width,
                "insertSpaces": true,
            },
        });
        match client.request("textDocument/formatting", &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Formatting,
                        buffer: DocumentId::new(index),
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "not asking how to lay the file out");
                false
            }
        }
    }

    /// Lays the file out the way the server said, and then writes it.
    ///
    /// The edits come back in the coordinates of the document that was
    /// asked about, so one that has moved since cannot take them -- and
    /// applying them in order would be applying each one to a document the
    /// last one changed, so they go in from the bottom up.
    fn on_formatting(&mut self, id: DocumentId, version: i32, reply: Reply) {
        if !self.unmoved(id, version) {
            tracing::debug!("the file changed while it was being laid out");
            self.wrong("The file changed while formatting".to_string());
        } else if let Some(edits) = action::edits_in(reply.result.ok()) {
            let encoding = self.file(id).and_then(Buffer::language).map_or_else(
                || lsp_types::PositionEncodingKind::UTF16,
                |language| self.encoding_for(language),
            );
            for edit in edits.into_iter().rev() {
                if let Some(buffer) = self.file_mut(id) {
                    let text = buffer.text();
                    let (line, column) = position::from_lsp(text, edit.range.start, &encoding);
                    let (end_line, end_column) =
                        position::from_lsp(text, edit.range.end, &encoding);
                    let span = Span {
                        line,
                        column,
                        end_line,
                        end_column,
                    };
                    buffer.edit(span, &edit.new_text, obelus_buffer::undo::Doing::Whole);
                }
            }
            self.change_document(id.get());
        }
        self.write_now(id.get());
    }

    /// Sends `workspace/symbol`, and says whether the question got out.
    pub(super) fn ask_workspace_symbols(&mut self, language: LanguageId, query: &str) -> bool {
        let Some(id) = self.current else { return false };
        let version = self.file(id).map(Buffer::version).unwrap_or_default();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "query": query });
        match client.request("workspace/symbol", &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Workspace,
                        buffer: id,
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask for the project's symbols");
                false
            }
        }
    }

    /// Puts the server's list of names into the search.
    ///
    /// Dropped if the reader has moved off the tab or closed the list: an
    /// answer nobody is looking at is not worth a redraw, and putting rows
    /// from one scope into another is worse than dropping them.
    fn on_workspace_symbols(&mut self, reply: Reply) {
        let root = self.working_directory.clone();
        // This project unless the reader has asked to see past it: a list of
        // every name a server knows is mostly the registry's, and the one
        // they meant is somewhere among them.
        let within = (!self.outside).then_some(root.as_path());
        let symbols = obelus_lsp::outline::found_in(reply.result, within);
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if self.searching.get(picker.tab()) != Some(&Scope::Symbols) {
            tracing::debug!("symbols with nothing waiting for them");
            return;
        }
        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| PickerItem {
                prose: false,
                marker: None,
                icon: obelus_icons::enabled().then(|| obelus_icons::for_kind(symbol.kind)),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: Some(symbol.kind),
                label: symbol.name.clone(),
                detail: None,
                // Relative to the working directory, the way the file
                // list shows paths: a server answers with absolute paths,
                // and a row that spends thirty columns on a prefix every
                // row shares is thirty columns not spent on the name.
                trailing: Some(format!(
                    "{}:{}",
                    symbol
                        .path
                        .strip_prefix(&root)
                        .unwrap_or(&symbol.path)
                        .display(),
                    symbol.line.saturating_add(1)
                )),
                changed: None,
                value: PickerValue::Place {
                    path: symbol.path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
                section: None,
            })
            .collect();
        picker.replace(items);
        picker.while_empty("The server knows no such name");
    }
}

impl App {
    /// Keeps what a server says is wrong with a file.
    ///
    /// Twice over, and neither copy is the other's cache. What arrived is
    /// kept as it arrived, for every file a server talks about -- and a
    /// server talks about files Obelus does not have open: rust-analyzer
    /// says what a `cargo check` found, which is the project rather than
    /// the buffer. That copy is the only one there can be for those files,
    /// because a range becomes a place by being counted against the text
    /// it is in and Obelus has not read that text.
    ///
    /// Then, for a file that *is* open, the same news placed. Everything
    /// that has to line up with a character on screen reads that one: the
    /// underline, the count on the status row, the complaint under the
    /// caret's line.
    pub(super) fn on_published(&mut self, language: LanguageId, params: &serde_json::Value) {
        let Some(path) = obelus_lsp::trouble::path_of(params) else {
            return;
        };
        // An empty set is a server saying the file is clean, which is news
        // worth keeping: it is how what was wrong stops being shown.
        //
        // In the order they are in the file, which is not the order they
        // arrive in. rustc reports what it found in the order it found it,
        // and rust-analyzer forwards that, so a file whose errors come out
        // at lines 73, 98, 25 is an ordinary file rather than a strange
        // one. Everything downstream reads these as places in a file --
        // the lists walk them top to bottom, and opening one on the row
        // nearest the caret means something only if the rows are in order.
        //
        // Sorted here rather than where a list is built, so that there is
        // one order and every reader of it gets the same one. Both
        // readings of the notification, for the same reason: two orders
        // would be two answers to "what is the third thing wrong with this
        // file".
        let mut reported = obelus_lsp::trouble::reported(params);
        reported.sort_by_key(|trouble| (trouble.line, trouble.character));
        match reported.is_empty() {
            true => self.reported.remove(&path),
            false => self.reported.insert(path.clone(), reported),
        };
        let Some(buffer) = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .find(|buffer| buffer.path() == path)
        else {
            return;
        };
        let encoding = self
            .servers
            .get(&language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let mut troubles = obelus_lsp::trouble::published(params, buffer.text(), &encoding);
        // What Obelus said about this file survives. A server's set is
        // replaced whole when it publishes another -- that is the
        // protocol's rule about a server's own -- and it says nothing
        // about anybody else's, so a server that happens to know about
        // TOML would otherwise take Obelus's marks off the settings by
        // having an opinion about the same file. `nothing_wrong_with`
        // keeps a server's for the same reason, the other way round.
        troubles.extend(
            self.troubles
                .get(&path)
                .into_iter()
                .flatten()
                .filter(|trouble| trouble.source.as_deref() == Some(OBELUS))
                .cloned(),
        );
        troubles.sort_by_key(|trouble| (trouble.span.line, trouble.span.column));
        match troubles.is_empty() {
            true => self.troubles.remove(&path),
            false => self.troubles.insert(path, troubles),
        };
    }

    /// Says something of Obelus's own about a file, for a test.
    ///
    /// The real ones are made where a file Obelus reads for its own sake
    /// will not read, which is not something a test of what happens to
    /// them afterwards should have to arrange.
    pub fn obelus_says_for_test(
        &mut self,
        path: &Path,
        span: obelus_text::coordinates::Span,
        severity: obelus_lsp::trouble::Severity,
        said: &str,
    ) {
        self.obelus_says(path, Some(span), severity, said);
    }

    /// How many marks Obelus has made against a file, and how many a
    /// server has, for a test about which of them outlive something.
    #[must_use]
    pub fn marks_on_for_test(&self, path: &Path) -> (usize, usize) {
        (
            self.troubles.get(path).map_or(0, |troubles| {
                troubles
                    .iter()
                    .filter(|trouble| trouble.source.as_deref() == Some(OBELUS))
                    .count()
            }),
            self.reported.get(path).map_or(0, Vec::len),
        )
    }

    /// Hands the application what a server would have published.
    ///
    /// The whole path a real notification takes -- the uri, the ranges,
    /// the severities -- because those are the parts worth testing, and a
    /// server cannot be made to find a mistake on demand.
    pub fn publish_for_test(&mut self, params: serde_json::Value) {
        let language = self
            .current_buffer()
            .and_then(Buffer::language)
            .unwrap_or(LanguageId::Rust);
        self.on_published(language, &params);
    }

    /// The line of the nearest problem one way or the other.
    ///
    /// By line number rather than by the order the server sent them: they
    /// arrive in file order from every server Obelus talks to, and a walk
    /// that trusts that would step backwards the day one does not. Several
    /// on a line are one stop, because the complaint under the caret says
    /// how many are there.
    #[must_use]
    pub fn trouble_from(&self, line: LineNumber, forward: bool) -> Option<LineNumber> {
        let at = self.problems().map(|trouble| trouble.span.line);
        if forward {
            at.filter(|at| *at > line).min()
        } else {
            at.filter(|at| *at < line).max()
        }
    }

    /// Moves the cursor to the problem above it.
    pub fn go_to_previous_trouble(&mut self) {
        self.go_to_trouble(false);
    }

    /// Moves the cursor to the problem below it.
    pub fn go_to_next_trouble(&mut self) {
        self.go_to_trouble(true);
    }

    /// Moves the cursor to the nearest problem one way or the other.
    ///
    /// No wrapping, and nothing said on the way out: the same walk as
    /// [`App::go_to_previous_change`], for the same reasons, and a reader
    /// who cannot tell the two apart is right not to be able to.
    fn go_to_trouble(&mut self, forward: bool) {
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            return;
        };
        let Some(target) = self.trouble_from(line, forward) else {
            return;
        };
        let from = self.here();
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.place_cursor(target, CharColumn::new(0));
            if buffer.cursor_screen_cell(area).is_none() {
                buffer.center_on_cursor(area);
            }
        }
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Lists what the servers say is wrong, at one radius or the other.
    ///
    /// Two tabs rather than two views, the way the search is one question
    /// at three radii: "what is wrong here" and "what is wrong in this
    /// project" are the same question about a wider circle, and a reader
    /// who finds this file clean is one row of tabs away from finding out
    /// the project is not.
    pub fn open_troubles(&mut self) {
        let here = self.wrong_here();
        // Nothing anywhere is not a list. What it is instead is whatever
        // makes it true: a server that has looked and found nothing, one
        // that is not answering, or none at all -- which is the difference
        // between "this is fine" and "nobody has said".
        if !self.anything_wrong() {
            // A server that looked and found nothing is good news, and
            // the other two are the key not having been answered at all --
            // which is the difference this match was already about.
            match self.server_state() {
                Some((_, obelus_lsp::ServerState::Ready)) => {
                    self.say("Nothing wrong with this file".to_string());
                }
                Some((command, _)) => self.wrong(format!("{command} is not answering")),
                None => self.wrong("No language server for this file".to_string()),
            }
            return;
        }
        let radii = self.wrongs();
        // Opened on the radius that has something to say. A reader asks
        // this about where they are, so the file comes first when the file
        // has anything -- and when it has nothing, a tab saying so is a
        // list that has to be walked before it answers anything at all.
        let opening = match here {
            0 => Wrong::Project,
            _ => Wrong::File,
        };
        let Some(tab) = radii.iter().position(|radius| *radius == opening) else {
            return;
        };
        let names: Vec<&str> = radii.iter().map(|radius| radius.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::Compact { rows: 10 });
        picker.before_typing("Filter problems");
        picker.with_scopes(&names);
        picker.keeps_order(true);
        // A row in another file is shown in the room above the rows, which
        // is where the file being read is drawn: the project's radius names
        // files nobody has opened, and a list that could only say
        // `other.rs:12` about them would be a list a reader has to leave to
        // read. The file's own radius never asks -- its rows are all in the
        // file already there -- so nothing moves when they walk the tabs.
        picker.previews();
        picker.go_to_tab(tab);
        picker.opened_by(Command::SymbolTroubles);
        self.show_list(picker);
        // After the list is shown, not before: showing one forgets what the
        // last one was, this included.
        self.troubling = radii;
        self.refresh_troubles();
    }

    /// Which radii can answer, in the order their tabs sit in.
    ///
    /// Settled when the list opens rather than watched while it is open,
    /// for the reason the search settles its own: a tab that came and went
    /// under the reader's hand would move the ground while they walk it.
    fn wrongs(&self) -> Vec<Wrong> {
        [Wrong::File, Wrong::Project]
            .into_iter()
            .filter(|radius| match radius {
                Wrong::File => self.current_buffer().is_some(),
                // Always: Obelus is started in a directory, and a project
                // nobody has said anything about is an empty list saying
                // so rather than a missing tab.
                Wrong::Project => true,
            })
            .collect()
    }

    /// Whether the list showing is the list of problems.
    pub(super) fn showing_troubles(&self) -> bool {
        self.picker.is_some() && !self.troubling.is_empty()
    }

    /// Fills the open list of problems with the radius it is showing.
    ///
    /// Called when it opens and whenever the reader walks onto the other
    /// tab: the two radii are two sets of rows over two sets of files, and
    /// the list itself knows nothing about where rows come from.
    pub(super) fn refresh_troubles(&mut self) {
        let Some(radius) = self
            .picker
            .as_ref()
            .and_then(|picker| self.troubling.get(picker.tab()).copied())
        else {
            return;
        };
        let paths = match radius {
            Wrong::File => self.reading().map(Path::to_path_buf).into_iter().collect(),
            Wrong::Project => self.troubled(),
        };
        let named: Vec<String> = paths
            .iter()
            .map(|path| crate::app::relative(path, &self.working_directory))
            .collect();
        let items: Vec<PickerItem> = paths
            .iter()
            .zip(&named)
            .flat_map(|(path, name)| {
                self.wrong_with(path).map(move |trouble| {
                    // A row says which file it is in only where that is
                    // news. In the file's own radius every row is in the
                    // file the heading names, and a path on each of them
                    // would be the same path ten times over, in the room
                    // the message needs.
                    let line = trouble.line + 1;
                    let trailing = match radius {
                        Wrong::File => format!("{line}"),
                        Wrong::Project => format!("{name}:{line}"),
                    };
                    trouble_row(trouble, path, trailing)
                })
            })
            .collect();
        let severities: Vec<obelus_lsp::trouble::Severity> = paths
            .iter()
            .flat_map(|path| self.wrong_with(path).map(|trouble| trouble.severity))
            .collect();
        let about = match (severities.is_empty(), radius) {
            // Nothing to count is nothing to say: the list has its own
            // words for being empty, and a heading over them would be the
            // same news twice.
            (true, _) => String::new(),
            (false, Wrong::File) => format!(
                "{} in {}",
                counted(&severities),
                named.first().map_or("", String::as_str)
            ),
            (false, Wrong::Project) => match paths.len() {
                1 => format!("{} in 1 file", counted(&severities)),
                files => format!("{} in {files} files", counted(&severities)),
            },
        };
        let at = self.trouble_to_open_on(radius, &items);
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.replace(items);
        picker.while_empty(match radius {
            Wrong::File => "Nothing wrong with this file",
            // Not "nothing is wrong with this project": no server was
            // asked, and none of them has to say anything about a file
            // nobody has opened. What is true is that nothing has been
            // said.
            Wrong::Project => "No server has said anything about this project",
        });
        picker.about(&about);
        if let Some(at) = at {
            picker.select_item(at);
        }
    }

    /// Which row the list opens on, which is the one nearest where the
    /// reader is standing.
    ///
    /// In the file, that is the row nearest the caret rather than the top
    /// of the file: a list that always starts at line one makes them walk
    /// back to somewhere they were already standing -- while the file
    /// behind it scrolls away from them, because the list shows its
    /// selection in the file. Nearest either way, not the next one down:
    /// the one they are on is the one they meant, and it is the caret's own
    /// line that is nearest it.
    ///
    /// In the project, it is the first row in the file they are reading.
    /// Same rule, as much of it as there is to have: a list of everywhere
    /// that opened in a stranger's file would answer from somewhere the
    /// reader is not.
    fn trouble_to_open_on(&self, radius: Wrong, items: &[PickerItem]) -> Option<usize> {
        let reading = self.reading()?;
        match radius {
            Wrong::File => {
                let line = self.current_buffer()?.cursor().line.get();
                let near = |trouble: &obelus_lsp::trouble::Reported| {
                    usize::try_from(trouble.line)
                        .unwrap_or(usize::MAX)
                        .abs_diff(line)
                };
                self.wrong_with(reading)
                    .enumerate()
                    .min_by_key(|(_, trouble)| near(trouble))
                    .map(|(at, _)| at)
            }
            Wrong::Project => items.iter().position(
                |item| matches!(&item.value, PickerValue::Place { path, .. } if path == reading),
            ),
        }
    }

    /// The path of the file being read, where one is.
    fn reading(&self) -> Option<&Path> {
        self.current_buffer().map(Buffer::path)
    }

    /// What has been said about one file that is a problem in its own
    /// right, in the order it is in the file.
    ///
    /// Read from what arrived rather than from what was placed, for every
    /// radius alike: most of a project is files Obelus has not opened, and
    /// the ones it has say the same thing either way. The notes a server
    /// hangs on other diagnostics are left out for the reason they are
    /// left out everywhere a reader works through what is wrong -- see
    /// [`App::problems`].
    fn wrong_with(&self, path: &Path) -> impl Iterator<Item = &obelus_lsp::trouble::Reported> {
        self.reported
            .get(path)
            .into_iter()
            .flatten()
            .filter(|trouble| trouble.severity != obelus_lsp::trouble::Severity::Hint)
    }

    /// Writes down something Obelus itself has to say about one of the
    /// files it reads for its own sake.
    ///
    /// Into the same list a server's go in, because from here down they
    /// are the same thing: a mark against a piece of a file, with a word
    /// about it and the name of whatever noticed. The underline, the count
    /// on the status row, the keys that walk them and the list they are
    /// in all read that one list and none of them has to be told which
    /// kind it is holding.
    ///
    /// Placed already, unlike a server's. A server names a place in a file
    /// it may be the only one holding, so what it sends is converted
    /// against the text later; Obelus has the text in hand at the moment
    /// it finds the fault.
    ///
    /// Nothing without a place. A diagnostic is a mark against a line, and
    /// a failure with no line -- a file whose permissions forbid it, a
    /// watcher that would not start -- has nothing to mark. Those are said
    /// where what went wrong on the way up is said, which is not here.
    pub(super) fn obelus_says(
        &mut self,
        path: &Path,
        at: Option<obelus_text::coordinates::Span>,
        severity: obelus_lsp::trouble::Severity,
        said: &str,
    ) {
        let Some(span) = at else {
            return;
        };
        let said = obelus_lsp::trouble::Trouble {
            span,
            severity,
            message: said.to_string(),
            // Obelus is the tool that said it, which is what a row of a
            // list of problems reads out -- and what tells this one from a
            // server's when the file is taken away again. Its own name,
            // spelled the way the name is spelled.
            source: Some(OBELUS.to_string()),
            // A code action is asked for *about* a diagnostic, and the
            // server matches it by every field it sent. There is no server
            // behind this one and nothing to ask.
            item: serde_json::Value::Null,
        };
        // In the order they are in the file, which is what a server's
        // arrive sorted into and what the keys that walk them and the list
        // they are in both read. Obelus finds its own in whatever order it
        // happens to ask -- the key table is alphabetical, because that is
        // what a table of it is.
        let troubles = self.troubles.entry(path.to_path_buf()).or_default();
        let at = troubles.partition_point(|other| {
            (other.span.line, other.span.column) <= (span.line, span.column)
        });
        troubles.insert(at, said);
    }

    /// Forgets what Obelus said about a file, leaving what a server said.
    ///
    /// Called where the file is read again, because what was said is about
    /// a version of it that no longer exists. Only Obelus's own: a server
    /// keeps its own set until it publishes another, which is the
    /// protocol's rule and not Obelus's to apply on its behalf.
    pub(super) fn nothing_wrong_with(&mut self, path: &Path) {
        let Some(troubles) = self.troubles.get_mut(path) else {
            return;
        };
        troubles.retain(|trouble| trouble.source.as_deref() != Some(OBELUS));
        if troubles.is_empty() {
            self.troubles.remove(path);
        }
    }

    /// Every file something is wrong with, in the order a project is read
    /// in.
    /// How many problems the file being read has.
    fn wrong_here(&self) -> usize {
        self.reading()
            .map_or(0, |path| self.wrong_with(path).count())
    }

    /// Whether the problems would be a list, anywhere: the one question
    /// `open_troubles` answers with a sentence instead, and asked before a
    /// view is put away for it (`app/switching`).
    pub(super) fn anything_wrong(&self) -> bool {
        self.wrong_here() > 0 || !self.troubled().is_empty()
    }

    fn troubled(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self
            .reported
            .keys()
            .filter(|path| self.wrong_with(path).next().is_some())
            .cloned()
            .collect();
        paths.sort();
        paths
    }
}

/// How wide a question about what is wrong is being asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wrong {
    /// The file being read.
    File,
    /// Every file anything has been said about.
    Project,
}

impl Wrong {
    /// The tab's name.
    const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Project => "Project",
        }
    }
}

/// One row of a list of problems, wherever the problem is.
///
/// The place is the one the server sent, in the units it sent it in, which
/// is what a row that names a place carries everywhere in Obelus: one kind
/// of value for "go here", and the conversion in one place.
fn trouble_row(
    trouble: &obelus_lsp::trouble::Reported,
    path: &Path,
    trailing: String,
) -> PickerItem {
    PickerItem {
        detail: trouble.source.clone(),
        ..problem_row(
            trouble.summary(),
            trouble.severity,
            Some(trailing),
            PickerValue::Place {
                path: path.to_path_buf(),
                line: trouble.line,
                character: trouble.character,
                end_line: trouble.end_line,
                end_character: trouble.end_character,
            },
        )
    }
}

/// One problem as a row of a list: what is wrong, how bad, and where.
///
/// One piece for every list of problems, so that an error is drawn as an
/// error wherever a list says one -- the mark in front where there is a
/// font for it, and the words in the colour the severity is: the colour an
/// error or a warning is in the file itself.
pub(super) fn problem_row(
    said: &str,
    severity: obelus_lsp::trouble::Severity,
    trailing: Option<String>,
    value: PickerValue,
) -> PickerItem {
    PickerItem {
        icon: obelus_icons::enabled().then(|| obelus_icons::for_problem(severity.kind())),
        label: said.to_string(),
        detail: None,
        prose: true,
        marker: None,
        trailing,
        changed: None,
        value,
        depth: 0,
        opens: None,
        status: None,
        enabled: true,
        colours: None,
        kind: Some(severity.kind()),
        tab: None,
        section: None,
    }
}

/// What is wrong where the reader is standing, for the box that says so.
///
/// One answer, wherever the box is drawn: under the caret in the file
/// being read, and under the line a preview is showing of somewhere else.
/// A preview whose box said something other than the editor's would be a
/// promise Obelus does not keep.
///
/// On the *span*, not on the line. A line is where the underline is and
/// the underline is under the word: a box that opened for the whole line
/// covered the code below it every time the caret passed through, and what
/// it said was about a word the reader might be nowhere near. Standing on
/// the thing is asking about the thing.
///
/// `chosen` is the column a list of problems walked the reader to, and it
/// wins: a reader who picked a row is looking at that one, and the caret
/// lands at the start of the span rather than inside it.
///
/// Nothing about the wrapping or the frame, which were worked out here
/// while the words went into the file as a block -- a block is rows, so
/// the width had to be known before there was anywhere to put them. A box
/// floated over the page is laid out to the room it is given, and the room
/// is the view's to know.
pub(super) fn what_is_wrong(
    troubles: &[obelus_lsp::trouble::Trouble],
    line: LineNumber,
    column: CharColumn,
    chosen: Option<CharColumn>,
) -> Option<Complaint> {
    let mut here: Vec<&obelus_lsp::trouble::Trouble> = troubles
        .iter()
        .filter(|trouble| trouble.span.line == line)
        .collect();
    here.sort_by_key(|trouble| trouble.severity);
    let worst = match chosen {
        Some(chosen) => here.iter().find(|trouble| trouble.span.column == chosen),
        // The worst of the ones the caret is actually inside, where it is
        // inside more than one: two spans over one word is two things
        // wrong with it, and the reader is owed the worse.
        None => here
            .iter()
            .find(|trouble| trouble.span.contains(line, column)),
    }?;
    Some(Complaint {
        line,
        column: worst.span.column,
        said: worst.message.clone(),
        severity: worst.severity,
        others: here.len() - 1,
    })
}

/// How many of each severity, as a phrase.
fn counted(severities: &[obelus_lsp::trouble::Severity]) -> String {
    use obelus_lsp::trouble::Severity;

    let count = |severity: Severity| severities.iter().filter(|had| **had == severity).count();
    let mut said = Vec::new();
    for severity in [
        Severity::Error,
        Severity::Warning,
        Severity::Information,
        Severity::Hint,
    ] {
        let many = count(severity);
        if many > 0 {
            let plural = if many == 1 { "" } else { "s" };
            said.push(format!("{many} {}{plural}", severity.title()));
        }
    }
    said.join(", ")
}

/// A question about a whole document that nobody asked for.
///
/// Standing, because the reader never presses anything to send one: they
/// are what a server can say about a file as a whole, and Obelus asks them
/// wherever the file has stopped moving -- opened, written, re-read, and a
/// moment after the reader stops typing.
///
/// One type because the three differ in four things and agree in
/// everything else. Which four is what this is: what to send, who answers
/// it, what to send with it, and what to do with the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Standing {
    /// What every token in the file is.
    Tokens,
    /// Where the colours in it are written down.
    Colours,
    /// What a server would have the reader know that the file does not say.
    Hints,
}

impl Standing {
    /// All of them, for the moments that ask everything.
    pub(super) const ALL: [Self; 3] = [Self::Tokens, Self::Colours, Self::Hints];

    /// The request that asks it.
    pub(super) const fn method(self) -> &'static str {
        match self {
            Self::Tokens => "textDocument/semanticTokens/full",
            Self::Colours => "textDocument/documentColor",
            Self::Hints => "textDocument/inlayHint",
        }
    }

    /// Whether the server says it answers this.
    ///
    /// A server that has not said so is not asked: an unanswerable question
    /// is a round trip for an error, and some servers answer one with an
    /// error that looks like a file with nothing in it.
    pub(super) fn answered_by(self, capabilities: &lsp_types::ServerCapabilities) -> bool {
        match self {
            Self::Tokens => capabilities.semantic_tokens_provider.is_some(),
            Self::Colours => obelus_lsp::colour::supported(capabilities),
            Self::Hints => obelus_lsp::hint::supported(capabilities),
        }
    }

    /// Whether the reader wants it at all.
    ///
    /// Only the hints have a switch: the other two are colours on a screen
    /// that was going to be coloured anyway, and these are cells the file
    /// does not contain.
    pub(super) const fn wanted(self, config: &obelus_config::Config) -> bool {
        match self {
            Self::Tokens | Self::Colours => true,
            Self::Hints => config.inlay_hints,
        }
    }

    /// What goes with it.
    ///
    /// `stop` is the document's own end, for the one of them that takes a
    /// range. The whole file rather than the rows on screen, which is what
    /// that range is for: scrolling would then be a question per screenful,
    /// and a question per screenful is a question while the reader is
    /// moving.
    pub(super) fn params(
        self,
        uri: &lsp_types::Uri,
        stop: lsp_types::Position,
    ) -> serde_json::Value {
        let document = serde_json::json!({ "uri": uri });
        match self {
            Self::Tokens | Self::Colours => serde_json::json!({ "textDocument": document }),
            Self::Hints => serde_json::json!({
                "textDocument": document,
                "range": { "start": { "line": 0, "character": 0 }, "end": stop },
            }),
        }
    }

    /// What is waited for, so the answer is known for what it is.
    pub(super) const fn asked(self) -> Asked {
        match self {
            Self::Tokens => Asked::Tokens,
            Self::Colours => Asked::Colours,
            Self::Hints => Asked::Hints,
        }
    }
}

/// What one question that is still out was about.
#[derive(Debug)]
pub(super) struct Question {
    pub(super) asked: Asked,
    pub(super) buffer: DocumentId,
    /// The document version it was asked against.
    pub(super) version: i32,
}

/// What a question was about.
///
/// Two kinds of answer come back over the same channel and are told apart by
/// the id they arrive under, so what was asked has to be remembered here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asked {
    /// One of the questions about the symbol under the cursor.
    Symbol(SymbolAction),
    /// Everything the file defines.
    Outline,
    /// The names the server knows across the project.
    Workspace,
    /// How the file should be laid out, asked because it is about to be
    /// written.
    Formatting,
    /// What every token in the file is.
    Tokens,
    /// Where the colours in the file are written down.
    Colours,
    /// What the server would have the reader know that the file does not
    /// say.
    Hints,
    /// Where else the name under the caret is used.
    Uses,
    /// Everywhere a symbol would have to change to be called something
    /// else.
    Rename,
    /// What has to change because a file is about to be somewhere else.
    ///
    /// The one question whose answer is not about a document: it is about
    /// a path, and the file it names may not be open at all.
    WillRename,
    /// The item a tree of calls will be rooted at.
    ///
    /// Apart from [`Asked::Symbol`] although it is asked from the same
    /// menu: the others answer with places and are done with, and this one
    /// answers with the thing every later question is asked about.
    Prepared,
    /// What one item in that tree calls, or who calls it, asked because
    /// the reader opened that row.
    Called {
        /// Which way round it was asked, so an answer that arrives after
        /// the reader turned round is not hung under the wrong tree.
        direction: obelus_lsp::hierarchy::Direction,
        /// Which row, by the name the tree gave it.
        ///
        /// Its name rather than its place: two questions about one tree
        /// are in the air at once, and either answer can land after the
        /// other has moved every row after it.
        id: u64,
    },
    /// Whether there is anything behind one row of that tree, asked
    /// because nobody has.
    ///
    /// Its own kind although it is the same request, because questions of
    /// a kind supersede each other: sharing one with [`Asked::Called`]
    /// would mean a probe a busy server is sitting on for two seconds is
    /// holding the reader's own key press behind it.
    Behind {
        /// Which way round it was asked.
        direction: obelus_lsp::hierarchy::Direction,
        /// Which row, by the name the tree gave it.
        id: u64,
    },
    /// What can be done about where the reader is.
    Actions,
    /// What the server would do to the whole file, asked because it is
    /// about to be written.
    Saving {
        /// Which of the kinds a save asks about.
        kind: usize,
    },
    /// That one, filled in.
    Saved {
        /// The same.
        kind: usize,
    },
    /// One of those, filled in.
    Action {
        /// Which of the offers, by its place in the list.
        at: usize,
    },
    /// What could be typed where the cursor was.
    ///
    /// The word's start rather than the cursor, because that is what makes
    /// a late answer still usable: two more letters of the same word is
    /// the same question, and the letters are the query.
    Completion {
        /// Where the word being completed starts.
        from: (LineNumber, CharColumn),
    },
    /// What a place in the file is.
    Hover {
        /// Which place, so that an answer about somewhere the reader has
        /// left can be thrown away.
        at: (LineNumber, CharColumn),
        /// Whether the pointer asked, which decides what "still there"
        /// means when the answer lands.
        pointed: bool,
    },
    /// What the call the cursor is inside takes.
    Signature {
        /// The line it was asked on. A panel about a call the reader has
        /// typed past is the same mistake as a completion for a word they
        /// have finished.
        line: LineNumber,
    },
    /// Everything about one candidate the reader is looking at.
    Resolve {
        /// Which candidate of the answer, by its place in it.
        index: usize,
    },
}

/// The name of the symbol the cursor is in, or the last one before it.
///
/// "Where am I in this file" is what an outline is usually opened to ask, and
/// a list that opens at the top answers "at the beginning", which is almost
/// never true.
pub(super) fn nearest_symbol(symbols: &[tags::Symbol], cursor: Option<Cursor>) -> Option<String> {
    let cursor = cursor?;
    symbols
        .iter()
        .rfind(|symbol| symbol.line <= cursor.line)
        .map(|symbol| symbol.name.clone())
}

/// One row per place, in the shape a search's rows have.
///
/// The line each place names, with the file it is in after it -- rather
/// than the path and the line number as the whole of the row, which is
/// what this listed before. A reference and a match are the same kind of
/// thing to a reader: somewhere in the workspace worth looking at, and what
/// says whether it is worth looking at is the line, not the number of it.
///
/// The lines are read here rather than left to the frame that draws them.
/// The list is filtered by typing, and filtering happens against the
/// labels: rows whose text arrived later would be rows a query could not
/// reach.
fn place_rows(places: &[obelus_lsp::action::Place], root: &Path) -> Vec<PickerItem> {
    /// The most a file may weigh before its lines are not worth reading for
    /// a label. The search's own limit, for the same reason.
    const BIGGEST: u64 = 2 * 1024 * 1024;

    let mut read: HashMap<PathBuf, Vec<String>> = HashMap::new();
    places
        .iter()
        .map(|place| {
            let lines = read.entry(place.path.clone()).or_insert_with(|| {
                let big = std::fs::metadata(&place.path)
                    .map(|about| about.len() > BIGGEST)
                    .unwrap_or(true);
                if big {
                    return Vec::new();
                }
                std::fs::read_to_string(&place.path)
                    .map(|text| text.lines().map(str::to_string).collect())
                    .unwrap_or_default()
            });
            let text = lines
                .get(place.line as usize)
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty());
            let at = format!(
                "{}:{}",
                relative(&place.path, root),
                place.line.saturating_add(1)
            );
            PickerItem {
                prose: false,
                marker: None,
                // No glyph. The file is named on the right of the row, and a
                // column of the same glyph down a list of references says
                // nothing.
                icon: None,
                // A line that could not be read leaves the place itself as
                // the row: it is still somewhere to go.
                label: text.unwrap_or_else(|| at.clone()),
                detail: None,
                trailing: Some(at),
                changed: None,
                value: PickerValue::Place {
                    path: place.path.clone(),
                    line: place.line,
                    character: place.character,
                    end_line: place.end_line,
                    end_character: place.end_character,
                },
                enabled: true,
                // Filled in by the frame that draws them, for the rows on
                // screen, from the same pass that colours a search's.
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            }
        })
        .collect()
}

/// What to call the server for a language, to a reader.
///
/// Its own program's name where there is one, because that is the thing
/// they would install, start or look in the log of.
pub(super) fn named(language: LanguageId) -> &'static str {
    obelus_lsp::command_for(language).unwrap_or("The language server")
}

#[cfg(test)]
mod tests {
    use obelus_component::picker::PickerValue;
    use obelus_lsp::action::Place;

    use super::place_rows;

    /// A list of places reads like a list of matches: the line each one
    /// names, and the file after it.
    #[test]
    fn a_place_is_the_line_it_names_and_the_file_it_is_in() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("tests/fixtures/long.rs");
        let places = [
            Place {
                path: path.clone(),
                line: 1,
                character: 6,
                end_line: 1,
                end_character: 13,
            },
            // Twice from the same file, which is read once.
            Place {
                path: path.clone(),
                line: 2,
                character: 0,
                end_line: 2,
                end_character: 0,
            },
        ];

        let rows = place_rows(&places, &root);
        assert_eq!(rows.len(), 2);
        // The line, trimmed, because a row that starts with a file's
        // indentation is a row of blanks.
        assert!(
            rows[0].label.starts_with("const NAMES: [&str; 12]"),
            "not the line the place names: {:?}",
            rows[0].label
        );
        assert_eq!(
            rows[0].trailing.as_deref(),
            Some("tests/fixtures/long.rs:2"),
            "the file is not on the row, or is not relative to the root"
        );
        assert!(rows[0].icon.is_none(), "a glyph per row says nothing here");
        // And the place itself is still what choosing the row goes to.
        assert!(matches!(
            &rows[0].value,
            PickerValue::Place {
                line: 1,
                character: 6,
                ..
            }
        ));
        assert_eq!(rows[1].label, "fn after() {}");
    }

    /// A place Obelus cannot read the line of is still somewhere to go, and
    /// the row says where.
    #[test]
    fn a_line_that_cannot_be_read_leaves_the_place_as_the_row() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let gone = root.join("tests/fixtures/nothing-here.rs");
        let places = [Place {
            path: gone,
            line: 41,
            character: 0,
            end_line: 41,
            end_character: 0,
        }];

        let rows = place_rows(&places, &root);
        assert_eq!(rows[0].label, "tests/fixtures/nothing-here.rs:42");
        assert_eq!(rows[0].trailing.as_deref(), Some(rows[0].label.as_str()));
    }

    /// A line past the end of a file, and a blank line inside one, are the
    /// same case: there is nothing to show, so the place is the row. A row
    /// of nothing is a row a reader cannot tell from a bug.
    #[test]
    fn a_blank_line_is_not_a_row_of_nothing() {
        let directory = std::env::temp_dir().join(format!("obelus-places-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a directory");
        let path = directory.join("gaps.rs");
        std::fs::write(&path, "fn one() {}\n\n    \nfn two() {}\n").expect("writing it");

        // An empty line, a line of blanks, and a line past the end.
        let places: Vec<Place> = [1, 2, 9_000]
            .into_iter()
            .map(|line| Place {
                path: path.clone(),
                line,
                character: 0,
                end_line: line,
                end_character: 0,
            })
            .collect();
        let rows = place_rows(&places, &directory);
        assert_eq!(rows[0].label, "gaps.rs:2");
        assert_eq!(
            rows[1].label, "gaps.rs:3",
            "a line of blanks became a row of blanks"
        );
        assert_eq!(rows[2].label, "gaps.rs:9001");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
