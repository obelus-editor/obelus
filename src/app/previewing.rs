//! The few lines of somewhere else that a list shows.
//!
//! Its own buffer, its own highlighting and its own scroll: a preview is a
//! second document on screen, and the one being read is not to be disturbed
//! by looking at it.

use super::*;

/// What a preview is, for the view that draws it.
///
/// A borrow of the whole of it rather than a tuple: it is the same list of
/// things the editor draws for the document being read, and a tuple of four
/// grows a fifth without saying what any of them are.
pub struct Previewed<'a> {
    /// The file, read into a buffer of its own.
    pub buffer: &'a Buffer,
    /// Its syntax, refreshed for the rows on screen.
    pub highlights: &'a Highlights,
    /// The run of characters the preview is about, once converted.
    pub marked: Option<Span>,
    /// What git says about the file.
    pub changes: Option<&'a crate::git::Changes>,
}

impl App {
    /// The file the picker's selection names, if it has been read, and the
    /// part of it the selection is about.
    #[must_use]
    pub fn preview(&self) -> Option<Previewed<'_>> {
        let preview = self.preview.as_ref()?;
        Some(Previewed {
            buffer: &preview.buffer,
            highlights: &preview.highlights,
            marked: preview.marked,
            changes: preview.changes.as_ref(),
        })
    }

    /// Wears whatever theme the picker's selection names.
    ///
    /// The preview *is* the application: there is no way to show what a theme
    /// looks like other than by using it, and every view already reads its
    /// colours from one place. Cancelling puts the old one back.
    pub(super) fn preview_theme(&mut self) {
        let selected = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .map(|item| item.value.clone());
        if let Some(PickerValue::Theme(theme)) = selected {
            self.theme = theme;
        }
    }

    /// Reads whatever the picker's selection names, and points it at the line
    /// the selection is about.
    pub(super) fn refresh_preview(&mut self, editor_area: Rect) {
        let Some(area) = ui::picker::preview_region(self.picker.as_ref(), editor_area) else {
            self.preview = None;
            return;
        };
        let Some((path, marked)) = self.preview_target() else {
            self.preview = None;
            return;
        };

        if self.preview.as_ref().map(|preview| preview.path.as_path()) != Some(path.as_path()) {
            self.preview = match Buffer::open(&path) {
                Ok(buffer) => Some(Preview {
                    path: path.clone(),
                    changes: git::head_text(&path).map(|committed| {
                        git::Changes::between(&committed, &buffer.text().rope().to_string())
                    }),
                    buffer,
                    highlights: Highlights::default(),
                    marked: None,
                    target: (marked.line, marked.character),
                    scrolled: 0,
                }),
                // A file that has gone, or one this reader cannot read. No
                // preview rather than a message: the list is the subject here.
                Err(error) => {
                    tracing::debug!(%error, "no preview");
                    None
                }
            };
        }

        // Whichever encoding the server for this language agreed to. Nothing
        // named the place if there is no server, and then the mark is the top
        // of the file, where the units do not matter.
        let encoding = self
            .preview
            .as_ref()
            .and_then(Preview::language)
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });

        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        // A different row is a different subject, so whatever the reader had
        // scrolled to is about the row they have left.
        if preview.target != (marked.line, marked.character) {
            preview.target = (marked.line, marked.character);
            preview.scrolled = 0;
        }

        // The line in question in the middle, the same as arriving at it by
        // jumping, and then wherever the reader has scrolled to. A preview is
        // read for the context around a line, so putting the line at the top
        // spends half the room on the half that was not asked for.
        let target = LineNumber::new(marked.line as usize);
        let text = TextArea {
            width: area
                .width
                .saturating_sub(ui::editor::gutter_width(preview.buffer.text().line_count()))
                .saturating_sub(ui::editor::SCROLLBAR_WIDTH),
            height: area.height,
            // A preview always wraps: it is a few lines of somewhere else,
            // and a line running off its right-hand edge with no way to
            // scroll it would be a line nobody can read.
            wrap: true,
        };
        preview.buffer.place_cursor(target, CharColumn::new(0));
        preview.buffer.center_on_cursor(text);
        // Stored back, so rows the file does not have are not banked against
        // the next press the other way.
        preview.scrolled = preview.buffer.scroll_rows(preview.scrolled, text);
        preview.marked = marked.resolve(preview.buffer.text(), &encoding);

        let range = visible_bytes(&preview.buffer, area.height);
        if let Some(state) = preview.buffer.syntax() {
            preview
                .highlights
                .refresh(state, preview.buffer.text(), range);
        } else {
            preview.highlights.clear();
        }
    }

    /// What the wheel turns.
    ///
    /// Whatever the reader is looking at: the list when one is open -- a list
    /// under a wheel scrolls, and with the mouse reported the wheel no longer
    /// arrives as arrow keys, so a picker that ignored it would have lost
    /// something -- and otherwise the file, by rows, with the cursor left
    /// where it was put.
    pub(super) fn scroll(&mut self, rows: isize) {
        if let Some(picker) = self.picker.as_mut() {
            // One row a notch in a list. Three is right for text, where a
            // notch is a gesture at a paragraph; a list is chosen through one
            // row at a time.
            picker.move_selection_by(rows.signum());
            return;
        }
        // The conversation's transcript, which is the only thing under a
        // list here that scrolls without a cursor in it.
        if self.showing_chat {
            self.chat.scroll(rows);
            return;
        }
        if let Some(rows_in_view) = self.markdown().map(<[_]>::len)
            && let Some(buffer) = self.current_buffer_mut()
        {
            buffer.scroll_rendering(rows, rows_in_view);
            return;
        }
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_by(rows, area);
        }
    }

    /// Scrolls the preview, without moving the selection.
    ///
    /// Not a command: it is navigation, and navigation belongs to whatever
    /// holds the position it moves. What holds this one is the preview.
    pub(super) fn scroll_preview(&mut self, pages: isize) {
        let Some(area) = ui::picker::preview_region(self.picker.as_ref(), self.editor_area) else {
            return;
        };
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let rows = isize::try_from(area.height.max(1)).unwrap_or(1);
        // Not clamped here: what the file can actually give is known when it
        // is drawn, and `refresh_preview` stores that back.
        preview.scrolled += pages * rows;
    }

    /// The file, and the part of it, the picker's selection is about.
    fn preview_target(&self) -> Option<(PathBuf, Marked)> {
        let item = self.picker.as_ref()?.selected_item()?;
        match &item.value {
            // A file has no symbol in it to mark, so the preview starts at
            // the top with nothing highlighted.
            PickerValue::File(path) => Some((self.working_directory.join(path), Marked::top())),
            PickerValue::Buffer(id) => self
                .buffers
                .get(id.get())
                .and_then(Option::as_ref)
                .map(|buffer| (buffer.path().to_path_buf(), Marked::top())),
            PickerValue::Place {
                path,
                line,
                character,
                end_line,
                end_character,
            } => Some((
                path.clone(),
                Marked {
                    line: *line,
                    character: *character,
                    end_line: *end_line,
                    end_character: *end_character,
                },
            )),
            PickerValue::Command(_)
            | PickerValue::Theme(_)
            | PickerValue::Setting { .. }
            | PickerValue::Permission(_)
            | PickerValue::Nothing => None,
        }
    }
}

/// A file read so that the picker's selection can be shown.
#[derive(Debug)]
pub(super) struct Preview {
    path: PathBuf,
    buffer: Buffer,
    highlights: Highlights,
    /// What git says about this file, so the preview carries the same
    /// margin the editor does.
    ///
    /// Worked out once, when the file is read: a preview is a snapshot of
    /// somewhere else, and the diff of a file nobody is editing does not
    /// change while it is being looked at.
    changes: Option<crate::git::Changes>,
    /// The part of it the selection is about, once converted.
    marked: Option<Span>,
    /// Which part of the file the selection is about, as it arrived.
    ///
    /// Kept so that moving to a different row can be told from redrawing the
    /// same one, which is when the scrolling below is forgotten.
    target: (u32, u32),
    /// How many rows the reader has scrolled the preview by.
    ///
    /// Held here rather than in the buffer's viewport because the viewport is
    /// set from the target on every frame: without this, a scroll would be
    /// undone before it was drawn.
    scrolled: isize,
}

impl Preview {
    fn language(&self) -> Option<LanguageId> {
        self.buffer.language()
    }
}

/// The part of a file a selection is about, in the protocol's units.
#[derive(Clone, Copy, Debug)]
pub(super) struct Marked {
    line: u32,
    character: u32,
    end_line: u32,
    end_character: u32,
}

impl Marked {
    /// The top of a file, with nothing to mark.
    const fn top() -> Self {
        Self {
            line: 0,
            character: 0,
            end_line: 0,
            end_character: 0,
        }
    }

    /// The same span in obelus's own coordinates, or nothing when it is empty.
    fn resolve(
        self,
        text: &crate::text::Text,
        encoding: &lsp_types::PositionEncodingKind,
    ) -> Option<Span> {
        let at = |line, character| {
            position::from_lsp(text, lsp_types::Position { line, character }, encoding)
        };
        let (line, column) = at(self.line, self.character);
        let (end_line, end_column) = at(self.end_line, self.end_character);
        // An empty span marks nothing: a file preview has no symbol in it.
        if (line, column) == (end_line, end_column) {
            return None;
        }
        Some(Span {
            line,
            column,
            end_line,
            end_column,
        })
    }
}

/// How many screenfuls a key scrolls the preview by.
///
/// The plain keys page the list, so these are the same keys with control
/// held. Reading a candidate and choosing between candidates are different
/// jobs, and a list of references is read by doing both at once.
pub(super) fn preview_paging(key: &KeyEvent) -> Option<isize> {
    if keymap::modifiers_of(key)? != KeyModifiers::CONTROL {
        return None;
    }
    match key.code {
        KeyCode::PageDown => Some(1),
        KeyCode::PageUp => Some(-1),
        _ => None,
    }
}
