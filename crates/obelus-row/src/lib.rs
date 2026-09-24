//! A row of laid-out text, and what it is drawn to look like.
//!
//! The currency between whoever lays something out and whoever draws it.
//! Markdown and a log are different things while they are being laid out,
//! and by the time they are rows they are the same thing: runs of text with
//! a look and a place they came from. So one piece of code draws either, and
//! a new way of laying something out is a new crate rather than a new view.
//!
//! Here rather than with either of them, because it belongs to neither: a
//! log's rows would be coming out of a crate called markdown. And not with
//! the drawing, because that would have whoever lays text out depending on
//! the screen -- the direction runs the other way, and always has.

/// What a run of text looks like.
///
/// The *screen's* vocabulary rather than markdown's or a log's: a heading
/// and a level are different ideas about the same thing, which is how a run
/// should be drawn. Roles rather than colours, for the reason the syntax
/// layer caches capture indices: switching theme then needs no re-render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The text itself.
    Plain,
    /// A heading, and how deep it is.
    Heading(u8),
    /// Something quoted from elsewhere: a code span, a field's value.
    Code,
    /// Quieter than the text: a quoted block, a timestamp, a level nobody
    /// reads until they have to.
    Aside,
    /// A mark the layout added rather than the author's words: a bullet, a
    /// table's border, the blanks that hold a column open.
    Mark,
    /// Who said it: a module, a host, a program.
    Name,
    /// What a value is called.
    Key,
    /// Something is wrong.
    Wrong,
    /// Something might be.
    Doubtful,
    /// Code, in the colour the code itself would be.
    ///
    /// What a fenced block in markdown is made of, once the grammar its
    /// fence names has said what each run is. A block with no language, or
    /// one Obelus has no grammar for, stays [`Ink::Code`] -- one colour,
    /// which says "this is code" and nothing more.
    Syntax(obelus_text::kind::SyntaxKind),
}

/// A run of text with one look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// The characters.
    ///
    /// Borrowed where they are the same every time -- a quotation's bar, the
    /// sides of a box round a block of code, a bullet. Those are the
    /// layout's own marks and there are as many of them as there are rows;
    /// a `String` for each was a three-byte allocation for a character that
    /// has never changed and never will.
    ///
    /// Owned for everything else. A run of what somebody wrote *is* a slice
    /// of the source, and says so in [`Self::from`] -- but the rows outlive
    /// the text they were laid out from, which is the whole point of keeping
    /// them, so it cannot be borrowed from there without the two being kept
    /// together.
    pub text: std::borrow::Cow<'static, str>,
    /// How they are drawn.
    pub ink: Ink,
    /// Whether they are emphasised.
    pub bold: bool,
    /// Whether they are emphasised the other way.
    pub italic: bool,
    /// Whether they are struck through.
    ///
    /// Markdown's third emphasis, and the one that cannot be dropped
    /// quietly: bold read as plain is a sentence that has lost a little, and
    /// struck-out text read as plain is a sentence that says the opposite of
    /// what the author meant.
    pub strikeout: bool,
    /// Where in the source these characters came from, in bytes.
    ///
    /// `None` for the runs a layout adds rather than reads: a bullet, a
    /// quotation's bar, the border round a fenced block, the padding that
    /// holds a table's columns open. Those are the layout's own marks and
    /// point at nothing anybody wrote.
    ///
    /// What it is for is anything that has to survive being laid out
    /// again. Rows are laid out at a width, and every width gives
    /// different rows -- so a place in them that is remembered as a
    /// row and a column is a place that moves when the window does. Kept
    /// as where it came from, it does not: the source is the same
    /// whatever the window is doing.
    pub from: Option<std::ops::Range<usize>>,
}

impl Span {
    /// A run with no emphasis, which is every run but markdown's.
    #[must_use]
    pub fn new(text: impl Into<std::borrow::Cow<'static, str>>, ink: Ink) -> Self {
        Self {
            text: text.into(),
            ink,
            bold: false,
            italic: false,
            strikeout: false,
            from: None,
        }
    }

    /// A run that came from somewhere in the source.
    #[must_use]
    pub fn from_source(
        text: impl Into<std::borrow::Cow<'static, str>>,
        ink: Ink,
        from: std::ops::Range<usize>,
    ) -> Self {
        Self {
            from: Some(from),
            ..Self::new(text, ink)
        }
    }
}

/// One row of laid-out text, ready to draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// The runs, left to right.
    pub spans: Vec<Span>,
    /// Whether the row is a horizontal rule, which has no text of its own.
    pub rule: bool,
}

impl Row {
    /// A row of runs, with no rule.
    #[must_use]
    pub const fn of(spans: Vec<Span>) -> Self {
        Self { spans, rule: false }
    }
}
