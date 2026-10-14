//! Going somewhere, and remembering where from.
//!
//! The jump list is [`crate::jump`]; what is here is every way a reader
//! arrives somewhere -- a symbol, a line, a bracket, a change -- and the
//! rule that all of them record where they left.

use obelus_component::spelling::Spells;

use crate::app::*;

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
    pub(in crate::app) fn go_to(&mut self, path: &Path, line: u32, character: u32) {
        self.go_to_place(path, line, Column::Named(character));
    }

    /// The same, landing where a row's own text matched.
    ///
    /// A search's rows are lines with their indentation trimmed off, so what
    /// the list knows is a column of the *row*: the file's column is that
    /// much further along, and the indent only still exists in the file.
    pub(in crate::app) fn go_to_match(&mut self, path: &Path, line: u32, column: usize) {
        self.go_to_place(path, line, Column::InRow(column));
    }

    fn go_to_place(&mut self, path: &Path, line: u32, column: Column) {
        let from = self.here();
        self.open(path);
        // A file that would not open leaves the reader where they were --
        // it has gone away, or it is not theirs to read -- and where they
        // were is a file of their own. Without this the cursor would move
        // in *their* file to a line from somebody else's, which is the
        // worst of both: nothing was opened and something was lost.
        if self
            .current_buffer()
            .is_none_or(|buffer| buffer.path() != path)
        {
            self.wrong(format!("Could not open {}", path.display()));
            return;
        }

        let Some(id) = self.current else { return };
        // Before the buffer is borrowed: the area depends on which file is
        // current, which the open above has just settled.
        let area = self.text_area();
        let Some(buffer) = file_in_mut(&mut self.documents, id) else {
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
    pub(in crate::app) fn record(&mut self, from: Option<Jump>) {
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Where the reader is, for the history.
    ///
    /// Any document, not only a file. It asked for a file, so standing in a
    /// conversation gave `None` -- and every caller starts with this, so
    /// leaving a conversation for a file recorded nothing and `alt+left` was
    /// a key that was offered and did not work.
    pub(in crate::app) fn here(&self) -> Option<Jump> {
        let id = self.current?;
        Some(Jump {
            document: id,
            at: self
                .file(id)
                .map(|buffer| (buffer.cursor().line, buffer.cursor().column)),
        })
    }

    /// Returns to where the last jump started.
    pub fn go_back(&mut self) {
        let Some(here) = self.here() else { return };
        match self.jumps.back(here) {
            Some(there) => self.go(there),
            None => self.wrong("Nowhere further back".to_string()),
        }
    }

    /// Undoes a jump back.
    pub fn go_forward(&mut self) {
        match self.jumps.forward() {
            Some(there) => self.go(there),
            None => self.wrong("Nowhere further forward".to_string()),
        }
    }

    fn go(&mut self, to: Jump) {
        if to.document.get() >= self.documents.len() {
            return;
        }
        // Whatever it names, because a conversation is somewhere the reader
        // was: going back to one is going back to it, and the cursor part
        // simply has nothing to say.
        self.go_to_document(to.document);
        let area = self.text_area();
        if let Some((line, column)) = to.at
            && let Some(buffer) = self.file_mut(to.document)
        {
            buffer.place_cursor(line, column);
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
            self.wrong("No file open".to_string());
            return;
        };
        let Some(state) = buffer.syntax() else {
            self.wrong("Not a language Obelus knows".to_string());
            return;
        };
        let text = buffer.text();
        let cursor = buffer.cursor();
        let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));

        let whole = ByteOffset::new(0)..text.byte_length();
        let mut highlights = Highlights::default();
        highlights.refresh(state, text, whole.clone());
        let Some((open, close)) = brackets::pair_at(text, &highlights, at, whole) else {
            self.wrong("No bracket here".to_string());
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
            self.wrong("No file to go into".to_string());
            return;
        }
        self.ask_on_the_status_row(Prompt::new(PromptKind::Line));
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
    ///
    /// Or all of whatever box is in front, asked in the order a copy is:
    /// `ctrl+a` in a question on the status row took the whole of the file
    /// behind it, and everywhere else a reader types it did nothing.
    pub fn select_all(&mut self) {
        match self.layers().nearest() {
            Some(Layer::Prompt) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.hold(true);
                }
                return;
            }
            Some(Layer::Settings) => {
                if let Some(settings) = self.settings.as_mut() {
                    settings.hold_in_query(true);
                }
                return;
            }
            Some(Layer::Picker) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.hold_in_query(true);
                }
                return;
            }
            Some(Layer::Names) => {
                if let Some((_, names)) = self.names.as_mut() {
                    names.hold_in_query(true);
                }
                return;
            }
            Some(Layer::Counts | Layer::Gone) => return,
            None if self.which_project.chooser.is_some() => {
                if let Some(chooser) = self.which_project.chooser.as_mut() {
                    chooser.hold_in_typing(true);
                }
                return;
            }
            None => {}
        }
        if self.conversation().is_some() {
            let area = self.editor_area;
            if let Some(card) = self.card() {
                let width =
                    obelus_ui::card::width_of(obelus_ui::chat::bands_for(area, card).writing);
                if let Some(card) = self
                    .conversation_mut()
                    .and_then(|talk| talk.card_in_front_mut())
                {
                    card.select_all(width);
                }
                return;
            }
            let width = obelus_ui::chat::writing_width(area);
            self.in_transcript(|chat| {
                // One selection between the two halves, so taking all of
                // the box lets the transcript go.
                chat.let_go();
                chat.writing_mut().select_all(width);
            });
            return;
        }
        if let Some(notes) = self.notes_mut() {
            notes.select_all();
            return;
        }
        let Some(buffer) = self.current_buffer_mut() else {
            self.wrong("No file open".to_string());
            return;
        };
        buffer.select_all();
    }

    /// Widens what is selected by one step.
    ///
    /// The grammar's own steps, so what the key selects is a thing rather
    /// than a number of characters: the word, the argument it is, the call
    /// it is in, the statement, the block. `clear-selection` is the way
    /// back out, because a stack of what was selected before would be a
    /// stack to keep right across every edit.
    pub fn widen_selection(&mut self) {
        // Whatever the tree still owes: this is a question about what the
        // text *means*, asked by a reader who is about to act on the
        // answer.
        self.settle_syntax();
        let Some(buffer) = self.current_buffer_mut() else {
            self.wrong("No file open".to_string());
            return;
        };
        if !buffer.widen_selection() {
            self.wrong("Nothing wider to select".to_string());
        }
    }

    /// Copies the selected text to the system clipboard.
    pub fn copy_selection(&mut self) {
        // Out of whatever is in front, the same answer a paste and a key
        // get. A box a reader can select in but not copy out of is a box
        // with half a selection -- and before this, `ctrl+c` over a list
        // copied the line of the file behind it, and over the counts it
        // still did, because a chain of the boxes that *had* a selection
        // left out the views that had none.
        //
        // The notes are not here because they answer these two keys
        // themselves.
        match self.layers().nearest() {
            Some(Layer::Prompt) => {
                if let Some(prompt) = self.prompt.as_ref() {
                    let (text, what) = prompt.copied();
                    self.copied(&text, what);
                }
                return;
            }
            Some(Layer::Settings) => {
                if let Some(settings) = self.settings.as_ref() {
                    let (text, what) = settings.copy_query();
                    self.copied(&text, what);
                }
                return;
            }
            Some(Layer::Picker) => {
                if let Some(picker) = self.picker.as_ref() {
                    let (text, what) = picker.copy_query();
                    self.copied(&text, what);
                }
                return;
            }
            Some(Layer::Names) => {
                if let Some((_, names)) = self.names.as_ref() {
                    let (text, what) = names.copy_query();
                    self.copied(&text, what);
                }
                return;
            }
            Some(Layer::Counts | Layer::Gone) => return,
            // Being asked which project, which is a page of its own and not
            // a layer, with a box on it like any other.
            None if self.which_project.chooser.is_some() => {
                self.copy_from_the_chooser();
                return;
            }
            None => {}
        }
        // What is held in a terminal, which is the one thing in it to copy:
        // the program's words are not a buffer, and there is no line under
        // a caret to take in their place.
        if self.terminal().is_some() {
            if let Some(text) = self.take_what_is_held_in_the_terminal() {
                self.copied(&text, "selection");
            }
            return;
        }
        let width = obelus_ui::chat::reading_width(self.editor_area);
        // What is held on a card, which covers the box a message is
        // written in: before this, a copy over a card took what was in the
        // box nobody could see. What is held in the transcript above it
        // still comes first where the card holds nothing.
        if let Some(card) = self.card() {
            let transcript = self.chat().and_then(|chat| chat.held_text(width));
            let copied = match (card.selected(), transcript) {
                (Some(held), _) => Some((held, "selection")),
                (None, Some(transcript)) => Some((transcript, "selection")),
                (None, None) => card.copied(),
            };
            if let Some((text, what)) = copied {
                self.copied(&text, what);
            }
            return;
        }
        if let Some((text, what)) = self.chat().map(|chat| chat.copied(width)) {
            self.copied(&text, what);
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // The line the cursor is on where nothing is selected, newline and
        // all -- so that what comes back out of the clipboard is a line
        // rather than the middle of one. Copying nothing is not something a
        // key can usefully do, and selecting the line first to copy it is a
        // step every editor spares the reader.
        let (text, what) = match buffer.selected_text() {
            Some(text) => (text, "selection"),
            None => (
                buffer.text().line(buffer.cursor().line).to_string() + "\n",
                "line",
            ),
        };

        self.copied(&text, what);
    }

    /// Hands text to the clipboard and says what the reader got.
    ///
    /// Handed to the terminal, which owns it from here: that is what makes
    /// the copy outlive Obelus and what makes it work over ssh. A terminal
    /// that does not implement the sequence copies nothing and cannot say
    /// so, so the note reports what Obelus did rather than what the terminal
    /// did with it.
    ///
    /// Shared with the notes, because a copy is a copy: the file and the box
    /// a note is written in say the same thing about it or a reader learns
    /// that one of them is lying.
    pub(in crate::app) fn copied(&mut self, text: &str, what: &str) {
        match obelus_clipboard::copy(text) {
            Ok(()) => self.say(format!("Copied {what}")),
            Err(error) => {
                tracing::warn!(%error, what, "copying failed");
                self.wrong(format!("Could not copy {what}"));
            }
        }
    }

    /// The same for text that is being taken out as well as copied.
    ///
    /// A different thing to say when the clipboard refuses, because a
    /// different thing happened: the text has gone from where it was either
    /// way, and a reader told only that the copy failed would go looking
    /// for it where it no longer is.
    pub(in crate::app) fn cut_away(&mut self, text: &str, what: &str) {
        match obelus_clipboard::copy(text) {
            Ok(()) => self.say(format!("Cut {what}")),
            Err(error) => {
                tracing::warn!(%error, what, "copying the cut failed");
                self.wrong(format!("Cut {what}, but could not copy it"));
            }
        }
    }

    /// Copies the selection and takes it out of the document.
    ///
    /// The selection in the *file*, not the one in an opened hunk. A hunk's
    /// rows are lines the file no longer has, and cutting them would be
    /// cutting from a diff.
    pub fn cut_selection(&mut self) {
        // The same answer, and the same reason with more at stake: before
        // this, `ctrl+x` over a list took a line out of the file behind it,
        // where nobody could see it go -- and over the counts it still did.
        match self.layers().nearest() {
            Some(Layer::Prompt) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    let (text, what) = prompt.cut();
                    self.cut_away(&text, what);
                }
                return;
            }
            Some(Layer::Settings) => {
                let offering = self.agent_offering();
                if let Some(settings) = self.settings.as_mut() {
                    let (text, what) = settings.cut_query(offering.as_ref());
                    self.cut_away(&text, what);
                }
                return;
            }
            Some(Layer::Picker) => {
                if let Some(picker) = self.picker.as_mut() {
                    let (text, what) = picker.cut_query();
                    let searching = picker.is_searching();
                    self.cut_away(&text, what);
                    if searching {
                        self.refresh_search();
                    }
                }
                return;
            }
            Some(Layer::Names) => {
                if let Some((_, names)) = self.names.as_mut() {
                    let (text, what) = names.cut_query();
                    self.cut_away(&text, what);
                }
                return;
            }
            Some(Layer::Counts | Layer::Gone) => return,
            None if self.which_project.chooser.is_some() => {
                self.cut_from_the_chooser();
                return;
            }
            None => {}
        }
        // Out of the card's words where a card is up, because it covers
        // the box: a cut from under it took words nobody could see go.
        if let Some(card) = self.card() {
            let width = obelus_ui::card::width_of(
                obelus_ui::chat::bands_for(self.editor_area, card).writing,
            );
            if let Some((text, what)) = self
                .conversation_mut()
                .and_then(|talk| talk.card_in_front_mut())
                .and_then(|card| card.cut(width))
            {
                self.cut_away(&text, what);
            }
            return;
        }
        if self.conversation().is_some() {
            // The width the box really has, from the same function the
            // view lays it out with: a cut is over a selection, and where
            // a selection ends was decided by where the rows wrap.
            let room = obelus_ui::chat::writing_width(self.editor_area);
            if let Some((text, what)) = self.conversation_mut().map(|talk| talk.chat.cut(room)) {
                self.cut_away(&text, what);
            }
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // The whole line where nothing is selected, and the line break with
        // it: a cut line has to leave, not leave a blank behind.
        let (span, what) = match buffer.selection() {
            Some(span) => (span, "selection"),
            None => {
                let line = buffer.cursor().line;
                let text = buffer.text();
                let (end_line, end_column) = match line >= text.last_line() {
                    true => (line, text.line_length(line)),
                    false => (line.saturating_add(1), CharColumn::new(0)),
                };
                (
                    Span {
                        line,
                        column: CharColumn::new(0),
                        end_line,
                        end_column,
                    },
                    "line",
                )
            }
        };
        let Some(text) = self
            .current_buffer()
            .map(|buffer| buffer.text().text_in(span))
        else {
            return;
        };
        self.cut_away(&text, what);
        self.change(span, "", obelus_buffer::undo::Doing::Whole);
    }

    /// Puts back what was last copied or cut.
    ///
    /// From wherever the clipboard is -- an outside program's, or Obelus's
    /// own where that cannot be read. A selection is what it replaces,
    /// because a reader who selected something and pasted meant to.
    pub fn paste(&mut self) {
        // A picture first, and only where it has somewhere to go: the box a
        // message is written in is the one place in Obelus that can carry
        // one, and an agent that did not say it takes them would answer
        // about the words around it and say nothing about the picture --
        // which looks to a reader exactly like the picture arriving and
        // being ignored. So the question is asked before the paste is
        // offered rather than after it fails, and where the answer is no
        // the words on the clipboard are pasted as they always were.
        if self.can_take_a_picture()
            && let Some((mime, bytes)) = obelus_clipboard::picture()
        {
            // The width the box wraps to, taken from the same function the
            // view lays itself out with -- see `App::chat_key`.
            let room = obelus_ui::chat::writing_width(self.editor_area);
            if let Some(talk) = self.conversation_mut() {
                talk.chat
                    .writing_mut()
                    .attach(obelus_component::composer::Attached { mime, bytes }, room);
            }
            return;
        }
        let Some(what) = obelus_clipboard::paste() else {
            // A picture and nowhere for it is not nothing, and what is
            // said is which of the two ways to fix it this is: the agent
            // said a prompt to it cannot carry one, or the box is not what
            // is in front -- no conversation, or a card or a list over it,
            // each of which the reader fixes by getting to the box. "Nothing
            // to paste" sent a reader to look at their clipboard, which is
            // the one place the answer is not.
            if obelus_clipboard::holds_a_picture() {
                self.wrong(match self.box_takes_a_picture() {
                    // Only reachable once the agent has said so: while
                    // nobody has asked it, a picture is taken.
                    true => "This agent does not take pictures".to_string(),
                    false => "A picture goes in a message to an agent".to_string(),
                });
                return;
            }
            self.wrong("Nothing to paste".to_string());
            return;
        };
        // Not the word a selection was taken out for: that arrives as an
        // input method's commit, and one given up with escape never does.
        self.spelled_over = None;
        self.paste_text(&what);
    }

    /// Whether a picture pasted now would have somewhere to go and somebody
    /// to read it.
    ///
    /// Two questions that are one: the box has to be in front of the
    /// reader, and the agent has to have said in the handshake that a
    /// prompt may carry a picture. The second is what `Carries::image` was
    /// put there for.
    #[must_use]
    fn can_take_a_picture(&self) -> bool {
        self.box_takes_a_picture() && self.agent_takes_pictures()
    }

    /// Whether the box a message is written in is where the keys are, with
    /// nothing over it.
    ///
    /// Not [`App::conversation_takes_text`], which says yes to a card with
    /// room for the reader's own words, and does not ask about a list open
    /// over the conversation at all: an answer to a question carries words
    /// and nothing else, and a list's query carries less, so a picture
    /// pasted on either went into the box behind it, where nobody could see
    /// it arrive. The layers are asked the way [`App::dropped`] asks them.
    #[must_use]
    fn box_takes_a_picture(&self) -> bool {
        self.layers().nearest().is_none()
            && self.which_project.chooser.is_none()
            && self
                .conversation()
                .is_some_and(|talk| talk.card_in_front().is_none())
            && self.conversation_takes_text()
    }

    /// Whether the agent has not said a prompt to it cannot carry a picture.
    #[must_use]
    pub(in crate::app) fn agent_takes_pictures(&self) -> bool {
        match self
            .agent
            .talker
            .as_ref()
            .and_then(obelus_agent::acp::Talk::carries)
        {
            // It has said, so the answer is what it said.
            Some(carries) => carries.image,
            // Nobody has asked it anything yet, which is where a reader
            // pasting a picture usually is: opening a conversation starts
            // no process, so the first thing they do is put the picture in
            // and then type the question about it. Taken, because the
            // alternative is refusing them over a handshake that has not
            // happened -- and by the time the prompt actually goes the
            // agent has started and said.
            None => true,
        }
    }

    /// Puts a run of text in where the reader is.
    ///
    /// What the terminal's own paste arrives as, and what the clipboard
    /// hands back. One change either way, so undoing it is one step.
    /// Whether there is a box a reader is typing into.
    ///
    /// The same places [`App::paste_text`] puts a paste, named once so the
    /// key that is offered and the place it would go cannot come apart: a
    /// key offered with nowhere to act is a key that does nothing, and one
    /// refused over a box the reader is looking at is worse.
    #[must_use]
    pub(in crate::app) fn somewhere_to_type(&self) -> bool {
        self.prompt.is_some()
            || self.which_project.chooser.is_some()
            || self.notes().is_some()
            || self.settings.is_some()
            || self.picker.is_some()
            || self.names.is_some()
            || self.conversation_takes_text()
            || self.typing_to_a_program()
    }

    /// Whether the conversation being read has somewhere text can go.
    ///
    /// Not "is a conversation open": the box is the place, and the keys are
    /// not always in it. A card covers the box, so while one is up the
    /// question is whether the agent left room to answer in the reader's own
    /// words; and the row of settings under the box takes the keys away from
    /// it. Either way the answer is no, and a key offered on a no is a key
    /// whose text goes somewhere nobody can see.
    #[must_use]
    pub(in crate::app) fn conversation_takes_text(&self) -> bool {
        let Some(talk) = self.conversation() else {
            return false;
        };
        match talk.card_in_front() {
            Some(card) => card.takes_words(),
            None => talk.chat.focus() == obelus_component::chat::Focus::Writing,
        }
    }

    /// Whether a character typed now would go into some text.
    ///
    /// What a window turns an input method on and off by: spelling a word
    /// where nothing takes it is keys swallowed and a list of candidates
    /// for nowhere. Not where the caret is, which is a different question
    /// with two different answers -- an empty filter has no caret and
    /// takes the first letter typed. An input method turned on by that
    /// first letter has already missed it. And the other way about: a
    /// conversation's transcript has a caret, and a letter typed there goes
    /// nowhere, so the input method is off.
    ///
    /// The layers in [`App::paste_text`]'s order, because what an input
    /// method commits arrives as a paste -- with the places that take a
    /// paste and not a key: a key being bound is a key and not a letter, a
    /// card with no box swallows what is typed at it, and a conversation
    /// with the keys off its box swallows it too.
    #[must_use]
    pub fn takes_text(&self) -> bool {
        match self.layers().nearest() {
            Some(Layer::Prompt | Layer::Names) => true,
            Some(Layer::Settings) => self
                .settings
                .as_ref()
                .is_some_and(|settings| settings.binding().is_none()),
            Some(Layer::Picker) => self
                .picker
                .as_ref()
                .is_some_and(|picker| !picker.is_only_read()),
            Some(Layer::Counts | Layer::Gone) => false,
            // Being asked which project, which is a page of its own and
            // typed into whichever box it is showing.
            None if self.which_project.chooser.is_some() => true,
            None => {
                if self.conversation().is_some() {
                    return self.conversation_takes_text();
                }
                if let Some(notes) = self.notes() {
                    return notes.writing().is_some() && !notes.selected_is_elsewhere();
                }
                // A program reads what is typed at it, an input method's
                // words included -- they arrive as a paste, which goes down
                // the pty like any other.
                if self.terminal().is_some() {
                    return self.typing_to_a_program();
                }
                // The same two things `Buffer::edit` refuses, and a reader
                // whose keys change no file: an input method spelling into
                // one would draw the word and then lose it on the commit.
                !self.settled.config.read_only
                    && self.current_buffer().is_some_and(|buffer| {
                        buffer.content().is_file() && buffer.mode() == obelus_buffer::Mode::Edit
                    })
            }
        }
    }

    /// Where what is typed now goes: the places [`App::paste_text`] writes
    /// into, in its order.
    ///
    /// Asked once, here, by the paste and by an input method's spelling --
    /// which has to know where it wrote last time as well as where it is
    /// writing now, and a second walk of the same layers in a second order
    /// is how a word would land in one box and be taken back out of another.
    #[must_use]
    pub(in crate::app) fn writing_into(&self) -> WritingInto {
        match self.layers().nearest() {
            Some(Layer::Prompt) => WritingInto::Prompt,
            Some(Layer::Settings) => WritingInto::Settings,
            Some(Layer::Names) => WritingInto::Names,
            Some(Layer::Picker) => WritingInto::Picker,
            // Nothing is typed into the counts, nor into the page saying the
            // project has gone -- and neither falls through to the file
            // behind it for want of anywhere else.
            Some(Layer::Counts | Layer::Gone) => WritingInto::Nowhere,
            // Being asked which project, which is a page of its own and not
            // a layer -- so this fell through to a file there is none of,
            // and a path pasted or spelled into its box went nowhere.
            None if self.which_project.chooser.is_some() => WritingInto::Chooser,
            // Nothing over the document, so it goes into the document -- and
            // a conversation is one. This asked about the *layers* and a
            // conversation was one of those, so when it stopped being one
            // the text stopped arriving anywhere: not in the box the reader
            // was typing in, and not in the file behind it either.
            None if self.conversation().is_some() => WritingInto::Conversation,
            // And the notes, for the same reason and by the same route.
            None if self.notes().is_some() => WritingInto::Notes,
            None if self.terminal().is_some() => WritingInto::Terminal,
            None => WritingInto::File,
        }
    }

    /// What an input method is spelling now, written in where the reader
    /// is writing, over what it was spelling a moment ago -- and taken out
    /// again, where it says nothing.
    ///
    /// In for real, the way Zed does it: a spelling drawn over the cells
    /// after the caret ran off the side of the box, where the same letters
    /// typed went onto the next row and pushed the rest along. The word the
    /// input method commits arrives afterwards as a paste, and an input
    /// method always says its spelling is over first, so that paste lands
    /// where the spelling was.
    ///
    /// Written in as a paste, because it is one in every way but the last:
    /// what is held goes, a list narrows, a search asks again. What a paste
    /// does not do is take its letters back, so the last spelling is held
    /// first, and the paste goes over it. In a file the spellings and the
    /// word are one step back, and what a word begun over a selection took
    /// is part of that step -- as it was when a word arrived alone.
    ///
    /// The caret goes where the input method has its own, `caret`
    /// characters into the spelling -- after all of it, where that is
    /// `None` -- and the spelling is underlined, as every input method's
    /// is, to say it is not a word yet.
    pub(in crate::app) fn spell(&mut self, said: &str, caret: Option<usize>) {
        let caret_after: String = said.chars().take(caret.unwrap_or(usize::MAX)).collect();
        let here = self.written_here();
        let takes = self.takes_text();
        // The last spelling is written over only where this one goes too.
        // Anywhere else it is taken back out of where it was first: a card
        // with no box came up over the message it was in, the reader was
        // taken to another file, the input method was turned off -- and a
        // spelling left there is letters nobody typed, in the reader's
        // words, for good.
        let before = match self.spelt.take() {
            Some(before) if before.at == here && takes => Some(before),
            Some(before) => {
                self.take_the_spelling_back(&before);
                None
            }
            None => None,
        };
        let into = here.into;
        // A program is sent nothing until the word arrives, because what is
        // sent to a program cannot be taken back: the letters are drawn
        // over its screen instead, and the word goes down the pty.
        self.spelling_over_a_program =
            (takes && into == WritingInto::Terminal && !said.is_empty()).then(|| said.to_string());
        if !takes {
            return;
        }
        match into {
            WritingInto::Nowhere | WritingInto::Terminal => {}
            WritingInto::File => self.spell_into_the_file(before, said, &caret_after),
            _ => {
                let mut over = false;
                let from = self.spelled_into().map(|spelling| {
                    if let Some(before) = &before {
                        over = spelling.hold_written(before.from, &before.said);
                    }
                    spelling.writes_from()
                });
                // Nothing written and nothing to take back: a paste of
                // nothing has a paste's other consequences -- a list goes
                // back to its top, a card's box opens -- and they would have
                // happened for nothing.
                if said.is_empty() && !over {
                    return;
                }
                self.paste_text(said);
                let landed = match (self.spelled_into(), from) {
                    (Some(spelling), Some(from)) => spelling.spelling(from, said, &caret_after),
                    _ => String::new(),
                };
                self.spelt = from.filter(|_| !landed.is_empty()).map(|from| Spelt {
                    said: landed,
                    from,
                    at: here,
                    took: false,
                    version: None,
                });
            }
        }
    }

    /// The same, into the file being read: through the one door a document
    /// changes by, where the box's own paste would make every spelling a
    /// step of its own.
    fn spell_into_the_file(&mut self, before: Option<Spelt>, said: &str, caret: &str) {
        // The same thing the paste refuses.
        if self.settled.config.read_only {
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // Over the last spelling where its letters are still in the file:
        // the buffer carries where it begins across whatever else changed
        // the file since, and says nothing where something changed the
        // letters themselves.
        let mut over = before.as_ref().and_then(|_| buffer.spelled());
        let ours = before
            .as_ref()
            .is_some_and(|before| before.version == Some(buffer.version()));
        // Something else changed the file under the word -- an agent wrote
        // in it. A step joins whatever came before it, so the word went
        // into that change's step, and undoing the word undid the agent's
        // edit. The last spelling goes out with that change instead, and
        // the word begins again as a step of its own: undoing it takes the
        // word and nothing else.
        if let (Some(span), false) = (over, ours) {
            self.change(span, "", obelus_buffer::undo::Doing::Joined);
            over = None;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let cursor = buffer.cursor();
        let at = self.written_here();
        let (span, doing) = match over {
            Some(span) => (span, obelus_buffer::undo::Doing::Joined),
            None => (
                buffer.selection().unwrap_or(Span {
                    line: cursor.line,
                    column: cursor.column,
                    end_line: cursor.line,
                    end_column: cursor.column,
                }),
                obelus_buffer::undo::Doing::Whole,
            ),
        };
        if said.is_empty() && over.is_none() {
            return;
        }
        // Whether the word's step took anything of the reader's: what was
        // held when the word began.
        let took = match (&before, over) {
            (Some(before), Some(_)) => before.took,
            _ => span.line != span.end_line || span.column != span.end_column,
        };
        let from = obelus_buffer::Cursor {
            line: span.line,
            column: span.column,
            ..cursor
        };
        self.change(span, said, doing);
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.spelling(from, said, caret);
        }
        let after = self.current.zip(self.current_buffer().map(Buffer::version));
        match (said.is_empty(), took) {
            // Over, having taken nothing: the step is letters in and out
            // again, and a press of undo that does nothing is forgotten.
            // The word, if one comes, is a step of its own.
            (true, false) => {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.forget_the_last_step();
                }
            }
            // Over, and the word it was for arrives next as a paste: it
            // joins the step the spelling took, with nothing between.
            (true, true) => self.spelled_over = after,
            (false, _) => {
                self.spelt = Some(Spelt {
                    said: said.to_string(),
                    from,
                    at,
                    took,
                    version: after.map(|(_, version)| version),
                });
            }
        }
    }

    /// Where what is typed now is written: what it goes into, and which
    /// one of those it is.
    ///
    /// More than [`App::writing_into`], because a spelling has to be found
    /// again where it was: two conversations are both a conversation, and a
    /// card over a message is not the message.
    fn written_here(&self) -> Written {
        let into = self.writing_into();
        let document = match into {
            WritingInto::Conversation | WritingInto::Notes | WritingInto::File => self.current,
            _ => None,
        };
        Written {
            into,
            document,
            card: into == WritingInto::Conversation && self.card().is_some(),
        }
    }

    /// Takes a spelling back out of where it was written, wherever the
    /// reader is now -- where its letters are still there.
    fn take_the_spelling_back(&mut self, spelt: &Spelt) {
        let (from, said) = (spelt.from, spelt.said.as_str());
        let spelling: Option<&mut dyn Spells> = match (spelt.at.into, spelt.at.document) {
            (WritingInto::File, Some(document)) => {
                let Some(buffer) = self.file(document) else {
                    return;
                };
                let Some(span) = buffer.spelled() else {
                    return;
                };
                // Part of the step the spelling took where nothing came
                // between, and then a step that took nothing is forgotten
                // -- the same as giving the word up where it was written.
                let ours = spelt.version == Some(buffer.version());
                let doing = match ours {
                    true => obelus_buffer::undo::Doing::Joined,
                    false => obelus_buffer::undo::Doing::Whole,
                };
                self.change_in(document.get(), span, "", doing);
                if ours
                    && !spelt.took
                    && let Some(buffer) = self.file_mut(document)
                {
                    buffer.forget_the_last_step();
                }
                return;
            }
            (WritingInto::Conversation, Some(document)) => {
                let Some(talk) = self.document_mut(document).and_then(Document::chat_mut) else {
                    return;
                };
                match spelt.at.card {
                    true => talk.card_in_front_mut().and_then(|card| card.spells()),
                    false => Some(talk.chat.writing_mut() as &mut dyn Spells),
                }
            }
            (WritingInto::Notes, Some(document)) => self
                .document_mut(document)
                .and_then(Document::notes_mut)
                .map(|notes| notes as &mut dyn Spells),
            (into, _) => self.spelled_in(into),
        };
        if let Some(spelling) = spelling {
            spelling.take_back(from, said);
        }
    }

    /// The box a spelling is written into, where what is typed goes into a
    /// box -- see [`App::writing_into`].
    fn spelled_into(&mut self) -> Option<&mut dyn Spells> {
        let into = self.writing_into();
        self.spelled_in(into)
    }

    /// The same, for what is typed into `into`.
    fn spelled_in(&mut self, into: WritingInto) -> Option<&mut dyn Spells> {
        match into {
            WritingInto::Prompt => self.prompt.as_mut().map(|prompt| prompt.spells()),
            WritingInto::Settings => self.settings.as_mut().map(|settings| settings.spells()),
            WritingInto::Names => self.names.as_mut().map(|(_, names)| names.spells()),
            WritingInto::Picker => self.picker.as_mut().map(|picker| picker.spells()),
            WritingInto::Chooser => self
                .which_project
                .chooser
                .as_mut()
                .map(|chooser| chooser.spells()),
            WritingInto::Conversation => {
                let talk = self.conversation_mut()?;
                if talk.card_in_front().is_some() {
                    return talk.card_in_front_mut().and_then(|card| card.spells());
                }
                Some(talk.chat.writing_mut() as &mut dyn Spells)
            }
            WritingInto::Notes => self.notes_mut().map(|notes| notes as &mut dyn Spells),
            WritingInto::Nowhere | WritingInto::Terminal | WritingInto::File => None,
        }
    }

    pub(in crate::app) fn paste_text(&mut self, what: &str) {
        // Into whatever the reader is writing on, which is the nearest
        // thing over the file -- the same answer a key gets, asked the same
        // way. A paste that went past it into the file behind would put
        // their text somewhere they cannot see it, and that is what this
        // did with a list or a conversation open: the guard asked about the
        // notes and a question, and about nothing else.
        match self.writing_into() {
            WritingInto::Prompt => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.put(what);
                }
                return;
            }
            WritingInto::Settings => {
                let offering = self.agent_offering();
                if let Some(settings) = self.settings.as_mut() {
                    settings.put_in_query(what, offering.as_ref());
                }
                return;
            }
            // The query of the list a setting's names are built in, which
            // is the box the reader is typing in while it is open.
            WritingInto::Names => {
                if let Some((_, names)) = self.names.as_mut() {
                    names.put_in_query(what);
                }
                return;
            }
            WritingInto::Picker => {
                // A search's rows come from the query, so a query that
                // changed by being pasted into has to be asked again -- the
                // same thing the key path does when a keystroke changes it.
                let searching = self.picker.as_ref().is_some_and(Picker::is_searching);
                if let Some(picker) = self.picker.as_mut() {
                    picker.put_in_query(what);
                }
                if searching {
                    self.refresh_search();
                }
                return;
            }
            WritingInto::Nowhere => return,
            WritingInto::Chooser => {
                self.paste_into_the_chooser(what);
                return;
            }
            WritingInto::Conversation => {
                self.paste_into_conversation(what);
                return;
            }
            WritingInto::Notes => {
                self.paste_into_notes(what);
                return;
            }
            // A terminal's program, which reads a paste the way a terminal
            // hands it one. One whose program has ended takes nothing, and
            // what it is does not change into a file for want of one.
            WritingInto::Terminal => {
                self.paste_into_terminal(what);
                return;
            }
            WritingInto::File => {}
        }
        // The file, which a reader who has the agent make every change does
        // not paste into: here rather than in `offers`, because a terminal's
        // own paste and a window's input method arrive without a command.
        if self.settled.config.read_only {
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let cursor = buffer.cursor();
        let here = self.current.zip(Some(buffer.version()));
        let span = buffer.selection().unwrap_or(Span {
            line: cursor.line,
            column: cursor.column,
            end_line: cursor.line,
            end_column: cursor.column,
        });
        // The word a selection was taken out for, with nothing between.
        let doing = match self.spelled_over.take() {
            Some(over) if Some(over) == here => obelus_buffer::undo::Doing::Joined,
            _ => obelus_buffer::undo::Doing::Whole,
        };
        self.change(span, what, doing);
    }

    /// Puts back what the last change took away.
    pub fn undo(&mut self) {
        if self.notes_undo(true) {
            return;
        }
        let Some(index) = self.current.map(DocumentId::get) else {
            return;
        };
        let went_back = self
            .file_mut(DocumentId::new(index))
            .is_some_and(Buffer::undo);
        match went_back {
            true => self.change_document(index),
            false => self.wrong("Nothing to undo".to_string()),
        }
    }

    /// Does again what [`undo`](Self::undo) put back.
    pub fn redo(&mut self) {
        if self.notes_undo(false) {
            return;
        }
        let Some(index) = self.current.map(DocumentId::get) else {
            return;
        };
        let went_forward = self
            .file_mut(DocumentId::new(index))
            .is_some_and(Buffer::redo);
        match went_forward {
            true => self.change_document(index),
            false => self.wrong("Nothing to redo".to_string()),
        }
    }

    /// The question being asked, if one is.
    #[must_use]
    pub const fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// Offers a key to the question on the status bar, and says whether it
    /// took it.
    ///
    /// It takes every bare character, accepted or refused -- a key the
    /// prompt turns down is still a key it *saw*, and a digit falling into
    /// the file under a "line:" question would be the reader's answer
    /// landing somewhere else. What falls through is the chords, which is
    /// how `ctrl+q` still leaves Obelus from inside one.
    pub(in crate::app) fn prompt_key(&mut self, key: &KeyEvent) -> bool {
        let Some(prompt) = self.prompt.as_mut() else {
            return false;
        };
        let kind = prompt.kind().clone();
        match prompt.handle_key(key) {
            PromptOutcome::Consumed => true,
            PromptOutcome::Cancelled => {
                self.leave(Layer::Prompt);
                true
            }
            PromptOutcome::Accepted(text) => {
                self.prompt = None;
                self.answer(kind, &text);
                true
            }
            PromptOutcome::Ignored => false,
        }
    }

    /// Acts on an answered prompt.
    ///
    /// Where "what it means" lives: the prompt knows what was typed and what
    /// kind of question it was, and nothing more. A line past the end
    /// of the file is clamped rather than refused -- `9999` in a short file
    /// means the end of it -- and a line number that is not a number is a
    /// note, because it is the reader's slip and not the file's.
    pub(in crate::app) fn answer(&mut self, kind: PromptKind, text: &str) {
        match kind {
            // The new name for whatever the cursor is on. Empty is the
            // reader having deleted the name and pressed enter, which is
            // not a rename to nothing -- it is a change of mind.
            PromptKind::Name => {
                let name = text.trim();
                if name.is_empty() {
                    return;
                }
                self.ask_rename(name);
            }
            // What a file should be called instead. Empty is the reader
            // having cleared the line and pressed enter, which is a change
            // of mind rather than a rename to nothing.
            PromptKind::Path => {
                let path = text.trim();
                if path.is_empty() {
                    return;
                }
                self.rename_file_to(std::path::Path::new(path));
            }
            // Where a file that is not there yet should go. No guard
            // against a blank one, unlike the arm above: `Field::is_empty`
            // asks whether the line is *blank*, so enter on one of spaces
            // is consumed and this is never reached with nothing in it. A
            // second answer here would be an unreachable one.
            PromptKind::NewPath => self.make_file(std::path::Path::new(text.trim())),
            // What one of a chat's fields is, from the remote page.
            PromptKind::Told(field) => self.tell_the_remote(field, Some(text)),
            // A variable the reader is adding to the agent's environment:
            // its name, and then what it holds.
            PromptKind::VariableName { agent, .. } => self.name_a_variable(&agent, text.trim()),
            PromptKind::Variable { agent, name } => {
                self.set_agent_variable(&agent, &name, Some(text));
            }
            // One of Obelus's own that is typed, from the settings page.
            PromptKind::Setting(key) => {
                self.change_setting(key, &obelus_config::Value::Text(text.to_string()));
            }
            PromptKind::Line => {
                let Ok(line) = text.trim().parse::<usize>() else {
                    self.wrong(format!("{text:?} is not a line number"));
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

/// Where what is typed now goes -- see [`App::writing_into`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum WritingInto {
    /// The question on the status row.
    Prompt,
    /// The settings' filter.
    Settings,
    /// The query of the list a setting's names are built in.
    Names,
    /// A list's query.
    Picker,
    /// Nowhere at all.
    Nowhere,
    /// The page that asks which project.
    Chooser,
    /// The box a message is written in, or the card over it.
    Conversation,
    /// The note being written.
    Notes,
    /// A terminal's program.
    Terminal,
    /// The file being read.
    File,
}

/// Where a spelling was written -- see [`App::written_here`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct Written {
    /// What it went into.
    into: WritingInto,
    /// Which conversation, note page or file, where it went into one.
    document: Option<DocumentId>,
    /// Whether it went into a card's box rather than the message under it.
    card: bool,
}

/// A word an input method is spelling, as it was last written in.
#[derive(Clone, Debug)]
pub(in crate::app) struct Spelt {
    /// What was written.
    said: String,
    /// Where it begins. Where it ends is worked out from what it says,
    /// because the caret may be anywhere in it.
    from: obelus_buffer::Cursor,
    /// Where it was written.
    at: Written,
    /// Whether the step it is part of took anything of the reader's -- what
    /// was held when the word began -- which is what says a word given up
    /// leaves nothing behind to undo.
    took: bool,
    /// The version the spelling left the file at, where it was written
    /// into one: what says nothing else has changed the file since.
    version: Option<i32>,
}
