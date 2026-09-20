//! Languages, their grammars, and their highlight queries.

pub mod brackets;
pub mod highlight;
pub mod parse;
pub mod tags;

use std::{path::Path, sync::OnceLock};

use obelus_text::kind::SyntaxKind;
use tree_sitter::{Language, Query};

/// A language obelus can highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LanguageId {
    /// Rust.
    Rust,
    /// TOML.
    Toml,
    /// JSON.
    Json,
    /// Python.
    Python,
    /// JavaScript, JSX included.
    JavaScript,
    /// TypeScript without JSX.
    TypeScript,
    /// TypeScript with JSX.
    Tsx,
    /// Go.
    Go,
    /// C.
    C,
    /// C++.
    Cpp,
    /// A shell script.
    Bash,
    /// CSS.
    Css,
    /// HTML.
    Html,
    /// YAML.
    Yaml,
    /// Markdown, block structure only.
    Markdown,
}

impl LanguageId {
    /// The language a file's extension implies, if obelus knows it.
    ///
    /// Extension only. Content sniffing and modelines are guesses that are
    /// wrong in exactly the cases where being wrong is confusing, and a file
    /// with no highlighting still reads fine.
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        Self::for_name(path.extension()?.to_str()?)
    }

    /// The language a name implies, if obelus knows it.
    ///
    /// Extensions and the names themselves, because the two arrive from
    /// different places and mean the same thing: a file is called `.rs` and
    /// a fenced block in markdown is called `rust`.
    #[must_use]
    pub fn for_name(name: &str) -> Option<Self> {
        match name {
            "rs" | "rust" => Some(Self::Rust),
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            "py" | "pyi" | "pyw" | "python" => Some(Self::Python),
            // JSX goes to the JavaScript grammar, whose query is shipped with
            // the JSX rules appended.
            "js" | "mjs" | "cjs" | "jsx" | "javascript" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" | "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            // `.h` to C, which is the convention and is right for the header
            // that came with a C library. A C++ header called `.h` parses as
            // C well enough to read: the declarations look the same, and
            // being wrong here costs some highlighting rather than a wrong
            // answer.
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ipp" | "c++" => Some(Self::Cpp),
            "sh" | "bash" | "zsh" | "ksh" | "shell" | "console" => Some(Self::Bash),
            "css" => Some(Self::Css),
            "html" | "htm" | "xhtml" => Some(Self::Html),
            "yaml" | "yml" => Some(Self::Yaml),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    /// What this language starts a line comment with, if it has one.
    ///
    /// A column of the same table `name` and `grammar` are columns of,
    /// because that is what it is: one fact per language, and the only one
    /// that toggling a comment needs. The toggling itself knows no
    /// languages at all -- helix and zed both keep this as data for the
    /// same reason, in a configuration file because their languages come
    /// from outside. obelus's are compiled in beside their grammars, so
    /// this is compiled in beside them.
    ///
    /// `None` for a language with only block comments. Better to say so and
    /// have the command dim than to guess at `//` and leave a reader with a
    /// file their tools will not parse.
    #[must_use]
    pub const fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Rust
            | Self::JavaScript
            | Self::TypeScript
            | Self::Tsx
            | Self::Go
            | Self::C
            | Self::Cpp => Some("//"),
            Self::Python | Self::Bash | Self::Toml | Self::Yaml => Some("#"),
            // Only block comments, or none at all: CSS and HTML have
            // `/* */` and `<!-- -->`, JSON has nothing, and markdown's
            // comment is an HTML one.
            Self::Json | Self::Css | Self::Html | Self::Markdown => None,
        }
    }

    /// The name to show.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::C => "c",
            Self::Cpp => "c++",
            Self::Bash => "bash",
            Self::Css => "css",
            Self::Html => "html",
            Self::Yaml => "yaml",
            Self::Markdown => "markdown",
        }
    }

    /// Every language, for the tests that have to cover all of them.
    ///
    /// A language left out of this list is one whose query is never compiled
    /// by the tests, and a query that does not compile takes the whole
    /// program down the first time that language is opened.
    pub const ALL: &'static [Self] = &[
        Self::Rust,
        Self::Toml,
        Self::Json,
        Self::Python,
        Self::JavaScript,
        Self::TypeScript,
        Self::Tsx,
        Self::Go,
        Self::C,
        Self::Cpp,
        Self::Bash,
        Self::Css,
        Self::Html,
        Self::Yaml,
        Self::Markdown,
    ];
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
    static PYTHON: OnceLock<Grammar> = OnceLock::new();
    static JAVASCRIPT: OnceLock<Grammar> = OnceLock::new();
    static TYPESCRIPT: OnceLock<Grammar> = OnceLock::new();
    static TSX: OnceLock<Grammar> = OnceLock::new();
    static GO: OnceLock<Grammar> = OnceLock::new();
    static C: OnceLock<Grammar> = OnceLock::new();
    static CPP: OnceLock<Grammar> = OnceLock::new();
    static BASH: OnceLock<Grammar> = OnceLock::new();
    static CSS: OnceLock<Grammar> = OnceLock::new();
    static HTML: OnceLock<Grammar> = OnceLock::new();
    static YAML: OnceLock<Grammar> = OnceLock::new();
    static MARKDOWN: OnceLock<Grammar> = OnceLock::new();

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
        LanguageId::Python => PYTHON.get_or_init(|| {
            Grammar::new(
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::HIGHLIGHTS_QUERY,
            )
        }),
        // The JSX rules are appended rather than kept for `.jsx` alone: they
        // capture nodes a plain JavaScript file does not contain, so they
        // cost nothing there, and one grammar per file extension would be
        // three copies of JavaScript.
        LanguageId::JavaScript => JAVASCRIPT.get_or_init(|| {
            Grammar::new(
                tree_sitter_javascript::LANGUAGE.into(),
                &format!(
                    "{}\n{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
                ),
            )
        }),
        // TypeScript's own query covers only what TypeScript adds to
        // JavaScript. On its own it highlights the types and leaves the code
        // around them plain, which reads as a broken file rather than as a
        // missing query.
        LanguageId::TypeScript => TYPESCRIPT.get_or_init(|| {
            Grammar::new(
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                &typescript_query(),
            )
        }),
        LanguageId::Tsx => TSX.get_or_init(|| {
            Grammar::new(
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                &format!(
                    "{}\n{}",
                    typescript_query(),
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
                ),
            )
        }),
        LanguageId::Go => GO.get_or_init(|| {
            Grammar::new(
                tree_sitter_go::LANGUAGE.into(),
                tree_sitter_go::HIGHLIGHTS_QUERY,
            )
        }),
        LanguageId::C => C.get_or_init(|| {
            Grammar::new(
                tree_sitter_c::LANGUAGE.into(),
                tree_sitter_c::HIGHLIGHT_QUERY,
            )
        }),
        LanguageId::Cpp => CPP.get_or_init(|| {
            Grammar::new(
                tree_sitter_cpp::LANGUAGE.into(),
                &format!(
                    "{}\n{}",
                    tree_sitter_c::HIGHLIGHT_QUERY,
                    tree_sitter_cpp::HIGHLIGHT_QUERY
                ),
            )
        }),
        LanguageId::Bash => BASH.get_or_init(|| {
            Grammar::new(
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY,
            )
        }),
        LanguageId::Css => CSS.get_or_init(|| {
            Grammar::new(
                tree_sitter_css::LANGUAGE.into(),
                tree_sitter_css::HIGHLIGHTS_QUERY,
            )
        }),
        LanguageId::Html => HTML.get_or_init(|| {
            Grammar::new(
                tree_sitter_html::LANGUAGE.into(),
                tree_sitter_html::HIGHLIGHTS_QUERY,
            )
        }),
        LanguageId::Yaml => YAML.get_or_init(|| {
            Grammar::new(
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY,
            )
        }),
        // The *block* grammar only. Markdown is two grammars -- one for the
        // structure of a document and one for what is inside a paragraph --
        // and using the second means parsing the ranges the first hands over,
        // which is the injection machinery obelus does not have yet.
        //
        // What that costs: emphasis, links and code spans inside a paragraph
        // stay plain. What it buys: headings, fenced code, lists, block
        // quotes and rules, which is what a reader skimming a README is
        // looking at.
        LanguageId::Markdown => MARKDOWN.get_or_init(|| {
            Grammar::new(
                tree_sitter_md::LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            )
        }),
    }
}

/// JavaScript's rules, then TypeScript's.
///
/// In that order, because a later pattern wins where two match the same node
/// and TypeScript's are the more specific.
fn typescript_query() -> String {
    format!(
        "{}
{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_typescript::HIGHLIGHTS_QUERY
    )
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
        assert_eq!(
            LanguageId::for_path(Path::new("setup.py")),
            Some(LanguageId::Python)
        );
        // JSX to the JavaScript grammar and TSX to its own: the TypeScript
        // grammar comes in two, and only one of them parses a tag.
        assert_eq!(
            LanguageId::for_path(Path::new("app.jsx")),
            Some(LanguageId::JavaScript)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("app.tsx")),
            Some(LanguageId::Tsx)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("index.ts")),
            Some(LanguageId::TypeScript)
        );
        // `.h` is a C header by convention, whichever language wrote it.
        assert_eq!(
            LanguageId::for_path(Path::new("zlib.h")),
            Some(LanguageId::C)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("main.cc")),
            Some(LanguageId::Cpp)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("ci.yml")),
            Some(LanguageId::Yaml)
        );
        assert_eq!(
            LanguageId::for_path(Path::new("README.md")),
            Some(LanguageId::Markdown)
        );
        assert_eq!(LanguageId::for_path(Path::new("Makefile")), None);
    }

    /// Every language has a name, and no two share one. The name is what the
    /// server table and the logs are keyed by in prose.
    #[test]
    fn every_language_has_its_own_name() {
        let names: std::collections::HashSet<&str> =
            LanguageId::ALL.iter().map(|id| id.name()).collect();
        assert_eq!(names.len(), LanguageId::ALL.len());
    }

    /// A capture the theme has no kind for renders as plain text, silently. If
    /// a whole language's queries went unrecognized the file would simply look
    /// unhighlighted, so assert that most of what the shipped queries capture
    /// is understood.
    #[test]
    fn the_theme_understands_what_the_queries_capture() {
        for language in LanguageId::ALL.iter().copied() {
            let grammar = grammar(language);
            let names = grammar.query().capture_names();
            let unknown: Vec<&str> = names
                .iter()
                .enumerate()
                .filter(|(index, _)| grammar.kind(*index as u32).is_none())
                .map(|(_, name)| *name)
                // `@none` is the one capture that *means* no colour: the
                // queries use it to stop an enclosing pattern from painting
                // something, and a colour for it would be inventing one.
                .filter(|name| *name != "none")
                .collect();
            assert!(
                unknown.is_empty(),
                "{} captures nothing the theme knows: {unknown:?}",
                language.name()
            );
        }
    }
}
