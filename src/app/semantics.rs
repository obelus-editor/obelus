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
        self.ask_tokens(index);
    }

    /// Tells the server a document changed.
    /// Tells a server the document has been written to disk.
    ///
    /// Some of them do work only then -- a linter that runs on save, a
    /// formatter's idea of the last good version -- and none of them can
    /// know from `didChange`, which says only that the text moved.
    pub(super) fn saved_document(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
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
        // of it is worth having: see [`App::ask_tokens`].
        self.ask_tokens(index);
    }

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
        // Nothing about a commit's version of the file: the server is
        // being told what is at this path, and this is not it.
        if !buffer.content().is_file() {
            return;
        }
        let version = buffer.version();
        // The whole document, which is what obelus sends and means to: a
        // range needs the *old* document's coordinates in the encoding the
        // server agreed to, which is the shape every coordinate bug in this
        // program has had.
        //
        // Copied out only where there is somebody to send it to, though.
        // With no server running -- no language, none installed, one that
        // died -- this was a copy of the file per keystroke that nothing
        // ever read.
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        let text = buffer.text().rope().to_string();
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
                    prose: false,
                    marker: None,
                    icon: icons::enabled().then(|| icons::for_command(command)),
                    label: command.spec().name.to_string(),
                    detail: Some(command.spec().title.to_string()),
                    trailing: self.keymap.chord_for(command).map(KeyChord::label),
                    changed: None,
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
        if !self.name_at(buffer) {
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

    /// Asks the server to classify every token in a document.
    ///
    /// Not on every change, which is what a `didChange` would suggest: the
    /// answer describes one version, an edit invalidates all of it, and a
    /// full-file classification per keystroke is work the server does and
    /// throws away. It is asked where a document stops moving -- opened,
    /// re-read, written -- which is also where a reader starts looking
    /// around it. While they are typing, the answer goes stale and
    /// [`App::name_at`] falls back to the tree obelus parses itself.
    pub(super) fn ask_tokens(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        // Nothing about a commit's version, the same as its siblings.
        if !buffer.content().is_file() {
            return;
        }
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let version = buffer.version();
        let id = BufferId::new(index);
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client
            .capabilities()
            .is_some_and(|capabilities| capabilities.semantic_tokens_provider.is_some())
        {
            return;
        }
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        if let Ok(request) = client.request("textDocument/semanticTokens/full", &params) {
            self.asked.insert(
                (language, request),
                Question {
                    asked: Asked::Tokens,
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Keeps a classification of a document, if it is still about it.
    fn on_tokens(
        &mut self,
        id: BufferId,
        version: i32,
        language: LanguageId,
        reply: lsp::client::Reply,
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
        let tokens =
            lsp::tokens::Tokens::decode(&numbers, legend, client.encoding().clone(), version);
        let Some(path) = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(|buffer| buffer.path().to_path_buf())
        else {
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
            let at =
                lsp::position::to_lsp(buffer.text(), cursor.line, cursor.column, tokens.encoding());
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
            Asked::Formatting => {
                self.on_formatting(question.buffer, question.version, reply);
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
                let mut picker = Picker::new(items, PickerLayout::FullArea);
                picker.previews();
                self.picker = Some(picker);
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
    /// stopped until `restart-server`, because otherwise opening the next
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
            picker.previews();
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
                    prose: false,
                    marker: None,
                    icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
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
        picker.previews();
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
                prose: false,
                marker: None,
                icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                enabled: true,
                colours: None,
                status: None,
                depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
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
    /// Asks how the file should be laid out, so it can be written that way.
    ///
    /// Says whether anybody was asked. A save that nobody could format goes
    /// ahead unformatted rather than waiting for an answer that is not
    /// coming: the reader pressed save, and a setting they turned on is not
    /// a reason to refuse them.
    pub(super) fn ask_formatting(&mut self, index: usize) -> bool {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return false;
        };
        let (Some(language), true) = (buffer.language(), buffer.content().is_file()) else {
            return false;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
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
                self.asked.insert(
                    (language, request),
                    Question {
                        asked: Asked::Formatting,
                        buffer: BufferId::new(index),
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
    fn on_formatting(&mut self, id: BufferId, version: i32, reply: Reply) {
        let now = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(Buffer::version);
        if now != Some(version) {
            tracing::debug!("the file changed while it was being laid out");
            self.note = Some("the file changed while formatting".to_string());
        } else if let Some(edits) = action::edits_in(reply.result.ok()) {
            let encoding = self
                .buffers
                .get(id.get())
                .and_then(Option::as_ref)
                .and_then(Buffer::language)
                .map_or_else(
                    || lsp_types::PositionEncodingKind::UTF16,
                    |language| self.encoding_for(language),
                );
            for edit in edits.into_iter().rev() {
                if let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::as_mut) {
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
                    buffer.edit(span, &edit.new_text, crate::buffer::undo::Doing::Whole);
                }
            }
            self.change_document(id.get());
        }
        self.write_now(id.get());
    }

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
        let root = self.working_directory.clone();
        // This tree unless the reader has asked to see past it: a list of
        // every name a server knows is mostly the registry's, and the one
        // they meant is somewhere among them.
        let within = (!self.outside).then_some(root.as_path());
        let symbols = lsp::outline::found_in(reply.result, within);
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
                changed: None,
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

impl App {
    /// Keeps what a server says is wrong with a file.
    ///
    /// Only for a file that is open: the ranges are turned into places in
    /// a document here, and a document obelus does not have is one it
    /// cannot place anything in. A server that talks about the rest of the
    /// project -- rust-analyzer does, after a `cargo check` -- is not
    /// wrong to, and this is where those would be kept if obelus ever
    /// listed them.
    pub(super) fn on_published(&mut self, language: LanguageId, params: &serde_json::Value) {
        let Some(path) = crate::lsp::trouble::path_of(params) else {
            return;
        };
        let Some(buffer) = self
            .buffers
            .iter()
            .flatten()
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
        let troubles = crate::lsp::trouble::published(params, buffer.text(), &encoding);
        // An empty set is a server saying the file is clean, which is news
        // worth keeping: it is how what was wrong stops being shown.
        match troubles.is_empty() {
            true => self.troubles.remove(&path),
            false => self.troubles.insert(path, troubles),
        };
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

    /// Lists what the server says is wrong with this file.
    pub fn open_troubles(&mut self) {
        let troubles = self.troubles().to_vec();
        if troubles.is_empty() {
            self.note = Some(match self.server_state() {
                Some((_, lsp::ServerState::Ready)) => "nothing wrong with this file".to_string(),
                Some((command, _)) => format!("{command} is not answering"),
                None => "no language server for this file".to_string(),
            });
            return;
        }
        // Back into the protocol's units, which is what a row that names a
        // place carries: one kind of value for "go here", and the
        // conversion in one place.
        let path = self
            .current_buffer()
            .map_or(PathBuf::new(), |buffer| buffer.path().to_path_buf());
        let encoding = self
            .current_buffer()
            .and_then(Buffer::language)
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let Some(text) = self
            .current_buffer()
            .map(|buffer| buffer.text().rope().to_string())
        else {
            return;
        };
        let text = crate::text::Text::from_string(&text);
        let at = |line, column| position::to_lsp(&text, line, column, &encoding);

        let items = troubles
            .iter()
            .map(|trouble| PickerItem {
                icon: icons::enabled().then(|| icons::for_kind(trouble.severity.kind())),
                label: trouble.summary().to_string(),
                detail: trouble.source.clone(),
                prose: true,
                marker: None,
                trailing: Some(format!("{}", trouble.span.line.get() + 1)),
                changed: None,
                value: {
                    let start = at(trouble.span.line, trouble.span.column);
                    let end = at(trouble.span.end_line, trouble.span.end_column);
                    PickerValue::Place {
                        path: path.clone(),
                        line: start.line,
                        character: start.character,
                        end_line: end.line,
                        end_character: end.character,
                    }
                },
                depth: 0,
                status: None,
                enabled: true,
                colours: None,
                kind: Some(trouble.severity.kind()),
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: 10 });
        picker.keeps_order(true);
        picker.about(&format!(
            "{} in {}",
            counted(&troubles),
            crate::app::relative(&path, &self.working_directory)
        ));
        self.picker = Some(picker);
    }
}

/// How many of each severity, as a phrase.
fn counted(troubles: &[crate::lsp::trouble::Trouble]) -> String {
    use crate::lsp::trouble::Severity;

    let count = |severity: Severity| {
        troubles
            .iter()
            .filter(|trouble| trouble.severity == severity)
            .count()
    };
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

/// What one question that is still out was about.
#[derive(Debug)]
pub(super) struct Question {
    pub(super) asked: Asked,
    pub(super) buffer: BufferId,
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
