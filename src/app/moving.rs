//! Going somewhere, and remembering where from.
//!
//! The jump list is [`crate::jump`]; what is here is every way a reader
//! arrives somewhere -- a symbol, a line, a bracket, a change -- and the
//! rule that all of them record where they left.

use super::*;

/// Which column of a line to land on.
enum Column {
    /// The one somebody named, in whichever units they agreed to -- a
    /// language server's answer, or a line number a reader typed.
    Named(u32),
    /// A column of a list's row, which is the line without its indentation.
    InRow(usize),
}

impl App {
    /// Goes to a place a server named, recording where the reader was.
    ///
    /// The position is converted here rather than when the answer arrived,
    /// because converting it needs the target file's text and that file may
    /// never have been opened.
    pub(super) fn go_to(&mut self, path: &Path, line: u32, character: u32) {
        self.go_to_place(path, line, Column::Named(character));
    }

    /// The same, landing where a row's own text matched.
    ///
    /// A search's rows are lines with their indentation trimmed off, so what
    /// the list knows is a column of the *row*: the file's column is that
    /// much further along, and the indent only still exists in the file.
    pub(super) fn go_to_match(&mut self, path: &Path, line: u32, column: usize) {
        self.go_to_place(path, line, Column::InRow(column));
    }

    fn go_to_place(&mut self, path: &Path, line: u32, column: Column) {
        let from = self.here();
        self.open(path);

        let Some(id) = self.current else { return };
        // Before the buffer is borrowed: the area depends on which file is
        // current, which the open above has just settled.
        let area = self.text_area();
        let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::as_mut) else {
            return;
        };
        let (line, column) = match column {
            Column::Named(character) => {
                let encoding = buffer
                    .language()
                    .and_then(|language| self.servers.get(&language))
                    .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                        client.encoding().clone()
                    });
                let at = lsp_types::Position { line, character };
                position::from_lsp(buffer.text(), at, &encoding)
            }
            Column::InRow(column) => {
                let text = buffer.text();
                let line = text.clamp_line(LineNumber::new(line as usize));
                let indent = text
                    .line(line)
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .count();
                (
                    line,
                    text.clamp_column(line, CharColumn::new(indent + column)),
                )
            }
        };
        buffer.place_cursor(line, column);
        buffer.center_on_cursor(area);

        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Records a place in the history, if there was one.
    ///
    /// One line, but it is the line every leap has to remember, and the
    /// three callers each compute `from` before moving.
    pub(super) fn record(&mut self, from: Option<Jump>) {
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Where the cursor is, for the history.
    pub(super) fn here(&self) -> Option<Jump> {
        let id = self.current?;
        let cursor = self.buffers.get(id.get())?.as_ref()?.cursor();
        Some(Jump {
            buffer: id,
            line: cursor.line,
            column: cursor.column,
        })
    }

    /// Returns to where the last jump started.
    pub fn go_back(&mut self) {
        let Some(here) = self.here() else { return };
        match self.jumps.back(here) {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further back".to_string()),
        }
    }

    /// Undoes a jump back.
    pub fn go_forward(&mut self) {
        match self.jumps.forward() {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further forward".to_string()),
        }
    }

    fn go(&mut self, to: Jump) {
        if to.buffer.get() >= self.buffers.len() {
            return;
        }
        self.go_to_buffer(to.buffer);
        let area = self.text_area();
        if let Some(buffer) = self
            .buffers
            .get_mut(to.buffer.get())
            .and_then(Option::as_mut)
        {
            buffer.place_cursor(to.line, to.column);
            // Arriving, like the jump that led here: the line the reader left
            // deserves its context as much as the definition did.
            buffer.center_on_cursor(area);
        }
    }

    /// Goes to the bracket that matches the one under the cursor.
    ///
    /// Over the whole file rather than what is on screen: the partner being
    /// off screen is the case worth having a key for. That costs one pass of
    /// highlighting over the file, because the scan has to know which
    /// brackets are inside strings and comments -- `"("` is not an unclosed
    /// bracket -- and highlights are otherwise only computed for what is
    /// visible. One pass on a keystroke, not per frame.
    ///
    /// Not recorded in the jump list. It is a motion within one expression,
    /// and a history filled with bracket hops is a history you cannot use to
    /// get back to where you were reading.
    pub fn go_to_bracket(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.note = Some("no file open".to_string());
            return;
        };
        let Some(state) = buffer.syntax() else {
            self.note = Some("obelus does not know this language".to_string());
            return;
        };
        let text = buffer.text();
        let cursor = buffer.cursor();
        let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));

        let whole = ByteOffset::new(0)..text.byte_length();
        let mut highlights = Highlights::default();
        highlights.refresh(state, text, whole.clone());
        let Some((open, close)) = brackets::pair_at(text, &highlights, at, whole) else {
            self.note = Some("no bracket here".to_string());
            return;
        };

        let partner = if open == at { close } else { open };
        let (line, column) = text.position(text.char_of_byte(partner));
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.place_cursor(line, column);
            // Centred only if it was somewhere else entirely. A partner on
            // screen is a short hop, and moving the whole view for it would
            // throw away the reader's place to show them something they were
            // already looking at.
            if buffer.cursor_screen_cell(area).is_none() {
                buffer.center_on_cursor(area);
            }
        }
    }

    /// Asks for a line number.
    ///
    /// A prompt with no rows: there is nothing to list, and a list of every
    /// line in the file would be the file. The picker takes the query as the
    /// answer, which is the shape searching a file will want too.
    pub fn open_line_prompt(&mut self) {
        if self.current_buffer().is_none() {
            self.note = Some("no file to go into".to_string());
            return;
        }
        self.prompt = Some(Prompt::new(PromptKind::Line));
    }

    /// Stops selecting, leaving the cursor where it is.
    ///
    /// What escape does at the file itself. Silent when there is nothing
    /// selected: escape meaning "never mind" is not worth a note when there
    /// was nothing to mind.
    pub fn clear_selection(&mut self) {
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.clear_selection();
        }
    }

    /// Selects the whole file.
    ///
    /// The cursor lands at the end of it, because the cursor is one end of
    /// a selection -- and the view follows, which is the frame's own job:
    /// what the reader has just taken all of ends there.
    pub fn select_all(&mut self) {
        let Some(buffer) = self.current_buffer_mut() else {
            self.note = Some("no file open".to_string());
            return;
        };
        buffer.select_all();
    }

    /// Copies the selected text to the system clipboard.
    pub fn copy_selection(&mut self) {
        let Some(text) = self.current_buffer().and_then(Buffer::selected_text) else {
            self.note = Some("nothing selected".to_string());
            return;
        };

        // Handed to the terminal, which owns it from here: that is what
        // makes the copy outlive obelus and what makes it work over ssh. A
        // terminal that does not implement the sequence copies nothing and
        // cannot say so, so the note reports what obelus did rather than
        // what the terminal did with it.
        match crate::clipboard::copy(&text) {
            Ok(()) => self.note = Some("copied selection".to_string()),
            Err(error) => {
                tracing::warn!(%error, "copying the selection failed");
                self.note = Some("could not copy selection".to_string());
            }
        }
    }

    /// The question being asked, if one is.
    #[must_use]
    pub const fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// Acts on an answered prompt.
    ///
    /// Where "what it means" lives: the prompt knows what was typed and what
    /// kind of question it was, and nothing more. A line past the end
    /// of the file is clamped rather than refused -- `9999` in a short file
    /// means the end of it -- and a line number that is not a number is a
    /// note, because it is the reader's slip and not the file's.
    pub(super) fn answer(&mut self, kind: PromptKind, text: &str) {
        match kind {
            PromptKind::Line => {
                let Ok(line) = text.trim().parse::<usize>() else {
                    self.note = Some(format!("{text:?} is not a line number"));
                    return;
                };
                let from = self.here();
                let area = self.text_area();
                if let Some(buffer) = self.current_buffer_mut() {
                    // One-based on the way in, because that is what every
                    // other tool prints and what the status bar shows.
                    buffer
                        .place_cursor(LineNumber::new(line.saturating_sub(1)), CharColumn::new(0));
                    buffer.center_on_cursor(area);
                }
                // A jump, so `go-back` comes back: typing a line number is
                // exactly the kind of leap the history is for.
                if let Some(from) = from {
                    self.jumps.push(from);
                }
            }
        }
    }
}
