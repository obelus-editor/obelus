//! A list the reader builds by name, in the order they want it tried.
//!
//! What is in it, and what else there is to put in it. Two sections of one
//! list: the names the reader has chosen, in their order, and the ones this
//! machine turned out to have -- filtered by what is being typed, and
//! without the ones already chosen, because a row that is already in the
//! list is not one to offer again.
//!
//! Written for the fonts a window draws with and written to know nothing
//! about fonts: what may go in comes from the caller as a list of names,
//! because the next thing that wants this -- the globs a project ignores,
//! the agents a reader keeps -- will have its own.
//!
//! It is a list with a query over it, which is what a picker is, and it
//! borrows a picker's parts for exactly that reason: the same window, the
//! same six movement keys, the same matcher, the same marking of what
//! matched, the same query on the status row. What it is not is a picker:
//! a picker chooses one row and closes, and this one is opened to be
//! *changed* -- every key that does anything here adds, removes or moves,
//! and the reader leaves when the list says what they meant. Share the
//! mechanism, not the meaning.
//!
//! What is offered comes from the window in `obg`, which is the only part
//! of Obelus that can see a font database. It says so with an event, once,
//! which is why the list takes what it is given later as well: the reader
//! opens it before a machine with a thousand faces has finished answering.
//! What is drawn with, and what a reader who chose nothing gets, are the
//! window's (`obelus-gui`'s `font` and `monospace`).
//!
//! Cells only, like everything else in this crate. A window draws it the
//! same way a terminal does.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::{
    field::Field,
    window::{Move, Window, Wrap},
};

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// One the reader has chosen, at this place in the order.
    Chosen {
        /// Where it sits, counted from one, which is what the reader is
        /// ordering.
        at: usize,
        /// What it is called.
        name: String,
        /// Whether this machine has it. A settings file is read on every
        /// machine the reader uses, and a name that means nothing here is
        /// a name that means something there.
        here: bool,
    },
    /// One that could be chosen.
    Offer {
        /// What it is called.
        name: String,
        /// Whether this machine has it, which is false only for the row
        /// that is what the reader has typed.
        here: bool,
    },
    /// Nothing has been chosen, so there is nothing above the boundary.
    ///
    /// A row rather than something the view draws beside the list, because
    /// everything here is one list: a screen row that was not a row of it
    /// would have to be added to every piece of arithmetic that says which
    /// row is where.
    Empty,
    /// The boundary between what is chosen and what is on offer.
    ///
    /// Also a row, for the same reason -- and one the focus steps over,
    /// the way tab in a transcript goes only to rows that do something.
    Boundary,
}

impl Row {
    /// What it is called.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Chosen { name, .. } | Self::Offer { name, .. } => name,
            Self::Empty | Self::Boundary => "",
        }
    }

    /// Whether this machine has it.
    #[must_use]
    pub const fn here(&self) -> bool {
        match self {
            Self::Chosen { here, .. } | Self::Offer { here, .. } => *here,
            Self::Empty | Self::Boundary => true,
        }
    }

    /// Whether this is one of the reader's.
    #[must_use]
    pub const fn chosen(&self) -> bool {
        matches!(self, Self::Chosen { .. })
    }

    /// Whether the reader can stand on it.
    ///
    /// Two of them say something about the list rather than being part of
    /// it, and a focus that could sit on either would be a row where every
    /// key does nothing.
    #[must_use]
    pub const fn stands(&self) -> bool {
        matches!(self, Self::Chosen { .. } | Self::Offer { .. })
    }
}

/// What a key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing anybody else has to hear about.
    Consumed,
    /// The list is different now, and has to be written down.
    Changed,
    /// The reader is done with it.
    Leave,
    /// Not this list's, and so for the table.
    Ignored,
}

/// A list of names, and what else there is.
pub struct Names {
    /// What the reader has chosen, in their order.
    chosen: Vec<String>,
    /// What this machine has.
    offered: Vec<String>,
    /// What it calls its monospaced face, which is what the reader gets
    /// while they have chosen nothing.
    ///
    /// Said on the page rather than left as "the machine's own": a reader
    /// deciding whether to choose anything is deciding against something,
    /// and a name is what that something is.
    otherwise: Option<String>,
    /// What typing into this list does, for the row before anything has
    /// been typed -- see [`Names::before_typing`].
    invitation: Option<String>,
    /// What is being typed, which filters the offers and is itself an
    /// offer when it matches nothing.
    query: Field,
    window: Window,
    matcher: Matcher,
    /// The rows as they are now, and what matched in each of them.
    ///
    /// Worked out when something changes rather than when they are asked
    /// for, because what asks is the view: matching needs the matcher,
    /// which is a thing to change, and a frame has only what is there.
    rows: Vec<Row>,
    marks: Vec<Vec<u32>>,
}

impl std::fmt::Debug for Names {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Names")
            .field("chosen", &self.chosen)
            .field("offered", &self.offered.len())
            .field("query", &self.query.said())
            .finish_non_exhaustive()
    }
}

impl Names {
    /// A list of what the reader has chosen, over what this machine has.
    #[must_use]
    pub fn new(chosen: Vec<String>, offered: Vec<String>, otherwise: Option<String>) -> Self {
        let mut names = Self {
            chosen,
            offered,
            otherwise,
            invitation: None,
            query: Field::new(),
            window: Window::default(),
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            rows: Vec::new(),
            marks: Vec::new(),
        };
        names.settle();
        // On the first thing there is to add, not on the first thing they
        // have: a list like this is opened to put something in it, and
        // enter on one of their own names takes it out -- so opening with
        // the focus there would make the first press of the one key that
        // does anything undo their answer.
        names.window.set_focus(names.taken());
        names.stand(true);
        names
    }

    /// What this machine has, which may arrive after the list is open.
    ///
    /// The window enumerates its fonts on its own thread and says so when
    /// it has; until then the list is what the reader chose, and what they
    /// type is still a name they can add.
    pub fn offered(&mut self, offered: Vec<String>, otherwise: Option<String>) {
        self.offered = offered;
        self.otherwise = otherwise;
        self.settle();
    }

    /// What is drawn with while nothing is chosen, where the machine said.
    #[must_use]
    pub fn otherwise(&self) -> Option<&str> {
        self.otherwise.as_deref()
    }

    /// What the reader has chosen, in their order.
    #[must_use]
    pub fn chosen(&self) -> &[String] {
        &self.chosen
    }

    /// Says what typing into this list does, for the row before anything
    /// has been.
    ///
    /// The caller's words, like the offers themselves: this list knows
    /// names and nothing about what they name, and the next thing that
    /// wants it will be narrowing something else.
    pub fn before_typing(&mut self, what: &str) {
        self.invitation = Some(what.to_string());
    }

    /// What it says typing does, while nothing has been typed.
    #[must_use]
    pub fn invitation(&self) -> Option<&str> {
        match self.query.said().is_empty() {
            true => self.invitation.as_deref(),
            false => None,
        }
    }

    /// The box the query is typed in.
    #[must_use]
    pub const fn query(&self) -> &Field {
        &self.query
    }

    /// Which rows are on screen, and which of them is under the keys.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// The same, to move.
    pub const fn window_mut(&mut self) -> &mut Window {
        &mut self.window
    }

    /// Every row, the chosen ones first.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Where what was typed matched, in the row at this place, for the view
    /// to mark.
    ///
    /// Only the offers are matched against: the chosen rows are the
    /// reader's own list and are not what is being searched.
    #[must_use]
    pub fn marks_at(&self, at: usize) -> &[u32] {
        self.marks.get(at).map_or(&[], Vec::as_slice)
    }

    /// How many rows the chosen list has, which is where the boundary
    /// between the two sections goes.
    #[must_use]
    pub fn taken(&self) -> usize {
        self.chosen.len()
    }

    /// What is on offer: what the machine has, less what is already
    /// chosen, filtered by what is being typed -- and what was typed,
    /// where nothing is called that.
    fn offers(&mut self) -> Vec<String> {
        let said = self.query.said();
        let typed = said.trim().to_string();
        let mut offers: Vec<String> = Vec::new();
        if typed.is_empty() {
            offers.extend(
                self.offered
                    .iter()
                    .filter(|name| !self.chosen.contains(name))
                    .cloned(),
            );
            return offers;
        }
        let pattern = Pattern::parse(&typed, CaseMatching::Ignore, Normalization::Smart);
        let mut buffer = Vec::new();
        let mut ranked: Vec<(u32, &String)> = self
            .offered
            .iter()
            .filter(|name| !self.chosen.contains(name))
            .filter_map(|name| {
                let haystack = Utf32Str::new(name, &mut buffer);
                pattern
                    .score(haystack, &mut self.matcher)
                    .map(|score| (score, name))
            })
            .collect();
        // Best first, and by name where two score the same: a list that
        // reorders itself between two keystrokes for no visible reason is
        // a list a reader cannot aim at.
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
        offers.extend(ranked.into_iter().map(|(_, name)| name.clone()));
        // What they typed, where nothing here is called that and they have
        // not chosen it already. A settings file is read on more than one
        // machine, so a name this one does not have is still worth adding.
        let known = self.offered.iter().any(|name| name == &typed);
        if !known && !self.chosen.contains(&typed) {
            offers.push(typed);
        }
        offers
    }

    /// Moves the window to where the rows about to be drawn put it.
    ///
    /// Once a frame, with the height the list will have: where the window
    /// belongs depends on the geometry, and the geometry is only settled at
    /// that point. Without it the focus walks off the bottom and the rows
    /// stay where they were, which is a list that cannot be scrolled.
    pub fn settle_window(&mut self, height: u16) {
        self.window.settle(height);
    }

    /// Walks the selection by this many rows, for a wheel.
    ///
    /// A notch steps the selection rather than the view, because a list's
    /// view *is* its selection and there is nothing else in it to scroll.
    pub fn step(&mut self, by: isize) {
        let at = self.window.focus();
        let to = at.saturating_add_signed(by);
        self.window
            .set_focus(to.min(self.rows.len().saturating_sub(1)));
        self.stand(by >= 0);
    }

    /// Puts pasted text in the query.
    ///
    /// The same door a key goes through, so that what was pasted narrows
    /// the list exactly as typing it would have.
    pub fn put_in_query(&mut self, what: &str) {
        self.query.put(what);
        self.retyped();
    }

    /// What a copy takes from the query: what is held, or all of it.
    #[must_use]
    pub fn copy_query(&self) -> (String, &'static str) {
        self.query.copied()
    }

    /// The same, and takes it out -- through the same door typing goes
    /// through, because what is left is a different list.
    pub fn cut_query(&mut self) -> (String, &'static str) {
        let cut = self.query.cut();
        self.retyped();
        cut
    }

    /// Puts the query's caret where a cell of its row is.
    pub fn place_in_query(&mut self, cell: u16, extend: bool) {
        self.query.place_at_cell(cell, extend);
    }

    /// Takes hold of the word under the caret, or of the whole query.
    pub fn hold_in_query(&mut self, all: bool) {
        match all {
            true => self.query.hold_all(),
            false => self.query.hold_word(),
        }
    }

    /// A different query is a different list, and the row the reader was
    /// on is not the row they are on now. On the first thing they can
    /// press, which is the first offer: what they are typing is a search
    /// for one.
    fn retyped(&mut self) {
        self.settle();
        self.window.set_focus(self.taken());
        self.stand(true);
    }

    /// Takes a key, and says what it did.
    ///
    /// `page` is how many rows are on screen, which is the view's answer
    /// and not this one's.
    pub fn handle_key(&mut self, key: &KeyEvent, page: u16) -> Outcome {
        // Moving a chosen name is the one thing here that is a chord, and
        // it is the chord the notes already use for moving a row.
        if key.modifiers == KeyModifiers::ALT && matches!(key.code, KeyCode::Up | KeyCode::Down) {
            let by = match key.code {
                KeyCode::Up => -1,
                _ => 1,
            };
            return self.shift(by);
        }
        // Bare, because with shift the keys are the query's: holding what
        // the caret passes over, the way they do in every box in Obelus.
        if key.modifiers.is_empty()
            && let Some(movement) = Move::of(key.code)
        {
            // It does not wrap: the two sections are one list with an
            // order to it, and walking off the last offer to land on the
            // first chosen name is a jump nobody asked for.
            let was = self.window.focus();
            self.window.apply(movement, page, Wrap::No);
            self.stand(self.window.focus() >= was);
            return Outcome::Consumed;
        }
        match key.code {
            // What is held in the query first, which is nearer than the
            // list.
            KeyCode::Esc if self.query.let_go() => Outcome::Consumed,
            KeyCode::Esc => Outcome::Leave,
            // On one of the reader's own: take it out. On one that is
            // offered: put it in. Which is the same key doing the same
            // thing -- what this row says about the list, said the other
            // way round.
            KeyCode::Enter => self.toggle(),
            // The two keys a reader reaches for to take something out, and
            // only ever out: on an offer they do nothing, because putting a
            // name in is not what anybody means by either. Delete was
            // enter's twin once, and added a face where it was pressed.
            //
            // Backspace is also the query's, where it takes a character
            // back, so it is this only with nothing typed: a reader
            // correcting what they typed who had stepped up onto their own
            // list would lose a face for a letter. Asked of the text as it
            // is rather than whether it is blank, because a query of one
            // space still has a space to take back. Delete needs no such
            // guard -- the query's caret is always at its end, where
            // delete has nothing to take.
            KeyCode::Delete => self.take_out(),
            KeyCode::Backspace if self.query.said().is_empty() => self.take_out(),
            // What the query refuses goes on to the table, which is how
            // copying, cutting and taking all of it reach the box: a box a
            // reader can select in but not copy out of has half a
            // selection.
            _ => {
                let said = self.query.said();
                if !self.query.handle_key(key) {
                    return Outcome::Ignored;
                }
                if self.query.said() != said {
                    self.retyped();
                }
                Outcome::Consumed
            }
        }
    }

    /// Takes the row the reader is on out of the list, where it is one of
    /// theirs, and does nothing where it is not.
    fn take_out(&mut self) -> Outcome {
        match self.rows.get(self.window.focus()).is_some_and(Row::chosen) {
            true => self.toggle(),
            false => Outcome::Consumed,
        }
    }

    /// Puts the row the reader is on into the list, or takes it out.
    fn toggle(&mut self) -> Outcome {
        let at = self.window.focus();
        match self.rows.get(at).cloned() {
            Some(Row::Chosen { at: place, .. }) => {
                self.chosen.remove(place - 1);
                self.settle();
                // Where the row they were on used to be, which is now
                // whatever moved up into it.
                let last = self.rows.len().saturating_sub(1);
                self.window.set_focus(at.min(last));
                self.stand(true);
                Outcome::Changed
            }
            Some(Row::Offer { name, .. }) => {
                self.chosen.push(name);
                // The query has done its job, and leaving it would leave
                // the reader looking at a list filtered by the name they
                // just used.
                self.query.clear();
                self.settle();
                // Where they were, which is now the next thing to add: the
                // name they took moved up across the boundary, so the row
                // they are on holds the one after it. Every list a reader
                // ticks through behaves this way, and the alternative --
                // landing on what was just added -- makes enter twice mean
                // add and then take back.
                self.stand(true);
                Outcome::Changed
            }
            Some(Row::Empty | Row::Boundary) | None => Outcome::Consumed,
        }
    }

    /// Moves the chosen row the reader is on up or down the list.
    fn shift(&mut self, by: isize) -> Outcome {
        let at = self.window.focus();
        let Some(Row::Chosen { at: place, .. }) = self.rows.get(at).cloned() else {
            // Only the reader's own list has an order. An offer has no
            // place to be moved within.
            return Outcome::Consumed;
        };
        let from = place - 1;
        let Some(to) = from
            .checked_add_signed(by)
            .filter(|to| *to < self.chosen.len())
        else {
            return Outcome::Consumed;
        };
        self.chosen.swap(from, to);
        self.settle();
        // With the row, which is the whole point: a reader holding alt and
        // pressing down watches one name walk down the list.
        self.window.set_focus(to);
        Outcome::Changed
    }

    /// Puts the focus on a row that does something, carrying on the way it
    /// was going.
    ///
    /// The boundary and the line saying nothing is chosen are rows of the
    /// list so that everything which counts rows counts the same ones.
    /// What they are not is somewhere to stand.
    fn stand(&mut self, onwards: bool) {
        for _ in 0..self.rows.len() {
            let at = self.window.focus();
            if self.rows.get(at).is_none_or(Row::stands) {
                return;
            }
            let next = match onwards {
                true => at.saturating_add(1),
                false => at.saturating_sub(1),
            };
            // The end of the list in the direction of travel: turn round
            // rather than sit on a row that does nothing.
            if next == at || next >= self.rows.len() {
                self.turn(!onwards);
                return;
            }
            self.window.set_focus(next);
        }
    }

    /// The same, the other way, once.
    fn turn(&mut self, onwards: bool) {
        for _ in 0..self.rows.len() {
            let at = self.window.focus();
            if self.rows.get(at).is_none_or(Row::stands) {
                return;
            }
            let next = match onwards {
                true => at.saturating_add(1),
                false => at.saturating_sub(1),
            };
            if next == at || next >= self.rows.len() {
                return;
            }
            self.window.set_focus(next);
        }
    }

    /// Works the rows out again, and tells the window how many there are.
    fn settle(&mut self) {
        let mut rows: Vec<Row> = self
            .chosen
            .iter()
            .enumerate()
            .map(|(at, name)| Row::Chosen {
                at: at + 1,
                name: name.clone(),
                here: self.offered.iter().any(|offer| offer == name),
            })
            .collect();
        if rows.is_empty() {
            rows.push(Row::Empty);
        }
        rows.push(Row::Boundary);
        let offers = self.offers();
        rows.extend(offers.into_iter().map(|name| Row::Offer {
            here: self.offered.iter().any(|offer| offer == &name),
            name,
        }));

        // What matched, row by row, while the matcher is at hand: a view
        // has no way to ask for this later.
        let said = self.query.said();
        let pattern = Pattern::parse(&said, CaseMatching::Ignore, Normalization::Smart);
        let mut buffer = Vec::new();
        self.marks = rows
            .iter()
            .map(|row| {
                if said.is_empty() || row.chosen() {
                    return Vec::new();
                }
                let mut marks = Vec::new();
                let haystack = Utf32Str::new(row.name(), &mut buffer);
                pattern.indices(haystack, &mut self.matcher, &mut marks);
                marks.sort_unstable();
                marks.dedup();
                marks
            })
            .collect();

        self.window.set_count(rows.len());
        self.rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    /// A list with two of the machine's four faces already chosen.
    fn names() -> Names {
        Names::new(
            vec!["Iosevka".to_string()],
            vec![
                "Iosevka".to_string(),
                "JetBrains Mono".to_string(),
                "Noto Sans CJK SC".to_string(),
                "Noto Color Emoji".to_string(),
            ],
            Some("Liberation Mono".to_string()),
        )
    }

    /// Presses a bare key.
    fn press(names: &mut Names, code: KeyCode) -> Outcome {
        names.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), 10)
    }

    /// Types a word into the query.
    fn type_in(names: &mut Names, word: &str) {
        for character in word.chars() {
            press(names, KeyCode::Char(character));
        }
    }

    /// What is already chosen is not offered again.
    ///
    /// Deliberate break: leaving the chosen names in the offers puts
    /// `Iosevka` on the page twice, once in each half, and enter on the
    /// lower one adds a second copy of it.
    #[test]
    fn what_is_in_the_list_is_not_offered_again() {
        let names = names();
        let offered: Vec<&str> = names
            .rows()
            .iter()
            .filter(|row| matches!(row, Row::Offer { .. }))
            .map(Row::name)
            .collect();
        assert!(!offered.contains(&"Iosevka"), "{offered:?}");
        assert_eq!(offered.len(), 3);
    }

    /// Enter on an offer puts it at the end of the list, and enter on one
    /// of the reader's takes it out.
    ///
    /// Both halves, because each passes with the other broken: one key
    /// means "put this in" above the boundary and "take this out" below
    /// it, and a key that only ever added would leave a list nobody can
    /// shorten.
    #[test]
    fn enter_puts_a_name_in_and_takes_one_out() {
        let mut names = names();
        type_in(&mut names, "jet");
        assert_eq!(press(&mut names, KeyCode::Enter), Outcome::Changed);
        assert_eq!(names.chosen(), ["Iosevka", "JetBrains Mono"]);
        // Still among the offers, which is where adding leaves the reader:
        // pressing it again adds the next one rather than taking back the
        // one they just chose.
        assert!(!names.rows()[names.window().focus()].chosen());
        // Up to their own list, where the same key means the other thing.
        press(&mut names, KeyCode::Up);
        assert_eq!(press(&mut names, KeyCode::Enter), Outcome::Changed);
        assert_eq!(names.chosen(), ["Iosevka"]);
    }

    /// Backspace takes one of the reader's names out, the way delete does,
    /// and puts nothing in -- and with something typed it is the query's.
    ///
    /// Deliberate break: drop the guard on the query. Stepping up from the
    /// offers onto `Iosevka` with `x` still typed, backspace then takes the
    /// face out rather than the `x`, which is a typo costing a font. And
    /// give delete back to `toggle`, where it was, and delete on an offer
    /// puts `JetBrains Mono` in.
    #[test]
    fn backspace_takes_a_name_out_only_when_nothing_is_typed() {
        let mut names = names();
        // On an offer, where neither has anything to take out.
        assert_eq!(press(&mut names, KeyCode::Backspace), Outcome::Consumed);
        assert_eq!(press(&mut names, KeyCode::Delete), Outcome::Consumed);
        assert_eq!(names.chosen(), ["Iosevka"]);

        // Something typed, and the reader on their own list above it.
        type_in(&mut names, "x");
        press(&mut names, KeyCode::Up);
        assert!(names.rows()[names.window().focus()].chosen());
        press(&mut names, KeyCode::Backspace);
        assert_eq!(names.chosen(), ["Iosevka"], "a letter cost a face");
        assert_eq!(names.query.said(), "", "the letter was not taken back");

        // And with nothing typed, on the same row, it is delete.
        let focus = names
            .rows()
            .iter()
            .position(Row::chosen)
            .expect("one of the reader's");
        names.window.set_focus(focus);
        assert_eq!(press(&mut names, KeyCode::Backspace), Outcome::Changed);
        assert!(names.chosen().is_empty());
    }

    /// The list opens on the first thing there is to add, not on the first
    /// thing the reader already has.
    ///
    /// Deliberate break: leaving the focus at nought opens it on one of
    /// their own names, where the one key that does anything takes that
    /// name out -- which is the first press of it undoing their answer.
    #[test]
    fn it_opens_where_a_name_can_be_added() {
        let names = names();
        let on = &names.rows()[names.window().focus()];
        assert!(matches!(on, Row::Offer { .. }), "{on:?}");
    }

    /// A name this machine does not have can still be added.
    ///
    /// One settings file is read on every machine the reader uses, so a
    /// face that is only on the other one is still an answer.
    ///
    /// Deliberate break: offering only what matched the machine's list
    /// leaves nothing to press enter on, and the name cannot be added at
    /// all.
    #[test]
    fn a_name_this_machine_does_not_have_is_still_a_name() {
        let mut names = names();
        type_in(&mut names, "Fira Code");
        let offered: Vec<&Row> = names
            .rows()
            .iter()
            .filter(|row| matches!(row, Row::Offer { .. }))
            .collect();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].name(), "Fira Code");
        assert!(!offered[0].here());
        assert_eq!(press(&mut names, KeyCode::Enter), Outcome::Changed);
        assert_eq!(names.chosen(), ["Iosevka", "Fira Code"]);
    }

    /// Alt and an arrow move one of the reader's names, and the focus goes
    /// with it.
    ///
    /// Deliberate break: swapping without moving the focus leaves the
    /// reader holding alt and watching two names trade places under a
    /// cursor that stays put.
    #[test]
    fn a_chosen_name_moves_with_the_focus() {
        let mut names = names();
        type_in(&mut names, "jet");
        press(&mut names, KeyCode::Enter);
        assert_eq!(names.chosen(), ["Iosevka", "JetBrains Mono"]);
        // Adding leaves the reader among the offers, and moving is about
        // their own list: up to the name they just added.
        press(&mut names, KeyCode::Up);
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(names.handle_key(&up, 10), Outcome::Changed);
        assert_eq!(names.chosen(), ["JetBrains Mono", "Iosevka"]);
        assert_eq!(names.window().focus(), 0);
        // And it stops at the end rather than wrapping round.
        assert_eq!(names.handle_key(&up, 10), Outcome::Consumed);
        assert_eq!(names.chosen(), ["JetBrains Mono", "Iosevka"]);
    }

    /// The focus never stands on the boundary or on the line that says
    /// nothing is chosen.
    ///
    /// Deliberate break: taking the step-over out leaves a row where enter
    /// does nothing, in the middle of a list where enter is the only key
    /// that does anything.
    #[test]
    fn the_focus_steps_over_what_is_not_a_name() {
        let empty = Names::new(Vec::new(), vec!["Iosevka".to_string()], None);
        // Nothing chosen: the first two rows say so and divide the page,
        // so the focus starts on the offer below them.
        assert!(empty.rows()[empty.window().focus()].stands());
        let mut names = names();
        for _ in 0..6 {
            press(&mut names, KeyCode::Down);
            assert!(
                names.rows()[names.window().focus()].stands(),
                "{:?}",
                names.rows()[names.window().focus()]
            );
        }
        for _ in 0..6 {
            press(&mut names, KeyCode::Up);
            assert!(names.rows()[names.window().focus()].stands());
        }
    }
}
