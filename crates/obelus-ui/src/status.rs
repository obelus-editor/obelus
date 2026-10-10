//! The status bar: the file on the left, the cursor on the right.
//!
//! **A list open over anything owns the status row.** It is the thing taking
//! the keys and holding the caret, so its prompt is drawn before the
//! settings' filter or the conversation's own row. A row belonging to what
//! is behind the list is a prompt with somebody else's words in it, and the
//! caret sitting in it says the words are being typed there. So whose row
//! this is is one question, asked of whatever is nearest the reader
//! (`Layer`) the way the caret asks it -- a dialog draws the row at its own
//! foot, and a question on the status row can open over a list.

use std::{ops::Range, path::Path};

use obelus_buffer::Buffer;
use obelus_component::{layers::Layer, picker::Picker};
use obelus_lsp::ServerState;
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::{
    Marked, Screen, fill, relative_to, truncate_from_left, truncate_from_right, write, write_marked,
};

/// The status region.
pub struct StatusView<'a> {
    buffer: Option<&'a Buffer>,
    /// Something Obelus has to tell the reader, until their next key.
    ///
    /// What a language server says it is doing used to share this. It is
    /// not the same kind of thing: a note is a sentence about something
    /// that just happened, and a server's progress is a few hundred
    /// messages over a cold start, changing between the file's name and
    /// the cursor's position while a reader tries to read both. What is
    /// left of it is the badge, which turns.
    middle: Option<&'a str>,
    /// How many rows the rendering has, when one is on screen.
    rows: Option<usize>,
    /// The server for this file, and what it is doing.
    ///
    /// On screen the whole time, unlike `middle`, which comes and goes. A
    /// reader whose jump did nothing needs to know whether anything was
    /// listening, and that question is asked *after* the answer disappoints:
    /// a marker that had come and gone would not be there to answer it.
    server: Option<(&'static str, ServerState)>,
    /// Whether that server is busy, for the mark that turns.
    ///
    /// The whole of what the row says about it now, and the reason the row
    /// has to say anything: an empty answer during indexing and an empty
    /// answer about a symbol with no definition are the same message on
    /// the wire, and this is what tells them apart.
    busy: bool,
    /// Where the animation has got to, for that mark.
    phase: u32,
    /// The chat this machine can be reached from, and where it stands.
    ///
    /// On the row for the reason the server is: it is a fact about the
    /// whole window that stays true while the reader reads, and the
    /// question it answers -- will a reply from my phone get here -- is
    /// asked when one has not.
    remote: Option<(&'static str, obelus_remote::State)>,
    /// When a question is being asked, the row is the question.
    ///
    /// The one dialog that is still kept here, because it is the one whose
    /// room *is* this row. The others -- a picker, a list of names, the
    /// settings -- draw the row at their own foot and are handed to the
    /// writer that draws it, so there was nothing left to keep them for.
    prompt: Option<&'a obelus_component::prompt::Prompt>,
    /// The directory that question would put a file in, where it is the
    /// question that makes one.
    making_in: Option<String>,
    /// Whether what Obelus has to say is about something that would not
    /// go, which is the ink it is drawn in.
    note_is_wrong: bool,
    /// The notes, while they are what is being read.
    notes: Option<&'a obelus_component::todo::TodoView>,
    terminal: Option<&'a obelus_terminal::Terminal>,
    /// Which of them is nearest the reader, and so whose row this is.
    ///
    /// The same question the caret asks. Three of these can be on screen
    /// at once and only one of them is taking the keys: a question asked
    /// on the status row opens *over* a list rather than closing it, so
    /// the list is not always the nearest thing any more.
    /// What is being asked, where Obelus is asking which project.
    ///
    /// The row is that question's then. Here rather than drawn at a
    /// dialog's own foot because the chooser is not a dialog over
    /// anything -- it is the page, and this is the page's row.
    choosing: Option<crate::Choosing>,
    nearest: Option<Layer>,
    /// Whether what is typed goes over what is under the cursor.
    ///
    /// A mode, so it is said where the mode goes: a reader who cannot see
    /// which one they are in finds out by typing, and by then a character
    /// is gone. The caret says it too where Obelus draws its own -- a
    /// block rather than a bar -- and a terminal's caret is the terminal's,
    /// which is why the word is the half that works in both.
    replacing: bool,
    /// Whether the reader's own keys change no file.
    ///
    /// Said where the mode goes, because it is what typing will do: a
    /// letter that goes nowhere has to have a reason on screen.
    read_only: bool,
    theme: &'a Theme,
    troubles: &'a [obelus_lsp::trouble::Trouble],
    working_directory: &'a Path,
    /// Which branch the tree is on, where it is a repository.
    ///
    /// Left of the file, because it is the widest fact on the row: the
    /// path is shown relative to this tree, so what the path is *of* comes
    /// before it. Nothing at all outside a repository, and no column spent
    /// -- a row saying something there would be answering a question that
    /// does not arise.
    head: Option<&'a obelus_git::Head>,
}

impl<'a> StatusView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Self {
        Self {
            buffer: app.current_buffer(),
            middle: app.note(),
            rows: app.rendered_rows(),
            server: app.server_state(),
            busy: app.server_busy(),
            phase: app.phase(),
            remote: app.remote(),
            troubles: app.troubles(),
            prompt: app.prompt(),
            making_in: app.making_in(),
            note_is_wrong: app.note_is_wrong(),
            notes: app.notes(),
            terminal: app.terminal(),
            choosing: app.choosing(),
            nearest: app.layers().nearest(),
            replacing: app.replacing(),
            read_only: app.config().read_only,
            theme: app.theme(),
            working_directory: app.working_directory(),
            head: app.head(),
        }
    }
}

impl Widget for StatusView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let style = Style::new()
            .bg(self.theme.background)
            .fg(self.theme.status_foreground);

        // The whole row, so nothing of whatever was there before shows
        // through the gaps. The page's own colour, not a band of its own:
        // the rule above the row already says the row is a different
        // subject from the file, and saying it twice makes the heaviest
        // thing on the screen out of the smallest part of it. The
        // conversation's row had been drawn this way for a while, which is
        // what made the difference visible.
        fill(cells, area, style);

        // Whose row this is: whatever is nearest the reader, which is the
        // thing taking the keys and the thing the caret is in. One match
        // on one value, the way the caret asks it, rather than a chain
        // whose order stands for an assumption -- the chain used to put
        // the list first because nothing could open over it, and a
        // question on the status row can.
        // Before anything about a file, because there is none and cannot
        // be one: until this is answered there is no project for a file
        // to be in.
        if let Some(choosing) = &self.choosing {
            self.render_choosing(choosing, area, cells, style);
            return;
        }

        match self.nearest {
            // The whole row is the question. Nothing else on it: a file
            // name beside a half-typed line number is two things asking to
            // be read at once.
            //
            // Which the directory a new file would go in is not. It is
            // part of the question rather than a second thing beside it:
            // the answer is a path, a path is relative to something, and
            // the row is the only place that can say what. Left unsaid, a
            // reader standing in the project's own root is shown a blank
            // line and a file that lands they know not where -- which is
            // how this was noticed.
            Some(Layer::Prompt) => {
                if let Some(prompt) = self.prompt {
                    let line = prompt.line();
                    write_marked(
                        cells,
                        area,
                        area.x + 1,
                        area.y,
                        &line,
                        style,
                        &held_after(prompt.held(), &prompt.kind().label(), self.theme),
                    );
                    self.render_landing(&line, area, cells, style);
                }
            }
            // Nothing over what is being read, so the row is about that.
            //
            // Which is every case left. A dialog draws the row at its own
            // foot, so this is never reached with one showing -- `draw`
            // does not call this at all then. The arms that were here drew
            // a picker's prompt, a list-being-built's and the settings'
            // filter, each of them a dialog's row written by Obelus.
            _ => {
                if let Some(notes) = self.notes {
                    self.render_notes(notes, area, cells, style);
                } else if let Some(terminal) = self.terminal {
                    self.render_terminal(terminal, area, cells, style);
                } else if let Some(buffer) = self.buffer {
                    self.render_file(buffer, area, cells, style);
                } else if let Some(note) = self.middle {
                    // Nothing open, so the row has nothing else to say --
                    // and what Obelus has just said still has to reach
                    // somebody. It was drawn only beside a file's name,
                    // which left the one reader most likely to be told
                    // something watching a key do nothing: `ob
                    // some-directory` opens on the welcome screen, and
                    // making the first file in a project is a command
                    // offered right there.
                    //
                    // Left of the row and in its own ink, which is what a
                    // conversation does with the same sentence: beside a
                    // path it is an aside and goes in the dim one, and
                    // alone on the row it is the row. Truncated rather
                    // than dropped, for the same reason -- there is
                    // nothing here it could be crowding.
                    let end = self.remote_at_end(area, cells, style);
                    write(
                        cells,
                        area.x + 1,
                        area.y,
                        &truncate_from_right(note, end.saturating_sub(1)),
                        self.wrong_ink().map_or(style, |ink| style.fg(ink)),
                    );
                } else {
                    self.render_project(area, cells, style);
                }
            }
        }
    }
}

/// The blank between a word that says what something is and the thing.
///
/// Two, which is what every other pair of them on this row uses.
const LABEL_GAP: u16 = 2;

/// How few columns of the path are still worth drawing.
///
/// What the branch may not take: a path cut below this is a row that has
/// said which branch and not which file, and the file is what the reader is
/// looking at.
const PATH_AT_LEAST: usize = 8;

/// The branch the tree is on, as the left of the file row says it.
///
/// Empty outside a repository, so the row spends no column on a question
/// that does not arise. A trailing pair of blanks is the gap before the
/// path, and what follows the glyph is `after_a_glyph`'s.
///
/// A branch keeps its own spelling, because it is a name the reader wrote.
/// `Detached` is not a name but Obelus saying something, so it is written
/// as copy -- and it is said rather than left blank because a tree with no
/// branch checked out and no tree at all are two different things, and a
/// row silent about both tells a reader neither.
#[must_use]
pub(crate) fn branch_badge(head: Option<&obelus_git::Head>) -> String {
    let Some(head) = head else {
        return String::new();
    };
    let said = match head {
        obelus_git::Head::Branch(name) => name.as_str(),
        obelus_git::Head::Detached => "Detached",
    };
    match obelus_icons::enabled() {
        true => format!(
            "{}{}{said}  ",
            obelus_icons::ui::BRANCH,
            crate::after_a_glyph()
        ),
        false => format!("{said}  "),
    }
}

/// Which server is behind the current file, and whether it is really there.
///
/// The name is part of the point: a mark on its own says something is running
/// without saying what, which is not an answer a reader can act on. The
/// trailing space is the gap before the cursor position.
#[must_use]
fn server_badge(server: Option<(&'static str, ServerState)>, busy: Option<u32>) -> String {
    server
        .map(|(name, state)| {
            // Busy beats the state it is in, because a server that is
            // reading the project is a server that is there: what the
            // reader wants off this mark while it turns is that an empty
            // answer may be about the reading rather than about the
            // symbol. Braille, which needs no particular font -- it is
            // drawn whether or not glyphs are, like every other mark in
            // Obelus that turns.
            if let Some(phase) = busy {
                return format!(
                    "{}{}{name} ",
                    crate::spinning(phase),
                    crate::after_a_glyph()
                );
            }
            match obelus_icons::enabled() {
                // A blank to read by, after the one a terminal's glyph
                // bleeds into -- see `after_a_glyph`.
                true => format!("{}{}{name} ", state.glyph(), crate::after_a_glyph()),
                false => format!("{} {name} ", state.mark()),
            }
        })
        .unwrap_or_default()
}

/// The chat, by name, and a mark for where it stands -- in the window it
/// talks to, and in every window heard in it through that one: their
/// conversations are in the chat too, and the words for what went wrong
/// are the holder's alone.
///
/// Turning while it connects, and again while it connects again, which is
/// a wait with an end and the one thing on the row that is not settled.
/// Which of the wrong things it is goes unsaid here: the row says it in
/// words once, as it goes wrong, and the settings page says it for as long
/// as it stays wrong -- this is the one cell that says to go and look.
#[must_use]
fn remote_badge(remote: Option<(&'static str, obelus_remote::State)>, phase: u32) -> String {
    let Some((name, state)) = remote else {
        return String::new();
    };
    let mark = match state {
        state if state.connected() => '\u{25cf}',
        state if state.wrong() => '\u{2715}',
        obelus_remote::State::Connecting => crate::spinning(phase),
        _ => '\u{25cb}',
    };
    format!("{mark} {name}  ")
}

/// The chat's mark, and the ink it is written in, for every status row
/// that carries it.
///
/// Every one, because the row the reader is on when the connection goes is
/// whichever they happen to be on: a mark that only a file's row drew was
/// one a reader in a conversation, on the notes or on the welcome screen
/// never saw -- and those are where a reader working from a chat spends
/// their time.
#[must_use]
pub(crate) fn remote_mark(
    remote: Option<(&'static str, obelus_remote::State)>,
    phase: u32,
    theme: &Theme,
) -> Option<(String, Color)> {
    let (_, state) = remote?;
    let ink = match state {
        state if state.connected() => theme.status_foreground,
        state if state.wrong() => theme.status_stale,
        _ => theme.gutter,
    };
    Some((remote_badge(remote, phase).trim_end().to_string(), ink))
}

/// How many things are wrong with the file, by kind.
///
/// Marks rather than words: the row is on screen the whole time and this
/// is the part of it that is usually empty. A reader who wants the words
/// has the list.
#[must_use]
fn wrong_badge(troubles: &[obelus_lsp::trouble::Trouble]) -> String {
    use obelus_lsp::trouble::Severity;

    let mut badge = String::new();
    for severity in [Severity::Error, Severity::Warning] {
        let many = troubles
            .iter()
            .filter(|trouble| trouble.severity == severity)
            .count();
        if many > 0 {
            badge.push_str(&format!("{}{many} ", severity.mark()));
        }
    }
    // The quiet two are counted together and only where there is nothing
    // louder: a hint is not what a reader needs told about.
    if badge.is_empty() && !troubles.is_empty() {
        badge.push_str(&format!(
            "{}{} ",
            Severity::Information.mark(),
            troubles.len()
        ));
    }
    badge
}

/// The worst thing a server said about the file, for the colour of the
/// count.
fn worst(troubles: &[obelus_lsp::trouble::Trouble]) -> obelus_lsp::trouble::Severity {
    troubles
        .iter()
        .map(|trouble| trouble.severity)
        .min()
        .unwrap_or(obelus_lsp::trouble::Severity::Hint)
}

/// The colour that says which state it is.
///
/// Ready is the ordinary status colour: a reader should not have to learn a
/// colour to know that things are normal. The other two are the two ways
/// things are not.
fn badge_colour(server: Option<(&'static str, ServerState)>, theme: &Theme) -> Color {
    match server.map(|(_, state)| state) {
        Some(ServerState::Ready) => theme.status_foreground,
        Some(ServerState::Gone) => theme.status_stale,
        Some(ServerState::Starting) | None => theme.gutter,
    }
}

/// A row of the status bar that is typed into, whatever is typing into it.
///
/// A magnifier and then what has been typed: every picker filters by
/// typing, the settings filter by typing, and they differ in what they list
/// rather than in what typing does. A question asked before the typing --
/// an agent asking to be allowed something -- goes in front of it.
///
/// One function for the text and the caret, because they are one fact: a
/// caret worked out separately is a caret that drifts from the words.
fn typed(question: Option<&str>, words: &str) -> String {
    let asked = question.map(|question| format!("{question}  "));
    let asked = asked.unwrap_or_default();
    if obelus_icons::enabled() {
        format!(
            "{asked}{}{}{words}",
            obelus_icons::ui::PROMPT,
            crate::after_a_glyph()
        )
    } else {
        format!("{asked}> {words}")
    }
}

/// What a row that is typed into says before anything has been.
///
/// After the prompt glyph, where the typing will go, and in the dim ink
/// an aside on this row is written in -- so it reads as the row telling
/// the reader what to do rather than as something already typed. The
/// first character replaces it, because by then the reader knows.
///
/// One writer for every row that is typed into, which is the rule the
/// glyph in front of it already follows: a list, the settings' filter and
/// the list of names all say it the same way, and what differs is only the
/// words each of them gives.
///
/// The caret is not moved by it. `typed_caret` measures what was typed,
/// and a hint counted in would put the caret at the end of words nobody
/// wrote.
fn hint(
    cells: &mut CellBuffer,
    area: Rect,
    question: Option<&str>,
    hint: Option<&str>,
    style: Style,
    theme: &Theme,
) {
    // Whether there is anything to say is the caller's, and every one of
    // them already knows: a list and a page of settings each hold the very
    // box this stands in for. Asked again here, the two would be free to
    // differ -- and they did, which is how a test about the first
    // character taking it away passed with that answer broken.
    let Some(hint) = hint else {
        return;
    };
    let after = usize::from(typed_inset(question));
    let Ok(offset) = u16::try_from(after) else {
        return;
    };
    if area.x + offset >= area.right() {
        return;
    }
    let room = usize::from(area.right() - area.x - offset);
    write(
        cells,
        area.x + offset,
        area.y,
        &truncate_from_right(hint, room),
        style.fg(theme.gutter),
    );
}

/// Says a row that is typed into is still working on what was typed.
///
/// After the words and never in front of them: what the reader is reading
/// is what they typed, and a mark that pushed it along would move the text
/// under their eyes every time an answer started or landed. After them it
/// also leaves the caret where it is -- `typed_caret` measures what comes
/// *before* the caret, so nothing appended can reach it.
///
/// A mark that turns and nothing else. The words for it belong to whoever
/// is waiting and are said where their list says them; what this row adds
/// is the one thing a list cannot say about itself from off screen, which
/// is that the waiting is still going on.
///
/// Here rather than in the picker because the next thing that types and
/// waits will want it too -- a search of a whole project is a query with a
/// walk behind it, and a second mark drawn a second way would be two
/// answers to "is this still going".
pub fn still_working(
    cells: &mut CellBuffer,
    area: Rect,
    question: Option<&str>,
    words: &str,
    standing_in: Option<&str>,
    phase: u32,
    theme: &Theme,
) {
    // After whatever is on the row where the typing goes, which with
    // nothing typed is the words saying what typing would do: a mark drawn
    // at the caret's own column would be drawn on top of them, and a
    // spinner in the middle of a word reads as neither.
    let said = match words.is_empty() {
        true => standing_in.unwrap_or(words),
        false => words,
    };
    let after = 1usize
        .saturating_add(text_width(&typed(question, said)))
        .saturating_add(1);
    let Ok(offset) = u16::try_from(after) else {
        return;
    };
    if area.x + offset >= area.right() {
        return;
    }
    write(
        cells,
        area.x + offset,
        area.y,
        &crate::spinning(phase).to_string(),
        Style::new().fg(theme.gutter).bg(theme.background),
    );
    crate::shapes::spun(area.x + offset, area.y);
}

/// The row a list is typed into: what was typed, what is held, and
/// whether the list has answered for it yet.
///
/// A function of its own rather than a method, because the row is about
/// the *picker* and nothing else on the status bar: what the view around
/// it holds -- a buffer, a server, a note -- has no part in it, and a test
/// of this should not have to build one.
pub fn prompt_row(
    picker: &Picker,
    area: Rect,
    cells: &mut CellBuffer,
    style: Style,
    theme: &Theme,
    phase: u32,
) {
    // A list that is only read is not typed into, so its row is not a box:
    // what the list is, and at the far end the one key that does anything
    // here. No prompt and no caret -- two marks saying "type here" over a
    // list nothing can be typed into would be two lies.
    if picker.is_only_read() {
        read_row(picker, area, cells, style, theme);
        return;
    }
    let said = picker.query();
    let line = typed(picker.question(), &said);
    // What is held, marked where it is: the prefix in front of the
    // query is not part of what was typed, so the run moves right by
    // however wide that is.
    let marked = held_after(picker.query_held(), &typed(picker.question(), ""), theme);
    write_marked(cells, area, area.x + 1, area.y, &line, style, &marked);
    hint(
        cells,
        area,
        picker.question(),
        picker.invitation(),
        style,
        theme,
    );
    // And that the list has not answered for what is in it yet -- where it
    // has rows to show. An empty one says so on its own line, with the
    // mark in front of it, and the reader is looking there.
    if picker.is_filling().is_some() && picker.nothing_to_show().is_none() {
        still_working(
            cells,
            area,
            picker.question(),
            &said,
            picker.invitation(),
            phase,
            theme,
        );
    }
    if let Some(tally) = picker.how_much() {
        how_much(
            cells,
            area,
            picker.question(),
            &said,
            picker.invitation(),
            tally,
            theme,
        );
    }
}

/// What a list says about how much of it there is, at the far end of the
/// row it is typed into, and the key that does something about it after
/// the words.
///
/// Dropped whole where it would reach what was typed, the mark after it,
/// or the words that stand in for nothing typed: half a count is a wrong
/// count, and what the reader typed is the one thing on the row that is
/// theirs. Except the words before a key, which lose their end instead --
/// they are why the key is there, and the key is what the reader can do.
fn how_much(
    cells: &mut CellBuffer,
    area: Rect,
    question: Option<&str>,
    words: &str,
    standing_in: Option<&str>,
    tally: &obelus_component::picker::Tally,
    theme: &Theme,
) {
    let said = match words.is_empty() {
        true => standing_in.unwrap_or(words),
        false => words,
    };
    // What is already on the row: the typing, then the blank, the mark and
    // the blank after it, then two more so the two never read as one.
    let taken = 1usize
        .saturating_add(text_width(&typed(question, said)))
        .saturating_add(5);
    let key = tally.key.as_ref().map_or(0, |(key, does)| {
        3 + crate::cap_width(key) + 1 + text_width(does)
    });
    let room = usize::from(area.width)
        .saturating_sub(1)
        .saturating_sub(taken);
    let words = match (text_width(&tally.words).saturating_add(key) <= room, key) {
        (true, _) => tally.words.clone(),
        // Room for the key and a few words of why, or for nothing.
        (false, key) if key > 0 && room >= key + 8 => {
            crate::truncate_from_right(&tally.words, room - key)
        }
        (false, _) => return,
    };
    let Ok(wanted) = u16::try_from(text_width(&words).saturating_add(key)) else {
        return;
    };
    let quiet = Style::new().fg(theme.gutter).bg(theme.background);
    let x = area.right().saturating_sub(1).saturating_sub(wanted);
    let after = write(cells, x, area.y, &words, quiet);
    if let Some((key, does)) = &tally.key {
        let after = write(cells, after, area.y, " \u{b7} ", quiet);
        let after = crate::capped(cells, after, area.y, key, theme);
        write(cells, after + 1, area.y, does, quiet);
    }
}

/// The row under a list that is only read: what it is, and how to let it
/// go.
///
/// Escape, which is otherwise the one key no foot names -- it gives up on
/// the nearest thing everywhere, so saying so is saying what every view
/// says. Named here because it is the only key this list answers to, and
/// a row with no box and no key on it is a row that does not say how to
/// get the screen back. The cap is the foot's own, so a key looks the same
/// here as on every page.
fn read_row(picker: &Picker, area: Rect, cells: &mut CellBuffer, style: Style, theme: &Theme) {
    let key = obelus_editing::keymap::KeyChord::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    )
    .label();
    let does = "Close";
    // The key first, so the words cannot take it off the row.
    let width = u16::try_from(crate::cap_width(&key) + 1 + text_width(does)).unwrap_or(0);
    let at = area.right().saturating_sub(width + 2);
    let room = at.saturating_sub(area.x + 1).saturating_sub(2);
    if let Some(what) = picker.question() {
        let said = crate::truncate_from_right(what, usize::from(room));
        write(cells, area.x + 1, area.y, &said, style);
    }
    if at > area.x {
        let after = crate::capped(cells, at, area.y, &key, theme);
        write(
            cells,
            after + 1,
            area.y,
            does,
            Style::new().fg(theme.gutter).bg(theme.background),
        );
    }
}

/// How many cells sit in front of what was typed on a status row.
///
/// The mark or the question, and the blank after it. Shared with whoever
/// turns a click into a place in the line: the renderer decides where the
/// text starts, so it is the renderer that has to say.
#[must_use]
pub fn typed_inset(question: Option<&str>) -> u16 {
    // The one column every status row is drawn in from, and then the
    // prefix itself.
    let inset = 1usize.saturating_add(text_width(&typed(question, "")));
    u16::try_from(inset).unwrap_or(u16::MAX)
}

/// Which column the caret belongs in on a row that is typed into.
///
/// `at` is how many characters of `words` are in front of the caret, which
/// is not the same as how many there are: a caret that could only ever sit
/// at the end was the whole of what made these boxes unable to reach the
/// middle of what a reader had typed.
#[must_use]
pub fn typed_caret(question: Option<&str>, words: &str, at: usize) -> u16 {
    let before: String = words.chars().take(at).collect();
    let caret = 1usize.saturating_add(text_width(&typed(question, &before)));
    u16::try_from(caret).unwrap_or(u16::MAX)
}

/// Which column the caret belongs in after a filter's text.
#[must_use]
pub fn filter_caret(query: &str, at: usize) -> u16 {
    typed_caret(None, query, at)
}

/// The word in front of the box that asks which project: what it narrows,
/// or that it is a path being named.
#[must_use]
pub const fn choosing_question(naming: bool) -> &'static str {
    match naming {
        true => "Open",
        false => "Filter",
    }
}

/// What is held in a box, marked on the row it is drawn in -- where
/// whatever is in front of the box (`ahead`) moves it right by however
/// many characters that is.
fn held_after(held: Option<Range<usize>>, ahead: &str, theme: &Theme) -> Marked<'static> {
    match held {
        Some(held) => {
            let ahead = ahead.chars().count();
            Marked::run(
                held.start + ahead..held.end + ahead,
                theme.selection_background,
            )
        }
        None => Marked::plain(),
    }
}

/// The same, in front of a question's answer.
#[must_use]
pub fn answer_inset(prompt: &obelus_component::prompt::Prompt) -> u16 {
    let inset = 1usize.saturating_add(text_width(&prompt.kind().label()));
    u16::try_from(inset).unwrap_or(u16::MAX)
}

/// Which column the caret belongs in while a question is being asked.
///
/// Shared with the renderer, like the picker's, so the text and the caret
/// cannot disagree about where the answer ends.
#[must_use]
pub fn answer_caret(prompt: &obelus_component::prompt::Prompt) -> u16 {
    // The label and what is in front of the caret, which is not all of
    // what was typed: an answer that starts with the old name in it is
    // one a reader edits, and the caret goes where they put it.
    let said = prompt.text();
    let before: String = said.chars().take(prompt.caret()).collect();
    let caret = 1usize.saturating_add(text_width(&prompt.kind().label()) + text_width(&before));
    u16::try_from(caret).unwrap_or(u16::MAX)
}

/// Which column of the status row the caret belongs in.
///
/// The terminal draws the caret, so this is only where to tell it to put it.
/// Shared with the renderer so the text and the caret cannot disagree.
#[must_use]
pub fn prompt_caret(picker: &Picker) -> u16 {
    typed_caret(picker.question(), &picker.query(), picker.query_caret())
}

impl StatusView<'_> {
    /// The file on the left, the cursor position on the right, and a marker
    /// between them when the file can no longer be read.
    /// Where the file a question is asking about would go, at the row's
    /// far end.
    ///
    /// In the dim ink the row's other aside is written in, because that is
    /// what it is: what the reader is typing is the answer, and this is
    /// what the answer means. The same ink for both would be two things
    /// reading as one sentence.
    ///
    /// Dropped whole where the two do not both fit, which is the rule the
    /// rest of this row follows: the answer is what the reader is looking
    /// at, and half a directory is worse than none.
    fn render_landing(&self, line: &str, area: Rect, cells: &mut CellBuffer, style: Style) {
        let Some(landing) = self.making_in.as_deref() else {
            return;
        };
        let landing_width = text_width(landing);
        // A column of padding at the far end, as the position has, and two
        // between the answer and this so the pair do not read as one path.
        let start = usize::from(area.width)
            .saturating_sub(landing_width)
            .saturating_sub(1);
        if start < 1usize.saturating_add(text_width(line)).saturating_add(2) {
            return;
        }
        if let Ok(offset) = u16::try_from(start) {
            write(
                cells,
                area.x + offset,
                area.y,
                landing,
                style.fg(self.theme.gutter),
            );
        }
    }

    /// The ink a note is written in where it is about something that
    /// would not go.
    ///
    /// `Saved` and `Not saved` are the same words in the same place, and
    /// the row had nothing else to tell them apart with -- so a reader
    /// glancing at it read them the same.
    ///
    /// `None` for a note that reports, because what *that* is written in
    /// depends on what else is on the row: a dim aside beside the file's
    /// own name, and the row's own ink where it is the row. Which is a
    /// thing each caller knows and this does not.
    fn wrong_ink(&self) -> Option<ratatui::style::Color> {
        // Through `colour_for`, which is where the count of what is wrong
        // with the file gets the same red a few columns along: one door,
        // so the two things on this row that mean "wrong" cannot come to
        // mean it in two shades.
        self.note_is_wrong.then(|| {
            self.theme
                .colour_for(Some(obelus_text::kind::SyntaxKind::Error))
        })
    }

    fn render_file(&self, buffer: &Buffer, area: Rect, cells: &mut CellBuffer, style: Style) {
        // Sits with the path rather than with the cursor position, because it
        // is a fact about the file. In its own colour: the whole point is that
        // it is noticed without being looked for.
        // The mode, then staleness. Both sit with the path rather than with
        // the cursor position, because both are facts about the file rather
        // than about where you are in it -- and a screen showing something
        // other than the file has to say so.
        let mut marker = String::new();
        // Which commit this came from, where the mode and the staleness go:
        // they are all the same kind of fact -- what is on screen is not
        // simply the file at this path -- and the one thing a reader must
        // not have to wonder about.
        if let Some(at) = buffer.content().short() {
            marker.push_str(&format!("  {at}"));
        }
        if let Some(mode) = buffer.mode().name() {
            marker.push_str(&format!("  {mode}"));
        }
        if self.replacing {
            marker.push_str("  Replacing");
        }
        if self.read_only {
            marker.push_str("  Read only");
        }
        // Unsaved work, and the file having moved under it. Both are the
        // same kind of fact as the three above -- what is on screen is not
        // simply the file at this path -- and the second is the one a
        // reader must know before they press save.
        if buffer.is_dirty() {
            marker.push_str(&match obelus_icons::enabled() {
                true => format!(
                    " {}{}unsaved",
                    obelus_icons::ui::UNSAVED,
                    crate::after_a_glyph()
                ),
                false => " [unsaved]".to_string(),
            });
        }
        let away = match buffer.on_disk() {
            obelus_buffer::Disk::Unchanged => None,
            obelus_buffer::Disk::Written => Some("Moved"),
            // Said in its own word. A reader who is told their file
            // "moved" when it is gone will go looking for it.
            obelus_buffer::Disk::Deleted => Some("Deleted"),
        };
        if let Some(away) = away {
            marker.push_str(&match obelus_icons::enabled() {
                true => format!(
                    " {}{}{away}",
                    obelus_icons::ui::STALE,
                    crate::after_a_glyph()
                ),
                false => format!(" [{away}]"),
            });
        }
        if buffer.is_stale() {
            marker.push_str(&match obelus_icons::enabled() {
                true => format!(
                    " {}{}stale",
                    obelus_icons::ui::STALE,
                    crate::after_a_glyph()
                ),
                false => " [stale]".to_string(),
            });
        }
        let marker = marker.as_str();
        let marker_width = text_width(marker);

        // What the server is doing, between the file and the position. It is
        // there so that an empty answer during indexing can be told from an
        // empty answer about a symbol with no definition: on the wire they
        // are the same message, and this is the only thing that says which.
        let working = self
            .middle
            .map(|what| format!("{what} "))
            .unwrap_or_default();
        let working_width = text_width(&working);

        let badge = server_badge(self.server, self.busy.then_some(self.phase));
        let badge_width = text_width(&badge);
        let remote = remote_badge(self.remote, self.phase);
        let remote_width = text_width(&remote);

        // What is wrong with the file, as a count of each kind: a reader
        // who has not looked at the list still has to know there is one.
        // Left of the server badge, because it is about the file and the
        // badge is about the thing that said so.
        let wrong = wrong_badge(self.troubles);
        let wrong_width = text_width(&wrong);

        let cursor = buffer.cursor();
        // One-based, because that is what every other tool reports. The column
        // counts characters rather than cells: it is the cursor's position in
        // the text, which is also the coordinate the LSP layer will speak in.
        //
        // Over a rendering there is no cursor, so what goes here is how far
        // down it the reader has scrolled: a position in what is on screen,
        // which is the question the same corner answers either way.
        let right = match (self.rows, buffer.in_block()) {
            (Some(rows), _) => format!("{}/{rows}", buffer.viewport().top.get() + 1),
            // In a hunk's removed lines, which are not lines of this file:
            // they have no number here, so what is reported is where the
            // caret is in the block, marked as the file's own position is
            // not. A number without the minus would name a line of the
            // file the caret is nowhere near.
            (None, Some((line, column))) => format!("-{}:{}", line.get() + 1, column.get() + 1),
            (None, None) => format!("{}:{}", cursor.line.get() + 1, cursor.column.get() + 1),
        };
        let right_width = text_width(&right);

        // Which branch the tree is on, in front of the path: the path is
        // shown relative to that tree, so what it is a path *of* comes
        // first. Dropped whole rather than truncated where the row cannot
        // hold both -- the rule the rest of this row follows, and the right
        // way round, because the path is what the reader is looking at and
        // half a branch name is worse than none.
        let branch = branch_badge(self.head);
        let branch_width = text_width(&branch);

        // One column of padding at each end, at least one between the two
        // halves, and room for the marker, which is never the part that gets
        // dropped.
        let reserved = right_width
            .saturating_add(3)
            .saturating_add(marker_width)
            .saturating_add(working_width)
            .saturating_add(badge_width)
            .saturating_add(remote_width)
            .saturating_add(wrong_width);
        // The file's own glyph, the same one the pickers give it, so a row in
        // a list and the file on screen are recognizably the same thing.
        let path = match obelus_icons::enabled() {
            true => format!(
                "{}{}{}",
                obelus_icons::for_path(buffer.path()),
                crate::after_a_glyph(),
                relative_to(buffer.path(), self.working_directory).display()
            ),
            false => relative_to(buffer.path(), self.working_directory)
                .display()
                .to_string(),
        };
        let available = usize::from(area.width).saturating_sub(reserved);
        // What the branch leaves, where there is anything left worth
        // leaving. A path cut to nothing is a row with a branch on it and
        // no file, which is the wrong half to keep.
        let (branch, available) = match available.saturating_sub(branch_width) {
            left if left >= PATH_AT_LEAST => (branch.as_str(), left),
            _ => ("", available),
        };
        let branch_width = text_width(branch);
        let path = truncate_from_left(&path, available);

        let right_start = usize::from(area.width)
            .saturating_sub(right_width)
            .saturating_sub(1);

        if !branch.is_empty() {
            write(
                cells,
                area.x + 1,
                area.y,
                branch,
                // The dim ink the row's other asides are written in. The
                // path's own would make the two read as one sentence --
                // `master  src/app/mod.rs` as a path with a first
                // component -- and the branch is a thing a reader looks at
                // deliberately rather than one that should catch the eye.
                style.fg(self.theme.gutter),
            );
        }
        if let Ok(offset) = u16::try_from(1usize.saturating_add(branch_width)) {
            write(cells, area.x + offset, area.y, &path, style);
        }

        // Only if it fits before the cursor position. On a screen too narrow
        // for both, the position wins: it is there every frame, and half a
        // word of warning is worse than none.
        let after_path = 1usize
            .saturating_add(branch_width)
            .saturating_add(text_width(&path));
        if let Ok(offset) = u16::try_from(after_path)
            && !marker.is_empty()
            && after_path + marker_width <= right_start
        {
            write(
                cells,
                area.x + offset,
                area.y,
                marker,
                style.fg(self.theme.status_stale),
            );
        }

        // Left of the position, which is where the eye already goes for the
        // state of things. The badge sits nearest it, with whatever passing
        // word there is to its left, so the part that is always there does
        // not move when the part that comes and goes appears.
        let badge_start = right_start.saturating_sub(badge_width);
        if !badge.is_empty()
            && let Ok(offset) = u16::try_from(badge_start)
            && badge_start > after_path + marker_width
        {
            let colour = badge_colour(self.server, self.theme);
            write(cells, area.x + offset, area.y, &badge, style.fg(colour));
            crate::turning_at(area.x + offset, area.y, &badge);
        }

        // The chat beside the server, and for the same reason: both say
        // whether something outside this window is listening.
        let remote_start = badge_start.saturating_sub(remote_width);
        if let Some((remote, ink)) = remote_mark(self.remote, self.phase, self.theme)
            && let Ok(offset) = u16::try_from(remote_start)
            && remote_start > after_path + marker_width
        {
            write(cells, area.x + offset, area.y, &remote, style.fg(ink));
            crate::turning_at(area.x + offset, area.y, &remote);
        }

        let wrong_start = remote_start.saturating_sub(wrong_width);
        if !wrong.is_empty()
            && let Ok(offset) = u16::try_from(wrong_start)
            && wrong_start > after_path + marker_width
        {
            let colour = self.theme.colour_for(Some(worst(self.troubles).kind()));
            write(cells, area.x + offset, area.y, &wrong, style.fg(colour));
        }

        let working_start = wrong_start.saturating_sub(working_width);
        if !working.is_empty()
            && let Ok(offset) = u16::try_from(working_start)
            && working_start > after_path + marker_width
        {
            write(
                cells,
                area.x + offset,
                area.y,
                &working,
                style.fg(self.wrong_ink().unwrap_or(self.theme.gutter)),
            );
        }

        if let Ok(offset) = u16::try_from(right_start) {
            write(cells, area.x + offset, area.y, &right, style);
        }
    }

    /// The prompt: what has been typed, and nothing else.
    ///
    /// There was a tally of the matches on the right of it. What it
    /// answered -- how much did that narrow it -- is answered better by the
    /// rows above: a reader can see whether the list is long, and a number
    /// on the row they are typing into is a number in the corner of their
    /// eye.
    /// What has been typed to narrow the settings.
    /// The settings page's filter, on whatever row it is given.
    ///
    /// The row is the caller's because the page owns its own last row now:
    /// a dialog does not borrow Obelus's status row, so the one thing this
    /// must not assume is where it is being drawn.
    pub(crate) fn render_filter(
        &self,
        settings: &obelus_component::settings::Settings,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let said = settings.query();
        let marked = held_after(settings.query_held(), &typed(None, ""), self.theme);
        write_marked(
            cells,
            area,
            area.x + 1,
            area.y,
            &typed(None, &said),
            style,
            &marked,
        );
        // Which tab's rows it narrows, because the page has three and they
        // are three different lists. The foot says a key filters; this
        // says what it filters, on the row the reader would type into.
        hint(
            cells,
            area,
            None,
            settings.what_is_filtered(),
            style,
            self.theme,
        );
    }

    /// The notes, where a file would have its path.
    ///
    /// The mark first, which is what tells this from a file at a glance: a
    /// lowercase word where a path usually goes reads as a file with a
    /// short name. It is the command's own mark, so the row the reader
    /// opened this from and the row they are in now wear the same one.
    ///
    /// And one number, where a file puts the cursor's place: how many are
    /// still to come back to. It is the only thing about this document that
    /// changes, and it is the answer to whether it is worth switching to.
    /// Which project the welcome screen's keys are about.
    ///
    /// Said here and not on the page above it, because this row is where
    /// Obelus says what the thing being read *is* -- a file's path and
    /// its mode go here, and which project is the same kind of fact one
    /// step out. The page is for the way in.
    ///
    /// Named, rather than left as a bare path: a path alone at the foot
    /// of a screen is a thing the reader has to work out, and what makes
    /// it worth saying at all is that every key above it is about this
    /// one project and no other -- `f1` searches it, `f3` asks git about
    /// it, a note and a conversation are filed under it.
    ///
    /// Last of the four, so a sentence Obelus has just said still takes
    /// the row: that is news and this is standing information, and the
    /// news goes when the next key is pressed.
    /// The chat's mark at the end of a row with nothing else there, and the
    /// column what is left of the row stops short of.
    ///
    /// A file's row places it among the other things it says about the
    /// window; these rows have only the one, so it takes the end. Dropped
    /// whole from a row with less than half to spare, which leaves what
    /// the row is about the room it needs.
    fn remote_at_end(&self, area: Rect, cells: &mut CellBuffer, style: Style) -> usize {
        let width = usize::from(area.width);
        let Some((mark, ink)) = remote_mark(self.remote, self.phase, self.theme) else {
            return width.saturating_sub(1);
        };
        let start = width.saturating_sub(text_width(&mark) + 1);
        match u16::try_from(start) {
            Ok(offset) if start >= width / 2 => {
                write(cells, area.x + offset, area.y, &mark, style.fg(ink));
                crate::turning_at(area.x + offset, area.y, &mark);
                start.saturating_sub(usize::from(LABEL_GAP))
            }
            _ => width.saturating_sub(1),
        }
    }

    fn render_project(&self, area: Rect, cells: &mut CellBuffer, style: Style) {
        let said = crate::with_home_as_tilde(self.working_directory);
        let label = "Project";
        write(
            cells,
            area.x + 1,
            area.y,
            label,
            style.fg(self.theme.gutter),
        );
        let after = u16::try_from(text_width(label)).unwrap_or(0) + LABEL_GAP;
        let end = self.remote_at_end(area, cells, style);
        let room = end.saturating_sub(usize::from(after) + 1);
        write(
            cells,
            area.x + 1 + after,
            area.y,
            // From the left, the way a path is cut everywhere it is one:
            // what says which project it is is at the end.
            &crate::truncate_from_left(&said, room),
            style,
        );
    }

    /// The box at the foot of the screen that asks which project, which is
    /// one of two.
    ///
    /// One piece draws both, and the word in front is what says which:
    /// `Filter` narrows the projects above it, `Open` is a path being
    /// named. Two words rather than two rows, because they are the same
    /// row at two moments -- and never two meanings at once, which is why
    /// nothing of what was typed into one is carried into the other.
    fn render_choosing(
        &self,
        choosing: &crate::Choosing,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let question = choosing_question(choosing.naming);
        let line = typed(Some(question), &choosing.typed);
        // A path that goes nowhere, said in the ink and then in words.
        //
        // Both only where there is nothing left to suggest: `/tmp/o` is
        // not there either, and marking it while the list below offers
        // `obelus/` would be a complaint about typing. Nothing to
        // suggest *and* nothing at the path is a reader who has gone
        // wrong, and that is the moment to say so.
        let nowhere =
            choosing.naming && !choosing.typed.is_empty() && !choosing.there && !choosing.offering;
        let ink = match nowhere {
            true => style.fg(self.theme.syntax.warning),
            false => style,
        };
        let after = write_marked(
            cells,
            area,
            area.x + 1,
            area.y,
            &line,
            ink,
            &held_after(
                choosing.held.clone(),
                &typed(Some(question), ""),
                self.theme,
            ),
        );
        if nowhere {
            // After the path rather than instead of it: what the reader
            // typed is what they are about to fix, and a row that
            // replaced it with a complaint would take away the thing
            // they need to look at. Dropped whole where it does not fit,
            // the way every sentence on this row is -- a warning cut in
            // half is worse than the ink alone, which is still there.
            let said = " — nothing is here";
            let room = area.right().saturating_sub(after);
            if u16::try_from(text_width(said)).unwrap_or(u16::MAX) <= room {
                write(cells, after, area.y, said, style.fg(self.theme.gutter));
            }
        }
        // What the box says before anything is in it. The filter says
        // what it would narrow; the path box says what shape of answer it
        // wants, because a reader who has never typed one here has no way
        // to know a file is as good as a directory.
        if choosing.typed.is_empty() {
            let hint = match choosing.naming {
                true => "A directory, or a file in one",
                false => "Narrow the list",
            };
            write(
                cells,
                area.x + typed_inset(Some(question)),
                area.y,
                hint,
                style.fg(self.theme.gutter),
            );
        }
    }

    fn render_notes(
        &self,
        notes: &obelus_component::todo::TodoView,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let name = match obelus_icons::enabled() {
            true => format!(
                "{}{}Todo",
                obelus_icons::for_command(obelus_command::Command::TodoOpen),
                crate::after_a_glyph()
            ),
            false => "Todo".to_string(),
        };
        write(cells, area.x + 1, area.y, &name, style);

        let end = self.remote_at_end(area, cells, style);
        let left = notes.todo().notes.iter().filter(|note| !note.done).count();
        let said = (left > 0).then(|| format!("{left} to come back to"));
        let count_at = said
            .as_ref()
            .map_or(end + 1, |said| end.saturating_sub(text_width(said)));
        if let Some(said) = &said
            && count_at > 1 + text_width(&name)
            && let Ok(offset) = u16::try_from(count_at)
        {
            write(cells, area.x + offset, area.y, said, style);
        }

        // Where the note the caret is in is being talked about, when that is
        // somewhere else: the lock beside it says the keys will do nothing,
        // and this says where to go to do something. Where the mode goes on
        // a file's row, because it is the same kind of fact -- what is on
        // screen cannot simply be changed here. Dropped whole where it does
        // not fit between the two, because half a place is no place.
        let Some(holder) = notes.selected_holder() else {
            return;
        };
        let held = match holder {
            obelus_component::todo::Holder::AnotherWindow => {
                "Talked about in another window".to_string()
            }
            obelus_component::todo::Holder::Checkout(name) => format!("Talked about in {name}"),
        };
        let at = 1 + text_width(&name) + 2;
        if at + text_width(&held) + 2 <= count_at
            && let Ok(offset) = u16::try_from(at)
        {
            write(cells, area.x + offset, area.y, &held, style);
        }
    }

    /// A terminal's row: the program, in the words it was started with, and
    /// how it ended once it has.
    ///
    /// Its words rather than its title, because this is the row that says
    /// what is running -- what Obelus was asked to start, not what the
    /// program has since called itself -- and how far back up the screen
    /// the reader is, which no bar says here.
    fn render_terminal(
        &self,
        terminal: &obelus_terminal::Terminal,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let end = self.remote_at_end(area, cells, style);
        let state = match terminal.ended() {
            Some(ended) => Some(crate::terminal::how_it_ended(ended)),
            None => match terminal.scrolled() {
                0 => None,
                1 => Some("1 row back".to_string()),
                rows => Some(format!("{rows} rows back")),
            },
        };
        let state_at = state
            .as_ref()
            .map_or(end + 1, |state| end.saturating_sub(text_width(state)));
        if let Some(state) = &state
            && let Ok(offset) = u16::try_from(state_at)
        {
            let ink = match terminal.ended() {
                // The red a wrong note is, by the same door.
                Some(ended) if !ended.succeeded() => style.fg(self
                    .theme
                    .colour_for(Some(obelus_text::kind::SyntaxKind::Error))),
                _ => style,
            };
            write(cells, area.x + offset, area.y, state, ink);
        }
        // The words, cut where the state begins: a command line is as long
        // as the program it names, and the row's own fact goes first.
        let name = match obelus_icons::enabled() {
            true => format!(
                "{}{}{}",
                obelus_icons::ui::TERMINAL,
                crate::after_a_glyph(),
                terminal.said()
            ),
            false => terminal.said().to_string(),
        };
        let room = state_at.saturating_sub(3);
        let shown = crate::truncate_from_right(&name, room);
        write(cells, area.x + 1, area.y, &shown, style);
    }

    /// The list a reader is building, on whatever row it is given.
    ///
    /// The same shape a picker's prompt has, because it is the same thing:
    /// what has been typed narrows what is above it.
    pub(crate) fn render_names(
        &self,
        names: &obelus_component::names::Names,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let said = names.query().said();
        write_marked(
            cells,
            area,
            area.x + 1,
            area.y,
            &typed(None, &said),
            style,
            &held_after(names.query().held(), &typed(None, ""), self.theme),
        );
        hint(cells, area, None, names.invitation(), style, self.theme);
    }

    /// A list's prompt, on whatever row it is given -- see
    /// [`Self::render_filter`] for why the row is the caller's.
    pub(crate) fn render_prompt(
        &self,
        picker: &Picker,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        prompt_row(picker, area, cells, style, self.theme, self.phase);
    }
}

#[cfg(test)]
mod tests {
    use obelus_theme::builtin::DARK;

    use super::*;

    /// The chat's mark turns while it connects -- and while it connects
    /// again -- and stands still once it has, or once something is wrong;
    /// and a window the chat does not talk to says nothing about it at all.
    ///
    /// Broken deliberately by giving connecting the still ring the other
    /// waits have: the mark said nothing was happening while it connected.
    #[test]
    fn the_chat_mark_turns_while_it_connects() {
        assert_eq!(remote_badge(None, 0), "");
        let connecting = remote_badge(Some(("Feishu", obelus_remote::State::Connecting)), 3);
        assert!(connecting.starts_with(crate::spinning(3)), "{connecting:?}");
        assert_ne!(
            remote_badge(Some(("Feishu", obelus_remote::State::Connecting)), 4),
            connecting,
            "the mark did not move from one frame to the next"
        );
        assert!(
            remote_badge(Some(("Feishu", obelus_remote::State::Connected)), 3)
                .starts_with('\u{25cf}')
        );
        // A field nobody filled in, and an app the platform will not let
        // connect, are as wrong as a refused token. Broken deliberately by
        // leaving `Unready` out of `wrong`: it drew the ring a chat that is
        // switched off draws, after the reader had asked it to connect.
        for state in [
            obelus_remote::State::Refused,
            obelus_remote::State::Unready,
            obelus_remote::State::Declined,
        ] {
            assert!(
                remote_badge(Some(("Feishu", state)), 3).starts_with('\u{2715}'),
                "{state:?}"
            );
        }
    }

    /// The three states have to be told apart at a glance, and the one that
    /// says nothing is running has to say nothing at all: a badge for a
    /// server that was never started would be the opposite of the point.
    #[test]
    fn a_badge_says_which_server_and_which_state() {
        let _held = crate::glyphs_held();
        assert_eq!(server_badge(None, None), "");
        // Whichever way the glyphs are switched, the badge names the server
        // and marks the state, and the two are told apart by the first
        // character.
        for state in [ServerState::Ready, ServerState::Starting, ServerState::Gone] {
            let badge = server_badge(Some(("rust-analyzer", state)), None);
            assert!(badge.contains("rust-analyzer"), "{badge:?}");
            let mark = badge.chars().next().expect("a mark");
            assert_eq!(
                mark,
                if obelus_icons::enabled() {
                    state.glyph()
                } else {
                    state.mark()
                },
                "{badge:?}"
            );
        }

        // And the three marks are three different characters, either way,
        // which is the part a reader actually uses.
        for pair in [
            [
                ServerState::Ready.mark(),
                ServerState::Starting.mark(),
                ServerState::Gone.mark(),
            ],
            [
                ServerState::Ready.glyph(),
                ServerState::Starting.glyph(),
                ServerState::Gone.glyph(),
            ],
        ] {
            assert_eq!(
                pair.iter().collect::<std::collections::HashSet<_>>().len(),
                3
            );
        }
    }

    /// A server that is running looks ordinary; the two ways it is not are
    /// each their own colour. Sharing one would leave "starting" and "dead"
    /// indistinguishable, which is the distinction the badge exists for.
    #[test]
    fn the_states_that_are_not_ready_do_not_look_ready() {
        let ready = badge_colour(Some(("rust-analyzer", ServerState::Ready)), &DARK);
        let starting = badge_colour(Some(("rust-analyzer", ServerState::Starting)), &DARK);
        let gone = badge_colour(Some(("rust-analyzer", ServerState::Gone)), &DARK);

        assert_eq!(ready, DARK.status_foreground);
        assert_ne!(starting, ready);
        assert_ne!(gone, ready);
        assert_ne!(gone, starting);
    }
}
