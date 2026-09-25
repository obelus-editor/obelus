//! The settings view: a tab per group, a row per setting, a control per row.
//!
//! It holds no settings of its own. What there is to show comes from
//! [`obelus_config::ALL`](obelus_config::ALL) and what each row says comes from
//! the [`Config`] it is handed, so a setting added to that table appears here
//! with nothing changed in this file.
//!
//! Changing anything produces [`SettingsOutcome::Changed`] rather than
//! writing to the config: this is a view, and what a change means -- apply
//! it, save the file, say why it could not be saved -- belongs to the
//! application.
//!
//! A setting is two rows: a name, and what it does under it. The name on
//! its own row with its control at the right, what it does on the rows under
//! that -- indented, in the dim colour, *wrapped* -- and a blank before the
//! next one, which is what makes an entry an entry rather than three rows of a
//! table. The same reason the agents' cards have one.
//!
//! Beside the name, the two competed for one row and the description lost: cut
//! off with an ellipsis on exactly the rows that had most to explain, and cut
//! further still on a row a project had pinned, where the file's name takes the
//! space as well. Under it, the sentence has the width of the page and can say
//! what it means -- `wrap` can say that lines break between words, `icons` can
//! say what a terminal without the font will draw.
//!
//! So the entries are not all one row tall, and the window is settled by
//! *height*, the way the page of cards already was. `Settings::setting_rows` is
//! the one answer to how tall one is, asked by the page laying them out and by
//! the window deciding which are on screen: two answers there is a reader
//! walking onto an entry nobody drew.
//!
//! A setting is a name and a gloss, not a sentence. `Colour theme`, `Nerd Font
//! glyphs`, `Wrap long lines`, `Blame in the margin` -- a noun phrase naming
//! the thing, not a clause about it. These were whole sentences ("Who last
//! changed the line the cursor is on") on the grounds that a name and a
//! description side by side read as a heading and a footnote. True when the
//! footnote says what the heading already had; what it produced was a page of
//! prose, where a reader looking for one row had to read every row to find it.
//! A column of names is *scanned*.
//!
//! The keys page, whose rows are one row each, starts what a command does in
//! one column two past the longest name rather than two past its own: four
//! beginnings to find is four, and one is one.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_agent::{Listed as Agent, Status, options::Offer};
use obelus_command::Command;
use obelus_config::{Config, Group, Kind, Setting, Value, Whose};
use obelus_editing::keymap::{KeyChord, Keymap};

use crate::{
    field::Field,
    window::{Move, Window, Wrap},
};

/// What a key did to the settings view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// The key means nothing here; let the key table have it.
    Ignored,
    /// Something moved or opened. Redraw.
    Consumed,
    /// A setting was changed, and this is what to.
    Changed(&'static str, Value),
    /// A setting should stop being the project's, and go back to being
    /// whatever the reader has.
    ///
    /// Only from the project's page, where `delete` means what it means on the
    /// keys page: take this one out.
    Unset(&'static str),
    /// An agent should be installed.
    Install(String),
    /// An agent should be the one Obelus talks to.
    Activate(String),
    /// Whichever agent was active should stop being.
    Deactivate,
    /// A setting with choices wants its list: which setting, what the
    /// choices are, and which one it is set to now.
    ///
    /// The list itself is the ordinary picker, opened by the application
    /// over this view. This one asks; it does not draw a list of its own.
    Choose(&'static str, &'static [&'static str], String),
    /// A command should be on this key from now on, or on none.
    ///
    /// Only ever a key nothing else is on: what is taken is said on the row
    /// and asked again, because the reader is looking at the row and it is
    /// that binding the answer is about.
    Bind(Command, Option<KeyChord>),
    /// One of the active agent's settings wants its values.
    ///
    /// Which setting, by the agent's id for it. What the values are is not
    /// carried: they came from the application in the first place, and a
    /// list of them travelling back out would be a second copy to keep
    /// level with the first.
    ///
    /// Its own outcome and not [`SettingsOutcome::Choose`]: that one names
    /// a setting of Obelus's own by a key Obelus wrote, and this one names
    /// somebody else's setting in somebody else's words.
    ChooseForAgent(String),
    /// One of them should stop being set, and go back to being the
    /// agent's.
    ///
    /// `delete`, which is what it means on the project's page: take this one
    /// out. Here what is left is not a default of Obelus's but the agent's
    /// own answer, which is the third state every one of these rows has.
    UnsetForAgent(String),
    /// The reader is done with the view.
    Cancelled,
}

/// Why the key a reader pressed would not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Another command has it.
    Taken(Command),
    /// It could never fire, and this is why.
    Never(&'static str),
}

/// How wide a setting's description is drawn, in a page this wide.
///
/// Indented under the name and stopping short of the right-hand edge, so a
/// paragraph under a name reads as belonging to it rather than as a row of
/// its own. Here rather than in the view because the window has to know how
/// tall an entry is before anything is drawn.
#[must_use]
pub fn description_width(room: u16) -> u16 {
    room.saturating_sub(DESCRIPTION_INDENT + GROUP_INDENT + 2)
        .max(8)
}

/// How far a description sits in from the name above it.
pub const DESCRIPTION_INDENT: u16 = 3;

/// How many rows the agent's heading takes: its name, the line saying when
/// what is under it takes effect, and the blank under that.
///
/// The line is the whole of what tells a reader that these rows are about
/// the *next* conversation. Without it the group is indistinguishable from
/// Obelus's own, whose settings take effect where they stand -- and the way
/// that is found out is by changing one and going back to a conversation
/// that has not moved.
pub const HEADING_ROWS: u16 = 3;

/// How far a setting sits in from the heading of the group it is in.
///
/// Which is the whole of what says where one group ends and the next
/// begins: no rule, no glyph, no second colour -- the same way the counts
/// say which directory a file is in, and the same way a description under a
/// name says it belongs to that name. A page whose groups were told apart
/// by a line across it would be a page with six lines on it, counting the
/// tabs' and the foot's, and the lines would be the loudest thing there.
pub const GROUP_INDENT: u16 = 2;

/// How many rows of a description a card will show.
///
/// Three: enough for the longest in the registry, and a limit so that one
/// verbose entry cannot push every other card off the screen.
const MOST_DESCRIPTION_ROWS: usize = 3;

/// Which of the settings' pages is showing.
///
/// Three, because there are three *shapes* of page here and not because
/// there are three kinds of setting: a column of settings with a control
/// each, a table of commands and the key each is on, and a shelf of cards
/// from a registry that changes while it is being looked at.
///
/// Which group a setting belongs to is a heading down the first of those
/// rather than a tab of its own. A tab is for somewhere else to go; a
/// heading is for somewhere further down the same page -- and a reader
/// looking for "the one about wrapping" should not have to guess which of
/// four tabs somebody filed it under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Every setting, under a heading per group.
    Settings,
    /// Every command, and the key it is on.
    Keys,
    /// The agents Obelus can install.
    Agents,
}

impl Page {
    /// Every page, in the order their tabs sit in.
    pub const ALL: [Self; 3] = [Self::Settings, Self::Keys, Self::Agents];

    /// The pages one file's settings have.
    ///
    /// The project's have one. Which keys a reader is on and which agent
    /// Obelus talks to are the reader's alone -- a downloaded project that
    /// could set either would be starting programs and moving `quit` under
    /// somebody's fingers -- so on the project's page those two tabs went
    /// nowhere. They said so, in a sentence, and the rows behind the
    /// sentence were still there: the focus walked a list nobody could see
    /// and enter on it wrote the reader's own file.
    ///
    /// Not drawn dim and not refusing when pressed: a tab is for somewhere
    /// else to go, and one that goes nowhere is a tab that lies. The foot
    /// of every view in Obelus follows the same rule, listing the keys
    /// that do something here and keeping the rest for `F1`.
    #[must_use]
    pub const fn of(whose: Whose) -> &'static [Self] {
        match whose {
            Whose::Reader => &Self::ALL,
            Whose::Project => &[Self::Settings],
        }
    }

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Keys => "Keys",
            Self::Agents => "Agents",
        }
    }
}

/// The active agent's settings, as this page needs them.
///
/// Handed in the way the agents are, and for the same reason: which agent
/// is active, what it last said it offers and what the reader has set are
/// three things the application knows, and a view that went looking for
/// them would be a second place they live.
///
/// Owned rather than borrowed because the application builds one out of
/// three places at once and hands it to a page it is also holding a
/// mutable borrow of. Small, and built where the cards are already built.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offering {
    /// What to call it, which is the heading its settings sit under.
    pub name: String,
    /// What it last said it can be set to.
    pub offers: Vec<Offer>,
    /// What the reader has said each is to start on, by the agent's id for
    /// the setting. What is not in here is what Obelus says nothing about.
    pub chosen: std::collections::BTreeMap<String, String>,
    /// Why there is nothing to list, where there is nothing.
    ///
    /// An agent Obelus has not talked to yet has told it nothing, which is
    /// not the same as an agent with nothing to be set -- and a group that
    /// simply was not drawn would say the second. So the group is drawn,
    /// with this in it instead of rows.
    pub silence: Option<String>,
}

/// One row of the settings page, and the heading it sits under if it opens
/// one.
///
/// The heading travels with the row rather than being a row of its own:
/// the focus walks rows, and a row it had to step over would make `down`
/// mean two different distances. It also makes a group with nothing left in
/// it disappear by itself -- a heading belongs to the first row of its
/// group that the query left, and where there is none there is no heading.
#[derive(Clone, Copy, Debug)]
pub enum Shown<'a> {
    /// One of Obelus's own settings.
    Obelus {
        /// The setting.
        setting: &'static Setting,
        /// The group it opens, where it is the first of one on show.
        opens: Option<obelus_config::Group>,
    },
    /// One thing the active agent offers, and what the reader has said a
    /// new conversation should start it on.
    Agent {
        /// What the agent offers, in the agent's own words.
        offer: &'a Offer,
        /// Which of its values the reader has chosen, if they have chosen
        /// one. `None` is the third state these rows have and Obelus's own
        /// do not: leave it to the agent.
        chosen: Option<&'a str>,
        /// The agent whose name this row opens the heading of, where it is
        /// the first of them.
        opens: Option<&'a str>,
    },
    /// The agent's group, with nothing in it but the reason why.
    ///
    /// A row so that the window, the heights and the headings are the ones
    /// every other row goes through. It has no control and nothing happens
    /// when it is pressed, which is what its being prose rather than a
    /// name and a control says on screen.
    Silent {
        /// What to say.
        saying: &'a str,
        /// The heading it opens, which it always does: it is the only row
        /// of its group.
        opens: &'a str,
    },
}

impl Shown<'_> {
    /// The setting this row is about, where it is one of Obelus's own.
    #[must_use]
    pub const fn setting(&self) -> Option<&'static Setting> {
        match self {
            Self::Obelus { setting, .. } => Some(*setting),
            Self::Agent { .. } | Self::Silent { .. } => None,
        }
    }

    /// The name on the row.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Obelus { setting, .. } => setting.name,
            Self::Agent { offer, .. } => &offer.name,
            Self::Silent { .. } => "",
        }
    }

    /// What it says under the name.
    #[must_use]
    pub fn about(&self) -> &str {
        match self {
            Self::Obelus { setting, .. } => setting.about,
            Self::Agent { offer, .. } => offer.about.as_deref().unwrap_or_default(),
            Self::Silent { saying, .. } => saying,
        }
    }
}

/// The settings view.
#[derive(Debug)]
pub struct Settings {
    /// Whether every key this page answers to is showing.
    keys_showing: bool,
    /// What has been typed, which narrows the rows.
    query: Field,
    /// Which command's key is being pressed, while one is.
    ///
    /// The page is in no mode otherwise: a key means what it means here
    /// until the reader says "this row's key is the next thing I press",
    /// and from then until they press it every key belongs to that row.
    binding: Option<Command>,
    /// The chord that would not do, and why it would not.
    ///
    /// On the row, because that is where the reader is looking and it is
    /// that binding the answer is about -- not on the status row, which is
    /// this page's filter, and not as a passing note, which the next
    /// keystroke would clear before it had been read.
    refused: Option<(KeyChord, Refused)>,
    /// Whose settings this page is: the reader's own, or the project's.
    ///
    /// The same page either way -- the same tabs, the same rows, the same
    /// keys -- because they are the same settings. What differs is which
    /// file a change is written to, and what a row says when the file this
    /// page is not about has the setting.
    whose: Whose,
    /// Which tab is showing, as an index into [`Page::of`] this page's.
    page: usize,
    /// Which row has the focus and which is on top -- of the settings, or
    /// of the cards, whichever page is showing.
    ///
    /// The same window every other list in Obelus has, which is what makes
    /// this page scroll the way they do: by the least that puts the focused
    /// row back on screen, and no further.
    window: Window,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// A view on the first group, with nothing typed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys_showing: false,
            query: Field::new(),
            binding: None,
            refused: None,
            whose: Whose::Reader,
            page: 0,
            window: Window::new(),
        }
    }

    /// The same page, over the project's own settings file.
    #[must_use]
    pub fn for_project() -> Self {
        Self {
            keys_showing: false,
            query: Field::new(),
            binding: None,
            refused: None,
            whose: Whose::Project,
            page: 0,
            window: Window::new(),
        }
    }

    /// Whose settings this page is.
    #[must_use]
    pub const fn whose(&self) -> Whose {
        self.whose
    }

    /// Whether this page is the project's.
    #[must_use]
    pub const fn on_project(&self) -> bool {
        matches!(self.whose, Whose::Project)
    }

    /// The tab names, in order: the ones this page's file has.
    #[must_use]
    pub fn tabs(&self) -> Vec<&'static str> {
        Page::of(self.whose)
            .iter()
            .map(|page| page.label())
            .collect()
    }

    /// Which page is showing.
    #[must_use]
    pub fn page(&self) -> Page {
        Page::of(self.whose)
            .get(self.page)
            .copied()
            .unwrap_or(Page::Settings)
    }

    /// Whether the list of every key is showing.
    #[must_use]
    pub const fn showing_keys(&self) -> bool {
        self.keys_showing
    }

    /// Whether the page showing is the keys rather than the settings.
    #[must_use]
    pub fn on_keys(&self) -> bool {
        self.page() == Page::Keys
    }

    /// Whether the page showing is the agents rather than the settings.
    #[must_use]
    pub fn on_agents(&self) -> bool {
        self.page() == Page::Agents
    }

    /// The commands on show, with the key each is on: this page's rows.
    ///
    /// Every command, whether or not it has a key -- a reader looking for
    /// something to bind is looking for the ones that have none, and a page
    /// that hid them could not be used for that.
    #[must_use]
    pub fn keys(&self, keymap: &Keymap) -> Vec<(Command, Option<KeyChord>)> {
        self.key_rows()
            .into_iter()
            .map(|command| (command, keymap.chord_for(command)))
            .collect()
    }

    /// The commands this page lists, without the keys they are on.
    ///
    /// What the filter leaves, which is what the focus moves over and how
    /// far the window can scroll. Split from [`Settings::keys`] because
    /// counting the rows does not need the table and the callers that
    /// count do not have it.
    #[must_use]
    pub fn key_rows(&self) -> Vec<Command> {
        if !self.on_keys() {
            return Vec::new();
        }
        let query = self.query.said().to_lowercase();
        obelus_command::ALL
            .iter()
            .filter(|spec| {
                query.is_empty()
                    || spec.name.to_lowercase().contains(&query)
                    || spec.title.to_lowercase().contains(&query)
            })
            .map(|spec| spec.command)
            .collect()
    }

    /// How many rows the page showing has.
    ///
    /// One answer for the settings and for the keys, because everything
    /// that moves the focus or scrolls the window needs it and two of them
    /// disagreed: the keys page counted the settings of a group that does
    /// not exist, which is none -- so its window had nothing in it and the
    /// page drew nothing at all.
    #[must_use]
    pub fn row_count(&self, offering: Option<&Offering>) -> usize {
        match self.on_keys() {
            true => self.key_rows().len(),
            false => self.rows(offering).len(),
        }
    }

    /// Which command's key is being pressed, if one is.
    #[must_use]
    pub const fn binding(&self) -> Option<Command> {
        self.binding
    }

    /// The key that would not do, and why.
    #[must_use]
    pub const fn refused(&self) -> Option<(KeyChord, Refused)> {
        self.refused
    }

    /// Which tab is showing.
    #[must_use]
    pub const fn tab(&self) -> usize {
        self.page
    }

    /// What has been typed.
    #[must_use]
    pub fn query(&self) -> String {
        self.query.said()
    }

    /// Where the caret is in it, for whoever draws the row.
    #[must_use]
    pub fn query_caret(&self) -> usize {
        self.query.caret().get()
    }

    /// Which characters of it the reader has hold of.
    #[must_use]
    pub fn query_held(&self) -> Option<std::ops::Range<usize>> {
        self.query.held()
    }

    /// Puts a run of text into it, which is what a paste is.
    pub fn put_in_query(&mut self, said: &str, offering: Option<&Offering>) {
        self.query.put(said);
        self.settle(offering);
    }

    /// Puts the filter's caret where a cell of its row is.
    pub fn place_in_query(&mut self, cell: u16, extend: bool) {
        self.query.place_at_cell(cell, extend);
    }

    /// Takes hold of the word under the caret, or of the whole filter.
    pub fn hold_in_query(&mut self, all: bool) {
        match all {
            true => self.query.hold_all(),
            false => self.query.hold_word(),
        }
    }

    /// What a copy takes from the query: what is held, or all of it.
    #[must_use]
    pub fn copy_query(&self) -> (String, &'static str) {
        self.query.copied()
    }

    /// The same, and takes it out.
    pub fn cut_query(&mut self, offering: Option<&Offering>) -> (String, &'static str) {
        let taken = self.query.cut();
        self.settle(offering);
        taken
    }

    /// Puts the focus on one of the rows.
    ///
    /// For a pointer: the keys move it a step at a time and have no use for
    /// naming a row outright, and a press names one.
    pub fn select_row(&mut self, row: usize, offering: Option<&Offering>) {
        self.window
            .set_focus(row.min(self.row_count(offering).saturating_sub(1)));
    }

    /// Which row has the focus.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.window.focus()
    }

    /// Which card the agents page draws first.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.window.top()
    }

    /// The window itself, for the view: what is on screen, and whether
    /// there is more of it than there is screen.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// The settings on show: all of them, narrowed by what has been typed,
    /// each with the heading it opens where it opens one.
    ///
    /// Grouped by walking the groups rather than by trusting the order the
    /// table happens to be written in: which group a setting is in is said
    /// on the setting, and a page that read it off the array's order would
    /// be a page one reordered line could quietly break.
    ///
    /// Narrowed plainly by substring rather than fuzzily: there are a dozen
    /// of these and a reader typing "the" means the word, where a fuzzy
    /// match would also offer everything with a t, an h and an e scattered
    /// through it.
    #[must_use]
    // The rows borrow the offering and not the page: what is on them comes
    // from the table Obelus ships with and from what the application
    // handed in, and a page that lent itself out here could not move its
    // own window while it held them.
    pub fn rows<'a>(&self, offering: Option<&'a Offering>) -> Vec<Shown<'a>> {
        if self.on_agents() || self.on_keys() {
            return Vec::new();
        }
        let query = self.query.said().to_lowercase();
        let mut rows = Vec::new();
        for group in Group::ALL {
            let mut opens = Some(group);
            for setting in obelus_config::ALL
                .iter()
                .filter(|setting| setting.group == group)
            {
                if !(query.is_empty()
                    || setting.name.to_lowercase().contains(&query)
                    || setting.key.contains(&query))
                {
                    continue;
                }
                rows.push(Shown::Obelus {
                    setting,
                    opens: opens.take(),
                });
            }
        }
        // Never on the project's page. What an agent starts on is the
        // reader's alone -- the same reason the agent itself is -- so a
        // group of them there would be rows that look like every other on
        // that page and write to the other file when pressed. The page
        // says as much about the agents tab, in words; here there is
        // simply nothing to say, because there is nothing a project could do
        // with it.
        //
        // And the agent's own, under a heading of its name. Last, because
        // Obelus's settings are the page a reader came to and an agent's
        // are about somewhere else; and a group rather than a tab of its
        // own, because it is the same shape of thing -- a name, a line
        // about it, and one control -- and a reader looking for the one
        // about thinking should not have to guess which page it is filed
        // under.
        let Some(offering) = offering.filter(|_| !self.on_project()) else {
            return rows;
        };
        let mut opens = Some(offering.name.as_str());
        for offer in &offering.offers {
            if !(query.is_empty() || offer.name.to_lowercase().contains(&query)) {
                continue;
            }
            rows.push(Shown::Agent {
                offer,
                chosen: offering.chosen.get(&offer.id).map(String::as_str),
                opens: opens.take(),
            });
        }
        // The reason there are none, where there are none and the reader
        // has not narrowed them away themselves: a group that was simply
        // not drawn would say the agent has nothing to be set, which is a
        // sentence about the agent and one Obelus has no grounds for.
        if let Some(saying) = offering.silence.as_deref()
            && query.is_empty()
            && let Some(opens) = opens
        {
            rows.push(Shown::Silent { saying, opens });
        }
        rows
    }

    /// The agents on show: all of them, narrowed by what has been typed.
    ///
    /// By name only. A description is a sentence, and matching sentences
    /// puts rows in the list for a word buried in prose -- which the
    /// highlight then cannot point at, because the name is what the card
    /// shows first.
    #[must_use]
    pub fn agents(&self, agents: &[Agent]) -> Vec<Agent> {
        if self.query.is_empty() {
            return agents.to_vec();
        }
        agents
            .iter()
            .filter(|listed| self.matched_in(&listed.agent.name).is_some())
            .cloned()
            .collect()
    }

    /// How wide a card's own words are, in the room the page has.
    ///
    /// One answer, because three asked it: the page that lays the cards
    /// out, the window that decides which of them are on screen, and the
    /// frame that works out where a mark will land. Three copies of a
    /// margin is three chances for a card to be measured at one width and
    /// drawn at another.
    #[must_use]
    pub const fn card_width(room: u16) -> u16 {
        // The indent either side, and the room the state in the corner
        // keeps for itself.
        room.saturating_sub(7)
    }

    /// How many rows a card takes.
    ///
    /// Its name, its description wrapped to the room there is, who wrote
    /// it, and why the last install failed if one did. Variable, because a
    /// description is a sentence and cutting it at the width loses the half
    /// that says what the thing is *for*.
    #[must_use]
    pub fn card_rows(&self, agent: &Agent, width: u16) -> u16 {
        let failed = u16::from(matches!(agent.status, Status::Failed(_)));
        2 + self.wrapped(&agent.agent.description, width).len().max(1) as u16 + failed
    }

    /// A sentence, broken into the rows it takes.
    ///
    /// Through the same word-breaking the editor uses -- a description is
    /// prose, and prose broken at the column is prose nobody reads twice --
    /// and capped, so that one verbose entry cannot push every other card
    /// off the screen.
    #[must_use]
    pub fn wrapped(&self, sentence: &str, width: u16) -> Vec<String> {
        if sentence.is_empty() {
            return Vec::new();
        }
        let mut rows = obelus_text::wrapped(sentence, width.max(8));
        rows.truncate(MOST_DESCRIPTION_ROWS);
        rows
    }

    /// How many cards fit in the room the page has.
    ///
    /// Counted from the focused card outwards, because that is the one that
    /// has to be whole -- and cards are not all the same height, so there
    /// is no number to divide by.
    #[must_use]
    pub fn cards_that_fit(&self, agents: &[Agent], room: (u16, u16)) -> usize {
        let width = Self::card_width(room.0);
        let height = room.1.saturating_sub(2);
        let listed = self.agents(agents);
        let mut taken = 0;
        let mut fits = 0;
        for agent in listed.iter().skip(self.window.focus().min(listed.len())) {
            let card = self.card_rows(agent, width) + 1;
            if taken + card > height && fits > 0 {
                break;
            }
            taken += card;
            fits += 1;
        }
        fits
    }

    /// Where what has been typed matched in a name, in characters.
    ///
    /// The same rule the settings rows follow -- a plain substring, so a
    /// match is one run -- and the run is what the view marks.
    #[must_use]
    pub fn matched_in(&self, text: &str) -> Option<std::ops::Range<usize>> {
        if self.query.is_empty() {
            return None;
        }
        let query: Vec<char> = self.query.said().to_lowercase().chars().collect();
        let text: Vec<char> = text.to_lowercase().chars().collect();
        if query.len() > text.len() {
            return None;
        }
        text.windows(query.len())
            .position(|window| window == query.as_slice())
            .map(|at| at..at + query.len())
    }

    /// Where what has been typed matched in a row's name, in characters.
    ///
    /// The filter is a plain substring, so a match is one run of them -- and
    /// the run is what the view colours. Without it a row in a narrowed list
    /// leaves the reader working out why it is there, which is the question
    /// the highlight answers everywhere else in Obelus.
    ///
    /// `None` for a row that matched on its key rather than on its name,
    /// and for no query at all: there is nothing on the row to point at.
    #[must_use]
    pub fn matched(&self, setting: &Setting) -> Option<std::ops::Range<usize>> {
        self.matched_in(setting.name)
    }

    /// What a row's control shows, from the config it is handed.
    #[must_use]
    pub fn value_of(setting: &Setting, config: &Config) -> Value {
        config.value_of(setting.key).unwrap_or(match setting.kind {
            Kind::Switch => Value::Switch(false),
            Kind::Choice(choices) => Value::Choice(
                choices
                    .first()
                    .map(|first| (*first).to_string())
                    .unwrap_or_default(),
            ),
            Kind::Count(counts) => Value::Count(
                counts
                    .first()
                    .and_then(|first| first.parse().ok())
                    .unwrap_or(1),
            ),
        })
    }

    /// Handles a key, given the settings as they stand and the agents on
    /// show.
    ///
    /// The agents are passed in rather than read here: what is installed
    /// and what is being installed are the application's to know, and a
    /// view that fetched them would be a second place they live.
    /// `room` is the region the page has, which is what a page of movement
    /// means: the rows a group of settings shows, or the cards that fit.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        config: &Config,
        keymap: &Keymap,
        agents: &[Agent],
        offering: Option<&Offering>,
        room: (u16, u16),
    ) -> SettingsOutcome {
        // A row waiting for a key takes the next one, whatever it is: that
        // is what the reader asked for by pressing enter on it, and a
        // modifier is half of most chords worth binding. Escape is the way
        // out, because escape is the way out of everything -- and it gives
        // up on the nearest thing first, which is this row rather than the
        // page.
        if let Some(command) = self.binding {
            if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                self.binding = None;
                self.refused = None;
                return SettingsOutcome::Consumed;
            }
            // Taking the key away is a decision a reader can make, and
            // there is nowhere else on the page to make it.
            if matches!(key.code, KeyCode::Delete | KeyCode::Backspace) && key.modifiers.is_empty()
            {
                self.binding = None;
                self.refused = None;
                return SettingsOutcome::Bind(command, None);
            }
            let Some(chord) = KeyChord::from_event(key) else {
                // A release, or a modifier Obelus cannot bind. Nothing to
                // say about it: the row is still waiting.
                return SettingsOutcome::Consumed;
            };
            // A key that could never fire is not offered, whatever the
            // reader pressed: the editor's own keys never reach the table,
            // the terminal sends another key for some chords, and some work
            // on this machine and not the next. The row says which of those
            // it is and goes on waiting.
            if let Some(why) = obelus_editing::keymap::why_not(chord) {
                self.refused = Some((chord, Refused::Never(why)));
                return SettingsOutcome::Consumed;
            }
            // And a key already spoken for stays where it is. One key, one
            // meaning, is a rule a reader can hold in their head -- and the
            // row says which command has it.
            match keymap.command_on(chord) {
                Some(taken) if taken != command => {
                    self.refused = Some((chord, Refused::Taken(taken)));
                    return SettingsOutcome::Consumed;
                }
                _ => {
                    self.binding = None;
                    self.refused = None;
                    return SettingsOutcome::Bind(command, Some(chord));
                }
            }
        }

        // The card's key before the modifiers are judged, because it has
        // one: control is what every key below this refuses.
        if obelus_editing::keymap::is_keys_card(key) {
            self.keys_showing = !self.keys_showing;
            return SettingsOutcome::Consumed;
        }

        // The same rule every other view follows: a modifier Obelus has no
        // meaning for disqualifies the key rather than being ignored.
        let Some(modifiers) = obelus_editing::keymap::modifiers_of(key) else {
            return SettingsOutcome::Ignored;
        };
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
            return SettingsOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let rows = self.rows(offering);
        let keys = self.keys(keymap);
        // How many things the focus can be on: the rows of the page, or the
        // agents that the query leaves.
        let count = match self.on_agents() {
            true => self.agents(agents).len(),
            false => self.row_count(offering),
        };

        // How far a page moves: the rows a group shows, or however many
        // cards fit -- which is not a fixed number, because a card is as
        // tall as its description needs.
        let page = if self.on_agents() {
            self.cards_that_fit(agents, room).max(1)
        } else {
            usize::from(room.1.saturating_sub(2)).max(1)
        };
        // How many rows there are, before any of them is moved between:
        // the count comes from the page rather than from the last frame,
        // because a key can arrive before the first one is drawn.
        self.window.set_count(count);

        match key.code {
            // The card first: a key that opens a thing closes that thing.
            KeyCode::Esc if bare && self.keys_showing => {
                self.keys_showing = false;
                SettingsOutcome::Consumed
            }
            KeyCode::Esc if bare => SettingsOutcome::Cancelled,
            // The ends, with and without control: the same keys reach the
            // ends of a document, a list and a rendering, and a key should
            // not mean one thing in one view and nothing in the next.
            // Every key that moves about a list, from the table every
            // list reads -- which is why they do the same here as they do
            // in a picker. The arrows need a bare key because the left and
            // right ones walk the tabs; paging and the ends have no other
            // meaning here.
            code if count > 0
                && let Some(movement) = Move::of(code)
                && (bare || !matches!(movement, Move::Up | Move::Down)) =>
            {
                self.window
                    .apply(movement, u16::try_from(page).unwrap_or(1), Wrap::Yes);
                SettingsOutcome::Consumed
            }
            // `tab` walks the tabs, as it does in every other view with
            // tabs on it -- which is why a switch is flipped with enter and
            // not by sliding it: one key with two jobs, decided by
            // whichever row happens to have the focus, is a key a reader
            // has to think about.
            //
            // The arrows used to do this. They are the caret's now: the
            // filter is a line with a caret in it, and a box a reader
            // cannot move about in is the thing this whole page filters
            // with.
            KeyCode::Tab if bare => {
                self.step_tab(true, offering);
                SettingsOutcome::Consumed
            }
            KeyCode::BackTab => {
                self.step_tab(false, offering);
                SettingsOutcome::Consumed
            }
            // Enter and space open a list or flip a switch: both are what a
            // reader reaches for, and neither has another job here.
            // On the agents page, enter is whatever the card offers: a
            // button while there is one, and the choice of which agent to
            // talk to once there is something to talk to.
            KeyCode::Enter if bare && self.on_agents() => {
                match self.agents(agents).get(self.window.focus()) {
                    Some(listed) => match &listed.status {
                        Status::Missing | Status::Failed(_) | Status::Outdated { .. } => {
                            SettingsOutcome::Install(listed.agent.id.clone())
                        }
                        Status::Installed if listed.active => SettingsOutcome::Deactivate,
                        Status::Installed => SettingsOutcome::Activate(listed.agent.id.clone()),
                        // Nothing to press while it is running, and nothing
                        // to press on one Obelus cannot install.
                        Status::Installing | Status::Unavailable(_) => SettingsOutcome::Consumed,
                    },
                    None => SettingsOutcome::Consumed,
                }
            }
            // On the keys page, enter is the reader saying "the next key I
            // press is this command's".
            KeyCode::Enter if bare && self.on_keys() => {
                self.binding = keys.get(self.window.focus()).map(|(command, _)| *command);
                self.refused = None;
                SettingsOutcome::Consumed
            }
            // The agent's rows are one thing before they are a switch or a
            // list: every one of them can also be left to the agent, which
            // is a third answer neither control has room for. So they all
            // open the same list, with that third answer at the top of it.
            KeyCode::Enter
                if bare && matches!(rows.get(self.window.focus()), Some(Shown::Agent { .. })) =>
            {
                match rows.get(self.window.focus()) {
                    Some(Shown::Agent { offer, .. }) => {
                        SettingsOutcome::ChooseForAgent(offer.id.clone())
                    }
                    _ => SettingsOutcome::Consumed,
                }
            }
            KeyCode::Enter if bare => {
                match rows.get(self.window.focus()).and_then(Shown::setting) {
                    Some(setting) => match setting.kind {
                        Kind::Switch => {
                            let on = matches!(Self::value_of(setting, config), Value::Switch(true));
                            SettingsOutcome::Changed(setting.key, Value::Switch(!on))
                        }
                        // Both open the same short list. A number is picked
                        // from one the way a word is, and the only difference
                        // is what it is written down as.
                        Kind::Choice(choices) | Kind::Count(choices) => {
                            let word = match Self::value_of(setting, config) {
                                Value::Choice(word) => word,
                                Value::Count(count) => count.to_string(),
                                Value::Switch(_) => String::new(),
                            };
                            SettingsOutcome::Choose(setting.key, choices, word)
                        }
                    },
                    None => SettingsOutcome::Consumed,
                }
            }
            // Take it out of the project's file, which is what `delete` means
            // on the keys page too: this one is not set here any more.
            // Only there -- the reader's own settings have no "unset", a
            // setting they have not changed is simply the default.
            // And on one of the agent's rows, wherever the page is: what
            // is left there is not a default of Obelus's but the agent's
            // own answer, so there is something to go back to.
            KeyCode::Delete
                if bare
                    && let Some(Shown::Agent { offer, chosen, .. }) =
                        rows.get(self.window.focus()) =>
            {
                match chosen {
                    Some(_) => SettingsOutcome::UnsetForAgent(offer.id.clone()),
                    // Already the agent's. Consumed rather than ignored:
                    // the key means this here, and letting it fall through
                    // to the filter would put a character in it.
                    None => SettingsOutcome::Consumed,
                }
            }
            KeyCode::Delete
                if bare && self.on_project() && !self.on_keys() && !self.on_agents() =>
            {
                match rows.get(self.window.focus()).and_then(Shown::setting) {
                    Some(setting) => SettingsOutcome::Unset(setting.key),
                    None => SettingsOutcome::Consumed,
                }
            }
            // Whatever the page did not want goes to the filter, which is
            // a line with a caret in it and takes the keys a line takes.
            _ => match self.query.handle_key(key) {
                true => {
                    self.settle(offering);
                    SettingsOutcome::Consumed
                }
                false => SettingsOutcome::Ignored,
            },
        }
    }

    /// Moves to the next tab, or the previous one, wrapping.
    fn step_tab(&mut self, forward: bool, offering: Option<&Offering>) {
        // Every tab, not every group: the agents are a tab and not a group,
        // and a walk that stopped at the groups could never reach them.
        // The project's page has one, so this walks it onto itself.
        let last = self.tabs().len() - 1;
        self.page = match (forward, self.page) {
            (true, at) if at >= last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        self.settle(offering);
    }

    /// Puts the focus back on a row that exists, after the rows change.
    ///
    /// And the window back to the top, because the rows a query leaves are
    /// not the rows the window was scrolled through.
    fn settle(&mut self, offering: Option<&Offering>) {
        self.window.set_count(self.row_count(offering));
        self.window.home();
    }

    /// Moves the window of cards if the focused one has left it, and no
    /// further.
    ///
    /// The same rule the lists follow -- the least that puts the focus back
    /// on screen -- counted in rows rather than in cards, because a card is
    /// as tall as its description needs. So a step towards either edge
    /// moves nothing until the card at that edge is the focused one.
    /// Called once a frame with the room the page has, rather than from
    /// every key that moves the focus: the window depends on the geometry,
    /// and the geometry is only settled at that point -- which is also what
    /// makes a resize move the window rather than leave the focus off the
    /// screen.
    pub fn settle_cards(&mut self, agents: &[Agent], room: (u16, u16)) {
        let listed = self.agents(agents);
        let width = Self::card_width(room.0);
        let heights: Vec<u16> = listed
            .iter()
            .map(|agent| self.card_rows(agent, width) + 1)
            .collect();
        self.window
            .settle_by_height(&heights, room.1.saturating_sub(2));
    }

    /// And the same for a page of settings, whose entries are as tall as
    /// what they have to say.
    pub fn settle_rows(&mut self, room: (u16, u16), offering: Option<&Offering>) {
        let heights: Vec<u16> = match self.on_keys() {
            // A key is a name and a chord: one row, the way it always was.
            true => vec![1; self.row_count(offering)],
            false => self
                .rows(offering)
                .iter()
                .map(|shown| self.setting_rows(shown, description_width(room.0)))
                .collect(),
        };
        self.window
            .settle_by_height(&heights, room.1.saturating_sub(2));
    }

    /// How many rows one setting takes: the heading it opens where it opens
    /// one, its name, what it does, and the blank that keeps it from running
    /// into the next one.
    ///
    /// Asked by the page that lays them out and by the window that decides
    /// which of them are on screen, so the two cannot disagree about where
    /// an entry ends -- the heading included, which is why it is counted
    /// here rather than drawn as an afterthought.
    #[must_use]
    pub fn setting_rows(&self, shown: &Shown, width: u16) -> u16 {
        let about = u16::try_from(self.wrapped(shown.about(), width).len()).unwrap_or(0);
        // A heading is its word and the blank under it: the word alone, with
        // the group's first setting hard against it, reads as a row of the
        // group rather than as its name. The agent's carries a line saying
        // when what is under it takes effect, which is the one thing about
        // this group that is not true of the others -- so it is three.
        let heading = match shown {
            Shown::Obelus { opens, .. } => u16::from(opens.is_some()) * 2,
            Shown::Agent { opens, .. } => u16::from(opens.is_some()) * HEADING_ROWS,
            Shown::Silent { .. } => HEADING_ROWS,
        };
        // A row with no name is its prose and the blank after it: there is
        // nothing to put a name on.
        let named = u16::from(!matches!(shown, Shown::Silent { .. }));
        heading + named + about + 1
    }
}
