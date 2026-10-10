//! What the pointer does: presses, drags, the wheel, and the bars.

use super::*;

impl App {
    /// What the pointer did to a box on the status row, and whether it was
    /// one of those.
    ///
    /// The nearest thing on screen, so it is asked first -- and it answers
    /// for every click on that row, including one that lands past the end
    /// of what was typed: the row is the box, and a click on it is a click
    /// in the box.
    fn pointer_on_status(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use crate::event::Pointer;

        let status = obelus_ui::regions(self.screen_area).status;
        if status.height == 0 || y != status.y || x < status.x || x >= status.right() {
            return false;
        }
        // The agent's settings, which are what this row carries while a
        // conversation is what the screen is showing. Each is a word saying
        // what the session is set to, and each is one key away -- so a
        // press on one goes to it and does what that key does: a switch
        // flips, and one with a list behind it opens the list.
        if kind == Pointer::Pressed
            && !self.layers().any()
            && let Some(view) = obelus_ui::chat::ChatView::new(self)
            && let Some(at) = view.setting_at(status, x, y)
        {
            if let Some(talk) = self.conversation_mut() {
                talk.chat.stand_on_setting(at);
            }
            self.chat_key(&enter());
            return true;
        }
        // Which box is showing, and how far in its text starts. The same
        // order the keys go in by, and the same insets the renderer draws
        // them at.
        //
        // The list of names before the settings, because it is opened from
        // them and drawn over them; and the page asking which project
        // last, because every one of the others is drawn over it.
        let inset = if self.prompt.is_some() {
            self.prompt.as_ref().map(obelus_ui::status::answer_inset)
        } else if self.names.is_some() || self.settings.is_some() {
            Some(obelus_ui::status::typed_inset(None))
        } else if self.picker.is_some() {
            self.picker
                .as_ref()
                .map(|picker| obelus_ui::status::typed_inset(picker.question()))
        } else {
            self.which_project.chooser.as_ref().map(|chooser| {
                obelus_ui::status::typed_inset(Some(obelus_ui::status::choosing_question(
                    chooser.is_naming(),
                )))
            })
        };
        let Some(inset) = inset else {
            return false;
        };
        let cell = (x - status.x).saturating_sub(inset);
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        match kind {
            // Nothing to do, but the row is still the box's: a move over it
            // must not reach the file underneath.
            Pointer::Moved | Pointer::Released => {}
            Pointer::Dragged => self.place_on_status(cell, true),
            Pointer::Pressed => {
                self.place_on_status(cell, false);
                // Twice is the word and three times is the whole of it,
                // which is what a line has instead of a line.
                match clicks {
                    2 => self.hold_on_status(false),
                    3 => self.hold_on_status(true),
                    _ => {}
                }
            }
        }
        true
    }

    /// What the pointer did to the box a note is written in.
    ///
    /// Where the box is on screen is the view's to say, so it says it --
    /// the same function that puts the caret there, read backwards.
    fn pointer_in_notes(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = self.editor_area;
        // The mark and the box first, which are the two things a row of the
        // list draws that the reader can *do* something to: the box says
        // whether the note is done, and the mark says somebody has talked
        // about it. Both are one key away and both are a picture of that
        // key, so a press on one does what the key does.
        if kind == Pointer::Pressed && self.press_in_a_note(x, y) {
            return;
        }
        let Some(at) = self
            .notes()
            .and_then(|notes| obelus_ui::todo::place_at(area, notes, x, y))
        else {
            return;
        };
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        let Some(notes) = self.notes_mut() else {
            return;
        };
        let width = notes.caret_width();
        let Some(composer) = notes.writing_mut() else {
            return;
        };
        match kind {
            Pointer::Moved | Pointer::Released => {}
            Pointer::Dragged => composer.place_at_cell(at.0, at.1, width, true),
            Pointer::Pressed => {
                composer.place_at_cell(at.0, at.1, width, false);
                match clicks {
                    2 => composer.hold_word(width),
                    3 => composer.hold_line(width),
                    _ => {}
                }
            }
        }
    }

    /// What the pointer did to the box a message is written in.
    fn pointer_in_chat(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = self.editor_area;
        // Worked out before the conversation is borrowed to change: the
        // card is the application's and the box is the conversation's.
        let carded = self.card().is_some();
        // The way back to the end, on the rule over the box: a press on it
        // is the key it names, through the same door the key goes through,
        // so the two cannot come to mean different things -- with the
        // cursor in the transcript, `ctrl+end` takes the cursor along.
        if kind == Pointer::Pressed
            && self
                .conversation()
                .and_then(|talk| obelus_ui::chat::way_back_at(area, &talk.chat, talk.card.as_ref()))
                .is_some_and(|at| at.contains(ratatui::layout::Position { x, y }))
        {
            self.chat_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::End,
                crossterm::event::KeyModifiers::CONTROL,
            ));
            return;
        }
        // The agent's commands, which are drawn over the transcript above
        // the box while one is being typed.
        if kind == Pointer::Pressed && self.press_in_the_commands(x, y) {
            return;
        }
        // The card first, where one is up: it is what covers the box, and
        // every row of it is a thing the reader answers with. What lands
        // above it is still the transcript, so a question on screen does
        // not stop the reader taking a copy of what led to it.
        if carded && self.press_in_card(kind, x, y) {
            return;
        }
        let Some(at) = self
            .conversation()
            .and_then(|talk| obelus_ui::chat::ChatView::place_at(area, &talk.chat, carded, x, y))
        else {
            self.pointer_in_transcript(kind, x, y);
            return;
        };
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        let width = obelus_ui::chat::writing_width(area);
        let mut held = false;
        self.in_transcript(|chat| {
            match kind {
                Pointer::Moved | Pointer::Released => {}
                Pointer::Dragged => chat.writing_mut().place_at_cell(at.0, at.1, width, true),
                Pointer::Pressed => {
                    // One selection between the two halves, and this is
                    // the other half taking hold -- and the keys with it,
                    // or the caret is put where nothing typed would go.
                    held = true;
                    chat.stand_in_the_box();
                    let writing = chat.writing_mut();
                    writing.place_at_cell(at.0, at.1, width, false);
                    match clicks {
                        2 => writing.hold_word(width),
                        3 => writing.hold_line(width),
                        _ => {}
                    }
                }
            }
        });
        if held {
            self.in_transcript(obelus_component::chat::Chat::let_go);
        }
    }

    /// What the pointer did to a view drawn over the file.
    ///
    /// Which for a long time was nothing at all: the wheel reached these --
    /// it is its own event and goes to whichever layer is nearest -- and a
    /// press did not, so a reader could scroll a list of files and not
    /// point at one. The query box was the exception, and a telling one:
    /// it is on the status row, which is asked before this, so the box was
    /// clickable and the list under it was not.
    ///
    /// A press moves the selection and nothing else. What *chooses* a row
    /// is a second press on it, because these lists are opened over a file
    /// and drawn where a mis-aimed press would otherwise take the reader
    /// somewhere they did not ask to go -- and nobody aims a double click
    /// badly twice in the same cell. The one exception is a row's own
    /// arrow, which says the row opens: pressing that does what pressing
    /// the arrow means everywhere, and cannot take the reader anywhere,
    /// because the arrow is declared on the rows that open and on no
    /// others.
    fn pointer_in_a_layer(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        if kind != Pointer::Pressed {
            return;
        }
        let twice = self.clicks_at(x, y) == 2;
        // Spent by the second press, so that a third -- on the file the
        // second may have opened -- is a first press there, and not a line
        // taken hold of.
        if twice {
            self.clicked = None;
        }
        // Whichever is nearest the reader, which is the one drawn over the
        // others: the same order a key is offered in.
        let Some(layer) = self.layers().nearest_first().next() else {
            return;
        };
        match layer {
            // The status row, which was asked before this.
            obelus_component::layers::Layer::Prompt => {}
            obelus_component::layers::Layer::Picker => self.press_in_picker(x, y, twice),
            // Nothing yet: what a press would have to land on is a row of
            // two sections and a boundary between them, and a press that
            // guessed wrong would add a font the reader did not point at.
            // The keys do all of it, and a list nobody can click is not a
            // list that lies about what it does.
            obelus_component::layers::Layer::Names => {}
            obelus_component::layers::Layer::Counts => self.press_in_counts(x, y, twice),
            obelus_component::layers::Layer::Settings => self.press_in_settings(x, y, twice),
            // Two keys, and nothing to point at.
            obelus_component::layers::Layer::Gone => {}
        }
    }

    /// Walks to one of a view's tabs, by the shorter way round.
    ///
    /// The tabs wrap, so from where the reader is to where they pressed is
    /// at most half the tabs away -- which for every list Obelus has is one
    /// step. Walked rather than jumped because what a tab *costs* is the
    /// application's: a scope asks the search again, a radius walks the
    /// history again, a direction turns the calls round. Going the short
    /// way is what keeps a tab in between from being asked its question on
    /// the way past.
    pub(super) fn walk_to_tab(
        &mut self,
        now: usize,
        wanted: usize,
        count: usize,
        key: impl Fn(&mut Self, bool),
    ) {
        if count == 0 || wanted == now {
            return;
        }
        let forward = (wanted + count - now) % count;
        let backward = (now + count - wanted) % count;
        let (steps, onwards) = match forward <= backward {
            true => (forward, true),
            false => (backward, false),
        };
        for _ in 0..steps {
            key(self, onwards);
        }
    }

    /// A press in a list of rows to choose from, and whether it is the
    /// second of a double click.
    fn press_in_picker(&mut self, x: u16, y: u16, twice: bool) {
        // Where the list drew itself, not the room it was given: a compact
        // one takes as many rows as it needs against the foot of that room,
        // so the two are ten rows apart for a palette on a tall screen.
        let room = self.picker_area();
        let area = self
            .picker
            .as_ref()
            .map_or(room, |picker| obelus_ui::picker::region(picker, room));
        // The tabs, which are above the rows: pressing one is the only
        // thing a tab is for, so there is nothing else a press there could
        // have meant.
        let tab = self.picker.as_ref().and_then(|picker| {
            let row = obelus_ui::picker::tab_row(picker, area)?;
            let at = obelus_ui::tab_at(row, picker.tabs(), picker.tab(), x, y)?;
            Some((picker.tab(), at, picker.tabs().len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.picker_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, arrow)) = self
            .picker
            .as_ref()
            .and_then(|picker| obelus_ui::picker::row_at(picker, area, x, y))
        else {
            return;
        };
        if let Some(picker) = self.picker.as_mut() {
            picker.select_row(at);
        }
        if arrow || twice {
            // Down the same path the key goes down, rather than a second
            // opener of its own: what enter does to the row under the
            // arrow is what the arrow is a picture of, and two of them
            // would be two answers to keep alike.
            self.picker_key(&enter());
        }
    }

    /// A press in the table of what this project is made of.
    fn press_in_counts(&mut self, x: u16, y: u16, twice: bool) {
        let area = self.drawn_in();
        // The tabs are the table's first row.
        let tab = self.counts.as_ref().and_then(|counts| {
            let names = counts.tabs();
            let at = obelus_ui::tab_at(Rect { height: 1, ..area }, &names, counts.tab(), x, y)?;
            Some((counts.tab(), at, names.len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.counts_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, mark)) = self
            .counts
            .as_ref()
            .and_then(|counts| obelus_ui::counts::row_at(area, counts, x, y))
        else {
            return;
        };
        if let Some(counts) = self.counts.as_mut() {
            counts.select_row(at);
        }
        if mark || twice {
            self.counts_key(&enter());
        }
    }

    /// A press on the page that asks which project.
    ///
    /// The way every list goes: one press stands on a row and a second
    /// chooses it. Nothing here is drawn over anything, but choosing a
    /// project starts everything a project starts, and a press meant to
    /// look at a path should not.
    ///
    /// Nothing while a path is being named: the rows are still drawn, but
    /// enter is the box's then, and a press on a project is not an answer
    /// to it.
    fn press_in_projects(&mut self, x: u16, y: u16) {
        if self
            .which_project
            .chooser
            .as_ref()
            .is_some_and(obelus_component::chooser::Chooser::is_naming)
        {
            return;
        }
        let Some(at) = self.what_is_being_chosen().and_then(|choosing| {
            obelus_ui::projects::row_at(self.drawn_in(), &choosing, &self.keymap, x, y)
        }) else {
            return;
        };
        let twice = self.clicks_at(x, y) == 2;
        if let Some(chooser) = self.which_project.chooser.as_mut() {
            chooser.select_row(at);
        }
        if twice {
            self.clicked = None;
            self.choosing_a_project(&enter());
        }
    }

    /// A press on what could finish the path being named.
    ///
    /// The way every list goes, one press to stand and a second to choose,
    /// and choosing here is what enter does: the row goes into the box.
    ///
    /// Answers whether the press was the list's, so that one beside it
    /// goes on to the page.
    fn press_in_the_naming_list(&mut self, x: u16, y: u16) -> bool {
        let Some(at) = self.which_project.naming_list.as_ref().and_then(|list| {
            let region = obelus_ui::picker::region(list, self.drawn_in());
            obelus_ui::picker::row_at(list, region, x, y)
        }) else {
            return false;
        };
        let twice = self.clicks_at(x, y) == 2;
        if let Some(list) = self.which_project.naming_list.as_mut() {
            list.select_row(at.0);
        }
        if twice {
            self.clicked = None;
            self.choosing_a_project(&enter());
        }
        true
    }

    /// A press on the agent's commands, while a name is being typed.
    ///
    /// One press stands on a row and a second chooses it, which is what
    /// enter does: the name goes into the box.
    ///
    /// Answers whether the press was the list's, so that one beside it
    /// goes on to the conversation.
    fn press_in_the_commands(&mut self, x: u16, y: u16) -> bool {
        let Some(at) = self.slash().zip(self.chat()).and_then(|(list, chat)| {
            let room = obelus_ui::chat::above_writing(self.editor_area, chat, self.card());
            let region = obelus_ui::picker::region(list, room);
            obelus_ui::picker::row_at(list, region, x, y)
        }) else {
            return false;
        };
        let twice = self.clicks_at(x, y) == 2;
        if let Some(list) = self.conversation_mut().and_then(|talk| talk.slash.as_mut()) {
            list.select_row(at.0);
        }
        if twice {
            self.clicked = None;
            self.slash_key(&enter());
        }
        true
    }

    /// What the pointer did to the page of settings.
    ///
    /// Nothing folds there, so there is no arrow. What a press reaches is
    /// the switch: a box with a tick in it or without, which is the one
    /// thing on the page that says by its shape that pressing it changes
    /// it.
    fn press_in_settings(&mut self, x: u16, y: u16, twice: bool) {
        let area = self.drawn_in();
        // The tabs are the page's first row.
        let tab = self.settings.as_ref().and_then(|settings| {
            let names = settings.tabs();
            let at = obelus_ui::tab_at(Rect { height: 1, ..area }, &names, settings.tab(), x, y)?;
            Some((settings.tab(), at, names.len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.settings_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, switch)) =
            obelus_ui::settings::SettingsView::new(self).and_then(|view| view.row_at(area, x, y))
        else {
            return;
        };
        let offering = self.agent_offering();
        if let Some(settings) = self.settings.as_mut() {
            settings.select_row(at, offering.as_ref());
        }
        if switch || twice {
            self.settings_key(&enter());
        }
    }

    /// A press on one of the candidates a server offered.
    ///
    /// Choosing it outright, because that is what the list is for: it is
    /// up only while the reader is in the middle of typing a word, it
    /// covers the word it is about, and there is nothing in it to browse
    /// past -- a press anywhere else puts it away.
    ///
    /// Answers whether the press was the list's, so that one beside it goes
    /// on to the file.
    fn press_in_completion(&mut self, x: u16, y: u16) -> bool {
        let Some(panel) = obelus_ui::complete::layout(self, self.editor_area) else {
            return false;
        };
        let Some(at) = self
            .completion()
            .and_then(|completion| obelus_ui::complete::row_at(panel, completion, x, y))
        else {
            return false;
        };
        if let Some(completion) = self.lsp.completion.as_mut() {
            completion.choose_row(at);
        }
        // Down the key's own path, which is what takes the word and puts
        // the list away.
        self.completion_key(&enter());
        true
    }

    /// A press on one of the two marks a note wears.
    ///
    /// Answers whether it was one of them, so that a press on the words
    /// goes on to put the caret there.
    fn press_in_a_note(&mut self, x: u16, y: u16) -> bool {
        use obelus_ui::todo::Column;

        let area = self.editor_area;
        let Some((row, column)) = self
            .notes()
            .and_then(|notes| obelus_ui::todo::row_at(area, notes, x, y))
        else {
            return false;
        };
        if column == Column::Words {
            return false;
        }
        // On to the note first: both keys ask about the note the caret is
        // in, so the note under the pointer is the note they are about.
        let note = self
            .notes()
            .and_then(|notes| notes.rows().get(row))
            .map(|row| row.note);
        if let (Some(note), Some(notes)) = (note, self.notes_mut()) {
            notes.stand_on(note);
        }
        // Down the keys' own paths, rather than a second way to tick a
        // note off and a second way to open its conversation.
        match column {
            // The arrow, which the key reaches through the command rather
            // than through the notes' own keys -- so this goes the same
            // way, which is the point of going down a key's path at all.
            Column::Folds => {
                self.toggle_fold();
                return true;
            }
            Column::Tick => self.notes_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(' '),
                crossterm::event::KeyModifiers::ALT,
            )),
            _ => self.notes_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('a'),
                crossterm::event::KeyModifiers::ALT,
            )),
        };
        true
    }

    /// What the pointer did to a question on a card.
    ///
    /// A card is a question that has taken part of the screen and is
    /// waiting, and every row of it is a thing the reader answers with --
    /// so unlike a list drawn over a file there is nothing here to browse
    /// past, and a press does what the key does on the row it landed on.
    /// The words are the exception: they are a box, and a press in a box
    /// puts the caret where it landed.
    ///
    /// Answers whether the press was the card's, so that one landing above
    /// it falls through to the transcript.
    fn press_in_card(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use obelus_component::card::On;

        let Some(card) = self.card() else {
            return false;
        };
        let band = obelus_ui::chat::bands_for(self.editor_area, card).writing;
        let Some(on) = obelus_ui::card::row_at(card, band, x, y) else {
            return false;
        };
        let width = obelus_ui::card::width_of(band);
        // A drag over the words holds what it crosses, the way it does in
        // every other box. Anywhere else on the card there is nothing for
        // one to do: said so rather than let through, or a drag begun on
        // the card would take hold of the transcript behind it.
        if kind == crate::event::Pointer::Dragged && on == On::Words {
            let place = obelus_ui::card::place_at(card, band, x, y);
            if let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut())
                && let Some((row, cell)) = place
            {
                card.place_in_words(row, cell, width, true);
            }
            return true;
        }
        if kind != crate::event::Pointer::Pressed {
            return true;
        }
        let clicks = self.clicks_at(x, y);
        let area = self.editor_area;

        let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut()) else {
            return true;
        };
        card.stand_on(on);
        if on == On::Words {
            // Asked once the reader is standing in the words, because where
            // in them a point is depends on how they scroll under the
            // caret -- and a card says nothing about a caret it has not got.
            let band = obelus_ui::chat::bands_for(area, card).writing;
            let place = obelus_ui::card::place_at(card, band, x, y);
            if let Some((row, cell)) = place {
                card.place_in_words(row, cell, width, false);
                match clicks {
                    2 => card.hold_in_words(false, width),
                    3 => card.hold_in_words(true, width),
                    _ => {}
                }
            }
            return true;
        }
        // Down the key's own path: what enter does to the row under the
        // pointer is what that row is for, and a second answer about it
        // would be a second answer to keep alike.
        self.card_key(&enter());
        true
    }

    /// What the pointer did to what has been said.
    ///
    /// Which is the half of a conversation with no caret in it: there is
    /// nothing to type there, and the only thing a pointer does is take
    /// hold of some of it.
    ///
    /// One selection between the two halves, so taking hold here lets the
    /// box go. A reader dragging across an answer means that answer, and a
    /// second selection still lit in the box would leave `ctrl+c` with two
    /// things to copy and no way to say which.
    ///
    /// A double click on a tool call that names a file goes to the file.
    /// Not enter's answer on a call with something behind it, which is to
    /// fold -- a single press already folds, so going is the one thing
    /// left for the pair of them to mean, and the call is folded back the
    /// way it was before the first.
    fn pointer_in_transcript(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let twice = kind == Pointer::Pressed && self.clicks_at(x, y) == 2;
        if twice && let Some((place, folded)) = self.pressed_call.take() {
            if let (Some(begins), Some(talk)) = (folded, self.conversation_mut()) {
                talk.chat.fold(begins);
            }
            self.clicked = None;
            self.go_to_where_the_agent_was(&place);
            return;
        }
        let area = self.editor_area;
        let width = obelus_ui::chat::reading_width(area);
        // Laid out once, and every question below asked of the one place.
        let found = self.conversation().and_then(|talk| {
            let rows = talk.chat.rows(width);
            let place = obelus_ui::chat::ChatView::place_in_transcript(
                area,
                &talk.chat,
                talk.card.as_ref(),
                &rows,
                x,
                y,
            )?;
            let spot = obelus_ui::chat::ChatView::spot_in_transcript(&rows, place);
            let row = rows.get(place.row);
            // Whether it landed on a heading that opens, which is a thing to
            // do to the row rather than to the words in it.
            //
            // Free of the selection, and not by luck: every row that folds is
            // one Obelus drew itself -- the heading over a run of tool calls,
            // the one over a piece of thinking, the one over the agent's plan
            // -- and none of them is anybody's words. A press on one already
            // meant nothing but "let go", so opening it costs the reader
            // nothing they had.
            let folds = row.and_then(|row| row.folds);
            // What the frame drew under the pointer, which is the one
            // answer the underline and a window's hand are also made of.
            let link = obelus_ui::links::at(&self.links, x, y).map(str::to_string);
            // A cursor stands on a row, and the band under the last of them
            // is not one; nor while a card is up, which has the keys -- a
            // cursor moved under it would be found there afterwards.
            let cursor = (row.is_some() && talk.card.is_none()).then_some(place);
            // The file the call this row is part of names, which is on the
            // call's first row whichever row of its title was pressed.
            let goes = obelus_component::chat::Row::acting(&rows, place.row)
                .and_then(|acting| rows.get(acting.start))
                .and_then(|first| first.place.as_ref())
                .map(|(place, _)| place.clone());
            Some((spot, folds, cursor, link, goes))
        });
        let (spot, folds, cursor, link, goes) = found.unwrap_or((None, None, None, None, None));
        // A link is its own thing to press, and opens on the letting go.
        if kind == Pointer::Pressed {
            self.pressed_call = goes.filter(|_| link.is_none()).map(|place| (place, folds));
        }
        // Where the cursor goes, for a press or a drag: the keys follow
        // the pointer, or the arrows after a press walk something the
        // reader had not pointed at.
        //
        // And a drag only carries a cursor the press put here. The box is
        // under the transcript's band, so a drag in the box is one held
        // past its edge, and every tick hands it on as a drag on the
        // transcript's last row: one that moved the cursor took the keys
        // out of the box while the reader was selecting in it.
        let carried = kind == Pointer::Pressed
            || self.chat().is_some_and(|chat| {
                matches!(chat.focus(), obelus_component::chat::Focus::Transcript(_))
            });
        let Some(talk) = self.conversation_mut() else {
            return;
        };
        if matches!(kind, Pointer::Pressed | Pointer::Dragged)
            && carried
            && let Some(cursor) = cursor
        {
            talk.chat.stand_in_transcript(cursor);
        }
        match kind {
            Pointer::Moved => {}
            // A link opens on the letting go of a press that never moved:
            // on the press it would be a selection that could not be begun
            // on a link's words, and a drag across one is taking hold of
            // them, not following it.
            Pointer::Released => {
                let clicked = spot.is_some_and(|spot| talk.chat.clicked(spot));
                talk.chat.let_go_of_nothing();
                if clicked && let Some(link) = link {
                    self.open_link(&link);
                }
            }
            // A link on a row that folds is its own thing to press: the
            // title of a call that fetched a page is often the page's
            // address, and the rest of the row still folds.
            Pointer::Pressed if folds.is_some() && link.is_none() => {
                if let Some(begins) = folds {
                    talk.chat.fold(begins);
                }
            }
            Pointer::Pressed => {
                talk.chat.writing_mut().let_go();
                match spot {
                    Some(spot) => talk.chat.hold_from(spot),
                    // A press on nothing lets go, the way a press on the
                    // page does everywhere else.
                    None => talk.chat.let_go(),
                }
            }
            Pointer::Dragged => {
                if let Some(spot) = spot {
                    talk.chat.hold_to(spot);
                }
            }
        }
    }

    /// How far past the edge of what a drag is selecting in a row is, if
    /// it is past it at all.
    ///
    /// The band it asks about is the one the reader is dragging in: the
    /// transcript of a conversation, which has the box under it, and
    /// otherwise the whole region a file is read in.
    fn past_the_edge(&self, y: u16) -> Option<i16> {
        let area = self.editor_area;
        let band = match self.chat() {
            Some(chat) => obelus_ui::chat::bands(area, chat, self.card()).transcript,
            None => area,
        };
        let row = i32::from(y);
        let past = match (row < i32::from(band.y), row >= i32::from(band.bottom())) {
            (true, _) => row - i32::from(band.y),
            (_, true) => row - i32::from(band.bottom()) + 1,
            _ => return None,
        };
        i16::try_from(past).ok().filter(|past| *past != 0)
    }

    /// Keeps a drag held against an edge moving.
    ///
    /// Through the same one function a notch of the wheel goes through, so
    /// it reaches whatever the reader is dragging in -- and then the far
    /// end of the selection is put where the pointer is again, against the
    /// edge, because the rows under it have moved.
    pub(super) fn drag_on(&mut self) {
        let Some(drag) = self.dragging else {
            return;
        };
        let past = i32::from(drag.past);
        // The further past the edge, the faster: a pointer held still has
        // no other way to ask for more, and one row a tick is a minute to
        // cross a morning's conversation.
        let rows = past.signum() * (1 + past.abs().min(4));
        self.scroll(isize::try_from(rows).unwrap_or(0));
        // Against the edge rather than where the pointer really is, which
        // is off the band: what is being asked is "carry on to here", and
        // here is as far as the band goes.
        let area = self.editor_area;
        let band = match self.chat() {
            Some(chat) => obelus_ui::chat::bands(area, chat, self.card()).transcript,
            None => area,
        };
        let y = drag.y.clamp(band.y, band.bottom().saturating_sub(1));
        self.on_pointer(crate::event::Pointer::Dragged, drag.x, y);
        // Which said the drag had come back inside the band, it having
        // been handed a row that is. It has not: the reader is still
        // holding it out there.
        self.dragging = Some(drag);
    }

    /// Puts the caret of whichever box is on the status row.
    fn place_on_status(&mut self, cell: u16, extend: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.place_at_cell(cell, extend);
        } else if let Some((_, names)) = self.names.as_mut() {
            names.place_in_query(cell, extend);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.place_in_query(cell, extend);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.place_in_query(cell, extend);
        } else if let Some(chooser) = self.which_project.chooser.as_mut() {
            chooser.place_in_typing(cell, extend);
        }
    }

    /// Takes hold of a word of it, or of all of it.
    fn hold_on_status(&mut self, all: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.hold(all);
        } else if let Some((_, names)) = self.names.as_mut() {
            names.hold_in_query(all);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.hold_in_query(all);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.hold_in_query(all);
        } else if let Some(chooser) = self.which_project.chooser.as_mut() {
            chooser.hold_in_typing(all);
        }
    }

    /// Whether a press may take hold of this bar, with what is showing.
    ///
    /// What is showing owns the pointer as it owns the keys: with a list or
    /// a page over the document, a bar of the document's left in sight above
    /// it is still the document's, and the list puts the document back where
    /// its selection is on the next frame -- so the drag would be undone as
    /// it was made.
    fn reaches(&self, whose: obelus_ui::bars::Whose) -> bool {
        use obelus_ui::bars::Whose;

        match whose {
            // And the two lists that are not layers, which are drawn under
            // every layer with the page they belong to: under a short list
            // their bars are in plain sight and the keys are the list's.
            Whose::Document
            | Whose::Conversation
            | Whose::Notes
            | Whose::Projects
            | Whose::Naming
            | Whose::Commands => !self.layers().covering(),
            Whose::Picker
            | Whose::Preview
            | Whose::Settings
            | Whose::Counts
            | Whose::Names
            | Whose::Completion
            | Whose::Documentation
            | Whose::Hover => true,
        }
    }

    /// What the pointer did to a scrollbar, if it did anything to one.
    ///
    /// The bars are the ones the last frame left on the page, nearest the
    /// reader last -- so the last one under the pointer is the one they can
    /// see. A press off the mark brings the mark to it and goes on holding,
    /// which is one gesture wherever on the track it started.
    fn pointer_on_a_bar(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use crate::event::Pointer;

        match kind {
            Pointer::Moved => false,
            Pointer::Released => self.holding.take().is_some(),
            Pointer::Pressed => {
                // A release can go missing -- let go outside the window --
                // and a press is a fresh start whatever it lands on.
                self.holding = None;
                let Some(bar) = self
                    .bars
                    .iter()
                    .rev()
                    .find(|bar| bar.under(x, y) && self.reaches(bar.whose))
                else {
                    return false;
                };
                let grip = bar.grip(y);
                let (whose, top) = (bar.whose, bar.top_for(y, grip));
                self.holding = Some((whose, grip));
                if let Some(top) = top {
                    self.drag_bar(whose, top);
                }
                true
            }
            Pointer::Dragged => {
                let Some((whose, grip)) = self.holding else {
                    return false;
                };
                // The bar as the latest frame drew it, which may be none:
                // the list it was beside has closed under the pointer.
                let Some(bar) = self.bars.iter().rev().find(|bar| bar.whose == whose) else {
                    self.holding = None;
                    return true;
                };
                if let Some(top) = bar.top_for(y, grip) {
                    self.drag_bar(whose, top);
                }
                true
            }
        }
    }

    /// What the pointer did to the file being read.
    ///
    /// Only over the text, and only with nothing else open: a list, the
    /// settings or the conversation is what the screen is showing while it
    /// is up, and a click landing on the code behind one would move a caret
    /// nobody can see.
    pub(super) fn on_pointer(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        self.pointer = Some((x, y));
        if kind == Pointer::Pressed {
            self.pressed_in_the_file = false;
        }
        // A bar first, and whatever it is beside: a press on one is about
        // the bar and nothing under it, and while it is held every move is
        // the bar's -- wherever the pointer has wandered, the way a bar
        // held anywhere else behaves.
        if self.pointer_on_a_bar(kind, x, y) {
            self.dragging = None;
            return;
        }

        // Whether the pointer is being held past the edge of what it is
        // selecting in, which nothing else will say again until it moves:
        // a terminal reports a drag when it happens and says nothing at
        // all while a held pointer is still.
        match kind {
            Pointer::Dragged => {
                self.dragging = self.past_the_edge(y).map(|past| Dragging { past, x, y });
            }
            Pointer::Pressed | Pointer::Released => self.dragging = None,
            Pointer::Moved => {}
        }

        // The list of what could be typed next, where one is up: it is
        // drawn over the file at the caret, so it covers the very place a
        // press would otherwise land -- and a press that went through it to
        // the text would move the caret out from under the question the
        // list is answering.
        if kind == Pointer::Pressed && self.press_in_completion(x, y) {
            return;
        }
        // The boxes on the status row first: a question, a list's query, a
        // page's filter. All three are one row, so one piece of arithmetic
        // serves them -- and a reader who can select in a box with the
        // keyboard but not with the pointer has half a selection.
        if self.pointer_on_status(kind, x, y) {
            return;
        }
        // Covering rather than merely open: a question on the status bar
        // leaves every line of the file where the reader can see it, and a
        // line they can see is a line they can point at. Before the notes,
        // which are a document like the file: a page over them took the
        // press here, and it ticked off a note nobody could see.
        if self.layers().covering() {
            self.pointer_in_a_layer(kind, x, y);
            return;
        }
        // A terminal, whose program may have asked for the pointer itself.
        if self.terminal().is_some() {
            self.pointer_in_terminal(kind, x, y);
            return;
        }
        // The notes, which are a page with a box on it: the box takes the
        // pointer the way the file does, and the rest of the page takes
        // nothing rather than letting it through to the code behind.
        if self.notes().is_some() {
            self.pointer_in_notes(kind, x, y);
            return;
        }
        // A conversation is what is being read rather than something over
        // it, so it is asked here, where a file would be. The box is the
        // half of it with a caret in it; the transcript has none.
        if self.conversation().is_some() {
            self.pointer_in_chat(kind, x, y);
            return;
        }
        // The page that asks which project, which is drawn where the
        // welcome screen would be.
        if kind == Pointer::Pressed
            && self.reading_nothing()
            && self.which_project.chooser.is_some()
        {
            if !self.press_in_the_naming_list(x, y) {
                self.press_in_projects(x, y);
            }
            return;
        }
        // The welcome screen's website, the one thing on it a press opens.
        // Not on the page that asks which project, which is drawn where
        // the welcome screen would be and has no address on it.
        if kind == Pointer::Pressed
            && self.reading_nothing()
            && self.what_is_being_chosen().is_none()
            && obelus_ui::welcome::site_at(self.editor_area, &*self)
                .is_some_and(|at| at.contains(ratatui::layout::Position { x, y }))
        {
            if let Err(error) = obelus_clipboard::links::open(obelus_ui::welcome::SITE) {
                tracing::warn!(%error, "the website was not opened");
                self.say("Nothing here opens links");
            }
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // A reading has no places in it for a caret: its rows are not the
        // file's lines.
        if buffer.mode() != Mode::Edit {
            return;
        }
        let area = self.editor_area;
        if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
            return;
        }
        // Everything the view draws in front of the text. The numbers are a
        // click at the start of that row rather than nothing: the reader
        // pointed at a line, and pointing left of the words is how a whole
        // line is reached.
        //
        // The two columns beside them say something about the line that the
        // reader can *do*: the fold mark says it has more behind it, and
        // the change margin says what it replaced. Both are one key away
        // and the mark is the picture of the key -- so a click on the mark
        // does what the mark is about, which is what a mark like that means
        // everywhere a reader has met one.
        let lines = buffer.text().line_count();
        let changed = obelus_ui::editor::changed(self.changes());
        let folds = !buffer.folds().is_empty();
        let offset = obelus_ui::editor::text_offset(lines, changed, folds);
        let column = obelus_ui::editor::margin_at(x - area.x, lines, changed, folds);
        let row = y - area.y;
        let cell = (x - area.x).saturating_sub(offset);
        let text = self.text_area();

        // Where it is, whatever it is doing: the rest that asks a question
        // is measured from the last place it was seen.
        self.pointer_rested(x, y);
        let count = match kind {
            Pointer::Pressed => {
                self.pressed_in_the_file = true;
                self.clicks_at(x, y)
            }
            _ => 0,
        };
        match kind {
            // Nothing but where it is, which was noted above.
            Pointer::Moved => return,
            Pointer::Dragged if !self.pressed_in_the_file => return,
            // Dragging is what a reader does to select, so the place they
            // put the button down stays put.
            Pointer::Dragged => {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, cell, text, true);
                }
            }
            Pointer::Released => return,
            // A mark in the margin, pressed: the caret goes to that line --
            // both keys ask about the line the caret is on -- and then the
            // key's own work is done. Not a selection as well: the reader
            // asked for one thing.
            Pointer::Pressed
                if matches!(
                    column,
                    obelus_ui::editor::Margin::Folds | obelus_ui::editor::Margin::Changes
                ) =>
            {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, 0, text, false);
                }
                match column {
                    obelus_ui::editor::Margin::Folds => self.toggle_fold(),
                    _ => self.toggle_hunk(),
                }
                // The same as every other way out of here: wherever the
                // caret ended up is somewhere the reader is working from.
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.settle_undo();
                }
                return;
            }
            Pointer::Pressed => {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, cell, text, false);
                }
                match count {
                    // Twice is the word, three times is the line: what
                    // every editor with a pointer has taught.
                    2 => self.widen_selection(),
                    3 => {
                        let line = self.current_buffer().map(|buffer| buffer.cursor().line);
                        if let Some(line) = line
                            && let Some(buffer) = self.current_buffer_mut()
                        {
                            buffer.select_line(line);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Wherever the caret ended up is somewhere the reader is now
        // working from, so the next edit is a step of its own to undo.
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.settle_undo();
        }
    }

    /// How many times in a row the pointer has been put down here.
    ///
    /// The same cell and inside the gap below, or the count starts again.
    /// Three is as far as it goes: a fourth press is a first press, which
    /// is what selecting a line and then clicking in it has to be.
    fn clicks_at(&mut self, x: u16, y: u16) -> u8 {
        /// Long enough for a deliberate double click and short enough that
        /// two separate clicks are not taken for one. The figure every
        /// desktop uses.
        const GAP: std::time::Duration = std::time::Duration::from_millis(400);

        let now = std::time::Instant::now();
        let count = match self.clicked {
            Some((was_x, was_y, when, count))
                if (was_x, was_y) == (x, y) && now.duration_since(when) < GAP && count < 3 =>
            {
                count + 1
            }
            _ => 1,
        };
        self.clicked = Some((x, y, now, count));
        count
    }
}
