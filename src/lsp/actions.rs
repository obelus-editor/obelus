//! What a server offers to do about a place in a file.
//!
//! A quick fix for a diagnostic, an import to add, a match to fill in.
//! The protocol sends two shapes in one list -- a command to run and an
//! action that may carry its own edit -- and the difference matters only
//! when one is chosen: a row is a title either way.

use lsp_types::{CodeAction, CodeActionOrCommand, ServerCapabilities};
use serde_json::Value;

/// One thing a server offers to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// What it is called, which is the row.
    pub title: String,
    /// What sort of thing it is -- `quickfix`, `refactor.extract` --
    /// where the server said.
    ///
    /// Not shown: it is the protocol's filing rather than anything a
    /// reader choosing between two offers needs. Kept because it is also
    /// the one field that tells an action from a bare command, which is
    /// what says a server is answering in the richer shape at all.
    pub kind: Option<String>,
    /// Whether the server marked it as the obvious one.
    pub preferred: bool,
    /// Why it cannot be done here, where the server said it cannot.
    ///
    /// An offer the server means the reader to *see* and not to take: the
    /// action applies to this place, and something about the place stops
    /// it -- a selection that crosses a `?`, a name that is already
    /// taken. Without it a server has two bad choices, leave the offer
    /// out and let nobody learn it exists, or send one that does nothing
    /// when it is chosen.
    pub disabled: Option<String>,
    /// The action as it arrived, to send back to `codeAction/resolve` or
    /// to read an edit out of.
    pub item: Value,
}

impl Action {
    /// The edit it carries, if it carries one.
    ///
    /// An action may arrive without its edit and with `data` instead,
    /// which is the server saying "ask me again if they pick it": working
    /// out every edit for every offer, when a reader picks one of them at
    /// most, is work nobody wanted.
    #[must_use]
    pub fn edit(&self) -> Option<super::edits::Wanted> {
        let action = serde_json::from_value::<CodeAction>(self.item.clone()).ok()?;
        action.edit.map(super::edits::from_edit)
    }

    /// The command it runs, if it runs one.
    #[must_use]
    pub fn command(&self) -> Option<lsp_types::Command> {
        match serde_json::from_value::<CodeActionOrCommand>(self.item.clone()).ok()? {
            CodeActionOrCommand::Command(command) => Some(command),
            CodeActionOrCommand::CodeAction(action) => action.command,
        }
    }

    /// Whether it is an offer to look at rather than to take.
    #[must_use]
    pub fn refused(&self) -> bool {
        self.disabled.is_some()
    }

    /// Whether the server has still to say what this does.
    #[must_use]
    pub fn unresolved(&self) -> bool {
        self.edit().is_none() && self.command().is_none() && self.item.get("data").is_some()
    }
}

/// Whether the server answers `textDocument/codeAction`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.code_action_provider.is_some()
}

/// Whether it fills in an action that arrived without its edit.
#[must_use]
pub fn resolves(capabilities: &ServerCapabilities) -> bool {
    match capabilities.code_action_provider.as_ref() {
        Some(lsp_types::CodeActionProviderCapability::Options(options)) => {
            options.resolve_provider.unwrap_or(false)
        }
        _ => false,
    }
}

/// What a server offered.
///
/// The preferred ones first -- a server that marks one has said which it
/// would pick -- and the rest in the order they arrived, which is the
/// server's own ordering and means something.
#[must_use]
pub fn offered_in(result: &Result<Value, String>) -> Vec<Action> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Ok(offered) = serde_json::from_value::<Option<Vec<CodeActionOrCommand>>>(value.clone())
    else {
        return Vec::new();
    };
    let items = value.as_array().cloned().unwrap_or_default();
    let mut actions: Vec<Action> = offered
        .unwrap_or_default()
        .into_iter()
        .zip(items)
        .map(|(offered, item)| match offered {
            CodeActionOrCommand::Command(command) => Action {
                title: command.title,
                kind: None,
                preferred: false,
                disabled: None,
                item,
            },
            CodeActionOrCommand::CodeAction(action) => Action {
                title: action.title,
                kind: action.kind.map(|kind| kind.as_str().to_string()),
                preferred: action.is_preferred.unwrap_or(false),
                disabled: action.disabled.map(|disabled| disabled.reason),
                item,
            },
        })
        .collect();
    // What can be done, then what cannot. The protocol says to show the
    // second faded where it stands, which is right for a menu that pops
    // up under the cursor; this is a list the reader steps through, and a
    // row they step over belongs after the ones they do not.
    actions.sort_by_key(|action| (action.disabled.is_some(), !action.preferred));
    actions
}
