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
//! which the list machinery cannot have: every picker in Obelus counts one
//! row per screen row, and a file list of thousands must not pay for a box
//! one caller wants. So this composes the two halves Obelus already has --
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
//! -- so the order the agent wrote them in is gone before Obelus sees it,
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
//!
//! **Enter acts on the row the reader is on, and that is the whole key
//! table.** One answer: enter on it answers the card, with whatever is in
//! the box. Many: enter ticks, and the card is sent from a row that says
//! `Submit`, because ticking and sending cannot both be enter. In the box:
//! enter sends, `alt` and enter makes a line, which is what enter does in
//! the box anywhere else in Obelus. No new key was needed -- not even space,
//! which everywhere else in Obelus is a character. The rest is said where
//! it is done: walking does not choose (`Card::focus`), typing goes to the
//! box wherever the reader is and a card with no box swallows it
//! (`handle_key`), and what the card cannot do yet it says only once the
//! reader has asked for it (`complaining`).
//!
//! **A question the reader did not start says what it is about.** The card
//! carries an `about` -- prose above its answers, a rule under it -- and
//! both questions an agent can ask fill it: a form puts its own message
//! there and the question of the field on the card (`App::put_the_question`
//! says when each), and a permission request what the agent is actually going
//! to do (`reason_of` in `obelus-agent`'s `acp/link`, which says why not
//! `raw_input`). "Allow" and "refuse" are answers, and a question with the
//! words missing is not one a reader can answer. It is wrapped to the width
//! and capped (`MOST_ABOUT`): it is somebody else's prose, and an agent
//! explaining itself at length must not push the list it belongs to off the
//! screen. The compact list keeps an `about` of its own for the same reason
//! (`Picker::about`).
//!
//! A form said it in a transcript line of its own once ("it asks: ..."),
//! which is the same words twice: the question is on screen, and what it is
//! about belongs over it rather than above the last thing the agent said.
//! The same went for a permission request, which Obelus introduced with a
//! note of its own ("asking to run the tests") over the question: the call
//! goes where every call goes now, waiting, which is what says the agent is
//! asking about it -- and where that row carries the words itself, the card
//! says nothing rather than quote the first five rows of them
//! (`App::ask_permission`).

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_text::coordinates::DisplayColumn;

use super::composer::Composer;

/// How far under an answer's name what it means is drawn.
///
/// A fixed step rather than the column the name happens to start in: the
/// tick and the icon are two cells and a card has one or the other, so this
/// is where the name begins either way -- and a card that measured its rows
/// against one column and drew them in another would lay out rows it then
/// does not draw.
pub const UNDER: u16 = 2;

/// How many rows of prose the question gets before its answers.
///
/// A card is where an answer is given, not where a long thing is read. What
/// a permission request is about is not put here at all when the call said
/// it in words -- the transcript has it, whole -- so what is left for this
/// to clamp is a form field's own question, which is a line or two, and on
/// a form's first card the agent's message and a blank row over it.
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

/// What an answer is short of, said one way on the card and another in a
/// chat.
enum Short {
    Least(u64),
    Most(u64),
    One,
    Words,
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
    /// offering to send it anyway would be Obelus promising something on
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

    /// How many rows one answer takes: its name, and under it what the
    /// agent said it means, wrapped.
    ///
    /// Not cut short. What an agent writes here is a sentence about what
    /// choosing this would do, and the answers to one question are told
    /// apart by exactly those sentences -- an ellipsis through the middle
    /// of them is a card that has asked something and then covered up the
    /// difference between its answers.
    #[must_use]
    pub fn choice_rows(&self, at: usize, width: u16) -> usize {
        let Some(choice) = self.choices.get(at) else {
            return 0;
        };
        let under = match choice.about.as_deref() {
            Some(about) => obelus_text::wrapped(about, width.saturating_sub(UNDER).max(1)).len(),
            None => 0,
        };
        1 + under
    }

    /// Which named answers are on screen, given the room they have.
    ///
    /// Worked out from where the reader is rather than remembered: a card's
    /// answers are few, and a window that keeps its own place would be one
    /// more thing that can be wrong about a list nobody scrolls.
    ///
    /// Whole answers only. An answer is a name and the sentence under it
    /// saying what it means, and half of that is a row of prose with
    /// nothing above it saying which answer it belongs to -- so the list
    /// scrolls by answers even though it is measured in rows. The one
    /// exception is an answer taller than the whole band, which is shown
    /// and clipped: something has to be on screen.
    #[must_use]
    pub fn visible(&self, height: u16, width: u16) -> std::ops::Range<usize> {
        let height = usize::from(height).max(1);
        let count = self.choices.len();
        if count == 0 {
            return 0..0;
        }
        let tall: Vec<usize> = (0..count).map(|at| self.choice_rows(at, width)).collect();
        if tall.iter().sum::<usize>() <= height {
            return 0..count;
        }
        let at = match self.on {
            On::Choice(at) => at.min(count - 1),
            // Below them, so what is on screen is the end of the list --
            // which is what the rows under it are attached to.
            On::Tick | On::Words | On::Submit => count - 1,
        };
        // Up from the one the reader is on, so walking down the list moves
        // it to the foot of the band rather than to the head of it, and
        // then down into whatever is left.
        let mut used = tall[at];
        let mut top = at;
        while top > 0 && used + tall[top - 1] <= height {
            used += tall[top - 1];
            top -= 1;
        }
        let mut bottom = at + 1;
        while bottom < count && used + tall[bottom] <= height {
            used += tall[bottom];
            bottom += 1;
        }
        top..bottom
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

    /// What has been written, as it is drawn -- with what is held in it,
    /// because a selection nothing shows is a key that looks broken, and
    /// one typed over is words gone that the reader never saw were held.
    #[must_use]
    pub fn written(&self, width: u16) -> Vec<crate::composer::Laid> {
        self.words
            .as_ref()
            .map(|words| words.composer.laid(width))
            .unwrap_or_default()
    }

    /// What is held in the words, if anything is.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        self.words.as_ref()?.composer.selected()
    }

    /// What a copy takes from the card: what is held in the words, or all
    /// of them. Nothing on a card with nowhere to write.
    #[must_use]
    pub fn copied(&self) -> Option<(String, &'static str)> {
        let words = self.words.as_ref()?;
        Some(match words.composer.selected() {
            Some(held) => (held, "selection"),
            None => (words.composer.text(), "answer"),
        })
    }

    /// The same, and takes it out.
    pub fn cut(&mut self, width: u16) -> Option<(String, &'static str)> {
        let words = self.words.as_mut()?;
        Some(match words.composer.cut(width.max(1)) {
            Some(held) => (held, "selection"),
            None => (words.composer.take(), "answer"),
        })
    }

    /// Takes hold of everything written, from wherever the reader is on
    /// the card -- the rule a paste follows, and for the same reason:
    /// taking all of the words is meaning to do something to them.
    pub fn select_all(&mut self, width: u16) {
        if self.words.is_none() {
            return;
        }
        if !self.writing_wanted() {
            self.tick_words();
        }
        self.focus(On::Words);
        self.write(|composer| composer.select_all(width));
    }

    /// Lets go of what is held in the words, and says whether anything
    /// was.
    pub fn let_go(&mut self) -> bool {
        let Some(words) = self.words.as_mut() else {
            return false;
        };
        let held = words.composer.selected().is_some();
        words.composer.let_go();
        held
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
    /// with the reason on it, which is how Obelus says no to everything
    /// else -- a command that does not apply here, a row that cannot be
    /// chosen.
    #[must_use]
    pub fn wanting(&self) -> Option<String> {
        let written = self
            .words
            .as_ref()
            .is_some_and(|words| !words.composer.is_blank());
        Some(match self.short_of(self.chosen().len() as u64, written)? {
            Short::Least(least) => format!("at least {least}"),
            Short::Most(most) => format!("at most {most}"),
            Short::One => "Choose one".to_string(),
            Short::Words => format!("{} first", self.placeholder().unwrap_or("An answer")),
        })
    }

    /// What an answer of `chosen` named answers, with words or without, is
    /// short of -- the one judgement of whether an answer will do, asked of
    /// the card on screen and of a reply from a chat alike, so the two
    /// cannot take different answers to one question.
    fn short_of(&self, chosen: u64, written: bool) -> Option<Short> {
        if let Some(least) = self.least.filter(|least| chosen < *least) {
            return Some(Short::Least(least));
        }
        if let Some(most) = self.most.filter(|most| chosen > *most) {
            return Some(Short::Most(most));
        }
        if self.needed && chosen == 0 {
            return Some(Short::One);
        }
        if self
            .words
            .as_ref()
            .is_some_and(|words| words.required && !written)
        {
            return Some(Short::Words);
        }
        None
    }

    /// The question in words, for a chat that cannot be given the card:
    /// what it is about, the named answers numbered from one, and how to
    /// answer it by replying.
    ///
    /// Here rather than wherever the chat is, so that the card on screen
    /// and the question in the chat are one question asked from one place:
    /// [`Card::answered_by`] reads the reply against the same numbers.
    #[must_use]
    pub fn in_words(&self) -> String {
        let mut said = String::from("\u{2753} ");
        if let Some(about) = &self.about {
            said.push_str(about.trim());
        }
        for (at, choice) in self.choices.iter().enumerate() {
            said.push_str(&format!("\n{}. {}", at + 1, choice.name));
        }
        let numbers = match self.several {
            true => {
                let bounds = match (self.least, self.most) {
                    (Some(least), Some(most)) => format!(" (at least {least}, at most {most})"),
                    (Some(least), None) => format!(" (at least {least})"),
                    (None, Some(most)) => format!(" (at most {most})"),
                    (None, None) => String::new(),
                };
                format!("the numbers{bounds}")
            }
            false => "a number".to_string(),
        };
        // Said from what `answered_by` will take, so that the sentence is
        // never an invitation to a reply that is then refused.
        let how = match (self.choices.is_empty(), &self.words) {
            (true, _) => "Reply with your answer.".to_string(),
            (false, None) => format!("Reply with {numbers}."),
            (false, Some(words)) if words.required => {
                format!("Reply with {numbers}, then your own words.")
            }
            // Words alone answer nothing where a named answer has to be
            // chosen: one, or at least some.
            (false, Some(_)) if self.needed || self.least.is_some_and(|least| least > 0) => {
                format!("Reply with {numbers}, and your own words after it if you like.")
            }
            (false, Some(_)) => format!("Reply with {numbers}, or in your own words."),
        };
        said.push('\n');
        said.push_str(&how);
        said
    }

    /// What a reply in words answers: the ids of the named answers it
    /// chose and the words it carries, or why it cannot be taken -- said to
    /// the reader, who is then still being asked.
    ///
    /// Numbers first, then the reader's own words where the card takes
    /// some: `2 because it is smaller`. Words with no numbers are words
    /// alone. A reply with words on a card that takes none is not an
    /// answer: the agent said what it would take, and Obelus guessing
    /// which was meant is Obelus answering for them. And whatever it is,
    /// it is held to what the card on screen holds an answer to.
    ///
    /// # Errors
    ///
    /// Why the reply cannot be taken, in a sentence.
    pub fn answered_by(&self, reply: &str) -> Result<(Vec<String>, Option<String>), String> {
        let reply = reply.trim();
        let ours = self.words.is_some();
        if self.choices.is_empty() {
            return match reply.is_empty() {
                true => Err("Reply with your answer".to_string()),
                false => Ok((Vec::new(), Some(reply.to_string()))),
            };
        }
        let separator = |character: char| character == ',' || character.is_whitespace();
        let mut numbers: Vec<usize> = Vec::new();
        let mut rest = reply;
        loop {
            let word = rest.trim_start_matches(separator);
            let end = word.find(separator).unwrap_or(word.len());
            match word[..end].trim_end_matches('.').parse() {
                Ok(number) if end > 0 => {
                    numbers.push(number);
                    rest = &word[end..];
                }
                _ => {
                    rest = word;
                    break;
                }
            }
        }
        let words = Some(rest.trim().to_string()).filter(|words| !words.is_empty());
        if (numbers.is_empty() && words.is_none()) || (words.is_some() && !ours) {
            return Err(format!(
                "Reply with a number from 1 to {}",
                self.choices.len()
            ));
        }
        if let Some(missing) = numbers
            .iter()
            .find(|number| **number == 0 || **number > self.choices.len())
        {
            return Err(format!("There is no {missing}"));
        }
        let mut chosen: Vec<String> = Vec::new();
        for number in numbers {
            let id = self.choices[number - 1].id.clone();
            if !chosen.contains(&id) {
                chosen.push(id);
            }
        }
        if !self.several && chosen.len() > 1 {
            return Err("Just one number".to_string());
        }
        match self.short_of(chosen.len() as u64, words.is_some()) {
            None => Ok((chosen, words)),
            Some(Short::Least(least)) => Err(format!("At least {least}")),
            Some(Short::Most(most)) => Err(format!("At most {most}")),
            Some(Short::One) => Err("Choose one of the numbers as well".to_string()),
            Some(Short::Words) => Err(format!(
                "{} as well, after the number",
                self.placeholder().unwrap_or("An answer")
            )),
        }
    }

    /// The name of a named answer, by its id.
    #[must_use]
    pub fn name_of(&self, id: &str) -> Option<&str> {
        self.choices
            .iter()
            .find(|choice| choice.id == id)
            .map(|choice| choice.name.as_str())
    }

    /// How many rows the card wants, given the width it has.
    #[must_use]
    pub fn rows(&self, width: u16) -> usize {
        self.about_rows(width)
            + (0..self.choices.len())
                .map(|at| self.choice_rows(at, width))
                .sum::<usize>()
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
        obelus_text::wrapped(about, width)
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

    /// Puts the focus on one of the card's rows.
    ///
    /// For a pointer: the keys step through the rows and have no use for
    /// naming one outright, and a press names one.
    pub const fn stand_on(&mut self, on: On) {
        self.focus(on);
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
        let Some(modifiers) = obelus_editing::keymap::modifiers_of(key) else {
            return CardOutcome::Ignored;
        };
        // A line in the box, asked for either of the two ways the box takes
        // anywhere else in Obelus: `shift+enter` is what a reader reaches
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
        // Sending now is the box's, and the box is under the card: let
        // through, the key stopped the turn from under a question and left
        // the question up with nobody waiting for its answer.
        if key.code == KeyCode::Enter && modifiers == KeyModifiers::CONTROL {
            return CardOutcome::Consumed;
        }
        // Holding a word at a time, or all the way to an end of what is
        // written, which is the box's -- and only while the reader is in
        // it, like the rest of the keys that move about it.
        if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
            )
        {
            if self.on != On::Words {
                return CardOutcome::Ignored;
            }
            self.write(|composer| {
                composer.handle_key(key, width.max(1));
            });
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
            // What is held in the words first, which is nearer than the
            // question: escape gives up on the whole form, and a reader
            // letting go of a selection does not mean that.
            KeyCode::Esc if bare && self.let_go() => CardOutcome::Consumed,
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

    /// Puts the caret in the words at a row and a cell of the box.
    ///
    /// For a pointer. Where the box is on screen and how tall it is belong
    /// to the drawing, so the caller hands in a place in the *box* rather
    /// than a place on the screen.
    ///
    /// `extend` is a drag: the place the button went down stays put and
    /// this end moves.
    pub fn place_in_words(&mut self, row: usize, cell: u16, width: u16, extend: bool) {
        self.write(|composer| {
            composer.place_at_cell(
                u16::try_from(row).unwrap_or(u16::MAX),
                cell,
                width.max(1),
                extend,
            );
        });
    }

    /// Takes hold of the word under the caret, or of its line: what a
    /// second and a third click mean in every box in Obelus.
    pub fn hold_in_words(&mut self, line: bool, width: u16) {
        self.write(|composer| match line {
            true => composer.hold_line(width.max(1)),
            false => composer.hold_word(width.max(1)),
        });
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

    /// A question in words is the card's question: its answers numbered
    /// from one, and a sentence on how to reply that says what the card
    /// will take.
    ///
    /// Broken deliberately by numbering from nought: the first answer was
    /// `0.` and the reply that chose it was refused.
    #[test]
    fn a_card_in_words_numbers_its_answers_and_says_how_to_reply() {
        let mut card = Card::new(answers(), false);
        card.about("Which one?");
        assert_eq!(
            card.in_words(),
            "\u{2753} Which one?\n1. one\n2. two\n3. three\nReply with a number."
        );
        card.writing("Other", false, None);
        assert!(
            card.in_words()
                .ends_with("Reply with a number, or in your own words.")
        );

        let mut several = Card::new(answers(), true);
        several.counts(Some(1), Some(2));
        assert!(
            several
                .in_words()
                .ends_with("Reply with the numbers (at least 1, at most 2)."),
            "{}",
            several.in_words()
        );
        assert!(
            Card::new(Vec::new(), false)
                .in_words()
                .ends_with("Reply with your answer.")
        );
    }

    /// A reply is read against the same numbers: one where one is asked,
    /// several within what the agent will take, the reader's own words
    /// where the card takes some, and a sentence where it will not do.
    ///
    /// Broken deliberately three ways. Not checking the count: two numbers
    /// answered a question that takes one. Not checking the bounds: three
    /// answered one that takes at most two. And taking words where the
    /// card takes none: "the second" was sent as an answer nobody asked
    /// for.
    #[test]
    fn a_reply_in_words_is_read_against_the_card() {
        let card = Card::new(answers(), false);
        assert_eq!(card.answered_by(" 2 "), Ok((vec!["two".to_string()], None)));
        assert_eq!(card.answered_by("2."), Ok((vec!["two".to_string()], None)));
        assert_eq!(card.answered_by("1 3"), Err("Just one number".to_string()));
        assert_eq!(card.answered_by("4"), Err("There is no 4".to_string()));
        assert_eq!(
            card.answered_by("the second"),
            Err("Reply with a number from 1 to 3".to_string())
        );

        let mut card = Card::new(answers(), false);
        card.writing("Other", false, None);
        assert_eq!(
            card.answered_by("neither, thanks"),
            Ok((Vec::new(), Some("neither, thanks".to_string())))
        );

        let mut several = Card::new(answers(), true);
        several.counts(Some(1), Some(2));
        assert_eq!(
            several.answered_by("1, 3"),
            Ok((vec!["one".to_string(), "three".to_string()], None))
        );
        assert_eq!(several.answered_by("1 2 3"), Err("At most 2".to_string()));

        let words = Card::new(Vec::new(), false);
        assert_eq!(
            words.answered_by("feature-x"),
            Ok((Vec::new(), Some("feature-x".to_string())))
        );
    }

    /// A reply from a chat is held to what the card on screen holds an
    /// answer to: words the agent needs, the fewest it will take, a named
    /// answer it needs -- and a number with words after it is both.
    ///
    /// Broken deliberately three ways. Not asking for the words when the
    /// reply is a number: `2` went out with no reason, which the card on
    /// screen refuses. Not counting when the reply is words: `foo` went out
    /// choosing none of a card that takes at least one. Reading the whole
    /// reply as numbers or as words: `2 because` was neither. And asking a
    /// card that takes at least one as if words alone would do: it invited
    /// `foo`, and then refused it.
    #[test]
    fn a_reply_is_held_to_what_the_card_holds_an_answer_to() {
        let mut reasoned = Card::new(answers(), false);
        reasoned.writing("Reason", true, None);
        assert_eq!(
            reasoned.answered_by("2"),
            Err("Reason as well, after the number".to_string())
        );
        assert_eq!(
            reasoned.answered_by("2 because it is smaller"),
            Ok((
                vec!["two".to_string()],
                Some("because it is smaller".to_string())
            ))
        );
        assert!(
            reasoned
                .in_words()
                .ends_with("Reply with a number, then your own words."),
            "{}",
            reasoned.in_words()
        );

        let mut several = Card::new(answers(), true);
        several.counts(Some(1), None);
        several.writing("Other", false, None);
        assert_eq!(several.answered_by("foo"), Err("At least 1".to_string()));
        // And the sentence asking it says so, rather than inviting the words
        // alone that it then refuses.
        assert!(
            several.in_words().ends_with(
                "Reply with the numbers (at least 1), and your own words after it if you like."
            ),
            "{}",
            several.in_words()
        );
        assert_eq!(
            several.answered_by("1, 3 and that"),
            Ok((
                vec!["one".to_string(), "three".to_string()],
                Some("and that".to_string())
            ))
        );

        let mut needed = Card::new(answers(), false);
        needed.needs_one();
        needed.writing("Other", false, None);
        assert_eq!(
            needed.answered_by("neither"),
            Err("Choose one of the numbers as well".to_string())
        );
        assert_eq!(
            needed.answered_by("3 or so"),
            Ok((vec!["three".to_string()], Some("or so".to_string())))
        );
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
                card.written(ROOM)
                    .into_iter()
                    .map(|row| row.said)
                    .collect::<Vec<_>>(),
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
