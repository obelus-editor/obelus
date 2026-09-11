//! The gutter and the text.
//!
//! A hand-written widget rather than a `Paragraph`: the text needs per-cell
//! styling, a tab has to expand to the next tab stop, a wide glyph has to
//! occupy two cells, and a glyph straddling either edge of a horizontally
//! scrolled viewport has to render as blanks. None of that survives going
//! through a widget that takes styled spans.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::{
    app::App,
    buffer::Buffer,
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    git::{self, Changes, Marker},
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

/// How many columns come before the text: the change margin, then the
/// gutter.
///
/// One function, because two of them disagreed. The caret's own position
/// used the gutter alone while the text is drawn after the margin as well,
/// so on every file in a repository the caret sat one cell to the left of
/// the character it was on -- which is what choosing a search match looks
/// like when the match is the thing you are staring at.
#[must_use]
pub fn text_offset(lines: usize, changes: bool) -> u16 {
    let margin = if changes { MARGIN_WIDTH } else { 0 };
    margin.saturating_add(gutter_width(lines))
}

/// How many rows an opened hunk draws above a line.
///
/// Opening a hunk pushes the file down to make room for what its lines
/// replaced, so everything from that line onwards is drawn lower than the
/// text alone would put it -- the caret included. Without this the caret
/// sat on a row belonging to text that is not in the file, and the reader
/// walked out of the hunk while it still looked as though they were in it.
///
/// The same rule the drawing follows, said once: the removed lines appear
/// when the loop reaches the line they belong to, so they are drawn only
/// while that line is on screen and starts on a row of its own.
#[must_use]
pub fn hunk_rows_above(
    changes: Option<&crate::git::Changes>,
    opened: Option<LineNumber>,
    top: LineNumber,
    top_row: usize,
    line: LineNumber,
) -> u16 {
    let Some(anchor) = opened else {
        return 0;
    };
    if anchor > line || anchor < top || (anchor == top && top_row > 0) {
        return 0;
    }
    changes
        .and_then(|changes| changes.hunk_at(anchor))
        .map_or(0, |hunk| {
            u16::try_from(hunk.removed.len()).unwrap_or(u16::MAX)
        })
}

/// The column the change map takes, right of the scrollbar.
///
/// One column, the same width as the margin on the other side, and drawn
/// with the same glyph: the two are one answer at two scales -- what changed
/// on this line, and where else in the file to look. Reserved on the same
/// terms as the margin, so a file obelus knows nothing about spends nothing.
pub const CHANGE_MAP_WIDTH: u16 = 1;

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
    /// The characters selected in the file being read.
    selection: Option<Span>,
    /// What has changed since the last commit, if obelus knows.
    ///
    /// `None` for a file outside a repository, and then the margin takes no
    /// column at all.
    changes: Option<&'a Changes>,
    /// The hunk the reader has opened, if any.
    opened: Option<LineNumber>,
    /// Who last changed each line of the committed file, if obelus has been
    /// told and the reader wants to see it.
    blame: Option<&'a [Option<git::Blamed>]>,
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
        let Some(changes) = self.changes else {
            return;
        };
        let total = buffer.text().line_count();
        for hunk in changes.hunks() {
            let marker = hunk.marker();
            let first = crate::ui::bar_row(hunk.line.get(), total, area.height);
            // At least the row it starts on, so a change of one line is not
            // lost to the arithmetic, and every row a long one covers, so
            // that a rewrite does not read like a one-line fix.
            let last =
                crate::ui::bar_row(hunk.line.get() + hunk.lines.max(1) - 1, total, area.height)
                    .max(first);
            for row in first..=last {
                draw_marker(
                    area.x,
                    area.y + row,
                    marker,
                    self.marker_colour(marker),
                    cells,
                );
            }
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
        // Through the working-tree changes, because the blame is about the
        // committed file: without this every uncommitted line above shifts
        // every name below it.
        let at = match self.changes {
            Some(changes) => changes.committed_line(line)?,
            None => line,
        };
        git::blame::label(blame.get(at.get())?.as_ref(), now)
    }

    /// The colour a marker is drawn in, wherever it is drawn.
    const fn marker_colour(&self, marker: Marker) -> Color {
        match marker {
            Marker::Added => self.theme.change_added,
            Marker::Modified => self.theme.change_modified,
            Marker::Removed => self.theme.change_removed,
        }
    }

    /// The tint behind a line of an opened hunk.
    const fn marker_background(&self, marker: Marker) -> Color {
        match marker {
            Marker::Added => self.theme.change_added_background,
            Marker::Modified => self.theme.change_modified_background,
            Marker::Removed => self.theme.change_removed_background,
        }
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
            marked: &[],
            selection: app.current_buffer().and_then(Buffer::selection),
            changes: app.changes(),
            opened: app.opened_hunk(),
            blame: app.blame(),
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
            changes,
            // Everything that answers "where am I and what am I doing" is
            // the document's rather than a look at another one's.
            selection: None,
            opened: None,
            blame: None,
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
        let margin = if self.changes.is_some() {
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
        let before = text_offset(text.line_count(), self.changes.is_some());
        debug_assert!(
            before >= area.width || before == margin + gutter_width(text.line_count()),
            "the caret and the text disagree about what comes before the text"
        );
        let gutter = gutter_width(text.line_count()).min(area.width - margin);
        let map = if self.changes.is_some() {
            CHANGE_MAP_WIDTH.min(area.width - margin - gutter)
        } else {
            0
        };
        let bar = SCROLLBAR_WIDTH.min(area.width - margin - gutter - map);
        let width = area.width - margin - gutter - bar - map;
        if width == 0 {
            return;
        }
        if map > 0 {
            let column = Rect {
                x: area.right() - map,
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

        // The hunk the reader has opened, worked out once: every row asks
        // whether it is one of its lines.
        let opened = self.opened.and_then(|anchor| {
            self.changes
                .and_then(|changes| changes.hunk_at(anchor))
                .map(|hunk| (hunk, self.marker_background(hunk.marker())))
        });

        // One clock reading for the frame, taken here rather than where it
        // is used: a frame is a moment, and "how long ago" is measured from
        // it rather than from whenever each row happened to be drawn.
        let now = std::time::SystemTime::now();

        let mut screen_row = 0u16;
        let mut line = viewport.top;
        let mut skip = viewport.top_row;

        while screen_row < area.height && line.get() < text.line_count() {
            // What this line replaced, if the reader has opened it. Above the
            // line, because that is where it was, and pushing the file down
            // rather than overwriting anything: text that is not in the file
            // must not look like text that is.
            if skip == 0
                && self.opened == Some(line)
                && let Some(changes) = self.changes
                && let Some(hunk) = changes.hunk_at(line)
            {
                for removed in &hunk.removed {
                    if screen_row >= area.height {
                        break;
                    }
                    let y = area.y + screen_row;
                    // Filled first, so the text below is written onto the
                    // tint rather than the tint over the text.
                    fill(
                        cells,
                        Rect {
                            x: area.x + margin,
                            y,
                            width: gutter + width,
                            height: 1,
                        },
                        Style::new()
                            .fg(self.theme.foreground)
                            .bg(self.theme.change_removed_background),
                    );
                    // The bar a line on screen gets, not the boundary mark:
                    // `Marker::Removed`'s top edge exists because deleted
                    // lines have no row of their own, and opening the hunk
                    // is exactly the act of giving them one. The colour
                    // still says they are gone.
                    draw_marker(
                        area.x,
                        y,
                        Marker::Modified,
                        self.theme.change_removed,
                        cells,
                    );
                    // No line number: these lines have no number in this
                    // file, and borrowing the next one's would be a lie
                    // about where they are.
                    // No background of its own: the fill above already put
                    // the tint on this row, and a style that names one paints
                    // over it wherever there is a glyph -- which leaves the
                    // colour showing in the gaps between words and nowhere
                    // else. The ordinary foreground, because the row's colour
                    // is now what says these lines are gone, and red text on
                    // a red row is a line nobody can read.
                    crate::ui::write(
                        cells,
                        area.x + margin + gutter,
                        y,
                        removed,
                        Style::new().fg(self.theme.foreground),
                    );
                    screen_row += 1;
                }
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
                if let Some((hunk, tint)) = opened
                    && hunk.covers(line)
                {
                    fill(
                        cells,
                        Rect {
                            x: area.x + margin,
                            y,
                            width: gutter + width,
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
                    draw_marker(area.x, y, marker, self.marker_colour(marker), cells);
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
                    x: area.x + margin + gutter,
                    y,
                    width,
                    row: wrap,
                    // How much of the line is off the left-hand edge, which
                    // is only ever more than nothing when lines do not wrap.
                    left: if self.wrap { 0 } else { viewport.left },
                };
                let ended = draw_row(
                    placement,
                    buffer,
                    line,
                    cells,
                    &Painting {
                        highlights: self.highlights,
                        theme: self.theme,
                        marked: self.marked,
                        selection: self.selection,
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
                if line == cursor.line
                    && last_row
                    && let Some(label) = self.blame_at(line, now)
                {
                    draw_blame(
                        area.x + margin + gutter,
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
        let more_below = line.get() < text.line_count();
        let scrolled = viewport.top.get() > 0 || viewport.top_row > 0;
        if bar > 0 && (more_below || scrolled) {
            let track = Rect {
                width: area.width - map,
                ..area
            };
            crate::ui::scrollbar(
                cells,
                track,
                viewport.top.get(),
                text.line_count(),
                self.theme,
            );
        }
    }
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

/// One cell of margin or map, saying what happened to a line.
///
/// A bar for a line that is there and differs; a mark hugging the top edge
/// for lines that are *not* there. The second is the whole difficulty of
/// showing a deletion in a grid of cells: the removed lines have no row of
/// their own, so what is left is the boundary they were on, and the top
/// edge of the cell below it is that boundary. A full bar there would claim
/// the line changed, and it did not.
fn draw_marker(x: u16, y: u16, marker: Marker, colour: Color, cells: &mut CellBuffer) {
    let glyph = match marker {
        // A line that is there and differs: a bar down its whole height,
        // half a cell wide and against the *right* edge of its cell in both
        // columns. Left of the numbers it then sits beside the text it is
        // about; right of the scrollbar it sits at the edge of the screen.
        // Against the other edge each one floats a cell away from the thing
        // it belongs to.
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
    /// The run a preview is about.
    marked: &'a [Span],
    /// The characters the reader selected in the file being read.
    selection: Option<Span>,
    /// The bracket under the cursor and its partner.
    brackets: Option<(ByteOffset, ByteOffset)>,
}

/// Draws one visual row of a line, and says which column it ended at.
///
/// The column is what the blame at the end of the line needs: "after the
/// text" is only knowable by whoever drew the text.
fn draw_row(
    placement: Placement,
    buffer: &Buffer,
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
    let text = buffer.text();
    // Where this row's characters begin, plus whatever is scrolled off the
    // side. With wrapping the second is always zero, by construction: the
    // row is exactly the characters that fit.
    let start = usize::from(text.display_column(line, row.first).get()) + left;
    let indent = usize::from(row.indent);
    let mut ended = indent.try_into().unwrap_or(u16::MAX);

    for (column, glyph) in text.glyphs(line).enumerate() {
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
        if glyph.first_cell >= usize::from(text.display_column(line, row.end).get()) {
            break;
        }
        let Ok(offset) = u16::try_from(indent + glyph.first_cell - start) else {
            break;
        };
        if offset >= width {
            break;
        }
        let colour = painting
            .theme
            .colour_for(painting.highlights.kind_at(glyph.first_byte));

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
        // The bracket the cursor is on, and its partner. After the mark, so
        // a symbol a preview is about keeps its own background where the two
        // land on the same cell.
        if painting
            .brackets
            .is_some_and(|(open, close)| glyph.first_byte == open || glyph.first_byte == close)
        {
            style = style.bg(painting.theme.bracket_background);
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
fn draw_blame(
    x: u16,
    y: u16,
    width: u16,
    text_ends: u16,
    label: &str,
    colour: Color,
    cells: &mut CellBuffer,
) {
    let Ok(label_width) = u16::try_from(crate::ui::text_width(label)) else {
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
