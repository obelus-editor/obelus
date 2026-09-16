//! Where else the name under the caret is used.
//!
//! Asked by the pointer coming to rest on a name, which is the same
//! gesture that asks what the name *is*: a reader who points at something
//! is asking about it, and the two answers -- what it is, and where else
//! it is -- belong to one question.
//!
//! Not by the caret. The caret moves with every arrow key, and a file
//! whose colours changed under every keystroke would be a file nobody can
//! read: the mark would be following the reader rather than answering
//! them.

use super::*;

impl App {
    /// The runs the editor marks: the uses of the name the pointer is
    /// resting on.
    ///
    /// Or what a hover is about, where one is up. Both are "this is what
    /// is being talked about", the mark says exactly that, and two marks
    /// at once would be two claims a reader has to tell apart by colour.
    #[must_use]
    pub fn marked_runs(&self) -> &[crate::coordinates::Span] {
        match self.hover().is_some() {
            true => self.hovered_range(),
            false => &self.uses,
        }
    }

    /// Whether a place in the document is inside a name, as the tree sees
    /// it.
    ///
    /// The same judgement the symbol menu makes before it offers to ask a
    /// server anything, and for the same reason: a question about a comma
    /// is a round trip for an empty answer.
    pub(super) fn a_name_at(&self, at: (LineNumber, CharColumn)) -> bool {
        let Some(buffer) = self.current_buffer() else {
            return false;
        };
        let Some(state) = buffer.syntax() else {
            // No grammar for this language: the server is the only one who
            // knows, so it is asked.
            return true;
        };
        let text = buffer.text();
        let byte = text.byte_of_char(text.char_offset(at.0, at.1));
        state.is_name_at(text, byte)
    }

    /// Forgets the marks, for a pointer that has moved off what they were
    /// about.
    pub(super) fn forget_uses(&mut self) {
        self.uses.clear();
    }

    /// Sends the question, about the place the pointer is resting on.
    pub(super) fn ask_uses_at(&mut self, at: (LineNumber, CharColumn)) {
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
        let version = buffer.version();
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client
            .capabilities()
            .is_some_and(crate::lsp::uses::supported)
        {
            return;
        }
        let at = position::to_lsp(buffer.text(), at.0, at.1, client.encoding());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
        });
        if let Ok(request) = client.request("textDocument/documentHighlight", &params) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Uses,
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Keeps what a server said, if it is still about what is on screen.
    pub(super) fn on_uses(&mut self, id: BufferId, version: i32, reply: Reply) {
        // The document it was asked about, unchanged since: these are
        // places in a text, and a text that has moved has moved them.
        let still = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .is_some_and(|buffer| buffer.version() == version);
        if self.current != Some(id) || !still {
            return;
        }
        let Some(buffer) = self.buffers.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        let encoding = buffer
            .language()
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        self.uses = crate::lsp::uses::in_reply(&reply.result, buffer.text(), &encoding);
    }

    /// How many uses are marked.
    ///
    /// For a test that has to tell them from the other thing the same
    /// mark is used for: while an answer about a place is up, what is
    /// marked is what *that* is about.
    #[must_use]
    pub fn uses_marked_for_test(&self) -> usize {
        self.uses.len()
    }

    /// Hands obelus an answer about a version of the document that has
    /// been left behind, which is what a late answer is.
    pub fn uses_at_version_for_test(&mut self, answer: serde_json::Value, version: i32) {
        let Some(id) = self.current else { return };
        self.on_uses(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands obelus an answer, as a server would.
    pub fn uses_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let version = self
            .current_buffer()
            .map_or(0, crate::buffer::Buffer::version);
        self.on_uses(
            id,
            version,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }
}
