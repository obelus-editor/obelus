//! The list of conversations, and what taking one up costs.
//!
//! What a reader has said to an agent about this project outlives the
//! window they said it in: the agent keeps every word, and Obelus keeps the
//! one thing it cannot -- which conversation is which. This is the way back
//! to one.
//!
//! A compact list, over whatever the reader is in -- a file, or one of the
//! conversations. Not a view of its own: the reader is choosing where to
//! talk, which is what a list over something rather than instead of it is
//! for.
//!
//! **A new conversation is one of the answers.** The first row starts one,
//! so the list is never empty and the key that opens it is never dead: on a
//! project nobody has talked about yet, that row is the whole list. It is
//! where the reader lands unless they are already in a conversation, which
//! is where they land then -- the way out of one goes to a neighbour, and
//! a row away from the one they came from is a row away from either. `f4`
//! used to open a conversation outside one and the list inside, which put
//! the list two presses away from a file and gave a reader in a file one
//! answer to "which conversation", always the same. It is one question with
//! one key now. A new conversation is an action as well, `new-conversation`
//! on the palette, and a new one nothing has been said in yet is gone back
//! to rather than joined by a second.
//!
//! **Newest first, by when something was last said in it**, which is a
//! field (`Kept::last`) because it cannot be worked out from anything else,
//! and written in the same words a commit's row uses (`how_long_ago`).
//!
//! **Its rows are read whole.** A conversation is a sentence -- what the
//! agent called it, often the whole of what the reader first said, and the
//! note it was about -- so a row wraps rather than being cut to a line, and
//! the rows are in runs by the day they were last spoken in. Only the
//! title is what the query is about; the note is said under it and matched
//! by nothing, the way every list's detail is.
//!
//! **A conversation belongs to the agent that had it.** A session id is a
//! name one agent minted and means nothing to another, so only the agent in
//! use can be asked to take one up. The others are still shown -- a tab
//! each, greyed, with the reason above them -- because the alternative is a
//! reader who changed agents finding their conversations gone and nowhere
//! saying where. A tab exists only where that agent has something in it,
//! or is the agent in use -- whose tab is where a new one starts -- so the
//! ordinary case of one agent has no tab row at all. The tabs are scopes
//! and not groups, because a picker's group tabs come with an `All` in
//! front of them and `All` is the one tab this list must not have -- it
//! would mix the rows that can be taken up with the rows that cannot.
//! Nothing here switches the agent: that is a setting, and doing it from a
//! row would drop the session of every conversation open, including the
//! one the reader is standing in.
//!
//! **And to the checkout it was had in.** The agent is told a directory and
//! keeps the conversation under it, so a worktree of this project is
//! another place it cannot be taken up from. Rows, not tabs: the reader
//! has one agent and several checkouts far more often than the other way
//! round, and a row that names `obelus` beside its time is a row that says
//! where to go.
//!
//! **Which conversations there are is a snapshot; which of them can be
//! taken up is not.** The rows are made once, as the list opens, because
//! making them reads the table and the notes and walks the claims -- and
//! because rows arriving under a reader's selection move them somewhere
//! they did not choose. What each row says *about itself* is asked again
//! whenever a claim moves, which is the half that has to be right before
//! they press: a conversation another window took a moment ago goes dim
//! under them. So one started elsewhere while this list is up shows the
//! next time it is opened, and one taken up elsewhere shows at once.
//!
//! And the waking is the other half. A claim is a lock, and a lock is
//! invisible to a watcher -- nothing is written when one is taken, which is
//! the whole reason the claim has a file -- so the directory of them is
//! what one Obelus wakes another on, and without a watch on it the rows
//! would be asked again only when the reader happened to press something.
//! The watch is counted, so the notes page and this list can hold it at
//! once; which of them holds it is decided every frame from what is showing
//! (`settle_the_watches`), rather than switched on where a list opens and
//! off in each of the ways it closes.

use obelus_component::picker::{Marking, Remark, Said};

use super::{talking::Whose, *};
use crate::conversation::Topic;

/// Which conversations somebody has open, by its place in `App::watching`.
pub(super) const CLAIMS: usize = 0;
/// Which of the notes has a conversation at all.
pub(super) const TABLE: usize = 1;
/// What the notes say.
pub(super) const NOTES: usize = 2;
/// Which windows are on which of the repository's trees.
pub(super) const WINDOWS: usize = 3;
/// Whether another window has asked for the chat.
pub(super) const REMOTE: usize = 4;
/// How many of them there are.
pub(super) const WATCHED: usize = 5;

/// One of the things Obelus watches, and whether it managed to.
///
/// Two fields and not one, because they answer two different questions and
/// the difference is not cosmetic. What the views *want* heard about is
/// what says when to read the thing -- once, as it becomes wanted -- and a
/// machine whose watcher would not start still has to read it, or its notes
/// page would show nothing at all rather than something a little behind.
/// What is actually *watched* is what there is to give up again. Folding
/// the two into one field tied the reading to the watching, and an Obelus
/// with no watcher read nothing for the rest of the session.
#[derive(Debug, Default)]
pub(super) struct Watched {
    /// Whether anything showing is drawn from it.
    ///
    /// A yes or no rather than the path, so that the question a frame asks
    /// is a comparison and not a path built and thrown away: where the
    /// state directory is takes an environment variable and two
    /// allocations, and three of those on every frame of the notes is work
    /// a view has caused. The path is worked out on the edge, where it is
    /// needed.
    wanted: bool,
    /// And what the watcher took, which is nothing when it would not.
    held: Option<PathBuf>,
}

impl Watched {
    /// Nothing wanted and nothing held, for the array on `App`.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            wanted: false,
            held: None,
        }
    }
}

/// How one of them is watched, and whether its directory is Obelus's to
/// make when it is not there yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum How {
    /// The directory itself, for whatever turns up in it.
    Directory,
    /// One file, through the directory holding it.
    File,
    /// The same, where that directory may not exist yet.
    FileMakingRoom,
}

/// The mark a conversation another Obelus has open wears.
///
/// The lock the notes page uses for the same fact, in the column that page
/// puts it in -- one writer, because the list builds it once and asks for
/// it again on every frame it is up, and two spellings of one mark is one
/// chance for those to disagree.
///
/// And the list of pull requests wears it on a review another Obelus has,
/// for the same fact.
pub(super) fn locked() -> (Marking, String) {
    (
        Marking::Aside,
        match obelus_icons::enabled() {
            true => obelus_icons::ui::ELSEWHERE.to_string(),
            false => "-".to_string(),
        },
    )
}

/// The mark on the row that starts a new conversation.
///
/// In the column the other two marks are in, so that its words start where
/// every title does.
fn fresh() -> (Marking, String) {
    (Marking::Aside, "+".to_string())
}

/// The mark on the conversation the reader is in.
///
/// The one a history puts on the branch the reader is on, and the
/// worktrees on this window, for the same reason: it is the one row in a
/// list of places that they do not need to go to. Opened from inside a
/// conversation, the list would otherwise draw the conversation they came
/// from as a row like any other.
pub(super) fn here() -> (Marking, String) {
    (Marking::Aside, "\u{2022}".to_string())
}

/// How tall the list of conversations may be.
///
/// More than a compact list is given, because a conversation is a sentence
/// and the rows wrap, and because what it is drawn over is the conversation
/// the reader is leaving -- the reason the others are kept short is that
/// what is under them is still being read.
const CONVERSATION_ROWS: u16 = 24;

/// Which run of the list a conversation goes in, by the day something was
/// last said in it.
///
/// Days where the reader is, not stretches of twenty-four hours: something
/// said at eleven last night was said yesterday. The week is the five days
/// before that, which is the stretch a reader still remembers as recent.
/// One from before Obelus wrote down when goes in the last run, which is
/// where the order has already put it.
fn when_said(
    last: Option<i64>,
    today: jiff::civil::Date,
    zone: &jiff::tz::TimeZone,
) -> &'static str {
    let days = last
        .and_then(|last| jiff::Timestamp::from_second(last).ok())
        .map(|when| when.to_zoned(zone.clone()).date())
        .and_then(|day| today.since(day).ok())
        .map(|since| since.get_days());
    match days {
        // A day ahead of this one is a clock that disagrees, and today is
        // the least wrong thing to say about it -- `how_long_ago`'s answer.
        Some(..=0) => "Today",
        Some(1) => "Yesterday",
        Some(2..=6) => "This week",
        _ => "Earlier",
    }
}

/// The list of conversations, while it is the list showing.
///
/// Two halves of one thing: which agent each tab names, and what each row
/// of the tab showing stands for. Both are refilled together when the
/// reader walks onto a tab, because a row is chosen by its position and a
/// list of rows that outlived the tab it was fetched for would be rows
/// pointing at somebody else's conversations.
#[derive(Debug, Default)]
pub(super) struct Conversing {
    /// The agents the tabs name, in tab order. Empty when the list showing
    /// is not this one.
    pub agents: Vec<String>,
    /// What each row stands for, in the order they were given to the list.
    rows: Vec<Listed>,
    /// Whether what Obelus wrote down about this project would not read.
    unreadable: bool,
}

/// What one row of the list stands for.
///
/// Worked out when the list is built rather than read off the file twice:
/// the row draws one of these and choosing it acts on the same one.
#[derive(Clone, Debug)]
struct Listed {
    /// Which conversation, which is what a claim is taken on.
    which: obelus_agent::chats::ChatId,
    /// The agent's own name for it, which is what reopens it.
    session: String,
    /// Whether this Obelus already has it open, and where.
    open: Option<DocumentId>,
    /// Whether it was had in another checkout of this project, which is
    /// the only one its agent can take it up in.
    ///
    /// Such a row is about a note this checkout may be talking about too,
    /// in a conversation of its own: so it is never the one open here, and
    /// a lock on the note is not a lock on it.
    there: bool,
    /// Which checkout and when, as the row was built: nothing a frame
    /// asks again moves it.
    trailing: Option<String>,
}

impl App {
    /// Offers every conversation this project has had, by agent.
    pub(super) fn open_conversation_picker(&mut self) {
        // Whatever the table holds, including nothing, and including a
        // table that will not read: the list always has the new row, so it
        // opens either way and says what it found.
        //
        // Looked at once as the list is built, because a watch says what
        // happens next and not what was already there -- and the rows are
        // made here, before the frame that takes the watch.
        self.reread_who_holds_what();
        // Once, for the tabs and the rows both: what is said over the list
        // and the rows under it have to come from one reading, and each
        // reading may try the lock on a claim.
        let reading = obelus_agent::acp::sessions::read(
            &self.working_directory,
            self.config().conversation_days,
        );
        self.conversing.unreadable =
            matches!(reading, obelus_agent::acp::sessions::Reading::Unreadable(_));
        let table = reading.remembered();
        let remembered = table.clone().unwrap_or_default();
        // A fourth reason the kept copy is read, and the one that costs
        // nothing: this has the table in its hands. Without it a
        // conversation taken up from here would be told about its note
        // again, because what the agent was already told is looked up in
        // that copy.
        self.sessions_kept = Some(remembered.clone());
        // Which agents have said anything here, most recently talked to
        // first -- and the one in use ahead of all of them, because that is
        // the tab the reader lands on and the only one whose rows they can
        // take up.
        let mut agents: Vec<String> = Vec::new();
        let mut when: HashMap<String, i64> = HashMap::new();
        for (_, agent, _, kept) in remembered.all() {
            let last = kept.last.unwrap_or(i64::MIN);
            let seen = when.entry(agent.to_string()).or_insert(i64::MIN);
            *seen = (*seen).max(last);
            if !agents.iter().any(|known| known == agent) {
                agents.push(agent.to_string());
            }
        }
        let in_use = self.settled.config.agent.clone().unwrap_or_default();
        // And the one in use whether or not it has said anything here, because
        // its tab is where a new conversation is started.
        if !agents.contains(&in_use) {
            agents.push(in_use.clone());
        }
        agents.sort_by_key(|agent| {
            (
                *agent != in_use,
                std::cmp::Reverse(when.get(agent).copied().unwrap_or(i64::MIN)),
            )
        });
        let names: Vec<String> = agents.iter().map(|agent| self.tab_called(agent)).collect();
        // Declared before the rows are built and before the list is shown:
        // the rows are fetched per tab and the tabs are these agents, so
        // nothing about this list can be worked out without them.
        //
        // `show_list` clears whatever the last list declared, which is why
        // they are put back after it -- the same order `open_troubles`
        // takes with its radii, and for the same reason.
        self.conversing.agents = agents;
        let rows = self.conversation_rows(0, table);
        // Where the list starts: on the conversation the reader is in,
        // where they are in one, and on the new one otherwise, which is the
        // first row.
        let here = self
            .conversing
            .rows
            .iter()
            .position(|listed| listed.open.is_some() && listed.open == self.current)
            .and_then(|at| {
                rows.iter().position(
                    |item| matches!(item.value, PickerValue::Conversation(row) if row == at),
                )
            });
        let mut picker = Picker::new(
            rows,
            PickerLayout::Compact {
                rows: CONVERSATION_ROWS,
            },
        );
        // Read whole, because every row is a sentence -- what the agent
        // called it and what the note says -- and one row of a sentence is
        // a row saying there was more. The mark in front of a note's words
        // is the note's own box.
        picker.wraps(Some(obelus_ui::tick(false)));
        // Newest first whatever is typed, because the runs are days: a
        // query ranking the rows would scatter them out from under their
        // headings. What it does is say which rows are left.
        picker.keeps_order(true);
        picker.before_typing("Filter conversations");
        // Scopes rather than groups: the rows of a tab are fetched when the
        // reader walks onto it, the way a search's are. A row of tabs the
        // picker filters would need an "All" in front of them, which is the
        // one tab this list must not have -- it would mix the rows that can
        // be taken up with the rows that cannot.
        //
        // And only where there is more than one: a tab row over the only
        // agent a reader has talked to says nothing and costs two rows.
        if names.len() > 1 {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            picker.with_scopes(&names);
        }
        self.say_whose_conversations(&mut picker, 0);
        picker.opened_by(Command::ConversationSelect);
        if let Some(row) = here {
            picker.select_item(row);
        }
        let conversing = std::mem::take(&mut self.conversing);
        self.show_list(picker);
        self.conversing = conversing;
    }

    /// Fills the list again for the tab the reader has walked onto.
    pub(super) fn refresh_conversations(&mut self) {
        let Some(tab) = self.picker.as_ref().map(Picker::tab) else {
            return;
        };
        // Read with the rows rather than beside them, so that what is said
        // over the list and the rows under it come from one reading: two
        // readings of a file another window writes can disagree.
        let reading = obelus_agent::acp::sessions::read(
            &self.working_directory,
            self.config().conversation_days,
        );
        self.conversing.unreadable =
            matches!(reading, obelus_agent::acp::sessions::Reading::Unreadable(_));
        let rows = self.conversation_rows(tab, reading.remembered());
        let Some(mut picker) = self.picker.take() else {
            return;
        };
        picker.replace(rows);
        self.say_whose_conversations(&mut picker, tab);
        self.picker = Some(picker);
    }

    /// Says the name a conversation goes by now in the lists that name
    /// one, where either is up: this list and the list of what is open.
    ///
    /// Built again rather than remarked, because a name is the one thing
    /// `remark` may not change -- what the query matched is offsets into it.
    /// And with the reader kept on the row they were on: a name arriving is
    /// nobody's keystroke, and a list that went back to its first row for
    /// one would be a list that moved under them.
    pub(super) fn say_the_new_name(&mut self) {
        self.relist_switching();
        if self.conversing.agents.is_empty() || self.picker.is_none() {
            return;
        }
        let on = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .and_then(|item| match item.value {
                PickerValue::Conversation(at) => self.conversing.rows.get(at),
                _ => None,
            })
            .map(|listed| (listed.which.clone(), listed.there));
        self.refresh_conversations();
        let Some((which, there)) = on else {
            return;
        };
        let Some(at) = self
            .conversing
            .rows
            .iter()
            .position(|listed| listed.which == which && listed.there == there)
        else {
            return;
        };
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let row = picker.matches().position(
            |item| matches!(item.value, PickerValue::Conversation(listed) if listed == at),
        );
        if let Some(row) = row {
            picker.select_row(row);
        }
    }

    /// What the list says about itself on this tab, which is why the rows
    /// of every tab but one cannot be chosen.
    ///
    /// Only where it is not the reader's own agent: a list that explained
    /// itself on the tab where everything works would be explaining a rule
    /// nothing on screen has broken.
    ///
    /// And where Obelus could not read what it wrote down, which is said
    /// here rather than as an empty list's line because the list is never
    /// empty: a table Obelus cannot read is not a project nobody has said
    /// anything about, and a list holding only the new row would tell the
    /// reader it is.
    fn say_whose_conversations(&self, picker: &mut Picker, tab: usize) {
        let in_use = self.settled.config.agent.clone().unwrap_or_default();
        let whose = self.conversing.agents.get(tab).cloned().unwrap_or_default();
        let said = match whose == in_use {
            true if self.conversing.unreadable => {
                "Obelus cannot read what it wrote down about this project.".to_string()
            }
            true if self.conversing.rows.iter().any(|listed| listed.there) => {
                "A conversation can only be taken up in the checkout it was had in.".to_string()
            }
            true => String::new(),
            false if in_use.is_empty() => {
                "No agent is chosen, and a conversation can only be taken up by the agent that had it."
                    .to_string()
            }
            false => format!(
                "{} is the agent in use, and a conversation can only be taken up by the agent that had it.",
                self.agent_called(&in_use)
            ),
        };
        picker.about(&said);
    }

    /// The rows of one tab, newest first, from a reading of the table --
    /// `None` where it would not read.
    fn conversation_rows(
        &mut self,
        tab: usize,
        remembered: Option<obelus_agent::acp::sessions::Remembered>,
    ) -> Vec<PickerItem> {
        self.conversing.rows.clear();
        let Some(whose) = self.conversing.agents.get(tab).cloned() else {
            return Vec::new();
        };
        let mine = self.settled.config.agent.as_deref().unwrap_or_default() == whose;
        // Only on the tab of the agent in use, because that is who a new
        // conversation would be with. Starting one from another agent's tab
        // would be changing agents, which is a setting.
        let fresh = mine.then(|| PickerItem {
            prose: false,
            marker: Some(fresh()),
            icon: None,
            label: "New conversation".to_string(),
            version: None,
            detail: None,
            trailing: None,
            changed: None,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            // Above the runs and in none of them: it is not a day's.
            section: None,
            value: PickerValue::Command(obelus_command::Command::ConversationNew),
        });
        // Three answers and not two, because a table Obelus cannot read is
        // not a project nobody has said anything about, and the new row
        // alone would tell the reader it is.
        let Some(remembered) = remembered else {
            return fresh.into_iter().collect();
        };
        // Every conversation somebody has open, as Obelus last looked --
        // which is when something told it to look. See
        // `App::reread_who_holds_what`.
        let held = self.held_kept.clone();
        let todo = obelus_todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default();
        let notes = &todo.notes;
        let now = std::time::SystemTime::now();
        let zone = jiff::tz::TimeZone::system();
        let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
        let mut rows: Vec<(Option<i64>, Listed, PickerItem)> = remembered
            .all()
            .filter(|(_, agent, _, _)| *agent == whose)
            .map(|(which, _, tree, kept)| {
                let there = (tree != self.working_directory).then_some(tree);
                let open = self.conversation_open_here(which, there.is_some());
                let elsewhere = there.is_none() && open.is_none() && held.contains_key(which);
                // What the agent called it, then what the note says, then
                // nothing anybody wrote: a conversation an agent never
                // titled and no note names has only the fact that it
                // happened.
                let about = match which {
                    obelus_agent::chats::ChatId::Note(id) => notes
                        .iter()
                        .find(|note| note.id == *id)
                        .map(|note| note.title().to_string()),
                    obelus_agent::chats::ChatId::PullRequest(number) => {
                        Some(self.what_a_review_is_called(*number))
                    }
                    obelus_agent::chats::ChatId::Issue(number) => {
                        Some(self.what_an_answer_is_called(*number))
                    }
                    obelus_agent::chats::ChatId::Loose(_) => None,
                };
                // One that is open here goes by what the list of what is
                // open calls it, which is newer than anything written down:
                // the agent's name as it arrives, and the reader's first
                // words before there is one.
                let label = open
                    .and_then(|id| self.document(id))
                    .and_then(Document::chat)
                    .and_then(|talk| self.conversation_name(talk, &todo))
                    .or_else(|| kept.title.clone())
                    .or_else(|| about.clone())
                    .unwrap_or_else(|| "Untitled".to_string());
                // Not said twice: a title the agent never gave is the
                // note's own words, and the same words after them is the
                // rule a setting's description already follows. And
                // nothing at all for a conversation about no note, which
                // is a row one line shorter rather than a line saying so.
                let detail = about.filter(|about| *about != label);
                // Which checkout, by the name a reader gave its directory:
                // what tells two worktrees of one project apart is the
                // last part of the path, and the rest is the same for both.
                // Beside the time rather than under the title, because the
                // row under it is the note's and carries the note's box.
                let when = kept.last.map(|last| obelus_git::how_long_ago(last, now));
                let trailing = match there.map(|tree| {
                    tree.file_name()
                        .unwrap_or(tree.as_os_str())
                        .to_string_lossy()
                }) {
                    Some(checkout) => Some(match when {
                        Some(when) => format!("{checkout}  {when}"),
                        None => checkout.to_string(),
                    }),
                    None => when,
                };
                let item = PickerItem {
                    // A sentence, which loses its end where it has to lose
                    // anything: cut from the front, a title is the half of
                    // it that does not say what it was about.
                    prose: true,
                    marker: self.listed_mark(open, elsewhere),
                    icon: None,
                    label,
                    version: None,
                    detail,
                    trailing,
                    changed: None,
                    // All three say the same thing in the ink: a row that
                    // belongs to another agent or another checkout cannot
                    // be taken up, and one another Obelus is in is not this
                    // window's to enter.
                    enabled: mine && there.is_none() && !elsewhere,
                    colours: None,
                    status: None,
                    depth: 0,
                    opens: None,
                    kind: None,
                    tab: None,
                    section: Some(when_said(kept.last, today, &zone).to_string()),
                    // Stands for its place in the list rather than for
                    // the conversation, the way a server's offered actions
                    // do: what a row stands for is Obelus's own
                    // bookkeeping -- a note or a session id, a claim,
                    // where it is already open -- and a list of rows is
                    // not where that belongs. Which place it is is not
                    // known until they are sorted, just below.
                    value: PickerValue::Conversation(0),
                };
                (
                    kept.last,
                    Listed {
                        which: which.clone(),
                        session: kept.session.clone(),
                        open,
                        there: there.is_some(),
                        trailing: item.trailing.clone(),
                    },
                    item,
                )
            })
            .collect();
        // Newest first, and the ones from before Obelus wrote down when
        // last. A made-up time would have put those somewhere in the order
        // on no evidence at all.
        rows.sort_by_key(|(last, _, _)| std::cmp::Reverse(*last));
        let mut stands_for: Vec<Listed> = Vec::with_capacity(rows.len());
        let mut items: Vec<PickerItem> = fresh.into_iter().collect();
        for (at, (_, listed, mut item)) in rows.into_iter().enumerate() {
            item.value = PickerValue::Conversation(at);
            stands_for.push(listed);
            items.push(item);
        }
        self.conversing.rows = stands_for;
        items
    }

    /// Asks again which conversations somebody else has open, while the
    /// list is up.
    ///
    /// The one thing on these rows that changes under the reader and is
    /// nobody's keystroke: another Obelus opens or closes a conversation
    /// and the row that was theirs to take stops being it, or the other way
    /// about. The rows themselves stay -- they are a snapshot, and a list
    /// that rebuilt itself every frame would slide new rows in under the
    /// reader's selection -- so only what a row says about itself is asked
    /// again.
    ///
    /// Both halves of that together, which is why [`Said`] carries both:
    /// the lock and whether the key works are one fact, and a list that
    /// could refresh one without the other is a list that says a row is
    /// somebody else's and lets the reader in anyway.
    ///
    /// One walk of the claims, which is what the notes page pays every
    /// frame it is showing, for the same answer. Measured: 8us for a
    /// project nobody has a conversation open in, 17us with five and 41us
    /// with twenty -- and Obelus has no frame rate, so the bill is one of
    /// those per keystroke plus, while something is animating behind the
    /// list, one per tick.
    ///
    /// Not kept between frames, and not asked only when the watcher says
    /// the directory moved, which is the cache that suggests itself now
    /// that there is a watch. A claim is given up by the *lock* going, and
    /// an Obelus that crashed gives up its lock with nothing written and no
    /// event at all: the file it left behind is still there. So an answer
    /// kept until the directory next changes is an answer that can go on
    /// saying a conversation is somebody else's for the rest of the
    /// session. The notes page settled this first, for the same reason.
    pub(super) fn freshen_the_conversation_rows(&mut self) {
        if self.conversing.agents.is_empty() || self.picker.is_none() {
            return;
        }
        // Worked out before the list is borrowed to change, because both
        // halves are this application's.
        let held = self.held_now();
        let in_use = self.settled.config.agent.clone().unwrap_or_default();
        let mine = self.conversing.agents.get(self.tab_showing()) == Some(&in_use);
        let said: Vec<Said> = self
            .conversing
            .rows
            .iter()
            .map(|listed| {
                let open = self.conversation_open_here(&listed.which, listed.there);
                let elsewhere = !listed.there && open.is_none() && held.contains_key(&listed.which);
                Said {
                    marker: self.listed_mark(open, elsewhere),
                    enabled: mine && !listed.there && !elsewhere,
                    trailing: listed.trailing.clone(),
                }
            })
            .collect();
        // And where a conversation was taken up in another window while the
        // reader was looking at the row, the row they are standing on is
        // one of these: it says so before they press, which is the whole
        // of what this is for.
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.remark(|value| match value {
            PickerValue::Conversation(at) => said
                .get(*at)
                .map_or(Remark::Keep, |now| Remark::Now(now.clone())),
            _ => Remark::Keep,
        });
    }

    /// Where this Obelus has a row's conversation open, which for a row from
    /// another checkout is nowhere: what is open here about the same note is
    /// this checkout's own conversation about it.
    fn conversation_open_here(
        &self,
        which: &obelus_agent::chats::ChatId,
        there: bool,
    ) -> Option<DocumentId> {
        match there {
            true => None,
            false => self.conversation_open(which),
        }
    }

    /// What the column in front of a conversation says about it: that it
    /// is the one the reader is in, or that another Obelus has it.
    ///
    /// One answer for the rows as they are built and as they are asked
    /// again, so that asking again cannot take the first mark away.
    fn listed_mark(&self, open: Option<DocumentId>, elsewhere: bool) -> Option<(Marking, String)> {
        match (open.is_some() && open == self.current, elsewhere) {
            (true, _) => Some(here()),
            (false, true) => Some(locked()),
            (false, false) => None,
        }
    }

    /// What a tab says it is, which for the agent in use when none is
    /// chosen is that.
    ///
    /// A tab with no words on it is what the agent's own name gives there:
    /// it has none, and the tab is still where a new conversation starts.
    fn tab_called(&self, agent: &str) -> String {
        match agent.is_empty() {
            true => "No agent".to_string(),
            false => self.agent_called(agent),
        }
    }

    /// Which tab the list is showing, or the first where there is no list.
    fn tab_showing(&self) -> usize {
        self.picker.as_ref().map_or(0, Picker::tab)
    }

    /// Watches what the views showing are drawn from, and stops watching
    /// what nothing is drawn from.
    ///
    /// Four things, and every one of them a file somebody else writes:
    /// which conversations are open elsewhere, which of the notes has a
    /// conversation at all, what the notes say, and which windows are on
    /// which of the repository's trees. Each is kept in memory
    /// and each is only as honest as what refreshes it, so the watch is the
    /// whole of the promise.
    ///
    /// Asked from what is open, every frame, rather than taken at each door
    /// a view can be opened by and given up at each door it can be closed
    /// by. Two of these had been done the second way and the third had
    /// both, which is how one file came to be watched twice by two owners:
    /// the counting inside the watcher made that work and made it invisible.
    /// A watch switched on in one place and off in three outlives its reason
    /// the first time somebody adds a fourth way out -- the ticker's rule,
    /// one level along.
    ///
    /// Only the watches. *Reading* the three is the view's own, done as it
    /// opens: a watch says what happens next and not what was already
    /// there, and a frame is too late anyway -- the notes page is opened by
    /// one key and talked about with the next, and two keys can be drained
    /// before a frame is drawn between them.
    pub(super) fn settle_the_watches(&mut self) {
        // All three are the project's, and are given up with it: a page
        // left open on a tree that has gone is a page of what the tree
        // had, and nothing will be written to it from here.
        let project = self.has_a_project();
        let notes = project && self.notes_document().is_some();
        let listing = project
            && ((!self.conversing.agents.is_empty() && self.picker.is_some())
                || self.listing_pull_requests());
        let about_a_note = project
            && self
                .conversation()
                .is_some_and(|talk| matches!(&talk.topic, Topic::Note(_)));

        // A directory rather than a file: a claim is a file appearing and
        // going again, so there is nothing here to watch by name. Wanted by
        // the notes, which mark the ones somebody else has, and by the list
        // of conversations and the list of pull requests, which grey them.
        self.settle_a_watch(
            CLAIMS,
            notes || listing,
            How::Directory,
            obelus_agent::chats::directory,
        );
        // The table saying which note has a conversation, which the notes
        // page draws, and what each is called, which the list of
        // conversations does.
        self.settle_a_watch(
            TABLE,
            notes || listing,
            How::FileMakingRoom,
            obelus_agent::acp::sessions::path,
        );
        // And the notes themselves: the page reads them, and so does the
        // box of a conversation about one -- which is usually open with
        // that page shut.
        self.settle_a_watch(NOTES, notes || about_a_note, How::File, obelus_todo::path);
        // And the other windows, which the list of worktrees marks. A
        // directory, for the reason the claims are one: a window is a file
        // appearing and going again.
        let worktrees = project && self.showing_worktrees();
        self.settle_a_watch(
            WINDOWS,
            worktrees,
            How::Directory,
            super::worktrees::directory,
        );
        // And the chat, wherever one is set: another window asking for it is
        // a file written beside the lock. Watched before this window has it
        // rather than from then -- a watch is taken a frame after it is
        // wanted, and an asking written in that frame would be missed -- and
        // heard only while it has it. Not the project's, and not given up
        // with it.
        let chat = self.platform().is_some();
        self.settle_a_watch(
            REMOTE,
            chat,
            How::Directory,
            super::remote::remote_directory,
        );
    }

    /// Takes or gives up one of them, so that what is watched matches what
    /// is wanted.
    ///
    /// Only the watch. Reading the thing is the view's own, done as it
    /// opens -- see the note on `settle_the_watches` -- because a view's
    /// keys can act before the frame after it opens, and a value read on
    /// that frame would be a keystroke too late.
    fn settle_a_watch(
        &mut self,
        which: usize,
        wanted: bool,
        how: How,
        where_it_is: impl FnOnce(&std::path::Path) -> Option<PathBuf>,
    ) {
        if wanted == self.watching[which].wanted {
            return;
        }
        // Whatever was actually taken, which is not always what was wanted.
        if let Some(had) = self.watching[which].held.take()
            && let Some(watcher) = self.watcher.as_mut()
        {
            match how {
                How::Directory => watcher.unwatch_directory(&had),
                How::File | How::FileMakingRoom => watcher.unwatch(&had),
            }
        }
        self.watching[which].wanted = wanted;
        if !wanted {
            return;
        }
        // Worked out here rather than by the caller, so that a frame on
        // which nothing has opened or closed does not build three paths to
        // throw away.
        let Some(path) = where_it_is(&self.working_directory) else {
            return;
        };
        // A watch on a directory that is not there is a watch on nothing.
        // Only where the directory is Obelus's own to make and may not
        // exist yet: the claims' one on a project nobody has talked about,
        // and the table's on an Obelus that has never written one down.
        // The notes' is not among them -- to be reading a conversation
        // about a note there has to be a note, which means a file, which
        // means the directory holding it, and making one here would be
        // Obelus putting a notes directory on a project with no notes.
        match how {
            How::Directory => {
                let _ = std::fs::create_dir_all(&path);
            }
            How::FileMakingRoom => {
                if let Some(directory) = path.parent() {
                    let _ = std::fs::create_dir_all(directory);
                }
            }
            How::File => {}
        }
        if let Some(watcher) = self.watcher.as_mut() {
            let taken = match how {
                How::Directory => watcher.watch_directory(&path),
                How::File | How::FileMakingRoom => watcher.watch(&path),
            };
            match taken {
                Ok(()) => self.watching[which].held = Some(path),
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "not watching it");
                }
            }
        }
    }

    /// Looks again at which conversations somebody has open.
    ///
    /// The one walk of the claims, done when something says to rather than
    /// on every frame: a claim taken or let go is a file appearing or
    /// going, and one let go by an Obelus dying is that file closed by a
    /// writer -- all three are things a watcher reports.
    ///
    /// The third on Linux alone. The close a dying process makes is
    /// inotify's and has no counterpart on macOS or Windows, so there this
    /// is stale from the moment another Obelus is killed until whichever
    /// of the other three moments comes first. The row says a lock that is
    /// not there; the key still works, because it asks for the claim
    /// rather than reading it off the row. See `obelus_watch`.
    ///
    /// Obelus's own claims are in here too. A lock belongs to the open file
    /// and not to the process, so a second look finds this window's own in
    /// the way; which of them are its own it knows from the conversations
    /// it is holding.
    pub(super) fn reread_who_holds_what(&mut self) {
        self.held_kept = obelus_agent::chats::held(&self.working_directory);
    }

    /// Whether a path that changed is one of this project's claims.
    #[must_use]
    pub(super) fn is_a_claim(&self, path: &std::path::Path) -> bool {
        obelus_agent::chats::directory(&self.working_directory)
            .is_some_and(|directory| path.parent() == Some(directory.as_path()))
    }

    /// Which conversations somebody has open, as Obelus last looked, and
    /// which checkout holds each where its claim says.
    #[must_use]
    pub fn held_now(
        &self,
    ) -> &std::collections::BTreeMap<obelus_agent::chats::ChatId, Option<std::path::PathBuf>> {
        &self.held_kept
    }

    /// Where this conversation is open in this Obelus, if it is.
    fn conversation_open(&self, which: &obelus_agent::chats::ChatId) -> Option<DocumentId> {
        self.documents
            .iter()
            .enumerate()
            .find(|(_, document)| {
                // By what names it, which is what claims it: for one about
                // nothing in particular that is the session it has, the one
                // it is asking for, or -- put back from last time and not
                // shown yet -- the one it is to take up. Asked of the session
                // alone, the last two were open here and listed as some
                // other window's, under the lock this one was holding.
                document
                    .as_ref()
                    .and_then(Document::chat)
                    .is_some_and(|talk| talk.which().as_ref() == Some(which))
            })
            .map(|(at, _)| DocumentId::new(at))
    }

    /// Goes to the conversation a row names, taking it up where it is not
    /// already open.
    ///
    /// The claim is asked for here and not read off the row: the list was
    /// built a moment ago and another Obelus may have walked into the
    /// conversation since. Where it has, the row says so -- the list stays
    /// open and redraws with the lock on it -- because a key that answers
    /// nothing is a key that looks broken.
    pub(super) fn take_up_conversation(&mut self, at: usize) -> bool {
        let Some(listed) = self.conversing.rows.get(at).cloned() else {
            return false;
        };
        if let Some(id) = listed.open {
            self.go_to_document(id);
            return true;
        }
        let Some(claim) = obelus_agent::chats::claim(&self.working_directory, &listed.which) else {
            // Being refused is itself news, and the freshest there is: it
            // says somebody holds this one at this instant, which is more
            // than the watcher has got round to saying. So Obelus looks
            // again here rather than waiting to be told what it has just
            // found out -- and the row the reader pressed goes dim under
            // them, which is the answer.
            self.reread_who_holds_what();
            return false;
        };
        self.make_room(Room::Region);
        // The same as the notes' own door: what the box is offered from is
        // read as the conversation opens, and kept level by a watch settled
        // on the next frame.
        self.reread_the_notes_kept();
        // A conversation about nothing in particular as much as one about a
        // note: both are taken up with every word the agent was told, and
        // reading "told nothing" for one sent it all again.
        let (told, introduced) = self.remembered_telling(&listed.which);
        let topic = Topic::of(&listed.which);
        let talk = crate::conversation::Conversation {
            told,
            introduced,
            topic,
            claim: Some(claim),
            ..crate::conversation::Conversation::default()
        };
        self.documents.push(Some(talk.into()));
        let at = DocumentId::new(self.documents.len() - 1);
        self.go_to_document(at);
        // Taking one up is the reader asking for it by name, so this is
        // one of the two moments a session is asked for -- the other is
        // their first message. The agent is started for it, because there
        // is nothing to ask until there is one.
        if self.talker.is_none() {
            self.start_agent();
        }
        self.ask_for_a_session(Whose::One(at), Some(listed.session.clone()));
        true
    }
}
