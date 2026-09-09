//! The status bar: the file on the left, the cursor on the right.

use std::path::Path;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::{
    app::App,
    buffer::Buffer,
    icons,
    lsp::ServerState,
    picker::Picker,
    theme::Theme,
    ui::{fill, text_width, truncate_from_left, write},
};

/// The status region.
pub struct StatusView<'a> {
    buffer: Option<&'a Buffer>,
    /// Something to tell the reader, or what a language server is busy with.
    ///
    /// One place for both: they are the same kind of thing — a passing word
    /// about state — and a note is the more urgent of the two.
    middle: Option<&'a str>,
    /// The server for this file, and what it is doing.
    ///
    /// On screen the whole time, unlike `middle`, which comes and goes. A
    /// reader whose jump did nothing needs to know whether anything was
    /// listening, and that question is asked *after* the answer disappoints:
    /// a marker that had come and gone would not be there to answer it.
    server: Option<(&'static str, ServerState)>,
    /// When a picker is open the row is its prompt instead.
    picker: Option<&'a Picker>,
    theme: &'a Theme,
    working_directory: &'a Path,
}

impl<'a> StatusView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            buffer: app.current_buffer(),
            middle: app.note().or_else(|| app.server_working_on()),
            server: app.server_state(),
            picker: app.picker(),
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
            .bg(self.theme.status_background)
            .fg(self.theme.status_foreground);

        // The whole row, including the padding at both ends and the gap in the
        // middle, so the bar reads as one solid band rather than as coloured
        // text floating on the code's background.
        fill(cells, area, style);

        if let Some(picker) = self.picker {
            self.render_prompt(picker, area, cells, style);
        } else if let Some(buffer) = self.buffer {
            self.render_file(buffer, area, cells, style);
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

/// What the prompt shows.
///
/// A magnifier for every picker, because every one of them filters by typing:
/// the four differ in what they list, not in what typing does. Followed by a
/// blank column, like every other glyph.
fn prompt_text(picker: &Picker) -> String {
    if icons::enabled() {
        format!("{}  {}", icons::ui::PROMPT, picker.query())
    } else {
        format!("> {}", picker.query())
    }
}

/// Which column of the status row the caret belongs in.
///
/// The terminal draws the caret, so this is only where to tell it to put it.
/// Shared with the renderer so the text and the caret cannot disagree.
#[must_use]
pub fn prompt_caret(picker: &Picker) -> u16 {
    let caret = 1usize.saturating_add(text_width(&prompt_text(picker)));
    u16::try_from(caret).unwrap_or(u16::MAX)
}

impl StatusView<'_> {
    /// The file on the left, the cursor position on the right, and a marker
    /// between them when the file can no longer be read.
    fn render_file(&self, buffer: &Buffer, area: Rect, cells: &mut CellBuffer, style: Style) {
        // Sits with the path rather than with the cursor position, because it
        // is a fact about the file. In its own colour: the whole point is that
        // it is noticed without being looked for.
        let marker = match (buffer.is_stale(), icons::enabled()) {
            (false, _) => String::new(),
            (true, true) => format!(" {}  stale", icons::ui::STALE),
            (true, false) => " [stale]".to_string(),
        };
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

        let cursor = buffer.cursor();
        // One-based, because that is what every other tool reports. The column
        // counts characters rather than cells: it is the cursor's position in
        // the text, which is also the coordinate the LSP layer will speak in.
        let right = format!("{}:{}", cursor.line.get() + 1, cursor.column.get() + 1);
        let right_width = text_width(&right);

        // One column of padding at each end, at least one between the two
        // halves, and room for the marker, which is never the part that gets
        // dropped.
        let reserved = right_width
            .saturating_add(3)
            .saturating_add(marker_width)
            .saturating_add(working_width)
            .saturating_add(badge_width);
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

        let working_start = badge_start.saturating_sub(working_width);
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

    /// The prompt: what has been typed.
    fn render_prompt(&self, picker: &Picker, area: Rect, cells: &mut CellBuffer, style: Style) {
        let prompt = prompt_text(picker);
        write(cells, area.x + 1, area.y, &prompt, style);
        let caret = usize::from(prompt_caret(picker));

        let count = format!("{}", picker.match_count());
        let start = usize::from(area.width)
            .saturating_sub(text_width(&count))
            .saturating_sub(1);
        if let Ok(offset) = u16::try_from(start)
            && usize::from(offset) > caret
        {
            write(
                cells,
                area.x + offset,
                area.y,
                &count,
                style.fg(self.theme.gutter),
            );
        }
    }
}

/// The path as it should be read: relative to the working directory when it
/// lies under it, and unchanged when it does not.
///
/// A reader spends its time inside one tree, and the leading directories of
/// that tree are the part already known.
fn relative_to<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
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
