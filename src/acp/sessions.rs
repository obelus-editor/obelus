//! Which conversation belongs to which note, across sittings.
//!
//! An agent keeps what was said -- `session/load` replays it -- so obelus
//! does not have to keep a word of it. What obelus has to keep is the one
//! thing the agent cannot: which of its conversations is about which of this
//! tree's notes. Nothing on the agent's side knows that a note exists.
//!
//! Beside the notes rather than in them, and in obelus's own state directory
//! rather than the tree's `.obelus`: a session id is a name one agent on one
//! machine gave to something, and committing it would hand the next person a
//! conversation they cannot open.
//!
//! One file per tree, named after the tree, so that a reader with eight
//! projects open has eight small files rather than one that every obelus is
//! writing at once.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::todo::NoteId;

/// What obelus remembers about one conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// The agent's own name for the conversation, which is what reopens it.
    pub session: String,
    /// And what the agent calls it, which is what the list of open documents
    /// shows.
    ///
    /// Kept rather than asked for again, because `session/load` replays what
    /// was said and is not obliged to send the title with it -- and the
    /// title is the document's name, so a reader coming back to a list of
    /// conversations with no names would be looking at a list of nothing.
    pub title: Option<String>,
}

/// Which conversation is about which note, for one tree.
///
/// Keyed by the note *and* the agent: the same note talked over with two
/// agents is two conversations, and an agent cannot be handed a session id
/// that another agent minted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    kept: BTreeMap<(NoteId, String), Kept>,
}

impl Remembered {
    /// What is remembered about one note, for one agent.
    #[must_use]
    pub fn get(&self, note: &NoteId, agent: &str) -> Option<&Kept> {
        self.kept.get(&(note.clone(), agent.to_string()))
    }

    /// Remembers one, replacing whatever was there.
    pub fn put(&mut self, note: &NoteId, agent: &str, kept: Kept) {
        self.kept.insert((note.clone(), agent.to_string()), kept);
    }

    /// Forgets one.
    pub fn forget(&mut self, note: &NoteId, agent: &str) {
        self.kept.remove(&(note.clone(), agent.to_string()));
    }

    /// Forgets every conversation whose note has gone.
    ///
    /// Collected when the file is written rather than when a note is
    /// deleted, because a note can go without obelus watching -- another
    /// obelus, the reader's own editor -- and a table that only shrank when
    /// obelus was looking would grow for ever.
    pub fn forget_notes_that_are_gone(&mut self, notes: &[NoteId]) {
        self.kept.retain(|(note, _), _| notes.contains(note));
    }

    /// Every session it holds, for asking an agent which it still knows.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.kept.values().map(|kept| kept.session.as_str())
    }

    /// Forgets every conversation the agent no longer has.
    ///
    /// The other half of reconciling: a session obelus holds that the agent
    /// has never heard of would be found out by a `session/load` that fails,
    /// which is a worse way to find out -- the reader has already pressed
    /// the key and is looking at an empty conversation.
    pub fn forget_what_the_agent_lost(&mut self, agent: &str, still_has: &[String]) {
        self.kept
            .retain(|(_, whose), kept| whose != agent || still_has.contains(&kept.session));
    }
}

/// Where one tree's table is kept.
///
/// The tree's own path, made into a file name: a reader with two checkouts
/// of one project has two trees with two sets of notes, and one file for
/// both would be one set of conversations for two sets of notes.
#[must_use]
pub fn path(root: &Path) -> Option<PathBuf> {
    let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    let flattened: String = root
        .to_string_lossy()
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' => character,
            _ => '_',
        })
        .collect();
    Some(
        crate::logging::state_directory()?
            .join("sessions")
            .join(format!("{flattened}.toml")),
    )
}

/// What the file says, or nothing where there is no file.
///
/// A file that will not parse is not a reason to stop, the way a settings
/// file that will not read is not: obelus goes on with no conversations
/// remembered, which costs the reader a replay and nothing else.
#[must_use]
pub fn read(root: &Path) -> Remembered {
    let Some(path) = path(root) else {
        return Remembered::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Remembered::default();
    };
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "not read, so no conversation is remembered");
            return Remembered::default();
        }
    };
    let mut kept = BTreeMap::new();
    let Some(written) = table.get("talked").and_then(toml::Value::as_array) else {
        return Remembered::default();
    };
    for value in written {
        let Some(row) = value.as_table() else {
            continue;
        };
        let text = |key: &str| row.get(key).and_then(toml::Value::as_str);
        let (Some(note), Some(agent), Some(session)) =
            (text("note"), text("agent"), text("session"))
        else {
            continue;
        };
        let Some(note) = NoteId::read(note) else {
            continue;
        };
        kept.insert(
            (note, agent.to_string()),
            Kept {
                session: session.to_string(),
                title: text("title").map(str::to_string),
            },
        );
    }
    Remembered { kept }
}

/// Reads, changes, and writes back.
///
/// Read-modify-write rather than holding a copy, because a second obelus on
/// the same tree is an ordinary thing to have running and the last one to
/// write would otherwise put back the other's conversations as they were
/// before it opened them.
pub fn change(root: &Path, notes: &[NoteId], what: impl FnOnce(&mut Remembered)) {
    let Some(path) = path(root) else { return };
    let mut remembered = read(root);
    what(&mut remembered);
    remembered.forget_notes_that_are_gone(notes);
    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        tracing::warn!(%error, path = %path.display(), "no directory to remember conversations in");
        return;
    }
    // Through a name beside it and a rename, the way the notes and the
    // settings are written: a crash halfway leaves the old file rather than
    // half of the new one.
    let beside = path.with_extension("toml.writing");
    if let Err(error) =
        std::fs::write(&beside, to_toml(&remembered)).and_then(|()| std::fs::rename(&beside, &path))
    {
        tracing::warn!(%error, path = %path.display(), "the conversations were not remembered");
    }
}

/// What the file holds, as text.
#[must_use]
fn to_toml(remembered: &Remembered) -> String {
    let mut out = String::new();
    for ((note, agent), kept) in &remembered.kept {
        out.push_str("[[talked]]\n");
        out.push_str(&format!("note = \"{note}\"\n"));
        out.push_str(&format!("agent = {}\n", quoted(agent)));
        out.push_str(&format!("session = {}\n", quoted(&kept.session)));
        if let Some(title) = &kept.title {
            out.push_str(&format!("title = {}\n", quoted(title)));
        }
        out.push('\n');
    }
    out
}

/// A string as TOML writes one.
fn quoted(text: &str) -> String {
    let escaped: String = text
        .chars()
        .map(|character| match character {
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            other if (other as u32) < 0x20 => format!("\\u{:04x}", other as u32),
            other => other.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(said: &str) -> NoteId {
        NoteId::read(said).expect("a name")
    }

    /// What goes out comes back, including the parts TOML has opinions
    /// about.
    #[test]
    fn a_conversation_survives_the_file() {
        let mut remembered = Remembered::default();
        remembered.put(
            &note("ABCDEFGH"),
            "claude-acp",
            Kept {
                session: "s-1".to_string(),
                title: Some("quotes \" and \\ backslashes".to_string()),
            },
        );
        // The same note with a second agent, which is a second conversation:
        // an agent cannot be handed a session another one minted.
        remembered.put(
            &note("ABCDEFGH"),
            "codex",
            Kept {
                session: "other".to_string(),
                title: None,
            },
        );

        let table = to_toml(&remembered)
            .parse::<toml::Table>()
            .unwrap_or_else(|error| panic!("{error}\n{}", to_toml(&remembered)));
        let written = table.get("talked").and_then(toml::Value::as_array);
        assert_eq!(written.map(Vec::len), Some(2));
        assert_eq!(
            remembered
                .get(&note("ABCDEFGH"), "claude-acp")
                .map(|kept| kept.session.as_str()),
            Some("s-1")
        );
        assert_ne!(
            remembered.get(&note("ABCDEFGH"), "codex"),
            remembered.get(&note("ABCDEFGH"), "claude-acp"),
            "two agents were given one conversation between them"
        );
    }

    /// A conversation whose note has gone is forgotten.
    #[test]
    fn a_conversation_about_a_note_that_is_gone_is_collected() {
        let mut remembered = Remembered::default();
        for name in ["ABCDEFGH", "JKMNPQRS"] {
            remembered.put(
                &note(name),
                "claude-acp",
                Kept {
                    session: name.to_lowercase(),
                    title: None,
                },
            );
        }
        remembered.forget_notes_that_are_gone(&[note("ABCDEFGH")]);
        assert!(remembered.get(&note("ABCDEFGH"), "claude-acp").is_some());
        assert!(
            remembered.get(&note("JKMNPQRS"), "claude-acp").is_none(),
            "a conversation outlived the note it was about"
        );
    }

    /// And one the agent no longer has.
    ///
    /// Reconciled rather than discovered by a `session/load` that fails,
    /// which is a worse way to find out: by then the reader has pressed the
    /// key and is looking at an empty conversation.
    #[test]
    fn a_conversation_the_agent_has_forgotten_is_dropped() {
        let mut remembered = Remembered::default();
        remembered.put(
            &note("ABCDEFGH"),
            "claude-acp",
            Kept {
                session: "s-1".into(),
                title: None,
            },
        );
        remembered.put(
            &note("JKMNPQRS"),
            "claude-acp",
            Kept {
                session: "s-2".into(),
                title: None,
            },
        );
        // A second agent's, which this one's answer says nothing about.
        remembered.put(
            &note("ABCDEFGH"),
            "codex",
            Kept {
                session: "x-9".into(),
                title: None,
            },
        );

        remembered.forget_what_the_agent_lost("claude-acp", &["s-1".to_string()]);

        assert!(remembered.get(&note("ABCDEFGH"), "claude-acp").is_some());
        assert!(
            remembered.get(&note("JKMNPQRS"), "claude-acp").is_none(),
            "a session the agent has never heard of was kept"
        );
        assert!(
            remembered.get(&note("ABCDEFGH"), "codex").is_some(),
            "one agent's answer threw away another agent's conversation"
        );
    }
}
