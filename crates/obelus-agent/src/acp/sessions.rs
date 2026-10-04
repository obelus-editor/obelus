//! Which conversation belongs to which note, across sittings.
//!
//! An agent keeps what was said -- `session/load` replays it -- so Obelus
//! does not have to keep a word of it. Both halves come back: the agent's
//! own, and the reader's as `user_message_chunk`, which is the only way a
//! client that was not running when they were typed can have them. What
//! Obelus has to keep is the one thing the agent cannot: which of its
//! conversations is about which of this project's notes. Nothing on the
//! agent's side knows that a note exists.
//!
//! Beside the notes rather than in them, and in Obelus's own state directory
//! rather than the project's `.obelus`: a session id is a name one agent on one
//! machine gave to something, and committing it would hand the next person a
//! conversation they cannot open.
//!
//! One file per project, named after the project, so that a reader with eight
//! projects open has eight small files rather than one that every Obelus is
//! writing at once.
//!
//! **A conversation belongs to the checkout it was had in.** The project is
//! every worktree of one repository, because the notes are; the agent is
//! told one directory, and keeps what was said under it -- Claude Code files
//! a conversation by the directory it was started in, and asked for it from
//! another answers that there is no such thing. So a row says which
//! checkout it was had in, a note has a conversation in each, and the ones
//! from somewhere else are shown and not offered, the way another agent's
//! are. Taking one up from the wrong checkout used to be refused, and the
//! refusal forgot it -- out of the one table every worktree reads.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use obelus_git::todo::NoteId;

use crate::chats::ChatId;

/// What Obelus remembers about one conversation.
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
    /// What the note said when Obelus last told the agent about it.
    ///
    /// Kept so that Obelus can tell whether the agent is out of date. The
    /// note is the reader's file and they rewrite it -- overnight, between
    /// two messages -- and an agent told what it said on Monday has no way
    /// to find out that it says something else on Tuesday: the protocol
    /// gives a client no channel to a conversation but the next prompt, so
    /// what is not said in one is not said at all.
    ///
    /// The note's own words and where it points, which are the two things
    /// about it that Obelus tells an agent. Not the paragraph Obelus wraps
    /// them in: rewording that is not the note being rewritten and must
    /// not read as it.
    ///
    /// `None` for a conversation from before this was written down, which
    /// is read as "it has been told nothing" -- one repeated telling on
    /// the way past, rather than a silence that lasts.
    pub told: Option<String>,
    /// Whether the agent has been told who it is talking to.
    ///
    /// A bit and not a fingerprint, unlike [`Self::told`]: what it carries
    /// is Obelus's own words about itself, which do not change under a
    /// conversation the way the reader's note does, so there is nothing to
    /// compare and nothing to say again.
    ///
    /// Missing from a file written before this was, which reads as `false`
    /// -- one repeated telling on the way past, which is what the field
    /// above settles for in the same case.
    pub introduced: bool,
    /// When something was last said in it, as seconds since the epoch.
    ///
    /// What the list of conversations is ordered by and what each row says
    /// about itself, through the same `how_long_ago` a commit's row is
    /// written with.
    ///
    /// `None` for a conversation from before this was written down, which
    /// sorts last and says nothing -- a made-up time would put a
    /// conversation somewhere in the order on no evidence at all.
    pub last: Option<i64>,
}

/// Which conversation is which, for one project.
///
/// Keyed by the conversation, the agent *and* the checkout: the same note
/// talked over with two agents is two conversations, and an agent cannot be
/// handed a session id that another agent minted -- which is why the list
/// Obelus offers is one agent's, and why the others are shown and not
/// offered. The checkout is the same fact one level down: what an agent
/// can take up is what it was told the directory of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    kept: BTreeMap<(ChatId, String, PathBuf), Kept>,
}

impl Remembered {
    /// What is remembered about one conversation, for one agent, in one
    /// checkout.
    #[must_use]
    pub fn get(&self, which: &ChatId, agent: &str, tree: &Path) -> Option<&Kept> {
        self.kept
            .get(&(which.clone(), agent.to_string(), tree.to_path_buf()))
    }

    /// Remembers one, replacing whatever was there.
    pub fn put(&mut self, which: &ChatId, agent: &str, tree: &Path, kept: Kept) {
        self.kept
            .insert((which.clone(), agent.to_string(), tree.to_path_buf()), kept);
    }

    /// Forgets one.
    pub fn forget(&mut self, which: &ChatId, agent: &str, tree: &Path) {
        self.kept
            .remove(&(which.clone(), agent.to_string(), tree.to_path_buf()));
    }

    /// Every conversation it holds, with which one it is, whose agent, and
    /// which checkout it was had in.
    ///
    /// In no particular order: what orders the list is when each was last
    /// talked in, which is the caller's to sort by because it is the
    /// caller that draws it.
    pub fn all(&self) -> impl Iterator<Item = (&ChatId, &str, &Path, &Kept)> {
        self.kept
            .iter()
            .map(|((which, agent, tree), kept)| (which, agent.as_str(), tree.as_path(), kept))
    }

    /// Forgets every conversation whose note has gone.
    ///
    /// Collected when the file is written rather than when a note is
    /// deleted, because a note can go without Obelus watching -- another
    /// Obelus, the reader's own editor -- and a table that only shrank when
    /// Obelus was looking would grow for ever.
    ///
    /// `None` is "Obelus does not know what notes there are", which is a
    /// different thing from "there are none" and was the same thing:
    /// `Todo::read` answered with an empty list for a file it could not
    /// read, and an empty list here forgets every conversation in the project.
    /// A name that cannot be checked against anything is not a name that has
    /// gone.
    pub fn forget_notes_that_are_gone(&mut self, notes: Option<&[NoteId]>) {
        let Some(notes) = notes else {
            return;
        };
        // Only the ones a note could have taken away with it. A
        // conversation about nothing in particular is not named by
        // anything in that file, so sweeping it against that file would
        // forget every one of them on the first pass.
        self.kept
            .retain(|(which, _, _), _| which.note().is_none_or(|note| notes.contains(note)));
    }

    /// Forgets every conversation whose checkout has gone.
    ///
    /// Nothing else would: the agent keeps a conversation under the
    /// directory it was told, so one had in a worktree that was removed can
    /// be taken up from nowhere, and its row sat dim in every other
    /// checkout's list for ever. Done as the file is read rather than as it
    /// is written, so the list stops offering it the moment the tree goes
    /// and the next write takes it out of the file -- the same moment the
    /// notes' sweep is made.
    ///
    /// Only a tree that is certainly not there: one the filesystem would
    /// not answer about is not one that has gone.
    fn forget_trees_that_are_gone(&mut self) {
        self.kept
            .retain(|(_, _, tree), _| !obelus_git::is_gone(tree));
    }

    /// Every session it holds, for asking an agent which it still knows.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.kept.values().map(|kept| kept.session.as_str())
    }

    /// Forgets every conversation the agent no longer has.
    ///
    /// The other half of reconciling: a session Obelus holds that the agent
    /// has never heard of would be found out by a `session/load` that fails,
    /// which is a worse way to find out -- the reader has already pressed
    /// the key and is looking at an empty conversation.
    pub fn forget_what_the_agent_lost(&mut self, agent: &str, still_has: &[String]) {
        self.kept
            .retain(|(_, whose, _), kept| whose != agent || still_has.contains(&kept.session));
    }
}

/// Where one project's table is kept.
///
/// Named by the same answer the notes are, and it has to be the same
/// answer: this table says which conversation is about which note, so a
/// key that told two checkouts apart while the notes no longer did would
/// be one note with two conversations under it and neither checkout able
/// to say the other's was there. Which checkout a conversation can be
/// taken up in is said on its row instead -- see [`Remembered`].
///
/// It read the checkout's own path once, for the opposite reason, and the
/// reason went when the notes did: two checkouts of one repository used to
/// be two sets of notes.
#[must_use]
pub fn path(root: &Path) -> Option<PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("sessions")
            .join(format!("{}.toml", obelus_git::project(root)?)),
    )
}

/// What reading a project's table found.
///
/// Three answers and not two, for the reason the notes beside it give three
/// and the settings gave three first: a file that will not read is not a
/// file with nothing in it, and "nothing in it" is a thing [`change`] writes
/// back. This table is read, changed and written whole, so one answer for
/// both meant a file somebody's editor had left half-written was a file
/// replaced by whatever this session happened to hold -- every conversation
/// in the project forgotten, while the agent still had every word of them.
#[derive(Debug)]
pub enum Reading {
    /// There is none yet, which is where every project starts. Also where this
    /// system has nowhere to keep one, which comes to the same thing: there
    /// is nothing to read and nothing will be written either.
    Nothing,
    /// Here they are.
    Remembered(Remembered),
    /// There is one and it could not be read, with what went wrong.
    Unreadable(String),
}

impl Reading {
    /// What is remembered, where remembering nothing is an answer the caller
    /// can live with.
    ///
    /// `None` for a file that would not read. True of everything that only
    /// looks something up -- the cost of an answer it does not have is a
    /// conversation replayed -- and never true of anything that writes.
    #[must_use]
    pub fn remembered(self) -> Option<Remembered> {
        match self {
            Self::Nothing => Some(Remembered::default()),
            Self::Remembered(remembered) => Some(remembered),
            Self::Unreadable(_) => None,
        }
    }
}

/// What the file says.
#[must_use]
pub fn read(root: &Path) -> Reading {
    let Some(path) = path(root) else {
        return Reading::Nothing;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Not there yet is the ordinary case: the file is written the first
        // time a conversation is worth writing down.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Reading::Nothing,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let mut reading = read_from(&text);
    if let Reading::Remembered(remembered) = &mut reading {
        remembered.forget_trees_that_are_gone();
    }
    reading
}

/// The same, from the text rather than the file.
///
/// Apart so that a test can put what [`to_toml`] writes straight back in:
/// what Obelus writes, Obelus has to be able to read, and a round trip
/// through a temporary directory proves it about the filesystem rather
/// than about the format.
#[must_use]
fn read_from(text: &str) -> Reading {
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let mut kept = BTreeMap::new();
    // A file that parses and has no table of conversations in it: read as
    // generously as a note with no name is. This is about the file as a
    // whole, not about what is in it.
    let Some(written) = table.get("talked").and_then(toml::Value::as_array) else {
        return Reading::Remembered(Remembered::default());
    };
    for value in written {
        let Some(row) = value.as_table() else {
            continue;
        };
        let text = |key: &str| row.get(key).and_then(toml::Value::as_str);
        let (Some(agent), Some(session), Some(tree)) =
            (text("agent"), text("session"), text("tree"))
        else {
            continue;
        };
        // A row with no note is one about nothing in particular, which is
        // named by its session and by nothing else. Every row written
        // before those were remembered has one, so an old file reads as it
        // always did.
        let which = match text("note") {
            Some(note) => match NoteId::read(note) {
                Some(note) => ChatId::Note(note),
                None => continue,
            },
            None => ChatId::Loose(session.to_string()),
        };
        kept.insert(
            (which, agent.to_string(), PathBuf::from(tree)),
            Kept {
                session: session.to_string(),
                title: text("title").map(str::to_string),
                told: text("told").map(str::to_string),
                introduced: row
                    .get("introduced")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                last: row.get("last").and_then(toml::Value::as_integer),
            },
        );
    }
    Reading::Remembered(Remembered { kept })
}

/// Reads, changes, and writes back.
///
/// Read-modify-write rather than holding a copy, because a second Obelus on
/// the same project is an ordinary thing to have running and the last one to
/// write would otherwise put back the other's conversations as they were
/// before it opened them.
/// Hands back what it wrote, or nothing where it wrote nothing.
///
/// So that a caller keeping a copy is given the one this just made rather
/// than reading the file again for it: the read is the dear half -- 37us
/// for one conversation, 351us for twenty -- and this has done it already.
pub fn change(
    root: &Path,
    notes: Option<&[NoteId]>,
    what: impl FnOnce(&mut Remembered),
) -> Option<Remembered> {
    let path = path(root)?;
    // Nothing at all where the file will not read. Every other way of
    // declining here leaves it alone; going on would write what Obelus can
    // make of a file it cannot read over the file itself, which is every
    // conversation in this project traded for a parse error.
    //
    // Said to the log and not to the reader, unlike the notes: this file is
    // Obelus's own bookkeeping in its own state directory, and there is
    // nothing for them to go and fix. What they see is a conversation that
    // has to be started again.
    let Some(mut remembered) = read(root).remembered() else {
        tracing::warn!(path = %path.display(), "will not read, so nothing is remembered over it");
        return None;
    };
    what(&mut remembered);
    remembered.forget_notes_that_are_gone(notes);
    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        tracing::warn!(%error, path = %path.display(), "no directory to remember conversations in");
        return None;
    }
    // Through a name beside it and a rename, the way the notes and the
    // settings are written: a crash halfway leaves the old file rather than
    // half of the new one.
    // This process's own name beside it and not one every Obelus shares:
    // two writing at once into one shared name truncate each other's
    // half-written file, and the first rename takes the other's away.
    let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
    if let Err(error) =
        std::fs::write(&beside, to_toml(&remembered)).and_then(|()| std::fs::rename(&beside, &path))
    {
        tracing::warn!(%error, path = %path.display(), "the conversations were not remembered");
        // And not left behind: the name is this process's own, so nobody
        // else will ever write over it, and a failed rename would leave one
        // more of them for every Obelus that failed.
        let _ = std::fs::remove_file(&beside);
        return None;
    }
    Some(remembered)
}

/// What the file holds, as text.
#[must_use]
fn to_toml(remembered: &Remembered) -> String {
    let mut out = String::new();
    for ((which, agent, tree), kept) in &remembered.kept {
        out.push_str("[[talked]]\n");
        if let Some(note) = which.note() {
            out.push_str(&format!("note = \"{note}\"\n"));
        }
        out.push_str(&format!("agent = {}\n", quoted(agent)));
        out.push_str(&format!("tree = {}\n", quoted(&tree.to_string_lossy())));
        out.push_str(&format!("session = {}\n", quoted(&kept.session)));
        if let Some(title) = &kept.title {
            out.push_str(&format!("title = {}\n", quoted(title)));
        }
        if let Some(told) = &kept.told {
            out.push_str(&format!("told = {}\n", quoted(told)));
        }
        // Only when it is true: `false` is what a missing line already
        // means, and a file carrying every default is a file a reader
        // cannot read their own decisions out of.
        if kept.introduced {
            out.push_str("introduced = true\n");
        }
        if let Some(last) = kept.last {
            out.push_str(&format!("last = {last}\n"));
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

    fn note(said: &str) -> ChatId {
        ChatId::Note(NoteId::read(said).expect("a name"))
    }

    /// The checkout these are had in, which none of them is about.
    fn here() -> &'static Path {
        Path::new("/somewhere/obelus")
    }

    /// What goes out comes back, including the parts TOML has opinions
    /// about.
    #[test]
    fn a_conversation_survives_the_file() {
        let mut remembered = Remembered::default();
        remembered.put(
            &note("ABCDEFGH"),
            "claude-acp",
            here(),
            Kept {
                session: "s-1".to_string(),
                title: Some("quotes \" and \\ backslashes".to_string()),
                // A note is allowed to be a paragraph, so this is the one
                // field that routinely has newlines in it.
                told: Some("what it said\n\nand the rest of it".to_string()),
                introduced: true,
                last: Some(1_700_000_000),
            },
        );
        // The same note with a second agent, which is a second conversation:
        // an agent cannot be handed a session another one minted.
        remembered.put(
            &note("ABCDEFGH"),
            "codex",
            here(),
            Kept {
                session: "other".to_string(),
                title: None,
                told: None,
                introduced: false,
                last: None,
            },
        );

        let table = to_toml(&remembered)
            .parse::<toml::Table>()
            .unwrap_or_else(|error| panic!("{error}\n{}", to_toml(&remembered)));
        let written = table.get("talked").and_then(toml::Value::as_array);
        assert_eq!(written.map(Vec::len), Some(2));
        assert_eq!(
            remembered
                .get(&note("ABCDEFGH"), "claude-acp", here())
                .map(|kept| kept.session.as_str()),
            Some("s-1")
        );
        assert_ne!(
            remembered.get(&note("ABCDEFGH"), "codex", here()),
            remembered.get(&note("ABCDEFGH"), "claude-acp", here()),
            "two agents were given one conversation between them"
        );
        // What the agent was told, through the file and back: a paragraph
        // written as it stands would end the string on its first newline,
        // and a note is allowed to be a paragraph.
        let told = written
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("agent").and_then(toml::Value::as_str) == Some("claude-acp")
                })
            })
            .and_then(|row| row.get("told"))
            .and_then(toml::Value::as_str);
        assert_eq!(told, Some("what it said\n\nand the rest of it"));
        // And the bit beside it, which is the other half of "what has this
        // agent already been told". Read back through the file rather than
        // off the map: a field Obelus writes and cannot read is a field
        // that repeats itself every time Obelus is started.
        //
        // Broken deliberately by dropping the `introduced` line from
        // `to_toml`, which reads back as `false` and tells the agent who it
        // is talking to again on the next message.
        let back = match read_from(&to_toml(&remembered)) {
            Reading::Remembered(back) => back,
            other => panic!("the file did not read: {other:?}"),
        };
        assert_eq!(
            back.get(&note("ABCDEFGH"), "claude-acp", here())
                .map(|kept| kept.introduced),
            Some(true)
        );
        assert_eq!(
            back.get(&note("ABCDEFGH"), "codex", here())
                .map(|kept| kept.introduced),
            Some(false)
        );
    }

    /// A conversation is the checkout's it was had in, through the file
    /// and back.
    ///
    /// Broken deliberately by writing every row's `tree` as one directory
    /// in `to_toml`: the row read back is no longer this checkout's.
    #[test]
    fn a_conversation_belongs_to_the_checkout_it_was_had_in() {
        let there = Path::new("/somewhere/obelus-worktree");
        let kept = |session: &str| Kept {
            session: session.to_string(),
            title: None,
            told: None,
            introduced: false,
            last: None,
        };
        let mut remembered = Remembered::default();
        remembered.put(&note("ABCDEFGH"), "claude-acp", here(), kept("here"));
        let back = match read_from(&to_toml(&remembered)) {
            Reading::Remembered(back) => back,
            other => panic!("the file did not read: {other:?}"),
        };
        assert_eq!(
            back.get(&note("ABCDEFGH"), "claude-acp", here())
                .map(|kept| kept.session.as_str()),
            Some("here")
        );
        assert_eq!(
            back.get(&note("ABCDEFGH"), "claude-acp", there),
            None,
            "a conversation was offered in a checkout the agent was never told of"
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
                here(),
                Kept {
                    session: name.to_lowercase(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: None,
                },
            );
        }
        remembered.forget_notes_that_are_gone(Some(&[NoteId::read("ABCDEFGH").expect("a name")]));
        assert!(
            remembered
                .get(&note("ABCDEFGH"), "claude-acp", here())
                .is_some()
        );
        assert!(
            remembered
                .get(&note("JKMNPQRS"), "claude-acp", here())
                .is_none(),
            "a conversation outlived the note it was about"
        );
    }

    /// A file that will not read is not a file with nothing in it, and
    /// nothing is written over one.
    ///
    /// This table is read, changed and written whole. Reading a file it
    /// could not parse as an empty one meant the next thing to remember a
    /// conversation wrote an almost-empty table over it -- every
    /// conversation in the project forgotten, while the agent still held every
    /// word of them and nothing here could name one again.
    ///
    /// Broken deliberately by reading a file that will not parse as an empty
    /// one: the write goes ahead and this goes red.
    #[test]
    fn a_file_that_will_not_read_is_not_written_over() {
        // Said before the directory is asked for: this one builds its
        // project *inside* Obelus's state directory, so without it the
        // test leaves a table in the reader's own -- which it had been
        // doing since it was written.
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-sessions-state-{}", std::process::id())),
        );
        let Some(state) = obelus_logging::state_directory() else {
            return;
        };
        let root = state.join("sessions-unreadable-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        let path = path(&root).expect("somewhere to keep it");
        std::fs::create_dir_all(path.parent().expect("the directory")).expect("the directory");
        let half = "[[talked]]\nnote = \"ABCDEFGH\"\nagent = \"half a no";
        std::fs::write(&path, half).expect("the half-written table");

        assert!(
            matches!(read(&root), Reading::Unreadable(_)),
            "a file that will not parse read as a file with nothing in it"
        );
        change(&root, None, |kept| {
            kept.put(
                &note("JKMNPQRS"),
                "claude-acp",
                here(),
                Kept {
                    session: "one this Obelus made up".to_string(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: None,
                },
            );
        });
        assert_eq!(
            std::fs::read_to_string(&path).expect("the table"),
            half,
            "the table Obelus could not read was written over"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A conversation whose checkout has gone is not read, and the next
    /// write takes it out of the file.
    ///
    /// Broken deliberately by taking the sweep out of `read`: the removed
    /// worktree's row comes back.
    #[test]
    fn a_conversation_whose_checkout_has_gone_is_collected() {
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-sessions-state-{}", std::process::id())),
        );
        let Some(state) = obelus_logging::state_directory() else {
            return;
        };
        let root = state.join("sessions-gone-tree-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        let there = state.join("sessions-gone-tree-test-two");
        std::fs::create_dir_all(&there).expect("the other checkout");
        let kept = |session: &str| Kept {
            session: session.to_string(),
            title: None,
            told: None,
            introduced: false,
            last: None,
        };
        change(&root, None, |remembered| {
            remembered.put(&note("ABCDEFGH"), "claude-acp", &root, kept("here"));
            remembered.put(&note("ABCDEFGH"), "claude-acp", &there, kept("there"));
        });
        let both = read(&root).remembered().expect("the table");
        assert!(
            both.get(&note("ABCDEFGH"), "claude-acp", &there).is_some(),
            "a checkout that is there was forgotten"
        );

        std::fs::remove_dir_all(&there).expect("the checkout goes");
        let read_back = read(&root).remembered().expect("the table");
        assert!(
            read_back
                .get(&note("ABCDEFGH"), "claude-acp", &root)
                .is_some(),
            "this checkout's conversation went with the other"
        );
        assert!(
            read_back
                .get(&note("ABCDEFGH"), "claude-acp", &there)
                .is_none(),
            "a conversation from a checkout that has gone was read"
        );
        change(&root, None, |_| {});
        let written = std::fs::read_to_string(path(&root).expect("the table")).expect("the table");
        assert!(
            !written.contains("sessions-gone-tree-test-two"),
            "a conversation from a checkout that has gone was written back:\n{written}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// And one whose notes Obelus could not read is not forgotten at all.
    ///
    /// "There are no notes" and "Obelus cannot tell what notes there are"
    /// were one answer, and this table is swept against it: a `todo.toml`
    /// somebody had left half-edited meant every conversation in the project
    /// went, on the next message anybody sent.
    ///
    /// Broken deliberately by sweeping against `None` as though it were an
    /// empty list: both of these go.
    #[test]
    fn nothing_is_forgotten_against_notes_that_could_not_be_read() {
        let mut remembered = Remembered::default();
        for name in ["ABCDEFGH", "JKMNPQRS"] {
            remembered.put(
                &note(name),
                "claude-acp",
                here(),
                Kept {
                    session: name.to_lowercase(),
                    title: None,
                    told: None,
                    introduced: false,
                    last: None,
                },
            );
        }
        remembered.forget_notes_that_are_gone(None);
        assert!(
            remembered
                .get(&note("ABCDEFGH"), "claude-acp", here())
                .is_some()
        );
        assert!(
            remembered
                .get(&note("JKMNPQRS"), "claude-acp", here())
                .is_some(),
            "a conversation was forgotten against a list Obelus does not have"
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
            here(),
            Kept {
                session: "s-1".into(),
                title: None,
                told: None,
                introduced: false,
                last: None,
            },
        );
        remembered.put(
            &note("JKMNPQRS"),
            "claude-acp",
            here(),
            Kept {
                session: "s-2".into(),
                title: None,
                told: None,
                introduced: false,
                last: None,
            },
        );
        // A second agent's, which this one's answer says nothing about.
        remembered.put(
            &note("ABCDEFGH"),
            "codex",
            here(),
            Kept {
                session: "x-9".into(),
                title: None,
                told: None,
                introduced: false,
                last: None,
            },
        );

        remembered.forget_what_the_agent_lost("claude-acp", &["s-1".to_string()]);

        assert!(
            remembered
                .get(&note("ABCDEFGH"), "claude-acp", here())
                .is_some()
        );
        assert!(
            remembered
                .get(&note("JKMNPQRS"), "claude-acp", here())
                .is_none(),
            "a session the agent has never heard of was kept"
        );
        assert!(
            remembered.get(&note("ABCDEFGH"), "codex", here()).is_some(),
            "one agent's answer threw away another agent's conversation"
        );
    }

    /// Another Obelus halfway through writing this table is left alone.
    ///
    /// Broken deliberately by writing beside it as `toml.writing` again,
    /// with no process number: the other's file is truncated and renamed
    /// away under it.
    #[test]
    fn another_obelus_writing_the_table_is_left_alone() {
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-sessions-state-{}", std::process::id())),
        );
        let Some(state) = obelus_logging::state_directory() else {
            return;
        };
        let root = state.join("sessions-beside-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        let theirs = path(&root)
            .expect("somewhere to keep it")
            .with_extension("toml.writing");
        std::fs::create_dir_all(theirs.parent().expect("the directory")).expect("the directory");
        std::fs::write(&theirs, "another Obelus is halfway through this").expect("theirs");

        assert!(change(&root, None, |_| {}).is_some(), "nothing was written");
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("another Obelus is halfway through this"),
            "the other Obelus's half-written table was taken"
        );
    }
}
