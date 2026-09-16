//! Asking what a place is, and showing the answer over it.
//!
//! The question a reader asks most often about somebody else's code, and
//! the one obelus could answer with what it already had: the answer is
//! markdown about one place, and obelus renders markdown and knows where a
//! place is on screen.
//!
//! Two ways in, because they are two different gestures with the same
//! answer: a key, which works wherever a keyboard does, and the pointer
//! resting on a word, which is what everybody's hands already do. What
//! they share is the rule that makes either bearable -- the answer is
//! about a *place*, and an answer that arrives after the reader has left
//! that place is thrown away.

use super::{Resting, *};
use crate::{component::hover::Hover, lsp::hover};

impl App {
    /// How long the pointer has to rest before it is asking a question.
    ///
    /// The reader's, because the answer people want differs by more than
    /// on and off: one who knows the code wants it slow enough never to
    /// appear by accident, and one reading somebody else's wants it as
    /// fast as their hand stops. `None` where they have said zero, which
    /// is the pointer asking nothing at all.
    fn dwell(&self) -> Option<std::time::Duration> {
        let delay = self.config().hover_delay;
        (delay > 0).then(|| std::time::Duration::from_millis(delay as u64))
    }

    /// Asks what the place under the caret is.
    pub fn ask_hover(&mut self) {
        let Some(at) = self.current_buffer().map(|buffer| {
            let cursor = buffer.cursor();
            (cursor.line, cursor.column)
        }) else {
            self.note = Some("no file open".to_string());
            return;
        };
        self.ask_hover_at(at, false);
    }

    /// The characters a hover is about, for the view to mark.
    ///
    /// The same marking a preview uses for the symbol it is about, because
    /// it is the same claim: these are the characters this is about. An
    /// answer about a long line is otherwise an answer about the line.
    #[must_use]
    pub fn hovered_range(&self) -> &[crate::coordinates::Span] {
        self.hover().map_or(&[], Hover::range)
    }

    /// Asks about a place, saying which gesture asked.
    pub(super) fn ask_hover_at(&mut self, at: (LineNumber, CharColumn), pointed: bool) {
        let Some(id) = self.current else { return };
        let Some(buffer) = self.documents.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        if !buffer.content().is_file() || buffer.mode() != crate::buffer::Mode::Edit {
            return;
        }
        let Some(language) = buffer.language() else {
            if !pointed {
                self.note = Some("no language server for this file".to_string());
            }
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let version = buffer.version();
        // Only where a key asked. The pointer asks this of every word it
        // rests on, and a reader moving the mouse across a file with no
        // server would be reading a status row that never stops saying so.
        if let Some(why) = self.why_not_asking(language) {
            if !pointed {
                self.note = Some(why);
            }
            return;
        }
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        if !client.capabilities().is_some_and(hover::supported) {
            if !pointed {
                self.note = Some(format!(
                    "{} does not say what things are",
                    server_named(language)
                ));
            }
            return;
        }
        let position = position::to_lsp(buffer.text(), at.0, at.1, client.encoding());
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position,
        });
        if let Ok(request) = client.request("textDocument/hover", &params) {
            self.remember(
                language,
                request,
                Question {
                    asked: Asked::Hover { at, pointed },
                    buffer: id,
                    version,
                },
            );
        }
    }

    /// Takes an answer, if the reader is still where it is about.
    pub(super) fn on_hover(
        &mut self,
        id: DocumentId,
        at: (LineNumber, CharColumn),
        pointed: bool,
        reply: Reply,
    ) {
        if self.current != Some(id) || !self.still_at(at, pointed) {
            return;
        }
        let Some(buffer) = self.documents.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        let encoding = buffer
            .language()
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let Some(hovered) = hover::in_reply(&reply.result, buffer.text(), &encoding) else {
            return;
        };
        self.hover = Some(Hover::new(hovered, at, pointed));
    }

    /// Whether the pointer is inside the answer's own box.
    ///
    /// Which keeps the answer up. Without this, reaching for it with the
    /// mouse -- to read the rest of it, to scroll it -- would dismiss it
    /// on the way: every cell crossed is a pointer that has left the word.
    pub(super) fn pointer_in_hover(&self) -> bool {
        self.resting
            .is_some_and(|resting| self.inside_hover(resting.x, resting.y))
    }

    /// Whether a cell is inside it.
    fn inside_hover(&self, x: u16, y: u16) -> bool {
        ui::hover::layout(self, self.editor_area)
            .is_some_and(|area| x >= area.x && x < area.right() && y >= area.y && y < area.bottom())
    }

    /// Scrolls the answer by a notch of the wheel.
    ///
    /// The answer rather than the file behind it: while it is up it is
    /// what the reader is looking at, which is the rule the list of
    /// candidates follows too.
    pub(super) fn scroll_hover(&mut self, rows: isize) {
        if let Some(hover) = self.hover.as_mut() {
            hover.scroll(rows);
        }
    }

    /// Whether the reader is still at the place a question was asked
    /// about.
    ///
    /// The caret for a question a key asked, and the pointer for one the
    /// pointer asked: a reader whose hand is on the mouse has not moved
    /// the caret, and one typing has not moved the mouse.
    fn still_at(&self, at: (LineNumber, CharColumn), pointed: bool) -> bool {
        match pointed {
            // Or the pointer is in the answer itself, which is the reader
            // reading it rather than leaving it.
            true => {
                self.pointer_in_hover()
                    || self
                        .resting
                        .and_then(|resting| self.place_under(resting.x, resting.y))
                        .is_some_and(|under| under == at)
            }
            false => self
                .current_buffer()
                .is_some_and(|buffer| (buffer.cursor().line, buffer.cursor().column) == at),
        }
    }

    /// Keeps the panel honest, once a frame, and asks what the pointer has
    /// been resting on.
    pub(super) fn settle_hover(&mut self) {
        // A list or a dialog is what the screen is showing; the other two
        // panels want the same cells and are nearer questions.
        if self.layers().any() {
            self.hover = None;
            self.resting = None;
            return;
        }
        if let Some(hover) = self.hover.as_ref()
            && !self.still_at(hover.at(), hover.pointed())
        {
            self.hover = None;
        }

        // The pointer, having been still for long enough to be asking.
        if let Some(dwell) = self.dwell()
            && let Some(resting) = self.resting
            && !resting.asked
            && resting.since.elapsed() >= dwell
            && self.hover.is_none()
            && let Some(at) = self.place_under(resting.x, resting.y)
        {
            self.resting = Some(Resting {
                asked: true,
                ..resting
            });
            self.ask_hover_at(at, true);
            // And where else that name is, which is the same question
            // asked of the same place: a reader pointing at something is
            // asking about it, and this half of the answer needs no panel.
            if self.a_name_at(at) {
                self.ask_uses_at(at);
            }
        }
    }

    /// Whether anything is waiting on the clock, so the frame asks to be
    /// woken.
    pub(super) fn is_resting(&self) -> bool {
        self.dwell().is_some_and(|dwell| {
            self.resting
                .is_some_and(|resting| !resting.asked && resting.since.elapsed() < dwell)
        })
    }

    /// The place in the document a screen cell is over, if it is over one.
    pub(super) fn place_under(&self, x: u16, y: u16) -> Option<(LineNumber, CharColumn)> {
        let buffer = self.current_buffer()?;
        if buffer.mode() != crate::buffer::Mode::Edit {
            return None;
        }
        let area = self.editor_area;
        if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
            return None;
        }
        let offset = ui::editor::text_offset(
            buffer.text().line_count(),
            self.changes().is_some(),
            !buffer.folds().is_empty(),
        );
        // Left of the text is the gutter, which names a line without
        // pointing at anything in it.
        if x < area.x + offset {
            return None;
        }
        buffer.place_of_cell(y - area.y, x - area.x - offset, self.text_area())
    }

    /// Notes where the pointer is, for the rest that asks a question.
    pub(super) fn pointer_rested(&mut self, x: u16, y: u16) {
        let moved = self
            .resting
            .is_none_or(|resting| (resting.x, resting.y) != (x, y));
        if !moved {
            return;
        }
        // Into the answer's own box is not leaving: a reader reaching for
        // it to read the rest is still reading it. About where the pointer
        // now is, not where it was -- the box it was in a moment ago is
        // the box it may have just left.
        let reading = self.inside_hover(x, y);
        self.resting = Some(Resting {
            x,
            y,
            since: std::time::Instant::now(),
            asked: reading,
        });
        // The answers on screen were about wherever the pointer was.
        if !reading {
            if self.hover.as_ref().is_some_and(Hover::pointed) {
                self.hover = None;
            }
            self.forget_uses();
        }
    }

    /// The panel's three keys.
    ///
    /// Everything else closes it and goes on to do what it was going to
    /// do: every other key moves the reader off the place the answer is
    /// about, and a key that had to be pressed twice -- once to dismiss,
    /// once to act -- would be a key that does nothing.
    pub(super) fn hover_key(&mut self, key: &KeyEvent) -> bool {
        if self.hover().is_none() {
            return false;
        }
        let Some(modifiers) = keymap::modifiers_of(key) else {
            self.hover = None;
            return false;
        };
        let room = ui::hover::layout(self, self.editor_area)
            .map_or(1, |area| area.height.saturating_sub(2));
        match (modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.hover = None;
                true
            }
            (KeyModifiers::NONE, KeyCode::PageDown | KeyCode::PageUp) => {
                let down = key.code == KeyCode::PageDown;
                if let Some(hover) = self.hover.as_mut() {
                    hover.page(down, room);
                }
                true
            }
            _ => {
                self.hover = None;
                false
            }
        }
    }

    /// Whether the rest the pointer is on has asked its question.
    ///
    /// For a test, which cannot see the asking any other way: what a
    /// question produces is a panel, and a panel needs a server to answer.
    #[must_use]
    pub fn rest_has_asked_for_test(&self) -> bool {
        self.resting.is_some_and(|resting| resting.asked)
    }

    /// Hands the panel an answer to a question the pointer asked, about
    /// wherever the pointer is resting.
    ///
    /// The dwell without the waiting: everything else about the path is
    /// the same, and what a test cannot do is stand still for half a
    /// second.
    pub fn hover_pointed_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let Some(at) = self
            .resting
            .and_then(|resting| self.place_under(resting.x, resting.y))
        else {
            return;
        };
        self.on_hover(
            id,
            at,
            true,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands the panel an answer to a question asked about somewhere else
    /// -- which is what a late answer is.
    pub fn hover_late_for_test(&mut self, answer: serde_json::Value, line: usize, column: usize) {
        let Some(id) = self.current else { return };
        self.on_hover(
            id,
            (LineNumber::new(line), CharColumn::new(column)),
            false,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands the panel an answer, as a server would.
    pub fn hover_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let Some(at) = self.current_buffer().map(|buffer| {
            let cursor = buffer.cursor();
            (cursor.line, cursor.column)
        }) else {
            return;
        };
        self.on_hover(
            id,
            at,
            false,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }
}
