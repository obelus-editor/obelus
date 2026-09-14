//! What can be asked about the symbol under the cursor.

use lsp_types::{OneOf, ServerCapabilities};

use crate::command::Command;

/// A question about a symbol whose answer is a set of places.
///
/// All four share one path — ask, get locations, jump to one — which is why
/// they arrived together rather than one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolAction {
    /// Where it is defined.
    Definition,
    /// Where its type is defined.
    TypeDefinition,
    /// What implements it.
    Implementation,
    /// Everywhere it is used.
    References,
}

/// Every action, in the order the menu lists them.
pub const ALL: &[SymbolAction] = &[
    SymbolAction::Definition,
    SymbolAction::TypeDefinition,
    SymbolAction::Implementation,
    SymbolAction::References,
];

impl SymbolAction {
    /// The request that asks it.
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Definition => "textDocument/definition",
            Self::TypeDefinition => "textDocument/typeDefinition",
            Self::Implementation => "textDocument/implementation",
            Self::References => "textDocument/references",
        }
    }

    /// What to call it, which is what its command is called.
    ///
    /// One name, in the command table, so the menu and the palette cannot
    /// disagree about what a question is called.
    #[must_use]
    pub fn title(self) -> &'static str {
        self.command().spec().title
    }

    /// The command that asks it.
    ///
    /// Each question is a named action, so each is a command: the menu and the
    /// palette then offer the same things, and whatever shows a key reads the
    /// same table.
    #[must_use]
    pub const fn command(self) -> Command {
        match self {
            Self::Definition => Command::SymbolDefinition,
            Self::TypeDefinition => Command::SymbolTypeDefinition,
            Self::Implementation => Command::SymbolImplementation,
            Self::References => Command::SymbolReferences,
        }
    }

    /// The question a command asks, if it asks one.
    #[must_use]
    pub fn for_command(command: Command) -> Option<Self> {
        ALL.iter()
            .copied()
            .find(|action| action.command() == command)
    }

    /// Whether the server said it can answer.
    ///
    /// A server declares each of these separately, and one that cannot answer
    /// should not be offered: a menu entry that always comes back empty is
    /// worse than a menu that is one line shorter.
    #[must_use]
    pub fn supported(self, capabilities: &ServerCapabilities) -> bool {
        match self {
            Self::Definition => declared(capabilities.definition_provider.as_ref()),
            Self::TypeDefinition => capabilities.type_definition_provider.is_some(),
            Self::Implementation => capabilities.implementation_provider.is_some(),
            Self::References => declared(capabilities.references_provider.as_ref()),
        }
    }
}

/// Whether a provider field says yes.
///
/// `Some(Left(false))` is a server saying it cannot, which is not the same as
/// the field being absent and reads the same if only the `Option` is checked.
fn declared<T>(provider: Option<&OneOf<bool, T>>) -> bool {
    match provider {
        Some(OneOf::Left(yes)) => *yes,
        Some(OneOf::Right(_)) => true,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server saying `false` is saying no. Checking only that the field is
    /// present offers an action that will always come back empty.
    #[test]
    fn a_provider_of_false_is_not_support() {
        let mut capabilities = ServerCapabilities::default();
        assert!(!SymbolAction::Definition.supported(&capabilities));

        capabilities.definition_provider = Some(OneOf::Left(false));
        assert!(!SymbolAction::Definition.supported(&capabilities));

        capabilities.definition_provider = Some(OneOf::Left(true));
        assert!(SymbolAction::Definition.supported(&capabilities));
    }

    /// The two directions have to agree, or a menu row runs a different
    /// question from the one it names.
    #[test]
    fn a_command_and_its_question_agree() {
        for action in ALL {
            assert_eq!(SymbolAction::for_command(action.command()), Some(*action));
        }
    }

    #[test]
    fn every_action_has_its_own_method() {
        let mut methods: Vec<&str> = ALL.iter().map(|action| action.method()).collect();
        methods.sort_unstable();
        methods.dedup();
        assert_eq!(methods.len(), ALL.len());
    }
}

/// What an answer turns out to mean.
///
/// A separate step from acting on it, because the rules are the subtle part
/// and every one of them is a claim worth being able to check: that an answer
/// about replaced text is dropped, and that an empty answer means two
/// different things the protocol does not distinguish.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The document changed after the question was asked, so the answer is
    /// about text that is no longer there.
    Stale,
    /// The server said no, with a reason.
    Failed(String),
    /// Nothing, and the server has finished indexing, so nothing is the
    /// answer.
    Nothing,
    /// Nothing, and the server is still indexing, so nothing is not an answer
    /// yet. The protocol sends the same reply for both.
    NotYet,
    /// The places it named.
    Places(Vec<Place>),
}

/// A place an answer named, in the protocol's own units.
///
/// Converted when the file is opened rather than now, because converting it
/// needs that file's text and the file may never be visited.
///
/// The whole range, not only where it starts: a list of references is read by
/// looking at the symbol in each one, and a preview that says only which line
/// leaves the reader finding it again on every row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Which file.
    pub path: std::path::PathBuf,
    /// Its line, counted from zero.
    pub line: u32,
    /// And how far along, in whichever units the server agreed to.
    pub character: u32,
    /// The line it ends on, which is usually the one it starts on.
    pub end_line: u32,
    /// And how far along that one.
    pub end_character: u32,
}

/// The text edits in a formatting answer, in the order the server gave
/// them.
///
/// `null` is a server saying it has nothing to change, and is turned away
/// before the parse so that it is not logged as a layout obelus could not
/// read -- the answer is the same either way, and the line in the log is
/// not. Anything else that will not parse is a server obelus cannot follow,
/// and following half of a layout is worse than following none.
#[must_use]
pub fn edits_in(result: Option<serde_json::Value>) -> Option<Vec<lsp_types::TextEdit>> {
    let result = result?;
    if result.is_null() {
        return None;
    }
    match serde_json::from_value::<Vec<lsp_types::TextEdit>>(result) {
        Ok(edits) if !edits.is_empty() => Some(edits),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%error, "a layout obelus cannot read");
            None
        }
    }
}

/// What to make of a reply.
///
/// `asked_against` and `now` are the document version the question was asked
/// about and the version there is now. `indexing` is whether the server has
/// said it is busy.
#[must_use]
pub fn outcome_of(
    reply: Result<serde_json::Value, String>,
    asked_against: i32,
    now: Option<i32>,
    indexing: bool,
) -> Outcome {
    if now != Some(asked_against) {
        return Outcome::Stale;
    }
    let result = match reply {
        Ok(result) => result,
        Err(message) => return Outcome::Failed(message),
    };
    let places = places_in(&result);
    if places.is_empty() {
        return if indexing {
            Outcome::NotYet
        } else {
            Outcome::Nothing
        };
    }
    Outcome::Places(places)
}

/// The places a result names.
///
/// One reply shape per question would be three shapes: a single location, a
/// list of them, and a list of links. The protocol's own type covers all
/// three, and that union is why a hand-rolled parser gets one of them wrong.
fn places_in(result: &serde_json::Value) -> Vec<Place> {
    use lsp_types::GotoDefinitionResponse;

    let Ok(response) = serde_json::from_value::<GotoDefinitionResponse>(result.clone()) else {
        return Vec::new();
    };
    let named: Vec<(String, lsp_types::Range)> = match response {
        GotoDefinitionResponse::Scalar(location) => {
            vec![(location.uri.as_str().to_string(), location.range)]
        }
        GotoDefinitionResponse::Array(locations) => locations
            .into_iter()
            .map(|location| (location.uri.as_str().to_string(), location.range))
            .collect(),
        // The *selection* range, which is the symbol, rather than the target
        // range, which is often the whole item it belongs to.
        GotoDefinitionResponse::Link(links) => links
            .into_iter()
            .map(|link| {
                (
                    link.target_uri.as_str().to_string(),
                    link.target_selection_range,
                )
            })
            .collect(),
    };

    named
        .into_iter()
        .filter_map(|(uri, range)| {
            crate::lsp::client::path_of(&uri).map(|path| Place {
                path,
                line: range.start.line,
                character: range.start.character,
                end_line: range.end.line,
                end_character: range.end.character,
            })
        })
        .collect()
}

#[cfg(test)]
mod outcomes {
    use serde_json::json;

    use super::*;

    fn location(path: &str, line: u32) -> serde_json::Value {
        json!({
            "uri": format!("file://{path}"),
            "range": {
                "start": { "line": line, "character": 4 },
                "end": { "line": line, "character": 9 },
            }
        })
    }

    /// The claim the whole no-runtime design rests on: an answer arrives after
    /// the world has moved on, and acting on it would jump to somewhere that
    /// was true a moment ago.
    #[test]
    fn an_answer_about_replaced_text_is_stale() {
        let reply = Ok(location("/a/b.rs", 10));
        assert_eq!(outcome_of(reply.clone(), 3, Some(4), false), Outcome::Stale);
        assert_eq!(outcome_of(reply.clone(), 3, None, false), Outcome::Stale);
        assert!(matches!(
            outcome_of(reply, 3, Some(3), false),
            Outcome::Places(_)
        ));
    }

    /// The two meanings of an empty answer. On the wire they are the same
    /// message, and telling a reader "no definition" while the server is
    /// still reading the project is the difference between a tool that looks
    /// broken and one that looks busy.
    #[test]
    fn nothing_means_two_different_things() {
        assert_eq!(
            outcome_of(Ok(json!(null)), 1, Some(1), false),
            Outcome::Nothing
        );
        assert_eq!(
            outcome_of(Ok(json!(null)), 1, Some(1), true),
            Outcome::NotYet
        );
        assert_eq!(outcome_of(Ok(json!([])), 1, Some(1), true), Outcome::NotYet);
    }

    /// Staleness is decided before the reply is even looked at: an error about
    /// text that is gone is not worth reporting either.
    #[test]
    fn staleness_comes_before_everything() {
        let failed = Err("no such thing".to_string());
        assert_eq!(
            outcome_of(failed.clone(), 1, Some(2), false),
            Outcome::Stale
        );
        assert_eq!(
            outcome_of(failed, 1, Some(1), false),
            Outcome::Failed("no such thing".to_string())
        );
    }

    /// All three shapes a location answer can arrive in. A parser that
    /// handles one and falls through on the others reports "nothing found"
    /// for a server that answered.
    #[test]
    fn every_shape_of_answer_is_understood() {
        let single = outcome_of(Ok(location("/a/b.rs", 7)), 1, Some(1), false);
        assert_eq!(
            single,
            Outcome::Places(vec![Place {
                path: "/a/b.rs".into(),
                line: 7,
                character: 4,
                end_line: 7,
                end_character: 9
            }])
        );

        let array = outcome_of(
            Ok(json!([location("/a/b.rs", 1), location("/c/d.rs", 2)])),
            1,
            Some(1),
            false,
        );
        assert!(matches!(array, Outcome::Places(places) if places.len() == 2));

        let links = outcome_of(
            Ok(json!([{
                "targetUri": "file:///a/b.rs",
                "targetRange": {
                    "start": {"line": 5, "character": 0},
                    "end": {"line": 5, "character": 3},
                },
                "targetSelectionRange": {
                    "start": {"line": 5, "character": 0},
                    "end": {"line": 5, "character": 3},
                },
            }])),
            1,
            Some(1),
            false,
        );
        assert_eq!(
            links,
            Outcome::Places(vec![Place {
                path: "/a/b.rs".into(),
                line: 5,
                character: 0,
                end_line: 5,
                end_character: 3
            }])
        );
    }

    /// A path with anything a URI reserves in it has to survive the round
    /// trip, or the answer names a different file — or no file at all.
    #[test]
    fn a_path_with_reserved_characters_survives() {
        let awkward = "/tmp/a dir/\u{4f60}\u{597d}#1.rs";
        let uri = crate::lsp::client::uri_for(std::path::Path::new(awkward)).expect("a uri");
        let outcome = outcome_of(
            Ok(json!({
                "uri": uri.as_str(),
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 1},
                }
            })),
            1,
            Some(1),
            false,
        );
        assert_eq!(
            outcome,
            Outcome::Places(vec![Place {
                path: awkward.into(),
                line: 0,
                character: 0,
                end_line: 0,
                end_character: 1
            }])
        );
    }
}
