//! What a reader means to come back to, drawn.
//!
//! A title, a rule, the notes, and a foot saying what the keys do. The notes
//! are the only thing here with more than one shape: a row is a box, what it
//! says, and where it points, and a note with more behind it turns the mark
//! every other folding thing in obelus turns.
//!
//! Where a note points is right-aligned in a column of its own, the way a
//! changed file's counts are: a place hung off the ragged end of a sentence
//! is a place a reader has to find again on every row.

use obelus_component::todo::{Row, TodoView as Notes};
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    Hint, Marked, Screen, editor::SCROLLBAR_WIDTH, fill, foot_without_a_card, footed, put,
    write_marked,
};

/// Where the notes go, inside a region this size.
///
/// One answer, asked by the drawing and by the keys that move about the
/// list: a page is worth what is on screen, and two answers to how much that
/// is would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn list_region(area: Rect, hints: &[Hint]) -> Rect {
    // Nothing above it. A conversation has a header saying who is being
    // talked to, and this had the same claim on one the moment it became a
    // document the reader switches to rather than a page they had just
    // opened -- but there is one notes document and its name never varies,
    // so a header here would be a row of the reader's screen spent on a
    // constant. What says where they are is the status row, which carries
    // the mark and how much is left.
    footed(area, hints)
}

/// What the keys do here, and which of them do anything at the moment.
///
/// One list, all of it common: what the foot draws is the whole of what
/// this view says about itself. Which of them are on varies with what is
/// selected, so the row is what can be pressed right now rather than a
/// standing notice.
#[must_use]
pub fn hints(notes: &Notes) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = obelus_editing::keymap::KeyChord::new;
    let bare = |code| chord(code, KeyModifiers::NONE);
    let alt = |code| chord(code, KeyModifiers::ALT);
    let on = notes.selected_note();
    // All of them at the foot, and none of them held back. A card of every
    // key is what a dialog has -- a thing opened over what the reader was
    // doing, which owns the keyboard while it is up and has to be able to
    // say so. This is a document, and a document's keys are either worth a
    // row here or not worth saying at all.
    //
    // Nothing about typing, the arrows or `shift+enter`: this is a page
    // being written, and what a page being written does with a letter is
    // not news. `enter` is here for the opposite reason -- it is what a
    // text box does *not* do with it, because `shift+enter` is the line
    // break and `enter` starts another note.
    //
    // Nothing about copy, cut and paste either, which were on the card
    // because "can I paste in here?" is a question a dialog has to answer.
    // Here they are what they are everywhere else.
    vec![
        Hint::common(bare(KeyCode::Enter), "Another").saying("Start another note"),
        Hint::common(alt(KeyCode::Char(' ')), "Done")
            .saying("Done, or not")
            .when(on.is_some()),
        Hint::common(alt(KeyCode::Enter), "Go there")
            .saying("Go to what it is about")
            .when(notes.can_go()),
        Hint::common(alt(KeyCode::Char('a')), "Talk")
            .saying("Talk to an agent about this one")
            .when(on.is_some()),
        // Taking a whole note away is the one thing here a reader will go
        // looking for and not find, because backspace on its own is a
        // letter.
        Hint::common(alt(KeyCode::Backspace), "Drop")
            .saying("Take the whole note away")
            .when(on.is_some()),
        Hint::common(alt(KeyCode::Up), "Move")
            .saying("Move it up or down")
            .or(alt(KeyCode::Down))
            .when(notes.rows().len() > 1),
        // One key each, because they are offered separately: a note at the
        // top can only go in, and one as deep as it may go can only come
        // out. A single row for both would be on whenever either was, and
        // would be saying a key works when it does not.
        Hint::common(bare(KeyCode::Tab), "Under")
            .saying("Put it under the one above")
            .when(notes.can_shift(false)),
        Hint::common(chord(KeyCode::BackTab, KeyModifiers::SHIFT), "Out")
            .saying("Bring it back out a level")
            .when(notes.can_shift(true)),
    ]
}

/// How wide a note's own text is, in a region this size.
///
/// The margin in front of it and the bar down the side come off: what is
/// left is where the words go, which is what they wrap at.
#[must_use]
pub fn text_width_in(area: Rect) -> u16 {
    area.width.saturating_sub(MARGIN + SCROLLBAR_WIDTH).max(1)
}

/// Where the terminal should put its caret: in the note being written.
///
/// Worked out from the same rows the drawing lays out, so the caret is in
/// the row the reader can see their typing in rather than a row the view
/// happens to agree about.
///
/// Nowhere while the list of every key is up. That card is drawn in the
/// middle of the list, which is where this caret lives, so it sat blinking
/// on the card -- claiming a box on a page that has none. The picker and
/// the settings keep theirs through their own card because theirs is on
/// the status row, which no card covers.
#[must_use]
pub fn caret(area: Rect, notes: &Notes) -> Option<ratatui::layout::Position> {
    let composer = notes.writing()?;
    let at = notes.writing_at()?;
    let hints = hints(notes);
    let list = list_region(area, &hints);
    let window = notes.window();
    let row = at.checked_sub(window.top())?;
    let (line, cell) = composer.caret(notes.caret_width());
    let y = list.y + u16::try_from(row + line).ok()?;
    // Where that note's own words start, which is where its caret goes.
    let step = notes
        .rows()
        .get(at)
        .map_or(0, |row| row.depth * obelus_git::todo::INDENT);
    (y < list.bottom()).then(|| ratatui::layout::Position {
        x: (list.x + MARGIN + step + cell.get()).min(list.right().saturating_sub(1)),
        y,
    })
}

/// Which row and cell of the note being written a point on screen is.
///
/// The inverse of [`caret`], and beside it for the reason that one gives
/// itself: the row is drawn from these numbers and the caret is put from
/// these numbers, so where a click lands has to come from them too. Three
/// arithmetics for one geometry is two chances to disagree.
///
/// `None` for a point that is not in the note being written -- another
/// note's row, the foot, outside the page. A click there is not a click in
/// the box, and the box is the only thing here with a caret in it.
#[must_use]
pub fn place_at(area: Rect, notes: &Notes, x: u16, y: u16) -> Option<(u16, u16)> {
    let composer = notes.writing()?;
    let at = notes.writing_at()?;
    let hints = hints(notes);
    let list = list_region(area, &hints);
    if y < list.y || y >= list.bottom() || x < list.x + MARGIN || x >= list.right() {
        return None;
    }
    // Where the note's first row sits, in the rows the list is showing.
    let first = at.checked_sub(notes.window().top())?;
    let row = usize::from(y - list.y).checked_sub(first)?;
    // And no further than the note has rows: below it is another note.
    let rows = composer.rows(notes.caret_width()).len();
    (row < rows).then(|| {
        (
            u16::try_from(row).unwrap_or(u16::MAX),
            x - (list.x + MARGIN),
        )
    })
}

/// The glyph that leaves half a cell of ground showing.
///
/// The colour is the cell's *background* and this is drawn over the half
/// of it that should not show, in the page's own colour. Drawn the other
/// way round -- a half block inked in the selection's colour -- the mark
/// is 1.4:1 against the page and a reader has to look for it: a ground
/// carries that colour everywhere else because there are bright words on
/// it, and a lone stroke has nothing to help it.
///
/// The right half is masked, so what shows is against the edge of the
/// page rather than against the box beside it.
const HALF: char = '\u{2590}';

/// Which column a note's own text starts in: a blank, the box, and the blank
/// after it.
///
/// One number, because two things need it and they have to agree: the row is
/// drawn from here and the caret is put here. They did not, once, and the
/// caret sat one cell right of the letter it was about to put down -- which
/// is a caret that is lying about the only thing it says.
const MARGIN: u16 = 3;

/// The notes, over the whole editor region.
pub struct TodoUi<'a> {
    notes: &'a Notes,
    theme: &'a Theme,
}

impl<'a> TodoUi<'a> {
    /// Borrows what the view needs, or nothing if the notes are not open.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            notes: app.notes()?,
            theme: app.theme(),
        })
    }
}

impl Widget for TodoUi<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
        let hints = hints(self.notes);
        foot_without_a_card(cells, area, &hints, self.theme);

        let list = list_region(area, &hints);
        if list.height == 0 {
            return;
        }
        if self.notes.rows().is_empty() {
            crate::nothing(cells, list, "Nothing to come back to", self.theme);
            return;
        }

        let window = self.notes.window();
        // Which note the keys are on, rather than which row: a note is one
        // thing on this page -- its lines, and the place it points at --
        // and a mark beside one row of it would say the reader was holding
        // a line, which is not something this list has.
        let on = self.notes.rows().get(window.focus()).map(|row| row.note);
        for (at, row) in self
            .notes
            .rows()
            .iter()
            .enumerate()
            .skip(window.top())
            .take(usize::from(list.height))
        {
            let Ok(offset) = u16::try_from(at - window.top()) else {
                break;
            };
            self.row(
                cells,
                Rect {
                    y: list.y + offset,
                    height: 1,
                    ..list
                },
                row,
                Some(row.note) == on,
            );
        }

        // Only where there is somewhere to scroll: a track with no thumb on
        // it is a control that does not work.
        if window.scrollable(list.height) {
            crate::scrollbar(
                cells,
                list,
                window.top(),
                self.notes.rows().len(),
                self.theme,
            );
        }
    }
}

impl TodoUi<'_> {
    /// One row: the mark, the box, what it says, and where it points.
    fn row(&self, cells: &mut CellBuffer, area: Rect, row: &Row, selected: bool) {
        let background = self.theme.background;
        fill(
            cells,
            Rect {
                width: area.width.saturating_sub(SCROLLBAR_WIDTH),
                ..area
            },
            Style::new().bg(background),
        );
        // "The keys are here", said down the edge of the whole note rather
        // than under it, which is how every other list in obelus says it.
        //
        // This is the one list whose rows the reader also selects text
        // *inside*, and a run of selection lying on a selected row's
        // background was two grounds arguing over the same cells: the
        // reader could not see where what they were holding began or
        // ended, which is the only thing a selection has to say. The mark
        // is outside the text entirely now, in the column nothing else
        // uses at any depth, so no cell carries two claims at once.
        //
        // In the selection's own colour, because the page has one idea of
        // what is picked out and should say it once. A third colour here
        // would be a third thing to learn, and the obvious alternative --
        // the colour the editor's gutter marks the current line in -- is
        // exactly that. A filled cell rather than a `\u{258c}` stroke in
        // that colour, which is what this first was: a ground carries the
        // selection's colour everywhere else because there are bright
        // words on it, and the same colour drawn as a lone stroke on the
        // page's own background is 1.4:1 against it -- a mark a reader has
        // to go looking for.
        //
        // Half a cell of it, and the half at the edge: a whole column of
        // colour is heavier than the claim, and against the edge it does
        // not touch the box beside it -- two marks with no gap read as one
        // wide one.
        if selected {
            put(
                cells,
                area.x,
                area.y,
                HALF,
                Style::new()
                    .fg(background)
                    .bg(self.theme.selection_background),
            );
        }
        // A note that is done is said in the ink, never by taking it away: a
        // list of what is done is how a reader tells "I decided against it"
        // from "I never got to it".
        let style = Style::new()
            .fg(match row.done {
                true => self.theme.gutter,
                false => self.theme.foreground,
            })
            .bg(background);
        let y = area.y;

        // The box, on the note's own row only: a line of a body is part of
        // the note above it and is not separately done. Its column is kept
        // on the rows below, so a note's lines line up under its first.
        //
        // Indented with the words rather than left in one column down the
        // edge: the box is the note's own mark, and a column of them all
        // hard left with the text stepping away from them reads as one flat
        // list with ragged words.
        let step = row.depth * obelus_git::todo::INDENT;
        if row.head {
            put(cells, area.x + 1 + step, y, crate::tick(row.done), style);
        }
        let x = area.x + MARGIN + step;

        // A row that is where the note points is dim: it is a fact about
        // the note rather than a word of it, and it is not the reader's to
        // change.
        let ink = match row.place || row.done {
            true => self.theme.gutter,
            false => self.theme.foreground,
        };
        // What the reader has hold of, marked the way every other row in
        // obelus marks a run of itself -- and the way the file marks its
        // own selection, which is the colour a reader has learnt means
        // "this is what you are holding".
        let marked = match row.held.clone() {
            Some(held) => Marked::run(held, self.theme.selection_background),
            None => Marked::plain(),
        };
        write_marked(
            cells,
            Rect {
                width: area.width.saturating_sub(SCROLLBAR_WIDTH),
                ..area
            },
            x,
            y,
            &crate::truncate_from_right(
                &row.said,
                usize::from(
                    area.width
                        .saturating_sub(MARGIN + SCROLLBAR_WIDTH + step)
                        .max(1),
                ),
            ),
            Style::new().fg(ink).bg(background),
            &marked,
        );
    }
}
