//! Talking to the active agent.
//!
//! One agent at a time, started when the reader first opens the view and
//! left running until they close obelus or choose another. The conversation
//! itself lives in [`Chat`] and outlives the view: escape hides it, and what
//! was said is still there when it comes back.
//!
//! What arrives from the agent is an [`Event::Acp`] like every other
//! background source, so nothing here waits on anything.

use crossterm::event::{KeyCode, KeyModifiers};

use super::*;
use crate::{
    acp,
    component::chat::{Chat, Speaker},
};

/// A form an agent asked the reader to fill in.
///
/// One field at a time, in the order the agent listed them: a list where
/// the answer is one of a few, the box where it is words. What has been
/// answered is kept here until the last field is, because the protocol
/// takes the whole form as one answer.
#[derive(Debug)]
pub struct Asking {
    /// The fields nobody has answered yet, the next one first.
    left: std::collections::VecDeque<acp::Field>,
    /// What has been answered, in the order it was.
    given: Vec<(String, acp::Reply)>,
    /// Where the answers go when the last one is in.
    answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
}

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
        self.talker.as_ref().and_then(acp::Talk::info).or(chosen)
    }

    /// Which way of working the agent is in, if it offers any.
    #[must_use]
    pub fn agent_mode(&self) -> Option<&acp::Mode> {
        self.talker.as_ref()?.mode()
    }

    /// The ways of working it offers.
    #[must_use]
    pub fn agent_modes(&self) -> &[acp::Mode] {
        self.talker.as_ref().map_or(&[], acp::Talk::modes)
    }

    /// The commands it says it takes.
    #[must_use]
    pub fn agent_orders(&self) -> &[acp::Order] {
        self.talker.as_ref().map_or(&[], acp::Talk::orders)
    }

    /// Moves to the agent's next way of working.
    pub(super) fn step_agent_mode(&mut self) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        talker.step_mode();
    }

    /// The settings it lets the reader change.
    #[must_use]
    pub fn agent_settings(&self) -> &[acp::Setting] {
        self.talker.as_ref().map_or(&[], acp::Talk::settings)
    }

    /// Puts the agent's settings up as a list.
    ///
    /// The compact list, over the conversation, like every other choice:
    /// each row is one setting and what it is on now, and choosing one
    /// opens its values.
    pub fn open_agent_settings(&mut self) {
        let items = self
            .agent_settings()
            .iter()
            .map(|setting| PickerItem {
                icon: None,
                label: setting.name.clone(),
                detail: setting.about.clone(),
                trailing: setting.current_name().map(str::to_string),
                value: PickerValue::AgentSetting(setting.id.clone()),
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask("how it works");
        picker.when_empty("this agent has nothing to change");
        self.picker = Some(picker);
    }

    /// And one setting's values, once one has been chosen.
    pub(super) fn open_agent_setting(&mut self, id: &str) {
        let Some(setting) = self.talker.as_ref().and_then(|talker| talker.setting(id)) else {
            return;
        };
        let question = setting.name.clone();
        let current = setting.current_name().map(str::to_string);
        let items = setting
            .values
            .iter()
            .map(|value| PickerItem {
                icon: None,
                label: value.name.clone(),
                detail: value.about.clone(),
                // The one that is on says so in words. A list where the
                // selected row and the current value look the same cannot
                // say which of the two it is showing.
                trailing: (value.id == setting.current).then(|| "current".to_string()),
                value: PickerValue::AgentValue {
                    setting: id.to_string(),
                    value: value.id.clone(),
                },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask(&question);
        picker.when_empty("this one has nothing to choose from");
        // Opened on what it is already on, so the list starts by saying
        // where the reader is rather than at whatever happens to be first.
        if let Some(current) = current {
            picker.prefer(current);
        }
        self.picker = Some(picker);
    }

    /// Asks for one of them to be put on one of its values.
    pub(super) fn set_agent_setting(&mut self, setting: &str, value: &str) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        let Some(known) = talker.setting(setting) else {
            return;
        };
        let (name, told) = (known.name.clone(), what_to_say(known, value));
        let chosen = match known.switch {
            true => acp::Chosen::Switch(value == "on"),
            false => acp::Chosen::Value(value.to_string()),
        };
        talker.set(setting, chosen);
        // In the transcript, because it is a thing the reader did to the
        // conversation: what the agent answers with is the whole set of
        // settings again, which is not something to show.
        self.chat.note(&format!("{name}: {told}"));
    }

    /// The setting a typed command names, if it names one.
    ///
    /// `/model` is the reason this exists. An agent's own answer to it is a
    /// dialog it cannot open -- Copilot says as much, in words, in the
    /// middle of the conversation -- while the same choice is already on
    /// offer as a setting. So a command that is the name of a setting opens
    /// that setting's list instead of being sent.
    fn setting_named(&self, name: &str) -> Option<String> {
        let name = name.to_lowercase();
        let settings = self.agent_settings();
        settings
            .iter()
            .find(|setting| setting.id.to_lowercase() == name)
            .or_else(|| {
                settings
                    .iter()
                    .find(|setting| setting.name.to_lowercase() == name)
            })
            .map(|setting| setting.id.clone())
    }

    /// Sends what the reader typed.
    pub(super) fn send_to_agent(&mut self, text: &str) {
        // Unless a field of a form is waiting for it: the agent asked for
        // words and these are the words, so they go back as the answer
        // rather than out as a message.
        if self.is_answering() {
            self.answer_typed(text);
            return;
        }
        // A command that is a setting's name is a choice to be made here,
        // not a message: nothing goes in the transcript and nothing is sent.
        if let Some(name) = text.trim().strip_prefix('/')
            && !name.contains(char::is_whitespace)
            && let Some(id) = self.setting_named(name)
        {
            self.open_agent_setting(&id);
            return;
        }
        self.chat.asked(text);
        if self.talker.is_none() {
            self.start_agent();
        }
        let Some(talker) = self.talker.as_mut() else {
            // `start_agent` has already said why in the transcript.
            return;
        };
        // Held until the session opens, which is the ordinary case for the
        // first thing said: the reader typed while it was starting, and the
        // handle sends it when there is somewhere to send it.
        talker.say(text);
    }

    /// Asks the agent to stop what it is doing.
    pub(super) fn interrupt_agent(&mut self) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        talker.interrupt();
    }

    /// Stops the agent, if one is running.
    pub(super) fn stop_agent(&mut self) {
        if let Some(mut talker) = self.talker.take() {
            talker.shutdown();
        }
        self.permission = None;
        // Dropped rather than answered: there is no longer anything to
        // answer, and dropping is what tells the other side so.
        self.asking = None;
    }

    /// The agent's own commands, while one is being typed.
    ///
    /// A list of them is the ordinary compact list -- the same rows, the
    /// same chosen row, the same marking of what matched -- rather than
    /// something drawn for this one screen. What it does not have is the
    /// keys: the box below it owns those, because that is where the reader
    /// is typing, and this list follows what they type.
    #[must_use]
    pub fn slash(&self) -> Option<&Picker> {
        self.slash.as_ref()
    }

    /// Builds or refreshes that list, once a frame.
    ///
    /// It exists exactly while a command's *name* is being typed: a slash
    /// opens it, a blank after the name settles it and closes it, and
    /// rubbing the slash out closes it too.
    pub(super) fn refresh_slash(&mut self) {
        let name = self
            .chat()
            .filter(|_| !self.agent_orders().is_empty())
            .and_then(Chat::typing_command);
        let Some(name) = name else {
            self.slash = None;
            return;
        };
        if let Some(slash) = self.slash.as_mut() {
            if slash.query() != name {
                slash.set_query(&name);
            }
            // A list of nothing is not a list. It also has to stop being
            // one: a name that matches no command is an ordinary message
            // as far as the box is concerned, and a list that stayed would
            // swallow the enter that sends it.
            if slash.match_count() == 0 {
                self.slash = None;
            }
            self.settle_slash();
            return;
        }
        let items = self
            .agent_orders()
            .iter()
            .map(|order| PickerItem {
                // No glyph: a column of the same one down a list says
                // nothing, and the slash in front of the name is what says
                // what these rows are.
                icon: None,
                label: format!("/{}", order.name),
                detail: Some(order.description.clone()),
                trailing: order.hint.clone(),
                value: PickerValue::Nothing,
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut slash = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        slash.set_query(&name);
        if slash.match_count() > 0 {
            self.slash = Some(slash);
        }
        self.settle_slash();
    }

    /// Gives that list the geometry it is about to be drawn in.
    ///
    /// The same thing [`App::prepare`] does for the other list, and for the
    /// same two reasons: the window follows the selection only when it
    /// knows how many rows are on screen, and the matched characters are
    /// worked out for the rows that will be drawn. Without it the list
    /// neither scrolls nor says what the query matched -- it is the picker,
    /// so it needs what the picker needs.
    fn settle_slash(&mut self) {
        let rows = self.slash.as_ref().zip(self.chat()).map(|(slash, chat)| {
            let view = ui::picker::PickerView::over(slash, self.theme());
            view.rows_region(view.region(ui::chat::above_writing(self.editor_area, chat)))
                .height
        });
        if let (Some(rows), Some(slash)) = (rows, self.slash.as_mut()) {
            slash.refresh_indices(rows);
        }
    }

    /// Whatever a key means to that list, if it means anything.
    ///
    /// Only the keys that move about a list and the ones that choose from
    /// it. Everything else -- every character, every line break -- belongs
    /// to the box, which is what makes the list a list of what is being
    /// typed rather than a mode the reader is in.
    pub(super) fn slash_key(&mut self, key: &KeyEvent) -> bool {
        let Some(slash) = self.slash.as_mut() else {
            return false;
        };
        let Some(modifiers) = keymap::modifiers_of(key) else {
            return false;
        };
        if modifiers != KeyModifiers::NONE {
            return false;
        }
        match key.code {
            // The window is not moved here: the frame that follows settles
            // it, which is the one place that knows how many rows are on
            // screen.
            KeyCode::Up => {
                slash.move_selection_by(-1);
                true
            }
            KeyCode::Down => {
                slash.move_selection_by(1);
                true
            }
            // Enter chooses from the list, like enter chooses in every other
            // list. What sends the message is enter *after* the name is
            // settled, by which time there is no list.
            KeyCode::Tab | KeyCode::Enter => {
                let chosen = slash.selected_item().map(|item| item.label.clone());
                if let Some(name) = chosen {
                    // A command that names a setting is that setting's
                    // list: the reader means the choice, and making them
                    // press enter twice to reach it is obelus being
                    // pedantic about which of its own lists they are in.
                    if let Some(id) = self.setting_named(name.trim_start_matches('/')) {
                        self.chat.put("");
                        self.slash = None;
                        self.open_agent_setting(&id);
                        return true;
                    }
                    // The name and a blank after it: the blank is what
                    // settles the name, so the list is done and whatever
                    // the command takes is typed next.
                    self.chat.put(&format!("{name} "));
                    self.slash = None;
                }
                true
            }
            // The list, not the conversation: escape gives up on the
            // nearest thing first, and what the reader typed stays.
            KeyCode::Esc => {
                self.slash = None;
                true
            }
            _ => false,
        }
    }

    /// Whether a field of a form is waiting for words to be typed.
    ///
    /// Which is the one state where what is typed in the box is not a
    /// message: the agent asked for something, and this is it.
    #[must_use]
    pub fn is_answering(&self) -> bool {
        self.asking
            .as_ref()
            .and_then(|asking| asking.left.front())
            .is_some_and(|field| {
                matches!(
                    field.takes,
                    acp::Takes::Words(_) | acp::Takes::Number { .. }
                )
            })
    }

    /// Whether the agent is waiting on an answer to something it asked.
    #[must_use]
    pub const fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Puts a form the agent asked for to the reader.
    fn ask_reader(
        &mut self,
        message: &str,
        fields: Vec<acp::Field>,
        answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
    ) {
        self.chat.note(&format!("it asks: {message}"));
        self.asking = Some(Asking {
            left: fields.into(),
            given: Vec::new(),
            answer,
        });
        self.put_the_question();
    }

    /// Puts the next field, or answers the form when there is none left.
    fn put_the_question(&mut self) {
        let Some(field) = self
            .asking
            .as_ref()
            .and_then(|asking| asking.left.front())
            .cloned()
        else {
            self.settle_asking();
            return;
        };
        match &field.takes {
            // One of a few: the list, like every other choice.
            acp::Takes::One(values) => {
                let picker = self.list_of(&field, values, None);
                self.picker = Some(picker);
            }
            acp::Takes::Switch(on) => {
                let sides = [
                    acp::Value {
                        id: "on".to_string(),
                        name: "on".to_string(),
                        about: None,
                    },
                    acp::Value {
                        id: "off".to_string(),
                        name: "off".to_string(),
                        about: None,
                    },
                ];
                let side = match on {
                    true => "on",
                    false => "off",
                };
                let picker = self.list_of(&field, &sides, Some(side));
                self.picker = Some(picker);
            }
            // Words: the box, which is where words are typed. What the
            // reader types next goes back as the answer rather than to the
            // agent as a message.
            acp::Takes::Words(suggested) => {
                self.chat.note(&question(&field));
                if let Some(words) = suggested {
                    self.chat.put(words);
                }
            }
            acp::Takes::Number { .. } => self.chat.note(&question(&field)),
        }
    }

    /// One field's values, as the list obelus puts every choice in.
    fn list_of(&self, field: &acp::Field, values: &[acp::Value], on: Option<&str>) -> Picker {
        let items = values
            .iter()
            .map(|value| PickerItem {
                icon: None,
                label: value.name.clone(),
                detail: value.about.clone(),
                trailing: (Some(value.id.as_str()) == on).then(|| "now".to_string()),
                value: PickerValue::AgentAsked {
                    field: field.name.clone(),
                    value: value.id.clone(),
                },
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.ask(&field.title);
        picker.when_empty("it offered nothing to choose from");
        if let Some(on) = on
            && let Some(name) = values.iter().find(|value| value.id == on)
        {
            picker.prefer(name.name.clone());
        }
        picker
    }

    /// Takes a row the reader chose as one field's answer.
    pub(super) fn answer_asked(&mut self, field: &str, value: &str) {
        let Some(asking) = self.asking.as_mut() else {
            return;
        };
        let Some(asked) = asking.left.front().filter(|asked| asked.name == field) else {
            // A row from a list that is no longer the question. Nothing to
            // do with it: the form moved on, or was given up on.
            return;
        };
        let (reply, said) = match &asked.takes {
            acp::Takes::Switch(_) => (acp::Reply::Switch(value == "on"), value.to_string()),
            // The name for the transcript, not the id: the id is the
            // agent's word for it and can be anything.
            acp::Takes::One(values) => (
                acp::Reply::Value(value.to_string()),
                values
                    .iter()
                    .find(|known| known.id == value)
                    .map_or(value, |known| known.name.as_str())
                    .to_string(),
            ),
            // A list cannot answer these, so a row from one is not theirs.
            acp::Takes::Words(_) | acp::Takes::Number { .. } => return,
        };
        let title = asked.title.clone();
        asking.given.push((field.to_string(), reply));
        asking.left.pop_front();
        self.chat.note(&format!("{title}: {said}"));
        self.put_the_question();
    }

    /// Takes what the reader typed as one field's answer.
    fn answer_typed(&mut self, text: &str) {
        let Some(asking) = self.asking.as_mut() else {
            return;
        };
        let Some(asked) = asking.left.front() else {
            return;
        };
        let reply = match &asked.takes {
            acp::Takes::Words(_) => acp::Reply::Words(text.to_string()),
            acp::Takes::Number { whole, least, most } => {
                let Ok(number) = text.trim().parse::<f64>() else {
                    // The reader's slip, so it is said and asked again:
                    // an answer nobody can give is worse than a question
                    // asked twice.
                    let title = asked.title.clone();
                    self.chat
                        .note(&format!("{title} takes a number, not {text:?}"));
                    return;
                };
                if least.is_some_and(|least| number < least)
                    || most.is_some_and(|most| number > most)
                {
                    let question = question(asked);
                    self.chat.note(&format!("that is outside {question}"));
                    return;
                }
                match whole {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a whole number the reader typed, and the protocol takes an i64"
                    )]
                    true => acp::Reply::Whole(number as i64),
                    false => acp::Reply::Number(number),
                }
            }
            // A typed answer to a question that is a list is not an answer.
            acp::Takes::One(_) | acp::Takes::Switch(_) => return,
        };
        let name = asked.name.clone();
        asking.given.push((name, reply));
        asking.left.pop_front();
        // Theirs, in the transcript, because that is what they said -- the
        // agent asked in words and this is the answer in words.
        self.chat.asked(text);
        self.put_the_question();
    }

    /// Answers the form, now that every field has one.
    fn settle_asking(&mut self) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        if asking.answer.send(Some(asking.given)).is_err() {
            self.chat.note("it stopped waiting for an answer");
        }
    }

    /// Says no to the form, whichever field the reader was on.
    pub(super) fn refuse_asking(&mut self) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        self.chat.note("not answered");
        let _ = asking.answer.send(None);
    }

    /// Follows the transcript, and notices an agent that has died.
    ///
    /// Once a frame, like the language servers' own check: an agent that
    /// has exited is not otherwise noticed -- its reader thread stops, and
    /// every prompt after that goes unanswered with nothing to say so.
    pub(super) fn settle_chat(&mut self, editor_area: Rect) {
        // Nothing to check: the thread says when the conversation has
        // ended, and `on_acp` puts that in the transcript once.
        if !self.showing_chat {
            return;
        }
        let width = crate::ui::chat::writing_width(editor_area);
        let needed = self.chat.writing().rows(width).len();
        let region = crate::ui::chat::regions(editor_area, needed).transcript;
        let rows = self.chat.rows(region.width.saturating_sub(4)).len();
        self.chat.settle(rows, region.height);
    }

    /// Takes one message from the agent.
    pub(super) fn on_acp(&mut self, incoming: acp::Incoming) {
        let Some(talker) = self.talker.as_mut() else {
            return;
        };
        // What the protocol needs of it is dealt with in there -- the
        // handshake, the session, which mode is on -- and what a reader
        // needs to see comes back.
        let Some(incoming) = talker.on(incoming) else {
            return;
        };
        match incoming {
            acp::Incoming::Update(update) => match update {
                acp::Update::Said(text) => self.chat.chunk(Speaker::Agent, &text),
                acp::Update::Thought(text) => self.chat.chunk(Speaker::Thought, &text),
                acp::Update::Tool { id, title, status } => self.chat.tool(&id, &title, &status),
                // Kept by the handle, which is where the view reads them:
                // these are facts about the agent rather than things it
                // said, and a transcript with them in it is a log.
                acp::Update::Mode(_) | acp::Update::Orders(_) | acp::Update::Settings(_) => {}
            },
            acp::Incoming::Ended(reason) => {
                // Only the ends that are not the ordinary one: a turn that
                // finished has its answer above it, and "end turn" under
                // every answer is noise.
                match reason.as_str() {
                    "endturn" | "end_turn" => {}
                    "cancelled" => self.chat.note("stopped"),
                    "refusal" => self.chat.note("it declined to answer"),
                    "maxtokens" | "max_tokens" => {
                        self.chat.note("it ran out of room to answer in");
                    }
                    other => self.chat.note(other),
                }
            }
            acp::Incoming::Failed(what, why) => {
                tracing::warn!(what, why, "the agent");
                self.chat.note(&format!("{what}: {why}"));
            }
            acp::Incoming::Permission {
                title,
                options,
                answer,
            } => self.ask_permission(&title, &options, answer),
            acp::Incoming::Ask {
                message,
                fields,
                answer,
            } => self.ask_reader(&message, fields, answer),
            acp::Incoming::Read {
                path,
                line,
                limit,
                answer,
            } => self.read_for_agent(&path, line, limit, answer),
            acp::Incoming::Gone(why) => {
                if let Some(why) = why {
                    tracing::warn!(why, "the conversation ended");
                    self.chat.note(&format!("the agent stopped: {why}"));
                } else {
                    self.chat.note("the agent stopped");
                }
            }
            // Folded into the handle above.
            acp::Incoming::Ready(_) | acp::Incoming::Started { .. } => {}
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
        tracing::info!(id, command = %command.display(), "starting an agent");
        // Nothing to fail here: the process is started on the thread, and
        // an agent that will not run says so as the conversation ending
        // with a reason -- which is the same path as one that dies later.
        self.talker = Some(acp::Talk::start(
            id,
            command,
            arguments,
            &self.working_directory,
            sender,
        ));
        self.said_it_died = false;
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
    fn ask_permission(
        &mut self,
        title: &str,
        options: &[acp::Choice],
        answer: acp::Answer<Option<String>>,
    ) {
        self.chat
            .note(&format!("asking to {}", title.to_lowercase()));
        let items = options
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
        picker.ask(title);
        self.permission = Some(answer);
        self.picker = Some(picker);
    }

    /// Answers the permission request the reader chose an option for.
    pub(super) fn allow(&mut self, option: &str) {
        let Some(answer) = self.permission.take() else {
            return;
        };
        if answer.send(Some(option.to_string())).is_err() {
            self.chat.note("it stopped waiting for an answer");
        }
    }

    /// Tells the agent the reader would not answer.
    ///
    /// The protocol has an outcome for it, and it matters: an agent whose
    /// request is never answered waits for ever, and one that is told it
    /// was cancelled ends the turn and says so.
    pub(super) fn refuse_permission(&mut self) {
        let Some(answer) = self.permission.take() else {
            return;
        };
        let _ = answer.send(None);
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
    fn read_for_agent(
        &mut self,
        path: &Path,
        line: Option<u32>,
        limit: Option<u32>,
        answer: acp::Answer<Option<String>>,
    ) {
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

        // A line and a limit, when it asked for them: an agent reading a
        // large file asks for a window of it, and answering with the whole
        // thing is a different answer.
        let _ = answer.send(text.map(|text| window(&text, line, limit)));
    }
}

/// One field, as a question in the transcript.
///
/// The title, and what it will take: a number with bounds is a question
/// that has to say them, because a reader who types the wrong one only
/// finds out afterwards.
fn question(field: &acp::Field) -> String {
    let mut asked = field.title.clone();
    if let acp::Takes::Number { whole, least, most } = field.takes {
        let kind = match whole {
            true => "a whole number",
            false => "a number",
        };
        let bounds = match (least, most) {
            (Some(least), Some(most)) => format!(": {kind} from {least} to {most}"),
            (Some(least), None) => format!(": {kind}, {least} or more"),
            (None, Some(most)) => format!(": {kind}, {most} or less"),
            (None, None) => format!(": {kind}"),
        };
        asked.push_str(&bounds);
    }
    match field.about.as_deref() {
        Some(about) => format!("{asked} \u{2014} {about}"),
        None => asked,
    }
}

/// What to call a value in the transcript.
///
/// Its name, or its id if the agent offered one it does not list -- which is
/// still what the reader chose.
fn what_to_say(setting: &acp::Setting, value: &str) -> String {
    setting
        .values
        .iter()
        .find(|known| known.id == value)
        .map_or(value, |known| known.name.as_str())
        .to_string()
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
