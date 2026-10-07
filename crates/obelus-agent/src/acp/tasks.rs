//! Work an agent goes on with after the call that started it has returned.
//!
//! A command left running in the background -- a dev server, a test run --
//! is a tool call that says it is done while what it started is not. The
//! protocol has no word for that yet: a turn is a prompt and its answer, and
//! nothing it defines is about the time after. So whether Obelus hears of
//! such work at all is a question about the agent, asked at the handshake
//! and answered by what it says there -- never by its name or its version.
//!
//! **What is shown is Obelus's own shape, and a dialect is only how it
//! arrives.** [`News`] and [`Board`] say what a reader is owed -- what is
//! running, what it is saying, how it ended, where its output is -- and
//! nothing in them is spelt the way any one agent spells it. The words on
//! the wire are a [`Dialect`]'s, each in a module of its own, so that the day
//! the protocol says this itself is a new module and a deleted one, and
//! nothing above this file notices. Today there is one: [`super::air`], the
//! extension claude-agent-acp speaks to JetBrains' client and documents as
//! experimental.
//!
//! **An extension that goes away leaves Obelus as it was before it came.**
//! A dialect nobody offers is [`Dialect::None`], and with it there is no
//! board, no count and no list -- a backgrounded command says it is done, as
//! it always did. One that is offered and then spoken badly is dropped a
//! message at a time, and given up on for the connection once enough of
//! them have failed: a half-read list of work that is always running is
//! worse than no list.

use std::{path::PathBuf, time::Instant};

use agent_client_protocol::schema::v1::Meta;

/// How many updates about work Obelus has not been told the start of are
/// kept, waiting for it.
///
/// The extension sends a task's start before anything else about it, so
/// this is room for a reordering, not for a stream: what is kept beyond it
/// is a dialect saying something Obelus does not understand, and memory
/// that grows for as long as it goes on saying it.
const HELD: usize = 32;

/// How many unreadable updates a connection is allowed before the dialect is
/// given up on for the rest of it.
///
/// One is a message Obelus could not read and is dropped. A run of them is
/// an extension whose shape has changed under Obelus, and a list built from
/// the half it can still read says work is running that ended long ago.
pub const UNREADABLE: u32 = 8;

/// The words a dialect speaks, and which one this connection settled on.
///
/// Chosen from the agent's answer to `initialize` and from nothing else:
/// Obelus declares every dialect it can read and uses the one the agent says
/// it speaks. An agent that says nothing is [`Self::None`], which is not a
/// failure -- it is how every agent but one is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// Nobody said they speak of background work, so nothing is heard of it.
    #[default]
    None,
    /// JetBrains' extension, as claude-agent-acp speaks it -- see
    /// [`super::air`].
    Air,
}

impl Dialect {
    /// Says in the handshake which dialects Obelus reads, into the client
    /// capabilities' `_meta`.
    ///
    /// Every one of them, because the handshake goes before the answer:
    /// Obelus cannot know which the agent speaks until it has been asked, and
    /// a key an agent does not know is a key it ignores.
    pub(crate) fn declare(meta: &mut Meta) {
        super::air::declare(meta);
    }

    /// Which one the agent answered in, from the `_meta` of its answer to
    /// `initialize`.
    pub(crate) fn chosen(meta: Option<&Meta>) -> Self {
        match super::air::offered(meta) {
            true => Self::Air,
            false => Self::None,
        }
    }

    /// What an update says about background work, if it is about that.
    ///
    /// `None` for an update that is not this dialect's, which goes on to be
    /// read as the protocol's own. `Some(Err)` for one that is, and could not
    /// be read: it is dropped and counted, never guessed at.
    pub(crate) fn read(self, update: &serde_json::Value) -> Option<Result<News, String>> {
        match self {
            Self::None => None,
            Self::Air => super::air::read(update),
        }
    }

    /// Whether a tool call's `_meta` says the work it started goes on.
    pub(crate) fn backgrounded(self, meta: Option<&Meta>) -> bool {
        match self {
            Self::None => false,
            Self::Air => super::air::backgrounded(meta),
        }
    }

    /// The method and the parameters that ask for one to be stopped.
    pub(crate) fn stop(self, session: &str, id: &str) -> Option<(&'static str, serde_json::Value)> {
        match self {
            Self::None => None,
            Self::Air => Some((super::air::STOP, super::air::stop(session, id))),
        }
    }

    /// Whether the answer to that says it stopped.
    ///
    /// `None` for an answer Obelus cannot read, which is not a refusal: what
    /// became of the task is said by the update that follows, whichever way
    /// it went.
    pub(crate) fn stopped(self, answer: &serde_json::Value) -> Option<bool> {
        match self {
            Self::None => None,
            Self::Air => super::air::stopped(answer),
        }
    }
}

/// How far a piece of background work has got.
///
/// The protocol's own words where there is one, and Obelus's where there is
/// not -- the same five every dialect has so far. Anything else is kept as
/// it was said and drawn as it was said, the way a tool call's unknown state
/// is: refused, it would be a task that never ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Still going.
    Running,
    /// Held, and still there.
    Paused,
    /// Ended, and well.
    Done,
    /// Ended, and not well.
    Failed,
    /// Ended because somebody stopped it.
    Stopped,
    /// A word Obelus has not heard, as it was said.
    Other(String),
}

impl State {
    /// Whether it is over, which decides whether it is counted and whether
    /// it can be stopped.
    ///
    /// A word Obelus does not know is not over: what it costs to be wrong
    /// that way is a count one too high until the agent says something
    /// else, and the other way is a task that is running and cannot be
    /// stopped from here.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Stopped)
    }

    /// What a tool call's row says about the call this work came from,
    /// once the work has ended -- in the protocol's own words for a call,
    /// which is what a row's state is written in.
    #[must_use]
    pub const fn as_call(&self) -> Option<&'static str> {
        match self {
            Self::Done => Some("completed"),
            Self::Failed => Some("failed"),
            Self::Stopped => Some("cancelled"),
            Self::Running | Self::Paused | Self::Other(_) => None,
        }
    }
}

/// What a dialect says about one piece of background work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum News {
    /// There is a new one.
    Began {
        /// The agent's id for it, which everything after names.
        id: String,
        /// What to call it.
        name: String,
        /// What sort of work it is, in the agent's word: `shell`,
        /// `workflow`, and whatever else it has.
        kind: String,
        /// One line about it, where the agent said one.
        about: Option<String>,
        /// The tool call that started it, where the agent said which.
        call: Option<String>,
        /// The file its output goes to, where it goes to one.
        output: Option<PathBuf>,
        /// Whether the agent says it can be stopped from here.
        stoppable: bool,
    },
    /// One has moved on: only what changed, and nothing for the rest.
    Changed {
        /// Which one.
        id: String,
        /// How far it has got, where that changed.
        state: Option<State>,
        /// What it says about itself now.
        summary: Option<String>,
        /// One line about it, where that changed.
        about: Option<String>,
        /// The tool call that started it, where that arrived late.
        call: Option<String>,
        /// The file its output goes to, where that arrived late.
        output: Option<PathBuf>,
    },
}

impl News {
    /// Which piece of work it is about.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Began { id, .. } | Self::Changed { id, .. } => id,
        }
    }
}

/// One piece of background work, as far as Obelus has been told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// The agent's id for it.
    pub id: String,
    /// What to call it.
    pub name: String,
    /// What sort of work it is, in the agent's word.
    pub kind: String,
    /// One line about it, where the agent said one.
    pub about: Option<String>,
    /// The tool call that started it, where the agent said which.
    pub call: Option<String>,
    /// The file its output goes to.
    pub output: Option<PathBuf>,
    /// Whether the agent says it can be stopped from here.
    pub stoppable: bool,
    /// How far it has got.
    pub state: State,
    /// The last thing it said about itself.
    pub summary: Option<String>,
    /// When Obelus heard it had started, for how long it has been going.
    pub began: Instant,
    /// When Obelus heard it had ended.
    pub ended: Option<Instant>,
    /// Whether Obelus has asked for it to be stopped and not heard back.
    pub stopping: bool,
}

/// What a conversation's background work has come to.
///
/// One of these a conversation, kept beside its settings: what the agent
/// started in one conversation is nothing to do with another, and a list
/// that mixed them would offer to stop a server the reader never asked for
/// here.
#[derive(Clone, Debug, Default)]
pub struct Board {
    /// Every piece of work it has been told of, in the order it began.
    tasks: Vec<Task>,
    /// What arrived about work it has not been told the start of, oldest
    /// first -- see [`HELD`].
    held: Vec<News>,
}

impl Board {
    /// Takes in one piece of news.
    ///
    /// Hands back the work it was about, where it was about work Obelus
    /// knows: what is drawn from the news -- the row of the call that
    /// started it -- needs the whole of it, and news carries only what
    /// changed.
    pub fn hear(&mut self, news: News) -> Option<&Task> {
        match news {
            News::Began {
                id,
                name,
                kind,
                about,
                call,
                output,
                stoppable,
            } => {
                // Said twice is said once: the second is the same work, and
                // two rows for it is a count that is wrong for its life.
                if let Some(at) = self.at(&id) {
                    let task = &mut self.tasks[at];
                    task.name = name;
                    task.kind = kind;
                    task.about = about.or(task.about.take());
                    task.call = call.or(task.call.take());
                    task.output = output.or(task.output.take());
                    task.stoppable = stoppable;
                } else {
                    self.tasks.push(Task {
                        id: id.clone(),
                        name,
                        kind,
                        about,
                        call,
                        output,
                        stoppable,
                        state: State::Running,
                        summary: None,
                        began: Instant::now(),
                        ended: None,
                        stopping: false,
                    });
                }
                // And what came about it before it began, in the order it
                // came.
                let (early, later): (Vec<News>, Vec<News>) = std::mem::take(&mut self.held)
                    .into_iter()
                    .partition(|held| held.id() == id);
                self.held = later;
                for news in early {
                    self.change(news);
                }
                self.find(&id)
            }
            changed @ News::Changed { .. } => {
                let id = changed.id().to_string();
                if self.at(&id).is_none() {
                    if self.held.len() >= HELD {
                        self.held.remove(0);
                    }
                    self.held.push(changed);
                    return None;
                }
                self.change(changed);
                self.find(&id)
            }
        }
    }

    /// Folds news about work it knows into it.
    fn change(&mut self, news: News) {
        let News::Changed {
            id,
            state,
            summary,
            about,
            call,
            output,
        } = news
        else {
            return;
        };
        let Some(at) = self.at(&id) else {
            return;
        };
        let task = &mut self.tasks[at];
        if let Some(state) = state {
            match (state.is_over(), task.ended) {
                (true, None) => task.ended = Some(Instant::now()),
                // Corrected back to running -- the extension reports a task
                // that left the agent's list as stopped, and takes it back
                // when the real end arrives.
                (false, Some(_)) => task.ended = None,
                _ => {}
            }
            if state.is_over() {
                task.stopping = false;
            }
            task.state = state;
        }
        if summary.is_some() {
            task.summary = summary;
        }
        if about.is_some() {
            task.about = about;
        }
        if call.is_some() {
            task.call = call;
        }
        if output.is_some() {
            task.output = output;
        }
    }

    /// Where one is, by the agent's id for it.
    fn at(&self, id: &str) -> Option<usize> {
        self.tasks.iter().position(|task| task.id == id)
    }

    /// One, by the agent's id for it.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// How many are still going, which is the number on the status row.
    #[must_use]
    pub fn running(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| !task.state.is_over())
            .count()
    }

    /// Every one it knows, the ones still going first and then the ones that
    /// ended, the most recent first in each.
    ///
    /// What a reader opening the list wants is what is running now; what
    /// ended is there to read what it said, and the last thing to end is
    /// the one they are most likely to be asking about.
    #[must_use]
    pub fn listed(&self) -> Vec<&Task> {
        let mut listed: Vec<&Task> = self.tasks.iter().rev().collect();
        listed.sort_by_key(|task| task.state.is_over());
        listed
    }

    /// Notes that Obelus has asked for one to stop.
    pub fn stopping(&mut self, id: &str) {
        if let Some(at) = self.at(id) {
            self.tasks[at].stopping = true;
        }
    }

    /// Notes that asking for it to stop came to nothing.
    pub fn not_stopping(&mut self, id: &str) {
        if let Some(at) = self.at(id) {
            self.tasks[at].stopping = false;
        }
    }

    /// Ends every one still going, because nothing will say they have.
    ///
    /// The agent is gone, or what told Obelus of them has been given up on;
    /// either way a task left running here is a task nothing will ever
    /// end, and a count that never goes down. Stopped rather than failed,
    /// because nothing said they failed.
    ///
    /// Hands back the calls whose rows still say their work goes on.
    pub fn end_all(&mut self) -> Vec<String> {
        let now = Instant::now();
        let mut calls = Vec::new();
        for task in &mut self.tasks {
            if task.state.is_over() {
                continue;
            }
            task.state = State::Stopped;
            task.ended = Some(now);
            task.stopping = false;
            calls.extend(task.call.clone());
        }
        self.held.clear();
        calls
    }

    /// Whether it has been told of any work at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn began(id: &str, call: Option<&str>) -> News {
        News::Began {
            id: id.to_string(),
            name: format!("task {id}"),
            kind: "shell".to_string(),
            about: None,
            call: call.map(str::to_string),
            output: None,
            stoppable: true,
        }
    }

    fn became(id: &str, state: State) -> News {
        News::Changed {
            id: id.to_string(),
            state: Some(state),
            summary: None,
            about: None,
            call: None,
            output: None,
        }
    }

    /// The number on the status row is what is still going, not everything
    /// ever started.
    ///
    /// Deliberate break: count every task in `running`, and the one that
    /// ended is still counted.
    #[test]
    fn only_what_is_still_going_is_counted() {
        let mut board = Board::default();
        board.hear(began("a", None));
        board.hear(began("b", None));
        board.hear(became("a", State::Done));
        assert_eq!(board.running(), 1);
    }

    /// What arrives before the work it is about is kept until the work
    /// begins, and applied then, in order.
    ///
    /// Deliberate break: drop a change for unknown work instead of holding
    /// it, and the task that ended before its start arrived is running for
    /// ever.
    #[test]
    fn what_arrives_early_waits_for_the_start() {
        let mut board = Board::default();
        assert!(board.hear(became("a", State::Failed)).is_none());
        let task = board.hear(began("a", None)).expect("it began");
        assert_eq!(task.state, State::Failed);
        assert_eq!(board.running(), 0);
    }

    /// What waits is capped, so a dialect speaking of work it never starts
    /// does not grow the board for ever.
    ///
    /// Deliberate break: take the cap out of `hear`, and the held list is as
    /// long as everything ever said.
    #[test]
    fn what_waits_is_capped() {
        let mut board = Board::default();
        for at in 0..HELD * 3 {
            board.hear(became(&format!("never-{at}"), State::Running));
        }
        assert_eq!(board.held.len(), HELD);
    }

    /// Ending everything ends only what was still going, and says which
    /// calls' rows were waiting on it.
    ///
    /// Deliberate break: have `end_all` mark every task, and the one that
    /// failed is written down as stopped.
    #[test]
    fn ending_everything_leaves_what_had_ended_alone() {
        let mut board = Board::default();
        board.hear(began("a", Some("call-a")));
        board.hear(began("b", Some("call-b")));
        board.hear(became("b", State::Failed));
        assert_eq!(board.end_all(), vec!["call-a".to_string()]);
        assert_eq!(
            board.find("b").map(|task| &task.state),
            Some(&State::Failed)
        );
        assert_eq!(board.running(), 0);
    }

    /// What is still going is listed before what has ended.
    ///
    /// Deliberate break: list in the order they began, and the running one
    /// started first is under the one that ended.
    #[test]
    fn what_is_going_is_listed_first() {
        let mut board = Board::default();
        board.hear(began("first", None));
        board.hear(began("second", None));
        board.hear(became("second", State::Done));
        let listed: Vec<&str> = board.listed().iter().map(|task| task.id.as_str()).collect();
        assert_eq!(listed, vec!["first", "second"]);
    }

    /// A state taken back -- reported stopped, then running again -- is not
    /// over.
    ///
    /// Deliberate break: leave `ended` set when a task goes back to running,
    /// and the list says how long ago it ended while it runs.
    #[test]
    fn a_state_taken_back_is_not_over() {
        let mut board = Board::default();
        board.hear(began("a", None));
        board.hear(became("a", State::Stopped));
        board.hear(became("a", State::Running));
        let task = board.find("a").expect("known");
        assert_eq!(task.ended, None);
        assert_eq!(board.running(), 1);
    }
}
