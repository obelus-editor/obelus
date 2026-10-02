//! Drawing.
//!
//! Nothing in here reads a file, makes a syscall or parses anything. The draw
//! path runs inside `Terminal::draw`, which blocks the main loop on a write to
//! stdout; adding slow work to it is the mistake that actually happens, rather
//! than the write itself being slow.
//!
//! Everything that scrolls says so, in the last column of the region it is
//! in. A file, a preview, a list, a page of settings, a conversation -- the
//! last of those had no bar at all, which left a reader paging through it with
//! nothing on screen answering "how much of this is there, and which part am I
//! looking at". The editor's used to
//! sit one short of it, because the map of where a file has changed had the
//! edge: a list opened over a file made the bar jump sideways, and inside one
//! screen a list with a preview under it had its bar in two columns with a rule
//! between them.
//!
//! The map is *inside* the bar now rather than outside it. They are the same
//! picture at the same scale -- the whole file squeezed into the height of the
//! screen -- so they belong side by side, and the reader reads across them:
//! here is where you are, and here is what has changed.

/// What a colour a server found written down is drawn as.
///
/// A square rather than the whole cell filled in. A terminal cell is about
/// twice as tall as it is wide, so a filled one is an upright bar -- and a
/// bar beside a colour literal reads as a mark on the text rather than as
/// the colour itself. One cell wide, which is what the line was measured
/// with.
pub(crate) const SWATCH: char = '\u{25a0}';

/// How many cells it takes.
///
/// Measured from the glyph rather than written down beside it: the number
/// the line is laid out with and the thing drawn in it have to agree, and
/// two constants that must agree are one that can be changed alone.
#[must_use]
pub fn swatch_cells() -> usize {
    unicode_width::UnicodeWidthChar::width(SWATCH).unwrap_or(1)
}

/// What one cell a file does not contain is drawn as.
///
/// One list per document, because a cell points at one entry and cannot
/// say which of two lists it meant: the colours a server found written
/// down and the hints it worked out are drawn from the same one, in the
/// order the application put them there.
///
/// Here rather than beside either of them: the application builds it and
/// the editor draws it, and a type that lives with one of its two sources
/// would make the other one a guest.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    /// A colour, as a cell of itself.
    Swatch(ratatui::style::Color),
    /// Something a server would have you read that the file does not say.
    Hint(obelus_lsp::hint::Hinted),
}

use std::path::Path;

use obelus_agent::{Listed, Talking, acp};
use obelus_buffer::{Buffer, TextArea};
use obelus_component::{
    card::Card, chat::Chat, completion::Completion, counts::Counts, hover::Hover, layers,
    prompt::Prompt, settings::Settings, todo::TodoView,
};
use obelus_editing::keymap::Keymap;
use obelus_syntax::highlight::Highlights;
use obelus_text::coordinates::{LineNumber, Span};

use crate::image::Images;

/// One thing that went wrong on the way up.
///
/// The words are written by the time they get here -- what Obelus says
/// about a file it could not read is the same sentence whether it is drawn
/// under a line or on a list -- so this carries them and where to go, and
/// nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WentWrong {
    /// What Obelus says about it.
    pub said: String,
    /// The file it is about and the line in it, where there is one.
    pub at: Option<(std::path::PathBuf, LineNumber)>,
}

/// A path with the reader's own directory written as `~`.
///
/// Here because it is about drawing and not about the filesystem:
/// everything under the home directory would otherwise spend a dozen
/// columns of every row on the same word, taken from the part of the path
/// that says which thing it is. One piece of code, so the welcome
/// screen's own line and the rows of projects under it cannot disagree
/// about how a path is written.
#[must_use]
pub fn with_home_as_tilde(path: &std::path::Path) -> String {
    let said = path.to_string_lossy();
    let Some(home) = std::env::home_dir() else {
        return said.into_owned();
    };
    let home = home.to_string_lossy();
    // Only a whole leading component, so `/home/sunlight` is not written
    // as `~light` for a reader whose directory is `/home/sun`.
    match said.strip_prefix(home.as_ref()) {
        Some("") => "~".to_string(),
        // Either separator: a path on Windows may hold `/` and still be
        // the reader's own directory with something under it.
        Some(rest) if rest.starts_with(std::path::is_separator) => format!("~{rest}"),
        _ => said.into_owned(),
    }
}

/// What the page that asks which project offers, while Obelus is asking.
///
/// Here rather than with the application for the reason everything else in
/// this file is: it exists so that a screen can be drawn, and what the
/// application keeps about projects is a different shape with a different
/// job.
///
/// The words are settled before they get here -- a path shortened with
/// `~`, a time said the way the conversations say one -- because those are
/// facts about the reader's machine and their clock. What is *not* settled
/// is how much of either fits, which is the drawing's and depends on a
/// width this does not know.
#[derive(Clone, Debug, Default)]
pub struct Choosing {
    /// The projects, newest first, after the filter has had them. Without
    /// the row under them that opens one that is not in the list: that row
    /// is the view's own and is always there.
    pub known: Vec<Opened>,
    /// Which row the reader is on: the projects from nought, and the
    /// opening row after the last of them.
    pub at: usize,
    /// Which of the projects is on the first row, where the window over
    /// them is -- the one every list keeps, which moves only when the
    /// reader's row leaves it.
    pub top: usize,
    /// What is in the box at the foot.
    pub typed: String,
    /// How many characters of it are in front of the caret.
    pub caret: usize,
    /// Whether that box is a path being named rather than a filter over
    /// the rows.
    ///
    /// Two boxes and two meanings, which is why this is a flag and not a
    /// guess from whether the text has a separator in it.
    pub naming: bool,
    /// Whether what is in that box is there at all.
    ///
    /// What the ink on the row says, so that a reader sees enter will
    /// refuse before they press it. A path that is not there is not a
    /// project, and taking the directory above it instead -- which is
    /// what a path on the command line means -- would put Obelus on
    /// wherever the process began.
    pub there: bool,
    /// Whether anything is being offered to finish it with.
    ///
    /// What keeps the row quiet while a path is half typed: `/tmp/o` is
    /// not there either, and saying so under a list that is offering
    /// `obelus/` would be a complaint about typing. The row speaks only
    /// where there is nothing to suggest *and* nothing at the path.
    pub offering: bool,
}

/// One project the reader has had open, as a row.
#[derive(Clone, Debug)]
pub struct Opened {
    /// Where it is, with `~` for the reader's own directory.
    pub path: String,
    /// Which characters of it the filter matched, as `first..end`.
    ///
    /// Counted in that string and not in the path it was made from,
    /// which is why the filter is run against it: a mark worked out in
    /// one string and painted onto another lands on the wrong letters.
    pub matched: Option<(usize, usize)>,
    /// When it was last opened, in words. Empty for a row that does not
    /// say.
    pub when: String,
}

/// What a preview is, for the view that draws it.
///
/// A borrow of the whole of it rather than a tuple: it is the same list of
/// things the editor draws for the document being read, and a tuple of four
/// grows a fifth without saying what any of them are.
///
/// Here rather than with the application, for the same reason as the rest
/// of this file: it exists only so that a preview can be drawn, and every
/// field of it is a borrow of something the renderer would otherwise have
/// to be handed one at a time.
pub struct Previewed<'a> {
    /// The file, read into a buffer of its own.
    pub buffer: &'a Buffer,
    /// What a server says is wrong with it, placed in that buffer's own
    /// text, so the underline is under the right characters of the file
    /// actually on screen.
    pub troubles: &'a [obelus_lsp::trouble::Trouble],
    /// Its syntax, refreshed for the rows on screen.
    pub highlights: &'a Highlights,
    /// The runs of characters the preview is about, once converted.
    ///
    /// A list rather than one: a language server names one run, and a
    /// search names whatever characters the query matched, which is as
    /// many runs as the match is scattered over.
    pub marked: &'a [Span],
    /// What git says about the file.
    pub changes: Option<&'a obelus_git::Changes>,
    /// The room its text was laid out in, which is what anything drawn
    /// against the same rows has to measure by.
    pub text: obelus_buffer::TextArea,
    /// What is wrong with the line this preview is showing, where the list
    /// showing it is about a problem.
    ///
    /// The row above carries only the message's first line -- a paragraph
    /// in a row is a row nobody can read -- so the box is where the whole
    /// of it fits.
    pub complaint: Option<Complained<'a>>,
}

/// What is wrong with the line the reader is on, for the box that says so.
///
/// A borrow rather than a tuple, the same as [`Previewed`], and here for
/// the same reason: it exists so that one box can be drawn, and every
/// field of it is something the renderer would otherwise be handed one at
/// a time.
///
/// What is *not* here is the wrapping and the frame. Those were worked out
/// in the application, because the words went into the file as a block and
/// a block is rows -- so the width had to be known before there was
/// anywhere to put them. A box floated over the page is laid out to the
/// room it is given, like every other one, and the room is the view's to
/// know.
pub struct Complained<'a> {
    /// The line it is about, which is what the box is anchored under.
    pub line: obelus_text::coordinates::LineNumber,
    /// And the character of it the trouble starts at, so the box hangs
    /// under the thing it is about rather than under the left-hand edge of
    /// code that may have nothing to do with it.
    pub column: obelus_text::coordinates::CharColumn,
    /// The worst one's own words, in the server's spelling.
    pub said: &'a str,
    /// How bad it is, which is the colour they are written in -- the same
    /// colour the underline under the word is in.
    pub severity: obelus_lsp::trouble::Severity,
    /// How many others are on that line, which the box counts rather than
    /// lists: one of them is a box, and five is the screen.
    pub others: usize,
}

/// Everything a frame is drawn from.
///
/// The renderer used to take `&App`, and this is the list of what it
/// actually asked it -- forty-four questions of a type with several
/// hundred methods. Written down, the list is an interface rather than an
/// acquaintance: what draws can be compiled and read without the
/// application, and the application cannot quietly become something a view
/// reaches into.
///
/// Every one of these is a question about state that has already settled.
/// None may take `&mut self` and none may do work: a frame is drawn on
/// every keystroke, and a view that could change what it is drawing is a
/// view whose output depends on how often it was asked for.
///
/// One trait rather than one per view, because the views overlap heavily
/// -- eleven of them ask for the theme, six for the file being read -- and
/// eleven traits with the same dozen methods is one list kept in eleven
/// places.
pub trait Screen {
    /// What to call the agent on screen.
    fn agent_name(&self) -> Option<&str>;
    /// The settings it lets the reader change.
    fn agent_settings(&self) -> &[acp::Setting];
    /// What the active agent offers to be set before a conversation
    /// starts, with what the reader has said about each.
    ///
    /// `None` where no agent is active, which is the one case where the
    /// settings page has no group for one. Built rather than borrowed: it
    /// comes out of the config, what the agent was last heard to offer and
    /// the registry at once.
    fn agent_offering(&self) -> Option<obelus_component::settings::Offering>;
    /// How full the agent's memory of this conversation is, once it has
    /// said -- and what it has cost, where it counts that too.
    fn agent_usage(&self) -> Option<&acp::Usage>;
    /// Who last changed each line of the file being read, if the answer has
    /// arrived and the reader wants to see it.
    fn blame(&self) -> Option<&[Option<obelus_git::Blamed>]>;
    /// Whether what is typed goes over what is under the cursor.
    fn replacing(&self) -> bool;
    /// The list of names a setting is being built into, while one is open.
    fn names(&self) -> Option<&obelus_component::names::Names>;
    /// The card an agent's question is on, while one is up.
    fn card(&self) -> Option<&Card>;
    /// What has changed in the current file, if Obelus can tell.
    fn changes(&self) -> Option<&obelus_git::Changes>;
    /// The conversation, while it is what the reader is looking at.
    fn chat(&self) -> Option<&Chat>;
    /// What could be typed next, while a server's answer is on screen.
    fn completion(&self) -> Option<&Completion>;
    /// What the reader has decided.
    fn config(&self) -> &obelus_config::Config;
    /// The line counts, while they are showing.
    fn counts(&self) -> Option<&Counts>;
    /// The document being read, if any is open.
    fn current_buffer(&self) -> Option<&Buffer>;
    /// What is drawn in the file being read that the file does not contain.
    fn drawn(&self) -> &[crate::Drawn];
    /// The highlight kinds for what is on screen.
    fn highlights(&self) -> &Highlights;
    /// What the server says the place under the caret is, while it is up.
    fn hover(&self) -> Option<&Hover>;
    /// The marks, for the view to draw.
    fn images(&self) -> &Images;
    /// The bindings currently in force.
    fn keymap(&self) -> &Keymap;
    /// How many screen rows the file's view has travelled altogether.
    ///
    /// A number to be compared rather than read: what a change in it says
    /// is that the view scrolled, and by how much.
    fn travelled(&self) -> i64;
    /// Whether a command can do its job right now, which is the one
    /// judgement of it: a view that says a key is there says it only where
    /// pressing it would do something.
    fn offers(&self, command: obelus_command::Command) -> bool;
    /// What is on screen over the file, worked out from what is open.
    fn layers(&self) -> layers::Layers;
    /// The agents page's rows.
    fn listed_agents(&self) -> Vec<Listed>;
    /// The runs the editor marks: the uses of the name the pointer is
    /// resting on.
    fn marked_runs(&self) -> &[obelus_text::coordinates::Span];
    /// What Obelus has to say, until the next key.
    fn note(&self) -> Option<&str>;
    /// The notes, while the reader is in them.
    fn notes(&self) -> Option<&TodoView>;
    /// Whether each note has a conversation, by the note's place in the
    /// list -- which is what a row of the notes names.
    fn talked_about(&self) -> Vec<obelus_component::todo::Talked>;
    /// The hunk the reader has opened in place, if any.
    fn opened_hunks(&self) -> Vec<LineNumber>;
    /// How far along the welcome screen's colours have travelled, in ticks.
    fn phase(&self) -> u32;
    /// What went wrong on the way up, for the screen that is showing when
    /// nothing is open.
    ///
    /// Empty on almost every start, and the block it fills is absent then:
    /// what this screen is for is the way in, and what went wrong goes
    /// under it rather than in front of it.
    fn went_wrong(&self) -> Vec<WentWrong>;
    /// What is being asked, while Obelus is asking which project. `None`
    /// on every start that was told one.
    fn choosing(&self) -> Option<Choosing>;
    /// What could finish the path being named, while one is being named.
    ///
    /// An ordinary compact list, the way the agent's own commands are:
    /// the rows and the chosen row are the picker's, and the box below it
    /// owns the keys.
    fn naming_list(&self) -> Option<&Picker>;
    /// Which of those rows the reader is on.
    fn went_wrong_at(&self) -> usize;
    /// The version of a newer Obelus, where one is out and the reader
    /// wants to be told.
    fn newer_release(&self) -> Option<&str>;
    /// And which of them are on screen, out of `rows` that fit.
    fn went_wrong_showing(&self, rows: u16) -> std::ops::Range<usize>;
    /// The open picker, for the renderer.
    fn picker(&self) -> Option<&Picker>;
    /// The settings the project has set, which are the ones the reader cannot
    /// change from here.
    fn pinned(&self) -> &[&'static str];
    /// The file the picker's selection names, if it has been read, and the
    /// part of it the selection is about.
    fn preview(&self) -> Option<Previewed<'_>>;
    /// Whether what Obelus has to say is about something that would not
    /// go.
    ///
    /// The ink it is drawn in and nothing else. A note that reports and a
    /// note that refuses are the same words in the same place, and the
    /// row has nothing else to tell them apart with -- which is what left
    /// a reader reading `Not saved` in the same colour as `Saved`.
    fn note_is_wrong(&self) -> bool;
    /// What is wrong with the line the reader is on, where anything is.
    ///
    /// Only the caret's line, or the row a list of problems has walked
    /// them to. Every other one is said by the underline, which costs no
    /// room at all and is on all of them.
    fn complaint(&self) -> Option<Complained<'_>>;
    /// The question being asked, if one is.
    fn prompt(&self) -> Option<&Prompt>;
    /// The directory the question being asked would put a file in.
    ///
    /// `None` unless the question is the one that makes a file, which is
    /// the only one on that row whose answer is a place. Worked out by the
    /// application, because where a path points is the application's
    /// question and the one that acts on it has to get the same answer.
    fn making_in(&self) -> Option<String>;
    /// Which settings the reader's own file named.
    fn readers_named(&self) -> &[&'static str];
    /// Whether there is anything being read at all.
    fn reading_nothing(&self) -> bool;
    /// Why the list could not be fetched, if it could not.
    fn registry_failure(&self) -> Option<&str>;
    /// How many rows it has, for the keys that scroll it.
    fn rendered_rows(&self) -> Option<usize>;
    /// The reading on screen, if the current file is being shown as one.
    fn rendering(&self) -> Option<&[obelus_row::Row]>;
    /// The server for the file being read, and what it is doing.
    fn server_state(&self) -> Option<(&'static str, obelus_lsp::ServerState)>;
    /// What a language server is busy with, if one is.
    fn server_working_on(&self) -> Option<&str>;
    /// Whether the server behind the file being read is busy with something.
    fn server_busy(&self) -> bool;
    /// The settings view, while it is open.
    fn settings(&self) -> Option<&Settings>;
    /// What the call the cursor is inside takes, while it is showing.
    fn signature(&self) -> Option<&obelus_component::signature::Signature>;
    /// The agent's own commands, while one is being typed.
    fn slash(&self) -> Option<&Picker>;
    /// What Obelus is doing about an agent.
    fn talking(&self) -> Talking;
    /// The room the text has, once the gutter has taken its columns.
    fn text_area(&self) -> TextArea;
    /// The colours currently in force.
    fn theme(&self) -> &Theme;
    /// The project's own settings file, while the project has one.
    fn project_config(&self) -> Option<&Path>;
    /// What the server says is wrong with the file being read.
    fn troubles(&self) -> &[obelus_lsp::trouble::Trouble];
    /// Whether the conversation being read is about a note that is still
    /// there.
    fn is_about_a_note(&self) -> bool;
    /// The branch the conversation being read is working on, once its
    /// agent has changed a file.
    fn branch_this_conversation_works_on(&self) -> Option<&obelus_git::Head>;
    /// Where Obelus was started, and the root every path is shown relative to.
    fn working_directory(&self) -> &Path;
    /// Which branch that tree has checked out, where it is a repository.
    fn head(&self) -> Option<&obelus_git::Head>;
    /// Whether that tree has gone from disk.
    fn tree_has_gone(&self) -> bool;
}

pub mod card;
pub mod chat;
pub mod complete;
pub mod counts;
pub mod editor;
pub mod hover;
pub mod image;
pub mod names;
pub mod picker;
pub mod projects;
pub mod reading;
pub mod settings;
pub mod shapes;
pub mod signature;
pub mod status;
pub mod todo;
pub mod trouble;
pub mod welcome;

use std::ops::Range;

use obelus_component::{
    layers::Layer,
    picker::{Colouring, Picker},
};
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    widgets::Widget as _,
};
use unicode_width::UnicodeWidthChar as _;

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
    if app.chat().is_some() {
        return editor;
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
    // for that reason rather than for an order among them.
    if let Some(choosing) = app.choosing() {
        let question = match choosing.naming {
            true => "Open",
            false => "Filter",
        };
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
        Some(Layer::Picker) => return on_the_status_row(status::prompt_caret(app.picker()?)),
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
        // Nothing over the document, so the caret is the document's own.
        None => {
            // A conversation is written into, and its caret is in the box
            // rather than on the status bar: a message is a paragraph, and
            // a paragraph does not fit on one row.
            if let Some(chat) = app.chat() {
                return chat::ChatView::caret(regions.editor, chat, app.card());
            }
            // And the notes, which are written into the same way.
            if let Some(notes) = app.notes() {
                return todo::caret(regions.editor, notes);
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
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &impl Screen) {
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
    if let Some(view) = todo::TodoUi::new(app) {
        view.render(canvas, cells);
    } else {
        match chat::ChatView::new(app) {
            Some(view) => view.render(regions.editor, cells),
            None => match app.rendering() {
                Some(rows) => {
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
                }
                None => editor::EditorView::new(app)
                    .the_reader_has(room.height)
                    .render(canvas, cells),
            },
        }
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
            Some(view) => view.render(regions.editor, cells),
            None => welcome::WelcomeView::new(app).render(regions.editor, cells),
        }
        // What could finish the path being named, which is not a layer
        // for the reason the agent's own commands are not one: the list
        // follows what is in the box rather than being something the
        // reader opened, and it goes where any compact list goes.
        if let Some(list) = app.naming_list() {
            list_over(cells, app, list, regions.editor, regions.edge, None);
        }
    }

    // And then whatever is over it, furthest from the reader first, which
    // is the order `layers` declares and the reverse of the one a key is
    // The row a full-screen dialog is about to take, filled before it is
    // drawn over. Nothing else fills it: the page beneath stops at the
    // editor region and the status row is written at the end of this
    // function, which is a thing a full-screen dialog makes Obelus skip.
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
    if layers.taking_the_status_row() {
        fill(
            cells,
            regions.status,
            Style::new().bg(app.theme().background),
        );
    }

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
                    view.render(regions.editor, cells);
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
                    view.render(area, cells);
                }
            }
            Layer::Picker => {
                if let Some(list) = app.picker() {
                    // `list_over` says where the pane is, because it is
                    // what works out the room the list takes: two answers
                    // to that would be a backdrop that does not line up
                    // with what is over it.
                    list_over(
                        cells,
                        app,
                        list,
                        room_for_a_picker(app, regions.editor),
                        regions.edge,
                        Some(regions.status),
                    );
                }
            }
            Layer::Names => {
                if let Some(names) = app.names() {
                    let room = room_for_a_picker(app, regions.editor);
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
                    names::NamesView::new(names, app.theme()).render(region, cells);
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
    // The agent's own commands, which are not a layer: the list follows
    // what is being typed in the box rather than being something the
    // reader opened, and it goes where any compact list goes. A picker
    // over the same conversation wins, because that one is a question the
    // agent is waiting on an answer to.
    if !layers.has(Layer::Picker)
        && let Some(list) = app.slash()
    {
        list_over(
            cells,
            app,
            list,
            room_for_the_commands(app, regions.editor),
            regions.edge,
            // Not a layer, so it never took the row: the conversation's own
            // row is still the conversation's while this is showing.
            None,
        );
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

/// Where the list of an agent's commands goes.
///
/// The editor region, less what a conversation's box has taken from the
/// foot of it: this list is a list of what is being typed into the box, so
/// a list drawn over the box would cover the thing the reader is typing
/// into to find it.
fn room_for_the_commands(app: &impl Screen, editor: Rect) -> Rect {
    app.chat()
        .map_or(editor, |chat| chat::above_writing(editor, chat, app.card()))
}

/// Where a list the reader opened goes, given what it is over.
///
/// The whole region, less only a question the agent is waiting on an answer
/// to. A picker is not part of the conversation the way the commands are --
/// it took the keys and it took the status row -- so the conversation is
/// behind it rather than beside it.
fn room_for_a_picker(app: &impl Screen, editor: Rect) -> Rect {
    chat::above_a_question(editor, app.card())
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
    picker::PickerView::new(list, app.theme(), app.phase()).render(region, cells);
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
        editor::EditorView::for_buffer(
            shown.buffer,
            shown.highlights,
            app.theme(),
            shown.marked,
            shown.changes,
            shown.troubles,
        )
        .render(over, cells);
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
            Some(shown) => {
                editor::EditorView::for_buffer(
                    shown.buffer,
                    shown.highlights,
                    app.theme(),
                    shown.marked,
                    shown.changes,
                    shown.troubles,
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
    shapes::ruled(Rect { height: 1, ..area });
}

/// The line between the two halves of one panel.
///
/// Not [`rule`], which is a boundary between two subjects on the page: this
/// one is the box's own, so it meets the border it crosses -- `├` and `┤`
/// rather than a line that overwrites the sides and leaves two boxes
/// touching. The completion panel's list and its documentation are divided
/// by it, and so are a signature's labels and what the call says about
/// itself; both are one thing with two parts.
pub(crate) fn divider(cells: &mut CellBuffer, area: Rect, y: u16, theme: &Theme) {
    let edge = Style::new().fg(theme.gutter).bg(theme.background);
    put(cells, area.x, y, '\u{251c}', edge);
    for x in area.x + 1..area.right().saturating_sub(1) {
        put(cells, x, y, '\u{2500}', edge);
    }
    put(cells, area.right().saturating_sub(1), y, '\u{2524}', edge);
    shapes::ruled(Rect {
        y,
        height: 1,
        ..area
    });
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
/// It also says it is here, and hands back what it said. One piece of code
/// draws a bar, so one piece of code knows where one is: the two callers
/// that pass a bar on to [`shapes::scrolled`] were each working the mark
/// out a second time from the same three numbers, which is the one thing
/// `bar_reach` exists to stop happening.
pub(crate) fn scrollbar(
    cells: &mut CellBuffer,
    area: Rect,
    top: usize,
    total: usize,
    theme: &Theme,
) -> Option<shapes::Bar> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let height = usize::from(area.height);
    let total = total.max(1);
    let x = area.right().saturating_sub(1);

    let (thumb, _, _) = bar_reach(height, total);
    let start = bar_mark(area.height, top, total);

    for row in 0..area.height {
        let inside = row >= start && usize::from(row) < usize::from(start) + thumb;
        let colour = if inside {
            theme.gutter_current
        } else {
            theme.scrollbar_track
        };
        put(cells, x, area.y + row, BAR, Style::new().fg(colour));
    }

    let bar = shapes::Bar {
        // The one column the cells went in, which is not `area`: a caller
        // hands this the whole list and the bar takes its last column.
        area: Rect {
            x,
            width: 1,
            ..area
        },
        mark: start,
        thumb: u16::try_from(thumb).unwrap_or(area.height),
    };
    shapes::barred(bar);
    Some(bar)
}

/// How big a bar's thumb is, how far it can travel, and how far the top it
/// is about can.
///
/// One answer, because two places need it now: the bar that draws the
/// thumb, and the front end that has to know how far the thumb moves for
/// each row the thing it is about moves. Two workings-out of the same
/// three numbers would be a mark that slides at one rate and lands at
/// another.
fn bar_reach(height: usize, total: usize) -> (usize, usize, usize) {
    // A bar with no rows has no thumb and nowhere to put one. The drawing
    // used to be the only caller and turned back at its own door; now that
    // two ask, the answer belongs here -- and a view is handed a region of
    // no height often enough that this is not a corner: a terminal one row
    // tall, a list with a question over it, a frame drawn before anything
    // has been laid out.
    if height == 0 {
        return (0, 0, 1);
    }
    // At least one row of thumb, or a long list has a bar with nothing on it.
    let thumb = (height * height / total.max(1)).clamp(1, height);
    let travel = height.saturating_sub(thumb);
    // Scaled by how far the *top* can travel, not by the total: dividing by
    // the total leaves the thumb short of the bottom exactly when the last
    // row is on screen, which is the one position anyone checks it against.
    let furthest = total.saturating_sub(height).max(1);
    (thumb, travel, furthest)
}

/// Which row of the track a bar's mark starts on.
///
/// The one place it is worked out, because two now want it: the bar that
/// draws the mark, and the front end that slides it. And the front end
/// needs *this* number rather than a rate, which is what it had first and
/// what made the mark step backwards. A rate is continuous and this is
/// not -- the mark is drawn on a row -- so sliding at the rate reached a
/// place a row's rounding away from where the mark was last drawn, in
/// whichever direction the rounding fell. Between two of these there is
/// nothing to disagree about: it sets out from where the mark was and
/// arrives where the mark is.
#[must_use]
pub fn bar_mark(height: u16, top: usize, total: usize) -> u16 {
    let rows = usize::from(height);
    if total <= rows {
        return 0;
    }
    let (_, travel, furthest) = bar_reach(rows, total);
    u16::try_from((top * travel).div_ceil(furthest).min(travel)).unwrap_or(0)
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
pub const fn opens(open: bool) -> char {
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
///
/// The glyph and the modifiers, and the colours patched over what was
/// there. A style is patched onto a cell everywhere in here -- that is what
/// lets a caller set a background and keep a foreground painted underneath
/// -- and a modifier patched the same way is one nothing can take off: an
/// underline written here by whatever was drawn before survives every
/// drawing over it. A preview of another file wore the underlines of the
/// file it was drawn over, at the columns they were at *there*, which is a
/// file complaining about a line it has never seen.
///
/// Anything drawn over anything else starts with this, so it is the one
/// place that has to take them off.
pub fn fill(cells: &mut CellBuffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = cells.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.modifier = ratatui::style::Modifier::empty();
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
        cell.set_char(shown(character));
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

/// What a character looks like in a cell.
///
/// Itself, unless it is a control character, which is a space. A terminal
/// cell holds something that is drawn, and a control character is an
/// instruction rather than a glyph: written into one it is not drawn wrong,
/// it takes the whole frame down, because what puts a frame on the screen
/// asks every cell how wide it is and a control character has no answer.
///
/// This is not Obelus's own text. It is whatever an agent said, whatever a
/// tool put in its output, whatever is in a file somebody opened -- and a
/// reader of code meets a tab in all three. None of them is a reason for
/// Obelus to stop.
///
/// One cell, which is what `put` already counted a control character as, so
/// nothing that walks a row in step with it has to learn a new rule. A tab
/// is therefore one space rather than a jump to the next stop: the place
/// that could do better than that is where the words become rows, and it
/// cannot be done here without the screen and every piece of arithmetic
/// about it disagreeing.
///
/// What is copied is untouched, because a copy is what was said rather than
/// what a terminal could show of it.
const fn shown(character: char) -> char {
    match character.is_control() {
        true => ' ',
        false => character,
    }
}

/// Writes a string, returning the column after it.
pub fn write(cells: &mut CellBuffer, x: u16, y: u16, contents: &str, style: Style) -> u16 {
    let mut column = x;
    for character in contents.chars() {
        column = column.saturating_add(put(cells, column, y, character, style));
    }
    column
}

/// The same, stopping before a column it may not write in.
///
/// [`write`] has no such column: it writes until the cell buffer runs out,
/// which is the whole screen. That is right wherever the caller has already
/// worked out the room -- a row wrapped to its width cannot overrun -- and
/// wrong wherever something is put down *after* such a row, because what
/// follows a row that fills its width begins past the end of it. In the
/// transcript that is a path, a fold's mark and a call's state, and the
/// column they were running into is the scrollbar's.
///
/// `stop` is the first column that may not be written, the way a `Rect`'s
/// right is. A wide glyph that would straddle it is not drawn at all: half
/// of one is a cell the terminal advances over and Obelus did not count.
pub fn write_within(
    cells: &mut CellBuffer,
    x: u16,
    y: u16,
    contents: &str,
    style: Style,
    stop: u16,
) -> u16 {
    let mut column = x;
    for character in contents.chars() {
        // The width `put` will report, worked out before it is asked, so
        // the decision is made before the cell is written rather than
        // after.
        let width = u16::try_from(character.width().unwrap_or(0))
            .unwrap_or(0)
            .max(1);
        if column.saturating_add(width) > stop {
            break;
        }
        column = column.saturating_add(put(cells, column, y, character, style));
    }
    column
}

/// Which characters of a row are marked out.
///
/// Two shapes because the questions have two shapes: a fuzzy match lands on
/// scattered characters, while a substring match -- or a selection, which is
/// the same shape and gets the same treatment -- is one run. Every row in
/// Obelus marks them the same way, which is what this is for: a row in a
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
/// same thing that marks a selected row: on any of Obelus's screens, that
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

    // The keys that walk them, drawn where they are walked. The tab
    // arrows rather than the left and right ones: those are the caret's
    // now, and a hint that names the wrong key is worse than none.
    //
    // Measured first, because the room the tabs have is what is left of the
    // row once this is on it.
    let keys = "\u{21e4} \u{21e5}";
    let placed = tabs_placed(area, names, current);
    if let Some(x) = placed.before {
        write(cells, x, area.y, TAB_MORE, dim);
    }
    let mut column = area.x + 1;
    for (index, x, _) in &placed.placed {
        let style = match *index == current {
            true => Style::new()
                .fg(theme.foreground)
                .bg(theme.selected_row_background),
            false => dim,
        };
        column = write_marked(
            cells,
            area,
            *x,
            area.y,
            &format!(" {} ", names[*index].as_ref()),
            style,
            &Marked::plain(),
        );
    }
    if let Some(x) = placed.after {
        column = write(cells, x, area.y, TAB_MORE, dim);
    }
    if let Ok(offset) = u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
        && area.x + offset > column
    {
        write(cells, area.x + offset, area.y, keys, dim);
    }
    column
}

/// What says the row holds more tabs than it had room to draw.
///
/// The same mark the conversation's own row of settings uses for the same
/// fact, because it is the same fact: a row is a window on a list, and a
/// reader who cannot see a thing has to be told it is there.
const TAB_MORE: &str = "\u{2026}";

/// Where each tab that fits is drawn, and where the marks for the ones that
/// did not go.
///
/// Walked once. The drawing goes down this to put the words and a press
/// goes down it to find which word it landed on, so the two cannot disagree
/// about where a tab is.
pub struct Tabs {
    /// Which tab the row starts at.
    pub first: usize,
    /// `(which tab, the cell its word starts at, how wide the word is)`.
    pub placed: Vec<(usize, u16, u16)>,
    /// Where the mark for the tabs behind the row goes, if any are.
    pub before: Option<u16>,
    /// And for the ones ahead of it.
    pub after: Option<u16>,
}

/// How many cells the tabs have, which is the row less the keys that walk
/// them.
fn tabs_room(area: Rect) -> usize {
    let keys = text_width("\u{21e4} \u{21e5}") + 1;
    usize::from(area.width).saturating_sub(keys + 1)
}

/// Which tab the row starts at, so that the one the reader is on is drawn.
///
/// As near the beginning as that allows: a row is a window on a list like
/// any other, and the one thing a window must not do is hide what the keys
/// are moving.
fn tabs_first<Name>(area: Rect, names: &[Name], current: usize) -> usize
where
    Name: AsRef<str>,
{
    let room = tabs_room(area);
    let wide = |at: usize| text_width(&format!(" {} ", names[at].as_ref()));
    let mut first = 0;
    let mut taken = 0;
    for at in (0..=current.min(names.len().saturating_sub(1))).rev() {
        taken += wide(at);
        // Room for the mark that says there are more behind, once there
        // are: the mark is part of what the row has to fit.
        let marked = usize::from(at > 0) * text_width(TAB_MORE);
        if taken + marked > room {
            first = at + 1;
            break;
        }
    }
    first
}

/// Where the tabs sit along the row.
#[must_use]
pub fn tabs_placed<Name>(area: Rect, names: &[Name], current: usize) -> Tabs
where
    Name: AsRef<str>,
{
    let room = tabs_room(area);
    let first = tabs_first(area, names, current);
    let mut column = area.x + 1;
    let mut left = room;
    let before = (first > 0).then(|| {
        let at = column;
        column = column.saturating_add(u16::try_from(text_width(TAB_MORE)).unwrap_or(0));
        left = left.saturating_sub(text_width(TAB_MORE));
        at
    });
    let mut placed = Vec::new();
    let mut after = None;
    for (index, name) in names.iter().enumerate().skip(first) {
        let wide = text_width(&format!(" {} ", name.as_ref()));
        // The last of them keeps room for the mark saying there are more.
        let marked = usize::from(index + 1 < names.len()) * text_width(TAB_MORE);
        if wide + marked > left {
            after = Some(column);
            break;
        }
        left -= wide;
        placed.push((index, column, u16::try_from(wide).unwrap_or(0)));
        column = column.saturating_add(u16::try_from(wide).unwrap_or(0));
    }
    Tabs {
        first,
        placed,
        before,
        after,
    }
}

/// Which tab a point on the tab row asks for.
///
/// The way back out of the arithmetic [`tabs`] goes in by, down the same
/// one walk. Written beside it because every view that has tabs draws them
/// with that one function, so every view that lets a reader press one asks
/// this.
///
/// A press on a mark saying there are more that way asks for the tab just
/// off the row in that direction, which is what pressing it is for: the
/// row is a window, and the mark is the only thing on it saying the window
/// can move.
///
/// `None` for a point that is not on the row, or is past the tabs -- where
/// the keys that walk them are drawn.
#[must_use]
pub fn tab_at<Name>(area: Rect, names: &[Name], current: usize, x: u16, y: u16) -> Option<usize>
where
    Name: AsRef<str>,
{
    if y != area.y || x < area.x {
        return None;
    }
    let placed = tabs_placed(area, names, current);
    let mark = u16::try_from(text_width(TAB_MORE)).unwrap_or(0);
    if let Some(at) = placed.before
        && x >= at
        && x < at.saturating_add(mark)
    {
        return Some(placed.first.saturating_sub(1));
    }
    if let Some(at) = placed.after
        && x >= at
        && x < at.saturating_add(mark)
    {
        return placed.placed.last().map(|(index, _, _)| index + 1);
    }
    placed
        .placed
        .into_iter()
        .find(|(_, at, wide)| x >= *at && x < at.saturating_add(*wide))
        .map(|(index, _, _)| index)
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
    pub chord: obelus_editing::keymap::KeyChord,
    /// A second key that does the same thing, for a pair that shares a word:
    /// `alt+up` and `alt+down` are one act in two directions, and two rows
    /// saying "move it up" and "move it down" is the same sentence twice.
    pub and_also: Option<obelus_editing::keymap::KeyChord>,
    /// What it does, in the one word the foot has room for.
    pub does: Option<&'static str>,
    /// How the key is written, where the chord does not say it.
    ///
    /// For the thing a page does that is not one key: a list narrowed by
    /// typing at it answers to every letter, and naming one of them would
    /// read as "press this one".
    ///
    /// Which is also what keeps it off the card. The foot has to say it
    /// -- a page that is filtered by typing at it looks exactly like one
    /// that is not -- and the card is a table of *chords*, read down its
    /// left column by somebody looking for what to press. A row there
    /// whose key is the word `type` is a row that column cannot answer,
    /// and it is `why_not`'s rule one place along: a printable character
    /// is not something anybody binds, so a table of bindings has no row
    /// for one.
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
    pub const fn common(chord: obelus_editing::keymap::KeyChord, does: &'static str) -> Self {
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
    pub const fn rare(chord: obelus_editing::keymap::KeyChord, does: &'static str) -> Self {
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
    pub const fn or(mut self, chord: obelus_editing::keymap::KeyChord) -> Self {
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

/// How wide a tick is, in cells.
///
/// Two: a Nerd Font draws its glyphs over two columns while the terminal
/// allocates one, so the blank after it belongs to it.
pub const TICK_WIDTH: u16 = 2;

/// What a tick looks like, set and not.
///
/// The plain ones where there is no Nerd Font. The same box in both, with
/// a mark in the second -- a pair that changed shape would put a jog in a
/// column read straight down.
///
/// This was a slider: a four-cell track with a knob at one end of it. What
/// a slider says is *which way it is*, by a position the eye has to measure
/// against a track two cells longer than the knob -- and Obelus draws it in
/// a row of text, at a size where that measurement is a guess. A box is
/// either marked or it is not, which is the same question answered in a
/// glyph.
#[must_use]
pub fn tick(on: bool) -> char {
    match (obelus_icons::enabled(), on) {
        (true, false) => obelus_icons::ui::TODO,
        (true, true) => obelus_icons::ui::TODO_DONE,
        (false, false) => '\u{25a1}',
        (false, true) => '\u{2611}',
    }
}

/// Draws one, and says where whatever follows it goes.
pub fn ticked(cells: &mut CellBuffer, x: u16, y: u16, on: bool, style: Style) -> u16 {
    put(cells, x, y, tick(on), style);
    // And what that cell *is*, for a front end that can draw the shape
    // rather than the glyph standing in for it. The cell above is the
    // whole of the switch in a terminal and nothing depends on this being
    // heard -- see `shapes`.
    shapes::ticked(
        Rect {
            x,
            y,
            width: 1,
            height: 1,
        },
        on,
    );
    x + TICK_WIDTH
}

/// Draws a key in a cap of its own, and says where the next thing goes.
///
/// A blank inside the cap either side, and the cap on a ground a shade off
/// the page. What binds a key to the word beside it is the block it sits
/// in: the row used to separate a key from its word by one blank and one
/// item from the next by three, which are near enough the same gap that the
/// eye could not tell which side of it a word belonged to.
fn capped(cells: &mut CellBuffer, x: u16, y: u16, keys: &str, theme: &Theme) -> u16 {
    let after = write(
        cells,
        x,
        y,
        &format!(" {keys} "),
        Style::new()
            .fg(theme.gutter_current)
            .bg(theme.raised_background),
    );
    // And what those cells *are*, for a front end that can draw the shape
    // rather than only its ground -- see `shapes`. The cells above are the
    // whole of the cap in a terminal and the ground of it in a window;
    // neither depends on this being heard.
    shapes::capped(
        keys,
        Rect {
            x,
            y,
            width: after.saturating_sub(x),
            height: 1,
        },
        theme.raised_background,
        theme.background,
        theme.gutter,
    );
    after
}

/// Says that a key already written here sits in a cap, without touching a
/// cell of it.
///
/// The foot writes its own cap, because a run of cells a shade off the page
/// is the only cap a terminal has and the foot is a row of keys among
/// words. The other two places a key is shown have no ground to give it and
/// need none: on the card and on the keys page the key is *a column*, and
/// being in that column is what says it is a key. So the cells stay exactly
/// as they are, and the shape is said around them -- over the blank either
/// side, which is where a cap's own blanks would have been.
///
/// Which is the whole channel's rule in the one case it is easiest to get
/// wrong: what is said here may not be the only thing saying it.
/// `keys` is how many cells the key itself takes, which the caller
/// measures: a screen that counts a Nerd Font glyph as two cells and one
/// that counts it as one are both here, and a cap measured by the wrong
/// one is a cap that does not fit the key it is round.
fn cap_around(x: u16, y: u16, keys: &str, wide: usize, cap: Color, page: Color, edge: Color) {
    let Ok(width) = u16::try_from(wide + 2) else {
        return;
    };
    if wide == 0 {
        return;
    }
    shapes::capped(
        keys,
        Rect {
            x: x.saturating_sub(1),
            y,
            width,
            height: 1,
        },
        cap,
        page,
        edge,
    );
}

/// How far apart two items on that row sit.
///
/// Three, against the one blank inside the cap and the one before a word:
/// the gap between items has to beat the gaps inside one, or the row is a
/// line of tokens with nothing saying which belongs to which.
const BETWEEN: u16 = 3;

/// How wide a cap is, with the key in it.
///
/// Asked by the item that is about to be drawn and by the pointer at the
/// far end, which were each carrying their own idea of how much a cap adds.
fn cap_width(keys: &str) -> usize {
    text_width(keys) + 2
}

/// How much of the row one hint takes, the gap after it aside.
///
/// The cap, the word and the blank before it, and the box with its own. One
/// answer, because the drawing asks it twice: once to find out whether the
/// item fits, and once by advancing exactly that far.
fn width_of(hint: &Hint) -> usize {
    cap_width(&hint.keys())
        + hint.does.map_or(0, |does| text_width(does) + 1)
        + hint.switched.map_or(0, |_| usize::from(TICK_WIDTH) + 1)
}

/// Marks a row that stopped before it had said everything.
///
/// The mark Obelus cuts text with everywhere else, in the dim ink, in the
/// blank the next item would have started in. Where the pointer to the card
/// is up this says which of the two is the whole list; where there is no
/// card -- a document's foot has none behind it -- it says on its own that
/// the terminal is too narrow for all of this.
fn cut(cells: &mut CellBuffer, x: u16, y: u16, edge: u16, theme: &Theme) {
    if x >= edge {
        return;
    }
    put(
        cells,
        x,
        y,
        '\u{2026}',
        Style::new().fg(theme.gutter).bg(theme.background),
    );
}

/// How much of a panel's edge is not for what is inside it.
///
/// Two each side: the line, and a blank inside it. Text against a border
/// reads as text that ran into it -- and a hover holds a README, whose own
/// fenced blocks are boxes, so without the blank there were two lines
/// touching with nothing between them.
pub const PANEL_INSET: u16 = 2;

/// The frame round something Obelus floats over the reader's work.
///
/// One shape for all of them: the completion list, the signature line, a
/// hover, and the card of every key. They are the same kind of thing --
/// something put over the page for a moment -- and four of them wearing two
/// shapes was a screen where the shape said nothing.
///
/// Rounded, and on the page's own colour. The rounding is not decoration:
/// what a hover holds is a *document*, and a document's own boxes -- a
/// markdown table, a fenced block -- are square, because that is what
/// every markdown renderer draws. A square frame around a square frame is
/// one thing that looks like two; a round one says which of them is
/// Obelus's furniture and which is the reader's text.
///
/// The frame is what says it, and says it alone. It was on a ground a
/// shade off the page as well, which in a window is the colour of its
/// glass -- a grey box over glass that is the page's colour everywhere
/// else, the lists and the settings under it included.
pub fn panel(cells: &mut CellBuffer, area: Rect, theme: &Theme) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let ground = theme.background;
    // What it is put over, said before it is: a window draws the panel as
    // glass inside its frame, and glass is what is behind it seen through
    // -- see `shapes::Joined::Nowhere`.
    shapes::behind(area, shapes::Joined::Nowhere, ground, cells);
    fill(cells, area, Style::new().fg(theme.foreground).bg(ground));
    let edge = Style::new().fg(theme.gutter).bg(ground);
    let (left, right) = (area.x, area.right() - 1);
    let (top, bottom) = (area.y, area.bottom() - 1);
    for (x, y, glyph) in [
        (left, top, '\u{256d}'),
        (right, top, '\u{256e}'),
        (left, bottom, '\u{2570}'),
        (right, bottom, '\u{256f}'),
    ] {
        put(cells, x, y, glyph, edge);
    }
    for x in left + 1..right {
        put(cells, x, top, '\u{2500}', edge);
        put(cells, x, bottom, '\u{2500}', edge);
    }
    for y in top + 1..bottom {
        put(cells, left, y, '\u{2502}', edge);
        put(cells, right, y, '\u{2502}', edge);
    }
}

/// What is left of a panel for the thing inside it.
///
/// One answer, asked by the drawing of the frame and by whatever is laid
/// out to fit in it -- which in a hover's case happens a frame earlier,
/// because markdown cannot be made into rows until there is a width to
/// make them for.
#[must_use]
pub fn inside(area: Rect) -> Rect {
    Rect {
        x: area.x + PANEL_INSET,
        y: area.y + 1,
        width: area.width.saturating_sub(PANEL_INSET * 2),
        height: area.height.saturating_sub(2),
    }
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
/// The common ones that can be pressed at the moment, and
/// [`obelus_editing::keymap::keys_card`] at the right-hand end saying there
/// are more. At the foot rather than beside a
/// title, because a key needs a word and words need room.
///
/// What belongs here is what *this* view does. A key that means the same
/// thing wherever the reader is does not: escape backs out of the nearest
/// thing everywhere in Obelus -- `keymap::why_not` refuses to rebind it for
/// that reason -- so a foot that spends a third of itself saying `Leave` is
/// a row of the reader's screen saying what every other view already said.
/// Those go on the card, which is every key here rather than the ones worth
/// telling.
pub fn foot(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    row_of_keys(cells, area, hints, theme, true);
}

/// The same row, for a view with no card behind it.
///
/// A card of every key is a layer's: something opened over the reader's
/// work, which owns the keyboard while it is up and has to be able to say
/// so. A document is where the reader already was, the card's key over one
/// is whatever that key means everywhere, and this row is the whole of what
/// the view says about itself -- so it points at nothing, and gets the width
/// the pointer would have taken.
pub fn foot_without_a_card(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    row_of_keys(cells, area, hints, theme, false);
}

/// Draws that row, with or without the pointer at the end of it.
fn row_of_keys(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme, card: bool) {
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
    let chord = obelus_editing::keymap::keys_card().label();
    let width = u16::try_from(cap_width(&chord) + 1 + text_width("Keys")).unwrap_or(0);
    let edge = match area.width.checked_sub(width + 2).filter(|_| card) {
        Some(offset) => {
            let after = capped(cells, area.x + offset, y, &chord, theme);
            write(
                cells,
                after + 1,
                y,
                "Keys",
                Style::new().fg(theme.gutter).bg(theme.background),
            );
            area.x + offset
        }
        None => area.x + area.width,
    };

    let mut x = area.x + 2;
    for hint in hints.iter().filter(|hint| hint.common && hint.usable) {
        let keys = hint.keys();
        // Saturating rather than refused: a hint wider than the screen can
        // hold is one that does not fit, which is the same answer the row
        // gives anything else that does not.
        let wanted = u16::try_from(width_of(hint)).unwrap_or(u16::MAX);
        if x.saturating_add(wanted).saturating_add(BETWEEN) > edge {
            // The row stops here, and says so. A foot that ran out of room
            // used to drop the rest of its keys and look exactly like a
            // foot that had said everything it had -- so a reader on a
            // narrow terminal was told a view answered to two keys when it
            // answered to eight, and nothing on the screen disagreed.
            cut(cells, x, y, edge, theme);
            return;
        }
        // The key in a cap and the word out of it: what a reader is looking
        // for down here is which key, and the word is read once to find out
        // that it is the one.
        x = capped(cells, x, y, &keys, theme);
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
            // Bright when it is set and dim when it is not, under a glyph
            // that already says which: a box that is empty and loud is the
            // brightest thing on a row about keys, and it is the one thing
            // here that is off.
            let ink = match on {
                true => theme.foreground,
                false => theme.gutter,
            };
            x = ticked(
                cells,
                x + 1,
                y,
                on,
                Style::new().fg(ink).bg(theme.background),
            );
        }
        x += BETWEEN;
    }
}

/// Every key a view answers to, on a card over it.
///
/// All of them, with what cannot be pressed at the moment greyed rather than
/// left out: what a reader should come away with is that this view has these
/// keys, not that its keys come and go. There is room here for a sentence,
/// which is why the words can be words rather than the one the foot fits.
///
/// Keys, though. What the foot says that is not one -- typing at a list to
/// narrow it -- is at the foot and not here: see `Hint::spelled`.
pub fn keys_card(cells: &mut CellBuffer, area: Rect, hints: &[Hint], theme: &Theme) {
    // The keys, which is not everything at the foot: what is written as
    // a word rather than a chord is not a key -- see `Hint::spelled`.
    let hints: Vec<Hint> = hints
        .iter()
        .filter(|hint| hint.spelled.is_none())
        .copied()
        .collect();
    let hints = hints.as_slice();
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
            .map(|hint| hint.said.or(hint.does).map_or(0, text_width))
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

    // The panel's own ground -- see `panel`.
    let paper = theme.background;
    panel(cells, card, theme);
    let room = inside(card);

    write(
        cells,
        room.x,
        room.y,
        "The keys here",
        Style::new().fg(theme.status_foreground).bg(paper),
    );
    let ground = Style::new().fg(theme.foreground).bg(paper);
    let off = Style::new().fg(theme.gutter).bg(paper);
    for (at, hint) in hints.iter().enumerate() {
        let Ok(offset) = u16::try_from(at) else { break };
        let y = room.y + 2 + offset;
        if y >= room.bottom() {
            break;
        }
        let style = match hint.usable {
            true => ground,
            false => off,
        };
        let keys = hint.keys();
        write(cells, room.x, y, &keys, style);
        // The panel's own ground on both counts: what draws the cap here is
        // its outline and the lip under it, the same as a key on a page
        // that is already the colour the key is.
        cap_around(
            room.x,
            y,
            &keys,
            text_width(&keys),
            paper,
            paper,
            theme.gutter,
        );
        let x = room.x + column;
        if let Some(does) = hint.said.or(hint.does) {
            write(cells, x, y, does, style);
        }
        // What the key is set to is not on the card. A card is read to find
        // out what a key *is* -- what to press for a thing -- and which way
        // one of them happens to be switched right now is a different
        // question, asked of the foot, where the key is offered rather than
        // catalogued.
    }
}

/// What a list says when it has nothing in it.
///
/// One place, so that every empty list in Obelus says its own reason in the
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

/// Held while a test changes, or reads, the one switch that says whether
/// glyphs are drawn.
///
/// The switch is a single atomic for the whole process and cargo runs a
/// crate's tests at once, so a test that flips it and a test that reads it
/// are two tests sharing a variable. The reader saw a flip land between
/// its two looks and compared a badge drawn with glyphs against the mark
/// for a terminal without them -- a failure that appeared about one run in
/// three and only under a full workspace build, which is the shape of
/// thing that gets rerun rather than read.
///
/// A lock and not a rule about which tests may touch it: the two are in
/// different modules and nothing would have stopped a third.
#[cfg(test)]
pub(crate) fn glyphs_held() -> std::sync::MutexGuard<'static, ()> {
    static GLYPHS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that panicked while holding it has poisoned it and has
    // already failed; the next one wants the lock, not a second failure
    // about the first.
    GLYPHS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{
        bar_mark, drop_from_left, drop_from_right, tick, truncate_from_left, truncate_from_right,
    };

    /// A bar's mark reaches the end of its track exactly when the last
    /// row does, and never steps back on the way there.
    ///
    /// Deliberate break: scale by the whole list rather than by how far
    /// its top can go -- `total` in place of `furthest` in `bar_reach`.
    /// The mark then stops short of the bottom on the one screenful
    /// anybody checks it against.
    ///
    /// Not checked against what `scrollbar` draws, because `scrollbar`
    /// works it out with this: one function, so there is nothing for the
    /// two of them to disagree about, and a test that compared them
    /// would be asking the rule what it expects.
    #[test]
    fn a_bar_reaches_the_end_of_its_track_when_the_last_row_does() {
        // Ten rows of a forty-row list: a two-row thumb with eight rows
        // of track to travel, over a top that can go thirty.
        assert_eq!(bar_mark(10, 0, 40), 0);
        assert_eq!(bar_mark(10, 30, 40), 8, "the last screenful is the end");
        // And never back, which is the one thing a mark must not do.
        let mut last = 0;
        for top in 0..=30 {
            let mark = bar_mark(10, top, 40);
            assert!(mark >= last, "the mark stepped back at {top}");
            last = mark;
        }
        assert_eq!(bar_mark(10, 0, 10), 0, "a list that fits");
    }

    /// A control character in what is drawn does not take Obelus down.
    ///
    /// A tab in a tool call's output, a stray escape in what an agent said,
    /// a `\r` in a file somebody opened: Obelus put each of them straight
    /// into a cell, and the frame died -- not where it was written, but
    /// later, when what puts a frame on the screen walked the cells asking
    /// each how wide it was. A control character has no answer to that, and
    /// the whole of Obelus went with the question.
    ///
    /// Which is why this draws a real frame. Every golden test in Obelus
    /// reads the cells straight out of the buffer, and the buffer was
    /// perfectly happy: nothing between writing a cell and a terminal
    /// receiving it ever asked.
    ///
    /// Broken deliberately by putting the character in the cell as it
    /// stands, which is what it did.
    #[test]
    fn a_control_character_does_not_take_the_frame_down() {
        use ratatui::{
            Terminal, backend::TestBackend, buffer::Buffer as CellBuffer, layout::Rect,
            style::Style, widgets::Widget,
        };

        struct Odd;

        impl Widget for Odd {
            fn render(self, _area: Rect, cells: &mut CellBuffer) {
                let mut column = 0;
                for character in "a\tb\u{1b}c\rd".chars() {
                    column += super::put(cells, column, 0, character, Style::new());
                }
            }
        }

        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("a terminal");
        terminal
            .draw(|frame| frame.render_widget(Odd, frame.area()))
            .expect("the frame went down on a control character");
        // And what is there is the words, with a cell each where the
        // control characters were rather than a hole or a shunted row.
        let drawn: String = (0..7)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(drawn, "a b c d", "the words were not drawn around them");
    }

    /// A box says which way it is set by being a different box.
    ///
    /// The whole of what replaced a slider: a slider said it by where its
    /// knob sat, which is a distance to measure, and this is a glyph to
    /// recognise. So the one thing that must be true of the pair is that
    /// they are not the same glyph -- in a terminal with the font and in
    /// one without, because both are drawn.
    ///
    /// Broken deliberately by giving either pair the same character twice:
    /// every switch Obelus draws goes quiet about its state, and only this
    /// says so -- the views' own tests flip a switch and read the box, and
    /// the settings' one flips the glyphs themselves and so reads one of
    /// each pair.
    #[test]
    fn a_box_is_not_the_same_box_when_it_is_marked() {
        let _held = super::glyphs_held();
        for glyphs in [true, false] {
            obelus_icons::use_glyphs(glyphs);
            assert_ne!(
                tick(true),
                tick(false),
                "the box reads the same either way, with glyphs {glyphs}"
            );
        }
        obelus_icons::use_glyphs(false);
    }

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
