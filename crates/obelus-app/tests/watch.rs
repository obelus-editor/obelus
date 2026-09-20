//! Noticing changes on disk.
//!
//! These are timing tests against the real filesystem, so each waits with a
//! generous deadline rather than sleeping a fixed amount: a slow machine
//! should make them slower, not flaky.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

use obelus_app::event::Event;
use obelus_watch::Watcher;

/// How long a change has to arrive in before the test gives up.
const DEADLINE: Duration = Duration::from_secs(5);

/// A directory only one test touches.
///
/// A directory rather than a file in the shared temporary directory: the watch
/// is on a whole directory, so anything else writing to it would arrive as
/// events for this test.
struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("obelus-watch-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("creating the scratch directory");
        Self { directory }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.file(name);
        fs::write(&path, contents).expect("writing");
        path
    }

    /// Saves the way an editor does: a new file, then a rename over the old
    /// one. The inode changes, which is what a watch on the file itself does
    /// not survive.
    fn replace_by_rename(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.file(name);
        let temporary = self.directory.join(format!("{name}.tmp"));
        fs::write(&temporary, contents).expect("writing the replacement");
        fs::rename(&temporary, &path).expect("renaming over the original");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Waits for a change to `path`, ignoring changes to anything else.
fn wait_for(events: &Receiver<Event>, path: &Path) -> bool {
    let deadline = Instant::now() + DEADLINE;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(remaining) {
            Ok(Event::Watched(obelus_watch::Changed { path: changed })) if changed == path => {
                return true;
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

/// Collects every change that arrives in `window`.
fn collect(events: &Receiver<Event>, window: Duration) -> Vec<PathBuf> {
    let deadline = Instant::now() + window;
    let mut seen = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(remaining) {
            Ok(Event::Watched(obelus_watch::Changed { path })) => seen.push(path),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}

#[test]
fn a_write_is_reported() {
    let scratch = Scratch::new("write");
    let path = scratch.write("a.rs", "fn main() {}\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    scratch.write("a.rs", "fn main() { }\n");
    assert!(wait_for(&events, &path), "the write was never reported");
}

/// The case a watch on the file itself fails, and it fails on the *second*
/// save rather than the first — which is why watching the directory has to be
/// the arrangement from the start rather than a fix later.
#[test]
fn a_file_replaced_by_rename_is_still_reported_the_second_time() {
    let scratch = Scratch::new("rename");
    let path = scratch.write("a.rs", "fn main() {}\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    scratch.replace_by_rename("a.rs", "fn one() {}\n");
    assert!(
        wait_for(&events, &path),
        "the first rename was not reported"
    );

    scratch.replace_by_rename("a.rs", "fn two() {}\n");
    assert!(
        wait_for(&events, &path),
        "the second rename was not reported, so the watch followed the inode"
    );
}

/// One save arrives as several filesystem events. Reloading on each would
/// reparse the same file repeatedly and, worse, read it half-written.
#[test]
fn a_burst_of_writes_is_reported_once() {
    let scratch = Scratch::new("burst");
    let path = scratch.write("a.rs", "0\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    for index in 1..=10 {
        scratch.write("a.rs", &format!("{index}\n"));
    }

    let seen = collect(&events, Duration::from_millis(600));
    let mine = seen.iter().filter(|changed| **changed == path).count();
    assert!(mine >= 1, "ten writes produced no change at all");
    assert!(
        mine <= 3,
        "ten writes produced {mine} changes, so they are not being gathered"
    );
}

/// A second file in the same directory must not start a second watch, and must
/// still be reported.
#[test]
fn two_files_in_one_directory_share_a_watch() {
    let scratch = Scratch::new("shared");
    let first = scratch.write("a.rs", "0\n");
    let second = scratch.write("b.rs", "0\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&first).expect("watching the first");
    watcher.watch(&second).expect("watching the second");

    scratch.write("b.rs", "1\n");
    assert!(
        wait_for(&events, &second),
        "the second file was not reported"
    );
}

/// Deleting the file is reported like any other change; what happens next is
/// the buffer's business, and it keeps what it last held.
#[test]
fn a_deletion_is_reported() {
    let scratch = Scratch::new("delete");
    let path = scratch.write("a.rs", "fn main() {}\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    fs::remove_file(&path).expect("deleting");
    assert!(wait_for(&events, &path), "the deletion was never reported");
}

/// Reading the file must not be reported as a change to it.
///
/// On Linux a read produces `Access(Open(Any))`. Without the filter obelus
/// reports its own reload's read as a change, which triggers another reload,
/// which reads again: a loop that never settles, spinning on one file and
/// redrawing forever. The reload's content comparison does not stop it —
/// nothing about the buffer changes, but the reading does not stop either.
#[test]
fn reading_the_file_is_not_a_change() {
    let scratch = Scratch::new("read");
    let path = scratch.write("a.rs", "fn main() {}\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    // Exactly what a reload does.
    let _ = fs::read_to_string(&path).expect("reading");

    let seen = collect(&events, Duration::from_millis(400));
    assert!(
        !seen.contains(&path),
        "reading the file was reported as changing it, which is a feedback loop"
    );
}

/// A file being written continuously has to be reported while it is still
/// being written.
///
/// The deadline belongs to the first change in a burst, not to the most
/// recent. Refreshing it on every event looks like the same thing and is not:
/// a writer that never pauses for longer than the debounce window is never
/// reported at all.
#[test]
fn a_file_written_continuously_is_still_reported() {
    let scratch = Scratch::new("continuous");
    let path = scratch.write("a.rs", "0\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    let writer = {
        let directory = scratch.directory.clone();
        std::thread::spawn(move || {
            for index in 1..=30 {
                fs::write(directory.join("a.rs"), format!("{index}\n")).expect("writing");
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };

    // Well inside the six hundred milliseconds the writer runs for, and well
    // outside the eighty the debounce window is.
    let seen = collect(&events, Duration::from_millis(400));
    let reported = seen.contains(&path);
    writer.join().expect("the writer thread");

    assert!(
        reported,
        "nothing was reported in four hundred milliseconds of continuous writing"
    );
}
