//! The few lines of somewhere else that a list shows.
//!
//! Its own buffer, its own highlighting and its own scroll: a preview is a
//! second document on screen, and the one being read is not to be disturbed
//! by looking at it.
//!
//! A preview is of a subject, not of a path. A row does not always name a
//! file on disk: a commit names what it said, and one of a commit's files names
//! that file as the commit had it -- a different document from the one at the
//! same path in the working tree. `Subject` is what a row resolves to, and the
//! preview is built from it the way the editor would build it, message block
//! and all, because a preview that showed something other than what choosing
//! the row gives is a promise Obelus does not keep. A commit's message previews
//! as a block over an empty buffer, which is how it gets no line numbers: a
//! message has no lines of its own to go to.
//!
//! A file that is open is previewed where it is being read. Whichever list
//! names it -- the open files, or the whole project -- because it is one
//! question with one answer: a file's own place in it is the thing a reader
//! remembers it by, and choosing the row takes them back to exactly that, so
//! the list reads as something folded over the file rather than as a way
//! somewhere new. A file nothing has opened has no such place and starts at the
//! top. `App::read_at` is the one answer; a list that had its own would be a
//! list where choosing a row moved the screen under the reader.

use obelus_ui::Previewed;

use super::*;

impl App {
    /// The file the picker's selection names, if it has been read, and the
    /// part of it the selection is about.
    #[must_use]
    pub fn preview(&self) -> Option<Previewed<'_>> {
        let preview = self.preview.as_ref()?;
        Some(Previewed {
            buffer: &preview.buffer,
            highlights: &preview.highlights,
            marked: &preview.marked,
            changes: preview.changes.as_ref(),
            troubles: &preview.troubles,
            text: preview.text,
            complaint: preview
                .complaint
                .as_ref()
                .map(|complaint| obelus_ui::Complained {
                    line: complaint.line,
                    column: complaint.column,
                    said: &complaint.said,
                    severity: complaint.severity,
                    others: complaint.others,
                }),
        })
    }

    /// Wears whatever theme the picker's selection names.
    ///
    /// The preview *is* the application: there is no way to show what a theme
    /// looks like other than by using it, and every view already reads its
    /// colours from one place. Cancelling puts the old one back.
    pub(super) fn preview_theme(&mut self) {
        let selected = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .map(|item| item.value.clone());
        let called = match &selected {
            Some(PickerValue::Theme(name)) => Some(name.clone()),
            // The same list reached from the settings page, where a theme
            // is one setting's value rather than a thing of its own. It is
            // the same choice and it needs the same answer: a droplist of
            // colours a reader can walk without seeing any of them is one
            // they have to choose from blind.
            Some(PickerValue::Setting { key: "theme", word }) => Some(word.clone()),
            _ => None,
        };
        let Some(called) = called else { return };
        // The name with the colours, because the two are what the theme *is*
        // here: a preview that wore one and answered with the other would
        // have the settings page saying one thing and the screen another.
        if let Some(theme) = self.theme_called(&called) {
            self.set_theme(&called, theme);
        }
    }

    /// Shows the reader where the list's selection is, in the file itself.
    ///
    /// Only for a list that sits on the status bar. A full-area one has
    /// covered the file and shows its selection in a preview of its own,
    /// so the question of what is behind it does not arise; a compact one
    /// is drawn *on* the file precisely so that the file stays readable,
    /// and then the file is the preview.
    ///
    /// Only for a place in the file being read, too: scrolling this file to
    /// a line number that belongs to another one would be showing the
    /// reader somewhere with confidence and getting it wrong.
    ///
    /// A look and not a move. The caret stays where the reader left it, and
    /// [`App::look_back`] puts the view there too if they leave without
    /// choosing -- so walking a list of problems costs nothing to change
    /// your mind about.
    pub(super) fn look_at_the_selection(&mut self) {
        let Some((line, _)) = self.the_selection_in_this_file() else {
            return;
        };
        // The room the reader can actually see, which the editor has been
        // given rather than having to be worked out here: a list sitting on
        // the status bar shortens the editor's region rather than covering
        // it, so this is the same area everything else measures with.
        let area = self.text_area();
        let Some(buffer) = self.current_buffer_mut() else {
            return;
        };
        // Nothing at all for somewhere already shown, which is the same
        // rule `go-to-next-change` follows: a place the reader can see is a
        // short hop, and moving the view for it throws away their place --
        // and on the row they are standing on, walking a list would scroll
        // the file out from under them before they had chosen anything.
        //
        // Shown, not merely drawn. A line on the last row above the list is
        // a line with the list against it and nothing of the file under it,
        // which reads as the list having covered the very thing it is
        // pointing at. A few rows of margin either side is the difference
        // between being on the screen and being somewhere the reader can
        // read -- and on a short screen there is no room to be fussy, so
        // the margin is a share of the room rather than a number.
        let edge = (area.height / 4).min(3);
        if buffer
            .screen_row_of(line, area)
            .is_some_and(|row| row >= edge && row + edge < area.height)
        {
            return;
        }
        buffer.look_at(line, area);
    }

    /// The line a list sitting on the status bar has selected, when what it
    /// has selected is a place in the file being read.
    ///
    /// The line the reader is looking at, in other words -- which is not
    /// the caret's while a list is up. Two things ask: the look, which
    /// scrolls the file to it, and the complaint, which opens under it. One
    /// answer, so the row they are on and the box they are reading are
    /// always about the same place.
    ///
    /// `None` for a full-area list, which has covered the file rather than
    /// sitting on it, and for a place in another file: scrolling this file
    /// to a line number that belongs to another one would be showing the
    /// reader somewhere with confidence and getting it wrong.
    #[must_use]
    pub(super) fn the_selection_in_this_file(
        &self,
    ) -> Option<(
        obelus_text::coordinates::LineNumber,
        obelus_text::coordinates::CharColumn,
    )> {
        let picker = self.picker.as_ref()?;
        if picker.layout() == obelus_component::picker::PickerLayout::FullArea {
            return None;
        }
        let PickerValue::Place {
            path,
            line,
            character,
            ..
        } = &picker.selected_item()?.value
        else {
            return None;
        };
        let buffer = self.current_buffer()?;
        if buffer.path() != path {
            return None;
        }
        // Back into the file's own units, which is the same conversion the
        // rows were made with read the other way: a row carries the
        // protocol's position because that is what a row that names a place
        // carries, and what is wrong with a line is held in the file's.
        let encoding = buffer
            .language()
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        Some(obelus_lsp::position::from_lsp(
            buffer.text(),
            lsp_types::Position {
                line: *line,
                character: *character,
            },
            &encoding,
        ))
    }

    /// Puts the view back where the reader was looking before a list showed
    /// them somewhere else.
    ///
    /// Nothing at all if they never looked anywhere -- a list of commands
    /// has no places in it -- and nothing if they have since moved to
    /// another document, which is a reader who has gone somewhere rather
    /// than one who is coming back.
    pub(super) fn look_back(&mut self) {
        let Some((id, viewport)) = self.looked_from.take() else {
            return;
        };
        if self.current != Some(id) {
            return;
        }
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.look_back(viewport);
        }
    }

    /// Puts the file a list was opened over back where it was, wherever the
    /// reader has gone since.
    ///
    /// For a list left by choosing a row. What the file was scrolled by while
    /// the list was up was the list's -- a list on the status bar shortens
    /// the editor, and the file scrolls to keep the caret above it -- and a
    /// row that runs a command, makes a change or opens a conversation is
    /// not the reader moving the file. Left where the list put it, the file
    /// stayed a few rows up after the list had gone, and at the end of a file
    /// with a run of nothing under the last line. Unlike `look_back`, not
    /// only while it is the file in front: a conversation chosen from a list
    /// is a reader who will come back to this file, and it should be as they
    /// left it.
    pub(super) fn put_the_file_back(&mut self) {
        let Some((id, viewport)) = self.looked_from.take() else {
            return;
        };
        if let Some(buffer) = self.file_mut(id) {
            buffer.look_back(viewport);
        }
    }

    /// Reads whatever the picker's selection names, and points it at the line
    /// the selection is about.
    pub(super) fn refresh_preview(&mut self, editor_area: Rect) {
        let Some(area) = self.preview_area(editor_area) else {
            self.preview = None;
            return;
        };
        let Some((subject, marked)) = self.preview_target() else {
            self.preview = None;
            return;
        };
        // The file being read is not read a second time to be shown over
        // itself. A compact list is drawn on it so that it stays readable,
        // and it is the real one that stays there -- with the caret where
        // the reader left it, the complaint under its line, and
        // `look_at_the_selection` scrolling it to the row they are on.
        if self.previews_over_the_code(editor_area) && self.reading_this(&subject) {
            self.preview = None;
            return;
        }

        if self.preview.as_ref().map(|preview| &preview.subject) != Some(&subject) {
            let read = self.read(&subject).map(|(buffer, changes)| Preview {
                // Both settled further down, once the room is known and
                // the row has been asked what it is about. Nothing is
                // drawn from a preview before then.
                text: obelus_buffer::TextArea {
                    width: 0,
                    height: 0,
                    wrap: true,
                },
                complaint: None,
                subject: subject.clone(),
                changes,
                troubles: self.wrong_in(&subject, &buffer),
                buffer,
                highlights: Highlights::default(),
                marked: Vec::new(),
                target: marked.at(),
                scrolled: 0,
            });
            self.preview = read;
        }

        // Whichever encoding the server for this language agreed to. Nothing
        // named the place if there is no server, and then the mark is the top
        // of the file, where the units do not matter.
        let encoding = self
            .preview
            .as_ref()
            .and_then(Preview::language)
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });

        let complaining = self.config().diagnostics;
        let troubling = self.showing_troubles();
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        // A different row is a different subject, so whatever the reader had
        // scrolled to is about the row they have left.
        if preview.target != marked.at() {
            preview.target = marked.at();
            preview.scrolled = 0;
        }

        // The line in question in the middle, the same as arriving at it by
        // jumping, and then wherever the reader has scrolled to. A preview is
        // read for the context around a line, so putting the line at the top
        // spends half the room on the half that was not asked for.
        let target = LineNumber::new(marked.line() as usize);
        // Everything the editor draws around the text, because the editor
        // is what draws this: the change margin and the fold column before
        // it, the change map and the bar after. A width that counted only
        // the gutter would wrap the preview at a column wider than the room
        // it is given, and the last cells of a wrapped line would fall off
        // the edge.
        let changes = preview.changes.as_ref();
        let aside = obelus_ui::editor::text_offset(
            preview.buffer.text().line_count(),
            obelus_ui::editor::changed(changes),
            !preview.buffer.folds().is_empty(),
        )
        .saturating_add(obelus_ui::editor::map_width(changes))
        .saturating_add(obelus_ui::editor::SCROLLBAR_WIDTH);
        let text = TextArea {
            width: area.width.saturating_sub(aside),
            height: area.height,
            // A preview always wraps: it is a few lines of somewhere else,
            // and a line running off its right-hand edge with no way to
            // scroll it would be a line nobody can read.
            wrap: true,
        };
        let runs = marked.resolve(preview.buffer.text(), &encoding);
        // What is wrong with the line it is showing, opened under that line
        // the way the editor opens it under the caret's. A list of problems
        // that previewed the line and left the words in the row above is a
        // reader reading a message through a slot; the box is where the
        // words fit.
        //
        // Only for that list. The underline belongs to every buffer the
        // editor draws -- what is wrong is wrong wherever the line is shown
        // -- but the box is an answer to a row that *names* a problem. Over
        // a search's rows it would be four lines about something the query
        // was not asking after, in the room the match was to be read in.
        //
        let Preview { troubles, .. } = preview;
        let said = match complaining && troubling {
            // Which of the line's troubles the row is about, where the row
            // is about one at all: a search's mark is characters a query
            // matched, and a column of that is not a column anything was
            // said at.
            true => {
                let chosen = matches!(marked, Marked::Span { .. })
                    .then(|| runs.first().map(|span| span.column))
                    .flatten();
                semantics::what_is_wrong(troubles, target, CharColumn::new(0), chosen)
            }
            // Nothing rather than left alone: the same file previewed from
            // the list of problems and then from a search is one preview
            // read twice, and the box the first list opened is not the
            // second one's to keep. The same for the setting going off.
            false => None,
        };
        // Beside the buffer rather than in it. A complaint is somebody
        // else's prose about the file and not text the file ever had, so
        // rows of it in the buffer are rows of the code the reader is
        // previewing pushed out of the way. The room goes with it, because
        // the box has to land on the row the text was laid out on.
        preview.complaint = said;
        preview.text = text;
        // A problem is not marked in its own preview. The box says which
        // line, which column and what was said, and a run of colour over
        // the same characters is that said again -- differently, because a
        // mark is one colour whatever the severity. It would also be the
        // odd one out: a row in the file the reader is already in has no
        // preview to mark, so half the list would be highlighted and half
        // of it not, for no reason a reader could see.
        preview.marked = match troubling {
            true => Vec::new(),
            false => runs,
        };
        preview.buffer.place_cursor(target, CharColumn::new(0));
        // Being put somewhere clears the caret out of any block, which is
        // right for a reader who asked to go to a line and wrong here: a
        // commit's message hangs above the first line, so a preview aimed
        // at that line shows the *end* of the message and calls it the
        // beginning of the file. Aimed at the top, the top is the message.
        if target == LineNumber::new(0) {
            preview.buffer.enter_block(LineNumber::new(0));
        }
        preview.buffer.center_on_cursor(text);
        // Stored back, so rows the file does not have are not banked against
        // the next press the other way.
        preview.scrolled = preview.buffer.scroll_rows(preview.scrolled, text);

        let range = preview.buffer.visible_bytes(area.height);
        if let Some(state) = preview.buffer.syntax() {
            preview
                .highlights
                .refresh(state, preview.buffer.text(), range);
        } else {
            preview.highlights.clear();
        }
    }

    /// Gives one layer the notch, and says whether it took it.
    ///
    /// Every layer answers, including the ones with nothing to scroll: a
    /// match the compiler checks is what stops the next view being left out
    /// of this the way the notes were.
    fn scroll_layer(&mut self, layer: obelus_component::layers::Layer, rows: isize) -> bool {
        use obelus_component::layers::Layer;
        match layer {
            // One row a notch in a list. Three is right for text, where a
            // notch is a gesture at a paragraph; a list is chosen through
            // one row at a time.
            Layer::Picker => {
                let Some(picker) = self.picker.as_mut() else {
                    return false;
                };
                picker.move_selection_by(rows.signum());
                true
            }
            // A list is a list: a notch steps the selection, because the
            // selection is what a list has to scroll.
            Layer::Names => {
                let Some((_, names)) = self.names.as_mut() else {
                    return false;
                };
                names.step(rows.signum());
                true
            }
            // The counts, which are a list as well: the notch steps the row
            // rather than the view, for the same reason it does in a picker.
            Layer::Counts => {
                let Some(counts) = self.counts.as_ref() else {
                    return false;
                };
                let height = obelus_ui::counts::list_height(self.screen_area, counts);
                if let Some(counts) = self.counts.as_mut() {
                    counts.scroll(rows, height);
                }
                true
            }
            // A question is one row and has nothing to scroll; the notes
            // and the settings scroll with the keys and have never taken
            // the wheel. Saying so is the point: the next view added has to
            // answer here rather than being quietly left out.
            Layer::Prompt | Layer::Settings => false,
            // Taken and spent: what is under the page is what the reader
            // was looking at when the project went, and it stays as it was.
            Layer::Gone => true,
        }
    }

    /// What the wheel turns.
    ///
    /// Whatever the reader is looking at: the list when one is open -- a list
    /// under a wheel scrolls, and with the mouse reported the wheel no longer
    /// arrives as arrow keys, so a picker that ignored it would have lost
    /// something -- and otherwise the document, by rows, with the cursor left
    /// where it was put. A conversation is a document like the file is, and
    /// the two say that the same way.
    pub(super) fn scroll(&mut self, rows: isize) {
        // What a server said about a place, while it is up: it is what the
        // reader is looking at, and the file behind it is not going
        // anywhere.
        if self.hover().is_some() {
            self.scroll_hover(rows);
            return;
        }
        // What could be typed next, which is a list beside the cursor: a
        // notch walks it a row, the way a notch walks any list in Obelus.
        if let Some(completion) = self.completion.as_mut() {
            completion.scroll(rows.signum());
            return;
        }
        // And then whatever is over the file, nearest first -- the same
        // order a key is offered in, because a notch is a key by another
        // name. The two that answer `false` do so on purpose: this used to
        // be a chain that simply did not mention them, so a notch over the
        // notes scrolled the file behind them.
        for layer in self.layers().nearest_first() {
            if self.scroll_layer(layer, rows) {
                return;
            }
        }
        // The conversation, which is a document and takes the wheel the
        // way the file below does: by rows, with the cursor left where the
        // reader put it. After the layers, because a list open over a
        // conversation is nearer than the conversation is; before the
        // file, because while a conversation is what is being read the
        // file is not on screen -- which is why a notch used to reach
        // nothing at all in here.
        if let Some(talk) = self.conversation_mut() {
            talk.chat.scroll(rows);
            return;
        }
        // A terminal: to its program where it asked for the pointer, and
        // otherwise back up what has gone past -- the view and not the
        // program, because a reader spinning the wheel is looking around,
        // and the next key they type brings it back down.
        if self.terminal().is_some() {
            self.wheel_in_terminal(rows);
            return;
        }
        let height = self.editor_area.height;
        if let Some(rows_in_view) = self.rendered_rows()
            && let Some(buffer) = self.current_buffer_mut()
        {
            buffer.scroll_rendering(rows, rows_in_view, height);
            return;
        }
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_by(rows, area);
        }
    }

    /// What a bar the reader has hold of moves: the thing it is beside, to
    /// put `top` first.
    ///
    /// The view and never the choice, for a list as for a file: the bar is
    /// a picture of where the view is. So a document leaves its cursor
    /// where the wheel would, and a list its selection where the reader
    /// put it, until a key moves that and the view goes back to it.
    pub(super) fn drag_bar(&mut self, whose: obelus_ui::bars::Whose, top: usize) {
        use obelus_ui::bars::Whose;

        match whose {
            Whose::Document => {
                let height = self.editor_area.height;
                if let Some(rows) = self.rendered_rows()
                    && let Some(buffer) = self.current_buffer_mut()
                {
                    // A reading counts in its own rows, one to a row of the
                    // screen, which is what this scrolls by.
                    let now = buffer.viewport().top.get();
                    let by = isize::try_from(top).unwrap_or(isize::MAX)
                        - isize::try_from(now).unwrap_or(isize::MAX);
                    buffer.scroll_rendering(by, rows, height);
                    return;
                }
                let area = self.text_area();
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.scroll_to_shown(top, area);
                }
            }
            Whose::Preview => {
                if let Some(preview) = self.preview.as_mut() {
                    // Kept as rows from its line, which is how the keys
                    // move it: so the viewport is moved now, clamped where
                    // the file ends, and what is kept is how far it went --
                    // which also makes a second move before the next frame
                    // measured from where this one left it.
                    let text = preview.text;
                    let from = preview.buffer.viewport();
                    preview.buffer.scroll_to_shown(top, text);
                    preview.scrolled += preview.buffer.rows_since(from, text);
                }
            }
            Whose::Conversation => {
                if let Some(talk) = self.conversation_mut() {
                    talk.chat.drag_to(top);
                }
            }
            Whose::Notes => {
                if let Some(notes) = self.notes_mut() {
                    notes.drag_to(top);
                }
            }
            Whose::Projects => {
                if let Some(chooser) = self.chooser.as_mut() {
                    chooser.drag_to(top);
                }
            }
            Whose::Picker => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.drag_to(top);
                }
            }
            Whose::Naming => {
                if let Some(list) = self.naming_list.as_mut() {
                    list.drag_to(top);
                }
            }
            Whose::Commands => {
                if let Some(slash) = self.conversation_mut().and_then(|talk| talk.slash.as_mut()) {
                    slash.drag_to(top);
                }
            }
            Whose::Settings => {
                if let Some(settings) = self.settings.as_mut() {
                    settings.drag_to(top);
                }
            }
            Whose::Counts => {
                let Some(counts) = self.counts.as_ref() else {
                    return;
                };
                // Settled here rather than once a frame, as its keys are.
                let height = obelus_ui::counts::list_height(self.screen_area, counts);
                if let Some(counts) = self.counts.as_mut() {
                    counts.drag_to(top, height);
                }
            }
            Whose::Names => {
                if let Some((_, names)) = self.names.as_mut() {
                    names.window_mut().drag_to(top);
                }
            }
            Whose::Completion => {
                if let Some(completion) = self.completion.as_mut() {
                    completion.drag_to(top);
                }
            }
            Whose::Documentation => {
                if let Some(completion) = self.completion.as_mut() {
                    completion.drag_documentation_to(top);
                }
            }
            Whose::Hover => {
                if let Some(hover) = self.hover.as_mut() {
                    hover.drag_to(top);
                }
            }
        }
    }

    /// Pages the preview, if that is what the key does and there is one to
    /// page.
    ///
    /// Answered here rather than where keys are sorted, because what decides
    /// it is whether a preview is on screen -- and that is this module's own
    /// question. A list with nothing under it says no, and the key goes on
    /// to the list, where the bare paging keys still page: a key that
    /// stopped working in a terminal too short for a preview would be worse
    /// than either arrangement.
    pub(super) fn page_preview(&mut self, key: &KeyEvent) -> bool {
        let Some(pages) = preview_paging(key) else {
            return false;
        };
        // A pane of its own, and something in it. Not a preview drawn over
        // the code: there the list is the thing the reader is working
        // through, and paging belongs to its rows -- which is where these
        // keys went before there was anything to show above them.
        if self.preview.is_none() || self.previews_over_the_code(self.editor_area) {
            return false;
        }
        self.scroll_preview(pages);
        true
    }

    /// Scrolls the preview, without moving the selection.
    ///
    /// Not a command: it is navigation, and navigation belongs to whatever
    /// holds the position it moves. What holds this one is the preview.
    pub(super) fn scroll_preview(&mut self, pages: isize) {
        let Some(area) = self.preview_area(self.editor_area) else {
            return;
        };
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let rows = isize::try_from(area.height.max(1)).unwrap_or(1);
        // Not clamped here: what the file can actually give is known when it
        // is drawn, and `refresh_preview` stores that back.
        preview.scrolled += pages * rows;
    }

    /// Where this list shows what its selection names.
    ///
    /// Under a full-area list, which has covered the file and needs a pane
    /// of its own, or over a compact one, in the room the code is drawn in.
    /// Two places and one preview: the reading and the marking are the same
    /// wherever it goes.
    fn preview_area(&self, editor_area: Rect) -> Option<Rect> {
        obelus_ui::picker::preview_region(self.picker.as_ref(), editor_area).or_else(|| {
            obelus_ui::picker::preview_over(self.picker.as_ref(), self.above(editor_area))
        })
    }

    /// Whether that place is the room the code was drawn in.
    fn previews_over_the_code(&self, editor_area: Rect) -> bool {
        obelus_ui::picker::preview_over(self.picker.as_ref(), self.above(editor_area)).is_some()
    }

    /// The room a compact list leaves above itself.
    ///
    /// Which is the room the document was given, because a list sitting on
    /// the status bar shortens the document rather than covering it --
    /// except over a conversation, which keeps the whole region and has the
    /// list drawn on top of it. Worked out here as well as where it is
    /// drawn, because a preview centred in a taller room than it is drawn
    /// in puts the line it is about below the bottom of it.
    fn above(&self, editor_area: Rect) -> Rect {
        let Some(picker) = self.picker.as_ref().filter(|_| self.chat().is_some()) else {
            return editor_area;
        };
        obelus_ui::picker::room_above(picker, editor_area)
    }

    /// Whether what a row names is the file already on screen behind the
    /// list.
    fn reading_this(&self, subject: &Subject) -> bool {
        match subject {
            Subject::File(path) => self
                .current_buffer()
                .is_some_and(|buffer| buffer.path() == path),
            // A file as a commit had it is not the file on disk, whatever
            // its path says, and a message is not a file at all.
            Subject::Commit { .. } | Subject::Message(_) => false,
        }
    }

    /// The file, and the part of it, the picker's selection is about.
    fn preview_target(&self) -> Option<(Subject, Marked)> {
        let picker = self.picker.as_ref()?;
        // Nothing chosen, because there is nothing to choose: a search with
        // nothing typed into it yet, or a query that matches none of the
        // rows. The preview is then the file being read, where it is being
        // read -- the list has pushed the editor down the screen rather
        // than replaced it with a blank, and what was under the reader's
        // eyes a moment ago is still there to look at.
        let Some(item) = picker.selected_item() else {
            return self.reading_now();
        };
        // A row that *is* the line it names is there because the query
        // matched some of its characters, and those are what the preview
        // marks: the list has already said which they are, and marking
        // anything else -- the whole line, as this did -- answers a
        // question nobody asked. A row that is a name rather than a line --
        // a symbol -- keeps the span whoever named it gave.
        let lines = self.rows_are_lines();
        let matched = |line: u32| Marked::Matched {
            line,
            columns: picker.indices_at(picker.selected()).to_vec(),
        };
        match &item.value {
            // A file has no symbol in it to mark, so nothing is
            // highlighted -- and where it is shown is where it is being
            // read, when it is open at all.
            PickerValue::File(path) => {
                let path = self.working_directory.join(path);
                let at = self.read_at(&path, None);
                Some((Subject::File(path), at))
            }
            // A file already open is being read somewhere, and that is the
            // part of it to show: choosing the row takes the reader back to
            // exactly this, so the list reads as something folded over the
            // file rather than as a way to somewhere new.
            //
            // A conversation has none of that -- no path, no cursor, and a
            // transcript that only its own view can draw -- so the row falls
            // back to what the row above the list falls back to: what the
            // reader was reading. A blank half-screen would be the list
            // saying a conversation is nothing rather than saying it has
            // nothing to show here.
            PickerValue::Document(id) => self
                .file(*id)
                .map(|buffer| {
                    let subject = match buffer.content().at() {
                        Some(id) => Subject::Commit {
                            id,
                            path: buffer.path().to_path_buf(),
                        },
                        None => Subject::File(buffer.path().to_path_buf()),
                    };
                    (subject, Marked::on(&buffer.cursor()))
                })
                .or_else(|| self.reading_now()),
            PickerValue::Place { path, line, .. } if lines => {
                Some((Subject::File(path.clone()), matched(*line)))
            }
            PickerValue::Place {
                path,
                line,
                character,
                end_line,
                end_character,
            } => Some((
                Subject::File(path.clone()),
                Marked::Span {
                    line: *line,
                    character: *character,
                    end_line: *end_line,
                    end_character: *end_character,
                },
            )),
            // A directory has nothing to show: what it holds goes under it
            // in the list itself.
            PickerValue::Directory(_)
            // A tree is a whole checkout and no one file of it, and its tab
            // shows no preview -- see `refresh_switching`.
            | PickerValue::Worktree(_)
            | PickerValue::Command(_)
            // A conversation has no path and no cursor, and this list is
            // compact anyway: there is no room under it to show one in.
            | PickerValue::Conversation(_)
            // A thing the server offers to do has nowhere to show: what it
            // would change is not worked out until it is chosen.
            | PickerValue::Action(_)
            | PickerValue::Theme(_)
            | PickerValue::Setting { .. }
            | PickerValue::AgentValue { .. }
            | PickerValue::AgentDefault { .. }
            // A list of background work is compact, and what a row has
            // written is opened rather than shown.
            | PickerValue::Task { .. } => None,
            // What choosing the row gives, which is not the same thing in
            // both radii. In a file's history it gives that file as the
            // commit had it, message and all; in the project's it opens the
            // commit's files under it, and until one of them is picked there
            // is no file to show -- so what the commit said is all there is.
            //
            // Under the name that commit had it under, which is the name
            // the row opens: a walk goes on under the name a file had
            // before it was moved, and asking for the name it has now
            // would find nothing for every row older than the move.
            PickerValue::Commit(id) => Some(match self.commit_opens_at(*id) {
                Some(path) => {
                    let path = self.working_directory.join(path);
                    let at = self.read_at(&path, Some(*id));
                    (Subject::Commit { id: *id, path }, at)
                }
                None => (Subject::Message(*id), Marked::top()),
            }),
            PickerValue::CommitFile { id, path } => {
                let path = self.working_directory.join(path);
                let at = self.read_at(&path, Some(*id));
                Some((Subject::Commit { id: *id, path }, at))
            }
            // A question is about what is already on screen, and the reader
            // has to be able to see it to answer: a preview would cover the
            // file whose fate is being asked about.
            PickerValue::Answer(_) | PickerValue::Nothing => None,
        }
    }
}

impl App {
    /// The buffer a subject is previewed as, and what git says about it.
    ///
    /// The same two answers the editor works from, so a preview of a
    /// commit's file carries the message above it and the margin beside it
    /// that opening the row would give: a preview that showed something
    /// else would be a promise Obelus does not keep.
    fn read(&self, subject: &Subject) -> Option<(Buffer, Option<obelus_git::Changes>)> {
        match subject {
            Subject::File(path) => {
                // The text as the reader has it, wherever they have it. A
                // file open in Obelus is open with what Obelus holds, which
                // is what choosing the row gives them -- and the disk stops
                // being the same document the moment anything writes to it
                // and Obelus has not read it again.
                let buffer = match self.open_file_at(path, None) {
                    Some(open) => Buffer::from_text(path, &open.text().rope().to_string()),
                    // A file that has gone, or one this reader cannot read.
                    // No preview rather than a message: the list is the
                    // subject here.
                    None => Buffer::open(path)
                        .inspect_err(|error| tracing::debug!(%error, "no preview"))
                        .ok()?,
                };
                let changes = obelus_git::head(path)
                    .map(|mut committed| committed.changes(&buffer.text().rope().to_string()));
                Some((buffer, changes))
            }
            Subject::Commit { id, path } => {
                let text = obelus_git::history::text_at(&self.working_directory, *id, path)?;
                let mut buffer = Buffer::at_commit(path, *id, &text);
                if let Some(said) = self.said_at(*id) {
                    buffer.open_held(
                        obelus_text::coordinates::LineNumber::new(0),
                        &said,
                        obelus_buffer::Held::Message,
                    );
                    if let Some(changed) = self.changed_at(*id, path) {
                        buffer.mark_block_change(
                            obelus_text::coordinates::LineNumber::new(0),
                            changed,
                        );
                    }
                    // Whole, not the first five lines. This is the only
                    // place the message is shown at all now -- opening the
                    // row gives the file and nothing else -- so a preview
                    // that cut it off would be Obelus hiding the one thing
                    // it still has to say about the commit. The reader
                    // scrolls it like anything else, and the rule under it
                    // says where the file starts.
                    //
                    // Landing at its top, which is where the commit says
                    // what it is: a preview that started at the file's
                    // first line would show the end of the message and call
                    // it the beginning of the file.
                    buffer.enter_block(obelus_text::coordinates::LineNumber::new(0));
                }
                // Against nothing where the commit added the file, as the
                // file opened at that commit is.
                let before = obelus_git::history::text_before(&self.working_directory, *id, path)
                    .unwrap_or_default();
                let changes = Some(obelus_git::Changes::between(
                    &before,
                    &buffer.text().rope().to_string(),
                ));
                Some((buffer, changes))
            }
            // A message on its own, with no file under it: a commit is not
            // a file, and what it has to show is what it said. Nothing to
            // fold it away in favour of, either, because the message is the
            // whole of what is there.
            //
            // The count is of the whole commit, because that is what this row
            // is: in a file's history the same number is about the file, and
            // in both places it answers the question the row asks. About two
            // milliseconds for a commit of this project's size, paid once
            // when the selection lands rather than per keystroke.
            Subject::Message(id) => {
                let said = self.said_at(*id)?;
                let mut buffer = Buffer::from_message(&said);
                if let Some(changed) = obelus_git::history::counted_in(&self.working_directory, *id)
                {
                    buffer.mark_block_change(obelus_text::coordinates::LineNumber::new(0), changed);
                }
                Some((buffer, None))
            }
        }
    }

    /// Where a version of a file should be shown: where it is being read if
    /// it is open, and at the top if it is not.
    ///
    /// The same answer the list of open files gives, because it is the same
    /// question -- a file's place in it is the thing a reader remembers it
    /// by, and choosing the row takes them back to exactly that. A list
    /// that previewed the top of a file the reader is twenty screens into
    /// would show them somewhere they have not been for an hour.
    ///
    /// `at` says which version: a commit's, or the one on disk. Both are
    /// asked for, because two buffers can wear one path -- a file and that
    /// same file as some commit had it -- and matching on the path alone
    /// would show a reader the other one's place in it.
    fn read_at(&self, path: &Path, at: Option<gix::ObjectId>) -> Marked {
        self.open_file_at(path, at)
            .map_or_else(Marked::top, |buffer| Marked::on(&buffer.cursor()))
    }

    /// What is known to be wrong with what a preview is showing, placed in
    /// the text it is showing.
    ///
    /// Placed against the preview's own buffer rather than taken from the
    /// placed copy Obelus keeps: that one is counted against the text of a
    /// file Obelus has open, and most of what a preview shows is a file
    /// nobody has opened.
    ///
    /// Nothing for a commit's version of a file. What a server said is
    /// about the file on disk, and hanging it on the bytes of an old commit
    /// would put the words under a line that was never the one complained
    /// about.
    fn wrong_in(&self, subject: &Subject, buffer: &Buffer) -> Vec<obelus_lsp::trouble::Trouble> {
        let Subject::File(path) = subject else {
            return Vec::new();
        };
        let encoding = buffer
            .language()
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        self.reported
            .get(path)
            .into_iter()
            .flatten()
            .map(|trouble| trouble.placed(buffer.text(), &encoding))
            .collect()
    }

    /// The document holding a file, where Obelus has one open.
    ///
    /// The content as well as the path: a commit's version of a file and
    /// the file itself share a path and are two documents, so a row about
    /// one must not be answered with the other.
    fn open_file_at(&self, path: &Path, at: Option<gix::ObjectId>) -> Option<&Buffer> {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .find(|buffer| buffer.path() == path && buffer.content().at() == at)
    }

    /// The file being read and the line it is being read at, as a preview's
    /// subject.
    fn reading_now(&self) -> Option<(Subject, Marked)> {
        let buffer = self.current_buffer()?;
        let subject = match buffer.content().at() {
            Some(id) => Subject::Commit {
                id,
                path: buffer.path().to_path_buf(),
            },
            None => Subject::File(buffer.path().to_path_buf()),
        };
        Some((subject, Marked::on(&buffer.cursor())))
    }
}

/// What a preview is of.
///
/// Not a path, because a row does not always name a file on disk: a commit
/// names a message, and one of a commit's files names the file as that
/// commit had it -- a different document from the one at the same path in
/// the working tree. The preview shows what choosing the row would give,
/// which is the whole point of a preview, so it has to be able to say the
/// same things a buffer can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Subject {
    /// A file on disk.
    File(PathBuf),
    /// A file as a commit had it.
    Commit {
        /// Which commit.
        id: gix::ObjectId,
        /// Which file, by the name it has on disk.
        path: PathBuf,
    },
    /// What a commit said about itself.
    Message(gix::ObjectId),
}

/// A file read so that the picker's selection can be shown.
#[derive(Debug)]
pub(super) struct Preview {
    subject: Subject,
    buffer: Buffer,
    highlights: Highlights,
    /// What a server says is wrong with it, placed in this buffer's text.
    ///
    /// Worked out when the file is read, like the changes beside it: a
    /// preview is a snapshot of somewhere else, and what is wrong with a
    /// file nobody is editing does not change while it is being looked at.
    troubles: Vec<obelus_lsp::trouble::Trouble>,
    /// What git says about this file, so the preview carries the same
    /// margin the editor does.
    ///
    /// Worked out once, when the file is read: a preview is a snapshot of
    /// somewhere else, and the diff of a file nobody is editing does not
    /// change while it is being looked at.
    changes: Option<obelus_git::Changes>,
    /// The room its text was laid out in.
    ///
    /// Kept rather than worked out again wherever it is wanted: the width
    /// takes off the change map and the bar as well as the gutter, and a
    /// second sum that forgot either would wrap the preview at a column
    /// wider than the room it has -- which the note where it is worked out
    /// is already about. What reads it is the box that says what is wrong
    /// with the line, which has to land on the same row the text did.
    text: obelus_buffer::TextArea,
    /// What is wrong with the line this preview is showing, where the list
    /// showing it is about a problem.
    ///
    /// The box that says so is floated over the preview, the way the
    /// editor floats one over the caret's line. Carried here rather than
    /// worked out at the drawing because two things settle it and neither
    /// is the drawing: which list is up, and which of that line's troubles
    /// the row is about.
    complaint: Option<super::Complaint>,
    /// The parts of it the selection is about, once converted.
    marked: Vec<Span>,
    /// Which part of the file the selection is about, as it arrived.
    ///
    /// Kept so that moving to a different row can be told from redrawing the
    /// same one, which is when the scrolling below is forgotten.
    target: (u32, u32),
    /// How many rows the reader has scrolled the preview by.
    ///
    /// Held here rather than in the buffer's viewport because the viewport is
    /// set from the target on every frame: without this, a scroll would be
    /// undone before it was drawn.
    scrolled: isize,
}

impl Preview {
    fn language(&self) -> Option<LanguageId> {
        self.buffer.language()
    }
}

/// The part of a file a selection is about, in the protocol's units.
#[derive(Clone, Debug)]
pub(super) enum Marked {
    /// A run a language server named, in whichever units it agreed to.
    Span {
        /// Where it starts.
        line: u32,
        /// And how far along.
        character: u32,
        /// Where it ends.
        end_line: u32,
        /// And how far along that.
        end_character: u32,
    },
    /// The characters a query matched, as columns of the row's own text.
    ///
    /// Which is not the same as columns of the line: a row is the line
    /// trimmed of its indentation, because the indentation is the same on
    /// every row of a block. What the indent was is worked out from the
    /// file, which is the only place it still exists.
    Matched {
        /// Which line of the file the row was.
        line: u32,
        /// Which characters of the row matched.
        columns: Vec<u32>,
    },
    /// A line, with nothing on it to mark.
    ///
    /// Which is what a row that names a whole file is about: the top of one
    /// Obelus has never opened, and wherever the reader is in one it has.
    At {
        /// Which line of the file, counted from zero.
        line: u32,
    },
}

impl Marked {
    /// The top of a file, with nothing to mark.
    const fn top() -> Self {
        Self::At { line: 0 }
    }

    /// The line a cursor is on, with nothing to mark.
    fn on(cursor: &obelus_buffer::Cursor) -> Self {
        Self::At {
            line: u32::try_from(cursor.line.get()).unwrap_or(u32::MAX),
        }
    }

    /// Which place in the file this is about, as it arrived.
    ///
    /// What tells one row from another: a preview that cannot tell them
    /// apart forgets the reader's scrolling on every redraw, or never.
    fn at(&self) -> (u32, u32) {
        match self {
            Self::Span {
                line, character, ..
            } => (*line, *character),
            Self::Matched { line, columns } => (*line, columns.first().copied().unwrap_or(0)),
            Self::At { line } => (*line, 0),
        }
    }

    /// Which line of the file the preview should be looking at.
    const fn line(&self) -> u32 {
        match self {
            Self::Span { line, .. } | Self::Matched { line, .. } | Self::At { line } => *line,
        }
    }

    /// The runs to mark, in Obelus's own coordinates.
    fn resolve(
        &self,
        text: &obelus_text::Text,
        encoding: &lsp_types::PositionEncodingKind,
    ) -> Vec<Span> {
        match self {
            Self::At { .. } => Vec::new(),
            Self::Span {
                line,
                character,
                end_line,
                end_character,
            } => {
                let at = |line, character| {
                    position::from_lsp(text, lsp_types::Position { line, character }, encoding)
                };
                let (line, column) = at(*line, *character);
                let (end_line, end_column) = at(*end_line, *end_character);
                // An empty span marks nothing: a file preview has no symbol
                // in it.
                if (line, column) == (end_line, end_column) {
                    return Vec::new();
                }
                vec![Span {
                    line,
                    column,
                    end_line,
                    end_column,
                }]
            }
            Self::Matched { line, columns } => {
                let line = text.clamp_line(LineNumber::new(*line as usize));
                // The row was the line without its indentation, so the
                // columns are that much further along the line itself.
                let indent = text
                    .line(line)
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .count();
                runs(columns)
                    .into_iter()
                    .map(|(first, end)| Span {
                        line,
                        column: CharColumn::new(indent + first),
                        end_line: line,
                        end_column: CharColumn::new(indent + end),
                    })
                    .collect()
            }
        }
    }
}

/// The runs of consecutive columns in a sorted list of them.
///
/// A fuzzy match lands on scattered characters, and a run of them is one
/// mark rather than one per character: the marking is a background, and
/// three adjacent backgrounds are one shape anyway.
fn runs(columns: &[u32]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for column in columns {
        let column = *column as usize;
        match runs.last_mut() {
            Some(last) if last.1 == column => last.1 = column + 1,
            _ => runs.push((column, column + 1)),
        }
    }
    runs
}

/// How many screenfuls a key scrolls the preview by.
///
/// The bare keys, because a screenful at a time is what a *reading* is
/// paged by and the preview is the thing on screen being read: the list
/// above it is ten rows walked one at a time, and its ends are a keypress
/// away. The same keys with control page the list, for a reader in a list
/// long enough to need it.
fn preview_paging(key: &KeyEvent) -> Option<isize> {
    if !keymap::modifiers_of(key)?.is_empty() {
        return None;
    }
    match key.code {
        KeyCode::PageDown => Some(1),
        KeyCode::PageUp => Some(-1),
        _ => None,
    }
}
