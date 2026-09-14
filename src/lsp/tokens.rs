//! What a language server says each run of characters in a file is.
//!
//! `textDocument/semanticTokens/full`: the server classifies every token in
//! the document -- keyword, variable, function, type, comment -- from the
//! compiler's own understanding of the language rather than from a grammar's
//! guess. obelus asks it one question: is the thing under the cursor a name
//! anybody could ask about, or is it `match`?
//!
//! A whole file at a time, and cached, because the answer is needed
//! *synchronously*: whether a command is offered is decided on every key
//! press and every time a list is built, and a round trip cannot be made
//! there. A response describes one document version and is used only while
//! the document is still on it -- an edit moves every token after it, and a
//! classification for text that has moved is worse than none.

use lsp_types::{Position, PositionEncodingKind, SemanticTokensLegend};

/// The kinds that are not something to ask a question about.
///
/// A list of what is *not* a name rather than of what is: the protocol's
/// twenty-odd types are a floor and every server adds its own -- rust
/// analyzer alone has `lifetime`, `builtinType`, `derive` and a dozen kinds
/// of punctuation. An unknown kind counts as a name, because hiding a
/// question obelus was unsure about is the worse of the two mistakes: a
/// question that comes back with nothing costs a round trip, and one that is
/// never offered costs the reader the feature.
const NOT_NAMES: &[&str] = &[
    "keyword",
    "selfKeyword",
    "modifier",
    "comment",
    "string",
    "escapeSequence",
    "formatSpecifier",
    "number",
    "boolean",
    "character",
    "regexp",
    "operator",
    "arithmetic",
    "bitwise",
    "comparison",
    "logical",
    "punctuation",
    "brace",
    "bracket",
    "parenthesis",
    "angle",
    "comma",
    "semicolon",
    "colon",
    "dot",
];

/// One run of characters, and what the server called it.
#[derive(Clone, Copy, Debug)]
struct Token {
    line: u32,
    /// Where it starts, in whichever units the server agreed to.
    start: u32,
    length: u32,
    /// Which of the legend's kinds it is.
    kind: u32,
}

/// A file's tokens, as the server last described them.
#[derive(Debug)]
pub struct Tokens {
    /// Which document version they describe.
    ///
    /// The whole of what makes them usable: an edit moves every token after
    /// it, so tokens for a version that has been replaced answer about text
    /// that is not there.
    version: i32,
    /// The server's own names for the kinds, in the order it numbers them.
    legend: Vec<String>,
    /// Which units the positions are counted in.
    ///
    /// Carried rather than looked up when asked, because the answer is a
    /// fact about the reply and the server could in principle be restarted
    /// with another one.
    encoding: PositionEncodingKind,
    /// In the order the server sent them, which is document order.
    tokens: Vec<Token>,
}

impl Tokens {
    /// Reads the flat array the protocol sends.
    ///
    /// Five numbers per token, each position relative to the token before
    /// it: the line as a delta from the last token's line, and the start as
    /// a delta from the last token's start where they share a line and from
    /// the beginning of the line where they do not. A trailing group of
    /// fewer than five numbers is a malformed reply and is dropped.
    #[must_use]
    pub fn decode(
        data: &[u32],
        legend: &SemanticTokensLegend,
        encoding: PositionEncodingKind,
        version: i32,
    ) -> Self {
        let mut tokens = Vec::with_capacity(data.len() / 5);
        let (mut line, mut start) = (0u32, 0u32);
        for group in data.as_chunks::<5>().0 {
            let [delta_line, delta_start, length, kind, _modifiers] = *group;
            line = line.saturating_add(delta_line);
            start = match delta_line {
                0 => start.saturating_add(delta_start),
                _ => delta_start,
            };
            tokens.push(Token {
                line,
                start,
                length,
                kind,
            });
        }
        Self {
            version,
            legend: legend
                .token_types
                .iter()
                .map(|kind| kind.as_str().to_string())
                .collect(),
            encoding,
            tokens,
        }
    }

    /// Which units the positions are counted in.
    #[must_use]
    pub const fn encoding(&self) -> &PositionEncodingKind {
        &self.encoding
    }

    /// Whether the token at a position is a name somebody could ask about.
    ///
    /// `None` where these tokens are about a different version of the
    /// document, which is the caller's cue to fall back to what it can work
    /// out itself.
    ///
    /// A position no token covers is not a name: the server classified the
    /// whole file, so a gap is whitespace or something it saw no reason to
    /// name.
    #[must_use]
    pub fn name_at(&self, version: i32, at: Position) -> Option<bool> {
        if version != self.version {
            return None;
        }
        let covering = self.tokens.iter().find(|token| {
            token.line == at.line
                && at.character >= token.start
                && at.character < token.start.saturating_add(token.length)
        });
        let Some(token) = covering else {
            return Some(false);
        };
        let kind = self.legend.get(token.kind as usize).map(String::as_str);
        // A kind the legend does not have is a server numbering its own
        // tokens wrongly. Offering the questions is the safe way to be
        // wrong about it.
        Some(kind.is_none_or(|kind| !NOT_NAMES.contains(&kind)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legend a server sends: its own names, in the order it numbers
    /// them. Nothing may be assumed about the order -- these are two
    /// different servers as far as a client is concerned.
    fn legend(kinds: &[&str]) -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: kinds
                .iter()
                .map(|kind| {
                    lsp_types::SemanticTokenType::new(Box::leak(
                        (*kind).to_string().into_boxed_str(),
                    ))
                })
                .collect(),
            token_modifiers: Vec::new(),
        }
    }

    /// `fn main() { match x {} }` over three lines, as a server would send
    /// it: `fn` and `match` keywords, `main` a function, `x` a variable.
    fn sample() -> Tokens {
        // line, start, length, kind -- each relative to the token before it.
        let data = [
            0, 0, 2, 0, 0, // `fn` at 0:0
            0, 3, 4, 1, 0, // `main` at 0:3, on the same line
            1, 4, 5, 0, 0, // `match` at 1:4, a line down
            0, 6, 1, 2, 0, // `x` at 1:10
        ];
        Tokens::decode(
            &data,
            &legend(&["keyword", "function", "variable"]),
            PositionEncodingKind::UTF16,
            7,
        )
    }

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn a_keyword_is_not_a_name() {
        let tokens = sample();
        assert_eq!(tokens.name_at(7, at(0, 0)), Some(false), "`fn`");
        assert_eq!(tokens.name_at(7, at(1, 4)), Some(false), "`match`");
        // Anywhere inside it, not only where it starts.
        assert_eq!(tokens.name_at(7, at(1, 8)), Some(false), "inside `match`");
    }

    #[test]
    fn a_function_and_a_variable_are_names() {
        let tokens = sample();
        assert_eq!(tokens.name_at(7, at(0, 3)), Some(true), "`main`");
        assert_eq!(tokens.name_at(7, at(1, 10)), Some(true), "`x`");
    }

    /// The deltas are what makes this worth testing: a start is counted from
    /// the token before it on the same line and from the beginning of the
    /// line otherwise, and getting that backwards shifts everything after
    /// the first line break.
    #[test]
    fn the_line_after_a_break_starts_from_the_left() {
        let tokens = sample();
        // `match` is at column 4, not at 3 + 4.
        assert_eq!(tokens.name_at(7, at(1, 3)), Some(false), "before `match`");
        assert_eq!(tokens.name_at(7, at(1, 4)), Some(false), "`match` itself");
        // And `x` is at 4 + 6, not at 6.
        assert_eq!(tokens.name_at(7, at(1, 6)), Some(false), "inside `match`");
        assert_eq!(tokens.name_at(7, at(1, 10)), Some(true), "`x`");
    }

    /// Whitespace, a bracket, past the end of a line: the server classified
    /// the whole file, so a gap is something it saw no reason to name.
    #[test]
    fn a_position_no_token_covers_is_not_a_name() {
        let tokens = sample();
        assert_eq!(tokens.name_at(7, at(0, 2)), Some(false), "the space");
        assert_eq!(tokens.name_at(7, at(0, 40)), Some(false), "past the end");
        assert_eq!(tokens.name_at(7, at(9, 0)), Some(false), "a line with none");
    }

    /// Every server adds kinds of its own, and a question that is never
    /// offered costs the reader the feature, where one that comes back
    /// empty costs a round trip.
    #[test]
    fn a_kind_obelus_does_not_know_is_a_name() {
        let data = [
            0, 0, 8, 0, 0, // a kind of the server's own
            1, 0, 4, 9, 0, // and a kind the legend does not have at all
        ];
        let tokens = Tokens::decode(
            &data,
            &legend(&["lifetime"]),
            PositionEncodingKind::UTF16,
            1,
        );
        assert_eq!(tokens.name_at(1, at(0, 2)), Some(true), "a kind of its own");
        // A number past the end of the legend is a server numbering its own
        // tokens wrongly. Offering the questions is the safe way to be wrong
        // about it.
        assert_eq!(
            tokens.name_at(1, at(1, 2)),
            Some(true),
            "a kind that is not in the legend"
        );
    }

    /// An edit moves every token after it, so a classification of the
    /// version before it answers about text that is not there.
    #[test]
    fn tokens_for_another_version_answer_nothing() {
        let tokens = sample();
        assert_eq!(tokens.name_at(8, at(0, 3)), None, "one version on");
        assert_eq!(tokens.name_at(6, at(0, 3)), None, "one version back");
    }

    /// A reply that ends mid-token is malformed, and the tokens before it
    /// are still good.
    #[test]
    fn a_trailing_part_of_a_token_is_dropped() {
        let data = [0, 0, 2, 0, 0, 0, 3, 4];
        let tokens = Tokens::decode(
            &data,
            &legend(&["keyword", "function"]),
            PositionEncodingKind::UTF16,
            1,
        );
        assert_eq!(tokens.name_at(1, at(0, 0)), Some(false));
        assert_eq!(tokens.name_at(1, at(0, 3)), Some(false), "never arrived");
    }
}
