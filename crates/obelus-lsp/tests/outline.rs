//! What a server's answer becomes, and what happens when it has none.
//!
//! Against values rather than against a server: the interesting rules are
//! the two shapes of reply, the nesting only one of them has, and the
//! fallback when the answer is empty. A server cannot be made to produce
//! any of those on demand.

use obelus_lsp::outline;
use obelus_text::kind::SyntaxKind;
use serde_json::json;

#[test]
fn the_nested_shape_keeps_its_nesting() {
    let reply = json!([
        {
            "name": "Thing",
            "kind": 23,
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 9, "character": 1 } },
            "selectionRange": { "start": { "line": 0, "character": 7 },
                                "end": { "line": 0, "character": 12 } },
            "children": [
                {
                    "name": "method",
                    "kind": 6,
                    "range": { "start": { "line": 2, "character": 4 },
                               "end": { "line": 4, "character": 5 } },
                    "selectionRange": { "start": { "line": 2, "character": 7 },
                                        "end": { "line": 2, "character": 13 } },
                    "children": []
                }
            ]
        }
    ]);

    let found = outline::symbols_in(Ok(reply));
    assert_eq!(found.len(), 2);

    // The parent first, then what is inside it, which is the order it reads
    // in and the order the rows are drawn in.
    assert_eq!(found[0].name, "Thing");
    assert_eq!(found[0].depth, 0);
    assert_eq!(found[0].kind, SyntaxKind::Type);
    assert_eq!(found[1].name, "method");
    assert_eq!(found[1].depth, 1, "the nesting was flattened");
    assert_eq!(found[1].kind, SyntaxKind::Function);

    // The *selection* range, not the range: one is the name and the other is
    // the whole definition, and a row is about the name.
    assert_eq!((found[0].line, found[0].character), (0, 7));
    assert_eq!(found[0].end_character, 12);
}

#[test]
fn the_flat_shape_is_read_too() {
    let reply = json!([
        {
            "name": "free",
            "kind": 12,
            "location": {
                "uri": obelus_lsp::client::uri_for(
                    &std::env::temp_dir().join("nowhere.rs")
                )
                .expect("a uri")
                .as_str(),
                "range": { "start": { "line": 4, "character": 3 },
                           "end": { "line": 6, "character": 1 } }
            }
        }
    ]);

    let found = outline::symbols_in(Ok(reply));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "free");
    assert_eq!(found[0].kind, SyntaxKind::Function);
    assert_eq!(found[0].depth, 0, "the flat shape has no nesting to keep");
    // And nothing is marked: this shape says where the *definition* starts,
    // which is an attribute or a doc comment as often as the name, so there
    // is no honest span to highlight.
    assert_eq!(found[0].character, 3);
    assert_eq!(
        found[0].end_character, 3,
        "a span was invented for a shape that does not carry one"
    );
}

/// Every way of saying nothing, all of which mean "ask the tree instead".
#[test]
fn nothing_is_nothing_however_it_is_said() {
    assert!(outline::symbols_in(Err("no".to_string())).is_empty());
    assert!(outline::symbols_in(Ok(json!(null))).is_empty());
    assert!(outline::symbols_in(Ok(json!([]))).is_empty());
    // A reply that is not either shape. A server Obelus cannot read is a
    // server it has no answer from.
    assert!(outline::symbols_in(Ok(json!({ "unexpected": true }))).is_empty());
}

/// The kinds a reader distinguishes. Twenty-six protocol kinds map onto the
/// handful of colours the theme has, and the grouping is what says whether
/// two rows are the same sort of thing.
#[test]
fn the_kinds_that_look_different_are_different() {
    let of = |kind: u8| {
        let reply = json!([{
            "name": "x",
            "kind": kind,
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 0, "character": 1 } },
            "selectionRange": { "start": { "line": 0, "character": 0 },
                                "end": { "line": 0, "character": 1 } }
        }]);
        outline::symbols_in(Ok(reply))[0].kind
    };

    assert_eq!(of(12), SyntaxKind::Function, "a function");
    assert_eq!(of(6), SyntaxKind::Function, "a method");
    assert_eq!(of(5), SyntaxKind::Type, "a class");
    assert_eq!(of(23), SyntaxKind::Type, "a struct");
    assert_eq!(of(14), SyntaxKind::Constant, "a constant");
    assert_eq!(of(8), SyntaxKind::Property, "a field");
    assert_eq!(of(2), SyntaxKind::Keyword, "a module");
    assert_ne!(of(12), of(5), "a function and a class");
    assert_ne!(of(14), of(12), "a constant and a function");
}
