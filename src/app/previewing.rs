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
    /// The runs of characters the preview is about, once converted.
    ///
    /// A list rather than one: a language server names one run, and a
    /// search names whatever characters the query matched, which is as
    /// many runs as the match is scattered over.
    pub marked: &'a [Span],
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
            marked: &preview.marked,
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
        let theme = match &selected {
            Some(PickerValue::Theme(theme)) => Some(*theme),
            // The same list reached from the settings page, where a theme
            // is one setting's value rather than a thing of its own. It is
            // the same choice and it needs the same answer: a droplist of
            // colours a reader can walk without seeing any of them is one
            // they have to choose from blind.
            Some(PickerValue::Setting { key: "theme", word }) => builtin::by_name(word),
            _ => None,
        };
        if let Some(theme) = theme {
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
                    marked: Vec::new(),
                    target: marked.at(),
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
        if preview.target != marked.at() {
            preview.target = marked.at();
            preview.scrolled = 0;
        }

        // The line in question in the middle, the same as arriving at it by
        // jumping, and then wherever the reader has scrolled to. A preview is
        // read for the context around a line, so putting the line at the top
        // spends half the room on the half that was not asked for.
        let target = LineNumber::new(marked.line() as usize);
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
        // The counts, which are a list as well: the notch steps the row
        // rather than the view, for the same reason it does in a picker.
        if let Some(counts) = self.counts.as_mut() {
            counts.scroll(rows, crate::ui::counts::list_height(self.screen_area));
            return;
        }
        // The conversation's transcript, which is the only thing under a
        // list here that scrolls without a cursor in it.
        if self.showing_chat {
            self.chat.scroll(rows);
            return;
        }
        let height = self.editor_area.height;
        if let Some(rows_in_view) = self.rendered_rows()
            && let Some(buffer) = self.current_buffer_mut()
        {
            buffer.scroll_rendering(rows, rows_in_view, height);
            return;
        }
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_by(rows, area);
        }
    }

    /// Pages the preview, if that is what the key does and there is one to
    /// page.
    ///
    /// Answered here rather than where keys are sorted, because what decides
    /// it is whether a preview is on screen -- and that is this module's own
    /// question. A list with nothing under it says no, and the key goes on
    /// to the list, where the bare paging keys still page: a key that
    /// stopped working in a terminal too short for a preview would be worse
    /// than either arrangement.
    pub(super) fn page_preview(&mut self, key: &KeyEvent) -> bool {
        let Some(pages) = preview_paging(key) else {
            return false;
        };
        if self.preview.is_none() {
            return false;
        }
        self.scroll_preview(pages);
        true
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
        let picker = self.picker.as_ref()?;
        // Nothing chosen, because there is nothing to choose: a search with
        // nothing typed into it yet, or a query that matches none of the
        // rows. The preview is then the file being read, where it is being
        // read -- the list has pushed the editor down the screen rather
        // than replaced it with a blank, and what was under the reader's
        // eyes a moment ago is still there to look at.
        let Some(item) = picker.selected_item() else {
            return self.reading_now();
        };
        // A row that *is* the line it names is there because the query
        // matched some of its characters, and those are what the preview
        // marks: the list has already said which they are, and marking
        // anything else -- the whole line, as this did -- answers a
        // question nobody asked. A row that is a name rather than a line --
        // a symbol -- keeps the span whoever named it gave.
        let lines = self.rows_are_lines();
        let matched = |line: u32| Marked::Matched {
            line,
            columns: picker.indices_at(picker.selected()).to_vec(),
        };
        match &item.value {
            // A file has no symbol in it to mark, so nothing is
            // highlighted -- and where it is shown is where it is being
            // read, when it is open at all.
            PickerValue::File(path) => {
                let path = self.working_directory.join(path);
                let at = self.read_at(&path);
                Some((path, at))
            }
            // A file already open is being read somewhere, and that is the
            // part of it to show: choosing the row takes the reader back to
            // exactly this, so the list reads as something folded over the
            // file rather than as a way to somewhere new.
            PickerValue::Buffer(id) => self
                .buffers
                .get(id.get())
                .and_then(Option::as_ref)
                .map(|buffer| (buffer.path().to_path_buf(), Marked::on(&buffer.cursor()))),
            PickerValue::Place { path, line, .. } if lines => Some((path.clone(), matched(*line))),
            PickerValue::Place {
                path,
                line,
                character,
                end_line,
                end_character,
            } => Some((
                path.clone(),
                Marked::Span {
                    line: *line,
                    character: *character,
                    end_line: *end_line,
                    end_character: *end_character,
                },
            )),
            PickerValue::Command(_)
            | PickerValue::Theme(_)
            | PickerValue::Setting { .. }
            | PickerValue::AgentValue { .. }
            | PickerValue::Nothing => None,
        }
    }
}

impl App {
    /// Where a file should be shown: where it is being read if it is open,
    /// and at the top if it is not.
    ///
    /// The same answer the list of open files gives, because it is the same
    /// question -- a file's place in it is the thing a reader remembers it
    /// by, and choosing the row takes them back to exactly that. A list
    /// that previewed the top of a file the reader is twenty screens into
    /// would show them somewhere they have not been for an hour.
    fn read_at(&self, path: &Path) -> Marked {
        self.buffers
            .iter()
            .flatten()
            .find(|buffer| buffer.path() == path)
            .map_or_else(Marked::top, |buffer| Marked::on(&buffer.cursor()))
    }

    /// The file being read and the line it is being read at, as a preview's
    /// subject.
    fn reading_now(&self) -> Option<(PathBuf, Marked)> {
        let buffer = self.current_buffer()?;
        Some((buffer.path().to_path_buf(), Marked::on(&buffer.cursor())))
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
    /// The parts of it the selection is about, once converted.
    marked: Vec<Span>,
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
#[derive(Clone, Debug)]
pub(super) enum Marked {
    /// A run a language server named, in whichever units it agreed to.
    Span {
        /// Where it starts.
        line: u32,
        /// And how far along.
        character: u32,
        /// Where it ends.
        end_line: u32,
        /// And how far along that.
        end_character: u32,
    },
    /// The characters a query matched, as columns of the row's own text.
    ///
    /// Which is not the same as columns of the line: a row is the line
    /// trimmed of its indentation, because the indentation is the same on
    /// every row of a block. What the indent was is worked out from the
    /// file, which is the only place it still exists.
    Matched {
        /// Which line of the file the row was.
        line: u32,
        /// Which characters of the row matched.
        columns: Vec<u32>,
    },
    /// A line, with nothing on it to mark.
    ///
    /// Which is what a row that names a whole file is about: the top of one
    /// obelus has never opened, and wherever the reader is in one it has.
    At {
        /// Which line of the file, counted from zero.
        line: u32,
    },
}

impl Marked {
    /// The top of a file, with nothing to mark.
    const fn top() -> Self {
        Self::At { line: 0 }
    }

    /// The line a cursor is on, with nothing to mark.
    fn on(cursor: &crate::buffer::Cursor) -> Self {
        Self::At {
            line: u32::try_from(cursor.line.get()).unwrap_or(u32::MAX),
        }
    }

    /// Which place in the file this is about, as it arrived.
    ///
    /// What tells one row from another: a preview that cannot tell them
    /// apart forgets the reader's scrolling on every redraw, or never.
    fn at(&self) -> (u32, u32) {
        match self {
            Self::Span {
                line, character, ..
            } => (*line, *character),
            Self::Matched { line, columns } => (*line, columns.first().copied().unwrap_or(0)),
            Self::At { line } => (*line, 0),
        }
    }

    /// Which line of the file the preview should be looking at.
    const fn line(&self) -> u32 {
        match self {
            Self::Span { line, .. } | Self::Matched { line, .. } | Self::At { line } => *line,
        }
    }

    /// The runs to mark, in obelus's own coordinates.
    fn resolve(
        &self,
        text: &crate::text::Text,
        encoding: &lsp_types::PositionEncodingKind,
    ) -> Vec<Span> {
        match self {
            Self::At { .. } => Vec::new(),
            Self::Span {
                line,
                character,
                end_line,
                end_character,
            } => {
                let at = |line, character| {
                    position::from_lsp(text, lsp_types::Position { line, character }, encoding)
                };
                let (line, column) = at(*line, *character);
                let (end_line, end_column) = at(*end_line, *end_character);
                // An empty span marks nothing: a file preview has no symbol
                // in it.
                if (line, column) == (end_line, end_column) {
                    return Vec::new();
                }
                vec![Span {
                    line,
                    column,
                    end_line,
                    end_column,
                }]
            }
            Self::Matched { line, columns } => {
                let line = text.clamp_line(LineNumber::new(*line as usize));
                // The row was the line without its indentation, so the
                // columns are that much further along the line itself.
                let indent = text
                    .line(line)
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .count();
                runs(columns)
                    .into_iter()
                    .map(|(first, end)| Span {
                        line,
                        column: CharColumn::new(indent + first),
                        end_line: line,
                        end_column: CharColumn::new(indent + end),
                    })
                    .collect()
            }
        }
    }
}

/// The runs of consecutive columns in a sorted list of them.
///
/// A fuzzy match lands on scattered characters, and a run of them is one
/// mark rather than one per character: the marking is a background, and
/// three adjacent backgrounds are one shape anyway.
fn runs(columns: &[u32]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for column in columns {
        let column = *column as usize;
        match runs.last_mut() {
            Some(last) if last.1 == column => last.1 = column + 1,
            _ => runs.push((column, column + 1)),
        }
    }
    runs
}

/// How many screenfuls a key scrolls the preview by.
///
/// The bare keys, because a screenful at a time is what a *reading* is
/// paged by and the preview is the thing on screen being read: the list
/// above it is ten rows walked one at a time, and its ends are a keypress
/// away. The same keys with control page the list, for a reader in a list
/// long enough to need it.
fn preview_paging(key: &KeyEvent) -> Option<isize> {
    if !keymap::modifiers_of(key)?.is_empty() {
        return None;
    }
    match key.code {
        KeyCode::PageDown => Some(1),
        KeyCode::PageUp => Some(-1),
        _ => None,
    }
}
