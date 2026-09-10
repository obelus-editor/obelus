//! Talking to the active agent.
//!
//! One agent at a time, started when the reader first opens the view and
//! left running until they close obelus or choose another. The conversation
//! itself lives in [`Chat`] and outlives the view: escape hides it, and what
//! was said is still there when it comes back.
//!
//! What arrives from the agent is an [`Event::Acp`] like every other
//! background source, so nothing here waits on anything.

use serde_json::{Value, json};

use super::*;
use crate::{
    acp,
    component::chat::{Chat, Speaker},
};

/// What obelus is doing about an agent, for the view to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Talking {
    /// No agent has been chosen.
    Nobody,
    /// One has been chosen and is not running: nothing has needed it yet.
    Idle,
    /// Starting, or opening a session.
    Starting,
    /// There is a session, and it is waiting to be asked something.
    Ready,
    /// It is working on a prompt.
    Thinking,
    /// It was running and has stopped.
    Gone,
}

impl App {
    /// Opens the conversation, starting the agent if it is not running.
    ///
    /// Not a buffer: it is a region over the editor, escape closes it, and
    /// what was said stays. So this is a flag and a process, not a document
    /// to open.
    pub fn open_agent(&mut self) {
        self.showing_chat = true;
        // Everything else on screen is something else the reader was
        // looking at, and the conversation is the whole region now.
        self.picker = None;
        self.settings = None;
        if self.talker.is_none() {
            self.start_agent();
        }
    }

    /// Hides the conversation, keeping it.
    pub(super) fn close_chat(&mut self) {
        self.showing_chat = false;
    }

    /// The conversation, while it is what the reader is looking at.
    #[must_use]
    pub fn chat(&self) -> Option<&Chat> {
        self.showing_chat.then_some(&self.chat)
    }

    /// What obelus is doing about an agent.
    #[must_use]
    pub fn talking(&self) -> Talking {
        let Some(talker) = self.talker.as_ref() else {
            return match self.config.agent.as_deref() {
                None | Some("") => Talking::Nobody,
                Some(_) => Talking::Idle,
            };
        };
        if talker.has_exited() {
            Talking::Gone
        } else if talker.is_thinking() {
            Talking::Thinking
        } else if talker.is_started() {
            Talking::Ready
        } else {
            Talking::Starting
        }
    }

    /// What to call the agent on screen.
    ///
    /// What it calls itself once it has said, and what the registry called
    /// it until then: a name that appears only after the handshake is a
    /// header that changes under the reader.
    #[must_use]
    pub fn agent_name(&self) -> Option<&str> {
        let chosen = match self.config.agent.as_deref() {
            None | Some("") => None,
            Some(id) => Some(id),
        };
        self.talker.as_ref().and_then(acp::Client::info).or(chosen)
    }

    /// Sends what the reader typed.
    pub(super) fn send_to_agent(&mut self, text: &str) {
        self.chat.asked(text);
        if self.talker.is_none() {
            self.start_agent();
        }
        let Some(talker) = self.talker.as_mut() else {
            // `start_agent` has already said why in the transcript.
            return;
        };
        match talker.say(text) {
            // Held until the session opens, which is the ordinary case for
            // the first thing said: the reader typed while it was starting.
            Ok(_) => {}
            Err(error) => {
                let why = error.to_string();
                self.chat.note(&format!("could not send that: {why}"));
            }
        }
    }

    /// Asks the agent to stop what it is doing.
    pub(super) fn interrupt_agent(&mut self) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        if let Err(error) = talker.interrupt() {
            let why = error.to_string();
            self.chat.note(&format!("could not stop it: {why}"));
        }
    }

    /// Stops the agent, if one is running.
    pub(super) fn stop_agent(&mut self) {
        if let Some(mut talker) = self.talker.take() {
            talker.shutdown();
        }
        self.permission = None;
    }

    /// Follows the transcript, and notices an agent that has died.
    ///
    /// Once a frame, like the language servers' own check: an agent that
    /// has exited is not otherwise noticed -- its reader thread stops, and
    /// every prompt after that goes unanswered with nothing to say so.
    pub(super) fn settle_chat(&mut self, editor_area: Rect) {
        if let Some(talker) = self.talker.as_mut()
            && !talker.check_alive()
            && !self.said_it_died
        {
            self.said_it_died = true;
            self.chat.note("the agent stopped");
        }
        if !self.showing_chat {
            return;
        }
        let region = crate::ui::chat::transcript(editor_area);
        let rows = self.chat.rows(region.width.saturating_sub(4)).len();
        self.chat.settle(rows, region.height);
    }

    /// Takes one message from the agent.
    pub(super) fn on_acp(&mut self, message: Value) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        match talker.on_message(&message) {
            acp::Incoming::Nothing | acp::Incoming::Ready => {}
            acp::Incoming::Started => {
                tracing::info!("the agent opened a session");
            }
            acp::Incoming::Update(update) => match update {
                acp::Update::Said(text) => self.chat.chunk(Speaker::Agent, &text),
                acp::Update::Thought(text) => self.chat.chunk(Speaker::Thought, &text),
                acp::Update::Tool { id, title, status } => self.chat.tool(&id, &title, &status),
            },
            acp::Incoming::Ended(reason) => {
                // Only the ends that are not the ordinary one: a turn that
                // finished has its answer above it, and "end_turn" under
                // every answer is noise.
                match reason.as_str() {
                    "end_turn" => {}
                    "cancelled" => self.chat.note("stopped"),
                    "refusal" => self.chat.note("it declined to answer"),
                    "max_tokens" => self.chat.note("it ran out of room to answer in"),
                    other => self.chat.note(other),
                }
            }
            acp::Incoming::Failed(what, why) => {
                tracing::warn!(what, why, "the agent");
                self.chat.note(&format!("{what}: {why}"));
            }
            acp::Incoming::Permission(permission) => self.ask_permission(permission),
            acp::Incoming::Read {
                id,
                path,
                line,
                limit,
            } => self.read_for_agent(&id, &path, line, limit),
        }
    }

    /// Starts an agent, whatever it is and wherever it came from.
    ///
    /// The seam between "which agent" and "talking to one": what is above
    /// this reads the reader's choice out of the settings, and what is below
    /// it only needs a command. Public because it is also how a test gets a
    /// conversation without a registry, an install, or a network.
    pub fn talk_to(&mut self, id: &str, command: &Path, arguments: &[String]) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        match acp::Client::start(id, command, arguments, &self.working_directory, sender) {
            Ok(talker) => {
                tracing::info!(id, command = %command.display(), "starting an agent");
                self.talker = Some(talker);
                self.said_it_died = false;
            }
            Err(error) => {
                let why = error.to_string();
                self.chat.note(&format!("could not start {id}: {why}"));
            }
        }
    }

    /// Starts the active agent, or says why it cannot.
    fn start_agent(&mut self) {
        let Some(id) = self.config.agent.clone().filter(|id| !id.is_empty()) else {
            return;
        };
        let Some(root) = crate::agent::root() else {
            self.chat
                .note("this system has nowhere for obelus to keep an agent");
            return;
        };
        let Some((command, arguments)) = crate::agent::remembered(&id, &root) else {
            self.chat.note(&format!(
                "{id} is not installed here any more \u{2014} open the settings and install it again"
            ));
            return;
        };
        self.talk_to(&id, &command, &arguments);
    }

    /// Puts a permission request to the reader, as a list.
    ///
    /// The compact picker, which is what every other choice in obelus is: a
    /// list of named things with one selected, filtered by typing. A dialog
    /// of its own would be a second way to choose something.
    fn ask_permission(&mut self, permission: acp::Permission) {
        self.chat
            .note(&format!("asking to {}", permission.title.to_lowercase()));
        let items = permission
            .options
            .iter()
            .map(|choice| PickerItem {
                icon: icons::enabled().then(|| icons::for_permission(&choice.kind)),
                label: choice.name.clone(),
                detail: None,
                trailing: None,
                value: PickerValue::Permission(choice.id.clone()),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask(&permission.title);
        self.permission = Some(permission.id);
        self.picker = Some(picker);
    }

    /// Answers the permission request the reader chose an option for.
    pub(super) fn allow(&mut self, option: &str) {
        let Some(id) = self.permission.take() else {
            return;
        };
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        let outcome = json!({ "outcome": "selected", "optionId": option });
        if let Err(error) = talker.answer(&id, json!({ "outcome": outcome })) {
            let why = error.to_string();
            self.chat.note(&format!("could not answer that: {why}"));
        }
    }

    /// Tells the agent the reader would not answer.
    ///
    /// The protocol has an outcome for it, and it matters: an agent whose
    /// request is never answered waits for ever, and one that is told it
    /// was cancelled ends the turn and says so.
    pub(super) fn refuse_permission(&mut self) {
        let Some(id) = self.permission.take() else {
            return;
        };
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        let _ = talker.answer(&id, json!({ "outcome": { "outcome": "cancelled" } }));
        self.chat.note("not answered");
    }

    /// Whether a permission request is waiting on the reader.
    #[must_use]
    pub fn is_asking_permission(&self) -> bool {
        self.permission.is_some()
    }

    /// Answers the agent's request for a file's text.
    ///
    /// From an open buffer when obelus has one, because what the reader is
    /// looking at is not always what is on disk -- and the whole point of an
    /// agent inside a reader is that they are looking at the same thing.
    /// Otherwise from disk.
    ///
    /// Refused outside the project, whichever way the text would have come:
    /// an agent asking for something outside the tree obelus was started on
    /// is asking for something the reader did not open it to look at.
    fn read_for_agent(&mut self, id: &Value, path: &Path, line: Option<u32>, limit: Option<u32>) {
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_directory.join(path)
        };
        let inside = full
            .canonicalize()
            .ok()
            .is_some_and(|full| full.starts_with(&self.working_directory));
        let text = if inside {
            self.buffers
                .iter()
                .flatten()
                .find(|buffer| buffer.path() == full)
                .map(|buffer| buffer.text().rope().to_string())
                .or_else(|| std::fs::read_to_string(&full).ok())
        } else {
            None
        };

        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        let Some(text) = text else {
            let _ = talker.refuse(id, -32602, "obelus will not read that");
            return;
        };
        // A line and a limit, when it asked for them: an agent reading a
        // large file asks for a window of it, and answering with the whole
        // thing is a different answer.
        let content = window(&text, line, limit);
        let _ = talker.answer(id, json!({ "content": content }));
    }
}

/// The lines of `text` an agent asked for.
///
/// `line` is counted from one, which is the protocol's own choice and not
/// obelus's: the file's first line is line 1.
fn window(text: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let first = line.unwrap_or(1).max(1) as usize - 1;
    let taken = limit.map_or(usize::MAX, |limit| limit as usize);
    let mut out = String::new();
    for row in text.lines().skip(first).take(taken) {
        out.push_str(row);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::window;

    /// The protocol counts a file's lines from one, and asking for line 2 of
    /// a three-line file has to give the second line rather than the third.
    #[test]
    fn a_window_starts_at_the_line_it_was_asked_for() {
        let text = "one\ntwo\nthree\n";
        assert_eq!(window(text, None, None), text);
        assert_eq!(window(text, Some(2), None), "two\nthree\n");
        assert_eq!(window(text, Some(1), Some(1)), "one\n");
        assert_eq!(window(text, Some(2), Some(1)), "two\n");
        // A line past the end is no lines, not a panic.
        assert_eq!(window(text, Some(9), Some(1)), "");
        // Line zero does not exist; the first line is what was meant.
        assert_eq!(window(text, Some(0), Some(1)), "one\n");
    }
}
