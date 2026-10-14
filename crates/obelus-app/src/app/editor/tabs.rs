//! What is open, as a row of tabs a window draws on its title bar.
//!
//! The same documents as the list of what is open, in the same order --
//! the order they were opened in, which holds still -- and called what that
//! list calls them, shortened: a tab is a few words wide, so a file is its
//! name and not its path, unless another tab has the same name.
//!
//! A terminal has no title bar and is told nothing; the list is how it
//! reaches the same documents. Choosing a tab is choosing that row of the
//! list: the same jump recorded, and a list over the file given up the way
//! a key naming another view gives it up (`app/switching`).

use crate::app::*;

impl App {
    /// The tabs, as they are now.
    pub(in crate::app) fn tabs(&self) -> Vec<Tab> {
        let none = obelus_todo::Todo::default();
        let notes = self.notes.notes_kept.as_ref().unwrap_or(&none);
        let open = || {
            self.documents
                .iter()
                .enumerate()
                // Closed slots are holes, not tabs.
                .filter_map(|(index, document)| Some((DocumentId::new(index), document.as_ref()?)))
        };
        // Which names more than one open file has: half the files in a
        // project are called `mod.rs`, and two tabs saying it are two tabs
        // the reader cannot tell apart.
        let mut seen = HashMap::new();
        for (_, document) in open() {
            if let Some(buffer) = document.file() {
                *seen.entry(file_name(buffer.path())).or_insert(0usize) += 1;
            }
        }
        open()
            .map(|(id, document)| {
                let name = match document {
                    Document::File(buffer) => {
                        let name = file_name(buffer.path());
                        let mut name = match seen.get(&name).copied().unwrap_or(0) > 1 {
                            true => with_its_directory(buffer.path(), &name),
                            false => name,
                        };
                        // Which commit, as the list says it: a file and a
                        // commit's version of it share a name.
                        if let Some(short) = buffer.content().short() {
                            name.push_str(" @ ");
                            name.push_str(&short);
                        }
                        name
                    }
                    Document::Chat(talk) => self
                        .conversation_name(talk, notes)
                        .unwrap_or_else(|| "A conversation".to_string()),
                    Document::Notes(_) => "Todo".to_string(),
                    Document::Terminal(terminal) => {
                        terminal.title().unwrap_or(terminal.said()).to_string()
                    }
                };
                // Somebody else's text, an agent's name for a conversation
                // or a program's for its terminal: one line, and capped, so
                // that one tab cannot push every other off the bar.
                let name = name.replace(['\n', '\r'], " ");
                // The row's mark in the list of what is open, so the tab and
                // the row are visibly one document.
                let icon = obelus_icons::enabled().then(|| match document {
                    Document::File(buffer) => obelus_icons::for_path(buffer.path()),
                    Document::Chat(_) => obelus_icons::ui::AGENT,
                    Document::Notes(_) => {
                        obelus_icons::for_command(obelus_command::Command::TodoOpen)
                    }
                    Document::Terminal(_) => obelus_icons::ui::TERMINAL,
                });
                Tab {
                    document: id,
                    name: obelus_ui::truncate_from_right(&name, TAB_NAME),
                    icon,
                    reading: self.current == Some(id),
                    unwritten: document.file().is_some_and(Buffer::is_dirty),
                }
            })
            .collect()
    }

    /// Tells whatever is drawing Obelus what is open, where that changed.
    ///
    /// Worked out on every frame and said only when it moved: the work is
    /// a walk over what is open, and every name in it is one that is
    /// already kept.
    pub(in crate::app) fn tell_the_tabs(&mut self) {
        let Some(drawing) = self.drawing.clone() else {
            return;
        };
        let tabs = self.tabs();
        if tabs != self.told_tabs {
            drawing.tabs(&tabs);
            self.told_tabs = tabs;
        }
    }

    /// Goes to a document because the reader chose its tab.
    ///
    /// What choosing its row in the list of what is open does: the jump is
    /// recorded, so `alt+left` comes back. And what is over the file is
    /// given up the way a key naming another view gives it up -- unless it
    /// is waiting on the reader or being typed into, which a press
    /// somewhere else does not answer.
    pub(in crate::app) fn choose_by_its_tab(&mut self, id: DocumentId) {
        if self.document(id).is_none() {
            return;
        }
        if self.layers().nearest().is_some() {
            if !self.gives_way_to_a_view() {
                return;
            }
            self.put_away_the_views();
        }
        if self.current != Some(id) {
            let from = self.here();
            self.record(from);
        }
        self.go_to_document(id);
    }
}

/// How many cells a tab's name may take before it is cut short.
const TAB_NAME: usize = 32;

/// The last part of a path.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A file's name with the directory it is in, for the tab of one whose
/// name another open file has as well.
fn with_its_directory(path: &Path, name: &str) -> String {
    match path
        .parent()
        .and_then(Path::file_name)
        .map(|directory| directory.to_string_lossy())
    {
        Some(directory) => format!("{directory}/{name}"),
        None => name.to_string(),
    }
}
