//! Renaming a symbol, everywhere it is.
//!
//! The one thing a language server does that a reader cannot do by hand:
//! a name is used in files they have not opened, and a search and replace
//! over the text would catch the word in a comment and in a string and in
//! somebody else's identifier that happens to contain it.
//!
//! Asked in two halves -- what to call it, then the server -- because the
//! question Obelus asks the reader is the only part it knows: what the
//! rename *touches* is the server's answer, and it arrives after.

use obelus_component::prompt::PromptKind;
use obelus_lsp::edits;

use crate::app::*;

impl App {
    /// Asks what to call the symbol under the cursor.
    ///
    /// The prompt opens holding the name as it is, which is what a rename
    /// usually edits rather than replaces.
    pub fn rename_symbol(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.wrong("No file open".to_string());
            return;
        };
        if !buffer.content().is_file() || buffer.mode() != obelus_buffer::Mode::Edit {
            self.wrong("This is not a file to change".to_string());
            return;
        }
        let Some(language) = buffer.language() else {
            self.wrong("No language server for this file".to_string());
            return;
        };
        if let Some(why) = self.why_not_asking(language) {
            self.wrong(why);
            return;
        }
        let renames = self
            .servers
            .get(&language)
            .and_then(Client::capabilities)
            .is_some_and(|capabilities| capabilities.rename_provider.is_some());
        if !renames {
            self.wrong(format!("{} does not rename", server_named(language)));
            return;
        }
        // The name the caret is in, which is what is being renamed: a
        // rename asked for on a comma is a question the server will refuse,
        // and the refusal arrives a round trip later.
        let Some(word) = self.word_at_cursor() else {
            self.wrong("The cursor is not on a name".to_string());
            return;
        };
        self.ask_on_the_status_row(obelus_component::prompt::Prompt::about(
            PromptKind::Name,
            word,
        ));
    }

    /// The name the caret is in, as text.
    fn word_at_cursor(&self) -> Option<String> {
        let buffer = self.current_buffer()?;
        let cursor = buffer.cursor();
        let (from, to) = obelus_editing::word_around(buffer.text(), cursor.line, cursor.column)?;
        Some(
            buffer
                .text()
                .line(cursor.line)
                .chars()
                .skip(from.get())
                .take(to.since(from))
                .collect(),
        )
    }

    /// Asks the server to rename it.
    pub(in crate::app) fn ask_rename(&mut self, name: &str) {
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
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
            "newName": name,
        });
        match client.request("textDocument/rename", &params) {
            Ok(request) => {
                self.remember(
                    language,
                    request,
                    Question {
                        asked: Asked::Rename,
                        buffer: id,
                        version,
                    },
                );
                self.say(format!("Renaming to {name}\u{2026}"));
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask for a rename");
                self.wrong("The language server is not listening".to_string());
            }
        }
    }

    /// Makes the rename the server worked out.
    ///
    /// Refused where the document has moved since: the places it names are
    /// places in the file as it was, and a file that has been typed into is
    /// not that file. A reader whose rename is refused can ask again, where
    /// one whose rename lands two characters out has a mess to find.
    pub(in crate::app) fn on_rename(&mut self, id: DocumentId, version: i32, reply: Reply) {
        if !self.unmoved(id, version) {
            self.wrong("The file changed while renaming".to_string());
            return;
        }
        let result = match reply.result {
            Ok(result) => result,
            Err(why) => {
                self.wrong(why);
                return;
            }
        };
        let wanted = edits::wanted_in(&result);
        if wanted.is_empty() && wanted.refused.is_empty() {
            self.wrong("The server renamed nothing".to_string());
            return;
        }
        let said = self.apply_wanted(&wanted);
        // Empty is a rename the server asked for that came to nothing --
        // see `make_asked_edit`, which tells the two apart by the same bit.
        match wanted.is_empty() {
            true => self.wrong(said),
            false => self.say(said),
        }
    }

    /// Hands Obelus an answer worked out against a version of the file
    /// that has been left behind.
    pub fn rename_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_rename(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands Obelus a server's answer, as one would arrive.
    pub fn rename_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self.current_buffer().map_or(0, Buffer::version);
        self.on_rename(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }
}
