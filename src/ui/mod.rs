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
pub mod image;
pub mod picker;
pub mod reading;
pub mod settings;
pub mod status;
pub mod welcome;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    widgets::Widget as _,
};
use unicode_width::UnicodeWidthChar as _;

use crate::{app::App, component::picker::Colouring, theme::Theme};

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

    if let Some(prompt) = app.prompt() {
        let column = status::answer_caret(prompt);
        return (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        });
    }

    if let Some(picker) = app.picker() {
        let column = status::prompt_caret(picker);
        return (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        });
    }

    // The conversation is written into, and its caret is in the box rather
    // than on the status bar: a message is a paragraph, and a paragraph
    // does not fit on one row. After the picker, because an agent's own
    // question is a list opened over it.
    if let Some(chat) = app.chat() {
        return chat::ChatView::caret(regions.editor, chat, app.card());
    }

    // The settings filter by typing too, so the caret goes where the typing
    // does. After the picker, because a list opened over them is what the
    // reader is typing into.
    if let Some(settings) = app.settings() {
        let column = status::filter_caret(settings.query());
        return (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        });
    }

    // Nothing is typed into the counts, so there is no caret in them: what
    // marks where the keys are going is the row's background, and a caret as
    // well would be two marks for one fact. Without this the file behind
    // them kept its own, blinking in a view it is not part of.
    if app.counts().is_some() {
        return None;
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
    // Under whatever the region holds and over the status bar, once, for
    // every view: what is above it changes and the boundary does not.
    rule(cells, regions.edge, app.theme());
    // A buffer being shown some other way is shown that way. The editor view
    // draws the file's own bytes, which in this mode is not what is on
    // screen.
    match app.rendering() {
        Some(rows) => {
            let top = app
                .current_buffer()
                .map_or(0, |buffer| buffer.viewport().top.get());
            reading::draw(cells, regions.editor, rows, top, app.theme());
        }
        None => editor::EditorView::new(app).render(regions.editor, cells),
    }
    // The conversation takes the whole region for the same reason the
    // settings do: it is its own screen with its own typing, and the file
    // behind it is not what is being read.
    if let Some(view) = chat::ChatView::new(app) {
        // The conversation's status row is obelus's own: one bar, at the
        // foot of the screen. Unless a list is open over it, in which case
        // the row is that list's prompt -- the keys are going there and so
        // is the caret, and a row about the conversation under a list
        // nobody is typing in is two things asking to be read at once.
        match app.picker() {
            Some(_) => status::StatusView::new(app).render(regions.status, cells),
            None => view.status(cells, regions.status),
        }
        view.render(regions.editor, cells);
        // A list opened over it is the agent's own question, or the
        // commands it takes: both draw where any compact list draws, with
        // the conversation behind them.
        let over = app.picker().or_else(|| app.slash());
        if let Some(list) = over {
            let room = app.chat().map_or(regions.editor, |chat| {
                chat::above_writing(regions.editor, chat)
            });
            let region = picker::region(list, room);
            picker::PickerView::new(list, app.theme()).render(region, cells);
            if region.y > regions.editor.y {
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
        }
        return;
    }
    // The counts take the whole region: a table of numbers with the file
    // behind it would be a screen with two things on it and no way to tell
    // which one a key would reach.
    // The counts take the screen whole -- the status row included, and the
    // rule that would be above one. A status row says what is being read and
    // where the cursor is in it; while this is showing there is no file being
    // read and no cursor anywhere, so obelus's own row could only name the
    // file behind the view, at a line and column belonging to a cursor that
    // is nowhere on screen. The two rows that would have said
    // it go to the list instead.
    if let Some(view) = counts::CountsView::new(app) {
        view.render(area, cells);
        return;
    }
    // The settings take the whole region: they are their own screen, with
    // their own typing, and nothing under them is being read.
    if let Some(view) = settings::SettingsView::new(app) {
        view.render(regions.editor, cells);
        // A list opened over them is a setting's choices: it draws where any
        // compact list draws, and the settings are what is behind it.
        if let Some(list) = app.picker() {
            let region = picker::region(list, regions.editor);
            picker::PickerView::new(list, app.theme()).render(region, cells);
            if region.y > regions.editor.y {
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
        }
        status::StatusView::new(app).render(regions.status, cells);
        return;
    }
    // Nothing open: the editor region has been painted and is otherwise
    // empty, which is the one moment a reader needs telling what the keys are.
    if app.current_buffer().is_none() {
        welcome::WelcomeView::new(app).render(regions.editor, cells);
    }
    // Over the code, because a compact list is meant to leave the code above
    // it visible.
    if let Some(list) = app.picker() {
        let region = picker::region(list, regions.editor);
        picker::PickerView::new(list, app.theme()).render(region, cells);

        // A compact list sits on top of the code, so it needs an edge: the
        // same rule the preview gets, for the same reason, which is that two
        // different things sharing a screen have to be told apart. A list
        // filling the whole region has no room above it and needs none.
        if region.y > regions.editor.y {
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

        // Below the list, with a rule between them. The preview is drawn by
        // the editor's own view, which is what makes it look like the editor.
        if let Some(preview) = picker::preview_region(app.picker(), regions.editor) {
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
                // Room set aside and nothing to put in it: a file that has
                // gone, or a row that names no file.
                None => fill(cells, preview, Style::new().bg(app.theme().background)),
            }
        }
    }
    // Over the code and over everything else in the region: what could be
    // typed next belongs beside the cursor, and the cursor is on top.
    if let Some(panel) = complete::layout(app, regions.editor) {
        complete::draw(cells, panel, app);
    }
    status::StatusView::new(app).render(regions.status, cells);
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

/// Which characters of a row matched what the reader typed.
///
/// Two shapes because the matching has two shapes: a fuzzy match lands on
/// scattered characters, and a substring match is one run. Every list in
/// obelus marks them the same way, which is what this is for -- a row in a
/// narrowed list has to say why it is in it.
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
}

/// Writes a row's text, marking what matched and colouring what the file
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

    let keys = "\u{2190} \u{2192}";
    if let Ok(offset) = u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
        && area.x + offset > column
    {
        write(cells, area.x + offset, area.y, keys, dim);
    }
    column
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
