//! Noticing changes on disk.
//!
//! These are timing tests against the real filesystem, so each waits with a
//! generous deadline rather than sleeping a fixed amount: a slow machine
//! should make them slower, not flaky.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc::Receiver},
    time::{Duration, Instant},
};

mod support;

use obelus_app::event::Event;
use obelus_command::Command;
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

/// A second watcher on the same path, saying what kind each event was.
///
/// `obelus_watch` hands over a path and nothing else, which is the whole
/// of what Obelus wants to know and leaves a failing test with nothing to
/// say but "something arrived". What these tests are about is which
/// things a platform reports and which of those Obelus should refuse, and
/// that question cannot be asked without the kinds -- so where one of
/// them fails, this is what it fails *with*.
fn kinds(path: &Path) -> (notify::RecommendedWatcher, Arc<Mutex<Vec<String>>>) {
    use notify::Watcher as _;
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&seen);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && let Ok(mut into) = into.lock()
        {
            into.push(format!("{:?} {:?}", event.kind, event.paths));
        }
    })
    .expect("a second watcher");
    let directory = path.parent().unwrap_or(path);
    watcher
        .watch(directory, notify::RecursiveMode::NonRecursive)
        .expect("watching it too");
    (watcher, seen)
}

/// What that second watcher saw, for a failure to name.
fn said(seen: &Arc<Mutex<Vec<String>>>) -> String {
    seen.lock()
        .map(|seen| seen.join("\n  "))
        .unwrap_or_else(|_| "(poisoned)".to_string())
}

/// Waits until whatever the setting up stirred is over.
///
/// A watcher is started on a file the test has just made, and on a
/// platform whose events are coalesced and delivered late that creation
/// and write arrive *after* the watch is taken -- with their flags rolled
/// into whatever the test does next. macOS reported a `chmod` as
/// `Create(File)`, `Modify(Metadata(Ownership))` and
/// `Modify(Data(Content))` at once, and the two about reading saw the
/// setup's own write land in the window they were watching.
///
/// Quiet is not the answer, because quiet is a guess about how late is
/// late. Two hundred milliseconds of nothing was how this used to decide,
/// and a loaded mac runner delivered the setup's write after it: the
/// second watcher in the failure, taken once the quiet was over, saw
/// nothing at all, so what Obelus's watcher reported was not the read.
///
/// So something is made after the setup and waited for. Events arrive in
/// the order things happened, so once the barrier has been reported the
/// setup's events have been too -- before it, or handed over in the same
/// breath, which is what the short wait after it is for. A directory and
/// not a file, because making one is one event everywhere: a file's
/// creation, its write and (on Linux) its close can be told separately,
/// and one of them arriving late would be the barrier doing what it is
/// here to stop.
fn settled(events: &Receiver<Event>, directory: &Path) {
    let barrier = directory.join("settled");
    fs::create_dir(&barrier).expect("making the barrier");
    assert!(
        wait_for(events, &barrier),
        "the barrier was never reported, so nothing here is being watched"
    );
    let _ = collect(events, Duration::from_millis(200));
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

    // One handle written ten times, not ten opened and closed. What the
    // debouncing has to gather is the events of a *burst*, and a burst is
    // what this has to produce: opening, writing and closing ten times
    // took longer on a loaded runner than the window is wide, so the
    // writes fell into eight windows of their own and the test read that
    // as the gathering being broken.
    {
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("opening it once");
        for index in 1..=10 {
            writeln!(file, "{index}").expect("writing");
            file.flush().expect("flushing");
        }
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
/// On Linux a read produces `Access(Open(Any))`. Without the filter Obelus
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
    settled(&events, &scratch.directory);
    let (_kinds, kinds) = kinds(&path);

    // Exactly what a reload does.
    let _ = fs::read_to_string(&path).expect("reading");

    let seen = collect(&events, Duration::from_millis(400));
    assert!(
        !seen.contains(&path),
        "reading the file was reported as changing it, which is a feedback loop.\n  {}",
        said(&kinds)
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

/// A change to what the file's record says is not a change to the file.
///
/// A mode, an owner, a time: none of them changes a line, so none of them
/// is a reason to read one again. Which matters most where it cannot be
/// seen from here: FSEvents has no event for a file being closed, so on
/// macOS a *read* announces itself the only way that API can -- the access
/// time moves, and `notify` hands that over as a metadata change. Obelus
/// reading a file then woke Obelus, which read it again.
///
/// Asked here with a mode, because that is the one metadata change a Linux
/// watcher reports (`IN_ATTRIB`) and a read is not: the two tests above
/// about reading say nothing at all on this platform, and pass whether the
/// rule is there or not.
///
/// Deliberate break: take the `Modify(Metadata(_))` refusal out of
/// `obelus_watch` and this arrives.
///
/// Linux's, and only there, which the diagnostic said in its own words: a
/// `chmod` on macOS came back as `Create(File)`,
/// `Modify(Metadata(Ownership))` and `Modify(Data(Content))` at once.
/// FSEvents reports the flags a path has *accumulated*, so the creation
/// and the write that made the file are still on the next event about it
/// however long the wait -- there is no telling a metadata change from a
/// data one there, and so nothing to hold to. The refusal is still right
/// and still worth having: it is what stops a `chmod` waking Obelus where
/// a watcher can say that is all it was.
#[test]
#[cfg(target_os = "linux")]
fn a_change_of_mode_is_not_a_change_to_the_file() {
    let scratch = Scratch::new("mode");
    let path = scratch.write("a.rs", "fn main() {}\n");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");
    settled(&events, &scratch.directory);
    let (_kinds, kinds) = kinds(&path);

    let mut how = fs::metadata(&path).expect("its mode").permissions();
    how.set_readonly(true);
    fs::set_permissions(&path, how).expect("changing it");

    let seen = collect(&events, Duration::from_millis(400));
    assert!(
        !seen.contains(&path),
        "a mode was reported as the file changing.\n  {}",
        said(&kinds)
    );
}

/// An Obelus that is killed reports that it let go of what it was holding.
///
/// The one thing on disk that nothing writes: a claim on a conversation is
/// a lock, taking one writes nothing, and a process that crashes or loses
/// power gives its lock up without a byte changing. That read as "nothing
/// can tell you", and the answer was to go and look on every frame for as
/// long as the page was showing.
///
/// Something does tell you. The kernel closes a dying process's files, and
/// a watcher reports a file closed by a process that had it open for
/// *writing* -- which is how `obelus_agent::chats` opens a claim, on
/// purpose.
///
/// Broken deliberately by putting `EventKind::Access(_)` back as a blanket
/// refusal in `obelus_watch`: nothing arrives and this waits out its
/// deadline.
///
/// Linux's, and only there. What the notice is made of is inotify's
/// `IN_CLOSE_WRITE` -- and neither macOS's FSEvents nor Windows's
/// `ReadDirectoryChangesW` has an event for a file another process
/// closed, so there is nothing for `notify` to hand over. See
/// `obelus_watch` for what that costs, which is a stale lock on a page
/// the reader is already looking at and not a conversation they cannot
/// open.
#[test]
#[cfg(target_os = "linux")]
fn a_process_that_is_killed_reports_letting_go_of_what_it_held() {
    let scratch = Scratch::new("killed");
    let path = scratch.write("claim", "");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");

    // The other Obelus: it holds the claim open for writing and is then
    // killed, which is the way out that writes nothing.
    //
    // `exec sleep` and not `sleep`, which is the whole test. What has to
    // be killed is the process holding the claim, and `kill` reaches only
    // the process that was spawned -- so the shell has to *become* the
    // thing that waits rather than fork it and stand over it. bash execs
    // the last command of a `-c` list by itself, which is why this passed
    // on the machine it was written on; where `/bin/sh` is dash it forked,
    // the shell died, `sleep` went on holding the claim as an orphan, and
    // the close that reports a claim let go never happened. Measured both
    // ways: with the fork, `fuser` still names a holder after the kill.
    let mut theirs = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "exec 9>{} ; echo open ; exec sleep 30",
            path.display()
        ))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the other Obelus");
    // Waited on rather than slept through. Four hundred milliseconds was
    // the shell's whole head start, and on a busy runner it had not opened
    // the claim yet when the kill arrived -- a close that never happened
    // reports nothing, and the test read that as the watcher's silence.
    // It says so itself now, which is a fact rather than a guess about how
    // long a shell takes.
    let mut said = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(theirs.stdout.take().expect("its stdout")),
        &mut said,
    )
    .expect("the other Obelus saying it had the claim open");
    // What opening it said, which is not what this is about.
    let _ = collect(&events, Duration::from_millis(400));
    theirs.kill().expect("killing it");
    let _ = theirs.wait();

    assert!(
        wait_for(&events, &path),
        "an Obelus died holding a claim and nothing said so"
    );
}

/// Reading a file does not wake the loop.
///
/// The other half of the one above, and the reason it is safe: Obelus looks
/// at every claim in the project whenever it is told one moved, and a look
/// that announced itself would be Obelus waking itself up to look again,
/// for ever. So the look is a read -- `chats::held` opens without write
/// access -- and a read is the Access event this still refuses.
///
/// Broken deliberately by letting `EventKind::Access(_)` through whole in
/// `obelus_watch`: the read arrives and this goes red.
#[test]
fn reading_a_file_says_nothing() {
    let scratch = Scratch::new("reading");
    let path = scratch.write("claim", "");

    let (sender, events) = obelus_app::event::channel();
    let mut watcher = Watcher::new(sender).expect("starting the watcher");
    watcher.watch(&path).expect("watching");
    settled(&events, &scratch.directory);
    let (_kinds, kinds) = kinds(&path);

    // Opened for reading and closed, which is what looking at a claim is.
    {
        let _look = fs::File::options()
            .read(true)
            .open(&path)
            .expect("looking at it");
    }

    assert!(
        collect(&events, Duration::from_millis(400)).is_empty(),
        "looking at a file woke the loop, which would wake it again.\n  {}",
        said(&kinds)
    );
}

/// A project that gains its first settings while it is open.
///
/// The watch is on the file the project *would* have, so that the file
/// appearing is a change like any other -- and it cannot be taken while
/// there is nowhere to take it, which is the ordinary case: a project with
/// no `.obelus` of its own. So the project itself is watched for the
/// directory turning up, and the watch on the file is taken then.
///
/// Broken deliberately two ways, and each leaves the setting on the screen
/// unchanged for the rest of the session: not watching the project at all,
/// and hearing the directory appear without taking the watch it makes
/// possible. It is written against the real filesystem rather than by
/// handing the application its own `Watched` event, because what is being
/// tested is whether a watch was *taken* -- an event fed in by hand proves
/// the handler and nothing else.
#[test]
fn a_project_that_gains_settings_while_it_is_open_is_heard() {
    let scratch = Scratch::new("project-settings");
    // Under the project rather than in it, and that is the whole of the
    // setup: a file of the root's own is watched by *its* directory, which
    // is the project -- so the first version of this, whose file sat in the
    // root, heard the directory appear whatever this change did and passed
    // with every part of it taken out.
    let source = scratch.directory.join("src");
    fs::create_dir_all(&source).expect("making the source directory");
    let file = source.join("one.rs");
    fs::write(&file, "fn one() {}\n").expect("writing the file");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = obelus_app::app::App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the file"),
    ]);
    app.working_directory_for_test(scratch.directory.clone());
    app.start(sender);

    // The project has no settings of its own, which is where this starts.
    let settings = scratch.directory.join(".obelus").join("config.toml");
    assert!(!settings.exists(), "the test began with settings already");

    // Somebody -- another window, a `git pull` -- gives the project its
    // first ones. The directory and the file, which may arrive in either
    // order and usually arrive together.
    fs::create_dir_all(settings.parent().expect("a directory")).expect("making it");
    // A setting whose default is *not* what the file asks for, which is
    // the whole of what this test can see. The first version wrote
    // `wrap = false` -- and `wrap` is false by default, so the condition
    // below was true before anything arrived and the test returned on its
    // first turn round the loop having checked nothing at all.
    assert!(
        app.config().blame_margin,
        "the setting this watches for is already what the file will ask for"
    );
    fs::write(&settings, "blame_margin = false\n").expect("writing the settings");

    // Whatever arrives, it has to end with the setting taken: the
    // directory appearing, the file appearing, or both.
    let deadline = Instant::now() + DEADLINE;
    let mut heard = Vec::new();
    while Instant::now() < deadline {
        let Ok(event) = events.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        if let Event::Watched(obelus_watch::Changed { path }) = &event {
            heard.push(path.display().to_string());
        }
        app.handle(event);
        if !app.config().blame_margin {
            assert!(
                !heard.is_empty(),
                "the setting was taken without anything being heard, so this \
                 is not a test of the watching"
            );
            return;
        }
    }
    panic!("the project's first settings were never heard; what arrived: {heard:?}");
}

/// And the file arriving after the directory, which is the other order.
///
/// A directory made now and written into later -- `mkdir .obelus` and then
/// an editor saving into it -- is heard only because the watch on the
/// directory was taken when it appeared. The test above cannot say so: the
/// two arrive together there, so the directory's own event carries the
/// file with it and the watch is never needed.
///
/// Broken deliberately by not taking that watch.
#[test]
fn settings_written_after_the_directory_is_made_are_heard() {
    let scratch = Scratch::new("project-settings-later");
    let source = scratch.directory.join("src");
    fs::create_dir_all(&source).expect("making the source directory");
    let file = source.join("one.rs");
    fs::write(&file, "fn one() {}\n").expect("writing the file");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = obelus_app::app::App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the file"),
    ]);
    app.working_directory_for_test(scratch.directory.clone());
    app.start(sender);
    assert!(app.config().blame_margin);

    // The directory alone, and everything it produces handled before
    // anything is written into it -- which is what makes this the other
    // order rather than the one above.
    let directory = scratch.directory.join(".obelus");
    fs::create_dir_all(&directory).expect("making the directory");
    let settled = Instant::now() + Duration::from_millis(600);
    while Instant::now() < settled {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
            app.handle(event);
        }
    }
    assert!(
        app.config().blame_margin,
        "the setting was taken before the file was written"
    );

    fs::write(directory.join("config.toml"), "blame_margin = false\n").expect("writing");

    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        let Ok(event) = events.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        app.handle(event);
        if !app.config().blame_margin {
            return;
        }
    }
    panic!("settings written into a directory Obelus watched were never heard");
}

/// A directory that could not be watched is not counted as watched.
///
/// The count is what says a directory is already watched, and a second ask
/// for one that is returns `Ok` without going near the watcher. Raised by
/// an attempt that failed, it left the directory counted and unwatched --
/// and nothing gives that count back, because what is unwatched is what a
/// caller was told it held. The next ask was answered by the count alone.
///
/// Broken deliberately by counting before watching, which is what it did.
#[test]
fn a_directory_that_could_not_be_watched_is_not_counted() {
    let scratch = Scratch::new("counted");
    let (sender, events) = std::sync::mpsc::channel();
    let mut watcher = Watcher::new(sender).expect("a watcher");

    // Nothing to watch yet, so this fails.
    let directory = scratch.directory.join("later");
    assert!(
        watcher.watch_directory(&directory).is_err(),
        "a directory that is not there was watched"
    );

    // And now there is, so this must actually take the watch rather than
    // be answered by a count the failure left behind.
    fs::create_dir_all(&directory).expect("making it");
    watcher
        .watch_directory(&directory)
        .expect("watching it once it is there");

    let path = directory.join("something.txt");
    fs::write(&path, "hello\n").expect("writing into it");
    assert!(
        wait_for(&events, &path),
        "nothing arrived, so the second watch was the count and not a watch"
    );
}

/// A watch given up once is given up.
///
/// The count says how many callers want a directory watched, so a caller
/// that was never given a watch must not have been counted: an attempt that
/// failed and counted itself anyway leaves the directory needing two
/// `unwatch`es to be let go of, and the one the caller makes is swallowed
/// by the count the failure left.
///
/// Broken deliberately by counting before watching rather than after.
#[test]
fn a_watch_let_go_of_once_is_let_go_of() {
    let scratch = Scratch::new("let-go");
    let (sender, events) = std::sync::mpsc::channel();
    let mut watcher = Watcher::new(sender).expect("a watcher");

    // One caller that got nothing, because there was nothing to watch.
    let directory = scratch.directory.join("later");
    assert!(watcher.watch_directory(&directory).is_err());

    // And one that did.
    fs::create_dir_all(&directory).expect("making it");
    watcher.watch_directory(&directory).expect("watching it");
    let heard = directory.join("first.txt");
    fs::write(&heard, "one\n").expect("writing");
    assert!(wait_for(&events, &heard), "the watch was never taken");

    // That one gives it up, and it is given up: the attempt that failed is
    // not a caller holding it.
    watcher.unwatch_directory(&directory);
    let quiet = directory.join("second.txt");
    fs::write(&quiet, "two\n").expect("writing again");
    assert!(
        !wait_for(&events, &quiet),
        "the directory is still watched, so the failed attempt was counted"
    );
}

/// Settings a project had and then lost, and then had again.
///
/// The watch on the file goes with the directory it was in -- the kernel
/// drops it when the directory is deleted, and says nothing to Obelus about
/// having done so. What is left is the project's own watch, which is why it
/// is taken whether or not the project has settings today: a reader who
/// removes `.obelus` and makes it again is asking the same question as one
/// who never had it.
///
/// Broken deliberately two ways, and each leaves the second lot of settings
/// unheard: watching the project only where it has no settings yet, and
/// letting the count answer for a watch the kernel has dropped.
#[test]
fn settings_a_project_loses_and_gains_again_are_heard() {
    let scratch = Scratch::new("project-settings-again");
    let source = scratch.directory.join("src");
    fs::create_dir_all(&source).expect("making the source directory");
    let file = source.join("one.rs");
    fs::write(&file, "fn one() {}\n").expect("writing the file");

    // Settings the project has from the start, which is what makes this
    // the case the project's own watch is not taken for.
    let directory = scratch.directory.join(".obelus");
    fs::create_dir_all(&directory).expect("making the directory");
    fs::write(directory.join("config.toml"), "blame_margin = false\n").expect("the settings");

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = obelus_app::app::App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the file"),
    ]);
    app.working_directory_for_test(scratch.directory.clone());
    app.start(sender);
    assert!(
        !app.config().blame_margin,
        "the project's settings were not read at all"
    );

    let settle = |app: &mut obelus_app::app::App, how_long: Duration| {
        let until = Instant::now() + how_long;
        while Instant::now() < until {
            if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
                app.handle(event);
            }
        }
    };

    // Taken away, whole.
    fs::remove_dir_all(&directory).expect("removing the directory");
    settle(&mut app, DEADLINE / 2);
    assert!(
        app.config().blame_margin,
        "settings that were deleted are still being applied"
    );

    // And put back -- the directory first, and the file into it only once
    // everything that came of the directory has been dealt with. Written
    // together, the directory's own event carries the file with it and the
    // watch on the directory is never needed: the two halves of this have
    // to be separated or the test passes with the watch never taken.
    fs::create_dir_all(&directory).expect("making it again");
    settle(&mut app, Duration::from_millis(600));
    assert!(
        app.config().blame_margin,
        "the settings were taken before they were written"
    );
    fs::write(directory.join("config.toml"), "blame_margin = false\n").expect("the settings");
    settle(&mut app, DEADLINE / 2);
    assert!(
        !app.config().blame_margin,
        "settings put back after the directory was deleted were never heard"
    );
}

/// A project on a tree, and a file from it open with a setting of the
/// project's own in force: what a tree that goes takes with it.
fn a_tree_with_settings(name: &str) -> (Scratch, PathBuf, obelus_app::app::App) {
    let scratch = Scratch::new(name);
    let source = scratch.directory.join("src");
    fs::create_dir_all(&source).expect("making the source directory");
    let file = source.join("one.rs");
    fs::write(&file, "fn one() {}\n").expect("writing the file");
    let settings = scratch.directory.join(".obelus").join("config.toml");
    fs::create_dir_all(settings.parent().expect("a directory")).expect("making it");
    fs::write(&settings, "blame_margin = false\n").expect("writing the settings");
    let mut app = obelus_app::app::App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening the file"),
    ]);
    app.working_directory_for_test(scratch.directory.clone());
    assert!(
        !app.config().blame_margin,
        "the project's setting was not in force to begin with"
    );
    (scratch, settings, app)
}

/// The same, with the tree already gone and Obelus told so.
///
/// By hand rather than through a watcher, because this is about what the
/// application makes of the change and holds on every platform; that the
/// change arrives at all is a test of its own. What arrives first is
/// whatever the kernel and the debouncing leave first, which is usually
/// something inside the tree -- so the change fed here is the project's
/// settings file going, and that has to be enough.
///
/// The file is changed and not written first, because what is unwritten
/// in a tree that has gone goes with it.
fn a_tree_that_has_gone(name: &str) -> (Scratch, obelus_app::app::App) {
    let (scratch, settings, mut app) = a_tree_with_settings(name);
    support::type_text(&mut app, "x");
    assert!(
        app.current_buffer()
            .is_some_and(obelus_buffer::Buffer::is_dirty),
        "nothing was left unwritten"
    );
    assert!(
        app.offers(Command::FileOpen),
        "a project that is there offers its files"
    );
    fs::remove_dir_all(&scratch.directory).expect("the tree going");
    app.handle(Event::Watched(obelus_watch::Changed { path: settings }));
    (scratch, app)
}

/// A tree that goes from under Obelus takes the project with it and
/// everything that was open in it, and the page that is left says so.
///
/// Broken deliberately, one at a time: the question about the tree taken
/// out of the handler (nothing goes dim), the documents left open when the
/// tree goes (one is still open), the project's settings kept over the
/// reader's (the setting stays), and the page left out of the frame (the
/// words are not on screen) -- each fails its own line below.
#[test]
fn a_tree_that_goes_takes_the_project_and_everything_open_in_it() {
    let (scratch, mut app) = a_tree_that_has_gone("tree-gone");

    assert!(
        app.tree_has_gone(),
        "a tree that has gone was taken for one that is there"
    );
    for command in [
        Command::FileOpen,
        Command::FileNew,
        Command::SearchProject,
        Command::TodoOpen,
        Command::ConversationNew,
        Command::ConfigProject,
        Command::HistoryProject,
        Command::FileChanged,
        Command::DocumentList,
    ] {
        assert!(
            !app.offers(command),
            "{} is offered on a tree that has gone",
            command.name()
        );
    }
    assert_eq!(
        app.document_count_for_test(),
        0,
        "what was open in a tree that has gone is still open"
    );
    assert!(
        app.config().blame_margin,
        "the settings of a project that has gone are still in force"
    );
    let dump = support::render(&mut app, 80, 8);
    assert!(
        dump.contains("The project has gone") && dump.contains("Nothing is left at"),
        "the page does not say so:\n{dump}"
    );
    assert!(!scratch.directory.exists(), "the tree was made again");
}

/// Enter on that page asks which project, and nothing else there does
/// anything but leave.
///
/// Broken deliberately twice: enter left to fall through (the page that
/// asks never comes), and the page left in the file's context rather than
/// a dialog's (the palette opens over it).
#[test]
fn enter_asks_which_project_once_the_tree_has_gone() {
    let (_scratch, mut app) = a_tree_that_has_gone("tree-gone-enter");
    support::state_of_its_own();

    support::press_control(&mut app, 'p');
    assert!(
        !app.is_showing_dialog(),
        "a key other than enter opened something over the page"
    );

    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let dump = support::render(&mut app, 80, 8);
    assert!(
        dump.contains("Open a project") && !dump.contains("The project has gone"),
        "enter did not ask which project:\n{dump}"
    );
    assert!(!app.should_quit(), "enter left Obelus");
}

/// And the key that leaves leaves, without asking about what was
/// unwritten: there is nowhere left to write it.
///
/// Broken deliberately by keeping the documents open, which puts the
/// question about unsaved work in the way.
#[test]
fn leaving_once_the_tree_has_gone_asks_nothing() {
    let (_scratch, mut app) = a_tree_that_has_gone("tree-gone-leaving");
    support::press_control(&mut app, 'q');
    assert!(app.should_quit(), "the key that leaves did not leave");
}

/// And the change arrives: a tree deleted from under a watching Obelus is
/// heard, without anything asking.
///
/// Linux's, because what this rests on is inotify telling a watch that the
/// directory it was on has gone. Elsewhere the window may go on believing
/// in the tree until something else says otherwise; what holds there as
/// well is that nothing of the project is written into a tree that has
/// gone, which is asked of the disk at the moment of writing.
///
/// Broken deliberately by taking the question about the tree out of the
/// handler: everything arrives and nothing is made of it.
#[test]
#[cfg(target_os = "linux")]
fn a_tree_deleted_from_under_obelus_is_heard() {
    let (scratch, _, mut app) = a_tree_with_settings("tree-deleted");
    let (sender, events) = std::sync::mpsc::channel();
    app.start(sender);
    settled_into(&events, &mut app);

    fs::remove_dir_all(&scratch.directory).expect("the tree going");
    let deadline = Instant::now() + DEADLINE;
    let mut heard = Vec::new();
    while Instant::now() < deadline && !app.tree_has_gone() {
        let Ok(event) = events.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        if let Event::Watched(obelus_watch::Changed { path }) = &event {
            heard.push(path.display().to_string());
        }
        app.handle(event);
    }
    assert!(
        app.tree_has_gone(),
        "the tree went and Obelus did not hear it; what arrived: {heard:?}"
    );
}

/// Where a server offering tools is listening, from the address an agent
/// is told.
#[cfg(target_os = "linux")]
fn listening_at(url: &str) -> String {
    url.trim_start_matches("http://")
        .split('/')
        .next()
        .expect("an address")
        .to_string()
}

/// A window whose tree went stops what was about it, and starts again on
/// whatever project the reader names next.
///
/// The tools are what this watches, because an address is a thing a test
/// can knock on: a server left listening answers for a tree that is not
/// there, under an address no agent is told any more.
///
/// Linux's, because it is a real watcher hearing a real `rm -rf`.
///
/// Broken deliberately twice: the server's task left running when what
/// holds it goes (`Listening` without its `Drop`), and the old address
/// still answers; and `the_page_saying_it_has_gone` leaving `gone` set,
/// and the project chosen afterwards offers nothing.
#[test]
#[cfg(target_os = "linux")]
fn a_window_whose_tree_went_starts_again_on_another() {
    let (scratch, _, mut app) = a_tree_with_settings("tree-again");
    let next = Scratch::new("tree-next");
    let (sender, events) = std::sync::mpsc::channel();
    app.start(sender);
    settled_into(&events, &mut app);
    let before = listening_at(app.tools_url().expect("tools offered for the project"));
    assert!(
        std::net::TcpStream::connect(&before).is_ok(),
        "the tools were never listening"
    );

    fs::remove_dir_all(&scratch.directory).expect("the tree going");
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline && !app.tree_has_gone() {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(200)) {
            app.handle(event);
        }
    }
    assert!(
        app.tree_has_gone(),
        "the tree went and Obelus did not hear it"
    );
    assert!(
        app.tools_url().is_none(),
        "an agent is still told about the tools of a tree that has gone"
    );
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline && std::net::TcpStream::connect(&before).is_ok() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        std::net::TcpStream::connect(&before).is_err(),
        "the tools of a tree that has gone are still listening"
    );

    support::state_of_its_own();
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    support::press(&mut app, crossterm::event::KeyCode::End);
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    support::type_text(&mut app, &next.directory.display().to_string());
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    support::press(&mut app, crossterm::event::KeyCode::Enter);

    assert_eq!(
        app.working_directory(),
        next.directory,
        "Obelus was not put on the project chosen after the last one went"
    );
    assert!(
        app.offers(Command::FileOpen),
        "the project chosen after the last one went offers nothing"
    );
    let after = listening_at(app.tools_url().expect("tools offered for the next project"));
    assert!(
        std::net::TcpStream::connect(&after).is_ok(),
        "the next project's tools are not listening"
    );
}

/// Hands the application whatever starting it stirred, until it is quiet.
///
/// The one test that wants it is Linux's, and so is this.
#[cfg(target_os = "linux")]
fn settled_into(events: &Receiver<Event>, app: &mut obelus_app::app::App) {
    while let Ok(event) = events.recv_timeout(Duration::from_millis(200)) {
        app.handle(event);
    }
}
