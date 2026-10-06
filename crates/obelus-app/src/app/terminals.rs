//! A terminal of Obelus's own, as somewhere the reader goes.
//!
//! **What is typed in a terminal is the program's.** Every other document
//! hands the key table what it does not want; a terminal hands it only what
//! `Context::Terminal` binds -- the palette, paste, closing it, and the
//! function keys -- and writes everything else down the pty, escape and
//! `ctrl+c` and `ctrl+w` included, because those are a shell's before they
//! are anybody's. Once the program has ended there is nothing to type to,
//! and the keys go back to Obelus.
//!
//! Two things open one. The reader's own shell, from `open-terminal`, which
//! starts in the project. And an agent's sign-in, where the agent says that
//! signing in is a program to run: the reader answers it there -- which
//! account, a code pasted back -- and its ending well is the sign-in, so
//! the terminal closes and the conversation that was waiting goes on. One
//! that ends badly stays open with what it said on it, because a failed
//! sign-in is something the reader has to read.

use obelus_buffer::question::{Answer, Closing, Question};
use obelus_terminal::{Ended, Heard, Program, Terminal};

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
        let program = match &self.shell {
            Some(shell) => Program::Command {
                program: shell.clone(),
                arguments: Vec::new(),
                env: Vec::new(),
            },
            None => Program::Shell,
        };
        let Some(id) = self.start_terminal(&program) else {
            return;
        };
        self.record(from);
        self.go_to_document(id);
    }

    /// Starts a program in a terminal of its own, and says where it landed.
    ///
    /// In the project, at the size the document region is now -- the next
    /// frame makes it whatever size it is drawn at, and a program told the
    /// wrong size first draws its first screen twice.
    pub(super) fn start_terminal(&mut self, program: &Program) -> Option<DocumentId> {
        let events = self.events.clone()?;
        self.terminals += 1;
        let size = (self.editor_area.height, self.editor_area.width);
        match Terminal::start(
            self.terminals,
            program,
            &self.working_directory,
            size,
            events,
        ) {
            Ok(terminal) => {
                tracing::info!(said = terminal.said(), "a terminal started");
                self.documents.push(Some(Document::from(terminal)));
                Some(DocumentId::new(self.documents.len() - 1))
            }
            Err(why) => {
                tracing::warn!(%why, "a terminal would not start");
                self.wrong(format!("The terminal would not start: {why}"));
                None
            }
        }
    }

    /// Starts this shell for `open-terminal` rather than the reader's own.
    pub fn shell_for_test(&mut self, shell: PathBuf) {
        self.shell = Some(shell);
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
        if !self.typing_to_a_program() {
            return false;
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
            Question::new(format!("{said} is still running"))
                .way("Stop it and close", Answer::Closing(id, Closing::Discard)),
        );
        true
    }

    /// Runs a way of signing in that is a program, for the conversation
    /// that asked.
    pub(super) fn sign_in_by_running(
        &mut self,
        conversation: DocumentId,
        connection: obelus_agent::acp::Connection,
        program: Program,
    ) {
        let Some(id) = self.start_terminal(&program) else {
            return;
        };
        if let Some(terminal) = self.document(id).and_then(Document::terminal) {
            self.signing_in = Some(SigningIn {
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
        let Some(signing) = self.signing_in.take() else {
            return;
        };
        let whose = talking::Whose::One(signing.conversation);
        let ours = self
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
            if let Some(talker) = self.talker.as_ref() {
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
        let trailing = terminal.ended().map(|ended| match ended.succeeded() {
            true => "Ended".to_string(),
            false => match &ended.signal {
                Some(signal) => signal.clone(),
                None => format!("Exited {}", ended.code),
            },
        });
        PickerItem {
            prose: false,
            marker: None,
            icon: obelus_icons::enabled().then_some(obelus_icons::ui::TERMINAL),
            label: terminal.title().unwrap_or(terminal.said()).to_string(),
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
