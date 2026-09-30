//! The status bar: the file on the left, the cursor on the right.

use std::path::Path;

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
    /// Which of them is nearest the reader, and so whose row this is.
    ///
    /// The same question the caret asks. Three of these can be on screen
    /// at once and only one of them is taking the keys: a question asked
    /// on the status row opens *over* a list rather than closing it, so
    /// the list is not always the nearest thing any more.
    nearest: Option<Layer>,
    /// Whether what is typed goes over what is under the cursor.
    ///
    /// A mode, so it is said where the mode goes: a reader who cannot see
    /// which one they are in finds out by typing, and by then a character
    /// is gone. The caret says it too where Obelus draws its own -- a
    /// block rather than a bar -- and a terminal's caret is the terminal's,
    /// which is why the word is the half that works in both.
    replacing: bool,
    theme: &'a Theme,
    troubles: &'a [obelus_lsp::trouble::Trouble],
    working_directory: &'a Path,
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
            troubles: app.troubles(),
            prompt: app.prompt(),
            making_in: app.making_in(),
            note_is_wrong: app.note_is_wrong(),
            notes: app.notes(),
            nearest: app.layers().nearest(),
            replacing: app.replacing(),
            theme: app.theme(),
            working_directory: app.working_directory(),
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
                    write(cells, area.x + 1, area.y, &line, style);
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
                    write(
                        cells,
                        area.x + 1,
                        area.y,
                        &truncate_from_right(note, usize::from(area.width).saturating_sub(2)),
                        self.wrong_ink().map_or(style, |ink| style.fg(ink)),
                    );
                }
            }
        }
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
                return format!("{}  {name} ", crate::spinning(phase));
            }
            match obelus_icons::enabled() {
                // Two blanks: one that the glyph bleeds into, one to read
                // by.
                true => format!("{}  {name} ", state.glyph()),
                false => format!("{} {name} ", state.mark()),
            }
        })
        .unwrap_or_default()
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
        format!("{asked}{}  {words}", obelus_icons::ui::PROMPT)
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
    let said = picker.query();
    let line = typed(picker.question(), &said);
    // What is held, marked where it is: the prefix in front of the
    // query is not part of what was typed, so the run moves right by
    // however wide that is.
    let marked = match picker.query_held() {
        Some(held) => {
            let ahead = typed(picker.question(), "").chars().count();
            Marked::run(
                held.start + ahead..held.end + ahead,
                theme.selection_background,
            )
        }
        None => Marked::plain(),
    };
    write_marked(cells, area, area.x + 1, area.y, &line, style, &marked);
    hint(
        cells,
        area,
        picker.question(),
        picker.invitation(),
        style,
        theme,
    );
    // And that the list has not answered for what is in it yet.
    if picker.is_filling().is_some() {
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

/// The same, in front of a question's answer.
#[must_use]
pub fn answer_inset(prompt: &obelus_component::prompt::Prompt) -> u16 {
    let inset = 1usize.saturating_add(text_width(prompt.kind().label()));
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
    let caret = 1usize.saturating_add(text_width(prompt.kind().label()) + text_width(&before));
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
        // Unsaved work, and the file having moved under it. Both are the
        // same kind of fact as the three above -- what is on screen is not
        // simply the file at this path -- and the second is the one a
        // reader must know before they press save.
        if buffer.is_dirty() {
            marker.push_str(&match obelus_icons::enabled() {
                true => format!(" {}  unsaved", obelus_icons::ui::UNSAVED),
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
                true => format!(" {}  {away}", obelus_icons::ui::STALE),
                false => format!(" [{away}]"),
            });
        }
        if buffer.is_stale() {
            marker.push_str(&match obelus_icons::enabled() {
                true => format!(" {}  stale", obelus_icons::ui::STALE),
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

        // One column of padding at each end, at least one between the two
        // halves, and room for the marker, which is never the part that gets
        // dropped.
        let reserved = right_width
            .saturating_add(3)
            .saturating_add(marker_width)
            .saturating_add(working_width)
            .saturating_add(badge_width)
            .saturating_add(wrong_width);
        // The file's own glyph, the same one the pickers give it, so a row in
        // a list and the file on screen are recognizably the same thing.
        let path = match obelus_icons::enabled() {
            true => format!(
                "{}  {}",
                obelus_icons::for_path(buffer.path()),
                relative_to(buffer.path(), self.working_directory).display()
            ),
            false => relative_to(buffer.path(), self.working_directory)
                .display()
                .to_string(),
        };
        let available = usize::from(area.width).saturating_sub(reserved);
        let path = truncate_from_left(&path, available);

        let right_start = usize::from(area.width)
            .saturating_sub(right_width)
            .saturating_sub(1);

        write(cells, area.x + 1, area.y, &path, style);

        // Only if it fits before the cursor position. On a screen too narrow
        // for both, the position wins: it is there every frame, and half a
        // word of warning is worse than none.
        let after_path = 1usize.saturating_add(text_width(&path));
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
        }

        let wrong_start = badge_start.saturating_sub(wrong_width);
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
        let marked = match settings.query_held() {
            Some(held) => {
                let ahead = typed(None, "").chars().count();
                Marked::run(
                    held.start + ahead..held.end + ahead,
                    self.theme.selection_background,
                )
            }
            None => Marked::plain(),
        };
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
    fn render_notes(
        &self,
        notes: &obelus_component::todo::TodoView,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let name = match obelus_icons::enabled() {
            true => format!(
                "{}  Todo",
                obelus_icons::for_command(obelus_command::Command::TodoOpen)
            ),
            false => "Todo".to_string(),
        };
        write(cells, area.x + 1, area.y, &name, style);

        let left = notes.todo().notes.iter().filter(|note| !note.done).count();
        if left == 0 {
            return;
        }
        let said = format!("{left} to come back to");
        if let Ok(offset) =
            u16::try_from(usize::from(area.width).saturating_sub(text_width(&said) + 1))
            && area.x + offset > area.x + 1 + u16::try_from(text_width(&name)).unwrap_or(0)
        {
            write(cells, area.x + offset, area.y, &said, style);
        }
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
        write(cells, area.x + 1, area.y, &typed(None, &said), style);
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
