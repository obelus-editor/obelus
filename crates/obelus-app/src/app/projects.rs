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
//! So the welcome screen asks, and this is what it offers: the projects
//! this reader has had open, newest first. The same shape the
//! conversations have and for the same reason -- what a reader did
//! outlives the window.
//!
//! **A project is remembered however it was named.** By an argument, by
//! the directory Obelus was started in, or by being chosen on the welcome
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
//! The path is kept as it was named, and that is not the key: two
//! worktrees of one repository are one project to everything Obelus
//! *keeps* (see `obelus_git::project`) and two different places to work to
//! the reader standing in one of them. This list is about where to work,
//! so it holds both.
//!
//! What is not here is anything about whether the directory is still
//! there. A list of twenty projects is twenty paths, and asking the
//! filesystem about each of them is twenty file reads on the way to a
//! screen nobody has pressed a key on yet. A row says what it was; the
//! answer to whether it still is comes from trying to open it.

use std::path::{Path, PathBuf};

use crossterm::event::KeyEvent;

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
pub(super) fn path() -> Option<PathBuf> {
    Some(obelus_logging::state_directory()?.join("projects.toml"))
}

/// One project, and when it was last opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Project {
    /// Where it is, as it was named.
    pub(super) path: PathBuf,
    /// Seconds since the epoch, for the order and for the words on the
    /// row.
    ///
    /// `None` for a row written before Obelus wrote this down, which sorts
    /// last -- the same answer the conversations give a row with no time
    /// on it.
    pub(super) last: Option<i64>,
}

/// What reading the list found.
///
/// Three answers and not two, for the reason the conversations' table
/// gives three: a file that will not read is not a file with nothing in
/// it, and "nothing in it" is what [`remember`] would write back over it.
/// A reader whose list is briefly unreadable gets an empty welcome screen
/// and can type a path; one whose list is *replaced* by an empty one has
/// lost every project they had.
#[derive(Debug)]
pub(super) enum Reading {
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
    /// [`remember`] needs and the welcome screen does not: a screen that
    /// drew no rows because of a parse error would say the reader has
    /// never opened anything, and the one row that is always there is the
    /// way out of that.
    #[must_use]
    pub(super) fn projects(self) -> Option<Vec<Project>> {
        match self {
            Self::Nothing => Some(Vec::new()),
            Self::Projects(projects) => Some(projects),
            Self::Unreadable(_) => None,
        }
    }

    /// The rows to draw, which is none where the file would not read.
    #[must_use]
    pub(super) fn rows(self) -> Vec<Project> {
        match self {
            Self::Nothing | Self::Unreadable(_) => Vec::new(),
            Self::Projects(projects) => projects,
        }
    }
}

/// Reads the list, newest first.
#[must_use]
pub(super) fn read() -> Reading {
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

/// Puts this project at the top of the list, where it is one.
///
/// Read-modify-write rather than holding a copy, for the reason the
/// conversations are: several Obelus on one machine is the normal case,
/// and the last one to write would otherwise put the list back as it was
/// when it started.
///
/// Answers whether anything was written, which is `false` for a directory
/// that is not a worktree and for a list that would not read -- neither is
/// a failure worth telling the reader about, and the caller uses it only
/// to know whether to look again.
pub(super) fn remember(root: &Path, now: i64) -> bool {
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
    projects.retain(|project| project.path != root);
    projects.insert(
        0,
        Project {
            path: root.to_path_buf(),
            last: Some(now),
        },
    );
    projects.truncate(KEPT);
    if let Some(directory) = path.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        tracing::warn!(%error, path = %path.display(), "nowhere to remember projects");
        return false;
    }
    // Beside it and a rename, the way the conversations, the notes and the
    // settings are written: another Obelus writing this at the same moment
    // leaves one whole file or the other, never half of either.
    let beside = path.with_extension("toml.writing");
    if let Err(error) =
        std::fs::write(&beside, to_toml(&projects)).and_then(|()| std::fs::rename(&beside, &path))
    {
        tracing::warn!(%error, path = %path.display(), "the project was not remembered");
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

impl super::App {
    /// Asks which project to work in, which nothing else has answered.
    ///
    /// Called from `startup` and nowhere else: being asked is a fact
    /// about how Obelus was started, not a state anything later can put
    /// the reader back into. A reader who wants another project opens
    /// another Obelus, which is how Obelus is used anyway.
    pub(crate) fn ask_which_project(&mut self) {
        let reading = read();
        // What Obelus could not make of its own file goes where everything
        // else that went wrong on the way up goes -- under the keys on the
        // welcome screen -- rather than into a log nobody is going to
        // open. The list is empty behind it, which is honest: there is one
        // row either way and it is the one that opens a project.
        if let Reading::Unreadable(why) = &reading {
            // `amiss` is where what went wrong with no line to mark
            // goes, and this has none: the file is Obelus's own and the
            // reader never opens it.
            self.amiss(&format!("The list of projects would not read: {why}"));
        }
        let known = reading
            .rows()
            .into_iter()
            .map(|project| obelus_component::chooser::Known {
                path: project.path,
                last: project.last,
            })
            .collect();
        self.chooser = Some(obelus_component::chooser::Chooser::new(known));
    }

    /// A key, while the reader is being asked which project.
    ///
    /// Answers whether it was taken. What is not taken falls to the
    /// ordinary lookup, which finds this a dialog and so offers only what
    /// `Context::Dialog` binds -- leaving, and the three keys that act on
    /// what the reader has hold of. Everything else in Obelus is about a
    /// project and `Requires::AProject` refuses it.
    pub(super) fn choosing_a_project(&mut self, key: &KeyEvent) -> bool {
        let Some(chooser) = &mut self.chooser else {
            return false;
        };
        match chooser.handle(*key, CHOOSER_ROWS) {
            obelus_component::chooser::Outcome::Taken => true,
            obelus_component::chooser::Outcome::Ignored => false,
            obelus_component::chooser::Outcome::Wants(directory) => {
                self.look_in(&directory);
                true
            }
            obelus_component::chooser::Outcome::Chose(path) => {
                self.settle_on(&path);
                true
            }
        }
    }

    /// Reads what a directory holds, for the box being typed in.
    ///
    /// On the main thread, and that is a measurement rather than an
    /// oversight: one `read_dir` of one directory is what a shell does
    /// between two keystrokes of `Tab`. It happens once per directory
    /// rather than once per letter -- the chooser only asks when the part
    /// before the last separator has changed -- so a reader typing a long
    /// path pays for the directories they pass through and not for the
    /// letters.
    ///
    /// Directories first and then files, each by name: what is being
    /// named is usually a project, and a list with the directories
    /// scattered through it reads as a list of files.
    fn look_in(&mut self, directory: &Path) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            // Nothing, and nothing said: a path half typed names a
            // directory that does not exist yet on almost every keystroke,
            // and a complaint about each of those is a complaint about
            // typing.
            self.offer_candidates(directory, Vec::new());
            return;
        };
        let mut found: Vec<(bool, PathBuf)> = entries
            .flatten()
            .map(|entry| (entry.path().is_dir(), entry.path()))
            .collect();
        found.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let found = found.into_iter().map(|(_, path)| path).collect();
        self.offer_candidates(directory, found);
    }

    /// Hands what was found to the box that asked for it.
    fn offer_candidates(&mut self, directory: &Path, entries: Vec<PathBuf>) {
        if let Some(chooser) = &mut self.chooser {
            chooser.offer(directory, entries);
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
        let opening = crate::app::opening(std::slice::from_ref(&path.to_path_buf()));
        let Some(root) = opening.root else {
            return;
        };
        // The chooser goes first, so that `work_in` and everything it
        // starts runs with a project settled rather than with the screen
        // still saying there is none.
        self.chooser = None;
        self.work_in(root);
        self.apply_project();
        for file in &opening.files {
            self.open(file);
        }
        // A directory is a reader saying which project and asking which
        // file, which is the list -- the same thing `ob some-directory`
        // does, said in the same place.
        if opening.list {
            self.open_file_picker();
        }
    }
}

/// How many rows the chooser's list is paged by.
///
/// The screen's own height is what a page should be, and the view is the
/// one that knows it. Until the key and the drawing take their room from
/// the same place, this is the number the keys use -- see `App::chat_key`
/// for why that is a thing worth being careful about.
const CHOOSER_ROWS: u16 = 10;

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

    /// The list does not grow without end.
    ///
    /// Broken deliberately by taking the `truncate` out of `remember`:
    /// the file grows for ever and the welcome screen's filter searches a
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
