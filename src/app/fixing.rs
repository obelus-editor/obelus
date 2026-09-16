//! What a server offers to do about where the reader is, and doing it.
//!
//! The other half of showing what is wrong: a reader who can see the
//! mistake underlined should be able to ask what to do about it without
//! leaving the line. The offers are a list, because there are usually
//! several and only the reader knows which -- and the list is the ordinary
//! compact one, so it filters by typing and closes on escape like every
//! other list in obelus.

use super::*;
use crate::lsp::actions;

/// The kinds a save asks for, in the order they are made.
///
/// `source.` is the protocol's word for an action about the whole file
/// rather than about a place in it, which is what makes these the only
/// ones worth doing to a file nobody pressed a key on. Matched as
/// prefixes: the kinds are dotted paths and a server may answer with
/// something under one of them.
///
/// The corrections first and the imports after, because a correction can
/// add an import or take one away: a sort that ran first would be a sort
/// of a list that then changed.
///
/// One at a time, each asked after the last has been made. Two edits from
/// one answer are two edits worked out against the same text, and the
/// first of them moves what the second is measured against.
const ON_SAVE: &[&str] = &["source.fixAll", "source.organizeImports"];

impl App {
    /// Asks what can be done about the selection, or the line the cursor
    /// is on.
    pub fn ask_actions(&mut self) {
        let Some(id) = self.current else {
            self.note = Some("no file open".to_string());
            return;
        };
        let Some(buffer) = self.buffers.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        if !buffer.content().is_file() || buffer.mode() != crate::buffer::Mode::Edit {
            self.note = Some("this is not a file to change".to_string());
            return;
        }
        let Some(language) = buffer.language() else {
            self.note = Some("no language server for this file".to_string());
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        // What the reader has hold of: the selection where there is one,
        // and the line the cursor is on where there is not. A point would
        // be right for a caret between two characters and wrong for the
        // question, which is about something that is there.
        let cursor = buffer.cursor();
        let span = buffer.selection().unwrap_or(crate::coordinates::Span {
            line: cursor.line,
            column: CharColumn::new(0),
            end_line: cursor.line,
            end_column: buffer.text().line_length(cursor.line),
        });
        let version = buffer.version();
        // The diagnostics the range covers, which is the context the whole
        // question turns on: a quick fix is offered *for* a diagnostic, and
        // a request with none in it gets the refactorings and nothing else.
        let troubles: Vec<serde_json::Value> = self
            .troubles()
            .iter()
            .filter(|trouble| overlaps(trouble.span, span))
            .map(|trouble| trouble.item.clone())
            .collect();

        let encoding = self
            .servers
            .get(&language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let start = position::to_lsp(buffer.text(), span.line, span.column, &encoding);
        let end = position::to_lsp(buffer.text(), span.end_line, span.end_column, &encoding);

        if let Some(why) = self.why_not_asking(language) {
            self.note = Some(why);
            return;
        }
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client.capabilities().is_some_and(actions::supported) {
            self.note = Some(format!("{} does not offer actions", server_named(language)));
            return;
        }
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "range": { "start": start, "end": end },
            "context": { "diagnostics": troubles },
        });
        if let Ok(request) = client.request("textDocument/codeAction", &params) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Actions,
                    buffer: id,
                    version,
                },
            );
            self.note = Some("asking what can be done\u{2026}".to_string());
        }
    }

    /// Lists what the server offered.
    ///
    /// Refused where the document has moved since it was asked. Every
    /// range in every offer is a place in the file as it was, and a file
    /// that has changed has moved them: the offers would still apply, to
    /// the wrong text, without anything going wrong loudly enough to
    /// notice.
    pub(super) fn on_actions(&mut self, id: BufferId, version: i32, reply: Reply) {
        if !self.unmoved(id, version) {
            self.note = Some("the file changed while asking".to_string());
            return;
        }
        let offered = actions::offered_in(&reply.result);
        if offered.is_empty() {
            self.note = Some("nothing to do here".to_string());
            return;
        }
        // Everything the server offered is an offer it will not carry
        // out. A list of rows the reader can only step over is a list
        // that answers nothing: the reasons are the answer, so they go
        // where a sentence goes.
        if offered.iter().all(actions::Action::refused) {
            let reasons: Vec<&str> = offered
                .iter()
                .filter_map(|action| action.disabled.as_deref())
                .collect();
            self.note = Some(format!("nothing can be done here: {}", reasons.join("; ")));
            return;
        }
        let items = offered
            .iter()
            .enumerate()
            .map(|(at, action)| PickerItem {
                icon: icons::enabled().then_some(icons::for_command(Command::SymbolActions)),
                label: action.title.clone(),
                // The reason it cannot be done, where there is one, and
                // nothing otherwise. The other thing that could go here
                // is the kind -- `refactor.rewrite`, `quickfix` -- which
                // is the protocol's own word for its own filing and tells
                // a reader choosing between two offers nothing the titles
                // have not already said. A reason is the opposite: it is
                // the whole of what a row they cannot choose is for, and
                // the list steps over such a row, so there is no moment
                // later at which it could be said instead.
                detail: action.disabled.clone(),
                prose: true,
                marker: None,
                trailing: None,
                changed: None,
                value: PickerValue::Action(at),
                depth: 0,
                status: None,
                enabled: !action.refused(),
                colours: None,
                kind: None,
                tab: None,
            })
            .collect();
        self.actions = offered;
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.keeps_order(true);
        picker.about("what the language server offers to do here");
        self.note = None;
        self.show_list(picker);
    }

    /// Does the one the reader chose.
    pub(super) fn do_action(&mut self, at: usize) {
        let Some(action) = self.actions.get(at).cloned() else {
            return;
        };
        // An action that arrived without its edit is one the server said
        // it would work out if asked: asked now, done when it answers.
        if action.unresolved() {
            self.resolve_action(at);
            return;
        }
        self.carry_out(&action);
    }

    /// Asks the server to fill an action in.
    fn resolve_action(&mut self, at: usize) {
        let Some(id) = self.current else { return };
        let Some(language) = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .and_then(Buffer::language)
        else {
            return;
        };
        let Some(item) = self.actions.get(at).map(|action| action.item.clone()) else {
            return;
        };
        let version = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map_or(0, Buffer::version);
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client.capabilities().is_some_and(actions::resolves) {
            self.note = Some("the server left this one unfinished".to_string());
            return;
        }
        if let Ok(request) = client.request("codeAction/resolve", &item) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Action { at },
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Takes a filled-in action and does it.
    ///
    /// The other half of the same guard. This is the longer window of the
    /// two: the list has closed by now, so the keys are the document's
    /// again and the reader can type the whole time the server is working
    /// the edit out.
    pub(super) fn on_action(&mut self, at: usize, id: BufferId, version: i32, reply: Reply) {
        if !self.unmoved(id, version) {
            self.note = Some("the file changed while asking".to_string());
            return;
        }
        let Ok(result) = reply.result else {
            self.note = Some("the server could not work that out".to_string());
            return;
        };
        let Some(action) = self.actions.get_mut(at) else {
            return;
        };
        action.item = result;
        let action = action.clone();
        self.carry_out(&action);
    }

    /// Makes an action's edit, and runs its command.
    ///
    /// Both, in that order, because an action may have either or both: the
    /// edit is what changes the files, and the command is what the server
    /// wants to do about it afterwards.
    fn carry_out(&mut self, action: &actions::Action) {
        let mut said = Vec::new();
        if let Some(wanted) = action.edit()
            && !(wanted.is_empty() && wanted.refused.is_empty())
        {
            said.push(self.apply_wanted(&wanted));
        }
        if let Some(command) = action.command() {
            said.push(self.run_server_command(&command));
        }
        self.note = Some(match said.is_empty() {
            true => format!("{} did nothing", action.title),
            false => said.join("; "),
        });
    }

    /// Asks the server to run one of its own commands.
    fn run_server_command(&mut self, command: &lsp_types::Command) -> String {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            return "no language server for this file".to_string();
        };
        let Some(client) = self.servers.get_mut(&language) else {
            return "no language server for this file".to_string();
        };
        let params = serde_json::json!({
            "command": command.command,
            "arguments": command.arguments.clone().unwrap_or_default(),
        });
        match client.request("workspace/executeCommand", &params) {
            // Nothing is waiting for the answer: what a command does, it
            // does to the project, and what obelus would learn from the
            // result is nothing it can act on. What it *sends back* is a
            // `workspace/applyEdit`, and that is where the change to the
            // files actually arrives -- made by `on_asked_edit`, not here.
            Ok(_) => format!("asked the server to {}", command.title),
            Err(_) => "the language server is not listening".to_string(),
        }
    }

    /// Asks for one of the whole-file actions, because the file is about
    /// to be written.
    ///
    /// The only code action obelus sends on its own account, which is the
    /// whole of why it says `only`: a request with no kind on it comes
    /// back with every refactoring the cursor happens to be near, and
    /// applying one of those to a file somebody pressed save on would be
    /// obelus rewriting their code on its own initiative.
    ///
    /// Says whether anybody was asked. A save nobody could tidy goes ahead
    /// untidied rather than waiting for an answer that is not coming.
    pub(super) fn ask_on_save(&mut self, index: usize, kind: usize) -> bool {
        let Some(want) = ON_SAVE.get(kind) else {
            return false;
        };
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
        // The whole file, because that is what a `source.` action is
        // about: a range around the cursor would be asking whichever
        // lines the reader happens to have stopped on.
        let lines = buffer.text().line_count();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        if !client.capabilities().is_some_and(actions::supported) {
            return false;
        }
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": lines, "character": 0 },
            },
            "context": { "diagnostics": [], "only": [want] },
        });
        match client.request("textDocument/codeAction", &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Saving { kind },
                        buffer: BufferId::new(index),
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "not asking what to do before writing");
                false
            }
        }
    }

    /// Asks for the next kind, and writes the file when there is none.
    pub(super) fn next_on_save(&mut self, index: usize, from: usize) {
        for kind in from..ON_SAVE.len() {
            if self.ask_on_save(index, kind) {
                self.note = Some("tidying it up\u{2026}".to_string());
                return;
            }
        }
        self.format_then_write(index);
    }

    /// Takes what the server offered and either does it or asks for it in
    /// full, and gets on with the save either way.
    pub(super) fn on_saving(&mut self, id: BufferId, version: i32, kind: usize, reply: Reply) {
        if !self.unmoved(id, version) {
            // Not a note: the reader pressed save, the save is what they
            // are waiting for, and this was obelus's own idea.
            tracing::debug!("the file changed while it was being looked at");
            self.write_now(id.get());
            return;
        }
        let want = ON_SAVE.get(kind).copied().unwrap_or_default();
        let Some(action) = actions::offered_in(&reply.result)
            .into_iter()
            .find(|action| {
                !action.refused()
                    && action
                        .kind
                        .as_deref()
                        .is_some_and(|kind| kind.starts_with(want))
            })
        else {
            // A server with nothing to say about the file as a whole,
            // which is many of them.
            self.next_on_save(id.get(), kind + 1);
            return;
        };
        if action.unresolved() && self.resolve_on_save(id, version, kind, &action) {
            return;
        }
        self.made_on_save(id, kind, action.edit());
    }

    /// Asks for the edit of an offer that arrived without one.
    fn resolve_on_save(
        &mut self,
        id: BufferId,
        version: i32,
        kind: usize,
        action: &actions::Action,
    ) -> bool {
        let Some(language) = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .and_then(Buffer::language)
        else {
            return false;
        };
        let item = action.item.clone();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        if !client.capabilities().is_some_and(actions::resolves) {
            return false;
        }
        match client.request("codeAction/resolve", &item) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Saved { kind },
                        buffer: id,
                        version,
                    },
                );
                true
            }
            Err(_) => false,
        }
    }

    /// The filled-in offer, made.
    pub(super) fn on_saved(&mut self, id: BufferId, version: i32, kind: usize, reply: Reply) {
        if !self.unmoved(id, version) {
            tracing::debug!("the file changed while the change was being worked out");
            self.write_now(id.get());
            return;
        }
        let wanted = reply
            .result
            .ok()
            .map(|result| crate::lsp::edits::wanted_in(&result));
        self.made_on_save(id, kind, wanted);
    }

    /// Makes the edit, if there is one, and carries on saving.
    fn made_on_save(
        &mut self,
        id: BufferId,
        kind: usize,
        wanted: Option<crate::lsp::edits::Wanted>,
    ) {
        if let Some(wanted) = wanted
            && !wanted.is_empty()
        {
            // Through the same machine as a rename, which means a server
            // that named another file gets that file opened rather than
            // written -- this save is about the one document.
            self.apply_wanted(&wanted);
        }
        // The next kind, and then the reader still waiting to be saved.
        self.next_on_save(id.get(), kind + 1);
    }

    /// Hands obelus a filled-in offer worked out against a version of the
    /// document that has been left behind.
    pub fn action_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_action(
            0,
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands obelus what a server offered mid-save, for one of the kinds
    /// a save asks about.
    pub fn saving_for_test(&mut self, kind: usize, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self.current_buffer().map_or(0, Buffer::version);
        self.on_saving(
            id,
            version,
            kind,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// The kinds a save asks about, in order, for a test that walks them.
    #[must_use]
    pub fn kinds_asked_on_save_for_test() -> &'static [&'static str] {
        ON_SAVE
    }

    /// Hands obelus a list of offers, as a server would.
    pub fn actions_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self
            .current_buffer()
            .map_or(0, crate::buffer::Buffer::version);
        self.on_actions(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// The same, about a version of the document that has been left
    /// behind -- which is what a late answer is.
    pub fn actions_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_actions(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }
}

/// Whether two spans touch at all.
fn overlaps(one: crate::coordinates::Span, two: crate::coordinates::Span) -> bool {
    let before = |left: crate::coordinates::Span, right: crate::coordinates::Span| {
        (left.end_line, left.end_column) < (right.line, right.column)
    };
    !before(one, two) && !before(two, one)
}
