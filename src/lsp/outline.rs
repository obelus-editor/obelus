//! What a language server says a file defines.
//!
//! Pure: a JSON reply in, a list of symbols out. The interesting rules are
//! all in here -- two shapes of answer, a kind that arrives as a number,
//! and nesting that only one of the two shapes has -- and none of them need
//! a server to test.

use lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolInformation, SymbolKind};
use serde_json::Value;

use crate::theme::SyntaxKind;

/// One symbol, ready to become a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outlined {
    /// What it is called.
    pub name: String,
    /// What sort of thing it is, as the colour it will be drawn in.
    pub kind: SyntaxKind,
    /// Which line, counted from zero, in the server's own units.
    pub line: u32,
    /// Where the name starts, in the server's own units.
    pub character: u32,
    /// Where it ends.
    pub end_character: u32,
    /// How many symbols enclose it.
    pub depth: usize,
}

/// The symbols in a `textDocument/documentSymbol` reply.
///
/// Empty for an error, for a null answer, and for a server that has nothing
/// to say yet -- all of which mean the same thing to the caller, which is
/// "ask the syntax tree instead".
#[must_use]
pub fn symbols_in(result: Result<Value, String>) -> Vec<Outlined> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Ok(response) = serde_json::from_value::<DocumentSymbolResponse>(value) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    match response {
        // The shape with nesting, which is the one worth having: a method
        // inside a class arrives inside it.
        DocumentSymbolResponse::Nested(symbols) => nest(&symbols, 0, &mut found),
        // The flat shape, from a server that has not implemented the other.
        // No nesting is available -- `container_name` is a string, not a
        // reference -- so every row sits at the top.
        DocumentSymbolResponse::Flat(symbols) => {
            found.extend(symbols.iter().map(flat));
        }
    }
    found
}

/// Walks the nested shape, remembering how deep it is.
fn nest(symbols: &[DocumentSymbol], depth: usize, found: &mut Vec<Outlined>) {
    for symbol in symbols {
        // `selection_range` is the name; `range` is the whole definition,
        // body and all. The name is what a row is about and what a preview
        // should mark.
        let at = symbol.selection_range.start;
        let end = symbol.selection_range.end;
        found.push(Outlined {
            name: symbol.name.clone(),
            kind: kind_of(symbol.kind),
            line: at.line,
            character: at.character,
            end_character: if end.line == at.line {
                end.character
            } else {
                at.character
            },
            depth,
        });
        if let Some(children) = &symbol.children {
            nest(children, depth + 1, found);
        }
    }
}

/// One symbol of the flat shape.
fn flat(symbol: &SymbolInformation) -> Outlined {
    let at = symbol.location.range.start;
    Outlined {
        name: symbol.name.clone(),
        kind: kind_of(symbol.kind),
        line: at.line,
        character: at.character,
        // Nothing here says where the *name* is: the range is the whole
        // definition, which starts at an attribute or a doc comment as often
        // as at the name. So the row marks nothing rather than marking the
        // wrong thing -- it still goes to the right place, and a highlight
        // over the wrong words is worse than none.
        end_character: at.character,
        depth: 0,
    }
}

/// What a kind means, as a colour.
///
/// The kinds arrive as numbers from a fixed list in the protocol. Grouped
/// the way the theme groups things rather than kept apart: a list of names
/// wants to distinguish what is callable from what is a type, and does not
/// want twenty-six colours.
fn kind_of(kind: SymbolKind) -> SyntaxKind {
    match kind {
        SymbolKind::FUNCTION | SymbolKind::METHOD | SymbolKind::CONSTRUCTOR => SyntaxKind::Function,
        SymbolKind::CLASS
        | SymbolKind::STRUCT
        | SymbolKind::INTERFACE
        | SymbolKind::ENUM
        | SymbolKind::TYPE_PARAMETER
        | SymbolKind::EVENT
        // What rust-analyzer calls an `impl` block, which is a type's own
        // section of the file and reads best as one.
        | SymbolKind::OBJECT => SyntaxKind::Type,
        SymbolKind::CONSTANT | SymbolKind::ENUM_MEMBER => SyntaxKind::Constant,
        SymbolKind::FIELD | SymbolKind::PROPERTY => SyntaxKind::Property,
        SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => SyntaxKind::Keyword,
        SymbolKind::STRING => SyntaxKind::String,
        SymbolKind::NUMBER => SyntaxKind::Number,
        SymbolKind::BOOLEAN => SyntaxKind::Boolean,
        // Variables, files, keys, operators and the rest. Ordinary
        // foreground reads; a colour invented for each would not.
        _ => SyntaxKind::Variable,
    }
}

/// One name the server knows, somewhere in the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// What it is called.
    pub name: String,
    /// What sort of thing it is, as the colour it will be drawn in.
    pub kind: SyntaxKind,
    /// Which file it is in.
    pub path: std::path::PathBuf,
    /// Which line of it, in the server's own units.
    pub line: u32,
    /// Where the name starts, in the server's own units.
    pub character: u32,
    /// Where it ends.
    pub end_character: u32,
}

/// The names in a `workspace/symbol` reply.
///
/// Two shapes again, and this time the difference is where the position is:
/// the old one carries a whole range, the newer one is allowed to carry only
/// the file and fill the range in later on request. A row needs a line to
/// jump to, so a symbol with no range is dropped -- a row that cannot answer
/// the one thing it is for is worse than a row that is not there.
///
/// Empty for an error and for a null answer, which is what a server that
/// has not finished indexing says.
#[must_use]
pub fn found_in(result: Result<Value, String>) -> Vec<Found> {
    let Ok(value) = result else {
        return Vec::new();
    };
    // The flat shape first: every server that answers this question at all
    // answers with it, and the newer shape deserializes from the same JSON
    // with its range thrown away.
    if let Ok(symbols) = serde_json::from_value::<Vec<SymbolInformation>>(value.clone()) {
        return symbols.iter().filter_map(found).collect();
    }
    match serde_json::from_value::<Vec<lsp_types::WorkspaceSymbol>>(value) {
        Ok(symbols) => symbols.iter().filter_map(newer).collect(),
        Err(error) => {
            tracing::debug!(%error, "a workspace/symbol answer in no shape obelus knows");
            Vec::new()
        }
    }
}

/// The old shape, whose location is always a range in a file.
fn found(symbol: &SymbolInformation) -> Option<Found> {
    let path = crate::lsp::client::path_of(symbol.location.uri.as_str())?;
    let start = symbol.location.range.start;
    Some(Found {
        name: symbol.name.clone(),
        kind: kind_of(symbol.kind),
        path,
        line: start.line,
        character: start.character,
        end_character: symbol.location.range.end.character,
    })
}

/// The newer shape, which may carry a file and no range at all.
fn newer(symbol: &lsp_types::WorkspaceSymbol) -> Option<Found> {
    let lsp_types::OneOf::Left(location) = &symbol.location else {
        return None;
    };
    let path = crate::lsp::client::path_of(location.uri.as_str())?;
    Some(Found {
        name: symbol.name.clone(),
        kind: kind_of(symbol.kind),
        path,
        line: location.range.start.line,
        character: location.range.start.character,
        end_character: location.range.end.character,
    })
}

#[cfg(test)]
mod workspace_tests {
    use serde_json::json;

    use super::found_in;
    use crate::theme::SyntaxKind;

    /// The shape every server that answers this question at all uses: a
    /// name, a kind as a number, and a location with a range in it.
    #[test]
    fn the_flat_shape_becomes_rows() {
        let reply = json!([
            {
                "name": "Picker",
                "kind": 23,
                "location": {
                    "uri": "file:///p/src/component/picker/mod.rs",
                    "range": {
                        "start": { "line": 41, "character": 11 },
                        "end": { "line": 41, "character": 17 }
                    }
                }
            }
        ]);
        let found = found_in(Ok(reply));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Picker");
        assert_eq!(found[0].kind, SyntaxKind::Type);
        assert_eq!(
            found[0].path,
            std::path::Path::new("/p/src/component/picker/mod.rs")
        );
        assert_eq!((found[0].line, found[0].character), (41, 11));
        assert_eq!(found[0].end_character, 17);
    }

    /// The newer shape is allowed to carry only the file and fill the range
    /// in later, on request. A row needs a line to jump to, so one with no
    /// range is dropped: a row that cannot answer the one thing it is for is
    /// worse than a row that is not there.
    #[test]
    fn a_symbol_with_no_range_is_not_a_row() {
        let reply = json!([
            { "name": "far_away", "kind": 12, "location": { "uri": "file:///p/a.rs" } }
        ]);
        assert!(found_in(Ok(reply)).is_empty());
    }

    /// An error, a null and a shape obelus does not know all mean the same
    /// thing to the caller: no rows. A server that has not finished indexing
    /// answers null.
    #[test]
    fn nothing_usable_means_no_rows() {
        assert!(found_in(Err("no".to_string())).is_empty());
        assert!(found_in(Ok(json!(null))).is_empty());
        assert!(found_in(Ok(json!({ "unexpected": true }))).is_empty());
    }
}
