//! Gathering the files the picker can offer.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
};

use ignore::WalkBuilder;

use crate::event::Event;

/// How many paths to send at a time.
///
/// One message per file would wake the loop once per file and redraw a list
/// that is about to change again. One message for everything would leave the
/// picker empty for as long as the walk takes on a large tree.
const BATCH: usize = 512;

/// Walks a directory tree on its own thread, sending paths back in batches.
///
/// `generation` comes back with every batch. The picker can be closed and
/// reopened while a walk is still running, and the batches from the old one
/// have to be recognizable as stale rather than merged into the new list.
///
/// `ignored` offers the files the tree has said to ignore as well. Only the
/// ignore rules go: hidden files stay hidden either way, because `.git` is a
/// directory with one file per object in it and a reader who asked to see
/// what `.gitignore` hides did not ask for that.
///
/// A thread because `WalkBuilder` is a blocking API, and the walk of a large
/// tree is long enough that the picker has to be usable while it runs.
pub fn spawn_walk(root: &Path, generation: u64, ignored: bool, sender: Sender<Event>) {
    let root = root.to_path_buf();
    let outcome = std::thread::Builder::new()
        .name("obelus-walk".to_string())
        .spawn(move || {
            // The files the tree keeps, first and on their own: they are what
            // a reader is usually after, and this is the quick walk -- it is
            // the one that does not descend into `target`.
            let mut sent = HashSet::new();
            if !walk(&root, true, generation, &sender, &mut sent) || !ignored {
                return;
            }
            // And then the ones it does not keep, which are whatever the
            // first walk did not send. Asking a second matcher whether a path
            // is ignored would be asking the same question twice and leaving
            // the two answers free to differ; this way "ignored" means
            // exactly "the walk that obeys the rules did not offer it".
            walk(&root, false, generation, &sender, &mut sent);
        });

    if let Err(error) = outcome {
        tracing::warn!(%error, "not walking the tree");
    }
}

/// One walk over the tree, sending what it finds in batches.
///
/// `obeying` says whether the ignore rules apply. The walk that obeys them
/// remembers in `sent` what it offered; the walk that does not obey them
/// leaves those out and sends the rest, marked as ignored.
///
/// Hidden files are skipped either way. Returns whether there is still
/// anybody to send to.
fn walk(
    root: &Path,
    obeying: bool,
    generation: u64,
    sender: &Sender<Event>,
    sent: &mut HashSet<PathBuf>,
) -> bool {
    let mut batch: Vec<PathBuf> = Vec::with_capacity(BATCH);
    let mut walk = WalkBuilder::new(root);
    walk.git_ignore(obeying)
        .git_global(obeying)
        .git_exclude(obeying)
        .ignore(obeying)
        .parents(obeying);

    for entry in walk.build() {
        let entry = match entry {
            Ok(entry) => entry,
            // An unreadable directory is not worth abandoning the walk over.
            Err(error) => {
                tracing::debug!(%error, "skipping an entry");
                continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        // Relative to the root, which is what the picker shows and what the
        // reader typed to get here.
        let path = entry
            .path()
            .strip_prefix(root)
            .unwrap_or_else(|_| entry.path())
            .to_path_buf();
        match obeying {
            true => {
                sent.insert(path.clone());
            }
            false if sent.contains(&path) => continue,
            false => {}
        }
        batch.push(path);

        if batch.len() >= BATCH {
            let paths = std::mem::replace(&mut batch, Vec::with_capacity(BATCH));
            // The receiver is gone, so the loop has ended.
            if sender
                .send(Event::FilesFound {
                    generation,
                    paths,
                    ignored: !obeying,
                })
                .is_err()
            {
                return false;
            }
        }
    }

    if !batch.is_empty() {
        let _ = sender.send(Event::FilesFound {
            generation,
            paths: batch,
            ignored: !obeying,
        });
    }
    true
}
