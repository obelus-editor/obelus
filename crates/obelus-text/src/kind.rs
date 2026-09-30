//! What a piece of text is.
//!
//! One vocabulary, named once, for the four things that have an opinion
//! about it: the grammar that captures a span, the theme that colours it,
//! the icon beside a symbol in a list, and the outline that labels one. It
//! is here rather than with any of them because a kind belongs to none of
//! them -- put it with the theme and a grammar cannot name what it found
//! without the colours; put it with the grammars and the theme carries
//! twenty-five C parsers to name an enum with no dependencies at all.

/// What a highlighted span is.
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
    /// A log's `ERROR`.
    ///
    /// Not something a grammar's captures produce: a log has no grammar,
    /// and the reading of one is the only thing that says a line is one.
    /// Here rather than beside the change colours because a log's level is
    /// what a span *is*, the same way a keyword is, and it gets a colour
    /// by the same route.
    Error,
    /// A log's `WARN`.
    Warning,
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
            // A `'a'` literal, in Haskell and Julia. A string of one
            // character is still a string: what a reader wants is to see
            // where the quotes are, which is what the string colour says.
            "character" => Some(Self::String),
            "comment" => Some(Self::Comment),
            // The older names for a keyword, which the queries written
            // against nvim-treesitter's vocabulary use and the newer ones
            // spell `keyword.conditional` and so reach `keyword` by the
            // fallback: Scala's query says `conditional`, `repeat`,
            // `exception` and `storageclass`, and Agda's and Scala's say
            // `include` where the rest say `keyword.import`.
            "conditional" | "exception" | "include" | "repeat" | "storageclass" => {
                Some(Self::Keyword)
            }
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
            // `float` and `method` are the same older vocabulary again,
            // against `number.float` and `function.method`.
            "float" => Some(Self::Number),
            "function" | "method" => Some(Self::Function),
            "keyword" => Some(Self::Keyword),
            "label" => Some(Self::Label),
            // The name of a scope: a C# or Julia module, an OCaml or Agda
            // one, a Scala or PHP namespace. `Keyword` because that is the
            // answer `tags::kind_of` already gives the same three words on
            // the outline, and one name drawn two colours by two tables is
            // the thing this enum exists to prevent -- it is the vocabulary
            // the grammar, the theme, the icon and the outline all share.
            // Before these languages arrived no shipped query produced
            // `@module` or `@namespace`, so the two tables had never yet had
            // to agree.
            "module" | "namespace" => Some(Self::Keyword),
            "number" => Some(Self::Number),
            "operator" => Some(Self::Operator),
            // A parameter is a variable, which is what the newer name
            // (`variable.parameter`) already says; Scala's query uses the
            // older one.
            "parameter" => Some(Self::Variable),
            "property" => Some(Self::Property),
            "punctuation" => Some(Self::Punctuation),
            "string" => Some(Self::String),
            // An HTML element, a CSS selector, a JSX component. The name of
            // the kind of thing the element is, which is what a type is, and
            // in JSX a capitalised tag *is* a type. `tag.error` -- a close
            // tag matching nothing -- falls back to this rather than getting
            // a colour of its own: Obelus does not show diagnostics yet, and
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
