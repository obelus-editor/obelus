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
            // C and C++ call `;` and `,` delimiters where the others call
            // them punctuation.
            "delimiter" => Some(Self::Punctuation),
            // The interpolated part of a string: `${name}`, `f"{value}"`,
            // `$(command)`. It is code rather than string, and what it names
            // is nearly always a variable. Anything captured inside it still
            // wins, because the innermost capture does.
            "embedded" => Some(Self::Variable),
            "escape" => Some(Self::Escape),
            "function" => Some(Self::Function),
            "keyword" => Some(Self::Keyword),
            "label" => Some(Self::Label),
            "number" => Some(Self::Number),
            "operator" => Some(Self::Operator),
            "property" => Some(Self::Property),
            "punctuation" => Some(Self::Punctuation),
            "string" => Some(Self::String),
            // An HTML element, a CSS selector, a JSX component. The name of
            // the kind of thing the element is, which is what a type is, and
            // in JSX a capitalised tag *is* a type. `tag.error` -- a close
            // tag matching nothing -- falls back to this rather than getting
            // a colour of its own: obelus does not show diagnostics yet, and
            // a colour that only appears in broken files is one nobody has
            // learnt.
            "tag" => Some(Self::Type),
            // Markdown's queries use the older `text.*` names, and each
            // means something a code theme already has a colour for: a
            // heading is the structure of the document, a literal is a code
            // span or a fenced block, a uri is a value, a reference is a
            // name pointing at one.
            "text.title" => Some(Self::Keyword),
            "text.literal" => Some(Self::String),
            "text.uri" => Some(Self::Constant),
            "text.reference" => Some(Self::Property),
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

/// A wash of `hue` over `base`: the mark's colour at `percent` strength.
///
/// Hand-picked tints drift away from the mark they belong to -- a green that
/// is not quite the margin's green reads as a third colour rather than as the
/// same claim said louder. Mixing keeps the hue exactly and lets the page
/// keep its own cast, which is what makes a wash look like part of the editor
/// instead of a highlighter pen.
///
/// Only [`Color::Rgb`] mixes; anything else has no components to mix and is
/// returned unchanged. The built-in themes are all RGB, and a terminal that
/// cannot do RGB cannot show a wash either.
#[must_use]
pub const fn tint(base: Color, hue: Color, percent: u32) -> Color {
    let (Color::Rgb(base_red, base_green, base_blue), Color::Rgb(red, green, blue)) = (base, hue)
    else {
        return base;
    };
    const fn mix(base: u8, hue: u8, percent: u32) -> u8 {
        let mixed = (base as u32 * (100 - percent) + hue as u32 * percent) / 100;
        mixed as u8
    }
    Color::Rgb(
        mix(base_red, red, percent),
        mix(base_green, green, percent),
        mix(base_blue, blue, percent),
    )
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
    /// Behind the run of characters a preview is about — the symbol a
    /// language server named.
    pub marked_background: Color,
    /// Behind the characters selected by the reader.
    pub selection_background: Color,
    /// The margin's mark beside a line that is new since the last commit.
    pub change_added: Color,
    /// Beside a line that replaced something.
    pub change_modified: Color,
    /// At the seam where lines were removed.
    ///
    /// Three colours rather than one: the margin is making three different
    /// claims about the file, and a reader who cannot tell them apart has to
    /// open every mark to find out which it was.
    pub change_removed: Color,
    /// Behind an opened hunk's lines: the same three claims again, said by
    /// the whole row instead of by one column.
    ///
    /// Built with [`tint`] from the mark's own colour, so the row and the
    /// mark beside it are the same hue and a reader does not have to learn
    /// two palettes.
    ///
    /// An opened hunk is being *read*, not glanced at, and the margin's mark
    /// is too small to answer "which of these lines am I looking at" while
    /// the removed ones sit among them. Tints rather than the mark's own
    /// colour: text has to stay readable on top of it.
    pub change_added_background: Color,
    /// Behind the lines of an opened hunk that replaced something.
    pub change_modified_background: Color,
    /// Behind the removed lines an opened hunk shows.
    pub change_removed_background: Color,
    /// Behind the bracket under the cursor and the one that closes it.
    ///
    /// Its own colour rather than [`Theme::marked_background`]: one appears
    /// wherever the cursor rests and the other means "this is the thing you
    /// went looking for", and two cells that mean different things should
    /// not look the same.
    pub bracket_background: Color,
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
    use ratatui::style::Color;

    use super::{SyntaxKind, tint};

    /// A wash has to be a wash: nearer the page it is drawn on than the mark
    /// it is made from. Code is read on top of it, and a row in the mark's
    /// own colour would be a stripe with text lost in it -- while a row that
    /// is exactly the page says nothing at all.
    #[test]
    fn a_wash_stays_close_to_the_page() {
        let page = Color::Rgb(24, 24, 27);
        let mark = Color::Rgb(34, 197, 94);
        let Color::Rgb(red, green, blue) = tint(page, mark, 18) else {
            panic!("a wash of two RGB colours is RGB");
        };
        assert_ne!((red, green, blue), (24, 24, 27), "the wash is the page");
        assert_ne!((red, green, blue), (34, 197, 94), "the wash is the mark");
        // The green channel is where the two differ most, so it is where a
        // wash that had drifted towards the mark would show.
        assert!(
            i32::from(green) - 24 < 197 - i32::from(green),
            "the wash is nearer the mark than the page: {green}"
        );
    }

    /// A terminal that cannot do RGB cannot show a wash either, and a theme
    /// built on named colours should get its page back rather than a colour
    /// invented for it.
    #[test]
    fn a_wash_of_something_unmixable_is_the_page() {
        assert_eq!(tint(Color::Black, Color::Red, 50), Color::Black);
        assert_eq!(
            tint(Color::Rgb(1, 2, 3), Color::Red, 50),
            Color::Rgb(1, 2, 3)
        );
    }

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
