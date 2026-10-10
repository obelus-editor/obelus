//! A row's text, with what matched marked and what the file colours
//! coloured.

use super::*;

/// Which characters of a row are marked out.
///
/// Two shapes because the questions have two shapes: a fuzzy match lands on
/// scattered characters, while a substring match -- or a selection, which is
/// the same shape and gets the same treatment -- is one run. Every row in
/// Obelus marks them the same way, which is what this is for: a row in a
/// narrowed list has to say why it is in it, and a row of a note has to say
/// what the reader has hold of.
#[derive(Clone, Copy, Debug, Default)]
pub enum Matched<'a> {
    /// Nothing was typed, or nothing in this text matched it.
    #[default]
    Nothing,
    /// These characters, counted from the start of the whole text.
    Indices(&'a [u32]),
    /// This run of them, as `first..end`.
    Run(usize, usize),
}

impl Matched<'_> {
    /// Whether the character at this index matched.
    fn covers(self, index: u32) -> bool {
        match self {
            Self::Nothing => false,
            Self::Indices(indices) => indices.binary_search(&index).is_ok(),
            Self::Run(first, end) => {
                usize::try_from(index).is_ok_and(|index| index >= first && index < end)
            }
        }
    }
}

/// How a row's text is to be drawn, beyond where and in what colour.
///
/// One type for the three things that happen to a row's characters -- the
/// match marked, the row's own syntax underneath, a truncated head skipped
/// -- so that every list does all three the same way, and a list that wants
/// none of them says so with [`Marked::plain`].
#[derive(Clone, Copy, Debug)]
pub struct Marked<'a> {
    /// Which characters the query matched.
    pub matched: Matched<'a>,
    /// What to mark them with. A background, so it survives whatever colour
    /// the character already has: a row that is a line of code carries the
    /// file's own colours, and a match painted over them would be one more
    /// hue among seven rather than an answer to "why is this row here".
    pub mark: Color,
    /// The row's own colours, when the row is a line of a file.
    pub syntax: Option<(&'a [Colouring], &'a Theme)>,
    /// How many leading characters are not drawn, for text whose head has
    /// been truncated away. The matched positions are still counted from
    /// the start of the whole text, so a match that fell in the dropped
    /// part simply has no character left to colour.
    pub skip: usize,
}

impl Marked<'_> {
    /// Text with nothing to say about it.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            matched: Matched::Nothing,
            mark: Color::Reset,
            syntax: None,
            skip: 0,
        }
    }

    /// Text with its matched characters marked.
    #[must_use]
    pub fn matched(matched: Matched<'_>, mark: Color) -> Marked<'_> {
        Marked {
            matched,
            mark,
            syntax: None,
            skip: 0,
        }
    }

    /// Text with one run of it marked out.
    ///
    /// For a selection, which is not a match and is drawn like one: a
    /// background over whatever colour the characters already carry.
    #[must_use]
    pub const fn run(held: Range<usize>, mark: Color) -> Self {
        Self {
            matched: Matched::Run(held.start, held.end),
            mark,
            syntax: None,
            skip: 0,
        }
    }
}

/// Writes a row's text, marking a run of it and colouring what the file
/// colours, and returns the column after it.
///
/// Clipped at the right edge of `area` rather than the screen: a row is
/// inside a list, and text that ran past the list's edge would be drawn over
/// whatever the list is on top of.
pub fn write_marked(
    cells: &mut CellBuffer,
    area: Rect,
    x: u16,
    y: u16,
    contents: &str,
    style: Style,
    marked: &Marked<'_>,
) -> u16 {
    let mut column = x;
    // A cluster the skip cut into is not drawn: its first character has
    // gone, and what is left of it says how nothing is drawn.
    for cluster in obelus_text::clusters(contents).skip_while(|cluster| cluster.first < marked.skip)
    {
        if column >= area.right() {
            break;
        }
        // Coloured by its first character, and marked where any of it
        // matched: it is one cell, and a match on the selector of a heart
        // is a match on the heart.
        let index = u32::try_from(cluster.first).unwrap_or(u32::MAX);
        let matched = (index..)
            .take(cluster.text.chars().count())
            .any(|index| marked.matched.covers(index));
        // The row's own colours first, then the matched characters over the
        // top: a reader scanning the list is looking for why the row is
        // there, and only then at what it says.
        let style = match marked.syntax {
            Some((runs, theme)) => match u16::try_from(index).ok().and_then(|at| {
                runs.iter()
                    .find(|(from, to, _)| at >= *from && at < *to)
                    .map(|(_, _, kind)| *kind)
            }) {
                Some(kind) => style.fg(theme.syntax.colour(kind)),
                None => style,
            },
            None => style,
        };
        let style = match matched {
            true => style.bg(marked.mark),
            false => style,
        };
        column = column.saturating_add(put_cluster(cells, column, y, cluster.text, style));
    }
    column
}
