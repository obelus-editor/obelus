//! The gutter and the text.
//!
//! A hand-written widget rather than a `Paragraph`: the text needs per-cell
//! styling, a tab has to expand to the next tab stop, a wide glyph has to
//! occupy two cells, and a glyph straddling either edge of a horizontally
//! scrolled viewport has to render as blanks. None of that survives going
//! through a widget that takes styled spans.
//!
//! What the bar measures is what is shown. The scrollbar and the change map
//! are pictures of the document at the height of the screen, and a closed run
//! makes the document shorter: drawn from the file's own line numbers they say
//! the reader is at the top of something long while the whole of it is in front
//! of them. Both count in lines that are shown, which is why `Folds` can say
//! how many are hidden above a line and how many altogether. An opened hunk
//! closes when a fold hides the line it hangs above, for the same reason
//! `refresh_changes` closes it when the diff is replaced: its rows belong to
//! something that is no longer on screen, and a caret in them is a caret nobody
//! can see.
//!
//! The caret can be in the block; the cursor never is. `Buffer::block_above`
//! is an opened hunk's lines *as a `Text`*, and `in_block` is a `Cursor` in one
//! of them.
//! A text, so those lines get everything the file's get from the same code:
//! they wrap at the same width, their tabs reach the same stops, a wide glyph
//! takes two cells, the caret moves by visual rows, a selection in them is a
//! `Span`, copying is `text_in`, and the rows are drawn by the writer every
//! other row goes through. The alternative was a second, smaller set of all of
//! that -- which is a second set of bugs, and was one: a line wider than the
//! screen was cut with the caret walking off the edge of it.
//!
//! The *cursor* stays on the line the block is anchored to, so everything that
//! asks the file about "here" -- a language server, a jump, the next change,
//! the margin -- goes on being answered from a line the file has. The status
//! row says `-4:7` while the caret is in there, because that place has no line
//! number in this file and a number without the minus would name one it is
//! nowhere near. The anchor of a selection belongs to whichever of the two the
//! caret is in, and `clear_selection` reaches both. A page that lands on one of
//! those rows puts the caret there, which is how the paging keys walk a block
//! of any size; anything that puts the cursor somewhere outright
//! (`place_cursor`) brings it back, as does closing the hunk -- which the key
//! that opened it does from wherever the reader has walked to, because "the
//! hunk at the cursor" is not the hunk in front of them once they have walked
//! into it.
//!
//! A bar is a block, so no rule has to meet it. The track is a block a
//! shade off the page and the thumb the same block brighter: a surface with
//! something sliding on it, which is what a scrollbar is. Drawn as a *line* --
//! a column of ┃ with the thumb picked out -- it was one more line on a screen
//! of lines, and every rule that crossed it then had to decide whether to join.
//!
//! That decision cost more than it was worth. `rule` and `scrollbar` made it by
//! reading the grid back: a cell holding ┃ or █ beside a rule meant a bar, and
//! the rule turned into a corner. But a cell is a cell. A file's own text
//! answers that question exactly as a control does, and this repository is full
//! of files that do -- every golden grid under `tests/fixtures` is drawn in box
//! characters. The note that used to be here called that a cosmetic slip in one
//! cell; what it looked like on screen was a row of ┬ across the whole width,
//! under a list previewing a file of grids, and the reader who found it was
//! looking at an obelus previewing obelus's own fixtures. A markdown table has
//! the same glyphs and would have done the same thing.
//!
//! So the shape of the thing says what it is, and nothing reads the grid back.
//! A rule runs its whole width in one glyph; a block column meets it and needs
//! nothing from it. The one row of the block that the rule takes is the
//! boundary between two bars -- a list's and its preview's -- which are two
//! controls over two different things, and reading as two is right.
//!
//! A column a file might need is reserved for the whole file, not for the
//! lines that need it. The change margin is there whenever git can answer
//! about the file, empty rows included; the fold column is there whenever the
//! file has anything to fold, whether or not anything is folded. A column that
//! arrived when the reader pressed a key would rewrap the text under them as it
//! came. What goes *in* the column is every run, open or folded: a reader
//! cannot press a key on a line that never said it had anything behind it, so
//! the mark is how folding is discovered at all. The one turned down is the
//! quieter of the two, which is the right way round -- most runs are open most
//! of the time, and the eye should be caught by the lines that are hiding
//! something. The column is decided when the file is read and stays decided,
//! so nothing a reader does to a fold ever moves the text sideways under them.
//!
//! A folded run also says so on the row it folded into: the view's mark after
//! the line's own text, and then whatever is left of the run's last line. That
//! is the whole rule -- no test for what a closing mark looks like, no table
//! per language -- because the run was *built* to stop before the bracket. A
//! run that closes with nothing leaves the mark on its own.
//!
//! Two colours, because they are two different things. The mark is obelus's own
//! and is drawn the way its notes are; the closing text *is* the file's and
//! keeps the colour the highlighting gives it where it really lives. A brace
//! that changed colour on its way up the screen would read as something else.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::{
    app::App,
    buffer::Buffer,
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    git::{self, Changes},
    marker::Marker,
    syntax::{brackets, highlight::Highlights},
    text::WrapRow,
    theme::Theme,
    ui::{fill, put},
};

/// The narrowest the gutter is allowed to be.
///
/// Enough for four digits and the separating space. A short file would
/// otherwise get a two-column gutter, which is correct and looks starved, and
/// the width would then change from file to file.
const MINIMUM_GUTTER_WIDTH: u16 = 5;

/// The column the change markers take, on the left.
///
/// Reserved whenever obelus has an answer about the file -- that is, when it
/// is in a repository -- and not otherwise. A column that came and went as
/// the file was *edited* would rewrap the text under the reader; one that
/// depends only on which file is open does not.
pub const MARGIN_WIDTH: u16 = 1;

/// The column the fold marks take, between the gutter and the text.
///
/// Reserved whenever the file has anything to fold, not when something is
/// folded: a column that appeared the moment a reader pressed the key would
/// rewrap the text under them as it arrived. The same rule the change
/// margin follows, for the same reason.
///
/// To the right of the numbers because it says what the *line* is -- one
/// that has more behind it -- and to the left of the text because it is not
/// part of what the file says. Numbers, then a mark about the line, then
/// the line.
pub const FOLD_WIDTH: u16 = 1;

/// What a folded line carries in that column, and what an open one does.
///
/// [`crate::ui::opens`], because folding a run of lines, a run of tool
/// calls and a commit's files are the same act. Both states are marked,
/// because a reader cannot press a key on a line that never said it had
/// anything behind it -- and the one turned down is the quieter of the two,
/// which is the right way round: most runs are open most of the time.
///
/// What a folded run leaves on the line it folds into.
///
/// Drawn after the line's own text, dim, because it is not what the file
/// says: the file says `fn shout(&self) -> String {` and the view is adding
/// that the rest is here. The closing line comes along when it is short
/// enough to be a closing mark -- `}`, `);`, `end` -- because then the row
/// reads as a whole construct, `{ … }`, spaced the way the file spaces it.
///
/// A run with no such line -- a paragraph of comments, a block whose last
/// line is code in its own right -- takes the mark against its text with no
/// gap, `and what is left to see…`, because there it stands for the rest of
/// a sentence rather than for the inside of something, and a sentence
/// trails off where it stops. The last line stays hidden either way: the
/// row is a summary, not two lines pretending to be one.
const ELIDED: &str = "\u{2026}";

/// Whether git has anything to say about this file.
///
/// Something to say rather than merely being asked: a file in a repository
/// that nobody has touched would otherwise spend a column on an answer of
/// "nothing", which is every file in every clean tree. What that costs is
/// a cell of sideways shift when a file does change under the reader --
/// and with wrapping on, a rewrap -- and it is paid where there is news
/// worth the column.
#[must_use]
pub fn changed(changes: Option<&Changes>) -> bool {
    changes.is_some_and(|changes| !changes.hunks().is_empty())
}

/// How many columns the map takes, just inside the scrollbar.
///
/// git's news and nothing else, and only where there is some.
///
/// What a server says is wrong is deliberately not here, and it was tried
/// three ways first: halved into this cell, given a column of its own, and
/// laid under the changes as a wash. The first two are what the trying
/// taught. A cell has one foreground and one background, and both kinds of
/// news say what *kind* they are by hue -- git's deleted is the same red as
/// an error, its modified the same yellow as a warning -- so halved they
/// merge into a solid block exactly where they matter most; and a column
/// each reads as one broken double bar rather than as two answers.
///
/// But the reason it is not here is not that it was awkward. A map is for
/// something with a shape: a twenty-line rewrite is a longer bar than a
/// one-line fix, and nothing else on screen says that. A problem has no
/// shape -- it is a point -- and where the other ones are is already
/// answered by the count on the status row, by `go-to-next-problem`, which
/// takes the reader there rather than telling them it exists, and by the
/// list, which names every one of them. A speckle in one column adds
/// nothing to those three.
#[must_use]
pub fn map_width(changes: Option<&Changes>) -> u16 {
    u16::from(changed(changes))
}

/// How many columns come before the text: the change margin, the gutter,
/// then the fold marks.
///
/// One function, because two of them disagreed. The caret's own position
/// used the gutter alone while the text is drawn after the margin as well,
/// so on every file in a repository the caret sat one cell to the left of
/// the character it was on -- which is what choosing a search match looks
/// like when the match is the thing you are staring at.
///
/// `changed` is [`changed`], which every caller asks rather than working
/// out for itself: two of them answering it differently is the same defect
/// this function was written to end, one column further left.
#[must_use]
pub fn text_offset(lines: usize, changed: bool, folds: bool) -> u16 {
    let margin = if changed { MARGIN_WIDTH } else { 0 };
    let folding = if folds { FOLD_WIDTH } else { 0 };
    margin
        .saturating_add(gutter_width(lines))
        .saturating_add(folding)
}

/// The column the change map takes, just inside the scrollbar.
///
/// The map and the bar are the same picture at the same scale -- the whole
/// file squeezed into the height of the screen -- so they belong beside
/// each other, and the reader reads across them: here is where you are, and
/// here is what has changed. It used to be *outside* the bar, which put the
/// bar one column short of the screen's edge; every list in obelus puts its
/// own bar in the last column, so a list opened over a file made the bar
/// jump sideways, and a list with a preview under it had one bar in two
/// columns with a rule between them. Inside the bar, both are true at once.
///
/// One column, the same width as the margin on the other side: the two are
/// one answer at two scales -- what changed on this line, and where else in
/// the file to look. Reserved on the same terms as the margin, so a file
/// obelus knows nothing about spends nothing.
///
/// On the left with the margin rather than out beyond the scrollbar, which
/// is where it was. Every other list in obelus puts its bar in the last
/// column; the editor's sat one column short of it, so a file list opened
/// over a file made the bar jump sideways -- and inside one screen, a list
/// with a preview under it had its bar in two different columns with a rule
/// between them. All the news about changes is on the left now, and all the
/// news about where you are is on the right.
pub const CHANGE_MAP_WIDTH: u16 = 1;

/// The mark a row of the map carries: a change on the half nearest the
/// text, a problem on the half away from it.
///
/// Both columns are split the same way, mirrored about the text between
/// them: in the margin the change hugs the text's edge of its cell and the
/// problem the screen's, and out here the change hugs the text's edge again
/// and the problem the scrollbar's. So a reader learns one rule -- the
/// change is the half nearer what it is about -- and reads both columns
/// with it.
///
/// This used to be a stroke on the left with the right half kept empty, so
/// that the map did not touch the bar: two thin strokes with a gap read as
/// two things, and `map + bar` with no gap reads as one thick bar. The gap
/// is spent now, and knowingly -- the bar beside it is a *full* block, so
/// this is the side that touches hardest. A cell has one foreground and one
/// background and nothing else, so two facts in one column is the gap or it
/// is nothing, and a reader who wants to know where else to look wants both
/// answers in the one picture.
/// The mark a row of the map carries.
///
/// The margin's own mark is against the *right* of its cell, where it sits
/// beside the text it is about. This one is against the left, so that it
/// does not touch the bar it is next to: two thin strokes with a gap read
/// as two things, and `map + bar` with no gap reads as one thick bar.
const MAP_MARK: char = '\u{258c}';

/// The column the scrollbar takes, on the right.
///
/// Always reserved, even for a file that fits: a column that came and went
/// would rewrap the text as files were opened, and an empty track is itself
/// an answer -- it says that what is on screen is all there is.
pub const SCROLLBAR_WIDTH: u16 = 1;

/// How many cells the gutter takes for a document with this many lines.
///
/// Enough digits for the largest line number, plus one column of separation,
/// never narrower than [`MINIMUM_GUTTER_WIDTH`]. No upper bound: showing a
/// wrong line number is worse than spending a column, and a file with more
/// than five digits of lines is rare rather than impossible.
#[must_use]
pub fn gutter_width(line_count: usize) -> u16 {
    let digits = line_count.max(1).ilog10() + 1;
    u16::try_from(digits)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .max(MINIMUM_GUTTER_WIDTH)
}

/// The editor region.
pub struct EditorView<'a> {
    buffer: Option<&'a Buffer>,
    highlights: &'a Highlights,
    theme: &'a Theme,
    /// The runs of characters to mark, for a preview of somewhere in
    /// particular.
    ///
    /// A list rather than one: a language server names one run, and a
    /// search names whatever characters the query matched, which is as many
    /// runs as the match is scattered over.
    marked: &'a [Span],
    /// What is drawn in the file that the file does not contain: the
    /// colours a server found written down, and what it would have the
    /// reader know.
    ///
    /// Under everything the reader did -- a selection, a mark -- because
    /// those are answers to something they just asked and these are
    /// standing facts about the text.
    drawn: &'a [crate::ui::Drawn],
    /// The characters selected in the file being read.
    selection: Option<Span>,
    /// What the language server says is wrong with the file being read.
    ///
    /// Empty for a preview: a preview is somewhere else, and what is wrong
    /// with the file the reader is editing is not about it.
    troubles: &'a [crate::lsp::trouble::Trouble],
    /// What has changed since the last commit, if obelus knows.
    ///
    /// `None` for a file outside a repository, and then the margin takes no
    /// column at all.
    changes: Option<&'a Changes>,
    /// The hunks the reader has opened, by the line each hangs above.
    opened: Vec<LineNumber>,
    /// Who last changed each line of the version that was blamed, if obelus
    /// has been told and the reader wants to see it.
    blame: Option<&'a [Option<git::Blamed>]>,
    /// Whether that version is the text on screen, line for line.
    ///
    /// A commit's version *is* what was blamed, so its lines line up. A file
    /// on disk may have moved on from the commit it was blamed at, and then
    /// every uncommitted line above it shifts every name below, so a line
    /// has to be carried back through the changes before it can be looked
    /// up.
    blamed_here: bool,
    /// Whether a line too long for the width continues on the next row.
    wrap: bool,
    /// Whether this is the document being read or a look at another one.
    editing: Editing,
}

impl EditorView<'_> {
    /// The bar down the right-hand edge: where in the file this screen is.
    ///
    /// Measured in *lines*, not in visual rows. Counting rows would mean
    /// wrapping every line in the document on every frame, which is the one
    /// thing this program must not do -- and a scrollbar is an indication of
    /// where you are, not a measurement. With wrapping on, a file of very
    /// long lines shows a thumb a little too big; nothing depends on it.
    /// Every change in the file, in one column beside the bar.
    ///
    /// Not only the changes on screen: the margin says what changed *here*,
    /// and this says where else to look. Its rows are lines of the *file*,
    /// the same mapping the bar uses, so a mark is level with the part of
    /// the bar that would bring it into view.
    fn change_map(&self, cells: &mut CellBuffer, area: Rect, buffer: &Buffer) {
        // The same picture at the same scale as the bar beside it, so the
        // same count: what is folded away is not part of the document this
        // is a picture of, and a change inside a closed run sits at the row
        // the run folded into.
        let folds = buffer.folds();
        let shown = |line: LineNumber| line.get() - folds.hidden_before(line);
        let total = buffer.text().line_count() - folds.hidden_total();
        let row_of = |line: LineNumber| crate::ui::bar_row(shown(line), total, area.height);

        // Gathered before anything is drawn, because a row holds many
        // lines: on a file taller than the screen two hunks land on the
        // same row, and which of them the reader sees cannot be whichever
        // was looked at last.
        let mut rows: Vec<Option<Marker>> = vec![None; usize::from(area.height)];
        if let Some(changes) = self.changes {
            for hunk in changes.hunks() {
                let first = row_of(hunk.line);
                // At least the row it starts on, so a change of one line is
                // not lost to the arithmetic, and every row a long one
                // covers, so that a rewrite does not read like a one-line
                // fix.
                let last = row_of(hunk.line.saturating_add(hunk.lines.max(1) - 1)).max(first);
                for row in first..=last {
                    if let Some(cell) = rows.get_mut(usize::from(row)) {
                        *cell = Some(hunk.marker());
                    }
                }
            }
        }

        for (row, marker) in rows.iter().enumerate() {
            let (Ok(row), Some(marker)) = (u16::try_from(row), marker) else {
                continue;
            };
            let colour = self.theme.marker_colour(*marker);
            put(
                cells,
                area.x,
                area.y + row,
                MAP_MARK,
                Style::new().fg(colour),
            );
        }
    }

    /// What to write after a line: who changed it and how long ago.
    ///
    /// Nothing for a line the reader has changed since the last commit --
    /// the committed file has no such line, so no commit can be named for
    /// it, and a name from the line that used to be here would be a lie
    /// about the line that is.
    fn blame_at(&self, line: LineNumber, now: std::time::SystemTime) -> Option<String> {
        let blame = self.blame?;
        let at = git::blame::line_of(line, self.changes, self.blamed_here)?;
        git::blame::label(blame.get(at.get())?.as_ref(), now)
    }
}

/// Whether a view is the document itself or a look at somewhere else.
///
/// Nothing writes through a view yet, so this is mostly a promise about
/// what will: a preview is a few lines of a file the reader is not in, and
/// when the editor learns to change a file this is what says the preview
/// does not.
///
/// It has readers today, which is why it is a mode rather than a comment.
/// Three things belong to the document being read and not to a look at
/// another one: the selection, the name against the line the cursor is on,
/// and an opened hunk. All three are answers to "where am I and what am I
/// doing", and a preview is not where anybody is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editing {
    /// The document being read, and one day written.
    Allowed,
    /// A look at somewhere else.
    Refused,
}

impl<'a> EditorView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            buffer: app.current_buffer(),
            highlights: app.highlights(),
            theme: app.theme(),
            // What is being talked about: the uses of the name the pointer
            // is resting on, or what a hover is about while one is up.
            marked: app.marked_runs(),
            drawn: app.drawn(),
            selection: app.current_buffer().and_then(Buffer::selection),
            troubles: app.troubles(),
            changes: app.changes(),
            opened: app.opened_hunks(),
            blame: app.blame(),
            blamed_here: app
                .current_buffer()
                .is_some_and(|buffer| buffer.content().at().is_some()),
            wrap: app.config().wrap,
            editing: Editing::Allowed,
        }
    }

    /// Draws a document that is not the one being read.
    ///
    /// What makes a preview look like the editor is that it *is* the editor:
    /// the same gutter, the same highlighting, the same wrapping. A second
    /// drawing path would be a second set of those decisions, and they would
    /// drift.
    /// `marked` is the run of characters the preview is about — the symbol a
    /// language server named. A list of references is read by looking at that
    /// symbol in each one, and a preview that says only which line leaves the
    /// reader finding it again on every row.
    ///
    /// `changes` is what git says about that file, so a preview carries the
    /// same margin the editor does: a reader looking at a list of matches
    /// wants to know which of them are in code that has just been touched,
    /// and that is the same question the margin answers everywhere else.
    #[must_use]
    pub const fn for_buffer(
        buffer: &'a Buffer,
        highlights: &'a Highlights,
        theme: &'a Theme,
        marked: &'a [Span],
        changes: Option<&'a Changes>,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            highlights,
            theme,
            marked,
            // Nothing for a preview: what is drawn there is somewhere else,
            // and where a colour or a hint goes is a fact about the file
            // the reader is in.
            drawn: &[],
            changes,
            // Everything that answers "where am I and what am I doing" is
            // the document's rather than a look at another one's.
            selection: None,
            troubles: &[],
            opened: Vec::new(),
            blame: None,
            blamed_here: false,
            // A preview always wraps: a line running off its edge with no
            // way to scroll it would be a line nobody can read.
            wrap: true,
            editing: Editing::Refused,
        }
    }

    /// Whether this view may be written through.
    #[must_use]
    pub const fn editing(&self) -> Editing {
        self.editing
    }
}

impl Widget for EditorView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );

        let Some(buffer) = self.buffer else {
            return;
        };

        let text = buffer.text();
        // The margin, then the gutter, then the text, then the scrollbar.
        // The margin is leftmost because it is about the line as a whole and
        // the line number is about where it is: a mark inside the numbers
        // would read as part of one.
        let marking = changed(self.changes);
        let margin = if marking {
            MARGIN_WIDTH.min(area.width)
        } else {
            0
        };
        // The same total the caret's position is worked out from, which is
        // what keeps the two agreeing -- checked only where the caret is
        // drawn at all. A screen too narrow for the margin and the gutter
        // clamps them away, and `cursor_position` draws nothing there: what
        // the two would disagree about is a caret neither of them puts on
        // the screen.
        let folding = !buffer.folds().is_empty();
        let before = text_offset(text.line_count(), marking, folding);
        let gutter = gutter_width(text.line_count()).min(area.width - margin);
        let folds = if folding {
            FOLD_WIDTH.min(area.width - margin - gutter)
        } else {
            0
        };
        let map = map_width(self.changes).min(area.width - margin - gutter - folds);
        // The same total the caret's position is worked out from, which is
        // what keeps the two agreeing -- checked only where the caret is
        // drawn at all. A screen too narrow for what goes before the text
        // clamps it away, and `cursor_position` draws nothing there: what
        // the two would disagree about is a caret neither of them puts on
        // the screen.
        debug_assert!(
            before >= area.width || before == margin + gutter + folds,
            "the caret and the text disagree about what comes before the text"
        );
        let bar = SCROLLBAR_WIDTH.min(area.width - margin - gutter - folds - map);
        let width = area.width - margin - gutter - folds - bar - map;
        if width == 0 {
            return;
        }
        // Just inside the bar, which is the same picture at the same
        // scale: the whole file in the height of the screen.
        if map > 0 {
            let column = Rect {
                x: area.right() - bar - map,
                width: map,
                ..area
            };
            self.change_map(cells, column, buffer);
        }
        let cursor = buffer.cursor();
        let viewport = buffer.viewport();

        // The bracket pair, worked out once for the frame rather than per
        // row: it is one scan over what is on screen, and every row asks the
        // same question.
        let visible = crate::app::visible_bytes(buffer, area.height);
        let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));
        let brackets = brackets::pair_at(text, self.highlights, at, visible);

        // The hunks the reader has opened, worked out once: every row asks
        // whether it is one of their lines.
        let opened: Vec<(&crate::git::Hunk, Color)> = self
            .opened
            .iter()
            .filter_map(|anchor| {
                self.changes
                    .and_then(|changes| changes.hunk_at(*anchor))
                    .map(|hunk| (hunk, self.theme.marker_background(hunk.marker())))
            })
            .collect();
        // What the reader has selected inside it, in the block's own
        // coordinates -- the selection the rest of this draws is the
        // file's, and says nothing about lines the file does not have.
        let selected = buffer.block_selection();
        // Whichever block the caret is in: the others are on screen with
        // nothing selected in them, and a span from one drawn in another
        // would mark lines nobody chose.
        let selected_in = |above: LineNumber| {
            selected
                .filter(|(block, _)| *block == above)
                .map(|(_, span)| span)
        };
        // And nothing to colour those lines by: a block's text has no
        // syntax tree of its own, and the row's colour is what says the
        // lines are gone.
        let plain = Highlights::default();

        // One clock reading for the frame, taken here rather than where it
        // is used: a frame is a moment, and "how long ago" is measured from
        // it rather than from whenever each row happened to be drawn.
        let now = std::time::SystemTime::now();

        let mut screen_row = 0u16;
        let mut line = viewport.top;
        let mut skip = viewport.top_row;

        // One past the last line, because a block can hang there: a hunk
        // that deleted the end of a file, or what a server says is wrong
        // with its last line. That pass draws the block and stops -- there
        // is no line at it to draw, and nothing after it.
        while screen_row < area.height && line.get() <= text.line_count() {
            let past = line.get() == text.line_count();
            if !past {
                // Past whatever is folded away here. Not a `continue` per
                // line: a file folded down to a dozen rows would otherwise
                // walk every line it is hiding, once per frame.
                line = buffer.folds().first_shown(line);
                if line.get() > text.line_count() {
                    break;
                }
            }
            // What this line replaced, if the reader has opened it. Above the
            // line, because that is where it was, and pushing the file down
            // rather than overwriting anything: text that is not in the file
            // must not look like text that is.
            // Nothing for a hunk that replaced nothing: an added one opens
            // like any other -- the tint behind its lines is what says what
            // kind of change it is -- and has no rows of its own to draw.
            if let Some(block) = buffer.block_above(line).filter(|block| !block.is_empty()) {
                // The rows above this line are rows of the screen, so the
                // viewport can be inside them: whatever the top row skips
                // is spent here first, and the rest of the block is drawn
                // from there. Without that a deletion taller than the
                // screen could only ever be seen from its first row --
                // which, past that first screenful, is no way to read it.
                let wrap_width = if self.wrap { width } else { u16::MAX };
                let mut into = skip.min(block.rows(wrap_width));
                skip -= into;
                // Line by line and row by row, the way the loop below walks
                // the file: one wrapping per line rather than one per row,
                // which for a deletion of a few hundred lines is the
                // difference between a frame and several.
                'block: for removed in 0..block.text().line_count() {
                    let removed = LineNumber::new(removed);
                    for wrap in block.text().wrap_rows(removed, wrap_width) {
                        if into > 0 {
                            into -= 1;
                            continue;
                        }
                        if screen_row >= area.height {
                            break 'block;
                        }
                        let y = area.y + screen_row;
                        // Filled first, so the text below is written onto
                        // the tint rather than the tint over the text.
                        fill(
                            cells,
                            Rect {
                                x: area.x + margin,
                                y,
                                width: gutter + width,
                                height: 1,
                            },
                            Style::new().fg(self.theme.foreground).bg(match block.kind {
                                crate::buffer::Held::Removed => {
                                    self.theme.change_removed_background
                                }
                                // The page's own colour. A deletion is
                                // tinted because the tint is what says
                                // those lines are gone -- there is nothing
                                // else on the row to say it. A message has
                                // the bar down its left and no line numbers
                                // beside it, which is already two things
                                // saying it is not the file; a panel of
                                // another colour on top of the file, the
                                // height of somebody's prose, was the
                                // loudest thing on a screen whose subject
                                // is the code underneath.
                                // The page's own colour for a complaint
                                // too, and for the same reason twice over:
                                // it has the bar and no line numbers
                                // already, and a red panel across the file
                                // would be the loudest thing on a screen
                                // whose subject is the code the complaint
                                // is about.
                                crate::buffer::Held::Message | crate::buffer::Held::Wrong => {
                                    self.theme.background
                                }
                            }),
                        );
                        // A bar down the whole height rather than the
                        // boundary mark a deletion gets in the margin:
                        // `Marker::Removed`'s top edge exists because
                        // deleted lines have no row of their own, and
                        // opening the hunk is exactly the act of giving
                        // them one. The colour still says they are gone.
                        //
                        // In the first column of whatever comes before
                        // the text: the margin where there is one, and the
                        // gutter's own left-hand padding where there is
                        // not. Not a column of its own and not skipped
                        // when the margin is unreserved -- these rows have
                        // no line number, so that cell is empty either
                        // way, and a bar that came and went with whether
                        // git had anything to say would be a bar that
                        // means something it does not.
                        {
                            draw_marker(
                                area.x,
                                y,
                                Marker::Modified,
                                match block.kind {
                                    crate::buffer::Held::Removed => self.theme.change_removed,
                                    crate::buffer::Held::Message => self.theme.gutter,
                                    // How bad it is, in the colour the same
                                    // trouble underlines the line in: one
                                    // complaint, one colour, whichever of
                                    // them the reader's eye lands on first.
                                    crate::buffer::Held::Wrong => {
                                        block.severity.map_or(self.theme.gutter, |severity| {
                                            self.theme.colour_for(Some(severity.kind()))
                                        })
                                    }
                                },
                                cells,
                            );
                        }
                        // No line number: these lines have no number in
                        // this file, and borrowing the next one's would be
                        // a lie about where they are.
                        //
                        // Written by the writer every other row goes
                        // through, over the block's own text: it wraps at
                        // the same width, its tabs reach the same stops,
                        // and what the reader selected in it is drawn like
                        // any other selection. Nothing is highlighted --
                        // the row's colour is what says these lines are
                        // gone, and syntax on a red row would be two things
                        // saying different ones.
                        let ended = draw_row(
                            Placement {
                                x: area.x + margin + gutter + folds,
                                y,
                                width,
                                row: wrap,
                                // Scrolled with the file: with wrapping off
                                // there is a window on every row of the
                                // screen, and a block whose rows ignored it
                                // would put its text under a caret that had
                                // followed the window.
                                left: if self.wrap { 0 } else { viewport.left },
                            },
                            block.text(),
                            removed,
                            cells,
                            &Painting {
                                highlights: &plain,
                                theme: self.theme,
                                // A complaint is prose about a line, not a
                                // line, and it is drawn in its trouble's
                                // colour so that it does not read as a
                                // line of the file written in English.
                                // Its last row is obelus counting the
                                // other troubles on that line rather than
                                // the server's own words, and goes a shade
                                // back for the reason the commit's `+n -n`
                                // does: obelus's arithmetic must not read
                                // as something somebody said.
                                //
                                // A deletion keeps the plain colour: the
                                // tint behind it is what says those lines
                                // are gone, and a second thing saying it
                                // in a different colour says a different
                                // thing.
                                ink: block
                                    .severity
                                    .map(|severity| self.theme.colour_for(Some(severity.kind()))),
                                marked: &[],
                                // A block is a commit's version of these
                                // lines, and what a server found is in the
                                // file as it is now -- at columns that mean
                                // nothing here.
                                drawn: &[],
                                selection: selected_in(block.above),
                                // A block is a commit's version of these
                                // lines. What is wrong with the file is
                                // wrong with the file, not with that.
                                troubles: &[],
                                brackets: None,
                            },
                        );
                        // What the commit did to this file, after the row
                        // that says which commit it was. In the colours the
                        // margin marks the same two facts in, because they
                        // are the same two facts -- and drawn here rather
                        // than written into the message, which would make
                        // obelus's arithmetic part of what the author
                        // wrote.
                        if let Some((added, removed)) = block.changed.filter(|_| {
                            block.kind == crate::buffer::Held::Message
                                && removed == LineNumber::new(0)
                                && wrap.first == crate::coordinates::CharColumn::new(0)
                        }) {
                            draw_change_count(
                                area.x + margin + gutter + folds,
                                y,
                                width,
                                ended,
                                (added, removed),
                                self.theme,
                                cells,
                            );
                        }
                        screen_row += 1;
                    }
                }
            }

            if past {
                break;
            }

            let wrap_width = if self.wrap { width } else { u16::MAX };
            for (index, wrap) in text.wrap_rows(line, wrap_width).into_iter().enumerate() {
                if index < skip {
                    continue;
                }
                if screen_row >= area.height {
                    break;
                }
                let y = area.y + screen_row;

                // Behind the lines of an opened hunk: what kind of change
                // this is, said by the whole row. Not the margin column and
                // not the bar, which have marks of their own to stay legible
                // -- from the line number across to the end of the text, so
                // the block reads as one thing.
                if let Some((_, tint)) = opened.iter().find(|(hunk, _)| hunk.covers(line)) {
                    let tint = *tint;
                    fill(
                        cells,
                        Rect {
                            x: area.x + margin,
                            y,
                            width: gutter + folds + width,
                            height: 1,
                        },
                        Style::new().fg(self.theme.foreground).bg(tint),
                    );
                }

                // Only the first row of a wrapped line is numbered. Repeating
                // the number on every row of one long line is how a wrapped
                // view stops being readable.
                // The margin marks the line, whether or not this is the
                // row its number is on: a wrapped line is one line, and a
                // change to it is a change to all of it.
                if margin > 0
                    && let Some(changes) = self.changes
                    && let Some(marker) = changes.marker_at(line)
                {
                    draw_marker(area.x, y, marker, self.theme.marker_colour(marker), cells);
                }

                // Beside the number, and on the numbered row only: a
                // folded run stands for lines that are not on screen, and
                // the row that says so is the row the run starts on.
                if folds > 0 && index == 0 {
                    let mark = if buffer.folds().is_folded_at(line) {
                        Some((crate::ui::FOLDED, self.theme.gutter_current))
                    } else if buffer.folds().opens_at(line) {
                        Some((crate::ui::UNFOLDED, self.theme.gutter))
                    } else {
                        None
                    };
                    if let Some((glyph, colour)) = mark {
                        put(
                            cells,
                            area.x + margin + gutter,
                            y,
                            glyph,
                            Style::new().fg(colour),
                        );
                    }
                }

                if index == 0 {
                    draw_line_number(
                        area.x + margin,
                        y,
                        gutter,
                        line,
                        cells,
                        if line == cursor.line {
                            self.theme.gutter_current
                        } else {
                            self.theme.gutter
                        },
                    );
                }

                let placement = Placement {
                    x: area.x + margin + gutter + folds,
                    y,
                    width,
                    row: wrap,
                    // How much of the line is off the left-hand edge, which
                    // is only ever more than nothing when lines do not wrap.
                    left: if self.wrap { 0 } else { viewport.left },
                };
                let ended = draw_row(
                    placement,
                    text,
                    line,
                    cells,
                    &Painting {
                        highlights: self.highlights,
                        theme: self.theme,
                        // The file's own lines, which have a syntax tree.
                        ink: None,
                        marked: self.marked,
                        drawn: self.drawn,
                        selection: self.selection,
                        troubles: self.troubles,
                        brackets,
                    },
                );

                // The cursor's line only, and only after the *last* row of
                // it: the note is about the line the reader is on. On every
                // line it is a wall of grey beside the code -- it is on
                // screen more often than any other text obelus draws -- and
                // a reader who wants the name for a line can put the cursor
                // on it, which is where their attention already is.
                let last_row = index + 1 == text.row_count(line, wrap_width);
                // What is folded away here, said on the row it folded into.
                //
                // Two colours, because they are two different things. The
                // mark is the view's -- it is not in the file, and it is
                // drawn the way every other note obelus adds is. The
                // closing line *is* in the file, so it is drawn the colour
                // it would be at home: a brace that changed colour on its
                // way up the screen would read as something else.
                if folds > 0
                    && last_row
                    && let Some(fold) = buffer.folds().folded_at(line)
                {
                    let elision = elided(text, &fold);
                    let at = area.x + margin + gutter + folds + ended;
                    let mark_width =
                        u16::try_from(crate::text::text_width(&elision.mark)).unwrap_or(width);
                    let closing_width =
                        u16::try_from(crate::text::text_width(&elision.closing)).unwrap_or(width);
                    if ended + mark_width + closing_width <= width {
                        crate::ui::write(
                            cells,
                            at,
                            y,
                            &elision.mark,
                            Style::new().fg(self.theme.gutter),
                        );
                        if !elision.closing.is_empty() {
                            crate::ui::write(
                                cells,
                                at + mark_width,
                                y,
                                &elision.closing,
                                Style::new()
                                    .fg(self.theme.colour_for(self.highlights.kind_at(elision.at))),
                            );
                        }
                    }
                }
                if line == cursor.line
                    && last_row
                    && let Some(label) = self.blame_at(line, now)
                {
                    draw_blame(
                        area.x + margin + gutter + folds,
                        y,
                        width,
                        ended,
                        &label,
                        self.theme.gutter,
                        cells,
                    );
                }
                screen_row += 1;
            }
            skip = 0;
            line = line.saturating_add(1);
        }

        // The bar last, because whether there is anywhere to scroll is a
        // question only the loop above can answer: the file can run out
        // before the screen does, or the screen before the file, and with
        // wrapping on neither follows from the number of lines. A track
        // with no thumb on it is a control that does not work.
        let more_below = buffer.folds().first_shown(line).get() < text.line_count();
        let scrolled = viewport.top.get() > 0 || viewport.top_row > 0;
        if bar > 0 && (more_below || scrolled) {
            // The whole region, so the bar is in the last column of it --
            // which is where every list in obelus puts its own, and what
            // keeps them in one line when a list opens over a file.
            // Counted in the lines that are *shown*: the bar answers how
            // much of this there is and which part of it is in front of
            // you, and with a run closed the document is shorter and the
            // reader is further into it than the file's own numbers say.
            let folds = buffer.folds();
            crate::ui::scrollbar(
                cells,
                area,
                viewport.top.get() - folds.hidden_before(viewport.top),
                text.line_count() - folds.hidden_total(),
                self.theme,
            );
        }
    }
}

/// What to draw after the line a folded run folds into: the view's mark,
/// and the run's closing line when it has one worth showing.
///
/// Two pieces rather than one string because they are coloured
/// differently: the mark is obelus's and the closing line is the file's.
/// The closing line comes along only when it is short enough to read as a
/// closing mark rather than as code in its own right.
fn elided(text: &crate::text::Text, fold: &crate::buffer::folds::Fold) -> Elision {
    let line = text.line(fold.to).to_string();
    // What is left of the run's last line, past where the run stops on it.
    // No rule about what a closing mark looks like and no table per
    // language: the run was built to stop before the bracket, so this is
    // simply what is on the other side of where it stops.
    let left = fold.tail.map(|tail| {
        line.chars()
            .skip(tail.get())
            .collect::<String>()
            .trim_end()
            .to_string()
    });
    // Nothing left when the run ends at the end of its last line, which is
    // what a block with no closing bracket does: the row has the mark and
    // nothing else, and `def ready(): ...` is the whole of what a language
    // that closes its blocks with nothing has to show.
    let closing = left.unwrap_or_default();
    if closing.is_empty() {
        return Elision {
            mark: format!(" {ELIDED}"),
            closing,
            at: ByteOffset::new(0),
        };
    }
    // Where that text sits in the file, so the highlighting can be asked
    // what colour it is at home: a brace that changed colour on its way up
    // the screen would read as something else.
    let indent = line.chars().count() - line.trim_start().chars().count();
    let at = match fold.tail.filter(|tail| tail.get() < indent) {
        Some(tail) => tail,
        None => CharColumn::new(indent),
    };
    Elision {
        mark: format!(" {ELIDED} "),
        closing,
        at: text.byte_of_char(text.char_offset(fold.to, at)),
    }
}

/// What a folded row draws after its own text.
struct Elision {
    /// The view's own mark, drawn the way obelus draws its notes.
    mark: String,
    /// What is left of the run's last line, or nothing when the server did
    /// not say where the run stops on it.
    closing: String,
    /// Where that text begins in the file, for its colour.
    at: ByteOffset,
}

/// Writes a right-aligned line number, one-based, with a trailing space.
fn draw_line_number(
    x: u16,
    y: u16,
    width: u16,
    line: LineNumber,
    cells: &mut CellBuffer,
    colour: Color,
) {
    if width == 0 {
        return;
    }
    let label = (line.get() + 1).to_string();
    let digits = u16::try_from(label.len()).unwrap_or(u16::MAX);
    // The separating space is the last column, so the number is right-aligned
    // in the ones before it.
    let padding = width.saturating_sub(1).saturating_sub(digits);
    for (index, character) in label.chars().enumerate() {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        let Some(offset) = padding.checked_add(index) else {
            break;
        };
        if offset >= width {
            break;
        }
        put(cells, x + offset, y, character, Style::new().fg(colour));
    }
}

/// Where one visual row of text goes.
#[derive(Clone, Copy)]
struct Placement {
    /// The first column of the text area.
    x: u16,
    /// The screen row.
    y: u16,
    /// How many columns the text area has.
    width: u16,
    /// Which slice of the line this row shows.
    row: WrapRow,
    /// How many cells of the line are off the left-hand edge.
    left: usize,
}

/// One cell of margin, saying what happened to a line.
///
/// A bar for a line that is there and differs; a mark hugging the top edge
/// for lines that are *not* there. The second is the whole difficulty of
/// showing a deletion in a grid of cells: the removed lines have no row of
/// their own, so what is left is the boundary they were on, and the top
/// edge of the cell below it is that boundary. A full bar there would claim
/// the line changed, and it did not.
///
/// git's news and nothing else. What is wrong with a line the reader can
/// see is said by the underline under the word and, when the caret is on
/// it, by the complaint framed underneath -- a third mark beside the line
/// would be the same news a third time, and it would have to share this
/// cell with marks whose colours it cannot be told apart from.
fn draw_marker(x: u16, y: u16, marker: Marker, colour: Color, cells: &mut CellBuffer) {
    let glyph = match marker {
        // A line that is there and differs: a bar down its whole height,
        // half a cell wide and against the *right* edge of its cell. It
        // then sits beside the text it is about; against the other edge it
        // would float a cell away from it.
        Marker::Added | Marker::Modified => '\u{2590}',
        // Lines that are not there: a mark on the boundary they were on,
        // which is the top edge of this cell.
        Marker::Removed => '\u{2594}',
    };
    put(cells, x, y, glyph, Style::new().fg(colour));
}

/// Everything about how a row looks, as against where it goes.
///
/// A struct because the list had grown to the point where the compiler was
/// the only thing keeping the order straight -- and because "where" and
/// "how" really are two groups.
struct Painting<'a> {
    highlights: &'a Highlights,
    theme: &'a Theme,
    /// One colour for every character of the row, where the row is not the
    /// file and has no syntax to be coloured by.
    ///
    /// A complaint is written in the colour everything else about that
    /// trouble is written in -- the underline under the word, the bar down
    /// its left, the mark out on the map. Four things saying one thing, so
    /// one colour. `None` leaves the row to the highlights, which is every
    /// row of the file and the lines a commit removed.
    ink: Option<Color>,
    /// The run a preview is about.
    marked: &'a [Span],
    /// What is drawn in the line that the line does not contain: the
    /// colours a server found written down, and what it would have the
    /// reader know.
    drawn: &'a [crate::ui::Drawn],
    /// The characters the reader selected in the file being read.
    selection: Option<Span>,
    /// What the language server says is wrong with the file.
    troubles: &'a [crate::lsp::trouble::Trouble],
    /// The bracket under the cursor and its partner.
    brackets: Option<(ByteOffset, ByteOffset)>,
}

/// Draws one visual row of a line, and says which column it ended at.
///
/// The column is what the blame at the end of the line needs: "after the
/// text" is only knowable by whoever drew the text.
fn draw_row(
    placement: Placement,
    text: &crate::text::Text,
    line: LineNumber,
    cells: &mut CellBuffer,
    painting: &Painting<'_>,
) -> u16 {
    let Placement {
        x,
        y,
        width,
        row,
        left,
    } = placement;
    // Where this row's characters begin, plus whatever is scrolled off the
    // side. With wrapping the second is always zero, by construction: the
    // row is exactly the characters that fit.
    let start = usize::from(text.display_column(line, row.first).get()) + left;
    // Where the row stops, worked out once. Asked inside the loop below it
    // was a walk of the line per glyph of the line, which is what a row of
    // text cost to draw: the square of its length, forty times a frame.
    let stop = usize::from(text.display_column(line, row.end).get());
    let indent = usize::from(row.indent);
    let mut ended = indent.try_into().unwrap_or(u16::MAX);

    // The glyph's own column, not the number of steps taken: a line can be
    // drawn with cells in it that the line does not contain, so counting
    // the steps stopped being the same as counting the columns.
    for glyph in text.glyphs(line) {
        let column = glyph.column.get();
        // A glyph the left-hand edge has cut in half leaves its cell blank:
        // half of a wide character is not that character, and drawing it
        // would put the rest of the row a column out of place. This is the
        // case that could not happen while everything wrapped -- wrapping
        // refuses to put a wide glyph across an edge -- and it comes back
        // with the sideways scrolling.
        if glyph.first_cell < start && glyph.first_cell + glyph.cells.max(1) > start {
            put(cells, x + indent as u16, y, ' ', Style::new());
            continue;
        }
        if glyph.first_cell < start {
            continue;
        }
        if glyph.first_cell >= stop {
            break;
        }
        let Ok(offset) = u16::try_from(indent + glyph.first_cell - start) else {
            break;
        };
        if offset >= width {
            break;
        }
        let colour = painting.ink.unwrap_or_else(|| {
            painting
                .theme
                .colour_for(painting.highlights.kind_at(glyph.first_byte))
        });

        // A foreground, so the background the fill painted stays — except
        // where the run being marked needs one of its own.
        let mut style = Style::new().fg(colour);
        if painting
            .marked
            .iter()
            .any(|marked| marked.contains(line, CharColumn::new(column)))
        {
            style = style.bg(painting.theme.marked_background);
        }
        if painting
            .selection
            .is_some_and(|selection| selection.contains(line, CharColumn::new(column)))
        {
            style = style.bg(painting.theme.selection_background);
        }
        // Underlined where a server says something is wrong, in the colour
        // that kind of trouble is written in. An underline rather than a
        // background or a foreground: those two are taken -- by the
        // selection and by the syntax -- and a third claim on them would
        // hide one of the two.
        if let Some(severity) = painting
            .troubles
            .iter()
            .filter(|trouble| trouble.span.contains(line, CharColumn::new(column)))
            .map(|trouble| trouble.severity)
            .min()
        {
            style = style
                .add_modifier(Modifier::UNDERLINED)
                .underline_color(painting.theme.colour_for(Some(severity.kind())));
        }
        // The bracket the cursor is on, and its partner. After the mark, so
        // a symbol a preview is about keeps its own background where the two
        // land on the same cell.
        if painting
            .brackets
            .is_some_and(|(open, close)| glyph.first_byte == open || glyph.first_byte == close)
        {
            style = style.bg(painting.theme.bracket_background);
        }

        // A cell the file does not contain, drawn as whatever put it
        // there. Written here rather than left to the character below,
        // because a phantom has no character: the cell count is what the
        // rest of the line was laid out against, so what goes in it has to
        // be exactly that wide.
        if let Some(drawn) = glyph.phantom.and_then(|which| painting.drawn.get(which)) {
            match drawn {
                // A colour written down here, shown as a square of itself
                // in front of the literal. The literal keeps its own
                // syntax colour: painting it said the same thing over
                // seven characters, and over the sixteen of an
                // `rgba(0, 0, 0, .5)` it said it over half a line.
                crate::ui::Drawn::Swatch(colour) => {
                    put(cells, x + offset, y, crate::ui::SWATCH, style.fg(*colour));
                    ended = offset + 1;
                }
                // Dim, and in its own colour: a hint is not code, and a
                // reader skimming a file for what it says has to be able
                // to skip it without reading it.
                crate::ui::Drawn::Hint(hint) => {
                    let style = style.fg(painting
                        .theme
                        .colour_for(Some(crate::kind::SyntaxKind::Comment)));
                    let mut cell = 0usize;
                    for character in hint.label.chars() {
                        let taken = unicode_width::UnicodeWidthChar::width(character)
                            .unwrap_or(0)
                            .max(1);
                        if cell + taken > glyph.cells {
                            break;
                        }
                        let Ok(at) = u16::try_from(usize::from(offset) + cell) else {
                            break;
                        };
                        if at >= width {
                            break;
                        }
                        put(cells, x + at, y, character, style);
                        ended = at + u16::try_from(taken).unwrap_or(1);
                        cell += taken;
                    }
                }
            }
            continue;
        }

        // A tab is blanks by definition.
        if glyph.character == '\t' {
            for cell in 0..glyph.cells.min(usize::from(width - offset)) {
                let Ok(cell) = u16::try_from(cell) else { break };
                put(cells, x + offset + cell, y, ' ', style);
                ended = offset + cell + 1;
            }
            continue;
        }

        put(cells, x + offset, y, glyph.character, style);
        ended = offset + u16::try_from(glyph.cells).unwrap_or(1);
    }
    ended
}

/// Writes who last changed a line, right-aligned at the end of its row.
///
/// Right-aligned rather than two columns after the text: the note is on
/// whichever line the cursor is on, so hung off the text it would jump left
/// and right as the reader moves down the file, and a thing that moves is a
/// thing the eye follows. At the right-hand edge it stays where it was and
/// the reader can look at it or not.
///
/// No column is reserved for it, so a line long enough to reach it keeps its
/// own space and loses the note. Code is never written over to make room for
/// a note about code.
/// `+added \u{2212}removed` after the text of a row, in the margin's colours.
///
/// Two words rather than one string, because they are two facts and wear two
/// colours: green for what arrived, red for what went. The gap in front is
/// the blame's gap, for the same reason -- it has to read as a note about
/// the row rather than as more of it.
fn draw_change_count(
    x: u16,
    y: u16,
    width: u16,
    text_ends: u16,
    changed: (usize, usize),
    theme: &Theme,
    cells: &mut CellBuffer,
) {
    let (added, removed) = changed;
    let words = [
        (format!("+{added}"), theme.change_added),
        (format!("\u{2212}{removed}"), theme.change_removed),
    ];
    let wanted: usize = words
        .iter()
        .map(|(word, _)| crate::text::text_width(word) + 1)
        .sum();
    let Ok(wanted) = u16::try_from(wanted) else {
        return;
    };
    // Three columns after the date, the way the message's own columns are
    // spaced. A row with no room for it keeps its text instead: the number
    // is a note, and a note may be the thing that goes.
    let mut column = text_ends.saturating_add(3);
    if column.saturating_add(wanted) > width {
        return;
    }
    for (word, colour) in words {
        column = crate::ui::write(cells, x + column, y, &word, Style::new().fg(colour))
            .saturating_sub(x)
            .saturating_add(1);
    }
}

fn draw_blame(
    x: u16,
    y: u16,
    width: u16,
    text_ends: u16,
    label: &str,
    colour: Color,
    cells: &mut CellBuffer,
) {
    let Ok(label_width) = u16::try_from(crate::text::text_width(label)) else {
        return;
    };
    // One column short of the edge, because the bar is the next cell and
    // grey text touching it reads as part of it.
    let Some(offset) = width.checked_sub(label_width + 1) else {
        return;
    };
    // Two columns of gap at least, so it reads as a note rather than as
    // more code -- and so a line that reaches this far keeps its own space.
    if offset < text_ends + 2 {
        return;
    }
    crate::ui::write(cells, x + offset, y, label, Style::new().fg(colour));
}

#[cfg(test)]
mod tests {
    use super::{MINIMUM_GUTTER_WIDTH, gutter_width};

    #[test]
    fn short_files_get_the_minimum() {
        for lines in [1, 9, 10, 999] {
            assert_eq!(gutter_width(lines), MINIMUM_GUTTER_WIDTH);
        }
    }

    #[test]
    fn the_gutter_grows_once_the_numbers_no_longer_fit() {
        // Five digits plus the separating space is the first width past the
        // minimum, and nothing caps it after that.
        assert_eq!(gutter_width(9_999), 5);
        assert_eq!(gutter_width(10_000), 6);
        assert_eq!(gutter_width(99_999), 6);
        assert_eq!(gutter_width(100_000), 7);
        assert_eq!(gutter_width(1_000_000), 8);
    }
}
