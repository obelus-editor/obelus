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

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::todo::{Row, TodoView as Notes},
    theme::Theme,
    ui::{Hint, Marked, editor::SCROLLBAR_WIDTH, fill, foot, footed, put, write_marked},
};

/// The box in front of a note, ticked and not.
///
/// The plain ones where there is no Nerd Font. Everything else on this page
/// survives losing the glyphs -- the words are words -- but whether a note
/// is done is said *only* here, so it has to be said in something every
/// terminal can draw.
fn box_of(done: bool) -> char {
    match (crate::icons::enabled(), done) {
        (true, false) => crate::icons::ui::TODO,
        (true, true) => crate::icons::ui::TODO_DONE,
        (false, false) => '\u{25a1}',
        (false, true) => '\u{2611}',
    }
}

/// Where the notes go, inside a region this size.
///
/// One answer, asked by the drawing and by the keys that move about the
/// list: a page is worth what is on screen, and two answers to how much that
/// is would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn list_region(area: Rect, hints: &[Hint]) -> Rect {
    // Nothing above it. There are no tabs here and nothing to filter by, so
    // a title row would be one row of the reader's screen spent saying what
    // they just asked for.
    footed(area, hints)
}

/// What the keys do here, and which of them do anything at the moment.
///
/// One list, read two ways: the foot draws the common ones that can be
/// pressed, and the card draws all of them with the rest greyed. A view that
/// kept two lists would be a view whose card and foot could disagree about
/// what it answers to.
#[must_use]
pub fn hints(notes: &Notes) -> Vec<Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = crate::keymap::KeyChord::new;
    let bare = |code| chord(code, KeyModifiers::NONE);
    let alt = |code| chord(code, KeyModifiers::ALT);
    let control = |letter| chord(KeyCode::Char(letter), KeyModifiers::CONTROL);
    let on = notes.selected_note();
    vec![
        // Nothing about typing, the arrows or `shift+enter`: this is a page
        // being written, and what a page being written does with a letter is
        // not news. What is worth a row is what it does with a *note*.
        Hint::common(bare(KeyCode::Enter), "another").saying("start another note"),
        Hint::common(alt(KeyCode::Char(' ')), "done")
            .saying("done, or not")
            .when(on.is_some()),
        Hint::common(alt(KeyCode::Enter), "go there")
            .saying("go to what it is about")
            .when(notes.can_go()),
        Hint::common(alt(KeyCode::Char('a')), "talk")
            .saying("talk to an agent about this one")
            .when(on.is_some()),
        // At the foot rather than on the card alone: taking a whole note
        // away is the one thing here a reader will go looking for and not
        // find, because backspace on its own is a letter.
        Hint::common(alt(KeyCode::Backspace), "drop")
            .saying("take the whole note away")
            .when(on.is_some()),
        Hint::common(bare(KeyCode::Esc), "leave").saying("leave, keeping what is written"),
        Hint::rare(alt(KeyCode::Up), "move")
            .saying("move it up or down")
            .or(alt(KeyCode::Down))
            .when(notes.rows().len() > 1),
        // One key each, because they are offered separately: a note at the
        // top can only go in, and one as deep as it may go can only come
        // out. A single row for both would be on whenever either was, and
        // would be saying a key works when it does not.
        Hint::rare(bare(KeyCode::Tab), "under")
            .saying("put it under the one above")
            .when(notes.can_shift(false)),
        Hint::rare(chord(KeyCode::BackTab, KeyModifiers::SHIFT), "out")
            .saying("bring it back out a level")
            .when(notes.can_shift(true)),
        // The four a reader arrives already holding, on the card rather
        // than at the foot: they are what these keys are everywhere else,
        // so the foot would spend four of its columns saying nothing. On
        // the card, though, because "can I paste in here?" is a question a
        // dialog has to have an answer to -- and because what copy and cut
        // take when nothing is held is this view's own rule.
        Hint::rare(control('c'), "copy")
            .saying("copy what is held, or the whole note")
            .when(on.is_some()),
        Hint::rare(control('x'), "cut")
            .saying("cut what is held, or the whole note")
            .when(on.is_some()),
        Hint::rare(control('v'), "paste").saying("paste what was copied"),
        Hint::rare(control('a'), "all")
            .saying("take hold of the whole note")
            .when(on.is_some()),
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
        .map_or(0, |row| row.depth * crate::component::todo::INDENT);
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
    pub fn new(app: &'a App) -> Option<Self> {
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
        foot(cells, area, &hints, self.theme);

        let list = list_region(area, &hints);
        if list.height == 0 {
            return;
        }
        if self.notes.rows().is_empty() {
            crate::ui::nothing(cells, list, "nothing to come back to", self.theme);
            self.keys(cells, area, &hints);
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
            crate::ui::scrollbar(
                cells,
                list,
                window.top(),
                self.notes.rows().len(),
                self.theme,
            );
        }

        self.keys(cells, area, &hints);
    }
}

impl TodoUi<'_> {
    /// Every key this view answers to, where the reader asked for them.
    ///
    /// Over everything, because it is what they asked for and the list is
    /// what they asked about.
    fn keys(&self, cells: &mut CellBuffer, area: Rect, hints: &[Hint]) {
        if self.notes.showing_keys() {
            // Above the foot: the foot says how to close this, and a card
            // that covered it would be a card with no way out on screen.
            crate::ui::keys_card(cells, footed(area, hints), hints, self.theme);
        }
    }

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
        let step = row.depth * crate::component::todo::INDENT;
        if row.head {
            put(cells, area.x + 1 + step, y, box_of(row.done), style);
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
            &crate::ui::truncate_from_right(
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
