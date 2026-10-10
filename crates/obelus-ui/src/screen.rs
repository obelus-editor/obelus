//! What a frame is drawn from: the questions the renderer may ask.

use super::*;

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
    /// Which of its characters the reader has hold of.
    pub held: Option<std::ops::Range<usize>>,
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
    /// What is shown instead of the buffer where the subject is a reading
    /// rather than a file -- a pull request's description, laid out as the
    /// markdown it is.
    ///
    /// Drawn by [`reading::draw`], which draws a markdown file read as one,
    /// so a description and a README look the same because they are drawn
    /// by the same code.
    pub reading: Option<Reading<'a>>,
}

/// A reading in a preview, for the view that draws it.
pub struct Reading<'a> {
    /// Its rows, laid out.
    pub rows: &'a [obelus_row::Row],
    /// The first of them on screen.
    pub top: usize,
    /// A row that says something is on its way, with two blank cells at
    /// its head for the mark that turns to be drawn in.
    ///
    /// Drawn here rather than laid out in the row: the rows are laid out
    /// once and kept, and a mark in them would stand still.
    pub turning: Option<usize>,
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
    /// How many pieces of the agent's background work in this conversation
    /// are still going, and how many have ended -- or nothing, where it has
    /// told Obelus of none here, or does not speak of such work at all.
    fn background_tasks(&self) -> Option<(usize, usize)>;
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
    /// What one of a setting's words is called on screen, where it is
    /// called something other than the word.
    ///
    /// Asked of the application because the workflows' titles are written
    /// in the files beside what each one hands an agent, which are its.
    fn called(&self, key: &str, word: &str) -> Option<std::borrow::Cow<'static, str>>;
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
    /// The terminal, while the reader is in one.
    fn terminal(&self) -> Option<&obelus_terminal::Terminal>;
    /// Whether each note has a conversation, by the note's place in the
    /// list -- which is what a row of the notes names.
    fn talked_about(&self) -> Vec<obelus_component::todo::Talked>;
    /// The hunk the reader has opened in place, if any.
    fn opened_hunks(&self) -> Vec<LineNumber>;
    /// How far along the welcome screen's colours have travelled, in ticks.
    fn phase(&self) -> u32;
    /// The cell the pointer was last reported over, if it has been.
    fn pointer(&self) -> Option<(u16, u16)>;
    /// What is being asked, while Obelus is asking which project. `None`
    /// on every start that was told one.
    fn choosing(&self) -> Option<Choosing>;
    /// What could finish the path being named, while one is being named.
    ///
    /// An ordinary compact list, the way the agent's own commands are:
    /// the rows and the chosen row are the picker's, and the box below it
    /// owns the keys.
    fn naming_list(&self) -> Option<&Picker>;
    /// Which version of Obelus this is.
    fn version(&self) -> &str;
    /// The version of a newer Obelus, where one is out and the reader
    /// wants to be told.
    fn newer_release(&self) -> Option<&str>;
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
    /// Whether the server behind the file being read is busy with something.
    fn server_busy(&self) -> bool;
    /// The chat, by name, and where this window stands with it -- `None`
    /// where it is not the window the chat talks to.
    fn remote(&self) -> Option<(&'static str, obelus_remote::State)>;
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
    /// What the conversation being read is called: the agent's name for
    /// it, or the note it is about, or the reader's first words.
    fn what_this_conversation_is_called(&self) -> Option<String>;
    /// Where Obelus was started, and the root every path is shown relative to.
    fn working_directory(&self) -> &Path;
    /// Which branch that tree has checked out, where it is a repository.
    fn head(&self) -> Option<&obelus_git::Head>;
    /// Whether that tree has gone from disk.
    fn tree_has_gone(&self) -> bool;
}
