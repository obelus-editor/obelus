//! Languages, their grammars, and their highlight queries.

pub mod highlight;
pub mod parse;

use std::{path::Path, sync::OnceLock};

use tree_sitter::{Language, Query};

use crate::theme::SyntaxKind;

/// A language obelus can highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LanguageId {
    /// Rust.
    Rust,
    /// TOML.
    Toml,
    /// JSON.
    Json,
}

impl LanguageId {
    /// The language a file's extension implies, if obelus knows it.
    ///
    /// Extension only. Content sniffing and modelines are guesses that are
    /// wrong in exactly the cases where being wrong is confusing, and a file
    /// with no highlighting still reads fine.
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// The name to show.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Toml => "toml",
            Self::Json => "json",
        }
    }
}

/// A compiled grammar and query, built once and shared.
pub struct Grammar {
    language: Language,
    query: Query,
    /// The theme kind each capture in `query` resolves to, by capture index.
    ///
    /// Resolved here, once, rather than at render time. Two things follow: the
    /// per-frame work is an index into this rather than a string comparison,
    /// and switching theme needs no reparse and invalidates no cache, because
    /// what is cached is which kind of thing a byte is, not what colour it is.
    kinds: Vec<Option<SyntaxKind>>,
}

impl Grammar {
    fn new(language: Language, source: &str) -> Self {
        let query = Query::new(&language, source)
            .expect("a highlight query shipped with obelus should compile");
        let kinds = query
            .capture_names()
            .iter()
            .map(|name| SyntaxKind::for_capture(name))
            .collect();
        Self {
            language,
            query,
            kinds,
        }
    }

    /// The grammar, for the parser.
    #[must_use]
    pub const fn language(&self) -> &Language {
        &self.language
    }

    /// The highlight query.
    #[must_use]
    pub const fn query(&self) -> &Query {
        &self.query
    }

    /// What kind of thing a capture marks, if the theme has an opinion.
    #[must_use]
    pub fn kind(&self, capture: u32) -> Option<SyntaxKind> {
        self.kinds.get(capture as usize).copied().flatten()
    }
}

/// The compiled grammar for a language.
///
/// Compiling a query is not cheap and the result never changes, so each one is
/// built on first use and then shared.
#[must_use]
pub fn grammar(language: LanguageId) -> &'static Grammar {
    static RUST: OnceLock<Grammar> = OnceLock::new();
    static TOML: OnceLock<Grammar> = OnceLock::new();
    static JSON: OnceLock<Grammar> = OnceLock::new();

    match language {
        LanguageId::Rust => RUST.get_or_init(|| {
            Grammar::new(
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            )
        }),
        LanguageId::Toml => TOML.get_or_init(|| {
            Grammar::new(
                tree_sitter_toml_ng::LANGUAGE.into(),
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            )
        }),
        LanguageId::Json => JSON.get_or_init(|| {
            Grammar::new(
                tree_sitter_json::LANGUAGE.into(),
                tree_sitter_json::HIGHLIGHTS_QUERY,
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_languages() {
        assert_eq!(
            LanguageId::for_path(Path::new("src/app.rs")),
            Some(LanguageId::Rust)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("Cargo.toml")),
            Some(LanguageId::Toml)
        );
        assert_eq!(LanguageId::for_path(Path::new("README.md")), None);
        assert_eq!(LanguageId::for_path(Path::new("Makefile")), None);
    }

    /// A capture the theme has no kind for renders as plain text, silently. If
    /// a whole language's queries went unrecognized the file would simply look
    /// unhighlighted, so assert that most of what the shipped queries capture
    /// is understood.
    #[test]
    fn the_theme_understands_what_the_queries_capture() {
        for language in [LanguageId::Rust, LanguageId::Toml, LanguageId::Json] {
            let grammar = grammar(language);
            let names = grammar.query().capture_names();
            let unknown: Vec<&str> = names
                .iter()
                .enumerate()
                .filter(|(index, _)| grammar.kind(*index as u32).is_none())
                .map(|(_, name)| *name)
                .collect();
            assert!(
                unknown.is_empty(),
                "{} captures nothing the theme knows: {unknown:?}",
                language.name()
            );
        }
    }
}
