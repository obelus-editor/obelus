//! What Obelus says to an agent before the reader's first words.
//!
//! Two pieces, and the difference between them is the whole of this module.
//! The first is said in every conversation and never changes: who the agent
//! is talking to, and how to put a question to them -- and, where the
//! project has chosen one, that it has a workflow. The second is whatever
//! the conversation is *about*, which is [`Topic`] -- so the way to add an
//! opening for a new kind of conversation is to add a variant there, and
//! the match below stops compiling until somebody decides what it says.
//!
//! The prose is in `.txt` files beside this one rather than in the source,
//! because it is prose: it is read and reworded by whoever is tuning what
//! an agent does, and a paragraph broken across string continuations is a
//! paragraph nobody rereads.
//!
//! Neither piece is said twice, and the two remember that differently
//! because their subjects differ. The first cannot change, so "has it gone"
//! is a bit. The second is the reader's own file, which they rewrite
//! between two messages, so what is written down is what the note *said* --
//! and Obelus compares rather than checks a flag. Rewording the `.txt`
//! around it is not the note being rewritten and does not send it again.

use obelus_git::todo::NoteId;

use super::*;
use crate::conversation::Topic;

/// Who the agent is talking to, in every conversation.
const ALWAYS: &str = include_str!("always.txt");

/// That the project has a workflow, and where to read it.
///
/// A line rather than the workflow, because most conversations change
/// nothing and the workflow is several paragraphs: the agent reads it when
/// it is about to change something, which is how a skill is loaded too. And
/// it is told to read it again at the end, because a long conversation is
/// summarised by the agent and the steps that come last are the ones a
/// summary drops.
const WORKFLOW: &str = include_str!("workflow.txt");

/// Every workflow, by the name the setting gives it.
///
/// One file each, holding both of the things a workflow says, so that the
/// two are written side by side: what the list it is chosen from says
/// about it, then a line of `----`, then what `read_workflow` hands an
/// agent. A file with nothing under the line, or no line, asks nothing of
/// the agent -- which is what `none` is, and why it is a file like the
/// others rather than a case in the code.
///
/// The names are written twice, here and in `obelus-config`, which reads
/// the setting and cannot see these files: the test below is what keeps
/// the two lists the same list.
const WORKFLOWS: &[(&str, &str)] = &[
    ("none", include_str!("workflows/none.txt")),
    (
        "feature-branch",
        include_str!("workflows/feature-branch.txt"),
    ),
];

/// A workflow's file, as what the list says and what the agent is handed.
fn read_workflow(file: &'static str) -> (&'static str, Option<&'static str>) {
    match file.split_once("\n----\n") {
        Some((about, asked)) => (
            about.trim(),
            Some(asked.trim()).filter(|asked| !asked.is_empty()),
        ),
        None => (file.trim(), None),
    }
}

/// Every workflow's name, and what the list it is chosen from says about
/// it.
pub(super) fn workflows() -> impl Iterator<Item = (&'static str, &'static str)> {
    WORKFLOWS
        .iter()
        .map(|(name, file)| (*name, read_workflow(file).0))
}

/// What a conversation about a note says it is about.
const NOTE: &str = include_str!("note.txt");

/// And what it says when that note has been rewritten since.
///
/// The difference on its own: the agent kept the first telling, and what
/// it needs is what changed.
const REWORDED: &str = include_str!("reworded.txt");

/// What Obelus has to say before the reader's own words, this time.
///
/// Three things rather than one string, because saying it is three things
/// at once: the block that goes in front of the reader's words, the line
/// the transcript shows where Obelus put something of the reader's own in
/// there, and what the topic was made from -- written down beside the
/// conversation, so that tomorrow's Obelus can tell whether the agent is
/// out of date.
pub(super) struct Opening {
    /// The block, first of the prompt's own.
    pub words: String,
    /// What the transcript says Obelus did, where it did anything the
    /// reader has a stake in.
    ///
    /// Only the topic fills this. What Obelus puts in the prompt in the
    /// reader's name is the reader's to see -- and the piece that says who
    /// the agent is talking to is not in their name: it is the client
    /// introducing itself, about Obelus rather than about them or their
    /// work. A line saying so would be the same line at the head of every
    /// conversation they ever open, which is a row that has stopped
    /// telling anybody anything.
    pub said: Option<&'static str>,
    /// What the topic was made from, where the topic had something to say.
    pub now: Option<String>,
    /// Whether this carries the piece that goes once per conversation.
    pub introduced: bool,
}

impl App {
    /// Everything Obelus has to tell this agent before the reader's words,
    /// given what it has been told already.
    ///
    /// `None` when it has been told all of it, which is the ordinary case:
    /// this is asked before every message and has something to say before
    /// the first one and after the reader rewrites their note.
    pub(super) fn opening(
        &self,
        topic: &Topic,
        introduced: bool,
        told: Option<&str>,
    ) -> Option<Opening> {
        let mut pieces = Vec::new();
        if !introduced {
            pieces.push(ALWAYS.trim().to_string());
            if self.workflow().is_some() {
                pieces.push(WORKFLOW.trim().to_string());
            }
        }
        let about = self.about_the_topic(topic, told);
        if let Some((words, _, _)) = &about {
            pieces.push(words.clone());
        }
        if pieces.is_empty() {
            return None;
        }
        Some(Opening {
            // One block rather than one each: the boundary between two
            // content blocks says nothing to a model, and every type
            // between here and the wire would have to become a list to
            // carry it.
            words: pieces.join("\n\n"),
            said: about.as_ref().map(|(_, line, _)| *line),
            now: about.map(|(_, _, now)| now),
            introduced: true,
        })
    }

    /// The workflow this project has chosen, if it has chosen one.
    ///
    /// A word Obelus does not know is no workflow, which is what it was
    /// before the reader wrote it: an agent told to follow nothing in
    /// particular goes about a change its own way.
    fn workflow(&self) -> Option<&'static str> {
        let (_, file) = WORKFLOWS
            .iter()
            .find(|(name, _)| *name == self.config().workflow)?;
        read_workflow(file).1
    }

    /// What `read_workflow` answers.
    ///
    /// Asked of the settings as they are now rather than as they were when
    /// the conversation began, so a reader who changes the workflow
    /// half-way is heard the next time the agent reads it.
    pub(super) fn workflow_for_an_agent(&self) -> String {
        self.workflow().map_or_else(
            || "this project has no workflow, so change it the way you would anyway".to_string(),
            str::to_string,
        )
    }

    /// What the conversation being about something adds, if anything.
    ///
    /// The words, the line for the transcript, and what those words were
    /// made from. Exhaustive on [`Topic`] on purpose: a new kind of
    /// conversation is a new decision about what an agent is told it is
    /// in, and a wildcard here would let one ship having said nothing.
    fn about_the_topic(
        &self,
        topic: &Topic,
        told: Option<&str>,
    ) -> Option<(String, &'static str, String)> {
        match topic {
            // Nothing Obelus knows that the agent does not: what a loose
            // conversation is about is whatever the reader types.
            Topic::Loose => None,
            Topic::Note(note) => self.about_the_note(note, told),
        }
    }

    /// The note a conversation is about, as much of it as the agent is
    /// missing.
    ///
    /// The whole of what the note says rather than its first line: the
    /// first line is what the reader called it and the rest is what they
    /// meant. Its name goes with it, because `todo_finish` takes a name
    /// and an agent that had to guess which of the notes it was looking at
    /// would rather not call it at all.
    ///
    /// Read from the file every time, because the note is the reader's and
    /// they rewrite it -- in Obelus, in their own editor, between two
    /// messages. So this answers "what does this agent not know" rather
    /// than "has it been told yet": once for a conversation that is new,
    /// again whenever the note has changed under one that is not, and
    /// nothing at all in between. A conversation about a note that has
    /// since been deleted is the one case with nothing to say: there is no
    /// note to quote, and the agent finds out the way anybody does, by
    /// `todo_finish` answering that no note has that name.
    fn about_the_note(
        &self,
        note: &NoteId,
        told: Option<&str>,
    ) -> Option<(String, &'static str, String)> {
        let about = obelus_git::todo::read(&self.working_directory)
            .notes()
            .unwrap_or_default()
            .notes
            .into_iter()
            .find(|other| other.id == *note)?;
        let now = what_the_note_says(&about);
        if told == Some(now.as_str()) {
            return None;
        }
        let at = about.at.as_ref().map_or_else(String::new, |at| {
            format!("\nIt is about {}:{}.", at.path.display(), at.line.get() + 1)
        });
        let (template, said) = match told {
            None => (NOTE, "Told the agent what this conversation is about"),
            Some(_) => (REWORDED, "Told the agent the note has been rewritten"),
        };
        Some((filled(template, &about.id, &at, &about.said), said, now))
    }
}

/// What can change about a note, in the note's own data and none of
/// Obelus's words: rewording the templates is not the note being rewritten
/// and must not read as it.
///
/// What is written down as told, and so what the next message is compared
/// against -- by the opening, and by the box deciding whether to offer
/// [`LOOK`].
pub(super) fn what_the_note_says(about: &obelus_git::todo::Note) -> String {
    match &about.at {
        Some(at) => format!(
            "{}\n{}:{}",
            about.said,
            at.path.display(),
            at.line.get() + 1
        ),
        None => about.said.clone(),
    }
}

/// What the box offers to say in a conversation about a note, while the
/// next message would carry the note.
///
/// The note is the question and the agent is about to be told it, so what
/// the reader has left to say is usually only that they would like an
/// answer -- and typing that every time is the cost of opening a
/// conversation from a note at all.
pub(super) const LOOK: &str = "Look into this";

/// Fills a template in: Obelus's own values first, the reader's words last.
///
/// The order is the whole of it. A note whose text happens to contain
/// `{name}` would otherwise have that stand in for the note's name -- the
/// reader's own words reread as a template -- and it cannot once they go in
/// after every other placeholder has already gone.
fn filled(template: &str, name: &NoteId, at: &str, said: &str) -> String {
    template
        .trim()
        .replace("{name}", &name.to_string())
        .replace("{at}", at)
        .replace("{said}", said)
}

#[cfg(test)]
mod tests {
    use obelus_git::todo::NoteId;

    /// A note that happens to look like a template is left alone.
    ///
    /// The reader writes their notes; nothing stops one of them containing
    /// the words `{name}`, and when it does those words are theirs and not
    /// a hole for Obelus to fill. Which is true only because their words go
    /// in after every other placeholder has already gone.
    ///
    /// Broken deliberately by filling `{said}` in first, which puts the
    /// note's own name in the middle of the reader's sentence.
    #[test]
    fn a_note_that_looks_like_a_template_is_left_alone() {
        let note = NoteId::read("0123456A").expect("a name");
        let filled = super::filled("{said} -- {name}", &note, "", "mind the {name} here");
        assert_eq!(filled, "mind the {name} here -- 0123456A");
    }

    /// The workflows here are the ones the setting accepts, and each says
    /// what it does.
    ///
    /// Two lists of one set of names, because the setting is read where
    /// these files cannot be seen. Broken deliberately by adding a name to
    /// the setting's list with no file beside it, and by taking the line
    /// above `----` out of `feature-branch.txt`.
    #[test]
    fn every_workflow_the_setting_accepts_is_written_here() {
        let accepted = match obelus_config::Setting::named("workflow").map(|setting| setting.kind) {
            Some(obelus_config::Kind::Choice(words)) => words,
            other => panic!("the workflow setting is not a choice: {other:?}"),
        };
        let written: Vec<&str> = super::workflows().map(|(name, _)| name).collect();
        assert_eq!(accepted, written.as_slice());
        for (name, file) in super::WORKFLOWS {
            let (about, _) = super::read_workflow(file);
            assert!(
                !about.is_empty() && !about.contains("----"),
                "{name} does not say what it does: {about:?}"
            );
        }
        assert!(
            super::read_workflow(super::WORKFLOWS[0].1).1.is_none(),
            "none asks something of the agent"
        );
        assert!(
            super::read_workflow(super::WORKFLOWS[1].1)
                .1
                .is_some_and(|asked| asked.contains("git worktree add")),
            "feature-branch hands the agent nothing"
        );
    }
}
