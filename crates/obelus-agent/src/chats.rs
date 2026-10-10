//! Which of a project's notes have a conversation open, and in whose Obelus.
//!
//! Obelus does not split its window, so several Obelus processes on one
//! project is the normal case -- and a conversation is not a thing two of
//! them may have open at once. The agent keeps what was said and takes one
//! prompt turn at a time; two clients prompting one conversation is two
//! turns in it, which is the thing Obelus's own queue exists to prevent,
//! arriving from outside the process that queue lives in.
//!
//! So a conversation is claimed, and the claim is a lock the operating
//! system holds rather than anything Obelus writes down. A process that is
//! killed, crashes or loses power releases it on the way out without having
//! to be asked, which is the whole reason for choosing a lock over a process
//! number in a file: a number has to be believed, checked against a process
//! that may be somebody else's by now, and given a staleness nobody can pick
//! -- ten minutes is too long for a reader waiting and too short for a
//! conversation left open over lunch.
//!
//! The file itself is what the watcher can see. A lock is invisible to it --
//! nothing is written when one is taken -- so the file is created with the
//! claim and removed with it, and that is the signal another Obelus wakes
//! on. The file's *existence* means nothing on its own: one left behind by a
//! process that died is a file nobody holds, and asking for the lock is what
//! says which it is.
//!
//! What the file *says* is which checkout holds it, so that a note locked
//! from another worktree can say which one -- the table of conversations is
//! every worktree's and so is this directory. Written by the holder once the
//! lock is its own, so it is only ever believed beside a lock: a file left
//! by an Obelus that died still names a checkout, and nobody holds it.
//!
//! **A refused claim is drawn, never said.** Nothing goes on the status row
//! when the key is refused. The lock beside the note says it, and the foot
//! says it again by not offering `Talk` there: the reader is told before
//! they press, which is the rule the palette follows for a command it will
//! not run. The list of conversations says the same thing the same way --
//! the lock in the marker column, the row dim -- and asks for the claim
//! *again* when the row is chosen, because the list was built a moment ago
//! and another Obelus may have walked in since. Where it has, the list stays
//! open and the row goes dim under the reader, which is the answer; nothing
//! happening at all is a key that looks broken. What the status row says,
//! while the caret is in such a note, is where it is being talked about --
//! `Talked about in obelus-worktree-1` -- which is where the mode goes on a
//! file's row and the same kind of fact.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use obelus_todo::NoteId;

/// Which conversation, for the two things Obelus keeps beside one: the
/// claim here and the name written down in [`crate::acp::sessions`].
///
/// A note where there is one, and the agent's own name for the conversation
/// where there is not. It has to be the note for a note's conversation and
/// not the session: the notes page reaches one before a session exists --
/// that is the whole reason the claim is taken before the conversation is
/// opened -- and a key that was the session there would be a second door
/// into one conversation, with an Obelus behind each.
///
/// The other kind has no such door. Nothing but the agent's name reaches it,
/// so that name is what says which.
///
/// A pull request is a note's shape again: the list of them is a door that
/// opens before any session exists, so its number is what says which.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChatId {
    /// One of the project's notes.
    Note(NoteId),
    /// One about nothing in particular, by the session the agent minted.
    Loose(String),
    /// A review of one of the repository's pull requests, by its number.
    PullRequest(u64),
    /// An answer to one of the repository's issues, by its number.
    Issue(u64),
}

/// What tells a loose conversation's file from a note's.
///
/// A note's name is eight characters of Crockford's alphabet, so no note
/// can be spelled with this in front of it.
const LOOSE: &str = "loose-";

/// What tells a pull request's file from the other two, for the same
/// reason: no note's name has a dash in it, and no loose one starts here.
const PULL: &str = "pull-";

/// And an issue's.
const ISSUE: &str = "issue-";

impl ChatId {
    /// The file this claim lives in, under [`directory`].
    ///
    /// A note is its own name, which every claim written before this was a
    /// pair was -- so a conversation somebody has open right now stays open
    /// across the change rather than coming back as a second claim on the
    /// same note.
    ///
    /// A session id is the agent's, which is to say it is any string at
    /// all: Copilot's are uuids and nothing promises the next agent's will
    /// be. So everything but the characters a filename may certainly hold
    /// is written as its bytes, which is also what makes [`Self::read`] the
    /// exact other direction.
    #[must_use]
    pub fn file_name(&self) -> String {
        let session = match self {
            Self::Note(note) => return note.as_str().to_string(),
            Self::PullRequest(number) => return format!("{PULL}{number}"),
            Self::Issue(number) => return format!("{ISSUE}{number}"),
            Self::Loose(session) => session,
        };
        let mut name = LOOSE.to_string();
        for byte in session.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' => name.push(byte as char),
                _ => name.push_str(&format!("-{byte:02x}")),
            }
        }
        name
    }

    /// The one that file name names, or nothing where it names neither.
    #[must_use]
    pub fn read(name: &str) -> Option<Self> {
        // Digits and nothing else: `parse` takes a leading `+`, and a name
        // that reads back as another name is two files for one claim.
        let numbered = |number: &str| {
            number
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| number.parse::<u64>().ok())
                .flatten()
        };
        if let Some(number) = name.strip_prefix(PULL) {
            return numbered(number).map(Self::PullRequest);
        }
        if let Some(number) = name.strip_prefix(ISSUE) {
            return numbered(number).map(Self::Issue);
        }
        let Some(rest) = name.strip_prefix(LOOSE) else {
            return NoteId::read(name).map(Self::Note);
        };
        let mut bytes = Vec::new();
        let mut left = rest.as_bytes();
        while let Some((first, after)) = left.split_first() {
            if *first != b'-' {
                bytes.push(*first);
                left = after;
                continue;
            }
            let (pair, after) = after.split_at_checked(2)?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?);
            left = after;
        }
        Some(Self::Loose(String::from_utf8(bytes).ok()?))
    }

    /// The note it is about, where it is about one.
    #[must_use]
    pub const fn note(&self) -> Option<&NoteId> {
        match self {
            Self::Note(note) => Some(note),
            Self::Loose(_) | Self::PullRequest(_) | Self::Issue(_) => None,
        }
    }
}

/// Where a project's claims are kept, one file per conversation.
#[must_use]
pub fn directory(root: &Path) -> Option<PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("chats")
            .join(obelus_git::project(root)?),
    )
}

/// Says this Obelus has the conversation `which` names, unless another has.
///
/// `None` is another Obelus holding it. Also a system with nowhere to keep
/// the claim, and that is deliberately the same answer: a reader whose
/// machine cannot say who has what is a reader for whom Obelus cannot keep
/// this promise, and the honest thing is to decline rather than to let two
/// windows into one conversation while saying nothing.
#[must_use]
pub fn claim(root: &Path, which: &ChatId) -> Option<Claim> {
    let path = directory(root)?.join(which.file_name());
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Opened rather than created exclusively: the file left behind by a
    // process that died is not a claim, and refusing on finding one would
    // hand the reader a conversation they can never open again.
    // For writing, and that is not only because it may have to be created:
    // what says a claim ended with the Obelus that held it is a watcher
    // reporting the file closed *by a writer*, which the kernel does on the
    // way out of a process it is killing. Opened for reading, a claim would
    // end in silence and the next window would go on drawing a lock nobody
    // holds until it looked again for some other reason.
    //
    // Which is what happens on macOS and Windows whatever this is opened
    // as: the event is inotify's and they have none like it. The lock
    // itself is the kernel's on all three and goes with the process
    // everywhere, so nothing is ever unopenable -- what is wrong there is
    // the drawing, until the view opens again. See `obelus_watch`.
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .ok()?;
    if obelus_claim::held_by_somebody_else(&file) {
        return None;
    }
    // After the lock rather than before, so that nothing is said over what
    // the holder wrote: until here, this file was somebody else's to speak
    // for. A claim that could not say where it is held is still a claim.
    if let Err(error) = file
        .set_len(0)
        .and_then(|()| (&file).write_all(root.to_string_lossy().as_bytes()))
    {
        tracing::warn!(%error, path = %path.display(), "a claim does not say where it is held");
    }
    Some(Claim {
        path,
        leave_the_file: false,
        file,
    })
}

/// Every conversation in this project somebody has open, and which checkout
/// has it where the claim says.
///
/// Including this Obelus's own: a lock is about the open file and not about
/// the process, so a second look from the same process finds its own claim
/// in the way. Which of them are this one's is a question this cannot answer
/// and the caller already knows -- it is holding them.
///
/// `None` for a claim that says nothing: one caught between being taken and
/// being written.
#[must_use]
pub fn held(root: &Path) -> BTreeMap<ChatId, Option<PathBuf>> {
    let Some(directory) = directory(root) else {
        return BTreeMap::new();
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        // No conversation has ever been opened in this project, which is
        // where every project starts.
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let which = ChatId::read(entry.file_name().to_str()?)?;
            let mut file = File::options().read(true).open(entry.path()).ok()?;
            // These bytes are nobody's: a claim's lock is on a byte past
            // any end the file could have, which is what leaves them
            // readable here at all -- see `held_by_somebody_else`.
            let mut said = String::new();
            let tree = file
                .read_to_string(&mut said)
                .ok()
                .filter(|_| !said.is_empty())
                .map(|_| PathBuf::from(said));
            obelus_claim::held_by_somebody_else(&file).then_some((which, tree))
        })
        .collect()
}

/// Whether somebody has the conversation `which` names open, this Obelus
/// included.
///
/// [`held`] for one conversation, for a caller that has a few in mind: that
/// one tries the lock on every claim in the project, and each try is an
/// instant in which a claim asked for elsewhere is refused.
#[must_use]
pub fn is_held(root: &Path, which: &ChatId) -> bool {
    directory(root)
        .and_then(|directory| {
            File::options()
                .read(true)
                .open(directory.join(which.file_name()))
                .ok()
        })
        .is_some_and(|file| obelus_claim::held_by_somebody_else(&file))
}

/// A conversation this Obelus has open, which it gives up by being dropped.
///
/// Dropped rather than given up by hand, for the reason the install's claim
/// beside this one is: every way out of a conversation would otherwise have
/// to remember, and the one that forgot would lock the reader out of their
/// own note until they restarted Obelus.
#[derive(Debug)]
pub struct Claim {
    path: PathBuf,
    /// Whether the file goes when the lock does.
    ///
    /// It does, every way out but one: a process that is killed or loses
    /// power drops its lock without running a line of anybody's code, and
    /// the file it left is still there. That is the case the whole of this
    /// is built around -- see [`Claim::as_if_this_obelus_died_for_test`],
    /// which is the only thing that sets this and is only there so that a
    /// test can be that process.
    leave_the_file: bool,
    /// Held open for as long as the claim is: the lock belongs to the open
    /// file and goes when it closes, which is also what makes a killed
    /// Obelus give it up.
    #[expect(
        dead_code,
        reason = "it is the lock itself: what it is for is staying open"
    )]
    file: File,
}

impl Claim {
    /// Lets the lock go the way an Obelus that was killed does: the file
    /// stays exactly where it was.
    ///
    /// The one way a claim can end that writes nothing, removes nothing and
    /// runs none of Obelus's code -- so the only notice of it is the kernel
    /// closing the descriptor, which a watcher reports as a close by a
    /// writer. Nothing in Obelus calls this; a test that wants to be the
    /// process that died does, because it cannot be killed and go on
    /// asserting.
    pub const fn as_if_this_obelus_died_for_test(&mut self) {
        self.leave_the_file = true;
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.leave_the_file {
            // The lock goes with the file this holds, a moment from now,
            // and nothing else happens -- which is the whole of what dying
            // looks like from outside.
            return;
        }
        // The name goes here and the lock goes a moment later, when the
        // file this holds is closed on the way out of this. That order is
        // the useful one: another Obelus wakes on the file going, and by
        // the time it has looked the lock is gone too.
        if let Err(error) = std::fs::remove_file(&self.path) {
            tracing::warn!(%error, path = %self.path.display(), "a claim outlived its conversation");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Somewhere of this run's own, and a project name to go with it.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("obelus-chats-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-chats-state-{}", std::process::id())),
        );
        root
    }

    /// Two Obelus cannot have one note's conversation open at once.
    ///
    /// The agent takes one prompt turn at a time and Obelus queues what the
    /// reader says into a running one -- a queue that lives in one process,
    /// so a second process prompting the same conversation walks straight
    /// past it. This is the half of that promise the other process can see.
    ///
    /// Two claims from one process rather than two processes, and it is the
    /// same question: a `flock` belongs to the open file and not to the
    /// process, so a second `open` of the same path is as much somebody else
    /// as another Obelus is. That is also what makes [`held`] work.
    ///
    /// Broken deliberately by having `claim` answer `Some` without asking
    /// `held_by_somebody_else`: the second one comes back held as well.
    /// A claim says which checkout holds it, and one that is refused says
    /// nothing over it.
    ///
    /// The refused one is asked from the same project spelled another way
    /// -- `root/.` names the same directory of claims -- which is what a
    /// second worktree is to this file: somebody else's checkout asking.
    ///
    /// Broken deliberately by writing before `held_by_somebody_else` is
    /// asked in `claim`: the refused claim writes `root/.` over the holder.
    #[test]
    fn a_claim_says_which_checkout_holds_it() {
        let root = scratch("says-where");
        let note = ChatId::Note(NoteId::read("0123456B").expect("a name"));

        let _held = claim(&root, &note).expect("nobody had it");
        assert!(
            claim(&root.join("."), &note).is_none(),
            "a second checkout was let into the conversation"
        );
        // As written, rather than as paths: a path compares `root/.` and
        // `root` equal, which is the one difference this is looking for.
        assert_eq!(
            held(&root)
                .get(&note)
                .cloned()
                .flatten()
                .map(PathBuf::into_os_string),
            Some(root.clone().into_os_string()),
            "the claim does not say which checkout holds it"
        );
    }

    #[test]
    fn one_note_has_one_conversation_open() {
        let root = scratch("one-at-a-time");
        let note = ChatId::Note(NoteId::read("0123456A").expect("a name"));

        let first = claim(&root, &note).expect("nobody had it");
        assert!(
            claim(&root, &note).is_none(),
            "a second Obelus was let into the conversation"
        );
        assert!(held(&root).contains_key(&note), "the claim does not show");

        // And giving it up gives it up: a reader who closes a conversation
        // in one window can open it in the next.
        drop(first);
        assert!(
            held(&root).is_empty(),
            "the claim outlived the conversation"
        );
        assert!(claim(&root, &note).is_some(), "nobody can have it now");
    }

    /// A conversation about nothing in particular is claimed too.
    ///
    /// It has to be: the list of conversations offers one to be taken up
    /// again, and two Obelus taking up the same one is the thing every
    /// claim here exists to stop -- the notes were only the first way to
    /// reach one.
    ///
    /// And the name it is claimed under survives being written to a
    /// filesystem and read back: a session id is the agent's own string,
    /// and the one Copilot mints has characters in it no directory would
    /// take. What Obelus writes, Obelus has to be able to read.
    ///
    /// Broken deliberately by having `ChatId::read` answer `None` for
    /// anything it does not recognise as a note: `held` stops seeing the
    /// claim, and the second Obelus walks in.
    #[test]
    fn a_conversation_about_nothing_is_claimed_by_its_session() {
        let root = scratch("loose");
        let loose = ChatId::Loose("sess/01 \u{4f60}?*".to_string());

        let first = claim(&root, &loose).expect("nobody had it");
        assert!(
            claim(&root, &loose).is_none(),
            "a second Obelus was let into the conversation"
        );
        assert_eq!(
            ChatId::read(&loose.file_name()),
            Some(loose.clone()),
            "the name does not survive the filesystem"
        );
        assert!(held(&root).contains_key(&loose), "the claim does not show");
        drop(first);
        assert!(held(&root).is_empty(), "the claim outlived it");
    }

    /// A review is claimed by its pull request's number, and that name
    /// reads back as the same number and as nothing else.
    ///
    /// Broken deliberately by having `file_name` leave the prefix off: the
    /// name then reads as no conversation at all, and `held` stops seeing
    /// the claim.
    #[test]
    fn a_review_is_claimed_by_its_number() {
        let root = scratch("pull");
        let pull = ChatId::PullRequest(123);

        let first = claim(&root, &pull).expect("nobody had it");
        assert!(
            claim(&root, &pull).is_none(),
            "a second Obelus was let into the review"
        );
        assert_eq!(ChatId::read(&pull.file_name()), Some(pull.clone()));
        assert_eq!(ChatId::read("pull-+123"), None, "two names for one claim");
        // And an issue of the same number is another conversation, under a
        // name of its own.
        let issue = ChatId::Issue(123);
        assert_eq!(ChatId::read(&issue.file_name()), Some(issue.clone()));
        assert_ne!(issue.file_name(), pull.file_name());
        assert!(held(&root).contains_key(&pull), "the claim does not show");
        drop(first);
    }

    /// Looking at a claim does not announce itself, and still sees it.
    ///
    /// Two halves of one arrangement. A claim is *held* by a writer, so that
    /// the kernel closing a dying process's files reports it -- which is the
    /// only notice there is that a lock ended without anybody writing
    /// anything. And a claim is *looked at* through a read, so that Obelus
    /// going round the directory whenever it is told one moved does not
    /// itself count as a move and send Obelus round again.
    ///
    /// What makes the second possible is `flock`: it belongs to the open
    /// file description rather than to the access mode, so a descriptor with
    /// no write access can still ask for an exclusive lock and be refused.
    /// `fcntl` locks cannot do this, which is the other reason they were not
    /// used.
    ///
    /// Broken deliberately by opening with `.write(true)` in `held`: the
    /// lock is still seen, so nothing here goes red -- and that is the point
    /// of the pair in `tests/watch.rs`, which is where the silence is
    /// asserted. Broken the other way, by taking the read off, `held` opens
    /// nothing and the claim disappears.
    #[test]
    fn a_claim_is_seen_through_a_descriptor_that_cannot_write() {
        let root = scratch("looking");
        let which = ChatId::Loose("s-looked-at".to_string());
        let held_by_them = claim(&root, &which).expect("their claim");

        let path = directory(&root).expect("somewhere").join(which.file_name());
        let looking = File::options()
            .read(true)
            .open(&path)
            .expect("looking at it");
        assert!(
            obelus_claim::held_by_somebody_else(&looking),
            "a claim cannot be seen without write access, so looking would announce itself"
        );
        assert!(held(&root).contains_key(&which), "the walk does not see it");
        drop(held_by_them);
        assert!(held(&root).is_empty(), "the claim outlived its holder");
    }

    /// A claim left behind by an Obelus that died is not a claim.
    ///
    /// Which is the whole reason for a lock rather than a process number
    /// written down: nothing sweeps this directory, nothing has to decide
    /// how old is too old, and a machine that lost power comes back with
    /// every conversation openable.
    ///
    /// Broken deliberately by having `held` answer for the file existing
    /// rather than for the lock: the leftover reads as somebody's.
    #[test]
    fn a_file_nobody_holds_is_not_a_claim() {
        let root = scratch("left-behind");
        let note = ChatId::Note(NoteId::read("0123456B").expect("a name"));
        let path = directory(&root).expect("somewhere").join(note.file_name());
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(&path, "").expect("what the dead Obelus left");

        assert!(held(&root).is_empty(), "a leftover file reads as a claim");
        assert!(
            claim(&root, &note).is_some(),
            "the conversation cannot be opened again"
        );
    }
}
