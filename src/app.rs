//! Application state, and the loop that drives it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
};

use crate::{
    buffer::{Buffer, BufferId, Motion, TextArea},
    command::dispatch,
    coordinates::ByteOffset,
    event::{self, Event},
    keymap::{Context, KeyChord, Keymap},
    picker::{Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue, files, icons},
    syntax::highlight::Highlights,
    theme::{Theme, builtin},
    ui,
    watch::Watcher,
};

/// How many rows a compact picker may take.
///
/// It sits on the status bar and grows upwards only as far as it has to, so
/// the code stays visible. Both pickers over a fixed, short list work that
/// way: they are used while looking at something, and covering the something
/// would be the wrong trade.
const COMPACT_ROWS: u16 = 10;

/// How many already-ready events to fold into one frame.
///
/// A held-down arrow key or a scroll burst delivers faster than a terminal can
/// usefully be redrawn; draining what is ready before drawing turns a burst
/// into one frame. The cap keeps a pathological producer from starving the
/// draw entirely.
const EVENT_DRAIN_LIMIT: usize = 256;

/// Everything obelus is currently showing or remembering.
#[derive(Debug)]
pub struct App {
    keymap: Keymap,
    buffers: Vec<Buffer>,
    current: Option<BufferId>,
    theme: &'static Theme,
    /// The open picker, if one is open.
    ///
    /// One field for all four, because they differ only in what they list.
    picker: Option<Picker>,
    /// Which file walk the picker is currently expecting batches from.
    ///
    /// Bumped every time a file picker opens, so batches from a walk whose
    /// picker has already closed are recognizable and dropped.
    walk_generation: u64,
    /// Sender for the background walk, once the loop has started.
    events: Option<std::sync::mpsc::Sender<Event>>,
    /// The file watcher, once started.
    ///
    /// Held because dropping it stops the watch. `None` means auto-reload is
    /// unavailable — the watcher failed to start, most likely against an
    /// inotify limit — and `file.reload` still works.
    watcher: Option<Watcher>,
    /// The highlight kinds for what is on screen.
    ///
    /// Recomputed every frame from the tree and kept here so the allocation is
    /// reused. Nothing invalidates it, because it is never stale: a reparse
    /// changes the tree and the next frame reads the new one.
    highlights: Highlights,
    /// Where the text was drawn last frame.
    ///
    /// Paging and horizontal scrolling need the size of the text area, which
    /// only the layout knows. Keeping last frame's is right: on the first
    /// frame there is nothing to page through yet, and afterwards it is the
    /// geometry the user is looking at.
    editor_area: Rect,
    working_directory: PathBuf,
    should_quit: bool,
}

impl App {
    /// Starts with the shipped key table and the given documents open.
    #[must_use]
    pub fn new(buffers: Vec<Buffer>) -> Self {
        let current = (!buffers.is_empty()).then(|| BufferId::new(0));
        Self {
            keymap: Keymap::new(),
            buffers,
            current,
            theme: &builtin::DARK,
            picker: None,
            walk_generation: 0,
            events: None,
            watcher: None,
            highlights: Highlights::default(),
            editor_area: Rect::ZERO,
            // Read once. Nothing later asks the operating system again, so
            // every path obelus shows is relative to the same root for the
            // whole session even if something else changes the process's
            // directory.
            working_directory: std::env::current_dir().unwrap_or_default(),
            should_quit: false,
        }
    }

    /// Whether the loop should stop.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Asks the loop to stop after this iteration.
    pub const fn request_quit(&mut self) {
        self.should_quit = true;
    }

    /// The bindings currently in force.
    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Replaces the key table.
    ///
    /// The point of the table being data. A configuration file becomes a
    /// caller of this rather than a change to how lookup works.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    /// The colours currently in force.
    #[must_use]
    pub const fn theme(&self) -> &'static Theme {
        self.theme
    }

    /// Switches theme.
    ///
    /// Takes effect on the next frame and costs nothing else: what is cached
    /// per byte is which kind of thing it is, not what colour, so there is no
    /// reparse and nothing to invalidate.
    pub const fn set_theme(&mut self, theme: &'static Theme) {
        self.theme = theme;
    }

    /// The highlight kinds for what is on screen.
    #[must_use]
    pub const fn highlights(&self) -> &Highlights {
        &self.highlights
    }

    /// Where obelus was started, and the root every path is shown relative to.
    #[must_use]
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The document being read, if any is open.
    #[must_use]
    pub fn current_buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.current?.get())
    }

    fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.current?.get())
    }

    /// Starts watching every open file for changes on disk.
    ///
    /// Best effort: a watcher that will not start is logged and then done
    /// without. Refusing to run because a file cannot be watched would trade a
    /// missing convenience for a missing program.
    pub fn start_watching(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
        let mut watcher = match Watcher::new(sender) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "auto-reload is off");
                return;
            }
        };
        for buffer in &self.buffers {
            if let Err(error) = watcher.watch(buffer.path()) {
                tracing::warn!(%error, path = %buffer.path().display(), "not watching");
            }
        }
        self.watcher = Some(watcher);
    }

    /// The open picker, for the renderer.
    #[must_use]
    pub const fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// Which context key lookup happens in.
    const fn context(&self) -> Context {
        Context::Normal
    }

    /// Offers every file under the working directory.
    pub fn open_file_picker(&mut self) {
        self.walk_generation += 1;
        self.picker = Some(Picker::new(Vec::new(), PickerLayout::FullArea));
        if let Some(sender) = self.events.clone() {
            files::spawn_walk(&self.working_directory, self.walk_generation, sender);
        }
    }

    /// Offers the files already open.
    pub fn open_buffer_picker(&mut self) {
        let items = self
            .buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| PickerItem {
                icon: Some(icons::for_path(buffer.path())),
                label: relative(buffer.path(), &self.working_directory),
                detail: None,
                trailing: None,
                value: PickerValue::Buffer(BufferId::new(index)),
            })
            .collect();
        self.picker = Some(Picker::new(items, PickerLayout::FullArea));
    }

    /// Offers the built-in themes.
    pub fn open_theme_picker(&mut self) {
        let items = builtin::ALL
            .iter()
            .map(|theme| PickerItem {
                // Themes and commands are not files, and a glyph for each
                // would be decoration rather than information.
                icon: None,
                label: theme.name.to_string(),
                detail: None,
                trailing: None,
                value: PickerValue::Theme(theme),
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    /// Offers every command by name.
    pub fn open_command_palette(&mut self) {
        let items = crate::command::ALL
            .iter()
            .map(|spec| PickerItem {
                icon: None,
                label: spec.name.to_string(),
                detail: Some(spec.title.to_string()),
                // The key it is bound to, if it is bound to one. A command
                // with nothing here is one the palette is the only way to
                // reach, which is worth being able to see.
                trailing: self.keymap.chord_for(spec.command).map(KeyChord::label),
                value: PickerValue::Command(spec.command),
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    fn accept(&mut self, value: PickerValue) {
        self.picker = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            PickerValue::Buffer(id) => {
                if id.get() < self.buffers.len() {
                    self.current = Some(id);
                }
            }
            PickerValue::Theme(theme) => self.set_theme(theme),
        }
    }

    /// Opens a file, or switches to it if it is already open.
    fn open(&mut self, path: &Path) {
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.path() == path) {
            self.current = Some(BufferId::new(index));
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(watcher) = self.watcher.as_mut()
                    && let Err(error) = watcher.watch(buffer.path())
                {
                    tracing::warn!(%error, path = %buffer.path().display(), "not watching");
                }
                self.buffers.push(buffer);
                self.current = Some(BufferId::new(self.buffers.len() - 1));
            }
            // A path from the walk can have gone away, or be a file this user
            // cannot read. Neither is a reason to stop.
            Err(error) => tracing::warn!(%error, "could not open"),
        }
    }

    /// The room the text has, once the gutter has taken its columns.
    pub(crate) fn text_area(&self) -> TextArea {
        let width = match self.current_buffer() {
            Some(buffer) => {
                let gutter = ui::editor::gutter_width(buffer.text().line_count());
                self.editor_area.width.saturating_sub(gutter)
            }
            None => self.editor_area.width,
        };
        TextArea {
            width,
            height: self.editor_area.height,
        }
    }

    /// Records the geometry, scrolls the cursor on screen, and highlights what
    /// that leaves visible.
    ///
    /// In this order: the highlight range depends on where the viewport ended
    /// up, so scrolling has to have happened.
    pub fn prepare(&mut self, editor_area: Rect) {
        self.editor_area = editor_area;
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_into_view(area);
        }

        let Self {
            buffers,
            current,
            highlights,
            ..
        } = self;
        let Some(buffer) = current.and_then(|id| buffers.get(id.get())) else {
            highlights.clear();
            return;
        };
        let Some(state) = buffer.syntax() else {
            highlights.clear();
            return;
        };
        let range = visible_bytes(buffer, area.height);
        highlights.refresh(state, buffer.text(), range);
    }

    /// Re-reads whichever open buffers came from `path`.
    ///
    /// The watch is on a directory, so most of what arrives here is about
    /// files obelus does not have open.
    fn reload_path(&mut self, path: &Path) {
        for index in 0..self.buffers.len() {
            if self.buffers[index].path() == path {
                reload(&mut self.buffers[index]);
            }
        }
    }

    /// Re-reads the current file and reparses what changed.
    pub fn reload_current(&mut self) {
        if let Some(buffer) = self.current_buffer_mut() {
            reload(buffer);
        }
    }

    /// Lays the screen out, scrolls the cursor into view, draws, and says
    /// where the terminal should put its cursor.
    ///
    /// The one path to a rendered frame, shared by the loop and the golden
    /// tests. Laying out here means it happens inside `Terminal::draw`, which
    /// is allowed: it is arithmetic over sizes, not work.
    pub fn draw_into(&mut self, cells: &mut CellBuffer, area: Rect) -> Option<Position> {
        self.prepare(ui::regions(area).editor);
        ui::draw(cells, area, self);
        ui::cursor_position(area, self)
    }

    /// Reacts to one event.
    ///
    /// The order keys are offered in is fixed here rather than encoded in the
    /// key table, because it is about which component owns the state a key
    /// moves, not about which key it is. Navigation belongs to whatever holds
    /// the position it moves; commands are the named actions left over.
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            // Redrawing is unconditional after every event, so a resize needs
            // no handling of its own beyond waking the loop.
            Event::Resize => {}
            Event::FileChanged { path } => self.reload_path(&path),
            Event::FilesFound { generation, paths } => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if generation != self.walk_generation {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    picker.extend(paths.into_iter().map(|path| PickerItem {
                        icon: Some(icons::for_path(&path)),
                        label: path.display().to_string(),
                        detail: None,
                        trailing: None,
                        value: PickerValue::File(path),
                    }));
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Discards releases once, here, so nothing further down has to
        // remember to.
        if KeyChord::from_event(&key).is_none() {
            return;
        }
        let editor_height = self.editor_area.height;

        // The picker gets first refusal, because the keys it wants are the
        // ones that move the thing it owns. What it does not want falls
        // through, which is how `ctrl+q` still works with one open.
        if let Some(picker) = self.picker.as_mut() {
            // A page is the rows actually on screen, which is why the layout
            // and the key handler share one function for it.
            let page = picker.visible_rows(editor_height);
            match picker.handle_key(&key, page) {
                PickerOutcome::Consumed => return,
                PickerOutcome::Cancelled => {
                    self.picker = None;
                    return;
                }
                PickerOutcome::Accepted(value) => {
                    self.accept(value);
                    return;
                }
                PickerOutcome::Ignored => {}
            }
        }

        if self.picker.is_none()
            && let Some(motion) = motion_for(&key)
        {
            let area = self.text_area();
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.move_cursor(motion, area);
            }
            return;
        }

        if let Some(command) = self.keymap.lookup(&key, self.context()) {
            dispatch::dispatch(self, command);
        }
    }
}

/// A path as it should be read: relative to the root when it lies under it.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Re-reads one buffer, reporting rather than propagating a failure.
///
/// A file that has been deleted or replaced by a directory leaves the buffer
/// showing what it last held. Losing the contents would be worse than showing
/// something a moment out of date.
fn reload(buffer: &mut Buffer) {
    match buffer.reload() {
        Ok(true) => tracing::debug!(path = %buffer.path().display(), "reloaded"),
        Ok(false) => tracing::trace!(path = %buffer.path().display(), "no change"),
        Err(error) => tracing::warn!(%error, path = %buffer.path().display(), "reload failed"),
    }
}

/// The byte range the viewport covers.
///
/// Whole lines, so a highlight that starts just off the top edge still reaches
/// the first visible row.
fn visible_bytes(buffer: &Buffer, height: u16) -> std::ops::Range<ByteOffset> {
    let text = buffer.text();
    let top = buffer.viewport().top;
    let bottom = top.saturating_add(usize::from(height));
    let start = text.line_start_byte(top);
    let end = if bottom.get() >= text.line_count() {
        text.byte_length()
    } else {
        text.line_start_byte(bottom)
    };
    start..end
}

/// The motion a navigation key stands for.
///
/// A modifier obelus has no meaning for disqualifies the key: `ctrl+left` is a
/// word motion it does not have yet, and treating it as a plain left would be
/// a wrong answer rather than a missing one.
fn motion_for(key: &KeyEvent) -> Option<Motion> {
    // Masked the same way the key table masks: SUPER and HYPER arrive only
    // from terminals speaking the kitty protocol, and no motion should depend
    // on which terminal you are in.
    let modifiers =
        key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);

    match (modifiers, key.code) {
        // Not `ctrl+PageUp`/`ctrl+PageDown`: those mean previous and next tab
        // almost everywhere, and the nearest thing obelus has to a tab is a
        // buffer, so they are worth leaving free.
        (KeyModifiers::CONTROL, KeyCode::Home) => Some(Motion::DocumentStart),
        (KeyModifiers::CONTROL, KeyCode::End) => Some(Motion::DocumentEnd),
        (KeyModifiers::NONE, code) => match code {
            KeyCode::Left => Some(Motion::Left),
            KeyCode::Right => Some(Motion::Right),
            KeyCode::Up => Some(Motion::Up),
            KeyCode::Down => Some(Motion::Down),
            KeyCode::PageUp => Some(Motion::PageUp),
            KeyCode::PageDown => Some(Motion::PageDown),
            KeyCode::Home => Some(Motion::LineStart),
            KeyCode::End => Some(Motion::LineEnd),
            _ => None,
        },
        _ => None,
    }
}

/// Lays out, scrolls and draws one frame.
///
/// Shared with the golden tests so the geometry they assert on is the geometry
/// the loop produces, rather than a second copy of the same three lines.
pub fn render<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    terminal.draw(|frame| {
        let area = frame.area();
        // `Terminal::draw` shows the cursor and moves it when the frame names
        // a position, and hides it when the frame does not, so saying where it
        // goes is the whole of it.
        if let Some(position) = app.draw_into(frame.buffer_mut(), area) {
            frame.set_cursor_position(position);
        }
    })?;
    Ok(())
}

/// Runs until the application asks to quit or input ends.
///
/// `terminal.draw` is synchronous and blocks the loop on a write to stdout.
/// Into a local tty that is tens of microseconds; over ssh or inside tmux,
/// stdout is a pipe and a slow reader really does stall the write. Accepted
/// for now — but nothing slow may go inside the draw closure, which is the
/// mistake that actually happens.
pub fn run<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    // 0.30 made the backend's error an associated type; `anyhow` needs it to
    // be a real, sendable error before `?` will take it.
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let (sender, events) = event::channel();
    event::spawn_terminal_reader(sender.clone());
    app.start_watching(sender);

    while !app.should_quit() {
        render(terminal, app)?;

        let Ok(event) = events.recv() else {
            // Every sender is gone, so no further event can arrive.
            break;
        };
        app.handle(event);

        // Fold in whatever else is already queued.
        for _ in 0..EVENT_DRAIN_LIMIT {
            let Ok(ready) = events.try_recv() else {
                break;
            };
            app.handle(ready);
        }
    }

    Ok(())
}
