//! The status bar: the file on the left, the cursor on the right.

use std::path::Path;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::{
    app::{App, layers::Layer},
    buffer::Buffer,
    component::picker::Picker,
    icons,
    lsp::ServerState,
    text::text_width,
    theme::Theme,
    ui::{Marked, fill, relative_to, truncate_from_left, write, write_marked},
};

/// The status region.
pub struct StatusView<'a> {
    buffer: Option<&'a Buffer>,
    /// Something to tell the reader, or what a language server is busy with.
    ///
    /// One place for both: they are the same kind of thing — a passing word
    /// about state — and a note is the more urgent of the two.
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
    /// When a picker is open the row is its prompt instead.
    picker: Option<&'a Picker>,
    /// And when the settings are open, the row is what narrows them.
    settings: Option<&'a crate::component::settings::Settings>,
    /// And when a question is being asked, the row is the question.
    prompt: Option<&'a crate::component::prompt::Prompt>,
    /// The notes, while they are what is being read.
    notes: Option<&'a crate::component::todo::TodoView>,
    /// Which of them is nearest the reader, and so whose row this is.
    ///
    /// The same question the caret asks. Three of these can be on screen
    /// at once and only one of them is taking the keys: a question asked
    /// on the status row opens *over* a list rather than closing it, so
    /// the list is not always the nearest thing any more.
    nearest: Option<Layer>,
    theme: &'a Theme,
    troubles: &'a [crate::lsp::trouble::Trouble],
    working_directory: &'a Path,
}

impl<'a> StatusView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            buffer: app.current_buffer(),
            middle: app.note().or_else(|| app.server_working_on()),
            rows: app.rendered_rows(),
            server: app.server_state(),
            troubles: app.troubles(),
            picker: app.picker(),
            settings: app.settings(),
            prompt: app.prompt(),
            notes: app.notes(),
            nearest: app.layers().nearest(),
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
            Some(Layer::Prompt) => {
                if let Some(prompt) = self.prompt {
                    write(cells, area.x + 1, area.y, &prompt.line(), style);
                }
            }
            Some(Layer::Picker) => {
                if let Some(picker) = self.picker {
                    self.render_prompt(picker, area, cells, style);
                }
            }
            // The same shape a picker's prompt has, because it is the same
            // thing: what has been typed narrows what is above it. And
            // nothing else on the row -- what narrowing did is on the
            // screen above it, in the rows themselves.
            Some(Layer::Settings) => {
                if let Some(settings) = self.settings {
                    self.render_filter(settings, area, cells, style);
                }
            }
            // Nothing over what is being read, so the row is about that.
            // The counts are still the exception: they take the row as
            // well as the region, which is what `Room::Screen` says.
            _ => {
                if let Some(notes) = self.notes {
                    self.render_notes(notes, area, cells, style);
                } else if let Some(buffer) = self.buffer {
                    self.render_file(buffer, area, cells, style);
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
fn server_badge(server: Option<(&'static str, ServerState)>) -> String {
    server
        .map(|(name, state)| match icons::enabled() {
            // Two blanks: one that the glyph bleeds into, one to read by.
            true => format!("{}  {name} ", state.glyph()),
            false => format!("{} {name} ", state.mark()),
        })
        .unwrap_or_default()
}

/// How many things are wrong with the file, by kind.
///
/// Marks rather than words: the row is on screen the whole time and this
/// is the part of it that is usually empty. A reader who wants the words
/// has the list.
#[must_use]
fn wrong_badge(troubles: &[crate::lsp::trouble::Trouble]) -> String {
    use crate::lsp::trouble::Severity;

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
fn worst(troubles: &[crate::lsp::trouble::Trouble]) -> crate::lsp::trouble::Severity {
    troubles
        .iter()
        .map(|trouble| trouble.severity)
        .min()
        .unwrap_or(crate::lsp::trouble::Severity::Hint)
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
    if icons::enabled() {
        format!("{asked}{}  {words}", icons::ui::PROMPT)
    } else {
        format!("{asked}> {words}")
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
pub fn answer_inset(prompt: &crate::component::prompt::Prompt) -> u16 {
    let inset = 1usize.saturating_add(text_width(prompt.kind().label()));
    u16::try_from(inset).unwrap_or(u16::MAX)
}

/// Which column the caret belongs in while a question is being asked.
///
/// Shared with the renderer, like the picker's, so the text and the caret
/// cannot disagree about where the answer ends.
#[must_use]
pub fn answer_caret(prompt: &crate::component::prompt::Prompt) -> u16 {
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
        // Unsaved work, and the file having moved under it. Both are the
        // same kind of fact as the three above -- what is on screen is not
        // simply the file at this path -- and the second is the one a
        // reader must know before they press save.
        if buffer.is_dirty() {
            marker.push_str(&match icons::enabled() {
                true => format!(" {}  unsaved", icons::ui::UNSAVED),
                false => " [unsaved]".to_string(),
            });
        }
        let away = match buffer.on_disk() {
            crate::buffer::Disk::Unchanged => None,
            crate::buffer::Disk::Written => Some("Moved"),
            // Said in its own word. A reader who is told their file
            // "moved" when it is gone will go looking for it.
            crate::buffer::Disk::Deleted => Some("Deleted"),
        };
        if let Some(away) = away {
            marker.push_str(&match icons::enabled() {
                true => format!(" {}  {away}", icons::ui::STALE),
                false => format!(" [{away}]"),
            });
        }
        if buffer.is_stale() {
            marker.push_str(&match icons::enabled() {
                true => format!(" {}  stale", icons::ui::STALE),
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

        let badge = server_badge(self.server);
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
        let path = match icons::enabled() {
            true => format!(
                "{}  {}",
                icons::for_path(buffer.path()),
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
                style.fg(self.theme.gutter),
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
    fn render_filter(
        &self,
        settings: &crate::component::settings::Settings,
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
        notes: &crate::component::todo::TodoView,
        area: Rect,
        cells: &mut CellBuffer,
        style: Style,
    ) {
        let name = match icons::enabled() {
            true => format!(
                "{}  Todo",
                icons::for_command(crate::command::Command::TodoOpen)
            ),
            false => "Todo".to_string(),
        };
        write(cells, area.x + 1, area.y, &name, style);

        let left = notes
            .as_written()
            .notes
            .iter()
            .filter(|note| !note.done)
            .count();
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

    fn render_prompt(&self, picker: &Picker, area: Rect, cells: &mut CellBuffer, style: Style) {
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
                    self.theme.selection_background,
                )
            }
            None => Marked::plain(),
        };
        write_marked(cells, area, area.x + 1, area.y, &line, style, &marked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin::DARK;

    /// The three states have to be told apart at a glance, and the one that
    /// says nothing is running has to say nothing at all: a badge for a
    /// server that was never started would be the opposite of the point.
    #[test]
    fn a_badge_says_which_server_and_which_state() {
        assert_eq!(server_badge(None), "");
        // Whichever way the glyphs are switched, the badge names the server
        // and marks the state, and the two are told apart by the first
        // character.
        for state in [ServerState::Ready, ServerState::Starting, ServerState::Gone] {
            let badge = server_badge(Some(("rust-analyzer", state)));
            assert!(badge.contains("rust-analyzer"), "{badge:?}");
            let mark = badge.chars().next().expect("a mark");
            assert_eq!(
                mark,
                if crate::icons::enabled() {
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
