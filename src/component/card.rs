//! The card an agent's question is answered on.
//!
//! Not a picker. A picker is for finding one thing among many by typing at
//! it: a query, a fuzzy match, tabs, rows arriving from a walk. A question
//! is somebody else asking, with a handful of named answers and sometimes
//! room to write your own -- nothing is filtered, and what the reader types
//! is an answer rather than a search.
//!
//! Strip the filtering from a picker and nothing is left but the row
//! drawing, and what a card needs on top of that is a row that *grows*,
//! which the list machinery cannot have: every picker in obelus counts one
//! row per screen row, and a file list of thousands must not pay for a box
//! one caller wants. So this composes the two halves obelus already has --
//! the rows, and the [`Composer`] a message is written in, which is what a
//! message to an agent is written in everywhere else -- and `ui/card.rs`
//! draws them.
//!
//! It sits where the message box sits, because while an agent is waiting on
//! an answer the box has nothing to send: the question is what the
//! conversation is doing. What was said stays above it, shrunk by however
//! much the card needs. The conversation keeps the status row: a card is
//! part of it rather than a list opened over it.
//!
//! The form on it is asked in the agent's order. `elicitation/create`'s
//! schema arrives as a *map* of fields -- JSON objects have no order to keep
//! -- so the order the agent wrote them in is gone before obelus sees it,
//! and asking in the alphabet's order put an "Other, if none of these suit"
//! in front of the list it was an alternative to. What is left to go on is
//! `required`: those first, in the order the agent listed them, and the rest
//! after.
//!
//! A named-answer field and a words field next to it go on the *one* card,
//! because that pair is one question -- "these, or say what you want
//! instead" -- however many fields it takes to write down. Everything else
//! is a card of its own, in turn.
//!
//! What the agent does not need, a reader must be able to say nothing to:
//! they send the card with the box empty, and the field is left out of the
//! answer. Escape is not that answer -- escape gives up on the whole form,
//! which is the one thing a reader walking past an aside does not mean.

use crossterm::event::{KeyCode, KeyModifiers};

use super::composer::Composer;
use crate::coordinates::DisplayColumn;

/// How many rows of prose the question gets before its answers.
///
/// A card is where an answer is given, not where a long thing is read. What
/// a permission request is about is not put here at all when the call said
/// it in words -- the transcript has it, whole -- so what is left for this
/// to clamp is a form field's own question, which is a line or two.
const MOST_ABOUT: usize = 5;

/// One named answer.
#[derive(Clone, Debug)]
pub struct Choice {
    /// The agent's id for it, which is what the answer names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// What it means, if the agent said and it is not the name again.
    pub about: Option<String>,
    /// Drawn before the name, where a question has icons for its answers.
    pub icon: Option<char>,
    /// Whether it is one of the answers as things stand.
    pub chosen: bool,
}

/// The half of a card the reader writes in.
#[derive(Clone, Debug)]
struct Words {
    /// What the field is called, which is what the row says while nothing
    /// has been written in it.
    placeholder: String,
    /// Whether the agent said it needs one.
    required: bool,
    /// Whether the reader is writing at all.
    ///
    /// Always true where the answers are chosen one at a time: the box is
    /// simply the last row, and walking onto it is all it takes. Where
    /// they are ticked it is a tick of its own, because there every row is
    /// something to tick and a row that worked differently would be a
    /// second way of saying yes.
    wanted: bool,
    /// What they have written.
    composer: Composer,
}

/// Where the keys are going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
    /// One of the named answers.
    Choice(usize),
    /// The row that says whether the reader is writing one of their own.
    Tick,
    /// The words themselves.
    Words,
    /// The row that sends the lot.
    Submit,
}

/// What a key did to the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardOutcome {
    /// Not the card's key.
    Ignored,
    /// Taken: something moved, or was ticked, or was typed.
    Consumed,
    /// The reader gave up on the whole question.
    Cancelled,
    /// The answer, as the reader left it.
    Answered {
        /// The agent's ids for the named answers they chose.
        chosen: Vec<String>,
        /// What they wrote, if they wrote anything.
        words: Option<String>,
    },
}

/// A question, and the answer as it stands.
#[derive(Clone, Debug)]
pub struct Card {
    /// What the agent said the question is about.
    about: Option<String>,
    /// The named answers.
    choices: Vec<Choice>,
    /// Whether more than one of them may be chosen.
    several: bool,
    /// Whether the agent said one of them has to be.
    ///
    /// Which is the difference between a card that can be sent with
    /// nothing but the reader's own words on it and one that cannot: the
    /// agent said it will not take the form without a named answer, and
    /// offering to send it anyway would be obelus promising something on
    /// the agent's behalf.
    needed: bool,
    /// The fewest of them the agent will take, if it said.
    least: Option<u64>,
    /// And the most.
    most: Option<u64>,
    /// The words half, when the question has one.
    words: Option<Words>,
    /// Where the keys are going.
    on: On,
    /// Whether the reader has asked for something the card cannot do.
    ///
    /// Set by an enter that had to be refused and cleared by the next key,
    /// so what is missing is said when they ask for it and not before: a
    /// card that opens saying "choose one" is telling somebody who has not
    /// tried anything yet that they have got it wrong.
    complaining: bool,
}

impl Card {
    /// A question with these named answers.
    #[must_use]
    pub fn new(choices: Vec<Choice>, several: bool) -> Self {
        let mut card = Self {
            about: None,
            choices,
            several,
            needed: false,
            least: None,
            most: None,
            words: None,
            on: On::Choice(0),
            complaining: false,
        };
        if card.choices.is_empty() {
            card.on = On::Words;
        }
        card
    }

    /// Starts on the answer that is already in force.
    ///
    /// Where it starts rather than what is chosen: nothing is chosen on a
    /// card until the reader says so. A switch that is on opens with the
    /// reader on "on", which is one key from leaving it there.
    pub fn prefer(&mut self, id: &str) {
        if let Some(at) = self.choices.iter().position(|choice| choice.id == id) {
            self.focus(On::Choice(at));
        }
    }

    /// What the card cannot do yet, once the reader has asked it to.
    #[must_use]
    pub fn complaint(&self) -> Option<String> {
        self.complaining.then(|| self.wanting()).flatten()
    }

    /// What the agent said the question is about.
    pub fn about(&mut self, about: &str) {
        self.about = Some(about.to_string());
    }

    /// Says that one of the named answers has to be chosen.
    pub const fn needs_one(&mut self) {
        self.needed = true;
    }

    /// How many of them the agent will take.
    pub const fn counts(&mut self, least: Option<u64>, most: Option<u64>) {
        self.least = least;
        self.most = most;
    }

    /// Gives the question a half the reader writes in.
    ///
    /// Where there are several answers to tick, it starts wanted only when
    /// the agent needs it: the row is one of the ticks, and a tick nobody
    /// made should not be made for them.
    pub fn writing(&mut self, placeholder: &str, required: bool, suggested: Option<&str>) {
        let mut composer = Composer::new();
        if let Some(words) = suggested {
            composer.replace(words);
        }
        self.words = Some(Words {
            placeholder: placeholder.to_string(),
            required,
            wanted: !self.several || required,
            composer,
        });
        if self.choices.is_empty() {
            self.on = On::Words;
        }
    }

    /// What the question is about, for the view.
    #[must_use]
    pub fn what_about(&self) -> Option<&str> {
        self.about.as_deref()
    }

    /// The named answers, for the view.
    #[must_use]
    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }

    /// Whether the answers are ticked rather than picked.
    #[must_use]
    pub const fn several(&self) -> bool {
        self.several
    }

    /// Where the keys are going, for the view to say so.
    #[must_use]
    pub const fn on(&self) -> On {
        self.on
    }

    /// Which named answers are on screen, given the room they have.
    ///
    /// Worked out from where the reader is rather than remembered: a card's
    /// answers are few, and a window that keeps its own place would be one
    /// more thing that can be wrong about a list nobody scrolls.
    #[must_use]
    pub fn visible(&self, height: u16) -> std::ops::Range<usize> {
        let height = usize::from(height).max(1);
        let count = self.choices.len();
        if count <= height {
            return 0..count;
        }
        let at = match self.on {
            On::Choice(at) => at,
            // Below them, so what is on screen is the end of the list --
            // which is what the rows under it are attached to.
            On::Tick | On::Words | On::Submit => count - 1,
        };
        let top = at.saturating_sub(height - 1).min(count - height);
        top..top + height
    }

    /// Whether the card has a row saying the reader is writing their own
    /// answer as well.
    #[must_use]
    pub const fn has_tick(&self) -> bool {
        self.several && self.words.is_some()
    }

    /// Whether the agent left room to answer in the reader's own words.
    ///
    /// Not [`Self::writing_wanted`], which is whether they have said they
    /// are going to: this is whether there is anywhere for them to. A card
    /// of named answers and nothing else has nowhere text can go, which is
    /// what the application has to know before it offers a paste.
    #[must_use]
    pub const fn takes_words(&self) -> bool {
        self.words.is_some()
    }

    /// Whether the reader has said they are writing one.
    #[must_use]
    pub fn writing_wanted(&self) -> bool {
        self.words.as_ref().is_some_and(|words| words.wanted)
    }

    /// What the row the reader writes in says while it is empty.
    #[must_use]
    pub fn placeholder(&self) -> Option<&str> {
        self.words.as_ref().map(|words| words.placeholder.as_str())
    }

    /// What has been written, as it is drawn.
    #[must_use]
    pub fn written(&self, width: u16) -> Vec<String> {
        self.words
            .as_ref()
            .map(|words| words.composer.rows(width))
            .unwrap_or_default()
    }

    /// Whether anything has been written.
    #[must_use]
    pub fn blank(&self) -> bool {
        self.words
            .as_ref()
            .is_none_or(|words| words.composer.is_blank())
    }

    /// Where the caret is in what has been written.
    #[must_use]
    pub fn caret(&self, width: u16) -> Option<(usize, DisplayColumn)> {
        if self.on != On::Words {
            return None;
        }
        self.words.as_ref().map(|words| words.composer.caret(width))
    }

    /// Whether the card has a row of its own to send from.
    ///
    /// Only where answers are ticked: everywhere else every row the reader
    /// can be on is an answer, and enter on it is the answer being given.
    #[must_use]
    pub const fn has_submit(&self) -> bool {
        self.several
    }

    /// Whether the answer as it stands is one the agent will take, and what
    /// is missing when it is not.
    ///
    /// Said rather than refused: a row that cannot be used is drawn dim
    /// with the reason on it, which is how obelus says no to everything
    /// else -- a command that does not apply here, a row that cannot be
    /// chosen.
    #[must_use]
    pub fn wanting(&self) -> Option<String> {
        let chosen = self.chosen().len() as u64;
        if let Some(least) = self.least.filter(|least| chosen < *least) {
            return Some(format!("at least {least}"));
        }
        if let Some(most) = self.most.filter(|most| chosen > *most) {
            return Some(format!("at most {most}"));
        }
        if self.needed && chosen == 0 {
            return Some("Choose one".to_string());
        }
        if self
            .words
            .as_ref()
            .is_some_and(|words| words.required && words.composer.is_blank())
        {
            let name = self.placeholder().unwrap_or("An answer");
            return Some(format!("{name} first"));
        }
        None
    }

    /// How many rows the card wants, given the width it has.
    #[must_use]
    pub fn rows(&self, width: u16) -> usize {
        self.about_rows(width)
            + self.choices.len()
            + usize::from(self.has_tick())
            + self.written_rows(width)
            + if self.has_submit() { 2 } else { 0 }
            // What is missing, on a card with no row of its own to say it
            // on. One row, and only while it is being said.
            + usize::from(!self.has_submit() && self.complaint().is_some())
    }

    /// How many rows the prose above the answers takes, the rule under it
    /// included.
    #[must_use]
    pub fn about_rows(&self, width: u16) -> usize {
        let Some(about) = self.about.as_deref() else {
            return 0;
        };
        crate::text::wrapped(about, width)
            .len()
            .clamp(1, MOST_ABOUT)
            + 1
    }

    /// How many rows what has been written takes.
    #[must_use]
    pub fn written_rows(&self, width: u16) -> usize {
        match self.words.as_ref() {
            Some(words) if words.wanted => words.composer.rows(width).len().max(1),
            _ => 0,
        }
    }

    /// The agent's ids for the answers that are chosen.
    fn chosen(&self) -> Vec<String> {
        self.choices
            .iter()
            .filter(|choice| choice.chosen)
            .map(|choice| choice.id.clone())
            .collect()
    }

    /// Answers the question, or says what is missing.
    fn give(&mut self) -> CardOutcome {
        if self.wanting().is_some() {
            self.complaining = true;
            return CardOutcome::Consumed;
        }
        self.answer()
    }

    /// Answers the question with what is on the card.
    fn answer(&self) -> CardOutcome {
        let words = self
            .words
            .as_ref()
            .filter(|words| words.wanted && !words.composer.is_blank())
            .map(|words| words.composer.text());
        CardOutcome::Answered {
            chosen: self.chosen(),
            words,
        }
    }

    /// The rows the reader can be on, in the order they are drawn.
    fn walk(&self) -> Vec<On> {
        let mut rows: Vec<On> = (0..self.choices.len()).map(On::Choice).collect();
        if self.has_tick() {
            rows.push(On::Tick);
        }
        if self.writing_wanted() {
            rows.push(On::Words);
        }
        if self.has_submit() {
            rows.push(On::Submit);
        }
        rows
    }

    /// Puts the focus on a row.
    ///
    /// Walking does not choose. A card is walked past on the way to the
    /// box under it -- that is what the box is for -- and a card where the
    /// answer followed the focus would answer with whichever row the
    /// reader walked over last.
    const fn focus(&mut self, on: On) {
        self.on = on;
    }

    /// Moves the focus by rows, stopping at the ends.
    fn step(&mut self, by: isize) {
        let rows = self.walk();
        let Some(at) = rows.iter().position(|row| *row == self.on) else {
            // The row they were on is no longer one: the box they unticked,
            // most likely. The first row rather than nowhere, because a
            // card the arrows cannot move is a card with no way out but
            // escape.
            if let Some(row) = rows.first().copied() {
                self.focus(row);
            }
            return;
        };
        let next = at
            .saturating_add_signed(by)
            .min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(next).copied() else {
            return;
        };
        self.focus(row);
    }

    /// Ticks or unticks what the reader is on.
    fn tick(&mut self) {
        match self.on {
            On::Choice(at) => {
                if let Some(choice) = self.choices.get_mut(at) {
                    choice.chosen = !choice.chosen;
                }
            }
            On::Tick => {
                self.tick_words();
                // Ticked means they mean to write, so the caret goes where
                // the writing happens rather than leaving them to press
                // one more key to say it again.
                if self.writing_wanted() {
                    self.focus(On::Words);
                }
            }
            On::Words | On::Submit => {}
        }
    }

    /// Ticks the row that says the reader is writing an answer of their own.
    fn tick_words(&mut self) {
        if let Some(words) = self.words.as_mut() {
            words.wanted = !words.wanted;
        }
    }

    /// Takes a key.
    ///
    /// `width` is the cells a row of the card has, which the caret's
    /// arithmetic needs and nothing else here does.
    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent, width: u16) -> CardOutcome {
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
            return CardOutcome::Ignored;
        };
        // A line in the box, asked for either of the two ways the box takes
        // anywhere else in obelus: `shift+enter` is what a reader reaches
        // for and it needs the kitty keyboard protocol to arrive at all,
        // and alt is the escape prefix, which is as old as terminals. This
        // is the same box, asked a different question.
        //
        // Taken wherever the reader is on the card, like typing: asking
        // for a line is meaning to write. And swallowed on a card with no
        // box, because what is under one is the box a message is written
        // in and it is covered.
        if key.code == KeyCode::Enter
            && (modifiers == KeyModifiers::ALT || modifiers == KeyModifiers::SHIFT)
        {
            if self.words.is_some() {
                if !self.writing_wanted() {
                    self.tick_words();
                }
                self.focus(On::Words);
                self.write(Composer::newline);
            }
            return CardOutcome::Consumed;
        }
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return CardOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let writing = self.on == On::Words;
        let room = width.max(1);
        // Said in answer to the key that asked for it, and gone by the next
        // one: the reader is doing something about it.
        if key.code != KeyCode::Enter {
            self.complaining = false;
        }

        match key.code {
            KeyCode::Esc if bare => CardOutcome::Cancelled,
            KeyCode::Enter if bare => match self.on {
                // A tick, where several answers may be chosen. Nothing is
                // sent by it: the card is sent from its own row, because
                // ticking and sending cannot both be enter.
                On::Choice(_) | On::Tick if self.several => {
                    self.tick();
                    CardOutcome::Consumed
                }
                // One answer, and this is it: chosen and given in the one
                // key, along with whatever is in the box -- both are on
                // the card, so the card is answered at once.
                On::Choice(at) => {
                    for (index, choice) in self.choices.iter_mut().enumerate() {
                        choice.chosen = index == at;
                    }
                    self.give()
                }
                On::Tick | On::Words | On::Submit => self.give(),
            },
            KeyCode::Up if bare => {
                if writing
                    && let Some(words) = self.words.as_mut()
                    && words.composer.up(room)
                {
                    return CardOutcome::Consumed;
                }
                self.step(-1);
                CardOutcome::Consumed
            }
            KeyCode::Down if bare => {
                if writing
                    && let Some(words) = self.words.as_mut()
                    && words.composer.down(room)
                {
                    return CardOutcome::Consumed;
                }
                self.step(1);
                CardOutcome::Consumed
            }
            // The rest are the box's, and only while the reader is in it.
            // A card with nothing to type in has no use for them, and a key
            // that does nothing should reach the table where it might.
            //
            // Which keys those are is the box's own rule, asked rather than
            // repeated: there are two boxes now, and the same table written
            // in both is a table that will be right in one.
            _ if writing
                && self
                    .words
                    .as_mut()
                    .is_some_and(|words| words.composer.handle_key(key, room)) =>
            {
                CardOutcome::Consumed
            }
            // Typing goes in the box wherever the reader is on the card: a
            // reader who starts typing means to type, and the box is the
            // only thing on a card that takes characters.
            //
            // A card with no box swallows them rather than letting them
            // fall through. What is under a card is the box a message is
            // written in, and it is covered: a key that fell through would
            // be typing into something nobody can see, and it would be
            // sent to the agent as a message once the question was
            // answered.
            KeyCode::Char(character) if bare || modifiers == KeyModifiers::SHIFT => {
                if self.words.is_none() {
                    return CardOutcome::Consumed;
                }
                if !self.writing_wanted() {
                    self.tick_words();
                }
                self.focus(On::Words);
                self.write(|composer| composer.insert(character));
                CardOutcome::Consumed
            }
            _ => CardOutcome::Ignored,
        }
    }

    /// Does something to what is being written.
    /// Puts pasted text in the box, and says whether there was one.
    ///
    /// The same rule the key for a new line follows, three lines above the
    /// key table and for the same reason: taken from wherever the reader is
    /// on the card, and it ticks the box on where the box has a tick,
    /// because pasting into a card is meaning to write in it. A card the
    /// agent left no room to write on says so rather than swallowing it --
    /// the box under this one is covered, and text put there is text nobody
    /// can see.
    pub fn paste(&mut self, what: &str, width: u16) -> bool {
        if self.words.is_none() {
            return false;
        }
        if !self.writing_wanted() {
            self.tick_words();
        }
        self.focus(On::Words);
        self.write(|composer| composer.write_in(what, width));
        true
    }

    fn write(&mut self, edit: impl FnOnce(&mut Composer)) {
        if let Some(words) = self.words.as_mut() {
            edit(&mut words.composer);
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    /// The room a card's rows have in these tests.
    const ROOM: u16 = 40;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn answers() -> Vec<Choice> {
        ["one", "two", "three"]
            .into_iter()
            .map(|name| Choice {
                id: name.to_string(),
                name: name.to_string(),
                about: None,
                icon: None,
                chosen: false,
            })
            .collect()
    }

    /// Walking a card is not answering it.
    ///
    /// The box is under the answers, so every way to it walks over them. A
    /// card whose answer followed the focus would answer with whichever row
    /// the reader passed over last -- and the reader would never see it
    /// happen, because they were on their way somewhere else.
    #[test]
    fn walking_over_an_answer_does_not_choose_it() {
        let mut card = Card::new(answers(), false);
        card.writing("Other", false, None);
        for _ in 0..3 {
            card.handle_key(&key(KeyCode::Down), ROOM);
        }
        assert_eq!(card.on(), On::Words, "the box is not under the answers");
        assert!(
            card.choices().iter().all(|choice| !choice.chosen),
            "walking past the answers chose one"
        );
        // And what it sends is what the reader said: their own words, and
        // no named answer they never chose.
        let outcome = card.handle_key(&key(KeyCode::Char('h')), ROOM);
        assert_eq!(outcome, CardOutcome::Consumed);
        assert_eq!(
            card.handle_key(&key(KeyCode::Enter), ROOM),
            CardOutcome::Answered {
                chosen: Vec::new(),
                words: Some("h".to_string()),
            }
        );
    }

    /// One key answers a card with one answer on it: the row it is on.
    #[test]
    fn enter_on_an_answer_gives_it_along_with_what_was_written() {
        let mut card = Card::new(answers(), false);
        card.writing("Other", false, None);
        card.handle_key(&key(KeyCode::Char('w')), ROOM);
        assert_eq!(card.on(), On::Words, "typing did not reach the box");
        // Back up to the answers, which is three rows: the box is under
        // the last of them.
        for _ in 0..3 {
            card.handle_key(&key(KeyCode::Up), ROOM);
        }
        assert_eq!(card.on(), On::Choice(0));
        assert_eq!(
            card.handle_key(&key(KeyCode::Enter), ROOM),
            CardOutcome::Answered {
                chosen: vec!["one".to_string()],
                words: Some("w".to_string()),
            },
            "the choice and the words are not one answer"
        );
    }

    /// A card that takes several answers ticks rather than answers, and says
    /// how many it is short.
    #[test]
    fn several_answers_are_ticked_and_counted() {
        let mut card = Card::new(answers(), true);
        card.counts(Some(2), None);
        card.needs_one();
        assert_eq!(
            card.handle_key(&key(KeyCode::Enter), ROOM),
            CardOutcome::Consumed
        );
        assert!(card.choices()[0].chosen, "enter did not tick");
        // The row that sends it is the last of them, and it will not send
        // one answer where the agent asked for two.
        while card.on() != On::Submit {
            card.handle_key(&key(KeyCode::Down), ROOM);
        }
        assert_eq!(
            card.handle_key(&key(KeyCode::Enter), ROOM),
            CardOutcome::Consumed
        );
        assert_eq!(card.complaint().as_deref(), Some("at least 2"));
        // Ticked again, and it goes -- in the order the answers are in
        // rather than the order they were ticked, because that is the
        // order the agent listed them in.
        card.handle_key(&key(KeyCode::Up), ROOM);
        card.handle_key(&key(KeyCode::Enter), ROOM);
        assert_eq!(card.complaint(), None, "the complaint outlived the answer");
        card.handle_key(&key(KeyCode::Down), ROOM);
        assert_eq!(
            card.handle_key(&key(KeyCode::Enter), ROOM),
            CardOutcome::Answered {
                chosen: vec!["one".to_string(), "three".to_string()],
                words: None,
            }
        );
    }

    /// A line in the box is asked for the way it is asked for in the box a
    /// message is written in: with shift, or with alt where the terminal
    /// cannot report shift.
    ///
    /// Both, because enter alone sends the card. A card that took only one
    /// of them left the other falling through to the box underneath -- the
    /// one the card is covering -- where the line went into a message
    /// nobody could see.
    #[test]
    fn shift_and_enter_makes_a_line_rather_than_sending() {
        for held in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
            let mut card = Card::new(answers(), false);
            card.writing("Other", false, None);
            card.handle_key(&key(KeyCode::Char('a')), ROOM);
            assert_eq!(
                card.handle_key(&KeyEvent::new(KeyCode::Enter, held), ROOM),
                CardOutcome::Consumed,
                "{held:?} and enter sent the card"
            );
            card.handle_key(&key(KeyCode::Char('b')), ROOM);
            assert_eq!(
                card.written(ROOM),
                ["a", "b"],
                "{held:?} and enter did not make a line"
            );
        }
    }

    /// A card with nothing to type in still swallows what is typed.
    ///
    /// What is under a card is the box a message is written in, and the
    /// card covers it. A character that fell through would go somewhere
    /// nobody can see and be sent to the agent once the question was
    /// answered.
    #[test]
    fn a_card_with_no_box_swallows_what_is_typed() {
        let mut card = Card::new(answers(), false);
        assert_eq!(
            card.handle_key(&key(KeyCode::Char('x')), ROOM),
            CardOutcome::Consumed
        );
        // A line break has nowhere to go either, and the box it would fall
        // through to is the one the card covers.
        assert_eq!(
            card.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), ROOM),
            CardOutcome::Consumed
        );
        // And a key that is not typing still falls through, or there would
        // be no way out of a card but escape.
        assert_eq!(
            card.handle_key(
                &KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
                ROOM
            ),
            CardOutcome::Ignored
        );
    }
}
