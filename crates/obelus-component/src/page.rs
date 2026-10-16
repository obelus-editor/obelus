//! A page of laid-out rows with a cursor in it: what walks the words, what
//! holds them, and what tab steps to.
//!
//! The conversation's transcript was the first page with a cursor, and all
//! of this was written for it. It is here rather than there because the
//! cursor is not about a conversation: a reader taking a copy of a sentence
//! has to be able to stand in it wherever it is laid out, and anything else
//! read as rows rather than as a file -- a pull request, an issue -- walks
//! the same way or walks in a way of its own that nobody asked for. Which
//! is the rule about one thing on screen being done by one piece of code,
//! and the reason it stops here: what a row *is* and what enter does on one
//! stay with whoever lays the page out. A transcript's rows are who said
//! what, with a box under them; another page's rows are something else.
//! Share the mechanism, not the meaning.
//!
//! **The cursor is a place on the screen; a selection is a place in the
//! words.** The cursor is where the reader is looking while they press
//! keys, and has to stand on rows that are in nobody's words -- a blank, a
//! heading over what it folds -- so it is a row and a character
//! ([`Place`]). A selection has to outlive the width it was made at,
//! because the reader makes one and then does something else -- reads on,
//! folds something, resizes the terminal -- before they copy it, so both
//! its ends are places in what the rows were laid out from ([`Spot`]).
//! What crosses between the two is [`spot_at`]: a shift-motion asks the row
//! under the cursor where in the words it is, and holds that.
//!
//! **The arrows walk every row drawn, and tab goes to what acts.** A row
//! that acts is one enter does something on, and which those are is the
//! page's to say ([`Line::acts`]) -- along with which rows are one thing
//! ([`Line::acting`]), so that a stop is the first row of a thing rather
//! than each row of it.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_row::Span;

use crate::window::Window;

/// One row of a page, wrapped to a width.
///
/// What the cursor needs to ask of a row and nothing more: its runs, which
/// text they were laid out from, how far in from the edge it is drawn, and
/// what enter does on it. Everything else a row carries is its page's.
pub trait Line: Sized {
    /// Which of the texts a page is made of a row was laid out from.
    ///
    /// Ordered the way the texts are drawn, which is what lets two places
    /// in them be compared: the ends of a selection are put in order by
    /// it, whatever the width and whatever is folded.
    type Text: Copy + Eq + Ord;

    /// The words, in runs, each carrying the bytes it was laid out from.
    fn spans(&self) -> &[Span];

    /// The text the runs were laid out from, or nothing for a row the page
    /// made up rather than laid out -- a blank, a heading -- which has no
    /// words in it to hold.
    fn laid_from(&self) -> Option<Self::Text>;

    /// How many cells in from the start of the words the row is drawn:
    /// what the page wraps it short by.
    fn indent(&self) -> u16;

    /// The block of code the row is part of, where it is one: a stop on a
    /// block is its first character of code, not the corner of its box.
    fn code(&self) -> Option<&obelus_row::Code>;

    /// Whether enter does something on this row, which is what makes it a
    /// stop for tab.
    fn acts(&self) -> bool;

    /// The rows a key pressed on `row` acts on, or none where it does
    /// nothing: the rows that are one thing with it.
    fn acting(rows: &[Self], row: usize) -> Option<Range<usize>>;

    /// Which of its characters the reader has hold of.
    fn held(&self) -> Option<&Range<usize>>;

    /// Says which of its characters the reader has hold of.
    fn hold(&mut self, held: Range<usize>);
}

/// A place in what a page was laid out from.
///
/// Not a row and a column. Rows are what a width makes of the words, and
/// they are made again whenever the width or the folding changes: a place
/// kept as one moves when the window is resized, and vanishes when
/// something above it is folded away. Bytes of the words do neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Spot<T> {
    /// Which text.
    pub of: T,
    /// How many bytes into it.
    pub at: usize,
}

/// Where the cursor is on a page.
///
/// A row and a character of it, which is where it is on the screen rather
/// than where it is in the words -- the opposite of how a selection is
/// kept, and on purpose: see the module's own doc.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    /// Which row, among the rows as they are drawn now.
    pub row: usize,
    /// How many characters into it.
    ///
    /// Its length means the place after its last character, where a caret
    /// at the end of a line sits.
    pub character: usize,
}

/// What the reader has hold of on a page, while they have hold of anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold<T> {
    /// The anchor and then the other end, in the order they were made
    /// rather than in the order they come: keeping the anchor is what lets
    /// a drag turn back on itself and shrink, and pass through where it
    /// started, without the two ends swapping meaning underneath it.
    ends: Option<(Spot<T>, Spot<T>)>,
}

impl<T> Default for Hold<T> {
    fn default() -> Self {
        Self { ends: None }
    }
}

impl<T: Copy + Ord> Hold<T> {
    /// Takes hold of a place, letting go of anything else.
    pub fn from(&mut self, spot: Spot<T>) {
        self.ends = Some((spot, spot));
    }

    /// Drags the far end of it to another place.
    ///
    /// The anchor stays where it was put: the two ends are compared when
    /// they are used, not stored in order.
    pub fn to(&mut self, spot: Spot<T>) {
        if let Some((_, other)) = self.ends.as_mut() {
            *other = spot;
        }
    }

    /// Lets go of whatever was held.
    pub fn let_go(&mut self) {
        self.ends = None;
    }

    /// Whether anything is held.
    #[must_use]
    pub const fn holding(&self) -> bool {
        self.ends.is_some()
    }

    /// Whether what is held is a press at `at` that never became a drag,
    /// which is what a click there leaves.
    ///
    /// At the place it was let go, as well as unmoved: a press on one link
    /// let go on another, with nothing reported between, is not a click on
    /// either.
    #[must_use]
    pub fn clicked(&self, at: Spot<T>) -> bool {
        self.ends.is_some_and(|(from, to)| from == to && from == at)
    }

    /// Lets go of a hold with nothing in it, which is what a press that
    /// never became a drag leaves.
    pub fn let_go_of_nothing(&mut self) {
        if self.ends.is_some_and(|(from, to)| from == to) {
            self.ends = None;
        }
    }

    /// Both ends, to move along with what they are in -- for a page whose
    /// texts are counted from a place something has just arrived in front
    /// of.
    pub fn ends_mut(&mut self) -> Option<(&mut Spot<T>, &mut Spot<T>)> {
        match &mut self.ends {
            Some((from, to)) => Some((from, to)),
            None => None,
        }
    }

    /// Marks the rows the reader has hold of.
    ///
    /// In two passes, because what a middle row holds cannot be worked out
    /// from the row alone. The ends of the selection are places in what was
    /// laid out; everything between them is held whole -- including the
    /// rows that came from nothing anybody wrote, a blank between two things
    /// and the bullet in front of a list item. Those are on the screen
    /// between the words that are held, so they are part of what the reader
    /// is pointing at, and leaving them out would copy something other than
    /// what is in front of them.
    pub fn mark<L: Line<Text = T>>(&self, rows: &mut [L]) {
        let Some((anchor, other)) = self.ends else {
            return;
        };
        let (first, last) = match anchor <= other {
            true => (anchor, other),
            false => (other, anchor),
        };
        let mut ends: Vec<(usize, Range<usize>)> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let Some(here) = row.laid_from() else {
                continue;
            };
            if here < first.of || here > last.of {
                continue;
            }
            let from = match here == first.of {
                true => first.at,
                false => 0,
            };
            let to = match here == last.of {
                true => last.at,
                false => usize::MAX,
            };
            if let Some(held) = row_held(row.spans(), from, to) {
                ends.push((index, held));
            }
        }
        let (Some((top, first_held)), Some((foot, last_held))) = (ends.first(), ends.last()) else {
            return;
        };
        let (top, foot) = (*top, *foot);
        let (first_held, last_held) = (first_held.clone(), last_held.clone());
        for (index, row) in rows.iter_mut().enumerate() {
            if index < top || index > foot {
                continue;
            }
            let whole = characters(row);
            row.hold(match (index == top, index == foot) {
                (true, true) => first_held.start..last_held.end,
                (true, false) => first_held.start..whole,
                (false, true) => 0..last_held.end,
                (false, false) => 0..whole,
            });
        }
    }
}

/// What is held, as it is drawn, off rows [`Hold::mark`] has marked.
///
/// Which is the whole of the promise: a reader takes a copy of what is on
/// the screen in front of them. So it is read off the rows -- a fold that
/// has put something away has put it out of this too, and the marks a
/// reading draws between held words come with them.
#[must_use]
pub fn held_text<L: Line>(rows: &[L]) -> Option<String> {
    let said: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let held = row.held()?;
            Some(text(row).chars().take(held.end).skip(held.start).collect())
        })
        .collect();
    (!said.is_empty()).then(|| said.join("\n"))
}

/// Which characters of a row fall between two places in what it was laid
/// out from.
///
/// The runs of a row carry the bytes they were laid out from, so this is
/// where those bytes fall inside the window and what that is in characters
/// of the row. The runs that carry nothing -- what the reading drew rather
/// than read -- are counted past rather than held: whether they are inside
/// is a question about the rows around them, and [`Hold::mark`] is where
/// that is answered.
#[must_use]
pub fn row_held(spans: &[Span], from: usize, to: usize) -> Option<Range<usize>> {
    let mut held: Option<Range<usize>> = None;
    let mut column = 0usize;
    for span in spans {
        let bytes = span.from.clone();
        for (offset, _) in span.text.char_indices() {
            if let Some(bytes) = bytes.as_ref() {
                let byte = bytes.start + offset;
                if byte >= from && byte < to {
                    let next = column..column + 1;
                    held = Some(match held {
                        Some(had) => had.start..next.end,
                        None => next,
                    });
                }
            }
            column += 1;
        }
    }
    held
}

/// What a row says, with nothing about how it is drawn.
///
/// Derived from the runs rather than kept beside them: a row that carried
/// both would be two answers to "what does this say", and the one that is
/// drawn is the runs.
#[must_use]
pub fn text<L: Line>(row: &L) -> String {
    row.spans().iter().map(|span| span.text.as_ref()).collect()
}

/// How many characters a row draws.
#[must_use]
pub fn characters<L: Line>(row: &L) -> usize {
    row.spans()
        .iter()
        .map(|span| span.text.chars().count())
        .sum()
}

/// The last place on a row a caret may stand, on a page whose rows are
/// `reading` cells wide.
///
/// After its last character, unless the row fills the width: then there is
/// no cell after it to draw a caret in -- the next one is the scrollbar's --
/// and the place is before the last character, the way a file's wrapped row
/// stops before the character that begins the next and leaves the rest to
/// the right arrow. The caret drawn one cell back instead was drawn through
/// the middle of a Chinese character ending the row, which is two cells and
/// the second of them was that one.
#[must_use]
pub fn last_stop<L: Line>(row: &L, reading: u16) -> usize {
    let text = text(row);
    let cells: usize = obelus_text::drawn_widths(&text)
        .map(|(_, wide)| usize::from(wide))
        .sum();
    if cells + usize::from(row.indent()) < usize::from(reading) {
        return characters(row);
    }
    // The start of the last cluster: the boundary before the end.
    obelus_text::boundaries(&text)
        .into_iter()
        .rev()
        .nth(1)
        .unwrap_or(0)
}

/// The place in the words that a row's nth character came from.
///
/// Characters of the row going in and a place in the words coming out,
/// which is the seam between the two halves of this. A row is what a width
/// made of the words; a place in them outlives the width, the folding and
/// the row itself. So everything that points at a page -- the pointer, the
/// cursor, both ends of a selection -- is kept as the second and worked in
/// the first.
///
/// Past the end of the row is the place just after its last character, so
/// that a cursor at the end of a row is somewhere rather than nowhere.
///
/// A run the reading drew rather than anybody wrote -- a bullet, a marker's
/// bar -- has no place in the words at all. Standing on one is standing
/// between the words around it, which is the same answer the pointer gives
/// for landing on one.
///
/// At the *start* of a row there is no word before, so it is the word
/// after, on this row. Handing back nothing there was the markdown
/// selection bug: only markdown lays out runs nobody wrote -- a plain
/// wrapping gives one run carrying the whole row -- so a bullet or a quote's
/// bar was the only place this arose. [`spot_near`] read the nothing as
/// "this row has no words at all", went looking down the page the way it
/// does for a blank or a heading, and landed past the row the cursor was
/// on: `shift+home` on a bullet held the rows *after* the cursor.
#[must_use]
pub fn spot_at<L: Line>(row: &L, characters: usize) -> Option<Spot<L::Text>> {
    let of = row.laid_from()?;
    let mut seen = 0usize;
    let mut after: Option<Spot<L::Text>> = None;
    // Whether a drawn run at the start of the row is still waiting for the
    // word that follows it.
    let mut waiting = false;
    for span in row.spans() {
        let bytes = span.from.clone();
        for (offset, character) in span.text.char_indices() {
            if let Some(bytes) = bytes.as_ref() {
                let at = bytes.start + offset;
                if seen == characters || waiting {
                    return Some(Spot { of, at });
                }
                after = Some(Spot {
                    of,
                    at: at + character.len_utf8(),
                });
            } else if seen == characters {
                match after {
                    Some(spot) => return Some(spot),
                    None => waiting = true,
                }
            }
            seen += 1;
        }
    }
    after
}

/// Where in a row a place in the words is, counted in characters.
///
/// The way back, for drawing a cursor kept as a place in the words at the
/// width it is being drawn at. Nothing where the place is not in this row
/// at all.
#[must_use]
pub fn characters_at<L: Line>(row: &L, spot: Spot<L::Text>) -> Option<usize> {
    if row.laid_from()? != spot.of {
        return None;
    }
    let mut seen = 0usize;
    let mut end: Option<usize> = None;
    for span in row.spans() {
        let bytes = span.from.clone();
        for (offset, character) in span.text.char_indices() {
            if let Some(bytes) = bytes.as_ref() {
                let at = bytes.start + offset;
                if at == spot.at {
                    return Some(seen);
                }
                // The end of the row is a place too: a cursor after the
                // last word of a wrapped line is on that line, not on the
                // next one.
                if at + character.len_utf8() == spot.at {
                    end = Some(seen + 1);
                }
            }
            seen += 1;
        }
    }
    end
}

/// Where in the words a place on the page is, for a selection a press
/// takes hold of.
///
/// A point in a row the reading drew rather than read -- a blank, a heading
/// over what it folds -- is the place just after the last word above it,
/// or where there is none, the place before the first word below. A drag
/// has to go somewhere while it crosses one, and the words either side of
/// it are what the reader is dragging between.
///
/// And the row after the last is the place after the last word of all, for
/// the same reason: it is where a reader starts a drag up through the end
/// of the page.
#[must_use]
pub fn spot_under<L: Line>(rows: &[L], place: Place) -> Option<Spot<L::Text>> {
    if let Some(spot) = rows
        .get(place.row)
        .and_then(|row| spot_at(row, place.character))
    {
        return Some(spot);
    }
    let at = place.row.min(rows.len());
    // Upwards first, where the cursor's nearest looks down, and nothing
    // turns on it: what lies between the end of the words above and the
    // start of those below is nobody's words, so a drag from either copies
    // the same. Under the last row there is only above.
    rows[..at]
        .iter()
        .rev()
        .find_map(|row| spot_at(row, characters(row)))
        .or_else(|| rows[at..].iter().find_map(|row| spot_at(row, 0)))
}

/// The place in the words the cursor is at, or the nearest there is.
///
/// Nearest, because a cursor can stand where there are no words: on a
/// blank, on the heading over something folded. A selection started from
/// one of those has to begin somewhere, and the somewhere a reader means is
/// the words they are next to.
#[must_use]
pub fn spot_near<L: Line>(place: Place, rows: &[L]) -> Option<Spot<L::Text>> {
    if let Some(spot) = spot_at(rows.get(place.row)?, place.character) {
        return Some(spot);
    }
    // Downwards first: the rows a page draws itself sit above what they are
    // about -- a heading over what it folds, a blank before what follows it
    // -- so the words they belong to are the ones under them.
    rows.iter()
        .skip(place.row)
        .find_map(|row| spot_at(row, 0))
        .or_else(|| {
            rows.iter()
                .take(place.row)
                .rev()
                .find_map(|row| spot_at(row, characters(row)))
        })
}

/// The next row worth standing on above or below `at`.
///
/// Above what `at` is part of, not above `at`: on the third row of a thing
/// the stop above is that thing's own first row, which is the one the key
/// was pressed in -- a key that looked as though it had done nothing.
///
/// And one stop for each thing, at the first of its rows: every row of a
/// block of code acts, and a key that stepped through them one at a time
/// would be the down arrow.
#[must_use]
pub fn next_stop<L: Line>(at: usize, up: bool, rows: &[L]) -> Option<usize> {
    let at = match up {
        true => L::acting(rows, at).map_or(at, |on| on.start),
        false => at,
    };
    let mut stops = rows
        .iter()
        .enumerate()
        .filter(|(at, row)| row.acts() && L::acting(rows, *at).is_some_and(|on| on.start == *at))
        .map(|(at, _)| at);
    match up {
        true => stops.take_while(|stop| *stop < at).last(),
        false => stops.find(|stop| *stop > at),
    }
}

/// Where the cursor stands on arriving at a stop.
///
/// The start of the row, except in a block of code, where it is the first
/// character of the code: the first row of a block is its box, and a caret
/// on the corner is standing in nothing anybody wrote.
#[must_use]
pub fn standing_at<L: Line>(stop: usize, rows: &[L]) -> Place {
    let at_the_start = Place {
        row: stop,
        character: 0,
    };
    let Some(on) = rows
        .get(stop)
        .filter(|row| row.code().is_some())
        .and(L::acting(rows, stop))
    else {
        return at_the_start;
    };
    on.into_iter()
        .find_map(|row| {
            let spot = spot_at(&rows[row], 0)?;
            Some(Place {
                row,
                character: characters_at(&rows[row], spot)?,
            })
        })
        .unwrap_or(at_the_start)
}

/// How much room a page has, which is what the keys are measured in.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    /// The rows on screen, which is what a page of the paging keys is.
    pub height: u16,
    /// The cells a row has, which is what the rows are wrapped to -- and so
    /// what decides which row is which.
    pub reading: u16,
}

/// Where a motion takes the cursor, or nothing where it runs out of page.
///
/// All of it in rows and characters, because that is what the keys mean: a
/// reader pressing the right arrow means the next character on the screen,
/// wherever in the words it happens to come from.
#[must_use]
pub fn walked<L: Line>(place: Place, key: KeyCode, rows: &[L], room: Room) -> Option<Place> {
    let count = rows.len();
    // As far along a row as a caret may go, which on a row that fills the
    // width is before its last character -- see `last_stop` -- except on
    // the last row, as on the last row of a line in a file: there is no
    // next row for the right arrow to go on to, and a selection could never
    // take that character.
    let characters = |row: usize| match rows.get(row) {
        Some(laid) if row + 1 < count => last_stop(laid, room.reading),
        Some(laid) => characters(laid),
        None => 0,
    };
    // Where a caret may stand on a row: between clusters, so that a picture
    // and its selector, or a letter and its accent, are one step -- the same
    // as in a file. See `Text::cluster_before`.
    let stops = |row: usize| {
        rows.get(row)
            .map_or_else(|| vec![0], |row| obelus_text::boundaries(&text(row)))
    };
    let settled = |place: Place| Place {
        character: stops(place.row)
            .into_iter()
            .take_while(|stop| *stop <= place.character)
            .last()
            .unwrap_or(0),
        ..place
    };
    let here = characters(place.row);
    // A click can stand beyond that, so that a drag from there takes the
    // last character; a key moves from where the caret is drawn, or the
    // first press of the left arrow moves it nowhere.
    let place = Place {
        character: place.character.min(here),
        ..place
    };
    Some(settled(match key {
        KeyCode::Right if place.character < here => Place {
            character: stops(place.row)
                .into_iter()
                .find(|stop| *stop > place.character)
                .unwrap_or(here),
            ..place
        },
        // Off the end of a row and on to the start of the next, which is
        // where the words carry on.
        KeyCode::Right if place.row + 1 < count => Place {
            row: place.row + 1,
            character: 0,
        },
        KeyCode::Left if place.character > 0 => Place {
            character: stops(place.row)
                .into_iter()
                .take_while(|stop| *stop < place.character)
                .last()
                .unwrap_or(0),
            ..place
        },
        KeyCode::Left if place.row > 0 => Place {
            row: place.row - 1,
            character: characters(place.row - 1),
        },
        // Up and down keep the column where they can, the way they do in
        // any text: a row too short for it takes the caret to its end
        // rather than refusing the key.
        KeyCode::Up if place.row > 0 => Place {
            row: place.row - 1,
            character: place.character.min(characters(place.row - 1)),
        },
        KeyCode::Down if place.row + 1 < count => Place {
            row: place.row + 1,
            character: place.character.min(characters(place.row + 1)),
        },
        KeyCode::Home => Place {
            character: 0,
            ..place
        },
        KeyCode::End => Place {
            character: here,
            ..place
        },
        KeyCode::PageUp => {
            let page = usize::from(room.height.max(1));
            let row = place.row.saturating_sub(page);
            Place {
                row,
                character: place.character.min(characters(row)),
            }
        }
        KeyCode::PageDown => {
            let page = usize::from(room.height.max(1));
            let row = (place.row + page).min(count.saturating_sub(1));
            Place {
                row,
                character: place.character.min(characters(row)),
            }
        }
        _ => return None,
    }))
}

/// Which rows of a page are on screen.
#[must_use]
pub fn in_view(window: &Window, count: usize, height: u16) -> Range<usize> {
    let top = window.top();
    top.min(count)..(top + usize::from(height)).min(count)
}

/// Puts a row on screen, moving the window as little as it takes.
pub fn show(window: &mut Window, at: usize, count: usize, height: u16) {
    let view = in_view(window, count, height);
    if at < view.start {
        window.scroll(-isize::try_from(view.start - at).unwrap_or(1));
    } else if at >= view.end {
        window.scroll(isize::try_from(at + 1 - view.end).unwrap_or(1));
    }
}

/// What a key did to the cursor on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walked {
    /// It moved, and the cursor is here now.
    To {
        /// Where it is.
        place: Place,
        /// Whether this press is the one that took hold of something:
        /// a page with something else on screen that can be held lets go of
        /// that, because there is one selection on a screen.
        took_hold: bool,
    },
    /// The down arrow without shift, off the last row: where a page with
    /// something under it goes on to that. What was held is let go of,
    /// because a caret moved on its own is a reader who has finished with
    /// it.
    OffTheFoot,
    /// A key that is the cursor's and that took it nowhere.
    Stayed,
}

/// Walks the page with the cursor, and holds what it walks over while
/// shift is down.
///
/// Every key that moves the cursor and nothing else: the arrows, the ends
/// and the paging keys, with shift holding what they pass over; control and
/// the ends going to the ends of the whole page; and tab and shift and tab,
/// to the next and the last row that acts. `None` for every other key --
/// enter, escape, typing -- which mean what the page says they mean.
///
/// `rows` are laid out once by the caller, and everything here asks them
/// rather than the page: laying a page out is real work, and a cursor that
/// moves on every arrow would pay it on every arrow.
pub fn walk<L: Line>(
    at: Place,
    code: KeyCode,
    modifiers: KeyModifiers,
    rows: &[L],
    room: Room,
    hold: &mut Hold<L::Text>,
    window: &mut Window,
) -> Option<Walked> {
    // What shift means on a motion is "and hold what I pass over", and it is
    // the only modifier that does: control reaches here too, for the two
    // ends, and asking only whether the key was bare made `ctrl+end` hold
    // the row it was on instead of going to the end.
    let holding = modifiers == KeyModifiers::SHIFT;
    match code {
        // The two ends of the page. They move the cursor and the view
        // follows, because the keys move the cursor -- a view sent to the
        // end with the cursor left behind is dragged back by the next arrow.
        KeyCode::Home | KeyCode::End if modifiers == KeyModifiers::CONTROL => {
            let last = rows.len().saturating_sub(1);
            let (row, character) = match code {
                KeyCode::Home => (0, 0),
                _ => (last, rows.get(last).map_or(0, characters)),
            };
            hold.let_go();
            match code {
                KeyCode::Home => window.home(),
                _ => window.end(),
            }
            Some(Walked::To {
                place: Place { row, character },
                took_hold: false,
            })
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown => {
            let Some(moved) = walked(at, code, rows, room) else {
                if code == KeyCode::Down && !holding {
                    hold.let_go();
                    return Some(Walked::OffTheFoot);
                }
                return Some(Walked::Stayed);
            };
            let mut took_hold = false;
            match holding {
                // A shift-motion with nothing held yet starts the selection
                // where the cursor was, not where it is going: what the
                // reader means to hold is what the key passed over.
                true => {
                    if !hold.holding()
                        && let Some(from) = spot_near(at, rows)
                    {
                        hold.from(from);
                        took_hold = true;
                    }
                    if let Some(to) = spot_near(moved, rows) {
                        hold.to(to);
                    }
                }
                // And a bare one lets go, the way it does in a box and in
                // the file: a caret moved on its own is a reader who has
                // finished with what they had.
                false => hold.let_go(),
            }
            show(window, moved.row, rows.len(), room.height);
            Some(Walked::To {
                place: moved,
                took_hold,
            })
        }
        // The next thing enter would open, which is what the arrows do not
        // land on: they walk the words, so getting to a heading in a long
        // run of them is its own key.
        KeyCode::Tab | KeyCode::BackTab => {
            let up = code == KeyCode::BackTab;
            // Taken either way. Nothing that way is a key that does
            // nothing, not a key that falls through to whatever else the
            // page has bound it to.
            let Some(stop) = next_stop(at.row, up, rows) else {
                return Some(Walked::Stayed);
            };
            hold.let_go();
            let standing = standing_at(stop, rows);
            show(window, standing.row, rows.len(), room.height);
            Some(Walked::To {
                place: standing,
                took_hold: false,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use obelus_row::{Ink, Span};

    use super::*;

    /// A row of a page that is not a conversation: which paragraph it was
    /// laid out from, and whether it is a heading enter opens.
    #[derive(Clone, Debug)]
    struct Plain {
        spans: Vec<Span>,
        from: Option<usize>,
        opens: bool,
        held: Option<Range<usize>>,
    }

    impl Line for Plain {
        type Text = usize;

        fn spans(&self) -> &[Span] {
            &self.spans
        }
        fn laid_from(&self) -> Option<usize> {
            self.from
        }
        fn indent(&self) -> u16 {
            0
        }
        fn code(&self) -> Option<&obelus_row::Code> {
            None
        }
        fn acts(&self) -> bool {
            self.opens
        }
        fn acting(rows: &[Self], row: usize) -> Option<Range<usize>> {
            rows.get(row)?.opens.then(|| row..row + 1)
        }
        fn held(&self) -> Option<&Range<usize>> {
            self.held.as_ref()
        }
        fn hold(&mut self, held: Range<usize>) {
            self.held = Some(held);
        }
    }

    /// A paragraph's words as one row, from byte `at` of it.
    fn words(text: &'static str, from: usize, at: usize) -> Plain {
        Plain {
            spans: vec![Span::from_source(text, Ink::Plain, at..at + text.len())],
            from: Some(from),
            opens: false,
            held: None,
        }
    }

    /// A heading the page made up, which is in nobody's words.
    fn heading(text: &'static str) -> Plain {
        Plain {
            spans: vec![Span::new(text, Ink::Heading(2))],
            from: None,
            opens: true,
            held: None,
        }
    }

    const ROOM: Room = Room {
        height: 10,
        reading: 40,
    };

    fn press(
        at: Place,
        code: KeyCode,
        modifiers: KeyModifiers,
        rows: &[Plain],
        hold: &mut Hold<usize>,
    ) -> Option<Walked> {
        walk(at, code, modifiers, rows, ROOM, hold, &mut Window::new())
    }

    /// A page made of rows that are not a conversation's walks the way the
    /// transcript does: tab steps over the words to the heading, shift
    /// holds what an arrow passes over, and what is held is the words on
    /// the screen, across the heading between them.
    ///
    /// Broken deliberately by making `next_stop` look for the next row
    /// rather than the next row that acts, and by starting a hold where the
    /// cursor goes rather than where it was: each fails here.
    #[test]
    fn a_page_of_its_own_rows_is_walked_and_held() {
        let mut rows = vec![
            words("first paragraph", 0, 0),
            words("goes on here", 0, 16),
            heading("Checks"),
            words("second", 1, 0),
        ];
        let mut hold = Hold::default();

        let tabbed = press(
            Place::default(),
            KeyCode::Tab,
            KeyModifiers::NONE,
            &rows,
            &mut hold,
        );
        assert_eq!(
            tabbed,
            Some(Walked::To {
                place: Place {
                    row: 2,
                    character: 0
                },
                took_hold: false,
            }),
            "tab did not go to the heading"
        );

        let from = Place {
            row: 1,
            character: 5,
        };
        let held = press(from, KeyCode::Down, KeyModifiers::SHIFT, &rows, &mut hold);
        assert!(
            matches!(
                held,
                Some(Walked::To {
                    took_hold: true,
                    ..
                })
            ),
            "{held:?}"
        );
        let Some(Walked::To { mut place, .. }) = held else {
            unreachable!()
        };
        for code in [KeyCode::Down, KeyCode::End] {
            if let Some(Walked::To { place: to, .. }) =
                press(place, code, KeyModifiers::SHIFT, &rows, &mut hold)
            {
                place = to;
            }
        }
        hold.mark(&mut rows);
        assert_eq!(
            held_text(&rows).as_deref(),
            Some("on here\nChecks\nsecond"),
            "what was held is not what the arrows passed over"
        );

        assert_eq!(
            press(
                Place {
                    row: 3,
                    character: 0
                },
                KeyCode::Down,
                KeyModifiers::NONE,
                &rows,
                &mut hold,
            ),
            Some(Walked::OffTheFoot),
        );
        assert!(!hold.holding(), "walking off the foot kept hold");
        assert_eq!(
            press(
                Place::default(),
                KeyCode::Enter,
                KeyModifiers::NONE,
                &rows,
                &mut hold
            ),
            None,
            "enter is the page's, not the cursor's"
        );
    }
}
