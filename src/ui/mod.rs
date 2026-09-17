//! Drawing.
//!
//! Nothing in here reads a file, makes a syscall or parses anything. The draw
//! path runs inside `Terminal::draw`, which blocks the main loop on a write to
//! stdout; adding slow work to it is the mistake that actually happens, rather
//! than the write itself being slow.

pub mod card;
pub mod chat;
pub mod complete;
pub mod counts;
pub mod editor;
pub mod hover;
pub mod image;
pub mod picker;
pub mod reading;
pub mod settings;
pub mod signature;
pub mod status;
pub mod todo;
pub mod welcome;

use std::ops::Range;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    widgets::Widget as _,
};
use unicode_width::UnicodeWidthChar as _;

use crate::{
    app::{App, layers::Layer},
    component::picker::{Colouring, Picker},
    theme::Theme,
};

/// Where the two regions of the screen are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    /// The gutter and the text.
    pub editor: Rect,
    /// The rule between them.
    ///
    /// Empty on a screen with no room for it, which is a screen with
    /// nothing but a status bar on it.
    pub edge: Rect,
    /// The one-line status bar.
    pub status: Rect,
}

/// Splits the screen.
///
/// Called both before drawing, to scroll the cursor into view, and while
/// drawing. One function so the two cannot disagree about where the boundary
/// is.
#[must_use]
pub fn regions(area: Rect) -> Regions {
    let status_height = area.height.min(1);
    // A rule between the two, which is what every other boundary in obelus
    // has. The status bar has a band of its own and so did not need one to
    // be read as a different thing; what it needed one for is the row above
    // it, which is a picker's list, a page of settings or the box a message
    // to an agent is written in -- all of them things a reader is working
    // in, and all of them ending in a row that was touching the bar.
    let edge_height = area.height.saturating_sub(status_height).min(1);
    let editor_height = area.height - status_height - edge_height;
    Regions {
        editor: Rect {
            height: editor_height,
            ..area
        },
        edge: Rect {
            y: area.y + editor_height,
            height: edge_height,
            ..area
        },
        status: Rect {
            y: area.y + editor_height + edge_height,
            height: status_height,
            ..area
        },
    }
}

/// The screen as a `Rect` starting at the origin.
#[must_use]
pub fn area_of(size: Size) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    }
}

/// The path as it should be read: relative to the working directory when it
/// lies under it, and unchanged when it does not.
///
/// A reader spends its time inside one tree, and the leading directories of
/// that tree are the part already known. Shared, because more than one view
/// writes a path now: the status bar says which file is open, and a
/// conversation says which ones an agent has been in.
#[must_use]
pub fn relative_to<'a>(path: &'a std::path::Path, root: &std::path::Path) -> &'a std::path::Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// Where the terminal should put its cursor, if anywhere.
///
/// The terminal's own cursor rather than a painted block, so it takes the
/// shape and the blink the reader configured, and — the reason this matters —
/// goes hollow by itself when the window loses focus. A cell grid cannot
/// express that: the terminal draws an outline over the cell, and an
/// application can only put characters in it.
///
/// While a picker is open the cursor belongs in the prompt, which is also
/// where the keys are going.
#[must_use]
pub fn cursor_position(area: Rect, app: &App) -> Option<Position> {
    let regions = regions(area);
    // Most of them filter or answer by typing on the status row, so the
    // caret goes where that typing does.
    let on_the_status_row = |column: u16| {
        (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        })
    };

    // Whatever is nearest, which is where the keys are going. Asked once
    // rather than walked as a chain of its own: a caret drawn in one view
    // while the typing reaches another is a screen that lies about what a
    // key will do, and that is what two chains in two orders produced.
    match app.layers().nearest() {
        Some(Layer::Prompt) => return on_the_status_row(status::answer_caret(app.prompt()?)),
        Some(Layer::Picker) => return on_the_status_row(status::prompt_caret(app.picker()?)),
        Some(Layer::Settings) => {
            let settings = app.settings()?;
            return on_the_status_row(status::filter_caret(
                &settings.query(),
                settings.query_caret(),
            ));
        }
        // A note being written has one in the row it is being written in,
        // which is the row it will be read in. The same answer the
        // conversation gives, for the same reason: what is typed is a
        // paragraph, and a paragraph does not fit on the status bar.
        Some(Layer::Notes) => return todo::caret(regions.editor, app.notes()?),
        // Nothing is typed into the counts, so there is no caret in them:
        // what marks where the keys are going is the row's background, and
        // a caret as well would be two marks for one fact. Without this the
        // file behind them kept its own, blinking in a view it is not part
        // of.
        Some(Layer::Counts) => return None,
        // Nothing over the document, so the caret is the document's own.
        None => {
            // A conversation is written into, and its caret is in the box
            // rather than on the status bar: a message is a paragraph, and
            // a paragraph does not fit on one row.
            if let Some(chat) = app.chat() {
                return chat::ChatView::caret(regions.editor, chat, app.card());
            }
        }
    }

    let buffer = app.current_buffer()?;
    // No cursor over a rendering. The rows are not the file's lines, so
    // there is nowhere in them the cursor honestly is.
    if buffer.mode() != crate::buffer::Mode::Edit {
        return None;
    }
    // Everything the editor draws before the text: the change margin, the
    // gutter and the fold marks, from the function the editor lays them out
    // with.
    let offset = editor::text_offset(
        buffer.text().line_count(),
        app.changes().is_some(),
        !buffer.folds().is_empty(),
    );
    if offset >= regions.editor.width {
        return None;
    }
    // The rows an opened hunk draws are counted by the arithmetic that
    // answers this, so the caret comes back on the row it is really drawn
    // on: the text area knows what the view inserted.
    let (row, cell) = buffer.cursor_screen_cell(app.text_area())?;
    if row >= regions.editor.height || cell >= regions.editor.width - offset {
        return None;
    }
    Some(Position {
        x: regions.editor.x + offset + cell,
        y: regions.editor.y + row,
    })
}

/// Draws one frame into a cell grid.
///
/// Takes the grid rather than a `Frame` so the golden tests can assert on the
/// cells obelus wrote. Going through a `Frame` would mean reading them back
/// from the backend afterwards, and by then ratatui's diff has dropped the
/// cell a wide glyph covers — correctly, since the terminal advances two
/// columns for it, but the record left behind cannot be told apart from a cell
/// nothing painted.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &App) {
    let regions = regions(area);
    let layers = app.layers();
    // Under whatever the region holds and over the status bar, once, for
    // every view: what is above it changes and the boundary does not.
    rule(cells, regions.edge, app.theme());

    // The document being read, under everything. Which is a file or a
    // conversation: both fill the editor region, and neither is over the
    // other -- switching between them is switching documents, not opening
    // something. A file being shown some other way is shown that way: the
    // editor view draws the file's own bytes, which in that mode is not
    // what is on screen.
    match chat::ChatView::new(app) {
        Some(view) => view.render(regions.editor, cells),
        None => match app.rendering() {
            Some(rows) => {
                let top = app
                    .current_buffer()
                    .map_or(0, |buffer| buffer.viewport().top.get());
                reading::draw(cells, regions.editor, rows, top, app.theme());
            }
            None => editor::EditorView::new(app).render(regions.editor, cells),
        },
    }
    // Nothing open and nothing to open: the one moment a reader needs
    // telling what the keys are. Not while something has taken the region,
    // because then the region is not empty -- but a list or a question
    // leaves it alone, and this is what they would be over.
    if app.reading_nothing() && !layers.filling() {
        welcome::WelcomeView::new(app).render(regions.editor, cells);
    }

    // And then whatever is over it, furthest from the reader first, which
    // is the order `layers` declares and the reverse of the one a key is
    // offered in. One array holds both, so the thing drawn last is the
    // thing a key reaches.
    for layer in layers.furthest_first() {
        match layer {
            // The notes and the settings take the region for the same
            // reason: each is its own screen, and a list of what to come
            // back to with the code behind it would be two things on one
            // screen with no way to tell which a key would reach.
            Layer::Notes => {
                if let Some(view) = todo::TodoUi::new(app) {
                    view.render(regions.editor, cells);
                }
            }
            Layer::Settings => {
                if let Some(view) = settings::SettingsView::new(app) {
                    view.render(regions.editor, cells);
                }
            }
            // The counts take `area` rather than the region: they are the
            // one view that has the status row as well, which is what
            // `Room::Screen` says about them.
            Layer::Counts => {
                if let Some(view) = counts::CountsView::new(app) {
                    view.render(area, cells);
                }
            }
            Layer::Picker => {
                if let Some(list) = app.picker() {
                    list_over(cells, app, list, room_for_a_list(app, regions.editor));
                }
            }
            // Drawn by the status row, which is the row it is on.
            Layer::Prompt => {}
        }
    }
    // The agent's own commands, which are not a layer: the list follows
    // what is being typed in the box rather than being something the
    // reader opened, and it goes where any compact list goes. A picker
    // over the same conversation wins, because that one is a question the
    // agent is waiting on an answer to.
    if !layers.has(Layer::Picker)
        && let Some(list) = app.slash()
    {
        list_over(cells, app, list, room_for_a_list(app, regions.editor));
    }

    // The three panels that belong to a place in the file. Each is empty
    // while anything is over the file -- they are settled that way once a
    // frame -- so nothing here has to ask a second time.
    //
    // What could be typed next belongs beside the cursor, and the cursor is
    // on top of everything in the region.
    if let Some(panel) = complete::layout(app, regions.editor) {
        complete::draw(cells, panel, app);
    }
    // And what the call takes, which is the same kind of thing one question
    // further back. Never both: the panel's own accessor refuses to give a
    // signature while there is a list of candidates.
    if let Some(panel) = signature::layout(app, regions.editor) {
        signature::draw(cells, panel, app);
    }
    // And what the thing under the caret *is*, which is the question
    // furthest back of the three -- so it is drawn last and its own
    // accessor gives nothing while either of the others is up.
    if let Some(panel) = hover::layout(app, regions.editor) {
        hover::draw(cells, panel, app);
    }

    // The status row, last, and whose it is. A conversation puts its own
    // there while it is what the reader is looking at -- and the moment
    // anything is over it, the row belongs to that: its query, its question,
    // its filter. A row about the conversation underneath would be two
    // things asking to be read at once.
    //
    // Anything, not a list. It asked about a list, which was every case
    // there was while a conversation was itself a layer and only a list
    // could be over one; as a document the notes and the settings open over
    // it too, and each wants the row.
    if layers.taking_the_status_row() {
        return;
    }
    match chat::ChatView::new(app) {
        Some(view) if !layers.any() => view.status(cells, regions.status),
        _ => status::StatusView::new(app).render(regions.status, cells),
    }
}

/// The frames a mark that says something is happening turns through.
///
/// Braille, which needs no particular font: a terminal that cannot draw
/// these cannot draw the rest of obelus either, and this is the one thing
/// on screen that has to be legible without one. Ten frames at the ticker's
/// twelve a second is a turn a second and a bit.
const SPINNING: [char; 10] = [
    '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283c}', '\u{2834}', '\u{2826}', '\u{2827}',
    '\u{2807}', '\u{280f}',
];

/// Which frame of it the screen is on.
///
/// Here rather than in the conversation, because a conversation is no
/// longer the only place something turns: a list of open documents says
/// which of them an agent is working in, and a mark that only turned while
/// you were looking at that conversation would be a mark that never turned.
#[must_use]
pub fn spinning(phase: u32) -> char {
    SPINNING[phase as usize % SPINNING.len()]
}

/// Where a compact list goes, given what it is over.
///
/// The editor region, less what a conversation's box has taken from the
/// foot of it: a list drawn over the box would cover the thing the reader
/// is typing into to find the list.
fn room_for_a_list(app: &App, editor: Rect) -> Rect {
    app.chat()
        .map_or(editor, |chat| chat::above_writing(editor, chat))
}

/// Draws a list over whatever is behind it, with its edge and its preview.
///
/// One function for all of them, because a list opened over the code, over
/// the settings and over a conversation is the same list: what differs is
/// the room it is given, which is the argument.
fn list_over(cells: &mut CellBuffer, app: &App, list: &Picker, room: Rect) {
    let region = picker::region(list, room);
    picker::PickerView::new(list, app.theme(), app.phase()).render(region, cells);

    // A compact list sits on top of what is behind it, so it needs an edge:
    // the same rule the preview gets, for the same reason, which is that two
    // different things sharing a screen have to be told apart. A list
    // filling the whole room has no space above it and needs none.
    if region.y > room.y {
        rule(
            cells,
            Rect {
                y: region.y - 1,
                height: 1,
                ..region
            },
            app.theme(),
        );
    }

    // Below the list, with a rule between them. The preview is drawn by the
    // editor's own view, which is what makes it look like the editor.
    if let Some(preview) = picker::preview_region(Some(list), room) {
        rule(
            cells,
            Rect {
                y: preview.y - 1,
                height: 1,
                ..preview
            },
            app.theme(),
        );

        match app.preview() {
            Some(shown) => {
                editor::EditorView::for_buffer(
                    shown.buffer,
                    shown.highlights,
                    app.theme(),
                    shown.marked,
                    shown.changes,
                )
                .render(preview, cells);
            }
            // Room set aside and nothing to put in it: a file that has gone,
            // or a row that names no file.
            None => fill(cells, preview, Style::new().bg(app.theme().background)),
        }
    }

    // Last, because the card it can put up goes over everything this list is
    // showing -- the preview included, which is drawn after the rows and
    // would otherwise be drawn over the bottom half of it.
    picker::foot_of(cells, list, room, app.theme());
}

/// The block a bar is drawn with, track and thumb alike.
///
/// One glyph in two colours rather than a line and a block: a bar is a
/// surface with something sliding on it, and it is the *shade* that says
/// which part of it the reader is looking at.
const BAR: char = '\u{2588}';

/// One row of rule, saying that what is above it and what is below it are
/// different things.
///
/// Filled first: the row it goes on held code a moment ago, and a rule drawn
/// over the top of that would have the code showing between its cells.
///
/// It runs the whole width, joining nothing. A rule that closed itself off
/// against whatever was drawn beside it had to decide, per cell, whether
/// that neighbour was a control -- and the only thing it could ask was what
/// glyph the cell held, which a file's own text answers just as well as a
/// scrollbar does. A rule over one of this repository's golden grids grew a
/// tick everywhere the file had a bar under it. What the bar is drawn with
/// is what tells the two apart now: a block is a surface, and a surface does
/// not need a line to meet it.
pub(crate) fn rule(cells: &mut CellBuffer, area: Rect, theme: &Theme) {
    fill(cells, area, Style::new().bg(theme.background));
    for x in area.left()..area.right() {
        put(cells, x, area.y, '\u{2500}', Style::new().fg(theme.gutter));
    }
}

/// A bar down the right-hand edge of a region: where its window sits.
///
/// Shared by the editor and the lists, because it is the same question in
/// both -- how much of this is on screen, and which part -- and two
/// implementations would answer it in two shapes.
///
/// Drawn as a block in two shades: the track a shade off the page and the
/// thumb the brighter one. A line would be a line, and every rule that
/// crossed it would have to work out whether to join.
///
/// `total` is how many rows the whole thing has and `top` which of them is
/// on the first row.
///
/// Called only when there *is* somewhere to scroll -- a track with no thumb
/// on it is a control that does not work, and what is on screen being all
/// there is says itself. Whether there is somewhere is left to the caller
/// because only the caller can answer it: a list of rows fits when it has
/// fewer rows than the screen, while a file of wrapped lines can spill off
/// the bottom with a tenth of the screen's worth of lines in it.
///
/// The column stays reserved either way. Handing it back would change the
/// width of the text -- and with wrapping on, that means every line rewraps
/// when a file turns out to be one row too long.
pub(crate) fn scrollbar(
    cells: &mut CellBuffer,
    area: Rect,
    top: usize,
    total: usize,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let height = usize::from(area.height);
    let total = total.max(1);
    let x = area.right().saturating_sub(1);

    // At least one row of thumb, or a long list has a bar with nothing on it.
    let thumb = (height * height / total).clamp(1, height);
    let travel = height.saturating_sub(thumb);
    // Scaled by how far the *top* can travel, not by the total: dividing by
    // the total leaves the thumb short of the bottom exactly when the last
    // row is on screen, which is the one position anyone checks it against.
    let furthest = total.saturating_sub(height).max(1);
    let start = if total <= height {
        0
    } else {
        (top * travel).div_ceil(furthest).min(travel)
    };

    for row in 0..area.height {
        let inside = usize::from(row) >= start && usize::from(row) < start + thumb;
        let colour = if inside {
            theme.gutter_current
        } else {
            theme.scrollbar_track
        };
        put(cells, x, area.y + row, BAR, Style::new().fg(colour));
    }
}

/// Which row of a bar `area.height` rows tall a line of `total` falls on.
///
/// Shared by the bar and the change map beside it so that a change is level
/// with the part of the bar it belongs to; two roundings would put them a
/// row apart on tall files, which is exactly where anyone would notice.
/// What a row that folds something away carries: turned right for a run
/// that is closed, turned down for one that is open.
///
/// One pair for the three places that fold: a run of lines in a file, a run
/// of tool calls in the transcript, a commit's files in a list. They are
/// the same act -- one row standing in for several, and a key that opens it
/// -- and a reader who learns the mark in one place has learned it.
pub(crate) const FOLDED: char = '\u{25b8}';
pub(crate) const UNFOLDED: char = '\u{25be}';

/// Whichever of the two says how a row stands.
#[must_use]
pub(crate) const fn opens(open: bool) -> char {
    match open {
        true => UNFOLDED,
        false => FOLDED,
    }
}

pub(crate) fn bar_row(line: usize, total: usize, height: u16) -> u16 {
    let height = usize::from(height);
    let row = line * height / total.max(1);
    u16::try_from(row.min(height.saturating_sub(1))).unwrap_or(0)
}

/// Paints every cell of a region in one style, blanking whatever was there.
pub fn fill(cells: &mut CellBuffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = cells.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.set_style(style);
            }
        }
    }
}

/// Writes one character, and blanks the cells it covers beyond the first.
///
/// A wide glyph owns the cells after it, and they must hold no symbol at all:
/// the terminal advances two columns for the glyph, so anything left in the
/// second cell shifts the rest of the row. That is the rule in here that
/// breaks silently, which is why there is one copy of it.
///
/// The style is patched onto the cell rather than replacing it, so a caller
/// that only wants to set a foreground can pass one and keep whatever
/// background was painted underneath.
///
/// Returns how many columns were used, never zero: a character the terminal
/// does not advance over still advances this, or a caller stepping through a
/// string would not terminate.
pub fn put(cells: &mut CellBuffer, x: u16, y: u16, character: char, style: Style) -> u16 {
    let width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
    if let Some(cell) = cells.cell_mut((x, y)) {
        cell.set_char(character);
        cell.set_style(style);
    }
    for extra in 1..width {
        if let Some(cell) = cells.cell_mut((x + extra, y)) {
            cell.set_symbol("");
            cell.set_style(style);
        }
    }
    width.max(1)
}

/// Writes a string, returning the column after it.
pub fn write(cells: &mut CellBuffer, x: u16, y: u16, contents: &str, style: Style) -> u16 {
    let mut column = x;
    for character in contents.chars() {
        column = column.saturating_add(put(cells, column, y, character, style));
    }
    column
}

/// Which characters of a row are marked out.
///
/// Two shapes because the questions have two shapes: a fuzzy match lands on
/// scattered characters, while a substring match -- or a selection, which is
/// the same shape and gets the same treatment -- is one run. Every row in
/// obelus marks them the same way, which is what this is for: a row in a
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
    for (index, character) in contents.chars().enumerate().skip(marked.skip) {
        if column >= area.right() {
            break;
        }
        let index = u32::try_from(index).unwrap_or(u32::MAX);
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
        let style = match marked.matched.covers(index) {
            true => style.bg(marked.mark),
            false => style,
        };
        column = column.saturating_add(put(cells, column, y, character, style));
    }
    column
}

/// A row of tabs, and the arrows that say how to change them.
///
/// The one that is showing gets the selected row's background, which is the
/// same thing that marks a selected row: on any of obelus's screens, that
/// background means "this is the one you are on". Returns the column after
/// the last tab.
///
/// The arrows are not a hint that can go stale: the keys are the arrows, and
/// there is nowhere to rebind them to.
pub fn tabs<Name>(
    cells: &mut CellBuffer,
    area: Rect,
    names: &[Name],
    current: usize,
    theme: &Theme,
) -> u16
where
    Name: AsRef<str>,
{
    let dim = Style::new().fg(theme.gutter).bg(theme.background);
    fill(cells, Rect { height: 1, ..area }, dim);

    let mut column = area.x + 1;
    for (index, name) in names.iter().enumerate() {
        let style = match index == current {
            true => Style::new()
                .fg(theme.foreground)
                .bg(theme.selected_row_background),
            false => dim,
        };
        column = write_marked(
            cells,
            area,
            column,
            area.y,
            &format!(" {} ", name.as_ref()),
            style,
            &Marked::plain(),
        );
    }

    // The keys that walk them, drawn where they are walked. The tab
    // arrows rather than the left and right ones: those are the caret's
    // now, and a hint that names the wrong key is worse than none.
    let keys = "\u{21e4} \u{21e5}";
    if let Ok(offset) = u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
        && area.x + offset > column
    {
        write(cells, area.x + offset, area.y, keys, dim);
    }
    column
}

/// One key, and what it does here.
///
/// The word is optional because some keys are their own explanation. The
/// arrows walk the tabs and there is nothing to add to an arrow; `alt+f`
/// means nothing at all until something says "fold".
#[derive(Clone, Copy, Debug)]
pub struct Hint {
    /// The key, spelled by the key table so that a reader who rebound it
    /// sees what they bound.
    pub chord: crate::keymap::KeyChord,
    /// A second key that does the same thing, for a pair that shares a word:
    /// `alt+up` and `alt+down` are one act in two directions, and two rows
    /// saying "move it up" and "move it down" is the same sentence twice.
    pub and_also: Option<crate::keymap::KeyChord>,
    /// What it does, in the one word the foot has room for.
    pub does: Option<&'static str>,
    /// How the key is written, where the chord does not say it.
    ///
    /// For the thing a page does that is not one key: a list narrowed by
    /// typing at it answers to every letter, and naming one of them would
    /// read as "press this one".
    pub spelled: Option<&'static str>,
    /// The same thing said properly, for the card, which has room for it.
    ///
    /// `None` where the word is the whole of it. Two forms rather than one
    /// because the two places are not the same place: a foot is a row shared
    /// by everything, and a card is a page about one thing.
    pub said: Option<&'static str>,
    /// Whether it goes at the foot, or waits in the list of them all.
    ///
    /// The foot is one row over the reader's work, so what goes there is
    /// what they reach for without thinking. Everything else is a keypress
    /// away and is not lost -- it is in the card, where there is room to say
    /// what it does in words rather than in one.
    pub common: bool,
    /// Which way it is set, for a key whose whole job is a switch.
    ///
    /// The word says what the key switches; this says what it is switched
    /// to. Without it a switch on a key is a coin toss: the reader presses
    /// it to find out which way it was, which is the one thing a switch
    /// must never make them do.
    ///
    /// `None` for a key that does something rather than sets something.
    pub switched: Option<bool>,
    /// Whether it does anything *now*.
    ///
    /// Worked out per frame by whoever knows: a note about the project has
    /// nowhere to go, so there is no "go there" on the foot while the
    /// selection is on one. The foot draws what can be pressed; the card
    /// draws all of them and greys this one out, so what a reader learns is
    /// that the view has eight keys rather than that its keys come and go.
    pub usable: bool,
}

impl Hint {
    /// A key that goes at the foot.
    #[must_use]
    pub const fn common(chord: crate::keymap::KeyChord, does: &'static str) -> Self {
        Self {
            chord,
            and_also: None,
            does: Some(does),
            spelled: None,
            said: None,
            switched: None,
            common: true,
            usable: true,
        }
    }

    /// One that waits in the card.
    #[must_use]
    pub const fn rare(chord: crate::keymap::KeyChord, does: &'static str) -> Self {
        Self {
            common: false,
            ..Self::common(chord, does)
        }
    }

    /// How to write the key, where the chord is not what a reader presses.
    #[must_use]
    pub const fn written(mut self, spelled: &'static str) -> Self {
        self.spelled = Some(spelled);
        self
    }

    /// What it does, at length, for the card.
    #[must_use]
    pub const fn saying(mut self, said: &'static str) -> Self {
        self.said = Some(said);
        self
    }

    /// The same act in the other direction, on a key of its own.
    #[must_use]
    pub const fn or(mut self, chord: crate::keymap::KeyChord) -> Self {
        self.and_also = Some(chord);
        self
    }

    /// Says whether it does anything at the moment.
    #[must_use]
    pub const fn when(mut self, usable: bool) -> Self {
        self.usable = usable;
        self
    }

    /// Says it is a switch, and which way it is set.
    #[must_use]
    pub const fn set(mut self, on: bool) -> Self {
        self.switched = Some(on);
        self
    }

    /// How it is written: the key, or the pair of them.
    #[must_use]
    pub fn keys(self) -> String {
        if let Some(spelled) = self.spelled {
            return spelled.to_string();
        }
        match self.and_also {
            Some(also) => format!("{} {}", self.chord.label(), also.label()),
            None => self.chord.label(),
        }
    }
}

/// The key that opens the list of what the keys here are.
///
/// `f1`, which has meant help for longer than any of this. It reaches no
/// command from inside a view -- [`crate::keymap::Context::Dialog`] binds
/// nothing, and that is the point -- and it is not a character, so it works
/// even in a view that takes every character the reader types.
#[must_use]
pub fn keys_chord() -> crate::keymap::KeyChord {
    crate::keymap::KeyChord::new(
        crossterm::event::KeyCode::F(1),
        crossterm::event::KeyModifiers::NONE,
    )
}

/// How wide a switch's track is, in cells.
///
/// Four: two for the knob and two for the room it slides into. Anything
/// narrower stops looking like something that slides.
pub const TRACK_WIDTH: u16 = 4;

/// A switch, drawn where it is asked for, and the column after it.
///
/// A square knob at one end of a short track. The shape says which way it
/// is without a word to read, and it says it the same way wherever a switch
/// appears: this is the settings page's control, so a reader who has seen
/// one there knows what one at the foot of a list is saying.
///
/// Squares rather than full blocks: a full block fills its cell's whole
/// height, and two of them one above the other read as one tall bar.
pub fn switch(cells: &mut CellBuffer, x: u16, y: u16, on: bool, ink: Color, theme: &Theme) -> u16 {
    fill(
        cells,
        Rect {
            x,
            y,
            width: TRACK_WIDTH,
            height: 1,
        },
        Style::new().bg(theme.control_background),
    );
    // Bright when on and dim when off, rather than a colour: the knob's
    // *position* already says which way it is, so a hue would be a second
    // answer to a question already answered.
    let (at, colour) = match on {
        true => (x + TRACK_WIDTH / 2, ink),
        false => (x, theme.gutter),
    };
    for cell in 0..TRACK_WIDTH / 2 {
        put(
            cells,
            at + cell,
            y,
            '\u{25a0}',
            Style::new().fg(colour).bg(theme.control_background),
        );
    }
    x + TRACK_WIDTH
}

/// How many rows a view gives up to its foot, where it has one.
pub const FOOT_ROWS: u16 = 2;

/// What is left of a region once its foot is taken off the bottom.
///
/// One answer, asked by the drawing and by whatever moves about inside: a
/// page is worth what is on screen, and two answers to how much that is
/// would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn footed(area: Rect, hints: &[Hint]) -> Rect {
    if hints.is_empty() || area.height <= FOOT_ROWS {
        return area;
    }
    Rect {
        height: area.height - FOOT_ROWS,
        ..area
    }
}

/// The keys a view answers to, along the bottom of it under a rule.
///
/// The common ones that can be pressed at the moment, and `f1` at the
/// right-hand end saying there are more. At the foot rather than beside a
/// title, because a key needs a word and words need room.
pub fn foot(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    if hints.is_empty() || area.height < FOOT_ROWS {
        return;
    }
    let top = area.y + area.height - FOOT_ROWS;
    rule(
        cells,
        Rect {
            y: top,
            height: 1,
            ..area
        },
        theme,
    );
    let y = top + 1;
    fill(
        cells,
        Rect {
            y,
            height: 1,
            ..area
        },
        Style::new().bg(theme.background),
    );

    // The one at the end first, because it is the one that must not be given
    // up: a foot that ran out of room and dropped the way to the rest of the
    // keys would be a foot that hides the thing it exists to point at.
    let all = format!("{} keys", keys_chord().label());
    let width = u16::try_from(text_width(&all)).unwrap_or(0);
    let edge = match area.width.checked_sub(width + 2) {
        Some(offset) => {
            write(
                cells,
                area.x + offset,
                y,
                &all,
                Style::new().fg(theme.gutter).bg(theme.background),
            );
            area.x + offset
        }
        None => area.x + area.width,
    };

    let mut x = area.x + 2;
    for hint in hints.iter().filter(|hint| hint.common && hint.usable) {
        let keys = hint.keys();
        let switched = hint.switched.map_or(0, |_| usize::from(TRACK_WIDTH) + 1);
        let wanted =
            text_width(&keys) + hint.does.map_or(0, |does| text_width(does) + 1) + switched + 3;
        let Ok(wanted) = u16::try_from(wanted) else {
            return;
        };
        if x + wanted > edge {
            return;
        }
        // The key brighter than the word: what a reader is looking for down
        // here is which key, and the word is read once to find out that it
        // is the one.
        x = write(
            cells,
            x,
            y,
            &keys,
            Style::new().fg(theme.gutter_current).bg(theme.background),
        );
        if let Some(does) = hint.does {
            x = write(
                cells,
                x + 1,
                y,
                does,
                Style::new().fg(theme.gutter).bg(theme.background),
            );
        }
        if let Some(on) = hint.switched {
            x = switch(cells, x + 1, y, on, theme.foreground, theme);
        }
        x += 3;
    }
}

/// Every key a view answers to, on a card over it.
///
/// All of them, with what cannot be pressed at the moment greyed rather than
/// left out: what a reader should come away with is that this view has these
/// keys, not that its keys come and go. There is room here for a sentence,
/// which is why the words can be words rather than the one the foot fits.
pub fn keys_card(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    if hints.is_empty() {
        return;
    }
    let column = u16::try_from(
        hints
            .iter()
            .map(|hint| text_width(&hint.keys()))
            .max()
            .unwrap_or(0),
    )
    .unwrap_or(0)
    .saturating_add(2);
    let widest = u16::try_from(
        hints
            .iter()
            .map(|hint| {
                hint.said.or(hint.does).map_or(0, text_width)
                    + hint.switched.map_or(0, |_| usize::from(TRACK_WIDTH) + 1)
            })
            .max()
            .unwrap_or(0),
    )
    .unwrap_or(0);
    // The edges, a margin inside them, the keys and what they do.
    let width = column
        .saturating_add(widest)
        .saturating_add(4)
        .min(area.width);
    // The edges, the title, a blank under it, and a row per key.
    let height = u16::try_from(hints.len())
        .unwrap_or(u16::MAX)
        .saturating_add(4)
        .min(area.height);
    if width < 4 || height < 4 {
        return;
    }
    let card = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };

    let ground = Style::new()
        .fg(theme.foreground)
        .bg(theme.raised_background);
    fill(cells, card, ground);
    // An edge, because this sits over a list it is not part of: a raised
    // ground alone reads as the list having changed colour.
    let edge = Style::new().fg(theme.gutter).bg(theme.raised_background);
    let last = card.width - 1;
    let foot = card.height - 1;
    for (x, glyph) in [(0, '\u{256d}'), (last, '\u{256e}')] {
        put(cells, card.x + x, card.y, glyph, edge);
    }
    for (x, glyph) in [(0, '\u{2570}'), (last, '\u{256f}')] {
        put(cells, card.x + x, card.y + foot, glyph, edge);
    }
    for x in 1..last {
        put(cells, card.x + x, card.y, '\u{2500}', edge);
        put(cells, card.x + x, card.y + foot, '\u{2500}', edge);
    }
    for y in 1..foot {
        put(cells, card.x, card.y + y, '\u{2502}', edge);
        put(cells, card.x + last, card.y + y, '\u{2502}', edge);
    }

    write(
        cells,
        card.x + 2,
        card.y + 1,
        "the keys here",
        Style::new()
            .fg(theme.status_foreground)
            .bg(theme.raised_background),
    );
    let off = Style::new().fg(theme.gutter).bg(theme.raised_background);
    for (at, hint) in hints.iter().enumerate() {
        let Ok(offset) = u16::try_from(at) else { break };
        let y = card.y + 3 + offset;
        if y >= card.y + foot {
            break;
        }
        let style = match hint.usable {
            true => ground,
            false => off,
        };
        write(cells, card.x + 2, y, &hint.keys(), style);
        let mut x = card.x + 2 + column;
        if let Some(does) = hint.said.or(hint.does) {
            x = write(cells, x, y, does, style);
        }
        // After the words, because what it is set to is worth reading second:
        // a reader coming to the card is finding out what the key *is*.
        if let Some(on) = hint.switched {
            let ink = match hint.usable {
                true => theme.foreground,
                false => theme.gutter,
            };
            switch(cells, x + 1, y, on, ink, theme);
        }
    }
}

/// What a list says when it has nothing in it.
///
/// One place, so that every empty list in obelus says its own reason in the
/// same voice and the same colour. What the reason *is* belongs to whoever
/// knows it -- the application for a list of files, the component for a
/// filtered one.
pub fn nothing(cells: &mut CellBuffer, area: Rect, reason: &str, theme: &Theme) {
    write(
        cells,
        area.x + 1,
        area.y,
        reason,
        Style::new().fg(theme.gutter).bg(theme.background),
    );
}

/// How many cells a string occupies.
#[must_use]
pub fn text_width(contents: &str) -> usize {
    contents
        .chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

/// How many leading characters to drop so the rest of `contents` fits in
/// `cells`, with one cell left for the ellipsis that marks the cut.
///
/// From the left, because the end is the part worth reading: the file name in
/// a path, the last component of a symbol. The leading directories are the
/// part already known. Measured in cells rather than characters so a path with
/// wide glyphs in it does not overrun whatever comes after it.
///
/// Zero when it already fits. Everything when there is no room even for the
/// ellipsis, so the caller can draw nothing rather than a lone `…`.
#[must_use]
pub fn drop_from_left(contents: &str, cells: usize) -> usize {
    if text_width(contents) <= cells {
        return 0;
    }
    let total = contents.chars().count();
    if cells <= 1 {
        return total;
    }

    let budget = cells - 1;
    let mut kept = 0usize;
    let mut width = 0usize;
    for character in contents.chars().rev() {
        let character_width = character.width().unwrap_or(0);
        if width + character_width > budget {
            break;
        }
        width += character_width;
        kept += 1;
    }
    total - kept
}

/// How many trailing characters to drop so the rest of `contents` fits in
/// `cells`, with one cell left for the ellipsis that marks the cut.
///
/// The mirror of [`drop_from_left`], for a sentence rather than a name. A
/// name is told from its fellows at the end -- the file, the last component
/// of a symbol -- and a sentence at the beginning: a commit subject cut to
/// its last few words has lost the half that said which commit it was.
///
/// Measured in cells for the same reason, which matters more here: a subject
/// written in Chinese is one character to two columns, and counting
/// characters would cut it at half the row.
///
/// Zero when it already fits. Everything when there is no room even for the
/// ellipsis, so the caller can draw nothing rather than a lone `…`.
#[must_use]
pub fn drop_from_right(contents: &str, cells: usize) -> usize {
    if text_width(contents) <= cells {
        return 0;
    }
    let total = contents.chars().count();
    if cells <= 1 {
        return total;
    }

    let budget = cells - 1;
    let mut kept = 0usize;
    let mut width = 0usize;
    for character in contents.chars() {
        let character_width = character.width().unwrap_or(0);
        if width + character_width > budget {
            break;
        }
        width += character_width;
        kept += 1;
    }
    total - kept
}

/// `contents` with its tail replaced by an ellipsis if it does not fit.
///
/// For a sentence, where the beginning is the part worth keeping. The three
/// places that wanted this had each grown their own: one measured in cells
/// and one in characters, so the same prose cut to the same width came out
/// two different lengths depending on which screen it was on -- and the one
/// counting characters cut a Chinese sentence at half the room it was given.
#[must_use]
pub fn truncate_from_right(contents: &str, cells: usize) -> String {
    let dropped = drop_from_right(contents, cells);
    if dropped == 0 {
        return contents.to_string();
    }
    let total = contents.chars().count();
    if dropped >= total {
        return String::new();
    }
    let mut result: String = contents.chars().take(total - dropped).collect();
    result.push('\u{2026}');
    result
}

/// `contents` with its head replaced by an ellipsis if it does not fit.
#[must_use]
pub fn truncate_from_left(contents: &str, cells: usize) -> String {
    let dropped = drop_from_left(contents, cells);
    if dropped == 0 {
        return contents.to_string();
    }
    let total = contents.chars().count();
    if dropped >= total {
        return String::new();
    }
    let mut result = String::from('\u{2026}');
    result.extend(contents.chars().skip(dropped));
    result
}

#[cfg(test)]
mod tests {
    use super::{drop_from_left, drop_from_right, truncate_from_left, truncate_from_right};

    #[test]
    fn a_path_that_fits_is_left_alone() {
        assert_eq!(drop_from_left("src/app.rs", 20), 0);
        assert_eq!(truncate_from_left("src/app.rs", 20), "src/app.rs");
    }

    /// The file name survives; the directories above it are what goes.
    #[test]
    fn the_end_survives_the_cut() {
        let truncated = truncate_from_left("a/very/deep/path/to/app.rs", 12);
        assert_eq!(truncated, "\u{2026}h/to/app.rs");
        assert_eq!(truncated.chars().count(), 12);
    }

    /// Cells, not characters: a wide glyph costs two, and counting characters
    /// would overrun whatever is drawn after the text.
    #[test]
    fn wide_glyphs_are_counted_by_the_cells_they_take() {
        // Five wide glyphs are ten cells. Six cells hold the ellipsis and two
        // glyphs; a third would need a seventh cell.
        let glyphs = "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}";
        assert_eq!(truncate_from_left(glyphs, 6), "\u{2026}\u{754c}\u{554a}");
        assert_eq!(super::text_width(&truncate_from_left(glyphs, 6)), 5);
    }

    /// The property the rest of the layout depends on: whatever comes back
    /// fits. Anything wider would be drawn over the row's other columns.
    #[test]
    fn the_result_never_exceeds_the_room_it_was_given() {
        let samples = [
            "src/app.rs",
            "a/very/deep/path/to/somewhere/app.rs",
            "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}/mixed/\u{8def}\u{5f84}.rs",
            "\tindented",
            "",
        ];
        for contents in samples {
            for cells in 0..40usize {
                let width = super::text_width(&truncate_from_left(contents, cells));
                assert!(
                    width <= cells || width == 0,
                    "{contents:?} at {cells} cells came back {width} wide"
                );
            }
        }
    }

    /// No room even for the ellipsis, so the caller can draw nothing rather
    /// than a lone `\u{2026}` that says only that something was hidden.
    #[test]
    fn nothing_fits_in_one_cell() {
        assert_eq!(drop_from_left("src/app.rs", 1), 10);
        assert_eq!(truncate_from_left("src/app.rs", 1), "");
        assert_eq!(drop_from_right("src/app.rs", 1), 10);
    }

    /// A sentence keeps its beginning, which is the half that says which
    /// sentence it is.
    #[test]
    fn a_sentence_that_fits_is_left_alone() {
        assert_eq!(drop_from_right("Let a page reach the end", 30), 0);
    }

    /// One cell of what fits goes to the mark, the way it does from the left.
    #[test]
    fn the_beginning_survives_the_cut() {
        let subject = "Stop and ask, instead of counting presses";
        let dropped = drop_from_right(subject, 12);
        let kept: String = subject
            .chars()
            .take(subject.chars().count() - dropped)
            .collect();
        assert_eq!(kept, "Stop and as");
        assert_eq!(super::text_width(&kept) + 1, 12);
    }

    /// Cells here too, and it matters more: a subject written in Chinese is
    /// one character to two columns, so counting characters would cut it at
    /// half the row it was given.
    #[test]
    fn a_wide_sentence_is_counted_by_the_cells_it_takes() {
        let subject = "\u{4fee}\u{590d}\u{4e00}\u{4e2a}\u{95ee}\u{9898}";
        let dropped = drop_from_right(subject, 7);
        let kept: String = subject
            .chars()
            .take(subject.chars().count() - dropped)
            .collect();
        // Six cells for three glyphs, and the seventh for the mark.
        assert_eq!(kept, "\u{4fee}\u{590d}\u{4e00}");
        assert_eq!(super::text_width(&kept), 6);
    }

    /// The string form, and the one cell the mark takes.
    #[test]
    fn a_sentence_comes_back_with_its_tail_marked() {
        assert_eq!(truncate_from_right("Stop and ask", 30), "Stop and ask");
        assert_eq!(
            truncate_from_right("Stop and ask, instead of counting", 12),
            "Stop and as\u{2026}"
        );
        assert_eq!(
            super::text_width(&truncate_from_right("Stop and ask, instead", 12)),
            12
        );
    }

    /// Nothing rather than a lone `\u{2026}`, which is what the other
    /// direction does and says only that something was hidden. The two
    /// helpers this replaced both drew the mark alone here.
    #[test]
    fn no_room_for_the_mark_means_no_mark() {
        assert_eq!(truncate_from_right("Stop and ask", 1), "");
        assert_eq!(truncate_from_right("Stop and ask", 0), "");
    }

    /// The same property the other direction has to hold: what is kept, plus
    /// the cell the mark takes, fits in the room it was given.
    #[test]
    fn what_is_kept_from_the_left_never_exceeds_the_room() {
        let samples = [
            "Stop and ask, instead of counting presses",
            "\u{4fee}\u{590d}\u{4e00}\u{4e2a}\u{95ee}\u{9898}",
            "mixed \u{4e2d}\u{6587} and latin",
            "\tindented",
            "",
        ];
        for contents in samples {
            let total = contents.chars().count();
            for cells in 0..40usize {
                let dropped = drop_from_right(contents, cells);
                if dropped >= total {
                    continue;
                }
                let kept: String = contents.chars().take(total - dropped).collect();
                let width = super::text_width(&kept) + usize::from(dropped > 0);
                assert!(
                    width <= cells,
                    "{contents:?} at {cells} cells kept {width} cells' worth"
                );
                let written = super::text_width(&truncate_from_right(contents, cells));
                assert!(
                    written <= cells,
                    "{contents:?} at {cells} cells came back {written} wide"
                );
            }
        }
    }
}
