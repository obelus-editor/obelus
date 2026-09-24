//! What a reader means to come back to, drawn.
//!
//! A title, a rule, the notes, and a foot saying what the keys do. The notes
//! are the only thing here with more than one shape: a row is a box, what it
//! says, and where it points, and a note with more behind it turns the mark
//! every other folding thing in Obelus turns.
//!
//! Where a note points is right-aligned in a column of its own, the way a
//! changed file's counts are: a place hung off the ragged end of a sentence
//! is a place a reader has to find again on every row.

use obelus_component::todo::{Row, Talked, TodoView as Notes};
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
pub fn hints(notes: &Notes, elsewhere: bool) -> Vec<Hint> {
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
        // Not where another Obelus has that conversation open: a
        // conversation is not a thing two of them may have at once, so the
        // key does nothing there and a foot offering it would be the page
        // promising something it will not do.
        Hint::common(alt(KeyCode::Char('a')), "Talk")
            .saying("Talk to an agent about this one")
            .when(on.is_some() && !elsewhere),
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
pub fn caret(area: Rect, notes: &Notes, elsewhere: bool) -> Option<ratatui::layout::Position> {
    let composer = notes.writing()?;
    let at = notes.writing_at()?;
    let hints = hints(notes, elsewhere);
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
pub fn place_at(area: Rect, notes: &Notes, elsewhere: bool, x: u16, y: u16) -> Option<(u16, u16)> {
    let composer = notes.writing()?;
    let at = notes.writing_at()?;
    let hints = hints(notes, elsewhere);
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

/// What a press in the list of notes landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    /// Either of the two marks saying somebody has talked about this
    /// note: what the conversation is doing, and that it is there.
    Talked,
    /// The arrow saying what hangs under it is folded away, or is not.
    Folds,
    /// The box saying whether it is done.
    Tick,
    /// The words of the note.
    Words,
}

/// Which row of the list a point on screen is on, and which of the row's
/// columns.
///
/// Only the first row of a note carries its mark and its box -- the rest
/// are the rest of what it says -- so a press lower down a note is a press
/// on its words wherever across the row it landed.
///
/// Read from the same four numbers the drawing spends: one to stand clear
/// of the edge, the two columns the conversation has, and the box; and the
/// note's own indent after them.
///
/// `None` for a point outside the list, or past the last row.
#[must_use]
pub fn row_at(
    area: Rect,
    notes: &Notes,
    elsewhere: bool,
    x: u16,
    y: u16,
) -> Option<(usize, Column)> {
    let list = list_region(area, &hints(notes, elsewhere));
    if y < list.y || y >= list.bottom() || x < list.x || x >= list.right() {
        return None;
    }
    let at = notes.window().top() + usize::from(y - list.y);
    let row = notes.rows().get(at)?;
    let step = row.depth * obelus_git::todo::INDENT;
    let column = match row.head {
        // A row that is not the head of its note has neither, whatever the
        // press landed on.
        false => Column::Words,
        // Both of the conversation's columns, because they are one thing
        // to press: what is happening in it and the mark saying it exists
        // are two halves of the same claim, and a press on either is a
        // reader asking to be taken there.
        true if x > list.x && x < list.x + 1 + WORKING + TALKED => Column::Talked,
        // The arrow, which is its own act: a press on it is a reader
        // asking for what is under the note, not for the note to be done.
        true if row.under.is_some()
            && x >= list.x + 1 + WORKING + TALKED + step
            && x < list.x + 1 + WORKING + TALKED + FOLDS + step =>
        {
            Column::Folds
        }
        true if x >= list.x + 1 + WORKING + TALKED + FOLDS + step && x < list.x + MARGIN + step => {
            Column::Tick
        }
        true => Column::Words,
    };
    Some((at, column))
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
/// Where a note's own words begin, counted from the edge of the list.
///
/// The half-cell that says the keys are here, the column that says what is
/// happening in the note's conversation, the column that says it has one,
/// the box, and the space after it. One answer, because the caret, the
/// pointer and the wrapping all ask it: the column below was added by hand
/// at the two places that draw and the pointer went on landing two cells
/// off, which is a selection that starts where the reader did not put it.
const MARGIN: u16 = 1 + WORKING + TALKED + FOLDS + 2;

/// The column a note's fold mark goes in.
///
/// After the note's own indent and in front of its box, which is where the
/// counts put theirs and where a tree puts one: a note further in has its
/// arrow further in, or the arrow stops saying which row it belongs to.
///
/// Always there, whether or not anything on the page folds -- the same
/// answer the two columns beside it give, for the reason [`TALKED`] gives:
/// a column that appears and disappears moves every note beside it, and
/// here it would move them the moment a reader put the first note under
/// another. The counts can spend theirs only when something folds because
/// nothing there is being typed into; a note's own words are wrapped to
/// what is left of the row, so a column that came and went would re-wrap
/// the page.
///
/// One cell, not two: an arrow is an ordinary glyph in every font, where
/// the icons beside it are private-use codepoints the font draws two cells
/// wide.
const FOLDS: u16 = 1;

/// The column that says what is happening in a note's conversation.
///
/// Two columns rather than one mark doing both jobs, which is how the list
/// of open documents says the same two things: a conversation is a picture
/// of an agent, and what that agent is *doing* turns beside it. Said in
/// one glyph -- an icon that meant "there is a conversation" and changed
/// colour when it wanted something -- the difference between a note being
/// worked on and a note wanting an answer was a shade of grey, which is
/// not something a reader glancing down a list sees.
///
/// Before [`TALKED`] rather than after, so the pair reads the way that
/// list reads it: what is happening, then what it is happening in.
const WORKING: u16 = 2;

/// The column that says whether a note has been talked about.
///
/// Always there, whether or not anything is in it, and in front of the box
/// rather than after the words. A mark that appears and disappears moves
/// every note beside it, so a reader glancing down the list sees the text
/// step in and out; and one hung off the end of the words lands in a
/// different column on every row, which is not a column anybody can read
/// down. Two cells, because that is what one of these glyphs measures.
const TALKED: u16 = 2;

/// What goes in the first of those two columns.
///
/// A mark that turns while an agent is working, drawn from the ticker
/// rather than from the note, because what it is saying is that time is
/// passing somewhere the reader is not looking; the glyph a waiting
/// question wears in the list of open documents where one is waiting; and
/// nothing at all otherwise, because an empty column is what says nothing
/// is happening.
fn working_mark(talked: Talked, phase: u32) -> Option<String> {
    match talked {
        Talked::Not | Talked::Yes => None,
        // Whose it is, where it is not this Obelus's. It goes in this
        // column and not the one beside it because this is the column
        // about what a conversation is doing -- and an empty one here
        // would say "nothing", which is a thing this Obelus is in no
        // position to say about somebody else's window.
        Talked::Elsewhere => Some(match obelus_icons::enabled() {
            true => obelus_icons::ui::ELSEWHERE.to_string(),
            false => "-".to_string(),
        }),
        Talked::Working => Some(crate::spinning(phase).to_string()),
        Talked::Waiting => Some(match obelus_icons::enabled() {
            true => obelus_icons::ui::READER.to_string(),
            false => "?".to_string(),
        }),
    }
}

/// What goes in the second of them.
///
/// The glyph a conversation wears everywhere else, or the plainest mark
/// there is where a terminal has no font for it -- and nothing at all for
/// a note nobody has talked about, because an empty column is what says
/// so. One glyph for every note that has one, whatever is going on in it:
/// this column says the conversation exists, and the column before it says
/// what it is doing.
fn talked_mark(talked: Talked) -> Option<String> {
    let said = match obelus_icons::enabled() {
        true => obelus_icons::ui::AGENT.to_string(),
        false => "*".to_string(),
    };
    match talked {
        Talked::Not => None,
        // A conversation open elsewhere is still a conversation, and this
        // column says only that there is one.
        Talked::Yes | Talked::Elsewhere | Talked::Working | Talked::Waiting => Some(said),
    }
}

/// The notes, over the whole editor region.
pub struct TodoUi<'a> {
    notes: &'a Notes,
    theme: &'a Theme,
    /// Which notes have a conversation, in the notes' own order.
    talked: Vec<Talked>,
    /// Whether the note the reader is on has its conversation open in
    /// another Obelus, which is the one key the foot holds back.
    elsewhere: bool,
    /// How far the ticker has got, for the mark that turns.
    ///
    /// The only thing here that changes without the reader doing
    /// something, and it is here for the same reason the list of open
    /// documents has it: a note whose agent is working says so from the
    /// list, because that is where a reader who is not watching the
    /// conversation would see it.
    phase: u32,
}

impl<'a> TodoUi<'a> {
    /// Borrows what the view needs, or nothing if the notes are not open.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            notes: app.notes()?,
            theme: app.theme(),
            talked: app.talked_about(),
            elsewhere: app.the_note_is_elsewhere(),
            phase: app.phase(),
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
        let hints = hints(self.notes, self.elsewhere);
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
        // than under it, which is how every other list in Obelus says it.
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
        // Whether anybody has talked about this note, in the column
        // before the box. On the note's own row only, like the box: a
        // line of a body is part of the note above it and is not
        // separately talked about.
        //
        // Before the box and not the words, so it sits in one column at
        // every depth -- the box steps in with the note and this does
        // not, because what it says is about the note rather than part of
        // it. The same two columns the list of open documents gives a
        // conversation, in the same order and the same colours: what is
        // happening in it, then the glyph saying what it is. A reader who
        // learnt them there should not have to learn them again here.
        if row.head {
            let talked = self.talked.get(row.note).copied().unwrap_or_default();
            // What it is doing, in whichever colour that is. An agent at
            // work recedes and a question does not: the second is the
            // reader's to do something about, so it is the one that
            // stands out -- which is the split that list draws.
            if let Some(mark) = working_mark(talked, self.phase) {
                let ink = match talked {
                    Talked::Waiting => self.theme.status_stale,
                    _ => self.theme.gutter,
                };
                crate::write(
                    cells,
                    area.x + 1,
                    y,
                    mark.as_str(),
                    Style::new().fg(ink).bg(background),
                );
            }
            // And that there is one at all, in the colour a mark beside a
            // name wears: the column before it carries what is going on,
            // so this one has nothing left to say with colour.
            if let Some(mark) = talked_mark(talked) {
                crate::write(
                    cells,
                    area.x + 1 + WORKING,
                    y,
                    mark.as_str(),
                    Style::new().fg(self.theme.gutter).bg(background),
                );
            }
        }
        let step = row.depth * obelus_git::todo::INDENT;
        if row.head {
            // What is under it, where anything is: the mark every other
            // folding thing in Obelus wears, because it is the same act.
            if let Some(open) = row.under {
                put(
                    cells,
                    area.x + 1 + WORKING + TALKED + step,
                    y,
                    crate::opens(open),
                    Style::new().fg(self.theme.gutter).bg(background),
                );
            }
            put(
                cells,
                area.x + 1 + WORKING + TALKED + FOLDS + step,
                y,
                crate::tick(row.done),
                style,
            );
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
        // Obelus marks a run of itself -- and the way the file marks its
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
