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
//! A setting is a name and a gloss, not a sentence. `Theme`, `Wrapping`,
//! `Blame`, `Ignored files` -- a noun phrase naming the thing, not a clause
//! about it, and one word where one will do: what the name says the gloss
//! does not say again, and where the name is the whole of it there is no
//! gloss at all. A line reading "the colours Obelus draws in" beside
//! `Theme` is the footnote this is about. These were whole sentences ("Who last
//! changed the line the cursor is on") on the grounds that a name and a
//! description side by side read as a heading and a footnote. True when the
//! footnote says what the heading already had; what it produced was a page of
//! prose, where a reader looking for one row had to read every row to find it.
//! A column of names is *scanned*.
//!
//! **A number is turned in a box at the foot.** Enter on one brings up a box
//! where a compact list comes up, the arrows turn what is in it a step and
//! the digits type it, and enter keeps it -- nothing is in force before
//! that, as nothing is while a list is walked. It was a list, which offered
//! only the numbers Obelus thought of (`Dialling`).
//!
//! The keys page, whose rows are one row each, starts what a command does in
//! one column two past the longest name rather than two past its own: four
//! beginnings to find is four, and one is one.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_agent::{Listed as Agent, Status, acp::Setting as Offer};
use obelus_command::Command;
use obelus_config::{Config, Group, Kind, Setting, Value, Whose};
use obelus_keymap::{KeyChord, Keymap};
use obelus_remote::platform::{Description, Field as Told, Setup};
use obelus_themes::Card as ThemeCard;

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
    /// Only from the project's page, where `delete` means take this one
    /// out.
    Unset(&'static str),
    /// An agent should be installed.
    Install(String),
    /// A theme from the registry should be installed, or updated.
    InstallTheme(String),
    /// A theme taken from the registry should be taken away.
    RemoveTheme(String),
    /// A theme taken from the registry should be worn, and kept.
    WearTheme(String),
    /// An agent should be the one Obelus talks to.
    Activate(String),
    /// Whichever agent was active should stop being.
    Deactivate,
    /// A setting with choices wants its list: which setting, what the
    /// choices are, and which one it is set to now.
    ///
    /// Words only. A number is turned in a box of its own instead
    /// ([`Dialling`]).
    ///
    /// The list itself is the ordinary picker, opened by the application
    /// over this view. This one asks; it does not draw a list of its own.
    Choose(&'static str, &'static [&'static str], String),
    /// Open the list of names this setting holds, for the reader to add to,
    /// take from and put in order.
    ///
    /// Only the key: what is in the list is in the config, and what may go
    /// in it comes from the machine rather than from anything here.
    Names(&'static str),
    /// A setting that is typed wants its line: which setting, and what it
    /// says now, which is where the typing starts.
    Type(&'static str, String),
    /// A command should be on these keys from now on, in this order, and
    /// on no others -- on none, where the list is empty.
    ///
    /// Only ever keys nothing else is on: what is taken is said on the row
    /// and asked again, because the reader is looking at the row and it is
    /// that binding the answer is about.
    Bind(Command, Vec<KeyChord>),
    /// A command should be on the keys its layout gives it again: the
    /// reader's line for it taken out of the file.
    ResetKeys(Command),
    /// Every command should be on the keys its layout gives it -- asked
    /// first, because it takes every key the reader chose.
    ResetAllKeys,
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
    /// Something on the remote page wants doing.
    Remote(Reaching),
    /// One of the active agent's variables wants a new value, by its name.
    EditVariable(String),
    /// The reader is adding a variable to the active agent's environment.
    AddVariable,
    /// One of them should go, by its name.
    RemoveVariable(String),
    /// The reader is done with the view.
    Cancelled,
}

/// What a row of the remote page asks for.
///
/// Asked, not done: what each of these means -- a keyring to write, a code
/// to make, a list to open -- is the application's, and the page only knows
/// which row the reader is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaching {
    /// Choose which chat, or none.
    Platform,
    /// Say what one of the platform's fields is.
    Edit(&'static Told),
    /// Forget what one of them was.
    Forget(&'static Told),
    /// The people who may talk to this machine.
    People,
    /// Make a code to pair somebody with.
    Pair,
    /// What the platform's side is made from: a copy, or a page of steps.
    Setup,
}

/// One row of the remote page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteRow {
    /// Which chat, or none.
    Platform,
    /// One of the platform's fields.
    Field(&'static Told),
    /// Who may talk to this machine.
    People,
    /// Pairing somebody.
    Pair,
    /// How the platform's side is made.
    Setup(&'static Setup),
}

/// What the remote page shows, as the application knows it.
///
/// Handed in, the way the agent's offering is: whether a token is kept, who
/// is paired and whether a code is waiting are the application's to know,
/// and a page that went and found out would be a second place they live.
/// Kept on the page rather than passed with every key, because what it
/// changes is which rows there are and how tall each is -- the window asks
/// that between keys -- and the application hands a new one whenever any
/// of it moves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reached {
    /// Which chat is set, where one is.
    pub platform: Option<&'static Description>,
    /// Where this machine stands with it.
    pub state: obelus_remote::State,
    /// Why, in the platform's words, where it said something was wrong.
    pub why: Option<String>,
    /// What each field's control says, by the field's key: the start and
    /// end of a secret, the whole of anything else. A field that is not here
    /// has not been set.
    pub kept: std::collections::BTreeMap<&'static str, String>,
    /// Who may talk to this machine, by name.
    pub people: Vec<String>,
    /// The code waiting to be sent to the bot, while there is one.
    pub pairing: Option<String>,
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
/// Three *shapes* of page, and not three kinds of setting: a column of
/// settings with a control each, a table of commands and the key each is on,
/// and a shelf of cards from a registry that changes while it is being looked
/// at. The fourth tab is the first shape again, and a tab all the same because
/// it is somewhere else to go: which chat can reach this machine, and what is
/// kept for it, is about a place outside Obelus rather than about how Obelus
/// behaves -- and half of it is in the keyring rather than the file every
/// other row writes to.
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
    /// Which chat a note can be worked on from.
    Remote,
    /// The themes the registry lists, and the ones taken from it.
    Themes,
}

impl Page {
    /// Every page, in the order their tabs sit in.
    pub const ALL: [Self; 5] = [
        Self::Settings,
        Self::Keys,
        Self::Remote,
        Self::Themes,
        Self::Agents,
    ];

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
            Self::Remote => "Remote",
            Self::Themes => "Themes",
        }
    }
}

/// What the pages of cards are of, as the application hands them in.
///
/// Both together, so that every function a page of cards goes through is
/// handed the same two lists -- the one that moves the focus, the one that
/// measures the cards and the one that scrolls them -- and a page cannot be
/// measured from one list and walked through another.
#[derive(Clone, Copy, Debug)]
pub struct Cards<'a> {
    /// The agents Obelus could install.
    pub agents: &'a [Agent],
    /// The themes the registry lists, and the ones taken from it.
    pub themes: &'a [ThemeCard],
}

/// The active agent's settings, as this page needs them.
///
/// Handed in the way the agents are, and for the same reason: which agent
/// is active, what it said it offers and what the reader has set are three
/// things the application knows, and a view that went looking for them
/// would be a second place they live.
///
/// Owned rather than borrowed because the application builds one out of
/// three places at once and hands it to a page it is also holding a
/// mutable borrow of. Small, and built where the cards are already built.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offering {
    /// What to call it, which is the heading its settings sit under.
    pub name: String,
    /// What it said it can be set to, the last time this Obelus asked or
    /// a conversation of its own heard. A session's own list, with what
    /// that session happened to be on beside each -- which nothing here
    /// reads: a row says what the reader chose, not what some conversation
    /// was left on.
    pub offers: Vec<Offer>,
    /// What the reader has said each is to start on, by the agent's id for
    /// the setting. What is not in here is what Obelus says nothing about.
    pub chosen: std::collections::BTreeMap<String, String>,
    /// Why there is nothing to list, where there is nothing.
    ///
    /// An agent that has not answered yet has told Obelus nothing, which is
    /// not the same as an agent with nothing to be set -- and a group that
    /// simply was not drawn would say the second. So the group is drawn,
    /// with this in it instead of rows.
    pub silence: Option<String>,
    /// What the reader has added to what it is started with, by name.
    pub environment: Vec<(String, String)>,
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
        /// Which agent it is, by the name the heading uses.
        agent: &'a str,
    },
    /// One row of the remote page.
    Remote {
        /// Which.
        row: RemoteRow,
        /// The heading it opens -- the platform's name, or Obelus's word for
        /// none -- where it is the first row.
        opens: Option<&'static str>,
    },
    /// One of the variables the active agent is started with.
    ///
    /// A group of its own, under the agent's settings rather than among
    /// them: those are said to every conversation as it opens, and these
    /// reach the process once, when it starts -- two different moments,
    /// and a row that looked like its neighbours would be read as taking
    /// effect at theirs.
    Variable {
        /// What it is called.
        name: &'a str,
        /// What it holds.
        value: &'a str,
        /// The agent whose environment this row opens the heading of,
        /// where it is the first of them.
        opens: Option<&'a str>,
    },
    /// The row a variable is added from, last in the agent's environment.
    AddVariable {
        /// The agent whose environment it opens the heading of, where there
        /// is nothing above it in the group.
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
            Self::Agent { .. }
            | Self::Silent { .. }
            | Self::Remote { .. }
            | Self::Variable { .. }
            | Self::AddVariable { .. } => None,
        }
    }

    /// The name on the row.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Obelus { setting, .. } => setting.name,
            Self::Agent { offer, .. } => &offer.name,
            Self::Variable { name, .. } => name,
            Self::AddVariable { .. } => "Add a variable",
            Self::Silent { .. } => "",
            Self::Remote { row, .. } => match row {
                RemoteRow::Platform => "Platform",
                RemoteRow::Field(field) => field.name,
                RemoteRow::People => "People",
                RemoteRow::Pair => "Pair",
                RemoteRow::Setup(setup) => setup.name(),
            },
        }
    }

    /// What it says under the name.
    #[must_use]
    pub fn about(&self) -> &str {
        match self {
            Self::Obelus { setting, .. } => setting.about,
            Self::Agent { offer, .. } => offer.about.as_deref().unwrap_or_default(),
            // What it holds, under its name where a gloss goes, and whole:
            // the column the controls are in is the width of a word, and
            // a proxy's address or a token cut down to that is a value the
            // reader cannot check.
            Self::Variable { value, .. } => value,
            Self::AddVariable { .. } => "A name and then what it holds",
            Self::Silent { saying, .. } => saying,
            Self::Remote { row, .. } => match row {
                RemoteRow::Platform => "Which chat a note can be worked on from",
                RemoteRow::Field(field) => field.about,
                RemoteRow::People => "Who may talk to this machine through it",
                RemoteRow::Pair => "A code to send in the group you put the bot in",
                RemoteRow::Setup(setup) => setup.about(),
            },
        }
    }

    /// What is wrong with what this row is set to, where something is.
    ///
    /// Said under what the row does, in the colour of something that will
    /// not work, by the one piece of code that draws every row -- so any
    /// row that has something wrong with it says so here, and the page and
    /// the window that decides what is on it count the same rows for it.
    ///
    /// In words and not only in colour: a value drawn red says that
    /// something is wrong and not what, and a reader who cannot tell the
    /// colours apart is told nothing at all.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match self {
            Self::Obelus { .. }
            | Self::Silent { .. }
            | Self::Remote { .. }
            | Self::Variable { .. }
            | Self::AddVariable { .. } => None,
            // A choice the agent has stopped offering is a line in the
            // settings file that does nothing: a new conversation is left
            // on whatever the agent opens on.
            //
            // Not opening on the agent's name, which is a name and keeps
            // its own spelling -- and until the registry arrives it is the
            // id, which is lowercase -- while copy opens on a capital.
            Self::Agent {
                offer,
                chosen: Some(value),
                agent,
                ..
            } if offer.name_of(value).is_none() => Some(format!(
                "No longer offered by {agent}, so a new conversation starts on its own"
            )),
            Self::Agent { .. } => None,
        }
    }
}

/// Whether a setting is one of those left by what has been typed, already
/// lowercased: by its name, or by what it is called in the file.
fn narrowed_to(setting: &Setting, query: &str) -> bool {
    query.is_empty() || setting.name.to_lowercase().contains(query) || setting.key.contains(query)
}

/// A row of the keys page waiting for its key, and what that key will do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The command.
    pub command: Command,
    /// The row it is waiting on, which is where it says so.
    pub slot: usize,
    /// Whether the key pressed takes the place of the one on that row, or
    /// is one more key for the command.
    pub replacing: bool,
}

/// A number being turned, in the box that comes up at the foot of the page.
///
/// The page's second mode, beside a key being bound, and for the same
/// reason: enter on the row said "the next keys are this number's", and
/// until enter or escape every key is. The arrows turn it and the digits
/// type it, so a reader who wants ninety days types `90` rather than
/// pressing right sixty times.
///
/// Nothing is in force until enter. The box is a question the reader is
/// answering, the way a compact list is, and a list does not change the
/// setting while the reader walks it either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialling {
    /// Which setting.
    pub key: &'static str,
    /// How it turns.
    pub dial: obelus_config::Dial,
    /// What it was when enter was pressed, and is until enter again.
    pub was: usize,
    /// What is in the box.
    pub typed: String,
    /// Whether what is in the box is still what it opened with, held: the
    /// first digit takes its place rather than going on the end of it,
    /// because a reader who types `18` over `14` means eighteen and not
    /// fourteen hundred and eighteen.
    pub held: bool,
}

impl Dialling {
    /// What enter keeps, and where the dial is drawn at: what is in the
    /// box brought inside the ends, or what it was where the box is empty.
    #[must_use]
    pub fn kept(&self) -> usize {
        self.typed
            .parse()
            .map_or(self.was, |typed| self.dial.held(typed))
    }
}

/// One row of the keys page.
#[derive(Clone, Copy, Debug)]
pub enum KeyRow {
    /// A setting about the whole table, above the commands it changes:
    /// which layout the reader's keys start from.
    Setting(&'static Setting),
    /// One of a command's keys, by where it is in the command's list: a
    /// row each, so every key a command is on can be stood on, replaced
    /// and taken away. The first is the one that carries the command's
    /// name, and a command on no key has that one row and no key in it.
    Key {
        /// The command.
        command: Command,
        /// Which of its keys, from nought.
        slot: usize,
    },
}

impl KeyRow {
    /// The name on the row.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Setting(setting) => setting.name,
            Self::Key { command, .. } => command.name(),
        }
    }
}

/// The settings view.
#[derive(Debug)]
pub struct Settings {
    /// What has been typed, which narrows the rows.
    query: Field,
    /// Which command's key is being pressed, while one is, and on which of
    /// its rows.
    ///
    /// The page is in no mode otherwise: a key means what it means here
    /// until the reader says "the next thing I press is this command's",
    /// and from then until they press it every key belongs to that row.
    binding: Option<Binding>,
    /// Which number is being turned, while one is -- see [`Dialling`].
    dialling: Option<Dialling>,
    /// The keys each command is on, as the table was when it was last
    /// heard ([`Settings::hear_keymap`]).
    ///
    /// Kept rather than asked for, because how many rows the keys page has
    /// is asked by everything that moves its focus or scrolls it, and
    /// almost none of those have the table to hand. The application says
    /// when the table changes, which is the one moment the answer can.
    held: Vec<(Command, Vec<KeyChord>)>,
    /// The commands whose keys are not the ones their layout gives them,
    /// heard with the keys: what the page marks, and what the keys that
    /// put a command back are offered on.
    changed: Vec<Command>,
    /// The commands the reader's own file has a line for: the ones a
    /// command can be put back from. A command can be changed without
    /// one -- another's line took its key -- and putting it back would
    /// take nothing out.
    lined: Vec<Command>,
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
    /// What the remote page shows -- see [`Reached`].
    reached: Reached,
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
            query: Field::new(),
            binding: None,
            dialling: None,
            held: Vec::new(),
            changed: Vec::new(),
            lined: Vec::new(),
            refused: None,
            whose: Whose::Reader,
            page: 0,
            window: Window::new(),
            reached: Reached::default(),
        }
    }

    /// The same page, over the project's own settings file.
    #[must_use]
    pub fn for_project() -> Self {
        Self {
            query: Field::new(),
            binding: None,
            dialling: None,
            held: Vec::new(),
            changed: Vec::new(),
            lined: Vec::new(),
            refused: None,
            whose: Whose::Project,
            page: 0,
            window: Window::new(),
            reached: Reached::default(),
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

    /// What typing into this page narrows, for the row before anything
    /// has been typed.
    ///
    /// Per tab, because the page is three lists and the filter is about
    /// whichever is showing. Said on the row the reader would type into,
    /// and only there: typing is not a key, so the foot has no cap for it.
    ///
    /// `None` once something has been typed, because by then the reader
    /// knows -- and the row has their words in it, which is what this
    /// stands in for.
    #[must_use]
    pub fn what_is_filtered(&self) -> Option<&'static str> {
        if !self.query().is_empty() {
            return None;
        }
        Some(match self.page() {
            Page::Keys => "Filter keys",
            Page::Agents => "Filter agents",
            Page::Remote => "Filter remote settings",
            Page::Settings => "Filter settings",
            Page::Themes => "Filter themes",
        })
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

    /// Whether the page showing is the themes.
    #[must_use]
    pub fn on_themes(&self) -> bool {
        self.page() == Page::Themes
    }

    /// Whether the page showing is one of cards rather than of rows.
    #[must_use]
    pub fn on_cards(&self) -> bool {
        self.on_agents() || self.on_themes()
    }

    /// Whether the page showing is the remote one.
    #[must_use]
    pub fn on_remote(&self) -> bool {
        self.page() == Page::Remote
    }

    /// What the remote page shows.
    #[must_use]
    pub const fn reached(&self) -> &Reached {
        &self.reached
    }

    /// Hands it a new account of the remote page, keeping the focus on a
    /// row that is still there.
    pub fn reach(&mut self, reached: Reached) {
        if reached == self.reached {
            return;
        }
        self.reached = reached;
        if self.on_remote() {
            self.window.set_count(self.row_count(None));
        }
    }

    /// What is wrong with a row, where something is: the row's own answer,
    /// or for one of the remote page's fields what the application said
    /// about it.
    ///
    /// Asked here by the page and by the window both, for the reason
    /// [`Shown::warning`] is one function: a row counted at one height and
    /// drawn at another is a row the window walks past.
    #[must_use]
    pub fn warning_of(&self, shown: &Shown) -> Option<String> {
        match shown {
            // What is wrong with the chat is said on the row whose value
            // it is wrong about: the page holds what the reader set, and
            // whether it is connected is the status row's. A secret
            // refused, or one the keyring will not give up, is a value the
            // reader can do something about here.
            //
            // What no row's value is about -- an app Feishu will not give a
            // long connection, a platform that will not answer -- goes on
            // the chat's own row, in the platform's words. The status row
            // says it once and moves on, and a reader who comes here to find
            // out why was otherwise told nothing.
            Shown::Remote {
                row: RemoteRow::Platform,
                ..
            } => match self.reached.state {
                obelus_remote::State::Declined | obelus_remote::State::Unreachable => {
                    self.reached.why.clone()
                }
                _ => None,
            },
            Shown::Remote {
                row: RemoteRow::Field(field),
                ..
            } => {
                let secret = matches!(
                    field.kind,
                    obelus_remote::platform::FieldKind::Secret { .. }
                );
                let name = self
                    .reached
                    .platform
                    .map_or("The chat", |platform| platform.name);
                match self.reached.state {
                    obelus_remote::State::Refused if secret => Some(format!("Refused by {name}")),
                    obelus_remote::State::Locked if secret => {
                        Some("The keyring is locked".to_string())
                    }
                    obelus_remote::State::NoKeyring if secret => {
                        Some("No keyring on this machine".to_string())
                    }
                    obelus_remote::State::Unread if secret => {
                        Some("The keyring would not answer".to_string())
                    }
                    _ => None,
                }
            }
            shown => shown.warning(),
        }
    }

    /// The rows of the remote page: which chat, and -- where one is set --
    /// what it has to be told, who may talk, pairing, and how its side is
    /// made, in the order a reader setting it up for the first time goes
    /// down them.
    fn remote_rows(&self) -> Vec<Shown<'static>> {
        let opens = Some(
            self.reached
                .platform
                .map_or("Remote", |platform| platform.name),
        );
        let mut rows = vec![RemoteRow::Platform];
        if let Some(platform) = self.reached.platform {
            rows.extend(platform.fields.iter().map(RemoteRow::Field));
            rows.extend([
                RemoteRow::People,
                RemoteRow::Pair,
                RemoteRow::Setup(&platform.setup),
            ]);
        }
        // By name, the way the settings are narrowed, and each row opens the
        // heading only if it is the first left.
        let query = self.query.said().to_lowercase();
        let mut opens = opens;
        rows.into_iter()
            .filter(|row| {
                query.is_empty()
                    || Shown::Remote {
                        row: *row,
                        opens: None,
                    }
                    .label()
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|row| Shown::Remote {
                row,
                opens: opens.take(),
            })
            .collect()
    }

    /// The commands on show, with the key each is on: this page's rows.
    ///
    /// Every command, whether or not it has a key -- a reader looking for
    /// something to bind is looking for the ones that have none, and a page
    /// that hid them could not be used for that.
    #[must_use]
    pub fn keys(&self) -> Vec<(KeyRow, Option<KeyChord>)> {
        self.key_rows()
            .into_iter()
            .map(|row| match row {
                KeyRow::Setting(_) => (row, None),
                KeyRow::Key { command, slot } => (row, self.held(command).get(slot).copied()),
            })
            .collect()
    }

    /// Hears the table of keys as it is now, which is what the keys page
    /// lists: on opening, and whenever the application changes it.
    ///
    /// The focus stays on the command it was on, on the same row of it or
    /// its last: a key taken away is a row fewer, and an index kept as it
    /// was would land on the next command.
    ///
    /// And the table the layout alone would make, which is what a command
    /// is changed from: asked of the keys, not of the file, so a line that
    /// says what the layout says anyway marks nothing.
    ///
    /// And which commands the reader's file has a line for, which is what
    /// putting one back takes out.
    pub fn hear_keymap(&mut self, keymap: &Keymap, laid_out: &Keymap, lined: Vec<Command>) {
        self.lined = lined;
        let focused = match self.on_keys() {
            true => self.key_rows().get(self.window.focus()).copied(),
            false => None,
        };
        self.held = obelus_command::ALL
            .iter()
            .map(|spec| (spec.command, keymap.chords_for(spec.command)))
            .collect();
        self.changed = self
            .held
            .iter()
            .filter(|(command, chords)| *chords != laid_out.chords_for(*command))
            .map(|(command, _)| *command)
            .collect();
        if !self.on_keys() {
            return;
        }
        self.window.set_count(self.row_count(None));
        if let Some(KeyRow::Key { command, slot }) = focused {
            let slot = slot.min(self.held(command).len().saturating_sub(1));
            let landed = self.key_rows().iter().position(|row| {
                matches!(row, KeyRow::Key { command: on, slot: at } if *on == command && *at == slot)
            });
            if let Some(at) = landed {
                self.window.set_focus(at);
            }
        }
    }

    /// Whether a command's keys are not the ones its layout gives it.
    #[must_use]
    pub fn changed(&self, command: Command) -> bool {
        self.changed.contains(&command)
    }

    /// Whether a command can be put back on its layout's keys: changed,
    /// and by a line of the reader's own for it.
    #[must_use]
    pub fn resettable(&self, command: Command) -> bool {
        self.changed(command) && self.lined.contains(&command)
    }

    /// Whether any command's are.
    #[must_use]
    pub fn any_changed(&self) -> bool {
        !self.changed.is_empty()
    }

    /// The keys a command is on, as last heard.
    fn held(&self, command: Command) -> &[KeyChord] {
        self.held
            .iter()
            .find(|(held, _)| *held == command)
            .map_or(&[], |(_, chords)| chords.as_slice())
    }

    /// The commands this page lists, without the keys they are on.
    ///
    /// What the filter leaves, which is what the focus moves over and how
    /// far the window can scroll: a row for each of a command's keys, as
    /// the page last heard them ([`Settings::hear_keymap`]). Split from
    /// [`Settings::keys`] because counting the rows does not need the keys
    /// themselves, only how many each command has.
    #[must_use]
    pub fn key_rows(&self) -> Vec<KeyRow> {
        if !self.on_keys() {
            return Vec::new();
        }
        let query = self.query.said().to_lowercase();
        // The layout first: it decides where every key below it starts.
        let settings = obelus_config::ALL
            .iter()
            .filter(|setting| setting.group == Group::Keys && setting.shown())
            .filter(|setting| narrowed_to(setting, &query))
            .map(KeyRow::Setting);
        // What this front end can never do has no key worth giving it here,
        // for the reason the palette leaves it out.
        let commands = obelus_command::ALL
            .iter()
            .filter(|spec| spec.command.shown())
            .filter(|spec| {
                query.is_empty()
                    || spec.name.to_lowercase().contains(&query)
                    || spec.title.to_lowercase().contains(&query)
            })
            .flat_map(|spec| {
                let rows = self.held(spec.command).len().max(1);
                (0..rows).map(|slot| KeyRow::Key {
                    command: spec.command,
                    slot,
                })
            });
        settings.chain(commands).collect()
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

    /// Scrolls the page by a wheel's rows of the screen, leaving the focus
    /// where it is.
    ///
    /// Not while a row waits for its key: every key is that row's then, and
    /// a notch that carried it off the screen would leave the reader
    /// binding a row they could no longer see.
    pub fn scroll(
        &mut self,
        rows: isize,
        cards: Cards<'_>,
        offering: Option<&Offering>,
        room: (u16, u16),
    ) {
        if self.binding.is_some() || self.dialling.is_some() {
            return;
        }
        let heights = match self.on_cards() {
            true => self.card_heights(cards, room),
            false => self.entry_heights(room, offering),
        };
        self.window.scroll_rows(rows, &heights);
    }

    /// Which command's key is being pressed, if one is.
    #[must_use]
    pub fn binding(&self) -> Option<Command> {
        self.binding.map(|binding| binding.command)
    }

    /// And on which of its rows, and whether the key pressed will take the
    /// place of the one there or be one more.
    #[must_use]
    pub const fn binding_at(&self) -> Option<Binding> {
        self.binding
    }

    /// The number being turned, if one is.
    #[must_use]
    pub const fn dialling(&self) -> Option<&Dialling> {
        self.dialling.as_ref()
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

    /// Which characters of it an input method is still spelling.
    #[must_use]
    pub fn query_spelled(&self) -> Option<std::ops::Range<usize>> {
        self.query.spelled()
    }

    /// Whether the key the foot calls delete is the filter's just now
    /// rather than the row's (`Field::delete_is_the_rows`): on a Mac that
    /// is backspace, which the filter keeps while anything is in it.
    #[must_use]
    pub fn delete_is_the_filters(&self) -> bool {
        !self.query.delete_is_the_rows()
    }

    /// What an input method's spelling is written into -- see
    /// [`crate::spelling`].
    pub fn spells(&mut self) -> &mut dyn crate::spelling::Spells {
        &mut self.query
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

    /// The window itself, for the view: what is on screen. Not whether there
    /// is more of it than there is screen -- it counts entries, and an entry
    /// on the settings page is several rows.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Puts this row at the top, for a bar the reader has hold of.
    pub fn drag_to(&mut self, top: usize) {
        self.window.drag_to(top);
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
        if self.on_cards() || self.on_keys() {
            return Vec::new();
        }
        if self.on_remote() {
            return self.remote_rows();
        }
        let query = self.query.said().to_lowercase();
        let mut rows = Vec::new();
        for group in Group::ALL {
            let mut opens = Some(group);
            for setting in obelus_config::ALL
                .iter()
                .filter(|setting| setting.group == group && setting.shown())
            {
                if !narrowed_to(setting, &query) {
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
                agent: &offering.name,
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
        // And what it is started with, as a group of its own after them --
        // see `Shown::Variable`. The row that adds one is there whatever
        // the agent has said, because this is the reader's and not the
        // agent's: an agent that never answered can still be given a proxy
        // to answer through.
        let mut opens = Some(offering.name.as_str());
        for (name, value) in &offering.environment {
            if !(query.is_empty() || name.to_lowercase().contains(&query)) {
                continue;
            }
            rows.push(Shown::Variable {
                name,
                value,
                opens: opens.take(),
            });
        }
        if query.is_empty() || "add a variable".contains(&query) {
            rows.push(Shown::AddVariable {
                opens: opens.take(),
            });
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
    pub fn cards_that_fit(&self, cards: Cards<'_>, room: (u16, u16)) -> usize {
        let heights = self.card_heights(cards, room);
        let mut taken = 0;
        let mut fits = 0;
        for card in heights.iter().skip(self.window.focus().min(heights.len())) {
            if taken + card > room.1 && fits > 0 {
                break;
            }
            taken += card;
            fits += 1;
        }
        fits
    }

    /// The themes on show: all of them, narrowed by what has been typed --
    /// by name, the way the agents are.
    #[must_use]
    pub fn themes(&self, themes: &[ThemeCard]) -> Vec<ThemeCard> {
        if self.query.is_empty() {
            return themes.to_vec();
        }
        themes
            .iter()
            .filter(|card| self.matched_in(&card.theme.name).is_some())
            .cloned()
            .collect()
    }

    /// Whether the theme at `at` among those on show is the first of its
    /// group, and so opens the group with a heading.
    #[must_use]
    pub fn opens_a_group(themes: &[ThemeCard], at: usize) -> bool {
        match at {
            0 => !themes.is_empty(),
            _ => themes
                .get(at)
                .zip(themes.get(at - 1))
                .is_some_and(|(this, before)| this.theme.group != before.theme.group),
        }
    }

    /// How many rows a theme's card takes, the heading of the group it
    /// opens included.
    ///
    /// Its name, its description wrapped, the line of facts under it, and
    /// why the last install failed if one did -- the agents' card, less
    /// nothing -- and two rows above it where it opens a group: the heading
    /// and the blank under it.
    #[must_use]
    pub fn theme_rows(&self, themes: &[ThemeCard], at: usize, width: u16) -> u16 {
        let Some(card) = themes.get(at) else {
            return 0;
        };
        let failed = u16::from(matches!(card.status, obelus_themes::Status::Failed(_)));
        let heading = if Self::opens_a_group(themes, at) {
            2
        } else {
            0
        };
        let described = self.wrapped(&card.theme.description, width).len().max(1);
        heading + 2 + u16::try_from(described).unwrap_or(u16::MAX) + failed
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
            Kind::Names => Value::Names(Vec::new()),
            Kind::Text => Value::Text(String::new()),
            Kind::Count(dial) => Value::Count(dial.least),
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
        cards: Cards<'_>,
        offering: Option<&Offering>,
        room: (u16, u16),
    ) -> SettingsOutcome {
        let agents = cards.agents;
        if self.dialling.is_some() {
            return self.turn(key);
        }
        // A row waiting for a key takes the next one, whatever it is: that
        // is what the reader asked for by pressing enter on it, and a
        // modifier is half of most chords worth binding. Escape is the way
        // out, because escape is the way out of everything -- and it gives
        // up on the nearest thing first, which is this row rather than the
        // page.
        if let Some(Binding {
            command,
            slot,
            replacing,
        }) = self.binding
        {
            if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                self.binding = None;
                self.refused = None;
                return SettingsOutcome::Consumed;
            }
            // Taking the key on this row away, on the key that does it
            // everywhere on this page. Only where the row is being changed:
            // one waiting for a key to add has nothing of its own to take,
            // and goes on waiting.
            let held = self.held(command).to_vec();
            if KeyChord::from_event(key)
                == Some(KeyChord::new(KeyCode::Backspace, KeyModifiers::ALT))
            {
                if !replacing || slot >= held.len() {
                    return SettingsOutcome::Consumed;
                }
                let mut kept = held;
                kept.remove(slot);
                self.binding = None;
                self.refused = None;
                return SettingsOutcome::Bind(command, kept);
            }
            let Some(chord) = KeyChord::from_event(key) else {
                // A release, or a modifier Obelus cannot bind. Nothing to
                // say about it: the row is still waiting.
                return SettingsOutcome::Consumed;
            };
            // A key that could never fire is not offered, whatever the
            // reader pressed: the editor's own keys never reach the table,
            // and every list and box takes some for itself. The row says
            // which of those it is and goes on waiting.
            if let Some(why) = obelus_keymap::why_not(chord) {
                self.refused = Some((chord, Refused::Never(why)));
                return SettingsOutcome::Consumed;
            }
            // And a key already spoken for stays where it is. One key, one
            // meaning, is a rule a reader can hold in their head -- and the
            // row says which command has it.
            let taken = self
                .held
                .iter()
                .find(|(_, chords)| chords.contains(&chord))
                .map(|(taken, _)| *taken);
            match taken {
                Some(taken) if taken != command => {
                    self.refused = Some((chord, Refused::Taken(taken)));
                    return SettingsOutcome::Consumed;
                }
                // A key the command is already on changes nothing.
                Some(_) => {
                    self.binding = None;
                    self.refused = None;
                    return SettingsOutcome::Consumed;
                }
                None => {
                    let mut chords = held;
                    match chords.get_mut(slot).filter(|_| replacing) {
                        Some(on_the_row) => *on_the_row = chord,
                        None => chords.push(chord),
                    }
                    self.binding = None;
                    self.refused = None;
                    return SettingsOutcome::Bind(command, chords);
                }
            }
        }

        // One more key for the command the reader is on, whichever of its
        // rows: `ctrl+enter`, and only that, so enter always changes a key
        // and control always adds one. A terminal without the keyboard
        // protocol sends it as enter, and there a key is added in the
        // file -- a command's first as well, enter having nothing to change
        // on a row with no key.
        let adding = key.code == KeyCode::Enter && key.modifiers == KeyModifiers::CONTROL;
        if adding && self.on_keys() {
            if let Some(KeyRow::Key { command, slot }) = self.key_rows().get(self.window.focus()) {
                self.binding = Some(Binding {
                    command: *command,
                    slot: *slot,
                    replacing: false,
                });
                self.refused = None;
            }
            return SettingsOutcome::Consumed;
        }

        // Taking keys away, on alt because the filter takes nothing on it:
        // `alt+backspace` the key on this row, `alt+r` this command put back
        // on its layout's keys, and `alt+shift+r` every command, asked
        // first. Backspace and delete are the filter's, and a Mac has no
        // delete to press.
        if self.on_keys() {
            let chord = KeyChord::from_event(key);
            let focused = self.keys().get(self.window.focus()).copied();
            let removing = chord == Some(KeyChord::new(KeyCode::Backspace, KeyModifiers::ALT));
            let resetting = chord == Some(KeyChord::new(KeyCode::Char('r'), KeyModifiers::ALT));
            let resetting_all = chord
                == Some(KeyChord::new(
                    KeyCode::Char('r'),
                    KeyModifiers::ALT | KeyModifiers::SHIFT,
                ));
            // The two that act on the row with the focus wait, as enter
            // does, for the row to be on the screen: the first press brings
            // back a row the wheel took away, and takes nothing from it.
            if removing || resetting {
                let count = self.key_rows().len();
                self.window.set_count(count);
                if count > 0 && self.window.reach_for_the_focus() {
                    return SettingsOutcome::Consumed;
                }
            }
            match focused {
                Some((KeyRow::Key { command, slot }, Some(_))) if removing => {
                    let mut kept = self.held(command).to_vec();
                    kept.remove(slot);
                    return SettingsOutcome::Bind(command, kept);
                }
                Some((KeyRow::Key { command, .. }, _)) if resetting && self.resettable(command) => {
                    return SettingsOutcome::ResetKeys(command);
                }
                _ if resetting_all && self.any_changed() => {
                    return SettingsOutcome::ResetAllKeys;
                }
                _ if removing || resetting || resetting_all => return SettingsOutcome::Consumed,
                _ => {}
            }
        }

        // The same rule every other view follows: a modifier Obelus has no
        // meaning for disqualifies the key rather than being ignored.
        let Some(modifiers) = obelus_keymap::modifiers_of(key) else {
            return SettingsOutcome::Ignored;
        };
        // Except holding a word, or all the way to an end, which is the
        // filter's and nothing else's here: control and shift together.
        if modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
            )
        {
            self.query.handle_key(key);
            return SettingsOutcome::Consumed;
        }
        // And the page's two ends, which are control's here, because bare
        // they are the filter's caret's.
        let ends = Move::under_a_box(key).filter(|_| modifiers == KeyModifiers::CONTROL);
        if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT && ends.is_none() {
            return SettingsOutcome::Ignored;
        }
        let bare = modifiers == KeyModifiers::NONE;
        let rows = self.rows(offering);
        let keys = self.keys();
        // How many things the focus can be on: the rows of the page, or the
        // agents that the query leaves.
        let themes = self.themes(cards.themes);
        let count = if self.on_agents() {
            self.agents(agents).len()
        } else if self.on_themes() {
            themes.len()
        } else {
            self.row_count(offering)
        };

        // How far a page moves: the rows a group shows, or however many
        // cards fit -- which is not a fixed number, because a card is as
        // tall as its description needs.
        let page = if self.on_cards() {
            self.cards_that_fit(cards, room).max(1)
        } else {
            usize::from(room.1).max(1)
        };
        // How many rows there are, before any of them is moved between:
        // the count comes from the page rather than from the last frame,
        // because a key can arrive before the first one is drawn.
        self.window.set_count(count);

        // Enter and delete act on the row with the focus, which the wheel
        // may have left off the screen: there the press brings it back
        // into view, and does nothing to it. Delete as the filter judges it,
        // which on a Mac is backspace too (`Field::leaves_to_the_row`).
        let acts = (bare && key.code == KeyCode::Enter) || self.query.leaves_to_the_row(key);
        if acts && count > 0 && self.window.reach_for_the_focus() {
            return SettingsOutcome::Consumed;
        }

        match key.code {
            // What is held in the filter first, which is nearer than the
            // page it is on.
            KeyCode::Esc if bare && self.query.let_go() => SettingsOutcome::Consumed,
            KeyCode::Esc if bare => SettingsOutcome::Cancelled,
            // The ends, with and without control: the same keys reach the
            // ends of a document, a list and a rendering, and a key should
            // not mean one thing in one view and nothing in the next.
            // Every key that moves about a list, from the table every
            // list reads -- which is why they do the same here as they do
            // in a picker. The arrows need a bare key because the left and
            // right ones walk the tabs; paging and the ends have no other
            // meaning here.
            //
            // As a list under a box has them: with shift they are the
            // filter's, holding what the caret passes over, and bare home
            // and end are where its caret goes -- so the page's ends are
            // control's.
            _ if count > 0
                && let Some(movement) = Move::under_a_box(key) =>
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
                        Status::Installing { .. } | Status::Unavailable(_) => {
                            SettingsOutcome::Consumed
                        }
                    },
                    None => SettingsOutcome::Consumed,
                }
            }
            // On the themes page, enter is the card's button: install it,
            // or update it, or wear it once it is installed and current or
            // no longer listed -- and nothing on one on its way.
            KeyCode::Enter if bare && self.on_themes() => match themes.get(self.window.focus()) {
                Some(card) if card.may_be_installed() => {
                    SettingsOutcome::InstallTheme(card.theme.name.clone())
                }
                Some(card) if card.may_be_worn() => {
                    SettingsOutcome::WearTheme(card.theme.name.clone())
                }
                _ => SettingsOutcome::Consumed,
            },
            // And delete takes one away, where one may be: the same answer
            // the foot asks before it offers the key, so a key it does not
            // show does nothing.
            KeyCode::Delete | KeyCode::Backspace
                if self.query.leaves_to_the_row(key) && self.on_themes() =>
            {
                match themes.get(self.window.focus()) {
                    Some(card) if card.may_be_taken_away() => {
                        SettingsOutcome::RemoveTheme(card.theme.name.clone())
                    }
                    _ => SettingsOutcome::Consumed,
                }
            }
            // On the keys page, enter on a key is the reader saying "the
            // next key I press goes here instead", with `alt+backspace` to
            // take it away. On a command with no key there is nothing to
            // change, and
            // enter does nothing: a key is added with `ctrl+enter`, there as
            // on every row.
            KeyCode::Enter if bare && self.on_keys() => match keys.get(self.window.focus()) {
                Some((KeyRow::Setting(setting), _)) => self.change(setting, config),
                Some((KeyRow::Key { command, slot }, Some(_))) => {
                    self.binding = Some(Binding {
                        command: *command,
                        slot: *slot,
                        replacing: true,
                    });
                    self.refused = None;
                    SettingsOutcome::Consumed
                }
                Some((KeyRow::Key { .. }, None)) => SettingsOutcome::Consumed,
                None => SettingsOutcome::Consumed,
            },
            // A row of the remote page asks for what it is about, and the
            // application does it.
            KeyCode::Enter if bare && self.on_remote() => match rows.get(self.window.focus()) {
                Some(Shown::Remote { row, .. }) => SettingsOutcome::Remote(match row {
                    RemoteRow::Platform => Reaching::Platform,
                    RemoteRow::Field(field) => Reaching::Edit(field),
                    RemoteRow::People => Reaching::People,
                    RemoteRow::Pair => Reaching::Pair,
                    RemoteRow::Setup(_) => Reaching::Setup,
                }),
                _ => SettingsOutcome::Consumed,
            },
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
            // A variable is changed by typing what it holds, and the row
            // under them adds one.
            KeyCode::Enter
                if bare
                    && let Some(Shown::Variable { name, .. }) = rows.get(self.window.focus()) =>
            {
                SettingsOutcome::EditVariable((*name).to_string())
            }
            KeyCode::Enter
                if bare
                    && matches!(
                        rows.get(self.window.focus()),
                        Some(Shown::AddVariable { .. })
                    ) =>
            {
                SettingsOutcome::AddVariable
            }
            KeyCode::Enter if bare => {
                match rows.get(self.window.focus()).and_then(Shown::setting) {
                    Some(setting) => self.change(setting, config),
                    None => SettingsOutcome::Consumed,
                }
            }
            // Take it out of the project's file, which is what `delete` means
            // on the keys page too: this one is not set here any more.
            // Once the filter has nothing for it -- nothing held and nothing
            // in front of its caret -- because until then the key is the
            // filter's, the rule backspace follows over a list of names.
            // Only there -- the reader's own settings have no "unset", a
            // setting they have not changed is simply the default.
            // And on one of the agent's rows, wherever the page is: what
            // is left there is not a default of Obelus's but the agent's
            // own answer, so there is something to go back to.
            KeyCode::Delete | KeyCode::Backspace
                if self.query.leaves_to_the_row(key)
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
            // A variable taken away, which is what `delete` means on the
            // agent's other rows too: this one is not said any more.
            KeyCode::Delete | KeyCode::Backspace
                if self.query.leaves_to_the_row(key)
                    && let Some(Shown::Variable { name, .. }) = rows.get(self.window.focus()) =>
            {
                SettingsOutcome::RemoveVariable((*name).to_string())
            }
            // A field the reader set, forgotten: what `delete` means
            // everywhere else on this page.
            KeyCode::Delete | KeyCode::Backspace
                if self.query.leaves_to_the_row(key)
                    && let Some(Shown::Remote {
                        row: RemoteRow::Field(field),
                        ..
                    }) = rows.get(self.window.focus()) =>
            {
                match self.reached.kept.contains_key(field.key) {
                    true => SettingsOutcome::Remote(Reaching::Forget(field)),
                    false => SettingsOutcome::Consumed,
                }
            }
            KeyCode::Delete | KeyCode::Backspace
                if self.query.leaves_to_the_row(key)
                    && self.on_project()
                    && !self.on_keys()
                    && !self.on_cards() =>
            {
                match rows.get(self.window.focus()).and_then(Shown::setting) {
                    Some(setting) => SettingsOutcome::Unset(setting.key),
                    None => SettingsOutcome::Consumed,
                }
            }
            // Whatever the page did not want goes to the filter, which is
            // a line with a caret in it and takes the keys a line takes.
            //
            // The page is settled again only where what is typed changed:
            // a caret moved along the filter moved no row, and settling
            // takes the page back to its first.
            _ => {
                let said = self.query.said();
                match self.query.handle_key(key) {
                    true => {
                        if self.query.said() != said {
                            self.settle(offering);
                        }
                        SettingsOutcome::Consumed
                    }
                    false => SettingsOutcome::Ignored,
                }
            }
        }
    }

    /// A key while a number is being turned, which is every key: the box
    /// asked for them.
    ///
    /// Enter keeps what is in it, brought inside the dial's ends, and
    /// escape gives it up -- the way out of everything, nearest thing
    /// first. Nothing is in force before enter.
    fn turn(&mut self, key: &KeyEvent) -> SettingsOutcome {
        let Some(dialling) = self.dialling.as_mut() else {
            return SettingsOutcome::Ignored;
        };
        // Shift is let through on a digit, which some layouts need for one.
        let bare = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        match key.code {
            KeyCode::Esc => {
                self.dialling = None;
                SettingsOutcome::Consumed
            }
            KeyCode::Enter => {
                let Some(dialling) = self.dialling.take() else {
                    return SettingsOutcome::Consumed;
                };
                let kept = dialling.kept();
                // An empty box, or the number it opened on, is no change and
                // nothing is written: a line in the file the reader never
                // meant to put there is a setting that stops following the
                // default.
                match dialling.typed.is_empty() || kept == dialling.was {
                    true => SettingsOutcome::Consumed,
                    false => SettingsOutcome::Changed(dialling.key, Value::Count(kept)),
                }
            }
            KeyCode::Left | KeyCode::Right if bare => {
                let turned = dialling
                    .dial
                    .turned(dialling.kept(), key.code == KeyCode::Right);
                dialling.typed = turned.to_string();
                dialling.held = false;
                SettingsOutcome::Consumed
            }
            KeyCode::Char(digit) if bare && digit.is_ascii_digit() => {
                if dialling.held {
                    dialling.typed.clear();
                    dialling.held = false;
                }
                // No longer than the most it may be: past that every digit
                // is a number the dial would only bring back down.
                if dialling.typed.len() < dialling.dial.most.to_string().len() {
                    dialling.typed.push(digit);
                }
                SettingsOutcome::Consumed
            }
            KeyCode::Backspace if bare => {
                match dialling.held {
                    true => dialling.typed.clear(),
                    false => {
                        dialling.typed.pop();
                    }
                }
                dialling.held = false;
                SettingsOutcome::Consumed
            }
            _ => SettingsOutcome::Consumed,
        }
    }

    /// What enter on one of Obelus's own settings asks for, whichever page
    /// the row is on.
    fn change(&mut self, setting: &'static Setting, config: &Config) -> SettingsOutcome {
        match setting.kind {
            Kind::Switch => {
                let on = matches!(Self::value_of(setting, config), Value::Switch(true));
                SettingsOutcome::Changed(setting.key, Value::Switch(!on))
            }
            Kind::Choice(choices) => {
                let word = match Self::value_of(setting, config) {
                    Value::Choice(word) => word,
                    Value::Count(count) => count.to_string(),
                    Value::Switch(_) | Value::Names(_) | Value::Text(_) => String::new(),
                };
                SettingsOutcome::Choose(setting.key, choices, word)
            }
            // Turned in a box of its own rather than picked from a list of
            // the few Obelus thought of.
            Kind::Count(dial) => {
                let was = match Self::value_of(setting, config) {
                    Value::Count(count) => count,
                    _ => dial.least,
                };
                self.dialling = Some(Dialling {
                    key: setting.key,
                    dial,
                    was,
                    typed: was.to_string(),
                    held: true,
                });
                SettingsOutcome::Consumed
            }
            // A list the reader builds rather than one Obelus
            // offers, so what opens is not the short list of
            // choices but the thing that adds and orders.
            Kind::Names => SettingsOutcome::Names(setting.key),
            Kind::Text => match Self::value_of(setting, config) {
                Value::Text(said) => SettingsOutcome::Type(setting.key, said),
                _ => SettingsOutcome::Type(setting.key, String::new()),
            },
        }
    }

    /// Goes to a page, with `said` in the filter in place of whatever was
    /// there -- what a list that found nothing hands on.
    ///
    /// Nothing, on a page this file has not got.
    pub fn go_to(&mut self, page: Page, said: &str) {
        let Some(at) = Page::of(self.whose).iter().position(|on| *on == page) else {
            return;
        };
        self.page = at;
        self.query = Field::new();
        self.query.put(said);
        self.settle(None);
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
    pub fn settle_cards(&mut self, cards: Cards<'_>, room: (u16, u16)) {
        let heights = self.card_heights(cards, room);
        self.window.settle_by_height(&heights, room.1);
    }

    /// How tall each card on the page showing is, with the blank after it.
    fn card_heights(&self, cards: Cards<'_>, room: (u16, u16)) -> Vec<u16> {
        let width = Self::card_width(room.0);
        if self.on_themes() {
            let themes = self.themes(cards.themes);
            return (0..themes.len())
                .map(|at| self.theme_rows(&themes, at, width) + 1)
                .collect();
        }
        self.agents(cards.agents)
            .iter()
            .map(|agent| self.card_rows(agent, width) + 1)
            .collect()
    }

    /// And the same for a page of settings, whose entries are as tall as
    /// what they have to say.
    pub fn settle_rows(&mut self, room: (u16, u16), offering: Option<&Offering>) {
        let heights = self.entry_heights(room, offering);
        self.window.settle_by_height(&heights, room.1);
    }

    /// How tall each entry of a page of settings or of keys is.
    fn entry_heights(&self, room: (u16, u16), offering: Option<&Offering>) -> Vec<u16> {
        match self.on_keys() {
            // A key is a name and a chord: one row, the way it always was.
            // The layout above them is a setting, and as tall as one.
            true => self
                .key_rows()
                .into_iter()
                .map(|row| match row {
                    KeyRow::Setting(setting) => self.setting_rows(
                        &Shown::Obelus {
                            setting,
                            opens: None,
                        },
                        description_width(room.0),
                    ),
                    KeyRow::Key { .. } => 1,
                })
                .collect(),
            false => self
                .rows(offering)
                .iter()
                .map(|shown| self.setting_rows(shown, description_width(room.0)))
                .collect(),
        }
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
        let warning = self
            .warning_of(shown)
            .map_or(0, |warning| self.wrapped(&warning, width).len());
        let about = about + u16::try_from(warning).unwrap_or(0);
        // A heading is its word and the blank under it: the word alone, with
        // the group's first setting hard against it, reads as a row of the
        // group rather than as its name. The agent's carries a line saying
        // when what is under it takes effect, which is the one thing about
        // this group that is not true of the others -- so it is three.
        let heading = match shown {
            Shown::Obelus { opens, .. } => u16::from(opens.is_some()) * 2,
            Shown::Agent { opens, .. } => u16::from(opens.is_some()) * HEADING_ROWS,
            Shown::Silent { .. } => HEADING_ROWS,
            Shown::Variable { opens, .. } | Shown::AddVariable { opens } => {
                u16::from(opens.is_some()) * HEADING_ROWS
            }
            Shown::Remote { opens, .. } => u16::from(opens.is_some()) * 2,
        };
        // A row with no name is its prose and the blank after it: there is
        // nothing to put a name on.
        let named = u16::from(!matches!(shown, Shown::Silent { .. }));
        heading + named + about + 1
    }
}
