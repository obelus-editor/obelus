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
