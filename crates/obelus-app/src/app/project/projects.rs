//! Where Obelus has been, so that a start with nothing to go on has an
//! answer.
//!
//! A reader who types `ob` in a project has said which one by standing in
//! it. A reader who picks Obelus out of a desktop menu has said nothing:
//! the process starts in whatever directory the launcher happened to be
//! in, which is the home directory and is not a project. Obelus used to
//! take it as one, and everything keyed on a project went with it -- the
//! file list walked the whole of `$HOME`, and the notes and the
//! conversations were filed under a directory nobody works in.
//!
//! So Obelus asks, on a page of its own before the welcome screen, and
//! this is what it offers: the projects this reader has had open, newest
//! first. The same shape the conversations have and for the same reason
//! -- what a reader did outlives the window.
//!
//! **A project is remembered however it was named.** By an argument, by
//! the directory Obelus was started in, or by being chosen on that
//! screen: all three are a reader saying where they work, and a list that
//! held only the third would be empty for exactly the reader who works
//! from a terminal and then reaches for the menu once. [`App::work_in`] is
//! the one door all three go through, which is why the remembering is
//! there and not at any of them.
//!
//! **Only a worktree is remembered.** Not every directory Obelus is
//! pointed at is somewhere a reader would come back to: a process started
//! in `$HOME`, in `/`, or in a directory of downloads is the normal case
//! for a launcher, and writing those down would fill the list with the
//! places Obelus happened to begin rather than the places it was used.
//! Asked of git, which is the same question [`crate::app::opening`] asks
//! of a path on the command line.
//!
//! The path is kept as it was named -- less the `\\?\` Windows puts on a
//! resolved one, which names the same place -- and that is not the key: two
//! worktrees of one repository are one project to everything Obelus
//! *keeps* (see `obelus_git::project`) and two different places to work to
//! the reader standing in one of them. This list is about where to work,
//! so it holds both.
//!
//! **A project whose directory has gone is forgotten.** Asked when the
//! screen opens, which is the moment a view reads what it shows: twenty
//! `stat`s, not twenty reads, and a row offering a place that is not
//! there is a row whose one answer is a refusal. And taken out of the file
//! as well as off the screen, which is a choice with a cost said out loud
//! -- a disk that is not plugged in, or a share not mounted yet at the
//! moment a launcher starts Obelus, looks exactly like a project that was
//! deleted, and its row goes with it. A reader who opens it again has it
//! back, because opening it is how anything gets onto the list.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_component::{
    chooser::Chooser,
    picker::{Picker, PickerItem, PickerLayout, PickerValue},
};

/// How many are kept.
///
/// A limit on the list is a limit on what can be found in it, so this is
/// as many as a reader could plausibly recognise rather than as many as
/// fit on the screen -- the filter is what reaches the ones below the
/// fold. Past twenty, a row is a project the reader has forgotten having
/// opened, and the way back to one of those is to type its path.
const KEPT: usize = 20;

/// Where the list lives.
///
/// Obelus's own state rather than the reader's preferences, so it is in
/// the state directory and not in the settings file: `obelus-config` holds
/// what a reader chose and would be upset to lose, and this is a note
/// Obelus made about itself. The settings file is also the one a reader
/// edits by hand, and a list that rewrites itself under them is a poor
/// neighbour for the lines they wrote.
///
/// `None` on a system with nowhere of its own to keep state, which is the
/// same answer the conversations and the notes give: there is nothing to
/// read and nothing will be written either.
#[must_use]
pub(in crate::app) fn path() -> Option<PathBuf> {
    Some(obelus_logging::state_directory()?.join("projects.toml"))
}

/// One project, and when it was last opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct Project {
    /// Where it is, as it was named.
    pub(in crate::app) path: PathBuf,
    /// Seconds since the epoch, for the order and for the words on the
    /// row.
    ///
    /// `None` for a row written before Obelus wrote this down, which sorts
    /// last -- the same answer the conversations give a row with no time
    /// on it.
    pub(in crate::app) last: Option<i64>,
}

/// What reading the list found.
///
/// Three answers and not two, for the reason the conversations' table
/// gives three: a file that will not read is not a file with nothing in
/// it, and "nothing in it" is what [`remember`] would write back over it.
/// A reader whose list is briefly unreadable gets an empty list of
/// projects and can type a path; one whose list is *replaced* by an empty
/// one has lost every project they had.
#[derive(Debug)]
pub(in crate::app) enum Reading {
    /// There is none yet, which is where every machine starts. Also where
    /// there is nowhere to keep one, which comes to the same thing.
    Nothing,
    /// Here they are, newest first.
    Projects(Vec<Project>),
    /// There is one and it would not read, with what went wrong.
    Unreadable(String),
}

impl Reading {
    /// What is remembered, where remembering nothing is an answer the
    /// caller can live with.
    ///
    /// `None` for a file that would not read, which is the answer
    /// [`remember`] needs and the screen that asks does not: a screen that
    /// drew no rows because of a parse error would say the reader has
    /// never opened anything, and the one row that is always there is the
    /// way out of that.
    #[must_use]
    pub(in crate::app) fn projects(self) -> Option<Vec<Project>> {
        match self {
            Self::Nothing => Some(Vec::new()),
            Self::Projects(projects) => Some(projects),
            Self::Unreadable(_) => None,
        }
    }

    /// The rows to draw, which is none where the file would not read.
    #[must_use]
    pub(in crate::app) fn rows(self) -> Vec<Project> {
        match self {
            Self::Nothing | Self::Unreadable(_) => Vec::new(),
            Self::Projects(projects) => projects,
        }
    }
}

/// Reads the list, newest first.
#[must_use]
pub(in crate::app) fn read() -> Reading {
    let Some(path) = path() else {
        return Reading::Nothing;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Not there is not the same as will not read, and only the second
        // is a reason to stop writing: every machine starts without this
        // file.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Reading::Nothing,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => return Reading::Unreadable(error.to_string()),
    };
    let mut projects: Vec<Project> = table
        .get("opened")
        .and_then(toml::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let row = row.as_table()?;
                    Some(Project {
                        // A row with no path is not a project, however
                        // much else it carries: do not delete what you do
                        // not recognise, but do not draw it either.
                        path: PathBuf::from(row.get("path")?.as_str()?),
                        last: row.get("last").and_then(toml::Value::as_integer),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Newest first, settled here rather than trusted from the file: the
    // file is written by several Obelus processes and the order one of
    // them left is not a fact about the others.
    projects.sort_by_key(|project| std::cmp::Reverse(project.last));
    Reading::Projects(projects)
}

/// A path the way a reader writes it, which is the one way a row is
/// written.
///
/// On Windows a path that has been resolved begins `\\?\`, and that is the
/// same place as the path without it: a project reached once from where
/// Obelus was started and once from the list of worktrees -- whose main
/// checkout is resolved -- was two rows on the list. Taken off on the way
/// in, where what is left still names the place, which `dunce` judges;
/// elsewhere a path is as it came. Not on the way out: what is in the file
/// is what was written, and putting a spelling right is the writer's.
fn spelled(path: &Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
}

/// Puts this project at the top of the list, where it is one.
///
/// Read-modify-write rather than holding a copy, for the reason the
/// conversations are: several Obelus on one machine is the normal case,
/// and the last one to write would otherwise put the list back as it was
/// when it started.
///
/// What it does not do is take a lock, and that is a choice rather than
/// an oversight: two of these within the same fraction of a millisecond
/// both read the list before either writes it, and what the second writes
/// has no row for what the first remembered. That costs one project
/// missing until it is next opened, which opening puts right -- and a lock
/// is whole-file on Windows, which is what a claim's cost last time.
///
/// Answers whether anything was written, which is `false` for a directory
/// that is not a worktree and for a list that would not read -- neither is
/// a failure worth telling the reader about, and the caller uses it only
/// to know whether to look again.
pub(in crate::app) fn remember(root: &Path, now: i64) -> bool {
    // Not a worktree is not a project, and this is the one place that
    // judgement is made: `work_in` is told about directories that are
    // somewhere Obelus was started as well as ones a reader chose.
    if obelus_git::worktree(root).is_none() {
        return false;
    }
    let Some(path) = path() else {
        return false;
    };
    // Nothing at all where the file will not read, which is the whole
    // reason `Reading` has three answers: going on would write what Obelus
    // can make of a file it cannot read over the file itself.
    let Some(mut projects) = read().projects() else {
        tracing::warn!(path = %path.display(), "will not read, so no project is remembered over it");
        return false;
    };
    // By the path as it was named rather than by `obelus_git::project`:
    // two worktrees of one repository are two places to work, and a reader
    // who has both open wants both rows. Compared as written, because a
    // path that differs only in how it was spelled is the same row and
    // the newer spelling is the one to keep.
    let root = spelled(root);
    projects.retain(|project| project.path != root);
    projects.insert(
        0,
        Project {
            path: root,
            last: Some(now),
        },
    );
    projects.truncate(KEPT);
    write(&path, &projects)
}

/// Whether a remembered project has gone: nothing is there, or something
/// other than a directory is standing where it was.
///
/// Not every way of failing to look, though. A directory that will not say
/// what it is -- permission refused, a mount that has stopped answering, a
/// disk that errs -- is a directory that is there, and the cost the reader
/// accepted in forgetting is that a disk not plugged in looks deleted, not
/// that a refusal does. `is_dir` answers `false` for all of them alike,
/// which is why this asks for the error.
fn gone(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(data) => !data.is_dir(),
        Err(error) => matches!(
            error.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ),
    }
}

/// Takes these projects off the list, where their directories are still
/// not there.
///
/// Asked again at the moment of writing rather than trusted from whoever
/// looked: another Obelus may have just opened one of them -- a disk
/// plugged back in -- and the row it wrote is a project that is there.
/// Read-modify-write for the reason [`remember`] is.
///
/// Answers whether anything was written, which is `false` where nothing
/// on the list had gone and where the list would not read.
pub(in crate::app) fn forget(these: &[PathBuf]) -> bool {
    let Some(path) = path() else {
        return false;
    };
    let Some(mut projects) = read().projects() else {
        tracing::warn!(path = %path.display(), "will not read, so no project is forgotten from it");
        return false;
    };
    let before = projects.len();
    projects.retain(|project| !these.contains(&project.path) || !gone(&project.path));
    if projects.len() == before {
        return false;
    }
    write(&path, &projects)
}

/// Writes the list, whole.
fn write(path: &Path, projects: &[Project]) -> bool {
    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        tracing::warn!(%error, path = %path.display(), "nowhere to remember projects");
        return false;
    }
    // Beside it and a rename, the way the conversations, the notes and the
    // settings are written: another Obelus writing this at the same moment
    // leaves one whole file or the other, never half of either.
    // This process's own name beside it and not one every Obelus shares:
    // two writing at once into one shared name truncate each other's
    // half-written file, and the first rename takes the other's away.
    let beside = path.with_extension(format!("toml.writing.{}", std::process::id()));
    if let Err(error) =
        std::fs::write(&beside, to_toml(projects)).and_then(|()| std::fs::rename(&beside, path))
    {
        tracing::warn!(%error, path = %path.display(), "the list of projects was not written");
        // And not left behind: the name is this process's own, so nobody
        // else will ever write over it, and a failed rename would leave one
        // more of them for every Obelus that failed.
        let _ = std::fs::remove_file(&beside);
        return false;
    }
    true
}

/// What the file holds, as text.
///
/// Written by hand rather than through `toml`'s serialiser, the way the
/// conversations' table is: the file is four lines a row and a reader who
/// opens it should find it readable.
#[must_use]
fn to_toml(projects: &[Project]) -> String {
    let mut out = String::new();
    for project in projects {
        out.push_str("[[opened]]\n");
        out.push_str(&format!(
            "path = {}\n",
            toml::Value::String(project.path.to_string_lossy().into_owned())
        ));
        if let Some(last) = project.last {
            out.push_str(&format!("last = {last}\n"));
        }
        out.push('\n');
    }
    out
}

impl crate::app::App {
    /// Asks which project to work in, which nothing else has answered.
    ///
    /// Called from `startup`, from the page saying the project has gone
    /// (`App::the_tree_has_gone`), and from `close-project` -- the moments
    /// a window is back where a start with nothing to go on began, because
    /// everything the project was has gone with it. A reader at a terminal
    /// could leave and open another Obelus, but one started from a desktop
    /// menu has no shell to do that from, and leaving would be closing the
    /// window to get the page it opened on.
    pub(crate) fn ask_which_project(&mut self) {
        let reading = read();
        // What Obelus could not make of its own file goes where everything
        // else that went wrong on the way up goes -- the list put up over
        // the page that asks -- rather than into a log nobody is going to
        // open. The list is empty behind it, which is honest: there is one
        // row either way and it is the one that opens a project.
        if let Reading::Unreadable(why) = &reading {
            // `amiss` is where what went wrong with no line to mark
            // goes, and this has none: the file is Obelus's own and the
            // reader never opens it.
            self.amiss(&format!("The list of projects would not read: {why}"));
        }
        let (here, went): (Vec<_>, Vec<_>) = reading
            .rows()
            .into_iter()
            .partition(|project| !gone(&project.path));
        if !went.is_empty() {
            let went: Vec<PathBuf> = went.into_iter().map(|project| project.path).collect();
            forget(&went);
        }
        let known = here
            .into_iter()
            .map(|project| obelus_component::chooser::Known {
                shown: obelus_ui::with_home_as_tilde(&project.path),
                path: project.path,
                last: project.last,
            })
            .collect();
        self.chooser = Some(obelus_component::chooser::Chooser::new(known));
    }

    /// Lets go of this project and asks which one next.
    ///
    /// Asking first where something is unwritten, the way going to another
    /// worktree does, and writing the notes without asking, as on the way
    /// out.
    pub(in crate::app) fn close_the_project(&mut self) {
        self.write_the_notes();
        let unsaved = self
            .documents
            .iter()
            .flatten()
            .filter_map(crate::app::Document::file)
            .filter(|buffer| buffer.is_dirty())
            .count();
        if unsaved > 0 {
            self.ask_before_closing_the_project(unsaved);
            return;
        }
        // And a program still running in a terminal, which closing stops:
        // asked for the reason leaving asks.
        let running = self.terminals_running();
        if running > 0 {
            self.ask_before_stopping_them(
                running,
                "close the project",
                obelus_component::question::Answer::ClosingTheProject(
                    obelus_component::question::Leaving::Discard,
                ),
            );
            return;
        }
        self.leave_the_project();
    }

    /// The project let go of, once nothing unwritten stands in the way.
    ///
    /// What was open is written down first, so that choosing the project
    /// again opens it again: closing it is leaving, as far as the tree is
    /// concerned.
    pub(in crate::app) fn leave_the_project(&mut self) {
        tracing::info!(tree = %self.working_directory.display(), "closing the project");
        // Asked before the letting go, which takes the chat with it -- and
        // connected again once there is a project, as going to another
        // worktree does (`App::move_to_tree`): the reader who reached this
        // window from the chat did not say "for this project".
        let reached = self.has_the_remote();
        self.write_down_what_is_open_on_leaving();
        self.let_go_of_the_project();
        self.remote_at_start = reached;
        // Kept by `let_go_of_the_project`, which is the process's door, and
        // with it the claim on the tree -- which a window asking which
        // project no longer has.
        self.worktrees.left_the_tree();
        self.ask_which_project();
    }

    /// Asks, with these projects, whatever is on this machine.
    ///
    /// Pressing nothing runs no `npm` here, but reading the real list
    /// would make a test's screen depend on which projects whoever ran it
    /// has opened -- the same trap the welcome screen's own fixture fell
    /// into with the working directory.
    pub fn ask_about_these_projects_for_test(
        &mut self,
        known: Vec<obelus_component::chooser::Known>,
    ) {
        self.chooser = Some(obelus_component::chooser::Chooser::new(known));
    }

    /// A key, while the reader is being asked which project.
    ///
    /// Answers whether it was taken. What is not taken falls to the
    /// ordinary lookup, which finds this a dialog and so offers only what
    /// `Context::Dialog` binds -- leaving, and the four keys that act on
    /// what the reader has hold of. Everything else in Obelus is about a
    /// project and `Requires::AProject` refuses it.
    pub(in crate::app) fn choosing_a_project(&mut self, key: &KeyEvent) -> bool {
        // The list in front of the box gets the key first, and keeps only
        // the ones that move about it or choose from it.
        if self.naming_list_key(key) {
            // Through the same door the box's own keys go out of: that
            // key may have put a candidate *in* the box, and leaving
            // without this left the row's ink describing what was in it
            // before.
            self.note_what_the_box_names();
            return true;
        }
        // What a page key moves by, which is what the page has room for.
        let rows = self.chooser_rows();
        let Some(chooser) = &mut self.chooser else {
            return false;
        };
        let before = chooser.named();
        let outcome = chooser.handle(*key, rows);
        self.the_chooser_answered(outcome, before)
    }

    /// Text pasted while the reader is being asked which project -- or a
    /// word an input method committed, which arrives the same way.
    pub(in crate::app) fn paste_into_the_chooser(&mut self, what: &str) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let before = chooser.named();
        let outcome = chooser.paste(what);
        self.the_chooser_answered(outcome, before);
    }

    /// A copy out of whichever box the page is showing.
    pub(in crate::app) fn copy_from_the_chooser(&mut self) {
        let Some(chooser) = &self.chooser else {
            return;
        };
        let (text, what) = chooser.copied();
        self.copied(&text, what);
    }

    /// And a cut, which moves the box the way a paste does.
    pub(in crate::app) fn cut_from_the_chooser(&mut self) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        let before = chooser.named();
        let ((text, what), outcome) = chooser.cut();
        self.cut_away(&text, what);
        self.the_chooser_answered(outcome, before);
    }

    /// What a box on the page that asks which project moving means outside
    /// it, whether a key, a paste or a cut moved it.
    fn the_chooser_answered(
        &mut self,
        outcome: obelus_component::chooser::Outcome,
        before: Option<PathBuf>,
    ) -> bool {
        let taken = match outcome {
            obelus_component::chooser::Outcome::Taken => true,
            obelus_component::chooser::Outcome::Ignored => false,
            obelus_component::chooser::Outcome::Wants(directory) => {
                self.look_in(&directory);
                true
            }
            obelus_component::chooser::Outcome::Chose(path) => {
                self.settle_on(&path);
                return true;
            }
        };
        // A box that moved is a new question, so a list the reader shut
        // comes back: what they said no to was what was in it then. The
        // same rule `component::completion` follows -- escape takes the
        // panel away and the next letter asks again.
        if self.chooser.as_ref().and_then(Chooser::named) != before {
            self.naming_shut = false;
        }
        self.note_what_the_box_names();
        // And what is typed narrows whatever the last read found, which
        // is not a question for the disk.
        self.settle_the_naming_list();
        taken
    }

    /// Looks at whether what is in the path box is there at all.
    ///
    /// Worked out on the keys that can move the box and kept, because
    /// the row is drawn on every frame and a `stat` per frame is a file
    /// read wearing a costume. One place, because there are two such
    /// keys -- what the reader types, and what the list puts in -- and
    /// the second one forgot.
    fn note_what_the_box_names(&mut self) {
        self.named_is_there = self
            .chooser
            .as_ref()
            .and_then(Chooser::named)
            .is_some_and(|path| path.exists());
    }

    /// Reads what a directory holds and makes a list of it.
    ///
    /// On the main thread, and that is a measurement rather than an
    /// oversight: one `read_dir` of one directory is what a shell does
    /// between two presses of a key. It happens once per *directory* and
    /// not once per letter -- the chooser asks only when the part before
    /// the last separator has moved -- so a reader typing a long path
    /// pays for the directories they pass through.
    ///
    /// The list itself is the ordinary compact one, which is the
    /// arrangement the agent's own commands already use: the rows, the
    /// chosen row and the marking of what matched are the picker's, and
    /// the box below owns the keys.
    ///
    /// Directories first and then files, each by name: what is being
    /// named is usually a project, and a list with the directories
    /// scattered through it reads as a list of files.
    fn look_in(&mut self, directory: &Path) {
        let mut found: Vec<(bool, PathBuf)> = std::fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            // Settled here, where the disk is being asked anyway: a row
            // is laid out on every frame, and asking again there would be
            // a `stat` per candidate per keystroke for an answer already
            // in hand.
            .map(|entry| (entry.path().is_dir(), entry.path()))
            .collect();
        found.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let items = found
            .into_iter()
            .map(|(directory, path)| {
                let name = path.file_name().map_or_else(
                    || path.to_string_lossy().into_owned(),
                    |name| name.to_string_lossy().into_owned(),
                );
                PickerItem {
                    prose: false,
                    marker: None,
                    icon: None,
                    // The name alone, and a separator where it holds other
                    // things: the directory they are all in is in the box
                    // above them, and repeating it down the list spends
                    // the width on the one part of every row that is the
                    // same.
                    label: match directory {
                        true => format!("{name}{}", std::path::MAIN_SEPARATOR),
                        false => name,
                    },
                    version: None,
                    detail: None,
                    trailing: None,
                    changed: None,
                    value: match directory {
                        true => PickerValue::Directory(path),
                        false => PickerValue::File(path),
                    },
                    enabled: true,
                    colours: None,
                    status: None,
                    depth: 0,
                    opens: None,
                    kind: None,
                    tab: None,
                    section: None,
                }
            })
            .collect::<Vec<_>>();
        // Kept, rather than turned straight into a list: the list goes
        // whenever the reader shuts it or types past what it holds, and
        // both have to be undoable without asking the disk again.
        self.naming_read = match items.is_empty() {
            true => None,
            false => Some((directory.to_path_buf(), items)),
        };
        // And the list goes with the directory it was made from. Kept,
        // it would be narrowed by letters belonging to a name in a
        // different place -- which is what it did: typing the separator
        // that walks into a directory left the one above it on screen.
        self.naming_list = None;
        self.settle_the_naming_list();
    }

    /// Narrows that list by what has been typed since the separator, and
    /// gives it the geometry it is about to be drawn in.
    ///
    /// No disk is touched: the rows are whatever the last read found, and
    /// the letters after the separator only choose among them. Which is
    /// what keeps a reader typing a name from reading the directory once
    /// per letter.
    ///
    /// A list that matches nothing stops being a list. It has to: the
    /// reader is typing a path nobody offered, which is a path they are
    /// allowed to type, and a list that stayed would swallow the enter
    /// that opens it.
    pub(in crate::app) fn settle_the_naming_list(&mut self) {
        let Some(chooser) = self.chooser.as_ref().filter(|chooser| chooser.is_naming()) else {
            // Not naming a path at all, so what a directory held a
            // moment ago is nobody's: left here, going back into the box
            // would open on the last directory's names under an empty
            // one.
            self.naming_list = None;
            self.naming_read = None;
            self.naming_shut = false;
            return;
        };
        // Shut by the reader, on the box as it stands. Not forgotten --
        // the next letter is a new question and brings it back.
        if self.naming_shut {
            self.naming_list = None;
            return;
        }
        let segment = chooser.segment();
        let Some((read, items)) = &self.naming_read else {
            self.naming_list = None;
            return;
        };
        // What was read has to be what the box is still about. It is not
        // when the reader rubs their way back past a separator -- or
        // rubs out everything -- and offering it then is a list of
        // somewhere they have left.
        if chooser.directory_named().as_deref() != Some(read.as_path()) {
            self.naming_list = None;
            self.naming_read = None;
            return;
        }
        // Made again from what the directory read found rather than kept
        // across keys: it costs one build of a list of names and it is
        // what lets a list that matched nothing come back when the
        // letter that emptied it is rubbed out.
        let mut list = match self.naming_list.take() {
            Some(list) => list,
            None => {
                let mut made = Picker::new(
                    items.clone(),
                    PickerLayout::Compact {
                        rows: crate::app::COMPACT_ROWS,
                    },
                );
                made.before_typing("Narrow what is here");
                made
            }
        };
        if list.query() != segment {
            list.set_query(&segment);
        }
        // A list of nothing is not a list, and it has to stop being one:
        // the reader is typing a path nobody offered, which they are
        // allowed to do, and a list that stayed would swallow the enter
        // that opens it.
        if list.match_count() == 0 {
            self.naming_list = None;
            return;
        }
        let room = self.editor_area;
        let rows = obelus_ui::picker::rows_drawn(&list, room);
        list.refresh_indices(rows, room.width);
        self.naming_list = Some(list);
    }

    /// Whatever a key means to that list, if it means anything.
    ///
    /// Only the keys that move about a list and the ones that choose from
    /// it -- the rule the agent's commands settled. Every character and
    /// every other key belongs to the box, which is what makes this a
    /// list of what is being typed rather than a mode the reader is in.
    pub(in crate::app) fn naming_list_key(&mut self, key: &KeyEvent) -> bool {
        if self.naming_list.is_none() {
            return false;
        }
        if obelus_keymap::modifiers_of(key) != Some(KeyModifiers::NONE) {
            return false;
        }
        match key.code {
            // The window is not moved here: the frame that follows
            // settles it, which is the one place that knows how many rows
            // are on screen.
            KeyCode::Up | KeyCode::Down => {
                let by = match key.code {
                    KeyCode::Up => -1,
                    _ => 1,
                };
                if let Some(list) = self.naming_list.as_mut() {
                    list.move_selection_by(by);
                }
                true
            }
            // Enter puts the chosen row *in the box*, the way enter takes
            // what is selected in every other completion in Obelus. What
            // opens the project is enter with no list in front of it,
            // which is what escape below leaves behind.
            KeyCode::Enter => {
                let chosen = self
                    .naming_list
                    .as_ref()
                    .and_then(Picker::selected_item)
                    .map(|item| item.value.clone());
                let put = match chosen {
                    Some(PickerValue::Directory(path)) => Some((path, true)),
                    Some(PickerValue::File(path)) => Some((path, false)),
                    _ => None,
                };
                if let Some((path, directory)) = put {
                    self.naming_list = None;
                    let outcome = self
                        .chooser
                        .as_mut()
                        .map(|chooser| chooser.put(&path, directory));
                    // A directory asks for what is inside it at once, so
                    // walking down a tree is one key a level.
                    if let Some(obelus_component::chooser::Outcome::Wants(next)) = outcome {
                        self.look_in(&next);
                    }
                }
                true
            }
            // The list, not the box: escape gives up on the nearest
            // thing first, and what the reader typed stays.
            //
            // Nothing is written down about having shut it, which is
            // where this differs from the agent's commands: that list is
            // rebuilt from the box on every frame, so closing it needs a
            // flag or the next frame puts it back. This one is built
            // only when the *directory* moves -- `settle_the_naming_list`
            // narrows what a read already found and never conjures a
            // list -- so shutting it is enough, and it comes back when
            // the reader types their way into somewhere else, which is
            // a new question.
            KeyCode::Esc => {
                self.naming_shut = true;
                self.naming_list = None;
                true
            }
            _ => false,
        }
    }

    /// Takes the reader's answer and puts Obelus on it.
    ///
    /// A file and a directory are the two answers, and which is which is
    /// not this function's to decide: [`crate::app::opening`] already says
    /// what a path on the command line means -- a directory is the project
    /// and the question it leaves is which file, a file names the tree it
    /// is in and is opened. A reader typing a path here means exactly what
    /// a reader typing one after `ob` means, and two answers to that would
    /// be two ways to open the same thing.
    fn settle_on(&mut self, path: &Path) {
        // A path that is not there is not a project, and this is the one
        // place that has to say so. `opening` answers what a path on the
        // command line *means*, and for one that is not there it means
        // "a file to make, in the directory above it" -- which is right
        // for `ob notes.md` and catastrophic here: a reader who typed a
        // name with a letter wrong would be put on the directory the
        // process happened to begin in, which from a desktop menu is the
        // home directory. That is the one answer this whole screen
        // exists to avoid.
        //
        // One `stat`, on a key the reader pressed. Which is not what the
        // ink on the row is worked out from -- see `named_is_there`.
        if !path.exists() {
            // Said in the ink before they pressed, so nothing is said
            // here. A row of the remembered list whose directory has
            // gone is dropped instead: it is not there to be offered any
            // more, and a row that vanishes under the reader is the
            // answer.
            //
            // And off the file, for the reason the screen forgets one
            // when it opens: a path typed into the box is not on the list,
            // and taking it off that is nothing written.
            if let Some(chooser) = &mut self.chooser {
                chooser.forget(path);
            }
            forget(std::slice::from_ref(&path.to_path_buf()));
            return;
        }
        let opening = crate::app::opening(std::slice::from_ref(&path.to_path_buf()));
        let Some(root) = opening.root else {
            return;
        };
        // The chooser goes first, so that `work_in` and everything it
        // starts runs with a project settled rather than with the screen
        // still saying there is none.
        self.chooser = None;
        self.naming_list = None;
        self.naming_read = None;
        self.naming_shut = false;
        self.settle(root, &opening.files);
        // A directory is a reader saying which project and asking which
        // file, which is the list -- the same thing `ob some-directory`
        // does, said in the same place.
        if opening.list {
            self.open_file_picker();
        }
    }

    /// Puts a window with no project on `root`, opening `files` in it.
    ///
    /// The half of answering which project that is not about the page
    /// asking it: going to another worktree is the same settling, on a
    /// window that has just let go of the tree it was on.
    pub(in crate::app) fn settle(&mut self, root: PathBuf, files: &[PathBuf]) {
        self.work_in(root);
        self.apply_project();
        // Everything `App::start` holds back while there is no project.
        // Held back rather than taken and re-pointed, because one of
        // them is a server that is already listening: moving it would
        // mean stopping one Obelus had told nothing about yet, and the
        // simpler half of that is not to start it.
        //
        // The watches are the half that cost most: with git's `HEAD` and
        // `index` unwatched, the branch on the status row and the marks
        // in the margin would be whatever they were when the project
        // opened, for the rest of the session.
        self.offer_the_tools();
        self.watch_the_project();
        self.say_where_this_window_is();
        if self.remote_at_start {
            self.connect_remote_at_start(false);
        }
        for file in files {
            self.open(file);
        }
        self.reopen_what_was_open();
    }
}

impl crate::app::App {
    /// How many projects the page has room for.
    ///
    /// Asked of the same function the page lays itself out with, so the
    /// keys that page the list and the window over it are about the rows
    /// the reader can see -- see `App::chat_key` for what two answers to
    /// that cost.
    pub(in crate::app) fn chooser_rows(&self) -> u16 {
        self.what_is_being_chosen().map_or(1, |choosing| {
            obelus_ui::projects::list_height(self.drawn_in(), &choosing, &self.keymap)
        })
    }

    /// What is being asked, where Obelus is asking which project.
    ///
    /// The words are settled here and the room for them is not: a path is
    /// shortened with `~` because which directory is the reader's own is
    /// a fact about this machine, and how long ago is said in the words
    /// `obelus_git::how_long_ago` already says it in, so a row on this
    /// screen and a row of the history do not disagree about what two
    /// hours ago is called. What is cut to fit is the drawing's, and it
    /// needs a width this does not have.
    pub(in crate::app) fn what_is_being_chosen(&self) -> Option<obelus_ui::Choosing> {
        let chooser = self.chooser.as_ref()?;
        let now = std::time::SystemTime::now();
        Some(obelus_ui::Choosing {
            known: chooser
                .rows()
                .into_iter()
                .map(|(known, matched)| obelus_ui::Opened {
                    path: known.shown.clone(),
                    matched,
                    when: known
                        .last
                        .map(|last| obelus_git::how_long_ago(last, now))
                        .unwrap_or_default(),
                })
                .collect(),
            at: chooser.at(),
            top: chooser.top(),
            typed: chooser.typing().said(),
            caret: chooser.typing().caret().get(),
            held: chooser.typing().held(),
            naming: chooser.is_naming(),
            there: self.named_is_there,
            offering: self.naming_list.is_some(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests here take turns.
    ///
    /// Unlike the conversations and the claims, which key a file per
    /// project and so never meet, this list is *one* file for the whole
    /// machine -- and `state_directory_for_test` is a `OnceLock`, so every
    /// test in this binary shares the directory it is in. Run at once they
    /// read each other's writes, and the one that leaves an unreadable file
    /// behind makes every other one fail for a reason that has nothing to
    /// do with what it is testing. That was not hypothetical: it is how
    /// these five first went red.
    static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Somewhere of this run's own to work in, and an empty list.
    ///
    /// Hands back the turn as well as the directory, so a test holds it
    /// for as long as it is running: dropped at the end of the test, with
    /// nothing to remember to do.
    fn scratch(name: &str) -> (PathBuf, std::sync::MutexGuard<'static, ()>) {
        // Taken even where the last holder panicked. A poisoned lock here
        // means some other test failed, and answering that by failing all
        // the rest hides the one that matters.
        let turn = TURNS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let root =
            std::env::temp_dir().join(format!("obelus-projects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-projects-state-{}", std::process::id())),
        );
        let _ = std::fs::remove_file(path().expect("somewhere"));
        (root, turn)
    }

    /// Makes `root` a worktree, since nothing else is remembered.
    fn a_repository(root: &Path) {
        gix::init(root).expect("a repository");
    }

    /// What Obelus writes, Obelus has to be able to read.
    ///
    /// The one test a format with a writer and a reader in the same
    /// program has to have, or the two drift and the symptom turns up
    /// somewhere that looks unrelated.
    ///
    /// Broken deliberately by writing `paths = ` instead of `path = ` in
    /// `to_toml`: the row comes back as no row at all.
    #[test]
    fn what_is_written_reads_back() {
        let (root, _turn) = scratch("round-trip");
        a_repository(&root);

        assert!(remember(&root, 1000), "nothing was written");
        let projects = read().rows();
        assert_eq!(projects.len(), 1, "the row does not come back");
        assert_eq!(projects[0].path, root, "the path does not come back");
        assert_eq!(projects[0].last, Some(1000), "the time does not come back");
    }

    /// The newest is first, whatever order the file was left in.
    ///
    /// Asked of a file written out of order on purpose, because `remember`
    /// puts its own row at the front and so leaves a file that is already
    /// sorted: a test built out of `remember` calls passes with no sort at
    /// all, which is how this one first went green while testing nothing.
    /// The order has to be settled on reading, because the file is written
    /// by several Obelus and the order one of them left is not a fact
    /// about the others.
    ///
    /// A row with no time sorts last, which is what a row written before
    /// Obelus kept one looks like.
    ///
    /// Broken deliberately by taking the `sort_by` out of `read`: the rows
    /// come back in the order the file happens to hold them.
    #[test]
    fn the_newest_is_first() {
        let (_root, _turn) = scratch("order");
        let path = path().expect("somewhere");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(
            &path,
            "[[opened]]\npath = \"/one\"\nlast = 10\n\n\
             [[opened]]\npath = \"/two\"\n\n\
             [[opened]]\npath = \"/three\"\nlast = 30\n\n\
             [[opened]]\npath = \"/four\"\nlast = 20\n",
        )
        .expect("a file another Obelus left");

        let projects = read().rows();
        assert_eq!(
            projects
                .iter()
                .map(|project| project.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["/three", "/four", "/one", "/two"],
            "the newest is not first, or the row with no time is not last"
        );
    }

    /// One project is one row, however many times it is opened.
    ///
    /// Broken deliberately by taking the `retain` out of `remember`: the
    /// list fills with the same project and the twentieth other one falls
    /// off the end.
    #[test]
    fn opening_the_same_project_again_moves_it_rather_than_adding_it() {
        let (root, _turn) = scratch("again");
        a_repository(&root);

        assert!(remember(&root, 10), "nothing was written");
        assert!(remember(&root, 20), "nothing was written");

        let projects = read().rows();
        assert_eq!(projects.len(), 1, "the project is in the list twice");
        assert_eq!(projects[0].last, Some(20), "the time did not move");
    }

    /// A project reached resolved and reached as a reader names it is one
    /// row, and the file holds it the way a reader names it.
    ///
    /// On Windows only, which is the one place a resolved path is spelled
    /// differently: it begins `\\?\`. The plain spelling is worked out from
    /// the resolved one rather than taken from the scratch directory, which
    /// on a machine with short names is `RUNNER~1` and resolves to another
    /// word altogether.
    ///
    /// Broken deliberately by taking `spelled` out of `remember`: the file
    /// holds both spellings.
    #[cfg(windows)]
    #[test]
    fn a_project_reached_resolved_is_one_row_spelled_plainly() {
        let (root, _turn) = scratch("resolved");
        a_repository(&root);
        let resolved = root.canonicalize().expect("the directory");
        assert!(resolved.to_string_lossy().starts_with(r"\\?\"));
        let plain = dunce::simplified(&resolved).to_path_buf();

        assert!(remember(&plain, 10), "nothing was written");
        assert!(remember(&resolved, 20), "nothing was written");

        let written = std::fs::read_to_string(path().expect("somewhere")).expect("the list");
        assert_eq!(
            written.matches("[[opened]]").count(),
            1,
            "the project is in the file twice: {written}"
        );
        assert!(!written.contains(r"\\?\"), "kept resolved: {written}");
        assert_eq!(read().rows()[0].path, plain);
    }

    /// Somewhere that is not a worktree is not somewhere to come back to.
    ///
    /// Which is the whole reason this is not written at startup from
    /// whatever directory the process began in: a desktop launcher starts
    /// Obelus in the home directory, and that would be the only row in
    /// every reader's list.
    ///
    /// Broken deliberately by taking the `worktree` check out of
    /// `remember`: the directory is written down and the list offers a
    /// place nobody works in.
    #[test]
    fn a_directory_that_is_not_a_worktree_is_not_remembered() {
        let (root, _turn) = scratch("not-a-project");

        assert!(!remember(&root, 10), "it was written down");
        assert!(read().rows().is_empty(), "it is in the list");
    }

    /// A file that will not read is not a file with nothing in it.
    ///
    /// The rule the conversations' table and the notes both follow: what
    /// is on disk is left exactly as it is, because the alternative is
    /// every project a reader had traded for a parse error.
    ///
    /// Broken deliberately by having `remember` use `read().rows()`
    /// instead of `read().projects()`: the unreadable file is written over
    /// with one row and everything in it is gone.
    #[test]
    fn a_list_that_will_not_read_is_not_written_over() {
        let (root, _turn) = scratch("unreadable");
        a_repository(&root);
        let path = path().expect("somewhere");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(&path, "[[opened]]\npath = \"half of a").expect("half a file");

        assert!(!remember(&root, 10), "it was written over");
        assert_eq!(
            std::fs::read_to_string(&path).expect("still there"),
            "[[opened]]\npath = \"half of a",
            "the file was changed"
        );
        assert!(
            matches!(read(), Reading::Unreadable(_)),
            "it does not read as unreadable"
        );
    }

    /// Two worktrees of one repository are two places to work.
    ///
    /// The opposite answer from everything Obelus *keeps* about a project,
    /// which is keyed by `obelus_git::project` so that two worktrees
    /// share it. This list is about where to open, and a reader with a
    /// worktree per branch wants both rows.
    ///
    /// Broken deliberately by keying the `retain` on
    /// `obelus_git::project` instead of on the path: the second worktree
    /// takes the first one's row and the reader can only reach one of
    /// them.
    #[test]
    fn two_worktrees_of_one_repository_are_two_rows() {
        let (root, _turn) = scratch("worktrees");
        let main = root.join("main");
        std::fs::create_dir_all(&main).expect("a directory");
        a_repository(&main);
        // A commit, because a worktree cannot be added to a repository
        // with no history.
        let linked = root.join("linked");
        let ok = std::process::Command::new("git")
            .args(["-C"])
            .arg(&main)
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=T",
                "commit",
                "--allow-empty",
                "-m",
                "first",
            ])
            .output()
            .is_ok_and(|out| out.status.success())
            && std::process::Command::new("git")
                .args(["-C"])
                .arg(&main)
                .arg("worktree")
                .arg("add")
                .arg(&linked)
                .arg("-b")
                .arg("other")
                .output()
                .is_ok_and(|out| out.status.success());
        assert!(ok, "the worktree was not made");

        assert!(remember(&main, 10), "nothing was written");
        assert!(remember(&linked, 20), "nothing was written");

        let projects = read().rows();
        assert_eq!(projects.len(), 2, "the two worktrees are one row");
    }

    /// A list holding `root`, which is there, and a project that is not.
    fn one_here_and_one_gone(root: &Path) -> PathBuf {
        let gone = root.join("gone");
        let path = path().expect("somewhere");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(
            &path,
            to_toml(&[
                Project {
                    path: root.to_path_buf(),
                    last: Some(20),
                },
                Project {
                    path: gone.clone(),
                    last: Some(10),
                },
            ]),
        )
        .expect("a list");
        gone
    }

    /// A project whose directory has gone comes off the file.
    ///
    /// Broken deliberately by having `forget`'s `retain` keep every row:
    /// the gone one is still in the list.
    #[test]
    fn a_project_that_has_gone_is_forgotten() {
        let (root, _turn) = scratch("forgotten");
        let gone = one_here_and_one_gone(&root);

        assert!(forget(&[gone]), "nothing was written");
        let projects = read().rows();
        assert_eq!(projects.len(), 1, "the gone project is still listed");
        assert_eq!(projects[0].path, root, "the wrong one was forgotten");
    }

    /// One that is there again by the time the file is written is kept:
    /// another Obelus may have just opened it, off a disk plugged back in.
    ///
    /// Broken deliberately by taking the `is_dir` out of `forget`'s
    /// `retain`: a project that is there is forgotten because somebody said
    /// it had gone.
    #[test]
    fn a_project_that_is_back_is_not_forgotten() {
        let (root, _turn) = scratch("back");
        let _gone = one_here_and_one_gone(&root);

        assert!(!forget(std::slice::from_ref(&root)), "it was written");
        assert_eq!(read().rows().len(), 2, "a project that is there went");
    }

    /// A file standing where a project was is not somewhere to work, and
    /// is forgotten the way nothing there is.
    ///
    /// Broken deliberately by having `gone` answer `false` for anything
    /// that is there, whatever it is: the file is kept as a project.
    #[test]
    fn a_file_where_a_project_was_is_forgotten() {
        let (root, _turn) = scratch("a-file");
        let gone = one_here_and_one_gone(&root);
        std::fs::write(&gone, "not a project").expect("a file");

        assert!(forget(&[gone]), "nothing was written");
        assert_eq!(read().rows().len(), 1, "a file is kept as a project");
    }

    /// A project behind a refusal is not a project that has gone.
    ///
    /// The directory above it will not be read, so asking after the project
    /// fails -- and fails as permission denied, not as nothing there. What
    /// the reader accepted losing is a disk that is not plugged in; a
    /// refusal is a directory that is there and is not telling.
    ///
    /// Broken deliberately by having `gone` answer `!path.is_dir()`, which
    /// is what it was: the project comes off the screen and out of the file.
    #[cfg(unix)]
    #[test]
    fn a_project_behind_a_refusal_is_not_forgotten() {
        use std::os::unix::fs::PermissionsExt as _;

        let (root, _turn) = scratch("refused");
        let locked = root.join("locked");
        let project = locked.join("project");
        std::fs::create_dir_all(&project).expect("the project");
        let path = path().expect("somewhere");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(
            &path,
            to_toml(&[Project {
                path: project.clone(),
                last: Some(10),
            }]),
        )
        .expect("a list");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("refusing");
        // Root reads through any mode, and then there is no refusal to
        // test: say so rather than pass for the wrong reason.
        let refused = std::fs::metadata(&project).is_err();

        let mut app = crate::app::App::new(Vec::new());
        app.ask_which_project();
        let offered = app.chooser.as_ref().expect("asking").rows().len();
        let kept = read().rows().len();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("giving it back");

        if !refused {
            eprintln!("skipped: this user reads through a directory with no permissions");
            return;
        }
        assert_eq!(offered, 1, "a project behind a refusal was not offered");
        assert_eq!(kept, 1, "a project behind a refusal was forgotten");
    }

    /// Forgetting writes nothing over a list that will not read, for the
    /// reason remembering does not.
    ///
    /// Two things keep it, and either is enough: the unreadable answer, and
    /// writing nothing where nothing came off -- a list that will not read
    /// has no rows to take any off. Broken deliberately by taking out both,
    /// having `forget` use `read().rows()` and write whatever it has: the
    /// unreadable file is written over with nothing. One at a time, it
    /// stays green.
    #[test]
    fn forgetting_does_not_write_over_a_list_that_will_not_read() {
        let (root, _turn) = scratch("forget-unreadable");
        let path = path().expect("somewhere");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(&path, "[[opened]]\npath = \"half of a").expect("half a file");

        assert!(!forget(&[root.join("gone")]), "it was written over");
        assert_eq!(
            std::fs::read_to_string(&path).expect("still there"),
            "[[opened]]\npath = \"half of a",
            "the file was changed"
        );
    }

    /// The screen that asks does not offer a project that has gone, and
    /// the file forgets it as the screen opens.
    ///
    /// Broken deliberately by having `ask_which_project` keep every row
    /// (the first assertion) and by taking its `forget` out (the second).
    #[test]
    fn asking_neither_offers_nor_keeps_a_project_that_has_gone() {
        let (root, _turn) = scratch("asking-gone");
        let _gone = one_here_and_one_gone(&root);

        let mut app = crate::app::App::new(Vec::new());
        app.ask_which_project();
        let offered: Vec<PathBuf> = app
            .chooser
            .as_ref()
            .expect("asking")
            .rows()
            .into_iter()
            .map(|(known, _)| known.path.clone())
            .collect();
        assert_eq!(offered, vec![root.clone()], "a gone project is offered");
        assert_eq!(
            read().rows().len(),
            1,
            "the gone project is still in the file"
        );
    }

    /// Another Obelus halfway through writing the list is left alone.
    ///
    /// Its file sits beside the list under the name every Obelus once
    /// wrote through, and writing through that name again truncates it and
    /// renames it away under the other's feet.
    ///
    /// Broken deliberately by writing beside the list as `toml.writing`
    /// again, with no process number: the other's file is gone.
    #[test]
    fn another_obelus_writing_the_list_is_left_alone() {
        let (root, _turn) = scratch("beside");
        a_repository(&root);
        let theirs = path().expect("somewhere").with_extension("toml.writing");
        std::fs::create_dir_all(theirs.parent().expect("a directory")).expect("the directory");
        std::fs::write(&theirs, "another Obelus is halfway through this").expect("theirs");

        assert!(remember(&root, 10), "nothing was written");
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("another Obelus is halfway through this"),
            "the other Obelus's half-written list was taken"
        );
    }

    /// A write that fails leaves nothing beside the list.
    ///
    /// The name it was written through is this process's own, so nobody
    /// will write over it later; left, there would be one per Obelus that
    /// ever failed. Asked of `write` and not of `remember`, because what
    /// refuses a rename here -- a directory where the list goes -- is what
    /// refuses the read before it.
    ///
    /// Broken deliberately by taking the `remove_file` out of `write`.
    #[test]
    fn a_write_that_fails_leaves_nothing_beside_the_list() {
        let (root, _turn) = scratch("failed");
        let target = root.join("projects.toml");
        std::fs::create_dir_all(target.join("in the way")).expect("a directory where it goes");

        assert!(!write(&target, &[]), "it was written");
        let beside: Vec<_> = std::fs::read_dir(&root)
            .expect("the directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "projects.toml")
            .collect();
        assert!(beside.is_empty(), "it left {beside:?} behind");
    }

    /// The list does not grow without end.
    ///
    /// Broken deliberately by taking the `truncate` out of `remember`:
    /// the file grows for ever and the screen's filter searches a
    /// list of every directory the reader has ever opened.
    #[test]
    fn the_list_stops_at_twenty() {
        let (root, _turn) = scratch("many");
        for which in 0..KEPT + 5 {
            let each = root.join(format!("p{which}"));
            std::fs::create_dir_all(&each).expect("a directory");
            a_repository(&each);
            assert!(
                remember(&each, i64::try_from(which).expect("a time")),
                "nothing was written"
            );
        }

        assert_eq!(read().rows().len(), KEPT, "the list is not capped");
    }
}
