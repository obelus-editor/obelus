//! What a frame is drawn from, worked out once before it is drawn.

use super::*;

impl App {
    /// The area a list is drawn in.
    ///
    /// The whole of the region, a question the agent is waiting on
    /// included: the drawing's own answer, which this has to be the same as
    /// or the rows a key moves through are not the rows on screen.
    ///
    /// An area and not a [`layers::Room`]: a room is how much of the screen
    /// a view declares it takes, and this is the rectangle that comes out of
    /// laying one out.
    pub(super) fn picker_area(&self) -> Rect {
        self.drawn_in()
    }

    /// The region a view drawn over the file is drawn in.
    ///
    /// Worked out from the screen, the way the drawing works it out, rather
    /// than read from [`Self::editor_area`] -- which is what the *document*
    /// has, and a compact list takes room off it. Asked against that, a
    /// press in the command palette was measured from a rect ten rows above
    /// the one the palette had drawn itself in: its tabs answered nothing,
    /// and its rows answered about the wrong ones.
    pub(super) fn drawn_in(&self) -> Rect {
        obelus_ui::regions(self.screen_area).editor
    }

    /// How far the file's view has travelled altogether, in screen rows.
    #[must_use]
    pub const fn travelled(&self) -> i64 {
        self.travelled
    }

    /// The room the text has, once the gutter has taken its columns.
    ///
    /// Public because a test of what the view does when the cursor reaches
    /// the right-hand edge has to know where that edge is, and working it
    /// out again in the test would be the same arithmetic twice.
    pub fn text_area(&self) -> TextArea {
        let width = match self.current_buffer() {
            Some(buffer) => {
                // The margin on the left and the change map on the right
                // both appear only for a file in a repository, and they
                // appear together: they are the same answer at two scales.
                // The fold column is its own condition, and it is asked
                // through `text_offset` so that this and the view cannot
                // disagree about what comes before the text -- a width one
                // cell wider than the view draws wraps a line here and not
                // there, and the caret then sits a row below the character
                // it is on.
                let before = obelus_ui::editor::text_offset(
                    buffer.text().line_count(),
                    obelus_ui::editor::changed(self.changes()),
                    !buffer.folds().is_empty(),
                );
                let after = obelus_ui::editor::map_width(self.changes());
                self.editor_area
                    .width
                    .saturating_sub(before)
                    .saturating_sub(after)
                    .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH)
            }
            None => self.editor_area.width,
        };
        TextArea {
            width,
            height: self.editor_area.height,
            wrap: self.settled.config.wrap,
        }
    }

    /// Records the geometry, scrolls the cursor on screen, and highlights what
    /// that leaves visible.
    ///
    /// In this order: the highlight range depends on where the viewport ended
    /// up, so scrolling has to have happened.
    ///
    /// Private: it has to run before the frame is drawn, and the only thing
    /// that knows that is [`App::draw_into`], which is the one caller.
    fn prepare(&mut self, editor_area: Rect) {
        // What this frame has, before anything is laid out against it.
        // Everything below asks for the room through `editor_area`, so
        // setting it afterwards laid the notes out at the width the *last*
        // frame had -- right on a screen that is not changing, and wrong on
        // every frame that changed it: the view opening, a terminal
        // resized, a region growing as something over it closes. The next
        // redraw put it right, so what a reader saw was their words go and
        // come back.
        self.editor_area = editor_area;
        // A terminal is told the size it is drawn at before anything else
        // looks at it, for the reason the notes are laid out against this
        // frame's room: a program told a size a frame late draws its screen
        // once at the old one.
        self.size_the_terminal(editor_area);
        self.note_where_the_view_has_got_to();
        // What the views showing are drawn from, and what Obelus has to be
        // told about it. First, because everything below this reads one of
        // those kept answers -- the notes' marks are worked out a dozen
        // lines down -- and a watch taken at the end of the frame is a view
        // that draws its first frame from whatever was there last time.
        self.settle_the_watches();
        // And what the settings page says about the chat, which can move
        // under it the way the settings can.
        self.settle_the_remote_page();
        // And the connection to that chat, from the same answer: which one
        // is set.
        self.settle_the_connection();
        // Or the window this one is heard in it through, where another has
        // it.
        self.settle_the_relay();
        // And a thread for every conversation there that can be named.
        self.settle_the_threads();
        // And the sessions, from the same question: which conversation is
        // on screen.
        self.settle_the_sessions();
        // And what is open, written down where it has changed, for the
        // same reason: there are a dozen ways a document opens or closes.
        self.write_down_what_is_open();
        // And what this window is reading, for the others' lists of the
        // worktrees, for the same reason.
        self.say_what_this_window_is_reading();
        // The notes are laid out against the room they have: a terminal is
        // resized and a setting is changed while they are open, and the rows
        // they are made of depend on both.
        let laid = self.notes_laid_out();
        // And which of them another Obelus has the conversation of, which
        // decides what the keys will change and so which of them the foot
        // offers. Beside the room for the same reason: both are the page's
        // answer to something outside it that moves while it is open.
        let elsewhere = self.which_notes_are_elsewhere();
        if let Some(notes) = self.notes_mut() {
            notes.lay_out(laid.0, laid.1);
            notes.these_are_elsewhere(elsewhere);
        }
        // And the window against the rows that room leaves, which is the
        // other half of the same question: the width says what the rows
        // are and this says how many of them the reader can see. Asked
        // after the laying out, because the rows have to exist before the
        // window can be put over them, and from the region the list is
        // really drawn in -- the foot under it is part of what decides how
        // many rows there are, and it grows and shrinks with what the note
        // under the caret can do.
        let seen = self.notes().map(|notes| {
            obelus_ui::todo::list_region(self.editor_area, &obelus_ui::todo::hints(notes)).height
        });
        if let (Some(seen), Some(notes)) = (seen, self.notes_mut()) {
            notes.settle_window(seen);
        }
        self.check_servers();
        self.check_runs();
        self.show_what_is_wrong();
        // The marks on a list of open documents, which say what an agent is
        // doing in a conversation nobody is watching. Here rather than
        // where the list is built, because that is the whole point: the
        // rows are a snapshot and this is the part of them that is about
        // now.
        self.freshen_the_document_marks();
        // And what a row of the list of conversations says about itself,
        // which is the same rule one level along: another Obelus opening
        // or closing one is not this reader's keystroke, and the row has
        // to say so before they press.
        self.freshen_the_conversation_rows();
        // And the list of pull requests, whose rows carry the same lock.
        self.freshen_the_pull_request_rows();
        // What the conversation says is happening, read off the state
        // rather than remembered: a row that is worked out every frame
        // cannot be left saying something that stopped being true.
        let doing = match self.talking() {
            Talking::Starting => Some("Starting\u{2026}"),
            Talking::Thinking => Some("Thinking\u{2026}"),
            Talking::Nobody | Talking::Idle | Talking::Ready | Talking::Gone => None,
        };
        let can = self.talking() == Talking::Thinking && self.ctrl_enter_arrives;
        self.in_transcript(|chat| {
            chat.doing(doing);
            chat.can_send_now(can);
        });
        self.show_what_is_running();
        // Only the animation, which is what the ticker is for. Everything
        // else that once rode this question waits on a clock of its own:
        // work that is owed is owed on a machine with nothing moving on it.
        self.animate(self.wants_animating(doing.is_some()));
        // Which for a tree behind its text is asked from what is true
        // rather than started where a document changes -- the same way the
        // ticker was asked, and what stops a clock outliving its reason.
        self.catch_up_soon();

        // Which rows the list will draw is what decides which rows need
        // their matched characters worked out, and only the geometry knows
        // how many rows there are. The room is the room it is *drawn* in,
        // which over a conversation is everything above the box.
        let width = self.picker_area().width;
        let rows = self
            .picker
            .as_ref()
            .map(|picker| obelus_ui::picker::rows_drawn(picker, self.picker_area()));
        if let (Some(rows), Some(picker)) = (rows, self.picker.as_mut()) {
            picker.refresh_indices(rows, width);
        }
        // And whether its last row is on screen, which is what asks for a
        // list fetched a page at a time to go on -- here, with the window
        // settled, for the same reason.
        if let Some(rows) = rows {
            self.reach_further(rows);
        }
        // And a scoring the list wants done somewhere that is not here.
        // Taken on the frame rather than where the query changed, for the
        // reason the indices above are: one place asks, so a path that
        // changes a query cannot forget to.
        self.send_the_scan();
        // The same question for the list a setting's names are built in,
        // and the same reason: its window moves when the rows about to be
        // drawn say where it goes.
        let building = self
            .names
            .as_ref()
            .map(|(_, names)| obelus_ui::names::rows_drawn(names, self.picker_area()));
        if let (Some(rows), Some((_, names))) = (building, self.names.as_mut()) {
            names.settle_window(rows);
        }
        // The window over the projects, while Obelus is asking which: the
        // rule every window follows, on the page's own count of the rows it
        // has.
        if self.chooser.is_some() {
            let rows = self.chooser_rows();
            if let Some(chooser) = self.chooser.as_mut() {
                chooser.settle(rows);
            }
        }
        // Unconditionally, because with no list open the geometry is `None`
        // and the trees parsed for the last one are what has to be let go.
        self.colour_visible_rows(rows.unwrap_or(0));

        // Before anything is drawn or measured: the theme decides colours
        // only, but the preview is the application wearing it, and a frame
        // drawn half in one theme is a frame nobody should see.
        self.preview_theme();

        // A file the search is listing can be rewritten under it -- by an
        // agent, which is the ordinary case here -- and the rows are the
        // lines of one version of it. Checked per frame rather than per
        // keystroke because nothing the reader does is what changed it.
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.is_searching() && picker.row_count() > 0)
            && self.searching() == Some(Scope::File)
            && self.searched
                != self
                    .current_buffer()
                    .map(|buffer| (buffer.path().to_path_buf(), buffer.version()))
        {
            self.search_this_file();
        }

        self.settle_agents(editor_area);
        self.prepare_icons();
        self.settle_chat(editor_area);
        self.refresh_slash();
        // The same thing for the list of what could finish a path: the
        // window follows the selection only once it knows how many rows
        // are on screen, and the matched characters are worked out for
        // the rows about to be drawn. No disk is touched -- the rows are
        // whatever the last directory read found.
        self.settle_the_naming_list();

        // Less the scrollbar's column, which the drawing keeps for itself:
        // a reading laid out for the whole width would have its last cell
        // clipped, and a box drawn round a block of code would lose the
        // side that closes it.
        // The panel is checked against the document rather than told about
        // every way the document can move: a cursor that has left the word
        // is a panel about somewhere else.
        self.settle_completion();
        // And the answer about a place, which the pointer resting is what
        // asks for: this is where the resting is noticed.
        self.settle_hover();
        // And what the call under the caret takes, which is the third of the
        // panels that belong to a place in the file and the last of them to
        // be asked this. It went only when a view opened over it, so a
        // reader who arrowed off the line kept a panel about a call that was
        // no longer under them.
        self.settle_signature();
        if let Some(hover) = self.hover.as_mut() {
            // What it is drawn in, so that paging it moves what is on
            // screen rather than a number nothing reads.
            hover.settle(
                obelus_ui::hover::room(editor_area),
                obelus_component::hover::MOST_ROWS,
            );
        }
        // How much room the panel's two halves have, which the keys need
        // as much as the drawing does: a page of documentation is the rows
        // of it that are on screen, and only the geometry knows how many
        // that is.
        if let Some(panel) = obelus_ui::complete::layout(self, editor_area)
            && let Some(completion) = self.completion.as_mut()
        {
            // The width inside the box, less the column the reading keeps
            // for its scrollbar: laid out for cells it does not get, the
            // last of every row would be clipped.
            completion.settle_documentation(
                panel
                    .area
                    .width
                    .saturating_sub(2)
                    .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH),
            );
            completion.settle(panel.list, panel.documentation);
        }

        self.refresh_rendering(
            editor_area
                .width
                .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH),
        );
        self.refresh_changes();
        self.refresh_blame();
        // After the changes, because the room the text has includes the
        // rows an opened hunk draws: the arithmetic counts them, so nothing
        // here has to make up for them.
        let area = self.text_area();
        // Only where the viewport is a place in the *text*. While a reading
        // is showing, the viewport's top is a row of that reading -- and a
        // reading has more rows than the file has lines, because it wraps
        // -- so the text's own arithmetic would clamp the top to the line
        // count and put the last rows out of reach. The reading's own
        // scrolling is what keeps it in bounds there.
        if let Some(buffer) = self
            .current_buffer_mut()
            .filter(|buffer| buffer.mode() == obelus_buffer::Mode::Edit)
        {
            buffer.scroll_into_view(area);
        }

        self.refresh_preview(editor_area);
        self.look_at_the_selection();

        let painted = obelus_ui::editor_canvas(self.screen_area).height;
        let Self {
            documents,
            current,
            highlights,
            ..
        } = self;
        let Some(buffer) = current
            .and_then(|id| documents.get(id.get()))
            .and_then(Option::as_ref)
            .and_then(Document::file)
        else {
            highlights.clear();
            return;
        };
        let Some(state) = buffer.syntax() else {
            highlights.clear();
            return;
        };
        // Over the rows that are *painted*, not the rows the reader has.
        // A compact list covers the foot of the document rather than
        // shortening it -- see `obelus_ui::editor_canvas` -- and what is
        // highlighted has to be what is drawn, or the rows under the list
        // come out in the plain foreground and a window shows them that
        // way through the glass.
        let range = buffer.visible_bytes(painted);
        highlights.refresh(state, buffer.text(), range);
    }

    /// Lays the screen out, scrolls the cursor into view, draws, and says
    /// where the terminal should put its cursor.
    ///
    /// The one path to a rendered frame, shared by the loop and the golden
    /// tests. Laying out here means it happens inside `Terminal::draw`, which
    /// is allowed: it is arithmetic over sizes, not work.
    pub fn draw_into(&mut self, cells: &mut CellBuffer, area: Rect) -> Option<Position> {
        self.screen_area = area;
        self.prepare(obelus_ui::editor_room(area, self));
        let left = obelus_ui::draw(cells, area, self);
        self.bars = left.bars;
        self.links = left.links;
        // With the frame rather than with the key that changed it: what
        // the caret is doing depends on where it ended up, which is not
        // known until the frame has been laid out.
        if let Some(drawing) = self.drawing.as_ref() {
            drawing.caret_is(self.caret(), self.layers().nearest(), self.takes_text());
        }
        obelus_ui::cursor_position(area, self)
    }
}
