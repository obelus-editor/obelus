//! What obelus asks a language server, and what it does with the answers.
//!
//! One half of the semantic layer: [`crate::lsp`] speaks the protocol, and
//! this decides when to speak it and what a reply means to the views. The
//! split is deliberate -- everything here needs the application's state, and
//! nothing there does.

use super::*;

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

    /// The server for the file being read, and what it is doing.
    ///
    /// Only the current file's: a status bar listing every server obelus has
    /// started would be a table, and the question a reader has is whether
    /// *this* file's questions can be answered.
    #[must_use]
    pub fn server_state(&self) -> Option<(&'static str, lsp::ServerState)> {
        let language = self.current_buffer()?.language()?;
        let client = self.servers.get(&language)?;
        Some((lsp::command_for(language)?, client.state()))
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
    /// directory, and asking it about a file outside its own tree gets
    /// answers about a project it cannot see.
    pub(super) fn serve(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        if !buffer.path().starts_with(&self.working_directory) {
            tracing::debug!(path = %buffer.path().display(), "outside the root, so no server");
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
            let Some(server) = lsp::server_for(language) else {
                return;
            };
            let command = server.command;
            if !lsp::on_path(command) {
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
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
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
    }

    /// Tells the server a document changed.
    pub(super) fn change_document(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let text = buffer.text().rope().to_string();
        let version = buffer.version();

        if let Some(client) = self.servers.get_mut(&language) {
            let _ = client.notify(
                "textDocument/didChange",
                &serde_json::json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            );
        }
    }

    /// Offers what a language server can say about the symbol under the
    /// cursor.
    ///
    /// One key for every question rather than a key each: the questions all
    /// take the same argument and differ only in what comes back, and a menu
    /// can say which ones this server actually answers.
    pub fn open_symbol_menu(&mut self) {
        let actions = match self.symbol_actions() {
            Ok(actions) => actions,
            // No menu at all. A list with one row explaining itself is still
            // a list: it covers the code, it has to be dismissed, and it
            // offers nothing. The reason belongs on the status bar, which is
            // where every other passing word about state goes.
            Err(why) => {
                self.note = Some(why);
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
                    icon: icons::enabled().then(|| icons::for_command(command.spec().name)),
                    label: command.spec().name.to_string(),
                    detail: Some(command.spec().title.to_string()),
                    trailing: self.keymap.chord_for(command).map(KeyChord::label),
                    value: PickerValue::Command(command),
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: 0,
                    kind: None,
                    tab: None,
                }
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    /// What the server for the current buffer will answer, or why nothing.
    pub(super) fn symbol_actions(&self) -> Result<Vec<SymbolAction>, String> {
        let buffer = self
            .current_buffer()
            .ok_or_else(|| "no file open".to_string())?;
        let language = buffer
            .language()
            .ok_or_else(|| "obelus does not know this language".to_string())?;

        // On a name, before anything about servers. Every question in the
        // menu is about the thing under the cursor, and on a bracket or a
        // blank line there is no thing: the answer would be nothing, four
        // different ways. Asked first because it is the reason a reader can
        // act on -- move the cursor -- where the others are about the
        // machine.
        let cursor = buffer.cursor();
        let at = buffer
            .text()
            .byte_of_char(buffer.text().char_offset(cursor.line, cursor.column));
        if !buffer
            .syntax()
            .is_some_and(|state| state.is_name_at(buffer.text(), at))
        {
            return Err("no symbol here".to_string());
        }

        let Some(client) = self.servers.get(&language) else {
            return Err(match lsp::command_for(language) {
                Some(command) if !lsp::on_path(command) => format!("{command} is not installed"),
                Some(_) => format!("no server running for {}", language.name()),
                None => format!("no language server for {}", language.name()),
            });
        };
        let Some(capabilities) = client.capabilities() else {
            return Err("the language server is still starting".to_string());
        };
        let actions: Vec<SymbolAction> = action::ALL
            .iter()
            .copied()
            .filter(|action| action.supported(capabilities))
            .collect();
        if actions.is_empty() {
            return Err("the language server answers none of these".to_string());
        }
        Ok(actions)
    }

    /// Asks whichever question a command names.
    pub fn ask_about_symbol(&mut self, command: crate::command::Command) {
        let Some(action) = SymbolAction::for_command(command) else {
            return;
        };
        self.ask(action);
    }

    /// Asks one of those questions.
    fn ask(&mut self, action: SymbolAction) {
        let Some(id) = self.current else { return };
        let Some(buffer) = self.buffers.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
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

        match client.request(action.method(), &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
                    Question {
                        asked: Asked::Symbol(action),
                        buffer: id,
                        version,
                    },
                );
                self.note = Some(format!("{}\u{2026}", action.title()));
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask");
                self.note = Some("the language server is not listening".to_string());
            }
        }
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

        let now = self
            .buffers
            .get(question.buffer.get())
            .and_then(Option::as_ref)
            .map(Buffer::version);
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
        };
        let indexing = self.server_working_on().is_some();
        match action::outcome_of(reply.result, question.version, now, indexing) {
            Outcome::Stale => {
                tracing::debug!(
                    asked_against = question.version,
                    now = ?now,
                    "dropping an answer about a version that has been replaced"
                );
                self.note = Some("the file changed while asking".to_string());
            }
            Outcome::Failed(message) => self.note = Some(message),
            Outcome::NotYet => self.note = Some("still indexing".to_string()),
            Outcome::Nothing => {
                self.note = Some(format!("nothing for {}", action.title()));
            }
            Outcome::Places(mut places) if places.len() == 1 => {
                let place = places.remove(0);
                self.note = None;
                self.go_to(&place.path, place.line, place.character);
            }
            Outcome::Places(places) => {
                let items = place_rows(&places, &self.working_directory);
                self.note = None;
                self.picker = Some(Picker::new(items, PickerLayout::FullArea));
            }
        }
    }

    /// Stops the server for the current file and starts it again.
    ///
    /// The way out of a server that has died, or wedged, or was installed
    /// after obelus started: those are the three states where every question
    /// gets the same silence, and none of them is worth restarting the whole
    /// program over. With none running it simply starts one, which is why it
    /// is offered whether or not there is one.
    pub fn restart_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.note = Some("no file to restart a server for".to_string());
            return;
        };

        self.stop(language);
        // Deliberately stopped and now deliberately started: the restart is
        // the way back from a stop, so it lifts one.
        self.stopped.remove(&language);

        // Announce every open file of that language to the new server, not
        // just the current one: the others are still open, and a server that
        // has not been told about a file answers nothing about it.
        let indices: Vec<usize> = (0..self.buffers.len())
            .filter(|index| {
                self.buffers
                    .get(*index)
                    .and_then(Option::as_ref)
                    .and_then(Buffer::language)
                    .is_some_and(|of| of == language)
            })
            .collect();
        for index in indices {
            self.serve(index);
        }

        self.note = Some(match lsp::command_for(language) {
            Some(command) if self.servers.contains_key(&language) => format!("restarted {command}"),
            Some(command) if !lsp::on_path(command) => format!("{command} is not installed"),
            Some(command) => format!("{command} would not start"),
            None => format!("no language server for {}", language.name()),
        });
    }

    /// Stops the server for the current file and leaves it stopped.
    ///
    /// For a server that is costing more than it is answering. It stays
    /// stopped until `lsp.restart`, because otherwise opening the next
    /// file of that language would start it again.
    pub fn stop_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.note = Some("no file to stop a server for".to_string());
            return;
        };
        let was_running = self.stop(language);
        self.stopped.insert(language);
        self.note = Some(match (lsp::command_for(language), was_running) {
            (Some(command), true) => format!("stopped {command}"),
            (Some(command), false) => format!("{command} was not running"),
            (None, _) => format!("no language server for {}", language.name()),
        });
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
            self.note = Some("no file to outline".to_string());
            return;
        };
        let path = buffer.path().to_path_buf();
        let Some(language) = buffer.syntax().map(SyntaxState::language) else {
            self.note = Some("obelus does not know this language".to_string());
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
            picker.when_empty("asking the language server\u{2026}");
            picker.is_outline_of(path);
            self.picker = Some(picker);
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
                    icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                    kind: Some(symbol.kind),
                    label: symbol.name.clone(),
                    detail: None,
                    trailing: Some(format!("{}", symbol.line.get() + 1)),
                    value: PickerValue::Place {
                        path: path.clone(),
                        line,
                        character,
                        end_line: line,
                        end_character,
                    },
                    tab: None,
                }
            })
            .collect();

        let mut picker = Picker::new(items, PickerLayout::FullArea);
        picker.when_empty(if tags::has_tags(language) {
            "this file defines nothing"
        } else {
            // Not the same fact, and the difference is the reader's next
            // move: one means look elsewhere, the other means do not bother
            // pressing this key for this language.
            "no outline for this language"
        });
        // On the symbol the cursor is in, or the nearest one above it, which
        // is the answer to "where am I" that an outline is usually opened to
        // ask.
        if let Some(here) = nearest_symbol(&symbols, self.current_buffer().map(Buffer::cursor)) {
            picker.prefer(here);
        }
        picker.is_outline_of(path);
        self.picker = Some(picker);
    }

    /// Asks a server what a file defines, and says whether the question got
    /// out.
    fn ask_outline(&mut self, path: &Path, language: LanguageId) -> bool {
        let Ok(uri) = lsp::client::uri_for(path) else {
            return false;
        };
        let Some(id) = self.current else { return false };
        let Some(version) = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(Buffer::version)
        else {
            return false;
        };
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        match client.request("textDocument/documentSymbol", &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
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

        let symbols = lsp::outline::symbols_in(reply.result);
        if symbols.is_empty() {
            tracing::debug!("the server has no outline, so the tree's it is");
            self.outline_from_tree();
            return;
        }

        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| PickerItem {
                icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                enabled: true,
                colours: None,
                status: None,
                depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                kind: Some(symbol.kind),
                label: symbol.name.clone(),
                detail: None,
                trailing: Some(format!("{}", symbol.line.saturating_add(1))),
                value: PickerValue::Place {
                    path: path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
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
            picker.when_empty("this file defines nothing");
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

    /// Sends `workspace/symbol`, and says whether the question got out.
    pub(super) fn ask_workspace_symbols(&mut self, language: LanguageId, query: &str) -> bool {
        let Some(id) = self.current else { return false };
        let version = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(Buffer::version)
            .unwrap_or_default();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "query": query });
        match client.request("workspace/symbol", &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
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
        let symbols = lsp::outline::found_in(reply.result);
        let root = self.working_directory.clone();
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
                icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
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
                value: PickerValue::Place {
                    path: symbol.path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
            })
            .collect();
        picker.replace(items);
        picker.while_empty("the server knows no such name");
    }
}

/// What one question that is still out was about.
#[derive(Debug)]
pub(super) struct Question {
    asked: Asked,
    buffer: BufferId,
    /// The document version it was asked against.
    version: i32,
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
fn place_rows(places: &[crate::lsp::action::Place], root: &Path) -> Vec<PickerItem> {
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
                // No glyph. The file is named on the right of the row, and a
                // column of the same glyph down a list of references says
                // nothing.
                icon: None,
                // A line that could not be read leaves the place itself as
                // the row: it is still somewhere to go.
                label: text.unwrap_or_else(|| at.clone()),
                detail: None,
                trailing: Some(at),
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
                kind: None,
                tab: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::place_rows;
    use crate::{component::picker::PickerValue, lsp::action::Place};

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

    /// A place obelus cannot read the line of is still somewhere to go, and
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
