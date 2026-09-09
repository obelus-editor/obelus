//! Which bracket closes which.
//!
//! Values in, a pair out. The rules worth pinning down are the ones a scan
//! gets wrong: nesting, an unclosed bracket, and a bracket that is not
//! syntax at all because it is inside a string or a comment.

mod support;

use obelus::{
    coordinates::ByteOffset,
    syntax::{LanguageId, brackets, highlight::Highlights, parse::SyntaxState},
    text::Text,
};

/// Everything on screen, which is what the editor passes.
fn scan(source: &str, at: usize) -> Option<(usize, usize)> {
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Rust, &text).expect("parsing");
    let mut highlights = Highlights::default();
    let all = ByteOffset::new(0)..text.byte_length();
    highlights.refresh(&state, &text, all.clone());
    brackets::pair_at(&text, &highlights, ByteOffset::new(at), all)
        .map(|(open, close)| (open.get(), close.get()))
}

#[test]
fn a_bracket_finds_its_partner_from_either_end() {
    let source = "fn main() {}\n";
    let open = source.find('(').expect("a bracket");
    let close = source.find(')').expect("a bracket");

    assert_eq!(scan(source, open), Some((open, close)));
    // And from the closing one, which is the same pair found the other way.
    assert_eq!(scan(source, close), Some((open, close)));

    let brace = source.find('{').expect("a brace");
    assert_eq!(scan(source, brace), Some((brace, brace + 1)));
}

#[test]
fn nesting_is_counted_rather_than_matched_by_shape() {
    let source = "let x = f(g(1), h(2));\n";
    let outer = source.find('(').expect("a bracket");
    let last = source.rfind(')').expect("a bracket");

    // The outer pair, not the first closing bracket that comes along.
    assert_eq!(scan(source, outer), Some((outer, last)));

    let inner = source.find("g(").expect("g") + 1;
    let inner_close = source.find("),").expect("the close");
    assert_eq!(scan(source, inner), Some((inner, inner_close)));
}

/// The rule that makes this worth writing down. `"("` is not an unclosed
/// bracket, and a scan that counted it would pair the wrong two characters
/// on every line of code that mentions one.
#[test]
fn brackets_in_strings_and_comments_are_not_syntax() {
    let source = "fn main() { let s = \"(\"; }\n";
    let open = source.find('(').expect("a bracket");
    let close = source.find(')').expect("a bracket");
    assert_eq!(scan(source, open), Some((open, close)));

    // The brace's partner is the last one, and the bracket in the string in
    // between does not enter into it.
    let brace = source.find('{').expect("a brace");
    let end = source.rfind('}').expect("a brace");
    assert_eq!(scan(source, brace), Some((brace, end)));

    // And standing on the one inside the string gets no answer at all: it
    // has no partner in any useful sense.
    let quoted = source.find("\"(").expect("the string") + 1;
    assert_eq!(scan(source, quoted), None);

    let commented = "fn main() { // (\n}\n";
    let brace = commented.find('{').expect("a brace");
    let end = commented.rfind('}').expect("a brace");
    assert_eq!(scan(commented, brace), Some((brace, end)));
}

#[test]
fn an_unclosed_bracket_has_no_partner() {
    let source = "let x = f(1;\n";
    let open = source.find('(').expect("a bracket");
    assert_eq!(scan(source, open), None);
}

#[test]
fn a_cursor_on_something_else_gets_nothing() {
    let source = "fn main() {}\n";
    assert_eq!(scan(source, 0), None, "on a letter");
    assert_eq!(scan(source, 2), None, "on a space");
    assert_eq!(scan(source, source.len() - 1), None, "on the newline");
}

/// The partner has to be on screen. A scan that ran to the end of a large
/// file to decide it is not would be work spent on a cell nobody sees.
#[test]
fn a_partner_off_screen_is_no_answer() {
    let source = "fn main() {\n    let x = 1;\n}\n";
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Rust, &text).expect("parsing");
    let mut highlights = Highlights::default();
    let all = ByteOffset::new(0)..text.byte_length();
    highlights.refresh(&state, &text, all);

    let brace = source.find('{').expect("a brace");
    // A window that stops before the closing brace.
    let window = ByteOffset::new(0)..ByteOffset::new(source.find("let").expect("let"));
    assert_eq!(
        brackets::pair_at(&text, &highlights, ByteOffset::new(brace), window),
        None
    );
}
