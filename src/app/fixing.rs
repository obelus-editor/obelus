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

        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client.capabilities().is_some_and(actions::supported) {
            self.note = Some(match lsp::command_for(language) {
                Some(command) => format!("{command} offers nothing to do here"),
                None => "no language server for this file".to_string(),
            });
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
    pub(super) fn on_actions(&mut self, reply: Reply) {
        let offered = actions::offered_in(&reply.result);
        if offered.is_empty() {
            self.note = Some("nothing to do here".to_string());
            return;
        }
        let items = offered
            .iter()
            .enumerate()
            .map(|(at, action)| PickerItem {
                icon: icons::enabled().then_some(icons::for_command(Command::SymbolActions)),
                label: action.title.clone(),
                // No detail. The only thing to put there is the kind,
                // which is the protocol's own word for its own filing --
                // `refactor.rewrite`, `quickfix` -- and it tells a reader
                // choosing between two offers nothing the titles have not
                // already said.
                detail: None,
                prose: true,
                marker: None,
                trailing: None,
                changed: None,
                value: PickerValue::Action(at),
                depth: 0,
                status: None,
                enabled: true,
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
        self.picker = Some(picker);
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
    pub(super) fn on_action(&mut self, at: usize, reply: Reply) {
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

    /// Hands obelus a list of offers, as a server would.
    pub fn actions_for_test(&mut self, answer: serde_json::Value) {
        self.on_actions(Reply {
            id: 0,
            result: Ok(answer),
        });
    }
}

/// Whether two spans touch at all.
fn overlaps(one: crate::coordinates::Span, two: crate::coordinates::Span) -> bool {
    let before = |left: crate::coordinates::Span, right: crate::coordinates::Span| {
        (left.end_line, left.end_column) < (right.line, right.column)
    };
    !before(one, two) && !before(two, one)
}
