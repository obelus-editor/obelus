//! What a server wants changed, across the project.
//!
//! One shape for the two questions that produce it -- a rename and a code
//! action -- because the answer is the same: a list of places in files and
//! what should be there instead.
//!
//! The protocol has two spellings of it, `changes` and `documentChanges`,
//! and the second can also ask for files to be created, renamed or
//! deleted. obelus reads the edits out of either and refuses the rest: a
//! program that quietly deleted a file because a server suggested it would
//! be a program nobody should run, and a refusal a reader can see beats a
//! surprise they cannot undo.

use std::path::PathBuf;

use lsp_types::WorkspaceEdit;
use serde_json::Value;

/// One change to one file, in the protocol's own coordinates.
///
/// The range stays in the server's units until the file it is about has
/// been read: converting it needs that text, and a file that is not open
/// has not been read yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Which file.
    pub path: PathBuf,
    /// What to replace.
    pub range: lsp_types::Range,
    /// What to put there.
    pub text: String,
}

/// What a server asked for, and what obelus will not do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The edits, in the order the server gave them.
    pub changes: Vec<Change>,
    /// The operations obelus refuses, named so a reader can be told.
    ///
    /// Creating, renaming and deleting files. A rename that moves a module
    /// to a new path is a real thing servers ask for and a thing obelus
    /// has no way to undo, so the edits go in and this is reported.
    pub refused: Vec<String>,
}

impl Wanted {
    /// Whether there is nothing to do.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// What a `WorkspaceEdit` asks for.
#[must_use]
pub fn wanted_in(value: &Value) -> Wanted {
    let Ok(edit) = serde_json::from_value::<WorkspaceEdit>(value.clone()) else {
        return Wanted::default();
    };
    from_edit(edit)
}

/// The same, from the type itself: a code action carries one inline.
#[must_use]
pub fn from_edit(edit: WorkspaceEdit) -> Wanted {
    use lsp_types::{DocumentChangeOperation, DocumentChanges, ResourceOp};

    let mut wanted = Wanted::default();
    let mut take = |uri: &lsp_types::Uri, edits: Vec<lsp_types::TextEdit>| {
        let Some(path) = super::path_of_uri(uri.as_str()) else {
            return;
        };
        for edit in edits {
            wanted.changes.push(Change {
                path: path.clone(),
                range: edit.range,
                text: edit.new_text,
            });
        }
    };

    if let Some(changes) = edit.changes {
        for (uri, edits) in changes {
            take(&uri, edits);
        }
    }
    match edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => {
            for edit in edits {
                let edits = edit
                    .edits
                    .into_iter()
                    .map(|one| match one {
                        lsp_types::OneOf::Left(edit) => edit,
                        // An annotated edit is an edit with a note on it
                        // saying what it is for. The note is for a dialog
                        // obelus does not have.
                        lsp_types::OneOf::Right(annotated) => annotated.text_edit,
                    })
                    .collect();
                take(&edit.text_document.uri, edits);
            }
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                match operation {
                    DocumentChangeOperation::Edit(edit) => {
                        let edits = edit
                            .edits
                            .into_iter()
                            .map(|one| match one {
                                lsp_types::OneOf::Left(edit) => edit,
                                lsp_types::OneOf::Right(annotated) => annotated.text_edit,
                            })
                            .collect();
                        take(&edit.text_document.uri, edits);
                    }
                    DocumentChangeOperation::Op(op) => wanted.refused.push(
                        match op {
                            ResourceOp::Create(_) => "creating a file",
                            ResourceOp::Rename(_) => "moving a file",
                            ResourceOp::Delete(_) => "deleting a file",
                        }
                        .to_string(),
                    ),
                }
            }
        }
        None => {}
    }
    wanted
}
