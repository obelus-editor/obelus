//! What the reader has decided, and where it is kept.
//!
//! One flat file of `key = value` lines, written every time anything changes.
//!
//! **Do not delete what you do not recognise.** It is *edited*, with
//! `toml_edit`, rather than written whole: Obelus used to write it whole on
//! the grounds that Obelus wrote all of it, which is not true -- readers put
//! lines in by hand -- and that silently took out a setting from a newer
//! version, a key renamed since, a line with a typo in it, and the comment
//! beside them, on the next switch the reader flipped. A project's file was
//! already edited for exactly this reason; see [`over`].
//!
//! **Anything written must survive another process writing it at the same
//! moment.** Several Obelus processes on one project is the normal case.
//! [`save_to`] wrote in place, which truncates first; a second Obelus reading
//! in that gap got an empty file, took it for "no settings", and wrote its
//! defaults over everything the reader had. It writes beside the file and
//! renames over it now -- the one filesystem operation with no gap in it, so
//! a write cannot half-apply either.
//!
//! Missing, unreadable, or nonsense all start a session on the defaults. A
//! reader whose config file has a typo in it should get Obelus, not an error
//! message where their editor was. But they are not one answer ([`Reading`]):
//! a file that is there and cannot be read stops Obelus writing at all,
//! because what is in it is the reader's and saving over something it could
//! not read replaces settings it never saw. The application says so on the
//! status row and starts saving again the moment the file reads, which the
//! watcher notices.
//!
//! The settings are a *table* ([`ALL`]), the way the commands are: a setting
//! is a row with a name, a group, a kind of control and a way to read and
//! write it. The settings view is built from that table and knows nothing
//! about any particular setting.
//!
//! What a project may set is a property of the setting, `Reach`, not a list of
//! exceptions somewhere: the next setting a stranger should not be trusted with
//! will be found by asking that question while writing the setting down.
//! `agent` is `ReaderOnly` because it says which agent Obelus *starts*, and a
//! program starting because a file in a downloaded project said so is a
//! decision that belongs to the person at the keyboard; `keys` is `ReaderOnly`
//! because a project that could rebind them could put `quit` where a reader
//! would find it by accident; and `agents` is `ReaderOnly` for the first reason
//! twice over -- what an agent may do without asking is the setting a
//! downloaded project would most like to write. VS Code learned this one the
//! same way and calls it `machine` scope. `remote` and `remotes` are the
//! reader's for a reason of their own: they say who may talk to this machine
//! from a chat, and a project that could name a person there could hand a
//! stranger the agent.
//!
//! The configuration file holds preferences, not state. `config.rs` is the
//! whole of it — one table, `dirs` for where it lives, written the moment
//! anything changes. Rebindable keys are still guaranteed by the key table
//! being *data* rather than by the file. What is not a preference does not go
//! in it: how to start an installed agent is written beside the install, not
//! here.

use std::path::{Path, PathBuf};

use obelus_text::coordinates::Span;

/// The theme a reader who has not chosen one gets.
///
/// Here rather than beside the themes themselves because it is a written
/// default like every other field below it -- the name of a theme, not a
/// theme -- and the list of names Obelus will accept is already next door.
pub const DEFAULT_THEME: &str = "dark";

/// What an agent calls itself for a reader who has not said.
pub const DEFAULT_SPEAKS_AS: &str = "Obelus";

/// Everything the reader can decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Which built-in theme to use, by name.
    pub theme: String,
    /// Whether to draw Nerd Font glyphs.
    pub icons: bool,
    /// Whether the margin says who last changed each line.
    ///
    /// About the margin, not about blame: whether the commit behind one
    /// line can be asked for is a different question, and a reader who
    /// wants no names beside their code has not said never to ask it.
    pub blame_margin: bool,
    /// Whether a line too long for the screen continues on the next row.
    pub wrap: bool,
    /// Whether things arrive where they are going instead of being there.
    ///
    /// A window's setting and nothing to a terminal, whose unit is a
    /// whole cell: what this is about is the caret walking to the cell it
    /// was sent to, a pane sliding in from the edge it hangs off, and a
    /// list catching up with where it has scrolled to -- none of which a
    /// terminal can draw a step of. On by default, because what it is for
    /// is following something with the eye rather than finding it again.
    pub animation: bool,
    /// How wide a tab is drawn, and how many spaces the tab key puts in.
    pub tab_width: usize,
    /// The faces text is drawn in, tried in the order they are written.
    ///
    /// Empty means the machine's own monospaced face, whichever that is.
    /// A name this machine does not have is stepped over rather than
    /// refused: one settings file is read on every machine the reader
    /// uses, and the fonts installed are not the same on two of them.
    pub fonts: Vec<String>,
    /// How big the text is, in points, where Obelus draws its own.
    ///
    /// A window's setting and nothing to a terminal, whose font is the
    /// terminal's own business and not Obelus's to have an opinion about.
    /// It is in the one settings file either way: two Obeluses on one
    /// machine are the normal case, and a reader who set this in a window
    /// has not asked for a second file to keep it in.
    pub font_size: usize,
    /// Whether the reader's own keys may change a file.
    ///
    /// For a reader who has the agent make every change: typing, pasting,
    /// cutting, undoing and the commands that rewrite lines are refused in
    /// a file, and what an agent writes still goes in. Not about the boxes
    /// a reader types into -- a message to the agent is the way the code
    /// gets changed at all -- and not about saving, because what an agent
    /// wrote into an open file reaches the disk by being saved.
    pub read_only: bool,
    /// Whether to ask a language server to lay the file out before writing.
    pub format_on_save: bool,
    /// Whether to make the server's whole-file fixes before writing.
    ///
    /// The protocol's `source.` code actions: what a server offers to do
    /// to a file rather than to a place in one -- the imports sorted, the
    /// corrections it can make on its own. They are the only actions
    /// Obelus takes without a key being pressed, which is why they are the
    /// only ones it asks for by name.
    pub code_actions_on_save: bool,
    /// Whether what a language server works out is drawn in the file.
    ///
    /// A type nobody wrote down, the name of the parameter an argument is
    /// passed to. On, because the thing a reader wants to know about a
    /// `let` with no type on it is the type -- and a switch nobody finds
    /// is a feature nobody has. They are cells the file does not contain,
    /// which is what the switch is for: turned off, the file on disk is
    /// what is on screen, to the column.
    pub inlay_hints: bool,
    /// Whether what a server says is wrong with the line the caret is on is
    /// opened under it.
    ///
    /// The underline is not this switch: what is wrong is marked on every
    /// line that has something wrong with it, always, and costs no room.
    /// This is the *words*, which take a row of the file's own space -- so
    /// a reader who would rather keep the shape of the code and read the
    /// complaint from the list can have that.
    pub diagnostics: bool,
    /// Whether the file list offers the files a project has said to ignore.
    ///
    /// About the list, not about the files: what `.gitignore` keeps out is
    /// kept out of the *offer*, and a reader who knows the path can still
    /// open one. Which is why this is worth a key as well as a switch --
    /// "where is that build log" is a question asked once and then not
    /// again for a week.
    pub ignored_files: bool,
    /// Whether the file list offers the files a system keeps out of sight.
    ///
    /// Which is not one rule: a name beginning with a dot everywhere, and
    /// on Windows the attribute as well -- `ignore` answers both there and
    /// only the first elsewhere. Said in the setting's own words rather
    /// than in one platform's, because one file is read on every machine
    /// the reader uses and a `cfg!` in copy would be two readings of one
    /// switch.
    ///
    /// Its own switch and not part of [`Config::ignored_files`], because
    /// they keep two different things out: one is what a project said to
    /// ignore and the other is what a convention says not to show. A
    /// reader after `.github/workflows/ci.yml` is not asking to see
    /// `target`, and one after a build log is not asking to see `.env`.
    ///
    /// `.git` comes with it, which is the cost of the switch meaning what
    /// it says: a directory whose files are a database is still a
    /// directory of files, and an exception carved out here would be
    /// Obelus deciding which of the reader's hidden files they meant.
    pub hidden_files: bool,
    /// Whether a tree opens again on what was open the last time it was,
    /// with the caret where it was in each.
    ///
    /// On, because a reader who left in the middle of something comes back
    /// to the middle of it -- and one who would rather begin on an empty
    /// screen each time can say so here.
    pub reopen: bool,
    /// How long the pointer has to rest on a word before Obelus asks what
    /// it is, in milliseconds.
    ///
    /// Zero is off: the pointer then asks nothing, and `alt+h` is the way
    /// in. A time rather than a switch because the answer people want
    /// differs by more than on and off -- a reader who knows the code
    /// wants it slow enough never to appear by accident, and one reading
    /// somebody else's wants it as fast as their hand stops.
    pub hover_delay: usize,
    /// How many days a conversation about no note in particular is
    /// remembered after anything was last said in it.
    ///
    /// Zero forgets none. Only those, because one about a note already goes
    /// when the note does, and a note still there is work not yet done --
    /// coming back to it a season later is coming back to that
    /// conversation. What is forgotten is Obelus's own line about it: the
    /// agent keeps every word, and nothing is asked of it.
    pub conversation_days: usize,
    /// Whether Obelus asks, once a day, whether a newer version is out.
    ///
    /// The one thing Obelus asks the network for that the reader did not
    /// press a key for, which is why it is a switch: a reader who would
    /// rather their reader did not talk to anybody unasked can have that,
    /// and then nothing is sent at all -- not a request whose answer is
    /// thrown away.
    pub new_versions: bool,
    /// How an agent is to change this project, by the workflow's name.
    ///
    /// A word rather than a switch though there is one workflow, because
    /// workflows exclude each other: two switches could both be on, and
    /// turning one into a list later would leave every file that wrote
    /// the switch with a line that did nothing.
    pub workflow: String,
    /// What an agent calls itself in a conversation.
    ///
    /// A name rather than "I", because the reader's own messages say "I"
    /// on the same page, and a reader who cannot tell whose "I" a sentence
    /// is cannot tell who did the thing it says was done. `Obelus` unless
    /// the reader says otherwise: to them the agent is part of the editor
    /// they are in.
    pub speaks_as: String,
    /// Which agent Obelus talks to, by the registry's own name for it.
    ///
    /// One, or none. Two would mean every question having to say which
    /// agent it was for, and a reader having to know.
    pub agent: Option<String>,
    /// The keys the reader has moved, by the command's own name.
    ///
    /// Changes rather than the whole table: a reader who rebinds one key
    /// should be given the new default for everything they said nothing
    /// about. An empty chord is a key taken away, which is also a decision.
    pub keys: std::collections::BTreeMap<String, String>,
    /// What each agent is to start a conversation on, in its own words.
    ///
    /// The agent's id, then the agent's id for one of the settings it
    /// offers, then the agent's id for the value. Obelus understands none
    /// of the three and is not meant to: which settings an agent has is
    /// the agent's, and this is the reader saying which of them they would
    /// rather not choose again every time.
    ///
    /// Only what they have said. A setting that is not in here is one
    /// Obelus says nothing about, and the conversation starts on whatever
    /// the agent starts it on -- which is a different thing from starting
    /// on the value the agent happened to be on last time, and the reason
    /// this is a map of what was said rather than a copy of a session.
    pub agents: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Which chat a note can be worked on from, by the platform's own key.
    ///
    /// One, or none, the way the agent is: a question put to two chats at
    /// once is three places that can answer it, and which one did is a
    /// thing nobody should have to work out.
    pub remote: Option<String>,
    /// What each chat keeps here, by the platform's key.
    ///
    /// Every platform's, not only the one in use: switching to another and
    /// back is not a reason to set the first one up again. What is secret
    /// is not here at all -- a token is in the keyring, and this file is one
    /// a reader may well keep in a public repository of dotfiles.
    pub remotes: std::collections::BTreeMap<String, Remote>,
}

/// What one chat platform keeps in the settings file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    /// Its settings that are not secret, by the platform's own name for
    /// each. Obelus understands none of them here: which there are is the
    /// platform's, the way which settings an agent has is the agent's.
    pub values: std::collections::BTreeMap<String, String>,
    /// Who may talk to this machine through it.
    pub people: Vec<Person>,
}

impl Remote {
    /// Whether there is nothing in it, which is a table not worth writing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.people.is_empty()
    }
}

/// Somebody allowed to talk to this machine from a chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    /// The platform's id for them, which is what a message is checked by.
    /// A name can be changed by its owner; this cannot.
    pub id: String,
    /// What they are called, for a reader looking down the list.
    pub name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            // Off, because a patched font is a thing the reader has to have
            // gone and got, and whether they have cannot be asked: a
            // default that assumes it draws a box beside every name for
            // everybody who has not, and a box is how Obelus looks broken.
            // Without the font, the fallbacks read correctly; with it, one
            // switch turns the glyphs on.
            icons: false,
            blame_margin: true,
            // Off, so a line is a line: a reader counting rows, comparing
            // two files side by side, or looking at a table in a comment is
            // reading something the screen has not rearranged. The reader
            // who wants it can say so, and then it is a line's own choice
            // no longer.
            wrap: false,
            // On, because what it is for is following something
            // with the eye rather than finding it again.
            animation: true,
            // What the code Obelus is written in uses, which is also what
            // the rest of the program laid a tab out at before a reader
            // could say otherwise.
            tab_width: obelus_text::TAB_WIDTH,
            fonts: Vec::new(),
            font_size: DEFAULT_FONT_SIZE,
            // Off: a formatter that ran without being asked would rewrite
            // a file somebody opened to read, and the first they would know
            // of it is the diff.
            read_only: false,
            format_on_save: false,
            // Off, for the reason above it: a file somebody opened to read
            // should not come back from a save with its imports rearranged
            // and three lines somewhere else rewritten.
            code_actions_on_save: false,
            inlay_hints: true,
            diagnostics: true,
            // Off, because a project says what it ignores and mostly means it:
            // a list whose first hundred rows are `target` is a list nobody
            // can find anything in.
            ignored_files: false,
            hidden_files: false,
            reopen: true,
            // Long enough that crossing a line of code does not ask about
            // every word on the way, short enough that a reader who has
            // stopped does not wonder whether Obelus noticed. The figure
            // every editor with a mouse uses.
            hover_delay: 400,
            // A month: long enough that a conversation left over a holiday
            // is still there, short enough that the list is of what the
            // reader is doing rather than of everything they ever asked.
            conversation_days: 30,
            // On, because a reader who installed Obelus from a release has
            // no other way to hear of the next one.
            new_versions: true,
            // A branch of its own: an agent changing the checkout the
            // reader is reading changes it under them, and a worktree is
            // what lets them see the change arrive and decide what becomes
            // of it.
            workflow: "feature-branch".to_string(),
            speaks_as: DEFAULT_SPEAKS_AS.to_string(),
            // None until the reader installs one: Obelus does not choose an
            // agent for anybody.
            agent: None,
            // Nothing moved: the table Obelus ships with.
            keys: std::collections::BTreeMap::new(),
            // And nothing said about any agent: every conversation starts
            // where the agent starts it.
            agents: std::collections::BTreeMap::new(),
            // No chat: a machine is not reachable from one until its
            // reader says so.
            remote: None,
            remotes: std::collections::BTreeMap::new(),
        }
    }
}

/// A setting's value, in whichever shape its control has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// On or off.
    Switch(bool),
    /// One of a list of words.
    Choice(String),
    /// A number.
    ///
    /// Its own thing rather than a `Choice` that happens to spell a number:
    /// a reader opening the file sees `tab_width = 4` rather than `"4"`, and
    /// what uses it wants a number rather than a parse.
    Count(usize),
    /// An ordered list of names the reader built.
    ///
    /// Ordered because the order is the answer: these are the faces text is
    /// tried in, first to last. A set would lose the only thing the reader
    /// said.
    Names(Vec<String>),
    /// Whatever the reader typed.
    Text(String),
}

/// What sort of control a setting gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Switch,
    /// One of a fixed list of words.
    Choice(&'static [&'static str]),
    /// A number, offered as the few that anybody picks.
    ///
    /// The same control a choice gets, because picking from a short list is
    /// what a reader is doing either way -- and a spinner for a number with
    /// three sensible values is a control nobody needs.
    Count(&'static [&'static str]),
    /// A list the reader builds by name, in the order they want it tried.
    ///
    /// No list of choices here, unlike the two above: what may go in comes
    /// from the machine Obelus is running on rather than from anything
    /// Obelus ships, and a setting written on one machine is read on
    /// another. So the control offers what is here and takes what is
    /// typed.
    Names,
    /// A line the reader types, asked for on the status row.
    ///
    /// No list at all: what may go in is anything a reader would call
    /// something, and a list of the names Obelus thought of would be a
    /// list of names that are not theirs.
    Text,
}

/// Where a setting means anything.
///
/// Obelus is drawn on two things and they do not ask for the same
/// preferences: a terminal draws with the font the reader gave the
/// terminal, and a window draws with its own, so `font_size` means nothing
/// in one and the glyph switch means nothing in the other. A setting shown
/// where it does nothing is worse than a missing one -- the reader changes
/// it, watches nothing happen, and has learnt something untrue about the
/// program.
///
/// It is still one file and one table. What is hidden is a row on the
/// settings page; the value stays in the file, because the other Obelus on
/// the same machine is the one it is for -- and writing the file is careful
/// not to delete what it does not recognise for the same reason.
///
/// What a front end owns, it is told about: `App::drawn_by` and one method
/// per setting, called when the front end says who it is and again after
/// every change, which in `obg` goes down the frames channel like everything
/// else about what is on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// Wherever Obelus is drawn.
    Anywhere,
    /// Only where a terminal is drawing it.
    InATerminal,
    /// Only where Obelus draws its own pixels.
    InAWindow,
}

impl Drawn {
    /// Whether this Obelus is drawn where it means anything.
    ///
    /// The one answer, for everything that says where it is drawn: a
    /// setting, and a command.
    #[must_use]
    pub fn here(self) -> bool {
        match self {
            Self::Anywhere => true,
            Self::InATerminal => !in_a_window(),
            Self::InAWindow => in_a_window(),
        }
    }
}

/// Whether this Obelus is the one that draws its own pixels.
///
/// A global, like the tab width and the glyph switch, and for the same
/// reason: it is one decision the whole program shares, settled once before
/// anything is drawn, and threading it through would put a parameter on
/// every list of settings rather than on the one fact.
static IN_A_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Says that Obelus is drawing its own pixels, which `obg` says once.
pub fn drawn_in_a_window() {
    IN_A_WINDOW.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether Obelus is drawing its own pixels.
#[must_use]
pub fn in_a_window() -> bool {
    IN_A_WINDOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Which group of settings a setting belongs to.
///
/// Coarse on purpose: a reader looking for a setting scans one screen, and
/// a dozen groups of two rows each is a worse index than two groups of a
/// dozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// How Obelus looks.
    Appearance,
    /// What it says about the file being read.
    Reading,
    /// Which files Obelus offers, and where it looks for them.
    Files,
    /// How an agent goes about its work.
    Agent,
}

impl Group {
    /// Every group, in the order their tabs sit in.
    pub const ALL: [Self; 4] = [Self::Appearance, Self::Reading, Self::Files, Self::Agent];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Reading => "Reading",
            Self::Files => "Files",
            Self::Agent => "Agent",
        }
    }
}

/// Who may set a setting.
///
/// A project's own settings are written by whoever wrote the project, and a
/// reader who opens somebody's repository has not agreed to everything in it.
/// Most of these are harmless to hand over -- a theme, a wrapped line, a name
/// in the margin -- and some are not: `agent` says which agent Obelus starts,
/// and a program starting because a file in a downloaded project said so is a
/// decision that belongs to the person at the keyboard. The keys are the
/// same: a project that could rebind them could put a reader's `quit` somewhere
/// they would find by accident.
///
/// A kind rather than a list of exceptions, because the next one of these
/// will be found the way this one was -- by asking, of a new setting,
/// whether a stranger may set it -- and the asking should be part of writing
/// the setting down rather than something to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Either file: the reader's own, or the project's.
    Anywhere,
    /// The reader's own file alone. A project naming it is ignored, with a word
    /// in the log for whoever wrote that file.
    ReaderOnly,
}

/// Which file a table of settings came out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The reader's, wherever this system keeps such things.
    Reader,
    /// The project Obelus was opened on.
    Project,
}

/// One setting, as data.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    /// What it is called in the file.
    pub key: &'static str,
    /// What the view calls it: one word where one will do.
    ///
    /// A *name*, not a sentence. These were sentences -- "Colour theme",
    /// "Who last changed the line the cursor is on" -- on the grounds that a
    /// name and a description side by side read as a heading and a footnote.
    /// Which is true when the footnote says what the heading already had;
    /// what it actually produced was a column of prose, where a reader
    /// looking for the one row they came for had to read every row to find
    /// it. A column of names is scanned. The palette has said a name and a
    /// description side by side since the beginning, and this is that row.
    pub name: &'static str,
    /// What it does, in the dim colour after the name.
    ///
    /// Empty where the name is the whole of it: `Theme` needs no gloss, and
    /// a line of prose saying "the colour theme" beside it is the footnote
    /// that objection was about.
    pub about: &'static str,
    /// Which tab it lives under.
    pub group: Group,
    /// What sort of control it gets.
    pub kind: Kind,
    /// Which files may set it.
    pub reach: Reach,
    /// Where it means anything.
    pub drawn: Drawn,
}

impl Setting {
    /// Whether a table out of `whose` file may set this.
    #[must_use]
    pub fn settable_by(&self, whose: Whose) -> bool {
        whose == Whose::Reader || self.reach == Reach::Anywhere
    }

    /// Whether this setting is one to show the reader here.
    #[must_use]
    pub fn shown(&self) -> bool {
        self.drawn.here()
    }

    /// The setting a key names, if Obelus has one.
    #[must_use]
    pub fn named(key: &str) -> Option<&'static Self> {
        ALL.iter().find(|setting| setting.key == key)
    }
}

/// The themes a reader can choose between.
const THEMES: &[&str] = &["dark", "light"];

/// The tab widths anybody sets.
const WIDTHS: &[&str] = &["2", "4", "8"];

/// The text sizes a window offers, in points.
///
/// A list rather than a number to nudge up and down, for the reason every
/// other count here is a list: a reader picking a size tries three of them
/// and keeps one, and the sizes between are a difference nobody sees. What
/// is offered is small enough to fit a lot of code on the screen and large
/// enough to read on a dense one.
const SIZES: &[&str] = &["11", "12", "13", "14", "16", "18", "20", "24"];

/// How big the text is where Obelus draws its own, in points.
pub const DEFAULT_FONT_SIZE: usize = 14;

/// How long a rest is, in milliseconds, as the few anybody picks.
///
/// Zero is the list's way of saying "not at all": a fourth control meaning
/// off, beside a list that already has a slowest, is a second way to say
/// the same thing.
const DELAYS: &[&str] = &["0", "200", "400", "800"];

/// How long a conversation is remembered, in days, as the few anybody picks.
///
/// Zero is never forgetting, the way it is off for a rest.
const DAYS: &[&str] = &["0", "7", "30", "90", "365"];

/// The workflows an agent can be asked to follow.
///
/// `none` is the agent's own way. What each one says -- to the reader in
/// the list it is chosen from, and to the agent -- is a file in
/// `obelus-app`, which a test there holds to this list.
const WORKFLOWS: &[&str] = &["none", "in-place", "feature-branch"];

/// Every setting Obelus has.
pub const ALL: &[Setting] = &[
    Setting {
        key: "theme",
        name: "Theme",
        // Nothing: the name is the whole of it. A line of prose saying "the
        // colours Obelus draws in" beside it is the footnote `Setting::about`
        // is about not having.
        about: "",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Choice(THEMES),
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "icons",
        name: "Nerd Font glyphs",
        about: "In lists, on the status bar, and beside a file's name -- a terminal without a patched font draws a box instead",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        // A window carries the face the glyphs are in, so there is nothing
        // to decide: they are drawn. The switch is a terminal's, which
        // draws with whatever font the reader installed -- a guess Obelus
        // cannot make for them.
        drawn: Drawn::InATerminal,
    },
    Setting {
        key: "wrap",
        name: "Wrapping",
        about: "A line too long for the screen carries onto the next row, broken between words",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "blame_margin",
        name: "Blame",
        about: "Who last changed the line the cursor is on",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "tab_width",
        name: "Tab width",
        about: "How wide a tab is drawn, and how many spaces one puts in",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Count(WIDTHS),
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "fonts",
        name: "Fonts",
        about: "The faces text is drawn in, tried in the order they are written",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Names,
        drawn: Drawn::InAWindow,
    },
    Setting {
        key: "animation",
        name: "Animation",
        about: "Let the caret, a list and a pane arrive where they are going instead of being there",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::InAWindow,
    },
    Setting {
        key: "font_size",
        name: "Text size",
        about: "How big the text is in a window, in points",
        group: Group::Appearance,
        reach: Reach::Anywhere,
        kind: Kind::Count(SIZES),
        drawn: Drawn::InAWindow,
    },
    Setting {
        key: "hover_delay",
        name: "Ask on a rest",
        about: "How long the pointer has to rest on a word before Obelus says what it is, in milliseconds -- zero asks only when a key does",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Count(DELAYS),
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "read_only",
        name: "Read only",
        about: "Your own keys change no file: typing, pasting and undo are refused there, and what an agent writes still goes in",
        group: Group::Reading,
        // The reader's alone: whether their keys may change a file is how
        // they work, and a repository has no say in it.
        reach: Reach::ReaderOnly,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "format_on_save",
        name: "Formatting",
        about: "Ask the language server to lay the file out before writing it",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "code_actions_on_save",
        name: "Server fixes",
        about: "Before writing, make the changes the language server offers for the whole file -- the imports sorted, the corrections it can make on its own",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "inlay_hints",
        name: "Inferred types",
        about: "Draw the types and parameter names a language server infers, where they would be written. They are not in the file: nothing in one can be selected or copied",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "diagnostics",
        name: "Problem messages",
        about: "Open the words under the line the caret is on, which costs that line a row of the file's own space. What is wrong is underlined on every line either way",
        group: Group::Reading,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "new_versions",
        name: "New versions",
        about: "Ask GitHub once a day whether a newer Obelus is out, and say so on the welcome screen",
        group: Group::Appearance,
        // The reader's alone: a project that could turn it on would be a
        // downloaded file deciding that Obelus talks to the network for a
        // reader who said it should not.
        reach: Reach::ReaderOnly,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "ignored_files",
        name: "Ignored files",
        about: "Offer them in the file list as well -- what `.gitignore` keeps out is build output most days and the file you are looking for on the others",
        group: Group::Files,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "hidden_files",
        name: "Hidden files",
        about: "Offer them in the file list as well -- a name beginning with a dot, and on Windows the attribute too; `.github` and `.env` are files like any other, and `.git` comes with them",
        group: Group::Files,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "reopen",
        name: "Reopen",
        about: "Open what was open the last time this tree was, with the caret where it was in each",
        group: Group::Files,
        reach: Reach::Anywhere,
        kind: Kind::Switch,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "workflow",
        name: "Workflow",
        about: "How an agent goes about changing this project",
        group: Group::Agent,
        // A project's too, because it is about the project: whether its
        // changes go through pull requests is the project's own rule. And
        // nothing in it is done unasked but the worktree, which is a
        // directory the project already ignores.
        reach: Reach::Anywhere,
        kind: Kind::Choice(WORKFLOWS),
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "speaks_as",
        name: "Speaks as",
        about: "What an agent calls itself in a conversation, so that an \"I\" there is always yours",
        group: Group::Agent,
        // A project's too: what an agent is called is no more than a word
        // in its sentences, and a project that names its own is not
        // starting or allowing anything.
        reach: Reach::Anywhere,
        kind: Kind::Text,
        drawn: Drawn::Anywhere,
    },
    Setting {
        key: "conversation_days",
        name: "Forget conversations",
        about: "How long after anything was last said in it a conversation about no note is forgotten. The agent still has it, but Obelus forgets the way back, and making this longer does not bring it back. One about a note goes when the note does",
        group: Group::Agent,
        // The reader's alone: it decides what Obelus forgets of theirs, and
        // a project that could shorten it would be a downloaded file
        // emptying their list.
        reach: Reach::ReaderOnly,
        kind: Kind::Count(DAYS),
        drawn: Drawn::Anywhere,
    },
];

impl Config {
    /// What a setting is set to.
    ///
    /// One match per setting rather than a map, so a setting added to the
    /// table without being read here does not compile.
    #[must_use]
    pub fn value_of(&self, key: &str) -> Option<Value> {
        match key {
            "theme" => Some(Value::Choice(self.theme.clone())),
            "icons" => Some(Value::Switch(self.icons)),
            "blame_margin" => Some(Value::Switch(self.blame_margin)),
            "wrap" => Some(Value::Switch(self.wrap)),
            "tab_width" => Some(Value::Count(self.tab_width)),
            "font_size" => Some(Value::Count(self.font_size)),
            "fonts" => Some(Value::Names(self.fonts.clone())),
            "hover_delay" => Some(Value::Count(self.hover_delay)),
            "conversation_days" => Some(Value::Count(self.conversation_days)),
            "animation" => Some(Value::Switch(self.animation)),
            "read_only" => Some(Value::Switch(self.read_only)),
            "format_on_save" => Some(Value::Switch(self.format_on_save)),
            "code_actions_on_save" => Some(Value::Switch(self.code_actions_on_save)),
            "inlay_hints" => Some(Value::Switch(self.inlay_hints)),
            "diagnostics" => Some(Value::Switch(self.diagnostics)),
            "ignored_files" => Some(Value::Switch(self.ignored_files)),
            "hidden_files" => Some(Value::Switch(self.hidden_files)),
            "reopen" => Some(Value::Switch(self.reopen)),
            "new_versions" => Some(Value::Switch(self.new_versions)),
            "workflow" => Some(Value::Choice(self.workflow.clone())),
            "speaks_as" => Some(Value::Text(self.speaks_as.clone())),
            "agent" => Some(Value::Choice(self.agent.clone().unwrap_or_default())),
            "remote" => Some(Value::Choice(self.remote.clone().unwrap_or_default())),
            _ => None,
        }
    }

    /// Sets one setting, ignoring a value of the wrong shape.
    pub fn set(&mut self, key: &str, value: &Value) {
        match (key, value) {
            ("theme", Value::Choice(word)) => self.theme = word.clone(),
            ("icons", Value::Switch(on)) => self.icons = *on,
            ("blame_margin", Value::Switch(on)) => self.blame_margin = *on,
            ("wrap", Value::Switch(on)) => self.wrap = *on,
            // Clamped where it is read rather than refused here: a file
            // somebody typed `0` into should not make every tab nothing.
            ("tab_width", Value::Count(width)) => self.tab_width = *width,
            ("hover_delay", Value::Count(delay)) => self.hover_delay = *delay,
            ("conversation_days", Value::Count(days)) => self.conversation_days = *days,
            ("font_size", Value::Count(points)) => self.font_size = *points,
            ("fonts", Value::Names(names)) => self.fonts = names.clone(),
            ("animation", Value::Switch(on)) => self.animation = *on,
            ("read_only", Value::Switch(on)) => self.read_only = *on,
            ("format_on_save", Value::Switch(on)) => self.format_on_save = *on,
            ("code_actions_on_save", Value::Switch(on)) => self.code_actions_on_save = *on,
            ("inlay_hints", Value::Switch(on)) => self.inlay_hints = *on,
            ("diagnostics", Value::Switch(on)) => self.diagnostics = *on,
            ("ignored_files", Value::Switch(on)) => self.ignored_files = *on,
            ("hidden_files", Value::Switch(on)) => self.hidden_files = *on,
            ("reopen", Value::Switch(on)) => self.reopen = *on,
            ("new_versions", Value::Switch(on)) => self.new_versions = *on,
            ("workflow", Value::Choice(word)) => self.workflow = word.clone(),
            ("speaks_as", Value::Text(name)) => self.speaks_as = speaks_as(name),
            // An empty word is nobody, which is how a reader stops talking
            // to an agent without a second setting meaning "off".
            ("agent", Value::Choice(word)) => {
                self.agent = (!word.is_empty()).then(|| word.clone());
            }
            // The same: an empty word is no chat.
            ("remote", Value::Choice(word)) => {
                self.remote = (!word.is_empty()).then(|| word.clone());
            }
            _ => tracing::debug!(key, ?value, "a setting that does not take this"),
        }
    }

    /// What this agent is to start one of its settings on, if the reader
    /// has said.
    #[must_use]
    pub fn agent_default(&self, agent: &str, setting: &str) -> Option<&str> {
        Some(self.agents.get(agent)?.get(setting)?.as_str())
    }

    /// Everything they have said about one agent.
    ///
    /// Empty where they have said nothing, which is the ordinary case and
    /// not a state worth a second answer: there is no difference between
    /// an agent nobody has set anything on and one whose settings were all
    /// unset again.
    #[must_use]
    pub fn agent_defaults(&self, agent: &str) -> &std::collections::BTreeMap<String, String> {
        static NOTHING: std::sync::LazyLock<std::collections::BTreeMap<String, String>> =
            std::sync::LazyLock::new(std::collections::BTreeMap::new);
        self.agents.get(agent).unwrap_or(&NOTHING)
    }

    /// Says what one of them is to start on.
    pub fn set_agent_default(&mut self, agent: &str, setting: &str, value: &str) {
        self.agents
            .entry(agent.to_string())
            .or_default()
            .insert(setting.to_string(), value.to_string());
    }

    /// Stops saying, which puts the setting back in the agent's hands.
    ///
    /// The agent's own table goes when the last of its settings does: a
    /// table with nothing in it says Obelus was thinking about that agent,
    /// which after this it is not.
    pub fn unset_agent_default(&mut self, agent: &str, setting: &str) {
        let Some(chosen) = self.agents.get_mut(agent) else {
            return;
        };
        chosen.remove(setting);
        if chosen.is_empty() {
            self.agents.remove(agent);
        }
    }

    /// What one chat platform keeps here, or nothing.
    #[must_use]
    pub fn remote_of(&self, platform: &str) -> Option<&Remote> {
        self.remotes.get(platform)
    }

    /// One of a platform's settings that is not secret.
    #[must_use]
    pub fn remote_value(&self, platform: &str, field: &str) -> Option<&str> {
        Some(self.remotes.get(platform)?.values.get(field)?.as_str())
    }

    /// Sets one of them, or with `None` takes it out.
    ///
    /// The platform's table goes when the last thing in it does, for the
    /// reason an agent's does.
    pub fn set_remote_value(&mut self, platform: &str, field: &str, value: Option<&str>) {
        let remote = self.remotes.entry(platform.to_string()).or_default();
        match value {
            Some(value) => {
                remote.values.insert(field.to_string(), value.to_string());
            }
            None => {
                remote.values.remove(field);
            }
        }
        self.drop_remote_if_empty(platform);
    }

    /// Lets somebody talk to this machine through a platform.
    ///
    /// By id: a person paired twice is the same person, under whichever name
    /// they go by now.
    pub fn add_person(&mut self, platform: &str, person: Person) {
        let people = &mut self.remotes.entry(platform.to_string()).or_default().people;
        match people.iter_mut().find(|known| known.id == person.id) {
            Some(known) => known.name = person.name,
            None => people.push(person),
        }
    }

    /// Stops letting them.
    pub fn remove_person(&mut self, platform: &str, id: &str) {
        if let Some(remote) = self.remotes.get_mut(platform) {
            remote.people.retain(|person| person.id != id);
        }
        self.drop_remote_if_empty(platform);
    }

    fn drop_remote_if_empty(&mut self, platform: &str) {
        if self.remotes.get(platform).is_some_and(Remote::is_empty) {
            self.remotes.remove(platform);
        }
    }
}

/// Where the file lives, or `None` on a system with nowhere to put it.
///
/// `dirs` rather than `$HOME/.config` spelled out here: the answer differs
/// per platform, and being wrong about it means writing a file the reader
/// will never find.
#[must_use]
pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("obelus").join("config.toml"))
}

/// Where a project keeps settings of its own, if it keeps any.
///
/// `.obelus/config.toml`, and only that. A project keeps more than settings for
/// Obelus -- a theme of its own, whatever comes after it -- and one
/// directory holding all of it is one thing to find, to copy between
/// machines and to name in a `.gitignore`, where a dotfile per kind of thing
/// is a row of them at the top of every listing of the project.
///
/// The working directory itself, without walking up: Obelus has one answer
/// to which project it is on -- the file list walks it, the counts count it,
/// git is read from it -- and settings found by walking somewhere else would
/// be a second answer to that question.
#[must_use]
pub fn project_path(root: &Path) -> Option<PathBuf> {
    let inside = root.join(".obelus").join("config.toml");
    inside.is_file().then_some(inside)
}

/// The table a file holds, for a caller that means to lay it over something.
///
/// Three answers, like [`read_from`]'s four: there is none, here it is, or
/// it will not read. A project's file that will not read is *not* a reason to
/// stop -- Obelus goes on with the reader's own settings and says so in the
/// log -- which is why this hands back the reason rather than a config with
/// the defaults in it.
pub fn read_table(path: &Path) -> Result<Option<(toml::Table, String)>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    text.parse::<toml::Table>()
        // The text as well as the table: where a key is written is a
        // question only the text can answer, and reading the file twice
        // to ask it is two readings that can disagree.
        .map(|table| Some((table, text)))
        .map_err(|error| error.to_string())
}

/// Says where each line that did nothing is written.
///
/// Apart from the reading of the values, because the two parsers answer
/// different questions and only one of them is asked here: what a key
/// *is* comes from the table, and where it is written comes from the
/// text.
#[must_use]
pub fn placed(ignored: Vec<Ignored>, text: &str) -> Vec<Ignored> {
    if ignored.is_empty() {
        return ignored;
    }
    settled(ignored, &spans_in(text))
}

/// The same, where the caller has already asked where the keys are.
#[must_use]
pub fn settled(
    ignored: Vec<Ignored>,
    spans: &std::collections::BTreeMap<String, Span>,
) -> Vec<Ignored> {
    ignored
        .into_iter()
        .map(|one| Ignored {
            at: spans.get(&one.key).copied(),
            ..one
        })
        .collect()
}

/// Which line each key in a settings file is written on.
///
/// A second parse, by the crate that keeps the spans -- the one the values
/// come from throws them away, and it is the one every reader of a setting
/// already speaks. Two parses of a file this size is not a cost worth a
/// word; what would be worth one is a settings file parsed by two crates
/// that disagreed, and they do not: a file either is TOML or is
/// [`Reading::Unreadable`] before this is asked.
///
/// Dotted for what is under a table, which is how the two that have one
/// are named: `keys.open-file`, `agents.copilot`.
#[must_use]
pub fn spans_in(text: &str) -> std::collections::BTreeMap<String, Span> {
    let mut found = std::collections::BTreeMap::new();
    // The immutable form, which is the one that keeps the spans: making a
    // document editable throws them away, because an edited document's
    // spans would be about text that is no longer there.
    let Ok(document) = toml_edit::ImDocument::parse(text) else {
        return found;
    };
    for (key, item) in document.as_table() {
        let Some(span) = document.as_table().key(key).and_then(toml_edit::Key::span) else {
            continue;
        };
        found.insert(key.to_string(), obelus_text::span_of_bytes(text, &span));
        let Some(under) = item.as_table_like() else {
            continue;
        };
        for (inner, _) in under.iter() {
            if let Some(span) = under.key(inner).and_then(toml_edit::Key::span) {
                found.insert(
                    format!("{key}.{inner}"),
                    obelus_text::span_of_bytes(text, &span),
                );
            }
        }
    }
    found
}

/// Reads the file, or the defaults for every way it can decline.
#[must_use]
pub fn load() -> Reading {
    let Some(path) = path() else {
        return Reading::Nowhere;
    };
    read_from(&path)
}

/// What laying a table of settings over a config came to.
#[derive(Clone, Debug, Default)]
pub struct Applied {
    /// The keys the table set. A setting it named is the reader's whether
    /// or not what they wrote differs from what Obelus would have done.
    pub set: Vec<&'static str>,
    /// And the ones it named that did nothing.
    pub ignored: Vec<Ignored>,
}

/// A line of a settings file that did nothing, and why.
///
/// Facts, not words. What to *say* about one is copy, and copy belongs
/// where the rest of what Obelus says to the reader is written -- here
/// there is no reader, only a file and what could not be made of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignored {
    /// The key it is about, spelled the way the file spells it.
    pub key: String,
    /// Why nothing happened.
    pub why: Why,
    /// Where that key is written, when the file was there to be looked at.
    pub at: Option<Span>,
}

/// What was wrong with a line that did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// Obelus has no setting by that name: one that has gone, a name that
    /// has changed, a word spelled wrong.
    NoSuchSetting,
    /// There is one, and a project's file is not allowed to set it.
    NotForAProject,
    /// What an agent is set to has to be a table of that agent's own
    /// settings, and this is not one.
    NotATable,
    /// The setting takes one of a list of words, and what was written is
    /// none of them.
    NoSuchChoice(String),
}

/// The same, from a path the caller names.
#[must_use]
pub fn read_from(path: &Path) -> Reading {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // Not there yet is the ordinary case, not an error: Obelus writes
        // the file the first time something is changed.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Reading::Nothing;
        }
        Err(error) => return Reading::Unreadable(error.to_string(), None),
    };
    reading_of(&text)
}

/// What a settings file's contents come to.
///
/// Split from the file so that what the parser makes of some text can be
/// asked without one: a file is an io error and a path, and neither is
/// what is being decided here.
#[must_use]
pub fn reading_of(text: &str) -> Reading {
    match text.parse::<toml::Table>() {
        Ok(table) => {
            let (config, applied) = from_table(&table);
            let spans = spans_in(text);
            Reading::Settings {
                config: Box::new(config),
                named: applied.set,
                ignored: settled(applied.ignored, &spans),
                spans,
            }
        }
        // The parser says which bytes it gave up on, which is the whole
        // reason a reader can be shown the line rather than told a number
        // they have to go and count to.
        Err(error) => {
            let at = error
                .span()
                .map(|bytes| obelus_text::span_of_bytes(text, &bytes));
            Reading::Unreadable(error.to_string(), at)
        }
    }
}

/// What reading the settings file found.
///
/// "There is no file" and "there is a file Obelus cannot read" are different
/// answers and were the same one: both became the defaults, and a session
/// that started on the defaults writes the defaults back the first time
/// anything is changed. A file being written by another Obelus at that
/// moment, or edited by hand into something that will not parse, is then a
/// file whose contents Obelus has thrown away.
#[derive(Clone, Debug)]
pub enum Reading {
    /// This system has nowhere to keep one.
    Nowhere,
    /// There is none yet, which is where everybody starts.
    Nothing,
    /// There is one, and this is what came of it.
    ///
    /// A struct rather than a tuple because it carries four things, and
    /// the fourth is one nothing here uses: where each key is written is
    /// for whatever has to say "this line" about that file *later* -- a
    /// key binding that will not bind is decided by the keymap, long after
    /// the text has been let go of.
    Settings {
        /// What the file says. Boxed, because it is most of the size of
        /// this enum and the other three answers carry next to nothing.
        config: Box<Config>,
        /// The settings it named. One it named is the reader's whether or
        /// not what they wrote differs from what Obelus would have done.
        named: Vec<&'static str>,
        /// The lines in it that did nothing.
        ignored: Vec<Ignored>,
        /// And where every key in it is written.
        spans: std::collections::BTreeMap<String, Span>,
    },
    /// There is one and it could not be read, with what went wrong and
    /// where.
    ///
    /// Nowhere for the failures that are not about the text -- a file
    /// whose permissions forbid it has no line that is wrong with it.
    Unreadable(String, Option<Span>),
}

/// The config a file's contents describe, taking the default for anything
/// missing or of the wrong type.
#[must_use]
pub fn from_toml(text: &str) -> Config {
    let Ok(table) = text.parse::<toml::Table>() else {
        tracing::warn!("the config file is not toml, so the defaults it is");
        return Config::default();
    };
    from_table(&table).0
}

/// The same, from a table already parsed, with the keys it named.
///
/// Which keys, not only what they came to: a reader who writes a setting
/// down has said something about it even where what they said is what
/// Obelus would have done anyway, and a page that worked that out by
/// comparing with the default could not tell them from a reader who said
/// nothing at all.
fn from_table(table: &toml::Table) -> (Config, Applied) {
    let mut config = Config::default();
    let applied = apply(&mut config, table, Whose::Reader);
    (config, applied)
}

/// Which files may set the key `key`.
///
/// Asked of a *key* rather than of a [`Setting`], because three of the
/// things in the file are not rows on the settings page: the agent is
/// chosen on a page of its own, the keys are a table, and what an agent
/// starts on is a table of the agent's own words. All three are the
/// reader's alone.
///
/// A key Obelus has never heard of reaches nowhere, which costs nothing --
/// nothing reads it either way -- and means a key added to the file before
/// it is added here cannot arrive from a project.
#[must_use]
pub fn reach_of(key: &str) -> Reach {
    match key {
        "agent" | "keys" | "agents" | "remote" | "remotes" => Reach::ReaderOnly,
        _ => Setting::named(key).map_or(Reach::ReaderOnly, |setting| setting.reach),
    }
}

/// Lays a table of settings over a config, and says which keys it set.
///
/// Over, rather than into a fresh one: a project's file names the few settings
/// that project cares about, and everything it does not name is the reader's
/// and stays theirs. Reading it into a default config and taking that would
/// be a project with one line in it turning off a reader's wrapped lines.
///
/// What the project may not set is left alone, with a word in the log for
/// whoever wrote that file: from the outside it is a line that did nothing,
/// which is worth being able to find out about.
pub fn apply(config: &mut Config, table: &toml::Table, whose: Whose) -> Applied {
    let mut applied = Applied::default();
    // Collected beside rather than into `applied`, because the closure
    // below has it borrowed for as long as the settings are being read.
    let mut not_a_table: Vec<String> = Vec::new();
    let mut not_a_workflow: Option<String> = None;
    let mut allowed = |key: &'static str| {
        if reach_of(key) == Reach::Anywhere || whose == Whose::Reader {
            applied.set.push(key);
            return true;
        }
        if table.contains_key(key) {
            tracing::warn!(key, "a project may not set this, so it is left alone");
            applied.ignored.push(Ignored {
                key: key.to_string(),
                why: Why::NotForAProject,
                at: None,
            });
        }
        false
    };

    if let Some(word) = table.get("theme").and_then(toml::Value::as_str)
        && allowed("theme")
    {
        config.theme = word.to_string();
    }
    if let Some(on) = table.get("icons").and_then(toml::Value::as_bool)
        && allowed("icons")
    {
        config.icons = on;
    }
    if let Some(on) = table.get("blame_margin").and_then(toml::Value::as_bool)
        && allowed("blame_margin")
    {
        config.blame_margin = on;
    }
    if let Some(on) = table.get("wrap").and_then(toml::Value::as_bool)
        && allowed("wrap")
    {
        config.wrap = on;
    }
    if let Some(width) = table.get("tab_width").and_then(toml::Value::as_integer)
        && allowed("tab_width")
    {
        config.tab_width = usize::try_from(width).unwrap_or(obelus_text::TAB_WIDTH);
    }
    if let Some(delay) = table.get("hover_delay").and_then(toml::Value::as_integer)
        && allowed("hover_delay")
    {
        config.hover_delay = usize::try_from(delay).unwrap_or(0);
    }
    if let Some(days) = table
        .get("conversation_days")
        .and_then(toml::Value::as_integer)
        && allowed("conversation_days")
    {
        config.conversation_days = usize::try_from(days).unwrap_or(0);
    }
    if let Some(points) = table.get("font_size").and_then(toml::Value::as_integer)
        && allowed("font_size")
    {
        config.font_size = usize::try_from(points).unwrap_or(DEFAULT_FONT_SIZE);
    }
    if let Some(names) = table.get("fonts").and_then(toml::Value::as_array)
        && allowed("fonts")
    {
        // Whatever of it is a name. A line somebody typed by hand with a
        // number in the middle of it loses the number and keeps the rest,
        // which is the same thing every other setting here does with a
        // value of the wrong shape.
        config.fonts = names
            .iter()
            .filter_map(|name| name.as_str().map(str::to_string))
            .filter(|name| !name.trim().is_empty())
            .collect();
    }
    if let Some(on) = table.get("animation").and_then(toml::Value::as_bool)
        && allowed("animation")
    {
        config.animation = on;
    }
    if let Some(on) = table.get("read_only").and_then(toml::Value::as_bool)
        && allowed("read_only")
    {
        config.read_only = on;
    }
    if let Some(on) = table.get("format_on_save").and_then(toml::Value::as_bool)
        && allowed("format_on_save")
    {
        config.format_on_save = on;
    }
    if let Some(on) = table
        .get("code_actions_on_save")
        .and_then(toml::Value::as_bool)
        && allowed("code_actions_on_save")
    {
        config.code_actions_on_save = on;
    }
    if let Some(on) = table.get("inlay_hints").and_then(toml::Value::as_bool)
        && allowed("inlay_hints")
    {
        config.inlay_hints = on;
    }
    if let Some(on) = table.get("diagnostics").and_then(toml::Value::as_bool)
        && allowed("diagnostics")
    {
        config.diagnostics = on;
    }
    if let Some(on) = table.get("ignored_files").and_then(toml::Value::as_bool)
        && allowed("ignored_files")
    {
        config.ignored_files = on;
    }
    if let Some(on) = table.get("hidden_files").and_then(toml::Value::as_bool)
        && allowed("hidden_files")
    {
        config.hidden_files = on;
    }
    if let Some(on) = table.get("reopen").and_then(toml::Value::as_bool)
        && allowed("reopen")
    {
        config.reopen = on;
    }
    if let Some(on) = table.get("new_versions").and_then(toml::Value::as_bool)
        && allowed("new_versions")
    {
        config.new_versions = on;
    }
    if let Some(word) = table.get("workflow").and_then(toml::Value::as_str) {
        // Checked here rather than where it is used, because a word nothing
        // answers to was taken as no workflow at all -- the line read as
        // obeyed and the agent was told nothing. And before `allowed`,
        // which counts the line as set: one that did nothing is not the
        // file's answer. The themes are not checked this way: a theme may
        // be a file of the reader's, which only the application can look
        // for.
        if !WORKFLOWS.contains(&word) {
            tracing::warn!(word, "no workflow by this name, so the line does nothing");
            not_a_workflow = Some(word.to_string());
        } else if allowed("workflow") {
            config.workflow = word.to_string();
        }
    }
    if let Some(name) = table.get("speaks_as").and_then(toml::Value::as_str)
        && allowed("speaks_as")
    {
        config.speaks_as = speaks_as(name);
    }
    if let Some(word) = table.get("agent").and_then(toml::Value::as_str)
        && allowed("agent")
    {
        config.agent = (!word.is_empty()).then(|| word.to_string());
    }
    if let Some(keys) = table.get("keys").and_then(toml::Value::as_table)
        && allowed("keys")
    {
        // Whatever is a string. A command Obelus has never heard of and a
        // chord it cannot read are dealt with where the table is built,
        // which is the one place that knows what either of those is.
        for (name, chord) in keys {
            if let Some(chord) = chord.as_str() {
                config.keys.insert(name.clone(), chord.to_string());
            }
        }
    }
    if let Some(agents) = table.get("agents").and_then(toml::Value::as_table)
        && allowed("agents")
    {
        // A table per agent, of strings. Whether an agent by that name
        // exists, whether it still has a setting by that name, and whether
        // that setting still takes that value are three questions asked
        // where the agent is -- here there is nobody to ask, and a line
        // about an agent that is not installed is not a line with anything
        // wrong with it.
        for (agent, chosen) in agents {
            let Some(chosen) = chosen.as_table() else {
                tracing::warn!(agent, "what this agent is set to is not a table");
                not_a_table.push(agent.clone());
                continue;
            };
            for (setting, value) in chosen {
                if let Some(value) = value.as_str() {
                    config.set_agent_default(agent, setting, value);
                }
            }
        }
    }
    if let Some(word) = table.get("remote").and_then(toml::Value::as_str)
        && allowed("remote")
    {
        config.remote = (!word.is_empty()).then(|| word.to_string());
    }
    if let Some(remotes) = table.get("remotes").and_then(toml::Value::as_table)
        && allowed("remotes")
    {
        // A table per platform. Which platforms there are and which fields
        // each takes are the remote crate's to know; here a string is a
        // value and `people` is who may talk, and nothing else is read --
        // which is not the same as taken out: see `lay`.
        for (platform, kept) in remotes {
            let Some(kept) = kept.as_table() else {
                tracing::warn!(platform, "what this chat keeps is not a table");
                not_a_table.push(format!("remotes.{platform}"));
                continue;
            };
            let remote = config.remotes.entry(platform.clone()).or_default();
            for (field, value) in kept {
                if let Some(value) = value.as_str() {
                    remote.values.insert(field.clone(), value.to_string());
                }
            }
            for person in kept
                .get("people")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
            {
                let said = |key: &str| person.get(key).and_then(toml::Value::as_str);
                if let Some(id) = said("id").filter(|id| !id.is_empty()) {
                    remote.people.push(Person {
                        id: id.to_string(),
                        name: said("name").unwrap_or(id).to_string(),
                    });
                }
            }
            if remote.is_empty() {
                config.remotes.remove(platform);
            }
        }
    }

    // And a word for the lines Obelus walked past. A key it has never heard
    // of -- a setting that has gone, a name that has changed, a word spelled
    // wrong -- is read, ignored, and from the outside looks exactly like one
    // that was obeyed. The line above says as much when a project oversteps,
    // for the same reason: from the outside it is a line that did nothing,
    // and that is worth being able to find out about.
    for key in table.keys() {
        if !known(key) {
            tracing::warn!(key, "no setting by this name, so the line does nothing");
            applied.ignored.push(Ignored {
                key: key.clone(),
                why: Why::NoSuchSetting,
                at: None,
            });
        }
    }
    // Named under the table they are in, which is how [`spans_in`] names
    // them and how a reader would say where to look.
    for agent in not_a_table {
        applied.ignored.push(Ignored {
            // The remotes' arrive named in full; an agent's by its name.
            key: match agent.starts_with("remotes.") {
                true => agent,
                false => format!("agents.{agent}"),
            },
            why: Why::NotATable,
            at: None,
        });
    }
    if let Some(word) = not_a_workflow {
        applied.ignored.push(Ignored {
            key: "workflow".to_string(),
            why: Why::NoSuchChoice(word),
            at: None,
        });
    }
    applied
}

/// Whether a key in a settings file names a setting Obelus has.
///
/// The three that are not rows in [`ALL`] count: they are settings a reader
/// writes by hand or sets somewhere else on the page, not settings Obelus
/// has stopped having.
fn known(key: &str) -> bool {
    matches!(key, "agent" | "keys" | "agents" | "remote" | "remotes")
        || Setting::named(key).is_some()
}

/// The file's contents for a config, with nothing else in it.
#[must_use]
pub fn to_toml(config: &Config) -> String {
    over("", config)
}

/// The file's contents for a config, laid over a file that already exists.
///
/// Edited rather than rewritten, the way a project's file is. Obelus used to
/// write its own file whole on the grounds that Obelus wrote all of it,
/// which is not true: readers open it and put lines in by hand. Writing it
/// whole took out everything Obelus did not recognise -- a setting from a
/// newer version, a key that has been renamed since, a line with a typo in
/// it -- silently, on the next switch they flipped. A program that will not
/// edit a file it is reading should not quietly delete from one it owns.
///
/// A file that is not toml at all is started again from nothing: there is
/// no document to lay anything over, and Obelus has already said so
/// elsewhere.
///
/// Only what differs from the default is written, and a line that has come
/// back to the default is taken out. A file with every setting in it
/// freezes the defaults of the version that wrote it: the glyphs went off
/// by default, and every reader who had ever flipped anything kept them on,
/// because Obelus had written `icons = true` down for them the first time
/// it saved. The same rule the keys and the agents already kept -- a line
/// is something the reader said, not something Obelus was thinking about.
#[must_use]
pub fn over(existing: &str, config: &Config) -> String {
    lay(existing, config, false)
}

/// What a settings file starts as: every setting, at its default, in a
/// comment.
///
/// Because a file of only what differs is empty until the reader has said
/// something, and an empty file opened to change a setting by hand is a
/// reader told nothing about what there is to change. In comments, so none
/// of it is said: a default that moves still reaches them. And the header
/// says that a line only counts once it says something else, because one
/// taken out of its comment as it stands is a default, and the next save
/// takes it out.
fn template() -> String {
    let every = lay("", &Config::default(), true);
    let mut said = String::from(
        "# Every setting, at its default. To set one, take it out of its comment\n# and change it: a line that says the default is taken out when Obelus\n# next saves, so that a default which changes reaches you.\n",
    );
    for line in every.lines() {
        said.push_str("# ");
        said.push_str(line);
        said.push('\n');
    }
    said
}

/// [`over`], or with `every` every setting whatever its value -- which is
/// only ever what [`template`] comments out.
fn lay(existing: &str, config: &Config, every: bool) -> String {
    let mut document = existing
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_default();
    let default = Config::default();
    let mut put = |key: &str, differs: bool, value: toml_edit::Item| {
        if every || differs {
            document[key] = value;
        } else {
            document.remove(key);
        }
    };
    put(
        "theme",
        config.theme != default.theme,
        toml_edit::value(config.theme.clone()),
    );
    put(
        "icons",
        config.icons != default.icons,
        toml_edit::value(config.icons),
    );
    put(
        "blame_margin",
        config.blame_margin != default.blame_margin,
        toml_edit::value(config.blame_margin),
    );
    put(
        "wrap",
        config.wrap != default.wrap,
        toml_edit::value(config.wrap),
    );
    put(
        "tab_width",
        config.tab_width != default.tab_width,
        toml_edit::value(i64::try_from(config.tab_width).unwrap_or(4)),
    );
    put(
        "fonts",
        config.fonts != default.fonts,
        toml_edit::value(config.fonts.iter().collect::<toml_edit::Array>()),
    );
    put(
        "font_size",
        config.font_size != default.font_size,
        toml_edit::value(i64::try_from(config.font_size).unwrap_or(14)),
    );
    put(
        "hover_delay",
        config.hover_delay != default.hover_delay,
        toml_edit::value(i64::try_from(config.hover_delay).unwrap_or(400)),
    );
    put(
        "conversation_days",
        config.conversation_days != default.conversation_days,
        toml_edit::value(i64::try_from(config.conversation_days).unwrap_or(30)),
    );
    put(
        "animation",
        config.animation != default.animation,
        toml_edit::value(config.animation),
    );
    put(
        "read_only",
        config.read_only != default.read_only,
        toml_edit::value(config.read_only),
    );
    put(
        "format_on_save",
        config.format_on_save != default.format_on_save,
        toml_edit::value(config.format_on_save),
    );
    put(
        "code_actions_on_save",
        config.code_actions_on_save != default.code_actions_on_save,
        toml_edit::value(config.code_actions_on_save),
    );
    put(
        "inlay_hints",
        config.inlay_hints != default.inlay_hints,
        toml_edit::value(config.inlay_hints),
    );
    put(
        "diagnostics",
        config.diagnostics != default.diagnostics,
        toml_edit::value(config.diagnostics),
    );
    put(
        "ignored_files",
        config.ignored_files != default.ignored_files,
        toml_edit::value(config.ignored_files),
    );
    put(
        "hidden_files",
        config.hidden_files != default.hidden_files,
        toml_edit::value(config.hidden_files),
    );
    put(
        "reopen",
        config.reopen != default.reopen,
        toml_edit::value(config.reopen),
    );
    put(
        "new_versions",
        config.new_versions != default.new_versions,
        toml_edit::value(config.new_versions),
    );
    put(
        "workflow",
        config.workflow != default.workflow,
        toml_edit::value(config.workflow.clone()),
    );
    put(
        "speaks_as",
        config.speaks_as != default.speaks_as,
        toml_edit::value(config.speaks_as.clone()),
    );
    put(
        "agent",
        config.agent != default.agent,
        toml_edit::value(config.agent.clone().unwrap_or_default()),
    );
    put(
        "remote",
        config.remote != default.remote,
        toml_edit::value(config.remote.clone().unwrap_or_default()),
    );
    // Only while the reader has moved something: an empty table in the file
    // says Obelus was thinking about keys, which it was not.
    if config.keys.is_empty() {
        document.remove("keys");
    } else {
        let mut keys = toml_edit::Table::new();
        for (name, chord) in &config.keys {
            keys[name] = toml_edit::value(chord.clone());
        }
        document["keys"] = toml_edit::Item::Table(keys);
    }
    // The same rule, an agent at a time: a table for an agent the reader
    // has said nothing about is Obelus writing down that it thought about
    // it. `unset_agent_default` already drops one that empties; this is
    // the same answer for a config that arrived from anywhere else.
    let agents: Vec<(&String, &std::collections::BTreeMap<String, String>)> = config
        .agents
        .iter()
        .filter(|(_, chosen)| !chosen.is_empty())
        .collect();
    if agents.is_empty() {
        document.remove("agents");
    } else {
        let mut table = toml_edit::Table::new();
        // Implicit, so the file says `[agents.claude-code]` rather than an
        // empty `[agents]` and a table under it: the reader opening this
        // file is looking for the agent's name.
        table.set_implicit(true);
        for (agent, chosen) in agents {
            let mut settings = toml_edit::Table::new();
            for (setting, value) in chosen {
                settings[setting] = toml_edit::value(value.clone());
            }
            table[agent] = toml_edit::Item::Table(settings);
        }
        document["agents"] = toml_edit::Item::Table(table);
    }
    lay_remotes(&mut document, &config.remotes);
    document.to_string()
}

/// What each chat keeps, laid over the tables the file already has.
///
/// Edited in place rather than built afresh, the way the file itself is: a
/// platform's table may hold a line this version does not read -- a field
/// from a newer one, a key the reader wrote by hand -- and rebuilding it
/// from what was read would take that out. What *is* taken out is what the
/// config no longer has: a value the reader removed, the people when there
/// are none left, and a platform's table when nothing is left in it.
///
/// A `remotes` entry that is not a table was never read, so it is not the
/// config's to remove either; it stays where the reader put it.
fn lay_remotes(
    document: &mut toml_edit::DocumentMut,
    remotes: &std::collections::BTreeMap<String, Remote>,
) {
    if !document.contains_key("remotes") {
        if remotes.values().all(Remote::is_empty) {
            return;
        }
        let mut table = toml_edit::Table::new();
        // Implicit, so the file says `[remotes.slack]`: what a reader
        // opening the file looks for is the platform's name.
        table.set_implicit(true);
        document["remotes"] = toml_edit::Item::Table(table);
    }
    let Some(table) = document["remotes"].as_table_like_mut() else {
        return;
    };
    // Every platform the file has a table for as well as every one the
    // config has: one the reader emptied is still a table here, and may
    // still hold a line this version does not read.
    let mut platforms: Vec<String> = table
        .iter()
        .filter(|(_, item)| item.is_table_like())
        .map(|(platform, _)| platform.to_string())
        .collect();
    platforms.extend(
        remotes
            .iter()
            .filter(|(platform, remote)| !remote.is_empty() && !table.contains_key(platform))
            .map(|(platform, _)| platform.clone()),
    );
    let nothing = Remote::default();
    for platform in platforms {
        let remote = remotes.get(&platform).unwrap_or(&nothing);
        if !table.contains_key(&platform) {
            table.insert(&platform, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let Some(kept) = table
            .get_mut(&platform)
            .and_then(toml_edit::Item::as_table_like_mut)
        else {
            continue;
        };
        let unsaid: Vec<String> = kept
            .iter()
            .filter(|(field, item)| item.as_str().is_some() && !remote.values.contains_key(*field))
            .map(|(field, _)| field.to_string())
            .collect();
        for field in unsaid {
            kept.remove(&field);
        }
        for (field, value) in &remote.values {
            kept.insert(field, toml_edit::value(value.clone()));
        }
        if remote.people.is_empty() {
            kept.remove("people");
        } else {
            let people: toml_edit::Array = remote
                .people
                .iter()
                .map(|person| {
                    let mut said = toml_edit::InlineTable::new();
                    said.insert("id", person.id.clone().into());
                    said.insert("name", person.name.clone().into());
                    toml_edit::Value::InlineTable(said)
                })
                .collect();
            kept.insert("people", toml_edit::value(people));
        }
        if kept.is_empty() {
            table.remove(&platform);
        }
    }
    if table.is_empty() {
        document.remove("remotes");
    }
}

/// What a path really names, following any links.
///
/// A reader who keeps their settings in git links the place Obelus looks at
/// the file in their repository, which makes the difference between the two
/// paths matter twice. Writing has to go *through* the link, because a
/// rename replaces what the name refers to -- the link would become an
/// ordinary file on the first setting they changed, and every change after
/// that would go somewhere the repository never sees, silently. And
/// watching has to follow it, because what a `git pull` rewrites is the
/// file at the far end: a watch on the link's own directory hears nothing,
/// so settings arriving from another machine would sit on disk until Obelus
/// was next started.
///
/// A path that is not there yet cannot be resolved, and is its own answer:
/// there is no link to follow.
#[must_use]
pub fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The name an agent is to call itself, from what somebody wrote.
///
/// A blank one is the default rather than nothing: an agent told to call
/// itself "" is told nothing it can follow, and a reader who cleared the
/// line has said they want no name of their own.
fn speaks_as(name: &str) -> String {
    match name.trim() {
        "" => DEFAULT_SPEAKS_AS.to_string(),
        name => name.to_string(),
    }
}

/// Sets or removes one key in a project's own settings file.
///
/// Edited rather than rewritten, like the reader's own ([`over`]). A
/// project's is written by hand and committed, so it has comments in it, an
/// order somebody chose, and possibly keys this version has never heard of.
/// A round trip through a `toml::Table` would throw all three away on the
/// first switch a reader flipped.
///
/// `None` takes the key out, which is how a setting stops being the project's
/// and goes back to being the reader's.
///
/// The file need not exist: setting the first key makes it, which is the
/// ordinary way a project acquires one.
pub fn write_project(path: &Path, key: &str, value: Option<&Value>) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    match value {
        Some(Value::Switch(on)) => document[key] = toml_edit::value(*on),
        Some(Value::Choice(word) | Value::Text(word)) => {
            document[key] = toml_edit::value(word.clone());
        }
        Some(Value::Count(count)) => {
            document[key] = toml_edit::value(i64::try_from(*count).unwrap_or(0));
        }
        Some(Value::Names(names)) => {
            document[key] = toml_edit::value(names.iter().collect::<toml_edit::Array>());
        }
        None => {
            // What was written above the key goes with it, except for
            // whatever is above the last blank line: a comment touching a
            // key is about that key, and anything an empty line away from
            // it is the file's own -- a heading, or a note about the lot.
            // Removing the first key of a file took its heading with it.
            let above = document
                .as_table()
                .get_key_value(key)
                .and_then(|(key, _)| key.leaf_decor().prefix().cloned())
                .and_then(|prefix| prefix.as_str().map(str::to_string));
            document.remove(key);
            let next = document
                .as_table()
                .iter()
                .next()
                .map(|(key, _)| key.to_string());
            if let Some(above) = above
                && let Some(end) = above.rfind("\n\n")
                && let Some(next) = next
                && let Some(mut next) = document.as_table_mut().key_mut(&next)
            {
                let kept = &above[..end + 2];
                let already = next
                    .leaf_decor()
                    .prefix()
                    .and_then(|prefix| prefix.as_str().map(str::to_string))
                    .unwrap_or_default();
                next.leaf_decor_mut().set_prefix(format!("{kept}{already}"));
            }
        }
    }

    let Some(directory) = path.parent() else {
        return std::fs::write(path, document.to_string());
    };
    // The settings' own directory and nothing above it. What is above it is
    // the project, and a project that has gone is not one to make again by
    // writing a setting into it: making the whole path did, and a tree the
    // reader had deleted came back with one directory and one file in it.
    match std::fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    // Beside it and renamed over it, for the reason the reader's own file is
    // written that way: another Obelus on this project may be reading it at
    // this moment, and a plain write truncates first.
    // This process's own name beside it and not one every Obelus shares:
    // two writing at once into one shared name truncate each other's
    // half-written file, and the first rename takes the other's away.
    let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
    let written =
        std::fs::write(&beside, document.to_string()).and_then(|()| std::fs::rename(&beside, path));
    if written.is_err() {
        // And not left behind: the name is this process's own, so nobody
        // else will ever write over it, and a failed rename would leave one
        // more of them for every Obelus that failed.
        let _ = std::fs::remove_file(&beside);
    }
    written
}

/// Where a project's settings *would* go, for a project that has none yet.
///
/// The directory, always. A project keeps more than settings for Obelus -- a
/// theme of its own, whatever comes after it -- and one directory holding
/// all of it is one thing to find, to copy between machines and to put in a
/// `.gitignore`, where a dotfile per kind of thing is a row of them at the
/// top of every listing of the project.
#[must_use]
pub fn project_path_for(root: &Path) -> PathBuf {
    root.join(".obelus").join("config.toml")
}

/// Writes a config to a path, making its directory if it is not there.
///
/// The path is passed in rather than looked up here, so that nothing can
/// write to the reader's real file by accident: a test, or a probe run while
/// working on Obelus, has to say where it is writing.
pub fn save_to(path: &Path, config: &Config) -> std::io::Result<()> {
    // Through the link rather than over it: a rename replaces what the name
    // refers to, and where the settings are kept in a dotfiles repository
    // the name refers to a link.
    let resolved = resolved(path);
    let path = resolved.as_path();
    // What is in it already, so that whatever Obelus does not recognise
    // stays there. The template where there is nothing: the first write
    // makes the file, and makes it saying what there is to set. A file the
    // reader emptied is not nothing, and stays as empty as they left it.
    let existing = match std::fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => template(),
        Err(_) => String::new(),
    };
    let Some(directory) = path.parent() else {
        return std::fs::write(path, over(&existing, config));
    };
    std::fs::create_dir_all(directory)?;
    // Written beside it and renamed over it, because another Obelus may be
    // reading this file at this moment: a plain write truncates first, and
    // a reader landing in that gap sees an empty file, takes it for "no
    // settings", and writes its defaults over everything the reader has.
    // A rename within one directory is the one filesystem operation that
    // has no such gap.
    //
    // Beside it rather than in a temporary directory: rename is only atomic
    // within a filesystem, and the only directory known to be on the same
    // one is this one.
    // This process's own name beside it and not one every Obelus shares:
    // two writing at once into one shared name truncate each other's
    // half-written file, and the first rename takes the other's away.
    let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
    let written = std::fs::write(&beside, over(&existing, config))
        .and_then(|()| std::fs::rename(&beside, path));
    if written.is_err() {
        // And not left behind: the name is this process's own, so nobody
        // else will ever write over it, and a failed rename would leave one
        // more of them for every Obelus that failed.
        let _ = std::fs::remove_file(&beside);
    }
    written
}

#[cfg(test)]
mod tests {
    // Both of the tests that save a file are unix's: one makes a symbolic
    // link and the other reads an inode. Imported beside them so a Windows
    // build is not warned about a name nothing there uses.
    #[cfg(unix)]
    use super::save_to;
    use super::{
        Config, Value, Whose, apply, from_toml, known, over, project_path_for, to_toml,
        write_project,
    };

    /// A setting written into a project that has gone does not make the
    /// project again.
    ///
    /// The settings' own directory is made where it is missing, which is
    /// how a project first gets any; everything above it is the project,
    /// and is the reader's to make or delete.
    ///
    /// Broken deliberately by making the whole path again, which is what
    /// this did: the tree comes back holding `.obelus/config.toml`.
    #[test]
    fn a_setting_written_into_a_project_that_has_gone_does_not_make_it_again() {
        let root = std::env::temp_dir().join(format!("obelus-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the project");
        let path = project_path_for(&root);
        let theme = Value::Choice("light".to_string());
        write_project(&path, "theme", Some(&theme)).expect("a project's first setting");
        assert!(path.is_file(), "a project's first setting made no file");

        std::fs::remove_dir_all(&root).expect("the project going");
        assert!(
            write_project(&path, "theme", Some(&theme)).is_err(),
            "a setting was written into a project that has gone"
        );
        assert!(!root.exists(), "the project was made again");
    }

    /// A path that is a link is written *through*, not over.
    ///
    /// Which is how anybody keeps their settings in git: the file lives in a
    /// dotfiles repository and the place Obelus looks is a link to it. The
    /// atomic rename replaces what the name refers to, and the name refers
    /// to the link -- so saving turned the link into an ordinary file and
    /// the repository stopped hearing about changes, with nothing on screen
    /// saying so.
    ///
    /// Broken deliberately by taking the `canonicalize` out of `save_to`:
    /// the link came back an ordinary file and the file in the repository
    /// still held the old theme.
    #[cfg(unix)]
    #[test]
    fn saving_through_a_link_keeps_the_link_and_writes_what_it_points_at() {
        let directory = std::env::temp_dir().join(format!("obelus-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let repository = directory.join("dotfiles");
        let config_home = directory.join("config");
        std::fs::create_dir_all(&repository).expect("a directory");
        std::fs::create_dir_all(&config_home).expect("a directory");

        let real = repository.join("config.toml");
        std::fs::write(&real, "theme = \"dark\"\n").expect("the file");
        let linked = config_home.join("config.toml");
        std::os::unix::fs::symlink(&real, &linked).expect("a link");

        let config = Config {
            theme: "light".to_string(),
            ..Config::default()
        };
        save_to(&linked, &config).expect("saving");

        assert!(
            std::fs::symlink_metadata(&linked)
                .expect("the link")
                .file_type()
                .is_symlink(),
            "saving replaced the link with a file of its own"
        );
        let written = std::fs::read_to_string(&real).expect("the file it points at");
        assert!(
            written.contains("light"),
            "the file in the repository did not get the change: {written:?}"
        );
        // And nothing left beside either of them, under whatever name it
        // was written through -- which has this process's number in it,
        // so asking after one name is asking after nothing.
        for directory in [&config_home, &repository] {
            let beside: Vec<_> = std::fs::read_dir(directory)
                .expect("the directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .filter(|name| name != "config.toml")
                .collect();
            assert!(beside.is_empty(), "it left {beside:?} behind");
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Saving replaces the file rather than rewriting it where it lies.
    ///
    /// Which is what makes it safe for another Obelus to be reading it at
    /// that moment: a write in place truncates first, and a reader landing
    /// in that gap sees an empty file, takes it for "no settings", and
    /// writes its own defaults over everything the reader had. A rename
    /// within one directory has no such gap -- and the file being a new
    /// one afterwards is how that shows from the outside.
    #[cfg(unix)]
    #[test]
    fn saving_replaces_the_file_rather_than_emptying_it_first() {
        use std::os::unix::fs::MetadataExt as _;

        let directory = std::env::temp_dir().join(format!("obelus-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");
        let path = directory.join("config.toml");

        let mut config = Config::default();
        config.set("theme", &Value::Choice("light".to_string()));
        save_to(&path, &config).expect("saving");
        let first = std::fs::metadata(&path).expect("the file").ino();

        config.set("theme", &Value::Choice("dark".to_string()));
        save_to(&path, &config).expect("saving again");
        let second = std::fs::metadata(&path).expect("the file").ino();

        assert_ne!(
            first, second,
            "the settings were rewritten where they lay, which another Obelus can read half of"
        );
        assert_eq!(
            from_toml(&std::fs::read_to_string(&path).expect("the file")).theme,
            "dark",
            "the new settings are not what is in the file"
        );
        // And nothing left beside it: a file called `config.toml.writing`
        // in a reader's config directory is Obelus's mess, not theirs.
        let beside: Vec<_> = std::fs::read_dir(&directory)
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "config.toml")
            .collect();
        assert!(beside.is_empty(), "it left {beside:?} behind");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A key Obelus has never heard of is a line that did nothing, and the
    /// only way to find out from the outside is to be told.
    #[test]
    fn a_key_obelus_does_not_know_is_reported() {
        assert!(known("theme"), "a setting on the page is not known");
        assert!(known("agent"), "a setting written by hand is not known");
        assert!(known("keys"), "the key table is not known");
        assert!(
            !known("blame"),
            "a name Obelus has stopped using is still known"
        );
        assert!(!known("prevlew"), "a name spelled wrong is known");
    }

    /// A project may choose its own workflow, because whether its changes go
    /// through pull requests is the project's rule rather than the reader's.
    ///
    /// Broken deliberately by giving `workflow` `Reach::ReaderOnly`: the
    /// project's line is ignored and the default `feature-branch` stands.
    /// Which is why the line chooses `none`: choosing the default would
    /// pass with the line ignored.
    #[test]
    fn a_project_may_choose_its_workflow() {
        let mut config = Config::default();
        let table: toml::Table = "workflow = \"none\"".parse().expect("toml");
        let applied = apply(&mut config, &table, Whose::Project);
        assert_eq!(config.workflow, "none");
        assert!(applied.ignored.is_empty(), "{:?}", applied.ignored);
    }

    /// A workflow nothing answers to does nothing, says so, and is not
    /// counted among what the file set -- a project's page would otherwise
    /// show the reader's value as the project's.
    ///
    /// Broken deliberately by asking `allowed` before the word is checked:
    /// `workflow` is counted as set.
    #[test]
    fn a_workflow_nothing_answers_to_does_nothing() {
        let mut config = Config::default();
        let table: toml::Table = "workflow = \"feature_branch\"".parse().expect("toml");
        let applied = apply(&mut config, &table, Whose::Project);
        assert_eq!(config.workflow, Config::default().workflow);
        assert!(!applied.set.contains(&"workflow"), "{:?}", applied.set);
        assert_eq!(
            applied.ignored,
            vec![super::Ignored {
                key: "workflow".to_string(),
                why: super::Why::NoSuchChoice("feature_branch".to_string()),
                at: None,
            }]
        );
    }

    /// Written and read back is the same config: the file is the only place
    /// a setting survives, so anything that does not survive the round trip
    /// is a setting the reader has to set twice.
    #[test]
    fn a_config_survives_the_file() {
        let config = Config {
            theme: "light".to_string(),
            icons: false,
            blame_margin: false,
            wrap: true,
            animation: false,
            tab_width: 8,
            fonts: vec!["JetBrains Mono".to_string(), "Noto Sans CJK SC".to_string()],
            font_size: 18,
            hover_delay: 800,
            conversation_days: 90,
            read_only: true,
            format_on_save: true,
            code_actions_on_save: true,
            inlay_hints: true,
            diagnostics: true,
            ignored_files: true,
            hidden_files: true,
            reopen: false,
            new_versions: false,
            workflow: "none".to_string(),
            speaks_as: "Ada".to_string(),
            agent: Some("claude-acp".to_string()),
            // A key moved and a key taken away: both are decisions, and
            // both have to survive the file or the reader makes them again
            // every time Obelus starts.
            keys: [
                ("open-file".to_string(), "alt+o".to_string()),
                ("close-file".to_string(), String::new()),
            ]
            .into_iter()
            .collect(),
            // Two agents, because the file keeps a table each and one of
            // them would not say whether the name on it is the agent's.
            agents: [
                (
                    "claude-acp".to_string(),
                    [("mode".to_string(), "accept-edits".to_string())]
                        .into_iter()
                        .collect(),
                ),
                (
                    "gemini-cli".to_string(),
                    [("model".to_string(), "flash".to_string())]
                        .into_iter()
                        .collect(),
                ),
            ]
            .into_iter()
            .collect(),
            remote: Some("slack".to_string()),
            // Two platforms, the one in use and one set up before it: a
            // switch to the other and back must not cost the first.
            remotes: [
                (
                    "slack".to_string(),
                    super::Remote {
                        values: std::collections::BTreeMap::new(),
                        people: vec![super::Person {
                            id: "U04ABCDEF".to_string(),
                            name: "Sunli".to_string(),
                        }],
                    },
                ),
                (
                    "feishu".to_string(),
                    super::Remote {
                        values: [("domain".to_string(), "lark".to_string())]
                            .into_iter()
                            .collect(),
                        people: Vec::new(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(from_toml(&to_toml(&config)), config);
        assert_eq!(
            from_toml(&to_toml(&Config::default())),
            Config::default(),
            "the defaults do not survive their own file"
        );
    }

    /// A file with a typo in it gives Obelus, not an error message where the
    /// editor was: every field falls back on its own.
    #[test]
    fn nonsense_in_the_file_is_the_default() {
        assert_eq!(from_toml("this is not toml at all ["), Config::default());
        assert_eq!(from_toml(""), Config::default());
        // The wrong type for a field leaves that field alone and keeps the
        // rest of the file.
        let mixed = from_toml("theme = 7\nicons = false\n");
        assert_eq!(mixed.theme, Config::default().theme);
        assert!(!mixed.icons);
        // And a key Obelus does not know is not an error either: an older
        // Obelus reading a newer file should still start.
        assert_eq!(from_toml("nonsense = true"), Config::default());
    }

    /// A setting takes only the shape of value its control produces.
    #[test]
    fn a_setting_ignores_the_wrong_shape_of_value() {
        let mut config = Config::default();
        config.set("icons", &Value::Choice("dark".to_string()));
        assert_eq!(config, Config::default(), "a word set a switch");
        config.set("theme", &Value::Switch(false));
        assert_eq!(config, Config::default(), "a switch set a word");

        config.set("icons", &Value::Switch(false));
        assert_eq!(config.value_of("icons"), Some(Value::Switch(false)));
    }

    /// Every setting in the table can be read and written, which is what the
    /// view assumes: a row it cannot read has nothing to draw.
    #[test]
    fn every_setting_in_the_table_is_a_field() {
        let config = Config::default();
        for setting in super::ALL {
            let value = config
                .value_of(setting.key)
                .unwrap_or_else(|| panic!("{} cannot be read", setting.key));
            match (setting.kind, &value) {
                (super::Kind::Switch, Value::Switch(_)) => {}
                (super::Kind::Choice(choices), Value::Choice(word)) => assert!(
                    choices.contains(&word.as_str()),
                    "{} defaults to {word:?}, which is not one of its choices",
                    setting.key
                ),
                (super::Kind::Count(counts), Value::Count(count)) => {
                    assert!(
                        counts.contains(&count.to_string().as_str()),
                        "{} defaults to {count}, which is not one of its choices",
                        setting.key
                    );
                    // The list is spelled, because the control that offers
                    // it offers words. Every one of them has to be a number
                    // or the reader picks something that parses to nothing.
                    for offered in counts {
                        assert!(
                            offered.parse::<usize>().is_ok(),
                            "{} offers {offered:?}, which is not a number",
                            setting.key
                        );
                    }
                }
                // Nothing to check against a list of choices: what may go
                // in a list of names comes from the machine rather than
                // from anything Obelus ships. What is checked is that it
                // starts empty -- a default of Obelus's own would be a
                // font name Obelus guessed at.
                (super::Kind::Names, Value::Names(names)) => assert!(
                    names.is_empty(),
                    "{} defaults to {names:?}, which is a guess about somebody's machine",
                    setting.key
                ),
                (super::Kind::Text, Value::Text(text)) => assert!(
                    !text.trim().is_empty(),
                    "{} defaults to nothing, which says nothing",
                    setting.key
                ),
                (kind, value) => {
                    panic!("{} is a {kind:?} holding a {value:?}", setting.key)
                }
            }
        }
    }

    /// What an agent is to start on survives the file, agent by agent.
    ///
    /// The file is where this lives between sittings, and it is a table of
    /// tables -- neither of which the flat settings above it exercise. A
    /// round trip is the whole of the claim: what was said is what comes
    /// back, under the agent it was said about.
    ///
    /// Broken deliberately by writing the inner tables under one shared
    /// name rather than the agent's: both agents came back with the
    /// second one's settings.
    #[test]
    fn what_an_agent_starts_on_survives_the_file() {
        let mut config = Config::default();
        config.set_agent_default("claude-code", "mode", "accept-edits");
        config.set_agent_default("claude-code", "thinking", "high");
        config.set_agent_default("gemini-cli", "mode", "plan");

        let written = to_toml(&config);
        assert!(
            written.contains("[agents.claude-code]"),
            "the agent is not its own table: {written}"
        );
        let read = from_toml(&written);
        assert_eq!(
            read.agent_default("claude-code", "mode"),
            Some("accept-edits")
        );
        assert_eq!(read.agent_default("claude-code", "thinking"), Some("high"));
        assert_eq!(read.agent_default("gemini-cli", "mode"), Some("plan"));
        assert_eq!(
            read.agent_default("gemini-cli", "thinking"),
            None,
            "an agent was given what another agent was set to"
        );
    }

    /// A project may not say what an agent starts on.
    ///
    /// The reason `agent` is the reader's alone, twice over: a downloaded
    /// project that could write this one could say that the agent it starts
    /// may edit files without asking.
    ///
    /// Broken deliberately by taking "agents" out of `reach_of` *and*
    /// making the fallback for an unknown key `Anywhere`: the project's line
    /// was obeyed and the mode came back as the project's. Both, because
    /// taking out the named arm alone changes nothing -- the fallback
    /// already refuses a key with no row on the page. The arm stays all
    /// the same: the rule is that a project may not set this, and a rule that
    /// holds only because Obelus happens to have no setting by that name
    /// is a rule nobody has written down.
    #[test]
    fn a_tree_may_not_say_what_an_agent_starts_on() {
        let table = r#"
            [agents.claude-code]
            mode = "yolo"
        "#
        .parse::<toml::Table>()
        .expect("a table");

        let mut readers = Config::default();
        readers.set_agent_default("claude-code", "mode", "plan");
        let mut config = readers.clone();
        apply(&mut config, &table, Whose::Project);
        assert_eq!(
            config.agent_default("claude-code", "mode"),
            Some("plan"),
            "a project set what the agent starts on"
        );

        // And the reader's own file is obeyed, so the test is about who
        // wrote it rather than about the line being unreadable.
        let mut config = readers;
        apply(&mut config, &table, Whose::Reader);
        assert_eq!(config.agent_default("claude-code", "mode"), Some("yolo"));
    }

    /// What a chat keeps is the reader's alone.
    ///
    /// It says who may talk to this machine, so a project that could write
    /// it could hand a stranger the agent.
    ///
    /// Broken deliberately by taking "remotes" and "remote" out of
    /// `reach_of` and making the fallback `Anywhere`: the stranger came
    /// back as one of the people, and the chat came back switched on.
    #[test]
    fn a_tree_may_not_say_who_may_talk_to_this_machine() {
        let table = r#"
            remote = "slack"
            [remotes.slack]
            people = [{ id = "U0STRANGER", name = "Somebody" }]
        "#
        .parse::<toml::Table>()
        .expect("a table");

        let mut config = Config::default();
        apply(&mut config, &table, Whose::Project);
        assert_eq!(config.remote, None, "a project switched a chat on");
        assert!(
            config.remote_of("slack").is_none(),
            "a project said who may talk to this machine"
        );

        let mut config = Config::default();
        apply(&mut config, &table, Whose::Reader);
        assert_eq!(config.remote.as_deref(), Some("slack"));
        assert_eq!(
            config.remote_of("slack").map(|remote| remote.people.len()),
            Some(1)
        );
    }

    /// A line in a chat's table this version does not read survives a save,
    /// and one the reader took out goes.
    ///
    /// The table is edited in place for the first reason, and has to notice
    /// the second for the same one: rebuilding it from what was read would
    /// lose the line it does not know, and laying over it without looking
    /// would keep the line the reader removed.
    ///
    /// Broken deliberately by rebuilding the platform's table from the
    /// config in `lay_remotes`: the line from a newer version went. And by
    /// taking out the loop that removes what is unsaid: the removed value
    /// stayed.
    #[test]
    fn a_chats_table_keeps_what_it_does_not_read_and_loses_what_was_removed() {
        let existing = "[remotes.feishu]\ndomain = \"lark\"\napp_id = \"cli_1\"\nlater = 3\n";
        let mut config = from_toml(existing);
        assert_eq!(config.remote_value("feishu", "app_id"), Some("cli_1"));
        config.set_remote_value("feishu", "app_id", None);
        let written = over(existing, &config);
        assert!(
            written.contains("later = 3"),
            "a line it does not read went: {written}"
        );
        assert!(
            !written.contains("app_id"),
            "a value the reader removed stayed: {written}"
        );
        assert!(written.contains("domain = \"lark\""), "{written}");

        config.set_remote_value("feishu", "domain", None);
        let written = over(&written, &config);
        assert!(
            written.contains("later = 3"),
            "the platform's table went with a line still in it: {written}"
        );
    }

    /// Somebody paired twice is one person, under their newer name, and the
    /// last one out takes the table with them.
    ///
    /// Broken deliberately by pushing without looking for the id: the list
    /// held them twice.
    #[test]
    fn a_person_is_known_by_their_id() {
        let mut config = Config::default();
        let person = |name: &str| super::Person {
            id: "U04ABCDEF".to_string(),
            name: name.to_string(),
        };
        config.add_person("slack", person("sunli"));
        config.add_person("slack", person("Sunli"));
        let people = &config.remote_of("slack").expect("a table").people;
        assert_eq!(
            people.len(),
            1,
            "one person, paired twice, is two: {people:?}"
        );
        assert_eq!(people[0].name, "Sunli");

        config.remove_person("slack", "U04ABCDEF");
        assert!(config.remote_of("slack").is_none(), "an empty table stayed");
        assert!(
            !to_toml(&config).contains("remotes"),
            "{}",
            to_toml(&config)
        );
    }

    /// Unsetting the last of an agent's settings takes its table out.
    ///
    /// The same rule the keys follow: a table with nothing in it says
    /// Obelus was thinking about that agent, which after this it is not --
    /// and a reader who opens the file looking for what they undid would
    /// find the heading still there.
    ///
    /// Broken deliberately by leaving the empty map in `agents`: the file
    /// kept `[agents.claude-code]` with nothing under it.
    #[test]
    fn unsetting_the_last_of_an_agents_settings_takes_its_table_out() {
        let mut config = Config::default();
        config.set_agent_default("claude-code", "mode", "plan");
        config.unset_agent_default("claude-code", "mode");
        assert!(config.agents.is_empty(), "the agent kept an empty table");

        let written = over("[agents.claude-code]\nmode = \"plan\"\n", &config);
        assert!(
            !written.contains("agents"),
            "the file kept a table with nothing in it: {written}"
        );
    }

    /// An agent Obelus has never installed keeps what the reader wrote.
    ///
    /// The table is Obelus's to write whole, the way the keys are -- which
    /// is only lossless because reading takes *every* agent out of the
    /// file, including ones this machine has never installed and ones a
    /// newer Obelus knows about. A read that kept only the installed ones
    /// would quietly empty the file on the next switch anybody flipped.
    ///
    /// Broken deliberately by having the writer keep only the agents it
    /// had read this session: the hand-written one went on the first save.
    #[test]
    fn an_agent_obelus_has_never_installed_keeps_what_the_reader_wrote() {
        let read = from_toml("[agents.some-other-agent]\nmode = \"ask\"\n");
        assert_eq!(read.agent_default("some-other-agent", "mode"), Some("ask"));

        let mut config = read;
        config.set_agent_default("claude-code", "mode", "plan");
        let read = from_toml(&to_toml(&config));
        assert_eq!(
            read.agent_default("some-other-agent", "mode"),
            Some("ask"),
            "writing one agent down lost another"
        );
        assert_eq!(read.agent_default("claude-code", "mode"), Some("plan"));
    }

    /// Only what differs from the default is written down, and a line that
    /// has come back to it is taken out.
    ///
    /// Otherwise the file freezes the defaults of the version that first
    /// saved it, and a default that changes afterwards never reaches the
    /// reader -- which is how the glyphs stayed on for everybody who had
    /// flipped anything, once they had gone off by default.
    ///
    /// Broken deliberately by writing every setting whatever its value, the
    /// way `over` used to: the defaults' file is no longer empty, and the
    /// stale `icons = true` survives a save it should have been taken out by.
    #[test]
    fn only_what_differs_from_the_default_is_written() {
        let written = to_toml(&Config::default());
        assert!(
            written.trim().is_empty(),
            "the defaults were written down: {written:?}"
        );

        // What an older Obelus left behind: every setting, one of them at a
        // default that has changed since, and a line of the reader's own.
        let existing = "theme = \"dark\"\nicons = true\nwrap = false\n# mine\nfuture_setting = 3\n";
        let config = Config {
            wrap: true,
            animation: false,
            ..Config::default()
        };
        let written = over(existing, &config);
        assert!(!written.contains("theme"), "a default stayed: {written:?}");
        assert!(
            !written.contains("icons"),
            "a line the reader never said stayed: {written:?}"
        );
        assert!(
            written.contains("wrap = true"),
            "what they did say is gone: {written:?}"
        );
        assert!(
            written.contains("future_setting = 3"),
            "a line Obelus does not know went: {written:?}"
        );
        assert_eq!(from_toml(&written), config);
    }

    /// A file Obelus makes lists every setting, in comments, and says none
    /// of them.
    ///
    /// Only what differs is written, so the first file would otherwise be
    /// empty -- and it is the file `open-settings-file` puts in front of a
    /// reader who has come to change something by hand. A file the reader
    /// emptied themselves is theirs, and stays empty.
    ///
    /// Broken deliberately by starting a new file from nothing again: the
    /// settings are not in it.
    #[test]
    fn a_new_settings_file_lists_every_setting_and_sets_none() {
        let directory =
            std::env::temp_dir().join(format!("obelus-config-template-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let file = directory.join("config.toml");

        super::save_to(&file, &Config::default()).expect("writing it");
        let made = std::fs::read_to_string(&file).expect("reading it");
        for setting in super::ALL {
            assert!(
                made.contains(&format!("# {} = ", setting.key)),
                "{} is not listed: {made}",
                setting.key
            );
        }
        assert_eq!(
            from_toml(&made),
            Config::default(),
            "the list said something"
        );

        std::fs::write(&file, "").expect("emptying it");
        super::save_to(&file, &Config::default()).expect("writing it again");
        assert_eq!(
            std::fs::read_to_string(&file).expect("reading it"),
            "",
            "a file the reader emptied was filled in again"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Another Obelus halfway through writing the settings is left alone,
    /// the reader's own and a project's.
    ///
    /// Broken deliberately by writing beside them as `toml.writing` again,
    /// with no process number, in `save_to` (the first) and in
    /// `write_project` (the second): the other's file is truncated and
    /// renamed away under it.
    #[test]
    fn another_obelus_writing_the_settings_is_left_alone() {
        let root =
            std::env::temp_dir().join(format!("obelus-config-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");

        let own = root.join("config.toml");
        let theirs = own.with_extension("toml.writing");
        std::fs::write(&theirs, "another Obelus is halfway through this").expect("theirs");
        super::save_to(&own, &super::Config::default()).expect("saving");
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("another Obelus is halfway through this"),
            "the other Obelus's half-written settings were taken"
        );

        let project = super::project_path_for(&root);
        let theirs = project.with_extension("toml.writing");
        std::fs::create_dir_all(theirs.parent().expect("a directory")).expect("the directory");
        std::fs::write(&theirs, "another Obelus is halfway through this").expect("theirs");
        let theme = super::Value::Choice("light".to_string());
        super::write_project(&project, "theme", Some(&theme)).expect("a project's setting");
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("another Obelus is halfway through this"),
            "the other Obelus's half-written project settings were taken"
        );
    }

    /// A save that fails leaves nothing beside the settings.
    ///
    /// Broken deliberately by taking the `remove_file` out of `save_to`:
    /// the half that was written stays, under a name nobody will write
    /// again.
    #[test]
    fn a_save_that_fails_leaves_nothing_beside_the_settings() {
        let root =
            std::env::temp_dir().join(format!("obelus-config-failed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("config.toml");
        std::fs::create_dir_all(target.join("in the way")).expect("a directory where it goes");

        assert!(
            super::save_to(&target, &super::Config::default()).is_err(),
            "it was saved"
        );
        let beside: Vec<_> = std::fs::read_dir(&root)
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "config.toml")
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        assert!(beside.is_empty(), "it left {beside:?} behind");
    }
}

#[cfg(test)]
mod where_it_went_wrong {
    use super::*;

    /// A settings file that will not parse says which line would not.
    ///
    /// The line is the whole point: a reader told only that the file is
    /// broken has to find it themselves, and what Obelus can do instead is
    /// put a mark on it.
    ///
    /// Deliberate break: `reading_of` answering `Unreadable(_, None)`,
    /// which is what it did before the parser was asked where it stopped.
    #[test]
    fn a_file_that_will_not_parse_says_where() {
        let Reading::Unreadable(why, at) =
            reading_of("theme = \"dark\"\nicons = true\n\nfont_size 15\nwrap = true\n")
        else {
            panic!("a file that will not parse");
        };
        assert!(why.contains("expected `=`"), "{why}");
        // Counted from zero, which is what a diagnostic counts in: the
        // fourth line.
        let at = at.expect("where the parser gave up");
        assert_eq!(at.line.get(), 3, "{at:?}");
        // And at the column the parser stopped at rather than at the start
        // of the line -- a mark over the whole line says less.
        assert!(at.column.get() > 0, "{at:?}");
    }

    /// And one that parses is not unreadable.
    ///
    /// Deliberate break: `reading_of` always answering `Unreadable`.
    #[test]
    fn a_file_that_parses_is_not_unreadable() {
        assert!(matches!(
            reading_of("theme = \"dark\"\n"),
            Reading::Settings { .. }
        ));
    }

    /// A parser that names no characters gets the rest of its line.
    ///
    /// `key with no value` points at the gap where the value would have
    /// gone, so the span it gives is empty -- and a mark over no
    /// characters is one a reader cannot see, which is the one thing a
    /// mark must not be.
    ///
    /// Deliberate break: `span_of_bytes` returning the empty range as it
    /// arrived, which leaves the end where the start is.
    #[test]
    fn a_span_of_nothing_is_the_rest_of_its_line() {
        let Reading::Unreadable(_, at) = reading_of("theme = \"dark\"\nfont_size 15\n") else {
            panic!("a file that will not parse");
        };
        let at = at.expect("where the parser gave up");
        assert_eq!(at.line, at.end_line);
        assert!(at.end_column.get() > at.column.get(), "{at:?}");
        // To the end of the line and no further: the lines under it are
        // not what is wrong.
        assert_eq!(at.end_column.get(), "font_size 15".len());
    }

    /// Every key in a settings file says which line it is written on,
    /// including the ones under a table.
    ///
    /// Dotted for those, because that is how a line that did nothing names
    /// itself when what is wrong is one agent's entry rather than the
    /// whole table.
    ///
    /// Deliberate break: `spans_in` walking only the top level, which is
    /// what the last assertion is for.
    #[test]
    fn every_key_says_which_line_it_is_on() {
        let spans = spans_in("theme = \"dark\"\nshrift = 15\n\n[agents]\ncopilot = \"gpt-5\"\n");
        assert_eq!(spans.get("theme").map(|at| at.line.get()), Some(0));
        assert_eq!(spans.get("shrift").map(|at| at.line.get()), Some(1));
        assert_eq!(spans.get("agents").map(|at| at.line.get()), Some(3));
        assert_eq!(spans.get("agents.copilot").map(|at| at.line.get()), Some(4));
    }

    /// A place is counted in lines and characters, from zero.
    ///
    /// The document's own counts, not bytes: a settings file may name a
    /// font or a theme in any language, and a column counted in bytes
    /// would put the mark in the middle of a character.
    ///
    /// Deliberate break: `span_of_bytes` handing back the byte offset in place
    /// of the character count, which the Chinese line catches.
    #[test]
    fn a_place_is_counted_in_lines_and_characters() {
        let text = "one\n\u{4f60}\u{597d} = 1\n";
        let at =
            obelus_text::span_of_bytes(text, &(text.find("= 1").expect("the equals")..text.len()));
        assert_eq!((at.line.get(), at.column.get()), (1, 3));
    }
}
