//! Offering what could be typed next, and putting it in.
//!
//! The panel is a list beside the cursor, and everything difficult about it
//! is that the reader does not stop typing while it is being asked for. The
//! answer arrives describing a document that has moved on, so what is
//! remembered alongside the question is not the version -- a version that
//! moved by two letters of the same word is still the same question -- but
//! **where the word starts**. If the cursor is still in that word, the
//! answer is still about it and the letters typed since are a narrower
//! query. If it is not, the answer is thrown away: a panel that appears
//! over code the reader has already finished writing is the worst thing
//! this feature can do.

use obelus_component::completion::{Completion, CompletionOutcome};
use obelus_lsp::{complete, snippet};
use obelus_text::coordinates::CharOffset;

use super::*;

/// Whether a character is part of a word Obelus would complete.
///
/// What starts the asking and what ends it. Deliberately not the language's
/// own idea of an identifier: Obelus asks servers about fourteen languages
/// and has no table of what each calls a word, and every one of them agrees
/// about letters, digits and an underscore.
#[must_use]
pub(super) fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Where the word the cursor is in begins.
fn word_start(text: &obelus_text::Text, line: LineNumber, column: CharColumn) -> CharColumn {
    let characters: Vec<char> = text.line(line).chars().take(column.get()).collect();
    let back = characters
        .iter()
        .rev()
        .take_while(|character| is_word(**character))
        .count();
    CharColumn::new(column.get() - back)
}

impl App {
    /// Whether the server for this file says a character is a question.
    ///
    /// Asked of the server rather than of a table here: which punctuation
    /// means "there is something to offer" is a fact about a language, and
    /// Obelus serves fourteen of them.
    fn triggers_completion(&self, character: char) -> bool {
        self.current_buffer()
            .and_then(Buffer::language)
            .and_then(|language| self.servers.get(&language))
            .and_then(Client::capabilities)
            .is_some_and(|capabilities| complete::triggered_by(capabilities, character))
    }

    /// Asks what could be typed where the cursor is, because a key said
    /// so.
    ///
    /// A command, so a reader who dismissed the panel can have it back:
    /// every other way in is typing a letter, and a reader who has pressed
    /// escape is by definition not going to type one.
    ///
    /// The difference from [`App::offer_completion`] is the whole of why
    /// there are two: a key that does nothing says why, and a letter that
    /// finds nothing to offer says nothing at all.
    pub fn ask_completion(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.wrong("No language server for this file".to_string());
            return;
        };
        if let Some(why) = self.why_not_asking(language) {
            self.wrong(why);
            return;
        }
        if !self
            .servers
            .get(&language)
            .and_then(Client::capabilities)
            .is_some_and(complete::supported)
        {
            self.wrong(format!(
                "{} does not offer completions",
                server_named(language)
            ));
            return;
        }
        self.offer_completion();
    }

    /// The same question, asked because a letter was typed.
    pub(super) fn offer_completion(&mut self) {
        let Some(id) = self.current else { return };
        let Some(buffer) = file_in(&self.documents, id) else {
            return;
        };
        // Nothing to complete into: a commit's version of a file, or a file
        // being read rather than written.
        if !buffer.content().is_file() || buffer.mode() != obelus_buffer::Mode::Edit {
            return;
        }
        // A list is already taking every key, so a second one beside the
        // cursor would be a panel nothing can reach.
        if self.layers().any() {
            return;
        }
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = obelus_lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let cursor = buffer.cursor();
        let version = buffer.version();
        let from = (
            cursor.line,
            word_start(buffer.text(), cursor.line, cursor.column),
        );

        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client.capabilities().is_some_and(complete::supported) {
            return;
        }
        let at = position::to_lsp(buffer.text(), cursor.line, cursor.column, client.encoding());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
        });
        if let Ok(request) = client.request("textDocument/completion", &params) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Completion { from },
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Asks what the call the cursor is inside takes.
    ///
    /// The character that asked goes with the question, as the protocol's
    /// `SignatureHelpContext` -- along with whatever is showing, because a
    /// server that knows this is the next comma of a call the reader is
    /// already looking at can keep them on that signature rather than
    /// choosing one again.
    pub(super) fn ask_signature(&mut self, asked: obelus_lsp::signature::Asked) {
        let Some(id) = self.current else { return };
        let Some(buffer) = file_in(&self.documents, id) else {
            return;
        };
        if !buffer.content().is_file() || buffer.mode() != obelus_buffer::Mode::Edit {
            return;
        }
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
        if !client
            .capabilities()
            .is_some_and(obelus_lsp::signature::supported)
        {
            return;
        }
        let at = position::to_lsp(buffer.text(), cursor.line, cursor.column, client.encoding());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
            "context": obelus_lsp::signature::context(
                asked,
                self.signature.as_ref().map(obelus_component::signature::Signature::answer),
            ),
        });
        if let Ok(request) = client.request("textDocument/signatureHelp", &params) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Signature { line: cursor.line },
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Keeps what a server said a call takes, if the reader is still in it.
    ///
    /// And keeps where it is about with it, which is what `settle_signature`
    /// asks every frame afterwards: this checks the answer against the place
    /// it arrives in, and the reader goes on moving after it has arrived.
    pub(super) fn on_signature(&mut self, id: DocumentId, line: LineNumber, reply: Reply) {
        // The line the question was asked on. A call spans one line often
        // enough, and a reader who has gone to another one is writing
        // something else -- a panel about the call above would be a panel
        // about somewhere they have left.
        if self.current != Some(id)
            || self
                .current_buffer()
                .is_none_or(|buffer| buffer.cursor().line != line)
        {
            return;
        }
        self.signature = obelus_lsp::signature::in_reply(&reply.result)
            .map(|answer| obelus_component::signature::Signature::new(answer, id, line));
    }

    /// Keeps the panel honest, once a frame.
    ///
    /// The same shape the completion panel and the hover have: the answer is
    /// checked against the document rather than every way the document can
    /// move being told about the panel. It had neither, so a reader who
    /// arrowed off the line -- or opened another file -- kept a panel about
    /// a call that was no longer under them, until they happened to type a
    /// bracket.
    pub(super) fn settle_signature(&mut self) {
        // A list or a dialog is what the screen is showing, and the panel
        // belongs to the file underneath it.
        if self.layers().any() {
            self.signature = None;
            return;
        }
        let Some((buffer, line)) = self
            .signature
            .as_ref()
            .map(obelus_component::signature::Signature::at)
        else {
            return;
        };
        if self.current != Some(buffer)
            || self
                .current_buffer()
                .is_none_or(|buffer| buffer.cursor().line != line)
        {
            self.signature = None;
        }
    }

    /// The panel's one key.
    ///
    /// Escape gives up on the nearest thing, and while this is showing the
    /// nearest thing is this. Everything else falls through: the reader is
    /// typing arguments, and a key that had to be pressed twice -- once to
    /// dismiss the panel, once to do what it says -- is a key that does
    /// nothing.
    pub(super) fn signature_key(&mut self, key: &KeyEvent) -> bool {
        if self.signature().is_none() {
            return false;
        }
        let Some(modifiers) = keymap::modifiers_of(key) else {
            return false;
        };
        match (modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.signature = None;
                true
            }
            _ => false,
        }
    }

    /// Whether a character is one the server says asks about a call.
    fn triggers_signature(&self, character: char) -> bool {
        self.serving_current().is_some_and(|capabilities| {
            obelus_lsp::signature::triggered_by(capabilities, character)
        })
    }

    /// Whether it is one the server says asks *again*.
    fn retriggers_signature(&self, character: char) -> bool {
        self.serving_current().is_some_and(|capabilities| {
            obelus_lsp::signature::retriggered_by(capabilities, character)
        })
    }

    /// What the server for the file being read says it can do.
    fn serving_current(&self) -> Option<&lsp_types::ServerCapabilities> {
        self.current_buffer()
            .and_then(Buffer::language)
            .and_then(|language| self.servers.get(&language))
            .and_then(Client::capabilities)
    }

    /// Hands the application a signature, as a server would.
    pub fn signature_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            return;
        };
        self.on_signature(
            id,
            line,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands the panel an answer, as a server would.
    ///
    /// The whole path a real answer takes -- the word's start, the query,
    /// the parsing, the filtering -- because those are the parts worth
    /// testing and a server cannot be made to answer on demand.
    pub fn complete_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let Some(buffer) = file_in(&self.documents, id) else {
            return;
        };
        let cursor = buffer.cursor();
        let from = (
            cursor.line,
            word_start(buffer.text(), cursor.line, cursor.column),
        );
        self.on_completion(
            id,
            from,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands the panel an answer to a question asked when the word started
    /// somewhere else -- which is what a late answer is.
    pub fn complete_late_for_test(
        &mut self,
        answer: serde_json::Value,
        line: usize,
        column: usize,
    ) {
        let Some(id) = self.current else { return };
        self.on_completion(
            id,
            (LineNumber::new(line), CharColumn::new(column)),
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands the panel what a resolve answered about one candidate.
    pub fn resolve_for_test(&mut self, index: usize, answer: serde_json::Value) {
        self.on_resolve(
            index,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Whether a snippet is still being filled in.
    #[must_use]
    pub const fn filling_for_test(&self) -> bool {
        self.filling.is_some()
    }

    /// What has been typed since a word started, if the reader is still in
    /// it.
    ///
    /// The one rule the whole feature turns on, in one place: a panel is
    /// about a word, and it stops being about anything the moment the
    /// cursor is somewhere that is not the end of that word. Both the
    /// answer arriving and the frame after every keystroke ask it, and two
    /// implementations of it would be two rules.
    fn typed_since(&self, id: DocumentId, from: (LineNumber, CharColumn)) -> Option<String> {
        if self.current != Some(id) {
            return None;
        }
        let buffer = self
            .file(id)
            .filter(|buffer| buffer.mode() == obelus_buffer::Mode::Edit)?;
        let cursor = buffer.cursor();
        if cursor.line != from.0 || cursor.column.get() < from.1.get() {
            return None;
        }
        // Still the same word, or a different one that happens to start in
        // the same place: either way what stands between them has to be
        // word characters, or the reader has typed past it.
        let query: String = buffer
            .text()
            .line(from.0)
            .chars()
            .skip(from.1.get())
            .take(cursor.column.get() - from.1.get())
            .collect();
        query.chars().all(is_word).then_some(query)
    }

    /// Takes an answer, if the reader is still in the word it is about.
    pub(super) fn on_completion(
        &mut self,
        id: DocumentId,
        from: (LineNumber, CharColumn),
        reply: Reply,
    ) {
        let Some(query) = self.typed_since(id, from) else {
            return;
        };
        let Some(buffer) = file_in(&self.documents, id) else {
            return;
        };
        let language = buffer.language();
        let encoding = language
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let offer = complete::offer_in(&reply.result, buffer.text(), &encoding);
        self.completion = Completion::new(
            id,
            from,
            language.map(obelus_syntax::LanguageId::name),
            offer,
            &query,
        );
    }

    /// Fills in what a resolve added to a candidate.
    pub(super) fn on_resolve(&mut self, index: usize, reply: Reply) {
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
        let text = obelus_text::Text::from_string(&text);
        if let Some(completion) = self.completion.as_mut()
            && let Some(candidate) = completion.candidate_mut(index)
        {
            complete::resolved_into(candidate, &reply.result, &text, &encoding);
        }
    }

    /// Keeps the panel honest, once a frame.
    ///
    /// Everything that can happen to a document while a panel is open
    /// happens somewhere else -- a key that moves the cursor, an undo, a
    /// file re-read under it -- so the panel is checked against the
    /// document rather than told by each of them.
    pub(super) fn settle_completion(&mut self) {
        let Some(completion) = self.completion.as_ref() else {
            return;
        };
        // A list, a dialog or the conversation opened over the file takes
        // every key: a panel under one of those is a panel nothing can
        // reach, drawn over something the reader is using.
        if self.layers().any() {
            self.completion = None;
            return;
        }
        let Some(query) = self.typed_since(completion.buffer(), completion.from()) else {
            self.completion = None;
            return;
        };

        // A server whose list was cut short has to be asked again: what it
        // sent was the best thousand for the query it was asked about, and
        // this is a different query.
        let asking = completion.incomplete() && completion.query() != query;
        if let Some(completion) = self.completion.as_mut()
            && !completion.narrow(&query)
        {
            self.completion = None;
        }
        if asking {
            self.offer_completion();
        }
        self.resolve_chosen();
    }

    /// Asks for the documentation of the candidate the reader is looking at.
    fn resolve_chosen(&mut self) {
        let Some(completion) = self.completion.as_ref() else {
            return;
        };
        let Some(index) = completion.unresolved() else {
            return;
        };
        let Some(id) = self.current else { return };
        let Some(language) = self.file(id).and_then(Buffer::language) else {
            return;
        };
        let Some(params) = completion
            .chosen()
            .map(obelus_lsp::complete::resolve_params)
        else {
            return;
        };
        let version = self.file(id).map_or(0, Buffer::version);
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client
            .capabilities()
            .is_some_and(obelus_lsp::complete::resolves)
        {
            return;
        }
        let Ok(request) = client.request("completionItem/resolve", &params) else {
            return;
        };
        self.remember(
            language,
            request,
            Question {
                asked: Asked::Resolve { index },
                buffer: id,
                version,
            },
        );
        // Counted as resolved now rather than when the answer lands: the
        // question is asked once per candidate, and a server that never
        // answers it would otherwise be asked again on every frame.
        if let Some(completion) = self.completion.as_mut()
            && let Some(candidate) = completion.candidate_mut(index)
        {
            candidate.resolved = true;
        }
    }

    /// The panel's six keys, while it is open.
    pub(super) fn completion_key(&mut self, key: &KeyEvent) -> bool {
        let Some(completion) = self.completion.as_mut() else {
            return false;
        };
        match completion.handle_key(key) {
            CompletionOutcome::Ignored => false,
            CompletionOutcome::Consumed => true,
            CompletionOutcome::Cancelled => {
                self.completion = None;
                true
            }
            CompletionOutcome::Accepted => {
                self.accept_completion();
                true
            }
        }
    }

    /// Puts the chosen candidate in.
    fn accept_completion(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(candidate) = completion.chosen().cloned() else {
            return;
        };
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let cursor = buffer.cursor();
        // What the candidate replaces: what the server said, and otherwise
        // the word the reader is in the middle of. A server knows things
        // Obelus does not -- that `::` is part of the path being completed,
        // that a method call replaces the dot as well -- so its answer wins
        // wherever it gave one.
        let replacing = candidate.replace.unwrap_or(obelus_text::coordinates::Span {
            line: completion.from().0,
            column: completion.from().1,
            end_line: cursor.line,
            end_column: cursor.column,
        });
        let filled = match candidate.snippet {
            true => snippet::parse(&candidate.insert),
            false => snippet::Parsed {
                text: candidate.insert.clone(),
                stops: Vec::new(),
            },
        };

        // Everything in one act: the candidate, and the import a server
        // sent with it. One press of undo takes back both, which is the
        // only sane answer to "what did that key just do to my file".
        let mut edits = vec![(replacing, filled.text.clone())];
        edits.extend(candidate.extra.iter().cloned());
        let Some((at, end)) = self.apply_together(edits) else {
            return;
        };

        // Where the reader is left: filling in the first hole, or after
        // what went in.
        self.filling = snippet::Filling::new(at, &filled.stops);
        if !self.step_snippet(true)
            && let Some(buffer) = self.current_buffer_mut()
        {
            let (line, column) = buffer.text().position(end);
            buffer.place_cursor(line, column);
        }
        // Whatever is typed next is the reader's own, not more of this.
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.settle_undo();
        }

        // A candidate that ends in punctuation the server calls a trigger
        // is a step rather than an answer: `std::` is not a name, it is the
        // start of one, and a reader who chose it means to go on. Not while
        // a snippet is being filled in -- there the cursor is in a hole, and
        // what ends the text is not where the reader is.
        if self.filling.is_none()
            && let Some(last) = filled.text.chars().next_back()
            && self.triggers_completion(last)
        {
            self.offer_completion();
        }

        // And a candidate that put a *call* in is a call the reader is
        // about to fill in: choosing `copy(…)` opens one without anybody
        // typing a bracket, so the keystroke that would have asked never
        // happened.
        //
        // Asked of the text that went in, not of the character behind the
        // caret. Those look like the same question and are not: a real
        // server sends `copy(${1:from}, ${2:to})`, the caret lands on the
        // first hole, and a hole with a default in it is *selected* -- so
        // the caret is at the end of `from` and what is behind it is `m`.
        // Read that way the rule never fired at all, and the test that
        // said it did used a snippet with an empty hole, which is the
        // shape that happens to put the caret against the bracket.
        //
        // Where the caret ends up is the server's business anyway. Obelus
        // needs a reason to ask, and the answer is about wherever the caret
        // is: a candidate that opened no call gets an empty one and no
        // panel.
        if filled
            .text
            .chars()
            .any(|character| self.triggers_signature(character))
        {
            self.ask_signature(obelus_lsp::signature::Asked::Changed);
        }
    }

    /// Makes several edits as one act, and says where the first one left
    /// the document.
    ///
    /// From the end backwards, so that every edit is made against the
    /// coordinates it was written in: a server's `additionalTextEdits`
    /// describe the document as it was, and an import put in first would
    /// move every line the other edits name.
    ///
    /// Returns where the first edit's text begins and ends afterwards --
    /// which is not where it began, because the edits above it have since
    /// pushed it down.
    fn apply_together(
        &mut self,
        edits: Vec<(obelus_text::coordinates::Span, String)>,
    ) -> Option<(CharOffset, CharOffset)> {
        let buffer = self.current_buffer()?;
        let text = buffer.text();
        let mut offsets: Vec<(usize, usize, String)> = edits
            .into_iter()
            .map(|(span, with)| {
                let from = text.char_offset(span.line, span.column).get();
                let to = text.char_offset(span.end_line, span.end_column).get();
                (from.min(to), from.max(to), with)
            })
            .collect();
        let first = offsets.first()?.clone();
        offsets.sort_by_key(|(from, _, _)| std::cmp::Reverse(*from));

        let mut at = first.0;
        let mut end = first.0 + first.2.chars().count();
        let mut started = false;
        for (from, to, with) in offsets {
            let buffer = self.current_buffer()?;
            let (line, column) = buffer.text().position(CharOffset::new(from));
            let (end_line, end_column) = buffer.text().position(CharOffset::new(to));
            let span = obelus_text::coordinates::Span {
                line,
                column,
                end_line,
                end_column,
            };
            // The first edit of the act starts a group of its own and the
            // rest join it, which is what makes the whole thing one undo.
            let doing = match started {
                false => obelus_buffer::undo::Doing::Whole,
                true => obelus_buffer::undo::Doing::Joined,
            };
            started = true;
            self.change(span, &with, doing);
            // An edit before the candidate's own moves it along.
            if from <= at && (from, to) != (first.0, first.1) {
                let moved = with.chars().count() as isize - (to - from) as isize;
                at = at.saturating_add_signed(moved);
                end = end.saturating_add_signed(moved);
            }
        }
        Some((CharOffset::new(at), CharOffset::new(end)))
    }

    /// Moves the snippet's stops across an edit somebody made.
    pub(super) fn keep_filling_across(&mut self, edit: obelus_text::coordinates::Replacement) {
        if let Some(filling) = self.filling.as_mut() {
            filling.keep_across(edit);
        }
    }

    /// `tab` and `shift+tab` while a snippet is being filled in.
    ///
    /// After the panel, which owns both keys while it is open: accepting a
    /// candidate is what put the snippet there, and a reader who has
    /// another panel up is choosing again rather than moving on.
    pub(super) fn snippet_key(&mut self, key: &KeyEvent) -> bool {
        if self.filling.is_none() || self.completion.is_some() {
            return false;
        }
        let Some(modifiers) = keymap::modifiers_of(key) else {
            return false;
        };
        match (modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Tab) => self.step_snippet(true),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.step_snippet(false)
            }
            // Out of the snippet, leaving the text where it is. The next
            // escape is the ordinary one, which clears a selection.
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.filling = None;
                true
            }
            _ => false,
        }
    }

    /// Takes the reader to the next hole in the snippet, or the one before.
    ///
    /// Says whether it did. A `tab` past the last stop is a `tab` again:
    /// the reader has filled the thing in, and a key that stayed captured
    /// for ever would be a key that never indents again.
    fn step_snippet(&mut self, forward: bool) -> bool {
        let Some(filling) = self.filling.as_mut() else {
            return false;
        };
        let stop = match forward {
            true => filling.forward(),
            false => filling.back(),
        };
        let finished = filling.finished();
        let Some((from, to)) = stop else {
            if finished {
                self.filling = None;
            }
            return false;
        };
        let Some(buffer) = self.current_buffer_mut() else {
            return false;
        };
        let (line, column) = buffer.text().position(from);
        let (end_line, end_column) = buffer.text().position(to);
        buffer.place_cursor(line, column);
        // A stop with a default in it is selected, so that typing replaces
        // it -- which is the rule Obelus already has for a selection, and
        // the reason a snippet's defaults are worth putting in at all.
        if (line, column) != (end_line, end_column) {
            buffer.select(obelus_text::coordinates::Span {
                line,
                column,
                end_line,
                end_column,
            });
        }
        // Somewhere new, so the next thing typed is a step of its own to
        // undo.
        buffer.settle_undo();
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_into_view(area);
        }
        true
    }

    /// What to do about the panel after a key put something in the
    /// document.
    ///
    /// Asking is here rather than in the typing itself because it is about
    /// what was typed rather than about the document: a letter is a reason
    /// to ask, and everything else is a reason to stop.
    pub(super) fn after_typing(&mut self, typing: keys::Typing) {
        match typing {
            // More of the same word. What is showing narrows to it now
            // rather than on the next frame, because the next key may be
            // the one that accepts -- and it has to accept a row of the
            // list the reader can see.
            keys::Typing::Character(character) if is_word(character) => {
                self.settle_completion();
                // Nothing left that matches, or nothing was open: either
                // way the server is the only one who can say what a longer
                // word could be.
                if self.completion.is_none() {
                    self.offer_completion();
                }
            }
            // Punctuation the server said means something: `.` and `::`
            // in Rust. The word starts at the cursor, so what comes back
            // is everything that could follow -- unfiltered, because the
            // reader has not typed anything to filter it by yet.
            //
            // Unless the server calls that character a trigger for the
            // *call* as well, which is what `(` is: a character in both of
            // its lists is the reader opening one, and the nearer question
            // there is what the call takes. The unfiltered answer is worth
            // having after `.` and `::`, where it is the members of one
            // thing; after `(` it is everything in scope -- `self::`,
            // `crate::`, every macro -- which is not an answer to "what
            // could be typed next" but another way of saying an expression
            // goes here. Both facts are the server's own, so nothing here
            // is guessing at punctuation.
            keys::Typing::Character(character)
                if self.triggers_completion(character) && !self.triggers_signature(character) =>
            {
                // Whatever was showing was about the word before the
                // punctuation, which has just ended.
                self.completion = None;
                self.offer_completion();
            }
            // Backspace inside the word widens what is showing, and back
            // past the word's start closes it. Both are the settling.
            keys::Typing::Backward => self.settle_completion(),
            // A newline, a bracket, a delete: the reader has moved on.
            _ => self.completion = None,
        }

        // And the same keystroke asked about the call it is in, which is a
        // different question with a different answer: a server may call one
        // character a trigger for both, and `(` usually is.
        match typing {
            keys::Typing::Character(character) if self.triggers_signature(character) => {
                self.ask_signature(obelus_lsp::signature::Asked::Typed(character));
            }
            // A character the server asks to be *re-*asked on, which is
            // rust-analyzer's `)`. Not an ending: the call it closes may be
            // an argument of another one, so the panel goes and the question
            // is asked again -- and an answer about nothing leaves it gone,
            // which is the server saying the reader is in no call at all.
            keys::Typing::Character(character)
                if self.signature.is_some() && self.retriggers_signature(character) =>
            {
                // Asked before the panel goes, in that order: the question
                // hands back what was showing, and a panel cleared first is
                // a question that says nothing was.
                self.ask_signature(obelus_lsp::signature::Asked::Typed(character));
                self.signature = None;
            }
            // A call closing where the server did not name `)` as one of
            // those, or the line ending: either way what is showing is
            // about somewhere the reader has left.
            keys::Typing::Character(')') | keys::Typing::Newline => self.signature = None,
            _ => {}
        }
    }
}
