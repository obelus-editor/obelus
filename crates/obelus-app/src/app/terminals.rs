//! A terminal of Obelus's own, as somewhere the reader goes.
//!
//! **What is typed in a terminal is the program's.** Every other document
//! hands the key table what it does not want; a terminal hands it only what
//! `Context::Terminal` binds -- the palette, paste (`ctrl+v` among them),
//! `ctrl+w` to close it as anything is closed, leaving Obelus, and the
//! function keys -- and writes everything else down the pty, escape and
//! `ctrl+c` included, because those are a shell's before they are
//! anybody's. So the mnemonic layout's keys for the views go to the program
//! here, where the function keys they stand in for do not: every one of
//! them is a chord a shell already has. Once the program has ended there is
//! nothing to type to, and the keys go back to Obelus.
//!
//! **The pointer is the program's where it asked for it, and the reader's
//! otherwise.** A program that asked to hear the pointer -- an editor, a
//! process list -- is told what the left button and the wheel do, the way
//! it asked to be told; one that did not leaves a drag to take hold of what
//! it printed, and the wheel to read back up it. Shift held is the
//! reader's whatever the program asked, which is what it is in every
//! terminal. And the key that copies copies where something is held --
//! `ctrl+c` included, which is the shell's interrupt everywhere else, and
//! the key a desktop sends a window for its own copy.
//!
//! Two things open one. The reader's own shell, from `open-terminal`, which
//! starts in the project. And an agent's sign-in, where the agent says that
//! signing in is a program to run: the reader answers it there -- which
//! account, a code pasted back -- and its ending well is the sign-in, so
//! the terminal closes and the conversation that was waiting goes on. One
//! that ends badly stays open with what it said on it, because a failed
//! sign-in is something the reader has to read.

use obelus_component::question::{Answer, Closing, Question};
use obelus_terminal::{Ended, Heard, Mouse, Program, Terminal};

use super::*;

/// A sign-in running in a terminal, and who is waiting on it.
#[derive(Debug)]
pub(super) struct SigningIn {
    /// Which terminal it is running in.
    terminal: obelus_terminal::Id,
    /// The conversation that asked, which is where the reader goes back to.
    conversation: DocumentId,
    /// Which connection asked: a sign-in for an agent that has since been
    /// started again is a sign-in for nobody.
    connection: obelus_agent::acp::Connection,
}

impl App {
    /// Starts the reader's own shell, and goes to it.
    pub fn open_terminal(&mut self) {
        let from = self.here();
        let program = match &self.terminal.shell {
            Some(shell) => Program::Command {
                program: shell.clone(),
                arguments: Vec::new(),
                env: Vec::new(),
            },
            None => Program::Shell,
        };
        match self.start_terminal(&program) {
            Ok(id) => {
                self.record(from);
                self.go_to_document(id);
            }
            Err(why) => self.wrong(format!("The terminal would not start: {why}")),
        }
    }

    /// Starts a program in a terminal of its own, and says where it landed
    /// -- or why it would not start, which the caller says where its reader
    /// is looking.
    ///
    /// In the project, at the size the document region is now -- the next
    /// frame makes it whatever size it is drawn at, and a program told the
    /// wrong size first draws its first screen twice.
    pub(super) fn start_terminal(&mut self, program: &Program) -> Result<DocumentId, String> {
        // Only a test driving its own events has no loop, and a program
        // whose words would go nowhere is not one to start.
        let Some(events) = self.events.clone() else {
            return Err("nothing is listening for what it writes".to_string());
        };
        self.terminal.terminals += 1;
        let size = (self.editor_area.height, self.editor_area.width);
        match Terminal::start(
            self.terminal.terminals,
            program,
            &self.working_directory,
            size,
            events,
        ) {
            Ok(terminal) => {
                tracing::info!(said = terminal.said(), "a terminal started");
                self.documents.push(Some(Document::from(terminal)));
                Ok(DocumentId::new(self.documents.len() - 1))
            }
            Err(why) => {
                tracing::warn!(%why, "a terminal would not start");
                Err(why)
            }
        }
    }

    /// Starts this shell for `open-terminal` rather than the reader's own.
    pub fn shell_for_test(&mut self, shell: PathBuf) {
        self.terminal.shell = Some(shell);
    }

    /// The terminal being read, if that is what is being read.
    #[must_use]
    pub fn terminal(&self) -> Option<&Terminal> {
        self.document(self.current?)?.terminal()
    }

    /// And to type to it.
    pub(super) fn terminal_mut(&mut self) -> Option<&mut Terminal> {
        let current = self.current?;
        self.document_mut(current)?.terminal_mut()
    }

    /// Whether what is being read is a terminal with its program running,
    /// which is what decides whose the keys are.
    pub(super) fn typing_to_a_program(&self) -> bool {
        self.terminal()
            .is_some_and(|terminal| terminal.ended().is_none())
    }

    /// A key, to the program -- unless the terminal has kept it for Obelus.
    pub(super) fn terminal_key(&mut self, key: &KeyEvent) -> bool {
        // Reading back up what went by, which is the reader's whether or
        // not the program is still there to type to.
        if self
            .terminal_mut()
            .is_some_and(|terminal| terminal.read_back(key))
        {
            return true;
        }
        if !self.typing_to_a_program() {
            return false;
        }
        // The key that copies, with something held: it copies. Without, it
        // is the program's -- `ctrl+c` is a shell's interrupt, and a reader
        // who has just taken hold of some words is not interrupting
        // anything. Which also makes the desktop's own copy work here,
        // which sends a window `ctrl+c`.
        if self.terminal().is_some_and(Terminal::is_holding)
            && self.keymap.lookup(key, Context::Normal) == Some(Command::SelectionCopy)
        {
            self.copy_selection();
            return true;
        }
        // Kept, and handed back to the key table, which reads the same
        // context and finds it.
        if self.keymap.lookup(key, Context::Terminal).is_some() {
            return false;
        }
        if let Some(terminal) = self.terminal_mut() {
            terminal.key(key);
        }
        true
    }

    /// What the pointer did over the terminal being read.
    ///
    /// To the program, where it asked for the pointer and is still there to
    /// hear it; otherwise a selection of Obelus's own, by cells. Shift held
    /// is a selection whatever the program asked for, which is what it is
    /// in every terminal: a program that has taken the pointer has taken
    /// the reader's only other way to copy what it printed.
    pub(super) fn pointer_in_terminal(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = obelus_ui::editor_canvas(self.screen_area);
        if area.is_empty() {
            return;
        }
        let inside = area.contains(ratatui::layout::Position::new(x, y));
        // A press outside is not about the terminal; a drag that has left
        // it is still holding, at its edge.
        if !inside && kind != Pointer::Dragged {
            return;
        }
        let at = (
            y.clamp(area.y, area.bottom() - 1) - area.y,
            x.clamp(area.x, area.right() - 1) - area.x,
        );
        let shifted = self.pointing.shifted;
        let Some(terminal) = self.terminal_mut() else {
            return;
        };
        if terminal.ended().is_none() && terminal.wants_the_pointer() && !shifted {
            let what = match kind {
                Pointer::Pressed => Mouse::Pressed,
                Pointer::Dragged => Mouse::Dragged,
                Pointer::Released => Mouse::Released,
                Pointer::Moved => Mouse::Moved,
            };
            terminal.pointer(what, at);
            return;
        }
        match kind {
            Pointer::Pressed => terminal.hold_from(at),
            Pointer::Dragged => terminal.hold_to(at),
            Pointer::Released | Pointer::Moved => {}
        }
    }

    /// The wheel over the terminal being read: to the program where it
    /// asked for the pointer, and otherwise back up what has gone by.
    pub(super) fn wheel_in_terminal(&mut self, rows: isize) {
        let area = obelus_ui::editor_canvas(self.screen_area);
        let at = self
            .pointing
            .pointer
            .filter(|(x, y)| area.contains(ratatui::layout::Position::new(*x, *y)))
            .map_or((0, 0), |(x, y)| (y - area.y, x - area.x));
        let shifted = self.pointing.shifted;
        let Some(terminal) = self.terminal_mut() else {
            return;
        };
        if terminal.ended().is_some() {
            terminal.scroll_by(-rows);
            return;
        }
        if terminal.wants_the_pointer() && !shifted {
            // A notch, whatever it is worth in rows here: the program
            // decides how far a notch goes.
            let what = match rows < 0 {
                true => Mouse::WheelUp,
                false => Mouse::WheelDown,
            };
            terminal.pointer(what, at);
            return;
        }
        terminal.wheel(rows);
    }

    /// The words held in the terminal being read, taken for a copy -- and
    /// let go of, the way a copy in a terminal always has.
    pub(super) fn take_what_is_held_in_the_terminal(&mut self) -> Option<String> {
        let terminal = self.terminal_mut()?;
        let held = terminal.held_text();
        terminal.let_go();
        held
    }

    /// How many terminals have a program still running, which is what
    /// leaving would stop.
    pub(super) fn terminals_running(&self) -> usize {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::terminal)
            .filter(|terminal| terminal.ended().is_none())
            .count()
    }

    /// Words pasted into the terminal being read, which go to its program.
    ///
    /// Answers whether there was one to take them.
    pub(super) fn paste_into_terminal(&mut self, words: &str) -> bool {
        if !self.typing_to_a_program() {
            return false;
        }
        if let Some(terminal) = self.terminal_mut() {
            terminal.paste(words);
        }
        true
    }

    /// Makes the terminal being read the size it is drawn at.
    ///
    /// Asked once a frame and quiet when nothing moved: the program is told
    /// only when the size it is drawn at is not the size it was.
    pub(super) fn size_the_terminal(&mut self, area: ratatui::layout::Rect) {
        if let Some(terminal) = self.terminal_mut() {
            terminal.resize(area.height, area.width);
        }
    }

    /// What a terminal's program said or did.
    pub(super) fn heard_from_a_terminal(&mut self, heard: Heard) {
        match heard {
            Heard::Wrote { id, bytes } => {
                if let Some(terminal) = self.terminal_numbered(id) {
                    terminal.wrote(&bytes);
                }
            }
            Heard::Ended { id, ended } => {
                tracing::info!(id, ?ended, "a terminal's program ended");
                if let Some(terminal) = self.terminal_numbered(id) {
                    terminal.end(ended.clone());
                }
                if self
                    .terminal
                    .signing_in
                    .as_ref()
                    .is_some_and(|signing| signing.terminal == id)
                {
                    self.sign_in_ended(&ended);
                }
            }
        }
    }

    /// The terminal with this number, wherever it is.
    fn terminal_numbered(&mut self, id: obelus_terminal::Id) -> Option<&mut Terminal> {
        self.documents
            .iter_mut()
            .flatten()
            .filter_map(Document::terminal_mut)
            .find(|terminal| terminal.id() == id)
    }

    /// The slot holding the terminal with this number.
    fn slot_of_terminal(&self, id: obelus_terminal::Id) -> Option<DocumentId> {
        self.documents
            .iter()
            .position(|document| {
                document
                    .as_ref()
                    .and_then(Document::terminal)
                    .is_some_and(|terminal| terminal.id() == id)
            })
            .map(DocumentId::new)
    }

    /// Closes a terminal, asking first where its program is still running.
    ///
    /// Asked for the reason an unsaved file is: what goes with it cannot be
    /// had back -- a shell's history, a build half done.
    pub(super) fn ask_before_stopping(&mut self, id: DocumentId) -> bool {
        let Some(said) = self
            .document(id)
            .and_then(Document::terminal)
            .filter(|terminal| terminal.ended().is_none())
            .map(|terminal| terminal.said().to_string())
        else {
            return false;
        };
        self.stop_to_ask(
            // The words after the colon, because a command line is a name
            // and keeps its own spelling -- and a sentence may not start
            // with one.
            Question::new(format!("Still running: {said}"))
                .way("Stop it and close", Answer::Closing(id, Closing::Discard)),
        );
        true
    }

    /// Asks before leaving, or closing the project, with programs still
    /// running in terminals: `then` is the word for which, and `answer`
    /// what saying yes does.
    ///
    /// Which one, when there is only one -- the words it was started with
    /// are what the reader would recognise -- and a count otherwise.
    pub(super) fn ask_before_stopping_them(&mut self, running: usize, then: &str, answer: Answer) {
        let only = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::terminal)
            .find(|terminal| terminal.ended().is_none())
            .map(|terminal| terminal.said().to_string());
        let (what, way) = match (running, only) {
            (1, Some(said)) => (
                format!("Still running: {said}"),
                format!("Stop it and {then}"),
            ),
            (many, _) => (
                format!("{many} terminals are still running"),
                format!("Stop them and {then}"),
            ),
        };
        self.stop_to_ask(Question::new(what).way(way, answer));
    }

    /// Runs a way of signing in that is a program, for the conversation
    /// that asked.
    pub(super) fn sign_in_by_running(
        &mut self,
        conversation: DocumentId,
        connection: obelus_agent::acp::Connection,
        program: Program,
    ) {
        // A way in that will not even start is a way in that did not work,
        // and the question goes back up for the reader to try another: what
        // it was asking for is still waiting, and nothing else would ever
        // ask it again.
        let id = match self.start_terminal(&program) {
            Ok(id) => id,
            Err(why) => {
                let whose = talking::Whose::One(conversation);
                self.in_talk(whose, |chat| {
                    chat.note(&format!("The sign-in would not start: {why}"));
                });
                self.ask_to_sign_in(whose, None);
                return;
            }
        };
        if let Some(terminal) = self.document(id).and_then(Document::terminal) {
            self.terminal.signing_in = Some(SigningIn {
                terminal: terminal.id(),
                conversation,
                connection,
            });
        }
        let from = self.here();
        self.record(from);
        self.go_to_document(id);
    }

    /// A sign-in's program has ended.
    ///
    /// Ending well is the sign-in: the terminal goes, the reader goes back
    /// to the conversation that was waiting, and the agent is told to go on
    /// with what it was asked. Ending badly leaves the terminal where it is,
    /// with what the program said on it, and the question back up in the
    /// conversation -- the reader is the one who knows whether to try again.
    fn sign_in_ended(&mut self, ended: &Ended) {
        let Some(signing) = self.terminal.signing_in.take() else {
            return;
        };
        let whose = talking::Whose::One(signing.conversation);
        let ours = self
            .agent
            .talker
            .as_ref()
            .is_some_and(|talker| talker.connection() == signing.connection);
        if !ours {
            return;
        }
        if ended.succeeded() {
            if let Some(slot) = self.slot_of_terminal(signing.terminal) {
                self.close(slot);
            }
            self.in_talk(whose, |chat| chat.note("Signed in"));
            self.signed_in_everywhere();
            if let Some(talker) = self.agent.talker.as_ref() {
                talker.signed_in();
            }
            if self.document(signing.conversation).is_some() {
                self.go_to_document(signing.conversation);
            }
            return;
        }
        let how = match &ended.signal {
            Some(signal) => format!("it was stopped by {signal}"),
            None => format!("it exited with {}", ended.code),
        };
        self.in_talk(whose, |chat| {
            chat.note(&format!("Signing in did not finish: {how}"));
        });
        self.ask_to_sign_in(whose, None);
    }

    /// The row in the list of what is open for a terminal.
    ///
    /// Called what the program calls itself where it has said, and by the
    /// words it was started with otherwise; and how it ended where it has,
    /// because a terminal whose program has gone is a page to read rather
    /// than one to type into.
    pub(super) fn terminal_row(index: usize, terminal: &Terminal) -> PickerItem {
        let trailing = terminal.ended().map(obelus_ui::terminal::how_it_ended);
        PickerItem {
            prose: false,
            marker: None,
            icon: obelus_icons::enabled().then_some(obelus_icons::ui::TERMINAL),
            label: terminal.title().unwrap_or(terminal.said()).to_string(),
            version: None,
            detail: None,
            trailing,
            changed: None,
            value: PickerValue::Document(DocumentId::new(index)),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }
}

/// The terminals Obelus has started: the number the next one takes, the
/// reader's own shell, and the sign-in an agent is waiting on.
#[derive(Debug, Default)]
pub(in crate::app) struct Terminals {
    /// The last number handed to a terminal, which is how what its program
    /// writes finds it again.
    pub(in crate::app) terminals: obelus_terminal::Id,
    /// A sign-in running in a terminal of its own, while it runs.
    pub(in crate::app) signing_in: Option<terminals::SigningIn>,
    /// Which shell `open-terminal` starts, where a test has said: the
    /// reader's own is whatever their environment says, and a test about
    /// keys is not a test about their prompt.
    pub(in crate::app) shell: Option<PathBuf>,
}
