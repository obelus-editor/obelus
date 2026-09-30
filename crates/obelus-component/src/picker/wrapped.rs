//! A list whose rows are read whole rather than scanned.
//!
//! A list of conversations is a list of sentences -- what an agent called
//! each one, and the note it was about -- and one row of a sentence is
//! mostly a row saying that there was more. So the rows of such a list
//! wrap: the label to three rows, the detail under it to two, a blank row
//! between one and the next, and a heading over each run of them.
//!
//! Laid out here rather than where it is drawn, because three things have
//! to agree about how tall a row is -- how tall the list is, which rows the
//! window shows, and what is drawn -- and two of them are not the drawing.
//!
//! Where a row's words stop is the same for every row: the column the
//! trailing words go in is as wide as the widest of them, so the words of
//! every row make one block with one right-hand edge and the trailing
//! column is read straight down.

use std::ops::Range;

use obelus_text::text_width;

use super::PickerItem;

/// The most rows a label is given.
///
/// Three, which is the cap a tool call's title gets in a transcript, and for
/// the same reason: somebody else's text may be as long as it likes and may
/// not push what it belongs to off the screen. An agent that names a
/// conversation after the whole of its first answer is the case in point.
pub const MOST_LABEL_ROWS: usize = 3;

/// The most rows a detail is given.
///
/// Fewer than the label: it is what the row is *about*, said under what the
/// row is, and a reader who wants all of it can open the thing it is about.
pub const MOST_DETAIL_ROWS: usize = 2;

/// Where the words start: a column to stand clear of the edge, and the two
/// the mark in front of a row takes.
///
/// Always kept, whether or not any row has a mark: a list whose words moved
/// two columns the moment a conversation was taken up elsewhere would
/// re-wrap every row under the reader.
pub const WORDS_AT: u16 = 3;

/// How far in a detail's words are from the label's: the detail's own mark,
/// and a blank after it.
pub const DETAIL_INDENT: u16 = 2;

/// The column the list's scrollbar takes.
///
/// `obelus_ui::editor::SCROLLBAR_WIDTH`, which this crate cannot see. The
/// drawing checks the two agree.
pub const BAR_COLUMNS: u16 = 1;

/// Between the words and the trailing column.
const GAP: u16 = 2;

/// Where a row's words go, across the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    /// How wide a label's rows are.
    pub label: u16,
    /// How wide a detail's rows are, which start [`DETAIL_INDENT`] further
    /// in.
    pub detail: u16,
}

impl Columns {
    /// The columns of a list `width` wide whose trailing words are at most
    /// `trailing` wide.
    #[must_use]
    pub fn of(width: u16, trailing: u16) -> Self {
        // The bar, and one blank before it that the trailing column ends on.
        let right = BAR_COLUMNS
            .saturating_add(1)
            .saturating_add(match trailing {
                0 => 0,
                _ => trailing.saturating_add(GAP),
            });
        let label = width.saturating_sub(WORDS_AT).saturating_sub(right).max(1);
        Self {
            label,
            detail: label.saturating_sub(DETAIL_INDENT).max(1),
        }
    }
}

/// A row's own words, wrapped: what does not depend on its neighbours.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Body {
    /// The label's rows, as byte ranges of the label.
    pub label: Vec<Range<usize>>,
    /// Whether the label goes on past the last of them.
    pub label_cut: bool,
    /// The detail's rows, as byte ranges of the detail.
    pub detail: Vec<Range<usize>>,
    /// Whether the detail goes on past the last of them.
    pub detail_cut: bool,
}

impl Body {
    /// Wraps one row's words into the columns it has.
    #[must_use]
    pub fn of(item: &PickerItem, columns: Columns) -> Self {
        let (label, label_cut) = capped(&item.label, columns.label, MOST_LABEL_ROWS);
        let (detail, detail_cut) = item
            .detail
            .as_deref()
            .filter(|detail| !detail.is_empty())
            .map_or((Vec::new(), false), |detail| {
                capped(detail, columns.detail, MOST_DETAIL_ROWS)
            });
        Self {
            label,
            label_cut,
            detail,
            detail_cut,
        }
    }

    /// How many rows the words take.
    #[must_use]
    pub fn rows(&self) -> u16 {
        u16::try_from(self.label.len().max(1) + self.detail.len()).unwrap_or(u16::MAX)
    }
}

/// What goes above a row: nothing, a blank row, or a heading.
///
/// Worked out from the row before it among the rows that match, which is
/// what makes a heading go with the last row of its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Above {
    /// Whether a heading starts here.
    pub headed: bool,
    /// Whether this is the first row of the list, which has nothing above
    /// it to stand apart from.
    pub first: bool,
}

impl Above {
    /// What goes above `item`, which follows `before`.
    #[must_use]
    pub fn of(item: &PickerItem, before: Option<&PickerItem>) -> Self {
        Self {
            headed: item.section.is_some()
                && before.is_none_or(|before| before.section != item.section),
            first: before.is_none(),
        }
    }

    /// How many rows that takes: a blank between this row and the one
    /// before, and a heading with a blank under it.
    #[must_use]
    pub fn rows(self) -> u16 {
        u16::from(!self.first) + if self.headed { 2 } else { 0 }
    }
}

/// How wide the widest trailing words are.
#[must_use]
pub fn trailing_width<'a>(items: impl IntoIterator<Item = &'a PickerItem>) -> u16 {
    items
        .into_iter()
        .filter_map(|item| item.trailing.as_deref())
        .map(text_width)
        .max()
        .map_or(0, |width| u16::try_from(width).unwrap_or(u16::MAX))
}

/// Wraps `text` into rows `width` wide, keeping at most `most` of them.
///
/// Where it keeps fewer than there are, the last one gives up a column for
/// the mark saying so: a sentence that stops at the edge of a row reads as a
/// sentence that stops there.
fn capped(text: &str, width: u16, most: usize) -> (Vec<Range<usize>>, bool) {
    let mut rows: Vec<Range<usize>> = obelus_text::wrapped_from(text, width)
        .into_iter()
        .map(|(_, range)| range)
        .collect();
    if rows.len() <= most {
        return (rows, false);
    }
    rows.truncate(most);
    if let Some(last) = rows.last_mut() {
        while last.end > last.start && text_width(&text[last.clone()]) + 1 > usize::from(width) {
            last.end = text[..last.end]
                .char_indices()
                .next_back()
                .map_or(last.start, |(at, _)| at);
        }
    }
    (rows, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(label: &str, detail: Option<&str>, section: Option<&str>) -> PickerItem {
        PickerItem {
            detail: detail.map(str::to_string),
            section: section.map(str::to_string),
            prose: true,
            ..super::super::tests::named(label)
        }
    }

    /// A label longer than three rows is three rows, and the last of them
    /// leaves room for the mark saying there was more.
    ///
    /// Broken by keeping every row `wrapped_from` gave: the label came back
    /// five rows long.
    #[test]
    fn a_long_label_is_three_rows_and_says_it_was_cut() {
        let long = "one two three four five six seven eight nine ten eleven twelve";
        let body = Body::of(
            &row(long, None, None),
            Columns {
                label: 10,
                detail: 8,
            },
        );
        assert_eq!(body.label.len(), MOST_LABEL_ROWS, "{:?}", body.label);
        assert!(body.label_cut, "a cut label did not say so");
        let last = body.label.last().cloned().unwrap_or_default();
        assert!(
            text_width(&long[last]) < 10,
            "the last row left no room for the ellipsis"
        );

        let short = Body::of(
            &row("one two", None, None),
            Columns {
                label: 10,
                detail: 8,
            },
        );
        assert_eq!(short.label.len(), 1);
        assert!(!short.label_cut, "a label that fit said it was cut");
    }

    /// A detail is two rows at most, at its own width, and a row with none
    /// is as tall as its label.
    #[test]
    fn a_detail_is_two_rows_under_the_label() {
        let columns = Columns {
            label: 12,
            detail: 10,
        };
        let body = Body::of(
            &row("label", Some("aaaa bbbb cccc dddd eeee ffff"), None),
            columns,
        );
        assert_eq!(body.detail.len(), MOST_DETAIL_ROWS);
        assert!(body.detail_cut);
        assert_eq!(body.rows(), 3);
        assert_eq!(Body::of(&row("label", None, None), columns).rows(), 1);
    }

    /// Chinese has no spaces and still wraps, and a row never starts on
    /// the punctuation that closes the one before it.
    #[test]
    fn chinese_wraps_between_characters_and_not_before_a_comma() {
        let label = "现在只能添加，不能修改。";
        let body = Body::of(
            &row(label, None, None),
            Columns {
                label: 12,
                detail: 10,
            },
        );
        assert!(body.label.len() > 1, "it did not wrap: {:?}", body.label);
        for range in &body.label {
            let first = label[range.clone()].chars().next();
            assert!(
                !matches!(first, Some('，' | '。')),
                "a row started on {first:?}"
            );
        }
    }

    /// A heading goes where a run starts, among the rows given -- which are
    /// the rows that match, so a run with none left has none.
    #[test]
    fn a_heading_goes_where_its_run_starts() {
        let today = row("a", None, Some("Today"));
        let also = row("b", None, Some("Today"));
        let earlier = row("c", None, Some("Earlier"));
        assert_eq!(
            Above::of(&today, None),
            Above {
                headed: true,
                first: true
            }
        );
        assert_eq!(Above::of(&today, None).rows(), 2);
        assert_eq!(Above::of(&also, Some(&today)).rows(), 1);
        assert_eq!(Above::of(&earlier, Some(&also)).rows(), 3);
        assert_eq!(Above::of(&row("d", None, None), None).rows(), 0);
    }

    /// The trailing column is as wide as its widest words, and comes out of
    /// every row's room.
    #[test]
    fn the_trailing_column_is_as_wide_as_its_widest() {
        let mut one = row("a", None, None);
        one.trailing = Some("just now".to_string());
        let mut two = row("b", None, None);
        two.trailing = Some("3 weeks ago".to_string());
        assert_eq!(trailing_width([&one, &two]), 11);
        let columns = Columns::of(60, 11);
        assert_eq!(columns.label, 60 - WORDS_AT - BAR_COLUMNS - 1 - 11 - GAP);
        assert_eq!(columns.detail, columns.label - DETAIL_INDENT);
    }
}
