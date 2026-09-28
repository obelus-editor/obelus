//! Colours.
//!
//! Every field has a reader in the renderer. A colour that nothing paints with
//! would compile, store, appear in the documentation, and change nothing when
//! set — which is the one kind of mistake here that announces itself to
//! nobody.

pub mod builtin;
pub mod written;

use obelus_text::kind::SyntaxKind;
use ratatui::style::Color;

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
    /// See [`SyntaxKind::Error`].
    ///
    /// Red, and its own field rather than the change colours' red: what a
    /// mark in the margin means and what a level in a log means are two
    /// different things, and a colour that says both says neither.
    pub error: Color,
    /// See [`SyntaxKind::Warning`].
    pub warning: Color,
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
            SyntaxKind::Error => self.error,
            SyntaxKind::Warning => self.warning,
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
    /// Behind the text.
    pub background: Color,
    /// The text.
    pub foreground: Color,
    /// Line numbers other than the cursor's.
    pub gutter: Color,
    /// The cursor's line number.
    pub gutter_current: Color,
    /// The track a scrollbar's thumb slides in.
    ///
    /// A block, a shade off the page, rather than a thin line: the bar is a
    /// surface with something sliding on it, and drawing the track as a rule
    /// made it a line -- which every other line on the screen then had to
    /// decide whether to join. That decision could only be made by reading
    /// the cells around it, and a cell holding a vertical stroke may as
    /// easily be a character of the file being read as a control: this
    /// repository's own golden grids are full of them, and a rule over one
    /// grew a tick everywhere the file had a bar under it.
    pub scrollbar_track: Color,
    /// The status line's text.
    pub status_foreground: Color,
    /// What the status row says in when a reader has to do something about
    /// it: a file that can no longer be read, a language server that has
    /// gone, a buffer with unsaved work in the list, an agent nearly out of
    /// room to remember this conversation in.
    ///
    /// The row's other three tones are shades of the page's own grey and
    /// say which of several things is in force. This one is not a shade --
    /// it is there to be found on a row nobody is reading.
    pub status_stale: Color,
    /// Behind the row the keys are on.
    ///
    /// Every list, page and card in Obelus: the rows of a picker, of the
    /// settings, of an agent's question, and the row of the transcript the
    /// reader is standing on. One colour, because they are one thing being
    /// said -- and it is said *only* where there is no caret to say it, a
    /// box being marked by the caret that sits in it.
    ///
    /// It marks where the keys are and nothing else. Whether the row can be
    /// used is said in the ink: a row that lost this for being unusable
    /// would leave the reader pressing a key, getting nothing, and unable
    /// to see which row refused.
    pub selected_row_background: Color,
    /// Behind something that reads as a surface rather than as prose: the
    /// cap a key is drawn in at the foot of a view.
    ///
    /// Not a panel's ground -- the card that lists every key, a hover, the
    /// completion list -- which is the page's own colour: a panel's frame
    /// already says it is not the file, and in a window the ground is the
    /// colour of the panel's glass, which everywhere else is the page's.
    ///
    /// Not the welcome screen, which has the same key-and-word shape and
    /// argues its way out of the cap: six caps in a block is six strips of
    /// colour on a screen that is otherwise a wordmark and some words, and
    /// there each key has a column to itself. A foot is one row among the
    /// reader's work, where nothing else gives the key an edge.
    ///
    /// The same colour as a selected row in the themes Obelus ships, and a
    /// field of its own because they are two different promises -- one says
    /// "the keys are here", the other "this is a thing to press" -- and a
    /// theme that wanted them apart could not say so through one name.
    pub raised_background: Color,
    /// Behind the characters of a row that the query matched.
    ///
    /// A background rather than a colour, because a row can be a line of
    /// code now: with the characters carrying the file's own syntax colours,
    /// a matched-character *colour* both fights them and can collide with
    /// one -- and the reader has to be able to see why a row is in the list
    /// whatever it is made of.
    pub picker_match_background: Color,
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
    /// The colour a change's marker is drawn in, wherever it is drawn.
    ///
    /// Here rather than in a view, because two of them draw one now: a file
    /// marks its changed lines in the margin, and a conversation draws the
    /// lines of a change an agent is asking to make. The same change said
    /// twice would be two colours for one fact.
    #[must_use]
    pub const fn marker_colour(&self, marker: obelus_text::marker::Marker) -> Color {
        use obelus_text::marker::Marker;
        match marker {
            Marker::Added => self.change_added,
            Marker::Modified => self.change_modified,
            Marker::Removed => self.change_removed,
        }
    }

    /// The tint behind a line of a change.
    #[must_use]
    pub const fn marker_background(&self, marker: obelus_text::marker::Marker) -> Color {
        use obelus_text::marker::Marker;
        match marker {
            Marker::Added => self.change_added_background,
            Marker::Modified => self.change_modified_background,
            Marker::Removed => self.change_removed_background,
        }
    }

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
    use obelus_text::kind::SyntaxKind;
    use ratatui::style::Color;

    use super::tint;

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
