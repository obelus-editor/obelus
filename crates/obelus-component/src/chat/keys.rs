//! What a key does in a conversation: in the box, on the settings, and in
//! the transcript, where a cursor walks the words.

use super::*;

impl Chat {
    /// Handles a key.
    ///
    /// `thinking` decides what escape means first: while the agent is
    /// working it stops the agent. Otherwise it lets go of what is held,
    /// then empties the box, and with none of those the key is not this
    /// component's. Escape everywhere in Obelus means "give up on the
    /// nearest thing", and once a conversation is a document rather than
    /// something over one, leaving it is not giving up on anything.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        thinking: bool,
        room: Room,
        settings: &[obelus_agent::acp::Setting],
        tasks: bool,
    ) -> ChatOutcome {
        let Some(modifiers) = obelus_keymap::modifiers_of(key) else {
            return ChatOutcome::Ignored;
        };
        // The line break that every terminal can report. `shift+enter` is
        // the one a reader reaches for and it needs the kitty keyboard
        // protocol to arrive at all -- alt is the escape prefix, which is
        // as old as terminals.
        // Only in the box: out of it a line break is typing like any other,
        // and goes nowhere -- but is still taken here, or it would fall
        // through to the application as a chord.
        if key.code == KeyCode::Enter && modifiers == KeyModifiers::ALT {
            if self.focus == Focus::Writing {
                self.input.newline();
            }
            return ChatOutcome::Consumed;
        }
        // Saying it now rather than after the turn: escape and then enter,
        // in one press. Where nothing is running there is nothing to stop,
        // and it is enter. Control rather than a third way to break a line,
        // so like `shift+enter` it needs the kitty keyboard protocol -- and
        // a terminal without it sends enter, which waits, which is the
        // harmless half of what was asked.
        if key.code == KeyCode::Enter && modifiers == KeyModifiers::CONTROL {
            return match (thinking, self.input.is_blank()) {
                (true, false) => ChatOutcome::SendNow(self.input.take_parts()),
                (false, false) => ChatOutcome::Send(self.input.take_parts()),
                _ => ChatOutcome::Consumed,
            };
        }
        // Everything else with a modifier on it is the application's:
        // a conversation is a document rather than something over one, so
        // `ctrl+q` leaves and `f2` lists what is open from inside it. A
        // conversation that swallowed those would be one a reader cannot
        // use Obelus from.
        //
        // Except the two ends of the transcript, which are nobody else's.
        // The arms below share Home and End three ways -- bare moves the
        // caret, shift holds what it passes over, control goes to the end
        // of the whole transcript -- and the modifier a reader reaches for
        // to jump to the end of a long document is control. It reached
        // nothing: the guard turned it away before the arm that was
        // waiting for it, and in a conversation there is no file for the
        // editor to take it instead, so the key did nothing at all.
        //
        // And holding with control and shift, which is the box's: a word
        // at a time, or the whole message to either end. Control alone
        // keeps the ends of the transcript; shift is what says hold, here
        // as everywhere, so with it the ends are the message's -- one
        // selection between the two halves, so the transcript lets go.
        if self.focus == Focus::Writing
            && modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
            )
        {
            self.let_go();
            self.input.handle_key(key, room.writing);
            return ChatOutcome::Consumed;
        }
        let ends = matches!(key.code, KeyCode::Home | KeyCode::End);
        if !ends && modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return ChatOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let page = usize::from(room.transcript).max(1);

        // The row of settings, while that is what the reader is in. What it
        // does not take falls through to the box below -- except the keys
        // that are the box's own, which it swallows rather than taking the
        // focus back with them.
        if let Focus::Settings(at) = self.focus
            && let Some(outcome) = self.on_settings(key, bare, at, settings, tasks)
        {
            return outcome;
        }

        // The transcript, while the reader is walking it. What it does not
        // take falls through to the box below, the same way.
        if let Focus::Transcript(at) = self.focus
            && let Some(outcome) = self.on_transcript(key, modifiers, at, room)
        {
            return outcome;
        }

        match key.code {
            // Stopping the agent is the first thing escape does here, and
            // emptying the box the second: those are the two things in a
            // conversation there are to give up on. A conversation is a
            // document, not something over one, and escape is what leaves
            // whatever is over the document being read -- so with neither
            // it does nothing, rather than taking the reader somewhere.
            //
            // At once, with no second press to make sure: the press that
            // stops a turn puts what was waiting back in the box, and
            // taking all of it back is what the next one is for.
            KeyCode::Esc if bare && thinking => ChatOutcome::Interrupt,
            // A selection is nearer than the box it is in, and the box has
            // no undo: so what is held goes first, the box's or the
            // transcript's, and the words only on the press after.
            KeyCode::Esc if bare && (self.holding() || self.input.selected().is_some()) => {
                self.let_go();
                self.input.let_go();
                ChatOutcome::Consumed
            }
            KeyCode::Esc if bare && !self.input.is_blank() => {
                let _ = self.input.take_parts();
                ChatOutcome::Consumed
            }
            // Which is why the box takes shift: a message to an agent is a
            // paragraph, and enter is how you send one.
            KeyCode::Enter if !bare => {
                self.input.newline();
                ChatOutcome::Consumed
            }
            KeyCode::Enter => {
                if self.input.is_blank() {
                    return ChatOutcome::Consumed;
                }
                ChatOutcome::Send(self.input.take_parts())
            }
            // Shift and tab, which arrives as its own key and needs no
            // protocol to be asked for.
            KeyCode::BackTab => ChatOutcome::StepMode,

            KeyCode::Backspace if bare => {
                self.input.backspace();
                ChatOutcome::Consumed
            }
            KeyCode::Delete if bare => {
                self.input.delete();
                ChatOutcome::Consumed
            }
            KeyCode::Left if bare => {
                self.input.left();
                ChatOutcome::Consumed
            }
            KeyCode::Right if bare => {
                match self.suggestion() {
                    Some(said) => self.input.replace(said),
                    None => self.input.right(),
                }
                ChatOutcome::Consumed
            }
            // Shift holds what the arrow passes over, the way it does in
            // the file. Without these the pair fell to the arm at the
            // bottom and did nothing at all. Not the suggestion: taking
            // that is what right means bare, and holding is not taking.
            KeyCode::Left if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_left();
                ChatOutcome::Consumed
            }
            KeyCode::Right if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_right();
                ChatOutcome::Consumed
            }
            // And up and down, which fell to the same arm: a message is a
            // paragraph, and holding a line of it is shift and down. Never
            // on into the transcript the way the bare key goes -- one
            // selection between the two halves, so the press that reached
            // the transcript would have let go of what it had just held.
            KeyCode::Up if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_up(room.writing);
                ChatOutcome::Consumed
            }
            KeyCode::Down if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_down(room.writing);
                ChatOutcome::Consumed
            }
            // The box owns the arrows while its caret has somewhere to go
            // in it. Past the top of it the caret carries on into the
            // transcript, which is where a reader who has run out of box
            // presses the key next.
            KeyCode::Up if bare => {
                if self.input.up(room.writing) {
                    return ChatOutcome::Consumed;
                }
                // On to the last row, which is the words nearest the box
                // and so the ones the reader was looking at. Not the
                // nearest row that *does* something, which is where this
                // used to land: the cursor walks the words now, and tab is
                // what goes to the next thing enter opens.
                match self.back_into_the_transcript(room) {
                    Some(place) => {
                        self.focus = Focus::Transcript(place);
                        self.show_row(place.row, room);
                    }
                    None => self.scroll_by(-1),
                }
                ChatOutcome::Consumed
            }
            KeyCode::Down if bare => {
                // Down the box, then down the transcript, then out of the
                // box altogether: one key, walking whatever is still able
                // to move, in the order the things are on screen. The row
                // of settings is under the box, so it is last -- and only
                // once the transcript has nothing left to scroll, or the
                // way back down from having scrolled up would be gone.
                if self.input.down(room.writing) {
                    ChatOutcome::Consumed
                } else if self.window.at_the_end() {
                    if !settings.is_empty() || tasks {
                        self.focus = Focus::Settings(0);
                    }
                    ChatOutcome::Consumed
                } else {
                    self.scroll_by(1);
                    ChatOutcome::Consumed
                }
            }
            KeyCode::PageUp => {
                self.scroll_by(-isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::PageDown => {
                self.scroll_by(isize::try_from(page).unwrap_or(1));
                ChatOutcome::Consumed
            }
            KeyCode::Home if bare => {
                self.input.home(room.writing);
                ChatOutcome::Consumed
            }
            // Shift extends, here as everywhere. It used to be the
            // transcript's -- the top of a long answer, on the grounds
            // that the box had nowhere else to ask for it -- and then
            // control was given the two ends, which is what the rule over
            // the box names. So the reason was spent, and what was left
            // was shift naming a command in the one place a reader reaches
            // for it to hold a line: they pressed it to take back what
            // they had just written, and the whole conversation flew to
            // its beginning.
            KeyCode::Home if modifiers == KeyModifiers::SHIFT => {
                // One selection between the two halves, so taking hold in
                // here lets the transcript go.
                self.let_go();
                self.input.hold_home(room.writing);
                ChatOutcome::Consumed
            }
            // And control is the two ends of the transcript, from the box
            // as from inside it.
            KeyCode::Home => {
                self.window.home();
                ChatOutcome::Consumed
            }
            KeyCode::End if bare => {
                self.input.end(room.writing);
                ChatOutcome::Consumed
            }
            KeyCode::End if modifiers == KeyModifiers::SHIFT => {
                self.let_go();
                self.input.hold_end(room.writing);
                ChatOutcome::Consumed
            }
            KeyCode::End => {
                self.window.end();
                ChatOutcome::Consumed
            }
            KeyCode::Char(character) => {
                self.input.insert(character);
                ChatOutcome::Consumed
            }
            _ => ChatOutcome::Ignored,
        }
    }

    /// What a key does while the row of settings is what the reader is in.
    ///
    /// `None` means the key is not this row's: the box below gets it. The
    /// keys that are the box's own -- typing, and the two that delete --
    /// are swallowed here instead, the way the transcript swallows them.
    fn on_settings(
        &mut self,
        key: &KeyEvent,
        bare: bool,
        at: usize,
        settings: &[obelus_agent::acp::Setting],
        tasks: bool,
    ) -> Option<ChatOutcome> {
        // Everything the keys can stand on: the settings, and the count of
        // background work after them, which is one more stop on the row.
        let stops = settings.len() + usize::from(tasks);
        match key.code {
            // Along the row, and round: it is a short cycle, and a reader
            // walking off one end means the other end.
            KeyCode::Left if bare && stops > 0 => {
                let last = stops - 1;
                self.focus = Focus::Settings(if at == 0 { last } else { at - 1 });
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Right if bare && stops > 0 => {
                self.focus = Focus::Settings((at + 1) % stops);
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Enter if bare && tasks && at == settings.len() => Some(ChatOutcome::Tasks),
            // Whatever the setting under the focus is: a list of values is
            // a list to open, and a switch has nowhere to go, so it flips.
            // The same judgement the row's drawing makes.
            KeyCode::Enter if bare => Some(settings.get(at).map_or(
                ChatOutcome::Consumed,
                |setting| match setting.kind {
                    obelus_agent::acp::Kind::Select => ChatOutcome::Choose(setting.id.clone()),
                    obelus_agent::acp::Kind::Switch => ChatOutcome::Toggle(setting.id.clone()),
                },
            )),
            // Back to the box. Escape as well, because escape gives up on
            // the nearest thing first, and the nearest thing is being here
            // rather than the whole conversation.
            KeyCode::Up | KeyCode::Esc if bare => {
                self.focus = Focus::Writing;
                Some(ChatOutcome::Consumed)
            }
            // Nothing is under this row.
            KeyCode::Down if bare => Some(ChatOutcome::Consumed),
            // The box's own keys, which do nothing here: a letter pressed
            // on this row is not a reader asking to be back in the box.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter => {
                Some(ChatOutcome::Consumed)
            }
            // Everything else -- the paging keys, the ends of the
            // transcript -- goes on meaning what it means, and the focus
            // stays where the reader put it.
            _ => None,
        }
    }

    /// Scrolls by rows.
    pub(super) fn scroll_by(&mut self, rows: isize) {
        self.window.scroll(rows);
    }

    /// The same, for a caller holding the rows already.
    pub(super) fn in_view_of(&self, count: usize, room: Room) -> std::ops::Range<usize> {
        let top = self.window.top();
        top.min(count)..(top + usize::from(room.transcript)).min(count)
    }

    /// Puts a row of the transcript on screen, moving the window as little
    /// as it takes.
    fn show_row(&mut self, at: usize, room: Room) {
        let count = self.rows(room.reading).len();
        self.show_to(at, count, room);
    }

    /// The same, for a caller holding the rows already.
    fn show_to(&mut self, at: usize, count: usize, room: Room) {
        let view = self.in_view_of(count, room);
        if at < view.start {
            self.scroll_by(-isize::try_from(view.start - at).unwrap_or(1));
        } else if at >= view.end {
            self.scroll_by(isize::try_from(at + 1 - view.end).unwrap_or(1));
        }
    }

    /// Goes back to the box, remembering which column of the transcript
    /// this was.
    pub(super) fn leave_the_transcript(&mut self, at: Place) {
        self.stood = Some(at.character);
        self.focus = Focus::Writing;
    }

    /// Where the cursor goes when it comes in from the box.
    ///
    /// The last row always: it is the words nearest the box, and it is
    /// where whatever was said while the reader was typing has gone. And
    /// the column it left in, where it has been in here -- otherwise the
    /// end of that row.
    fn back_into_the_transcript(&self, room: Room) -> Option<Place> {
        let rows = self.rows(room.reading);
        let row = rows.len().checked_sub(1)?;
        let characters = rows[row].characters();
        Some(Place {
            row,
            // The row is not the one the column was left in, and even if
            // it were it may be a different length than it was: the window
            // is resized, a run is opened, the agent says something that
            // reflows what is above it.
            character: self.stood.unwrap_or(characters).min(characters),
        })
    }

    /// The next row worth standing on above or below `at`, among rows the
    /// caller is holding already.
    ///
    /// Above what `at` is part of, not above `at`: on the third row of a
    /// command the stop above is the command's own first row, which is
    /// the call the key was pressed in -- a key that looked as though it
    /// had done nothing.
    ///
    /// And one stop for each thing, at the first of its rows: every row of
    /// a block of code acts, and a key that stepped through them one at a
    /// time would be the down arrow.
    fn next_stop_in(at: usize, up: bool, laid: &[Row]) -> Option<usize> {
        let at = match up {
            true => Row::acting(laid, at).map_or(at, |on| on.start),
            false => at,
        };
        let mut stops = laid
            .iter()
            .enumerate()
            .filter(|(at, row)| {
                row.acts() && Row::acting(laid, *at).is_some_and(|on| on.start == *at)
            })
            .map(|(at, _)| at);
        match up {
            true => stops.take_while(|stop| *stop < at).last(),
            false => stops.find(|stop| *stop > at),
        }
    }

    /// Where the cursor stands on arriving at a stop.
    ///
    /// The start of the row, except in a block of code, where it is the
    /// first character of the code: the first row of a block is its box,
    /// and a caret on the corner is standing in nothing anybody wrote.
    fn standing_at(stop: usize, laid: &[Row]) -> Place {
        let at_the_start = Place {
            row: stop,
            character: 0,
        };
        let Some(on) = laid
            .get(stop)
            .filter(|row| row.code.is_some())
            .and(Row::acting(laid, stop))
        else {
            return at_the_start;
        };
        on.into_iter()
            .find_map(|row| {
                let spot = laid[row].spot_at(0)?;
                Some(Place {
                    row,
                    character: laid[row].characters_at(spot)?,
                })
            })
            .unwrap_or(at_the_start)
    }

    /// The place in the words the cursor is at, or the nearest there is.
    ///
    /// Nearest, because a cursor can stand where there are no words: on a
    /// blank, on a plan's step, on the heading over a folded run. A
    /// selection started from one of those has to begin somewhere, and the
    /// somewhere a reader means is the words they are next to.
    fn spot_near(place: Place, laid: &[Row]) -> Option<Spot> {
        let rows = laid;
        if let Some(spot) = rows.get(place.row)?.spot_at(place.character) {
            return Some(spot);
        }
        // Downwards first: the rows Obelus draws itself sit above what they
        // are about -- a heading over its run, a blank before what follows
        // it -- so the words they belong to are the ones under them.
        rows.iter()
            .skip(place.row)
            .find_map(|row| row.spot_at(0))
            .or_else(|| {
                rows.iter()
                    .take(place.row)
                    .rev()
                    .find_map(|row| row.spot_at(row.characters()))
            })
    }

    /// Where a motion takes the cursor, or nothing where it runs out of
    /// transcript.
    ///
    /// All of it in rows and characters, because that is what the keys
    /// mean: a reader pressing the right arrow means the next character on
    /// the screen, wherever in the words it happens to come from.
    fn walked(place: Place, key: KeyCode, laid: &[Row], room: Room) -> Option<Place> {
        let characters = |row: usize| laid.get(row).map_or(0, Row::characters);
        // Where a caret may stand on a row: between clusters, so that a
        // picture and its selector, or a letter and its accent, are one
        // step -- the same as in a file. See `Text::cluster_before`.
        let stops = |row: usize| {
            laid.get(row)
                .map_or_else(|| vec![0], |row| obelus_text::boundaries(&row.text()))
        };
        let settled = |place: Place| Place {
            character: stops(place.row)
                .into_iter()
                .take_while(|stop| *stop <= place.character)
                .last()
                .unwrap_or(0),
            ..place
        };
        let rows = laid.len();
        let here = characters(place.row);
        Some(settled(match key {
            KeyCode::Right if place.character < here => Place {
                character: stops(place.row)
                    .into_iter()
                    .find(|stop| *stop > place.character)
                    .unwrap_or(here),
                ..place
            },
            // Off the end of a row and on to the start of the next, which
            // is where the words carry on.
            KeyCode::Right if place.row + 1 < rows => Place {
                row: place.row + 1,
                character: 0,
            },
            KeyCode::Left if place.character > 0 => Place {
                character: stops(place.row)
                    .into_iter()
                    .take_while(|stop| *stop < place.character)
                    .last()
                    .unwrap_or(0),
                ..place
            },
            KeyCode::Left if place.row > 0 => Place {
                row: place.row - 1,
                character: characters(place.row - 1),
            },
            // Up and down keep the column where they can, the way they do
            // in any text: a row too short for it takes the caret to its
            // end rather than refusing the key.
            KeyCode::Up if place.row > 0 => Place {
                row: place.row - 1,
                character: place.character.min(characters(place.row - 1)),
            },
            KeyCode::Down if place.row + 1 < rows => Place {
                row: place.row + 1,
                character: place.character.min(characters(place.row + 1)),
            },
            KeyCode::Home => Place {
                character: 0,
                ..place
            },
            KeyCode::End => Place {
                character: here,
                ..place
            },
            KeyCode::PageUp => {
                let page = usize::from(room.transcript.max(1));
                let row = place.row.saturating_sub(page);
                Place {
                    row,
                    character: place.character.min(characters(row)),
                }
            }
            KeyCode::PageDown => {
                let page = usize::from(room.transcript.max(1));
                let row = (place.row + page).min(rows.saturating_sub(1));
                Place {
                    row,
                    character: place.character.min(characters(row)),
                }
            }
            _ => return None,
        }))
    }

    /// Walks the transcript with the cursor, and holds what it walks over
    /// while shift is down.
    ///
    /// What it does not take falls through to the box below. The keys that
    /// are the box's own it does take, and does nothing with: a letter
    /// pressed while reading is not a reader asking to be in the box. Down
    /// off the last row and escape are the ways back, so nobody is stuck in
    /// here.
    fn on_transcript(
        &mut self,
        key: &KeyEvent,
        modifiers: KeyModifiers,
        at: Place,
        room: Room,
    ) -> Option<ChatOutcome> {
        let bare = modifiers == KeyModifiers::NONE;
        // Laid out once, and everything below asks these rows rather than
        // the conversation. A width makes rows out of every word ever said
        // in it, which is real work -- tens of milliseconds for a long
        // morning's conversation -- and a cursor that moves on every arrow
        // pays it on every arrow. Asked five times over for one keypress,
        // as this was, the caret crawls.
        let laid = self.rows(room.reading);
        // What shift means on a motion is "and hold what I pass over", and
        // it is the only modifier that does: control reaches here too --
        // the guard lets the two ends through -- and asking only whether
        // the key was bare made `ctrl+end` hold the row it was on instead
        // of going to the end, while the rule over the box went on naming
        // it as the way back.
        let holding = modifiers == KeyModifiers::SHIFT;
        match key.code {
            // Which is that way back, with the cursor in here: the two ends
            // of the transcript. They move the cursor and the view follows,
            // because in here the keys move the cursor -- a view sent to
            // the end with the cursor left behind is dragged back by the
            // next arrow.
            KeyCode::Home | KeyCode::End if modifiers == KeyModifiers::CONTROL => {
                let last = laid.len().saturating_sub(1);
                let (row, character) = match key.code {
                    KeyCode::Home => (0, 0),
                    _ => (last, laid.get(last).map_or(0, Row::characters)),
                };
                self.let_go();
                self.focus = Focus::Transcript(Place { row, character });
                match key.code {
                    KeyCode::Home => self.window.home(),
                    _ => self.window.end(),
                }
                Some(ChatOutcome::Consumed)
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => {
                let moved = Self::walked(at, key.code, &laid, room);
                // Down off the end of the transcript is the box, which is
                // where a reader who has walked to the bottom is going
                // next. Every other motion that runs out simply stops.
                let Some(moved) = moved else {
                    if key.code == KeyCode::Down && !holding {
                        self.let_go();
                        self.leave_the_transcript(at);
                        return Some(ChatOutcome::Consumed);
                    }
                    return Some(ChatOutcome::Consumed);
                };
                match holding {
                    // A shift-motion with nothing held yet starts the
                    // selection where the cursor was, not where it is
                    // going: what the reader means to hold is what the key
                    // passed over.
                    true => {
                        if self.held.is_none()
                            && let Some(from) = Self::spot_near(at, &laid)
                        {
                            self.hold_from(from);
                            // One selection between the two halves, so
                            // taking hold here lets the box go.
                            self.input.let_go();
                        }
                        if let Some(to) = Self::spot_near(moved, &laid) {
                            self.hold_to(to);
                        }
                    }
                    // And a bare one lets go, the way it does in the box
                    // and in the file: a caret moved on its own is a
                    // reader who has finished with what they had.
                    false => self.let_go(),
                }
                self.focus = Focus::Transcript(moved);
                self.show_to(moved.row, laid.len(), room);
                Some(ChatOutcome::Consumed)
            }
            // The next thing enter would open, which is what the arrows
            // used to land on and no longer do: they walk the words now, so
            // getting to a heading in a long run of them is its own key.
            KeyCode::Tab | KeyCode::BackTab => {
                let up = key.code == KeyCode::BackTab;
                // Consumed either way. Nothing that way is a key that does
                // nothing, not a key that falls through: shift and tab
                // steps the agent's way of working, and a reader who
                // pressed it once too often while reading would have
                // changed how the agent works without meaning to.
                let Some(stop) = Self::next_stop_in(at.row, up, &laid) else {
                    return Some(ChatOutcome::Consumed);
                };
                self.let_go();
                let standing = Self::standing_at(stop, &laid);
                self.focus = Focus::Transcript(standing);
                self.show_to(standing.row, laid.len(), room);
                Some(ChatOutcome::Consumed)
            }
            // Whatever the row is: a heading opens and closes what is
            // under it, and a row that names a file goes there. Both are
            // "do what this row is for", which is what enter means
            // everywhere else in Obelus. On a row that is only words it
            // does nothing, because there is nothing there to do.
            KeyCode::Enter if bare => {
                let start = Row::acting(&laid, at.row).map_or(at.row, |on| on.start);
                let row = laid.get(start).cloned();
                match row {
                    // A block of code is copied, whichever of its rows the
                    // reader is on: it is what they would otherwise have
                    // to hold from corner to corner, and holding it takes
                    // the box and the width's breaks with it.
                    Some(Row {
                        code: Some(code), ..
                    }) => Some(ChatOutcome::Copy(code.text.to_string())),
                    Some(row) => match (row.unsent, row.again, row.folds, row.place, row.away) {
                        // Something they said that has not gone: one of
                        // the two rows here whose key gives rather than
                        // opens.
                        (Some(which), ..) => match self.take_back(which) {
                            // Back to the box with them, where the caret
                            // is: taking something back is almost always
                            // meaning to say it again differently.
                            Some(words) => {
                                self.leave_the_transcript(at);
                                Some(ChatOutcome::TakeBack(words))
                            }
                            None => Some(ChatOutcome::Consumed),
                        },
                        // And something they said that has gone, which
                        // stays on the page: it was said, and the box gets
                        // a copy to say again or differently.
                        (None, Some(which), ..) => match self.again(which) {
                            Some(words) => {
                                self.leave_the_transcript(at);
                                Some(ChatOutcome::TakeBack(words))
                            }
                            None => Some(ChatOutcome::Consumed),
                        },
                        (None, None, Some(begins), _, _) => {
                            self.fold(begins);
                            // The heading stays under the reader: what
                            // moved is what is below it. And so does the
                            // row they pressed it on, unless the fold took
                            // it: a title closes to fewer rows than it has
                            // open, and the key is answered on all of them,
                            // so the cursor goes to the first of them
                            // rather than staying on a number that is now
                            // the next thing said.
                            let laid = self.rows(room.reading);
                            let kept =
                                Row::acting(&laid, at.row).is_some_and(|on| on.start == start);
                            if !kept {
                                self.focus = Focus::Transcript(Place {
                                    row: start,
                                    character: 0,
                                });
                            }
                            self.show_to(if kept { at.row } else { start }, laid.len(), room);
                            Some(ChatOutcome::Consumed)
                        }
                        (None, None, None, Some((place, _)), _) => Some(ChatOutcome::GoTo(place)),
                        // And a row that points at a web address goes
                        // there, which is the same rule about the same key.
                        (None, None, None, None, Some(url)) => Some(ChatOutcome::Away(url)),
                        (None, None, None, None, None) => Some(ChatOutcome::Consumed),
                    },
                    None => Some(ChatOutcome::Consumed),
                }
            }
            // Back to the box: escape gives up on the nearest thing first,
            // and the nearest thing is walking about in here. What is held
            // goes with it -- a selection nobody can see the cursor of is
            // one the reader has left behind.
            KeyCode::Esc if bare => {
                self.let_go();
                self.leave_the_transcript(at);
                Some(ChatOutcome::Consumed)
            }
            // The box's own keys do nothing in here, and leave the cursor
            // and what it holds where they are: a letter pressed while
            // reading threw the reader out of the place they had walked to
            // and into a box they had not asked for.
            KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter => {
                Some(ChatOutcome::Consumed)
            }
            _ => None,
        }
    }
}
