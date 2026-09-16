//! Making the changes a server asked for.
//!
//! A rename and a code action both come back as a list of places in files
//! and what should be there instead, and the difficult part is not the
//! list: it is that some of those files are open and some are not.
//!
//! Every one of them is *opened* and edited, which makes a rename the
//! same kind of thing as typing: it lands in the reader's undo, it shows
//! on screen, and nothing reaches the disk until they save. Writing the
//! closed ones directly would be quicker and would be a change with no
//! undo in it, made to files the reader never saw -- which is the one
//! thing a rename must not be, because a rename they regret is exactly the
//! change they will want back.
//!
//! The edits for one file are made from the end backwards, so that each
//! one lands in the coordinates the server wrote it in.

use std::{collections::BTreeMap, path::PathBuf};

use super::*;
use crate::lsp::edits::{Change, Wanted};

impl App {
    /// Makes what a server asked for, and says what happened.
    ///
    /// The sentence it returns is what goes on the status row: how much
    /// changed, and anything obelus refused to do.
    pub(super) fn apply_wanted(&mut self, wanted: &Wanted) -> String {
        if wanted.is_empty() {
            return match wanted.refused.is_empty() {
                true => "nothing to change".to_string(),
                false => format!("obelus will not do that: {}", wanted.refused.join(", ")),
            };
        }

        // Per file, because the order edits are made in matters within a
        // file and not between them.
        let mut per_file: BTreeMap<PathBuf, Vec<Change>> = BTreeMap::new();
        for change in &wanted.changes {
            per_file
                .entry(change.path.clone())
                .or_default()
                .push(change.clone());
        }

        let encoding = self
            .current_buffer()
            .and_then(Buffer::language)
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });

        let (mut changed, mut opened, mut failed) = (0usize, 0usize, 0usize);
        for (path, changes) in per_file {
            // Every file this touches becomes a document. A file written
            // on disk instead would be a change with no undo in it and no
            // way to look at before it happens -- and a rename the reader
            // regrets is exactly the change they will want to take back.
            // The cost is a buffer list with the files a rename touched in
            // it, which is the honest shape of what just happened.
            let was_open = self
                .documents
                .iter()
                .flatten()
                .any(|buffer| buffer.path() == path && buffer.content().is_file());
            let Some(index) = self.open_quietly(&path) else {
                failed += 1;
                continue;
            };
            if !was_open {
                opened += 1;
            }
            match self.change_open(index, &changes, &encoding) {
                true => changed += 1,
                false => failed += 1,
            }
        }

        let mut said = format!("changed {changed} {}", files(changed));
        if opened > 0 {
            said.push_str(&format!(", {opened} of them opened"));
        }
        said.push_str(" -- nothing written yet");
        if failed > 0 {
            said.push_str(&format!(", and {failed} could not be changed"));
        }
        if !wanted.refused.is_empty() {
            said.push_str(&format!(" -- not {}", wanted.refused.join(", ")));
        }
        said
    }

    /// Makes an edit a server asked for, and tells it what happened.
    ///
    /// The one request from a server that obelus answers by doing
    /// something. It is how a refactoring that the server works out for
    /// itself arrives: the action obelus chose carried a command rather
    /// than an edit, the server ran it, and this is the result coming
    /// back the other way.
    pub(super) fn on_asked_edit(&mut self, language: LanguageId, asked: &lsp::client::AskedEdit) {
        let answer = self.make_asked_edit(asked);
        if let Some(client) = self.servers.get_mut(&language) {
            client.answer_request(&answer);
        }
    }

    /// The same, as far as the answer: what obelus did and what it will
    /// say it did.
    fn make_asked_edit(&mut self, asked: &lsp::client::AskedEdit) -> serde_json::Value {
        let wanted = crate::lsp::edits::wanted_in(&asked.edit);
        // Whether anything landed, which is what the server is asking.
        // An edit with nothing obelus will do in it is a no, and the
        // sentence that says why is the reason the protocol asks for.
        let applied = !wanted.is_empty();
        let said = self.apply_wanted(&wanted);
        self.note = Some(match asked.label.as_deref() {
            Some(label) => format!("{label}: {said}"),
            None => said.clone(),
        });
        lsp::client::edit_answer(&asked.id, applied, &said)
    }

    /// Hands obelus an edit, as a server would ask for one, and gives
    /// back the answer it would send.
    pub fn asked_edit_for_test(&mut self, edit: serde_json::Value) -> serde_json::Value {
        self.make_asked_edit(&lsp::client::AskedEdit {
            id: serde_json::json!(1),
            label: None,
            edit,
        })
    }

    /// Edits a document that is open, in one act of undo.
    fn change_open(
        &mut self,
        index: usize,
        changes: &[Change],
        encoding: &lsp_types::PositionEncodingKind,
    ) -> bool {
        let Some(buffer) = self.documents.get(index).and_then(Option::as_ref) else {
            return false;
        };
        // Into this document's own coordinates, all of them, before any of
        // them moves the text they are measured against.
        let mut spans: Vec<(crate::coordinates::Span, String)> = changes
            .iter()
            .map(|change| {
                let (line, column) =
                    position::from_lsp(buffer.text(), change.range.start, encoding);
                let (end_line, end_column) =
                    position::from_lsp(buffer.text(), change.range.end, encoding);
                (
                    crate::coordinates::Span {
                        line,
                        column,
                        end_line,
                        end_column,
                    },
                    change.text.clone(),
                )
            })
            .collect();
        // From the end backwards.
        spans.sort_by(|left, right| {
            (right.0.line, right.0.column).cmp(&(left.0.line, left.0.column))
        });
        for (at, (span, with)) in spans.iter().enumerate() {
            // The first of them starts a group and the rest join it: one
            // key was pressed, so one press of undo takes it back.
            let doing = match at {
                0 => crate::buffer::undo::Doing::Whole,
                _ => crate::buffer::undo::Doing::Joined,
            };
            self.change_in(index, *span, with, doing);
        }
        if let Some(buffer) = self.documents.get_mut(index).and_then(Option::as_mut) {
            buffer.settle_undo();
        }
        true
    }
}

/// "file" or "files", for a sentence that counts them.
fn files(many: usize) -> &'static str {
    match many {
        1 => "file",
        _ => "files",
    }
}
