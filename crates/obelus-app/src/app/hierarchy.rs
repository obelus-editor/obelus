//! The tree a reader opened, and what it is showing.
//!
//! Flat, because a list is flat: the tree is kept as the rows it is drawn
//! as, each carrying how deep it is. Opening a row splices its children in
//! after it; closing one takes back everything deeper that follows. That
//! is the whole of the structure, and it is the shape the picker already
//! draws -- an outline and a commit's files are laid out the same way.
//!
//! The protocol asks one question per item per direction, and it has no
//! way at all to say whether an item has callers short of naming them. So
//! a mark that offered to open a row Obelus had not asked about would be a
//! guess -- and measured against rust-analyzer on this project, the guess
//! is wrong most of the time: five of the six callers of one function here
//! have no callers of their own.
//!
//! Which is why the tree runs one question ahead of what is on screen. Each
//! row that arrives is asked about, one at a time, by nobody -- and what
//! comes back is filed rather than shown. The mark then only ever claims
//! there is something behind a row where somebody has already looked, and
//! opening a row that was asked about costs nothing at all, which is the
//! half that matters: the first question about a symbol is the dear one.
//! Measured, warm, on a settled server: three milliseconds for a narrow
//! name, eighty for one with forty-five callers, and two and a half
//! seconds the first time anybody asks about a busy one -- which is
//! exactly the cost worth paying before the reader presses anything.
//!
//! None of it is allowed in front of the reader. A question they asked and
//! a question nobody asked are two kinds here rather than one queue,
//! because questions of a kind supersede each other: sharing one would put
//! a probe a busy server is sitting on in front of their own key press.

use obelus_lsp::hierarchy::{Called, Direction};

use super::*;

/// What a row that repeats one above it carries.
///
/// An arrow turning back, because that is what the tree does there: the
/// thing this row names is already on the path to it, and opening it would
/// walk the same ring again.
const LOOPS: char = '\u{21a9}';

/// What the row a tree is rooted at is called.
///
/// A name rather than a position, like every other row: the root is the
/// one whose name is known before the tree exists.
const ROOT: u64 = 1;

/// One row of it.
#[derive(Clone, Debug)]
pub(super) struct Rung {
    /// A name of its own, which is what a question about it is asked
    /// under.
    ///
    /// Not its position: two questions about one tree are in the air at
    /// once -- the reader opening a row, and the probe finding out whether
    /// the row below has anything behind it -- and either answer can land
    /// after the other has moved every row after it. A number that moves
    /// is a number that cannot be answered against.
    id: u64,
    /// What it is, where it goes, and what to ask about it.
    pub(super) called: Called,
    /// How far in it is drawn, and what its children will be.
    pub(super) depth: usize,
    /// Whether its children are showing.
    pub(super) opened: bool,
    /// What is behind it, once anybody has asked.
    ///
    /// `None` until somebody has: which is the state the mark must not
    /// promise anything in, because the protocol has no way to say whether
    /// an item has callers short of naming them. Kept rather than counted,
    /// so opening a row that has already been asked about costs nothing --
    /// which matters, since the first question about a symbol is the dear
    /// one: measured against rust-analyzer at 2.4 seconds for a busy name,
    /// and 80 milliseconds every time after.
    behind: Option<Vec<Called>>,
    /// Whether the reader is waiting to see what is behind it.
    ///
    /// Which is both half of what the answer is for -- an answer nobody
    /// asked for is filed, and one the reader asked for is opened -- and
    /// the reason the row turns while they wait.
    pub(super) waiting: bool,
    /// Whether the same item is already somewhere above it on its own
    /// path.
    ///
    /// A recursive function reaches itself, and two functions that call
    /// each other reach each other: opening for ever is what a tree with
    /// no bottom does, so the repeat is marked and left closed. The reader
    /// can still see it is there, which is the answer they came for.
    pub(super) looping: bool,
}

impl Rung {
    /// Whether anything is behind it, as far as anybody knows.
    ///
    /// `None` where nobody has asked -- which the mark draws the same as
    /// "nothing", on purpose: a mark may only ever claim there *is*
    /// something, so the one state that is a guess understates instead.
    pub(super) fn holds(&self) -> Option<bool> {
        self.behind.as_ref().map(|behind| !behind.is_empty())
    }
}

/// What is showing.
#[derive(Clone, Debug)]
pub(super) struct Tree {
    /// Which way round the question is being asked.
    pub(super) direction: Direction,
    /// The rows, in the order they are drawn.
    rows: Vec<Rung>,
    /// What the next row to arrive will be called.
    next: u64,
    /// The row a probe is out about, if one is.
    ///
    /// One at a time: the probe is work nobody asked for, and a server
    /// answering ten of them at once is a server not answering the reader.
    probing: Option<u64>,
}

impl Tree {
    /// One rooted at the item a server prepared.
    pub(super) fn about(root: Called, direction: Direction) -> Self {
        Self {
            direction,
            rows: vec![Rung {
                id: 1,
                called: root,
                depth: 0,
                opened: false,
                behind: None,
                waiting: false,
                looping: false,
            }],
            next: 2,
            probing: None,
        }
    }

    /// The rows, in the order they are drawn.
    pub(super) fn rows(&self) -> &[Rung] {
        &self.rows
    }

    /// What a row is called.
    pub(super) fn id_at(&self, row: usize) -> Option<u64> {
        Some(self.rows.get(row)?.id)
    }

    /// Where a row is now, if it is still here.
    fn row_of(&self, id: u64) -> Option<usize> {
        self.rows.iter().position(|rung| rung.id == id)
    }

    /// The item at a row, to ask a question about.
    pub(super) fn item_of(&self, id: u64) -> Option<&serde_json::Value> {
        let row = self.row_of(id)?;
        Some(&self.rows[row].called.item)
    }

    /// Says the reader is waiting on a row, and nobody is waiting on any
    /// other.
    ///
    /// Only one at a time, because only one question of a kind can be out:
    /// a second supersedes the first the way every other question here
    /// does, and a row left turning for an answer that was taken back
    /// would turn for the rest of the session.
    pub(super) fn wants(&mut self, id: u64) {
        for rung in &mut self.rows {
            rung.waiting = rung.id == id;
        }
    }

    /// The row the reader is waiting on, if they are waiting.
    pub(super) fn wanted(&self) -> Option<u64> {
        self.rows
            .iter()
            .find(|rung| rung.waiting)
            .map(|rung| rung.id)
    }

    /// Says a probe is out about a row.
    pub(super) fn probing(&mut self, id: u64) {
        self.probing = Some(id);
    }

    /// The next row nobody has asked about.
    ///
    /// In the order they are drawn, which is the order they are read. One
    /// level beyond what is showing and no further: a row that is not on
    /// screen has no mark to be wrong about.
    pub(super) fn unasked(&self) -> Option<u64> {
        if self.probing.is_some() {
            return None;
        }
        self.rows
            .iter()
            .find(|rung| rung.behind.is_none() && !rung.looping && !rung.waiting)
            .map(|rung| rung.id)
    }

    /// Files what is behind a row, and says whether the reader was waiting
    /// for it.
    pub(super) fn keep(&mut self, id: u64, children: Vec<Called>) -> bool {
        if self.probing == Some(id) {
            self.probing = None;
        }
        let Some(row) = self.row_of(id) else {
            return false;
        };
        let rung = &mut self.rows[row];
        let wanted = rung.waiting;
        rung.waiting = false;
        rung.behind = Some(children);
        wanted
    }

    /// Puts what is behind a row under it.
    ///
    /// Each child is marked where it repeats something already on the path
    /// from the root to it, which is the only place a repeat matters: the
    /// same function called from two different branches is two honest
    /// rows.
    pub(super) fn open(&mut self, id: u64) {
        let Some(row) = self.row_of(id) else {
            return;
        };
        let Some(children) = self.rows[row].behind.clone() else {
            return;
        };
        if children.is_empty() {
            return;
        }
        self.rows[row].opened = true;
        let depth = self.rows[row].depth + 1;
        let path = self.path_to(row);
        let mut rungs = Vec::with_capacity(children.len());
        for called in children {
            rungs.push(Rung {
                id: self.next,
                looping: path
                    .iter()
                    .any(|above| *above == (called.path.clone(), called.own)),
                depth,
                opened: false,
                behind: None,
                waiting: false,
                called,
            });
            self.next += 1;
        }
        self.rows.splice(row + 1..row + 1, rungs);
    }

    /// Takes a row's children back.
    ///
    /// Everything after it that is deeper than it, which is its children
    /// and theirs: they are the rows that only exist because this one is
    /// open. What was found out about them goes with them -- they are
    /// gone, and what is kept is on the row that holds them.
    pub(super) fn close(&mut self, row: usize) {
        let Some(depth) = self.rows.get(row).map(|rung| rung.depth) else {
            return;
        };
        let end = self.rows[row + 1..]
            .iter()
            .position(|rung| rung.depth <= depth)
            .map_or(self.rows.len(), |at| row + 1 + at);
        // A probe out about one of them has nowhere to land now. Cleared
        // rather than left, or the next one would never start.
        if self
            .probing
            .is_some_and(|id| self.rows[row + 1..end].iter().any(|rung| rung.id == id))
        {
            self.probing = None;
        }
        self.rows.drain(row + 1..end);
        if let Some(rung) = self.rows.get_mut(row) {
            rung.opened = false;
        }
    }

    /// Which row above a row is the one it repeats.
    ///
    /// Only somewhere on its own path, which is the only place a repeat
    /// means anything: the same function reached down two different
    /// branches is two honest rows, and neither is the other's ring.
    pub(super) fn repeats(&self, row: usize) -> Option<usize> {
        let rung = self.rows.get(row)?;
        let what = (rung.called.path.clone(), rung.called.own);
        let mut want = rung.depth.checked_sub(1)?;
        for (above, higher) in self.rows[..row].iter().enumerate().rev() {
            if higher.depth == want {
                if (higher.called.path.clone(), higher.called.own) == what {
                    return Some(above);
                }
                want = want.checked_sub(1)?;
            }
        }
        None
    }

    /// Every item on the path from the root down to a row, that row
    /// included.
    ///
    /// Walked backwards through the rows, which is what a flat tree has
    /// instead of parent links: the row above a row that is one shallower
    /// is its parent.
    fn path_to(&self, row: usize) -> Vec<(std::path::PathBuf, (u32, u32))> {
        let mut path = Vec::new();
        let Some(mut want) = self.rows.get(row).map(|rung| rung.depth) else {
            return path;
        };
        for rung in self.rows[..=row].iter().rev() {
            if rung.depth == want {
                path.push((rung.called.path.clone(), rung.called.own));
                if want == 0 {
                    break;
                }
                want -= 1;
            }
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn called(name: &str, line: u32) -> Called {
        Called {
            name: name.to_string(),
            kind: obelus_text::kind::SyntaxKind::Function,
            path: std::path::PathBuf::from("/tmp/one.rs"),
            at: (line, 0),
            end: (line, u32::try_from(name.len()).unwrap_or(0)),
            own: (line, 0),
            item: serde_json::json!({ "name": name }),
        }
    }

    fn names(tree: &Tree) -> Vec<(usize, String)> {
        tree.rows()
            .iter()
            .map(|rung| (rung.depth, rung.called.name.clone()))
            .collect()
    }

    /// Filing what is behind a row and putting it on screen, which is what
    /// every answer here does one of.
    fn answer(tree: &mut Tree, row: usize, children: Vec<Called>) {
        let id = tree.id_at(row).expect("a row");
        tree.keep(id, children);
        tree.open(id);
    }

    /// Opening a row puts its children under it, and closing takes back
    /// everything that was only there because it was open.
    #[test]
    fn opening_and_closing_a_row() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2), called("restart", 3)]);
        assert_eq!(
            names(&tree),
            [
                (0, "run".to_string()),
                (1, "main".to_string()),
                (1, "restart".to_string())
            ]
        );

        // And deeper, under the first of them.
        answer(&mut tree, 1, vec![called("start", 4)]);
        assert_eq!(
            names(&tree),
            [
                (0, "run".to_string()),
                (1, "main".to_string()),
                (2, "start".to_string()),
                (1, "restart".to_string())
            ],
            "the child went under the wrong row"
        );

        // Closing the root takes all of it, and leaves the root.
        tree.close(0);
        assert_eq!(names(&tree), [(0, "run".to_string())]);
    }

    /// Closing a row in the middle takes its own children and nobody
    /// else's.
    #[test]
    fn closing_takes_only_what_is_under_it() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2), called("restart", 3)]);
        answer(&mut tree, 1, vec![called("start", 4)]);
        tree.close(1);
        assert_eq!(
            names(&tree),
            [
                (0, "run".to_string()),
                (1, "main".to_string()),
                (1, "restart".to_string())
            ],
            "closing one row took another row's children"
        );
    }

    /// What was found out about a row outlives it being shut, so opening
    /// it again asks nobody anything.
    ///
    /// Which is the whole reason the answer is kept: the first question
    /// about a symbol is the dear one, measured at seconds against a busy
    /// server.
    #[test]
    fn what_is_behind_a_row_survives_it_being_shut() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2)]);
        tree.close(0);
        assert_eq!(names(&tree), [(0, "run".to_string())]);

        let id = tree.id_at(0).expect("the root");
        tree.open(id);
        assert_eq!(
            names(&tree),
            [(0, "run".to_string()), (1, "main".to_string())],
            "shutting a row threw away what was behind it"
        );
    }

    /// A tree of calls has no bottom: a recursive function reaches itself.
    /// The repeat is shown and left closed.
    #[test]
    fn a_repeat_on_the_path_is_marked_and_cannot_be_opened() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2)]);
        answer(&mut tree, 1, vec![called("run", 1)]);

        let last = tree.rows().len() - 1;
        assert!(
            tree.rows()[last].looping,
            "the root came back round and was not noticed"
        );
        // And nobody ever asks about it, so it never grows a mark either.
        assert!(
            !tree
                .rows()
                .iter()
                .any(|rung| rung.looping && rung.behind.is_some()),
            "the ring was followed round again"
        );
        assert_ne!(
            tree.unasked(),
            tree.id_at(last),
            "the ring is next in line to be asked about"
        );
    }

    /// The same thing on two different branches is two honest rows: it is
    /// only a repeat where it is above itself.
    #[test]
    fn the_same_thing_on_two_branches_is_not_a_repeat() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("left", 2), called("right", 3)]);
        answer(&mut tree, 1, vec![called("shared", 9)]);
        // `right` has moved down by one now that `left` has a child.
        let right = tree
            .rows()
            .iter()
            .position(|rung| rung.called.name == "right")
            .expect("the row");
        answer(&mut tree, right, vec![called("shared", 9)]);

        let repeats: Vec<bool> = tree
            .rows()
            .iter()
            .filter(|rung| rung.called.name == "shared")
            .map(|rung| rung.looping)
            .collect();
        assert_eq!(
            repeats,
            [false, false],
            "one branch was told it repeats the other"
        );
    }

    /// Only one row at a time is one the reader is waiting on: a second
    /// press supersedes the first, the way every other question here does.
    #[test]
    fn only_one_row_is_ever_waited_on() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2), called("restart", 3)]);
        let first = tree.id_at(1).expect("a row");
        let second = tree.id_at(2).expect("a row");
        tree.wants(first);
        assert_eq!(tree.wanted(), Some(first));
        tree.wants(second);
        assert_eq!(
            tree.wanted(),
            Some(second),
            "two rows are waiting on answers, and one of them was taken back"
        );
        assert_eq!(tree.rows().iter().filter(|rung| rung.waiting).count(), 1);
    }

    /// A row keeps its name while the rows around it move, which is what
    /// makes an answer that lands late still land in the right place.
    #[test]
    fn a_row_keeps_its_name_when_the_rows_move() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2), called("restart", 3)]);
        let restart = tree.id_at(2).expect("a row");

        // Something opens above it, and every row after moves down.
        answer(&mut tree, 1, vec![called("start", 4), called("again", 5)]);
        assert_eq!(names(&tree).len(), 5);

        tree.keep(restart, vec![called("late", 9)]);
        tree.open(restart);
        assert_eq!(
            names(&tree).last(),
            Some(&(2, "late".to_string())),
            "the answer landed under whatever had taken that row's place"
        );
    }

    /// Nothing is asked about twice: a row that has been answered about
    /// drops out of the queue, and so does one a probe is already out for.
    #[test]
    fn the_probe_walks_the_rows_once() {
        let mut tree = Tree::about(called("run", 1), Direction::Callers);
        answer(&mut tree, 0, vec![called("main", 2), called("restart", 3)]);

        let first = tree.unasked().expect("something to ask about");
        assert_eq!(first, tree.id_at(1).expect("a row"), "not in reading order");
        tree.probing(first);
        assert_eq!(tree.unasked(), None, "a second probe went out at once");

        tree.keep(first, Vec::new());
        assert_eq!(
            tree.unasked(),
            tree.id_at(2),
            "the probe stopped at the first row it was told about"
        );
    }
}

/// A tree of calls, while one is on screen.
///
/// Beside the list rather than inside it: the list is rows, and what a row
/// is *for* here -- the item to ask the next question with, how deep it
/// sits, whether it repeats -- is not something a list of rows can carry.
/// The same arrangement a history has, and for the same reason.
#[derive(Debug)]
pub(super) struct Calls {
    /// The item the reader asked about, which every tree here is rooted at.
    ///
    /// Kept apart from the tree because turning round throws the tree away:
    /// who calls this and what this calls are two trees with one root.
    root: Called,
    /// What is showing.
    tree: Tree,
    /// Which server is answering about it.
    language: LanguageId,
    /// The document it was asked from, which is what a question about it is
    /// remembered against.
    buffer: DocumentId,
}

impl App {
    /// Opens the tree, on the item a server prepared.
    ///
    /// The root alone is not an answer: a list of one row saying the name
    /// the reader's cursor was already on says nothing. So the first
    /// question goes out with the list, and everything below it waits to be
    /// asked for.
    pub(super) fn on_prepared(&mut self, buffer: DocumentId, language: LanguageId, reply: Reply) {
        let Some(item) = obelus_lsp::hierarchy::prepared(&reply.result) else {
            self.wrong("Nothing here to follow".to_string());
            return;
        };
        let Some(root) = obelus_lsp::hierarchy::root_of(&item) else {
            self.wrong("Nothing here to follow".to_string());
            return;
        };
        // Whatever the tree being replaced still had out. Without this a
        // question about the old root is answered into the new tree: both
        // start at callers, so the direction check lets it through, and
        // what the reader sees is one function's callers under another
        // function's name.
        self.close_calls();
        self.quiet();
        let mut calls = Calls {
            tree: Tree::about(root.clone(), Direction::Callers),
            root,
            language,
            buffer,
        };
        // The reader is waiting on the root, which is what makes its
        // answer open rather than file itself.
        calls.tree.wants(ROOT);
        // The list first, and the tree after it. Putting a list up closes
        // whichever list was there, and closing one is what takes its tree
        // away -- so a tree installed first would be taken away by the very
        // list that is about to draw it.
        self.open_calls(&calls);
        self.calls = Some(calls);
        self.ask_called(ROOT);
    }

    /// Puts the list up, with the rows a tree has.
    fn open_calls(&mut self, calls: &Calls) {
        let mut picker = Picker::new(self.rows_of(calls), PickerLayout::FullArea);
        picker.before_typing("Filter calls");
        picker.previews();
        // The children of a row belong to it: filtering that pulled one out
        // from under the row that calls it would leave a place on screen
        // with nothing saying what reaches it.
        picker.nests();
        // Scopes rather than groups: who calls this and what this calls
        // are two answers from two questions, not two halves of one list,
        // and an "all" tab over them would promise a tree nothing produces.
        picker.with_scopes(&Direction::ALL.map(Direction::label));
        // Its rows hold what they call, so enter opens them and
        // `alt+enter` goes to one.
        picker.opens_rows();
        self.show_list(picker);
    }

    /// Puts the rows the tree has now into the list that is already up.
    fn refresh_calls(&mut self) {
        let Some(items) = self.calls.as_ref().map(|calls| self.rows_of(calls)) else {
            return;
        };
        if let Some(picker) = self.picker.as_mut() {
            picker.relist(items);
        }
    }

    /// One row per rung, in the order the tree draws them.
    fn rows_of(&self, calls: &Calls) -> Vec<PickerItem> {
        calls
            .tree
            .rows()
            .iter()
            .map(|rung| {
                let called = &rung.called;
                PickerItem {
                    prose: false,
                    icon: obelus_icons::enabled().then(|| obelus_icons::for_kind(called.kind)),
                    marker: mark_of(rung),
                    opens: opens_of(rung),
                    label: called.name.clone(),
                    detail: None,
                    trailing: Some(self.where_called(called)),
                    changed: None,
                    value: PickerValue::Place {
                        path: called.path.clone(),
                        line: called.at.0,
                        character: called.at.1,
                        end_line: called.end.0,
                        end_character: called.end.1,
                    },
                    depth: u16::try_from(rung.depth).unwrap_or(u16::MAX),
                    status: None,
                    enabled: true,
                    colours: None,
                    kind: Some(called.kind),
                    tab: None,
                    section: None,
                }
            })
            .collect()
    }

    /// Where a row's place is, written the way a reader reads a place.
    ///
    /// Relative to the tree they are reading, because everything in it is
    /// under the same root and the absolute part is the same on every row.
    fn where_called(&self, called: &Called) -> String {
        let path = called
            .path
            .strip_prefix(&self.working_directory)
            .unwrap_or(&called.path);
        format!("{}:{}", path.display(), called.at.0.saturating_add(1))
    }

    /// Asks what one row calls, or who calls it, for a reader waiting on
    /// it.
    fn ask_called(&mut self, id: u64) {
        self.ask_hierarchy(id, true);
    }

    /// The same question, asked because nobody has: what comes back is
    /// filed rather than opened, and all the reader sees of it is that the
    /// row starts offering to open, or never does.
    fn probe_called(&mut self, id: u64) {
        self.ask_hierarchy(id, false);
    }

    /// One of the two, which differ in what is done with the answer.
    ///
    /// Two kinds rather than one queue, because that is what keeps the
    /// reader in front: questions of a kind supersede each other, so a
    /// probe that a busy server is sitting on for two seconds would be
    /// holding the reader's own key press behind it. Told apart, one of
    /// each is in the air and the reader never waits on work nobody asked
    /// for.
    fn ask_hierarchy(&mut self, id: u64, wanted: bool) {
        let Some(calls) = self.calls.as_ref() else {
            return;
        };
        let Some(item) = calls.tree.item_of(id).cloned() else {
            return;
        };
        let direction = calls.tree.direction;
        let language = calls.language;
        let buffer = calls.buffer;
        let version = self.file(buffer).map_or(0, Buffer::version);
        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        let Ok(request) = client.request(direction.method(), &serde_json::json!({ "item": item }))
        else {
            if wanted {
                self.wrong("The language server is not listening".to_string());
            }
            return;
        };
        if let Some(calls) = self.calls.as_mut()
            && !wanted
        {
            calls.tree.probing(id);
        }
        self.remember(
            language,
            request,
            Question {
                asked: match wanted {
                    true => Asked::Called { direction, id },
                    false => Asked::Behind { direction, id },
                },
                buffer,
                version,
            },
        );
    }

    /// Takes an answer about a row the reader opened.
    pub(super) fn on_called(&mut self, direction: Direction, id: u64, reply: Reply) {
        let Some(nothing) = self.file_called(direction, id, &reply) else {
            return;
        };
        if let Some(calls) = self.calls.as_mut() {
            calls.tree.open(id);
        }
        // Told apart, because they are different facts about the world and
        // the reader pressed a key to find out which: a server that could
        // not answer and one that answered nothing look the same from the
        // list, and only one of them is worth pressing the key again over.
        if nothing {
            self.wrong(match &reply.result {
                Err(why) => why.clone(),
                Ok(_) => direction.nothing().to_string(),
            });
        }
        self.after_an_answer();
    }

    /// Takes an answer nobody was waiting for.
    ///
    /// Nothing is opened and nothing is said: all it changes is whether the
    /// row offers to open, which is the whole point of having asked.
    pub(super) fn on_behind(&mut self, direction: Direction, id: u64, reply: Reply) {
        if self.file_called(direction, id, &reply).is_none() {
            return;
        }
        self.after_an_answer();
    }

    /// Files what a server said about one row, and says whether it was
    /// nothing.
    ///
    /// `None` where the answer is about a tree that is no longer showing.
    fn file_called(&mut self, direction: Direction, id: u64, reply: &Reply) -> Option<bool> {
        let calls = self.calls.as_mut()?;
        // The reader turned round while it was on its way. The tree it was
        // asked about is gone, and hanging callers under a row of callees
        // would be an answer to a question nobody asked.
        if calls.tree.direction != direction {
            return None;
        }
        let children = obelus_lsp::hierarchy::called_in(&reply.result, direction);
        let nothing = children.is_empty();
        calls.tree.keep(id, children);
        Some(nothing)
    }

    /// Redraws, and asks about the next row nobody has asked about.
    ///
    /// One question ahead of what is on screen, always: the mark on a row
    /// may only claim there is something behind it, and the only way to
    /// know is to ask. Measured against rust-analyzer on this project, a
    /// narrow row costs about three milliseconds to ask about and the
    /// widest answer in it -- forty-five callers -- settles in about a
    /// tenth of a second.
    fn after_an_answer(&mut self) {
        self.refresh_calls();
        let next = self.calls.as_ref().and_then(|calls| calls.tree.unasked());
        if let Some(id) = next {
            self.probe_called(id);
        }
    }

    /// Opens the row the reader is on, or closes it again.
    ///
    /// What the list reports when the key that opens a row is pressed on
    /// one: the list knows the key was pressed and nothing else, because
    /// what is behind a row here is a question for a language server.
    pub(super) fn open_call(&mut self) {
        let Some(row) = self.picker.as_ref().and_then(Picker::selected_row) else {
            return;
        };
        let Some(calls) = self.calls.as_mut() else {
            return;
        };
        let Some(rung) = calls.tree.rows().get(row) else {
            return;
        };
        // A ring has nothing to open -- what is behind it is what is
        // behind the row it repeats -- so the key turns back to that row
        // instead, which is what the mark on it has been saying. A
        // sentence on the status bar was the other way to answer, and it
        // is gone by the next key without having said *where*.
        if rung.looping {
            let above = calls.tree.repeats(row);
            if let (Some(above), Some(picker)) = (above, self.picker.as_mut()) {
                picker.select_item(above);
            }
            return;
        }
        let direction = calls.tree.direction;
        let Some(id) = calls.tree.id_at(row) else {
            return;
        };
        match (rung.opened, rung.holds()) {
            // Shut it, and everything that was only on screen because it
            // was open.
            (true, _) => {
                calls.tree.close(row);
                self.after_an_answer();
            }
            // Asked already, and there was nothing. Said again rather than
            // shut, because shutting one would put the mark back and offer
            // the reader the same empty answer a second time.
            (false, Some(false)) => self.wrong(direction.nothing().to_string()),
            // Asked already, and kept: the dear part of this was paid when
            // nobody was waiting.
            (false, Some(true)) => {
                calls.tree.open(id);
                self.after_an_answer();
            }
            // Nobody has asked yet, so the reader waits -- and the row
            // turns while they do.
            (false, None) => {
                calls.tree.wants(id);
                self.ask_called(id);
                self.refresh_calls();
            }
        }
    }

    /// Asks the same root the other way round.
    ///
    /// A fresh tree rather than one kept aside: the two are asked of a
    /// server that has been reading the file all along, and a tree kept
    /// from before the reader last edited is a tree of places that have
    /// moved.
    pub(super) fn turn_calls_round(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let Some(direction) = Direction::ALL.get(picker.tab()).copied() else {
            return;
        };
        let Some(calls) = self.calls.as_mut() else {
            return;
        };
        if calls.tree.direction == direction {
            return;
        }
        calls.tree = Tree::about(calls.root.clone(), direction);
        calls.tree.wants(ROOT);
        self.refresh_calls();
        self.ask_called(ROOT);
    }

    /// Takes the tree away, and the questions it had out with it.
    ///
    /// The two together because they mean one thing: a question is asked
    /// about a row of a particular tree, and a tree that is gone has no
    /// rows. An answer with nothing waiting for it is dropped where every
    /// other one is -- in [`App::on_reply`], which is why this only has to
    /// forget it here.
    pub(super) fn close_calls(&mut self) {
        self.calls = None;
        let stale: Vec<(LanguageId, i64)> = self
            .asked
            .iter()
            .filter(|(_, question)| {
                matches!(question.asked, Asked::Called { .. } | Asked::Behind { .. })
            })
            .map(|(key, _)| *key)
            .collect();
        for (language, id) in stale {
            self.asked.remove(&(language, id));
            if let Some(client) = self.servers.get_mut(&language) {
                client.cancel(id);
            }
        }
    }

    /// Whether a row of the tree is waiting on a server, and so turning.
    ///
    /// What wakes the screen: a busy server takes seconds over one of
    /// these, and a mark drawn once and never again is a mark that says
    /// Obelus has stopped rather than that it is waiting.
    pub(super) fn calls_turning(&self) -> bool {
        self.calls
            .as_ref()
            .is_some_and(|calls| calls.tree.wanted().is_some())
    }

    /// Whether a tree of calls is what the list is showing.
    pub(super) const fn showing_calls(&self) -> bool {
        self.calls.is_some()
    }

    /// Hands Obelus an item, as a server that prepared one would.
    pub fn prepared_for_test(&mut self, answer: serde_json::Value) {
        let Some(id) = self.current else { return };
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            return;
        };
        self.on_prepared(
            id,
            language,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// Hands Obelus an answer about one row of the tree it is showing.
    ///
    /// Opened where the reader is waiting on that row and filed where they
    /// are not, which is what the two kinds of question mean.
    pub fn called_for_test(&mut self, row: usize, answer: serde_json::Value) {
        let Some((direction, id, wanted)) = self.calls.as_ref().and_then(|calls| {
            let id = calls.tree.id_at(row)?;
            Some((calls.tree.direction, id, calls.tree.wanted() == Some(id)))
        }) else {
            return;
        };
        let reply = Reply {
            id: 0,
            result: Ok(answer),
        };
        match wanted {
            true => self.on_called(direction, id, reply),
            false => self.on_behind(direction, id, reply),
        }
    }

    /// Hands Obelus a server's refusal to answer about a row.
    pub fn refused_call_for_test(&mut self, row: usize, why: &str) {
        let Some((direction, id)) = self
            .calls
            .as_ref()
            .and_then(|calls| Some((calls.tree.direction, calls.tree.id_at(row)?)))
        else {
            return;
        };
        self.on_called(
            direction,
            id,
            Reply {
                id: 0,
                result: Err(why.to_string()),
            },
        );
    }

    /// Hands Obelus an answer to the question it was asking before the
    /// reader turned the tree round.
    pub fn late_call_for_test(&mut self, row: usize, answer: serde_json::Value) {
        let Some((direction, id)) = self
            .calls
            .as_ref()
            .and_then(|calls| Some((calls.tree.direction, calls.tree.id_at(row)?)))
        else {
            return;
        };
        let stale = match direction {
            Direction::Callers => Direction::Calls,
            Direction::Calls => Direction::Callers,
        };
        self.on_called(
            stale,
            id,
            Reply {
                id: 0,
                result: Ok(answer),
            },
        );
    }

    /// What the reader is standing on: how deep it sits, and its name.
    ///
    /// Both, because a ring has the same name as the row it repeats -- that
    /// is what makes it a ring -- so a name alone cannot say which of the
    /// two the reader is on.
    #[must_use]
    pub fn selected_call_for_test(&self) -> Option<(usize, String)> {
        let row = self.picker.as_ref().and_then(Picker::selected_row)?;
        let calls = self.calls.as_ref()?;
        let rung = calls.tree.rows().get(row)?;
        Some((rung.depth, rung.called.name.clone()))
    }

    /// Which way round the tree on screen is being read.
    #[must_use]
    pub fn calls_direction_for_test(&self) -> Option<&'static str> {
        self.calls
            .as_ref()
            .map(|calls| calls.tree.direction.label())
    }

    /// The rows of the tree, as what they are called and how deep they sit.
    ///
    /// For tests, which is the only thing that can see a tree without a
    /// terminal.
    #[must_use]
    pub fn call_tree_for_test(&self) -> Vec<(usize, String)> {
        self.calls.as_ref().map_or_else(Vec::new, |calls| {
            calls
                .tree
                .rows()
                .iter()
                .map(|rung| (rung.depth, rung.called.name.clone()))
                .collect()
        })
    }
}

/// What a row says about itself, in its own column.
///
/// Four states, and the one that is easy to get wrong is the mark's whole
/// job: it may claim there is something behind a row only where somebody
/// has asked and there was. A row nobody has asked about draws nothing --
/// the same as a row with nothing behind it -- because the protocol cannot
/// say whether an item has callers short of naming them, and a mark that
/// guessed would be a mark that sends the reader to an empty answer.
fn mark_of(rung: &Rung) -> Option<(Marking, String)> {
    if rung.waiting {
        // Turning, because a busy server takes seconds over this and a key
        // that looks like it did nothing is a key nobody presses twice.
        return Some((Marking::Working, String::new()));
    }
    if rung.looping {
        return Some((Marking::Aside, LOOPS.to_string()));
    }
    None
}

/// Whether a rung opens, and whether it is open.
///
/// Said rather than drawn as a marker, so that whoever asks -- the view
/// drawing the arrow, a pointer wondering what it may open -- asks the same
/// question of the same field.
fn opens_of(rung: &Rung) -> Option<bool> {
    match (rung.opened, rung.holds()) {
        (true, _) | (false, Some(true)) => Some(rung.opened),
        (false, _) => None,
    }
}
