//! A frame: where each region is, and what is drawn in it in what order.

use super::*;

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
    // A rule between the two, which is what every other boundary in Obelus
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

/// The room the document being read actually has: the editor region, less
/// whatever is drawn over its foot.
///
/// A list that sits on the status bar is drawn *over* the editor, so the
/// editor drew rows nobody could see. Every measurement of a screenful was
/// then a measurement of a screen that was partly a list: the caret could
/// be scrolled to a row behind it, paging went a listful too far, and a
/// list showing its selection in the file put it where the list was.
///
/// One function, for the reason [`regions`] is one: what the drawing
/// measures and what the scrolling measures cannot be allowed to disagree,
/// and two subtractions in two places is how they come to.
///
/// Not for a conversation. A conversation puts a list *inside* itself,
/// above the box a message is written in, so it has already made the room
/// -- and shortening it here would put the list under its own box.
#[must_use]
pub fn editor_room(area: Rect, app: &impl Screen) -> Rect {
    let editor = regions(area).editor;
    let Some(list) = app
        .picker()
        .filter(|list| list.layout() != obelus_component::picker::PickerLayout::FullArea)
    else {
        return editor;
    };
    // Nor a terminal: a list over it is over it, and its program is not
    // told it is smaller for as long as the palette is open -- which would
    // make a shell draw its prompt again under a list the reader is about
    // to close.
    match app.shown() {
        Shown::Chat(_) | Shown::Terminal(_) => return editor,
        Shown::File(_) | Shown::Reading(_) | Shown::Notes(_) | Shown::Nothing => {}
    }
    picker::room_above(list, editor)
}

/// The rows a document is *painted* on, which is not the room it is read
/// in.
///
/// Two counts, and they differ for exactly one reason: a compact list is
/// drawn *over* the document. The reader's room stops above it, which is
/// what [`editor_room`] says and what the scrolling, the paging, the
/// preview's placement and the caret are all measured in -- so the line a
/// selection is about never lands under the list. The painting does not
/// stop there, because what a list is laid over has to be *there* for it
/// to be laid over: a pane in a window shows what is behind it, and cells
/// nobody wrote are not a page seen through glass, they are a hole.
///
/// So this is the whole region, always. The list is drawn afterwards and
/// covers what it covers, which in a terminal leaves the same cells as
/// before -- the difference is only visible where the front end can see
/// through.
#[must_use]
pub fn editor_canvas(area: Rect) -> Rect {
    regions(area).editor
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
/// A reader spends its time inside one project, and the leading directories
/// of that project are the part already known. Shared, because more than one
/// view writes a path now: the status bar says which file is open, and a
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
pub fn cursor_position(area: Rect, app: &impl Screen) -> Option<Position> {
    let regions = regions(area);
    // Most of them filter or answer by typing on the status row, so the
    // caret goes where that typing does.
    let on_the_status_row = |column: u16| {
        (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        })
    };

    // Being asked which project, which is not a layer -- it is a page of
    // its own, and the page is what this row is under. Before the layers
    // for that reason rather than for an order among them -- and so only
    // where no layer is up: what went wrong on the way up is a list put
    // over this page, and while it is there the keys are its.
    if app.layers().nearest().is_none()
        && let Some(choosing) = app.choosing()
    {
        let question = status::choosing_question(choosing.naming);
        // A path box is opened in order to type, so it has a caret from
        // the first frame -- the rule a picker follows. The filter is not:
        // the rows are what the reader came for and the filter is what
        // they reach for second, so an empty one has no caret, which is
        // the settings' answer and for the settings' reason. What says
        // where the keys are going is the row the reader is on, and two
        // marks for one fact is one too many.
        if !choosing.naming && choosing.typed.is_empty() {
            return None;
        }
        return on_the_status_row(status::typed_caret(
            Some(question),
            &choosing.typed,
            choosing.caret,
        ));
    }

    // Whatever is nearest, which is where the keys are going. Asked once
    // rather than walked as a chain of its own: a caret drawn in one view
    // while the typing reaches another is a screen that lies about what a
    // key will do, and that is what two chains in two orders produced.
    match app.layers().nearest() {
        Some(Layer::Prompt) => return on_the_status_row(status::answer_caret(app.prompt()?)),
        // None in a list that is only read, which is not typed into.
        Some(Layer::Picker) => {
            let picker = app.picker()?;
            if picker.is_only_read() {
                return None;
            }
            return on_the_status_row(status::prompt_caret(picker));
        }
        // The same shape a picker's query has, because it is the same
        // thing: what has been typed narrows what is above it.
        Some(Layer::Names) => {
            let names = app.names()?;
            return on_the_status_row(status::filter_caret(
                &names.query().said(),
                names.query().caret().get(),
            ));
        }
        Some(Layer::Settings) => {
            let settings = app.settings()?;
            let said = settings.query();
            // A filter nobody has typed into has no caret, the same answer
            // the counts give and for the same reason: what says where the
            // keys are going is the row the reader is on, and a caret sat
            // in an empty box is a second mark for one fact. The row keeps
            // its prompt either way, so there is still somewhere visibly
            // waiting to be typed into.
            //
            // Not the rule a picker follows, because a picker is opened in
            // order to type -- this page is opened in order to walk it,
            // and the filter is the thing a reader reaches for second.
            //
            // It is also what keeps the agents page still. A terminal is
            // handed a picture by writing it at the caret, so a frame that
            // moves the marks has to put the caret out and bring it back
            // -- around a write the terminal takes milliseconds to chew
            // through, which is a caret blinking once per scrolled row.
            // No caret, nothing to put out.
            if said.is_empty() {
                return None;
            }
            return on_the_status_row(status::filter_caret(&said, settings.query_caret()));
        }
        // A note being written has one in the row it is being written in,
        // which is the row it will be read in. The same answer the
        // conversation gives, for the same reason: what is typed is a
        // paragraph, and a paragraph does not fit on the status bar.
        // Nothing is typed into the counts, so there is no caret in them:
        // what marks where the keys are going is the row's background, and
        // a caret as well would be two marks for one fact. Without this the
        // file behind them kept its own, blinking in a view it is not part
        // of.
        Some(Layer::Counts) => return None,
        // Nor into the page saying the project has gone, which is answered
        // with a key and has no box.
        Some(Layer::Gone) => return None,
        // Nothing over the document, so the caret is the document's own.
        None => {
            // A conversation is written into, and its caret is in the box
            // rather than on the status bar: a message is a paragraph, and
            // a paragraph does not fit on one row.
            match app.shown() {
                Shown::Chat(chat) => {
                    return chat::ChatView::caret(regions.editor, chat, app.card());
                }
                // And the notes, which are written into the same way.
                Shown::Notes(notes) => return todo::caret(regions.editor, notes),
                // And a terminal, whose caret is its program's cursor.
                Shown::Terminal(terminal) => return terminal::caret(regions.editor, terminal),
                Shown::File(_) | Shown::Reading(_) | Shown::Nothing => {}
            }
        }
    }

    let buffer = app.current_buffer()?;
    // No cursor over a rendering. The rows are not the file's lines, so
    // there is nowhere in them the cursor honestly is.
    if buffer.mode() != obelus_buffer::Mode::Edit {
        return None;
    }
    // Everything the editor draws before the text: the change margin, the
    // gutter and the fold marks, from the function the editor lays them out
    // with.
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::changed(app.changes()),
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
/// cells Obelus wrote. Going through a `Frame` would mean reading them back
/// from the backend afterwards, and by then ratatui's diff has dropped the
/// cell a wide glyph covers — correctly, since the terminal advances two
/// columns for it, but the record left behind cannot be told apart from a cell
/// nothing painted.
///
/// Hands back the bars it left on the page and the links it drew, which is
/// what a press is asked against: see [`bars`] and [`links`].
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &impl Screen) -> Left {
    let (bars, links) =
        links::collect(|| bars::collect(cells, |cells| draw_the_frame(cells, area, app)));
    Left { bars, links }
}

/// What a frame left that a press is asked against.
#[derive(Debug, Default)]
pub struct Left {
    /// The bars it left on the page.
    pub bars: Vec<bars::Drawn>,
    /// The links it drew.
    pub links: Vec<links::Drawn>,
}

/// Whether what is drawn for `layer` -- or for the document, where that is
/// `None` -- is nearest the reader.
///
/// Every view on screen is drawn, however many are stacked; only the nearest
/// marks where the keys are. A row lit under a list, in the colour the list
/// lights its own, is two places saying the keys are here, and the reader
/// cannot tell from either which one is lying. The caret is the same answer
/// from the other side: it goes where `layers().nearest()` says.
pub(crate) fn in_front(app: &impl Screen, layer: Option<Layer>) -> bool {
    app.layers().nearest() == layer
}

/// What [`draw`] draws.
fn draw_the_frame(cells: &mut CellBuffer, area: Rect, app: &impl Screen) {
    let regions = regions(area);
    let layers = app.layers();
    // Under whatever the region holds and over the status bar, once, for
    // every view: what is above it changes and the boundary does not.
    rule(cells, regions.edge, app.theme());

    // The document being read, under everything. A file, a conversation or
    // the notes: each fills the editor region, and none is over another --
    // switching between them is switching documents, not opening something.
    // A file being shown some other way is shown that way: the editor view
    // draws the file's own bytes, which in that mode is not what is on
    // screen.
    //
    // In the room it actually has, which is short of the region when a
    // list is sitting on the status bar over it. A conversation is the one
    // that takes the whole region: it puts a list *inside* itself, above
    // the box a message is written in, so it has already made the room.
    // The room the reader has, and the rows it is painted on -- see
    // `editor_canvas` for why those are two answers. The chrome that says
    // *where in the file this is* belongs to the first: a scrollbar whose
    // bottom third is behind a list is a scrollbar that cannot be read.
    let room = editor_room(area, app);
    let canvas = editor_canvas(area);
    match app.shown() {
        Shown::Notes(_) => {
            if let Some(view) = todo::TodoUi::new(app) {
                bars::of(Whose::Notes, || view.render(canvas, cells));
            }
        }
        // No bar: what scrolled off the top is the program's, and how far
        // back the reader is is said on the status row instead.
        Shown::Terminal(_) => {
            if let Some(view) = terminal::TerminalView::new(app) {
                view.render(canvas, cells);
            }
        }
        Shown::Chat(_) => {
            if let Some(view) = chat::ChatView::new(app) {
                bars::of(Whose::Conversation, || view.render(regions.editor, cells));
            }
        }
        Shown::Reading(rows) => bars::of(Whose::Document, || {
            let top = app
                .current_buffer()
                .map_or(0, |buffer| buffer.viewport().top.get());
            reading::draw(
                cells,
                canvas,
                rows,
                top,
                app.theme(),
                app.theme().background,
            );
        }),
        Shown::File(_) | Shown::Nothing => bars::of(Whose::Document, || {
            editor::EditorView::new(app)
                .the_reader_has(room.height)
                .render(canvas, cells);
        }),
    }
    // Nothing open and nothing to open: the one moment a reader needs
    // telling what the keys are. Not while something has taken the region,
    // because then the region is not empty -- but a list or a question
    // leaves it alone, and this is what they would be over.
    //
    // Or, where there is no project yet, the question of which: a screen
    // of its own rather than the welcome screen, because every key the
    // welcome screen names is about a project.
    if app.reading_nothing() && !layers.filling() {
        match projects::ProjectsView::new(app) {
            Some(view) => bars::of(Whose::Projects, || view.render(regions.editor, cells)),
            None => welcome::WelcomeView::new(app).render(regions.editor, cells),
        }
        // What could finish the path being named, which is not a layer
        // for the reason the agent's own commands are not one: the list
        // follows what is in the box rather than being something the
        // reader opened, and it goes where any compact list goes.
        if let Some(list) = app.naming_list() {
            bars::of(Whose::Naming, || {
                list_over(cells, app, list, None, regions.editor, regions.edge, None);
            });
        }
    }

    // The agent's own commands, which are not a layer: the list follows
    // what is being typed in the box rather than being something the
    // reader opened, and it goes where any compact list goes. So it is
    // part of the conversation and is drawn with it, under every layer:
    // a page opened over the conversation covers it, and a list opened
    // over it lies over it, and neither puts it away -- it is still there
    // when they go, because what is in the box is still a name.
    if let Some(list) = app.slash() {
        bars::of(Whose::Commands, || {
            list_over(
                cells,
                app,
                list,
                None,
                room_for_the_commands(app, regions.editor),
                regions.edge,
                // Not a layer, so it never took the row: the conversation's
                // own row is still the conversation's while this is showing.
                None,
            );
        });
    }

    // The status row, filled before a pane is laid over it. Nothing else
    // fills it in time: the page beneath stops at the editor region, and
    // the row is written by whoever owns it only after the pane is said --
    // at the end of this function, or at a dialog's own foot.
    //
    // The row and not the rule above it. The rule is drawn at the top of
    // this function and is already there; filling over it took the line
    // between a page's foot and its own row away, which is the one thing
    // this must leave exactly as it was.
    //
    // A terminal never noticed, because cells nobody wrote keep whatever
    // was on them. A window is the one that has to be told -- it is the
    // same rule `editor_canvas` is written for, one row along: what a pane
    // is laid over has to be *there* for it to be laid over, and cells
    // nobody wrote are not a page seen through glass, they are a hole. The
    // hole showed as a grey band across the foot of the counts and of the
    // settings, which is the blur behind the glass reading the nothing
    // under it.
    //
    // Whenever anything covers the file, not when the nearest thing takes
    // the row: whatever covers it is a pane, and a pane may reach the row
    // whether or not the row is its own.
    // A setting's words are typed on the status row, and that line *is* the
    // row rather than taking it, so asking the nearest left the settings'
    // glass over a hole for as long as the reader was typing -- the same
    // band, under the box.
    if layers.covering() {
        fill(
            cells,
            regions.status,
            Style::new().bg(app.theme().background),
        );
    }

    // And then whatever is over it, furthest from the reader first, which
    // is the order `layers` declares and the reverse of the one a key is
    // offered in. One array holds both, so the thing drawn last is the
    // thing a key reaches.
    for layer in layers.furthest_first() {
        match layer {
            // The settings take `area`, like the counts: a dialog does not
            // borrow Obelus's status row. What the row under the page says
            // is the page's own filter, so the page is handed the row and
            // draws it -- rather than typing into a row that belongs to the
            // file behind it, which is a row saying two things at once.
            Layer::Settings => {
                if let Some(view) = settings::SettingsView::new(app) {
                    shapes::behind(area, shapes::Joined::Screen, app.theme().background, cells);
                    // The same three rectangles as ever -- the page, the rule
                    // under it, the row at the foot. What changed is who
                    // writes the last of them: the page does, because it is
                    // the page's row. Splitting `area` some other way would
                    // be a second answer to a question `regions` already
                    // has, and the rule went missing the moment there was
                    // one: the page was handed the row the rule is drawn on
                    // and painted over it.
                    bars::of(Whose::Settings, || view.render(regions.editor, cells));
                    if let Some(settings) = app.settings() {
                        let style = Style::new()
                            .bg(app.theme().background)
                            .fg(app.theme().status_foreground);
                        // The whole row first, for the reason the status row
                        // was always filled by whoever drew it: what writes
                        // a filter writes words and not a ground.
                        fill(cells, regions.status, style);
                        status::StatusView::new(app).render_filter(
                            settings,
                            regions.status,
                            cells,
                            style,
                        );
                    }
                }
            }
            // The counts take `area` rather than the region: they are the
            // one view that has the status row as well, which is what
            // `Room::Screen` says about them.
            Layer::Counts => {
                if let Some(view) = counts::CountsView::new(app) {
                    shapes::behind(area, shapes::Joined::Screen, app.theme().background, cells);
                    bars::of(Whose::Counts, || view.render(area, cells));
                }
            }
            // The screen, like the counts, over whatever the reader was in
            // when the project went: a window draws it as glass over that,
            // and a terminal, which has no glass, as the page alone.
            Layer::Gone => {
                if let Some(view) = gone::GoneView::new(app) {
                    shapes::behind(area, shapes::Joined::Screen, app.theme().background, cells);
                    view.render(area, cells);
                }
            }
            Layer::Picker => {
                if let Some(list) = app.picker() {
                    // `list_over` says where the pane is, because it is
                    // what works out the room the list takes: two answers
                    // to that would be a backdrop that does not line up
                    // with what is over it.
                    bars::of(Whose::Picker, || {
                        list_over(
                            cells,
                            app,
                            list,
                            Some(Layer::Picker),
                            room_for_a_picker(regions.editor),
                            regions.edge,
                            Some(regions.status),
                        );
                    });
                }
            }
            Layer::Names => {
                if let Some(names) = app.names() {
                    let room = room_for_a_picker(regions.editor);
                    let region = names::region(names, room);
                    // Standing on the row below it, like every list that
                    // leaves the page showing above it -- and its rule
                    // with it, the same as a compact list's.
                    shapes::behind(
                        with_its_rules(region, room, regions.edge),
                        shapes::Joined::Below,
                        app.theme().background,
                        cells,
                    );
                    bars::of(Whose::Names, || {
                        names::NamesView::new(names, app.theme()).render(region, cells);
                    });
                    let style = Style::new()
                        .bg(app.theme().background)
                        .fg(app.theme().status_foreground);
                    fill(cells, regions.status, style);
                    status::StatusView::new(app).render_names(names, regions.status, cells, style);
                    // The edge every band gets, for the same reason a
                    // compact list gets one: two different things sharing
                    // a screen have to be told apart.
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
                }
            }
            // Drawn by the status row, which is the row it is on.
            Layer::Prompt => {}
        }
    }
    // The three panels that belong to a place in the file. Each is empty
    // while anything is over the file -- they are settled that way once a
    // frame -- so nothing here has to ask a second time.
    //
    // What could be typed next belongs beside the cursor, and the cursor is
    // on top of everything in the region.
    if let Some(panel) = complete::layout(app, regions.editor) {
        bars::of(Whose::Completion, || complete::draw(cells, panel, app));
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
        bars::of(Whose::Hover, || hover::draw(cells, panel, app));
    }
    // And what is *wrong* with the line the reader is on, which is the
    // one of the four nobody asked for -- so it is drawn last and gives
    // nothing while any of the others is up.
    if let Some(panel) = trouble::layout(app, regions.editor) {
        trouble::draw(cells, panel, app);
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
/// these cannot draw the rest of Obelus either, and this is the one thing
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

/// How far round its turn a cell holding one of those frames is, from
/// nought to one, or `None` for a cell holding anything else.
///
/// For a window, which draws the turn itself and has two things to ask of
/// the cell it was told about: whether it still holds the mark, and --
/// where the reader has said nothing is to move on its own -- where the
/// turn has got to on the application's clock rather than its own.
#[must_use]
pub fn how_far_round(symbol: &str) -> Option<f32> {
    let mut characters = symbol.chars();
    let glyph = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    let frame = SPINNING.iter().position(|&frame| frame == glyph)?;
    Some(frame as f32 / SPINNING.len() as f32)
}

/// What goes between a Nerd Font glyph and the words it is in front of,
/// where there is a gap to read by as well as a glyph.
///
/// Two blanks in a terminal and one in a window. A terminal's Nerd Font
/// draws the glyph two cells wide in the one cell it was given, so the
/// first blank is the half it bleeds into and only the second is a gap;
/// the window draws with the `Mono` face it carries, fitted to one cell,
/// and two blanks there were a gap and a cell of nothing beside it.
///
/// Not where a glyph has one blank after it, which is the bleed and no gap:
/// in a terminal those glyphs sit against their words, and in a window the
/// blank is the gap a terminal never showed.
pub(crate) fn after_a_glyph() -> &'static str {
    gap_after_a_glyph(obelus_config::in_a_window())
}

/// The same, asked of a front end named rather than of the one drawing,
/// for whoever has to answer for both in one test binary.
pub(crate) const fn gap_after_a_glyph(window: bool) -> &'static str {
    match window {
        true => " ",
        false => "  ",
    }
}

/// Says that the mark that turns is at this cell, where what was written
/// there begins with it.
///
/// For the marks that are a word as well as a glyph -- a server's badge, an
/// install's progress -- which are put together somewhere else and written
/// here, and only begin with the mark while it turns. Everywhere else the
/// view knows which branch it is in and says so with `shapes::spun`.
pub(crate) fn turning_at(x: u16, y: u16, said: &str) {
    if said
        .chars()
        .next()
        .is_some_and(|first| SPINNING.contains(&first))
    {
        shapes::spun(x, y);
    }
}

/// Where the list of an agent's commands goes.
///
/// The editor region, less what a conversation's box has taken from the
/// foot of it: this list is a list of what is being typed into the box, so
/// a list drawn over the box would cover the thing the reader is typing
/// into to find it.
fn room_for_the_commands(app: &impl Screen, editor: Rect) -> Rect {
    app.shown()
        .chat()
        .map_or(editor, |chat| chat::above_writing(editor, chat, app.card()))
}

/// Where a list the reader opened goes, given what it is over.
///
/// The whole region, whatever it is over. A picker is not part of the
/// conversation the way the commands are -- it took the keys and it took
/// the status row -- so the conversation is behind it rather than beside
/// it, and that includes a question the agent is waiting on. The card was
/// once left showing under the list, so a reader who went to look
/// something up would find the question still there; but it was a second
/// thing on screen that no key reached, drawn sharp beside the glass, and
/// the question is in the conversation either way -- it is the first thing
/// there when the list goes.
const fn room_for_a_picker(editor: Rect) -> Rect {
    editor
}

/// A pane, and the rules either side of it that are its edges.
///
/// The row above a band that stands lower than the top of its room is the
/// rule it draws over itself, and the row under anything that reaches
/// the foot of the region is the rule the status row has over it. A
/// window draws both lines itself, in the middle of their rows, and ends
/// the glass at them -- see `shapes::Shapes::ruled`. Left out, the glass
/// stopped at the cell boundary, half a row short of the line that said
/// where the pane ends.
///
/// One answer, asked by every pane with a rule beside it.
fn with_its_rules(band: Rect, room: Rect, edge: Rect) -> Rect {
    let top = match band.y > room.y {
        true => band.y - 1,
        false => band.y,
    };
    let bottom = match edge.height > 0 && band.bottom() == edge.y {
        true => edge.bottom(),
        false => band.bottom(),
    };
    Rect {
        y: top,
        height: bottom - top,
        ..band
    }
}

/// Draws a list over whatever is behind it, with its edge and its preview.
///
/// One function for all of them, because a list opened over the code, over
/// the settings and over a conversation is the same list: what differs is
/// the room it is given, which is the argument.
fn list_over(
    cells: &mut CellBuffer,
    app: &impl Screen,
    list: &Picker,
    whose: Option<Layer>,
    room: Rect,
    edge: Rect,
    own_row: Option<Rect>,
) {
    let region = picker::region(list, room);
    // What the list actually takes, which for a compact one is a strip at
    // the foot of the region and not the region: saying the region would
    // put glass over the whole file and slide the whole file with it.
    //
    // And the rules either side of it go with it, because they are its
    // edges -- see `with_its_rules`.
    let (pane, joined) = match list.layout() {
        obelus_component::picker::PickerLayout::FullArea => {
            // Joined on every side once it has the row at its foot: the
            // edge it would draw a line along is the screen's own, and a
            // line there is a hair across the bottom of the window with
            // nothing on the other side of it. A list that does not own
            // that row still ends above one, and still has an edge.
            let joined = match own_row {
                Some(_) => shapes::Joined::Screen,
                None => shapes::Joined::Above,
            };
            (with_its_rules(room, room, edge), joined)
        }
        obelus_component::picker::PickerLayout::Compact { .. } => {
            // Still `Below` with the row: what it is joined to is the foot
            // of the screen either way, and the edge it draws is the one
            // along its top, where the file it is over carries on.
            (with_its_rules(region, room, edge), shapes::Joined::Below)
        }
    };
    // And the row the list types into, where the list is the one that owns
    // it: a dialog is one thing, so it arrives as one. Left out, the list
    // slid down and its own query sat still at the foot of the screen,
    // which reads as two things opening rather than one.
    //
    // Worked out here beside the pane and not by the caller, for the reason
    // the comment above gives: two answers to where the pane is would be a
    // backdrop that does not line up with what is over it, and the row is
    // part of the pane now.
    let pane = match own_row {
        Some(row) => Rect {
            height: row.bottom() - pane.y,
            ..pane
        },
        None => pane,
    };
    shapes::behind(pane, joined, app.theme().background, cells);
    picker::PickerView::new(list, app.theme(), app.phase(), in_front(app, whose))
        .render(region, cells);
    if let Some(row) = own_row {
        let style = Style::new()
            .bg(app.theme().background)
            .fg(app.theme().status_foreground);
        fill(cells, row, style);
        status::StatusView::new(app).render_prompt(list, row, cells, style);
    }

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

    // Above a compact list, in the room the code was drawn in a moment ago:
    // it sits on the status bar and has nothing under it to divide. Over
    // the code rather than instead of it, because what a row names changes
    // as the reader walks and the editor has already drawn the file they
    // came from.
    if let Some(over) = picker::preview_over(Some(list), picker::room_above(list, room))
        && let Some(shown) = app.preview()
    {
        bars::of(Whose::Preview, || {
            editor::EditorView::for_buffer(
                shown.buffer,
                shown.highlights,
                app.theme(),
                shown.marked,
                shown.changes,
                shown.troubles,
            )
            .render(over, cells);
        });
        // And what is wrong with the line it is showing, floated over it
        // the way the editor floats one over the caret's. Here and not
        // under a full list's rows: the box is an answer to a row that
        // *names* a problem, and the one list that does is compact.
        //
        // In the room the preview was laid out in, which it carries: a sum
        // of its own here would be a second reckoning of the gutter, the
        // change map and the bar, and the one that forgot any of them
        // would put the box a row from where the line is.
        if let Some(complaint) = shown.complaint.as_ref()
            && let Some(area) =
                trouble::where_it_goes(complaint, shown.buffer, shown.changes, shown.text, over)
        {
            trouble::write(cells, area, complaint, app.theme());
        }
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
            Some(Previewed {
                reading: Some(shown),
                ..
            }) => bars::of(Whose::Preview, || {
                fill(cells, preview, Style::new().bg(app.theme().background));
                let area = reading::in_a_preview(preview);
                reading::draw(
                    cells,
                    area,
                    shown.rows,
                    shown.top,
                    app.theme(),
                    app.theme().background,
                );
                // The mark at the head of the row that says something is on
                // its way, where that row is on screen.
                if let Some(row) = shown.turning
                    && let Some(below) = row.checked_sub(shown.top)
                    && let Ok(below) = u16::try_from(below)
                    && below < area.height
                {
                    let y = area.y + below;
                    write(
                        cells,
                        area.x,
                        y,
                        &spinning(app.phase()).to_string(),
                        Style::new()
                            .fg(app.theme().gutter)
                            .bg(app.theme().background),
                    );
                    shapes::spun(area.x, y);
                }
            }),
            Some(shown) => bars::of(Whose::Preview, || {
                editor::EditorView::for_buffer(
                    shown.buffer,
                    shown.highlights,
                    app.theme(),
                    shown.marked,
                    shown.changes,
                    shown.troubles,
                )
                .render(preview, cells);
            }),
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
