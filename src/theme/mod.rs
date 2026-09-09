//! Colours.
//!
//! Every field has a reader in the renderer. A colour that nothing paints with
//! would compile, store, appear in the documentation, and change nothing when
//! set — which is the one kind of mistake here that announces itself to
//! nobody.

pub mod builtin;

use ratatui::style::Color;

/// What a highlighted span is, as far as a theme is concerned.
///
/// A fixed enum rather than the capture name as a string: the name is resolved
/// once when a grammar's query is compiled, so the per-frame cost is an array
/// index, and a theme cannot quietly fail to cover something.
///
/// The variants are the roots of the capture names the shipped queries
/// actually use — nothing is here speculatively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    /// `#[derive(Debug)]`.
    Attribute,
    /// `true`, `false`.
    Boolean,
    /// A comment, documentation included.
    Comment,
    /// A constant, builtin ones included.
    Constant,
    /// A struct or enum name in a pattern or a literal.
    Constructor,
    /// `\n` inside a string.
    Escape,
    /// A function, method or macro name.
    Function,
    /// `fn`, `let`, `match`.
    Keyword,
    /// A loop label.
    Label,
    /// A numeric literal.
    Number,
    /// `+`, `=>`, `?`.
    Operator,
    /// A field or key name.
    Property,
    /// Brackets and separators.
    Punctuation,
    /// A string literal.
    String,
    /// A type name, builtin ones included.
    Type,
    /// A variable, parameter or `self`.
    Variable,
}

impl SyntaxKind {
    /// The kind a capture name means.
    ///
    /// Capture names are hierarchical — `function.method`, `string.special.key`
    /// — and a theme is not obliged to name every leaf. One dotted segment is
    /// dropped at a time until something matches, so `string.special.key`
    /// lands on `string` rather than on nothing. Without the fallback a query
    /// loses most of its highlighting to names the theme never heard of.
    #[must_use]
    pub fn for_capture(name: &str) -> Option<Self> {
        let mut remaining = name;
        loop {
            if let Some(kind) = Self::exact(remaining) {
                return Some(kind);
            }
            remaining = remaining.rsplit_once('.')?.0;
        }
    }

    fn exact(name: &str) -> Option<Self> {
        match name {
            "attribute" => Some(Self::Attribute),
            "boolean" => Some(Self::Boolean),
            "comment" => Some(Self::Comment),
            "constant" => Some(Self::Constant),
            "constructor" => Some(Self::Constructor),
            "escape" => Some(Self::Escape),
            "function" => Some(Self::Function),
            "keyword" => Some(Self::Keyword),
            "label" => Some(Self::Label),
            "number" => Some(Self::Number),
            "operator" => Some(Self::Operator),
            "property" => Some(Self::Property),
            "punctuation" => Some(Self::Punctuation),
            "string" => Some(Self::String),
            "type" => Some(Self::Type),
            "variable" => Some(Self::Variable),
            _ => None,
        }
    }
}

/// One colour per [`SyntaxKind`].
#[derive(Clone, Copy, Debug)]
pub struct SyntaxTheme {
    /// See [`SyntaxKind::Attribute`].
    pub attribute: Color,
    /// See [`SyntaxKind::Boolean`].
    pub boolean: Color,
    /// See [`SyntaxKind::Comment`].
    pub comment: Color,
    /// See [`SyntaxKind::Constant`].
    pub constant: Color,
    /// See [`SyntaxKind::Constructor`].
    pub constructor: Color,
    /// See [`SyntaxKind::Escape`].
    pub escape: Color,
    /// See [`SyntaxKind::Function`].
    pub function: Color,
    /// See [`SyntaxKind::Keyword`].
    pub keyword: Color,
    /// See [`SyntaxKind::Label`].
    pub label: Color,
    /// See [`SyntaxKind::Number`].
    pub number: Color,
    /// See [`SyntaxKind::Operator`].
    pub operator: Color,
    /// See [`SyntaxKind::Property`].
    pub property: Color,
    /// See [`SyntaxKind::Punctuation`].
    pub punctuation: Color,
    /// See [`SyntaxKind::String`].
    pub string: Color,
    /// See [`SyntaxKind::Type`].
    pub type_name: Color,
    /// See [`SyntaxKind::Variable`].
    pub variable: Color,
}

impl SyntaxTheme {
    /// The colour for a kind.
    #[must_use]
    pub const fn colour(&self, kind: SyntaxKind) -> Color {
        match kind {
            SyntaxKind::Attribute => self.attribute,
            SyntaxKind::Boolean => self.boolean,
            SyntaxKind::Comment => self.comment,
            SyntaxKind::Constant => self.constant,
            SyntaxKind::Constructor => self.constructor,
            SyntaxKind::Escape => self.escape,
            SyntaxKind::Function => self.function,
            SyntaxKind::Keyword => self.keyword,
            SyntaxKind::Label => self.label,
            SyntaxKind::Number => self.number,
            SyntaxKind::Operator => self.operator,
            SyntaxKind::Property => self.property,
            SyntaxKind::Punctuation => self.punctuation,
            SyntaxKind::String => self.string,
            SyntaxKind::Type => self.type_name,
            SyntaxKind::Variable => self.variable,
        }
    }
}

/// A complete set of colours.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// The name the theme picker shows.
    pub name: &'static str,
    /// Behind the text.
    pub background: Color,
    /// The text.
    pub foreground: Color,
    /// Line numbers other than the cursor's.
    pub gutter: Color,
    /// The cursor's line number.
    pub gutter_current: Color,
    /// Behind the status line, across its whole width.
    pub status_background: Color,
    /// The status line's text.
    pub status_foreground: Color,
    /// The status line's marker for a file that can no longer be read.
    pub status_stale: Color,
    /// Behind the selected row of a picker.
    pub picker_selected_background: Color,
    /// The characters of the selected row that the query matched.
    pub picker_match: Color,
    /// The syntax colours.
    pub syntax: SyntaxTheme,
}

impl Theme {
    /// The colour text of this kind is drawn in, falling back to plain text.
    #[must_use]
    pub const fn colour_for(&self, kind: Option<SyntaxKind>) -> Color {
        match kind {
            Some(kind) => self.syntax.colour(kind),
            None => self.foreground,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind;

    #[test]
    fn a_capture_name_falls_back_along_its_dots() {
        assert_eq!(
            SyntaxKind::for_capture("function.method"),
            Some(SyntaxKind::Function)
        );
        assert_eq!(
            SyntaxKind::for_capture("string.special.key"),
            Some(SyntaxKind::String)
        );
        assert_eq!(
            SyntaxKind::for_capture("comment"),
            Some(SyntaxKind::Comment)
        );
        assert_eq!(SyntaxKind::for_capture("nonsense.entirely"), None);
        assert_eq!(SyntaxKind::for_capture(""), None);
    }
}
